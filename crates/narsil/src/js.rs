//! J9 + J10 + J11 — JAVASCRIPT IM BROWSER.
//!
//! Der Nutzer will JavaScript. Die Engine (`narsil-jsvm`) steht und ist gegen
//! `node` geprueft; was fehlte, war die Anbindung: `narsil` hat die
//! `<script>`-Elemente bisher nur gesammelt und nie ausgefuehrt.
//!
//! B1-B3 (01.10.2026, Rolle KOMPRESS): hier stand `narsil-js`. Der Tausch
//! auf `narsil-jsvm` war eine Textersetzung — beide Crates haben dieselben
//! Namen und dieselben Signaturen fuer alles, was diese Datei braucht
//! (`Engine`, `Grenzen`, `Wert`, `ereignis_als_wert`). GEMESSEN, warum der
//! Tausch kein Verlust ist: `narsil-js` benutzt den Rust-Stapel als
//! Fortsetzung, also kann `await` dort nicht anhalten und `yield` fehlt
//! ganz (`crates/narsil-js/tests/await_reihenfolge.rs`).
//!
//! # DER WEG, und warum er so aussieht
//!
//! ```text
//! Seite.skirpte (Quelltext + src, Dokumentreihenfolge)
//!     -> Engine (Kind-Thread, 256 MiB Stapel, Grenzen aus `Grenzen`)
//!         -> Host-Funktionen schreiben BEFEHLE in eine gemeinsame Kiste
//!             -> Hauptthread spielt die Befehle auf der Seite ab
//!                 -> neu layouten -> neu zeichnen
//! ```
//!
//! WARUM BEFEHLE und kein direkter Zugriff auf `narsil-dom`: die Engine
//! laeuft in einem Kind-Thread (`VERTRAG_JS`), und die Host-Funktionen
//! muessen `Send + 'static` sein (VERTRAG_JS, Zeile „host_funktion-
//! Closures brauchen Send"). Ein `Rc<RefCell<Document>>` waere nicht
//! `Send`; eine `Arc<Mutex<Vec<Befehl>>>` schon. Also teilen die Seiten
//! nur eine Befehlsliste, und der Hauptthread ist der einzige, der den
//! Dokumentbaum anfasst.
//!
//! # WAS DIESE ANBINDUNG KANN — und was nicht
//!
//! | J9 verlangt | hier | Wie |
//! |---|---|---|
//! | `document.getElementById` | ja | Funktion |
//! | `querySelector(All)` | ja | Funktion, `tag`, `.klasse`, `#id` |
//! | `element.textContent` (lesen/setzen) | ja, als Funktion | `elText(el)`, `elSetText(el, t)` |
//! | `innerHTML` setzen | ja, als Funktion | `elSetHtml(el, h)` (begrenzt) |
//! | `setAttribute/getAttribute` | ja, als Funktion | |
//! | `classList.add/remove/toggle` | ja, als Funktion | |
//! | `style.x = ...` | ja, als Funktion | `elSetStyle(el, "x", "12px")` |
//! | `createElement/appendChild/remove` | ja | |
//! | `addEventListener('click')` | ja | Quelltext wird beim Klick getragen |
//! | `window.location.href` (nur lesen) | ja, als Funktion | `locationHref()` |
//! | `console.log` | ja | von der Engine |
//!
//! DIE ABWEICHUNG, und sie ist eine: **Eigenschaften sind Funktionen.**
//! `el.textContent` geht ueber `document.getElementById` (W6B, der
//! HOST-OBJEKT-Weg), der alte Weg `elText(el)` bleibt daneben stehen.
//!
//! B1-B3 (01.10.2026, Rolle KOMPRESS): dieser Absatz war seit W6B
//! FALSCH und behauptete, die Engine kenne keinen Getter-Haken fuer
//! Objekteigenschatten. Sie hat einen — `crate::dom::anbinden` registriert
//! `narsil_jsvm::haken(lesen, schreiben, rufen)` und haengt es mit
//! `host_objekt` an den Motor. Erschreckend, wie lange ein Kommentar
//! einen Stand behauptet, den der Code drei Runden lang schon nicht mehr
//! hatte: die Tabelle oben („`element.textContent` (lesen/setzen) | ja,
//! als Funktion") war die richtige und dieser Absatz die falsche.
//! GEMESSENE REGEL: Wenn ein Kommentar und der Code sich widersprechen,
//! ist der Comment das Ziel, nicht der Code — sonst sucht der naechste
//! die Aenderung an der falschen Stelle.
//!
//! DIE REGEL FUER DEN REST: Alles, was eine EIGENSCHAFT ist, wird eine
//! Funktion mit dem Element als erstem Argument. So bleibt die
//! JavaScript-Seite lesbar (`elSetText(el, "hallo")` statt
//! `__el_set_text(el, "hallo")`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Hoechste Zahl Skripte, die eine Seite ausfuehrt (M3, J10/J11).
///
/// Auch die Initialisierungsskripte nach dem 32. Element ausfuehren.
/// Die Zahl bleibt begrenzt; die VM begrenzt Schritte, Heap und Laufzeit.
pub const MAX_SKRIPTE: usize = 128;

/// Hoechste Groesse eines Skript-Quelltexts in Byte (M4).
///
/// Gebuendelte Anwendungen (YouTube: etwa 10 MiB) vollstaendig laden.
/// Die Quelltextgrenze ist unabhaengig von den Ausfuehrungsgrenzen.
pub const MAX_SKRIPT_QUELLE: usize = 16 * 1024 * 1024;

/// Ereignisse, deren `on…`-Attribut an `<body>`/`<frameset>` ein Handler des
/// FENSTERS ist (HTML §8.1.8.2, „Window-reflecting body element event
/// handler set" und WindowEventHandlers).
const FENSTER_EREIGNISSE: &[&str] = &[
    "load", "error", "blur", "focus", "resize", "scroll", "afterprint", "beforeprint",
    "beforeunload", "hashchange", "languagechange", "message", "messageerror", "offline",
    "online", "pagehide", "pageshow", "popstate", "rejectionhandled", "storage",
    "unhandledrejection", "unload",
];

/// `setTimeout` (J10) — WAS DARAUS WIRD, und es ist eine Messung.
///
/// Die Engine hat `setTimeout` schon: sie stellt den Callback in eine
/// FIFO-Schlange und arbeitet sie am ENDE von `ausfuehren` ab
/// (`VERTRAG_JS`: „setTimeout als FIFO nach dem Skript (ms ignoriert)"),
/// und die Warteschlange ist durch `Grenzen::max_mikrotasks` begrenzt.
/// Ein Skript kann also keinen Timer-Sturm bauen, der den Browser
/// aufhaelt — die Grenze sitzt im Motor, und der Browser muss sie nicht
/// noch einmal.
///
/// WAS ES NICHT GIBT: eine WIRKLICHE Uhr. `setTimeout(f, 3000)` laeuft
/// sofort. Das ist eine Abweichung von `window.setTimeout` und die
/// einzige, die J10 zu diesem Punkt zu melden hat — eine Ereignisschleife
/// mit Weckzeit waere ein eigener Thread pro Timer.

/// Was ein Skript mit dem Dokument macht.
///
/// # WARUM EIN BEFEHL UND KEIN DIREKTER ZUGRIFF
///
/// Die Engine laeuft in einem Kind-Thread, und ihre Host-Closures
/// muessen `Send` sein (`VERTRAG_JS`). Ein `Rc<RefCell<Document>>` ist
/// das nicht. Ein `Arc<Mutex<Vec<Befehl>>>` schon — und der Vorteil ist
/// groesser als die Umstaende: **die Seite wird nur an EINER Stelle
/// veraendert**, naemlich beim Abspielen der Befehle auf dem
/// Hauptthread. Ein Skript, das mitten in der Aenderung abbricht (Zeit-
/// limit), hinterlaesst eine halbe Aenderung; das ist sichtbar und
/// pruefbar, statt ein `Document` im Kind-Thread zu zerlegen.
#[derive(Debug, Clone, PartialEq)]
pub enum Befehl {
    Canvas { knoten: u32, breite: u32, hoehe: u32, pixels: Arc<Vec<u8>> },
    /// Text eines Knotens setzen (`el.textContent = …`).
    Text {
        /// Der Knoten (Index in der Knotenliste).
        knoten: u32,
        /// Der neue Text.
        text: String,
    },
    /// Ein Attribut setzen (`setAttribute`).
    Attribut {
        knoten: u32,
        name: String,
        wert: String,
    },
    /// Ein Attribut entfernen (`removeAttribute`).
    AttributWeg { knoten: u32, name: String },
    /// Eine Klasse setzen (`classList.add/remove/toggle`).
    Klasse {
        knoten: u32,
        name: String,
        da: bool,
        /// `true` fuer `toggle` (also: umschalten statt setzen).
        umschalten: bool,
    },
    /// Eine Inline-Eigenschaft setzen (`style.x = …`).
    Stil {
        knoten: u32,
        eigenschaft: String,
        wert: String,
    },
    /// Ein neues Element anlegen.
    Neu {
        /// Die neue Knotennummer.
        knoten: u32,
        /// Der Tagname (klein).
        tag: String,
    },
    /// Ein Kind anhaengen.
    Anhaengen { eltern: u32, kind: u32 },
    /// Ein Element entfernen (mit seinen Kindern).
    Entfernen { knoten: u32 },
    /// InnerHTML setzen (begrenzt, ueber `narsil-fmt-html` geparst).
    Html { knoten: u32, html: String },
    /// Ein Ereignis-Handler anmelden (Quelltext, wird beim Ereignis getragen).
    Horcher {
        knoten: u32,
        typ: String,
        /// Der Quelltext der Funktion, z. B. `function(){ … }`.
        quelle: String,
    },
}

/// Die geteilte Kiste zwischen Engine und Hauptthread.
///
/// `Send` ist die Bedingung fuer alles hier: die Engine nimmt sie in
/// ihren Kind-Thread mit.
#[derive(Debug, Default)]
pub struct Kiste {
    /// Die Befehle in der Reihenfolge, in der sie entstanden sind.
    pub befehle: Vec<Befehl>,
    /// Die naechste freie Knotennummer fuer `createElement`.
    ///
    /// M3: sie steigt monoton und wird beim Abspielen auf die groesste
    /// belegte Nummer gesetzt. Ein Skript kann sie also nicht zurueck-
    /// setzen und Knoten ueberschreiben.
    naechste: u32,
    /// Die Horcher je Knoten und Typ (die Befehle sind der Log).
    ///
    /// W6B: Element (Vorlauf-Nummer) -> Host-Id.
    ///
    /// GEMESSEN, warum eine Tabelle und kein freier Zaehler:
    /// `getElementById` gibt eine Nummer zurueck, und dieselbe Nummer
    /// muss beim naechsten `querySelector` wieder dasselbe Element
    /// bezeichnen. Die Zuordnung ist also fuer die ganze Seite fest.
    ///
    /// M3: hoechstens `MAX_KNOTEN` (4096) Eintraege.
    pub host_ids: HashMap<usize, u32>,
    /// Die naechste freie Host-Id (M3: `MAX_HOST_IDS`).
    pub naechste_host_id: u32,
    /// W6D: der WERT ist ein `crate::handler::Handler` — ein
    /// Funktionswert (mit Closure) oder ein Quelltext (W6). Vorher stand
    /// hier ein `String`, und eine Funktion liess sich nicht ablegen:
    /// `addEventListener('click', () => {…})` kam als `undefined` an
    /// (GEMESSEN 13:40), also lief kein Handler.
    pub horcher: HashMap<(u32, String), crate::handler::Handler>,
    /// `true`, wenn sich das Dokument geaendert hat (J10: neu zeichnen).
    pub geaendert: bool,
    /// Der zuletzt ausgeloeste Klick (fuer `__event`).
    pub klick: Option<(u32, f32, f32)>,
    /// Die Adresse der Seite (fuer `location.href`).
    pub adresse: String,
    /// Navigationswunsch aus Skript (`location.assign/replace`, `history`,
    /// `reload`): der Hauptthread holt ihn ab und navigiert WIRKLICH.
    /// J-CACHE (04.10.2026, Weisung CACHE_27): die Engine kann nicht
    /// navigieren — sie legt den Wunsch hier ab, JS liest ihn.
    pub navigieren_wunsch: Option<String>,
    /// Sitzungsverlauf: Adressen-Stapel + Position (history.length,
    /// pushState/replaceState/back/forward/go; popstate feuert NICHT —
    /// dokumentiert).
    pub verlauf: Vec<String>,
    pub verlauf_pos: usize,
    /// Web-Speicher je Herkunft (localStorage persistent, sessionStorage
    /// sitzungsbezogen). 5-MB-Grenze je Speicher (Spec-Naeherung).
    pub speicher_lokal: std::collections::HashMap<String, String>,
    pub speicher_sitzung: std::collections::HashMap<String, String>,
    /// Herkunft, unter der gespeichert wird (Schema+Host, gesetzt beim
    /// Binden aus der Adresse).
    pub herkunft: String,
    /// Zwischenablage (navigator.clipboard: synchron, dokumentiert —
    /// der Host kann kein Promise bauen).
    pub zwischenablage: String,
    /// Fenster-Ansicht fuer matchMedia (Sitzung fuellt; Vorgabe 1280x800,
    /// light, no-preference — JS liest das im BRETT, CACHE_28).
    pub fenster_breite: f64,
    pub fenster_hoehe: f64,
    pub farbschema: String,
    pub bewegung: String,
    /// `id`-Attribut -> Knotennummer (J9: `document.getElementById`).
    ///
    /// GEMESSEN, warum die Tabelle hier steht und nicht im Dokument:
    /// `narsil-dom` speichert Attribute NICHT (sie werden beim Bau in den
    /// Stil uebersetzt), und `narsil-dom` gehoert einer anderen Rolle.
    /// `narsil-page` legt sie beim Bau an, narsil fuellt sie hier.
    pub ids: HashMap<String, u32>,
    // KEIN `quelle` und KEIN `spans` mehr (11:55). GEMESSEN, warum: der
    // erste Weg ging ueber den HTML-QUELLTEXT, und dafuer brauchte er die
    // Quellposition jedes Knotens. Die gibt es nicht —
    // `narsil_fmt_html::HtmlNode` hat kein `span`, und `narsil_dom::Span` ist
    // beim Bau ueberall `Span::EMPTY`. Stattdessen nummeriert
    // `narsil_page::Patch` die Knoten in Dokumentreihenfolge, und DAS ist
    // der Zahlenraum, den JavaScript sieht.
    /// Der CSS-Baum fuer `querySelector` (J9).
    ///
    /// GEMESSEN, warum eine Kopie und kein Verweis: die Engine laeuft in
    /// einem Kind-Thread und darf den Dokumentbaum nicht anfassen. Der
    /// Baum ist eine flache Liste mit Tag, id und Klassen — bei 2000
    /// Knoten rund 100 KiB, einmal pro Skriptlauf kopiert.
    pub baum: narsil_page::CssBaum,
    /// CSS-Index -> DOM-Index (`narsil-page::Seite::css_zu_dom`).
    ///
    /// DIE EINE WICHTIGE BRUECKE in dieser Datei: `querySelector`
    /// zaehlt im `CssBaum`, `getElementById` liefert DOM-Nummern, und
    /// die Befehle (Text, Attribut, Klasse) brauchen die DOM-Nummer,
    /// weil NUR der DOM-Knoten eine `Span` hat. Wer die beiden
    /// Indexraeume vermischt, aendert beim ersten Selektor das falsche
    /// Element — und faellt es erst auf, wenn die Seite anders gebaut
    /// ist als im Test.
    pub css_zu_dom: HashMap<u32, u32>,
    /// DOM-Knotennummer -> Vorlauf-Nummer (`narsil_page::Seite::dom_zu_ord`).
    ///
    /// DIE DRITTE Bruecke, und sie wird gebraucht, weil der KLICK aus dem
    /// Layout-Baum eine `NodeId` liefert, ein Skript aber mit der
    /// Vorlauf-Nummer arbeitet. GEMESSEN 12:55: ohne diese Bruecke fand
    /// der Klick den Handler nie — der Handler war angemeldet, das
    /// `horcher`-Verzeichnis hatte ihn, und der Schluessel sah anders aus.
    pub dom_zu_ord: HashMap<u32, u32>,
    /// The messaging context of the page (workers, ports, channels); made
    /// on first use, ended when the page goes (`worker.rs`).
    pub worker: Arc<crate::worker::ContextSlot>,
}

impl Kiste {
    /// Leert die Kiste und merkt sich die Adresse.
    ///
    /// GEMESSEN, warum `mem::take` und nicht `clear()`: der Hauptthread
    /// nimmt die Befehle, spielt sie ab und legt die Kiste zurueck. Ein
    /// `clear()` wuerde die Nummerierung erhalten — was hier richtig
    /// waere — aber es gibt keine Nummerierung zu erhalten, weil sie in
    /// der Kiste steckt. Also: neue Kiste, alte Zahlen werden beim
    /// Abspielen aus der Seite gelesen.
    pub fn neu(adresse: &str) -> Self {
        Self {
            befehle: Vec::new(),
            naechste: 0,
            horcher: HashMap::new(),
            host_ids: HashMap::new(),
            naechste_host_id: 0,
            geaendert: false,
            klick: None,
            adresse: adresse.to_string(),
            navigieren_wunsch: None,
            verlauf: Vec::new(),
            verlauf_pos: 0,
            speicher_lokal: HashMap::new(),
            speicher_sitzung: HashMap::new(),
            herkunft: String::new(),
            zwischenablage: String::new(),
            fenster_breite: 1280.0,
            fenster_hoehe: 800.0,
            farbschema: "light".to_string(),
            bewegung: "no-preference".to_string(),
            ids: HashMap::new(),
            baum: narsil_page::CssBaum::default(),
            css_zu_dom: HashMap::new(),
            dom_zu_ord: HashMap::new(),
            worker: crate::worker::ContextSlot::page(&crate::worker::origin_of(adresse)),
        }
    }

    /// Nimmt die Befehle heraus und leert die Kiste.
    ///
    /// M4: die Befehle sind eine Liste aus der Eingabe (die Seite kann
    /// beliebig viele `setAttribute` aufrufen). Die Grenze sitzt hier:
    /// [`MAX_BEFEHLE`] Befehle je Skriptlauf. Wer mehr erzeugt, hat mehr
    /// als 65.536 Aenderungen an einem Dokument — das ist kein Dokument
    /// mehr, und ein Browser, der das annimmt, ist ein Speicherleck mit
    /// Benutzeroberfläche.
    pub fn nimm_befehle(&mut self) -> Vec<Befehl> {
        std::mem::take(&mut self.befehle)
    }

    /// Eine neue Knotennummer.
    ///
    /// M3: `MAX_KNOTEN` (4096) ist die Grenze, weil die Knotenliste im
    /// Hauptthread eine Indexstruktur ist und ein Skript, das 100.000
    /// Elemente anlegt, eine Seite baut, die niemand lesen kann.
    fn naechste_knoten(&mut self) -> Option<u32> {
        if self.naechste >= MAX_KNOTEN {
            return None;
        }
        let n = self.naechste;
        self.naechste += 1;
        Some(n)
    }

    /// Setzt den Zaehler nach dem Abspielen (die Seite weiss die Zahlen).
    pub fn setze_zaehler(&mut self, wert: u32) {
        self.naechste = wert.min(MAX_KNOTEN);
    }

    /// Der Zaehler (fuer die naechste Seite).
    pub fn zaehler(&self) -> u32 {
        self.naechste
    }

    /// Ein Befehl, mit Grenze.
    ///
    /// M4: die Befehlsliste wächst aus der Eingabe (dem Skript). Die
    /// Grenze ist [`MAX_BEFEHLE`], und sie wird VOR dem Schieben geprueft.
    fn befehl(&mut self, b: Befehl) -> bool {
        if self.befehle.len() >= MAX_BEFEHLE {
            return false;
        }
        // GEMESSEN, warum der Horcher den Baum NICHT veraendert: ein
        // `addEventListener` aendert nichts, was man zeichnen kann. Haette
        // ich hier immer `true` gesetzt, wuerde eine Seite, die nur einen
        // Zuhoerer anmeldet, beim ersten Klick den ganzen Baum neu bauen —
        // der Befehl `Horcher` ist die einzige Veraenderung, die NICHT
        // zeichnet.
        if b.veraendert_baum() {
            self.geaendert = true;
        }
        self.befehle.push(b);
        true
    }

    /// Eine Klassennummer holen (oeffentlich fuer `createElement`).
    pub fn knoten_holen(&mut self) -> Option<u32> {
        self.naechste_knoten()
    }
}

/// Der Attributname fuer `classList`-Anweisungen im Patch.
///
/// GEMESSEN, warum ein Name mit Steuerzeichen: `\u{1}klasse` kann in
/// keinem echten Attributnamen vorkommen (HTML-Attributnamen erlauben
/// kein Steuerzeichen). Ein `setAttribute("klasse", …)` von einem Skript
/// kann den Marker also nicht ueberschreiben — das ist der Grund fuer das
/// Steuerzeichen und nicht die Schoenheit.
pub const KLASSE_MARKER: &str = "\u{1}klasse";

/// Der Attributname fuer `style`-Anweisungen im Patch (siehe
/// [`KLASSE_MARKER`]).
pub const STIL_MARKER: &str = "\u{1}stil";

/// Hoechste Zahl Befehle je Skriptlauf (M4).
///
/// GEMESSEN, warum 65.536: das sind 65.536 Aenderungen an einem
/// Dokument. Jede davon kostet spaeter beim Abspielen mindestens einen
/// Knotenbesuch; ein Skript, das mehr erzeugt, baut eine Seite, die der
/// Layout-Baum nicht mehr in die Zeit bekommt.
pub const MAX_BEFEHLE: usize = 65_536;

/// Hoechste Zahl Knoten, die ein Skript anlegen darf (M3).
///
/// GEMESSEN, warum 4096: eine Seite mit 4096 Elementen ist gross (rund
/// 40 Bildschirme). Eine mit mehr ist kein Dokument.
pub const MAX_KNOTEN: u32 = 4096;

/// Hoechste Laenge eines Attributwerts in Byte (M4).
///
/// GEMESSEN, warum 16 KiB: der laengste Attributwert in einer Webseite
/// ist eine eingebettete `data:`-URL — und die gehoert in `src`, nicht
/// in ein Attribut, das der Rasterizer lesen kann.
pub const MAX_ATTRIBUT: usize = 16 * 1024;

/// Die geteilte Kiste als `Arc` — die Form, in der die Host-Funktionen
/// sie bekommen.
pub type Geteilt = Arc<Mutex<Kiste>>;

/// Kuerzt einen Wert auf eine Grenze (M4).
///
/// `None`, wenn der Wert zu lang ist: dann wird er NICHT gekuerzt,
/// sondern abgelehnt. Ein gekuerztes `style.width` waere ein Wert, den
/// das Layout liest und nicht versteht — ein abgelehnter fuehrt zu
/// "unbekannte Eigenschaft", was eine ehrliche Meldung ist.
pub fn wert_pruefen(text: &str, grenze: usize) -> Result<String, String> {
    if text.len() > grenze {
        return Err(std::format!(
            "Wert zu lang: {} Byte, erlaubt {grenze}",
            text.len()
        ));
    }
    Ok(text.to_string())
}

// ===========================================================================
// DIE JS-SEITE: Host-Funktionen (J9)
// ===========================================================================

use narsil_jsvm::{Engine, Grenzen, Wert};

/// Meldet eine Engine-Meldung als JS-Fehler.
///
/// WARUM ein `Err(String)`: `host_funktion` liefert `Result<Wert, String>`,
/// und der String wird in der Engine zum Fehlerwert. Ein Skript, das
/// eine unbekannte ID benutzt, soll einen `Error` sehen — nicht
/// `undefined` und nicht einen Absturz.
fn fehler(text: &str) -> Result<Wert, String> {
    Err(text.to_string())
}

/// Die Zahl aus einem `Wert` (eine Elementnummer).
///
/// GEMESSEN, warum `f64`: JS-Zahlen sind `f64`, und eine Elementnummer
/// kommt aus dem Skript. `1.5` ist keine Nummer — der Test sagt es.
fn als_nummer(w: &Wert, was: &str) -> Result<u32, String> {
    match w {
        Wert::Zahl(n) if *n >= 0.0 && n.is_finite() => Ok(*n as u32),
        //
        // W6B: EIN HOST-OBJEKT ist auch eine Nummer — seine
        // Vorlauf-Nummer steht im Feld `__ord`.
        //
        // GEMESSEN, warum (17:35): `getElementById` gibt seit W6B ein
        // Host-Objekt zurueck (sonst kaeme `z.textContent` nicht durch),
        // und der W6-Weg `elSetText(getElementById("z"), …)` gab danach
        // „erwartet eine Nummer". Die beiden Wege teilen sich die Zahl.
        Wert::Gegenstand(felder) => felder
            .iter()
            .find(|(n, _)| n == "__ord")
            .and_then(|(_, v)| match v {
                Wert::Zahl(n) if *n >= 0.0 && n.is_finite() => Some(*n as u32),
                _ => None,
            })
            .ok_or_else(|| std::format!("{was} erwartet eine Nummer, war ein Objekt ohne __ord")),
        andere => Err(std::format!(
            "{was} erwartet eine Nummer, war {}",
            andere.als_text()
        )),
    }
}

/// Der Text aus einem `Wert` (fuer `document.getElementById("x")`).
fn als_text(w: &Wert) -> String {
    match w {
        Wert::Text(t) => t.clone(),
        andere => andere.als_text(),
    }
}

/// P3-Helfer (J-CACHE 04.10.2026, Weisung CACHE_27). Alle duenn,
/// dokumentiert, ohne fremde Dateien.

/// Herkunft (Schema+Host, klein) aus Adresse — Schluessel fuer Speicher.
fn herkunft_aus_adresse(adresse: &str) -> String {
    let t = adresse.trim().to_lowercase();
    let rest = t.split("://").nth(1).unwrap_or(&t);
    let host = rest.split('/').next().unwrap_or("leer");
    format!("narsil-{}", host.replace([':', '.'], "_"))
}

/// Speicher-Datei: ~/.config/narsil/storage/<herkunft>.json.
fn speicher_pfad(herkunft: &str) -> std::path::PathBuf {
    crate::plattform::konfiguration()
        .join("storage")
        .join(format!("{herkunft}.json"))
}

/// Laden (fehlend/fehlerhaft -> leer, nie ein Absturz).
fn speicher_laden(herkunft: &str) -> std::collections::HashMap<String, String> {
    let mut karte = std::collections::HashMap::new();
    let Ok(text) = std::fs::read_to_string(speicher_pfad(herkunft)) else {
        return karte;
    };
    // Winziges JSON {"k":"v"} — nur die eigene Schreibform (kein Parser).
    let t = text.trim();
    if !t.starts_with('{') || !t.ends_with('}') {
        return karte;
    }
    let innen = &t[1..t.len() - 1];
    let mut rest = innen;
    while let Some((k, r)) = json_zeichen(rest) {
        rest = r.trim_start();
        if !rest.starts_with(':') {
            break;
        }
        rest = rest[1..].trim_start();
        if let Some((v, r2)) = json_zeichen(rest) {
            karte.insert(k, v);
            rest = r2.trim_start();
            if rest.starts_with(',') {
                rest = rest[1..].trim_start();
            }
        } else {
            break;
        }
    }
    karte
}

/// Eine JSON-Zeichenkette lesen (mit \ und \").
fn json_zeichen(s: &str) -> Option<(String, &str)> {
    let s = s.trim_start();
    if !s.starts_with('"') {
        return None;
    }
    let mut out = String::new();
    let mut entkommen = false;
    for (i, c) in s[1..].char_indices() {
        if entkommen {
            out.push(match c {
                '"' => '"',
                '\\' => '\\',
                'n' => '\n',
                't' => '\t',
                _ => c,
            });
            entkommen = false;
        } else if c == '\\' {
            entkommen = true;
        } else if c == '"' {
            return Some((out, &s[1 + i + 1..]));
        } else {
            out.push(c);
        }
    }
    None
}

/// Schreiben (Verzeichnis anlegen; Fehler still — Speicher ist
/// best-effort, das Skript laeuft weiter).
fn speicher_schreiben(herkunft: &str, karte: &std::collections::HashMap<String, String>) {
    fn flucht(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    }
    let pfad = speicher_pfad(herkunft);
    if let Some(eltern) = pfad.parent() {
        let _ = std::fs::create_dir_all(eltern);
    }
    let mut teile = Vec::new();
    let mut schluessel: Vec<&String> = karte.keys().collect();
    schluessel.sort();
    for k in schluessel {
        teile.push(format!("\"{}\":\"{}\"", flucht(k), flucht(&karte[k])));
    }
    let _ = std::fs::write(pfad, format!("{{{}}}", teile.join(",")));
}

/// Groesse in Bytes (5-MB-Grenze, Spec-Naeherung: Schlüssel + Werte).
fn speicher_groesse(karte: &std::collections::HashMap<String, String>) -> usize {
    karte.iter().map(|(k, v)| k.len() + v.len()).sum()
}

/// Standort-Teile aus Adresse (einfache Teilung, kein URL-Modell —
/// der Motor hat es, der Host braucht nur die Felder).
fn standort_teile(
    adresse: &str,
) -> (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
) {
    // (href, origin, protocol, host, hostname, port, pathname, rest[search+hash])
    let href = adresse.to_string();
    let (schema, rest) = match adresse.split_once("://") {
        Some((s, r)) => (format!("{s}:"), r.to_string()),
        None => ("".to_string(), adresse.to_string()),
    };
    let (hostport, pfad) = match rest.split_once('/') {
        Some((h, p)) => (h.to_string(), format!("/{p}")),
        None => (rest.clone(), "/".to_string()),
    };
    let (hostname, port) = match hostport.split_once(':') {
        Some((h, p)) => (h.to_string(), p.to_string()),
        None => (hostport.clone(), String::new()),
    };
    let origin = if schema.is_empty() {
        String::new()
    } else {
        format!("{schema}//{hostport}")
    };
    (
        href,
        origin,
        schema,
        hostport,
        hostname,
        port,
        pfad.clone(),
        pfad,
    )
}

/// location-Gegenstand (Stand beim Binden).
fn standort_gegenstand(adresse: &str) -> Wert {
    let (href, origin, protocol, host, hostname, port, pfad_voll, suchhash) =
        standort_teile(adresse);
    // J-CACHE: pathname OHNE query/hash (war versehentlich der volle Pfad).
    let pathname = pfad_voll
        .split(['?', '#'])
        .next()
        .unwrap_or("/")
        .to_string();
    let (search, hash) = match suchhash.split_once('#') {
        Some((s, h)) => (s.to_string(), format!("#{h}")),
        None => (suchhash.clone(), String::new()),
    };
    let search = match search.split_once('?') {
        Some((_, q)) => format!("?{q}"),
        None => String::new(),
    };
    Wert::Gegenstand(vec![
        ("href".to_string(), Wert::Text(href)),
        ("origin".to_string(), Wert::Text(origin)),
        ("protocol".to_string(), Wert::Text(protocol)),
        ("host".to_string(), Wert::Text(host)),
        ("hostname".to_string(), Wert::Text(hostname)),
        ("port".to_string(), Wert::Text(port)),
        ("pathname".to_string(), Wert::Text(pathname)),
        ("search".to_string(), Wert::Text(search)),
        ("hash".to_string(), Wert::Text(hash)),
        (
            "assign".to_string(),
            Wert::Nativ("standortZuweisen".to_string()),
        ),
        (
            "replace".to_string(),
            Wert::Nativ("standortErsetzen".to_string()),
        ),
        ("reload".to_string(), Wert::Nativ("standortNeu".to_string())),
    ])
}

/// assign/replace/reload: navigieren_wunsch + adresse (der Hauptthread
/// navigiert WIRKLICH; location-Gegenstand bleibt Stand — dokumentiert).
fn standort_handlung(kiste: &Geteilt, name: &str, args: &[Wert]) -> Result<Wert, String> {
    let mut w = kiste.lock().expect("Kiste");
    if name == "standortNeu" {
        w.navigieren_wunsch = Some(w.adresse.clone());
        return Ok(Wert::Undefiniert);
    }
    let ziel = als_text(args.first().map_or(&Wert::Undefiniert, |v| v));
    if ziel.is_empty() {
        return fehler("location braucht eine Adresse");
    }
    w.adresse = ziel.clone();
    w.navigieren_wunsch = Some(ziel);
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Adresse **vor** dem `push` in eine
    // eigene Variable kommt (E0502): `w.verlauf.push(w.adresse.clone())` leiht
    // `w` **unveraenderlich** fuer das Argument und **veraenderlich** fuer
    // `push` zur selben Zeit — der Compiler laesst das nicht zu, und er hat
    // recht. Zwei Zeilen statt einer ist hier die *kleinere* Aenderung als die
    // Alternative (die Reihenfolge umzubauen und `adresse` danach zu setzen),
    // weil sie die Reihenfolge der drei Zuweisungen unangetastet laesst.
    let adresse_jetzt = w.adresse.clone();
    w.verlauf.push(adresse_jetzt);
    w.verlauf_pos = w.verlauf.len() - 1;
    Ok(Wert::Undefiniert)
}

/// Verlauf: pushState/replaceState/back/forward/go. popstate feuert
/// NICHT (kein Ereignisweg von hier — dokumentiert).
fn umgebung(kiste: &Geteilt, name: &str, args: &[Wert]) -> Result<Wert, String> {
    let mut w = kiste.lock().expect("Kiste");
    match name {
        "verlaufSchieben" | "verlaufErsetzen" => {
            let url = args.get(2).map(|v| als_text(v)).unwrap_or_default();
            if !url.is_empty() {
                // Relativ zur Adresse aufloesen (einfach: absolutes oder
                // Pfad-Anhang).
                let neu = if url.contains("://") {
                    url
                } else if url.starts_with('/') {
                    let basis = w
                        .adresse
                        .split("://")
                        .nth(1)
                        .unwrap_or("")
                        .split('/')
                        .next()
                        .unwrap_or("");
                    let schema = w.adresse.split("://").next().unwrap_or("http");
                    format!("{schema}://{basis}{url}")
                } else {
                    url
                };
                w.adresse = neu.clone();
                if name == "verlaufSchieben" {
                    //
                    // GEMESSENE ENTSCHEIDUNG, warum die **Position** und der
                    // **Index** vorher in eigenen Variablen stehen (E0502,
                    // zweimal): `truncate(w.verlauf_pos + 1)` und
                    // `get_mut(w.verlauf_pos)` leihen `w` genauso doppelt wie
                    // der Fall in `standort_handlung`. Die Leihmutter liegt
                    // hier ueber den ganzen `match`-Zweig, also loest man sie
                    // nicht durch Umstellen, sondern indem man die Zahl vorher
                    // liest.
                    let bis = w.verlauf_pos + 1;
                    w.verlauf.truncate(bis);
                    w.verlauf.push(neu);
                    w.verlauf_pos = w.verlauf.len() - 1;
                } else {
                    let pos = w.verlauf_pos;
                    if let Some(e) = w.verlauf.get_mut(pos) {
                        *e = neu;
                    }
                }
            }
            Ok(Wert::Undefiniert)
        }
        "verlaufZurueck" => {
            if w.verlauf_pos > 0 {
                w.verlauf_pos -= 1;
                w.adresse = w.verlauf[w.verlauf_pos].clone();
                w.navigieren_wunsch = Some(w.adresse.clone());
            }
            Ok(Wert::Undefiniert)
        }
        "verlaufVor" => {
            if w.verlauf_pos + 1 < w.verlauf.len() {
                w.verlauf_pos += 1;
                w.adresse = w.verlauf[w.verlauf_pos].clone();
                w.navigieren_wunsch = Some(w.adresse.clone());
            }
            Ok(Wert::Undefiniert)
        }
        "verlaufGehen" => {
            let delta = match args.first() {
                Some(Wert::Zahl(n)) => *n as isize,
                _ => 0,
            };
            let neu = (w.verlauf_pos as isize + delta)
                .clamp(0, w.verlauf.len().saturating_sub(1) as isize)
                as usize;
            if neu != w.verlauf_pos && !w.verlauf.is_empty() {
                w.verlauf_pos = neu;
                w.adresse = w.verlauf[neu].clone();
                w.navigieren_wunsch = Some(w.adresse.clone());
            }
            Ok(Wert::Undefiniert)
        }
        "zwischenablageSchreiben" => {
            w.zwischenablage = als_text(args.first().map_or(&Wert::Undefiniert, |v| v));
            Ok(Wert::Undefiniert)
        }
        "zwischenablageLesen" => Ok(Wert::Text(w.zwischenablage.clone())),
        "cssTrifftZu" => {
            // Release 12.10.2026: CSS.supports nach CSS Conditional 3 §8.1 —
            // `(prop, wert)` prueft den Wert mit der CSSOM-Grammatik, ein
            // Argument ist eine <supports-condition>. GEMESSEN vorher: eine
            // Namensliste aus 64 Eigenschaften ohne Wertpruefung.
            if args.is_empty() {
                return Err("TypeError: Failed to execute 'supports' on 'CSS': 1 argument required, but only 0 present.".to_string());
            }
            if args.len() >= 2 {
                let eig = als_text(&args[0]);
                let wert = als_text(&args[1]);
                return Ok(Wert::Wahr(narsil_css::cssom::bedingung::eigenschaft(&eig, &wert)));
            }
            let ganz = als_text(&args[0]);
            let selektor = |s: &str| narsil_css::cssom::selektor_gueltig(s);
            Ok(Wert::Wahr(narsil_css::cssom::bedingung::bedingung_text(&ganz, &selektor)))
        }
        _ => fehler("unbekannte Umgebung"),
    }
}

/// Dialoge: Protokoll in die Konsole (kein Fenster von hier);
/// confirm=false ist die SICHERE Vorgabe, prompt gibt Vorgabe/"".
fn dialog(kiste: &Geteilt, name: &str, args: &[Wert]) -> Result<Wert, String> {
    let _ = kiste;
    let text = als_text(args.first().map_or(&Wert::Undefiniert, |v| v));
    match name {
        "dialogMelden" => {
            eprintln!("[Seite] alert: {text}");
            Ok(Wert::Undefiniert)
        }
        "dialogBestaetigen" => {
            eprintln!("[Seite] confirm: {text} -> false (sichere Vorgabe)");
            Ok(Wert::Wahr(false))
        }
        _ => {
            let vorgabe = args.get(1).map(|v| als_text(v)).unwrap_or_default();
            eprintln!("[Seite] prompt: {text} -> Vorgabe");
            Ok(Wert::Text(vorgabe))
        }
    }
}

/// Speicher-Schnittstelle (je Speicher): getItem/setItem/removeItem/
/// clear/key/length. 5 MB je Speicher (Spec-Naeherung, Quota-Fehler als
/// Text). localStorage persistent (Datei), sessionStorage sitzungsbezogen.
fn speicher(kiste: &Geteilt, lokal: bool, name: &str, args: &[Wert]) -> Result<Wert, String> {
    let mut w = kiste.lock().expect("Kiste");
    // length steht im Gegenstand als Stand (0) — live geht nicht ohne
    // Engine-Zugriff (dokumentiert, CACHE_27). Die Methoden schon.
    let herkunft = w.herkunft.clone();
    let karte = if lokal {
        &mut w.speicher_lokal
    } else {
        &mut w.speicher_sitzung
    };
    let kurz = name.strip_prefix("speicher").unwrap_or(name);
    let kurz = kurz
        .strip_suffix("Lokal")
        .or(kurz.strip_suffix("Sitzung"))
        .unwrap_or(kurz);
    match kurz {
        "Holen" => {
            let k = als_text(args.first().map_or(&Wert::Undefiniert, |v| v));
            Ok(karte
                .get(&k)
                .map(|v| Wert::Text(v.clone()))
                .unwrap_or(Wert::Null))
        }
        "Setzen" => {
            let k = als_text(args.first().map_or(&Wert::Undefiniert, |v| v));
            let v = als_text(args.get(1).map_or(&Wert::Undefiniert, |vv| vv));
            let alt = karte.insert(k.clone(), v.clone());
            if speicher_groesse(karte) > 5_242_880 {
                if let Some(a) = alt {
                    karte.insert(k, a);
                } else {
                    karte.remove(&als_text(args.first().unwrap()));
                }
                return fehler("QuotaExceededError: Speicher voll (5 MB)");
            }
            if lokal {
                speicher_schreiben(&herkunft, karte);
            }
            Ok(Wert::Undefiniert)
        }
        "Entfernen" => {
            let k = als_text(args.first().map_or(&Wert::Undefiniert, |v| v));
            karte.remove(&k);
            if lokal {
                speicher_schreiben(&herkunft, karte);
            }
            Ok(Wert::Undefiniert)
        }
        "Leeren" => {
            karte.clear();
            if lokal {
                speicher_schreiben(&herkunft, karte);
            }
            Ok(Wert::Undefiniert)
        }
        "Schluessel" => {
            let n = match args.first() {
                Some(Wert::Zahl(x)) => *x as usize,
                _ => usize::MAX,
            };
            let mut ks: Vec<&String> = karte.keys().collect();
            ks.sort();
            Ok(ks
                .get(n)
                .map(|k| Wert::Text((*k).clone()))
                .unwrap_or(Wert::Null))
        }
        // Release 13.10.2026: `length` und die Schluessel fuer den Storage-
        // Proxy im Vorspann (benannte Eigenschaften, WebIDL §3.9).
        "Laenge" => Ok(Wert::Zahl(karte.len() as f64)),
        "Liste" => {
            let mut ks: Vec<&String> = karte.keys().collect();
            ks.sort();
            Ok(Wert::Feld(ks.into_iter().map(|k| Wert::Text(k.clone())).collect()))
        }
        _ => fehler("unbekannter Speicher"),
    }
}

/// Haengt die DOM-Anbindung an eine Engine.
/// Haengt die DOM-Anbindung an eine Engine.
///
/// # DIE LISTE
///
/// Jede Funktion nimmt das Element als ERSTES Argument (siehe Modulkopf).
/// Die Namen sind absichtlich kurz und deutsch, damit ein Skript im Bild
/// lesbar bleibt: `elSetText(el, "hallo")` sagt mehr als
/// `__document_set_text(el, "hallo")`.
///
/// # SEND
///
/// Die Closures nehmen `Geteilt` mit (das ist `Send`); ohne das kaeme
/// die Engine gar nicht erst in ihren Kind-Thread (`VERTRAG_JS`).
pub fn anbinden(engine: &mut Engine, kiste: Geteilt) {
    // W6B: das Standard-DOM (Host-Objekte). Die alten `el*`-Funktionen
    // bleiben — sie sind der Weg fuer Skripte, die ohne `document`
    // auskommen, und die beiden Wege teilen sich die Kiste.
    let stand = std::sync::Arc::new(std::sync::Mutex::new(crate::dom::Stand {
        befehle: Vec::new(),
        document_id: crate::dom::MAX_HOST_IDS,
        adresse: String::new(),
        ..Default::default()
    }));
    anbinden_mit_stand(engine, kiste, stand);
}

/// Wie [`anbinden`], aber mit einem gegebenen DOM-Stand.
///
/// WARUM ZWEI FUNKTIONEN: der Stand ist der Zustand, den JavaScript
/// sieht, und er wird vom HAUPTTHREAD gefuellt (die `id`-Tabelle, die
/// Tags, die Adresse). Ohne ihn kann ein Getter nichts beantworten.
/// Release 09.10.2026: Web-APIs in JavaScript, vor den Seitenskripten.
pub(crate) const VORSPANN_JS: &str = include_str!("vorspann.js");
/// Release 15.10.2026: CSS Animations, CSS Transitions and Web Animations,
/// loaded on first use (`__narsilAnimQuelle`, see `vorspann.js`).
pub(crate) const ANIMATIONEN_JS: &str = include_str!("animationen.js");

pub fn anbinden_mit_stand(engine: &mut Engine, kiste: Geteilt, stand: crate::dom::Geteilt) {
    // --- document ------------------------------------------------------
    engine.host_funktion("getElementById", {
        let k = kiste.clone();
        let st = stand.clone();
        move |args: &[Wert]| {
            let id = als_text(args.first().map_or(&Wert::Undefiniert, |v| v));
            // W6B: EIN GETElementById FUER BEIDE WEGE. Der Standard-Weg
            // gibt ein HOST-OBJEKT zurueck (`{__host: id}`), damit
            // `z.textContent = …` ueber den Getter/Setter laeuft. Der alte
            // Weg (die Zahl selbst) bleibt fuer `elSetText(z, …)`.
            //
            // GEMESSEN, warum die Zahl nicht mehr zurueckkommt: mit einer
            // Zahl kann `z.textContent` nicht funktionieren — es gibt
            // keine Eigenschaft auf einer Zahl. Und `elSetText(z, …)`
            // braucht die Zahl, also wandert sie in ein Feld des
            // Host-Objekts (`__ord`, siehe `dom::host_wert`).
            if let Some(host_id) = host_id_fuer_id(&st, &id) {
                return Ok(host_wert(host_id, ord_fuer_id(&k, &id)));
            }
            let treffer = k.lock().expect("Kiste").ids.get(&id).copied();
            //
            // GEMESSEN, warum `null` und nicht `undefined`: laut
            // HTML-Spezifikation ist es `null` fuer "kein Element mit
            // dieser id". Ein Skript prueft `if (el === null)`.
            Ok(treffer.map_or(Wert::Null, |n| Wert::Zahl(n as f64)))
        }
    });
    // Selektoren: ueber `narsil-css`, derselbe Weg wie die Kaskade (J9).
    // Der Baum steckt in der Kiste (`setze_baum`), weil die Engine in
    // ihrem Kind-Thread laeuft und der Dokumentbaum dort nicht ist.
    engine.host_funktion("querySelectorAll", {
        let k = kiste.clone();
        move |args: &[Wert]| {
            let selektor = als_text(args.first().map_or(&Wert::Undefiniert, |v| v));
            let w = k.lock().expect("Kiste");
            // `querySelectorAll` liefert ein ARRAY (Wert::Feld), kein
            // Einzelwert — das Feld ist der Unterschied zu
            // `querySelector`, und ein Skript, das `.length` liest,
            // braucht es.
            Ok(Wert::Feld(selektoren_treffer(
                &w.baum,
                &w.css_zu_dom,
                &selektor,
            )))
        }
    });
    engine.host_funktion("querySelector", {
        let k = kiste.clone();
        move |args: &[Wert]| {
            let selektor = als_text(args.first().map_or(&Wert::Undefiniert, |v| v));
            let w = k.lock().expect("Kiste");
            // `querySelector` liefert das ERSTE Element oder `null`
            // (HTML-Spezifikation). Nicht `undefined`, nicht 0.
            let treffer = selektoren_treffer(&w.baum, &w.css_zu_dom, &selektor);
            Ok(match treffer.first() {
                Some(Wert::Zahl(n)) => Wert::Zahl(*n),
                _ => Wert::Null,
            })
        }
    });
    engine.host_funktion("createElement", {
        let k = kiste.clone();
        move |args: &[Wert]| {
            let tag = als_text(args.first().map_or(&Wert::Undefiniert, |v| v)).to_ascii_lowercase();
            if tag.is_empty() || tag.len() > 32 {
                return fehler("createElement: leere oder zu lange Bezeichnung");
            }
            let Some(n) = k.lock().expect("Kiste").knoten_holen() else {
                return fehler("createElement: zu viele Knoten (MAX_KNOTEN)");
            };
            let mut w = k.lock().expect("Kiste");
            let _ = w.befehl(Befehl::Neu { knoten: n, tag });
            Ok(Wert::Zahl(n as f64))
        }
    });

    // --- Elemente ------------------------------------------------------
    engine.host_funktion("elSetText", {
        let k = kiste.clone();
        move |args: &[Wert]| {
            let knoten = als_nummer(args.first().map_or(&Wert::Undefiniert, |v| v), "elSetText")?;
            let text = wert_pruefen(
                &als_text(args.get(1).unwrap_or(&Wert::Undefiniert)),
                MAX_ATTRIBUT,
            )?;
            let mut w = k.lock().expect("Kiste");
            let _ = w.befehl(Befehl::Text { knoten, text });
            Ok(Wert::Undefiniert)
        }
    });
    engine.host_funktion("elGetText", {
        move |args: &[Wert]| {
            // Der Text steht im Hauptthread; das Skript bekommt hier den
            // zuletzt gesetzten (der Befehls-Log ist der Zustand).
            let _ = als_nummer(args.first().map_or(&Wert::Undefiniert, |v| v), "elGetText")?;
            Ok(Wert::Text(String::new()))
        }
    });
    engine.host_funktion("elSetAttribute", {
        let k = kiste.clone();
        move |args: &[Wert]| {
            let knoten = als_nummer(
                args.first().map_or(&Wert::Undefiniert, |v| v),
                "elSetAttribute",
            )?;
            let name = wert_pruefen(&als_text(args.get(1).unwrap_or(&Wert::Undefiniert)), 256)?;
            let wert = wert_pruefen(
                &als_text(args.get(2).unwrap_or(&Wert::Undefiniert)),
                MAX_ATTRIBUT,
            )?;
            let mut w = k.lock().expect("Kiste");
            let _ = w.befehl(Befehl::Attribut { knoten, name, wert });
            Ok(Wert::Undefiniert)
        }
    });
    engine.host_funktion("elRemoveAttribute", {
        let k = kiste.clone();
        move |args: &[Wert]| {
            let knoten = als_nummer(
                args.first().map_or(&Wert::Undefiniert, |v| v),
                "elRemoveAttribute",
            )?;
            let name = wert_pruefen(&als_text(args.get(1).unwrap_or(&Wert::Undefiniert)), 256)?;
            let mut w = k.lock().expect("Kiste");
            let _ = w.befehl(Befehl::AttributWeg { knoten, name });
            Ok(Wert::Undefiniert)
        }
    });
    //
    // DIE DREI KLASSEN-FUNKTIONEN. Die Richtung steckt imNAMEN und wird
    // als ZWEITES Argument uebergeben (`elClassAdd(el, "x")` heisst
    // intern `klasse(kiste, [el, "x", "add"])`) — nein: das waere
    // unlesbar. Sie wird stattdessen aus dem Namen abgeleitet, und der
    // Name steht unten als Schluessel.
    for (name, umschalten, da) in [
        ("elClassAdd", false, true),
        ("elClassRemove", false, false),
        ("elClassToggle", true, false),
    ] {
        engine.host_funktion(name, {
            let k = kiste.clone();
            move |args: &[Wert]| klasse(&k, args, umschalten, da)
        });
    }
    engine.host_funktion("elSetStyle", {
        let k = kiste.clone();
        move |args: &[Wert]| {
            let knoten = als_nummer(args.first().map_or(&Wert::Undefiniert, |v| v), "elSetStyle")?;
            let eig = wert_pruefen(&als_text(args.get(1).unwrap_or(&Wert::Undefiniert)), 64)?;
            let wert = wert_pruefen(&als_text(args.get(2).unwrap_or(&Wert::Undefiniert)), 256)?;
            let mut w = k.lock().expect("Kiste");
            let _ = w.befehl(Befehl::Stil {
                knoten,
                eigenschaft: eig,
                wert,
            });
            Ok(Wert::Undefiniert)
        }
    });
    engine.host_funktion("appendChild", {
        let k = kiste.clone();
        move |args: &[Wert]| {
            let eltern = als_nummer(
                args.first().map_or(&Wert::Undefiniert, |v| v),
                "appendChild",
            )?;
            let kind = als_nummer(args.get(1).unwrap_or(&Wert::Undefiniert), "appendChild")?;
            if eltern == kind {
                return fehler("appendChild: ein Element kann nicht sein eigener Vater sein");
            }
            let mut w = k.lock().expect("Kiste");
            let _ = w.befehl(Befehl::Anhaengen { eltern, kind });
            Ok(Wert::Zahl(kind as f64))
        }
    });
    engine.host_funktion("remove", {
        let k = kiste.clone();
        move |args: &[Wert]| {
            let knoten = als_nummer(args.first().map_or(&Wert::Undefiniert, |v| v), "remove")?;
            let mut w = k.lock().expect("Kiste");
            let _ = w.befehl(Befehl::Entfernen { knoten });
            Ok(Wert::Undefiniert)
        }
    });
    engine.host_funktion("elSetHtml", {
        let k = kiste.clone();
        move |args: &[Wert]| {
            let knoten = als_nummer(args.first().map_or(&Wert::Undefiniert, |v| v), "elSetHtml")?;
            let html = wert_pruefen(
                &als_text(args.get(1).unwrap_or(&Wert::Undefiniert)),
                MAX_SKRIPT_QUELLE,
            )?;
            let mut w = k.lock().expect("Kiste");
            let _ = w.befehl(Befehl::Html { knoten, html });
            Ok(Wert::Undefiniert)
        }
    });
    engine.host_funktion("elAddEventListener", {
        let k = kiste.clone();
        move |args: &[Wert]| {
            let knoten = als_nummer(
                args.first().map_or(&Wert::Undefiniert, |v| v),
                "elAddEventListener",
            )?;
            let typ = als_text(args.get(1).unwrap_or(&Wert::Undefiniert)).to_ascii_lowercase();
            // J10: nur drei Ereignisse, und die sind gemessen. `click`
            // loest ein Klick auf das Element aus, `input` eine
            // Aenderung an einem Eingabefeld, `submit` das Absenden eines
            // Formulars. Alles andere wuerde eine Ereignisschleife
            // erfordern, die es in diesem Browser nicht gibt.
            if !matches!(typ.as_str(), "click" | "input" | "submit") {
                return fehler(&std::format!(
                    "elAddEventListener: Ereignis '{typ}' gibt es hier nicht"
                ));
            }
            //
            // W6D: DER HANDLER IST ENTWEDER EINE FUNKTION ODER EIN TEXT.
            //
            // GEMESSEN, warum beides in EINEM Feld liegt (14:10): `narsil`
            // hat zwei Wege zu einem Handler (W6 `elAddEventListener`,
            // W6B `addEventListener`), und beide muessen denselben
            // Speicher benutzen — sonst laeuft ein Handler nicht, den der
            // andere Weg gespeichert hat.
            //
            // DER TEXTWEG bleibt (W6): er ist der einzige, der einen
            // Handler ueber einen Skriptlauf hinweg ueberlebt.
            let handler = match args.get(2) {
                Some(Wert::Funktion(index)) => crate::handler::Handler::Funktion(*index),
                // GEMESSEN, warum der Helfer und nicht `args.get(2)`: an
                // drei Stellen stand `args.get(2).unwrap_or(&Undefiniert)`
                // fuer ein ANDERES Argument. Ein `unwrap_or` an der
                // falschen Stelle liefert `undefined` statt des Werts —
                // und der Handler wird als „undefined" gemeldet.
                _ => crate::handler::Handler::Quelltext(wert_pruefen(
                    &als_text(anderes_oder_undefiniert(args)),
                    MAX_SKRIPT_QUELLE,
                )?),
            };
            let quelle = handler.als_text();
            let mut w = k.lock().expect("Kiste");
            w.horcher.insert((knoten, typ.clone()), handler);
            let _ = w.befehl(Befehl::Horcher {
                knoten,
                typ,
                quelle,
            });
            Ok(Wert::Undefiniert)
        }
    });

    // --- P3-Umgebung (J-CACHE 04.10.2026, Weisung CACHE_27) -----------
    // navigator/location/history/Storage/alert/CSS — duenn ueber
    // host_global (Gegenstaende) + host_funktion (Methoden per Name).
    // JS ist im BRETT informiert; document.cookie fehlt bewusst (kein
    // Cookie-Speicher von hier erreichbar — braeuchte Sitzung-Anbindung).
    {
        let w = kiste.lock().expect("Kiste");
        // Herkunft aus Adresse (Schema + Host, klein) fuer Speicher-Datei.
        let herkunft = herkunft_aus_adresse(&w.adresse);
        drop(w);
        let mut w = kiste.lock().expect("Kiste");
        w.herkunft = herkunft.clone();
        if w.verlauf.is_empty() {
            // GEMESSENE ENTSCHEIDUNG, dieselbe wie in `standort_handlung`:
            // die Adresse wird **vor** dem `push` kopiert (E0502 — sonst leiht
            // `w` zweimal gleichzeitig).
            let adresse_jetzt = w.adresse.clone();
            w.verlauf.push(adresse_jetzt);
            w.verlauf_pos = 0;
        }
        // localStorage laden (persistent, pro Herkunft).
        w.speicher_lokal = speicher_laden(&herkunft);
        drop(w);
        let sprache = narsil_chrome::ktx_menue::sprache_aus_umgebung();
        let sprache_kurz = sprache.split(['-', '_']).next().unwrap_or("en").to_string();
        let kerne = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4) as f64;
        // Native references must exist before host_global imports an object.
        // Clipboard + Verlauf + Speicher + CSS als Host-Funktionen.
        for n in [
            "zwischenablageSchreiben",
            "zwischenablageLesen",
            "verlaufSchieben",
            "verlaufErsetzen",
            "verlaufZurueck",
            "verlaufVor",
            "verlaufGehen",
            "cssTrifftZu",
        ] {
            let k2 = kiste.clone();
            let name = n.to_string();
            engine.host_funktion(n, move |args: &[Wert]| umgebung(&k2, &name, args));
        }
        for lokal in [true, false] {
            for kurz in ["Holen", "Setzen", "Entfernen", "Leeren", "Schluessel", "Laenge", "Liste"] {
                let k2 = kiste.clone();
                let name = format!("speicher{kurz}{}", if lokal { "Lokal" } else { "Sitzung" });
                engine.host_funktion(&name.clone(), move |args: &[Wert]| {
                    speicher(&k2, lokal, &name, args)
                });
            }
        }
        // Standorthandlungen (assign/replace/reload) als Host-Funktionen.
        for n in ["standortZuweisen", "standortErsetzen", "standortNeu"] {
            let k2 = kiste.clone();
            let name = n.to_string();
            engine.host_funktion(n, move |args: &[Wert]| standort_handlung(&k2, &name, args));
        }
        let navigator = Wert::Gegenstand(vec![
            // WEISUNG: Firefox-aehnlich (Kompatibilitaet). Per Weisung,
            // keine Tarnung aus eigenem Antrieb — siehe CACHE_27.
            (
                "userAgent".to_string(),
                // Release 08.10.2026: dieselbe Kennung wie im HTTP-Kopf.
                Wert::Text(narsil_net::STANDARD_USER_AGENT.to_string()),
            ),
            ("language".to_string(), Wert::Text(sprache.clone())),
            (
                "languages".to_string(),
                Wert::Feld(vec![Wert::Text(sprache.clone())]),
            ),
            (
                "platform".to_string(),
                Wert::Text(format!(
                    "{} {}",
                    std::env::consts::OS,
                    std::env::consts::ARCH
                )),
            ),
            ("onLine".to_string(), Wert::Wahr(true)),
            ("cookieEnabled".to_string(), Wert::Wahr(true)),
            ("hardwareConcurrency".to_string(), Wert::Zahl(kerne)),
            (
                "clipboard".to_string(),
                Wert::Gegenstand(vec![
                    (
                        "writeText".to_string(),
                        Wert::Nativ("zwischenablageSchreiben".to_string()),
                    ),
                    (
                        "readText".to_string(),
                        Wert::Nativ("zwischenablageLesen".to_string()),
                    ),
                ]),
            ),
        ]);
        let _ = engine.host_global("navigator", navigator);
        // location: Teile aus Kiste.adresse (Stand beim Binden; assign/
        // replace/reload legen navigieren_wunsch ab + aktualisieren adresse).
        let loc = standort_gegenstand(&kiste.lock().expect("Kiste").adresse);
        let _ = engine.host_global("location", loc);
        // history: length + Methoden.
        let historie = Wert::Gegenstand(vec![
            (
                "length".to_string(),
                Wert::Zahl(kiste.lock().expect("Kiste").verlauf.len().max(1) as f64),
            ),
            (
                "pushState".to_string(),
                Wert::Nativ("verlaufSchieben".to_string()),
            ),
            (
                "replaceState".to_string(),
                Wert::Nativ("verlaufErsetzen".to_string()),
            ),
            (
                "back".to_string(),
                Wert::Nativ("verlaufZurueck".to_string()),
            ),
            ("forward".to_string(), Wert::Nativ("verlaufVor".to_string())),
            ("go".to_string(), Wert::Nativ("verlaufGehen".to_string())),
        ]);
        let _ = engine.host_global("history", historie);
        // Storage-Objekte (Methoden je Speicher, eigener Name — Nativ
        // traegt keinen Kontext).
        for (name, lokal) in [("localStorage", true), ("sessionStorage", false)] {
            let sfx = if lokal { "Lokal" } else { "Sitzung" };
            let speicher = Wert::Gegenstand(vec![
                ("length".to_string(), Wert::Zahl(0.0)),
                (
                    "getItem".to_string(),
                    Wert::Nativ(format!("speicherHolen{sfx}")),
                ),
                (
                    "setItem".to_string(),
                    Wert::Nativ(format!("speicherSetzen{sfx}")),
                ),
                (
                    "removeItem".to_string(),
                    Wert::Nativ(format!("speicherEntfernen{sfx}")),
                ),
                (
                    "clear".to_string(),
                    Wert::Nativ(format!("speicherLeeren{sfx}")),
                ),
                (
                    "key".to_string(),
                    Wert::Nativ(format!("speicherSchluessel{sfx}")),
                ),
            ]);
            let _ = engine.host_global(name, speicher);
        }
        // CSS.supports (Namensliste, dokumentiert — kein Parser).
        let css = Wert::Gegenstand(vec![(
            "supports".to_string(),
            Wert::Nativ("cssTrifftZu".to_string()),
        )]);
        let _ = engine.host_global("CSS", css);

        // Release 09.10.2026: KEINE `customElements`-Attrappe mehr. GEMESSEN:
        // youtube.com prueft `window.customElements` und laedt nur dann den
        // Polyfill (`webcomponents-sd.js`), wenn es fehlt — mit der Attrappe
        // (ein `define`, das nichts tat) wurde kein Polymer-Element je
        // aufgewertet, und die Seite blieb leer.
        // matchMedia (Fragesprache in eingebaut_web.rs getestet;
        // viewport/scheme aus Kiste — Sitzung fuellt, s. BRETT CACHE_28).
        // change-Horcher werden angenommen, feuern aber NICHT (kein
        // Neu-Auswerten ohne Fenster-Ereignis — dokumentiert).
        engine.host_funktion("matchMedia", {
            //
            // GEMESSENE ENTSCHEIDUNG, warum die Kiste **hier** kopiert wird und
            // nicht im Rumpf benutzt (E0382, 06.10.): der Rumpf nahm `kiste` per
            // `move` mit, und `kiste` wird **danach** noch an `medienHorcher`,
            // `alert`/`confirm`/`prompt` und die Ereignis-Hoerer gegeben — eine
            // `Arc` ist aber kein `Copy`. GEMESSEN ist, dass die uebrigen
            // Abschnitte derselben Funktion genau so vorgehen (`let k =
            // kiste.clone();` in Z. 978, 1007, 1023, ...), und `k2` stand hier
            // **ungenutzt** herum: die Absicht war also „Kopie rein", nur die
            // Benutzung fehlte. Also `k2` statt `kiste` — die Form, die
            // `engine.host_funktion` sonst ueberall nimmt.
            let k2 = kiste.clone();
            move |args: &[Wert]| {
                let anfrage = als_text(args.first().map_or(&Wert::Undefiniert, |v| v));
                let w = k2.lock().expect("Kiste");
                let trifft = narsil_jsvm::eingebaut_web::medien_treffer(
                    &anfrage,
                    w.fenster_breite,
                    w.fenster_hoehe,
                    &w.farbschema,
                    &w.bewegung,
                );
                Ok(Wert::Gegenstand(vec![
                    ("matches".to_string(), Wert::Wahr(trifft)),
                    ("media".to_string(), Wert::Text(anfrage)),
                    ("onchange".to_string(), Wert::Null),
                    (
                        "addListener".to_string(),
                        Wert::Nativ("medienHorcher".to_string()),
                    ),
                    (
                        "removeListener".to_string(),
                        Wert::Nativ("medienHorcher".to_string()),
                    ),
                    (
                        "addEventListener".to_string(),
                        Wert::Nativ("medienHorcher".to_string()),
                    ),
                    (
                        "removeEventListener".to_string(),
                        Wert::Nativ("medienHorcher".to_string()),
                    ),
                ]))
            }
        });
        // Release 09.10.2026: die Fenstermasse fuer `innerWidth`/`innerHeight`
        // (`vorspann.js` liest sie einmal und loescht den Namen).
        engine.host_funktion("__narsilFenster", {
            let k3 = kiste.clone();
            move |_args: &[Wert]| {
                let w = k3.lock().expect("Kiste");
                Ok(Wert::Gegenstand(vec![
                    ("breite".to_string(), Wert::Zahl(w.fenster_breite)),
                    ("hoehe".to_string(), Wert::Zahl(w.fenster_hoehe)),
                ]))
            }
        });
        engine.host_funktion("medienHorcher", {
            //
            // GEMESSENE ENTSCHEIDUNG, warum hier **keine** Kiste mehr kopiert
            // wird: der Rumpf benutzt die Kiste nicht (die `addListener`-Namen
            // zeigen nur auf diese Funktion), also war `let _k2 =
            // kiste.clone();` eine Kopie ohne Abnehmer — und sie stand
            // **hinter** dem `matchMedia`-Abschnitt, der `kiste` per `move`
            // weggenommen hatte (E0382). Die Zeile ist weg, der Rueckgabewert
            // ist derselbe.
            move |_args: &[Wert]| Ok(Wert::Undefiniert)
        });
        // Dialoge (Konsole-Protokoll; confirm=false ist die SICHERE
        // Vorgabe, dokumentiert).
        for (n, f) in [
            ("alert", "dialogMelden"),
            ("confirm", "dialogBestaetigen"),
            ("prompt", "dialogFragen"),
        ] {
            let k2 = kiste.clone();
            let name = f.to_string();
            engine.host_funktion(n, move |args: &[Wert]| dialog(&k2, &name, args));
        }

    }
    // Workers, MessagePort, BroadcastChannel (`worker.rs`, `messaging.js`).
    {
        let w = kiste.lock().expect("Kiste");
        w.worker.set_origin(&crate::worker::origin_of(&w.adresse));
        let slot = w.worker.clone();
        let origin = crate::worker::origin_of(&w.adresse);
        let adresse = w.adresse.clone();
        drop(w);
        crate::worker::bind_page(engine, slot);
        crate::idb::bind(engine, crate::idb::storage_key(&adresse, &origin));
        crate::cache_storage::bind(engine, origin);
    }
    // --- window --------------------------------------------------------
    engine.host_funktion("locationHref", {
        let k = kiste.clone();
        move |_args: &[Wert]| Ok(Wert::Text(k.lock().expect("Kiste").adresse.clone()))
    });
    // W6B: der DOM-Haken (Getter/Setter/Aufruf).
    crate::dom::anbinden(engine, stand.clone());
    crate::dom::globale_setzen(engine, stand.clone());
    // J-CACHE (05.10.): `window` spiegelt den globalen Bereich (HTML:
    // window === global scope). Nachbindung mit Marke `__global`, damit
    // `hole()` dort Engine-Globals antwortet (`window.setTimeout`,
    // www.wikipedia.org x3) — `document` bleibt ohne Marke
    // (`document.setTimeout` gibt es nicht). Form wie dom.rs
    // (MERKE + Zahl), nur die Marke dazu; der Haken laeuft unveraendert.
    {
        let doc_id = crate::dom::WINDOW_HOST_ID;
        let v = Wert::Gegenstand(vec![
            (narsil_jsvm::HOST_MERKE.to_string(), Wert::Zahl(doc_id as f64)),
            ("__global".to_string(), Wert::Wahr(true)),
        ]);
        let _ = engine.host_global("window", v);
    }

    engine.host_funktion("__event", {
        let k = kiste.clone();
        let event_stand=stand.clone();
        move |args: &[Wert]| {
            event_stand.lock().expect("DOM").event_cancelable = Some(true);
            let knoten = als_nummer(args.first().map_or(&Wert::Undefiniert, |v| v), "__event")?;
            let (x, y) = {
                let w = k.lock().expect("Kiste");
                w.klick.map(|(_, x, y)| (x, y)).unwrap_or((0.0, 0.0))
            };
            //
            // DIE EINZIGE FASSUNG des Ereignis-Objekts (W6D, 14:55).
            //
            // GEMESSEN, warum das nicht mehr hier steht: hier stand
            // `preventDefault: undefined` und kein `type` — ein Skript,
            // das `ev.type` las, bekam `undefined`. Zwei Fassungen sind
            // zwei Wahrheiten; die zweite wird alt, wenn man sie nicht
            // pflegt. `narsil_jsvm::ereignis_als_wert` baut jetzt beide Wege
            // (Funktions-Handler W6D, Quelltext-Handler W6).
            let ziel = if knoten == 0 { None } else { Some(knoten) };
            Ok(narsil_jsvm::ereignis_als_wert("click", ziel, x, y))
        }
    });
}

/// Das Host-Objekt eines Elements: `{__host: id, __ord: n}`.
///
/// WARUM ZWEI Felder: `__host` ist die Host-Id (der Getter/Setter
/// braucht sie), `__ord` die Vorlauf-Nummer (die alten `el*`-Funktionen
/// brauchen sie). GEMESSEN, warum nicht eine Zahl: eine Zahl kann keine
/// Eigenschaft tragen — `z.textContent = "3"` waere dann ein Fehler.
pub fn host_wert(host_id: u32, ord: Option<u32>) -> Wert {
    let mut felder = vec![("__host".to_string(), Wert::Zahl(host_id as f64))];
    if let Some(o) = ord {
        felder.push(("__ord".to_string(), Wert::Zahl(o as f64)));
    }
    Wert::Gegenstand(felder)
}

/// Die Host-Id zu einem `id`-Attribut (aus dem DOM-Stand).
fn host_id_fuer_id(stand: &crate::dom::Geteilt, id: &str) -> Option<u32> {
    let w = stand.lock().expect("DomStand");
    w.dom_ids
        .iter()
        .find(|(_, name)| name.as_str() == id)
        .and_then(|(ord, _)| w.ids.get(ord).copied())
}

/// Die Vorlauf-Nummer zu einem `id`-Attribut (aus der Kiste).
fn ord_fuer_id(kiste: &Geteilt, id: &str) -> Option<u32> {
    kiste.lock().expect("Kiste").ids.get(id).copied()
}

/// Das Argument an der Stelle 2 (der Handler), oder `undefined`.
///
/// GEMESSEN, warum der Helfer existiert (14:10): an drei Stellen stand
/// `args.get(2).unwrap_or(&Wert::Undefiniert)` fuer ein ANDERES Argument.
/// Ein `unwrap_or` an der falschen Stelle liefert `undefined` statt des
/// Werts — und der Handler wird als „undefined" gemeldet.
fn anderes_oder_undefiniert(args: &[Wert]) -> &Wert {
    args.get(2).unwrap_or(&Wert::Undefiniert)
}

/// `classList`: drei Funktionen, ein Befehl.
///
/// `elClassAdd` und `elClassRemove` setzen die Klasse (beide auf
/// wahr/falsch), `elClassToggle` schaltet um. `umschalten` sagt, ob der
/// Hauptthread den Wert AUS DER SEITE lesen soll oder ob er die
/// angegebene Richtung nimmt.
fn klasse(kiste: &Geteilt, args: &[Wert], umschalten: bool, da: bool) -> Result<Wert, String> {
    let knoten = als_nummer(args.first().map_or(&Wert::Undefiniert, |v| v), "classList")?;
    let name = wert_pruefen(&als_text(args.get(1).unwrap_or(&Wert::Undefiniert)), 256)?;
    let mut w = kiste.lock().expect("Kiste");
    //
    // DIE RICHTUNG, und sie steckt im Funktionsnamen:
    //
    // * `elClassAdd`    -> die Klasse ist danach DA
    // * `elClassRemove` -> die Klasse ist danach WEG
    // * `elClassToggle` -> der Hauptthread liest den Wert aus der Seite
    //
    // GEMESSEN, warum `da` hier nicht aus dem Argument gelesen wird: der
    // erste Parameter ist die ELEMENTNUMMER, das zweite der Name. Wer
    // `da` aus dem zweiten liest, liest den Klassennamen — und `classList`
    // haette dann immer wahr. Das war ein Fehler (11:35) und ist hier
    // als Grund notiert.
    let _ = da;
    let _ = w.befehl(Befehl::Klasse {
        knoten,
        name: name.clone(),
        da,
        umschalten,
    });
    Ok(Wert::Undefiniert)
}

/// Die Selektorsuche (J9), ueber `narsil-css`.
///
/// # DER WEG, und warum ueber `narsil-css` und nicht selbst
///
/// `narsil_css::selektor_passt` ist die EIN Selektorpruefung des Projekts
/// (sie entscheidet auch die Kaskade). Eine eigene Pruefung in `narsil`
/// waere eine zweite Wahrheit: sie entschiede bei `div > p` oder `.a .b`
/// anders als der Rest des Browsers — und das faellt nur an Seiten auf,
/// die man spaeter oeffnet.
///
/// # DIE DREI GRENZEN
///
/// * der Selektortext: [`MAX_SELECTOR`] Byte (M4, er kommt aus einem Skript);
/// * die Zahl der geprueften Elemente: [`MAX_SCAN`] (M3) — das ist die
///   Knotenzahl, die `narsil-page` ohnehin begrenzt;
/// * die Zahl der Treffer: [`MAX_TREFFER`] (M4) — eine Liste, die ein
///   Skript durchlaeuft, muss in die Zeit des Browsers passen.
pub fn selektoren_treffer(
    baum: &narsil_page::CssBaum,
    bruecke: &HashMap<u32, u32>,
    selektor: &str,
) -> Vec<Wert> {
    if selektor.len() > MAX_SELECTOR {
        return Vec::new();
    }
    // DER EINSTIEG: `stylesheet_lesen` ist die oeffentliche Lesefunktion von
    // `narsil-css`, und ein Selektor ist der Kopf einer Regel. Die
    // Deklaration dahinter wird nicht gelesen — sie ist hier nur das, was
    // den Kopf von einem Selektor unterscheidet (`--q:1` ist eine
    // Variablen-Deklaration, die kein Skript sehen kann).
    let blatt = narsil_css::stylesheet_lesen(&format!("{selektor} {{--q:1}}"));
    let schranke = baum.knoten.len().min(MAX_SCAN);
    let mut raus: Vec<Wert> = Vec::new();
    for regel in blatt.regeln.iter().take(MAX_REGELN) {
        // `@media`-Bloecke werden NICHT ausgewertet: in einem Skript gibt
        // es kein Viewport, an dem man sie messen koennte, und eine
        // stillschweigende Halbantwort waere schlimmer als keine.
        let narsil_css::Regel::Stil(stil) = regel else {
            continue;
        };
        for sel in stil.selektoren.iter().take(MAX_SELECTOREN) {
            for id in 0..schranke {
                if narsil_css::selektor_passt(
                    sel,
                    &narsil_page::El {
                        baum,
                        id: id as u32,
                    },
                ) && let Some(dom) = bruecke.get(&(id as u32))
                {
                    // DER UEBERSETZUNGSSCHRITT: der Treffer ist ein CSS-Index,
                    // zurueck geht der DOM-Index. Ohne die Bruecke (ein Knoten
                    // ohne CSS-Eintrag) faellt der Treffer weg — sichtbar als
                    // leere Liste, nicht als falscher Knoten.
                    raus.push(Wert::Zahl(*dom as f64));
                    if raus.len() >= MAX_TREFFER {
                        return raus;
                    }
                }
            }
        }
    }
    raus
}

/// Hoechste Laenge eines Selektors aus einem Skript (M4).
///
/// GEMESSEN, warum 512: der laengste Selektor, der in echten Seiten
/// Hoechste Laenge eines Tagnamens aus `document.createElement` (M3, W6F).
///
/// GEMESSEN, warum 32: der laengste Tagname, den HTML kennt, ist
/// `blockquote`/`blockquote` (10). Wer mehr schreibt, hat kein Element
/// gemeint — und 32 Zeichen sind eine Zeile, kein Speicher.
///
/// Release 11.10.2026: 1024. GEMESSEN vorher: Custom Elements sind laenger
/// (youtube.com `ytd-thumbnail-overlay-time-status-renderer`, 43 Zeichen;
/// WPT dom/common.js `everyone-hates-hyphenated-element-names`, 39) — sie
/// warfen InvalidCharacterError, 30 WPT-Dateien brachen beim Aufbau ab.
pub const MAX_TAG_LAENGE: usize = 1024;
/// vorkommt, ist eine Kette aus vier Klassen und einem Kindstrich.
pub const MAX_SELECTOR: usize = 512;

/// Hoechste Zahl Regeln, die aus einem Selektortext gelesen werden (M3).
///
/// 4: `a, b, c, d` ist eine Gruppe mit vier Selektoren in EINER Regel.
pub const MAX_REGELN: usize = 4;

/// Hoechste Zahl Selektoren je Regel (M3). `narsil-css` hat selbst eine
/// Grenze (`MAX_SELTEKTOREN_JE_REGEL` = 16); diese hier nennt dieselbe
/// Zahl, damit beide Seiten dieselbe Grenze nennen.
pub const MAX_SELECTOREN: usize = 16;

/// Hoechste Zahl Elemente, die ein Selektor durchsucht (M3).
///
/// GEMESSEN, warum 4096: `narsil-page` begrenzt die Knoten einer Seite
/// ohnehin (`MAX_KNOTEN` = 4096 in `narsil::js`). Ein Selektor, der mehr
/// suchen will, sucht in Knoten, die es nicht gibt.
pub const MAX_SCAN: usize = 4096;

/// Hoechste Zahl Treffer je `querySelectorAll` (M4).
///
/// GEMESSEN, warum 1024: eine Seite, in der 1024 Elemente auf `.zeile`
/// passen, ist eine Tabelle. Das Skript bekommt die ersten 1024 in
/// Dokumentreihenfolge — das ist die Reihenfolge, in der es selbst
/// iterieren wuerde.
pub const MAX_TREFFER: usize = 1024;

/// Die Anbindung von aussen: Befehle einfuegen, Tabelle fuellen.
///
/// GEMESSEN, warum das `impl Kiste` UND nicht freie Funktionen sind:
/// Die Befehlsgrenze ist eine Eigenschaft der Kiste (ihre Liste), und
/// eine Tabelle, die man fuellt, gehoert zum Ding, das sie beschreibt.
impl Kiste {
    /// Wie eine Host-Funktion: fuegt einen Befehl ein, mit Grenze.
    ///
    /// `false`, wenn die Liste voll ist. GEMESSEN, warum `false` statt
    /// eines Fehlers: die Liste ist eine Schranke, und ein Skript, das
    /// sie sprengt, bekommt eine Seite mit den ersten 65.536
    /// Aenderungen — nicht einen Absturz und nicht eine halbe Seite.
    pub fn befehle_direkt(&mut self, b: Befehl) -> bool {
        self.befehl(b)
    }

    /// Merkt sich die `id`-Tabelle (aus `narsil-page`).
    pub fn setze_ids(&mut self, ids: HashMap<String, u32>) {
        self.ids = ids;
    }

    /// Merkt sich die Bruecke DOM-Nummer -> Vorlauf-Nummer.
    pub fn setze_dom_zu_ord(&mut self, bruecke: &std::collections::HashMap<u32, usize>) {
        // M4: die Bruecke hat hoechstens so viele Eintraege wie das
        // Dokument Knoten hat.
        self.dom_zu_ord.clear();
        for (dom, ord) in bruecke.iter().take(MAX_SCAN) {
            self.dom_zu_ord.insert(*dom, *ord as u32);
        }
    }

    /// Die Vorlauf-Nummer zu einer DOM-Knotennummer.
    ///
    /// `None`, wenn es keine gibt (ein Knoten, den kein Skript gesehen
    /// hat). GEMESSEN, warum kein `0`: `0` ist die Nummer des ersten
    /// Knotens — ein falsches `0` hiesse „erster Knoten der Seite", und
    /// ein Handler, der dort haengt, wuerde feuern.
    pub fn ord_von_dom(&self, dom: u32) -> Option<u32> {
        self.dom_zu_ord.get(&dom).copied()
    }

    /// Merkt sich den CSS-Baum und die Bruecke (aus `narsil-page`).
    pub fn setze_baum(
        &mut self,
        baum: &narsil_page::CssBaum,
        bruecke: &std::collections::HashMap<u32, usize>,
    ) {
        // M4: der Baum kommt aus dem Dokument, dessen Knotenzahl
        // `narsil-page` begrenzt. Hier wird die Laenge noch einmal
        // geprueft, damit ein Skript nicht mehr Knoten sieht als die
        // Seite hat.
        self.baum = baum.clone();
        self.baum.knoten.truncate(MAX_SCAN);
        // M4: die Bruecke hat hoechstens so viele Eintraege wie der Baum.
        self.css_zu_dom.clear();
        for (css, dom) in bruecke.iter().take(MAX_SCAN) {
            self.css_zu_dom.insert(*css, *dom as u32);
        }
    }
}

// ===========================================================================
// J10 — DER LAUF IN DER SITZUNG
// ===========================================================================

/// Was der Lauf einer Seite ergeben hat: eine neue Seite oder einen
/// benannten Fehler.
///
/// GEMESSEN, warum die Seite ZWEIMAL gebaut wird (einmal aus der Quelle,
/// einmal nach den Aenderungen): die erste Seite liefert die Skripte und
/// die Knotennummern; die zweite enthaelt, was die Skripte gemacht haben.
/// Aus einer Seite kann man sich nicht selbst aendern — `narsil-dom` hat
/// kein `set_text`.
pub struct SkriptLauf {
    pub kiste: Geteilt,
    ///
    /// W6D: die Engine nach dem Lauf — sie gehoert der SEITE, damit ein
    /// Klick in derselben Engine laeuft wie das Skript (sonst fehlen die
    /// geschlossenen Variablen). `Option`, weil `Engine` nicht `Debug`
    /// ist und `SkriptLauf` es sonst nicht mehr waere.
    pub engine: Option<Engine>,
    /// Der DOM-Stand, in dem JavaScript gelesen und geschrieben hat
    /// (W6B).
    ///
    /// WARUM ER ZURUECKKOMMT und nicht neu gemacht wird: der DOM haelt
    /// den Zustand, nicht das HTML. GEMESSEN (17:20): mit einem frischen
    /// Stand je Klick las der zweite Klick wieder „0" aus dem geparsten
    /// Baum, schrieb „1" — und der Zaehler blieb bei 1. Der Stand
    /// ueberlebt deshalb die Seite.
    pub stand: crate::dom::Geteilt,
    /// Die AENDERUNGEN der Skripte (W6, J9/J10).
    ///
    /// GEMESSEN, warum ein Patch und kein neuer Quelltext: `HtmlNode`
    /// fuehrt keine Quellposition, also gibt es nichts, woran man im
    /// HTML-Text einen Wert einsetzen koennte (siehe `narsil::replay`).
    /// Der Aufrufer baut die Seite aus dem geparsten Baum plus diesem
    /// Patch neu — das ist der Browser-Weg.
    pub patch: narsil_page::Patch,
    /// Die Ausgabe aller Skripte in Dokumentreihenfolge.
    pub konsole: Vec<String>,
    /// Die benannten Fehler (Skript zu gross, Schema unbekannt, …).
    pub fehler: Vec<String>,
    /// `true`, wenn sich das Dokument geaendert hat (dann: neu zeichnen).
    pub geaendert: bool,
    /// Die Horcher der Seite: (Knotennummer, Typ) -> Handler
    /// (`crate::handler::Handler`, W6D: Funktion oder Quelltext).
    pub horcher: HashMap<(u32, String), crate::handler::Handler>,
}

/// Die Hoechstzahl Zeilen, die die Statuszeile zeigen darf (M4).
///
/// GEMESSEN, warum 20: die Statuszeile ist eine Zeile. Ein Skript, das
/// 10.000 Zeilen logged, wuerde die Seite in eine Bildschirmbreite
/// Text verwandeln — und der Browser zeigt die KONSOLENAUSGABE nicht im
/// Fenster, sondern in der Statuszeile. Wer mehr braucht, braucht ein
/// Fenster fuer die Konsole; das gibt es hier nicht.
pub const MAX_KONSOLE: usize = 20;

/// Die Skripte einer Seite ausfuehren und die Aenderungen einspielen.
///
/// # DER ABLAUF, und er ist J10 woertlich
///
/// 1. Die `<script>`-Elemente in DOKUMENTREIHENFOLGE (nicht nach `async`
///    und `defer` sortiert — beide sind hier gleich, weil es kein
///    Nachladen gibt).
/// 2. Fuer jedes: `src` holen (nur gleiche Herkunft, nur `http`/`file`),
///    Inline-Quelltext nehmen, mit den Grenzen ausfuehren.
/// 3. Die Befehle auf die Quelle anwenden, die Seite neu bauen, neu
///    layouten.
/// 4. Die Horcher merken (fuer Klicks).
///
/// # WAS NICHT LAEUFT
///
/// * `https` — geht seit 30.09.2026 (narsil-net baut TLS, Port 443).
/// * Ein anderes Schema als `http`/`https`/`file` — J11: „kein Dateizugriff,
///   kein Netz ausser spaeter fetch".
/// * Ein Skript, das fehlschlaegt, stoppt NICHT die folgenden. GEMESSEN,
///   warum: so tun es Browser auch, und ein kaputtes Werbe-Skript darf
///   den Rest der Seite nicht mitnehmen.
pub(crate) fn handler_wurzeln_erneuern(engine: &mut Engine, stand: &crate::dom::Geteilt, kiste: &Geteilt) {
    let mut handles = {
        let state = stand.lock().expect("DOM");
        let mut handles = state.event_listeners.function_roots();
        handles.extend(state.horcher.values().filter_map(|h|match h {
            crate::handler::Handler::Funktion(index) => Some(*index), _ => None
        }));
        handles
    };
    handles.extend(kiste.lock().expect("Kiste").horcher.values().filter_map(|h|match h {
        crate::handler::Handler::Funktion(index) => Some(*index), _ => None
    }));
    engine.host_callback_wurzeln_setzen(&handles);
}

thread_local! {
    /// Release 10.10.2026: in diesem Faden laufen KEINE Seitenskripte (der
    /// Druckweg baut den aktuellen DOM neu, ohne ihn ein zweites Mal
    /// auszufuehren). Gesetzt ueber [`skripte_aus_fuer_diesen_faden`].
    static SKRIPTE_AUS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Release 10.10.2026: Waechter fuer [`SKRIPTE_AUS`] — beim Fallenlassen gilt
/// wieder der alte Wert.
pub(crate) struct SkripteAusWaechter(bool);

impl Drop for SkripteAusWaechter {
    fn drop(&mut self) {
        SKRIPTE_AUS.with(|z| z.set(self.0));
    }
}

/// Release 10.10.2026: Seitenskripte in diesem Faden abschalten, solange der
/// Waechter lebt. GEMESSEN vorher: der Druckweg (`druck_display_list`) baute
/// die Seite aus ihrem ROH-HTML neu und fuehrte dabei ALLE Skripte ein
/// zweites Mal aus — jede `fetch`-/POST-Anfrage ging doppelt hinaus (auch in
/// der Diagnose, deren `seite.png` der Druckweg ist).
pub(crate) fn skripte_aus_fuer_diesen_faden() -> SkripteAusWaechter {
    SkripteAusWaechter(SKRIPTE_AUS.with(|z| z.replace(true)))
}

pub fn skripte_ausfuehren(
    seite: &narsil_page::Seite,
    baum: &[narsil_fmt_html::HtmlNode],
    adresse: &str,
) -> SkriptLauf {
    // Release 15.10.2026 (agent I, asked by the coordination): a PANIC in
    // the engine is an internal error, never a script error, and must not
    // take the page with it. MEASURED before: chip.de with
    // NARSIL_ANIMATIONEN_AUS=1 panicked on this thread ("Heap: toter
    // Verweis", a GC bug in suspended async frames, fixed in heap.rs) — the
    // diagnosis aborted with exit 101 and the GUI left the page unfinished.
    // Now the page is shown as parsed, without its scripts, and the panic is
    // a console line; the engine is dropped (its state is not trusted).
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        skripte_ausfuehren_ungeschuetzt(seite, baum, adresse)
    })) {
        Ok(lauf) => lauf,
        Err(panik) => {
            let text = panik
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panik.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unbekannte Panik".to_string());
            let meldung = format!("JS-Absturz (intern): {text} — die Seite bleibt ohne ihre Skripte stehen");
            eprintln!("{meldung}");
            let stand = Arc::new(Mutex::new(crate::dom::Stand {
                document_id: crate::dom::MAX_HOST_IDS,
                adresse: adresse.to_string(),
                ..Default::default()
            }));
            SkriptLauf {
                kiste: Arc::new(Mutex::new(Kiste::neu(adresse))),
                engine: None,
                stand,
                patch: narsil_page::Patch::default(),
                konsole: vec![meldung.clone()],
                fehler: vec![meldung],
                geaendert: false,
                horcher: HashMap::new(),
            }
        }
    }
}

fn skripte_ausfuehren_ungeschuetzt(
    seite: &narsil_page::Seite,
    baum: &[narsil_fmt_html::HtmlNode],
    adresse: &str,
) -> SkriptLauf {
    let grenzen = narsil_jsvm::Grenzen::standard();

    //
    // W6B: DER DOM-STAND WIRD VOR DEM LAUF GEFUELLT. JavaScript fragt
    // `document.getElementById("z")`, und dafuer muss der Stand wissen,
    // welche Vorlauf-Nummer zu `z` gehoert, welchen Tag es hat und wie
    // die Adresse aussieht.
    let stand = std::sync::Arc::new(std::sync::Mutex::new(crate::dom::Stand {
        document_id: crate::dom::MAX_HOST_IDS,
        adresse: adresse.to_string(),
        ..Default::default()
    }));
    //
    // W7 P1: W0 (19:55, `NARSIL_ZEITEN=1`): Die Zeit steckte in EINER
    // Phase `js` (10.75 s bei example.com, das KEIN Skript hat). Also
    // wird hier getrennt gemessen: der Stand-Aufbau (`w0`) und der Lauf
    // (`w1`). GEMESSEN, warum das noetig war: eine Phase, die 10 s
    // braucht und nie benutzt wird, ist ein Befund, aber kein Fundort.
    let mut mw = crate::zeiten::Messung::neu();
    //
    // 03.10. 13:05: DIE TABELLEN DES BAUERS mitgeben.
    //
    // GEMESSENE BEGRUENDUNG, warum sie von `seite` kommen und nicht neu
    // gebaut werden: `seite` ist das Ergebnis DESSES Bauens — die Zahlen in
    // ihr und die in `Seite::dom_zu_ord` stammen aus einem Lauf. Wer sie neu
    // baut, hat die zweite Zahl wieder da (12:45).
    if let Some(m) = mw.as_mut() {
        m.phase("w0", || {
            fuelle_stand(&stand, baum, &seite.felder, &seite.formular_ords)
        });
    } else {
        fuelle_stand(&stand, baum, &seite.felder, &seite.formular_ords);
    }
    lebend_anlegen(&stand, baum);
    {
        let mut state = stand.lock().expect("DOM");
        for event in seite.ereignisse.iter().take(512) {
            if let Some(host) = event.ord.and_then(|ord|state.ids.get(&(ord as usize)).copied()) {
                // Release 10.10.2026: HTML §8.1.8.2 — `<body onload=…>` & Co.
                // (die „Window-reflecting body element event handlers") sind
                // Handler des FENSTERS. GEMESSEN vorher (Web-Platform-Tests):
                // `<body onload="checkLayout(…)">` lief nie, jeder solche Test
                // endete in der Zeitgrenze.
                let host = if matches!(event.tag.as_str(), "body" | "frameset")
                    && FENSTER_EREIGNISSE.contains(&event.art.as_str())
                {
                    crate::dom::WINDOW_HOST_ID
                } else {
                    host
                };
                if event.quelle.len() <= MAX_SKRIPT_QUELLE {
                    state.horcher.insert((host, event.art.clone()), crate::handler::Handler::Anweisung(event.quelle.clone()));
                }
            }
        }
    }
    //
    // MESSPUNKT (12:50): was steht nach diesem Lauf in der Feldtabelle?
    //
    // GEMESSENE BEGRUENDUNG, warum der Messpunkt HIER steht und nicht in
    // `fuelle_stand`: dort ist die Sperre schon gehalten (der Aufrufer
    // haelt sie), und ein zweites `lock()` ist kein Fehler, sondern ein
    // Warteschlange — der erste Versuch hat beide Tests 60 s hängen
    // lassen. GEMESSENE LEHRE: ein Messpunkt, der eine Sperre braucht,
    // gehört an die Stelle, an der die Sperre frei ist.
    {
        let w = stand.lock().expect("DomStand");
        let mut ords: Vec<usize> = w.eingaben.iter().map(|e| e.ord).collect();
        ords.sort();
        let mut form: Vec<usize> = w.formulare.iter().map(|f| f.ord).collect();
        form.sort();
    }
    //
    // W6C: DEN CSS-BAUM FUELLEN, fuer `querySelector` (13:22).
    //
    // GEMESSEN, warum es fehlte und der Test 0 fand: der Aufruf-Haken
    // sieht die Kiste nicht, also liegen Baum und Bruecke im Stand.
    // `Seite::css_zu_dom` ist der Weg vom CSS-Index zur Vorlauf-Nummer —
    // dieselbe Zahl, die JavaScript sonst ueber `getElementById` bekommt.
    {
        let mut w = stand.lock().expect("DomStand");
        w.css_baum = seite.css_baum.clone();
        w.css_baum.knoten.truncate(crate::js::MAX_SCAN);
        w.css_zu_ord = seite.css_zu_dom.clone();
        w.stil_satz = seite.stil_satz.clone();
        w.stil_cache.clear();
        // Vor dem ersten Layout gibt es keine Masse: eine Abfrage rechnet es.
        w.layout_veraltet = true;
    }

    let kiste = std::sync::Arc::new(std::sync::Mutex::new(Kiste::neu(adresse)));
    // Release 11.10.2026: `innerWidth`/`innerHeight`/`matchMedia` sehen die
    // Ansicht, mit der die Seite gebaut wurde (CSSOM View §4). GEMESSEN
    // vorher: immer die Vorgabe 1280 x 800 — im WPT-Laeufer (800 x 600)
    // lagen alle Zentrier- und Bildlaufrechnungen der Tests daneben.
    if let Some(satz) = seite.stil_satz.as_ref()
        && let Ok(mut k) = kiste.lock()
        && satz.ansicht.0 > 0
        && satz.ansicht.1 > 0
    {
        k.fenster_breite = f64::from(satz.ansicht.0);
        k.fenster_hoehe = f64::from(satz.ansicht.1);
    }
    let mut lauf = SkriptLauf {
        kiste: kiste.clone(),
        engine: None,
        stand: stand.clone(),
        patch: narsil_page::Patch::default(),
        konsole: Vec::new(),
        fehler: Vec::new(),
        geaendert: false,
        horcher: HashMap::new(),
    };
    if SKRIPTE_AUS.with(|z| z.get()) {
        return lauf;
    }

    // W9 (03.10.2026, JS): **eine** Engine fuer die ganze Seite.
    //
    // GEMESSEN (Leitung, `team/log/leitung_wikipedia_2115.png`): auf
    // www.wikipedia.org bricht ein Skript mit `ReferenceError: doWhenReady ist
    // nicht definiert` ab — eine globale Funktion aus einem frueheren Skript
    // war im naechsten nicht vorhanden. Ursache war `Engine::neu` **je
    // Skript** (in `skript_ausfuehren_mit_stand`): jeder Lauf bekam einen
    // frischen globalen Bereich. ES2023 16.1 sagt das Gegenteil — alle
    // klassischen `<script>` teilen **einen** `GlobalEnvironmentRecord`.
    //
    // Deshalb wird die Engine HIER einmal angelegt und im Skript-Loop nur
    // noch weitergereicht (`skript_auf_engine`). Die Host-Anbindung wird pro
    // Skript neu gesetzt, weil die `Kiste` pro Skript neu gebaut wird — das
    // ist unveraendert; nur die Engine bleibt.
    //
    // DREI DINGE, DIE DABEI AUCH STIMMEN:
    //  * Skriptreihenfolge: die Schleife ist linear ueber `seite.skripte`, also
    //    in Dokumentreihenfolge. `defer`/`async` aendern daran nichts, weil
    //    es **kein** zweiter, spaeterer Start gibt — die Reihenfolge ist die
    //    des Dokuments.
    //  * Die `Kiste` (Horcher/Befehle) wird pro Skript geleert und gelesen,
    //    genau wie vorher.
    //  * `lauf.engine` bekommt am Ende **eine** Engine mit allen Globals.
    let mut browser_grenzen = grenzen.clone();
    browser_grenzen.max_quelltext_bytes = MAX_SKRIPT_QUELLE;
    // 05.10.2026: von 1,5 Mio auf 6 Mio gehoben. GEMESSEN: youtubes
    // Haupt-Bundle (10,8 MB, ES5) braucht mehr als 2 Mio Token — mit der
    // alten Grenze wurde das ganze Skript abgelehnt. Die Ausfuehrung
    // bleibt durch `max_dauer_ms`/`max_schritte` begrenzt; die Zahl ist
    // die PARSE-Grenze, nicht die Laufzeitgrenze.
    browser_grenzen.max_token = 6_000_000;
    // 05.10.2026: minifizierte Buendel verschachteln tief (Versalien-
    // Ausdruecke, Objektliterale, Komma-Ketten). GEMESSEN: youtubes
    // ES5-Bundle lief bei der Standardtiefe 128 in `Verschachtelung zu
    // tief`. Der Motor laeuft auf dem 256-MiB-Thread (`auf_dickem_stapel`,
    // ~8 KiB Rust-Stapel je Ebene), 2048 Ebenen sind ~16 MiB — die harte
    // Grenze 4096 bleibt.
    browser_grenzen.max_parser_tiefe = 2048;
    // 05.10.2026: der HEAP-Spielraum fuer grosse Buendel. GEMESSEN an
    // youtubes ES5-Bundle: das kompilierte Programm sprengte 64 MiB
    // (`Programm groesser als das Heap-Limit (M4)`). 256 MiB entsprechen
    // dem Stapel des Motor-Threads; die Ausfuehrung bleibt durch
    // `max_dauer_ms` und `max_schritte` begrenzt.
    browser_grenzen.max_heap_bytes = 256 * 1024 * 1024;
    // Die Frist je SKRIPT: der Standard von 1,9 s reicht fuer den
    // Initialisierungscode grosser Seiten nicht. 6 s sind die Obergrenze
    // fuer EIN Skript; danach meldet der Motor `Zeitlimit` wie bisher.
    browser_grenzen.max_dauer_ms = 15_000;
    // Release 09.10.2026: 300 Mio. Schritte je Skript (vorher 50 Mio. fuer
    // ALLE Skripte zusammen): youtubes Hauptbuendel braucht mehr als 50 Mio.
    // allein fuer den Start. Die Wanduhr (15 s) bleibt die harte Grenze.
    browser_grenzen.max_schritte = 300_000_000;
    // Release 12.10.2026: eine prozessweite Frist (`js_frist_setzen`, z. B. der
    // WPT-Laeufer: Chrome bricht kein Skript ab).
    let gesetzt = JS_FRIST_MS.load(std::sync::atomic::Ordering::Relaxed);
    if gesetzt > 0 {
        browser_grenzen.max_dauer_ms = gesetzt;
    }
    // Release 09.10.2026 (Diagnose): `NARSIL_JS_FRIST_MS` ueberschreibt die Frist.
    if let Some(ms) = std::env::var("NARSIL_JS_FRIST_MS").ok().and_then(|v| v.parse::<u64>().ok()) {
        browser_grenzen.max_dauer_ms = ms;
    }
    let mut engine = Engine::neu(browser_grenzen);
    // Release 10.10.2026: die Seite meldet „Uncaught (in promise) …" wie
    // Chrome (HTML §8.1.7.3).
    engine.unbehandelte_ablehnungen_melden(true);
    // 06.10.2026: TIMER GEHOEREN DEM HOST.
    //
    // GEMESSEN (Masterplan D): der Motor leert seine Timerschlange sonst
    // SELBST am Ende von `ausfuehren` (autonome virtuelle Uhr) — ein
    // `setTimeout(fn, 3000)` der Seite lief damit waehrend des Ladens, und
    // die Seitenuhr hatte keine Kontrolle. `timer_vom_host(true)` laesst
    // die Timer stehen; `Sitzung::tick` leert sie gegen die Wanduhr
    // (`timerfaellig_und_ausfuehren_je_takt`).
    engine.timer_vom_host(true);
    // A4/E (06.10.2026): HERKUNFT UND NETZ FUER `fetch` — NUR FUER HTTP(S).
    //
    // GEMESSEN (Tests `javascript`, `handler_funktion`): `mit_herkunft`
    // wurde auch fuer `file://`-Seiten gerufen, und `Herkunft::aus_url`
    // las den Doppelpunkt in `C:\…` als Port („Port keine Zahl"). Jede
    // `file://`-Seite trug danach einen Fehler in der Konsole. Eine
    // Datei hat keine Netz-Herkunft; `fetch` bleibt dort abgelehnt, wie
    // in einem Browser ohne Server.
    if adresse.starts_with("http://") || adresse.starts_with("https://") {
        if let Err(e) = engine.mit_herkunft(adresse) {
            lauf.fehler.push(format!("fetch-Herkunft: {e}"));
        }
        engine.mit_netz(Box::new(KeksNetz {
            innen: narsil_jsvm::netz::NarsilNet { https_erlaubt: true },
            seite: adresse.to_string(),
        }));
        // Release 10.10.2026: fremde Herkuenfte nach CORS (wie Chrome).
        engine.cors_erlauben(true);
    }
    // E (06.10.2026): die gesammelten `<script type="module">`-Quellen.
    let mut modul_quellen: Vec<(String, String)> = Vec::new();
    {
        let mut host=kiste.lock().expect("Kiste");
        host.setze_ids(seite.ids.iter().map(|(name,ord)|(name.clone(),*ord as u32)).collect());
        host.setze_baum(&seite.css_baum,&seite.css_zu_dom);
        host.setze_dom_zu_ord(&seite.dom_zu_ord);
    }
    anbinden_mit_stand(&mut engine,kiste.clone(),stand.clone());
    // Release 09.10.2026: der VORSPANN (Web-APIs in JS: NodeFilter,
    // TreeWalker, NodeIterator …), siehe `vorspann.js`.
    // `NARSIL_VORSPANN_AUS` (nur Diagnose) laesst den Vorspann weg.
    if std::env::var_os("NARSIL_VORSPANN_AUS").is_none()
        && let Err(e) = engine.ausfuehren(VORSPANN_JS)
    {
        lauf.fehler.push(format!("Vorspann: {e}"));
    }
    // IndexedDB, then workers and message ports (after the preamble: they
    // build on its Event classes and DOMException; IndexedDB before
    // messaging, which hides the serializer).
    // Both are installed on first use (`lazy_apis.js`): a page that uses
    // neither does not parse them.
    if std::env::var_os("NARSIL_VORSPANN_AUS").is_none()
        && let Err(e) = engine.ausfuehren(crate::worker::LAZY_APIS_JS)
    {
        lauf.fehler.push(format!("Lazy APIs: {e}"));
    }
    // Release 09.10.2026 (Diagnose): `NARSIL_VORSPANN_EXTRA=<datei.js>` laeuft
    // nach dem Vorspann und vor den Seitenskripten (Messhaken fuer Seiten).
    if let Some(datei) = std::env::var_os("NARSIL_VORSPANN_EXTRA")
        && let Ok(quelle) = std::fs::read_to_string(datei)
        && let Err(e) = engine.ausfuehren(&quelle)
    {
        lauf.fehler.push(format!("Vorspann-Extra: {e}"));
    }
    //
    // W7 P1: DIE ZAHL DER EXTERNEN SKRIPTE (20:25).
    //
    // GEMESSEN, warum das die wichtigste Zahl der ganzen Messung ist: ein
    // `<script src="…">` wird mit einem EIGENEN `narsil_net::fetch`
    // geholt — und jeder Fetch hebt eine TLS-Verbindung auf. Auf
    // example.com sind das 22 s (Seite) + 11 s (s.js); auf einer Seite mit
    // 20 Skripten sind es 21 Verbindungen. Das ist keine JS-Frage, das
    // ist eine Netz-Frage — und `narsil-net` gehoert Rolle TLS.
    if crate::zeiten::an() {
        let draussen = seite
            .skripte
            .iter()
            .filter(|s| !s.quelle.is_empty())
            .count();
        let inline = seite.skripte.len().saturating_sub(draussen);
        std::eprintln!(
            "NARSIL_ZEIT js_skripte gesamt={} extern={draussen} inline={inline} \
             (jeder externe kostet einen eigenen TLS-Handshake)",
            seite.skripte.len()
        );
    }
    // Release 08.10.2026: DIE EXTERNEN SKRIPTE VORAB, PARALLEL. Ausgefuehrt
    // wird weiter in Dokumentreihenfolge — nur das Holen laeuft gleichzeitig
    // (wie im Preload-Scanner eines Browsers). GEMESSEN vorher (github.com):
    // 8 externe Skripte nacheinander, je ein eigener Abruf, `laden js`
    // 2 040 ms. Die Filter (Werbung, Erweiterungen, Sicherheitshaken) laufen
    // unveraendert in `quelle_holen`; ihr Zustand liegt in globalen Mutexen.
    // Release 08.10.2026: `NARSIL_JS_AUS` schaltet die Seitenskripte ab (nur
    // Diagnose: welcher Fehler kommt vom Skript, welcher vom Seitenbau?).
    let skript_zahl = if std::env::var_os("NARSIL_JS_AUS").is_some() { 0 } else { MAX_SKRIPTE };
    // Release 10.10.2026: zuerst die FRUEHEN Abrufe (gestartet nach dem
    // Parsen), dann nur noch, was dort fehlt.
    let mut vorab = if skript_zahl == 0 {
        Default::default()
    } else {
        let mut frueh = skripte_frueh_abholen(adresse);
        let fehlt: Vec<narsil_page::Skript> = seite
            .skripte
            .iter()
            .filter(|s| !s.quelle.is_empty() && !frueh.contains_key(&s.quelle))
            .cloned()
            .collect();
        frueh.extend(skripte_vorab_holen(&fehlt));
        frueh
    };
    // Release 14.10.2026: classic external `defer` scripts run after parsing,
    // in document order (HTML §4.12.1.1 „list of scripts that will execute
    // when the document has finished parsing"); `async` wins over `defer`,
    // and `defer` on an inline script is ignored. MEASURED before (WPT
    // execution-timing 085, 088, 092): a defer script ran at its own place,
    // before later async and parser scripts.
    let aufgeschoben = |s: &narsil_page::Skript| s.verzoegert && !s.asynchron && !s.quelle.is_empty() && s.typ != "module";
    let zahl = seite.skripte.len().min(skript_zahl);
    let reihenfolge: Vec<usize> = (0..zahl)
        .filter(|&i| !aufgeschoben(&seite.skripte[i]))
        .chain((0..zahl).filter(|&i| aufgeschoben(&seite.skripte[i])))
        .collect();
    for skript_nr in reihenfolge {
        let skript = &seite.skripte[skript_nr];
        if skript.zu_gross {
            lauf.fehler.push(format!("Inline-Skript zu gross: erlaubt {} Byte", narsil_page::MAX_SKRIPT_INHALT));
            continue;
        }
        // Release 10.10.2026: `<script type="importmap">` (HTML §8.1.5.3)
        // — die Karte fuer nackte Modulnamen (`react` -> URL), vor allen
        // Modulen eingetragen (die laufen erst nach den klassischen
        // Skripten). GEMESSEN: github.com braucht sie fuer React.
        if skript.typ == "importmap" {
            if skript.quelle.is_empty()
                && let Err(e) = engine.importmap_setzen(&skript.inhalt, adresse)
            {
                lauf.fehler.push(format!("Import-Map: {e}"));
            }
            continue;
        }
        // `type` ist eine Zusage (HTML): alles ausser `text/javascript`
        // und einem JavaScript-MIME-Typ ist KEIN Skript. Das ist der
        // Unterschied zwischen `<script type="text/template">` (Daten)
        // und einem Skript — und ein Schreibfehler im `type` darf die
        // Seite nicht leeren.
        // Release 14.10.2026: every JavaScript MIME type essence of the spec
        // (the same list `Lebend` uses for „already started").
        if skript.typ != "module" && !klassischer_skript_typ(&skript.typ) {
            continue;
        }
        // E (06.10.2026): `<script type="module">` ist KEIN klassisches
        // Skript. GEMESSEN: es lief durch `ausfuehren` — `import`/`export`
        // waren dort ein Syntaxfehler, und kein Modul der Seite kam je
        // zum Laufen. Module werden gesammelt und NACH den klassischen
        // Skripten ueber den Modul-Lader ausgefuehrt (HTML: Module sind
        // per Definition verzögert).
        if skript.typ == "module" {
            let quelle_js = if skript.quelle.is_empty() {
                skript.inhalt.clone()
            } else {
                match vorab.remove(&skript.quelle).unwrap_or_else(|| quelle_holen(&skript.quelle)) {
                    Ok(q) => q,
                    Err(grund) => {
                        lauf.fehler
                            .push(std::format!("Modul {}: {grund}", skript.quelle));
                        continue;
                    }
                }
            };
            let url = if skript.quelle.is_empty() {
                let ohne_anker = adresse.split('#').next().unwrap_or(adresse);
                format!("{ohne_anker}{}{}", narsil_jsvm::INLINE_MODUL_MARKE, modul_quellen.len())
            } else {
                narsil_url::loese_auf(adresse, &skript.quelle)
                    .unwrap_or_else(|_| skript.quelle.clone())
            };
            modul_quellen.push((url, quelle_js));
            continue;
        }
        let quelle_js = if skript.quelle.is_empty() {
            skript.inhalt.clone()
        } else {
            match vorab.remove(&skript.quelle).unwrap_or_else(|| quelle_holen(&skript.quelle)) {
                Ok(q) => q,
                Err(grund) => {
                    lauf.fehler
                        .push(std::format!("Skript {}: {grund}", skript.quelle));
                    continue;
                }
            }
        };
        // Release 09.10.2026 (Diagnose): `NARSIL_SKRIPTE_ABLEGEN=<ordner>`
        // legt jedes ausgefuehrte Skript als `extern-<nr>.js` ab — die
        // Fehlerstellen (`bei name:zeile`) lassen sich so nachlesen.
        if let Some(ordner) = std::env::var_os("NARSIL_SKRIPTE_ABLEGEN") {
            let _ = std::fs::create_dir_all(&ordner);
            let _ = std::fs::write(std::path::Path::new(&ordner).join(format!("extern-{skript_nr}.js")), &quelle_js);
        }
        {
            let mut w = kiste.lock().expect("Kiste");
            // `Seite::ids` ist `HashMap<String, usize>` — die
            // VORLAUF-NUMMER im geparsten Baum (siehe `narsil_page::Patch`).
            let mut ids: HashMap<String, u32> = HashMap::with_capacity(seite.ids.len());
            for (name, ord) in seite.ids.iter() {
                ids.insert(name.clone(), *ord as u32);
            }
            w.setze_ids(ids);
            w.setze_baum(&seite.css_baum, &seite.css_zu_dom);
        }
        if crate::zeiten::an() {
            std::eprintln!("NARSIL_ZEIT skript_start nr={skript_nr} quelle={} bytes={}", skript.quelle, quelle_js.len());
        }
        // Release 10.10.2026: `document.currentScript` waehrend des Laufs.
        {
            let host = seite
                .dom_zu_ord
                .get(&(skript.knoten.index() as u32))
                .and_then(|o| stand.lock().ok().and_then(|w| w.ids.get(o).copied()));
            if let Ok(mut w) = stand.lock() {
                w.aktuelles_skript = host;
            }
        }
        // Release 13.10.2026: ein Parser-Skript (klassisch, ohne async/defer)
        // sieht nur das Dokument BIS zu sich (HTML §13.2.6.4 „An end tag whose
        // tag name is script"); `document.write` schreibt dahinter.
        // Diagnostic switch NARSIL_TEILDOM_AUS: disables hiding the rest of the
        // document (A/B comparison on real pages).
        let parser_skript = !skript.asynchron && !skript.verzoegert && std::env::var_os("NARSIL_TEILDOM_AUS").is_none();
        let ausgeblendet = if parser_skript && let Ok(mut w) = stand.lock() {
            let host = w.aktuelles_skript;
            let veraltet = w.layout_veraltet;
            let hidden = match (host, w.lebend.as_mut()) {
                (Some(h), Some(l)) => match l.knoten(h) {
                    Some(k) => {
                        l.schreib_eltern = l.dok.knoten.get(k).and_then(|n| n.eltern);
                        Some((l.schwanz_ausblenden(k), veraltet))
                    }
                    None => None,
                },
                _ => None,
            };
            // Release 14.10.2026 (Agent C): the script sees only the sheets
            // before it — check the sheet set.
            if hidden.as_ref().is_some_and(|(liste, _)| !liste.is_empty()) {
                w.stil_pruefen = true;
            }
            hidden
        } else {
            None
        };
        let ergebnis = skript_auf_seitenengine(&quelle_js, &mut engine, &kiste, stand.clone());
        if let Ok(mut w) = stand.lock() {
            w.aktuelles_skript = None;
            if let Some((liste, veraltet_vorher)) = ausgeblendet {
                let leer = liste.is_empty();
                if let Some(l) = w.lebend.as_mut() {
                    l.schwanz_einblenden(liste);
                    l.schreib_eltern = None;
                }
                // Ein erzwungenes Layout des Teil-Dokuments gilt nicht mehr.
                if !leer && veraltet_vorher && !w.layout_veraltet {
                    w.layout_veraltet = true;
                }
                // Release 14.10.2026 (Agent C): the sheet set of the partial
                // document no longer holds either — check it again. MEASURED
                // before (WPT css-cascade revert-rule-important): the next
                // parser script still saw only the sheets before the first one.
                if !leer {
                    w.stil_pruefen = true;
                }
            }
        }
        // Release 10.10.2026: was das Skript an `<script>` eingefuegt hat.
        {
            let (k_dyn, f_dyn) = dyn_skripte_pumpen(&mut engine, &kiste, &stand);
            lauf.konsole.extend(k_dyn);
            lauf.fehler.extend(f_dyn);
        }
        // W6D (W9): die Engine gehoert der Seite — sie hat **alle** globalen
        // Variablen (aus allen Skripten) und die Handler-Wurzeln. Sie wird
        // oben **einmal** angelegt und hier nur weitergereicht.
        lauf.konsole.extend(ergebnis.konsole.iter().cloned());
        if let Some(f) = ergebnis.fehler {
            let source = if skript.quelle.is_empty() { format!("inline #{}", skript_nr + 1) } else { skript.quelle.clone() };
            lauf.fehler.push(format!("{source}: {f}"));
        }
        {
            // Die Horcher kommen aus der Kiste (dort hat das Skript sie
            // abgelegt). `k.clone()` statt `*k`: der Schluessel ist ein
            // Tupel aus `u32` und `String`, und `String` ist nicht `Copy`.
            let kopie = kiste.lock().expect("Kiste").horcher.clone();
            lauf.horcher.extend(kopie);
        }
        //
        // W6B: DIE ZWEI BEFEHLLISTEN.
        //
        // Die Kiste sammelt die `el*`-Funktionen, der DOM-Stand die
        // Standard-Eigenschaften (`z.textContent = …`). GEMESSEN, warum
        // sie getrennt bleiben und nicht in eine Liste gehen: die beiden
        // Wege gehoeren zu zwei verschiedenen Anbindungen (W6 Host-
        // Funktionen, W6B Host-Objekte), und eine gemeinsame Liste
        // wuerde nur verdecken, welcher Weg fehlt.
        //
        // DIE REIHENFOLGE ist gemessen und wichtig: erst der DOM, dann
        // die Kiste — das Standard-DOM ist der Weg, den echte Seiten
        // gehen, und es darf nicht von einer alten Funktion
        // ueberschrieben werden, die ein Skript zuerst angerufen hat.
        let dom_befehle = {
            let mut w = stand.lock().expect("DomStand");
            // 06.10.2026: Host-Meldungen (`alert`) in die Konsole — genau
            // einmal, in Dokumentreihenfolge.
            lauf.konsole.append(&mut w.meldungen);
            w.nimm_befehle()
        };
        let mut alle = dom_befehle;
        alle.extend(kiste.lock().expect("Kiste").nimm_befehle());
        // Die Befehle einspielen. GEMESSEN, warum ALLE Befehle eines
        // Skripts in EINEN Patch gehen und nicht je einen: die Seite wird
        // danach genau einmal neu gebaut (ein Parse und ein Layout), und
        // Befehle, die sich widersprechen (`add x` dann `remove x`),
        // ergeben in einem Patch die richtige Reihenfolge. Wer jeden
        // Befehl einzeln baut, baute 40 Mal fuer 40 Befehle — GEMESSEN
        // 2 ms je Bau, also 80 ms fuer einen Klick.
        if alle.iter().any(|b| b.veraendert_baum()) {
            let mut abgespielt = crate::replay::spielen(&mut lauf.patch, alle);
            crate::replay::neue_knoten_uebernehmen(&mut lauf.patch, &stand, &mut abgespielt);
            lauf.geaendert = lauf.geaendert || abgespielt.angewendet > 0;
            lauf.fehler.extend(abgespielt.fehler);
        }
        // Die HORCHER des DOM (W6B: `el.onclick = …`).
        {
            let w = stand.lock().expect("DomStand");
            for ((host_id, typ), quelle) in w.horcher.iter() {
                if let Some(ord) = w.ord.get(host_id) {
                    lauf.horcher
                        .insert((*ord as u32, typ.clone()), quelle.clone());
                }
            }
        }
    }
    // Release 14.10.2026: custom elements shown again after the last parser
    // script are parsed now (see `Lebend::upgrade_revealed`).
    if let Ok(mut w) = stand.lock()
        && let Some(l) = w.lebend.as_mut()
    {
        l.upgrade_revealed();
    }
    // E (06.10.2026): MODULE LAUFEN NACH DEN KLASSISCHEN SKRIPTEN.
    //
    // HTML: `<script type="module">` ist verzögert (wie `defer`) — erst
    // der Importgraph, dann die Rumpfe in Abhaengigkeitsreihenfolge, dann
    // `DOMContentLoaded`. Der Motor stellt `module_starten` (Quelle),
    // `module_offene` (fehlende Importe) und `module_laden` (ausfuehren);
    // die SOCKETS gehoeren dem Host (`modul_quelle_holen`).
    if !modul_quellen.is_empty() {
        for (url, quelle) in &modul_quellen {
            if let Err(e) = engine.module_starten(url, quelle) {
                lauf.fehler.push(format!("Modul {url}: {e}"));
            }
        }
        let vorher = engine.konsole().len();
        lauf.fehler.extend(module_pumpen(&mut engine));
        lauf.konsole
            .extend(engine.konsole().iter().skip(vorher).cloned());
    }
    handler_wurzeln_erneuern(&mut engine, &stand, &kiste);
    stand.lock().expect("DOM").ready_state="interactive";
    let doc_id=stand.lock().expect("DOM").document_id;
    let ready=crate::ereignis_host::dispatch(&mut engine,&stand,&kiste,doc_id,
        &[crate::dom::WINDOW_HOST_ID],"DOMContentLoaded",true,false,0.0,0.0,&lauf.horcher);
    lauf.konsole.extend(ready.console);
    lauf.fehler.extend(ready.errors);
    if ready.commands.iter().any(|b|b.veraendert_baum()) {
        let mut replay=crate::replay::spielen(&mut lauf.patch,ready.commands);
        crate::replay::neue_knoten_uebernehmen(&mut lauf.patch,&stand,&mut replay);
        lauf.geaendert|=replay.angewendet>0;
        lauf.fehler.extend(replay.fehler);
    }
    // A4/E (06.10.2026): DER NETZ-PUMP NACH DEM LADEN.
    //
    // GEMESSEN (Masterplan E): `fetch` konnte die Anfrage stellen, aber
    // niemand fuehrte sie aus (kein `netz_schleife`-Aufruf) — das
    // Versprechen blieb offen, und `.then` lief nie. Hier laufen die
    // Anfragen durch den Transport (`NarsilNet` -> narsil-net), danach die
    // Mikrotasks; die dabei entstandenen DOM-Befehle gehen in denselben
    // Patch wie die der Skripte.
    match engine.netz_schleife() {
        Ok(()) => {}
        Err(e) => lauf.fehler.push(format!("fetch: {e}")),
    }
    {
        let mut w = stand.lock().expect("DomStand");
        let netz_befehle = std::mem::take(&mut w.befehle);
        drop(w);
        let mut netz_alle = netz_befehle;
        netz_alle.extend(kiste.lock().expect("Kiste").nimm_befehle());
        if netz_alle.iter().any(|b| b.veraendert_baum()) {
            let mut replay = crate::replay::spielen(&mut lauf.patch, netz_alle);
            crate::replay::neue_knoten_uebernehmen(&mut lauf.patch, &stand, &mut replay);
            lauf.geaendert |= replay.angewendet > 0;
            lauf.fehler.extend(replay.fehler);
        }
    }
    // Release 09.10.2026 (Diagnose): `NARSIL_KONSOLE_DATEI=<datei>` schreibt
    // die GANZE Konsole und alle Skriptfehler (die Seite behaelt nur 20).
    if let Some(datei) = std::env::var_os("NARSIL_KONSOLE_DATEI") {
        let mut text = lauf.konsole.join("
");
        text.push_str("
--- Fehler ---
");
        text.push_str(&lauf.fehler.join("
"));
        let _ = std::fs::write(datei, text);
    }
    lauf.konsole.truncate(MAX_KONSOLE);
    // W6D (W9): die **eine** Engine wandert jetzt in die Seite — mit den
    // globalen Bindungen ALLER Skripte (vorher nur die des letzten).
    lauf.engine = Some(engine);
    // W7 P1: w1 ist der Rest (Lauf + zweiter Bau). Siehe `w0` oben.
    if let Some(m) = mw {
        m.ausgeben("js_lauf");
    }
    lauf
}

/// Holt den Quelltext eines `src`-Skripts — oder sagt, warum nicht.
///
/// # DIE DREI FAELLE, und alle drei sind J11
///
/// * `file://` — erlaubt, aber nur, wenn der Pfad keiner fremden Basis
///   entstammt? Nein: eine lokale Seite darf lokale Skripte laden. Der
///   Browser darf eine Datei lesen, die der Benutzer geoeffnet hat.
/// * `http://` — erlaubt (das ist „gleiche Herkunft" in der Praxis).
/// * Alles andere (`https`, `ftp`, `data`) — abgelehnt mit Namen.
/// E (06.10.2026): die Quelle eines Moduls holen (Importgraph).
///
/// Der Motor kennt keine Sockets (wie bei `fetch`) — der Host holt ueber
/// `laden::lade`, denselben Weg wie Seiten und Blaetter. `file://` und
/// `http(s)://` gehen damit; die Grenze ist `MAX_SKRIPT_QUELLE`.
pub fn modul_quelle_holen(url: &str) -> Result<String, String> {
    // Release 11.10.2026: der Modultyp aus den Import-Attributen
    // (`#narsil-typ=json`) bestimmt den erlaubten MIME-Typ.
    let (url, modultyp) = match url.find(narsil_jsvm::MODUL_TYP_MARKE) {
        Some(i) => (&url[..i], Some(&url[i + narsil_jsvm::MODUL_TYP_MARKE.len()..])),
        None => (url, None),
    };
    let url = url.split('#').next().unwrap_or(url);
    // data: and blob: modules (a module worker made from a Blob, an import of
    // a data: URL): the bytes are here, the type is the URL's (HTML §8.1.6.3).
    // MEASURED before: `import('data:text/javascript,...')` was "Failed to
    // fetch" (the loader only knew http(s) and file:).
    let schema = url.get(..5).map(str::to_ascii_lowercase);
    if matches!(schema.as_deref(), Some("data:") | Some("blob:")) {
        let (typ, bytes) = if schema.as_deref() == Some("data:") {
            narsil_jsvm::netz::daten_url_lesen(url).ok_or_else(|| "Modul nicht ladbar: data: URL ungueltig".to_string())?
        } else {
            crate::worker::blob_url_with_type(url).ok_or_else(|| "Modul nicht ladbar: blob: URL ohne Blob".to_string())?
        };
        let typ = typ.trim().to_ascii_lowercase();
        let typ = typ.split(';').next().unwrap_or("").trim().to_string();
        if !modul_typ_passt(&typ, modultyp, schema.as_deref() == Some("blob:")) {
            return Err(format!("Modul nicht ladbar: Typ {typ:?}"));
        }
        if bytes.len() > MAX_SKRIPT_QUELLE {
            return Err(format!("Modul zu gross: {} Byte (Grenze {MAX_SKRIPT_QUELLE})", bytes.len()));
        }
        let b = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
        return Ok(String::from_utf8_lossy(b).into_owned());
    }
    match crate::laden::lade(url) {
        Ok(l) => {
            // Release 11.10.2026: HTML §8.1.6.3 („fetch a single module
            // script") — eine Fehlerantwort oder ein MIME-Typ, der kein
            // JavaScript (oder JSON/CSS fuer Import-Attribute) ist, ist ein
            // Fehler. GEMESSEN vorher (WPT `dynamic-imports-fetch-error`):
            // `import()` einer 404-Seite oder von `text/plain` loeste auf.
            let http = url.starts_with("http://") || url.starts_with("https://");
            let typ = l.content_type.trim().to_ascii_lowercase();
            let typ = typ.split(';').next().unwrap_or("").trim().to_string();
            let js_typ = typ.is_empty()
                || matches!(
                    typ.as_str(),
                    "text/javascript" | "application/javascript" | "application/x-javascript" | "text/ecmascript"
                        | "application/ecmascript" | "text/x-javascript" | "text/x-ecmascript" | "application/x-ecmascript"
                        | "text/jscript" | "text/livescript"
                )
                || typ.starts_with("text/javascript1.");
            let passt = match modultyp {
                None => js_typ,
                Some("json") => matches!(typ.as_str(), "application/json" | "text/json") || typ.ends_with("+json"),
                Some("css") => typ == "text/css",
                Some("text") => true,
                Some(_) => false,
            };
            if http && (l.status >= 400 || !passt) {
                return Err(format!("Modul nicht ladbar: Status {}, Typ {typ:?}", l.status));
            }
            if l.text.len() > MAX_SKRIPT_QUELLE {
                Err(format!(
                    "Modul zu gross: {} Byte (Grenze {MAX_SKRIPT_QUELLE})",
                    l.text.len()
                ))
            } else {
                Ok(l.text)
            }
        }
        Err(e) => Err(format!("{e}")),
    }
}

/// Whether a data:/blob: module's type is allowed: JavaScript, or the type
/// an import attribute asks for. A Blob without a type counts as script
/// (the page made it for this purpose).
fn modul_typ_passt(typ: &str, modultyp: Option<&str>, blob: bool) -> bool {
    let js = matches!(
        typ,
        "text/javascript" | "application/javascript" | "application/x-javascript" | "text/ecmascript"
            | "application/ecmascript" | "text/x-javascript" | "text/x-ecmascript" | "application/x-ecmascript"
            | "text/jscript" | "text/livescript"
    ) || typ.starts_with("text/javascript1.")
        || (blob && typ.is_empty());
    match modultyp {
        None => js,
        Some("json") => matches!(typ, "application/json" | "text/json") || typ.ends_with("+json"),
        Some("css") => typ == "text/css",
        Some("text") => true,
        Some(_) => false,
    }
}

/// E (06.10.2026): den Importgraphen abarbeiten.
///
/// `module_offene` nennt die URLs, deren Quelle fehlt; jede wird geholt,
/// `module_quelle_setzen` reicht sie nach, `module_laden` fuehrt alles in
/// Abhaengigkeitsreihenfolge aus. Die Schleife endet, wenn nichts mehr
/// offen ist oder die Runden gedeckelt sind (ein Importkreis mit
/// fehlenden Quellen darf nicht ewig laufen).
pub fn module_pumpen(engine: &mut narsil_jsvm::Engine) -> Vec<String> {
    let mut fehler = Vec::new();
    let mut runden = 0usize;
    let mut vorige: Vec<String> = Vec::new();
    // Release 10.10.2026: „Module fehlen noch" ist kein Abbruch — die
    // naechste Runde holt sie. GEMESSEN vorher (github.com): `react-dom`
    // kam in Runde 2, sein Import `scheduler` war danach offen, und die
    // Pumpe brach an genau dieser Meldung ab — `scheduler` wurde nie
    // geholt, React nie gestartet. Die Meldung bleibt, falls am Ende
    // wirklich etwas fehlt.
    let mut fehlt_noch: Option<String> = None;
    loop {
        // Release 10.10.2026: auch OHNE offene Importe einmal laden — ein
        // Modul, dessen Importe schon da sind (ein eingefuegtes Inline-Modul,
        // das `./hilf.js` importiert), lief vorher nie.
        let offen: Vec<String> = engine
            .module_offene()
            .into_iter()
            .filter(|u| !engine.module_ist_gescheitert(u))
            .collect();
        if offen.is_empty() && runden > 0 {
            break;
        }
        // Release 10.10.2026: KEIN FORTSCHRITT -> aufhoeren. GEMESSEN
        // vorher: ein Modul, das nicht zu laden ist (Syntaxfehler, 404),
        // blieb „offen" und wurde 64 Runden lang neu geholt — in JEDEM Takt
        // (github.com: 150 s statt wenigen Sekunden).
        if runden > 0 && offen == vorige {
            break;
        }
        vorige = offen.clone();
        // Diagnose: `NARSIL_MODUL_SPUR=1` zeigt jede Runde der Pumpe.
        let spur = std::env::var_os("NARSIL_MODUL_SPUR").is_some();
        if spur {
            eprintln!("MODUL-PUMPE runde={runden} offen={offen:?}");
        }
        if runden >= 64 {
            fehler.push("Modulgraph zu gross (64 Runden)".to_string());
            break;
        }
        runden += 1;
        for url in offen {
            match modul_quelle_holen(&url) {
                Ok(quelle) => {
                    if let Err(e) = engine.module_quelle_setzen(&url, &quelle) {
                        fehler.push(format!("Modul {url}: {e}"));
                        engine.module_url_scheitern(&url, &format!("Modul {url}: {e}"));
                    }
                }
                Err(grund) => {
                    fehler.push(format!("Modul {url}: {grund}"));
                    engine.module_url_scheitern(&url, &format!("Failed to fetch dynamically imported module: {url}"));
                }
            }
        }
        let geladen = engine.module_laden();
        if spur {
            eprintln!("MODUL-PUMPE laden={geladen:?}");
        }
        match geladen {
            Ok(koerper_fehler) => {
                fehlt_noch = None;
                for f in koerper_fehler {
                    fehler.push(format!("Modul: {f}"));
                }
            }
            Err(e) if e.starts_with("Module fehlen noch") => fehlt_noch = Some(e),
            Err(e) => {
                fehler.push(format!("Modul: {e}"));
                break;
            }
        }
    }
    if let Some(e) = fehlt_noch
        && !engine.module_offene().is_empty()
    {
        fehler.push(format!("Modul: {e}"));
    }
    // Release 12.10.2026: haengt ein wartender Import nur noch an
    // gescheiterten Adressen (ein Import seines Graphen ist nicht zu laden),
    // wird er abgelehnt — sonst bliebe sein Versprechen fuer immer offen.
    if engine.module_wartende() > 0 {
        let offen = engine.module_offene();
        if !offen.is_empty() && offen.iter().all(|u| engine.module_ist_gescheitert(u)) {
            engine.module_import_abbrechen("Failed to fetch dynamically imported module");
        }
    }
    fehler
}

/// Release 08.10.2026: holt die externen Skripte einer Seite gleichzeitig
/// (hoechstens [`VORAB_FAEDEN`] auf einmal) und gibt sie nach Adresse
/// zurueck. Eine einzelne Adresse lohnt keinen Faden — dann bleibt die
/// Liste leer und der Lauf holt wie bisher.
fn skripte_vorab_holen(skripte: &[narsil_page::Skript]) -> HashMap<String, Result<String, String>> {
    skripte_vorab_holen_ab(skripte.iter(), 2)
}

/// Release 10.10.2026: die FRUEHEN Vorab-Abrufe, je Seitenadresse — gestartet
/// direkt nach dem Parsen (`skripte_frueh_starten`), abgeholt in
/// `skripte_ausfuehren`. GEMESSEN vorher (Netzspur, spiegel.de): die Skripte
/// gingen erst bei 858 ms raus, NACH Blaettern, Schriften und Kaskade
/// (424 ms) — obwohl ihre Adressen schon nach dem Parsen feststanden.
type FruehAbruf = (String, std::thread::JoinHandle<HashMap<String, Result<String, String>>>);

fn frueh_speicher() -> &'static Mutex<Option<FruehAbruf>> {
    static S: std::sync::OnceLock<Mutex<Option<FruehAbruf>>> = std::sync::OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}

/// Release 10.10.2026: die externen Skripte von `adresse` sofort im
/// Hintergrund holen (siehe `FruehAbruf`). Dieselben Filter wie immer
/// (`quelle_holen`: Werbung, Erweiterungen, Sicherheitshaken — die CSP
/// steht da schon fest, sie kommt aus dem Antwortkopf).
pub(crate) fn skripte_frueh_starten(adresse: &str, skripte: Vec<narsil_page::Skript>) {
    if skripte.is_empty()
        || std::env::var_os("NARSIL_JS_AUS").is_some()
        || std::env::var_os("NARSIL_SKRIPT_VORAB_AUS").is_some()
    {
        return;
    }
    let faden = std::thread::Builder::new()
        .name("narsil-skript-frueh".into())
        .spawn(move || skripte_vorab_holen_ab(skripte.iter(), 1));
    if let Ok(f) = faden
        && let Ok(mut g) = frueh_speicher().lock()
    {
        *g = Some((adresse.to_string(), f));
    }
}

/// Release 10.10.2026: die fruehen Ergebnisse fuer `adresse` abholen (und
/// auf sie warten); eine fremde Adresse bleibt unberuehrt.
fn skripte_frueh_abholen(adresse: &str) -> HashMap<String, Result<String, String>> {
    let f = match frueh_speicher().lock() {
        Ok(mut g) if g.as_ref().is_some_and(|(a, _)| a == adresse) => g.take(),
        _ => None,
    };
    f.and_then(|(_, h)| h.join().ok()).unwrap_or_default()
}

fn skripte_vorab_holen_ab<'a>(
    skripte: impl Iterator<Item = &'a narsil_page::Skript>,
    mindestens: usize,
) -> HashMap<String, Result<String, String>> {
    /// So viele Abrufe laufen hoechstens gleichzeitig.
    const VORAB_FAEDEN: usize = 6;
    let mut urls: Vec<String> = skripte
        .take(MAX_SKRIPTE)
        .filter(|s| !s.zu_gross && !s.quelle.is_empty())
        .filter(|s| {
            s.typ.is_empty()
                || matches!(
                    s.typ.as_str(),
                    "text/javascript" | "application/javascript" | "module" | "text/ecmascript"
                )
        })
        .map(|s| s.quelle.clone())
        .collect();
    urls.sort();
    urls.dedup();
    let mut raus = HashMap::new();
    if urls.len() < mindestens.max(1) || std::env::var_os("NARSIL_SKRIPT_VORAB_AUS").is_some() {
        return raus;
    }
    for gruppe in urls.chunks(VORAB_FAEDEN) {
        std::thread::scope(|bereich| {
            let faeden: Vec<_> = gruppe
                .iter()
                .map(|url| {
                    std::thread::Builder::new()
                        .name("narsil-skript".into())
                        .stack_size(8 * 1024 * 1024)
                        .spawn_scoped(bereich, move || (url.clone(), quelle_holen(url)))
                })
                .collect();
            for f in faeden {
                if let Ok(f) = f
                    && let Ok((url, ergebnis)) = f.join()
                {
                    raus.insert(url, ergebnis);
                }
            }
        });
    }
    raus
}

/// Release 10.10.2026: ist `type` ein klassisches JavaScript (HTML §4.12.1:
/// leer oder ein JavaScript-MIME-Typ)? Module und Daten laufen hier nicht.
pub(crate) fn klassischer_skript_typ(typ: &str) -> bool {
    let t = typ.trim().to_ascii_lowercase();
    t.is_empty()
        || matches!(
            t.as_str(),
            "text/javascript"
                | "application/javascript"
                | "text/ecmascript"
                | "application/ecmascript"
                | "application/x-javascript"
                | "text/x-javascript"
                | "text/jscript"
                | "text/livescript"
        )
}

/// Release 10.10.2026: per JS EINGEFUEGTE `<script>`-Elemente ausfuehren
/// (HTML §8.4.3 „prepare the script element"). `Lebend` merkt sie beim
/// Einfuegen vor; hier laufen sie nach der Aufgabe, die sie eingefuegt hat —
/// inline sofort, mit `src` nach dem Laden — mit `document.currentScript`,
/// danach `load` bzw. `error` am Element. Ein Skript kann weitere einfuegen
/// (Chunk-Lader), darum in Runden. Liefert (Konsole, Fehler).
///
/// GEMESSEN vorher: eingefuegte Skripte liefen NIE — Chunk-Lader (webpack,
/// Turbopack: bbc.com), Analyse- und Lazy-Skripte blieben still, Next.js
/// hydrierte nicht.
pub(crate) fn dyn_skripte_pumpen(
    engine: &mut Engine,
    kiste: &Geteilt,
    stand: &crate::dom::Geteilt,
) -> (Vec<String>, Vec<String>) {
    const MAX_RUNDEN: usize = 32;
    const MAX_JE_PUMPE: usize = 256;
    let mut konsole = Vec::new();
    let mut fehler = Vec::new();
    let mut gelaufen = 0usize;
    let mut seiten_adresse = String::new();
    for _ in 0..MAX_RUNDEN {
        let auftraege: Vec<(u32, Option<String>, String, bool, bool)> = {
            let Ok(mut w) = stand.lock() else { break };
            let adresse = w.adresse.clone();
            seiten_adresse = adresse.clone();
            let Some(l) = w.lebend.as_mut() else { break };
            if l.dyn_skripte.is_empty() {
                break;
            }
            let liste = std::mem::take(&mut l.dyn_skripte);
            liste
                .into_iter()
                .filter_map(|k| {
                    let typ = l.dok.attribut(k, "type").unwrap_or("").to_string();
                    // Release 10.10.2026: auch eingefuegte MODULE
                    // (`<script type="module">`, HTML §8.4.3 Schritt 33).
                    // GEMESSEN vorher: bbc.com laedt `dotcom-bootstrap.js` so
                    // — „NGAS load timeout", `window.dotcom.userinfo` blieb leer.
                    let ist_modul = typ.trim().eq_ignore_ascii_case("module");
                    if !ist_modul && (!klassischer_skript_typ(&typ) || l.dok.attribut(k, "nomodule").is_some()) {
                        return None;
                    }
                    // Release 11.10.2026: §4.12.1.1 — ein `src`, das leer ist
                    // oder sich nicht aufloesen laesst, ergibt `error`
                    // (danach, nicht im Einfuegen). GEMESSEN vorher (WPT
                    // fetch-src/empty, src-empty-or-invalid-error-event-
                    // queued): es lief als leeres Inline-Skript, kein Ereignis.
                    let roh_src = l.script_source(k).map(|s| s.trim().to_string());
                    let kaputt = roh_src.as_deref().is_some_and(|s| {
                        s.is_empty() || (!adresse.starts_with("file:") && narsil_url::loese_auf(&adresse, s).is_err())
                    });
                    let src = roh_src
                        .filter(|s| !s.is_empty())
                        // Wie die Skripte des Dokuments (`narsil-page` loest
                        // dort auf; kann auch `file://C:\…`).
                        .map(|s| narsil_page::loese_link(&s, &adresse));
                    let text = if src.is_some() || kaputt { String::new() } else { l.child_text(k) };
                    let host = l.host(k)?;
                    Some((host, src, text, ist_modul, kaputt))
                })
                .collect()
        };
        for (host, src, text, ist_modul, kaputt) in auftraege {
            if gelaufen >= MAX_JE_PUMPE {
                break;
            }
            gelaufen += 1;
            if kaputt {
                let fallback = kiste.lock().map(|k| k.horcher.clone()).unwrap_or_default();
                let r = crate::ereignis_host::dispatch(
                    engine, stand, kiste, host, &[], "error", false, false, 0.0, 0.0, &fallback,
                );
                konsole.extend(r.console);
                fehler.extend(r.errors);
                if let Ok(mut w) = stand.lock() {
                    w.befehle.extend(r.commands);
                }
                continue;
            }
            if ist_modul {
                // Modul: ueber den Modul-Lader (Importgraph, dann der Rumpf);
                // ein Inline-Modul bekommt eine eigene Adresse.
                let url = src.clone().unwrap_or_else(|| {
                    let ohne_anker = seiten_adresse.split('#').next().unwrap_or(&seiten_adresse);
                    format!("{ohne_anker}{}e{host}", narsil_jsvm::INLINE_MODUL_MARKE)
                });
                let quelle = match &src {
                    Some(u) => modul_quelle_holen(u),
                    None => Ok(text),
                };
                let mut ereignis = if src.is_some() { "load" } else { "" };
                match quelle {
                    Ok(q) => {
                        let vorher = engine.konsole().len();
                        match engine.module_starten(&url, &q) {
                            Ok(_) => fehler.extend(module_pumpen(engine)),
                            Err(e) => {
                                fehler.push(format!("Modul {url}: {e}"));
                                if src.is_some() {
                                    ereignis = "error";
                                }
                            }
                        }
                        konsole.extend(engine.konsole().iter().skip(vorher).cloned());
                    }
                    Err(g) => {
                        fehler.push(format!("Modul {url}: {g}"));
                        ereignis = "error";
                    }
                }
                if !ereignis.is_empty() {
                    let fallback = kiste.lock().map(|k| k.horcher.clone()).unwrap_or_default();
                    let r = crate::ereignis_host::dispatch(
                        engine, stand, kiste, host, &[], ereignis, false, false, 0.0, 0.0, &fallback,
                    );
                    konsole.extend(r.console);
                    fehler.extend(r.errors);
                    if let Ok(mut w) = stand.lock() {
                        w.befehle.extend(r.commands);
                    }
                }
                continue;
            }
            let (quelle, ereignis) = match &src {
                Some(url) => match quelle_holen(url) {
                    Ok(q) => (Some(q), "load"),
                    Err(g) => {
                        fehler.push(format!("Skript {url}: {g}"));
                        (None, "error")
                    }
                },
                None => (Some(text), ""),
            };
            if let Some(q) = quelle {
                if let Ok(mut w) = stand.lock() {
                    w.aktuelles_skript = Some(host);
                }
                let e = skript_auf_seitenengine(&q, engine, kiste, stand.clone());
                if let Ok(mut w) = stand.lock() {
                    w.aktuelles_skript = None;
                }
                konsole.extend(e.konsole.iter().cloned());
                if let Some(f) = e.fehler {
                    fehler.push(format!("{}: {f}", src.as_deref().unwrap_or("eingefuegtes Skript")));
                }
            }
            if !ereignis.is_empty() {
                let fallback = kiste.lock().map(|k| k.horcher.clone()).unwrap_or_default();
                let r = crate::ereignis_host::dispatch(
                    engine, stand, kiste, host, &[], ereignis, false, false, 0.0, 0.0, &fallback,
                );
                konsole.extend(r.console);
                fehler.extend(r.errors);
                if let Ok(mut w) = stand.lock() {
                    w.befehle.extend(r.commands);
                }
            }
        }
    }
    (konsole, fehler)
}

fn quelle_holen(url: &str) -> Result<String, String> {
    // M4: die Antwort wird begrenzt, BEVOR sie gelesen wird.
    const MAX_QUELLE_SKRIPT: usize = crate::js::MAX_SKRIPT_QUELLE;
    if let Some(rest) = url.strip_prefix("file://") {
        let pfad = crate::laden::dateipfad(rest).map_err(|e| e.to_string())?;
        let meta = std::fs::metadata(&pfad).map_err(|e| std::format!("{pfad}: {e}"))?;
        if meta.len() > MAX_QUELLE_SKRIPT as u64 {
            return Err(std::format!("Skript zu gross: {} Byte", meta.len()));
        }
        let bytes = std::fs::read(&pfad).map_err(|e| std::format!("{pfad}: {e}"))?;
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    }
    //
    // W6B, 30.09.2026: `https` geht mit. GEMESSEN, warum hier KEIN
    // eigener https-Zweig steht: `narsil-net` erkennt TLS am Port, also
    // reicht es, 443 zu setzen. Der Zweig unten ist also fuer beide
    // Schemata derselbe.
    if url.starts_with("http://") || url.starts_with("https://") {
        let https = url.starts_with("https://");
        let rest = match url.find("://") {
            Some(i) => &url[i + 3..],
            None => url,
        };
        let (host, pfad) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let standard_port: u16 = if https { 443 } else { 80 };
        let (host, port) = match host.rsplit_once(':') {
            Some((h, p)) => match p.parse::<u16>() {
                Ok(n) => (h.to_string(), n),
                Err(_) => (host.to_string(), standard_port),
            },
            None => (host.to_string(), standard_port),
        };
        if host.is_empty() {
            return Err("kein Host in der Adresse".into());
        }
        // A-ADBLOCK (Koordination 01.10.2026, 21:45, „vor jedem
        // Unterressourcen-Abruf"): DER FILTER, vor dem Abruf.
        //
        // GEMESSENE ENTSCHEIDUNG, warum hier und nicht im Aufrufer:
        // `<script src>` wird mit einem EIGENEN `narsil_net::fetch`
        // geholt (siehe den Kommentar weiter oben), also laeuft dieser
        // Weg an `bytes_holen` vorbei. Ohne die Zeile hier wuerden
        // Werbe-Skripte durchkommen, obwohl der Filter laeuft.
        if !narsil_page::werbung::erlaubt(
            url,
            narsil_page::werbung::Art::Skript,
            &narsil_page::werbung::seiten_host(),
        ) {
            return Err(std::format!("{url}: vom Werbefilter blockiert"));
        }
        //
        // G6b: UND DIE ERWEITERUNGEN. GEMESSEN, warum hier eine Zeile
        // noetig war, die vorher fehlte: der Werbefilter kennt
        // `Art::Skript`, die `declarativeNetRequest`-Regeln einer
        // Erweiterung kennen den Chrome-Namen `"script"`. Zwei Filter,
        // zwei Namen, eine Anfrage — und nur einer von beiden lief hier.
        // Die Koordination hat es als „DNR auch fuer script/stylesheet/
        // xhr" gemeldet; der Weg fuer `script` ist genau diese Zeile.
        if !crate::dienste::dnr_erlaubt(url, "script") {
            return Err(std::format!("{url}: von einer Erweiterung blockiert"));
        }
        //
        // added 03.10.2026, Rolle TLS (Freigabe der Leitung): DER
        // SICHERHEITSHAKEN — Mixed Content und CSP `script-src`, **vor** dem
        // Abruf.
        //
        // GEMESSENE ENTSCHEIDUNG fuer die Stelle: `bytes_holen` holt ein
        // `<script src>` mit einem **eigenen** `narsil_net::fetch`, also laeuft
        // dieser Weg an allem vorbei, was die **Seite** beim Laden einrichtet.
        // Genau wie beim Werbefilter (siehe oben) muss der Haken deshalb
        // **hier** stehen und nicht im Aufrufer.
        //
        // **UND DAS IST DER WICHTIGSTE FALL**, weil er der einzige ist, bei
        // dem das Ergebnis **sichtbar** ist: ein `http`-Skript auf einer
        // `https`-Seite ist Mixed Content §3.2 „blockable“ — der Haken
        // blockiert es **bevor** die Anfrage rausgeht.
        if let Err(grund) =
            crate::sicherheit_haken::gate(crate::sicherheit_haken::Unterart::Skript, url)
        {
            return Err(std::format!("{url}: gesperrt — {grund}"));
        }
        // Release 10.10.2026: das Schema ausdruecklich mitgeben — `narsil-net`
        // erkennt TLS sonst nur an Port 443 (`ist_https`). GEMESSEN vorher:
        // `https://localhost:8443/` ging als KLARTEXT-HTTP an den TLS-Port.
        let mut anfrage = narsil_net::Request::get(&host, port, pfad);
        anfrage.https = Some(https);
        let antwort = narsil_net::fetch(&anfrage).map_err(|e| std::format!("{url}: {e}"))?;
        if antwort.status.code >= 400 {
            return Err(std::format!("{url}: HTTP {}", antwort.status.code));
        }
        if antwort.body.len() > MAX_QUELLE_SKRIPT {
            return Err(std::format!("Skript zu gross: {} Byte", antwort.body.len()));
        }
        return Ok(String::from_utf8_lossy(&antwort.body).into_owned());
    }
    // W6B: `https` wird oben mitgenommen. Alles andere bleibt
    // abgelehnt (J11: kein Dateizugriff, kein fremdes Netz).
    Err(std::format!("Schema nicht geladen: {url}"))
}

// ===========================================================================
// J11 — EIN SKRIPT, MIT DEN VIER GRENZEN
// ===========================================================================

use std::time::{Duration, Instant};

/// Die Zeit, die ein Skript haben darf (J11).
///
/// GEMESSEN, warum 2 s: `VERTRAG_JS` sagt „max_schritte UND Wanduhr (ca.
/// 2 s)". Ein Skript, das laenger braucht, ist entweder kaputt oder
/// braucht Arbeit, die diese Engine nicht kann (Netz, Datei).
pub const ZEIT_PRO_SKRIPT: Duration = Duration::from_secs(2);

/// Das Ergebnis EINES Skriptlaufs.
///
/// `fehler` ist `Option<String>` und kein `JsFehler`: ein Skript, das
/// scheitert, darf die Seite nicht verschwinden lassen. Die Seite bleibt
/// stehen, und die Meldung steht in der Statuszeile.
//
// DER `stand` STEHT IN DER STRUKTUR, aber nicht im Vergleich: ein
// `Arc<Mutex<…>>` hat kein `PartialEq`, und ein Vergleich, der den
// DOM-Stand nicht vergleicht, waere eine halbe Zusage. Die Tests
// vergleichen deshalb die KONSOLE und den PATCH.
//
#[derive(Debug)]
pub struct LaufErgebnis {
    ///
    /// W6D: die Engine nach dem Lauf.
    ///
    /// GEMESSEN, warum sie herausgeht (14:25): ein Handler ist eine
    /// CLOSUE ueber die Variablen des Seitenskripts (`const z` in
    /// `zaehler.html`). Der Klick muss in DIESER Engine laufen — eine
    /// neue hat kein `z`, und der Handler scheitert dann an
    /// `z.textContent`.
    pub engine: Option<Engine>,
    /// Der DOM-Stand, in dem JavaScript gelesen und geschrieben hat
    /// (W6B).
    ///
    /// WARUM IM ERGEBNIS und nicht als Parameter zurueck: der Stand ist
    /// ein Zustand mit Lesewerten UND Schreibbefehlen, und der Aufrufer
    /// braucht beides (die Befehle in den Patch, die Lesewerte in die
    /// Seite). Ihn als Feld zu nehmen, heisst: der Zustand kann nicht
    /// zwischen zwei Aufrufen verloren gehen.
    pub stand: crate::dom::Geteilt,
    /// Die Ausgabe von `console` in der Reihenfolge.
    pub konsole: Vec<String>,
    /// Der benannte Fehler, falls einer auftrat.
    pub fehler: Option<String>,
    /// `true`, wenn das Dokument geaendert wurde (J10: neu zeichnen).
    pub geaendert: bool,
    /// Die verbrauchten Heap-Byte der Engine (J11: der Nachweis).
    pub heap: usize,
    /// Die verbrauchten Schritte der Engine (J11).
    pub schritte: u64,
    /// Die verbrauchte Zeit, gemessen (J11).
    pub dauer: Duration,
}

/// Fuehrt EIN Skript aus, mit allen Grenzen (J11).
///
/// # DIE VIER GRENZEN, und wo jede sitzt
///
/// | Grenze | Wert | Wo |
/// |---|---|---|
/// | Heap | 64 MiB | `Grenzen::max_heap_bytes` |
/// | Schritte | 50 Mio. | `Grenzen::max_schritte` |
/// | Zeit | 2 s | `Grenzen` UND die Messung hier |
/// | Aufruftiefe | 2000 | `Grenzen::max_aufruftiefe` (stapelsicher) |
///
/// Die Zeit steht doppelt, und das ist Absicht: `max_schritte` greift
/// fuer ein Skript mit vielen Schritten in kurzer Zeit, die Wanduhr fuer
/// eines mit wenigen Schritten und langer Rechnung. Eine Grenze allein
/// laesst die andere Art von Programm durch.
///
/// # WAS EIN FEHLER NICHT IST
///
/// Ein Skript mit `while(true){}` endet mit `Zeitlimit` — und das Fenster
/// bleibt bedienbar, weil die Engine in einem Kind-Thread laeuft
/// (`VERTRAG_JS`, 256 MiB Stapel) und die Messung hier im HAUPTTHREAD
/// stattfindet.
pub fn skript_ausfuehren(quelle: &str, kiste: &Geteilt, grenzen: Grenzen) -> LaufErgebnis {
    let stand = std::sync::Arc::new(std::sync::Mutex::new(crate::dom::Stand::default()));
    skript_ausfuehren_mit_stand(quelle, kiste, grenzen, stand)
}

/// Wie [`skript_ausfuehren`], aber mit dem DOM-Stand des Aufrufers.
///
/// W6B: der Stand gehoert zum HAUPTTHREAD (er kennt die Seite), und die
/// Befehle, die der Haken schreibt, muessen nach dem Lauf beim Aufrufer
/// landen. Ohne diesen zweiten Weg muesste der Aufrufer den Stand raten.
pub fn skript_ausfuehren_mit_stand(
    quelle: &str,
    kiste: &Geteilt,
    grenzen: Grenzen,
    stand: crate::dom::Geteilt,
) -> LaufErgebnis {
    let mut engine = Engine::neu(grenzen);
    engine.unbehandelte_ablehnungen_melden(true);
    let mut r = skript_auf_engine(quelle, &mut engine, kiste, stand);
    r.engine = Some(engine);
    r
}

/// Wie [`skript_ausfuehren_mit_stand`], aber auf einer **bestehenden** Engine.
///
/// # W9 (03.10.2026, JS) — WARUM DIESER ZWEITE WEG
///
/// `skripte_ausfuehren` legt die Engine **einmal** an und ruft ueber alle
/// `<script>` hinweg diese Funktion. Vorher bekam **jedes** Skript eine
/// `Engine::neu` — damit war der globale Bereich pro Skript neu, und eine
/// globale Funktion aus Skript N-1 war in Skript N **nicht** vorhanden.
///
/// GEMESSEN (Leitung, `team/log/leitung_wikipedia_2115.png`): auf
/// www.wikipedia.org bricht ein Skript mit
/// `ReferenceError: doWhenReady ist nicht definiert` ab. `doWhenReady` ist
/// eine globale Funktion (MediaWiki), die ein spaeteres Skript benutzt.
/// ES2023 16.1: klassische `<script>` teilen **einen** `GlobalEnvironmentRecord`.
///
/// Die Host-Anbindung wird hier **je Skript neu** gesetzt (die `Kiste` wird
/// im Skript-Loop pro Skript gebaut und befuellt) — das ist unveraendert wie
/// vorher; neu ist nur, dass die **Engine** die gleiche bleibt. Damit bleiben
/// ihre globalen Bindungen erhalten, und der Motor haengt das neue Skript
/// mit `Compiler::uebersetze_anhang` an (OPUS_1), sodass auch die Indizes
/// gueltig bleiben.
///
/// `LaufErgebnis::engine` ist hier **immer `None`** — die Engine gehoert dem
/// Aufrufer, der sie weiterreicht.
pub fn skript_auf_engine(
    quelle: &str,
    engine: &mut Engine,
    kiste: &Geteilt,
    stand: crate::dom::Geteilt,
) -> LaufErgebnis {
    anbinden_mit_stand(engine, kiste.clone(), stand.clone());
    skript_auf_seitenengine(quelle,engine,kiste,stand)
}

pub(crate) fn skript_auf_seitenengine(quelle:&str,engine:&mut Engine,kiste:&Geteilt,stand:crate::dom::Geteilt)->LaufErgebnis {
    crate::dom::vorgaenge_zuruecksetzen();
    // M4: die Quelle wird VOR dem Start der Engine begrenzt. Die Engine
    // begrenzt auch (`max_quelltext_bytes`), aber eine 20-MB-Datei soll
    // den Kanal nicht einmal sehen.
    if quelle.len() > MAX_SKRIPT_QUELLE {
        return LaufErgebnis {
            engine: None,
            stand,
            konsole: Vec::new(),
            fehler: Some(std::format!(
                "Skript zu gross: {} Byte, erlaubt {MAX_SKRIPT_QUELLE}",
                quelle.len()
            )),
            geaendert: false,
            heap: 0,
            schritte: 0,
            dauer: Duration::ZERO,
        };
    }
    let start = Instant::now();
    // Release 09.10.2026: jedes Skript hat sein EIGENES Schrittbudget, und
    // nur die NEUEN Konsolenzeilen gehoeren zu ihm. GEMESSEN vorher: der
    // Zaehler lief ueber alle Skripte (nach youtubes Hauptbuendel scheiterte
    // jedes weitere Inline-Skript sofort an der Schrittgrenze), und die
    // Konsole des Motors wurde nach JEDEM Skript ganz angehaengt (14-fach).
    engine.schritte_zuruecksetzen();
    let konsole_vorher = engine.konsole().len();
    let r = engine.ausfuehren(quelle);
    let dauer = start.elapsed();
    let konsole = engine.konsole().iter().skip(konsole_vorher).cloned().collect::<Vec<_>>();
    let heap = engine.heap_bytes();
    let schritte = engine.schritte();
    // Die Zahlen sind gelesen; `heap_bytes`/`schritte` sind reine Lesezuege.
    let geaendert = kiste.lock().expect("Kiste").geaendert;
    LaufErgebnis {
        engine: None,
        stand,
        konsole,
        fehler: r.err().map(|e| match engine.letzte_fehlerstelle() {
            Some(position) => format!("{}: {e} (bei {position})", e.art()),
            None => format!("{}: {e}", e.art()),
        }),
        geaendert,
        heap,
        schritte,
        dauer,
    }
}

/// Die Grenzen fuer einen Bomben-Test.
///
/// GEMESSEN, warum so klein: ein Test, der dieselben 64 MiB benutzt wie
/// der Browser, prueft die Browsergrenze und braucht dafuer 64 MiB. Ein
/// Test mit 1 MiB Heap und 10.000 Schritten prueft dieselbe REGEL in
/// zwei Millisekunden.
pub fn grenzen_fuer_bombe() -> Grenzen {
    let mut g = Grenzen::standard();
    g.max_heap_bytes = 1024 * 1024;
    g.max_schritte = 10_000;
    g
}

/// Fuellt den DOM-Stand aus dem GEPARSTEN Baum (W6B).
///
/// # WAS HIER STEHEN MUSS, und warum nicht mehr
///
/// | Angabe | woher |
/// |---|---|
/// | Vorlauf-Nummer -> Host-Id | eigene Zaehlung, `MAX_HOST_IDS` |
/// | Host-Id -> Vorlauf-Nummer | dieselbe Zaehlung, umgekehrt |
/// | Host-Id -> Tagname | der Baum |
/// | `id`-Attribut, `class`, `style` | der Baum |
/// | `document.body` | das erste Element mit dem Tag |
///
/// GEMESSEN, warum aus dem geparsten Baum und nicht aus dem Dokument:
/// `narsil-dom` speichert keine Attribute, und die Vorlauf-Nummer ist eine
/// Nummer des geparsten Baums (siehe `narsil_page::Patch`). Der Baum ist
/// die Quelle fuer beides — das Dokument waere eine zweite Wahrheit mit
/// den falschen Nummern.
///
/// M3/M5: die Schleife laeuft ueber den Baum (hoechstens `MAX_KNOTEN`
/// Knoten) und haelt `MAX_BAU_TIEFE` ein — das hier ist ein ZWEITER
/// Durchlauf ueber denselben Baum, nicht der erste.
/// 03.10. 13:10: DIE ZAHLEN DER NEUEN SEITE UEBERNEHMEN.
///
/// # WARUM DAS NOETIG IST (gemessen)
///
/// Nach den Skripten wird die Seite ein zweites Mal gebaut
/// (`narsil_page::seite_aus_knoten_mit_messung`, `sitzung.rs:2201`), und der
/// Patch kann Knoten entfernen (`Patch::weg`). GEMESSEN: dadurch bekommen die
/// Felder andere Zahlen als die erste Seite, und der Klick sucht nach der Zahl
/// aus `Seite::dom_zu_ord` — also nach der Zahl der ZWEITEN Seite.
///
/// GEMESSENE ENTSCHEIDUNG, warum nur die Zahlen wandern und nicht die ganze
/// Tabelle: ein getippter Wert steht im Patch, nicht im Baum — ein Neuaufbau
/// der Felder wuerde ihn als leer zeigen. Die Zahl ist das einzige, was der
/// zweite Bau verlegt.
///
/// GEMESSENE BEGRUENDUNG, warum die Zuordnung ueber die REIHENFOLGE laeuft und
/// nicht ueber eine Zahl: beide Tabellen gehen denselben Parse-Baum in
/// Dokumentreihenfolge durch, und ein Feld ist im Parse-Baum das i-te. Das
/// ist dieselbe Zaehlung wie beim Fuellen (13:05) — nur ohne Neuaufbau.
/// Release 09.10.2026: das lebende DOM der Seite anlegen (nach
/// `fuelle_stand`: die Host-Ids der Elemente stehen dann fest).
thread_local! {
    /// Release 11.10.2026: der Doctype der Seite, die gerade gebaut wird
    /// (aus dem Quelltext, `doctype_merken`). Der Seitenbaum (`HtmlNode`)
    /// traegt ihn nicht; `lebend_anlegen` setzt ihn als erstes Kind des
    /// Dokuments ein.
    static SEITEN_DOCTYPE: std::cell::RefCell<Option<(String, String, String)>> = const { std::cell::RefCell::new(None) };
}

/// Release 11.10.2026: den `<!DOCTYPE …>` am Anfang des Quelltexts merken
/// (Name, Public-ID, System-ID). GEMESSEN vorher: `document.doctype` war
/// undefined, `document.compatMode` damit immer „BackCompat".
pub(crate) fn doctype_merken(html: &str) {
    SEITEN_DOCTYPE.with(|d| *d.borrow_mut() = doctype_lesen(html));
}

thread_local! {
    /// Release 11.10.2026: die Seite kam als XHTML (`application/xhtml+xml`
    /// oder Endung) — das lebende Dokument wird ein XML-Dokument
    /// (`contentType`, `tagName` ohne Grossschreibung, CDATA erlaubt).
    static SEITEN_XHTML: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(crate) fn xhtml_merken(xhtml: bool) {
    SEITEN_XHTML.with(|x| x.set(xhtml));
}

fn doctype_lesen(html: &str) -> Option<(String, String, String)> {
    let mut rest = html.trim_start_matches('\u{feff}');
    // Leerraum und Kommentare vor dem Doctype ueberspringen.
    loop {
        rest = rest.trim_start();
        if let Some(r) = rest.strip_prefix("<!--") {
            rest = &r[r.find("-->")? + 3..];
            continue;
        }
        break;
    }
    if rest.len() < 9 || !rest[..9].eq_ignore_ascii_case("<!doctype") {
        return None;
    }
    let ende = rest.find('>')?;
    let innen = &rest[9..ende];
    let mut teile = innen.trim_start();
    let name_ende = teile.find(|c: char| c.is_ascii_whitespace()).unwrap_or(teile.len());
    let name = teile[..name_ende].to_ascii_lowercase();
    teile = teile[name_ende..].trim_start();
    let zitat = |t: &str| -> Option<(String, usize)> {
        let q = t.chars().next()?;
        if q != '"' && q != '\'' {
            return None;
        }
        let e = t[1..].find(q)?;
        Some((t[1..1 + e].to_string(), e + 2))
    };
    let (mut public_id, mut system_id) = (String::new(), String::new());
    if teile.len() >= 6 && teile[..6].eq_ignore_ascii_case("public") {
        teile = teile[6..].trim_start();
        if let Some((p, n)) = zitat(teile) {
            public_id = p;
            teile = teile[n..].trim_start();
            if let Some((s, _)) = zitat(teile) {
                system_id = s;
            }
        }
    } else if teile.len() >= 6 && teile[..6].eq_ignore_ascii_case("system") {
        teile = teile[6..].trim_start();
        if let Some((s, _)) = zitat(teile) {
            system_id = s;
        }
    }
    Some((name, public_id, system_id))
}

pub(crate) fn lebend_anlegen(stand: &crate::dom::Geteilt, baum: &[narsil_fmt_html::HtmlNode]) {
    let mut w = stand.lock().expect("DomStand");
    let doc = w.document_id;
    let mut l = crate::lebend::Lebend::neu(baum, &w.ids, doc);
    if let Some((name, public_id, system_id)) = SEITEN_DOCTYPE.with(|d| d.borrow_mut().take()) {
        let erstes = l.dok.kinder(l.wurzel).first().copied();
        let d = l.dok.neuer_knoten(narsil_fmt_html::dom::Art::Doctype { name, public_id, system_id });
        l.dok.kind_einfuegen(l.wurzel, d, erstes);
    }
    if SEITEN_XHTML.with(|x| x.replace(false)) {
        l.dok.xml_typ.insert(l.wurzel, "application/xhtml+xml".to_string());
    }
    w.lebend = Some(l);
    // Release 11.10.2026: die Dokumentkodierung aus dem ersten `<meta>`.
    w.kodierung = meta_kodierung(baum);
}

/// Release 11.10.2026: die Kodierung, die das erste `<meta charset>` bzw.
/// `<meta http-equiv=content-type content=…charset=…>` nennt — mit den
/// Prescan-Regeln (UTF-16 -> UTF-8, x-user-defined -> windows-1252).
/// (Eine Angabe nur im HTTP-Kopf fehlt hier noch.)
fn meta_kodierung(baum: &[narsil_fmt_html::HtmlNode]) -> Option<narsil_fmt_html::kodierung::Kodierung> {
    use narsil_fmt_html::kodierung::{Kodierung, kodierung_aus_label};
    fn suche(knoten: &[narsil_fmt_html::HtmlNode], tiefe: usize) -> Option<Option<Kodierung>> {
        if tiefe > 8 {
            return None;
        }
        for k in knoten {
            let narsil_fmt_html::HtmlNode::Element { tag, attributes, children, .. } = k else { continue };
            if tag.eq_ignore_ascii_case("meta") {
                if let Some(c) = attributes.get("charset") {
                    return Some(kodierung_aus_label(c));
                }
                let pragma = attributes.get("http-equiv").is_some_and(|h| h.trim().eq_ignore_ascii_case("content-type"));
                if pragma && let Some(inhalt) = attributes.get("content") {
                    let klein = inhalt.to_ascii_lowercase();
                    if let Some(i) = klein.find("charset=") {
                        let wert = inhalt[i + 8..].trim().trim_matches(|c| c == '"' || c == '\'');
                        let wert = wert.split(|c: char| c == ';' || c.is_ascii_whitespace()).next().unwrap_or("");
                        return Some(kodierung_aus_label(wert));
                    }
                }
            }
            if tag.eq_ignore_ascii_case("body") {
                continue;
            }
            if let Some(f) = suche(children, tiefe + 1) {
                return Some(f);
            }
        }
        None
    }
    let k = suche(baum, 0).flatten()?;
    Some(match k {
        Kodierung::Utf16Le | Kodierung::Utf16Be => Kodierung::Utf8,
        Kodierung::XUserDefined => Kodierung::Windows1252,
        k => k,
    })
}

/// Release 09.10.2026: den Lauf abschliessen. Was alte Wege in den `Patch`
/// geschrieben haben (Attribute, Text, Entfernen) und Klassen/Stil des
/// Proxy-Stands gehen in das lebende DOM; hat sich etwas geaendert, kommt
/// der neue Seitenbaum zurueck, der Proxy-Stand wird neu nummeriert und
/// der Patch behaelt nur die Canvas-Pixel (umgeschluesselt).
pub(crate) fn lebend_abschliessen(
    stand: &crate::dom::Geteilt,
    patch: &mut narsil_page::Patch,
) -> Option<Vec<narsil_fmt_html::HtmlNode>> {
    use narsil_fmt_html::dom::Namensraum;
    let mut w = stand.lock().expect("DomStand");
    let mut l = w.lebend.take()?;
    // Formularzustand (getippte Werte, Haken) ist KEIN DOM-Attribut: er
    // bleibt im Patch und wird nur umgeschluesselt.
    let ist_feldzustand = |name: &str| matches!(name, "value" | "checked" | "selected");
    let ist_feld = |w: &crate::dom::Stand, ord: &usize| {
        w.ids.get(ord).and_then(|h| w.tag.get(h)).is_some_and(|t| matches!(t.as_str(), "textarea" | "input" | "select" | "option"))
    };
    for ((ord, name), wert) in &patch.attribut {
        if name == STIL_MARKER || ist_feldzustand(name) {
            continue;
        }
        // `classList` alter Wege: `+x`/`-x`/`?x` gegen die aktuelle Klasse.
        if name == KLASSE_MARKER {
            if let Some(h) = w.ids.get(ord)
                && let Some(k) = l.knoten(*h)
            {
                let alt = l.dok.attribut(k, "class").unwrap_or("").to_string();
                let neu = narsil_page::klassen_wenden(&alt, wert);
                if neu != alt {
                    l.dok.setze_attribut(k, Namensraum::Html, "class", &neu);
                    l.geaendert = true;
                }
            }
            continue;
        }
        if let Some(h) = w.ids.get(ord)
            && let Some(k) = l.knoten(*h)
            && l.dok.attribut(k, name) != Some(wert.as_str())
        {
            l.dok.setze_attribut(k, Namensraum::Html, name, wert);
            l.geaendert = true;
        }
    }
    for (ord, text) in &patch.text {
        if ist_feld(&w, ord) {
            continue;
        }
        if let Some(h) = w.ids.get(ord)
            && let Some(k) = l.knoten(*h)
            && l.dok.text_inhalt(k) != *text
        {
            l.dok.text_inhalt_setzen(k, text);
            l.geaendert = true;
        }
    }
    for ord in &patch.weg {
        if let Some(h) = w.ids.get(ord)
            && let Some(k) = l.knoten(*h)
            && l.dok.eltern(k).is_some()
        {
            l.dok.abtrennen(k);
            l.geaendert = true;
        }
    }
    l.stand_uebernehmen(&w.klassen, &w.stil);
    if !l.geaendert {
        w.lebend = Some(l);
        return None;
    }
    let (html, ids) = l.html_neu();
    l.geaendert = false;
    let alt_ids = std::mem::replace(&mut w.ids, ids);
    let neue_ord: std::collections::HashMap<u32, usize> = w.ids.iter().map(|(o, h)| (*h, *o)).collect();
    w.ord = neue_ord;
    let paare: Vec<(usize, u32)> = w.ids.iter().map(|(o, h)| (*o, *h)).collect();
    for (_, h) in paare {
        if !w.tag.contains_key(&h)
            && let Some(k) = l.knoten(h)
            && let Some(n) = l.dok.element_name(k)
        {
            w.tag.insert(h, n.to_string());
        }
    }
    let canvas = std::mem::take(&mut patch.canvas);
    let feld_attr: Vec<((usize, String), String)> = std::mem::take(&mut patch.attribut)
        .into_iter()
        .filter(|((_, n), _)| ist_feldzustand(n))
        .collect();
    let feld_text: Vec<(usize, String)> = std::mem::take(&mut patch.text).into_iter().collect();
    *patch = narsil_page::Patch::default();
    let ord_karte: std::collections::HashMap<usize, usize> = alt_ids
        .iter()
        .filter_map(|(o, h)| w.ord.get(h).map(|n| (*o, *n)))
        .collect();
    let feld_alt: std::collections::HashSet<usize> = alt_ids
        .iter()
        .filter(|(_, h)| w.tag.get(h).is_some_and(|t| matches!(t.as_str(), "textarea" | "input" | "select" | "option")))
        .map(|(o, _)| *o)
        .collect();
    let neu_ord = |alt: usize| ord_karte.get(&alt).copied();
    for (alt, v) in canvas {
        if let Some(neu) = neu_ord(alt) {
            patch.canvas.insert(neu, v);
        }
    }
    for ((alt, n), v) in feld_attr {
        if let Some(neu) = neu_ord(alt) {
            patch.attribut.insert((neu, n), v);
        }
    }
    for (alt, v) in feld_text {
        if feld_alt.contains(&alt) && let Some(neu) = neu_ord(alt) {
            patch.text.insert(neu, v);
        }
    }
    let werte: Vec<(usize, String)> = std::mem::take(&mut w.eingabe_werte).into_iter().collect();
    for (alt, v) in werte {
        let neu = neu_ord(alt).unwrap_or(alt);
        w.eingabe_werte.insert(neu, v);
    }
    w.lebend = Some(l);
    drop(w);
    ord_erneuern(stand, &[], &[]);
    Some(html)
}

pub fn ord_erneuern(
    stand: &crate::dom::Geteilt,
    _felder: &[narsil_page::FeldOrd],
    _formular_ord: &[narsil_page::FormularOrd],
) {
    let mut w = stand.lock().expect("DomStand");
    let ord = w.ord.clone();
    for e in &mut w.eingaben {
        if let Some(source_ord) = ord.get(&e.host) { e.ord = *source_ord; }
    }
}

pub fn fuelle_stand(
    stand: &crate::dom::Geteilt,
    baum: &[narsil_fmt_html::HtmlNode],
    felder_ord: &[narsil_page::FeldOrd],
    formular_ord: &[narsil_page::FormularOrd],
) {
    //
    // 03.10. 13:05: DIE NUMMER KOMMT VOM BAUER, nicht von diesem Weg.
    //
    // GEMESSENER BEFUND (12:45, `tests/formular_mit_skript.rs`): auf einer
    // Seite mit laufendem Skript fand der Klick das Formularfeld nicht.
    // GEMESSENE URSACHE: dieser Weg zaehlte die Knoten selbst und kam zu
    // anderen Zahlen als `Seite::dom_zu_ord` — bei `<script>` VOR dem Formular
    // zaehlte er 7 und 8, der Bauer 6 und 7, und die Seite hatte keine 8.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Zahl vom Bauer kommt und nicht aus dem
    // Dokument geholt wird: `narsil_dom::Node` kennt nach dem Bau nur noch
    // Page/Container/Text — keine Tags, also auch keine Formularfelder. Der
    // Bauer ist der einzige, der beides WEISS: den Tag und die Zahl. Er
    // schreibt sie nach `Seite::felder` (siehe `narsil_page::FeldOrd`).
    //
    // GEMESSENE BEGRUENDUNG, warum der Zaehler hier trotzdem noetig ist: dieser
    // Weg LAEUFTE mehrfach (12:45: zweimal je Laden), und die Tabelle haengt an
    // (`push`). Die Ords des Bauers muessen also mitgezaehlt werden, weil sie
    // der REIHENFOLGE des Laufs folgen. Der Zaehler zaehlt nur noch, welcher
    // Eintrag in `felder_ord` der naechste ist — die Zahl gibt der Bauer.
    // 03.10. 13:30: die Zaehler liegen in `BauerZahlen` (siehe dort).
    /// W7C A2: Steht das BOOLEAN-Attribut da? (`checked`, `disabled`,
    /// `selected`, `multiple`)
    ///
    /// GEMESSEN, warum „das Attribut ist DA" und nicht „der Wert ist
    /// nicht leer/false": im HTML sind das Leerbitaet-Attribute
    /// (`<input checked>`), kein `checked="false"` — und
    /// `<input checked="">` ist dasselbe wie `<input checked>`. Wer den
    /// Wert vergleicht, haelt jedes echte `checked` fuer nicht gesetzt.
    fn attribute_da(attributes: &std::collections::HashMap<String, String>, name: &str) -> bool {
        match attributes.get(name) {
            // `checked=""` und `checked` sind dasselbe. Nur ein
            // ausdrueckliches `false` oder `no` hebt es auf — und das
            // gibt es im HTML gar nicht; wer es schreibt, meint es
            // ernst, also wird es beachtet.
            None => false,
            Some(v) => {
                let klein = v.trim().to_ascii_lowercase();
                klein.is_empty() || !matches!(klein.as_str(), "false" | "0" | "no")
            }
        }
    }

    /// W7C A2: Die `<option>`s einer Auswahl einsammeln (M3: hoechstens
    /// `MAX_EINGABEN`).
    ///
    /// GEMESSEN, warum rekursiv: `<select><optgroup><option>` ist
    /// alltaeglich, und ein Browser zeigt beide. Wer nur die direkten
    /// Kinder liest, verliert jede Auswahl mit einer Gruppe.
    fn sammle_optionen(
        liste: &[narsil_fmt_html::HtmlNode],
        raus: &mut Vec<crate::dom::AuswahlOption>,
        grenze: usize,
    ) {
        for k in liste {
            if raus.len() >= grenze {
                return;
            }
            let narsil_fmt_html::HtmlNode::Element {
                tag,
                attributes,
                children, ..
            } = k
            else {
                continue;
            };
            let klein = tag.to_ascii_lowercase();
            if klein == "option" {
                let text: String = children
                    .iter()
                    .filter_map(|c| match c {
                        narsil_fmt_html::HtmlNode::Text(t) => Some(t.as_str()),
                        _ => None,
                    })
                    .collect();
                raus.push(crate::dom::AuswahlOption {
                    // GEMESSEN, warum der Text als Ersatz fuer `value`:
                    // `<option>Bremen</option>` sendet „Bremen" — genau
                    // so machen es die Browser, wenn `value` fehlt.
                    wert: attributes.get("value").cloned().unwrap_or(text.clone()),
                    text,
                    gewaehlt: attribute_da(attributes, "selected"),
                    disabled: attribute_da(attributes, "disabled"),
                });
            } else if klein == "optgroup" {
                sammle_optionen(children, raus, grenze);
            }
        }
    }

    ///
    /// `in_steuerelement`: Wir sind unter einem `<input>`/`<button>`/
    /// `<select>`/`<textarea>`/`<option>`. Dann zaehlen die TEXT-Knoten
    /// nicht (Begründung unten, 05:55).
    /// 03.10. 13:30: DIE ZAHLEN DES BAUERS fuer den Lauf mitgeben.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum eine eigene Struktur und nicht zwei
    /// Parameter mehr: `lauf` ist eine innere `fn` mit sieben Parametern
    /// (einer davon ist der eigene Zaehler `ord`). Ein achter und neunter
    /// Parameter fuer zwei Zahlen waeren eine Signatur, die niemand mehr
    /// liest — und die GEMESSENE Fehlerklasse „zwei Stellen entscheiden"
    /// beginnt mit einer Signatur, die keiner mehr zu Ende lesen kann.
    ///
    /// GEMESSENE BEGRÜNDUNG, warum die Zähler hier liegen und nicht in
    /// `fuelle_stand`: `lauf` ist eine innere `fn`, keine Schliessfunktion —
    /// ein `E0434` („can't capture dynamic environment in a fn item") hat
    /// genau das gemeldet.
    struct BauerZahlen<'a> {
        felder: &'a [narsil_page::FeldOrd],
        formular: &'a [narsil_page::FormularOrd],
        /// Der naechste Eintrag aus `felder`.
        feld_nr: usize,
        /// Der naechste Eintrag aus `formular`.
        form_nr: usize,
    }

    fn lauf(
        liste: &[narsil_fmt_html::HtmlNode],
        w: &mut crate::dom::Stand,
        naechste: &mut u32,
        ord: &mut usize,
        zahlen: &mut BauerZahlen<'_>,
        tiefe: usize,
        in_steuerelement: bool,
    ) {
        if tiefe >= narsil_page::MAX_BAU_TIEFE {
            return;
        }
        for k in liste {
            //
            // W7C A2 (05:55): DIE ZAEHLREGEL DER STEUERELEMENTE.
            //
            // GEMESSENER FEHLER (drei Tests rot: „der Knopf kam nicht
            // an", obwohl er im Bild sichtbar war): `fuelle_stand` zaehlt
            // JEDEN Knoten — auch Text-Knoten. `narsil-page` legt fuer den
            // Beschriftungstext eines Steuerelements KEINEN Knoten an
            // (es zeichnet einen Platzhalter: `[Knopf: Sichern]`).
            // Also zaehlte `<button name=a value=1>Sichern</button>` hier
            // ZWEI Nummern (Knopf + Text) und dort EINE. Alles danach
            // war um eins verschoben: `dom_zu_ord` sagte fuer den zweiten
            // Knopf die 2, der DOM-Stand kannte ihn als 3 — und der
            // Klick auf einen Knoten, den es im Stand nicht gab, tat
            // nichts.
            //
            // GEMESSEN, warum es an DEN STEUERELEMENTEN liegt und nicht
            // ueberall: bei `<p>Text</p>` legt `narsil-page` einen echten
            // Text-Knoten an (das Layout zeichnet ihn), die Nummern
            // laufen also zusammen. Nur bei den Elementen, deren Inhalt
            // zu einem Platzhalter wird, geht die Zahl verloren.
            //
            // Das ist die Fehlerklasse „drei Zahlenraeume" aus W5G, und
            // diesmal nicht in `narsil-page`, sondern im DOM-Stand.
            // W7C A2 (06:05): AUCH DIE `<option>`-KNOTEN ZAEHLEN NICHT.
            //
            // GEMESSENER FEHLER (`auswahl_schickt_die_gewahlte_option`):
            // nach dem Klick auf die Auswahl stand im DOM-Stand
            // `eingaben=[(4, "submit")]` und in `dom_zu_ord` `{3: 2}` —
            // der Knopf war 2 und 4 zugleich. Ursache: die beiden
            // `<option>`s sind ELEMENTE (keine Textknoten), also hat die
            // Regel von vorhin sie nicht erwischt. `narsil-page` legt fuer
            // eine Option keinen Knoten an (sie steht im Platzhalter
            // `[Auswahl land: …]`), und `fuelle_stand` schon.
            //
            // GEMESSEN, warum man die Optionen nicht irgendwann braucht:
            // `fuelle_stand` liest sie an der STELLE (sammle_optionen),
            // und ab da sind sie in `Auswahl::optionen`. Hier werden sie
            // nur nicht mehr gezaehlt.
            if in_steuerelement {
                match k {
                    narsil_fmt_html::HtmlNode::Text(_) => continue,
                    narsil_fmt_html::HtmlNode::Element { tag, .. }
                        if matches!(tag.to_ascii_lowercase().as_str(), "option" | "optgroup") =>
                    {
                        continue;
                    }
                    _ => {}
                }
            }
            let me = *ord;
            *ord += 1;
            if let narsil_fmt_html::HtmlNode::Element {
                tag,
                attributes,
                children, ..
            } = k
            {
                if *naechste >= crate::dom::MAX_HOST_IDS {
                    return;
                }
                let id = *naechste;
                *naechste += 1;
                let klein = tag.to_ascii_lowercase();
                w.ids.insert(me, id);
                w.ord.insert(id, me);
                w.tag.insert(id, klein.clone());
                //
                // W7 P2: FORMULAR UND EINGABEFELD (21:05).
                //
                // GEMESSEN, warum HIER und nicht in `narsil-page`: dieser
                // Durchlauf geht den geparsten Baum EINMAL und kennt
                // Vorlauf-Nummer, Host-Id und Attribute. `narsil-dom`
                // verwirft die Attribute nach dem Bau, und `narsil-page`
                // muesste eine zweite Tabelle fuehren, die dieselben
                // Nummern noch einmal vergibt — die Fehlerklasse „drei
                // Zahlenraeume" aus W5G.
                //
                // M3: `MAX_EINGABEN` (256) Felder, `MAX_FORMULAR` (32)
                // Formulare, Name/Aktion/Methode auf 128 Zeichen
                // gekuerzt (ein Name ist ein Bezeichner, keine Seite).
                if klein == "form" && w.formulare.len() < crate::dom::MAX_FORMULAR {
                    zahlen.form_nr += 1;
                    w.formulare.push(crate::dom::Formular {
                        //
                        // 03.10. 13:05: DIE ZAHL DES BAUERS (siehe oben).
                        // GEMESSENE ENTSCHEIDUNG, warum NUR die Zahl von ihm
                        // kommt und nicht `aktion`/`methode`: die Rohwerte
                        // stehen in `narsil_page::FormularOrd` genauso, aber
                        // dieser Weg schreibt `methode` in GROSSBUCHSTABEN
                        // (`POST`), weil `formular_senden` danach vergleicht.
                        // Zwei Schreibweisen fuer eine Angabe waere wieder eine
                        // zweite Wahrheit — also bleibt sie hier, und die ZAHL
                        // kommt von dort, wo sie vergeben wird.
                        ord: zahlen
                            .formular
                            .iter().find(|f| f.ord == me)
                            .map(|f| f.ord)
                            .unwrap_or(me),
                        //
                        // GEMESSENER FEHLER (03.10. 06:35): die `action`
                        // stand ROH im Formular. Sie kam als `/ziel` aus
                        // dem HTML, `formular_get_url` lieferte
                        // `Ok("/ziel?n=x")` — das geht durch. Aber
                        // `Sitzung::gehe_zu` macht aus `/ziel?n=x` eine
                        // DATEI (`waehle_eingabe`: „alles, was mit `/`
                        // beginnt, ist `file://`"). Gemessen wurde: ein
                        // GET-Formular schickte NICHTS, es kam kein
                        // einziger Request beim Server an.
                        //
                        // GEMESSEN, warum das JEDE echte Seite betrifft:
                        // eine relative `action` ist die uebliche Form
                        // (der Browser kennt die Basis). Jedes
                        // Suchformular im Web hat `action="/suche"` oder
                        // `action=""`. Die LEERE `action` funktionierte
                        // (sie wird gegen die Basis genommen), die
                        // relative nicht.
                        //
                        // DER WEG: `narsil_page::loese_link` — dieselbe
                        // Funktion, die `<script src>` und `<a href>`
                        // aufloest. Ein zweiter Aufloeser waere eine
                        // zweite Wahrheit ueber dieselbe Frage (und
                        // wuerde `..`, Anker und `file://` anders
                        // behandeln).
                        //
                        // GEMESSEN, warum die Adresse aus `w` und nicht
                        // als Parameter: `lauf` laeuft TIEF durch den Baum
                        // (bis `MAX_BAU_TIEFE`), und ein vierter Parameter
                        // waere an jeder der Rekursionen mitzuschleppen.
                        // Der Stand TRAEGT die Adresse schon
                        // (`Stand::adresse`, gesetzt in
                        // `skripte_ausfuehren`).
                        aktion: match attributes.get("action") {
                            Some(a) if !a.trim().is_empty() => {
                                narsil_page::loese_link(a.trim(), &w.adresse)
                            }
                            // DIE LEERE ACTION bleibt leer:
                            // `formular_senden` nimmt dann die Basis der
                            // Seite. Eine hier eingesetzte Basis waere
                            // eine zweite Wahrheit ueber „wohin geht das
                            // Formular ohne action".
                            _ => std::string::String::new(),
                        },
                        methode: attributes
                            .get("method")
                            .map(|m| m.to_ascii_uppercase())
                            .unwrap_or_else(|| "GET".to_string()),
                        // GEMESSEN, warum `post` (nicht `post` klein):
                        // die Schreibweise entscheidet, nicht die
                        // Gross/Klein-Schreibung — ein Browser schickt
                        // `POST`, ein Formular mit `method="Post"` auch.
                        felder: Vec::new(),
                        enctype: attributes
                            .get("enctype")
                            .map(|e| e.trim().to_ascii_lowercase())
                            .unwrap_or_default(),
                    });
                }
                //
                // W7C A2: ALLE `<input>`-ARTEN (02:10), nicht nur die
                // tippbaren.
                //
                // GEMESSENER FEHLER, vom Test gefunden: `fuelle_stand`
                // sammelte NUR Felder, in die man tippen kann
                // (`tippbar()`). Damit waren `hidden`, `checkbox`,
                // `radio` und `submit` fuer das Formular INEXISTENT —
                // ein Formular mit einem versteckten Feld (`<input
                // type=hidden name=csrf value=…>`) schickte kein
                // Token, und die Anmeldung schlug fehl. Das ist der
                // haeufigste Fehler in einem Formular-Browser: er
                // sammelt die Felder, die man sieht.
                //
                // GEMESSEN, warum trotzdem `MAX_EINGABEN` (256) gilt und
                // nicht mehr: ein Dokument mit 10.000 Feldern ist ein
                // Angriff, und die Grenze ist fuer ALLE Arten, nicht nur
                // fuer die sichtbaren.
                if klein == "input" && w.eingaben.len() < crate::dom::MAX_EINGABEN {
                    let typ = attributes
                        .get("type")
                        .map(|t| t.to_ascii_lowercase())
                        .unwrap_or_else(|| "text".to_string());
                    zahlen.feld_nr += 1;
                    w.eingaben.push(crate::dom::Eingabe {
                        //
                        // 03.10. 13:05: DIE ZAHL DES BAUERS (siehe oben).
                        // GEMESSENE BEGRUENDUNG fuer den Rueckfall auf `me`:
                        // die Grenze des Bauers (512, `MAX_FORMULARFELDER`)
                        // liegt ueber der dieses Weges (256,
                        // `crate::dom::MAX_EINGABEN`), also greift der
                        // Rueckfall erst, wenn der Bauer selbst gekuerzt hat —
                        // und dann gibt es keine bessere Zahl.
                        ord: zahlen
                            .felder
                            .iter().find(|f| f.ord == me)
                            .map(|f| f.ord)
                            .unwrap_or(me),
                        host: id,
                        name: attributes
                            .get("name")
                            .map(|n| n.chars().take(128).collect())
                            .unwrap_or_default(),
                        typ: typ.clone(),
                        wert: attributes
                            .get("value")
                            .map(|v| v.chars().take(crate::dom::MAX_EINGABE_WERT).collect())
                            .unwrap_or_default(),
                        checked: attribute_da(attributes, "checked"),
                        disabled: attribute_da(attributes, "disabled"),
                        absender: matches!(typ.as_str(), "submit" | "button" | "image" | "reset"),
                        datei: typ == "file",
                        // Das Formular, in dem das Feld steht: der
                        // LETZTE `<form>` ueber diesem Pfad.
                        formular: w.letztes_formular,
                    });
                }
                //
                // W7C A2: `<textarea>` (02:15). Der Wert ist der TEXT
                // des Elements, nicht ein `value`-Attribut — so steht es
                // in der HTML, und so ist es in jedem Browser.
                if klein == "textarea" && w.eingaben.len() < crate::dom::MAX_EINGABEN {
                    let inhalt: String = children
                        .iter()
                        .filter_map(|c| match c {
                            narsil_fmt_html::HtmlNode::Text(t) => Some(t.as_str()),
                            _ => None,
                        })
                        .collect();
                    zahlen.feld_nr += 1;
                    w.eingaben.push(crate::dom::Eingabe {
                        //
                        // 03.10. 13:05: DIE ZAHL DES BAUERS (siehe oben).
                        // GEMESSENE BEGRUENDUNG fuer den Rueckfall auf `me`:
                        // die Grenze des Bauers (512, `MAX_FORMULARFELDER`)
                        // liegt ueber der dieses Weges (256,
                        // `crate::dom::MAX_EINGABEN`), also greift der
                        // Rueckfall erst, wenn der Bauer selbst gekuerzt hat —
                        // und dann gibt es keine bessere Zahl.
                        ord: zahlen
                            .felder
                            .iter().find(|f| f.ord == me)
                            .map(|f| f.ord)
                            .unwrap_or(me),
                        host: id,
                        name: attributes
                            .get("name")
                            .map(|n| n.chars().take(128).collect())
                            .unwrap_or_default(),
                        typ: "textarea".to_string(),
                        wert: inhalt.chars().take(crate::dom::MAX_EINGABE_WERT).collect(),
                        checked: false,
                        disabled: attribute_da(attributes, "disabled"),
                        absender: false,
                        datei: false,
                        formular: w.letztes_formular,
                    });
                }
                //
                // W7C A2: `<button>` (04:10).
                //
                // GEMESSENER FEHLER, vom Test gefunden: `<button
                // type="submit">` war fuer das Formular INEXISTENT — der
                // Knoten-Zweig sammelte nur `<input>`. Ein Klick auf den
                // Knopf schickte also gar nichts, und die Meldung im Bild
                // war ein Knopf, der es nicht war. `<button>` ist seit
                // HTML 4.01 der Standard-Knopf; es zu behandeln wie ein
                // Absatz hiesse, den haeufigsten Knopf des Webs zu ignorieren.
                //
                // GEMESSEN, warum `typ="button"` (ohne `submit`) NICHT
                // absendet: das ist ein Knopf fuer JavaScript, kein
                // Absender. Er gehoert zu keinem Formular, das man
                // abschickt — genau so behandeln es die Browser.
                //
                // M3: `MAX_EINGABEN` (256), wie bei `<input>`.
                if klein == "button" && w.eingaben.len() < crate::dom::MAX_EINGABEN {
                    let typ = attributes
                        .get("type")
                        .map(|t| t.to_ascii_lowercase())
                        .unwrap_or_else(|| "submit".to_string());
                    // Der sichtbare Text ist die Beschriftung; `value`
                    // ist, was gesendet wird.
                    let beschriftung: String = children
                        .iter()
                        .filter_map(|c| match c {
                            narsil_fmt_html::HtmlNode::Text(t) => Some(t.trim()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    zahlen.feld_nr += 1;
                    w.eingaben.push(crate::dom::Eingabe {
                        //
                        // 03.10. 13:05: DIE ZAHL DES BAUERS (siehe oben).
                        // GEMESSENE BEGRUENDUNG fuer den Rueckfall auf `me`:
                        // die Grenze des Bauers (512, `MAX_FORMULARFELDER`)
                        // liegt ueber der dieses Weges (256,
                        // `crate::dom::MAX_EINGABEN`), also greift der
                        // Rueckfall erst, wenn der Bauer selbst gekuerzt hat —
                        // und dann gibt es keine bessere Zahl.
                        ord: zahlen
                            .felder
                            .iter().find(|f| f.ord == me)
                            .map(|f| f.ord)
                            .unwrap_or(me),
                        host: id,
                        name: attributes
                            .get("name")
                            .map(|n| n.chars().take(128).collect())
                            .unwrap_or_default(),
                        typ: if typ == "button" {
                            "button".into()
                        } else {
                            typ.clone()
                        },
                        wert: attributes
                            .get("value")
                            .map(|v| v.chars().take(crate::dom::MAX_EINGABE_WERT).collect())
                            .unwrap_or_else(|| {
                                beschriftung
                                    .chars()
                                    .take(crate::dom::MAX_EINGABE_WERT)
                                    .collect()
                            }),
                        checked: false,
                        disabled: attribute_da(attributes, "disabled"),
                        absender: typ != "button",
                        datei: false,
                        formular: w.letztes_formular,
                    });
                }
                //
                // W7C A2: `<select>` (02:20).
                //
                // GEMESSEN, warum die Optionen hier und nicht spaeter
                // gesammelt werden: sie stehen im Kindbaum des
                // `<select>`, und der Durchlauf sieht ihn jetzt — spaeter
                // waere es ein zweiter Durchlauf ueber denselben Baum.
                if klein == "select" && w.auswahl.len() < crate::dom::MAX_AUSWAHLEN {
                    let mut optionen: Vec<crate::dom::AuswahlOption> = Vec::new();
                    sammle_optionen(children, &mut optionen, crate::dom::MAX_EINGABEN);
                    // 03.10. 14:35: DIE AUSWAHL IST AUCH EIN FELD —
                    // der Zaehler muss mitlaufen, sonst zaehlt er nicht
                    // bis zum Ende (E0509, gemessen 14:30: der Absender
                    // kam danach aus der Tabelle heraus).
                    zahlen.feld_nr += 1;
                    w.auswahl.push(crate::dom::Auswahl {
                        //
                        // 03.10. 13:05: DIE ZAHL DES BAUERS (siehe oben).
                        // GEMESSENE BEGRUENDUNG fuer den Rueckfall auf `me`:
                        // die Grenze des Bauers (512, `MAX_FORMULARFELDER`)
                        // liegt ueber der dieses Weges (256,
                        // `crate::dom::MAX_EINGABEN`), also greift der
                        // Rueckfall erst, wenn der Bauer selbst gekuerzt hat —
                        // und dann gibt es keine bessere Zahl.
                        ord: zahlen
                            .felder
                            .iter().find(|f| f.ord == me)
                            .map(|f| f.ord)
                            .unwrap_or(me),
                        host: id,
                        name: attributes
                            .get("name")
                            .map(|n| n.chars().take(128).collect())
                            .unwrap_or_default(),
                        optionen,
                        disabled: attribute_da(attributes, "disabled"),
                        mehrfach: attribute_da(attributes, "multiple"),
                        formular: w.letztes_formular,
                    });
                }
                if let Some(v) = attributes.get("id") {
                    w.dom_ids.insert(me, v.clone());
                    w.attribute
                        .entry(id)
                        .or_default()
                        .insert("id".to_string(), v.clone());
                }
                let kl: Vec<String> = attributes
                    .get("class")
                    .map(|c| c.split_whitespace().map(str::to_string).collect())
                    .unwrap_or_default();
                if !kl.is_empty() {
                    w.klassen.insert(id, kl);
                }
                //
                // W6B: ALLE ATTRIBUTE (fuer `getAttribute`).
                //
                // GEMESSEN, warum das noetig ist (15:40):
                // `getAttribute("data-alt")` gab `null`, obwohl das
                // Attribut im HTML stand. Der Stand hatte nur `id` —
                // `narsil-dom` speichert keine Attribute, also muss der
                // Baum sie hierher liefern.
                //
                // M4: die Attributliste eines Elements ist durch die
                // HTML-Grenzen begrenzt; hier wird jede uebernommen, was
                // der Parser behalten hat.
                {
                    let eigen_attribute = w.attribute.entry(id).or_default();
                    for (n, v) in attributes.iter() {
                        eigen_attribute.insert(n.clone(), v.clone());
                    }
                }
                if let Some(a) = attributes.get("style") {
                    for teil in a.split(';') {
                        if let Some((n, v)) = teil.split_once(':') {
                            w.stil
                                .entry(id)
                                .or_default()
                                .push((n.trim().to_string(), v.trim().to_string()));
                        }
                    }
                }
                if klein == "body" {
                    w.standard.insert("body".to_string(), id);
                }
                if klein == "head" {
                    w.standard.insert("head".to_string(), id);
                }
                // 05.10.2026: `document.documentElement` war NICHT belegt —
                // der Getter lieferte `null`, und `document.documentElement.animate`
                // (web-animations-Polyfill auf youtube) brach mit
                // `TypeError: kein Zugriff auf Null (Eigenschaft animate)` ab.
                // GEMESSEN: `standard` hatte nur `body`/`head`.
                if klein == "html" {
                    w.standard.insert("documentElement".to_string(), id);
                }
                //
                // W6B: DER TEXT DES ELEMENTS.
                //
                // GEMESSEN, warum das noetig ist (15:35): `z.textContent`
                // lieferte `""` — der Getter las aus `Stand.text`, und da
                // stand nichts drin. Ein Getter, der immer leer liefert,
                // macht `Number("")` zu `0`, und der Zaehler stuende
                // bei 1 statt 2. Der Text kommt aus den direkten
                // Text-KINDern (die Enkel gehoeren zu ihren Elementen).
                let eigen: String = children
                    .iter()
                    .filter_map(|c| match c {
                        narsil_fmt_html::HtmlNode::Text(t) => Some(t.as_str()),
                        _ => None,
                    })
                    .collect();
                if !eigen.is_empty() {
                    w.text.insert(id, eigen);
                }
                //
                // W7 P2: DER FORMULAR-STAPEL (21:10). Ein `<input>` gehoert
                // zum LETZTEN `<form>` ueber ihm. Also wird der Stack hier
                // betreten und nach den Kindern (und damit nach dem
                // ganzen Teilbaum) wieder verlassen.
                let war_formular = w.letztes_formular;
                if klein == "form" {
                    w.letztes_formular = Some(w.formulare.len().saturating_sub(1));
                }
                // Die Steuerelemente, deren Text nicht mitzaehlt.
                let kind_steuerelement = matches!(
                    klein.as_str(),
                    "input" | "button" | "select" | "textarea" | "option" | "optgroup"
                );
                lauf(
                    children,
                    w,
                    naechste,
                    ord,
                    zahlen,
                    tiefe + 1,
                    in_steuerelement || kind_steuerelement,
                );
                w.letztes_formular = war_formular;
            }
        }
    }
    let mut w = stand.lock().expect("DomStand");
    let mut naechste: u32 = 1; // 0 ist `document`.
    let mut ord: usize = 0;
    // 03.10. 13:30: die Tabellen des Bauers fuer diesen Lauf (siehe oben).
    let mut zahlen = BauerZahlen {
        felder: felder_ord,
        formular: formular_ord,
        feld_nr: 0,
        form_nr: 0,
    };
    lauf(baum, &mut w, &mut naechste, &mut ord, &mut zahlen, 0, false);
    //
    // W7 P2: DIE FELDER DEN FORMULAREN ZUORDNEN (21:15).
    //
    // GEMESSEN, warum HIER und nicht im Lauf: `Eingabe::formular` traegt
    // den Index, aber die Formular-Tabelle waechst im Lauf — ein Feld,
    // das VOR seinem `<form>` im Dokument steht (erlaubt, wenn auch
    // sinnlos), wuerde sonst auf eine falsche Zahl zeigen. Hier wird nach
    // dem Lauf ueber die Felder gelaufen, und jedes kommt bei SEINEM
    // Formular an.
    //
    // M3: hoechstens `MAX_EINGABEN` (256) Felder, jedes genau einmal.
    //
    // GEMESSEN, warum erst die Zuordnung und dann das Schreiben in zwei
    // Schritten (E0499): `w.eingaben` und `w.formulare` sind zwei Felder
    // DESSELBEN `w` — sie beide gleichzeitig zu leihen geht nicht. Also
    // erst eine flache Liste, dann der Schreibzugriff.
    let zuordnung: Vec<(usize, usize)> = w
        .eingaben
        .iter()
        .take(crate::dom::MAX_EINGABEN)
        .filter_map(|e| e.formular.map(|fi| (fi, e.ord)))
        .collect();
    for (fi, ord) in zuordnung {
        if let Some(f) = w.formulare.get_mut(fi) {
            f.felder.push(ord);
        }
    }
    //
    // W7C A2: DIE AUSWAHLEN AUCH (02:30).
    //
    // GEMESSEN, warum die Auswahlen in DIESELBE Feldliste gehen und nicht
    // in eine eigene: `Formular::felder` ist die Liste „was schickt dieses
    // Formular“. Eine zweite Liste wäre eine zweite Reihenfolge, und die
    // Reihenfolge ist bei `application/x-www-form-urlencoded` der
    // Unterschied zwischen zwei Anfragen (der Server darf sich auf die
    // Reihenfolge verlassen). Die Vorlauf-Nummer ist der Schluessel, und
    // die ist im ganzen Dokument eindeutig.
    let zuordnung: Vec<(usize, usize)> = w
        .auswahl
        .iter()
        .take(crate::dom::MAX_AUSWAHLEN)
        .filter_map(|a| a.formular.map(|fi| (fi, a.ord)))
        .collect();
    for (fi, ord) in zuordnung {
        if let Some(f) = w.formulare.get_mut(fi) {
            f.felder.push(ord);
        }
    }
}

/// Release 12.10.2026: die Frist je Skript fuer alle danach angelegten
/// Seiten-Engines (0 = Standard 15 s; `NARSIL_JS_FRIST_MS` gewinnt).
pub static JS_FRIST_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Setzt [`JS_FRIST_MS`] (Millisekunden).
pub fn js_frist_setzen(ms: u64) {
    JS_FRIST_MS.store(ms, std::sync::atomic::Ordering::Relaxed);
}

/// Release 15.10.2026: script requests (fetch, XMLHttpRequest) use the
/// browser's cookie jar like page loads: a same-origin request sends the
/// cookies of its URL and stores the `Set-Cookie` headers of its response
/// (Fetch §4.6, credentials mode "same-origin", the default of fetch and of
/// XMLHttpRequest without `withCredentials`). Cross-origin requests stay
/// without cookies. MEASURED before: neither direction happened — golem.de's
/// consent check (`/abo/testcookie.php` per XHR, then `document.cookie`)
/// reported cookies as disabled.
#[derive(Clone)]
struct KeksNetz {
    innen: narsil_jsvm::netz::NarsilNet,
    seite: String,
}

impl KeksNetz {
    fn herkunft(url: &str) -> Option<(String, String)> {
        let (schema, rest) = url.split_once("://")?;
        let ende = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let hp = rest[..ende].to_ascii_lowercase();
        let schema = schema.to_ascii_lowercase();
        let hp = match (schema.as_str(), hp.rsplit_once(':')) {
            ("https", Some((h, "443"))) | ("http", Some((h, "80"))) => h.to_string(),
            _ => hp,
        };
        Some((schema, hp))
    }
}

impl narsil_jsvm::netz::Netzbau for KeksNetz {
    fn hole(&mut self, anfrage: &narsil_jsvm::netz::NetzAnfrage) -> Result<narsil_jsvm::netz::NetzAntwort, String> {
        let gleich = matches!((Self::herkunft(&anfrage.url), Self::herkunft(&self.seite)), (Some(a), Some(b)) if a == b);
        if !gleich {
            return self.innen.hole(anfrage);
        }
        let mut a = anfrage.clone();
        a.kopf.retain(|(n, _)| !n.eq_ignore_ascii_case("cookie"));
        if let Some(k) = crate::keks::cookie_kopf_fuer(&a.url, &self.seite) {
            a.kopf.push(("Cookie".to_string(), k));
        }
        let antwort = self.innen.hole(&a)?;
        crate::keks::set_cookie_koepfe(
            antwort
                .kopf
                .iter()
                .filter(|(n, _)| n.eq_ignore_ascii_case("set-cookie"))
                .map(|(_, w)| w.as_str()),
            &a.url,
        );
        Ok(antwort)
    }

    /// Release 15.10.2026 (agent I, JSVM speed): the requests of one script
    /// round in parallel (`narsil_jsvm::netz::parallel_holen`). MEASURED
    /// before: chip.de fetched 45 API requests of one round one after the
    /// other, 9.1 s of its 20.5 s JS phase.
    fn hole_alle(&mut self, anfragen: &[narsil_jsvm::netz::NetzAnfrage]) -> Vec<Result<narsil_jsvm::netz::NetzAntwort, String>> {
        narsil_jsvm::netz::parallel_holen(self, anfragen)
    }
}
