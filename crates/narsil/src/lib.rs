//! narsil — der Browser.
//!
//! Diese Datei macht aus [`Sitzung`] ein Programm. Sie enthaelt keine
//! Entscheidung ueber Farben, Adressen oder Links — das steht in
//! [`sitzung`], weil es ohne Fenster getestet werden muss.

/// DAS ABNAHMEBILD AN ZWEI ORTEN (03.10., 3:0x) — eine Regel, eine Stelle.
///
/// GEMESSENE URSACHE: `werkzeuge/platz_halten.sh:39` verschiebt jede
/// `*.png` und `*.txt` unter `team/log` nach 180 Minuten auf die externe
/// Platte; `team/bilder` nennt das Skript NULL Mal. Siehe das Modul.
pub mod abnahme_bild;
/// Release 15.10.2026: host functions of the animation module.
mod anim_hosts;
/// Release 15.10.2026: animation frames painted without a page rebuild.
mod anim_schnell;
/// Release 15.10.2026: request URL modifiers of CSS `url()` for images.
mod url_modifiers;

/// KOMPRESS 04.10.2026 (Anordnung der Leitung, 14:50): die `<audio controls>`-
/// Leiste und die Verdrahtung zu `narsil-audio`. **Neu** — siehe
/// `team/BERICHT_KOMPRESS_G9ar.txt`.
pub mod audio;
/// I3: Bilder laden, dekodieren und an die Anzeige geben.
pub mod bilder;
pub mod formular_werte;
// Release 09.10.2026: Animationen der Oberflaeche.
pub mod bewegung;
/// Release 08.10.2026: Bild-Diaet (Anzeigegroesse, Sichtfenster, spaeter laden).
pub mod bild_diaet;
mod bild_lader;
pub mod canvas;
pub mod canvas_text;
pub(crate) mod canvas_gsub;
mod bilder_daten;
mod bilder_hintergrund;
mod seiten_lader;
// Release 07.10.2026: Absätze nach dem Layout mit der gemessenen Breite
// neu umbrechen (Grid-Spalten, Flex, Floats).
pub mod nachumbruch;
/// W7 P3: externe Stylesheets (`<link rel=stylesheet>`).
pub mod blaetter;
/// D9 (01.10.2026): die Entwicklerwerkzeuge im echten Browser.
/// F12, Strg+Shift+I, Kontextmenue, Fuellen, Klick, Anzeige.
pub mod devtools_bruecke;
pub mod dienste;
pub mod plattform;
/// W6B (J9): das Standard-DOM — `document`, Element, `style`, `classList`.
pub mod dom;
/// UI-VORLAGE Schritt 5 (Koordination 02.10. mittags): die Einstellungen.
/// `narsil-chrome` zeichnet (ZUSTAND + GEOMETRIE, Rolle UI), hier steht der
/// Zustand der Sitzung und der Weg aus Zeichnen/Klick/Speichern.
pub mod einstellungen_weg;
/// W6D: ein Handler als Funktionswert (mit Closure) oder als Quelltext.
pub mod handler;
mod ereignis_host;
/// W6 (J9-J11): JavaScript — DOM-Anbindung, Skriptlauf, Klick-Handler.
pub mod js;
/// W7C R4: die JS-DOM-Bruecke zu `narsil-jsvm` (`team/WUNSCH_JS.txt`).
///
/// B1-B3 (01.10.2026, Rolle KOMPRESS): dieser Satz war einmal „Additiv —
/// `Sitzung` laeuft weiter ueber `narsil-js`". Er ist seit dem Tausch
/// FALSCH, und ein falscher Satz im Kopf einer Datei ist schlimmer als
/// keiner: `js`, `dom`, `handler` und `sitzung` sprechen jetzt
/// `narsil-jsvm`, und `narsil-js` steht nicht mehr in `Cargo.toml`.
pub mod jsvm;
// added 04.10.2026, Rolle TLS (Freigabe der Leitung: „(1) in narsil laden.rs“).
// **EINE Zeile**, weil Rust ein Modul erst benennt. Die Datei `keks.rs` ist
// **meine** (Freigabe 23:0x fuer `narsil`), `lib.rs` ist es auch — und
// beides steht hier offengelegt.
pub mod keks;
pub mod laden;
/// U1c (01.10.2026, Rolle UI): DIE KLICK-WEITERLEITUNG DER LEISTE.
///
/// GEMESSENE BEGRUENDUNG fuer die eigene Datei (Freigabe der Koordination,
/// UI Runde 2: „in crates/narsil NUR die Klickweiterleitung fuer die Leiste,
/// z.B. neue Datei narsil/src/leiste_klick.rs + minimale Einhaengung"):
/// `sitzung.rs` hat ueber 5900 Zeilen und gehoert der Rolle HTML. Jede
/// Zeile dort waere eine Zeile, an der zwei Rollen aneinander vorbeiarbeiten.
///
/// WAS DRIN IST: das Menue des Menue-Knopfs (offen, Treffer, Eintraege,
/// Zeichnen), das Plus (neuer Tab), der Hover der Leiste. Die GEOMETRIE
/// kommt aus `narsil-chrome` (`Chrome::hover_unter`), damit es keine zweite
/// Wahrheit ueber die Stellen gibt.
pub mod leiste_klick;
/// W6 (J10): die Befehle werden auf die HTML-Quelle angewendet.
pub mod replay;
/// H6 (03.10.2026): DIE SICHERHEITSHAKEN — Mixed Content, CSP, CORS und der
/// Download-Vorbehalt.
///
/// added 03.10.2026, Rolle TLS. **FREIGABE DER LEITUNG** am 03.10.2026 (weil
/// HTML und UI ausgelastet waren): „neue Datei `narsil/src/sicherheit_haken.rs`
/// (deine) mit **minimalen** Aufrufen in `narsil`“.
///
/// **WARUM EINE EIGENE DATEI**, in einem Workspace, in dem die Regel „Crate
/// bekommt eine Verzeichnisdatei“ sonst gilt: `bilder.rs`, `blaetter.rs`,
/// `js.rs` und `sitzung.rs` gehoeren **vier anderen** Rollen (EIGENTUM.txt:
/// BILD, CSS, JS, HTML). Ein Modul, das dort an **vier** Stellen eine
/// Bedingung einfuegt, ist genau das, worueber die Rolle „Keine Meta-Arbeit als
/// Hauptarbeit“ geschrieben wurde — es gibt **eine** Datei mit allen Regeln,
/// und die Aufrufstellen sind je **eine** Zeile.
///
/// **DIE REGELN STEHEN NICHT HIER**, sondern in
/// `narsil-net/src/sicherheit.rs` (Crate TLS) und werden hier **gerufen**. Die
/// Tests der Regeln stehen dort; die Tests der Verdrahtung stehen hier.
pub mod sicherheit_haken;
pub mod sitzung;
/// VIDEO_19 (04.10.2026, Freigabe der Leitung: „narsil/src/sitzung.rs
/// minimal, drei Aufrufe"): das **Video im Fenster**.
///
/// GLEICHES MUSTER wie `sicherheit_haken.rs`, und aus demselben Grund:
/// `sitzung.rs` hat ueber 10000 Zeilen und gehoert der Rolle HTML, KONTEXT
/// und UI arbeiten dort. Die **Regeln** stehen hier, die **Aufrufstellen**
/// sind je eine Zeile.
///
/// WAS DRIN IST: das Laden einer WebM-Datei, der Fensterablauf aus
/// `narsil_media::fenster` (Spieler, Bedienung, Leiste, Weckzeit), der
/// Abzug des Bildes in den Framebuffer (ein `memcpy` je Zeile, weil
/// `narsil_raster::Framebuffer::pixels` und `narsil_image::Bild::rgba`
/// **dasselbe** Format sind) und die Zuordnung der Tasten.
pub mod video;
/// W7 P1: Phasen-Messung beim Laden (`NARSIL_ZEITEN=1`).
pub mod zeiten;
pub mod zwangslayout;
/// Release 09.10.2026: `@font-face` der Seite holen und anmelden.
pub mod webschriften;
/// Web workers, MessagePort, BroadcastChannel, SharedWorker (HTML §9, §10).
pub mod worker;
/// IndexedDB storage engine (the API is `indexeddb.js`).
pub mod idb;
/// IndexedDB files on disk.
pub mod idb_file;
/// Cache API storage (the API is `cache_storage.js`).
pub mod cache_storage;
/// Release 09.10.2026: das lebende DOM fuer Seitenskripte.
pub mod lebend;
/// Release 2026-10-15: the `innerText` getter on computed styles.
mod inner_text;
// Release 12.10.2026 (Agent C): implizites @scope im Seitenbaum.
mod scope_implizit;
/// Release 12.10.2026: die Host-Seite von `Range` (DOM §5, lebende Bereiche).
pub mod bereich;
/// Release 08.10.2026: die Zusatzfunktionen aus dem Tab „Features" der
/// Einstellungen (Schnappschuss, Element-Waechter, Netz-Sandbox, Formular-
/// Gedaechtnis, Einfrieren, Kopfregeln, Link-Reiniger, Fristen, Ruhemodus,
/// Belege, Cookie-Autopilot, Link-Ziel-Vorschau).
pub mod features;
pub mod zustand;
/// H5 (03.10.2026): DER ZUSTAND IN EINER DATEI.
///
/// GEMESSENE BEGRUENDUNG fuer ein eigenes Modul und nicht zwei Funktionen
/// in `sitzung.rs`: die Datei ist der einzige Weg, der etwas I/O tut,
/// und `sitzung.rs` ist 5000 Zeilen Pfadlogik. Ein Modul, das man in
/// einem Test aufruft, ohne eine Sitzung zu bauen, ist ein Modul, das man
/// pruefen kann — und diese Datei IST pruefbar, ohne Fenster.
//
// KONTEXT (05.10.2026, 22:45, Leitung): DIE **EINGABE-EINSPEISUNG** fuer
// Tests (Rechtsklick -> Menue -> Zeile) — ein oeffentliches Modul, weil
// GEMESSEN ist, dass die Integrationstests in `crates/narsil/tests/` ein
// `#[cfg(test)]`-Modul **nicht** sehen.
pub mod testmodus;
pub mod zwischenablage_system;

pub use bilder::{BildCache, BildFehler, GeladenesBild};
pub use devtools_bruecke::{an_seite, bei_fensterwechsel, taste as devtools_taste};
pub use js::{Befehl, Kiste};
pub use laden::{LadeFehler, Ladung, Quellart, lade};
// KONTEXT (04.10.2026, 16:20): Der Scrollbalken ist ein Teil der **Seite**, und
// die Tests in `tests/ktx_scrollen.rs` brauchen seine Geometrie ohne Fenster.
// KONTEXT (04.10.2026, 19:05): `SCROLL_DAUER_MS` und `ScrollAni` sind dazugekommen
// (die sanfte Bewegung, 120 ms) — dieselbe Zeile, damit die Liste **eine** bleibt.
// NARSIL (06.10.): `RechtsklickMenue` steht dazu, weil `Sitzung::menue` ein
// **oeffentliches** Feld von diesem Typ ist. GEMESSEN ist, dass `tests/
// ktx_menue_aktionen.rs` den Typ ueber `narsil::RechtsklickMenue` baut (E0422:
// „not found in `narsil`") — ein oeffentliches Feld, dessen Typ man ueber den
// Crate-Weg nicht benennen kann, ist ein halb oeffentlicher Zustand.
pub use sitzung::{
    Angezeigt, Auswahl, Fehler, RechtsklickMenue, SCROLL_DAUER_MS, SCROLL_DAUSBLENDEN_MS,
    SCROLLBREITE, SEITEN_START_Y, ScrollAni, ScrollFlaeche, Scrollbalken, Scrollziel, Sitzung,
    TextStelle, scroll_flaechen, startseite, text_stellen,
};
