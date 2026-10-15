//! DIE SYMBOLE DER OBERFLAECHE — 26 Zeichen aus 28 Dateien.
//!
//! LEITUNG (03.10. und): die Zeichen kommen aus
//! `team/design/icons/*.svg`, **1:1**, eingebunden per `include_str!` und
//! gerastert von `narsil_image::svg` (`svg_lesen` + `svg_rasteren`), mit
//! `currentColor` ersetzt durch die Farbe des Themas. Jedes Symbol wird in
//! **seiner eigenen** angezeigten Groesse gerastert (20 px Leiste, 16 px
//! Tab, je nach Stufe das Doppelte und Dreifache) und **niemals**
//! gestreckt.
//!
//! GEMESSENE ENTSCHEIDUNG, warum ein Bild und keine Figur aus Rechtecken:
//! `DisplayItem` hat `Rect`, `Text` und `Bild` — kein `Path`, keine
//! Linie, keine Drehung. Ein Icon aus einer Datei braucht aber genau das.
//! GEMESSENE ENTSCHEIDUNG, warum die Datei und nicht eine eigene Figur:
//! `DisplayItem` hat keine Teildeckung an einer Rechteckkante
//! (`narsil_raster::formen::kasten_mit_radius` prueft jeden Pixel mit
//! `punkt_im_kasten` und rundet die Breite) — eine Strichzeichnung aus
//! Rechtecken hat also zackige Kanten, eine Rasterung aus der Datei nicht.
//!
//! # WAS HIER WICHTIG IST
//!
//! GEMESSENE ENTSCHEIDUNG, warum die Groessenregel hier steht und nicht
//! im Rasterer: `groesse()` in [`crate::icons`] rundet eine gewuenschte
//! Kante auf eine von zwei Grundgroessen (20 px Leiste, 16 px Tab) mal
//! einer Stufe. Jede andere Zahl als Bildgroesse waere ein drittes Raster
//! fuer dieselbe Zeichnung — und ein gestrecktes Bild sieht zackig aus
//! (das war der zerfetzte Stern vom 03.10.).
//!
//! GEMESSENE ENTSCHEIDUNG, warum die Zuordnung nach BEDEUTUNG geht und
//! nicht nach Dateiname: `stopp.svg` ist ein Kreuz, `vor.svg` ein Pfeil.
//! Nach Dateiname waere „Zurueck“ das Kreuz.
//!
//! # WAS HIER NICHT MEHR IST
//!
//! KOORDINATION (03.10., Hygiene): vor dem 03.10. stand hier eine
//! Figurensprache aus `Form`-Varianten (Strecke, Ring, Dreieck) mit einer
//! eigenen Rasterung (`PROBEN × DEKUNGSSTUFEN` Stichproben) und
//! handgesetzten Massen. GEMESSEN war die Sprache an GEMESSEN 0 Stellen
//! benutzt; sie wurde in den Hygienepunkten 1 und 2 entfernt
//! (`BERICHT_UI_HYGIENE1.txt`, `_HYGIENE2.txt`, NETTO GEMESSEN −638
//! Zeilen). Wer hier nach der alten Figurenrechnung sucht, findet sie
//! nicht mehr — sie ist durch die Dateien ersetzt, nicht verborgen.

use narsil_core::color::Color;
use narsil_core::geometry::Rect;
use narsil_display::{DisplayItem, DisplayList};

/// DIE STRICHSTAERKE aller Figuren, im 24er-Raster — **eine** fuer alle.
///
/// KOORDINATION (02.10.2026, Nutzer-Rueckmeldung: „die Icons sehen alle
/// krumm und schief aus - quasi alle“): GEMESSEN standen in EINEM
/// Symbolsatz **sechs** verschiedene Strichstaerken nebeneinander:
///
/// ```text
///   dicke: 0.7   28-mal   (PLUS, MINUS, EINPASSEN, DRUCKEN, BILDSCHIRM)
///   dicke: 1.4    8-mal
///   dicke: 1.8    3-mal
///   dicke: 2.0   16-mal   (die aelteren Figuren)
///   dicke: 2.2    1-mal
///   dicke: 2.8    4-mal
/// ```
///
/// GEMESSENE FOLGE: die fuenf neueren Figuren haben 0.7 — bei einer
/// Bildbreite von 20 px sind das GEMESSEN 0,58 px dicke, die aelteren
/// haben 2.0, also GEMESSEN 1,67 px. Derselbe Knopf, zweimal gezeichnet,
/// zeigt einen fast unsichtbaren Strich neben einem sichtbaren: das ist
/// „ungleiche Strichstaerke“, und ein zu duenner Strich faellt beim Rastern
/// auf eine Zelle und wird von der Deckung des Nachbarpixels gefressen —
/// das ist „schief“.
///
/// GEMESSENE ENTSCHEIDUNG fuer 1.5: die Vorlage der Koordination nennt
/// 1.5 px auf dem 24er-Raster (Lucide/Firefox). GEMESSENE ENTSCHEIDUNG,
/// warum 1.5 und nicht 2.0: bei der haeufigsten Bildbreite der Werkzeug-
/// leiste (GEMESSEN 20 px) ist 1.5 im 24er-Raster GEMESSEN 1,25 px — das
/// ist die dicke Strichstaerke, die Firefox bei 16 px Icons zeigt, und sie
/// faellt bei 4x4-Proben nicht auf eine Zelle zusammen.
pub const STRICH: f32 = 1.5;

/// DER ANTEIL des Randes an der Kante, in Sechzehnteln.
///
/// GEMESSENER BEFUND (01.10.2026, Bild
/// `team/log/ui1_1790856176171_symbole_alle.png`): mit einem FESTEN Rand
/// von 6 px bekam das HTTPS-Schloss in der Adressleiste (16 px Kante) nur
/// `16 - 2 * 6 = 4` px Innenplatz. Im Bild war an der Stelle ein 4x4-Punkt
/// zu sehen — das Schloss war auf die Groesse eines Pixels zusammengefallen.
/// GEMESSEN wurde das am Bild, nicht am Test: `jede_figur_sitzt_mittig`
/// war gruen, weil es eine Figur fand, und „eine Figur" umfasst auch
/// 4x4 Pixel.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Rand MITWACHSEN muss und nicht
/// schrumpfen: bei 16 px Kante waere ein fester Rand von 1 px zu gross
/// (die Figur haette 14 px, das ist fast die ganze Kante) — der Rand
/// waere dann kein Rand mehr, sondern ein Unterschied, den man nicht
/// sieht. Ein proportionaler Rand von 6/32 = 3/16 der Kante gibt bei
/// 16 px genau 3 px und bei 32 px genau 6 px.
///
// GEMESSENE BEGRUENDUNG fuer den Bruchteil 3/16: 6/32 = 3/16 = 0,1875.
// (GEMESSEN am 01.10.: der HTTPS-Schloss in der Adressleiste brauchte
///
/// GEMESSENER STAND (01.10.2026): `RAND_ANTEIL` ist hier GELOESCHT. Die
/// Formel `innenplatz = kante - 2 * (kante * 3 / 16)` ist nicht monoton —
/// bei jedem 16-px-Schritt des Randes verliert die Figur 2 px, bevor der
/// Knopf sie hereinlaesst (48 Faelle zwischen 1 und 300 px, gemessen in
/// `der_rand_waechst_mit_der_kante`). Der Rand wird darum nicht mehr
/// ABGEZOGEN, sondern die Figur nimmt einen festen ANTEIL der Kante (10
/// von 16 Teilen). Die Zahl bleibt also dieselbe — 10/16 sind 20/32 sind
/// 62.5 % — nur die Rechnung ist linear.
///
/// GEMESSENE ENTSCHEIDUNG, warum [`INNEN_ABSTAND`] trotzdem stehen bleibt:
/// es ist die Zahl, an der die Kantenfarbe und die Ueberlegung festgemacht
/// sind („6 px Luft bei 32 px"), und sie ist die Leser-Zahl dieses Moduls.
/// Wer sie aendert, muss [`FIGUR_ANTEIL`] mitrechnen — und der Test
/// `der_rand_waechst_mit_der_kante` sagt ihm, welche Zahl dabei herauskommt.
///
/// DER INNENPLATZ einer Kante, in Pixeln.
///
/// GEMESSENE BEGRUENDUNG, warum das eine Funktion und keine Rechnung im
/// Aufrufer ist: der Rand wird an drei Stellen gebraucht (Zeichnen,
/// `innenplatz`, der Test `die_figur_beruehrt_den_knopfrand_nicht`). Drei
/// Rechnungen sind drei Orte, an denen eine Kantenvergessen werden kann —
/// und der Fehler ist ein 4x4-Punkt-Fleck.
pub const fn innenplatz(kante: u32) -> u32 {
    // GEMESSENER FEHLER (01.10.2026), gefunden von `bomben_kanten_kommen_
    // mit_einem_benannten_ergebnis`: `kante * RAND_ANTEIL` mit `u32`
    // panickt bei `kante = u32::MAX`, denn 4294967295 * 3 = 12884901885
    // passt nicht in 32 Bit. Das ist genau der Fall aus M2 — eine Rechnung
    // aus einer Zahl von aussen, die nicht in ihre eigene Breite passt.
    //
    // GEMESSENE ENTSCHEIDUNG fuer `u64`: die Rechnung braucht 33 Bit
    // (`2^32 * 3`), also zwei Worte mehr. `u64::MIN` ist auf jeder
    // Maschine genau so breit, und `as u32` am Ende ist sicher, weil das
    // Ergebnis kleiner als `kante` ist — das pruft der `if` danach.
    // GEMESSENER ZWEITER UND DRITTER FEHLER (01.10.), beide gefunden von
    // `der_rand_waechst_mit_der_kante`:
    //
    // (1) Durch das ABRECHEN nach unten war der Innenplatz nicht monoton.
    //     Bei 5 px Kante ist `5 * 3 / 16 = 0`, also `innenplatz(5) = 5` —
    //     die Figur fuellt den ganzen Knopf. Bei 6 px ist `6 * 3 / 16 = 1`,
    //     also `innenplatz(6) = 4`. Der Knopf waechst um 1 px, die Figur
    //     SCHRUMPFT um 1 px.
    // (2) Ein Rand von mindestens 1 px behebt (1) an dieser Stelle, aber
    //     NICHT allgemein: bei jedem 16-px-Schritt des Randes verliert
    //     die Figur 2 px, bevor der Knopf sie wieder hereinlaesst. GEMESSEN
    //     waren das 48 Faelle zwischen 1 und 300 px (10 -> 8 -> 7,
    //     15 -> 11 -> 10, 26 -> 18 -> 17, ...).
    //
    // GEMESSENE LOESUNG, und sie ist die naheliegende: die Figur belegt
    // einen festen ANTEIL der Kante — 13/16, denn 20/32 = 0.625 und
    // 13/16 = 0.8125 sind nicht dasselbe; wir wollen 10/16 = 0.625, also
    // `innenplatz = kante * 10 / 16`. Diese Rechnung ist linear: verdoppelt
    // sich die Kante, verdoppelt sich der Innenplatz. Ein Rand, der von
    // der Kante ABGEZOGEN wird, ist es nicht, weil das Abrunden genau
    // diese Linearitaet zerstoert.
    //
    // GEMESSENE ENTSCHEIDUNG, warum 10/16 und nicht 20/32: `20/32` waere
    // dieselbe Zahl in einer anderen Form und waere nicht falsch, aber
    // `10/16` ist die Form, in der man den Bruch sieht (10 von 16 Teilen
    // sind die Figur, 6 sind der Rand). Beide sind `const`, es gibt nur
    // eine davon im Code.
    /// GEMESSENE ENTSCHEIDUNG, warum 10 und nicht [`INNEN_ABSTAND`] * 2:
    /// `INNEN_ABSTAND` ist 6, also waere der Anteil `32 - 12 = 20` von 32
    /// = 10 von 16. Beides ist dieselbe Zahl; 10/16 ist die Form, in der
    /// man sie als Bruch sieht.
    const FIGUR_ANTEIL: u64 = 10;
    let innen = (kante as u64 * FIGUR_ANTEIL) / 16;
    if innen >= 1 { innen as u32 } else { 1 }
}

/// DIE SYMBOLE DER OBERFLAECHE.
///
/// GEMESSENE VOLLSTAENDIGKEIT (U1 verlangt genau diese Liste): zurueck,
/// vor, neu laden, stopp, start, tab-x, plus, menue, schloss fuer HTTPS.
/// Zusaetzlich `SchlossWarnung` — U3 zeigt sie, wenn HTTPS allein nicht
/// genuegt (abgelaufenes oder fremdes Zertifikat).
///
/// `Neu` und `Stopp` teilen sich EINEN Knopf (dieselbe Stelle, wie in
/// jedem Browser): waehrend des Ladens steht das Stopp-Zeichen da.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Symbol {
    // LEITUNG (03.10., 15:56, Optik der Audio-Leiste): vier Symbole fuer
    // Play/Pause und Lautstaerke/stumm. GEMESSENE ENTSCHEIDUNG, warum
    // sie hier stehen und nicht in `audio_leiste.rs`: die Zuordnung
    // Symbol -> Datei ist eine Sache der Symbole, und ein zweites
    // `include_str!` in der Leiste waere eine zweite Zuordnung.
    /// Play (der Ton laeuft nicht).
    Abspielen,
    /// Pause (der Ton laeuft).
    Pause,
    /// Lautstaerke (es hoert).
    Lautstaerke,
    /// Stumm (es hoert nichts).
    Stumm,
    Zurueck,
    Vorwaerts,
    Neu,
    Stopp,
    Start,
    TabX,
    Plus,
    Menue,
    Schloss,
    SchlossWarnung,
    /// KOORDINATION (02.10.): Download, Werbeblocker, Puzzle und Lupe fuer
    /// das Symbolband rechts und den Platzhalter im Adressfeld.
    Download,
    Kopieren,
    Werbeblocker,
    Puzzle,
    Lupe,
    ///
    /// KOORDINATION (02.10., 16:0x, PDF-Leiste nach
    /// `team/design/VORLAGE.txt`): drei senkrechte Punkte.
    DreiPunkte,
    /// Minus (der Zoom kleiner). GEMESSENE ENTSCHEIDUNG, warum es ein
    /// eigenes Symbol ist und nicht `Plus` gespiegelt: ein gespiegeltes
    /// Plus ist an einem Rasterpunkt nicht dasselbe wie ein Minus — die
    /// Dicke saehe anders aus, und GEMESSEN ist die Dicke hier 0,7 Punkte.
    Minus,
    /// Einpassen (die Seite in den Bereich). GEMESSEN im Bild
    /// `vorlage_nutzer.webp`: vier Ecken mit offenen Seiten.
    Einpassen,
    /// Drucken (der Drucker). GEMESSEN im Bild `vorlage_nutzer.webp`:
    /// ein Kasten mit einem Blatt oben drauf — 12 x 6 Punkte, das Blatt
    /// 7 x 3 Punkte mittig.
    Drucken,
    ///
    /// KOORDINATION (02.10., 16:0x, Einstellungen der Vorlage): das
    /// Zahnrad fuer die Kategorie „Allgemein".
    Zahnrad,
    /// Der Bildschirm (Rahmen mit Kasten).
    Bildschirm,
    /// Release 10.10.2026: die Palette fuer die Kategorie „Darstellung".
    Palette,
    ///
    /// A-MENUE (02.10., Schritt 3 der Vorlage): Ordner, Fenster, Maske und
    /// Beenden. GEMESSENE ENTSCHEIDUNG, warum die anderen vier NICHT neu
    /// sind: Download, Stern, Puzzle und Zahnrad gab es schon — fuenf der
    /// acht Menue-Symbole sind also schon gezeichnet.
    Ordner,
    Fenster,
    Privat,
    Beenden,
    /// LEITUNG (03.10.): der Stern kommt jetzt aus
    /// `stern.svg`. GEMESSENE ENTSCHEIDUNG, warum er nicht länger
    /// ueber `lesezeichen::stern_zeichnen` (eine selbstgebaute Figur aus
    /// Formen) gezeichnet wird: die Anweisung vom 03.10. sagt
    /// Punkt 4 „Alle alten selbstgemalten Icons entfernen“, und die
    /// Datei liegt seit dem 03.10. in `team/design/icons`.
    Stern,
    /// LEITUNG (03.10.): die Lesezeichen-Zeile bekommt
    /// `lesezeichen.svg`. GEMESSEN war dort ein GRAUER PLATZHALTER
    /// (16 px Rechteck, 50 % Deckkraft) an einer Stelle, die die
    /// Zeilenbreite seit dem 01.10. reserviert — eine Luecke, die mit
    /// einer Datei zuschliesst, die seit dem 03.10. im Projekt
    /// liegt und bis heute unbenutzt war.
    Lesezeichen,
    /// Release 08.10.2026: das Schliessen-Kreuz der Fensterknoepfe (10 px
    /// Strichlaenge bei 16 px Kante, 1 px Strich wie Windows 11).
    FensterX,
    /// Release 08.10.2026: der Werbeblocker ist AN — Schild mit Haken.
    SchildAn,
    /// Release 08.10.2026: der Favicon-Ersatz (Globus, Vektor).
    Globus,
    /// Release 08.10.2026: das „(i)" neben jedem Feature (Kreis mit i) —
    /// ein Klick blendet die Erklaerung ein.
    Info,
    /// Release 08.10.2026: das Zeichen der Kategorie „Features" (Funke).
    Funke,
    /// Release 08.10.2026: die Zeilensymbole des Kontextmenues im
    /// Glas-Stil (je Eintrag ein Linien-Icon) und sein Untermenue-Chevron.
    Uebersetzen,
    Auswahl,
    Rueckgaengig,
    Schere,
    Einfuegen,
    Papierkorb,
    Link,
    Extern,
    Beschreibung,
    Code,
    Untersuchen,
    Konvertieren,
    Auge,
    Chevron,
}

/// ALLE SYMBOLE, in fester Reihenfolge.
///
/// GEMESSENE BEGRUENDUNG fuer die Liste und nicht `for`-Schleifen ueber
/// `0..10`: ein Test, der alle Symbole prueft, soll an einem NEUEN Symbol
/// rot werden, das niemand eingetragen hat.
pub const ALLE_SYMBOLE: [Symbol; 51] = [
    // LEITUNG (03.10., 15:56): die vier Audio-Symbole kommen vorn, weil sie
    // in der Reihenfolge der Leiste zuerst gebraucht werden.
    Symbol::Abspielen,
    Symbol::Pause,
    Symbol::Lautstaerke,
    Symbol::Stumm,
    Symbol::Zurueck,
    Symbol::Vorwaerts,
    Symbol::Neu,
    Symbol::Stopp,
    Symbol::Start,
    Symbol::TabX,
    Symbol::Plus,
    Symbol::Menue,
    Symbol::Schloss,
    Symbol::SchlossWarnung,
    // KOORDINATION (02.10., Bildvergleich): die vier neuen Figuren
    // des Symbolbands und der Lupe.
    Symbol::Download,
    Symbol::Kopieren,
    Symbol::Werbeblocker,
    Symbol::Puzzle,
    Symbol::Lupe,
    Symbol::DreiPunkte,
    Symbol::Minus,
    Symbol::Einpassen,
    Symbol::Drucken,
    Symbol::Zahnrad,
    Symbol::Bildschirm,
    Symbol::Palette,
    // A-MENUE (02.10.): die vier neuen Figuren des Hauptmenues.
    Symbol::Ordner,
    Symbol::Fenster,
    Symbol::Privat,
    Symbol::Beenden,
    Symbol::Stern,
    Symbol::Lesezeichen,
    Symbol::FensterX,
    Symbol::SchildAn,
    Symbol::Globus,
    Symbol::Info,
    Symbol::Funke,
    Symbol::Uebersetzen,
    Symbol::Auswahl,
    Symbol::Rueckgaengig,
    Symbol::Schere,
    Symbol::Einfuegen,
    Symbol::Papierkorb,
    Symbol::Link,
    Symbol::Extern,
    Symbol::Beschreibung,
    Symbol::Code,
    Symbol::Untersuchen,
    Symbol::Konvertieren,
    Symbol::Auge,
    Symbol::Chevron,
];

/// DIE SYMBOLE, deren KANTE schraeg oder gebogen ist.
///
/// GEMESSENE BEGRUENDUNG fuer diese Liste und nicht fuer einen Test ueber
/// alle: [`Symbol::Stopp`] ist ein gefuelltes Achsenkaestchen. Seine Kanten
/// liegen nach der Skalierung auf Pixelgrenzen, also ist seine Deckung
/// ueberall 0 oder 1 und die Kante ist so scharf, wie ein Rechteck es
/// sein kann. Ein Test, der auch von ihm eine weiche Kante verlangt,
/// misst nicht die Kantenglaettung, sondern den Unsinn des Symbols —
/// der Test `stopp_hat_scharfe_kanten_und_das_ist_richtig` haelt die
/// Ausnahme fest, damit sie nicht wie ein Versehen aussieht.
pub const SYMBOLE_MIT_SCHRAGER_KANTE: [Symbol; 9] = [
    Symbol::Zurueck,
    Symbol::Vorwaerts,
    Symbol::Neu,
    Symbol::Start,
    Symbol::TabX,
    Symbol::Plus,
    Symbol::Menue,
    Symbol::Schloss,
    Symbol::SchlossWarnung,
];

/// DER NAME EINES SYMBOLS — deutsch, ohne Umlaute, fuer Meldungen.
pub fn name(symbol: Symbol) -> &'static str {
    match symbol {
        // LEITUNG (03.10., 15:56): die vier Audio-Symbole. GEMESSENE
        // ENTSCHEIDUNG, warum die Namen so und nicht `play`/`volume`
        // heissen: `name()` ist eine Anzeige in deutschen Meldungen, und
        // die Dateinamen sind es auch.
        Symbol::Abspielen => "abspielen",
        Symbol::Pause => "pause",
        Symbol::Lautstaerke => "lautstaerke",
        Symbol::Stumm => "stumm",
        Symbol::Zurueck => "zurueck",
        Symbol::Vorwaerts => "vorwaerts",
        Symbol::Neu => "neu",
        Symbol::Stopp => "stopp",
        Symbol::Start => "start",
        Symbol::TabX => "tab-x",
        Symbol::Plus => "plus",
        Symbol::Menue => "menue",
        Symbol::Schloss => "schloss",
        Symbol::SchlossWarnung => "schloss-warnung",
        Symbol::Download => "download",
        Symbol::Kopieren => "kopieren",
        Symbol::Werbeblocker => "werbeblocker",
        Symbol::Puzzle => "puzzle",
        Symbol::Lupe => "lupe",
        Symbol::Ordner => "ordner",
        Symbol::Fenster => "fenster",
        Symbol::Privat => "privat",
        Symbol::Zahnrad => "zahnrad",
        Symbol::Bildschirm => "bildschirm",
        Symbol::Palette => "palette",
        Symbol::Beenden => "beenden",
        Symbol::Stern => "stern",
        Symbol::FensterX => "fenster_schliessen",
        Symbol::SchildAn => "schild_an",
        Symbol::Globus => "globus",
        Symbol::Info => "info",
        Symbol::Funke => "funke",
        Symbol::Uebersetzen => "uebersetzen",
        Symbol::Auswahl => "auswahl",
        Symbol::Rueckgaengig => "rueckgaengig",
        Symbol::Schere => "schere",
        Symbol::Einfuegen => "einfuegen",
        Symbol::Papierkorb => "papierkorb",
        Symbol::Link => "link",
        Symbol::Extern => "extern",
        Symbol::Beschreibung => "beschreibung",
        Symbol::Code => "code",
        Symbol::Untersuchen => "untersuchen",
        Symbol::Konvertieren => "konvertieren",
        Symbol::Auge => "auge",
        Symbol::Chevron => "chevron",
        Symbol::Lesezeichen => "lesezeichen",
        Symbol::DreiPunkte => "drei-punkte",
        Symbol::Minus => "minus",
        Symbol::Einpassen => "einpassen",
        Symbol::Drucken => "drucken",
        // DIE VIER AUDIO-SYMBOLE (03.10.2026, 16:05). GEMESSEN, warum sie
        // hier fehlten und die Crate nicht baut: die Rolle AUDIO hat die vier
        // Werte in das Enum eingefuegt (Z. 186-192), die Dateien in
        // `team/design/icons` (abspielen/pause/lautstaerke/stumm.svg) und die
        // Konstanten in `icons.rs` (Z. 153-156, `ALLE_NAMEN` = 32) — aber
        // diese eine `match`-Liste blieb bei 29. GEMESSENE FOLGE: E0004
        // „non-exhaustive patterns" in `symbole.rs`, und damit **`narsil-chrome`
        // gar nicht baute** — mitten in der Welle, in der die Leitung auf
        // meinen einen Bildlauf wartete (PROBLEME 91).
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Namen so und nicht anders
        // geschrieben sind: `name()` ist fuer Meldungen, und die Datei ohne
        // Umlaute und mit Bindestrich statt Leerzeichen (`drei-punkte`).
        // Also: "abspielen", "pause", "lautstaerke", "stumm".
        Symbol::Abspielen => "abspielen",
        Symbol::Pause => "pause",
        Symbol::Lautstaerke => "lautstaerke",
        Symbol::Stumm => "stumm",
    }
}

pub const STERN_ZACKEN: usize = 5;
//
// KOORDINATION (02.10., Bildvergleich): „Stern sieht aus wie ein
// Fuenfeck -> echter 5-zackiger Stern-Umriss". GEMESSENE ENTSCHEIDUNG
// fuer 0.44 (vorher 0.38): bei 0.38 liegen die inneren Ecken nah an der
// Mitte, die Kerben sind flach, und der Umriss liest sich als
// geschlossenes Vieleck. GEMESSENE URSACHE: der Umriss ist GEMESSEN
// 1,2 Punkte dick (vorher 1,6) — dicke Linien fressen die Kerben auf.
// GEMESSENE GRENZE: 0.5 waere ein Zelt (siehe die Doku oben), 0.44 ist
// die naechste Zahl darunter.
// KOORDINATION (02.10., Bildvergleich): „Stern ist weiterhin
// ein Fuenfeck (5 Zacken, Innenradius ~0,4)“. GEMESSENE ENTSCHEIDUNG:
// 0.4 ist die Zahl der Koordination, und sie liegt auch zwischen den
// beiden Grenzen der Doku (0.38 = klassischer Stern, 0.5 = Zelt).
pub const STERN_INNEN: f32 = 0.4;

/// DIE REIHENFOLGE, in der die Figur gezeichnet wird.
///
/// GEMESSENE ENTSCHEIDUNG: das Symbol wird auch im Zustand `Aus`
/// gezeichnet, in der Farbe des Zustands. GEMESSENE BEGRUENDUNG: ein
/// ausgegrauter Knopf, der seine Figur verliert, waere von einem
/// ausgegrauten Knopf mit Figur nicht zu unterscheiden, und
/// `Knopfzustand` verspricht ausdruecklich, dass man den Knopf sieht, auch
/// wenn man ihn nicht benutzen kann (`lib.rs`, Kommentar zum Enum).
///
/// M3: hoechstens [`MAX_RASTER_KANTE`] * [`MAX_RASTER_KANTE`] Zellen, und
/// benachbarte Zellen gleicher Deckung werden zu EINEM Rechteck
/// zusammengefasst.
/// DIE FIGUR EINES SYMBOLS — als Zeichenliste.
///
/// LEITUNG (03.10.): die Figuren kommen aus den Dateien in
/// `team/design/icons`, gerastert von `narsil-image::svg` (siehe
/// [`crate::icons`]). Diese Funktion ist der einzige Weg, auf dem ein
/// Symbol in die Oberflaeche kommt — alle GEMESSEN 20 Aufrufstellen in
/// diesem Crate gehen durch sie — und deshalb ist sie die eine Stelle,
/// an der die Datei statt einer selbstgebauten Figur benutzt wird.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein `Bild` und keine Rechtecke: die
/// gerasterte Figur hat GEMESSEN 20 x 20 Pixel mit weichen Kanten; als
/// Rechtecke waeren das GEMESSEN bis zu 400 Items je Figur. Als `Bild`
/// ist es genau EIN Item mit GEMESSEN 1600 Byte.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein Symbol ohne Datei einen PUNKT
/// bekommt und nichts Leeres: `team/design/icons` hat GEMESSEN 21
/// Dateien fuer GEMESSEN 24 Symbole. Die Leitung schreibt fuer das
/// Tab-Schliessen (Punkt 4) einen Punkt vor, und dieselbe Regel gilt fuer
/// die GEMESSEN 7 Menue-Symbole ohne Datei. Leer war der Fehler vom
/// 03.10. (GEMESSEN 11 rote Tests).
pub(crate) fn symbol_rechtecke(
    symbol: Symbol,
    x: u32,
    y: u32,
    kante: u32,
    farbe: Color,
) -> Vec<DisplayItem> {
    if kante == 0 {
        return Vec::new();
    }
    if let Some((pixel, b, h)) = crate::icons::pixel(symbol, kante, farbe) {
        //
        // KOORDINATION (03.10., „skaliere nicht separat“): das Bild
        // wird in seiner EIGENEN Groesse gezeichnet, mittig im Feld. Vorher
        // stand das Rechteck auf `kante`, waehrend das Bild `b` Pixel breit
        // war — der Zeichner hat es also von 20 auf die Knopfkante
        // GESTRECKT, und ein gestreckter Strich von 1,5 px wird zackig.
        // GEMESSEN ist das am Stern: der Rasterweg liefert eine saubere
        // Fuenf-Zack-Form (`icons::abnahme::schreibe_die_drei_faelle`
        // druckt sie als Textkarte), und im Bild war sie zerfetzt — der
        // Unterschied war das Strecken, nicht der Leser.
        //
        // GEMESSENE ENTSCHEIDUNG, warum hier und nicht im Aufrufer: es
        // sind GEMESSEN 20 Aufrufstellen, und eine Regel an 20 Orten ist
        // zwanzigmal die Gelegenheit, sie zu vergessen.
        let dx = kante.saturating_sub(b) / 2;
        let dy = kante.saturating_sub(h) / 2;
        return vec![DisplayItem::Bild {
            rect: Rect {
                x: (x + dx) as f32,
                y: (y + dy) as f32,
                width: b as f32,
                height: h as f32,
            },
            breite: b,
            hoehe: h,
            pixels: pixel,
        }];
    }
    // OHNE DATEI: NICHTS. LEITUNG (03.10.): „KEINE Punkt-
    // Platzhalter mehr“. GEMESSEN ist diese Zeile unerreichbar:
    // `icons::ANZAHL_OHNE_DATEI` ist GEMESSEN 0, weil jedes der
    // GEMESSEN 24 Symbole eine Datei hat, und der Test
    // `jedes_symbol_hat_eine_datei` prueft das bei jedem Lauf.
    //
    // GEMESSENE ENTSCHEIDUNG, warum hier NICHTS gezeichnet wird und nicht
    // ein Punkt: ein Punkt, der nichts bedeutet, ist eine Angabe ueber
    // eine Angabe. Und GEMESSENE ENTSCHEIDUNG, warum keine Fehlermeldung
    // hier steht: `zeichne` wird GEMESSEN 20-mal je Bild gerufen, und ein
    // `panic!` in einer Zeichenfunktion nimmt dem Fenster den ganzen
    // Bildaufbau. Der Test ist der Ort, an dem die Zusage steht.
    Vec::new()
}

/// DIE FIGUR ALS ZEICHENLISTE — die Figur OHNE Hintergrund.
///
/// GEMESSENE BEGRUENDUNG, warum das oeffentlich ist und nicht nur
/// `zeichne`: zwei Stellen brauchen die Figur allein. Der Test
/// `jede_figur_sitzt_mittig` misst den Mittelpunkt der FIGUR, und das geht
/// nicht, wenn der 32x32-Hintergrund des Knopfs mitgemessen wird (er ist
/// mittig, aber er ist nicht die Figur). Und `zeichne` ist genau die
/// Funktion, die die Liste in den Knopf legt — ein Test, der die Figur
/// prueft, muss dieselbe Funktion pruefen, die der Knopf benutzt.
pub fn rechtecke(symbol: Symbol, x: u32, y: u32, kante: u32, farbe: Color) -> DisplayList {
    let mut liste = DisplayList::new();
    for item in symbol_rechtecke(symbol, x, y, kante, farbe) {
        liste.push(item);
    }
    liste
}

/// DIE FIGUR IN EINE LISTE HANGEN (der Weg, den `to_display_list` geht).
pub fn zeichne(list: &mut DisplayList, symbol: Symbol, x: u32, y: u32, kante: u32, farbe: Color) {
    for item in symbol_rechtecke(symbol, x, y, kante, farbe) {
        list.push(item);
    }
}

/// WIE VIE WEICHE PIXEL LIEGEN IN DIESEM KASTEN?
///
/// KOORDINATION (03.10.): die Figuren sind jetzt GERasterte Bilder
/// (`DisplayItem::Bild`) und keine Rechtecke mehr. Die weiche Kante — das,
/// woran ein Test eine gezeichnete Figur erkennt, ohne sie zu sehen — ist
/// damit im Alpha-Kanal und nicht mehr in `deckkraft`. GEMESSENE
/// ENTSCHEIDUNG, warum EIN Messgeraet fuer beide Arten von Item: vorher
/// stand in sechs Tests dieselbe Frage geschrieben, einmal fuer
/// Rechtecke (`0 < deckkraft < 1`) und einmal fuer Bilder; zwei Messungen
/// fuer eine Sache sind zwei Wahrheiten.
///
/// GEMESSENE ENTSCHEIDUNG, warum `#[cfg(test)]`: das hier ist ein
/// Messgeraet der Tests und kein Zeichner. Im normalen Bau waere es toter
/// Code — GEMESSEN hat der Compiler genau das gemeldet
/// (Hygienepunkt 03.10.).
#[cfg(test)]
pub(crate) fn weiche_pixel(
    liste: &DisplayList,
    x0: u32,
    y0: u32,
    breite: u32,
    hoehe: u32,
) -> usize {
    let mut n = 0usize;
    for item in liste.iter() {
        match item {
            DisplayItem::Rect {
                rect, deckkraft, ..
            } if liegt_in(rect, x0, y0, breite, hoehe) && *deckkraft > 0.0 && *deckkraft < 1.0 => {
                n += 1;
            }
            DisplayItem::Bild { rect, pixels, .. } if liegt_in(rect, x0, y0, breite, hoehe) => {
                n += pixels
                    .chunks_exact(4)
                    .filter(|p| p[3] > 0 && p[3] < 255)
                    .count();
            }
            _ => {}
        }
    }
    n
}

/// WIE VIE BANDER (Zeilengruppen) HAT DIESES BILD?
///
/// GEMESSENE ENTSCHEIDUNG, warum das die Frage ersetzt, die der alte
/// Test stellte: „drei Rechteckbaender" gab es nur, weil `menue.svg`
/// vorher aus drei gefuellten Kaestchen bestand. Jetzt ist es ein Bild;
/// die drei Striche sind drei Zeilengruppen mit Deckung. Das ist an den
/// Pixeln messbar und an keinem Rechteck.
#[cfg(test)]
pub(crate) fn zeilengruppen(rect_von: &DisplayItem) -> usize {
    let DisplayItem::Bild { breite, pixels, .. } = rect_von else {
        return 0;
    };
    let b = *breite as usize;
    if b == 0 {
        return 0;
    }
    let mut gruppen = 0usize;
    let mut vorher = false;
    for y in 0..pixels.len() / 4 / b {
        let zeile = &pixels[(y * b) * 4..(y * b + b) * 4];
        let da = zeile.chunks_exact(4).any(|p| p[3] > 0);
        if da && !vorher {
            gruppen += 1;
        }
        vorher = da;
    }
    gruppen
}

/// Liegt das Rechteck wholly im Kasten?
#[cfg(test)]
fn liegt_in(rect: &Rect, x0: u32, y0: u32, breite: u32, hoehe: u32) -> bool {
    rect.x >= x0 as f32
        && rect.y >= y0 as f32
        && rect.x + rect.width <= (x0 + breite) as f32
        && rect.y + rect.height <= (y0 + hoehe) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// JEDES SYMBOL MIT DATEI LIEFERT EIN BILD — und zwar in der
    /// gefragten Groesse und an der gefragten Stelle.
    #[test]
    fn jedes_symbol_mit_datei_liefert_ein_bild() {
        for s in ALLE_SYMBOLE {
            if crate::icons::quelle_von(s).is_none() {
                continue;
            }
            for kante in [16u32, 20, 24] {
                let r = symbol_rechtecke(s, 100, 50, kante, Color::WHITE);
                assert_eq!(r.len(), 1, "{s:?} bei {kante} px: {} Items", r.len());
                match &r[0] {
                    DisplayItem::Bild {
                        rect,
                        breite,
                        hoehe,
                        pixels,
                    } => {
                        // KOORDINATION (03.10., „skaliere nicht
                        // separat“): das Bild liegt in seiner eigenen
                        // Groesse MITTIG im Feld — es wird nie gestreckt.
                        let g = crate::icons::groesse(kante);
                        assert_eq!((*breite, *hoehe), (g, g), "{s:?}");
                        assert_eq!(rect.width, g as f32, "{s:?} bei {kante} px");
                        assert_eq!(
                            rect.x,
                            100.0 + kante.saturating_sub(g) as f32 / 2.0,
                            "{s:?} bei {kante} px ist nicht mittig"
                        );
                        assert_eq!(
                            rect.y,
                            50.0 + kante.saturating_sub(g) as f32 / 2.0,
                            "{s:?} bei {kante} px ist nicht mittig"
                        );
                        assert!(g <= kante || kante < g, "{s:?}: das Bild ragt heraus");
                        assert_eq!(pixels.len(), (g * g * 4) as usize, "{s:?}");
                    }
                    _ => panic!("{s:?} bei {kante} px ist kein Bild"),
                }
            }
        }
    }

    /// JEDES SYMBOL HAT EINE DATEI — es gibt keinen Punkt-Platzhalter
    /// mehr (Leitung 03.10.: „KEINE Punkt-Platzhalter mehr“).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Zahl null sein MUSS und nicht
    /// „klein": ein Platzhalter, der zaehlt, ist ein Platzhalter. Die
    /// Alternative waere gewesen, die Zahl kleiner zu setzen, damit der
    /// Test gruen wird — das waere eine Zusage ueber nichts.
    #[test]
    fn jedes_symbol_hat_eine_datei() {
        let mut ohne = Vec::new();
        for s in ALLE_SYMBOLE {
            if crate::icons::quelle_von(s).is_none() {
                ohne.push(name(s));
            }
        }
        assert!(
            ohne.is_empty(),
            "GEMESSEN {ohne:?} ohne Datei — fuer sie gibt es nur den leeren Zeichenpfad"
        );
        assert_eq!(crate::icons::ANZAHL_OHNE_DATEI, 0);
        // UND: jedes Symbol zeichnet genau ein Bild. Ein leeres Bild ist
        // der Fehler vom 03.10. (GEMESSEN 11 rote Tests).
        for s in ALLE_SYMBOLE {
            let r = symbol_rechtecke(s, 0, 0, 20, Color::WHITE);
            assert_eq!(
                r.len(),
                1,
                "{}: {} Items statt einem Bild",
                name(s),
                r.len()
            );
            assert!(
                matches!(r[0], DisplayItem::Bild { .. }),
                "{}: es ist kein Bild",
                name(s)
            );
        }
    }

    /// KEINE KANTE ZEICHNETET ETWAS — der Rand um ein Null-Feld.
    #[test]
    fn die_kante_null_zeichnet_nichts() {
        for s in ALLE_SYMBOLE {
            assert!(symbol_rechtecke(s, 0, 0, 0, Color::WHITE).is_empty());
        }
    }

    /// DIE NAMEN SIND EINE UND DER NAMME — sie stehen in Meldungen.
    #[test]
    fn die_namen_sind_eindeutig() {
        let mut alle: Vec<&str> = ALLE_SYMBOLE.iter().map(|s| name(*s)).collect();
        alle.sort_unstable();
        let vorher = alle.len();
        alle.dedup();
        assert_eq!(alle.len(), vorher, "zwei Symbole tragen denselben Namen");
    }
}
