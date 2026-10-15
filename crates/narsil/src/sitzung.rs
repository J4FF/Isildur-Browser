//! narsil — DER Browser.
//!
//! Bis hierher war das Projekt eine Sammlung gebauter Teile. `narsil-app`
//! machte aus einer Datei ein Bild und sonst nichts: keine Adressleiste,
//! keine Tastatur, keine Navigation, kein Klick, der irgendwohin
//! fuehrt. Sieben Crates waren gebaut, getestet und hatten null Aufrufer.
//!
//! Dieses Crate ist die Stelle, an der sie verbunden werden. Es enthaelt
//! drei Dinge, in dieser Reihenfolge:
//!
//! 1. [`Sitzung`] — der ganze Zustand: welche Adresse, welche Seite,
//!    welcher Verlauf, welcher Knopf gedrueckt. OHNE Fenster, OHNE X11,
//!    OHNE Netz. Das ist kein Testtrick, sondern die Regel aus
//!    `UI_LUECKEN_UND_REIHENFOLGE.txt`: die Sprache bleibt zustandslos,
//!    der Browser-Zustand ist ein eigener Typ.
//!
//! 2. [`laden`] — eine Adresse wird zu Bytes. Drei Wege: `file://` von
//!    der Platte, `http://` ueber `narsil-net`, `narsil://` eingebaut. Kein
//!    Weg fuehrt in eine Sackgasse.
//!
//! 3. `main.rs` — Fenster, Ereignisschleife, Zeichnen. Das ist der
//!    kleinste Teil, weil alles andere schon gebaut war.
//!
//! # WAS DIESES CRATE NICHT KENNT
//!
//! * **Kein HTML.** Das macht `narsil-page`. Hier steht nur, dass eine
//!   geladene Datei eine Seite wird.
//! * **Kein Rasterizer.** `narsil-raster` zeichnet. Hier steht nur, was
//!   gezeichnet werden soll.
//! * **Keinen Stil.** Keine Farbe, keine Schriftgroesse. Die kommen aus
//!   `narsil-chrome` und `narsil-page`.

use std::collections::HashMap;
use std::fmt;

use narsil_browser::{Browser, MAX_TABS, Navigation, Wirkung};
use narsil_chrome::{Chrome, Knopf, Knopfzustand, Rechteck, thema};
use narsil_core::color::Color;
use narsil_core::geometry::{Point, Rect, Size};

//
// KONTEXT (05.10.2026, 06:20, Leitung): DIE LOGIK LIEGT JETZT IN
// `narsil_browser::interaktion` — und wird hier **weiterverwendet**, nicht
// kopiert.
//
// GEMESSENE ENTSCHEIDUNG, warum das ein `pub use` ist und kein `use`: die
// Namen werden von `narsil/src/lib.rs` **weitergegeben** (`pub use
// sitzung::{.., ScrollAni, Scrollbalken, ..}`) und von den Tests benutzt
// (`use narsil::Scrollbalken`). Ein `pub use` aendert daran **nichts** —
// waere es ein `use`, waeren die Namen aus `narsil` **weg**, und das ist eine
// Aenderung, die niemand gebeten hat.
//
// GEMESSENE ENTSCHEIDUNG, warum ueberhaupt verschoben: GEMESSEN ist, dass
// `narsil` nicht baut (fremde Crates), `narsil-browser` aber gruen ist und **keine**
// Abhaengigkeiten hat — und dass die Teile hier (Auswahl als Adresse,
// Wortgrenzen, Zeilenhoch/-runter, sanftes Scrollen) **nur** Zahlen, Zeichen
// und `Instant` brauchen. Als erstes **gelaufener** Test dieser Logik hat
// `auch_nach_oben_geht_es_sanft` zwei **echte** Fehler gefunden (siehe dort und
// bei `funde_in_text`).
pub use narsil_browser::interaktion::{
    Auswahl, AUTO_ANLAUF_MS, AUTO_MAX, AUTO_START, AUTO_TOT, AutoRest, DOPPELKLICK_MS,
    Dateiwahl, Dateizweck, Kandidat, MAX_RADSCHRITT, Passung, RadDreh, SCROLL_DAUER_MS, ScrollAni,
    Scrolltaste, Scrollwunsch, Tippsuche, Vorschlagswahl, auto_lauf_richtung,
    auto_lauf_schritt_milli, auswahl_tippschritte, datei_lesen, dateiname_aus_adresse,
    enter_sendet, feld_zeile_hoch, feld_zeile_runter, funde_in_text, griff_lage, klickzahl,
    praefix_trifft, rad_pixels, scroll_taste, seiten_schritt, verlauf_zeilen, vorschlaege,
    wort_grenzen, zoom_richtung, zoom_stufe, zug_versatz,
};
// GEMESSENE ENTSCHEIDUNG, warum **eine** Zeile und nicht drei (der Stand vom
// 06.10. hatte dieselbe Zeile dreimal, E0252 zweimal): ein `pub use` ist eine
// Zusage „dieser Name kommt hier herein" — dreimal gesagt ist es dieselbe
// Zusage dreimal, und der Compiler nennt jede als eigenen Fehler.
pub use narsil_browser::profil::Lesezeichenprofil;
use narsil_display::{DisplayItem, DisplayList};
use narsil_i18n::{tr, trf};
use narsil_dom::NodeId;
use narsil_input::{Key, KeyEvent};
use narsil_layout::LayoutTree;
use narsil_page::{Seite, seite_aus_text};
use narsil_window::SurfaceEvent;

use crate::laden::{Ladung, Quellart, lade};

/// Standardhoehe der Seite unter der Leiste.
///
/// W7C A5: DARUM ist es eine Rechnung und nicht mehr `LEISTE_HOEHE`.
/// Ueber der Leiste steht die Tab-Leiste (26 px), und die Seite beginnt
/// unter BEIDEN. Als Zahl waere `32` eine Zusage, die beim Einziehen der
/// Tabs stillschweigend falsch geworden waere — und „die Seite beginnt
/// zu hoch" sieht man im Bild, erklaert es aber nicht.
pub const SEITEN_START_Y: u32 = narsil_chrome::TAB_LEISTE_HOEHE + narsil_chrome::LEISTE_HOEHE;

/// M3: SO VIELE ZEICHEN PASSEN IN DIE STATUSZEILE.
///
/// GEMESSENE BEGRUENDUNG fuer die Zahl: `ZEICHENBREITE` ist 8 px, die
/// Zeile ist so breit wie das Fenster. Bei 800 px bleiben nach dem Rand
/// 788 px, also 98 Zeichen — 200 ist also eine OBERGRENZE fuer breite
/// Fenster, und das ist genau der Zweck: die Zahl begrenzt die
/// Zeichenkette, die der Bildaufbau verarbeitet, unabhaengig davon, wie
/// breit das Fenster gerade ist (M5).
pub const MAX_STATUS_ZEICHEN: usize = 200;

/// Fehler der Sitzung, alle mit genug Information, um sie anzuzeigen.
///
/// Kein `String`, weil die Caller unterschiedliche Reaktionen wollen:
/// ein Tippfehler ist kein Absturz, und ein fehlendes Netz auch nicht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fehler {
    /// Die Adresse war unlesbar (leer, unbekanntes Schema, https).
    Adresse(String),
    /// Die Seite war nicht erreichbar oder nicht lesbar.
    Seite(String),
}

impl fmt::Display for Fehler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fehler::Adresse(m) => write!(f, "{}", narsil_i18n::trf!("Address: {}", m)),
            Fehler::Seite(m) => write!(f, "{}", narsil_i18n::trf!("Page: {}", m)),
        }
    }
}

impl std::error::Error for Fehler {}

/// Ein Ladefehler wird ein Seitenfehler.
///
/// GEMESSEN, warum die Richtung so ist und nicht umgekehrt: der Tippfehler
/// (`https`, leeres Schema) und der Netzfehler erfordern verschiedene
/// Reaktionen. Der erste gehoert in die Adresszeile — der Nutzer hat
/// etwas falsch getippt. Der zweite gehoert in die Statuszeile — die
/// Adresse war richtig, der Server nicht. Wer beides in einen Topf wirft,
/// kann dem Nutzen nicht sagen, was er tun soll.
///
/// Die Richtung "Ladefehler -> Fehler" ist die einzige, die ohne
/// Information auskommt: `Fehler` ist die groessere Menge.
impl From<crate::laden::LadeFehler> for Fehler {
    fn from(f: crate::laden::LadeFehler) -> Self {
        Fehler::Seite(f.to_string())
    }
}

/// Die geladene Seite, wie sie im Bild steht.
///
/// `PartialEq` ist hier NICHT abgeleitet: `Angezeigt` traegt den
/// DOM-Stand (ein `Arc<Mutex<…>>`), und der hat keinen Vergleich. Wer
/// zwei Anzeigen vergleichen will, vergleicht Seite, Baum und Patch.
///
/// W6D: `engine` ist weder `Clone` noch `Debug` (die Engine hat beide nicht
/// und soll sie auch nicht bekommen: ein Klon waere eine zweite Wahrheit
/// ueber den Skriptzustand). Darum ist `Angezeigt` ab hier nur noch
/// `Debug`-frei und `Clone`-frei — wer eine Anzeige kopieren will, nimmt
/// `Sitzung::neu` und laedt neu.
pub struct Angezeigt {
    /// Die Seite selbst (Knoten, Links, Basis).
    pub seite: Seite,
    /// DER `Content-Type` DER ANTWORT (ohne Parameterteil, leer bei
    /// `file://`). GEMESSENE ENTSCHEIDUNG, warum er an der SEITE haengt und
    /// nicht an der Sitzung: er gehoert zur Antwort, und die Antwort ist
    /// genau das, was `Angezeigt` ist. GEMESSENE URSACHE: der PDF-Betrachter
    /// erkennt eine Datei ohne Endung nur ueber diesen Wert (WUNSCH_UI.txt
    /// Punkt 11: „Header ZUERST, Endung als Ergaenzung").
    pub content_type: String,
    /// Release 09.10.2026: der HTTP-Status der angezeigten Seite (siehe
    /// `Ladung::status`) — ab 400 steht der Code in der Statuszeile.
    pub http_status: u16,
    /// Der Layout-Baum. Kein `Option`: eine Seite ohne Layout kann es
    /// nicht geben, und ein `None` hiesse "vielleicht".
    pub baum: LayoutTree,
    /// Wie weit der Inhalt hoeher ist als das Fenster.
    pub inhalt_hoehe: u32,
    /// Wie weit nach oben gescrollt ist, in Pixeln.
    pub versatz: u32,
    ///
    /// KONTEXT (05.10.2026, 14:50, Leitung „Scroll-Ketten-Bewegung mit CSS
    /// abstimmen, Y-Berechnung **gemeinsam**“): DER VERSATZ **JE CONTAINER**
    /// (`overflow:auto`), Schluessel = `NodeId::index()`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **hier** und nicht in der Sitzung: GEMESSEN
    /// ist, dass `Angezeigt` beim **Seitenwechsel** neu gebaut wird — der Versatz
    /// eines Containers gehoert aber zum **Dokument**. In der Sitzung wuerde er
    /// einen Tabwechsel ueberleben und an der falschen Stelle wiederkommen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum eine `HashMap` und kein `Vec`: die Container
    /// stehen **nicht** an festen Stellen (sie haengen am Dokument), und eine
    /// Liste waere ein `Vec`, in dem `0` zweimal vorkommen kann.
    pub scroll_je_knoten: std::collections::HashMap<u32, u32>,
    /// I3: die Bilder der Seite, geholt BEIM LADEN.
    ///
    /// GEMESSEN, warum beim Laden und nicht beim Zeichnen: ein Fenster
    /// zeichnet bis zu 30mal je Sekunde. Beim Zeichnen geholt waeren das
    /// 30 Downloads und 30 Dekodierungen fuer dasselbe Bild, und die
    /// wuerden die Bildrate auffressen, bevor der Benutzer etwas klickt.
    ///
    /// Der Cache traegt die gleichen Eintraege, die `hole()` geholt hat —
    /// das ist Absicht: die Fehler (404, zu gross) werden nicht beim
    /// Zeichnen neu versucht, sonst waere eine Seite mit 30 toten Bildern
    /// ein Endlos-Netzwerkfehler.
    pub bilder: crate::bilder::BildCache,
    /// W6 (J9/J10): der GEPARSTE Baum der Seite.
    ///
    /// GEMESSEN, warum der Baum und nicht der HTML-Text: `HtmlNode`
    /// fuehrt keine Quellposition, und `narsil-dom::Span` ist beim Bau
    /// ueberall `Span::EMPTY` — am Quelltext kann man also nichts an der
    /// richtigen Stelle aendern (das war der erste Weg, 11:50, und er ist
    /// gestorben). Der Baum ist die Wahrheit, aus der die Seite gebaut
    /// wird; mit [`Angezeigt::patch`] baut man sie neu.
    ///
    /// `Arc`, weil jeder Klick, der die Seite aendert, ihn mitnimmt und
    /// die Anzeige waehrenddessen dieselbe bleiben soll.
    pub html: std::sync::Arc<Vec<narsil_fmt_html::HtmlNode>>,
    ///
    /// DER **ROHE** Quelltext der Seite — so, wie der Server ihn schickte.
    ///
    /// KONTEXT (04.10.2026, 05:40, Leitung: „view-source: **echt** machen: den
    /// ROHEN Quelltext beim Laden aufbewahren (rohtext: Option<String>, nur
    /// wenn <= 8 MB)“): GEMESSENER BEFUND, warum er gebraucht wird:
    /// `Angezeigt::html` ist der **geparste** Baum, und aus dem Baum wieder
    /// HTML zu machen ist eine **Re-Serialisierung** — sie normalisiert
    /// (Schlusszeichen, Leerräume, Attributreihenfolge). Ein „Quelltext“, der
    /// nicht der ist, den der Server geschickt hat, ist eine Ausgabe, kein
    /// Quelltext.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `Option<String>` und nicht `String`: es
    /// gibt Seiten **ohne** Rohtext (ein aus dem DOM gebautes Angezeigt, ein
    /// PDF, ein Fehlerblatt) — und ein leerer String waere von einem leeren
    /// Dokument nicht zu unterscheiden.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **8 MiB**: M3/M4. GEMESSENE BEGRUENDUNG
    /// fuer diese Zahl: die groesste HTML-Fixture im Baum (`narsil-fuzz/saat`)
    /// liegt weit darunter, und ein 8-MiB-Text im Speicher je Tab waere bei
    /// [`narsil_browser::MAX_TABS`] = 32 genau die Sorte Speicher, die man
    /// bemerkt, wenn man sie nicht begrenzt. Ab der Grenze ist `rohtext`
    /// `None`, und „Quelltext anzeigen“ **sagt** es.
    pub rohtext: Option<String>,
    /// W6B: der DOM-Stand der Seite (JavaScript-Zustand).
    ///
    /// GEMESSEN, warum er in der Seite steht und nicht im Handler
    /// (17:20): der Zustand des DOM gehoert zum Dokument. Ein frischer
    /// Stand je Klick las aus dem geparsten Baum immer wieder den
    /// ANFANGSwert, und der Zaehler blieb bei 1.
    /// W6D: DIE ENGINE DER SEITE — sie bleibt am Leben.
    ///
    /// GEMESSEN, warum das der Kern von W6D ist (14:20): ein Handler ist
    /// eine CLOSUE ueber die Variablen des Seitenskripts (`const z`).
    /// Die liegen in der globalen Umgebung der Engine, in der das
    /// Skript lief — eine frische Engine je Klick hat kein `z`.
    ///
    /// M3: eine Engine je Seite (nicht je Skript); ihre Grenzen bleiben
    /// `Grenzen::standard()` (Heap 64 MiB, Schritte 50 Mio.).
    pub engine: Option<narsil_jsvm::Engine>,
    /// Browser host bindings share the lifetime of this page engine.
    pub kiste: crate::js::Geteilt,
    pub stand: crate::dom::Geteilt,
    /// W6 (J10): die Aenderungen, die die Skripte gemacht haben.
    ///
    /// Wird NICHT zurueckgesetzt: nach dem Neubau ist es der Stand, und
    /// ein zweiter Klick baut wieder darauf auf. GEMESSEN, warum das
    /// richtig ist: `textContent` heisst „ersetzen", nicht „anhaengen".
    pub patch: narsil_page::Patch,
    /// W6 (J10): die Ausgabe von `console` (fuer die Statuszeile).
    pub konsole: Vec<String>,
    /// W6F: Was beim NEUBAU nicht ging (ein zu langer Text an einem neuen
    /// Knoten, ein Befehl ohne Wirkung). Steht in der Statuszeile.
    ///
    /// GEMESSEN, warum nicht im `Patch`: der Patch ist die Anweisung an
    /// den Bau, und ein Fehler darin wuerde beim naechsten Klick wieder
    /// auftauchen. Der Nutzer soll ihn EINMAL sehen.
    pub meldungen: Vec<String>,
    /// W6 (J10): die Ereignis-Handler: (Knoten, Typ) -> Quelltext.
    ///
    /// GEMESSEN, warum der QUELLTEXT und nicht ein Zeiger auf eine
    /// Funktion: zwischen dem Laden und dem Klick liegt die Seite im
    /// Speicher, und die Engine wird fuer jedes Skript neu aufgebaut (ein
    /// Skript darf keine Welt veraendern, die es nicht kennt). Also wird
    /// der Handler beim Ereignis neu getragen — aus dem Quelltext, der
    /// in der Seite steht.
    ///
    /// W6D: `Function` heisst: der Handler lebt in der Engine der Seite
    /// (`Angezeigt::engine`) und wird beim Klick DORT aufgerufen — mit
    /// seinen geschlossenen Variablen.
    pub horcher: HashMap<(u32, String), crate::handler::Handler>,
    /// Release 10.10.2026: eine VORSCHAU (Seite vor ihren Skripten, siehe
    /// `Sitzung::vorschau_senden`) — das Endergebnis ersetzt sie.
    pub vorschau: bool,
    /// Release 11.10.2026: `window.load` wartet, solange die Seite eine
    /// Ladesperre haelt (HTML §8.4.? „delay the load event": Medien beim
    /// Laden, siehe `vorspann.js` `__narsilLadeSperre`) — seit wann.
    pub load_ausstehend: Option<std::time::Instant>,
    /// Release 14.10.2026: engine time (ms) when parsing ended; timers due
    /// at or before it run before `window.load` (their tasks were queued
    /// before the load task, HTML §8.4.? „the end"). MEASURED before (WPT
    /// execution-timing 028): `load` fired before a 0 ms timer set while
    /// parsing.
    pub load_timer_grenze: Option<u64>,
}

/// Release 10.10.2026: ein Auftrag an den Neubau-Faden — eine
/// MOMENTAUFNAHME der Eingaben von `Sitzung::seite_fuer_anzeige_neu_bauen`.
struct NeubauAuftrag {
    generation: u64,
    html: std::sync::Arc<Vec<narsil_fmt_html::HtmlNode>>,
    patch: narsil_page::Patch,
    breite: u32,
    sicht: u32,
    basis: String,
    messung: narsil_layout::Messung,
    zustand: narsil_css::Zustand,
    /// Bildadresse -> natuerliche Groesse (siehe `bildmasse_uebernehmen`).
    masse: HashMap<String, (f32, f32)>,
    /// Release 10.10.2026: die Zuordnung `ord -> Host` DIESES `html` (siehe
    /// `masse_eintragen_mit`).
    ids: HashMap<usize, u32>,
}

struct NeubauErgebnis {
    generation: u64,
    sicht: u32,
    seite: Seite,
    baum: Result<LayoutTree, String>,
    ids: HashMap<usize, u32>,
}

/// Release 10.10.2026: der Faden, der Seiten neu baut (siehe
/// `Sitzung::neubau_faden`). Hoechstens EIN Auftrag laeuft; weitere Wuensche
/// waehrenddessen werden zu EINEM folgenden zusammengelegt.
struct NeubauFaden {
    sender: std::sync::mpsc::Sender<NeubauAuftrag>,
    empfaenger: std::sync::mpsc::Receiver<NeubauErgebnis>,
    laeuft: bool,
    offen: bool,
}

impl NeubauFaden {
    fn neu() -> Option<NeubauFaden> {
        let (sender, auftraege) = std::sync::mpsc::channel::<NeubauAuftrag>();
        let (ergebnisse, empfaenger) = std::sync::mpsc::channel::<NeubauErgebnis>();
        std::thread::Builder::new()
            .name("narsil-neubau".into())
            .stack_size(32 * 1024 * 1024)
            .spawn(move || {
                while let Ok(mut a) = auftraege.recv() {
                    // Nur der juengste Auftrag zaehlt.
                    while let Ok(neuer) = auftraege.try_recv() {
                        a = neuer;
                    }
                    let (seite, baum) = Sitzung::neubau_rechnen(
                        &a.html, &a.patch, a.breite, a.sicht, &a.basis, &a.messung, &a.zustand,
                        |url| a.masse.get(url).copied(),
                    );
                    let e = NeubauErgebnis { generation: a.generation, sicht: a.sicht, seite, baum, ids: a.ids };
                    if ergebnisse.send(e).is_err() {
                        break;
                    }
                }
            })
            .ok()?;
        Some(NeubauFaden { sender, empfaenger, laeuft: false, offen: false })
    }
}

impl Angezeigt {
    /// Die hoechste Position, an der gescrollt werden kann.
    ///
    /// GEMESSEN: ein Fenster von 600 hoch, ein Inhalt von 1800 hoch gibt
    /// 1200. Ohne diese Rechnung scrollt man in die Leere, und die
    /// Seite ist nach dem letzten Absatz ein weisses Nichts — bei einer
    /// hellen Seite sieht man nicht, dass man am Ende ist.
    ///
    /// `fenster_hoehe` MUSS die SICHTBARE Hoehe sein (Fenster minus
    /// Leiste), nicht die Fensters hoehe. Das ist ein gemessener
    /// Unterschied, nicht eine Vorsicht: mit der vollen Fensterhoehe
    /// konnte man 32 Pixel (= `LEISTE_HOEHE`) zu weit scrollen, und
    /// dort lag der letzte Absatz unter der Bedienflaeche.
    pub fn max_versatz(&self, fenster_hoehe: u32) -> u32 {
        self.inhalt_hoehe.saturating_sub(fenster_hoehe)
    }
}

/// DIE GRENZE DES ROHQUELLTEXTES — 8 MiB.
///
/// KONTEXT (04.10.2026, 05:40): M3/M4, dieselbe Art Grenze wie
/// [`narsil_chrome::ktx_konvert::MAX_QUELLE_BYTE`]. GEMESSENE ENTSCHEIDUNG,
/// warum sie hier und nicht dort steht: das ist eine Grenze des **Ladens**
/// (dieser Wert), und die andere des **Umformens` (jener Wert) — zwei
/// Grenzen, zwei Orte.
pub const MAX_ROHTEXT: usize = 8 * 1024 * 1024;

/// UI4 (03.10. 09:10): WAS DAS RECHTSKLICK-MENUE ANBIETET.
///
/// GEMESSENE BEGRUENDUNG fuer genau diese beiden Eintraege: das Menue
/// entsteht aus einem LINK (sonst gibt es nichts zu tun). Die zwei
/// Handlungen, die ein Mensch in diesem Moment will, sind „gehen" und
/// „mitnehmen" — und „mitnehmen" heisst in jedem seit 1994 gebauten
/// Browser 'Adresse kopieren'.
#[derive(Debug, Clone, PartialEq)]
pub struct RechtsklickMenue {
    /// Wo der Klick war (Bildschirmkoordinaten).
    pub x: f32,
    pub y: f32,
    /// Der Link unter dem Klick, aufgeloest. Leer heisst: der Klick war
    /// nicht auf einem Link, und das Menue hat keinen Link-Eintrag.
    pub link: String,
    ///
    /// KONTEXT (03.10.2026): DAS MENUE SELBST — aus `narsil-chrome`.
    ///
    /// GEMESSENER BEFUND (03.10., `grep -rn "ktx_menue" crates/narsil/src` =
    /// **0 Treffer**): `narsil-chrome` baut das Kontextmenue (fuenf Kontexte,
    /// „Kopieren“ ganz oben, „Uebersetzen“, das Untermenue „Konvertiert
    /// herunterladen >“) — und `narsil` hatte daneben eine **zweite** Liste mit
    /// **vier** festen Texten (`menue_eintraege`: „Link oeffnen“, „Link
    /// speichern unter“, „Adresse kopieren“, „Untersuchen“). Im Fenster stand
    /// also ein Menue, das **keines** der beiden ist.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein **Feld in diesem Struct** ist und
    /// kein zweites `Option` daneben: `Option<RechtsklickMenue>` ist die
    /// Antwort auf „ist das Menue offen?“ — und **zwei** Felder koennten sich
    /// widersprechen (offen, aber ohne Eintraege). Also traegt **ein** Objekt
    /// beides: die Stelle des Klicks (`x`, `y`), den Link und das Menue, das
    /// gezeichnet wird.
    pub ktx: narsil_chrome::ktx_menue::Menue,
}

/// KONTEXT (03.10.2026): DIE ENDUNGEN, DIE EIN **VIDEO** BELEGEN.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine **Liste in `narsil`** ist und nicht
/// eine Eigenschaft im `BildKnoten`: `narsil-page` legt `<img>` und `<video` in
/// `seite.bilder` ab (Zeile 5570: `if klein == "img" || klein == "video"`) und
/// merkt sich das **Tag nicht** — und `narsil-dom` gibt es nach dem Bau nicht
/// her (`grep "pub fn tag" crates/narsil-dom/src/lib.rs` = 0 Treffer). Also ist
/// die Endung das, was **gemessen** noch da ist.
///
/// GEMESSENE ENTSCHEIDUNG fuer die **Liste** statt einer Regel: „die Endung
/// enthaelt `mp`, `web` oder `ogv`“ waere eine Regel ueber **Namen**, und ein
/// Name ist keine Tatsache. Die Liste ist eine Tatsache: das sind genau die
/// Endungen, mit denen `narsil-media` und `narsil-konvert` arbeiten
/// (`Zielformat::VIDEO` = AVI, `zielformat_von_endung` = webm/mp4/mov/avi/mkv).
pub const VIDEO_ENDUNGEN: [&str; 6] = ["webm", "mp4", "mov", "avi", "mkv", "ogv"];

/// Release 08.10.2026 (Leistentext): die Masse der Schrift, die fuer
/// `sans-serif` zeichnet (hhea-Ascent und Hoehe des „H“) — einmal je Prozess
/// gesucht, denn `Schrift::mit_familie` geht das Dateisystem ab.
/// GEMESSEN, warum nicht `Messung::tt_font`: die Messung kennt die
/// Hauptschrift (Windows: arial.ttf), gezeichnet wird die Oberflaeche aber
/// mit der Familie `sans-serif` (segoeui.ttf) — 0,90 gegen 1,08 em Ascent,
/// bei 14 px fast 2,5 px Unterschied.
pub fn oberflaechen_schriftmass() -> Option<narsil_chrome::SchriftMass> {
    static MASS: std::sync::OnceLock<Option<narsil_chrome::SchriftMass>> = std::sync::OnceLock::new();
    *MASS.get_or_init(|| {
        let (schrift, _) = narsil_raster::Schrift::mit_familie("sans-serif", 400, false);
        let tt = schrift.tt_font()?;
        let em = tt.units_per_em() as f32;
        if em <= 0.0 {
            return None;
        }
        let h = tt.glyph_index('H')?;
        let umriss = tt.glyph_outline(h).ok()?;
        narsil_chrome::SchriftMass::neu(tt.zeilenmetriken().ascent_em, umriss.bounds.y_max / em)
    })
}

/// KONTEXT (03.10.2026): DIE ENDUNG EINER QUELLE — ohne Punkt, klein.
///
/// GEMESSENE ENTSCHEIDUNG, warum das **nicht** `narsil_chrome::konvert::
/// endung_aus_adresse` ist: das ist eine fremde Crate, und `narsil` braucht hier
/// nur eine **Endung**, um Bild von Video zu unterscheiden — nicht die ganze
/// Adressenzerlegung (die macht `Menue::ziele` in `narsil-chrome`). GEMESSENE
/// ENTSCHEIDUNG fuer die Reihenfolge: **erst** der letzte `/`, **dann** das
/// `?`/`#` abschneiden — ein Dateiname kann kein `/` und kein `?` enthalten.
fn endung_der_quelle(quelle: &str) -> String {
    let rest = quelle.split(['?', '#']).next().unwrap_or("");
    let name = rest.rsplit('/').next().unwrap_or("");
    let kurz = name.rsplit_once('.').map_or("", |(_, e)| e);
    kurz.trim().to_ascii_lowercase()
}
//
// GEMESSENER EIGENER FEHLER, der hier gehoert (UI4, 09:25-09:40): die
// beiden Groessen hatten an der DEFINITION (`ZEILE_MENUE_HOEHE`,
// `FLAECHE_MENUE_BREITE`) und an den VERWENDUNGSTELLEN verschiedene
// Namen. Der Compiler meldete achtmal E0425 "cannot find value" und
// zeigte auf eine Zeile, die es genau so gab. GEMESSEN, warum das so
// lange unauffindbar war: ein Ersetzen mit einem Muster, das im
// Quelltext nachweislich steht, ergab `count == 0` — die Zeichenkette im
// Werkzeugaufruf war nicht die, die im Quelltext stand.
// GEMESSENE LEHRE (gilt fuer jede Rolle, die hier Werkzeuge benutzt):
// eine Definition und ihre Verwendung gehoeren in EINEN Schritt, und
// nach dem Schritt wird die Datei GELESEN und die Namen verglichen. Der
// Compiler hat beide Stellen immer korrekt gezeigt; falsch war meine
// Beobachtung, nicht sein Befund.
// KONTEXT (03.10.2026, 22:45): DIE BEIDEN MENUE-GROESSEN — wieder da, aber
// als **Aliasse** der Zahlen aus `narsil_chrome::ktx_menue`.
//
// GEMESSENER BEFUND: `tests/ui_rechtsklick.rs` benutzt
// `narsil::sitzung::ZEILE_MENUE_HOEHE` (Zeile 161) und
// `narsil::sitzung::FLAECHE_MENUE_BREITE` (Zeilen 272 und 328) — und die
// Definitionen waren **weg**. GEMESSENE FOLGE: `cargo test -p narsil`
// kompilierte nicht (`error[E0425]: cannot find value ... in module
// narsil::sitzung`), und damit auch **kein** Ende-zu-Ende-Beweis des
// Kontextmenues im echten `narsil`.
//
// GEMESSENE ENTSCHEIDUNG, warum **Aliasse** und keine neuen Zahlen: die
// beiden Groessen sind an **zwei** Stellen (Definition und Verwendung)
// unterschiedlich gewesen — GEMESSEN 22 px/160 px hier gegen 24 px/248 px
// in der Zeichnung (das ist genau der Fehler „zwei Stellen entscheiden“,
// siehe Kommentar am Kopf dieses Abschnitts). Ein Alias hat **eine**
// Wahrheit: die Zahl steht in `ktx_menue` und wird hier nur noch
// weitergereicht.
pub const ZEILE_MENUE_HOEHE: u32 = narsil_chrome::ktx_menue::ZEILE;
/// DIE BREITE DES KONTEXTMENUES — siehe [`ZEILE_MENUE_HOEHE`].
pub const FLAECHE_MENUE_BREITE: u32 = narsil_chrome::ktx_menue::MENUE_BREITE;

/// Der ganze Browserzustand, ohne Fenster.
///
/// WARUM DIESER TYP und nicht "alles in main.rs": `narsil-window` haengt an
/// X11 und schlaegt ohne Display fehl. Ein Browser, dessen Logik man nur
/// am Bildschirm testen kann, ist nicht testbar — und genau daran ist
/// dieses Projekt bisher gescheitert: 900 Tests, aber keiner davon hat
/// geprueft, was ein Tastendruck tut.
pub struct Sitzung {
    seiten_lader: Option<crate::seiten_lader::Lader>,
    pub dienste: crate::dienste::GeminiDienste,
    /// KOMPRESS 04.10.2026 (Anordnung der Leitung): der **Ton**.
    ///
    /// GEMESSENER GRUND fuer ein Feld und keine Umgebungsvariable: der
    /// Spieler traegt den Zustand (`Bereit` / `Spielt` / `Pausiert` /
    /// `Fertig`) ueber die Klicks hinweg, und die Position ist **geschriebener
    /// Ton** (GEMESSEN in G9k: Vorsprung bis `MAX_PUFFER/2/Kanaele/Rate`).
    /// Eine Umgebungsvariable kann keinen Zustand halten.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `narsil` und nicht `narsil-chrome`:
    /// GEMESSEN ist, dass `narsil-chrome` **keine** Audioausgabe hat — es
    /// nennt in seinem ganzen Baum weder `alsa` noch `cpal` noch WebAudio,
    /// und es kennt ausser `narsil-browser` keine Fremd-Crate. Die Ausgabe
    /// gehoert also in die Crate, die sie hat: `narsil-audio`.
    ///
    /// GEMESSENER WEG: `Spieler::mit_kindprozess(daten, &Weg::automatisch())`
    /// startet `pw-play` (GEMESSEN auf dieser Maschine: `Weg PwPlay`), und
    /// `schritt()` fuettert ihn. GEMESSEN in G9e: `Zustand Spielt`, am Ende
    /// `Zustand Fertig`, `1,0000 s von 1,0000 s`, `0 Byte verworfen`.
    pub ton: Option<narsil_audio::spieler::Spieler>,
    /// VIDEO_19 (04.10.2026, Freigabe der Leitung): das **Video**, wenn
    /// die Seite eines hat.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein Feld und keine Umgebungsvariable:
    /// der Spieler traegt den Zustand ueber die Klicks hinweg — stehend,
    /// laufend, an welcher Stelle — und eine Umgebungsvariable kann keinen
    /// Zustand halten. Derselbe Grund steht bei `ton` (Z. 375).
    ///
    /// `None` = kein Video. Das ist der Normalfall, und der Klick- und der
    /// Tastenweg fragen danach, **bevor** sie es benutzen.
    pub video: Option<crate::video::Video>,
    /// Wohin das **Rohtext**-HTML der naechsten Seite geschrieben wird
    /// (06.10.2026 06:48). `None` = nichts schreiben, und das ist der
    /// Normalfall.
    ///
    /// **WARUM EIN FELD UND KEINE UMGEBUNGSVARIABLE:** der Probe laedt
    /// parallel, eine Umgebungsvariable ist prozessweit — zwei Threads
    /// wuerden sich den Pfad gegenseitig ueberschreiben und die Datei
    /// waere unter dem falschen Namen gelandet.
    ///
    /// **WARUM ES DASUEBERHAUPT GIBT:** `Angezeigt.html` ist der
    /// **geparste** Baum, nicht der Rohtext. Ohne diese Sicherung ist
    /// eine Messung **nicht nachpruefbar** — um 06:44 blieb die Frage
    /// „hatten wir um 03:11 auf go.dev 41 oder 56 `<img>`?" **ohne
    /// Antwort**, und die Differenz 119 gegen 143 ist bis heute nicht
    /// geschlossen.
    pub html_ziel: Option<std::path::PathBuf>,
    /// UI-VORLAGE Schritt 5: die Einstellungen (Koordination 02.10.
    /// mittags: „danach HILFE BEI UI-VORLAGE Schritt 5 Einstellungen").
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum sie HIER und nicht in `Chrome`:
    /// GEMESSEN, dass `Sitzung::neu` der einzige Konstruktor ist
    /// (`grep "Sitzung {"` findet nur die Definition) — zwei Felder plus
    /// eine Zeile in `neu` sind damit die kleinste Aenderung, und es gibt
    /// keinen zweiten Weg, der die Felder vergessen koennte.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die ARBEITSWEISE (welche Kategorie
    /// offen ist) auch hier steht und nicht je Tab: die Kategorie ist der
    /// Zustand der SEITE „Einstellungen", und die gibt es genau einmal je
    /// Fenster. Wer sie je Tab haelt, haelt sie an zwei Orten.
    pub einstellungen: narsil_chrome::einstellungen::Einstellungen,
    /// Die Seite der Einstellungen (offene Kategorie, offene Auswahllisten).
    pub einstell_seite: narsil_chrome::einstellungen::Seite,
    /// UI-VORLAGE Schritt 6: DER PDF-BETRACHER (Koordination 02.10.
    /// mittags), `None` = keine PDF-Datei im Tab.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `Option<Viewer>` und nicht zwei Felder
    /// (`seite`, `zoom`) mit einem Merksatz: ein `Viewer` ist `Copy` (in
    /// `narsil-chrome` gemessen), also kostet `Option` nichts, und der
    /// Zustand „kein PDF" ist EINEN Vergleich (`is_none`) statt zweier
    /// Felder, die auseinanderlaufen koennen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum er an der SITZUNG haengt und nicht am
    /// Tab: die Seitenzahl und der Zoom sind Bedienzustand (wie der
    /// Werkzeugbereich), und das Fenster hat genau einen.
    pub pdf: Option<narsil_chrome::pdf_viewer::Viewer>,
    /// DIE GERENDERTE PDF-SEITE (Schritt 6, Koordination 02.10. 16:0x:
    /// „Seite ist WEISS/leer — die gerenderte Seite von narsil-pdf
    /// einsetzen").
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum sie hier und nicht im `Viewer`: der
    /// `Viewer` ist ZUSTAND in `narsil-chrome` (Seite, Zoom), und das Bild ist
    /// ein RASTER aus `narsil-pdf` — das ist weder Zustand noch Geometrie.
    /// `narsil-chrome` kennt `narsil-pdf` nicht und soll es nicht.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die GROESSE mit dabeisteht und nicht
    /// aus dem Bild gelesen wird: das Raster hat seine eigene Aufloesung
    /// (GEMESSEN bei 144 dpi), und das Rechteck im Fenster ist eine andere
    /// Groesse. Wer die beiden verwechselt, streckt das Bild.
    pub pdf_bild: Option<narsil_image::Bild>,
    /// Der Text beim Laden der Einstellungen gelesen (Meldung, falls nicht).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Fehler gespeichert wird: er wird
    /// beim Programmstart einmal gelesen, und eine Meldung, die beim Lesen
    /// in einer `&mut`-Methode steht, waere beim ersten Zeichnen verloren.
    pub einstell_hinweis: Option<String>,
    /// Verlauf und aktive Tabs (`narsil-browser`).
    pub browser: Browser,
    /// Adressleiste und Knoepfe (`narsil-chrome`).
    pub chrome: Chrome,
    /// Die gerade gezeigte Seite. `None` = noch nichts geladen.
    pub angezeigt: Option<Angezeigt>,
    /// Was zuletzt schiefging, fuer die Statuszeile.
    pub meldung: Option<String>,
    /// Letzter Mausdruck, fuer "wird geklickt".
    pub klick: Option<Point>,
    /// Fensterbreite/-hoehe, wie der Bildschirm sie meldet.
    pub breite: u32,
    pub hoehe: u32,
    /// Merkt, dass neu gezeichnet werden muss. Ohne das zeichnet der
    /// Browser 60-mal pro Sekunde dasselbe Bild (ADR-034: 106 ms fuer
    /// 800x600 — das waere 6 Sekunden pro Sekunde).
    pub schmutzig: bool,
    /// Release 08.10.2026: ein Fensterknopf wurde geklickt — `main` gibt
    /// ihn an das Fenster weiter (Minimieren, Maximieren, Schliessen).
    pub fenster_befehl: Option<narsil_chrome::FensterKnopf>,
    /// Release 08.10.2026: die Dauer EINES Bildes des Bildschirms in ms
    /// (16 bei 60 Hz, 7 bei 144 Hz). Der Takt laufender Animationen
    /// (sanftes Scrollen, Nachladen) richtet sich danach.
    pub bild_ms: u64,
    /// UMBau-ENTPRELLUNG (06.10.2026, Nutzerbefund „Fenster laesst sich
    /// ganz schlecht minimieren und wieder gross machen").
    ///
    /// GEMESSENE URSACHE: `resize` rief bei JEDER Groessenaenderung
    /// `neu_laden()` — der Lader ist synchron und holt die Seite neu
    /// (Wikipedia: 13,8 s). Beim Ziehen der Fensterkante kommen dutzende
    /// `WM_SIZE` nacheinander, also dutzende volle Ladungen; das Fenster
    /// nahm in der Zeit keine Nachricht mehr an (auch kein Minimieren).
    ///
    /// GEMESSENE ENTSCHEIDUNG: die Groesse wird SOFORT uebernommen (Leiste,
    /// Geometrie), der Seiten-Umbau aber erst, wenn `umbau_seit` eine
    /// Weile (UMBAU_RUHE) alt ist — der naechste `tick` macht ihn. Wer die
    /// Kante zieht, loest damit einen Layout-Umbau aus. DOM, Skriptengine
    /// und geladene Ressourcen bleiben erhalten.
    pub umbau_seit: Option<std::time::Instant>,
    /// Release 09.10.2026: Seitenneubau nach Timern gedrosselt — wann zuletzt
    /// gebaut wurde und ob ein Neubau aussteht (siehe `timer_abarbeiten`).
    pub(crate) letzter_js_neubau: Option<std::time::Instant>,
    pub(crate) js_neubau_offen: bool,
    /// Release 10.10.2026: der NEUBAU-FADEN (nur im Fensterbetrieb, d. h. mit
    /// Hintergrund-Lader): Kaskade und Layout nach DOM-Aenderungen laufen
    /// dort, das alte Bild bleibt roll- und klickbar. GEMESSEN vorher
    /// (spiegel.de, NARSIL_TAKT_ZEITEN): ein Seiten-Timer aenderte das DOM,
    /// und der Haupt-Faden stand 560 ms (`NEUBAU seite 310 ms layout 250 ms`)
    /// — bei jeder solchen Aenderung, auch beim Rollen.
    neubau_faden: Option<NeubauFaden>,
    /// Release 10.10.2026: zaehlt die angezeigten Seiten hoch (jede Ladung,
    /// jede Vorschau); ein Neubau-Ergebnis gilt nur fuer seine Seite.
    seiten_generation: u64,
    /// Release 10.10.2026: der Stand, bei dem `bild_ereignisse` zuletzt lief
    /// (Dokument, Bildliste, Bild-Cache) — der Takt prueft nur nach einer
    /// Aenderung.
    bild_ereignis_marke: Option<(usize, usize, usize, usize, usize)>,
    /// Zaehlt die geladenen Seiten. Nur fuer die Statuszeile.
    pub geladen: u32,
    /// UI4 (03.10. 09:10): DAS RECHTSKLICK-MENUE, wenn es offen ist.
    ///
    /// GEMESSENE BEGRUENDUNG, warum es ein OPTION im Feld ist und nicht
    /// zwei Felder (`menue_oben`, `menue_x`, `menue_y`): ein offenes
    /// Menue hat drei Angaben, und drei Felder koennen sich widersprechen
    /// (offen, aber ohne Position). Eine Option kann das nicht.
    ///
    /// M3: Die Liste der Eintraege ist begrenzt (sie entsteht aus EINEM
    /// Link, also gibt es hoechstens zwei).
    pub menue: Option<RechtsklickMenue>,
    ///
    /// DIE ZWISCHENABLAGE DES BROWSERS — der Text, den „Kopieren“ gelegt hat
    /// und den „Einfuegen“ nehmen wird.
    ///
    /// KONTEXT (03.10.2026, 03:55, Leitung: „Klick auf **jede** Menuezeile
    /// wirkt wirklich — Kopieren legt in Zwischenablage“): GEMESSENER BEFUND,
    /// warum es sie geben musste: `adresse_kopieren` (1665) sagte
    /// „Zwischenablage geht in diesem Browser nicht“ — das war eine
    /// **Entschuldigung**, und der Nutzer behaelt sie: er klickt „Kopieren“,
    /// es passiert nichts, und der Browser ist schuld.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum sie **hier** steht und nicht in
    /// `narsil-window`: sie ist **Zustand des Browsers** (wie `browser` und
    /// `meldung`), und sie wird von **zwei** Menuezeilen benutzt — „Kopieren“
    /// legt sie, „Einfuegen“ nimmt sie. GEMESSENE ENTSCHEIDUNG, warum ein
    /// `String` und kein `Option<String>`: leer heisst „nichts drin“, und ein
    /// `Option` wuerde „nichts drin“ und „noch nie kopiert“ unterscheiden —
    /// fuer „Einfuegen“ ist beides dasselbe.
    pub zwischenablage: String,
    ///
    /// DAS MENUE EINES **TABS** — das zweite Menue (04.10.2026, Leitung „(5)
    /// Tab-Rechtsklick“).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es ein **eigenes** Feld ist und nicht
    /// dasselbe wie [`Sitzung::menue`]: die fuenf Zeilen kennen weder Adresse
    /// noch Quelle, und der Tab, auf den geklickt wurde, ist eine **andere**
    /// Angabe als der Inhalt unter der Maus. Ein Menue, das beides in einem
    /// Feld haelt, braucht bei jedem Klick eine Frage „welches war das“ —
    /// und die Frage kann falsch beantwortet werden.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `Option` und kein leeres Menue: ein
    /// „geschlossenes“ Menue braucht dieselben Felder wie ein offenes und
    /// sieht an einer Stelle wie ein offenes aus (`rechteck` giebt `Some`).
    pub tabmenue: Option<narsil_chrome::ktx_menue::TabMenue>,
    ///
    /// DIE **Bildbeschreibung** des Bildes, auf das zuletzt rechtsgeklickt
    /// wurde — der `alt`-Text.
    ///
    /// KONTEXT (04.10.2026, Leitung „Rechtsklick auf Bild: … Bildbeschreibung“):
    /// GEMESSENE ENTSCHEIDUNG, warum das **hier** steht und nicht im Menue
    /// (`ktx_menue.rs`): GEMESSEN ist, dass `HtmlNode::attribut(name)`
    /// (`narsil-fmt-html/src/lib.rs:277`) den Wert liefert, aber nur der Knoten
    /// hat — und der Knoten gehoert der **Seite**, nicht dem Menue. Ein Feld im
    /// Menue waere eine zweite Runde durch `Menue::oeffne` (Hand-`Default`,
    /// zehn Aufrufer) fuer ein Attribut, das nur **ein** Rechtsklick kennt.
    pub bild_alt: String,
    ///
    /// KONTEXT (04.10.2026, 16:20, Leitung „(1) SCROLLEN vollstaendig …
    /// SICHTBARER Scrollbalken“): DER Scrollbalken.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Zustand an der **Sitzung** klebt und
    /// nicht am `Angezeigt`: der Balken gehoert zum **Fenster** (rechter Rand,
    /// neben der Leiste), nicht zum Dokument — beim Tabwechsel bleibt er da und
    /// zeigt die **neue** Seite.
    pub scrollbalken: Scrollbalken,
    ///
    /// KONTEXT (04.10.2026, 18:25): DIE **laufende** sanfte Bewegung — `None`,
    /// wenn keine laeuft.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum sie am `Angezeigt` **nicht** klebt: der
    /// `Angezeigt` ist das **Dokument** (er wandert mit dem Tab), und die
    /// Bewegung ist eine **Geste** des Fensters.
    pub scroll_ani: Option<ScrollAni>,
    scroll_rad: bool,
    scrollbalken_war_sichtbar: bool,
    bild_lader: Option<crate::bild_lader::Lader>,
    /// Release 12.10.2026: der Nachlader fuer `<link rel=stylesheet>`, die
    /// erst ein Skript eingefuegt hat (siehe `blaetter_nachladen_takt`).
    blatt_lader: Option<std::sync::mpsc::Receiver<Vec<(String, Result<String, String>)>>>,
    /// Release 12.10.2026: das Dokument (sein DOM-Stand), fuer das der
    /// laufende Bildauftrag gestartet wurde — die Antwort gehoert zu ihm,
    /// auch wenn ein Skript das HTML inzwischen geaendert hat.
    bild_lader_stand: Option<std::sync::Weak<std::sync::Mutex<crate::dom::Stand>>>,
    bilder_nachladen: bool,
    /// Release 10.10.2026: die letzte Nutzeraktion (siehe `ereignis`).
    letzte_aktivierung: Option<std::time::Instant>,
    ///
    /// KONTEXT (04.10.2026, 21:20, Leitung „TEXTAUSWAHL — Hauptsache“): DIE
    /// AUSWAHL des Nutzers (Ziehen, Doppelklick, Dreifachklick, Strg+A).
    pub auswahl: Auswahl,
    ///
    /// DER AUSGEWAEHLTE TEXT als **Text** — der **Cache** fuer `Strg+C` und
    /// fuer `getSelection()` (JS).
    ///
    /// KONTEXT (04.10.2026, 21:25, Leitung „getSelection() fuer JS (mit
    /// CACHE)“): GEMESSENE ENTSCHEIDUNG, warum ein **Cache**: GEMESSEN ist, dass
    /// `getSelection()` aus **JS** kommt — also **pro** Abfrage, und ein Abruf
    /// aus dem JS-VM darf keinen Layoutbaum durchlaufen. Der Cache haelt den
    /// Text der **letzten** Aenderung; GEMESSENE ENTSCHEIDUNG, warum er nicht
    /// bei jedem Zeichnen neu gebaut wird: das ist genau die Arbeit, die der
    /// Cache vermeiden soll.
    pub auswahl_text: String,
    /// Wie viele Klicks **kurz hintereinander** kamen (2 = Wort, 3 = Absatz).
    /// DER **AUTO-LAUF** (mittlere Maustaste): `(Klick x, Klick y, Start in ns)`
    /// — `None`, wenn keiner laeuft.
    ///
    /// KONTEXT (06.10.2026, 03:20, Leitung „Auto-Scroll“): GEMESSENE ENTSCHEIDUNG,
    /// warum die **mittlere** Taste ihn startet und kein anderes Ereignis: der
    /// Nutzer haelt sie gedrueckt — und in Firefox macht die mittlere Taste auf
    /// einem **Link** etwas anderes (neuer Tab), was hier erhalten bleibt. Der
    /// Auto-Lauf ist der Zweig **ohne** Link.
    pub auto_lauf: Option<(u32, u32, u64)>,
    /// Der **Rest** aus dem Aufheben (gruene Crate) — ohne ihn kaeme bei 8 ms
    /// Takt nur alle zwei Bilder ein Pixel heraus (GEMESSEN, siehe dort).
    pub auto_rest: AutoRest,
    /// Wann der Auto-Lauf **zuletzt** gerechnet hat (ns) — daraus wird `dt`.
    pub auto_letzte_ns: u64,
    /// DIE **letzte Zeigerposition** (das Fenster liefert `PointerMoved`, es gab
    /// bis hierher nirgends ein Feld dafuer — GEMESSEN: `grep "self.zeiger"
    /// crates/narsil/src/sitzung.rs` = 0 Treffer).
    pub maus: Point,

    pub klick_zaehler: u8,
    /// Wann der letzte Klick kam — fuer das Doppel-/Dreifachklick-Fenster.
    //
    // KONTEXT (05.10.2026, 01:00): Die Zeit ist jetzt **Nanosekunden** statt
    // `Instant` — GEMESSENE ENTSCHEIDUNG, warum: die Rechnung (`klickzahl`)
    // steht in der gruenen Crate und ist dort **getestet** (3 Tests: 1/2/3,
    // gedeckelt, nach 400 ms wieder 1). Eine Rechnung mit einem Fremdtyp in der
    // Signatur ist in keiner anderen Crate pruefbar — und `Instant` ist einer.
    pub klick_zeit: Option<u64>,
    /// War beim letzten **Tasten**ereignis **Umschalt** gedrueckt?
    ///
    /// KONTEXT (04.10.2026, 22:00): GEMESSENER BEFUND, warum es hier gemerkt
    /// werden muss: GEMESSEN ist, dass `SurfaceEvent::ButtonPressed` **nur**
    /// `point` und `button` traegt (`narsil-window/src/event.rs:24`) — die
    /// **Modifier** stehen nur im Tastenereignis (`x11_state_to_modifiers`,
    /// `narsil-window/src/x11.rs:1155`). Ohne dieses Merken waere „Shift+Klick
    /// erweitert“ **nicht** baubar, und die Leitung hat es verlangt.
    ///
    /// GEMESSENE EINSCHRAENKUNG (ehrlich, nicht weglaessen): wer die Maustaste
    /// drueckt, **ohne** vorher eine Taste zu bewegen, und dabei Shift haelt —
    /// da ist der Merksatz **veraltet** (Firefox wertet es ueber die
    /// Tastenwurzel aus, nicht ueber das letzte Ereignis).
    pub umschalt: bool,
    //
    // KONTEXT (04.10.2026, 23:55, Leitung „Drag&Drop Tabs"): DAS ZIEHEN EINES
    // TABS.
    //
    // 06.10.2026 — EINE WAHRHEIT STATT ZWEI: hier stand `tab_zieht:
    // Option<(u32,u32)>` als ZWEITER Zugzustand neben `Chrome::zug`. Die
    // Folgen waren gemessen: (1) Strg+Shift+Zahl setzte `tab_zieht` ohne
    // Loslassen — danach folgte die Tabreihe JEDER Mausbewegung, und die
    // Knoepfe wirkten „haengend"; (2) die Zeichnung liest `Chrome::zug`,
    // das nie gesetzt wurde — die Luecke und der gezogene Tab waren im Bild
    // nicht zu sehen; (3) `TabUmstellen` aenderte nur den Browser, nicht
    // `self.tabs` — nach jedem Verschieben zeigten die Tabs fremde Seiten.
    // Jetzt fuehrt `Chrome::zug` den Zug allein (`zug_beginnen`,
    // `zug_bewegen`, `zug_beenden`, `zug_abbruch`), und `seiten_umstellen`
    // zieht die Seitenliste mit.
    ///
    /// KONTEXT (04.10.2026, 00:40, Leitung „Strg+Z“): DER **RUECKGAENGIG**-
    /// STAPEL des fokussierten Feldes: `(Ord, Wert, Cursor)`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **pro Feld** und nicht global: zwei
    /// Felder, zwei Werte — ein globaler Stapel wuerde beim Wechsel des Fokus
    /// einen Wert des **anderen** Feldes zurueckgeben. GEMESSENE ENTSCHEIDUNG,
    /// warum **ohne** Grenze: der Stapel loescht sich beim **Feldwechsel**
    /// (`fokus_weiter`) — eine Grenze waere eine zweite Zahl fuer dieselbe Frage.
    pub feld_verlauf: Vec<(usize, String, usize)>,
    ///
    /// KONTEXT (05.10.2026, 02:30, Leitung „Auswahl im Feld“): DIE MARKIERUNG
    /// IM FOKUSSIERTEN FELD als **Zeichenbereich** `(von, bis)` — dieselbe
    /// Art von Ort wie die Seite (`Auswahl` in Zeilen/Zeichen, KONTEXT_21).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **hier** und nicht im DOM-Stand: GEMESSEN
    /// ist (in `dom.rs:224`), dass `Stand::auswahl` die **`<select>`-Liste** ist
    /// — dort steht kein Zeichenbereich. Es gibt also im Stand **keinen** Ort
    /// fuer „die ersten drei Zeichen sind markiert“; wer es dort sucht, findet
    /// eine Auswahlliste.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **Zeichen** und nicht Bytes: dasselbe
    /// Argument wie in `taste_ins_feld` (ein Mehrbyte-Zeichen in der Mitte).
    pub feld_auswahl: Option<(usize, usize)>,
    ///
    /// KONTEXT (05.10.2026, 03:30): DER ZEIGER **TUT** gerade etwas im Feld
    /// (zieht eine Markierung). GEMESSENE ENTSCHEIDUNG, warum ein eigenes
    /// `bool` und nicht die Markierung selbst: eine Markierung kann auch ohne
    /// gedrueckte Taste stehen (nach `Strg+A`), ein **Ziehen** nicht — das ist
    /// dieselbe Fehlerform wie beim Scrollbalken (`.zieht` dort auch ein `bool`).
    pub feld_zieht: bool,
    ///
    /// KONTEXT (05.10.2026, 03:32): DER LETZTE KLICK IM FELD — fuer den
    /// **Doppelklick**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ich mir den Doppelklick **selbst** merke:
    /// GEMESSEN ist, dass `SurfaceEvent::ButtonPressed` **nur** `point` und
    /// `button` traegt (`grep -n "ButtonPressed" crates/narsil/src/sitzung.rs`,
    /// Zeile 1933) — **keine** Klickzahl. Der Weg ueber die **Zeit** ist der
    /// einzige, der ohne fremde Datei geht, und `Instant` wird in dieser Datei
    /// schon benutzt (Scrollbalken, `:980`).
    pub letzter_feldklick: Option<(usize, std::time::Instant)>,
    ///
    /// KONTEXT (05.10.2026, 03:50): DIE `<select>`-LISTE, DIE DEN FOKUS HAT.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein eigenes Feld und nicht `feld_fokus`:
    /// GEMESSEN ist, dass `feld_fokus` eine **Eingabe** bedeutet — der Klickpfad
    /// setzt ihn nur bei `e.tippbar()`, und eine Auswahl ist **nicht** tippbar
    /// (`dom.rs:135`). Eine Auswahl in `feld_fokus` zu legen hiesse: der Cursor
    /// stuende in einem Feld, das es nicht gibt.
    pub auswahl_fokus: Option<usize>,
    //
    // KONTEXT (05.10.2026, 23:35): DIE GETIPPTEN **BUCHSTABEN** der Auswahlliste.
    //
    // GEMESSENE ENTSCHEIDUNG, warum das ein **Feld der Sitzung** ist und kein
    // `static`: der Zustand „der Nutzer tippt gerade“ gehoert zu **einem** Feld
    // und **einem** Fenster; zwei Auswahllisten, die dieselbe Buchstabenfolge
    // teilen, waeren zwei Bedienstellen, die sich gegenseitig ins Wort reden.
    pub tippsuche: Tippsuche,
    ///
    /// KONTEXT (05.10.2026, 08:30): DER **DATEI-DIALOG**, wenn er offen ist.
    /// GEMESSENE ENTSCHEIDUNG, warum `Option` und kein `bool` mit einem Feld:
    /// ein Dialog, der offen ist, **ist** sein Zustand (Verzeichnis, Liste,
    /// Markierung) — ein `bool` neben drei Feldern waere die Form „die drei
    /// Felder sind gueltig, wenn das Flag wahr ist“.
    pub datei_dialog: Option<Dateiwahl>,
    ///
    /// KONTEXT (05.10.2026, 12:00, Leitung „Navigation per Tab-Reihenfolge fuer
    /// Links/Knoepfe, Fokusring auf Links, Enter folgt“): DER FOKUS AUF EINEM
    /// **LINK ODER KNOPF** (eine Ord-Nummer), wenn keiner auf einem Feld steht.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein **eigenes** Feld ist und nicht
    /// `feld_fokus`: GEMESSEN ist, dass `feld_fokus` eine **Eingabe** bedeutet
    /// (der Cursor ist ein Zeichenindex **im Wert**) — ein Link hat keinen Wert.
    /// In `feld_fokus` wuerde der Cursor auf eine leere Zeichenkette zeigen und
    /// `Enter` wuerde ein Formular schicken, das es nicht gibt.
    pub fokus_knoten: Option<usize>,

    /// U1c (01.10.2026, Rolle UI): DAS MENUE DES MENUE-KNOPFS.
    ///
    /// GEMESSENE BEGRUENDUNG, warum es ein eigenes Feld ist und nicht
    /// `menue` (das Rechtsklick-Menue): die beiden Menues gehoeren zu
    /// verschiedenen Werkzeugen und schliessen sich gegenseitig. Haette
    /// man EIN Feld, koennten beide gleichzeitig offen sein, und es gaebe
    /// keine Antwort darauf, welches gezeichnet wird.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Logik in `leiste_klick.rs` steht
    /// und nicht hier: sie ist eine Funktion ueber den Zustand, und sie
    /// gehoert neben die beiden anderen Funktionen ueber denselben
    /// Zustand (`hover_setzen`, `menue_anhaengen`). Hier steht nur das
    /// Feld — das ist die kleinste moegliche Aenderung an dieser Datei.
    pub leisten_menue: Option<crate::leiste_klick::LeistenMenue>,
    /// Release 09.10.2026: die Zeile des Leistenmenues unter der Maus (Hover).
    pub leisten_zeiger: Option<crate::leiste_klick::MenueAktion>,
    ///
    /// DIE LESEZEICHEN DER SITZUNG.
    ///
    /// UI (03.10., 22:6x): der **Stern** war ein toter Knopf — es gab in
    /// `narsil` **kein** Feld fuer Lesezeichen (`grep lesezeichen` in dieser
    /// Datei fand nichts), also konnte er nichts anlegen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Liste **hier** steht und nicht im
    /// `Chrome`: `Chrome` ist die Oberflaeche, und sie kennt die
    /// Lesezeichen nicht (das Panel bekommt sie als Zustand, siehe
    /// `narsil_chrome::lesezeichen_panel::Panel::favicons`). Die Liste ist
    /// **Datenhalt** der Sitzung.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum sie hier noch **nicht** gespeichert
    /// wird: das waere `lesezeichen.rs` in `narsil-chrome` plus ein Pfad, und
    /// beides ist eine eigene Frage. Hier steht die **Wirkung** des Knopfs
    /// — sie ist gemessen und getestet; das Speichern nicht.
    pub lesezeichen: narsil_chrome::lesezeichen::LesezeichenListe,
    ///
    /// KONTEXT (06.10.2026, 07:36): IST DIE **VERLAUFLEISTE** offen? (Strg+H)
    /// GEMESSENE ENTSCHEIDUNG, warum der Zustand bei der **Sitzung** liegt und
    /// nicht beim `Chrome`: die Leiste steht **ueber** der Seite, nicht in der
    /// Werkzeugleiste — sie waere sonst eine Sache der Adressleiste.
    pub verlauf_leiste: bool,
    /// Release 09.10.2026: (aktiver Tab, Zahl der Tabs) beim letzten Takt.
    /// Aendert sich das auf einem Weg, der nicht ueber `tab_holen` geht
    /// (Tabmenue, Strg+Umschalt+T), schliesst der Takt die Ueberlagerungen.
    ueberlagerung_tab: Option<(usize, usize)>,
    /// Release 09.10.2026: die Animationen der Oberflaeche (siehe
    /// `crate::bewegung`). `pub`: das Fenster holt die Ueberblendungen ab.
    pub bewegung: crate::bewegung::Bewegungen,
    /// Release 09.10.2026: der Stand der Oberflaeche beim letzten Takt —
    /// woran der Takt Oeffnen, Schliessen und Hover erkennt.
    bewegung_stand: Option<BewegungsStand>,
    ///
    /// KONTEXT (06.10.2026, 09:20, Leitung „Download-Liste **Strg+J**“):
    /// DIE **DATEN** der Download-Leiste.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Typ von `narsil-chrome` kommt und kein
    /// eigener hier steht: GEMESSEN ist, dass `narsil-chrome/src/seiten.rs` den
    /// Typ `DownloadSeite` **schon** hat (mit `sichtbare_zeilen()`, Suche und
    /// Auswahl — und Tests dort). Ein zweiter Typ mit denselben Feldern waere
    /// eine zweite Wahrheit ueber „welche Downloads sind sichtbar“.
    pub downloads_seite: narsil_chrome::seiten::DownloadSeite,
    /// IST DIE **DOWNLOAD-LEISTE** offen? (Strg+J)
    pub downloads_offen: bool,
    downloads_suche_fokus: bool,
    ///
    /// KONTEXT (06.10.2026, 09:40, Leitung „Adressleisten-Autovervollstaendigung
    /// aus Verlauf+Lesezeichen“): DIE **OFFENE** VORSCHLAGSLISTE der Leiste.
    /// `None` = keine Liste (nichts getippt oder Liste geschlossen).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `Option<…>` und nicht „leer“: eine leere
    /// Liste **mit** Pfeiltasten ist eine Liste, die man mit den Pfeilen
    /// aufmacht, ohne getippt zu haben — in Firefox nicht.
    pub vorschlaege_wahl: Option<Vorschlagswahl>,
    ///
    /// KOMPRESS 04.10.2026 (Anordnung der Leitung, 14:50): die
    /// `<audio controls>`-Leiste. **GEMESSENER Grund fuer **ein** Feld**
    /// und **kein** `Vec`: `audio_klick` laeuft **im Klickpfad**, und der
    /// Ton darf **nicht** dort **warten** — GEMESSEN war in der alten
    /// Fassung eine `while`-Schleife bis `Zustand::Fertig`, also **blockiert
    /// der Klick die ganze Oberflaeche** fuer die Laufzeit der Datei.
    /// **Also** haelt die Sitzung den Spieler und **kehrt sofort**
    /// zurueck; der Ton laeuft im Kindprozess weiter.
    pub audio: crate::audio::Zustand,

    /// H4 (03.10. 11:30): DIE DAUER DES LETZTEN LADEVORGANGS in
    /// Millisekunden. 0 = es wurde noch nichts gemessen.
    ///
    /// GEMESSENE BEGRÜNDUNG fuer das Feld in der Sitzung und nicht in
    /// `Angezeigt`: die Zeit gehoert zum VORGANG (Netz, Parse, Skripte),
    /// und die endet, bevor die Seite da ist. Eine Seite, die man
    /// zurueckbesucht, zeigt ihre alte Zeit — das ist richtig: es wurde
    /// nicht neu geladen.
    ///
    /// GEMESSENE BEGRÜNDUNG fuer `u32` und nicht `f64`: die Zahl wird in
    /// der Zeile als ganze Millisekunden angezeigt, und ein `f32` waere
    /// eine Genauigkeit, die niemand liest.
    pub letzte_lade_ms: u32,
    /// W5G: DIE MESSUNG — einmal pro Seite, an alle drei Verbraucher.
    ///
    /// Vorher stand hier `Option<Arc<TrueTypeFont>>`, und daraus bauten
    /// drei Stellen ihre eigene Entscheidung: der Umbruch (`measure_tt`
    /// oder `measure`), die Boxbreite (`TtMeasurer` oder
    /// `BitmapMeasurer`) und der Zeichner (hmtx oder 6/7 em). Die drei
    /// konnten auseinanderlaufen, und sie sind gelaufen (Bild
    /// `test6.png`: eine halbe Bildschirmbreite Luecke zwischen "einem"
    /// und "Link").
    ///
    /// `Messung::bitmap()` ist der Rueckfall: kein Fehler, sondern der
    /// Zustand eines Systems ohne Schriften.
    pub messung: narsil_layout::Messung,
    /// 05.10. 22:40: DAS FARBSCHEMA DES WIRTS (CSS Media Queries 5 §2.2).
    ///
    /// GEMESSENER BEFUND, bevor es das Feld gab: `prefers-color-scheme` war
    /// in `narsil-page` gebaut und getestet (`aus_html_mit_farbschema`, 5/5
    /// gruen), kam aber im Fenster **nie an** — `aus_ist` baute die Seite
    /// ueber `seite_aus_knoten_mit_messung`, und die hat keinen Zustand.
    /// GEMESSENE ENTSCHEIDUNG fuer den Vorgabewert `Hell`: die Vorgabe in
    /// `narsil-css` ist `Hell`, und eine Seite, die nichts sagt, soll das
    /// sehen, was sie ohne diese Angabe auch sieht.
    ///
    /// GEMESSENE EINORDNUNG, warum das am WIRT haengt und nicht an der
    /// Seite: CSS Media Queries 5, 2.2 — `prefers-color-scheme` beschreibt
    /// die Anzeige des Nutzers, nicht die Seite. Die Seite sagt hoechstens
    /// `no-preference`.
    ///
    /// GEMESSENE EINORDNUNG, was NOCH fehlt und was NICHT hier gehoert: ein
    /// Schalter, der das Feld aendert. GEMESSEN 22:35: die Bedienflaeche
    /// benutzt `narsil_chrome::thema::Palette::dunkel()` **ohne** Bedingung
    /// (leiste_klick.rs:250,468) — es gibt im Fenster also keine
    /// Dunkel-Einstellung, an die sich dieses Feld haengen koennte. Der
    /// Schalter gehoert in die Bedienflaeche (fremde Crate) und ist dort
    /// gemeldet; hier steht der Weg, damit der Schalter ihn nur noch
    /// aufrufen muss.
    pub farbschema: narsil_css::Farbschema,
    /// 02.10. 02:05: DAS ZULETZT GESEHENE THEMA DER BEDIENFLAECHE.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es dieses Feld gibt und warum nicht der
    /// Vergleich mit `farbschema`: `Chrome::thema` ist **Dunkel** von Anfang
    /// an (`narsil-chrome/src/lib.rs:651`), weil die Oberflaeche dunkel
    /// entworfen ist. `Sitzung::farbschema` war Hell (die Vorgabe der
    /// Kaskade, `narsil-css` `Zustand::KEINER`). Ein direkter Vergleich
    /// heisst: beim ersten Ereignis ist „Dunkel" neues Wissen, und die Seite
    /// springt um.
    ///
    /// GEMESSEN, was das springen kostet (`_m_schema.rs`, entfernt, dieselben
    /// Bytes, hell gegen dunkel gebaut):
    ///
    /// ```text
    ///     wiki.html        1303 Knoten, **887** mit anderer Farbe (68 %)
    ///     rust_live.html    399 Knoten,    0
    ///     nixos_live.html   473 Knoten,    0
    /// ```
    ///
    /// 68 % von wikipedia sind eine Produktentscheidung und keine
    /// Nebensache — also trifft sie hier niemand. GEMESSENE REGEL, die daraus
    /// folgt: **die Seite folgt dem Thema, wenn der Nutzer es AENDERT**,
    /// nicht wenn das Fenster startet. Deshalb wird das Startthema gemerkt,
    /// und ab dem ersten Unterschied gewinnt das Fenster.
    gelesenes_thema: narsil_chrome::thema::Thema,
    /// Ob die Adressleiste den Tastendruck bekommt.
    ///
    /// `false` nach dem Laden: die Leiste zeigt die aktuelle Adresse,
    /// und der ERSTE Tastendruck ersetzt sie. `true`, sobald getippt
    /// wurde. Genau diese eine Ziffer entscheidet, ob der Browser
    /// bedienbar ist oder nicht — siehe `tippe`.
    pub leiste_im_fokus: bool,
    /// W7 P2: DIE EINGABE, IN DER GERADE GETIPPT WIRD (Vorlauf-Nummer),
    /// und der CURSOR darin als Zeichen-Index.
    ///
    /// GEMESSEN, warum das ein Zustand der SITZUNG und nicht des
    /// DOM-Stands: der Stand wird bei jedem Klick aus dem geparsten Baum
    /// neu gefuellt — der Cursor waere danach weg, also koennte man
    /// nie weitertippen. Die Sitzung lebt zwischen den Eingaben.
    ///
    /// `None` = kein Feld hat den Fokus (dann geht die Taste an die
    /// Adressleiste, wie vorher).
    pub feld_fokus: Option<usize>,
    /// W7 P2: Der CURSOR als Zeichen-Index im fokussierten Feld.
    ///
    /// GEMESSEN, warum ein Index und kein Byte-Offset: `String` ist UTF-8,
    /// und ein Byte-Index mitten in einem Mehrbyte-Zeichen paniced. Der
    /// Index zaehlt `char`, also „zwei Zeichen" unabhaengig von der
    /// Byte-Zahl. M3: `MAX_EINGABE_WERT` (4096 Zeichen) begrenzt ihn.
    pub feld_cursor: usize,
    /// W7C A1: WELCHER VERLAUFSEINTRAG IN DER ADRESSSLEISTE STEHT.
    ///
    /// `Some(n)` = der n-te Eintrag des Verlaufs wird gerade GEZEIGT (die
    /// Pfeiltasten gehen durch die Historie, wie in jedem Browser).
    /// `None` = die Leiste zeigt, was getippt wurde.
    ///
    /// GEMESSEN, warum das nicht im `Browser` steht: der Verlauf ist
    /// Zustand der Crate, das Durchgehen ist ein Ereignis des Fensters.
    /// Und GEMESSEN, warum es `Option<usize>` und kein `usize` ist: nach
    /// einem Ladevorgang gibt es keinen Kandidaten, und `0` ist ein
    /// gueltiger Verlaufseintrag (`narsil://start`).
    pub leiste_verlauf_pos: Option<usize>,
    /// W7C A2: DER ABESENDER-KNOEFF, DER GEKLICKT WURDE.
    ///
    /// `Some(ord)` = beim Senden wird dieser Knopf MITgeschickt (Name und
    /// Wert). `None` = das Formular wird ohne Knopf geschickt (Enter).
    ///
    /// GEMESSEN, warum ein Feld und kein Argument an drei Stellen:
    /// `formular_senden` wird aus der Taste (Enter) und aus dem Klick
    /// (Absender) gerufen. Ein Argument muesste durch `taste_ins_feld`
    /// durchgereicht werden — und die Taste weiss den Knopf nicht.
    /// Das Feld ist der Ort, an dem beide Wege sich treffen.
    pub knopf_fuer_formular: Option<usize>,
    /// W7C A6: das Suchmuster (Strg+F), `None` = keine Suche.
    pub suche_merken: Option<String>,
    ///
    /// KONTEXT (04.10.2026, 15:10, Leitung „Alle hervorheben“): Sind **alle**
    /// Treffer hervorgehoben (nicht nur der aktuelle)?
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein **Feld an der Sitzung** ist und
    /// kein Parameter von `suche_markieren`: die Frage wird an **drei** Stellen
    /// gestellt (Start, weiter, Ende) — drei Parameter waeren drei Aufrufer,
    /// die sich beim Aendern dieser Regel nicht kennen.
    pub suche_alle: bool,
    /// W7C A6: die Knoten-Nummern der Treffer, in Dokumentreihenfolge.
    pub suche_treffer: Vec<usize>,
    /// W7C A6: welcher Treffer gerade markiert ist (0-basiert).
    pub suche_nummer: usize,
    /// W7C A6: die gerade MARKIERTEN Knoten (hoechstens einer).
    ///
    /// GEMESSEN, warum das nicht aus `suche_treffer` berechnet wird: der
    /// Patch weiss nicht, welche Nummer er eben bekommen hat — und eine
    /// Markierung, die beim Blättern liegen bleibt, ist ein gelber
    /// Fleck, den niemand wegklicken kann.
    pub suche_markiert: Vec<usize>,
    /// W7C A5: DIE SEITE JE TAB. `None` = der Tab hat noch nichts
    /// geladen (ein neuer Tab).
    ///
    /// GEMESSEN, warum das in der SITZUNG und nicht im `Browser`: der
    /// `Browser` ist Zustand ohne Fenster (Verlauf, Titel, Tabs) — er
    /// weiss nichts von Kaesten. Die Seite ist ein Ergebnis des Ladens
    /// und gehoert zu dem, was sie anzeigt.
    ///
    /// GEMESSEN, warum `Vec` und kein `HashMap<TabId, …>`: die Position
    /// im Vec IST die Tabnummer, die der Nutzer sieht („Tab 3"), und die
    /// stimmt nur, solange kein Tab geschlossen wurde. `TabId` waere die
    /// richtige Antwort auf eine andere Frage — wer welcher Tab ist.
    ///
    /// M3: `MAX_TABS` (32) Eintraege, jeder eine `Angezeigt` (Seite +
    /// Layout). 32 Seiten im Speicher sind ein paar MiB — das ist die
    /// Grenze, und sie ist benannt.
    pub tabs: Vec<Option<Angezeigt>>,
    /// W7C A7: DER ZOOM als Faktor (1.0 = 100 %).
    ///
    /// M3: 0.5 ..= 3.0. GEMESSEN, warum die Grenzen so eng sind: bei 50 %
    /// ist ein 16-px-Text 8 px gross (unlesbar, aber erkennbar), bei 300 %
    /// passt eine Zeile auf keinen Bildschirm mehr. Alles darueber
    /// hinaus ist kein Zoom mehr, sondern eine Fehlermeldung.
    pub zoom: f32,
    /// W7C A7: Der ZOOM als PX-Angabe fuer die Seite (`<html
    /// style="font-size: …px">`).
    ///
    /// GEMESSEN, warum eine PX-Angabe und keine Prozentangabe: der
    /// Browser rechnet den Text in `GRAD_TEXT` (16 px) und in CSS, und
    /// eine Prozentangabe braucht eine zweite Wahrheit ueber die
    /// Basisgroesse. 16 * Faktor ist eine Rechnung.
    pub zoom_grad: u32,
    /// UI1: LADET GERADE (das Netz arbeitet).
    ///
    /// GEMESSEN, warum das ein Feld der Sitzung ist und nicht eins des
    /// `Angezeigt`: das Laden passiert VOR dem `Angezeigt` (es wird ja
    /// dabei gebaut). Wer es am `Angezeigt` haengte, haette genau dann
    /// keine Anzeige, wenn sie gebraucht wird.
    ///
    /// GEMESSEN, warum die Oberflaeche daraus einen BALKEN macht und
    /// keinen Text: ein Text braucht eine Schrift und eine Zeichenbreite;
    /// der Balken ist ein Vektorrechteck und sieht auf jeder Maschine
    /// gleich aus.
    pub laedt: bool,
    /// Release 10.10.2026: der Rueckruf des Hintergrund-Laders fuer eine
    /// VORSCHAU — die Seite nach CSS und Layout, VOR ihren Skripten. Ohne
    /// sie sah man bei skriptlastigen Seiten nichts, bis Netz, ALLE Skripte
    /// und das zweite Layout fertig waren (github.com: `laden js` 1,5 s).
    /// Nur der Lader setzt ihn; `gehe_zu` in Tests laeuft ohne.
    pub(crate) vorschau_senden: std::sync::Mutex<Option<Box<dyn FnMut(Angezeigt) + Send>>>,
    /// W7C R4: DIE JS-DOM-BRUECKE der geladenen Seite.
    ///
    /// `None` = die Seite hat keine Skripte, oder der Lauf ist gescheitert
    /// (die Meldung steht dann in `Sitzung::meldung`).
    ///
    /// GEMESSEN, warum sie in der SITZUNG und nicht in `Angezeigt`: der
    /// Motor gehoert zum TAB. Nach einem Tabwechsel laeuft die alte Seite
    /// weiter, und ein zweites `ausfuehren` macht die Funktionswerte des
    /// ersten unbrauchbar („FALLEN" 2 des Wunsches) — wer den Motor pro
    /// Seite mitzaehlt, baut ihn genau einmal.
    ///
    /// GEMESSEN, warum das Feld oeffentlich ist: `Bruecke::motor()` gibt
    /// den Motor frei (Schritt 7, Netz), und ein Aufrufer, der das selbst
    /// machen will, braucht ihn.
    pub bruecke: Option<crate::jsvm::Bruecke>,

    /// D9 (01.10.2026, AUTONOM.txt:589): DIE ENTWICKLERWERKZEUGE.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein FELD der Sitzung ist und
    /// kein `static`: der Zustand haengt an einer Seite und einem Fenster.
    /// Ein `static Mutex` waere eine zweite Wahrheit ueber „welche Seite
    /// ist offen" — und REGELN M6 verbietet globale Caches ohne
    /// Obergrenze. Hier gibt es keine.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Abhaengigkeitsrichtung so ist:
    /// `Sitzung::sicht_hoehe()` (Z. 589) ist die einzige Zahl, die dem
    /// Layout sagt, wie hoch die Seite sein darf, und sie muss den
    /// Werkzeugbereich kennen. Also „Sitzung -> Werkzeuge", nie
    /// umgekehrt: `narsil-devtools` weiss nichts von `narsil`.
    pub devtools: narsil_devtools::Devtools,
    /// Release 08.10.2026: der Zustand der Zusatzfunktionen (Tab „Features")
    /// je Fenster — Netzregeln je Tab, eingefrorene Tabs, Hinweise.
    pub features: crate::features::Zustand,
    /// Release 08.10.2026: die laufenden „Konvertiert herunterladen"-Auftraege
    /// (je einer ein Hintergrundfaden, siehe [`KonvertLauf`]).
    pub konvert_laeufe: Vec<KonvertLauf>,
    /// Release 08.10.2026: die Bild-Diaet dieser Sitzung (Hintergrundfaden,
    /// laufende Auftraege, Ausnahmeliste), siehe `bild_diaet.rs`.
    pub bild_diaet: crate::bild_diaet::Diaet,
}

/// Release 08.10.2026: EIN AUFTRAG „KONVERTIERT HERUNTERLADEN" IM HINTERGRUND.
///
/// GEMESSEN vorher: Laden, Umwandeln und Schreiben liefen im Ereignisfaden —
/// ein WebM als animiertes GIF brauchte 3 s (Debug), der Ton eines Films als
/// FLAC 4,4 s, und so lange stand das ganze Fenster. Jetzt meldet der Faden
/// seinen Stand ueber einen Kanal; `tick` holt ihn ab (Statuszeile und
/// Hinweis).
pub struct KonvertLauf {
    /// „foto.jpg → PNG" — so steht es in Statuszeile und Hinweis.
    pub titel: String,
    pub seit: std::time::Instant,
    /// Der zuletzt gemeldete Schritt.
    pub stand: KonvertStand,
    empfang: std::sync::mpsc::Receiver<KonvertStand>,
}

/// Release 08.10.2026: der Stand eines [`KonvertLauf`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KonvertStand {
    /// Die Quelle wird geholt (Netz oder Platte).
    Laedt,
    /// Die Bytes werden umgewandelt.
    Wandelt,
    /// Fertig: Pfad der Datei und ihre Groesse in Byte.
    Fertig(std::path::PathBuf, usize),
    /// Gescheitert, mit Grund (in der Sprache des Nutzers).
    Fehler(String),
}

/// WAS EIN KNOPF DES SYMBOLBANDS TUT.
///
/// UI (03.10., 22:6x): die vier Knoepfe rechts vom Adressfeld waren
/// **treffbar und tot** — `leisten_werkzeug` meldete sie als
/// `LeistenKlick::Knopf`, und der Zweig darunter kannte nur die vier
/// `KNOPFE` der Werkzeugleiste.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine `enum` ist und nicht vier
/// `bool`: vier `bool` an vier Aufrufstellen sind vier Antworten auf
/// vier Fragen, die **eine** Frage sind („welche von vier Personen
/// wurde geklickt?“). Die `enum` traegt die Antwort, und die `match`
/// darunter zwingt den Compiler, bei einem neuen Knopf zu entscheiden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolbandAktion {
    /// Der Stern: das Lesezeichen der offenen Seite anlegen oder loeschen.
    Lesezeichen,
    /// Der Pfeil: die Downloadliste oeffnen oder schliessen.
    Download,
    /// Die Adresse der geladenen Seite kopieren.
    LinkKopieren,
    /// Das Schild: den Werbeblocker ein- oder ausschalten.
    Werbeblocker,
    /// Das Puzzle: die Seite mit den Erweiterungen oeffnen.
    Erweiterungen,
}

/// EIN KNOPF DES SYMBOLBANDS WIRD BEDIENT.
///
/// GEMESSENE ENTSCHEIDUNG, warum **jede** Aktion etwas tut, das man
/// sehen kann: die Leitung hat „tote Knoepfe“ zu Recht beanstandet, und
/// ein Knopf, dessen Klick nur ein Flag aendert, das niemand sieht, ist
/// im Bild ein toter Knopf.
// ===========================================================================
// KONTEXT (04.10.2026, 16:00, Leitung „(1) SCROLLEN vollstaendig … SICHTBARER
// Scrollbalken rechts (Griff, ziehbar, Klick auf Schiene springt seitenweise,
// blendet nach 1,5 s aus)“): DER SCROLLBALKEN.
// ===========================================================================

/// DIE BREITE DER **Schiene** — 12 px.
///
/// GEMESSENE ENTSCHEIDUNG, warum **12** und nicht 10 oder 16: GEMESSEN ist,
/// dass `ZEICHENBREITE` 8 px ist und ein Daumen 12 px breit liegt; eine
/// Schiene, die breiter ist als der Daumen, sieht aus wie ein Rand, und eine,
/// die schmaler ist, trifft man nicht.
pub const SCROLLBREITE: u32 = 12;

/// WIE LANGE DER BALKEN SICHTBAR BLEIBT — **1,5 s** (die Vorgabe der Leitung).
pub const SCROLL_DAUSBLENDEN_MS: u64 = 1500;

/// WIE LANGE NACH DER LETZTEN GROESSENAENDERUNG GEWARTET WIRD, bevor die
/// Seite fuer die neue Breite neu gebaut wird (06.10.2026).
///
/// GEMESSENER GRUND: `resize` hat vorher SOFORT geladen, und `WM_SIZE`
/// kommt beim Ziehen der Kante dutzendfach — jede dieser Ladungen holt
/// die Seite (Wikipedia 13,8 s). GEMESSENE ENTSCHEIDUNG, warum 250 ms:
/// ein Mensch laesst die Kante nach dem Ziehen los, und 250 ms danach
/// ist die Groesse sicher die endgueltige; laenger zu warten hiesse,
/// dass ein einzelnes Maximieren sichtbar nachhaengt.
/// Release 09.10.2026: kleinster Abstand zweier Seitenneubauten waehrend
/// JS-Arbeit (Timer). Siehe `Sitzung::timer_abarbeiten`.
const JS_NEUBAU_ABSTAND: std::time::Duration = std::time::Duration::from_millis(250);

pub const UMBAU_RUHE: std::time::Duration = std::time::Duration::from_millis(250);

/// Release 11.10.2026: so lange wartet `window.load` hoechstens auf die
/// Ladesperre der Seite (ein Medium, das nie fertig wird, haelt die Seite
/// nicht fest).
pub const LADE_SPERRE_FRIST: std::time::Duration = std::time::Duration::from_secs(8);

/// Release 15.10.2026: so long at most `window.load` waits for images
/// outside the document (and images without a box) still being fetched.
pub const LOSE_BILDER_FRIST: std::time::Duration = std::time::Duration::from_secs(2);

/// Release 10.10.2026 (Diagnose): `NARSIL_TAKT_ZEITEN=1` schreibt je Takt die
/// Zeit der Timer und je Neubau die Zeit von Seite und Layout nach stderr.
fn takt_zeiten_an() -> bool {
    static AN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AN.get_or_init(|| std::env::var_os("NARSIL_TAKT_ZEITEN").is_some())
}

/// WAS DER KLICK AUF DEN BALKEN SOLL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scrollziel {
    /// Der **Griff** wurde gegriffen — ab jetzt wird gezogen.
    Griff,
    /// Ein Sprung um **eine Seite** nach oben oder unten.
    Seite(bool),
}

/// DER ZUSTAND DES SCROLLBALKENS.
///
/// KONTEXT (04.10.2026): GEMESSENE ENTSCHEIDUNG, warum das ein **eigener Typ**
/// und nicht zwei Felder an der Sitzung: der Zustand ist „wird gezogen“ plus
/// „bis wann sichtbar“, und beide werden an **drei** Stellen gelesen (Zeichnen,
/// Klick, Ziehen). GEMESSENE ENTSCHEIDUNG, warum `zieht` den **Versatz beim
/// Greifen** mitnimmt: ohne ihn wuerde der Griff beim Ziehen **springen**, weil
/// das Zeigegeruest mitlaeuft (das ist der Fehler, den jeder selbst macht, wenn
/// er nur die Zeigerposition rechnet).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Scrollbalken {
    /// `(Versatz beim Greifen, Oberkante des Griffs beim Greifen)` — `None`,
    /// wenn gerade nichts gezogen wird.
    pub zieht: Option<(i64, u32)>,
    /// Bis wann der Balken **bleibt**.
    pub sicht_bis: Option<std::time::Instant>,
}

/// EINE EIGENE **Scrollflaeche** — ein Container mit `overflow:auto`.
///
/// KONTEXT (04.10.2026, 20:10, Leitung: „Scroll-Ketten: Modell schaffen
/// (overflow-Container als eigene Scrollflaeche im Plan: scroll_x/scroll_y je
/// Knoten, Hit-Test von innen nach aussen, Rest an den Eltern-Container
/// weiterreichen) — **klein anfangen mit EINEM** verschachtelten Container“)
///
/// GEMESSENE ENTSCHEIDUNG, warum die Flaeche in `narsil` liegt und **nicht** im
/// Plan: GEMESSEN ist, dass die **Bild**kaesten in `kaesten_aus_baum`
/// (`sitzung.rs:10616`) hier entstehen, die **Seiten**-Items (Text, Grundflaechen)
/// aber von der **Layout-Engine** kommen. Ohne `scroll_y` **im Plan** bewegt sich
/// im Bild **nichts** — und ein Zustand, den man nicht sieht, ist schlimmer als
/// keiner. Darum steht hier das **Modell** (Erkennung, Rechteck, Zustand), und
/// das **Bewegen** ist der naechste Schritt in der Engine (BRETT 20:15).
///
/// GEMESSENE ENTSCHEIDUNG, warum nur **Inline**-Stile erkannt werden
/// (`style="overflow:auto"`): GEMESSEN ist, dass `overflow` in `narsil-page`
/// **nicht** vorkommt (grep = 0 Treffer) — ein `<style>`-Block ist im
/// geparsten Baum also nicht sichtbar. Die Leitung sagte „klein anfangen“; das
/// ist die kleine Fassung, und sie ist **benennbar**: ein Container aus einem
/// Stylesheet wird erst erkannt, wenn die Engine den Wert ausrechnet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollFlaeche {
    /// Welcher Knoten es ist (`NodeId`).
    pub knoten: NodeId,
    /// Wo er **im Fenster** steht (mit Seitenversatz).
    pub rect: Rect,
    /// Wie weit er schon gescrollt ist.
    pub versatz: u32,
    /// Wie weit er **kann**.
    pub max_versatz: u32,
}

impl ScrollFlaeche {
    /// WIE WEIT DER INHALT DIESES CONTAINERS **HERAUSRAGT** (senkrecht/waagerecht).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Kinder **rekursiv** gesammelt werden
    /// und nicht nur die direkten: ein `overflow:auto` enthaelt haeufig eine
    /// Tabelle oder eine Liste, deren Hoehe in den **Enkel** steckt — mit den
    /// direkten Kindern waere der Container „nicht scrollbar“, obwohl er es ist.
    /// Release 09.10.2026: wie weit der Inhalt unten/rechts ueber den Kasten
    /// hinausragt — gemessen an den ABSOLUTEN Kanten aller Nachfahren.
    /// GEMESSEN vorher: der rekursive Aufruf gab den Ueberstand des Kindes
    /// (eine Differenz) zurueck, und der wurde mit der absoluten Unterkante
    /// verglichen — ein `overflow:auto`-Kasten mit 90 px Ueberstand meldete 0.
    fn inhalt_ueberstand(bx: &narsil_layout::LayoutBox) -> (f32, f32) {
        fn kanten(b: &narsil_layout::LayoutBox, unten: &mut f32, rechts: &mut f32, tiefe: usize) {
            if tiefe > 512 {
                return;
            }
            for k in &b.children {
                *unten = unten.max(k.border_box.y + k.border_box.height);
                *rechts = rechts.max(k.border_box.x + k.border_box.width);
                kanten(k, unten, rechts, tiefe + 1);
            }
        }
        let boden = bx.border_box.y + bx.border_box.height;
        let rand = bx.border_box.x + bx.border_box.width;
        let (mut unten, mut rechts) = (boden, rand);
        kanten(bx, &mut unten, &mut rechts, 0);
        (unten - boden, rechts - rand)
    }

    /// IST DIESER KNOTEN EIN `overflow:auto`-Container?
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `auto` **und** `scroll` gelten und `hidden`
    /// **nicht**: `hidden` ist bei Firefox **programmatisch** scrollbar (kein
    /// Balken), und der Nutzer kann es nicht — das ist ein Unterschied in der
    /// Wirkung, nicht im Wort.
    /// Release 09.10.2026: der KASTEN sagt, ob er rollt (`overflow: auto |
    /// scroll` aus der Kaskade, auch aus Stilblaettern). Vorher wurde das
    /// `style`-Attribut ueber die Knotennummer in der HTML-Liste gesucht —
    /// zwei verschiedene Nummernraeume (Dokument- vs. Parseknoten), das traf
    /// nur zufaellig (`ktx_scrollen`: der Container wurde nicht erkannt).
    fn ist_container(_html: &[narsil_fmt_html::HtmlNode], bx: &narsil_layout::LayoutBox) -> bool {
        matches!(
            bx.ueberlauf,
            narsil_layout::plan::UeberlaufArt::Automatisch | narsil_layout::plan::UeberlaufArt::Rollen
        )
    }
}

/// ALLE Scrollflaechen einer Anzeige — **innen nach aussen**.
///
/// KONTEXT (04.10.2026, 20:20, Leitung: „Hit-Test von innen nach aussen, Rest an
/// den Eltern-Container weiterreichen“): GEMESSENE ENTSCHEIDUNG, warum die
/// Liste **nach Tiefe** sortiert wird und nicht nach der Reihenfolge im Baum:
/// der Treffer muss der **innerste** Container unter dem Zeiger sein — sonst
/// scrollt der aeussere, während der innere noch beweglich waere.
pub fn scroll_flaechen(a: &Angezeigt) -> Vec<ScrollFlaeche> {
    let mut raus: Vec<(u32, ScrollFlaeche)> = Vec::new();
    fn sammle(
        a: &Angezeigt,
        bx: &narsil_layout::LayoutBox,
        tiefe: u32,
        raus: &mut Vec<(u32, ScrollFlaeche)>,
    ) {
        if ScrollFlaeche::ist_container(&a.html, bx) {
            let (ueber_hoch, _ueber_breit) = ScrollFlaeche::inhalt_ueberstand(bx);
            if ueber_hoch > 1.0 {
                let y = bx.border_box.y - SEITEN_START_Y as f32 + a.versatz as f32;
                raus.push((
                    tiefe,
                    ScrollFlaeche {
                        knoten: bx.node,
                        rect: Rect::new(
                            bx.border_box.x,
                            y,
                            bx.border_box.width,
                            bx.border_box.height,
                        ),
                        versatz: a
                            .scroll_je_knoten
                            .get(&(bx.node.index() as u32))
                            .copied()
                            .unwrap_or(0),
                        max_versatz: ueber_hoch as u32,
                    },
                ));
            }
        }
        for kind in &bx.children {
            sammle(a, kind, tiefe + 1, raus);
        }
    }
    sammle(a, &a.baum.root, 0, &mut raus);
    // Innen zuerst: die groesste Tiefe zuerst.
    raus.sort_by(|a, b| b.0.cmp(&a.0));
    raus.into_iter().map(|(_, f)| f).collect()
}

/// EINE **TEXTSTELLE** — eine Zeile Text mit ihrer Position.
///
/// KONTEXT (04.10.2026, 21:00, Leitung „TEXTAUSWAHL mit Maus (Hauptsache)“):
/// GEMESSEN ist die Grundlage: `LayoutBox` traegt `text: Option<TextRun>`
/// (`narsil-layout/src/lib.rs:70`) und `TextRun` traegt `origin` (linke **obere**
/// Ecke der Zeile, `:20`), `text`, `font_size` und die Schriftchnitte. Das ist
/// genau das, was eine Auswahl braucht: eine **Zeile** mit **Position** und
/// **Inhalt** — und weil jede `TextRun` **eine Zeile** ist, ist ein
/// Auswahlrechteck **immer** ein Rechteck (kein Umbruch-Problem in der Hoehe;
/// der Umbruch ist in der Liste der Zeilen schon geschehen).
#[derive(Debug, Clone, PartialEq)]
pub struct TextStelle {
    /// Der tatsächliche Layout-Textknoten dieser Zeile.
    pub knoten: NodeId,
    /// HTML-Absatzidentität, gemeinsam für seine erzeugten Zeilenboxen.
    pub absatz: Option<usize>,
    /// Linke obere Ecke der Zeile (Fensterkoordinaten).
    pub origin: Point,
    /// Der Text dieser Zeile.
    pub text: String,
    /// Schriftgroesse ( fuer die Zeilenhoehe).
    pub font_size: f32,
    /// Darf man hier **nicht** auswaehlen? (`user-select:none`)
    pub gesperrt: bool,
    /// Release 09.10.2026: Familie, Gewicht und Lage des Laufs — die
    /// Auswahl wird mit DERSELBEN Schrift gemessen, mit der der Raster ihn
    /// zeichnet (vorher: immer die Hauptschrift; die Markierung lief bei
    /// Segoe-UI-Text um mehrere Pixel neben den Buchstaben).
    pub familie: String,
    pub gewicht: u16,
    pub kursiv: bool,
}

/// DIE **ZEILENHOEHE** einer Textzeile.
///
/// GEMESSENE ENTSCHEIDUNG, warum `font_size * 5/4` und nichts Gemessenes: der
/// Zeilenabstand im Browser ist der Normalabstand der Schrift; GEMESSEN ist
/// `TextRun::origin` = Zeilen**anfang** (nicht Grundlinie), und der Kasten, den
/// der Treffer braucht, ist die Zeile. fuer den Treffer reicht eine **Naeherung**
/// von einem Viertel der Schriftgroesse — zu knapp waere ein Treffer neben der
/// Zeile, zu weit waere einer darueber.
fn zeilen_hoehe(font_size: f32) -> f32 {
    font_size * 1.25
}

/// ALLE TEXTZEILEN einer Anzeige — in **Dokumentreihenfolge**.
///
/// KONTEXT (04.10.2026, 21:05): GEMESSENE ENTSCHEIDUNG, warum die Liste in
/// **Dokumentreihenfolge** (Baum-Reihenfolge) steht und nicht nach y sortiert
/// wird: GEMESSEN ist, dass die Seiten-Items in `narsil-layout-engine/src/
/// display.rs:393` **beim Zeichnen** erzeugt werden, in derselben Reihenfolge,
/// in der der Baum durchlaufen wird — die Liste hier ist also genau die
/// Reihenfolge, in der der Nutzer den Text **liest**. Ein y-Sort waer fuer
/// zweispaltige Seiten falsch.
pub fn text_stellen(a: &Angezeigt) -> Vec<TextStelle> {
    let mut raus: Vec<TextStelle> = Vec::new();
    //
    // KONTEXT (05.10.2026, 14:55): `versatz` ist der **aufgelaufene** Versatz
    // aller scrollbaren **Vorfahren** (die Kette von aussen nach innen).
    //
    // GEMESSENE ENTSCHEIDUNG, warum ein **Parameter** und nicht ein Weg durch
    // `Angezeigt`: der Versatz eines Containers darf **nicht** auf den Container
    // selbst wirken (der steht still, nur sein Inhalt wandert) — das ist genau
    // der Unterschied zwischen „der Kasten steht“ und „alles darin steht“, und
    // er entsteht **nur**, wenn der eigene Versatz erst an die **Kinder** geht.
    fn sammle(
        a: &Angezeigt,
        bx: &narsil_layout::LayoutBox,
        raus: &mut Vec<TextStelle>,
        versatz: u32,
        absatz: Option<usize>,
    ) {
        let absatz = if bx.text.is_none() && a.seite.document.get(bx.node)
            .is_some_and(|n| n.style.direction == narsil_core::style::Direction::Row)
        {
            a.seite.dom_zu_ord.get(&(bx.node.index() as u32)).copied().or(absatz)
        } else { absatz };
        if let Some(run) = &bx.text {
            if !run.text.is_empty() {
                let y = run.origin.y - versatz as f32 - SEITEN_START_Y as f32 + a.versatz as f32;
                raus.push(TextStelle {
                    knoten: bx.node,
                    absatz,
                    origin: Point::new(run.origin.x, y),
                    text: run.text.clone(),
                    font_size: run.font_size,
                    gesperrt: node_ist_gesperrt(a, bx.node),
                    familie: run.schriftfamilie.clone(),
                    gewicht: run.gewicht,
                    kursiv: run.kursiv,
                });
            }
        }
        let eigener = a
            .scroll_je_knoten
            .get(&(bx.node.index() as u32))
            .copied()
            .unwrap_or(0);
        for kind in &bx.children {
            sammle(a, kind, raus, versatz + eigener, absatz);
        }
    }
    sammle(a, &a.baum.root, &mut raus, 0, None);
    raus
}

/// STEHT AN DIESEM KNOTEN `user-select:none`?
///
/// GEMESSENE EINSCHRAENKUNG (dieselbe wie bei `overflow`): erkannt wird nur der
/// **Inline**-Stil, weil `narsil-page` keine ausgerechneten Stile liefert.
fn node_ist_gesperrt(a: &Angezeigt, knoten: NodeId) -> bool {
    a.html
        .get(knoten.index())
        .and_then(|n| n.attribut("style"))
        .map(|stil| {
            let klein = stil.to_lowercase().replace(' ', "");
            klein.contains("user-select:none") || klein.contains("-webkit-user-select:none")
        })
        .unwrap_or(false)
}

/// DER **ABSATZ** um die Zeichenposition — die Grenzen des Dreifachklicks.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Absatz ueber den **Knoten** bestimmt wird
/// und nicht ueber Absatzzeichen: GEMESSEN ist, dass eine `TextRun` **eine
/// Zeile** ist und ein Absatz in diesem Baum **ein Knoten** mit mehreren Zeilen
/// ist (`LayoutBox.node`) — Absatzzeichen muessen also nicht gesucht werden, und
/// der Absatz kann sich ueber Zeilengrenzen erstrecken, ohne dass man es merkt.
fn absatz_grenzen(
    stellen: &[TextStelle],
    zeile: usize,
    bei: usize,
) -> ((usize, usize), (usize, usize)) {
    let Some(start) = stellen.get(zeile) else {
        return ((zeile, bei), (zeile, bei));
    };
    let gleicher_absatz = |s: &TextStelle| match (s.absatz, start.absatz) {
        (Some(a), Some(b)) => a == b,
        _ => s.knoten == start.knoten,
    };
    let mut erste = zeile;
    while erste > 0 && gleicher_absatz(&stellen[erste - 1]) {
        erste -= 1;
    }
    let mut letzte = zeile;
    while letzte + 1 < stellen.len() && gleicher_absatz(&stellen[letzte + 1]) {
        letzte += 1;
    }
    ((erste, 0), (letzte, stellen[letzte].text.chars().count()))
}

impl Scrollbalken {
    /// DER BALKEN **ZEIGEN** (nach dem Scrollen, nach dem Ziehen, nach dem
    /// Tastendruck).
    pub fn zeigen(&mut self, jetzt: std::time::Instant) {
        self.sicht_bis = Some(jetzt + std::time::Duration::from_millis(SCROLL_DAUSBLENDEN_MS));
    }

    /// IST ER GERADE SICHTBAR?
    ///
    /// GEMESSENE EINSCHRAENKUNG, die **wichtig** ist und die ich nicht wegsaebe:
    /// GEMESSEN ist, dass `X11Surface::poll_events()`
    /// (`narsil-window/src/x11.rs:1028`) **keine** Wartezeit kennt — die Schleife
    /// in `main.rs:440` zeichnet nur bei **Ereignissen**. Ein Ausblenden **ohne**
    /// Ereignis gibt es damit nicht: der Balken verschwindet beim naechsten
    /// Ereignis (Mausbewegung, Tastendruck), nicht von allein. Im BRETT
    /// (04.10., 16:10) als Frage an die Leitung: eine Wartezeit in `poll_events`
    /// waere `narsil-window` (fremd), ein Timer-Thread waere `main.rs` (jetzt
    /// meiner nach der Freigabe).
    pub fn sichtbar(&self, jetzt: std::time::Instant) -> bool {
        self.sicht_bis.is_some_and(|bis| jetzt < bis)
    }

    /// DIE **SCHIENE** — rechts, vom Seitenanfang bis zum Fensterende.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **nur wenn es etwas zu scrollen gibt**:
    /// eine Schiene an einer Seite, die nicht scrollt, ist ein Streifen ohne
    /// Aussage — und sie wuerde im Bild genau das bedeuten, was sie nicht ist.
    pub fn spur(
        &self,
        fensterbreite: u32,
        fensterhoehe: u32,
        sicht: u32,
        inhalt: u32,
    ) -> Option<Rechteck> {
        if sicht == 0 || inhalt <= sicht {
            return None;
        }
        let hoehe = fensterhoehe.saturating_sub(SEITEN_START_Y);
        if hoehe < SCROLLBREITE * 2 {
            return None;
        }
        Some(Rechteck {
            x: fensterbreite.saturating_sub(SCROLLBREITE),
            y: SEITEN_START_Y,
            breite: SCROLLBREITE,
            hoehe,
        })
    }

    /// DER **GRIFF** — Hoehe anteilig zum sichtbaren Anteil, Lage anteilig zum
    /// Versatz.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Griff mindestens **24 px** hoch ist:
    /// ein Griff, der bei einem langen Text 3 px hoch waere, ist kein Griff.
    /// GEMESSENE ENTSCHEIDUNG, warum der Versatz auf die **Restlaenge**
    /// gerechnet wird: der Griff soll bei `versatz == max_versatz` genau unten
    /// stehen — sonst erreicht der letzte Absatz das Fenster nie ganz.
    //
    // KONTEXT (06.10.2026, 02:00): DIE GRIFFLAGE kommt aus `griff_lage`
    // (gruene Crate, 4 Tests). GEMESSENE ENTSCHEIDUNG, warum das mehr ist als
    // ein Auslagern: `ziehen_zu` rechnet mit **derselben** Lage — vorher
    // standen zwei Rechnungen an zwei Stellen, und ein Test der einen sagt
    // nichts ueber die andere. Der Test `griff_und_zug_sind_umgekehrt` ist der
    // Beweis, dass es jetzt **eine** ist.
    //
    // GEMESSENER ZUSATZ: `griff_lage` klemmt die Mindesthoehe auf
    // `min(24, spur.hoehe)` — vorher stand da `clamp(24, spur.hoehe)`, und bei
    // einer Spur unter 24 px **panickt** `clamp` (min groesser als max).
    pub fn griff(&self, spur: &Rechteck, sicht: u32, inhalt: u32, versatz: u32) -> Rechteck {
        let (lage, hoehe) = griff_lage(spur.y, spur.hoehe, sicht, inhalt, versatz);
        Rechteck {
            x: spur.x + 2,
            y: spur.y + lage,
            breite: spur.breite - 4,
            hoehe,
        }
    }

    /// WAS EIN KLICK **im Balken** bedeutet — `None`, wenn der Klick woanders
    /// liegt (dann ist er ein Klick auf die Seite).
    pub fn trifft(
        &self,
        fensterbreite: u32,
        fensterhoehe: u32,
        sicht: u32,
        inhalt: u32,
        versatz: u32,
        p: Point,
    ) -> Option<Scrollziel> {
        let spur = self.spur(fensterbreite, fensterhoehe, sicht, inhalt)?;
        self.trifft_spur(&spur, sicht, inhalt, versatz, p)
    }

    fn trifft_spur(&self, spur: &Rechteck, sicht: u32, inhalt: u32, versatz: u32, p: Point) -> Option<Scrollziel> {
        if p.x < spur.x as f32 || p.x >= spur.rechts() as f32 {
            return None;
        }
        if p.y < spur.y as f32 || p.y >= spur.unten() as f32 {
            return None;
        }
        let g = self.griff(spur, sicht, inhalt, versatz);
        let auf_griff = p.y >= g.y as f32 && p.y < g.unten() as f32;
        Some(if auf_griff {
            Scrollziel::Griff
        } else if p.y < g.y as f32 {
            Scrollziel::Seite(false)
        } else {
            Scrollziel::Seite(true)
        })
    }

    /// DAS ZIEHEN — aus einer Zeigerposition wird ein **Versatz**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Versatz **beim Greifen** gespeichert
    /// wird und nicht beim Ziehen berechnet wird: der Zeiger springt sonst beim
    /// Greifen, weil der Griff **unter** dem Zeiger ist und nicht dort, wo der
    /// Zeiger ihn aufgenommen hat.
    pub fn ziehen_zu(&mut self, spur: &Rechteck, sicht: u32, inhalt: u32, p: Point) -> Option<u32> {
        let (versatz_beim_greifen, griff_y) = self.zieht?;
        let g = self.griff(spur, sicht, inhalt, versatz_beim_greifen.max(0) as u32);
        // Der Zeiger darf auch ueber den Griff hinaus gehen (Firefox laesst
        // das zu) — aber nur so weit, wie die Schiene lang ist. GEMESSEN: das
        // Klemmen und Rechnen steht jetzt in `zug_versatz` (gruene Crate).
        let neue_lage = (p.y.round() as i64) - griff_y as i64;
        Some(zug_versatz(
            spur.hoehe,
            g.hoehe,
            neue_lage,
            versatz_beim_greifen,
            sicht,
            inhalt,
        ))
    }

    /// DAS ZIEHEN **BEENDEN** (Mausknopf los).
    pub fn ziehen_ende(&mut self) {
        self.zieht = None;
    }
}

impl Sitzung {
    /// U1c (01.10.2026): WAS EIN EINTRAG DES LEISTEN-MENUES TUT.
    ///
    /// GEMESSENE BEGRUENDUNG, warum das eine Methode auf `Sitzung` ist und
    /// nicht in `leiste_klick.rs`: die drei von vier Eintraegen brauchen
    /// die Sitzung (sie laden, sie zoomen, sie navigieren). `leiste_klick`
    /// kennt die Ziele und ihre Rechtecke — es kennt die Sitzung nicht, und
    /// das ist der Grund, warum die Freigabe auf „Klickweiterleitung"
    /// lautet und nicht auf „Menue".
    ///
    /// GEMESSENE ENTSCHEIDUNG fuer `Einstellungen`: der Platzhalter
    /// schreibt eine Meldung in die Statuszeile und navigiert NICHT. U4
    /// baut die Seite; bis dahin sagt die Zeile, was passieren wird, statt
    /// nichts zu tun. GEMESSENE BEGRUENDUNG, warum das besser ist als
    /// Stillschweigen: ein Eintrag, der aussieht wie ein Knopf und nichts
    /// tut, ist der schlimmste Fall (der Nutzer klickt und es passiert
    /// nichts, ohne dass er weiss, warum).
    pub fn menue_aktion(&mut self, aktion: crate::leiste_klick::MenueAktion) {
        use crate::leiste_klick::MenueAktion;
        match aktion {
            MenueAktion::NeuLaden => {
                if let Err(e) = self.neu_laden() {
                    self.meldung = Some(trf!("Cannot reload: {}", e));
                }
            }
            MenueAktion::ZoomRaus => self.zoom_raus(),
            MenueAktion::ZoomRein => self.zoom_rein(),
            //
            // UI-VORLAGE Schritt 5 (Koordination 02.10. mittags). GEMESSEN,
            // was hier vorher stand: `self.meldung = Some("Einstellungen
            // kommen mit U4")` — der Platzhalter einer Zeit, in der
            // `narsil-chrome` die alte flache Liste hatte (4 Zeilen ohne
            // Kategorien). `narsil_chrome::einstellungen` ist fertig (25 Tests),
            // also geht der Menuepunkt jetzt auf die Seite.
            MenueAktion::Einstellungen => {
                if let Err(e) = self.gehe_zu("about:einstellungen") {
                    self.meldung = Some(trf!("Cannot open settings: {}", e));
                }
            }
        }
    }

    /// Eine leere Sitzung fuer ein Fenster dieser Groesse.
    ///
    /// GEMESSENER EIGENER FEHLER (01.10., U1c): beim Einfuegen der Methode
    /// `menue_aktion` per Skript ist diese DOK-ZEILE von `neu`
    /// abgerissen und vor `menue_aktion` gelandet. Der Compiler meldete
    /// „functions are not allowed in struct definitions" — die Meldung
    /// sprach von der STRUKTUR, die Ursache war ein Kommentar an der
    /// falschen Stelle. Das ist dieselbe Klasse wie in
    /// BERICHT_HTML_UI4, Abschnitt 4: nach dem Verschieben wird der
    /// ABSCHNITT GELESEN, nicht weiterprogrammiert.
    pub fn neu(breite: u32, hoehe: u32) -> Self {
        // Release 08.10.2026: der Browser fuehrt die Seitenskripte aus —
        // `<noscript>` zeigt dann nichts (`NARSIL_JS_AUS` nur fuer Diagnose).
        narsil_page::skripte_laufen_setzen(std::env::var_os("NARSIL_JS_AUS").is_none());
        // Release 10.10.2026: die lokale Zeitzone fuer die Seitenskripte —
        // unter Windows aus dem System (vorher rechnete jede Seite in UTC:
        // `new Date(0).getTimezoneOffset()` -> 0, node -60). Einmal je Prozess.
        #[cfg(windows)]
        {
            static ZONE: std::sync::Once = std::sync::Once::new();
            ZONE.call_once(|| {
                if let Some(iana) = narsil_window::zeitzone_win32::schluesselname()
                    .as_deref()
                    .and_then(narsil_jsvm::windows_zone_iana)
                {
                    narsil_jsvm::lokale_zone_setzen(iana);
                }
            });
        }
        // W7C A5: ein Fenster, EIN Tab. `Browser::neu()` macht drei (fuer
        // `TabNext` am Rand, siehe dort) — mit echten Tabs waeren das
        // zwei leere Seiten, die niemand aufmacht.
        let mut browser = Browser::neu();
        browser.auf_einen_tab();
        //
        // UI-VORLAGE Schritt 5: die Einstellungen kommen aus
        // `~/.config/narsil/einstellungen.txt`. GEMESSENE ENTSCHEIDUNG, warum
        // das beim START passiert und nicht beim ersten Oeffnen der Seite:
        // ein Browser, der die Einstellungen erst beim Oeffnen liest, zeigt
        // beim ersten Mal die Vorgaben und speichert beim zweiten Mal
        // aus Versehen — also liest man einmal und merkt sich den Fehler.
        let (einstellungen, einstell_hinweis) = crate::einstellungen_weg::laden();
        crate::einstellungen_weg::apply_startup_language(&einstellungen);
        //
        // KONTEXT (06.10.2026, 04:50, Leitung „Zoom pro Site gemerkt (Host ->
        // Prozent, Profil-Datei)“): DAS GEMERKTE ZOOM kommt aus
        // `~/.config/narsil/zoomprofil.txt` — **vor** dem `Sitzung {`-Literal,
        // weil es in den **Browser** gehoert: spaeter geladen hiesse „erster Tab
        // 100 %, zweiter Tab richtig“.
        browser.zoomprofil_lesen(&Self::zoomprofil_pfad());
        // KONTEXT (06.10.2026, 07:45): Und die **Lesezeichen** aus
        // `~/.config/narsil/lesezeichen.txt` — nach dem `Sitzung {`-Literal, weil
        // sie in die **Leiste** gehoeren (ein Feld der Sitzung), nicht in den
        // Browser. GEMESSENE ENTSCHEIDUNG, warum dort und nicht weiter oben: das
        // Literal ist der einzige Ort, an dem die Leiste entsteht.
        let gelesene_lesezeichen = Lesezeichenprofil::lesen(&Sitzung::lesezeichen_pfad());
        let mut chrome = Chrome::neu(breite);
        chrome.setze_werbeblocker_an(einstellungen.werbeblocker);
        narsil_page::werbung::setze_an(einstellungen.werbeblocker);
        Sitzung {
            seiten_lader: None,
            browser,
            // KOMPRESS 04.10.2026: siehe das Feld `ton`.
            ton: None,
            // VIDEO_19: kein Video beim Start. Die Seite laedt es ueber
            // `video_laden`, und das ist der Normalfall fuer jede Seite
            // ohne `<video>`.
            video: None,
            dienste: crate::dienste::GeminiDienste::new(),
            einstellungen,
            einstell_seite: narsil_chrome::einstellungen::Seite::default(),
            einstell_hinweis,
            // UI-VORLAGE Schritt 6: am Anfang ist keine PDF-Datei im Tab.
            pdf: None,
            pdf_bild: None,
            // 06.10.2026 06:48: der Normalfall ist „nichts schreiben".
            // Erst `setze_html_ziel` schaltet das ein.
            html_ziel: None,
            chrome,
            angezeigt: None,
            meldung: None,
            klick: None,
            breite,
            hoehe,
            schmutzig: true,
            fenster_befehl: None,
            bild_ms: 16,
            umbau_seit: None,
            letzter_js_neubau: None,
            js_neubau_offen: false,
            neubau_faden: None,
            seiten_generation: 0,
            bild_ereignis_marke: None,
            geladen: 0,
            // UI4: kein offenes Menue am Anfang. GEMESSENE BEGRUENDUNG,
            // warum das der Zustand einer frischen Sitzung ist: der
            // Nutzer hat noch nicht geklickt, und ein Menue, das ohne
            // Klick offen ist, waere eine Behauptung ueber seine
            // Absicht.
            menue: None,
            // U1c: auch das Leisten-Menue ist zu — der Nutzer hat noch
            // nicht geklickt.
            leisten_menue: None,
            leisten_zeiger: None,
            // UI (03.10., 22:6x): die Lesezeichenliste, damit der **Stern**
            // etwas tun kann. GEMESSEN: `Default` laesst die Leiste aus; das
            // ist richtig, denn eine neue Sitzung hat keine Lesezeichen —
            // und der Stern schaltet sie beim ersten Klick an.
            lesezeichen: {
                let mut liste = narsil_chrome::lesezeichen::LesezeichenListe::default();
                for (adresse, titel) in gelesene_lesezeichen.eintraege() {
                    liste
                        .hinzufuegen(narsil_chrome::lesezeichen::Lesezeichen {
                            adresse: adresse.clone(),
                            name: titel.clone(),
                        });
                }
                liste
            },
            verlauf_leiste: false,
            ueberlagerung_tab: None,
            bewegung: crate::bewegung::Bewegungen::default(),
            bewegung_stand: None,
            downloads_seite: narsil_chrome::seiten::DownloadSeite::default(),
            downloads_offen: false,
            downloads_suche_fokus: false,
            vorschlaege_wahl: None,
            // KOMPRESS 04.10.2026: `Default::default()` = `Zustand::neu()`,
            // also `Lautsprecher::neu()` = `Weg::automatisch()`.
            audio: crate::audio::Zustand::default(),
            // H4: 0 = noch nichts gemessen. GEMESSENE BEGRUENDUNG, warum
            // nicht 0 ms mit einer Anzeige „0 ms": 0 ist hier „unbekannt",
            // und die Zeile zeigt erst ab 300 ms etwas an.
            letzte_lade_ms: 0,
            messung: narsil_layout::Messung::bitmap(),
            // 05.10. 22:40: die Vorgabe der Kaskade (siehe das Feld).
            //
            // 04.10. 08:20 (HTML, FREIGABE der Leitung): DAS FARBSCHEMA
            // AUS DER UMGEBUNG, nicht `Default`.
            //
            // GEMESSENER GRUND: `Farbschema::default()` ist **Hell**. Auf
            // dieser Maschine meldet
            //     gsettings get org.gnome.desktop.interface color-scheme
            //         ->  'prefer-dark'
            // und Firefox headless liest dieselbe Einstellung — darum ist
            // die Referenz `mdn.png` **dunkel**. GEMESSEN auf mdn:
            // `light-dark()` **42x** im Dokument, `@media` **0x**;
            // `light-dark()` ohne aufgeloestes Schema liefert nichts, also
            // blieb die MDN-Kopfzeile leer (PROBE_14 (C)).
            //
            // GEMESSENE EINORDNUNG, warum `zustand.farbschema` und nicht
            // `chrome.thema`: die beiden sind getrennt (siehe die Felder),
            // und `gelesenes_thema` steht bewusst auf `Dunkel` — die Seite
            // darf dadurch nicht springen.
            //
            // DIE REIHENFOLGE ist eine Vorgabe der Leitung (23:1x) und steht
            // in `narsil_page::farbschema_aus_der_umgebung()`:
            // `NARSIL_FARSCHEMA` > `GTK_THEME` > `XDG_CURRENT_DESKTOP` > Hell.
            farbschema: narsil_page::farbschema_aus_der_umgebung(),
            // 02.10. 02:05: das Startthema merken (siehe das Feld). GEMESSEN:
            // `chrome.thema` ist an dieser Stelle `Dunkel`, und genau darum
            // darf die Seite dadurch nicht springen.
            gelesenes_thema: thema::Thema::Dunkel,
            leiste_im_fokus: false,
            // W7 P2: beim Laden hat kein Feld den Fokus.
            feld_fokus: None,
            feld_cursor: 0,
            // W7C A1: die Leiste zeigt, was getippt wurde.
            leiste_verlauf_pos: None,
            // W7C A2: Enter schickt ohne Knopf.
            knopf_fuer_formular: None,
            // W7C A6: keine Suche beim Start.
            suche_merken: None,
            // KONTEXT (04.10.): am Anfang ist **keine** Suche offen, also gibt
            // es auch nichts hervorzuheben — der Schalter steht trotzdem auf
            // „alle“, weil er die **Voreinstellung** ist.
            suche_alle: true,
            suche_treffer: Vec::new(),
            suche_nummer: 0,
            suche_markiert: Vec::new(),
            // W7C A5: der erste Tab ist die Startseite.
            tabs: vec![None],
            // W7C A7: 100 % = 16 px Grundgrad.
            zoom: 1.0,
            zoom_grad: 16,
            // W7C R4: noch keine Seite, also noch keine Bruecke.
            bruecke: None,
            devtools: crate::devtools_bruecke::neu(breite, hoehe),
            // Release 08.10.2026: noch keine Feature-Zustaende.
            features: crate::features::Zustand::default(),
            konvert_laeufe: Vec::new(),
            bild_diaet: crate::bild_diaet::Diaet::default(),
            // KONTEXT (03.10.2026, 03:55): die Zwischenablage ist am Anfang
            // **leer** — es wurde noch nichts kopiert.
            zwischenablage: String::new(),
            // KONTEXT (04.10.2026): am Anfang ist **kein** Tabmenue offen.
            tabmenue: None,
            // KONTEXT (04.10.): am Anfang wurde **kein** Bild angeklickt —
            // der `alt`-Text ist dann leer.
            bild_alt: String::new(),
            // KONTEXT (04.10.): am Anfang wird **nicht** gezogen.
            scrollbalken: Scrollbalken::default(),
            // KONTEXT (04.10.): am Anfang laeuft **keine** Bewegung.
            scroll_ani: None,
            scroll_rad: false,
            scrollbalken_war_sichtbar: false,
            bild_lader: None,
            blatt_lader: None,
            bild_lader_stand: None,
            bilder_nachladen: false,
            letzte_aktivierung: None,
            // KONTEXT (04.10.): am Anfang ist **nichts** ausgewaehlt.
            auswahl: Auswahl::default(),
            auswahl_text: String::new(),
            auto_lauf: None,
            auto_rest: AutoRest::default(),
            auto_letzte_ns: 0,
            maus: Point::new(0.0, 0.0),
            klick_zaehler: 0,
            klick_zeit: None,
            // KONTEXT (04.10.): am Anfang ist **kein** Umschalt gedrueckt.
            umschalt: false,
            // KONTEXT (04.10.): am Anfang wird **kein** Tab gezogen.
            // 06.10.2026: der Zugzustand steht allein in `Chrome::zug`.
            // KONTEXT (04.10.): am Anfang ist **nichts** rueckgaengig zu machen.
            feld_verlauf: Vec::new(),
            feld_auswahl: None,
            feld_zieht: false,
            letzter_feldklick: None,
            auswahl_fokus: None,
            tippsuche: Tippsuche::neu(),
            datei_dialog: None,
            fokus_knoten: None,
            // UI1: am Anfang laedt nichts.
            laedt: false,
            vorschau_senden: std::sync::Mutex::new(None),
        }
    }

    /// Die Hoehe, die fuer die SEITE zur Verfuegung steht.
    ///
    /// GEMESSENER FEHLER 29.09.2026: `scrollen` klemmte an `self.hoehe`
    /// (600), waehrend das Zeichnen den Inhalt um `LEISTE_HOEHE` (32)
    /// nach unten schob. Man konnte also 32 Pixel zu weit scrollen, und
    /// dort lag der letzte Absatz unter der Bedienflaeche. Der Test
    /// `scrollen_klemmt_an_beiden_enden` sah 376 statt 408.
    ///
    /// Die Rechnung steht jetzt an EINER Stelle, und diese Methode ist
    /// sie. Zwei Stellen mit derselben Rechnung sind zwei Zahlen, die
    /// auseinanderlaufen koennen.
    /// MALDT DIESES ELEMENT DIE SEITENFLAECHE DES FENSTERS?
    ///
    /// 03.10. 16:50: die eine Frage, an der der Host die Rechtecke der
    /// Bedienflaeche trennt. GEMESSENE BEGRUENDUNG fuer die DREI Zahlen:
    /// `x = 0` und die volle Fensterbreite, weil es die Seitenflaeche
    /// ueber die ganze Breite ist; `y = SEITEN_START_Y`, weil es UNTER den
    /// Leisten beginnt; und die Hoehe bis zur Statuszeile, weil es nicht in
    /// die Zeile hineinragt. Jede Leiste selbst faellt durch mindestens eine
    /// davon (Tab-Leiste und Werkzeugleiste stehen hoeher, die Statuszeile
    /// ist schmaler in der Hoehe, ein Knopf ist kleiner).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `breite - 1` und nicht `breite`: ein
    /// Rechteck, das 1 px schmaler waere, faellt noch als Seitenflaeche
    /// durch — das ist Absicht, weil die Zeichnung runden darf. 1 px sind
    /// keine sichtbare Kante.
    fn ist_seitenflaeche(&self, item: &DisplayItem, breite: u32) -> bool {
        let DisplayItem::Rect { rect, .. } = item else {
            return false;
        };
        let sicht = self.sicht_hoehe() as f32;
        //
        // D9 (01.10.2026): DIE HOEHE IST NUR EINE UNTERE GRENZE.
        //
        // GEMESSENER FEHLER, den die Koordination im Bild gemeldet hat
        // (team/log/devtools_D9_wikipedia.png, 22:2x): „der Seitenbereich
        // oben ist LEER/dunkel". GEMESSENE URSACHE: `narsil_chrome` bekommt
        // von hier `self.hoehe` (die VOLLE Fensterhoehe, 900) und zeichnet
        // seinen Seitenhintergrund in der Hoehe 900 - 58 - 24 = 818 px.
        // `sicht_hoehe()` ist mit offenen Werkzeugen aber nur noch
        // 900 - 82 - 450 = 368 px. Der Vergleich `(rect.height - sicht) <
        // 0.5` schlug also fehl, der Seitenhintergrund wurde NICHT
        // uebersprungen — und die dunkle Flaeche der Leiste lag ueber der
        // ganzen Seite. Das ist die Fehlerklasse „zwei Zahlen fuer
        // dieselbe Frage", und die zweite Zahl kam aus einer anderen
        // Crate.
        //
        // GEMESSENE ENTSCHEIDUNG: erkannt wird der Seitenhintergrund an
        // Position und BREITE, und die Hoehe muss nur GROSS GENUG sein,
        // um den Seitenbereich zu unterdecken. Der Leisten-Hintergrund
        // der Crate liegt ueber der Werkzeugflaeche sowieso nicht (er
        // endet an der Seite), und der Werkzeugbereich wird danach
        // gezeichnet.
        rect.x <= 0.5
            && rect.width >= breite.saturating_sub(1) as f32
            && (rect.y - self.chrome.seiten_oberkante() as f32).abs() < 0.5
            && rect.height >= sicht - 0.5
    }

    pub fn sicht_hoehe(&self) -> u32 {
        //
        // UI1 (03.10. 10:45): DIE STATUSZEILE GEHOERT NICHT ZUR SEITE.
        //
        // GEMESSENE BEGRUENDUNG: `sicht_hoehe` sagt, wie viele Zeilen
        // Pixel die Seite benutzen darf. Ohne die Statuszeile wuerde die
        // Seite 24 px UNDER ihre Zeile gehen und der unterste Absatz
        // waere bei jedem Bild halb verdeckt — und weil die Seite genau
        // bis zum Rand des Sichtbereichs gebaut wird, sieht man den
        // Fehler erst, wenn man ans Ende scrollt.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Zeile hier abgezogen wird und
        // nicht beim Zeichnen: der Sichtbereich ist die EINE Zahl, die
        // Layout, Scroll-Grenze und Bildaufbau gemeinsam benutzen. Ein
        // Abzug an drei Stellen waeren drei Zahlen, die auseinanderlaufen
        // koennen — und die dritte faellt erst auf, wenn die Seite
        // scrollt.
        //
        // D9 (01.10.2026): DER WERKZEUGBEREICH STEHT UNTEN.
        //
        // GEMESSENE ENTSCHEIDUNG, warum HIER und nicht an drei Stellen:
        // diese Funktion ist die EINZIGE Quelle fuer „wie viele Pixel hat
        // die Seite" — sie geht an den Umbruch (`let viewport = Size::new(
        // breite, sicht_hoehe)`, Z. 2222), ans Scrollen (`max_versatz`),
        // ans Bild (`ist_seitenflaeche`) und ans Cursorkasten. Ein Abzug
        // an mehreren Stellen waere die Fehlerklasse „zwei Zahlen fuer
        // dieselbe Frage" — genau die, an der der Abzug der Statuszeile
        // (UI1, 03.10. 10:45) dreimal stand.
        //
        // GEMESSENE BEGRUENDUNG fuer `saturating_sub` statt `-`: im 400-px-
        // Fenster (Hoehe 400 - 58 - 24 = 318) minus Werkzeug (200) sind
        // 118 Pixel uebrig; im 200-px-Fenster waeren es negative, und ein
        // `u32`-Unterlauf wuerde im Debug-Bau PANICKEN.
        self.hoehe
            // Release 08.10.2026: die Statusleiste mit DEM Massstab der
            // Leisten (`statusleiste().hoehe`) — mit der unskalierten Zahl
            // passte die Seitenflaeche der Leiste bei 150 % nicht mehr zum
            // Sichtbereich, wurde nicht erkannt und verdeckte die Seite.
            .saturating_sub(self.chrome.seiten_oberkante() + self.chrome.statusleiste(self.hoehe).hoehe)
            .saturating_sub(crate::devtools_bruecke::hoehe(self))
            .max(1)
    }

    /// Fenster wurde groesser oder kleiner. Der neue Umbruch wird vorgemerkt.
    pub fn resize(&mut self, breite: u32, hoehe: u32) {
        // Auch eine reine DPI-Aenderung kann bei gleicher Clientgroesse
        // die Position des Toolbarankers aendern.
        if self.leisten_menue.is_some() {
            self.chrome.setze_fensterbreite(breite);
            self.leisten_menue = self.chrome.menue_sichtbar().map(|m| crate::leiste_klick::LeistenMenue {
                x: m.x as f32, y: m.y as f32,
                skala_prozent: narsil_chrome::glas::prozent_von(&self.chrome),
            });
        }
        if breite == self.breite && hoehe == self.hoehe {
            return;
        }
        self.breite = breite;
        self.hoehe = hoehe;
        self.chrome.setze_fensterbreite(breite);
        self.menue = None;
        self.tabmenue = None;
        // D9: die Werkzeuge kennen ihre eigene Fenstergroesse. Ohne das
        // laege der Bereich nach dem Wechsel an der ALTEN Stelle - und
        // der Nutzer kaeme an nichts mehr.
        crate::devtools_bruecke::bei_fensterwechsel(self);
        self.schmutzig = true;
        //
        // UMBau-ENTPRELLUNG (06.10.2026, Nutzerbefund).
        //
        // GEMESSENER FEHLER: hier stand `let _ = self.neu_laden();`. Der
        // Lader ist SYNCHRON und holt die Seite neu (Wikipedia 13,8 s),
        // und WM_SIZE kommt beim Ziehen der Kante dutzendfach — also
        // dutzende volle Ladungen. In der Zeit nahm das Fenster keine
        // Nachricht mehr an, und Minimieren/Maximieren fühlte sich
        // kaputt an.
        //
        // GEMESSENE ENTSCHEIDUNG: die GROESSE gilt sofort (Leiste,
        // Geometrie, Zeichnen), der SEITEN-Umbau wartet `UMBAU_RUHE` —
        // `tick` fuehrt ihn aus, wenn keine neue Groesse mehr kam. Eine
        // gezogene Kante ergibt damit einen Umbau ohne neue Seitenladung.
        if self.angezeigt.is_some() {
            self.umbau_seit = Some(std::time::Instant::now());
        }
    }

    // ------------------------------------------------------------------
    // D20 (06.10., NARSIL): DIE WERKZEUG-ZUGAENGE VON AUSSEN
    // ------------------------------------------------------------------
    //
    // GEMESSENE ENTSCHEIDUNG, warum diese sieben Methoden ueberhaupt da sind:
    // `crates/narsil/tests/d20_werkzeug_hooks.rs` (Freigabe Regel 15 vom
    // 04.10.) beweist den **Weg** vom Klick zum Zustand. GEMESSEN ist, dass
    // dieser Weg ueber `testmodus::linksklick` -> `Sitzung::ereignis` ->
    // `mausklick` -> `devtools_bruecke::mausklick_zuerst` laeuft — der Weg ist
    // die Behauptung. Fuer den **Anfangszustand** (Werkzeuge offen, dieser
    // Reiter, diese Spalte, diese Konsole, dieser Knoten) gibt es drei
    // Wege: drei Tastendruecke simulieren, oder **eine** Methode, die den
    // Zustand setzt. Jede Methode unten ruft **denselben** Aufruf, den auch
    // der Klick ruft — das ist der ganze Testwert. Eine Methode, die etwas
    // anderes tut als der Klick, wuerde den Weg nicht beweisen, sondern eine
    // zweite Wahrheit danebenstellen.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Logik in `devtools_bruecke.rs`
    // bleibt und hier nur **ein Aufruf** steht: die Regeln „was macht ein
    // Klick im Reiter" gehoeren an die Bruecke. Hier steht nur, dass die
    // Sitzung den Zustand von aussen erreichbar macht.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Bruecke fuer den Anfangszustand
    // ueberhaupt keine eigene Funktion bekommen hat (der Test rief
    // `s.setze_reiter(..)`, nicht `devtools_bruecke::setze_reiter(&mut s, ..)`):
    // `devtools_bruecke::setze_reiter` EXISTIERT seit dem 04.10. und hat
    // null Aufrufer in `crates/narsil/tests` (GEMESSEN: grep = 1, die
    // Definition). Der Test hat den Aufruf als Methode geschrieben, also
    // standen zwei Namen fuer eine Sache. Jetzt gibt es **eine** Methode,
    // und sie ruft die vorhandene Funktion — der Aufrufer bleibt der
    // einzige Weg.

    /// DER WERKZEUGZUSTAND — nur zum **Lesen**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es ein `&Devtools` und kein `&mut` gibt:
    /// die Tests wollen den Zustand **nach** einem Klick vergleichen
    /// (`konsole_reihen(&s.devtools(), panel)` z. B.). Ein `&mut` wuerde
    /// behaupten, der Zustand solle sich aendern — und die Abfrage aendert
    /// nichts.
    pub fn devtools(&self) -> &narsil_devtools::Devtools {
        &self.devtools
    }

    /// DIE WERKZEUGE **auf** oder **zu** — nicht schalten.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `aus_menue` und nicht
    /// `devtools_bruecke::taste(Taste::F12)`: F12 **schaltet**. Ein
    /// `setze_werkzeuge(true)`, das bei schon offenen Werkzeugen schliesst,
    /// waere eine Zusage, die das Wort „setze" sofort wieder aufhebt. Der
    /// Weg des Menuepunkts „Untersuchen" ist genau „auf, nicht umschalten"
    /// (siehe dort) — er macht `neu_legen` **und** `an_seite`, also bekommen
    /// die Werkzeuge die Seite, wie beim Oeffnen im Fenster.
    ///
    /// `false` ist derselbe Weg wie `Devtools::schliessen` plus `neu_legen`:
    /// die Geometrie muss auch beim Schliessen neu gerechnet werden, sonst
    /// waere der Bereich nach dem naechsten Ereignis an der alten Stelle.
    pub fn setze_werkzeuge(&mut self, an: bool) {
        if an {
            crate::devtools_bruecke::aus_menue(self);
            return;
        }
        self.devtools.schliessen();
        crate::devtools_bruecke::neu_legen(self);
        self.schmutzig = true;
    }

    /// DER AKTIVE REITER — dieselbe Funktion, die der Klick im Reiterband
    /// am Ende aufruft.
    pub fn setze_reiter(&mut self, r: narsil_devtools::d1_geruest::Reiter) {
        crate::devtools_bruecke::setze_reiter(self, r);
    }

    /// DIE SPALTE IM ELEMENTE-REITER (Regeln / Berechnet / Layout).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `spalte_setzen` und nicht ein
    /// `Wirkung::ElementeSpalte`: GEMESSEN ist, dass die Leiste diesen
    /// Zustand ueber genau diesen Aufruf aendert
    /// (`narsil_devtools::werkzeug::wirke`, Z. 560:
    /// `Wirkung::ElementeSpalte(s) => d.elemente.spalte_setzen(s)`). Ein
    /// zweiter Weg waere eine zweite Wahrheit darueber, was der Klick tut.
    ///
    /// Liefert, ob sich **etwas** geaendert hat — dasselbe wie beim Klick
    /// (`spalte_setzen` gibt bei gleicher Spalte `false`).
    pub fn setze_spalte(&mut self, s: narsil_devtools::werkzeug::Spalte) -> bool {
        let geaendert = self.devtools.elemente.spalte_setzen(s);
        if geaendert {
            self.schmutzig = true;
        }
        geaendert
    }

    /// DIE KONSOLENAUSGABEN ERSETZEN (der Zustand, den die Seite liefert).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **ersetzen** und nicht anhaengen: der
    /// Aufrufer baut die Konsole selbst (`Konsole::fuehre_aus`), und er
    /// will genau **diese** Zeilen im Reiter sehen — nicht die der Seite
    /// plus seine. Und es ist derselbe Zustand, den `an_seite` uebernimmt
    /// (`konsole.seite_uebernehmen`): ein Weg, ein Feld.
    pub fn devtools_setze_konsole(&mut self, k: narsil_devtools::d3_konsole::Konsole) {
        self.devtools.konsole = k;
        self.schmutzig = true;
    }

    /// EINEN KNOTEN IM ELEMENTE-REITER WAEHLEN — wie ein Klick auf seine
    /// Baumzeile.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `Option<NodeId>` und nicht `NodeId`:
    /// die Frage „welcher Knoten ist das?" hat ohne Seite keine Antwort, und
    /// `NodeId(0)` als Antwort „gibt es nicht" waere eine Zahl, die auf das
    /// `<html>` zeigt. Also sagt die Signatur selbst, dass die Seite den
    /// Knoten vielleicht nicht hat — und der Aufrufer gibt sein `None`
    /// unveraendert durch.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ueber `css_von_dom` und nicht direkt
    /// mit der DOM-Nummer: GEMESSEN ist, dass die Reiter-Zeilen mit der
    /// **CSS**-Nummer arbeiten (`Zeile::css`, `Elemente::waehle(css: u32)`)
    /// und dass `Seite` zwei Bruecken baut, weil die Nummern verschieden
    /// sind (`Seite::css_zu_dom`, `Seite::dom_zu_ord`,
    /// `narsil-page/src/lib.rs:1131ff`: „CSS-Baum-Index -> DOM-Knoten"). Die
    /// Bruecke `Elemente::css_von_dom` ist genau die Umkehrung davon, und
    /// `inhalt_klick` (der Klick) geht denselben Weg.
    pub fn devtools_setze_knoten(&mut self, knoten: Option<NodeId>) -> bool {
        let Some(n) = knoten else {
            return false;
        };
        let Some(css) = self.devtools.elemente.css_von_dom.get(&n.0).copied() else {
            return false;
        };
        // `waehle` rechnet Stil, Regeln und Barrierefreiheit neu — der
        // Grund, warum der Klick danach den Zustand des gewaehlten Knotens
        // zeigt (siehe `d2_elemente.rs:363`).
        let gewaehlt = self.devtools.elemente.waehle(css);
        if gewaehlt {
            self.schmutzig = true;
        }
        gewaehlt
    }

    /// DER ERSTE KNOTEN DIESER SEITE MIT DIESEM TAG-NAMEN.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Suche im **CSS-Baum** laeuft und
    /// nicht in `Angezeigt::html`: GEMESSEN an der Testseite
    /// (`d20_werkzeug_hooks.rs`, `SEITE`) ist `html.len() == 1` — es ist der
    /// **verschachtelte** Parse-Baum (`narsil_page::html_lesen` =
    /// `parse_html`, ein `Vec` mit der einen `<html>`-Wurzel, alle Kinder in
    /// `children`). Ein Lauf ueber dieses `Vec` sieht also **nur die
    /// Wurzel**. Und ein Lauf mit einem eigenen Zaehler durch die Kinder waere
    /// eine **zweite** Zaehlung der DOM-Nummern — genau die zweite Wahrheit,
    /// die diese Datei an zehn Stellen kommentiert.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der CSS-Baum der richtige Ort ist: er
    /// ist **flach** (GEMESSEN: `css_baum.knoten.len() == 7` fuer dieselbe
    /// Seite: html, head, style, body, p, button, script) und er traegt die
    /// Tag-Namen. Das DOM kann sie nicht: GEMESSEN ist
    /// `narsil_dom::NodeKind` = `Page | Container | Text`
    /// (`narsil-dom/src/lib.rs:242`) — dort steht kein `<p>`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `css_von_dom` **umgekehrt** wird und
    /// nicht `Seite::css_zu_dom`: GEMESSEN ist, dass `Seite::css_zu_dom` auf
    /// die **Vorlauf-Nummer** zeigt, nicht auf den DOM-Knoten
    /// (`narsil-page/src/lib.rs:1131`: „Der Wert ist eine Vorlauf-Nummer"), und
    /// dass `d2_elemente::bruecke_bauen` (Z. 1144) daraus erst die
    /// DOM-Nummer macht. `Elemente::css_von_dom` ist genau dieses Ergebnis
    /// und ist `pub` — es ist die **eine** fertige Bruecke, also wird sie
    /// benutzt und nicht neu gebaut.
    pub fn knoten_mit_tag(&self, tag: &str) -> Option<NodeId> {
        let a = self.angezeigt.as_ref()?;
        let css = a
            .seite
            .css_baum
            .knoten
            .iter()
            .position(|k| k.tag.eq_ignore_ascii_case(tag))? as u32;
        self.devtools
            .elemente
            .css_von_dom
            .iter()
            .find(|(_, c)| **c == css)
            .map(|(dom, _)| NodeId(*dom))
    }

    // ------------------------------------------------------------------
    // Eingabe
    // ------------------------------------------------------------------

    /// Ein Ereignis vom Fenster annehmen.
    ///
    /// Der EINZIGE Weg, auf dem sich die Sitzung aendert. Alles andere
    /// sind Lesezugriffe — so kann kein Zustand entstehen, den kein
    /// Ereignis erklaeren kann.
    pub fn ereignis(&mut self, e: &SurfaceEvent) {
        // Release 10.10.2026: die letzte NUTZERAKTION (HTML §6.4.3 „transient
        // activation") — `input.click()` auf einem Dateifeld oeffnet den
        // Dialog nur kurz danach, sonst koennte jede Seite ungefragt Dialoge
        // oeffnen.
        if matches!(e, SurfaceEvent::ButtonPressed { .. } | SurfaceEvent::Key(_)) {
            self.letzte_aktivierung = Some(std::time::Instant::now());
        }
        //
        // 02.10. 01:55: DAS THEMA DER BEDIENFLAECHE VORAB UEBERNEHMEN (die
        // Begründung steht an `thema_uebernehmen`). GEMESSENE EINORDNUNG,
        // warum es hier und nicht in `display_list`: das nimmt `&self` (es
        // darf die Seite nicht bauen), und der Neuaufbau gehoert an eine
        // Stelle mit `&mut self`.
        //
        // GEMESSEN, dass es nichts kostet: die Methode vergleicht zwei
        // Zwei-Werte-Enums und gibt `false` zurueck, wenn nichts sich
        // geaendert hat — auf jedem Ereignis, auch auf 60 Mausbewegungen
        // je Sekunde.
        self.thema_uebernehmen();
        match e {
            SurfaceEvent::CloseRequested => self.meldung = Some(tr("Window closed").into()),
            SurfaceEvent::Resized(size) => {
                // GEMESSEN: X11 meldet 0,0 beim Minimieren. Eine Breite
                // von 0 ist ein gueltiger Zustand (Backlog 0051), und der
                // darf nicht in einer Division landen.
                let b = if size.width.is_finite() && size.width > 0.0 {
                    size.width as u32
                } else {
                    self.breite
                };
                let h = if size.height.is_finite() && size.height > 0.0 {
                    size.height as u32
                } else {
                    self.hoehe
                };
                self.resize(b, h);
            }
            //
            // KONTEXT (03.10.2026, „Optik wie Firefox“): DIE MAUS BEWEGT SICH.
            //
            // GEMESSENER BEFUND: `PointerMoved` war **nicht** behandelt
            // (`grep -n "PointerMoved" crates/narsil/src/sitzung.rs` = 0
            // Treffer vor dieser Aenderung). Also stand das Menue in **einer**
            // Farbe, und der Nutzer konnte nicht sehen, welche Zeile der Klick
            // treffen wuerde. Firefox faerbt die Zeile unter dem Mauszeiger.
            //
            // GEMESSENE ENTSCHEIDUNG, warum **nur bei Aenderung** gezeichnet
            // wird: `Menue::setze_zeiger` liefert `true`, wenn eine Zeile neu
            // getroffen ist. Ohne diese Bedingung wuerde **jede** Mausbewegung
            // ein Bild zeichnen lassen — und `narsil_raster` braucht GEMESSEN
            // 106 ms fuer 800x600 (ADR-034, Kopf `main.rs`). Also:
            // `MotionNotify` ist eine **Aenderung**, `Expose` ist keiner.
            SurfaceEvent::PointerMoved(p) => {
                // KONTEXT (06.10.2026, 03:22): DER AUTO-LAUF braucht die
                // **Zeigerposition**, und es gab bis hierher kein Feld dafuer
                // (GEMESSEN: `grep "self.zeiger"` = 0 Treffer). GEMESSENE
                // ENTSCHEIDUNG, warum das hier **ganz oben** steht: der
                // Zeigerstand ist eine **Tatsache**, die kein Zweig aendert —
                // und weiter unten waere sie in einem Zweig gelandet, den
                // jemand spaeter als „den Hover-Zweig“ umgebaut haette.
                self.maus = *p;
                //
                // KONTEXT (04.10.2026, 16:30): DAS ZIEHEN DES BALKENS. GEMESSENE
                // ENTSCHEIDUNG, warum es **vor** allem anderen steht: solange
                // gezogen wird, ist die Maus am **Balken** und nicht an der
                // Seite — ein Hover der Zeile waere eine Luege, solange der
                // Nutzer den Griff haelt.
                //
                // KONTEXT (04.10.2026, 21:40): DIE AUSWAEHL **VERLAENGERN**.
                // GEMESSENE ENTSCHEIDUNG, warum **nach** dem Scrollbalken und
                // nicht davor: der Balken liegt am Rand und ist ein eigener
                // Bereich — wer auf ihn zieht, zieht den Balken, nicht Text.
                //
                // KONTEXT (04.10.2026, 23:58): DER **TAB** FOLGT DEM ZEIGER.
                //
                // 06.10.2026: EINE WAHRHEIT — der Zug steht in `Chrome::zug`
                // (`zug_bewegen` rechnet die Stelle aus der Zeigerposition),
                // und umgestellt wird erst beim LOSLASSEN. Vorher stand hier
                // ein zweiter Zustand (`tab_zieht`), der `TabUmstellen` bei
                // JEDER Bewegung ausfuehrte — und `self.tabs` (die Seiten)
                // blieb dabei stehen.
                if self.chrome.zug.is_some() {
                    let x = p.x as i64;
                    if let Some(z) = self.chrome.zug_bewegen(x) {
                        let _ = z;
                        self.schmutzig = true;
                    }
                }
                // DER LEISTEN-HOVER AUS DER BEWEGUNG.
                //
                // GEMESSEN (06.10.2026, echte Fensteraufnahme): `hover_setzen`
                // hatte im Produktivpfad **keinen** Aufrufer — kein Knopf
                // leuchtete auf, das X eines inaktiven Tabs blieb unsichtbar,
                // und die Leiste wirkte „haengend". Waehrend eines Tabzugs
                // bleibt der Hover weg (die Maus haelt den Tab).
                // Release 08.10.2026: Hover der Fensterknoepfe und der Tabs.
                {
                    let vorher = (self.chrome.fenster_hover, self.chrome.tab_hover);
                    self.chrome.fenster_hover = self.chrome.fenster_knopf_unter(p);
                    self.chrome.tab_hover = if p.y >= 0.0 && p.y < self.chrome.titel_hoehe() as f32 {
                        self.chrome.tab_unter(p)
                    } else {
                        None
                    };
                    if self.chrome.fenster_gedrueckt.is_some() && self.chrome.fenster_hover != self.chrome.fenster_gedrueckt {
                        self.chrome.fenster_gedrueckt = None;
                        self.schmutzig = true;
                    }
                    if (self.chrome.fenster_hover, self.chrome.tab_hover) != vorher {
                        self.schmutzig = true;
                    }
                }
                if self.chrome.zug.is_none() {
                    let vorher = (self.chrome.hover, self.chrome.schliessen_hover);
                    if p.y < self.chrome.seiten_oberkante() as f32 {
                        crate::leiste_klick::hover_setzen(&mut self.chrome, p);
                    } else {
                        self.chrome.hover_weg();
                    }
                    if (self.chrome.hover, self.chrome.schliessen_hover) != vorher {
                        self.schmutzig = true;
                    }
                }
                //
                // KONTEXT (05.10.2026, 03:42): DAS ZIEHEN IM FELD.
                //
                // GEMESSENE ENTSCHEIDUNG, warum **vor** der Auswahl auf der
                // Seite: ein Feld und ein Absatz sind nicht dieselbe Frage, und
                // `feld_zieht` ist nur dann wahr, wenn der Druck **im Feld**
                // war — wer im Feld zieht, markiert dort und nicht auf der Seite.
                if self.feld_zieht && self.feld_bei_punkt(*p) {
                    self.schmutzig = true;
                    return;
                }
                if self.auswahl.zieht && self.scrollbalken.zieht.is_none() {
                    if self.auswahl_bewegen(*p) {
                        self.schmutzig = true;
                        return;
                    }
                }
                if self.scrollbalken.zieht.is_some() {
                    let sicht = self.sicht_hoehe();
                    let (inhalt, versatz) = self
                        .angezeigt
                        .as_ref()
                        .map_or((0, 0), |a| (a.inhalt_hoehe, a.versatz));
                    if let Some(spur) = self.scrollspur() {
                        if let Some(neu) = self.scrollbalken.ziehen_zu(&spur, sicht, inhalt, *p) {
                            //
                            // KONTEXT (04.10.2026, 18:45): DAS ZIEHEN **BRICHT**
                            // die sanfte Bewegung ab. GEMESSENE ENTSCHEIDUNG,
                            // warum: unter dem Zeiger darf **nichts** laufen —
                            // ein Balken, der dem Zeiger nachläuft, ist genau
                            // das „hüpfende“ Scrollen, das die Leitung nicht
                            // wollte.
                            self.scroll_ani = None;
                            if let Some(a) = self.angezeigt.as_mut() {
                                a.versatz = neu;
                            }
                            self.bilder_sichtbar_laden();
                            self.scrollbalken.zeigen(std::time::Instant::now());
                            self.schmutzig = true;
                            return;
                        }
                    }
                }
                //
                // Release 09.10.2026: das Leistenmenue bekommt die Hover-Farbe
                // wie die beiden anderen Menues.
                if let Some(m) = self.leisten_menue.as_ref() {
                    let neu = m.treffer(self.breite, self.hoehe, p);
                    if neu != self.leisten_zeiger {
                        self.leisten_zeiger = neu;
                        self.schmutzig = true;
                    }
                } else {
                    self.leisten_zeiger = None;
                }
                // KONTEXT (04.10.2026, 06:40): auch das **Tabmenue** bekommt
                // die Hover-Farbe — dieselbe Frage, dieselbe Wirkung.
                if let Some(m) = self.tabmenue.as_mut() {
                    if m.setze_zeiger(self.breite, self.hoehe, p) {
                        self.schmutzig = true;
                        return;
                    }
                }
                // Release 08.10.2026 (Feature 12): die Link-Ziel-Vorschau.
                if self.features_zeiger(*p) {
                    self.schmutzig = true;
                }
                if let Some(m) = self.menue.as_mut() {
                    // GEMESSENE ENTSCHEIDUNG, warum **nur** der Zeiger gesetzt
                    // wird und nicht auch das Untermenue aufgeklappt wird:
                    // Firefox oeffnet es beim **Darueberfahren mit einer
                    // Verzoegerung** (250 ms). Ohne diese Zeit oeffnet es sich
                    // beim Vorbeiwischen — und ein Untermenue, das beim
                    // Vorbeifahren aufpoppt, ist schlimmer als keines. Der
                    // **Klick** auf die Zeile oeffnet es (siehe `menue_klick`),
                    // und das ist messbar und ohne Timer.
                    if m.ktx.setze_zeiger(self.breite, self.hoehe, p) {
                        self.schmutzig = true;
                    }
                }
            }
            SurfaceEvent::Wheel { point, delta } => {
                // Release 09.10.2026: nur ueber dem Download-Panel faengt es das
                // Rad ab — daneben rollt die Seite.
                if self.downloads_offen {
                    let anzahl = self.downloads_seite.sichtbare_zeilen().len();
                    if narsil_chrome::downloads_panel::geometrie(&self.chrome, self.breite, self.hoehe, anzahl)
                        .is_some_and(|g| narsil_chrome::downloads_panel::im_panel(&g, point))
                    {
                        return;
                    }
                }
                // Release 09.10.2026: auf den Einstellungen rollt das Rad die
                // Inhaltsspalte (vorher: nichts — die unteren Features waren
                // bei 800 px Fensterhoehe unerreichbar).
                if point.y >= self.chrome.seiten_oberkante() as f32
                    && crate::einstellungen_weg::ist_einstellungen(self.browser.adresse())
                {
                    let px = self.rad_als_pixels(*delta) as f32;
                    if narsil_chrome::einstellungen::rollen(&mut self.einstell_seite, &self.chrome, self.hoehe, px) {
                        self.schmutzig = true;
                    }
                    return;
                }
                //
                // KONTEXT (05.10.2026, 15:20, Leitung „Scroll-Ketten-Bewegung“):
                // DAS RAD BEI EINEM **PUNKT**.
                //
                // GEMESSENE ENTSCHEIDUNG, warum die Kette **vor** `scrollen`:
                // GEMESSEN ist, dass `scrollen` immer die **Seite** bewegt — ein
                // Rad **auf** einem inneren Kasten wuerde die Seite schieben und
                // den Kasten stehen lassen. `scroll_kette_bei_punkt` gibt `false`
                // zurueck, wenn der Punkt **nicht** in einem Kasten liegt — dann
                // ist die Seite dran, wie bisher.
                //
                // GEMESSENE ENTSCHEIDUNG, warum die Bildschirmkoordinate hier
                // gerechnet wird: GEMESSEN ist (Z. 2111), dass die Seite mit
                // `y_seite = punkt.y - SEITEN_START_Y` rechnet und dann den
                // **Versatz** dazuzählt, wenn sie im Layoutbaum sucht — dieselbe
                // Rechnung, damit der Zeiger und die Kaesten dasselbe meinen.
                let y_seite = point.y - self.chrome.seiten_oberkante() as f32;
                if !self.scroll_kette_bei_punkt(point.x, y_seite, self.rad_als_pixels(*delta)) {
                    self.scrollen(*delta);
                }
                self.schmutzig = true;
            }
            //
            // KONTEXT (04.10.2026, 16:40): DER **MAUSKNOPF LOS** beendet das
            // Ziehen. GEMESSENER BEFUND: `SurfaceEvent::ButtonReleased` war vor
            // dieser Zeile **nirgends** behandelt (GEMESSEN mit
            // `grep -n "SurfaceEvent::ButtonReleased" crates/narsil/src/sitzung.rs`
            // = 0 Treffer) — der Arm fehlte also nicht nur fuer den Balken,
            // sondern **fuer alles**, was ein Gedrueckt-Halten ueberlebt
            // (Bild aus einem Tab ziehen, Spaltenbreite der Leiste).
            SurfaceEvent::ButtonReleased { point, .. } => {
                // Release 08.10.2026: ein Fensterknopf wirkt beim LOSLASSEN
                // ueber demselben Knopf (wie jeder Knopf in Windows).
                if let Some(k) = self.chrome.fenster_gedrueckt.take() {
                    if self.chrome.fenster_knopf_unter(point) == Some(k) {
                        self.fenster_befehl = Some(k);
                    }
                    self.schmutzig = true;
                    return;
                }
                if self.scrollbalken.zieht.is_some() {
                    self.scrollbalken.ziehen_ende();
                    self.schmutzig = true;
                }
                //
                // KONTEXT (04.10.2026, 21:45): DAS ZIEHEN DER AUSWAHL ENDET
                // HIER — und **nur** hier. GEMESSENE ENTSCHEIDUNG, warum nicht
                // beim Bewegen: `PointerMoved` kommt auch **ohne** gedrueckte
                // Taste (der Zeiger wandert ueber die Seite), und dann waere die
                // Auswahl beim blossen Vorbeifahren schon zu Ende.
                if self.auswahl.zieht {
                    self.auswahl.zieht = false;
                    self.schmutzig = true;
                }
                //
                // KONTEXT (05.10.2026, 03:44): DAS ZIEHEN IM FELD ENDET HIER —
                // aus demselben Grund wie bei der Auswahl (siehe dort).
                if self.feld_zieht {
                    self.feld_zieht = false;
                    self.schmutzig = true;
                }
                //
                // KONTEXT (04.10.2026, 23:59): DAS TABZIEHEN ENDET HIER.
                // 06.10.2026: Das Umstellen selbst passiert JETZT (beim
                // Loslassen), nicht bei jeder Bewegung — vorher lief
                // `TabUmstellen` dutzendfach je Zug, und die Seitenliste
                // blieb stehen.
                if let Some(z) = self.chrome.zug_beenden() {
                    if z.ziel != z.nr {
                        self.tabs_umstellen(z.nr as u32, z.ziel as u32);
                    } else {
                        self.schmutzig = true;
                    }
                }
            }
            SurfaceEvent::Key(k) if k.pressed => {
                // Die Downloadliste liegt ueber der Seite; ihre Suche darf
                // keine Tasten an Formulare oder den Seitenscroll weitergeben.
                if self.downloads_offen && !self.leiste_im_fokus && !k.modifiers.ctrl && !k.modifiers.alt {
                    match k.key {
                        narsil_input::Key::Escape => { self.downloads_offen = false; self.downloads_suche_fokus = false; }
                        narsil_input::Key::Char(c) if self.downloads_suche_fokus => {
                            let mut text = self.downloads_seite.suchtext.clone();
                            text.push(c);
                            self.downloads_seite.suchen(&text);
                        }
                        narsil_input::Key::Backspace if self.downloads_suche_fokus => {
                            let mut text = self.downloads_seite.suchtext.clone();
                            text.pop();
                            self.downloads_seite.suchen(&text);
                        }
                        _ => {}
                    }
                    self.schmutzig = true;
                    return;
                }
                //
                // KONTEXT (04.10.2026, 22:05): HIER ist die **einzige** Stelle,
                // an der GEMESSEN ist, dass die Modifier ankommen — also werden
                // sie hier gemerkt, damit der **naechste** Mausklick weiss, ob
                // Umschalt gedrueckt war.
                self.umschalt = k.modifiers.shift;
                // UI4 (03.10. 09:35): ESC SCHLIESST DAS MENUE.
                //
                // GEMESSENE BEGRUENDUNG: `taste()` hat am Anfang ZWEIERLEI
                // (Strg+L, Zoom, Tab) und danach die Navigation. Ohne
                // diesen Vorab-Schritt wuerde ESC in den Zweig fuer
                // „Auswahl aufheben" oder „Pos1" laufen — und der
                // Nutzer, der nur das Menue loswerden wollte, saehe die
                // Seite springen.
                if k.key == narsil_input::Key::Escape && crate::einstellungen_weg::ist_einstellungen(self.browser.adresse()) {
                    let offen = self.einstell_seite.offen.take().is_some();
                    let tippen = self.einstell_seite.tippfeld.take().is_some();
                    if offen || tippen {
                        self.schmutzig = true;
                        return;
                    }
                }
                // Release 09.10.2026: in ein Textfeld der Einstellungen tippen
                // (Downloads-Ordner, eigene Such-URL, eigene Startseite).
                // GEMESSEN vorher: die Tasten gingen in die Adressleiste.
                if !self.leiste_im_fokus
                    && !k.modifiers.ctrl
                    && !k.modifiers.alt
                    && self.einstell_seite.tippfeld.is_some()
                    && crate::einstellungen_weg::ist_einstellungen(self.browser.adresse())
                {
                    use narsil_chrome::einstellungen::Tippen;
                    let t = match k.key {
                        narsil_input::Key::Char(c) => Some(Tippen::Zeichen(c)),
                        narsil_input::Key::Space => Some(Tippen::Zeichen(' ')),
                        narsil_input::Key::Backspace => Some(Tippen::Zurueck),
                        narsil_input::Key::Enter => Some(Tippen::Fertig),
                        _ => None,
                    };
                    if let Some(t) = t {
                        self.einstell_tippen(t);
                        return;
                    }
                }
                if (self.menue.is_some() || self.leisten_menue.is_some()) && k.key == narsil_input::Key::Escape {
                    self.menue = None;
                    self.leisten_menue = None;
                    self.schmutzig = true;
                    return;
                }
                // 06.10.2026: ESC BRICHT EINEN TABZUG AB — ohne Umstellen
                // (Firefox). Ohne diesen Zweig blieb `Chrome::zug` stehen,
                // und der Zug ging beim naechsten Loslassen doch durch.
                if k.key == narsil_input::Key::Escape && self.chrome.zug.is_some() {
                    self.chrome.zug_abbruch();
                    self.schmutzig = true;
                    return;
                }
                self.taste(*k)
            }
            SurfaceEvent::ButtonPressed { point, button } => {
                self.klick = Some(*point);
                if *button == narsil_window::MouseButton::Left
                    && let Some(k) = self.chrome.fenster_knopf_unter(point)
                {
                    self.chrome.fenster_gedrueckt = Some(k);
                    self.schmutzig = true;
                    return;
                }
                if *button == narsil_window::MouseButton::Left {
                    self.mausklick(*point);
                } else if *button == narsil_window::MouseButton::Middle {
                    // W7C A1: DIE MITTELTASTE (23:55).
                    self.mittelklick(*point);
                } else if *button == narsil_window::MouseButton::Right {
                    // UI4 (03.10. 09:20): DIE RECHTE TASTE oeffnet das
                    // Menue.
                    self.rechtsklick(*point);
                }
            }
            _ => {}
        }
    }

    /// UI4: DAS RECHTSKLICK-MENUE OEFFNEN.
    ///
    /// GEMESSENE BEGRUENDUNG, warum das Menue IMMER aufgeht (auch ohne
    /// Link darunter): der Nutzer will sehen, was er bekommen kann. Ein
    /// Menue, das nur bei einem Link erscheint, ist eine Belohnung fuer
    /// eine Ortsangabe — und der Rechtsklick ist die Frage „was geht hier?",
    /// nicht die Angabe „wo".
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Link hier AUFGELOEST wird (und
    /// nicht spaeter beim Klick): der `__id`-Abgleich der Bruecke und
    /// `link_unter` des Layouts sind zwei verschiedene Fragen, und beide
    /// gehoeren an denselben Moment — den Klick. Wer sie aufteilt, hat
    /// eine halbe Antwort und eine Zeit lang.
    fn rechtsklick(&mut self, punkt: narsil_core::geometry::Point) {
        //
        // KONTEXT (04.10.2026, 06:30, Leitung „(5) Tab-Rechtsklick“): DER
        // KLICK AUF EINEN TAB GEHOERT DEM TAB, NICHT DER SEITE.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Frage **vor** allem anderen steht:
        // die Tableiste liegt **ueber** der Seite, und ein Klick bei y = 20
        // ist beides — ohne diese Frage wuerde der Nutzer auf einem Tab das
        // **Seiten**menue bekommen.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Tabrechtecke aus `Chrome` kommen
        // und nicht gerechnet werden: `Chrome::tab(nr)` ist die **eine** Stelle,
        // die weiss, wo ein Tab steht — die Zeichnung (`lib.rs:3632`) liest
        // dieselbe Funktion. Wer hier rechnete, haette zwei Zahlen fuer
        // dieselbe Sache.
        //
        // KONTEXT (04.10.2026, 08:55): DIE TABRECHTECKE **SELBST** HOLEN.
        // GEMESSENER BEFUND, und der ist ein toter Fremdcode: `tabs_anzeigen`
        // (`sitzung.rs:5268`, W7C A5) hat **null** Aufrufer — GEMESSEN mit
        // `grep -rn "tabs_anzeigen" crates/narsil/src/` = **eine** Zeile (die
        // Definition). Also ist `Chrome::tabs` im echten Fenster **leer**, und
        // `Chrome::tab(nr)` gibt `None` — mein Tabmenue waere ohne diesen Aufruf
        // **nie** aufgegangen.
        //
        // GEMESSENE ENTSCHEIDUNG, warum ich die **fremde** Funktion aufrufe und
        // sie nicht neu schreibe: sie macht genau diese eine Sache (Titel des
        // aktiven Tabs aus `Angezeigt`, der anderen aus `self.tabs`) und ist
        // seit 01.10. im Baum. Eine zweite Berechnung waere die Fehlerform, die
        // ich in `ktx_menue.rs` selbst dokumentiert habe („zwei Zahlen fuer
        // dieselbe Sache“).
        //
        // GEMESSENE EINSCHRAENKUNG, die ehrlich benannt wird: die **Leiste**
        // im Bild bekommt ihre Tabs erst dadurch, dass der Rechtsklick sie
        // fuellt — im normalen Bildlauf (`display_list`) passiert das **nicht**,
        // weil dort nichts `tabs_anzeigen` aufruft. Das ist eine Luecke im
        // Fremdcode (im BRETT 08:55 gemeldet); mein Menue geht ab jetzt
        // trotzdem auf.
        self.tabs_anzeigen();
        for nr in 0..self.browser.tabs() {
            let Some(r) = self.chrome.tab(nr as usize) else {
                continue;
            };
            if punkt.x >= r.x as f32
                && punkt.x < r.rechts() as f32
                && punkt.y >= r.y as f32
                && punkt.y < r.unten() as f32
            {
                self.menue = None;
                //
                // KONTEXT (04.10.2026, 08:05): DIE ZAHLEN UMRECHNEN. GEMESSENER
                // EIGENER FEHLER (gefunden ohne Compiler, durch Lesen der
                // Signatur `oeffne(tab: u32, tab_anzahl: u32, punkt: &Point)` in
                // `ktx_menue.rs:6537`): `for nr in 0..self.browser.tabs()` laesst
                // `nr` ein `usize` — `browser.tabs()` gibt auch `usize` zurueck.
                // Beides wandert in `u32`-Felder. Das ist der Fehler, an dem
                // man ohne Compiler nur durch **Lesen** vorbeikommt.
                //
                // KONTEXT (04.10.2026, 12:15): DER KLICK MACHT DEN TAB ZUM
                // **AKTIVEN** — wie bei Firefox. GEMESSENE ENTSCHEIDUNG,
                // warum das noetig ist: die Zeilen des Tabmenues bekommen
                // `Navigation`-Befehle (`Navigation::TabAnheften` usw.), und
                // die wirken in `narsil-browser` auf den **aktiven** Tab. Ohne
                // diesen Schritt wuerde ein Rechtsklick auf den zweiten Tab
                // das Menue des **ersten** zeigen und bearbeiten — die Zeile
                // waere im Bild sichtbar und ihre Wirkung waegte an einem
                // anderen Tab.
                //
                // GEMESSENE ENTSCHEIDUNG, warum mit `tab_speichern` /
                // `tab_holen` und nicht nur `anwenden`: die Seite des
                // aktiven Tabs wandert mit (`Sitzung::tab_wechseln`, 5570 macht
                // es genauso) — sonst zeigte der Bildschirm die Seite des
                // alten Tabs, waehrend die Leiste den neuen markiert.
                if nr != self.browser.aktiver_tab_nr() {
                    self.tab_speichern();
                    self.browser.anwenden(&Navigation::TabSpringen(nr as u32));
                    self.angezeigt = None;
                    self.tab_holen();
                }
                //
                // KONTEXT (04.10.2026, 12:10): DER ZUSTAND FUER DIE ZEICHEN.
                // GEMESSENE ENTSCHEIDUNG, warum er aus `nr` kommt und nicht
                // aus „dem aktiven“: `nr` **ist** jetzt der aktive Tab (der
                // Schritt oben), und `nr` ist zugleich die Position, an der
                // das Menue aufgeht — eine Zahl statt zwei.
                let zustand = narsil_chrome::ktx_menue::TabZustand {
                    tab_anzahl: self.browser.tabs() as u32,
                    tabs_rechts: self.browser.tabs_rechts_von(nr),
                    wiederherstellbar: self.browser.hat_geschlossenen_tab(),
                    gepinnt: self.browser.tab_gepinnt(nr),
                    lautlos: self.browser.tab_ist_lautlos(nr),
                };
                let mut tm = narsil_chrome::ktx_menue::TabMenue::oeffne(nr as u32, zustand, &punkt);
                // Release 08.10.2026: die Zeilen der Features 3 und 5.
                let f = self.einstellungen.features;
                tm.setze_features(
                    f.an(crate::features::Feature::NetzSandbox).then(|| self.netzregel_aktiver_tab()),
                    f.an(crate::features::Feature::TabEinfrieren),
                );
                tm.setze_skalierung(self.chrome.sk());
                self.tabmenue = Some(tm);
                self.schmutzig = true;
                return;
            }
        }
        self.tabmenue = None;
        let y_seite = punkt.y - self.chrome.seiten_oberkante() as f32;
        let link = self
            .angezeigt
            .as_ref()
            .and_then(|a| link_am_punkt(a, punkt.x, y_seite + a.versatz as f32))
            .unwrap_or_default();
        //
        // KONTEXT (03.10.2026): WAS LIEGT UNTER DEM ZEIGER?
        //
        // GEMESSENE ENTSCHEIDUNG, warum das **hier** entschieden wird und nicht
        // im Menue: das Menue zeichnet, aber es **weiss nicht**, was unter dem
        // Zeiger liegt — das weiss nur die Seite (`narsil_page`). Also bekommt
        // das Menue den **Kontext** als Angabe, statt ihn selbst zu erraten.
        //
        // GEMESSENE ENTSCHEIDUNG fuer die Reihenfolge: erst der **Link**, dann
        // das **Bild** — und das Bild **gewinnt**. Grund: ein `<a>` um ein
        // `<img>` ist **beides**, und wer auf so einem Bild rechtsklickt, will
        // fast immer das **Bild** („Bild speichern“, „Konvertiert
        // herunterladen“), nicht den Link drumherum.
        //
        // GEMESSENE ENTSCHEIDUNG fuer den **Video**-Fall: `narsil-page` legt
        // `<video>` in `seite.bilder` ab (Zeile 5570: `if klein == "img" ||
        // klein == "video"`) und merkt sich das Tag **nicht** — also
        // unterscheidet `narsil` an der **Endung** der Quelle.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der **MIME-Typ hier nicht** gesetzt
        // wird: `Angezeigt::content_type` ist der Typ der **Seite**
        // (`text/html`), nicht der des Bildes. Ein geratener Bildtyp waere eine
        // **dritte** Wahrheit ueber dieselbe Frage — und der Rueckfall ist
        // gemessen und ausreichend: `Menue::ziele` entscheidet aus der Endung
        // (`endung_aus_adresse`), wenn kein MIME dasteht.
        let mut kontext = narsil_chrome::ktx_menue::Kontext::Leer;
        let mut quelle = link.clone();
        //
        // KONTEXT (04.10.2026): `medien_unter` liefert jetzt **drei** Werte —
        // Kontext, Adresse und der `alt`-Text. GEMESSENE ENTSCHEIDUNG, warum
        // der dritte Wert **nicht** ins Menue wandert: das Menue wuerde ihn
        // beim naechsten Rechtsklick auf etwas anderes noch tragen (es wird
        // ja neu gebaut, aber die Zwischenablage der Zeile waere ein Zustand
        // ohne Besitzer).
        self.bild_alt = String::new();
        if let Some(gefunden) = self.medien_unter(punkt.x, y_seite) {
            kontext = gefunden.0;
            quelle = gefunden.1;
            self.bild_alt = gefunden.2;
        } else if !link.is_empty() {
            kontext = narsil_chrome::ktx_menue::Kontext::Link;
        }
        // KONTEXT (03.10.2026, GEMESSUNG): der Rechtsklick kam am 18:0x an und
        // um 19:30 **nicht** — bei demselben Aufruf. Diese Zeile sagt welcher
        // von beiden Faellen es war.
        eprintln!(
            "[KTX] rechtsklick ({},{}) kontext={kontext:?}",
            punkt.x, punkt.y
        );
        let mut ktx = narsil_chrome::ktx_menue::Menue::oeffne(kontext, &quelle, &punkt);
        // Release 08.10.2026: Feature 0 (Konvertieren) und Feature 2
        // (Element beobachten) aus dem Tab „Features".
        ktx.setze_features(
            self.einstellungen.features.an(crate::features::Feature::Konvertieren),
            self.einstellungen.features.an(crate::features::Feature::Beobachter),
        );
        // Release 08.10.2026 (Glas-Stil, HiDPI): das Menue waechst mit der
        // Skalierung der Leisten.
        ktx.setze_skalierung(self.chrome.sk());
        self.menue = Some(RechtsklickMenue {
            x: punkt.x,
            y: punkt.y,
            link,
            ktx,
        });
        self.schmutzig = true;
    }

    /// KONTEXT (03.10.2026): LIEGT UNTER (x, y) EIN BILD ODER EIN VIDEO?
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **hier** steht und nicht in
    /// `narsil-page`: die Bildkaesten stehen im **Layout-Baum** der Sitzung, und
    /// der Baum gehoert `narsil`. GEMESSENE ENTSCHEIDUNG, warum **dieselbe**
    /// Funktion (`bild_kaesten`) und keine zweite: sie ist der Weg, den
    /// `bild_items` beim Zeichnen geht — ein zweiter Weg zu denselben Kaesten
    /// waere die Fehlerklasse „zwei Stellen entscheiden“ (siehe
    /// `ZEILE_MENUE_HOEHE`, UI4 09:25).
    ///
    /// GEMESSENE ENTSCHEIDUNG fuer die Reihenfolge der Schleife: von hinten
    /// nach vorn, weil der spaetere Knoten **oben** liegt — dieselbe Regel wie
    /// `narsil_page::link_unter` (dort steht sie als Kommentar).
    ///
    /// M3: `seite.bilder` ist durch `MAX_BILDER` (128) begrenzt und
    /// `bild_kaesten` liefert hoechstens so viele Kaesten — die Schleife laeuft
    /// also **hoechstens 128 Mal**, auch wenn die Seite 10 000 Bildknoten hat.
    fn medien_unter(
        &self,
        x: f32,
        y_seite: f32,
    ) -> Option<(narsil_chrome::ktx_menue::Kontext, String, String)> {
        use narsil_chrome::ktx_menue::Kontext;
        let a = self.angezeigt.as_ref()?;
        let kaesten = bild_kaesten(a);
        let y = y_seite + a.versatz as f32;
        for bk in a.seite.bilder.iter().rev() {
            let Some(k) = kaesten.get(&bk.knoten) else {
                continue;
            };
            // GEMESSEN: ein Kasten ohne Flaeche ist **unsichtbar** (`display:
            // none`), und ein Treffer darauf waere ein Bild, das der Nutzer
            // nicht sieht.
            if k.width <= 0.0 || k.height <= 0.0 {
                continue;
            }
            if x < k.x || x > k.x + k.width || y < k.y || y > k.y + k.height {
                continue;
            }
            let endung = endung_der_quelle(&bk.bild.quelle);
            let kontext = if VIDEO_ENDUNGEN.contains(&endung.as_str()) {
                Kontext::Video
            } else {
                Kontext::Bild
            };
            //
            // KONTEXT (04.10.2026): DIE **Bildbeschreibung** kommt mit.
            // GEMESSEN ist der Weg: `HtmlNode::attribut("alt")` gibt den Text
            // des **Knotens** — und der Knoten steht hier, nicht im Menue.
            // GEMESSENE ENTSCHEIDUNG, warum **nur** `alt` und nicht alle
            // Attribute: die Zeile heisst „Bildbeschreibung“, und ein Attribut
            // mehr waere eine Antwort auf eine Frage, die niemand gestellt hat.
            //
            // Die HTML-Bildangaben gehoeren bereits zum getroffenen Bild.
            let alt = bk.bild.alt.clone();
            return Some((kontext, bk.bild.quelle.clone(), alt));
        }
        None
    }

    /// KONTEXT (03.10.2026): WELCHE MENUE-ZEILE LIEGT UNTER DEM PUNKT?
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das an `narsil_chrome::ktx_menue` geht und
    /// nicht hier gerechnet wird: `Menue::zeile_unter` rechnet aus **derselben**
    /// Zeilenliste, aus der das Menue **gezeichnet** wird (`Menue::zeilen`). Der
    /// alte Weg (`menue_treffer` mit `ZEILE_MENUE_HOEHE`) hatte **drei** eigene
    /// Zahlen — 22 px Zeile, 160 px Breite, 1 px Luft — und die Zeichnung hatte
    /// **andere** (24 px Zeile, 248 px Breite, 4 px Luft). GEMESSENE FOLGE: das
    /// Menue waere **an anderer Stelle** getroffen worden als gezeichnet, und
    /// bei 22 statt 24 px waere der vierte Eintrag **eine Zeile zu hoch**
    /// gewesen. Das ist die Fehlerklasse „zwei Stellen entscheiden“.
    ///
    /// KONTEXT (04.10.2026, 06:35): DER KLICK INS **TABMENUE** — und die
    /// fuenf Handlungen dahinter.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **ein** `bool` gibt wie
    /// [`Sitzung::menue_klick`]: der Aufrufer muss wissen, ob der Klick
    /// verbraucht war — sonst ginge ein Klick auf „Schließen“ an die Seite
    /// weiter und schließe den Tab **und** lud die Seite.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum jede Zeile `anwenden` sagt und nichts
    /// selbst: die Crate `narsil-browser` hat EINEN Weg, den Zustand zu
    /// aendern (`anwenden`), und die drei neuen Zeilen sind dort gebaut
    /// (04.10.2026, Freigabe der Leitung). Hier steht nur, **welche** Zeile
    /// welchen Befehl bedeutet.
    fn tabmenue_klick(&mut self, x: f32, y: f32) -> bool {
        let p = Point::new(x, y);
        // Release 08.10.2026: die Zeile kommt aus dem Menue selbst — mit den
        // Features 3 und 5 ist die Liste laenger als `ALLE_TAB`.
        let (nr, eintrag) = match self.tabmenue.as_ref() {
            Some(m) => {
                let nr = m.nr_unter(self.breite, self.hoehe, &p);
                (nr, nr.and_then(|n| m.eintrag(n)))
            }
            None => return false,
        };
        self.tabmenue = None;
        let Some(nr) = nr else {
            //
            // KONTEXT (04.10.2026, 09:20): GEMESSENER EIGENER FEHLER in der
            // ersten Fassung: hier stand `return true` — der Klick **daneben**
            // waere geschluckt worden und nie bei der Seite angekommen.
            // GEMESSEN ist das Verhalten des Seitenmenues (`menue_klick`,
            // `sitzung.rs:1774/1775`): `self.schmutzig = true; false` — das
            // Menue schliesst, und der Klick **geht weiter**. Zwei Menues, die
            // sich beim Klick daneben anders verhalten, waeren eine Falle, die
            // man erst beim Klick bemerkt.
            self.schmutzig = true;
            return false;
        };
        use narsil_chrome::ktx_menue::TabEintrag;
        //
        // KONTEXT (04.10.2026, Leitung „(1) Tab anheften / Lautlos / Tabs
        // rechts schließen / Wiederherstellen“): JEDE Zeile sagt `anwenden`, und
        // die Zeile **„Tab in neuem Fenster“** hat kein `anwenden` — GEMESSEN ist
        // (siehe `ktx_menue.rs`, `TabEintrag::aktiv`), dass es kein
        // Fenstermodell gibt. Sie ist grau und damit **nicht** klickbar; der
        // Zweig darunter ist nur die Antwort, falls das sich aendert.
        let Some(eintrag) = eintrag else {
            self.schmutzig = true;
            return false;
        };
        let _ = nr;
        let befehl = match eintrag {
            // Release 08.10.2026: die Zeilen der Features 3 und 5.
            TabEintrag::TabNetz => {
                self.netz_umschalten();
                return true;
            }
            TabEintrag::TabEinfrieren => {
                self.aktiven_tab_einfrieren();
                return true;
            }
            TabEintrag::TabNeu => Navigation::TabNeu,
            TabEintrag::TabNeuLaden => Navigation::Reload,
            TabEintrag::TabAnheften => Navigation::TabAnheften,
            TabEintrag::TabStumm => Navigation::TabLautlos,
            TabEintrag::TabDuplizieren => Navigation::TabDuplizieren,
            TabEintrag::TabNeuesFenster => {
                self.meldung = Some(tr("No new window: the browser has only one window").into());
                self.schmutzig = true;
                return true;
            }
            TabEintrag::TabSchliessen => Navigation::TabSchliessen,
            TabEintrag::TabAndereSchliessen => Navigation::TabAndereSchliessen,
            TabEintrag::TabsRechtsSchliessen => Navigation::TabsRechtsSchliessen,
            TabEintrag::TabWiederherstellen => Navigation::TabWiederherstellen,
        };
        //
        // KONTEXT (04.10.2026): DIE MELDUNG NENNT DIE **ZAHL**, wenn eine da
        // ist. GEMESSENE ENTSCHEIDUNG, warum „2 Tabs rechts geschlossen“ mehr
        // sagt als „geht“: der Nutzer will wissen, **wieviele** Tabs weg sind.
        let rechts = self.browser.tabs_rechts_von(self.browser.aktiver_tab_nr());
        match self.browser.anwenden(&befehl) {
            Wirkung::Leer => {
                self.meldung = Some(match eintrag {
                    TabEintrag::TabSchliessen => tr("The last tab stays open").into(),
                    TabEintrag::TabAndereSchliessen => tr("There are no other tabs").into(),
                    TabEintrag::TabWiederherstellen => tr("No tab was closed").into(),
                    _ => tr("That is not possible here").to_string(),
                });
            }
            Wirkung::Geaendert => {
                if rechts > 0
                    && eintrag == narsil_chrome::ktx_menue::TabEintrag::TabsRechtsSchliessen
                {
                    self.meldung = Some(trf!("{} tab(s) to the right closed", rechts));
                }
            }
        }
        self.schmutzig = true;
        true
    }

    /// GEMESSENE ENTSCHEIDUNG fuer `None`: `zeile_unter` gibt `None`, wenn der
    /// Punkt **ausserhalb** liegt — auch wenn er im Rechteck-Kasten, aber in
    /// der Zeile „unter der letzten“ ist. Genau das war der Grund fuer
    /// `Option` (UI4, Zeile 0 = „Link oeffnen“ waere sonst der Klick daneben).
    /// KONTEXT (04.10.2026, 21:50): DER KLICK AUF **TEXT** — einmal, zweimal
    /// (Wort), dreimal (Absatz), mit **Umschalt** (erweitern).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Klickzaehler an der **Sitzung** klebt
    /// und nicht im Ereignis steht: die drei Faelle muessen sich **kennen** — der
    /// zweite Klick weiss nur dann, dass es der zweite ist, wenn der erste
    /// gezaehlt wurde.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das Fenster **400 ms** ist und nicht
    /// 500: GEMESSEN ist, dass der Ablauf 8 ms pollt — 400 ms sind 50 Takte, und
    /// ein Doppelklick, der langsamer kommt, ist in Firefox auch kein
    /// Doppelklick.
    fn auswahl_klick(&mut self, p: Point) {
        //
        // KONTEXT (05.10.2026, 01:02): DER ZAEHLER IST **GEPRUEFT** —
        // `klickzahl` (gruene Crate, 3 Tests). GEMESSENE ENTSCHEIDUNG, warum
        // `jetzt_ns` und nicht `Instant`: siehe beim Feld `klick_zeit`.
        let jetzt_ns = crate::video::jetzt_ns();
        self.klick_zaehler = klickzahl(self.klick_zaehler, self.klick_zeit, jetzt_ns);
        self.klick_zeit = Some(jetzt_ns);

        // GEMESSENE ENTSCHEIDUNG, warum **Umschalt** VOR dem Doppelklick kommt:
        // mit Umschalt ist der Klick immer eine **Erweiterung** — auch der
        // zweite und der dritte (Firefox ebenso).
        if self.auswahl.anfang.is_some() && self.umschalt {
            if let Some(stelle) = self.stelle_unter(p.x, p.y) {
                self.auswahl.ende = Some(stelle);
                self.auswahl_text_holen();
                self.schmutzig = true;
            }
            return;
        }
        let Some((zeile, zeichen)) = self.stelle_unter(p.x, p.y) else {
            // Kein Text getroffen: die Auswahl **geht** weg (Firefox).
            self.auswahl = Auswahl::default();
            self.auswahl_text.clear();
            self.schmutzig = true;
            return;
        };
        let stellen = self
            .angezeigt
            .as_ref()
            .map(|a| text_stellen(a))
            .unwrap_or_default();
        self.auswahl = match self.klick_zaehler {
            2 => {
                let (an, aus) = stellen
                    .get(zeile)
                    .map(|st| wort_grenzen(&st.text, zeichen))
                    .unwrap_or((zeichen, zeichen));
                Auswahl {
                    anfang: Some((zeile, an)),
                    ende: Some((zeile, aus)),
                    zieht: false,
                    shift: false,
                }
            }
            3 => {
                let ((za, ka), (ze, ke)) = absatz_grenzen(&stellen, zeile, zeichen);
                Auswahl {
                    anfang: Some((za, ka)),
                    ende: Some((ze, ke)),
                    zieht: false,
                    shift: false,
                }
            }
            _ => Auswahl {
                anfang: Some((zeile, zeichen)),
                ende: Some((zeile, zeichen)),
                zieht: true,
                shift: false,
            },
        };
        self.auswahl_text_holen();
        self.schmutzig = true;
    }

    /// DIE AUSWAHL **BEWEGEN** (beim Ziehen) — `true`, wenn sich etwas bewegt
    /// hat.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier **das Ende** wandert und nicht der
    /// Anfang: der Nutzer zieht vom **Start** weg — wandert der Anfang, laeuft
    /// die Auswahl **vorwaerts**, und ein Wort waer beim Ziehen nach links
    /// wieder weg, bevor es da war.
    fn auswahl_bewegen(&mut self, p: Point) -> bool {
        let Some((zeile, zeichen)) = self.stelle_unter(p.x, p.y) else {
            return false;
        };
        if self.auswahl.ende == Some((zeile, zeichen)) {
            return false;
        }
        self.auswahl.ende = Some((zeile, zeichen));
        self.auswahl_text_holen();
        true
    }

    /// KONTEXT (04.10.2026, 16:35): DER KLICK AUF DEN SCROLLBALKEN — Greifen
    /// oder ein Sprung um **eine Seite**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum eine **Seite** und nicht „ans Fensterende“:
    /// Firefox springt seitenweise, und der Sprung ans Ende überspringt genau das,
    /// worum es beim Scrollen geht: den Absatz, der nicht passt.
    fn scrollspur(&self) -> Option<Rechteck> {
        let sicht = self.sicht_hoehe();
        let inhalt = self.angezeigt.as_ref()?.inhalt_hoehe;
        if inhalt <= sicht || sicht < SCROLLBREITE * 2 { return None; }
        Some(Rechteck {
            x: self.breite.saturating_sub(SCROLLBREITE),
            y: self.chrome.seiten_oberkante(),
            breite: SCROLLBREITE,
            hoehe: sicht,
        })
    }

    fn scrollbalken_klick(&mut self, p: Point) -> bool {
        let sicht = self.sicht_hoehe();
        let (inhalt, versatz) = self
            .angezeigt
            .as_ref()
            .map_or((0, 0), |a| (a.inhalt_hoehe, a.versatz));
        let Some(spur) = self.scrollspur() else { return false; };
        let Some(ziel) =
            self.scrollbalken.trifft_spur(&spur, sicht, inhalt, versatz, p)
        else {
            return false;
        };
        let max = inhalt.saturating_sub(sicht);
        match ziel {
            Scrollziel::Griff => {
                // GEMESSENE ENTSCHEIDUNG, warum **beide** Zahlen gespeichert
                // werden (Versatz **und** Griffoberkante): die Griffoberkante
                // ist der Abstand des Zeigers zum Griff beim Greifen — ohne sie
                // springt der Griff beim ersten Ziehen.
                if let Some(spur) = self.scrollspur() {
                    let g = self.scrollbalken.griff(&spur, sicht, inhalt, versatz);
                    self.scrollbalken.zieht = Some((versatz as i64, g.y));
                    self.scrollbalken.zeigen(std::time::Instant::now());
                }
            }
            Scrollziel::Seite(runter) => {
                let sprung = if runter {
                    versatz.saturating_add(sicht)
                } else {
                    versatz.saturating_sub(sicht)
                };
                //
                // KONTEXT (04.10.2026, 18:35): **sanft** — GEMESSENE
                // ENTSCHEIDUNG, warum der Sprung hier sanft sein darf und beim
                // Rad nicht: `narsil/tests/browser.rs:228` hält das Rad fest
                // (fremde Datei), diesen Weg haelt kein Test fest.
                self.scroll_ziel_setzen(sprung.min(max), std::time::Instant::now());
            }
        }
        self.schmutzig = true;
        true
    }

    fn menue_treffer(&self, x: f32, y: f32) -> Option<narsil_chrome::ktx_menue::Zeile> {
        let m = self.menue.as_ref()?;
        let p = Point::new(x, y);
        //
        // KONTEXT (03.10.2026, GEMESSUNG -> ENTFERNT): die Geometrie
        // wurde hier als `eprintln!` ausgegeben, weil der Klick auf
        // „Als WebP“ nichts ausloeste und das Bild beide Faelle gleich
        // zeigte. GEMESSEN ist die Ursache inzwischen behoben (das
        // Untermenue-Feld wird abgefragt) — die Zeile bleibt weg, weil
        // ein Menue **jeder** Klick eine Zeile auf stderr schreibt.
        //
        // KONTEXT (03.10.2026): DAS UNTERMENUE-FELD ZUERST.
        //
        // GEMESSENER BEFUND (18:1x, Fenster, `ktx_narsil_datei_nach-ereignis.png`):
        // ohne diese Zeile **schloss** der Klick auf „Als WebP“ das Menue und
        // tat sonst nichts — der Treffer war `None`, weil `zeile_unter` nur das
        // **Hauptmenue** kennt (das Untermenue ist ein eigenes Feld **daneben**,
        // siehe `Menue::untermenue_rechteck`).
        //
        // GEMESSENE ENTSCHEIDUNG, warum hier **beide** Abfragen stehen und
        // nicht eine: die beiden Felder haben verschiedene Rechtecke **und**
        // verschiedene Zeilenlisten (`Menue::zeilen` gegen
        // `Menue::untermenue_ziele`). Eine gemeinsame Nummer kann die nicht
        // unterscheiden — und der Aufrufer soll wissen, **welches** Feld er
        // getroffen hat, weil die Handlungen verschieden sind.
        if let Some(ziel) = m.ktx.untermenue_ziel_unter(self.breite, self.hoehe, &p) {
            return Some(narsil_chrome::ktx_menue::Zeile::Ziel(ziel));
        }
        m.ktx.zeile_unter(self.breite, self.hoehe, &p)
    }

    /// KONTEXT (03.10.2026): GELOESCHT.
    ///
    /// GEMESSENER BEFUND: diese Liste („Link oeffnen“, „Link speichern unter“,
    /// „Adresse kopieren“, „Untersuchen“) war die **zweite** Antwort auf
    /// „was steht im Menue“ — die erste ist `narsil_chrome::ktx_menue::zeilen`.
    /// GEMESSENE ENTSCHEIDUNG, warum sie **weg** und nicht „gepflegt“ heisst:
    /// `ANFORDERUNGEN.txt` Zeile 21 verlangt „Kopieren“ **ganz oben** und
    /// Zeile 20 das Untermenue — beides kann diese Liste nicht, weil sie an
    /// **Texten** haengt und nicht am Zustand.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum kein `Vec` daneben stehen bleibt: eine
    /// zweite Liste, die man pflegt, ist die Fehlerklasse „zwei Stellen
    /// entscheiden“ — und genau die hat `knopf_farben` (siehe Kommentar oben)
    /// und `ZEILE_MENUE_HOEHE` (UI4 09:25) getroffen.

    fn menue_klick(&mut self, x: f32, y: f32) -> bool {
        if self.menue.is_none() {
            return false;
        }
        let treffer = self.menue_treffer(x, y);
        // GEMESSENE ENTSCHEIDUNG, warum der Zustand **vor** `self.menue = None`
        // gelesen wird: die Zeile traegt ihre **Quelle** (`Menue::quelle`) und
        // der Link steht in `RechtsklickMenue::link`. Wer erst loescht und dann
        // liest, hat beides nicht mehr — das war der Fehler von 10:10
        // (`self.menue.take()` vor `menue_klick`, siehe Kommentar in `klick`).
        let (link, quelle) = self
            .menue
            .as_ref()
            .map(|m| (m.link.clone(), m.ktx.quelle.clone()))
            .unwrap_or_default();
        let kontext = self
            .menue
            .as_ref()
            .map_or(narsil_chrome::ktx_menue::Kontext::Leer, |m| m.ktx.kontext);
        // Release 08.10.2026 (Feature 2): wo der Rechtsklick war — dort liegt
        // das Element, das beobachtet werden soll.
        let (menue_x, menue_y) = self.menue.as_ref().map_or((0.0, 0.0), |m| (m.x, m.y));
        let sprache = narsil_chrome::ktx_menue::sprache_aus_umgebung();
        // Das Untermenue: ein Klick auf „Konvertiert herunterladen >“ **oeffnet**
        // es, statt es zu schliessen. GEMESSENE ENTSCHEIDUNG, warum das so und
        // nicht „immer schliessen“: Firefox laesst das Untermenue offen stehen,
        // wenn der Zeiger dariueber ist — und ein zweiter Klick auf dieselbe
        // Zeile **schliesst** es (`unterm Toggle`).
        if treffer.as_ref()
            == Some(&narsil_chrome::ktx_menue::Zeile::Aktion(
                narsil_chrome::ktx_menue::Eintrag::Konvertieren,
            ))
        {
            if let Some(m) = self.menue.as_mut() {
                if m.ktx.ist_aufgeklappt() {
                    m.ktx.klappe_zu();
                } else {
                    m.ktx.klappe_auf();
                }
            }
            self.schmutzig = true;
            return true;
        }
        self.menue = None;
        if let Some(zeile) = treffer {
            use narsil_chrome::ktx_menue::{Eintrag, Zeile};
            match zeile {
                Zeile::Ziel(ziel) => {
                    // DER KLICK, der die Datei macht (ANFORDERUNGEN Zeile 20).
                    self.konvertieren_und_speichern(&quelle, &ziel);
                    self.schmutzig = true;
                    return true;
                }
                Zeile::Aktion(e) => {
                    match e {
                        //
                        // KONTEXT (03.10.2026, 04:00): „Kopieren“ legt **etwas**
                        // ab, und **was**, entscheidet der Kontext (siehe
                        // `Eintrag::kopier_ziel`): an einem Link die Adresse, an
                        // einem Bild die Bild-Adresse, an **markiertem Text**
                        // der Text. GEMESSENE EINSCHRAENKUNG, die ehrlich
                        // benannt wird: `narsil-page` hat kein Auswahlmodell
                        // (BRETT 02:55), also ist der „Text“ hier die Quelle,
                        // die der Kontext nennt — und das ist genau die Angabe,
                        // die auch `uebersetzen()` und `google_suchen()`
                        // bekommen.
                        Eintrag::Kopieren => {
                            // KONTEXT (04.10.2026, 22:15): DIE AUSWAHL gewinnt
                            // gegen die Adresse. REPARIERT 05.10.2026: dieser
                            // Zweig stand als ZWEITER `Eintrag::Kopieren`-Arm
                            // (`if !self.auswahl.ist_leer()`) weiter unten —
                            // ein bereits getroffener erster Arm macht ihn
                            // unerreichbar (Compilerwarnung `unreachable
                            // pattern`), die markierte Auswahl wurde also NIE
                            // kopiert.
                            if !self.auswahl.ist_leer() {
                                let text = self.auswahl_text_holen();
                                self.zwischenablage = text.clone();
                                self.meldung = Some(trf!(
                                    "Selection copied: {} characters",
                                    text.chars().count()
                                ));
                            } else if kontext == narsil_chrome::ktx_menue::Kontext::Text {
                                self.text_kopieren(&quelle);
                            } else if matches!(
                                kontext,
                                narsil_chrome::ktx_menue::Kontext::Link
                                    | narsil_chrome::ktx_menue::Kontext::Bild
                                    | narsil_chrome::ktx_menue::Kontext::Video
                            ) && !quelle.is_empty()
                            {
                                // Release 09.10.2026: am Link (Bild, Video)
                                // kopiert „Kopieren" DESSEN Adresse
                                // (`Eintrag::kopier_ziel`). GEMESSEN vorher:
                                // es kopierte die Adresse der SEITE — auf
                                // `narsil://start` stand „Adresse kopiert: "
                                // mit leerem Text in der Meldung.
                                let adresse = self.features_teilen(&quelle);
                                self.zwischenablage = adresse.clone();
                                self.meldung = Some(trf!("Address copied: {}", adresse));
                            } else {
                                self.adresse_kopieren();
                            }
                        }
                        //
                        // KONTEXT (03.10.2026, 21:35, Freigabe: „Uebersetzen
                        // oeffnet translate.google.com mit tl=Browsersprache in
                        // **neuem Tab**“): GEMESSENER BEFUND: hier stand
                        // `gehe_zu(&url)` — das ersetzt die **aktuelle** Seite
                        // durch die Uebersetzung. GEMESSENE ENTSCHEIDUNG:
                        // `link_in_neuem_tab` ist derselbe Weg, den
                        // `LinkNeuerTab` geht (`Navigation::TabNeu` +
                        // `Navigation::To`), und es ist der Weg, den der
                        // Nutzer von Firefox kennt: die Seite bleibt, die
                        // Uebersetzung steht daneben.
                        //
                        // GEMESSENE ENTSCHEIDUNG, warum die Adresse **hier**
                        // gebaut wird und nicht in `narsil`: `ktx_menue::
                        // uebersetzen` ist der Weg mit `sl=auto` und
                        // `tl=<Browsersprache>`, und er gehoert zur
                        // Menue-Datei. Hier steht nur der **Tab**.
                        Eintrag::Uebersetzen => {
                            if let Some(url) =
                                narsil_chrome::ktx_menue::uebersetzen(kontext, &quelle, &sprache)
                            {
                                self.link_in_neuem_tab(&url);
                            } else {
                                self.meldung =
                                    Some(tr("Nothing to translate — select some text first").into());
                            }
                        }
                        //
                        // KONTEXT (03.10.2026, 02:10, Leitung „(1) markierter
                        // Text: … Mit Google suchen, Alles auswaehlen“):
                        // GEMESSENE ENTSCHEIDUNG, warum die URL **hier** nicht
                        // gebaut wird: `ktx_menue::google_suchen` ist der Ort
                        // (eine Adresse ist eine Angabe der Menue-Datei, siehe
                        // `uebersetzen_url`). Hier steht nur der **Tab** —
                        // derselbe Weg wie bei „Übersetzen“.
                        //
                        // GEMESSENE EINSCHRAENKUNG, die **nicht** meine ist:
                        // `narsil-page` hat **kein** Auswahlmodell (GEMESSEN:
                        // `grep "pub fn" crates/narsil-page/src/lib.rs` findet
                        // `link_unter` und **keine** Auswahl-Funktion), also
                        // ist `quelle` hier die **Adresse**, nicht der Text —
                        // die Suche laeuft also noch nicht, und die Meldung
                        // sagt es dem Nutzer statt zu raten. BRETT 02:40.
                        Eintrag::GoogleSuchen => {
                            match narsil_chrome::ktx_menue::google_suchen(&quelle) {
                                Some(url) => self.link_in_neuem_tab(&url),
                                None => {
                                    self.meldung =
                                        Some(tr("Nothing to search for — select some text first").into())
                                }
                            }
                        }
                        //
                        // KONTEXT (03.10.2026, 02:10): „Alles auswählen“ und die
                        // sechs **Feldzeilen** brauchen ein **Auswahl- bzw.
                        // Feldmodell**, das es nicht gibt (GEMESSEN: siehe
                        // oben). GEMESSENE ENTSCHEIDUNG, warum trotzdem **hier**
                        // ein Zweig steht und nicht `unreachable!()`: das Menue
                        // zeigt die Zeile, also muss der Klick etwas sagen —
                        // ein Panic waere die schlechteste der drei Antworten.
                        Eintrag::AllesAuswaehlen => {
                            self.meldung = Some(
                                tr("Select all is not built in this browser (the page has no selection)")
                                    .into(),
                            )
                        }
                        //
                        // KONTEXT (03.10.2026, 02:10, Leitung „(3) leere
                        // Seitenflaeche: Zurueck, Vorwaerts, Neu laden, Seite
                        // speichern unter, Drucken, Quelltext anzeigen, Element
                        // untersuchen“): GEMESSEN ist, welche davon **wirklich**
                        // gehen — `Navigation::Back`/`Forward`/`Reload` sind
                        // gebaut (`narsil-browser/src/lib.rs:80-84`), der Download
                        // ist gebaut (`download_url`), die Entwicklerwerkzeuge
                        // sind gebaut (`Untersuchen`).
                        Eintrag::Zurueck => {
                            if let narsil_browser::Wirkung::Leer =
                                self.browser.anwenden(&narsil_browser::Navigation::Back)
                            {
                                self.meldung =
                                    Some(tr("Cannot go back — this is the first page").into());
                            }
                        }
                        Eintrag::Vorwaerts => {
                            if let narsil_browser::Wirkung::Leer =
                                self.browser.anwenden(&narsil_browser::Navigation::Forward)
                            {
                                self.meldung =
                                    Some(tr("Cannot go forward — this is the last page").into());
                            }
                        }
                        //
                        // GEMESSENE ENTSCHEIDUNG, warum „Seite speichern
                        // unter“ **derselbe** Weg ist wie „Link speichern
                        // unter“: es ist derselbe Download mit einem anderen
                        // Namen. GEMESSENE ENTSCHEIDUNG, warum der Dateiname
                        // **nicht** aus dem Menue kommt: der Aufrufer kennt die
                        // Adresse, und `download_url` baut den Namen daraus.
                        //
                        // KONTEXT (03.10.2026, 04:00): „Seite speichern unter“
                        // **schreibt eine Datei** — die Seite als HTML in
                        // `~/Downloads` (siehe `Sitzung::seite_speichern`).
                        // GEMESSENER BEFUND vorher: hier stand
                        // `download_url(&adresse, "download")` — das laedt die
                        // Adresse **neu** und schreibt, was der Server sendet;
                        // im Test gibt es kein Netz, und der Nutzer wollte die
                        // Seite, die er **sieht**.
                        Eintrag::SeiteSpeichern => self.seite_speichern(),
                        //
                        // GEMESSENE ENTSCHEIDUNG, warum „Drucken“ eine
                        // **Meldung** ist und kein Versuch zu drucken: es gibt
                        // keinen Druckweg in diesem Browser (GEMESSEN:
                        // `narsil-window` hat kein Druck-Kommando). Eine Meldung
                        // ist ehrlich; ein stilles Nichts waere es nicht.
                        Eintrag::Drucken => {
                            // Release 09.10.2026: derselbe Weg wie Strg+P (PDF).
                            // Vorher: „Drucken ist in diesem Browser nicht gebaut".
                            self.drucken();
                        }
                        //
                        // GEMESSENE ENTSCHEIDUNG, warum der Quelltext in die
                        // **Meldung** geht: `view-source:` ist im Browser kein
                        // Schema (GEMESSEN: `laden` kann es nicht holen), und
                        // eine Datei dafuer zu schreiben waere ein zweiter
                        // Speicher. GEMESSENE ENTSCHEIDUNG, warum der Text
                        // **gekuerzt** wird: die Meldung ist eine Zeile.
                        //
                        // KONTEXT (03.10.2026, 04:00, Leitung: „Quelltext
                        // anzeigen **oeffnet view-source-Tab**“): GEMESSENE
                        // ENTSCHEIDUNG, warum ein **Tab** und keine Meldung:
                        // die Leitung hat es so genannt, und es ist der Weg,
                        // den Firefox geht.
                        //
                        // KONTEXT (04.10.2026, 08:40, Leitung „view-source:
                        // **echt** machen“): DER BISHERIGE ZWEIG WAR **HALB**
                        // WAHR. Er oeffnete den Tab, und der blieb **leer** —
                        // GEMESSEN ist jetzt, dass `link_in_neuem_tab`
                        // (`sitzung.rs:2188`) **nicht** ueber `gehe_zu` geht,
                        // sondern nur `Navigation::To` sagt. Der Abfang in
                        // `gehe_zu` wurde also von dieser Zeile **nie**
                        // erreicht. Deshalb steht der Aufruf jetzt **hier**:
                        // erst der Tab (Firefox-Wortlaut), dann der Inhalt.
                        //
                        // GEMESSENE ENTSCHEIDUNG, warum die **alte** Adresse
                        // mitgegeben wird und nicht die neue: der Rohtext
                        // gehoert zur **geladenen** Seite (`Angezeigt`), und
                        // `browser.adresse()` ist jetzt schon
                        // `view-source:…`.
                        //
                        // KONTEXT (06.10.2026, 05:05): Der Weg geht jetzt durch
                        // `quelltext_zeigen()` — **eine** Quelle fuer Menue und
                        // Taste. GEMESSENER BEFUND, warum das noetig war: der
                        // Menue-Zweig tat **zwei** Dinge (neuer Tab **und**
                        // `view_source_zeigen`), der Tastenzweig (Z. 3758) tat
                        // nur **eines** — dieselbe Ansicht, zwei Bedeutungen,
                        // und die Taste zeigte eine Seite ohne Rohtext.
                        Eintrag::Quelltext => {
                            self.quelltext_zeigen();
                        }
                        //
                        // DIE SECHS FELDZEILEN (Leitung 02:10, „(2) Eingabefeld“):
                        // GEMESSEN ist, dass `narsil` den Rechtsklick im Feld
                        // **nicht** erkennt (es gibt keine `feld_unter`-Abfrage,
                        // siehe BRETT 02:40). Also ist der Zweig hier der
                        // **Vertrag**: kommt das Feld, sagt der Klick, was
                        // geht — und nicht `unreachable!()`.
                        //
                        // KONTEXT (03.10.2026, 04:05): „Einfuegen“ **nimmt**
                        // aus der Zwischenablage — das ist der zweite Benutzer
                        // des Feldes [`Sitzung::zwischenablage`] und damit der
                        // Grund, warum es kein toter Zustand ist. GEMESSEN ist,
                        // dass die **anderen drei** weiterhin nichts koennen: es
                        // gibt kein Feldmodell (BRETT 02:55), also kein
                        // Rueckwaertsstapel und keine Markierung.
                        Eintrag::Einfuegen => {
                            if self.zwischenablage.is_empty() {
                                self.meldung = Some(tr("The clipboard is empty").into());
                            } else {
                                let text = self.zwischenablage.clone();
                                self.meldung = Some(trf!(
                                    "Pasted: {}",
                                    text.chars().take(60).collect::<String>()
                                ));
                            }
                        }
                        Eintrag::Rueckgaengig | Eintrag::Ausschneiden | Eintrag::Loeschen => {
                            self.meldung = Some(trf!(
                                "{} is not built in this browser — input fields have no state yet",
                                e.text()
                            ));
                        }
                        Eintrag::LinkOeffnen => {
                            if let Err(f) = self.gehe_zu(&link) {
                                self.meldung = Some(f.to_string());
                            }
                        }
                        Eintrag::LinkNeuerTab => self.link_in_neuem_tab(&quelle),
                        // GEMESSENE ENTSCHEIDUNG: „Bild-Adresse anzeigen“ gibt
                        // die Adresse **in der Meldung** — es gibt in diesem
                        // Browser keine Zwischenablage (`adresse_kopieren` sagt
                        // das ausdruecklich), und ein zweites Feld waere eine
                        // zweite Anzeige derselben Sache.
                        //
                        // KONTEXT (04.10.2026, Leitung „Bildadresse kopieren“):
                        // DIE ZEILE KOPIERT JETZT. GEMESSENE ENTSCHEIDUNG,
                        // warum der alte Kommentar hier nicht mehr gilt: er
                        // sagte „es gibt in diesem Browser keine
                        // Zwischenablage“ — GEMESSEN ist, dass es sie seit
                        // 03.10. (KONTEXT_12) gibt (`Sitzung::zwischenablage`).
                        Eintrag::BildAdresse => {
                            self.zwischenablage = quelle.clone();
                            self.meldung = Some(trf!("Image address copied: {}", quelle));
                        }
                        //
                        // KONTEXT (04.10.2026): DAS BILD IM NEUEN TAB — derselbe
                        // Weg wie bei einem Link (`link_in_neuem_tab`).
                        Eintrag::BildNeuerTab => self.link_in_neuem_tab(&quelle),
                        //
                        // KONTEXT (04.10.2026): DIE BILDBESCHREIBUNG. GEMESSENE
                        // ENTSCHEIDUNG, warum ein **leerer** `alt`-Text eine
                        // eigene Meldung bekommt und nicht „“: ein Bild ohne
                        // `alt` ist fuer einen Screenreader unbeschrieben, und
                        // „keine Beschreibung“ ist eine **Auskunft**, ein
                        // leeres Kasten ist keine.
                        Eintrag::BildBeschreibung => {
                            self.meldung = Some(if self.bild_alt.trim().is_empty() {
                                trf!("Image description: none (no alt text) — {}", quelle)
                            } else {
                                trf!("Image description: {}", self.bild_alt)
                            });
                        }
                        //
                        // KONTEXT (04.10.2026, Leitung „Link als Lesezeichen“):
                        // GEMESSEN ist, dass `narsil-browser` ein echtes
                        // Lesezeichenmodell hat (`daten.rs:73`) — der Klick
                        // schreibt **dorthin**, und die Lesezeichenleiste zeigt
                        // es danach. GEMESSENE ENTSCHEIDUNG, warum der **Titel**
                        // die Adresse ist: GEMESSEN ist, dass `link_unter` nur
                        // die Adresse liefert — der Linktext waere eine zweite
                        // Abfrage an dieselbe Seite.
                        Eintrag::LinkLesezeichen => {
                            let neu = self
                                .browser
                                .lesezeichen_hinzufuegen(quelle.clone(), quelle.clone());
                            self.meldung = Some(if neu {
                                trf!("Bookmark added: {}", quelle)
                            } else {
                                trf!("Bookmark already existed (now at the top): {}", quelle)
                            });
                        }
                        //
                        // GEMESSENE EIN-SCHRAENKUNG: diese Zeile ist **grau**
                        // (`Eintrag::aktiv`), also ist der Zweig unerreichbar.
                        // Er steht trotzdem hier — mit derselben Begruendung wie
                        // bei den Feldzeilen: ein `match`, der die Zeile nicht
                        // kennt, ist ein Compilerfehler, sobald jemand den
                        // Zustand aendert, und die Meldung sagt dann, was fehlt.
                        Eintrag::LinkPrivatesFenster => {
                            self.meldung = Some(
                                tr("No private window: the browser has only one window").into(),
                            );
                        }
                        // KONTEXT (03.10.2026, 21:35): `AudioSpeichern` ist
                        // derselbe Download wie bei Bild und Video — GEMESSENE
                        // ENTSCHEIDUNG, warum kein eigener Zweig: es gibt
                        // keinen Unterschied, und ein eigener Zweig waere ein
                        // zweiter Weg zum selben Download.
                        Eintrag::LinkSpeichern
                        | Eintrag::BildSpeichern
                        | Eintrag::VideoSpeichern
                        | Eintrag::AudioSpeichern => self.download_url(&quelle, "download"),
                        Eintrag::Untersuchen => crate::devtools_bruecke::aus_menue(self),
                        Eintrag::SeiteNeuLaden => {
                            let url = self.browser.adresse().to_string();
                            if let Err(f) = self.gehe_zu(&url) {
                                self.meldung = Some(f.to_string());
                            }
                        }
                        Eintrag::SeitendatenLoeschen => self.adresse_kopieren(),
                        // GEMESSENE ENTSCHEIDUNG: `Konvertieren` ist oben schon
                        // behandelt (es schliesst das Menue nicht). Es steht
                        // hier **nicht** als `Konvertieren => {}`, damit ein
                        // neues `Eintrag` hier **auffällt** und nicht still
                        // nichts tut.
                        Eintrag::Konvertieren => {}
                        // Release 08.10.2026 (Feature 2, Element-Waechter).
                        Eintrag::ElementBeobachten => self.element_beobachten(menue_x, menue_y),
                    }
                    self.schmutzig = true;
                    return true;
                }
            }
        }
        self.schmutzig = true;
        false
    }

    /// KONTEXT (03.10.2026): DIE AUSGEWAEHLTE DATEI UMFORMATEN UND SPEICHERN.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **hier** und nicht in `narsil-chrome`: das
    /// Umformatieren ist `narsil_chrome::ktx_konvert::konvertiere` (Byte in,
    /// Datei raus) — aber **schreiben** muss der Browser, denn er weiss sein
    /// Download-Verzeichnis. GEMESSENE ENTSCHEIDUNG, warum der Weg ueber
    /// `crate::laden::lade_bytes` laeuft und nicht ueber `download_url`: das
    /// Umformatieren braucht die **Bytes**, und `download_url` schreibt die
    /// Originaldatei — das waere ein Download, dessen Ergebnis niemand liest.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein Fehler in die **Meldung** geht und
    /// nicht in ein `panic`: ANFORDERUNGEN Zeile 20 sagt „kein externes
    /// Konvertieren noetig“ — und ein Klick, der abstuerzt, waere genau das.
    fn konvertieren_und_speichern(&mut self, quelle: &str, ziel: &narsil_chrome::konvert::Ziel) {
        //
        // KONTEXT (03.10.2026, GEMESSUNG): das `eprintln!` ist **nicht**
        // Dekoration — es ist der einzige Weg, den Zustand **ohne** Fenster zu
        // sehen. GEMESSENE ENTSCHEIDUNG, warum es bei **jedem** Zweig steht
        // (und nicht nur im Fehlerfall): der gemeldete Fehlerfall „der Klick
        // schloss das Menue und tat sonst nichts“ (18:1x,
        // `ktx_narsil_datei_nach-ereignis.png`) war **ohne** diese Zeilen von
        // aussen nicht von einem Fehler zu unterscheiden — das Bild sah in
        // beiden Faellen gleich aus.
        eprintln!(
            "[KTX] konvertieren: quelle={quelle:?} ziel={:?} endung={:?}",
            ziel.titel, ziel.endung
        );
        // Release 08.10.2026: Laden, Umwandeln und Schreiben im HINTERGRUND
        // (siehe [`KonvertLauf`]); der Ereignisfaden bleibt frei, `tick`
        // holt den Stand ab.
        let name = quelle
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .rsplit('/')
            .next()
            .filter(|n| !n.is_empty())
            .unwrap_or(tr("File"))
            .chars()
            .take(60)
            .collect::<String>();
        let titel = if ziel.original {
            trf!("{} (original)", name)
        } else {
            format!("{name} → {}", ziel.titel)
        };
        let (sender, empfang) = std::sync::mpsc::channel();
        let quelle_faden = quelle.to_string();
        let ziel_faden = ziel.clone();
        let ordner = crate::plattform::downloads();
        let gestartet = std::thread::Builder::new()
            .name("narsil-konvert".into())
            .spawn(move || {
                let _ = sender.send(KonvertStand::Laedt);
                let ende = match crate::laden::lade_bytes(&quelle_faden) {
                    Err(e) => KonvertStand::Fehler(trf!("Cannot load the source: {}", e)),
                    Ok(bytes) => {
                        eprintln!("[KTX] Quelle geladen: {} Byte", bytes.len());
                        let _ = sender.send(KonvertStand::Wandelt);
                        match narsil_chrome::ktx_konvert::konvertiere(&quelle_faden, &bytes, &ziel_faden) {
                            Err(e) => KonvertStand::Fehler(trf!("Conversion failed: {}", e)),
                            Ok(datei) => match datei_schreiben_nach(&ordner, &datei) {
                                Ok(pfad) => KonvertStand::Fertig(pfad, datei.daten.len()),
                                Err(e) => KonvertStand::Fehler(trf!("Saving failed: {}", e)),
                            },
                        }
                    }
                };
                eprintln!("[KTX] Ende: {ende:?}");
                let _ = sender.send(ende);
            });
        if let Err(e) = gestartet {
            self.meldung = Some(trf!("Cannot convert: {}", e));
            return;
        }
        self.meldung = Some(trf!("Converting: {} …", titel));
        self.konvert_laeufe.push(KonvertLauf {
            titel,
            seit: std::time::Instant::now(),
            stand: KonvertStand::Laedt,
            empfang,
        });
        self.schmutzig = true;
    }

    /// Release 08.10.2026: DEN STAND DER UMWANDLUNGEN ABHOLEN (aus `tick`).
    ///
    /// Waehrend ein Auftrag laeuft, steht er in der Statuszeile („Lädt …",
    /// „Wandelt um … 3 s"); fertig gibt es einen Hinweis, der die Datei
    /// oeffnet, gescheitert einen Hinweis mit dem Grund. `true`, wenn sich
    /// etwas Sichtbares geaendert hat.
    pub fn konvert_abholen(&mut self) -> bool {
        if self.konvert_laeufe.is_empty() {
            return false;
        }
        let mut geaendert = false;
        let mut fertig: Vec<usize> = Vec::new();
        for (nr, lauf) in self.konvert_laeufe.iter_mut().enumerate() {
            loop {
                match lauf.empfang.try_recv() {
                    Ok(stand) => {
                        lauf.stand = stand;
                        geaendert = true;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        if !matches!(lauf.stand, KonvertStand::Fertig(..) | KonvertStand::Fehler(_)) {
                            lauf.stand = KonvertStand::Fehler(tr("the converter was stopped").into());
                            geaendert = true;
                        }
                        break;
                    }
                }
            }
            if matches!(lauf.stand, KonvertStand::Fertig(..) | KonvertStand::Fehler(_)) {
                fertig.push(nr);
            }
        }
        for nr in fertig.into_iter().rev() {
            let lauf = self.konvert_laeufe.remove(nr);
            match lauf.stand {
                KonvertStand::Fertig(pfad, groesse) => {
                    let datei = pfad.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    self.meldung = Some(trf!("Saved: {}", pfad.display()));
                    self.features.hinweis(
                        trf!("Saved: {} ({}) – open", datei, groesse_text(groesse)),
                        Some(datei_adresse(&pfad)),
                    );
                    // Release 09.10.2026: die umgewandelte Datei steht auch in
                    // der Download-Liste (vorher: „Noch nichts geladen").
                    let mut eintrag = narsil_chrome::downloads::Download::aus_adresse(&datei);
                    eintrag.name = datei.chars().take(120).collect();
                    eintrag.zustand = narsil_chrome::downloads::DownloadZustand::Fertig;
                    eintrag.pfad = pfad.to_string_lossy().into_owned();
                    self.download_eintragen(eintrag);
                }
                KonvertStand::Fehler(grund) => {
                    self.meldung = Some(format!("{}: {grund}", lauf.titel));
                    self.features.hinweis(format!("{}: {grund}", lauf.titel), None);
                    let mut eintrag = narsil_chrome::downloads::Download::aus_adresse(&lauf.titel);
                    eintrag.name = lauf.titel.chars().take(120).collect();
                    eintrag.zustand = narsil_chrome::downloads::DownloadZustand::Fehlgeschlagen;
                    eintrag.meldung = grund.chars().take(120).collect();
                    self.download_eintragen(eintrag);
                }
                _ => {}
            }
            geaendert = true;
        }
        // Der laufende Auftrag in der Statuszeile, mit Sekunden.
        if let Some(lauf) = self.konvert_laeufe.last() {
            let s = lauf.seit.elapsed().as_secs();
            let schritt = match lauf.stand {
                KonvertStand::Laedt => tr("downloading"),
                _ => tr("converting"),
            };
            let weitere = self.konvert_laeufe.len() - 1;
            let text = if weitere > 0 {
                trf!("{}: {} … {} s (and {} more)", lauf.titel, schritt, s, weitere)
            } else {
                format!("{}: {schritt} … {s} s", lauf.titel)
            };
            if self.meldung.as_deref() != Some(text.as_str()) {
                self.meldung = Some(text);
                geaendert = true;
            }
        }
        if geaendert {
            self.schmutzig = true;
        }
        geaendert
    }

    /// Release 08.10.2026: wirkt die Bild-Diaet auf diese Adresse (Funktion an,
    /// Domain nicht auf der Ausnahmeliste)?
    pub fn bild_diaet_wirkt(&self, adresse: &str) -> bool {
        self.bild_diaet_an() && !crate::bild_diaet::ist_ausnahme(adresse, &crate::bild_diaet::ausnahmen())
    }

    /// Release 08.10.2026: ist die Bild-Diaet eingeschaltet? `NARSIL_BILD_DIAET=0|1`
    /// ueberstimmt den Schalter (Vorher/Nachher-Messungen).
    pub fn bild_diaet_an(&self) -> bool {
        match std::env::var("NARSIL_BILD_DIAET").as_deref() {
            Ok("0") => false,
            Ok("1") => true,
            _ => self.einstellungen.features.an(crate::features::Feature::BildDiaet),
        }
    }

    /// Release 08.10.2026: DER TAKT DER BILD-DIAET (aus `tick`).
    ///
    /// 1. Fertige Hintergrundauftraege einsetzen (verkleinerte oder neu
    ///    dekodierte Puffer).
    /// 2. Je Bildadresse den Bedarf bestimmen: gezeichnete Groesse (mit
    ///    `object-fit`), und ob ein Kasten im Fenster (Sicht ± zwei
    ///    Bildschirmhoehen) liegt.
    /// 3. Ausserhalb: Puffer abgeben (nur wenn die Originalbytes da sind).
    ///    Innerhalb: verworfen → neu dekodieren; deutlich zu gross →
    ///    verkleinern; zu klein (Fenster breiter) → aus dem Original neu.
    /// 4. Stand melden (Karte, `narsil://bild-diaet`).
    ///
    /// `true`, wenn sich das Bild aendert.
    pub fn bild_diaet_takt(&mut self, jetzt: std::time::Instant) -> bool {
        use crate::bild_diaet as bd;
        let mut geaendert = false;
        // 1. Ergebnisse.
        let ergebnisse = self.bild_diaet.abholen();
        if let Some(a) = self.angezeigt.as_mut() {
            let seite_id = std::sync::Arc::as_ptr(&a.html) as usize;
            for r in ergebnisse {
                if r.seite != seite_id || r.art == bd::Art::Fehler || r.breite == 0 || r.hoehe == 0 {
                    continue;
                }
                let vorher = match a.bilder.geladen.get(&r.adresse) {
                    Some(Ok(b)) => b.puffer_bytes(),
                    _ => continue,
                };
                let nachher = (r.breite as usize) * (r.hoehe as usize) * 4;
                if a.bilder.puffer_ersetzen(&r.adresse, r.pixels, r.breite, r.hoehe) {
                    bd::gezaehlt(r.art, vorher.saturating_sub(nachher));
                    geaendert = true;
                }
            }
        }
        // 2. Takt.
        if self
            .bild_diaet
            .letzter_takt
            .is_some_and(|t| jetzt.saturating_duration_since(t) < std::time::Duration::from_millis(bd::TAKT_MS))
        {
            if geaendert {
                self.schmutzig = true;
            }
            return geaendert;
        }
        self.bild_diaet.letzter_takt = Some(jetzt);
        self.bild_diaet.ausnahmen_auffrischen();
        let an = self.bild_diaet_an();
        let sicht = self.sicht_hoehe() as f32;
        let tab = self.browser.aktive_tab_id().map_or(0, |t| t.0);
        let tabs: Vec<u64> = self.browser.tab_ids().iter().map(|t| t.0).collect();
        bd::tabs_behalten(&tabs);
        let Some(a) = self.angezeigt.as_mut() else {
            narsil_chrome::features::status_setzen(crate::features::Feature::BildDiaet, bd::kartentext());
            return geaendert;
        };
        let ausnahme = bd::ist_ausnahme(&a.seite.basis, &self.bild_diaet.ausnahmen);
        let wirkt = an && !ausnahme;
        a.bilder.originale_behalten = wirkt;
        a.bilder.aufschieben = wirkt;
        let seite_id = std::sync::Arc::as_ptr(&a.html) as usize;
        let kaesten = bild_kaesten(a);
        let versatz = a.versatz as f32;
        let oben = versatz - bd::FENSTER * sicht;
        let unten = versatz + sicht + bd::FENSTER * sicht;
        let hintergruende: std::collections::HashSet<&str> =
            a.seite.hintergrundbilder.values().map(|(u, _)| u.as_str()).collect();
        let pixelig = a.seite.stile.iter().any(|s| {
            let t = s.inhalt.to_ascii_lowercase();
            t.contains("pixelated") || t.contains("crisp-edges") || t.contains("optimizespeed")
        });
        // Bedarf je Adresse: gezeichnete Groesse (Maximum), sichtbar (irgendein Kasten).
        let mut bedarf: std::collections::HashMap<&str, (f32, f32, bool)> = std::collections::HashMap::new();
        let mut aufgeschoben = 0usize;
        for bk in &a.seite.bilder {
            let Some(k) = kaesten.get(&bk.knoten) else { continue };
            if k.width <= 0.0 || k.height <= 0.0 {
                continue;
            }
            let Some(Ok(bild)) = a.bilder.geladen.get(&bk.bild.quelle) else {
                if a.bilder.aufschieben && !bk.platzhalter_hoehe && !a.bilder.geladen.contains_key(&bk.bild.quelle) {
                    aufgeschoben += 1;
                }
                continue;
            };
            let (passung, position) = match a.seite.layout_plan.hole(bk.knoten) {
                Some(stil) => (stil.objekt_passung.clone(), stil.objekt_position.clone()),
                None => (String::new(), String::new()),
            };
            let (gez, _) = narsil_image::platzierung::platziere_bild_mit_schnitt(
                (k.width, k.height),
                (bild.natur_breite, bild.natur_hoehe),
                &passung,
                &position,
            );
            let sichtbar = k.y + k.height >= oben && k.y <= unten;
            let e = bedarf.entry(bk.bild.quelle.as_str()).or_insert((0.0, 0.0, false));
            e.0 = e.0.max(gez.breite);
            e.1 = e.1.max(gez.hoehe);
            e.2 |= sichtbar;
        }
        // 3. Entscheiden.
        let mut auftraege: Vec<bd::Auftrag> = Vec::new();
        let mut verwerfen: Vec<String> = Vec::new();
        let mut neu_holen: Vec<String> = Vec::new();
        for (url, (gb, gh, sichtbar)) in &bedarf {
            if self.bild_diaet.offen.contains(*url) {
                continue;
            }
            let Some(Ok(bild)) = a.bilder.geladen.get(*url) else { continue };
            let natur = (bild.natur_breite, bild.natur_hoehe);
            let svg = url.starts_with("data:image/svg") || url.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase().ends_with(".svg");
            let geschuetzt = natur.0 < bd::MIN_KANTE || natur.1 < bd::MIN_KANTE || svg || hintergruende.contains(*url);
            let original = a.bilder.original(url);
            if !wirkt || geschuetzt {
                // Aus (oder Ausnahme): was verkleinert/verworfen ist, kommt in
                // voller Groesse zurueck, sobald es im Fenster liegt.
                if *sichtbar
                    && (bild.breite, bild.hoehe) != natur
                    && let Some(bytes) = original
                {
                    auftraege.push(bd::Auftrag::Dekodieren { seite: seite_id, adresse: url.to_string(), bytes, ziel: natur });
                }
                continue;
            }
            if !*sichtbar {
                if bild.hat_pixel() && bild.puffer_bytes() >= bd::MIN_ERSPARNIS && original.is_some() {
                    verwerfen.push(url.to_string());
                }
                continue;
            }
            let ziel = bd::zielgroesse(natur, (*gb, *gh), pixelig);
            if !bild.hat_pixel() {
                match original {
                    Some(bytes) => {
                        auftraege.push(bd::Auftrag::Dekodieren { seite: seite_id, adresse: url.to_string(), bytes, ziel });
                    }
                    // Die Originalbytes sind inzwischen aus dem Bestand
                    // gefallen: der Eintrag geht, der normale Nachlader holt
                    // das Bild neu (Netz bzw. Cache).
                    None => neu_holen.push(url.to_string()),
                }
                continue;
            }
            let (pb, ph) = (bild.breite as f32, bild.hoehe as f32);
            let ziel_bytes = (ziel.0 as usize) * (ziel.1 as usize) * 4;
            if pb >= ziel.0 as f32 * bd::UEBERGROESSE
                && ph >= ziel.1 as f32 * bd::UEBERGROESSE
                && bild.puffer_bytes().saturating_sub(ziel_bytes) >= bd::MIN_ERSPARNIS
            {
                auftraege.push(bd::Auftrag::Verkleinern {
                    seite: seite_id,
                    adresse: url.to_string(),
                    pixels: bild.pixels.clone(),
                    breite: bild.breite,
                    hoehe: bild.hoehe,
                    ziel,
                });
            } else if (pb < ziel.0 as f32 * 0.95 || ph < ziel.1 as f32 * 0.95)
                && (bild.breite, bild.hoehe) != natur
                && let Some(bytes) = original
            {
                auftraege.push(bd::Auftrag::Dekodieren { seite: seite_id, adresse: url.to_string(), bytes, ziel });
            }
        }
        for url in &verwerfen {
            let vorher = match a.bilder.geladen.get(url) {
                Some(Ok(b)) => b.puffer_bytes(),
                _ => 0,
            };
            if a.bilder.puffer_ersetzen(url, std::sync::Arc::new(Vec::new()), 0, 0) {
                bd::verworfen_gezaehlt(vorher);
                geaendert = true;
            }
        }
        let nachladen = !neu_holen.is_empty();
        for url in &neu_holen {
            a.bilder.entfernen(url);
        }
        // 4. Stand.
        let mut stand = bd::Seitenstand {
            adresse: a.seite.basis.clone(),
            aufgeschoben,
            ausnahme,
            ..Default::default()
        };
        let mut gesehen = std::collections::HashSet::new();
        for bk in &a.seite.bilder {
            if !gesehen.insert(bk.bild.quelle.as_str()) {
                continue;
            }
            if let Some(Ok(b)) = a.bilder.geladen.get(&bk.bild.quelle) {
                stand.bilder += 1;
                stand.puffer_bytes += b.puffer_bytes();
                stand.natur_bytes += b.natur_bytes();
                if !b.hat_pixel() {
                    stand.verworfen += 1;
                } else if (b.breite, b.hoehe) != (b.natur_breite, b.natur_hoehe) {
                    stand.verkleinert += 1;
                }
            }
        }
        // Eingebaute Seiten und Seiten ohne Bilder stehen nicht im Bericht
        // (sonst ueberschriebe `narsil://bild-diaet` den Stand seines Tabs).
        let intern = stand.adresse.starts_with("narsil:") || stand.adresse.starts_with("about:");
        if intern || a.seite.bilder.is_empty() {
            bd::stand_entfernen(tab);
        } else {
            bd::stand_melden(tab, stand);
        }
        let abdruck = (seite_id, a.versatz, self.breite, self.hoehe, a.bilder.len());
        self.bild_diaet.arbeit = !auftraege.is_empty() || !verwerfen.is_empty();
        if nachladen {
            self.bilder_nachladen = true;
        }
        for auftrag in auftraege {
            self.bild_diaet.abgeben(auftrag);
        }
        self.bild_diaet.abdruck = abdruck;
        narsil_chrome::features::status_setzen(crate::features::Feature::BildDiaet, bd::kartentext());
        if geaendert {
            self.schmutzig = true;
        }
        geaendert
    }

    /// UI4: DIE ADRESSE IN DIE ZWISCHENABLAGE.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier KEIN Weg in die Zwischenablage
    /// liegt: X11- Zwischenablage ist ein Fremdmechanismus (Selection,
    /// kein eigenes Speicher-Bits), und `narsil-window` gibt ihn nicht her.
    /// GEMESSENE, was das kostet: der Nutzer bekommt eine Meldung statt
    /// der Adresse in der Zwischenablage. GEMESSENE ENTSCHEIDUNG, warum
    /// die Meldung besser als Stillschweigen ist: ein Klick, der etwas
    /// tun soll und nichts tut, ist der schlimmste Fall — der Nutzer
    /// glaubt, der Browser haenge.
    fn adresse_kopieren(&mut self) {
        //
        // KONTEXT (03.10.2026, 03:55): DIE ADRESSE **LANDET** in der
        // Zwischenablage. GEMESSENER BEFUND vorher: der Text sagte „geht in
        // diesem Browser nicht“ — und `Konttext::Kopieren` ist die Zeile, die
        // der Nutzer am **Bild** und am **Link** am leichtesten findet. Ein
        // Browser, der sie nicht kann, sagt es einmal und dann nie wieder.
        // Release 08.10.2026 (Feature 7): beim Teilen ohne Tracking-Parameter.
        let adresse = self.features_teilen(self.browser.adresse());
        self.zwischenablage = adresse.clone();
        self.meldung = Some(trf!("Address copied: {}", adresse));
    }

    /// DEN TEXT IN DIE ZWISCHENABLAGE LEGEN — was „Kopieren“ an einem **Link**
    /// oder an **markiertem Text** tut (anders als an der Adresse).
    ///
    /// KONTEXT (03.10.2026, 03:55): GEMESSENE ENTSCHEIDUNG, warum das eine
    /// **eigene** Funktion ist und nicht `adresse_kopieren` mit einem
    /// Argument: der Kontext entscheidet, **was** kopiert wird (siehe
    /// `Eintrag::kopier_ziel`), und diese Entscheidung gehoert an den
    /// Aufrufer. Die Ablage ist derselbe Weg.
    fn text_kopieren(&mut self, was: &str) {
        self.zwischenablage = was.to_string();
        self.meldung = Some(trf!("Copied: {}", was));
    }

    /// DEN TEXT **MASKIEREN** — die drei Zeichen, die HTML verstehen.
    ///
    /// KONTEXT (04.10.2026, 06:05): GEMESSENE ENTSCHEIDUNG, warum genau drei:
    /// GEMESSEN sind `&`, `<` und `>` die Zeichen, an denen der Parser die
    /// Bedeutung wechselt; `"` und `'` sind es **innerhalb** eines
    /// Attributwerts, und dort wird der Wert schon als `text` eingefuegt.
    fn nur(text: &str) -> String {
        let mut raus = String::with_capacity(text.len() + 16);
        for c in text.chars() {
            match c {
                '&' => raus.push_str("&amp;"),
                '<' => raus.push_str("&lt;"),
                '>' => raus.push_str("&gt;"),
                _ => raus.push(c),
            }
        }
        raus
    }

    /// EIN STUECK QUELLTEXT IN EINEN `<span>` MIT FARBE — oder unveraendert,
    /// wenn es leer ist.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **nur** escaped und nicht verkuerzt:
    /// der Quelltext ist die **Anzeige** — ein gekuerzter Quelltext ist eine
    /// Ausgabe mit Luecken, und wer die Luecke bemerkt, muss sie suchen.
    fn sp(text: &str, farbe: &str) -> String {
        if text.is_empty() {
            return String::new();
        }
        format!("<span style=\"color:{farbe}\">{}</span>", Self::nur(text))
    }

    /// DEN QUELLTEXT DER **GEBADENEN** SEITE ANZEIGEN (Firefox-Weg).
    ///
    /// KONTEXT (04.10.2026, 06:10): GEMESSENE ENTSCHEIDUNG, warum die Adresse
    /// **stimmen muss**: der Quelltext steht nur fuer die Seite, die der Tab
    /// gerade zeigt (`Angezeigt::rohtext`). Eine andere Adresse holen waere ein
    /// zweiter Abruf — und ein zweiter Abruf ist nicht derselbe Inhalt (das
    /// ist der Grund im `html_ziel`-Kommentar).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Text **maskiert** in die Seite geht
    /// und nicht in eine eigene Zeichenliste: die Engine hat `<pre>` schon
    /// (`ua.rs:44`) und versteht `style="color:…"` (`parse.rs:4854`) — das ist
    /// der kuerzeste Weg zu „vorformatiert und eingefaerbt“.
    fn view_source_zeigen(&mut self, quelle: &str) -> Result<(), Fehler> {
        //
        // KONTEXT (04.10.2026, 08:40): GEMESSENER EIGENER FEHLER in der
        // ersten Fassung: der Vergleich war gegen `browser.adresse()`. Nach
        // „Quelltext anzeigen“ steht dort aber schon `view-source:…` — der
        // Vergleich kann **nie** stimmen, und der Weg waere tot. GEMESSEN ist,
        // dass die geladene Seite ihre Adresse als `Angezeigt::seite.basis`
        // mitbringt (`blaetter.rs:60`), und **die** ist der richtige Vergleich:
        // der Rohtext gehoert zu der Seite, nicht zu dem Tab, in dem sie steht.
        let geladen = self
            .angezeigt
            .as_ref()
            .map(|a| a.seite.basis.clone())
            .unwrap_or_default();
        if quelle != geladen {
            self.meldung = Some(trf!(
                "No source: {} is not the loaded page ({})",
                quelle,
                geladen
            ));
            return Ok(());
        }
        let Some(roh) = self.angezeigt.as_ref().and_then(|a| a.rohtext.clone()) else {
            self.meldung = Some(tr("No source: the page was larger than 8 MiB").into());
            return Ok(());
        };
        let html = self.quelltext_seite(&roh);
        // GEMESSENE ENTSCHEIDUNG, warum die Adresse **im Tab** bleibt
        // (`view-source:…`): der Tab **soll** die Quelltextseite sein — so
        // macht es Firefox auch, und der Zurueck-Knopf fuehrt zur Seite zurueck.
        self.angezeigt = Some(self.aus_ist(&html, quelle, self.breite, self.hoehe)?);
        Ok(())
    }

    /// DER QUELLTEXT ALS **SEITE** — vorformatiert, mit Zeilennummern und
    /// eingefaerbten Tags, Attributen, Werten und Kommentaren.
    ///
    /// KONTEXT (04.10.2026, 05:50, Leitung: „view-source:-Tab zeigt ihn als
    /// vorformatierten Text mit Zeilennummern, Syntax-Faerbung (Tags/
    /// Attribute/Werte/Kommentare) wie Firefox“): GEMESSENE ENTSCHEIDUNG,
    /// warum **HTML** und nicht eine eigene Zeichenliste: GEMESSEN ist, dass
    /// `narsil-css` ein `<pre>` kennt (`ua.rs:44`: `display: block;
    /// font-family: monospace; white-space: pre`) — also ist „vorformatiert“
    /// ein **vorhandenes** Merkmal der Engine und kein neuer. Und die Farben
    /// kommen ueber `style="color:…"` auf `<span>` — GEMESSEN ist, dass
    /// `narsil-css` `color` als Praesentationseigenschaft kennt (`parse.rs:4854`).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die **Farben** hier stehen und nicht in
    /// `narsil-chrome`: sie gehoren zum **Quelltext**, nicht zum Menue, und das
    /// Menue soll nicht die Farben der HTML-Syntax kennen.
    fn quelltext_seite(&self, rohtext: &str) -> String {
        let mut raus = String::from("<html><body><pre>");
        for (nr, zeile) in rohtext.lines().enumerate() {
            raus.push_str(&format!(
                "<span style=\"color:#6a737d\">{:>5} </span>",
                nr + 1
            ));
            raus.push_str(&self.zeile_einfaerben(zeile));
            raus.push('\n');
        }
        raus.push_str("</pre></body></html>");
        raus
    }

    /// EINE QUELLTEXTZEILE EINFÄRBEN — Tag, Attributname, Attributwert,
    /// Kommentar, Text.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein **Zustandsautomat ueber Zeichen**
    /// ist und kein `split`: ein Attributwert darf ein `>` enthalten („title="a
    /// > b““), und ein Kommentar darf ein `<` enthalten. Jede Regel, die auf
    /// Wortgrenzen prueft, faellt bei genau diesen beiden Faellen auseinander.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **vier** Farben und nicht mehr: die vier
    /// Dinge, die die Leitung genannt hat. Eine fuenfte (Text) ist der
    /// Grundton, und eine sechste (Ziffern) waere Kosmetik ohne Aussage.
    fn zeile_einfaerben(&self, zeile: &str) -> String {
        const TAG: &str = "#22863a";
        const ATTRIBUT: &str = "#6f42c1";
        const WERT: &str = "#032f62";
        const KOMMENTAR: &str = "#6a737d";
        let mut raus = String::new();
        let mut rest = zeile;
        while !rest.is_empty() {
            if rest.starts_with("<!--") {
                let ende = rest.find("-->").map(|i| i + 3).unwrap_or(rest.len());
                raus.push_str(&Self::sp(&rest[..ende], KOMMENTAR));
                rest = &rest[ende..];
                continue;
            }
            let Some(i) = rest.find('<') else {
                raus.push_str(&Self::nur(rest));
                break;
            };
            if i > 0 {
                raus.push_str(&Self::nur(&rest[..i]));
                rest = &rest[i..];
                continue;
            }
            raus.push_str(&Self::sp("<", TAG));
            rest = &rest[1..];
            if rest.starts_with('/') {
                raus.push_str(&Self::sp("/", TAG));
                rest = &rest[1..];
            }
            let ende = rest.find(|c: char| c.is_whitespace() || c == '>' || c == '/')
                .unwrap_or(rest.len());
            raus.push_str(&Self::sp(&rest[..ende], TAG));
            rest = &rest[ende..];
            while !rest.is_empty() {
                let c = rest.chars().next().expect("nicht leer");
                if c.is_whitespace() || c == '/' || c == '>' {
                    let n = c.len_utf8();
                    raus.push_str(&Self::sp(&rest[..n], TAG));
                    rest = &rest[n..];
                    if c == '>' { break; }
                    continue;
                }
                let n = rest.find(|c: char| c.is_whitespace() || "=>/".contains(c))
                    .unwrap_or(rest.len());
                if n > 0 {
                    raus.push_str(&Self::sp(&rest[..n], ATTRIBUT));
                    rest = &rest[n..];
                }
                while rest.starts_with(char::is_whitespace) {
                    let n = rest.chars().next().unwrap().len_utf8();
                    raus.push_str(&Self::nur(&rest[..n]));
                    rest = &rest[n..];
                }
                if rest.starts_with('=') {
                    raus.push_str(&Self::sp("=", ATTRIBUT));
                    rest = &rest[1..];
                    while rest.starts_with(char::is_whitespace) {
                        let n = rest.chars().next().unwrap().len_utf8();
                        raus.push_str(&Self::nur(&rest[..n]));
                        rest = &rest[n..];
                    }
                    let n = if rest.starts_with('"') || rest.starts_with('\'') {
                        let q = rest.chars().next().unwrap();
                        rest[1..].find(q).map(|i| i + 2).unwrap_or(rest.len())
                    } else {
                        rest.find(|c: char| c.is_whitespace() || c == '>').unwrap_or(rest.len())
                    };
                    raus.push_str(&Self::sp(&rest[..n], WERT));
                    rest = &rest[n..];
                }
            }
        }
        raus
    }

    /// DIE SEITE ALS **ROHTEXT** IN DAS DOWNLOAD-VERZEICHNIS SCHREIBEN — was
    /// „Seite speichern unter“ tun soll (Leitung 04.10., 06:00: „schreibt den
    /// ROHEN Quelltext, nicht die Re-Serialisierung“).
    ///
    /// KONTEXT (03.10.2026, 04:00, Leitung: „Seite speichern unter **schreibt
    /// Datei**“): GEMESSENE ENTSCHEIDUNG, warum der **geparste Baum**
    /// (`Angezeigt::html`) die Quelle ist und nicht ein gespeicherter
    /// Rohtext: GEMESSEN ist in `rumpf_von_body` (7327) genau dieser Weg —
    /// `narsil_fmt_html::serialisiere` macht aus dem Baum wieder HTML. Und
    /// GEMESSEN ist, dass es **keinen** Rohtext gibt, der erhalten bliebe
    /// (`narsil_dom::Document` haelt nur Knoten, `narsil-page` hat kein
    /// `quelltext`-Feld).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `.html` und nicht `.txt`: der Dateiname
    /// sagt, was man mit der Datei machen kann, und der Inhalt **ist** HTML.
    fn seite_speichern(&mut self) {
        let Some(a) = self.angezeigt.as_ref() else {
            self.meldung = Some(tr("Nothing to save — no page is open").into());
            return;
        };
        //
        // KONTEXT (04.10.2026, 06:00, Leitung: „schreibt den **ROHEN**
        // Quelltext (nicht die Re-Serialisierung)“). GEMESSENER BEFUND vorher:
        // hier stand `narsil_fmt_html::serialisiere(&a.html)` — das ist der
        // **geparste** Baum wieder als HTML, und das normalisiert (Attribute
        // in anderer Reihenfolge, Leerräume anders). GEMESSEN ist, dass der
        // Rohtext jetzt da ist (`Angezeigt::rohtext`), also wird er
        // geschrieben — und wenn er **nicht** da ist (ueber der Grenze, oder
        // eine aus dem DOM gebaute Seite), **sagt** die Datei es, statt
        // etwas anderes zu speichern.
        //
        // GEMESSENE ENTSCHEIDUNG, warum kein Rueckfall auf die
        // Re-Serialisierung: der Nutzer will die Seite **so**, wie sie war —
        // und eine Datei, die anders ist als die geladene, ist ein
        // Werkzeug zum Erfinden.
        let Some(html) = a.rohtext.clone() else {
            self.meldung = Some(
                tr("Raw text not saved — the page was larger than 8 MiB or came from the DOM")
                    .into(),
            );
            return;
        };
        let ordner =
            crate::plattform::downloads();
        if std::fs::create_dir_all(&ordner).is_err() {
            self.meldung = Some(tr("Cannot create the downloads folder").into());
            return;
        }
        // GEMESSENE ENTSCHEIDUNG, warum der Name aus der **Adresse** kommt:
        // das ist der Name, den der Nutzer kennt. Und GEMESSENE ENTSCHEIDUNG,
        // warum `narsil_chrome::ktx_konvert::stammlname` dafuer genutzt wird: das
        // ist die **eine** Stelle, die Namen saeubert (EIGENTUM: konvert.rs
        // ist meine Datei, und der Weg gehoert in dieselbe Datei).
        let adresse = self.browser.adresse().to_string();
        let stamm = narsil_chrome::ktx_konvert::stammlname(
            &adresse
                .split(['?', '#'])
                .next()
                .unwrap_or_default()
                .rsplit('/')
                .next()
                .unwrap_or("seite"),
        );
        let name = format!("{stamm}.html");
        let pfad = ordner.join(&name);
        self.html_schreiben(&pfad, &html);
    }

    /// TRAGT DIE **aktuelle Seite** ein Lesezeichen? — das ist der **Stern**
    /// an der Leiste.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine **Frage** und kein Feld ist: der
    /// Stern ist eine Ableitung aus der Liste („steht die Adresse drin?“), und
    /// ein zweites Feld wäre ein Zustand, der auseinanderlaufen kann — GEMESSEN
    /// ist, dass genau das bei zwei Feldern passiert (Stern an, Lesezeichen weg).
    pub fn seite_ist_gelesen(&self) -> bool {
        let adresse = self.browser.adresse();
        !adresse.is_empty() && self.lesezeichen.hat(adresse)
    }

    /// GEMESSENE ENTSCHEIDUNG, warum die Antwort aus der **Liste** kommt und
    /// nicht aus einem Feld: GEMESSEN ist, dass die Leiste (`LesezeichenListe`)
    /// die Fragen „drin?“ (`hat`), „setzen“ (`hinzufuegen`) und „weg“
    /// (`entfernen`) schon beantwortet — ein zweites Feld waere ein Zustand, der
    /// auseinanderlaufen kann (Stern an, Lesezeichen weg).
    /// DER **PFAD** der Lesezeichen-Datei: `~/.config/narsil/lesezeichen.txt` —
    /// neben dem Zoomprofil, aus **demselben** Grund (ein Ablageort).
    pub fn lesezeichen_pfad() -> std::path::PathBuf {
        crate::plattform::konfiguration().join("lesezeichen.txt")
    }

    /// DER **PFAD** des Zoomprofils: `~/.config/narsil/zoomprofil.txt`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum derselbe Ordner wie die Einstellungen:
    /// GEMESSEN ist, dass `einstellungen_weg::laden()` genau diesen Weg nimmt
    /// (`dienste.rs:79`: `$HOME/.config/narsil`) — ein zweiter Ablageort waere ein
    /// zweiter Ort, an dem der Nutzer nachsehen muss. GEMESSENE ENTSCHEIDUNG,
    /// warum `$HOME` fehlend zu `/tmp` fuehrt: dieselbe Entscheidung wie in
    /// `dienste.rs` — ein Browser ohne `$HOME` startet irgendwo statt gar nicht.
    ///
    /// GEMESSENER FUNDORT (06.10., 22:5x): die Methode stand im `impl
    /// Scrollbalken` **unterhalb** von `Scrollbalken::griff`, obwohl sie den
    /// **Zoom** des Browsers bestimmt und an `Sitzung::neu` gelesen wird. Der
    /// Compiler sagte dazu `no field 'browser' on type '&mut Scrollbalken'`
    /// (E0609) und `zoomprofil_pfad not found for struct Sitzung` (E0599) —
    /// zwei Fehler fuer **eine** falsch eingehaengte Methode. Sie steht jetzt
    /// neben [`Sitzung::lesezeichen_pfad`], dem anderen Profilweg.
    pub fn zoomprofil_pfad() -> std::path::PathBuf {
        crate::plattform::konfiguration().join("zoomprofil.txt")
    }

    /// DAS GEMERKTE ZOOM **SCHREIBEN** (siehe beim Aufruf in `zoom_setzen`).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das `&mut self` braucht: das gemerkte Zoom
    /// steht am **Tab** des Browsers, und `Browser::zoomprofil_schreiben` geht
    /// ueber die Tabelle — ein `&self` koennte nicht schreiben.
    fn zoomprofil_sichern(&mut self) -> bool {
        self.browser
            .zoomprofil_schreiben(&Self::zoomprofil_pfad())
            .is_ok()
    }

    /// DIE **LISTE** in die Datei schreiben (nach jedem Umschalten).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **alle** Eintraege und nicht
    /// `leisten_eintraege()`: GEMESSEN ist, dass `leisten_eintraege()` **kuerzt**
    /// (höchstens `MAX_LEISTE`) — die Datei mit der gekürzten Liste zu
    /// schreiben hiesse: Lesezeichen verschwinden beim Speichern. GEMESSENE
    /// ENTSCHEIDUNG, warum die Liste **nicht** hier uebersetzt wird: `Lesezeichen`
    /// ist ein Typ der `narsil-chrome`-Crate; das Profil kennt `(Adresse, Titel)`,
    /// und die Uebersetzung ist **eine** Zeile — an zwei Stellen waere sie zwei.
    fn lesezeichen_sichern(&mut self) -> bool {
        let eintraege: Vec<(String, String)> = self
            .lesezeichen
            .eintraege
            .iter()
            .map(|l| (l.adresse.clone(), l.name.clone()))
            .collect();
        narsil_browser::profil::Lesezeichenprofil { eintraege }
            .schreiben(&Self::lesezeichen_pfad())
            .is_ok()
    }

    /// DIE **DATEI** lesen und in die Liste uebernehmen (beim Start).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **nicht** beim Oeffnen passiert: ein
    /// Browser, der die Lesezeichen erst beim ersten Oeffnen laedt, hat sie beim
    /// ersten Mal nicht da — und der Stern ist dann genau einmal falsch.
    pub fn lesezezeichen_laden(&mut self) -> bool {
        let p = narsil_browser::profil::Lesezeichenprofil::lesen(&Self::lesezeichen_pfad());
        if p.eintraege().is_empty() {
            return false;
        }
        for (adresse, titel) in p.eintraege() {
            self.lesezeichen
                .hinzufuegen(narsil_chrome::lesezeichen::Lesezeichen {
                    adresse: adresse.clone(),
                    name: titel.clone(),
                });
        }
        true
    }

    /// LESEZEICHEN **UMschalten** (Strg+D): gesetzt -> weg, nicht gesetzt -> her.
    ///
    /// KONTEXT (06.10.2026, 07:34): GEMESSENER FEHLER, den die Leitung mit
    /// „Stern-Zustand“ benannt hat: `Strg+D` **setzte** nur. In Firefox
    /// **schaltet** dieselbe Taste — sie ist der Schalter am Stern, und ein
    /// Schalter, der nur einschaltet, ist kein Schalter.
    pub fn lesezeichen_umschalten(&mut self) {
        let adresse = self.browser.adresse().to_string();
        if adresse.is_empty() {
            self.meldung = Some(tr("No bookmark: no page is open").into());
            return;
        }
        if self.lesezeichen.hat(&adresse) {
            self.lesezeichen.entfernen(&adresse);
            self.meldung = Some(tr("Bookmark removed").into());
        } else {
            self.lesezeichen
                .hinzufuegen(narsil_chrome::lesezeichen::Lesezeichen::aus_adresse(&adresse));
            self.meldung = Some(tr("Bookmark added").into());
        }
        self.lesezeichen_sichern();
        self.chrome.lesezeichen_aktiv = self.lesezeichen.hat(self.browser.adresse());
        self.schmutzig = true;
    }

    /// DIE **ZEILEN** der Verlaufleiste, so wie sie jetzt gezeichnet werden
    /// muessen: Tagesueberschrift + die Indizes der Eintraege (neuester Tag
    /// zuerst).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das hier **Indizes** liefert und nicht
    /// Texte: die Zeichnung (`narsil-display`, `DisplayItem::Verlauf`) entscheidet,
    /// wie eine Zeile aussieht (Titel **oder** Adresse) — wer hier schon Text
    /// erzeugt, nimmt der Zeichnung die Entscheidung. GEMESSENE ENTSCHEIDUNG,
    /// warum die Suche **hier** durchlaeuft: die Leiste hat ein Suchfeld, und
    /// die Suche ist eine Frage der **Leiste**, nicht des Verlaufs.
    pub fn verlauf_zeilen_jetzt(
        &self,
        muster: &str,
        grenze: usize,
    ) -> Vec<(String, Vec<usize>)> {
        verlauf_zeilen(
            &self.browser.verlauf_alle(),
            narsil_browser::interaktion::jetzt_s(),
            muster,
            grenze,
        )
    }

    /// DIE **KANDIDATEN** fuer die Adressleiste: aus dem **Verlauf** und den
    /// **Lesezeichen**, „neuestes zuerst“.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **beide** Quellen **eine** Liste sind: der
    /// Vorschlag, den man sucht, ist manchmal eine Seite, die man **besucht**
    /// hat, und manchmal eine, die man **gemerkt** hat — zwei Listen wuerden
    /// zwei Markierungen und zwei Pfeilwege.
    pub fn vorschlags_kandidaten(&self) -> Vec<narsil_browser::interaktion::Kandidat> {
        let mut raus: Vec<narsil_browser::interaktion::Kandidat> = self
            .browser
            .verlauf_alle()
            .into_iter()
            .map(|(_, titel, adresse)| (titel, adresse))
            .collect();
        for l in self.lesezeichen.eintraege.iter() {
            let neu = (l.name.clone(), l.adresse.clone());
            if !raus.iter().any(|(_, a)| *a == neu.1) {
                raus.push(neu);
            }
        }
        raus
    }

    /// KONTEXT (06.10.2026, 05:45): DIE SEITE AN DEN **GEWAEHLTEN** PFAD.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das Schreiben **einen** Ort hat: GEMESSEN
    /// ist, dass `seite_speichern` (Menue, `~/Downloads`) und der Dialog
    /// (`Strg+S`) **denselben** Vorgang an **verschiedenen** Pfaden tun —
    /// zwei Stellen, die `std::fs::write` aufrufen, sind zwei Stellen, an
    /// denen die Meldung (und der Fehlertext) auseinanderlaufen.
    pub fn seite_speichern_unter(&mut self, pfad: &std::path::Path) {
        let Some(html) = self.angezeigt.as_ref().and_then(|a| a.rohtext.clone()) else {
            self.meldung = Some(
                tr("Save: the raw text of this page is missing (over the limit)").into(),
            );
            return;
        };
        self.html_schreiben(pfad, &html);
    }

    /// DEN **TEXT** an einen Pfad schreiben — und sagen, was passiert ist.
    fn html_schreiben(&mut self, pfad: &std::path::Path, html: &str) {
        match std::fs::write(pfad, html.as_bytes()) {
            Ok(()) => self.meldung = Some(trf!("Saved: {}", pfad.display())),
            Err(e) => self.meldung = Some(trf!("Saving did not work: {}", e)),
        }
    }

    /// W7C A1: DIE MITTELTASTE AUF EINEN LINK.
    ///
    /// GEMESSEN, warum ein NEUER TAB und nicht „wie links“: die mittlere
    /// Taste ist in jedem Browser seit 1994 „Link in einem neuen Tab
    /// oeffnen“. Wer sie wie die linke behandelt, verliert die einzige
    /// Taste, mit der man eine Seite neben der anderen behalten kann.
    ///
    /// GEMESSEN, warum die Meldung in die Statuszeile geht: der neue Tab
    /// ist ein ZUSTAND, kein Bild. Die sichtbare Tab-Leiste ist A5; bis
    /// dahin sagt die Leiste, was passiert ist — und zwar mit der
    /// Nummer des Tabs, damit „Tab 4“ etwas ist, worauf man sich etwas
    /// beziehen kann.
    fn mittelklick(&mut self, punkt: Point) {
        // Die Bedienflaeche hat keine mittlere Taste. Wer auf die Leiste
        // mittelklickt, will die Adresse in einen neuen Tab — das ist
        // genau das, was passiert.
        if punkt.y < self.chrome.seiten_oberkante() as f32 {
            if treffer(&self.chrome.adressleiste(), punkt) && !self.browser.adresse().is_empty() {
                self.link_in_neuem_tab(&self.browser.adresse().to_string());
            }
            return;
        }
        //
        // GEMESSENER FEHLER (08:10), und es ist ein Produktfehler, nicht
        // ein Testfehler: der Punkt kommt in BILDSCHIRM-Koordinaten
        // (`SurfaceEvent`), der Layoutbaum rechnet in SEITEN-Koordinaten.
        // Es fehlte das Abziehen von `SEITEN_START_Y` (58 px seit der
        // Tab-Leiste; davor 32 px). GEMESSENE FOLGE: ein Klick mit der
        // MAUS auf einen Link hat ihn nie getroffen — der Test
        // `http.rs::ein_klick_auf_einen_link_fuehrt_woanders_hin`
        // schlug fehl und meldete „der Klick hat den Browser nicht auf
        // das Ziel gebracht", ohne zu sagen, dass der Klick 58 px daneben
        // lag.
        //
        // GEMESSENE BEGRUENDUNG fuer die Formel: dieselbe, die
        // `Sitzung::klick` benutzt (`y_seite = y - SEITEN_START_Y +
        // versatz`). Zwei Formeln fuer dieselbe Frage sind zwei
        // Wahrheiten — und sie standen 58 px auseinander.
        let y_seite = punkt.y - self.chrome.seiten_oberkante() as f32;
        let ziel = self
            .angezeigt
            .as_ref()
            .and_then(|a| link_am_punkt(a, punkt.x, y_seite + a.versatz as f32));
        match ziel {
            // 05.10. 23:22: `ziel` ohne Extra-& (GEMESSENER Typfehler):
            // `narsil_page::link_unter` liefert inzwischen `Option<&str>`
            // (das `ersetzte Groesse`-Nebenprodukt der Werbung), und
            // `link_in_neuem_tab` nimmt `&str`. Mit `&ziel` war es ein
            // `&&str`. Siehe `team/PROBLEME.txt` 23:12 und 23:22.
            Some(ziel) => self.link_in_neuem_tab(&ziel),
            None => {
                //
                // KONTEXT (06.10.2026, 03:24): OHNE LINK STARTET DER **AUTO-
                // LAUF** (Leitung: „Auto-Scroll“). GEMESSENE ENTSCHEIDUNG,
                // warum nicht „immer“: in Firefox oeffnet die mittlere Taste auf
                // einem **Link** einen neuen Tab, und das bleibt (es ist
                // brauchbarer als ein Scrollkreuz). Und warum die mittlere Taste
                // **noch einmal** beendet: der Nutzer muss den Finger von der
                // Taste nehmen koennen, ohne nach dem Menue zu suchen.
                if self.auto_lauf.is_some() {
                    self.auto_lauf = None;
                    self.auto_rest = AutoRest::default();
                    self.meldung = Some(tr("Auto scroll ended").into());
                    self.schmutzig = true;
                    return;
                }
                let jetzt_ns = crate::video::jetzt_ns();
                self.auto_lauf = Some((punkt.x as u32, punkt.y as u32, jetzt_ns));
                self.auto_letzte_ns = jetzt_ns;
                self.auto_rest = AutoRest::default();
                self.meldung = Some(
                    tr("Auto scroll: moving the mouse, the middle button or Escape ends it").into(),
                );
                self.schmutzig = true;
            }
        }
    }

    /// DEN **QUELLTEXT** zeigen — der Weg fuer **Menue** und **Strg+U**.
    ///
    /// KONTEXT (06.10.2026, 05:08): GEMESSENE ENTSCHEIDUNG, warum **hier** und
    /// nicht an beiden Stellen: zwei Stellen, die dasselbe tun, sind zwei
    /// Stellen, an denen es einmal nur halb getan wird — GEMESSEN war genau das
    /// (Menue = Tab + Rohtext, Taste = nur Tab). GEMESSENE ENTSCHEIDUNG, warum
    /// **im neuen Tab**: so macht es Firefox.
    pub fn quelltext_zeigen(&mut self) {
        let adresse = self.browser.adresse().to_string();
        if adresse.is_empty() {
            self.meldung = Some(tr("No source: no page is open").into());
            return;
        }
        self.link_in_neuem_tab(&format!("view-source:{adresse}"));
        if self.view_source_zeigen(&adresse).is_err() {
            self.meldung = Some(tr("Source: the page could not be built").into());
        }
    }

    /// Einen Link in einem neuen Tab oeffnen — der Weg, den die mittlere
    /// Taste geht (und der einzige, der einen Tab anlegt).
    fn link_in_neuem_tab(&mut self, ziel: &str) {
        if ziel.is_empty() {
            self.meldung = Some(tr("Middle click: the link has no target").into());
            self.schmutzig = true;
            return;
        }
        // Der Verlauf des ALTEN Tabs wird nicht angetastet: das ist der
        // ganze Sinn der mittleren Taste.
        let vorher = self.browser.tabs();
        if self.browser.anwenden(&Navigation::TabNeu) == Wirkung::Leer {
            self.meldung = Some(trf!(
                "Middle click: {} tabs — the new tab is not opened",
                MAX_TABS
            ));
            self.schmutzig = true;
            return;
        }
        self.browser.anwenden(&Navigation::To(ziel.to_string()));
        // Die Nummer des neuen Tabs: GEMESSEN, warum `tabs() - 1` und
        // nicht ein `TabId`: die Zahl, die der Nutzer sieht, ist die
        // Position in der Leiste (1, 2, 3), und die stimmt nur, solange
        // kein Tab geschlossen wurde. `TabId` waere die richtige Antwort
        // auf eine andere Frage.
        self.meldung = Some(trf!(
            "Tab {} of {}: {} (switch: A5)",
            self.browser.tabs(),
            vorher,
            ziel
        ));
        self.chrome
            .setze_adresse(&self.browser.adresse().to_string());
        self.schmutzig = true;
    }

    /// Eine Taste: Adresszeile bearbeiten oder navigieren.
    fn taste(&mut self, k: KeyEvent) {
        //
        // VIDEO_19 (04.10.2026): DAS VIDEO ZUERST — aber nur, wenn es
        // vorhanden ist, und **vor** dem bisherigen Weg.
        //
        // GEMESSENE ENTSCHEIDUNG, warum **vor** allem anderen: die Leertaste
        // ist nach eigener Messung (Kommentar weiter unten, aus
        // `narsil/src/sitzung.rs` Z. 2217) in **KEINEM** Browser eine reine
        // Video-Taste — sie scrollt auch. Ein Video, das die Leertaste
        // **nur dann** nimmt, wenn die Leiste sichtbar ist, waere die
        // richtige Reihenfolge fuer den Normalfall; solange die Seite aber
        // nichts ueber den Fokus sagt, ist die Reihenfolge eine
        // **Entscheidung**, und ich habe sie so getroffen wie bei
        // `mausklick`: **das Objekt ueber der Seite zuerst**, das Menue,
        // jetzt das Video.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `return`, wenn das Video die Taste
        // **nicht** angenommen hat: `Option::None` heisst „kein Video
        // oder keine Wirkung" — dann laeuft der bisherige Weg weiter. So
        // verliert eine Seite ohne Video nichts.
        if let Some(v) = self.video.as_mut() {
            if let Some(t) = video_taste(k) {
                let jetzt = crate::video::jetzt_ns();
                if v.f.zugriff() {
                    v.taste(t, jetzt);
                    self.schmutzig = true;
                    return;
                }
            }
        }
        //
        // W7C A1 (00:05): DIE SCROLL-TASTEN KOMMEN ZUERST.
        //
        // GEMESSEN, warum VOR allem anderen, auch vor dem Formularfeld:
        // Bild auf/ab, Pos1/Ende und die Leertaste sind in KEINEM Browser
        // Text, und ein Textfeld, das Bild auf abfaengt, ist ein
        // Formular, in dem man nicht blättern kann. Wer sie abfaengt,
        // haelt die Seite fest.
        //
        // GEMESSEN, warum die Leertaste eine Ausnahme ist: sie ist BEIDES
        // (Text und Scrollen). Sie scrollt, wenn die Leiste NICHT im
        // Fokus ist, und tippt, wenn sie es ist — genau so, wie man es
        // erwartet.
        if self.scroll_taste(k) {
            self.schmutzig = true;
            return;
        }
        //
        // W7 P2 (21:25): EIN FOKUSSIERTES FELD HAT VORRANG.
        //
        // GEMESSEN, warum VOR dem Strg+L-Zweig: `Ctrl+L` ist ein Ausgang,
        // aber `Escape`, die Pfeiltasten und `Enter` sind es nicht — und
        // die gehoeren ins Feld, solange eines den Fokus hat. Wer sie
        // vorher abfaengt, kann in einem Formular nicht tippen.
        //
        // GEMESSEN, warum `false` zurueckkommt, wenn nichts passiert:
        // sonst wuerde der Aufrufer denken, es sei etwas geschehen, und
        // die Seite als geaendert markieren.
        //
        // KONTEXT (04.10.2026, 01:05, Leitung „Strg+Z“ und „Tab wandert den
        // Fokus“): BEIDE **vor** dem Zweig fuer das fokussierte Feld.
        //
        // GEMESSENER EIGENER FEHLER (gefunden beim Lesen, nicht beim Bauen):
        // mein erster Entwurf stand **weiter unten**, nach
        // `if self.feld_fokus.is_some() && self.taste_ins_feld(k)` — und das
        // heisst: **in einem Feld kommt die Taste nie an**. Genau die
        // Wirkung, die verlangt war, waere in genau dem Zustand ausgefallen,
        // um den es geht. GEMESSENE ENTSCHEIDUNG, warum `Tab` **immer**
        // wandert (auch ohne fokussiertes Feld): Firefox setzt dann auf das
        // **erste** Feld.
        //
        // KONTEXT (05.10.2026, 08:45): EIN OFFENER DATEI-DIALOG IST EIN
        // **MODUS** — er beantwortet die Tasten **vor** allem anderen.
        if self.datei_dialog_tasten(k) {
            return;
        }
        // Release 08.10.2026: Tasten der Features (Strg+Umschalt+F fuellt
        // Formulare aus dem Formular-Gedaechtnis).
        if self.features_taste(k) {
            self.schmutzig = true;
            return;
        }
        if k.key == narsil_input::Key::Tab && !k.modifiers.ctrl {
            //
            // KONTEXT (05.10.2026, 12:20): TAB GEHT UEBER **LINKS UND
            // FELDER** (siehe `fokus_reihe_weiter`).
            //
            // GEMESSENE ENTSCHEIDUNG, warum die **alte** `fokus_weiter` fuer
            // Felder nicht mehr der Aufrufer ist: GEMESSEN ist, dass sie die
            // Reihenfolge aus `Seite::felder` nimmt — also **nur** Felder. Zwei
            // Wege fuer dieselbe Taste waere die Fehlerform „zwei Stellen
            // entscheiden“ (und die Frage „ist jetzt ein Feld oder ein Link im
            // Fokus?“ haette zwei Antworten).
            self.fokus_reihe_weiter(!k.modifiers.shift);
            return;
        }
        //
        // KONTEXT (05.10.2026, 15:40, Leitung „Drag&Drop-Text“): STRG+UMSCHALT+V
        // — **Einfuegen ohne Formatierung**.
        //
        // GEMESSENE ENTSCHEIDUNG, warum **nur** im Feld: GEMESSEN ist, dass es
        // auf der Seite keinen Ort fuer eingefuegten **Text** gibt (es gibt kein
        // `contenteditable`-Modell — GEMESSEN: `contenteditable` kommt in
        // `narsil-page` **nicht** vor). Ohne Feld hat ein Einfuegen **nirgends**
        // eine Wirkung, und eine Taste ohne Wirkung wird als tot gemeldet.
        //
        // GEMESSENE ENTSCHEIDUNG, warum derselbe Weg wie `Strg+V`: der Weg liest
        // **erst die System-Zwischenablage** (KONTEXT_26) — eine zweite
        // Einfuege-Art waere ein zweiter Ort, der die Frage „was kommt herein?“
        // beantwortet.
        //
        // GEMESSENE **Einschraenkung**: `Umschalt+Einfuegen` — die klassische
        // Einfuegetaste — gibt es **nicht**: GEMESSEN ist, dass `narsil-input`
        // **keine** `Key::Insert` kennt (`grep -n "Insert" crates/narsil-input/
        // src/*.rs` = null; die Liste ist Char, Enter, Tab, Backspace, Delete,
        // Escape, Space, Left, Right, Up, Down, Home, End, PageUp, PageDown,
        // Shift, Control, Alt). Das ist eine **fremde** Datei und **eine** Zeile
        // dort — im BRETT.
        if k.modifiers.ctrl
            && k.modifiers.shift
            && !k.modifiers.alt
            && k.pressed
            && k.key == narsil_input::Key::Char('v')
            && self.feld_fokus.is_some()
        {
            self.feld_markiert_einfuegen();
            return;
        }
        //
        // KONTEXT (05.10.2026, 12:25): **ENTER FOLGT** dem fokussierten Link
        // oder Knopf.
        //
        // GEMESSENE ENTSCHEIDUNG, warum **vor** dem Eingabefeld-Zweig: ein
        // fokussiertes **Feld** nimmt `Enter` selbst (Formular abschicken), und
        // `fokus_knoten` ist dann `None` — die Abfrage kostet also nichts und
        // schlaegt nie den falschen Zweig.
        if k.key == narsil_input::Key::Enter && !k.modifiers.ctrl {
            if self.fokus_link_oeffnen() {
                return;
            }
        }
        if k.modifiers.ctrl
            && !k.modifiers.alt
            && k.key == narsil_input::Key::Char('z')
            && self.feld_fokus.is_some()
        {
            self.feld_rueckgaengig();
            return;
        }
        //
        // KONTEXT (05.10.2026, 02:00, Leitung „Kuerzel-Tabelle komplett“):
        // DIE **TAB-ZAHLEN** (Strg+1 … Strg+9 / Strg+Umschalt+1 … 9).
        //
        // GEMESSENE ENTSCHEIDUNG, warum `Char('1')..=Char('9')` und nicht
        // `Key::Digit`: GEMESSEN ist, dass `narsil-input` **kein** `Key::Digit`
        // hat (die Tastatur liefert `Char`), also waere `Digit` hier ein Weg,
        // den es nicht gibt.
        //
        // GEMESSENE ENTSCHEIDUNG, warum **vor** dem Zweig fuer das fokussierte
        // Feld: dasselbe Argument wie bei `Strg+Z` (siehe dort) — und hier
        // noch schaerfer: `Strg+4` in einem Textfeld **tippt** nichts, es
        // springt — das ist in jedem Browser so.
        if k.modifiers.ctrl && !k.modifiers.alt && !k.modifiers.shift {
            if let narsil_input::Key::Char(z) = k.key
                && z.is_ascii_digit()
                && z != '0'
            {
                // Release 09.10.2026: Strg+1 ist der ERSTE Tab (Nummer 0),
                // Strg+9 der LETZTE (Chrome/Firefox). GEMESSEN vorher: Strg+1
                // sprang auf den zweiten Tab (`tab_wechseln_zu` zaehlt ab 0).
                let ziel = if z == '9' {
                    self.browser.tabs().saturating_sub(1) as u32
                } else {
                    (z as u32) - ('1' as u32)
                };
                if (ziel as usize) < self.browser.tabs() {
                    self.tab_wechseln_zu(ziel);
                }
                return;
            }
        }
        //
        // KONTEXT (05.10.2026, 02:05): STRG+UMSCHALT+1 … 9 **VERSCHIEBT** den
        // Tab an die Stelle (Firefox: „Tab an Position n“).
        //
        // GEMESSENE ENTSCHEIDUNG, warum auch das **vor** dem Feld-Zweig: in
        // einem Textfeld tippt `Umschalt+4` ein `4` — in Firefox aber nicht,
        // weil `Strg` gedrueckt ist.
        if k.modifiers.ctrl && !k.modifiers.alt && k.modifiers.shift && k.pressed {
            if let narsil_input::Key::Char(z) = k.key
                && z.is_ascii_digit()
                && z != '0'
            {
                let ziel = (z as u32) - ('0' as u32);
                // KONTEXT (05.10.2026, 18:05): `aktiver_tab_nr()` gibt **usize**
                // (GEMESSEN: `narsil-browser/src/lib.rs:1033`), `TabUmstellen`
                // will **u32** (E0308, vom Compiler benannt). GEMESSENE
                // ENTSCHEIDUNG, warum die Umrechnung **hier** und nicht im
                // Browser: die `Navigation`-Variante ist die **Schnittstelle**
                // zwischen beiden Crates — eine Umrechnung an der Schnittstelle
                // ist eine, nicht zwei.
                let von = self.browser.aktiver_tab_nr() as u32;
                // 06.10.2026: ueber `tabs_umstellen` — der Browser UND die
                // Seitenliste ziehen mit. Vorher blieb `self.tabs` stehen,
                // und ein gesetztes `tab_zieht` OHNE Loslassen machte danach
                // jede Mausbewegung zu einem Umstellen (die Knoepfe wirkten
                // „haengend").
                self.tabs_umstellen(von, ziel);
                return;
            }
        }
        //
        // KONTEXT (05.10.2026, 02:10): STRG+G — der **naechste** Treffer.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das **ein eigener Zweig** ist und nicht
        // `is_f3()`: GEMESSEN ist (in `narsil-input/src/key.rs:368`), dass
        // `is_f3()` **nur** `Key::F(3)` ist — `Strg+G` kommt dort **nicht** vor,
        // obwohl es in der Kuerzelliste der Oberflaeche steht
        // (`tasten.rs`, „Naechster Treffer“, `Herkunft::Narsil` — also meins).
        if k.modifiers.ctrl
            && !k.modifiers.alt
            && !k.modifiers.shift
            && k.pressed
            && k.key == narsil_input::Key::Char('g')
        {
            self.suche_weiter();
            return;
        }
        //
        // KONTEXT (05.10.2026, 02:12): STRG+U — der **Quelltext** der Seite.
        //
        // GEMESSENE ENTSCHEIDUNG, warum ueber `link_in_neuem_tab`: genau der
        // Weg, den das Kontextmenue „Quelltext anzeigen“ nimmt (GEMESSEN: der
        // Zweig `Eintrag::Quelltext` bei 2699) — zwei Zeilen, die dasselbe
        // sagen, muessen nicht zwei Zeilen sein.
        //
        // GEMESSENE ENTSCHEIDUNG, warum **im neuen Tab**: so macht es Firefox,
        // und der Weg ueber `Eintrag::Quelltext` ist bereits gemessen.
        if k.modifiers.ctrl
            && !k.modifiers.alt
            && !k.modifiers.shift
            && k.pressed
            && k.key == narsil_input::Key::Char('u')
        {
            self.quelltext_zeigen();
            return;
        }
        //
        // KONTEXT (06.10.2026, 05:48): STRG+S — die Seite **mit Dateidialog**
        // speichern (Leitung). GEMESSENE ENTSCHEIDUNG, warum der **Dialog**
        // und nicht sofort `~/Downloads`: GEMESSEN ist, dass `seite_speichern`
        // ohne Frage in `~/Downloads` schreibt und dabei ein vorhandenes
        // **ueberschreibt** — ein Name, den der Nutzer nie genannt hat, ist
        // keine Speicherung, sondern ein Fund. GEMESSENE ENTSCHEIDUNG, warum
        // `Umschalt` nichts aendert: in Firefox speichert `Umschalt+Strg+S`
        // **ohne** Dialog — das ist eine **zweite** Entscheidung (ein fester
        // Name, ueber den man nicht abstimmen kann) und gehoert an die
        // Oberflaeche, nicht hierher. GEMESSENE ENTSCHEIDUNG, warum `Alt`
        /// ausgeschlossen ist wie bei `Strg+U`: dieselbe Taste, dieselbe Regel.
        if k.modifiers.ctrl
            && !k.modifiers.alt
            && k.pressed
            && k.key == narsil_input::Key::Char('s')
        {
            self.datei_dialog_speichern_oeffnen();
            return;
        }
        //
        // KONTEXT (05.10.2026, 02:15): STRG+K — die **Suchleiste**.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das die **Adressleiste** ist: in
        // Firefox ist `Strg+K` „in der Suchleiste suchen“ — und unsere
        // Adressleiste **ist** die Suchleiste: GEMESSEN ist die Regel in
        // `adresse_wohin` (5000ff), dass `beispiel.org` ohne Schema als
        // `http://beispiel.org` gesucht wird. Es gibt keinen zweiten Ort fuer
        // Eingaben aus der Leiste (`leiste_im_fokus` ist der Ort, siehe dort).
        if k.modifiers.ctrl
            && !k.modifiers.alt
            && !k.modifiers.shift
            && k.pressed
            && k.key == narsil_input::Key::Char('k')
        {
            self.chrome.leeren();
            self.leiste_im_fokus = true;
            self.leiste_verlauf_pos = None;
            self.schmutzig = true;
            return;
        }
        //
        // KONTEXT (05.10.2026, 02:18): STRG+D — **Lesezeichen setzen**.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das ueber `hinzufuegen` geht: GEMESSEN
        // ist, dass `Sitzung::lesezeichen` eine echte Liste traegt (`:640`) und
        // dass `Lesezeichen::aus_adresse` der Weg ist, den der Knopf in der
        // Leiste nimmt (`:4768`). Es fehlte nur die **Taste**.
        if k.modifiers.ctrl
            && !k.modifiers.alt
            && !k.modifiers.shift
            && k.pressed
            && k.key == narsil_input::Key::Char('d')
        {
            // Release 09.10.2026: Strg+D SETZT ein Lesezeichen, entfernt aber
            // keins (Firefox/Chrome zeigen dann den Bearbeiten-Dialog). GEMESSEN
            // vorher: ein zweites Strg+D loeschte das gerade gesetzte
            // Lesezeichen (Test `strg_d_setzt_aus_ein_lesezeichen`).
            if self.lesezeichen.hat(self.browser.adresse()) {
                self.meldung = Some(tr("This page is already a bookmark (the star removes it)").into());
                self.schmutzig = true;
            } else {
                self.lesezeichen_umschalten();
            }
            return;
        }
        //
        // KONTEXT (06.10.2026, 07:30, Leitung „Lesezeichen mit **Stern-Zustand**“):
        // STRG+STRG — nein: STRG+UMSCHALT+B blendet die **Lesezeichenleiste** ein
        // und aus. GEMESSEN ist, dass die Leiste (`narsil_chrome::lesezeichen::
        // LesezeichenListe`) eine **eigene** Funktion dafuer hat
        // (`liste_umschalten`, Z. 335) — der Zweig ist also ein Aufruf, und der
        // Zustand gehoert der Leiste, nicht der Sitzung (sonst waeren es zwei
        // Wahrheiten ueber dieselbe Sichtbarkeit).
        if k.modifiers.ctrl
            && k.modifiers.shift
            && !k.modifiers.alt
            && k.pressed
            && k.key == narsil_input::Key::Char('b')
        {
            self.lesezeichen.liste_umschalten();
            self.schmutzig = true;
            return;
        }
        //
        // KONTEXT (06.10.2026, 07:32): STRG+H — die **Verlaufleiste**.
        //
        // GEMESSENE ENTSCHEIDUNG, warum hier **nur der Zustand** wechselt: die
        // Seitenleiste selbst zeichnet `narsil-display` (GEMESSEN: dort ist die
        // neue Variante `DisplayItem::Verlauf` entstanden, BILD baut sie). Der
        // Zustand gehoert hier, damit die Taste **eine** Sache sagt („Leiste
        // an/aus“) und nicht „Leiste an“ im Fenster suchen muss.
        if k.modifiers.ctrl
            && !k.modifiers.alt
            && !k.modifiers.shift
            && k.pressed
            && k.key == narsil_input::Key::Char('h')
        {
            self.verlauf_leiste = !self.verlauf_leiste;
            self.schmutzig = true;
            self.meldung = Some(if self.verlauf_leiste {
                tr("History bar")
            } else {
                tr("History bar closed")
            }
            .into());
            return;
        }
        //
        // KONTEXT (06.10.2026, 09:22): STRG+J — die **Download-Leiste**.
        //
        // GEMESSENE ENTSCHEIDUNG, warum hier **derselbe** Zustandswechsel wie
        // bei `Strg+H` steht und nicht in einem gemeinsamen Zweig: zwei
        // Tasten, zwei Leisten — und ein gemeinsamer Zweig wuerde eine Liste
        // „welche Taste schaltet was“ brauchen, die laenger ist als die zwei
        // Zeilen.
        if k.modifiers.ctrl
            && !k.modifiers.alt
            && !k.modifiers.shift
            && k.pressed
            && k.key == narsil_input::Key::Char('j')
        {
            self.downloads_umschalten();
            return;
        }
        //
        // KONTEXT (05.10.2026, 02:20): F11 — **Vollbild**.
        //
        // GEMESSENE ENTSCHEIDUNG, warum hier nur der **Zustand** gesetzt wird
        // und nichts gezeichnet wird: GEMESSEN ist, dass `Chrome::vollbild` ein
        // Feld ist (`:1400`) und `setze_vollbild` dazugehoert (`:3283`) — aber
        // GEMESSEN ist **auch**, dass die Zeichnung der Oberflaeche den Zustand
        // **nirgends liest** (`grep vollbild crates/narsil-chrome/src/*.rs` ohne
        // den Treffer in `lib.rs`). Die Werkzeugleiste bleibt also stehen, bis
        // die Oberflaeche das liest — steht im BRETT, mit der Messung dabei.
        if k.key == narsil_input::Key::F(11) {
            let an = !self.chrome.vollbild;
            self.chrome.setze_vollbild(an);
            self.schmutzig = true;
            self.meldung = Some(if an { tr("Full screen") } else { tr("Window") }.into());
            return;
        }
        //
        // KONTEXT (05.10.2026, 03:58): DIE AUSWAHL-LISTE BEKOMMT IHRE TASTEN —
        // **vor** dem Eingabefeld, denn beides kann nicht zugleich im Fokus
        // sein (siehe `auswahl_fokus`).
        if self.auswahl_tasten(k) {
            self.schmutzig = true;
            return;
        }
        if self.feld_fokus.is_some() && self.taste_ins_feld(k) {
            self.schmutzig = true;
            return;
        }
        // Strg+L behält die Adresse und markiert sie zum Überschreiben.
        if k.is_ctrl_l() {
            // Release 09.10.2026: kommt der Fokus von der Seite, steht in der
            // Leiste die Adresse der SEITE (Firefox/Chrome) — vorher markierte
            // Strg+L, was zufaellig in der Leiste stand (im Test: nichts).
            if !self.leiste_im_fokus {
                let adresse = self.browser.adresse().to_string();
                if !adresse.is_empty() {
                    self.chrome.setze_adresse(&adresse);
                }
            }
            self.chrome.alles_auswaehlen();
            self.leiste_im_fokus = true;
            // W7C A1: mit geleerter Leiste gibt es keinen Verlaufskandidaten.
            self.leiste_verlauf_pos = None;
            self.schmutzig = true;
            return;
        }
        //
        // W7C A7 (12:20): STRG++ / STRG+- / STRG+0.
        //
        // GEMESSEN, warum VOR den Tabs: der Zoom ist ein SPRUNGBEFEHL
        // wie die Tabs, und die Reihenfolge zweier Sprungbefehle ist
        // beliebig — aber es ist besser, sie an EINER Stelle zu haben
        // (dieser Block), damit ein neuer Sprungbefehl nicht zwischen
        // zwei anderen landet.
        if let Some(b) = k.zoom_befehl() {
            match b {
                1 => self.zoom_rein(),
                -1 => self.zoom_raus(),
                _ => self.zoom_zurueck(),
            }
            return;
        }
        //
        // W7C A5 (11:20): STRG+T / STRG+W / STRG+TAB.
        //
        // GEMESSEN, warum die Tabs VOR allem anderen kommen (und vor
        // Strg+F): sie sind ein SPRUNGBEFEHL — wie Strg+F — und ein
        // Sprungbefehl, der im Formularfeld landet, tippt statt zu
        // springen.
        //
        // KONTEXT (04.10.2026, 13:20, Leitung „Strg+Umschalt+T“): DER
        // WIEDERHERSTELLEN-BEFEHL WAR **TOT** an der Taste. GEMESSEN:
        // `narsil-input` hat keine `is_ctrl_shift_t` (GEMESSEN: alle
        // `pub fn is_*` in `narsil-input/src/lib.rs` — `is_ctrl_c/q/v/a/l/t/w/f`,
        // `ist_ctrl_tab`, `is_f3`, `is_reload`, `zoom_befehl`,
        // `alt_navigation`), und `grep -rnE "'t'.*shift|shift.*'t'"`
        // `crates/narsil/src/*.rs` findet **null** Treffer. GEMESSENE
        // ENTSCHEIDUNG, warum hier und nicht in `narsil-input`: dieselbe Form
        // steht 30 Zeilen weiter unten fuer F12/Strg+Umschalt+I (GEMESSEN dort
        // der Kommentar „F12 lief ins Leere“) — ein zweiter Weg fuer
        // Tastenfragen waere die Fehlerform „zwei Stellen entscheiden“.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `!alt`: `Alt` ist in Firefox ein
        // eigener Zweiweg-Bereich (GEMESSEN: `alt_navigation`, `only_alt`), und
        // ein Klick mit Alt soll nicht den Tab wiederherstellen.
        if k.pressed
            && k.key == Key::Char('t')
            && k.modifiers.ctrl
            && k.modifiers.shift
            && !k.modifiers.alt
        {
            let vorher = self.browser.geschlossene_anzahl();
            match self.browser.anwenden(&Navigation::TabWiederherstellen) {
                Wirkung::Leer => {
                    self.meldung = Some(tr("No tab was closed").into());
                }
                Wirkung::Geaendert => {}
            }
            // GEMESSENE ENTSCHEIDUNG, warum die Meldung die **Zahl** nennt und
            // nicht nur „wiederhergestellt“: der Stapel kann **25** Eintraege
            // haben (GEMESSEN: `MAX_GESCHLOSSENE`), und nach dem ersten Strg-
            // Umschalt+T weiss der Nutzer sonst nicht, wie viele folgen.
            self.meldung = Some(if vorher > 0 {
                trf!(
                    "Tab restored, {} left on the stack",
                    vorher.saturating_sub(1)
                )
            } else {
                tr("No tab was closed").into()
            });
            self.chrome
                .setze_adresse(&self.browser.adresse().to_string());
            self.schmutzig = true;
            return;
        }
        if k.is_ctrl_t() {
            self.tab_neu();
            return;
        }
        if k.is_ctrl_w() {
            self.tab_schliessen();
            return;
        }
        if let Some(vorwaerts) = k.ist_ctrl_tab() {
            self.tab_wechseln(vorwaerts);
            return;
        }
        //
        // W7C A6 (10:05): STRG+F — in der Seite suchen.
        //
        // GEMESSEN, warum VOR dem Formularfeld: `Strg+F` ist ein
        // Sprungbefehl, kein Zeichen. Haette es der Feld-Zweig, wuerde
        // es in einem Formular tippen statt suchen — und die Suche ist
        // in einem Formular die haeufigste (ein Kommentar-Feld ist
        // meistens lang).
        //
        // GEMESSEN, warum das Muster aus der LEISTE kommt und nicht aus
        // einem neuen Feld: die Leiste ist das einzige
        // Texteingabeelement dieses Browsers, und „Cursor in die Leiste,
        // Strg+F" ist genau das, was ein Nutzer tippt.
        if k.pressed && k.modifiers.ctrl && k.key == narsil_input::Key::Char('p') {
            // Release 09.10.2026: der echte Druckweg (PDF in den Download-Ordner,
            // dann im neuen Tab). GEMESSEN vorher: dieser fruehere Zweig rief
            // den leeren Druck-Haken der Dienste — Strg+P tat nichts und
            // meldete nichts (Test `strg_p_druckt`); der richtige Zweig weiter
            // unten wurde nie erreicht.
            self.drucken();
            return;
        }
        //
        // KONTEXT (04.10.2026, 22:10, Leitung „Strg+C/Strg+A“): DIE AUSWAHL
        // KOMMT VOR **allen** anderen Strg-Zweigen. GEMESSENE ENTSCHEIDUNG,
        // warum hier und nicht unten bei der Zwischenablage: `Strg+C` ist die
        // **erste** Frage, die der Nutzer stellt, wenn er Text markiert hat —
        // und ein Klick, der an einer anderen Stelle landet, ist ein Fehler,
        // den man erst an der leeren Zwischenablage bemerkt.
        if k.modifiers.ctrl && !k.modifiers.alt && k.key == narsil_input::Key::Char('c') {
            let text = self.auswahl_text_holen();
            if text.is_empty() {
                // GEMESSENE ENTSCHEIDUNG, warum ohne Auswahl **nichts** in die
                // Zwischenablage kommt: GEMESSEN ist, dass das Konto seit
                // KONTEXT_12 ein **Feld** ist — ein leeres „Kopieren“ (ohne
                // Auswahl und ohne Eingabefeld) wuerde beim naechsten Einfuegen
                // einen **Leerstrich** einfuegen statt nichts.
                self.meldung = Some(tr("Copy: nothing selected").into());
            } else {
                self.zwischenablage = text.clone();
                //
                // KONTEXT (05.10.2026, 10:30, Leitung „Kopieren/Einfuegen
                // **systemweit**“): DER TEXT GEHT **NACH AUSSEN**.
                //
                // GEMESSENE ENTSCHEIDUNG, warum **zusaetzlich** zur internen
                // Ablage: GEMESSEN ist, dass `Sitzung::zwischenablage` nur
                // **hier** steht — ein `Strg+V` in ein anderes Programm
                // bekaeme nichts, und genau das ist der Fall, den die Leitung
                // meint („Nutzer kopieren aus anderen Programmen“).
                //
                // GEMESSENE ENTSCHEIDUNG, warum die **Meldung** den Werkzeug-
                // namen nicht nennt: sie nennt die **Zeichen** (das ist die
                // Zahl, die der Nutzer prueft). „Kein xclip“ steht im BRETT,
                // weil ein Rechner ohne `xclip` ein Rechner ohne Werkzeug ist
                // und kein Fehler des Browsers.
                let draussen = crate::zwischenablage_system::schreiben(&text);
                self.meldung = Some(trf!(
                    "Copied: {} characters{}",
                    text.chars().count(),
                    if draussen { "" } else { tr(" (only in the browser)") }
                ));
            }
            self.schmutzig = true;
            return;
        }
        if k.modifiers.ctrl && !k.modifiers.alt && k.key == narsil_input::Key::Char('a') {
            //
            // GEMESSENE ENTSCHEIDUNG, warum `Strg+A` **nur** bei einer Auswahl
            // gilt und nicht immer: GEMESSEN ist, dass der Adressleisten-Zweig
            // `is_ctrl_a() && self.leiste_im_fokus` (W7C) fuer die **Leiste**
            // gilt — ohne diese Beschraenkung wuerde jeder `Strg+A` die Seite
            // markieren, obwohl der Nutzer die **Adresse** markieren wollte.
            if !self.leiste_im_fokus {
                if let Some(a) = self.angezeigt.as_ref() {
                    let stellen = text_stellen(a);
                    if let Some(letzte) = stellen.last() {
                        self.auswahl = Auswahl::alles(stellen.len(), letzte.text.chars().count());
                        self.auswahl_text_holen();
                        self.meldung = Some(tr("All selected").into());
                        self.schmutzig = true;
                        return;
                    }
                }
            }
        }
        if k.is_ctrl_f() {
            self.suche_starten();
            return;
        }
        //
        // W7C A6: F3 = der naechste Treffer.
        // D9 (01.10.2026): F12 UND STRG+SHIFT+I.
        //
        // GEMESSENE ENTSCHEIDUNG, warum VOR dem `match k.key` (Z. 1090)
        // und nicht darin: zwei gemessene Fehler.
        //
        // 1. **Strg+Shift+I zerstoerte die Adressleiste.** Der Pfad
        //    `only_ctrl()` ist bei gesetztem Shift falsch, alle
        //    `is_ctrl_*`-Pruefungen greifen nicht, `typed_char()` gibt
        //    `None` (Strg gesetzt) — und `k.key` ist `Key::Char('i')`,
        //    also landet die Taste in `self.tippe(c)` (Z. 1091). Gemessen
        //    am Code: das leert die Leiste und schreibt „i" hinein.
        // 2. **F12 lief ins Leere.** `scroll_taste` gibt bei `Key::F(_)`
        //    `false`, `taste_ins_feld` ebenso, und ganz unten steht
        //    `_ => {}` (Z. 1139). Gemessen: `grep "Key::F("` in
        //    `narsil/src` findet NICHTS.
        //
        // Deshalb EIN Test vor dem `match`, und beide Wege gehen an
        // `Geruest::taste_wirkt` — dort steht die Unterscheidung
        // „schaltet" gegen „setzt" an EINER Stelle.
        if k.pressed
            && (k.key == narsil_input::Key::F(12)
                || (k.key == narsil_input::Key::Char('i')
                    && k.modifiers.ctrl
                    && k.modifiers.shift
                    && !k.modifiers.alt))
        {
            crate::devtools_bruecke::taste(
                self,
                if k.key == narsil_input::Key::F(12) {
                    narsil_devtools::d1_geruest::Taste::F12
                } else {
                    narsil_devtools::d1_geruest::Taste::StrgSchiftI
                },
            );
            return;
        }
        if k.is_f3() {
            self.suche_weiter();
            return;
        }
        //
        // W7C A1: STRG+A — alles in der Leiste markieren.
        //
        // GEMESSEN, warum die Auswahl EIGEN sichtbar sein muss: eine
        // Markierung, die man nicht sieht, ist ein Zustand, den man nicht
        // bemerkt — und der naechste Buchstabe loescht dann die halbe
        // Adresse, ohne dass jemand etwas getan haette. `narsil-chrome`
        // zeichnet sie (`to_display_list`).
        if self.leiste_im_fokus && k.modifiers.ctrl && !k.modifiers.alt {
            match k.key {
                Key::Char('v') => {
                    if let Some(text) = crate::zwischenablage_system::system_oder_intern(Some(self.zwischenablage.clone())) {
                        self.adress_text_einfuegen(&text);
                    }
                    return;
                }
                Key::Char('c') | Key::Char('x') => {
                    if let Some((a, b)) = self.chrome.auswahl {
                        let text: String = self.chrome.adresse.chars().skip(a.min(b)).take(a.abs_diff(b)).collect();
                        self.zwischenablage = text.clone();
                        crate::zwischenablage_system::schreiben(&text);
                        if k.key == Key::Char('x') {
                            self.chrome.backspace();
                            self.schmutzig = true;
                        }
                    }
                    return;
                }
                _ => {}
            }
        }
        if k.is_ctrl_a() && self.leiste_im_fokus {
            self.chrome.alles_auswaehlen();
            self.schmutzig = true;
            return;
        }
        //
        // D3 (02.10.2026, 3:2x): DIE KONSOLENEINGABE.
        //
        // GEMESSENER ZUSTAND BIS HIER: die Konsole zeichnete ihre
        // Eingabezeile, aber keine Taste kam dort an — die Tasten gingen an
        // die Seite oder die Adressleiste. Die Ausfuehrung war fertig
        // (`Konsole::ausfuehren_und_leeren`), nur der Weg fehlte: D3 („JS-
        // Eingabe ausfuehren") war im Fenster nicht benutzbar.
        //
        // GEMESSENE ENTSCHEIDUNG, warum HIER und nicht weiter oben: die
        // Vorgaenge, die VOR dieser Zeile ihre eigene Bedeutung bekommen
        // (F12, F3, Strg+A, laufende Suche), sollen auch bei offener
        // Konsole gelten — man tippt Strg+A, um die Adresse zu markieren,
        // nicht um die Konsoleneingabe zu loeschen.
        if crate::devtools_bruecke::taste_eingabe(self, &k) {
            return;
        }
        // Enter: laden. Steht im Vordergrund, weil es die wichtigste
        // Taste ist — alles andere ist Beiwerk.
        //
        // KONTEXT (05.10.2026, 14:30, Leitung „Enter/Shift+Enter“): WAEHREND EINER
        // **SUCHE** IST `Enter` DER NAECHSTE TREFFER — und `Umschalt+Enter` der
        // vorherige.
        //
        // GEMESSENE ENTSCHEIDUNG, warum **vor** dem Laden-Zweig: GEMESSEN ist,
        // dass die Suche **in der Adressleiste** laeuft (das Muster steht dort,
        // `suche_starten`), und dass der Zweig darunter `gehe_zu` macht — ein
        // `Enter` wuerde also **navigieren**, obwohl der Nutzer den nächsten
        // Treffer wollte. Zwei Ziele fuer eine Taste, und die Reihenfolge
        // entscheidet es.
        if k.key == Key::Enter && self.suche_merken.is_some() && self.leiste_im_fokus {
            if k.modifiers.shift {
                self.suche_zurueck();
            } else {
                self.suche_weiter();
            }
            self.leiste_im_fokus = false;
            return;
        }
        //
        // KONTEXT (06.10.2026, 09:46): DIE **VORSCHLAGSLISTE** in der Leiste —
        // Pfeile, Enter, Escape.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das **vor** dem Enter-Zweig steht: ein
        // Enter mit markierter Zeile **springt**, und der Enter-Zweig navigiert.
        // Zwei Ziele fuer eine Taste, und die Reihenfolge entscheidet es.
        //
        // GEMESSENE ENTSCHEIDUNG (NARSIL, 06.10., 23:0x), warum hier `return;`
        // und nicht `return true;` steht (der Block kam mit `true` und machte
        // `taste` rot, E0308): `taste` gibt `()` zurueck — GEMESSEN ist, dass
        // ihr **einziger** Aufrufer `Sitzung::ereignis` ist (Z. 2137) und
        // dass dessen `Key`-Arm das Ergebnis wegwirft. Eine Antwort, die
        // niemand liest, ist eine Zusage ohne Abnehmer; `taste` auf `bool` um
        // zustellen hiesse, seine 42 `return;` zu beruehren — eine
        // Massen-Aenderung an einer Datei, die drei anderen Rollen gehoert, fuer
        // ein Ergebnis, das niemand braucht.
        if self.leiste_im_fokus && self.vorschlaege_wahl.is_some() {
            let mut fertig = false;
            if let Some(wahl) = self.vorschlaege_wahl.as_mut() {
                match k.key {
                    Key::Down => {
                        wahl.runter();
                        fertig = true;
                    }
                    Key::Up => {
                        wahl.hoch();
                        fertig = true;
                    }
                    _ => {}
                }
            }
            if fertig {
                self.schmutzig = true;
                return;
            }
            if k.key == Key::Enter {
                //
                // GEMESSENE ENTSCHEIDUNG, warum der Kandidat **aus der Liste**
                // geholt wird und nicht ueber einen Index in eine neu gebaute
                // Liste: `kandidat()` nimmt **dieselbe** Liste, in der die
                // Nummer entstanden ist — ein zweites `vorschlags_kandidaten()`
                // koennte (bei einem neuen Lesezeichen) anders sortiert sein,
                // und dann zeigte Enter auf eine andere Adresse als die
                // markierte Zeile.
                // `kandidat()` leiht aus der **uebergebenen** Liste, also kann
                // man ihr Ergebnis nicht aus einem Aufruf heraus
                // zurueckgeben (E0515): die Liste braucht einen Namen, der
                // laenger lebt als der Ausdruck. Die Liste wird trotzdem
                // **einmal** gebaut — derselbe Aufruf waere zweimal teurer.
                let kandidaten = self.vorschlags_kandidaten();
                let kandidat = self
                    .vorschlaege_wahl
                    .as_ref()
                    .and_then(|w| w.kandidat(&kandidaten))
                    .cloned();
                if let Some((_titel, adresse)) = kandidat {
                    self.vorschlaege_wahl = None;
                    self.chrome.setze_adresse(&adresse);
                    self.leiste_im_fokus = false;
                    if let Err(e) = self.gehe_zu(&adresse) {
                        self.meldung = Some(e.to_string());
                    }
                    return;
                }
            }
            if k.key == Key::Escape {
                self.vorschlaege_wahl = None;
                self.schmutzig = true;
                return;
            }
        }
        if k.key == Key::Enter && self.leiste_im_fokus {
            let adresse = self.adresse_aus_leiste(&self.chrome.adresse);
            // Der Fehler wandert in die Meldung, er wird NICHT
            // weggeworfen. `gehe_zu` schreibt ihn zwar schon selbst in
            // `self.meldung` — aber das ist eine Tatsache, die sich
            // aendern kann. Wer das Ergebnis einer Operation mit
            // `let _ =` wegwirft, baut sich ein `#[must_use]` ab, das
            // niemand mehr beachtet.
            if let Err(e) = self.gehe_zu(&adresse) {
                self.meldung = Some(e.to_string());
            }
            // Nach dem Laden: die Leiste zeigt wieder nur, wo man ist.
            self.leiste_im_fokus = false;
            return;
        }
        if k.is_reload() {
            self.knopf(Knopf::Neu);
            return;
        }
        //
        // KONTEXT (05.10.2026, 07:20, Leitung „Hart-neu-laden“): STRG+UMSCHALT+R.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das **vor** `is_reload()` muesste und
        // trotzdem **danach** steht: GEMESSEN ist (`narsil-input/src/lib.rs:402`),
        // dass `is_reload()` **auch** auf `Strg+R` passt, aber **nicht** auf
        // `Strg+Umschalt+R` (`only_ctrl()` verbietet Umschalt). Der Zweig ist
        // damit **nicht** doppelt — und wenn `narsil-input` das einmal aufnimmt,
        // ist dieser Zweig **ueberfluessig** (und waere trotzdem harmlos).
        //
        // GEMESSENE ENTSCHEIDUNG, warum `Navigation::ReloadHart` und nicht ein
        // Feld an der Sitzung: GEMESSEN ist die Regel von `narsil-browser` („der
        // EINZIGE Weg, den Zustand zu aendern, ist `anwenden`“) — und der
        // Wunsch steht im Browser (`ladung_hart`), nicht in der Fenster-Buch-
        // haltung.
        if k.modifiers.ctrl
            && k.modifiers.shift
            && !k.modifiers.alt
            && k.pressed
            && k.key == narsil_input::Key::Char('r')
        {
            self.knopf(Knopf::Neu);
            let _ = self.browser.anwenden(&narsil_browser::Navigation::ReloadHart);
            // GEMESSENE EINORDNUNG, wo der Wunsch abgeholt wird: in
            // `gehe_zu`/`laden` (siehe dort) — dort wird er **einmal** gelesen
            // und dem Lader weitergegeben. Hier wird er nur **notiert**.
            return;
        }
        // UI1 (03.10. 09:55): DIE TASTE FUER DEN START-KNOPF.
        //
        // GEMESSENE BEGRUENDUNG fuer eine Taste ueberhaupt: der Knopf ist
        // sichtbar, also muss er auch ohne Maus zu bedienen sein — sonst
        // ist er fuer Tastaturnutzer eine Dekoration.
        //
        // GEMESSENER FEHLER MEINES ERSTEN ENTWURFS: ich hatte `Home`
        // genommen (das ist in jedem Browser die Startseite), und der Test
        // `ui1_die_taste_home_zeigt_die_startseite` war rot — die Adresse
        // blieb `file:///etc/hostname`. Ursache: `Home` ist hier bereits
        // belegt, und zwar als „Pos1" (an den Anfang der SCITE). Zwei
        // Bedeutungen fuer eine Taste, und die bestehende gewinnt.
        //
        // DER WEG: `Alt+Home` — dieselbe Bindung wie in Firefox und
        // Chrome, und sie kollidiert mit nichts: `only_alt` (an den
        // MODIFIERN, nicht am Ereignis — gemessen, E0599) verlangt, dass
        // keine andere Taste gedrueckt ist, und `Pos1` wird ohne
        // Modulator behandelt.
        if narsil_input::Key::Home == k.key && k.modifiers.only_alt() {
            self.knopf(Knopf::Start);
            return;
        }
        // added 02.10.2026, UI4: ALT+PFEIL LINKS/RECHTS = Verlauf.
        //
        // GEMESSEN, warum **vor** dem `Key::Char`-Zweig und vor dem
        // `Key::Left`-Zweig darunter: beide Zweiwege kommen an denselben
        // Tasten vorbei. Ohne Alt gehoeren die Pfeile dem Textcursor in
        // der Leiste (`Key::Left | Key::Right` unten) — `alt_navigation`
        // verlangt `only_alt`, es kann also nicht mit ihnen verwechselt
        // werden. Und `Alt+Buchstabe` ist im `Key::Char`-Zweig ein
        // dritter Weg; er wird hier nicht erreicht, weil `zoom_befehl`
        // und die anderen Befehle weiter oben stehen und `alt_navigation`
        // nur auf den Pfeilen zutrifft.
        if let Some(vorwaerts) = k.alt_navigation() {
            self.navigiere(vorwaerts);
            return;
        }
        //
        // KONVERT_18 (05.10.): STRG+P = DRUCKEN.
        //
        // GEMESSENE BEGRUENDUNG fuer die **Stelle**: VOR dem `Key::Char`-Zweig,
        // weil `Key::Char('p')` bei gesetztem Strg dort in `self.tippe('p')`
        // landet — und `tippe` schreibt das `p` in die **Adressleiste**. Ohne
        // diese Zeile waere Strg+P kein Druck, sondern ein `p` in der Leiste.
        //
        // GEMESSEN, warum es **kein** `Knopf::Drucken` wird: `Knopf` ist ein
        // enum, das an **mehreren** Stellen (`match`) in fremden Dateien
        // ausgewertet wird, und jede neue Variante macht dort einen
        // Compilefehler. Also der **direkte** Weg in `taste()` — das ist
        // tatsaechlich kleiner und bricht nichts.
        //
        // GEMESSEN, dass `only_control` an den **MODIFIERN** steht, nicht am
        // Ereignis (E0599 an der Ereignis-Variante, siehe `Alt+Home` oben):
        // es verlangt, dass **keine** andere Taste gedrueckt ist, also
        // kollidiert Strg+P nicht mit Strg+Shift+P.
        //
        // GEMESSEN: `Modifiers::only_control()` gibt es **nicht** — in
        // `crates/narsil-input/src/lib.rs` ist von der `only_*`-Familie nur
        // `only_alt()` da (Z. 198). Also stehen die drei Felder **hier**, und
        // damit ist die Frage „welche Modifikatoren genau?“ an **zwei** Stellen
        // beantwortet. Der Helfer waere drei Zeilen in `narsil-input` — als
        // Board-Eintrag gemeldet, fremde Datei **nicht** angefasst.
        if narsil_input::Key::Char('p') == k.key
            && k.modifiers.ctrl
            && !k.modifiers.shift
            && !k.modifiers.alt
        {
            self.drucken();
            return;
        }
        match k.key {
            Key::Char(c) => {
                self.tippe(c);
            }
            // W7C A1: PFEIL LINKS/RECHTS bewegen den Cursor in der Leiste.
            // GEMESSEN, warum sie hier und nicht bei den Scroll-Tasten:
            // in der Leiste sind sie Text, auf der Seite sind sie nichts
            // (waagerecht scrollt eine Seite nicht).
            Key::Left | Key::Right => {
                if self.leiste_im_fokus {
                    self.chrome.bewegen(if k.key == Key::Left { -1 } else { 1 });
                    self.schmutzig = true;
                }
            }
            Key::Backspace => {
                if self.leiste_im_fokus {
                    self.chrome.backspace();
                    self.leiste_verlauf_pos = None;
                    self.vorschlaege_aktualisieren();
                    self.schmutzig = true;
                }
            }
            // W7C A1: ENTF loescht vorwaerts (die Taste gab es vorher gar
            // nicht — sie war im `Key`-Zweig, aber niemand behandelte sie).
            Key::Delete => {
                if self.leiste_im_fokus {
                    self.chrome.entf();
                    self.leiste_verlauf_pos = None;
                    self.vorschlaege_aktualisieren();
                    self.schmutzig = true;
                }
            }
            Key::Space => {
                self.tippe(' ');
            }
            Key::Escape => {
                //
                // W7C A6: Escape beendet eine laufende SUCHE zuerst —
                // sie ist der kurzfristigere Zustand. In jedem Browser
                // ist Escape „das Letzte abbrechen", und nach einer
                // Suche ist das Letzte die Suche.
                if self.suche_merken.is_some() {
                    self.suche_ende();
                    return;
                }
                // Escape: zurück zur gerade gezeigten Adresse. Der
                // Nutzer hat sich verlaufen — das ist die häufigste
                // Bedeutung von Escape in einem Browser.
                let aktuell = self.browser.adresse().to_string();
                if !aktuell.is_empty() {
                    self.chrome.setze_adresse(&aktuell);
                    self.leiste_im_fokus = false;
                    self.schmutzig = true;
                }
            }
            _ => {}
        }
    }

    /// Mehrzeiliger Ablagetext wird in der einzeiligen Adressleiste zusammengefuehrt.
    pub fn adress_text_einfuegen(&mut self, text: &str) {
        for c in text.chars().filter(|c| !c.is_control()).take(8192) {
            self.chrome.einfuegen(c);
        }
        self.vorschlaege_aktualisieren();
        self.schmutzig = true;
    }

    /// Ein getipptes Zeichen.
    ///
    /// EINE REGEL, und sie ist in jedem echten Browser gleich: wer in
    /// der Adressleiste tippt, ersetzt. Wer auf der Seite ist und tippt,
    /// schreibt an den Cursor.
    ///
    /// GEMESSENER FEHLER 29.09.2026, gefunden von
    /// `getippte_adresse_erscheint_im_bild`: nach dem Laden stand
    /// `narsil://start` in der Leiste, und ein getipptes `T` landete
    /// DANACH — die Leiste enthielt `narsil://startTest`. Jemand, der eine
    /// neue Adresse eintippt, bekam damit nie eine gueltige Adresse.
    ///
    /// Die Regel dafuer: die Leiste ist nach einem Ladevorgang NICHT im
    /// Fokus, sondern zeigt nur, wo man ist. Der erste Tastendruck
    /// wechselt in den Fokus und loescht den alten Text. `Strg+L` macht
    /// dasselbe, ohne dass man tippen muss.
    fn tippe(&mut self, c: char) {
        // W7C A1: ein getipptes Zeichen beendet den Verlaufskandidaten —
        // wer weiterschreibt, meint eine eigene Adresse, nicht die von
        // vorhin.
        self.leiste_verlauf_pos = None;
        if !self.leiste_im_fokus {
            self.chrome.leeren();
            self.leiste_im_fokus = true;
        }
        self.chrome.einfuegen(c);
        //
        // KONTEXT (06.10.2026, 09:42): NACH DEM ZEICHEN DIE **LISTE** neu
        // rechnen. GEMESSENE ENTSCHEIDUNG, warum hier und nicht im
        // `Key::Char`-Zweig: die Leertaste tippt **hier** (`Key::Space` geht
        // nach `tippe(' ')`) — eine Liste, die bei „a“ rechnet und bei „a b“
        // nicht, ist eine Liste, die bei jeder zweiten Eingabe falsch ist.
        self.vorschlaege_aktualisieren();
        self.schmutzig = true;
    }

    /// DIE VORSCHLAGSLISTE **NEU RECHNEN** — aus dem, was gerade in der
    /// Leiste steht.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **leere Eingabe** die Liste **schliesst**
    /// und nicht „alles“ anzeigt: eine Liste mit allem steht im Weg, und in
    /// Firefox auch.
    pub fn vorschlaege_aktualisieren(&mut self) {
        let eingabe = self.chrome.adresse.trim().to_string();
        if !self.leiste_im_fokus || eingabe.is_empty() {
            self.vorschlaege_wahl = None;
            return;
        }
        // GEMESSENE ENTSCHEIDUNG, warum **8** Zeilen: die Liste steht **unter**
        // der Leiste und darf das Fenster nicht halb verdecken — 8 Zeilen sind
        // 224 px, mehr als die halbe Leistenhoehe von 58 px deutlich, weniger
        // als ein halber Bildschirm.
        const ZEILEN: usize = 8;
        let kandidaten = self.vorschlags_kandidaten();
        let v = vorschlaege(&kandidaten, &eingabe, ZEILEN);
        self.vorschlaege_wahl = if v.is_empty() {
            None
        } else {
            Some(Vorschlagswahl::aus_vorschlaegen(&v))
        };
        // GEMESSENE ENTSCHEIDUNG, warum die **alte Markierung** verloren geht:
        // die Liste ist eine **andere** Liste; eine Markierung „Zeile 2“ aus der
        // vorigen waere hier eine andere Adresse.
        self.vorschlaege_wahl.as_mut().map(|w| {
            w.markiert = None;
        });
    }

    /// W7C A1: DIE SCROLL-TASTEN. `true`, wenn die Taste den Viewport
    /// bewegt hat.
    ///
    /// | Taste | Leiste im Fokus | Ergebnis |
    /// |---|---|---|
    /// | Bild auf / Bild ab | egal | ein Bildschirm (Sicht hoehe minus 40) |
    /// | Pos1 / Ende | nein | an den Anfang / ans Ende der Seite |
    /// | Pos1 / Ende | ja | Cursor an den Anfang / ans Ende der Leiste |
    /// | Pfeil hoch / runter | ja | ein Eintrag zurueck / vor im Verlauf |
    /// | Pfeil hoch / runter | nein | 40 px wie ein Radschlag |
    /// | Leertaste | nein | eine Seite abwaerts |
    /// | Leertaste | ja | ein Leerzeichen in der Leiste |
    ///
    /// GEMESSEN, warum „ein Bildschirm“ und nicht „eine Zeile“: 40 px
    /// bei Bild auf ist eine Taste, die man druecken muss 30 mal, um eine
    /// Seite zu lesen. Die Leertaste in jedem Browser blättert eine Seite.
    fn scroll_taste(&mut self, k: KeyEvent) -> bool {
        if !k.pressed {
            return false;
        }
        //
        // UI1 (03.10. 10:05): `Alt+Home` GEHOERT NICHT ZUM SCROLLEN.
        //
        // GEMESSENER FEHLER: `Alt+Home` (Startseite, UI1) kam nie an,
        // weil diese Funktion VOR dem Start-Knopf-Zweig laeuft und
        // `Key::Home | Key::End` ohne jede Modulator-Pruefung annimmt. Der
        // Test `ui1_die_taste_alt_home_zeigt_die_startseite` war rot und
        // die Adresse blieb stehen — wer nach der Bindung gesucht hat,
        // ohne den Reihenfolgenamen zu lesen, haelt sie fuer tot.
        //
        // GEMESSENE ENTSCHEIDUNG, warum NUR `Home` und nicht „alles mit
        // Alt": `Alt+End` hat keine Bindung, und `Alt+PageUp`/
        // `Alt+PageDown` zu aendern waere eine Aenderung ohne Messung
        // (kein Test, keine Behauptung). Diese eine Taste ist der
        // gemessene Fall — und die Inkonsistenz ist benannt statt
        // wegdefiniert.
        if k.key == Key::Home && k.modifiers.only_alt() {
            return false;
        }
        let sicht = self.sicht_hoehe() as i64;
        //
        // KONTEXT (05.10.2026, 00:40): DER SEITENSCHRITT kommt aus der gruenen
        // Crate (`seiten_schritt`). GEMESSENER BEFUND, warum das ein Gewinn ist:
        // hier stand `(sicht - 40).max(40)` — **zwei** Bedeutungen in **einer**
        // Zahl (Rand und Mindestschritt), und der Mindestschritt war nirgends
        // als solcher benannt.
        let blatt = seiten_schritt(sicht);
        //
        // DER PDF-BETRACHTER ZUERST (03.10.2026, 5:3x). GEMESSENER
        // BEFUND: `scroll_pixels` arbeitet auf `self.angezeigt`, und bei
        // einem PDF ist das `None` — die Tasten scrollten also gar nichts.
        // Das Rad war um 23:5x verdrahtet, die Tasten nicht; GEMESSEN habe
        // ich das erst, als ich nach dem Rad-Test (`_RADTEST`) die
        // Tastenzweige gelesen habe. GEMESSENE ENTSCHEIDUNG, warum die
        // Abfrage VOR dem `match` steht und nicht in jedem Zweig: sechs
        // Zweige (Bild ab, Bild auf, Pos1, Ende, Pfeil auf, Pfeil runter,
        // Leertaste) brauchen dieselbe eine Zeile, und sie an sieben Stellen
        // zu schreiben waere sieben Stellen, an denen sie fehlen kann.
        //
        // GEMESSENE ENTSCHEIDUNG, warum Pos1 und Ende hier ANFANG/ENDE
        // rufen und nicht `scroll_pixels`: ein PDF hat keine Zeilen, es hat
        // Seitenhoehe — `Viewer::anfang` und `::ende` klemmen am Ueberstand
        // des Blatts, und das ist die Zahl, die hier gilt.
        //
        // KONTEXT (05.10.2026, 00:25): DIE **TASTE** ZU EINEM **WUNSCH** —
        // `scroll_taste` (gruene Crate, 8 Tests). GEMESSENER BEFUND, warum:
        // die beiden Aeste (Seite und PDF) hatten **verschiedene** Leertasten —
        // die Seite sah den **Umschalt** nicht, das PDF schon — und die Zahl 40
        // stand an fuenf Stellen als Zaehlung. Jetzt entscheidet **eine**
        // Funktion, und die beiden Aeste **tun** nur noch.
        let wunsch = Self::scrolltaste(k.key).map(|t| {
            let seiten_px = blatt.clamp(-MAX_RADSCHRITT, MAX_RADSCHRITT) as i32;
            scroll_taste(
                t,
                k.modifiers.shift,
                self.leiste_im_fokus,
                self.feld_fokus.is_some(),
                seiten_px,
            )
        });
        let Some(wunsch) = wunsch else {
            return false;
        };
        //
        // DER PDF-BETRACHTER ZUERST (03.10.2026, 5:3x). GEMESSENER BEFUND
        // (unveraendert): `scroll_pixels` arbeitet auf `self.angezeigt`, und bei
        // einem PDF ist das `None` — die Tasten scrollten also gar nichts. Es
        // gibt fuer den Wunsch also **zwei** Ausfuehrer: den Bildschirm
        // (`scroll_pixels`, `springe_an`/`springe_ans_ende`) und den
        // Betrachter (`Viewer::scrollen`/`anfang`/`ende`).
        if self.pdf.is_some() {
            let (breite, hoehe) = (self.breite.max(1), self.hoehe.max(1));
            let mut bewegt = false;
            if let Some(v) = self.pdf.as_mut() {
                bewegt = match wunsch {
                    Scrollwunsch::Seiten(px) | Scrollwunsch::Zeilen(px) => {
                        v.scrollen(0, px, &self.chrome, breite, hoehe)
                    }
                    Scrollwunsch::Anfang => v.anfang(&self.chrome, breite, hoehe),
                    Scrollwunsch::Ende => v.ende(&self.chrome, breite, hoehe),
                    // GEMESSENE ENTSCHEIDUNG, warum die **Leisten**-Wuensche hier
                    // `false` sind: sie gehoeren dem **Bild** und nicht dem
                    // Betrachter — ein Tastendruck, der die Adressleiste trifft,
                    // scrollt kein Blatt.
                    _ => false,
                };
            }
            self.schmutzig = true;
            return bewegt;
        }
        match wunsch {
            Scrollwunsch::Zeilen(px) | Scrollwunsch::Seiten(px) => {
                self.scroll_pixels(px as i64);
                true
            }
            //
            // GEMESSENER FEHLER (00:35, `tests/bedienung.rs`): Pos1 war
            // `scroll_pixels(-sicht)` — also EIN Bild hoch, nicht nach oben. In
            // jedem Browser heisst Pos1 „ganz nach oben“, und das ist eine
            // absolute Zahl, kein Schritt.
            Scrollwunsch::Anfang => {
                self.springe_an(0);
                true
            }
            Scrollwunsch::Ende => {
                self.springe_ans_ende();
                true
            }
            // In der Leiste: der Cursor springt, wie in jedem Textfeld (sonst
            // koennte man eine lange Adresse nicht an ihr Ende bringen).
            Scrollwunsch::LeisteAnfang => {
                self.chrome.cursor_ende(false);
                true
            }
            Scrollwunsch::LeisteEnde => {
                self.chrome.cursor_ende(true);
                true
            }
            // In der Leiste: der Verlauf (Pfeil hoch/runter).
            Scrollwunsch::Verlauf(zurueck) => {
                self.verlauf_gehe(zurueck);
                true
            }
            Scrollwunsch::Nichts => false,
        }
    }

    /// WELCHE **TASTE** ist eine Scrolltaste? (`None` = ist keine)
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `Key` hier **nicht** durchgereicht wird:
    /// `narsil_input::Key` ist ein **Fremdtyp**, und eine Rechnung, die einen
    /// Fremdtyp in ihrer Signatur hat, ist in keiner anderen Crate pruefbar.
    fn scrolltaste(k: narsil_input::Key) -> Option<Scrolltaste> {
        Some(match k {
            narsil_input::Key::Up => Scrolltaste::Auf,
            narsil_input::Key::Down => Scrolltaste::Ab,
            narsil_input::Key::PageUp => Scrolltaste::BildAuf,
            narsil_input::Key::PageDown => Scrolltaste::BildAb,
            narsil_input::Key::Home => Scrolltaste::Pos1,
            narsil_input::Key::End => Scrolltaste::Ende,
            narsil_input::Key::Space => Scrolltaste::Leertaste,
            _ => return None,
        })
    }

    /// W7C A1: DIE ADRESSSLEISTE DURCH DEN VERLAUF GEHEN (Pfeil hoch/runter).
    ///
    /// GEMESSEN, warum der Verlauf aus `Browser` kommt und nicht aus
    /// einem eigenen Vec in der Sitzung: es gibt einen Verlauf, und er
    /// ist der des Browsers. Ein zweiter waere eine zweite Wahrheit ueber
    /// „wo war ich“.
    fn verlauf_gehe(&mut self, hoeher: bool) {
        let eintraege: Vec<String> = self
            .browser
            .verlauf()
            .into_iter()
            // GEMESSEN, warum die LEEREN Eintraege raus: `Browser::neu`
            // beginnt mit `Page::leer()` (die Crate sagt „`verlauf` ist
            // nie leer"). Mit dem leeren Eintrag in der Liste zeigt
            // Pfeil hoch auf einen Tab, dessen Adresse "" ist — und
            // Enter darauf laedt eine Seite ohne Adresse. In keinem
            // Browser steht ein leerer Eintrag in der Liste.
            .filter(|a| !a.is_empty())
            .map(|a| a.to_string())
            .collect();
        if eintraege.is_empty() {
            self.meldung = Some(tr("The history is empty").into());
            return;
        }
        // Startpunkt: die gerade gezeigte Adresse. Ohne `verlauf_pos`
        // ist das der LETZTE Eintrag (der aktuelle).
        let start = self
            .leiste_verlauf_pos
            .unwrap_or(eintraege.len().saturating_sub(1))
            .min(eintraege.len() - 1);
        let neu = if hoeher {
            start.saturating_sub(1)
        } else {
            (start + 1).min(eintraege.len() - 1)
        };
        self.leiste_verlauf_pos = Some(neu);
        self.chrome.setze_adresse(&eintraege[neu]);
        self.leiste_im_fokus = true;
        self.meldung = Some(trf!(
            "History {}/{}: {}",
            neu + 1,
            eintraege.len(),
            eintraege[neu]
        ));
    }

    /// Rad: nach unten ist positiv (X11 liefert 5 fuer "rad runter").
    /// DER RAD-WERT ALS **PIXEL** — GEMESSENE ENTSCHEIDUNG, warum **hier** und
    /// nicht im Kette-Zweig: GEMESSEN ist, dass `scrollen` genau diese Rechnung
    /// macht (`Lines(n) => n * 40`, `Pixels(n) => n`), und ein zweiter Ort fuer
    /// dieselbe Zahl waere eine Zahl, die auseinanderlaufen kann — ein Rad,
    /// das auf der Seite 40 Zeilen geht und im Kasten 39.
    /// KONTEXT (05.10.2026, 00:30): DIE RAD-RECHNUNG LIEGT **JETZT** IN DER
    /// GRUENEN CRATE (`rad_pixels`) — GEMESSEN ist, dass sie hier **zweimal**
    /// stand (dieser Ast **ohne** Klemmung, der Zweig in `scrollen` **mit**)
    /// und dass die ungeklemmte nicht nur doppelt, sondern **kaputt** war: ein
    /// `Pixels(1.0e30)` wurde `i64::MAX` und lief beim Addieren ueber.
    pub fn rad_als_pixels(&self, delta: narsil_window::ScrollDelta) -> i64 {
        rad_pixels(Self::rad_dreh(delta))
    }

    /// DER **RAD-DREH** des Fensters in der Form der Rechnung (die
    /// Fenstertypen gehoeren dem Fenster).
    fn rad_dreh(delta: narsil_window::ScrollDelta) -> RadDreh {
        match delta {
            narsil_window::ScrollDelta::Lines(n) => RadDreh::Zeilen(n),
            narsil_window::ScrollDelta::Pixels(n) => RadDreh::Pixel(n),
        }
    }

    fn scrollen(&mut self, delta: narsil_window::ScrollDelta) {
        // VOR der Leihmutter lesen: `self.sicht_hoehe()` braucht `&self`,
        // `self.angezeigt.as_mut()` haelt schon `&mut self`. Der Compiler
        // laesst beides nicht gleichzeitig zu, und er hat recht — die
        // Reihenfolge ist hier keine Formalie.
        //
        // DER PDF-BETRACHTER ZUERST (03.10.2026, 3:5x, `VORLAGE.txt` Z. 10:
        // „Bildlaufleiste"). GEMESSENER BEFUND: `scroll_pixels` arbeitet auf
        // `self.angezeigt`, und bei einem PDF ist das `None` — das Rad tat
        // also NICHTS. GEMESSEN, wie gross das nichts war: bei 500 % ist
        // das Blatt 3067 x 4340 px bei 1888 x 868 px sichtbar, also lagen
        // 4651 px ausserhalb (1179 px davon seitlich) und es gab keinen Weg
        // zu ihnen. GEMESSENE ENTSCHEIDUNG, warum die Abfrage vor der
        // Leihmutter steht: `self.pdf` und `self.chrome` sind eigene Felder,
        // aber der Aufrufer unten braucht `&mut self.pdf` — dieselbe
        // Reihenfolge wie bei `sicht_hoehe()`, aus demselben Grund.
        if self.pdf.is_some() {
            self.pdf_scrollen(delta);
            return;
        }
        //
        // GEMESSENER FEHLER 30.09.2026, 07:20 (`tests/bomben.rs`):
        // `ScrollDelta::Pixels(1.0e30)` ergab `1e30.round() as i64` = der
        // groesste `i64`, und `alt + schritt` lief ueber ("attempt to
        // add with overflow"). Im `debug`-Profil panict das, im
        // `release`-Profil waechst es still.
        //
        // ZWEI DINGE SIND HIER FALSCH GEWESEN, und beide sind es noch:
        // 1. `as i64` von einem `f32` ueber `i64::MAX` ist
        //    UNGEPRUEFT — Rust gibt den Grenzwert zurueck, kein `None`.
        // 2. Das Addieren ohne Sättigung. `saturating_add` haette auch
        //    funktioniert, aber `MAX_SCHRITT` macht das Problem VOR dem
        //    Rechnen unsichtbar: ein Rad mit 1e30 Pixeln ist ein
        //    Ereignisfehler, kein Scrollen.
        //
        // `MAX_SCHRITT` (M3) ist der Wert fuer EIN Ereignis. Groesser
        // als der Fenstersprung kann ein Ereignis nicht sinnvoll sein,
        // also wird es auf diesen Wert gesetzt — sichtbar, benannt,
        // pruefbar.
        // GEMESSENE ENTSCHEIDUNG, warum `rad_pixels` **ohne** `.clamp` hier
        // steht: es klemmt **selbst** (`MAX_RADSCHRITT`), damit es nur eine
        // Grenze fuer diese Rechnung gibt. Der frueher hier stehende
        // `MAX_SCHRITT` ist derselbe Wert — als **zweite** Grenze an zweiter
        // Stelle.
        let schritt = rad_pixels(Self::rad_dreh(delta));
        //
        // KONTEXT (04.10.2026, 19:40, Leitung: „stelle ihn mit Begruendung
        // richtig (Eigentuemer im BRETT informieren)“): DAS MAUSRAD SCROLLT
        // **SANFT**. GEMESSENE ENTSCHEIDUNG, warum der **Zielwert** gerechnet
        // und nicht der Schritt addiert wird: die Bewegung hat einen **Start**
        // (`von`) und ein **Ende** (`nach`), und beide muessen geklemmt sein —
        // ein Zwischenschritt, der geklemmt wird, laeuft aus dem Takt.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das Rad der **Tasten** vorausgeht und
        // umgekehrt nicht: GEMESSEN ist, dass `narsil/tests/browser.rs:228` das
        // Rad festhält (1000 Klicks, dann `versatz == max_v`), die **Tasten**
        // aber in `tests/bedienung.rs` (Bild runter > 300 px **sofort**, Ende
        // **sofort** am Ende). Die Leitung hat den einen Test zum Aelteren
        // erklaert; fuer die Tasten hat sie das **nicht** gesagt — darum
        // bleiben die Tasten sofort (GEMESSEN: `scroll_taste` ruft
        // `scroll_pixels`), und nur das Rad wird sanft. Im BRETT (19:50) an UI.
        if schritt == 0 { return; }
        let jetzt = std::time::Instant::now();
        let sicht = self.sicht_hoehe();
        let stand = self.scroll_ani.map_or_else(
            || self.angezeigt.as_ref().map_or(0, |a| a.versatz),
            |ani| if self.scroll_rad { ani.rad_versatz_bei(jetzt) } else { ani.versatz_bei(jetzt) });
        // Gleichgerichtete Impulse addieren zum ausstehenden Ziel. Bei
        // Richtungswechsel reagieren wir sofort vom erreichten Stand aus.
        let basis = self.scroll_ani.filter(|ani|
            (ani.nach as i64 - stand as i64).signum() == schritt.signum())
            .map_or(stand, |ani| ani.nach);
        if let Some(a) = self.angezeigt.as_mut() { a.versatz = stand.min(a.max_versatz(sicht)); }
        let neu = (basis as i64).saturating_add(schritt).clamp(0, i64::from(u32::MAX)) as u32;
        self.scroll_ziel_setzen(neu, jetzt);
        self.scroll_rad = true;
    }

    /// DAS RAD IM PDF-BETRACHTER (`VORLAGE.txt` Z. 10, „Bildlaufleiste").
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine eigene Funktion ist und kein
    /// Zweig in [`Sitzung::scrollen`]: der PDF-Versatz rechnet in
    /// Bildschirmpixeln des Blatts und klemmt am Ueberstand, die Seite
    /// rechnet in Zeilen des Layouts und klemmt an `max_versatz`. Zwei
    /// Klemmungen an einem Ort waeren eine dritte Wahrheit.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die KLEMMUNG hier doppelt geprueft
    /// wird (unten in `pdf_scroll`) und trotzdem hier: `MAX_SCHRITT` gilt
    /// fuer das EREIGNIS, und ein Rad von 1e30 Pixeln ist ein
    /// Ereignisfehler. Die Klemmung am Ueberstand ist eine andere Frage —
    /// sie gehoert an den Zustand.
    fn pdf_scrollen(&mut self, delta: narsil_window::ScrollDelta) {
        /// M3: der groesste Weg eines EREIGNISSES (in Pixeln). Groesser als
        /// der Fenstersprung kann ein Ereignis nicht sinnvoll sein.
        const MAX_SCHRITT_PDF: i32 = 4096;
        let roh = match delta {
            narsil_window::ScrollDelta::Lines(n) => (n * 40.0).round() as i64,
            narsil_window::ScrollDelta::Pixels(n) => n.round() as i64,
        };
        let schritt = roh.clamp(-(MAX_SCHRITT_PDF as i64), MAX_SCHRITT_PDF as i64) as i32;
        let (breite, hoehe) = (self.breite.max(1), self.hoehe.max(1));
        if let Some(v) = self.pdf.as_mut() {
            v.scrollen(0, schritt, &self.chrome, breite, hoehe);
        }
        self.schmutzig = true;
    }

    /// WAS IN DER STATUSZEILE STEHT (UI1, 03.10. 10:50).
    ///
    /// DIE REIHENFOLGE, und sie ist eine Frage nach Wichtigkeit:
    ///
    /// 1. DIE MELDUNG — sie ist die Zusage des Browsers an den Nutzer
    ///    („Formular: HTTP 500", „Datei nicht gefunden"). Sie gehoert
    ///    nach vorn, weil sie das Einzige ist, was den Nutzer vom
    ///    Schaden abhaelt.
    /// 2. DIE KONSOLE — die Meldungen des Skripts der Seite.
    /// 3. DIE LADEZEIT — die Zahl, die man sehen will, wenn eine Seite
    ///    langsam war. Sie steht hinten, weil sie die interessanteste
    ///    Zahl ist, wenn sie schlecht ist.
    ///
    /// GEMESSENE BEGRUENDUNG, warum die Ladezeit nicht VORN steht: die
    /// Meldung ist ein Fehler, die Ladezeit ist eine Tatsache. Ein
    /// Browser, der „0,4 s" vor einer Fehlermeldung zeigt, traegt dem
    /// Nutzer die Reihenfolge seiner Sorgen auf — und die ist: was ist
    /// kaputt?
    ///
    /// Release 07.10.2026: die KONSOLE DER SEITE steht nicht mehr hier.
    /// GEMESSEN am Korpus: auf 7 von 12 Seiten stand ein Skriptfehler der
    /// Seite („TypeError: … is not a function") dauerhaft in der Leiste —
    /// das ist eine Entwicklerangabe, kein Zustand des Browsers. Firefox
    /// zeigt sie nur in der Konsole; hier ebenso (DevTools, `Angezeigt::konsole`).
    fn status_text(&self) -> String {
        if let Some(m) = self.meldung.as_ref().filter(|m| !m.trim().is_empty()) {
            return m.clone();
        }
        self.ladezeit_text()
    }

    /// Ist der Statustext eine Fehlermeldung (roter Grund)?
    fn status_ist_fehler(text: &str) -> bool {
        let t = text.to_lowercase();
        [
            "fehler", "nicht moeglich", "nicht möglich", "nicht ladbar", "nicht zu oeffnen", "fehlgeschlagen", "error",
            // English messages (the UI language is English by default).
            "cannot", "could not", "failed", "not possible",
        ]
            .iter()
            .any(|w| t.contains(w))
    }

    /// DIE LADEZEIT — oder nichts.
    ///
    /// GEMESSENER ZUSTAND BIS 11:30: das hier war LEER, weil die Sitzung
    /// kein Feld fuer die Ladedauer hatte. `narsil_layout::Messung` traegt
    /// Phasen (`w0`, `js`, `laden_netz` …), aber die wird nur bei
    /// `NARSIL_ZEITEN=1` ausgegeben und nicht gehalten (W6B hat das
    /// getrennt, um die 10,75 s zu finden, die niemand brauchte). Seit
    /// 11:30 steht sie in [`Sitzung::letzte_lade_ms`], und `lade_jetzt`
    /// fuellt sie an allen drei Rueckwegen (Erfolg, Netzfehler, leere
    /// Adresse).
    ///
    /// GEMESSENE BEGRUENDUNG fuer die Schwelle 300 ms: darunter ist die
    /// Zahl gerundet und waere eine Behauptung ueber eine Genauigkeit,
    /// die es nicht gibt („12 ms" fuer ein lokales `file://` ist gerundet
    /// aus 11,8). Der Nutzer will die Zeit sehen, wenn sie schlecht ist,
    /// und 300 ms ist die Grenze, ab der „langsam" jemandem auffaellt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier eine LEERE Zusage steht und
    /// kein Platzhalter: ein Platzhalter („—") in einer Zeile, die man
    /// liest, ist eine Behauptung ueber eine Zahl, die es nicht gibt.
    /// GEMESSENER WEG, sie zu fuellen (H4): `lade_jetzt` misst die
    /// Dauer mit `std::time::Instant` und legt sie in EIN Feld der
    /// Sitzung; dort gehoert sie hin, weil es dieselbe Zahl ist, die
    /// `lade_jetzt` erzeugt.
    fn ladezeit_text(&self) -> String {
        const AB_MS: u32 = 300;
        let ms = self.letzte_lade_ms;
        if ms < AB_MS {
            return String::new();
        }
        std::format!("{ms} ms")
    }

    /// W7C A1: AN EINE ABSOLUTE POSITION SPRINGEN (Pos1/Ende).
    ///
    /// GEMESSEN, warum das nicht `scroll_pixels` mit einer grossen Zahl
    /// ist: die Klemmung sitzt in `scroll_pixels`, und ein Sprung ueber
    /// 1e9 Pixel waere ein Schritt, kein Sprung. Hier steht die Klammer
    /// noch einmal — aber sie ist die GLEICHE Formel, und beide stehen
    /// nebeneinander, damit man sie vergleichen kann.
    fn springe_an(&mut self, position: u32) {
        let sicht = self.sicht_hoehe();
        let Some(a) = self.angezeigt.as_mut() else {
            return;
        };
        a.versatz = position.min(a.max_versatz(sicht));
        drop(a);
        // H3 (03.10.): auch hier — Ende/Pos1 sind genauso ein Weg, den
        // Sichtbereich zu aendern, wie das Mausrad.
        //
        // GEMESSENE WAHRHEIT ueber den Kommentar darueber: `scroll_pixels`
        // ist NICHT der einzige Ort, an dem `versatz` bewegt wird — dieser
        // hier ist der zweite. Der alte Kommentar behauptete das Gegenteil
        // („der EINZIGE Ort"), und die Behauptung war falsch. Der
        // Nachladeweg sitzt jetzt an beiden, was die Behauptung richtig und
        // die Fehlerklasse „zwei Stellen entscheiden" sichtbar macht.
        self.bilder_sichtbar_laden();
    }

    /// Springt zu einem Anker (`#id`), wenn das Ziel **dieselbe Seite** ist.
    ///
    /// 02.10. 07:55. GEMESSENER BEFUND (07:54): ein Klick auf `<a href="#ziel">`
    /// wurde angenommen und tat nichts — `versatz` blieb 0, die Adresse blieb,
    /// und es kam keine Meldung. Siehe die Stelle im Klickpfad.
    ///
    /// GEMESSENE REGEL, wann gesprungen wird:
    ///
    /// | Ziel                            | gemessen                          |
    /// |---------------------------------|-----------------------------------|
    /// | `#ziel`                         | Sprung auf `ziel`                 |
    /// | `<dieselbe adresse>#ziel`       | Sprung auf `ziel`                 |
    /// | `<andere seite>.html#ziel`      | **Laden** (Browser-Verhalten)     |
    /// | `#gibt_es_nicht`                | **Laden** (Browser-Verhalten)     |
    ///
    /// GEMESSENE EINORDNUNG, warum die letzten beiden laden: genau das machen
    /// die Browser — ein Sprung ist nur moeglich, wenn es das Ziel schon im
    /// Dokument gibt. Und es ist die **sparsamere** Regel: sie braucht keine
    /// Liste von Sonderfaellen, sondern eine Frage („steht das Ziel in diesem
    /// Dokument?") und ein `ja`.
    ///
    /// GEMESSENE EINORDNUNG, warum die **Oberkante** des Ziels das Ziel des
    /// Sprungs ist: so zeigen es die Browser (Chrome, Firefox) fuer ein
    /// normales Element. `scroll-margin-top` gibt es in diesem Browser nicht
    /// (kein Anker-Offset gemessen), also gibt es auch keine Ausnahme davon.
    fn anker_sprung(&mut self, ziel: &str) -> bool {
        let aktuell = self.browser.adresse().to_string();
        // Das Dokument-Teil (ohne `#`) und das Fragment.
        let (dokument, fragment) = if let Some(rest) = ziel.strip_prefix('#') {
            (
                aktuell.split('#').next().unwrap_or("").to_string(),
                rest.to_string(),
            )
        } else {
            match ziel.split_once('#') {
                Some((d, f)) => (d.to_string(), f.to_string()),
                None => return false,
            }
        };
        if fragment.is_empty() || dokument != aktuell.split('#').next().unwrap_or("") {
            return false;
        }
        //
        // GEMESSENE ENTSCHEIDUNG (08:20), warum **nicht** `Seite::ids`:
        // `ids` nennt eine Ordnungszahl, und die tragen mehrere Knoten
        // (gemessen: `ids["ziel"] = 8`, und `dom_zu_ord` bildete `8->8`,
        // `9->8` und `10->8` ab). Der Sprung landete dadurch bei `y = 138`
        // statt bei `y = 3091` — **2953 px zu frueh**: der Nutzer haette
        // einen Text gesehen, auf den er nicht geklickt hat.
        // `id_zu_knoten` sagt genau das, was gebraucht wird: welcher Kasten.
        let Some(knoten) = self
            .angezeigt
            .as_ref()
            .and_then(|a| a.seite.id_zu_knoten.get(&fragment).copied())
        else {
            return false;
        };
        let Some(y) = self.anker_y(knoten) else {
            return false;
        };
        self.springe_an(y);
        //
        // GEMESSENE EINORDNUNG, warum `Navigation::To` und nicht ein
        // direktes Schreiben der Leiste: `narsil_browser::Browser` hat kein
        // oeffentliches `setze_adresse` — `adresse_aufrufen` ist privat. Und
        // `Navigation::To` ist genau der dokumentierte Weg fuer „Adresse in
        // den Verlauf, **ohne** zu laden" (`sitzung.rs`, 2094). GEMESSEN, dass
        // es beim Sprung nicht laedt: `anwenden` gibt nur `Wirkung` zurueck,
        // das Laden macht der Aufrufer — und das ist hier niemand.
        //
        // GEMESSENE EINORDNUNG, warum die Leiste ueberhaupt folgen muss: sie
        // ist die Antwort auf „wo bin ich" — und nach dem Sprung ist die
        // Seite dieselbe, nur an anderer Stelle. Ohne die Leiste behaupte
        // `adresse()` das Gegenteil dessen, was der Nutzer sieht.
        self.browser.anwenden(&narsil_browser::Navigation::To(format!(
            "{dokument}#{fragment}"
        )));
        self.schmutzig = true;
        true
    }

    /// Die **Oberkante** des Kastens dieses Knotens, in Seitenkoordinaten.
    ///
    /// GEMESSENE EINORDNUNG, warum der Knoten und nicht eine Zahl: siehe
    /// `Seite::id_zu_knoten` — drei Knoten koennen dieselbe Ordnungszahl
    /// tragen, und die Wahl unter ihnen war gemessen **falsch** (2953 px
    /// daneben).
    ///
    /// GEMESSENE EINORDNUNG, warum bei mehreren Kaesten mit DEMSELBEN Knoten
    /// der **letzte** zaehlt: das ist der innerste — die Tiefe wird zuerst
    /// durchlaufen, und bei einem Wrapper steht der sichtbare Inhalt innen.
    fn anker_y(&self, knoten: narsil_dom::NodeId) -> Option<u32> {
        let a = self.angezeigt.as_ref()?;
        let mut treffer: Option<f32> = None;
        fn lauf(b: &narsil_layout::LayoutBox, ziel: narsil_dom::NodeId, treffer: &mut Option<f32>) {
            if b.node == ziel {
                *treffer = Some(b.border_box.y);
            }
            for k in &b.children {
                lauf(k, ziel, treffer);
            }
        }
        lauf(&a.baum.root, knoten, &mut treffer);
        treffer.map(|y| y.max(0.0) as u32)
    }

    /// Ans Ende der Seite springen (Ende).
    fn springe_ans_ende(&mut self) {
        let sicht = self.sicht_hoehe();
        let ende = self
            .angezeigt
            .as_ref()
            .map(|a| a.max_versatz(sicht))
            .unwrap_or(0);
        self.springe_an(ende);
    }

    /// W7C A1: UM EINE ZAHL PIXEL SCROLLEN — der EINZIGE Ort, an dem
    /// `versatz` bewegt wird.
    ///
    /// GEMESSEN, warum das geteilt sein MUSS: das Mausrad, Bild auf/ab,
    /// Pos1/Ende und die Pfeiltasten sind vier Wege zum selben Ziel. Vor
    /// dieser Methode hatten sie jeder eine eigene Kopie der Klammerung —
    /// und der Test `scrollen_klemmt_an_beiden_enden` (07:20) hat
    /// gefunden, dass eine davon an einer anderen Zahl klemmte als die
    /// anderen drei. Wer das Rechnen teilt, kann es nicht mehr
    /// zweimal falsch machen.
    /// PROBE (06.10.2026 03:58): DER PRUEFSTAND SCROLLT — und das ist
    /// die **eine** Veroeffentlichung, die fuer die Bildabnahme noetig war.
    ///
    /// GEMESSENER GRUND (03:44, 30 Seiten, 196 Bilder): **161 Bilder
    /// liegen unterhalb des Ladebands** (`2 x sicht_hoehe`, Z. 4992) und
    /// wurden deshalb nicht geholt. Bis dahin war das eine **Ableitung
    /// aus der Bandformel**, keine Messung: der 30-Seiten-Lauf
    /// **scrollt nie**, also gibt es keinen Weg, die Bilder auch nur
    /// einmal zu versuchen.
    ///
    /// Warum eine Veroeffentlichung und nicht ein eigener Weg:
    /// `bilder_sichtbar_laden` ist **privat**, und es gibt genau **zwei**
    /// Aufrufer (`scroll_pixels` Z. 1677, `springe_an` Z. 1628). Der
    /// Nachladeweg an einen dritten zu haengen hiesse, die Klemmung
    /// (`max_versatz`) zu duplizieren — und die Fehlerklasse „zwei
    /// Stellen entscheiden" waere damit gebaut. **Diese eine Zeile ruft
    /// den vorhandenen Weg auf.**
    ///
    /// **WAS SIE NICHT KANN:** sie aendert nichts an `display_list` — das
    /// schiebt um `versatz` (`display_list`, Z. 6394). Wer scrollt und
    /// dann rasternt, sieht den unteren Teil der Seite. **Der
    /// Pruefstand setzt `versatz` deshalb hinterher auf 0**
    /// (`Angezeigt::versatz` ist ein oeffentliches Feld), damit das
    /// Abnahmebild dieselbe Bedeutung hat wie vorher.
    ///
    /// Rueckgabe: der neue Versatz. **Der ist das Messbare**: er bleibt
    /// stehen, wenn das Seitenende erreicht ist, und die Schleife im
    /// Pruefstand endet dann.
    pub fn scrolle_und_lade(&mut self, schritt: i64) -> u32 {
        self.scroll_pixels(schritt);
        // Expliziter synchroner Probe-/Exportweg; die Fenstergeste nutzt
        // ausschliesslich den Hintergrundlader.
        self.bilder_sichtbar_laden_sync();
        self.angezeigt.as_ref().map_or(0, |a| a.versatz)
    }

    fn scroll_pixels(&mut self, schritt: i64) {
        let sicht = self.sicht_hoehe();
        let Some(a) = self.angezeigt.as_mut() else {
            return;
        };
        let alt = a.versatz as i64;
        // Klemmen, nicht addieren: sonst scrollt man in die Leere und
        // der Inhalt ist weg, ohne dass man es merkt.
        let neu = alt.saturating_add(schritt).clamp(0, a.max_versatz(sicht) as i64);
        a.versatz = neu as u32;
        drop(a);
        //
        // KONTEXT (04.10.2026, 16:25): DAS SCROLLEN ZEIGT DEN BALKEN. GEMESSENE
        // ENTSCHEIDUNG, warum hier und nicht im Rad-Zweig: `scroll_pixels`
        // wird auch von den **Tasten** gerufen (W7C A1, `scroll_taste`), und
        // ein Balken, der nur beim Rad erscheint, waere bei Tastenscrollen
        // weg — zwei Wege, eine Wirkung.
        self.scrollbalken.zeigen(std::time::Instant::now());
        //
        // H3 (03.10.): DIE LAZY-BILDER NACHLADEN.
        //
        // GEMESSEN, warum hier und nicht im Zeichnen: `BildCache::hole`
        // geht ins Netz, und ein Fenster zeichnet 30mal in der Sekunde.
        // Der Weg beim Scrollen ist der einzige, bei dem sich der
        // Sichtbereich geaendert hat — und er wird genau einmal pro
        // Bildlaufgeruf ausgefuehrt.
        //
        // GEMESSENE KOSTE: `hole` holt nur, was NICHT im Cache liegt
        // (`bilder.rs:317`), und `bilder_sichtbar_laden` gibt nur die
        // sichtbaren Adressen weiter. Ein Frame, in dem sich nichts
        // bewegt hat, kostet eine Schleife ueber `MAX_BILDER` und sonst
        // nichts.
        self.bilder_sichtbar_laden();
    }

    /// Linksklick: erst die Leiste, dann die Seite.
    ///
    /// REIHENFOLGE, und sie ist nicht verhandelbar: die Leiste liegt
    /// OBEN (y=0..32), die Seite darunter. Wer erst die Seite prueft,
    /// hat bei y=10 beides getroffen und wuerde den Knopf statt des
    /// Textes bedienen.
    fn mausklick(&mut self, punkt: Point) {
        self.tabs_anzeigen();
        // Release 08.10.2026: der Hinweis der Features liegt ueber allem; ein
        // Klick darauf oeffnet sein Ziel (Liste, Kuendigungsvorlage).
        if self.menue.is_none() && self.tabmenue.is_none() && self.features_klick(punkt) {
            return;
        }
        // Das Popup liegt ueber der Seite; seine Eintraege liegen unter der Leiste.
        if self.leisten_menue.is_some() {
            let klick = crate::leiste_klick::leisten_werkzeug(
                &mut self.chrome, self.breite, self.hoehe, &punkt,
                &mut self.leisten_menue,
            );
            self.schmutzig = true;
            match klick {
                crate::leiste_klick::LeistenKlick::MenueWahl(aktion) => {
                    self.menue_aktion(aktion);
                    return;
                }
                crate::leiste_klick::LeistenKlick::MenueZu => return,
                _ => {}
            }
        }

        //
        // UI4 (03.10. 09:30): DAS MENUE ZUERST.
        //
        // GEMESSENE BEGRUENDUNG fuer die Reihenfolge: solange das Menue
        // offen ist, geht der Klick an das Menue — sonst wuerde der
        // Nutzer, der auf 'Link oeffnen' klickt, den Link DIREKT
        // ausloesen (die Seite waere schneller geladen, aber der Klick
        // waere an der falschen Stelle gelandet, und die Meldung, dass der
        // Link geoeffnet wurde, faellt weg). `menue_klick` gibt zurueck,
        // OB es den Klick verbraucht hat: bei einem Klick daneben geht er
        // an die Seite weiter — und schliesst das Menue.
        //
        // GEMESSENER FEHLER MEINES ERSTEN ENTWURFS (10:10): ich habe hier
        // `self.menue.take()` aufgerufen und dann `menue_klick` — das
        // Menue war damit schon weg, `menue_klick` sah `None` und gab
        // immer `false` zurueck. GEMESSEN: der Klick auf „Link oeffnen"
        // navigierte nicht, die Adresse blieb leer und die Meldung blieb
        // leer (der Test `ui4_link_oeffnen_navigiert`).
        //
        // DER WEG: `menue_klick` loescht das Menue selbst (es weiss, ob
        // es den Klick verbraucht hat), und dieser Ort fragt nur vorher,
        // OB ueberhaupt eines offen war.
        //
        // KONTEXT (04.10.2026, 06:30): DAS TABMENUE ZUERST — es ist das
        // juengere der beiden und liegt ueber der Seite.
        let tabmenue_auf = self.tabmenue.is_some();
        if self.tabmenue_klick(punkt.x, punkt.y) {
            return;
        }
        //
        // KONTEXT (04.10.2026, 16:35, Leitung „(1) SICHTBARER Scrollbalken
        // rechts (Griff, ziehbar, Klick auf Schiene springt seitenweise)“):
        // DER KLICK AUF DEN **BALKEN**. GEMESSENE ENTSCHEIDUNG, warum er **nach**
        // den Menues steht: solange ein Menue offen ist, gehoert der Klick dem
        // Menue — ein Klick auf den Balken wuerde das Menue schliessen und den
        // Balken zugleich anfassen.
        if self.scrollbalken_klick(punkt) {
            return;
        }
        //
        // KONTEXT (04.10.2026, 23:55): DER KLICK AUF DIE **TABLESTISTE**
        // startet das Ziehen eines Tabs. GEMESSENE ENTSCHEIDUNG, warum das
        // **hier** und nicht in der Leisten-Zeichnung: GEMESSEN ist, dass der
        // Klick in `mausklick` landet (das prueft die Menue-Bedingungen), und
        // `leiste_klick.rs` ist **fremd** — die Leiste wuerde den Klick sonst
        // fuer sich behalten und der Tab liege sich nie.
        //
        // GEMESSENE ENTSCHEIDUNG, warum **nur** der linke Knopf zieht: der
        // rechte ist das Kontextmenue, der mittlere ein neuer Tab — beide stehen
        // schon an anderer Stelle, und ein Ziehen, das den Klick verbraucht,
        // waere eine dritte Bedeutung fuer denselben Knopf.
        for nr in 0..self.browser.tabs() {
            let Some(r) = self.chrome.tab(nr) else {
                continue;
            };
            if punkt.x >= r.x as f32
                && punkt.x < r.rechts() as f32
                && punkt.y >= r.y as f32
                && punkt.y < r.unten() as f32
            {
                if self.chrome.tab_schliessen(nr).is_some_and(|r| treffer(&r, punkt)) {
                    self.tab_wechseln_zu(nr as u32);
                    self.tab_schliessen();
                    return;
                }
                // Der Klick AKTIVIERT den Tab (wie in Firefox) und merkt
                // sich in `Chrome::zug` den Anfang eines moeglichen Zugs.
                // UMGE STELLT wird erst beim Loslassen: ein Klick mit ein
                // paar Pixern Zittern darf die Reihenfolge nicht aendern.
                self.tab_wechseln_zu(nr as u32);
                self.leiste_im_fokus = false;
                self.chrome.zug_beginnen(nr, punkt.x as i64);
                self.schmutzig = true;
                return;
            }
        }
        //
        // KONTEXT (04.10.2026, 21:35, Leitung „TEXTAUSWAHL mit Maus (Ziehen,
        // Doppelklick=Wort, Dreifachklick=Absatz, Shift+Klick erweitert)“):
        // DER **KLICK AUF DIE SEITE**. GEMESSENE ENTSCHEIDUNG, warum er **nach**
        // dem Scrollbalken und **vor** allem anderen steht: solange der Balken
        // getroffen ist, gehoert der Klick ihm; danach ist jede andere Stelle
        // **Text** (oder der leere Raum daneben).
        self.auswahl_klick(punkt);
        let menue_auf = self.menue.is_some();
        if self.menue_klick(punkt.x, punkt.y) {
            return;
        }
        if tabmenue_auf {
            self.schmutzig = true;
        }
        if menue_auf {
            self.schmutzig = true;
        }
        //
        // D9 (01.10.2026): DER WERKZEUGBEREICH ZUERST.
        //
        // GEMESSENE BEGRUENDUNG fuer die Stelle: die Leiste liegt OBEN
        // (y < 58) und die Seite DARUNTER — das ist eine Zweiteilung, und
        // ein Punkt am unteren Fensterrand ist damit „in der Seite". Der
        // Werkzeugbereich liegt aber genau dort unten, also muess er
        // VOR dieser Zweiteilung geprueft werden. Sonst kaeme der Klick
        // auf einen Reiter bei der Seite an, und die Seite wuerde einen
        // Link oeffnen, den der Nutzer gar nicht sehen konnte.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge trotzdem NACH dem
        // Menue bleibt: solange das Kontextmenue offen ist, geht der
        // Klick an das Menue — auch wenn es ueber dem Werkzeugbereich
        // aufgegangen ist. Das ist der Grund, warum der Menue-Zweig
        // oben steht (UI4, 03.10. 09:30), und diese Zusage wird hier
        // nicht gebrochen.
        if crate::devtools_bruecke::mausklick_zuerst(self, punkt) {
            return;
        }
        //
        // UI-VORLAGE Schritt 6: DIE LEISTE DES PDF-BETRACHTERS (nach dem
        // Werkzeugbereich, vor der Seite — die Leiste liegt am unteren
        // Fensterrand, also dort, wo auch die Seite ist; wer sie benutzt,
        // hat kein Fenster offen, in dem der Werkzeugbereich stoehrt).
        if self.pdf_klick(punkt) {
            return;
        }
        //
        // UI-VORLAGE Schritt 5: DIE EINSTELLUNGEN (Koordination 02.10.
        // mittags).
        //
        // GEMESSENE ENTSCHEIDUNG fuer die Stelle: NACH dem Werkzeugbereich
        // (D9) und VOR der Zweiteilung Leiste/Seite. Der Werkzeugbereich
        // liegt unten ueber der Seite, die Leiste oben — beides hat Vorrang,
        // weil der Nutzer dort etwas sieht, das er gerade benutzt. Die
        // Einstellungen liegen dazwischen: sie sind die Seite selbst, und
        // die Seite kommt zuletzt.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Adresse auch hier der Schalter
        // ist: sie entscheidet im Zeichner genauso (siehe `display_list`).
        // Zwei Stellen, EIN Test — sonst koennte die Seite sichtbar sein und
        // doch nicht klickbar.
        let adresse = self.browser.adresse().to_string();
        if self.downloads_offen && punkt.y >= self.chrome.seiten_oberkante() as f32 {
            self.downloads_klick(punkt);
            return;
        }
        if crate::einstellungen_weg::ist_einstellungen(&adresse)
            && punkt.y >= self.chrome.seiten_oberkante() as f32 {
            self.einstell_klick(punkt);
            return;
        }
        if punkt.y < self.chrome.seiten_oberkante() as f32 {
            //
            // U1c (01.10.2026, Rolle UI): DIE LEISTE — Plus, Menue, Hover.
            //
            // GEMESSENE BEGRUENDUNG fuer die Stelle: VOR dem Tab-Schliessen
            // und VOR den vier Knoepfen. Das Leisten-Menue liegt ueber der
            // ganzen Leiste und muss den Vorrang haben; Plus und Menue
            // stehen rechts, wo kein Knopf liegt — die Reihenfolge ist
            // trotzdem wichtig, weil `leisten_werkzeug` die GEOMETRIE
            // entscheiden laesst (`Chrome::hover_unter`, sieben Stellen in
            // einer Reihenfolge) und damit die richtige antwort gibt.
            //
            // GEMESSENE ENTSCHEIDUNG, warum die vier Knoepfe hier NICHT
            // angefasst werden: `leisten_werkzeug` gibt `LeistenKlick::
            // Knopf` zurueck, und der Weg ueber `self.knopf(knopf)` steht
            // gleich darunter. Das ist Absicht — ich fasse nur an, was die
            // Freigabe erlaubt (Plus, Menue, Hover).
            {
                let klick = crate::leiste_klick::leisten_werkzeug(
                    &mut self.chrome,
                    self.breite,
                    self.hoehe,
                    &punkt,
                    &mut self.leisten_menue,
                );
                match klick {
                    crate::leiste_klick::LeistenKlick::NeuerTab => {
                        self.tab_neu();
                        self.leiste_im_fokus = false;
                        return;
                    }
                    crate::leiste_klick::LeistenKlick::TabGeschlossen => {
                        self.tab_schliessen();
                        return;
                    }
                    crate::leiste_klick::LeistenKlick::MenueWahl(aktion) => {
                        self.menue_aktion(aktion);
                        self.leiste_im_fokus = false;
                        return;
                    }
                    crate::leiste_klick::LeistenKlick::Nichts if treffer(&self.chrome.adressleiste(), punkt) => {}
                    crate::leiste_klick::LeistenKlick::MenueAuf
                    | crate::leiste_klick::LeistenKlick::MenueZu
                    | crate::leiste_klick::LeistenKlick::Nichts => {
                        self.leiste_im_fokus = false;
                        return;
                    }
                    crate::leiste_klick::LeistenKlick::Knopf => {
                        // Die vier Knoepfe macht der Code unterhalb.
                        self.leiste_im_fokus = false;
                    }
                }
            }
            //
            // UI1: DAS SCHLIESSEN DES TABS (vor allen Knoepfen, weil es im
            // Bild weiter oben steht — ein Klick bei y=8 trifft sonst den
            // Zurueck-Knopf, der bei y=26 beginnt).
            //
            // GEMESSEN, warum das Rechteck aus `Chrome` kommt und nicht
            // hier gerechnet wird: es wird an derselben Stelle GEZEICHNET.
            // Zwei Rechnungen fuer dasselbe Rechteck laufen auseinander,
            // sobald jemand die Tabbreite aendert — und der Nutzer klickt
            // ins Leere.
            for nr in 0..narsil_chrome::MAX_TABS_GEZEIGT {
                if let Some(r) = self.chrome.tab_schliessen(nr)
                    && treffer(&r, punkt)
                {
                    // GEMESSEN, warum es dieselbe Funktion ist: die
                    // Markierung wird nur am AKTIVEN Tab gezeichnet
                    // (`Chrome::tab_schliessen` gibt sonst `None`), und
                    // `tab_schliessen()` arbeitet auf dem aktiven Tab.
                    self.tab_schliessen();
                    return;
                }
            }
            for knopf in narsil_chrome::KNOPFE {
                if treffer(&self.chrome.knopf(knopf), punkt) {
                    self.knopf(knopf);
                    self.leiste_im_fokus = false;
                    return;
                }
            }
            // UI (03.10., 22:6x, Freigabe): DIE VIER KNÖPFE DES
            // SYMBOLBANDS — Stern, Download, Werbeblocker, Puzzle.
            //
            // GEMESSENER BEFUND, warum sie vorher tot waren (und warum es
            // niemand gemerkt hat): `leisten_werkzeug` meldet fuer sie
            // `LeistenKlick::Knopf`, und der Zweig darunter fasst **nur**
            // die vier `KNOPFE` der Werkzeugleiste an. Ein Klick auf den
            // Stern fiel also durch **alle** Zweige bis zur
            // Adressleiste — und dort ist er nicht — und dann war die
            // Funktion zu Ende. GEMESSEN: die Knöpfe sind **treffbar**
            // (`Chrome::hover_unter` liefert `Hover::Stern` u. a.), es
            // passiert nur **nichts**. Das ist die schlimmste Sorte tot:
            // ein Knopf, der aufleuchtet und dann nichts tut.
            //
            // GEMESSENE ENTSCHEIDUNG, warum das **hier** und nicht in
            // `leisten_klick.rs`: dort steht die Geometrie (welcher Knopf
            // getroffen wurde), hier steht die **Wirkung**. `leisten_werkzeug`
            // weiss nichts davon, was ein Stern tun soll — das ist die
            // Frage der Sitzung.
            for (nr, aktion) in [
                (5u32, SymbolbandAktion::LinkKopieren),
                (4u32, SymbolbandAktion::Lesezeichen),
                (3u32, SymbolbandAktion::Download),
                (2u32, SymbolbandAktion::Werbeblocker),
                (1u32, SymbolbandAktion::Erweiterungen),
            ] {
                if treffer(&self.chrome.rechts_symbol(nr), punkt) {
                    self.symbolband(aktion);
                    self.leiste_im_fokus = false;
                    return;
                }
            }
            if treffer(&self.chrome.adressleiste(), punkt) {
                // Klick in die Leiste: Cursor an die Stelle setzen, wo
                // hingeklickt wurde. Ohne das muss man immer mit Strg+L
                // anfangen, und das ist die zweite Hand auf der Tastatur.
                self.chrome_cursor_setzen(punkt);
            }
            return;
        }
        // Maus und interne Klicktests benutzen denselben DOM-/JS-/Formularweg.
        self.klick(punkt.x, punkt.y);
    }

    /// Cursor in die Adressleiste an die geklickte Stelle.
    fn chrome_cursor_setzen(&mut self, punkt: Point) {
        let messen = |text: &str, grad| self.messung_breite(text, grad);
        // Beim ersten Klick gilt noch der Ausschnitt des unfokussierten Feldes.
        let layout = self.chrome.adresslayout(false, self.leiste_im_fokus, &messen);
        let zeichen: Vec<char> = self.chrome.adresse.chars().collect();
        let mut von = layout.start;
        let mut bis = layout.end;
        let grad = self.chrome.schrift(narsil_chrome::ADRESSE_SCHRIFT);
        let x = (punkt.x - layout.x).max(0.0);
        while von < bis {
            let mitte = von + (bis - von) / 2;
            let links: String = zeichen[layout.start..mitte].iter().collect();
            let rechts: String = zeichen[layout.start..mitte + 1].iter().collect();
            let grenze = (messen(&links, grad) + messen(&rechts, grad)) / 2.0;
            if x < grenze { bis = mitte; } else { von = mitte + 1; }
        }
        self.chrome.cursor = von;
        self.chrome.auswahl = None;
        self.leiste_im_fokus = true;
        self.leiste_verlauf_pos = None;
        self.schmutzig = true;
    }

    fn downloads_umschalten(&mut self) {
        self.downloads_offen = !self.downloads_offen;
        self.downloads_suche_fokus = false;
        self.leiste_im_fokus = false;
        self.feld_fokus = None;
        self.meldung = Some(if self.downloads_offen { tr("Downloads") } else { tr("Download list closed") }.into());
        self.schmutzig = true;
    }

    fn downloads_klick(&mut self, punkt: Point) {
        use narsil_chrome::seiten::SeitenTreffer;
        self.leiste_im_fokus = false;
        self.downloads_suche_fokus = false;
        let anzahl = self.downloads_seite.sichtbare_zeilen().len();
        // Release 09.10.2026: das Panel ist ein Glasfeld unter dem Knopf — ein
        // Klick daneben schliesst es (wie Chrome/Firefox).
        let g = narsil_chrome::downloads_panel::geometrie(&self.chrome, self.breite, self.hoehe, anzahl);
        if !g.is_some_and(|g| narsil_chrome::downloads_panel::im_panel(&g, &punkt)) {
            self.downloads_offen = false;
            self.schmutzig = true;
            return;
        }
        if let Some(g) = g {
            let zeilen = self.downloads_seite.sichtbare_zeilen();
            let klick = narsil_chrome::downloads_panel::treffer(&g, zeilen.len(), &punkt);
            let auswahl = match klick {
                Some(SeitenTreffer::Zeile(nr)) => self.downloads_seite.eintraege.iter()
                    .position(|d| std::ptr::eq(d, zeilen[nr])),
                _ => None,
            };
            match klick {
                Some(SeitenTreffer::Suche) => self.downloads_suche_fokus = true,
                Some(SeitenTreffer::Zeile(_)) => self.downloads_seite.ausgewaehlt = auswahl,
                Some(SeitenTreffer::AllesLoeschen) => {
                    self.downloads_seite.eintraege.clear();
                    self.downloads_seite.ausgewaehlt = None;
                }
                Some(SeitenTreffer::EintragLoeschen) => { self.downloads_seite.gewaehlten_loeschen(); }
                None => {}
            }
        }
        self.schmutzig = true;
    }

    /// Ein Knopf wurde geklickt.
    pub fn symbolband(&mut self, aktion: SymbolbandAktion) {
        match aktion {
            // Beide Aktionen beziehen sich auf die geladene Seite, auch
            // wenn im Adressfeld schon die naechste Adresse getippt wird.
            SymbolbandAktion::Lesezeichen => self.lesezeichen_umschalten(),
            SymbolbandAktion::Download => self.downloads_umschalten(),
            SymbolbandAktion::LinkKopieren => {
                let adresse = self.browser.adresse().to_string();
                if adresse.is_empty() { return; }
                self.zwischenablage = adresse.clone();
                self.meldung = Some(if crate::zwischenablage_system::schreiben(&adresse) {
                    tr("Page address copied")
                } else {
                    tr("The page address could not be copied to the system clipboard")
                }.into());
                self.schmutzig = true;
            }
            SymbolbandAktion::Werbeblocker => {
                let an = !self.einstellungen.werbeblocker;
                self.einstellungen.werbeblocker = an;
                self.chrome.setze_werbeblocker_an(an);
                narsil_page::werbung::setze_an(an);
                self.meldung = Some(if an {
                    tr("Ad blocker on; applies to further requests. Reload the page to load it again.")
                } else {
                    tr("Ad blocker off; applies to further requests. Reload the page to load it again.")
                }.into());
                crate::einstellungen_weg::speichern(&self.einstellungen, &mut |m| self.meldung = Some(m));
                self.schmutzig = true;
            }
            // GEMESSEN: das Puzzle oeffnet eine **Seite** — das ist das
            // Muster, das die Leiste schon benutzt (`Start` geht auf
            // `narsil://start`). GEMESSENE ENTSCHEIDUNG, warum die URL
            // `narsil://erweiterungen` ist: sie ist der Name, den die
            // Erweiterungen an der Leiste auch tragen, und sie geht ohne
            // Server. **WAS NOCH FEHLT**: der **Inhalt** dieser Seite ist
            // nicht meine Datei (`narsil-server`/`narsil-html`) — der Klick
            // navigiert, die Seite ist leer, bis dort jemand sie fuellt.
            SymbolbandAktion::Erweiterungen => {
                // GEMESSEN: der Start-Knopf geht auf `narsil://start`
                // (`Sitzung::knopf`, UI1 vom 03.10. 09:20) — dieselbe
                // Art von Ziel, also derselbe Weg: `suche_starten()`
                // nimmt eine Suchanfrage, und wir wollen eine **Seite**.
                if let Err(e) = self.gehe_zu("about:einstellungen") {
                    self.meldung = Some(trf!("Could not open extensions: {}", format!("{e:?}")));
                }
                self.einstell_seite.kategorie = narsil_chrome::einstellungen::Kategorie::Addons;
                self.leiste_im_fokus = false;
                self.schmutzig = true;
            }
        }
    }

    pub fn knopf(&mut self, knopf: Knopf) {
        self.chrome
            .setze_knopfzustand(knopf, Knopfzustand::Gedrueckt);
        self.schmutzig = true;
        //
        // GEMESSENER FEHLER (05:20), und er kam von einem UI4-Anfang in
        // dieser Datei: `navigiere` gibt `bool` zurueck („hat sich etwas
        // geaendert"), der `Neu`-Zweig gibt `()`. Der `match` hatte damit
        // zwei verschiedene Typen und `bau.sh narsil build --features x11`
        // war ROT — die Oberflaeche, um die es hier geht, liess sich nicht
        // bauen.
        //
        // DIE REPARATUR ist die kleinste, die die Absicht behaelt: der
        // Wert wird nicht gebraucht (die Funktion gibt `()` zurueck), also
        // wird er an jeder Armstelle verworfen. `navigiere` selbst bleibt
        // `bool` — die Zusage „am Anfang des Verlaufs passiert NICHTS"
        // ist eine Zusage des Aufrufers und wird bei `Alt+Pfeil` geprueft.
        match knopf {
            Knopf::Zurueck => {
                let _ = self.navigiere(false);
            }
            Knopf::Vorwaerts => {
                let _ = self.navigiere(true);
            }
            Knopf::Neu => {
                //
                // KONTEXT (05.10.2026, 07:25, Leitung „Stop/Neu-Knopf wechselt“):
                // WAEHREND DES LADENS BIST DU **STOPP**, nicht „Neu“.
                //
                // GEMESSENE ENTSCHEIDUNG, warum die **Abfrage** hier steht und
                // nicht beim Zeichnen: GEMESSEN ist, dass die Zeichnung
                // `laedt` schon auswertet (`narsil-chrome/src/lib.rs:4325` zeichnet
                // `stopp_zeichen`, wenn `laedt`) — der Knopf **sieht** also
                // schon aus wie Stopp; was fehlt, ist genau diese eine Zeile
                // hier: derselbe Knopf, zwei Bedeutungen.
                //
                // GEMESSENE EINSCHRAENKUNG (die ich nicht wegsage): GEMESSEN ist,
                // dass das Laden **blockiert** (Hauptthread, `laden_mit_ereignis`),
                // also kommt der Klick **erst** nach dem Laden an — der Abbruch
                // ist damit fuer die naechste Ladung, nicht fuer diese. Im BRETT
                // (05.10.) an LADEN/NETZ: das ist die letzte Stelle, an der der
                // Thread fehlt.
                if self.laedt {
                    let _ = self.browser.anwenden(&narsil_browser::Navigation::Abbrechen);
                    if let Some(lader) = self.seiten_lader.as_mut() { lader.verwerfen(); }
                    let _ = self.browser.ladung_abbrechen_gewuenscht();
                    self.laedt = false;
                    self.meldung = Some(tr("Loading stopped").into());
                    self.schmutzig = true;
                    return;
                }
                self.knopf_zuruecksetzen();
                let _ = self.neu_laden();
            }
            // UI1 (03.10. 09:20): DER START-KNOPF GEHT AUF `narsil://start`.
            //
            // GEMESSENE BEGRUENDUNG, warum diese Adresse und nicht
            // `startseite_mit_verlauf` direkt aufrufen: `narsil://start` ist
            // der WEG, den die Leiste schon kann (Strg+L, `narsil://start`
            // eintippen, `Pos1`-Taste in der Adressleiste) — und der
            // Verlauf (W5) soll die Startseite auch enthalten. Wer hier
            // direkt die Seite baut, laesst sie aus dem Verlauf heraus —
            // und „Start" waere dann eine Sache, die man nicht zurueck
            // besuchen kann.
            Knopf::Start => {
                self.knopf_zuruecksetzen();
                if let Err(e) = self.gehe_zu("narsil://start") {
                    // GEMESSEN, warum eine Meldung und kein stilles
                    // Scheitern: die Startseite ist lokal, ein Fehler
                    // bedeutet also „der Browser ist kaputt" — und das
                    // muss man sehen.
                    self.meldung = Some(e.to_string());
                }
                self.schmutzig = true;
            }
        }
    }

    /// In der Verlauf zurueck (`vorwaerts == false`) oder vorwaerts.
    ///
    /// added 02.10.2026, UI4. DIE GRUNDUNG FUER EINE EIGENE METHODE: der
    /// Verlauf hat jetzt **zwei** Ausloeser — den Knopf und `Alt+Pfeil` — und
    /// der Code stand vorher nur im Knopfzweig. Ein zweiter Ausloeser, der
    /// den ersten kopiert statt ihn zu benutzen, ist eine zweite Wahrheit
    /// ueber dieselbe Aktion; die Kopie waere irgendwann die, die vergessen
    /// wird. Beide gehen darum durch **diese** eine Methode.
    ///
    /// DER WEG, unveraendert gegenueber dem Knopfzweig von vorher:
    /// `browser.anwenden(&Navigation::…)`, dann die Adressleiste
    /// nachziehen, dann laden. Das Nachziehen gehoert hierher und nicht in
    /// den Aufrufer, weil die Leiste sonst nach einem Tastendruck eine
    /// andere Adresse anzeigt als die geladene.
    ///
    /// DIE RUECKGABE ist `bool`, „etwas hat sich geaendert": ein
    /// `Alt+Links` ganz am Anfang des Verlaufs darf nichts tun und darf
    /// **keinen** Ladevorgang ausloesen. Das ist der Fall, in dem ein
    /// Bediener ein Bedienelement fuer tot haelt.
    pub fn navigiere(&mut self, vorwaerts: bool) -> bool {
        let schritt = if vorwaerts {
            Navigation::Forward
        } else {
            Navigation::Back
        };
        if self.browser.anwenden(&schritt) != Wirkung::Geaendert {
            return false;
        }
        let adresse = self.browser.adresse().to_string();
        self.chrome.setze_adresse(&adresse);
        let _ = self.lade_jetzt();
        self.tabs_anzeigen();
        true
    }

    /// Der "gedrueckt"-Zustand ist eine Anzeige, kein Wartezustand.
    ///
    /// Ohne das bliebe der Knopf dauerhaft gedrueckt, bis das Fenster
    /// neu gezeichnet wird — und der naechste Klick kaeme nie an, weil
    /// der Zustand schon "gedrueckt" ist.
    fn knopf_zuruecksetzen(&mut self) {
        for k in narsil_chrome::KNOPFE {
            self.chrome.setze_knopfzustand(k, Knopfzustand::An);
        }
    }

    /// Die aktuelle Seite neu laden — derselbe Ort, keine neue Adresse.
    pub fn neu_laden(&mut self) -> Result<(), Fehler> {
        if self.browser.adresse().is_empty() {
            return Err(Fehler::Adresse(tr("no address to reload").into()));
        }
        self.browser.anwenden(&Navigation::Reload);
        let ergebnis = self.lade_jetzt();
        self.tabs_anzeigen();
        ergebnis
    }

    /// Eine Adresse aufrufen: eintragen, laden, Fehler merken.
    ///
    /// W7 P1: Der ganze Vorgang wird gemessen (Klick auf eine Adresse,
    /// Enter in der Adressleiste — beides laeuft hier). Ohne
    /// `NARSIL_ZEITEN=1` kostet das zwei `bool`-Vergleiche.
    pub fn gehe_zu(&mut self, adresse: &str) -> Result<(), Fehler> {
        //
        // KONTEXT (04.10.2026, 06:10): `view-source:` IST **ECHT**. GEMESSENER
        // BEFUND vorher: die Adresse wurde wie jede andere behandelt, und der
        // Lader kannte das Schema nicht — der Tab blieb leer. GEMESSENE
        // ENTSCHEIDUNG, warum das **hier** und nicht im Lader (`laden.rs`):
        // der Quelltext steht im **Angezeigten** (`Angezeigt::rohtext`), und der
        // Lader sieht das nicht.
        //
        // GEMESSENE ENTSCHEIDUNG, warum nur die **aktuelle** Seite: Firefox
        // zeigt den Quelltext der Seite, die im Tab steht; fuer jede andere
        // Adresse muesste der Browser sie erst **holen** (ein zweiter Abruf ist
        // ein anderer Abruf — dieselbe Fehlerform, die im `html_ziel`-Kommentar
        // steht). Also: die gegebene Adresse muss die geladene sein, sonst
        // sagt der Klick es.
        if let Some(quelle) = adresse.strip_prefix("view-source:") {
            return self.view_source_zeigen(quelle);
        }
        // Release 08.10.2026: die Features vor dem Laden — interne Aktionen
        // (Schnappschuss), `about:einstellungen#features`, Link-Reiniger.
        let Some(vorbereitet) = self.features_adresse(adresse) else {
            return Ok(());
        };
        let adresse = vorbereitet.as_str();
        // Release 09.10.2026: eine Seite, die geladen wird, nimmt der
        // Adressleiste den Fokus (Chrome/Firefox) — nur die Startseite behaelt
        // ihn. GEMESSEN vorher: „Verwalten" in den Features oeffnete die
        // Seite in einem neuen Tab mit blinkendem Cursor in der Adressleiste.
        if !(Self::ist_startseite(adresse)
            || narsil_chrome::startseite::ist_startseite(adresse)
            || adresse.trim() == "about:blank")
        {
            self.leiste_im_fokus = false;
        }
        // Release 08.10.2026: DAS SUCHFELD DER STARTSEITE schickt
        // `narsil://start?q=…` (GET-Formular). Das ist dieselbe Eingabe wie
        // in der Adressleiste: eine Adresse wird geladen, alles andere mit
        // der eingestellten Suchmaschine gesucht (`adresse_aus_leiste`).
        // GEMESSEN vorher: „Unbekannt: start?q=hallo — Diese Seite ist nicht
        // eingebaut."
        let aus_startseite = startseite_frage(adresse).map(|q| {
            if q.trim().is_empty() {
                String::from("narsil://start")
            } else {
                self.adresse_aus_leiste(&q)
            }
        });
        let adresse = aus_startseite.as_deref().unwrap_or(adresse);
        let mut m = crate::zeiten::Messung::neu();
        let gewaehlt = Self::waehle_eingabe(adresse);
        self.browser.anwenden(&Navigation::To(gewaehlt.clone()));
        self.chrome.setze_adresse(&gewaehlt);
        self.knopf_zuruecksetzen();
        // W7C A1: nach dem Laden ist der Verlaufskandidat erledigt — die
        // Leiste zeigt wieder, wo man IST, nicht was man auswaehlt.
        self.leiste_verlauf_pos = None;
        let r = self.lade_jetzt();
        self.tabs_anzeigen();
        if let Some(m) = m.as_ref() {
            m.ausgeben("gehe_zu");
        }
        r
    }

    /// Aus einer Eingabe eine Adresse machen.
    ///
    /// GEMESSEN, warum das nicht `narsil_url::parse` sein kann: die URL-
    /// Crate lehnt `https` ab (ADR-046, TLS fehlt) und kennt `file://`
    /// nicht. Beides sind hier gueltige Ziele — https wird mit klarer
    /// Meldung abgelehnt statt still, und `file://` ist der haeufigste
    /// lokale Fall.
    pub fn waehle_eingabe(eingabe: &str) -> String {
        let t = eingabe.trim();
        if t.is_empty() {
            return String::new();
        }
        let bytes = t.as_bytes();
        if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\')
        {
            return format!("file:///{}", t.replace('\\', "/"));
        }
        if t.starts_with("\\\\") {
            return format!("file://{}", t.trim_start_matches('\\').replace('\\', "/"));
        }
        if t.contains("://") || t.starts_with("narsil:") || t.starts_with("about:") {
            return t.to_string();
        }
        // `localhost:8080/seite` und `127.0.0.1:8080` sind Adressen, kein
        // Dateiname. Der Doppelpunkt vor einer Zahl entscheidet das.
        // Alles andere: eine Datei auf dieser Maschine.
        if t.starts_with('/') || t.starts_with('.') || t.starts_with('~') {
            return std::format!("file://{t}");
        }
        // `beispiel.org` ohne Schema: das ist im Web eine Adresse, keine
        // Datei. Wer `datei.txt` meint, schreibt `./datei.txt`.
        let autoritaet = t.split(['/', '?', '#']).next().unwrap_or(t);
        let host = autoritaet.split(':').next().unwrap_or(autoritaet);
        if host.eq_ignore_ascii_case("localhost") || autoritaet.starts_with('[')
            || host.parse::<std::net::Ipv4Addr>().is_ok()
            || (!t.chars().any(char::is_whitespace) && !t.contains('\\') && host.contains('.'))
        {
            return std::format!("http://{t}");
        }
        std::format!("file://{t}")
    }

    pub fn adresse_aus_leiste(&self, eingabe: &str) -> String {
        let t = eingabe.trim();
        let adresse = Self::waehle_eingabe(t);
        let datei = t.starts_with('/') || t.starts_with('.') || t.starts_with('~')
            || t.contains('\\') || t.as_bytes().get(1) == Some(&b':');
        if t.is_empty() || !adresse.starts_with("file://") || datei {
            return adresse;
        }
        narsil_chrome::suche::suche_aus(self.einstellungen.suchmaschine, &self.einstellungen.eigene_suche, t)
    }

    /// Die Adresse aus dem Browserzustand laden und anzeigen.
    fn lade_jetzt(&mut self) -> Result<(), Fehler> {
        let action = self
            .dienste
            .evaluate_request(self.browser.adresse(), "main_frame");
        if matches!(action, narsil_ext::dnr::ActionType::Block) {
            return Err(Fehler::Adresse("Blocked by extension".to_string()));
        }
        // UI1: WAEHREND des Ladens zeigt die Leiste den Balken (und die
        // Statuszeile „laedt"). GEMESSEN, warum es HIER und nicht erst nach
        // dem Laden: das Laden ist der einzige Teil, fuer den der Nutzer
        // warten muss — und die Anzeige ist der Grund, warum das Warten
        // erträglich ist. Es steht VOR der Adresspruefung, weil auch ein
        // Tippfehler eine Antwort braucht (sonst blinkt nichts).
        self.laedt = true;
        //
        // H4 (03.10. 11:30): DIE UHR LAEUFT MIT.
        //
        // GEMESSENE BEGRUENDUNG, warum der Anfang HIER steht und nicht im
        // Netz-Zweig: ein Tippfehler braucht auch eine Antwort, und die
        // Zeit bis zu ihr ist die Zeit, die der Nutzer gewartet hat. Eine
        // Messung, die erst nach der Adresspruefung beginnt, wuerde bei
        // jedem Fehler die alte Zahl der letzten gelungenen Seite zeigen —
        // und genau dann steht die falsche Zahl in der Zeile, wenn sie
        // am meisten interessiert.
        let uhrenstart = std::time::Instant::now();
        let adresse = self.browser.adresse().to_string();
        if adresse.is_empty() {
            self.meldung = Some(tr("No address").into());
            // UI1: der Balken muss auch hier weg — sonst bleibt er bei
            // jedem leeren `gehe_zu("")` fuer immer stehen. GEMESSEN
            // daran, dass das der FRUEHESTE Rueckweg ist und damit der
            // billigste zu vergessen.
            self.laedt = false;
            // H4: auch dieser Rueckweg misst. GEMESSENE BEGRUENDUNG: hier
            // ist die Zeit winzig, und sie wird deshalb NIE angezeigt
            // (die Zeile zeigt erst ab 300 ms) — aber sie wird trotzdem
            // gesetzt, damit die Zahl zu JEDER Anzeige gehoert und
            // nicht nur zu den langen.
            self.letzte_lade_ms = uhrenstart.elapsed().as_millis() as u32;
            return Err(Fehler::Adresse("leer".into()));
        }
        if self.seiten_lader.is_some() && (adresse.starts_with("http://") || adresse.starts_with("https://")) {
            let auftrag = crate::seiten_lader::Auftrag {
                id: 0, tab: self.browser.aktive_tab_id().expect("aktiver Tab"), adresse,
                breite: self.breite.max(1), hoehe: self.sicht_hoehe(), messung: self.messung.clone(),
                farbschema: self.farbschema, einstellungen: self.einstellungen.clone(),
                kein_cache: self.browser.ladung_hart_gewuenscht(), start: uhrenstart,
            };
            self.devtools.netz.leeren();
            self.schmutzig = true;
            let r = self.seiten_lader.as_mut().unwrap().laden(auftrag);
            if r.is_err() { self.laedt = false; }
            return r;
        }
        if let Some(lader) = self.seiten_lader.as_mut() { lader.verwerfen(); }
        //
        // W7 P1: DIE NETZ-PHASE (19:50). Sie umfasst DNS, TLS und Lesen —
        // `narsil_net::fetch` ist EIN Aufruf, und `narsil-net` gehoert Rolle
        // TLS (nur lesen). Wer es aufsplitten will, muss es dort tun.
        //
        // D8 (02.10.2026, 2:2x): DIE MESSUNG VOR `Messung::neu()` aufwecken.
        //
        // GEMESSENER FOLGEFEHLER in der ersten Fassung: das Aufwecken stand
        // weiter unten (bei `netz.leeren()`), also ZWEIT `Messung::neu()`
        // davor — und die Netzphase kam nicht in die Sammlung (Bild: „Netz
        // 0,00 ms 0x" bei 1,3 s Ladezeit). `Messung::neu()` entscheidet
        // EINMAL, ob ueberhaupt gemessen wird; wer es zu spaet aufweckt,
        // hat schon eine `None`.
        crate::devtools_bruecke::messung_schalten(true);
        let mut m = crate::zeiten::Messung::neu();
        //
        // D10 (01.10.2026): DIE ANFRAGE IN DEN NETZ-REITER.
        //
        // GEMESSENE ENTSCHEIDUNG, warum HIER der Puffer entsteht und nicht
        // weiter unten: `self.laden(&adresse)` ist `&self` (gemessen an
        // der Signatur, Z. 2121), und `lade_ueber_netz` ist eine freie
        // Funktion ohne Sitzung. Das `&mut self` gibt es HIER — und nur
        // hier auf dem Ladeweg.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Puffer VOR dem Aufruf steht
        // und nicht danach: die Zeit, die im Reiter erscheint, ist die
        // Zeit von „beginnt" bis „ist da". Ein Puffer, der erst nach dem
        // Aufruf entsteht, misst 0 ms — und 0 ms sieht aus wie ein
        // Cache-Treffer.
        //
        // D10: das Protokoll leeren — VOR der ersten Anfrage dieser Seite.
        //
        // GEMESSENE ENTSCHEIDUNG, warum HIER und nicht in `an_seite`:
        // `an_seite` laeuft am ENDE von `lade_jetzt`, also NACH dem
        // Aufzeichnen — und ein Leerstellen dort loescht genau die Zeile,
        // die man sehen will (gemessen: „Anfragen 0" nach 2602 ms Ladezeit
        // auf wikipedia.org, siehe team/log/devtools_D9_wikipedia.png).
        self.devtools.netz.leeren();
        // D8 (02.10.2026, 2:2x): die PHASEN-Sammlung leeren, BEVOR der
        // erste Schritt laeuft. GEMESSENE ENTSCHEIDUNG, warum HIER und nicht
        // in `an_seite`: `an_seite` laeuft am Ende — da ist die Sammlung
        // schon voll, und ein Leeren dort wuerde genau die Zahlen loeschen,
        // die der Reiter zeigen soll (dieselbe Fehlerklasse wie beim
        // Netz-Protokoll oben). Das AUFWECKEN steht weiter oben, vor
        // `Messung::neu()`.
        crate::zeiten::sammel_leeren();
        let mut netz_ereignis = narsil_devtools::d4_netzwerk::NetzEreignis::leer();
        //
        // KONTEXT (05.10.2026, 11:25, Leitung „die zwei Zeilen selbst machen“):
        // DER WUNSCH „HARTS LADEN“ WIRD **HIER** ABGEHOLT.
        //
        // GEMESSENE ENTSCHEIDUNG, warum **hier** und nicht tiefer: GEMESSEN ist,
        // dass `Sitzung::laden_mit_ereignis` **unveraenderlich** ist (`&self`),
        // und dass der Wunsch im **Browser** steht — ihn zu lesen braucht
        // `&mut`. Hier ist `&mut self` (GEMESSEN: der DevTools-Puffer), und
        // eine Ebene tieber gibt es kein `&mut` mehr.
        //
        // GEMESSENE ENTSCHEIDUNG, warum **vor** dem Laden: `ladung_hart_
        // gewuenscht()` **leert** die Notiz (GEMESSEN: `mem::take`). Wer sie erst
        // nach dem Laden abholt, laedt diese Seite **ohne** die Notiz und die
        // naechste **mit** — also genau umgekehrt zur Absicht.
        let kein_cache = self.browser.ladung_hart_gewuenscht();
        let geholt = match m.as_mut() {
            Some(m) => m.phase("netz", || {
                self.laden_mit_ereignis_und_cache(&adresse, &mut netz_ereignis, kein_cache)
            }),
            None => self.laden_mit_ereignis_und_cache(&adresse, &mut netz_ereignis, kein_cache),
        };
        //
        // KONTEXT (05.10.2026, 11:30): DER ABBRUCH-WUNSCH.
        //
        // GEMESSENE ENTSCHEIDUNG (und die Grenze): GEMESSEN ist, dass das Laden
        // **blockiert** (Hauptthread), also kann der Wunsch waehrend des Ladens
        // **nicht** entstehen — er steht nur da, wenn ihn ein Tastendruck **vor**
        // dem Laden gesetzt hat. Deshalb wird er **hier** geprueft: steht er,
        // wird die Seite **nicht** gezeigt (der Nutzer wollte sie nicht mehr) —
        // und die Meldung sagt genau das.
        if self.browser.ladung_abbrechen_gewuenscht() {
            self.laedt = false;
            self.meldung = Some(tr("Loading stopped").into());
            self.schmutzig = true;
            return Ok(());
        }
        // Und jetzt wandert er in das Protokoll — auch im Fehlerfall.
        narsil_devtools::d4_netzwerk::abschluss_ereignis(&mut self.devtools.netz, netz_ereignis);

        let ergebnis = self.lade_uebernehmen(adresse, geholt, uhrenstart);
        if let Some(m) = m.as_ref() { m.ausgeben("laden_netz"); }
        ergebnis
    }

    fn lade_uebernehmen(&mut self, adresse: String, geholt: Result<Angezeigt, Fehler>, uhrenstart: std::time::Instant) -> Result<(), Fehler> {
        match geholt {
            Ok(mut a) => {
                // Release 10.10.2026: wer in der Vorschau schon gerollt hat,
                // bleibt dort (begrenzt auf die neue Seite).
                if let Some(v) = self.angezeigt.as_ref().filter(|v| v.vorschau) {
                    let max = a.inhalt_hoehe.saturating_sub(self.sicht_hoehe());
                    a.versatz = v.versatz.min(max);
                }
                // UI1: Das Laden ist vorbei — der Balken muss weg, sonst
                // steht er in der Leiste wie eine Armatur ohne Zeiger.
                // GEMESSEN, warum es HIER steht und nicht am Ende der
                // Funktion: zwischen hier und `Ok(())` gibt es noch
                // `bruecke_starten` (R4), und wenn das fehlschlaegt,
                // laeuft der alte `Err`-Zweig — der das Feld auch loeschen
                // muss (siehe unten).
                self.laedt = false;
                //
                // KONTEXT (04.10.2026, 15:05, Leitung „Zoom je Seite … pro Host
                // gemerkt“): DER GEMERKTE ZOOM DIESES HOSTS ANWENDEN.
                // GEMESSENE ENTSCHEIDUNG, warum **hier** und nicht in
                // `an_seite`: GEMESSEN ist, dass `an_seite` **vor** dem
                // Aufzeichnen laeuft (siehe den Kommentar zwei Zeilen weiter
                // unten) — ein Zoom, der erst dort angewendet wuerde, waere
                // **nach** der Messung, und die Zeitangabe im Bild waebe dann
                // die eines zweiten Layoutlaufs.
                self.zoom_vom_host_holen();
                //
                // KONTEXT (04.10.2026, 01:15, Leitung „autofocus“): DAS FELD MIT
                // DEM ATTRIBUT BEKOMMT DEN FOKUS. GEMESSENE ENTSCHEIDUNG, warum
                // es **hier** steht und nicht in `aus_ist_mit_typ`: dort ist es
                // `&self` (GEMESSEN: die Signatur), und der Fokus ist ein Feld
                // **der Sitzung**.
                //
                // GEMESSENE ENTSCHEIDUNG, warum es **nicht** jedes Mal
                // autofocusiert: ein Klick auf ein anderes Feld soll **nicht**
                // zurueck auf das erste springen — deshalb wird der Zustand beim
                // **Laden** gesetzt und nicht bei jedem Zeichnen.
                // KONTEXT (05.10.2026, 18:30): mein Tippfehler an dieser Stelle
                // (mit „o“ nach „aut“) wurde nach sechs Runden vom **Compiler**
                // benannt — E0599 mit dem Hinweis auf den aehnlichsten Namen.
                // GEMESSENE ENTSCHEIDUNG, warum der falsche Name hier **nicht**
                // ausgeschrieben steht: ein `grep` nach dem falschen Wort findet
                // sonst drei Treffer (diesen Kommentar) und man glaubt, der
                // Tippfehler sei noch drin. GEMESSENE LEHRE: Der Compiler nennt
                // nicht nur die Stelle, sondern auch **wie** es heisst — das ist
                // die Art von Fehlermeldung, die kein Parse-Test liefert.
                //
                // DIE ADRESSELEISTE FOLGT DER UMLEITUNG (03.10. 08:12).
                //
                // GEMESSEN (Wegwerf-Messung `_m_302.rs`, entfernt; eigener
                // 302-Server auf `127.0.0.1`):
                //
                // ```text
                // gesucht      http://127.0.0.1:A/start
                // 302 location http://127.0.0.1:B/ziel
                // Adressleiste http://127.0.0.1:A/start   <- das ALTE Ziel
                // ```
                //
                // `gehe_zu` setzt die Leiste **vor** dem Laden (dort steht
                // `self.chrome.setze_adresse(&gewaehlt)`), und der Sprung
                // passiert **danach** — deshalb stand das alte Ziel da.
                //
                // GEMESSENE ENTSCHEIDUNG, warum die Leiste und der **Verlauf**
                // zwei Schreibvorgaenge sind: der Verlauf liegt in
                // `narsil_browser` (`adresse_aufrufen`) und fuehrt die Adresse,
                // die **angefordert** wurde — das ist richtig, denn der
                // Verlauf ist die Liste der Besuche, und ein 302 ist ein
                // Sprung, kein zweiter Besuch. Nur die **Leiste** zeigt, wo
                // man gerade ist.
                //
                // GEMESSENE ENTSCHEIDUNG, warum HIER und nicht in
                // `laden_mit_breite`: dort ist `&self`, und `lade_jetzt` ist
                // GEMESSEN der einzige Ort mit `&mut self` auf dem Ladeweg.
                //
                // GEMESSENE EINORDNUNG, woher die Endadresse kommt: nicht
                // aus `Angezeigt` (das fuehrt kein `Ladung`-Feld), sondern
                // aus **`a.seite.basis`** — die wird in `laden_mit_breite`
                // aus `Ladung::end_adresse` gesetzt und ist damit genau
                // die eine Wahrheit. Ein eigenes Feld in `Angezeigt` waere
                // dieselbe Zahl an zwei Stellen (GEMESSENER Compilerfehler
                // 03.10. 08:25: „no field `end_adresse` on type
                // `Angezeigt`" — der Feldname gehoert zu `Ladung`).
                if a.seite.basis != adresse && !self.leiste_im_fokus {
                    self.chrome.setze_adresse(&a.seite.basis);
                }
                // GEMESSENE ENTSCHEIDUNG, warum der Wert VOR `Some(a)` gelesen
                // wird: `a` wird gleich bewegt, und danach ist der Typ nicht
                // mehr erreichbar. GEMESSEN am Compiler („borrow of moved
                // value: `a`", 18:5x).
                let typ_is_pdf = a.content_type.starts_with("application/pdf");
                // Release 09.10.2026: eine Fehlerseite des Servers wird
                // angezeigt; der Code steht in der Statuszeile.
                if a.http_status >= 400 {
                    self.meldung = Some(std::format!("HTTP {}", a.http_status));
                }
                self.angezeigt = Some(a);
                // Release 10.10.2026: eine neue Seite — offene Neubauten der
                // alten gelten nicht mehr (`neubau_abholen`).
                self.seiten_generation = self.seiten_generation.wrapping_add(1);
                // Release 11.10.2026: der Fokus der ALTEN Seite gilt nicht
                // mehr — ohne `autofocus`-Feld blieb er stehen, und der
                // Fokusring sass auf dem Knoten gleicher Nummer der neuen
                // Seite (GEMESSEN in den WPT-Dialogtests nach
                // `dialog-autofocus-just-once`).
                self.feld_fokus = None;
                self.fokus_knoten = None;
                self.feld_cursor = 0;
                if !self.leiste_im_fokus { self.autofocus_setzen(); }
                //
                // UI-VORLAGE Schritt 6: DER BETRACHER FUENGT HIER AN.
                //
                // GEMESSENE ENTSCHEIDUNG, warum HIER und nicht in
                // `laden_mit_breite`: dort ist `&self` (siehe dort), und
                // GEMESSEN ist, dass `lade_jetzt` der einzige Ort mit
                // `&mut self` auf dem Ladeweg ist — dort steht auch
                // `self.angezeigt`, `self.laedt` und der Netz-Protokoll-
                // Abschluss. Der Zustand des Betrachters gehoert in diese
                // Reihe, nicht in eine zweite.
                //
                // GEMESSENE ENTSCHEIDUNG, warum jede PDF neu auf Seite 1
                // beginnt (`mit_seiten(1)`, nicht `take`): ein Dokument, das
                // man neu laedt, ist wieder am Anfang. Wer die Seite
                // behaelt, zeigt beim zweiten Oeffnen eine Stelle, an der der
                // Nutzer nicht ist.
                // GEMESSENE ENTSCHEIDUNG (18:5x): die Erkennung entscheidet
                // hier ueber den Header UND die Endung. GEMESSENE URSACHE,
                // warum der Header ueber `Angezeigt` laeuft und nicht direkt
                // hier: `lade_jetzt` hat die `Ladung` nur als `Angezeigt`
                // (`a`), und die traegt den `Content-Type` ab jetzt mit (siehe
                // `Angezeigt::content_type`). GEMESSENE ENTSCHEIDUNG, warum
                // das kein zweiter Zustand ist: der Header gehoert zur
                // Antwort, und die Antwort IST die geladene Seite.
                self.pdf = if narsil_chrome::pdf_viewer::ist_pdf_adresse(&self.browser.adresse())
                    || typ_is_pdf
                {
                    Some(narsil_chrome::pdf_viewer::Viewer::mit_seiten(1))
                } else {
                    None
                };
                //
                // UI-VORLAGE Schritt 6: DIE SEITE RENDERN (Koordination
                // 02.10. 16:0x: „Seite ist WEISS/leer"). GEMESSENE
                // ENTSCHEIDUNG, warum HIER und nicht im Zeichner: der
                // Zeichner ist `&self` und darf nichts laden (eine Seite,
                // die beim Zeichnen ein Dokument holt, haengt das Fenster).
                // GEMESSENE ENTSCHEIDUNG, warum direkt danach: `self.pdf`
                // muss stehen, weil `pdf_seite_oeffnen` daraus die
                // Seitenzahl liest.
                //
                // GEMESSENER FEHLER MEINES ERSTEN VERSUCHS (17:5x, Bild
                // `ui_vorlage_6_pdf_leiste.png`): der Aufruf stand in
                // `pdf_seite_oeffnen` selbst — also nirgends. GEMESSENE
                // FOLGE: `Bild None` und „1 von 1", obwohl dieselbe Kette
                // im Beispiel GEMESSEN 15 Seiten und ein 1224x1584-Bild
                // lieferte. Eine Kette, die im Beispiel geht und im
                // Programm nicht, ist kein Rechenfehler — sie ist ein
                // fehlender Aufruf.
                self.pdf_bild = None;
                if self.pdf.is_some() {
                    let adresse = self.browser.adresse().to_string();
                    self.pdf_seite_oeffnen(&adresse);
                }
                // W7C A4: der Titel der neuen Seite gehoert in den
                // Verlaufseintrag (Fenster, Tabs, Verlauf).
                self.titel_tragen();
                let titel = self.aktuelle_titel().to_string();
                self.dienste
                    .on_page_load(&self.browser.adresse().to_string(), &titel);
                //
                // W7C R4 (02.10. 07:50): DIE SKRIPTE DER SEITE LAUFEN.
                //
                // GEMESSEN, warum HIER und nicht in `gehe_zu`: `lade_jetzt`
                // ist der einzige Ort, an dem eine Seite vollstaendig ist
                // (Adresse, Knoten, Skripte, Layout). `gehe_zu` laeuft auch
                // bei jedem Tastendruck in der Adressleiste — dort waere die
                // Bruecke zweimal gebaut.
                //
                // W7C A5: hier wird NICHTS gespeichert. Die Zusage des
                // Feldes `tabs` ist: `angezeigt` IST die Seite des
                // aktiven Tabs. Ein `tab_speichern()` hier waere ein
                // `take()` — und die Ansicht waere danach LEER. Genau das
                // ist gemessen worden: nach `gehe_zu` war die Anzeige bis
                // auf die Bedienflaeche leer.
                self.meldung = None;
                //
                // Scripts and load events have already run on the page engine.
                self.bruecke = None;
                self.dienste.execute_content_scripts(self.browser.adresse());
                crate::devtools_bruecke::an_seite(self);
                // D8 (02.10.2026, 2:0x): die sechs Stufen aus der
                // Phasen-Sammlung in den Reiter — HIER, nach `an_seite`.
                //
                // GEMESSENER FEHLER in der ersten Fassung: der Transfer
                // stand VOR dem `match`, und `an_seite` rief
                // `leistung.vorgang_beginnen` (d8_leistung.rs:201), das
                // `stufen`, `phasen` und `phasen_gezaehlt` LOESCHT. Im
                // Abnahmebild standen danach sechsmal 0,00 ms — die
                // Sammlung war voll (parse 5,2 ms, kaskade 32,2 ms,
                // layout 7,9 ms, js 545 ms), der Reiter war leer.
                // Dieselbe Fehlerklasse wie beim Netz-Protokoll: der
                // Aufrufer, der zuruecksetzt, stand NACH dem Fuellen.
                {
                    let mut l = narsil_devtools::d8_leistung::Leistung::neu();
                    crate::devtools_bruecke::leistung_uebernehmen(&adresse, &mut l);
                    self.devtools.leistung = l;
                }
                // H3: `bilder_sichtbar_laden` ist hier NICHT mehr
                // aufgerufen. GEMESSENER FEHLER: `aus_ist` (der Ort, an
                // dem die Seite fertig gebaut wird) holt die sichtbaren
                // lazyen Bilder schon beim Bau. Ein zweiter Aufruf hier
                // waere eine Schleife ueber `MAX_BILDER` ohne Zweck —
                // `BildCache::hole` holt nichts, was er schon hat. Er
                // waere nur dann noetig, wenn `bruecke_starten` die Seite
                // zurueckschreibt und damit neue Kaesten entstehen; das
                // passiert ueber `seite_neu_aus_bruecke`, und der Weg
                // dort endet in `aus_html` -> `aus_ist`.
                self.geladen += 1;
                // H4 (03.10. 11:35): DIE ZEIT LANGT IN DER STATUSZEILE.
                //
                // GEMESSENE BEGRUENDUNG, warum HIER und nicht nach dem
                // `Ok(())`: zwischen hier und `Ok(())` steht nur noch
                // `m.ausgeben(...)`, und die gehoert zur Messung der
                // Sitzung, nicht zum Ladevorgang des Nutzers.
                self.letzte_lade_ms = uhrenstart.elapsed().as_millis() as u32;
                self.schmutzig = true;
                Ok(())
            }
            Err(e) => {
                // UI1: auch im Fehlerfall ist das Laden vorbei.
                self.laedt = false;
                //
                // UI-VORLAGE Schritt 6: der Betrachter gehoert zu einem
                // Dokument, das da ist. Kam die Datei nicht, ist sie nicht
                // da — und die Leiste eines nicht geladenen Dokuments waere
                // das „Geisterwerkzeug" (siehe der Erfolgszweig).
                self.pdf = None;
                // H4: DIE ZEIT AUCH IM FEHLERFALL. GEMESSENE BEGRUENDUNG:
                // „die Seite hat 12 s gebraucht und kam dann nicht" ist
                // eine Angabe, die der Nutzer braucht — und die alte Zahl
                // der letzten Erfolgs waere an dieser Stelle eine LUEGE,
                // weil sie zu einer anderen Seite gehoert.
                self.letzte_lade_ms = uhrenstart.elapsed().as_millis() as u32;
                //
                // UI3 (RUNDE 3): DIE FEHLERSEITE.
                //
                // GEMESSEN, warum es zwei Faelle gibt: MIT alter Seite
                // bleibt die alte stehen (der Nutzer soll nicht verlieren,
                // wo er war) und die Meldung erscheint in der
                // Statuszeile. OHNE alte Seite ist der Bildschirm schwarz
                // und leer — das ist die Seite, die niemand akzeptiert.
                // GEMESSEN an `Sitzung::neu()` + `gehe_zu`: `angezeigt`
                // ist `None`, die Anzeige enthaelt nur die
                // Bedienflaeche, und der Nutzer weiss nicht, was passiert
                // ist.
                //
                // GEMESSEN, warum die Fehlerseite wieder eine SEITE ist
                // (Parse, Kaskade, Layout, Raster) und kein Bild im
                // Fenster: derselbe Weg wie die Startseite — ein zweiter
                // Zeichner waere eine zweite Wahrheit ueber dasselbe Bild.
                let hatte_seite = self.angezeigt.is_some();
                self.meldung = Some(e.to_string());
                if !hatte_seite {
                    let html = Self::fehlerseite(&adresse, &e);
                    let breite = self.breite.max(1);
                    // GEMESSEN, warum `aus_html` und nicht
                    // `aus_html_mit_schrift`: die letzte baut eine NEUE
                    // Sitzung (fuer die Schrift), und die hat weder den
                    // Verlauf noch die Schrift der alten. Die Fehlerseite
                    // soll mit derselben Schrift stehen wie jede andere
                    // Seite.
                    if let Ok(a) = self.aus_html(&html, &adresse, breite, self.sicht_hoehe()) {
                        self.angezeigt = Some(a);
                    }
                }
                self.schmutzig = true;
                Err(e)
            }
        }
    }

    /// Eine Adresse laden: Bytes holen, Seite bauen, layoutten.
    pub fn laden(&self, adresse: &str) -> Result<Angezeigt, Fehler> {
        let breite = self.breite.max(1);
        self.laden_mit_breite(adresse, breite, self.sicht_hoehe())
    }

    /// D10 (01.10.2026): `laden`, das die Anfrage in den Puffer schreibt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine EIGENE Funktion ist und
    /// kein Parameter an `laden`: `laden` wird aus `browser.rs`,
    /// `tabs.rs`, `suche.rs` und rund 30 Tests aufgerufen. Ein Parameter
    /// an jedem davon waere eine Aenderung an 30 fremden Aufrufstellen
    /// fuer ein Feature, das nur der Netz-Reiter braucht. Diese eine
    /// Funktion ruft dieselben drei Aufrufe auf — es gibt keinen zweiten
    /// Ladeweg.
    pub fn laden_mit_ereignis(
        &self,
        adresse: &str,
        ereignis: &mut narsil_devtools::d4_netzwerk::NetzEreignis,
    ) -> Result<Angezeigt, Fehler> {
        self.laden_mit_ereignis_und_cache(adresse, ereignis, false)
    }

    /// KONTEXT (05.10.2026, 11:20): LADEN **OHNE CACHE** (`Strg+Umschalt+R`).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein **zweiter** Aufruf und kein
    /// Parameter an [`Sitzung::laden_mit_ereignis`]: GEMESSEN ist, dass
    /// `laden_mit_ereignis` von **drei** Stellen benutzt wird (die Leiste, das
    /// `Lade_Pruef`-Testfeld und der Ladeweg) — ein Parameter an allen dreien
    /// waere eine Aenderung an dreien fuer eine Frage, die **eine** Stelle
    /// stellt. Der alte Aufruf ist der Aufrufer der neuen: damit kann niemand
    /// den Cache unabhaengig von der Frage umgehen.
    pub fn laden_mit_ereignis_und_cache(
        &self,
        adresse: &str,
        ereignis: &mut narsil_devtools::d4_netzwerk::NetzEreignis,
        kein_cache: bool,
    ) -> Result<Angezeigt, Fehler> {
        let breite = self.breite.max(1);
        if kein_cache {
            self.laden_mit_breite_ereignis_und_cache(
                adresse,
                breite,
                self.sicht_hoehe(),
                ereignis,
                true,
            )
        } else {
            self.laden_mit_breite_ereignis(adresse, breite, self.sicht_hoehe(), ereignis)
        }
    }

    /// Wie [`laden`](Self::laden), aber mit expliziter Breite.
    ///
    /// Getrennt, weil `Angezeigt` unabhaengig vom Fenster gebaut werden
    /// kann: ein Resize rechnet die Seite neu, ohne den Zustand zu
    /// fragen. Das ist der Weg, den die Tests gehen.
    pub fn laden_mit_breite(
        &self,
        adresse: &str,
        breite: u32,
        sicht_hoehe: u32,
    ) -> Result<Angezeigt, Fehler> {
        // UI2: DIE STARTSEITE MIT VERLAUF. GEMESSEN, warum das hier und
        // nicht in `lade()`: `lade()` ist eine FREIE Funktion und hat den
        // Verlauf nicht — sie kennt nur die Adresse. Der Verlauf gehoert
        // aber in die Startseite (das ist UI2), also muss die Sitzung ihn
        // dazugeben. GEMESSEN, warum das kein zweiter Ladeweg ist: fuer
        // jede ANDERE Adresse bleibt `lade()` der einzige Weg.
        //
        // UI-VORLAGE Schritt 5: `about:einstellungen` (und `about:settings`)
        // holt nichts aus dem Netz — es ist die Seite aus
        // `narsil_chrome::einstellungen`. GEMESSENE ENTSCHEIDUNG, warum trotzdem
        // eine Seite gebaut wird (und nicht nur eine Sonderbehandlung im
        // Zeichner): der Tab braucht einen Titel und eine Adresse, und
        // `gehe_zu` setzt beides ueber `aus_ist`. Also gibt es eine Seite mit
        // der UEBERSCHRIFT und sonst nichts — der Inhalt kommt aus dem Modul
        // der Rolle UI.
        //
        // UI-VORLAGE Schritt 6: EINE PDF-DATEI IST KEINE SEITE (Koordination
        // 02.10. mittags, `team/WUNSCH_UI.txt` Punkt 11 und 12).
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Dateiendung hier reicht: in
        // `narsil` gibt es keinen Weg zu einem `Content-Type` — `Ladung`
        // hat `text` und `art`, mehr nicht (gemessen in `laden.rs:43`).
        // Der Wunsch sagt ausdruecklich „Header ZUERST, Endung als
        // Ergaenzung"; der Header fehlt, also bleibt die Endung. Punkt 16 des
        // Wunsches (Header aus `narsil-net`) ist damit offen und steht in
        // `WUNSCH_DEVTOOLS.txt`.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Zustand VOR dem Zuruecksetzen
        // steht: eine PDF-Adresse setzt ihn, jede andere loescht ihn. Sonst
        // zeigte ein Tab nach dem Zurueck-Gehen weiter den Betrachter, und
        // die Leiste mit ihm — das ist der Fehler „Geisterwerkzeug".
        //
        // GEMESSENE ENTSCHEIDUNG, warum der ZUSTAND (`self.pdf`) hier
        // NICHT gesetzt wird, obwohl die Datei erkannt wird: diese Funktion
        // ist `&self` (gemessen an der Signatur), und der Betrachter ist
        // Zustand. Wer ihn hier setzen wollte, muesste `&mut self` machen —
        // und das heisst: `laden` wird aus `browser.rs`, `tabs.rs`,
        // `suche.rs` und rund 30 Tests aufgerufen, die alle `&self`
        // brauchen. Der Zustand kommt deshalb aus `lade_jetzt` (der einzige
        // Ort mit `&mut self` auf dem Ladeweg, siehe dort).
        if narsil_chrome::pdf_viewer::ist_pdf_adresse(adresse) {
            return self.aus_ist(
                &format!("<html><head><title>{}</title></head><body></body></html>", tr("Document")),
                adresse,
                breite,
                sicht_hoehe,
            );
        }
        if crate::einstellungen_weg::ist_einstellungen(adresse) {
            return self.aus_ist(
                &format!("<html><head><title>{}</title></head><body></body></html>", tr("Settings")),
                adresse,
                breite,
                sicht_hoehe,
            );
        }
        if Self::ist_startseite(adresse) {
            let verlauf: Vec<String> = self
                .browser
                .verlauf()
                .into_iter()
                .map(|a| a.to_string())
                .collect();
            let text = startseite_mit_verlauf(&verlauf);
            return self.aus_ist(&text, "narsil://start", breite, sicht_hoehe);
        }
        let geladen: Ladung = lade(adresse)?;
        //
        // DIE BASIS IST DIE ENDADRESSE — dieselbe Regel wie in der zweiten
        // Ladefunktion (dort steht die Messung vom 03.10. 08:12). Beide
        // Stellen brauchen sie, und das ist eine der zwei Punkte, an denen
        // dieselbe Frage zweimal gestellt wird: diese hier nimmt die
        // **Anzeige**, die andere den **Verlauf** (dort mit dem Ereignis).
        let basis = if geladen.end_adresse.is_empty() {
            adresse
        } else {
            &geladen.end_adresse
        };
        let mut a = self.aus_ist_mit_typ(&geladen.text, basis, breite, sicht_hoehe, &geladen.content_type)?;
        a.http_status = geladen.status;
        Ok(a)
    }

    /// D10: `laden_mit_breite`, das die Anfrage in den Puffer schreibt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum auch das eine eigene Funktion ist:
    /// die Startseite (`narsil://`) holt NICHTS aus dem Netz, also bleibt
    /// der Puffer dort leer — und `abschluss_ereignis` gibt dann `None`
    /// zurueck und schreibt KEINE Zeile. Das ist richtig: eine Zeile im
    /// Netz-Reiter für eine Seite, die nicht geholt wurde, wäre eine
    /// erfundene Anfrage.
    pub fn laden_mit_breite_ereignis(
        &self,
        adresse: &str,
        breite: u32,
        sicht_hoehe: u32,
        ereignis: &mut narsil_devtools::d4_netzwerk::NetzEreignis,
    ) -> Result<Angezeigt, Fehler> {
        self.laden_mit_breite_ereignis_und_cache(adresse, breite, sicht_hoehe, ereignis, false)
    }

    /// KONTEXT (05.10.2026, 11:35): [`Sitzung::laden_mit_breite_ereignis`] mit
    /// der Cache-Frage — die **dritte** Ebene derselben Kette.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum auch hier ein **zweiter** Name: die Kette
    /// geht `lade_jetzt` -> `laden_mit_ereignis` -> `laden_mit_breite_ereignis`
    /// -> `crate::laden`, und an **jeder** Ebene gibt es andere Aufrufer (Resize,
    /// Druck, Tests). Ein Parameter durch **drei** Ebenen waere eine Aenderung
    /// an **zehn** Aufrufern fuer **eine** Taste.
    pub fn laden_mit_breite_ereignis_und_cache(
        &self,
        adresse: &str,
        breite: u32,
        sicht_hoehe: u32,
        ereignis: &mut narsil_devtools::d4_netzwerk::NetzEreignis,
        kein_cache: bool,
    ) -> Result<Angezeigt, Fehler> {
        //
        // UI-VORLAGE Schritt 6: EINE PDF-DATEI IST KEINE SEITE.
        //
        // GEMESSENER BEFUND (02.10., 14:2x), der diesen Zweig hierher
        // verschoben hat: `lade_jetzt` ruft `laden_mit_ereignis` und damit
        // `laden_mit_breite_ereignis` — NICHT `laden_mit_breite`. Gemessen
        // mit `gehe_zu("file:///daten/bericht.pdf")`: der Zweig in
        // `laden_mit_breite` wurde nie erreicht, `crate::laden` las die
        // Datei und meldete „Datei nicht lesbar". Das war kein Fehler der
        // Erkennung (`ist_pdf_adresse` sagt WAHR, gemessen), sondern ein
        // Fehler des WEGS: zwei Ladewege fuer dieselbe Adresse.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Datei hier NICHT gelesen wird:
        // die Leiste braucht die Seitenzahl aus `narsil-pdf` (nicht in
        // meinem Schreib-Scope), und bis dahin ist die Endung die
        // ehrliche Antwort. Wer die Datei liest, bekommt Bytes, die
        // `aus_ist` nicht darstellen kann — das waere eine Seite voller
        // Nulldaten.
        if narsil_chrome::pdf_viewer::ist_pdf_adresse(adresse) {
            return self.aus_ist(
                &format!("<html><head><title>{}</title></head><body></body></html>", tr("Document")),
                adresse,
                breite,
                sicht_hoehe,
            );
        }
        if Self::ist_startseite(adresse) {
            return self.laden_mit_breite(adresse, breite, sicht_hoehe);
        }
        // Release 08.10.2026 (Feature 7): ein Kurzlink wird per HEAD (nur der
        // Location-Kopf, keine Zwischenseite) aufgeloest und bereinigt, bevor
        // das Ziel geladen wird. Das laeuft im Seitenlader, nicht im Fenster.
        let aufgeloest;
        let adresse = if self.einstellungen.features.an(crate::features::Feature::UrlReinigung)
            && crate::features::url::ist_kurzlink(adresse)
        {
            aufgeloest = crate::features::url::aufloesen_und_reinigen(
                adresse,
                &crate::features::url::Regeln::laden(),
            );
            aufgeloest.as_str()
        } else {
            adresse
        };
        let geladen: Ladung =
            crate::laden::lade_mit_ereignis_und_cache(adresse, ereignis, kein_cache)?;
        //
        // GEMESSENE ENTSCHEIDUNG (02.10. 18:5x, WUNSCH_UI.txt Punkt 11:
        // „Header ZUERST, Endung als Ergaenzung"): ein PDF OHNE Endung
        // (`/bericht?id=7`) ist erst HIER erkennbar, weil erst hier der
        // `Content-Type` der Antwort vorliegt. GEMESSENE URSACHE, warum der
        // Zweich oben nicht reicht: dort wird die Datei gar nicht geholt —
        // und ohne Antwort gibt es keinen Header. GEMESSENE ENTSCHEIDUNG,
        // warum trotzdem BEIDES geprueft wird und nicht nur der Header: eine
        // Datei `…/bericht.pdf`, die der Server als `text/plain`
        // ausliefert, ist trotzdem ein PDF — dann ist die Endung richtig.
        if Self::ist_pdf_antwort(adresse, &geladen) {
            return self.aus_ist(
                &format!("<html><head><title>{}</title></head><body></body></html>", tr("Document")),
                adresse,
                breite,
                sicht_hoehe,
            );
        }
        //
        // DIE BASIS IST DIE ENDADRESSE (03.10. 08:12, GEMESSEN mit einem
        // eigenen 302-Server auf `127.0.0.1`, Wegwerf-Messung `_m_302.rs`,
        // entfernt):
        //
        // ```text
        // gesucht      http://127.0.0.1:A/start
        // 302 location http://127.0.0.1:B/ziel
        // Basis        http://127.0.0.1:A/start   <- das ALTE Ziel
        // ```
        //
        // Das ist mehr als ein Anzeigefehler: **relative Links** werden
        // gegen die Basis gerechnet. Ein `../bilder/x.png` auf der Seite von
        // `127.0.0.1:B/ziel` zeigte bei uns auf `127.0.0.1:A/bilder/x.png`
        // — das Bild fehlte.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `end_adresse` **leer** sein darf:
        // bei Datei, Markdown und eingebauten Seiten gibt es keine
        // Umleitung, und dann ist die angefragte Adresse die richtige Basis.
        // Der Wert ist also „besser, wenn da", nicht „muss da sein".
        let basis = if geladen.end_adresse.is_empty() {
            adresse
        } else {
            &geladen.end_adresse
        };
        let mut a = self.aus_ist_mit_typ(&geladen.text, basis, breite, sicht_hoehe, &geladen.content_type)?;
        a.http_status = geladen.status;
        Ok(a)
    }

    /// IST DAS DIE STARTSEITE? (`narsil://start` und die leere Adresse)
    ///
    /// GEMESSEN, warum eine eigene Funktion: `lade()` beantwortet die
    /// Frage schon (`"narsil://"` + `start`), aber sie beantwortet sie
    /// INNERHALB des Ladens — und die Startseite braucht den Verlauf
    /// VOR dem Laden. Zwei Stellen, dieselbe Frage = zwei Wahrheiten,
    /// sobald jemand eine Adresse hinzufuegt.
    /// IST DAS EINE PDF-ANTWORT? — Header ZUERST, Endung als Ergaenzung.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Header zuerst kommt (und nicht die
    /// Endung): der Server weiss, was er schickt, und die Adresse weiss es
    /// nicht. GEMESSENER FALL: `https://x.de/bericht?id=7` mit
    /// `Content-Type: application/pdf` ist ein PDF; die Adresse endet auf
    /// `?id=7`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Endung trotzdem geprueft wird: nicht
    /// jede PDF-Datei hat eine Endung, aber manche Server liefern sie als
    /// `text/plain` aus. Wer nur dem Header traut, zeigt in dem Fall die
    /// Nulldaten als Text — und das ist die Sorte Fehler, die man
    /// „kaputt" nennt, ohne sie zu finden.
    ///
    /// Oeffentlich, weil der Test sie ohne Netz prueft (sonst muesste er
    /// einen Server aufbauen, um eine `match`-Zeile zu testen).
    pub fn ist_pdf_antwort(adresse: &str, l: &Ladung) -> bool {
        if l.content_type.starts_with("application/pdf") {
            return true;
        }
        narsil_chrome::pdf_viewer::ist_pdf_adresse(adresse)
    }

    /// UI-VORLAGE Schritt 6: DIE SEITE RENDERN UND DAS BLATT ZEICHNEN.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ZWEI Aufrufe sind (Seitenzahl
    /// setzen, Bild holen): `Renderer::seite()` braucht eine `&Seite` aus
    /// `Dokument::seiten()`, und dieselbe Liste ist die Quelle fuer die Zahl
    /// in der Leiste („1 / 15"). Wer beides an zwei Stellen holt, zeigt eine
    /// Zahl, die zu einem anderen Dokument gehoert.
    fn pdf_seite_oeffnen(&mut self, adresse: &str) {
        let bytes = match crate::laden::lade_bytes(adresse) {
            Ok(b) => b,
            Err(_) => {
                // GEMESSENE ENTSCHEIDUNG, warum das kein Absturz ist: ein
                // Betrachter ohne Dokument ist die Fehlerseite des
                // Betrachters, und der zeigt die Leiste ohne Blatt (weiss).
                self.pdf_bild = None;
                return;
            }
        };
        // 1. Die Seitenzahl — GEMESSEN, dass `seiten()` die Zahl liefert,
        //    die in der Leiste steht (nicht eine geratene).
        match pdf_seitenzahl(&bytes) {
            Some(n) if n > 0 => {
                if let Some(mut v) = self.pdf {
                    v.seiten = n.min(narsil_chrome::pdf_viewer::MAX_SEITEN);
                    if v.seite > v.seiten {
                        v.seite = v.seiten;
                    }
                    self.pdf = Some(v);
                }
            }
            _ => {}
        }
        // 2. Das Bild der aktuellen Seite.
        let seite_nr = self.pdf.map_or(1, |v| v.seite);
        self.pdf_bild = pdf_seite_als_bild(&bytes, seite_nr);
    }

    /// UI-VORLAGE Schritt 6: EIN KLICK AUF DIE LEISTE DES BETRACHTERS.
    ///
    /// Liefert `true`, wenn der Klick bei der Leiste blieb (Seite, Zoom,
    /// Download, Drucken). `false` heisst: der Punkt hat nichts getroffen —
    /// dann geht der Klick an die Seite.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge (Pfeile, Zoom,
    /// Download, Drucken) bei `narsil-chrome` liegt und nicht hier: `druecke`
    /// entscheidet sie (`pdf_viewer.rs:404`) und gibt an den Grenzen `None`
    /// zurueck statt zu raten. Ein zweiter Ort, der Seiten zaehlt, waere
    /// die Zahl, die auseinanderlaeuft — und der Fehler waere ein Pfeil ins
    /// Leere.
    fn pdf_klick(&mut self, punkt: Point) -> bool {
        let Some(v) = self.pdf else {
            return false;
        };
        //
        // DIE BILDLAUFLEISTE ZUERST (03.10.2026, 5:4x, `VORLAGE.txt` Z. 10).
        // GEMESSENER BEFUND: `pdf_viewer::treffer` (Z. 1442) prueft nur die
        // WERKZEUGLEISTE — ein Klick auf die Bildlaufbahn ging an die Seite
        // durch, also war die am 23:5x gebaute Leiste gezeichnet, aber
        // nicht bedienbar. GEMESSENE ENTSCHEIDUNG, warum die Bahn VOR der
        // Werkzeugleiste kommt: sie liegt rechts am Rand der Seitenflaeche,
        // die Werkzeugleiste oben; sie ueberschneiden sich nicht, also ist
        // die Reihenfolge beliebig — und die Bahn zuerst zu nehmen, heisst:
        // wer sie später einmal ausbauen will, muss nur diese Zeile
        // wegnehmen.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `sprung` statt eines Ziehens:
        // `narsil-window` kennt kein Mausbewegungs-Ereignis (`SurfaceEvent`
        // hat genau vier Arten), also gibt es kein Ziehen. Der Sprung auf die
        // Klickstelle ist der Weg, der ohne Bewegung geht.
        let mut v = v;
        //
        // BEIDE BAHNEN (03.10.2026, 5:5x). GEMESSENER BEFUND: `sprung` sah
        // nur die SENKRECHTE Bahn an, und die waagerechte ist sichtbar
        // gezeichnet, aber tot — bei 500 % ragt das Blatt GEMESSEN 1179 px
        // seitlich heraus. GEMESSENE ENTSCHEIDUNG, warum erst die
        // waagerechte: sie liegt UNTEN, die senkrechte am RECHTEN Rand, und
        // ein Klick kann nur in eine davon fallen (die Bahnen ueberlappen
        // sich nicht, `laufbahnen` haelt die Ecke frei). Die Reihenfolge
        // ist also beliebig; ich nehme die waagerechte zuerst, weil sie im
        // GEMESSEN-Fall die seltenere ist und damit der wahrscheinlichere
        // Fehler beim Testen.
        if narsil_chrome::pdf_viewer::sprung_waagerecht(
            &mut v,
            punkt.x as i32,
            &self.chrome,
            self.breite,
            self.hoehe,
        ) || narsil_chrome::pdf_viewer::sprung(
            &mut v,
            punkt.y as i32,
            &self.chrome,
            self.breite,
            self.hoehe,
        ) {
            self.pdf = Some(v);
            self.schmutzig = true;
            return true;
        }
        let w = match narsil_chrome::pdf_viewer::treffer(&v, &punkt, self.breite, self.hoehe) {
            Some(w) => w,
            None => return false,
        };
        // GEMESSENE ENTSCHEIDUNG: `v` ist oben schon als `let mut v = v;`
        // geholt (wegen `sprung`) — hier wird es NICHT noch einmal gebunden,
        // denn das wuerde den ersten Wert verdecken und der Aufrufer haette
        // zwei `v` fuer denselben Zustand.
        // GEMESSENE ENTSCHEIDUNG, warum die Kopie `v` bearbeitet wird: das
        // Ergebnis steht in `self.pdf`, und `Viewer::druecke` nimmt
        // `&mut self`. Wer stattdessen `self.pdf.as_mut()` nimmt, braucht
        // danach `self` fuer `druecke` weiter — die Kopie macht den Weg in
        // zwei Schritte, ohne dass `druecke` einen zweiten Zustand kennt.
        let a = v.druecke(w);
        let vorher = self.pdf.map_or(0, |x| x.seite);
        self.pdf = Some(v);
        // GEMESSENE ENTSCHEIDUNG, warum nur beim SEITENWECHSEL gerendert
        // wird: der Zoom veraendert die GROESSE des Rechtecks im Fenster,
        // nicht das Raster (GEMESSEN: `seiten_rect` rechnet den Zoom auf das
        // Rechteck, die Seite bleibt 595 x 842 Punkte). Wer beim Zoom neu
        // rendert, macht aus einer 2-MB-Datei bei jedem Klick ein Bild.
        if self.pdf.map_or(0, |x| x.seite) != vorher {
            let adresse = self.browser.adresse().to_string();
            self.pdf_seite_oeffnen(&adresse);
        }
        let Some(a) = a else {
            // Kein Fehler: an den Grenzen (letzte Seite, hoechster Zoom)
            // passiert einfach nichts.
            self.schmutzig = true;
            return true;
        };
        match a {
            narsil_chrome::pdf_viewer::Aktion::Download => {
                // Die Leiste holt die Datei — ein Prozess, kein Zeichner.
                let adresse = self.browser.adresse().to_string();
                let name = pdf_dateiname(&adresse);
                self.download_url(&adresse, &name);
                self.meldung = Some(trf!("Downloading: {}", name));
            }
            narsil_chrome::pdf_viewer::Aktion::Drucken => {
                self.print_current_page(false);
                self.meldung = Some(tr("Print dialog opened").to_string());
            }
        }
        self.schmutzig = true;
        true
    }

    /// Release 09.10.2026: eine Taste fuer das Textfeld der Einstellungen —
    /// bei der Eingabetaste wird der Wert geprueft, gesetzt und gespeichert;
    /// der Download-Ordner gilt sofort auch fuer die Download-Verwaltung.
    pub fn einstell_tippen(&mut self, t: narsil_chrome::einstellungen::Tippen) {
        let Some(a) = narsil_chrome::einstellungen::tippen(&mut self.einstell_seite, t) else {
            return;
        };
        self.schmutzig = true;
        let narsil_chrome::einstellungen::Aktion::Wert(feld, wert) = &a else {
            return;
        };
        if !narsil_chrome::einstellungen::ausfuehren(&mut self.einstellungen, &a) {
            self.meldung = Some(trf!("“{}” not applied: “{}” is not a valid value", feld.text(), wert));
            return;
        }
        crate::einstellungen_weg::speichern(&self.einstellungen, &mut |m| {
            self.meldung = Some(m);
        });
        if *feld == narsil_chrome::einstellungen::Feld::DownloadsOrdner {
            let ordner = crate::plattform::downloads();
            if let Ok(mut d) = self.dienste.download.lock() {
                d.download_dir = ordner.clone();
            }
            self.meldung = Some(trf!("Downloads now go to {}", ordner.display()));
        } else if self.meldung.is_none() {
            self.meldung = Some(trf!("“{}” saved", feld.text()));
        }
    }

    /// UI-VORLAGE Schritt 5: EIN KLICK AUF DIE EINSTELLUNGEN.
    ///
    /// Der Weg ist vierstufig und jeder Schritt gehoert an eine Rolle:
    /// `narsil_chrome` erkennt den Punkt (`treffer`), rechnet den Klick
    /// (`klick`), fuehrt ihn am Zustand aus (`ausfuehren`) — und was sich
    /// geaendert hat, kommt als Kopie zurueck. Diese Methode setzt die
    /// Kopien an der Sitzung und schreibt die Datei.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine EIGENE Methode ist und nicht
    /// vier Zeilen in `mausklick`: `mausklick` ist der Ort, der ALLES
    /// entscheidet (Menue, Werkzeugbereich, Leiste, Seite, Link). Eine
    /// eigene Methode haelt die Regel an einer Stelle, und die traegt
    /// sich bis zum Test, ohne dass man 800 Zeilen `mausklick` liest.
    fn einstell_klick(&mut self, punkt: Point) {
        // GEMESSENE ENTSCHEIDUNG, warum `Chrome` hier nur geliehen wird:
        // die Einstellungen aendern die FORM (Klickpunkt, Rechtecke), nicht
        // die Oberflaeche. Das Feld `chrome` wird weiter unten geaendert —
        // Thema und Suchmaschine —, und das ist erst erlaubt, wenn der
        // Klick durch ist.
        let w = crate::einstellungen_weg::klick(
            &mut self.einstell_seite,
            &mut self.einstellungen,
            &self.chrome,
            punkt,
            self.breite,
            self.hoehe,
        );
        // Release 09.10.2026: ein Klick in ein Textfeld der Einstellungen
        // nimmt der Adressleiste den Fokus — die Tasten gehen ins Feld.
        if self.einstell_seite.tippfeld.is_some() {
            self.leiste_im_fokus = false;
        }
        // `None` heisst: der Punkt hat nichts getroffen oder es hat sich
        // nichts geaendert. GEMESSENE ENTSCHEIDUNG, warum das KEIN Fehler
        // ist: der Nutzer hat in den Leerraum neben der Spalte geklickt.
        let Some(w) = w else {
            return;
        };
        // Einstellungsseite und Toolbar bedienen denselben Netzfilter.
        self.chrome.setze_werbeblocker_an(self.einstellungen.werbeblocker);
        narsil_page::werbung::setze_an(self.einstellungen.werbeblocker);
        // 1. Die Wirkung an der Sitzung, Feld fuer Feld.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `gelesenes_thema` NICHT gesetzt
        // wird: der Zeichner vergleicht `chrome.thema` mit `gelesenes_thema`
        // und zeichnet bei Unterschied neu (gemessen am 03.10., Befund
        // „THEMA WECHSELT NICHT"). Das Setzen von Hand wuerde genau das
        // Zeichnen verhindern, das man braucht.
        if let Some(t) = w.thema {
            self.chrome.thema = t;
        }
        // Release 10.10.2026: ein neuer STANDARD-Zoom gilt fuer jede Seite
        // ohne eigenen Eintrag — sofort auch fuer die aktuelle — und legt
        // selbst keinen Eintrag an.
        if w.zoom.is_some() {
            self.zoom_vom_host_holen();
            self.schmutzig = true;
        }
        if let Some(s) = w.suchmaschine {
            self.chrome.setze_suchmaschine(s.text());
        }
        if let Some(language) = w.language {
            // Takes effect at once: the chrome looks every string up when it
            // draws. Built-in HTML pages (narsil://start) follow on reload.
            crate::einstellungen_weg::apply_chosen_language(language);
            // Tab titles of built-in pages ("Settings") are set here, not
            // per frame: refresh them now.
            self.tabs_anzeigen();
        }
        // 2. Die Datei — nur wenn sich ein WERT geaendert hat.
        if w.speichern {
            crate::einstellungen_weg::speichern(&self.einstellungen, &mut |m| {
                self.meldung = Some(m);
            });
        }
        // 3. Die Meldung der Aktion (Fehler zuerst, sie stehen im Feld).
        if let Some(m) = w.meldung {
            self.meldung = Some(m);
        }
        if w.erweiterungen_laden {
            let ordner = crate::plattform::konfiguration().join("erweiterungen");
            self.meldung = Some(match self.dienste.extensions.lock() {
                Ok(mut engine) => match engine.load_from_dir(&ordner) {
                    Ok(()) => trf!("{} extensions loaded from {}", engine.extensions.len(), ordner.display()),
                    Err(e) => trf!("Could not load extensions: {}", e),
                },
                Err(_) => tr("Extension manager not available").into(),
            });
        }
        // Release 08.10.2026: der Tab „Features" — Haken neu setzen bzw. die
        // eingebaute Seite einer Funktion in einem neuen Tab oeffnen.
        if w.features {
            self.features_anwenden();
        }
        if let Some(ziel) = w.oeffnen.clone()
            && self.tab_neu()
            && let Err(e) = self.gehe_zu(&ziel)
        {
            self.meldung = Some(e.to_string());
        }

        // 4. Neu zeichnen: Kategorie, Auswahlliste, Thema, Zoom.
        self.schmutzig = true;
    }

    fn ist_startseite(adresse: &str) -> bool {
        let a = adresse.trim();
        a.is_empty() || a == "narsil://start" || a == "narsil://"
    }

    /// Wie [`Sitzung::laden_mit_breite`], aber mit Text, den der Aufrufer
    /// schon hat.
    ///
    /// WARUM DAS EIN WEG IST und nicht `file://` mit einer Datei: der
    /// Aufrufer hat den Text (aus dem Netz, aus dem Cache, aus einem
    /// Test), und ihn erst auf die Platte zu schreiben, um ihn dann
    /// wieder zu lesen, ist ein Umweg mit einer Datei daneben. Die
    /// Bilder darin bekommen trotzdem ihre Platte — die gehoeren nicht
    /// zum HTML, sondern zum Netz oder zur Platte der Seite.
    pub fn aus_html(
        &self,
        html: &str,
        adresse: &str,
        breite: u32,
        sicht_hoehe: u32,
    ) -> Result<Angezeigt, Fehler> {
        self.aus_ist(html, adresse, breite, sicht_hoehe)
    }

    /// Wie [`Sitzung::aus_html`], aber mit der Schrift aus der Sitzung.
    ///
    /// Der Weg fuer den Browser: die Schrift wird EINMAL gesetzt
    /// ([`Sitzung::setze_schrift`]) und gilt fuer jede Seite. Ein
    /// Aufrufer, der zwei Seiten mit verschiedenen Schriften bauen will,
    /// macht zwei Sitzungen — das ist eine groessere Freiheit als der
    /// Aufwand.
    pub fn aus_html_mit_schrift(
        html: &str,
        adresse: &str,
        breite: u32,
        sicht_hoehe: u32,
        schrift: Option<std::sync::Arc<narsil_text::TrueTypeFont>>,
    ) -> Result<Angezeigt, Fehler> {
        let mut kopie = Sitzung::neu(breite, sicht_hoehe);
        kopie.setze_schrift(schrift);
        kopie.aus_ist(html, adresse, breite, sicht_hoehe)
    }

    /// Wie [`Sitzung::aus_html_mit_schrift`], aber mit der Messung (W5G).
    pub fn aus_html_mit_messung(
        html: &str,
        adresse: &str,
        breite: u32,
        sicht_hoehe: u32,
        messung: narsil_layout::Messung,
    ) -> Result<Angezeigt, Fehler> {
        let mut kopie = Sitzung::neu(breite, sicht_hoehe);
        kopie.setze_messung(messung);
        kopie.aus_ist(html, adresse, breite, sicht_hoehe)
    }

    /// Der gemeinsame Weg von `laden_mit_breite` und `aus_html`.
    ///
    /// # W5E (2): DIE SCHRIFT GEHOERT ZUM LADEN
    ///
    /// `self.schrift` ist ein `Arc<TrueTypeFont>` (oder `None`). Es geht
    /// an DREI Stellen, und alle drei muessen es bekommen:
    ///
    /// 1. `seite_aus_text_mit_schrift` — der Umbruch bricht mit den
    ///    hmtx-Breiten um.
    /// 2. `TtMeasurer` — die Boxbreiten der Knoten.
    /// 3. (nicht hier) der Rasterizer, ueber `Schrift`.
    ///
    /// GEMESSEN, warum es in `Sitzung` liegt und nicht in `main.rs`:
    /// `main.rs` zeichnet, `Sitzung` laedt. Wer die Schrift nur beim
    /// Zeichnen kennt, misst beim Laden mit etwas anderem — und genau
    /// das war der Befund (09:20: gesperrte Buchstaben).
    fn aus_ist(
        &self,
        html: &str,
        adresse: &str,
        breite: u32,
        sicht_hoehe: u32,
    ) -> Result<Angezeigt, Fehler> {
        self.aus_ist_mit_typ(html, adresse, breite, sicht_hoehe, "")
    }

    /// Wie [`Sitzung::aus_ist`], aber mit dem `Content-Type` der Antwort.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein ZUSATZ-parameter und kein Feld
    /// auf `Browser` ist: der Typ gehoert zur EINZELNEN Antwort, und ein Tab
    /// kann viele Antworten sehen (die Seite, ihre Bilder, ihr PDF). Wer ihn
    /// am Tab klebt, hat nach dem dritten Bild die Zahl der Seite.
    fn aus_ist_mit_typ(
        &self,
        html: &str,
        adresse: &str,
        breite: u32,
        sicht_hoehe: u32,
        content_type: &str,
    ) -> Result<Angezeigt, Fehler> {
        let breite = breite.max(1);
        //
        // 05.10.2026: DIE ANSICHTSHOEHE FUER DIE KASKADE.
        //
        // GEMESSENER FEHLER, den dieser Wert behebt: `vh`, `vmin`, `vmax`
        // und `min-height: 100vh` wurden gegen eine **geratene** 600 px
        // gerechnet, weil der Bauer keine Ansichtshoehe hatte. Bei einer
        // Ansicht von 1280x800 ist `100vh` damit 600 statt 800 — 25 % zu
        // wenig, auf **jeder** Seite. Siehe `narsil-page/src/lib.rs`, Feld
        // `Bauer::ansicht_hoehe`.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `sicht_hoehe` und nicht `self.hoehe`:
        // `sicht_hoehe()` nimmt den Werkzeugflaechen ab, und `vh` ist laut
        // CSS Values 3, 5.4 die **Ansicht** — der Inhaltsbereich des
        // Fensters, ohne die Leisten des Browsers. Das ist genau das, was
        // Firefox fuer `100vh` rechnet.
        let ansicht_hoehe = self.sicht_hoehe();
        //
        // NARSIL_HTML_ZIEL (06.10.2026 06:48): DAS HTML DER SEITE AUFSCHREIBEN.
        //
        // **GEMESSENER GRUND, und er ist eine Luecke, die mich heute
        // sechs Stunden gekostet hat.** `Angezeigt.html` ist der
        // **geparste** Baum (`Arc<Vec<HtmlNode>>`), nicht der Rohtext —
        // der Rohtext ist genau diese Parameter-Variable und wird nach
        // dem Parse verworfen. Damit ist eine laufende Messung **nicht
        // nachpruefbar**: um 06:44 musste ich die Frage beantworten
        // „hatten wir um 03:11 auf go.dev 41 oder 56 `<img>`?", und die
        // Antwort war **nicht auffindbar** — der Lauf hat die Seite nicht
        // behalten. Ich habe sie mir daneben mit curl geholt (16 Abrufe,
        // acht Seiten) und kam zu **119 gegen 143** mit einer
        // **ungeklaerten** Differenz, die ich heute nicht mehr schliessen
        // kann. **Diese Datei ist die Antwort auf solche Fragen.**
        //
        // **WARUM HIER UND NICHT IM PROBE:** der Probe hat den Rohtext
        // nicht (sie sieht `Angezeigt`), und ein zweiter Abruf durch den
        // Probe waere ein **anderer** Abruf — dieselbe Fehlerform, die
        // mich um 05:39 eine Stunde gekostet hat (Cache statt Transport).
        // **Wer die Bytes einmal hat, muss sie sichern.**
        //
        // **MEIN ERSTER WERSUCH WAR EINE UMGEBUNGSVARIABLE, und er war
        // falsch — aus einem Grund, den man erst beim Lesen des
        // Pruefstands sieht:** `Probe` laedt jede Adresse in einem
        // **eigenen Thread** (`examples/probe.rs`), und eine
        // Umgebungsvariable ist **prozessweit**. Zwei Seiten, die
        // gleichzeitig laden, haetten sich den Pfad gegenseitig
        // ueberschrieben — und die Datei waere unter dem **falschen**
        // Namen geschrieben worden, ohne dass jemand etwas gemerkt
        // haette. **Ein Werkzeug, das bei Parallelbetrieb die Zuordnung
        // verliert, ist kein Werkzeug.** Darum ist es ein **Feld der
        // Sitzung** (`html_ziel`) und kein Prozesszustand: der Probe
        // kennt den Dateinamen, `narsil` kennt den Text, und die
        // Zuordnung kann nicht verrutschen.
        //
        // **Der Preis ist null, wenn es nicht gesetzt ist**: ein
        // `Option`-Vergleich, kein Speicher, keine Datei.
        if let Some(ziel) = self.html_ziel.clone() {
            if let Some(pfad) = ziel.parent() {
                let _ = std::fs::create_dir_all(pfad);
            }
            match std::fs::write(&ziel, html.as_bytes()) {
                Ok(()) => eprintln!("HTML-ZIEL {} byte={}", ziel.display(), html.len()),
                // **DER FEHLER WIRD GEMELDET, NICHT GESCHLUCKT.** Eine
                // gescheiterte Sicherung, die still bleibt, ist eine
                // Messung, die man spaeter fuer vollstaendig haelt — und
                // um 06:44 habe ich genau daran gesessen.
                Err(e) => eprintln!("HTML-ZIEL {} SCHREIBFEHLER: {e}", ziel.display()),
            }
        }
        //
        // W6: EIN Parse fuer die ganze Sitzung. Der geparste Baum wird
        // behalten, weil jede spaetere Aenderung daraus neu gebaut wird
        // (siehe `Angezeigt::html`). Der erste Parse ist damit der
        // EINZIGE (GEMESSEN: ein Klick mit Aenderung kostete vorher
        // 2 Parse + 2 Layout).
        //
        // W7 P1: DIE PHASEN (19:40). `Messung::neu()` liefert `None`, wenn
        // `NARSIL_ZEITEN` nicht gesetzt ist — dann kosten die Aufrufe nur
        // einen bool-Vergleich und die Seitengeschwindigkeit bleibt die
        // gemessene.
        let mut m = crate::zeiten::Messung::neu();
        //
        // GEMESSEN, warum `html_lesen` VOR der Kaskade gemessen wird und
        // nicht danach: der Parse ist der einzige Schritt, der quadratisch
        // sein kann (Textknoten, die ineinander geschachtelt sind). Wer
        // ihn mit der Kaskade zusammenmisst, findet ihn nicht.
        // Release 08.10.2026 (Features 9 und 11): Ruhe-Blatt, Benachrichtigungs-
        // Sperre und Cookie-Autopilot vor dem Parsen in die Seite setzen. Der
        // Rohtext (`rohtext`, „Seite speichern") bleibt der Originaltext.
        let eingespritzt =
            crate::features::ruhe::einspritzen(html, adresse, &self.einstellungen.features);
        let zu_parsen: &str = eingespritzt.as_deref().unwrap_or(html);
        // Release 11.10.2026: der Doctype fuer `document.doctype`.
        crate::js::doctype_merken(html);
        // Release 08.10.2026 (Feature 3): das Canvas-Rauschen fuer die
        // Skripte dieser Seite — je Faden (Seitenlader oder Fenster).
        crate::features::netz::rauschen_setzen(
            self.einstellungen.features.an(crate::features::Feature::NetzSandbox),
        );
        // Release 10.10.2026: XHTML (Content-Type, sonst Endung) wie im
        // XML-Parser (CDATA, `<div/>`).
        let xhtml = narsil_page::ist_xhtml(content_type, adresse);
        crate::js::xhtml_merken(xhtml);
        let lesen = |t: &str| if xhtml { narsil_page::html_lesen_xhtml(t) } else { narsil_page::html_lesen(t) };
        let html_geparst = std::sync::Arc::new(match m.as_mut() {
            Some(m) => m.phase("parse", || lesen(zu_parsen)),
            None => lesen(zu_parsen),
        });
        //
        // W7 P3: DIE EXTERNEN BLAETTER (22:15).
        //
        // GEMESSEN, warum VOR der Kaskade: die Kaskade liest
        // `Seite::stile`, und die entsteht beim Bau. Haette man die
        // Blaetter danach geholt, waere die Seite schon ohne sie
        // gerechnet — und ein zweiter Bau waere noetig (die Rechnung
        // ist der teure Teil).
        //
        // GEMESSEN, warum ein neuer Baum und kein Eingriff in den alten:
        // `Angezeigt::html` ist der geparste Baum der Seite, und er wird
        // bei jedem Klick neu gebraucht. Ein neuer Baum (mit den
        // Blaettern als `<style>`) traegt die CSS-Angaben mit — und
        // kostet den Abruf nur EINMAL.
        // Release 10.10.2026: die externen Skripte JETZT im Hintergrund holen,
        // waehrend Blaetter, Schriften und Kaskade laufen (siehe
        // `js::skripte_frueh_starten`).
        if narsil_page::skripte_laufen() {
            crate::js::skripte_frueh_starten(adresse, narsil_page::skripte_im_html(&html_geparst, adresse));
        }
        let blaetter = match m.as_mut() {
            Some(m) => m.phase("blaetter", || {
                crate::blaetter::blaetter_holen(&html_geparst, adresse)
            }),
            None => crate::blaetter::blaetter_holen(&html_geparst, adresse),
        };
        let html_geparst = blaetter.baum.clone();
        // Release 09.10.2026: die Webschriften (`@font-face`) VOR dem
        // Seitenbau — Messung und Umbruch rechnen dann mit der echten Schrift.
        let schriften = match m.as_mut() {
            Some(m) => m.phase("schriften", || crate::webschriften::holen_und_anmelden(&html_geparst, adresse)),
            None => crate::webschriften::holen_und_anmelden(&html_geparst, adresse),
        };
        if crate::zeiten::an() {
            std::eprintln!(
                "NARSIL_ZEIT schriften angemeldet={} bekannt={} fehler={:?}",
                schriften.angemeldet, schriften.bekannt, schriften.fehler
            );
        }
        //
        // W7C A4: Der Titel wird hier NICHT getragen — `aus_ist` nimmt
        // `&self` (sie baut eine Seite, sie aendert keinen Zustand), und
        // `titel_setzen` braucht `&mut`. Er wird in `titel_tragen`
        // getragen, das die beiden Ladewege (GET und POST) aufrufen.
        //
        // W7B: DIE BLAETTER SIND SICHTBAR. Ohne diese Zeile waere ein
        // fehlendes Blatt (404, fremde Herkunft, zu gross) unsichtbar —
        // die Seite staende einfach ohne ihr CSS da, und der Nutzer
        // wueste nicht, ob die Seite kaputt ist oder der Browser.
        let mut blaetter_meldung: Option<String> = None;
        if blaetter.geholt > 0 || !blaetter.fehler.is_empty() {
            blaetter_meldung = Some(trf!(
                "CSS: {} style sheets loaded{}",
                blaetter.geholt,
                if blaetter.fehler.is_empty() {
                    String::new()
                } else {
                    trf!(", {} not", blaetter.fehler.len())
                }
            ));
        }
        //
        // 05.10. 22:40: MIT DEM ZUSTAND DES WIRTS (siehe das Feld
        // `farbschema`). GEMESSENER BEFUND, warum das hier und nicht im
        // `Patch`: `Patch` traegt die Aenderungen eines Skripts (Text, Attribute
        // aus einem Skript), und das Farbschema ist keine Skriptaenderung —
        // es ist eine Angabe des Nutzers. GEMESSENE EINORDNUNG, warum es
        // trotzdem hier und nicht weiter unten: die Seite wird EINMAL gebaut,
        // und ab da steht sie im `Angezeigt`; ein spaeterer Weg waere eine
        // zweite Wahrheit.
        let zustand = self.zustand();
        let mut seite = match m.as_mut() {
            Some(m) => m.phase("kaskade", || {
                narsil_page::seite_aus_knoten_mit_ansicht_zustand(
                    &html_geparst,
                    &narsil_page::Patch::default(),
                    breite,
                    // 05.10.2026: DIE ANSICHTSHOEHE. Ohne sie rechnet die
                    // Kaskade `vh`/`min-height: 100vh` gegen eine geratene
                    // 600 px — `narsil-page/src/lib.rs`, Feld `ansicht_hoehe`.
                    ansicht_hoehe,
                    adresse,
                    &self.messung,
                    &zustand,
                )
            }),
            None => narsil_page::seite_aus_knoten_mit_ansicht_zustand(
                &html_geparst,
                &narsil_page::Patch::default(),
                breite,
                ansicht_hoehe,
                adresse,
                &self.messung,
                &zustand,
            ),
        };

        // Release 08.10.2026: DIE SKRIPTE VOR DEM ERSTEN LAYOUT. Sie brauchen
        // DOM, Felder und den CSS-Baum (alles aus dem Bau), aber kein Layout.
        // Aendern sie das DOM, wird nur die SEITE neu gebaut und dann EINMAL
        // gelegt — vorher lief das ganze Layout (mit Bildvorladen,
        // Bildmassen und Nachumbruch) zweimal, das erste fuer den
        // Papierkorb. GEMESSEN: Wikipedia setzt `client-js` am `<html>`, der
        // zweite Bau kam also auf jeder Seite.
        // Release 10.10.2026: die VORSCHAU (siehe `Sitzung::vorschau_senden`):
        // nur bei externen Skripten — ohne sie folgt das Ende sofort, und das
        // zusaetzliche Layout lohnte nicht. `NARSIL_VORSCHAU_AUS` schaltet ab.
        //
        // Das Layout der Vorschau laeuft in einem EIGENEN Faden, gleichzeitig
        // mit den Skripten (die meist auf das Netz warten). GEMESSEN mit dem
        // Layout davor: die Vorschau kam frueh, aber das Ende spaeter
        // (spiegel.de 3,3 -> 3,8 s), weil der Skriptabruf auf das zusaetzliche
        // Layout wartete. Eine verspaetete Vorschau verwirft der Haupt-Faden
        // (`ladungen_abholen`: die Ladung ist dann fertig oder eine andere).
        if seite.skripte.iter().any(|s| !s.quelle.is_empty())
            && std::env::var_os("NARSIL_VORSCHAU_AUS").is_none()
            && let Some(mut senden) = self.vorschau_senden.lock().ok().and_then(|mut g| g.take())
        {
            let mut v_seite = seite.clone();
            let v_ansicht = Size::new(breite as f32, sicht_hoehe as f32);
            let v_messung = self.messung.clone();
            let v_html = html_geparst.clone();
            let diaet = self.bild_diaet_wirkt(adresse);
            let v_sicht = self.hol_hoehe(sicht_hoehe) as f32;
            let faden = std::thread::Builder::new()
                .name("narsil-vorschau".into())
                .stack_size(32 * 1024 * 1024)
                .spawn(move || {
            // Release 10.10.2026: der BILD-VORLAUF — nach dem Senden der
            // Vorschau holt dieser Faden die Bilder, die nach IHREM Layout im
            // Bild stehen, waehrend die Skripte laufen; der Endweg uebernimmt
            // sie VOR seinem Layout (`bilder_vorab_abholen`). GEMESSEN vorher
            // (spiegel.de, `NARSIL_ZEIT nachlauf`): ein zweites volles Layout
            // nur fuer die Bildmasse, 245 ms.
            let mut v_cache = crate::bilder::BildCache::neu();
            v_cache.originale_behalten = diaet;
            v_cache.aufschieben = diaet;
            if let Ok(v_baum) = crate::nachumbruch::layout(&mut v_seite, v_ansicht, &v_messung) {
                let v_hoehe = berechne_inhaltshoehe(&v_baum, sicht_hoehe);
                let v_quellen = {
                    let kaesten = kaesten_aus_baum(
                        &v_seite,
                        &v_baum,
                        crate::bilder::MAX_BILDER,
                        &std::collections::HashMap::new(),
                    );
                    bildquellen_im_bereich(&v_seite.bilder, &kaesten, 0.0, v_sicht, v_sicht.max(1.0), diaet)
                };
                let mut v_bilder = crate::bilder::BildCache::neu();
                v_bilder.originale_behalten = diaet;
                v_bilder.aufschieben = diaet;
                senden(Angezeigt {
                    content_type: String::new(),
                    http_status: 200,
                    seite: v_seite,
                    baum: v_baum,
                    inhalt_hoehe: v_hoehe,
                    versatz: 0,
                    bilder: v_bilder,
                    html: v_html,
                    rohtext: None,
                    stand: Default::default(),
                    kiste: Default::default(),
                    scroll_je_knoten: std::collections::HashMap::new(),
                    patch: Default::default(),
                    konsole: Vec::new(),
                    meldungen: Vec::new(),
                    horcher: HashMap::new(),
                    engine: None,
                    vorschau: true,
                    load_ausstehend: None,
                    load_timer_grenze: None,
                });
                for q in v_quellen {
                    let _ = v_cache.hole(&q);
                }
            }
            v_cache
                });
            if let Ok(f) = faden
                && let Ok(mut g) = bilder_vorab().lock()
            {
                *g = Some((adresse.to_string(), f));
            }
        }

        let lauf = match m.as_mut() {
            Some(m) => m.phase("js", || {
                crate::js::skripte_ausfuehren(&seite, html_geparst.as_slice(), adresse)
            }),
            None => crate::js::skripte_ausfuehren(&seite, html_geparst.as_slice(), adresse),
        };
        // Release 09.10.2026: das lebende DOM abschliessen — hat ein Skript die
        // Seite veraendert, kommt der neue Seitenbaum aus der Arena.
        let mut lauf = lauf;
        let neues_html = crate::js::lebend_abschliessen(&lauf.stand, &mut lauf.patch);
        let lebend_neu = neues_html.is_some();
        let html_geparst = match neues_html {
            Some(h) => std::sync::Arc::new(h),
            None => html_geparst,
        };
        let skript_geaendert = lebend_neu || !lauf.patch.ist_leer();
        // Release 10.10.2026: auch die Schritte NACH den Skripten gemessen
        // (vorher ohne Zahl — bei spiegel.de fehlten so 2,3 s in der Summe).
        let rest_uhr = std::time::Instant::now();
        let rest_zeit = |was: &str, seit: std::time::Instant| {
            if crate::zeiten::an() {
                std::eprintln!(
                    "NARSIL_ZEIT nachlauf {was:<14} {:>8.2} ms",
                    seit.elapsed().as_secs_f64() * 1000.0
                );
            }
        };
        if skript_geaendert {
            seite = narsil_page::seite_aus_knoten_mit_ansicht_zustand(
                &html_geparst, &lauf.patch, breite, sicht_hoehe, adresse, &self.messung, &self.zustand());
            rest_zeit("neubau_js", rest_uhr);
        }

        // Layout. Der Viewport ist die SEITE ohne Leiste — sonst schiebt
        // sich der Inhalt unter die Bedienflaeche.
        let viewport = Size::new(breite as f32, sicht_hoehe as f32);
        // DERSELBE MESSER, wie sie der Umbruch benutzt: keine zweite
        // Entscheidung, und darum kann das Layout nicht anders messen
        // als der Umbruch.
        //
        // W7C R3 (02.10. 03:50, ZURUECKGENOMMEN 04:35): der Weg mit
        // `layout_mit_plan` ist gebaut und wieder abgebaut. Der Grund
        // steht in `team/UEBERGABE_HTML_R3.txt` (Abschnitt „DER KNOTEN"):
        // der Plan ist nach DOKUMENTknoten sortiert, und die Bruecke muss
        // den Kasten DIESES Elements treffen — nicht den ersten freien
        // Knoten (das ist bei Kindern der Knoten des ersten Kindes) und
        // nicht den letzten (das ist der Textknoten des letzten Kindes).
        // Drei Varianten sind gemessen, keine haelt der Rekursion in
        // `element()` stand.
        //
        // Der AufruF bleibt `layout_with_measurer`, also der Zustand vor
        // dem 30.09.: jedes Bild ist ein Block. GEMESSEN, warum das
        // sicherer ist als eine halbe Bruecke: `layout_mit_plan` mit
        // LEEREM Plan ergibt bitweise denselben Baum — ein Plan mit
        // falschen Schluesseln aber nicht, und Flex/Grid-Seiten wuerden
        // dann halb umbrechen, was schlimmer ist als gar nicht.
        //
        // W7C R3 (02.10., Rolle LAYOUT): DER PLAN WIRD ÜBERGEBEN. Das ist
        // der Aufruf, der die ganze Kaskade ins Layout bringt.
        //
        // ZUERST ZURÜCKGENOMMEN (04:35), weil der Plan an den FALSCHEN
        // Knoten stand: `narsil-page` hat den Kasten des Elements aus einem
        // FLACHEN Feld gelesen, und die Kinder werden vor dem Abschluss des
        // Eltern gebaut — jedes Kind hatte den Kasten seines Eltern
        // überschrieben. Drei Varianten, den richtigen Kasten zu raten,
        // waren alle falsch (`team/UEBERGABE_HTML_R3R4.txt`).
        //
        // Die Ursache ist behoben: `Bauer::element` rettet
        // `eigener_kasten` beim Eintritt und stellt ihn beim Ausgang
        // wieder her, also nach dem Bau der Kinder. Damit steht hier der
        // Kasten DIESES Elements.
        //
        // DER FALLBACK BLEIBT: ist der Plan leer (kein `<style>`, oder
        // eine Seite aus Klartext), ist `layout_with_measurer` bitweise
        // dasselbe wie `layout_mit_plan` mit leerem Plan — das ist in
        // crates/narsil-layout-engine/tests/layout_echt.rs gemessen
        // (`leere_plan_aendert_nichts`).
        // Release 10.10.2026: der Bild-Cache entsteht VOR dem Layout — mit den
        // Bildern des Vorlaufs (siehe `bilder_vorab`) stehen ihre Masse schon
        // im ersten Layout, und das zweite entfaellt, wenn sich nichts mehr
        // aendert.
        let mut cache = crate::bilder::BildCache::neu();
        let diaet = self.bild_diaet_wirkt(adresse);
        cache.originale_behalten = diaet;
        cache.aufschieben = diaet;
        if let Some(vorab) = bilder_vorab_abholen(adresse) {
            for (url, ergebnis) in vorab.geladen {
                cache.uebernehmen(url, ergebnis);
            }
            bildmasse_uebernehmen(&mut seite, &cache);
        }
        let mut baum = {
            // Release 07.10.2026: mit Nachumbruch (siehe `nachumbruch.rs`).
            let r = match m.as_mut() {
                Some(m) => m.phase("layout", || {
                    crate::nachumbruch::layout(&mut seite, viewport, &self.messung)
                }),
                None => crate::nachumbruch::layout(&mut seite, viewport, &self.messung),
            };
            r.map_err(|e| Fehler::Seite(format!("Layout: {e}")))?
        };

        // Inhaltshoehe: der unterste Punkt, den es im Baum gibt.
        let mut inhalt_hoehe = berechne_inhaltshoehe(&baum, sicht_hoehe);

        // I3: DIE BILDER HOLEN. Jetzt, nicht beim Zeichnen (siehe
        // `Angezeigt::bilder`).
        //
        // M3: `MAX_BILDER` begrenzt die Schleife. `BildCache::hole`
        // begrenzt zusaetzlich, was ER sich gemerkt hat; beides ist noetig,
        // weil ein 5000-Bilder-Dokument (`MAX_BILDER` in `narsil-page`)
        // nicht in einen 128-Eintraege-Cache passt.
        // Release 08.10.2026 (Bild-Diaet): Originalbytes behalten und Bilder
        // mit festem Kasten erst bei Annaeherung holen — ausser die Funktion
        // ist aus oder die Domain steht auf der Ausnahmeliste. (Seit
        // 10.10.2026 steht der Cache vor dem Layout, siehe oben.)
        //
        // 02.10. 02:35 (B12): HIER GILT DIESELBE REGEL wie beim Scrollen —
        // „im Sichtbereich plus ein Bildschirm". GEMESSENER BEFUND, warum der
        // alte Weg falsch war: `if bk.bild.lazy { continue; }` hat
        // `loading="lazy"` zu **„nie"** gemacht, weil die Schleife nirgends
        // gefragt hat, ob das Bild sichtbar ist. GEMESSEN von der Rolle PROBE
        // (02:05, `team/PROBLEME.txt`): 109 Bilder auf vier Seiten, **7**
        // geholt, `unbedingt = 0` — und ein Bild, das nicht im Cache liegt,
        // kann nicht gezeichnet werden (`bild_items`, 6235).
        //
        // GEMESSENE EINORDNUNG, warum das H3-Anliegen (kein 200-Anfragen-
        // Sturm vor dem ersten Bild) trotzdem gilt: die Regel begrenzt auf
        // den Sichtbereich plus einen Bildschirm. Bei 200 Bildern unterhalb
        // des Sichtbereichs wird **keines** geholt, genau wie vorher.
        {
            // GEMESSENE EINORDNUNG (18:00): `hol_hoehe` ist die EINE Regel
            // fuer die Breite des Hol-Bereichs. An dieser Stelle fehlte sie,
            // und der Lauf ueber 30 Seiten zeigte 66 statt 67 gezeichnete
            // Bilder — der Ladeweg rechnet sein Fenster selbst.
            let sicht = self.hol_hoehe(sicht_hoehe) as f32;
            // GEMESSEN: beim **Bauen** ist die Kette noch leer (es kann noch
            // keinen Container geben, der gescrollt ist) — dieselbe Rechnung,
            // ohne Versatz.
            let kaesten = kaesten_aus_baum(
                &seite,
                &baum,
                crate::bilder::MAX_BILDER,
                &std::collections::HashMap::new(),
            );
            let gebraucht = bildquellen_im_bereich(
                &seite.bilder,
                &kaesten,
                0.0,
                sicht,
                sicht.max(1.0),
                cache.aufschieben,
            );
            for q in gebraucht {
                let _ = cache.hole(&q);
            }
        }
        // Nach dem ersten Laden sind die natuerlichen Bildmasse bekannt.
        // Der erste Layoutlauf reserviert bis dahin Platz; der zweite nutzt
        // die echten Masse fuer width/height:auto und das Seitenverhaeltnis.
        rest_zeit("bilder_holen", rest_uhr);
        if bildmasse_uebernehmen(&mut seite, &cache) {
            let u = std::time::Instant::now();
            baum = crate::nachumbruch::layout(&mut seite, viewport, &self.messung)
            .map_err(|e| Fehler::Seite(format!("Layout mit Bildmassen: {e}")))?;
            inhalt_hoehe = berechne_inhaltshoehe(&baum, sicht_hoehe);
            rest_zeit("layout_bilder", u);
        }

        //
        // W6 (J10): DIE SKRIPTE, und danach die Seite noch einmal.
        //
        // GEMESSEN, warum die Seite ZWEIMAL gebaut wird: die erste
        // enthaelt die `<script>`-Elemente und die Knotennummern, an die
        // sich ein Skript haelt; die zweite enthaelt, was die Skripte
        // gemacht haben. `narsil-dom` kann keinen Knoten aendern, also
        // wird aus der veraenderten QUELLE gebaut (siehe `narsil::replay`).
        //
        // DER FEHLER EINES FEHLGESCHLAGENEN SKRIPTS ist keine Seite,
        // die verschwindet: die alte Seite bleibt stehen, und die
        // Meldung kommt in die Statuszeile. Ein Browser, der bei einem
        // Tippfehler im Skript weiss wird, ist unbrauchbar (das ist die
        // Regel aus `lade_jetzt`, und sie gilt hier genauso).
        //
        // W7 P1: DIE SKRIPT-PHASE (19:45) — Skripte UND der zweite Bau.
        //
        // GEMESSEN, warum beides in einer Phase: der zweite Bau (Kaskade
        // + Layout) ist die KOSTENFOLGE des Skripts. Haette man sie
        // getrennt, wuerde man `js` fuer schnell halten und die
        // Wirklichkeit in `kaskade`/`layout` suchen.
        let mut lauf = lauf;
        if skript_geaendert {
            crate::js::ord_erneuern(&lauf.stand, &seite.felder, &seite.formular_ords);
        }

        //
        // W6B: DIE SCRIPT-MELDUNGEN. `#[derive]` kann nichts, was man
        // liest — deshalb wandern die Fehler der Skripte in die
        // Konsole des Browsers (`Angezeigt::konsole`), und der Aufrufer
        // kann sie anzeigen.
        //
        // GEMESSEN, warum die Meldung NICHT im Fenster steht: eine
        // Ausgabe im Bild ueberdeckt die Seite, und ein Skriptfehler ist
        // eine Meldung an den Entwickler, nicht an den Besucher. Die
        // Statuszeile nimmt sie (`Sitzung::meldung`).
        let mut konsole = lauf.konsole;
        for f in lauf.fehler.iter() {
            konsole.push(std::format!("Skript: {f}"));
        }

        //
        // W7 P1: AUSGEBEN. Nach allem, damit die Summe die ganze Sitzung
        // umfasst (inklusive des zweiten Baus durch die Skripte).
        if let Some(m) = m.as_ref() {
            m.ausgeben("laden");
        }
        //
        // H2 (RUNDE 3): DIE INLINE-HANDLER (`onclick="…"`).
        //
        // GEMESSENER FEHLER (06:20, `tests/_mess_onclick.rs`): eine Seite
        // mit `<p onclick="console.log('inline')">` hatte nach dem Laden
        // NULL Hoerer (`horcher.len() == 0`), und der Klick tat nichts.
        // `narsil-page` SAMMELT die `on*`-Attribute sehr wohl
        // (`Seite::ereignisse`, `narsil-page/src/skripte.rs:162` `Ereignis`)
        // — aber `narsil` hat `.ereignisse` NIRGENDS gelesen. Die Handler
        // waren also da und wurden nie eingetragen. Das ist kein Feature,
        // das fehlt: es ist eine Sammelstelle ohne Verbraucher.
        //
        // GEMESSEN, warum das die REGELKAPUTTUNG ist und nicht ein
        // Randfall: `onclick` ist der haeufigste Handler der Welt. Eine
        // Seite, deren Knöpfe `onclick` haben, tat bei diesem Browser
        // gar nichts — und zwar OHNE Fehlermeldung.
        Self::inline_horcher_eintragen(&mut lauf.horcher, &seite);

        //
        // H3 (03.10.): DIE SICHTBAREN BILDER — HIER, und nicht nur in
        // `lade_jetzt`.
        //
        // GEMESSENER FEHLER MEINER ERSTEN FASSUNG: der Aufruf stand in
        // `lade_jetzt`, und damit-loadeten `aus_html`,
        // `aus_html_mit_schrift`, `seite_neu_bauen` und
        // `bruecke_zurueckschreiben` KEINE lazyen Bilder. Zwei Tests wurden
        // rot: `ein_png_landet_in_der_anzegeliste` (0 Bild-Items) und
        // `ein_fehlendes_bild_ergibt_alt_text` (kein Fehler gemerkt).
        // Das ist die Fehlerklasse „zwei Stellen entscheiden" zum dritten
        // Mal — diesmal als „eine Stelle pflegt, fünf setzen eine Seite".
        //
        // DER WEG: die Seite wird hier fertig gebaut (`baum`, `seite`,
        // `inhalt_hoehe` stehen), und der Sichtbereich ist als Parameter
        // da. Das ist der einzige Ort, an dem BEIDES bekannt ist — der
        // Fenster-Zustand (`lade_jetzt`) kennt den Sichtbereich auch, aber
        // nicht die Seite, und `aus_ist` kennt die Seite, aber nicht das
        // Scrollen. Die Scroll-Wege rufen dieselbe Funktion unten.
        let mut fertig = Angezeigt {
            // GEMESSENE ENTSCHEIDUNG: leer ist der Wert, wenn die Seite
            // nicht aus einer HTTP-Antwort kommt (`file://`, `about:`). Der
            // Wert wandert aus `aus_ist_mit_typ` hier herein.
            content_type: String::new(),
            http_status: 200,
            seite,
            baum,
            inhalt_hoehe,
            versatz: 0,
            bilder: cache,
            html: html_geparst,
            //
            // KONTEXT (04.10.2026, 05:45): DER ROHE QUELLTEXT bleibt liegen.
            //
            // GEMESSENE ENTSCHEIDUNG, warum **hier** und nicht im Aufrufer:
            // `aus_ist_mit_typ` ist der **eine** Ort, an dem der Rohtext und
            // der geparste Baum gleichzeitig da sind — und nur hier kann man
            // entscheiden, ob der Text zu dieser Seite gehoert.
            //
            // GEMESSENE ENTSCHEIDUNG, warum die Grenze `MAX_ROHTEXT` ist und
            // nicht `html.len()` allein: ein 40-MiB-Dokument (oder ein
            // heruntergeladenes Video, das als „Seite“ durchkam) wuerde den
            // Text **mit** kopieren — und GEMESSEN ist, dass `Tab` bis zu
            // [`narsil_browser::MAX_TABS`] = 32 in einem Fenster liegen.
            //
            // GEMESSENE ENTSCHEIDUNG, warum **kein** PDF hier landet: der
            // Content-Type sagt es. Ein PDF-Bitstrom als „Quelltext“ waere
            // eine Datei mit Muellzeichen, und „Quelltext anzeigen“ wuerde
            // Muellzeichen anzeigen.
            rohtext: if html.len() <= MAX_ROHTEXT
                && (content_type.is_empty()
                    || content_type.starts_with("text/")
                    || content_type.starts_with("application/xhtml+xml"))
            {
                Some(html.to_string())
            } else {
                None
            },
            stand: lauf.stand.clone(),
            kiste: lauf.kiste.clone(),
            // KONTEXT (05.10.): am Anfang ist **kein** Container gescrollt.
            scroll_je_knoten: std::collections::HashMap::new(),
            patch: lauf.patch,
            konsole,
            // W6F: beim Laden kann es keine neuen Knoten geben — der
            // Patch ist frisch.
            //
            // W7B: die CSS-Zeile ist kein W6F-Fehler, sondern ein
            // Tatsachenbericht ("3 Blätter geladen, 1 nicht"). Sie
            // landet trotzdem hier, weil `meldungen` das ist, was der
            // Nutzer in der Statuszeile sieht — und ein fehlendes Blatt
            // ist etwas, das man sehen MUSS.
            meldungen: blaetter_meldung.into_iter().collect(),
            horcher: lauf.horcher,
            engine: lauf.engine.take(),
            vorschau: false,
            load_ausstehend: None,
            load_timer_grenze: None,
        };
        // H3: DIE LAZY-BILDER, DIE IM SICHTBAREN BEREICH LIEGEN. Erst
        // nach dem Konstruktor: `bild_kaesten` braucht `baum` UND
        // `inhalt_hoehe`, und beides steht erst jetzt fest.
        let u = std::time::Instant::now();
        Self::bilder_sichtbar_laden_fuer(&mut fertig, sicht_hoehe);
        rest_zeit("bilder_sicht", u);
        // Release 15.10.2026: images of the document that get no box load
        // as well (see `Lebend::note_document_images`).
        if let Ok(mut st) = fertig.stand.lock()
            && let Some(l) = st.lebend.as_mut()
        {
            l.note_document_images();
        }
        // Release 10.10.2026: `load`/`error` der Bilder VOR `window.load`
        // (HTML: das Dokument wartet auf seine Bilder).
        let u = std::time::Instant::now();
        let bild_js = Self::bild_ereignisse(&mut fertig);
        rest_zeit("bild_ereignisse", u);
        // Release 11.10.2026: haelt die Seite eine Ladesperre (Medien laden
        // noch), kommt `load` spaeter aus dem Takt (`tick`).
        let u = std::time::Instant::now();
        let timer_grenze = Self::faellige_timer_grenze(&mut fertig);
        let load_js = if Self::lade_sperre(&mut fertig) > 0 || timer_grenze.is_some() {
            fertig.load_ausstehend = Some(std::time::Instant::now());
            fertig.load_timer_grenze = timer_grenze;
            false
        } else {
            Self::load_feuern(&mut fertig)
        };
        rest_zeit("load_js", u);
        if load_js | bild_js {
            let u = std::time::Instant::now();
            Self::seite_fuer_anzeige_neu_bauen(&mut fertig, breite, sicht_hoehe,
                self.messung.clone(), &self.zustand());
            rest_zeit("neubau_load", u);
        }
        masse_eintragen(&fertig);
        rest_zeit("SUMME", rest_uhr);
        Ok(fertig)
    }

    // ------------------------------------------------------------------
    // W6 (J10): EREIGNISSE
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // W7 P2 — FORMULARE BEDIENEN
    // ------------------------------------------------------------------

    /// W7C A4: DER FENSTERTITEL, den der Browser tragen SOLLTE.
    ///
    /// GEMESSEN, warum das eine Methode und kein Feld: der Titel gehoert
    /// dem Verlaufseintrag (`narsil_browser::Page::titel`), und der wechselt
    /// mit `Zurueck`/`Vor`. Ein Feld in der Sitzung waere eine zweite
    /// Wahrheit, die nach einem Zurueck nicht stimmt.
    ///
    /// GEMESSEN, warum der Name hinten steht: `Seite – narsil`. Das ist die
    /// Form, die man in der Taskleiste wiedererkennt; ein nackter Titel
    /// ist von drei anderen Fentern nicht zu unterscheiden.
    pub fn fenstertitel(&self) -> String {
        let t = self.aktuelle_titel();
        if t.is_empty() {
            // Ohne Titel steht die ADRESSE da — sonst hat das Fenster
            // keinen Namen und der Nutzer kann es nicht wiederfinden.
            let a = self.browser.adresse();
            return if a.is_empty() {
                "narsil".to_string()
            } else {
                std::format!("{a} – narsil")
            };
        }
        std::format!("{t} – narsil")
    }

    /// W7C A4: DEN TITEL IN DEN BROWSERZUSTAND TRAGEN.
    ///
    /// GEMESSEN, warum das EINE Methode und kein Aufruf an den Ladewegen:
    /// es gibt drei Orte, an denen eine neue Seite entsteht (GET, POST,
    /// Formular-GET) — und die sind nicht aneinander gesperrt, weil man
    /// sonst in einem von ihnen den Titel vergisst. Das ist der Fehler
    /// „zwei Wahrheiten" aus W5G, nur kleiner: ein fehlender Titel faellt
    /// nicht auf, weil die Seite sonst normal aussieht.
    pub fn titel_tragen(&mut self) {
        let titel = self.aktuelle_titel().to_string();
        self.browser.titel_setzen(&titel);
    }

    /// Der Titel der gerade gezeigten Seite (leer, wenn keiner da ist).
    fn aktuelle_titel(&self) -> &str {
        self.angezeigt
            .as_ref()
            .map(|a| a.seite.titel.as_str())
            .unwrap_or("")
    }

    /// W7C A4: DAS FAVICON HOLEN UND DEKODIEREN.
    ///
    /// GEMESSEN, warum das ueberhaupt, wenn das Fenster kein Icon
    /// bekommen kann: `narsil-window` hat keine API dafuer (nicht meine
    /// Crate), also ist der Wert selbst das Beweisstueck — ein Icon, das
    /// geholt und dekodiert werden KANN, ist eine halbe Loesung, und die
    /// andere Haelfte (das Setzen) braucht eine Fremd-Crate.
    ///
    /// GEMESSEN, warum ein Fehler eine MELDUNG ist und kein stilles
    /// Weitergehen: ein Favicon, das 404 gibt, ist in jedem Browser ein
    /// leeres Kachel-Feld. Der Nutzer soll wissen, dass die Seite eins
    /// verlangt hat.
    ///
    /// M3: `MAX_FAVICON_BYTES` (256 KiB) — ein Icon ist ein Icon.
    pub fn favicon_laden(&mut self) -> Option<String> {
        const MAX_FAVICON_BYTES: usize = 256 * 1024;
        let adresse = self
            .angezeigt
            .as_ref()
            .and_then(|a| a.seite.favicon.clone())?;
        match crate::bilder::bytes_holen(&adresse) {
            Ok(bytes) if bytes.len() <= MAX_FAVICON_BYTES => {
                match crate::bilder::dekodieren(&bytes) {
                    Ok(_) => {
                        self.meldung = Some(std::format!("Favicon: {adresse}"));
                        self.schmutzig = true;
                        Some(adresse)
                    }
                    Err(e) => {
                        self.meldung = Some(std::format!("Favicon {adresse}: {e}"));
                        self.schmutzig = true;
                        None
                    }
                }
            }
            Ok(_) => {
                self.meldung = Some(trf!("Favicon {}: too large (over 256 KiB)", adresse));
                self.schmutzig = true;
                None
            }
            Err(e) => {
                self.meldung = Some(std::format!("Favicon {adresse}: {e}"));
                self.schmutzig = true;
                None
            }
        }
    }

    // ------------------------------------------------------------------
    // W7C A7 — ZOOM
    // ------------------------------------------------------------------

    /// W7C A7: DEN ZOOM SETZEN (Faktor, z. B. 1.25 = 125 %).
    ///
    /// GEMESSEN, warum die Schriftgroesse und nicht die Seitenbreite:
    /// ein Browserzoom vergroessert die SCHRIFT, nicht das Fenster (bei
    /// Strg+Plus wird die Seite breiter, nicht der Text). Wer die Breite
    /// aendert, macht Resize — und das ist ein anderer Vorgang mit
    /// anderen Folgen fuer den Umbruch.
    ///
    /// GEMESSEN, warum `<html style="font-size: …px">` und nicht
    /// `transform: scale()`: eine Skalierung wuerde die Zeichengroesse
    /// nicht veraendern (alles bliebe gleich gross, nur verzerrt), und
    /// eine neue Seite mit `zoom` im CSS ist in keinem Browser unterstuetzt.
    ///
    /// Die Zusage: der ZOOM gehoert zum TAB-WECHSEL in der Zusage
    /// `angezeigt` = Seite des aktiven Tabs — und er wird deshalb wie die
    /// Seite behandelt (er steckt im Patch und ueberlebt den Wechsel).
    pub fn zoom_setzen(&mut self, faktor: f32) {
        // M3: die Grenzen VOR der Rechnung (ein `f32` aus dem Netz ist
        // eine Eingabe, auch wenn dieser Faktor nicht aus dem Netz kommt).
        //
        // KONTEXT (05.10.2026, 01:34): DIE KLEMME IST DIE **TABELLE**
        // (`ZOOM_MIN`/`ZOOM_MAX` = 25 %..500 %). GEMESSENER BEFUND: hier stand
        // `clamp(0.5, 3.0)`, und `prozent_aus_faktor` in derselben Kette klemmt
        // bei **25 %** — zwei Grenzen, und die engere gewinnt. Nach der Klemme
        // waere ein Zoom von 20 % (z. B. aus dem CSS) auf 50 % gesprungen,
        // obwohl die Zahl darunter says 25 % erlaubt.
        let f = if faktor.is_finite() {
            faktor.clamp(
                narsil_browser::interaktion::ZOOM_MIN as f32 / 100.0,
                narsil_browser::interaktion::ZOOM_MAX as f32 / 100.0,
            )
        } else {
            1.0
        };
        self.zoom = f;
        //
        // KONTEXT (04.10.2026, 14:50, Leitung „Zoom je Seite … **pro Host
        // gemerkt**“): DER ZOOM WIRD GEMERKT. GEMESSEN ist, dass es in
        // `narsil-browser` dafuer `merke_zoom_fuer_adresse` gibt (14:40, 22/22
        // gruen) — und GEMESSEN ist auch, warum es **hier** passiert: das ist
        // der **eine** Ort, an dem ein Faktor gesetzt wird (GEMESSEN:
        // `zoom_rein`, `zoom_raus`, `zoom_zurueck` gehen alle ueber
        // `zoom_setzen`). Ein Merken an einer zweiten Stelle waere eine zweite
        // Wahrheit ueber denselben Faktor.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die **Adresse** aus dem Browser kommt
        // und nicht aus dem Angezeigten: der Zoom gehoert dem **Host**, und der
        // Host steht im Tab (`Browser::adresse()`).
        //
        // GEMESSENER EIGENER FEHLER (der Compiler, E0502): `&self.browser
        // .adresse()` **und** `self.browser` mutabel in einem Ausdruck — das ist
        // die gleiche Form, die ich vorhin in `narsil-browser` bei `remove`
        // gefunden habe: erst in eine lokale Variable, dann benutzen.
        let adresse_jetzt = self.browser.adresse().to_string();
        let prozent = narsil_browser::Browser::prozent_aus_faktor(f);
        // Release 10.10.2026: der Standard-Zoom (Einstellungen) braucht
        // keinen Eintrag — eine Seite auf dem Standard folgt ihm, auch wenn
        // er sich spaeter aendert (Chrome).
        if prozent == self.einstellungen.zoom {
            self.browser.vergiss_zoom_fuer_adresse(&adresse_jetzt);
        } else {
            self.browser
                .merke_zoom_fuer_adresse(&adresse_jetzt, prozent);
        }
        //
        // KONTEXT (06.10.2026, 04:52): UND DANN IN DIE **DATEI**. GEMESSENE
        // ENTSCHEIDUNG, warum **pro Aenderung** und nicht beim Beenden: ein
        // Browser, der beim Beenden speichert, speichert bei einem Absturz
        // nichts. GEMESSENE ENTSCHEIDUNG, warum ein Fehler hier **kein**
        // Abbruch ist: ein fehlender Ordner (kein `$HOME`, keine Rechte) darf
        // den Browser nicht aufhalten — er merkt dann eben nur fuer diese
        // Sitzung, und die Zeile in der Statusleiste sagt es.
        if !self.zoomprofil_sichern() {
            self.meldung = Some(tr("Cannot write the zoom profile (only for this session)").into());
        }
        // 16 px ist `narsil_page::GRAD_TEXT` — und die Zahl steht hier noch
        // einmal. GEMESSEN, warum das ein Fehler ist: wenn der
        // Grundgrad sich aendert, stimmt 100 % hier nicht mehr. Wer das
        // pruefen will, muss es an EINER Stelle tun — siehe
        // `zoom_grad_aus`.
        self.zoom_grad = ((narsil_page::GRAD_TEXT as f32) * f).round() as u32;
        self.zoom_anwenden();
    }

    /// W7C A7: eine Stufe groesser (`Strg++`).
    ///
    // KONTEXT (05.10.2026, 01:32): DIE STUFE kommt aus der **Tabelle**
    // (`zoom_stufe`, gruene Crate, 4 Tests). GEMESSENER BEFUND, warum das ein
    // Gewinn ist: GEMESSEN war `zoom + 0.1` auf dem **Faktor** — also
    // 100 -> 110 -> 121 -> 133 -> 146 % ..., Stufen, die es in keinem Browser
    // gibt und die in der Leiste als „121 %“ erscheinen. Die Tabelle hat die
    // Stufen, die Firefox hat, und am Ende **nichts** (statt Rauschen).
    pub fn zoom_rein(&mut self) {
        let prozent = narsil_browser::Browser::prozent_aus_faktor(self.zoom);
        self.zoom_setzen(narsil_browser::Browser::faktor_aus_prozent(zoom_stufe(
            prozent,
            false,
        )));
    }

    /// W7C A7: eine Stufe kleiner (`Strg+-`).
    ///
    /// KONTEXT (05.10.2026, 01:33): dieselbe **Tabelle** wie in `zoom_rein` —
    /// zwei Stellen, die den Faktor rechnen, waeren zwei Stellen mit je einem
    /// anderen Zoom.
    pub fn zoom_raus(&mut self) {
        let prozent = narsil_browser::Browser::prozent_aus_faktor(self.zoom);
        self.zoom_setzen(narsil_browser::Browser::faktor_aus_prozent(zoom_stufe(
            prozent,
            true,
        )));
    }

    /// **SANFT** NACH `nach` SCROLLEN — der Versatz wandert in
    /// [`SCROLL_DAUER_MS`] von `von` nach `nach`.
    ///
    /// KONTEXT (04.10.2026, 18:20, Leitung „sanftes Scrollen (Animation 120 ms)“,
    /// Weg: „nimm die Fensteruhr/Weckzeit wie VIDEO sie in main.rs nutzt“):
    /// GEMESSEN ist der Weg: `main.rs:485-519` pollt mit **8 ms** Schlaf und holt
    /// sich von VIDEO eine **Weckzeit** (`schlaf_ms`). Es gibt also **kein**
    /// `narsil-window`-Problem — die Uhr ist der Takt der Schleife.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum drei Zahlen und kein „Zeiger“: der Versatz
    /// muss beim **Start** feststehen (sonst haengt die Bewegung davon ab, wann
    /// der Takt kommt), und der **Takt** ist eine Angabe, kein Zustand.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **hier** und nicht in `scroll_pixels`:
    /// GEMESSEN ist, dass `narsil/tests/browser.rs:228` (eine **fremde** Datei)
    /// das **sofortige** Rad-Scrollen festhält (1000 Klicks, dann `versatz ==
    /// max_v`). Mit einer Animation im Rad-Zweig wuerde dieser Test **rot**,
    /// weil er den Takt nicht aufruft. Darum: das Rad bleibt sofort (und der Test
    /// bleibt gruen), und **sanft** scrollen die Wege, die kein Test festhält —
    /// der Sprung auf der Schiene. Im BRETT (04.10., 18:30) als Frage: soll die
    /// Animation auch ins Rad, dann braucht es diese Testaenderung (UI-Datei).
    pub fn scroll_ziel_setzen(&mut self, nach: u32, jetzt: std::time::Instant) {
        self.scroll_rad = false;
        let sicht = self.sicht_hoehe();
        let ziel = nach.min(self.angezeigt.as_ref().map_or(0, |a| a.max_versatz(sicht)));
        let von = self.angezeigt.as_ref().map_or(0, |a| a.versatz);
        if von == ziel {
            return;
        }
        self.scroll_ani = Some(ScrollAni {
            von,
            nach: ziel,
            start: jetzt,
        });
        self.scrollbalken.zeigen(jetzt);
        self.schmutzig = true;
    }

    /// DIE TREFFER-RECHTECKE — aus **derselben** Funktion wie die Auswahl.
    ///
    /// KONTEXT (04.10.2026, 23:20, Leitung „Treffer mit derselben
    /// Textposition-Funktion wie die Auswahl“): GEMESSENER BEFUND vorher: die
    /// Rechtecke entstanden in `display_list` durch einen Lauf ueber die
    /// **Displayliste** mit `suche_versatz(text, font_size)` — und
    /// `suche_versatz` liefert **hoechstens eines je Zeile** (sein eigener
    /// Kommentar: „M3: hoechstens EIN Rechteck je Text“). Auf einer Zeile mit
    /// dem Muster **zweimal** war also nur **ein** Treffer sichtbar — bei einer
    /// Seite, die „Fehler Fehler Fehler“ schreibt, fand der Nutzer **einen**
    /// Fehler, obwohl **drei** dastehen. Das ist die Leitungspointe.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **zeichenweise** verglichen wird und nicht
    /// mit `to_lowercase().contains()`: GEMESSEN ist, dass `to_lowercase` die
    /// **Bytelänge** aendern kann (`ß` → zwei Zeichen) — ein Byte-Index in der
    /// kleingeschriebenen Zeichenkette ist dann **kein** Zeichenindex in der
    /// Originalzeile, und das Rechteck kaeme **ein Zeichen** daneben zu liegen.
    pub fn suche_rechtecke(&self) -> Vec<(Rect, bool)> {
        let Some(a) = self.angezeigt.as_ref() else {
            return Vec::new();
        };
        let Some(muster) = self.suche_merken.as_deref() else {
            return Vec::new();
        };
        let pat: Vec<char> = muster.to_lowercase().chars().collect();
        if pat.is_empty() {
            return Vec::new();
        }
        let stellen = text_stellen(a);
        let mut raus: Vec<(Rect, bool)> = Vec::new();
        // Release 09.10.2026: die Nummer zaehlt die Funde der GANZEN Seite (in
        // Dokumentreihenfolge, wie `suche_nummer`). GEMESSEN vorher: sie
        // begann je Zeile bei 0 — bei Treffer 1 war der erste Fund JEDER Zeile
        // „aktuell" (zwei starke Markierungen), nach F3 keiner.
        let mut nr = 0usize;
        for st in &stellen {
            if st.gesperrt {
                continue;
            }
            let klein: Vec<char> = st
                .text
                .chars()
                .map(|c| c.to_lowercase().next().unwrap_or(c))
                .collect();
            if klein.len() < pat.len() {
                continue;
            }
            for start in 0..=(klein.len() - pat.len()) {
                if klein[start..start + pat.len()] == pat[..] {
                    let aktuell = self.suche_nummer == nr;
                    nr += 1;
                    let vor: String = st.text.chars().take(start).collect();
                    let bis: String = st.text.chars().take(start + pat.len()).collect();
                    let x1 = st.origin.x + self.stelle_breite(st, &vor);
                    let x2 = st.origin.x + self.stelle_breite(st, &bis);
                    raus.push((
                        Rect::new(
                            x1,
                            st.origin.y,
                            (x2 - x1).max(1.0),
                            zeilen_hoehe(st.font_size),
                        ),
                        aktuell,
                    ));
                    if raus.len() >= 1000 {
                        return raus;
                    }
                }
            }
        }
        raus
    }

    /// DER **TAKT** — einmal je Schleifendurchgang (`main.rs`).
    ///
    /// `true`, wenn sich etwas geaendert hat (also gezeichnet werden muss) oder
    /// der Balken noch sichtbar ist. GEMESSENE ENTSCHEIDUNG, warum der Balken
    /// `true` liefert, obwohl sich **nichts bewegt**: sonst wuerde er nach dem
    /// Scrollen **nie** verschwinden — das Ausblenden ist selbst eine Aenderung.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **smoothstep** (`t*t*(3-2t)`) und nicht
    /// linear: linear beginnt und endet die Bewegung mit **Sprung** — das ist
    /// genau das „hüpfende“ Rollen, das die Leitung mit „sanft“ meint.
    pub fn tick(&mut self, jetzt: std::time::Instant) -> bool {
        // Release 09.10.2026: hat ein anderer Weg den Tab gewechselt, gehen
        // Panels und Menues zu — sie gehoeren zum alten Tab.
        let tab_stand = (self.browser.aktiver_tab_nr(), self.browser.tabs());
        match self.ueberlagerung_tab {
            Some(alt) if alt != tab_stand => self.ueberlagerungen_schliessen(),
            Some(_) => {}
            None => self.ueberlagerung_tab = Some(tab_stand),
        }
        // Release 09.10.2026: Animationen — Wechsel erkennen, Takt halten.
        self.bewegung_erkennen(jetzt);
        let animiert = self.bewegung.takt(jetzt);
        self.chrome.tab_bewegung = self.bewegung.tab.map(|(u, b)| {
            (
                match b {
                    crate::bewegung::TabBewegung::Auf(n) => narsil_chrome::TabWandel::Auf(n),
                    crate::bewegung::TabBewegung::Zu(n) => narsil_chrome::TabWandel::Zu(n),
                },
                u.wert(jetzt),
            )
        });
        if animiert {
            self.schmutzig = true;
        }
        // Release 09.10.2026: das Untermenue „Konvertiert herunterladen" folgt
        // der Maus mit Verzug (der Takt laeuft auch ohne Ereignis, ≤ 8 ms).
        if let Some(m) = self.menue.as_mut()
            && m.ktx.hover_tick(self.hoehe, jetzt)
        {
            self.schmutzig = true;
        }
        self.ladungen_abholen();
        // Release 11.10.2026: ein Bildlauf aus dem Skript (`__narsilRollen`):
        // Versatz setzen, sichtbare Lazy-Bilder laden, `scroll` am Dokument.
        let wunsch = self.angezeigt.as_ref().and_then(|a| {
            a.stand.lock().ok().and_then(|mut w| w.rollen_wunsch.take().map(|y| (y, w.document_id)))
        });
        if let Some((y, doc)) = wunsch {
            let sicht = self.sicht_hoehe();
            if let Some(a) = self.angezeigt.as_mut() {
                a.versatz = (y.max(0.0) as u32).min(a.max_versatz(sicht));
                // Release 13.10.2026: auch der Stand, den `scrollY` liest.
                // GEMESSEN vorher (WPT iframe-loading-lazy-*): ein Bildlauf im
                // `load`-Handler stand danach bei scrollY 0.
                if let Ok(mut w) = a.stand.lock() {
                    w.rollen_y = a.versatz as f32;
                }
            }
            self.bilder_sichtbar_laden();
            self.ereignis_ausloesen(doc, &[crate::dom::WINDOW_HOST_ID], "scroll", true, false, 0.0, 0.0);
            self.schmutzig = true;
        }
        // Release 11.10.2026: das aufgeschobene `window.load` (Ladesperre der
        // Seite, hoechstens `LADE_SPERRE_FRIST`).
        if let Some(a) = self.angezeigt.as_mut()
            && let Some(seit) = a.load_ausstehend
            && (seit.elapsed() >= LADE_SPERRE_FRIST || (Self::lade_sperre(a) == 0 && !Self::timer_vor_load(a)))
        {
            a.load_ausstehend = None;
            a.load_timer_grenze = None;
            if Self::load_feuern(a) {
                self.seite_neu_bauen();
            }
            self.schmutzig = true;
        }
        // Release 10.10.2026: ein fertiger Neubau aus dem Neubau-Faden.
        if self.neubau_abholen() {
            self.schmutzig = true;
        }
        let bilder_neu = self.bilder_abholen_und_starten();
        // Release 10.10.2026: `load`/`error` an Bildern, die jetzt da sind.
        let bilder_neu = self.bild_ereignisse_takt() | bilder_neu;
        let bilder_neu = self.blaetter_nachladen_takt() | bilder_neu;
        // Release 08.10.2026: der Takt der Features (Schnappschuss, Waechter,
        // Einfrieren, Fristen/Belege, Hinweise, Link-Vorschau).
        let bilder_neu = self.features_tick(jetzt) | bilder_neu;
        // Release 08.10.2026: der Stand der Umwandlungen im Hintergrund.
        let bilder_neu = self.konvert_abholen() | bilder_neu;
        // Release 08.10.2026: die Bild-Diaet (Anzeigegroesse, Sichtfenster).
        let bilder_neu = self.bild_diaet_takt(jetzt) | bilder_neu;

        //
        // UMBau-ENTPRELLUNG: die aufgeschobene Layout-Neuberechnung nach
        // einer Groessenaenderung (siehe `umbau_seit` und `UMBAU_RUHE`).
        //
        // GEMESSENE ENTSCHEIDUNG, warum sie VOR dem Rest steht: sie
        // aktualisiert das Layout vor weiteren Ereignissen. Ein Reload
        // wuerde DOM-Aenderungen, Formulare und die Skriptengine verlieren.
        if let Some(seit) = self.umbau_seit
            && jetzt.duration_since(seit) >= UMBAU_RUHE
        {
            self.umbau_seit = None;
            self.seite_neu_bauen();
            self.ereignis_ausloesen(crate::dom::WINDOW_HOST_ID, &[], "resize", false, false, 0.0, 0.0);
            self.schmutzig = true;
            return true;
        }
        let mut bewegt = bilder_neu;
        //
        // KONTEXT (06.10.2026, 03:30): DER **AUTO-LAUF** wandert hier, weil dies
        // der **Takt** ist (8 ms, `tick_warte_ms`). GEMESSENE ENTSCHEIDUNG,
        // warum die Rechnung in der **gruenen** Crate liegt: die Beschleunigung
        // und der Rest sind Zahlen, und Zahlen, die man nicht pruefen kann,
        // stimmen irgendwann nicht mehr — der Test
        // `der_lauf_ist_von_der_bildrate_unabhaengig` ist der Beweis.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `dt` **gemessen** und nicht angenommen
        // wird: der Takt ist 8 ms, aber nicht **immer** (Kopieren, Laden). Ein
        // angenommener Takt hiesse: langsamer Rechner = langsamer Lauf.
        if let Some((kx, ky, start)) = self.auto_lauf {
            let jetzt_ns = crate::video::jetzt_ns();
            let dt_ms = (jetzt_ns.saturating_sub(self.auto_letzte_ns) / 1_000_000).min(100);
            self.auto_letzte_ns = jetzt_ns;
            let seit_ms = (jetzt_ns.saturating_sub(start)) / 1_000_000;
            let dx = self.maus.x - kx as f32;
            let dy = self.maus.y - ky as f32;
            if let Some((_rx, ry)) = auto_lauf_richtung(dx, dy) {
                let px = self.auto_rest.nimm(auto_lauf_schritt_milli(seit_ms, dt_ms));
                let dy_px = ry as i64 * px as i64;
                if dy_px != 0 {
                    self.scroll_pixels(dy_px);
                    bewegt = true;
                }
                //
                // GEMESSENE ENTSCHEIDUNG, warum die **waagerechte** Achse hier
                // keine ist: GEMESSEN ist, dass es kein waagerechtes Scrollen
                // gibt (`ScrollDelta` in `narsil-window/src/event.rs`: „die
                // horizontale Achse faellt derzeit auf dieselbe Skala; STUFE 2
                // nutzt vertikal, eine eigene Achse ist Folgearbeit“) — ein
                // Aufruf von etwas, das es nicht gibt, waere ein Fehler, und ein
                // stillschweigendes Ignorieren (`let _ = rx;`) waere eine
                // Zusage, die nicht stimmt. Also: senkrecht scrollen, und die
                // waagerechte Achte ist **benannt** (im BRETT an UI/CSS).
            }
        }
        //
        // KONTEXT (05.10.2026, 06:30): DIE BEWEGUNG RECHNET **`ani`** — nicht
        // mehr diese Funktion.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das nicht nur „woanders ist ja gleich“:
        // der erste **gelaufene** Test dieser Logik (`auch_nach_oben_geht_es_
        // sanft`, `narsil-browser/tests/interaktion.rs`) fand hier einen echten
        // Fehler: `von + ((nach - von) * e) as u32` **schneidet negative
        // Fliesskommazahlen auf 0 ab** (seit Rust 1.45 saettigend) — beim
        // Scrollen **nach oben** blieb der Versatz also stehen. Dieselbe
        // Rechnung steht jetzt an **einer** Stelle (`ScrollAni::versatz_bei`)
        // und hat **drei** Tests daneben.
        if let Some(ani) = self.scroll_ani {
            let stand = if self.scroll_rad { ani.rad_versatz_bei(jetzt) } else { ani.versatz_bei(jetzt) };
            let sicht = self.sicht_hoehe();
            if let Some(a) = self.angezeigt.as_mut() {
                let neu = stand.min(a.max_versatz(sicht));
                if a.versatz != neu {
                    a.versatz = neu;
                    self.bilder_nachladen = true;
                    bewegt = true;
                }
            }
            if ani.fertig(jetzt) { self.scroll_ani = None; }
        }
        let balken_sichtbar = self.scrollbalken.sichtbar(jetzt);
        if balken_sichtbar != self.scrollbalken_war_sichtbar {
            self.scrollbalken_war_sichtbar = balken_sichtbar;
            bewegt = true;
        }
        // T1 (06.10.2026): TIMER UND RAF gehoeren in den Takt.
        if self.timer_abarbeiten() {
            bewegt = true;
        }
        self.datei_dialog_wunsch_abholen();
        if self.js_navigation_abholen() {
            bewegt = true;
        }
        bewegt
    }

    /// Release 14.10.2026: a navigation a script asked for (`location.href =`,
    /// `location.assign/replace/reload`, `history.back()`, a form submitted by
    /// script) — the main thread navigates now (HTML §7.4.2.2 „navigate").
    /// MEASURED before: `Kiste::navigieren_wunsch` had no reader at all, so
    /// no script could ever leave the page (redirects after login, search
    /// boxes that set `location`, `window.location = url`).
    fn js_navigation_abholen(&mut self) -> bool {
        let wunsch = self
            .angezeigt
            .as_ref()
            .and_then(|a| a.kiste.lock().ok().and_then(|mut k| k.navigieren_wunsch.take()));
        let Some(ziel) = wunsch else { return false };
        if let Err(e) = self.gehe_zu(&ziel) {
            self.meldung = Some(std::format!("Navigation: {e}"));
        }
        self.schmutzig = true;
        true
    }

    /// T1 (06.10.2026): die faelligen Timer/RAF der Seite ausfuehren.
    ///
    /// # WARUM DAS DER FEHLENDE TEIL WAR
    ///
    /// GEMESSEN (Masterplan D): die Motor-API (`timer_frueheste_ms`,
    /// `timer_einen_ausfuehren`) hatte im Fensterloop **keine Aufrufstelle**
    /// — `setTimeout`, `setInterval` und `requestAnimationFrame` waren
    /// Namen ohne Wirkung. Hier werden sie Teil des Takts: Uhr auf Wanduhr
    /// ziehen, faellige Timer ausfuehren, die erzeugten DOM-Befehle in den
    /// Patch spielen und neu bauen.
    ///
    /// DIE GEDECKELTE ZAHL IST ABSICHT: ein `setInterval(fn, 0)` wuerde
    /// sonst den Takt nie verlassen. 64 Timer je Takt sind viel Arbeit und
    /// immer noch endlich; der Rest bleibt fuer den naechsten Takt stehen.
    fn timer_abarbeiten(&mut self) -> bool {
        const MAX_TIMER_JE_TAKT: usize = 64;
        // GEMESSEN (06.10.2026): eine Seite mit `setInterval(fn, 0)` liess
        // den Takt nicht mehr zur Ruhe kommen — 64 Timer je Takt, jeder mit
        // JS-Arbeit, und die Oberflaeche wirkte „haengend". Die Zahl allein
        // ist keine Grenze: hier begrenzt auch die ZEIT das Stueck, das EIN
        // Takt fuer Timer ausgibt. Der Rest wartet auf den naechsten Takt,
        // und die Maus wird dazwischen bedient.
        //
        // Release 09.10.2026: 12 statt 4 ms — der teure Teil (der
        // Seitenneubau) ist jetzt gedrosselt (`JS_NEUBAU_ABSTAND`); App-Seiten
        // wie youtube.com brauchen die Zeit fuer ihren Planer.
        const MAX_TIMER_MS: u128 = 12;
        let takt_start = std::time::Instant::now();
        let (gelaufen, console_neu, befehle) = {
            let Some(a) = self.angezeigt.as_mut() else {
                return false;
            };
            let Some(mut engine) = a.engine.take() else {
                return false;
            };
            engine.uhr_auf_wanduhr();
            let vorher = engine.konsole().len();
            let mut gelaufen = 0usize;
            let mut fehler: Vec<String> = Vec::new();
            // Messages from workers, ports and channels (`worker.rs`): a
            // batch of tasks per pump call (within the budget of this tick),
            // interleaved with the timers.
            let slot = a.kiste.lock().ok().map(|k| k.worker.clone());
            while gelaufen < MAX_TIMER_JE_TAKT && takt_start.elapsed().as_millis() < MAX_TIMER_MS {
                if let Some(s) = slot.as_ref()
                    && s.pending()
                {
                    crate::dom::vorgaenge_zuruecksetzen();
                    engine.browser_task_beginnen();
                    let rest_ms = MAX_TIMER_MS.saturating_sub(takt_start.elapsed().as_millis()).max(1) as u64;
                    let r = crate::worker::page_pump(&mut engine, s, rest_ms, MAX_TIMER_JE_TAKT - gelaufen);
                    engine.browser_task_beenden();
                    match r {
                        Ok(n) => gelaufen += n,
                        Err(e) => {
                            fehler.push(format!("Message: {e}"));
                            gelaufen += 1;
                        }
                    }
                }
                let faellig = engine
                    .timer_frueheste_ms()
                    .is_some_and(|t| t <= engine.wanduhr_ms());
                if !faellig {
                    if slot.as_ref().is_some_and(|s| s.pending()) {
                        continue;
                    }
                    break;
                }
                crate::dom::vorgaenge_zuruecksetzen();
                engine.browser_task_beginnen();
                let timer_result = engine.timer_einen_ausfuehren();
                engine.browser_task_beenden();
                match timer_result {
                    Ok(true) => gelaufen += 1,
                    Ok(false) => break,
                    // Ein Ausnahmefehler im Timer beendet nicht den
                    // Browser — er wird sichtbar gemacht wie ein
                    // Handler-Fehler.
                    Err(e) => {
                        fehler.push(format!("Timer: {e}"));
                        break;
                    }
                }
            }
            if takt_zeiten_an() && gelaufen > 0 {
                eprintln!("TAKT timer {:.1} ms n={gelaufen}", takt_start.elapsed().as_secs_f64() * 1000.0);
            }
            let mut console_neu: Vec<String> =
                engine.konsole().iter().skip(vorher).cloned().collect();
            console_neu.extend(fehler);
            // A4/E (06.10.2026): fetch im Timer. Der Netz-Pump gehoert
            // hinter die Timer — eine `.then`-Kette kann in beiden stecken.
            if let Err(e) = engine.netz_schleife() {
                console_neu.push(format!("Timer/fetch: {e}"));
            }
            // E: ein dynamischer `import('…')` aus einem Timer.
            console_neu.extend(crate::js::module_pumpen(&mut engine));
            // Release 10.10.2026: eingefuegte `<script>` aus einem Timer.
            {
                let (k_dyn, f_dyn) = crate::js::dyn_skripte_pumpen(&mut engine, &a.kiste, &a.stand);
                console_neu.extend(k_dyn);
                console_neu.extend(f_dyn);
            }
            // Release 10.10.2026 (Diagnose): `NARSIL_KONSOLE_SPUR=1` zeigt die
            // Konsole der Timer (React-Scheduler, Hydrationsfehler) auf stderr.
            if std::env::var_os("NARSIL_KONSOLE_SPUR").is_some() {
                for z in &console_neu {
                    eprintln!("KONSOLE-TIMER {}", z.chars().take(2000).collect::<String>());
                }
            }
            let mut befehle: Vec<crate::js::Befehl> = {
                let mut st = a.stand.lock().expect("DOM");
                // Host-Meldungen (`alert`) aus einem Timer.
                console_neu.append(&mut st.meldungen);
                st.nimm_befehle()
            };
            befehle.extend(a.kiste.lock().expect("Kiste").nimm_befehle());
            crate::js::handler_wurzeln_erneuern(&mut engine, &a.stand, &a.kiste);
            a.engine = Some(engine);
            (gelaufen, console_neu, befehle)
        };
        if gelaufen == 0 && console_neu.is_empty() && befehle.is_empty() {
            // Release 09.10.2026: JS ruht — ein aufgeschobener Neubau kommt
            // jetzt sofort (siehe unten).
            if self.js_neubau_offen {
                self.js_neubau_offen = false;
                self.letzter_js_neubau = Some(std::time::Instant::now());
                self.seite_neu_bauen();
                return true;
            }
            return false;
        }
        let Some(a) = self.angezeigt.as_mut() else {
            return false;
        };
        let result = crate::ereignis_host::DispatchResult {
            invoked: true,
            errors: Vec::new(),
            console: console_neu,
            commands: befehle,
        };
        let mut geaendert = Self::ereignis_anwenden(a, result);
        // Release 15.10.2026 (Agent C): an animation frame that changed only
        // paint-only values is painted into the shown page, without a
        // rebuild (`anim_schnell`); otherwise as before.
        let nur_gemalt = match crate::anim_schnell::after_scripts(a, geaendert) {
            crate::anim_schnell::Outcome::Painted => true,
            crate::anim_schnell::Outcome::Rebuild => {
                geaendert = true;
                false
            }
            crate::anim_schnell::Outcome::Nothing => false,
        };
        // Release 09.10.2026: der Neubau (HTML aus der Arena, Parsen,
        // Kaskade, Layout) laeuft waehrend JS-Arbeit hoechstens alle
        // `JS_NEUBAU_ABSTAND` — und sofort, sobald JS ruht.
        //
        // GEMESSEN, warum: jeder Takt mit einer DOM-Aenderung baute die
        // ganze Seite neu. youtube.com rendert seine App in vielen kleinen
        // Planer-Haeppchen; mit 4 ms JS und danach einem Neubau von einigen
        // hundert Millisekunden (3,3-MB-Blatt) je Takt kam die Startseite nie
        // ueber das Skelett hinaus (800 Takte: 243 s).
        if geaendert || self.js_neubau_offen {
            let jetzt = std::time::Instant::now();
            let faellig = self
                .letzter_js_neubau
                .is_none_or(|t| jetzt.duration_since(t) >= JS_NEUBAU_ABSTAND);
            if faellig {
                self.js_neubau_offen = false;
                self.letzter_js_neubau = Some(jetzt);
                self.seite_neu_bauen();
            } else {
                self.js_neubau_offen = true;
            }
        }
        geaendert || nur_gemalt
    }

    /// WIE LANGE DER TAKT NOCH WARTEN MUSS (ms) — `None`, wenn nichts laeuft.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das neben `tick` steht und nicht darin:
    /// `tick` **tut**, das hier **sagt** — und die Schleife braucht die Zahl zum
    /// Schlafen (dieselbe Form wie bei VIDEO, `main.rs:494`).
    pub fn tick_warte_ms(&self, jetzt: std::time::Instant) -> Option<u64> {
        let mut warte: Option<u64> = None;
        // Release 09.10.2026: solange eine Animation laeuft, im Takt der
        // Bildrate; sonst wartet die Schleife auf Ereignisse.
        if self.bewegung.laeuft(jetzt) {
            warte = Some(self.bild_ms.max(1));
        }
        if let Some(ani) = self.scroll_ani {
            let dauer = std::time::Duration::from_millis(SCROLL_DAUER_MS);
            let uebrig = dauer.saturating_sub(jetzt.saturating_duration_since(ani.start));
            warte = Some((uebrig.as_millis() as u64).min(self.bild_ms));
        }
        // Release 12.10.2026: der Nachlade-Wunsch zaehlt nur, wenn die Seite
        // Bilder hat. GEMESSEN vorher (ktx_scrollen „die sanfte Bewegung"):
        // das Ende einer Bewegung setzte den Wunsch, und eine Seite OHNE
        // Bilder meldete danach noch 16 ms Weckzeit.
        let nachladen = self.bilder_nachladen
            && self.angezeigt.as_ref().is_some_and(|a| !a.seite.bilder.is_empty() || !a.stand.lock().is_ok_and(|st| st.lose_bilder.is_empty()));
        if nachladen || self.bild_lader.as_ref().is_some_and(|l| l.laeuft) {
            warte = Some(warte.map_or(16, |w| w.min(16)));
        }
        // Release 08.10.2026: solange umgewandelt wird, viermal je Sekunde
        // nachsehen (Statuszeile mit Sekunden, Hinweis bei Ende).
        if !self.konvert_laeufe.is_empty() {
            warte = Some(warte.map_or(250, |w| w.min(250)));
        }
        // Release 08.10.2026 (Bild-Diaet): nachsehen, solange Auftraege laufen
        // oder sich die Lage (Seite, Versatz, Fenster, Bilder) seit dem
        // letzten Takt geaendert hat.
        if let Some(a) = self.angezeigt.as_ref()
            && !a.seite.bilder.is_empty()
        {
            let abdruck = (
                std::sync::Arc::as_ptr(&a.html) as usize,
                a.versatz,
                self.breite,
                self.hoehe,
                a.bilder.len(),
            );
            if !self.bild_diaet.offen.is_empty() || self.bild_diaet.arbeit || abdruck != self.bild_diaet.abdruck {
                let w = crate::bild_diaet::TAKT_MS;
                warte = Some(warte.map_or(w, |x| x.min(w)));
            }
        }
        if let Some(bis) = self.scrollbalken.sicht_bis.filter(|_| self.scrollbalken_war_sichtbar) {
            let rest = bis.saturating_duration_since(jetzt).as_millis() as u64;
            warte = Some(warte.map_or(rest, |w| w.min(rest)));
        }
        // T1 (06.10.2026): den Schlaf an der naechsten Timer-Weckzeit
        // deckeln. GEMESSEN: ohne diese Zeile wartete der Loop bis zu 8 ms
        // (sein Standardschlaf), obwohl `setTimeout(fn, 1)` schon faellig
        // war — sichtbar als ungenaue Timer auf jeder Seite.
        if let Some(a) = self.angezeigt.as_ref()
            && let Some(engine) = a.engine.as_ref()
            && let Some(ziel) = engine.timer_frueheste_ms()
        {
            // GEMESSEN: ein faelliger 0-ms-Timer darf den Takt nicht auf 1 ms
            // ziehen — das ist eine Schleife mit voller Last. Browser klemmen
            // verschachtelte Timer auf 4 ms; dieselbe Zahl steht hier.
            let rest = ziel.saturating_sub(engine.wanduhr_ms()).max(4);
            warte = Some(warte.map_or(rest, |w| w.min(rest)));
        }
        // A message from a worker or port waits for the page: soon.
        if let Some(a) = self.angezeigt.as_ref()
            && a.kiste.lock().is_ok_and(|k| k.worker.pending())
        {
            warte = Some(warte.map_or(1, |w| w.min(1)));
        }
        // Release 09.10.2026: ein aufgeschobener Neubau (siehe
        // `timer_abarbeiten`) braucht spaetestens dann einen Takt.
        if self.js_neubau_offen {
            let rest = self.letzter_js_neubau.map_or(0, |t| {
                JS_NEUBAU_ABSTAND.saturating_sub(jetzt.saturating_duration_since(t)).as_millis() as u64
            });
            warte = Some(warte.map_or(rest, |w| w.min(rest)));
        }
        warte
    }

    /// DER PUNKT ALS **TEXTSTELLE** — (Zeilenindex, Zeichenindex).
    ///
    /// KONTEXT (04.10.2026, 21:30): GEMESSENE ENTSCHEIDUNG, warum **alle**
    /// Zeichen der Zeile durchgemessen werden und nicht ein **Durchschlag**
    /// gesucht wird: GEMESSEN ist, dass `Messung::breite` fuer ein Praefix
    /// keine Tabelle liefert, sondern gerechnet wird — ein Durchschlag waere
    /// also `log n` Mal **dieselbe** Rechnung mit einem Puffer daneben. Bei
    /// einer Zeile von 80 Zeichen ist das eine Zeile Code statt drei.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **nächste** Zeichen gewinnt und nicht
    /// das vorherige: wer auf die **Hälfte** eines Buchstabens klickt, meint den
    /// **rechten** — mit dem vorherigen kaeme links von der Mitte immer das
    /// falsche Zeichen.
    pub fn stelle_unter(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        let a = self.angezeigt.as_ref()?;
        let stellen = text_stellen(a);
        for (i, st) in stellen.iter().enumerate() {
            if st.gesperrt {
                // GEMESSENE ENTSCHEIDUNG: `user-select:none` heisst „hier
                // nicht“ — **kein** Treffer, nicht nur kein Ziehen. Sonst
                // koennte eine Auswahl trotzdem ueber einen gesperrten Text
                // laufen.
                continue;
            }
            let h = zeilen_hoehe(st.font_size);
            if y < st.origin.y || y >= st.origin.y + h {
                continue;
            }
            let mut best = (0usize, f32::MAX);
            for k in 0..=st.text.chars().count() {
                let vor: String = st.text.chars().take(k).collect();
                let grenze = st.origin.x + self.stelle_breite(st, &vor);
                let d = (x - grenze).abs();
                if d < best.1 {
                    best = (k, d);
                }
            }
            return Some((i, best.0));
        }
        None
    }

    /// DIE AUSWAHL ALS **RECHTECKE** — zum Zeichnen der Hervorhebung.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum je **Zeile** ein Rechteck entsteht (und
    /// nicht eines ueber die ganze Auswahl): GEMESSEN ist, dass eine `TextRun`
    /// **eine Zeile** ist — ein Rechteck ueber drei Zeilen waere ein Streifen
    /// ueber dem Zwischenraum, und genau das sieht nach einem Fehler aus.
    pub fn auswahl_rects(&self) -> Vec<Rect> {
        let Some(a) = self.angezeigt.as_ref() else {
            return Vec::new();
        };
        let stellen = text_stellen(a);
        let Some(((za, ka), (ze, ke))) = self.auswahl.geordnet() else {
            return Vec::new();
        };
        if ze >= stellen.len() {
            return Vec::new();
        }
        let mut raus: Vec<Rect> = Vec::new();
        for i in za..=ze {
            let st = &stellen[i];
            let von = if i == za { ka } else { 0 };
            let bis = if i == ze { ke } else { st.text.chars().count() };
            if bis <= von {
                continue;
            }
            let vor: String = st.text.chars().take(von).collect();
            let bis_text: String = st.text.chars().take(bis).collect();
            let x1 = st.origin.x + self.stelle_breite(st, &vor);
            let x2 = st.origin.x + self.stelle_breite(st, &bis_text);
            raus.push(Rect::new(
                x1,
                st.origin.y,
                (x2 - x1).max(1.0),
                zeilen_hoehe(st.font_size),
            ));
        }
        raus
    }

    /// DER AUSGEWAEHLTE TEXT — und **der Cache** wird dabei gefuellt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Zeilen mit `\n` verbunden werden und
    /// nicht ohne Trenner: GEMESSEN ist, dass eine `TextRun` eine **Zeile** ist
    /// — ohne Trenner waere „Absatz einsAbsatz zwei“ ein Wort.
    pub fn auswahl_text_holen(&mut self) -> String {
        let mut raus = String::new();
        let Some(((za, ka), (ze, ke))) = self.auswahl.geordnet() else {
            self.auswahl_text.clear();
            return raus;
        };
        if let Some(a) = self.angezeigt.as_ref() {
            let stellen = text_stellen(a);
            if ze < stellen.len() {
                for i in za..=ze {
                    let st = &stellen[i];
                    let von = if i == za { ka } else { 0 };
                    let bis = if i == ze { ke } else { st.text.chars().count() };
                    let teil: String = st
                        .text
                        .chars()
                        .skip(von)
                        .take(bis.saturating_sub(von))
                        .collect();
                    if !teil.is_empty() {
                        if !raus.is_empty() {
                            raus.push('\n');
                        }
                        raus.push_str(&teil);
                    }
                }
            }
        }
        self.auswahl_text = raus.clone();
        raus
    }

    /// Release 09.10.2026: die Breite eines Textstuecks einer Seitenzeile —
    /// mit der Schrift der Zeile (Familie, Gewicht, Lage).
    pub fn stelle_breite(&self, st: &TextStelle, text: &str) -> f32 {
        self.messung.breite_stil_familie(text, st.font_size, st.gewicht, st.kursiv, &st.familie)
    }

    /// DIE **BREITE** eines Textpraefixes bei einer Schriftgroesse — die eine
    /// Rechnung, die Auswahl, Suche und Cache gemeinsam brauchen.
    ///
    /// KONTEXT (04.10.2026, 22:45, Leitung „Textposition je Zeichen aus
    /// Messung::breite je Praefix“): GEMESSENE ENTSCHEIDUNG, warum das ein
    /// **oeffentlicher** Weg ist und nicht `self.messung.breite(...)` an drei
    /// Stellen: GEMESSEN ist, dass `messung` ein **Feld** ist, und die
    /// Textbreite wird jetzt an drei Orten gebraucht (Treffer, Rechtecke, Cache)
    /// — dazu die Tests, die **ohne** Fenster dieselbe Zahl brauchen.
    pub fn messung_breite(&self, text: &str, grad: f32) -> f32 {
        // Release 09.10.2026: mit der Familie, in der die Leisten ZEICHNEN
        // (`LEISTEN_SCHRIFT` = sans-serif, unter Windows Segoe UI). GEMESSEN
        // vorher: gemessen wurde mit der Hauptschrift (Arial) — bei
        // „narsil://bild-diaet" 94,7 px Messung gegen 98 px Tinte, der Cursor
        // der Adressleiste stand IM letzten Buchstaben, Auswahl und Klickstelle
        // lagen daneben.
        self.messung.breite_stil_familie(text, grad, 400, false, narsil_display::LEISTEN_SCHRIFT)
    }

    /// KONTEXT (04.10.2026, 14:55): DEN GEMERKTEN ZOOM DIESES HOSTS
    /// ANWENDEN — nach dem Laden einer Seite.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **ein** Aufruf ist und nicht in
    /// `zoom_setzen` steckt: `zoom_setzen` merkt, und beim Laden soll der
    /// **gemerkte** Wert **angewendet** werden, ohne ihn neu zu schreiben —
    /// sonst haette jeder Ladevorgang den Wert bestaetigt und die Karte waere
    /// eine Liste von Ladevorgaengen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Wert **nur bei `Some`** angewendet
    /// wird: ohne Eintrag heisst „100 %“ (Firefox), und `zoom_setzen(1.0)` an
    /// jeder frischen Seite wuerde die Seite **ohne Grund** neu bauen — bei
    /// jeder Seite ein zweiter Layoutlauf.
    ///
    /// Release 10.10.2026: ohne Eintrag gilt der STANDARD-Zoom der
    /// Einstellungen (vorher fest 100 % — die Einstellung wirkte nur auf den
    /// Tab, in dem sie geaendert wurde, und legte fuer die Einstellungsseite
    /// einen eigenen Eintrag an). Angewendet wird ohne Merken.
    pub fn zoom_vom_host_holen(&mut self) -> bool {
        let adresse = self.browser.adresse().to_string();
        let prozent = self
            .browser
            .zoom_fuer_adresse(&adresse)
            .unwrap_or(self.einstellungen.zoom);
        let f = narsil_browser::Browser::faktor_aus_prozent(prozent);
        let traegt_zoom = self
            .angezeigt
            .as_ref()
            .is_some_and(|a| a.patch.attribut.contains_key(&(0, "style".to_string())));
        if prozent == 100 && !traegt_zoom {
            // Die Seite steht schon bei 100 % — kein zweiter Layoutlauf.
            self.zoom = 1.0;
            self.zoom_grad = narsil_page::GRAD_TEXT as u32;
            return false;
        }
        let meldung = self.meldung.take();
        self.zoom = f;
        self.zoom_grad = ((narsil_page::GRAD_TEXT as f32) * f).round() as u32;
        self.zoom_anwenden();
        // Beim Laden keine Meldung „Zoom: …" — der Nutzer hat nichts getan.
        self.meldung = meldung;
        true
    }

    /// W7C A7: zurueck auf den Standard (`Strg+0`) — Release 10.10.2026: der
    /// Standard-Zoom der Einstellungen, nicht fest 100 %.
    pub fn zoom_zurueck(&mut self) {
        self.zoom_setzen(self.einstellungen.zoom_faktor());
    }

    /// W7C A7: DIE ZOOM-ANGABE IN DIE SEITE SCHREIBEN und sie neu bauen.
    fn zoom_anwenden(&mut self) {
        // Die Angabe gehoert an die WURZEL (`<html>`, Vorlauf-Nummer 0 —
        // gemessen: `dom_zu_ord` ist `{1: 0, …}`, Knoten 1 ist der
        // html-Container). Von dort erben alle, die keine eigene
        // `font-size` haben — so, wie es ein Browser macht.
        let grad = self.zoom_grad;
        if let Some(a) = self.angezeigt.as_mut() {
            if grad == narsil_page::GRAD_TEXT as u32 {
                // 100 %: die Angabe wieder WEG. GEMESSEN, warum nicht
                // `font-size: 16px` stehen lassen: dann waere eine Seite
                // mit eigener `font-size` an `html` (selten, aber
                // moeglich) nicht mehr erkennbar — und der Patch waere
                // nie wieder leer.
                a.patch.attribut.remove(&(0, "style".to_string()));
            } else {
                a.patch
                    .attribut
                    .insert((0, "style".to_string()), std::format!("font-size:{grad}px"));
            }
        }
        self.seite_neu_bauen();
        self.meldung = Some(std::format!(
            "Zoom: {} %",
            (self.zoom * 100.0).round() as i32
        ));
        self.schmutzig = true;
    }

    // ------------------------------------------------------------------
    // W7C A5 — TABS
    // ------------------------------------------------------------------

    /// W7C A5: DIE SEITE DES AKTIVEN TABS IN DEN TAB LEGEN.
    ///
    /// GEMESSEN, warum das an JEDEM Navigationsende passiert und nicht
    /// nur beim Laden: ein Klick auf einen Link, ein Formular, ein
    /// zurueck — alles aendert die Seite, und alles muss im richtigen Tab
    /// landen. Ein Tab, der die Seite des Nachbarn zeigt, ist schlimmer
    /// als kein Tab.
    pub fn tabs_anzeigen(&mut self) {
        // GEMESSEN, warum der aktive Tab den Titel aus `angezeigt` nimmt
        // und die anderen aus ihrer gespeicherten Seite: der aktive Tab
        // IST die angezeigte Seite, und der Weg ueber den Verlauf waere
        // ein zweiter. Ein leerer Tab bekommt seinen Platzhalter von der
        // Chrome — damit die Leiste die einzige Stelle ist, die ueber
        // „wie heisst ein leerer Tab" entscheidet.
        let aktiv = self.browser.aktiver_tab_nr();
        let tabs: Vec<(String, bool)> = (0..self.browser.tabs())
            .map(|nr| {
                let titel = if crate::einstellungen_weg::ist_einstellungen(self.browser.adresse_von_tab(nr)) {
                    tr("Settings").to_string()
                } else if nr == aktiv {
                    self.angezeigt
                        .as_ref()
                        .map(|a| a.seite.titel.clone())
                        .unwrap_or_default()
                } else {
                    self.tabs
                        .get(nr)
                        .and_then(|t| t.as_ref())
                        .map(|a| a.seite.titel.clone())
                        .unwrap_or_default()
                };
                (titel, nr == aktiv)
            })
            .collect();
        let adressen: Vec<String> = (0..self.browser.tabs())
            .map(|nr| self.browser.adresse_von_tab(nr).to_string())
            .collect();
        self.chrome.setze_tabs_mit_adressen(&tabs, &adressen);
        self.chrome.lesezeichen_aktiv = self.lesezeichen.hat(self.browser.adresse());
    }

    /// Die Seite des aktiven Tabs in den Tab legen.
    ///
    /// W7C A5: `angezeigt` IST die Seite des aktiven Tabs (siehe die
    /// Zusage am Feld `tabs`).
    fn tab_speichern(&mut self) {
        let laden_offen = self.laedt;
        if let Some(lader) = self.seiten_lader.as_mut() { lader.verwerfen(); }
        self.laedt = false;
        let nr = self.browser.aktiver_tab_nr();
        // M3: `MAX_TABS` (32) — mehr Tabs gibt es nicht, und der Vektor
        // waechst nicht ueber die Grenze hinaus.
        while self.tabs.len() <= nr {
            self.tabs.push(None);
        }
        if nr < self.tabs.len() {
            self.tabs[nr] = if laden_offen { self.angezeigt.take(); None } else { self.angezeigt.take() };
        }
    }

    /// W7C A5: DIE SEITE DES AKTIVEN TABS HERAUSNEHMEN (und anzeigen).
    /// Release 09.10.2026: ALLE UEBERLAGERUNGEN ZU — beim Tabwechsel (neu,
    /// wechseln, schliessen). GEMESSEN vorher (Nutzer): das Download-Panel
    /// blieb offen und lag ueber der Startseite des anderen Tabs; dasselbe
    /// galt fuer Kontext-, Tab- und Leistenmenue, die Suchleiste, das
    /// Klappfeld der Einstellungen, den Dateidialog und die Link-Vorschau —
    /// alle gehoeren zu dem Tab, in dem sie geoeffnet wurden.
    /// Release 09.10.2026 (Animation): haengt ein Menue an — waehrend es
    /// aufgeht um `(x, y)` von 0,96 auf 1 skaliert und eingeblendet.
    fn aufgehend(liste: &mut DisplayList, menue: &DisplayList, u: Option<crate::bewegung::Uebergang>, x: f32, y: f32) {
        match u.map(|u| u.wert(std::time::Instant::now())) {
            Some(w) if w < 1.0 => {
                let (deckkraft, skala) = crate::bewegung::menue_form(w);
                let bewegt = menue.transformiert(Point::new(x, y), skala, 0.0, 0.0);
                let bereich = bewegt
                    .bereich_mit_effekten()
                    .or_else(|| menue.bereich_mit_effekten())
                    .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0));
                liste.anhaengen_mit_deckkraft(&bewegt, deckkraft, bereich);
            }
            _ => liste.anhaengen(menue),
        }
    }

    /// Release 09.10.2026 (Animation): vergleicht den Stand der Oberflaeche
    /// mit dem letzten Takt und startet, was dazu gehoert — Aufgehen,
    /// Ausblenden beim Schliessen, Ueberblenden bei Hover und Tabwechsel.
    fn bewegung_erkennen(&mut self, jetzt: std::time::Instant) {
        use crate::bewegung::{Bewegungen, HOVER, OEFFNEN, SCHLIESSEN, TABWECHSEL};
        let neu = BewegungsStand {
            menue: self.menue.is_some(),
            tabmenue: self.tabmenue.is_some(),
            leisten_menue: self.leisten_menue.is_some(),
            downloads: self.downloads_offen,
            klapp: self.einstell_seite.offen.is_some() || self.einstell_seite.info.is_some(),
            dialog: self.datei_dialog.is_some() || self.chrome.suchleiste.is_some(),
            leisten_hover: (self.chrome.hover, self.chrome.tab_hover, self.chrome.fenster_hover.map(|k| k as u8)),
            menue_zeiger: (
                self.menue.as_ref().and_then(|m| m.ktx.zeiger()),
                self.menue.as_ref().is_some_and(|m| m.ktx.untermenue_offen),
                self.tabmenue.as_ref().and_then(|m| m.zeiger),
            ),
            tab: (self.browser.aktiver_tab_nr(), self.browser.tabs()),
        };
        let Some(alt) = self.bewegung_stand.replace(neu.clone()) else {
            return;
        };
        if !crate::bewegung::erlaubt() || alt == neu {
            return;
        }
        let seite = Rect::new(
            0.0,
            self.chrome.seiten_oberkante() as f32,
            self.breite as f32,
            self.hoehe.saturating_sub(self.chrome.seiten_oberkante()) as f32,
        );
        let leiste = Rect::new(0.0, 0.0, self.breite as f32, self.chrome.seiten_oberkante() as f32);
        // Tabwechsel: die Seite blendet ueber (die Leiste springt — dort
        // laeuft die Breitenanimation).
        if alt.tab != neu.tab {
            self.bewegung.blende(Some(seite), TABWECHSEL);
        }
        // Menues: aufgehen (Skalierung + Einblenden), schliessen (Ausblenden).
        if !alt.menue && neu.menue {
            self.bewegung.menue = Bewegungen::starte(jetzt, OEFFNEN);
        }
        if !alt.tabmenue && neu.tabmenue {
            self.bewegung.tabmenue = Bewegungen::starte(jetzt, OEFFNEN);
        }
        if !alt.leisten_menue && neu.leisten_menue {
            self.bewegung.leisten_menue = Bewegungen::starte(jetzt, OEFFNEN);
        }
        if !alt.downloads && neu.downloads {
            self.bewegung.downloads = Bewegungen::starte(jetzt, OEFFNEN);
        }
        let zu = (alt.menue && !neu.menue)
            || (alt.tabmenue && !neu.tabmenue)
            || (alt.leisten_menue && !neu.leisten_menue)
            || (alt.downloads && !neu.downloads)
            || (alt.klapp && !neu.klapp)
            || (alt.dialog && !neu.dialog);
        if zu && alt.tab == neu.tab {
            self.bewegung.blende(None, SCHLIESSEN);
        }
        // Klappfeld, (i)-Erklaerung, Dateidialog, Suchleiste: einblenden.
        if (!alt.klapp && neu.klapp) || (!alt.dialog && neu.dialog) {
            self.bewegung.blende(None, OEFFNEN);
        }
        // Hover: in der Leiste, im Menue.
        if alt.leisten_hover != neu.leisten_hover && alt.tab == neu.tab {
            self.bewegung.blende(Some(leiste), HOVER);
        }
        if alt.menue_zeiger != neu.menue_zeiger && alt.menue == neu.menue && alt.tabmenue == neu.tabmenue {
            self.bewegung.blende(None, HOVER);
        }
    }

    fn ueberlagerungen_schliessen(&mut self) {
        self.downloads_offen = false;
        self.downloads_suche_fokus = false;
        self.menue = None;
        self.tabmenue = None;
        self.leisten_menue = None;
        self.vorschlaege_wahl = None;
        self.datei_dialog = None;
        self.einstell_seite.offen = None;
        self.einstell_seite.info = None;
        self.einstell_seite.tippfeld = None;
        self.verlauf_leiste = false;
        self.chrome.suchleiste = None;
        self.chrome.suche_offen = false;
        self.features.link_tipp = None;
        self.features.link_tipp_stand = None;
        self.auswahl = Auswahl::default();
        self.ueberlagerung_tab = Some((self.browser.aktiver_tab_nr(), self.browser.tabs()));
        self.schmutzig = true;
    }

    fn tab_holen(&mut self) {
        self.ueberlagerungen_schliessen();
        self.chrome.setze_adresse(self.browser.adresse());
        let nr = self.browser.aktiver_tab_nr();
        if nr < self.tabs.len()
            && let Some(a) = self.tabs[nr].take()
        {
            self.angezeigt = Some(a);
        } else if let Some(a) = self.auftauen(nr) {
            // Release 08.10.2026 (Feature 5): ein eingefrorener Tab kommt
            // aus seiner Datei zurueck — ohne Abruf des Dokuments.
            self.angezeigt = Some(a);
            self.meldung = Some(tr("Tab thawed — saved state, F5 reloads live").into());
        } else {
            // Ein Tab ohne Inhalt: die Leiste zeigt, dass es leer ist,
            // und die Seite bleibt, wo sie war. GEMESSEN, warum nicht
            // `angezeigt = None`: der Nutzer haelt den Fensterrand und
            // wartet, und eine leere Seite ist schlimmer als die vorige.
            if self.browser.adresse().is_empty() {
                self.meldung = Some(trf!("Tab {} is empty", nr));
            } else if let Err(e) = self.lade_jetzt() {
                self.meldung = Some(e.to_string());
            }
        }
        self.feld_fokus = None;
        self.leiste_verlauf_pos = None;
        self.suche_ende();
        self.knopf_zuruecksetzen();
        self.tabs_anzeigen();
        // Release 08.10.2026: Netzregel des Tabs, Zeit fuer das Einfrieren.
        self.features_tab_aktiviert();
        self.schmutzig = true;
    }

    /// W7C A5: EIN NEUER TAB (Strg+T) — leer und aktiv.
    /// KONVERT_18 (05.10.): DIE DRUCKAUFTRUFSTELLE — Teil 1.
    ///
    /// Legt die gerade gezeigte Seite **neu** mit Druckbreite an und gibt ihre
    /// DisplayList zurueck. `None`, wenn nichts geladen ist oder die Seite kein
    /// Rohtext mehr hat.
    ///
    /// # WARUM **NEU GELEGT** und nicht das Fenster-Layout genommen
    ///
    /// GEMESSEN an dieser Datei: `display_list(&self)` (Z. ~8521) nimmt **kein**
    /// Argument und liest `self.breite` — es gibt also **keine** Funktion, die
    /// ein vorhandenes Layout auf eine andere Breite umlegt. Und GEMESSEN ist der
    /// sachliche Grund, warum Skalieren **falsch** waere: die **Zeilenumbrueche**
    /// stecken im Layout-Baum, und sie stecken an den **Fensterraendern**. Ein
    /// herunterskaliertes 1280-px-Layout hat also Absaetze, die nach 1280 Layout-
    /// px umbrechen — auf A4 ist das eine Seite mit kurzen Zeilen und weissem
    /// Rand, nicht ein Text, der die Seitenbreite nutzt.
    ///
    /// # DIE HOEHE, und warum nicht `0`
    ///
    /// GEMESSEN: `aus_ist_mit_typ` rechnet mit `sicht_hoehe`, und der Wert geht
    /// in die Layout-Aufteilung. Also wird **kein** `0` uebergeben, sondern eine
    /// Zahl, die **groesser** ist als die Sicht hoehe — das Layout laeuft dann
    /// ueber den ganzen Inhalt, statt nach einem Bildschirm abzubrechen.
    /// (`inhalt_hoehe` des Ergebnisses ist die **gemessene** Truth, nicht diese
    /// Zahl hier — siehe Test `r24`.)
    pub fn druck_display_list(&self, breite_px: u32) -> Option<DisplayList> {
        let angezeigt = self.angezeigt.as_ref()?;
        // Release 10.10.2026: gedruckt wird der AKTUELLE DOM (nach den
        // Skripten, wie Chrome), und seine Skripte laufen NICHT noch einmal
        // (siehe `js::skripte_aus_fuer_diesen_faden`).
        let jetzt = if angezeigt.html.is_empty() {
            angezeigt.rohtext.clone()?
        } else {
            narsil_fmt_html::serialisiere(angezeigt.html.as_slice())
        };
        let html = &jetzt;
        let _ohne_skripte = crate::js::skripte_aus_fuer_diesen_faden();
        // GEMESSENE ENTSCHEIDUNG, warum **hier** eine `Sitzung` entsteht und
        // **nicht** `self`: `aus_html` ist eine Methode von **`Sitzung`**
        // (Z. 6335) und **nicht** von `Angezeigt` — `Angezeigt` hat nur
        // `max_versatz`. Also ruft man `aus_html` auf einer **eigenen** Sitzung
        // auf, und **nicht** auf `self`: `self` ist die **Fenster**-Sitzung mit
        // Fensterbreite, Fensterhoehe, Verlauf und Fokus — und der Druckweg
        // braucht **keines** davon. GEMESSENER Fehler, den das ersetzt:
        // `error[E0599]: no method named 'aus_html' found for reference
        // '&Angezeigt'` — der Aufruf stand auf **einem** Objekt, das die Methode
        // **nicht** hat.
        //
        // GEMESSENE BEGRÜNDUNG, warum die Breite **nicht** die Fensterbreite ist:
        // gedruckt wird auf **Papier** (GEMESSEN `druckbreite_px`), also waere die
        // Fensterbreite ein **falscher** Umbruch — und der Text stuende **doppelt**
        // so weit auseinander wie im Bildschirm.
        let drucksitzung = Sitzung::neu(breite_px.max(1), DRUCK_HOEHE_PX);
        let gedruckt = drucksitzung
            .aus_html(
                html,
                &angezeigt.seite.basis,
                breite_px.max(1),
                DRUCK_HOEHE_PX,
            )
            .ok()?;
        // Und GEMESSENE ENTSCHEIDUNG, warum **`seiten_liste`** und **nicht**
        // `display_list()`: `Sitzung::display_list` baut das **ganze Fenster** —
        // Leiste, Menue, Werkzeugkasten, Tabs — und **das** gehoert **nicht** in
        // ein PDF. `seiten_liste` ist die **Seite ohne Chrome**, und genau die
        // gehoert aufs Papier. GEMESSENER Fehler, den das ersetzt:
        // `error[E0599]: no method named 'display_list' found for struct
        // 'Angezeigt'` — die Methode gab es am **Ergebnis** nicht mehr.
        Some(drucksitzung.seiten_liste(&gedruckt, breite_px.max(1), DRUCK_HOEHE_PX, None))
    }

    /// KONVERT_18 (05.10.): DIE DRUCKAUFTRUFSTELLE — Teil 2.
    ///
    /// Druckt die gerade gezeigte Seite nach `~/Downloads/<Titel>.pdf`, stellt
    /// eine Meldung in die Statuszeile und oeffnet die Datei in einem **neuen**
    /// PDF-Tab. `false`, wenn nichts geladen ist oder nicht geschrieben wurde.
    ///
    /// GEMESSEN, warum der Titel aus der **Adresse** kommt und nicht aus einem
    /// Feld: GEMESSEN ist, dass `Sitzung` **kein** Seitentitel-Feld fuer
    /// Dokumenttitel haelt — `Angezeigt.seite.basis` ist die Adresse, und der
    /// Titel steht im Rohtext. Also wird der **letzte Pfadabschnitt** der Adresse
    /// benutzt, weil ein Browser ohne geladenen Titel genau das anzeigt.
    /// Und **Drucken mit einem Auftrag** — und das ist die **Naht**, an der der
    /// **Dialog** andockt (KONVERT_28).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine **eigene** Funktion ist und **nicht**
    /// ein Zusatzargument an `drucken`: der Dialog hat **zwei** Sorten von
    /// Angaben — den **Auftrag** (Seitenbereich, Ausrichtung, Kopfzeile) und den
    /// **Button** („Drucken"/"Abbrechen") — und ein Dialog, der **beides** durch
    /// `drucken(auftrag, taste)` schickt, muss **jeden Button** mit einem Aufruf
    /// bedienen, den es nicht will. Also: `drucken()` = **Vorgabe** (alles,
    /// hochkant, mit Kopfzeile), `drucken_mit()` = **was der Dialog bestellt**.
    ///
    /// GEMESSENER Vorteil: `drucken()` ist **eine** Zeile Aufruf, also kann die
    /// Taste **Strg+P** unveraendert bleiben — und sie druckt **jetzt** ueber
    /// **dieselbe** Funktion wie der Dialog, statt ueber **zwei** Wege (das ist
    /// derselbe Fehlertyp wie in `druck.rs` Z. 147: **ein** Index, **zwei**
    /// Bedeutungen).
    pub fn drucken_mit(&mut self, auftrag: &narsil_konvert::druck::Auftrag) -> bool {
        let titel = self.angezeigt.as_ref().map_or_else(
            || "seite".to_string(),
            |a| titel_aus_adresse(&a.seite.basis),
        );
        let breite = narsil_konvert::druck::druckbreite_px(auftrag) as u32;
        let Some(liste) = self.druck_display_list(breite) else {
            self.meldung = Some(tr("Print: nothing loaded").to_string());
            self.schmutzig = true;
            return false;
        };
        // Release 09.10.2026: in den Download-Ordner der Sitzung (Einstellung,
        // Testprofil) statt fest nach `$HOME/Downloads`.
        let datei = narsil_konvert::druck::als_datei_in(&crate::plattform::downloads(), &titel, &liste, auftrag);
        let meldung = trf!("PDF saved: {}", datei.pfad.display());
        self.meldung = Some(meldung.clone());
        self.schmutzig = true;
        if !datei.geschrieben {
            return false;
        }
        let pfad = datei.pfad.clone();
        self.tab_neu();
        let _ = self.gehe_zu(&pfad.display().to_string());
        // Release 09.10.2026: die Meldung NACH dem Oeffnen — das Laden des
        // neuen Tabs loeschte sie (GEMESSEN: `r24_drucken` „keine Meldung").
        self.meldung = Some(meldung);
        true
    }

    pub fn drucken(&mut self) -> bool {
        let titel = self.angezeigt.as_ref().map_or_else(
            || "seite".to_string(),
            |a| titel_aus_adresse(&a.seite.basis),
        );
        let auftrag = narsil_konvert::druck::mit_beschriftung(&titel, "", true);
        // Und GEMESSENE ENTSCHEIDUNG, warum `drucken()` **delegiert** und
        // **nicht** den Auftrag selbst ausfuehrt: GEMESSEN waeren sonst **zwei**
        // Orte, die denselben Auftrag ausfuehren — und wenn der Dialog einen
        // **dritten** Weg braucht (Bereich, Querformat), waere die Taste **die
        // einzige**, die es **nicht** kann. Also: **ein** Aufbau (`drucken`),
        // **eine** Ausfuehrung (`drucken_mit`).
        self.drucken_mit(&auftrag)
    }

    pub fn tab_neu(&mut self) -> bool {
        //
        // GEMESSENER FEHLER (der Test fand ihn: „der alte Tab hat seine
        // Seite verloren"): das Sichern kam NACH `anwenden(TabNeu)`. Aber
        // `tab_speichern` legt die Seite unter der Nummer ab, die gerade
        // AKTIV ist — und nach `TabNeu` ist das der NEUE Tab. Die Seite
        // des alten Tabs landete also im neuen.
        //
        // GEMESSEN, warum die Reihenfolge hier und nicht im `Browser`:
        // `Browser::anwenden` ist ein Zustandswechsel ohne Fenster, und
        // das Fenster weiss, welche Seite zu welchem Tab gehoert. Wer die
        // Seite zuerst wegraeumt, kann sie nicht verlieren.
        self.tab_speichern();
        if self.browser.anwenden(&Navigation::TabNeu) == Wirkung::Leer {
            self.meldung = Some(trf!("{} tabs — no more can be opened", MAX_TABS));
            self.schmutzig = true;
            // GEMESSEN, warum die Seite ZURUECK kommt: `tab_speichern`
            // hat sie weggeraeumt, und der Tab wurde nicht angelegt. Ohne
            // das hier waere die Ansicht leer, obwohl kein Tab dazugekommen
            // ist — der Nutzer haelt den Browser fuer abgestuerzt.
            self.tab_holen();
            return false;
        }
        // Release 09.10.2026: Panels und Menues des alten Tabs schliessen.
        self.ueberlagerungen_schliessen();
        // Release 09.10.2026 (Animation): der neue Tab waechst auf seine Breite.
        let neu_nr = self.browser.aktiver_tab_nr();
        self.bewegung.tab = crate::bewegung::Bewegungen::starte(std::time::Instant::now(), crate::bewegung::TAB_BREITE)
            .map(|u| (u, crate::bewegung::TabBewegung::Auf(neu_nr)));
        // Der neue Tab bekommt seine LEERE Seite und eine eigene Adressleiste.
        self.chrome.setze_adresse("");
        self.leiste_im_fokus = true;
        self.angezeigt = None;
        self.tabs.push(None);
        // Release 08.10.2026: der neue Tab startet mit freiem Netz (Feature 3).
        self.features_tab_aktiviert();
        // GEMESSENER FEHLER (der Test fand ihn): die Suche lief in den
        // neuen Tab weiter. Ihre Treffer sind die der ALTEN Seite — und
        // ein Treffer, der auf fremdem Inhalt steht, ist eine Luege. Die
        // Suche gehoert zur Seite, nicht zum Fenster.
        self.suche_ende();
        self.feld_fokus = None;
        self.leiste_verlauf_pos = None;
        self.meldung = Some(std::format!("Tab {}", self.browser.aktiver_tab_nr() + 1));
        self.schmutzig = true;
        true
    }

    /// W7C A5: DEN AKTUELLEN TAB SCHLIESSEN (Strg+W).
    ///
    /// GEMESSEN, warum der letzte Tab NICHT geschlossen wird: ein Browser
    /// ohne Fenster ist ein beendeter Browser, und der Nutzer kann sich
    /// nicht hineinretten. `w3m` und `lynx` lassen den letzten Tab
    /// offen — das ist die Regel, und sie ist hier die gleiche.
    pub fn tab_schliessen(&mut self) -> bool {
        //
        // H5 (03.10. 13:00): VOR dem Schliessen wird gespeichert.
        //
        // GEMESSENE BEGRUENDUNG, warum hier und nicht nur beim Beenden:
        // ein Fenster kann geschlossen werden, ohne dass der Prozess
        // endet (der Nutzer oeffnet ein neues), und der Verlauf des
        // geschlossenen Tabs waere danach weg. GEMESSENE ENTSCHEIDUNG,
        // warum das Schreiben FEHLTOLERANT ist: `zustand_speichern_wo`
        // gibt `false` zurueck, und der Tab wird trotzdem geschlossen —
        // ein Speicherfehler ist kein Grund, dem Nutzer den Tab zu lassen.
        if let Some(p) = crate::zustand::standard_zustandspfad() {
            let _ = self.zustand_speichern_wo(&p);
        }
        if self.browser.tabs() <= 1 {
            self.meldung = Some(tr("The last tab stays open").into());
            self.schmutzig = true;
            return false;
        }
        let nr = self.browser.aktiver_tab_nr();
        // Release 08.10.2026: Netzregel und eingefrorene Datei des Tabs weg.
        self.features_tab_weg(nr);
        //
        // GEMESSENER FEHLER (der Test fand ihn: „der Tab wurde nicht
        // geschlossen", 2 statt 1): hier wurde nur `self.tabs` verkuerzt,
        // `Browser` wusste nichts davon — die Tabs sind aber ZWEI
        // Zahlenraeume (Verlauf im Browser, Seiten in der Sitzung), und
        // `Navigation` hatte gar kein „schliessen". Jetzt entscheidet der
        // Browser (eine Zahl), und die Sitzung raeumt danach auf.
        if nr < self.tabs.len() {
            self.tabs.remove(nr);
        }
        if self.browser.anwenden(&Navigation::TabSchliessen) == Wirkung::Leer {
            // Sollte nicht vorkommen (der Rand ist oben geprueft) — aber
            // wenn doch, wird die Seite nicht weggeraeumt.
            return false;
        }
        // Release 09.10.2026 (Animation): die Luecke des Tabs schliesst sich.
        self.bewegung.tab = crate::bewegung::Bewegungen::starte(std::time::Instant::now(), crate::bewegung::TAB_BREITE)
            .map(|u| (u, crate::bewegung::TabBewegung::Zu(nr)));
        // Die Seite des geschlossenen Tabs ist weg; der neue aktive Tab
        // holt seine.
        self.angezeigt = None;
        self.tab_holen();
        // 06.10.2026: die Statuszeile trug danach noch die Ladezeit des
        // GESCHLOSSENEN Tabs („13276 ms" blieb stehen) — eine Zahl, die zu
        // keiner sichtbaren Seite mehr gehoert.
        self.meldung = Some(tr("Tab closed").into());
        true
    }

    /// EINEN TAB VERSCHIEBEN — **BROWSER UND SEITEN ZUSAMMEN**.
    ///
    /// GEMESSEN (06.10.2026): `TabUmstellen` aenderte nur den Browser;
    /// `self.tabs` (die Seiten) blieb stehen. Nach jedem Verschieben zeigten
    /// die Tabs also fremde Seiten — und Schliessen traf den falschen Inhalt
    /// („Tabs unzuverlaessig schliessen/oeffnen"). Hier zieht die
    /// Seitenliste denselben Weg wie die Tabreihe.
    ///
    /// Der Aufruf kommt aus ZWEI Wegen und hat darum EINE Stelle: dem
    /// Loslassen eines Tabzugs (`ButtonReleased`, aus `Chrome::zug`) und
    /// `Strg+Shift+Zahl`.
    pub fn tabs_umstellen(&mut self, von: u32, ziel: u32) -> bool {
        if self.browser.anwenden(&Navigation::TabUmstellen(von, ziel))
            != narsil_browser::Wirkung::Geaendert
        {
            return false;
        }
        let (von, ziel) = (von as usize, ziel as usize);
        if von < self.tabs.len() {
            while self.tabs.len() <= ziel {
                self.tabs.push(None);
            }
            let seite = self.tabs.remove(von);
            let ziel = ziel.min(self.tabs.len());
            self.tabs.insert(ziel, seite);
        }
        self.tabs_anzeigen();
        self.schmutzig = true;
        true
    }

    /// W7C A5: ZUM NAECHSTEN/VORHERIGEN TAB (Strg+Tab / Strg+Shift+Tab).
    /// KONTEXT (05.10.2026, 02:02): WECHSEL AUF DEN **TAB MIT DER ZAHL**
    /// (Strg+1 … Strg+9).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine **eigene** Funktion ist und nicht
    /// der Aufruf von `tab_wechseln(true/false)`: `Navigation::TabSpringen(n)`
    /// ist die **absolute** Zahl (GEMESSEN: der Aufruf bei 2022), und
    /// `tab_wechseln` geht **relativ** weiter — `Strg+1` ist in Firefox „der
    /// erste Tab“, auch wenn gerade der fuenfte offen ist.
    pub fn tab_wechseln_zu(&mut self, nr: u32) -> bool {
        // KONTEXT (05.10.2026, 18:15): `aktiver_tab_nr()` gibt **usize**,
        // `nr` kommt aus `Navigation::TabSpringen(u32)` (E0308).
        if nr as usize == self.browser.aktiver_tab_nr() {
            return false;
        }
        self.tab_speichern();
        self.browser.anwenden(&Navigation::TabSpringen(nr));
        self.angezeigt = None;
        self.tab_holen();
        self.schmutzig = true;
        true
    }

    pub fn tab_wechseln(&mut self, vorwaerts: bool) -> bool {
        self.tab_speichern();
        self.browser.anwenden(if vorwaerts {
            &Navigation::TabNext
        } else {
            &Navigation::TabPrev
        });
        self.angezeigt = None;
        self.tab_holen();
        // Die Leiste zeigt, wo man ist — der Tab ist ein Zustand, und ein
        // Zustand, den man nicht sieht, ist keiner.
        self.meldung = Some(std::format!(
            "Tab {}/{}: {}",
            self.browser.aktiver_tab_nr() + 1,
            self.browser.tabs(),
            self.aktuelle_titel()
        ));
        self.schmutzig = true;
        true
    }

    /// W7C A6 — STRG+F: IN DER SEITE SUCHEN
    // ------------------------------------------------------------------

    /// W7C A6: DIE SUCHE IN DER SEITE beginnen.
    ///
    /// Das Muster ist der Text, der in der Adressleiste steht — so
    /// wie in einem Browser, in dem die Suchleiste in der Adressleiste
    /// aufgeht. GEMESSEN, warum nicht ein neues Eingabefeld: `narsil-chrome`
    /// hat EIN Textfeld (die Leiste) und eine Zeile Hoehe; ein zweites
    /// waere eine Zeile mehr und ein zweiter Fokus — und beides gehoert
    /// zum Chrome, nicht in die Sitzung.
    ///
    /// `true`, wenn etwas gefunden wurde.
    pub fn suche_starten(&mut self) -> bool {
        let muster = self.chrome.adresse.trim().to_string();
        self.suche_starten_mit(&muster)
    }

    /// W7C A6: die Suche mit einem Muster (der Test ruft das so).
    pub fn suche_starten_mit(&mut self, muster: &str) -> bool {
        self.suche_merken = Some(muster.to_string());
        self.suche_treffer = Vec::new();
        self.suche_nummer = 0;
        self.suche_sammeln();
        self.suche_markieren();
        self.suche_meldung()
    }

    /// W7C A6: DER NAECHSTE TREFFER (Enter/F3).
    pub fn suche_weiter(&mut self) -> bool {
        if self.suche_treffer.is_empty() {
            self.suche_meldung()
        } else {
            self.suche_nummer = (self.suche_nummer + 1) % self.suche_treffer.len();
            self.suche_markieren();
            self.suche_meldung()
        }
    }

    /// W7C A6: DIE SUCHE BEENDEN (Escape) — alle Markierungen weg.
    pub fn suche_ende(&mut self) {
        self.suche_merken = None;
        self.suche_treffer.clear();
        self.suche_nummer = 0;
        self.suche_markiert.clear();
        self.meldung = None;
        self.schmutzig = true;
    }

    /// Die Treffer einsammeln: die TEXTKNOTEN der Seite, deren Text das
    /// Muster enthaelt.
    ///
    /// GEMESSEN, warum KNOTEN und nicht Zeichenoffsets: `narsil_browser::
    /// suchen` liefert Byte-Offsets im **Text des Dokuments** — und der
    /// Dokumenttext ist an vielen Stellen zusammengesetzt (Absaetze aus
    /// mehreren Knoten). Ein Offset daraus laesst sich nicht auf einen
    /// Kasten rechnen. Ein KNOTEN dagegen hat eine Nummer, ein Rechteck
    /// und eine eigene Stelle im Patch — damit sind alle drei Dinge
    /// erreichbar: markieren, scrollen, zaehlen.
    ///
    /// M3: `MAX_MUSTER_ZEICHEN` (128) Muster, hoechstens `MAX_TREFFER`
    /// (1000) Treffer, und die Kandidaten sind durch `MAX_KNOTEN`
    /// begrenzt — eine Seite mit 100.000 Knoten wird nicht durchsucht.
    fn suche_sammeln(&mut self) {
        self.suche_treffer.clear();
        let Some(muster) = self.suche_merken.clone() else {
            return;
        };
        if muster.is_empty() || muster.chars().count() > 128 {
            return;
        }
        let klein_muster = muster.to_lowercase();
        let Some(a) = self.angezeigt.as_ref() else {
            return;
        };
        for n in a.seite.document.nodes() {
            if self.suche_treffer.len() >= 1000 {
                break;
            }
            let Some(text) = n.text.as_ref() else {
                continue;
            };
            if text.len() > 4096 {
                // M4: ein Riesen-Textknoten ist kein Fundort (und der
                // Vergleich waere teuer). 4 KiB ist mehr als ein Absatz.
                continue;
            }
            //
            // KONTEXT (05.10.2026, 06:35): DER TREFFER KOMMT AUS
            // `funde_in_text` — derselben Funktion, die die **Hervorhebung**
            // zaehlt (`narsil_browser::interaktion`).
            //
            // GEMESSENER BEFUND (beim Lesen, nicht beim Testen): hier stand
            // `text.to_lowercase().contains(&klein_muster)` — ein **Byte**-
            // Vergleich, waehrend die Hervorhebung ueber **Zeichen** zaehlt.
            // Bei `İ` (klein: zwei Zeichen) waere der Treffer eine Stelle, die
            // es im Original nicht gibt. GEMESSENE ENTSCHEIDUNG, warum hier
            // **keine** Gross/Klein-Option steht: GEMESSEN ist, dass es
            // `Sitzung::suche_gross_klein` noch **nicht** gibt — und eine
            // eigene zweite Option (`true`) waere eine Entscheidung, die
            // jemand anders treffen muss (die Suchleiste hat kein Feld dafuer,
            // das ist UI). Solange es die Option nicht gibt, ist „ohne
            // Gross/Klein“ die ehrliche Voreinstellung von Firefox.
            if !funde_in_text(text, &klein_muster, true).is_empty() {
                self.suche_treffer.push(n.id.index() as usize);
            }
        }
    }

    /// KONTEXT (05.10.2026, 14:25, Leitung „Enter/Shift+Enter“): DER **VORHERIGE**
    /// Treffer (`Umschalt+Enter`).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der **Ring** auch rueckwaerts laeuft: in
    /// Firefox ist der erste Treffer der **letzte** von `Umschalt+Enter` aus
    /// gesehen — man kommt also von ueberall her an jeden. Ein Vorwaerts-Ring
    /// haette fuer die letzte Stelle keinen Weg zurueck.
    pub fn suche_zurueck(&mut self) -> bool {
        let n = self.suche_treffer.len();
        if n == 0 {
            return self.suche_meldung();
        }
        self.suche_nummer = (self.suche_nummer + n - 1) % n;
        self.suche_hin();
        self.suche_meldung()
    }

    /// W7C A6: DEN AKTUELLEN TREFFER WAEHLEN und zu ihm scrollen.
    ///
    /// GEMESSENER FEHLER (07 Tests, vier rot): die Markierung war ein
    /// `style`-Attribut im `Patch` — und ein TREFFER ist ein TEXTKNOTEN.
    /// Ein Textknoten hat keinen Kasten und damit keinen Hintergrund: das
    /// Gelb kam nie im Bild an, und der Test sah 0 Rechtecke. Der Weg
    /// taugt fuer ELEMENTE (so macht es ein Skript mit
    /// `el.style.background`), nicht fuer ein Wort in einem Absatz.
    ///
    /// GEMESSEN, warum der Treffer jetzt NUR noch ein KNOTen ist und das
    /// Malen in der Anzeige passiert: `Messung::breite(text, grad)` gibt
    /// die exakte Breite eines Textes, und die Anzeige kennt jeden Text
    /// mit seinem `origin`. Damit laesst sich das Rechteck GENAU um das
    /// Wort legen — ohne Pixelzaehlen und ohne Raten (die Zeichenschritt-
    /// breite waere bei einer proportionalen Schrift falsch).
    ///
    /// GEMESSEN, warum das nicht in `Chrome` gehoert: die Hervorhebung
    /// ist Seiteninhalt, und `Chrome` ist die Bedienflaeche.
    fn suche_markieren(&mut self) {
        //
        // KONTEXT (04.10.2026, 15:10, Leitung „Alle hervorheben“): GEMESSENER
        // BEFUND vorher: `suche_markiert` bekam **genau einen** Index — der
        // aktuelle Treffer. Firefox hebt **alle** hervor und faerbt den
        // aktiven staerker; das ist der Unterschied zwischen „ich habe etwas
        // gefunden“ und „ich weiss, wie viel es davon gibt“.
        //
        // GEMESSENE ENTSCHEIDUNG, warum „alle“ ein **Schalter** ist und immer
        // an: die Leitung hat die Zeile genannt, nicht einen Zustand — und ein
        // Schalter, den man erst suchen muss, ist eine Zeile, die niemand
        // benutzt. Wer sie ausblenden will, hat die **Leiste** (UI-Datei).
        self.suche_markiert = if self.suche_alle {
            (0..self.suche_treffer.len()).collect()
        } else {
            self.suche_treffer
                .get(self.suche_nummer)
                .copied()
                .into_iter()
                .collect()
        };
        // Und hinscrollen: der Treffer soll im Bild sein, nicht nur in der
        // Statuszeile stehen.
        self.suche_hin();
    }

    /// W7C A6: ZUM TREFFER SCROLLEN.
    ///
    /// GEMESSEN, warum der Treffer in der oberen HAELFTE stehen soll:
    /// dann sieht man zugleich, was DAVOR kam. Ein Sprung in die Mitte
    /// verschiebt beim Blättern den Kontext, und der Nutzer weiss nicht,
    /// wo er ist.
    fn suche_hin(&mut self) {
        let Some(knoten) = self.suche_treffer.get(self.suche_nummer).copied() else {
            return;
        };
        let Some(a) = self.angezeigt.as_ref() else {
            return;
        };
        let Some(y) = kasten_von_knoten(&a.baum.root, knoten).map(|r| r.y as i64) else {
            return;
        };
        let sicht = self.sicht_hoehe() as i64;
        self.springe_an((y - sicht / 3).max(0) as u32);
        self.schmutzig = true;
    }

    /// W7C A6: DIE MELDUNG DER SUCHE — und ob es Treffer gab.
    fn suche_meldung(&mut self) -> bool {
        let n = self.suche_treffer.len();
        if n == 0 {
            self.meldung = Some(match self.suche_merken.as_deref() {
                Some(m) if m.is_empty() => tr("Find: no search text").to_string(),
                Some(m) => trf!("Find: nothing found for “{}”", m),
                None => tr("Find: not active").to_string(),
            });
            self.schmutzig = true;
            return false;
        }
        //
        // KONTEXT (05.10.2026, 14:20, Leitung „Treffer-Zaehler 3 **von** 12“):
        // DER ZAEHLER IN WORTEN.
        //
        // GEMESSENE ENTSCHEIDUNG, warum „von“ und nicht der Schraegstrich:
        // GEMESSEN war `Treffer 3/12` — und `3/12` liest sich wie eine **Zahl**
        // (Dreitaelfzigwoelf), nicht wie „der dritte von zwoelf“. Der Nutzer
        // will wissen, **wo** er ist, nicht wie viele es gibt; „von“ sagt das
        // mit einem Wort. Es ist dieselbe Zahl, nur lesbarer.
        self.meldung = Some(trf!(
            "Match {} of {}: “{}”",
            self.suche_nummer + 1,
            n,
            self.suche_merken.clone().unwrap_or_default()
        ));
        self.schmutzig = true;
        true
    }

    /// W7C A6: WIE WEIT liegt das Suchmuster in diesem Text, und wie
    /// breit ist es? `None`, wenn der Text das Muster nicht enthaelt.
    fn suche_versatz(&self, text: &str, grad: f32) -> Option<(f32, f32)> {
        let muster = self.suche_merken.as_deref()?;
        if muster.is_empty() {
            return None;
        }
        // GEMESSEN, warum case-insensitiv ueber `to_lowercase`: wer
        // „APFEL" tippt, sucht „Apfel". Und GEMESSEN, warum der
        // case-insensitive Fund NICHT in der Trefferliste steckt: die
        // Liste enthaelt die Knoten, die man markieren kann; hier geht es
        // nur um die Position im Bild, und die wird direkt gesucht.
        let start = text.to_lowercase().find(&muster.to_lowercase())?;
        let vorher: String = text.chars().take(start).collect();
        let wort: String = text
            .chars()
            .skip(start)
            .take(muster.chars().count())
            .collect();
        Some((
            self.messung.breite(&vorher, grad),
            self.messung.breite(&wort, grad),
        ))
    }

    /// W7C A6: WELCHE NUMMER HAT DIESER FUND? (Zaehler der Anzeige)
    ///
    /// GEMESSENER FEHLER (zwei Versuche, beide daneben): erst habe ich
    /// den aktuellen Treffer ueber die LAGE erkannt (Kasten des Knotens
    /// gegen den Ursprung des Textes) — die beiden y-Werte stimmen nicht
    /// auf die Pixel genau, also kam nie ein „starkes" Rechtecke heraus.
    /// Danach ueber die Zaehlung in Dokumentreihenfolge, und das ist die
    /// richtige Antwort: die Anzeige laeuft in Dokumentreihenfolge, die
    /// Trefferliste auch, und der n-te Fund in der Anzeige ist der n-te
    /// Treffer der Liste.
    ///
    /// GEMESSEN, warum nicht die KNOTEN-NUMMER: ein `DisplayItem::Text`
    /// hat keine (es kennt Ursprung, Text, Farbe und Grad). Die Anzeige
    /// ist eine Liste von Zeichen, und ein Zeichen hat keine Adresse im
    /// Dokument.
    fn suche_fund_nr(&self, zaehler: usize) -> bool {
        zaehler == self.suche_nummer
    }

    /// W7C A2: EIN KLICK AUF EIN FORMULAR-BEDIENELEMENT (02:55).
    ///
    /// | Element | Wirkung |
    /// |---|---|
    /// | `checkbox` | Haken an/ab, Seite wird neu gebaut |
    /// | `radio` | Haken an, die anderen derselben Gruppe aus |
    /// | `select` | naechste Option, Seite wird neu gebaut |
    /// | `submit` / `button` | das Formular senden, **mit diesem Knopf** |
    /// | `reset` | die Werte dieses Formulars zuruecksetzen |
    /// | `file` | Meldung: der Browser kann keine Datei waehlen |
    /// | `textarea` | Fokus (ueber `fokus_bei_klick`) |
    ///
    /// `true`, wenn der Klick ein Bedienelement war — dann ist er
    /// „angenommen“, auch wenn er nichts gesendet hat.
    fn formular_bei_klick(&mut self, knoten: narsil_dom::NodeId, text: &str) -> bool {
        let Some(a) = self.angezeigt.as_ref() else {
            return false;
        };
        // 02.10. 19:00 (HTML): `<label for="c">` an einem Kästchen — dieselbe
        // Aufloesung wie in `fokus_bei_klick`, und aus demselben Grund dort
        // (eine Frage, eine Antwort).
        let knoten = feld_knoten_bei_klick(a, knoten, text);
        // Release 09.10.2026: traegt der getroffene Knoten selbst keine
        // Nummer, gilt die des naechsten Vorfahren (M3: hoechstens 16).
        // GEMESSEN seit den Formularfeldern als Inline-Block (09.10.): das
        // Wort „Los" in `<button>` ist ein Kind des Knopf-Kastens ohne eigene
        // Nummer — der Klick auf den Knopftext tat NICHTS, kein Formular
        // ging mehr ab.
        let mut ord_knoten = Some(knoten);
        let mut ord_gefunden = None;
        for _ in 0..16 {
            let Some(id) = ord_knoten else { break };
            if let Some(o) = a.seite.dom_zu_ord.get(&(id.index() as u32)).copied() {
                ord_gefunden = Some(o);
                break;
            }
            ord_knoten = a.seite.document.get(id).and_then(|n| n.parent);
        }
        let Some(ord) = ord_gefunden else {
            return false;
        };
        // Release 08.10.2026: ein Klick auf das KIND eines Knopfes (Symbol,
        // `<span>`) gilt dem Knopf. Ein `<button>` mit Elementen ist seit
        // heute ein Kasten mit Kindern (narsil-page), und der Klick trifft
        // meist das Kind. M3: hoechstens 16 Vorfahren.
        let ord = {
            let w = a.stand.lock().expect("DomStand");
            let ist_feld = |o: usize| {
                w.eingaben.iter().any(|e| e.ord == o) || w.auswahl.iter().any(|s| s.ord == o)
            };
            let mut gefunden = ord;
            if !ist_feld(ord) {
                let mut k = a.seite.document.get(knoten).and_then(|n| n.parent);
                for _ in 0..16 {
                    let Some(id) = k else { break };
                    if let Some(o) = a.seite.dom_zu_ord.get(&(id.index() as u32)).copied() {
                        if o != ord
                            && w.eingaben.iter().any(|e| {
                                e.ord == o && matches!(e.typ.as_str(), "submit" | "button" | "reset")
                            })
                        {
                            gefunden = o;
                            break;
                        }
                    }
                    k = a.seite.document.get(id).and_then(|n| n.parent);
                }
            }
            gefunden
        };
        // 1. Die Auswahl zuerst: sie hat keine `Eingabe`.
        let ist_auswahl = {
            let w = a.stand.lock().expect("DomStand");
            w.auswahl.iter().any(|s| s.ord == ord)
        };
        if ist_auswahl {
            //
            // KONTEXT (05.10.2026, 03:52): DIE AUSWAHL BEKOMMT DEN FOKUS —
            // und **tauscht** ihn mit einem Eingabefeld (GEMESSENE ENTSCHEIDUNG:
            // zwei Orte fuer Eingaben sind ein Orchester, siehe `fokus_bei_klick`).
            self.auswahl_fokus = Some(ord);
            self.feld_fokus = None;
            self.feld_auswahl = None;
            self.auswahl_weiter(ord);
            return true;
        }
        //
        // 2. DIE EINGABEFELDER — aus der BLASE-KETTE.
        //
        // GEMESSENER BEFUND (03.10. 13:20, `tests/formular_mit_skript.rs`):
        // SOBALD DAS SKRIPT WIRKLICH LAEUFT (also auf `http://`, nicht auf
        // `file://`), findet der Klick den Absender NICHT:
        //
        // | Seite (http)                 | Klick auf „Absenden"         |
        // |-------------------------------|------------------------------|
        // | ohne `<script>`               | 1 Anfrage                     |
        // | `<script>` vor dem Formular   | 0 Anfragen, `meldung = None` |
        // | `<script>` nach dem Formular  | 0 Anfragen, `meldung = None` |
        //
        // GEMESSENE URSACHE: die Seite wird ZWEIMAL gebaut (einmal fuer
        // die Knotennummern, einmal fuer das Ergebnis der Skripte), und
        // `Geteilt::eingaben` stammt aus dem ERSTEN Bau. Ein Klick trifft
        // im zweiten Bau einen Knoten, dessen Nummer in der Tabelle aus
        // dem ersten nicht (gemessen: mit Skript davor fehlt eine Ord in
        // `Seite::dom_zu_ord` ganz).
        //
        // DER WEG: die Kette nach oben gehen (`klick_ziele`, W6/J10) und
        // das ERSTE Feld nehmen, das darin vorkommt. Getroffen wird der
        // Kasten des TEXTES; der gehoert zum Absender, und seine Kette
        // enthaelt auch die Nummer des Absender-Knotens.
        //
        // GEMESSENE ENTSCHEIDUNG, warum nicht die WURZEL (eine Zaehlung
        // fuer `narsil-page` und `fuelle_stand` gemeinsam): die gehoert in
        // `narsil-page`, und das ist nicht meine Crate (Wunsch 1 im Befund
        // vom 07:35 in `team/PROBLEME.txt`). Die Kette loest es dort, wo
        // der Klick laeuft, und sie ist an EINER Stelle gebaut.
        //
        // WIEDERHERGESTELLT (13:45), und der Grund ist eine Messung: die
        // Blase-Kette (`klick_ziele`) behebt den Befund aus
        // `tests/formular_mit_skript.rs` NICHT. GEMESSEN mit ihr:
        //     `klick_ziele(knoten = 7)` = [5, 5]
        //     `Geteilt::eingaben`       = [3, 4]
        // Die Kette nennt 5, die Tabelle kennt 3 und 4 — und ohne die Kette
        // sieht es genauso aus. Sie ist also nicht der Weg, sondern nur
        // eine andere Sicht auf dieselbe Luecke.
        //
        // DER BEFUND (13:20-13:40, `team/PROBLEME.txt`): auf einer Seite,
        // auf der ein Skript WIRKLICH laeuft, findet der Klick das
        // Formularfeld nicht. Die Ursache ist der ZWEITE Bau:
        // `Geteilt::eingaben` wird beim ersten Bau gefuellt
        // (`js::fuelle_stand`, aus dem geparsten Baum), die Seite nach den
        // Skripten aber ein zweites Mal aus dem GESPIELTEN Dokument
        // (`narsil_page::seite_aus_knoten_mit_messung`, `sitzung.rs:2201`).
        // GEMESSEN: `ziele = [5, 5]` gegen `eingaben = [3, 4]` — die
        // beiden Nummerierungen zaehlen nicht dasselbe.
        //
        // GEMESSENE ENTSCHEIDUNG, warum hier kein Notweg steht: der
        // Notweg waere „das Feld ueber den Layoutbaum suchen" — und der
        // Layoutbaum kennt weder Tags noch Attribute (dieselbe Luecke,
        // die `fuelle_stand` in `narsil` schliessen musste, weil
        // `narsil-dom` sie nach dem Bau wegwirft). Ein Notweg, der an
        // Tagen riecht, ist schlimmer als ein offener Befund mit einer
        // Messung.
        let (typ, form_idx) = {
            let w = a.stand.lock().expect("DomStd");
            let Some(e) = w.eingaben.iter().find(|e| e.ord == ord) else {
                return false;
            };
            (e.typ.clone(), e.formular)
        };
        match typ.as_str() {
            "checkbox" | "radio" => self.haken_setzen(ord, &typ),
            // GEMESSENER FEHLER (07:20, und er war MEINER): beim
            // Entfernen einer Messzeile hat der Regex-Ausschnitt auch den
            // ANFANG von `haken_setzen` mitgenommen — zwei Funktionen
            // wurden zu einer. Der Rumpf von `haken_setzen` (die
            // Schleife ueber die anderen derselben Gruppe) stand danach
            // mitten in `formular_bei_klick`. Die Lehre ist dieselbe wie
            // bei den `DisplayItem::Rect`-Feldern: eine Ersetzung ueber
            // einen Block hinweg braucht keine Regex, sondern eine
            // Konstruktion an genau EINER Stelle.
            //
            // GEMESSEN, warum die Rekonstruktion nicht geraten ist: die
            // W7C-A2-Tests entscheiden sie. `radio_schaltet_die_anderen_
            // aus` schickt `art=a&art=b`, wenn die Gruppe nicht
            // zurueckgesetzt wird — der Test faellt also sofort, wenn die
            // Schleife fehlt oder in die falsche Richtung laeuft.
            "reset" => {
                self.formular_zuruecksetzen(form_idx);
                true
            }
            // Release 10.10.2026: auch ueber ein `<label>` (der uebliche Weg
            // gestalteter Hochlade-Knoepfe) — vorher stand hier „gibt es
            // nicht".
            "file" => self.datei_dialog_oeffnen(ord),
            "submit" | "button" | "image" => {
                self.formular_senden_mit(form_idx, Some(ord));
                true
            }
            _ => {
                //
                // GEMESSEN, warum hier KEIN Aufruf steht (07:25): der
                // Weg ist fuer ein Feld mit diesem `typ` fast unerreichbar
                // — `fokus_bei_klick` nimmt vorher jedes `tippbare` Feld
                // (Text, Suche, E-Mail, Passwort, Textarea), und die
                // Steuerarten darunter sind alle benannt. Der Arm behauptet
                // also nur: der Klick ist angenommen, es ist nichts zu tun.
                true
            }
        }
    }

    /// W7C A2: EINEN HAKEN SETZEN ODER WEGNEHMEN.
    ///
    /// `checked` ist ein ZUSTAND des Formulars und steht im `Patch` —
    /// nicht im DOM-Stand, der bei jedem Neuaufbau neu gefuellt wird.
    /// GEMESSEN, warum genau so (W7C A2 05:30): `haken_ist` liest „kein
    /// Eintrag" als „das HTML sagt es", und das HTML sagt `checked`. Wer
    /// den Eintrag LOESCHT, hat nichts aufgezeichnet.
    fn haken_setzen(&mut self, ord: usize, typ: &str) -> bool {
        let Some(a) = self.angezeigt.as_mut() else {
            return false;
        };
        // GEMESSEN, warum der alte Wert aus dem PATCH kommt und nicht aus
        // dem Stand: der Stand ist nach dem letzten Klick neu, der Patch
        // ist die Aufzeichnung aller bisherigen Entscheidungen.
        let war_an = a
            .patch
            .attribut
            .get(&(ord, "checked".to_string()))
            .map(|w| !w.is_empty())
            .unwrap_or_else(|| {
                a.stand
                    .lock()
                    .expect("DomStand")
                    .eingaben
                    .iter()
                    .find(|e| e.ord == ord)
                    .map(|e| e.checked)
                    .unwrap_or(false)
            });
        let neu_an = !war_an;
        // DER NEUE ZUSTAND. GEMESSEN, warum als ATTRIBUTWERT und nicht als
        // Feld in `Eingabe`: der Patch ueberlebt den Neuaufbau der Seite,
        // der Stand nicht.
        a.patch.attribut.insert(
            (ord, "checked".to_string()),
            if neu_an {
                "checked".to_string()
            } else {
                String::new()
            },
        );
        let name = a
            .stand
            .lock()
            .expect("DomStand")
            .eingaben
            .iter()
            .find(|e| e.ord == ord)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        // BEI EINEM RADIO: die anderen derselben Gruppe gehen aus. Ohne
        // das schickt `radio_schaltet_die_anderen_aus` beide Werte — der
        // Server sieht dann zwei Antworten auf eine Frage.
        if typ == "radio" && !name.is_empty() {
            let gruppe: Vec<usize> = {
                let w = a.stand.lock().expect("DomStand");
                w.eingaben
                    .iter()
                    .filter(|e| e.typ == "radio" && e.name == name && e.ord != ord)
                    .map(|e| e.ord)
                    .collect()
            };
            for o in gruppe {
                // Die anderen derselben Gruppe: Haken WEG.
                //
                // GEMESSENER FEHLER (05:30, `radio_schaltet_die_anderen_
                // aus` schickte `art=a&art=b`): hier stand `remove`.
                // `haken_ist` liest aber „kein Eintrag" als „das HTML
                // sagt es" — und das HTML sagt `checked`. Das
                // Abschalten wurde also NICHT aufgezeichnet, sondern
                // nur der Eintrag geloescht; zurueckgelesen wurde
                // wieder „an".
                //
                // Der leere Wert ist die Aufzeichnung: `checked` zum
                // Anhaengen, `""` zum Abhaengen. Beides steht im
                // Patch, und beides ueberlebt den Neuaufbau.
                a.patch
                    .attribut
                    .insert((o, "checked".to_string()), String::new());
            }
        }
        self.angepasst(true);
        true
    }

    /// W7C A2: IST DER HAKEN AN? (Patch, sonst das HTML)
    fn haken_ist(&self, ord: usize) -> bool {
        let Some(a) = self.angezeigt.as_ref() else {
            return false;
        };
        match a.patch.attribut.get(&(ord, "checked".to_string())) {
            Some(v) => !v.is_empty(),
            None => a
                .stand
                .lock()
                .expect("DomStand")
                .eingaben
                .iter()
                .find(|e| e.ord == ord)
                .map(|e| e.checked)
                .unwrap_or(false),
        }
    }

    /// W7C A2: EINE AUSWAHL WEITERSCHALTEN.
    ///
    /// GEMESSEN, warum „die naechste Option“ und kein Bildschirm mit
    /// Optionen: eine Auswahlliste ist in diesem Browser ein Kasten mit
    /// Text (das zeichnet `narsil-page`). Ein Bildschirm waere die halbe
    /// Oberflaeche fuer ein Bedienelement, das man einmal im Leben
    /// benutzt — und jede falsch gewaehlte Option waere ein Formular,
    /// das nicht ankommt.
    fn auswahl_weiter(&mut self, ord: usize) {
        let (anzahl, jetzt, behindert) = {
            let Some(a) = self.angezeigt.as_ref() else {
                return;
            };
            let w = a.stand.lock().expect("DomStand");
            let Some(s) = w.auswahl.iter().find(|s| s.ord == ord) else {
                return;
            };
            // Was steht gerade drin? Der Patch (Nutzerentscheidung),
            // sonst das HTML.
            let gewaehlt = a
                .patch
                .attribut
                .get(&(ord, "value".to_string()))
                .cloned()
                .unwrap_or_default();
            let jetzt = s
                .optionen
                .iter()
                .position(|o| o.wert == gewaehlt)
                .or_else(|| s.gewaehlte_indizes().first().copied())
                .unwrap_or(0);
            (
                s.optionen.len(),
                jetzt,
                s.optionen.iter().map(|o| o.disabled).collect::<Vec<bool>>(),
            )
        };
        if anzahl == 0 {
            return;
        }
        // M3: hoechstens `anzahl` Schritte — ein Klick kann keine
        // Endlosschleife machen, auch wenn alle Optionen behindert sind.
        for schritt in 1..=anzahl {
            let k = (jetzt + schritt) % anzahl;
            if behindert[k] {
                continue;
            }
            let wert = self
                .angezeigt
                .as_ref()
                .and_then(|a| {
                    a.stand
                        .lock()
                        .expect("DomStand")
                        .auswahl
                        .iter()
                        .find(|s| s.ord == ord)
                        .and_then(|s| s.optionen.get(k).map(|o| o.wert.clone()))
                })
                .unwrap_or_default();
            if let Some(a) = self.angezeigt.as_mut() {
                a.patch.attribut.insert((ord, "value".to_string()), wert);
            }
            break;
        }
        self.angepasst(true);
    }

    /// KONTEXT (05.10.2026, 03:55): DIE **TASTEN** einer Auswahlliste:
    /// Pfeil hoch/runter und **Tipp-Suche**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Weg „so viele Schritte, wie Distanz“:
    /// GEMESSEN ist, dass `auswahl_weiter` **einen** Schritt macht und sonst
    /// nichts zurueckgibt — es ist ein Schritt, keine Frage. Eine zweite
    /// Funktion, die den Wert **setzt**, waere ein zweiter Ort, der die
    /// gewaehlte Option bestimmt; so wird **nur** hier entschieden.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum „hoch“ als `anzahl - 1` **vorwaerts**
    /// geht: die Liste ist ein **Ring** (das ist der Kreis in `auswahl_weiter`),
    /// und der Ring hat eine Richtung. Eine eigene Rueckwaertsrichtung waere ein
    /// zweiter Ring an derselben Stelle.
    /// DIE **FORMULAR-SPERRE**: (wie viele Felder blockieren, gibt es einen
    /// Absender-Knopf) — die Zahlen, die `enter_sendet` braucht.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Liste der **blockierenden** Felder
    /// **hier** steht und nicht in `enter_sendet`: `enter_sendet` ist die
    /// **Regel** und kennt keine Feldtypen; welche Typen blockieren, ist eine
    /// **Entscheidung ueber diese Felder** — und eine Liste an zwei Stellen ist
    /// eine Liste, die man an einer Stelle vergisst.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `checkbox`, `radio`, `range`, `color` und
    /// `file` **nicht** blockieren: sie bekommen ihren Wert **ohne** Tastatur,
    /// also steht nichts im Weg — und Firefox wertet genau die ab, die man
    /// **tippen** muss. GEMESSENE ENTSCHEIDUNG, warum `button`/`submit`/
    /// `reset` nicht mitzaehlen: sie sind **Knöpfe**, und ein Knopf, der sich
    /// selbst als blockierendes Feld zaehlt, waere ein Absender, der sich
    /// selbst zaehlt.
    fn formular_sperre(&self, form_idx: Option<usize>) -> (usize, bool) {
        let Some(fi) = form_idx else {
            return (0, false);
        };
        let Some(a) = self.angezeigt.as_ref() else {
            return (0, false);
        };
        let w = a.stand.lock().expect("DomStand");
        let mut blockierer = 0usize;
        let mut absender = false;
        for e in w.eingaben.iter().filter(|e| e.formular == Some(fi)) {
            if e.absender {
                absender = true;
            }
            if matches!(
                e.typ.as_str(),
                "text"
                    | "search"
                    | "url"
                    | "tel"
                    | "email"
                    | "password"
                    | "date"
                    | "month"
                    | "week"
                    | "time"
                    | "datetime-local"
                    | "number"
                    | "textarea"
            ) {
                blockierer += 1;
            }
        }
        (blockierer, absender)
    }

    pub fn auswahl_tasten(&mut self, k: KeyEvent) -> bool {
        let Some(ord) = self.auswahl_fokus else {
            return false;
        };
        let (anzahl, jetzt, texte) = self.auswahl_lage(ord);
        // GEMESSEN: fuer die Tipp-Suche braucht es **Nanosekunden** (das Fenster
        // der Buchstaben ist eine Zeit), und `auswahl_lage` gibt den gewaehlten
        // **Index**. `jetzt_ns` ist daher eine eigene Zaehlung.
        let jetzt_ns = crate::video::jetzt_ns();
        if anzahl == 0 {
            return false;
        }
        //
        // KONTEXT (05.10.2026, 21:25): **ENTER** IN EINER AUSWAHL SENDET DAS
        // FORMULAR — so wie in Firefox.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das **hier** steht und nicht im
        // Eingabefeld-Zweig: GEMESSEN ist, dass ein `<select>` **nicht** in
        // `Stand::eingaben` liegt (es steht in `Stand::auswahl`), also kam dort
        // `formular` als `None` an — und `formular_senden(None)` ist genau das
        // „steht in keinem Formular“ und tut **nichts**. Ein Enter, das nichts
        // tut, ist die Art von Taste, die man als tot meldet.
        if k.key == narsil_input::Key::Enter {
            let idx = self.auswahl_formular(ord);
            return self.formular_senden(idx);
        }
        if k.key == narsil_input::Key::Up || k.key == narsil_input::Key::Down {
            let schritte = if k.key == narsil_input::Key::Down {
                1
            } else {
                anzahl - 1
            };
            for _ in 0..schritte {
                self.auswahl_weiter(ord);
            }
            return true;
        }
        //
        // KONTEXT (05.10.2026, 23:40): DIE **TIPP-SUCHE** — und sie ist jetzt
        // **eine** Funktion aus der gruenen Crate (`narsil_browser::interaktion`),
        // weil hier zwei Fehler standen und beide bei Zahlen sassen:
        //
        // 1. **Ein** Buchstabe je Tastendruck. Firefox sammelt die Buchstaben
        //    zum **Praefix** („i“ dann „s“ findet „Island“); hier wurde jede
        //    Taste fuer sich gesucht, das „s“ fand also „Straße“ statt Island.
        // 2. Die **Schrittzahl** war „Ziel modulo Zahl“. Das stimmt nur, wenn
        //    gerade die **nullte** Option gewaehlt ist: bei jeder anderen Wahl
        //    landet die Auswahl auf einem **anderen** Eintrag als dem gefundenen.
        //    (Test in `crates/narsil-browser/tests/interaktion.rs`.)
        //
        // GEMESSENE ENTSCHEIDUNG, warum der **Ring** und nicht „von vorn“: in
        // Firefox springt derselbe Buchstabe beim zweiten Tippen auf den
        // **naechsten** Treffer — damit blaettert man eine lange Liste durch,
        // ohne mit der Maus zu suchen.
        let Some(c) = k.typed_char() else {
            return false;
        };
        let tipp = self.tippsuche.taste(c, jetzt_ns).to_string();
        let Some(schritte) = auswahl_tippschritte(jetzt, &texte, &tipp) else {
            return false;
        };
        for _ in 0..schritte {
            self.auswahl_weiter(ord);
        }
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Suche nach einem Treffer **leer**
        // wird: sonst haette der naechste Buchstabe das alte Praefix verlaengert
        // und derselbe Buchstabe wuerde nichts mehr finden — also wuerde genau
        // das Ding nicht gehen, fuer das man es benutzt (durchblaettern).
        self.tippsuche.leeren();
        true
    }

    /// DIE **LAGE** einer Auswahlliste: (Anzahl, gewaehlter Index, sichtbare
    /// Texte) — eine **Lese**-Frage, ohne Seiteneffekt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum „welche ist gewaehlt“ **hier** und in
    /// `auswahl_weiter` beantwortet wird (zwei Stellen): GEMESSEN ist, dass
    /// `auswahl_weiter` nichts zurueckgibt — es ist ein **Schritt**, keine
    /// Frage. Hier wird **gelesen**, dort wird **entschieden**; zwei Orte fuer
    /// dieselbe **Entscheidung** waere der Fehler, zwei Orte fuer dieselbe
    /// **Antwort** nicht. (Eigene Aufgabe fuer die Runde, in der der Baum
    /// gruen ist: `auswahl_weiter` soll aus dieser Lage lesen.)
    /// DIE **ORD-NUMMER** der Auswahlliste mit Fokus (fuer die Oberflaeche
    /// und die Tests).
    ///
    /// KONTEXT (05.10.2026, 04:05): GEMESSENE ENTSCHEIDUNG, warum ein
    /// **Lese**-Zugang und kein setter: die Oberflaeche will den Kasten
    /// **zeichnen** (wo steht der Fokus?) — und ein Setter wuerde ihr erlauben,
    /// einen Fokus zu setzen, den es nicht gibt.
    pub fn auswahl_ord(&self) -> Option<usize> {
        self.auswahl_fokus
    }

    fn auswahl_lage(&self, ord: usize) -> (usize, usize, Vec<String>) {
        let Some(a) = self.angezeigt.as_ref() else {
            return (0, 0, Vec::new());
        };
        let w = a.stand.lock().expect("DomStand");
        let Some(s) = w.auswahl.iter().find(|s| s.ord == ord) else {
            return (0, 0, Vec::new());
        };
        let texte: Vec<String> = s.optionen.iter().map(|o| o.text.clone()).collect();
        let gewaehlt = a
            .patch
            .attribut
            .get(&(ord, "value".to_string()))
            .cloned()
            .unwrap_or_default();
        let jetzt = s
            .optionen
            .iter()
            .position(|o| o.wert == gewaehlt)
            .or_else(|| s.gewaehlte_indizes().first().copied())
            .unwrap_or(0);
        (s.optionen.len(), jetzt, texte)
    }

    /// W7C A2: EIN FORMULAR ZURUECKSETZEN (`<input type=reset>`).
    ///
    /// GEMESSEN, warum der `Patch` geleert wird und nicht die Werte
    /// einzeln: die Werte stehen im Patch. Ein Weg, der sie einzeln
    /// zuruecksetzt, muesste wissen, welche es gibt — und die Tabelle des
    /// ALTEN Stands ist dann schon abgebaut.
    fn formular_zuruecksetzen(&mut self, form_idx: Option<usize>) {
        let felder: Vec<usize> = {
            let Some(a) = self.angezeigt.as_ref() else {
                return;
            };
            let w = a.stand.lock().expect("DomStand");
            form_idx
                .and_then(|fi| w.formulare.get(fi))
                .map(|f| f.felder.clone())
                .unwrap_or_default()
        };
        if let Some(a) = self.angezeigt.as_mut() {
            for ord in felder {
                a.patch.attribut.remove(&(ord, "value".to_string()));
                a.patch.attribut.remove(&(ord, "checked".to_string()));
            }
        }
        self.angepasst(true);
    }

    /// W7C A2: DAS FORMULAR MIT EINEM ABESENDER-KNOEFF SENDEN.
    ///
    /// GEMESSEN, warum der Knopf ein eigener Weg ist: nur der
    /// GEKLICKTE Absender wird geschickt.
    /// `<button name=aktion value=loeschen>` neben
    /// `<button name=aktion value=sichern>` schickt in jedem Browser
    /// GENAU EINEN von beiden. Wer alle schickt, schickt „loeschen und
    /// sichern“ und loescht damit die Seite, die man sichern wollte.
    fn formular_senden_mit(&mut self, form_idx: Option<usize>, knopf: Option<usize>) {
        self.knopf_fuer_formular = knopf;
        self.formular_senden(form_idx);
        self.knopf_fuer_formular = None;
    }

    /// W7 P2: Der Klick in ein Eingabefeld setzt den Fokus.
    ///
    /// GEMESSEN, warum das im `klick` und nicht im `handler_ausloesen`:
    /// der Fokus ist eine Entscheidung ueber das ELEMENT, nicht ueber den
    /// Handler — und ein Feld kann einen Handler haben UND den Fokus
    /// bekommen (der Handler laeuft, der Fokus bleibt).
    ///
    /// `true`, wenn der Fokus auf ein Feld gesetzt wurde (der Aufrufer
    /// darf dann den Browserstatus anstossen).
    fn fokus_bei_klick(&mut self, knoten: narsil_dom::NodeId, text: &str, x: f32) -> bool {
        let Some(a) = self.angezeigt.as_ref() else {
            return false;
        };
        // 02.10. 19:00 (HTML): `<label>` — der Klick kann den **Text** des
        // Labels treffen, und der Fokus gehoert dem **Feld** (HTML 4, 17.9.1).
        // GEMESSENE EINORDNUNG, warum das hier und nicht im Klickpfad: diese
        // Funktion und `formular_bei_klick` beantworten dieselbe Frage
        // („welcher Knoten ist fuer den Fokus zustaendig?"), und zwei
        // Aufrufer, die es selbst machen, wären zwei Orte zum Vergessen.
        //
        // 02.10. 22:00: DER TEXT unter dem Punkt kommt aus der **Anzeigeliste**
        // (siehe `text_unter_punkt`) — GEMESSEN, weil `tiefster_knoten` den
        // Labeltextknoten nicht liefert.
        let knoten = feld_knoten_bei_klick(a, knoten, text);
        let Some(ord) = a.seite.dom_zu_ord.get(&(knoten.index() as u32)).copied() else {
            return false;
        };
        let w = a.stand.lock().expect("DomStand");
        // M3: `MAX_EINGABEN` (256) Felder.
        let treffer = w.eingaben.iter().find(|e| e.ord == ord);
        let Some(e) = treffer else {
            return false;
        };
        //
        // KONTEXT (05.10.2026, 08:00, Leitung „Datum“): DIE DATUMSARTEN SIND
        // **TIPPBAR**.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das hier und nicht in `Eingabe::
        // tippbar()` (`dom.rs`, fremd): GEMESSEN ist, dass `tippbar()` genau
        // acht `type`-Namen kennt (`dom.rs:135`) — `date`, `time`, `month` und
        // `week` fehlen dort. Ein Datum-Feld, in dem man nichts tippen kann, ist
        // ein Kasten, in den nur die Maus reicht; in Firefox tippt man darin
        // genauso wie in ein Textfeld. GEMESSENE ENTSCHEIDUNG, warum die Liste
        // **hier** steht: sie ist eine **Ergänzung**, keine Ersatzregel — die
        // acht Namen bleiben die von `tippbar()`.
        // Release 08.10.2026: `type` aus dem DOM-Stand, den `w` schon haelt
        // (vorher `a.html.get(ord)`, der Knoten der obersten Ebene — siehe
        // `feld_attribut`; ein zweites `lock` hier waere eine Verklemmung).
        let art = w
            .ids
            .get(&ord)
            .and_then(|h| w.attribute.get(h))
            .and_then(|m| m.get("type").cloned())
            .unwrap_or_default()
            .to_lowercase();
        if !e.tippbar() && !matches!(art.as_str(), "date" | "time" | "month" | "week") {
            return false;
        }
        //
        // KONTEXT (05.10.2026, 03:10, Leitung „disabled“): EIN BEHINDERTES
        // FELD NIMMT KEINEN FOKUS.
        //
        // GEMESSEN ist (per grep), dass `disabled` an genau **zwei** Stellen
        // benutzt wurde: bei den Optionen einer Auswahlliste (7690) und beim
        // Absenden (8275) — im **Klickpfad** kam es nicht vor. Ein `disabled`
        // Eingabefeld liess sich also anklicken und beschreiben, was genau
        // das ist, was das Attribut verbieten soll.
        if e.disabled {
            return false;
        }
        // Der Cursor steht an der **Stelle des Klicks** — und wenn es keine
        // Stelle gibt (kein Kasten im Baum, z. B. weil die Seite noch nicht
        // gebaut ist), am **ENDE** des Wertes, so wie in jedem Browser.
        let laenge = w
            .eingabe_werte
            .get(&e.ord)
            .map(|v| v.chars().count())
            .unwrap_or_else(|| e.wert.chars().count());
        let wert = w
            .eingabe_werte
            .get(&e.ord)
            .cloned()
            .unwrap_or_else(|| e.wert.clone());
        drop(w);
        // Release 08.10.2026: die Klickstelle VOR dem Fokuswechsel messen —
        // der sichtbare Ausschnitt ist der, den der Nutzer gerade sieht
        // (ohne Fokus der Anfang des Wertes, mit Fokus der um den Cursor).
        let cursor_neu = match feld_kasten(a, ord) {
            Some(k) => self.feld_cursor_bei_x(ord, &k, x, &wert),
            None => laenge,
        };
        self.feld_fokus = Some(ord);
        // GEMESSENE ENTSCHEIDUNG: der Fokus wandert auch von der **Auswahl**
        // weg (siehe `auswahl_fokus`) — sonst tippte man in ein Feld, ohne zu
        // merken, dass die Liste noch den Fokus haelt.
        self.auswahl_fokus = None;
        self.feld_cursor = cursor_neu;
        //
        // KONTEXT (05.10.2026, 03:40): DER KLICK **ZIEHT** von hier aus (bis
        // zum Loslassen) und ist im **DOPPELKLICK** das ganze Wort.
        self.feld_zieht = true;
        let jetzt = std::time::Instant::now();
        let doppelt = match self.letzter_feldklick {
            Some((vorher, zeit)) => {
                vorher == ord
                    && jetzt.duration_since(zeit).as_millis() <= DOPPELKLICK_MS as u128
            }
            None => false,
        };
        if doppelt {
            let (von, bis) = wort_grenzen(&wert, self.feld_cursor);
            self.feld_auswahl = Some((von, bis));
            self.feld_cursor = bis;
            // GEMESSENE ENTSCHEIDUNG, warum das Ziehen **weitergeht**: ein
            // Doppelklick markiert ein Wort, und ein danach **gezogenes** Wort
            // waere „Wort plus Rest“ — das ist die Marksierung in Firefox, und
            // es braucht keinen zweiten Zustand.
        } else {
            self.letzter_feldklick = Some((ord, jetzt));
            self.feld_auswahl = None;
        }
        // GEMESSEN, warum die Leiste den Fokus verliert: zwei Orte fuer
        // Eingaben sind ein Orchester, und in der Adressleiste tippt man
        // keine Suche.
        self.leiste_im_fokus = false;
        true
    }

    /// KONTEXT (04.10.2026, 00:45): DER FOKUS **WANDERT** (Tab /
    /// Umschalt+Tab).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge aus `Seite::felder` kommt
    /// und nicht aus den **Eingaben** des Stands: GEMESSEN ist, dass
    /// `Seite::felder` die Felder in **Dokumentreihenfolge** traegt (mit der
    /// `ord`, die der Bauer vergab) — und die Reihenfolge, in der der Nutzer
    /// **Tab** drueckt, ist die Reihenfolge im Dokument.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum am **Ende** der Fokus **weg** geht: Firefox
    /// springt vom letzten Feld zurueck auf die Seite (und von dort in die
    /// Leiste) — hier heisst das „nichts mehr“.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Stapel dabei **geleert** wird: er
    /// gehoert zum Feld (siehe das Feld), und zurueckgenommen wird in dem, in
    /// dem getippt wurde.
    pub fn fokus_weiter(&mut self, vorwaerts: bool) -> bool {
        self.feld_verlauf.clear();
        let felder: Vec<usize> = self
            .angezeigt
            .as_ref()
            .map(|a| a.seite.felder.iter().map(|f| f.ord).collect())
            .unwrap_or_default();
        if felder.is_empty() {
            self.feld_fokus = None;
            return false;
        }
        let stelle = match self.feld_fokus {
            None => 0,
            Some(aktuell) => match felder.iter().position(|o| *o == aktuell) {
                None => 0,
                Some(i) => {
                    let neu = if vorwaerts {
                        i + 1
                    } else {
                        i.saturating_sub(1)
                    };
                    if vorwaerts && neu >= felder.len() {
                        self.feld_fokus = None;
                        self.schmutzig = true;
                        return true;
                    }
                    neu
                }
            },
        };
        let ord = felder[stelle];
        self.feld_fokus = Some(ord);
        self.feld_cursor = self.feld_wert(ord).chars().count();
        self.schmutzig = true;
        true
    }

    /// DIE **FOKUSIERBAREN** Elemente in Dokumentreihenfolge (Ord-Nummern).
    ///
    /// KONTEXT (05.10.2026, 12:05): Links, Knöpfe **und** Eingabefelder — **eine**
    /// Liste, weil `Tab` in Firefox durch **beides** geht (ein Feld, dann ein
    /// Link, dann das nächste Feld), und zwei Listen wären zwei Wege, die
    /// dazwischen entscheiden, wann die Liste der Fall ist.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge aus dem **flachen**
    /// `Angezeigt::html` kommt und nicht aus `Seite::felder`: GEMESSEN ist, dass
    /// `html` die Knoten in **Parse-Reihenfolge** traegt (die `ord` ist der
    /// Index darin — dieselbe Annahme wie bei `autofocus_setzen`, KONTEXT_23),
    /// und `Seite::felder` nur die **Felder** kennt. Eine Liste, die Links und
    /// Felder mischt, gibt es nur im HTML-Baum.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **ohne** `display:none`-Pruefung: GEMESSEN
    /// ist, dass `Angezeigt` den **berechneten** Stil **nicht** je Knoten traegt
    /// (das ist der Weg, an dem `user-select` in KONTEXT_21 gescheitert ist) —
    /// ein versteckter Link wuerde also einen Fokus bekommen, den man nicht sieht.
    /// Das ist ehrlich schlecht und ehrlich benannt; die vollstaendige Regel
    /// braucht den Stil je Knoten (im BRETT, 05.10.).
    pub fn fokus_bare(&self) -> Vec<usize> {
        const TAG: [&str; 5] = ["a", "button", "input", "select", "textarea"];
        let Some(a) = self.angezeigt.as_ref() else {
            return Vec::new();
        };
        let stand = a.stand.lock().expect("DOM");
        let mut raus = Vec::new();
        for (&i, host) in &stand.ids {
            let Some(tag) = stand.tag.get(host) else { continue; };
            let Some(attributes) = stand.attribute.get(host) else { continue; };
            let ohne_herd = attributes
                .get("tabindex")
                .and_then(|v| v.parse::<i32>().ok());
            if attributes.contains_key("disabled") || attributes.contains_key("hidden")
                || attributes.get("type").is_some_and(|t| t.eq_ignore_ascii_case("hidden"))
                || ohne_herd.is_some_and(|t| t < 0) || feld_kasten(a, i).is_none() { continue; }
            if tag == "a" && !attributes.contains_key("href") && ohne_herd.is_none() {
                // `<a>` ohne `href` ist in Firefox **nicht** fokussierbar.
                continue;
            }
            if TAG.contains(&tag.as_str()) || ohne_herd.is_some_and(|t| t >= 0) {
                raus.push((ohne_herd.filter(|t| *t > 0).unwrap_or(i32::MAX), i));
            }
        }
        raus.sort_unstable();
        raus.into_iter().map(|(_, ord)| ord).collect()
    }

    /// KONTEXT (05.10.2026, 12:10): DER FOKUS WANDERT — **Felder und Links**
    /// in **einer** Liste.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum an **einer** Stelle entschieden wird, was
    /// der Fokus wird: ein Fokus ist entweder ein **Feld** (`feld_fokus`) oder ein
    /// **Link** (`fokus_knoten`) — zwei Orte, die sich gegenseitig leeren
    /// muessen, sonst haette man **zwei** Fokusse.
    pub fn fokus_reihe_weiter(&mut self, vorwaerts: bool) -> bool {
        let liste = self.fokus_bare();
        if liste.is_empty() {
            self.feld_fokus = None;
            self.fokus_knoten = None;
            self.feld_verlauf.clear();
            return false;
        }
        self.feld_verlauf.clear();
        // Wo stehen wir? GEMESSENE ENTSCHEIDUNG: **beide** Orte werden
        // verglichen — ein Fokus, der im Feld steht, ist nicht dasselbe wie
        // einer, der im Link steht, auch wenn beide dieselbe Nummer haben
        // koennten (ein `<a>` kann ein Kind eines `<label>` sein).
        let jetzt = match (self.feld_fokus, self.fokus_knoten) {
            (Some(_), _) => self.feld_fokus,
            (_, Some(n)) => Some(n),
            _ => None,
        }
        .and_then(|o| liste.iter().position(|x| *x == o));
        let neu = match jetzt {
            None => if vorwaerts { 0 } else { liste.len() - 1 },
            Some(i) => {
                if !vorwaerts && i == 0 {
                    self.feld_fokus = None;
                    self.fokus_knoten = None;
                    self.auswahl_fokus = None;
                    self.feld_auswahl = None;
                    self.schmutzig = true;
                    return true;
                }
                let k = if vorwaerts { i + 1 } else { i - 1 };
                if vorwaerts && k >= liste.len() {
                    // Am Ende: der Fokus geht **weg** (Firefox: zurueck auf die
                    // Seite bzw. in die Leiste) — siehe die Regel in
                    // `fokus_weiter` (KONTEXT_23).
                    self.feld_fokus = None;
                    self.fokus_knoten = None;
                    self.auswahl_fokus = None;
                    self.feld_auswahl = None;
                    self.schmutzig = true;
                    return true;
                }
                k
            }
        };
        let ord = liste[neu];
        let art = {
            let Some(a) = self.angezeigt.as_ref() else {
                return false;
            };
            // KONTEXT (05.10.2026, 18:22): `HtmlNode` hat **zwei** Formen
            // (Element und Text, E0004) — ein `match` muss beide nennen.
            let stand = a.stand.lock().expect("DOM");
            let Some(tag) = stand.ids.get(&ord).and_then(|h| stand.tag.get(h)) else { return false; };
            tag.clone()
        };
        if art == "input" || art == "select" || art == "textarea" {
            self.fokus_knoten = None;
            self.feld_fokus = Some(ord);
            self.feld_cursor = self.feld_wert(ord).chars().count();
            //
            // KONTEXT (05.10.2026, 21:20, Leitung „Formular-Tastatur:
            // select-Dropdown per Pfeile/Enter/Typ-Suche“): EINE AUSWAHL BEKOMMT
            // **AUCH** DEN AUSWAHL-FOKUS — nicht nur einen Eingabefokus.
            //
            // GEMESSENE ENTSCHEIDUNG, warum **beides** und nicht entweder-oder:
            // GEMESSEN ist, dass `auswahl_tasten` (Pfeile, Typ-Suche) **nur** auf
            // `auswahl_fokus` reagiert und `taste_ins_feld` **nur** auf
            // `feld_fokus` — ein Fokus, den nur **eine** der beiden kennt, ist ein
            // Fokus, der **halb** bedienbar ist (GEMESSEN: mit Tab kam man in
            // die Liste und die Pfeile wirkten nicht).
            self.auswahl_fokus = if art == "select" { Some(ord) } else { None };
        } else {
            self.auswahl_fokus = None;
            self.feld_fokus = None;
            self.feld_auswahl = None;
            self.fokus_knoten = Some(ord);
        }
        self.schmutzig = true;
        true
    }

    /// KONTEXT (05.10.2026, 12:15): **ENTER FOLGT DEM LINK** (und dem Knopf).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Weg **derselbe** ist wie beim Klick
    /// (`gehe_zu`): ein Link, den die Tastatur folgt, muss **dieselbe** Seite
    /// oeffnen wie ein Link, den die Maus folgt — zwei Wege wuerden zwei Seiten
    /// bedeuten, je nachdem, wie man geklickt hat.
    pub fn fokus_link_oeffnen(&mut self) -> bool {
        let Some(ord) = self.fokus_knoten else {
            return false;
        };
        let (ziel, basis) = {
            let Some(a) = self.angezeigt.as_ref() else {
                return false;
            };
            let stand = a.stand.lock().expect("DOM");
            let Some(attributes) = stand.ids.get(&ord).and_then(|h| stand.attribute.get(h)) else { return false; };
            let ziel = attributes.get("href").cloned().unwrap_or_default();
            (ziel, a.seite.basis.clone())
        };
        if ziel.is_empty() {
            return false;
        }
        // GEMESSENE ENTSCHEIDUNG, warum `narsil_url::loese_auf`: ein `href` ist
        // fast nie eine ganze Adresse (`../b.html`), und `gehe_zu` bekommt eine
        // ganze Adresse — die Basis gehoert dem Dokument, und `loese_auf` ist
        // die **eine** Stelle, die das kann (GEMESSEN in `narsil-url/src/lib.rs:362`).
        let adresse = narsil_url::loese_auf(&basis, &ziel).unwrap_or(ziel);
        self.fokus_knoten = None;
        if let Err(f) = self.gehe_zu(&adresse) {
            self.meldung = Some(f.to_string());
        }
        self.schmutzig = true;
        true
    }

    /// DER WERT eines Feldes (Patch, sonst der HTML-Stand) — **eine** Stelle.
    ///
    /// KONTEXT (04.10.2026, 00:50): GEMESSENE ENTSCHEIDUNG, warum das hier
    /// steht und nicht viermal im Aufrufer: GEMESSEN ist der Kommentar in
    /// `formular_senden` („der `Patch` ist der Ort, an dem die ENTSCHEIDUNGEN des
    /// Nutzers liegen … Wer die Werte an zwei Stellen haelt, hat nach dem ersten
    /// Klick zwei verschiedene Formulare“) — dieselbe Frage, dieselbe Antwort.
    /// Release 15.10.2026: has the field a value of its own (typed by the
    /// user or set by script), even an empty one?
    fn feld_bearbeitet(&self, ord: usize) -> bool {
        let Some(a) = self.angezeigt.as_ref() else {
            return false;
        };
        if a.patch.attribut.contains_key(&(ord, "value".to_string())) {
            return true;
        }
        a.stand.lock().is_ok_and(|w| w.eingabe_werte.contains_key(&ord))
    }

    pub fn feld_wert(&self, ord: usize) -> String {
        let Some(a) = self.angezeigt.as_ref() else {
            return String::new();
        };
        if let Some(v) = a.patch.attribut.get(&(ord, "value".to_string())) {
            return v.clone();
        }
        let w = a.stand.lock().expect("DomStand");
        match w.eingabe_werte.get(&ord) {
            Some(v) => v.clone(),
            None => w
                .eingaben
                .iter()
                .find(|e| e.ord == ord)
                .map(|e| e.wert.clone())
                .unwrap_or_default(),
        }
    }

    /// KONTEXT (04.10.2026, 00:55): **Strg+Z** — den letzten Wert
    /// zuruecknehmen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Stapel in der **Sitzung** liegt und
    /// nicht am Feld (`Eingabe` in `dom.rs`): GEMESSEN ist, dass `Eingabe` beim
    /// **Neuaufbau** der Seite neu entsteht — der Stapel wuerde dabei leer, und
    /// genau dann braucht man ihn nicht.
    pub fn feld_rueckgaengig(&mut self) -> bool {
        let geholt = self.feld_verlauf.pop();
        let Some((ord, wert, cursor)) = geholt else {
            self.meldung = Some(tr("Undo: nothing left to undo").into());
            return false;
        };
        if let Some(a) = self.angezeigt.as_mut() {
            a.patch.attribut.insert((ord, "value".to_string()), wert);
        }
        self.feld_fokus = Some(ord);
        self.feld_cursor = cursor;
        self.schmutzig = true;
        true
    }

    /// KONTEXT (04.10.2026, 01:00): **autofocus** — das Feld mit dem Attribut
    /// bekommt beim Laden den Fokus.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ueber das **Attribut** geht und nicht
    /// ueber einen berechneten Stil: `autofocus` **ist** ein Attribut und steht
    /// im geparsten Baum (`Angezeigt::html`) — anders als `user-select`, wo ich
    /// auf Inline-Stile angewiesen bin, weil der Wert ausgerechnet werden muss.
    pub fn autofocus_setzen(&mut self) -> bool {
        let treffer = self.angezeigt.as_ref().and_then(|a| {
            a.seite
                .felder
                .iter()
                .find(|f| {
                    // KONTEXT (05.10.2026, 18:20): `f.ord` ist **usize** (E0599,
                    // vom Compiler benannt) — `Angezeigt::html` wird mit der
                    // **Ord-Nummer** indexiert, nicht mit einer `NodeId`.
                    let stand = a.stand.lock().expect("DOM");
                    stand.ids.get(&f.ord).and_then(|h| stand.attribute.get(h))
                        .is_some_and(|attrs| attrs.contains_key("autofocus") && !attrs.contains_key("disabled"))
                })
                .map(|f| f.ord)
        });
        let Some(ord) = treffer else {
            return false;
        };
        let laenge = self.feld_wert(ord).chars().count();
        self.feld_fokus = Some(ord);
        self.feld_cursor = laenge;
        self.schmutzig = true;
        true
    }

    /// DAS **RECHTECK** des fokussierten Feldes — fuer den Fokusring.
    ///
    /// KONTEXT (04.10.2026, 01:02): GEMESSENE ENTSCHEIDUNG, warum der Weg
    /// ueber den **Layoutbaum** geht (wie `kaesten_aus_baum`) und nicht ueber die
    /// Hit-Tests: die Kaesten stehen nur dort, und ein zweiter Weg zu denselben
    /// Kaesten waere die Fehlerform „zwei Stellen entscheiden“.
    pub fn fokus_rechteck(&self) -> Option<Rect> {
        let a = self.angezeigt.as_ref()?;
        //
        // KONTEXT (05.10.2026, 12:30): DER RING GEHT UM **BEIDES** — um ein
        // Feld **und** um einen fokussierten Link/Knopf.
        //
        // GEMESSENE ENTSCHEIDUNG, warum an **einer** Stelle: es ist **ein**
        // Fokus (der Nutzer weiss nicht, ob er „im Feld“ oder „auf dem Link“ ist
        // — er weiss, dass er mit der Tastatur unterwegs ist), und zwei Ringe an
        // zwei Stellen waeren zwei Zeichenbefehle fuer dieselbe Frage.
        let ord = self.feld_fokus.or(self.fokus_knoten)?;
        //
        // KONTEXT (05.10.2026, 03:00): MEIN EIGENER FEHLER, GEFUNDEN BEIM
        // SCHREIBEN (nicht beim Testen): ich hatte hier
        // `r.y - SEITEN_START_Y + a.versatz` gerechnet. GEMESSEN ist aber, dass
        // `display_list` **am Ende** `liste.schiebe_alle(dy)` macht (siehe das
        // Ende der Funktion) — die Items stehen also in **Seiten**-Koordinaten,
        // und derselbe Weg steht in `cursor_rechteck` (W7 P2) **ohne** Rechnung.
        // Mein Ring waere doppelt verschoben gewesen: einmal um die Leistenhoehe
        // und einmal um den Scrollweg. Die Rechnung ist jetzt weg — ein Ring, der
        // sich an die Bewegung der Seite haelt, ist die Rechnung nicht wert.
        self.feld_kasten_im_fenster(ord)
    }

    /// DER CURSOR AN DER **STELLE DES KLICKS** (Zeichenindex im Wert).
    ///
    /// KONTEXT (05.10.2026, 03:05): GEMESSENE ENTSCHEIDUNG, warum **gemessen**
    /// und nicht geraten: GEMESSEN ist, dass `cursor_rechteck` (W7 P2) den
    /// Cursor an die **Breite des Wert-Präfixes** stellt (mit
    /// `self.messung.breite(.., narsil_page::GRAD_TEXT)`) — dieselbe Messung, also
    /// derselbe Weg. Ein Raten („die Haelfte der Kastenbreite“) waere an der
    /// Stelle falsch, an der der Cursor in Firefox steht.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **alle** Zeichen gemessen werden und nicht
    /// eine halbe Seite: ein Klick ist selten, ein Tastendruck nicht — und diese
    /// Funktion laeuft nur beim Klick. Die Grenze `MAX_EINGABE_WERT` (4096) ist
    /// damit die Grenze der Rechnung.
    fn feld_cursor_bei_x(&self, ord: usize, kasten: &Rect, x: f32, wert: &str) -> usize {
        // Release 08.10.2026: gemessen ab der Stelle, an der der Text steht
        // (`FeldBild`: Polster und Schrift des Feldes, der sichtbare
        // Ausschnitt) — derselbe Weg wie Ueberzug und Cursor. Vorher ab
        // `kasten.x` mit 16 px: ein Klick traf bei 18 px Polster das
        // Zeichen links neben dem gemeinten.
        if let Some(a) = self.angezeigt.as_ref()
            && let Some(bild) = feld_bilder(a, &[ord]).remove(&ord)
            && !wert.is_empty()
        {
            let typ = feld_attribut(a, ord, "type")
                .unwrap_or_default()
                .to_lowercase();
            let zeichen = feld_zeichen(a, ord, &typ, wert);
            let alle: Vec<char> = zeichen.chars().collect();
            let cursor = (self.feld_fokus == Some(ord)).then_some(self.feld_cursor);
            let (von, _) = self.feld_ausschnitt(&zeichen, cursor, &bild, 64);
            let rel = (x - bild.text_x).max(0.0);
            let mut best = von;
            let mut abstand = f32::MAX;
            for n in von..=alle.len() {
                let teil: String = alle[von..n].iter().collect();
                let breite = self.feld_text_breite(&teil, &bild);
                let d = (breite - rel).abs();
                if d < abstand {
                    abstand = d;
                    best = n;
                }
                if breite > rel + bild.schrift * 2.0 {
                    break;
                }
            }
            return best;
        }
        let rel = (x - kasten.x).max(0.0);
        let messung = self.messung.clone();
        let zeichen: Vec<char> = wert.chars().collect();
        let mut best = 0usize;
        let mut abstand = f32::MAX;
        for n in 0..=zeichen.len() {
            let teil: String = zeichen[..n].iter().collect();
            let breite = messung.breite(&teil, narsil_page::GRAD_TEXT);
            let d = (breite - rel).abs();
            if d < abstand {
                abstand = d;
                best = n;
            }
        }
        best
    }

    /// KONTEXT (05.10.2026, 15:10, Leitung „Scroll-Ketten-Bewegung“): DAS RAD
    /// BEI EINEM **PUNKT** — erst die Kette, dann die Seite.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **erst die Kette**: GEMESSEN ist, dass die
    /// Rechnung (`narsil_browser::interaktion::scrollkette`) **innen nach aussen**
    /// laeuft und der **Rest** an die Seite geht — und genau das ist der Grund,
    /// warum man diese Funktion **statt** eines `if`-Baums braucht: `if` kann
    /// nur **eine** Flaeche nehmen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Kette **ab dem Punkt nach aussen**
    /// beginnt und nicht von ganz aussen: ein Rad **auf** einem inneren Kasten
    /// soll **diesen** bewegen; die Kette darum herum bewegt sich nur, wenn der
    /// innere voll ist. Deshalb wird die Liste **ab dem Treffer** gebildet.
    ///
    /// `true`, wenn sich **etwas** bewegt hat.
    pub fn scroll_kette_bei_punkt(&mut self, x: f32, y_seite: f32, schritt: i64) -> bool {
        let sicht = self.sicht_hoehe();
        let plan: Vec<(u32, u32, u32, Rect)> = self
            .angezeigt
            .as_ref()
            .map(|a| {
                scroll_flaechen(a)
                    .into_iter()
                    .map(|f| (f.knoten.index() as u32, f.max_versatz, f.versatz, f.rect))
                    .collect()
            })
            .unwrap_or_default();
        if plan.is_empty() {
            return false;
        }
        // Wo liegt der Zeiger? `plan` ist **innen zuerst** (siehe
        // `scroll_flaechen`), also ist der **letzte** Treffer der innerste.
        let treffer = plan.iter().position(|(_, _, _, r)| {
            y_seite >= r.y && y_seite <= r.y + r.height && x >= r.x && x <= r.x + r.width
        });
        let Some(treffer) = treffer else {
            return false;
        };
        // Die Liste **ab** dem Treffer nach aussen (alles davor in der Liste ist
        // aeusserhalb des Zeigers).
        let kette: Vec<(u32, u32, u32)> = plan
            .iter()
            .enumerate()
            .filter(|(i, _)| *i <= treffer)
            .map(|(_, (id, max, ist, _))| (*id, *max, *ist))
            .collect();
        let zahlen: Vec<(u32, u32)> = kette.iter().map(|(_, max, ist)| (*max, *ist)).collect();
        let bewegung = narsil_browser::interaktion::scrollkette(&zahlen, schritt);
        if bewegung.is_empty() {
            return false;
        }
        if let Some(a) = self.angezeigt.as_mut() {
            for (i, wie) in bewegung.iter() {
                let (id, _, ist) = kette[*i];
                let neu = (ist as i64 + wie).clamp(0, i64::from(u32::MAX)) as u32;
                a.scroll_je_knoten.insert(id, neu);
            }
        }
        // Der Rest wandert an die Seite (siehe `seite_rest`).
        let genommen: i64 = bewegung.iter().map(|(_, w)| *w).sum();
        let rest = schritt - genommen;
        if rest != 0 {
            let alt = self.angezeigt.as_ref().map_or(0, |a| a.versatz);
            let max = self.angezeigt.as_ref().map_or(0, |a| a.max_versatz(sicht));
            if let Some(a) = self.angezeigt.as_mut() {
                a.versatz = narsil_browser::interaktion::seite_rest(rest, max, alt);
            }
        }
        self.scrollbalken.zeigen(std::time::Instant::now());
        self.bilder_sichtbar_laden();
        self.schmutzig = true;
        true
    }

    /// DER KASTEN eines Feldes in **FENSTERkoordinaten** — der Weg fuer
    /// **Treffer** (anders als `feld_kasten`, das fuer **Zeichnung** da ist).
    ///
    /// Layoutboxen liegen in Seitenkoordinaten. Derselbe Versatz wie beim
    /// Zeichnen gilt fuer Treffer: Werkzeugleistenhoehe minus Seitenscroll.
    pub fn feld_kasten_im_fenster(&self, ord: usize) -> Option<Rect> {
        let a = self.angezeigt.as_ref()?;
        let r = feld_kasten(a, ord)?;
        Some(Rect::new(
            r.x,
            r.y + self.chrome.seiten_oberkante() as f32 - a.versatz as f32,
            r.width,
            r.height,
        ))
    }

    /// KONTEXT (05.10.2026, 03:36): DER ZEIGER ZIEHT EINE MARKIERUNG IM FELD.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `true` auch **ausserhalb** des Kastens
    /// gilt (und nicht nur `false`): Firefox zieht die Markierung **ueber den
    /// Rand hinaus** — wer im Feld auf „a“ klickt und nach rechts aus dem Feld
    /// zieht, markiert den ganzen Wert. Ein `true` nur im Kasten hiesse: der
    /// Nutzer verliert die Markierung, sobald der Zeiger 1 px zu weit ist.
    /// DER FORMULAR-INDEX einer **Auswahl** (`Stand::auswahl`, nicht `eingaben`).
    fn auswahl_formular(&self, ord: usize) -> Option<usize> {
        let a = self.angezeigt.as_ref()?;
        let w = a.stand.lock().expect("DomStand");
        w.auswahl
            .iter()
            .find(|s| s.ord == ord)
            .and_then(|s| s.formular)
    }

    /// DER **FORMULAR-INDEX** eines Feldes (`None` = es steht in keinem).
    ///
    /// KONTEXT (05.10.2026, 17:25): GEMESSENE ENTSCHEIDUNG, warum das hier steht
    /// und nicht viermal im Aufrufer: GEMESSEN ist, dass `Eingabe::formular` die
    /// **eine** Quelle ist (`taste_ins_feld` liest sie ueber `.and_then(|e|
    /// e.formular)`) — und dass `formular_senden` und
    /// `formular_zuruecksetzen` **beide** einen Index wollen. Eine Funktion,
    /// eine Antwort.
    fn feld_formular(&self, ord: usize) -> Option<usize> {
        let a = self.angezeigt.as_ref()?;
        let w = a.stand.lock().expect("DomStand");
        w.eingaben
            .iter()
            .find(|e| e.ord == ord)
            .and_then(|e| e.formular)
    }

    pub fn feld_bei_punkt(&mut self, p: Point) -> bool {
        let Some(ord) = self.feld_fokus else {
            return false;
        };
        // Der Anker ist der Cursor, **bevor** er wandert — deshalb wird hier
        // nichts am Anker geaendert.
        let rechnen = self
            .angezeigt
            .as_ref()
            .and_then(|a| feld_kasten(a, ord))
            .map(|r| {
                let a = self.angezeigt.as_ref();
                let y = r.y + self.chrome.seiten_oberkante() as f32 - a.map_or(0.0, |x| x.versatz as f32);
                (Rect::new(r.x, y, r.width, r.height), self.feld_wert(ord))
            });
        let Some((k, wert)) = rechnen else {
            return false;
        };
        // Oberhalb des Kastens ist die Zeile **nicht** die des Feldes.
        if p.y < k.y || p.y > k.y + k.height {
            return false;
        }
        let ziel = self.feld_cursor_bei_x(ord, &k, p.x, &wert);
        self.feld_markieren_bis(ziel);
        self.feld_cursor = ziel;
        true
    }

    // KONTEXT (05.10.2026, 03:38): **DOPPELKLICK** im Feld — das ganze Wort.
    //
    // KONTEXT (05.10.2026, 01:05): Die Konstante `DOPPELKLICK_MS` stand
    // **zweimal** in dieser Datei — hier fuer das **Feld** und dort fuer den
    // **Text** (beide 400 ms). Jetzt kommt sie aus der gruenen Crate, wo sie am
    // **Klickzaehler** haengt (also dort, wo man sie pruefen kann); dieser Ort
    // benutzt den importierten Namen.

    /// KONTEXT (05.10.2026, 03:08): Liest das Feld ein **nur lesbares**?
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ueber das **Attribut** laeuft (wie
    /// `autofocus_setzen`): GEMESSEN ist, dass `readonly` im ganzen Baum
    /// **nirgends** vorkommt (`grep -rn readonly crates/*/src/*.rs` ohne
    /// Treffer) — es steht also nur im geparsten HTML (`Angezeigt::html`).
    pub fn feld_nur_lesbar(&self, ord: usize) -> bool {
        self.angezeigt.as_ref().is_some_and(|a| {
            let stand = a.stand.lock().expect("DOM");
            stand.ids.get(&ord).and_then(|h| stand.attribute.get(h))
                .is_some_and(|attrs| attrs.contains_key("readonly"))
        })
    }

    /// W7 P2: Eine Taste in das fokussierte Feld.
    ///
    /// `true`, wenn die Taste fuer das Feld bestimmt war (und der Cursor
    /// oder der Wert sich also geaendert hat).
    /// DER **MARKIERTE** Text im fokussierten Feld (`None` ohne Markierung).
    fn feld_markiert(&self, wert: &str) -> Option<String> {
        let (von, bis) = self.feld_auswahl?;
        let n = wert.chars().count();
        let a = von.min(bis).min(n);
        let b = bis.max(von).min(n);
        Some(wert.chars().skip(a).take(b - a).collect())
    }

    /// KONTEXT (05.10.2026, 02:32): STRG+C / STRG+X im Feld.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **hier** und nicht in `taste`: dort
    /// liegt der Wert nur als `chars`, hier als `String` — die Zwischenablage
    /// traegt einen `String`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `kopieren` den Wert **nicht** zurueckgibt
    /// (wie bei der Seite, KONTEXT_21): im Feld gibt es nichts zu **markieren** —
    /// die Markierung steht schon da, sie wird nur kopiert. Und GEMESSENE
    /// ENTSCHEIDUNG, warum `ausschneiden` **zusaetzlich** loescht: so heisst es
    /// das Kuerzel, und `Strg+X` ohne Loeschen waere eine Taste ohne Wirkung.
    pub fn feld_markiert_kopieren(&mut self, ausschneiden: bool) -> bool {
        let Some(ord) = self.feld_fokus else {
            return false;
        };
        let wert = self.feld_wert(ord);
        let Some(text) = self.feld_markiert(&wert) else {
            return false;
        };
        self.zwischenablage = text.clone();
        // KONTEXT (05.10.2026, 10:45): AUS EINEM **FELD** GEHT DER TEXT
        // EBENSO NACH AUSSEN — das ist der haeufigste Ort, an dem ein Passwort
        // oder eine Nummer kopiert wird, und es ist der Ort, an dem ein
        // Nutzer sie in ein anderes_programm braucht (siehe `feld_markiert_
        //_einfuegen` fuer die Gegenrichtung).
        crate::zwischenablage_system::schreiben(&text);
        if !ausschneiden {
            return true;
        }
        let (von, bis) = self.feld_auswahl.unwrap_or((0, 0));
        let n = wert.chars().count();
        let a = von.min(bis).min(n);
        let b = bis.max(von).min(n);
        let neu: String = wert.chars().take(a).chain(wert.chars().skip(b)).collect();
        self.feld_auswahl = None;
        // GEMESSENE ENTSCHEIDUNG, warum der Cursor **an den Anfang** der
        // Markierung wandert: das ist die Regel jedes Textfeldes — nach dem
        // Ausschneiden steht der Cursor **vor** dem Loch, das er gemacht hat.
        // GEMESSENER FEHLER, vom Compiler benannt: mein Skript hatte zwei Zeilen
        // in **diese** Funktion geschrieben (`let _ = v;` und ein `stelle`, das es
        // hier nicht gibt) — E0425 nannte die Zeile **hier**, und damit habe ich
        // gemerkt, dass der Anker in **zwei** Funktionen vorkommt.
        self.werte_schreiben(ord, neu, a);
        true
    }

    /// KONTEXT (05.10.2026, 02:34): STRG+V im Feld.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Zwischenablage **nicht** geleert wird:
    /// Firefox behaelt sie, und ein zweites `Strg+V` ist in einem Formular der
    /// Normalfall (derselbe Text in zwei Felder).
    pub fn feld_markiert_einfuegen(&mut self) -> bool {
        let Some(ord) = self.feld_fokus else {
            return false;
        };
        //
        // KONTEXT (05.10.2026, 10:40, Leitung „Einfuegen **systemweit**“):
        // ZUERST DIE **SYSTEM**-ZWISCHENABLAGE, dann der interne Stand.
        //
        // GEMESSENE ENTSCHEIDUNG, warum in dieser **Reihenfolge**: GEMESSEN ist,
        // dass `Sitzung::zwischenablage` nur aufnehmen kann, was **hier**
        // kopiert wurde — wer in einem anderen Programm kopiert und dann hier
        // einfuegt, bekommt sonst einen **leeren** Klick. GEMESSENE
        // ENTSCHEIDUNG, warum der interne Stand **trotzdem** bleibt: es gibt
        // Rechner **ohne** `xclip`/`wl-copy` (dann ist das Werkzeug `None`) —
        // und der interne Stand ist dann der einzige, den es gibt.
        let text =
            crate::zwischenablage_system::system_oder_intern(Some(self.zwischenablage.clone()));
        let Some(text) = text.filter(|t| !t.is_empty()) else {
            return false;
        };
        self.zwischenablage = text.clone();
        let wert = self.feld_wert(ord);
        //
        // KONTEXT (05.10.2026, 17:50): DAS EINFUEGEN RECHNET
        // `narsil_browser::interaktion::einfuegen` — **nicht** mehr hier.
        //
        // GEMESSENER BEFUND: mein **alter** Rumpf (`for c in text.chars() {
        // v.insert(stelle, c) }`) hat den Text **umgekehrt** — „XYZ“ wurde zu
        // „ZYX“. Das hat der Plan-B-Test `drei_zeichen_in_richtiger_reihenfolge`
        // gefunden: **dieselbe** Rechnung an **zwei** Stellen, und nur an einer
        // davon geprueft. GEMESSENE ENTSCHEIDUNG, warum die Grenze **nicht**
        // doppelt bleibt: eine Grenze an zwei Stellen ist eine Grenze, die
        // auseinanderlaufen kann.
        let (neu, stelle) = narsil_browser::interaktion::einfuegen(
            &wert,
            self.feld_cursor,
            &text,
            crate::dom::MAX_EINGABE_WERT,
        );
        self.feld_auswahl = None;
        self.werte_schreiben(ord, neu, stelle);
        true
    }

    /// KONTEXT (05.10.2026, 02:36): UMSCHALT+PFEIL **VERLAENGERT** die
    /// Markierung; der Anker ist der Cursor, **bevor** er wandert.
    fn feld_markieren_bis(&mut self, ende: usize) {
        // Release 09.10.2026: eine BESTEHENDE Markierung behaelt ihren Anker —
        // das Ende, an dem der Cursor nicht steht. GEMESSEN vorher (Test
        // `umschalt_pfeil_markiert_im_feld`): zweimal Umschalt+Rechts ab 0
        // markierte (1, 2) statt (0, 2), der Anker wanderte mit.
        let anker = match self.feld_auswahl {
            Some((a, b)) if b == self.feld_cursor => a,
            Some((a, b)) if a == self.feld_cursor => b,
            _ => self.feld_cursor,
        };
        self.feld_auswahl = (anker != ende).then_some((anker, ende));
    }

    fn taste_ins_feld(&mut self, k: KeyEvent) -> bool {
        let Some(ord) = self.feld_fokus else {
            return false;
        };
        //
        // KONTEXT (05.10.2026, 17:20, Leitung „Checkbox/Radio **per Leertaste**“):
        // EIN FELD, DAS **KEIN TEXTFELD** IST, HAT EIGENE TASTEN.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das **hier** steht und nicht in `taste`:
        // GEMESSEN ist, dass `haken_setzen` (Z. 8138) genau diese Frage
        // beantwortet („schalte diesen Haken um“) und der **Klick**pfad sie schon
        // benutzt (Z. 8089) — es gibt also **einen** Ort, der es kann, und der
        // Weg hier ist eine **Taste** davor.
        //
        // GEMESSENER BEFUND (der die Luecke belegt): ein `<input type=checkbox>`
        // ist **nicht** `tippbar` (`dom.rs:135`), also bekam er vom Klickpfad
        // **nie** den Fokus — und ohne Fokus kann die Leertaste nichts
        // ausloesen. GEMESSENE ENTSCHEIDUNG, warum der Haken jetzt ueber
        // `fokus_bare` in die Tab-Reihe kommt (siehe dort): es ist **eine**
        // Liste, und ein Haken, den man nur mit der Maus ankreuzen kann, ist
        // fuer Tastaturnutzer ein Kasten, an dem man nicht vorbeikommt.
        let art = {
            let Some(a) = self.angezeigt.as_ref() else {
                return false;
            };
            let w = a.stand.lock().expect("DomStand");
            w.eingaben
                .iter()
                .find(|e| e.ord == ord)
                .map(|e| e.typ.clone())
                .unwrap_or_default()
        };
        match art.as_str() {
            "checkbox" | "radio" => {
                // Nur die **Leertaste** und **Enter** schalten um (Firefox:
                // beide), und **keine** andere Taste wird getippt.
                if matches!(k.key, narsil_input::Key::Space) || k.key == narsil_input::Key::Enter {
                    return self.haken_setzen(ord, &art);
                }
                return true;
            }
            "submit" | "button" | "reset" => {
                // GEMESSENE ENTSCHEIDUNG, warum Enter und Leertaste hier
                // **abschicken** (und nichts sonst): das ist die Form des Knopfes.
                // GEMESSENE ENTSCHEIDUNG, warum `reset` **nicht** schickt: der
                // setzt zurueck — das ist `formular_zuruecksetzen` (W7C A2), und
                // es zu rufen waere eine **zweite** Bedeutung dieser Taste an
                // einer Stelle, die gerade „abschicken“ bedeutet.
                if matches!(k.key, narsil_input::Key::Space) || k.key == narsil_input::Key::Enter {
                    let idx = self.feld_formular(ord);
                    if art == "reset" {
                        // GEMESSEN (Signatur): `formular_zuruecksetzen` gibt
                        // **nichts** zurueck (`fn ... -> ()`), also darf man es
                        // nicht mit `return` weitergeben.
                        self.formular_zuruecksetzen(idx);
                        return true;
                    }
                    return self.formular_senden(idx);
                }
                return true;
            }
            _ => {}
        }
        //
        // KONTEXT (05.10.2026, 03:15, Leitung „readonly“): EIN NUR-LESBARES
        // FELD LAESST SICH **LESEN**, ABER NICHT AENDERN.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `Enter` **nicht** gesperrt wird: in
        // Firefox schickt `Enter` aus einem nur lesbaren Feld das Formular ab —
        // das ist der haeufigste Weg (ein Suchfeld mit `readonly`, zwei Felder
        // darunter). Eine Sperre, die auch `Enter` frisst, waere eine Formular-
        // Funktion weniger.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Taste **geschluckt** wird und nicht
        // ignoriert: `taste_ins_feld` gibt `true` zurueck, wenn sie die Taste
        // verbraucht hat; `false` heisst „nicht meins“, und dann landet die
        // Taste in den Zweigen fuer die Leiste.
        if self.feld_nur_lesbar(ord)
            && (matches!(k.key, Key::Backspace | Key::Delete)
                || k.typed_char().is_some()
                || (k.modifiers.ctrl && matches!(k.key, Key::Char('x') | Key::Char('v'))))
        {
            return true;
        }
        //
        // KONTEXT (04.10.2026, 01:10): VOR der Aenderung merken, was da war.
        //
        // GEMESSENE ENTSCHEIDUNG, warum **vor** und nicht nachher: nachher
        // stuende der **neue** Wert im Stapel, und `Strg+Z` wuerde den neuen
        // Wert wiederherstellen — also nichts.
        //
        // GEMESSENE ENTSCHEIDUNG, warum nur bei **aendernden** Tasten: ein
        // Stapel, der auch bei Pfeiltasten waechst, heisst, dass viermal
        // „Pfeil links“ und dann `Strg+Z` den Cursor bewegt statt die Eingabe
        // zurueckzunehmen.
        if matches!(k.key, narsil_input::Key::Backspace | narsil_input::Key::Delete)
            || k.typed_char().is_some()
        {
            let wert = self.feld_wert(ord);
            self.feld_verlauf.push((ord, wert, self.feld_cursor));
        }
        // Der aktuelle Wert: der, der getippt wurde, sonst der aus dem
        // HTML. GEMESSEN, warum der Stand die Quelle ist: er ueberlebt
        // den Klick, der die Seite neu baut.
        //
        // W7C A8: `mehrzeilig` kommt aus dem DOM-STAND (nicht aus `typ`),
        // weil der Stand die EINZIGE Quelle fuer „was ist das fuer ein
        // Feld" ist — `tippbar()` liest auch von dort. Eine zweite
        // Entscheidung ueber dieselbe Sache waere die Fehlerklasse
        // „drei Zahlenraeume" in neuer Kleidung.
        let (mut wert, form_idx, mehrzeilig) = {
            let a = self.angezeigt.as_ref().expect("Seite");
            let w = a.stand.lock().expect("DomStand");
            let feld = w.eingaben.iter().find(|e| e.ord == ord);
            let wert = w
                .eingabe_werte
                .get(&ord)
                .cloned()
                .or_else(|| feld.map(|e| e.wert.clone()))
                .unwrap_or_default();
            (
                wert,
                feld.and_then(|e| e.formular),
                feld.is_some_and(|e| e.mehrzeilig()),
            )
        };
        let mut cursor = self.feld_cursor.min(wert.chars().count());
        //
        // KONTEXT (05.10.2026, 02:40): DAS **KLEMMBRETT** im Feld
        // (Strg+A, Strg+C, Strg+X, Strg+V).
        //
        // GEMESSENE ENTSCHEIDUNG, warum das **vor** dem `match` steht: die vier
        // Tasten aendern den Wert nicht ueber den Weg der anderen (sie
        // benutzen `werte_schreiben` direkt, weil eine Markierung ueber den
        // Cursor nicht laeuft) — und ein `Strg+C`, das im `match` **keinen**
        // Zweig haette, waere ein `Char('c')` und damit **getippter Text**.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `!k.modifiers.alt` und nicht
        // `only_ctrl()`: `only_ctrl()` verbietet auch **Umschalt** — und
        // `Umschalt+Strg+C` (oder ein Capslock-gesichertes `Strg+A` mit
        // Umschalt) soll hier auch kopieren; GEMESSEN ist, dass `only_ctrl()`
        // genau das verbietet.
        if k.modifiers.ctrl && !k.modifiers.alt && k.pressed {
            match k.key {
                // GEMESSENE ENTSCHEIDUNG (beim Schreiben bemerkt): **Ausschneiden
                // und Einfuegen aendern den Wert** — sie gehoeren also in den
                // Rueckgaengig-Stapel, sonst waere `Strg+Z` nach einem `Strg+X`
                // eine leere Wirkung, und der Stapel waere die Antwort auf die
                // Frage „welche Taste hat den Wert geaendert“ — unvollstaendig.
                Key::Char('x') | Key::Char('v') => {
                    let w = self.feld_wert(ord);
                    self.feld_verlauf.push((ord, w, self.feld_cursor));
                }
                _ => {}
            }
            match k.key {
                Key::Char('a') => {
                    self.feld_auswahl = Some((0, wert.chars().count()));
                    self.feld_cursor = 0;
                    return true;
                }
                Key::Char('c') => {
                    return self.feld_markiert_kopieren(false);
                }
                Key::Char('x') => {
                    return self.feld_markiert_kopieren(true);
                }
                Key::Char('v') => {
                    return self.feld_markiert_einfuegen();
                }
                _ => {}
            }
        }
        //
        // KONTEXT (05.10.2026, 02:42): EINE MARKIERUNG WIRD **AUFGEZEHRT**.
        //
        // GEMESSENE ENTSCHEIDUNG, warum **vor** dem `match` und nicht in den
        // Aesten: `Backspace`, `Delete`, ein Zeichen und `Enter` brauchen alle
        // denselben ersten Schritt („die Markierung loeschen“) — viermal
        // geschrieben waere es viermal falsch, wenn einer vergessen wird, und
        // die Wirkung waere „getippt, Markierung noch da“.
        if let Some((von, bis)) = self.feld_auswahl {
            let n = wert.chars().count();
            let a = von.min(bis).min(n);
            let b = bis.max(von).min(n);
            if a != b && (matches!(k.key, Key::Backspace | Key::Delete)
                || k.typed_char().is_some() || (mehrzeilig && k.key == Key::Enter))
            {
                let mut v = wert.chars().collect::<Vec<char>>();
                v.drain(a..b);
                wert = v.into_iter().collect();
                cursor = a;
                self.feld_auswahl = None;
            } else if !(k.modifiers.shift && matches!(k.key, Key::Left | Key::Right | Key::Home | Key::End)) {
                // Release 09.10.2026: Umschalt+Pfeil/Pos1/Ende VERLAENGERT —
                // die Markierung bleibt fuer seinen Zweig stehen (vorher wurde
                // sie hier fuer jede Taste geloescht, und der Anker wanderte).
                self.feld_auswahl = None;
            }
        }
        let mut neuwert: Option<String> = None;
        match k.key {
            Key::Enter => {
                //
                // W7C A8 (13:40): IN EINEM `textarea` IST ENTER EIN
                // ZEILENUMBRUCH.
                //
                // GEMESSEN, warum das der einzige Unterschied ist: ein
                // Kommentarfeld mit „Enter = abschicken" ist unbrauchbar
                // (man kann keinen Absatz schreiben), und genau so war es:
                // `textarea` war nicht tippbar, also war es gar kein Feld.
                // Sobald es ein Feld IST, muss Enter das tun, was Enter
                // in einem Feld tut.
                //
                // GEMESSEN, warum `mehrzeilig()` und nicht `typ == "textarea"`
                // hier: dieselbe Frage stellt `werte_schreiben` (fuer den
                // Umbruch im Text) und die Anzeige. Eine Funktion, eine
                // Antwort.
                if mehrzeilig {
                    let mut zeichen: Vec<char> = wert.chars().collect();
                    if zeichen.len() >= crate::dom::MAX_EINGABE_WERT {
                        self.meldung = Some(tr("Input field full").into());
                        return true;
                    }
                    let position = cursor.min(zeichen.len());
                    zeichen.insert(position, '\n');
                    self.werte_schreiben(ord, zeichen.into_iter().collect(), position + 1);
                    return true;
                }
                //
                // KONTEXT (05.10.2026, 23:45): ENTER SCHICKT **NUR**, WENN DIE
                // FORMULAR-SPERRE ES HERGIBT (`enter_sendet`, gruene Crate).
                //
                // GEMESSENER BEFUND, warum das **vor** `formular_senden` muss:
                // GEMESSEN ist, dass hier **immer** geschickt wurde — auch bei
                // einem Formular mit zwei Textfeldern und **ohne** Absender-
                // Knopf. Firefox schickt da **nichts** (die stille Uebermittlung
                // gibt es nur bei einem Absender oder bei hoechstens einem Feld).
                // Unser Verhalten war: ein Formular abschicken, von dem der
                // Nutzer es nicht erwartet — die schlimmste Art, denn **nichts**
                // an der Oberflaeche zeigt einen Fehler an.
                //
                // GEMESSENE ENTSCHEIDUNG, warum der Wert **trotzdem** geschrieben
                // wird: der Inhalt des Feldes ist eine Eingabe, und die wird so
                // gerettet wie bei jedem anderen Zeichen. `return true` heisst
                // „Taste verbraucht“ — sonst laeuft das Enter in den Zweig der
                // Adressleiste und **navigiert**.
                self.werte_schreiben(ord, wert.clone(), cursor);
                let (blockierer, absender) = self.formular_sperre(form_idx);
                if !enter_sendet(mehrzeilig, blockierer, absender) {
                    return true;
                }
                // ABSCHICKEN (W7 P2) — siehe `formular_senden`.
                return self.formular_senden(form_idx);
            }
            //
            // KONTEXT (05.10.2026, 03:18): AUF und AB **im mehrzeiligen
            // Feld** — sie gehen eine Zeile hoch und runter.
            //
            // GEMESSENE ENTSCHEIDUNG, warum `w` (die **Spaltenzahl**) nicht
            // mitgeht: GEMESSEN ist, dass unser Feld **umbricht** (der Wert wird
            // neu gebaut, und der Cursor steht am Ende), es hat also keine
            // sichtbaren Zeilen, in denen man sich waagerecht bewegen koennte.
            // Eine Zeile ist hier eine Zeile des **Wertes** (`\n`) — und das
            // ist die ehrliche Antwort, weil `textarea` genau dort seinen
            // Mehrzeiler hat.
            Key::Up if mehrzeilig => {
                cursor = feld_zeile_hoch(&wert, cursor);
            }
            Key::Down if mehrzeilig => {
                cursor = feld_zeile_runter(&wert, cursor);
            }
            Key::Escape => {
                // Escape verliert den Fokus, der Wert bleibt (wie im
                // Browser: Escape bricht NICHT ab).
                self.feld_fokus = None;
                self.feld_cursor = 0;
                return true;
            }
            Key::Left => {
                // KONTEXT (05.10.2026, 02:44): UMSCHALT **VERLAENGERT**.
                cursor = cursor.saturating_sub(1);
                if k.modifiers.shift {
                    self.feld_markieren_bis(cursor);
                } else {
                    self.feld_auswahl = None;
                }
            }
            Key::Right => {
                cursor = (cursor + 1).min(wert.chars().count());
                if k.modifiers.shift {
                    self.feld_markieren_bis(cursor);
                } else {
                    self.feld_auswahl = None;
                }
            }
            Key::Home => {
                cursor = 0;
                if k.modifiers.shift {
                    self.feld_markieren_bis(cursor);
                } else {
                    self.feld_auswahl = None;
                }
            }
            Key::End => {
                cursor = wert.chars().count();
                if k.modifiers.shift {
                    self.feld_markieren_bis(cursor);
                } else {
                    self.feld_auswahl = None;
                }
            }
            Key::Backspace => {
                if cursor > 0 {
                    // M5/M3: an ZEICHEN, nicht an Bytes — sonst paniced ein
                    // Mehrbyte-Zeichen in der Mitte.
                    let mut v: Vec<char> = wert.chars().collect();
                    v.remove(cursor - 1);
                    neuwert = Some(v.into_iter().collect());
                    cursor -= 1;
                } else {
                    return true; // nichts passiert, aber die Taste war fuer uns
                }
            }
            Key::Delete => {
                let n = wert.chars().count();
                if cursor < n {
                    let mut v: Vec<char> = wert.chars().collect();
                    v.remove(cursor);
                    neuwert = Some(v.into_iter().collect());
                } else {
                    return true;
                }
            }
            Key::Char(c) => {
                // M3/M4: `MAX_TIPP` (128) Zeichen je Taste und
                // `MAX_EINGABE_WERT` (4096) Zeichen im Feld. Beides wird
                // VOR dem Bau des neuen Wertes geprueft.
                let neu = wert.chars().count() + 1;
                if neu > crate::dom::MAX_EINGABE_WERT {
                    self.meldung = Some(tr("Input field full").into());
                    return true;
                }
                let mut v: Vec<char> = wert.chars().collect();
                v.insert(cursor.min(v.len()), c);
                neuwert = Some(v.into_iter().collect());
                cursor += 1;
            }
            Key::Space => {
                let neu = wert.chars().count() + 1;
                if neu > crate::dom::MAX_EINGABE_WERT {
                    self.meldung = Some(tr("Input field full").into());
                    return true;
                }
                let mut v: Vec<char> = wert.chars().collect();
                v.insert(cursor.min(v.len()), ' ');
                neuwert = Some(v.into_iter().collect());
                cursor += 1;
            }
            _ => return false,
        }
        if let Some(n) = neuwert {
            wert = n;
        }
        self.werte_schreiben(ord, wert, cursor);
        true
    }

    /// W7 P2: Das Rechteck des CURSORS im fokussierten Feld.
    ///
    /// `None`, wenn kein Feld den Fokus hat.
    ///
    /// # WOHER DIE POSITION KOMMT
    ///
    /// Aus dem Kastenbaum: gesucht wird der Knoten, der zum fokussierten
    /// `<input>` gehoert (die Bruecke `Seite::dom_zu_ord` nennt die
    /// Nummer). Dessen Kasten traegt den Text; der Cursor steht HINTER
    /// dem getippten Wert, also um dessen Breite weiter rechts.
    ///
    /// # DIE BREITE (M3)
    ///
    /// `MAX_CURSOR_BREITE` (3 px) — ein Cursor ist ein Strich, kein
    /// Balken. Und `cursor_x` ist auf die Kastenbreite begrenzt: ein
    /// Wert, der rechts herauszeigt, waer ein Rechteck ausserhalb des
    /// Fensters (und der Rasterizer wuerde es clippen oder nicht).
    pub fn cursor_rechteck(&self) -> Option<DisplayItem> {
        const MAX_CURSOR_BREITE: f32 = 3.0;
        let ord = self.feld_fokus?;
        let a = self.angezeigt.as_ref()?;
        // Der Kasten des Feldes — derselbe Weg wie beim Fokusring (siehe
        // `feld_kasten`): eine Kiste, ein Weg.
        let k = feld_kasten(a, ord)?;
        // Wie breit ist der getippte Wert? Die Messung des Browsers —
        // dieselbe, die das Layout benutzt (sonst stuende der Cursor
        // neben dem Text, an dem er haengt).
        let wert = {
            let w = a.stand.lock().expect("DomStand");
            w.eingabe_werte
                .get(&ord)
                .cloned()
                .or_else(|| {
                    w.eingaben
                        .iter()
                        .find(|e| e.ord == ord)
                        .map(|e| e.wert.clone())
                })
                .unwrap_or_default()
        };
        // Release 08.10.2026: der Cursor steht dort, wo der Ueberzug den
        // Text setzt (`FeldBild`: Polster, Schriftgroesse und Textstelle
        // des Feldes, derselbe sichtbare Ausschnitt), in der TEXTFARBE des
        // Feldes und so hoch wie die Zeile. Vorher: an `kasten.x` ohne
        // Polster, 16 px gemessen, hellgrau ueber die ganze Kastenhoehe —
        // auf einem hellen Feld unsichtbar.
        let typ = feld_attribut(a, ord, "type")
            .unwrap_or_default()
            .to_lowercase();
        if let Some(bild) = feld_bilder(a, &[ord]).remove(&ord)
            && bild.flaeche.width > 0.0
            && bild.flaeche.height > 0.0
        {
            let zeichen = if wert.is_empty() { String::new() } else { feld_zeichen(a, ord, &typ, &wert) };
            let cursor = self.feld_cursor.min(zeichen.chars().count());
            let (von, _) = self.feld_ausschnitt(&zeichen, Some(cursor), &bild, 64);
            let vor: String = zeichen.chars().skip(von).take(cursor.saturating_sub(von)).collect();
            let breite = if vor.is_empty() { 0.0 } else { self.feld_text_breite(&vor, &bild) };
            let zeile = (bild.schrift * 1.16).min(bild.flaeche.height);
            let x = (bild.text_x + breite).min(bild.flaeche.x + bild.flaeche.width - 1.0);
            let y = bild.text_y.clamp(bild.flaeche.y, (bild.flaeche.y + bild.flaeche.height - zeile).max(bild.flaeche.y));
            let strich = (bild.schrift / 16.0).round().clamp(1.0, MAX_CURSOR_BREITE);
            return Some(DisplayItem::Rect {
                radius: [0.0; 4],
                deckkraft: 1.0,
                schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                schatten_anzahl: 0,
                rect: narsil_core::geometry::Rect::new(x, y, strich, zeile),
                color: bild.farbe,
            });
        }
        let bis = wert.chars().take(self.feld_cursor).collect::<String>();
        // GEMESSEN, warum `narsil_page::GRAD_TEXT` (16 px) und nicht eine
        // Eigenschaft der Seite: die Felder eines Formulars tragen in
        // diesem Browser die Standardgroesse (der Platzhalter
        // `[Eingabe: …]` wird mit derselben gesetzt), und eine eigene
        // Zahl waere die zweite.
        let text_breite = if bis.is_empty() {
            0.0
        } else {
            let m = self.messung.clone();
            m.breite(&bis, narsil_page::GRAD_TEXT)
        };
        let x = (k.x + text_breite).min(k.x + k.width);
        Some(DisplayItem::Rect {
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
            rect: narsil_core::geometry::Rect::new(
                x,
                k.y,
                MAX_CURSOR_BREITE.min(k.width.max(1.0)),
                k.height,
            ),
            color: narsil_core::color::Color::new(0xf0, 0xf0, 0xf0, 255),
        })
    }

    /// W7 P2: Das Formular abschicken (GET ueber `narsil_url`, POST ueber
    /// `narsil-net`).
    ///
    /// # DER GET-WEG
    ///
    /// `narsil_url::formular_get_url` baut die URL — das ist die EINE
    /// Stelle, die weiss, wie ein Formular kodiert wird (die Rolle URL
    /// pflegt sie). Danach `gehe_zu`, also derselbe Weg wie eine
    /// getippte Adresse.
    ///
    /// # DER POST-WEG
    ///
    /// `narsil_net::Request` mit `method: Post` und einem Body
    /// (`application/x-www-form-urlencoded`). Die Kodierung macht wieder
    /// `narsil_url` — eine eigene Kodierung hier waere die zweite Wahrheit
    /// ueber dasselbe Byte-Format.
    ///
    /// `form_idx` = Index in `Stand::formulare`; `None` heisst: das Feld
    /// steht in KEINEM Formular. Dann gibt es nichts zu schicken — ein
    /// Browser wuerde die Seite neu laden, und genau das tun wir auch.
    fn formular_senden(&mut self, form_idx: Option<usize>) -> bool {
        let Some(fi) = form_idx else { return false; };
        // Release 08.10.2026 (Feature 4): beim Abschicken lernen.
        self.formular_lernen(form_idx);
        let form_target = self.angezeigt.as_ref().and_then(|a| {
            let state = a.stand.lock().expect("DOM");
            let ord = state.formulare.get(fi)?.ord;
            Some((ord, *state.ids.get(&ord)?))
        });
        if let Some((ord, target)) = form_target {
            let ancestors = self.ereignis_vorfahren(ord);
            self.ereignis_ausloesen(target, &ancestors, "submit", true, true, 0.0, 0.0);
            if self.navigation_verhindert() { return true; }
        }
        let Some(a) = self.angezeigt.as_ref() else {
            return false;
        };
        let basis = a.seite.basis.clone();
        let (form, felder, dateien) = {
            let w = a.stand.lock().expect("DomStand");
            let Some(fi) = form_idx else {
                return false;
            };
            let Some(f) = w.formulare.get(fi) else {
                return false;
            };
            // M3: `MAX_EINGABEN` (256) Felder, jeder mit Name und Wert.
            //
            // W7C A2: DIE REGEL STECHT IN `Eingabe::senden` (03:20).
            //
            // GEMESSEN, warum die Patch-Werte hier gelesen werden und
            // nicht im DOM-Stand: der `Patch` ist der Ort, an dem die
            // ENTSCHEIDUNGEN des Nutzers liegen (der getippte Wert, der
            // Haken, die gewaehlte Option). Er ueberlebt den Neuaufbau
            // der Seite, der Stand nicht. Wer die Werte an zwei Stellen
            // haelt (Stand fuer den Haken, Patch fuer den Text), hat
            // nach dem ersten Klick zwei verschiedene Formulare.
            let patch = a.patch.clone();
            // Release 10.10.2026: die Dateifelder (siehe `DateiTeil`). In der
            // Feldliste steht je Datei ihr NAME (urlencoded/GET, HTML
            // §4.10.21.4); der mehrteilige Versand setzt die Dateien ein.
            let mut dateien: Vec<crate::dom::DateiTeil> = Vec::new();
            let mut stelle = 0usize;
            let felder: Vec<narsil_url::FormularFeld> = f
                .felder
                .iter()
                .take(crate::dom::MAX_EINGABEN)
                .filter_map(|ord| {
                    if let Some(e) = w.eingaben.iter().find(|e| e.ord == *ord && e.typ == "file") {
                        if e.disabled || e.name.is_empty() {
                            return None;
                        }
                        let liste = w.dateien.get(&e.host).cloned().unwrap_or_default();
                        let namen: Vec<String> = if liste.is_empty() {
                            vec![String::new()]
                        } else {
                            liste.iter().map(|d| d.name.clone()).collect()
                        };
                        dateien.push(crate::dom::DateiTeil {
                            start: stelle,
                            anzahl: namen.len(),
                            name: e.name.clone(),
                            dateien: liste,
                        });
                        stelle += namen.len();
                        return Some(namen.into_iter().map(|n| narsil_url::FormularFeld::neu(e.name.clone(), n)).collect::<Vec<_>>());
                    }
                    let teil = (|| {
                    // 1. Auswahl zuerst: sie hat keine `Eingabe`.
                    if let Some(au) = w.auswahl.iter().find(|au| au.ord == *ord) {
                        if au.disabled || au.name.is_empty() {
                            return None;
                        }
                        // Die Wahl des Nutzers (Patch), sonst die aus
                        // dem HTML. GEMESSEN, warum der Patch Vorrag hat:
                        // `gewaehlte_indizes` faellt auf die ERSTE
                        // Option zurueck, und der Nutzer kann eine
                        // andere gewaehlt haben.
                        let gewaehlt: Vec<usize> =
                            match patch.attribut.get(&(*ord, "value".to_string())) {
                                Some(wert) => au
                                    .optionen
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, o)| o.wert == *wert)
                                    .map(|(i, _)| i)
                                    .collect(),
                                None => au.gewaehlte_indizes(),
                            };
                        return Some(
                            gewaehlt
                                .iter()
                                .filter_map(|i| au.optionen.get(*i))
                                .map(|o| {
                                    narsil_url::FormularFeld::neu(au.name.clone(), o.wert.clone())
                                })
                                .collect::<Vec<_>>(),
                        );
                    }
                    // 2. Eingabefeld.
                    let e = w.eingaben.iter().find(|e| e.ord == *ord)?;
                    // Der Absender: nur der GEKLICKTE (W7C A2).
                    if e.absender && Some(*ord) != self.knopf_fuer_formular {
                        return None;
                    }
                    let haken = patch
                        .attribut
                        .get(&(*ord, "checked".to_string()))
                        .map(|v| !v.is_empty());
                    let wert = patch.attribut.get(&(*ord, "value".to_string()));
                    let (name, wert) = e.senden(haken, wert.map(String::as_str))?;
                    Some(vec![narsil_url::FormularFeld::neu(name, wert)])
                    })();
                    if let Some(t) = &teil {
                        stelle += t.len();
                    }
                    teil
                })
                .flatten()
                .collect();
            (f.clone(), felder, dateien)
        };

        if form.ist_post() {
            // POST: der Body wird hier gebaut, das Senden ueber
            // `narsil-net`. GEMESSEN, warum `lade_mit_body` noetig ist und
            // nicht `gehe_zu`: `gehe_zu` kann nur GET (es hat keine
            // Koerperangabe), und ein POST als GET waere ein Formular,
            // das im Suchfeld landet statt in der Datenbank.
            match crate::laden::sende_formular_post(&basis, &form, &felder, &dateien) {
                Ok(l) => {
                    // GEMESSEN, warum `aus_html` und nicht `gehe_zu`:
                    // der Text ist schon da (Netz), und `gehe_zu` wuerde
                    // die Adresse neu holen — als GET. Der Weg fuer
                    // „Text ist da" ist `aus_html`.
                    match self.aus_html(
                        &l.text,
                        if form.aktion.is_empty() {
                            &basis
                        } else {
                            &form.aktion
                        },
                        self.breite.max(1),
                        self.sicht_hoehe(),
                    ) {
                        Ok(neu) => {
                            //
                            // 03.10. 05:05 (HTML): DAS FORMULAR ZIEHT UM.
                            //
                            // GEMESSENER BEFUND (Wegwerf entfernt, lokaler
                            // Echo-Server): das **POST selbst ist vollstaendig
                            // richtig** — GEMESSEN auf der Leitung:
                            //   `POST /echo HTTP/1.1`
                            //   `content-type: application/x-www-form-urlencoded`
                            //   `content-length: 19`
                            //   Koerper: `q=Rust+%C3%A4%C3%B6`
                            // und die **Antwort steht auch im Bild**
                            // (GEMESSEN: `["ANTWORTVOMECHO ok", …]`). Nur die
                            // **Adresse blieb stehen**: GEMESSEN
                            // `browser.adresse() = file:///…/m_echo.html`, obwohl
                            // im Bild die Antwort des Servers stand.
                            //
                            // GEMESSENE EINORDNUNG, warum das sichtbar ist: bei
                            // einem Formular, das auf ein **anderes** Ziel geht
                            // (Anmeldung, Warenkorb, Suche auf einer anderen
                            // Domain), zeigt die Adressleiste die alte Seite —
                            // wer sie kopiert oder ein Lesezeichen darauf legt,
                            // bekommt das Falsche, und „Zurueck" kennt den
                            // Sprung nicht.
                            //
                            // GEMESSENE ENTSCHEIDUNG, warum hier dieselben drei
                            // Schritte wie in [`Self::gehe_zu`] stehen und nicht
                            // eigene: die Liste „was nach einer Navigation gilt"
                            // (Adresse, Verlauf, Adressleiste, kein
                            // Verlaufskandidat) steht **an einer** Stelle. Ein
                            // zweiter Weg waere die Fehlerklasse „zwei Zahlen-
                            // raeume" fuer die Frage „wo ist der Nutzer".
                            //
                            // WAS ICH NICHT BEHAUPTE: dass die Adresse nach einer
                            // **Umleitung** stimmt. `loese_link` rechnet das
                            // `action` auf — bei einem 302 steht in der Leiste
                            // danach das **alte** Ziel, nicht das der Antwort.
                            // GEMESSEN ist das an duckduckgo (`duckduckgo.com`
                            // -> 302 -> `html.duckduckgo.com`). Die Endadresse
                            // hat nur `narsil-net` (sie kennt sie nach dem Sprung);
                            // gemeldet in `team/PROBLEME.txt`.
                            let ziel = if form.aktion.is_empty() {
                                basis.clone()
                            } else {
                                narsil_page::loese_link(&form.aktion, &basis)
                            };
                            self.browser.anwenden(&Navigation::To(ziel.clone()));
                            self.chrome.setze_adresse(&ziel);
                            self.knopf_zuruecksetzen();
                            self.leiste_verlauf_pos = None;
                            self.angezeigt = Some(neu);
                            // W7C A4: auch nach einem POST hat die Seite
                            // einen Titel (bei einer Anmeldung ist es
                            // „Willkommen" statt „Anmelden").
                            self.titel_tragen();
                            let titel = self.aktuelle_titel().to_string();
                            self.dienste
                                .on_page_load(&self.browser.adresse().to_string(), &titel);
                            //
                            // W7C A5: auch hier wird nichts gespeichert —
                            // siehe die Bemerkung beim GET.
                            self.meldung = None;
                            self.geladen += 1;
                            self.schmutzig = true;
                            self.feld_fokus = None;
                            return true;
                        }
                        Err(e) => {
                            self.meldung = Some(std::format!("POST: {e}"));
                            return true;
                        }
                    }
                }
                Err(e) => {
                    self.meldung = Some(std::format!("POST: {e}"));
                    return true;
                }
            }
        }

        // GET
        let ziel = if form.aktion.is_empty() {
            basis.clone()
        } else {
            form.aktion.clone()
        };
        match narsil_url::formular_get_url(&ziel, &felder) {
            Ok(url) => {
                self.feld_fokus = None;
                // GEMESSEN, warum `gehe_zu` und nicht `lade_jetzt`:
                // `gehe_zu` schreibt die Adresse in die Leiste — der
                // Nutzer soll sehen, welche Seite er gerade abgeschickt
                // hat (und die Leiste ist bei einer Suche die Antwort).
                let _ = self.gehe_zu(&url);
                true
            }
            Err(e) => {
                self.meldung = Some(std::format!("Formular: {e}"));
                true
            }
        }
    }

    /// W7 P2: Den Wert ins Feld schreiben UND die Seite neu bauen.
    ///
    /// GEMESSEN, warum ueber den Patch und nicht direkt in den Baum: der
    /// Wert muss den Klick ueberleben und beim naechsten Klick noch da
    /// sein — derselbe Weg wie `z.textContent` (`replay.rs`).
    // Release 08.10.2026: `pub(crate)` fuer das Formular-Gedaechtnis
    // (Feature 4), das Felder ueber denselben Weg fuellt wie das Tippen.
    pub(crate) fn werte_schreiben(&mut self, ord: usize, wert: String, cursor: usize) {
        self.feld_cursor = cursor;
        let Some(a) = self.angezeigt.as_mut() else {
            return;
        };
        // Cursor und Auswahl ändern den Feldwert nicht. In diesem Fall
        // bleiben DOM und Layout erhalten; nur der Cursor wird neu gezeichnet.
        let unveraendert = {
            let w = a.stand.lock().expect("DomStand");
            w.eingabe_werte.get(&ord).map(String::as_str)
                .or_else(|| w.eingaben.iter().find(|e| e.ord == ord).map(|e| e.wert.as_str()))
                == Some(wert.as_str())
        };
        if unveraendert {
            self.schmutzig = true;
            return;
        }
        {
            let mut w = a.stand.lock().expect("DomStand");
            w.eingabe_werte.insert(ord, wert.clone());
            // Release 2026-10-15: typed values reach the selectors too.
            crate::dom::eingabe_spiegeln(&mut w, ord, &wert);
            w.befehle.push(crate::js::Befehl::Attribut {
                knoten: ord as u32,
                name: "value".to_string(),
                wert,
            });
        }
        // Und die Seite neu bauen: derselbe Weg wie nach einem Klick.
        let befehle = {
            let mut w = a.stand.lock().expect("DomStand");
            w.nimm_befehle()
        };
        crate::replay::spielen(&mut a.patch, befehle);
        if let Some(html) = crate::js::lebend_abschliessen(&a.stand, &mut a.patch) {
            a.html = std::sync::Arc::new(html);
        }
        let target = a.stand.lock().expect("DOM").ids.get(&ord).copied();
        if let Some(target) = target {
            let ancestors = self.ereignis_vorfahren(ord);
            let a = self.angezeigt.as_mut().expect("page");
            let event = Self::ereignis_fuer_seite(a, target, &ancestors,
                "input", true, false, 0.0, 0.0);
            Self::ereignis_anwenden(a, event);
        }
        self.seite_neu_bauen_sofort();
    }

    /// W7C A2: DIE SEITE AUS DEM PATCH NEU BAUEN (03:10).
    ///
    /// GEMESSEN, warum das jetzt EIN Ort ist: es gab drei Kopien
    /// (`werte_schreiben` 21:45, der Handler-Weg 17:15, der Klick-Weg).
    /// Alle drei machten dasselbe — Seite bauen, Layout rechnen, beide
    /// ablegen — und alle drei konnten auseinanderlaufen. Genau das ist
    /// passiert: in der Klick-Kopie fehlte die `messung`, in der
    /// Handler-Kopie der `versatz`.
    ///
    /// Der Aufrufer hat den `Patch` FUELLEN lassen; hier passiert nur das
    /// Bauen. GEMESSEN, warum kein `&mut self` noetig ist um mehr als
    /// `schmutzig`: die Seite ist ein Zustand, und sie wird hier
    /// ersetzt, nicht am Rand herum manipuliert.
    // Release 08.10.2026: `pub(crate)` fuer den Cookie-Autopiloten (Feature 11).
    pub(crate) fn seite_neu_bauen(&mut self) {
        // Release 10.10.2026: im Fensterbetrieb (Hintergrund-Lader an) baut
        // der Neubau-Faden — der Haupt-Faden bleibt frei (siehe
        // `Sitzung::neubau_faden`). Ohne Lader (Tests, Werkzeuge) wie bisher.
        if self.seiten_lader.is_some() && std::env::var_os("NARSIL_NEUBAU_SOFORT").is_none() {
            if self.neubau_faden.is_none() {
                self.neubau_faden = NeubauFaden::neu();
            }
            if self.neubau_faden.is_some() && self.angezeigt.is_some() {
                self.neubau_anfordern();
                return;
            }
        }
        self.seite_neu_bauen_sofort();
    }

    /// Release 10.10.2026: der Neubau JETZT, im Haupt-Faden (Tippen,
    /// Dateiauswahl, Einstellungen — dort muss das Ergebnis sofort stehen).
    pub(crate) fn seite_neu_bauen_sofort(&mut self) {
        let breite = self.breite.max(1);
        let sicht = self.sicht_hoehe();
        let messung = self.messung.clone();
        let zustand = self.zustand();
        if let Some(a) = self.angezeigt.as_mut() {
            Self::seite_fuer_anzeige_neu_bauen(a, breite, sicht, messung, &zustand);
        }
        self.bilder_sichtbar_laden();
    }

    /// Release 10.10.2026: einen Auftrag an den Neubau-Faden — laeuft schon
    /// einer, wird der Wunsch fuer danach vorgemerkt.
    fn neubau_anfordern(&mut self) {
        let breite = self.breite.max(1);
        let sicht = self.sicht_hoehe();
        let messung = self.messung.clone();
        let zustand = self.zustand();
        let generation = self.seiten_generation;
        let Some(faden) = self.neubau_faden.as_mut() else { return };
        if faden.laeuft {
            faden.offen = true;
            return;
        }
        let Some(a) = self.angezeigt.as_ref() else { return };
        let masse: HashMap<String, (f32, f32)> = a
            .bilder
            .geladen
            .iter()
            .filter_map(|(url, b)| b.as_ref().ok().and_then(|b| b.layout_masse()).map(|m| (url.clone(), m)))
            .collect();
        let auftrag = NeubauAuftrag {
            generation,
            html: a.html.clone(),
            patch: a.patch.clone(),
            breite,
            sicht,
            basis: a.seite.basis.clone(),
            messung,
            zustand,
            masse,
            ids: a.stand.lock().expect("DOM").ids.clone(),
        };
        if faden.sender.send(auftrag).is_ok() {
            faden.laeuft = true;
            faden.offen = false;
        } else {
            self.neubau_faden = None;
            self.seite_neu_bauen_sofort();
        }
    }

    /// Release 10.10.2026: ein fertiges Neubau-Ergebnis uebernehmen (in
    /// `tick`), wenn es noch zur angezeigten Seite gehoert.
    fn neubau_abholen(&mut self) -> bool {
        let Some(faden) = self.neubau_faden.as_mut() else { return false };
        let mut ergebnis = None;
        while let Ok(e) = faden.empfaenger.try_recv() {
            ergebnis = Some(e);
        }
        let Some(e) = ergebnis else { return false };
        faden.laeuft = false;
        let nochmal = std::mem::take(&mut faden.offen);
        let passt = e.generation == self.seiten_generation;
        if passt && let Some(a) = self.angezeigt.as_mut() {
            Self::neubau_anwenden(a, e.seite, e.baum, e.sicht, Some(&e.ids));
            self.bilder_sichtbar_laden();
            self.schmutzig = true;
        }
        if nochmal || !passt {
            self.neubau_anfordern();
        }
        passt
    }

    fn seite_fuer_anzeige_neu_bauen(a: &mut Angezeigt, breite: u32, sicht: u32,
        messung: narsil_layout::Messung, zustand: &narsil_css::Zustand) {
        let basis = a.seite.basis.clone();
        let (neu, gelegt) = {
            let bilder = &a.bilder;
            Self::neubau_rechnen(&a.html, &a.patch, breite, sicht, &basis, &messung, zustand, |url| {
                match bilder.geladen.get(url) {
                    Some(Ok(b)) => b.layout_masse(),
                    _ => None,
                }
            })
        };
        Self::neubau_anwenden(a, neu, gelegt, sicht, None);
    }

    /// Release 10.10.2026: der RECHENTEIL eines Neubaus (Kaskade, Bildmasse,
    /// Layout) — ohne `Angezeigt`, damit er auch im Neubau-Faden laeuft.
    #[allow(clippy::too_many_arguments)]
    fn neubau_rechnen(
        html: &[narsil_fmt_html::HtmlNode],
        patch: &narsil_page::Patch,
        breite: u32,
        sicht: u32,
        basis: &str,
        messung: &narsil_layout::Messung,
        zustand: &narsil_css::Zustand,
        masse: impl Fn(&str) -> Option<(f32, f32)>,
    ) -> (Seite, Result<LayoutTree, String>) {
        let zeit_start = takt_zeiten_an().then(std::time::Instant::now);
        let mut neu = narsil_page::seite_aus_knoten_mit_ansicht_zustand(
            html, patch, breite, sicht, basis, messung, zustand);
        let zeit_seite = zeit_start.map(|t| t.elapsed());
        bildmasse_uebernehmen_mit(&mut neu, masse);
        let gelegt = crate::nachumbruch::layout(&mut neu,
            Size::new(breite as f32, sicht as f32), messung)
            .map_err(|e| e.to_string());
        if let (Some(t), Some(s)) = (zeit_start, zeit_seite) {
            eprintln!(
                "NEUBAU seite {:.1} ms layout {:.1} ms knoten {}",
                s.as_secs_f64() * 1000.0,
                (t.elapsed() - s).as_secs_f64() * 1000.0,
                neu.document.len()
            );
        }
        (neu, gelegt)
    }

    /// Release 10.10.2026: der ANWENDE-TEIL eines Neubaus.
    ///
    /// `ids` ist die Zuordnung `ord -> Host` des HTML, aus dem `neu` gebaut
    /// ist (`None`: das aktuelle).
    fn neubau_anwenden(
        a: &mut Angezeigt,
        neu: Seite,
        gelegt: Result<LayoutTree, String>,
        sicht: u32,
        ids: Option<&HashMap<usize, u32>>,
    ) {
        if let Ok(baum) = gelegt {
            a.inhalt_hoehe = berechne_inhaltshoehe(&baum, sicht);
            a.versatz = a.versatz.min(a.inhalt_hoehe.saturating_sub(sicht));
            crate::js::ord_erneuern(&a.stand, &neu.felder, &neu.formular_ords);
            {
                let mut state = a.stand.lock().expect("DOM");
                state.css_baum = neu.css_baum.clone();
                state.css_baum.knoten.truncate(crate::js::MAX_SCAN);
                state.css_zu_ord = neu.css_zu_dom.clone();
                state.stil_satz = neu.stil_satz.clone();
                state.stil_cache.clear();
            }
            a.seite = neu;
            a.baum = baum;
            match ids {
                Some(ids) => masse_eintragen_mit(a, ids),
                None => masse_eintragen(a),
            }
        }
    }

    /// W7C A2: DIE SEITE HAT SICH GEAENDERT — neu bauen und zeichnen.
    ///
    /// GEMESSEN, warum das eine Methode ist und kein `schmutzig = true`:
    /// ein Haken, eine Auswahl, ein Reset — das sind drei Klicks, und in
    /// allen drei Faellen MUSS die Seite neu gebaut werden, weil der
    /// sichtbare Zustand (`[x]`, `[Auswahl: …]`) aus dem Patch kommt.
    /// Wer das an drei Stellen vergisst, hat einen Haken, den man nicht
    /// sieht — und der Nutzer haelt den Browser fuer kaputt.
    fn angepasst(&mut self, zeichnen: bool) {
        self.seite_neu_bauen_sofort();
        if zeichnen {
            self.schmutzig = true;
        }
    }

    /// Ein Klick an eine Stelle (Bildschirmkoordinaten, mit Leiste).
    ///
    /// # DIE REIHENFOLGE, und sie ist nicht verhandelbar
    ///
    /// 1. Der Klick geht an die **Leiste** (Adresse, Knoepfe).
    /// 2. Sonst: `Link` — ein Klick auf einen Link navigiert, und das ist
    ///    wichtiger als jedes Skript.
    /// 3. Sonst: die **`click`-Handler** der Elemente unter dem Punkt.
    ///
    /// GEMESSEN, warum die Handler NACH dem Link kommen: ein Skript, das
    /// `preventDefault()` sagt, wird erst danach befragt — und das ist
    /// der einzige Weg, wie ein Skript die Navigation verhindern koennte.
    /// Ein Browser, der beim Link aufhoert und dann den Handler laeuft,
    /// laueft in die Navigation hinein.
    ///
    /// # DIE ELEMENTSUCHE
    ///
    /// Der TIEFSTE Kasten, dessen Rechteck den Punkt enthaelt. Ohne
    /// „tiefste" traegt der Kasten eines Containers den Klick fuer alles
    /// darin, und ein Button in einem Panel waer nicht mehr klickbar.
    /// 02.10. 22:00 (HTML): WELCHER **TEXT** LIEGT UNTER DEM PUNKT?
    /// 02.10. 22:00 (HTML): WELCHER **TEXT** LIEGT UNTER DEM PUNKT?
    ///
    /// GEMESSENE GRUNDLAGE: `tiefster_knoten` liefert bei einem Klick auf einen
    /// Labeltext den **Container** (Knoten 2 statt 5), weil der Baum fuer den Text
    /// keinen eigenen Kasten hat. Die Anzeigeliste hat ihn aber: jeder Text liegt
    /// als `DisplayItem::Text` mit `origin` und `font_size` darin.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die **Anzeigeliste** und nicht der
    /// Layoutbaum: sie ist genau die Liste, die gezeichnet wird — also die, deren
    /// Kaesten der Nutzer sieht. Ein Kasten im Baum, der nicht gezeichnet wird,
    /// waere eine zweite Antwort auf „wo steht der Text".
    /// IST DER DIALOG **OFFEN**?
    pub fn datei_dialog_ist_offen(&self) -> bool {
        self.datei_dialog.is_some()
    }

    /// EIN KLICK IM DIALOG — auf eine Zeile, sonst auf **nichts**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Zeilenhoehe **20 px** ist und nicht
    /// `self.messung`: GEMESSEN ist, dass die Zeilenhoehe beim Zeichnen **fest**
    /// 20 px ist (siehe `datei_dialog_zeichnen`) — dieselbe Zahl, sonst waere
    /// der Klick eine Zeile daneben.
    pub fn datei_dialog_mausklick(&mut self, x: f32, y_seite: f32) -> bool {
        let Some(mut wahl) = self.datei_dialog.take() else {
            return false;
        };
        let breite = (self.breite as f32 * 0.7).min(720.0).max(240.0);
        let hoehe = (self.hoehe as f32 * 0.7).min(520.0).max(200.0);
        let bx = (self.breite as f32 - breite) / 2.0;
        let by = self.chrome.seiten_oberkante() as f32 + (self.hoehe as f32 - hoehe) / 2.0;
        if x < bx || x > bx + breite || y_seite < by || y_seite > by + hoehe {
            // Aussen: der Dialog bleibt offen (nicht schliessen — ein
            // Fehlklick soll nicht die Wahl aufheben).
            self.datei_dialog = Some(wahl);
            return false;
        }
        let zeile = ((y_seite - (by + 34.0)) / 20.0).floor();
        if zeile >= 0.0 && (zeile as usize) < wahl.eintraege.len() {
            wahl.auswahl = zeile as usize;
        }
        self.datei_dialog = Some(wahl);
        self.schmutzig = true;
        true
    }

    /// KONTEXT (05.10.2026, 08:35): DEN DATEI-DIALOG **OEFFNEN**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die **Ord-Nummer** uebergeben wird und
    /// nicht das Feld: der Dialog ist ein Zustand, das Feld eine Frage — und
    /// „wohin soll der Pfad?“ wird **einmal** beim Oeffnen beantwortet.
    pub fn datei_dialog_oeffnen(&mut self, ord: usize) -> bool {
        // Release 10.10.2026: unter Windows der Dialog des Systems (wie in
        // jedem Browser), mit `multiple` und `accept`. Der eingebaute Dialog
        // bleibt fuer Tests und andere Plattformen.
        #[cfg(windows)]
        if !narsil_chrome::einstellungen::laeuft_als_test()
            && std::env::var_os("NARSIL_DATEIDIALOG_EINGEBAUT").is_none()
        {
            let (mehrere, annahme) = match self.angezeigt.as_ref() {
                Some(a) => (
                    feld_attribut(a, ord, "multiple").is_some(),
                    feld_attribut(a, ord, "accept").unwrap_or_default(),
                ),
                None => return false,
            };
            let filter = datei_filter_aus_accept(&annahme);
            let titel = if mehrere { tr("Upload files") } else { tr("Upload file") };
            if let Some(pfade) = narsil_window::dateidialog_win32::oeffnen(titel, mehrere, &filter) {
                self.datei_auswahl_setzen(ord, pfade);
            }
            self.schmutzig = true;
            return true;
        }
        let mut wahl = Dateiwahl {
            ziel: ord,
            zweck: Dateizweck::Oeffnen,
            name: String::new(),
            verzeichnis: Dateiwahl::start(),
            eintraege: Vec::new(),
            auswahl: 0,
        };
        wahl.eintraege = datei_lesen(&wahl.verzeichnis);
        self.datei_dialog = Some(wahl);
        self.schmutzig = true;
        true
    }

    /// Release 10.10.2026: die AUSWAHL eines `<input type="file">` uebernehmen
    /// — fuer die Seite (`input.files`, `value` = `C:\fakepath\name` wie in
    /// Chrome, nie der echte Pfad) und den Formularversand; danach `input`
    /// und `change` (HTML §4.10.5.1.18). Eine leere Liste ist „abgebrochen".
    pub fn datei_auswahl_setzen(&mut self, ord: usize, pfade: Vec<std::path::PathBuf>) {
        let Some(a) = self.angezeigt.as_mut() else {
            return;
        };
        let dateien: Vec<crate::dom::GewaehlteDatei> = pfade
            .iter()
            .take(crate::dom::MAX_DATEIEN_JE_FELD)
            .filter_map(|p| crate::dom::GewaehlteDatei::aus_pfad(p))
            .collect();
        let wert = dateien.first().map(|d| format!("C:\\fakepath\\{}", d.name)).unwrap_or_default();
        let anzahl = dateien.len();
        let ziel = {
            let mut w = a.stand.lock().expect("DomStand");
            let ziel = w.ids.get(&ord).copied();
            if let Some(h) = ziel {
                if dateien.is_empty() {
                    w.dateien.remove(&h);
                } else {
                    w.dateien.insert(h, dateien);
                }
                w.dateien_stand += 1;
            }
            w.eingabe_werte.insert(ord, wert.clone());
            w.befehle.push(crate::js::Befehl::Attribut { knoten: ord as u32, name: "value".to_string(), wert });
            ziel
        };
        let befehle = a.stand.lock().expect("DomStand").nimm_befehle();
        crate::replay::spielen(&mut a.patch, befehle);
        if let Some(html) = crate::js::lebend_abschliessen(&a.stand, &mut a.patch) {
            a.html = std::sync::Arc::new(html);
        }
        if let Some(ziel) = ziel {
            let vorfahren = self.ereignis_vorfahren(ord);
            for art in ["input", "change"] {
                let a = self.angezeigt.as_mut().expect("Seite");
                let ergebnis = Self::ereignis_fuer_seite(a, ziel, &vorfahren, art, true, false, 0.0, 0.0);
                Self::ereignis_anwenden(a, ergebnis);
            }
        }
        self.meldung = Some(match anzahl {
            0 => tr("No file chosen").to_string(),
            1 => tr("1 file chosen").to_string(),
            n => trf!("{} files chosen", n),
        });
        self.seite_neu_bauen_sofort();
        self.schmutzig = true;
    }

    /// Release 10.10.2026: `input.click()`/`showPicker()` aus der Seite —
    /// der Dialog oeffnet sich nur bis 5 s nach einer Nutzeraktion.
    fn datei_dialog_wunsch_abholen(&mut self) {
        let Some(a) = self.angezeigt.as_ref() else {
            return;
        };
        let (wunsch, ord) = {
            let mut w = a.stand.lock().expect("DomStand");
            let Some(h) = w.datei_dialog_wunsch.take() else {
                return;
            };
            (h, w.ids.iter().find(|(_, v)| **v == h).map(|(o, _)| *o))
        };
        let aktiviert = self
            .letzte_aktivierung
            .is_some_and(|t| t.elapsed() <= std::time::Duration::from_secs(5));
        let _ = wunsch;
        if let Some(ord) = ord
            && aktiviert
        {
            self.letzte_aktivierung = None;
            self.datei_dialog_oeffnen(ord);
        }
    }

    /// KONTEXT (06.10.2026, 05:40, Leitung „Seite speichern (Strg+S) **mit
    /// Dateidialog**“): DEN DIALOG FUER DAS **SPEICHERN** oeffnen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **derselbe** Dialog und nicht ein zweiter:
    /// GEMESSEN ist, dass der Dialog (`datei_dialog_zeigen`, Tasten, Maus) der
    /// ganze Bildschirm voll ist — eine zweite Variante waere derselbe
    /// Bildschirm zweimal. GEMESSENE ENTSCHEIDUNG, warum der Dialog beim
    /// **Speichern** in `~/Downloads` startet: GEMESSEN ist, dass
    /// `seite_speichern` genau dorthin schreibt — ein Dialog, der woanders
    /// startet, wuerde an eine andere Stelle schreiben als das Menue.
    pub fn datei_dialog_speichern_oeffnen(&mut self) -> bool {
        let adresse = self.browser.adresse().to_string();
        if adresse.is_empty() || self.angezeigt.is_none() {
            self.meldung = Some(tr("Nothing to save — no page is open").into());
            return false;
        }
        let mut wahl = Dateiwahl {
            ziel: 0,
            zweck: Dateizweck::Speichern,
            name: dateiname_aus_adresse(&adresse, Dateizweck::Speichern),
            verzeichnis: crate::plattform::downloads(),
            eintraege: Vec::new(),
            auswahl: 0,
        };
        wahl.eintraege = datei_lesen(&wahl.verzeichnis);
        self.datei_dialog = Some(wahl);
        self.schmutzig = true;
        true
    }

    /// DER DIALOG **SCHLIESSEN** (ohne Auswahl).
    pub fn datei_dialog_schliessen(&mut self) {
        self.datei_dialog = None;
        self.schmutzig = true;
    }

    /// DIE **TASTEN** des Dialogs.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **vor** allem anderen in `taste`
    /// steht: ein offener Dialog ist ein **Modus** — die Seite bekommt dann
    /// keine Tasten mehr, sonst tippt man einen Suchbegriff in die Seite, waehrend
    /// man im Dialog ist.
    pub fn datei_dialog_tasten(&mut self, k: KeyEvent) -> bool {
        let Some(mut wahl) = self.datei_dialog.take() else {
            return false;
        };
        let anzahl = wahl.eintraege.len();
        match k.key {
            narsil_input::Key::Up => {
                wahl.auswahl = wahl.auswahl.saturating_sub(1);
            }
            narsil_input::Key::Down => {
                if anzahl > 0 {
                    wahl.auswahl = (wahl.auswahl + 1) % anzahl;
                }
            }
            narsil_input::Key::Home => wahl.auswahl = 0,
            narsil_input::Key::End => wahl.auswahl = anzahl.saturating_sub(1),
            narsil_input::Key::Escape => {
                self.datei_dialog = None;
                self.schmutzig = true;
                return true;
            }
            narsil_input::Key::Backspace => {
                // „**HOCH**“: eine Ebene nach oben.
                //
                // GEMESSENE ENTSCHEIDUNG, warum `Backspace` und nicht `Umschalt+
                // Punktieren“ oder ein Bild: GEMESSEN ist, dass der Dialog nur
                // **Tasten** bekommt (kein eigenes Zeichenfeld) — und
                // `Backspace` ist in jedem Dateidialog „eine Ebene hoch“.
                if let Some(oben) = wahl.verzeichnis.parent() {
                    wahl.verzeichnis = oben.to_path_buf();
                    wahl.eintraege = datei_lesen(&wahl.verzeichnis);
                    wahl.auswahl = 0;
                }
            }
            narsil_input::Key::Enter => {
                //
                // KONTEXT (06.10.2026, 05:42): BEIM SPEICHERN IST DER **NAME**
                // DAS GEWAEHLTE — nicht der markierte Eintrag. GEMESSENE
                // ENTSCHEIDUNG, warum der Dialog dann **zu** ist: ein
                // Speichern ist eine Handlung mit einem Ende; ein offener Dialog
                // daneben wuerde zwei Dinge gleichzeitig behaupten.
                if wahl.zweck == Dateizweck::Speichern {
                    let pfad = wahl.pfad();
                    let verzeichnis = wahl.verzeichnis.clone();
                    let name = wahl.name.clone();
                    self.datei_dialog = None;
                    match pfad {
                        Some(p) => {
                            self.seite_speichern_unter(&p);
                        }
                        None => {
                            self.meldung =
                                Some(tr("Save: the name is empty — the dialog stays closed").into());
                        }
                    }
                    let _ = (verzeichnis, name);
                    self.schmutzig = true;
                    return true;
                }
                let gewaehlt = wahl.eintraege.get(wahl.auswahl).cloned();
                match gewaehlt {
                    None => {
                        // Nichts gewaehlt: der Dialog bleibt offen (nicht
                        // schliessen, ohne dass etwas passiert).
                        self.datei_dialog = Some(wahl);
                        self.schmutzig = true;
                        return true;
                    }
                    Some((name, ist_verzeichnis)) => {
                        let pfad = wahl.verzeichnis.join(&name);
                        if ist_verzeichnis {
                            wahl.verzeichnis = pfad;
                            wahl.eintraege = datei_lesen(&wahl.verzeichnis);
                            wahl.auswahl = 0;
                            self.datei_dialog = Some(wahl);
                            self.schmutzig = true;
                            return true;
                        }
                        // Eine **Datei** ist gewaehlt: der Pfad kommt ins Feld.
                        let ziel = wahl.ziel;
                        self.datei_dialog = None;
                        // Release 10.10.2026: derselbe Weg wie der Dialog des
                        // Systems (`files`, `change`, kein Pfad in `value`).
                        self.datei_auswahl_setzen(ziel, vec![pfad]);
                        self.schmutzig = true;
                        return true;
                    }
                }
            }
            _ => {
                self.datei_dialog = Some(wahl);
                return false;
            }
        }
        self.datei_dialog = Some(wahl);
        self.schmutzig = true;
        true
    }

    /// DER DIALOG **ZEICHNEN** — ein Kasten ueber der Seite.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **im** Kasten (halbe Fenstergroesse,
    /// mittig) und nicht als Leiste unten: GEMESSENE ENTSCHEIDUNG, warum
    /// **Text** und keine Liste von Knöpfen: der Dialog wird **mit Tasten**
    /// bedient (siehe `datei_dialog_tasten`), also gehoeren die Namen in
    /// Zeilen.
    fn datei_dialog_zeichnen(&self, liste: &mut DisplayList) {
        let Some(w) = self.datei_dialog.as_ref() else {
            return;
        };
        const FELD_BG: Color = Color::rgb(0x24, 0x24, 0x2c);
        const TEXT_FARBE: Color = Color::rgb(0xf0, 0xf0, 0xf0);
        const PFAD_FARBE: Color = Color::rgb(0xa0, 0xa0, 0xb0);
        const MARK_FARBE: Color = Color::rgb(0x40, 0x50, 0x80);
        let breite = (self.breite as f32 * 0.7).min(720.0).max(240.0);
        let hoehe = (self.hoehe as f32 * 0.7).min(520.0).max(200.0);
        let x = (self.breite as f32 - breite) / 2.0;
        let y = self.chrome.seiten_oberkante() as f32 + (self.hoehe as f32 - hoehe) / 2.0;
        liste.push(narsil_display::DisplayItem::Rect {
            rect: Rect::new(x, y, breite, hoehe),
            color: FELD_BG,
            radius: [6.0; 4],
            deckkraft: 0.98,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
        });
        // Der Pfad (oben) — damit man weiss, **wo** man ist.
        liste.push(narsil_display::DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(x + 12.0, y + 10.0),
            text: w.verzeichnis.to_string_lossy().to_string(),
            color: PFAD_FARBE,
            font_size: 14.0,
            gewicht: 400,
            kursiv: false,
        });
        // Hoechstens `MAX_ZEILEN` Eintraege, ab der Markierung nach oben und
        // unten (sonst laeuft die Liste aus dem Kasten).
        let anzahl = w.eintraege.len();
        let sichtbar = anzahl.min(Dateiwahl::MAX_ZEILEN);
        for i in 0..sichtbar {
            let (name, ist_verzeichnis) = &w.eintraege[i];
            let ly = y + 34.0 + i as f32 * 20.0;
            if i == w.auswahl {
                liste.push(narsil_display::DisplayItem::Rect {
                    rect: Rect::new(x + 6.0, ly - 3.0, breite - 12.0, 20.0),
                    color: MARK_FARBE,
                    radius: [3.0; 4],
                    deckkraft: 1.0,
                    schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                    schatten_anzahl: 0,
                });
            }
            let text = if *ist_verzeichnis {
                format!("{name}/")
            } else {
                name.clone()
            };
            liste.push(narsil_display::DisplayItem::Text {
                // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
                // traegt jetzt die `font-family`-Liste. Hier steht die
                // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
                // Bildschirmpunkte schreibt keine eigene Familie.
                schriftfamilie: String::from("sans-serif"),
                deko: 0,
                origin: Point::new(x + 14.0, ly),
                text,
                color: TEXT_FARBE,
                font_size: 15.0,
                gewicht: 400,
                kursiv: false,
            });
        }
    }

    /// KONTEXT (05.10.2026, 08:10): EIN KLICK AUF EIN **BEDIENELEMENT** —
    /// `range` (setzt den Wert) und `color` (setzt die Farbe).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **hier** und nicht im Formular-Klickpfad
    /// (`formular_bei_klick`, W7C A2, fremde Logik): GEMESSEN ist, dass dort die
    /// Frage „welcher Knoten wurde getroffen?“ beantwortet wird — und ein Regler
    /// braucht die **Stelle auf der Linie**, nicht den Knoten. Zwei verschiedene
    /// Fragen, zwei verschiedene Orte.
    pub fn bedienelement_bei_klick(&mut self, x: f32, y_seite: f32) -> bool {
        let Some(ord) = self.ord_unter(x, y_seite) else {
            return false;
        };
        let art = {
            let Some(a) = self.angezeigt.as_ref() else {
                return false;
            };
            feld_attribut(a, ord, "type")
                .unwrap_or_default()
                .to_lowercase()
        };
        //
        // KONTEXT (05.10.2026, 08:55): `file` oeffnet den **DIALOG** — es hat
        // keinen Wert, den man klicken koennte (der Pfad kommt aus dem
        // Dateisystem, nicht aus dem Klick).
        if art == "file" {
            return self.datei_dialog_oeffnen(ord);
        }
        let wert = match art.as_str() {
            // DER REGLER: die **Stelle auf der Linie** ist der Wert.
            //
            // GEMESSENE ENTSCHEIDUNG, warum **0..100** und nicht `min`/`max`
            // aus dem Attribut: GEMESSEN ist, dass wir den Wert als Text
            // speichern; ein Text, der `7` ist, ist in **jedem** Regler
            // 7 Prozent — und die Attribute zu lesen waere eine zweite
            // Wahrheit ueber dieselbe Zahl.
            //
            // GEMESSENE ENTSCHEIDUNG, warum die **Mitte** der Default ist: das
            // ist der `value` von Firefox, wenn das Attribut fehlt.
            "range" => {
                let (k, breit) = match self.feld_rahmen(ord) {
                    Some(v) => v,
                    None => return false,
                };
                let anteil = (((x - k.x - 4.0) / (breit - 8.0).max(1.0)).clamp(0.0, 1.0) * 100.0)
                    .round() as u32;
                Some(anteil.to_string())
            }
            // DAS FARBFELD: der Klick wechselt in eine **Liste** von Farben.
            // GEMESSENE ENTSCHEIDUNG, warum **nicht** ein Dialog: GEMESSEN ist,
            // dass ein Dialog (also ein Fenster) bei uns nicht gibt (ein
            // X11-Fenster, siehe Bericht KONTEXT_24) — also wird der Wert
            // **hier** gesetzt, aus einer festen Liste, und der Nutzer klickt
            // so lange, bis die Farbe passt.
            "color" => {
                const FARBEN: [&str; 6] = [
                    "#ff0000", "#00ff00", "#0000ff", "#ffff00", "#000000", "#ffffff",
                ];
                let alt = self.farben_index(ord);
                let neu = FARBEN[(alt + 1) % FARBEN.len()];
                self.farben_index_setzen(ord, (alt + 1) % FARBEN.len());
                Some(neu.to_string())
            }
            _ => None,
        };
        let Some(neu) = wert else {
            return false;
        };
        if let Some(a) = self.angezeigt.as_mut() {
            a.patch.attribut.insert((ord, "value".to_string()), neu);
        }
        self.angepasst(true);
        self.schmutzig = true;
        true
    }

    /// DER KASTEN eines Feldes im **Fenster** — fuer Treffer und fuer den
    /// Regler (beides braucht die Stelle, nicht den Knoten).
    fn feld_rahmen(&self, ord: usize) -> Option<(Rect, f32)> {
        let k = self.feld_kasten_im_fenster(ord)?;
        Some((k, k.width))
    }

    /// DIE **ORD-NUMMER** unter einem Punkt — aus dem Knoten, den der Klick
    /// getroffen hat (`Angezeigt::html` traegt die Bruecke).
    fn ord_unter(&self, x: f32, y_seite: f32) -> Option<usize> {
        // GEMESSENE ENTSCHEIDUNG, warum `tiefster_knoten` (die freie Funktion,
        // Zeile 13122) und nicht ein neuer Weg durch den Baum: es ist derselbe
        // Weg, den der Klickpfad schon nimmt (siehe die Aufrufe bei 9820 und
        // 9967) — ein zweiter „welcher Knoten liegt unter dem Punkt?“ waere eine
        // zweite Antwort auf dieselbe Frage.
        let knoten = {
            let a = self.angezeigt.as_ref()?;
            tiefster_knoten_v(&a.baum, x, y_seite, a.versatz as f32)?
        };
        let a = self.angezeigt.as_ref()?;
        a.seite.dom_zu_ord.get(&(knoten.index() as u32)).copied()
    }

    /// WELCHE FARBE gerade **gewählt** ist (Index in der Liste `FARBEN`).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das aus dem **Wert** gelesen wird und
    /// nicht aus einem eigenen Index-Feld: der Wert **ist** die Entscheidung
    /// (er geht beim Absenden mit) — ein zweites Feld waere ein zweiter Ort,
    /// der dieselbe Frage beantwortet.
    fn farben_index(&self, ord: usize) -> usize {
        const FARBEN: [&str; 6] = [
            "#ff0000", "#00ff00", "#0000ff", "#ffff00", "#000000", "#ffffff",
        ];
        let wert = self.feld_wert(ord).to_lowercase();
        FARBEN
            .iter()
            .position(|f| *f == wert)
            .unwrap_or(FARBEN.len() - 1)
    }

    fn farben_index_setzen(&mut self, _ord: usize, _index: usize) {
        // GEMESSENE ENTSCHEIDUNG, warum das **nichts** tut: der Index steht im
        // **Wert** (siehe `farben_index`) — ein zweites Feld waere die zweite
        // Wahrheit. Diese Funktion bleibt, damit die Reihenfolge an **einer**
        // Stelle steht (und als Ort fuer eine spaetere Merkliste).
    }

    /// DER **UEBERZUG** ueber den Feldern: was der Wert eines Feldes **zeigt**.
    ///
    /// KONTEXT (05.10.2026, 07:40, Leitung „Passwortpunkte, Platzhalter grau,
    /// Datum/Farbe/Range — bau selbst“): DER WERT WIRD **UEBER** DER SEITE
    /// GEZEICHNET.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein **Ueberzug** und keine Aenderung an
    /// `js.rs` (wo der Wert in den Anzeigebaum geschrieben wird, KONTEXT_24):
    /// GEMESSEN ist, dass `js.rs` **fremd** und gerade rot ist, und dass der Wert
    /// dort nur als Text ankommt — **ohne die Art des Feldes**. Der Ueberzug liest
    /// die Art aus dem **Attribut** (`Angezeigt::html`, derselbe Weg wie
    /// `autofocus_setzen` und `feld_nur_lesbar`) und liegt damit **komplett in
    /// dieser Datei**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Kasten des Feldes **zuerst** uebermalt
    /// wird: der Wert steht **doppelt** da (einmal aus `js.rs`, einmal von mir) —
    /// bei einem Passwort saehe man sonst **beides**. GEMESSENE ENTSCHEIDUNG,
    /// warum die Farbe fest steht und nicht aus dem Thema kommt: GEMESSEN ist,
    /// dass unsere Eingabefelder **dunkel** sind (der Cursor wird `0xf0f0f0`
    /// gezeichnet, siehe `cursor_rechteck`) — dieselbe Zahl ist das Mass, und
    /// eine eigene Palette waere eine zweite Zahl fuer dieselbe Frage.
    /// **UI-Absprache**: die Oberflaeche darf die Farben spaeter an ihr Thema
    /// haengen (im BRETT, 05.10.).
    fn feld_ueberzug(&self, liste: &mut DisplayList) {
        /// GEMESSENE ENTSCHEIDUNG, warum diese Grenze: 4096 ist die Grenze des
        /// Wertes (`MAX_EINGABE_WERT`) — ein Ueberzug, der laenger waere als der
        /// Wert, zeichnet Zeichen, die es nicht gibt.
        const MAX_UEBERZUG: usize = 64;
        let Some(a) = self.angezeigt.as_ref() else {
            return;
        };
        let felder: Vec<usize> = a.seite.felder.iter().map(|f| f.ord).collect();
        // Release 08.10.2026: alle Feldkaesten in EINEM Baumlauf (vorher je
        // Feld ein eigener Lauf ueber den ganzen Baum — 6,5 ms je Bild auf
        // Wikipedia). Und jetzt mit dem AUSSEHEN des Feldes (`FeldBild`):
        // Flaeche, Farben, Schrift und Radius aus seinem Stil statt fest
        // Weiss mit dunklem Text.
        let bilder = feld_bilder(a, &felder);
        for ord in felder {
            let typ = feld_attribut(a, ord, "type")
                .unwrap_or_default()
                .to_lowercase();
            let tag = a.seite.felder.iter().find(|f| f.ord == ord).map(|f| f.tag).unwrap_or_default();
            if tag != "textarea" && (tag != "input" || !matches!(typ.as_str(), "" | "text" | "search" | "password" | "email" | "url" | "tel" | "number")) {
                continue;
            }
            let wert = self.feld_wert(ord);
            // range und color bekommen eine **Figur**, keinen Text.
            if typ == "range" || typ == "color" {
                continue;
            }
            // Release 15.10.2026: an empty field nobody edited shows the
            // placeholder the PAGE draws (also a placeholder changed by
            // script, the page tree is rebuilt from the live DOM); only an
            // edited field (typed and emptied again) needs the overlay with
            // its placeholder. MEASURED before (github.com hero form): the
            // placeholder a script set came as an overlay in the wrong
            // colours (a black box with black text) over the page's field.
            if wert.is_empty() && !self.feld_bearbeitet(ord) {
                continue;
            }
            let zeichen = feld_zeichen(a, ord, &typ, &wert);
            let Some(bild) = bilder.get(&ord) else {
                continue;
            };
            // Release 08.10.2026: ohne Text kein Ueberzug — die Seite zeichnet
            // das Feld (samt Platzhalter) schon selbst; und ein Feld in einem
            // unsichtbaren Teilbaum (`opacity: 0`) bekommt keinen. GEMESSEN
            // vorher: t-online.de, ein weisses Rechteck ueber dem Platzhalter
            // „Suchbegriff eingeben" und eines aus dem versteckten Klebekopf
            // ueber der oberen Linkzeile.
            if zeichen.is_empty() || !feld_sichtbar(a, ord) {
                continue;
            }
            // Die Flaeche des Feldes in SEINER Farbe und Form (Release
            // 08.10.2026; vorher fest Weiss, eckig, 2 px eingerueckt).
            let f = bild.flaeche;
            if f.width <= 0.0 || f.height <= 0.0 {
                continue;
            }
            // Release 08.10.2026: der sichtbare Ausschnitt — was in den
            // Inhalt passt; im Feld mit Fokus so geschoben, dass der Cursor
            // sichtbar bleibt (wie ein Browser ein langes Feld schiebt).
            // Vorher: die ersten 64 Zeichen, auch ueber den Rand hinaus.
            let cursor = (self.feld_fokus == Some(ord) && !wert.is_empty()).then_some(self.feld_cursor);
            let (_von, sichtbar) = self.feld_ausschnitt(&zeichen, cursor, bild, MAX_UEBERZUG);
            liste.push(narsil_display::DisplayItem::Rect {
                rect: f,
                color: bild.grund,
                radius: bild.radius,
                deckkraft: 1.0,
                schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                schatten_anzahl: 0,
            });
            liste.push(narsil_display::DisplayItem::Text {
                // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
                // traegt jetzt die `font-family`-Liste. Release 08.10.2026:
                // die Familie, Groesse und Staerke des Feldes selbst, und
                // die Stelle, an der die Seite den Text setzt.
                schriftfamilie: bild.familie.clone(),
                deko: 0,
                origin: Point::new(bild.text_x, bild.text_y),
                text: sichtbar,
                color: if wert.is_empty() {
                    bild.platzhalter_farbe()
                } else {
                    bild.farbe
                },
                font_size: bild.schrift,
                gewicht: bild.gewicht,
                kursiv: false,
            });
        }
        self.regler_ueberzug(liste);
    }

    /// Release 08.10.2026: DER SICHTBARE AUSSCHNITT eines Feldtextes —
    /// `(erstes Zeichen, Text)`. Hoechstens `max` Zeichen um den Cursor,
    /// gekuerzt auf die Breite des Inhalts. Mit Cursor wird von links
    /// gekuerzt, bis der Text bis zum Cursor passt (das Feld „schiebt"),
    /// ohne Cursor von rechts. Ueberzug und Cursor rufen dieselbe Funktion,
    /// damit der Strich am Zeichen steht.
    fn feld_ausschnitt(&self, zeichen: &str, cursor: Option<usize>, bild: &FeldBild, max: usize) -> (usize, String) {
        let alle: Vec<char> = zeichen.chars().collect();
        let n = alle.len();
        let anker = cursor.map(|c| c.min(n));
        let (mut von, mut bis) = match anker {
            Some(c) => (c.saturating_sub(max), (c + max).min(n)),
            None => (0, max.min(n)),
        };
        let platz = bild.inhalt_rechts - bild.text_x;
        // Ohne messbaren Platz (Kasten ohne Inhalt): nichts kuerzen.
        if !platz.is_finite() || platz < bild.schrift {
            return (von, alle[von..bis].iter().collect());
        }
        let breite = |a: usize, b: usize| -> f32 {
            let s: String = alle[a..b].iter().collect();
            self.feld_text_breite(&s, bild)
        };
        if let Some(c) = anker {
            while von < c && breite(von, c) > platz {
                von += 1;
            }
            while bis > c && breite(von, bis) > platz {
                bis -= 1;
            }
        } else {
            while bis > von && breite(von, bis) > platz {
                bis -= 1;
            }
        }
        (von, alle[von..bis].iter().collect())
    }

    /// Release 08.10.2026: die Breite eines Feldtextes mit Familie, Groesse
    /// und Staerke des Feldes — dieselbe Wahl, mit der der Raster zeichnet
    /// (`breite_stil_familie`). GEMESSEN vorher mit `breite(.., 16)`: auf
    /// der Startseite (`system-ui`) stand der Cursor im letzten Buchstaben.
    fn feld_text_breite(&self, text: &str, bild: &FeldBild) -> f32 {
        self.messung
            .breite_stil_familie(text, bild.schrift, bild.gewicht, false, &bild.familie)
    }

    /// DER **REGLER** (`<input type=range>`) und das **FARBFELD**
    /// (`<input type=color>`) — zwei Figuren, kein Text.
    ///
    /// KONTEXT (05.10.2026, 07:45): GEMESSENE ENTSCHEIDUNG, warum **zwei**
    /// Figuren und nicht noch ein Feld wie die anderen: ein `range` ist ein
    /// **Strich mit einem Griff**, ein `color` ist ein **farbiges Quadrat** —
    /// sie als „Feld mit Text“ zu malen waere eine Luege in Form von drei
    /// gleich aussehenden Kaesten.
    fn regler_ueberzug(&self, liste: &mut DisplayList) {
        const REGLER_FARBE: Color = Color::rgb(0x60, 0x60, 0x70);
        const GRIFF_FARBE: Color = Color::rgb(0xe0, 0xe0, 0xf0);
        let Some(a) = self.angezeigt.as_ref() else {
            return;
        };
        let felder: Vec<usize> = a.seite.felder.iter().map(|f| f.ord).collect();
        let kaesten = feld_kaesten_alle(a);
        for ord in felder {
            let typ = feld_attribut(a, ord, "type")
                .unwrap_or_default()
                .to_lowercase();
            if typ != "range" && typ != "color" {
                continue;
            }
            let Some(k) = kaesten.get(&ord).copied() else {
                continue;
            };
            let wert = self.feld_wert(ord);
            match typ.as_str() {
                "range" => {
                    // GEMESSENE ENTSCHEIDUNG, warum der Wert eine Zahl
                    // **0..100** ist: GEMESSEN ist, dass wir den Wert als
                    // **Text** speichern (`patch.attribut`), und ein Text
                    // braucht zum Zeichnen eine Zahl. 0..100 ist die Form, in
                    // der Firefox ohne Lesen der Attribute `min`/`max` darstellt.
                    let anteil = wert
                        .trim()
                        .parse::<f32>()
                        .map(|v| (v / 100.0).clamp(0.0, 1.0))
                        .unwrap_or(0.5);
                    let mitte = k.y + k.height / 2.0;
                    liste.push(narsil_display::DisplayItem::Rect {
                        rect: Rect::new(k.x + 4.0, mitte - 2.0, (k.width - 8.0).max(1.0), 4.0),
                        color: REGLER_FARBE,
                        radius: [2.0; 4],
                        deckkraft: 1.0,
                        schatten: [narsil_core::style::SchattenArt::LEER;
                            narsil_core::style::MAX_SCHATTEN],
                        schatten_anzahl: 0,
                    });
                    let gx = k.x + 4.0 + (k.width - 8.0) * anteil;
                    liste.push(narsil_display::DisplayItem::Rect {
                        rect: Rect::new(gx - 5.0, mitte - 7.0, 10.0, 14.0),
                        color: GRIFF_FARBE,
                        radius: [3.0; 4],
                        deckkraft: 1.0,
                        schatten: [narsil_core::style::SchattenArt::LEER;
                            narsil_core::style::MAX_SCHATTEN],
                        schatten_anzahl: 0,
                    });
                }
                "color" => {
                    // Ohne Wert: **schwarz** (siehe `wert_color`) — ein
                    // schwarzes Quadrat waere ein Feld, das nichts anzeigt.
                    let (r, g, bl) = wert_color(&wert);
                    liste.push(narsil_display::DisplayItem::Rect {
                        rect: k,
                        color: Color::rgb(r, g, bl),
                        radius: [2.0; 4],
                        deckkraft: 1.0,
                        schatten: [narsil_core::style::SchattenArt::LEER;
                            narsil_core::style::MAX_SCHATTEN],
                        schatten_anzahl: 0,
                    });
                }
                _ => {}
            }
        }
    }

    fn text_unter_punkt(&self, x: f32, y_seite: f32) -> String {
        // GEMESSENE EINORDNUNG des Versatzes: die Anzeigeliste zeichnet in
        // **Bildschirm**-Koordinaten (der Seiteninhalt beginnt bei
        // `SEITEN_START_Y`), und `y_seite` ist bereits um `SEITEN_START_Y` und den
        // Scrollversatz gebueckt — darum kommt der Versatz hier **nicht** noch
        // einmal dazu.
        let versatz = self
            .angezeigt
            .as_ref()
            .map(|a| a.versatz as f32)
            .unwrap_or(0.0);
        //
        // GEMESSENER FEHLER, den diese Rechnung hatte (22:15, `NARSIL_L1_LABEL`):
        // `text` kam **leer** zurueck. GEMESSENE GRUNDLAGE: die Anzeigeliste
        // zeichnet in **Bildschirm**-Koordinaten — ein Text stand bei
        // `origin.y` = 148 bei `SEITEN_START_Y` = 108 —, und `y_seite` ist
        // **Seiten**-Koordinaten (`klick` rechnet `y - SEITEN_START_Y +
        // versatz`). Mit `bild_y = y_seite + versatz` wurde aus 156 die Zahl 48,
        // der Vergleich `[143, 170]` schlug fehl, also **kein** Text.
        //
        // GEMESSENE ENTSCHEIDUNG, warum zurueckgerechnet wird und nicht die
        // Liste umgebaut: `origin` ist die **gezeichnete** Stelle, und ein Text
        // ohne Kasten (der Kern des Problems) hat nur `origin`.
        let bild_y = y_seite + self.chrome.seiten_oberkante() as f32 - versatz;
        for item in self.sichtliste().iter() {
            if let narsil_display::DisplayItem::Text {
                origin,
                text,
                font_size,
                ..
            } = item
            {
                let hoehe = font_size * 1.4;
                if x >= origin.x
                    && x <= origin.x + (text.chars().count() as f32) * font_size * 0.55
                    && bild_y >= origin.y - font_size * 0.3
                    && bild_y <= origin.y + hoehe
                {
                    return text.clone();
                }
            }
        }
        String::new()
    }

    /// KOMPRESS 04.10.2026 (Anordnung der Leitung): der Klick auf den
    /// **Play-Knopf** eines `<audio>`-Steuers.
    ///
    /// GEMESSENER WEG, Schritt fuer Schritt:
    ///
    /// 1. Der Text unter dem Punkt muss `[Knopf: ▶ N]` sein — GEMESSEN ist
    ///    das die Beschriftung, die `narsil-page` schreibt (ein `<button>`
    ///    wird als `[Knopf: {text}]` gezeichnet, und der Play-Knopf traegt
    ///    den Zaehler).
    /// 2. Das **`N.`-te** `<audio>`-Element dieser Seite, in
    ///    **Dokumentordnung** — GEMESSEN aus `Angezeigt.html`, dem
    ///    **geparsten** Baum (der die **Attribute** traegt; `narsil_dom`
    ///    traegt keine).
    /// 3. Der Pfad **aufgeloest** gegen die Basis der Seite, weil `src`
    ///    relativ zur Seite gemeint ist.
    /// 4. `Spieler::mit_kindprozess(daten, &Weg::automatisch())` — GEMESSEN
    ///    auf dieser Maschine `Weg PwPlay`; `schritt()` fuettert den
    ///    Kindprozess, und **das** ist der Weg zum Lautsprecher.
    ///
    /// GEMESSENE EINEINSCHRAENKUNG, und sie gehoert in jeden Bericht: die
    /// Wiedergabe laeuft **hier**, im Klickpfad, also **blockiert** sie das
    /// Fenster fuer die Dauer des Tons. Das ist **kein** Fehler, sondern eine
    /// bekannte Grenze dieses Wegs; ein Thread waere der naechste Schritt
    /// und ist **nicht** gebaut.
    fn audio_klick(&mut self, label: &str) -> bool {
        const MARKE: &str = "[Knopf: \u{25b6} ";
        // **GEMESSENE DIAGNOSE, und sie ist der Grund fuer diese Zeile:** am
        // 04.10. 09:02 kam nach einem Klick auf (602,523) **keine** `TON:`-
        // Zeile. Also ist entweder der Text unter dem Punkt nicht die Marke,
        // oder der Klick kommt gar nicht an. GEMESSEN wird deshalb **jeder**
        // Text, den ein Seitenklick sieht — und die Marke wird **nicht**
        // vorausgesetzt.
        eprintln!(
            "[{}] KLICK-LABEL: `{}`",
            zeit_stempel(),
            &label.chars().take(60).collect::<String>()
        );
        let Some(rest) = label.strip_prefix(MARKE) else {
            return false;
        };
        let Some(nummer) = rest
            .split(']')
            .next()
            .and_then(|z| z.trim().parse::<usize>().ok())
        else {
            return false;
        };
        // **DAS N-TE `<audio>`, in Dokumentordnung.**
        let mut quellen: Vec<String> = Vec::new();
        sammle_audio(
            &self
                .angezeigt
                .as_ref()
                .map(|a| a.html.as_slice())
                .unwrap_or(&[]),
            &mut quellen,
        );
        eprintln!(
            "[{}] TON-KLICK: Nummer {nummer}, {} <audio>-Elemente auf der Seite",
            zeit_stempel(),
            quellen.len()
        );
        let Some(src) = quellen.get(nummer - 1) else {
            eprintln!(
                "[{}] TON-KLICK: Nummer {nummer} gibt es nicht ({})",
                zeit_stempel(),
                quellen.len()
            );
            self.meldung = Some(trf!(
                "Audio {} does not exist (the page has {})",
                nummer,
                quellen.len()
            ));
            return true;
        };
        // **DER PFAD, und er wird **gemessen**, nicht geraten:** `src` ist
        // relativ zur Seite. Also: `file://` aufloesen, sonst als Pfad
        // neben der Seite.
        let basis = self
            .angezeigt
            .as_ref()
            .map(|a| a.seite.basis.clone())
            .unwrap_or_default();
        let pfad = pfad_relativ(&basis, src);
        // **KOMPRESS 04.10.2026: DER ZWEITE KLICK PAUSIERT.** GEMESSEN ist,
        // dass die Marke `[Knopf: \u{25b6} N]` **eine** ist fuer **beide**
        // Zustaende — GEMESSEN im Bild (`BERICHT_KOMPRESS_G9am.txt`):
        // `[Knopf: \u{25b6} 1] [Audio: ton.wav]`. **Also** ist der zweite
        // Klick auf **denselben** Knopf **Pause**, und der dritte
        // **Fortsetzen** — das ist `crate::audio::Zustand::pause`, das
        // GEMESSEN **beide** Richtungen kann (G9aq).
        //
        // **GEMESSENE ENTSCHEIDUNG, warum **vor** dem Lesen der Datei:** wer
        // pausiert, **liest nicht**. Sonst wuerde der zweite Klick auf Play
        // **noch einmal** `pw-play` starten — und der **laufende** Ton
        // waere **zwei** Streams auf demselben Sink.
        if self.audio.laeuft(nummer) {
            eprintln!(
                "[{}] TON-KLICK: Nummer {nummer} laeuft — PAUSE",
                zeit_stempel()
            );
            self.audio.pause(nummer);
            self.schmutzig = true;
            self.meldung = Some(trf!("Sound {} paused", nummer));
            return true;
        }
        eprintln!("[{}] TON-KLICK: src `{src}` -> `{pfad}`", zeit_stempel());
        match std::fs::read(&pfad) {
            Ok(daten) => {
                let groesse = daten.len();
                // **KOMPRESS 04.10.2026 (Anordnung der Leitung, 14:50): die
                // Anbindung an `crate::audio::Zustand` — und der Grund fuer
                // **diesen** Umbau statt eines neuen Aufrufs:
                //
                // **GEMESSENER FEHLER, und er ist der schwerste der ganzen
                // Kette:** die alte Fassung hatte
                // ```text
                //     while sp.zustand() != Zustand::Fertig {
                //         let n = sp.schritt();
                //         ...
                //     }
                // ```
                // **also gewartet, bis der Ton `Fertig` ist.** Bei
                // `ton.wav` sind das GEMESSEN **3,000000 s**
                // (`BERICHT_KOMPRESS_G9am.txt`), bei einem **Musikstueck**
                // von **140 s** (GEMESSEN in G9ap, MP3 aus
                // `/home/jaff/Videos/`) waeren es **140 s**, in denen
                // **kein** Klick, **kein** Scrollen und **keine** Eingabe
                // **ankommt**. **Die Oberflaeche steht.**
                //
                // **GEMESSENE ENTSCHEIDUNG: `crate::audio::Zustand` haelt
                // den Spieler und KEHRT SOFORT ZURUECK.** Der Ton laeuft im
                // Kindprozess (`pw-play`) weiter, und die Leiste liest
                // `position_s()`/`dauer_s()` bei jedem Bild.
                //
                // **UND** die alte Fassung baute **zwei** Spieler: **hier**
                // einen (`Spieler::mit_kindprozess`), der **wegfiel**, und
                // **dort** (`crate::audio`) einen **zweiten**. Also **zwei**
                // `pw-play` **nacheinander**, von denen der **erste** **nie**
                // Samples bekam. **Also** entfaellt der erste **ganz** — und
                // **genau** hier hängt jetzt der einzige Aufruf.
                match self.audio.abspielen(nummer, &daten) {
                    Ok(format) => {
                        eprintln!(
                            "[{}] TON: abgespielt, Format {:?}, Dauer {:.4} s, Spieler {}, {} Byte",
                            zeit_stempel(),
                            format,
                            self.audio.dauer_s(nummer).unwrap_or(0.0),
                            self.audio.anzahl_spieler(),
                            groesse
                        );
                        self.schmutzig = true;
                        self.meldung = Some(trf!(
                            "Sound {} played ({} bytes, {})",
                            nummer,
                            groesse,
                            format!("{format:?}")
                        ));
                    }
                    Err(e) => {
                        eprintln!("[{}] TON: Spieler nicht baubar: {e}", zeit_stempel());
                        self.meldung = Some(trf!("Cannot play the sound: {}", e));
                    }
                }
            }
            Err(e) => {
                eprintln!("[{}] TON: Datei nicht lesbar: {e}", zeit_stempel());
                self.meldung = Some(trf!("Cannot read the audio file: {}", e));
            }
        }
        true
    }

    pub fn klick(&mut self, x: f32, y: f32) -> bool {
        // VIDEO_19 (04.10.2026): DAS VIDEO, wenn der Klick darin liegt.
        //
        // GEMESSENE ENTSCHEIDUNG, warum hier **und nicht** weiter unten:
        // auf einer Seite mit einem Video ist die meiste Bildflaeche **nicht**
        // die Leiste, und ein Klick irgendwo auf der Seite, der die Zeit
        // springen liesse, wuerde jedes Bild unerreichbar machen. Wer erst
        // den Layout-Baum abfragt und dann das Video, hat dem Video
        // schon die Seite unter sich gemacht.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `schmutzig` bei einer **Wirkung**
        // gesetzt wird, aber nicht bei `Aktion::Nichts`: ein Klick, der
        // nichts tut, soll kein Bild kosten.
        if let Some(v) = self.video.as_mut() {
            let jetzt = crate::video::jetzt_ns();
            if let Some(a) = v.klick(x as u32, y as u32, jetzt) {
                if a != narsil_media::bedienung::Aktion::Nichts {
                    self.schmutzig = true;
                    return true;
                }
            }
        }
        // 1. Leiste: dieselbe Pruefung wie in `mausklick` — ein Klick auf
        // einen Knopf oder die Adressleiste gehoert dem Fenster, nicht
        // der Seite (GEMESSEN: `narsil_chrome::Chrome` hat kein `trifft`,
        // die Rechtecke stehen in `knopf()` und `adressleiste()`).
        if y < self.chrome.seiten_oberkante() as f32 {
            return false;
        }
        // DIE KOORDINATEN: Der Bildschirm hat die Leiste oben (`y < 80`),
        // darunter die Seite. `versatz` ist die Scrollposition und wird
        // ADDIERT (nach unten scrollen heisst: der Inhalt rueckt nach
        // oben, der Punkt wandert relativ zum Inhalt nach unten).
        // GEMESSEN, warum die Klammern noetig sind: `SEITEN_START_Y` ist
        // ein `u32`, und `f32 - u32` gibt es nicht.
        let y_seite =
            y - self.chrome.seiten_oberkante() as f32 + self.angezeigt.as_ref().map_or(0.0, |a| a.versatz as f32);
        let knoten = self
            .angezeigt
            .as_ref()
            .and_then(|a| tiefster_knoten_v(&a.baum, x, y_seite, a.versatz as f32));
        let Some(knoten) = knoten else {
            return false;
        };
        //
        // W7C A2 (06:25): DIE REIHENFOLGE EINES KLICKS.
        //
        // GEMESSENER FEHLER, vom Test gefunden: `<button>` wurde zum
        // Absender, und der Absender lief VOR dem Handler. Die drei Tests
        // in `erzeugte_liste.rs` (ein JavaScript-Zaehler an einem Knopf)
        // zeigten danach „Stand: 0" — der Klick hat das Formular
        // abgeschickt, statt den Handler zu rufen. Ein Skript, das einen
        // Klick abfaengt, kann in keinem Browser trotzdem eine Navigation
        // oder ein Absenden ausloesen; `preventDefault` heisst
        // „nicht die Standardhandlung".
        //
        // | Schritt | was | warum so
        // |---|---|---|
        // | 1 | `fokus_bei_klick` | der Cursor geht vor allem anderen hinein |
        // | 2 | `handler_ausloesen` | ein Skript darf alles verhindern |
        // | 3 | `formular_bei_klick` | Haken, Auswahl, Absenden — nur wenn nicht verhindert |
        // | 4 | Link | nur wenn nicht verhindert |
        // | 5 | sonst nichts | |
        //
        // GEMESSEN, warum der Handler EINMAL laeuft und nicht zweimal
        // (hier und im Link-Zweig): zweimal heisst zweimal `console.log`,
        // zweimal eine Aktion, zweimal ein Zaehler. Ein Handler, der
        // zweimal laeuft, ist ein Handler, dem niemand mehr traut.
        // 02.10. 22:00 (HTML): DER TEXT UNTER DEM PUNKT — fuer `<label>`
        // (siehe `text_unter_punkt`). GEMESSEN, warum er **vor** dem Aufruf
        // gelesen wird: `fokus_bei_klick` braucht `&mut self` (es setzt den
        // Fokus), und `sichtliste()` braucht `&self`.
        //
        //
        // KONTEXT (05.10.2026, 09:00): EIN KLICK IM **DIALOG** waehlt eine Zeile.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Dialog **auch** mit der Maus geht,
        // obwohl er Tasten hat: GEMESSEN ist, dass der Dialog die **ganze**
        // Seitenflaeche bedeckt — ein Modus, in dem die Maus nichts tut, ist ein
        // Modus, in dem man auf ein Fenster schaut und sich aergert.
        if self.datei_dialog_ist_offen() {
            self.datei_dialog_mausklick(x, y_seite);
            return true;
        }
        self.leiste_im_fokus = false;
        self.handler_ausloesen(knoten, x, y_seite);
        // Release 10.10.2026: ein Handler, der `input.click()` auf einem
        // Dateifeld ruft (gestaltete Hochlade-Knoepfe), bekommt den Dialog
        // sofort — der Klick ist die Nutzeraktion.
        self.datei_dialog_wunsch_abholen();
        if self.navigation_verhindert() { return true; }
        // A callback may have changed the layout; use the current hit for defaults.
        let knoten = self.angezeigt.as_ref()
            .and_then(|a| tiefster_knoten_v(&a.baum, x, y_seite, a.versatz as f32)).unwrap_or(knoten);
        if self.bedienelement_bei_klick(x, y_seite) { return true; }
        let label_text = self.text_unter_punkt(x, y_seite);
        if self.fokus_bei_klick(knoten, &label_text, x) { return true; }
        let y_seite_jetzt =
            y - self.chrome.seiten_oberkante() as f32 + self.angezeigt.as_ref().map_or(0.0, |a| a.versatz as f32);
        let knoten = self
            .angezeigt
            .as_ref()
            .and_then(|a| tiefster_knoten_v(&a.baum, x, y_seite_jetzt, a.versatz as f32))
            .unwrap_or(knoten);
        let label_text = self.text_unter_punkt(x, y_seite_jetzt);
        if self.formular_bei_klick(knoten, &label_text) {
            return true;
        }
        // 4. Link. Release 08.10.2026: und der Kasten eines Block-Links
        // (`<a style="display:block">`, Kachel, Karte) ausserhalb seines
        // Textes — siehe `anker_ueber`.
        let adresse = self.angezeigt.as_ref().and_then(|a| {
            a.seite
                .adresse_von(knoten)
                .map(str::to_string)
                .or_else(|| anker_ueber(a, knoten))
        });
        if let Some(ziel) = adresse {
            // Release 14.10.2026: a `javascript:` link runs its code in the
            // page (HTML §7.4.2.3.4 „evaluate a javascript: URL"). MEASURED
            // before: the click tried to load „javascript:…" as an address
            // and only reported a link error.
            if ziel.get(..11).is_some_and(|p| p.eq_ignore_ascii_case("javascript:")) {
                let code = match narsil_url::prozent_dekodieren(&ziel[11..]) {
                    narsil_url::Dekodiert::Text(t) => t,
                    narsil_url::Dekodiert::Bytes(b) => String::from_utf8_lossy(&b).into_owned(),
                };
                let _ = self.seiten_js(&code);
                self.schmutzig = true;
                return true;
            }
            //
            // 02.10. 07:55: EIN ANKER SPRINGT, ER LÄDT NICHT.
            //
            // GEMESSENER BEFUND (07:54, `tests/_m_anker.rs`, entfernt):
            // `<a href="#ziel">` auf einer Seite mit 3 000 px Fuellung:
            //
            //     vorher: versatz=0  browser=file:///…/m_anker.html
            //     Klick:  geklickt=true  versatz=**0**  browser=**unveraendert**
            //             meldung=**None**
            //
            // Der Klick wurde also **angenommen** und tat **nichts**: kein
            // Sprung, keine neue Adresse, keine Meldung. `gehe_zu("#ziel")`
            // kann mit einem Fragment nichts anfangen — es ist keine Adresse
            // mit Schema. Auf wikipedia ist das der Inhaltsverzeichnis-Link,
            // auf jeder Seite mit Inhaltsverzeichnis dasselbe.
            //
            // GEMESSENE EINORDNUNG, warum `Seite::ids` die Sache entscheidet:
            // die Id-Tabelle gibt es bereits (W6/J9), sie wurde nur nirgends
            // gefragt. Vorher eine eigene Tabelle waere eine zweite Wahrheit
            // ueber „welches Element ist das".
            //
            // GEMESSENE EINORDNUNG, warum ein **anderes** Dokument mit
            // Fragment weiterhin geladen wird: das ist Browser-Verhalten
            // (Firefox laedt neu, wenn das Ziel nicht dieselbe Seite ist).
            if self.anker_sprung(&ziel) {
                self.schmutzig = true;
                return true;
            }
            //
            // GEMESSENE ENTSCHEIDUNG, warum ein Fehler HIER sichtbar wird:
            // `let _ = self.gehe_zu(&ziel)` hat das Ergebnis weggeworfen, und
            // GEMESSEN ist `meldung = None` nach einem Klick, der nirgends
            // hinfuehrte. Ein Klick, der ins Leere geht, muss sagen, dass er
            // ins Leere ging — sonst ist die Seite unbedienbar und der Nutzer
            // glaubt, er habe falsch geklickt.
            if let Err(e) = self.gehe_zu(&ziel) {
                self.meldung = Some(std::format!("Link: {e}"));
            }
            self.schmutzig = true;
            return true;
        }
        // 5. DER PLAY-KNOF DES `<audio>`-STEUERS (KOMPRESS 04.10.2026).
        //
        // GEMESSENER ZUSTAND VORHER: `narsil-page` schrieb fuer `<audio>`
        // einen Platzhalter und einen Zeitknoten, aber **nichts, was man
        // anklicken konnte**. Also gab es keinen Weg, vom Browser aus Ton
        // zu starten — und die Abnahme der Leitung („Play druecken, Ton
        // kommt aus dem Lautsprecher") war **nicht erfuellbar**, egal wie
        // gut der Dekoder ist.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Haken der **Text** ist und
        // keine `id`: `narsil_dom` speichert **keine** Attribute (GEMESSEN:
        // `NodeKind` kennt nur `Page | Container | Text`), also kann der
        // Browser einen Knoten **nicht** ueber eine `id` finden. Der
        // Ordinal zaehlt **alle** Elemente und steht **nicht** im
        // `HtmlNode`-Baum. Also steht die Nummer im Text: `[Knopf: ▶ 2]`.
        //
        // GEMESSENE REIHENFOLGE, und sie ist die von `klick()`: Formular
        // (3) und Link (4) sind **Standardhandlungen**; der Ton ist **keine**
        // Standardhandlung eines Dokuments, sondern etwas, das nur dieser
        // Browser kann. Er kommt **danach** — sonst wuerde ein `<audio>` in
        // einem Formular den Absender ausloesen statt den Ton.
        if self.audio_klick(&label_text) {
            return true;
        }

        true
    }

    /// Tag und Text des tiefsten Knotens an einer Stelle der Seite.
    ///
    /// GEMESSEN, warum `tiefster_knoten` und nicht der oberste: getroffen
    /// wird der Kasten des TEXTES; der Tag, den ein Skript mit `id`
    /// benutzt, ist der des ELEMENTS. Steht der Treffer im Weg, wird der
    /// erste Vorfahr mit einem TAG genommen, der kein `p`/`div`-Kasten um
    /// den Text ist — gemessen daran, dass ein `<button>`-Klick ohne
    /// Vorfahr-Suche nie ankommt.
    ///
    /// GEMESSENER FEHLER (10:35), und deshalb ist es nur der TEXT: der
    /// Tag steht nicht im `narsil-dom`-Knoten. `NodeKind` kennt `Page`,
    /// `Container` und `Text` — keinen Elementnamen (gemessen in
    /// `narsil-dom/src/lib.rs:242`). `narsil-page` haelt keine
    /// `HashMap<NodeId, String>` fuer Tags (nur `links`). Wer hier einen
    /// Tag erfinden wuerde, schickt den Klick an ein Element, das es
    /// nicht gibt.
    ///
    /// DER WEG, der stattdessen trägt: der TEXT des getroffenen Knotens,
    /// und die Suche im Baum des Motors ueber ALLE Elementtypen
    /// (`Bruecke::id_von_text_ohne_tag`). Mehrdeutigkeit wird abgelehnt —
    /// ein Text, der zweimal im Bild steht, loest keinen Handler aus.
    fn knoten_bei(&self, x: f32, y: f32) -> Option<String> {
        let a = self.angezeigt.as_ref()?;
        let id = tiefster_knoten_v(&a.baum, x, y, a.versatz as f32)?;
        let doc = &a.seite.document;
        let knoten = doc.nodes().iter().find(|n| n.id == id)?;
        let text = knoten.text.clone().unwrap_or_default();
        if text.trim().is_empty() {
            return None;
        }
        Some(Self::platzhalter_abschneiden(&text))
    }

    /// `[Knopf: Kaputt]` -> `Kaputt`.
    ///
    /// GEMESSEN (05:50, mit `tests/_mess_kaputt.rs`): im BILD steht der
    /// Text eines Knopfes als `[Knopf: Kaputt]` — das ist der Platzhalter,
    /// mit dem `narsil-page` einen Knopf zeichnet (es gibt dort noch keinen
    /// Knopfrahmen, siehe UI1). Die Bruecke sucht aber im Baum des MOTORS
    /// nach `textContent`, und der heisst `Kaputt`. Ohne dieses Abschneiden
    /// findet die Suche nichts, der Klick faellt auf den alten Weg
    /// zurueck, und die Bruecke sieht aus, als waere sie nicht verdrahtet.
    ///
    /// GEMESSEN, warum KEIN Raten: der Text im Bild wird nicht erfunden,
    /// sondern abgeschnitten — die Klammern stehen in `narsil-page`, und
    /// `tiefster_knoten` liefert genau den Knoten, dessen Text gezeichnet
    /// wurde. Also: was im Bild steht, minus was der Zeichner davor
    /// geschrieben hat.
    ///
    /// GEMESSEN, warum ein Text ohne Klammern unveraendert bleibt: die
    /// meisten Knoten tragen normalen Text, und jeder Versuch, „irgendwas"
    /// zu normalisieren, wuerde die Suche aufloesen.
    fn platzhalter_abschneiden(text: &str) -> String {
        let t = text.trim();
        if let Some(rest) = t.strip_prefix('[')
            && let Some(innen) = rest.strip_suffix(']')
            && let Some((_, nachher)) = innen.split_once(": ")
        {
            return nachher.trim().to_string();
        }
        t.to_string()
    }

    /// Hat ein Handler in DIESEM Klick `preventDefault()` gerufen?
    ///
    /// GEMESSEN, warum das eine Methode und kein Feldzugriff ist (14:55):
    /// `default_verhindert` steht hinter einem `Mutex` im DOM-Stand, und
    /// der Aufrufer fragt es an zwei Stellen (Link, Bildschirm). Wer
    /// direkt sperrt, muss an beiden Stellen an die Sperre denken.
    fn navigation_verhindert(&self) -> bool {
        self.angezeigt
            .as_ref()
            .map(|a| a.stand.lock().expect("DomStand").default_verhindert)
            .unwrap_or(false)
    }

    /// Die Vorlauf-Nummern von einem Knoten nach oben (Bubble, J10).
    ///
    /// GEMESSEN, warum nach oben und nicht nur der getroffene Knoten
    /// (16:50): getroffen wird der Kasten des TEXTES, und der gehoert zum
    /// Absatz, nicht zum Element, bei dem der Handler haengt.
    ///
    /// M3/M5: `MAX_KETTE` (16) Knoten — eine DOM-Kette von 16 ist tief
    /// genug (`MAX_BAU_TIEFE` = 64 beim Bau).
    fn klick_ziele(&self, knoten: narsil_dom::NodeId) -> Vec<usize> {
        let Some(a) = self.angezeigt.as_ref() else {
            return Vec::new();
        };
        let doc = &a.seite.document;
        let mut raus: Vec<usize> = Vec::new();
        let mut cur = knoten;
        for _ in 0..doc.len().min(crate::js::MAX_SCAN) {
            if let Some(o) = a.seite.dom_zu_ord.get(&(cur.index() as u32)) {
                raus.push(*o);
            }
            match doc.get(cur).and_then(|n| n.parent) {
                Some(el) => cur = el,
                None => break,
            }
        }
        raus
    }

    /// Die Adresse kurz genug fuer eine Meldung.
    ///
    /// GEMESSEN, warum eine Kuerzung und nicht die ganze Adresse: die
    /// Statuszeile ist eine Zeile breit, und `file:///home/jaff/.../r4_0.
    /// html` ist langer als das Fenster. 48 Zeichen passen in jede Leiste.
    fn kurz_adresse(adresse: &str) -> String {
        const MAX: usize = 48;
        if adresse.chars().count() <= MAX {
            return adresse.to_string();
        }
        let kopf: String = adresse.chars().take(MAX - 3).collect();
        std::format!("{kopf}...")
    }

    /// UI3: DIE FEHLERSEITE — verstaendlich statt technisch.
    ///
    /// DIE DREI TEILE, und jeder hat einen gemessenen Grund:
    ///
    /// 1. EINE UEBERSCHRIFT, die sagt, was passiert ist, nicht warum:
    ///    GEMESSEN, warum nicht der Originaltext zuerst — `Fehler::
    ///    Seite("timeout")` sagt einem Menschen nichts. Wer mit einer
    ///    Meldung beginnt, die er nicht lesen kann, liest den Rest auch
    ///    nicht.
    ///
    /// 2. EIN SATZ ZUM TUN, und die Unterscheidung ist keine Vermutung:
    ///    `Fehler::Adresse` ist per Definition der Tippfehler (der Nutzer
    ///    hat es falsch getippt), `Fehler::Seite` der Serverfehler (die
    ///    Adresse war richtig). Beides steht in der Doku des `enum` — eine
    ///    Behauptung mit Quelle.
    ///
    /// 3. DER TECHNISCHE TEXT, klein, ganz unten. GEMESSEN, warum er
    ///    hineingehört: ohne ihn kann niemand einen gemeldeten Fehler
    ///    nachschlagen, und dieses Projekt arbeitet nach Messungen.
    ///
    /// M3: 8 KiB (`MAX_FEHLERSEITE`), und die Adresse wird escaped — sie
    /// kommt aus dem Tippfeld des Nutzers und darf kein HTML sein.
    fn fehlerseite(adresse: &str, fehler: &Fehler) -> String {
        /// M3: die Fehlerseite ist eine Anzeige, kein Dokument.
        const MAX_FEHLERSEITE: usize = 8 * 1024;
        let (was, tat, knopf) = match fehler {
            Fehler::Adresse(_) => (
                tr("This address cannot be opened."),
                tr("Check the address: a scheme is missing (e.g. http://), or the address is incomplete."),
                tr("Home page"),
            ),
            Fehler::Seite(_) => (
                tr("This page could not be loaded."),
                tr("The server did not answer, or the connection was interrupted. A network error does not change your address."),
                tr("Try again"),
            ),
        };
        // Changed: decided by the error kind, not by the (translated) label.
        let ziel = if matches!(fehler, Fehler::Adresse(_)) {
            "narsil://start".to_string()
        } else {
            escape_html(adresse)
        };
        let mut s = String::with_capacity(1024);
        // Release 07.10.2026: der Tab trug hier „Narsil – Startseite" — auch
        // bei einem Ladefehler. Titel = die Überschrift; Schrift wie die
        // Startseite (ohne Angabe gilt `serif`).
        s.push_str(&std::format!(
            "<title>{}</title><style>body{{font-family:system-ui,sans-serif}}</style>",
            escape_html(was)
        ));
        s.push_str("<h1 style=\"font-size:24px;margin:32px 24px 8px 24px\">");
        s.push_str(was);
        s.push_str("</h1>");
        s.push_str(&std::format!(
            "<p style=\"margin:0 24px 16px 24px;color:#334\">{}</p>",
            escape_html(adresse)
        ));
        s.push_str(&std::format!(
            "<p style=\"margin:0 24px 24px 24px;color:#334\">{}</p>",
            tat
        ));
        s.push_str(&std::format!(
            "<p style=\"margin:0 24px 24px 24px\">\
             <a href=\"{ziel}\" style=\"padding:8px 16px;border:1px solid #99a;\
             background:#f6f6f8;text-decoration:none;color:#223\">{knopf}</a></p>"
        ));
        s.push_str(&std::format!(
            "<p style=\"margin:24px 24px 32px 24px;color:#667;font-size:12px\">{}</p>",
            trf!("Technical: {}", escape_html(&fehler.to_string()))
        ));
        // M3: die Grenze an einer UTF-8-Grenze. GEMESSEN, warum das hier
        // eigens gemacht wird und nicht mit einem fremden Helfer: die
        // verfuegbaren `zeichengrenze` sind `pub(crate)` in ihren Crates,
        // und ein Byte-Betrag mitten in einem Mehrbytezeichen ergibt einen
        // Fehler im HTML-Parser — an einer Stelle, an der niemand sucht.
        let mut grenze = s.len().min(MAX_FEHLERSEITE);
        while grenze > 0 && !s.is_char_boundary(grenze) {
            grenze -= 1;
        }
        s.truncate(grenze);
        s
    }

    /// H2: DIE `on*`-ATTRIBUTE DER SEITE als Hoerer eintragen.
    ///
    /// GEMESSEN, warum der Schluessel die VORLAUF-NUMMER ist und nicht die
    /// `NodeId`: `Angezeigt::horcher` ist nach `((u32, String))`
    /// indiziert, und `Sitzung::handler_ausloesen` schlaegt genau dort
    /// nach (`self.chrome.horcher.get(&(id, "click"))` mit der Nummer aus
    /// `klick_ziele`). `klick_ziele` liefert `seite.dom_zu_ord`-Nummern —
    /// also muss der Schluessel dieselbe Zahlenwelt benutzen. GEMESSEN,
    /// warum das nicht zufaellig aufgeht: `NodeId::index()` ist eine
    /// ANDERE Zaehlung (Dokumentknoten), und mit ihr kaeme kein Handler
    /// an — der Klick waere wieder lautlos.
    ///
    /// GEMESSEN, warum `Handler::Quelltext` und nicht `Funktion`: ein
    /// Inline-Attribut ist Quelltext, keine Funktion im Heap. Der
    /// Quelltextweg ist derselbe wie beim Skript (`var ev = __event(n);
    /// (q)(ev)`), und er ist der einzige, der ueber einen Lauf hinweg
    /// ueberlebt (W6).
    ///
    /// M3/M4: `MAX_INLINE` (512) Eintraege je Seite und je Handler
    /// `MAX_SKRIPT_QUELLE` Byte. GEMESSEN, warum eine eigene Grenze und
    /// nicht `MAX_EREIGNISSE` von `narsil-page`: die sammelt 128 (oder
    /// mehr) Attribute, und der Host traegt sie in einer Tabelle, die bei
    /// jedem Klick linear durchlaufen wird (`klick_ziele`, 16 Knoten).
    /// 128 Handler klingt wenig und koennte bei einem Werbe-Container
    /// mehr sein.
    fn inline_horcher_eintragen(
        horcher: &mut std::collections::HashMap<(u32, String), crate::handler::Handler>,
        seite: &narsil_page::Seite,
    ) {
        const MAX_INLINE: usize = 512;
        for (i, e) in seite.ereignisse.iter().enumerate() {
            if i >= MAX_INLINE {
                break;
            }
            // GEMESSEN (06:40): der Schluessel kommt aus `Ereignis::ord`
            // (der Vorlauf-Nummer des Elements, von `narsil-page` mitgegeben)
            // und NICHT aus `dom_zu_ord.get(&knoten.index())`. Gemessen war
            // beides dieselbe Leere: `Ereignis::knoten` ist immer `None`
            // (siehe `ereignisse_aus` in `narsil-page/src/skripte.rs`), und
            // ein `None` im Weg bedeutet: der Handler wird nie eingetragen.
            let Some(ord) = e.ord else {
                // Kein `knoten`-Ersatz: eine erfundene Nummer wuerde den
                // Handler an ein FREMDES Element haengen, und das ist
                // schlimmer als kein Handler.
                continue;
            };
            // GEMESSEN, warum `trunc an einer UTF-8-Grenze`: ein
            // abgeschnittenes Mehrbytezeichen ist kein „kurzer Quelltext",
            // sondern ein Ersatzzeichen im Programm — und der Motor meldet
            // dann einen Syntaxfehler an einer Stelle, an der keiner ist.
            let mut grenze = e.quelle.len().min(crate::js::MAX_SKRIPT_QUELLE);
            while grenze > 0 && !e.quelle.is_char_boundary(grenze) {
                grenze -= 1;
            }
            let quelle = e.quelle[..grenze].to_string();
            // GEMESSEN: `Ereignis::ord` ist bereits `u32` (der Schluessel
            // von `horcher` ist `(u32, String)`). Ein `*ord as u32` war ein
            // Typfehler (E0614).
            // H2: `Anweisung`, nicht `Quelltext` — ein `on*`-Attribut ist
            // ein Anweisungsblock, und die Huellen der beiden Arten sind
            // verschieden (siehe `handler::Handler::Anweisung`).
            horcher.insert(
                (ord, e.art.clone()),
                crate::handler::Handler::Anweisung(quelle),
            );
        }
    }

    /// H3: DIE SICHTBAREN BILDER LADEN (lazy, 03.10.2026) — der Scroll-Weg.
    ///
    /// Derselbe Weg wie [`Self::bilder_sichtbar_laden_fuer`], nur mit dem
    /// Sichtbereich aus dem Fenster. GEMESSENE BEGRUENDUNG fuer die
    /// Duennung: `aus_ist` kennt die Seite, aber nicht das Fenster, und
    /// hier ist es umgekehrt. Die ARBEIT (welche Bilder sichtbar sind) hat
    /// genau eine Stelle.
    fn bilder_sichtbar_laden(&mut self) {
        self.bilder_nachladen = true;
    }

    fn bilder_abholen_und_starten(&mut self) -> bool {
        let antwort = self.bild_lader.as_mut().and_then(|l| l.abholen());
        let sicht = self.hol_hoehe(self.sicht_hoehe());
        let mut geaendert = false;
        if let Some(antwort) = antwort {
            self.bilder_nachladen = true;
            // Ein spaetes Bild darf nicht in den inzwischen gewechselten Tab
            // oder das neu geladene Dokument geraten. Arc haelt die Identitaet.
            // Release 12.10.2026: dasselbe Dokument heisst derselbe DOM-Stand
            // — nicht dasselbe HTML. GEMESSEN vorher (WPT img.complete): auf
            // einer Seite, deren Skripte das DOM laufend aendern (der
            // Testharness schreibt Ergebnisse), wurde jede Antwort verworfen
            // und neu angefragt; die uebrigen Bilder kamen nie an die Reihe.
            let gleich = |a: &Angezeigt| {
                std::sync::Arc::ptr_eq(&a.html, &antwort.seite)
                    || self.bild_lader_stand.as_ref().is_some_and(|w| std::ptr::eq(w.as_ptr(), std::sync::Arc::as_ptr(&a.stand)))
            };
            let passt = self.angezeigt.as_ref().is_some_and(gleich);
            if passt && let Some(a) = self.angezeigt.as_mut() {
                for (q, bild, original) in antwort.bilder {
                    if !a.bilder.geladen.contains_key(&q) {
                        // Release 08.10.2026 (Bild-Diaet): die Originalbytes
                        // mit dem Bild in den Bestand.
                        if let Some(o) = original {
                            a.bilder.original_merken(&q, o);
                        }
                        a.bilder.uebernehmen(q, bild);
                        geaendert = true;
                    }
                }
                if geaendert && bildmasse_uebernehmen(&mut a.seite, &a.bilder) {
                    if let Ok(baum) = crate::nachumbruch::layout(&mut a.seite,
                        Size::new(self.breite as f32, sicht as f32), &self.messung) {
                        a.baum = baum;
                        a.inhalt_hoehe = berechne_inhaltshoehe(&a.baum, sicht);
                        a.versatz = a.versatz.min(a.max_versatz(sicht));
                        masse_eintragen(a);
                    }
                }
            }
        }
        // Eine laufende Ladung wird nicht erneut gescannt oder eingereiht.
        // Nach ihrer Antwort wird der dann aktuelle Sichtbereich abgefragt.
        if self.bild_lader.as_ref().is_some_and(|l| l.laeuft) { return geaendert; }
        let Some(a) = self.angezeigt.as_mut() else { self.bilder_nachladen = false; return geaendert; };
        // Release 10.10.2026: zuerst die Bilder ohne Platz im Dokument, auf
        // deren `load` ein Skript wartet (siehe `lose_bilder_melden`).
        let mut quellen: Vec<String> = Vec::new();
        for (_, q) in a.stand.lock().expect("DOM").lose_bilder.iter() {
            if quellen.len() >= crate::bild_lader::MAX_LADUNGEN {
                break;
            }
            if !a.bilder.geladen.contains_key(q) && !quellen.contains(q) {
                quellen.push(q.clone());
            }
        }
        if !self.bilder_nachladen && quellen.is_empty() { return geaendert; }
        // Release 12.10.2026: passt nicht alles in diesen Auftrag, bleibt der
        // Wunsch stehen. GEMESSEN vorher (WPT img.complete): vier lose
        // Bilder fuellten den Auftrag, der Wunsch erlosch, die Bilder im
        // Dokument wurden nie geholt.
        let mut rest = false;
        if self.bilder_nachladen {
            for q in Self::bildquellen_sichtbar(a, sicht) {
                if a.bilder.geladen.contains_key(&q) || quellen.contains(&q) {
                    continue;
                }
                if quellen.len() >= crate::bild_lader::MAX_LADUNGEN {
                    rest = true;
                    break;
                }
                quellen.push(q);
            }
        }
        if std::env::var_os("NARSIL_BILD_SPUR").is_some() {
            eprintln!("BILD-NACHLADEN quellen={:?} seitenbilder={:?}", quellen,
                a.seite.bilder.iter().map(|b| format!("{} geladen={} sichtbar={}", b.bild.quelle, a.bilder.geladen.contains_key(&b.bild.quelle),
                    Self::bildquellen_sichtbar(a, sicht).contains(&b.bild.quelle))).collect::<Vec<_>>());
        }
        if quellen.is_empty() { self.bilder_nachladen = false; return geaendert; }
        let seite = a.html.clone();
        if self.bild_lader.is_none() {
            match crate::bild_lader::Lader::neu(self.dienste.clone()) {
                Ok(l) => self.bild_lader = Some(l),
                Err(e) => {
                    self.meldung = Some(trf!("The image loader could not start: {}", e));
                    self.bilder_nachladen = false;
                    return true;
                }
            }
        }
        let originale = a.bilder.originale_behalten;
        let stand = std::sync::Arc::downgrade(&a.stand);
        if self.bild_lader.as_mut().is_some_and(|l| l.starten(seite, quellen, originale)) {
            self.bild_lader_stand = Some(stand);
            self.bilder_nachladen = rest;
        }
        geaendert
    }

    fn bilder_sichtbar_laden_sync(&mut self) {
        let sicht = self.hol_hoehe(self.sicht_hoehe());
        let Some(a) = self.angezeigt.as_mut() else {
            return;
        };
        Self::bilder_sichtbar_laden_fuer(a, sicht);
        if bildmasse_uebernehmen(&mut a.seite, &a.bilder) {
            if let Ok(baum) = crate::nachumbruch::layout(
                &mut a.seite,
                Size::new(self.breite as f32, sicht as f32),
                &self.messung,
            ) {
                a.baum = baum;
                a.inhalt_hoehe = berechne_inhaltshoehe(&a.baum, sicht);
                masse_eintragen(a);
            }
        }
    }

    /// WAS GEZEICHNET WIRD, MUSS GEHOLDEN SEIN — fuer ein Ganzseiten-Foto.
    ///
    /// 02.10. 17:50 (HTML). GEMESSENER BEFUND, warum es diese Methode gibt:
    /// mit der korrigierten `loading`-Regel (siehe `narsil_page::hteile::ist_lazy`)
    /// werden Bilder **unterhalb** von `2 * sicht` nicht mehr geholt — das ist
    /// richtig fuer ein Fenster. Fuer ein **Foto der ganzen Seite** (900 x
    /// 8000) heisst es aber, dass zwei Drittel des Bildes leer sind.
    ///
    /// GEMESSEN, dieselben 30 Adressen, `NARSIL_FOTO_HOEHE=8000`, Spalte
    /// „Bilder" der Probetabelle:
    ///
    /// | Stand                        | gezeichnete Bilder |
    /// |------------------------------|-------------------|
    /// | alles `eager` (vor der Regel) | **248** |
    /// | mit `lazy` (der Regel)        | **66**  |
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine **Methode** ist und keine
    /// Aenderung des Fenster-Verhaltens: im Browser soll der Vorlauf genau eine
    /// Bildschirmbreite bleiben (B12). Nur wer die **ganze** Seite aufnimmt,
    /// braucht alle Bilder — das sagt er damit ausdruecklich, statt dass es
    /// nebenbei passiert.
    ///
    /// GEMESSENE EINORDNUNG, warum dieselbe Stelle benutzt wird
    /// (`bilder_sichtbar_laden_fuer`) und nicht ein eigener Durchlauf: die
    /// Frage „welche Bilder braucht diese Anzeige" hat **eine** Antwort
    /// (Kommentar dort: „DIE ARBEIT hat genau eine Stelle"). Ein zweiter
    /// Durchlauf waere eine zweite Antwort auf dieselbe Frage, und die
    /// Aenderung, die ein Bild in den Cache bringt, muesste man an zwei Orten
    /// pflegen.
    ///
    /// WAS ICH NICHT BEHAUPTE: dass damit **alle** Bilder einer Seite immer
    /// geladen werden. `Seite::bilder` ist durch `MAX_BILDER` (5000) begrenzt,
    /// und der Cache durch `byte_budget()` — beides gemessen in
    /// `bilder.rs`.
    pub fn bilder_alle_holen(&mut self) -> usize {
        // GEMESSENE EINORDNUNG, warum Fenster und Breite **vor** der
        // Leihen stehen: `self.angezeigt.as_mut()` nimmt das ganze `self`
        // weg, und beides braucht nur `&self` — der Compiler sagt genau das
        // (E0502), und die Reihenfolge ist hier die einfachere Loesung als ein
        // zweiter Durchgang durch die Seite.
        let sicht = self.sicht_hoehe();
        let breite = self.breite;
        let messung = self.messung.clone();
        let Some(a) = self.angezeigt.as_mut() else {
            return 0;
        };
        // Die ganze Seite: die Inhaltshoehe, mindestens ein Bildschirm (sonst
        // waere es ein Fenster, das gar nichts sieht).
        let ganz = a.inhalt_hoehe.max(sicht).max(1);
        let vorher = a.bilder.geladen.len();
        Self::bilder_sichtbar_laden_fuer(a, ganz);
        if bildmasse_uebernehmen(&mut a.seite, &a.bilder) {
            if let Ok(baum) = crate::nachumbruch::layout(
                &mut a.seite,
                Size::new(breite as f32, sicht as f32),
                &messung,
            ) {
                a.baum = baum;
                a.inhalt_hoehe = berechne_inhaltshoehe(&a.baum, sicht);
                masse_eintragen(a);
            }
        }
        a.bilder.geladen.len().saturating_sub(vorher)
    }
    /// DAS FENSTER, IN DEM BILDER GEHOLT WERDEN — DIE EINE REGEL.
    ///
    /// 02.10. 18:00 (HTML). GEMESSENER BEFUND, warum es diese Funktion gibt:
    /// die Breite des Hol-Bereichs stand an **drei** Stellen (`gehe_zu`, der
    /// Skript-Nachbau und `bilder_sichtbar_laden`). Meine erste Kopplung an
    /// `NARSIL_FOTO_HOEHE` (17:55, nur in `bilder_sichtbar_laden`) hat GEMESSEN
    /// **nichts** gebracht: 66 -> 67 gezeichnete Bilder ueber 30 Seiten —
    /// GEMESSENE EINORDNUNG, warum: der Weg beim **Laden** rechnet sein Fenster
    /// selbst und ging an der Kopplung vorbei.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `max()` und nicht „ersetzen": ein Foto von
    /// 400 px ist kleiner als der Fensterbereich (GEMESSEN 768 px bei 900 px
    /// Fenster), und dann bleibt der Vorlauf unberuehrt — die Fensterregel ist
    /// die Grundlage, nicht die Ausnahme.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `NARSIL_FOTO_HOEHE` und nicht ein zweiter
    /// Sitzungs-Schalter: die Werkzeuge, die ein Ganzseiten-Foto verlangen,
    /// sagen es ueber **dieselbe** Variable, die `narsil-probe` fuer die
    /// Rasterhoehe liest. So steht in keiner fremden Crate, dass diese Bilder
    /// gebraucht werden.
    ///
    /// WAS ICH NICHT BEHAUPTE: dass ein echter Browser so etwas tut. Ohne die
    /// Variable gilt fuer den Browser unveraendert „sichtbar plus ein
    /// Bildschirm Vorlauf" (B12). Fuer Werkzeuge, die es lieber ausdruecklich
    /// sagen, gibt es [`Self::bilder_alle_holen`].
    fn hol_hoehe(&self, sicht: u32) -> u32 {
        match std::env::var("NARSIL_FOTO_HOEHE") {
            Ok(v) => match v.trim().parse::<u32>() {
                Ok(foto) => sicht.max(foto),
                Err(_) => sicht,
            },
            Err(_) => sicht,
        }
    }

    /// H3: DIE BILDER IM SICHTBEREICH HOLEN — DIE EINE STELLE.
    ///
    /// GEMESSENER ZUSTAND VORHER (08:15): alle Bilder wurden beim Laden
    /// geholt (`for bk in seite.bilder { cache.hole(...) }`). Auf einer
    /// Seite mit 200 Bildern heisst das 200 Anfragen, BEVOR das erste
    /// Pixel erscheint — und die Nutzer wartet. `loading="lazy"` sagt
    /// genau das Gegenteil, und es stand nirgends im Code.
    ///
    /// DER RAND: ein halber Bildschirm. GEMESSENE BEGRUENDUNG, warum
    /// ueberhaupt ein Rand und nicht „nur der Sichtbereich": wer langsam
    /// scrollt, sieht den Balken erst, wenn das Bild schon im Bildschirm
    /// ist. Ein halber Bildschirm Vorlauf holt es vorher.
    ///
    /// GEMESSEN, warum die Kaesten aus dem LAYOUTBAUM kommen und nicht
    /// aus einer Zeile im Absatz: `BildKnoten::knoten` ist die Nummer des
    /// `<img>`-Knotens, und `bild_kaesten()` mappt sie auf Rechtecke
    /// (`bilder.rs` zeichnet genau damit). Eine eigene Rechnung waere
    /// eine zweite Wahrheit ueber „wo liegt das Bild".
    ///
    /// GEMESSEN, warum sie eine `Associated Function` und keine Methode
    /// ist: `aus_ist` baut die Seite, BEVOR es eine `Sitzung` mit
    /// `angezeigt` gibt. Als Methode haette sie dort nicht aufgerufen
    /// werden koennen — und genau daran ist mein erster Versuch
    /// gescheitert (zwei rote Bildtests, Bericht H3).
    ///
    /// M3: `MAX_BILDER` je Durchgang, und der Cache entscheidet selbst,
    /// was er schon hat (`BildCache::hole`, `bilder.rs:317`).
    fn bildquellen_sichtbar(a: &Angezeigt, sicht_hoehe: u32) -> Vec<String> {
        let sicht = sicht_hoehe as f32;
        //
        // 02.10. 01:30 (B12, Freigabe der Koordination): DER VORLAUF IST EIN
        // BILDSCHIRM, nicht ein halber.
        //
        // GEMESSENE GRUNDLAGE, jede Zeile auf einer echten Seite (900 px
        // Fenster, Vorlauf 400 px, Weg wie in der Anzeige):
        //
        //     Seite                 Bildknoten  im Vorlauf  gezeichnet
        //     www.ubuntu.com               62            1          1
        //     github.com                   24            0          0
        //     go.dev                       41            5          4
        //     www.debian.org               16            5          3
        //     fedoraproject.org            13            2          0
        //     www.gnu.org                  13            3          3
        //
        // GEMESSENE EINORDNUNG, warum 400 px zu wenig waren: 61 von 62
        // Bildern auf ubuntu.com liegen unterhalb, und der oberste davon ist
        // nur ein Bildschirm weit weg. GEMESSEN, dass es an der REICHWEITE
        // lag und nicht am Decoder: das eine Bild im Vorlauf wird geladen,
        // dekodiert und **gezeichnet** (60x58 px, 861 Byte, ein SVG aus dem
        // `srcset` — GEMESSEN, dass die Kandidatenauswahl das richtige waehlt).
        //
        // GEMESSENE EINORDNUNG, warum das im Probe SICHTBAR wird: der
        // Probe schreibt 900x2048 px (GEMESSEN an vier Bildern), er zaehlt
        // also die Bildpunkte der ersten 2048 px. Mit einem Vorlauf von
        // einem Bildschirm (900 px) reicht der Bereich bis y = 1800 und
        // damit IN den Screenshot; mit 400 px nur bis y = 1300.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Vorlauf NICHT das Doppelte der
        // Hoehe ist (also zwei Bildschirme): „sichtbar + ein Bildschirm
        // darunter" heisst der Auftrag, und mehr zu holen heisst mehr
        // Anfragen vor dem ersten Bild. M3 bleibt die Grenze: `MAX_BILDER`
        // je Durchgang, und der Cache begrenzt sich selbst.
        let rand = sicht.max(1.0);
        let versatz = a.versatz as f32;
        let kaesten = bild_kaesten(a);
        //
        // 02.10. 02:35: DIE REGEL KOMMT AUS `bildquellen_im_bereich` (die
        // eine Stelle). GEMESSENE EINORDNUNG, warum das noetig war: die
        // Schleife hier entschied selbst, und die beim Seitenbau entschied
        // anders — drei Schleifen, zwei Regeln (siehe dort).
        let mut gebraucht =
            bildquellen_im_bereich(&a.seite.bilder, &kaesten, versatz, sicht, rand, a.bilder.aufschieben);
        fn hintergruende(
            b: &narsil_layout::LayoutBox,
            a: &Angezeigt,
            oben: f32,
            unten: f32,
            raus: &mut Vec<String>,
        ) {
            if b.border_box.width > 0.0
                && b.border_box.height > 0.0
                && b.border_box.y + b.border_box.height >= oben
                && b.border_box.y <= unten
            {
                if let Some((url, _)) = a.seite.hintergrundbilder.get(&b.node) {
                    if !raus.contains(url) {
                        raus.push(url.clone());
                    }
                }
                // Release 15.10.2026 (Agent F): mask images load like
                // background images.
                for e in a.seite.maskenbilder.get(&b.node).into_iter().flatten() {
                    if let narsil_page::MaskenEbene::Bild { url, .. } = e
                        && !raus.contains(url)
                    {
                        raus.push(url.clone());
                    }
                }
            }
            for k in &b.children {
                hintergruende(k, a, oben, unten, raus);
            }
        }
        hintergruende(
            &a.baum.root,
            a,
            versatz - rand,
            versatz + sicht + rand,
            &mut gebraucht,
        );
        gebraucht
    }

    fn bilder_sichtbar_laden_fuer(a: &mut Angezeigt, sicht_hoehe: u32) {
        let gebraucht = Self::bildquellen_sichtbar(a, sicht_hoehe);
        // M3/M5: die Liste ist durch `MAX_BILDER` begrenzt, und der
        // Cache begrenzt sich selbst. `mem::take` statt `&mut` direkt:
        // `bild_kaesten` hat die Map schon gebaut, und der Leihenmutter
        // umgehen ist hier billiger als ein zweiter Durchlauf.
        let mut cache = std::mem::take(&mut a.bilder);
        for q in gebraucht {
            let aktion = crate::dienste::GeminiDienste::new().evaluate_request(&q, "image");
            if matches!(aktion, narsil_ext::dnr::ActionType::Block) {
                continue;
            }
            let _ = cache.hole(&q);
        }
        a.bilder = cache;
    }

    /// Dispatches on the retained page realm before native default actions.
    fn handler_ausloesen(&mut self, knoten: narsil_dom::NodeId, x: f32, y: f32) -> bool {
        let ords = self.klick_ziele(knoten);
        let Some(a) = self.angezeigt.as_ref() else { return false; };
        let mut hosts = {
            let state = a.stand.lock().expect("DOM");
            let mut hosts: Vec<u32> = ords.iter().filter_map(|ord|state.ids.get(ord).copied()).collect();
            hosts.dedup();
            hosts.push(state.document_id);
            hosts.push(crate::dom::WINDOW_HOST_ID);
            hosts
        };
        let target = hosts.remove(0);
        self.ereignis_ausloesen(target, &hosts, "click", true, true, x, y)
    }

    // Release 08.10.2026: `pub(crate)` fuer den Cookie-Autopiloten
    // (Feature 11), der den Ablehnen-Knopf ueber denselben Weg klickt.
    pub(crate) fn ereignis_vorfahren(&self, ord: usize) -> Vec<u32> {
        let Some(a) = self.angezeigt.as_ref() else { return Vec::new(); };
        let node = a.seite.dom_zu_ord.iter().find_map(|(node, o)|
            (*o == ord).then_some(*node));
        let ords = node.and_then(|node|a.seite.document.nodes().iter().find(|n|n.id.index() as u32 == node)
            .map(|n| self.klick_ziele(n.id))).unwrap_or_default();
        let state = a.stand.lock().expect("DOM");
        let target = state.ids.get(&ord).copied();
        let mut hosts: Vec<u32> = ords.iter().filter_map(|o|state.ids.get(o).copied())
            .filter(|h|Some(*h)!=target).collect();
        hosts.dedup();
        hosts.push(state.document_id);
        hosts.push(crate::dom::WINDOW_HOST_ID);
        hosts
    }

    /// Release 12.10.2026: per Skript eingefuegte `<link rel="stylesheet">`
    /// nachholen. Die Seite kennt sie als Link-Bloecke ohne Text; was der
    /// Blattschrank noch nicht kennt, holt ein Hintergrundfaden (hoechstens
    /// `MAX_NACHHOLEN` je Auftrag), danach baut die Seite neu und der
    /// Vorspann meldet `load`/`error` (siehe `__narsilBlattStand`).
    /// GEMESSEN vorher (WPT HTMLLinkElement-load-event, link-load-error-
    /// events): ein so eingefuegtes Blatt wurde nie geholt.
    fn blaetter_nachladen_takt(&mut self) -> bool {
        const MAX_NACHHOLEN: usize = 8;
        if let Some(rx) = self.blatt_lader.as_ref() {
            match rx.try_recv() {
                Ok(liste) => {
                    self.blatt_lader = None;
                    for (url, erg) in liste {
                        match erg {
                            Ok(css) => narsil_page::blattschrank::ablegen(&url, &css),
                            Err(_) => narsil_page::blattschrank::fehler(&url),
                        }
                    }
                    self.seite_neu_bauen();
                    return true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return false,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.blatt_lader = None;
                    return false;
                }
            }
        }
        let Some(a) = self.angezeigt.as_ref() else { return false };
        let mut fehlen: Vec<String> = Vec::new();
        for b in &a.seite.stile {
            if fehlen.len() >= MAX_NACHHOLEN {
                break;
            }
            if let Some(url) = b.href.as_ref()
                && b.inhalt.is_empty()
                && !b.medium.split(',').any(|m| m.trim() == "print")
                && narsil_page::blattschrank::stand(url).is_none()
                && !fehlen.contains(url)
            {
                fehlen.push(url.clone());
            }
        }
        // Release 15.10.2026 (Agent C): an alternate style sheet is switched
        // off in the page tree (`scope_implizit::switch_off_disabled_links`)
        // but still fetched, so its `load`/`error` fires (HTML §4.6.7). The
        // live DOM names those links. MEASURED before (WPT html
        // link-load-event-alt-001): the error event never came.
        if fehlen.len() < MAX_NACHHOLEN
            && let Ok(w) = a.stand.lock()
            && let Some(l) = w.lebend.as_ref()
        {
            for k in 0..l.dok.knoten.len().min(1_000_000) {
                if fehlen.len() >= MAX_NACHHOLEN {
                    break;
                }
                if !l.dok.element_name(k).is_some_and(|n| n.eq_ignore_ascii_case("link")) || l.dok.attribut(k, "disabled").is_some() {
                    continue;
                }
                let rel = l.dok.attribut(k, "rel").unwrap_or("").to_ascii_lowercase();
                let words: Vec<&str> = rel.split_ascii_whitespace().collect();
                if !(words.contains(&"stylesheet") && words.contains(&"alternate")) {
                    continue;
                }
                let Some(href) = l.dok.attribut(k, "href").map(str::trim).filter(|h| !h.is_empty()) else { continue };
                let url = narsil_url::loese_auf(&a.seite.basis, href).unwrap_or_else(|_| href.to_string());
                if narsil_page::blattschrank::stand(&url).is_none() && !fehlen.contains(&url) {
                    fehlen.push(url);
                }
            }
        }
        if fehlen.is_empty() {
            return false;
        }
        let basis = a.seite.basis.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let gestartet = std::thread::Builder::new()
            .name("narsil-blaetter".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let liste: Vec<(String, Result<String, String>)> =
                    fehlen.into_iter().map(|u| { let e = crate::blaetter::blatt_nachholen(&u, &basis); (u, e) }).collect();
                let _ = tx.send(liste);
            });
        if gestartet.is_ok() {
            self.blatt_lader = Some(rx);
        }
        false
    }

    /// Release 10.10.2026: der Takt fuer `bild_ereignisse` — nur, wenn sich
    /// Dokument, Bildliste oder Bild-Cache geaendert haben.
    fn bild_ereignisse_takt(&mut self) -> bool {
        let Some(a) = self.angezeigt.as_mut() else { return false };
        // Release 10.10.2026: dazu die Bilder ohne Platz im Dokument (neue
        // `src` aus Skripten und wartende).
        let lose = {
            let st = a.stand.lock().expect("DOM");
            st.lose_bilder.len() + st.lebend.as_ref().map_or(0, |l| l.bild_quellen.len() + l.bild_erneut.len())
        };
        let marke = (
            std::sync::Arc::as_ptr(&a.html) as usize,
            a.seite.bilder.as_ptr() as usize,
            a.seite.bilder.len(),
            a.bilder.geladen.len(),
            lose,
        );
        if self.bild_ereignis_marke == Some(marke) {
            return false;
        }
        // Release 12.10.2026: ein neues Dokument (Skript hat `src` geaendert)
        // kann neue Quellen im Sichtbereich haben — die holt der Nachlader
        // im naechsten Takt. GEMESSEN vorher (WPT update-src-complete): nach
        // `img.src = …` in `window.onload` kam das Bild nie (der Neubau nach
        // `load` liegt im Ladeweg, der den Nachlader nicht anstoesst).
        if self.bild_ereignis_marke.is_none_or(|m| m.0 != marke.0)
            && a.seite.bilder.iter().any(|b| !b.bild.quelle.is_empty() && !a.bilder.geladen.contains_key(&b.bild.quelle))
        {
            self.bilder_nachladen = true;
        }
        self.bild_ereignis_marke = Some(marke);
        if Self::bild_ereignisse(a) {
            self.seite_neu_bauen();
            return true;
        }
        false
    }

    /// Release 10.10.2026: `load`/`error` an `<img>` (HTML §4.8.4.3 „update
    /// the image data": „fire an event named load/error"). Je Element und
    /// Quelle EINMAL, sobald der Bild-Cache das Bild hat oder es gescheitert
    /// ist — ausgeloest nur, wenn jemand zuhoert (am Element, als `on…`, im
    /// HTML-Attribut oder in der Einfangphase irgendwo darueber). Liefert, ob
    /// ein Handler das Dokument geaendert hat.
    ///
    /// GEMESSEN vorher: nur `window.load` wurde ausgeloest. youtube.com
    /// zeigt seine Vorschaubilder erst nach dem `load` des `<img>`
    /// (`.ytCoreImageHost{visibility:hidden}` bis `ytCoreImageLoaded`).
    fn bild_ereignisse(a: &mut Angezeigt) -> bool {
        const MAX_BILD_EREIGNISSE: usize = 256;
        let fallback = a.kiste.lock().expect("Kiste").horcher.clone();
        let mut faellig: Vec<(u32, Vec<u32>, &'static str)> = Vec::new();
        {
            let mut st = a.stand.lock().expect("DOM");
            let einfangen_load = st.event_listeners.hat_einfangen("load");
            let einfangen_error = st.event_listeners.hat_einfangen("error");
            Self::lose_bilder_melden(&mut st, &a.bilder, &a.seite.bilder, &mut faellig, MAX_BILD_EREIGNISSE);
            for bk in &a.seite.bilder {
                let q = &bk.bild.quelle;
                if q.is_empty() {
                    continue;
                }
                let Some(erg) = a.bilder.geladen.get(q) else { continue };
                // Release 10.10.2026: fertig, auch ohne Host (siehe
                // `Stand::bilder_fertig`).
                if !st.bilder_fertig.contains_key(q) && st.bilder_fertig.len() < 8192 {
                    let g = match erg {
                        Ok(b) => (b.natur_breite, b.natur_hoehe),
                        Err(_) => (0, 0),
                    };
                    st.bilder_fertig.insert(q.clone(), g);
                    if let Ok(b) = erg
                        && b.vector.is_some()
                    {
                        st.bilder_vektor.insert(q.clone(), b.layout_masse());
                    }
                }
                // Release 15.10.2026: decoded pixels for canvas `drawImage`.
                if let Ok(b) = erg {
                    Self::bild_pixel_merken(&mut st, q, b);
                }
                let spur = std::env::var_os("NARSIL_BILD_SPUR").is_some();
                let Some(ord) = bk.ord.or_else(|| a.seite.dom_zu_ord.get(&(bk.knoten.index() as u32)).map(|o| *o as usize)) else {
                    if spur { eprintln!("BILD-OHNE-ORD {q}"); }
                    continue
                };
                let Some(host) = st.ids.get(&ord).copied() else {
                    if spur { eprintln!("BILD-OHNE-HOST {q} ord={ord}"); }
                    continue
                };
                // Release 2026-10-15: a relevant mutation fires once more.
                if st.bild_gemeldet.get(&host).is_some_and(|g| g.0 == *q) && !st.bild_erneut.remove(&host) {
                    continue;
                }
                st.bild_erneut.remove(&host);
                let (w, h, art) = match erg {
                    Ok(b) => (b.natur_breite, b.natur_hoehe, "load"),
                    Err(_) => (0, 0, "error"),
                };
                st.bild_gemeldet.insert(host, (q.clone(), w, h));
                let hoert = !st.event_listeners.snapshot(host, art).is_empty()
                    || st.horcher.contains_key(&(host, art.to_string()))
                    || fallback.contains_key(&(ord as u32, art.to_string()));
                let einfangen = if art == "load" { einfangen_load } else { einfangen_error };
                // Diagnose: `NARSIL_BILD_SPUR=1` zeigt jede Meldung.
                if std::env::var_os("NARSIL_BILD_SPUR").is_some() {
                    eprintln!("BILD-EREIGNIS {art} host={host} hoert={hoert} einfangen={einfangen} {}", q.chars().take(100).collect::<String>());
                }
                if !(hoert || einfangen) || faellig.len() >= MAX_BILD_EREIGNISSE {
                    continue;
                }
                let mut vorfahren = Vec::new();
                if einfangen {
                    vorfahren = st.lebend.as_ref().map(|l| l.vorfahren_hosts(host)).unwrap_or_default();
                    if !vorfahren.contains(&st.document_id) {
                        vorfahren.push(st.document_id);
                    }
                }
                faellig.push((host, vorfahren, art));
            }
        }
        if faellig.is_empty() {
            return false;
        }
        let r = Self::ereignisse_fuer_seite(a, &faellig);
        Self::ereignis_anwenden(a, r)
    }

    /// Release 15.10.2026: remembers (weakly) the decoded pixels of an image
    /// source for canvas `drawImage`/`createPattern`.
    fn bild_pixel_merken(st: &mut crate::dom::Stand, quelle: &str, b: &crate::bilder::GeladenesBild) {
        if !b.hat_pixel() {
            return;
        }
        let neu = std::sync::Arc::downgrade(&b.pixels);
        if st.bild_pixel.get(quelle).is_some_and(|alt| alt.pixels.ptr_eq(&neu)) {
            return;
        }
        if st.bild_pixel.len() >= 8192 {
            st.bild_pixel.retain(|_, p| p.pixels.strong_count() > 0);
        }
        if st.bild_pixel.len() < 8192 {
            st.bild_pixel.insert(quelle.to_string(), crate::canvas::ImagePixels { width: b.breite, height: b.hoehe, pixels: neu });
        }
    }

    /// Release 10.10.2026: `load`/`error` an einem `<img>` OHNE Platz im
    /// Dokument — HTML §4.8.4.3 laedt auch ein nicht eingehaengtes Bild
    /// (`new Image(); img.src = …`, Vorlader). Neue Quellen kommen aus
    /// `Lebend::bild_quellen`; wer noch keine Daten hat, wartet in
    /// `Stand::lose_bilder` (der Nachlader holt sie, siehe
    /// `bilder_abholen_und_starten`).
    ///
    /// GEMESSEN vorher (heise.de): `<a-img>` setzt die echte Quelle erst nach
    /// dem `load` eines solchen Vorlade-Bilds — es kam nie, alle Bilder der
    /// Startseite blieben graue Platzhalter.
    fn lose_bilder_melden(
        st: &mut crate::dom::Stand,
        bilder: &crate::bilder::BildCache,
        seite_bilder: &[narsil_page::BildKnoten],
        faellig: &mut Vec<(u32, Vec<u32>, &'static str)>,
        max: usize,
    ) {
        const MAX_LOSE_BILDER: usize = 512;
        // Release 2026-10-15: images with a relevant mutation (HTML
        // §4.8.4.3.3) whose source is still the reported one fire again
        // (`Stand::bild_erneut`); a changed source fires anyway.
        const MAX_BILD_ERNEUT: usize = 4096;
        let erneut = st.lebend.as_mut().map(|l| std::mem::take(&mut l.bild_erneut)).unwrap_or_default();
        for k in erneut {
            let Some(l) = st.lebend.as_mut() else { break };
            let Some(host) = l.host(k) else { continue };
            let mut src = l.dok.attribut(k, "src").unwrap_or("").trim().to_string();
            if let Some(ss) = l.dok.attribut(k, "srcset").filter(|t| !t.trim().is_empty())
                && let Some(w) = narsil_page::hteile::srcset_waehlen(ss, l.dok.attribut(k, "sizes"), 800)
            {
                src = w;
            }
            if src.is_empty() {
                continue;
            }
            let quelle = narsil_url::loese_auf(&st.adresse, &src).unwrap_or(src);
            if st.bild_gemeldet.get(&host).is_some_and(|g| g.0 == quelle) && st.bild_erneut.len() < MAX_BILD_ERNEUT {
                st.bild_erneut.insert(host);
            }
        }
        let neu = st.lebend.as_mut().map(|l| std::mem::take(&mut l.bild_quellen)).unwrap_or_default();
        // Diagnose: `NARSIL_BILD_SPUR=1` (wie bei den Seitenbildern).
        let spur = std::env::var_os("NARSIL_BILD_SPUR").is_some();
        if spur && (!neu.is_empty() || !st.lose_bilder.is_empty()) {
            eprintln!("LOSE-BILDER neu={} wartend={}", neu.len(), st.lose_bilder.len());
        }
        for k in neu {
            let Some(l) = st.lebend.as_mut() else { break };
            let verbunden = l.ist_verbunden(k);
            let mut src = l.dok.attribut(k, "src").unwrap_or("").trim().to_string();
            // Release 12.10.2026: ohne `src` ein Kandidat aus `srcset` (HTML
            // §4.8.4.3.6 „select an image source"). GEMESSEN vorher (WPT
            // img.complete „async srcset removal"): `new Image()` mit nur
            // `srcset` lud nie, `load` kam nicht.
            if let Some(ss) = l.dok.attribut(k, "srcset").filter(|t| !t.trim().is_empty()) {
                if let Some(w) = narsil_page::hteile::srcset_waehlen(ss, l.dok.attribut(k, "sizes"), 800) {
                    src = w;
                }
            }
            if spur {
                eprintln!("LOSES-BILD k={k} src={src:?} srcset={:?}", l.dok.attribut(k, "srcset"));
            }
            // Eingehaengte Bilder meldet der Seitenweg (nach dem Neubau) —
            // except those without a box (`display: none`, Release
            // 15.10.2026), which the page does not list.
            let quelle = narsil_url::loese_auf(&st.adresse, &src).unwrap_or_else(|_| src.clone());
            if verbunden && seite_bilder.iter().any(|b| b.bild.quelle == quelle) {
                continue;
            }
            // Release 2026-10-15: a connected `loading=lazy` image without a
            // box never reaches the viewport, so it does not load (HTML
            // §2.5.7 "will lazy load element steps"). MEASURED before (WPT
            // update-the-image-data/src-then-lazy-load, 8 failures): the
            // `display: none` path above loaded it.
            if verbunden && l.dok.attribut(k, "loading").is_some_and(|v| v.eq_ignore_ascii_case("lazy")) {
                continue;
            }
            let Some(l) = st.lebend.as_mut() else { break };
            let Some(host) = l.host(k) else { continue };
            st.lose_bilder.retain(|(h, _)| *h != host);
            if src.is_empty() || st.lose_bilder.len() >= MAX_LOSE_BILDER {
                continue;
            }
            st.lose_bilder.push((host, quelle));
        }
        let mut offen = Vec::new();
        for (host, q) in std::mem::take(&mut st.lose_bilder) {
            let Some(erg) = bilder.geladen.get(&q) else {
                offen.push((host, q));
                continue;
            };
            if st.bild_gemeldet.get(&host).is_some_and(|g| g.0 == q) && !st.bild_erneut.remove(&host) {
                continue;
            }
            st.bild_erneut.remove(&host);
            let (w, h, art) = match erg {
                Ok(b) => (b.natur_breite, b.natur_hoehe, "load"),
                Err(_) => (0, 0, "error"),
            };
            if !st.bilder_fertig.contains_key(&q) && st.bilder_fertig.len() < 8192 {
                st.bilder_fertig.insert(q.clone(), (w, h));
                if let Ok(b) = erg
                    && b.vector.is_some()
                {
                    st.bilder_vektor.insert(q.clone(), b.layout_masse());
                }
            }
            if let Ok(b) = erg {
                Self::bild_pixel_merken(st, &q, b);
            }
            st.bild_gemeldet.insert(host, (q, w, h));
            let hoert = !st.event_listeners.snapshot(host, art).is_empty()
                || st.horcher.contains_key(&(host, art.to_string()));
            if spur {
                eprintln!("LOSES-EREIGNIS {art} host={host} hoert={hoert}");
            }
            if hoert && faellig.len() < max {
                faellig.push((host, Vec::new(), art));
            }
        }
        st.lose_bilder = offen;
    }

    /// Release 10.10.2026: mehrere nicht aufsteigende Ereignisse in EINEM
    /// Lauf (Baum und Ids werden einmal uebergeben, das Netz einmal gepumpt)
    /// — fuer `bild_ereignisse`. GEMESSEN, warum: `ereignis_fuer_seite`
    /// kopiert je Aufruf den ganzen CSS-Baum (`setze_baum`).
    fn ereignisse_fuer_seite(a: &mut Angezeigt, liste: &[(u32, Vec<u32>, &str)]) -> crate::ereignis_host::DispatchResult {
        let Some(mut engine) = a.engine.take() else { return Default::default(); };
        let fallback = {
            let mut host = a.kiste.lock().expect("Kiste");
            host.setze_ids(a.seite.ids.iter().map(|(name,ord)|(name.clone(),*ord as u32)).collect());
            host.setze_baum(&a.seite.css_baum, &a.seite.css_zu_dom);
            host.setze_dom_zu_ord(&a.seite.dom_zu_ord);
            host.horcher.clone()
        };
        let mut result = crate::ereignis_host::DispatchResult::default();
        for (ziel, vorfahren, art) in liste {
            let ord = a.stand.lock().expect("DOM").ord.get(ziel).copied().unwrap_or(0) as u32;
            a.kiste.lock().expect("Kiste").klick = Some((ord, 0.0, 0.0));
            let r = crate::ereignis_host::dispatch(&mut engine, &a.stand, &a.kiste,
                *ziel, vorfahren, art, false, false, 0.0, 0.0, &fallback);
            result.invoked |= r.invoked;
            result.errors.extend(r.errors);
            result.console.extend(r.console);
            result.commands.extend(r.commands);
        }
        {
            let (k_dyn, f_dyn) = crate::js::dyn_skripte_pumpen(&mut engine, &a.kiste, &a.stand);
            result.console.extend(k_dyn);
            result.errors.extend(f_dyn);
        }
        if let Err(e) = engine.netz_schleife() {
            result.errors.push(format!("fetch: {e}"));
        }
        result.errors.extend(crate::js::module_pumpen(&mut engine));
        {
            let mut st = a.stand.lock().expect("DOM");
            let mut nach = st.nimm_befehle();
            drop(st);
            nach.extend(a.kiste.lock().expect("Kiste").nimm_befehle());
            result.commands.extend(nach);
        }
        a.engine = Some(engine);
        result
    }

    fn ereignis_fuer_seite(a: &mut Angezeigt, target: u32, ancestors: &[u32],
        kind: &str, bubbles: bool, cancelable: bool, x: f32, y: f32) -> crate::ereignis_host::DispatchResult {
        let Some(mut engine) = a.engine.take() else { return Default::default(); };
        let fallback = {
            let mut host = a.kiste.lock().expect("Kiste");
            host.klick = Some((a.stand.lock().expect("DOM").ord.get(&target).copied().unwrap_or(0) as u32,x,y));
            host.setze_ids(a.seite.ids.iter().map(|(name,ord)|(name.clone(),*ord as u32)).collect());
            host.setze_baum(&a.seite.css_baum, &a.seite.css_zu_dom);
            host.setze_dom_zu_ord(&a.seite.dom_zu_ord);
            host.horcher.clone()
        };
        let result = crate::ereignis_host::dispatch(&mut engine, &a.stand, &a.kiste,
            target, ancestors, kind, bubbles, cancelable, x, y, &fallback);
        let mut result = result;
        // Release 10.10.2026: eingefuegte `<script>` aus dem Handler.
        {
            let (k_dyn, f_dyn) = crate::js::dyn_skripte_pumpen(&mut engine, &a.kiste, &a.stand);
            result.console.extend(k_dyn);
            result.errors.extend(f_dyn);
        }
        // A4/E (06.10.2026): fetch im Handler. Der Netz-Pump laeuft NACH
        // dem Verteiler und VOR dem Patch — die `.then`-Kette eines
        // `fetch` im Handler darf die Seite noch in diesem Klick aendern.
        if let Err(e) = engine.netz_schleife() {
            result.errors.push(format!("fetch: {e}"));
        }
        // E: ein dynamischer `import('…')` aus einem Handler.
        result.errors.extend(crate::js::module_pumpen(&mut engine));
        {
            let mut st = a.stand.lock().expect("DOM");
            let mut nach = st.nimm_befehle();
            drop(st);
            nach.extend(a.kiste.lock().expect("Kiste").nimm_befehle());
            result.commands.extend(nach);
        }
        a.engine = Some(engine);
        result
    }

    fn ereignis_anwenden(a: &mut Angezeigt, mut result: crate::ereignis_host::DispatchResult) -> bool {
        a.konsole.append(&mut result.console);
        a.konsole.extend(result.errors.into_iter().map(|e|format!("Handler: {e}")));
        // 06.10.2026: Host-Meldungen (`alert`) aus Handler/Timer.
        {
            let mut st = a.stand.lock().expect("DOM");
            a.konsole.append(&mut st.meldungen);
        }
        a.konsole.truncate(crate::js::MAX_KONSOLE);
        let mut angewendet = false;
        if result.commands.iter().any(|b|b.veraendert_baum()) {
            let mut replay = crate::replay::spielen(&mut a.patch, result.commands);
            crate::replay::neue_knoten_uebernehmen(&mut a.patch, &a.stand, &mut replay);
            a.meldungen.extend(replay.fehler);
            a.meldungen.truncate(crate::js::MAX_KONSOLE);
            angewendet = replay.angewendet > 0;
        }
        // Release 09.10.2026: das lebende DOM (Aenderungen ohne Befehle).
        let uhr = takt_zeiten_an().then(std::time::Instant::now);
        if let Some(html) = crate::js::lebend_abschliessen(&a.stand, &mut a.patch) {
            a.html = std::sync::Arc::new(html);
            angewendet = true;
        }
        if let Some(u) = uhr {
            eprintln!("TAKT lebend_abschliessen {:.1} ms", u.elapsed().as_secs_f64() * 1000.0);
        }
        angewendet
    }

    /// Release 10.10.2026: Quelltext im Kontext der angezeigten Seite
    /// ausfuehren (wie eine DevTools-Konsole) — liefert Konsole und Fehler
    /// dieses Laufs. DOM-Aenderungen werden wie bei einem Timer
    /// uebernommen.
    pub fn seiten_js(&mut self, code: &str) -> Vec<String> {
        let Some(a) = self.angezeigt.as_mut() else { return vec!["keine Seite".to_string()] };
        let Some(mut engine) = a.engine.take() else { return vec!["keine Engine".to_string()] };
        let e = crate::js::skript_auf_seitenengine(code, &mut engine, &a.kiste, a.stand.clone());
        let mut raus: Vec<String> = e.konsole.clone();
        if let Some(f) = e.fehler {
            raus.push(format!("Fehler: {f}"));
        }
        a.engine = Some(engine);
        let geaendert = crate::js::lebend_abschliessen(&a.stand, &mut a.patch);
        if let Some(html) = geaendert {
            a.html = std::sync::Arc::new(html);
            self.seite_neu_bauen();
        }
        raus
    }

    /// Release 14.10.2026: the engine time if a timer is already due when
    /// parsing ends (it then runs before `load`), else `None`.
    fn faellige_timer_grenze(a: &mut Angezeigt) -> Option<u64> {
        let engine = a.engine.as_mut()?;
        engine.uhr_auf_wanduhr();
        let jetzt = engine.uhr_ms();
        engine.timer_frueheste_ms().is_some_and(|t| t <= jetzt).then_some(jetzt)
    }

    /// Release 14.10.2026: is a timer queued before the load task still
    /// waiting? Timers set later (e.g. a repeating `setInterval(f, 0)`) are
    /// due after the limit and do not hold `load` back.
    fn timer_vor_load(a: &Angezeigt) -> bool {
        match (a.load_timer_grenze, a.engine.as_ref()) {
            (Some(g), Some(e)) => e.timer_frueheste_ms().is_some_and(|t| t <= g),
            _ => false,
        }
    }

    /// Release 11.10.2026: der Zaehler `__narsilLadeSperre` der Seite (0 =
    /// nichts haelt `load` zurueck).
    fn lade_sperre(a: &mut Angezeigt) -> u32 {
        // Release 15.10.2026: images outside the document still being
        // fetched delay `window.load` too (HTML "update the image data"
        // delays the load event of the node document). MEASURED before (WPT
        // html/canvas drawimage_html_image): a `createElement('img')` drawn
        // in `window.onload` had not loaded yet, the canvas stayed empty.
        // Detached images hold `load` for at most `LOSE_BILDER_FRIST`: a
        // page that preloads many of them (sprites) still gets its `load`
        // promptly once its own images are done. Images in the document
        // without a box (`display: none`) count like other document images.
        let lose_zaehlen = a.load_ausstehend.is_none_or(|seit| seit.elapsed() < LOSE_BILDER_FRIST);
        let lose = a.stand.lock().map_or(0, |st| {
            let offen = st.lebend.as_ref().map_or(0, |l| l.bild_quellen.len());
            let wartend = st
                .lose_bilder
                .iter()
                .filter(|(host, _)| lose_zaehlen || st.lebend.as_ref().is_some_and(|l| l.knoten(*host).is_some_and(|k| l.ist_verbunden(k))))
                .count();
            if lose_zaehlen { offen + wartend } else { wartend }
        }) as u32;
        let Some(engine) = a.engine.as_mut() else { return lose };
        lose + match engine.ausfuehren("(globalThis.__narsilLadeSperre | 0)") {
            Ok(narsil_jsvm::Wert::Zahl(n)) if n > 0.0 => n as u32,
            _ => 0,
        }
    }

    /// `document.readyState = "complete"` und `load` am Fenster; `true`,
    /// wenn die Seite dabei etwas geaendert hat.
    fn load_feuern(a: &mut Angezeigt) -> bool {
        {
            let mut state = a.stand.lock().expect("DOM");
            state.ready_state = "complete";
            state.lifecycle_complete = true;
        }
        let load = Self::ereignis_fuer_seite(a, crate::dom::WINDOW_HOST_ID, &[], "load", false, false, 0.0, 0.0);
        Self::ereignis_anwenden(a, load)
    }

    pub(crate) fn ereignis_ausloesen(&mut self, target: u32, ancestors: &[u32], kind: &str,
        bubbles: bool, cancelable: bool, x: f32, y: f32) -> bool {
        let Some(a) = self.angezeigt.as_mut() else { return false; };
        let result = Self::ereignis_fuer_seite(a, target, ancestors, kind, bubbles, cancelable, x, y);
        let invoked = result.invoked;
        if Self::ereignis_anwenden(a, result) { self.seite_neu_bauen(); }
        self.schmutzig |= invoked;
        invoked
    }

    // ------------------------------------------------------------------
    // Zeichnen
    // ------------------------------------------------------------------

    /// Setzt die Schrift, mit der ab jetzt geladen wird (W5E, 2).
    ///
    /// DER WEG: `main.rs` sucht EINMAL eine Systemschrift
    /// (`Schrift::mit_systemschrift`) und gibt sie hierher und an den
    /// Rasterizer. Zwei Orte, eine Schrift.
    ///
    /// `None` stellt auf die eingebaute Bitmap zurueck — mit allem, was
    /// daran haengt: Umbruch, Boxbreiten, Vorschub.
    /// NARSIL_HTML_SPEICHERN: Wohin das **Rohtext**-HTML der naechsten
    /// Seite geschrieben wird (06.10.2026 06:48).
    ///
    /// **GEMESSENER GRUND:** `Angezeigt.html` ist der **geparste** Baum
    /// (`Arc<Vec<HtmlNode>>`), nicht der Rohtext — der Rohtext ist die
    /// Parameter-Variable von `aus_ist` und wird nach dem Parse
    /// verworfen. Damit ist eine laufende Messung **nicht
    /// nachpruefbar**: um 06:44 blieb die Frage „hatten wir um 03:11 auf
    /// go.dev 41 oder 56 `<img>`?" **ohne Antwort**, und die Differenz
    /// 119 gegen 143 ist bis heute nicht geschlossen.
    ///
    /// **EINMAL** vor `gehe_zu` setzen, nicht je Seite: es ist der Weg
    /// fuer die naechste Seite. **Und es ist ein Feld, keine
    /// Umgebungsvariable** — der Probe laedt parallel in Threads, und
    /// eine Umgebungsvariable ist prozessweit (siehe das Feld).
    pub fn setze_html_ziel(&mut self, ziel: Option<std::path::PathBuf>) {
        self.html_ziel = ziel;
    }

    pub fn setze_schrift(&mut self, schrift: Option<std::sync::Arc<narsil_text::TrueTypeFont>>) {
        self.messung = match schrift {
            Some(f) => narsil_layout::Messung::systemschrift(f),
            None => narsil_layout::Messung::bitmap(),
        };
    }

    /// Setzt die Messung direkt (W5G).
    ///
    /// Das ist der Weg des Browsers: `Schrift` traegt die Messung, und
    /// der gibt sie hierher — dadurch kann Zeichnen und Umbruch gar
    /// nicht auseinanderlaufen.
    pub fn setze_messung(&mut self, messung: narsil_layout::Messung) {
        // Release 07.10.2026: Schnitte UND Familienquelle anhängen — dieselbe
        // Dateiwahl, mit der `narsil-raster` zeichnet. GEMESSENER FEHLER: die
        // Messung kannte nur die Hauptschrift (Windows: arial.ttf), gezeichnet
        // wurde `sans-serif` mit segoeui.ttf. „1 Plain: Servo is a web " war
        // gemessen 167,2 px, die Tinte 152 px; ein fetter Lauf war zu schmal.
        // Folge auf jeder Seite: Lücken vor und Überlappung nach Inline-Elementen
        // („web  browserrendering").
        self.messung = narsil_raster::schrift::messung_mit_schnitten(messung);
        // Release 08.10.2026 (Leistentext): Tabtitel und Adresstext werden
        // ueber die Masse der Schrift mittig gesetzt, die `sans-serif`
        // wirklich zeichnet.
        if let Some(mass) = oberflaechen_schriftmass() {
            self.chrome.setze_schrift_mass(mass);
        }
    }

    /// Die Messung, mit der geladen wird.
    pub fn messung(&self) -> &narsil_layout::Messung {
        &self.messung
    }

    /// Der Zustand, mit dem der Wirt die Seite baut (CSS Media Queries 5,
    /// §2.2: `prefers-color-scheme` beschreibt die Anzeige).
    ///
    /// GEMESSENE BEGRUENDUNG, warum `KEINER` und nicht `Zustand::default()`:
    /// `KEINER` ist die ausdruecklich leere Vorlage (`typen.rs:2146`), und
    /// ein Feld, das sich still aendert, wenn jemand ein neues Feld in
    /// `Zustand` aufnimmt, waere eine Falle. GEMESSEN: `Zustand::KEINER`
    /// traegt `farbschema: Farbschema::Hell` (`typen.rs:2160`).
    fn zustand(&self) -> narsil_css::Zustand {
        narsil_css::Zustand {
            farbschema: self.farbschema,
            ..narsil_css::Zustand::KEINER
        }
    }

    /// Das Farbschema des Wirts setzen und die Seite **neu bauen**.
    ///
    /// GEMESSENE BEGRUENDUNG, warum hier ein Neubau passiert und nicht erst
    /// beim naechsten Laden: die Seite steht bereits da, und ohne Neubau
    /// bliebe die alte Farbe im Bild stehen, bis jemand die Seite neu laedt.
    /// Das ist dieselbe Pflicht, die bei einem Klick gilt (siehe
    /// [`Sitzung::seite_neu_bauen`]).
    ///
    /// GEMESSENE EINORDNUNG, warum die Fensterbreite nicht zurueckspringt:
    /// der Neubau baut aus dem geparsten Baum der SITZUNG, nicht aus einer
    /// neuen Sitzung — die Seitengewaechse (Tab, Verlauf, Bild, Patch)
    /// bleiben, weil `Angezeigt` erhalten bleibt und nur `.seite` und
    /// `.baum` ersetzt werden.
    pub fn setze_farbschema(&mut self, farbschema: narsil_css::Farbschema) {
        self.farbschema = farbschema;
        self.seite_neu_bauen();
    }

    /// Das Farbschema des Wirts (CSS Media Queries 5, §2.2).
    pub fn farbschema(&self) -> narsil_css::Farbschema {
        self.farbschema
    }

    /// 02.10. 01:55: DIE SEITE FOLGT DEM THEMA DER BEDIENFLAECHE.
    ///
    /// GEMESSENER STAND, bevor es diese Methode gab: `Chrome::thema` ist ein
    /// Zustand des Fensters (die Rolle UI baut ihn, `narsil-chrome/src/
    /// einstellungen.rs:244` — `Feld::Thema` mit „Hell" und „Dunkel"), und
    /// `Sitzung::farbschema` war ein eigenes Feld daneben. Zwei Felder fuer
    /// eine Sache: der Schalter konnte umgestellt werden, ohne dass die
    /// Seite etwas davon merkte, und `Sitzung` hatte keine Stelle, an der
    /// sie es merken koennte.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es hier und nicht im Menue: der Weg
    /// von einem Klick bis zur Seite laeuft ueber `ereignis` — das ist der
    /// einzige Eingang des Fensters, und dort steht diese Methode am Anfang.
    /// GEMESSENE EINORDNUNG der Kosten: ein `match` ueber ein
    /// Zwei-Werte-Enum und ein Vergleich je Ereignis. Der Neuaufbau laeuft
    /// **nur**, wenn sich das Thema tatsaechlich geaendert hat.
    ///
    /// GEMESSENE EINORDNUNG, warum `Keine` (kein Wunsch des Nutzers) hier
    /// nicht vorkommt: die Bedienflaeche kennt zwei Zustaende, hell und
    /// dunkel. Wer „kein Wunsch" sagen will, braegt eine dritte Auswahl in
    /// `Feld::Thema` — und bis dahin ist „dunkel eingestellt" dasselbe wie
    /// „der Nutzer will dunkel", was CSS Media Queries 5, §2.2 `dark` nennt.
    pub fn thema_uebernehmen(&mut self) -> bool {
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Vergleich gegen `gelesenes_thema`
        // und nicht gegen `farbschema`: sonst heisst der erste Aufruf immer
        // „anders" (das Fenster startet dunkel, die Seite war hell) — und
        // wikipedia waechste bei 887 von 1303 Knoten auf einmal die Farbe.
        // Diese Regel sagt: **die Seite folgt dem Fenster ab dem ersten
        // Unterschied**, und ein Fenster, das nie umgestellt wird, laesst
        // die Seite so, wie sie war.
        //
        // GEMESSENE EINORDNUNG, warum das keine verpasste Funktion ist: der
        // Weg, den die Seite auch ohne Aenderung geht, ist
        // `Sitzung::aus_html` — dort wird `self.farbschema` gelesen, und das
        // ist die Vorgabe. Wer die Seite auch in der Voreinstellung dunkel
        // haben will, stellt die Vorgabe **an einer Stelle** um (das Feld
        // `farbschema`), und beide Wege — Voreinstellung und Umschalten —
        // benutzen denselben Wert.
        //
        // GEMESSENE KORREKTUR AN DER RUECKGABE (02:15, `thema_der_
        // bedienflaeche` Test 3): sie sagte „ich habe die Seite neu gebaut",
        // als der Aufrufer „hat sich das Thema geaendert" wissen will. Beides
        // ist richtig und nicht dasselbe: der Nutzer kann das Fenster auf
        // **Hell** stellen, wenn die Seite schon hell ist — dann hat sich das
        // Thema geaendert, aber die Seite nicht.
        //
        // GEMESSENE ENTSCHEIDUNG: `true` heisst **das Thema hat sich
        // geaendert** (genau einmal je Nutzeraktion), und der Neuaufbau
        // passiert nur, wenn die Seite dadurch auch anders aussieht. Damit
        // bleibt die Kostenregel erhalten (ein Fenster mit 60 Mausbewegungen
        // je Sekunde baut nichts) und die Antwort ist die, die ein Aufrufer
        // braucht.
        if self.chrome.thema == self.gelesenes_thema {
            return false;
        }
        self.gelesenes_thema = self.chrome.thema;
        let gewollt = match self.chrome.thema {
            narsil_chrome::thema::Thema::Hell => narsil_css::Farbschema::Hell,
            narsil_chrome::thema::Thema::Dunkel => narsil_css::Farbschema::Dunkel,
        };
        if gewollt != self.farbschema {
            self.farbschema = gewollt;
            self.seite_neu_bauen();
            self.schmutzig = true;
        }
        true
    }

    /// Wo ein Text auf dem BILDSCHIRM steht (W6E) — in Klick-Koordinaten.
    ///
    /// # WARUM DIESE METHODE (Koordinationsbefund 30.09.2026, 16:40)
    ///
    /// „21 Klicks rund um den Knopf 'Klick mich' um den Textursprung +/-
    /// 20 px — `klick()` liefert immer `false`." Zweiraeume gab es da:
    /// `Sitzung::display_list` liefert BILDSCHIRM-Koordinaten (die Leiste
    /// ist schon dazugezaehlt), `narsil_layout_engine::build_display_list`
    /// liefert DOKUMENT-Koordinaten (y=0 ist die Seite, ohne Leiste). Die
    /// beiden unterscheiden sich um genau `SEITEN_START_Y` (32 px) — und 32
    /// px sind bei einem 19-px hohen Text der Unterschied zwischen „Treffer"
    /// und „Leiste".
    ///
    /// Diese Methode liefert den Bildschirm-Punkt, mit dem
    /// [`Sitzung::klick`] den Text wirklich trifft. Wer testen will, was
    /// ein Nutzer sieht, nimmt den Punkt von hier.
    ///
    /// GEMESSEN, warum die SUCHE und nicht `hit_test`: `hit_test` fragt
    /// einen Punkt ab, hier wird der Text gesucht — fuer einen Test ist das
    /// die Richtung, die man kennt.
    ///
    /// # DREI DINGE, DIE DIESE METHODE NICHT KANN
    ///
    /// 1. Sie findet den ERSTEN Knoten mit diesem Text. Zwei gleiche
    ///    Texte sind fuer einen Test derselbe Befund.
    /// 2. Sie liefert den Ursprung der Grundlinie, nicht die Mitte. Wer
    ///    die Mitte braucht, nimmt `+ halbe Zeilenhoehe` (der Text ist
    ///    `font_size` hoch, `DisplayItem::Text::font_size`).
    /// 3. Sie sieht nichts, was nicht als Text gezeichnet wird — ein Knopf
    ///    ohne Beschriftung hat hier keinen Punkt.
    pub fn text_ursprung(&self, text: &str) -> Option<(f32, f32)> {
        self.display_list().iter().find_map(|item| match item {
            DisplayItem::Text {
                origin, text: t, ..
            } if t == text => Some((origin.x, origin.y)),
            _ => None,
        })
    }

    /// Wie hoch ein Text in der Anzeige ist (W6E: die halbe Zeilenhoehe
    /// zum Klicken).
    pub fn text_hoehe(&self, text: &str) -> Option<f32> {
        self.display_list().iter().find_map(|item| match item {
            DisplayItem::Text {
                text: t, font_size, ..
            } if t == text => Some(*font_size),
            _ => None,
        })
    }

    /// Das fertige Bild: Leiste oben, Seite darunter, versetzt.
    ///
    /// DER ORT, an dem der Zustand zum Bild wird. Alles davor ist
    /// Rechnung, alles danach ist Malerei.
    ///
    /// W6E: die Koordinaten hier sind BILDSCHIRM-Koordinaten (die Leiste
    /// ist dazugezaehlt) — dasselbe, was [`Sitzung::klick`] will. Fuer
    /// einen Punkt, mit dem ein Text getroffen wird: [`Sitzung::text_ursprung`].
    pub fn display_list(&self) -> DisplayList {
        self.display_list_im_bereich(None)
    }

    /// Release 08.10.2026: die Anzeigeliste, deren SEITENteil nur die Kaesten
    /// im senkrechten `bereich` (Dokumentkoordinaten) enthaelt — fuer das
    /// Bild im Fenster (`sichtliste`). `None` = die ganze Seite (Tests,
    /// Probe, Ganzseitenbild). GEMESSEN vorher: 12,4 ms je Bild fuer den
    /// Aufbau aller 4 486 Eintraege einer langen Wikipedia-Seite.
    fn display_list_im_bereich(&self, bereich: Option<(f32, f32)>) -> DisplayList {
        let breite = self.breite.max(1);
        let hoehe = self.hoehe.max(1);
        //
        // DER CANVAS DER ANZEIGE: WEISS (Koordination, 03.10. 12:00).
        //
        // GEMESSENER BEFUND, der die Farbe bestimmt: `narsil-page` gibt
        // jedem Knoten `background: Color::TRANSPARENT` (der Bauer setzt
        // das fuer Zeilen-Container), also sieht man durch die Seite
        // hindurch den Canvas der Anzeige. DER stand auf der dunklen
        // Oberflaechenfarbe (0x0d, 0x0d, 0x11) — damit war jede Seite
        // ohne eigenes CSS dunkel (Abnahmebild wikipedia.org, 11:07).
        //
        // GEMESSENE ENTSCHEIDUNG, warum der CANVAS und nicht ein
        // zusatzliches weisses Rechteck ueber den Seitenbereich (der
        // erste Versuch, 11:20): der Canvas gilt fuer JEDEN Weg —
        // Fenster, Probe (`narsil-probe` rastert `breite x inhalt_hoehe`,
        // bis 2048 px hoch) und Tests. GEMESSEN, warum der erste Versuch
        // dort nicht galt: das Rechteck reichte nur bis `sicht_hoehe`
        // (650 - 58 - 24 = 568 px), und darunter kam wieder der Canvas —
        // im Probe-Bild also ab y=568 wieder dunkel
        // (`team/log/befund_arabisch_1200.png`, Befund der Koordination).
        //
        // GEMESSENE BEGRUENDUNG, warum die LEISTEN trotzdem dunkel
        // bleiben: die Bedienflaeche malt ihre eigenen Rechtecke —
        // Tab-Leiste (0, 0, breite, 26), Werkzeugleiste
        // (0, 26, breite, 32) und Statuszeile (0, hoehe - 24, breite,
        // 24) — und die stehen NACH der Seite in der Liste (Abschnitt
        // 2b). Sie sind also gemessen dunkel, nicht angenommen (der Test
        // `koordination_die_leisten_bleiben_dunkel` prueft genau das,
        // und `tests/acceptance_dunkel.rs` malt das Bild dazu).
        //
        // GEMESSENE ENTSCHEIDUNG, warum das die UA-Vorgabe ist und keine
        // Designfrage: `html { background: white }` ist, was ein Browser
        // ohne CSS zeigt. Die dunkle Oberflaeche liegt UEBER der Seite,
        // nicht in ihr.
        let mut liste = DisplayList::with_background(Color::new(0xff, 0xff, 0xff, 255));

        //
        // DAS BLATT VON 11:20 IST WEG (12:05). GEMESSENER GRUND: es
        // reichte nur bis `sicht_hoehe`, und der Probe-Weg rastert bis
        // `inhalt_hoehe` hoch — darunter kam wieder der Canvas, also
        // wieder dunkel (`team/log/befund_arabisch_1200.png`). Der Canvas
        // der Anzeige traegt die Aussage jetzt an EINER Stelle, und die
        // gilt fuer jeden Weg: Fenster, Probe, Tests.
        //
        //
        // GEMESSENE ENTSCHEIDUNG (02.10. 18:4x, Koordination: „alles zu
        // klein und in einem blauen Rahmenkasten"; PROBLEME 16), warum die
        // Seite bei EINGEBAUTEN Seiten nicht gezeichnet wird:
        // `about:einstellungen` und eine PDF-Datei bekommen von mir eine
        // LEERE HTML-Seite (nur mit Titel), weil der Tab einen Titel braucht.
        // GEMESSENE FOLGE, die niemand gesehen hat, bis ich die Rechtecke
        // aus der Liste gezählt habe: diese leere Seite hat einen weissen
        // Canvas, und der lag als Rechteck (16,124) 1888x916 UNTER der
        // Seite der Einstellungen — als „blauer Rahmen" sichtbar, weil das
        // Weiss neben dem dunklen Chrome-Grund wie ein Kasten aussieht.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Seite hier ENTFAELLT und nicht
        // unsichtbar gemacht wird: die Einstellungen und der Betrachter
        // malen ihren Grund selbst (`einstellungen::display_list` und
        // `pdf_viewer::display_list` beginnen beide mit einem Grund-
        // Rechteck). Ein zweiter Grund unter ihnen ist der Fehler; ein
        // durchsichtiger Grund waere dasselbe mit mehr Schritten.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das nicht am Erkennen der Adresse
        // haengt, sondern am Zustand: dieselbe Frage stellen `display_list`
        // und `mausklick` (siehe dort), und beide benutzen
        // `einstellungen_weg::ist_einstellungen` — EIN Ort, an dem die
        // Frage beantwortet wird.
        let eingebaute_seite = {
            let adresse = self.browser.adresse().to_string();
            crate::einstellungen_weg::ist_einstellungen(&adresse) || self.pdf.is_some()
        };
        //
        // 2. Die Seite, um den Versatz nach oben geschoben.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Bedingung IM `if` steht und nicht
        // in einem `filter` (der Compiler hat gemessen: „cannot move out of
        // `self.angezeigt`", 18:4x): `filter` nimmt `self` by value. Das
        // `if` braucht kein Filter — es fragt dieselbe Frage.
        if let Some(a) = &self.angezeigt
            && !eingebaute_seite
        {
            let seiten_liste = self.seiten_liste(a, breite, hoehe, bereich);
            //
            // W7C A6 (10:30): DIE HERVORHEBUNG DES SUCHTREFFERS.
            //
            // GEMESSEN, warum das HIER und nicht in `Chrome` und nicht im
            // Patch: ein Treffer ist ein Textknoten, und ein Textknoten hat
            // keinen Hintergrund (der gehoert seinem Absatz). Ein
            // `style`-Attribut im Patch faerbte den ganzen Absatz — oder
            // gar nichts, wenn der Knoten kein Element ist (gemessen:
            // vier Tests rot, 0 gelbe Rechtecke im Bild).
            //
            // GEMESSEN, warum das Rechteck VOR dem Text kommt: der
            // Rasterizer malt in Listenreihenfolge, und ein Gelb ueber dem
            // Text waere ein gelber Fleck ohne Buchstaben.
            //
            // GEMESSEN, warum die Breite aus der MESSUNG kommt und nicht
            // aus `Zeichenanzahl * 8`: die eingebaute Schrift ist
            // proportional, `8 px je Zeichen` waere an jedem Wort
            // daneben. `Messung::breite` ist dieselbe Zahl, mit der der
            // Umbruch rechnet — eine Wahrheit.
            //
            // M3: hoechstens EIN Rechteck je Text (der aktuelle Treffer).
            // `suche_versatz` liefert hoechstens eines je Text, und ein
            // Text mit dem Muster kommt einmal vor — die Grenze ist damit
            // die Anzeige selbst, und es ist eine Grenze.
            //
            // Der Zaehler der Funde in Dokumentreihenfolge (siehe
            // `suche_fund_nr`): welcher Text der aktuelle Treffer ist.
            //
            // KONTEXT (04.10.2026, 23:30, Leitung „Treffer mit derselben
            // Textposition-Funktion wie die Auswahl“): DER WEG IST
            // [`Sitzung::suche_rechtecke`].
            //
            // GEMESSENER BEFUND, den dieser Weg **beseitigt**: vorher stand hier
            // ein Lauf ueber die **Displayliste** mit `suche_versatz`, und
            // `suche_versatz` liefert **hoechstens einen** Treffer je Zeile
            // (sein Kommentar: „M3: hoechstens EIN Rechteck je Text“). Eine
            // Zeile mit dem Muster **dreimal** zeigte also **einen** Treffer —
            // und `suche_fund_nr` zaehlte die Zeilen, nicht die Funde, sodass
            // „Treffer 2/3“ auch dann falsch war, wenn es drei Funde gab.
            //
            // GEMESSENE ENTSCHEIDUNG, warum die Rechtecke **vor** den
            // Seiten-Items kommen (und nicht je Item dazwischen): sie liegen
            // jetzt **ausserhalb** der Schleife ueber `seiten_liste` — ein
            // Trefferrechteck gehoert **vor** den Text, weil der Rasterizer in
            // Listenreihenfolge malt und ein Gelb **ueber** dem Text ein gelber
            // Fleck ohne Buchstaben waere (dieselbe Regel wie bei der Auswahl,
            // 22:20).
            let viewport = Rect::new(0.0, self.chrome.seiten_oberkante() as f32, breite as f32, self.sicht_hoehe() as f32);
            liste.push_clip(viewport);
            // Release 12.10.2026 (Agent CSS2, mit Okay des Koordinators):
            // `anhaengen` statt der Kopierschleife — dasselbe `push_mit_clip`
            // mit dem Viewport als aeusserem Clip, aber mit den Effekten und
            // den Buchstaben-/Wortabstaenden der Seite
            // (`DisplayList::versaetze_bei`). GEMESSEN vorher: die Abstaende
            // gingen hier verloren (68 WPT-Tests `text/letter-spacing-*`,
            // `word-spacing-*` ohne Abstand).
            liste.anhaengen(&seiten_liste);
            // Release 09.10.2026: die Treffer liegen jetzt UEBER der Seite,
            // durchscheinend wie die Auswahl. GEMESSEN vorher: sie standen VOR
            // der Seite in der Liste — jeder Seitenhintergrund (weisser Body,
            // graue Karte) deckte sie zu, auf den meisten Seiten war Strg+F
            // unsichtbar.
            for (r, aktuell) in self.suche_rechtecke() {
                liste.push(narsil_display::DisplayItem::Rect {
                    radius: [2.0; 4],
                    deckkraft: if aktuell { 0.55 } else { 0.6 },
                    schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                    schatten_anzahl: 0,
                    rect: r,
                    // ZWEI Farben, und die Unterscheidung ist der ganze
                    // Nutzen: blass fuer „hier ist ein Treffer“, stark fuer
                    // „du bist hier“.
                    color: if aktuell {
                        Color::new(0xFF, 0xC4, 0x00, 255)
                    } else {
                        Color::new(0xFF, 0xF3, 0xA0, 255)
                    },
                });
            }
            liste.pop_clip();
        }

        // 3. Die Meldung, wenn eine da ist. Sie steht ueber allem, weil
        //    sie die einzige Information ist, die der Nutzer sofort
        //    braucht.
        //
        // UI1 (03.10. 11:05): DER ALTE MELDUNGSBALKEN IST WEG.
        //
        // GEMESSEN, warum er weg muss: er lag auf den untersten 24 px —
        // genau den Platz, den jetzt die Statuszeile hat. GEMESSENE FOLGE,
        // bevor ich es gemerkt hatte: `meldung` stand im Bild ZWEIMAL
        // (der Test `ui1_ein_langer_text_wird_abgeschnitten` zaehlte zwei
        // Textstuecke mit demselben Inhalt), einmal in der Zeile der
        // Oberflaeche und einmal auf diesem Balken darueber.
        //
        // DER WEG: die Statuszeile traegt die Meldung, und die Farbe
        // unterscheidet sie vom leeren Zustand (sie wird oben
        // rotgestrichen, wenn eine Meldung da ist). GEMESSENE BEGRUENDUNG,
        // warum die Farbe bleibt und nicht verschwindet: „Meldung" und
        // „keine Meldung" muessen sich unterscheiden, und Rot ist der
        // Unterschied, den der Nutzer kennt.
        //
        // GEMESSEN (11:25): hier stand nur noch `let _ = chrome_liste;` —
        // ein leerer Abschnitt mit einem Restschatten aus der Zeit, als
        // die Bedienflaeche VOR der Seite gezeichnet wurde. Die Zeile ist
        // weg, weil die Bedienflaeche jetzt weiter unten (2b) gebaut wird
        // und die Meldung in der Statuszeile steht (1b).

        //
        //
        // 2b. DIE BEDIENFLAECHE, jetzt ÜBER der Seite (11:25).
        //
        // GEMESSENER BEFUND, der die Reihenfolge bestimmt: die Seite
        // zeichnet über ihre volle Höhe. GEMESSEN an der Wikipedia-Seite:
        // der Rumpf laeuft von y=74 bis y=642, das Fenster ist 600 hoch —
        // die Seite malt also über die Statuszeile hinweg (576..600).
        // Vor dieser Aenderung lag die Statuszeile UNTEN in der Liste und
        // wurde von langen Seiten ueberdeckt.
        //
        // GEMESSENE BEGRUENDUNG fuer „Leisten oben drueber" statt
        // „Seite beschneiden": die Liste kennt keinen Schnittbereich
        // (DisplayItem hat kein `clip`), und die Seite weiss ihre
        // Inhaltshoehe nicht. Die Reihenfolge ist die einzige Stelle, an
        // der sich beides sagen laesst — und sie ist an EINER Stelle
        // geaendert, nicht an sechzig.

        // 1. Die Bedienflaeche. Sie kommt OBEN in die Liste, damit die
        //    Seite sie nicht uebermalt.
        //
        // UI1: DIE OBERFLAECHE BRAUCHT ZWEI ZUSTAENDE, und beide stehen
        // hier, weil sie der Sitzung gehoeren: `leiste_im_fokus` (der
        // Nutzer tippt) und `laedt` (das Netz arbeitet).
        //
        // GEMESSEN, warum der Fokus hier und nicht in `Chrome`: `Chrome`
        // ist die Form. Der Fokus ist eine Eingabe, und die setzt die
        // Sitzung an vier Stellen (Maus auf die Leiste, Strg+L, Tab
        // gedrueckt, Escape). Ein Feld in `Chrome` muesste an allen vier
        // mitgepflegt werden — und die Oberflaechen-Tests, die ohne
        // Sitzung arbeiten, wuerden nie einen Fokus zeigen.
        let chrome_liste = self.chrome.to_display_list_mit_textmessung(
            self.leiste_im_fokus,
            self.laedt,
            self.hoehe,
            narsil_chrome::SchlossZustand::Keins,
            &|text, grad| self.messung_breite(text, grad),
        );
        //
        // 03.10. 16:50 (KOORDINATION, DRINGEND): DIE SEITENFLAECHE DES
        // THEMAS WIRD NICHT GEMALT.
        //
        // GEMESSENER BEFUND (16:30, Rechtecke in Listenreihenfolge fuer
        // `<html><body><p>Ein Absatz</p></body></html>`, Fenster 800x600):
        //     MESSUNG canvas=Some((255,255,255,255))
        //     MESSUNG #0 RECT (16,74 768x568) farbe=(255,255,255)  <- Seite
        //     MESSUNG #1 TEXT y=98 "Ein Absatz"
        //     MESSUNG #3 RECT (0,58 800x518) farbe=(20,20,26)      <- Taeter
        // (20,20,26) ist `Palette::dunkel().hintergrund =
        // Color::rgb(0x14,0x14,0x1a)` (`narsil-chrome/src/thema.rs:247`),
        // gemalt von `thema::liste_hintergrund` (`thema.rs:428`), aufgerufen
        // aus `to_display_list_...` (`lib.rs:1568`). Weil die Bedienflaeche
        // NACH der Seite in die Liste kommt (Abschnitt 2b, 11:25), lag das
        // Rechteck UEBER dem Inhalt.
        //
        // GEMESSENE FOLGE in der Probe (Koordination 03.10., Bild
        // `team/log/befund_hn_dunkel.png`, 1000x700): news.ycombinator.com
        // war GANZ DUNKEL, kein Text sichtbar, trotz 424 gemeldeter Texte.
        // GEMESSEN in `tests/hn_dunkel.rs`: vor diesem Fix 0 Pixel, die vom
        // Untergrund abweichen — der Text war da und unsichtbar.
        //
        // GEMESSENE BEGRUENDUNG, warum der HOST sie weglässt und nicht
        // `narsil-chrome`: die Fläche ist die einzige Stelle, an der die
        // Bedienflaeche den SEITENBEREICH nennt — ihre eigene Begruendung
        // (`thema.rs:392`) sagt, sie sei da, weil im hellen Thema die
        // Seitenflaeche schwarz war, als `DisplayList::new()` den Hintergrund
        // auf `None` laesst. GENAU DAS ist seit 12:00 behoben: `narsil` setzt
        // den Canvas mit `DisplayList::with_background(weiss)` (Zeile 5152).
        // Der Host LIEFERT die Schicht, die dem Thema fehlte — er malt sie
        // nicht noch einmal, und er faerbt sie auch nicht.
        //
        // GEMESSENE BEGRUENDUNG, warum WEGLASSEN und nicht NACH UNTEN
        // schieben: legte man sie unter die Seite, waere der Bereich UNTER
        // einem kurzen Dokument (gemessen: `OHNE_CSS` in
        // `tests/acceptance_dunkel.rs`, Punkt (700,560)) wieder die
        // Themenfarbe — und damit genau der Befund, den die Koordination am
        // 03.10. 11:07 und 12:00 gemeldet und behoben sehen wollte.
        //
        // GEMESSENE ENTSCHEIDUNG, warum an der GEOMETRIE erkannt und nicht
        // an der Farbe: die Farbe des Themas darf sich aendern (helles
        // Thema, spaeter ein helles Blatt) — die Aussage „das Rechteck
        // FUELLT den Seitenbereich" nicht. Es ist der einzige Punkt, an dem
        // die Bedienflaeche den ganzen Seitenbereich nennt (siehe
        // `ist_seitenflaeche`).
        // Release 09.10.2026: `behalte` + `anhaengen` statt Item-Schleife —
        // die Glasflaechen der Leiste (Nebentabelle) wandern mit.
        liste.anhaengen(&chrome_liste.behalte(|item| !self.ist_seitenflaeche(item, breite)));
        //
        // 1a-NEU. UI-VORLAGE SCHRITT 5: DIE EINSTELLUNGEN (Koordination
        // 02.10. mittags).
        //
        // GEMESSENE ENTSCHEIDUNG, warum die ADRESSE der Schalter ist und
        // kein Flag in der Sitzung: jeder Tab hat seine Adresse, also
        // stimmt die Frage „zeigt dieser Tab die Einstellungen?" immer —
        // auch nachdem der Nutzer in einem zweiten Tab weiterscrollt. Ein
        // Flag waere nach einem Tabwechsel veraltet und wuerde die
        // Einstellungen im falschen Tab zeigen.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Seite aus `narsil-chrome` kommt
        // und nicht aus meinem Werkzeug-Code: `WUNSCH_UI.txt` sagt es
        // ausdruecklich („`narsil-chrome` ist ZUSTAND und GEOMETRIE, `narsil`
        // zeichnet"), und die Rolle UI hat die Datei mit 25 Tests fertig.
        let adresse = self.browser.adresse().to_string();
        if crate::einstellungen_weg::ist_einstellungen(&adresse) {
            crate::einstellungen_weg::anhaengen(
                &self.einstell_seite,
                &self.einstellungen,
                &self.chrome,
                breite,
                hoehe,
                &mut liste,
            );
        }
        // Release 09.10.2026: das Download-Panel steht jetzt bei den Menues (ganz hinten).
        // 1b. DER TEXT DER STATUSZEILE.
        //
        // GEMESSENE BEGRUENDUNG, warum der Text HIER kommt und nicht in
        // `narsil-chrome`: `Chrome` kennt die Form, die Sitzung kennt den
        // Zustand (`meldung`, `konsole`). Ein Textfeld in `Chrome` muesste
        // an jeder Stelle mitgepflegt werden, an der eine Meldung
        // entsteht — und die gibt es in `narsil` an ueber zwanzig.
        //
        // M3: `MAX_MELDUNG` (8 KiB) und `MAX_MELDUNGEN_ZEICHEN` (200) —
        // eine Zeile, die laenger ist, wird abgeschnitten, nicht
        // umgebrochen. GEMESSENE BEGRUENDUNG: 200 Zeichen sind bei
        // `ZEICHENBREITE` 8 px genau 1600 px; alles darueber waere eine
        // zweite Zeile in einer Zeile, die 24 px hoch ist.
        if self.hoehe >= narsil_chrome::STATUSLEISTE_HOEHE {
            let st = self.chrome.statusleiste(self.hoehe);
            liste.push(narsil_display::DisplayItem::Rect {
                rect: Rect::new(st.x as f32, st.y as f32, st.breite as f32, st.hoehe as f32),
                color: Color::rgb(0x20, 0x22, 0x29), radius: [0.0; 4], deckkraft: 1.0,
                schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN], schatten_anzahl: 0,
            });
            let text = self.status_text();
            // DER ROTE GRUND bei einer Meldung (die alte Farbe des
            // Meldungsbalkens, 0x40/0x18/0x18 bei 235 Deckkraft). Er wird
            // VOR dem Text gelegt, damit der Text darauf steht.
            // Release 07.10.2026: nur bei einer FEHLERmeldung — vorher lag er
            // auch unter der Ladezeit und „Tab geschlossen".
            if !text.is_empty() && Self::status_ist_fehler(&text) {
                liste.push(narsil_display::DisplayItem::Rect {
                    radius: [0.0; 4],
                    deckkraft: 235.0 / 255.0,
                    schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                    //
                    // KONTEXT (04.10., 17:25): `DisplayItem::Rect` traegt die
                    // Anzahl der **benutzten** Schattenatten (GEMESSEN:
                    // `missing field schatten_anzahl`, E0063). Der Balken hat
                    // keinen Schatten, also **null** — und nicht etwa die volle
                    // Zahl, denn die Zahl ist eine **Angabe**, kein Fuellwert.
                    schatten_anzahl: 0,
                    rect: Rect::new(st.x as f32, st.y as f32, st.breite as f32, st.hoehe as f32),
                    color: Color::new(0x40, 0x18, 0x18, 255),
                });
            }
            if !text.is_empty() {
                let mut sichtbar: String = text.chars().take(MAX_STATUS_ZEICHEN).collect();
                if text.chars().count() > MAX_STATUS_ZEICHEN {
                    sichtbar.push('…');
                }
                liste.push(narsil_display::DisplayItem::Text {
                    // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
                    // traegt jetzt die `font-family`-Liste. Hier steht die
                    // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
                    // Bildschirmpunkte schreibt keine eigene Familie.
                    schriftfamilie: String::from("sans-serif"),
                    deko: 0,
                    origin: Point::new(st.x as f32 + 6.0, st.y as f32 + 4.0),
                    text: sichtbar,
                    color: Color::rgb(0xb0, 0xb0, 0xc0),
                    font_size: 14.0,
                    // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
                    // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
                    // alles aufrecht, weil kein Schnitt ankam.
                    gewicht: 400,
                    kursiv: false,
                });
            }
        }

        //
        //
        // 3. DAS RECHTSKLICK-MENUE, ganz ZULETZT (UI4, 03.10. 10:30).
        //
        // GEMESSENER FEHLER MEINES ERSTEN PLATZES (10:30): der Block stand
        // bei „1c" — zusammen mit der Bedienflaeche und der Statuszeile,
        // also VOR der Seite. GEMESSEN am Bild
        // (`team/log/ui4_*_rechtsklick_menue.png`): das Menue war nicht zu
        // sehen, obwohl der Zustandstest und der Listentest es gefunden
        // hatten — der weisse Seitenhintergrund liegt ueber ihm.
        //
        // GEMESSENE ENTSCHEIDUNG, warum ganz hinten und nicht „vor dem
        // Seitentext, aber nach dem Seitenhintergrund": die Liste
        // zeichnet in der Reihenfolge, in der sie steht, und der
        // Seitenhintergrund ist ein Item der Seite. Wer das Menue
        // dazwischen schiebt, muss die Seite aufteilen — und eine
        // Aufteilung ist eine zweite Wahrheit ueber „wo endet die Seite".
        //
        // 1c. DAS RECHTSKLICK-MENUE (UI4, 03.10. 09:40).
        //
        // GEMESSENE BEGRUENDUNG, warum es ganz oben in der Liste steht (und
        // nicht bei der Statuszeile): es ist ein FENSTER ueber der Seite,
        // und ein Fenster, das unter der Seite liegt, ist keins. Die
        // Bedienflaeche (1.) steht auch oben — das Menue wird deshalb
        // NACH ihr gezeichnet, damit es die Leiste ueberdeckt, wenn der
        // Klick nahe am oberen Rand war.
        //
        // GEMESSENE ENTSCHEIDUNG, warum hier KEIN Rand und kein Schatten
        // fuer die Zeile darunter: `DisplayItem::Rect` kann beides, aber
        // ein Schatten waere eine Behauptung ueber Licht in einer
        // Oberflaeche, die sonst kein Licht hat (alles ist flach). Die
        // Trennlinie ueber der Zeile reicht, um sie abzuheben.
        //
        // KONTEXT (03.10.2026): DER BLOCK, DER HIER STAND, IST WEG — und die
        // Zeichnung kommt jetzt aus `narsil_chrome::ktx_menue::display_list`.
        //
        // GEMESSENER BEFUND (03.10., `grep -rn "ktx_menue" crates/narsil/src` =
        // **0 Treffer**): dieser Block zeichnete das Menue **selbst** — mit
        // `ZEILE_MENUE_HOEHE` (22), `FLAECHE_MENUE_BREITE` (160), 1 px Luft,
        // 0,96 Deckkraft, eigener Schatten (0), harten Farben
        // (`0x33333e`/`0xd8d8e4`). Und `narsil-chrome::ktx_menue` zeichnet
        // dasselbe Menue mit **anderen** Zahlen: 24 px Zeile, 248 px Breite,
        // 4 px Luft, `Palette::adresse`, 1 px Versatz / 4 px weich Schatten.
        //
        // GEMESSENE FOLGE (der Grund fuer die Ersetzung): das Menue, was der
        // Nutzer **sieht**, und das Menue, was der **Klick** trifft, waren zwei
        // Programme. Und `ANFORDERUNGEN.txt` Zeile 20/21 (Kopieren ganz oben,
        // Untermenue „Konvertiert herunterladen >“) konnte **keines** der
        // beiden erfuellen — dieser Block kannte vier feste Texte.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `display_list` und nicht „die Liste
        // von `narsil` an `ktx_menue` uebergeben und dort zeichnen lassen“:
        // `display_list` **ist** diese Uebergabe. Es gibt in `ktx_menue` genau
        // **eine** Zeichenfunktion, und es gibt jetzt genau **einen** Aufruf.
        //
        // GEMESSENE ENTSCHEIDUNG fuer die Argumente: `self.chrome` (das Thema),
        // `self.breite`, `self.hoehe` — dieselben drei, die die alte Zeichnung
        // brauchte (Farbe und Fenstergroesse). Der **Punkt** steht im Menue
        // selbst (`Menue::x`/`Menue::y`, aus `Menue::oeffne`).
        // Release 09.10.2026: Rechtsklick- und Tabmenue stehen jetzt GANZ HINTEN
        // (vor den Feature-Hinweisen) — siehe dort.

        //
        // KONTEXT (04.10.2026, 22:20, Leitung „Hervorhebung als Rects ueber
        // Text … ::selection-Farbe“): DIE **AUSWAHL** — vor den Menues, weil
        // sie **ueber** dem Text, aber **unter** dem Menue liegen muss.
        //
        // GEMESSENE ENTSCHEIDUNG, warum **ein** Farbwert und kein Themenwert:
        // GEMESSEN ist, dass `narsil-page` keine `::selection`-Regel ausrechnet
        // (`grep -rlnE "selection" crates/narsil-page/src/*.rs` = 0), und die
        // **Vorgabefarbe** des Systems hat dieser Browser nicht. Also eine
        // gemessene Zahl: Firefox hell `#3390ff`, dunkel `#4a4a4a` — hier
        // **halbtransparent**, damit der Text darunter lesbar bleibt
        // (Deckung 0,4), was Firefox mit seinem Systemakzent ebenfalls tut.
        //
        // KONTEXT (04.10.2026, 01:18, Leitung „Fokusring“): DER RING UM DAS
        // FOKUSSIERTE FELD.
        //
        // GEMESSENE ENTSCHEIDUNG, warum **vier** schmale Rechtecke und nicht
        // eines: GEMESSEN ist, dass `DisplayItem::Rect` **fuellt** — ein
        // „Rahmen“ gibt es im Display-Item nicht. Ein fuellendes Rechteck **um**
        // das Feld waere ein zweiter Kasten (und verdeckte den Text), also
        // wird der Ring aus vier Streifen gebaut — das ist, was Firefox auch
        // zeichnet, nur mit einem anderen Zeichenbefehl.
        //
        // KONTEXT (05.10.2026, 07:55): DER FELD-UEBERZUG (Passwortpunkte,
        // Platzhalter, range, color). GEMESSENE ENTSCHEIDUNG, warum **vor** dem
        // Fokusring: der Ring ist ein **Rand** um den Kasten, der Ueberzug
        // **fuellt** ihn — waere der Ring zuerst gezeichnet, wuerde der
        // Ueberzug ihn **zudecken** (die vier Streifen liegen auf dem Kasten).
        let mut felder = DisplayList::new();
        // Release 09.10.2026: das Download-Panel deckt die Seite zu — ihre
        // Felder, ihr Cursor und ihr Fokusring gehoeren nicht darueber.
        // GEMESSEN vorher: das Suchfeld der Startseite stand mitten im Panel.
        // Release 09.10.2026: das Download-Panel ist ein schwebendes Glasfeld
        // und liegt (spaeter gezeichnet) UEBER den Feldern — die Seite bleibt.
        let seite_verdeckt = false;
        if !seite_verdeckt {
            self.feld_ueberzug(&mut felder);
        }
        // Release 08.10.2026: der Cursor UEBER dem Ueberzug (siehe
        // `seiten_liste`).
        if !seite_verdeckt && let Some(c) = self.cursor_rechteck() {
            felder.push(c);
        }
        let dy =self.chrome.seiten_oberkante() as f32 - self.angezeigt.as_ref().map_or(0.0, |a| a.versatz as f32);
        felder.schiebe_alle(dy);
        let seiten_clip = Rect::new(0.0, self.chrome.seiten_oberkante() as f32, breite as f32, self.sicht_hoehe() as f32);
        for (nr, item) in felder.iter().enumerate() {
            let clip = felder.clip_at(nr).map_or(seiten_clip, |c| c.intersect(seiten_clip).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)));
            liste.push_mit_clip(item.clone(), Some(clip));
        }
        // KONTEXT (05.10.2026, 08:50): DER DIALOG LIEGT **UEBER** allem —
        // auch ueber dem Feld-Überzug und dem Fokusring, denn er ist ein Modus.
        self.datei_dialog_zeichnen(&mut liste);
        if let Some(fr) = self.fokus_rechteck().filter(|_| !seite_verdeckt) {
            let pal = narsil_chrome::thema::Palette::fuer(self.chrome.thema);
            let streifen = 2.0f32;
            // Release 08.10.2026: ein rundes Feld (oder ein runder Knopf)
            // bekommt einen runden Ring — derselbe Radius wie sein
            // Rahmenkasten. GEMESSEN vorher: ein eckiger Ring um die Pille
            // der Startseite, die Ecken standen ausserhalb des Feldes.
            let radius = self
                .angezeigt
                .as_ref()
                .zip(self.feld_fokus.or(self.fokus_knoten))
                .and_then(|(a, ord)| feld_bilder(a, &[ord]).remove(&ord))
                .map(|b| b.radius_aussen)
                .unwrap_or([0.0; 4]);
            if radius.iter().any(|r| *r > 0.0) {
                use narsil_core::style::{Rahmen, RahmenStil};
                let mut farbe = pal.akzent;
                farbe.a = 230;
                liste.push(narsil_display::DisplayItem::Rand {
                    rect: fr,
                    rahmen: Rahmen {
                        breite: [streifen; 4],
                        stil: [RahmenStil::Durchgezogen; 4],
                        farbe: [farbe; 4],
                    },
                    radius,
                });
            }
            let eckig = !radius.iter().any(|r| *r > 0.0);
            for (x, y, b, h) in [
                (fr.x, fr.y, fr.width, streifen),
                (fr.x, fr.y + fr.height - streifen, fr.width, streifen),
                (fr.x, fr.y, streifen, fr.height),
                (fr.x + fr.width - streifen, fr.y, streifen, fr.height),
            ]
            .into_iter()
            .filter(|_| eckig)
            {
                liste.push(narsil_display::DisplayItem::Rect {
                    rect: Rect::new(x, y, b, h),
                    color: pal.akzent,
                    radius: [0.0; 4],
                    deckkraft: 0.9,
                    schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                    schatten_anzahl: 0,
                });
            }
        }
        if !self.auswahl.ist_leer() {
            let hell = matches!(self.chrome.thema, narsil_chrome::thema::Thema::Hell);
            let farbe = if hell {
                narsil_core::color::Color {
                    r: 0x33,
                    g: 0x90,
                    b: 0xff,
                    a: 0xff,
                }
            } else {
                narsil_core::color::Color {
                    r: 0x4a,
                    g: 0x4a,
                    b: 0x4a,
                    a: 0xff,
                }
            };
            for r in self.auswahl_rects() {
                liste.push(narsil_display::DisplayItem::Rect {
                    rect: r,
                    color: farbe,
                    radius: [0.0; 4],
                    deckkraft: 0.4,
                    schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                    schatten_anzahl: 0,
                });
            }
        }

        // U1c (01.10.2026, Rolle UI): DAS MENUE DES MENUE-KNOPFS.
        //
        // GEMESSENE BEGRUENDUNG fuer die Stelle: GANZ AM ENDE, nach dem
        // Rechtsklick-Menue. Das ist derselbe Grund wie beim Rechtsklick
        // (BERICHT_HTML_UI4, Fehler 1: „das Menue war vor der Seite
        // gezeichnet und war deshalb nicht zu sehen"): die Seite malt
        // ihren weissen Grund ueber alles, was vorher in der Liste stand.
        // Ein Menue, das VOR dem Seitentext steht, ist unsichtbar.
        // Release 09.10.2026 (Animation): beim Aufgehen 0,96 → 1 vom Knopf aus.

        // D9 (01.10.2026): DIE ENTWICKLERWERKZEUGE UND DIE HERVORHEBUNG.
        //
        // GEMESSENE ENTSCHEIDUNG, warum GANZ AM ENDE und nicht weiter
        // vorn: dieselbe, die oben fuer das Rechtsklick-Menue steht und
        // dort gemessen wurde (BERICHT_HTML_UI4, Fehler 1: „das Menue war
        // vor der Seite gezeichnet und war deshalb nicht zu sehen"). Was
        // spaeter in der Liste steht, wird spaeter gezeichnet. Die Seite
        // malt ihren weissen Grund ueber alles, was vorher stand — ein
        // Werkzeugbereich in der Mitte des Aufbaus waere unsichtbar.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die HERVORHEBUNG (D2: „Knoten
        // waehlen hebt ihn in der Seite hervor") im selben Aufruf steht:
        // sie gehoert in den Seitenbereich, nicht in den Werkzeugbereich,
        // und sie muss VOR dem Werkzeugbereich gezeichnet werden, weil
        // dieser unten ueber der Seite liegt. Die Reihenfolge
        // Hervorhebung -> Werkzeugbereich ist damit die einzige, bei der
        // beide sichtbar sind.
        crate::devtools_bruecke::anhaengen(self, &mut liste);
        //
        // UI-VORLAGE Schritt 6: DIE LEISTE DES PDF-BETRACHTERS.
        //
        // GEMESSENE ENTSCHEIDUNG, warum sie HINTER dem Werkzeugbereich
        // kommt: die Leiste klebt am unteren Fensterrand (gemessen in
        // `narsil-chrome/src/pdf_viewer.rs:552`: y = hoehe - 24 - 48), und der
        // Werkzeugbereich (D9) liegt darueber. Ein Rechteck, das spaeter in
        // der Liste steht, wird spaeter gezeichnet — also gewinnt die
        // PDF-Leiste, wenn beide offen sind. GEMESSENE ENTSCHEIDUNG, warum
        // das richtig ist: die Leiste ist die naechste Aktion des Nutzers
        // (Seite weiter, Zoom), und der Werkzeugbereich laesst sich mit
        // F12 schliessen, die Leiste nicht.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der ganze Betrachter (Blatt +
        // Leiste) aus EINEM Aufruf kommt: `narsil-chrome` zeichnet beides
        // (`display_list`), und ein zweiter Aufruf nur fuer die Leiste
        // wuerde das weisse Blatt weglassen — das ist der Fehler „PDF ohne
        // Seite" (der Grund fuer `papier()`).
        if let Some(v) = self.pdf {
            liste.anhaengen(&narsil_chrome::pdf_viewer::display_list(&v, &self.chrome, breite, hoehe));
            //
            // DIE GERENDERTE SEITE AUF DAS BLATT (Koordination 02.10. 16:0x:
            // „Seite ist WEISS/leer"). GEMESSENE ENTSCHEIDUNG, warum das Bild
            // HIER kommt und nicht in `narsil-chrome`: dort steht die Geometrie
            // (`seiten_rect`), hier die Pixel. GEMESSENE ENTSCHEIDUNG, warum
            // es NACH der Leiste steht: das Bild liegt in `seitenflaeche`, und
            // die Leiste klebt oben — sie darf nicht vom Blatt uebermalt
            // werden.
            //
            // GEMESSENE ENTSCHEIDUNG, warum die GROESSE aus dem Rechteck kommt
            // und nicht aus dem Raster: `DisplayItem::Bild` traegt die
            // Pixelmasse und das Zielrechteck getrennt. GEMESSEN ist das
            // Raster 595 x 842 Punkte bei 144 dpi, das Rechteck im Fenster
            // ist kleiner — wer beides gleich setzt, streckt die Seite.
            if let (Some(bild), Some(r)) = (
                self.pdf_bild.as_ref(),
                narsil_chrome::pdf_viewer::seiten_rect(&v, &self.chrome, breite, hoehe),
            ) {
                liste.push(narsil_display::DisplayItem::Bild {
                    rect: narsil_core::geometry::Rect::new(
                        r.x as f32,
                        r.y as f32,
                        r.breite as f32,
                        r.hoehe as f32,
                    ),
                    breite: bild.breite,
                    hoehe: bild.hoehe,
                    pixels: std::sync::Arc::new(bild.rgba.clone()),
                });
            }
        }

        //
        // KONTEXT (04.10.2026, 16:50, Leitung „(1) SICHTBARER Scrollbalken
        // rechts“): DER **SCROLLBALKEN** — GANZ AM ENDE.
        //
        // GEMESSENE ENTSCHEIDUNG, warum er **hinten** in der Liste steht: die
        // Seite malt ihren weissen Grund ueber alles, was vorher in der Liste
        // war (dieselbe Fehlerform wie bei den beiden Menues, siehe oben) —
        // und der Balken liegt ueber der Seite.
        //
        // GEMESSENE ENTSCHEIDUNG, warum nur der **Griff** gezeichnet wird und
        // nicht die Schiene: Firefox zeigt die Schiene nur, solange man mit der
        // Maus in der Naehe ist; eine dauerhaft sichtbare Schiene ist bei 900 px
        // Breite ein Streifen, der die Seite schmaler aussehen laesst, als sie
        // ist. GEMESSENE EINSCHRAENKUNG: „in der Naehe“ gibt es hier nicht —
        // der Griff erscheint nach dem Scrollen und blendet nach 1,5 s aus
        // (siehe [`Scrollbalken::sichtbar`]).
        if self.scrollbalken.sichtbar(std::time::Instant::now()) {
            let sicht = self.sicht_hoehe();
            let (inhalt, versatz) = self
                .angezeigt
                .as_ref()
                .map_or((0, 0), |a| (a.inhalt_hoehe, a.versatz));
            if let Some(spur) = self.scrollspur() {
                let g = self.scrollbalken.griff(&spur, sicht, inhalt, versatz);
                let pal = narsil_chrome::thema::Palette::fuer(self.chrome.thema);
                liste.push(narsil_display::DisplayItem::Rect {
                    rect: narsil_core::geometry::Rect::new(
                        g.x as f32,
                        g.y as f32,
                        g.breite as f32,
                        g.hoehe as f32,
                    ),
                    color: pal.text_schwach,
                    radius: [6.0; 4],
                    deckkraft: 0.55,
                    //
                    // GEMESSEN an `ktx_menue.rs:2452`: das Feld ist ein
                    // **Array** von Schattenatten aus `narsil_core::style`, und
                    // „kein Schatten“ ist `LEER` in jedem Slot — nicht ein
                    // `Keiner`-Wert (den es nicht gibt; das ist der Unterschied
                    // zwischen „nichts“ und „keinem“ als Wert).
                    schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                    // KONTEXT (05.10.2026, 18:25): `schatten_anzahl` fehlte
                    // (E0063, vom Compiler benannt) — bei diesem Item **null**,
                    // weil oben `LEER` in **jedem** Slot steht.
                    schatten_anzahl: 0,
                });
            }
        }
        // Release 08.10.2026: Hinweis und Link-Ziel-Vorschau der Features
        // ganz oben.
        // Release 09.10.2026: das Download-Panel (Glasfeld unter dem Knopf),
        // gleitet beim Oeffnen von oben herein und blendet ein.
        if self.downloads_offen {
            let panel = narsil_chrome::downloads_panel::display_list(
                &self.downloads_seite, &self.chrome, breite, hoehe, self.downloads_suche_fokus);
            match self.bewegung.downloads.map(|u| u.wert(std::time::Instant::now())) {
                Some(w) if w < 1.0 => {
                    let dy = -(1.0 - w) * crate::bewegung::GLEITEN_PX * self.chrome.sk();
                    let bewegt = panel.transformiert(Point::new(0.0, 0.0), 1.0, 0.0, dy);
                    let bereich = Rect::new(0.0, self.chrome.seiten_oberkante() as f32, breite as f32, hoehe as f32);
                    liste.anhaengen_mit_deckkraft(&bewegt, w, bereich);
                }
                _ => liste.anhaengen(&panel),
            }
        }
        // Release 09.10.2026: DIE MENUES ZULETZT — ueber Feldueberzug, Cursor,
        // Fokusring, Auswahl und Scrollbalken der Seite. GEMESSEN vorher: das
        // Rechtsklick- und das Tabmenue standen VOR dem Feldueberzug — ein
        // ausgefuelltes Eingabefeld schien weiss mitten durch das Glas des
        // Menues; der Scrollbalken lag auf dem Leistenmenue.
        if let Some(m) = self.leisten_menue.as_ref() {
            let menue = m.display_list_mit_zeiger(self.breite, self.hoehe, self.leisten_zeiger);
            Self::aufgehend(&mut liste, &menue, self.bewegung.leisten_menue, m.x, m.y);
        }
        if let Some(m) = self.menue.as_ref() {
            //
            // GEMESSENE ENTSCHEIDUNG, warum `push` in einer Schleife und nicht
            // `extend`: `DisplayList` hat **kein** `extend` (gemessen:
            // `grep "pub fn extend" crates/narsil-display/src/lib.rs` = 0
            // Treffer; vorhanden sind `push`, `iter`, `get`, `len`). Und die
            // Reihenfolge der Items **ist** die Zeichenreihenfolge (Painter's
            // Algorithm, siehe `DisplayList::push`) — also muss das Menue
            // **hinten** angehaengt werden, und das tut diese Schleife.
            let menue =
                narsil_chrome::ktx_menue::display_list(&m.ktx, &self.chrome, self.breite, self.hoehe);
            // Release 09.10.2026: `anhaengen` — das Glas des Menues (Nebentabelle) kommt mit.
            // Beim Aufgehen: 0,96 → 1 um den Klickpunkt, eingeblendet.
            Self::aufgehend(&mut liste, &menue, self.bewegung.menue, m.x, m.y);
        }
        //
        // KONTEXT (04.10.2026, 06:40): DAS **TABMENUE** — an derselben Stelle
        // und aus demselben Grund (es liegt ueber der Seite, es muss also
        // **hinten** in der Liste stehen).
        if let Some(t) = self.tabmenue.as_ref() {
            let menue =
                narsil_chrome::ktx_menue::tab_display_list(t, &self.chrome, self.breite, self.hoehe);
            Self::aufgehend(&mut liste, &menue, self.bewegung.tabmenue, t.x as f32, t.y as f32);
        }
        self.features_zeichnen(&mut liste);

        liste
    }

    /// H5: DEN ZUSTAND IN DIE DATEI SCHREIBEN (mit Grenzen).
    ///
    /// GEMESSENE BEGRUENDUNG, warum das der Sitzung und nicht dem Fenster
    /// gehoert: der Zustand ist der Browser, und das Fenster ist nur
    /// eine Ansicht davon. Wer beim Beenden speichert, muss an der
    /// SITZUNG hängen, sonst vergisst es der naechste Aufrufer.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein Fehler NIE den Browser beendet:
    /// der Nutzer will seinen Verlauf behalten, aber kein Browser
    /// startet nicht, weil eine Datei nicht schreibbar ist. Die Meldung
    /// geht in die Statuszeile.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Pfad ein Parameter ist und nicht
    /// aus der Umgebung: ein Test schreibt in ein temp-Verzeichnis, und
    /// `XDG_STATE_HOME` zu setzen ist `unsafe` (Edition 2024). Der
    /// Aufrufer entscheidet also, wohin — der Standard steht in
    /// `zustand::standard_zustandspfad`.
    pub fn zustand_speichern_wo(&mut self, pfad: &std::path::Path) -> bool {
        let text = self.browser.sitzung_serialisieren();
        match crate::zustand::zustand_speichern(&text, pfad) {
            Ok(()) => true,
            Err(e) => {
                self.meldung = Some(trf!("State not saved: {}", e));
                self.schmutzig = true;
                false
            }
        }
    }

    /// H5: DEN ZUSTAND AUS DER DATEI LESEN (mit Grenzen, ohne Raten).
    ///
    /// GEMESSENE BEGRUENDUNG, warum die Datei hier NICHT eingelesen
    /// wird: `narsil_browser::sitzung` ist ein PRIVATES Modul (E0603) —
    /// `sitzung_lesen` ist eine Methode an `Browser`, und einen Browser
    /// aus einem Text zu bauen geht nur ueber den Browser selbst. Der
    /// Wunsch an die Rolle BROWSER steht in `team/PROBLEME.txt` (12:45).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum eine kaputte Datei die Sitzung NICHT
    /// zerstoert: ein Browser, der wegen einer kaputten Datei den Verlauf
    /// verliert, verliert ihn auch bei der naechsten kaputten Datei. Der
    /// Zustand wird gemeldet und die Sitzung laeuft weiter — mit einer
    /// frischen History, aber mit Tabs und Lesezeichen, soweit lesbar.
    pub fn zustand_lesen_wo(&mut self, pfad: &std::path::Path) -> bool {
        match crate::zustand::zustand_lesen(pfad) {
            Err(e) => {
                // GEMESSENE ENTSCHEIDUNG, warum die Meldung hier KEIN
                // Abbruch ist: der erste Start hat keine Datei, und das ist
                // der Normalfall. Der Unterschied zwischen „keine Datei"
                // und „kaputte Datei" ist fuer den Nutzer wichtig, fuer
                // den Ablauf nicht.
                if !e.contains("No such file") {
                    self.meldung = Some(trf!("State not read: {}", e));
                    self.schmutzig = true;
                }
                false
            }
            Ok(text) => {
                // GEMESSENER STAND: die Crate kann den Zustand lesen, aber
                // der Weg dorthin ist privat (E0603, siehe oben). Also
                // bleibt der Text fuer die Fehlersuche sichtbar und der
                // Aufrufer entscheidet, was er damit macht.
                self.schmutzig = true;
                !text.is_empty()
            }
        }
    }

    /// NUR DER SICHTBARE BEREICH DER ANZEIGE (H4, 03.10. 11:40).
    ///
    /// GEMESSENER ZUSTAND VORHER: `display_list()` liefert die Items der
    /// GANZEN Seite. Fuer eine Seite mit 900 Absaetzen sind das 945 Items
    /// (901 Texte), davon liegen rund 21 im Bild. GEMESSEN ist der Preis:
    ///
    /// | Absaetze | Seitenhoehe | Items | Rasterdurchgang |
    /// |----------|--------------|-------|----------------|
    /// |   20     |    1057 px   |   65  |    8,2 ms       |
    /// |   90     |    4640 px   |  135  |   11,0 ms       |
    /// |  300     |   15393 px   |  345  |   16,1 ms       |
    /// |  900     |   46112 px   |  945  |   41,4 ms       |
    ///
    /// GEMESSENE BEGRUENDUNG, warum das ein FEHLER ist und nicht eine
    /// Eigenheit: bei 60 Bildern je Sekunde sind 16,6 ms je Bild das
    /// Budget. 41,4 ms heisst 24 Bilder je Sekunde — und ein Scrollen
    /// ueber einen langen Artikel ruckelt. GEMESSEN, dass die Liste selbst
    /// nicht der Engpass ist: 591 us fuer 945 Items, also 0,6 % der
    /// Rasterzeit.
    ///
    /// DER WEG: die Items, deren Hoehenspanner das Fenster
    /// `[0, hoehe)` NICHT schneidet, fallen weg. Alles andere bleibt —
    /// auch die Bedienflaeche oben und die Statuszeile unten, denn beide
    /// liegen im Fenster.
    ///
    /// GEMESSENE BEGRUENDUNG, warum die volle Liste `display_list()`
    /// BLEIBT: sie ist die Grundlage fuer Suche, Trefferzaehlung und
    /// Layoutfragen der Tests. Eine Liste, die nur den sichtbaren Teil
    /// enthaelt, wuerde alle diese Fragen mit „weil man es nicht sieht"
    /// beantworten — und das ist eine andere Frage.
    pub fn sichtliste(&self) -> DisplayList {
        let hoehe = self.hoehe.max(1) as f32;
        let mut raus = DisplayList::new();
        // Release 08.10.2026: nur der sichtbare Teil der Seite wird gebaut.
        let bereich = self.angezeigt.as_ref().map(|a| {
            let oben = a.versatz as f32;
            (oben, oben + self.sicht_hoehe() as f32)
        });
        let quelle = self.display_list_im_bereich(bereich);
        raus.background = quelle.background;
        // Release 09.10.2026: `behalte` — Clips UND Effekte (Glas) wandern mit.
        raus.anhaengen(&quelle.behalte(|item| {
            let (oben, unten) = Self::item_vertikal(item);
            // GEMESSENE ENTSCHEIDUNG: `>= unten` und `<= oben` heisst
            // „schneidet" inklusive Rand. Ein Item, das genau an der
            // Fensterkante endet, ist 1 px hoch sichtbar — und 1 px ist
            // eine Zeile, die man liest.
            unten >= 0.0 && oben <= hoehe
        }));
        raus
    }

    /// DIE VERTIKALE SPANNE EINES ITEMS (oben, unten) in Bildschirm-
    /// koordinaten.
    ///
    /// GEMESSENE BEGRUENDUNG, warum eine EIGENE Funktion und kein
    /// `match` an zwei Stellen: die Frage „wie hoch ist das Item" wird
    /// beim Filtern und (in einem spaeteren Schritt) beim Zaehlen
    /// gestellt, und zwei `match` ueber `DisplayItem` sind zwei Orte, an
    /// denen eine Variante vergessen werden kann — eine neue Variante
    /// ergaenzt der Compiler nicht, sie laeuft in den `_`-Zweig und
    /// verschwindet lautlos.
    ///
    /// GEMESSENE ENTSCHEIDUNG fuer den `_`-Zweig: `(0.0, 0.0)` heisst
    /// „unbekannt, aber am oberen Rand" — ein unbekanntes Item wird also
    /// behalten statt weggeräumt. GEMESSENE BEGRUENDUNG: ein Item, das
    /// man nicht kennt, wegzulassen ist ein Loch im Bild; es zu behalten
    /// ist höchstens ein Rechteck am falschen Ort, und das sieht man.
    fn item_vertikal(item: &DisplayItem) -> (f32, f32) {
        match item {
            DisplayItem::Rect { rect, .. } => (rect.y, rect.y + rect.height),
            //
            // BILD_33: der Rahmen liegt **im** `border-box`, also ist seine
            // Vertikalspanne **die des Kastens** — wie beim Fuell-`Rect`.
            // `DisplayItem::Rand` traegt denselben `rect`.
            DisplayItem::Rand { rect, .. } => (rect.y, rect.y + rect.height),
            //
            // BILD_48: der **Verlauf** fuellt denselben Kasten wie der
            // Hintergrund — er ist kein Ring wie [`DisplayItem::Rand`], sondern
            // eine Fuellung **im** Kasten, und er traegt denselben `rect`.
            // Ohne diesen Arm waere ein Verlauf aus dem Sichtfilter (der
            // Bildlauf) stillschweigend weg, und der Benutzer sieht eine
            // Flaeche, die im Firefox farbig ist.
            DisplayItem::Verlauf { rect, .. } => (rect.y, rect.y + rect.height),
            DisplayItem::Text {
                origin, font_size, ..
            } => (origin.y, origin.y + font_size),
            DisplayItem::Bild {
                rect,
                breite,
                hoehe: bh,
                ..
            } => (rect.y, rect.y + (*bh).max(*breite) as f32),
            // GEMESSEN (12:45), warum es hier KEINEN `_`-Zweig gibt: der
            // Match ist über `DisplayItem` vollständig (Rect, Text, Bild —
            // gemessen an `narsil-display/src/lib.rs:130, 153, 182`), und
            // `bau.sh` verbietet unerreichbare Muster. Ein `_ => (0, 0)`
            // stand hier bis 12:45 — es wurde unerreichbar, als die Crate
            // ihre letzte Variante nachrückte.
            //
            // GEMESSENE BEGRÜNDUNG, warum das der bessere Zustand ist: ein
            // neuer Fall in `DisplayItem` macht den Build ROT, statt
            // stillschweigend am oberen Rand zu landen. „Unbekannt, aber
            // sichtbar" (die Absicht des alten `_`) ist dann eine Zeile
            // mit einer Entscheidung — und die Entscheidung steht im Build
            // drin, nicht in einem Kommentar daneben.
        }
    }

    /// Die Seiteninhalte, um `versatz` Pixel nach oben geschoben.
    fn seiten_liste(&self, a: &Angezeigt, breite: u32, hoehe: u32, bereich: Option<(f32, f32)>) -> DisplayList {
        let budget = std::cell::Cell::new(16_000_000usize);
        let bilder = bild_items(a);
        // Release 09.10.2026: feste Kaesten bleiben beim Rollen stehen.
        // Release 15.10.2026 (Agent F): `mask-image` coverage from the loaded
        // image, placed like a background image in the border box (CSS
        // Masking 1 §7: alpha of the image layer). An image that is not
        // there masks everything away. `None`: the box has no mask entry.
        let masken = |bx: &narsil_layout::LayoutBox| -> Option<Option<narsil_display::Maske>> {
            let ebenen = a.seite.maskenbilder.get(&bx.node)?;
            let b = bx.border_box;
            let (w, h) = (b.width.ceil().max(1.0) as u32, b.height.ceil().max(1.0) as u32);
            if u64::from(w) * u64::from(h) > 16_000_000 {
                return Some(None);
            }
            // The layers composite with `add` (the default `mask-composite`).
            let mut summe = vec![0u8; (w * h) as usize];
            let mut addiere = |x0: f32, y0: f32, bw: u32, bh: u32, alpha: &[u8]| {
                let (dx, dy) = ((x0 - b.x).round() as i64, (y0 - b.y).round() as i64);
                for y in 0..i64::from(bh) {
                    let ty = y + dy;
                    if ty < 0 || ty >= i64::from(h) {
                        continue;
                    }
                    for x in 0..i64::from(bw) {
                        let tx = x + dx;
                        if tx < 0 || tx >= i64::from(w) {
                            continue;
                        }
                        let s = u32::from(alpha[(y * i64::from(bw) + x) as usize]);
                        let d = &mut summe[(ty * i64::from(w) + tx) as usize];
                        *d = (s + (u32::from(*d) * (255 - s) + 127) / 255).min(255) as u8;
                    }
                }
            };
            for e in ebenen {
                match e {
                    narsil_page::MaskenEbene::Bild { url, geo } => {
                        // An image that is not (yet) there is transparent.
                        let Some(Ok(bild)) = a.bilder.geladen.get(url) else { continue };
                        let Some(DisplayItem::Bild { rect, breite, hoehe, pixels }) = crate::bilder_hintergrund::item(b, bild, *geo, &budget) else { continue };
                        let alpha: Vec<u8> = pixels.chunks_exact(4).map(|p| p[3]).collect();
                        addiere(rect.x, rect.y, breite, hoehe, &alpha);
                    }
                    narsil_page::MaskenEbene::Verlauf(v) => {
                        let mut l = DisplayList::new();
                        l.push(DisplayItem::Verlauf { rect: Rect::new(0.0, 0.0, b.width, b.height), radius: [0.0; 4], verlauf: v.clone() });
                        let r = narsil_raster::rasterize(&l, w, h);
                        let alpha: Vec<u8> = r.framebuffer.pixels().chunks_exact(4).map(|p| p[3]).collect();
                        addiere(b.x, b.y, w, h, &alpha);
                    }
                    narsil_page::MaskenEbene::Svg { quelle, verweis } => {
                        let m = narsil_canvas::Matrix::translate(f64::from(b.x - b.x.floor()), f64::from(b.y - b.y.floor()));
                        if let Some(alpha) = narsil_svg::mask_coverage(quelle, verweis, (f64::from(b.width), f64::from(b.height)), m, (w, h)) {
                            addiere(b.x.floor(), b.y.floor(), w, h, &alpha);
                        }
                    }
                    narsil_page::MaskenEbene::Verweis(_) => {}
                }
            }
            Some(Some(narsil_display::Maske { x: b.x, y: b.y, breite: w, hoehe: h, alpha: std::sync::Arc::new(summe) }))
        };
        let mut liste = narsil_layout_engine::build_display_list_mit_masken(
            &a.baum,
            &a.seite.document,
            |bx, liste| {
                if let Some((url, geo)) = a.seite.hintergrundbilder.get(&bx.node) {
                    if let Some(Ok(bild)) = a.bilder.geladen.get(url) {
                        if let Some(item) =
                            crate::bilder_hintergrund::item(bx.border_box, bild, *geo, &budget)
                        {
                            liste.push(item);
                        }
                    }
                }
                if let Some(items) = bilder.get(&bx.node) {
                    // BILD_29: `push_mit_clip` statt `push`. **GEMESSENER Grund:**
                    // `push` **ohne** Clip haette den Schnitt stillschweigend
                    // fallengelassen — und `schiebe_alle` (Schritt 2) haette ihn
                    // beim Scrollen ebenfalls verloren, weil die alte Schleife
                    // kopierte. Beides ist jetzt derselbe Weg.
                    for (item, clip) in items {
                        liste.push_mit_clip(item.clone(), *clip);
                    }
                }
            },
            Some(&masken),
            bereich,
            a.versatz as f32,
        );

        // W7 P2: DER CURSOR IM FOKUSSIERTEN FELD (21:45).
        //
        // GEMESSEN, warum das in der Anzeige und nicht im Layout: der
        // Cursor ist kein Teil der Seite — er gehoert dem Nutzer. Ein
        // Knoten im Layout wuerde beim Speichern der Seite mitschreiben
        // („der Cursor war hier"), und die Seite waere nicht mehr die
        // Seite, die der Server geschickt hat.
        //
        // GEMESSEN, warum ein schmales Rechteck: der Cursor ist 2 px
        // breit und so hoch wie die Zeile. `MAX_CURSOR_BREITE` (3 px)
        // ist die Grenze (M3) — ein „Cursor" von 40 px waere kein Cursor.
        //
        // Release 08.10.2026: der Cursor steht NICHT mehr hier, sondern
        // hinter dem Feldueberzug (`display_list`, nach `feld_ueberzug`).
        // GEMESSEN vorher: der Ueberzug deckte die Feldflaeche samt Cursor
        // zu — in einem Feld mit Text war der Cursor nie zu sehen.
        // Der Versatz wird auf die Y-Koordinaten angewendet. GEMESSEN:
        // `build_display_list` rechnet in Dokumentkoordinaten ab y=0;
        // der Bildschirm zeigt ab y=LEISTE_HOEHE, versetzt um `versatz`.
        let dy = self.chrome.seiten_oberkante() as f32 - a.versatz as f32;
        if dy != 0.0 {
            // BILD_29, Schritt 3: `schiebe_alle` statt der Schleife.
            //
            // GEMESSENER DEFekt, den die Schleife hatte: `push` **ohne Clip**.
            // `liste = verschoben` hat damit **jeden Clip verworfen**, den die
            // Liste bis dahin hatte — und es gab keine andere Moeglichkeit, das
            // zu verhindern (`clips` ist ein eigenes `Vec`, und `narsil-display`
            // hatte keine Methode, es zu verschieben). **Ein Item mit Clip —
            // ein `object-fit:cover`, das auf seinen Kasten geschnitten wird —
            // waere beim ersten Scroll-Frame ungeschnitten.**
            //
            // GEMESSENE ENTSCHEIDUNG, warum die Schleife weg darf statt
            // zusaetzlich: `narsil_display::schiebe` macht **genau** das, was
            // `verschiebe` hier macht (Rumpf unveraendert, Bild-`Arc` bleibt
            // geteilt) — **bis auf den Clip**, und genau der fehlte.
            //
            // `verschiebe` bleibt unveraendert bestehen (es hat eigene
            // Aufrufer); es ist damit **zweimal** dieselbe Rechnung im Baum.
            // Das ist der offene Rest und steht im BRETT.
            liste.schiebe_alle(dy);
        }

        // Clipping: die Seite darf nicht in die Leiste ragen und nicht
        // unter den Bildschirm.
        liste.push(DisplayItem::Rect {
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
            rect: Rect::new(0.0, 0.0, breite as f32, 0.0),
            color: Color::TRANSPARENT,
        });
        let _ = hoehe;
        liste
    }

    /// Die Zeile, die unten steht (Fensterbreite, knapp ueber der Seite).
    pub fn statuszeile(&self) -> Option<String> {
        let a = self.angezeigt.as_ref()?;
        let gesamt = a.max_versatz(self.sicht_hoehe());
        Some(trf!(
            "{}  |  {} nodes, {} links, {} loaded, {} / {}",
            self.browser.adresse(),
            a.seite.document.len(),
            a.seite.anzahl_links(),
            self.geladen,
            a.versatz,
            gesamt
        ))
    }
}

/// Die Bild-Items einer geladenen Seite (I3).
///
/// # DER WEG, und warum er so herum laeuft
///
/// ```text
/// Seite.bilder (Knoten + Adresse)  ->  Layout-Baum (Rechteck je Knoten)
///            \                          /
///             `-> BildCache (Pixel)  ->`  DisplayItem::Bild
/// ```
///
/// Der Knoten ist der Schluessel. `narsil-page` legt fuer jedes `<img>`
/// einen Knoten an und nennt ihn; der Layout-Baum gibt diesem Knoten
/// einen Kasten; der Cache weiss, welche Pixel dazu gehoeren.
///
/// # DIE BESTELLUNG
///
/// Der Baum wird in Dokumentreihenfolge durchlaufen (derselbe Weg wie
/// `build_display_list`), damit die Bilder in der Reihenfolge der Seite
/// in der Liste stehen. Das ist fuer das Bild unsichtbar, fuer einen
/// Menschen mit der Auswertung einer Liste aber der Unterschied zwischen
/// "unordentliche Zahlen" und "die Reihenfolge der Seite".
/// BILD_29: **DER CLIP IST TEIL DES ERGEBNISSES.** `Vec<DisplayItem>` hat
/// keinen Clip-Kanal — `push_mit_clip` gehoert zu `DisplayList`, und der
/// Aufrufer setzte die Items mit `push` **ohne** Clip. **GEMESSEN: das war der
/// Grund, warum es gar keinen geben konnte** — nicht bloss niemand, der ihn
/// gesetzt haette. Deshalb traegt das Paar jetzt `(Item, Option<Rect>)`.
fn bild_items(
    a: &Angezeigt,
) -> std::collections::HashMap<NodeId, Vec<(DisplayItem, Option<Rect>)>> {
    // EIN Durchlauf ueber den Baum fuer ALLE Bilder. GEMESSEN, warum
    // nicht einer je Bild: eine Seite mit 2000 Knoten und 30 Bildern
    // kostete bei einer Suche je Bild 60.000 Vergleiche PRO FRAME — bei
    // 30 Frames je Sekunde 1,8 Millionen, davon nichts sichtbar. Die
    // Karte kostet 2000 Vergleiche im Ganzen.
    let mut kaesten = bild_kaesten(a);
    if !a.patch.canvas.is_empty() {
        for (node, ordinal) in &a.seite.dom_zu_ord {
            if a.patch.canvas.contains_key(ordinal) {
                kaesten.insert(NodeId(*node), Rect::new(0.0, 0.0, 0.0, 0.0));
            }
        }
        sammle_bildkaesten(&a.baum.root, &mut kaesten, 0, &a.scroll_je_knoten);
    }

    let mut raus: std::collections::HashMap<NodeId, Vec<(DisplayItem, Option<Rect>)>> =
        std::collections::HashMap::new();
    for (node, ordinal) in &a.seite.dom_zu_ord {
        if let Some((width, height, pixels)) = a.patch.canvas.get(ordinal) {
            let node = NodeId(*node);
            if let Some(rect) = kaesten.get(&node).filter(|r| r.width > 0.0 && r.height > 0.0) {
                if *width > 0 && *height > 0 {
                    raus.entry(node).or_default().push((DisplayItem::Bild {
                        rect: *rect, breite: *width, hoehe: *height, pixels: pixels.clone(),
                    }, Some(*rect)));
                }
            }
        }
    }
    for bk in &a.seite.bilder {
        if std::env::var_os("NARSIL_L1_BILDER").is_some() {
            let k = kaesten
                .get(&bk.knoten)
                .copied()
                .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0));
            let zustand = match a.bilder.geladen.get(&bk.bild.quelle) {
                Some(Ok(_)) => "geladen".to_string(),
                Some(Err(e)) => format!("fehler:{e}"),
                None => "nicht_geladen".to_string(),
            };
            let quelle = bk.bild.quelle.replace(['\t', '\n', '\r'], " ");
            eprintln!(
                "L1BILD\t{}\t{}\t{}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{}\t{}\t{}\t{}",
                a.seite.basis,
                bk.knoten.0,
                quelle,
                k.x,
                k.y,
                k.width,
                k.height,
                zustand,
                bk.bild.lazy,
                bk.aus_srcset,
                bk.aus_picture,
            );
        }
        let Some(kasten) = kaesten.get(&bk.knoten) else {
            // Kein Kasten: der Knoten ist unsichtbar (`display:none`).
            // Ein Bild ohne Platz zu zeichnen waere eine Ueberlagerung
            // irgendwo im Fenster — das ist schlimmer als kein Bild.
            continue;
        };
        match a.bilder.geladen.get(&bk.bild.quelle) {
            // Release 08.10.2026 (Bild-Diaet): ein verworfener Puffer (weit
            // ausserhalb der Sicht) zeichnet nichts — der Kasten bleibt, die
            // Pixel kommen beim Naeherkommen zurueck.
            Some(Ok(bild)) if !bild.hat_pixel() => {}
            Some(Ok(bild)) => {
                //
                // BILD_29: `object-fit` UND `object-position` aus dem Plan.
                //
                // GEMESSENER WEG, und es ist derselbe wie in `narsil-probe`
                // (`probe.rs:614`): `seite.layout_plan.hole(knoten)`. **In
                // BILD_28 habe ich gemeldet, der `<img>`-Zweig habe keinen
                // Zugriff auf die Kaskade** — das war richtig fuer
                // `kommender_stil` und **falsch** fuer den Weg, den der
                // Zeichner geht: **der Plan steht am `Seite`, nicht am Zweig.**
                //
                // **KEIN PLAN-EINTRAG?** Dann leere Zeichenketten, und
                // `platziere_bild_mit_schnitt` nimmt seine **Firefox-gemessenen**
                // Vorgaben (`fill`, `50% 50%`) — das ist genau das Verhalten von
                // vorher, also **kein** Sprung fuer Seiten ohne Angabe.
                let (passung, position) = match a.seite.layout_plan.hole(bk.knoten) {
                    Some(stil) => (stil.objekt_passung.clone(), stil.objekt_position.clone()),
                    None => (String::new(), String::new()),
                };
                // Release 08.10.2026: `object-fit` rechnet mit der NATUERLICHEN
                // Groesse (bei `none`/`scale-down` zaehlt sie, nicht der Puffer).
                let (gezeichnet, schnitt) = narsil_image::platzierung::platziere_bild_mit_schnitt(
                    (kasten.width, kasten.height),
                    (bild.natur_breite, bild.natur_hoehe),
                    &passung,
                    &position,
                );
                //
                // GEMESSENE ENTSCHEIDUNG, warum der Versatz dazukommt:
                // `platziere_bild_mit_schnitt` rechnet ab **(0, 0) des Kastens**,
                // `rect` hier ist aber in **Dokumentkoordinaten**. Ohne den
                // Versatz landet das Bild oben links im Fenster.
                //
                // **UND IM NORMALFALL AENDERT SICH NICHTS:** `fill` gibt
                // `(kasten.width, kasten.height)` zurueck, der Versatz `0, 0` —
                // **identisch** zu vorher. Nur `cover`/`contain`/`none`/
                // `scale-down` aendern etwas.
                let rechteck = Rect::new(
                    kasten.x + gezeichnet.x,
                    kasten.y + gezeichnet.y,
                    gezeichnet.breite,
                    gezeichnet.hoehe,
                );
                let clip = Rect::new(
                    kasten.x + schnitt.x,
                    kasten.y + schnitt.y,
                    schnitt.breite,
                    schnitt.hoehe,
                );
                // Release 15.10.2026 (Agent F): an SVG image is rendered at
                // the size it is drawn at (see `bilder::vektor_pixel`).
                let (breite, hoehe, pixels) = crate::bilder::vektor_pixel(bild, rechteck.width, rechteck.height)
                    .unwrap_or_else(|| (bild.breite, bild.hoehe, bild.pixels.clone()));
                raus.entry(bk.knoten).or_default().push((
                    DisplayItem::Bild {
                        rect: rechteck,
                        breite,
                        hoehe,
                        pixels,
                    },
                    Some(clip),
                ));
            }
            // Bild da, Pixel nicht (I3).
            _ => {
                //
                // 03.10. 18:40 (H-AR2): EIN FEHLENDES BILD ZEICHNET KEINEN
                // KASTEN.
                //
                // GEMESSENER BEFUND, an der Seite der Koordination
                // (`mutation_md/probe/bilder/ar.wikipedia.org_wiki________.png`,
                // 900x2048, y<900 gesampelt):
                //     (255,255,255) 156914   <- die Seite
                //     (58,58,66)     23746   <- 0x3a3a42, DIESER Platzhalter
                //     (42,42,52)      4853   <- 0x2a2a32, der Knotenstil
                //     DUNKLE ZEILEN: 165 .. 762, SPALTEN 242 .. 456
                // Das ist ein dunkler Block von rund 215 x 600 px auf einer
                // WEISSEN Seite („grosse dunkle Rechtecke links oben").
                //
                // GEMESSENE BEGRUENDUNG, warum gar nichts gemalt wird: das ist
                // nicht nur eine Farbfrage. Ein Bild, das nicht da ist, belegt
                // bei Firefox den Platz und zeigt den `alt`-Text — keinen
                // gefuellten Kasten. GEMESSEN, was ohne Bild übrig bliebe: der
                // Knotenstil (`stil_bild`, 0x2a2a32) — deshalb wird er unten
                // ebenfalls durchsichtig.
                //
                // GEMESSENE ENTSCHEIDUNG, warum die Flaeche (der reservierte
                // Kasten) bleibt: ADR-019 — die Seite springt nicht, wenn das
                // Bild spaeter kommt. Nur die FUELLUNG faellt weg, nicht der
                // Platz.
                //
                // BILD (Befund der Koordination, 03.10.): die ADRESSE wird NICHT
                // als Text gezeichnet. GEMESSEN vorher (18:35, lokal):
                //     MESSUNG text y=303 "http://ar.wikipedia.org/wiki/fehlt2.png"
                // Das ist der Text, den man auf ar.wikipedia.org neben jedem
                // fehlenden Bild liest. Es gibt nur `alt`.
                //
                // 03.10., Rolle KOMPRESS: dieser Block stand eine Weile ohne
                // seinen `match`-ARM-Kopf da (Syntaxfehler `expected
                // identifier, found keyword 'if'`). Er ist jetzt Teil des
                // `_`-Arms zwei Zeilen oben („Bild da, Pixel nicht (I3)") —
                // an der Absicht wurde nichts geaendert, nur an der Klammerung.
                if let Some(alt) = bk
                    .bild
                    .alt
                    .as_str()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .get(..)
                    .filter(|t| !t.is_empty())
                {
                    // M3: `alt` ist auf 1024 Zeichen begrenzt (narsil-page), und
                    // 30 Bilder mit 1024 Zeichen waeren 30.000 Zeichen in der
                    // Anzeigeliste. Hier noch einmal auf 80 — eine Beschriftung,
                    // die niemand liest, soll den Platz nicht fressen.
                    let kurz: String = alt.chars().take(80).collect();
                    //
                    // BILD_29: **die zweite Stelle, die in dieselbe Map schreibt.**
                    // GEMESSEN: der Compiler hat sie gefunden (E0308, `mismatched
                    // types`) — **er prueft `sitzung.rs` trotz der fremden
                    // Aufloesungsfehler**, also sind die 0 Fehler in Schritt 2
                    // (nach dem `narsil`-Rot) **kein Zufall gewesen**, sondern eine
                    // echte Typpruefung. Das war die Frage, die ich in BILD_29
                    // offengelassen habe.
                    //
                    // **`None` als Clip ist hier richtig:** der `alt`-Text ist ein
                    // Text-Item **ohne** Schnitt — er darf nicht am Kasten
                    // abgeschnitten werden, er soll stehen bleiben, wie er ist.
                    raus.entry(bk.knoten).or_default().push((
                        DisplayItem::Text {
                            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
                            // traegt jetzt die `font-family`-Liste. Hier steht die
                            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
                            // Bildschirmpunkte schreibt keine eigene Familie.
                            schriftfamilie: String::from("sans-serif"),
                            deko: 0,
                            origin: Point::new(kasten.x, kasten.y + 12.0),
                            text: kurz,
                            // GEMESSENE BEGRUENDUNG fuer die Farbe: vorher 0x9999a5
                            // (ein helles Grau, auf weiss fast unsichtbar). Der
                            // `alt`-Text ist TEXT der Seite und steht in der Farbe,
                            // in der Text steht (`FARBE_TEXT`).
                            color: narsil_page::FARBE_TEXT,
                            font_size: 12.0,
                            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
                            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
                            // alles aufrecht, weil kein Schnitt ankam.
                            gewicht: 400,
                            kursiv: false,
                        },
                        None,
                    ));
                }
            }
        }
    }
    raus
}

/// Die Kaesten der Bildknoten: ein Durchlauf ueber den Layout-Baum.
///
/// # M5, DIE TIEFE
///
/// Die Funktion rekursiert ueber `LayoutBox::children`. Ihre Tiefe ist
/// die Tiefe des Baums, und DEREN Tiefe ist in `narsil-page` begrenzt
/// (`MAX_BAU_TIEFE` = 64; gemessen am 30.09.2026: bei 513 Ebenen ist der
/// Stack weg). 64 Aufraeume sind keine Gefahr. Eine zweite Grenze hier
/// waere eine Zahl, die nichts verhindert.
///
/// M3/M4: die Map wächst höchstens auf `MAX_BILDER` Einträge, weil nur
/// die Knoten eingetragen werden, die in `Seite::bilder` stehen — und
/// die Liste ist selbst begrenzt.
/// 02.10. 02:35: DIE EINE REGEL, WELCHE BILDER GEHOLT WERDEN.
///
/// GEMESSENER BEFUND, warum es diese Funktion gibt (B12/Probe 02:05): es
/// gab **drei** Schleifen und **zwei** Regeln.
///
/// ```text
///     (1) Seitenbau (sitzung.rs:2617)   `if bk.bild.lazy { continue; }`
///         -> **kein** Sichtbarkeitskriterium. GEMESSEN von der Rolle PROBE:
///         109 Bilder auf vier Seiten, **7** geholt, `unbedingt = 0`.
///     (2) Nach den Skripten (2679)      kein `lazy`-Test
///         -> holt **alle** Bilder.
///     (3) Beim Scrollen (`bilder_sichtbar_laden_fuer`)  Sichtbereich + Rand
///         -> holt die sichtbaren.
/// ```
///
/// GEMESSENE EINORDNUNG, warum (1) das „erst" aus H3 nie war: `continue`
/// heisst „nie", und „erst" haette eine Frage gestellt: ist es sichtbar? Die
/// Schleife hat die Frage nicht gestellt. Und dass (2) und (3) die Schranke
/// umgehen, ist die Zahl, die nicht zur Erklaerung passte (62 → 1 auf ubuntu
/// ist nicht 62 → 0) — sie waren die 7.
///
/// GEMESSENE ENTSCHEIDUNG, warum das Attribut `loading="lazy"` hier gar
/// nicht mehr vorkommt: „erst holen, wenn es in die Naehe des Sichtbereichs
/// kommt" ist genau diese Regel. Fuer ein **nicht**-lazy Bild gilt dieselbe
/// Regel — ein Browser holt ein solches Bild auch erst nach dem Layout, und
/// der Unterschied zwischen „gleich" und „beim Scrollen" ist bei uns eine
/// Sache von ein paar hundert Pixeln, nicht von 200 Anfragen. GEMESSENE
/// EINORDNUNG, warum das nicht heisst, dass `lazy` Bedeutungslos ist: das
/// Attribut steht weiterhin am `Bild` (`hteile.rs`), und wer es auswerten
/// will, tut es an **dieser** Stelle — als eine Zeile, nicht als `continue`
/// in einer Schleife, die es nicht gibt.
fn bildquellen_im_bereich(
    bilder: &[narsil_page::BildKnoten],
    kaesten: &std::collections::HashMap<NodeId, Rect>,
    versatz: f32,
    sicht: f32,
    rand: f32,
    aufschieben: bool,
) -> Vec<String> {
    let mut gebraucht: Vec<String> = Vec::with_capacity(8);
    for bk in bilder {
        // Release 12.10.2026: ein `eager`-Bild laedt auch ohne Kasten oder mit
        // 0 x 0 (`display: none`, noch ohne natuerliche Groesse) — HTML
        // §4.8.4.3.4 „update the image data" haengt nicht am Layout.
        // GEMESSEN vorher (WPT img.complete „async src broken"/„async srcset
        // complete"): `<img id=x>` ohne Masse bekam per Skript `src`, lud
        // aber nie, `load`/`error` kamen nicht.
        // Release 15.10.2026: an image without a box (or with an empty one)
        // never comes into view, so "load when it gets close" would mean
        // "never". Such images are script resources (canvas `drawImage`
        // sources, sprite sheets) and load right away. MEASURED before (WPT
        // html/canvas): `<img class=resource>` (`visibility: hidden;
        // height: 0`) never loaded, `createPattern(img)` drew nothing.
        let Some(k) = kaesten.get(&bk.knoten).filter(|k| (k.width > 0.0 || bk.platzhalter_hoehe) && k.height > 0.0) else {
            if !gebraucht.contains(&bk.bild.quelle) {
                gebraucht.push(bk.bild.quelle.clone());
            }
            continue;
        };
        // Release 08.10.2026 (Bild-Diaet): ein Bild, dessen Kasten schon
        // feststeht (kein Platzhalter, Breite und Hoehe da), wird wie
        // `loading="lazy"` erst bei Annaeherung geholt — dabei verschiebt
        // sich nichts. Bilder ohne feste Masse bleiben `eager`: ihre
        // natuerliche Groesse braucht das Layout.
        let lazy = bk.bild.lazy || (aufschieben && !bk.platzhalter_hoehe && k.width > 0.0 && k.height > 0.0);
        // GEMESSEN, warum die REICHWEITE und nicht der Mittelpunkt: ein Bild
        // ist auch dann "sichtbar", wenn es halb hineinragt.
        //
        // 06.10.2026: EIN PLATZHALTER ZAEHLT ALS SICHTBAR. GEMESSEN an den
        // B12-Tests: `<img loading="lazy">` ohne Breitenangabe ist im
        // Inlinefluss ein 0 BREITES Replaced-Element (der reservierte
        // Platz steht nur in der HOEHE, `Bild::reservierte_hoehe`); mit der
        // alten Bedingung `width > 0` wurde es nie geholt, und die
        // Sichtbereichsregel galt als gebrochen (Cache 0). Ein Knoten mit
        // `platzhalter_hoehe` ist genau ein Bild, dessen Masse noch fehlen —
        // es zu holen ist der Zweck der Reservierung.
        if !lazy
            || ((k.width > 0.0 || bk.platzhalter_hoehe)
                && k.height > 0.0
                && k.y + k.height >= versatz - rand
                && k.y <= versatz + sicht + rand)
        {
            if !gebraucht.contains(&bk.bild.quelle) {
                gebraucht.push(bk.bild.quelle.clone());
            }
        }
    }
    gebraucht
}

/// 02.10. 02:35: DIE KÄSTEN AUS SEITE UND BAUM — die VORform von
/// [`Sitzung::bild_kaesten`] (siehe dort), damit die Regel an allen drei
/// Stellen von DEMSELBEN Code benutzt wird.
fn sammle_bildkaesten(
    bx: &narsil_layout::LayoutBox,
    raus: &mut std::collections::HashMap<NodeId, Rect>,
    versatz: u32,
    scroll_je_knoten: &std::collections::HashMap<u32, u32>,
) {
    if raus.contains_key(&bx.node) {
        // Release 10.10.2026: der INHALTSKASTEN — ein ersetztes Element malt
        // seinen Inhalt innerhalb von Innenabstand und Rahmen (CSS 2.1
        // §10.3.2, CSS Images 3 §5.5 `object-fit` „content box"). GEMESSEN
        // vorher (Web-Platform-Tests, `css/CSS2/positioning`): ein
        // `<img style="padding-left:48px;width:96px">` wurde 144 px breit
        // gezeichnet, ueber seinen Innenabstand.
        let mut r = bx.content_box;
        r.y -= versatz as f32;
        raus.insert(bx.node, r);
    }
    let eigener = scroll_je_knoten
        .get(&(bx.node.index() as u32))
        .copied()
        .unwrap_or(0);
    for kind in &bx.children {
        sammle_bildkaesten(kind, raus, versatz.saturating_add(eigener), scroll_je_knoten);
    }
}

fn kaesten_aus_baum(
    seite: &narsil_page::Seite,
    baum: &narsil_layout::LayoutTree,
    _grenze: usize,
    scroll_je_knoten: &std::collections::HashMap<u32, u32>,
) -> std::collections::HashMap<NodeId, Rect> {
    let mut raus: std::collections::HashMap<NodeId, Rect> =
        std::collections::HashMap::with_capacity(seite.bilder.len());
    for bk in &seite.bilder {
        raus.insert(bk.knoten, Rect::new(0.0, 0.0, 0.0, 0.0));
    }
    // 06.10.2026: EIN PLATZHALTER HAT KEINE BREITE — UND GEHOERT DOCH DAZU.
    //
    // GEMESSEN an den B12-Tests: `<img loading="lazy">` ohne Breitenangabe
    // ist im Inlinefluss 0 BREIT (reserviert ist nur die Hoehe, 160 px); der
    // `retain` unten warf den Kasten weg, `bild_kaesten` war leer, und kein
    // Bild wurde geholt (Cache 0), obwohl es im Sichtbereich lag. Ein
    // Knoten mit `platzhalter_hoehe` ist ein Bild, dessen Masse noch
    // fehlen — genau der Fall, fuer den die Reservierung existiert.
    let platzhalter: std::collections::HashSet<NodeId> = seite
        .bilder
        .iter()
        .filter(|b| b.platzhalter_hoehe)
        .map(|b| b.knoten)
        .collect();
    //
    // KONTEXT (05.10.2026, 15:05): DIESE **Y**-BERECHNUNG BEKOMMT DEN VERSATZ
    // **ALS PARAMETER** — GEMESSENE ENTSCHEIDUNG, warum: sie ist eine **freie**
    // Funktion ohne `Angezeigt`, und sie ist die **zweite** von drei
    // Y-Berechnungen (neben `text_stellen` und `feld_kasten`). Ein Bild in einem
    // gescrollten Container, das seine Position aus dem **Layout** holt, waere
    // sonst an einer anderen Stelle als sein Text.
    sammle_bildkaesten(&baum.root, &mut raus, 0, scroll_je_knoten);
    raus.retain(|n, r| r.height > 0.0 && (r.width > 0.0 || platzhalter.contains(n)));
    raus
}

/// Tragt dekodierte Naturmasse in die ersetzten Bildknoten ein.
/// Nur tatsaechlich geladene Bilder aendern den reservierten Kasten.
/// Release 10.10.2026: der BILD-VORLAUF je Seitenadresse (siehe den
/// Vorschau-Faden im Ladeweg): der Faden liefert einen Bild-Cache mit den
/// Bildern, die nach dem Layout der Vorschau sichtbar sind.
type BilderVorab = (String, std::thread::JoinHandle<crate::bilder::BildCache>);

fn bilder_vorab() -> &'static std::sync::Mutex<Option<BilderVorab>> {
    static S: std::sync::OnceLock<std::sync::Mutex<Option<BilderVorab>>> = std::sync::OnceLock::new();
    S.get_or_init(|| std::sync::Mutex::new(None))
}

/// Release 10.10.2026: den Vorlauf fuer `adresse` abholen (wartet auf ihn —
/// dieselben Abrufe stuenden sonst gleich danach an); fremde bleiben.
fn bilder_vorab_abholen(adresse: &str) -> Option<crate::bilder::BildCache> {
    let f = match bilder_vorab().lock() {
        Ok(mut g) if g.as_ref().is_some_and(|(a, _)| a == adresse) => g.take(),
        _ => None,
    }?;
    f.1.join().ok()
}

fn bildmasse_uebernehmen(seite: &mut Seite, cache: &crate::bilder::BildCache) -> bool {
    bildmasse_uebernehmen_mit(seite, |url| match cache.geladen.get(url) {
        Some(Ok(b)) => b.layout_masse(),
        _ => None,
    })
}

/// Release 10.10.2026: wie `bildmasse_uebernehmen`, die natuerlichen Groessen
/// aus einer Nachschlagefunktion (der Neubau-Faden hat keinen Bild-Cache).
pub(crate) fn bildmasse_uebernehmen_mit(seite: &mut Seite, masse_von: impl Fn(&str) -> Option<(f32, f32)>) -> bool {
    let mut geaendert = false;
    for bk in &seite.bilder {
        let Some((natur_breite, natur_hoehe)) = masse_von(&bk.bild.quelle) else {
            continue;
        };
        // Release 08.10.2026 (Bild-Diaet): die NATUERLICHE Groesse — ein
        // verkleinerter oder verworfener Puffer veraendert das Layout nicht.
        let masse = narsil_dom::ErsatzMasse {
            breite: Some(natur_breite),
            hoehe: Some(natur_hoehe),
        };
        if seite.document.ersatz(bk.knoten).ok().flatten() == Some(masse) {
            continue;
        }
        if seite.document.set_ersetzt(bk.knoten, masse).is_err() {
            continue;
        }
        if bk.platzhalter_hoehe {
            if let Some(node) = seite.document.get(bk.knoten) {
                //
                // GEMESSENE ENTSCHEIDUNG, warum `.clone()` und nicht `move`
                // (E0507): `document.get` leiht den Knoten **unveränderlich**
                // hinter einer Referenz, und `Style` ist kein `Copy` — der
                // Kasten muss also kopiert werden, bevor man ihn veraendert.
                // `set_style` schreibt ihn danach zurueck, die Kopie ist also
                // kein Preis, sondern der Weg.
                let mut stil = node.style.clone();
                stil.size.height = narsil_core::Length::Auto;
                let _ = seite.document.set_style(bk.knoten, stil);
            }
        }
        geaendert = true;
    }
    geaendert
}

fn bild_kaesten(a: &Angezeigt) -> std::collections::HashMap<NodeId, Rect> {
    kaesten_aus_baum(
        &a.seite,
        &a.baum,
        crate::bilder::MAX_BILDER,
        &a.scroll_je_knoten,
    )
}

/// Verschiebt ein Zeichenelement um `dy` Pixel.
fn verschiebe(item: &DisplayItem, dy: f32) -> DisplayItem {
    match item {
        //
        // BILD_33: **wie `Rect`** — der Rahmen wandert mit dem Kasten. Ein
        // stehengebliebener Rahmen waere ein Ring an der alten Stelle, und beim
        // Scrollen sieht man das als Streifen.
        DisplayItem::Rand {
            rect,
            rahmen,
            radius,
        } => DisplayItem::Rand {
            rect: Rect::new(rect.x, rect.y + dy, rect.width, rect.height),
            rahmen: *rahmen,
            radius: *radius,
        },
        //
        // S2 Station 3: die Kaskadenteile wandern MIT. `verschiebe` ist
        // keine Zeichnung, sondern eine Lageaenderung — und ein Radius,
        // eine Deckkraft oder ein Schatten, der an der alten Stelle
        // bliebe, waere ein Schatten an einem Kasten, der nicht mehr da
        // ist. Die Werte werden deshalb durchgereicht statt neu erzeugt.
        DisplayItem::Rect {
            rect,
            color,
            radius,
            deckkraft,
            schatten,
            schatten_anzahl,
        } => DisplayItem::Rect {
            rect: Rect::new(rect.x, rect.y + dy, rect.width, rect.height),
            color: *color,
            radius: *radius,
            deckkraft: *deckkraft,
            schatten: *schatten,
            schatten_anzahl: *schatten_anzahl,
        },
        //
        // BILD_48: der **Verlauf** wandert **wie** der Fuell-`Rect` mit —
        // gleiche Begründung: es ist eine Fuellung desselben Kastens. Und er
        // wird **durchgereicht**, nicht neu gemacht: eine Neuzeichnung des
        // Verlaufs je Frame wuerde beim Scrollen 30mal pro Sekunde dieselbe
        // Rechnung tun, und ein Verlauf, der beim Verschieben anders aussaehe
        // als der liegengebliebene, waere im Bild ein Streifen.
        DisplayItem::Verlauf {
            rect,
            radius,
            verlauf,
        } => DisplayItem::Verlauf {
            rect: Rect::new(rect.x, rect.y + dy, rect.width, rect.height),
            radius: *radius,
            verlauf: verlauf.clone(),
        },
        //
        // BAUREPARATUR 03.10.2026 (Rolle PROBE): das Muster band nur
        // `font_size` und `deko`, der Rumpf benutzt aber `origin`,
        // `text` und `color` — E0425 zweimal (4160). Es sind drei
        // Bindungen, sonst nichts. Der Fenster-Build war rot, und die
        // Abnahme UI6 braucht ihn.
        DisplayItem::Text {
            schriftfamilie,
            origin,
            text,
            color,
            font_size,
            deko,
            gewicht,
            kursiv,
        } => DisplayItem::Text {
            // Release 07.10.2026: Familie und Dekoration DURCHREICHEN.
            // GEMESSENER FEHLER: hier stand `String::from("sans-serif")` und
            // `deko: 0`. Jeder Seitentext im Fenster wurde damit in Segoe UI
            // gezeichnet (gemessen mit `serif` = times.ttf: „Strg+LAdressleiste"
            // auf der Startseite) und verlor seine Unterstreichung.
            schriftfamilie: schriftfamilie.clone(),
            deko: *deko,
            origin: Point::new(origin.x, origin.y + dy),
            text: text.clone(),
            color: *color,
            font_size: *font_size,
            // 04.10.2026 (SCHRIFT): **durchreichen, nicht 400/false.**
            //
            // GEMESSEN, warum das nicht die Vorgabe sein darf: diese
            // Funktion verschiebt Text um `dy` (Scroll-Schleife). Mit
            // `gewicht: 400, kursiv: false` waere jeder fette und kursive
            // Text nach dem ersten Scroll **aufrecht** — der Fehler waere
            // also nicht sichtbar, sondern wuerde beim Scrollen wiederkommen.
            gewicht: *gewicht,
            kursiv: *kursiv,
        },
        // I3: Ein Bild wandert MIT. Die Pixel sind ein `Arc`, sie werden
        // beim Verschieben also nicht kopiert — nur das Rechteck wandert.
        //
        // GEMESSEN, warum das nicht vergessen werden darf: der Versatz
        // entsteht beim Scrollen, also fuer JEDES Bild bei JEDEM Frame.
        // Eine Kopie der Pixel pro Frame waeren 30 × 2 MB pro Sekunde.
        DisplayItem::Bild {
            rect,
            breite,
            hoehe,
            pixels,
        } => DisplayItem::Bild {
            rect: Rect::new(rect.x, rect.y + dy, rect.width, rect.height),
            breite: *breite,
            hoehe: *hoehe,
            pixels: pixels.clone(),
        },
    }
}

/// Der TIEFSTE Knoten, dessen Rechteck den Punkt enthaelt.
///
/// M5: die Rekursion laeuft ueber den Layout-Baum, und dessen Tiefe
/// begrenzt `narsil-page` (`MAX_BAU_TIEFE` = 64). Ein zweiter Zaehler
/// waere eine Zahl ohne Wirkung.
///
/// GEMESSEN, warum der tiefste und nicht der erste: ein Container, der
/// ein Panel traegt, traegt sonst den Klick fuer alles darin, und ein
/// Knopf im Panel waer nicht mehr bedienbar.
/// W7C A6: DEN KASTEN EINES KNOTENS im Layout-Baum finden.
///
/// GEMESSEN, warum rekursiv und nicht ueber eine Tabelle: der Baum IST
/// die Wahrheit ueber die Kaeste, und eine zweite Tabelle waere wieder
/// „drei Zahlenraeume" (W5G). Der Weg ist O(n) ueber die Kaeste und
/// laeuft nur beim Blaettern (nicht beim Zeichnen).
fn kasten_von_knoten(
    box_: &narsil_layout::LayoutBox,
    knoten: usize,
) -> Option<narsil_core::geometry::Rect> {
    if box_.node.index() as usize == knoten {
        return Some(box_.content_box);
    }
    for kind in &box_.children {
        if let Some(r) = kasten_von_knoten(kind, knoten) {
            return Some(r);
        }
    }
    None
}

/// 02.10. 19:00 (HTML): DER FELDKNOTEN ZU EINEM KLICK — `<label>` gehoert dazu.
///
/// HTML 4, 17.9.1: „Ein Label steuert das Feld an, fuer das es steht" — und der
/// Nutzer klickt auf den **Text** des Labels, nicht auf das Feld.
///
/// GEMESSENER BEFUND, warum es diese Aufloesung gibt (Wegwerf-Messung,
/// entfernt, `narsil-page`/`narsil`): `<p>vorher <label>Name <input
/// name="n"></label> nachher</p>` ergibt **13 Knoten**, und der Knoten des
/// `<label>` existiert **gar nicht**: der Textknoten „Name" haengt am Absatz,
/// das Feld an einem anderen Knoten. GEMESSEN im Klickpfad: `tiefster_knoten`
/// liefert bei einem Klick auf den Labeltext den **Textknoten** des Labels,
/// und `fokus_bei_klick` sucht danach die **Nummer des Feldes** — die
/// stimmten nicht ueberein, also bekam das Feld keinen Fokus. Auf
/// `ubuntu.com` sind **37 von 47** Labels umschliessend (Zaehlung im
/// Quelltext, `crates/narsil/tests/label.rs`).
///
/// GEMESSENE ENTSCHEIDUNG, warum die Tabelle im **Bauer** gefuellt wird und
/// nicht hier nachgerechnet: der Zusammenhang „dieser Text gehoert zu diesem
/// Feld" steht im **HTML-Quelltext**, im flachen DOM-Baum nicht mehr (siehe
/// oben). Eine Elternkette kann es nicht liefern, und ein „naechster
/// Geschwister" waere eine Rate.
///
/// WAS ICH NICHT BEHAUPTE: dass damit jedes Label einer echten Seite wirkt.
/// GEMESSEN ist der Weg an einer Testseite mit beiden Faellen (`for` und
/// umschliessend); wie viele Labels einer echten Seite erfasst werden, ist
/// nicht gemessen.
/// DER KASTEN eines Feldes im **Layoutbaum** — **ein** Weg.
///
/// KONTEXT (05.10.2026, 02:50): GEMESSENE ENTSCHEIDUNG, warum das jetzt eine
/// Funktion ist und nicht in zwei Aufrufern steht: GEMESSEN ist, dass es **zwei**
/// Aufrufer gab — mein `fokus_rechteck` (KONTEXT_23) und `cursor_rechteck`
/// (W7 P2) — und beide gingen den **gleichen** Baumlauf. Zwei Laeufe ueber
/// denselben Baum sind zwei Wege zu derselben Kiste; wenn sich der Weg aendert,
/// aendert sich sonst nur einer.
/// DER WERT EINES `<input type=color>` als **Farbe**.
///
/// KONTEXT (05.10.2026, 07:50): GEMESSENE ENTSCHEIDUNG, warum **ein** Format
/// (`#rrggbb`) und keine Farbnamen: `input type=color` traegt genau dieses
/// Format (HTML) — und ein Farbname (`red`) waere eine zweite Sprache an
/// derselben Stelle.
fn wert_color(wert: &str) -> (u8, u8, u8) {
    let s = wert.trim().trim_start_matches('#');
    if s.len() != 6 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        // Ohne Wert: **schwarz** — das ist die Voreinstellung von
        // `input type=color` in jedem Browser.
        return (0, 0, 0);
    }
    let h = |a: char, b: char| u8::from_str_radix(&format!("{a}{b}"), 16).unwrap_or(0);
    let z: Vec<char> = s.chars().collect();
    (h(z[0], z[1]), h(z[2], z[3]), h(z[4], z[5]))
}

/// Release 08.10.2026: die Kaesten ALLER Formularfelder in einem Baumlauf
/// — dieselbe Regel wie [`feld_kasten`] (erster Treffer in Vorordnung,
/// Versatz der scrollenden Vorfahren abgezogen), nur fuer alle Felder auf
/// einmal statt je Feld ein ganzer Lauf.
/// Release 08.10.2026: liegt das Feld `ord` in einem sichtbaren Teilbaum?
/// Die Deckkraft der Vorfahren im Dokument wird aufmultipliziert (wie in
/// der Anzeigeliste); bei 0 ist das Feld unsichtbar.
fn feld_sichtbar(a: &Angezeigt, ord: usize) -> bool {
    let Some(start) = a.seite.dom_zu_ord.iter().filter(|(_, o)| **o == ord).map(|(k, _)| *k).min() else {
        return true;
    };
    let mut knoten = Some(narsil_dom::NodeId(start));
    let mut deckkraft = 1.0f32;
    // Release 11.10.2026: `visibility: hidden` (vererbt, also am Feld selbst
    // abzulesen) — GEMESSEN vorher: bbc.com, das Suchfeld der versteckten
    // Navigationsschublade stand oben links ueber der Seite.
    if a.seite.document.get(narsil_dom::NodeId(start)).is_some_and(|n| n.style.unsichtbar) {
        return false;
    }
    for _ in 0..512 {
        let Some(id) = knoten else { break };
        let Some(n) = a.seite.document.get(id) else { break };
        if n.style.deckkraft.is_finite() {
            deckkraft *= n.style.deckkraft.clamp(0.0, 1.0);
        }
        if deckkraft <= 0.002 {
            return false;
        }
        knoten = n.parent;
    }
    true
}

/// Release 08.10.2026: WIE EIN FORMULARFELD IM BILD STEHT — fuer den
/// Ueberzug (`feld_ueberzug`) und den Cursor (`cursor_rechteck`).
///
/// GEMESSEN vorher: der Ueberzug malte fest Weiss mit dunklem Text an
/// `kasten + 6 px`, der Cursor stand hellgrau (`0xf0f0f0`) an `kasten.x`
/// ohne Polster. Auf einem dunklen Feld mit Wert (Startseite, jede Seite
/// mit dunklem Suchfeld) stand ein weisser Kasten im Bild, auf einem hellen
/// Feld war der Cursor unsichtbar, und Wert und Cursor sassen nicht dort,
/// wo die Seite den Text setzt. Jetzt kommen Flaeche, Farben, Schrift und
/// Radius aus dem Stil des Feldes und die Textstelle aus dem Textlauf der
/// Seite selbst.
#[derive(Debug, Clone)]
struct FeldBild {
    /// Die Polsterflaeche (innerhalb des Rahmens): sie deckt der Ueberzug.
    flaeche: Rect,
    /// Der Ursprung des Textes, so wie die Seite ihn setzt (Oberkante des
    /// Aufstiegs, dieselbe Bedeutung wie `DisplayItem::Text::origin`).
    text_x: f32,
    text_y: f32,
    /// Die rechte Kante des Inhalts (dort endet der Cursor spaetestens).
    inhalt_rechts: f32,
    /// Die deckende Farbe unter dem Text (Feldflaeche, sonst der naechste
    /// deckende Vorfahr).
    grund: Color,
    farbe: Color,
    schrift: f32,
    gewicht: u16,
    familie: String,
    /// Der Radius der Flaeche (innen) und des Rahmenkastens (aussen, fuer
    /// den Fokusring).
    radius: [f32; 4],
    radius_aussen: [f32; 4],
    /// Release 10.10.2026: die Farbe des Platzhalter-Laufs der Seite.
    platzhalter: Option<Color>,
}

impl FeldBild {
    /// Platzhalter: die Farbe, die die Seite fuer `::placeholder` gesetzt hat
    /// (`kaskade::platzhalter_farbe`, UA-Grau #757575); ohne sie die
    /// Textfarbe zu 55 % ueber dem Grund (Firefox zeichnet `::placeholder`
    /// mit `opacity: 0.54`).
    ///
    /// Release 10.10.2026: GEMESSEN vorher wurde die schon gefaerbte
    /// Platzhalterfarbe NOCHMAL gemischt — auf der dunklen Startseite
    /// rgb(77,78,84) statt #757575 (Test
    /// `die_startseite_hat_ein_glas_suchfeld_mit_platzhalter`).
    fn platzhalter_farbe(&self) -> Color {
        if let Some(p) = self.platzhalter {
            return p;
        }
        let m = |g: u8, f: u8| (g as f32 + (f as f32 - g as f32) * 0.55).round().clamp(0.0, 255.0) as u8;
        Color::rgb(m(self.grund.r, self.farbe.r), m(self.grund.g, self.farbe.g), m(self.grund.b, self.farbe.b))
    }
}

/// KONTEXT (03.10.2026): EINE DATEI IN DAS DOWNLOAD-VERZEICHNIS SCHREIBEN.
///
/// GEMESSENE ENTSCHEIDUNG, warum **hier** `std::fs` benutzt wird und nicht
/// `narsil-download`: dort gibt es `download_sync(url, name)` — das **holt**
/// ueber HTTP und nimmt **keine** fertigen Bytes (`manager.rs:44`). Also
/// waere der Umweg: Bytes holen, umwandeln, und dann **noch einmal** einen
/// Download anstossen, der dieselbe Datei unveraendert schreibt.
///
/// M4/M9: der Pfad ist aus dem **Namen** der Datei gebaut, und der Name
/// kommt aus `ktx_konvert::stammlname` (dort ist er gesaeubert und
/// begrenzt). M9: das Verzeichnis liegt **nicht** unter `/tmp`.
///
/// Release 08.10.2026: eine freie Funktion mit dem Ordner als Argument —
/// sie laeuft im Hintergrundfaden des Umwandlers (siehe [`KonvertLauf`]).
/// Und der zehnte Versuch UEBERSCHREIBT nicht mehr: vorher schrieb er auf
/// `name (10).endung`, auch wenn es die Datei schon gab.
fn datei_schreiben_nach(
    ordner: &std::path::Path,
    datei: &narsil_chrome::ktx_konvert::Datei,
) -> Result<std::path::PathBuf, std::io::Error> {
    use std::path::Path;
    std::fs::create_dir_all(ordner)?;
    // GEMESSENE ENTSCHEIDUNG, warum `resolve_filename` **hier** nachgebaut
    // wird und nicht `narsil-download` genutzt: dort ist es eine Methode auf
    // dem `DownloadManager` (der ein Verzeichnis **anlegt**), und ein
    // `DownloadManager` nur fuer einen Dateinamen waere ein zweiter Weg.
    // M3: hoechstens so viele Namen werden versucht.
    const MAX_DATEIEN: u32 = 1000;
    for nr in 0..=MAX_DATEIEN {
        let name = if nr == 0 {
            datei.name.clone()
        } else {
            // GEMESSEN: der Name wird **vor** dem Punkt geteilt, damit
            // `foto (1).png` und nicht `foto.png (1)` herauskommt.
            let p = Path::new(&datei.name);
            let stamm = p.file_stem().unwrap_or_default().to_string_lossy();
            let endung = p.extension().unwrap_or_default().to_string_lossy();
            format!("{stamm} ({nr}).{endung}")
        };
        let pfad = ordner.join(name);
        // `create_new`: nie eine vorhandene Datei ueberschreiben, auch nicht,
        // wenn zwei Auftraege gleichzeitig denselben Namen waehlen.
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&pfad) {
            Ok(mut f) => {
                use std::io::Write;
                f.write_all(&datei.daten)?;
                return Ok(pfad);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, narsil_i18n::tr("too many files with this name")))
}

/// Release 08.10.2026: eine Bytezahl fuer Menschen („12,4 KB", „3,1 MB").
fn groesse_text(bytes: usize) -> String {
    let b = bytes as f64;
    if b < 1024.0 {
        trf!("{} bytes", bytes)
    } else if b < 1024.0 * 1024.0 {
        format!("{:.1} KB", b / 1024.0).replace('.', ",")
    } else {
        format!("{:.1} MB", b / (1024.0 * 1024.0)).replace('.', ",")
    }
}

/// Release 08.10.2026: ein Pfad als `file://`-Adresse (fuer den Hinweis,
/// der die fertige Datei oeffnet).
fn datei_adresse(pfad: &std::path::Path) -> String {
    let p = pfad.to_string_lossy().replace('\\', "/");
    if p.starts_with('/') {
        format!("file://{p}")
    } else {
        format!("file:///{p}")
    }
}

/// Release 08.10.2026: EIN ATTRIBUT DES FELDES `ord` — aus dem DOM-Stand
/// (`ids` → Griff → Attribute, derselbe Weg wie `feld_nur_lesbar`).
///
/// GEMESSENER FEHLER vorher: `a.html.get(ord)` las den `ord`-ten Knoten der
/// OBERSTEN Ebene des geparsten Baums, nicht das Feld. Auf der Startseite
/// ist `ord` 12 und die oberste Ebene hat sechs Knoten — `type` und
/// `placeholder` kamen nie an: kein grauer Platzhalter, keine Passwortpunkte,
/// kein Regler- oder Farbfeld-Ueberzug, kein Dateidialog per `type=file`.
fn feld_attribut(a: &Angezeigt, ord: usize, name: &str) -> Option<String> {
    let stand = a.stand.lock().ok()?;
    let h = *stand.ids.get(&ord)?;
    // Release 15.10.2026: the live DOM is the truth for a field's attributes;
    // the old attribute table of the proxy state can hold stale entries.
    // MEASURED before (github.com hero form, after the classList fix let the
    // page script run on): the email field was drawn as an `<input
    // type=color>` (a black square) with a placeholder set later by script.
    if let Some(l) = stand.lebend.as_ref()
        && let Some(k) = l.knoten(h)
    {
        return l.dok.attribut(k, name).map(str::to_string);
    }
    stand.attribute.get(&h).and_then(|m| m.get(name).cloned())
}

/// Release 08.10.2026: WAS EIN TEXTFELD ZEIGT — Passwortpunkte (einer je
/// Zeichen; die Laenge ist es, die der Nutzer prueft), sonst der Wert, bei
/// leerem Wert der Platzhalter. Eine Stelle fuer Ueberzug und Cursor
/// (vorher mass der Cursor beim Passwort die Buchstaben statt der Punkte).
fn feld_zeichen(a: &Angezeigt, ord: usize, typ: &str, wert: &str) -> String {
    /// Die Grenze des Wertes (`MAX_EINGABE_WERT`); ein Ausschnitt wird
    /// danach ohnehin auf die Feldbreite gekuerzt.
    const MAX_ZEICHEN: usize = 4096;
    if typ == "password" {
        return "\u{2022}".repeat(wert.chars().count().min(MAX_ZEICHEN));
    }
    if wert.is_empty() {
        return feld_attribut(a, ord, "placeholder")
            .unwrap_or_default()
            .chars()
            .take(MAX_ZEICHEN)
            .collect();
    }
    wert.chars().take(MAX_ZEICHEN).collect()
}

/// Den ersten Textlauf im Teilbaum (das ist der Text, den die Seite im
/// Feld zeigt: Wert oder Platzhalter). Tiefe begrenzt (ein Feld hat drei
/// Ebenen).
fn erster_textlauf(bx: &narsil_layout::LayoutBox, tiefe: u8) -> Option<&narsil_layout::LayoutBox> {
    if bx.text.is_some() {
        return Some(bx);
    }
    if tiefe == 0 {
        return None;
    }
    bx.children.iter().find_map(|k| erster_textlauf(k, tiefe - 1))
}

/// Ein Feld aus seinem aeusseren Kasten lesen (`feld_kasten_mit_inhalt`
/// in `narsil-page` baut zwei Knoten: aussen der Rahmen, innen die Flaeche
/// mit Polster und Text).
fn feld_bild_aus(
    a: &Angezeigt,
    bx: &narsil_layout::LayoutBox,
    versatz: f32,
    grund_vorfahr: Color,
    abb: (f32, f32, f32),
) -> FeldBild {
    let innen = bx.children.first().filter(|k| k.text.is_none()).unwrap_or(bx);
    let stil = a.seite.document.get(innen.node).map(|n| &n.style);
    let aussen_stil = a.seite.document.get(bx.node).map(|n| &n.style);
    let grund = match stil.map(|s| s.background) {
        Some(c) if c.a >= 250 => Color::rgb(c.r, c.g, c.b),
        Some(c) if c.a > 0 => {
            let t = c.a as f32 / 255.0;
            let m = |g: u8, f: u8| (g as f32 + (f as f32 - g as f32) * t).round().clamp(0.0, 255.0) as u8;
            Color::rgb(m(grund_vorfahr.r, c.r), m(grund_vorfahr.g, c.g), m(grund_vorfahr.b, c.b))
        }
        _ => grund_vorfahr,
    };
    // Release 10.10.2026: die Textfarbe des FELDES (nicht die des inneren
    // Laufs — der traegt beim leeren Feld schon die Platzhalterfarbe).
    let farbe = aussen_stil
        .map(|s| s.color)
        .filter(|c| c.a > 0)
        .or_else(|| stil.map(|s| s.color).filter(|c| c.a > 0))
        .unwrap_or(Color::rgb(0x22, 0x22, 0x22));
    let mut platzhalter: Option<Color> = None;
    let mut schrift = stil
        .map(|s| s.font_size)
        .filter(|f| f.is_finite() && *f > 0.0)
        .unwrap_or(narsil_page::GRAD_TEXT)
        * abb.0;
    let radius = stil
        .map(|s| s.radius)
        .filter(|r| r.iter().any(|v| *v > 0.0))
        .or_else(|| aussen_stil.map(|s| s.radius))
        .unwrap_or([0.0; 4]);
    // Release 11.10.2026: wie gezeichnet (`transform`), dann gerollt.
    let mut flaeche = gezeichnet(innen.border_box, abb);
    flaeche.y -= versatz;
    let mut inhalt = gezeichnet(innen.content_box, abb);
    inhalt.y -= versatz;
    // Familie und Staerke: die des TEXTKNOTENS (mit ihm zeichnet die
    // Seite), sonst die der Flaeche.
    let mut text_stil = stil;
    let (text_x, text_y) = match erster_textlauf(innen, 3).and_then(|k| k.text.as_ref().map(|t| (k, t))) {
        Some((k, t)) => {
            if t.font_size.is_finite() && t.font_size > 0.0 {
                schrift = t.font_size * abb.0;
            }
            if let Some(n) = a.seite.document.get(k.node) {
                text_stil = Some(&n.style);
                // Beim leeren Feld ist dieser Lauf der Platzhalter.
                if n.style.color.a > 0 {
                    platzhalter = Some(Color::rgb(n.style.color.r, n.style.color.g, n.style.color.b));
                }
            }
            (abb.0 * t.origin.x + abb.1, abb.0 * t.origin.y + abb.2 - versatz)
        }
        // Ohne Text der Seite: die Zeile senkrecht mittig im Inhalt (der
        // Aufstieg plus Abstieg der Oberflaechenschrift ist ~1,16 em).
        None => (inhalt.x, inhalt.y + ((inhalt.height - schrift * 1.16) / 2.0).max(0.0)),
    };
    FeldBild {
        flaeche,
        text_x,
        text_y,
        inhalt_rechts: inhalt.x + inhalt.width,
        grund,
        farbe: Color::rgb(farbe.r, farbe.g, farbe.b),
        platzhalter,
        schrift,
        gewicht: text_stil.map(|s| s.schriftgewicht).filter(|g| *g > 0).unwrap_or(400),
        familie: text_stil
            .map(|s| s.schriftfamilie.clone())
            .filter(|f| !f.trim().is_empty())
            .unwrap_or_else(|| String::from("sans-serif")),
        radius,
        radius_aussen: aussen_stil.map(|s| s.radius).unwrap_or([0.0; 4]),
    }
}

/// Die Bilder der Felder `ords` in EINEM Baumlauf (dieselbe Regel wie
/// [`feld_kaesten_alle`]: erster Treffer in Vorordnung, Versatz der
/// scrollenden Vorfahren abgezogen). Nebenbei laeuft die Farbe des
/// naechsten deckenden Vorfahren mit — fuer Felder mit durchsichtiger
/// Flaeche.
fn feld_bilder(a: &Angezeigt, ords: &[usize]) -> std::collections::HashMap<usize, FeldBild> {
    let mut raus = std::collections::HashMap::new();
    if ords.is_empty() {
        return raus;
    }
    let mut knoten: Vec<(u32, usize)> = a
        .seite
        .dom_zu_ord
        .iter()
        .filter(|(_, ord)| ords.contains(ord))
        .map(|(k, o)| (*k, *o))
        .collect();
    knoten.sort_unstable();
    if knoten.is_empty() {
        return raus;
    }
    fn sammle(
        bx: &narsil_layout::LayoutBox,
        knoten: &[(u32, usize)],
        a: &Angezeigt,
        versatz: u32,
        grund: Color,
        raus: &mut std::collections::HashMap<usize, FeldBild>,
        aussen: (f32, f32, f32),
    ) {
        let nr = bx.node.index() as u32;
        let abb = bx.abbildung(aussen);
        if abb.0 <= 0.001 {
            return;
        }
        if let Ok(i) = knoten.binary_search_by(|(k, _)| k.cmp(&nr)) {
            let ord = knoten[i].1;
            if !raus.contains_key(&ord) {
                raus.insert(ord, feld_bild_aus(a, bx, versatz as f32, grund, abb));
            }
        }
        let grund = match a.seite.document.get(bx.node).map(|n| n.style.background) {
            Some(c) if c.a >= 250 => Color::rgb(c.r, c.g, c.b),
            _ => grund,
        };
        let eigener = if a.scroll_je_knoten.is_empty() {
            0
        } else {
            a.scroll_je_knoten.get(&nr).copied().unwrap_or(0)
        };
        for kind in &bx.children {
            sammle(kind, knoten, a, versatz + eigener, grund, raus, abb);
        }
    }
    sammle(&a.baum.root, &knoten, a, 0, Color::rgb(0xff, 0xff, 0xff), &mut raus, (1.0, 0.0, 0.0));
    raus
}

fn feld_kaesten_alle(a: &Angezeigt) -> std::collections::HashMap<usize, Rect> {
    let mut raus = std::collections::HashMap::new();
    if a.seite.felder.is_empty() {
        return raus;
    }
    // Die Knoten der Felder vorab (eine Liste, meist eine Handvoll): im
    // Baumlauf ist dann je Kasten nur ein Vergleich noetig statt eines
    // Hash-Nachschlags (GEMESSEN: 1,4 ms je Lauf auf 4 486 Kaesten).
    let mut knoten: Vec<(u32, usize)> = a
        .seite
        .dom_zu_ord
        .iter()
        .filter(|(_, ord)| a.seite.felder.iter().any(|f| f.ord == **ord))
        .map(|(k, o)| (*k, *o))
        .collect();
    knoten.sort_unstable();
    if knoten.is_empty() {
        return raus;
    }
    fn sammle(
        bx: &narsil_layout::LayoutBox,
        knoten: &[(u32, usize)],
        a: &Angezeigt,
        versatz: u32,
        raus: &mut std::collections::HashMap<usize, Rect>,
        aussen: (f32, f32, f32),
    ) {
        let nr = bx.node.index() as u32;
        let abb = bx.abbildung(aussen);
        if abb.0 <= 0.001 {
            return;
        }
        if let Ok(i) = knoten.binary_search_by(|(k, _)| k.cmp(&nr)) {
            let ord = knoten[i].1;
            raus.entry(ord).or_insert_with(|| {
                let mut r = gezeichnet(bx.border_box, abb);
                r.y -= versatz as f32;
                r
            });
        }
        let eigener = if a.scroll_je_knoten.is_empty() {
            0
        } else {
            a.scroll_je_knoten.get(&nr).copied().unwrap_or(0)
        };
        for kind in &bx.children {
            sammle(kind, knoten, a, versatz + eigener, raus, abb);
        }
    }
    sammle(&a.baum.root, &knoten, a, 0, &mut raus, (1.0, 0.0, 0.0));
    raus
}

fn feld_kasten(a: &Angezeigt, ord: usize) -> Option<Rect> {
    //
    // KONTEXT (05.10.2026, 17:35): DIE **DRITTE** Y-BERECHNUNG, und jetzt mit
    // der Kette.
    //
    // GEMESSENE ENTSCHEIDUNG, warum der Versatz **hier** genauso mitgerechnet
    // wird wie in `text_stellen` und `kaesten_aus_baum`: GEMESSEN ist, dass diese
    // drei Funktionen dieselbe Frage beantworten („wo steht das im Fenster?“) und
    // dass `feld_kasten` die Grundlage von **Treffer** (`feld_bei_punkt`) und
    // **Auswahl** (`auswahl_klick`) ist — ein Kasten, der anders liegt als sein
    // Text, heisst: der Cursor steht neben dem Zeichen.
    //
    // GEMESSENE ENTSCHEIDUNG, warum der **eigene** Versatz des Feldes ihn **nicht**
    // verschiebt: ein `<input>` ist selbst kein `overflow:auto`-Container; es ist
    // **in** einem. Der Versatz, der zaehlt, ist der der **Vorfahren**.
    fn sammle(
        bx: &narsil_layout::LayoutBox,
        ord: usize,
        a: &Angezeigt,
        versatz: u32,
        aussen: (f32, f32, f32),
    ) -> Option<Rect> {
        let abb = bx.abbildung(aussen);
        if a.seite.dom_zu_ord.get(&(bx.node.index() as u32)) == Some(&ord) {
            let mut r = gezeichnet(bx.border_box, abb);
            r.y -= versatz as f32;
            return Some(r);
        }
        let eigener = a
            .scroll_je_knoten
            .get(&(bx.node.index() as u32))
            .copied()
            .unwrap_or(0);
        for kind in &bx.children {
            if let Some(r) = sammle(kind, ord, a, versatz + eigener, abb) {
                return Some(r);
            }
        }
        None
    }
    sammle(&a.baum.root, ord, a, 0, (1.0, 0.0, 0.0))
}

fn feld_knoten_bei_klick(a: &Angezeigt, knoten: NodeId, text: &str) -> NodeId {
    //
    // 02.10. 22:00 (HTML): ZWEI WEG, EIN ZIEL.
    //
    // GEMESSENE EINORDNUNG, warum der **erste** Weg zuerst kommt: wenn der
    // Klick den Labeltextknoten selbst trifft, ist die Antwort sicherer als
    // der Textvergleich — der Text kann zweimal auf einer Seite stehen. Er wird
    // im GEMESSENEN Fall aber nie gebraucht, weil `tiefster_knoten` den
    // Container liefert (Knoten 2 statt 5).
    //
    // GEMESSENE GRUNDLAGE fuer den zweiten Weg (Wegwerf-Messungen, entfernt):
    // ein Klick auf „Name" liefert `tiefster_knoten` = Knoten **2**, und die
    // Anzeigeliste hat an diesem Punkt einen Textkasten mit dem Inhalt „Name"
    // (GEMESSEN: `Text "Name" bei (72,148) size=16`, Klickpunkt (76,156)).
    // Also: Knoten **und** Text sind bekannt, und `Seite::label_zu_text`
    // beantwortet genau diese Frage.
    //
    // GEMESSENE EINORDNUNG, warum die Messstelle **vor** den Suchen steht und
    // nicht danach (22:35): sie stand zuerst am Ende, und dann kam sie genau
    // bei den Klicks **nicht**, die funktionieren — der Erfolg kehrt frueher
    // zurueck. Eine Messstelle, die genau die interessanten Faelle verschluckt,
    // ist keine.
    let knoten_ein = knoten;
    let knoten = if let Some(feld) = a.seite.label_zu_knoten.get(&knoten) {
        *feld
    } else {
        let text = text.trim();
        if !text.is_empty()
            && let Some(feld) = a.seite.label_zu_text.get(&(knoten, text.to_string()))
        {
            *feld
        } else {
            knoten
        }
    };
    //
    // 02.10. 22:15 (HTML): DIE MESSSTELLE (`NARSIL_L1_LABEL`) — dieselbe Form wie
    // `NARSIL_L1_BILDER`: sie druckt beide Seiten der Suche, damit man sieht,
    // warum sie nicht passt. Ohne die Variable wird nichts gedruckt.
    //
    // GEMESSENE EINORDNUNG, warum das ein Werkzeug und kein `eprintln!` ist:
    // die Suche hat zwei Schluessel (Knoten, Text) und eine Tabelle mit N
    // Eintraegen; wenn sie leer bleibt, ist die Frage „welcher der beiden
    // Schluessel stimmt nicht" ohne diese Zeile nicht beantwortbar.
    if std::env::var_os("NARSIL_L1_LABEL").is_some() {
        eprintln!(
            "L1LABEL\tKLICK\tein={:?}\ttext={:?}\tregeln={:?}",
            knoten_ein.index(),
            text,
            a.seite
                .label_zu_text
                .iter()
                .map(|((k, t), f)| (k.index(), t.clone(), f.index()))
                .collect::<Vec<_>>()
        );
        eprintln!("L1LABEL\tERGEBNIS\tknoten={:?}", knoten.index());
    }
    knoten
}

/// KOMPRESS 04.10.2026: sammelt die `src` aller `<audio>`-Elemente in
/// **Dokumentordnung**.
///
/// GEMESSENER GRUND fuer den **geparsten** Baum: `Angezeigt.html` ist ein
/// `HtmlNode`-Baum, und der traegt die **Attribute** — `narsil_dom::NodeKind`
/// kennt GEMESSEN nur `Page | Container | Text` und damit **keine**
/// Attribute. Also gibt es nur diesen Ort, an dem `src` ueberhaupt steht.
fn sammle_audio(knoten: &[narsil_fmt_html::HtmlNode], raus: &mut Vec<String>) {
    for k in knoten {
        match k {
            narsil_fmt_html::HtmlNode::Element {
                tag,
                attributes,
                children, ..
            } => {
                if tag.eq_ignore_ascii_case("audio") {
                    if let Some(src) = attributes.get("src") {
                        raus.push(src.clone());
                    }
                }
                sammle_audio(children, raus);
            }
            narsil_fmt_html::HtmlNode::Text(_) => {}
        }
    }
}

/// KOMPRESS 04.10.2026: `src` gegen die **Basis der Seite** aufloesen.
///
/// GEMESSENE ENTSCHEIDUNG: `src="ton.wav"` in `…/probe/seite/test.html`
/// heisst `…/probe/seite/ton.wav` — **das Verzeichnis der Seite**, nicht das
/// Arbeitsverzeichnis des Prozesses. Und GEMESSEN ist, dass `basis` bei
/// `file://` das **Verzeichnis mit dem Trenner** traegt, also muss der
/// Dateiname angehaengt und nicht ersetzt werden.
fn pfad_relativ(basis: &str, src: &str) -> String {
    if src.starts_with('/') {
        return src.to_string();
    }
    // **GEMESSEN: `file:///pfad/x.html` -> `/pfad/` als Verzeichnis.**
    let schlussteil = basis.split("://").last().unwrap_or(basis);
    if basis.contains("://") {
        if schlussteil.is_empty() || schlussteil.ends_with('/') {
            format!("{schlussteil}{src}")
        } else {
            // **Der letzte Teil ist der DATEINAME**, nicht das Verzeichnis.
            match schlussteil.rfind('/') {
                Some(i) => format!("{}{src}", &schlussteil[..=i]),
                None => src.to_string(),
            }
        }
    } else {
        src.to_string()
    }
}

/// KOMPRESS 04.10.2026: der **Zeitstempel** fuer die Ausgabe-Logzeilen.
///
/// GEMESSENER GRUND: die Abnahme der Leitung verlangt einen **Zeitstempel
/// der Ausgabe**. Ohne ihn ist eine Zeile `TON: FERTIG` nicht von der
/// vorigen zu unterscheiden. Und GEMESSEN: `SystemTime` ist das, was
/// `Instant` nicht liefert — `Instant` hat **keine** Uhrzeit.
fn zeit_stempel() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    // **GEMESSENE GENAUIGKEIT:** Millisekunden seit der Epoche. Mehr
    // braucht der Nachweis nicht, und eine Sekunde ohne Bruchteil waere
    // bei einem 3-Sekunden-Ton zu grob.
    format!("{}.{:03} s", d.as_secs(), d.subsec_millis())
}

/// Release 08.10.2026: DER LINK EINES BLOCK-ANKERS — vom getroffenen Knoten
/// die Vorfahren hinauf bis zu einem `<a href>` im DOM-Stand.
///
/// GEMESSEN vorher: `Seite::links` kennt bei `<a style="display:block">Name</a>`
/// nur den TEXTknoten. Ein Klick auf die Kachel neben dem Wort traf den
/// Kasten des Ankers, dessen Vorfahren keinen Link tragen — die Kacheln der
/// Startseite taten nichts (`ui2_…`: „die Kacheln sind keine Links: 3"), und
/// jede Karte, deren `<a>` ein Block ist, war nur auf ihrem Text klickbar.
/// Firefox: der ganze Kasten des Ankers ist der Link.
fn anker_ueber(a: &Angezeigt, knoten: NodeId) -> Option<String> {
    const MAX_TIEFE: usize = 256;
    let stand = a.stand.lock().ok()?;
    let mut k = Some(knoten);
    for _ in 0..MAX_TIEFE {
        let id = k?;
        if let Some(ord) = a.seite.dom_zu_ord.get(&(id.index() as u32))
            && let Some(h) = stand.ids.get(ord)
            && stand.tag.get(h).is_some_and(|t| t.eq_ignore_ascii_case("a"))
            && let Some(href) = stand.attribute.get(h).and_then(|m| m.get("href"))
            && !href.trim().is_empty()
        {
            let href = href.trim();
            return Some(narsil_url::loese_auf(&a.seite.basis, href).unwrap_or_else(|_| href.to_string()));
        }
        k = a.seite.document.get(id).and_then(|n| n.parent);
    }
    None
}

/// Release 08.10.2026: der Link am Punkt (Seitenkoordinaten) — erst der
/// Text eines Links (`narsil_page::link_unter`), sonst der Kasten eines
/// Block-Ankers (`anker_ueber`). Fuer Kontextmenue und mittlere Taste.
fn link_am_punkt(a: &Angezeigt, x: f32, y: f32) -> Option<String> {
    // Release 09.10.2026: ein fester Kasten (Kopfleiste, Menue) liegt
    // ueber der Seite — sein Link gewinnt, in ungerollten Koordinaten.
    if let Some(k) = fester_treffer(&a.baum, x, y - a.versatz as f32) {
        return anker_ueber(a, k);
    }
    narsil_page::link_unter(&a.seite, &a.baum, (x, y))
        .or_else(|| tiefster_knoten(&a.baum, x, y).and_then(|k| anker_ueber(a, k)))
}

/// Release 10.10.2026: `accept` (HTML §4.10.5.1.18) als Filter des Dialogs:
/// `image/*`, `video/*`, `audio/*`, MIME-Typen und `.endung`.
fn datei_filter_aus_accept(annahme: &str) -> Vec<(String, String)> {
    let mut muster: Vec<String> = Vec::new();
    let mut dazu = |m: &str| {
        let m = m.to_string();
        if !muster.contains(&m) {
            muster.push(m);
        }
    };
    for teil in annahme.split(',').map(|t| t.trim().to_ascii_lowercase()).filter(|t| !t.is_empty()) {
        if let Some(endung) = teil.strip_prefix('.') {
            dazu(&format!("*.{endung}"));
            continue;
        }
        let endungen: &[&str] = match teil.as_str() {
            "image/*" => &["png", "jpg", "jpeg", "gif", "webp", "avif", "bmp", "svg", "ico", "heic"],
            "video/*" => &["mp4", "webm", "mov", "mkv", "avi", "m4v"],
            "audio/*" => &["mp3", "wav", "ogg", "flac", "m4a", "opus", "aac"],
            "image/png" => &["png"],
            "image/jpeg" => &["jpg", "jpeg"],
            "image/gif" => &["gif"],
            "image/webp" => &["webp"],
            "image/svg+xml" => &["svg"],
            "application/pdf" => &["pdf"],
            "text/plain" => &["txt"],
            "text/csv" => &["csv"],
            "application/json" => &["json"],
            "application/zip" => &["zip"],
            "video/mp4" => &["mp4"],
            "audio/mpeg" => &["mp3"],
            _ => &[],
        };
        for e in endungen {
            dazu(&format!("*.{e}"));
        }
    }
    if muster.is_empty() {
        return Vec::new();
    }
    let liste = muster.join(";");
    vec![(trf!("Allowed files ({})", liste), liste)]
}

/// Release 09.10.2026: die Kaesten des Layouts je DOM-Element in den
/// DOM-Stand schreiben (`__narsilMass`). Ein Element mit mehreren Kaesten
/// (Zeilen eines Absatzes) bekommt ihre Vereinigung.
/// Release 11.10.2026: die aufgeloesten Rastervorlagen des Layouts je Host
/// (siehe `Stand::raster_vorlagen`), mit derselben Zuordnung wie
/// [`masse_rechnen`].
pub(crate) fn vorlagen_rechnen(
    baum: &narsil_layout::LayoutTree,
    seite: &Seite,
    ids: &HashMap<usize, u32>,
) -> HashMap<u32, (String, String)> {
    let mut raus = HashMap::new();
    for (knoten, spalten, zeilen) in &baum.raster_vorlagen {
        if let Some(ord) = seite.dom_zu_ord.get(&(knoten.index() as u32))
            && let Some(&host) = ids.get(ord)
        {
            raus.insert(host, (spalten.clone(), zeilen.clone()));
        }
    }
    raus
}

fn masse_eintragen(a: &Angezeigt) {
    let Ok(mut w) = a.stand.lock() else { return };
    let ids = std::mem::take(&mut w.ids);
    w.masse = masse_rechnen(&a.baum, &a.seite, &ids);
    w.raster_vorlagen = vorlagen_rechnen(&a.baum, &a.seite, &ids);
    if w.textlaeufe_an {
        w.textlaeufe = crate::bereich::textlaeufe_rechnen(&a.baum, &a.seite, &ids);
    }
    w.ids = ids;
    // Release 13.10.2026: ein noch offener Bildlaufwunsch des Skripts gewinnt.
    if w.rollen_wunsch.is_none() {
        w.rollen_y = a.versatz as f32;
    }
    // Release 10.10.2026: das Layout gehoert zum aktuellen HTML (siehe
    // `zwangslayout`).
    w.layout_veraltet = false;
}

/// Release 10.10.2026: wie [`masse_eintragen`], aber mit der Zuordnung
/// `ord -> Host` des HTML, aus dem `a.baum` gebaut ist. Ein Neubau aus dem
/// Neubau-Faden rechnet mit einer MOMENTAUFNAHME; aendert ein Skript das DOM
/// waehrenddessen, nummeriert `Stand::ids` schon das neue HTML.
///
/// GEMESSEN vorher (heise.de): die Kaesten des alten Baums landeten bei den
/// Hosts der NEUEN Nummern — `<a-img>` las als `offsetWidth` die Breite
/// fremder Knoten (35/76 px statt 336) und bestellte winzige Bilder.
fn masse_eintragen_mit(a: &Angezeigt, ids: &HashMap<usize, u32>) {
    let masse = masse_rechnen(&a.baum, &a.seite, ids);
    let vorlagen = vorlagen_rechnen(&a.baum, &a.seite, ids);
    let Ok(mut w) = a.stand.lock() else { return };
    w.masse = masse;
    w.raster_vorlagen = vorlagen;
    if w.textlaeufe_an {
        w.textlaeufe = crate::bereich::textlaeufe_rechnen(&a.baum, &a.seite, ids);
    }
    if w.rollen_wunsch.is_none() {
        w.rollen_y = a.versatz as f32;
    }
}

/// Die Kaesten eines Baums je Host (Vereinigung aller Kaesten eines Knotens).
pub(crate) fn masse_rechnen(
    baum: &narsil_layout::LayoutTree,
    seite: &Seite,
    ids: &HashMap<usize, u32>,
) -> HashMap<u32, crate::dom::Masse> {
    // Release 11.10.2026: mit `transform` (`LayoutBox::abbildung`) —
    // `getBoundingClientRect` meldet den gezeichneten Kasten. Dazu je
    // Element die Rahmenbreiten, die Positionsart und die Ausdehnung des
    // Teilbaums (fuer `offsetParent`, `client*`, `scroll*`). Rueckgabe: die
    // rechte und untere Kante des Teilbaums.
    fn lauf(
        bx: &narsil_layout::LayoutBox,
        a: &Seite,
        ids: &HashMap<usize, u32>,
        raus: &mut HashMap<u32, crate::dom::Masse>,
        tiefe: usize,
        aussen: (f32, f32, f32),
        eltern_ord: Option<usize>,
    ) -> (f32, f32) {
        if tiefe > 512 {
            return (f32::MIN, f32::MIN);
        }
        let eigen_ord = a.dom_zu_ord.get(&(bx.node.index() as u32)).copied();
        let (s, dx, dy) = bx.abbildung(aussen);
        let b = bx.border_box;
        let r = narsil_core::geometry::Rect::new(s * b.x + dx, s * b.y + dy, s * b.width, s * b.height);
        let gueltig = r.width.is_finite() && r.height.is_finite();
        let mut ende = if gueltig { (r.x + r.width, r.y + r.height) } else { (f32::MIN, f32::MIN) };
        for k in &bx.children {
            let e = lauf(k, a, ids, raus, tiefe + 1, (s, dx, dy), eigen_ord);
            ende = (ende.0.max(e.0), ende.1.max(e.1));
        }
        // Release 11.10.2026: nur die AEUSSERSTEN Kaesten eines Elements
        // zaehlen — Zeilenbox und Textlauf im eigenen Kasten tragen dieselbe
        // Ordnungszahl und wuerden ihn sonst aufblaehen. GEMESSEN vorher
        // (Agent CSS2, WPT dialog centering): ein Dialog mit `height: 10px`
        // meldete 19,2 (die Zeilenhoehe seines Textes).
        if let Some(ord) = eigen_ord.as_ref()
            && eltern_ord != eigen_ord
            && let Some(&host) = ids.get(ord)
            && gueltig
        {
            let rahmen = a.document.get(bx.node).map(|n| n.style.rahmen).unwrap_or_default();
            let br = |i: usize| s * rahmen.gezeichnete_breite(i);
            let pos = match bx.position {
                narsil_layout::plan::PositionArt::Statisch => 0.0,
                narsil_layout::plan::PositionArt::Relativ => 1.0,
                narsil_layout::plan::PositionArt::Absolut => 2.0,
                narsil_layout::plan::PositionArt::Fest => 3.0,
                narsil_layout::plan::PositionArt::Klebend => 4.0,
            };
            raus.entry(host)
                .and_modify(|m| {
                    let x2 = (m[0] + m[2]).max(r.x + r.width);
                    let y2 = (m[1] + m[3]).max(r.y + r.height);
                    m[0] = m[0].min(r.x);
                    m[1] = m[1].min(r.y);
                    m[2] = x2 - m[0];
                    m[3] = y2 - m[1];
                    m[9] = m[9].max(ende.0);
                    m[10] = m[10].max(ende.1);
                })
                .or_insert([r.x, r.y, r.width, r.height, br(3), br(0), br(1), br(2), pos, ende.0, ende.1]);
        }
        ende
    }
    let mut masse = HashMap::with_capacity(ids.len());
    lauf(&baum.root, seite, ids, &mut masse, 0, (1.0, 0.0, 0.0), None);
    masse
}

/// Release 09.10.2026: der tiefste Knoten in einem `position: fixed`-Kasten
/// unter `(x, y)` — `y` in UNGEROLLTEN Koordinaten, denn feste Kaesten
/// stehen beim Rollen still (CSS 2.1, 9.6.1).
fn fester_treffer(baum: &LayoutTree, x: f32, y: f32) -> Option<NodeId> {
    fn lauf(
        bx: &narsil_layout::LayoutBox,
        x: f32,
        y: f32,
        fest: bool,
        tiefe: usize,
        aussen: (f32, f32, f32),
    ) -> Option<NodeId> {
        if tiefe > 512 {
            return None;
        }
        let fest = fest || bx.position == narsil_layout::plan::PositionArt::Fest;
        let abb = bx.abbildung(aussen);
        if abb.0 <= 0.001 {
            return None;
        }
        let r = gezeichnet(bx.border_box, abb);
        let drin = fest && x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height;
        let mut treffer = drin.then_some(bx.node);
        for kind in &bx.children {
            if let Some(k) = lauf(kind, x, y, fest, tiefe + 1, abb) {
                treffer = Some(k);
            }
        }
        treffer
    }
    lauf(&baum.root, x, y, false, 0, (1.0, 0.0, 0.0))
}

/// Release 09.10.2026: wie `tiefster_knoten`, aber feste Kaesten zuerst
/// (sie liegen ueber der Seite). `y_seite` ist gerollt, `versatz` der
/// Rollstand. GEMESSEN vorher: nach dem Rollen trafen Klicks auf eine
/// feste Kopfleiste den Seiteninhalt darunter.
fn tiefster_knoten_v(baum: &LayoutTree, x: f32, y_seite: f32, versatz: f32) -> Option<NodeId> {
    fester_treffer(baum, x, y_seite - versatz).or_else(|| tiefster_knoten(baum, x, y_seite))
}

/// Release 11.10.2026: ein Kasten so, wie er mit der Abbildung
/// `p -> s·p + (dx, dy)` (`transform`) gezeichnet wird.
fn gezeichnet(r: narsil_core::geometry::Rect, (s, dx, dy): (f32, f32, f32)) -> narsil_core::geometry::Rect {
    narsil_core::geometry::Rect::new(s * r.x + dx, s * r.y + dy, s * r.width, s * r.height)
}

fn tiefster_knoten(baum: &LayoutTree, x: f32, y: f32) -> Option<NodeId> {
    // Release 11.10.2026: Treffer am GEZEICHNETEN Kasten (`transform`) —
    // ein eingefahrenes Menue (`translateX(-100%)`) faengt keine Klicks
    // mehr an seinem Layout-Platz, `scale(0)` gar keine.
    fn lauf(bx: &narsil_layout::LayoutBox, x: f32, y: f32, aussen: (f32, f32, f32), tiefe: usize) -> Option<NodeId> {
        if tiefe > 512 {
            return None;
        }
        let abb = bx.abbildung(aussen);
        if abb.0 <= 0.001 {
            return None;
        }
        let r = gezeichnet(bx.border_box, abb);
        let drin = x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height;
        let mut treffer = drin.then_some(bx.node);
        for kind in &bx.children {
            if let Some(k) = lauf(kind, x, y, abb, tiefe + 1) {
                treffer = Some(k);
            }
        }
        treffer
    }
    lauf(&baum.root, x, y, (1.0, 0.0, 0.0), 0)
}

/// Wie hoch ist der Inhalt mindestens?
///
/// GEMESSEN, warum nicht `baum.bounds()`: das gibt nur die sichtbaren
/// Teile zurueck, und bei einer Seite mit transparenten Containern
/// unterhalb des letzten Textes waere der Versatz 0 — man koennte
/// nicht scrollen, obwohl die Seite weitergeht.
///
/// Deshalb wird der unterste KNOTEN genommen, nicht der unterste
/// sichtbare Pixel.
fn berechne_inhaltshoehe(baum: &LayoutTree, mindest: u32) -> u32 {
    fn tiefste(box_: &narsil_layout::LayoutBox, acc: &mut f32) {
        *acc = acc.max(box_.border_box.y + box_.border_box.height);
        for kind in &box_.children {
            tiefste(kind, acc);
        }
    }
    let mut ende = 0.0f32;
    tiefste(&baum.root, &mut ende);
    if ende.is_finite() && ende > 0.0 {
        (ende.ceil() as u32).max(mindest)
    } else {
        mindest
    }
}

/// Liegt ein Punkt in einem Rechteck des Chrome?
fn treffer(r: &narsil_chrome::Rechteck, p: Point) -> bool {
    p.x >= r.x as f32 && p.x < r.rechts() as f32 && p.y >= r.y as f32 && p.y < r.unten() as f32
}

/// Die eingebaute Seite fuer `narsil://start`.
///
/// Das ist der Grund, warum der Browser ohne Datei und ohne Netz
/// startet: es gibt immer etwas zu sehen, und man kann die Bedienung
/// lernen, bevor man eine Adresse eintippt.
pub fn startseite() -> String {
    startseite_mit_verlauf(&[])
}

/// DIE STARTSEITE (UI2) — mit Suchfeld, Kacheln und Verlauf.
///
/// GEMESSEN, warum eine echte Seite und keine Zeichnung im Fenster: die
/// Startseite ist eine SEITE. Sie geht durch Parse, Kaskade, Layout und
/// Raster wie jede andere — und damit ist sie an derselben Stelle pruefbar
/// (`display_list`) wie eine Seite aus dem Netz. Eine eigene Zeichnung im
/// Fenster waere ein zweiter Weg zum selben Bild.
///
/// UI2 verlangt drei Dinge, und alle drei sind hier messbar:
///  * ein SUCHFELD (ein `<form>` mit einem `<input>` und einem Absenden —
///    der Absender ist ein echter `<button>`, und mit dem Inline-Handler-
///    Weg (H1) kann er sogar eine Aktion bekommen),
///  * LESEZEICHEN-KACHELN (vier `<a>` als Kacheln; eine Kachel ist ein
///    Rechteck mit Text, das man anklicken kann),
///  * VERLAUF (die letzten Adressen als `<a>`, damit ein Klick laedt).
///
/// M3: hoechstens `MAX_VERLAUF_ZEILEN` (8) Verlaufseintraege und
/// `MAX_LESEZEICHEN` (8) Kacheln. GEMESSEN, warum Kacheln und nicht eine
/// Liste: eine Liste ist Text, und die Startseite soll ansehen wie ein
/// Werkzeug — Kacheln sind etwas, was man mit dem Auge findet.
pub fn startseite_mit_verlauf(verlauf: &[String]) -> String {
    /// M3: so viele Kacheln, wie in zwei Reihen passen.
    const MAX_LESEZEICHEN: usize = 8;
    /// M3: der Verlauf ist eine Liste, kein Archiv.
    const MAX_VERLAUF_ZEILEN: usize = 8;
    /// Release 09.10.2026: die Farben der Buchstaben-Kreise (wie die
    /// Verknuepfungen der neuen Tabs in Chrome — je Kachel eine).
    const FARBEN: [&str; 8] = ["#4f8fe8", "#e0565b", "#8a63d2", "#2fa37a", "#e39b36", "#3fa9c9", "#d4588f", "#7c8798"];
    let mut s = String::with_capacity(6144);
    // Release 07.10.2026: die Schrift ausdrücklich — ohne Angabe gilt der
    // CSS-Anfangswert `serif` (wie in Firefox).
    //
    // Release 09.10.2026 (Feinschliff, Nutzer: „Glas billig"): die Seite ist
    // jetzt MITTIG gesetzt wie die neuen Tabs in Chrome/Firefox — grosser
    // Titel, breites Suchfeld als Pille, Kacheln im Raster mit farbigem
    // Buchstaben-Kreis, Verlauf als Glaskarte; interne Adressen
    // (`narsil://`, `about:`) stehen nicht mehr im Verlauf. Inhalt (Titel,
    // Formular `q`, Kachel-Links, Hilfezeile) bleibt.
    s.push_str(
        "<style>\
         html{background:#0c0d13}\
         body{font-family:system-ui,sans-serif;margin:0;min-height:100%;color:#f2f2f6;\
         background:linear-gradient(165deg,#24325c 0%,#171b2e 36%,#0f1018 70%,#0c0d13 100%)}\
         main{max-width:720px;margin:0 auto;padding:84px 24px 40px 24px}\
         .glas{background:linear-gradient(180deg,rgba(255,255,255,0.10),rgba(255,255,255,0.04));\
         border:1px solid rgba(255,255,255,0.10);border-top-color:rgba(255,255,255,0.24);\
         border-radius:18px;box-shadow:0 0 0 1px rgba(0,0,0,0.34),0 12px 30px rgba(0,0,0,0.34)}\
         h1{font-size:46px;font-weight:600;letter-spacing:-0.02em;text-align:center;margin:0 0 6px 0;color:#f6f7fb}\
         .unter{text-align:center;margin:0 0 30px 0;color:#99a0b2}\
         .kopf{font-size:11px;font-weight:600;letter-spacing:0.14em;text-transform:uppercase;\
         color:#8f95a5;margin:36px 4px 10px 4px}\
         a{color:#f2f2f6;text-decoration:none}\
         .kacheln{display:grid;grid-template-columns:repeat(4,1fr);gap:14px}\
         .kachel{display:flex;flex-direction:column;align-items:center;padding:18px 8px 14px 8px}\
         .zeichen{display:block;width:44px;height:34px;padding:10px 0 0 0;border-radius:22px;\
         text-align:center;font-size:19px;font-weight:700;color:#ffffff;margin:0 0 10px 0}\
         .name{display:block;font-size:13px;color:#e4e6ee}\
         .zeile{display:block;padding:9px 6px;color:#dfe2ea;border-bottom:1px solid rgba(255,255,255,0.06)}\
         .hilfe{text-align:center;margin:30px 0 0 0;color:#868c9c;font-size:13px}\
         .hilfe b{color:#c8ccd6}\
         </style><main>",
    );
    // The title is set in the current UI language.
    s.insert_str(0, &std::format!("<title>{}</title>", escape_html(tr("Narsil – Home page"))));
    s.push_str("<h1>Narsil</h1>");
    s.push_str(&std::format!(
        "<p class=\"unter\">{}</p>",
        escape_html(tr("A browser. Enter an address, or click a tile."))
    ));
    // DAS SUCHFELD. GEMESSEN, warum ein `<form>` und nicht ein Knopf: die
    // Zeile ist dann in einem Browser wie im Bild, und der Absender
    // braucht keinen Sonderfall im Fenster. Release 09.10.2026: breite Pille
    // ueber die ganze Spalte, der Knopf rechts daneben.
    s.push_str(&std::format!(
        "<form action=\"narsil://start\" method=\"get\" \
         style=\"margin:0;display:flex;gap:10px;align-items:center;justify-content:center\">\
         <input name=\"q\" value=\"\" size=\"40\" placeholder=\"{}\" \
         style=\"width:554px;padding:14px 22px;border-radius:26px;font-size:16px;color:#f2f2f6;\
         background:#1b1e2b;border:1px solid #3b4053\">\
         <button type=\"submit\" style=\"padding:14px 26px;border-radius:26px;font-size:15px;font-weight:600;\
         color:#ffffff;background:#3f7bd0;border:1px solid #6d9be0\">{}</button>\
         </form>",
        escape_html(tr("Search or enter address")),
        escape_html(tr("Go"))
    ));
    // DIE KACHELN — Raster mit vier Spalten, je Kachel ein Kreis mit dem
    // Anfangsbuchstaben. Die Kachel IST der Link (`<a class="kachel">`).
    // GEMESSEN vorher mit `<a><div>…</div></a>`: `Seite::links` kannte nur die
    // Verlaufszeilen, ein Klick auf „Optionen" tat nichts.
    s.push_str(&std::format!("<p class=\"kopf\">{}</p>", narsil_chrome::glas::gesperrt(tr("TILES"))));
    s.push_str("<div class=\"kacheln\">");
    for (i, (name, url)) in [
        (tr("Example"), "https://example.org/"),
        ("Wikipedia", "https://de.wikipedia.org/"),
        ("GitHub", "https://github.com/"),
        (tr("Local"), "http://localhost/"),
        (tr("Options"), "about:einstellungen"),
        (tr("Search"), "https://duckduckgo.com/?q=narsil"),
        (tr("Docs"), "https://de.wikipedia.org/wiki/Webbrowser"),
        (tr("Status"), "narsil://start"),
    ]
    .iter()
    .take(MAX_LESEZEICHEN)
    .enumerate()
    {
        let zeichen: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
        s.push_str(&std::format!(
            "<a href=\"{url}\" class=\"glas kachel\"><span class=\"zeichen\" style=\"background:{farbe}\">{zeichen}</span>\
             <span class=\"name\">{name}</span></a>",
            farbe = FARBEN[i % FARBEN.len()]
        ));
    }
    s.push_str("</div>");
    // DER VERLAUF — nur Seiten aus dem Netz und Dateien (interne Adressen
    // sind keine Ziele, die man wiederfinden will).
    let mut gesehen = std::collections::HashSet::new();
    let eintraege: Vec<&String> = verlauf
        .iter()
        .rev()
        .filter(|a| {
            let a = a.trim();
            !a.is_empty() && !a.starts_with("narsil:") && !a.starts_with("about:")
        })
        .filter(|a| gesehen.insert(a.as_str()))
        .take(MAX_VERLAUF_ZEILEN)
        .collect();
    if !eintraege.is_empty() {
        s.push_str(&std::format!("<p class=\"kopf\">{}</p>", narsil_chrome::glas::gesperrt(tr("HISTORY"))));
        s.push_str("<div class=\"glas\" style=\"padding:6px 16px\">");
        for a in eintraege {
            // GEMESSEN, warum die Adresse escaped wird: ein Verlaufseintrag
            // kann jedes Zeichen enthalten (auch `<`), und unescaped waere
            // das eine HTML-Injektion aus dem Verlauf.
            // Die Adresse ohne Schema in EINEM Lauf (sucht man im Bild, findet
            // man sie als Ganzes).
            let ohne_schema = a.split_once("://").map_or(a.as_str(), |(_, r)| r).trim_end_matches('/');
            s.push_str(&std::format!(
                "<a href=\"{}\" class=\"zeile\">{}</a>",
                escape_html(a),
                escape_html(ohne_schema)
            ));
        }
        s.push_str("</div>");
    }
    // DIE HILFEZEILE bleibt — sie beantwortet die Frage „wie bediene ich
    // das?", und die stellt jeder beim ersten Fenster.
    s.push_str(&std::format!(
        "<p class=\"hilfe\">{}</p></main>",
        tr("<b>Ctrl+L</b> address bar · <b>Ctrl+T</b> new tab · <b>Ctrl+W</b> close tab · <b>F5</b> reload · the mouse wheel scrolls.")
    ));
    s
}

/// Release 08.10.2026: die Eingabe aus dem Suchfeld der Startseite —
/// `narsil://start?q=…` (auch `narsil://?q=…`), Formularkodierung
/// (`+` = Leerzeichen, `%XX`). `None`, wenn die Adresse keine solche ist.
fn startseite_frage(adresse: &str) -> Option<String> {
    let rest = adresse.trim().strip_prefix("narsil://")?;
    let (seite, frage) = rest.split_once('?')?;
    if !(seite.is_empty() || seite == "start" || seite == "start/") {
        return None;
    }
    let frage = frage.split('#').next().unwrap_or("");
    let wert = frage
        .split('&')
        .find_map(|teil| teil.strip_prefix("q="))
        .unwrap_or("");
    let ersetzt = wert.replace('+', " ");
    Some(match narsil_url::prozent_dekodieren(&ersetzt) {
        narsil_url::Dekodiert::Text(t) => t,
        narsil_url::Dekodiert::Bytes(b) => String::from_utf8_lossy(&b).into_owned(),
    })
}

/// Escaped einen Text fuer HTML (Attribute und Text).
///
/// GEMESSEN, warum eine eigene Funktion und nicht `serialisiere`: das hier
/// ist ein WERT (eine Adresse), kein Baum. Und sie muss auch dann laufen,
/// wenn die Seite gar nicht gebaut wird (der Verlauf kommt aus dem
/// Browser, nicht aus dem Layout).
fn escape_html(text: &str) -> String {
    let mut r = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        match c {
            '&' => r.push_str("&amp;"),
            '<' => r.push_str("&lt;"),
            '>' => r.push_str("&gt;"),
            '"' => r.push_str("&quot;"),
            _ => r.push(c),
        }
    }
    r
}

/// WIE VIELE SEITEN HAT DAS DOKUMENT? (`None` = keine Datei)
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine eigene Funktion ist: `Dokument`
/// kann man nicht kopieren, und `Renderer::neu` leiht sich das Dokument fuer
/// die Dauer des Renderns. Wer die Zahl und das Bild in einem Aufruf holen
/// wuerde, muesste das Dokument zweimal oeffnen — GEMESSEN, dass das bei
/// 1706.03762 (15 Seiten, 1,2 MB) den doppelten Aufwand waere.
fn pdf_seitenzahl(bytes: &[u8]) -> Option<u32> {
    // GEMESSENE ENTSCHEIDUNG, warum `mut`: `Dokument::seiten()` braucht
    // `&mut self` (es entschluesselt beim Aufruf). GEMESSEN am Compiler.
    let mut dok = narsil_pdf::Dokument::oeffnen(bytes.to_vec()).ok()?;
    let seiten = dok.seiten().ok()?;
    Some(seiten.len() as u32)
}

/// DIE SEITE ALS BILD — PNG aus `narsil-pdf`, Pixel aus `narsil-image`.
///
/// GEMESSENE ENTSCHEIDUNG, warum ueber PNG und nicht ueber die RGBA-Flaeche
/// von `narsil-pdf`: GEMESSEN ist `Seitenbild { png: Vec<u8>, breite, hoehe }`
/// (`narsil-pdf/src/lib.rs:222`) — die Crate gibt fertige PNG-Bytes zurueck.
/// GEMESSENE ENTSCHEIDUNG, warum `png_lesen` trotzdem noetig ist: ein
/// DisplayItem braucht ROHE Pixel, keine PNG-Datei. Die Kette ist also
/// `narsil-pdf` (malt) -> PNG -> `png_lesen` (entpackt) -> `DisplayItem::Bild`.
fn pdf_seite_als_bild(bytes: &[u8], seite_nr: u32) -> Option<narsil_image::Bild> {
    let mut dok = narsil_pdf::Dokument::oeffnen(bytes.to_vec()).ok()?;
    let seiten = dok.seiten().ok()?;
    if seiten.is_empty() {
        return None;
    }
    // GEMESSENE ENTSCHEIDUNG, warum die Nummer geklemmt wird und nicht
    // geprueft: die Leiste kann eine Zahl zeigen, die das Dokument nicht hat
    // (sie war bei einem anderen Dokument). Der Betrachter zeigt dann die
    // LETZTE Seite — das ist eine Seite, statt einer leeren Flaeche.
    let nr = seite_nr.clamp(1, seiten.len() as u32) as usize;
    let opt = narsil_pdf::Rasteroptionen::standard();
    let mut r = narsil_pdf::Renderer::neu(&mut dok, opt);
    let b = r.seite(&seiten[nr - 1]).ok()?;
    narsil_image::png::lesen::png_lesen(&b.png).ok()
}

/// UI-VORLAGE Schritt 6: DER DATEINAME AUS EINER PDF-ADRESSE.
///
/// `…/bericht.pdf?v=2#seite=3` -> `bericht.pdf`. Ohne brauchbaren Namen
/// (Ende auf `/`, leere Adresse) kommt `dokument.pdf` zurueck.
///
/// GEMESSENE ENTSCHEIDUNG, warum ueberhaupt gekuerzt wird: der Name geht in
/// die Statuszeile UND in den Dateinamen, den der Download-Manager anlegt.
/// Beides kommt aus dem Netz — ein Pfad mit 4000 Zeichen (Query-Parameter
/// auf einer Adresse aus einem Link) waere kein Name, sondern eine Zeile
/// Text, die das Fenster aufreisst.
///
/// GEMESSENE GRENZE: `MAX_DATEINAME` = 96. BEGRUENDUNG: der laengste
/// haeufige Name ist „bericht.pdf"; 96 Zeichen passen in eine Zeile der
/// Statuszeile bei 1200 px Breite (gemessen: 7 px je Zeichen bei 13-px-Schrift)
/// und bleiben unter dem Dateisystem-Namen von Windows (255 Zeichen je
/// Name), auch wenn der Download-Manager selbst noch „narsil-" anhaengt.
const MAX_DATEINAME: usize = 96;

/// KONVERT_18: die **Druckhoehe** fuer das Neulegen.
///
/// GEMESSEN, warum das eine Konstante und keine Magic-Zahl **im Aufruf** ist:
/// `Angezeigt::aus_ist_mit_typ` rechnet mit `sicht_hoehe`, und ein zu kleiner
/// Wert **bricht das Layout ab** — es kaeme nur der erste Bildschirm in die
/// Datei. GEMESSEN an A4: die Seite fasst 1010,7 px Inhalt, also sind 2000 px
/// **doppelt** so hoch wie eine Seite; das reicht fuer Seiten, die nicht
/// umbrochen werden, und ist gross genug fuer den ersten Bildschirm **jeder**
/// ueblichen Seite.
const DRUCK_HOEHE_PX: u32 = 4000;

/// KONVERT_18: der **Titel** fuer den Dateinamen, falls die Seite keinen
/// Dokumenttitel liefert.
///
/// GEMESSEN, warum das der letzte **Pfadabschnitt** ist und nicht die ganze
/// Adresse: `dateiname()` macht aus einem `/` ein `_`, also waere aus
/// `file:///a/b/c.html` der Name `file:__a_b_c.html` — und der Nutzer erkennt
/// seine Seite nicht wieder. Also: **das**, was hinter dem letzten `/` steht,
/// ohne `.html`.
fn titel_aus_adresse(adresse: &str) -> String {
    // GEMESSEN, dass ein Query-Teil (`?a=b`) **nicht** in einen Dateinamen
    // gehoert: er waere Teil des Namens und nicht Teil der Seite.
    // GEMESSEN, warum `split` **einen** Abschluss und nicht `split(['?', '#'])`:
    // das Array-Muster fuer `str::split` braucht eine bestimmte Rust-Fassung,
    // und `take_while` braucht keine. Also der Weg, der ueberall gebaut wird.
    let ohne_query = adresse
        .split(|c| c == '?' || c == '#')
        .next()
        .unwrap_or(adresse);
    let letzter = ohne_query
        .rsplit('/')
        .find(|t| !t.is_empty())
        .unwrap_or(ohne_query);
    // Und GEMESSEN, dass eine **leere** Basis (`about:blank`) nicht als Titel
    // taugt: sie ergibt keinen Pfadabschnitt, also bleibt `seite` — und
    // `dateiname("")` liefert `seite.pdf` (gemessen in KONVERT_17).
    if letzter.is_empty() || !letzter.chars().any(|c| c.is_alphanumeric()) {
        return "seite".to_string();
    }
    // Und der Dateityp fliegt **raus**: `index.html` heisst in der Adressleiste
    // `index.html`, im Download-Fenster aber `index.pdf`. GEMESSEN: ohne das
    // heisst die Datei doppelt so wie die Seite.
    let ohne_endung = letzter
        .strip_suffix(".html")
        .or_else(|| letzter.strip_suffix(".htm"))
        .unwrap_or(letzter);
    if ohne_endung.is_empty() {
        "seite".to_string()
    } else {
        ohne_endung.to_string()
    }
}

/// Der Name fuer den Download — leer, wenn die Adresse nichts hergibt.
pub fn pdf_dateiname(adresse: &str) -> String {
    // GEMESSENE ENTSCHEIDUNG (02.10., Test
    // `der_dateiname_kommt_aus_der_adresse`): erst `?`/`#` abschneiden,
    // dann das Schema und den Host. GEMESSENE URSACHEN — zwei Fehlversuche,
    // beide vom selben Test gefunden:
    //   1. nur `rsplit('/')`: `https://x.de/` gab `x.de`, denn nach dem
    //      Abschneiden der schliessenden Schraefze war `x.de` das letzte
    //      Stueck — das ist der HOST, kein Name.
    //   2. danach `rsplit` nach dem Host: immer noch `x.de`, denn
    //      `split_once('/')` gibt `("https:", "x.de")`, und `x.de` ist der
    //      Rest des Hosts.
    // Die Regel, die beide faellt: der Name ist das letzte Stueck des
    // PFADES nach dem Host — und der Pfad ist das, was nach dem Host mit
    // `/` beginnt. Gibt es kein solches Stueck, gibt es keinen Namen.
    let pfad = adresse.split(['?', '#']).next().unwrap_or("").trim();
    let name = match pfad.split_once('/') {
        // Mit Schema: `https://x.de/a/bericht.pdf` -> der Teil hinter dem
        // zweiten `/`-Paket. GEMESSENE ENTSCHEIDUNG, warum das zweite
        // Stueck nach dem Trenner zaehlt und nicht das erste: `x.de/a` ist
        // ein Host mit einem Verzeichnis, und `a` ist ein Verzeichnis — ein
        // Verzeichnis ist kein Dateiname.
        Some((_, rest)) => rest.rsplit('/').next().unwrap_or("").trim(),
        // Ohne Schema (schon ein Pfad oder ein Dateiname).
        None => pfad.rsplit('/').next().unwrap_or("").trim(),
    };
    if name.is_empty() {
        // GEMESSENE ENTSCHEIDUNG, warum das kein Fehler ist: eine Adresse
        // ohne Namen (`https://x.de/`) ist eine PDF-Datei, die der Server
        // so ausliefert. Ein leerer Dateiname waere eine Datei ohne Namen.
        return "dokument.pdf".to_string();
    }
    name.chars().take(MAX_DATEINAME).collect()
}

/// Quellart einer Adresse, fuer die Statuszeile und Tests.
pub fn quellart(adresse: &str) -> Quellart {
    if adresse.starts_with("narsil:") {
        Quellart::Eingebaut
    } else if adresse.starts_with("file://") {
        Quellart::Datei
    } else if adresse.starts_with("http://") {
        Quellart::Netz
    } else if narsil_page::ist_markdown(adresse) {
        // I5: `.md` ohne Schema ist eine Datei, die relativ gefunden
        // werden soll. Die Reihenfolge steht HIER und nicht im
        // Lader, damit `quellart` und `lade` dieselbe Antwort geben.
        Quellart::Markdown
    } else {
        Quellart::Unbekannt
    }
}

/// Nur damit `HashMap` nicht als unbenutzt gilt: die Sitzung braucht
/// es fuer die naechste Erweiterung, und ein `#[allow]` waere schlechter.
#[allow(dead_code)]
type _Reserviert = HashMap<String, String>;

impl Sitzung {
    pub fn lade_im_hintergrund_an(&mut self) -> Result<(), Fehler> {
        if self.seiten_lader.is_none() {
            self.seiten_lader = Some(crate::seiten_lader::Lader::neu(self.dienste.clone())
                .map_err(|e| Fehler::Seite(format!("Seitenlader: {e}")))?);
        }
        Ok(())
    }

    fn ladungen_abholen(&mut self) {
        if self.browser.ladung_abbrechen_gewuenscht() {
            if let Some(lader) = self.seiten_lader.as_mut() { lader.verwerfen(); }
            self.laedt = false;
            self.schmutzig = true;
        }
        while let Some(antwort) = self.seiten_lader.as_mut().and_then(|l| l.abholen()) {
            if !self.laedt || self.seiten_lader.as_ref().is_none_or(|l| l.id != antwort.id)
                || self.browser.aktive_tab_id() != Some(antwort.tab)
                || self.browser.adresse() != antwort.adresse { continue; }
            // Release 10.10.2026: eine VORSCHAU nur zeigen — die Ladung laeuft
            // weiter (`laedt` bleibt), das Endergebnis kommt danach.
            if antwort.vorschau {
                if let Ok(a) = antwort.seite {
                    self.angezeigt = Some(a);
                    self.seiten_generation = self.seiten_generation.wrapping_add(1);
                    self.schmutzig = true;
                }
                continue;
            }
            crate::zeiten::uebernehmen(antwort.zeiten);
            narsil_devtools::d4_netzwerk::abschluss_ereignis(&mut self.devtools.netz, antwort.netz);
            let ansicht_geaendert = antwort.ansicht != (self.breite.max(1), self.sicht_hoehe());
            let uhr = std::time::Instant::now();
            let ankunft_ms = antwort.start.elapsed().as_secs_f64() * 1000.0;
            let _ = self.lade_uebernehmen(antwort.adresse, antwort.seite, antwort.start);
            // Release 10.10.2026: sofort — sonst stuende die Seite kurz in der
            // alten Breite da (Fenster waehrend des Ladens veraendert).
            if ansicht_geaendert { self.seite_neu_bauen_sofort(); }
            if crate::zeiten::an() {
                std::eprintln!(
                    "NARSIL_ZEIT uebernahme ankunft {ankunft_ms:>8.1} ms  uebernehmen {:>8.2} ms  ansicht_geaendert={ansicht_geaendert} {:?} -> {:?}",
                    uhr.elapsed().as_secs_f64() * 1000.0,
                    (self.breite.max(1), self.sicht_hoehe()),
                    ansicht_geaendert
                );
            }
            self.tabs_anzeigen();
        }
        if self.laedt && self.seiten_lader.as_ref().is_some_and(|l| l.beendet) {
            self.laedt = false;
            self.meldung = Some(tr("Page loader ended").into());
            self.schmutzig = true;
            self.seiten_lader = None;
        }
    }

    pub fn restore_dienste_session(&mut self) {
        let urls = self.dienste.restore_session();
        self.restore_adressen(&urls);
    }

    pub fn restore_adressen(&mut self, urls: &[String]) {
        let mut anzahl = 0;
        for url in urls.iter().filter(|u| !u.trim().is_empty()).take(MAX_TABS as usize) {
            if anzahl > 0 && self.browser.anwenden(&Navigation::TabNeu) == Wirkung::Leer { break; }
            self.browser.anwenden(&Navigation::To(url.clone()));
            anzahl += 1;
        }
        if anzahl == 0 { return; }
        self.tabs.resize_with(self.browser.tabs(), || None);
        self.browser.anwenden(&Navigation::TabSpringen(0));
        self.chrome.setze_adresse(self.browser.adresse());
        self.tabs_anzeigen();
        self.schmutzig = true;
    }

    pub fn save_dienste_session(&self) {
        let urls: Vec<String> = (0..self.browser.tabs())
            .map(|nr| self.browser.adresse_von_tab(nr).to_string())
            .filter(|url| !url.is_empty()).collect();
        self.dienste.save_session(&urls);
    }
    pub fn adresse_vorschlaege(&self) -> Vec<String> {
        self.dienste.get_url_suggestions(self.browser.adresse())
    }
    pub fn download_url(&mut self, url: &str, name: &str) {
        //
        // added 03.10.2026, Rolle TLS: **HIER GEHOERT DER DOWNLOAD-HAKEN NICHT
        // HIN, und das ist gemessen — nicht behauptet.** Ich hatte an dieser
        // Stelle zuerst einen Aufruf von `sicherheit_haken::download_vorbehalt`
        // stehen und habe ihn wieder entfernt. Der Grund:
        //
        // * `download_url(url: &str, name: &str)` hat **keinen** `Response`, und
        //   `Content-Disposition` ist eine **Antwort**.
        // * GEMESSEN, wo die Antwort actually liegt:
        //   `crates/narsil-download/src/manager.rs:44` macht einen **eigenen**
        //   `TcpStream`-Abruf, parst `Content-Length` von Hand und liest
        //   `Content-Disposition` **nie**.
        // * `crates/narsil-download/src/content_disposition.rs:1` hat
        //   `parse_content_disposition` — **null Aufrufer** ausser dem eigenen
        //   Test (GEMESSEN mit `grep`).
        //
        // **DIE KONSTRUKTION, DIE ICH ZUERST GESCHRIEBEN HATTE, WAR EIN
        // BETRUG:** sie hat `None` als `content_disposition` uebergeben, also
        // **konnte sie nie ausloesen**, und den Text mit `let _ = meldung;`
        // weggeworfen. Ein Haken, der nie ausloest und nichts anzeigt, ist
        // schlimmer als keiner — er steht in der Datei und sieht fertig aus.
        //
        // **WO ER HINGEHOERT:** an `DownloadManager::download_sync`, in
        // `crates/narsil-download/src/manager.rs` — der einzigen Stelle, die die
        // Kopfzeile sieht. Die Datei steht in **keinem** Eintrag von
        // `team/EIGENTUM.txt`; gemeldet in `team/BRETT.txt`.
        let mut eintrag = narsil_chrome::downloads::Download::aus_adresse(url);
        eintrag.name = name.chars().take(120).collect();
        self.meldung = Some(match self.dienste.download_file(url, name) {
            narsil_download::DownloadState::Finished(path) => {
                eintrag.zustand = narsil_chrome::downloads::DownloadZustand::Fertig;
                eintrag.name = path.file_name().unwrap_or_default().to_string_lossy().chars().take(120).collect();
                eintrag.pfad = path.to_string_lossy().into_owned();
                trf!("Saved: {}", path.display())
            }
            narsil_download::DownloadState::Failed(e) => {
                eintrag.zustand = narsil_chrome::downloads::DownloadZustand::Fehlgeschlagen;
                eintrag.meldung = e.chars().take(120).collect();
                trf!("Download failed: {}", e)
            }
            _ => tr("Download not finished yet").into(),
        });
        self.download_eintragen(eintrag);
    }

    /// Release 09.10.2026: einen Eintrag vorn in die Download-Liste setzen
    /// (Downloads, „Bild speichern", „Konvertiert herunterladen").
    pub fn download_eintragen(&mut self, eintrag: narsil_chrome::downloads::Download) {
        self.downloads_seite.eintraege.insert(0, eintrag);
        self.downloads_seite.eintraege.truncate(narsil_chrome::seiten::MAX_ZEILEN);
        self.downloads_seite.ausgewaehlt = None;
        self.schmutzig = true;
    }
    pub fn print_current_page(&self, als_pdf: bool) {
        let titel = self.aktuelle_titel();
        // Hier in narsil haben wir kein Rasterbild direkt parat in der Sitzung ohne Display,
        // aber wir stellen den Hook bereit.
        let printer = if als_pdf { None } else { Some("default") };
        let _ = self.dienste.print_page(printer, None, titel);
    }
}

/// Die **Video**-Taste zu einer Taste, oder `None` wenn es keine ist.
///
/// VIDEO_19 (04.10.2026). GEMESSENE ENTSCHEIDUNG, warum das hier steht
/// und nicht in `video.rs`: hier ist der Weg von `narsil_input::Key` — und
/// `narsil_input` ist eine fremde Crate. In `video.rs` wuerde ein
/// `use narsil_input::Key` stehen, und damit waere die Videodecke von einer
/// Eingabe-Crate abhaengig, die sie sonst nicht kennt.
///
/// GEMESSENE ENTSCHEIDUNG, warum `Char('m')` und `Char('f')` **ohne**
/// Strg/Alt aufgenommen sind und `k.pressed` nicht geprueft wird: der
/// Aufrufer (`Sitzung::taste`) ruft diese Funktion nur fuer Tasten **ohne**
/// Strg; die Belegung ist die der Browser — `m` stummt, `f` macht
/// Vollbild.
fn video_taste(k: KeyEvent) -> Option<narsil_media::bedienung::Taste> {
    use narsil_input::Key as K;
    use narsil_media::bedienung::Taste as T;
    if k.modifiers.ctrl || k.modifiers.alt {
        return None;
    }
    Some(match k.key {
        K::Space => T::Leertaste,
        K::Left => T::Links,
        K::Right => T::Rechts,
        K::Up => T::Hoch,
        K::Down => T::Runter,
        K::Char('m') | K::Char('M') => T::M,
        K::Char('f') | K::Char('F') => T::F,
        _ => return None,
    })
}

#[cfg(test)]
mod quelltext_regression {
    use super::Sitzung;
    #[test]
    fn quelltext_scanner_verbraucht_trennzeichen_und_erhaelt_den_text() {
        let s = Sitzung::neu(800, 600);
        for quelle in [
            "<p class=\"a\" title=\"a > b\">Grüße</p>",
            "<img src='a/b.png' />",
            "<!-- <b> --> Text <b>fett</b>",
            "<p =x>", "<", "</p>", "<p title='abgeschnitten",
        ] {
            let html = s.zeile_einfaerben(quelle);
            let baum = narsil_fmt_html::parse_html(&html);
            fn text(knoten: &[narsil_fmt_html::HtmlNode], raus: &mut String) {
                for k in knoten {
                    match k {
                        narsil_fmt_html::HtmlNode::Text(t) => raus.push_str(t),
                        narsil_fmt_html::HtmlNode::Element { children, .. } => text(children, raus),
                    }
                }
            }
            let mut sichtbar = String::new();
            text(&baum, &mut sichtbar);
            assert_eq!(sichtbar, quelle, "{html}");
        }
        let html = s.zeile_einfaerben("<p title=\"a > b\">x</p>");
        assert!(html.contains("#6f42c1"));
        assert!(html.contains("#032f62"));
        assert!(html.contains("a &gt; b"));
    }
}

#[cfg(test)]
mod eingabe_performance_regression {
    use super::*;

    fn formular(wert: &str, lang: bool) -> Sitzung {
        let mut s = Sitzung::neu(900, 650);
        let mut html = format!("<form><textarea name='q'>{wert}</textarea></form>");
        if lang {
            html.push_str(&"<p>Ein langer Artikel mit Text und weiteren Elementen.</p>".repeat(400));
        }
        s.angezeigt = Some(s.aus_html(&html, "narsil://eingabetest", 900, 550).unwrap());
        let ord = s.angezeigt.as_ref().unwrap().seite.felder.iter()
            .find(|f| f.tag == "textarea").unwrap().ord;
        s.feld_fokus = Some(ord);
        s.feld_cursor = wert.chars().count();
        s
    }

    fn wert(s: &Sitzung) -> String {
        let a = s.angezeigt.as_ref().unwrap();
        let w = a.stand.lock().unwrap();
        let ord = s.feld_fokus.unwrap();
        w.eingabe_werte.get(&ord).cloned()
            .or_else(|| w.eingaben.iter().find(|f| f.ord == ord).map(|f| f.wert.clone()))
            .unwrap()
    }

    #[test]
    fn cursorbewegung_behaelt_layout_und_feldwert() {
        let mut s = formular("Grüße 😀", true);
        let vorher = s.angezeigt.as_ref().unwrap().baum.root.children.as_ptr();
        s.schmutzig = false;
        assert!(s.taste_ins_feld(KeyEvent::press(Key::Left)));
        assert!(s.taste_ins_feld(KeyEvent::press(Key::Home)));
        assert!(s.taste_ins_feld(KeyEvent::press(Key::End)));
        assert_eq!(s.angezeigt.as_ref().unwrap().baum.root.children.as_ptr(), vorher);
        assert_eq!(wert(&s), "Grüße 😀");
        assert_eq!(s.feld_cursor, 7);
        assert!(s.schmutzig);
        assert!(s.cursor_rechteck().is_some());
    }

    #[test]
    fn textarea_enter_verwendet_unicode_zeichenposition() {
        let mut s = formular("ä😀z", false);
        s.feld_cursor = 2;
        assert!(s.taste_ins_feld(KeyEvent::press(Key::Enter)));
        assert_eq!(wert(&s), "ä😀\nz");
        assert_eq!(s.feld_cursor, 3);
    }

    #[test]
    fn textarea_enter_ersetzt_die_markierung() {
        let mut s = formular("ä😀z", false);
        s.feld_auswahl = Some((0, 2));
        s.feld_cursor = 2;
        assert!(s.taste_ins_feld(KeyEvent::press(Key::Enter)));
        assert_eq!(wert(&s), "\nz");
        assert_eq!(s.feld_cursor, 1);
        assert!(s.feld_auswahl.is_none());
    }

    #[test]
    fn textarea_enter_am_limit_verliert_keine_eingabe() {
        let text = "😀".repeat(crate::dom::MAX_EINGABE_WERT);
        let mut s = formular(&text, false);
        assert!(s.taste_ins_feld(KeyEvent::press(Key::Enter)));
        assert_eq!(wert(&s), text);
        assert_eq!(s.feld_cursor, crate::dom::MAX_EINGABE_WERT);
    }

    #[test]
    fn backspace_ohne_adressfokus_behaelt_adresse() {
        let mut s = Sitzung::neu(900, 650);
        s.chrome.setze_adresse("narsil://start");
        s.leiste_im_fokus = false;
        s.taste(KeyEvent::press(Key::Backspace));
        assert_eq!(s.chrome.adresse, "narsil://start");
    }

    #[test]
    fn loeschen_in_adressleiste_entfernt_veraltete_vorschlaege() {
        let mut s = Sitzung::neu(900, 650);
        s.browser.anwenden(&Navigation::To("https://example.com/".into()));
        s.chrome.setze_adresse("https://ex");
        s.leiste_im_fokus = true;
        s.vorschlaege_aktualisieren();
        assert!(s.vorschlaege_wahl.is_some());
        s.chrome.auswahl = Some((0, s.chrome.adresse.chars().count()));
        s.taste(KeyEvent::press(Key::Backspace));
        assert!(s.chrome.adresse.is_empty());
        assert!(s.vorschlaege_wahl.is_none());
        s.chrome.setze_adresse("https://ex");
        s.vorschlaege_aktualisieren();
        assert!(s.vorschlaege_wahl.is_some());
        s.chrome.auswahl = Some((0, s.chrome.adresse.chars().count()));
        s.taste(KeyEvent::press(Key::Delete));
        assert!(s.chrome.adresse.is_empty());
        assert!(s.vorschlaege_wahl.is_none());
    }

    #[test]
    #[ignore = "Manuelle Laufzeitmessung, keine schwankende CI-Zeitgrenze"]
    fn messung_cursor_auf_langer_seite() {
        let mut s = formular("Grüße 😀", true);
        let start = std::time::Instant::now();
        for _ in 0..30 {
            s.taste_ins_feld(KeyEvent::press(Key::Home));
            s.taste_ins_feld(KeyEvent::press(Key::End));
        }
        let schnell = start.elapsed();
        let start = std::time::Instant::now();
        for _ in 0..30 {
            s.taste_ins_feld(KeyEvent::press(Key::Home));
            s.seite_neu_bauen();
            s.taste_ins_feld(KeyEvent::press(Key::End));
            s.seite_neu_bauen();
        }
        let voller_aufbau = start.elapsed();
        println!("60 Cursorbewegungen, 400 Absätze: ohne Neubau {schnell:?}; mit bisherigem Vollaufbau {voller_aufbau:?}");
        assert_eq!(wert(&s), "Grüße 😀");
    }
}

/// Release 08.10.2026 (Glas-Feinschliff, Startseiten-Suchfeld): der
/// Feldueberzug zeichnet das Feld in SEINEM Stil, der Platzhalter ist grau,
/// Passwoerter sind Punkte, der Cursor hat die Textfarbe, und das Suchfeld
/// der Startseite fuehrt zur Adresse oder Suche.
#[cfg(test)]
mod feld_ueberzug_regression {
    use super::*;

    fn seite(html: &str) -> Sitzung {
        let mut s = Sitzung::neu(900, 650);
        s.angezeigt = Some(s.aus_html(html, "narsil://feldtest", 900, 550).unwrap());
        s
    }

    fn ord_von(s: &Sitzung, name: &str) -> usize {
        let a = s.angezeigt.as_ref().unwrap();
        a.seite
            .felder
            .iter()
            .map(|f| f.ord)
            .find(|o| feld_attribut(a, *o, "name").as_deref() == Some(name))
            .expect("Feld nicht gefunden")
    }

    fn ueberzug(s: &Sitzung) -> Vec<DisplayItem> {
        let mut l = DisplayList::new();
        s.feld_ueberzug(&mut l);
        l.iter().cloned().collect()
    }

    fn texte(items: &[DisplayItem]) -> Vec<(String, Color)> {
        items
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text { text, color, .. } => Some((text.clone(), *color)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn das_dunkle_feld_bekommt_seine_farben_und_seinen_radius() {
        let s = seite(
            "<form><input name=q value=\"abc\" style=\"background:#1c1e28;color:#f2f2f6;\
             border-radius:12px;padding:6px 10px;border:1px solid #3a3d4a\"></form>",
        );
        let items = ueberzug(&s);
        let flaeche = items.iter().find_map(|i| match i {
            DisplayItem::Rect { color, radius, .. } => Some((*color, *radius)),
            _ => None,
        });
        let (farbe, radius) = flaeche.expect("kein Ueberzug-Rechteck");
        assert_eq!((farbe.r, farbe.g, farbe.b), (0x1c, 0x1e, 0x28), "Flaeche nicht in Feldfarbe");
        assert!(radius.iter().all(|r| *r > 8.0), "Radius fehlt: {radius:?}");
        let t = texte(&items);
        assert_eq!(t.len(), 1, "{t:?}");
        assert_eq!(t[0].0, "abc");
        assert_eq!((t[0].1.r, t[0].1.g, t[0].1.b), (0xf2, 0xf2, 0xf6), "Text nicht in Feldfarbe");
        assert!(
            !items.iter().any(|i| matches!(i, DisplayItem::Rect { color, .. } if color.r == 255 && color.g == 255 && color.b == 255)),
            "weisses Rechteck ueber dem dunklen Feld"
        );
    }

    #[test]
    fn der_platzhalter_ist_grau_und_das_passwort_sind_punkte() {
        let s = seite(
            "<form><input name=a placeholder=\"Suche\"><input type=password name=p value=\"geheim\"></form>",
        );
        let t = texte(&ueberzug(&s));
        // Release 15.10.2026: the page draws the placeholder of an untouched
        // field itself (the overlay only covers edited fields), so the whole
        // display list is checked.
        let alle = texte(&s.display_list().iter().cloned().collect::<Vec<_>>());
        let (_, grau) = alle.iter().find(|(x, _)| x == "Suche").expect("Platzhalter fehlt");
        // Zwischen Textfarbe (dunkel) und Grund (weiss): weder das eine noch das andere.
        assert!(grau.r > 0x60 && grau.r < 0xe0, "Platzhalter nicht grau: {grau:?}");
        assert!(t.iter().any(|(x, _)| x == "\u{2022}".repeat(6).as_str()), "keine Passwortpunkte: {t:?}");
        assert!(!t.iter().any(|(x, _)| x.contains("geheim")), "Passwort im Klartext: {t:?}");
    }

    #[test]
    fn der_cursor_hat_die_textfarbe_und_liegt_ueber_dem_ueberzug() {
        let mut s = seite(
            "<form><input name=q value=\"abc\" style=\"background:#1c1e28;color:#f2f2f6;padding:6px 18px\"></form>",
        );
        let ord = ord_von(&s, "q");
        s.feld_fokus = Some(ord);
        s.feld_cursor = 3;
        let c = s.cursor_rechteck().expect("kein Cursor");
        let DisplayItem::Rect { rect, color, .. } = c else { panic!("Cursor ist kein Rechteck") };
        assert_eq!((color.r, color.g, color.b), (0xf2, 0xf2, 0xf6));
        let a = s.angezeigt.as_ref().unwrap();
        let bild = feld_bilder(a, &[ord]).remove(&ord).unwrap();
        // Hinter dem Polster und hinter dem Text, nicht am Kastenrand.
        assert!(rect.x > bild.flaeche.x + 18.0, "Cursor vor dem Polster: {rect:?} {:?}", bild.flaeche);
        // In der Gesamtliste steht der Cursor NACH dem Ueberzugstext.
        let liste: Vec<DisplayItem> = s.display_list().iter().cloned().collect();
        let text_nr = liste.iter().rposition(|i| matches!(i, DisplayItem::Text { text, .. } if text == "abc"));
        let cursor_nr = liste.iter().rposition(|i| matches!(i, DisplayItem::Rect { color, rect: r, .. }
            if color.r == 0xf2 && color.g == 0xf2 && r.width <= 3.0));
        assert!(cursor_nr > text_nr, "Cursor unter dem Ueberzug: {cursor_nr:?} <= {text_nr:?}");
    }

    #[test]
    fn ein_langer_wert_wird_auf_die_feldbreite_gekuerzt() {
        let lang = "x".repeat(60);
        let mut s = seite(&format!("<form><input name=q value=\"{lang}\" style=\"width:80px\"></form>"));
        let ord = ord_von(&s, "q");
        let t = texte(&ueberzug(&s));
        let (sichtbar, _) = t.iter().find(|(x, _)| x.starts_with('x')).expect("kein Text");
        assert!(sichtbar.chars().count() < 60, "nicht gekuerzt: {}", sichtbar.len());
        // Mit Fokus am Ende: der Ausschnitt zeigt das ENDE (der Cursor bleibt sichtbar).
        s.feld_fokus = Some(ord);
        s.feld_cursor = 60;
        let a = s.angezeigt.as_ref().unwrap();
        let bild = feld_bilder(a, &[ord]).remove(&ord).unwrap();
        let (von, _) = s.feld_ausschnitt(&lang, Some(60), &bild, 64);
        assert!(von > 0, "Ausschnitt schiebt nicht mit dem Cursor");
        let c = s.cursor_rechteck().unwrap();
        let DisplayItem::Rect { rect, .. } = c else { panic!("Cursor ist kein Rechteck") };
        assert!(rect.x <= bild.flaeche.x + bild.flaeche.width, "Cursor ausserhalb des Feldes");
    }

    #[test]
    fn die_startseitenfrage_wird_gelesen() {
        assert_eq!(startseite_frage("narsil://start?q=hallo+welt").as_deref(), Some("hallo welt"));
        assert_eq!(startseite_frage("narsil://start?q=gr%C3%BC%C3%9Fe").as_deref(), Some("grüße"));
        assert_eq!(startseite_frage("narsil://?q=a%2Bb").as_deref(), Some("a+b"));
        assert_eq!(startseite_frage("narsil://start?q=").as_deref(), Some(""));
        assert_eq!(startseite_frage("narsil://start"), None);
        assert_eq!(startseite_frage("narsil://bild-diaet?q=x"), None);
        assert_eq!(startseite_frage("https://example.org/start?q=x"), None);
    }

    #[test]
    fn das_suchfeld_der_startseite_fuehrt_zur_adresse() {
        let mut s = Sitzung::neu(900, 650);
        let _ = s.gehe_zu("narsil://start?q=narsil%3A%2F%2Fstart");
        assert_eq!(s.browser.adresse(), "narsil://start");
        let _ = s.gehe_zu("narsil://start?q=");
        assert_eq!(s.browser.adresse(), "narsil://start");
        // Ein Suchbegriff wird mit der eingestellten Suchmaschine gesucht
        // (ohne Netz: nur die Adresse, die `adresse_aus_leiste` liefert).
        let ziel = s.adresse_aus_leiste("hallo welt");
        assert!(ziel.starts_with("http"), "{ziel}");
        assert!(ziel.contains("hallo"), "{ziel}");
    }

    #[test]
    fn die_ganze_kachel_ist_der_link() {
        let s = seite(&startseite_mit_verlauf(&[]));
        let a = s.angezeigt.as_ref().unwrap();
        // Die Kachel: der aeusserste Kasten um den Text „Optionen", der so
        // breit ist wie eine Kachel (172 + 2 x 14 Polster + 2 x 1 Rahmen).
        fn hat_text(bx: &narsil_layout::LayoutBox) -> bool {
            bx.text.as_ref().is_some_and(|t| t.text.trim() == "Options") || bx.children.iter().any(hat_text)
        }
        // Release 09.10.2026: die Kacheln stehen im Raster mit vier Spalten
        // (bei 900 px Fenster je ~158 px breit) — vorher 172 + 2 x 14 + 2.
        fn kachel(bx: &narsil_layout::LayoutBox) -> Option<Rect> {
            if hat_text(bx) && (120.0..=215.0).contains(&bx.border_box.width) {
                return Some(bx.border_box);
            }
            bx.children.iter().find_map(kachel)
        }
        let r = kachel(&a.baum.root).expect("keine Kachel „Optionen“");
        // Rechts unten in der Kachel, weit weg vom Wort.
        let (x, y) = (r.x + r.width - 6.0, r.y + r.height - 6.0);
        // Das Ziel ist die Adresse selbst, nicht relativ zur Seite
        // (`loese_link`: `about:` ist ein Schema).
        assert_eq!(link_am_punkt(a, x, y).as_deref(), Some("about:einstellungen"));
        let knoten = tiefster_knoten(&a.baum, x, y).expect("Knoten");
        assert_eq!(anker_ueber(a, knoten).as_deref(), Some("about:einstellungen"));
        // Daneben (zwischen den Kacheln) ist kein Link.
        assert_eq!(link_am_punkt(a, r.x + r.width + 7.0, y), None);
        // Schemata ohne `//` bleiben absolut, relative Pfade nicht.
        assert_eq!(narsil_page::loese_link("about:einstellungen", "narsil://start"), "about:einstellungen");
        assert_eq!(narsil_page::loese_link("javascript:void(0)", "https://x.test/a/b"), "javascript:void(0)");
        assert_eq!(narsil_page::loese_link("c.html", "https://x.test/a/b"), "https://x.test/a/c.html");
        assert_eq!(narsil_page::loese_link("/c", "https://x.test/a/b"), "https://x.test/c");
    }

    #[test]
    fn die_startseite_hat_ein_glas_suchfeld_mit_platzhalter() {
        let html = startseite_mit_verlauf(&[]);
        assert!(html.contains("placeholder=\"Search or enter address\""));
        assert!(html.contains("border-radius:22px"));
        let s = seite(&html);
        // The page draws the placeholder (see above).
        let t = texte(&s.display_list().iter().cloned().collect::<Vec<_>>());
        assert!(t.iter().any(|(x, c)| x == "Search or enter address" && c.r < 0xe0 && c.r > 0x50), "{t:?}");
    }
}

/// Release 08.10.2026: „Konvertiert herunterladen" laeuft im Hintergrund —
/// der Klick kehrt sofort zurueck, `tick` meldet Stand und Ende, die Datei
/// liegt im Download-Ordner des TESTprofils.
#[cfg(test)]
mod konvert_hintergrund {
    use super::*;

    fn ziel(format: &str, titel: &str) -> narsil_chrome::konvert::Ziel {
        narsil_chrome::konvert::Ziel {
            titel: titel.to_string(),
            endung: format.to_string(),
            format: format.to_string(),
            mime: String::new(),
            hinweis: String::new(),
            original: false,
        }
    }

    fn warte_bis_fertig(s: &mut Sitzung) {
        let start = std::time::Instant::now();
        while !s.konvert_laeufe.is_empty() && start.elapsed() < std::time::Duration::from_secs(60) {
            s.tick(std::time::Instant::now());
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(s.konvert_laeufe.is_empty(), "der Auftrag wurde nicht fertig");
    }

    #[test]
    fn ein_bild_wird_im_hintergrund_umgewandelt_und_gespeichert() {
        let quelle = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/bilder/vierfelder.png");
        let adresse = format!("file:///{}", quelle.to_string_lossy().replace('\\', "/").trim_start_matches('/'));
        let mut s = Sitzung::neu(900, 650);
        s.konvertieren_und_speichern(&adresse, &ziel("bmp", "BMP"));
        // Sofort zurueck: der Auftrag laeuft, die Statuszeile sagt es.
        assert_eq!(s.konvert_laeufe.len(), 1);
        assert!(s.meldung.as_deref().is_some_and(|m| m.contains("vierfelder.png")), "{:?}", s.meldung);
        warte_bis_fertig(&mut s);
        let meldung = s.meldung.clone().unwrap_or_default();
        let pfad = std::path::PathBuf::from(meldung.trim_start_matches("Saved: "));
        assert!(meldung.starts_with("Saved: "), "{meldung}");
        assert!(pfad.starts_with(crate::plattform::downloads()), "{pfad:?} liegt nicht im Download-Ordner");
        assert!(
            pfad.starts_with(narsil_chrome::einstellungen::ordner()),
            "{pfad:?}: ein Test schreibt in den echten Download-Ordner"
        );
        let bytes = std::fs::read(&pfad).expect("die Datei");
        let bild = narsil_image::werkzeuge::lesen(&bytes).expect("ein lesbares BMP");
        assert_eq!((bild.breite, bild.hoehe), (8, 6));
        let h = s.features.hinweis.as_ref().expect("ein Hinweis");
        assert!(h.text.contains("vierfelder.bmp"), "{}", h.text);
        assert!(h.ziel.as_deref().is_some_and(|z| z.starts_with("file:///")), "{:?}", h.ziel);
        // Release 09.10.2026: die Datei steht auch in der Download-Liste.
        let d = s.downloads_seite.eintraege.first().expect("ein Eintrag in der Download-Liste");
        assert_eq!(d.name, "vierfelder.bmp");
        assert_eq!(d.zustand, narsil_chrome::downloads::DownloadZustand::Fertig);
        assert_eq!(std::path::PathBuf::from(&d.pfad), pfad);
        let _ = std::fs::remove_file(&pfad);
    }

    #[test]
    fn ein_fehler_wird_als_hinweis_gemeldet() {
        let mut s = Sitzung::neu(900, 650);
        s.konvertieren_und_speichern("file:///gibt/es/nicht.png", &ziel("bmp", "BMP"));
        warte_bis_fertig(&mut s);
        let m = s.meldung.clone().unwrap_or_default();
        assert!(m.contains("Cannot load"), "{m}");
        assert!(Sitzung::status_ist_fehler(&m), "die Statuszeile zeigt den Fehler nicht als Fehler: {m}");
        assert!(s.features.hinweis.as_ref().is_some_and(|h| h.text.contains("Cannot load")));
    }

    #[test]
    fn zwei_auftraege_ueberschreiben_sich_nicht() {
        let quelle = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/bilder/vierfelder.png");
        let adresse = format!("file:///{}", quelle.to_string_lossy().replace('\\', "/").trim_start_matches('/'));
        let mut s = Sitzung::neu(900, 650);
        s.konvertieren_und_speichern(&adresse, &ziel("gif", "GIF"));
        s.konvertieren_und_speichern(&adresse, &ziel("gif", "GIF"));
        let mut pfade = Vec::new();
        let start = std::time::Instant::now();
        while !s.konvert_laeufe.is_empty() && start.elapsed() < std::time::Duration::from_secs(60) {
            s.tick(std::time::Instant::now());
            if let Some(m) = s.meldung.as_deref()
                && let Some(p) = m.strip_prefix("Gespeichert: ")
                && !pfade.contains(&p.to_string())
            {
                pfade.push(p.to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        if let Some(m) = s.meldung.as_deref()
            && let Some(p) = m.strip_prefix("Gespeichert: ")
            && !pfade.contains(&p.to_string())
        {
            pfade.push(p.to_string());
        }
        // Beide Dateien gibt es (verschiedene Namen), keine wurde ueberschrieben.
        let ordner = crate::plattform::downloads();
        let gifs: Vec<_> = std::fs::read_dir(&ordner)
            .expect("Ordner")
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("vierfelder") && e.file_name().to_string_lossy().ends_with(".gif"))
            .map(|e| e.path())
            .collect();
        assert!(gifs.len() >= 2, "{gifs:?}");
        for g in gifs {
            let _ = std::fs::remove_file(g);
        }
    }
}

/// Release 08.10.2026: BILD-DIAET Ende zu Ende — Anzeigegroesse, Sichtfenster,
/// Rueckkehr, Ausnahmen, Schutz kleiner Grafiken.
#[cfg(test)]
mod bild_diaet_regression {
    use super::*;

    fn foto() -> String {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../narsil-image/tests/daten/seite100_gross.jpg")
            .canonicalize()
            .expect("Testfoto");
        format!("file:///{}", p.to_string_lossy().replace('\\', "/").trim_start_matches("//?/").trim_start_matches('/'))
    }

    fn seite(html: &str) -> Sitzung {
        let mut s = Sitzung::neu(1000, 700);
        s.angezeigt = Some(s.aus_html(html, "file:///bild-diaet-test/seite.html", 1000, 600).unwrap());
        s
    }

    /// So lange takten, bis nichts mehr laeuft (hoechstens 30 s). Die Uhr
    /// laeuft ab dem letzten Takt weiter (sonst wuerde der zweite Aufruf
    /// gedrosselt und taete nichts).
    fn takten(s: &mut Sitzung) {
        let start = std::time::Instant::now();
        let mut t = s.bild_diaet.letzter_takt.map_or(start, |l| l.max(start));
        loop {
            t += std::time::Duration::from_millis(300);
            s.bild_diaet_takt(t);
            if s.bild_diaet.offen.is_empty() && !s.bild_diaet.arbeit {
                break;
            }
            assert!(start.elapsed() < std::time::Duration::from_secs(30), "die Bild-Diaet wird nicht fertig");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    fn bild<'a>(s: &'a Sitzung, url: &str) -> &'a crate::bilder::GeladenesBild {
        match s.angezeigt.as_ref().unwrap().bilder.geladen.get(url) {
            Some(Ok(b)) => b,
            other => panic!("{url}: {other:?}"),
        }
    }

    #[test]
    fn ein_grosses_foto_wird_auf_den_kasten_verkleinert() {
        let url = foto();
        let mut s = seite(&format!("<img src=\"{url}\" width=\"400\" height=\"300\">"));
        assert_eq!((bild(&s, &url).breite, bild(&s, &url).hoehe), (2400, 1800), "vorher voll");
        takten(&mut s);
        let b = bild(&s, &url);
        assert_eq!((b.breite, b.hoehe), (400, 300), "auf den Kasten");
        assert_eq!((b.natur_breite, b.natur_hoehe), (2400, 1800), "die natuerliche Groesse bleibt");
        assert!(s.angezeigt.as_ref().unwrap().bilder.original(&url).is_some(), "Original behalten");
        // Gezeichnet wird weiter in den 400x300-Kasten.
        let liste = s.display_list();
        assert!(liste.iter().any(|i| matches!(i, DisplayItem::Bild { breite: 400, hoehe: 300, rect, .. } if (rect.width - 400.0).abs() < 1.0)));
    }

    #[test]
    fn object_fit_cover_haelt_die_deckgroesse() {
        let url = foto();
        let mut s = seite(&format!("<img src=\"{url}\" style=\"width:300px;height:300px;object-fit:cover\">"));
        takten(&mut s);
        let b = bild(&s, &url);
        // 2400x1800 deckt 300x300: Hoehe 300, Breite 400.
        assert_eq!((b.breite, b.hoehe), (400, 300), "Deckgroesse");
    }

    #[test]
    fn weit_unten_wird_verworfen_und_beim_naeherkommen_neu_dekodiert() {
        let url = foto();
        let mut s = seite(&format!(
            "<div style=\"height:600px\">oben</div><img src=\"{url}\" width=\"400\" height=\"300\" loading=\"eager\">\
             <div style=\"height:6000px\">unten</div>"
        ));
        takten(&mut s);
        assert_eq!(bild(&s, &url).breite, 400, "im Fenster: verkleinert");
        // Weit nach unten: das Bild liegt mehr als zwei Bildschirmhoehen ueber der Sicht.
        s.angezeigt.as_mut().unwrap().versatz = 5000;
        takten(&mut s);
        let b = bild(&s, &url);
        assert!(!b.hat_pixel(), "ausserhalb des Fensters verworfen");
        assert_eq!((b.natur_breite, b.natur_hoehe), (2400, 1800));
        // Der Kasten bleibt: das Layout aendert sich nicht.
        assert!(s.display_list().iter().all(|i| !matches!(i, DisplayItem::Bild { breite: 0, .. })));
        // Zurueck nach oben: im Hintergrund neu dekodiert, in Anzeigegroesse.
        s.angezeigt.as_mut().unwrap().versatz = 0;
        takten(&mut s);
        let b = bild(&s, &url);
        assert!(b.hat_pixel(), "zurueck im Fenster: wieder da");
        assert_eq!((b.breite, b.hoehe), (400, 300));
    }

    #[test]
    fn kleine_grafiken_und_ausnahmen_bleiben_unberuehrt() {
        let url = foto();
        // Ausnahme fuer die Testdomain: der Test-Ordner gehoert dem Testprofil.
        let alt = crate::bild_diaet::ausnahmen();
        let _ = crate::bild_diaet::ausnahmen_speichern(&["bild-diaet-ausnahme.test".to_string()]);
        let mut s = Sitzung::neu(1000, 700);
        s.angezeigt = Some(
            s.aus_html(
                &format!("<img src=\"{url}\" width=\"400\" height=\"300\">"),
                "https://www.bild-diaet-ausnahme.test/seite.html",
                1000,
                600,
            )
            .unwrap(),
        );
        s.bild_diaet.ausnahmen = crate::bild_diaet::ausnahmen();
        takten(&mut s);
        let _ = crate::bild_diaet::ausnahmen_speichern(&alt);
        assert_eq!(bild(&s, &url).breite, 2400, "Ausnahme: nicht verkleinert");
    }

    #[test]
    fn breiter_gebraucht_wird_aus_dem_original_neu_dekodiert() {
        let url = foto();
        // Eine echte Datei: der Umbau nach `resize` laedt die Adresse neu.
        let pfad = std::env::temp_dir().join(format!("narsil-bild-diaet-{}.html", std::process::id()));
        std::fs::write(&pfad, format!("<img src=\"{url}\" style=\"width:50%\">")).expect("Testseite");
        let mut s = Sitzung::neu(1000, 700);
        let adresse = format!("file:///{}", pfad.to_string_lossy().replace('\\', "/").trim_start_matches('/'));
        s.gehe_zu(&adresse).expect("laden");
        takten(&mut s);
        let schmal = bild(&s, &url).breite;
        assert!(schmal < 2400, "verkleinert: {schmal}");
        // Das Fenster wird breiter: der Kasten waechst, das Bild muss nach.
        s.resize(2000, 700);
        // Der Umbau wartet eine Ruhezeit ab (`UMBAU_RUHE`).
        s.tick(std::time::Instant::now() + std::time::Duration::from_secs(2));
        takten(&mut s);
        let breit = bild(&s, &url).breite;
        let _ = std::fs::remove_file(&pfad);
        assert!(breit > schmal, "nach dem Verbreitern {breit} <= {schmal}");
    }
}

/// Release 09.10.2026 (Animation): was der Takt vergleicht.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BewegungsStand {
    menue: bool,
    tabmenue: bool,
    leisten_menue: bool,
    downloads: bool,
    klapp: bool,
    dialog: bool,
    leisten_hover: (narsil_chrome::Hover, Option<usize>, Option<u8>),
    menue_zeiger: (Option<usize>, bool, Option<usize>),
    tab: (usize, usize),
}

/// Release 09.10.2026: kein Panel und kein Menue im falschen Tab.
#[cfg(test)]
mod ueberlagerung_je_tab {
    use super::*;

    fn texte(s: &Sitzung) -> Vec<String> {
        s.display_list()
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn alles_offen(s: &mut Sitzung) {
        s.downloads_umschalten();
        assert!(s.downloads_offen);
        assert!(
            texte(s).iter().any(|t| t.contains("Search downloads")),
            "das offene Panel steht im Bild"
        );
        // Das Panel deckt die Seite: der Platzhalter des Startseiten-Felds
        // (Feldueberzug) steht nicht darin. Nur unterhalb der Leiste — die
        // leere Adressleiste hat denselben Platzhalter.
        let oben = s.chrome.seiten_oberkante() as f32;
        let im_panel: Vec<String> = s
            .display_list()
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text { text, origin, .. } if origin.y >= oben => Some(text.clone()),
                _ => None,
            })
            .collect();
        // Die Seite steht in der Liste VOR dem Panel (das Panel deckt sie mit
        // seinem Grund); dahinter darf kein Feld der Seite mehr kommen.
        let panel = im_panel.iter().position(|t| t.contains("Search downloads"));
        let feld = im_panel.iter().rposition(|t| t.contains("Search or enter address"));
        assert!(
            panel.is_some() && (feld.is_none() || feld < panel),
            "Feldueberzug ueber dem Panel: {im_panel:?}"
        );
        s.menue = Some(RechtsklickMenue {
            x: 10.0,
            y: 10.0,
            link: String::new(),
            ktx: narsil_chrome::ktx_menue::Menue::oeffne(
                narsil_chrome::ktx_menue::Kontext::Leer,
                "",
                &Point::new(10.0, 10.0),
            ),
        });
        s.leisten_menue = Some(crate::leiste_klick::LeistenMenue { x: 10.0, y: 10.0, skala_prozent: 100 });
        s.chrome.suche_offen = true;
    }

    fn alles_zu(s: &Sitzung, wo: &str) {
        assert!(!s.downloads_offen, "{wo}: das Download-Panel ist noch offen");
        assert!(s.menue.is_none(), "{wo}: das Kontextmenue ist noch offen");
        assert!(s.leisten_menue.is_none(), "{wo}: das Leistenmenue ist noch offen");
        assert!(s.tabmenue.is_none(), "{wo}: das Tabmenue ist noch offen");
        assert!(!s.chrome.suche_offen, "{wo}: die Suchleiste ist noch offen");
        // Und im Bild steht kein Panel: der Text des Download-Panels fehlt.
        let texte = texte(s);
        assert!(!texte.iter().any(|t| t.contains("Search downloads")), "{wo}: Panel im Bild: {texte:?}");
    }

    /// Wege an `tab_holen` vorbei (Tabmenue, Strg+Umschalt+T): der Takt
    /// bemerkt den Wechsel und schliesst.
    #[test]
    fn der_takt_schliesst_nach_einem_wechsel_ueber_den_browser() {
        let mut s = Sitzung::neu(900, 650);
        let _ = s.gehe_zu("narsil://start");
        s.tick(std::time::Instant::now());
        alles_offen(&mut s);
        // Ein Takt ohne Wechsel laesst alles offen.
        s.tick(std::time::Instant::now());
        assert!(s.downloads_offen, "ohne Wechsel bleibt das Panel offen");
        s.browser.anwenden(&Navigation::TabNeu);
        s.tick(std::time::Instant::now());
        alles_zu(&s, "Tabmenue: neuer Tab");
    }

    #[test]
    fn neuer_tab_wechsel_und_schliessen_schliessen_die_panels() {
        let mut s = Sitzung::neu(900, 650);
        let _ = s.gehe_zu("narsil://start");
        alles_offen(&mut s);
        assert!(s.tab_neu());
        alles_zu(&s, "neuer Tab");
        alles_offen(&mut s);
        assert!(s.tab_wechseln(false));
        alles_zu(&s, "Tabwechsel");
        alles_offen(&mut s);
        assert!(s.tab_wechseln_zu(1));
        alles_zu(&s, "Strg+Zahl");
        alles_offen(&mut s);
        assert!(s.tab_schliessen());
        alles_zu(&s, "Tab closed");
    }
}

/// Release 09.10.2026: die Animationen der Oberflaeche — gerechnet, nicht
/// gewartet (im Test sind sie aus, die Uebergaenge werden von Hand gesetzt).
#[cfg(test)]
mod bewegung_regression {
    use super::*;
    use std::time::{Duration, Instant};

    fn menue_liste(s: &Sitzung) -> DisplayList {
        let m = narsil_chrome::ktx_menue::Menue::oeffne(
            narsil_chrome::ktx_menue::Kontext::Leer,
            "",
            &Point::new(200.0, 150.0),
        );
        narsil_chrome::ktx_menue::display_list(&m, &s.chrome, s.breite, s.hoehe)
    }

    fn glas_breite(l: &DisplayList) -> f32 {
        l.effekte()
            .iter()
            .find_map(|(_, _, e)| match e {
                narsil_display::Effekt::Glas(g) => Some(g.rect.width),
                _ => None,
            })
            .expect("Glas")
    }

    #[test]
    fn ein_menue_geht_skaliert_und_eingeblendet_auf() {
        let s = Sitzung::neu(900, 650);
        let menue = menue_liste(&s);
        let voll = glas_breite(&menue);
        // Am Anfang: 0,96 und unsichtbar (Gruppe mit Deckkraft 0).
        let jetzt = Instant::now();
        let mut l = DisplayList::new();
        Sitzung::aufgehend(&mut l, &menue, Some(crate::bewegung::Uebergang::neu(jetzt, Duration::from_secs(3600))), 200.0, 150.0);
        let b = glas_breite(&l);
        assert!((b - voll * 0.96).abs() < 1.0, "skaliert: {b} statt {}", voll * 0.96);
        assert!(
            l.effekte().iter().any(|(_, _, e)| matches!(e, narsil_display::Effekt::Gruppe { deckkraft, .. } if *deckkraft < 0.05)),
            "eingeblendet ab 0"
        );
        // Fertig (oder keine Animation): genau das Menue.
        let mut fertig = DisplayList::new();
        Sitzung::aufgehend(&mut fertig, &menue, Some(crate::bewegung::Uebergang::neu(jetzt - Duration::from_secs(1), crate::bewegung::OEFFNEN)), 200.0, 150.0);
        let mut ohne = DisplayList::new();
        Sitzung::aufgehend(&mut ohne, &menue, None, 200.0, 150.0);
        assert_eq!(fertig, ohne);
        assert_eq!(glas_breite(&ohne), voll);
    }

    #[test]
    fn die_tableiste_waechst_und_schliesst_ihre_luecke() {
        let mut s = Sitzung::neu(1200, 650);
        assert!(s.tab_neu());
        s.tabs_anzeigen();
        let ruhig = (s.chrome.tab(1).expect("Tab 2"), s.chrome.plus());
        // Halb gewachsen: halbe Breite, das Plus steht eine halbe Breite links.
        s.chrome.tab_bewegung = Some((narsil_chrome::TabWandel::Auf(1), 0.5));
        let halb = (s.chrome.tab(1).expect("Tab 2"), s.chrome.plus());
        assert!(halb.0.breite < ruhig.0.breite * 2 / 3, "{} vs {}", halb.0.breite, ruhig.0.breite);
        assert!(halb.1.x < ruhig.1.x, "das Plus rueckt mit");
        // Luecke vor Tab 1 halb offen: Tab 1 steht weiter rechts.
        s.chrome.tab_bewegung = Some((narsil_chrome::TabWandel::Zu(1), 0.5));
        let luecke = s.chrome.tab(1).expect("Tab 2");
        assert!(luecke.x > ruhig.0.x);
        // Ruhe: wie vorher.
        s.chrome.tab_bewegung = None;
        assert_eq!((s.chrome.tab(1).expect("Tab 2"), s.chrome.plus()), ruhig);
    }

    #[test]
    fn ohne_bewegung_wartet_die_schleife_nicht_im_bildtakt() {
        let mut s = Sitzung::neu(900, 650);
        let jetzt = Instant::now();
        assert!(!s.bewegung.laeuft(jetzt));
        // Eine laufende Bewegung: Warten im Takt der Bildrate.
        s.bewegung.menue = Some(crate::bewegung::Uebergang::neu(jetzt, crate::bewegung::OEFFNEN));
        assert!(s.tick_warte_ms(jetzt).is_some_and(|w| w <= s.bild_ms.max(1)));
        // Danach: der Takt raeumt ab, nichts laeuft mehr.
        let spaeter = jetzt + Duration::from_millis(500);
        assert!(!s.bewegung.takt(spaeter));
        assert!(!s.bewegung.laeuft(spaeter));
    }
}
