//! W6B, J9 — DAS STANDARD-DOM: `document`, `Element`, `style`, `classList`, `Event`.
//!
//! # WAS HIER GEBAUT WIRD
//!
//! Echte Webseiten schreiben:
//!
//! ```js
//! const z = document.getElementById('z');
//! z.textContent = Number(z.textContent) + 1;
//! z.style.color = 'red';
//! z.classList.add('voll');
//! el.onclick = () => { … };
//! ```
//!
//! Das sind EIGENSCHAFTEN und METHODEN auf OBJEKTEN — keine globale
//! Funktion mit einem Element als erstem Argument. Die Maschine dafuer
//! steht in `narsil-jsvm` (`hostobjekt.rs`: Getter/Setter/Aufruf-Haken) —
//! B1-B3, 01.10.2026: hier stand bis heute `narsil-js`, und die Datei war
//! die mit den meisten Stellen (25).
//!
//! # WIE DIE ELEMENTE VORTEILEN
//!
//! Ein Element bekommt eine **Host-Id**, und die Id ist ein Schluessel im
//! `DomStand`. JavaScript sieht die Id nie; es sieht `{__host: id}`.
//! Der Getter beantwortet `textContent` aus dem Stand (damit ein Skript
//! den Wert sieht, den es selbst geschrieben hat), der Setter schreibt
//! einen BEFEHL in die Kiste, und der Hauptthread spielt die Befehle nach
//! dem Lauf in den Patch.
//!
//! # METHODEN ALS ZAHLEN
//!
//! `Wert` kennt keine Funktionen (`wert.rs`: Undefiniert, Null, Wahr,
//! Zahl, Text, Feld, Gegenstand). Der Getter gibt fuer eine Methode also
//! `{__host: id, __methode: <ZAHL>}` zurueck, und der Aufruf-Haken macht
//! aus der Zahl wieder einen Namen. GEMESSEN, warum `rufen` im Haken
//! und nicht als Funktion: eine neue `Wert`-Variante waere eine
//! Aenderung an ~30 Stellen in 18.883 Zeilen.

use crate::js::{Befehl, MAX_ATTRIBUT};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

/// Hoechste Zahl Host-Objekte einer Seite (M3).
///
/// GEMESSEN, warum 65.536: eine Seite, in der ein Skript 65.536 Elemente
/// anfasst, ist eine Anwendung, kein Dokument.
pub const MAX_HOST_IDS: u32 = 65_536;
/// Window and Document are separate event targets sharing one page state.
pub const WINDOW_HOST_ID: u32 = MAX_HOST_IDS + 1;

/// W7 P2: Hoechste Zahl Eingabefelder je Seite (M3).
///
/// GEMESSEN, warum 256: eine Suchseite hat 1–3 Felder, ein grosser
/// Warenkorb vielleicht 40. 256 ist eine Seite mit Formular und kein
/// Skript, das Felder erzeugt.
pub const MAX_EINGABEN: usize = 256;
/// W7 P2: Hoechste Zahl Formulare je Seite (M3).
pub const MAX_FORMULAR: usize = 32;
/// W7C A2: Hoechste Zahl AUSWAHLEN (`<select>`) je Seite (M3).
///
/// GEMESSEN, warum 64 und nicht 256: eine Auswahlliste ist ein
/// Bedienelement mit mindestens einer Zeile Hoehe. 256 davon waeren ein
/// Formular, das niemand ausfuellt — und der Grenzwert soll Seiten
/// stoppen, nicht Formulare.
pub const MAX_AUSWAHLEN: usize = 64;
/// W7 P2: Hoechste Laenge eines getippten Wertes in Zeichen (M3/M4).
pub const MAX_EINGABE_WERT: usize = 4096;
/// W7 P2: Hoechste Zahl Zeichen, die ein Klick als „getippt" meldet.
///
/// GEMESSEN, warum 128: ein Tastendruck ist ein Zeichen (Wust, Umlaut
/// sind zwei Bytes, aber ein Zeichen). Wer 10.000 Zeichen auf einmal
/// „tippt", ist kein Mensch — und die Liste waere eine Angriffsflaeche
/// auf den Speicher.
pub const MAX_TIPP: usize = 128;
/// W7 P2: Hoechste Zahl Bytes, die ein `POST`-Body haben darf (M4).
///
/// GEMESSEN, warum 64 KiB: `narsil_net::Request::body` ist ein `Vec<u8>`
/// ohne Grenze, und ein Formular koennte megabyteweise Felder schicken.
/// 64 KiB ist das, was ein Formular braucht (eine Datei ist ein
/// `multipart`, und das ist ein anderes Formular).
pub const MAX_POST_BODY: usize = 64 * 1024;

/// W7 P2: EIN EINGABEFELD (aus dem geparsten Baum).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eingabe {
    /// Die Vorlauf-Nummer des `<input>`.
    pub ord: usize,
    /// Die Host-Id (fuer `focus`).
    pub host: u32,
    /// Das `name`-Attribut (leer = nicht im Formular).
    pub name: String,
    /// Der `type` (klein, „text" wenn keiner).
    pub typ: String,
    /// Der Anfangswert aus dem HTML.
    pub wert: String,
    /// W7C A2: IST DAS FELD ANGEHAKBT? (checkbox/radio)
    ///
    /// GEMESSEN, warum das im DOM-Stand und nicht im `Patch`: ein Haken
    /// ist ein ZUSTAND des Formulars, und der Stand wird bei jedem Klick
    /// neu gefuellt. Staende er im Patch, waere er beim Neuaufbau weg —
    /// und genau dann, wenn der Nutzer ihn abhakt.
    pub checked: bool,
    /// W7C A2: `disabled` — ein behindertes Feld wird NICHT gesendet.
    pub disabled: bool,
    /// W7C A2: Ist das ein ABSENDER-Knopf (`submit`/`button`/`image`)?
    ///
    /// GEMESSEN, warum das eine eigene Zahl und kein `typ`-Vergleich an
    /// drei Stellen: die Entscheidung „wird gesendet“ kommt an vier
    /// Stellen vor (Felder sammeln, Wert holen, Knopf klicken, Body
    /// bauen). Ein Vergleich, der viermal `typ == "submit"` schreibt, ist
    /// viermal eine Gelegenheit, eine Schreibweise zu vergessen.
    pub absender: bool,
    /// W7C A2: `type="file"` — der Browser kann keine Datei waehlen.
    ///
    /// Behauptet als Zahl, damit der Fall BENENNT werden kann: ein
    /// stillschweigend leeres Dateifeld sieht aus wie ein Fehler des
    /// Servers, und der Nutzer sucht den Fehler am falschen Ende.
    pub datei: bool,
    /// Der Index in [`Stand::formulare`], in dem das Feld steht.
    pub formular: Option<usize>,
}

impl Eingabe {
    /// Bekommt man in dieses Feld tippen?
    ///
    /// GEMESSEN, warum `search` dazugehoert: der Auftrag nennt
    /// `<input type=text/search>` ausdruecklich. `password` auch — der
    /// Wert wird nicht angezeigt, aber getippt wird er.
    ///
    /// W7C A8: `textarea` ist DABEI. GEMESSEN, warum es fehlte: der
    /// Platzhalter `[Textfeld kommentar: …]` stand im Bild, und der Wert
    /// wurde auch GESENDET (A2) — aber ein Klick setzte keinen Fokus, weil
    /// `tippbar()` kein `textarea` kannte. Ein Kommentarfeld, das man
    /// nicht ausfuellen kann, ist das haeufigste Formularfeld, das
    /// fehlt.
    pub fn tippbar(&self) -> bool {
        matches!(
            self.typ.as_str(),
            "text" | "search" | "email" | "url" | "tel" | "password" | "textarea" | ""
        )
    }

    /// W7C A8: Hat dieses Feld mehr als eine Zeile?
    ///
    /// GEMESSEN, warum das eine eigene Zahl und kein Vergleich an drei
    /// Stellen: Enter soll in einem `textarea` eine ZEILEUMBRUCH
    /// setzen und nicht das Formular abschicken — das ist der
    /// Unterschied zwischen einem Kommentarfeld und einem Suchfeld, und
    /// er steht an drei Stellen (Taste, Cursor, Anzeige).
    pub fn mehrzeilig(&self) -> bool {
        self.typ == "textarea"
    }

    /// W7C A2: WIRD DIESES FELD MITGESENDET — und wenn, MIT WELCHEM WERT?
    ///
    /// GEMESSEN, warum die vier Faelle in EINER Funktion stehen: sie
    /// werden an drei Stellen gebraucht (Felder sammeln, Body bauen, GET
    /// bauen), und an jeder Stelle eine eigene Liste zu pflegen ist der
    /// Grund, warum ein `disabled`-Feld in einem Formular mitkam und in
    /// einem anderen nicht.
    ///
    /// | Fall | gesendet? | Wert |
    /// |---|---|---|
    /// | `disabled` | nein | — |
    /// | `name` leer | nein | — |
    /// | checkbox/radio ohne Haken | nein | — |
    /// | checkbox/radio mit Haken | ja | `wert`, sonst `on` |
    /// | sonst | ja | der Wert |
    ///
    /// `haken` und `wert` sind die ENTSCHEIDUNGEN DES NUTZERS (der Klick
    /// auf den Kasten, das Getippte). Sie kommen von aussen, weil sie im
    /// `Patch` stehen — dort, wo auch der getippte Wert steht, und aus
    /// demselben Grund: `Patch` ueberlebt den Neuaufbau der Seite,
    /// `Eingabe` (die das HTML abbildet) nicht.
    ///
    /// `None` heisst: der Nutzer hat nichts gesagt, dann gilt das HTML.
    pub fn senden(&self, haken: Option<bool>, wert: Option<&str>) -> Option<(String, String)> {
        if self.disabled || self.name.is_empty() {
            return None;
        }
        let gewaehlt = haken.unwrap_or(self.checked);
        if matches!(self.typ.as_str(), "checkbox" | "radio") && !gewaehlt {
            return None;
        }
        // GEMESSEN, warum der Haken-Wert `on` ist, wenn kein `value`
        // dasteht: das ist die HTML-Vorgabe, und jedes Formular der Welt
        // rechnet damit.
        let mut raus = wert
            .map(std::string::ToString::to_string)
            .unwrap_or_else(|| self.wert.clone());
        if raus.is_empty() && matches!(self.typ.as_str(), "checkbox" | "radio") {
            raus = "on".to_string();
        }
        Some((self.name.clone(), raus))
    }

    /// W7C A2: Ist das ein KASTEN, den man an- und abhaken kann?
    pub fn ist_haken(&self) -> bool {
        matches!(self.typ.as_str(), "checkbox" | "radio")
    }
}

/// W7C A2: EINE OPTION IN EINER AUSWAHL (`<select>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuswahlOption {
    /// Der `value` (leer = der Text der Option, wie im Browser).
    pub wert: String,
    /// Der sichtbare Text der Option.
    pub text: String,
    /// `selected` im HTML.
    pub gewaehlt: bool,
    /// `disabled` — eine behinderte Option kann nicht gewaehlt werden.
    pub disabled: bool,
}

/// W7C A2: EINE AUSWAHL (`<select>`).
///
/// GEMESSEN, warum ein eigener Typ und kein `Eingabe` mit `typ="select"`:
/// eine Auswahl hat eine LISTE von Werten, und `Eingabe::wert` ist ein
/// String. Haette man sie als `Eingabe` gebaut, muesste der Wert der
/// gewaehlten Option in `wert` stehen UND die Liste irgendwo liegen — zwei
/// Orte fuer eine Sache. So ist die Auswahl der Ort, und `Stand::auswahl`
/// haelt die Auswahl.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Auswahl {
    /// Die Vorlauf-Nummer des `<select>`.
    pub ord: usize,
    /// Die Host-Id.
    pub host: u32,
    /// Das `name`-Attribut.
    pub name: String,
    /// Die Optionen in Dokumentreihenfolge.
    pub optionen: Vec<AuswahlOption>,
    /// `disabled`.
    pub disabled: bool,
    /// `multiple` — bei mehrfachem `select` werden alle angehakten
    /// Optionen gesendet.
    pub mehrfach: bool,
    /// Der Index in [`Stand::formulare`].
    pub formular: Option<usize>,
}

impl Auswahl {
    /// W7C A2: WELCHE OPTION IST GEWAEHLT? (die erste, wenn keine
    /// `selected` hat).
    ///
    /// GEMESSEN, warum die erste und nicht `None`: ein Browser zeigt eine
    /// Auswahlliste nie „leer“. Wenn kein `selected` dasteht, ist die
    /// erste Option gewaehlt — und das ist der Grund, warum sich
    /// `<select name=land>` ohne `selected` beim Absenden wie „Deutschland“
    /// verhaelt.
    pub fn gewaehlte_indizes(&self) -> Vec<usize> {
        if self.mehrfach {
            return self
                .optionen
                .iter()
                .enumerate()
                .filter(|(_, o)| o.gewaehlt && !o.disabled)
                .map(|(i, _)| i)
                .collect();
        }
        if let Some(i) = self.optionen.iter().position(|o| o.gewaehlt && !o.disabled) {
            return vec![i];
        }
        match self.optionen.iter().position(|o| !o.disabled) {
            Some(i) => vec![i],
            None => Vec::new(),
        }
    }
}

/// W7 P2: EIN FORMULAR (aus dem geparsten Baum).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Formular {
    /// Die Vorlauf-Nummer des `<form>`.
    pub ord: usize,
    /// Das `action`-Attribut (leer = die Seite selbst).
    pub aktion: String,
    /// Das `method`-Attribut, GROSS (Vorgabe `GET`).
    pub methode: String,
    /// Die Vorlauf-Nummern der Felder, die DIREKT in diesem Formular
    /// stehen.
    ///
    /// GEMESSEN, warum `Vec` und kein Zaehler: die Felder muessen
    /// sortiert werden (Dokumentreihenfolge), und `HashMap` haelt keine
    /// Reihenfolge. Die Zahl ist durch `MAX_EINGABEN` begrenzt.
    pub felder: Vec<usize>,
    /// Release 10.10.2026: `enctype`, klein (`multipart/form-data` schickt
    /// die gewaehlten Dateien mit, HTML §4.10.21.7).
    pub enctype: String,
}

/// Release 10.10.2026: ein Dateifeld im Versand — `start`/`anzahl` sind die
/// Platzhalter (Dateinamen) in der Feldliste, die ein mehrteiliger Versand
/// durch die Dateien selbst ersetzt.
#[derive(Debug, Clone)]
pub struct DateiTeil {
    pub start: usize,
    pub anzahl: usize,
    pub name: String,
    pub dateien: Vec<GewaehlteDatei>,
}

/// Release 10.10.2026: hoechstens so gross wird ein mehrteiliger Versand (M4).
pub const MAX_MULTIPART_BODY: usize = 256 * 1024 * 1024;

impl Formular {
    /// Ist das ein `POST`? (Alles andere ist GET, wie im Browser.)
    pub fn ist_post(&self) -> bool {
        self.methode == "POST"
    }
}

/// Hoechste Zahl Getter-/Setter-/Aufruf-Vorgaenge je Skriptlauf (M3).
///
/// GEMESSEN, warum 200.000: ein Skript, das eine Eigenschaft 200.000 Mal
/// liest, liest sie entweder in einer Schleife (ein Fehler im Skript) oder
/// es hat einen Endlosschleifen-Bug. Beides soll eine benannte Meldung
/// geben und nicht still weiterlaufen.
///
/// Release 12.10.2026: 50 Mio. wie die Schrittgrenze der Engine
/// (`narsil_jsvm::grenzen`, dazu deren Wanduhr) — eine Endlosschleife
/// faengt die Engine. GEMESSEN vorher (WPT img.complete „IDL attribute
/// complete cannot randomly change"): eine zweisekuendige Leseschleife
/// (`img.complete`, ~1,3 Mio. Zugriffe) brach nach 200.000 ab, danach
/// fehlte dem Skript sogar `self` — der ganze Test lief in die Zeit.
pub const MAX_VORGAENGE: u32 = 50_000_000;

/// Das Feld, an dem die Engine ein Host-Objekt erkennt.
const MERKE: &str = narsil_jsvm::HOST_MERKE;

/// Das Feld, in dem der Getter eine METHODEN-NUMMER ablegt.
const METHODE: &str = narsil_jsvm::METHODE;

/// Die Vorgaenge (M3).
static VORGAENGE: AtomicU32 = AtomicU32::new(0);

/// Setzt den Zaehler zurueck (je Skriptlauf).
pub fn vorgaenge_zuruecksetzen() {
    VORGAENGE.store(0, Ordering::SeqCst);
}

/// Die Methoden-Nummern.
///
/// GEMESSEN, warum eine Zahl und kein Name: `Wert` hat keine
/// Funktions-Variante, also muss der Name durch `Wert::Zahl`. Die
/// Tabelle hier macht aus der Zahl wieder den Namen — und sie ist
/// `const`, also gibt es keinen zweiten Ort, an dem eine Methode entsteht.
pub fn methode_nummer(name: &str) -> Option<f64> {
    Some(match name {
        "add" => 1.0,
        "remove" => 2.0,
        "toggle" => 3.0,
        "contains" => 4.0,
        "setProperty" => 5.0,
        "removeProperty" => 6.0,
        "getAttribute" => 7.0,
        "setAttribute" => 8.0,
        "removeAttribute" => 9.0,
        "querySelector" => 10.0,
        "querySelectorAll" => 11.0,
        "appendChild" => 12.0,
        "addEventListener" => 13.0,
        "removeEventListener" => 14.0,
        "click" => 15.0,
        // Release 11.10.2026: `focus`/`blur` (16/17) kommen aus dem Prototyp
        // (`vorspann.js`, HTML §6.6) — die Host-Fassung tat nichts und
        // verdeckte ihn.
        "getElementsByTagName" => 18.0,
        "getElementById" => 19.0,
        "querySelector" => 20.0,
        "querySelectorAll" => 21.0,
        // W6F: `document.createElement('li')`.
        "createElement" => 22.0,
        "createElementNS" => 23.0,
        // Release 15.10.2026: `getContext`, `toDataURL` and the 2D context
        // methods (24, 26-44) moved to `canvas2d.js` (prototype methods on
        // HTMLCanvasElement / CanvasRenderingContext2D); the numbers stay
        // reserved.
        // 07.10.2026: `document.write(html)` — schreibt HTML an die aktuelle
        // Position. NAEHERUNG: der HTML-String wird geparst und die
        // resultierenden Knoten werden an `document.body` angehaengt. Ein
        // echter Browser fuegt an der Parser-Position ein (wo das <script>
        // steht); hier laeuft das Skript NACH dem Parsen, also ist der Body
        // das naheliegendste Ziel. Wer `document.write` vor dem Schliessen
        // des Body ruft, bekommt die Knoten am Ende des Body.
        // Release 11.10.2026: `write` macht jetzt `vorspann.js` (HTML §8.4,
        // an der Einfuegestelle, open/close); der Host meldet es nicht mehr.
        // Release 08.10.2026: Einfuegen an einer Lage (DOM §4.2.6, HTML
        // §8.5 `insertAdjacentHTML`). GEMESSEN: das Skript von example.com
        // brach an `insertAdjacentHTML` ab („Methode … gibt es nicht").
        "insertAdjacentHTML" => 45.0,
        "insertAdjacentText" => 46.0,
        "insertAdjacentElement" => 47.0,
        "prepend" => 48.0,
        "append" => 49.0,
        "before" => 50.0,
        "after" => 51.0,
        _ => return crate::lebend::METHODEN.iter().find(|(n, _)| *n == name).map(|(_, z)| *z),
    })
}

/// Der Name zu einer Methoden-Nummer.
pub fn methode_name(nummer: f64) -> Option<&'static str> {
    Some(match nummer as u32 {
        1 => "add",
        2 => "remove",
        3 => "toggle",
        4 => "contains",
        5 => "setProperty",
        6 => "removeProperty",
        7 => "getAttribute",
        8 => "setAttribute",
        9 => "removeAttribute",
        10 => "querySelector",
        11 => "querySelectorAll",
        12 => "appendChild",
        13 => "addEventListener",
        14 => "removeEventListener",
        15 => "click",
        16 => "focus",
        17 => "blur",
        18 => "getElementsByTagName",
        19 => "getElementById",
        20 => "querySelector",
        21 => "querySelectorAll",
        // W6F
        22 => "createElement",
        23 => "createElementNS",
        // 07.10.2026: `document.write(html)`.
        25 => "write",
        45 => "insertAdjacentHTML",
        46 => "insertAdjacentText",
        47 => "insertAdjacentElement",
        48 => "prepend",
        49 => "append",
        50 => "before",
        51 => "after",
        _ => return crate::lebend::METHODEN.iter().find(|(_, z)| (*z - nummer).abs() < 0.5).map(|(n, _)| *n),
    })
}

/// Release 08.10.2026: wohin ein neuer Knoten kommt — Host-Ids, im Patch
/// zu Vorlauf-Nummern (`narsil_page::NeuLage`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostLage {
    Anfang,
    Vor(u32),
    Nach(u32),
}

/// Was JavaScript sieht und was der Browser ausfuehrt.
/// Release 10.10.2026: eine gewaehlte Datei eines `<input type="file">`.
#[derive(Debug, Clone, PartialEq)]
pub struct GewaehlteDatei {
    pub pfad: std::path::PathBuf,
    /// Nur der Dateiname (die Seite sieht nie den Pfad — HTML §4.10.5.1.18).
    pub name: String,
    pub groesse: u64,
    /// Der MIME-Typ aus der Endung ("" wenn unbekannt, wie in Chrome).
    pub typ: String,
    /// Aenderungszeit in ms seit 1970 (`File.lastModified`).
    pub geaendert_ms: f64,
}

/// Release 10.10.2026: hoechstens so viele Dateien je Feld (M3).
pub const MAX_DATEIEN_JE_FELD: usize = 256;
/// Release 10.10.2026: hoechstens so viele Bytes liest `input.files` je Datei
/// in den Speicher der Seite (M4).
pub const MAX_DATEI_BYTES: u64 = 256 * 1024 * 1024;

impl GewaehlteDatei {
    pub fn aus_pfad(pfad: &std::path::Path) -> Option<Self> {
        let meta = std::fs::metadata(pfad).ok()?;
        if !meta.is_file() {
            return None;
        }
        let name = pfad.file_name()?.to_string_lossy().to_string();
        let endung = pfad.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let typ = match endung.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "avif" => "image/avif",
            "bmp" => "image/bmp",
            "svg" => "image/svg+xml",
            "ico" => "image/x-icon",
            "pdf" => "application/pdf",
            "txt" => "text/plain",
            "csv" => "text/csv",
            "html" | "htm" => "text/html",
            "css" => "text/css",
            "js" | "mjs" => "text/javascript",
            "json" => "application/json",
            "xml" => "text/xml",
            "zip" => "application/zip",
            "gz" => "application/gzip",
            "mp3" => "audio/mpeg",
            "wav" => "audio/wav",
            "ogg" | "oga" => "audio/ogg",
            "flac" => "audio/flac",
            "m4a" => "audio/mp4",
            "opus" => "audio/opus",
            "mp4" | "m4v" => "video/mp4",
            "webm" => "video/webm",
            "mov" => "video/quicktime",
            "mkv" => "video/x-matroska",
            "doc" => "application/msword",
            "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            _ => "",
        }
        .to_string();
        let geaendert_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0.0, |d| d.as_millis() as f64);
        Some(Self { pfad: pfad.to_path_buf(), name, groesse: meta.len(), typ, geaendert_ms })
    }
}

/// Release 11.10.2026: die Masse eines Elements aus dem letzten Layout —
/// `[x, y, breite, hoehe, rahmen links, oben, rechts, unten, positionsart,
/// rechte Kante des Teilbaums, untere Kante des Teilbaums]` in
/// Seitenkoordinaten (positionsart: 0 static, 1 relative, 2 absolute,
/// 3 fixed, 4 sticky).
pub type Masse = [f32; 11];

#[derive(Debug, Default)]
pub struct Stand {
    /// Release 10.10.2026: die gewaehlten Dateien je `<input type="file">`
    /// (Host-Id) — `input.files`, Formularversand.
    pub dateien: HashMap<u32, Vec<GewaehlteDatei>>,
    /// Release 10.10.2026: zaehlt jede neue Auswahl (die Seite haelt ihre
    /// `File`-Objekte, bis sich die Auswahl aendert).
    pub dateien_stand: u64,
    /// Release 10.10.2026: `input.click()`/`showPicker()` aus der Seite —
    /// die Sitzung oeffnet den Dialog (nur kurz nach einer Nutzeraktion).
    pub datei_dialog_wunsch: Option<u32>,
    /// Release 09.10.2026: die Kaesten des letzten Layouts je Element
    /// (Host-Id -> `[x, y, breite, hoehe]` in Seitenkoordinaten) — fuer
    /// `getBoundingClientRect`, `offsetWidth` und `ResizeObserver`.
    pub masse: HashMap<u32, Masse>,
    /// Release 09.10.2026: der Rollstand der Seite beim letzten Layout.
    pub rollen_y: f32,
    /// Release 12.10.2026: die Textlaeufe des letzten Layouts je Element-Host
    /// (`Range.getClientRects`, `OpaqueRange`). Erst gefuellt, wenn ein
    /// Skript danach fragt (`textlaeufe_an`), dann bei jedem Layout.
    pub textlaeufe: HashMap<u32, Vec<crate::bereich::TextLauf>>,
    pub textlaeufe_an: bool,
    /// Release 11.10.2026: ein Bildlauf, den ein Skript verlangt
    /// (`scrollTo`, `scrollIntoView`) — der Takt fuehrt ihn aus.
    pub rollen_wunsch: Option<f32>,
    /// Release 11.10.2026: die Dokumente der Iframes (Iframe-Host -> Dokument-
    /// Host), angemeldet vom Vorspann (`__narsilRahmenAnmelden`) — das
    /// Zwangslayout rechnet sie mit der Innengroesse des Iframes.
    pub rahmen: Vec<(u32, u32)>,
    /// Die Host-Ids, deren Masse aus einem Iframe-Layout stammen (ohne den
    /// Bildlauf der Hauptseite).
    pub rahmen_hosts: std::collections::HashSet<u32>,
    /// Vorlauf-Nummer -> Host-Id.
    pub ids: HashMap<usize, u32>,
    /// Host-Id -> Vorlauf-Nummer.
    pub ord: HashMap<u32, usize>,
    /// Host-Id -> Tagname (klein).
    pub tag: HashMap<u32, String>,
    /// Vorlauf-Nummer -> `id`-Attribut.
    pub dom_ids: HashMap<usize, String>,
    /// Host-Id -> Vorlauf-Nummer -> Attributwert.
    ///
    /// GEMESSEN, warum pro Element: `getAttribute("class")` und
    /// `getAttribute("id")` koennen verschiedene Werte haben, und ein
    /// flacher Schluessel (`(id, name) -> wert`) waere ein Tupelschluessel
    /// in jedem Zugriff.
    pub attribute: HashMap<u32, HashMap<String, String>>,
    /// Host-Id -> der Text, den JavaScript zuletzt gelesen oder
    /// geschrieben hat.
    ///
    /// GEMESSEN, warum HIER und nicht im Patch: ein Skript darf lesen,
    /// was es selbst geschrieben hat, ohne dass die Seite neu gebaut
    /// wurde. Der Patch kostet 2 ms — das ist fuer eine Schleife von
    /// 1000 Zugriffen zu lang.
    pub text: HashMap<u32, String>,
    /// Host-Id -> gesetzte Inline-Stile (`color` -> `red`).
    pub stil: HashMap<u32, Vec<(String, String)>>,
    /// Host-Id -> Klassen.
    pub klassen: HashMap<u32, Vec<String>>,
    ///
    ///
    /// W7 P2: die FORMULARE der Seite, in Dokumentreihenfolge.
    ///
    /// GEMESSEN, warum sie im DOM-Stand und nicht in `narsil-page`: der
    /// Stand entsteht aus dem geparsten Baum in EINEM Durchgang, der
    /// Vorlauf-Nummer, Host-Id und Attribute kennt (`fuelle_stand`). Eine
    /// zweite Tabelle in `narsil-page` wuerde dieselben Nummern noch einmal
    /// vergeben — die Fehlerklasse „drei Zahlenraeume" aus W5G.
    ///
    /// M3: `MAX_FORMULAR` (32).
    pub formulare: Vec<Formular>,
    /// W7 P2: die EINGABEFELDER, in Dokumentreihenfolge.
    ///
    /// M3: `MAX_EINGABEN` (256). Der Wert ist auf `MAX_EINGABE_WERT`
    /// (4096 Zeichen) gekuerzt — das ist der Wert, den der Nutzer tippt,
    /// und ein Browser haelt ihn nicht unbegrenzt.
    pub eingaben: Vec<Eingabe>,
    /// W7 P2: Index des `<form>`, in dem der gerade besuchte Knoten
    /// steht (wird beim Betreten eines `<form>` gesetzt).
    pub letztes_formular: Option<usize>,
    /// W7 P2: Vorlauf-Nummer -> der WERT, der getippt wurde.
    ///
    /// GEMESSEN, warum nicht in `Eingabe::wert`: `Eingabe` ist die
    /// BESCHREIBUNG des Feldes (aus dem HTML), und der getippte Wert
    /// aendert sich. Beides in einem Feld hiesse: nach jedem Tastendruck
    /// ist die HTML-Beschreibung falsch.
    ///
    /// M3/M4: `MAX_EINGABEN` (256) Eintraege, jeder Wert
    /// `MAX_EINGABE_WERT` (4096 Zeichen).
    pub eingabe_werte: HashMap<usize, String>,
    /// Release 11.10.2026: der „schmutzige" Wert eines Formularfelds je Host
    /// (HTML §4.10.5.4), auch fuer Felder ohne Vorlauf-Nummer (per Skript
    /// erzeugt). Hat ein Feld eine Nummer, wird zusaetzlich
    /// `eingabe_werte` gespiegelt (Anzeige und Nutzereingabe).
    pub feld_werte: HashMap<u32, String>,
    /// W7C A2: die AUSWAHLEN (`<select>`), in Dokumentreihenfolge.
    ///
    /// M3: `MAX_AUSWAHLEN` (64) — eine Seite mit 100 Auswahllisten ist
    /// ein Formular, kein Artikel.
    ///
    /// GEMESSEN, warum die Wahl des Nutzers hier NICHT steht, sondern im
    /// `Patch`: sie ist eine ENTSCHEIDUNG, und `Patch` ist der Ort fuer
    /// Entscheidungen ueber die Seite (der getippte Wert eines Feldes
    /// steht dort, W7 P2). Hier steht, was die HTML-Datei sagt; die
    /// Wahl des Nutzers steht eine Ebene hoeher. Ein `HashMap` hier
    /// waere ein zweiter Zustand, der beim Neuaufbau (17:15) zurueckgesetzt
    /// wird — und der Haken waere beim naechsten Klick wieder weg.
    pub auswahl: Vec<Auswahl>,
    /// W6F: die NEUEN Knoten aus `document.createElement` — Host-Id und
    /// Tagname, in der Reihenfolge des Anlegens.
    ///
    /// GEMESSEN, warum sie im Stand und nicht im Patch: der Stand ist die
    /// EINZIGE Stelle, die einen Klick lang lebt. Der Patch entsteht bei
    /// jedem Klick neu aus den Befehlen; ein Knoten, der nur im Patch
    /// stuende, waere beim naechsten Klick weg. So bleibt die Liste, und
    /// jeder Neubau setzt dieselben Nummern (`NeuKnoten::ORD_BASIS` + i).
    ///
    /// M3: `NeuKnoten::MAX_NEU` (256).
    pub neu: Vec<(u32, String)>,
    pub namespaces: std::collections::HashMap<u32, (Option<String>, Option<String>, String)>,
    /// W6F: Kind-Host-Id -> Eltern-Host-Id (`appendChild`).
    pub eltern: std::collections::HashMap<u32, u32>,
    /// Release 08.10.2026: Einfügelage eines neuen Knotens (Host-Ids);
    /// fehlt der Eintrag, gilt „am Ende" (`appendChild`).
    pub lage: std::collections::HashMap<u32, HostLage>,
    /// W6D: `true`, wenn ein Handler `ev.preventDefault()` gerufen hat.
    ///
    /// GEMESSEN, warum das ein Flag und kein Zaehler ist (14:55): es geht
    /// um „der Aufrufer will es nicht" — einmal ja heisst ja. Und es muss
    /// VOR dem Aufruf geloescht werden: sonst verhindert ein früher
    /// `preventDefault` jeden spaeteren Klick (GEMESSEN: der Test
    /// `prevent_default_verhindert_die_navigation` blieb beim zweiten Klick
    /// stehen).
    pub default_verhindert: bool,
    pub propagation_stopped: bool,
    pub immediate_stopped: bool,
    pub passive_listener: bool,
    /// `None` = ausserhalb eines Dispatch (ein Ereignis ist cancelable,
    /// solange niemand das Gegenteil sagt); `Some(cancelable)` setzt der
    /// Verteiler (`ereignis_host::dispatch`).
    ///
    /// 06.10.2026: GEMESSEN am Test
    /// `prevent_default_ist_eine_funktion_und_setzt_das_flag` — mit einem
    /// harten `false` als Vorgabe setzte ein direkter
    /// `__preventDefault()`-Aufruf das Flag nicht. `Option<bool>` traegt
    /// den Unterschied „kein Verteiler" (Default) und „nicht cancelable".
    pub event_cancelable: Option<bool>,
    /// (Host-Id, Typ) -> Handler ([`crate::handler::Handler`], W6D:
    /// Funktionswert oder Quelltext).
    ///
    /// W6D: das Feld ist von `String` auf `Handler` umgestellt, weil
    /// `el.onclick = function () {…}` sonst wieder nur den Namen bekaeme
    /// — und die Closure waere verloren (GEMESSEN 13:40).
    pub horcher: HashMap<(u32, String), crate::handler::Handler>,
    /// `addEventListener` registrations retain order and callback identity.
    pub event_listeners: crate::handler::ListenerRegistry,
    /// Tagname -> Host-Id (`body`, `head`).
    pub standard: HashMap<String, u32>,
    ///
    /// Host-Id -> welche ART von Unterobjekt zuletzt gelesen wurde:
    /// 1 = `style`, 2 = `classList`, 3 = `dataset`, 0 = das Element.
    ///
    /// GEMESSEN, warum das noetig ist (16:15): `el.style.color = "rot"`
    /// und `el.farbe = "rot"` kommen als (`Host-Id`, `Name`) hier an —
    /// der Unterschied ist im Aufruf nicht mehr zu sehen, weil `style`
    /// ein Objekt MIT DERSELBEN Id ist. Ohne diese Tabelle waere
    /// `el.style.color` genau der Fehler „Eigenschaft 'color' gibt es
    /// nicht".
    ///
    /// DIE GENAUIGKEIT, und sie ist eine Naeherung: der Wert gilt fuer den
    /// LETZTEN Lesezugriff. `el.style; el.farbe = "x"` wuerde als
    /// Inline-Stil landen. GEMESSEN, warum das vertretbar ist: diese
    /// Schreibweise kommt in echtem Code nicht vor.
    pub art: HashMap<u32, u8>,
    ///
    /// Der CSS-Baum und die Bruecke CSS-Index -> Vorlauf-Nummer
    /// (W6C: `querySelectorAll`).
    ///
    /// GEMESSEN, warum sie HIER und nicht in der Kiste (13:20):
    /// `document.querySelectorAll` geht ueber den Host-Haken, und der
    /// Haken sieht die Kiste nicht. Ohne diese beiden Felder gab
    /// `querySelectorAll("li")` immer eine LEERE Liste — der Test fand 0.
    pub css_baum: narsil_page::CssBaum,
    /// CSS-Index -> Vorlauf-Nummer (aus `Seite::css_zu_dom`).
    pub css_zu_ord: HashMap<u32, usize>,
    /// Die Befehle, die nach dem Lauf in den Patch gespielt werden.
    pub befehle: Vec<Befehl>,
    /// `document.body` und Freunde (Host-Id 0).
    pub document_id: u32,
    /// Empty (the derived default) is exposed as `loading`.
    pub ready_state: &'static str,
    pub lifecycle_complete: bool,
    /// Release 11.10.2026: die aufgeloesten Rastervorlagen des letzten
    /// Layouts je Host (Spalten, Zeilen) — `getComputedStyle` liefert sie fuer
    /// `grid-template-columns/-rows` (CSS Grid 1 §7.2.6).
    pub raster_vorlagen: HashMap<u32, (String, String)>,
    /// Die Adresse (`location.href`).
    pub adresse: String,
    /// EXPANDO (05.10.2026): beliebige Eigenschaften, die ein Skript an ein
    /// Element oder an `window` schreibt (`window.eventLoggingLite = …`).
    ///
    /// GEMESSENER GRUND: vorher warf jede nicht erkannte Eigenschaft einen
    /// `Err` („Eigenschaft 'eventLoggingLite' gibt es nicht") und beendete
    /// das ganze Skript — GEMESSEN an www.wikipedia.org. Firefox legt solche
    /// Eigenschaften an und liest sie zurueck. Der TEXT genuegt, um das
    /// Skript am Leben zu halten; die Laengengrenze ist `MAX_ATTRIBUT`.
    pub expando: std::collections::HashMap<(u32, String), String>,
    /// Meldungen der Host-Seite (`alert(...)`) in Dokumentreihenfolge.
    ///
    /// 06.10.2026: GEMESSEN am Test `js_sripte::alert_erscheint_als_eigene_meldung`
    /// — die Seiten-Engine band `alert` nicht, das Skript brach mit
    /// `ReferenceError` ab und die Meldung fehlte. Der Puffer gehoert dem
    /// Host (der Motor sieht keine Statuszeile); die Ladepfade leeren ihn
    /// in die Konsole.
    pub meldungen: Vec<String>,
    /// Canvas surfaces and their 2D contexts (see `crate::canvas`).
    pub canvas: crate::canvas::Canvases,
    /// Decoded `<img>` pixels by source, for `drawImage` (weak: the image
    /// cache owns them).
    pub bild_pixel: HashMap<String, crate::canvas::ImagePixels>,
    /// Release 09.10.2026: das lebende DOM (siehe `crate::lebend`).
    pub lebend: Option<crate::lebend::Lebend>,
    /// Release 10.10.2026: je `<img>`-Host die Quelle, deren `load`/`error`
    /// gemeldet ist, und ihre natuerliche Groesse (0 x 0 = Fehler) — fuer
    /// `img.complete`/`naturalWidth`/`naturalHeight` (siehe
    /// `Sitzung::bild_ereignisse`).
    pub bild_gemeldet: HashMap<u32, (String, u32, u32)>,
    /// Release 2026-10-15: `<img>` hosts whose image data a relevant
    /// mutation updated while the source stayed the reported one: their
    /// `load`/`error` fires once more (HTML §4.8.4.3.3).
    pub bild_erneut: std::collections::HashSet<u32>,
    /// Release 10.10.2026: die FERTIGEN Bildquellen der Seite (Adresse ->
    /// natuerliche Groesse, 0 x 0 = Fehler). `img.complete` gilt auch fuer
    /// ein Bild, das fertig war, bevor ein Skript den Knoten kannte —
    /// GEMESSEN vorher (bbc.com): `complete` blieb nach der Hydration
    /// `false`, Next.js `<Image>` nahm den grauen Platzhalter nie weg.
    pub bilder_fertig: HashMap<String, (u32, u32)>,
    /// Release 15.10.2026: for finished VECTOR images the natural size the
    /// layout uses (`GeladenesBild::layout_masse`, `None` = none at all);
    /// `bilder_fertig` keeps the decoded size for `naturalWidth` and canvas.
    pub bilder_vektor: HashMap<String, Option<(f32, f32)>>,
    /// Release 10.10.2026: Bilder OHNE Platz im Dokument, die auf ihre Daten
    /// warten — (Host, absolute Quelle). HTML §4.8.4.3 laedt ein `<img>`
    /// auch, wenn es nicht eingehaengt ist (`new Image(); img.src = …`).
    pub lose_bilder: Vec<(u32, String)>,
    /// Release 10.10.2026: der Blattsatz der Seite (fuer `getComputedStyle`,
    /// siehe `__narsilStil`) und die berechneten Werte je Arena-Knoten — der
    /// Cache wird bei JEDER Schreib- und Methodenaenderung geleert.
    pub stil_satz: Option<narsil_page::StilSatz>,
    pub stil_cache: HashMap<usize, narsil_css::Berechnet>,
    /// Release 10.10.2026: hat sich das DOM seit dem letzten Layout
    /// geaendert? Dann rechnet eine Geometrie-Abfrage das Layout neu
    /// (`zwangslayout::erzwingen`), im Zeitbudget `zwang_fenster`.
    pub layout_veraltet: bool,
    pub zwang_fenster: Option<(std::time::Instant, std::time::Duration)>,
    /// Release 12.10.2026: ein `<style>`/`<link>` (oder sein Text) hat sich
    /// geaendert — der Blattsatz (`stil_satz`) ist veraltet und wird vor dem
    /// naechsten `getComputedStyle` neu gelesen (`blaetter_neu`).
    pub stil_veraltet: bool,
    /// Release 12.10.2026: nach einer DOM-Aenderung die `<style>`/`<link>`-
    /// Signatur pruefen (ein eingefuegtes oder entferntes Fragment mit einem
    /// `<style>` darin; WPT css-cascade scope-evaluation: `main.append(…)`,
    /// `main.replaceChildren()`).
    pub stil_pruefen: bool,
    pub stil_signatur: Option<Vec<(usize, u64)>>,
    /// Release 11.10.2026: die Kodierung des Dokuments (aus seinem `<meta>`;
    /// `None` = UTF-8) — `document.characterSet` und die Abfrage aufgeloester
    /// Adressen (HTML „parse a URL" mit der Dokumentkodierung).
    pub kodierung: Option<narsil_fmt_html::kodierung::Kodierung>,
    /// Release 13.10.2026: es gibt einen `slotchange`-Lauscher (Reaktion 11
    /// ging schon an den Vorspann).
    pub slot_lauscher: bool,
    /// Release 10.10.2026: das `<script>`, das gerade laeuft
    /// (`document.currentScript`, HTML §8.4.2) — nur waehrend eines
    /// klassischen Skripts, sonst `None` (Module, Timer, Handler).
    pub aktuelles_skript: Option<u32>,
    /// Release 15.10.2026: counts the changes that can change computed
    /// styles (DOM writes and methods, style sheets) — not the animated
    /// values. The animation module rescans the CSS animations and
    /// transitions only when it moved (`anim_hosts`).
    pub style_generation: u64,
    /// Release 15.10.2026: the computed styles from before the last change
    /// (what `stil_cache` held) — the before-change style of a CSS
    /// transition on an element that had none yet (`anim_hosts`).
    pub style_prev: HashMap<usize, narsil_css::Berechnet>,
    /// Release 15.10.2026: counts the style sheet changes (a part of
    /// `style_generation`); the animation module then scans all elements,
    /// otherwise only the changed ones.
    pub sheet_generation: u64,
    /// The `sheet_generation` of the last full animation scan (+1; 0 = none).
    pub anim_full_scan: u64,
}

impl Stand {
    pub fn nimm_befehle(&mut self) -> Vec<Befehl> {
        // Release 15.10.2026: a canvas without a number in the page (drawn
        // before it was appended) stays dirty and is shown once it has one.
        let ord = &self.ord;
        for (host, width, height, pixels) in self.canvas.take_dirty(|h| ord.contains_key(&h)) {
            if let Some(ord) = self.ord.get(&host) {
                self.befehle.push(Befehl::Canvas { knoten: *ord as u32, breite: width, hoehe: height, pixels });
            }
        }
        std::mem::take(&mut self.befehle)
    }
}

/// Release 12.10.2026: Inhaltsbreite und -hoehe des naechsten Vorfahren mit
/// `container-type: size` bzw. `inline-size` (dann nur die Breite).
fn behaelter_masse(w: &mut Stand, id: u32) -> (Option<f64>, Option<f64>) {
    let (Some(l), Some(satz)) = (w.lebend.as_ref(), w.stil_satz.as_ref()) else { return (None, None) };
    for h in l.vorfahren_hosts(id) {
        let Some(e) = l.knoten(h).filter(|e| l.dok.ist_element_knoten(*e)) else { continue };
        let Some(be) = crate::lebend::berechnet(l, e, satz, &mut w.stil_cache) else { continue };
        let art = narsil_css::berechnet_css::berechneter_wert(&be, "container-type").unwrap_or_default();
        let art = art.split_whitespace().find(|t| *t == "size" || *t == "inline-size").unwrap_or("").to_string();
        if art.is_empty() {
            continue;
        }
        let Some(m) = w.masse.get(&h).copied() else { return (None, None) };
        let breite = f64::from((m[2] - m[4] - m[6] - be.fuellung_links - be.fuellung_rechts).max(0.0));
        let hoehe = f64::from((m[3] - m[5] - m[7] - be.fuellung_oben - be.fuellung_unten).max(0.0));
        return (Some(breite), (art == "size").then_some(hoehe));
    }
    (None, None)
}

/// Release 12.10.2026: der umgebende Block (x, y, Breite, Hoehe) eines
/// Kastens: Inhaltsbox des Elternelements (im Fluss, relativ), Polsterbox des
/// naechsten positionierten (absolut) bzw. transformierten (fest) Vorfahren,
/// sonst die Ansicht.
fn umgebender_block(w: &mut Stand, id: u32, position: narsil_css::Position) -> Option<(f32, f32, f32, f32)> {
    let (Some(l), Some(satz)) = (w.lebend.as_ref(), w.stil_satz.as_ref()) else { return None };
    let ansicht = (0.0, 0.0, satz.ansicht.0 as f32, satz.ansicht.1 as f32);
    let absolut = matches!(position, narsil_css::Position::Absolut | narsil_css::Position::Fest);
    for h in l.vorfahren_hosts(id) {
        let Some(e) = l.knoten(h).filter(|e| l.dok.ist_element_knoten(*e)) else { continue };
        let be = crate::lebend::berechnet(l, e, satz, &mut w.stil_cache)?;
        if absolut {
            let schluessel = |n: &str| be.schluessel.iter().rev().find(|(k, _)| k == n).map(|(_, v)| v.trim().to_ascii_lowercase());
            let transformiert = schluessel("transform").is_some_and(|v| v != "none")
                || schluessel("will-change").is_some_and(|v| v.contains("transform"));
            let positioniert = position == narsil_css::Position::Absolut && be.position != narsil_css::Position::Statisch;
            if !(transformiert || positioniert) {
                continue;
            }
            let m = *w.masse.get(&h)?;
            return Some((m[0] + m[4], m[1] + m[5], (m[2] - m[4] - m[6]).max(0.0), (m[3] - m[5] - m[7]).max(0.0)));
        }
        let m = *w.masse.get(&h)?;
        return Some((
            m[0] + m[4] + be.fuellung_links,
            m[1] + m[5] + be.fuellung_oben,
            (m[2] - m[4] - m[6] - be.fuellung_links - be.fuellung_rechts).max(0.0),
            (m[3] - m[5] - m[7] - be.fuellung_oben - be.fuellung_unten).max(0.0),
        ));
    }
    Some(ansicht)
}

/// Der aufgeloeste Versatz `name` eines positionierten Kastens (siehe oben).
fn versatz_aufgeloest(w: &mut Stand, id: u32, k: usize, b: &narsil_css::Berechnet, name: &str) -> Option<f64> {
    let gegen = match name {
        "top" => "bottom",
        "bottom" => "top",
        "left" => "right",
        _ => "left",
    };
    let senkrecht = matches!(name, "top" | "bottom");
    crate::zwangslayout::erzwingen(w);
    let cb = umgebender_block(w, id, b.position)?;
    let basis = f64::from(if senkrecht { cb.3 } else { cb.2 });
    let spez_von = |w: &Stand, n: &str| -> String {
        w.lebend
            .as_ref()
            .and_then(|l| crate::lebend::stil_spez(l, k, n))
            .or_else(|| narsil_css::berechnet_css::berechneter_wert(b, n))
            .unwrap_or_else(|| String::from("auto"))
    };
    let s_eigen = spez_von(w, name);
    let s_gegen = spez_von(w, gegen);
    let (Some(l), Some(satz)) = (w.lebend.as_ref(), w.stil_satz.as_ref()) else { return None };
    let eigen = crate::lebend::laenge_mit_basis(l, k, name, &s_eigen, satz, &mut w.stil_cache, basis);
    if let Some(v) = eigen {
        return Some(v);
    }
    if !s_eigen.trim().eq_ignore_ascii_case("auto") {
        return None;
    }
    if b.position == narsil_css::Position::Relativ {
        let anderes = crate::lebend::laenge_mit_basis(l, k, gegen, &s_gegen, satz, &mut w.stil_cache, basis);
        return Some(anderes.map_or(0.0, |o| -o));
    }
    // Absolut/fest: der genutzte Wert aus dem Layout (Randbox zum Block).
    let m = *w.masse.get(&id)?;
    let (ml, mr, mo, mu) = (b.marge_links, b.marge_rechts, b.marge_oben, b.marge_unten);
    Some(f64::from(match name {
        "left" => m[0] - ml - cb.0,
        "right" => cb.0 + cb.2 - (m[0] + m[2] + mr),
        "top" => m[1] - mo - cb.1,
        _ => cb.1 + cb.3 - (m[1] + m[3] + mu),
    }))
}

/// Release 12.10.2026: der genutzte automatische Rand (links/rechts) aus den
/// Kaesten: Abstand zur Inhaltskante des Elternelements (im Fluss) bzw. zur
/// Polsterkante plus `left`/`right` des enthaltenden Blocks (absolut).
fn auto_rand_genutzt(w: &mut Stand, id: u32, b: &narsil_css::Berechnet, links: bool) -> Option<f32> {
    let (Some(l), Some(satz)) = (w.lebend.as_ref(), w.stil_satz.as_ref()) else { return None };
    let m = *w.masse.get(&id)?;
    let absolut = matches!(b.position, narsil_css::Position::Absolut | narsil_css::Position::Fest);
    for h in l.vorfahren_hosts(id) {
        let Some(e) = l.knoten(h).filter(|e| l.dok.ist_element_knoten(*e)) else { continue };
        let be = crate::lebend::berechnet(l, e, satz, &mut w.stil_cache)?;
        if absolut && be.position == narsil_css::Position::Statisch {
            continue;
        }
        let mc = *w.masse.get(&h)?;
        let (rand_l, rand_r) = if absolut {
            (mc[0] + mc[4] + b.links?, mc[0] + mc[2] - mc[6] - b.rechts?)
        } else {
            (mc[0] + mc[4] + be.fuellung_links, mc[0] + mc[2] - mc[6] - be.fuellung_rechts)
        };
        return Some(if links { m[0] - rand_l } else { rand_r - (m[0] + m[2]) });
    }
    None
}

/// Release 12.10.2026: die Inhaltsbreite des Elternelements (des
/// umgebenden Blocks im Fluss) aus dem Layout; ohne Eltern die Ansicht.
fn umgebende_breite(w: &mut Stand, id: u32, position: narsil_css::Position) -> Option<f64> {
    let (Some(l), Some(satz)) = (w.lebend.as_ref(), w.stil_satz.as_ref()) else { return None };
    let ansicht = f64::from(satz.ansicht.0);
    let absolut = matches!(position, narsil_css::Position::Absolut | narsil_css::Position::Fest);
    for h in l.vorfahren_hosts(id) {
        let Some(e) = l.knoten(h).filter(|e| l.dok.ist_element_knoten(*e)) else { continue };
        let be = crate::lebend::berechnet(l, e, satz, &mut w.stil_cache)?;
        // Absolut: der naechste positionierte (oder transformierte) Vorfahr,
        // dessen Polsterbox (CSS 2.1 §10.1, CSS Transforms 1 §2); fest: die
        // Ansicht, ausser ein Vorfahr transformiert.
        if absolut {
            let schluessel = |n: &str| be.schluessel.iter().rev().find(|(k, _)| k == n).map(|(_, v)| v.trim().to_ascii_lowercase());
            let transformiert = schluessel("transform").is_some_and(|v| v != "none")
                || schluessel("will-change").is_some_and(|v| v.contains("transform"));
            let positioniert = position == narsil_css::Position::Absolut && be.position != narsil_css::Position::Statisch;
            if !(transformiert || positioniert) {
                continue;
            }
            let m = *w.masse.get(&h)?;
            return Some(f64::from((m[2] - m[4] - m[6]).max(0.0)));
        }
        let m = *w.masse.get(&h)?;
        return Some(f64::from((m[2] - m[4] - m[6] - be.fuellung_links - be.fuellung_rechts).max(0.0)));
    }
    Some(ansicht)
}

/// Release 12.10.2026: welche `<style>`/`<link>` im Dokument haengen (Knoten
/// und Pruefsumme ihres Texts bzw. ihrer Adresse).
fn stil_signatur(w: &Stand) -> Vec<(usize, u64)> {
    use std::hash::{Hash, Hasher};
    let Some(l) = w.lebend.as_ref() else { return Vec::new() };
    let mut raus = Vec::new();
    for k in 0..l.dok.knoten.len().min(1_000_000) {
        let Some(n) = l.dok.element_name(k) else { continue };
        let style = n.eq_ignore_ascii_case("style");
        if !style && !n.eq_ignore_ascii_case("link") {
            continue;
        }
        // Nur verbundene Knoten (bis zur Wurzel). Release 14.10.2026 (Agent
        // C): also through the parent's child list — a parser script hides the
        // rest of the document by cutting the child lists only
        // (`Lebend::schwanz_ausblenden`). MEASURED before (WPT css-cascade
        // revert-rule-important): a later script still saw the sheet set of
        // the first one.
        let mut x = Some(k);
        let mut verbunden = false;
        for _ in 0..4096 {
            match x {
                Some(e) if e == l.wurzel => {
                    verbunden = true;
                    break;
                }
                Some(e) => {
                    let p = l.dok.eltern(e);
                    if p.is_some_and(|p| !l.dok.kinder(p).contains(&e)) {
                        break;
                    }
                    x = p;
                }
                None => break,
            }
        }
        if !verbunden {
            continue;
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        if style {
            l.dok.text_inhalt(k).hash(&mut h);
        } else {
            l.dok.attribut(k, "href").hash(&mut h);
            l.dok.attribut(k, "rel").hash(&mut h);
            l.dok.attribut(k, "disabled").is_some().hash(&mut h);
        }
        raus.push((k, h.finish()));
    }
    raus
}

/// Release 12.10.2026: ist der Host-Knoten ein `<style>`/`<link>` oder ein
/// Text darin? (Dann aendert eine Schreibung den Blattsatz.)
fn stil_knoten(w: &Stand, id: u32) -> bool {
    let Some(l) = w.lebend.as_ref() else { return false };
    let Some(k) = l.knoten(id) else { return false };
    let ist = |k: usize| l.dok.element_name(k).is_some_and(|n| n.eq_ignore_ascii_case("style") || n.eq_ignore_ascii_case("link"));
    ist(k) || l.dok.eltern(k).is_some_and(ist)
}

/// Release 12.10.2026: den Blattsatz aus dem aktuellen DOM neu lesen (nach
/// `insertRule`, `style.textContent = …`, einem eingefuegten `<style>`).
/// GEMESSEN vorher (WPT css/cssom CSSStyleSheet-insertRule-computed,
/// style-element-insertion-removal-computed, set-selector-text-attachment):
/// `getComputedStyle` sah bis zum naechsten Neubau die alten Regeln.
fn blaetter_neu(w: &mut Stand) {
    let Some(satz) = w.stil_satz.clone() else { return };
    let Some(l) = w.lebend.as_mut() else { return };
    let (html, _ids) = l.html_neu();
    let seite = narsil_page::seite_aus_knoten_mit_ansicht_zustand(
        &html,
        &narsil_page::Patch::default(),
        satz.ansicht.0,
        satz.ansicht.1,
        &satz.basis,
        &satz.messung,
        &satz.zustand,
    );
    if let Some(neu) = seite.stil_satz {
        w.stil_satz = Some(neu);
        crate::anim_hosts::sheets_changed(w);
        w.layout_veraltet = true;
    }
}

/// Release 12.10.2026: Namensraeume aus der Kodierung des Vorspanns
/// (`standard U+1E praefix U+1F url U+1E …`).
fn namensraeume_aus(w: Option<&narsil_jsvm::Wert>) -> narsil_css::cssom::selektor::Namensraeume {
    let mut ns = narsil_css::cssom::selektor::Namensraeume::default();
    if let Some(narsil_jsvm::Wert::Text(k)) = w {
        let mut teile = k.split('\u{1e}');
        if let Some(s) = teile.next()
            && !s.is_empty()
        {
            ns.standard = Some(s.to_string());
        }
        for p in teile {
            if let Some((a, b)) = p.split_once('\u{1f}') {
                ns.praefixe.push((a.to_string(), b.to_string()));
            }
        }
    }
    ns
}

/// Release 12.10.2026: eine gelesene Regel als JS-Objekt fuer den Vorspann.
fn regel_wert(r: &narsil_css::cssom::regeln::Regel) -> narsil_jsvm::Wert {
    use narsil_jsvm::Wert;
    let mut f = vec![
        ("art".to_string(), Wert::Text(r.art.to_string())),
        ("kopf".to_string(), Wert::Text(r.kopf.clone())),
        ("roh".to_string(), Wert::Text(r.roh.clone())),
        ("block".to_string(), r.block.as_ref().map_or(Wert::Null, |b| Wert::Text(b.kodieren()))),
        ("kinder".to_string(), r.kinder.as_ref().map_or(Wert::Null, |k| Wert::Feld(k.iter().map(regel_wert).collect()))),
    ];
    for (n, v) in &r.felder {
        f.push(((*n).to_string(), Wert::Text(v.clone())));
    }
    Wert::Gegenstand(f)
}

/// Release 10.10.2026: der berechnete Wert fuer `getComputedStyle` (CSSOM
/// §6.7) — die Kaskade des Elements (`lebend::berechnet`), `width`/`height`
/// als genutzte Groesse aus dem Layout. `None` = unbekannt (der Vorspann
/// faellt zurueck). Release 12.10.2026: als Funktion (Kurzformen rufen sie
/// je Langform).
fn stil_berechnet(w: &mut Stand, id: u32, name: &str, tiefe: usize) -> Option<String> {
    let mut r = stil_berechnet_kern(w, id, name, tiefe);
    // Release 15.10.2026: `animation-duration: auto` resolves to `0s` with
    // the document timeline (`animation-timeline: auto`; CSS Animations 2
    // §3.3; WPT css-animations animation-duration-computed).
    if name == "animation-duration"
        && r.as_deref().is_some_and(|t| t.split(',').any(|p| p.trim() == "auto"))
        && stil_berechnet_kern(w, id, "animation-timeline", tiefe + 1).as_deref().is_none_or(|t| t.trim() == "auto")
    {
        r = r.map(|t| t.split(',').map(|p| if p.trim() == "auto" { "0s" } else { p.trim() }).collect::<Vec<_>>().join(", "));
    }
    // Release 12.10.2026: `transform-origin`/`perspective-origin` sind
    // aufgeloest der genutzte Wert in px gegen den Rahmenkasten (CSSOM §9; WPT
    // css-transforms transform-origin-computed, perspective-origin-computed).
    if matches!(name, "transform-origin" | "perspective-origin")
        && let Some(t) = r.clone()
        && (t.contains('%') || t.contains("calc("))
    {
        crate::zwangslayout::erzwingen(w);
        if let Some(m) = w.masse.get(&id).copied() {
            r = Some(ursprung_px(&t, f64::from(m[2]), f64::from(m[3])));
        }
    }
    // Release 12.10.2026: `url()` im berechneten Wert absolut gegen die
    // Basisadresse des Dokuments (CSS Values 4 §4.5.1) — auch fuer Werte aus
    // `<style>`-Blaettern, die die Kaskade woertlich traegt (Link-Blaetter sind
    // schon beim Laden absolut). GEMESSEN vorher (WPT css-values
    // urls/resolve-relative-to-base): `url(images/test.png)` blieb relativ.
    if !name.starts_with("--")
        && r.as_deref().is_some_and(|t| t.contains("url("))
        && let (Some(l), Some(satz)) = (w.lebend.as_ref(), w.stil_satz.as_ref())
        && !satz.basis.is_empty()
    {
        let basis = crate::lebend::dokument_basis(l, &satz.basis);
        r = r.map(|t| crate::blaetter::urls_absolut(&t, &basis));
    }
    // Release 12.10.2026: `min-width`/`min-height: auto` ist aufgeloest meist
    // `0px` (`lebend::min_auto_bleibt`).
    if matches!(name, "min-width" | "min-height" | "min-inline-size" | "min-block-size")
        && r.as_deref() == Some("auto")
        && let (Some(l), Some(satz)) = (w.lebend.as_ref(), w.stil_satz.as_ref())
        && let Some(k) = l.knoten(id)
        && !crate::lebend::min_auto_bleibt(l, k, satz, &mut w.stil_cache)
    {
        return Some(String::from("0px"));
    }
    r
}

/// Release 12.10.2026: die Teile eines Ursprungs (`10% calc(100% - 5px) 0px`)
/// gegen Breite bzw. Hoehe in px.
fn ursprung_px(text: &str, breite: f64, hoehe: f64) -> String {
    use narsil_css::cssom::syntax::{self, Kv, Tok};
    let kv = syntax::komponenten(text);
    let teile: Vec<&Kv> = kv.iter().filter(|k| !k.ist_leer()).collect();
    let px = |w: f64| format!("{}px", narsil_css::berechnet_css::zahl(w as f32));
    teile
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let basis = if i == 0 { breite } else { hoehe };
            match k {
                Kv::T(Tok::Prozent(w, _)) if i < 2 => px(w / 100.0 * basis),
                Kv::F(n, a) if i < 2 => {
                    let u = narsil_css::cssom::mathe::Umgebung { prozent: Some(basis), ..Default::default() };
                    narsil_css::cssom::mathe::lesen(n, a, narsil_css::cssom::mathe::Ziel::LaengeProzent)
                        .and_then(|m| m.aufgeloest(&u).zahlwert().map(|(w, e)| (w, e.to_string())))
                        .filter(|(_, e)| e == "px")
                        .map_or_else(|| syntax::kv_text(std::slice::from_ref(*k)), |(w, _)| px(w))
                }
                k => syntax::kv_text(std::slice::from_ref(*k)),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Rereads the style sheets when they changed since the last computed value
/// (Release 15.10.2026: shared with `anim_hosts`).
pub(crate) fn refresh_styles(w: &mut Stand) {
    if w.stil_pruefen {
        w.stil_pruefen = false;
        let neu = stil_signatur(w);
        // Ohne fruehere Signatur weiss niemand, was der Blattsatz traegt:
        // einmal neu lesen.
        match &w.stil_signatur {
            Some(alt) if *alt == neu => {}
            _ => w.stil_veraltet = true,
        }
        w.stil_signatur = Some(neu);
    }
    if w.stil_veraltet {
        w.stil_veraltet = false;
        blaetter_neu(w);
    }
}

fn stil_berechnet_kern(w: &mut Stand, id: u32, name: &str, tiefe: usize) -> Option<String> {
    if tiefe > 2 {
        return None;
    }
    // Release 12.10.2026: Alias-Namen (`-webkit-transition`) wie ihr Ziel
    // (CSSOM §6.7.1 „legacy name alias"; WPT computed-style-set-property).
    if !name.starts_with("--")
        && let Some(k) = narsil_css::cssom::block::name_kanonisch(name)
        && k != name
    {
        return stil_berechnet(w, id, &k, tiefe);
    }
    refresh_styles(w);
    let (Some(l), Some(satz)) = (w.lebend.as_ref(), w.stil_satz.as_ref()) else {
        return None;
    };
    let Some(k) = l.knoten(id) else { return None };
    let Some(b) = crate::lebend::berechnet(l, k, satz, &mut w.stil_cache) else {
        return None;
    };
    if let Some(v) = name.strip_prefix("--") {
        // Release 12.10.2026: `attr()` in einer Custom Property gilt beim
        // berechneten Wert (WPT css-values attr-length-specified); was attr()
        // einsetzt, wird wieder eingesetzt (`lebend::ersetzen_kette`).
        let attr = |n: &str| l.dok.attribut(k, n).map(str::to_string);
        narsil_css::cssom::ersetzen::geschwister_setzen(crate::lebend::geschwister_von(l, k));
        let wenn = crate::lebend::LebendWenn::neu(l, k, satz, &mut w.stil_cache);
        // Der Rohwert zuerst (ein `if()` darin loest seine Bedingungen selbst).
        let start = b.variablen.holen(name).map_or_else(|| format!("var(--{v})"), str::to_string);
        // Unter Zykluswache: eine Bedingung darf auf die Eigenschaft selbst
        // zurueckfuehren (WPT css-values if-cycle).
        let roh = crate::lebend::wenn_wache(name, || {
            crate::lebend::ersetzen_kette_mit(
                &start,
                &b.variablen,
                &attr,
                false,
                wenn.as_ref().map(|x| x as &dyn narsil_css::cssom::wenn::Bedingungen),
            )
        });
        // Release 13.10.2026: whitespace at the end of a chosen if() branch
        // stays (WPT if-conditionals 1: `if( style( --x : 3 ) : true_value )`
        // gives `true_value `).
        let has_if = start.to_ascii_lowercase().contains("if(");
        let roh = roh.map(|t| if has_if { t.trim_start().to_string() } else { t.trim().to_string() });
        // Release 13.10.2026: a CSS-wide keyword from `attr()` acts as if it
        // were specified (`LebendWenn::css_wide_keyword`).
        if let Some(t) = &roh
            && start.to_ascii_lowercase().contains("attr(")
            && let Some(lw) = &wenn
            && let Some(resolved) = lw.css_wide_keyword(name, t)
        {
            return Some(resolved);
        }
        return Some(roh.unwrap_or_default());
    }
    // Release 12.10.2026: `attr()` kennt die Kaskade inzwischen selbst
    // (`narsil_css::attr_kaskade`) — die genutzte Groesse kommt aus dem Layout
    // (WPT css-values attr-all-types: ungueltig heisst `unset`, also 784px).
    let geometrie = matches!(name, "width" | "height")
        && !matches!(b.anzeige, narsil_css::Anzeige::Keins | narsil_css::Anzeige::Inline)
        && !b.nur_inhalt;
    if geometrie {
        crate::zwangslayout::erzwingen(w);
    }
    // Release 11.10.2026: `grid-template-columns/-rows` eines
    // gerenderten Rasters sind die Spuren in px (aufgeloester Wert,
    // CSS Grid 1 §7.2.6). GEMESSEN vorher (WPT css-grid
    // grid-definition/parsing): der angegebene Wert (`1fr 2fr`).
    if matches!(name, "grid-template-columns" | "grid-template-rows")
        && b.anzeige == narsil_css::Anzeige::Raster
    {
        crate::zwangslayout::erzwingen(w);
        if let Some((spalten, zeilen)) = w.raster_vorlagen.get(&id) {
            let t = if name == "grid-template-columns" { spalten } else { zeilen };
            if !t.is_empty() {
                return Some(t.clone());
            }
        }
    }
    if geometrie && let Some(m) = w.masse.get(&id) {
        let breit = name == "width";
        let rand = if breit { b.rahmen_links_breite + b.rahmen_rechts_breite } else { b.rahmen_oben_breite + b.rahmen_unten_breite };
        let fuell = if breit { b.fuellung_links + b.fuellung_rechts } else { b.fuellung_oben + b.fuellung_unten };
        let aussen = if breit { m[2] } else { m[3] };
        let v = if b.box_art == narsil_css::BoxArt::Rand { aussen } else { aussen - rand - fuell };
        return Some(format!("{}px", narsil_css::berechnet_css::zahl(v.max(0.0))));
    }
    // Release 12.10.2026: was die Kaskade nicht (oder verlustbehaftet)
    // kennt, rechnet das CSSOM-Modul aus dem angegebenen Wert
    // (`lebend::cssom_berechnet`).
    let kaskade = narsil_css::berechnet_css::berechneter_wert(&b, name);
    // Release 12.10.2026: die Versaetze positionierter Kaesten (CSSOM §9
    // „resolved value special case … top"): angegebene Werte absolut (Prozente
    // gegen den umgebenden Block), `auto` als genutzter Wert — relativ als
    // Gegenwert der anderen Seite, absolut/fest aus dem Layout. GEMESSEN
    // vorher (WPT cssom getComputedStyle-insets-*): `10%` und `auto` kamen
    // unveraendert.
    if matches!(name, "top" | "right" | "bottom" | "left")
        && matches!(b.position, narsil_css::Position::Relativ | narsil_css::Position::Absolut | narsil_css::Position::Fest)
        && b.anzeige != narsil_css::Anzeige::Keins
        && let Some(v) = versatz_aufgeloest(w, id, k, &b, name)
    {
        return Some(format!("{}px", narsil_css::berechnet_css::zahl(v as f32)));
    }
    // Release 12.10.2026: klebende Kaesten rechnen Prozente gegen den naechsten
    // Scrollport (CSS Position 3 §3.4, csswg-drafts#3115; WPT cssom
    // getComputedStyle-sticky-pos-percent, -insets-sticky). `auto` bleibt.
    if matches!(name, "top" | "right" | "bottom" | "left")
        && b.position == narsil_css::Position::Klebend
        && b.anzeige != narsil_css::Anzeige::Keins
        && let Some(s) = w.lebend.as_ref().and_then(|l| crate::lebend::stil_spez(l, k, name)).or_else(|| kaskade.clone())
        && (s.contains('%') || s.contains("calc("))
    {
        crate::zwangslayout::erzwingen(w);
        let senkrecht = matches!(name, "top" | "bottom");
        let mut basis: Option<f64> = None;
        if let (Some(l), Some(satz)) = (w.lebend.as_ref(), w.stil_satz.as_ref()) {
            for h in l.vorfahren_hosts(id) {
                let Some(e) = l.knoten(h).filter(|e| l.dok.ist_element_knoten(*e)) else { continue };
                let Some(be) = crate::lebend::berechnet(l, e, satz, &mut w.stil_cache) else { break };
                if be.ueberlauf != narsil_css::Ueberlauf::Sichtbar
                    && let Some(m) = w.masse.get(&h)
                {
                    basis = Some(f64::from(if senkrecht { m[3] - m[5] - m[7] - be.fuellung_oben - be.fuellung_unten } else { m[2] - m[4] - m[6] - be.fuellung_links - be.fuellung_rechts }));
                    break;
                }
            }
            let basis = basis.unwrap_or(if senkrecht { f64::from(satz.ansicht.1) } else { f64::from(satz.ansicht.0) });
            if let Some(v) = crate::lebend::laenge_mit_basis(l, k, name, &s, satz, &mut w.stil_cache, basis) {
                return Some(format!("{}px", narsil_css::berechnet_css::zahl(v as f32)));
            }
        }
    }
    // Release 12.10.2026: `margin-left/right: auto` ist als aufgeloester Wert
    // der genutzte Rand aus dem Layout (CSSOM §9; WPT cssom computed-style-005).
    let auto_rand = (name == "margin-left" && b.marge_links_auto) || (name == "margin-right" && b.marge_rechts_auto);
    if auto_rand
        && b.anzeige != narsil_css::Anzeige::Keins
        && b.anzeige != narsil_css::Anzeige::Inline
        && b.schweben == narsil_css::Float::Keins
        && (b.position != narsil_css::Position::Relativ || (b.links.unwrap_or(0.0) == 0.0 && b.links_prozent.is_none()))
    {
        crate::zwangslayout::erzwingen(w);
        if let Some(v) = auto_rand_genutzt(w, id, &b, name == "margin-left") {
            return Some(format!("{}px", narsil_css::berechnet_css::zahl(v.max(0.0))));
        }
    }
    // Release 12.10.2026: Rand/Polster mit Prozenten gegen die Inhaltsbreite
    // des umgebenden Blocks (`lebend::cssom_genutzt`; nur im Fluss).
    if (name.starts_with("margin-") || name.starts_with("padding-"))
        && !matches!(b.position, narsil_css::Position::Klebend)
        && b.anzeige != narsil_css::Anzeige::Keins
        && w.lebend.as_ref().is_some_and(|l| crate::lebend::stil_spez(l, k, name).is_some_and(|s| s.contains('%')))
    {
        crate::zwangslayout::erzwingen(w);
        if let Some(basis) = umgebende_breite(w, id, b.position)
            && let (Some(l), Some(satz)) = (w.lebend.as_ref(), w.stil_satz.as_ref())
            && let Some(t) = crate::lebend::cssom_genutzt(l, k, name, kaskade.as_deref(), satz, &mut w.stil_cache, basis)
        {
            return Some(t);
        }
    }
    // Release 12.10.2026: Container-Einheiten (`cqw` …) gegen den naechsten
    // Groessen-Container aus dem Layout (CSS Contain 3 §6.1). GEMESSEN vorher
    // (WPT css-flexbox flex-computed, css-text tab-size-computed):
    // `sign(20cqw - 10px)` rechnete mit der Ansicht.
    let cq = w.lebend.as_ref().is_some_and(|l| crate::lebend::stil_spez(l, k, name).is_some_and(|s| s.contains("cq")));
    if cq {
        crate::zwangslayout::erzwingen(w);
        let m = behaelter_masse(w, id);
        crate::lebend::setze_behaelter(m);
    }
    let r = match (w.lebend.as_ref(), w.stil_satz.as_ref()) {
        (Some(l), Some(satz)) => crate::lebend::cssom_berechnet(l, k, name, kaskade.as_deref(), satz, &mut w.stil_cache),
        _ => None,
    };
    if cq {
        crate::lebend::setze_behaelter((None, None));
    }
    if let Some(t) = r {
        return Some(t);
    }
    // Release 12.10.2026: CSSOM-Kurzformen zuerst aus ihren Langformen (die
    // Kaskade kennt z. B. `white-space: break-spaces` nicht; `overflow: clip`
    // kam als `hidden`).
    // Ohne Inline-Angabe (Kurzform oder eine ihrer Langformen) bleibt es beim
    // Kaskadenwert der Kurzform, wenn die Kaskade sie kennt (WPT css-variables
    // css-variable-change-style-002: `white-space: var(--x)` im Stylesheet).
    let inline_beteiligt = narsil_css::cssom::kurz::langformen(name).is_some_and(|lang| {
        w.lebend.as_ref().is_some_and(|l| {
            crate::lebend::stil_spez(l, k, name).is_some()
                || lang.iter().any(|n| crate::lebend::stil_spez(l, k, n).is_some() || crate::lebend::stil_block_ausstehend(l, k, n))
        })
    });
    // Woertlich gespeicherte Angaben der Kaskade (`font-variant` …) liest das
    // CSSOM selbst (`lebend::spez_kaskade`) — dann die Langformen.
    let schluessel_beteiligt = narsil_css::cssom::kurz::langformen(name).is_some_and(|lang| {
        b.schluessel.iter().any(|(n, _)| n == name || lang.contains(&n.as_str()))
    });
    if !inline_beteiligt && !schluessel_beteiligt && kaskade.is_some() {
        return kaskade;
    }
    if let Some(lang) = narsil_css::cssom::kurz::langformen(name)
        && tiefe < 2
    {
        let mut werte = Vec::with_capacity(lang.len());
        let mut alle = true;
        for l in lang {
            match stil_berechnet(w, id, l, tiefe + 1) {
                Some(v) => werte.push(v),
                None => {
                    alle = false;
                    break;
                }
            }
        }
        if alle && let Some(s) = narsil_css::cssom::kurz::schreiben(name, &werte) {
            return Some(s);
        }
    }
    if kaskade.is_some() {
        return kaskade;
    }
    // Release 12.10.2026: eine Kurzform aus den berechneten Werten
    // ihrer Langformen (CSSOM §6.7: der aufgeloeste Wert einer
    // Kurzform). GEMESSEN vorher (WPT): `getComputedStyle(e).borderWidth`,
    // `.placeContent`, `.gridArea` waren leer.
    let lang = narsil_css::cssom::kurz::langformen(name)?;
    let mut werte = Vec::with_capacity(lang.len());
    for l in lang {
        werte.push(stil_berechnet(w, id, l, tiefe + 1)?);
    }
    narsil_css::cssom::kurz::schreiben(name, &werte)
}

fn canvas_groesse(w: &Stand, id: u32) -> (u32,u32) {
    let attributes = w.attribute.get(&id);
    let lebend = w.lebend.as_ref().and_then(|l| l.knoten(id).map(|k| (l, k)));
    let dimension = |name: &str, fallback| {
        let v = match lebend {
            Some((l, k)) => l.dok.attribut(k, name).map(str::to_string),
            None => attributes.and_then(|a| a.get(name)).cloned(),
        };
        v.map_or(fallback, |v| crate::canvas::parse_dimension(&v, fallback))
    };
    (dimension("width",300),dimension("height",150))
}

/// The bitmap size a `<canvas>` element asks for (HTML §4.12.5).
pub(crate) fn canvas_size(w: &Stand, id: u32) -> (u32, u32) {
    canvas_groesse(w, id)
}

/// A computed style value of an element (for canvas `currentColor`).
pub(crate) fn computed_value(w: &mut Stand, id: u32, name: &str) -> Option<String> {
    stil_berechnet(w, id, name, 0)
}

fn canvas_reset(w: &mut Stand, id: u32) -> Result<(),String> {
    let (width,height) = canvas_groesse(w,id);
    w.canvas.element_resized(id, width, height);
    Ok(())
}

/// Die geteilte Kiste.
///
/// WARUM EIN `Mutex`: die Engine laeuft im Kind-Thread (`VERTRAG_JS`), und
/// die Closures muessen `Send` sein.
pub type Geteilt = Arc<Mutex<Stand>>;

/// Haengt Getter, Setter und Aufruf an die Engine.
pub fn anbinden(engine: &mut narsil_jsvm::Engine, stand: Geteilt) {
    // Release 15.10.2026: the Canvas 2D API (`canvas2d.js` -> `crate::canvas`).
    engine.host_funktion("__narsilC2d", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let mut w = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
            crate::canvas::host_call(&mut w, args)
        }
    });
    engine.host_funktion("__narsilC2dQuelle", |_args: &[narsil_jsvm::Wert]| Ok(narsil_jsvm::Wert::Text(crate::canvas::SCRIPT.to_string())));
    // Release 15.10.2026: `FontFace` fonts for the text engine (family,
    // weight, italic, bytes) -> registered?
    engine.host_funktion("__narsilSchriftAnmelden", |args: &[narsil_jsvm::Wert]| {
        let familie = args.first().map(|a| a.als_text()).unwrap_or_default();
        let gewicht = match args.get(1) {
            Some(narsil_jsvm::Wert::Zahl(n)) if n.is_finite() => n.clamp(1.0, 1000.0) as u16,
            _ => 400,
        };
        let kursiv = matches!(args.get(2), Some(narsil_jsvm::Wert::Wahr(true)));
        let Some(narsil_jsvm::Wert::ByteFeld { daten, .. }) = args.get(3) else { return Ok(narsil_jsvm::Wert::Wahr(false)) };
        // Optional fifth argument: font-stretch in percent.
        let stretch = match args.get(4) {
            Some(narsil_jsvm::Wert::Zahl(n)) if n.is_finite() => (n.clamp(50.0, 200.0) * 10.0).round() as u16,
            _ => narsil_text::webschrift::NORMAL_STRETCH,
        };
        Ok(narsil_jsvm::Wert::Wahr(narsil_text::webschrift::register_with_stretch(&familie, (gewicht, gewicht), kursiv, stretch, daten).is_ok()))
    });
    // Bare global listeners are Window listeners, with the same registry
    // as `window.addEventListener`, rather than a second VM listener list.
    for (name, method) in [("addEventListener", "13"), ("removeEventListener", "14")] {
        let state = stand.clone();
        engine.host_funktion(name, move |args: &[narsil_jsvm::Wert]| {
            zaehle()?;
            rufen(&state, WINDOW_HOST_ID, method, args)
        });
    }
    // Release 09.10.2026: `__narsilLauscher(knoten, typ, capture)` — die
    // Lauscher eines Knotens (oder des Fensters) fuer einen Typ und eine
    // Phase, in Anmeldereihenfolge, `once` schon abgemeldet. Der Vorspann
    // baut damit `dispatchEvent` mit Capture/Ziel/Bubble.
    //
    // GEMESSEN vorher: `el.dispatchEvent(ev)` aus einem Skript rief keinen
    // einzigen Lauscher (die Anmeldungen liegen hier, der Motor suchte in
    // seiner eigenen Liste). youtube.com: `ytd-page-manager` meldet sich mit
    // einem `attached`-Ereignis bei `ytd-app` — es kam nie an, die Startseite
    // blieb beim Skelett stehen.
    // Release 10.10.2026: die Dateiauswahl fuer die Seite —
    // `__narsilDateien(el)` (Name, Groesse, Typ, Zeit je Datei, ohne Pfad),
    // `__narsilDateiBytes(el, i)` (der Inhalt) und `__narsilDateiDialog(el)`
    // (`input.click()` auf einem Dateifeld).
    engine.host_funktion("__narsilDateien", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let Some(id) = args.first().and_then(narsil_jsvm::host_id) else {
                return Ok(narsil_jsvm::Wert::Null);
            };
            let w = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
            let Some(liste) = w.dateien.get(&id) else {
                return Ok(narsil_jsvm::Wert::Feld(Vec::new()));
            };
            let stand = w.dateien_stand;
            Ok(narsil_jsvm::Wert::Feld(
                liste
                    .iter()
                    .enumerate()
                    .map(|(i, d)| {
                        narsil_jsvm::Wert::Gegenstand(vec![
                            ("name".to_string(), narsil_jsvm::Wert::Text(d.name.clone())),
                            ("size".to_string(), narsil_jsvm::Wert::Zahl(d.groesse as f64)),
                            ("type".to_string(), narsil_jsvm::Wert::Text(d.typ.clone())),
                            ("lastModified".to_string(), narsil_jsvm::Wert::Zahl(d.geaendert_ms)),
                            ("schluessel".to_string(), narsil_jsvm::Wert::Text(format!("{id}:{stand}:{i}"))),
                        ])
                    })
                    .collect(),
            ))
        }
    });
    engine.host_funktion("__narsilDateiBytes", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let Some(id) = args.first().and_then(narsil_jsvm::host_id) else {
                return Ok(narsil_jsvm::Wert::Null);
            };
            let nr = match args.get(1) {
                Some(narsil_jsvm::Wert::Zahl(z)) if *z >= 0.0 => *z as usize,
                _ => return Ok(narsil_jsvm::Wert::Null),
            };
            let pfad = {
                let w = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
                match w.dateien.get(&id).and_then(|l| l.get(nr)) {
                    Some(d) if d.groesse <= MAX_DATEI_BYTES => d.pfad.clone(),
                    _ => return Ok(narsil_jsvm::Wert::Null),
                }
            };
            Ok(match std::fs::read(&pfad) {
                Ok(daten) => narsil_jsvm::Wert::ByteFeld { daten, geklemmt: false },
                Err(_) => narsil_jsvm::Wert::Null,
            })
        }
    });
    engine.host_funktion("__narsilDateiDialog", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let Some(id) = args.first().and_then(narsil_jsvm::host_id) else {
                return Ok(narsil_jsvm::Wert::Wahr(false));
            };
            let mut w = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
            w.datei_dialog_wunsch = Some(id);
            Ok(narsil_jsvm::Wert::Wahr(true))
        }
    });
    // Release 09.10.2026: `__narsilMass(el)` — der Kasten des Elements im
    // letzten Layout als `[x, y, breite, hoehe, rollen_y]` (Seitenkoordinaten),
    // `null`, wenn es (noch) keinen hat. GEMESSEN vorher: `offsetWidth` war
    // `undefined`, `getBoundingClientRect()` immer 0x0 — youtube.com rechnete
    // die Kacheln je Zeile aus der Gitterbreite und stempelte keine einzige.
    // Release 10.10.2026: `__narsilStil(el, name)` — der berechnete Wert fuer
    // `getComputedStyle` (CSSOM §6.7): die Kaskade des Elements
    // (`lebend::berechnet`), `width`/`height` als genutzte Groesse aus dem
    // Layout. `null` = unbekannt (der Vorspann faellt zurueck).
    // Release 12.10.2026: `__narsilDekl(op, block, …)` — der CSSOM-
    // Deklarationsblock (`narsil_css::cssom::block`) fuer `rule.style` im
    // Vorspann; `block` ist die Kodierung aus `Block::kodieren`.
    engine.host_funktion("__narsilDekl", |args: &[narsil_jsvm::Wert]| {
        use narsil_css::cssom::block::Block;
        use narsil_jsvm::Wert;
        let t = |i: usize| match args.get(i) {
            None | Some(Wert::Null | Wert::Undefiniert) => String::new(),
            Some(a) => a.als_text(),
        };
        let op = t(0);
        Ok(match op.as_str() {
            "lesen" => Wert::Text(Block::lesen(&t(1)).kodieren()),
            "lesen_roh" => Wert::Text(Block::lesen_roh(&t(1)).kodieren()),
            "lesen_seite" => Wert::Text(Block::lesen_im(&t(1), narsil_css::cssom::block::SEITE).kodieren()),
            "lesen_kf" => Wert::Text(Block::lesen_im(&t(1), narsil_css::cssom::block::SCHLUESSELBILD).kodieren()),
            "text" => Wert::Text(Block::dekodieren(&t(1)).text()),
            "wert" => Wert::Text(Block::dekodieren(&t(1)).wert(&t(2))),
            "prio" => Wert::Text(Block::dekodieren(&t(1)).prioritaet(&t(2)).to_string()),
            "laenge" => Wert::Zahl(Block::dekodieren(&t(1)).laenge() as f64),
            "eintrag" => {
                let i = match args.get(2) {
                    Some(Wert::Zahl(z)) if *z >= 0.0 => *z as usize,
                    _ => usize::MAX,
                };
                Wert::Text(Block::dekodieren(&t(1)).eintrag(i).unwrap_or_default().to_string())
            }
            "setzen" => {
                let mut b = Block::dekodieren(&t(1));
                if b.setzen(&t(2), &t(3), &t(4)) { Wert::Text(b.kodieren()) } else { Wert::Null }
            }
            "entfernen" => {
                let mut b = Block::dekodieren(&t(1));
                let (alt, geaendert) = b.entfernen(&t(2));
                Wert::Feld(vec![Wert::Text(alt), if geaendert { Wert::Text(b.kodieren()) } else { Wert::Null }])
            }
            _ => Wert::Undefiniert,
        })
    });
    // Release 12.10.2026: `__narsilSelektor(text, namensraeume, streng)` — die
    // kanonische Schreibweise einer Selektorliste (CSSOM §6.6.3) oder `null`
    // (ungueltig). `namensraeume`: Standard und Praefixe des Blatts,
    // kodiert `standard \x1e praefix \x1f url \x1e …`.
    engine.host_funktion("__narsilSelektor", |args: &[narsil_jsvm::Wert]| {
        use narsil_css::cssom::selektor::{Namensraeume, liste_mit};
        use narsil_jsvm::Wert;
        let text = args.first().map(Wert::als_text).unwrap_or_default();
        let mut ns = Namensraeume::default();
        if let Some(Wert::Text(k)) = args.get(1) {
            let mut teile = k.split('\u{1e}');
            if let Some(s) = teile.next()
                && !s.is_empty()
            {
                ns.standard = Some(s.to_string());
            }
            for p in teile {
                if let Some((a, b)) = p.split_once('\u{1f}') {
                    ns.praefixe.push((a.to_string(), b.to_string()));
                }
            }
        }
        let streng = matches!(args.get(2), Some(Wert::Wahr(true)));
        Ok(liste_mit(&text, &ns, streng).map_or(Wert::Null, Wert::Text))
    });
    // Release 12.10.2026: `__narsilStilNamen()` — die Langformen fuer die
    // Aufzaehlung von `getComputedStyle` (`length`, `item()`, Iteration).
    // Dahinter die Custom Properties des Elements (CSSOM §9; WPT
    // cssstyledeclaration-custom-properties: `length` zaehlte sie nicht).
    engine.host_funktion("__narsilStilNamen", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let mut namen: Vec<String> = narsil_css::cssom::langformen_sortiert().into_iter().map(str::to_string).collect();
            if let Some(id) = args.first().and_then(narsil_jsvm::host_id)
                && let Ok(mut guard) = state.lock()
            {
                let w: &mut Stand = &mut guard;
                if let (Some(l), Some(satz)) = (w.lebend.as_ref(), w.stil_satz.as_ref())
                    && let Some(k) = l.knoten(id)
                    && let Some(b) = crate::lebend::berechnet(l, k, satz, &mut w.stil_cache)
                {
                    let mut eigene: Vec<String> = b.variablen.eintraege().into_iter().map(|(n, _)| n).collect();
                    eigene.sort();
                    eigene.dedup();
                    namen.extend(eigene);
                }
            }
            Ok(narsil_jsvm::Wert::Feld(namen.into_iter().map(narsil_jsvm::Wert::Text).collect()))
        }
    });
    // Release 12.10.2026: die Regeln eines Stylesheets fuer das CSSOM des
    // Vorspanns (`narsil_css::cssom::regeln`): `__narsilBlatt(text, ns)` alle
    // Regeln, `__narsilRegel(text, ns, ort, nach_import, nach_namespace)` genau
    // eine (`insertRule`; sonst der Fehlername), `__narsilMedien(text)` eine
    // Medienabfrageliste kanonisch, `__narsilSchluessel(text)` eine
    // Schluesselzeit (`from` -> `0%`).
    engine.host_funktion("__narsilBlatt", |args: &[narsil_jsvm::Wert]| {
        let text = args.first().map(narsil_jsvm::Wert::als_text).unwrap_or_default();
        let ns = namensraeume_aus(args.get(1));
        let regeln = narsil_css::cssom::regeln::blatt_lesen(&text, &ns);
        Ok(narsil_jsvm::Wert::Feld(regeln.iter().map(regel_wert).collect()))
    });
    // Release 13.10.2026: `__narsilBlattAus(el, off)` — the sheet of a
    // `<style>` element is disabled or enabled again (CSSOM §6.1 `disabled`).
    engine.host_funktion("__narsilBlattAus", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let Some(id) = args.first().and_then(narsil_jsvm::host_id) else {
                return Ok(narsil_jsvm::Wert::Null);
            };
            let off = matches!(args.get(1), Some(narsil_jsvm::Wert::Wahr(true)));
            let mut w = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
            let changed = match w.lebend.as_mut() {
                Some(l) => {
                    let changed = match l.knoten(id) {
                        // Release 14.10.2026: on a `<link>`, `false` means
                        // "explicitly enabled" (HTML §4.6.7).
                        Some(k) if l.dok.element_name(k).is_some_and(|n| n.eq_ignore_ascii_case("link")) => {
                            if off { l.enabled_links.remove(&k) } else { l.enabled_links.insert(k) }
                        }
                        Some(k) if off => l.disabled_sheets.insert(k),
                        Some(k) => l.disabled_sheets.remove(&k),
                        None => false,
                    };
                    // The page is built again after the scripts.
                    l.geaendert |= changed;
                    changed
                }
                None => false,
            };
            if changed {
                w.stil_veraltet = true;
                crate::anim_hosts::sheets_changed(&mut w);
                w.layout_veraltet = true;
            }
            Ok(narsil_jsvm::Wert::Null)
        }
    });
    // Release 13.10.2026: `__narsilBlattSpiegel(el, text)` — the CSSOM text of
    // a `<style>` sheet changed by scripts (`null`: back to the element text).
    engine.host_funktion("__narsilBlattSpiegel", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let Some(id) = args.first().and_then(narsil_jsvm::host_id) else {
                return Ok(narsil_jsvm::Wert::Null);
            };
            let text = match args.get(1) {
                Some(narsil_jsvm::Wert::Text(t)) => Some(t.clone()),
                _ => None,
            };
            let mut w = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
            let Some(l) = w.lebend.as_mut() else { return Ok(narsil_jsvm::Wert::Null) };
            let Some(k) = l.knoten(id) else { return Ok(narsil_jsvm::Wert::Null) };
            // The page tree carries the text in the element's first text node
            // or, without children, in a new one; otherwise (element
            // children only) the caller writes the element text (`false`).
            let has_text = l.dok.kinder(k).is_empty()
                || l.dok.kinder(k).iter().any(|&c| matches!(l.dok.knoten[c].art, narsil_fmt_html::dom::Art::Text(_)));
            match text {
                Some(t) if t.len() <= 16 * 1024 * 1024 && has_text => {
                    let key = crate::lebend::sheet_key(&l.dok, k);
                    l.sheet_texts.insert(k, (key, t));
                }
                Some(_) => return Ok(narsil_jsvm::Wert::Wahr(false)),
                None => {
                    l.sheet_texts.remove(&k);
                }
            }
            // The page is built again after the scripts (the DOM itself did
            // not change).
            l.geaendert = true;
            w.stil_veraltet = true;
            crate::anim_hosts::sheets_changed(&mut w);
            w.layout_veraltet = true;
            Ok(narsil_jsvm::Wert::Wahr(true))
        }
    });
    // Release 15.10.2026: animations (`anim_hosts`).
    crate::anim_hosts::register(engine, &stand);
    engine.host_funktion("__narsilRegel", |args: &[narsil_jsvm::Wert]| {
        use narsil_css::cssom::regeln::{Fehler, Ort, regel_lesen_mit};
        let text = args.first().map(narsil_jsvm::Wert::als_text).unwrap_or_default();
        let ns = namensraeume_aus(args.get(1));
        let ort = match args.get(2).map(narsil_jsvm::Wert::als_text).as_deref() {
            Some("gruppe") => Ort::Gruppe,
            Some("verschachtelt" | "verschachtelt_stil") => Ort::Verschachtelt,
            Some("schluesselbilder") => Ort::Schluesselbilder,
            _ => Ort::Blatt,
        };
        let unter_stil = args.get(2).map(narsil_jsvm::Wert::als_text).as_deref() == Some("verschachtelt_stil");
        let ja = |i: usize| matches!(args.get(i), Some(narsil_jsvm::Wert::Wahr(true)));
        Ok(match regel_lesen_mit(&text, &ns, ort, ja(3), ja(4), unter_stil) {
            Ok(r) => regel_wert(&r),
            Err(Fehler::Syntax) => narsil_jsvm::Wert::Text(String::from("SyntaxError")),
            Err(Fehler::Hierarchie) => narsil_jsvm::Wert::Text(String::from("HierarchyRequestError")),
        })
    });
    engine.host_funktion("__narsilMedien", |args: &[narsil_jsvm::Wert]| {
        let text = args.first().map(narsil_jsvm::Wert::als_text).unwrap_or_default();
        Ok(narsil_jsvm::Wert::Feld(narsil_css::cssom::medien::liste(&text).into_iter().map(narsil_jsvm::Wert::Text).collect()))
    });
    engine.host_funktion("__narsilSchluessel", |args: &[narsil_jsvm::Wert]| {
        let text = args.first().map(narsil_jsvm::Wert::als_text).unwrap_or_default();
        Ok(narsil_css::cssom::regeln::schluessel_text(&text).map_or(narsil_jsvm::Wert::Null, narsil_jsvm::Wert::Text))
    });
    engine.host_funktion("__narsilStil", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let Some(id) = args.first().and_then(narsil_jsvm::host_id) else {
                return Ok(narsil_jsvm::Wert::Null);
            };
            if crate::zwangslayout::aus("NARSIL_STIL_AUS") {
                return Ok(narsil_jsvm::Wert::Null);
            }
            // Release 12.10.2026: Custom Properties behalten ihre Schreibung.
            let roh = args.get(1).map(|a| a.als_text()).unwrap_or_default();
            let name = if roh.starts_with("--") { roh } else { roh.to_ascii_lowercase() };
            let mut guard = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
            Ok(stil_berechnet(&mut guard, id, &name, 0).map_or(narsil_jsvm::Wert::Null, narsil_jsvm::Wert::Text))
        }
    });
    // Release 11.10.2026: `__narsilRollen(y)` — die Seite an `y` rollen
    // (geklemmt an die Inhaltshoehe; CSSOM View §4 „scroll"), Rueckgabe die
    // neue Lage; `__narsilRollenLesen()` die aktuelle. GEMESSEN vorher:
    // `scrollTo`/`scrollIntoView` taten nichts, `scrollY` war immer 0 (WPT
    // cssom-view, Lazy-Bilder: 40 Zeitueberschreitungen).
    engine.host_funktion("__narsilRollen", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let y = match args.first() {
                Some(narsil_jsvm::Wert::Zahl(n)) if n.is_finite() => *n as f32,
                _ => 0.0,
            };
            let mut w = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
            crate::zwangslayout::erzwingen(&mut w);
            let sicht = w.stil_satz.as_ref().map_or(600.0, |s| s.ansicht.1 as f32);
            let unten = w.masse.values().fold(0.0f32, |acc, m| acc.max(m[1] + m[3]).max(m[10]));
            let neu = y.clamp(0.0, (unten - sicht).max(0.0)).round();
            w.rollen_y = neu;
            w.rollen_wunsch = Some(neu);
            Ok(narsil_jsvm::Wert::Zahl(f64::from(neu)))
        }
    });
    // Release 11.10.2026: `__narsilRahmenAnmelden(iframe, dokument)` — ein
    // Iframe-Dokument fuer das Layout anmelden (siehe `zwangslayout`).
    // GEMESSEN vorher (WPT dialog centering.html): in einem Iframe war jedes
    // `getBoundingClientRect` 0 — Iframe-Dokumente wurden nie gelayoutet.
    engine.host_funktion("__narsilRahmenAnmelden", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let (Some(r), Some(d)) = (args.first().and_then(narsil_jsvm::host_id), args.get(1).and_then(narsil_jsvm::host_id)) else {
                return Ok(narsil_jsvm::Wert::Undefiniert);
            };
            let mut w = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
            w.rahmen.retain(|(a, _)| *a != r);
            if w.rahmen.len() < 64 {
                w.rahmen.push((r, d));
            }
            w.layout_veraltet = true;
            Ok(narsil_jsvm::Wert::Undefiniert)
        }
    });
    // Release 12.10.2026: ein `<img>` mit leerem/ungueltigem `src` hat seinen
    // `error` gemeldet — ab jetzt ist es `complete` (HTML §4.8.4.3: Zustand
    // „broken"). GEMESSEN vorher (WPT img-complete-invalid-url): `complete`
    // blieb im `error`-Handler falsch.
    // Release 12.10.2026: der Stand eines externen Blatts (1 = geladen,
    // 2 = gescheitert, 0 = noch unbekannt; siehe `narsil_page::blattschrank`).
    engine.host_funktion("__narsilBlattStand", {
        move |args: &[narsil_jsvm::Wert]| {
            let url = args.first().map(|a| a.als_text()).unwrap_or_default();
            Ok(narsil_jsvm::Wert::Zahl(match narsil_page::blattschrank::stand(&url) {
                Some(narsil_page::blattschrank::Stand::Geladen(_)) => 1.0,
                Some(narsil_page::blattschrank::Stand::Fehler) => 2.0,
                None => 0.0,
            }))
        }
    });
    // Release 12.10.2026: `__narsilFormKodieren(text, acceptCharset)` — ein
    // Name oder Wert eines Formulars nach dem urlencoded-Serialisierer (HTML
    // §4.10.21.6/7, URL §5.2) in der Formularkodierung: das erste bekannte
    // Etikett aus `accept-charset`, sonst die Kodierung des Dokuments.
    // Unkodierbares wird `&#N;` (Fehlermodus „html"), Zeilenenden CRLF.
    // GEMESSEN vorher (WPT encoding/*-encode-form*, 46 Dateien): GET-Formulare
    // kodierten immer UTF-8 (`URLSearchParams`), euc-kr erwartete `%A2%AE`.
    // Release 13.10.2026: `__narsilFeldSauber(el)` — der Wert eines Felds
    // folgt wieder seinem Standardwert (HTML §4.10.5.4 „dirty value flag"
    // false; Schritt der „reset algorithm"). GEMESSEN vorher (WPT
    // reset-form-2): nach `reset()` blieb das Feld schmutzig und folgte
    // einem neuen `defaultValue` nicht.
    engine.host_funktion("__narsilFeldSauber", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let Some(id) = args.first().and_then(narsil_jsvm::host_id) else {
                return Ok(narsil_jsvm::Wert::Undefiniert);
            };
            let mut w = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
            w.feld_werte.remove(&id);
            wert_spiegeln(&mut w, id, None);
            let ord = w.ord.get(&id).copied();
            if let Some(o) = ord {
                w.eingabe_werte.remove(&o);
            }
            let vorgabe = w.lebend.as_ref().and_then(|l| {
                let k = l.knoten(id)?;
                Some(if l.dok.element_name(k) == Some("textarea") {
                    l.dok.text_inhalt(k)
                } else {
                    l.dok.attribut(k, "value").unwrap_or("").to_string()
                })
            });
            if let (Some(o), Some(v)) = (ord, vorgabe) {
                w.befehle.push(Befehl::Attribut { knoten: o as u32, name: "value".into(), wert: v });
            }
            if let Some(l) = w.lebend.as_mut() {
                l.geaendert = true;
            }
            Ok(narsil_jsvm::Wert::Undefiniert)
        }
    });
    engine.host_funktion("__narsilFormKodieren", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let text = args.first().map(|a| a.als_text()).unwrap_or_default();
            let etiketten = args.get(1).map(|a| a.als_text()).unwrap_or_default();
            let k = etiketten
                .split(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C'))
                .filter(|t| !t.is_empty())
                .find_map(narsil_fmt_html::kodierung::kodierung_aus_label);
            let k = match k {
                Some(k) => k,
                None => {
                    let w = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
                    w.kodierung.unwrap_or(narsil_fmt_html::kodierung::Kodierung::Utf8)
                }
            };
            Ok(narsil_jsvm::Wert::Text(form_kodieren(&text, k.ausgabe())))
        }
    });
    // Release 12.10.2026 (Agent C): der Text eines geladenen Link-Blatts aus
    // dem Blattschrank (fuer `link.sheet.cssRules`).
    // Release 12.10.2026: `__narsilPseudoStil(el, 'before'|'after', name)` —
    // der berechnete Wert am Pseudoelement (`lebend::pseudo_berechnet`).
    engine.host_funktion("__narsilPseudoStil", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let Some(id) = args.first().and_then(narsil_jsvm::host_id) else {
                return Ok(narsil_jsvm::Wert::Null);
            };
            let art = args.get(1).map(narsil_jsvm::Wert::als_text).unwrap_or_default();
            let name = args.get(2).map(narsil_jsvm::Wert::als_text).unwrap_or_default();
            let mut w = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
            if w.stil_veraltet {
                w.stil_veraltet = false;
                blaetter_neu(&mut w);
            }
            if matches!(name.as_str(), "width" | "height") {
                crate::zwangslayout::erzwingen(&mut w);
            }
            let w = &mut *w;
            let (Some(l), Some(satz)) = (w.lebend.as_ref(), w.stil_satz.as_ref()) else {
                return Ok(narsil_jsvm::Wert::Null);
            };
            let Some(k) = l.knoten(id) else { return Ok(narsil_jsvm::Wert::Null) };
            let mut r = crate::lebend::pseudo_berechnet(l, k, &art, &name, satz, &mut w.stil_cache);
            // Breite/Hoehe in Prozent: gegen den Inhaltskasten des naechsten
            // Kastens (das Element, bei `display: contents` ein Vorfahr).
            if matches!(name.as_str(), "width" | "height")
                && let Some(p) = r.as_deref().and_then(|v| v.strip_suffix('%')).and_then(|v| v.trim().parse::<f64>().ok())
            {
                let breit = name == "width";
                let mut kandidaten: Vec<u32> = vec![id];
                kandidaten.extend(l.vorfahren_hosts(id));
                let mut basis: Option<f64> = None;
                for h in kandidaten {
                    let Some(e) = l.knoten(h).filter(|e| l.dok.ist_element_knoten(*e)) else { continue };
                    let Some(be) = crate::lebend::berechnet(l, e, satz, &mut w.stil_cache) else { break };
                    if be.anzeige == narsil_css::Anzeige::Keins {
                        break;
                    }
                    if crate::lebend::stil_spez(l, e, "display").is_some_and(|d| d.trim() == "contents")
                        || narsil_css::berechnet_css::berechneter_wert(&be, "display").is_some_and(|d| d == "contents")
                    {
                        continue;
                    }
                    if let Some(m) = w.masse.get(&h) {
                        basis = Some(f64::from(if breit {
                            m[2] - m[4] - m[6] - be.fuellung_links - be.fuellung_rechts
                        } else {
                            m[3] - m[5] - m[7] - be.fuellung_oben - be.fuellung_unten
                        }));
                    }
                    break;
                }
                if let Some(bs) = basis {
                    r = Some(format!("{}px", narsil_css::berechnet_css::zahl((bs * p / 100.0).max(0.0) as f32)));
                }
            }
            Ok(r.map_or(narsil_jsvm::Wert::Null, narsil_jsvm::Wert::Text))
        }
    });
    engine.host_funktion("__narsilBlattText", {
        move |args: &[narsil_jsvm::Wert]| {
            let url = args.first().map(|a| a.als_text()).unwrap_or_default();
            Ok(narsil_page::blattschrank::inhalt(&url).map_or(narsil_jsvm::Wert::Null, |t| narsil_jsvm::Wert::Text(t.to_string())))
        }
    });
    engine.host_funktion("__narsilBildKaputt", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let Some(id) = args.first().and_then(narsil_jsvm::host_id) else {
                return Ok(narsil_jsvm::Wert::Undefiniert);
            };
            let mut w = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
            let src = w
                .lebend
                .as_ref()
                .and_then(|l| l.knoten(id).map(|k| l.dok.attribut(k, "src").unwrap_or("").trim().to_string()));
            if let Some(src) = src {
                let quelle = narsil_url::loese_auf(&w.adresse, &src).unwrap_or_else(|_| src.clone());
                w.bild_gemeldet.insert(id, (quelle, 0, 0));
            }
            Ok(narsil_jsvm::Wert::Undefiniert)
        }
    });
    engine.host_funktion("__narsilRollenLesen", {
        let state = stand.clone();
        move |_args: &[narsil_jsvm::Wert]| {
            let w = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
            Ok(narsil_jsvm::Wert::Zahl(f64::from(w.rollen_y)))
        }
    });
    engine.host_funktion("__narsilMass", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let Some(id) = args.first().and_then(narsil_jsvm::host_id) else {
                return Ok(narsil_jsvm::Wert::Null);
            };
            let mut w = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
            // Release 10.10.2026: geaendertes DOM -> das Layout jetzt rechnen
            // (siehe `zwangslayout`).
            crate::zwangslayout::erzwingen(&mut w);
            // Noch nie gelayoutet (gerade erst gebaut): die Breite des naechsten
            // gelayouteten Vorfahren, Hoehe 0 — ein Block fuellt seinen
            // Elternkasten. Chrome layoutet fuer die Abfrage synchron; das
            // waere hier ein ganzer Seitenneubau. GEMESSEN: youtube.com las
            // `clientWidth` des Gitters direkt nach dem Bauen und stempelte
            // bei 0 keinen einzigen Eintrag.
            // Release 11.10.2026: ein Element, das selbst oder ueber einen
            // Vorfahren `display: none` ist, hat KEINEN Kasten (CSSOM View
            // §6: `offsetWidth` 0, `offsetParent` null) — nicht schaetzen.
            // GEMESSEN vorher: `offsetWidth` eines versteckten `<p>` war die
            // Breite des Elternkastens (784; WPT `:lang()`-Tests, 36 Dateien).
            if !w.masse.contains_key(&id) {
                let ww = &mut *w;
                if let (Some(l), Some(satz)) = (ww.lebend.as_ref(), ww.stil_satz.as_ref()) {
                    let mut kette = vec![id];
                    kette.extend(l.vorfahren_hosts(id));
                    for h in kette {
                        let Some(k) = l.knoten(h) else { continue };
                        if !l.dok.ist_element_knoten(k) {
                            continue;
                        }
                        if crate::lebend::berechnet(l, k, satz, &mut ww.stil_cache)
                            .is_some_and(|b| b.anzeige == narsil_css::Anzeige::Keins)
                        {
                            return Ok(narsil_jsvm::Wert::Null);
                        }
                    }
                }
            }
            let geschaetzt = if w.masse.contains_key(&id) {
                None
            } else {
                w.lebend.as_ref().and_then(|l| {
                    l.vorfahren_hosts(id).into_iter().find_map(|h| w.masse.get(&h).copied())
                }).map(|m| [m[0], m[1], m[2], 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, m[0] + m[2], m[1]])
            };
            // Release 10.10.2026: der sechste Wert sagt, ob geschaetzt wurde —
            // der IntersectionObserver wartet dann auf das echte Layout.
            let ist_geschaetzt = geschaetzt.is_some();
            Ok(match w.masse.get(&id).copied().or(geschaetzt).as_ref() {
                Some(m) => narsil_jsvm::Wert::Feld(vec![
                    narsil_jsvm::Wert::Zahl(m[0] as f64),
                    narsil_jsvm::Wert::Zahl(m[1] as f64),
                    narsil_jsvm::Wert::Zahl(m[2] as f64),
                    narsil_jsvm::Wert::Zahl(m[3] as f64),
                    narsil_jsvm::Wert::Zahl(if w.rahmen_hosts.contains(&id) { 0.0 } else { w.rollen_y as f64 }),
                    narsil_jsvm::Wert::Wahr(ist_geschaetzt),
                    // Release 11.10.2026: Rahmenbreiten (links, oben, rechts,
                    // unten), Positionsart (0 static, 1 relative, 2 absolute,
                    // 3 fixed, 4 sticky) und die Ausdehnung des Teilbaums
                    // (rechte und untere Kante) — fuer `offsetParent`,
                    // `client*` und `scroll*` (CSSOM View §7).
                    narsil_jsvm::Wert::Zahl(m[4] as f64),
                    narsil_jsvm::Wert::Zahl(m[5] as f64),
                    narsil_jsvm::Wert::Zahl(m[6] as f64),
                    narsil_jsvm::Wert::Zahl(m[7] as f64),
                    narsil_jsvm::Wert::Zahl(m[8] as f64),
                    narsil_jsvm::Wert::Zahl(m[9] as f64),
                    narsil_jsvm::Wert::Zahl(m[10] as f64),
                ]),
                None => narsil_jsvm::Wert::Null,
            })
        }
    });
    // Release 09.10.2026: `__narsilMoAn(an)` — das Aenderungsprotokoll des
    // lebenden DOM ein-/ausschalten (`MutationObserver` im Vorspann).
    engine.host_funktion("__narsilMoAn", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let an = matches!(args.first(), Some(narsil_jsvm::Wert::Wahr(true)));
            let mut w = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
            if let Some(l) = w.lebend.as_mut() {
                if an {
                    if l.dok.journal.is_none() {
                        l.dok.journal = Some(Vec::new());
                    }
                } else {
                    l.dok.journal = None;
                }
            }
            Ok(narsil_jsvm::Wert::Undefiniert)
        }
    });
    // Release 12.10.2026: Rechtecke von Text und Bereichen aus dem Layout
    // (CSSOM View §6.4), siehe `bereich::kaesten`.
    engine.host_funktion("__narsilTextKaesten", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let mut w = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
            crate::bereich::kaesten(&mut w, args, false)
        }
    });
    engine.host_funktion("__narsilStelleAnPunkt", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let mut w = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
            crate::bereich::stelle_an_punkt(&mut w, args)
        }
    });
    engine.host_funktion("__narsilGezeichnet", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let mut w = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
            crate::bereich::gezeichnet(&mut w, args)
        }
    });
    engine.host_funktion("__narsilBereichKaesten", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let mut w = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
            crate::bereich::kaesten(&mut w, args, true)
        }
    });
    // Release 12.10.2026: `__narsilBereich(op, …)` — lebende Bereiche
    // (`Range`, DOM §5) im lebenden DOM, siehe `bereich.rs`.
    engine.host_funktion("__narsilBereich", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let mut w = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
            match w.lebend.as_mut() {
                Some(l) => crate::bereich::rufen(l, args),
                None => Ok(narsil_jsvm::Wert::Null),
            }
        }
    });
    // Release 11.10.2026: `document.currentScript` fuer ein Skript, das der
    // Vorspann beim Einfuegen ausfuehrt (`__narsilSkript`).
    engine.host_funktion("__narsilAktuellesSkript", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let id = args.first().and_then(narsil_jsvm::host_id);
            state.lock().map_err(|_| "DomStand vergiftet".to_string())?.aktuelles_skript = id;
            Ok(narsil_jsvm::Wert::Undefiniert)
        }
    });
    engine.host_funktion("__narsilLauscher", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            zaehle()?;
            let id = args.first().and_then(narsil_jsvm::host_id).unwrap_or(WINDOW_HOST_ID);
            let typ = args.get(1).map_or(String::new(), |a| a.als_text());
            let capture = matches!(args.get(2), Some(narsil_jsvm::Wert::Wahr(true)));
            let mut w = state.lock().map_err(|_| "DomStand vergiftet".to_string())?;
            let mut raus = Vec::new();
            for lid in w.event_listeners.snapshot(id, &typ) {
                if !w.event_listeners.get(lid).is_some_and(|l| l.options.capture == capture) {
                    continue;
                }
                if let Some(eintrag) = w.event_listeners.prepare(lid)
                    && let crate::handler::Handler::Funktion(f) = eintrag.callback
                {
                    // Release 2026-10-15: a passive listener comes as
                    // `{f, passive: true}` (the prelude sets the "in passive
                    // listener" flag around it, DOM §2.10 inner invoke).
                    if eintrag.options.passive {
                        raus.push(narsil_jsvm::Wert::Gegenstand(vec![
                            ("f".to_string(), narsil_jsvm::Wert::Funktion(f)),
                            ("passive".to_string(), narsil_jsvm::Wert::Wahr(true)),
                        ]));
                    } else {
                        raus.push(narsil_jsvm::Wert::Funktion(f));
                    }
                }
            }
            // `onklick = fn` (Eigenschafts-Handler) laeuft nach den
            // Anmeldungen, nicht in der Capture-Phase.
            // Release 11.10.2026: ein `on*`-ATTRIBUT (Anweisungsblock) kommt
            // als Quelltext; der Vorspann uebersetzt ihn (HTML §8.1.8.1).
            // GEMESSEN vorher: `<source onerror="a++">` lief bei einem aus
            // dem Skript gesendeten `error` nie (WPT media-elements).
            if !capture {
                // Release 11.10.2026: im lebenden DOM gilt das AKTUELLE
                // Attribut (auch nach `setAttribute`, `cloneNode`, `innerHTML`).
                // GEMESSEN vorher (WPT Event-dispatch-single-activation-
                // behavior 11/132): ein so gesetztes `oninput` lief nie.
                let element = id != WINDOW_HOST_ID && id != w.document_id;
                let attr_text = if element {
                    w.lebend.as_ref().and_then(|l| {
                        let k = l.knoten(id)?;
                        l.dok.attribut(k, &format!("on{}", typ.to_ascii_lowercase())).map(str::to_string)
                    })
                } else {
                    None
                };
                let lebend_da = element && w.lebend.is_some();
                match w.horcher.get(&(id, typ.clone())) {
                    Some(crate::handler::Handler::Funktion(f)) => raus.push(narsil_jsvm::Wert::Funktion(*f)),
                    Some(crate::handler::Handler::Anweisung(q)) => {
                        if lebend_da {
                            if let Some(t) = attr_text {
                                raus.push(narsil_jsvm::Wert::Text(t));
                            }
                        } else {
                            raus.push(narsil_jsvm::Wert::Text(q.clone()));
                        }
                    }
                    _ => {
                        if let Some(t) = attr_text {
                            raus.push(narsil_jsvm::Wert::Text(t));
                        }
                    }
                }
            }
            Ok(narsil_jsvm::Wert::Feld(raus))
        }
    });
    // 06.10.2026: `alert` — eine Host-Meldung, kein Dialog.
    //
    // GEMESSEN am Test `js_sripte::alert_erscheint_als_eigene_meldung`:
    // die Seiten-Engine band `alert` nicht; das Skript brach mit
    // `ReferenceError: 'alert' ist nicht definiert` ab. Die Meldung landet
    // im Stand und wird in die Konsole geleert — genau einmal, mit Namen.
    engine.host_funktion("alert", {
        let state = stand.clone();
        move |args: &[narsil_jsvm::Wert]| {
            let text = args
                .iter()
                .map(|a| a.als_text())
                .collect::<Vec<_>>()
                .join(" ");
            let mut w = state.lock().expect("DomStand");
            if w.meldungen.len() < 64 {
                w.meldungen.push(format!("alert: {text}"));
            }
            Ok(narsil_jsvm::Wert::Undefiniert)
        }
    });
    // Und dieselbe Form fuer `confirm`/`prompt`: antworten, nicht werfen.
    engine.host_funktion("confirm", {
        let state = stand.clone();
        move |_args: &[narsil_jsvm::Wert]| {
            state.lock().expect("DomStand").meldungen.push("confirm".to_string());
            Ok(narsil_jsvm::Wert::Wahr(true))
        }
    });
    // W6D: `ev.preventDefault()` — eine EIGENE Host-Funktion.
    //
    // GEMESSEN, warum sie im DOM-Haken und nicht in `js.rs` (14:55): sie
    // schreibt in den DOM-Stand, und der Stand ist die einzige Stelle,
    // die der Hauptthread ohne die Engine lesen kann. Eine Engine-Meldung
    // waere hier ein Umweg ueber drei Waende.
    engine.host_funktion(narsil_jsvm::PREVENT_DEFAULT, {
        let stand = stand.clone();
        move |_args: &[narsil_jsvm::Wert]| {
            let mut state = stand.lock().expect("DomStand");
            // Ohne laufenden Verteiler (`None`) ist ein Ereignis cancelable
            // — `new MouseEvent('click')` ist es in jedem Browser.
            if state.event_cancelable.unwrap_or(true) && !state.passive_listener {
                state.default_verhindert = true;
            }
            Ok(narsil_jsvm::Wert::Undefiniert)
        }
    });
    engine.host_funktion("__stopPropagation", {
        let stand = stand.clone();
        move |_args: &[narsil_jsvm::Wert]| {
            stand.lock().expect("DomStand").propagation_stopped = true;
            Ok(narsil_jsvm::Wert::Undefiniert)
        }
    });
    engine.host_funktion("__stopImmediatePropagation", {
        let stand = stand.clone();
        move |_args: &[narsil_jsvm::Wert]| {
            let mut state = stand.lock().expect("DomStand");
            state.propagation_stopped = true;
            state.immediate_stopped = true;
            Ok(narsil_jsvm::Wert::Undefiniert)
        }
    });
    engine.host_objekt(std::sync::Arc::new(BrowserHaken(stand)));

}

/// Release 11.10.2026: Namen, die bei `audio`/`video`/`track` das
/// Medienmodell im Vorspann beantwortet (HTML §4.8.11) und nicht der
/// Attribut-Spiegel: `muted` ist dort KEIN gespiegeltes Attribut (das ist
/// `defaultMuted`), `readyState` ein Medienzustand, `preload` ein
/// Aufzaehlungswert, und `src` setzen startet den Ladeablauf (gelesen
/// bleibt `src` beim Host, der die Adresse aufloest). GEMESSEN vorher:
/// `video.src = …` schrieb nur das Attribut, kein `loadstart` folgte.
fn medien_name(w: &Stand, id: u32, name: &str, schreiben: bool) -> bool {
    // Release 11.10.2026: `select.value`/`type` und `option.selected` sind
    // Auswahlzustand, kein Attribut (HTML §4.10.7) — das Modell steht im
    // Vorspann. GEMESSEN vorher: `select.value` war der Text ALLER Optionen,
    // `option.selected` nur das Attribut.
    if matches!(name, "value" | "type" | "selected" | "length" | "max" | "min" | "low" | "high" | "optimum") {
        let tag = w
            .lebend
            .as_ref()
            .and_then(|l| l.knoten(id).and_then(|k| l.dok.element_name(k).map(str::to_string)));
        return match tag.as_deref() {
            Some("select") => matches!(name, "value" | "type" | "length"),
            Some("option") => name == "selected",
            // Release 11.10.2026: `progress`/`meter` rechnet der Vorspann
            // (HTML §4.10.13/14: Zahlen, geklemmt) — nicht der Rohtext.
            Some("progress") => matches!(name, "value" | "max"),
            Some("meter") => matches!(name, "value" | "max" | "min" | "low" | "high" | "optimum"),
            // Release 14.10.2026: `output.value`/`type` and `fieldset.type`
            // come from the preamble (HTML §4.10.12, §4.10.15). MEASURED
            // before (WPT output.html): `output.type` read the empty attribute.
            Some("output") => matches!(name, "value" | "type"),
            Some("fieldset") => name == "type",
            _ => false,
        };
    }
    if !matches!(name, "src" | "muted" | "readyState" | "preload" | "crossOrigin") {
        return false;
    }
    let tag = w
        .lebend
        .as_ref()
        .and_then(|l| l.knoten(id).and_then(|k| l.dok.element_name(k).map(str::to_string)))
        .or_else(|| w.tag.get(&id).cloned());
    match tag.as_deref() {
        Some("audio" | "video") => schreiben || name != "src",
        Some("track") => name == "readyState",
        _ => false,
    }
}

struct BrowserHaken(Geteilt);
impl narsil_jsvm::HostObjektHaken for BrowserHaken {
    /// Release 2026-10-15: the live DOM's tree and attribute versions (see
    /// `narsil_fmt_html::dom::Version`) as one stamp for the engine's
    /// collection cache; 0 without a live DOM.
    fn sammlung_stand(&self) -> u64 {
        let w = self.0.lock().expect("DomStand");
        // Never 0 (that means "unknown"); every bit of both versions counts.
        w.lebend.as_ref().map_or(0, |l| (l.dok.tree_version.0.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ l.dok.attr_version.0).max(1))
    }
    fn verwaltet(&self, id: u32, art: u8, name: &str) -> bool {
        // Release 2026-10-15: live collections (art 5/6) take no writes; the
        // engine keeps expandos and refuses indices and names.
        if (5..=8).contains(&art) {
            return false;
        }
        // Release 2026-10-14: in XML documents the prelude sets
        // `innerHTML`/`outerHTML` (XML fragment parser, DOM Parsing §2.2).
        if art == 0
            && matches!(name, "innerHTML" | "outerHTML")
            && self.0.lock().expect("DomStand").lebend.as_mut().is_some_and(|l| xml_element(l, id))
        {
            return false;
        }
        // Release 13.10.2026: am FENSTER beansprucht der Host beim Schreiben
        // nur die Ereignis-Handler; `window.target = el`, `window.id = …` sind
        // gewoehnliche Eigenschaften (globale Variablen). GEMESSEN vorher (WPT
        // intersection-observer svg-*): `window.target = rect` legte den Text
        // "[object …]" ab, `target.getBoundingClientRect` war kein Rueckruf.
        if art == 0 && id == WINDOW_HOST_ID && !name.starts_with("on") {
            return false;
        }
        if art == 0 && medien_name(&self.0.lock().expect("DomStand"), id, name, true) {
            return false;
        }
        if art == 0 && name == "cookie" {
            return id == self.0.lock().expect("DomStand").document_id;
        }
        // Release 2026-10-15: writing `innerText` of an element is the
        // prelude's (text runs and <br>s; SVG/MathML have no `innerText`).
        if art == 0 && name == "innerText" {
            let w = self.0.lock().expect("DomStand");
            if w.lebend.as_ref().is_some_and(|l| l.knoten(id).is_some_and(|k| l.dok.ist_element_knoten(k))) {
                return false;
            }
        }
        // Release 2026-10-15: `value` of elements other than input,
        // textarea and option, and `data` of elements, belong to the
        // prelude's reflection (`object.data` is a URL).
        if art == 0 && matches!(name, "value" | "data") {
            let w = self.0.lock().expect("DomStand");
            if let Some(l) = w.lebend.as_ref()
                && let Some(k) = l.knoten(id)
                && l.dok.ist_element_knoten(k)
                && (name == "data" || !matches!(l.dok.element_name(k), Some("input" | "textarea" | "option")))
            {
                return false;
            }
        }
        // Release 2026-10-15: `htmlFor`/`sizes` only where the element's
        // interface has them as strings (`lebend::reflection_applies`).
        if art == 0 && matches!(name, "htmlFor" | "sizes" | "name") {
            let w = self.0.lock().expect("DomStand");
            if w.lebend.as_ref().is_some_and(|l| l.knoten(id).is_some_and(|k| !crate::lebend::reflection_applies(&l.dok, k, name))) {
                return false;
            }
        }
        // Release 12.10.2026: `style.disabled` gehoert dem Vorspann (Blatt).
        if art == 0 && name == "disabled" {
            let w = self.0.lock().expect("DomStand");
            if w.lebend.as_ref().is_some_and(|l| l.knoten(id).is_some_and(|k| l.dok.element_name(k).is_some_and(|n| n.eq_ignore_ascii_case("style")))) {
                return false;
            }
        }
        art == 1 || art == 3 || (art == 2 && name == "value") || name.starts_with("on")
            || (art == 0
                && (crate::lebend::ist_schreibbar_basis(name)
                    || (crate::lebend::ist_schreibbar(name)
                        && self.0.lock().expect("DomStand").lebend.is_some())))
            || matches!(name,
            "textContent" | "innerText" | "innerHTML" | "value" | "id" | "className" |
            "namespaceURI" | "localName" | "prefix" | "readyState")
    }
    fn schreibbar(&self, _id: u32, art: u8, name: &str) -> bool {
        art != 0 || !matches!(name, "namespaceURI" | "localName" | "prefix" | "readyState")
    }
    fn lesen(&self, id: u32, name: &str) -> Result<Option<narsil_jsvm::Wert>, String> {
        self.lesen_mit_art(id, 0, name)
    }
    fn schreiben(&self, id: u32, name: &str, wert: &narsil_jsvm::Wert) -> Result<(), String> {
        self.schreiben_mit_art(id, 0, name, wert)
    }
    fn lesen_mit_art(&self, id: u32, art: u8, name: &str) -> Result<Option<narsil_jsvm::Wert>, String> {
        zaehle()?;
        {
            let mut w = self.0.lock().expect("DomStand");
            if art == 0 && medien_name(&w, id, name, false) {
                return Ok(None);
            }
            // Release 2026-10-15: no map write when the kind is unchanged
            // (every property read passes here; live collections read
            // `length` and an index per loop step).
            if w.art.get(&id) != Some(&art) {
                w.art.insert(id, art);
            }
        }
        lesen(&self.0, id, name)
    }
    fn schreiben_mit_art(&self, id: u32, art: u8, name: &str, wert: &narsil_jsvm::Wert) -> Result<(), String> {
        zaehle()?;
        self.0.lock().expect("DomStand").art.insert(id, art);
        schreiben(&self.0, id, name, wert)
    }
    fn rufen(&self, id: u32, name: &str, args: &[narsil_jsvm::Wert]) -> Result<narsil_jsvm::Wert, String> {
        zaehle()?; rufen(&self.0, id, name, args)
    }
    fn rufen_mit_art(&self, id: u32, art: u8, name: &str, args: &[narsil_jsvm::Wert]) -> Result<narsil_jsvm::Wert, String> {
        zaehle()?;
        self.0.lock().expect("DomStand").art.insert(id, art);
        rufen(&self.0, id, name, args)
    }
    fn reaktionen(&self) -> Vec<narsil_jsvm::Wert> {
        reaktionen_abholen(&self.0)
    }
}

/// Release 09.10.2026: der Proxy-Stand folgt dem lebenden DOM (Klassen, Id,
/// Attribute eines Elements). Sonst saehe der alte Weg alte Werte, und
/// `Lebend::stand_uebernehmen` schriebe am Ende die alten Klassen zurueck.
/// `geaendert` ist ein Attribut, das gerade entfernt worden sein kann.
fn spiegeln(w: &mut Stand, id: u32, geaendert: Option<&str>) {
    let Some(l) = w.lebend.as_ref() else { return };
    let Some(k) = l.knoten(id) else { return };
    if !l.dok.ist_element_knoten(k) {
        return;
    }
    let attrs = l.dok.attribute_von(k);
    let wert = |n: &str| attrs.iter().find(|(a, _)| a == n).map(|(_, v)| v.clone());
    match wert("class") {
        Some(c) => {
            w.klassen.insert(id, c.split_ascii_whitespace().map(str::to_string).collect());
        }
        None => {
            if w.klassen.contains_key(&id) {
                w.klassen.insert(id, Vec::new());
            }
        }
    }
    if let Some(o) = w.ord.get(&id).copied() {
        match wert("id") {
            Some(v) => {
                w.dom_ids.insert(o, v);
            }
            None => {
                w.dom_ids.remove(&o);
            }
        }
    }
    if geaendert == Some("style") {
        w.stil.remove(&id);
    }
    let karte = w.attribute.entry(id).or_default();
    if let Some(g) = geaendert
        && !attrs.iter().any(|(a, _)| a == g)
    {
        karte.remove(g);
    }
    for (n, v) in attrs {
        karte.insert(n, v);
    }
    // HTML §4.12.5: setting (or removing) width/height resets the bitmap.
    if matches!(geaendert, Some("width" | "height")) && crate::canvas::element_tag(w, id).as_deref() == Some("canvas") {
        let _ = canvas_reset(w, id);
    }
}

/// Release 09.10.2026: wartende Custom-Element-Reaktionen als JS-Werte
/// (`[art, knoten, name, alt, neu]`, siehe `vorspann.js`).
fn reaktionen_abholen(stand: &Geteilt) -> Vec<narsil_jsvm::Wert> {
    use narsil_jsvm::Wert;
    let mut w = stand.lock().expect("DomStand");
    let Some(l) = w.lebend.as_mut() else { return Vec::new() };
    let journal_leer = l.dok.journal.as_ref().is_none_or(|j| j.is_empty());
    if l.ce_schlange.is_empty() && journal_leer {
        return Vec::new();
    }
    let text = |t: Option<String>| t.map_or(Wert::Null, Wert::Text);
    let mut raus: Vec<Wert> = std::mem::take(&mut l.ce_schlange)
        .into_iter()
        .map(|r| {
            Wert::Feld(match r {
                crate::lebend::CeReaktion::Verbunden(h) => vec![Wert::Zahl(1.0), host_wert(h)],
                crate::lebend::CeReaktion::Getrennt(h) => vec![Wert::Zahl(2.0), host_wert(h)],
                crate::lebend::CeReaktion::Upgrade(h) => vec![Wert::Zahl(3.0), host_wert(h)],
                crate::lebend::CeReaktion::Attribut { host, name, alt, neu, raum } => {
                    vec![Wert::Zahl(4.0), host_wert(host), Wert::Text(name), text(alt), text(neu), text(raum)]
                }
                crate::lebend::CeReaktion::Adoptiert { host, alt, neu } => vec![Wert::Zahl(7.0), host_wert(host), host_wert(alt), host_wert(neu)],
                crate::lebend::CeReaktion::Formular(h) => vec![Wert::Zahl(8.0), host_wert(h)],
                crate::lebend::CeReaktion::Klick(h) => vec![Wert::Zahl(6.0), host_wert(h)],
                crate::lebend::CeReaktion::Medien { host, art } => vec![Wert::Zahl(9.0), host_wert(host), Wert::Zahl(f64::from(art))],
                crate::lebend::CeReaktion::Skript(h) => vec![Wert::Zahl(10.0), host_wert(h)],
                crate::lebend::CeReaktion::SlotLauscher(h) => vec![Wert::Zahl(11.0), host_wert(h)],
            })
        })
        .collect();
    // Release 09.10.2026: das Aenderungsprotokoll fuer `MutationObserver`
    // (`[5, typ, ziel, knoten, hinzu, vorher, nachher, name, alt]`, siehe
    // `vorspann.js`). Nur gefuellt, solange ein Beobachter angemeldet ist.
    if !journal_leer {
        let eintraege = l.dok.journal.as_mut().map(std::mem::take).unwrap_or_default();
        for a in eintraege {
            use narsil_fmt_html::dom::Aenderung;
            let satz = match a {
                Aenderung::Kinder { eltern, knoten, hinzu, vorher, nachher } => vec![
                    Wert::Zahl(5.0),
                    Wert::Text("childList".into()),
                    l.wert(Some(eltern)),
                    l.wert(Some(knoten)),
                    Wert::Wahr(hinzu),
                    l.wert(vorher),
                    l.wert(nachher),
                    Wert::Null,
                    Wert::Null,
                ],
                // Release 12.10.2026: ein gesammelter Satz (`addedNodes` und
                // `removedNodes` als Listen, siehe `__narsilMoSatz`).
                Aenderung::KinderSatz { eltern, hinzu, weg, vorher, nachher } => {
                    let hinzu = Wert::Feld(hinzu.into_iter().map(|k| l.wert(Some(k))).collect());
                    let weg = Wert::Feld(weg.into_iter().map(|k| l.wert(Some(k))).collect());
                    vec![
                        Wert::Zahl(5.0),
                        Wert::Text("childList".into()),
                        l.wert(Some(eltern)),
                        hinzu,
                        weg,
                        l.wert(vorher),
                        l.wert(nachher),
                        Wert::Null,
                        Wert::Null,
                    ]
                }
                Aenderung::Attribut { knoten, name, alt, ns } => vec![
                    Wert::Zahl(5.0),
                    Wert::Text("attributes".into()),
                    l.wert(Some(knoten)),
                    Wert::Null,
                    Wert::Wahr(false),
                    Wert::Null,
                    Wert::Null,
                    Wert::Text(name),
                    text(alt),
                    text(ns),
                ],
                Aenderung::Daten { knoten, alt } => vec![
                    Wert::Zahl(5.0),
                    Wert::Text("characterData".into()),
                    l.wert(Some(knoten)),
                    Wert::Null,
                    Wert::Wahr(false),
                    Wert::Null,
                    Wert::Null,
                    Wert::Null,
                    Wert::Text(alt),
                ],
            };
            raus.push(Wert::Feld(satz));
        }
    }
    raus
}

/// Release 10.10.2026: ein Teil einer absoluten URL wie bei `URL`/`<a>`
/// (WHATWG URL §6.1: `protocol` mit `:`, `search` mit `?`, `hash` mit `#`,
/// beide leer, wenn nur das Zeichen steht; `origin` nur fuer Schemata mit
/// Host, sonst `"null"`).
/// Release 11.10.2026: eine Adresse aus einem Attribut aufloesen — die
/// Nicht-ASCII-Zeichen der Abfrage in der Kodierung des Dokuments (URL §4.4
/// „query state": percent-encode after encoding; der Rest bleibt dem
/// URL-Parser). GEMESSEN vorher (WPT `*-encode-href*`, ~150 000 Teiltests):
/// eine euc-jp-Seite bekam `?%E4%B8%82` statt `?%26%2319970%3B`.
pub(crate) fn adresse_aufloesen(basis: &str, kodierung: Option<narsil_fmt_html::kodierung::Kodierung>, roh: &str) -> String {
    // HTML: nur ASCII-Leerraum an den Raendern (Rusts `trim` naehme auch
    // U+0085 und U+00A0 mit — WPT `*-encode-href-errors-misc`).
    let roh = roh.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C'));
    let neu = abfrage_in_kodierung(roh, kodierung);
    let roh = neu.as_deref().unwrap_or(roh);
    narsil_url::loese_auf(basis, roh).unwrap_or_else(|_| roh.to_string())
}

/// Release 12.10.2026: der urlencoded-Serialisierer fuer EINEN Namen oder
/// Wert (siehe `__narsilFormKodieren`).
pub(crate) fn form_kodieren(text: &str, k: narsil_fmt_html::kodierung::Kodierung) -> String {
    use narsil_fmt_html::kodierung::Kodiert;
    // HTML „converting an entry list to a list of name-value pairs": jedes
    // einzelne CR und jedes einzelne LF wird CRLF.
    let mut normal = String::with_capacity(text.len());
    let mut zeichen = text.chars().peekable();
    while let Some(c) = zeichen.next() {
        match c {
            '\r' => {
                normal.push_str("\r\n");
                if zeichen.peek() == Some(&'\n') {
                    zeichen.next();
                }
            }
            '\n' => normal.push_str("\r\n"),
            c => normal.push(c),
        }
    }
    let mut raus = String::with_capacity(normal.len() * 3);
    let byte = |b: u8, raus: &mut String| {
        if b == b' ' {
            raus.push('+');
        } else if b.is_ascii_alphanumeric() || matches!(b, b'*' | b'-' | b'.' | b'_') {
            raus.push(b as char);
        } else {
            raus.push_str(&format!("%{b:02X}"));
        }
    };
    if k.ist_utf8() {
        for b in normal.bytes() {
            byte(b, &mut raus);
        }
        return raus;
    }
    for teil in narsil_fmt_html::kodierung::kodiere(&normal, k) {
        match teil {
            Kodiert::Bytes(bs) => {
                for b in bs {
                    byte(b, &mut raus);
                }
            }
            Kodiert::Fehler(c) => {
                for b in format!("&#{};", c as u32).bytes() {
                    byte(b, &mut raus);
                }
            }
        }
    }
    raus
}

fn abfrage_in_kodierung(roh: &str, kodierung: Option<narsil_fmt_html::kodierung::Kodierung>) -> Option<String> {
    let k = kodierung?.ausgabe();
    if k.ist_utf8() {
        return None;
    }
    let frage = roh.find('?')?;
    let ende = roh[frage..].find('#').map_or(roh.len(), |i| frage + i);
    let abfrage = &roh[frage + 1..ende];
    if abfrage.is_ascii() {
        return None;
    }
    let mut neu = String::with_capacity(roh.len() * 2);
    neu.push_str(&roh[..=frage]);
    let mut lauf = String::new();
    for c in abfrage.chars() {
        if c.is_ascii() {
            if !lauf.is_empty() {
                neu.push_str(&narsil_fmt_html::kodierung::abfrage_prozentkodieren(&lauf, k, true));
                lauf.clear();
            }
            neu.push(c);
        } else {
            lauf.push(c);
        }
    }
    if !lauf.is_empty() {
        neu.push_str(&narsil_fmt_html::kodierung::abfrage_prozentkodieren(&lauf, k, true));
    }
    neu.push_str(&roh[ende..]);
    Some(neu)
}

fn url_teil(u: &str, teil: &str) -> String {
    let (schema, rest) = match u.find(':') {
        Some(i) => (&u[..=i], &u[i + 1..]),
        None => ("", u),
    };
    let (vor_anker, anker) = match rest.find('#') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let (vor_frage, frage) = match vor_anker.find('?') {
        Some(i) => (&vor_anker[..i], &vor_anker[i..]),
        None => (vor_anker, ""),
    };
    let (autoritaet, pfad) = match vor_frage.strip_prefix("//") {
        Some(a) => match a.find('/') {
            Some(i) => (&a[..i], &a[i..]),
            None => (a, if a.is_empty() { "" } else { "/" }),
        },
        None => ("", vor_frage),
    };
    let host = autoritaet.rsplit('@').next().unwrap_or("");
    let (hostname, port) = if host.starts_with('[') {
        match host.find(']') {
            Some(i) => (&host[..=i], host[i + 1..].strip_prefix(':').unwrap_or("")),
            None => (host, ""),
        }
    } else {
        match host.rfind(':') {
            Some(i) => (&host[..i], &host[i + 1..]),
            None => (host, ""),
        }
    };
    match teil {
        "protocol" => schema.to_ascii_lowercase(),
        "host" => host.to_string(),
        "hostname" => hostname.to_string(),
        "port" => port.to_string(),
        "pathname" => pfad.to_string(),
        "search" => if frage.len() > 1 { frage.to_string() } else { String::new() },
        "hash" => if anker.len() > 1 { anker.to_string() } else { String::new() },
        "origin" => {
            if host.is_empty() {
                "null".to_string()
            } else {
                format!("{}//{}", schema.to_ascii_lowercase(), host)
            }
        }
        _ => String::new(),
    }
}

/// M3: die Vorgaenge zaehlen.
fn zaehle() -> Result<(), String> {
    if VORGAENGE.fetch_add(1, Ordering::SeqCst) > MAX_VORGAENGE {
        return Err(std::format!(
            "zu viele DOM-Zugriffe ({MAX_VORGAENGE}) — das Skript liest in einer Schleife"
        ));
    }
    Ok(())
}

/// Meldet die Vorgaenge (fuer Tests und Meldungen).
pub fn vorgaenge() -> u32 {
    VORGAENGE.load(Ordering::SeqCst)
}

// ===========================================================================
// GETTER
// ===========================================================================

/// Eine Eigenschaft lesen. `Ok(None)` = „kenne ich nicht".
///
/// DER WEG: `document` (Id 0) und die Unterobjekte (`style`, `classList`)
/// tragen dieselbe Id wie ihr Element — `el.style.color` und
/// `el.classList.add` kommen hier mit der ELEMENT-Id an, und der Name
/// sagt, was gemeint ist.
///
/// WARUM DIE NENNUNG ueber `name` und nicht ueber ein Flag am Objekt: ein
/// Flag waere ein viertes Feld am Host-Objekt, und ein Objekt mit
/// `__host` und `__art` waere eine zweite Wahrheit neben dem Namen.
fn lesen(stand: &Geteilt, id: u32, name: &str) -> Result<Option<narsil_jsvm::Wert>, String> {
    use narsil_jsvm::Wert;
    // Release 2026-10-15: a live collection answers from the live DOM.
    if crate::lebend::is_collection_host(id) {
        let mut w = stand.lock().expect("DomStand");
        return Ok(w.lebend.as_mut().and_then(|l| crate::lebend::collection_lesen(l, id, name)));
    }
    // Release 2026-10-15: `innerText` of an element follows its rendering
    // (`crate::inner_text`).
    if name == "innerText" {
        let mut w = stand.lock().expect("DomStand");
        if let Some(t) = crate::inner_text::inner_text(&mut w, id) {
            return Ok(Some(Wert::Text(t)));
        }
    }
    // METHODEN: `{__host: id, __methode: <ZAHL>}`. Der Getter gibt sie als
    // Objekt zurueck, und der Aufruf-Haken macht daraus einen Aufruf.
    // Release 09.10.2026: `replace`/`item`/`supports` gibt es nur an `classList`.
    // Release 10.10.2026: `item` auch an `style`, `getPropertyValue`/
    // `getPropertyPriority` nur dort.
    let nur_klassenliste = match name {
        "replace" | "supports" => stand.lock().expect("DomStand").art.get(&id).copied().unwrap_or(0) != 2,
        "item" => !matches!(stand.lock().expect("DomStand").art.get(&id).copied().unwrap_or(0), 1 | 2),
        "getPropertyValue" | "getPropertyPriority" => stand.lock().expect("DomStand").art.get(&id).copied().unwrap_or(0) != 1,
        // Release 12.10.2026: an `style` (Art 1) gibt es nur die Methoden der
        // CSSStyleDeclaration — `style.scale`/`rotate`/`translate`/`fill`/
        // `stroke` sind Eigenschaften, keine Canvas-Methoden; an `dataset`
        // (Art 3) gibt es gar keine. GEMESSEN vorher (WPT css-values
        // calc-mix-serialize, sin-cos-tan-serialize): `el.style.scale` war
        // „function scale() { [native code] }".
        _ => match stand.lock().expect("DomStand").art.get(&id).copied().unwrap_or(0) {
            1 => !matches!(name, "setProperty" | "removeProperty"),
            3 => true,
            _ => false,
        },
    };
    // Release 2026-10-15: `matches`/`webkitMatchesSelector`/`closest` are
    // Element methods (DOM §4.9); a document or fragment has none. MEASURED
    // before (WPT dom/nodes/Element-matches): `"matches" in document` was
    // true.
    let element_only_missing = matches!(name, "matches" | "webkitMatchesSelector" | "closest") && {
        let w = stand.lock().expect("DomStand");
        match w.lebend.as_ref().and_then(|l| l.knoten(id).map(|k| l.dok.ist_element_knoten(k))) {
            Some(is_element) => !is_element,
            None => id == w.document_id,
        }
    };
    // Release 2026-10-15: in XML documents the prelude's insertAdjacentHTML
    // (XML fragment parser, DOM Parsing §2.2) answers.
    let element_only_missing = element_only_missing
        || (name == "insertAdjacentHTML" && stand.lock().expect("DomStand").lebend.as_mut().is_some_and(|l| xml_element(l, id)));
    if !nur_klassenliste && !element_only_missing && let Some(nummer) = methode_nummer(name) {
        return Ok(Some(Wert::Gegenstand(vec![
            (MERKE.to_string(), Wert::Zahl(id as f64)),
            (METHODE.to_string(), Wert::Zahl(nummer)),
            ("__art".to_string(), Wert::Zahl(stand.lock().expect("DomStand").art.get(&id).copied().unwrap_or(0) as f64)),
            ("__name".to_string(), Wert::Text(name.to_string())),
        ])));
    }
    let mut w = stand.lock().expect("DomStand");
    // Release 10.10.2026: `el.style.x` aus dem lebenden DOM (dem `style`-Attribut).
    if w.art.get(&id) == Some(&1)
        && let Some(l) = w.lebend.as_mut()
        && l.knoten(id).is_some()
    {
        return Ok(crate::lebend::stil_lesen(l, id, name));
    }
    if w.art.get(&id) == Some(&1) {
        let value = w.stil.get(&id).and_then(|list| list.iter().rev().find(|(key, _)| key == name))
            .map(|(_, v)| v.clone()).unwrap_or_default();
        return Ok(Some(Wert::Text(value)));
    }
    // Release 12.10.2026: das lebende DOM zuerst (siehe unten,
    // `unterobjekt_lesen`). GEMESSEN vorher (WPT encoding/*-decode*.html, 25
    // Dateien): `span.dataset.cp` war in einem DOMParser-/Rahmendokument
    // `undefined` — die alte Tabelle kennt nur die Knoten der Ladeseite.
    if w.art.get(&id) == Some(&3) && !w.lebend.as_ref().is_some_and(|l| l.knoten(id).is_some()) {
        let mut attribute = String::from("data-");
        for c in name.chars() { if c.is_ascii_uppercase() { attribute.push('-'); attribute.push(c.to_ascii_lowercase()); } else { attribute.push(c); } }
        return Ok(w.attribute.get(&id).and_then(|a| a.get(&attribute)).map(|v| Wert::Text(v.clone())));
    }

    // Handler properties also exist on Document and Window. Return the
    // original function handle so reading `onclick` retains its identity.
    if name.starts_with("on") && name.len() > 2 {
        let event_type = name[2..].to_string();
        // Release 11.10.2026: an ELEMENTEN beantwortet der Host nur die per
        // Eigenschaft gesetzte Funktion; ein Handler aus dem Inhaltsattribut
        // (`setAttribute('onclick', …)`) uebersetzt `vorspann.js` (HTML
        // §8.1.8.1 „getting the current value of the event handler").
        // GEMESSEN vorher: `el.onclick` war danach `null`.
        // Release 12.10.2026: an Elementen antwortet IMMER der Vorspann (er
        // merkt sich die gesetzte Funktion) — eine Funktion kommt ueber
        // diesen Weg nicht zurueck (`gval_aus_wert` macht `undefined`
        // daraus). GEMESSEN vorher: `el.onclick = f; el.onclick` war undefined.
        let element = id != WINDOW_HOST_ID && id != w.document_id;
        if element {
            return Ok(None);
        }
        return Ok(match w.horcher.get(&(id, event_type)) {
            Some(crate::handler::Handler::Funktion(index)) => Some(Wert::Funktion(*index)),
            Some(handler) => Some(Wert::Text(handler.als_text())),
            None => Some(Wert::Null),
        });
    }

    if id == WINDOW_HOST_ID {
        return Ok(match name {
            "document" => Some(host_wert(w.document_id)),
            "window" | "self" | "top" | "parent" => Some(host_wert(WINDOW_HOST_ID)),
            // Release 10.10.2026: das GLOBALE `location` (ueber `__global`).
            // GEMESSEN vorher: `window.location` war das Dokument-Objekt mit
            // nur `href` — `window.location.hostname` war leer
            // (bbc.com: „kein Zugriff auf Undefiniert (Eigenschaft split)").
            "location" => None,
            _ => match w.expando.get(&(id, name.to_string())) {
                Some(value) => Some(Wert::Text(value.clone())),
                // Release 10.10.2026: benannter Zugriff (HTML §7.2.2.3) —
                // ein Element mit `id`/`name` ist eine Eigenschaft des
                // Fensters und damit eine globale Variable (der Motor fragt
                // hier, wenn er einen globalen Namen nicht kennt).
                None if !name.starts_with("__") => w
                    .lebend
                    .as_mut()
                    .and_then(|l| crate::lebend::benannt(l, name))
                    .map(host_wert),
                None => None,
            },
        });
    }

    // `document` (Id 0) — die WENIGEN Dinge, die es da gibt.
    // Release 09.10.2026: das lebende DOM zuerst (Struktur, Text, innerHTML,
    // gespiegelte Attribute; `classList`/`dataset` aus den Attributen).
    let art_jetzt = w.art.get(&id).copied().unwrap_or(0);
    // Release 09.10.2026: URL-Attribute spiegeln als ABSOLUTE URL (HTML
    // §2.6.1 „reflect" fuer USVString-URLs): `script.src`, `a.href`,
    // `link.href`, `img.src`, `form.action`, `video.poster` …; fehlt das
    // Attribut, ist es der leere Text.
    //
    // GEMESSEN vorher: `src` kannte der Host gar nicht (`null`), und `href`
    // lieferte fuer JEDES Element die Seitenadresse. youtube.com liest
    // `document.getElementById("base-js").src`, um weitere Module
    // nachzuladen — der Lader brach mit `null.startsWith` ab, und die
    // Startseite blieb beim Skelett.
    if art_jetzt == 0 && matches!(name, "src" | "href" | "action" | "poster") {
        let adresse = w.adresse.clone();
        if let Some(l) = w.lebend.as_ref()
            && let Some(k) = l.knoten(id)
            && let Some(tag) = l.dok.element_name(k)
        {
            let passt = match name {
                "src" => matches!(
                    tag,
                    "img" | "script" | "iframe" | "audio" | "video" | "source" | "embed" | "input" | "track" | "frame"
                ),
                "href" => matches!(tag, "a" | "area" | "link" | "base"),
                "action" => tag == "form",
                _ => tag == "video",
            };
            // Release 14.10.2026: relative to the document base URL (HTML
            // §2.4.1, the first `<base href>`); `base.href` itself relative to
            // the fallback base URL, and without the attribute it IS that URL
            // (HTML §4.2.3). Elements of another document (frames,
            // DOMParser) resolve in the preamble against their own
            // `baseURI`. MEASURED before (WPT base_href_unspecified,
            // base-javascript): `base.href` without the attribute was "",
            // and `a.href` ignored `<base>`.
            // Another document: the preamble's reflection (the generic
            // `href` further down would answer with the page address).
            if passt && l.dokument_von(k) != l.wurzel {
                return Ok(None);
            }
            if passt {
                let basis = if tag == "base" { adresse.clone() } else { crate::lebend::dokument_basis(l, &adresse) };
                let wert = match l.dok.attribut(k, name) {
                    Some(roh) => adresse_aufloesen(&basis, w.kodierung, roh),
                    // `form.action` ohne Attribut ist die Dokumentadresse.
                    None if name == "action" || tag == "base" => adresse,
                    None => String::new(),
                };
                return Ok(Some(Wert::Text(wert)));
            }
        }
    }
    // Release 10.10.2026: die URL-Teile eines Links (HTML §4.6.4
    // HTMLHyperlinkElementUtils: `protocol`, `host`, `hostname`, `port`,
    // `pathname`, `search`, `hash`, `origin`) aus dem aufgeloesten `href`.
    // GEMESSEN vorher (spiegel.de): `a.hash` war `undefined`
    // (`i.hash.split(…)` warf in `initPrivacyManagerButtons`), `a.pathname`
    // war `""` — die Seitenlogik brach ab, Dialoge blieben offen.
    if art_jetzt == 0
        && matches!(name, "protocol" | "host" | "hostname" | "port" | "pathname" | "search" | "hash" | "origin")
        && let Some(l) = w.lebend.as_ref()
        && let Some(k) = l.knoten(id)
        && let Some(tag) = l.dok.element_name(k)
        && matches!(tag, "a" | "area")
    {
        let href = match l.dok.attribut(k, "href") {
            Some(roh) => adresse_aufloesen(&crate::lebend::dokument_basis(l, &w.adresse), w.kodierung, roh),
            // Release 2026-10-15: no url: `protocol` is ":" (HTML §4.6.4).
            None => return Ok(Some(Wert::Text(if name == "protocol" { ":".to_string() } else { String::new() }))),
        };
        return Ok(Some(Wert::Text(url_teil(&href, name))));
    }
    // Release 10.10.2026: `img.complete`/`naturalWidth`/`naturalHeight`
    // (HTML §4.8.4.3): fertig ohne Quelle oder sobald `load`/`error` fuer
    // DIESE Quelle gemeldet ist (siehe `Stand::bild_gemeldet`).
    if art_jetzt == 0
        && matches!(name, "complete" | "naturalWidth" | "naturalHeight")
        && let Some(l) = w.lebend.as_ref()
        && let Some(k) = l.knoten(id)
        && l.dok.element_name(k) == Some("img")
    {
        let src = l.dok.attribut(k, "src").unwrap_or("").trim().to_string();
        let srcset = l.dok.attribut(k, "srcset").is_some_and(|s| !s.trim().is_empty());
        let quelle = narsil_url::loese_auf(&w.adresse, &src).unwrap_or_else(|_| src.clone());
        let mut gemeldet = w
            .bild_gemeldet
            .get(&id)
            .filter(|g| srcset || g.0 == quelle)
            .map(|g| (g.1, g.2));
        if gemeldet.is_none() {
            gemeldet = w.bilder_fertig.get(&quelle).copied();
        }
        if gemeldet.is_none() && srcset {
            // Ein Kandidat aus `srcset` (`url 320w, url 480w`).
            let liste = l.dok.attribut(k, "srcset").unwrap_or("").to_string();
            for teil in liste.split(',') {
                let Some(u) = teil.split_whitespace().next() else { continue };
                let u = narsil_url::loese_auf(&w.adresse, u).unwrap_or_else(|_| u.to_string());
                if let Some(g) = w.bilder_fertig.get(&u) {
                    gemeldet = Some(*g);
                    break;
                }
            }
        }
        return Ok(Some(match name {
            "complete" => Wert::Wahr((src.is_empty() && !srcset) || gemeldet.is_some()),
            "naturalWidth" => Wert::Zahl(gemeldet.map_or(0, |g| g.0) as f64),
            _ => Wert::Zahl(gemeldet.map_or(0, |g| g.1) as f64),
        }));
    }
    // Release 2026-10-15: `dataset` of an element is the prelude's
    // DOMStringMap proxy (supported property names for Object.keys etc.).
    if name == "dataset"
        && art_jetzt == 0
        && w.lebend.as_ref().is_some_and(|l| l.knoten(id).is_some_and(|k| l.dok.ist_element_knoten(k)))
    {
        return Ok(None);
    }
    // Release 2026-10-14: the template element whose contents `id` is (the
    // "host" of a template's DocumentFragment), for the positional HTML
    // methods in the prelude; null for any other node.
    if name == "__narsilTemplateHost"
        && let Some(l) = w.lebend.as_mut()
        && let Some(k) = l.knoten(id)
    {
        let host = (0..l.dok.knoten.len()).find(|&i| l.dok.template_inhalt(i) == Some(k));
        return Ok(Some(match host {
            Some(h) => l.wert(Some(h)),
            None => Wert::Null,
        }));
    }
    // Release 2026-10-14: in XML documents the prelude answers
    // `innerHTML`/`outerHTML` (XML serialization, DOM Parsing §2.1).
    if matches!(name, "innerHTML" | "outerHTML")
        && w.lebend.as_mut().is_some_and(|l| xml_element(l, id))
    {
        return Ok(None);
    }
    // Release 13.10.2026: `location` eines erzeugten Dokuments beantwortet
    // der Vorspann (null, Rahmendokumente ihren eigenen Ort). GEMESSEN
    // vorher (WPT DOMImplementation-createHTMLDocument): `doc.location` war
    // das Dokument selbst (das alte Unterobjekt unten).
    if name == "location"
        && id != w.document_id
        && let Some(l) = w.lebend.as_ref()
        && let Some(k) = l.knoten(id)
        && matches!(l.dok.knoten.get(k).map(|n| &n.art), Some(narsil_fmt_html::dom::Art::Dokument))
    {
        return Ok(None);
    }
    if let Some(l) = w.lebend.as_mut() {
        let v = match art_jetzt {
            0 => crate::lebend::lesen(l, id, name),
            2 | 3 => crate::lebend::unterobjekt_lesen(l, id, art_jetzt, name),
            _ => None,
        };
        if let Some(v) = v {
            return Ok(Some(v));
        }
        // Ein fehlendes `data-*` ist `undefined` (kein Rueckfall auf andere Namen).
        if art_jetzt == 3 && l.knoten(id).is_some() && !name.starts_with("__") {
            return Ok(None);
        }
    }
    // Release 11.10.2026: benannte Elemente des Dokuments (`document.f`
    // fuer `<form name=f>`), nur fuer Namen, die keine DOM-Methode und kein
    // Expando sind (die Suche laeuft durch den Baum).
    if id == w.document_id
        && !name.starts_with("__")
        && methode_nummer(name).is_none()
        && !w.expando.contains_key(&(id, name.to_string()))
        && !matches!(name, "cookie" | "nodeType" | "nodeName" | "currentScript" | "characterSet" | "charset"
            | "inputEncoding" | "readyState" | "defaultView" | "body" | "documentElement" | "head" | "title"
            | "href" | "URL" | "location" | "constructor" | "then" | "toJSON")
        && let Some(l) = w.lebend.as_mut()
        && let Some(v) = crate::lebend::dokument_benannt(l, name)
    {
        return Ok(Some(v));
    }
    if id == w.document_id {
        return Ok(match name {
            "cookie" => Some(Wert::Text(crate::keks::cookie_lesen(&w.adresse))),
            "nodeType" => Some(Wert::Zahl(9.0)),
            "nodeName" => Some(Wert::Text("#document".to_string())),
            // Release 10.10.2026: GEMESSEN vorher (bbc.com, Turbopack):
            // `document.currentScript.getAttribute(...)` -> TypeError, das
            // Laufzeit-Skript brach ab.
            "currentScript" => Some(w.aktuelles_skript.map_or(Wert::Null, |h| Wert::Gegenstand(host_feld(h)))),
            // Release 11.10.2026: die Dokumentkodierung (DOM §4.5).
            "characterSet" | "charset" | "inputEncoding" => {
                Some(Wert::Text(w.kodierung.map_or_else(|| String::from("UTF-8"), |k| k.kanonischer_name())))
            }
            "readyState" => Some(Wert::Text(if w.ready_state.is_empty() {
                "loading"
            } else { w.ready_state }.to_string())),
            "defaultView" => Some(host_wert(WINDOW_HOST_ID)),
            "body" | "documentElement" | "head" => w
                .standard
                .get(name)
                .map(|i| Wert::Gegenstand(host_feld(*i)))
                .or(Some(Wert::Null)),
            "title" => Some(Wert::Text(String::new())),
            // Release 10.10.2026: `document.location` ist `window.location`
            // (Getter am `Document.prototype`, siehe `vorspann.js`).
            "href" | "URL" => Some(Wert::Text(w.adresse.clone())),
            // EXPANDO (05.10.2026): `window.eventLoggingLite` und Freunde.
            _ => w
                .expando
                .get(&(id, name.to_string()))
                .map(|t| Wert::Text(t.clone())),
        });
    }
    Ok(match name {
        // --- TEXT -------------------------------------------------------
        //
        // GEMESSEN, warum `innerText` dasselbe ist wie `textContent`: in
        // diesem Browser gibt es keinen sichtbaren Text ohne
        // CSS-Transformation (kein `text-transform`, kein `visibility`
        // pro Zeile). Der Unterschied faellt erst bei
        // `display: none` auf — und das ist eine spaetere Welle.
        "textContent" | "innerText" | "innerHTML" => {
            Some(Wert::Text(w.text.get(&id).cloned().unwrap_or_default()))
        }
        "value" if feld_wert_lesen(&w, id).is_some() => feld_wert_lesen(&w, id).map(Wert::Text),
        // Release 2026-10-15: `value` of any other element of the live DOM
        // is the prelude's reflection (li: long, data/param/button: the
        // attribute). MEASURED before (WPT reflection-*): li.value was a
        // string, data.value = "x" stored nothing.
        "value" if w.lebend.as_ref().is_some_and(|l| l.knoten(id).is_some_and(|k| l.dok.ist_element_knoten(k))) => None,
        "value" => {
            let value = w.ord.get(&id).and_then(|ord|
                w.eingabe_werte.get(ord).cloned().or_else(||
                    w.eingaben.iter().find(|e|e.ord==*ord).map(|e|e.wert.clone())))
                .or_else(||w.attribute.get(&id).and_then(|a|a.get("value")).cloned())
                .or_else(||w.text.get(&id).cloned()).unwrap_or_default();
            Some(Wert::Text(value))
        },
        // Release 12.10.2026: `style.disabled` beantwortet der Vorspann (Blatt).
        "disabled"
            if w.lebend.as_ref().is_some_and(|l| l.knoten(id).is_some_and(|k| l.dok.element_name(k).is_some_and(|n| n.eq_ignore_ascii_case("style")))) =>
        {
            None
        }
        "checked" | "disabled" | "hidden" => {
            let v = w
                .attribute
                .get(&id)
                .and_then(|a| a.get(name))
                .cloned()
                .unwrap_or_default();
            Some(Wert::Wahr(v == "true" || v == name))
        }
        // --- IDENTITAET --------------------------------------------------
        "namespaceURI" => Some(w.namespaces.get(&id).map_or(
            Wert::Text("http://www.w3.org/1999/xhtml".to_string()), |(ns, _, _)| ns.as_ref().map_or(Wert::Null, |s| Wert::Text(s.clone())))),
        "prefix" => Some(w.namespaces.get(&id).and_then(|(_, p, _)| p.clone()).map_or(Wert::Null, Wert::Text)),
        "localName" => Some(Wert::Text(w.namespaces.get(&id).map(|(_, _, l)| l.clone()).unwrap_or_else(|| w.tag.get(&id).cloned().unwrap_or_default()))),
        "tagName" | "nodeName" => match w.tag.get(&id) {
            Some(t) => Some(Wert::Text(if w.namespaces.get(&id).is_some_and(|(ns, _, _)| ns.as_deref() != Some("http://www.w3.org/1999/xhtml")) { t.clone() } else { t.to_ascii_uppercase() })),
            None => return Ok(None),
        },
        "nodeType" => Some(Wert::Zahl(1.0)),
        "id" => Some(Wert::Text(
            w.ord
                .get(&id)
                .and_then(|o| w.dom_ids.get(o))
                .cloned()
                .unwrap_or_default(),
        )),
        "className" => Some(Wert::Text(
            w.klassen.get(&id).map_or(String::new(), |k| k.join(" ")),
        )),
        // --- UNTEROBJEKTE ------------------------------------------------
        //
        // `el.style === el.style`: beide tragen dieselbe Id, und der
        // Getter gibt beide Male dasselbe Objekt zurueck.
        "style" | "classList" | "dataset" | "location" => {
            //
            // DIE ART MERKEN (siehe `Stand::art`): der naechste Zugriff mit
            // dieser Id weiss damit, ob er im Stil, in der Klasse oder im
            // Element ist.
            w.art.insert(
                id,
                match name {
                    "style" => 1,
                    "classList" => 2,
                    "dataset" => 3,
                    _ => 0,
                },
            );
            let mut fields = host_feld(id);
            fields.push(("__art".to_string(), Wert::Zahl(*w.art.get(&id).unwrap_or(&0) as f64)));
            Some(Wert::Gegenstand(fields))
        }
        "children" | "childNodes" => Some(Wert::Feld(kinder_von(&w, id))),
        "childElementCount" => Some(Wert::Zahl(kinder_von(&w, id).len() as f64)),
        "parentNode" | "parentElement" => Some(eltern_host(&w, id)
            .map_or(Wert::Null, |parent| Wert::Gegenstand(host_feld(parent)))),

        // --- LOCATION ----------------------------------------------------
        //
        // `window.location.href` (nur lesen, J9). Das `location`-Objekt
        // traegt die Id von `document`, und `href` steht hier.
        "href" | "host" | "hostname" | "protocol" | "pathname" => {
            if name == "href" {
                Some(Wert::Text(w.adresse.clone()))
            } else {
                Some(Wert::Text(String::new()))
            }
        }
        // --- STIL ZURUECKLESEN (W6E, 17:05) -----------------------------
        //
        // GEMESSENER FEHLER: `g.style.color = 'red'` schrieb nach
        // `Stand::stil[id]` (das steht in `schreiben`), und `g.style.color`
        // las danach `undefined`. Ein Skript, das seinen Stil prueft
        // (`if (el.style.color === 'red')`), bekam also immer `falsch` —
        // in einem Browser liefert `el.style.color = 'red'; el.style.color`
        // `red`.
        //
        // GEMESSEN, warum dieser Arm als LETZTER steht: ein Arm davor
        // fing auch `style` selbst ab (der Zweig oben liefert `art = 1`),
        // und `g.style` wurde dann `undefined` — der naechste Schritt
        // `g.style.color = …` endete mit „kein Setzen auf Primitiv"
        // (gemessen 17:10).
        //
        // GEMESSEN, warum NUR ein gesetzter Name antwortet: der Getter darf
        // keine Eigenschaft erfinden. `rev()`: der LETZTE Eintrag gewinnt,
        // weil `schreiben` bei mehrfachem Setzen anhaengt.
        _ if w.art.get(&id) == Some(&1) => w.stil.get(&id).and_then(|liste| {
            liste
                .iter()
                .rev()
                .find(|(n, _)| n == name)
                .map(|(_, wert)| Wert::Text(wert.clone()))
        }),
        // EXPANDO (05.10.2026): eine vom Skript gesetzte Eigenschaft, die
        // dieser Browser nicht kennt. Sie wird zurueckgelesen (siehe den
        // Setter) statt `undefined` zu liefern.
        _ => w
            .expando
            .get(&(id, name.to_string()))
            .map(|t| Wert::Text(t.clone())),
    })
}

/// Das Feld, das ein Host-Objekt traegt.
fn host_feld(id: u32) -> Vec<(String, narsil_jsvm::Wert)> {
    vec![(MERKE.to_string(), narsil_jsvm::Wert::Zahl(id as f64))]
}

pub fn host_wert(id: u32) -> narsil_jsvm::Wert {
    let mut fields = host_feld(id);
    if id == WINDOW_HOST_ID {
        fields.push(("__global".to_string(), narsil_jsvm::Wert::Wahr(true)));
    }
    narsil_jsvm::Wert::Gegenstand(fields)
}

// ===========================================================================
// SETTER
// ===========================================================================

/// Eine Eigenschaft schreiben.
///
/// WAS JAVASCRIPT SCHREIBT, WIRD EIN BEFEHL. Der Hauptthread spielt die
/// Befehle nach dem Lauf in den Patch und baut die Seite neu — so ist
/// `textContent` nach drei Klicks eine `3` und kein `1`.
/// Release 11.10.2026: `value` von `<input>`/`<textarea>` nach HTML §4.10.5
/// (Wertmodus und Bereinigung je `type`, siehe `formular_werte`). `None` =
/// kein solches Feld (der alte Weg gilt).
fn feld_wert_lesen(w: &Stand, id: u32) -> Option<String> {
    use crate::formular_werte::{Wertmodus, bereinigen, wertmodus};
    let l = w.lebend.as_ref()?;
    let k = l.knoten(id)?;
    let tag = l.dok.element_name(k)?;
    let attr = |n: &str| l.dok.attribut(k, n).map(str::to_string);
    let ord = w.ord.get(&id).copied();
    let schmutzig = ord.and_then(|o| w.eingabe_werte.get(&o).cloned()).or_else(|| w.feld_werte.get(&id).cloned());
    match tag {
        "input" => {
            let typ = crate::lebend::typ_lesen("input", l.dok.attribut(k, "type"));
            Some(match wertmodus(&typ) {
                Wertmodus::Vorgabe => attr("value").unwrap_or_default(),
                Wertmodus::VorgabeAn => attr("value").unwrap_or_else(|| "on".to_string()),
                // Die Wahl des Nutzers legt der Host als `C:\fakepath\name`
                // ab (`Sitzung::datei_auswahl_setzen`).
                Wertmodus::Dateiname => schmutzig.unwrap_or_default(),
                Wertmodus::Wert => {
                    let roh = schmutzig.unwrap_or_else(|| attr("value").unwrap_or_default());
                    bereinigen(&typ, &roh, &attr)
                }
            })
        }
        // HTML §4.10.10: `value` einer Option ist das Attribut, sonst ihr Text
        // (ASCII-Leerraum entfernt und zusammengezogen).
        "option" => Some(attr("value").unwrap_or_else(|| {
            l.dok.text_inhalt(k).split(['\t', '\n', '\u{c}', '\r', ' ']).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" ")
        })),
        // Release 14.10.2026: the raw value defaults to the CHILD text content
        // (only own Text children), and the API value normalizes CRLF/CR to LF
        // (HTML §4.10.11). The parser already drops the newline right after
        // the start tag (`lf_nach_start`), so it is not stripped again here.
        // MEASURED before (WPT textarea value-defaultValue-textContent): the
        // text of child elements counted, CR stayed, and a leading newline
        // set by script vanished.
        "textarea" => Some(
            schmutzig
                .unwrap_or_else(|| l.child_text(k))
                .replace("\r\n", "\n")
                .replace('\r', "\n"),
        ),
        _ => None,
    }
}

/// Release 11.10.2026: der `type` eines `<input>` (normalisiert), `None` bei
/// anderen Elementen.
fn eingabe_typ(w: &Stand, id: u32) -> Option<String> {
    let l = w.lebend.as_ref()?;
    let k = l.knoten(id)?;
    (l.dok.element_name(k)? == "input").then(|| crate::lebend::typ_lesen("input", l.dok.attribut(k, "type")))
}

/// Release 11.10.2026: HTML §4.10.5 „type attribute change steps" — der
/// Wert folgt dem Wechsel des Wertmodus (value -> default schreibt das
/// Attribut, ein anderer Modus -> value liest das Attribut, -> filename
/// leert). GEMESSEN vorher (WPT `type-change-text-to-file-to-text`): der
/// alte Wert blieb.
fn typ_wechsel(w: &mut Stand, id: u32, alt: Option<String>) {
    use crate::formular_werte::{Wertmodus, bereinigen, wertmodus};
    let Some(alt) = alt else { return };
    let Some(neu) = eingabe_typ(w, id) else { return };
    if alt == neu {
        return;
    }
    // Release 11.10.2026: ein angehaktes Feld, das zum Radio wird, hakt die
    // anderen seiner Gruppe ab (HTML §4.10.5.1.18). GEMESSEN vorher (WPT
    // radio-morphed): zwei angehakte Radios.
    if neu == "radio" && alt != "radio"
        && let Some(l) = w.lebend.as_mut()
        && let Some(k) = l.knoten(id)
        && crate::lebend::ist_angehakt(&l.dok, k)
    {
        crate::lebend::haken_neu_setzen(l, k);
    }
    let (ma, mn) = (wertmodus(&alt), wertmodus(&neu));
    let ord = w.ord.get(&id).copied();
    let schmutzig = ord.and_then(|o| w.eingabe_werte.get(&o).cloned()).or_else(|| w.feld_werte.get(&id).cloned());
    let leeren = |w: &mut Stand| {
        w.feld_werte.remove(&id);
        wert_spiegeln(w, id, None);
        if let Some(o) = ord {
            w.eingabe_werte.remove(&o);
        }
    };
    if ma == Wertmodus::Wert && matches!(mn, Wertmodus::Vorgabe | Wertmodus::VorgabeAn) {
        if let Some(v) = schmutzig.filter(|v| !v.is_empty())
            && let Some(l) = w.lebend.as_mut()
            && let Some(k) = l.knoten(id)
        {
            l.dok.setze_attribut(k, narsil_fmt_html::dom::Namensraum::Html, "value", &v);
            l.geaendert = true;
        }
        leeren(w);
    } else if ma != Wertmodus::Wert && mn == Wertmodus::Wert {
        leeren(w);
    } else if mn == Wertmodus::Dateiname {
        leeren(w);
    } else if ma == Wertmodus::Wert && mn == Wertmodus::Wert
        && let Some(v) = schmutzig
    {
        let neu_wert = {
            let Some(l) = w.lebend.as_ref() else { return };
            let Some(k) = l.knoten(id) else { return };
            let attr = |n: &str| l.dok.attribut(k, n).map(str::to_string);
            bereinigen(&neu, &v, &attr)
        };
        wert_spiegeln(w, id, Some(&neu_wert));
        w.feld_werte.insert(id, neu_wert.clone());
        if let Some(o) = ord {
            w.eingabe_werte.insert(o, neu_wert);
        }
    }
}

/// Release 2026-10-15: mirrors a dirty value into the live document
/// (`Dokument::css_werte`) for selector matching and the page tree
/// (`:valid`, `:in-range`, `:placeholder-shown`); `None` removes it.
fn wert_spiegeln(w: &mut Stand, id: u32, wert: Option<&str>) {
    let Some(l) = w.lebend.as_mut() else { return };
    let Some(k) = l.knoten(id) else { return };
    l.dok.css_wert_setzen(k, wert);
}

/// Release 2026-10-15: the user's input into control `ord` (see
/// `wert_spiegeln`).
pub(crate) fn eingabe_spiegeln(w: &mut Stand, ord: usize, wert: &str) {
    if let Some(id) = w.ids.get(&ord).copied() {
        wert_spiegeln(w, id, Some(wert));
    }
}

fn feld_wert_schreiben(w: &mut Stand, id: u32, text: &str) -> Option<Result<(), String>> {
    use crate::formular_werte::{Wertmodus, bereinigen, wertmodus};
    let (tag, typ, wert) = {
        let l = w.lebend.as_ref()?;
        let k = l.knoten(id)?;
        let tag = l.dok.element_name(k)?.to_string();
        let typ = crate::lebend::typ_lesen(&tag, l.dok.attribut(k, "type"));
        let attr = |n: &str| l.dok.attribut(k, n).map(str::to_string);
        let wert = match tag.as_str() {
            "input" if wertmodus(&typ) == Wertmodus::Wert => bereinigen(&typ, text, &attr),
            "textarea" => text.replace("\r\n", "\n").replace('\r', "\n"),
            _ => text.to_string(),
        };
        (tag, typ, wert)
    };
    match tag.as_str() {
        "option" => {
            let l = w.lebend.as_mut()?;
            let k = l.knoten(id)?;
            l.dok.setze_attribut(k, narsil_fmt_html::dom::Namensraum::Html, "value", &wert);
            l.geaendert = true;
            return Some(Ok(()));
        }
        "input" => match wertmodus(&typ) {
            Wertmodus::Vorgabe | Wertmodus::VorgabeAn => {
                let l = w.lebend.as_mut()?;
                let k = l.knoten(id)?;
                l.dok.setze_attribut(k, narsil_fmt_html::dom::Namensraum::Html, "value", &wert);
                l.geaendert = true;
                return Some(Ok(()));
            }
            Wertmodus::Dateiname => {
                return Some(if wert.is_empty() {
                    w.feld_werte.remove(&id);
                    wert_spiegeln(w, id, None);
                    if let Some(ord) = w.ord.get(&id).copied() {
                        w.eingabe_werte.remove(&ord);
                    }
                    Ok(())
                } else {
                    Err("InvalidStateError: Failed to set the 'value' property on 'HTMLInputElement': This input element accepts a filename, which may only be programmatically set to the empty string.".to_string())
                });
            }
            Wertmodus::Wert => {}
        },
        "textarea" => {}
        _ => return None,
    }
    wert_spiegeln(w, id, Some(&wert));
    w.feld_werte.insert(id, wert.clone());
    if let Some(ord) = w.ord.get(&id).copied() {
        w.eingabe_werte.insert(ord, wert.clone());
        w.befehle.push(Befehl::Attribut { knoten: ord as u32, name: "value".into(), wert });
    }
    Some(Ok(()))
}

fn schreiben(stand: &Geteilt, id: u32, name: &str, wert: &narsil_jsvm::Wert) -> Result<(), String> {
    use narsil_jsvm::Wert;
    {
        let mut w = stand.lock().expect("DomStand");
        crate::anim_hosts::styles_changed(&mut w);
        w.layout_veraltet = true;
        w.stil_pruefen = true;
        if stil_knoten(&w, id) {
            w.stil_veraltet = true;
        }
    }
    // Release 09.10.2026: das lebende DOM zuerst — VOR der 16-KiB-Grenze, denn
    // `innerHTML` einer Vorlage ist oft groesser (eigene Grenze dort).
    {
        let mut w = stand.lock().expect("DomStand");
        let art = w.art.get(&id).copied().unwrap_or(0);
        // Release 11.10.2026: `el.style = "…"` ist [PutForwards=cssText]
        // (CSSOM §6.7), `el.classList = "…"` [PutForwards=value] (DOM
        // §4.9). GEMESSEN vorher (WPT top-layer-position): die Zuweisung
        // legte ein freies Feld an, der Stil blieb.
        let (art, name) = match (art, name) {
            (0, "style") if w.lebend.is_some() => (1, "cssText"),
            (0, "classList") if w.lebend.is_some() => (2, "value"),
            x => x,
        };
        let typ_vorher = (art == 0 && name == "type").then(|| eingabe_typ(&w, id)).flatten();
        let r = match (art, w.lebend.as_mut()) {
            (0, Some(l)) => crate::lebend::schreiben(l, id, name, wert),
            (1, Some(l)) => crate::lebend::stil_schreiben(l, id, name, wert),
            (2 | 3, Some(l)) => crate::lebend::unterobjekt_schreiben(l, id, art, name, wert),
            _ => None,
        };
        if let Some(r) = r {
            if typ_vorher.is_some() && r.is_ok() {
                typ_wechsel(&mut w, id, typ_vorher);
            }
            if art == 1 {
                spiegeln(&mut w, id, Some("style"));
            } else if art != 0 || matches!(name, "id" | "className" | "hidden" | "title" | "lang" | "dir" | "slot") {
                spiegeln(&mut w, id, None);
            }
            return r;
        }
    }
    // M4: der Wert kommt aus einem Skript. Fuer Text, Attribute, Klassen
    // und Stile gilt `MAX_ATTRIBUT` (16 KiB). Ein laengerer Wert wird
    // ABGELEHNT und nicht gekuerzt — ein gekuerzter `textContent` waere
    // eine falsche Seite.
    //
    // AUSNAHME: ein Handler-Quelltext. `MAX_SKRIPT_QUELLE` (1 MiB) gilt
    // dort, weil ein Handler ein Skript ist.
    let grenze = if name.starts_with("on") {
        crate::js::MAX_SKRIPT_QUELLE
    } else {
        MAX_ATTRIBUT
    };
    let text = wert.als_text();
    if text.len() > grenze {
        return Err(std::format!(
            "Wert zu lang fuer '{name}': {} Byte (erlaubt {grenze})",
            text.len()
        ));
    }
    let mut w = stand.lock().expect("DomStand");
    if id == w.document_id && w.art.get(&id).copied().unwrap_or(0) == 0 && name == "cookie" {
        let adresse = w.adresse.clone();
        drop(w);
        crate::keks::cookie_schreiben(&adresse, &text);
        return Ok(());
    }
    let ord = w.ord.get(&id).copied().unwrap_or(0) as u32;

    //
    // W6B: `el.style.color = "rot"` — der Getter hat `style` gemerkt, also
    // ist der Name eine CSS-Eigenschaft und keine DOM-Eigenschaft.
    //
    // GEMESSEN, warum die Pruefung HIER und nicht im Getter (16:15): der
    // Getter gibt nur ein Objekt zurueck; was danach geschrieben wird,
    // sieht er nicht (siehe `Stand::art`). Ohne diese Zeile war
    // `el.style.color = "rot"` der Fehler „Eigenschaft 'color' gibt es
    // nicht".
    if w.art.get(&id) == Some(&1) {
        // Release 10.10.2026: je Name EIN Eintrag (vorher wuchs die Liste
        // mit jeder Zuweisung); `cssText` ersetzt alle.
        let liste = w.stil.entry(id).or_default();
        if name == "cssText" {
            liste.clear();
        } else {
            liste.retain(|(n, _)| n != name);
        }
        liste.push((name.to_string(), text.clone()));
        w.befehle.push(Befehl::Stil {
            knoten: ord,
            eigenschaft: name.to_string(),
            wert: text,
        });
        return Ok(());
    }
    // `dataset.x = …` ist ein `data-`-Attribut (Web-IDL).
    if w.art.get(&id) == Some(&3) {
        let mut attribut = String::from("data-");
        for c in name.chars() { if c.is_ascii_uppercase() { attribut.push('-'); attribut.push(c.to_ascii_lowercase()); } else { attribut.push(c); } }
        w.attribute.entry(id).or_default().insert(attribut.clone(), text.clone());
        w.befehle.push(Befehl::Attribut {
            knoten: ord,
            name: attribut,
            wert: text,
        });
        return Ok(());
    }
    // Release 11.10.2026: Fenster und Dokument haben keine Element-Nummer —
    // `window.id = token()` (WPT track/cors, 44 Dateien) wurde sonst als
    // `id`-Attribut auf Nummer 0 geschrieben (das erste Element der Seite)
    // und war danach weder `window.id` noch die globale Variable `id`.
    if (id == WINDOW_HOST_ID || id == w.document_id)
        && !w.ord.contains_key(&id)
        && matches!(name, "textContent" | "innerText" | "value" | "innerHTML" | "id" | "className")
    {
        w.expando.insert((id, name.into()), text);
        return Ok(());
    }
    match name {
        // --- TEXT ---------------------------------------------------------
        "textContent" | "innerText" => {
            w.text.insert(id, text.clone());
            w.befehle.push(Befehl::Text { knoten: ord, text });
        }
        "value" => {
            // `[LegacyNullToEmptyString]` on input/textarea `value`.
            let text = if matches!(wert, Wert::Null) && feld_wert_lesen(&w, id).is_some() { String::new() } else { text };
            if let Some(r) = feld_wert_schreiben(&mut w, id, &text) {
                return r;
            }
            w.eingabe_werte.insert(ord as usize, text.clone());
            w.befehle.push(Befehl::Attribut { knoten: ord, name: "value".into(), wert: text });
        }
        "innerHTML" => {
            w.text.insert(id, text.clone());
            w.befehle.push(Befehl::Html {
                knoten: ord,
                html: text,
            });
        }
        // --- IDENTITAET ----------------------------------------------------
        "id" => {
            //
            // GEMESSEN, warum `o` kopiert wird (E0502, 14:25): `w.ord.get()`
            // leiht `w` unveranderlich, und `w.dom_ids.insert()` will es
            // veraendern — beides in EINEM `if let`. Die Kopie ist eine
            // Zahl.
            if let Some(o) = w.ord.get(&id).copied() {
                w.dom_ids.insert(o, text.clone());
            }
            w.befehle.push(Befehl::Attribut {
                knoten: ord,
                name: "id".to_string(),
                wert: text,
            });
        }
        "className" => {
            w.klassen
                .insert(id, text.split_whitespace().map(str::to_string).collect());
            w.befehle.push(Befehl::Attribut {
                knoten: ord,
                name: "class".to_string(),
                wert: text,
            });
        }
        // --- EREIGNISSE -----------------------------------------------------
        // One replaceable property handler, separate from ordered listeners.
        _ if name.starts_with("on") && name.len() > 2 => {
            let typ = name[2..].to_string();
            if matches!(wert, Wert::Null | Wert::Undefiniert) || text.is_empty() {
                w.horcher.remove(&(id, typ.clone()));
            } else {
                let handler = match wert {
                    Wert::Funktion(index) => crate::handler::Handler::Funktion(*index),
                    _ => crate::handler::Handler::Quelltext(text.clone()),
                };
                w.horcher.insert((id, typ.clone()), handler);
            }
            // Document/Window have no element ordinal. Their registrations
            // must never accidentally become an HTML attribute on ordinal 0.
            if w.ord.contains_key(&id) {
                w.befehle.push(Befehl::Horcher { knoten: ord, typ, quelle: text });
            }
        }
        // EXPANDO (05.10.2026): eine Eigenschaft, die dieser Browser nicht
        // kennt, wird wie in Firefox angelegt statt abgelehnt. Vorher warf
        // jede solche Zuweisung einen `Err` („Eigenschaft 'eventLoggingLite'
        // gibt es nicht") und beendete das GANZE Skript — GEMESSEN an
        // www.wikipedia.org. `MAX_ATTRIBUT` begrenzt den Wert (oben schon
        // geprueft).
        other => {
            w.expando.insert((id, other.to_string()), text.clone());
        }
    }
    Ok(())
}

// ===========================================================================
// AUFRUF (Methoden)
// ===========================================================================

/// Eine Methode rufen: `classList.add`, `style.setProperty`,
/// `addEventListener`, `getAttribute`, …
fn rufen(
    stand: &Geteilt,
    id: u32,
    nummer: &str,
    args: &[narsil_jsvm::Wert],
) -> Result<narsil_jsvm::Wert, String> {
    use narsil_jsvm::Wert;
    let name = nummer.parse::<f64>().ok().and_then(methode_name)
        .or_else(|| methode_nummer(nummer).and_then(methode_name))
        .ok_or_else(|| std::format!("Methode {nummer} gibt es nicht"))?;
    // Release 09.10.2026: das lebende DOM zuerst (eigene Grenzen).
    {
        let mut w = stand.lock().expect("DomStand");
        // Release 10.10.2026: jede Methode kann das DOM aendern — die
        // berechneten Werte fuer `getComputedStyle` gelten dann nicht mehr.
        if !matches!(name, "getAttribute" | "hasAttribute" | "querySelector" | "querySelectorAll" | "getElementById"
            | "getElementsByClassName" | "getElementsByTagName" | "getBoundingClientRect" | "contains" | "matches" | "closest")
        {
            crate::anim_hosts::styles_changed(&mut w);
            w.layout_veraltet = true;
            w.stil_pruefen = true;
            if stil_knoten(&w, id) || args.iter().filter_map(narsil_jsvm::host_id).any(|a| stil_knoten(&w, a)) {
                w.stil_veraltet = true;
            }
        }
        let art = w.art.get(&id).copied().unwrap_or(0);
        let zustand = w.css_baum.zustand.clone();
        let typ_vorher = (art == 0
            && matches!(name, "setAttribute" | "removeAttribute" | "toggleAttribute")
            && args.first().is_some_and(|a| a.als_text().eq_ignore_ascii_case("type")))
            .then(|| eingabe_typ(&w, id))
            .flatten();
        if let Some(l) = w.lebend.as_mut()
            && let Some(r) = crate::lebend::rufen(l, id, art, name, args, &zustand)
        {
            if typ_vorher.is_some() && r.is_ok() {
                typ_wechsel(&mut w, id, typ_vorher);
            }
            // Release 11.10.2026: HTML „cloning steps" von input/textarea —
            // der Wert samt dirty-Flag geht mit. GEMESSEN vorher (WPT
            // cloning-steps): der Klon hatte den Attributwert.
            if art == 0
                && name == "cloneNode"
                && let Ok(narsil_jsvm::Wert::Gegenstand(felder)) = &r
                && let Some(neu) = felder.iter().find(|(n, _)| n == MERKE).and_then(|(_, v)| match v {
                    narsil_jsvm::Wert::Zahl(z) => Some(*z as u32),
                    _ => None,
                })
            {
                let ord = w.ord.get(&id).copied();
                let wert = ord.and_then(|o| w.eingabe_werte.get(&o).cloned()).or_else(|| w.feld_werte.get(&id).cloned());
                if let Some(v) = wert {
                    wert_spiegeln(&mut w, neu, Some(&v));
                    w.feld_werte.insert(neu, v);
                }
            }
            if art == 2 {
                spiegeln(&mut w, id, None);
            } else if art == 1 {
                spiegeln(&mut w, id, Some("style"));
            } else if matches!(name, "setAttribute" | "removeAttribute" | "toggleAttribute") {
                let n = args.first().map(|a| a.als_text().to_ascii_lowercase()).unwrap_or_default();
                spiegeln(&mut w, id, Some(&n));
            }
            return r;
        }
    }
    // M4: die Argumente kommen aus einem Skript.
    for a in args.iter().take(16) {
        if a.als_text().len() > MAX_ATTRIBUT {
            return Err("Argument zu lang".into());
        }
    }
    let mut w = stand.lock().expect("DomStand");
    let ord = w.ord.get(&id).copied().unwrap_or(0) as u32;
    match name {
        // --- classList --------------------------------------------------
        //
        // GEMESSEN, warum `add` MEHRERE Namen annimmt: so macht es der
        // Browser, und `add("a", "b")` steht in jeder zweiten Zeile
        // echten Codes.
        "add" | "remove" | "toggle" => {
            let da = match name {
                "add" => true,
                "remove" => false,
                _ => args
                    .first()
                    .map(|a| a.als_text() == "false")
                    .unwrap_or(true),
            };
            for a in args.iter().take(16) {
                let kl = a.als_text();
                if kl.is_empty() {
                    continue;
                }
                let vorhanden = w.klassen.entry(id).or_default();
                let ist_da = vorhanden.iter().any(|k| *k == kl);
                if da && !ist_da {
                    vorhanden.push(kl.clone());
                } else if !da && ist_da {
                    vorhanden.retain(|k| *k != kl);
                }
                //
                // DER BEFEHL: `da` und `umschalten` sagen dem Bauer, was
                // mit der Klasse geschehen soll (siehe
                // `narsil_page::klassen_wenden`). GEMESSEN, warum NICHT
                // `+x` als Text: der Bauer kennt die vorhandenen Klassen
                // und muss sie selbst lesen — ein fertiger String waere
                // hier eine zweite Wahrheit ueber dieselben Klassen.
                w.befehle.push(Befehl::Klasse {
                    knoten: ord,
                    name: kl,
                    da,
                    umschalten: name == "toggle",
                });
            }
            Ok(Wert::Undefiniert)
        }
        "contains" => {
            let kl = args.first().map_or(String::new(), |a| a.als_text());
            //
            // GEMESSEN, warum hier drei Klammern schliessen (14:10): es
            // sind vier Klammern offen (`Ok`, `Wahr`, `map_or`, `any`),
            // und drei geschlossene gaben „unclosed delimiter: Ok(...)"
            // mitten in einem Match-Zweig.
            Ok(Wert::Wahr(
                w.klassen
                    .get(&id)
                    .map_or(false, |k| k.iter().any(|x| *x == kl)),
            ))
        }
        // --- style ------------------------------------------------------
        //
        // GEMESSEN, warum der Name in EINER Anweisung steht und nicht in
        // `w.stil`: der Patch traegt eine Liste, und der Bauer haengt sie
        // an das `style`-Attribut.
        "setProperty" => {
            let eig = args.first().map_or(String::new(), |a| a.als_text());
            let wert = args.get(1).map_or(String::new(), |a| a.als_text());
            if eig.is_empty() {
                return Err("setProperty: der Eigenschaftsname fehlt".into());
            }
            // GEMESSEN, warum die Klammern noetig sind (E0502): der
            // Eintrag in `stil` und der Schub in `befehle` sind zwei
            // mutable Zugriffe auf dasselbe `w` — der erste gibt den
            // Leihen-Zeiger frei, wenn er endet.
            {
                let liste = w.stil.entry(id).or_default();
                liste.retain(|(n, _)| *n != eig);
                liste.push((eig.clone(), wert.clone()));
            }
            w.befehle.push(Befehl::Stil {
                knoten: ord,
                eigenschaft: eig,
                wert,
            });
            Ok(Wert::Undefiniert)
        }
        "removeProperty" => {
            let eig = args.first().map_or(String::new(), |a| a.als_text());
            if let Some(stile) = w.stil.get_mut(&id) {
                stile.retain(|(n, _)| *n != eig);
            }
            w.befehle.push(Befehl::Stil {
                knoten: ord,
                eigenschaft: eig,
                wert: String::new(),
            });
            Ok(Wert::Undefiniert)
        }
        // --- ATTRIBUTE ---------------------------------------------------
        //
        // GEMESSEN, warum der Wert im `Stand` UND im Befehl steht: das
        // `getAttribute` danach soll den NEUEN Wert sehen (ohne Neuaufbau)
        // und der Patch muss ihn bekommen.
        "setAttribute" => {
            let mut n = args.first().map_or(String::new(), |a| a.als_text());
            if !w.namespaces.get(&id).is_some_and(|(ns, _, _)| ns.as_deref() != Some("http://www.w3.org/1999/xhtml")) {
                n.make_ascii_lowercase();
            }
            let v = args.get(1).map_or(String::new(), |a| a.als_text());
            if n.is_empty() {
                return Err("setAttribute: der Attributname fehlt".into());
            }
            w.attribute
                .entry(id)
                .or_default()
                .insert(n.clone(), v.clone());
            if matches!(n.as_str(),"width"|"height") && w.tag.get(&id).is_some_and(|t|t=="canvas") {canvas_reset(&mut w,id)?;}
            if n == "id" { w.dom_ids.insert(ord as usize, v.clone()); }
            if n == "class" { w.klassen.insert(id, v.split_whitespace().map(str::to_string).collect()); }
            w.befehle.push(Befehl::Attribut {
                knoten: ord,
                name: n,
                wert: v,
            });
            Ok(Wert::Undefiniert)
        }
        "getAttribute" => {
            let mut n = args.first().map_or(String::new(), |a| a.als_text());
            if !w.namespaces.get(&id).is_some_and(|(ns, _, _)| ns.as_deref() != Some("http://www.w3.org/1999/xhtml")) {
                n.make_ascii_lowercase();
            }
            Ok(match w.attribute.get(&id).and_then(|a| a.get(&n)) {
                Some(v) => Wert::Text(v.clone()),
                None => Wert::Null,
            })
        }
        "removeAttribute" => {
            let mut n = args.first().map_or(String::new(), |a| a.als_text());
            if !w.namespaces.get(&id).is_some_and(|(ns, _, _)| ns.as_deref() != Some("http://www.w3.org/1999/xhtml")) {
                n.make_ascii_lowercase();
            }
            if let Some(a) = w.attribute.get_mut(&id) {
                a.remove(&n);
            }
            if n == "id" { w.dom_ids.remove(&(ord as usize)); }
            if n == "class" { w.klassen.remove(&id); }
            w.befehle.push(Befehl::AttributWeg {
                knoten: ord,
                name: n,
            });
            Ok(Wert::Undefiniert)
        }
        // --- EREIGNISSE ---------------------------------------------------
        // The host records mutations only; the page owner dispatches after
        // dropping this lock and uses the same VM that created callbacks.
        "addEventListener" => {
            if args.len() < 2 {
                return Err("addEventListener: type and callback are required".into());
            }
            let typ = args.first().map_or(String::new(), |a| a.als_text());
            let handler = match args.get(1) {
                Some(Wert::Funktion(index)) => crate::handler::Handler::Funktion(*index),
                Some(Wert::Null | Wert::Undefiniert) => return Ok(Wert::Undefiniert),
                // The historical source-string API remains an extension.
                Some(Wert::Text(source)) => crate::handler::Handler::Quelltext(source.clone()),
                _ => return Err("addEventListener: callback must be a function; handleEvent objects are not supported yet".into()),
            };
            let mut options = crate::handler::ListenerOptions::from_wert(args.get(2));
            // Release 2026-10-15: the default passive value (DOM §2.7):
            // touchstart, touchmove, wheel and mousewheel listeners on the
            // window, a document, its document element or its body are
            // passive unless `passive` is given. MEASURED before (WPT
            // dom/events/passive-by-default 43/100): preventDefault worked.
            if matches!(typ.as_str(), "touchstart" | "touchmove" | "wheel" | "mousewheel")
                && !crate::handler::passive_given(args.get(2))
                && (id == WINDOW_HOST_ID
                    || id == w.document_id
                    || w.lebend.as_ref().is_some_and(|l| l.knoten(id).is_some_and(|k| l.default_passive_node(k))))
            {
                options.passive = true;
            }
            // Release 13.10.2026: der erste `slotchange`-Lauscher schaltet die
            // Slot-Beobachtung im Vorspann ein (Reaktion 11).
            if typ == "slotchange"
                && !w.slot_lauscher
                && let Some(l) = w.lebend.as_mut()
            {
                l.ce_schlange.push(crate::lebend::CeReaktion::SlotLauscher(id));
                w.slot_lauscher = true;
            }
            w.event_listeners.add(id, typ, handler, options).map_err(str::to_string)?;
            Ok(Wert::Undefiniert)
        }
        "removeEventListener" => {
            if args.len() < 2 {
                return Err("removeEventListener: type and callback are required".into());
            }
            let typ = args.first().map_or(String::new(), |a| a.als_text());
            let callback = match args.get(1) {
                Some(Wert::Funktion(index)) => crate::handler::Handler::Funktion(*index),
                Some(Wert::Null | Wert::Undefiniert) => return Ok(Wert::Undefiniert),
                Some(Wert::Text(source)) => crate::handler::Handler::Quelltext(source.clone()),
                _ => return Err("removeEventListener: callback must be a function; handleEvent objects are not supported yet".into()),
            };
            let options = crate::handler::ListenerOptions::from_wert(args.get(2));
            w.event_listeners.remove(id, &typ, &callback, options.capture);
            Ok(Wert::Undefiniert)
        }
        // Release 10.10.2026: `el.click()` (HTML §6.4.5 „click()") — ein
        // `click`-Ereignis und danach das Standardverhalten (Dateifeld: der
        // Dialog, Link: die Navigation, Kaestchen: umschalten). Die Seite
        // fuehrt beides aus (Reaktion 6, `vorspann.js`). GEMESSEN vorher:
        // `click()` tat nichts — kein Ereignis, kein Hochlade-Dialog, kein
        // Download-Link per `a.click()`.
        "click" => {
            if let Some(l) = w.lebend.as_mut()
                && l.ce_schlange.len() < 1024
            {
                l.ce_schlange.push(crate::lebend::CeReaktion::Klick(id));
            }
            Ok(Wert::Undefiniert)
        }
        "focus" | "blur" => {
            // GEMESSEN: `focus` und `blur` loesen in diesem Browser nichts
            // aus; sie werden angenommen, damit ein Skript nicht scheitert.
            Ok(Wert::Undefiniert)
        }
        // The parser currently executes scripts after parsing the document.
        // Preserve fragment structure and append writes to the body in order.
        "write" => {
            if id != w.document_id { return Err("write: nur auf document verfuegbar".into()); }
            let html = args.iter().take(16).map(|a| a.als_text()).collect::<String>();
            if html.len() > MAX_ATTRIBUT { return Err("document.write: HTML zu lang".into()); }
            let body = w.standard.get("body").copied().ok_or("write: document.body fehlt")?;
            let nodes = narsil_fmt_html::parse_html(&html);
            fn body_children(nodes: &[narsil_fmt_html::HtmlNode]) -> Option<&[narsil_fmt_html::HtmlNode]> {
                for node in nodes {
                    if let narsil_fmt_html::HtmlNode::Element { tag, children, .. } = node {
                        if tag == "body" { return Some(children); }
                        if tag == "html" { if let Some(body) = body_children(children) {return Some(body);} }
                    }
                }
                None
            }
            let children = body_children(&nodes).unwrap_or(&nodes);
            schreibe_fragment(&mut w, body, children, 0)?;
            Ok(Wert::Undefiniert)
        }
        // --- document ------------------------------------------------------
        //
        // GEMESSEN, warum `document` hier behandelt wird und nicht in
        // `anbinden` als Host-Funktion: `document` ist ein WERT, keine
        // Funktion (siehe `Engine::host_global`). `document.body` liest
        // darum ueber den Getter, und `document.getElementById(...)`
        // ruft hier auf.
        //
        // GEMESSEN, warum `getElementById` EINE Methode ist und keine
        // globale Funktion: `document.getElementById` ist ein
        // Eigenschaftszugriff. Eine globale Funktion `getElementById`
        // gabe es nur ohne `document` davor — das ist der W6-Weg.
        "getElementById" => {
            let gesucht = args.first().map_or(String::new(), |a| a.als_text());
            // HTML-Spezifikation: `null`, wenn es kein Element gibt.
            match w
                .dom_ids
                .iter()
                .find(|(_, v)| v.as_str() == gesucht)
                .and_then(|(ord, _)| w.ids.get(ord).copied())
            {
                Some(host) => Ok(Wert::Gegenstand(host_feld(host))),
                None => Ok(Wert::Null),
            }
        }
        //
        // W6C: `querySelector`/`querySelectorAll` UEBER `narsil-css`.
        //
        // GEMESSEN, warum hier und nicht als globale Funktion (13:20):
        // `document.querySelectorAll` ist ein Eigenschaftszugriff auf
        // `document`. Als globale Funktion gabe es sie nur ohne `document`
        // davor — und sie gab ausserdem eine LEERE Liste zurueck (der Test
        // fand 0 `li` auf einer Seite mit drei).
        "querySelector" | "querySelectorAll" => {
            let selektor = args.first().map_or(String::new(), |a| a.als_text());
            let treffer = treffer_von(&w, &selektor);
            //
            // W6E: ORDNER -> HOST-ID (30.09.2026, 17:00).
            //
            // GEMESSENER FEHLER, Koordinationsbefund: `querySelector` gab
            // `host_feld(ord)` — die VORLAUF-NUMMER als Host-Id. Damit
            // bekam dasselbe Element zwei Identitaeten: `getElementById`
            // lieferte Host 3, `querySelector` Host 2 (gemessen an
            // `<h1 id="t">` mit `Stand::ids = {2: 3}`). GEMESSENE FOLGE:
            // `querySelector('h1').style.color = 'green'` schrieb in
            // `Stand::stil[2]`, und wer danach ueber `getElementById`
            // nachsah, fand nichts.
            //
            // GEMESSEN, warum `w.ids` und nicht eine eigene Tabelle: das ist
            // dieselbe Bruecke, die `getElementById` benutzt (siehe oben) —
            // eine zweite Bruecke waere die zweite Wahrheit, die sich
            // wieder zerstimmt.
            let als_objekt = |o: &Wert| -> Wert {
                match o {
                    // `Stand::ids` ist Vorlauf-Nummer (usize) -> Host-Id.
                    Wert::Zahl(n) => match w.ids.get(&(*n as usize)).copied() {
                        Some(host) => Wert::Gegenstand(host_feld(host)),
                        // Kein Host fuer diese Ordinalzahl: `null` wie in
                        // `getElementById` (HTML-Spezifikation).
                        None => Wert::Null,
                    },
                    _ => Wert::Null,
                }
            };
            if name == "querySelector" {
                return Ok(treffer.first().map(als_objekt).unwrap_or(Wert::Null));
            }
            Ok(Wert::Feld(treffer.iter().map(als_objekt).collect()))
        }
        // --- STRUKTUR ------------------------------------------------------
        //
        // W6F: `document.createElement('li')` (30.09.2026, 17:45).
        //
        // GEMESSEN, warum der Knoten hier gleich eine Vorlauf-Nummer
        // BEKOMMT und nicht erst beim `appendChild`: `li.textContent = …`
        // steht im Handler ZWISCHEN beiden. Der Text geht nach
        // `Stand::text`, und `schreiben` braucht die Nummer
        // (`w.ord[host]`) — ohne sie gaenge der Text an Ord 0.
        //
        // GEMESSEN, warum die Nummer aus `NeuKnoten::ORD_BASIS` kommt:
        // sie muss ueber alle Klicks gleich bleiben (sonst verliert der
        // Patch nach dem zweiten Klick seinen Knoten) und darf sich nie
        // mit einer Parse-Nummer treffen.
        "createElement" | "createElementNS" => {
            let ns = if name == "createElementNS" {
                args.first().filter(|v| !matches!(v, Wert::Null | Wert::Undefiniert))
                    .map(|v| v.als_text()).filter(|s| !s.is_empty())
            } else { Some("http://www.w3.org/1999/xhtml".to_string()) };
            let raw = args.get(if name == "createElementNS" { 1 } else { 0 })
                .map_or("undefined".to_string(), |v| v.als_text());
            if raw.len() > crate::js::MAX_TAG_LAENGE { return Err("Tagname zu lang".to_string()); }
            let (prefix, local) = if name == "createElementNS" {
                narsil_jsvm::namespace_name(ns.as_deref(), &raw).map_err(str::to_string)?
            } else { narsil_jsvm::element_name(&raw).map_err(str::to_string)?; (None, raw.clone()) };
            let tag = if name == "createElement" { raw.to_ascii_lowercase() } else { raw };
            if w.neu.len() >= narsil_page::NeuKnoten::MAX_NEU {
                return Err(std::format!(
                    "createElement: mehr als {} neue Knoten",
                    narsil_page::NeuKnoten::MAX_NEU
                ));
            }
            // Die naechste freie Host-Id (M3: `MAX_HOST_IDS`).
            if w.ids.len() >= crate::dom::MAX_HOST_IDS as usize {
                return Err("createElement: keine Host-Id mehr".to_string());
            }
            let host = w.ids.values().copied().max().map_or(1u32, |m| m + 1);
            let ord = narsil_page::NeuKnoten::ORD_BASIS + w.neu.len();
            w.neu.push((host, tag.clone()));
            w.ids.insert(ord, host);
            w.ord.insert(host, ord);
            w.tag.insert(host, tag.clone());
            w.namespaces.insert(host, (ns, prefix, if name == "createElement" { local.to_ascii_lowercase() } else { local }));
            w.befehle.push(Befehl::Neu {
                knoten: ord as u32,
                tag,
            });
            Ok(Wert::Gegenstand(host_feld(host)))
        }
        // W6F: `document.getElementById('liste').appendChild(li)`.
        //
        // GEMESSEN, warum `Anhaengen` trotzdem ein Befehl ist: der neue
        // Knoten steht im STAND, und der Patch entsteht bei jedem Klick
        // neu aus ihm. Der Befehl ist das LOG (der Nutzer soll sehen, was
        // passiert) und der Anker fuer `replay`.
        "insertAdjacentHTML" | "insertAdjacentText" | "insertAdjacentElement" => {
            let wo = args.first().map(|a| a.als_text().to_ascii_lowercase()).unwrap_or_default();
            let (eltern, lage) = match wo.as_str() {
                "beforebegin" => (eltern_host(&w, id).ok_or("insertAdjacent: kein Elternknoten")?, Some(HostLage::Vor(id))),
                "afterbegin" => (id, Some(HostLage::Anfang)),
                "beforeend" => (id, None),
                "afterend" => (eltern_host(&w, id).ok_or("insertAdjacent: kein Elternknoten")?, Some(HostLage::Nach(id))),
                _ => return Err(std::format!("SyntaxError: {name}: unbekannte Lage {wo:?}")),
            };
            match name {
                "insertAdjacentHTML" => {
                    let html = args.get(1).map(|a| a.als_text()).unwrap_or_default();
                    if html.len() > MAX_ATTRIBUT {
                        return Err("insertAdjacentHTML: HTML zu lang".into());
                    }
                    let kinder = fragment_inhalt(narsil_fmt_html::parse_html(&html));
                    fragment_an_lage(&mut w, eltern, lage, &kinder)?;
                    Ok(Wert::Undefiniert)
                }
                "insertAdjacentText" => {
                    let text = args.get(1).map(|a| a.als_text()).unwrap_or_default();
                    fragment_an_lage(&mut w, eltern, lage, &[narsil_fmt_html::HtmlNode::Text(text)])?;
                    Ok(Wert::Undefiniert)
                }
                _ => {
                    let el = args.get(1).cloned().unwrap_or(Wert::Undefiniert);
                    knoten_an_lage(&mut w, eltern, lage, std::slice::from_ref(&el))?;
                    Ok(el)
                }
            }
        }
        "prepend" | "append" | "before" | "after" => {
            let (eltern, lage) = match name {
                "prepend" => (id, Some(HostLage::Anfang)),
                "append" => (id, None),
                "before" => match eltern_host(&w, id) {
                    Some(e) => (e, Some(HostLage::Vor(id))),
                    None => return Ok(Wert::Undefiniert),
                },
                _ => match eltern_host(&w, id) {
                    Some(e) => (e, Some(HostLage::Nach(id))),
                    None => return Ok(Wert::Undefiniert),
                },
            };
            knoten_an_lage(&mut w, eltern, lage, args)?;
            Ok(Wert::Undefiniert)
        }
        "appendChild" => {
            let kind = args.first().cloned().unwrap_or(Wert::Undefiniert);
            let kind_host = narsil_jsvm::host_id(&kind).ok_or_else(|| {
                std::format!(
                    "appendChild: {} ist kein Element (in einem Browser: TypeError)",
                    kind.als_text()
                )
            })?;
            // Der Knoten muss aus `createElement` stammen.
            if !w.neu.iter().any(|(h, _)| *h == kind_host) {
                return Err(
                    "appendChild: der Knoten wurde nicht mit createElement erzeugt".to_string(),
                );
            }
            let kind_ord = w.ord.get(&kind_host).copied().unwrap_or(0) as u32;
            w.eltern.insert(kind_host, id);
            w.befehle.push(Befehl::Anhaengen {
                eltern: ord,
                kind: kind_ord,
            });
            Ok(Wert::Gegenstand(host_feld(kind_host)))
        }
        // --- STRUKTUR ------------------------------------------------------
        //
        // GEMESSEN, warum diese zwei hier „leer" sind: sie brauchen einen
        // Knoten, den es im geparsten Baum noch nicht gibt. Sie liefern
        // eine LEERE Liste statt eines Fehlers — mit Namen im Befehls-
        // Log (siehe `narsil::replay::spielen`).
        "querySelectorAll" | "getElementsByTagName" => Ok(Wert::Feld(Vec::new())),
        other => Err(std::format!(
            "Methode '{other}' gibt es in diesem Browser nicht"
        )),
    }
}
// ===========================================================================
// DIE ZWEI GLOBALEN (`document`, `window`)
// ===========================================================================

/// Die ELEMENT-Kinder eines Knotens — aus dem CSS-Baum.
///
/// REPARIERT 05.10.2026. Vorher gab `children` IMMER eine leere Liste
/// zurueck (ein Platzhalter). GEMESSEN an `example.com`: dessen Skript
/// beginnt mit `document.body.children[0].insertAdjacentHTML(…)` — bei
/// leerer Liste ist `children[0]` `undefined`, und der Zugriff darauf
/// beendete das Skript mit `TypeError: kein Zugriff auf Undefiniert`.
/// Der ganze Rest des Skripts (fuenf Uebersetzungen, das SVG) lief nicht.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Quelle der **CSS-Baum** ist und
/// keine neue Kindtabelle: `CssKnoten` traegt bereits `eltern`
/// (`narsil-page/src/kaskade.rs:129`), und `Stand::css_zu_ord` verbindet
/// CSS-Index und Vorlauf-Nummer — dieselbe Bruecke, die
/// `querySelectorAll` schon benutzt. Eine zweite Eltern-Kind-Tabelle im
/// `Stand` waere eine zweite Wahrheit ueber denselben Baum.
///
/// Die Reihenfolge ist die Dokumentreihenfolge, weil `knoten` in
/// Dokumentreihenfolge aufgebaut wird.
/// Der Elternknoten (Host-Id) — eines neuen Knotens aus `Stand::eltern`,
/// eines geparsten aus dem CSS-Baum (Release 08.10.2026: vorher lieferte
/// `parentNode` fuer jedes geparste Element `null`).
pub fn eltern_host(w: &Stand, id: u32) -> Option<u32> {
    if let Some(e) = w.eltern.get(&id) {
        return Some(*e);
    }
    let ord = w.ord.get(&id).copied()?;
    let (css, _) = w.css_zu_ord.iter().find(|(_, o)| **o == ord)?;
    let eltern_css = w.css_baum.knoten.get(*css as usize)?.eltern?;
    let eltern_ord = w.css_zu_ord.get(&eltern_css)?;
    w.ids.get(eltern_ord).copied()
}

/// Ein Fragment an eine Lage schreiben: die obersten Knoten bekommen die
/// Lage, ihre Kinder haengen wie gewohnt an ihnen.
fn fragment_an_lage(
    w: &mut Stand,
    eltern: u32,
    lage: Option<HostLage>,
    nodes: &[narsil_fmt_html::HtmlNode],
) -> Result<(), String> {
    let vorher = w.neu.len();
    schreibe_fragment(w, eltern, nodes, 0)?;
    if let Some(lage) = lage {
        let neue: Vec<u32> = w.neu[vorher..]
            .iter()
            .map(|(h, _)| *h)
            .filter(|h| w.eltern.get(h) == Some(&eltern))
            .collect();
        for h in neue {
            w.lage.insert(h, lage);
        }
    }
    Ok(())
}

/// Die Knoten aus `prepend/append/before/after`: Texte werden Textknoten,
/// mit `createElement` erzeugte Elemente werden eingehaengt. Ein schon
/// geparstes Element zu VERSCHIEBEN kann der Patch nicht — das wird
/// uebergangen statt das ganze Skript abzubrechen.
fn knoten_an_lage(
    w: &mut Stand,
    eltern: u32,
    lage: Option<HostLage>,
    args: &[narsil_jsvm::Wert],
) -> Result<(), String> {
    for a in args.iter().take(64) {
        match narsil_jsvm::host_id(a) {
            Some(kind) if w.neu.iter().any(|(h, _)| *h == kind) => {
                let kind_ord = w.ord.get(&kind).copied().unwrap_or(0) as u32;
                let eltern_ord = w.ord.get(&eltern).copied().unwrap_or(0) as u32;
                w.eltern.insert(kind, eltern);
                match lage {
                    Some(l) => {
                        w.lage.insert(kind, l);
                    }
                    None => {
                        w.lage.remove(&kind);
                    }
                }
                w.befehle.push(Befehl::Anhaengen { eltern: eltern_ord, kind: kind_ord });
            }
            Some(_) => {}
            None => {
                let text = a.als_text();
                fragment_an_lage(w, eltern, lage, &[narsil_fmt_html::HtmlNode::Text(text)])?;
            }
        }
    }
    Ok(())
}

/// Der Inhalt eines geparsten Fragments: `<body>`-Kinder, dazu `<head>`-
/// Inhalt (ein vorangestelltes `<style>` landet dort).
fn fragment_inhalt(nodes: Vec<narsil_fmt_html::HtmlNode>) -> Vec<narsil_fmt_html::HtmlNode> {
    for node in &nodes {
        if let narsil_fmt_html::HtmlNode::Element { tag, children, .. } = node {
            if tag == "body" {
                return children.clone();
            }
            if tag == "html" {
                let mut alle = Vec::new();
                for k in children {
                    if let narsil_fmt_html::HtmlNode::Element { tag, children, .. } = k
                        && (tag == "head" || tag == "body")
                    {
                        alle.extend(children.iter().cloned());
                        continue;
                    }
                    alle.push(k.clone());
                }
                return alle;
            }
        }
    }
    nodes
}

pub fn kinder_von(w: &Stand, id: u32) -> Vec<narsil_jsvm::Wert> {
    use narsil_jsvm::Wert;
    let Some(ord) = w.ord.get(&id).copied() else {
        return Vec::new();
    };
    // Vorlauf-Nummer -> CSS-Index (die Umkehrung von `css_zu_ord`).
    let Some((css, _)) = w.css_zu_ord.iter().find(|(_, o)| **o == ord) else {
        return Vec::new();
    };
    let mut raus = Vec::new();
    for (i, k) in w.css_baum.knoten.iter().enumerate() {
        if k.eltern != Some(*css) {
            continue;
        }
        if let Some(o) = w.css_zu_ord.get(&(i as u32))
            && let Some(host) = w.ids.get(o)
        {
            raus.push(Wert::Gegenstand(host_feld(*host)));
        }
    }
    raus
}

/// Die Treffer eines Selektors — ueber `narsil_css::selektor_passt`.
///
/// GEMESSEN, warum ueber `narsil-css`: es ist die EIN Selektorpruefung des
/// Projekts (sie entscheidet auch die Kaskade). Eine eigene Pruefung in
/// `narsil` waere eine zweite Wahrheit: sie wuerde bei `div > p` anders
/// entscheiden als der Rest des Browsers.
///
/// DIE GRUENZEN (M3/M4): Selektortext 512 Byte, 4096 Elemente, 1024
/// Treffer. Der Rueckgabewert ist eine Liste von ZAHLEN (die
/// Vorlauf-Nummer) — beim Zugriff wird daraus ein Element (`host_feld`).
pub fn treffer_von(w: &Stand, selektor: &str) -> Vec<narsil_jsvm::Wert> {
    use narsil_jsvm::Wert;
    if selektor.len() > crate::js::MAX_SELECTOR {
        return Vec::new();
    }
    // Der Selektor ist der Kopf einer Regel; `stylesheet_lesen` ist der
    // oeffentliche Einstieg von `narsil-css`.
    let blatt = narsil_css::stylesheet_lesen(&std::format!("{selektor} {{--q:1}}"));
    let schranke = w.css_baum.knoten.len().min(crate::js::MAX_SCAN);
    let mut raus: Vec<Wert> = Vec::new();
    for regel in blatt.regeln.iter().take(crate::js::MAX_REGELN) {
        // `@media`-Bloecke werden nicht ausgewertet: in einem Skript gibt
        // es kein Viewport, an dem man sie messen koennte.
        let narsil_css::Regel::Stil(stil) = regel else {
            continue;
        };
        for sel in stil.selektoren.iter().take(crate::js::MAX_SELECTOREN) {
            for id in 0..schranke {
                if narsil_css::selektor_passt(
                    sel,
                    &narsil_page::El {
                        baum: &w.css_baum,
                        id: id as u32,
                    },
                ) && let Some(ord) = w.css_zu_ord.get(&(id as u32))
                {
                    raus.push(Wert::Zahl(*ord as f64));
                    if raus.len() >= crate::js::MAX_TREFFER {
                        return raus;
                    }
                }
            }
        }
    }
    raus
}

/// Register inserted nodes in the same host/ordinal maps as createElement.
fn schreibe_fragment(w: &mut Stand, parent: u32, nodes: &[narsil_fmt_html::HtmlNode], depth: usize) -> Result<(), String> {
    if depth > 64 {return Err("document.write: Fragment zu tief".into());}
    for node in nodes {
        if w.neu.len() >= narsil_page::NeuKnoten::MAX_NEU {return Err("document.write: Knotengrenze erreicht".into());}
        let host = w.tag.keys().filter(|id| **id < MAX_HOST_IDS).copied().max().unwrap_or(0)+1;
        if host >= MAX_HOST_IDS {return Err("document.write: Host-Ids voll".into());}
        let ord = narsil_page::NeuKnoten::ORD_BASIS+w.neu.len();
        let (tag, text) = match node {
            narsil_fmt_html::HtmlNode::Text(text) => ("span".to_string(), Some(text.clone())),
            narsil_fmt_html::HtmlNode::Element {tag, ..} => (tag.clone(), None),
        };
        w.neu.push((host,tag.clone()));w.ids.insert(ord,host);w.ord.insert(host,ord);
        w.tag.insert(host,tag.clone());w.eltern.insert(host,parent);
        w.namespaces.insert(host,(Some("http://www.w3.org/1999/xhtml".into()),None,tag.clone()));
        w.befehle.push(Befehl::Neu{knoten:ord as u32,tag});
        if let Some(text)=text {w.text.insert(host,text);}
        if let narsil_fmt_html::HtmlNode::Element {attributes,children,..}=node {
            w.attribute.insert(host,attributes.clone());
            for (name,value) in attributes {
                if name=="id" {w.dom_ids.insert(ord,value.clone());}
                w.befehle.push(Befehl::Attribut{knoten:ord as u32,name:name.clone(),wert:value.clone()});
            }
            schreibe_fragment(w,host,children,depth+1)?;
        }
        w.befehle.push(Befehl::Anhaengen{eltern:w.ord.get(&parent).copied().unwrap_or(0) as u32,kind:ord as u32});
    }
    Ok(())
}

/// Meldet `document` und `window` als globale Host-Objekte.
///
/// # WARUM BEIDE, und nicht nur `document`
///
/// Document and Window share page state but have separate identities and
/// listener targets. The Window wrapper also mirrors VM globals.
pub fn globale_setzen(engine: &mut narsil_jsvm::Engine, stand: Geteilt) {
    let doc_id = stand.lock().expect("DomStand").document_id;
    //
    // GEMESSEN, warum `host_global` und nicht `host_funktion` (14:45):
    // eine Funktion als `document` hat keine DOM-Eigenschaften.
    for name in ["document", "window"] {
        let host_id = if name == "window" { WINDOW_HOST_ID } else { doc_id };
        let mut felder = vec![(
            MERKE.to_string(),
            narsil_jsvm::Wert::Zahl(host_id as f64),
        )];
        //
        // J-CACHE (05.10.2026): `window` SPIEGELT den globalen Bereich.
        //
        // GEMESSEN: `window.setTimeout(...)` war tot, weil das Host-Objekt
        // die Engine-Globals nicht kannte (www.wikipedia.org: „setTimeout is
        // not a function" x3). Die Engine antwortet fuer ein Objekt mit der
        // Marke `__global` mit ihren eigenen Globals
        // (`narsil-jsvm/src/eigenschaften.rs:227`). Und die Gegenrichtung
        // (`window.ytcfg = …` -> `ytcfg` als globale Variable) liest der
        // Motor ueber `global_aus_window_host` (`interp.rs`).
        //
        // `document` bekommt die Marke NICHT: `document.setTimeout` gibt es
        // in keinem Browser.
        if name == "window" {
            felder.push(("__global".to_string(), narsil_jsvm::Wert::Wahr(true)));
        }
        let v = narsil_jsvm::Wert::Gegenstand(felder);
        // Der Fehler wird hier NICHT propagiert: `document` fehlt dann,
        // und jedes Skript meldet es selbst als ReferenceError.
        let _ = engine.host_global(name, v);
    }
    // Die Methoden von `document` als globale Funktionen — sie sind die
    // Haken, die der Getter als Objekt zurueckgibt, und der Aufruf
    // laeuft ueber `host_methode_von_gval`.
    let _ = stand;
}

#[cfg(test)]
mod form_kodieren_tests {
    use super::form_kodieren;
    use narsil_fmt_html::kodierung::Kodierung;

    /// Release 12.10.2026: urlencoded in der Formularkodierung (WPT
    /// encoding/*-encode-form*): euc-kr-Bytes, `&#N;` fuer Unkodierbares,
    /// Leerzeichen `+`, CRLF, und die Zeichen ausserhalb des Satzes.
    #[test]
    fn euc_kr_utf8_und_fehlermodus_html() {
        assert_eq!(form_kodieren("\u{A1} a", Kodierung::EucKr), "%A2%AE+a");
        assert_eq!(form_kodieren("\u{1F600}", Kodierung::EucKr), "%26%23128512%3B");
        assert_eq!(form_kodieren("a\nb\rc\r\nd", Kodierung::Utf8), "a%0D%0Ab%0D%0Ac%0D%0Ad");
        assert_eq!(form_kodieren("*-._~!'()", Kodierung::Utf8), "*-._%7E%21%27%28%29");
        assert_eq!(form_kodieren("\u{E9}", Kodierung::Utf8), "%C3%A9");
    }
}

/// Release 2026-10-14: is `id` an element of an XML document? Then
/// `innerHTML`/`outerHTML` belong to the prelude (XML serialization and the
/// XML fragment parser, DOM Parsing §2.1/§2.2). A detached element asks its
/// `ownerDocument` (a clone of an XML element stays XML).
/// MEASURED before (WPT domparsing innerhtml-01/-03/-05.xhtml 2/11): HTML
/// serialization (`<br>`, no `xmlns`), no error for malformed markup.
fn xml_element(l: &mut crate::lebend::Lebend, id: u32) -> bool {
    if l.dok.xml_typ.is_empty() {
        return false;
    }
    let Some(k) = l.knoten(id) else { return false };
    if !l.dok.ist_element_knoten(k) {
        return false;
    }
    let w = l.dok.wurzel_von(k);
    let doc = if matches!(l.dok.knoten.get(w).map(|n| &n.art), Some(narsil_fmt_html::dom::Art::Dokument)) {
        Some(w)
    } else {
        match crate::lebend::lesen(l, id, "ownerDocument") {
            Some(narsil_jsvm::Wert::Gegenstand(fields)) => fields
                .iter()
                .find(|(n, _)| n == MERKE)
                .and_then(|(_, v)| match v {
                    narsil_jsvm::Wert::Zahl(z) => Some(*z as u32),
                    _ => None,
                })
                .and_then(|h| l.knoten(h)),
            _ => None,
        }
    };
    doc.is_some_and(|d| l.dok.xml_typ.get(&d).is_some_and(|t| t != "text/html"))
}
