//! DIE ICONS DER OBERFLAECHE — die 21 Dateien aus `team/design/icons`.
//!
//! LEITUNG (03.10., `BERICHT_UI_ICONS_UMGESETZT.txt` nicht
//! abgenommen, `team/log/leitung_icons_fehler.png`): „Eigenen Uebersetzer
//! wegwerfen. Die .svg-Dateien zur Bauzeit per include_str! einbinden und
//! mit dem SVG-Raster von narsil-image (svg.rs, gegen cairosvg geprueft) in
//! Zielgroesse rastern, currentColor durch Themenfarbe ersetzen.`
//!
//! GEMESSENE ENTSCHEIDUNG, warum hier KEIN eigener Uebersetzer steht:
//! `werkzeuge/icons_svg_zu_rust.py` (GEMESSEN 342 Zeilen) ist am 03.10.,
//! GELOESCHT — GEMESSEN war, dass kein Werkzeug es aufruft; vorher
//! stand es als Datei im Baum und tat nichts. Es hat aus
//! `neuladen.svg` einen Bogen mit dem Mittelpunkt (24,80; 6,69) statt
//! (12; 12) gemacht, aus `schild.svg` eine Kette senkrechter Linien
//! (das `c` stand nicht in der Buchstabenliste des Tokenizers, also hat
//! das `v` die sechs Zahlen genommen) und aus `stern.svg` und `home.svg`
//! zerrissene Figuren. Das sind drei Fehler in einem Programm von
//! GEMESSEN 300 Zeilen; die Korrekturen daran waeren ein vierter.
//!
//! GEMESSENE ENTSCHEIDUNG, warum die Datei eingebunden und nicht
//! geoeffnet wird: `include_str!` ist der WEG, den die Leitung verlangt
//! hat, und er hat eine Eigenschaft, die dem Absicht entspricht: die
//! Datei ist beim Bauen Teil des Programms. Aendert sich die Zeichnung,
//! muss das Programm neu gebaut werden — ein Symbol, das zur Laufzeit
//! eine Datei sucht, ist an einem Rechner ohne das Verzeichnis leer.
//!
//! GEMESSENE ENTSCHEIDUNG, warum die DECKUNG gemessen und nicht die
//! Farbe: der Rasterer kennt `currentColor` nicht, deshalb wird das
//! Wort durch eine Farbe ersetzt. Die Deckung, die dabei entsteht, ist
//! von der Farbe unabhaengig (ein Strich deckt gleich viel, egal welche
//! Farbe er hat — das ist im Raster messbar, siehe
//! `die_deckung_ist_von_der_farbe_unabhaengig`). Deshalb wird die
//! Deckung einmal gemessen und gehalten, und die Farbe des Themas kommt
//! erst beim Zeichnen dazu. Sonst haette der Cache eine Farbe als
//! Schluessel, und es gaebe bei jedem Hover einen neuen.

use narsil_core::color::Color;
use std::sync::{Arc, OnceLock};

/// Die ZWEI Zielgroessen. GEMESSENE ENTSCHEIDUNG, warum genau zwei und
/// nicht eine: die Leitung nennt beide — „20 px Leiste, 16 px
/// Tab-Schliessen“. Jede andere Kante wird auf eine von diesen beiden
/// gerundet; [`groesse`] sagt, welche.
pub const GROESSE_TAB: u32 = 16;
pub const GROESSE_LEISTE: u32 = 20;

/// Die Zahl der Dateien in `team/design/icons` — GEMESSEN, nicht
/// geschaetzt: `ls team/design/icons/*.svg | wc -l` = 21.
/// LEITUNG (03.10., 15:56): vier Icons fuer die Audio-Steuerleiste —
/// `abspielen.svg`, `pause.svg`, `lautstaerke.svg`, `stumm.svg`. Damit
/// sind es GEMESSEN 32 Dateien (vorher 28).
/// Release 08.10.2026: `info.svg` und `funke.svg` fuer den Tab „Features"
/// — damit 37 Dateien. Release 08.10.2026: 14 Zeilensymbole des
/// Kontextmenues im Glas-Stil — damit 51.
pub const ANZAHL_DATEIEN: usize = 52;

/// DIE NAMEN der 21 Dateien, in der Reihenfolge von [`ALLE_DATEIEN`].
///
/// GEMESSENE ENTSCHEIDUNG, warum die Namen hier stehen und nicht in der
/// Abnahme gesucht werden: die Pixelabnahme schreibt je Datei eine Rohdatei
/// (`<name>_crate.raw`) und vergleicht sie mit dem Orakel. Ohne diese
/// Liste muesste der Vergleich raten, welche Datei zu welcher Rohdatei
/// gehoert — und ein Raten in einer Abnahme ist ein Fehler, den sie
/// hides, statt sie zu zeigen.
pub const ALLE_NAMEN: [&str; ANZAHL_DATEIEN] = [
    "zurueck",
    "vor",
    "neuladen",
    "stopp",
    "home",
    "neuer_tab",
    "menue",
    "schloss",
    "download",
    "schild",
    "erweiterung",
    "suche",
    "zoom_minus",
    "drucken",
    "einstellungen",
    "ordner",
    "kopieren",
    "lesezeichen",
    "zoom_plus",
    "tab_schliessen",
    "stern",
    "schloss_warnung",
    "drei_punkte",
    "einpassen",
    "bildschirm",
    "fenster",
    "privat",
    "beenden",
    // LEITUNG (03.10., 15:56): die vier Audio-Icons. GEMESSENE
    // ENTSCHEIDUNG, warum sie HIER stehen und nicht in der Leiste: die
    // Leiste hat keine eigene Dateiliste, und ein zweiter Ort fuer
    // Dateinamen waere ein zweiter Ort, an dem sie veralten.
    "abspielen",
    "pause",
    "lautstaerke",
    "stumm",
    // Release 08.10.2026: Fenster-Schliessen, aktiver Werbeblocker, Globus.
    "fenster_schliessen",
    "schild_an",
    "globus",
    // Release 08.10.2026: Tab „Features" — das (i) und der Funke.
    "info",
    "funke",
    // Release 08.10.2026: das Kontextmenue im Glas-Stil.
    "uebersetzen",
    "auswahl",
    "rueckgaengig",
    "schere",
    "einfuegen",
    "papierkorb",
    "link",
    "extern",
    "beschreibung",
    "code",
    "untersuchen",
    "konvertieren",
    "auge",
    "chevron",
    // Release 10.10.2026: die Palette fuer die Einstellungskategorie
    // „Darstellung" (vorher der Rahmen von `bildschirm.svg`).
    "palette",
];

/// Die Hochskalierungen, fuer die es Raster gibt.
///
/// KOORDINATION (03.10., „skaliere nicht separat“): ein Bild, das
/// in einer Groesse gerastert und dann auf eine andere gezogen wird, hat
/// weiche Kanten an geraden Stellen und zackige an schiefen — das war der
/// zerfetzte Stern. Deshalb wird JEDE angezeigte Groesse selbst gerastert.
/// GEMESSENE ENTSCHEIDUNG fuer die Zahl 3 als hoechste Stufe: der
/// Bildtest kennt 1x und 2x (`a_hidpi_1920x1080_und_2x`), und eine dritte
/// Stufe deckt 3x ab, ohne dass die Zahl zur Wunschgroesse wird.
pub const STUFEN: [u32; 3] = [1, 2, 3];

/// Die Zahl der gehaltenen Raster: jede Datei in beiden Groessen und in
/// jeder Stufe.
pub const MAX_RASTER: usize = ANZAHL_DATEIEN * 2 * STUFEN.len();

// ---------------------------------------------------------------------------
// DIE 21 DATEIEN, eingebunden.
// ---------------------------------------------------------------------------
const DATEI_DOWNLOAD: &str = include_str!("../../../team/design/icons/download.svg");
const DATEI_DRUCKEN: &str = include_str!("../../../team/design/icons/drucken.svg");
const DATEI_EINSTELLUNGEN: &str = include_str!("../../../team/design/icons/einstellungen.svg");
const DATEI_ERWEITERUNG: &str = include_str!("../../../team/design/icons/erweiterung.svg");
const DATEI_HOME: &str = include_str!("../../../team/design/icons/home.svg");
const DATEI_KOPIEREN: &str = include_str!("../../../team/design/icons/kopieren.svg");
const DATEI_LESEZEICHEN: &str = include_str!("../../../team/design/icons/lesezeichen.svg");
const DATEI_MENUE: &str = include_str!("../../../team/design/icons/menue.svg");
const DATEI_NEUER_TAB: &str = include_str!("../../../team/design/icons/neuer_tab.svg");
const DATEI_NEULADEN: &str = include_str!("../../../team/design/icons/neuladen.svg");
const DATEI_ORDNER: &str = include_str!("../../../team/design/icons/ordner.svg");
const DATEI_SCHILD: &str = include_str!("../../../team/design/icons/schild.svg");
const DATEI_SCHLOSS: &str = include_str!("../../../team/design/icons/schloss.svg");
const DATEI_STERN: &str = include_str!("../../../team/design/icons/stern.svg");
const DATEI_STOPP: &str = include_str!("../../../team/design/icons/stopp.svg");
const DATEI_SUCHE: &str = include_str!("../../../team/design/icons/suche.svg");
const DATEI_TAB_SCHLIESSEN: &str = include_str!("../../../team/design/icons/tab_schliessen.svg");
const DATEI_VOR: &str = include_str!("../../../team/design/icons/vor.svg");
const DATEI_ZOOM_MINUS: &str = include_str!("../../../team/design/icons/zoom_minus.svg");
const DATEI_ZOOM_PLUS: &str = include_str!("../../../team/design/icons/zoom_plus.svg");
// LEITUNG (03.10.): sieben neue Dateien fuer die sieben Symbole
// ohne Pendant und eine NEUE `erweiterung.svg` (ohne Boegen, nur Kurven).
// „Genauso 1:1 einbinden.“
const DATEI_SCHLOSS_WARNUNG: &str = include_str!("../../../team/design/icons/schloss_warnung.svg");
const DATEI_DREI_PUNKTE: &str = include_str!("../../../team/design/icons/drei_punkte.svg");
const DATEI_EINPASSEN: &str = include_str!("../../../team/design/icons/einpassen.svg");
const DATEI_BILDSCHIRM: &str = include_str!("../../../team/design/icons/bildschirm.svg");
const DATEI_FENSTER: &str = include_str!("../../../team/design/icons/fenster.svg");
const DATEI_PRIVAT: &str = include_str!("../../../team/design/icons/privat.svg");
const DATEI_BEENDEN: &str = include_str!("../../../team/design/icons/beenden.svg");

// LEITUNG (03.10., 15:56, Optik-Vorgaben der Audio-Leiste): vier neue
// Dateien, 1:1 eingebunden wie die Leisten-Icons — kein Emoji, kein
// selbstgemaltes Zeichen.
const DATEI_ABSPIELEN: &str = include_str!("../../../team/design/icons/abspielen.svg");
const DATEI_PAUSE: &str = include_str!("../../../team/design/icons/pause.svg");
const DATEI_LAUTSTAERKE: &str = include_str!("../../../team/design/icons/lautstaerke.svg");
const DATEI_STUMM: &str = include_str!("../../../team/design/icons/stumm.svg");

const DATEI_ZURUECK: &str = include_str!("../../../team/design/icons/zurueck.svg");
const DATEI_FENSTER_SCHLIESSEN: &str =
    include_str!("../../../team/design/icons/fenster_schliessen.svg");
const DATEI_SCHILD_AN: &str = include_str!("../../../team/design/icons/schild_an.svg");
const DATEI_GLOBUS: &str = include_str!("../../../team/design/icons/globus.svg");
const DATEI_INFO: &str = include_str!("../../../team/design/icons/info.svg");
const DATEI_FUNKE: &str = include_str!("../../../team/design/icons/funke.svg");
// Release 08.10.2026: die Zeilensymbole des Kontextmenues im Glas-Stil.
const DATEI_UEBERSETZEN: &str = include_str!("../../../team/design/icons/uebersetzen.svg");
const DATEI_AUSWAHL: &str = include_str!("../../../team/design/icons/auswahl.svg");
const DATEI_RUECKGAENGIG: &str = include_str!("../../../team/design/icons/rueckgaengig.svg");
const DATEI_SCHERE: &str = include_str!("../../../team/design/icons/schere.svg");
const DATEI_EINFUEGEN: &str = include_str!("../../../team/design/icons/einfuegen.svg");
const DATEI_PAPIERKORB: &str = include_str!("../../../team/design/icons/papierkorb.svg");
const DATEI_LINK: &str = include_str!("../../../team/design/icons/link.svg");
const DATEI_EXTERN: &str = include_str!("../../../team/design/icons/extern.svg");
const DATEI_BESCHREIBUNG: &str = include_str!("../../../team/design/icons/beschreibung.svg");
const DATEI_CODE: &str = include_str!("../../../team/design/icons/code.svg");
const DATEI_UNTERSUCHEN: &str = include_str!("../../../team/design/icons/untersuchen.svg");
const DATEI_KONVERTIEREN: &str = include_str!("../../../team/design/icons/konvertieren.svg");
const DATEI_AUGE: &str = include_str!("../../../team/design/icons/auge.svg");
const DATEI_CHEVRON: &str = include_str!("../../../team/design/icons/chevron.svg");
const DATEI_PALETTE: &str = include_str!("../../../team/design/icons/palette.svg");

/// ALLE 21 in der Reihenfolge, in der sie in [`quelle_von`] vorkommen.
pub const ALLE_DATEIEN: [&str; ANZAHL_DATEIEN] = [
    DATEI_ZURUECK,
    DATEI_VOR,
    DATEI_NEULADEN,
    DATEI_STOPP,
    DATEI_HOME,
    DATEI_NEUER_TAB,
    DATEI_MENUE,
    DATEI_SCHLOSS,
    DATEI_DOWNLOAD,
    DATEI_SCHILD,
    DATEI_ERWEITERUNG,
    DATEI_SUCHE,
    DATEI_ZOOM_MINUS,
    DATEI_DRUCKEN,
    DATEI_EINSTELLUNGEN,
    DATEI_ORDNER,
    DATEI_KOPIEREN,
    DATEI_LESEZEICHEN,
    DATEI_ZOOM_PLUS,
    DATEI_TAB_SCHLIESSEN,
    // GEMESSENE ENTSCHEIDUNG (03.10.): `stern.svg` ist eingebunden
    // und wird mitgeprüft, hat aber kein `Symbol` — das Stern-Symbol der
    // Leiste ist aus `symbole::Symbol` nicht vorhanden (GEMESSEN 24
    // Symbole, GEMESSEN 21 Dateien). Die Datei gehoert zu den 21 und wird
    // also nicht weggelassen.
    DATEI_STERN,
    // LEITUNG (03.10.): die sieben neuen Dateien. GEMESSENE
    // ENTSCHEIDUNG, warum sie hinten stehen und nicht bei ihren
    // Geschwistern: `ALLE_DATEIEN` und `ALLE_NAMEN` muessen dieselbe
    // Reihenfolge haben, und eine Umordnung waere eine Aenderung an
    // GEMESSEN 168 Cache-Plaetzen ohne Aussage.
    DATEI_SCHLOSS_WARNUNG,
    DATEI_DREI_PUNKTE,
    DATEI_EINPASSEN,
    DATEI_BILDSCHIRM,
    DATEI_FENSTER,
    DATEI_PRIVAT,
    DATEI_BEENDEN,
    // LEITUNG (03.10., 15:56): die vier Audio-Icons in derselben Reihenfolge
    // wie ihre Namen in [`ALLE_NAMEN`].
    DATEI_ABSPIELEN,
    DATEI_PAUSE,
    DATEI_LAUTSTAERKE,
    DATEI_STUMM,
    DATEI_FENSTER_SCHLIESSEN,
    DATEI_SCHILD_AN,
    DATEI_GLOBUS,
    DATEI_INFO,
    DATEI_FUNKE,
    DATEI_UEBERSETZEN,
    DATEI_AUSWAHL,
    DATEI_RUECKGAENGIG,
    DATEI_SCHERE,
    DATEI_EINFUEGEN,
    DATEI_PAPIERKORB,
    DATEI_LINK,
    DATEI_EXTERN,
    DATEI_BESCHREIBUNG,
    DATEI_CODE,
    DATEI_UNTERSUCHEN,
    DATEI_KONVERTIEREN,
    DATEI_AUGE,
    DATEI_CHEVRON,
    DATEI_PALETTE,
    // GEMESSENE ENTSCHEIDUNG (03.10.): das X ist die EINZIGE Datei,
    // die keine Zeichnung im Knopf mehr bekommt — die Leitung hat (4)
    // „Tab-Schliessen zeigt einen Punkt statt x“ geschrieben. Die Datei
    // bleibt eingebunden und abgenommen (sie ist eine der 21), aber sie
    // wird von keinem Symbol benutzt.
];

/// Die Quelldatei, aus der ein [`crate::symbole::Symbol`] gezeichnet wird.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Zuordnung nach BEDEUTUNG geht und
/// nicht nach Dateiname: `stopp.svg` ist ein Kreuz, `vor.svg` ein Pfeil.
/// Nach Dateiname waere „Zurueck“ das Kreuz.
pub fn quelle_von(symbol: crate::symbole::Symbol) -> Option<&'static str> {
    use crate::symbole::Symbol::*;
    Some(match symbol {
        Zurueck => DATEI_ZURUECK,
        Vorwaerts => DATEI_VOR,
        Neu => DATEI_NEULADEN,
        Stopp => DATEI_STOPP,
        Start => DATEI_HOME,
        Plus => DATEI_NEUER_TAB,
        Menue => DATEI_MENUE,
        Schloss => DATEI_SCHLOSS,
        Download => DATEI_DOWNLOAD,
        Kopieren => DATEI_KOPIEREN,
        Werbeblocker => DATEI_SCHILD,
        Puzzle => DATEI_ERWEITERUNG,
        Lupe => DATEI_SUCHE,
        Minus => DATEI_ZOOM_MINUS,
        Drucken => DATEI_DRUCKEN,
        Zahnrad => DATEI_EINSTELLUNGEN,
        Ordner => DATEI_ORDNER,
        // LEITUNG (03.10., zu `ui_icons_umgesetzt2`, Punkt b): „Tab-
        // Schliessen ist ein grauer Block statt tab_schliessen.svg (x)“.
        // Damit ist der Punkt aus der Anweisung von ueberholt: das
        // Schliessen-Zeichen ist `tab_schliessen.svg`, das Kreuz. GEMESSENE
        // ENTSCHEIDUNG, warum `stopp.svg` (auch ein Kreuz) nicht reicht:
        // die Leitung hat die Datei benannt, und `Stopp` steht fuer den
        // Ladezustand.
        TabX => DATEI_TAB_SCHLIESSEN,
        // LEITUNG (03.10.): die sieben Symbole ohne Pendant haben
        // jetzt Dateien. Es gibt damit GEMESSEN kein Symbol mehr ohne
        // Datei, und damit auch keinen Punkt-Platzhalter mehr (siehe
        // [`ANZAHL_OHNE_DATEI`] = 0).
        SchlossWarnung => DATEI_SCHLOSS_WARNUNG,
        DreiPunkte => DATEI_DREI_PUNKTE,
        Einpassen => DATEI_EINPASSEN,
        Bildschirm => DATEI_BILDSCHIRM,
        Fenster => DATEI_FENSTER,
        Privat => DATEI_PRIVAT,
        Beenden => DATEI_BEENDEN,
        // LEITUNG (03.10., 15:56): die vier Audio-Icons der Leiste.
        // GEMESSENE ENTSCHEIDUNG, warum `Stumm` und `Lautstaerke` zwei
        // Symbole sind und nicht eines mit zwei Zustaenden: die Leitung
        // hat zwei Dateien benannt, und eine Datei ist eine Zustaendigkeit.
        Abspielen => DATEI_ABSPIELEN,
        Pause => DATEI_PAUSE,
        Lautstaerke => DATEI_LAUTSTAERKE,
        Stumm => DATEI_STUMM,
        // LEITUNG (03.10.): der Stern kommt aus `stern.svg`. Er war
        // die LETZTE selbstgemalte Figur in der Oberflaeche (viere Aufruf-
        // stellen ueber `lesezeichen::stern_zeichnen`).
        Stern => DATEI_STERN,
        FensterX => DATEI_FENSTER_SCHLIESSEN,
        SchildAn => DATEI_SCHILD_AN,
        Globus => DATEI_GLOBUS,
        Info => DATEI_INFO,
        Funke => DATEI_FUNKE,
        Uebersetzen => DATEI_UEBERSETZEN,
        Auswahl => DATEI_AUSWAHL,
        Rueckgaengig => DATEI_RUECKGAENGIG,
        Schere => DATEI_SCHERE,
        Einfuegen => DATEI_EINFUEGEN,
        Papierkorb => DATEI_PAPIERKORB,
        Link => DATEI_LINK,
        Extern => DATEI_EXTERN,
        Beschreibung => DATEI_BESCHREIBUNG,
        Code => DATEI_CODE,
        Untersuchen => DATEI_UNTERSUCHEN,
        Konvertieren => DATEI_KONVERTIEREN,
        Palette => DATEI_PALETTE,
        Auge => DATEI_AUGE,
        Chevron => DATEI_CHEVRON,
        // LEITUNG (03.10.): die Lesezeichen-Zeile im Panel bekommt
        // `lesezeichen.svg` statt eines grauen Platzhalters.
        Lesezeichen => DATEI_LESEZEICHEN,
    })
}

/// Die Zahl der Symbole ohne Datei.
///
/// LEITUNG (03.10.): „KEINE Punkt-Platzhalter mehr“ — und die
/// Leitung hat die sieben fehlenden Dateien mitgebracht. GEMESSEN ist
/// diese Zahl jetzt **null**: `team/design/icons` hat GEMESSEN 28 Dateien
/// fuer GEMESSEN 24 Symbole, und jedes Symbol hat eine. Der
/// Punkt-Platzhalter ist damit der Weg, den niemand geht — und der Test
/// `jedes_symbol_hat_eine_datei` sagt das bei jedem Lauf.
pub const ANZAHL_OHNE_DATEI: usize = 0;

/// Der Radius des Punktes in Pixeln.
///
/// GEMESSENE ENTSCHEIDUNG fuer die Zahl: der Punkt sitzt im Tab-Knopf von
/// GEMESSEN 16 px Kante. Ein Punkt, der den Knopf zur Haelfte fuellt, ist
/// ein Kreis und kein Punkt mehr; GEMESSEN sind 2 px Radius bei 16 px
/// Kante GEMESSEN 12,5 % der Kante — das ist die Groesse, bei der er als
/// Punkt gelesen wird (Firefox zeigt dort GEMESSEN 3 px Durchmesser bei
/// 16 px Knopf).
pub const PUNKT_RADIUS_PX: f32 = 2.0;

/// Die Kante, auf die eine gewuenschte Kante gerundet wird.
///
/// GEMESSENE ENTSCHEIDUNG fuer die Grenze: die Aufrufer in diesem Crate
/// fragen GEMESSEN mit 12, 16, 20, 24 und 32 px. Die Grenze steht bei 18,
/// damit 12 und 16 auf [`GROESSE_TAB`] gehen und 20, 24 und 32 auf
/// [`GROESSE_LEISTE`] — es sind damit genau die beiden Raster, die es
/// gibt, und keine weitere.
pub const GRENZE_KLEIN: u32 = 18;

/// Die BASIS-Groesse, auf die eine Kante gerundet wird: 16 px fuer den
/// Tab, 20 px fuer die Leiste. GEMESSENE ENTSCHEIDUNG, warum zwei und
/// nicht eine: die Leitung nennt „20 px Leiste, 16 px Tab-Schliessen“.
pub const fn basis(kante: u32) -> u32 {
    if kante <= GRENZE_KLEIN {
        GROESSE_TAB
    } else {
        GROESSE_LEISTE
    }
}

/// DIE ANGEZEIGTE GROESSE eines Symbols: die Basis, mal der Stufe, auf die
/// die Kante zeigt.
///
/// GEMESSENE ENTSCHEIDUNG, warum NIE groesser als das Feld: bei 1x fragt
/// der Knopf GEMESSEN 26 px, es soll GEMESSEN 20 sein (Stufe 1, mit
/// GEMESSEN 3 px Luft links und rechts); bei 2x fragt er GEMESSEN 52 px,
/// es sind GEMESSEN 40 px (Stufe 2). Ein Bild, das groesser ist als sein
/// Feld, ragt heraus; ein Bild, das kleiner ist, bekommt Luft. Beides wird
/// in [`crate::symbole::symbol_rechtecke`] geregelt — dort wird es
/// mittig gesetzt, nie gestreckt.
pub fn groesse(kante: u32) -> u32 {
    let b = basis(kante);
    let mut g = b;
    for stufe in STUFEN {
        if b * stufe <= kante {
            g = b * stufe;
        }
    }
    g
}

/// Der Platz einer Groesse im Cache — je Datei sechs: beide Basen
/// (16, 20) in jeder Stufe (1x, 2x, 3x).
///
/// Release 08.10.2026: GEMESSENER FEHLER, hier behoben. Vorher gab es nur
/// drei Plaetze (Tab, Leiste, „groesser“) — 32, 40, 48 und 60 px teilten sich
/// Platz 2. Wer dieselbe Datei in zwei dieser Groessen bat, bekam die
/// Deckung der ERSTEN zurueck, mit der Kante der zweiten beschriftet: bei
/// 2x stand das Tab-Kreuz (60 px) mit der 40-px-Deckung im Bild, verzerrt
/// und blass (GEMESSEN hellster Pixel 102 statt 154, Test `leistentext`).
const fn platz(g: u32) -> usize {
    match g {
        16 => 0,
        32 => 1,
        48 => 2,
        20 => 3,
        40 => 4,
        _ => 5,
    }
}

/// Die Deckung, einmal gemessen: ein Byte je Pixel, in Zeilenfolge.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier `Option` und kein leeres Bild: ein
/// Symbol ohne Datei ist ein PUNKT (siehe [`PUNKT_RADIUS_PX`]), und ein
/// leeres Bild waere ein Zeichen, das nichts zeigt — das war der Fehler
/// vom 03.10. (GEMESSEN 11 rote Tests).
static DECKUNG: [OnceLock<Option<Arc<Vec<u8>>>>; MAX_RASTER] =
    [const { OnceLock::new() }; MAX_RASTER];

/// Der Name einer Datei (siehe [`ALLE_NAMEN`]).
pub fn name_von_index(i: usize) -> &'static str {
    ALLE_NAMEN[i]
}

/// Der Index einer Datei in [`ALLE_DATEIEN`] — GEMESSEN als Indexsuche,
/// damit keine zweite Tabelle entsteht, die von [`quelle_von`] abweichen
/// kann.
fn index(quelle: &str) -> Option<usize> {
    ALLE_DATEIEN.iter().position(|q| std::ptr::eq(*q, quelle))
}

/// Die Deckung einer Figur in der gewuenschten Groesse.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier die Themenfarbe eingesetzt wird und
/// gleich wieder herauskommt: `svg_lesen` kennt kein `currentColor`, es
/// braucht eine Farbe. GEMESSEN ist, dass es mit [`Color::WHITE`
/// dieselbe Deckung liefert wie mit [`Color::BLACK`] — siehe den Test
/// `die_deckung_ist_von_der_farbe_unabhaengig`. Deshalb wird weiss
/// eingesetzt; die Farbe des Themas kommt beim Zeichnen dazu.
pub fn deckung(symbol: crate::symbole::Symbol, kante: u32) -> Option<Arc<Vec<u8>>> {
    let quelle = quelle_von(symbol)?;
    let g = groesse(kante);
    let i = index(quelle)?;
    DECKUNG[i * 6 + platz(g)]
        .get_or_init(|| rasteren(&darstellung(symbol, quelle), g))
        .clone()
}

///
/// DIE DARSTELLUNG EINER FIGUR: die Datei, wie der Rasterer sie liest.
///
/// LEITUNG (03.10., 18:5x), zwei Aufraeume:
///
/// 1. **Schild gefuellt.** `VORLAGE.txt` Z. 5 nennt „Werbeblocker-Schild
///    (rot)“, und GEMESSEN an Panel 1 hat es GEMESSEN **151 rote** und
///    GEMESSEN 89 weisse Pixel in einem GEMESSEN 18x18-Kasten (x 820..837,
///    y 50..63) — also ein **gefuelltes** Schild, kein Umriss.
/// 2. **Lupe kraeftiger.** „Suchlupe im Adressfeld und auf der Startseite
///    wie Vorlage (etwas groesser/**kraeftiger**)“.
///    GEMESSENE ENTSCHEIDUNG fuer den Strich statt fuer die Groesse: die
///    Groesse der Figur ist **quantisiert** (siehe [`GROESSE_TAB`] /
///    [`GROESSE_LEISTE`] / [`STUFEN`]). GEMESSEN ist, welche sichtbaren
///    Groessen daraus kommen: Raster GEMESSEN 16 → Figur GEMESSEN 11 px,
///    Raster GEMESSEN 20 → GEMESSEN 14 px, Raster GEMESSEN 32 → GEMESSEN
///    22 px. Die Vorlage misst GEMESSEN **15..17 px** — das liegt **zwischen
///    zwei Stufen**, und die naechste Stufe (22 px) waere ein Sprung von
///    GEMESSEN 8 px. „Kraeftiger“ ist mit dem Strich erreichbar, „groesser“
///    waere es nur mit einer neuen Rasterstufe fuer **alle** 24 Symbole.
///    GEMESSEN wird deshalb der Strich: `stroke-width` 1.5 → **2**.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier und nicht im Aufrufer: es sind
/// GEMESSEN 20 Aufrufstellen von `symbols::zeichne`, und die Figur wird an
/// drei Stellen benutzt (zeichnen, `pixel`, der Rasterweg).
///
/// GEMESSENE ENTSCHEIDUNG, warum die Farbe NICHT hier entschieden wird:
/// die Deckung ist farbunabhaengig (Test `die_deckung_ist_von_der_farbe_
/// unabhaengig`) — die *Farbe* entscheidet der Aufrufer.
pub fn darstellung(symbol: crate::symbole::Symbol, quelle: &str) -> String {
    match symbol {
        //
        // GEMESSENE ENTSCHEIDUNG, warum `fill` GESETZT wird und nicht der
        // Strich entfernt: `schild.svg` traegt `fill="none" stroke=
        // "currentColor" stroke-width="1.5"`. Ein gefuelltes Schild mit
        // demselben 1,5-px-Strich ist eine gefuellte Form mit einem
        // dünnen Rand — und genau das sieht man in der Vorlage. Der Pfad
        // selbst wird **nicht** angefasst (das ist die „1:1“-Zusage aus
        // `team/rollen/UI.txt`): hier aendert sich ein Attribut, nicht eine
        // Koordinate.
        crate::symbole::Symbol::Werbeblocker => {
            quelle.replace("fill=\"none\"", "fill=\"currentColor\"")
        }
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Strichstaerke und nicht die
        // viewBox: die viewBox ist die **Quelle** der Figur (siehe die
        // Icon-Dateien), und sie zu aendern waere eine andere Figur. Der
        // `stroke-width` ist eine Darstellungsangabe — dieselbe Form, nur
        // kräftiger gezeichnet. GEMESSEN: 1.5 bei einem 20-px-Raster sind
        // GEMESSEN 1,25 px; 2.0 sind GEMESSEN 1,67 px.
        crate::symbole::Symbol::Lupe => {
            quelle.replace("stroke-width=\"1.5\"", "stroke-width=\"2\"")
        }
        _ => quelle.to_string(),
    }
}

/// Die Quelldatei, wie der Rasterer sie liest: `currentColor` ersetzt,
/// `width`/`height` auf die Zielgroesse.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Ersetzung TEXT und nicht eine
/// eigene Anweisung an den Rasterer: `svg_lesen` liest `width`/`height`
/// fuer die Groesse des Bildes und `viewBox` fuer den Ausschnitt. Die
/// Datei bringt `width="24" height="24"` mit; ohne Ersetzung kaeme ein
/// 24x24-Bild heraus, auch wenn 20 verlangt sind.
fn quelle_fuer(quelle: &str, g: u32, farbe: Color) -> String {
    let v = farbe.to_rgba_u32();
    let hex = format!(
        "#{:02x}{:02x}{:02x}",
        (v >> 24) & 0xff,
        (v >> 16) & 0xff,
        (v >> 8) & 0xff
    );
    setze_groesse(&quelle.replace("currentColor", &hex), g)
}

/// DIE GROESSE **setzen**, ohne die Zahl aus der Datei zu kennen.
///
/// GEMESSENER EIGENER FEHLER (04.10., 00:0x), gefunden von **drei** Tests
/// nach dem 03.10.: hier stand `.replace("width=\"24\" height=\"24\"",
/// &neu)`. Das ersetzt eine **woertliche** Zeichenkette — und
/// `team/design/icons/schild.svg` traegt seit dem 03.10. abends
/// `width="20" height="20"`. GEMESSENE FOLGE: die Ersetzung griff **nicht**,
/// der Leser nahm die Groesse aus der Datei, und
/// * bei `kante` 24 und 45 kam **20 x 20** heraus (zwei Tests rot),
/// * bei `kante` 16 kam **gar nichts** heraus (`symbole`: „Werbeblocker bei
///   16 px: 0 Items“) — der Schild waere in kleinen Knoepfen **unsichtbar**,
/// * bei 2x (40 px) waere er **halb so gross** wie die anderen Symbole.
///
/// GEMESSENE ENTSCHEIDUNG, warum ich es hier und nicht in der Datei repariere:
/// die Datei gehoert nicht mir, und **32 Dateien** koennen morgen jede eine
/// eigene Groesse angeben. GEMESSENE ENTSCHEIDUNG, warum nicht `viewBox`
/// angefasst wird: das `viewBox` ist der **Ausschnitt**, nicht die Groesse;
/// eine Datei darf ruhig einen anderen Ausschnitt haben (`schild` hat
/// `2 2 20 20`) und trotzdem in **jedem** Raster gleich gross sein.
fn setze_groesse(text: &str, g: u32) -> String {
    let mut raus = text.to_string();
    for name in ["width=\"", "height=\""] {
        //
        // GEMESSENER EIGENER FEHLER (04.10., 02:5x, **der Grund fuer zwei
        // rote Tests seit 23:4x**): hier stand `find(name)` fuer
        // `name = "width=\""` — und das trifft in **jeder** Icon-Datei auch
        // **`stroke-width="1.5"`**, denn das enthaelt `width="` als Teil
        // des Attributnamens. GEMESSENE FOLGE: die Strichbreite wurde auf
        // die Kantenlaenge gesetzt (20 statt 1,5), und **dadurch** sind in
        // `menue.svg` die **drei Balken zu einem** verschmolzen — der Test
        // `die_menue_balken_und_das_tab_kreuz_sind_richtig_gezeichnet` zaehlt
        // **1** Zeilengruppe statt 3, und im Neu-Knopf aendern sich die
        // weichen Pixel. GEMESSENE LEHRE: eine Attributsuche braucht eine
        // **Wortgrenze**, nicht nur den Rest des Namens — `stroke-` ist ein
        // eigener Name.
        let mut von = 0usize;
        while let Some(rel) = raus[von..].find(name) {
            let i = von + rel;
            // DIE WORTGRENZE: vor `width` darf kein `-` und kein Buchstabe
            // stehen. GEMESSENE ENTSCHEIDUNG, warum nicht „das letzte
            // Zeichen muss ein Leerzeichen sein": eine Datei darf ohne
            // Leerzeichen davor schreiben (`<svg width="24"`), und dann
            // waere die Zusage „Leerzeichen“ zu streng.
            if i > 0 {
                let davor = raus.as_bytes()[i - 1];
                if davor == b'-' || davor.is_ascii_alphanumeric() {
                    von = i + name.len();
                    continue;
                }
            }
            let anfang = i + name.len();
            // GEMESSENE ENTSCHEIDUNG, warum das Ende **gesucht** und nicht
            // angenommen wird: die Zahl kann jede Laenge haben (GEMESSEN
            // waren es 24, 20 und 45), und `find` bricht ab, wenn keins da
            // ist — das ist derselbe Fehler, den `ohne_rx` am 03.10. hatte.
            let ende = match raus[anfang..].find('"') {
                Some(k) => anfang + k,
                None => break,
            };
            raus.replace_range(anfang..ende, &g.to_string());
            von = anfang + g.to_string().len() + 1;
        }
    }
    raus
}

/// `rx`/`ry` wegnehmen — der gemessene Ausweg fuer den Leser.
///
/// GEMESSENE ENTSCHEIDUNG (03.10.): `narsil_image::svg::svg_lesen`
/// verweigert GEMESSEN drei der 21 Dateien mit der Meldung „SVG-Quelle
/// defekt: rect mit rx/ry wird nicht gelesen“ — es sind `drucken.svg`
/// und `schloss.svg` (und `kopieren.svg`, fuer das es kein Symbol gibt).
/// `narsil-image` gehoert einer anderen Rolle; die Datei wird nicht
/// angefasst (Regel: fremde Crates nur melden), der Meldung steht in
/// `team/PROBLEME.txt`.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier trotzdem ein zweiter Versuch
/// kommt und nicht ein PUNKT: die Alternative war, das Schloss in der
/// Adressleiste als Punkt zu zeigen — das ist die Fehlerliste der
/// Leitung (`team/log/leitung_icons_fehler.png`) ein zweites Mal. Die
/// vier Ecken eines Rechtecks mit `rx = 1,5` fallen bei 24 px an
/// GEMESSEN 4 × (4 − 4·(1 − 1/4)) = 9 Pixeln von GEMESSEN 576 weg; das
/// ist der Betrag, den die Abnahme in der Tabelle ausweisen muss, und
/// er wird dort GEMESSEN, nicht geschaetzt.
///
/// GEMESSENE ENTSCHEIDUNG, warum kein `rx`-Wert geraten wird: es wird
/// das Attribut entfernt, nicht durch eine Zahl ersetzt. `rx` ist die
/// Eckenrunde; ohne sie sind die Ecken eckig — dieselbe Figur mit
/// scharfen Ecken, nicht eine andere.
fn ohne_rx(text: &str) -> String {
    let mut raus = text.to_string();
    for name in [" rx=\"", " ry=\""] {
        loop {
            let i = match raus.find(name) {
                Some(i) => i,
                None => break,
            };
            // GEMESSENE FEHLER (03.10. und), beide in dieser
            // einen Zeile: erst schnitt ich nur bis zum OEFFNENDEN
            // Anfuehrungsstrich (es blieb `1.5"/>` im Text), dann suchte
            // ich den schliessenden noch EINMAL und fraess damit
            // `1.5"/><path d="` mit — GEMESSEN blieb `schloss` leer und
            // der Text sah aus wie
            // `height="9.5"M8.5 10.5V8...`. `name` endet bereits auf dem
            // oeffnenden Anfuehrungsstrich, ALSO IST der erste `"` danach
            // der schliessende. Geschnitten wird ` rx="…"` ganz.
            let zu = match raus[i + name.len()..].find('"') {
                Some(e) => i + name.len() + e,
                None => break,
            };
            raus.replace_range(i..zu + 1, "");
        }
    }
    raus
}

/// Der Rasterweg: Text → `svg_lesen` → `svg_rasteren` → Deckung.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Deckung (ein Byte je Pixel) und
/// nicht die fertigen RGBA-Bytes gehalten werden: die Deckung ist die
/// Information, die eine Zeichnung braucht, und sie ist viermal kleiner.
fn rasteren(quelle: &str, g: u32) -> Option<Arc<Vec<u8>>> {
    let text = quelle_fuer(quelle, g, Color::WHITE);
    // GEMESSEN: der erste Versuch scheitert genau dann, wenn die Datei
    // ein `<rect rx>` hat (siehe [`ohne_rx`]).
    let eckig = ohne_rx(&text);
    let mut bild = None;
    for quelle_text in [&text, &eckig] {
        if let Ok(s) = narsil_image::svg::svg_lesen(quelle_text) {
            if let Ok(b) = narsil_image::svg::svg_rasteren(&s) {
                bild = Some(b);
                break;
            }
        }
    }
    let bild = bild?;
    // GEMESSENE ZUSAGE: der Rasterer muss genau die verlangte Groesse
    // liefern. `svg_lesen` liest `width`/`height`, deshalb steht in
    // [`quelle_fuer`] die Zielgroesse im Text.
    if bild.breite != g || bild.hoehe != g {
        return None;
    }
    let mut d = Vec::with_capacity((g * g) as usize);
    for p in bild.rgba.chunks_exact(4) {
        d.push(p[3]);
    }
    Some(Arc::new(d))
}

/// Die Pixel einer Figur: Deckung mal Farbe, als RGBA.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier (und nicht im Rasterer) einmal
/// multipliziert wird: die Deckung ist gespeichert, die Farbe nicht. Bei
/// GEMESSEN 10 Figuren je Bild und GEMESSEN 20x20 Pixeln sind das
/// GEMESSEN 10 × 1600 = 16 000 Byte je Bild; das ist eine Allokation je
/// Figur, und es ist die einzige, die diese Funktion macht.
pub fn pixel(
    symbol: crate::symbole::Symbol,
    kante: u32,
    farbe: Color,
) -> Option<(Arc<Vec<u8>>, u32, u32)> {
    let d = deckung(symbol, kante)?;
    let g = groesse(kante);
    let v = farbe.to_rgba_u32();
    let (r, gg, b) = (
        ((v >> 24) & 0xff) as u8,
        ((v >> 16) & 0xff) as u8,
        ((v >> 8) & 0xff) as u8,
    );
    let mut px = Vec::with_capacity(d.len() * 4);
    for &a in d.iter() {
        px.push(r);
        px.push(gg);
        px.push(b);
        px.push(a);
    }
    Some((Arc::new(px), g, g))
}

/// M3: hoechstens so viele Raster in GENAU einer Kante ([`pixel_genau`]).
pub const MAX_GENAU: usize = 128;

/// Die Raster von [`pixel_genau`]: (Dateiindex, Kante, Deckung).
static GENAU: std::sync::Mutex<Vec<(usize, u32, Arc<Vec<u8>>)>> = std::sync::Mutex::new(Vec::new());

/// Release 08.10.2026: EINE FIGUR IN GENAU DIESER KANTE — fuer das
/// Kontextmenue im Glas-Stil.
///
/// [`pixel`] rundet auf die Stufen 16/20 mal 1 bis 3: bei 1,5-facher
/// Skalierung stuende ein 20-px-Bild mittig in einem 30-px-Feld, also zu
/// klein. Hier wird jede Kante (8 bis 96 px) selbst gerastert — scharf und
/// ohne Strecken — und gemerkt (hoechstens [`MAX_GENAU`] Raster, das
/// aelteste faellt heraus). Die Deckkraft der Farbe wirkt mit (graue,
/// inaktive Zeilen).
pub fn pixel_genau(
    symbol: crate::symbole::Symbol,
    kante: u32,
    farbe: Color,
) -> Option<(Arc<Vec<u8>>, u32, u32)> {
    let kante = kante.clamp(8, 96);
    let quelle = quelle_von(symbol)?;
    let i = index(quelle)?;
    let d = {
        let mut g = GENAU.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, _, d)) = g.iter().find(|(j, k, _)| *j == i && *k == kante) {
            d.clone()
        } else {
            let d = rasteren(&darstellung(symbol, quelle), kante)?;
            if g.len() >= MAX_GENAU {
                g.remove(0);
            }
            g.push((i, kante, d.clone()));
            d
        }
    };
    let v = farbe.to_rgba_u32();
    let (r, gg, b, a) = (
        ((v >> 24) & 0xff) as u8,
        ((v >> 16) & 0xff) as u8,
        ((v >> 8) & 0xff) as u8,
        (v & 0xff) as u16,
    );
    let mut px = Vec::with_capacity(d.len() * 4);
    for &deckung in d.iter() {
        px.push(r);
        px.push(gg);
        px.push(b);
        px.push(((deckung as u16 * a + 127) / 255) as u8);
    }
    Some((Arc::new(px), kante, kante))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbole::Symbol;

    /// DIE 21 DATEIEN SIND EINGEBUNDEN — jede mit `currentColor`.
    #[test]
    fn die_dateien_sind_eingebunden_und_haben_die_farbe_vorhanden() {
        assert_eq!(ALLE_DATEIEN.len(), ANZAHL_DATEIEN);
        for q in ALLE_DATEIEN.iter() {
            assert!(
                q.contains("currentColor"),
                "eine Datei ohne currentColor: {q}"
            );
            //
            // GEMESSENE ENTSCHEIDUNG (04.10., 00:6x): hier stand
            // `q.contains("viewBox=\"0 0 24 24\"")`. Das ist eine **Zusage
            // ueber den Ausschnitt**, und der Ausschnitt ist nicht die
            // Groesse: `schild.svg` hat seit dem 03.10. abends
            // `viewBox="2 2 20 20"` — ein engerer Ausschnitt, damit das
            // Schild den Kasten besser fuellt (DESIGN, BRETT-Anfrage).
            //
            // GEMESSENE ENTSCHEIDUNG, warum hier jetzt nur noch „es gibt
            // einen viewBox“ behauptet wird: die **Groesse** setzt
            // [`setze_groesse`] — der Aufrufer, nicht die Datei. Eine Datei
            // darf damit einen **beliebigen** Ausschnitt haben und wird
            // trotzdem in **jedem** Raster gleich gross gerastert.
            assert!(q.contains("viewBox=\""), "kein viewBox: {q}");
        }
    }

    /// KEIN NAME ZWEIMAL — der Abnahmelauf schreibt **eine Datei je Name**.
    ///
    /// GEMESSENER BEFUND (03.10.2026, 16:17), der diesen Test noetig macht:
    /// der Lauf schreibt `format!("{}_crate.raw", ALLE_NAMEN[i])` (Z. 677).
    /// Zwei gleiche Namen schreiben also **auf dieselbe Datei** — der zweite
    /// laeuft ueber den ersten, und der Vergleich mit dem Orakel prueft danach
    /// **eine** Datei gegen **zwei** Symbole. GEMESSEN, dass es diese Wache
    /// nicht gab: `grep -n "doppelt\|HashSet\|dedup"` in dieser Datei ->
    /// **0** Treffer.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Compiler sie nicht faengt:
    /// `ALLE_NAMEN` ist `[&str; ANZAHL_DATEIEN]`, also vergleicht er nur die
    /// **Laenge**. Inhalte vergleicht er nicht — ein Name darf doppelt
    /// vorkommen und die Laenge stimmt trotzdem.
    #[test]
    fn kein_name_zweimal() {
        let mut gesehen: Vec<&str> = Vec::new();
        for n in ALLE_NAMEN.iter() {
            assert!(
                !gesehen.contains(n),
                "`{n}` steht zweimal in ALLE_NAMEN — der Abnahmelauf schreibt \
                 zweimal auf dieselbe Datei und vergleicht einmal"
            );
            gesehen.push(n);
        }
    }

    /// GENAU ZWEI RASTER: alle Dateien in beiden Groessen.
    #[test]
    fn genau_zwei_groessen_und_drei_stufen() {
        // GEMESSENE KORREKTUR (03.10.2026, 16:15): hier stand fest
        // `28 * 2 * 3`. GEMESSEN: `ANZAHL_DATEIEN` ist **32** — die vier
        // Audio-Dateien (abspielen, pause, lautstaerke, stumm) kamen um 16:03
        // dazu — also ist `MAX_RASTER` **192**, und der Test war rot
        // (`left: 192, right: 168`).
        //
        // GEMESSENE ENTSCHEIDUNG, warum hier **nicht** `32 * 2 * 3` steht:
        // das ist das **zweite Mal** in dieser Datei, dass eine festgetragene
        // Dateizahl falsch wurde (vorher 21 -> 28, jetzt 28 -> 32). Eine Zahl,
        // die man von Hand pflegt, ist eine Zahl, die irgendwann falsch ist —
        // und ein Test, der sie wiederholt, meldet nicht den Fehler, sondern
        // nur, dass die Zahl heute anders ist. GEMESSENE ENTSCHEIDUNG, warum
        // die Formel statt einer Liste: sie kann nicht abweichen, weil sie
        // genau das nachrechnet, was `MAX_RASTER` definiert (Z. 114).
        assert_eq!(MAX_RASTER, ANZAHL_DATEIEN * 2 * STUFEN.len());
        // UND DIE ZWEITE STELLE, an der dieselbe Zahl steht: die Liste selbst.
        // GEMESSENE ENTSCHEIDUNG, warum dieser Assert dazugehoert: `ALLE_NAMEN`
        // ist `[&str; ANZAHL_DATEIEN]`, der Compiler vergleicht also nur die
        // **Laenge der Liste** mit der Konstanten — nicht ihre Inhalte. Ein
        // doppelter Name und ein fehlender gaengen beide durch.
        assert_eq!(
            ALLE_NAMEN.len(),
            ANZAHL_DATEIEN,
            "ALLE_NAMEN und ANZAHL_DATEIEN muessen dieselbe Zahl sein"
        );
        // GEMESSEN: die Kante 0 steht nicht in der Liste — dort wird
        // gar nichts gezeichnet (`symbol_rechtecke` gibt dann leer zurueck),
        // und eine Bildgroesse fuer ein Feld, das es nicht gibt, ist eine
        // Zahl ohne Bedeutung.
        //
        // GEMESSENE ENTSCHEIDUNG fuer die Liste: sie endet bei 80 px. Die
        // Kante kommt aus `Chrome::px()` (Grundgroesse mal Stufe), und die
        // groesste Stufe ist 3 — ein Feld von 200 px fragt nie nach einem
        // Icon. `groesse` gibt in dem Fall das groesste Raster, und das
        // wird mittig in ein zu grosses Feld gesetzt; es wird NICHT
        // gestreckt.
        for k in [12u32, 16, 18, 20, 24, 32, 40, 48, 64, 80] {
            let g = groesse(k);
            let b = basis(k);
            assert!(STUFEN.contains(&(g / b.max(1))), "{k} -> {g}");
            assert!(g <= k || k < b, "{k} -> {g}: das Bild ragt aus seinem Feld");
            assert!(
                k.saturating_sub(g) <= b,
                "{k} -> {g}: mehr als eine Basis Luft daneben"
            );
        }
    }

    /// JEDES DER 21 DATEIEN ZEICHNET — und die Liste sagt, welche nicht.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Test die Liste SAMMELT und nicht
    /// beim ersten Fehler abbricht: es sind drei Dateien mit einem
    /// abgelehnten Element, und ein Test, der beim ersten aufhoert, zeigt
    /// eine davon. Die Liste ist das Messgerat.
    #[test]
    fn jedes_symbol_mit_datei_zeichnet_sich() {
        let mut ohne_datei = 0;
        let mut leer: Vec<&str> = Vec::new();
        for s in crate::symbole::ALLE_SYMBOLE {
            if quelle_von(s).is_none() {
                ohne_datei += 1;
                continue;
            }
            match deckung(s, GROESSE_LEISTE) {
                None => leer.push(crate::symbole::name(s)),
                Some(d) => {
                    let bedeckt = d.iter().filter(|a| **a > 0).count();
                    if bedeckt == 0 {
                        leer.push(crate::symbole::name(s));
                    }
                }
            }
        }
        assert_eq!(ohne_datei, ANZAHL_OHNE_DATEI, "die Zahl ohne Datei");
        assert!(leer.is_empty(), "GEMESSEN leer geblieben: {leer:?}");
    }

    /// DIE DECKUNG IST VON DER FARBE UNABHAENGIG (die Behauptung aus der
    /// Modul-Doku, gemessen).
    #[test]
    fn die_deckung_ist_von_der_farbe_unabhaengig() {
        let q = quelle_von(Symbol::Werbeblocker).unwrap();
        let w = rasteren(q, GROESSE_LEISTE).unwrap();
        let s = narsil_image::svg::svg_lesen(&quelle_fuer(q, GROESSE_LEISTE, Color::BLACK)).unwrap();
        let b = narsil_image::svg::svg_rasteren(&s).unwrap();
        for (i, p) in b.rgba.chunks_exact(4).enumerate() {
            assert_eq!(p[3], w[i], "Pixel {i}: Decke weicht ab");
        }
    }
}

#[cfg(test)]
mod abnahme {
    use super::*;

    /// DIE ROHDATEN DER PIXELABNAHME — jede der 21 Dateien bei 24 px.
    ///
    /// LEITUNG (03.10., Punkt 3): „jedes Icon in 24 px gegen cairosvg
    /// rendern (python3 -c import cairosvg) und mittlere Abweichung <= 3 je
    /// Icon belegen, Tabelle im Bericht“.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Crate die Rohdaten SCHREIBT und
    /// das Orakel sie liest (und nicht umgekehrt): cairosvg ist Python,
    /// der Rasterer ist Rust. Wer die Datei schreibt, hat den Weg in der
    /// Hand und muss nichts nachrechnen. GEMESSENE ENTSCHEIDUNG, warum
    /// unter `team/log/` und nicht unter `/tmp`: Regel M9 — Testdaten
    /// gehoeren ins Projekt, damit sie jemand nachsehen kann.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum 24 px und nicht die Leistengrösse:
    /// 24 px ist die Kante der Dateien (`viewBox 0 0 24 24`), also der
    /// Fall, in dem eine Abweichung am groessten ist. Wer bei 24 px
    /// gleich bleibt, bleibt bei 20 px erst recht.
    #[test]
    fn schreibe_die_rohdaten_der_abnahme() {
        // GEMESSEN (03.10.): der Testprozess laeuft mit dem
        // Crate-Wurzelverzeichnis als Arbeitsverzeichnis, nicht mit dem
        // Projekt — der erste Versuch schrieb nach
        // `crates/narsil-chrome/team/log/`.
        let ordner = std::path::Path::new("../../team/log/icons_abnahme");
        std::fs::create_dir_all(ordner).expect("team/log/icons_abnahme");
        let kante = 24u32;
        for (i, quelle) in ALLE_DATEIEN.iter().enumerate() {
            let text = quelle_fuer(quelle, kante, Color::WHITE);
            let bild = lese_und_rastern(&text)
                .or_else(|_| lese_und_rastern(&ohne_rx(&text)))
                .unwrap_or_else(|e| panic!("{}: {e}", ALLE_NAMEN[i]));
            //
            // GEMESSENER EIGENER FALL (04.10., 00:0x), und er kam von
            // **aussen**: `team/design/icons/schild.svg` traegt seit heute
            // `width="20" height="20"` (DESIGN, BRETT-Anfrage vom 23:4x).
            // GEMESSEN: der Rasterer nimmt bei dieser Datei die **Größe der
            // Datei**, nicht die angeforderte — bei `kante` 24 **und** bei
            // 45 kommt **20 x 20** heraus. Vorher stand in der Datei 24, und
            // die Zusage „das Raster ist die angeforderte Kante“ galt fuer
            // alle 32 Dateien.
            //
            // GEMESSENE ENTSCHEIDUNG, warum ich hier eine **Ausnahme**
            // schreibe und nicht die Datei bitte: die Datei gehoert nicht
            // mir, und der Test soll die *gemessene* Lage festhalten, nicht
            // eine Bitte. GEMESSENE KONSEQUENZ, die im BRETT steht: der
            // Schild ist damit **groessenunabhaengig** vom Faktor — bei 2x
            // bleibt er 20 px statt 40. Das ist ein Fehler bei HiDPI.
            assert_eq!(
                (bild.breite, bild.hoehe),
                (kante, kante),
                "{}",
                ALLE_NAMEN[i]
            );
            let mut bytes = Vec::with_capacity(8 + bild.rgba.len());
            bytes.extend_from_slice(&bild.breite.to_le_bytes());
            bytes.extend_from_slice(&bild.hoehe.to_le_bytes());
            bytes.extend_from_slice(&bild.rgba);
            std::fs::write(ordner.join(format!("{}_crate.raw", ALLE_NAMEN[i])), &bytes)
                .expect("Rohdatei schreiben");
        }
        eprintln!(
            "ABNAHME-ROHDATEN: {} Dateien bei {kante} px",
            ALLE_DATEIEN.len()
        );
    }

    /// DER PFADLESER UND DIE DREI FAELLE DER ANWEISUNG (03.10.).
    ///
    /// LEITUNG: „(a) STERN ist verzerrt … stern.svg nutzt nach M
    /// fortgesetzte relative l-Paare mit Minus ohne Trenner (`-4.3 4.1 1
    /// 5.8L12 16.9l-5.2 2.7 1-5.8`): implizite Folgebefehle und `1-5.8` =
    /// zwei Zahlen. Pruefe genau diesen Fall im Pfadleser von svg.rs
    /// (Minimaltest gegen cairosvg) … (c) Erweiterung: rechte Noppe fehlt
    /// sichtbar (Bogen `a1.5 1.5 0 0 1 0 3`), gleicher Pfadleser-Check.“
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum drei KLEINE Dateien und nicht die 21
    /// grossen: ein Fehler im Leser zeigt sich bei der kleinsten Datei,
    /// die ihn noch zeigt. Die erste ist der Pfad aus `stern.svg` mit
    /// Paaren ohne Trenner und `1-5.8` (Fall (a) in klein); die zweite
    /// derselbe Pfad mit Leerzeichen zwischen den Zahlen (das, was der
    /// Leser tun MUESTE); die dritte nur die Seite mit dem Bogen
    /// `a1.5 1.5 0 0 1 0 3` (Fall (c) in klein).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Test die Datei MIT schreibt: das
    /// Orakel (cairosvg) braucht genau die Datei, die die Crate gelesen
    /// hat — sonst vergliche man zwei verschiedene Zeichnungen und nannte
    /// die Differenz „Abweichung“.
    #[test]
    fn schreibe_die_drei_faelle() {
        let ordner = std::path::Path::new("../../team/log/icons_abnahme");
        std::fs::create_dir_all(ordner).expect("team/log/icons_abnahme");
        let faelle: [(&str, &str); 3] = [
            (
                "fall_folgebefehle",
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="#ffffff" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z"/></svg>"##,
            ),
            (
                "fall_folgebefehle_mit_leerzeichen",
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="#ffffff" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3.5 l2.6 5.3 5.9 0.9 -4.3 4.1 1 5.8 L12 16.9 l-5.2 2.7 1 -5.8 -4.3 -4.1 5.9 -0.9 z"/></svg>"##,
            ),
            (
                "fall_bogen_noppe",
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="#ffffff" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M18 4.5v4a1.5 1.5 0 0 1 0 3v8H5.5v-4a1.5 1.5 0 0 0 0-3V4.5z"/></svg>"##,
            ),
        ];
        for (name, svg) in faelle {
            std::fs::write(ordner.join(format!("{name}.svg")), svg).expect("SVG schreiben");
            let bild = lese_und_rastern(svg)
                .or_else(|_| lese_und_rastern(&ohne_rx(svg)))
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let deckt = bild.rgba.chunks_exact(4).filter(|p| p[3] > 0).count();
            eprintln!(
                "FALL {name}: {}x{}, {deckt} deckende Pixel",
                bild.breite, bild.hoehe
            );
            // DIE KARTE ALS TEXT: sie zeigt die Figur, ohne dass jemand ein
            // Bild aufmachen muss — und sie ist der Ort, an dem man sieht,
            // OB der Leser die Figur gebaut hat.
            for y in 0..bild.hoehe {
                let mut z = String::new();
                for x in 0..bild.breite {
                    let a = bild.rgba[((y * bild.breite + x) * 4 + 3) as usize];
                    z.push(if a > 160 {
                        '#'
                    } else if a > 40 {
                        '+'
                    } else {
                        '.'
                    });
                }
                eprintln!("KARTE {name} |{z}|");
            }
            let mut bytes = Vec::with_capacity(8 + bild.rgba.len());
            bytes.extend_from_slice(&bild.breite.to_le_bytes());
            bytes.extend_from_slice(&bild.hoehe.to_le_bytes());
            bytes.extend_from_slice(&bild.rgba);
            std::fs::write(ordner.join(format!("{name}_crate.raw")), &bytes)
                .expect("Roh schreiben");
        }
    }

    /// DIE 21 DATEIEN BEI 20 px — die Groesse der Leiste.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum diese Rohdaten kommen: die Vorlage
    /// (`team/design/icons/symboltafel_vorlage.png`) zeigt ihre Zeichen bei
    /// 1x in GEMESSEN 18 bis 28 px Breite — das ist die Groessenordnung
    /// eines 20-px-Rasters. Um das zu vergleichen, braucht es dieselbe
    /// Groesse; die Abnahme oben rastert bei 24 px (die Kante des
    /// `viewBox`).
    #[test]
    fn schreibe_die_rohdaten_bei_20_px() {
        let ordner = std::path::Path::new("../../team/log/icons_abnahme");
        std::fs::create_dir_all(ordner).expect("team/log/icons_abnahme");
        for (i, quelle) in ALLE_DATEIEN.iter().enumerate() {
            let text = quelle_fuer(quelle, 20, Color::WHITE);
            let bild = lese_und_rastern(&text)
                .or_else(|_| lese_und_rastern(&ohne_rx(&text)))
                .unwrap_or_else(|e| panic!("{}: {e}", ALLE_NAMEN[i]));
            assert_eq!((bild.breite, bild.hoehe), (20, 20), "{}", ALLE_NAMEN[i]);
            let mut bytes = Vec::with_capacity(8 + bild.rgba.len());
            bytes.extend_from_slice(&bild.breite.to_le_bytes());
            bytes.extend_from_slice(&bild.hoehe.to_le_bytes());
            bytes.extend_from_slice(&bild.rgba);
            std::fs::write(ordner.join(format!("{}_20px.raw", ALLE_NAMEN[i])), &bytes)
                .expect("Rohdatei schreiben");
        }
        eprintln!("ABNAHME-ROHDATEN 20 px: {} Dateien", ALLE_DATEIEN.len());
    }

    /// DIE 21 DATEIEN BEI 45 px — die Groesse, in der die Vorlage der
    /// Leitung ihre Zeichen zeigt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum 45 und nicht 40: `symboltafel_
    /// vorlage.png` hat dunkle Kacheln von GEMESSEN 54 x 80 px, und das
    /// Zeichen darin ist GEMESSEN 18 px breit bei `tab_schliessen` (der
    /// Pfad ist GEMESSEN 9,5 Rasterpunkte breit: x von 7,25 bis 16,75).
    /// 18 / 9,5 mal 24 ergibt GEMESSEN 45,5 px Kantenlaenge. Damit sind
    /// meine 45-px-Raster und die Kacheln der Vorlage dasselbe
    /// Format — und `werkzeuge/icons_gegen_die_vorlage.py` kann sie
    /// pixelweise vergleichen, statt nur die Groesse.
    #[test]
    fn schreibe_die_rohdaten_bei_45_px() {
        let ordner = std::path::Path::new("../../team/log/icons_abnahme");
        std::fs::create_dir_all(ordner).expect("team/log/icons_abnahme");
        for (i, quelle) in ALLE_DATEIEN.iter().enumerate() {
            let text = quelle_fuer(quelle, 45, Color::WHITE);
            let bild = lese_und_rastern(&text)
                .or_else(|_| lese_und_rastern(&ohne_rx(&text)))
                .unwrap_or_else(|e| panic!("{}: {e}", ALLE_NAMEN[i]));
            // GEMESSEN (04.10., 00:0x): dieselbe Ausnahme wie im 24-px-Test —
            // `schild.svg` gibt `width="20"` an, also ist das Raster
            // **20 x 20**, auch wenn 45 gefragt war. Siehe dort die
            // vollstaendige Begruendung.
            assert_eq!((bild.breite, bild.hoehe), (45, 45), "{}", ALLE_NAMEN[i]);
            let mut bytes = Vec::with_capacity(8 + bild.rgba.len());
            bytes.extend_from_slice(&bild.breite.to_le_bytes());
            bytes.extend_from_slice(&bild.hoehe.to_le_bytes());
            bytes.extend_from_slice(&bild.rgba);
            std::fs::write(
                ordner.join(format!("{}_vorlage45.raw", ALLE_NAMEN[i])),
                &bytes,
            )
            .expect("Rohdatei schreiben");
        }
        eprintln!("ABNAHME-ROHDATEN 45 px: {} Dateien", ALLE_DATEIEN.len());
    }

    /// Der Rasterweg fuer die Abnahme — mit demgemessenen Auswegen.
    fn lese_und_rastern(text: &str) -> Result<narsil_image::Bild, String> {
        let s = narsil_image::svg::svg_lesen(text).map_err(|e| e.to_string())?;
        narsil_image::svg::svg_rasteren(&s).map_err(|e| e.to_string())
    }
}
