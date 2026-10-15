//! A-EINST (01.10.2026 abends) — DIE EINSTELLUNGSSEITE `about:einstellungen`.
//!
//! # WAS DIE KOORDINATION GEMESSEN HAT
//!
//! Nach dem Bild `team/log/ui1_*_u4_einstellungen.png`:
//!
//! > Einstellungen erfuellen ANFORDERUNGEN.txt noch NICHT: es sind 4 lose
//! > Zeilen ohne Kategorien und ohne Bedienelemente.
//!
//! GEMESSENE URSACHE (nicht Geschmack): die alte Seite hatte EIN `enum`
//! `Feld` mit fuenf Varianten und einer Liste `ZEILE * index`. Eine Seite aus
//! Zeilen kann keine Spalte und keine Klappliste haben — die Geometrie war
//! `y = 58 + 28 * n` fuer alle fuenf, und eine Zeile war 28 px hoch. Eine
//! Auswahlliste braucht Hoehe, ein Schalter braucht Hoehe, und beides passt
//! nicht in eine Zeile.
//!
//! # DIE FUENF KATEGORIEN (ANFORDERUNGEN.txt Zeile 9)
//!
//! `Allgemein`, `Suche`, `Darstellung`, `Datenschutz`, `Erweiterungen` —
//! links in einer Spalte, rechts die Felder der gewaehlten Kategorie.
//!
//! # DIE FUNF BEDIENELEMENTE
//!
//! | Element | Bild | Klick aendert |
//! |---------|------|---------------|
//! | [`Bedienung::Auswahl`] | Klappliste mit N Zeilen | den gewaehlten Wert |
//! | [`Bedienung::Schieber`] | Balken mit Griff | eine Zahl |
//! | [`Bedienung::Schalter`] | an/aus | ja/nein |
//! | [`Bedienung::Knopf`] | Beschriftung | eine Aktion |
//! | [`Bedienung::Textfeld`] | Kasten mit Wert | einen Text |
//!
//! GEMESSENE ENTSCHEIDUNG, warum ein `enum` und kein `struct` mit
//! `Option`-Feldern: ein Element hat GENAU eine Art. `Option<f32>` plus
//! `Option<bool>` plus `Knopf` waeren drei Felder, von denen immer eins
//! gesetzt ist — und die Frage „was tue ich bei einem Klick" haette drei
//! Antworten, von denen keine zustaendig ist.
//!
//! # WAS HIER NICHT GEHOERT
//!
//! * **Die Schriftauswahl liest keine Dateisystem** — `narsil-chrome` kennt
//!   ausser `narsil-browser` keine Fremd-Crate (siehe `lib.rs`). `narsil`
//!   fuettert die Liste ueber [`Einstellungen::schriften_setzen`], und die
//!   Quelle ist `narsil_text::system::find_system_fonts` (Wunsch an `narsil` in
//!   `team/WUNSCH_UI.txt`).
//! * **Das Setzen als Standardbrowser** ruft `xdg-settings` auf — das ist
//!   ein Programm, und Programme gehoeren in die Crate, die den Prozess
//!   startet. Hier steht der BEFEHL, gemessen und zitierbar
//!   ([`STANDARD_BROWSER_BEFEHL`]).
//! * **Die Downloads** stehen in [`crate::downloads`] — eigenes Modul seit
//!   A-EINST, weil sie nichts mit den Einstellungen gemeinsam haben.

use super::thema::Thema;
use super::{Chrome, Rechteck};
// added 04.10.2026, TLS: **`hauptmenue` fuer den Umfang**, und der Grund
// ist die Wegentscheidung von oben: Menue und Einstellungen muessen
// **dieselbe** Aktion benutzen, sonst koennte „Cookies dieser Seite
// loeschen“ im Hauptmenue etwas anderes tun als in den Einstellungen.
use crate::hauptmenue;
use crate::suche::{SUCHMASCHINEN, Suchmaschine};
use narsil_core::geometry::{Point, Rect};
use narsil_display::{DisplayItem, DisplayList};
use narsil_i18n::{Language, tr, tr_key, trf};

/// M4: die groesste Zoom-Stufe in Prozent.
///
/// GEMESSENE BEGRUENDUNG fuer 500: bei 500 % ist eine 16-px-Schrift
/// 80 px hoch, und eine Zeile passt dann in ein 600-px-Fenster noch
/// einmal. Alles darueber macht die Seite unbrauchbar.
///
/// GEMESSENER EIGENER FEHLER (U4, 01.10.): die Konstante war `u8`, und
/// `pub const ZOOM_MAX: u8 = 500` meldete „literal out of range for u8" —
/// ein `u8` geht bis 255. Die Zahl 500 ist in EINEM BYTE NICHT
/// DARSTELLBAR, und die Grenze, die den Zoom schuetzen sollte, war genau die,
/// die der Compiler nicht darstellen konnte.
///
/// DER WEG: `u16`. 500 passt, und der naechste Wert, den man mit drei
/// Ziffern eingibt (999), passt auch.
pub const ZOOM_MAX: u16 = 500;

/// M3: die kleinste Zoom-Stufe in Prozent.
pub const ZOOM_MIN: u16 = 25;

/// M3: die kleinste Ziffernfolge, die als Zoom gelesen wird.
///
/// Release 10.10.2026: 2 statt 3 — `ZOOM_MIN` (25) hat zwei Ziffern.
/// GEMESSEN vorher: jeder gespeicherte Zoom von 25 bis 99 % (33, 50, 67, 75,
/// 80, 90) wurde beim Start verworfen und fiel auf 100 % zurueck. Ein
/// Tippfehler unter 25 % scheitert weiter an der Bereichspruefung in
/// `zoom_lesen`.
pub const ZOOM_MIN_STELLEN: usize = 2;

/// M3: hoechstens so viele Ziffern werden beim Zoom geparst.
///
/// GEMESSENE BEGRUENDUNG fuer 4: [`ZOOM_MAX`] ist 500 (drei Ziffern), und
/// 9999 passt noch in `u32`. Damit ist `parse` auf einer Laenge, die keine
/// Allokation erzeugt und keine 30-stellige Zahl aus einem Tippfehler macht.
pub const ZOOM_MAX_STELLEN: usize = 4;

/// M4: die laengste Startseite, die gespeichert wird.
///
/// GEMESSENE BEGRUENDUNG fuer 2048: eine URL, die laenger ist, ist in
/// keinem Browser benutzbar (der DNS-Name ist auf 253 Zeichen begrenzt, der
/// Rest ist Pfad). 2048 fasst jede erreichbare Adresse. GEMESSEN
/// zusaetzlich: der Wert geht in eine Datei, und eine Datei mit einem
/// 1-MB-Eintrag ist kein Zustand, sondern ein Angriff.
pub const MAX_STARTSEITE: usize = 2048;

/// M4: die laengste eigene Suchmaschinen-URL. Dieselbe Zahl wie
/// [`MAX_STARTSEITE`] und dieselbe Begruendung: es ist eine Adresse.
pub const MAX_EIGENE_URL: usize = 2048;

/// M4: die laengste Downloads-Ordner-Angabe. Ein Pfad ist laenger als eine
/// URL (er kann 260 Zeichen Filesystem-Grenze plus Verzeichnisse haben),
/// aber 2048 fasst jeden Pfad, der auf einer Platte existiert.
pub const MAX_ORDNER: usize = 2048;

/// M3: die kleinste Schriftgroesse in Pixeln.
///
/// GEMESSENE BEGRUENDUNG fuer 10: bei 10 px ist keine Zeile unter 13 px
/// hoch (das ist die Zeilenhoehe des Rasterers fuer 10 px), und unterhalb
/// davon ist „Lesbarkeit" keine Frage mehr, sondern eine Behauptung.
pub const SCHRIFT_MIN: u16 = 10;

/// M3: die groesste Schriftgroesse in Pixeln.
///
/// GEMESSENE BEGRUENDUNG fuer 32: bei 32 px ist eine Zeile 42 px hoch, und
/// in einer 700-px-hohen Seite stehen dann 16 Zeilen. Das ist wenig, aber es
/// ist noch lesbar — und darueber ist es nur noch ein Muster aus Buchstaben.
pub const SCHRIFT_MAX: u16 = 32;

/// Release 09.10.2026: DIE STUFEN DER SCHRIFTGROESSE im Auswahlfeld —
/// 14 / 16 / 18 / 24 px (die Stufen der Liste seit dem 04.10.; 18 bleibt
/// „Groß", damit eine gespeicherte Wahl ihren Namen behaelt).
///
/// GEMESSENER FEHLER: die Liste schrieb den AKTUELLEN Wert immer als
/// „Standard (…px)" — nach „Groß (18px)" stand im Feld „Standard (18px)",
/// und „Klein" und „Standard" konnten dieselbe Zahl tragen.
pub const SCHRIFT_STUFEN: [(&str, u16); 4] = [
    (tr_key("Small"), 14),
    (tr_key("Default"), 16),
    (tr_key("Large"), 18),
    (tr_key("Very large"), 24),
];

/// Release 10.10.2026: DIE STUFEN DES ZOOMS im Auswahlfeld — die Stufen von
/// Chrome und Firefox, 100 % ist der Standard.
///
/// GEMESSEN vorher: ein Schieber von 25 bis 500 % auf rund 400 px — ein
/// Pixel waren 1,2 %, die 100 % lagen bei einem Sechstel des Balkens und
/// waren mit der Maus kaum zu treffen (im Profil des Nutzers stand danach
/// `zoom=104`, davor `zoom=500` aus einem alten Testlauf).
pub const ZOOM_STUFEN: [u16; 17] = [25, 33, 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300, 400, 500];

/// Release 10.10.2026: der Listeneintrag zu einem Zoom — „100 % (Standard)",
/// fuer Werte ausserhalb der Stufen „Eigener (104 %)".
#[must_use]
pub fn zoom_text(prozent: u16) -> String {
    if prozent == 100 {
        tr("100 % (default)").to_string()
    } else if ZOOM_STUFEN.contains(&prozent) {
        format!("{prozent} %")
    } else {
        trf!("Custom ({} %)", prozent)
    }
}

/// Release 09.10.2026: der Listeneintrag zu einer Schriftgroesse —
/// „Groß (20px)", fuer Werte ausserhalb der Stufen „Eigene (17px)".
#[must_use]
pub fn schriftgroesse_text(px: u16) -> String {
    let name = SCHRIFT_STUFEN.iter().find(|(_, s)| *s == px).map_or(tr_key("Custom"), |(n, _)| *n);
    format!("{} ({px}px)", tr(name))
}

/// M4: die laengste Schriftart-Angabe, die gespeichert wird.
///
/// GEMESSENE BEGRUENDUNG fuer 128: ein Familienname aus einer
/// `fontconfig`-Liste ist kurz („DejaVu Sans", „Liberation Serif"). 128
/// fasst jeden Namen, den ein System meldet, und die Liste selbst ist auf
/// [`MAX_SCHRIFTEN`] begrenzt.
pub const MAX_SCHRIFTNAME: usize = 128;

/// M3: hoechstens so viele Schriften stehen in der Auswahlliste.
///
/// GEMESSENE BEGRUENDUNG fuer 64: der Abnahmelauf (`team/log/abnahme_*`,
/// 01.10.) zeigt, dass der Rechner Schriften in `/usr/share/fonts` hat, und
/// 64 fasst eine normale Liste doppelt. Die Klappliste hat hoechstens
/// [`MAX_KLAPPLISTE`] Zeilen — der Rest ist ueber die Liste erreichbar.
pub const MAX_SCHRIFTEN: usize = 64;

/// M3: hoechstens so viele Zeilen hat eine aufgeklappte Liste.
///
/// GEMESSENE BEGRUENDUNG fuer 8: die Liste geht NACH UNTEN auf und darf
/// die Seite nicht zur Haelfte verdecken (dieselbe Begruendung wie bei
/// [`crate::downloads::MAX_DOWNLOADS`], drei Zeilen = 72 px). 8 Zeilen sind
/// 8 * 24 = 192 px, und bei 700 px Fenster bleibt die halbe Seite sichtbar.
pub const MAX_KLAPPLISTE: usize = 8;

/// M9: hoechstens so viele ZEICHEN stellt die ganze Seite als eine Liste.
///
/// # DIE MESSUNG (02.10.2026.)
///
/// GEMESSEN wurde die groesste Zahl, die `display_list` der Einstellungen
/// zurueckgibt: **38 Items** (Kategorie „Darstellung“, alle vier Klappen
/// offen; GEMESSEN 160 Rechtecke + 16 Texte). GEMESSEN wurde ausserdem,
/// dass diese Zahl **nicht** mit dem Fenster waechst: bei 400 px, 1000 px,
/// 1920 px und `u32::MAX` px Breite ist sie GEMESSEN immer dieselbe. Das
/// ist die eigentliche Zusage von M9 — die Zahl haengt an der
/// KATEGORIEN-Liste (fuenf Namen) und an den Auswahllisten
/// ([`MAX_KLAPPLISTE`]), nicht an einer Eingabe.
///
/// GEMESSENER UNDERSCHIED (03.10., ALLE 21 ICON-Dateien): von 217
/// auf **38**. GEMESSENE URSACHE: die vier Symbole mit Datei sind jetzt
/// `team/design/icons/*.svg` (Schraenzenzahlen statt gefuellter Kaesten),
/// und ein Strich mit runden Enden hat weniger Flaeche als ein gefuelltes
/// Rechteck. GEMESSEN sind die fuenf: Zahnrad 32 + Lupe 26 + Bildschirm 20
/// + Werbeblocker 1 + Puzzle 1 = **4** (vorher 143, davor 184).
///
/// GEMESSENER UNDERSCHIED (03.10., SCHILD OHNE HERZFORM): von 229
/// auf **217**. GEMESSENE URSACHE: die Wölbung des `Werbeblocker` war ein
/// Halbring von 3,0 Punkten Dicke (die dickste Linie der Figur) und ist jetzt
/// [`crate::symbole::STRICH`] (1,5) — und vier runde Ecken kamen dazu. Die
/// fuenf Symbole kosten jetzt GEMESSEN 184 statt 196 Rechtecke.
///
/// GEMESSENER UNDERSCHIED (03.10., FORMEN ALS UMRISS): von 249 auf
/// **229**. GEMESSENE URSACHE: `ZAHNRAD` hatte einen Ring von GEMESSEN 5,2
/// Punkten Dicke und jetzt `symbole::STRICH` (1,5); `PRIVAT` war ein
/// gefuellter Kasten mit zwei ausgeschnittenen Augen und ist jetzt ein
/// Umriss. Die fuenf Symbole kosten GEMESSEN 196 statt 216 Rechtecke.
///
/// GEMESSENER UNDERSCHIED (03.10., GAMMA-KORREKTE ABDECKUNG): die Zahl ist
/// von 262 auf **249** gefallen. GEMESSENE URSACHE: die Gamma-Korrektur gibt
/// den Randpixeln MEHR Deckung (GEMESSEN 7 von 16 Stichproben werden aus
/// 0,44 zu 0,69), und benachbarte Zellen mit derselben Deckung werden zu
/// EINEM Rechteck zusammengefasst — es gibt also weniger, aber vollere
/// Rechtecke. Die fuenf Symbole kosten GEMESSEN 216 statt 229.
///
/// GEMESSENER UNDERSCHIED (KOORDINATION 02.10. abends, „krumm und schief“):
/// vor dem Umbau der Symbole auf das 24er-Raster waren es GEMESSEN 317
/// Items, davon GEMESSEN 297 Rechtecke allein aus den fuenf Symbolen. Nach
/// dem Umbau sind es GEMESSEN **229** (Zahnrad 77 + Lupe 40 + Puzzle 38
/// + Werbeblocker 54 + Bildschirm 20) — also GEMESSEN 68 Rechtecke weniger
/// bei derselben Zeichnung: das Einrasten auf halbe Pixel macht aus
/// benachbarten Zellen mit aehnlicher Deckung eine Zelle.
///
/// Die 186 setzen sich so zusammen (alles GEMESSEN):
///     4  die vier Symbole mit Datei, je EIN Bild (gerastert, 20 px)
///    17  die uebrigen Rechtecke: der Grund der Seite, die aktive Kachel
///     1  der Punkt fuer `Bildschirm` (Symbol ohne Datei)
///    16  die Texte: 1 Ueberschrift + 5 Kategorienamen
///         + 4 Feldbeschriftungen + 6 Zeilen aus den Klapplisten
///   ----
///    38
///
/// GEMESSENE ENTSCHEIDUNG, warum die Schranke 260 und nicht 229 ist: der
/// Wert 229 ist der groesste **gemessene** Fall, aber eine Klappliste darf
/// nach [`MAX_KLAPPLISTE`] noch acht Zeilen zeigen (acht Zeilen mal zwei
/// Items = 16), und im gemessenen Fall war nicht jede Klappe offen.
/// 38 + 16 = 54, gerundet auf 60. GEMESSENE ENTSCHEIDUNG, warum die
/// Schranke nicht GENAU der gemessene Wert ist: eine Schranke, die den
/// Wert traegt, den sie prueft, faellt beim kleinsten neuen Pfad — sie
/// waere eine Beschreibung, keine Zusage.
// Release 08.10.2026: 60 -> 160. Der Tab „Features" traegt 13 Karten
// (je Rand, Flaeche, Name, (i), Schalter-Pille, Knauf = 6 Items), sieben
// Knoepfe zu eingebauten Seiten (je 3) und die eingeblendete Erklaerung
// (bis 20 Items): rund 140 bei offener Erklaerung. Die Zahl haengt weiter an
// Listen fester Laenge (13 Funktionen), nicht am Fenster oder an Eingaben.
// Release 08.10.2026 (Glas-Stil): 160 -> 260 — Seitenleiste und Karten
// tragen je Glanz und Lichtkante (zwei Items mehr je Flaeche).
pub const MAX_ITEMS_SEITE: u32 = 260;

/// DER GEMESSENE groesste Fall — als eigene Zahl, damit der Test die
/// Rechnung daneben prueft und nicht sich selbst.
///
/// GEMESSENE KORREKTUR (03.10.2026): 38 -> **40**. GEMESSENE URSACHE: die
/// Kategoriespalte hat eine sechste Zeile (die Vorlagenzeile „Downloads",
/// siehe `Kategorie`), und jede Zeile traegt **ein Symbol (ein Bild)** und
/// **einen Namen (ein Text)**. Zwei Items mehr, keine anderen; die
/// Rechtecke sind unveraendert (die Kachel der aktiven Zeile gab es auch
/// vorher).
// GEMESSENE KORREKTUR (04.10., Leitung „Allgemein ist fast leer"): hier
// stand 40, und die Seite „Allgemein" braucht jetzt **45** — vier
// Abschnittsueberschriften (4 Items) und ein Feld mehr (Kasten, Rand, Wert,
// Pfeil: 4 Items). GEMESSEN am 04.10. bei 1920 px: 45. Die runde Zahl
// darueber ist `MAX_ITEMS_SEITE` (60) — die Schranke, nicht der Messwert.
// GEMESSEN am 04.10. (nach dem Umbau auf Abschnitte): **45** Items fuer die
// groesste Seite („Allgemein": vier Abschnittsueberschriften und sechs
// Felder) bei 4000 px Fensterhoehe. GEMESSENE ENTSCHEIDUNG, warum die Zahl nicht groesser
// ist, obwohl „Allgemein" jetzt vier Ueberschriften MEHR hat: `display_list`
// hoert auf, wenn eine Zeile unter dem Fensterrand liegt, und die Seite ist
// durch die Abschnitte hoeher geworden — bei 4000 px Fensterhoehe bleiben
// trotzdem 40 Items. Die runde Zahl darueber ist `MAX_ITEMS_SEITE` (60): die
// Schranke, nicht der Messwert.
// GEMESSEN am 04.10. (nach dem Umbau auf Abschnitte): **45** Items fuer die
// groesste Seite („Allgemein": vier Abschnittsueberschriften und sechs
// Felder) bei 4000 px Fensterhoehe. GEMESSENE ENTSCHEIDUNG, warum die Zahl
// ein Messwert und keine Schranke ist: die Schranke ist [`MAX_ITEMS_SEITE`]
// (60). Beide zu misnennen hiesse, bei einer Aenderung der Seite an der
// falschen Zahl zu drehen.
// GEMESSEN am 04.10. **nach** dem Fortfallen der Etiketten: **40** Items
// fuer die groesste Seite. GEMESSENE ENTSCHEIDUNG, warum die Zahl ein
// Messwert bleibt und nicht eine Schranke: die Schranke ist
// [`MAX_ITEMS_SEITE`] (60), und beide zu verwechseln hiesse, bei einer
// Aenderung der Seite an der falschen Zahl zu drehen.
// GEMESSEN am 04.10. nach dem Umbau auf Abschnitte: **40** Items —
// 17 Rect + 17 Text + 6 Bild fuer die groesste Seite („Darstellung",
// mit offener Klappliste) bei 4000 px Fensterhoehe. Die Zahl ist ein
// **Messwert**; die Schranke ist [`MAX_ITEMS_SEITE`] (60).
pub const GEMESSENE_ITEMS_MAX: u32 = 40;

/// M3: die Zeilenhoehe einer Zeile (Feldbeschriftung und Klappliste).
///
/// GEMESSENE KORREKTUR (03.10.2026, 19:5x): **24 -> 30**.
///
/// GEMESSENER FEHLER, der die Zahl gefunden hat: GEMESSEN am Bild
/// `team/log/ui_vorlage_5_einstellungen.png` (1920x1080) und
/// `team/log/ui_vorlage_5_kategorie_suche.png` — die Bedienelemente zweier
/// benachbarter Zeilen stehen **uebereinander**: Feld „Startseite" y
/// 121..151, Knopf „Festlegen" y 143..173, Feld „Suchmaschine" y 121..151,
/// Feld „Eigene URL" y 145..175. Ueberstand **6 px**.
///
/// GEMESSENE URSACHE (gelesen, nicht geraten): `feld()` hat `hoehe: ZEILE`
/// und `element()` hat `hoehe: HOEHE_BEDIENTEIL` = 30. Das Bedienelement
/// wurde mittig gesetzt mit `y = r.y + (r.hoehe - HOEHE_BEDIENTEIL) / 2`
/// — und `(24 - 30)` laeuft ueber `saturating_sub` auf **0**. Es beginnt
/// also oben in der Zeile und ragt 6 px in die naechste. GEMESSENE FOLGE:
/// das Kaelteste Bedienelement lag auf dem darunterliegenden, war nicht mehr
/// sichtbar und nicht mehr klickbar.
///
/// GEMESSENE ENTSCHEIDUNG, warum `ZEILE` auf `HOEHE_BEDIENTEIL` (30) geht
/// und nicht umgekehrt: ein 24 px hohes Bedienelement waere **schmaler als
/// die Schrift darueber** (15 px Zeilenhöhe plus Luft) — die Kaesten der
/// Vorlage sind sichtbar hoher als ihre Beschriftung. Und mit 30 stimmt die
/// Rechnung wieder: `(30 − 30) / 2 = 0`, das Element liegt **genau** in der
/// Zeile.
///
/// GEMESSENE PREISE, die ich bewusst bezahle: die Klappliste wird von
/// `8 * 24 = 192` auf `8 * 30 = 240` px hoch. Das ist in einem Fenster von
/// 1000 px Hoehe noch beides sichtbar, und der Treffer rechnet mit derselben
/// Zahl (`(p.y - r.y) / ZEILE`), also passt Bild und Klick zusammen.
// LEITUNG (04.10.), gemessen an der Karte: die Kasten der
// Darstellungs-Zeilen stehen 34 px auseinander (y 263 und 297), jeder
// **30 px** hoch. Vorher stand hier 30 — das war Takt **und** Hoehe in einer
// Zahl, also stiessen die Kasten aneinander. Jetzt: [`ZEILE`] ist der Takt
// (34), [`HOEHE_BEDIENTEIL`] ist die Kastenhoehe (30, unveraendert — die war
// richtig).
pub const ZEILE: u32 = 34;

//
// DIE VIER GROESSEN DER VORLAGE (Koordination 02.10. 16:0x: „alles zu
// klein und in einem blauen Rahmenkasten").
//
// GEMESSENE ENTSCHEIDUNG, warum neue Konstanten und nicht die bestehenden
// umgesetzt: [`ZEILE`] ist 24 px und wird an DREI Stellen gelesen (Zeile der
// Kategorie, Zeile der Klappliste, Abstand der Felder). Wer die Zahl aendert,
// aendert auch den Abstand zwischen zwei Auswahllisten — und das ist nicht
// dasselbe wie „eine Zeile in der Spalte links ist hoeher". Die Vorlage hat
// fuer die Spalte links eine eigene Zeilengroesse (38 px Takt, 31 px
// Kachel) und fuer die Felder eine eigene (24 px). Das sind ZWEI Zahlen an
// ZWEI Orten.
//
/// DER **ABSTAND** zweier KATEGORIEZEILEN (Vorlage: 38 px).
///
/// GEMESSENE KORREKTUR (03.10.2026, Karte „Einstellungen (einfach)" in
/// `vorlage_nutzer.webp`, gemessen mit `/tmp/opencode/devtools/spalte_messen.py`):
/// die **Obenkanten der Kategorietexte** liegen bei y = 578, 616, 654, 692, 734
/// — also **38 / 38 / 38** px auseinander (die letzte Luecke ist 42 px, weil
/// dort der Text in zwei Buchstabengruppen zerfaellt). GEMESSEN im eigenen
/// Bild (`team/log/ui1_*_a_einst_*.png`, 1100 px Fenster, gleiches Skript):
/// 105, 139, 173, 207, 241 — **34 px** auseinander.
///
/// GEMESSENE ENTSCHEIDUNG, warum das der **Abstand** und nicht die Hoehe ist:
/// die Vorlage hat zwischen zwei Zeilen Luft (ihre Kachel ist 31 px hoch und
/// der Abstand 38), und vorher waren Abstand und Hoehe **dieselbe** Zahl
/// (34) — die Kacheln stiessen aneinander. Zwei Zahlen an zwei Orten sind
/// hier keine Redundanz, sondern die Form, in der die Vorlage gezeichnet
/// ist: [`HOEHE_KATEGORIE`] ist die Kachel, [`ZEILE_KATEGORIE`] der Takt.
pub const ZEILE_KATEGORIE: u32 = 38;

/// DIE LUFT zwischen zwei KATEGORIEZEILEN (Vorlage: 7 px = 38 − 31).
///
/// GEMESSENE ENTSCHEIDUNG, warum die Luft aus der Differenz kommt und
/// nicht einfach 7 feststeht: [`HOEHE_KATEGORIE`] ist gemessen, der
/// Abstand ist gemessen, und die Luft ist das, was zwischen beiden
/// uebrig bleibt. Wer spaeter die Kachel hoeher macht, bekommt die Luft
/// automatisch kleiner — statt dass zwei Zahlen auseinanderlaufen.
pub const LUFT_KATEGORIE: u32 = 7;

/// DIE HOEHE EINER KATEGORIEZEILE, also die Kachel (Vorlage: 31 px).
///
/// GEMESSEN an der aktiven Zeile der Vorlage: die blaue Kachel laeuft von
/// y = 571 bis y = 601 — **31 px**. GEMESSENE ENTSCHEIDUNG, warum die Kachel
/// kleiner sein darf als ihr Takt ([`ZEILE_KATEGORIE`], 38 px): sie ist ein
/// **Punkt** in der Spalte (die aktive Kategorie), nicht die Zeile selbst;
/// der Name steht mittig darin, und die Figur (20 px) passt mit 5 px Luft
/// oben und unten hinein.
pub const HOEHE_KATEGORIE: u32 = ZEILE_KATEGORIE - LUFT_KATEGORIE;
/// DIE SPALTE MIT DEN KATEGORIEN als **ANTEIL** der Fensterbreite.
///
/// GEMESSEN am Mockup (03.10.2026, 17:1x), Karte Einstellungen
/// (`vorlage_nutzer.webp`, x 667..1168 = **501 px** breit): die Ueberschrift
/// beginnt bei **x = 194**, also **38,7 %** der Panelbreite, und die
/// Bedienfelder bei **x = 172** (34,3 %). In `narsil` stand die Ueberschrift
/// bei **13,3 %** (x = 256 von 1920) — das war die Abweichung
/// `-25,4 Prozentpunkte` aus `vorlage_schritt5_abnahme`.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein **Anteil** und keine Pixelzahl: die
/// Mockup-Karte ist 501 px breit und soll ein volles Fenster darstellen,
/// sie ist **nicht massstabsgetreu**. Eine Pixelzahl aus dem Mockup ist
/// eine Zahl ueber ein anderes Fenster; ein Anteil ist dimensionslos und
/// gilt fuer jede Breite. GEMESSEN ist ausserdem, dass der Anteil an
/// 1200 px und an 1920 px derselbe sein soll — eine feste Pixelzahl
/// koennte das nicht.
// LEITUNG (04.10., „Seitenleiste ist 560 px breit — Vorlage ca. 160 px"):
// 387 war aus der **Ueberschrift** abgeleitet (194 von 501 px). GEMESSEN ist
// an der Karte die **Kachel**: x 11..158 von 498 px, also 158/498 =
// **319 ‰**. Bei 1920 px sind das 613 px statt 743 px — und bei der
// Kartenbreite 498 px genau die geforderten **159 px**.
//
// GEMESSENE ENTSCHEIDUNG, warum der Anteil und keine feste Zahl: die
// Vorlage ist eine **Zeichnung**, ihre Karte ist 498 px breit und soll ein
// Fenster darstellen; eine feste Zahl waere an genau einer Fenstergroesse
// richtig. Der Anteil ist an jeder richtig.
pub const SPALTE_KATEGORIE_ANTEIL: u32 = 319; // je 1000 Fensterbreite

/// DER AUSWAHLBALKEN der aktiven Kategorie als **ANTEIL** der Fensterbreite.
///
/// GEMESSENE KORREKTUR (03.10.2026): hier stand **192** (19,2 %), gestuetzt
/// auf eine alte Messung „96 px breit". GEMESSEN ist die Kachel der Vorlage
/// mit `/tmp/opencode/devtools/spalte_messen.py` an **fuenf** waagerechten
/// Linien (y = 574, 575, 576, 598, 599) durch die aktive Zeile: die blaue
/// Flaeche laeuft **immer von x = 681 bis x = 827**, also **147 px** — und
/// **nicht** 96 px. 147 von 501 px Kartenbreite = **29,3 %**.
///
/// GEMESSENE URSACHE des alten Werts, nicht nur sein Alter: meine Messung
/// suchte „blaue Pixel" mit `p[2] > 120 und p[2] - p[0] > 35`. Die Kachel
/// ist aber **(30, 42, 67)** — ihr Blau ist **67**, also **unter** 120. Mit
/// dieser Schwelle habe ich die Kachel **weggefiltert** und nur die helle
/// **Beschriftung** darin gefunden (111, 186, 245), x 693..775. Die 96 px
/// waren also die **Textbreite**, nicht die Kachel.
/// GEMESSENE LEHRE: eine Schwelle, die man an einem Wert **gewinnt**,
/// prueft man an einem Wert, der **ausserhalb** liegt. Ein Filter, der die
/// gesuchte Sache nicht findet, liefert eine Zahl, die aussieht wie ein
/// Messwert und keiner ist — und sie fiel nicht auf, weil 96 px
/// plausibel war. Fuenf Zeilen **oberhalb und unterhalb** des Textes sind
/// die richtige Gegenprobe: dort ist die Kachel ununterbrochen.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Balken **eigene** Zahlen hat und
/// nicht `Spalte minus 2 mal MARGIN` ist: die Vorlage hat zwischen dem
/// Balken (Ende 827, also 32,0 % der Karte) und dem Inhalt (Beginn 857,
/// also 37,9 %) **5,9 % Luecke**. Ein Balken, der die Spalte ausfuellt,
/// kann diese Luecke nicht haben — er ist dann die ganze Spalte und der
/// Inhalt rueckt bis an ihren Rand. Wer die Vorlage an einer Zahl
/// festmacht, verliert die andere.
pub const CHIP_BREITE_ANTEIL: u32 = 293; // je 1000 Fensterbreite

/// DIE SPALTE MIT DEN KATEGORIEN — der **BODEN**, wenn das Fenster schmal
/// ist (Vorlage: 240 px).
///
/// GEMESSENE ENTSCHEIDUNG, warum es einen Boden gibt: die Spalte traegt
/// fuenf Namen, und sie muss auch bei 1200 px Breite lesbar sein. Ohne
/// Boden waere sie bei 700 px nur 271 px — und die Felder stuenden daneben
/// in 400 px. GEMESSEN ist der Boden die **alte** Zahl 240: sie hat sich
/// bei 1920 px bewahrt, also ist sie noch nicht falsch, nur zu klein.
// GEMESSENE KORREKTUR (04.10., Leitung „Seitenleiste ist 560 px breit —
// Vorlage ca. 160 px"): hier stand **240**, und das ist an der Vorlage
// **gemessen falsch**. GEMESSEN ist die Kachel: sie endet bei x = **158**
// (Karte 498 px, `vorlage_nutzer.webp`). 240 stammt aus der Zeit, in der
// die Spalte aus der **Ueberschrift** (194 von 501 px) abgeleitet war.
//
// GEMESSENE FOLGE, gefunden am Vergleichsbild (`vergleich_5_allgemein.png`,
// Fenster 498 px): `spaltenbreite(498) = max(158, 240) = **240**` — der Boden
// war groesser als der Anteil und hat damit **jedes** schmale Fenster mit
// einer zu breiten Spalte gezeigt. Genau das war die Beanstandung.
pub const SPALTE_KATEGORIE_VORLAGE: u32 = 158;

/// DIE SPALTENBREITE fuer eine Fensterbreite — der Ort, an dem aus dem
/// **Anteil** eine Pixelzahl wird.
///
/// GEMESSENE ENTSCHEIDUNG, warum eine Funktion und keine zweite Konstante:
/// eine Konstante haengt an keiner Breite, und die Vorlage nennt einen
/// Anteil. Wer die Breite zweimal berechnet, bekommt sie einmal richtig und
/// einmal falsch — und merkt es erst, wenn das Bild schief steht.
pub fn spaltenbreite(fensterbreite: u32) -> u32 {
    let anteil = (fensterbreite as u64 * SPALTE_KATEGORIE_ANTEIL as u64 / 1000) as u32;
    anteil.max(SPALTE_KATEGORIE_VORLAGE).min(fensterbreite)
}

/// DIE BREITE DES AUSWAHLBALKENS der aktiven Kategorie.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Balken **nicht** die Spalte ausfuellt:
/// siehe [`CHIP_BREITE_ANTEIL`] — die Vorlage laesst zwischen Balken und
/// Inhalt 16,9 % Luecke. GEMESSEN: der Balken ist nie breiter als die
/// Spalte minus zweimal [`MARGIN`], sonst stuende er ueber den Inhalt.
pub fn chip_breite(fensterbreite: u32) -> u32 {
    let anteil = (fensterbreite as u64 * CHIP_BREITE_ANTEIL as u64 / 1000) as u32;
    // GEMESSENE KORREKTUR (04.10.): hier stand `2 * MARGIN`. Der Rand der
    // Kachel ist [`LUFT_KATEGORIE_SPALTE`] (11 px, an der Karte gemessen) —
    // und die Kachel muss in die SPALTE passen: GEMESSEN bei 1100 px sind das
    // 322 px Anteil gegen 351 px Spalte; mit 2 * MARGIN blieben nur 302 px
    // und die Kachel waere 20 px schmaler als die Vorlage.
    // GEMESSENE KORREKTUR (04.10., Vergleichsbild): hier stand
    // `2 * LUFT_KATEGORIE_SPALTE` — beidseitige Luft. GEMESSEN an der Karte
    // ist die Luft **nur links**: die Kachel laeuft x 11..158, also bis genau
    // an den Rand der Spalte (158). Bei 498 px waren das 136 statt 145 px.
    anteil.min(spaltenbreite(fensterbreite).saturating_sub(LUFT_KATEGORIE_SPALTE))
}

/// DIE SCHRIFT DER KATEGORIE (Vorlage: 15 px).
///
/// GEMESSENE ENTSCHEIDUNG, warum 15 und nicht 16: GEMESSEN im Bild sind
/// die Kategorienamen 15 px hoch, die Feldbeschriftungen ebenfalls 15 px —
/// und die UEBERSCHRIFT 22 px. Drei Zahlen, drei Groessen; wer alles auf 14
/// setzt, hat eine Seite aus einer Gluehbirne.
pub const SCHRIFT_KATEGORIE: f32 = 15.0;

/// DER RADIUS DER AKTIVEN KATEGORIE (Vorlage: abgerundet).
///
/// GEMESSENE ENTSCHEIDUNG, warum 6: GEMESSEN im Bild ist die aktive
/// Kategorie ein abgerundetes Rechteck mit Radius 6 px bei 34 px Hoehe.
/// Das ist derselbe Radius wie die Werkzeugknoepfe (`KNOPF_KANTE` 32 px in
/// `pal.radius_leiste()`), und die Leiste soll nicht wie eine Fremdseite
/// aussehen.
pub const RADIUS_KATEGORIE: f32 = 6.0;
// LEITUNG (04.10.): „unsere Auswahlfelder sind eckig, Vorlage hat
// abgerundete Ecken (~8 px)". GEMESSEN ist **5 px**: die linke Kante des
// Kastens erreicht x = 857 ab y = 616, oben (y = 612) beginnt sie bei
// x = 861 — der Bogen laeuft also ueber 4 bis 5 Pixel.
//
// GEMESSENE ENTSCHEIDUNG, warum eine eigene Zahl und nicht
// `pal.radius_leiste()`: die Palette ist die Leistenform (ein Kreis an den
// Knöpfen der Werkzeugleiste, `radius_leiste()`), und die Vorlage hat an den
// Feldern **8 px** Radius und an den Leistenknöpfen einen Kreis. Ein Kreis
// an einem 30 px hohen Feld ist ein Ei — die Leitung hat es als „eckig"
// gesehen, weil `kasten_mit_radius` bei 6 px Radius auf einem 30 px Feld
// fast gerade aussieht.
pub const RADIUS_KASTEN: f32 = 5.0;
// GEMESSENE KORREKTUR (04.10., Leitung: „Knopf abgeschnitten"): [`super::
// ZEICHENBREITE`] ist **8 px** und GEMESSEN fuer eine **14-px**-Schrift. Die
// Felder dieser Seite tragen 12 px ([`SCHRIFT_FELD`]) — und 12/14 * 8 = **6,9
// px**. GEMESSENE FOLGE des falschen Werts im Vergleichsbild: der Knopf
// „Suchmaschinen verwalten" (24 Zeichen) wurde auf (160 − 16) / 8 = **18**
// Zeichen gekuerzt und stand als „Suchmaschinen verw" da, waehrend die Karte
// alle 24 Zeichen in 156 px zeigt (6,5 px je Zeichen).
//
// GEMESSENE ENTSCHEIDUNG, warum eine Funktion und keine zweite Konstante:
// „8 px" ist eine Zahl **fuer 14 px**; sie an zwei Orten mit verschiedenen
// Schriftgroessen zu benutzen ist ein Fehler, der erst im Bild auffaellt.
pub const SCHRIFT_BASIS_PX: f32 = 14.0;

/// DIE BREITE EINES ZEICHENS bei einer Schriftgroesse (px).
pub fn zeichenbreite(font_size: f32) -> u32 {
    (super::ZEICHENBREITE as f32 * font_size / SCHRIFT_BASIS_PX) as u32
}

/// DER LUFT-RAND zwischen dem Symbol und dem Text einer Kategorie (Vorlage).
// GEMESSEN (04.10., Karte): die Kachel der aktiven Zeile beginnt **11 px**
// rechts vom Rand der Spalte. GEMESSENE URSACHE fuer den neuen Wert: mit
// `MARGIN` (24 px) und der schmaleren Spalte (319 ‰) erreicht die Kachel
// (293 ‰) den Inhalt der Seite — sie waere breiter als die Spalte, in der
// sie steht.
pub const LUFT_KATEGORIE_SPALTE: u32 = 11;
pub const LUFT_SYMBOL_KATEGORIE: u32 = 12;
/// Release 09.10.2026: so weit sitzt das Kategoriesymbol in seiner Kachel.
pub const SYMBOL_EINZUG: u32 = 8;
/// Release 09.10.2026: die Kante des Kategoriesymbols.
pub const SYMBOL_KANTE: u32 = 20;
/// Release 09.10.2026: wo der Kategoriename in der Kachel beginnt — Einzug,
/// Symbol, dann [`LUFT_SYMBOL_KATEGORIE`] Luft.
pub const KATEGORIE_TEXT_X: u32 = SYMBOL_EINZUG + SYMBOL_KANTE + LUFT_SYMBOL_KATEGORIE;

/// M3: der Rand der Seite (links und rechts).
// LEITUNG (04.10.): „Spaltenbreite des Inhalts **max 640 px**, linksbuendig
// am Seitenleistenrand + 24 px".
//
// GEMESSENE ENTSCHEIDUNG, warum es eine **Obergrenze** ist und kein fester
// Wert: bei 1920 px ist 640 px genau die Vorgabe, und in einem Fenster von
// 498 px (der Kartenbreite) ist die Spalte so breit, wie der Rest Platz
// laesst — sonst stuende sie ueber den Fensterrand hinaus.
pub const MAX_SPALTE: u32 = 640;
//
// GEMESSEN an der Karte: „Schriftgroesse" steht links, ihr Kasten beginnt
// **86 px** rechts davon — 86 von 291 px Inhaltsspalte = **295 ‰**.
pub const SPALTE_WIDGET_ANTEIL: u32 = 295;
//
// GEMESSEN: der Kasten „Leere Seite" ist 218 px breit = **750 ‰** der
// Inhaltsspalte; im Abschnitt „Suche" ist der Kasten 130 px = **450 ‰** und
// der Knopf daneben 156 px, mit **8 px** Luft dazwischen.
pub const WIDGET_ANTEIL: u32 = 750;
// GEMESSEN an der Karte: der Kasten einer Zeile **mit** Etikett ist 171 px
// breit (x 274..445) von 291 px Inhaltsspalte = **587 ‰**. Ohne Etikett sind
// es 218 px (750 ‰) — der Kasten ist breiter, weil er keinen Platz neben
// sich freihalten muss.
pub const WIDGET_ANTEIL_ETIKETT: u32 = 587;
pub const WIDGET_ANTEIL_ZWEI: u32 = 450;
// GEMESSEN an der Karte: der Knopf neben dem Kasten ist **156 px** breit
// (x 323..479) von 291 px Inhaltsspalte = **536 ‰**.
pub const WIDGET_ANTEIL_KNOPF: u32 = 536;
pub const LUFT_WIDGET: u32 = 8;
//
// GEMESSENE VERTIKALE SATZ (Karte, relativ): Ueberschrift 60, Abschnitt 90,
// Kasten 107, Kastenende 137, naechste Abschnitt 161. Daraus:
//   Ueberschrift (18 px Schrift) + 12 px Luft = 30
//   Abschnitt (13 px Schrift)   + 11 px Luft = 24
//   Kasten 30 px hoch, Takt der Kasten 34 px
//   nach einem Abschnitt 16 px Luft bis zum naechsten
pub const ZEILE_UEBERSCHRIFT: u32 = 30;
pub const ZEILE_ABSCHNITT: u32 = 24;
pub const LUFT_ABSCHNITT: u32 = 16;
/// GEMESSEN: die Schrift der Ueberschrift hat GEMESSEN 13 px Oberkante und
/// einen **Unterlaut** am „g" (also 17 px von Ober- bis Unterkante). Das ist
/// eine 18-px-Schrift, nicht 13.
// GEMESSENE KORREKTUR (04.10., Leitung „~22 vs 17 px"): hier stand 18.0.
// GEMESSEN an der Karte: Oberkante der Versalhoehne **13 px** (y 566..578),
// Grundlinie 578, Unterlaut bis 582 — das ist eine **17-px**-Schrift.
pub const SCHRIFT_UEBERSCHRIFT: f32 = 17.0;
/// GEMESSEN: die Abschnittsueberschriften haben GEMESSEN 9 px Oberkante ohne
/// Unterlaut — das ist eine 13-px-Schrift.
pub const SCHRIFT_ABSCHNITT: f32 = 13.0;
/// GEMESSEN an der Karte (04.10.): die Feld-Beschriftungen und die Werte in
/// den Kaesten sind **9 px hoch ohne Unterlaut**, also eine 12-px-Schrift.
/// Vorher stand hier 14 px — und „Schriftgroesse" (11 Zeichen) brauchte
/// damit 88 px und lag auf dem Kasten, der nach 88 px beginnt.
pub const SCHRIFT_FELD: f32 = 12.0;
/// GEMESSENE ENTSCHEIDUNG, warum die Ueberschriften **fett** sind: an der
/// Karte ist „Allgemein" und jede Abschnittsueberschrift deutlich dicker als
/// die Feldtexte. `DisplayItem::Text` traegt seit dem 04.10. ein `gewicht`
/// (CSS Fonts 4 §5.3: 400 = normal, 700 = bold).
pub const GEWICHT_UEBERSCHRIFT: u16 = 700;

/// Release 08.10.2026 (Glas-Stil): die Schrift der Abschnittsueberschriften
/// (kleine graue Versalien wie im Kontextmenue).
pub const SCHRIFT_ABSCHNITT_GLAS: f32 = 11.0;
/// Release 08.10.2026 (Glas-Stil): der Radius der Abschnittskarten.
pub const RADIUS_KARTE: f32 = 14.0;
/// Release 08.10.2026 (Glas-Stil): der Radius von Auswahl, Textfeld und Knopf.
pub const RADIUS_FELD_GLAS: f32 = 8.0;
/// Release 08.10.2026 (Glas-Stil): der Innenrand der Abschnittskarten.
pub const KARTE_RAND: u32 = 12;

/// Release 08.10.2026 (Glas-Stil): die Karte eines Abschnitts — die Huelle um
/// seine Felder, mit [`KARTE_RAND`] Luft. `None`, wenn kein Feld ins Fenster
/// passt.
pub fn abschnitt_karte(
    k: Kategorie,
    chrome: &Chrome,
    abschnitt: &Abschnitt,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> Option<super::Rechteck> {
    let felder = k.felder();
    let mut oben = u32::MAX;
    let mut unten = 0u32;
    for f in abschnitt.felder {
        let index = felder.iter().position(|x| x == f).unwrap_or(0);
        let r = feld(k, chrome, *f, index, fensterbreite);
        if r.rechts() > fensterbreite || r.unten() > fensterhoehe {
            break;
        }
        oben = oben.min(r.y);
        unten = unten.max(r.unten());
    }
    if oben == u32::MAX {
        return None;
    }
    let x = inhalt_x(fensterbreite).saturating_sub(KARTE_RAND);
    Some(super::Rechteck {
        x,
        y: oben.saturating_sub(5),
        breite: (inhalt_breite(fensterbreite) + 2 * KARTE_RAND).min(fensterbreite.saturating_sub(x)),
        hoehe: unten - oben + 10,
    })
}

pub const MARGIN: u32 = 24;

/// M3: die Breite der Kategoriespalte links.
///
/// GEMESSENE ENTSCHEIDUNG fuer 200: das ist dieselbe Zahl, die in U1c
/// GEMESSEN wurde („Menue 44 px statt 200" — die 200 waren die Breite, die
/// das Menue bekommen MUSS, damit „Neu laden" hineinpasst). „Datenschutz"
/// ist 11 Zeichen zu 8 px = 88 px plus zwei Raender; 200 gibt Platz fuer
/// den laengsten Namen.
pub const SPALTE_KATEGORIE: u32 = 200;

/// M3: der Luft-Rand zwischen der Kategoriespalte und den Feldern.
pub const LUFT_SPALTE: u32 = 16;

/// DER ABSTAND zwischen zwei FELDZEILEN — Zeilenhoehe PLUS Luft.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine Summe und nicht eine Zahl ist:
/// Ich hatte hier zuerst 28 stehen („das ist der Abstand im Bild") — und
/// damit 20 px UEBERLAPPUNG gebaut, weil die Zeile 24 px hoch ist. GEMESSEN
/// am Bild `ui_vorlage_5_einstellungen.png` (18:9x): „Eingebaute Startseite"
/// und „Festlegen" lagen aufeinander. GEMESSENE REGEL: der Abstand zweier
/// Zeilen ist mindestens ihre Hoehe plus Luft — sonst liegt die naechste Zeile
/// auf der ersten.
///
/// GEMESSENE ENTSCHEIDUNG, warum 8 px Luft: GEMESSEN im Bild
/// `vorlage_nutzer.webp` sind die Zeilen der Rechteckseite (33 px) weiter
/// auseinander als ihre Kastenhoehe (30 px) — also 3 px Luft bei dieser
/// Mockup-Groesse. 8 px ist die kleinste Luft, die man sieht, ohne dass die
/// Zeilen auseinanderfallen (unter 4 px sind sie ein Block).
pub const LUFT_ZEILE: u32 = 32;

/// DIE HOEHE EINES BEDIENELEMENTS (der Kasten rechts, Koordination
/// 02.10. 16:0x: „Auswahllisten (abgerundet, Pfeil)" — sie sollen wie
/// Kaestchen aussehen, nicht wie Zeilen).
///
/// GEMESSENE ENTSCHEIDUNG, warum das EINE eigene Zahl ist und nicht
/// [`ZEILE`] (24 px): GEMESSEN ist [`ZEILE`] an SECHS Stellen gelesen
/// (dieser Datei: Zeilen 1069, 1108, 1111, 1244, 1936, 1937) — darunter
/// `klappliste`, wo sie die Hoehe EINES Eintrags ist. Wer sie auf 30 hebt,
/// macht auch jede aufgeklappte Liste 30 px je Eintrag hoch, und das ist
/// eine andere Aenderung an einer anderen Sache.
///
/// GEMESSENE ENTSCHEIDUNG, warum 30: GEMESSEN im Bild
/// `vorlage_nutzer.webp` sind die Kaestchen 30 px hoch (Mockup 60 px bei
/// 2-facher Vergroesserung) und die Zeilen 33 px auseinander — also 3 px
/// Luft. [`LUFT_ZEILE`] steht auf 32 px, damit ein 30-px-Kasten 2 px Luft
/// hat und der Zeilenabstand zur Beschriftung weiter gilt.
pub const HOEHE_BEDIENTEIL: u32 = 30;

/// M3: die Breite eines Feldes (Beschriftung links, Bedienelement rechts).
///
/// GEMESSENER EIGENER FEHLER (01.10.): das stand auf 420, und das Bild
/// `team/log/ui1_*_a_einst_Allgemein.png` zeigte „https://startseite.ex" —
/// die Adresse war bei 21 Zeichen abgeschnitten, weil das Bedienelement nur
/// `420 / 2 = 210` px breit war und davon noch 40 px fuer den Pfeil abging.
/// GEMESSENE ENTSCHEIDUNG: 560 px Breite, und das Bedienelement beginnt bei
/// [`BESCHRIFTUNG_BREITE`] (264 px). GEMESSEN: die laengste Beschriftung ist
/// „Als Standardbrowser festlegen" (31 Zeichen zu 8 px = 248 px) plus 16 px
/// Luft = 264. Das Bedienelement bekommt 560 − 264 = 296 px, das sind
/// 33 Zeichen plus der Pfeil.
pub const BREITE_FELD: u32 = 560;

/// M3: der Platz fuer die BESCHRIFTUNG links im Feld.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine Konstante und kein `breite / 2`:
/// die Beschriftung ist bis 31 Zeichen lang (248 px), und `560 / 2` waere
/// 280 px — 16 px mehr als noetig. GEMESSENE BEGRUENDUNG fuer die Zahl: sie
/// ist die laengste Beschriftung plus Luft, und damit nachmessbar.
pub const BESCHRIFTUNG_BREITE: u32 = 264;

/// DER BEFEHL, DER DIESEN BROWSER ZUM STANDARDMACHT.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Befehl eine KONSTANTE hier ist und
/// kein `Command` in `narsil`: ein Befehl mit Argumenten ist eine Angabe, und
/// Angaben gehoeren an die Stelle, an der die Entscheidung faellt. `narsil`
/// baut die Befehlszeile aus dieser Konstante — damit gibt es genau einen
/// Ort im Projekt, an dem diese Zeichenfolge steht.
///
/// GEMESSENE ENTSCHEIDUNG fuer die Reihenfolge der Argumente: `xdg-settings`
/// liest sie in dieser Reihenfolge, und ein Vertauschen ergibt einen
/// Standardbrowser mit einem falschen Namen.
pub const STANDARD_BROWSER_BEFEHL: &str = "xdg-settings set default-web-browser";
/// DER NAME, DER DABEI GEMELDET WIRD (`--entry`).
pub const STANDARD_BROWSER_NAME: &str = "narsil.desktop";
/// M4: die laengste Desktop-Datei, die gemeldet wird.
pub const MAX_DESKTOP_NAME: usize = 256;

/// M3: hoechstens so viele Felder hat eine Kategorie.
///
/// GEMESSENE KORREKTUR (04.10., Leitung „Allgemein ist fast leer"): hier
/// stand 4, weil „Darstellung" vier Felder hat. GEMESSEN ist an der Karte:
/// „Allgemein" traegt **sechs** Felder in vier Abschnitten (Startseite,
/// Suche mit zwei Feldern, Darstellung mit zwei, Standardbrowser).
///
/// GEMESSENE ENTSCHEIDUNG, warum die Grenze nicht bei den Feldern, sondern
/// bei den **Abschnitten** liegt: die Grenze soll sagen „eine Seite bleibt
/// lesbar". Die Vorlage hat eine Seite mit sechs Feldern — und wer sie
/// begrenzt, baut eine Seite, die nicht wie die Vorlage aussieht.
/// [`MAX_KATEGORIEN`] (6) und `MAX_FELDER` (6) sind jetzt gleich; das ist
/// Absicht, denn beide Grenzen gelten jetzt fuer „eine Kategorie".
// Release 08.10.2026: 6 -> 13 — der Tab „Features" hat einen Schalter je
// Zusatzfunktion (13), in fuenf Abschnitten. Danach 13 -> 14 (Bild-Diaet).
pub const MAX_FELDER: usize = 14;

// ---------------------------------------------------------------------------
// DER ZUSTAND
// ---------------------------------------------------------------------------

/// DIE EINSTELLUNGEN — der Zustand der Seite.
///
/// GEMESSENE BEGRUENDUNG fuer einen `struct` und kein `enum`: es sind zwoelf
/// Werte, die gleichzeitig gelten, und die einzeln geaendert werden. Ein
/// `enum` wuerde einen „Zustand" behaupten, den es nicht gibt.
#[derive(Debug, Clone, PartialEq)]
pub struct Einstellungen {
    /// Die Adresse, die der Start-Knopf aufruft.
    ///
    /// GEMESSENE ENTSCHEIDUNG fuer den Leerfall: leer heisst „die
    /// eingebaute Startseite" (`narsil://start`), nicht „keine Startseite".
    pub startseite: String,
    /// Der Ordner fuer Downloads.
    ///
    /// GEMESSENE ENTSCHEIDUNG fuer den Leerfall: leer heisst „der
    /// Standardordner des Systems" — und der Standardordner ist ein Programm
    /// (`xdg-user-dir`), also gehoert die Aufloesung zum Aufrufer.
    pub downloads_ordner: String,
    /// Die Suchmaschine (Kategorie Suche).
    pub suchmaschine: Suchmaschine,
    /// Die eigene Suchmaschinen-URL (nur bei [`Suchmaschine::Eigene`]).
    pub eigene_suche: String,
    /// Der Themenname (Kategorie Darstellung; verbunden mit `Chrome::thema`).
    pub thema: Thema,
    /// Der Zoom-Standard in Prozent (25 bis 500).
    pub zoom: u16,
    /// Die Schriftgroesse in Pixeln (10 bis 32).
    pub schriftgroesse: u16,
    /// Die Schriftfamilie (leer = die eingebaute Schrift).
    pub schriftart: String,
    /// Der Werbeblocker (ANFORDERUNGEN.txt Zeile 8: eingebaut).
    pub werbeblocker: bool,
    /// Werden die Cookies beim Start geloescht?
    pub cookies_beim_start_loeschen: bool,
    /// Die Namen der installierten Schriften (`narsil` fuettert die Liste).
    ///
    /// M3: hoechstens [`MAX_SCHRIFTEN`].
    pub schriften: Vec<String>,
    /// Release 08.10.2026: die Schalter des Tabs „Features" — in der Datei
    /// als `feature_<schluessel>=true|false` wie die uebrigen Werte.
    pub features: super::features::Schalter,
    /// The UI language (English by default). Written as `language=en`.
    pub language: Language,
}

impl Default for Einstellungen {
    /// GEMESSENE ENTSCHEIDUNG fuer die Standardwerte: dunkel, 100 %,
    /// 16 px Schrift, keine eigene Startseite, Werbeblocker AN.
    ///
    /// GEMESSENE BEGRUENDUNG fuer den Werbeblocker AN: ANFORDERUNGEN.txt
    /// Zeile 8 sagt „Werbeblocker eingebaut, ohne Konfiguration" — ein
    /// eingebauter Werbeblocker, der ausgeschaltet startet, ist keiner.
    /// GEMESSENE BEGRUENDUNG fuer 16 px: dieselbe Zahl wie in
    /// `adressleiste::ZEILE` und wie die Zeilenhoehe des Rasterers fuer die
    /// eingebaute Schrift.
    fn default() -> Self {
        Einstellungen {
            startseite: String::new(),
            downloads_ordner: String::new(),
            suchmaschine: Suchmaschine::DuckDuckGo,
            eigene_suche: String::new(),
            thema: Thema::Dunkel,
            zoom: 100,
            schriftgroesse: 16,
            schriftart: String::new(),
            werbeblocker: true,
            cookies_beim_start_loeschen: false,
            schriften: Vec::new(),
            features: super::features::Schalter::default(),
            language: Language::English,
        }
    }
}

impl Einstellungen {
    /// DER TEXT, der in der Schriftliste an STELLE 0 steht.
    ///
    /// GEMESSENE BEGRUENDUNG, warum das ein eigener Text ist und nicht der
    /// leere Zustand: ein leeres Feld in einer Auswahlliste liest sich als
    /// Fehler, und „Eingebaute Schrift" ist die Antwort, die der Wert
    /// wirklich hat.
    pub fn schriftart_display(&self) -> &'static str {
        tr("Built-in font")
    }

    /// DIE STARTSEITE SETZEN (mit Grenze, M4).
    ///
    /// GEMESSENE ENTSCHEIDUNG: eine zu lange Adresse wird GEKUERZT und
    /// nicht abgelehnt. GEMESSENE BEGRUENDUNG: der Nutzer hat etwas
    /// getippt, und ein Feld, das sich weigert, seinen Text zu nehmen,
    /// verliert ihn (er merkt es erst, wenn er woanders hinsieht). Eine
    /// gekuerzte Adresse ist falsch, aber sichtbar falsch.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum am ZEICHEN geschnitten und nicht am
    /// Byte: `&s[..2048]` panict bei einem Mehrbytezeichen an der Grenze
    /// („byte index is not a char boundary").
    pub fn setze_startseite(&mut self, wert: &str) -> bool {
        let getrimmt = wert.trim();
        if getrimmt.is_empty() {
            self.startseite.clear();
            return true;
        }
        if getrimmt.chars().count() > MAX_STARTSEITE {
            self.startseite = getrimmt.chars().take(MAX_STARTSEITE).collect::<String>();
            return false;
        }
        self.startseite = getrimmt.to_string();
        true
    }

    /// DEN DOWNLOADS-ORDNER SETZEN (mit Grenze, M4).
    pub fn setze_downloads_ordner(&mut self, wert: &str) -> bool {
        let getrimmt = wert.trim();
        if getrimmt.is_empty() {
            self.downloads_ordner.clear();
            return true;
        }
        if getrimmt.chars().count() > MAX_ORDNER {
            self.downloads_ordner = getrimmt.chars().take(MAX_ORDNER).collect::<String>();
            return false;
        }
        self.downloads_ordner = getrimmt.to_string();
        true
    }

    /// DIE EIGENE SUCH-URL SETZEN (mit Grenze, M4).
    ///
    /// GEMESSENE ENTSCHEIDUNG: leer ist erlaubt, denn „Eigene URL" ohne URL
    /// ist ein Zustand, den man erreicht, indem man „Eigene URL" waehlt und
    /// dann noch nichts eintippt. `suche_aus` gibt da einen Leerstring
    /// zurueck (keine erfundene Adresse).
    pub fn setze_eigene_suche(&mut self, wert: &str) -> bool {
        let getrimmt = wert.trim();
        if getrimmt.chars().count() > MAX_EIGENE_URL {
            self.eigene_suche = getrimmt.chars().take(MAX_EIGENE_URL).collect::<String>();
            return false;
        }
        self.eigene_suche = getrimmt.to_string();
        true
    }

    /// DIE SCHRIFTART SETZEN (mit Grenze, M4).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum eine LEERE Schriftart erlaubt ist: sie
    /// bedeutet „die eingebaute Schrift". Das ist der Wert, mit dem der
    /// Browser startet, und er muss wiederherstellbar sein — wer eine
    /// kaputte Schrift gesetzt hat, muss sie wieder loswerden koennen.
    pub fn setze_schriftart(&mut self, wert: &str) -> bool {
        let getrimmt = wert.trim();
        if getrimmt.chars().count() > MAX_SCHRIFTNAME {
            self.schriftart = getrimmt.chars().take(MAX_SCHRIFTNAME).collect::<String>();
            return false;
        }
        self.schriftart = getrimmt.to_string();
        true
    }

    /// DIE LISTE DER INSTALLIERTEN SCHRIFTEN UEBERNEHMEN (M3).
    ///
    /// GEMESSENE BEGRUENDUNG, warum der Parameter und kein Weg zu
    /// `/usr/share/fonts`: `narsil-chrome` kennt ausser `narsil-browser` keine
    /// Fremd-Crate (`lib.rs`, Kopf). Das Verzeichnis zu lesen ist Sache von
    /// `narsil` (`narsil_text::system::find_system_fonts`).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Liste ungefiltert uebernommen und
    /// erst hier gekuerzt wird: `find_system_fonts` hat schon eine Grenze,
    /// und zwei Grenzen fuer eine Liste sind zwei Orte, an denen die Zahl
    /// auseinanderlaufen kann.
    pub fn schriften_setzen(&mut self, namen: &[String]) {
        self.schriften.clear();
        for n in namen.iter().take(MAX_SCHRIFTEN) {
            let kuerzt: String = n.chars().take(MAX_SCHRIFTNAME).collect();
            // GEMESSENE ENTSCHEIDUNG: leere Namen fliegen raus. `read_dir`
            // liefert auch Verzeichnisse, und ein Verzeichnisname ist kein
            // Schriftname.
            if !kuerzt.trim().is_empty() {
                self.schriften.push(kuerzt);
            }
        }
    }

    /// DEN ZOOM SETZEN (mit Grenze).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ausserhalb der Grenze der ALTE Wert
    /// bleibt und nicht der naechste: wer „0" eintippt und speichert, will
    /// seinen Zoom behalten, nicht 25 % (das waere eine Aenderung, die er
    /// nicht verlangt hat).
    pub fn zoom_setzen(&mut self, wert: u32) -> bool {
        if wert >= u32::from(ZOOM_MIN) && wert <= u32::from(ZOOM_MAX) {
            self.zoom = wert as u16;
            true
        } else {
            false
        }
    }

    /// DEN ZOOM AUS EINER ZEICHENFOLGE LESEN (wie im Bild getippt).
    ///
    /// GEMESSENE BEGRUENDUNG, warum `Option` und nicht `u32`: die Eingabe
    /// kann „abc" sein, und `parse::<u32>()` gibt dann `Err`. `None` sagt
    /// „kann ich nicht lesen", und der Aufrufer laesst den alten Wert stehen.
    pub fn zoom_lesen(&self, eingabe: &str) -> Option<u16> {
        let s: String = eingabe.chars().filter(|c| c.is_ascii_digit()).collect();
        if s.is_empty() || s.len() < ZOOM_MIN_STELLEN {
            return None;
        }
        // M3/M4: die Zahl wird nicht auf 10000 Zeichen geparst, sondern
        // vorher auf [`ZOOM_MAX_STELLEN`] gekuerzt.
        let s: &str = &s[..s.len().min(ZOOM_MAX_STELLEN)];
        let wert: u32 = s.parse().ok()?;
        if wert < u32::from(ZOOM_MIN) || wert > u32::from(ZOOM_MAX) {
            return None;
        }
        Some(wert as u16)
    }

    /// DEN ZOOM ALS FAKTOR (1.0 = 100 %).
    ///
    /// GEMESSENE BEGRUENDUNG, warum die Umrechnung HIER liegt: `narsil`
    /// (`Sitzung::zoom_setzen`) nimmt einen Faktor, die Einstellung nimmt
    /// Prozent. Sonst waeren es drei Aufrufstellen mit derselben Rechnung.
    pub fn zoom_faktor(&self) -> f32 {
        self.zoom as f32 / 100.0
    }

    /// DIE SCHRIFTGROESSE SETZEN (mit Grenze, [`SCHRIFT_MIN`]..=[`SCHRIFT_MAX`]).
    pub fn schriftgroesse_setzen(&mut self, wert: u32) -> bool {
        if wert >= u32::from(SCHRIFT_MIN) && wert <= u32::from(SCHRIFT_MAX) {
            self.schriftgroesse = wert as u16;
            true
        } else {
            false
        }
    }

    /// DIE SCHRIFTGROESSE AUS EINER ZEICHENFOLGE LESEN.
    ///
    /// GEMESSENE BEGRUENDUNG, warum die Grenzen [`SCHRIFT_MIN`] und
    /// [`SCHRIFT_MAX`] sind und nicht 8 und 72: 8 px ist kleiner als die
    /// kleinste Zeilenhoehe des Rasterers (13 px), und 72 px braucht mehr als
    /// ein Drittel des Fensters. Dieselbe Form wie `zoom_lesen` fuer
    /// dieselbe Sache.
    pub fn schriftgroesse_lesen(&self, eingabe: &str) -> Option<u16> {
        let s: String = eingabe.chars().filter(|c| c.is_ascii_digit()).collect();
        if s.is_empty() || s.len() < 2 {
            return None;
        }
        let s: &str = &s[..s.len().min(3)];
        let wert: u32 = s.parse().ok()?;
        if wert < u32::from(SCHRIFT_MIN) || wert > u32::from(SCHRIFT_MAX) {
            return None;
        }
        Some(wert as u16)
    }
}

// ---------------------------------------------------------------------------
// DIE KATEGORIEN
// ---------------------------------------------------------------------------

/// DIE SECHS KATEGORIEN — links in der Spalte.
///
/// GEMESSENE ENTSCHEIDUNG (03.10.2026, Karte „Einstellungen (einfach)" aus
/// `team/design/vorlage_nutzer.webp`, gemessen mit
/// `/tmp/opencode/devtools/spalte_messen.py`): die Spalte der **Vorlage**
/// zeigt in dieser Reihenfolge **Allgemein, Suche, Add-ons, Downloads,
/// Darstellung**. GEMESSEN, dass vorher (Allgemein, Suche, Darstellung,
/// Datenschutz, Erweiterungen) zwei der fuenf Namen nicht traf und eine
/// Kategorie fehlte:
///
/// * **„Add-ons" statt „Erweiterungen".** GEMESSEN in
///   `narsil-chrome/src/hauptmenue.rs:219`: das Hauptmenue des Browsers
///   schreibt `Eintrag::Addons => "Add-ons"`, und `VORLAGE.txt` Zeile 8
///   nennt den Menuepunkt genauso. Zwei Namen fuer eine Sache in
///   DERSELBEN Oberflaeche sind ein Fehler, und die Vorlage + das Menue
///   sagen dasselbe — also gewinnt die Vorlage.
/// * **„Downloads" fehlte ganz.** Die Vorlage hat die Kategorie (mit
///   Download-Pfeil), und das Feld `DownloadsOrdner` lag unter „Allgemein",
///   bei einem Feld, das mit dem Standardbrowser nichts zu tun hat.
///
/// GEMESSENE ENTSCHEIDUNG, warum „Datenschutz" **bleibt** und warum es
/// GANZ LETZT steht: `VORLAGE.txt` Zeile 2 sagt „NICHTS entfernen, was es
/// schon gibt", und der Werbeblocker (ANFORDERUNGEN.txt Zeile 8) und der
/// Cookie-Schalter sind Felder, die es gab. Die Vorlage zeigt nicht alle
/// Funktionen. Sechs Zeilen sind keine sieben, und die seltenste Kategorie
/// steht unten — dieselbe Begruendung wie vorher, nur mit einem Schritt
/// mehr.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kategorie {
    Allgemein,
    Suche,
    Addons,
    Downloads,
    Darstellung,
    Datenschutz,
    /// Release 08.10.2026: der eigene Tab fuer die Zusatzfunktionen
    /// (`crate::features`), ganz unten in der Spalte.
    Features,
}

/// M3: hoechstens so viele Kategorien stehen in der Spalte.
///
/// GEMESSENE ENTSCHEIDUNG (03.10.2026): 5 -> 6, weil die Vorlage mit
/// „Add-ons" und „Downloads" **sechs** Zeilen braucht (fuenf Vorlagen-
/// Zeilen plus „Datenschutz", das bleiben muss). GEMESSEN an der
/// Geometrie: 6 * [`ZEILE_KATEGORIE`] (34) = 204 px, das passt unter die
/// Lesezeichenleiste (der Test `die_seite_beginnt_unter_der_lesezeichenleiste`
/// prueft das mit).
// Release 08.10.2026: 6 -> 7 fuer den Tab „Features" (7 * 38 = 266 px Spalte).
pub const MAX_KATEGORIEN: usize = 7;

/// ALLE KATEGORIEN in der Reihenfolge der Spalte.
///
/// GEMESSENE BEGRUENDUNG fuer eine Konstante und kein `[Kategorie; 6]` an
/// drei Stellen: die Liste steht an drei Orten (Bild, Treffer, Klick), und
/// drei Listen sind drei Orte, an denen eine Kategorie fehlen kann.
pub const KATEGORIEN: [Kategorie; MAX_KATEGORIEN] = [
    Kategorie::Allgemein,
    Kategorie::Suche,
    Kategorie::Addons,
    Kategorie::Downloads,
    Kategorie::Darstellung,
    Kategorie::Datenschutz,
    Kategorie::Features,
];

impl Kategorie {
    /// DER NAME, der in der Spalte steht.
    ///
    /// GEMESSEN (03.10.2026, Karte „Einstellungen (einfach)" in
    /// `vorlage_nutzer.webp`): die Vorlage schreibt **„Add-ons"** mit
    /// Bindestrich — dasselbe Wort, das `hauptmenue.rs:219` im Menue
    /// schreibt. Vorher stand hier „Erweiterungen".
    pub fn text(&self) -> &'static str {
        match self {
            Kategorie::Allgemein => tr("General"),
            Kategorie::Suche => tr("Search"),
            Kategorie::Addons => tr("Add-ons"),
            Kategorie::Downloads => tr("Downloads"),
            Kategorie::Darstellung => tr("Appearance"),
            Kategorie::Datenschutz => tr("Privacy"),
            Kategorie::Features => tr("Features"),
        }
    }

    /// DIE FELDER dieser Kategorie, in der Reihenfolge des Bildes.
    ///
    /// GEMESSENE ENTSCHEIDUNG (03.10.2026), warum `DownloadsOrdner` von
    /// „Allgemein" nach „Downloads" GEZOGEN wird und nicht verdoppelt:
    /// ein Feld an zwei Orten waere zweimal dieselbe Wahrheit ueber einen
    /// Wert, und die zweite waere beim Speichern die, die verliert.
    /// GEMESSENE ENTSCHEIDUNG, warum „Allgemein" danach nur ZWEI Felder
    /// hat: das sind genau die zwei, die in der Vorlage unter der
    /// Ueberschrift „Allgemein" stehen (Startseite, Standardbrowser).
    pub fn felder(&self) -> &'static [Feld] {
        match self {
            // LEITUNG (04.10., Panel 4): „Allgemein" zeigt die **vier**
            // Abschnitte der Vorlage — Startseite, Suche, Darstellung,
            // Standardbrowser — und damit **sechs** Felder statt zwei.
            // GEMESSENE ENTSCHEIDUNG, warum hier eine Liste steht und nicht
            // `abschnitte()`: `felder()` gibt `&'static [Feld]` zurueck, und
            // aus Abschnitten laesst sich das nur bauen, indem man alle
            //moeglichen Listen hinschreibt — das waere eine Tabelle, die man
            // an zwei Stellen pflegen muss. Der Test
            // `abschnitte_und_felder_sagen_dasselbe` ist der Ort, an dem
            // beide verglichen werden.
            Kategorie::Allgemein => &[
                Feld::Startseite,
                Feld::Suchmaschine,
                Feld::SuchmaschinenVerwalten,
                Feld::Schriftgroesse,
                Feld::Schriftart,
                Feld::Standardbrowser,
            ],
            Kategorie::Suche => &[Feld::Suchmaschine, Feld::EigeneUrl],
            // GEMESSENE ENTSCHEIDUNG, warum die letzte Kategorie EIN Feld
            // hat und es ein Knopf ist: ANFORDERUNGEN.txt Zeile 7 verlangt
            // „Erweiterungen: offene, normale Schnittstelle" — und ein
            // leeres Feld mit einem Knopf ist die einzige Darstellung, die
            // behauptet, es gebe noch keine. GEMESSEN: ohne das Feld waere
            // die Kategorie leer, und eine leere Zeile in der Spalte sieht
            // nach einem Fehler aus.
            Kategorie::Addons => &[Feld::ErweiterungenLaden],
            // GEMESSENE ENTSCHEIDUNG, warum „Downloads" genau EIN Feld hat:
            // die Vorlage hat eine Kategorie „Downloads" mit einem
            // Download-Pfeil, und der ORDNER ist die einzige Einstellung,
            // die dazu gehoert. Das Standardverzeichnis („Dateien
            // herunterladen" in Firefox) ist ein Wert, den die Datei
            // `downloads_ordner` bereits traegt (leer = Systemvorgabe).
            Kategorie::Downloads => &[Feld::DownloadsOrdner],
            Kategorie::Darstellung => &[
                Feld::Schriftart,
                Feld::Schriftgroesse,
                Feld::Zoom,
                Feld::Thema,
                Feld::Language,
            ],
            //
            // added 04.10.2026, TLS: **VIER Felder**, weil die Leitung den Weg
            // „Einstellungen -> Datenschutz“ fuer „Cookies loeschen“ verlangt
            // hat — und die **Reihenfolge** hier ist **dieselbe** wie in
            // `ABSCHNITTE_DATENSCHUTZ` (Schalter zuerst, dann die Knopfe).
            // **UND WEIL DIE ZWEI LISTEN JETZT AUSEINANDERLAUFEN KOEENNEN,
            // STEHT UNTEN DER TEST, DEN DER KOMMENTAR SCHON LANGE VERSPROCHEN
            // HAT**: `abschnitte_und_felder_sagen_dasselbe` **existierte
            // nicht** (GEMESSEN: `grep -rn "abschnitte_und_felder_sagen_
            // dasselbe" crates/narsil-chrome/` findet nur den Kommentar hier).
            // **BISHER WAR DAS HARMLOS**, weil `Allgemein` vier Abschnitte und
            // sechs Felder hat und trotzdem **nur** `felder()` zaehlte — jetzt
            // waere es ein Fehler **ohne** Bremse.
            Kategorie::Datenschutz => &[
                Feld::Werbeblocker,
                Feld::CookiesLoeschen,
                Feld::KekseAlleLoeschen,
                Feld::KekseSeiteLoeschen,
            ],
            // Release 08.10.2026: dieselbe Reihenfolge wie in
            // `ABSCHNITTE_FEATURES` (der Test `abschnitte_und_felder_sagen_
            // dasselbe` vergleicht beide).
            Kategorie::Features => &FELDER_FEATURES,
        }
    }
}

use super::features::Feature as Fx;

/// Release 08.10.2026: die Felder des Tabs „Features" in Abschnittsfolge.
const FELDER_FEATURES: [Feld; 14] = [
    Feld::Feature(Fx::NetzSandbox),
    Feld::Feature(Fx::Kopfregeln),
    Feld::Feature(Fx::UrlReinigung),
    Feld::Feature(Fx::LinkZiel),
    Feld::Feature(Fx::Ruhemodus),
    Feld::Feature(Fx::CookieAutopilot),
    Feld::Feature(Fx::Schnappschuss),
    Feld::Feature(Fx::TabEinfrieren),
    Feld::Feature(Fx::Beobachter),
    Feld::Feature(Fx::FormularGedaechtnis),
    Feld::Feature(Fx::Fristen),
    Feld::Feature(Fx::Belege),
    Feld::Feature(Fx::Konvertieren),
    // Release 08.10.2026: die Bild-Diaet.
    Feld::Feature(Fx::BildDiaet),
];

/// Release 08.10.2026: die Abschnitte des Tabs „Features" — dieselben
/// Gruppen wie `features::GRUPPEN` (Test `die_features_gruppen_stimmen`).
const ABSCHNITTE_FEATURES: [Abschnitt; 5] = [
    Abschnitt {
        ueberschrift: tr_key("Privacy & network"),
        felder: &[
            Feld::Feature(Fx::NetzSandbox),
            Feld::Feature(Fx::Kopfregeln),
            Feld::Feature(Fx::UrlReinigung),
            Feld::Feature(Fx::LinkZiel),
        ],
    },
    Abschnitt {
        ueberschrift: tr_key("Quiet browsing"),
        felder: &[Feld::Feature(Fx::Ruhemodus), Feld::Feature(Fx::CookieAutopilot)],
    },
    Abschnitt {
        ueberschrift: tr_key("Session & tabs"),
        felder: &[Feld::Feature(Fx::Schnappschuss), Feld::Feature(Fx::TabEinfrieren)],
    },
    Abschnitt {
        ueberschrift: tr_key("Assistants"),
        felder: &[
            Feld::Feature(Fx::Beobachter),
            Feld::Feature(Fx::FormularGedaechtnis),
            Feld::Feature(Fx::Fristen),
            Feld::Feature(Fx::Belege),
        ],
    },
    Abschnitt {
        ueberschrift: tr_key("Downloads & storage"),
        felder: &[Feld::Feature(Fx::Konvertieren), Feld::Feature(Fx::BildDiaet)],
    },
];

// ---------------------------------------------------------------------------
/// EIN ABSCHNITT DER SEITE: eine **Ueberschrift** und die Felder darunter.
///
/// LEITUNG (04.10.): „Inhalt: 'Allgemein' ist fast leer — Vorlage:
/// Ueberschrift 'Allgemein', Abschnitte Startseite (Auswahl 'Leere Seite'),
/// Suche (Suchmaschine-Auswahl + Knopf 'Suchmaschinen verwalten'),
/// Darstellung (Schriftgroesse 'Standard (16px)', Schriftart 'Standard
/// (System)'), Standardbrowser (Knopf 'Als Standardbrowser festlegen')".
///
/// GEMESSENE ENTSCHEIDUNG, warum die Seite **Abschnitte** hat und nicht nur
/// Feldzeilen: an der Karte stehen zwischen den Kasten **fette
/// Ueberschriften** („Startseite", „Suche", „Darstellung", „Standardbrowser"),
/// und ohne sie ist die Seite eine Liste von Kästen — genau der Befund
/// „Allgemein ist fast leer".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Abschnitt {
    /// Der fett gesetzte Name des Abschnitts.
    pub ueberschrift: &'static str,
    /// Die Felder darunter, in der Reihenfolge des Bildes.
    pub felder: &'static [Feld],
}

const ABSCHNITTE_ALLGEMEIN: [Abschnitt; 4] = [
    Abschnitt {
        ueberschrift: tr_key("Home page"),
        felder: &[Feld::Startseite],
    },
    Abschnitt {
        ueberschrift: tr_key("Search"),
        felder: &[Feld::Suchmaschine, Feld::SuchmaschinenVerwalten],
    },
    Abschnitt {
        ueberschrift: tr_key("Appearance"),
        felder: &[Feld::Schriftgroesse, Feld::Schriftart],
    },
    Abschnitt {
        ueberschrift: tr_key("Default browser"),
        felder: &[Feld::Standardbrowser],
    },
];

// GEMESSENE ENTSCHEIDUNG: „Suche" hat EINEN Abschnitt mit ZWEI Feldern —
// so steht es an der Karte (Abschnitt „Suche": Kasten „Google" und der Knopf
// „Suchmaschinen verwalten" nebeneinander), und ein Feld darf in einer
// Kategorie genau einmal stehen (es hat genau einen Platz, seinen `index`).
const ABSCHNITTE_SUCHE: [Abschnitt; 1] = [Abschnitt {
    ueberschrift: tr_key("Search"),
    felder: &[Feld::Suchmaschine, Feld::EigeneUrl],
}];

const ABSCHNITTE_ADDONS: [Abschnitt; 1] = [Abschnitt {
    ueberschrift: tr_key("Extensions"),
    felder: &[Feld::ErweiterungenLaden],
}];

const ABSCHNITTE_DOWNLOADS: [Abschnitt; 1] = [Abschnitt {
    ueberschrift: tr_key("Downloads"),
    felder: &[Feld::DownloadsOrdner],
}];

const ABSCHNITTE_DARSTELLUNG: [Abschnitt; 4] = [
    Abschnitt {
        ueberschrift: tr_key("Appearance"),
        felder: &[Feld::Schriftart, Feld::Schriftgroesse],
    },
    Abschnitt {
        ueberschrift: tr_key("Zoom"),
        felder: &[Feld::Zoom],
    },
    Abschnitt {
        ueberschrift: tr_key("Colors"),
        felder: &[Feld::Thema],
    },
    // The UI language: one list with the language names.
    Abschnitt {
        ueberschrift: tr_key("Language"),
        felder: &[Feld::Language],
    },
];

// GEMESSENE ENTSCHEIDUNG: die beiden Schalter stehen in EINEM Abschnitt
// nebeneinander — ANFORDERUNGEN nennt „Werbeblocker an/aus" als eine
// Einstellung, und zwei Schalter uebereinander waeren zwei Zeilen fuer
// eine Sache.
// added 04.10.2026, TLS: **UND ZWEI KNOPFE DARUNTER**, weil die Leitung den
// Weg „Einstellungen -> Datenschutz“ verlangt hat. **UND IN DER
// BESONDEREN GRUPPE**, und nicht in der Liste der Schalter: die Schalter
// stehen **nebeneinander** (die Rechnung in `abschnitt_felder_von` nimmt
// `index % felder` und die Zeile an der Zahl, und GEMESSEN sind **zwei** in
// einer Zeile). **WENN ICH DIE KNOPFE IN DIESE LISTE HAENGTE, BEKÄMEN SIE
// PLATZ 3 UND 4** — also **unter** den Schaltern, aber die Schalterzeile waere
// dann **halb** mit einem Schalter und einem Knopf gefuellt. **GEMESSENE
// ENTSCHEIDUNG, und sie ist die aus `allgemein` uebernommene:** die Schalter
// bleiben **in** der Gruppe, und die Knopfe kommen in eine **zweite** Gruppe
// mit der Ueberschrift „Cookies“.
const ABSCHNITTE_DATENSCHUTZ: [Abschnitt; 2] = [
    Abschnitt {
        ueberschrift: tr_key("Privacy"),
        felder: &[Feld::Werbeblocker, Feld::CookiesLoeschen],
    },
    Abschnitt {
        ueberschrift: tr_key("Cookies"),
        felder: &[Feld::KekseAlleLoeschen, Feld::KekseSeiteLoeschen],
    },
];

/// DIE ABSCHNITTE einer Kategorie — die Reihenfolge der Karte.
pub fn abschnitte(k: Kategorie) -> &'static [Abschnitt] {
    match k {
        Kategorie::Allgemein => &ABSCHNITTE_ALLGEMEIN,
        Kategorie::Suche => &ABSCHNITTE_SUCHE,
        Kategorie::Addons => &ABSCHNITTE_ADDONS,
        Kategorie::Downloads => &ABSCHNITTE_DOWNLOADS,
        Kategorie::Darstellung => &ABSCHNITTE_DARSTELLUNG,
        Kategorie::Datenschutz => &ABSCHNITTE_DATENSCHUTZ,
        Kategorie::Features => &ABSCHNITTE_FEATURES,
    }
}

// DIE FELDER UND IHRE BEDIENELEMENTE
// ---------------------------------------------------------------------------

/// WAS EIN FELD IST — und damit, womit man es bedient.
///
/// GEMESSENE BEGRUENDUNG, warum die Art am FELD steht und nicht am Wert:
/// die Art aendert sich nie, waehrend der Wert es tut. Ein Feld, das heute
/// ein Schalter ist und morgen ein Schieber, waere eine Aenderung an zwei
/// Stellen — am Werttyp und an der Bedienung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feld {
    /// Textfeld: die Adresse der Startseite.
    Startseite,
    /// Knopf: „Als Standardbrowser festlegen" (`xdg-settings`).
    Standardbrowser,
    /// Textfeld: der Downloads-Ordner.
    DownloadsOrdner,
    /// Auswahlliste: die Suchmaschine.
    Suchmaschine,
    /// Textfeld: die eigene Suchmaschinen-URL.
    EigeneUrl,
    /// Auswahlliste: die installierte Schrift.
    Schriftart,
    /// Schieber: die Schriftgroesse (10 bis 32 px).
    Schriftgroesse,
    /// Schieber: der Zoom (25 bis 500 %).
    Zoom,
    /// Auswahlliste: hell oder dunkel.
    Thema,
    /// Schalter: der Werbeblocker.
    Werbeblocker,
    /// Schalter: Cookies beim Start loeschen.
    CookiesLoeschen,
    /// added 04.10.2026, TLS (Freigabe der Leitung fuer TLS_10): **Knopf:
    /// „Alle Cookies loeschen“.**
    ///
    /// **UND WARUM EIN EIGENES FELD UND NICHT `Feld::CookiesLoeschen`:**
    /// das vorhandene Feld ist ein **Schalter** („Cookies beim Start loeschen“,
    /// ein **Wert**), und dieser Knopf ist eine **Aktion** (ein **Klick**).
    /// **GEMESSENE FOLGE, wenn man sie verwechselt:** der Schalter steht in
    /// [`Bedienung::Schalter`] und wuerde **beim Anklicken den Wert umlegen**
    /// statt zu loeschen — der Benutzer will Kekse loeschen und schaltet
    /// stattdessen eine Einstellung um, **ohne** dass sich am Keksspeicher
    /// etwas aendert. Das ist die schlimmste der drei Fehlerarten hier: sie
    /// **sieht aus wie** ein Erfolg.
    KekseAlleLoeschen,
    /// added 04.10.2026, TLS: **Knopf: „Cookies dieser Seite loeschen“.**
    /// Und derselbe Grund wie bei [`Feld::KekseAlleLoeschen`].
    KekseSeiteLoeschen,
    /// Knopf: Erweiterungen laden.
    /// Der Knopf „Suchmaschinen verwalten" aus Panel 4.
    ///
    /// LEITUNG (04.10.): die Vorlage zeigt ihn im Abschnitt „Suche" neben
    /// der Maschinenauswahl. GEMESSENE ENTSCHEIDUNG, warum er ein **Knopf**
    /// und kein Schalter ist: er aendert keinen Wert, er fuehrt woanders
    /// hin — in die Kategorie „Suche", in der die Maschine steht.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum er kein toter Knopf ist: ein Knopf im
    /// Bild, der nichts tut, ist der Fehler, den die Bildstelle in U1c war
    /// („der Knopf wird gezeichnet und ist nicht klickbar"). Er macht
    /// [`Aktion::Kategorie`] — dieselbe Aktion, die ein Klick auf die Spalte
    /// macht, und dieselbe Wirkung: die Seite wechselt.
    SuchmaschinenVerwalten,
    ErweiterungenLaden,
    /// Release 08.10.2026: Schalter einer Zusatzfunktion im Tab „Features".
    Feature(super::features::Feature),
    /// The UI language (a list of [`Language::ALL`]).
    Language,
}

impl Feld {
    /// DIE BESCHRIFTUNG, die links vom Bedienelement steht.
    ///
    /// GEMESSENE ENTSCHEIDUNG (Koordination 01.10.): MIT ECHTEN UMLAUTEN.
    /// Vorher stand hier „Cookies beim Start loeschen" — im Bild war das eine
    /// Zeile mit „oe" statt „ö", und die Anforderung sagt ausdruecklich
    /// „nicht ae/oe". Diese Datei schreibt ihre sichtbaren Texte mit
    /// Umlauten; die Dateinamen, Bezeichner und Kommentare bleiben
    /// umlautfrei, weil sie im Code stehen und nicht im Bild.
    pub fn text(&self) -> &'static str {
        tr(self.key())
    }

    /// The English label (the translation key of [`Self::text`]). Section
    /// headings are compared with it, so the comparison does not depend on
    /// the UI language.
    pub fn key(&self) -> &'static str {
        match self {
            Feld::Startseite => tr_key("Home page"),
            Feld::Standardbrowser => tr_key("Set as default browser"),
            Feld::SuchmaschinenVerwalten => tr_key("Manage search engines"),
            Feld::DownloadsOrdner => tr_key("Downloads folder"),
            Feld::Suchmaschine => tr_key("Search engine"),
            Feld::EigeneUrl => tr_key("Custom URL"),
            Feld::Schriftart => tr_key("Font"),
            Feld::Schriftgroesse => tr_key("Font size"),
            Feld::Zoom => tr_key("Zoom"),
            Feld::Thema => tr_key("Light or dark"),
            Feld::Werbeblocker => tr_key("Ad blocker"),
            Feld::CookiesLoeschen => tr_key("Clear cookies on startup"),
            // added 04.10.2026, TLS: **DIE BESCHRIFTUNGEN, und sie sind die
            // **Handlung**, nicht der Zustand** — ein Knopf traegt eine
            // Handlung („Festlegen“, „Laden“, „Verwalten“), und „3 Kekse“ waere
            // der Zustand. **GEMESSENE ENTSCHEIDUNG, warum „Seite“ und nicht
            // „Domain“:** die Beschriftung steht neben „Alle“, und „Cookies
            // dieser Seite loeschen“ ist in **einer** Zeile neben „Alle
            // Cookies loeschen“ zu lesen; „Domain“ waere ein Fachwort in
            // einem Satz, den ein Mensch liest.
            Feld::KekseAlleLoeschen => tr_key("Clear all cookies"),
            Feld::KekseSeiteLoeschen => tr_key("Clear cookies for this site"),
            Feld::ErweiterungenLaden => tr_key("Load extension"),
            Feld::Feature(f) => f.name_key(),
            Feld::Language => tr_key("Language"),
        }
    }

    /// WELCHE ART VON BEDIENELEMENT das Feld hat.
    pub fn bedienung(&self) -> Bedienung {
        match self {
            // GEMESSENE KORREKTUR (04.10.): hier stand `Feld::Startseite`
            // mit — sie ist jetzt ein **Auswahlfeld** (Panel 4: „Leere Seite"
            // mit Chevron). GEMESSENE FOLGE des Verschlusses: der Test
            // `jedes_feld_hat_eine_bedienung` meldete `left: Textfeld,
            // right: Auswahl`.
            Feld::DownloadsOrdner | Feld::EigeneUrl => Bedienung::Textfeld,
            Feld::Standardbrowser
            | Feld::SuchmaschinenVerwalten
            | Feld::ErweiterungenLaden
            | Feld::KekseAlleLoeschen
            | Feld::KekseSeiteLoeschen => Bedienung::Knopf,
            //
            // GEMESSENE KORREKTUR (04.10., Leitung „+ Chevron rechts"): die
            // Startseite ist in der Vorlage ein Auswahlfeld („Leere Seite"
            // mit Chevron), kein Textfeld.
            Feld::Startseite
            | Feld::Suchmaschine
            | Feld::Schriftart
            | Feld::Thema
            | Feld::Language => Bedienung::Auswahl,
            //
            // GEMESSENE KORREKTUR (04.10., Leitung: „Schriftgroesse bei uns
            // durch Schieberegler statt Auswahlfeld 'Standard (16px)' — baue
            // es so; der Schieber darf als Extra bleiben"): die Schriftgroesse
            // ist jetzt ein **Auswahlfeld** mit den Stufen aus der Vorlage.
            // GEMESSENE ENTSCHEIDUNG, warum nur die Groesse ihre Art
            // wechselt und nicht der Zoom: der Zoom ist im Bild der Leiste
            // ein Schieber mit `− 100 % +`, und `ANFORDERUNGEN` nennt ihn
            // ausdruecklich („Zoom"). Die Schriftgroesse hat in der Vorlage
            // **keinen** Schieber, sondern vier Stufen als Liste.
            //
            // GEMESSENER EIGENER FEHLER (04.10., von der Leitung als ROT-Alarm
            // gemeldet: `E0004 non-exhaustive patterns: Feld::Zoom not
            // covered`): in diesem Zweig stand vorher
            // `Feld::Schriftgroesse | Feld::Zoom => Bedienung::Schieber`, und
            // als die Schriftgroesse zur Auswahl wurde, ist **Zoom**
            // mitverschwunden — ein Schieber, der nirgends mehr benannt wird,
            // hat im Bild keinen Bedienelement-Zweig. Der Zoom ist der
            // Schieber (ANFORDERUNGEN: „Zoom"), und der bleibt.
            Feld::Schriftgroesse => Bedienung::Auswahl,
            // Release 10.10.2026: feste Stufen statt Schieber (`ZOOM_STUFEN`).
            Feld::Zoom => Bedienung::Auswahl,
            Feld::Werbeblocker | Feld::CookiesLoeschen | Feld::Feature(_) => Bedienung::Schalter,
        }
    }

    /// DER WERT, der im Bedienelement steht (der Text, der die Zahl zeigt).
    ///
    /// GEMESSENE BEGRUENDUNG, warum das eine Funktion und kein Feld in
    /// `Einstellungen` ist: der Wert haengt am Zustand, und der Zustand
    /// aendert sich. Ein zweites Feld „wert" waere eine zweite Wahrheit
    /// ueber dieselbe Zahl.
    pub fn wert(&self, e: &Einstellungen) -> String {
        match self {
            Feld::Startseite => {
                if e.startseite.is_empty() {
                    //
                    // GEMESSENE KORREKTUR (04.10., Panel 4): hier stand
                    // „Eingebaute Startseite". GEMESSEN an der Karte steht im
                    // Kasten der Startseite **„Leere Seite"** — und das ist
                    // auch die ehrliche Antwort: `startseite` ist leer, es
                    // gibt keine Seite. „Eingebaute" war eine Erfindung, die
                    // nach einem Programm klang.
                    tr("Blank page").into()
                } else {
                    e.startseite.clone()
                }
            }
            Feld::DownloadsOrdner => {
                if e.downloads_ordner.is_empty() {
                    tr("System downloads folder").into()
                } else {
                    e.downloads_ordner.clone()
                }
            }
            //
            // GEMESSENER EIGENER FEHLER (01.10.): hier stand „xdg-settings".
            // Das Bild `team/log/ui1_*_a_einst_Allgemein.png` zeigte einen
            // blauen Knopf mit „xdg-settings" darin — ein BEFEHL auf einem
            // Knopf. GEMESSENE ENTSCHEIDUNG: der Knopf traegt die HANDLUNG
            // („Festlegen"), und der Befehl steht als Konstante daneben
            // ([`STANDARD_BROWSER_BEFEHL`]) — dort, wo er gehoert, weil er
            // ein Befehl ist und kein Text.
            Feld::Standardbrowser => tr("Make default").into(),
            // GEMESSENE ENTSCHEIDUNG: der Knopf traegt seinen **Namen**, nicht
            // eine Handlung wie „Festlegen" — er macht nichts an einem Wert.
            Feld::SuchmaschinenVerwalten => tr("Manage search engines").into(),
            Feld::Suchmaschine => e.suchmaschine.label().to_string(),
            Feld::EigeneUrl => {
                if e.eigene_suche.is_empty() {
                    tr("No custom URL").into()
                } else {
                    e.eigene_suche.clone()
                }
            }
            Feld::Schriftart => {
                if e.schriftart.is_empty() {
                    //
                    // GEMESSENE KORREKTUR (04.10., Panel 4): hier stand
                    // „Eingebaute Schrift". GEMESSEN an der Karte steht im
                    // Kasten **„Standard (System)"**. Der erste Eintrag der
                    // **Liste** bleibt „Eingebaute Schrift" (Zeile 716) — die
                    // Liste nennt die Quelle, der Kasten den Vorgabewert.
                    tr("Default (system)").into()
                } else {
                    e.schriftart.clone()
                }
            }
            //
            // GEMESSENE KORREKTUR (04.10., Leitung: „Auswahlfeld
            // 'Standard (16px)'"): der Kasten zeigte „16 px", die Karte
            // schreibt „Standard (16px)". GEMESSENE ENTSCHEIDUNG, warum der
            // Kasten den **Listeneintrag** zeigt und nicht eine eigene Zahl:
            // die Liste traegt die Groesse im Text, und zwei Texte fuer
            // dieselbe Groesse sind zwei Zahlen, die auseinanderlaufen
            // koennen. (Der Zoom-Schieber zeigt weiter „100 %" — dort ist
            // es eine Zahl auf einem Balken, nicht ein Listeneintrag.)
            Feld::Schriftgroesse => schriftgroesse_text(e.schriftgroesse),
            // GEMESSENE KORREKTUR (03.10.2026, 21:0x): hier stand
            // `format!("{} %", e.zoom)`. GEMESSEN am Vergleichsbild
            // `team/log/ui_vorlage_6_leiste_vs_vorlage.png` und an der
            // Karte „Einstellungen (einfach)" in `vorlage_nutzer.webp`:
            // die Vorlage schreibt „100%" ohne Leerzeichen, Firefox auch.
            // `VORLAGE.txt` Z. 9 schreibt „Zoom" nur als Wort — die Prosa
            // der Karte zaehlt die drei Bedienelemente auf und ist kein
            // Wortlaut, der im Bild steht.
            Feld::Zoom => zoom_text(e.zoom),
            Feld::Thema => match e.thema {
                Thema::Dunkel => tr("Dark").into(),
                Thema::Hell => tr("Light").into(),
            },
            Feld::Werbeblocker => an_aus(e.werbeblocker),
            Feld::CookiesLoeschen => an_aus(e.cookies_beim_start_loeschen),
            // GEMESSENE ENTSCHEIDUNG: das ist ein KNOPF, also steht eine
            // Handlung darin. „Keine installiert" waere der Zustand der
            // Liste — und die Liste ist eine eigene Zeile darunter.
            Feld::ErweiterungenLaden => tr("Load").into(),
            // added 04.10.2026, TLS: **DER KNOPFTEXT, und er ist kuerzer als
            // die Beschriftung**, wie bei „Standardbrowser“ („Festlegen“ statt
            // „Als Standardbrowser festlegen“). **UND WARUM „Loeschen“ und nicht
            // „Leeren“:** `leeren` heisst im Deutschen eher „in eine Liste
            // schreiben“ (Formular leeren), und der Benutzer soll **wissen**,
            // dass der **Speicher** weg ist.
            Feld::KekseAlleLoeschen => tr("Clear").into(),
            Feld::KekseSeiteLoeschen => tr("Clear").into(),
            Feld::Feature(f) => an_aus(e.features.an(*f)),
            Feld::Language => e.language.native_name().into(),
        }
    }

    /// DIE ZAHL, die ein Schieber zeigt — und ihre Grenzen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das hier `Option` ist: ein Textfeld
    /// hat keine Zahl, und `0` waere eine gueltige Zahl. `None` heisst
    /// „dieses Feld hat keinen Schieber" — das ist eine andere Frage als
    /// „der Schieber steht auf 0".
    pub fn schieber(&self, e: &Einstellungen) -> Option<(u32, u32, u32)> {
        match self {
            // (ist, minimum, maximum)
            Feld::Schriftgroesse => Some((
                u32::from(e.schriftgroesse),
                u32::from(SCHRIFT_MIN),
                u32::from(SCHRIFT_MAX),
            )),
            _ => None,
        }
    }

    /// DIE EINTRAGE EINER AUSWAHL — leer bei Feldern ohne Auswahlliste.
    ///
    /// M3/M4: `MAX_SCHRIFTEN` Eintraege fuer die Schriftliste, 5 fuer die
    /// uebrigen (4 Themen-/Suchmaschinenwerte plus das erste Feld).
    pub fn auswahl(&self, e: &Einstellungen) -> Vec<String> {
        match self {
            Feld::Suchmaschine => SUCHMASCHINEN.iter().map(|m| m.label().to_string()).collect(),
            Feld::Thema => vec![tr("Light").to_string(), tr("Dark").to_string()],
            // The names stay untranslated, so every user finds their language.
            Feld::Language => Language::ALL.iter().map(|l| l.native_name().to_string()).collect(),
            //
            // GEMESSEN an der Karte (04.10.): „Standard (16px)". GEMESSENE
            // ENTSCHEIDUNG, warum die Liste die **Schrittweiten** nennt und
            // nicht nur „Standard": die Vorlage zeigt einen Wert, und ein
            // Kasten mit einem einzigen Eintrag ist eine Anzeige, keine Wahl.
            // 14 / 16 / 18 / 24 px sind die vier Stufen, die Firefox zeigt.
            // Release 09.10.2026: feste Stufen ([`SCHRIFT_STUFEN`]); ein
            // eigener Wert aus der Datei steht davor, damit er waehlbar bleibt.
            Feld::Schriftgroesse => {
                let mut liste: Vec<String> = SCHRIFT_STUFEN.iter().map(|(_, px)| schriftgroesse_text(*px)).collect();
                if !SCHRIFT_STUFEN.iter().any(|(_, px)| *px == e.schriftgroesse) {
                    liste.insert(0, schriftgroesse_text(e.schriftgroesse));
                }
                liste
            }
            // Release 10.10.2026: die Zoomstufen. Ein eigener Wert aus der
            // Datei steht im Feld („Eigener (104 %)"); die Liste markiert die
            // naechste Stufe (siehe `gewaehlt`), damit 100 % sichtbar bleibt.
            Feld::Zoom => ZOOM_STUFEN.iter().map(|z| zoom_text(*z)).collect(),
            Feld::Startseite => vec![tr("Blank page").to_string(), tr("Custom URL…").to_string()],
            Feld::Schriftart => {
                let mut liste = vec![e.schriftart_display().to_string()];
                liste.extend(e.schriften.iter().cloned());
                liste
            }
            _ => Vec::new(),
        }
    }

    /// DER INDEX DES GEWAEHLTEN EINTRAGS in [`Feld::auswahl`] (0, wenn keiner).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum 0 statt `None`: die Auswahlliste zeigt
    /// immer einen Eintrag als gewaehlt, und wenn der Zustand keinen
    /// enthaelt (etwa eine Schrift, die nicht mehr installiert ist), ist
    /// der erste Eintrag der ehrliche Fallback („Eingebaute Schrift").
    pub fn gewaehlt(&self, e: &Einstellungen) -> usize {
        match self {
            Feld::Suchmaschine => SUCHMASCHINEN
                .iter()
                .position(|m| *m == e.suchmaschine)
                .unwrap_or(0),
            Feld::Thema => match e.thema {
                Thema::Hell => 0,
                Thema::Dunkel => 1,
            },
            Feld::Schriftart => e
                .schriften
                .iter()
                .position(|s| *s == e.schriftart)
                .map(|i| i + 1)
                .unwrap_or(0),
            // Release 09.10.2026: die Stufe der aktuellen Groesse (ein
            // eigener Wert steht an Stelle 0, siehe `auswahl`).
            Feld::Schriftgroesse => SCHRIFT_STUFEN
                .iter()
                .position(|(_, px)| *px == e.schriftgroesse)
                .unwrap_or(0),
            Feld::Zoom => ZOOM_STUFEN
                .iter()
                .enumerate()
                .min_by_key(|(_, z)| z.abs_diff(e.zoom))
                .map_or(0, |(i, _)| i),
            Feld::Language => Language::ALL.iter().position(|l| *l == e.language).unwrap_or(0),
            _ => 0,
        }
    }
}

/// GEMESSENE ENTSCHEIDUNG fuer diese ZWEI Texte: „An"/„Aus" sind Woerter, die
/// man in jedem Browser liest, und „ja"/„nein" ist eine Antwort auf eine
/// Frage, die nicht gestellt wurde. Ein Schalter steht AN oder AUS.
fn an_aus(wert: bool) -> String {
    if wert { tr("On") } else { tr("Off") }.to_string()
}

/// DIE FUNF BEDIENELEMENTE.
///
/// GEMESSENE BEGRUENDUNG fuer ein `enum`: ein Element hat genau eine Art.
/// `Option<f32>` (Schieber) plus `Option<bool>` (Schalter) plus `Knopf`
/// waeren drei Felder, von denen zwei `None` sind — und die Frage „was tue
/// ich bei einem Klick" haette drei Antworten, von denen keine zustaendig ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bedienung {
    /// Ein Textfeld (Beschriftung und Wert in einem Kasten).
    Textfeld,
    /// Eine Klappliste mit N Eintraegen.
    Auswahl,
    /// Ein Balken mit einem Griff (eine Zahl zwischen min und max).
    Schieber,
    /// Ein Schalter (an/aus).
    Schalter,
    /// Ein Knopf (eine Aktion).
    Knopf,
}

// ---------------------------------------------------------------------------
// DIE GEOMETRIE
// ---------------------------------------------------------------------------

/// DER OBERKANTE DER SEITE — Tab-Leiste, Werkzeugleiste und Lesezeichenleiste.
///
/// GEMESSENE ZAHL: 26 + 32 = 58 px ohne Lesezeichenleiste, und 82 px mit ihr
/// (die Leiste ist 24 px hoch). Es ist die Summe von Konstanten, die schon
/// existieren — eine dritte Zahl waere eine Stelle, an der sie
/// auseinanderlaufen koennen.
///
/// GEMESSENER BEFUND (01.10., Bild `team/log/ui1_*_a_fenster_alles.png` und
/// der Test `das_ganze_fenster_mit_alle_seiten`): die Einstellungen begannen
/// bei 58 px und lagen damit auf der Lesezeichenleiste (58..82). Die Seite
/// hat den Text der Leiste ueberdeckt. GEMESSENE ENTSCHEIDUNG: die Zahl
/// kommt aus [`Chrome::seiten_oberkante`], und die Chrome weiss, wie viel
/// Platz die Leiste braucht (`Chrome::lesezeichen_hoehe`).
pub fn seiten_oberkante(chrome: &Chrome) -> u32 {
    chrome.seiten_oberkante()
}

/// Die Oberkante des INHALTS — die Seite plus die Ueberschrift.
///
/// GEMESSENE ENTSCHEIDUNG (Koordination 02.10., „Feinschliff aller Seiten"):
/// es gibt zwei Oberkanten. Die erste ist der Grund der Seite (diesen Namen
/// hat die Funktion behalten, damit Runde 4 nicht umbenannt werden muss), die
/// zweite der Inhalt darunter. GEMESSENE BEGRUENDUNG, warum die
/// Ueberschrift nicht in `seiten_oberkante()` steckt: die Startseite hat
/// KEINE Ueberschrift (ANFORDERUNGEN: „leere Startseite"), und sie benutzt
/// dieselbe Funktion — waere die Ueberschrift schon drin, rueckte das
/// Suchfeld der Startseite um 24 px nach unten, ohne dass jemand etwas
/// geaendert haette.
pub fn inhalt_oberkante(chrome: &Chrome) -> u32 {
    chrome.inhalt_oberkante()
}

/// DIE FRAGE, die der Knopf „Als Standardbrowser festlegen" beim ersten
/// Klick stellt.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Worte **hier** stehen und nicht in
/// `narsil/src/einstellungen_weg.rs` (wo sie am 04.10. standen): die Frage
/// muss im Bild stehen, und das Bild zeichnet `einstellungen.rs`. GEMESSEN
/// war der Grund fuer die alte Stelle nicht falsch — der Text nennt
/// `xdg-settings` beim Namen, und den Prozess startet nur `narsil` — aber
/// eine Zeichenfolge an zwei Orten ist eine, die auseinanderlaufen kann.
/// Wer die Frage stellt, schreibt sie; wer sie zeigt, holt sie.
///
/// GEMESSENE ENTSCHEIDUNG, warum kein Dialog: `narsil-chrome` kennt ausser
/// `narsil-browser` keine Fremd-Crate (Modulkopf) und hat damit keinen. Die
/// Zeile, in der Firefox seinen Hinweis schreibt, gibt es hier auch.
/// A translation key: show it with `tr(FRAGE_STANDARDBROWSER)`.
pub const FRAGE_STANDARDBROWSER: &str =
    tr_key("Make Narsil the default browser? Click again to confirm.");

/// DIE LAENGE EINER MELDUNG IN ZEICHEN.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier eine eigene Grenze steht und nicht die
/// Meldungsgrenze des Fensters (`narsil`, fremd): die Meldungszeile der Seite
/// ist eine andere Laenge als die des Fensters — das Fenster hat auch das
/// Suchfeld, die Seite nicht. 200 Zeichen sind bei 8 px je Zeichen 1600 px,
/// und das Fenster ist 1920 px breit: eine laengere Zeile laeuft aus dem Bild.
pub const MAX_HINWEIS_ZEICHEN: usize = 200;

/// DIE MELDUNGSZEILE DER EINSTELLUNGEN — der Grund und der Text.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Zeile hier gezeichnet wird und nicht im
/// Fenster (`narsil/src/sitzung.rs`): die Seite kennt ihre Meldung, das
/// Fenster kennt sie erst, wenn es sie bekommt. Eine Seite, die etwas
/// fragt, und ein Fenster, das etwas zeigt, sind zwei Orte fuer eine
/// Frage — und die Frage ist die eine Sache, die man sehen muss.
pub fn hinweis_zeile(
    text: &str,
    chrome: &Chrome,
    _fensterbreite: u32,
    fensterhoehe: u32,
) -> DisplayList {
    let mut liste = DisplayList::new();
    if text.is_empty() || fensterhoehe < super::STATUSLEISTE_HOEHE {
        return liste;
    }
    let r = chrome.statusleiste(fensterhoehe);
    // GEMESSENE ENTSCHEIDUNG, warum der Grund hier (0x40, 0x18, 0x18) steht:
    // das ist die Farbe, die `narsil/src/sitzung.rs` fuer eine Meldung legt
    // (dieselbe Datei, dieselbe Farbe). Zwei Farben fuer „das ist eine
    // Meldung" waeren zwei Bedeutungen an zwei Orten.
    liste.push(DisplayItem::Rect {
        rect: Rect::new(r.x as f32, r.y as f32, r.breite as f32, r.hoehe as f32),
        color: narsil_core::color::Color::new(0x40, 0x18, 0x18, 255),
        radius: [0.0; 4],
        deckkraft: 235.0 / 255.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });
    liste.push(DisplayItem::Text {
        // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
        // traegt jetzt die `font-family`-Liste. Hier steht die
        // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
        // Bildschirmpunkte schreibt keine eigene Familie.
        schriftfamilie: String::from("sans-serif"),
        deko: 0,
        // GEMESSENER FEHLER (04.10.2026), korrigiert: hier stand
        // `r.y + 16` — GEMESSEN am Bild `abnahme_5_6a_…png`: der Text stand
        // 6 px zu tief und war am Fensterrand **abgeschnitten** (nur die
        // oberste Haelfte war zu sehen). GEMESSENE REGEL, warum jetzt 5:
        // das ist dieselbe Mitte wie bei den Feldzeilen (`fr.y + 5` bei
        // [`ZEILE`] = 30 und `font_size` = 14) — in einer 24 px hohen Zeile
        // sind (24 - 14) / 2 = **5** px Luft. Eine eigene Zahl fuer diese
        // Zeile waere eine Rechnung, die man gegen die andere messen muss.
        origin: Point::new((r.x + 8) as f32, (r.y + 5) as f32),
        text: text.chars().take(MAX_HINWEIS_ZEICHEN).collect(),
        color: narsil_core::color::Color::rgb(0xff, 0xff, 0xff),
        font_size: 14.0,
        gewicht: 400,
        kursiv: false,
    });
    liste
}

/// DIE SPALTE DER KATEGORIEN.
// GEMESSENE ENTSCHEIDUNG (Punkt 35, 02.10.), warum der Parameter
// bleibt und nicht wegfällt: die Spalte wird in einer Schleife ueber die
// KATEGORIEN aufgerufen (Zeile 1423), und eine Schleife, die einer Funktion
// etwas uebergibt, was sie nicht braucht, ist die haeufigste Form eines
// Parameters, den man nicht verstehen muss. GEMESSENE ENTSCHEIDUNG, warum
// das Rechteck gleich fuer jede Kategorie ist: die KATEGORIEN sind gleich
// hoch (alle eine Zeile), und die Zeilen stehen untereinander — eine Spalte
// ist ein Rechteck, keine Liste.
//
// GEMESSENER BEFUND (Punkt 35): hier stand `let _ = kategorie;`. Das ist
// keine Begruendung, das ist eine Anweisung an den Compiler, die Warnung zu
// unterdruecken — und sie sah aus wie Code. GEMESSENE REGEL: eine
// Unterdrueckung gehoert in den Namen (`_kategorie`), nicht in eine Zeile
// im Rumpf, die man beim Lesen fuer eine Berechnung haelt.
pub fn spalte(chrome: &Chrome, _kategorie: Kategorie, fensterbreite: u32) -> Rechteck {
    Rechteck {
        x: 0,
        y: inhalt_oberkante(chrome),
        // GEMESSENE ENTSCHEIDUNG: die SPALTE ist die Vorlage-Spalte
        // ([`spaltenbreite`], 38,7 % der Fensterbreite), nicht die alte 200.
        breite: spaltenbreite(fensterbreite),
        //
        // GEMESSENER FEHLER MEINES UMBAUS (02.10. 18:6x, Bild
        // `ui_vorlage_5_einstellungen.png`): hier stand `MAX_KATEGORIEN *
        // ZEILE` = 5 x 24 = 120 px. Nach dem Umbau sind die Zeilen aber
        // [`ZEILE_KATEGORIE`] = 38 px auseinander, also 228 px — und die
        // Flaeche endete nach der VIERZEILENZEILE: die letzte Kategorie stand
        // auf dem dunklen Seitengrund statt auf der Spalte. GEMESSEN am Bild:
        // vier Zeilen mit hellem Grund, die fuenfte ohne.
        hoehe: MAX_KATEGORIEN as u32 * ZEILE_KATEGORIE,
    }
}

/// DIE KATEGORIE, deren Name in der Spalte steht.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Zeilenhoehe hier [`ZEILE`] ist und
/// nicht 24 an anderer Stelle: es ist dieselbe Zahl, und die KATEGORIE ist
/// der uebergeordnete Begriff — wer sie gleich nennt, verwechselt sie an
/// einer Stelle und nicht an der anderen.
pub fn kategorie_feld(
    chrome: &Chrome,
    _k: Kategorie,
    index: usize,
    fensterbreite: u32,
) -> Rechteck {
    Rechteck {
        // GEMESSEN (04.10., Karte): die Kachel beginnt **11 px** rechts vom
        // Spaltenrand. `MARGIN` (24 px) war aus dem Fenster geholt; mit der
        // jetzt schmaleren Spalte (319 ‰ statt 387) erreicht die Kachel mit
        // 24 px Rand den Inhalt — GEMESSEN vom Test
        // `die_kachel_der_aktiven_kategorie_hat_die_breite_der_vorlage`:
        // 346 px Kachel-Rand gegen 343 px Inhalt.
        x: LUFT_KATEGORIE_SPALTE,
        // GEMESSENE ENTSCHEIDUNG (Koordination 02.10. 16:0x): die Zeile der
        // KATEGORIE ist nicht [`ZEILE`] (24 px) hoch, weil die Vorlage eine
        // Figur neben dem Text hat und 24 px so hoch waere wie die Figur
        // (20 px) plus nichts. GEMESSENE KORREKTUR (03.10.2026): der **Takt**
        // ist [`ZEILE_KATEGORIE`] (38 px, gemessen an den Obenkanten der
        // Kategorietexte in der Vorlage: 578, 616, 654, 692, 734), die
        // **Kachel** ist [`HOEHE_KATEGORIE`] (31 px, gemessen an der
        // blauen Flaeche y = 571..601) — dazwischen [`LUFT_KATEGORIE`].
        y: inhalt_oberkante(chrome) + ZEILE_KATEGORIE * index as u32,
        breite: spaltenbreite(fensterbreite)
            .saturating_sub(2 * LUFT_KATEGORIE_SPALTE)
            .min(fensterbreite),
        // GEMESSENE KORREKTUR (03.10.2026): hier stand `ZEILE_KATEGORIE`
        // (damals 34) — Takt UND Hoehe waren dieselbe Zahl, also stiessen
        // die Kacheln aneinander. GEMESSEN in der Vorlage: Kachel 31 px
        // hoch bei 38 px Takt. Jetzt ist die Hoehe ihre eigene Zahl.
        hoehe: HOEHE_KATEGORIE,
    }
}

/// WELCHE FIGUR steht links an einer Kategorie?
///
/// GEMESSENE ENTSCHEIDUNG, warum die Zuordnung HIER steht und nicht beim
/// Zeichnen: sie ist eine Angabe ueber die Kategorie, und `Kategorie` ist
/// die Stelle, an der die Angaben ueber eine Kategorie stehen (`felder`,
/// `text`). Wer sie beim Zeichnen schreibt, hat sie an zwei Orten.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge der Vorlage folgt:
/// Allgemein = Zahnrad (alles), Suche = Lupe, Add-ons = Puzzle,
/// Downloads = Download, Darstellung = Bildschirm. GEMESSEN im Bild
/// `vorlage_nutzer.webp` in dieser Reihenfolge von oben nach unten.
pub fn kategorie_symbol(k: Kategorie) -> Option<super::symbole::Symbol> {
    use super::symbole::Symbol;
    Some(match k {
        Kategorie::Allgemein => Symbol::Zahnrad,
        Kategorie::Suche => Symbol::Lupe,
        Kategorie::Addons => Symbol::Puzzle,
        //
        // GEMESSENE ENTSCHEIDUNG (03.10.2026): „Downloads" bekommt den
        // **Download-Pfeil** — genau den, den die Vorlage in ihrer Spalte
        // neben „Downloads" zeigt (Karte „Einstellungen (einfach)",
        // Zeile 4, Symbol x = 681 wie bei der aktiven Zeile „Allgemein").
        // Damit ist die Zuordnung je Kategorie genau die, die man ablesen
        // kann, und „Datenschutz" verliert den Pfeil wieder (siehe unten).
        Kategorie::Downloads => Symbol::Download,
        //
        // GEMESSENE ENTSCHEIDUNG (02.10. 18:6x), warum Datenschutz ein
        // WERBEBLOCKER-Schild ist und **kein** Download-Pfeil: ich hatte
        // dort erst `Download` gesetzt — im Bild stand dann ein
        // Download-Pfeil neben „Datenschutz", und das ist eine falsche
        // Aussage ueber die Kategorie. GEMESSENE ENTSCHEIDUNG, warum das
        // Bild von 03.10. die Frage jetzt beantwortet: die Vorlage hat
        // eine Kategorie „Downloads" **mit** dem Pfeil, und seit dem
        // Zufuegen dieser Kategorie ist „Datenschutz" die einzige
        // Kategorie ohne Download-Bedeutung — die Verwechslung ist damit
        // nicht mehr moeglich. Die Zuordnung folgt also der KATEGORIE.
        // Release 10.10.2026: Umriss-Symbole wie die uebrigen Kategorien —
        // das Schild mit Haken (vorher das GEFUELLTE Werbeblocker-Schild,
        // eine Flaeche zwischen lauter Strichen) und die Palette (vorher der
        // Einpass-Rahmen von `bildschirm.svg`, der nach „Vollbild" aussah).
        Kategorie::Datenschutz => Symbol::SchildAn,
        Kategorie::Darstellung => Symbol::Palette,
        Kategorie::Features => Symbol::Funke,
    })
}

/// DIE KATEGORIE, die angezeigt wird.
///
/// Release 09.10.2026: nicht mehr `Copy` — der Tipp-Puffer ist ein `String`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seite {
    /// Die gewaehlte Kategorie (links in der Spalte markiert).
    pub kategorie: Kategorie,
    /// Die aufgeklappte Auswahlliste — `(Feld, Nummer des Eintrags)`.
    ///
    /// GEMESSENE ENTSCHEIDUNG fuer ein `Option<(Feld, usize)>` und nicht
    /// fuer zwei Felder (welches Feld, welcher Eintrag): die Klappliste IST
    /// ein Feld mit einem Eintrag. Zwei Felder koennten „Feld = Schriftart,
    /// Eintrag = 3 (von Suche)" bedeuten — ein Zustand, den es nicht gibt.
    pub offen: Option<(Feld, usize)>,
    /// Das Feld, in dem getippt wird (`None` = kein Textfeld aktiv).
    pub tippfeld: Option<Feld>,
    /// HAT DER NUTZER DEN KNOPF „Als Standardbrowser festlegen" BEREITS
    /// GESEHEN? (`true` = der zweite Klick fuehrt aus).
    ///
    /// GEMESSENE ENTSCHEIDUNG (04.10.2026, Leitung: „Standardbrowser setzt
    /// xdg-Eintrag **nur nach Rueckfrage**"): der erste Klick stellt die
    /// Frage in die Meldungszeile, der zweite fuehrt `xdg-settings` aus.
    /// GEMESSENE URSACHE fuer die Form: `narsil-chrome` kennt ausser
    /// `narsil-browser` keine Fremd-Crate (Modulkopf dieses Files) und hat
    /// damit **keinen** Dialog. Die Meldungszeile gibt es schon
    /// (`Wirkung::meldung` -> `Sitzung::meldung`).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das in der **Seite** steht und nicht in
    /// den Einstellungen: es ist ein **Zustand der Seite** (die Frage ist
    /// offen), kein gespeicherter Wert. In den Einstellungen waere es ein
    /// Wert, der den Neustart ueberlebt — und eine Frage, die nach dem
    /// Neustart wieder offen ist, ist keine Frage mehr.
    pub standardbrowser_gefragt: bool,
    /// Release 08.10.2026: die Funktion, deren (i)-Erklaerung gerade
    /// eingeblendet ist (Tab „Features").
    pub info: Option<super::features::Feature>,
    /// Release 09.10.2026: wie weit die Inhaltsspalte nach oben gerollt ist
    /// (Pixel; die Kategoriespalte rollt nicht). Siehe [`rollen`].
    pub rollen: u32,
    /// Release 09.10.2026: der getippte Text des Feldes [`Seite::tippfeld`]
    /// (wird mit der Eingabetaste uebernommen, mit Esc verworfen).
    pub eingabe: String,
}

impl Default for Seite {
    /// GEMESSENE ENTSCHEIDUNG fuer die erste Kategorie: `Allgemein` ist die
    /// erste in der Liste und die mit dem Standardbrowser — dem ersten
    /// Schritt, den ein Nutzer macht.
    fn default() -> Self {
        Seite {
            kategorie: Kategorie::Allgemein,
            offen: None,
            tippfeld: None,
            standardbrowser_gefragt: false,
            info: None,
            rollen: 0,
            eingabe: String::new(),
        }
    }
}

impl Seite {
    /// DIE KATEGORIE WECHSELN — und die Klappliste schliessen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Liste dabei ZUGEHT: eine
    /// aufgeklappte Schriftliste, deren Beschriftung nicht mehr da ist,
    /// schwebt im Nichts. Wer die Kategorie wechselt, will die Liste der
    /// neuen Kategorie sehen.
    pub fn wechsle(&mut self, k: Kategorie) {
        self.kategorie = k;
        self.offen = None;
        self.tippfeld = None;
        // GEMESSENE ENTSCHEIDUNG (04.10.2026): die Rueckfrage zum
        // Standardbrowser wird beim Kategoriewechsel zurueckgesetzt. Wer
        // nach dem Weggehen zurueckkommt, bekommt die Frage neu gestellt —
        // sonst waere eine Frage, die niemand gelesen hat, durch blosses
        // Wegklicken beantwortet.
        self.standardbrowser_gefragt = false;
        self.info = None;
        // Release 09.10.2026: eine neue Kategorie beginnt oben.
        self.rollen = 0;
        self.eingabe.clear();
    }

    /// DIE AUSWAHLLISTE AUF- ODER ZU KLAPPEN.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein Klick auf das Feld oeffnet und ein
    /// Klick auf einen EINTRAG waehlt (und die Liste schliesst): eine
    /// Liste, die sich beim Klick auf ihren Eintrag wieder schliesst, waere
    /// ein Kasten, aus dem man nur durch einen Klick daneben herauskommt.
    pub fn klappe_wechseln(&mut self, feld: Feld, eintrag: usize) {
        if self.offen == Some((feld, eintrag)) {
            self.offen = None;
        } else {
            self.offen = Some((feld, eintrag));
        }
    }

    /// DIE LISTE SCHLIESSEN.
    pub fn klappe_zu(&mut self) {
        self.offen = None;
    }

    /// IST DIE LISTE FUER DIESES FELD OFFEN?
    pub fn ist_offen(&self, feld: Feld) -> bool {
        matches!(self.offen, Some((f, _)) if f == feld)
    }

    /// DIE AUSWAHLLISTE FUER EIN FELD — die Eintraege, die im Bild stehen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier gekuerzt wird und nicht im Bild:
    /// [`treffer`] und [`display_list`] brauchen dieselbe Liste. Wer sie an
    /// zwei Stellen bildet, zeigt an einer Stelle 8 Zeilen und trifft an der
    /// anderen den 9.
    pub fn offene_eintraege(&self, e: &Einstellungen) -> Vec<String> {
        let Some((feld, _)) = self.offen else {
            return Vec::new();
        };
        let start = self.klapp_beginn(e);
        feld.auswahl(e).into_iter().skip(start).take(MAX_KLAPPLISTE).collect()
    }

    /// Release 10.10.2026: der erste SICHTBARE Eintrag der offenen Liste —
    /// das Fenster aus [`MAX_KLAPPLISTE`] Zeilen steht um die aktuelle Wahl.
    /// GEMESSEN vorher: nur die ersten 8 Eintraege; beim Zoom (17 Stufen)
    /// lagen 100 % und alles darueber ausserhalb der Liste.
    pub fn klapp_beginn(&self, e: &Einstellungen) -> usize {
        let Some((feld, _)) = self.offen else {
            return 0;
        };
        let n = feld.auswahl(e).len();
        if n <= MAX_KLAPPLISTE {
            return 0;
        }
        feld.gewaehlt(e)
            .saturating_sub(MAX_KLAPPLISTE / 2)
            .min(n - MAX_KLAPPLISTE)
    }
}

/// DAS RECHTECK EINES FELDES (Beschriftung + Bedienelement).
///
/// GEMESSENE ENTSCHEIDUNG, warum die Felder RECHTS neben der Kategoriespalte
/// stehen und nicht darunter: die Spalte ist 200 px, und darunter waere die
/// Seite zweispaltig mit einer sehr schmalen und einer sehr breiten Spalte.
/// Nebeneinander stehen beide auf ihrer natuerlichen Breite.
pub fn feld(k: Kategorie, chrome: &Chrome, _f: Feld, index: usize, fensterbreite: u32) -> Rechteck {
    Rechteck {
        // GEMESSEN (04.10., Karte): der Inhalt beginnt **30 px** rechts von der
        // Kachel — jetzt [`inhalt_x`] (Kachel + [`LUFT_SPALTE`]).
        x: inhalt_x(fensterbreite),
        y: feld_y(k, chrome, index),
        breite: inhalt_breite(fensterbreite),
        hoehe: HOEHE_BEDIENTEIL,
    }
}

/// DER LINKE RAND DES INHALTS — Seitenleiste plus [`LUFT_SPALTE`].
///
/// LEITUNG (04.10.): „linksbuendig am Seitenleistenrand + 24 px". GEMESSEN an
/// der Karte: Kachel-Rechtskante 158, Inhalt 188 → **30 px** Luft. Gewaehlt
/// ist die Vorgabe (24 px, `LUFT_SPALTE`); die gemessenen 30 px sind 6 px
/// mehr, also 1,2 % der Kartenbreite — und [`LUFT_SPALTE`] ist an vier
/// Stellen im Gebrauch, also aendert man sie an **einer**.
pub fn inhalt_x(fensterbreite: u32) -> u32 {
    spaltenbreite(fensterbreite) + LUFT_SPALTE
}

/// DIE BREITE DER INHALTSSPALTE — hoechstens [`MAX_SPALTE`].
///
/// LEITUNG (04.10.): „max 640 px". GEMESSEN an der Karte: die Spalte ist
/// 291 px breit (x 188..479) — 640 px passt bei 498 px Fensterbreite nicht,
/// also gewinnt der Rest, den das Fenster hergibt.
pub fn inhalt_breite(fensterbreite: u32) -> u32 {
    let x = inhalt_x(fensterbreite);
    MAX_SPALTE
        .min(fensterbreite.saturating_sub(x + MARGIN))
        .max(40)
}

/// DIE BREITE DER BESCHRIFTUNGSSPALTE — bis zum Kasten.
///
/// GEMESSEN an der Karte: „Schriftgroesse" steht links, ihr Kasten beginnt
/// **86 px** rechts davon, bei 291 px Inhaltsspalte = 295 ‰ ([`SPALTE_
/// WIDGET_ANTEIL`]). GEMESSENE UNTERGRENZE: 160 px, damit eine
/// Beschriftung von 31 Zeichen (248 px) nicht im Kasten landet — das war der
/// Grund fuer [`BESCHRIFTUNG_BREITE`] und der bleibt.
pub fn beschriftung_breite(fensterbreite: u32) -> u32 {
    (inhalt_breite(fensterbreite) as u64 * SPALTE_WIDGET_ANTEIL as u64 / 1000) as u32
}

/// DIE BREITE EINES KASTENS.
///
/// GEMESSEN an der Karte: ein Kasten allein im Abschnitt ist 218 px
/// (**750 ‰**), zwei nebeneinander stehen sich bei **450 ‰** und **530 ‰** mit
/// 8 px Luft dazwischen. GEMESSENE ENTSCHEIDUNG, warum die Karte zwei
/// various Breiten zeigt (218 und 171 px): die Breite folgt dem Inhalt
/// (Firefox), und ein Kasten, der immer die volle Breite nimmt, sieht aus wie
/// ein Formular. Darum: allein = 750 ‰, zu zweit = 450 ‰.
pub fn widget_breite(fensterbreite: u32, index_im_abschnitt: usize, felder: usize) -> u32 {
    let b = inhalt_breite(fensterbreite);
    if felder <= 1 {
        return (b as u64 * WIDGET_ANTEIL as u64 / 1000) as u32;
    }
    //
    // GEMESSENER EIGENER FEHLER (04.10., Vergleichsbild „Allgemein"): hier
    // stand `rest.saturating_sub(WIDGET_ANTEIL_ZWEI) / (n - 1)` — der Rest
    // ist eine **Pixelzahl**, `WIDGET_ANTEIL_ZWEI` ein **Anteil**. GEMESSEN
    // ist die Folge bei 300 px Inhaltsspalte: `300 * 157 / 1000 = 47 px`
    // statt 157 px, und der Knopf „Suchmaschinen verwalten" lag quer ueber
    // dem Kasten und ragte rechts aus dem Fenster. Jetzt wird ausschliesslich
    // in Pixeln gerechnet.
    //
    // GEMESSENE KORREKTUR (04.10., Leitung „Breite 156 px wie gemessen"):
    // der zweite Kasten (der Knopf „Suchmaschinen verwalten") ist an der
    // Karte **156 px** breit = **536 ‰** der Inhaltsspalte (291 px). Der
    // erste ist 130 px = 450 ‰, die Luft dazwischen 8 px. 450 + 536 + 8 =
    // 994 ‰ — das passt in die Inhaltsspalte, und 136 px (der „Rest") waere
    // **20 px** breiter als gemessen und haette den Knopf ueber den
    // Fensterrand hinausgeschoben (das war der Befund „abgeschnitten").
    let luft = LUFT_WIDGET * (felder as u32 - 1);
    let erstes = (b as u64 * WIDGET_ANTEIL_ZWEI as u64 / 1000) as u32;
    if index_im_abschnitt == 0 {
        return erstes;
    }
    if felder == 2 {
        return (b as u64 * WIDGET_ANTEIL_KNOPF as u64 / 1000) as u32;
    }
    let rest = b.saturating_sub(erstes + luft) / (felder as u32 - 1);
    rest
}

/// Release 08.10.2026: der Zeilentakt einer Kategorie — die Karten des Tabs
/// „Features" brauchen 40 px statt der 34 px der Kaesten.
pub fn takt(k: Kategorie) -> u32 {
    if k == Kategorie::Features {
        super::features::ZEILE_FEATURE
    } else {
        ZEILE
    }
}

/// Release 08.10.2026: Platz fuer die Hinweiszeile unter der Ueberschrift
/// des Tabs „Features".
pub fn kopf_extra(k: Kategorie) -> u32 {
    if k == Kategorie::Features { 22 } else { 0 }
}

/// DIE OBERKANTE DES ABSCHNITTS mit der Nummer `i`.
pub fn abschnitt_y(k: Kategorie, chrome: &Chrome, i: usize) -> u32 {
    let mut y = inhalt_oberkante(chrome) + ZEILE_UEBERSCHRIFT + kopf_extra(k);
    for (n, a) in abschnitte(k).iter().enumerate() {
        if n == i {
            return y;
        }
        y += ZEILE_ABSCHNITT
            + takt(k)
                * if abschnitt_nebeneinander(k, a.felder[0]) {
                    // GEMESSENE KORREKTUR (04.10., Karte): die Felder eines
                    // Abschnittes stehen nebeneinander (mit Knopf) auf EINER
                    // Zeile — vorher wurde fuer jedes Feld eine Zeile gerechnet,
                    // und der Abschnitt „Darstellung" stand dadurch 34 px zu
                    // tief.
                    1
                } else {
                    a.felder.len() as u32
                }
            + if n + 1 == abschnitte(k).len() {
                0
            } else {
                LUFT_ABSCHNITT
            };
    }
    y
}

/// Release 09.10.2026: die Unterkante des Inhalts einer Kategorie (ohne
/// Rollversatz) — die letzte Zeile plus Luft.
pub fn inhalt_unten(k: Kategorie, chrome: &Chrome) -> u32 {
    abschnitt_y(k, chrome, abschnitte(k).len()) + LUFT_ABSCHNITT
}

/// Release 09.10.2026: so weit laesst sich die Inhaltsspalte hoechstens
/// rollen — bis die letzte Zeile ueber der Statuszeile steht.
pub fn max_rollen(k: Kategorie, chrome: &Chrome, fensterhoehe: u32) -> u32 {
    let sicht_unten = fensterhoehe.saturating_sub(super::STATUSLEISTE_HOEHE);
    inhalt_unten(k, chrome).saturating_sub(sicht_unten)
}

/// Release 09.10.2026: DAS RAD AUF DER INHALTSSPALTE. `pixel` > 0 rollt nach
/// unten. Begrenzt auf `0..=max_rollen`; eine offene Klappliste geht zu (sie
/// gehoert zu einer Zeile, die gerade wandert). `true`, wenn sich etwas
/// bewegt hat.
pub fn rollen(seite: &mut Seite, chrome: &Chrome, fensterhoehe: u32, pixel: f32) -> bool {
    if !pixel.is_finite() || pixel == 0.0 {
        return false;
    }
    let max = max_rollen(seite.kategorie, chrome, fensterhoehe) as f32;
    let neu = (seite.rollen.min(max as u32) as f32 + pixel).clamp(0.0, max).round() as u32;
    if neu == seite.rollen {
        return false;
    }
    seite.rollen = neu;
    seite.offen = None;
    true
}

/// DIE OBERKANTE der BESCHRIFTUNG des Feldes `index` (der Index in
/// [`Kategorie::felder`]).
///
/// GEMESSENE ENTSCHEIDUNG, warum die Kategorie ein Parameter ist: die Zeile
/// eines Feldes haengt davon ab, **wie viele Abschnittsueberschriften** ueber
/// ihr stehen — und das weiss nur die Kategorie. Ohne sie waere die Seite
/// eine flache Liste, und genau das war der Befund „Allgemein ist fast leer".
pub fn feld_y(k: Kategorie, chrome: &Chrome, index: usize) -> u32 {
    let mut gesehen = 0usize;
    for (i, a) in abschnitte(k).iter().enumerate() {
        // GEMESSENE ENTSCHEIDUNG (04.10., Karte): stehen die Felder eines
        // Abschnitts nebeneinander, teilen sie sich **eine** Zeile; stehen
        // sie untereinander, hat jedes seine eigene (das ist der Fall
        // „Darstellung" mit „Schriftgroesse" und „Schriftart" — y 263 und
        // 297, also 34 px auseinander).
        let nebeneinander = abschnitt_nebeneinander(k, a.felder[0]);
        for (j, _) in a.felder.iter().enumerate() {
            if gesehen == index {
                let versatz = if nebeneinander { 0 } else { takt(k) * j as u32 };
                return abschnitt_y(k, chrome, i) + ZEILE_ABSCHNITT + versatz;
            }
            gesehen += 1;
        }
    }
    abschnitt_y(k, chrome, abschnitte(k).len().saturating_sub(1)) + ZEILE_ABSCHNITT
}

/// DIE OBERKANTE DES BEDIENELEMENTS `index` — neben der Beschriftung, in der
/// zweiten Spalte ([`beschriftung_breite`]).
pub fn element_y(k: Kategorie, chrome: &Chrome, index: usize) -> u32 {
    feld_y(k, chrome, index)
}

/// DAS RECHTECK DES BEDIENELEMENTS (die rechte Haelfte des Feldes).
///
/// GEMESSENE ENTSCHEIDUNG fuer [`BESCHRIFTUNG_BREITE`] statt der Haelfte:
/// die Beschriftung ist bis 31 Zeichen lang (248 px), und `560 / 2` waere
/// 280 px — 16 px mehr als noetig. GEMESSEN am Bild: mit 210 px war die
/// Adresse bei 21 Zeichen abgeschnitten.
pub fn element(
    k: Kategorie,
    chrome: &Chrome,
    f: Feld,
    index: usize,
    fensterbreite: u32,
) -> Rechteck {
    let r = feld(k, chrome, f, index, fensterbreite);
    // Release 08.10.2026: im Tab „Features" ist das Bedienelement die
    // Schalter-Pille am rechten Rand der Karte.
    if k == Kategorie::Features {
        return super::features::schalter_rect(&r);
    }
    let b = inhalt_breite(fensterbreite);
    let n = abschnitt_felder_von(k, f);
    let stelle = abschnitt_feld_index(k, f);
    //
    // DIE DREI FORMEN, gemessen an der Karte (siehe Modulkopf):
    //
    //   1) ein Kasten **ohne** Etikett: er beginnt bei x = 0 der
    //      Inhaltsspalte und ist [`WIDGET_ANTEIL`] (750 ‰) breit —
    //      218 px von 291 px an der Karte;
    //   2) ein Kasten **mit** Etikett: er beginnt um
    //      [`beschriftung_breite`] (295 ‰, 86 px) und ist
    //      [`WIDGET_ANTEIL_ETIKETT`] (587 ‰, 171 px) breit;
    //   3) **zwei** Kaesten ohne Etikett: beide ab x = 0, der erste
    //      [`WIDGET_ANTEIL_ZWEI`] (450 ‰), der zweite der Rest, mit
    //      [`LUFT_WIDGET`] (8 px) dazwischen.
    //
    // GEMESSENER FEHLER (04.10., Vergleichsbild): vorher stand fuer **alle**
    // drei Formen `r.x + beschriftung_breite` — also auch fuer das zweite
    // Feld, das gar kein Etikett hat. GEMESSENE FOLGE im Bild: „Suchmaschin"
    // (Etikett des zweiten) lag auf „DuckDuckGo" (Wert des ersten), und der
    // zweite Kasten began um **dieselbe** Breite verschoben, ragte also aus
    // dem Fenster.
    //
    // GEMESSENER EIGENER FEHLER (04.10., gefunden von
    // `der_treffer_kennt_sich_selbst`: „Schriftgroesse bei 1000 px: die
    // Beschriftung ist ein Treffer — Feld x 335, Element x 335"): hier stand
    // `if n > 1`, also „zwei Felder im Abschnitt". GEMESSEN ist aber: die
    // beiden Felder des Abschnitts „Darstellung" stehen **untereinander**
    // (siehe [`abschnitt_nebeneinander`]) — der Zweig fuer „nebeneinander"
    // wurde trotzdem genommen und rueckte den Kasten auf die Beschriftung.
    let (x, breite) = if abschnitt_nebeneinander(k, f) {
        let mut versatz = 0u32;
        for i in 0..stelle {
            versatz += widget_breite(fensterbreite, i, n) + LUFT_WIDGET;
        }
        (r.x + versatz, widget_breite(fensterbreite, stelle, n))
    } else if hat_etikett(k, f) {
        (
            r.x + beschriftung_breite(fensterbreite),
            (b as u64 * WIDGET_ANTEIL_ETIKETT as u64 / 1000) as u32,
        )
    } else {
        (r.x, widget_breite(fensterbreite, 0, 1))
    };
    Rechteck {
        x,
        y: r.y,
        breite,
        hoehe: HOEHE_BEDIENTEIL,
    }
}

/// TRAEGT DIESES FELD EIN **ETIKETT** links von seinem Kasten?
///
/// GEMESSEN an der Karte: eine Zeile traegt ein Etikett, wenn sie **ein
/// einziges** Feld hat **und** dieses Feld seinen Abschnitt nicht benennt.
/// GEMESSENE FOLGE jeder anderen Regel: die Abschnitte „Startseite" und
/// „Suche" zeigten zweimal ihren Namen (Abschnitt + Etikett), und die Zeile
/// „Suche" zeigte **drei** Texte an derselben Stelle (Etikett des zweiten
/// Feldes, Wert des ersten Kastens, Beschriftung des Knopfes).
///
/// GEMESSENE ENTSCHEIDUNG, warum „genau ein Feld" und nicht „das erste":
/// ein zweites Feld im Abschnitt ist bei der Karte immer ein **Knopf neben
/// dem Kasten** („Suchmaschinen verwalten"), und der traegt seinen Text im
/// Kasten — ein Etikett daneben waere eine zweite Beschriftung fuer
/// denselben Kasten.
pub fn hat_etikett(k: Kategorie, f: Feld) -> bool {
    // Release 08.10.2026: die Karten des Tabs „Features" tragen ihren Namen
    // selbst (`features::zeile_zeichnen`).
    k != Kategorie::Features
        && !abschnitt_nebeneinander(k, f)
        && f.key() != abschnitt_ueberschrift(k, f)
}

/// STEHEN DIE FELDER DIESES ABSCHNITTS **NEBENEINANDER**?
///
/// GEMESSEN an der Karte (04.10.): **nur** wenn ein Knopf daneben steht.
/// Der Abschnitt „Suche" hat einen Kasten und einen Knopf — die stehen
/// nebeneinander, beide ohne Beschriftung. Der Abschnitt „Darstellung" hat
/// zwei Kaesten — die stehen **untereinander**, beide mit Beschriftung.
///
/// GEMESSENE URSACHE: ein Knopf traegt seinen Text **im Kasten**. Ein zweiter
/// Kasten traegt seinen Text auch im Kasten — zwei Kaesten nebeneinander
/// haben also zwei Texte, von denen der erste im Bild des zweiten landet
/// (GEMESSEN: „Schriftgroesse" lag auf dem Kasten „Schriftart").
pub fn abschnitt_nebeneinander(k: Kategorie, f: Feld) -> bool {
    abschnitte(k)
        .iter()
        .find(|a| a.felder.contains(&f))
        .map(|a| a.felder.len() > 1 && a.felder.iter().any(|x| x.bedienung() == Bedienung::Knopf))
        .unwrap_or(false)
}

/// DER NAME des Abschnitts, in dem dieses Feld steht.
pub fn abschnitt_ueberschrift(k: Kategorie, f: Feld) -> &'static str {
    abschnitte(k)
        .iter()
        .find(|a| a.felder.contains(&f))
        .map(|a| a.ueberschrift)
        .unwrap_or("")
}

/// IN WELCHEM ABSCHNITT steht dieses Feld? (die Nummer, 0 beginnt).
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine Funktion ist und kein `find_map`
/// an drei Stellen: „gehoeren diese beiden Felder zum selben Abschnitt" ist
/// eine Frage, die der Zeichenlauf, der Treffer und der Test stellen — und
/// drei `find_map` sind drei Orte, an denen sie falsch stehen kann.
pub fn abschnitt_von(k: Kategorie, f: Feld) -> usize {
    abschnitte(k)
        .iter()
        .position(|a| a.felder.contains(&f))
        .unwrap_or(0)
}

/// DER VORGABEWERT eines Schalters — **die Frage**, keine Vermutung.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine Funktion ist und kein `match` im
/// Test: der Test will fuer **jeden** Schalter das Gegenteil der Vorgabe
/// klicken, und „das Gegenteil der Vorgabe" ist eine Eigenschaft des Feldes.
/// Wer sie im Test per `match` holt, hat sie an zwei Orten.
fn vorgabe(f: Feld, e: &Einstellungen) -> bool {
    schalter_an(f, e)
}

/// WIE VIE Felder im Abschnitt dieses Feldes stehen.
fn abschnitt_felder_von(k: Kategorie, f: Feld) -> usize {
    abschnitte(k)
        .iter()
        .find(|a| a.felder.contains(&f))
        .map(|a| a.felder.len())
        .unwrap_or(1)
}

/// DIE STELLE des Feldes im Abschnitt (0, 1, …) — fuer die Breite.
fn abschnitt_feld_index(k: Kategorie, f: Feld) -> usize {
    abschnitte(k)
        .iter()
        .find_map(|a| a.felder.iter().position(|x| *x == f))
        .unwrap_or(0)
}

/// DAS RECHTECK DER AUFRUPPENDEN AUSWAHLLISTE.
///
/// GEMESSENE ENTSCHEIDUNG fuer die Lage: die Liste geht von der UNTERKANTE
/// des Feldes nach unten. GEMESSENE BEGRUENDUNG: nach oben waere sie im Weg,
/// wenn der Nutzer das Feld unter den Fingern hat — und das Feld ist das,
/// worauf er klickt.
pub fn klappliste(
    k: Kategorie,
    chrome: &Chrome,
    f: Feld,
    index: usize,
    fensterbreite: u32,
    fensterhoehe: u32,
    zeilen: usize,
) -> Option<Rechteck> {
    let e = element(k, chrome, f, index, fensterbreite);
    let n = zeilen.min(MAX_KLAPPLISTE) as u32;
    if n == 0 {
        return None;
    }
    let hoehe = n * ZEILE;
    // M3: die Liste darf nicht ueber das Fenster hinaus wachsen.
    let platz = fensterhoehe.saturating_sub(e.unten());
    if platz < ZEILE {
        return None;
    }
    Some(Rechteck {
        x: e.x,
        y: e.unten(),
        breite: e.breite,
        hoehe: hoehe.min(platz),
    })
}

/// M3: die Breite des Griffs an einem Schieber.
///
/// GEMESSENE BEGRUENDUNG fuer 12: das ist dieselbe Zahl wie die
/// Schliessen-Markierung an einem Tab (12 x 12) — ein Griff, den man mit
/// der Maus treffen muss, und ein „x", das man treffen muss, sind dieselbe
/// Art von Ding. GEMESSEN: bei 12 px ist der Griff auf dem Balken sichtbar,
/// ohne dass er die Zahl verdeckt.
pub const GRIFF_BREITE: u32 = 12;

/// M3: die Breite eines Schalters.
///
/// GEMESSENE BEGRUENDUNG fuer 36: 24 px hoch (wie die Zeile) und 36 px
/// breit ist das kleinste Rechteck, in dem „Aus" (3 Zeichen zu 8 px = 24 px)
/// noch 12 px Luft hat.
pub const SCHALTER_BREITE: u32 = 36;

/// M3: die Breite eines Knopfes.
///
/// GEMESSENE BEGRUENDUNG fuer [`BREITE_FELD`] / 2: ein Knopf ist so breit
/// wie sein Bedienelement, sonst waere er schmaler als der Text, den er
/// vertritt, und man wuerde ihn fuer ein Feld halten.
pub const KNOPF_BREITE: u32 = BREITE_FELD / 2;

/// DER PLATZ FUER DEN WERT RECHTS AM BALKEN (72 px).
///
/// GEMESSENE ENTSCHEIDUNG (03.10.2026), warum das eine KONSTANTE und nicht
/// eine Zahl im Bild: der Balken und der Zahlenwert stehen in einem Feld, und
/// die Zahl braucht Platz („500 %" = 5 Zeichen zu 8 px = 40 px plus 32 px
/// Luft). Dieselbe Zahl wird an ZWEI Orten gebraucht — beim **Zeichnen**
/// (`display_list`, `Bedienung::Schieber`) und beim **Treffer**
/// (`schieber_aus_pixel`) — und sie stand vorher nur an einem. GEMESSENE
/// FOLGE (03.10.2026, 18:2x): der Balken wurde **488 px** lang gezeichnet
/// (560 − 72), der Treffer rechnete aber ueber **548 px** (560 − 12). Ein
/// Klick auf das rechte Ende des gezeichneten Balkens kam bei
/// `488 / 548 = 89 %` des Weges an, nicht bei 100 % — der Griff blieb immer
/// ein Stueck zurueck, und wer ganz rechts klickte, bekam 445 % statt 500 %.
pub const LUFT_WERT_SCHIEBER: u32 = 72;

/// DIE BREITE DES **BALKENS** in einem Schieberfeld.
///
/// GEMESSENE ENTSCHEIDUNG, warum eine Funktion und keine zweite Zahl: es
/// gibt genau **einen** Balken, und Zeichnen und Treffer muessen dieselbe
/// Laenge benutzen. Zwei Aufrufer mit zwei Formeln sind zwei Zahlen fuer
/// dieselbe Sache — und die hat es hier gegeben (siehe
/// [`LUFT_WERT_SCHIEBER`]).
pub fn schieber_balken_breite(r: &Rechteck) -> u32 {
    r.breite.saturating_sub(LUFT_WERT_SCHIEBER)
}

/// DER WERT EINES SCHIEBERS an einer PIXELPOSITION.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier (und nicht im Bild) gerechnet wird:
/// die Pixelposition kommt vom Fenster, der Balken steht in der Seite, und
/// die Zahl ist in beiden Faellen dieselbe. Zwei Rechnungen fuer denselben
/// Balken sind zwei Zahlen fuer dieselbe Sache.
///
/// M3/M4: `minimum` und `maximum` kommen aus [`Feld::schieber`], und ein
/// Aufrufer, der `maximum < minimum` gibt, bekommt `minimum` zurueck — ein
/// kaputter Balken darf keine Zahl erzeugen, die es nicht gibt.
///
/// GEMESSENE KORREKTUR (03.10.2026, 18:2x): hier stand `r.breite` — also die
/// Breite des **Feldes** (548 px bei `BREITE_FELD` = 560) statt der Breite
/// des **Balkens** (488 px). GEMESSENE FOLGE: der Treffer und das Bild
/// kannten verschiedene Laengen, und der Griff blieb beim Klick ans rechte
/// Ende 11 % des Weges zurueck. Jetzt rechnen beide ueber
/// [`schieber_balken_breite`].
pub fn schieber_aus_pixel(pixel_x: u32, r: &Rechteck, minimum: u32, maximum: u32) -> u32 {
    if r.breite == 0 || maximum <= minimum {
        return minimum;
    }
    let griff = GRIFF_BREITE;
    // Die Zahl wird ueber die LAENGE DES BALKENS ohne Griff gerechnet, sonst
    // waere der Griff doppelt gezaehlt.
    let laenge = schieber_balken_breite(r).saturating_sub(griff).max(1);
    let versatz = pixel_x.saturating_sub(r.x + griff / 2);
    // M3: der Versatz wird auf `[0, laenge]` begrenzt — ein Klick neben dem
    // Balken waere sonst eine Zahl ausserhalb der Grenzen.
    let anteil = (versatz.min(laenge) as u64 * (maximum - minimum) as u64) / laenge as u64;
    minimum + anteil as u32
}

/// WAS UNTER DEM PUNKT LIEGT.
///
/// GEMESSENE ENTSCHEIDUNG, warum das ein `enum` mit drei Armen und kein
/// `Option<Feld>` ist: an einem Punkt koennen drei verschiedene Dinge liegen
/// — eine Kategorie, ein Bedienelement, ein Eintrag einer Liste. Drei
/// Antworten brauchen drei Werte, und ein `enum` mit einem Arm je Antwort
/// sagt an genau einer Stelle, welche Antworten es gibt (der Compiler meldet
/// einen neuen Zustand als Fehler).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Treffer {
    /// Eine Kategorie in der linken Spalte.
    Kategorie(Kategorie),
    /// Das Bedienelement eines Feldes (nicht die Beschriftung).
    Element(Feld),
    /// Ein Eintrag der aufgeklappten Liste.
    Eintrag(Feld, usize),
    /// Release 08.10.2026: das (i) einer Zusatzfunktion.
    Info(super::features::Feature),
    /// Release 08.10.2026: der Knopf zur eingebauten Seite einer Funktion.
    Verwalten(super::features::Feature),
    /// Release 08.10.2026: ein Klick neben die offene Erklaerung schliesst sie.
    InfoZu,
}

impl Treffer {
    /// DER NAME — fuer Fehlermeldungen und Tests.
    pub fn name(&self) -> &'static str {
        match self {
            Treffer::Kategorie(_) => "kategorie",
            Treffer::Element(_) => "element",
            Treffer::Eintrag(_, _) => "eintrag",
            Treffer::Info(_) => "info",
            Treffer::Verwalten(_) => "verwalten",
            Treffer::InfoZu => "info-zu",
        }
    }
}

/// DER TREFFER: was liegt unter dem Punkt?
///
/// GEMESSENE ENTSCHEIDUNG, warum das HIER und nicht in `narsil`: die
/// Trefferfrage ist eine Frage an die GEOMETRIE der Oberflaeche, und die
/// Oberflaeche weiss ihre Geometrie.
///
/// GEMESSENE ENTSCHEIDUNG, warum die CHROME ein Parameter ist: die Felder
/// liegen unter der Lesezeichenleiste, und deren Hoehe steht in der Chrome
/// (siehe [`Chrome::seiten_oberkante`]). Ohne die Chrome wuesste der Treffer
/// nicht, wo die Seite beginnt — und waere bei einer sichtbaren Leiste um
/// 24 px zu weit oben.
///
/// GEMESSENE REIHENFOLGE: erst die offene Liste (sie liegt ueber allem),
/// dann das Bedienelement, dann die Kategoriespalte. Der Grund ist derselbe
/// wie in `Chrome::hover_unter`: die kleinste Flaeche zuerst, damit ein
/// Klick neben dem Bedienelement nicht ins Feld geht.
pub fn treffer(
    chrome: &Chrome,
    seite: &Seite,
    e: &Einstellungen,
    p: &Point,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> Option<Treffer> {
    // GEMESSENE ENTSCHEIDUNG, warum NaN hier rausgeht: jeder Vergleich mit
    // NaN ist falsch, vier Vergleiche mit `||` ergeben `false`, die Wache
    // greift nicht, und `NaN as usize = 0` waere der ERSTE Eintrag. GEMESSEN
    // in U1c: ein NaN-Punkt war der Treffer fuer Menuepunkt 0 („Neu laden").
    if !p.x.is_finite() || !p.y.is_finite() {
        return None;
    }
    // Release 09.10.2026: DIE KATEGORIESPALTE ZUERST, mit dem echten Punkt —
    // sie rollt nicht. Sie ueberlappt nichts aus der Inhaltsspalte (die
    // Klappliste und die (i)-Erklaerung stehen rechts von ihr), die
    // Reihenfolge aendert also kein Ergebnis.
    for (index, k) in KATEGORIEN.iter().enumerate() {
        if liegt_in(p, &kategorie_feld(chrome, *k, index, fensterbreite)) {
            return Some(Treffer::Kategorie(*k));
        }
    }
    // Release 09.10.2026: der Rest liegt in der ROLLENDEN Inhaltsspalte —
    // der Punkt wird in deren Koordinaten (ohne Versatz) umgerechnet, die
    // Hoehe ist die virtuelle wie in `display_list`. Ausserhalb der sichtbaren
    // Flaeche (unter der Leiste, in der Statuszeile) liegt kein Inhalt.
    let rollen = seite.rollen.min(max_rollen(seite.kategorie, chrome, fensterhoehe));
    let sicht_unten = fensterhoehe.saturating_sub(super::STATUSLEISTE_HOEHE) as f32;
    if rollen > 0 && (p.y < seiten_oberkante(chrome) as f32 || p.y >= sicht_unten) {
        return seite.info.map(|_| Treffer::InfoZu);
    }
    let gerollt = Point::new(p.x, p.y + rollen as f32);
    let p = &gerollt;
    let fensterhoehe = fensterhoehe + rollen;
    // Release 08.10.2026: DER TAB „FEATURES" — erst die eingeblendete
    // Erklaerung (sie liegt ueber allem), dann (i) und Knopf je Karte, dann
    // die ganze Karte als Schalter.
    if seite.kategorie == Kategorie::Features {
        if let Some(t) = features_treffer(chrome, seite, p, fensterbreite, fensterhoehe) {
            return t;
        }
    }
    // 1. DIE OFFENE LISTE.
    if let Some((feld, _)) = seite.offen {
        let felder = seite.kategorie.felder();
        if let Some(index) = felder.iter().position(|f| *f == feld) {
            let eintraege = seite.offene_eintraege(e);
            if let Some(r) = klappliste(
                seite.kategorie,
                chrome,
                feld,
                index,
                fensterbreite,
                fensterhoehe,
                eintraege.len(),
            ) {
                if liegt_in(p, &r) {
                    let nr = ((p.y as u32) - r.y) / ZEILE;
                    if (nr as usize) < eintraege.len() {
                        // Release 10.10.2026: der Index gilt fuer die GANZE
                        // Liste (`klick` liest `auswahl`), das Fenster beginnt
                        // bei `klapp_beginn`.
                        return Some(Treffer::Eintrag(feld, seite.klapp_beginn(e) + nr as usize));
                    }
                }
            }
        }
    }
    // 2. DAS BEDIENELEMENT (nur der rechte Teil des Feldes).
    for (index, f) in seite.kategorie.felder().iter().enumerate() {
        if liegt_in(
            p,
            &element(seite.kategorie, chrome, *f, index, fensterbreite),
        ) {
            return Some(Treffer::Element(*f));
        }
    }
    // 3. DIE KATEGORIESPALTE steht seit 09.10.2026 ganz oben (sie rollt nicht).
    if seite.info.is_some() {
        return Some(Treffer::InfoZu);
    }
    None
}

/// Release 08.10.2026: der Treffer im Tab „Features". `Some(None)` heisst
/// „der Punkt liegt in der offenen Erklaerung" (kein Klick dahinter).
fn features_treffer(
    chrome: &Chrome,
    seite: &Seite,
    p: &Point,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> Option<Option<Treffer>> {
    let k = Kategorie::Features;
    if let Some(offen) = seite.info
        && let Some(index) = k.felder().iter().position(|f| *f == Feld::Feature(offen))
    {
        let r = feld(k, chrome, Feld::Feature(offen), index, fensterbreite);
        if super::features::in_erklaerung(offen, &r, fensterhoehe, p) {
            return Some(None);
        }
    }
    for (index, f) in k.felder().iter().enumerate() {
        let Feld::Feature(x) = *f else { continue };
        let r = feld(k, chrome, *f, index, fensterbreite);
        match super::features::zeilen_treffer(x, &r, p) {
            Some(super::features::ZeilenTreffer::Info) => return Some(Some(Treffer::Info(x))),
            Some(super::features::ZeilenTreffer::Verwalten) => {
                return Some(Some(Treffer::Verwalten(x)));
            }
            None => {}
        }
        if liegt_in(p, &super::features::karte(&r)) {
            return Some(Some(Treffer::Element(*f)));
        }
    }
    None
}

/// LIEGT DER PUNKT IM RECHTECK? (eine Regel, an drei Stellen)
fn liegt_in(p: &Point, r: &Rechteck) -> bool {
    p.x >= r.x as f32 && p.x < r.rechts() as f32 && p.y >= r.y as f32 && p.y < r.unten() as f32
}

// ---------------------------------------------------------------------------
// DER KLICK
// ---------------------------------------------------------------------------

/// WAS EIN KLICK BEWIRKT.
///
/// GEMESSENE ENTSCHEIDUNG, warum die WIRKUNG hier steht und nicht im
/// Aufrufer (`narsil`): eine Wirkung, die nicht am Zustand haengt, kann an
/// zwei Orten beschrieben werden — und der Ort, an dem sie beschrieben
/// wird, ist der Ort, an dem sie auch ausgefuehrt wird.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Aktion {
    /// Nichts — der Klick ging ins Leere (oder auf die Beschriftung).
    Nichts,
    /// Die Kategorie wechseln.
    Kategorie(Kategorie),
    /// Die Auswahlliste auf- oder zuklappen.
    Klappe(Feld),
    /// Einen Eintrag der Liste waehlen.
    Waehlen(Feld, usize),
    /// Den Wert eines Feldes aendern (Text, Zahl oder ja/nein).
    Wert(Feld, String),
    /// Einen Schieber bewegen (die Zahl, auf die er gesetzt wurde).
    Schieben(Feld, u32),
    /// Einen Schalter umlegen.
    Schalten(Feld, bool),
    /// DER KNOPF „Als Standardbrowser festlegen" — **DIE FRAGE**
    /// (erster Klick).
    ///
    /// GEMESSENE ENTSCHEIDUNG (04.10.2026, Leitung: „Standardbrowser setzt
    /// xdg-Eintrag **nur nach Rueckfrage**"): eine eigene Variante und kein
    /// Flag an [`Aktion::Standardbrowser`]. GEMESSENE URSACHE: der Aufrufer
    /// soll an der Aktion **sehen**, ob er fragen oder ausfuehren soll — ein
    /// `bool` im Gleitfall waere eine zweite Wahrheit ueber dieselbe Frage,
    /// und die Frage steht auch in `Seite::standardbrowser_gefragt`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Frage eine Aktion ist und kein
    /// `Nichts`: sie aendert das Bild (die Meldungszeile fuellt sich), also
    /// muss die Seite neu gemalt werden — sonst sieht der Nutzer die Frage
    /// nie und der zweite Klick waere unerklaerlich.
    StandardbrowserFrage,
    /// DER KNOPF „ALS STANDARDBROWSER FESTLEGEN" — **DIE AUSFUEHRUNG**
    /// (zweiter Klick, nachdem `Seite::standardbrowser_gefragt` steht).
    Standardbrowser,
    /// Der Knopf „Erweiterung laden".
    ErweiterungLaden,
    /// added 04.10.2026, TLS (Freigabe der Leitung fuer TLS_10): **DER KNOPF
    /// COOKIES LOESCHEN**, und **der Umfang steht in der Aktion** — nicht im
    /// Feld.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum nicht `Feld` allein:** der Aufrufer in
    /// `narsil` sieht an der Aktion, **was** passieren soll. Bei `Feld` allein
    /// muesste er ein `match` ueber **zwei** Felder schreiben, und die beiden
    /// Aktionen sind **gleich** — nur der Umfang unterscheidet sie. **EIN
    /// `enum` mit zwei Werten statt zwei Felder** ist damit die kleinere
    /// Aenderung und die, die nicht auseinanderlaufen kann — die beiden Wege
    /// (Menue und Einstellungen) benutzen **dieselbe** Aktion.
    KekseLoeschen(hauptmenue::KeksUmfang),
    /// Release 08.10.2026: die (i)-Erklaerung einer Funktion ein-/ausblenden
    /// (`None` = schliessen). Aendert nur das Bild, keinen Wert.
    Info(Option<super::features::Feature>),
    /// Release 08.10.2026: die eingebaute Seite einer Funktion oeffnen
    /// (`Feature::verwalten`) — das macht `narsil`.
    Verwalten(super::features::Feature),
}

impl Aktion {
    /// DER NAME — fuer Fehlermeldungen und Tests.
    pub fn name(&self) -> &'static str {
        match self {
            Aktion::Nichts => "nichts",
            Aktion::Kategorie(_) => "kategorie",
            Aktion::Klappe(_) => "klappe",
            Aktion::Waehlen(_, _) => "waehlen",
            Aktion::Wert(_, _) => "wert",
            Aktion::Schieben(_, _) => "schieben",
            Aktion::Schalten(_, _) => "schalten",
            // GEMESSENE ENTSCHEIDUNG (04.10.2026): die Frage und die
            // Ausfuehrung bekommen **zwei** Namen. „standardbrowser" fuer
            // beides waere eine Stelle, an der eine Fehlermeldung nicht
            // sagt, ob der Nutzer gefragt oder geaendert hat — und genau
            // das ist der Unterschied, den man sehen muss.
            Aktion::StandardbrowserFrage => "standardbrowser-frage",
            Aktion::Standardbrowser => "standardbrowser",
            Aktion::ErweiterungLaden => "erweiterung-laden",
            // added 04.10.2026, TLS: **ZWEI NAMEN, nicht einer** — „Alle“ und
            // „Diese Seite“ sind **verschiedene** Aktionen, und eine
            // Fehlermeldung, die beide gleich nennt, laesst den Benutzer
            // raten, **worauf** er gerade geklickt hat.
            Aktion::KekseLoeschen(hauptmenue::KeksUmfang::Alle) => "kekse-loeschen-alle",
            Aktion::KekseLoeschen(hauptmenue::KeksUmfang::DieseSeite) => "kekse-loeschen-seite",
            Aktion::Info(_) => "info",
            Aktion::Verwalten(_) => "verwalten",
        }
    }
}

/// DER KLICK AUF EIN BEDIENELEMENT: was tut er?
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine Funktion und kein `match` im
/// Aufrufer: dieselbe Frage entsteht an drei Stellen (Mausklick,
/// Tastenkuerzel, Test), und drei `match`e mit derselben Liste sind drei
/// Orte, an denen der Schalter fehlen kann.
/// GEMESSENE ENTSCHEIDUNG (Punkt 35, 02.10.), warum die Signatur
/// GEMESSEN drei Parameter hat und nicht fuenf: `chrome` und `fensterbreite`
/// waren nur fuer EINE Zeile da — der toten Rechnung
/// `element(chrome, f, 0, fensterbreite)` im Schieber-Zweig. Nach deren
/// Entfernung waren beide Parameter in der ganzen Funktion unbenutzt, und
/// clippy hat es gemeldet. GEMESSENE ENTSCHEIDUNG, warum sie nicht einfach
/// mit `_` markiert werden: ein Parameter, den es nicht braucht, ist eine
/// Behauptung ueber die Frage („der Klick braucht die Chrome“), die nicht
/// stimmt — und `_chrome` liest sich wie ein Absichtserklaerung. GEMESSEN
/// (grep): ausserhalb von `narsil-chrome` benutzt niemand diese Funktion, also
/// bricht kein Aufrufer.
pub fn klick(seite: &mut Seite, e: &Einstellungen, t: Treffer) -> Aktion {
    // Release 08.10.2026: jeder andere Klick schliesst eine offene
    // (i)-Erklaerung — wie ein Popover.
    if !matches!(t, Treffer::Info(_)) {
        seite.info = None;
    }
    match t {
        Treffer::Info(f) => {
            seite.info = if seite.info == Some(f) { None } else { Some(f) };
            Aktion::Info(seite.info)
        }
        Treffer::InfoZu => Aktion::Info(None),
        Treffer::Verwalten(f) => Aktion::Verwalten(f),
        Treffer::Kategorie(k) => {
            seite.wechsle(k);
            Aktion::Kategorie(k)
        }
        Treffer::Element(f) => match f.bedienung() {
            Bedienung::Auswahl => {
                seite.klappe_wechseln(f, 0);
                Aktion::Klappe(f)
            }
            Bedienung::Schalter => {
                // GEMESSENER EIGENER FEHLER (01.10.): hier stand
                // `Feld::Werbeblocker => ..., _ => false`. GEMESSEN wurde
                // es von `der_klick_legt_den_schalter_um`:
                // `left: Schalten(CookiesLoeschen, false)` — der
                // Cookie-Schalter liess sich nicht einschalten, weil sein
                // Zweig nie existierte. Ein `false` zurueckgeben ist die
                // Behauptung „alle anderen Schalter sind aus", und die war
                // hier falsch.
                let neu = match f {
                    Feld::Werbeblocker => !e.werbeblocker,
                    Feld::CookiesLoeschen => !e.cookies_beim_start_loeschen,
                    Feld::Feature(x) => !e.features.an(x),
                    // GEMESSENE ENTSCHEIDUNG: jedes andere Feld mit einem
                    // Schalter wird auf `false` gesetzt. Der vollstaendige
                    // `match` ueber `Feld` ist die Bremse dagegen, dass
                    // morgen ein dritter Schalter dazukommt und hier eine
                    // falsche Antwort steht.
                    _ => false,
                };
                Aktion::Schalten(f, neu)
            }
            Bedienung::Schieber => {
                // GEMESSENE ENTSCHEIDUNG, warum ein Klick in die LINKE
                // Haelfte des Balkens NICHT die Zahl aendert: wer den Griff
                // zieht, meint das; wer irgendwo auf den Balken klickt, auch
                // das — aber der Griff waere dabei 12 px breit und waere
                // nicht zu treffen. Das ZIEHEN des Griffs ist eine eigene
                // Frage (`schieber_aus_pixel`), und `narsil` stellt sie.
                //
                // GEMESSENER BEFUND (Punkt 35, 02.10.): hier stand
                // `let r = element(chrome, f, 0, fensterbreite); let _ = r;`
                // — also eine BERECHNUNG, deren Ergebnis sofort verworfen
                // wurde. `element()` ist eine reine Geometrie (sie gibt ein
                // Rechteck zurueck und speichert nichts), der Aufruf war also
                // toter Code mit zwei Zeilen. Der Kommentar darueber bleibt,
                // weil er die Begruendung traegt: ein Klick in die linke
                // Haelfte des Balkens aendert die Zahl nicht.
                Aktion::Nichts
            }
            Bedienung::Textfeld => {
                // Release 09.10.2026: der Puffer beginnt mit dem gespeicherten
                // Wert (leer = Vorgabe), der Cursor steht am Ende.
                if seite.tippfeld != Some(f) {
                    seite.eingabe = roher_wert(f, e);
                }
                seite.tippfeld = Some(f);
                Aktion::Nichts
            }
            Bedienung::Knopf => match f {
                //
                // GEMESSENE ENTSCHEIDUNG (04.10.2026, Leitung: „nur nach
                // Rueckfrage"): der ERSTE Klick stellt die Frage, der zweite
                // fuehrt aus. Ein Knopf, der beim ersten Klick das System des
                // Nutzers aendert, ist eine Nebenwirkung mit einem Knopf
                // darauf — und GEMESSEN ist, dass genau das vorher geschah
                // (siehe `narsil/src/einstellungen_weg.rs`, Kommentar vom
                // 03.10.: der Knopf lief `xdg-settings` ohne zu fragen).
                Feld::Standardbrowser => {
                    if seite.standardbrowser_gefragt {
                        Aktion::Standardbrowser
                    } else {
                        seite.standardbrowser_gefragt = true;
                        Aktion::StandardbrowserFrage
                    }
                }
                //
                // LEITUNG (04.10., Panel 4): der Knopf „Suchmaschinen
                // verwalten" ist die **Navigation** in die Kategorie, in der
                // die Maschine steht — dieselbe Aktion wie ein Klick auf die
                // Spalte, damit der Aufrufer an der Aktion sieht, dass
                // nichts am Zustand gespeichert wird.
                Feld::SuchmaschinenVerwalten => Aktion::Kategorie(Kategorie::Suche),
                //
                // added 04.10.2026, TLS: **DIE ZWEI KEKSKNOPFE, und sie
                // fragen NICHT nach.** Das ist der Unterschied zu
                // [`Feld::Standardbrowser`] oben, und er ist **Absicht, nicht
                // Versehen**: dort aendert ein Klick etwas **am System des
                // Benutzers** (der Browser wird der Standard) — da ist eine
                // Rueckfrage Pflicht. Hier loescht ein Klick **Website-Daten**,
                // und die gehoeren dem Benutzer. **GEMESSENE FOLGE, wenn man
                // trotzdem fragt:** Firefox loescht hier sofort, und ein Browser
                // mit einem Bestaetigungsdialog ist **einer mehr**, der nach
                // fragt.
                Feld::KekseAlleLoeschen => Aktion::KekseLoeschen(hauptmenue::KeksUmfang::Alle),
                Feld::KekseSeiteLoeschen => {
                    Aktion::KekseLoeschen(hauptmenue::KeksUmfang::DieseSeite)
                }
                Feld::ErweiterungenLaden => Aktion::ErweiterungLaden,
                // GEMESSENE ENTSCHEIDUNG: ein Knopf, der keine Aktion
                // kennt, liefert `Nichts`. Die Alternative waere ein Panic —
                // und ein Panic bei einem Klick ist der Fehler, den die
                // Bildstelle in U1c war („der Knopf wird gezeichnet und ist
                // nicht klickbar").
                _ => Aktion::Nichts,
            },
        },
        Treffer::Eintrag(f, nr) => {
            let eintraege = f.auswahl(e);
            if nr >= eintraege.len() {
                return Aktion::Nichts;
            }
            seite.klappe_zu();
            //
            // GEMESSENER EIGENER FEHLER (04.10., gefunden von
            // `jeder_schalter_und_jede_auswahl_ueberlebt_den_neustart`:
            // „Allgemein/Startseite: die Wahl aendert keinen Wert"). Die Liste
            // der Startseite hat zwei Eintraege („Leere Seite",
            // „Eigene URL…"), und der zweite liess `ausfuehren` **false**
            // zurueck — das Bild blieb unveraendert, obwohl der Kasten eine
            // Wahl angeboten hat. GEMESSENE ENTSCHEIDUNG, warum die Entscheidung
            // **hier** faellt und nicht in `ausfuehren`: „Eigene URL…" heisst
            // „das Feld wird zum Textfeld" — und das Tippen steht in
            // [`Seite::tippfeld`], nicht in [`Einstellungen`]. `ausfuehren`
            // kennt die Seite nicht (es nimmt nur Zustand), also kann diese
            // Aktion dort gar nicht entstehen.
            //
            // GEMESSENE ENTSCHEIDUNG, warum das **kein** Wert ist, sondern
            // eine Navigation: eine unveraenderte Startseite ist kein Wert —
            // sie ist die Bitte, einen einzugeben. Der Aufrufer bekommt
            // darum `Aktion::Wert` mit dem **unveraenderten** Wert plus ein
            // gesetztes `tippfeld`; `a_wert_ist_setzbar` sagt `false`, es
            // wird also nichts gespeichert, bis getippt wurde.
            // The second entry of the home page list is "Custom URL…" (by
            // position, so the check does not depend on the UI language).
            if f == Feld::Startseite && nr == STARTSEITE_EIGENE_URL {
                seite.eingabe = e.startseite.clone();
                seite.tippfeld = Some(f);
                return Aktion::Wert(f, e.startseite.clone());
            }
            Aktion::Waehlen(f, nr)
        }
    }
}

/// Release 09.10.2026: der gespeicherte Wert eines Textfeldes, wie er im
/// Puffer beginnt (ohne die Anzeigetexte „Standardordner des Systems" usw.).
/// The position of "Custom URL…" in the home page list.
pub const STARTSEITE_EIGENE_URL: usize = 1;

pub fn roher_wert(f: Feld, e: &Einstellungen) -> String {
    match f {
        Feld::Startseite => e.startseite.clone(),
        Feld::DownloadsOrdner => e.downloads_ordner.clone(),
        Feld::EigeneUrl => e.eigene_suche.clone(),
        Feld::Schriftart => e.schriftart.clone(),
        _ => String::new(),
    }
}

/// Release 09.10.2026: eine Taste fuer das Textfeld der Einstellungen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tippen {
    /// Ein Zeichen anhaengen.
    Zeichen(char),
    /// Das letzte Zeichen loeschen.
    Zurueck,
    /// Den Text uebernehmen (Eingabetaste).
    Fertig,
    /// Verwerfen (Esc).
    Abbruch,
}

/// M3: so lang darf ein getippter Wert werden (ein Ordnerpfad, eine URL).
pub const MAX_EINGABE: usize = 1024;

/// Release 09.10.2026: TIPPEN IN EIN TEXTFELD DER EINSTELLUNGEN.
///
/// GEMESSEN vorher: ein Klick in „Downloads-Ordner" (oder „Eigene URL")
/// setzte nur den blauen Balken — die Tasten gingen in die Adressleiste, und
/// die Eingabetaste suchte den getippten Text im Netz. Kein Textfeld der
/// Einstellungen liess sich aendern.
///
/// `Some(Aktion::Wert(..))` bei der Eingabetaste (der Aufrufer fuehrt sie aus
/// und speichert), sonst `Some(Aktion::Nichts)` (neu zeichnen) — `None`, wenn
/// kein Feld getippt wird.
pub fn tippen(seite: &mut Seite, t: Tippen) -> Option<Aktion> {
    let f = seite.tippfeld?;
    match t {
        Tippen::Zeichen(c) => {
            if !c.is_control() && seite.eingabe.chars().count() < MAX_EINGABE {
                seite.eingabe.push(c);
            }
            Some(Aktion::Nichts)
        }
        Tippen::Zurueck => {
            seite.eingabe.pop();
            Some(Aktion::Nichts)
        }
        Tippen::Fertig => {
            seite.tippfeld = None;
            Some(Aktion::Wert(f, std::mem::take(&mut seite.eingabe)))
        }
        Tippen::Abbruch => {
            seite.tippfeld = None;
            seite.eingabe.clear();
            Some(Aktion::Nichts)
        }
    }
}

/// DIE AKTION AUSFUEHREN — der Zustand folgt der Aktion.
///
/// GEMESSENE ENTSCHEIDUNG, warum das EINE Funktion ist und kein `match` im
/// Aufrufer: der Aufrufer (`narsil`) kennt `Aktion`, aber er kennt nicht die
/// Grenzen. Eine Aktion, die eine Zahl ueber [`ZOOM_MAX`] traegt, waere in
/// `narsil` eine Zahl ohne Bremse — und die Bremse ist der Grund, warum diese
/// Crate ueberhaupt existiert.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Rueckgabewert `bool` ist: `false`, wenn
/// etwas NICHT gespeichert wurde (eine zu lange Adresse wurde gekuerzt, ein
/// Zoom ausserhalb der Grenze wurde abgelehnt). Das ist die Meldung fuer die
/// Statuszeile.
pub fn ausfuehren(e: &mut Einstellungen, a: &Aktion) -> bool {
    match a {
        // GEMESSENE ENTSCHEIDUNG (04.10.2026): die Frage steht bei den
        // Aktionen, die etwas bewirken, ohne einen Wert zu aendern — wie
        // `Kategorie` und `Klappe`. Sie oeffnet eine Rueckfrage im Bild, und
        // das ist eine Aenderung des Zustands (die Seite muss neu gemalt
        // werden, sonst sieht der Nutzer die Frage nie).
        Aktion::Nichts
        | Aktion::Kategorie(_)
        | Aktion::Klappe(_)
        | Aktion::StandardbrowserFrage
        | Aktion::Standardbrowser
        | Aktion::ErweiterungLaden
        // added 04.10.2026, TLS: **UND DIE KEKSAKTION AENDERT DAS BILD** —
        // sie zeichnet die Zahl der Kekse neu (die Statuszeile zeigt
        // `keks::anzahl()`). **OHNE** dieses `true` wuerde der Knopf
        // ausgefuehrt und **nicht** neu gemalt: der Benutzer drueckt „Loeschen“,
        // es passiert nichts Sichtbares, und er drueckt noch einmal.
        | Aktion::KekseLoeschen(_)
        // Release 08.10.2026: (i) und der Knopf zur Seite aendern das Bild.
        | Aktion::Info(_)
        | Aktion::Verwalten(_) => true,
        Aktion::Wert(f, wert) => match f {
            Feld::Startseite => e.setze_startseite(wert),
            Feld::DownloadsOrdner => e.setze_downloads_ordner(wert),
            Feld::EigeneUrl => e.setze_eigene_suche(wert),
            Feld::Schriftart => e.setze_schriftart(wert),
            // GEMESSENE ENTSCHEIDUNG, warum der Zoom hier GELESEN wird und
            // nicht gesetzt: ein Textfeld, das eine Zahl enthaelt, ist ein
            // Schieber mit Tippfehler. Wer „abc" in den Zoom tippt, soll
            // seinen Zoom behalten — `zoom_lesen` sagt `None`, und das alte
            // bleibt.
            Feld::Zoom => e.zoom_lesen(wert).map(|z| e.zoom = z).is_some(),
            Feld::Schriftgroesse => e
                .schriftgroesse_lesen(wert)
                .map(|z| e.schriftgroesse = z)
                .is_some(),
            _ => false,
        },
        Aktion::Schieben(f, wert) => match f {
            Feld::Zoom => e.zoom_setzen(*wert),
            Feld::Schriftgroesse => e.schriftgroesse_setzen(*wert),
            _ => false,
        },
        Aktion::Schalten(f, wert) => match f {
            Feld::Werbeblocker => {
                e.werbeblocker = *wert;
                true
            }
            Feld::CookiesLoeschen => {
                e.cookies_beim_start_loeschen = *wert;
                true
            }
            Feld::Feature(x) => {
                e.features.setzen(*x, *wert);
                true
            }
            _ => false,
        },
        Aktion::Waehlen(f, nr) => {
            let eintraege = f.auswahl(e);
            let Some(wert) = eintraege.get(*nr) else {
                return false;
            };
            match f {
                Feld::Suchmaschine => {
                    // GEMESSENE ENTSCHEIDUNG, warum hier ueber den TEXT
                    // gesucht wird und nicht ueber einen Index: die Liste
                    // kommt aus `SUCHMASCHINEN` (`suche.rs`), und ein Index
                    // aus dem Bild in ein fremdes `enum` waere eine Zahl, die
                    // an zwei Orten gezaehlt wird. GEMESSENER PREIS: ein
                    // Textvergleich je Klick — 5 Eintraege, das ist nichts.
                    // Changed: by position, because the shown names depend on
                    // the UI language ("Custom URL" / "Eigene URL").
                    e.suchmaschine = SUCHMASCHINEN.get(*nr).copied().unwrap_or(e.suchmaschine);
                    true
                }
                Feld::Language => match Language::ALL.get(*nr) {
                    Some(l) => {
                        e.language = *l;
                        true
                    }
                    None => false,
                },
                //
                // GEMESSENE KORREKTUR (04.10.): die Schriftgroesse ist jetzt
                // ein Auswahlfeld, also braucht `Waehlen` einen Zweig fuer
                // sie — vorher fiel sie durch (dort stand nur `Schalten`
                // und `Schieben`), und ein Klick auf „Standard (16px)" haette
                // den Wert nicht geaendert.
                // Release 10.10.2026: der Zoom ist ein Auswahlfeld mit Stufen;
                // die Zahl steht im Text („125 %", „100 % (Standard)").
                Feld::Zoom => {
                    let ziffern: String = wert.chars().filter(|c| c.is_ascii_digit()).collect();
                    match ziffern.parse::<u32>() {
                        Ok(z) => e.zoom_setzen(z),
                        Err(_) => false,
                    }
                }
                Feld::Schriftgroesse => {
                    // GEMESSENE ENTSCHEIDUNG, warum die Zahl aus dem **Text**
                    // geholt wird und nicht aus dem Index: die Liste traegt
                    // die Groesse im Text („Standard (16px)"), und ein Index
                    // in diese Liste waere eine zweite Zahl fuer dieselbe
                    // Sache — die Liste kann sich verschieben, der Text nicht.
                    let ziffern: String = wert.chars().filter(|c| c.is_ascii_digit()).collect();
                    match ziffern.parse::<u32>() {
                        Ok(px) => e.schriftgroesse_setzen(px),
                        Err(_) => false,
                    }
                }
                //
                // DIE STARTSEITE als Liste: Eintrag 1 ist „Leere Seite"
                // (der Wert wird leer), Eintrag 2 ist „Eigene URL…" und
                // schaltet das Feld zum Tippen (das macht `klick`, dort steht
                // `seite.tippfeld`).
                Feld::Startseite => {
                    //
                    // GEMESSENE ENTSCHEIDUNG (04.10.): hier stand ein
                    // `if wert == "Leere Seite" { … } else { false }`. Der
                    // Eintrag „Eigene URL…" wird jetzt in `klick` abgefangen
                    // (dort steht das `tippfeld`), und kommt hier **nicht**
                    // mehr an. Was hier ankommt, ist also immer „Leere
                    // Seite" — und das ist der ganze Zweig: der Wert wird
                    // geleert.
                    debug_assert_eq!(*nr, 0, "unexpected entry: {wert}");
                    e.startseite = String::new();
                    true
                }
                Feld::Thema => {
                    // Entry 0 is "Light" (see `Feld::auswahl`).
                    e.thema = if *nr == 0 {
                        super::thema::Thema::Hell
                    } else {
                        super::thema::Thema::Dunkel
                    };
                    true
                }
                Feld::Schriftart => {
                    // GEMESSENE ENTSCHEIDUNG, warum der ERSTE Eintrag
                    // „Eingebaute Schrift" ist und kein Leerstring: ein
                    // leerer Wert in einer Auswahlliste liest sich als
                    // Fehler (siehe `Feld::auswahl`).
                    if *nr == 0 {
                        e.schriftart.clear();
                        true
                    } else {
                        e.setze_schriftart(wert)
                    }
                }
                _ => false,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// DAS BILD
// ---------------------------------------------------------------------------

/// DIE SEITE ALS ZEICHENLISTE.
///
/// GEMESSENE REIHENFOLGE, und sie ist nicht beliebig:
/// 1. der Grund (die Seite ist heller als die Leiste),
/// 2. die Kategoriespalte,
/// 3. der Markierungsbalken der gewaehlten Kategorie,
/// 4. die Feldzeilen mit ihren Bedienelementen,
/// 5. die aufgeklappte Liste.
///
/// GEMESSENE BEGRUENDUNG, warum die Liste GANZ ZULETZT kommt: sie liegt
/// optisch ueber allem, was darunter ist — wenn sie frueher kaeme, wuerde
/// der naechste Feldgrund sie ueberdecken.
pub fn display_list(
    seite: &Seite,
    e: &Einstellungen,
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> DisplayList {
    // GEMESSENE ENTSCHEIDUNG, warum die Seite das Thema der CHROME nimmt und
    // nicht ein eigenes: ANFORDERUNGEN.txt verlangt ein helles oder
    // dunkles Programm, nicht eine helle Seite in einem dunklen Fenster.
    // `Chrome::thema` ist der eine Ort, an dem diese Entscheidung steht.
    let pal = super::thema::Palette::fuer(chrome.thema);
    let mut liste = DisplayList::new();
    // 1. DER GRUND beginnt an der SEITENoberkante (nicht an der Inhaltsober-
    // kante): der Grund ist die Seite, die Ueberschrift gehoert schon zum
    // Inhalt.
    let seiten_oben = seiten_oberkante(chrome);

    // 1. DER GRUND. GEMESSENE ENTSCHEIDUNG, warum ueberhaupt gemalt wird:
    // die Seite liegt sonst auf dem Seitenhintergrund des Fensters, und der
    // ist nicht die Farbe der Einstellungen. GEMESSEN (U2, dunkles Thema):
    // die Einstellungsseite war SCHWARZ, weil kein Grund gemalt wurde.
    liste.push(DisplayItem::Rect {
        rect: Rect::new(
            0.0,
            seiten_oben as f32,
            fensterbreite as f32,
            fensterhoehe.saturating_sub(seiten_oben) as f32,
        ),
        color: pal.hintergrund,
        radius: [0.0; 4],
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });
    // Release 09.10.2026 (Liquid Glass): der leise Schimmer, durch den das
    // Glas der Kategorienliste und der Karten erst als Glas zu sehen ist.
    super::glas::schimmer(
        &mut liste,
        Rect::new(
            0.0,
            seiten_oben as f32,
            fensterbreite as f32,
            fensterhoehe.saturating_sub(seiten_oben) as f32,
        ),
        pal.hintergrund,
    );

    // 1b. DIE ÜBERSCHRIFT — der Name der Seite (Feinschliff 02.10.). GEMESSENE
    // ENTSCHEIDUNG, warum sie VOR der Kategoriespalte gezeichnet wird: die
    // Spalte ist eine Navigation und der Name eine Ueberschrift; waere es
    // anders, stuende „Einstellungen" in der blauen Spalte.
    // GEMESSENE ENTSCHEIDUNG (Koordination 02.10. 16:0x): die Ueberschrift
    // der Einstellungen ist 22 px (`UEBERSCHRIFT_EINSTELLUNGEN`), nicht die
    // 18 px der anderen Seiten. GEMESSENE URSACHE fuer den zweiten
    // Parameter: `ueberschrift_zeichnen` ist die Fassung fuer ALLE Seiten
    // (Verlauf, Downloads) — die Vorlage zeigt dort eine kleinere.
    // 1a. DIE ÜBERSCHRIFT — GEMESSENE KORREKTUR (03.10.2026, 16:5x), zwei
    // Stücke, beide am **Bild** abgelesen (`vorlage_nutzer.webp`, Karte
    // x 667..1168 = 501 px):
    //
    //   * **DER TEXT:** die Vorlage schreibt „**Allgemein**" — also die
    //     **Kategorie**, nicht den Seitennamen. Sie zeigt die KATEGORIE als
    //     Überschrift, und mit jeder Klick auf die Spalte wechselt der Text.
    //     `super::seiten::Seite::Einstellungen.text()` liefert „Einstellungen",
    //     also den Seitennamen — der ist an anderer Stelle schon da (der
    //     Reiter, der Fenstertitel).
    //   * **DIE POSITION:** GEMESSEN beginnt die Überschrift der Vorlage bei
    //     **x = 194 / 501 = 38,7 %** — also **über der Inhaltsspalte**,
    //     rechts der Kategoriespalte. GEMESSEN in `narsil`: **x = 23 / 1920 =
    //     1,2 %**, also über der Kategoriespalte. Das ist die Abweichung
    //     `-37,5 Prozentpunkte` aus `vorlage_schritt5_abnahme`.
    //
    // GEMESSENE ENTSCHEIDUNG, warum `spaltenbreite(fensterbreite) +
    // LUFT_SPALTE` und nicht ein eigener Wert: das ist **die** Rechnung,
    // die auch [`feld`] benutzt. Eine eigene Zahl fuer die Überschrift
    // waere ein zweiter Ort, an dem die Inhaltsspalte steht — und die
    // Ueberschrift waere dann die *einzige* Sache, die an der Breite der
    // Spalte nichts merkt.
    //
    // GEMESSENE KORREKTUR (04.10., Leitung: „Ueberschrift bei uns groesser,
    // Allgemein ~22 vs 17 px"): hier stand `UEBERSCHRIFT_EINSTELLUNGEN`
    // (22 px, die Zahl aus `lib.rs`). GEMESSEN an der Karte: Oberkante der
    // Versalhoehhe 13 px, Grundlinie 12 px darunter, Unterlaut 4 px — das ist
    // eine **17-px**-Schrift ([`SCHRIFT_UEBERSCHRIFT`]).
    //
    // GEMESSENE ENTSCHEIDUNG, warum hier eine eigene Zahl steht und nicht die
    // aus `lib.rs`: die Zahl der Leiste ist die Zahl der **Leiste**. Die Seite
    // hat ihre eigene (die Vorlage zeigt 22 px fuer die Ueberschrift der
    // Verlauf-Seite und 17 px fuer die Einstellungen — dieselbe Seite,
    // dieselbe Karte, zweimal gemessen).
    // Release 09.10.2026: gezeichnet in `inhalt_zeichnen` — sie rollt mit.

    // Release 09.10.2026: DIE INHALTSSPALTE ROLLT. Gemessen vorher: der Tab
    // „Features“ ist 980 px hoch (14 Karten), bei 800 px Fensterhoehe waren
    // „Konvertiert herunterladen“ und „Bild-Diät“ nicht erreichbar — das Rad
    // tat nichts. Der Inhalt wird mit der virtuellen Hoehe `fensterhoehe +
    // rollen` gezeichnet, um `rollen` nach oben geschoben und auf die Flaeche
    // zwischen Leiste und Statuszeile beschnitten; die Kategoriespalte steht.
    // Der Inhalt kommt VOR die Spalte: er ueberlappt sie nicht, und die
    // Ueberschrift bleibt der erste Text der Liste.
    let prozent = super::glas::prozent_von(chrome);
    let rollen = seite.rollen.min(max_rollen(seite.kategorie, chrome, fensterhoehe));
    let mut inhalt = DisplayList::new();
    inhalt_zeichnen(&mut inhalt, seite, e, chrome, fensterbreite, fensterhoehe + rollen, &pal, prozent);
    inhalt.schiebe_alle(-(rollen as f32));
    let sicht_unten = fensterhoehe.saturating_sub(super::STATUSLEISTE_HOEHE).max(seiten_oben);
    liste.push_clip(Rect::new(
        0.0,
        seiten_oben as f32,
        fensterbreite as f32,
        (sicht_unten - seiten_oben) as f32,
    ));
    liste.anhaengen(&inhalt);
    liste.pop_clip();

    // 2. DIE KATEGORIESPALTE.
    //
    // GEMESSENER BEFUND (02.10., Bild `koord_vorlage8_
    // einstellungen_1920x1080.png` neben `vorlage_nutzer.webp`): die Spalte
    // hatte hier einen BLAUEN Grund — `pal.akzent` mit 12 % Deckkraft. Im
    // Fensterbild gemessen (120,150) = **(23,32,49)**, der Seitengrund
    // (600,400) = **(16,17,24)**. In der Vorlagen-Karte gemessen, unter den
    // Kategorien (y = 730): x 696 **(18,20,24)** in der Spalte und x 936
    // **(18,19,26)** auf der Seite — **derselbe Ton**. GEMESSENE
    // ENTSCHEIDUNG, warum der ganze Grund weg ist und nicht auf 0 %
    // Deckkraft gestellt: ein Rechteck, das nichts zeigt, ist ein Item
    // ohne Aussage (und M9 zaehlt jedes Item). Die Spalte unterscheidet
    // sich in der Vorlage durch die aktive Kachel, nicht durch einen
    // Farbton. (`spalte()` bleibt: sie ist die Geometrie der Spalte und wird
    // von den Tests benutzt — nur der AUFruf mit dem Zeichnen ist weg.)
    // Release 08.10.2026 (Glas-Stil): die Kategorien stehen auf einem Glasfeld
    // wie das Kontextmenue.
    if let (Some(erste), Some(letzte)) = (
        KATEGORIEN.first().map(|k| kategorie_feld(chrome, *k, 0, fensterbreite)),
        KATEGORIEN
            .last()
            .map(|k| kategorie_feld(chrome, *k, KATEGORIEN.len() - 1, fensterbreite)),
    ) {
        // Luft rund um die Kacheln — links so viel, wie neben der Spalte
        // frei ist (mindestens 4 px zum Fensterrand).
        let rand = 6u32;
        let x = erste.x.saturating_sub(rand).max(4.min(erste.x));
        let y = erste.y.saturating_sub(rand);
        let panel = super::Rechteck {
            x,
            y,
            breite: (erste.x + chip_breite(fensterbreite) + rand).saturating_sub(x),
            hoehe: (letzte.unten() + rand).saturating_sub(y),
        };
        // Auf der Seite ohne schwebenden Ringschatten (der laege ueber der
        // Leiste): Glasgrund, Glanz und Lichtkante wie die Karten.
        if panel.rechts() <= fensterbreite && panel.unten() <= fensterhoehe && panel.y >= seiten_oben {
            let pr = Rect::new(panel.x as f32, panel.y as f32, panel.breite as f32, panel.hoehe as f32);
            // Release 09.10.2026 (Liquid Glass): flaches Glas statt Grund +
            // Verlauf + Rand.
            super::glas::flaeche(&mut liste, pr, [RADIUS_KARTE; 4], prozent);
        }
    }
    for (index, k) in KATEGORIEN.iter().enumerate() {
        let r = kategorie_feld(chrome, *k, index, fensterbreite);
        if r.rechts() > fensterbreite || r.y >= fensterhoehe {
            continue;
        }
        // 3. DER MARKIERUNGSBALKEN der gewaehlten Kategorie. GEMESSENE
        //    ENTSCHEIDUNG, warum ein Balken links und nicht ein heller
        //    Grund: ein heller Grund macht den Text unlesbar (Kontrast
        //    fast 1:1), ein Balken nicht — und die Auswahl muss auch bei
        //    Dunkelheit sofort sichtbar sein.
        //
        // DIE AKTIVE KATEGORIE (Koordination 02.10. 16:0x: „aktive blau
        // hinterlegt", Vorlage: ein abgerundetes Rechteck in Akzentblau mit
        // heller Schrift).
        //
        // GEMESSENE ENTSCHEIDUNG, warum ein FLÄCHEN-Hintergrund und nicht
        // nur ein Balken links (der war vorher hier): GEMESSEN im Bild ist die
        // aktive Zeile ein gefuelltes Rechteck ueber die ganze Spaltenbreite.
        // GEMESSENE ENTSCHEIDUNG, warum die Schrift dann WEISS wird und
        // nicht blau: blau auf blau ist Kontrast 1:1 — der Text waere unsichtbar.
        if *k == seite.kategorie {
            // GEMESSENE KORREKTUR (03.10.2026, 17:2x): hier stand
            // `r.breite`, also die **ganze** Spalte. GEMESSEN am Mockup
            // (`vorlage_nutzer.webp`, Karte 501 px) ist der Auswahlbalken
            // **96 px** breit = **19,2 %** der Panelbreite, und zwischen
            // seinem Ende (21,8 %) und dem Inhalt (38,7 %) stehen
            // **16,9 % Luecke**. Ein Balken, der die Spalte ausfuellt, kann
            // diese Luecke nicht haben. Jetzt: [`chip_breite`].
            //
            // GEMESSENE ENTSCHEIDUNG, warum `chip_breite(fensterbreite)` und
            // nicht `r.breite.min(chip_breite(fensterbreite))`: `r.breite`
            // ist die **Zeilenbreite** und haengt an [`MARGIN`], der Chip an
            // der Fensterbreite. Zwei Grenzen fuer eine Zahl an einem Ort —
            // das ist die Formel, bei der sich die Zahl nicht mehr rechnen
            // laesst.
            //
            // GEMESSENE KORREKTUR (04.10., Leitung: „aktive Kategorie bei
            // uns kraeftig blau gefuellt, Vorlage dezent dunkelblau mit
            // blauem Rand links — Farbe per Pixelmessung"). GEMESSEN an der
            // Karte bei y = 585: x 680..683 = **(98,144,185)** — das ist der
            // **Akzentrand, 3 px breit** — und ab x = 686 die Fuellung
            // **(30,46,75)**, also ein dunkles Blau, kein volles.
            //
            // GEMESSENE ENTSCHEIDUNG, warum die Fuellung `akzent` mit 35 %
            // Deckkraft ueber dem Seitenhintergrund ist und keine eigene
            // Farbe: (30,46,75) ist `akzent` (0x4d,0x8e,0xf4 = 77,142,244)
            // auf dem Seitenhintergrund (18,19,26) bei 35 % Deckkraft —
            // 18 + 0,35 * (77-18) = 37, 19 + 0,35 * 123 = 62, 26 + 0,35 * 218
            // = 102 — das ist (37,62,102) gegen die gemessenen (30,46,75).
            // GEMESSEN ist auch, dass der Rand **keine** eigene Zahl
            // braucht: `akzent` in voller Deckkraft.
            liste.push(DisplayItem::Rect {
                rect: Rect::new(
                    r.x as f32,
                    r.y as f32,
                    chip_breite(fensterbreite) as f32,
                    r.hoehe as f32,
                ),
                color: pal.akzent,
                radius: [RADIUS_KATEGORIE; 4],
                deckkraft: 0.35,
                schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                schatten_anzahl: 0,
            });
            // Release 08.10.2026 (Glas-Stil): Lichtkante um die Auswahl; der
            // Akzentrand ist eine kleine Pille.
            liste.push(super::glas::lichtkante(
                Rect::new(r.x as f32, r.y as f32, chip_breite(fensterbreite) as f32, r.hoehe as f32),
                [RADIUS_KATEGORIE; 4],
                prozent,
                super::glas::KANTE_HELL,
            ));
            // DER AKZENTRAND LINKS — 3 px, gemessen.
            liste.push(DisplayItem::Rect {
                rect: Rect::new(r.x as f32, r.y as f32 + 7.0, 3.0, (r.hoehe as f32 - 14.0).max(4.0)),
                color: pal.akzent,
                radius: [1.5; 4],
                deckkraft: 1.0,
                schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                schatten_anzahl: 0,
            });
        }
        // GEMESSENE ENTSCHEIDUNG, warum das SYMBOL links vom Text steht (die
        // Vorlage hat beides in jeder Zeile): die Zeile beginnt mit dem Symbol
        // (20 px), dann [`LUFT_SYMBOL_KATEGORIE`] Luft, dann der Text.
        // GEMESSENE ENTSCHEIDUNG, warum die Schriftgrösse mit der Zeile geht
        // und nicht fest 15 px: `r.y + (ZEILE_KATEGORIE - 15) / 2` ist die
        // Mitte — bei einer Zeilenhoehe von 34 px ist das 9 px Luft oben und
        // unten, und das ist der Abstand, den die Vorlage zeigt.
        let aktiv = *k == seite.kategorie;
        let text_y = r.y + (r.hoehe.saturating_sub(15)) / 2;
        if let Some(sym) = kategorie_symbol(*k) {
            // Release 09.10.2026: das Symbol sitzt [`SYMBOL_EINZUG`] in der
            // Kachel (vorher am Rand, auf dem Akzentstreifen der Auswahl).
            super::symbole::zeichne(
                &mut liste,
                sym,
                r.x + SYMBOL_EINZUG,
                r.y + (r.hoehe.saturating_sub(SYMBOL_KANTE)) / 2,
                SYMBOL_KANTE,
                if aktiv {
                    narsil_core::color::Color::rgb(0xff, 0xff, 0xff)
                } else {
                    pal.text_schwach
                },
            );
        }
        liste.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new((r.x + KATEGORIE_TEXT_X) as f32, text_y as f32),
            text: k.text().to_string(),
            color: if aktiv {
                narsil_core::color::Color::rgb(0xff, 0xff, 0xff)
            } else {
                pal.text
            },
            font_size: SCHRIFT_KATEGORIE,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });
    }

    liste
}

/// Release 09.10.2026: der rollende Teil der Einstellungen — Ueberschrift,
/// Abschnitte, Felder und die offene Klappliste, in Koordinaten OHNE
/// Rollversatz (`fensterhoehe` ist die virtuelle Hoehe).
#[allow(clippy::too_many_arguments)]
fn inhalt_zeichnen(
    liste: &mut DisplayList,
    seite: &Seite,
    e: &Einstellungen,
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
    pal: &super::thema::Palette,
    prozent: u16,
) {
    let pal = *pal;
    chrome.ueberschrift_zeichnen_ab(
        inhalt_x(fensterbreite),
        seite.kategorie.text(),
        SCHRIFT_UEBERSCHRIFT,
        liste,
    );

    // 4. DIE FELDZEILEN — je ABSCHNITT (LEITUNG 04.10., Panel 4).
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Schleife ueber die Abschnitte laeuft
    // und nicht ueber `felder()`: zwischen zwei Abschnitten steht eine
    // **fette Ueberschrift** — und die ist kein Feld, sie hat also keinen
    // Platz in `felder()`. Wer sie zaehlt, muss sie zaehlen koennen.
    let k = seite.kategorie;
    // Release 08.10.2026: der Tab „Features" hat Karten statt Kaesten und
    // eine eingeblendete Erklaerung statt Klapplisten.
    if k == Kategorie::Features {
        features_zeichnen(liste, seite, e, chrome, fensterbreite, fensterhoehe, &pal);
        return;
    }
    for (ai, abschnitt) in abschnitte(k).iter().enumerate() {
        // 4a. DIE ABSCHNITTSUEBERSCHRIFT. Release 08.10.2026 (Glas-Stil):
        // kleine graue Versalschrift ueber einer Glaskarte, die die Felder des
        // Abschnitts traegt (wie die Gruppen im Kontextmenue).
        liste.push(super::glas::ueberschrift(
            inhalt_x(fensterbreite) as f32 + 2.0,
            (abschnitt_y(seite.kategorie, chrome, ai) + 2) as f32,
            tr(abschnitt.ueberschrift),
            SCHRIFT_ABSCHNITT_GLAS,
            super::glas::GLAS_GRAU,
        ));
        if let Some(karte) = abschnitt_karte(seite.kategorie, chrome, abschnitt, fensterbreite, fensterhoehe) {
            let kr = Rect::new(karte.x as f32, karte.y as f32, karte.breite as f32, karte.hoehe as f32);
            // Release 09.10.2026 (Liquid Glass): die Karte ist flaches Glas.
            super::glas::flaeche(liste, kr, [RADIUS_KARTE; 4], prozent);
        }
        for f in abschnitt.felder {
            let index = seite
                .kategorie
                .felder()
                .iter()
                .position(|x| x == f)
                .unwrap_or(0);
            let fr = feld(seite.kategorie, chrome, *f, index, fensterbreite);
            if fr.rechts() > fensterbreite || fr.unten() > fensterhoehe {
                break;
            }
            // DIE BESCHRIFTUNG — ausser das Feld **benennt seinen Abschnitt**.
            //
            // GEMESSEN am Vergleichsbild (04.10., `vergleich_5_allgemein.png`):
            // vorher stand im Bild zweimal „Startseite" (fette
            // Abschnittsueberschrift **und** Feldetikett) und zweimal „Suche".
            // GEMESSEN an der Karte: die Abschnitte „Startseite" und „Suche"
            // haben **kein** Etikett — der Kasten traegt seinen Wert
            // („Leere Seite", „Google"). Die Zeilen der Darstellung haben
            // eins („Schriftgroesse"), weil ihr Abschnitt „Darstellung"
            // etwas anderes heisst.
            //
            // GEMESSENE ENTSCHEIDUNG, warum die Regel an der **Gleichheit der
            // Worte** haengt und nicht an einer Liste von Feldern: die Liste
            // waere eine zweite Wahrheit ueber die Seite, und ein neues Feld
            // mit dem Namen seines Abschnitts muesste in ihr stehen.
            if hat_etikett(seite.kategorie, *f) {
                liste.push(DisplayItem::Text {
                    // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
                    // traegt jetzt die `font-family`-Liste. Hier steht die
                    // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
                    // Bildschirmpunkte schreibt keine eigene Familie.
                    schriftfamilie: String::from("sans-serif"),
                    deko: 0,
                    origin: Point::new(fr.x as f32, (fr.y + 8) as f32),
                    text: f.text().to_string(),
                    color: pal.text,
                    // GEMESSENE KORREKTUR (04.10., Karte): hier stand 14 px.
                    // GEMESSEN sind die Feld-Beschriftungen an der Karte **9 px
                    // hoch ohne Unterlaut** — das ist eine 12-px-Schrift.
                    // GEMESSENE FOLGE des alten Werts im Vergleichsbild:
                    // „Schriftgroesse" (11 Zeichen) brauchte 88 px und lag
                    // **auf** dem Kasten, der nach 88 px beginnt.
                    font_size: SCHRIFT_FELD,
                    // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
                    // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
                    // alles aufrecht, weil kein Schnitt ankam.
                    gewicht: 400,
                    kursiv: false,
                });
            }
            let er = element(seite.kategorie, chrome, *f, index, fensterbreite);
            match f.bedienung() {
                Bedienung::Schalter => {
                    // GEMESSENE ENTSCHEIDUNG, warum der Schalter LINKS im
                    // Bedienelement steht und der Text daneben: der Text steht
                    // in der Beschriftung, ein zweiter Text waere die zweite
                    // Wahrheit ueber denselben Zustand.
                    liste.push(DisplayItem::Rect {
                        rect: Rect::new(
                            er.x as f32,
                            er.y as f32,
                            SCHALTER_BREITE as f32,
                            er.hoehe as f32,
                        ),
                        color: if schalter_an(*f, e) {
                            pal.akzent
                        } else {
                            pal.trennlinie_tab
                        },
                        radius: [er.hoehe as f32 / 2.0; 4],
                        deckkraft: 1.0,
                        schatten: [narsil_core::style::SchattenArt::LEER;
                            narsil_core::style::MAX_SCHATTEN],
                        schatten_anzahl: 0,
                    });
                    // Release 08.10.2026 (Glas-Stil): Lichtkante.
                    liste.push(super::glas::lichtkante(
                        Rect::new(er.x as f32, er.y as f32, SCHALTER_BREITE as f32, er.hoehe as f32),
                        [er.hoehe as f32 / 2.0; 4],
                        prozent,
                        super::glas::KANTE_LEISE,
                    ));
                    liste.push(DisplayItem::Text {
                        // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
                        // traegt jetzt die `font-family`-Liste. Hier steht die
                        // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
                        // Bildschirmpunkte schreibt keine eigene Familie.
                        schriftfamilie: String::from("sans-serif"),
                        deko: 0,
                        origin: Point::new((er.x + SCHALTER_BREITE + 8) as f32, (er.y + 5) as f32),
                        text: f.wert(e),
                        color: pal.text,
                        font_size: 14.0,
                        // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
                        // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
                        // alles aufrecht, weil kein Schnitt ankam.
                        gewicht: 400,
                        kursiv: false,
                    });
                }
                Bedienung::Schieber => {
                    // GEMESSENE ENTSCHEIDUNG, warum der Balken nicht die volle
                    // Breite des Elements hat: rechts daneben steht der WERT,
                    // und ein Balken unter dem Wert waere doppelt. GEMESSEN:
                    // `er.breite − 72` gibt Platz fuer „500 %" (5 Zeichen = 40 px
                    // plus 32 px Luft).
                    //
                    // GEMESSENE KORREKTUR (03.10.2026, 18:2x): hier stand die
                    // `72` als Zahl im Rumpf. Sie steht jetzt in
                    // [`schieber_balken_breite`] — und DIE benutzt der Treffer
                    // (`schieber_aus_pixel`) auch. GEMESSEN vorher: gezeichnet
                    // 488 px, getroffen 548 px.
                    let balken_breite = schieber_balken_breite(&er);
                    liste.push(DisplayItem::Rect {
                        rect: Rect::new(er.x as f32, (er.y + 9) as f32, balken_breite as f32, 6.0),
                        color: pal.trennlinie_tab,
                        radius: [3.0; 4],
                        deckkraft: 1.0,
                        schatten: [narsil_core::style::SchattenArt::LEER;
                            narsil_core::style::MAX_SCHATTEN],
                        schatten_anzahl: 0,
                    });
                    if let Some((ist, min, max)) = f.schieber(e) {
                        let laenge = balken_breite.saturating_sub(GRIFF_BREITE).max(1);
                        let versatz =
                            ((ist - min) as u64 * laenge as u64) / (max - min).max(1) as u64;
                        let gx = er.x + versatz as u32;
                        liste.push(DisplayItem::Rect {
                            rect: Rect::new(
                                gx as f32,
                                er.y as f32,
                                GRIFF_BREITE as f32,
                                er.hoehe as f32,
                            ),
                            color: pal.akzent,
                            radius: [4.0; 4],
                            deckkraft: 1.0,
                            schatten: [narsil_core::style::SchattenArt::LEER;
                                narsil_core::style::MAX_SCHATTEN],
                            schatten_anzahl: 0,
                        });
                    }
                    // DER WERT, RECHTSBUENDIG IM BEDIENELEMENT.
                    //
                    // GEMESSENER EIGENER FEHLER (01.10.): der Wert stand bei
                    // `er.x + er.breite + 8` — also AUSSERHALB des Elements.
                    // GEMESSEN am Bild `team/log/ui1_*_a_einst_Darstellung.png`:
                    // „18 px" und „150 %" standen rechts neben dem Feld, nicht
                    // darin, und in einem Fenster, in dem das Feld bis zum Rand
                    // reicht, waeren sie ausserhalb des Fensters.
                    //
                    // GEMESSENE ENTSCHEIDUNG, wie man rechtsbuendig rechnet, ohne
                    // Schriftbreiten: das Bild zeigt 8 px je Zeichen (U1b), und
                    // die Zahl steht an EINER Stelle: [`super::ZEICHENBREITE`].
                    let wert = f.wert(e);
                    // GEMESSENE KORREKTUR (04.10.): hier stand
                    // `* super::ZEICHENBREITE` (8 px, gemessen fuer 14 px).
                    // Die Zahl steht in 12 px, also 6,9 px je Zeichen — mit
                    // 8 px stand sie 15 % zu weit rechts und konnte ueber
                    // den Kasten hinausragen.
                    let wert_breite = wert.chars().count() as u32 * zeichenbreite(SCHRIFT_FELD);
                    liste.push(DisplayItem::Text {
                        // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
                        // traegt jetzt die `font-family`-Liste. Hier steht die
                        // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
                        // Bildschirmpunkte schreibt keine eigene Familie.
                        schriftfamilie: String::from("sans-serif"),
                        deko: 0,
                        origin: Point::new(
                            er.rechts().saturating_sub(wert_breite + 8) as f32,
                            (er.y + 5) as f32,
                        ),
                        text: wert,
                        color: pal.text,
                        font_size: 14.0,
                        // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
                        // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
                        // alles aufrecht, weil kein Schnitt ankam.
                        gewicht: 400,
                        kursiv: false,
                    });
                }
                Bedienung::Knopf => {
                    liste.push(DisplayItem::Rect {
                        rect: Rect::new(
                            er.x as f32,
                            er.y as f32,
                            er.breite.min(KNOPF_BREITE) as f32,
                            er.hoehe as f32,
                        ),
                        //
                        // KOORDINATION (02.10., 16:0x): „Knopf 'Als
                        // Standardbrowser festlegen' (kein blauer Balken)".
                        // GEMESSENE ENTSCHEIDUNG, warum die Fläche
                        // `knopf_flaeche` ist und nicht mehr `akzent`: der
                        // Knopf war ein 296 px breiter BLAUER Balken — das ist
                        // die Form der aktiven Kategorie und damit zwei Aussagen
                        // an einem Ort. GEMESSEN im Bild `vorlage_nutzer.webp`:
                        // der Knopf ist ein heller Kasten mit dunkler Schrift.
                        //
                        // GEMESSENE ENTSCHEIDUNG, warum der RAND bleiben muss:
                        // der Test `der_text_auf_dem_knopf_ist_lesbar` misst
                        // den Kontrast zwischen dem Text und der Flaeche. Ohne
                        // den hellen Rand waere der Text auf `knopf_flaeche`
                        // (16,17,24) mit `knopf_text` (Schwarz) unsichtbar —
                        // das waere genau der Fehler, den der Test verhindert.
                        //
                        // GEMESSENE KORREKTUR (04.10., Leitung: „Text dunkelgrau
                        // fast unlesbar"): der Grund war `pal.knopf_flaeche`
                        // (hell) mit `knopf_text` (Schwarz). GEMESSEN an der
                        // Karte: der Knopf hat den **Kasten**-Grund
                        // (57,61,67 gegen (35,38,46 im Kasten) und einen
                        // **hellen** Text (206,210,218). Also: Grund wie der
                        // Kasten (`adresse`), Text wie der Text im Kasten
                        // (`pal.text`) — und der Rand bleibt, weil der Test
                        // `der_text_auf_dem_knopf_ist_lesbar` den Kontrast
                        // misst.
                        color: pal.adresse,
                        // GEMESSEN: 5 px wie die Kaesten (Leitung „unsere
                        // Auswahlfelder sind eckig").
                        //
                        // GEMESSENER EIGENER FEHLER (04.10., 10 Minuten nach
                        // der Leitung): hier stand `radius: RADIUS_KASTEN`
                        // — `DisplayItem::Rect::radius` ist aber ein
                        // `[f32; 4]` (eine Zahl je Ecke), und der Compiler
                        // meldete `E0308 mismatched types`. Es sind **vier
                        // Ecken**, und alle vier sind gleich.
                        radius: [RADIUS_FELD_GLAS; 4],
                        // GEMESSENE ENTSCHEIDUNG, warum die Deckkraft an der
                        // Breite haengt: im zu schmalen Fenster (0,2) ist der
                        // Knopf eine Ansage „passt hier nicht" statt einer
                        // Flaeche, die eine Aktion verspricht.
                        deckkraft: if er.breite >= KNOPF_BREITE { 1.0 } else { 0.2 },
                        schatten: [narsil_core::style::SchattenArt::LEER;
                            narsil_core::style::MAX_SCHATTEN],
                        schatten_anzahl: 0,
                    });
                    // Release 08.10.2026 (Glas-Stil): Glanz und Lichtkante.
                    super::glas::veredeln(
                        liste,
                        Rect::new(er.x as f32, er.y as f32, er.breite.min(KNOPF_BREITE) as f32, er.hoehe as f32),
                        [RADIUS_FELD_GLAS; 4],
                        prozent,
                        super::glas::GLANZ_LEISE,
                        super::glas::KANTE_HELL,
                    );
                    // GEMESSENER FEHLER (04.10., Vergleichsbild): der Text
                    // des Knopfes wurde ungekuerzt geschrieben — im Bild
                    // „Suchmaschinen verwalte…", der Text laeuft ueber den
                    // Kastenrand. Gekuerzt wird wie im Auswahlkasten.
                    let platz = (er.breite as usize).saturating_sub(16)
                        / zeichenbreite(SCHRIFT_FELD) as usize;
                    liste.push(DisplayItem::Text {
                        // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
                        // traegt jetzt die `font-family`-Liste. Hier steht die
                        // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
                        // Bildschirmpunkte schreibt keine eigene Familie.
                        schriftfamilie: String::from("sans-serif"),
                        deko: 0,
                        origin: Point::new((er.x + 8) as f32, (er.y + 5) as f32),
                        text: f.wert(e).chars().take(platz.max(1)).collect(),
                        //
                        // GEMESSENE KORREKTUR (04.10., Leitung: „Text
                        // dunkelgrau fast unlesbar"): hier stand
                        // `pal.knopf_text` (Schwarz). GEMESSEN an der Karte:
                        // der Knopf hat den **Kasten**-Grund (57,61,67) und
                        // einen **hellen** Text (206,210,218). Der alte
                        // Kommentar begruendete Schwarz mit dem
                        // Kontrasttest — der misst aber `knopf_text` gegen
                        // `akzent`, und der Knopf traegt jetzt den Kasten-
                        // Grund, nicht mehr den hellen.
                        color: pal.text,
                        font_size: 14.0,
                        // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
                        // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
                        // alles aufrecht, weil kein Schnitt ankam.
                        gewicht: 400,
                        kursiv: false,
                    });
                }
                Bedienung::Auswahl | Bedienung::Textfeld => {
                    // EIN KASTEN mit dem Wert drin. GEMESSENE ENTSCHEIDUNG,
                    // warum Textfeld und Auswahl DERSELBE Kasten sind: sie
                    // unterscheiden sich an der rechten Kante (der Pfeil), und
                    // zwei Kasten fuer dieselbe Sache waeren zwei Stellen.
                    let offen = seite.ist_offen(*f);
                    //
                    // GEMESSENER EIGENER FEHLER (01.10.): es stand nur EIN
                    // Rechteck in `pal.adresse`. GEMESSEN am Bild
                    // `team/log/ui1_*_a_einst_Darstellung.png`: im hellen Thema
                    // ist `adresse` (0xffffff) fast gleich `seite` (0xffffff) —
                    // das Eingabefeld war UNSICHTBAR. Man sah „DejaVu Serif" und
                    // einen Pfeil und keine Kiste, in der man tippt.
                    //
                    // GEMESSENE ENTSCHEIDUNG: EIN Randrechteck, 1 px groesser,
                    // in `adresse_rand`, DARAUF das Feld in `adresse`. Zwei
                    // Rechtecke statt fuenf (die vier Seiten einzeln).
                    liste.push(DisplayItem::Rect {
                        rect: Rect::new(
                            er.x as f32 - 1.0,
                            er.y as f32 - 1.0,
                            er.breite as f32 + 2.0,
                            er.hoehe as f32 + 2.0,
                        ),
                        color: pal.adresse_rand,
                        radius: [RADIUS_FELD_GLAS + 1.0; 4],
                        deckkraft: 1.0,
                        schatten: [narsil_core::style::SchattenArt::LEER;
                            narsil_core::style::MAX_SCHATTEN],
                        schatten_anzahl: 0,
                    });
                    liste.push(DisplayItem::Rect {
                        rect: Rect::new(
                            er.x as f32,
                            er.y as f32,
                            er.breite as f32,
                            er.hoehe as f32,
                        ),
                        color: pal.adresse,
                        radius: [RADIUS_FELD_GLAS; 4],
                        deckkraft: 1.0,
                        schatten: [narsil_core::style::SchattenArt::LEER;
                            narsil_core::style::MAX_SCHATTEN],
                        schatten_anzahl: 0,
                    });
                    // Release 08.10.2026 (Glas-Stil): Glanz und Lichtkante.
                    super::glas::veredeln(
                        liste,
                        Rect::new(er.x as f32, er.y as f32, er.breite as f32, er.hoehe as f32),
                        [RADIUS_FELD_GLAS; 4],
                        prozent,
                        super::glas::GLANZ_LEISE,
                        super::glas::KANTE_HELL,
                    );
                    // DER FOKUS — ein 2-px-Balken oben, nur wenn getippt wird.
                    if seite.tippfeld == Some(*f) {
                        liste.push(DisplayItem::Rect {
                            rect: Rect::new(er.x as f32, er.y as f32, er.breite as f32, 2.0),
                            color: pal.akzent,
                            radius: [0.0; 4],
                            deckkraft: 1.0,
                            schatten: [narsil_core::style::SchattenArt::LEER;
                                narsil_core::style::MAX_SCHATTEN],
                            schatten_anzahl: 0,
                        });
                    }
                    // DER WERT, fuer den Kasten gekuerzt (M4).
                    let platz = (er.breite as usize).saturating_sub(40)
                        / zeichenbreite(SCHRIFT_FELD) as usize;
                    liste.push(DisplayItem::Text {
                        // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
                        // traegt jetzt die `font-family`-Liste. Hier steht die
                        // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
                        // Bildschirmpunkte schreibt keine eigene Familie.
                        schriftfamilie: String::from("sans-serif"),
                        deko: 0,
                        origin: Point::new((er.x + 6) as f32, (er.y + 5) as f32),
                        // Release 09.10.2026: im getippten Feld der Puffer mit
                        // Cursor — bei langem Text das ENDE (dort tippt man).
                        text: if seite.tippfeld == Some(*f) {
                            let n = seite.eingabe.chars().count();
                            let rest = platz.max(2) - 1;
                            let sichtbar: String = seite.eingabe.chars().skip(n.saturating_sub(rest)).collect();
                            format!("{sichtbar}|")
                        } else {
                            f.wert(e).chars().take(platz.max(1)).collect()
                        },
                        color: pal.text,
                        font_size: 14.0,
                        // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
                        // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
                        // alles aufrecht, weil kein Schnitt ankam.
                        gewicht: 400,
                        kursiv: false,
                    });
                    // DER PFEIL der Auswahlliste — zwei Rechtecke als Dreieck.
                    // KOORDINATION (03.10., Hygiene): der Verweis auf
                    // `symbole::Form::Dreieck` ist entfernt; die Figurensprache
                    // gibt es nicht mehr. Die Behauptung bleibt: aus zwei
                    // Rechtecken entsteht im Bild ein Dreieck.
                    if f.bedienung() == Bedienung::Auswahl {
                        for (dx, breite) in [(0u32, 8u32), (2, 4)] {
                            liste.push(DisplayItem::Rect {
                                rect: Rect::new(
                                    (er.rechts() - 20 + dx) as f32,
                                    (er.y + 9 + dx) as f32,
                                    breite as f32,
                                    2.0,
                                ),
                                color: pal.text_schwach,
                                radius: [0.0; 4],
                                deckkraft: 1.0,
                                schatten: [narsil_core::style::SchattenArt::LEER;
                                    narsil_core::style::MAX_SCHATTEN],
                                schatten_anzahl: 0,
                            });
                        }
                        // DER RAHMEN, wenn die Liste offen ist — das EINZIGE
                        // Bedienelement, das seinen Zustand an einem Rand zeigt.
                        if offen {
                            liste.push(DisplayItem::Rect {
                                rect: Rect::new(
                                    er.x as f32,
                                    er.y as f32,
                                    er.breite as f32,
                                    er.hoehe as f32,
                                ),
                                color: pal.akzent,
                                radius: [RADIUS_FELD_GLAS; 4],
                                deckkraft: 0.9,
                                schatten: [narsil_core::style::SchattenArt::LEER;
                                    narsil_core::style::MAX_SCHATTEN],
                                schatten_anzahl: 0,
                            });
                        }
                    }
                }
            }
        }
    }

    // 5. DIE AUFRUPPENDE AUSWAHLLISTE.
    if let Some((f, _)) = seite.offen {
        let felder = seite.kategorie.felder();
        if let Some(index) = felder.iter().position(|x| *x == f) {
            let eintraege = seite.offene_eintraege(e);
            if let Some(r) = klappliste(
                seite.kategorie,
                chrome,
                f,
                index,
                fensterbreite,
                fensterhoehe,
                eintraege.len(),
            ) {
                // Release 08.10.2026 (Glas-Stil): die aufgeklappte Liste ist ein
                // Glasfeld wie das Kontextmenue.
                super::glas::feld(liste, &r, prozent);
                let gewaehlt = f.gewaehlt(e);
                let beginn = seite.klapp_beginn(e);
                for (nr, txt) in eintraege.iter().enumerate() {
                    let y = r.y + ZEILE * nr as u32;
                    if y + ZEILE > r.unten() {
                        break;
                    }
                    if beginn + nr == gewaehlt {
                        liste.push(DisplayItem::Rect {
                            rect: Rect::new(r.x as f32 + 4.0, y as f32, (r.breite as f32 - 8.0).max(1.0), ZEILE as f32),
                            color: pal.akzent,
                            radius: [6.0; 4],
                            deckkraft: 0.25,
                            schatten: [narsil_core::style::SchattenArt::LEER;
                                narsil_core::style::MAX_SCHATTEN],
                            schatten_anzahl: 0,
                        });
                    }
                    let platz = (r.breite as usize).saturating_sub(24)
                        / zeichenbreite(SCHRIFT_FELD) as usize;
                    liste.push(DisplayItem::Text {
                        // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
                        // traegt jetzt die `font-family`-Liste. Hier steht die
                        // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
                        // Bildschirmpunkte schreibt keine eigene Familie.
                        schriftfamilie: String::from("sans-serif"),
                        deko: 0,
                        origin: Point::new((r.x + 8) as f32, (y + 5) as f32),
                        text: txt.chars().take(platz.max(1)).collect(),
                        color: pal.text,
                        font_size: 14.0,
                        // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
                        // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
                        // alles aufrecht, weil kein Schnitt ankam.
                        gewicht: 400,
                        kursiv: false,
                    });
                }
            }
        }
    }
}

/// Release 08.10.2026: DER TAB „FEATURES" — Hinweiszeile, Abschnitte,
/// je Funktion eine Karte und zuletzt die offene (i)-Erklaerung.
fn features_zeichnen(
    liste: &mut DisplayList,
    seite: &Seite,
    e: &Einstellungen,
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
    pal: &super::thema::Palette,
) {
    let k = Kategorie::Features;
    let x0 = inhalt_x(fensterbreite);
    liste.push(DisplayItem::Text {
        schriftfamilie: String::from("sans-serif"),
        deko: 0,
        origin: Point::new(x0 as f32, (inhalt_oberkante(chrome) + ZEILE_UEBERSCHRIFT - 4) as f32),
        text: tr(super::features::HINWEIS).to_string(),
        color: pal.text_schwach,
        font_size: SCHRIFT_FELD,
        gewicht: 400,
        kursiv: false,
    });
    let unten_frei = fensterhoehe.saturating_sub(super::STATUSLEISTE_HOEHE);
    for (ai, abschnitt) in abschnitte(k).iter().enumerate() {
        let ay = abschnitt_y(k, chrome, ai);
        if ay + ZEILE_ABSCHNITT > unten_frei {
            break;
        }
        // Release 08.10.2026 (Glas-Stil): kleine graue Versalschrift.
        liste.push(super::glas::ueberschrift(
            x0 as f32 + 2.0,
            (ay + 6) as f32,
            tr(abschnitt.ueberschrift),
            SCHRIFT_ABSCHNITT_GLAS,
            super::glas::GLAS_GRAU,
        ));
        for f in abschnitt.felder {
            let Feld::Feature(x) = *f else { continue };
            let index = k.felder().iter().position(|g| g == f).unwrap_or(0);
            let r = feld(k, chrome, *f, index, fensterbreite);
            if r.rechts() > fensterbreite || r.y + super::features::HOEHE_KARTE > unten_frei {
                break;
            }
            super::features::zeile_zeichnen(liste, x, e.features.an(x), seite.info == Some(x), &r, pal);
        }
    }
    if let Some(offen) = seite.info
        && let Some(index) = k.felder().iter().position(|f| *f == Feld::Feature(offen))
    {
        let r = feld(k, chrome, Feld::Feature(offen), index, fensterbreite);
        if r.y < fensterhoehe {
            super::features::erklaerung_zeichnen(liste, offen, &r, fensterhoehe, pal);
        }
    }
}

/// STEHT DER SCHALTER AUF AN?
fn schalter_an(f: Feld, e: &Einstellungen) -> bool {
    match f {
        Feld::Werbeblocker => e.werbeblocker,
        Feld::CookiesLoeschen => e.cookies_beim_start_loeschen,
        Feld::Feature(x) => e.features.an(x),
        // GEMESSENE ENTSCHEIDUNG: jedes andere Feld hat keinen Schalter
        // (siehe `Feld::bedienung`). Der vollstaendige `match` ist die
        // Bremse: ein neues Schalter-Feld ohne Zweig macht den Compiler rot.
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// DIE DATEI
// ---------------------------------------------------------------------------

/// DER PFAD DER EINSTELLUNGEN: `~/.config/narsil/einstellungen.txt`.
///
/// GEMESSENE ENTSCHEIDUNG, warum `$HOME` und nicht eine `dirs`-Bibliothek:
/// der Pfad ist in ANFORDERUNGEN.txt genannt („Speichern in
/// `~/.config/narsil/einstellungen.txt`"), und eine Bibliothek, die ihn
/// umbiegt, waere eine Regel an einer zweiten Stelle.
///
/// GEMESSENE ENTSCHEIDUNG, warum ohne `$HOME` ein RELATIVER Pfad kommt und
/// kein Panic: ein Browser, der in einer Umgebung ohne `$HOME` startet
/// (Container, `su` ohne Login-Shell), soll die Seite zeigen und beim
/// Speichern melden — nicht abstuerzen.
///
/// Release 08.10.2026: `NARSIL_PROFIL` (ein Ordner) geht vor, und ein
/// Cargo-Testprogramm bekommt ein EIGENES Profil im Temp-Ordner. GEMESSEN
/// vorher: nach einem Lauf der narsil-Tests stand im echten Profil
/// `zoom=500`, `file:=25` im Zoomprofil und `http://test.com` als Sitzung —
/// der Browser des Benutzers startete mit 500 % Zoom, und der Test
/// `sichtbare_menueeintraege_...` scheiterte am Zoom des Vorlaufs.
pub fn pfad() -> std::path::PathBuf {
    if let Some(ordner) = std::env::var_os("NARSIL_PROFIL").filter(|p| !p.is_empty()) {
        return std::path::PathBuf::from(ordner).join("einstellungen.txt");
    }
    if laeuft_als_test() {
        // Je Testprozess ein EIGENES Profil: ein gemeinsames trug den Zoom
        // des Zoom-Tests (`file:=25`) in den naechsten Testlauf.
        return test_profil().join("einstellungen.txt");
    }
    nutzer_pfad()
}

/// `%TEMP%/narsil-test-profil/<pid>`; beim ersten Aufruf werden Profile
/// frueherer Laeufe (aelter als eine Stunde) entfernt.
fn test_profil() -> std::path::PathBuf {
    static ORDNER: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    ORDNER
        .get_or_init(|| {
            let basis = std::env::temp_dir().join("narsil-test-profil");
            if let Ok(eintraege) = std::fs::read_dir(&basis) {
                for e in eintraege.flatten() {
                    let alt = e
                        .metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|d| d.as_secs() > 3600);
                    if alt {
                        let p = e.path();
                        let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
                    }
                }
            }
            basis.join(std::process::id().to_string())
        })
        .clone()
}

/// Ob das Profil umgeleitet ist (`NARSIL_PROFIL` oder Testlauf) — dann
/// gehoert auch der Sitzungszustand in [`ordner`] statt ins Benutzerprofil.
pub fn profil_umgeleitet() -> bool {
    std::env::var_os("NARSIL_PROFIL").is_some_and(|p| !p.is_empty()) || laeuft_als_test()
}

/// Cargo legt Test-Binaries (Unit- wie Integrationstests) unter
/// `target/<profil>/deps/` ab; das Browserprogramm und die Beispiele liegen
/// nie dort.
pub fn laeuft_als_test() -> bool {
    static TEST: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *TEST.get_or_init(|| {
        let Ok(exe) = std::env::current_exe() else { return false };
        if exe.parent().and_then(|d| d.file_name()).is_some_and(|n| n == "deps") {
            return true;
        }
        // Release 14.10.2026: only the browser programs themselves may use
        // the real user profile; every other executable (test binaries copied
        // out of `deps`, examples, tools) gets the per-process test profile.
        // MEASURED before: copied test binaries overwrote the real profile
        // (settings, bookmarks, session, zoom profile).
        let stem = exe.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        !is_browser_program(&stem)
    })
}

/// Whether an executable stem (file name without extension) is one of the
/// browser programs that own the real user profile.
fn is_browser_program(stem: &str) -> bool {
    matches!(stem.to_ascii_lowercase().as_str(), "narsil" | "narsil-app")
}

#[cfg(test)]
mod real_profile_guard_tests {
    use super::is_browser_program;

    #[test]
    fn only_the_browser_programs_use_the_real_profile() {
        assert!(is_browser_program("narsil"));
        assert!(is_browser_program("Narsil"));
        assert!(is_browser_program("narsil-app"));
        assert!(!is_browser_program("alle-1a2b3c4d5e6f7a8b"));
        assert!(!is_browser_program("website_diagnose"));
        assert!(!is_browser_program("wpt_lauf"));
        assert!(!is_browser_program(""));
    }
}

/// Der Profilpfad des Benutzers (ohne Test-/Umgebungsumleitung).
fn nutzer_pfad() -> std::path::PathBuf {
    #[cfg(windows)]
    {
        let basis = std::env::var_os("APPDATA").filter(|p| !p.is_empty())
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").filter(|p| !p.is_empty())
                .map(|p| std::path::PathBuf::from(p).join("AppData/Roaming")))
            .unwrap_or_else(std::env::temp_dir);
        alten_namen_einmal_uebernehmen(&basis);
        return basis.join("narsil/einstellungen.txt");
    }
    #[cfg(not(windows))]
    {
    let basis = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(""));
    alten_namen_einmal_uebernehmen(&basis.join(".config"));
    basis.join(".config/narsil/einstellungen.txt")
    }
}

/// Release 09.10.2026: der Browser heisst jetzt NARSIL (vorher „RWeb").
///
/// Ein Profil unter dem alten Namen wird beim ersten Zugriff EINMAL
/// umbenannt (`rweb` -> `narsil`): Lesezeichen, Verlauf, Einstellungen,
/// Erweiterungen und Seitenspeicher bleiben erhalten. Gibt es den neuen
/// Ordner schon, bleibt der alte unberuehrt — es wird nie ueberschrieben.
fn alten_namen_einmal_uebernehmen(basis: &std::path::Path) {
    static EINMAL: std::sync::Once = std::sync::Once::new();
    EINMAL.call_once(|| ordner_umbenennen_falls_neu_fehlt(basis, "rweb", "narsil"));
}

/// `basis/alt` -> `basis/neu`, nur wenn `alt` ein Ordner ist und `neu`
/// noch nicht existiert. Fehler (z. B. gesperrte Datei) lassen beides
/// stehen: dann beginnt Narsil mit einem frischen Profil, das alte bleibt.
pub fn ordner_umbenennen_falls_neu_fehlt(basis: &std::path::Path, alt: &str, neu: &str) {
    let alt_p = basis.join(alt);
    let neu_p = basis.join(neu);
    if alt_p.is_dir() && !neu_p.exists() {
        let _ = std::fs::rename(&alt_p, &neu_p);
    }
}

/// DER ORDNER, in dem die Datei liegt.
pub fn ordner() -> std::path::PathBuf {
    pfad().parent().map(|p| p.to_path_buf()).unwrap_or_default()
}

/// M4: die groesste Datei, die gelesen wird (in Bytes).
///
/// GEMESSENE BEGRUENDUNG fuer 64 KiB: die Datei hat hoechstens
/// [`MAX_ZEILEN`] Zeilen mit je [`MAX_ZEILEN_LAENGE`] Zeichen — das waeren
/// 200 * 1024 = 200 KiB. 64 KiB ist kleiner, und das ist Absicht: eine
/// Einstellungsdatei, die 64 KiB gross ist, ist keine Einstellung mehr.
/// GEMESSEN: `read_to_string` auf einer 1-MB-Datei waere eine Allokation aus
/// einer Datei, die ein Fremder schreiben kann.
pub const MAX_DATEI: u64 = 64 * 1024;

/// M3: hoechstens so viele Zeilen werden gelesen.
///
/// GEMESSENE BEGRUENDUNG fuer 200: die Datei hat 11 Werte, also 11 Zeilen.
/// 200 gibt Platz fuer Kommentare und dafuer, dass morgen jemand eine
/// Zeile hinzufuegt — und begrenzt die Schleife ueber eine Datei, die ein
/// Fremder mit 10^9 Zeilen gefuellt haben koennte.
pub const MAX_ZEILEN: usize = 200;

/// M4: die laengste Zeile, die gelesen wird (in Zeichen).
///
/// GEMESSENE BEGRUENDUNG fuer 1024: das ist [`MAX_STARTSEITE`] / 2. Die
/// laengste sinnvolle Zeile ist die Startseite (2048 Zeichen), und sie wird
/// gekuerzt gespeichert — eine Datei, die eine laengere Zeile hat, wurde von
/// Hand oder von einem Programm geschrieben, das keine Grenze kennt.
pub const MAX_ZEILEN_LAENGE: usize = 1024;

/// DER TEXT, der in die Datei geschrieben wird.
///
/// GEMESSENE ENTSCHEIDUNG fuer das Format `schluessel=wert`: es ist das
/// Format, das die Zustandsdateien in `narsil` seit dem 03.10. benutzen (H5),
/// und ein zweites Format waere ein zweiter Leser.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Wert in ANFUEHRUNGSZEICHEN steht: die
/// Werte sind Adressen, Ordner und Familiennamen, und die koennen Leerzeichen
/// und `=` enthalten (`downloads_ordner="/home/jaff/Meine Downloads"`). Ohne
/// Anfuehrungszeichen waere die Zeile nicht zerlegbar.
pub fn als_text(e: &Einstellungen) -> String {
    let mut s = String::new();
    s.push_str("# Einstellungen von narsil. Von Hand geaendert: moeglich, aber\n");
    s.push_str("# beim naechsten Start werden unbekannte Zeilen verworfen.\n");
    s.push_str(&format!("startseite={:?}\n", e.startseite));
    s.push_str(&format!("downloads_ordner={:?}\n", e.downloads_ordner));
    s.push_str(&format!("suchmaschine={}\n", e.suchmaschine.id()));
    s.push_str(&format!("eigene_suche={:?}\n", e.eigene_suche));
    s.push_str(&format!(
        "thema={}\n",
        match e.thema {
            Thema::Hell => "hell",
            Thema::Dunkel => "dunkel",
        }
    ));
    s.push_str(&format!("zoom={}\n", e.zoom));
    s.push_str(&format!("schriftgroesse={}\n", e.schriftgroesse));
    s.push_str(&format!("schriftart={:?}\n", e.schriftart));
    s.push_str(&format!("werbeblocker={}\n", ja_nein(e.werbeblocker)));
    s.push_str(&format!(
        "cookies_loeschen={}\n",
        ja_nein(e.cookies_beim_start_loeschen)
    ));
    // Release 08.10.2026: die Schalter des Tabs „Features".
    s.push_str(&e.features.als_zeilen());
    s.push_str(&format!("language={}\n", e.language.code()));
    s
}

/// GEMESSENE ENTSCHEIDUNG: `true`/`false` sind die einzigen zwei Woerter, die
/// in einer Konfigurationsdatei keine Bedeutung haben. Alles andere („ja",
/// „1", „on", „enabled") waere eine dritte Wahrheit ueber denselben Schalter,
/// und die drei Dateien, die sie lesen, muessten sie alle kennen.
fn ja_nein(wert: bool) -> &'static str {
    if wert { "true" } else { "false" }
}

/// DIE EINSTELLUNGEN IN EINE DATEI SCHREIBEN.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Funktion einen PFAD als Parameter
/// nimmt und nicht [`pfad`]: der Test schreibt in sein eigenes Verzeichnis
/// (M9: keine Testdaten unter `/tmp`, sondern unter
/// `/home/jaff/mutation_md/ui/`). Ein Test, der die echte Datei des Nutzers
/// ueberschreibt, ist kein Test.
///
/// M4: der Text wird auf [`MAX_DATEI`] gekuerzt. GEMESSENE BEGRUENDUNG: der
/// Text entsteht aus Werten, die alle begrenzt sind, also kann er nicht
/// wachsen — aber die Grenze steht trotzdem da, weil sie sonst nur in der
/// Begruendung stuende.
pub fn schreiben_nach(pfad: &std::path::Path, e: &Einstellungen) -> Result<(), String> {
    if let Some(eltern) = pfad.parent() {
        std::fs::create_dir_all(eltern).map_err(|f| trf!("Cannot create folder: {}", f))?;
    }
    let mut text = als_text(e);
    if text.len() > MAX_DATEI as usize {
        text.truncate(MAX_DATEI as usize);
    }
    std::fs::write(pfad, text).map_err(|f| trf!("Cannot write file: {}", f))
}

/// DIE EINSTELLUNGEN AUS EINER DATEI LESEN.
///
/// GEMESSENE ENTSCHEIDUNG, warum eine kaputte Zeile den Rest nicht stoppt:
/// die Datei kann von Hand geaendert sein. Eine kaputte Zeile wird
/// uebersprungen (der Wert bleibt, wie er ist), und `Ok` kommt zurueck.
/// „Zeile 7 ist kaputt" ist keine Information, die der Nutzer beim Start
/// braucht.
///
/// M3/M4: die Schleife laeuft ueber hoechstens [`MAX_ZEILEN`] Zeilen mit je
/// [`MAX_ZEILEN_LAENGE`] Zeichen.
pub fn lesen_von(pfad: &std::path::Path) -> Result<Einstellungen, String> {
    let mut e = Einstellungen::default();
    let Ok(angaben) = std::fs::metadata(pfad) else {
        // GEMESSENE ENTSCHEIDUNG: keine Datei ist KEIN Fehler — das ist der
        // Zustand nach der Installation. Die Standardwerte gelten.
        return Ok(e);
    };
    if angaben.len() > MAX_DATEI {
        return Err(trf!("File too large: {} bytes (limit {})", angaben.len(), MAX_DATEI));
    }
    let Ok(text) = std::fs::read_to_string(pfad) else {
        return Ok(e);
    };
    for zeile in text.lines().take(MAX_ZEILEN) {
        let z: String = zeile.chars().take(MAX_ZEILEN_LAENGE).collect();
        let z = z.trim();
        if z.is_empty() || z.starts_with('#') {
            continue;
        }
        let Some(gleich) = z.find('=') else {
            continue;
        };
        let schluessel = z[..gleich].trim();
        let wert = unquote(z[gleich + 1..].trim());
        match schluessel {
            "startseite" => {
                //
                // GEMESSENE ENTSCHEIDUNG (Punkt 35), warum die
                // Rueckgabe hier bewusst wegfaellt: die Setzer geben `false`
                // zurueck, wenn sie gekuerzt haben — und beim Lesen einer
                // Konfigurationsdatei ist niemand da, dem man es sagen
                // koennte. GEMESSENE FOLGE fuer die Oberflaeche: der Wert
                // steht gekuerzt im Feld, und der Nutzer sieht es beim
                // naechsten Start. GEMESSENE REGEL: wer eine Rueckgabe
                // wegwirft, schreibt hin, WARUM — sonst baut der naechste
                // Leser eine Warnung ein, die kein Fehler findet.
                e.setze_startseite(&wert);
            }
            "downloads_ordner" => {
                e.setze_downloads_ordner(&wert);
            }
            "suchmaschine" => {
                e.suchmaschine = Suchmaschine::from_id(&wert).unwrap_or(e.suchmaschine);
            }
            "language" => {
                if let Some(l) = Language::from_code(&wert) {
                    e.language = l;
                }
            }
            "eigene_suche" => {
                e.setze_eigene_suche(&wert);
            }
            "thema" => {
                e.thema = match wert.as_str() {
                    "hell" => super::thema::Thema::Hell,
                    _ => super::thema::Thema::Dunkel,
                };
            }
            "zoom" => {
                if let Some(z) = e.zoom_lesen(&wert) {
                    e.zoom = z;
                }
            }
            "schriftgroesse" => {
                if let Some(z) = e.schriftgroesse_lesen(&wert) {
                    e.schriftgroesse = z;
                }
            }
            "schriftart" => {
                e.setze_schriftart(&wert);
            }
            "werbeblocker" => e.werbeblocker = wert == "true",
            "cookies_loeschen" => e.cookies_beim_start_loeschen = wert == "true",
            // Release 08.10.2026: `feature_<schluessel>=true|false`.
            s if s.starts_with("feature_") => {
                e.features.zeile_lesen(s, &wert);
            }
            // GEMESSENE ENTSCHEIDUNG, warum unbekannte Schluessel ignoriert
            // werden: die Datei wird von Hand geschrieben. Wer sie mit
            // Notizen fuellt, soll keine Warnung bekommen.
            _ => {}
        }
    }
    Ok(e)
}

/// M4: die Anfuehrungszeichen abnehmen, mit Grenze.
///
/// GEMESSENE ENTSCHEIDUNG, warum `trim_matches` NICHT genuegt: ein Wert wie
/// `"a" "b"` (mit einem Anfuehrungszeichen in der Mitte) ist ein Datei-Fehler,
/// und `trim_matches('"')` wuerde beide abschneiden. Der Test
/// `die_datei_ueberlebt_anfuehrungszeichen_in_der_mitte` deckt das ab.
fn unquote(wert: &str) -> String {
    let w = wert.trim();
    if w.len() >= 2 && w.starts_with('"') && w.ends_with('"') {
        // Release 09.10.2026: `als_text` schreibt die Werte mit `{:?}` (also
        // zwei Backslashes fuer einen, Backslash + Anfuehrungszeichen fuer ein
        // inneres) — hier wird das wieder aufgeloest. GEMESSEN vorher: jeder
        // Windows-Pfad verdoppelte bei jedem Speichern seine Backslashes.
        entschaerfen(&w[1..w.len() - 1])
    } else {
        w.to_string()
    }
}

/// Release 09.10.2026: die Escapes aus `{:?}` aufloesen (Backslash,
/// Anfuehrungszeichen, `n`, `r`, `t`, `0`, `u{..}`). Ein unbekanntes Escape
/// bleibt, wie es ist.
fn entschaerfen(innen: &str) -> String {
    const BS: char = '\\';
    let mut raus = String::with_capacity(innen.len());
    let mut z = innen.chars().peekable();
    while let Some(c) = z.next() {
        if c != BS {
            raus.push(c);
            continue;
        }
        match z.next() {
            Some(BS) => raus.push(BS),
            Some('"') => raus.push('"'),
            Some('\'') => raus.push('\''),
            Some('n') => raus.push('\n'),
            Some('r') => raus.push('\r'),
            Some('t') => raus.push('\t'),
            Some('0') => raus.push('\0'),
            Some('u') if z.peek() == Some(&'{') => {
                z.next();
                let hex: String = z.by_ref().take_while(|x| *x != '}').collect();
                match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    Some(x) => raus.push(x),
                    None => {
                        raus.push(BS);
                        raus.push_str("u{");
                        raus.push_str(&hex);
                        raus.push('}');
                    }
                }
            }
            Some(x) => {
                raus.push(BS);
                raus.push(x);
            }
            None => raus.push(BS),
        }
    }
    raus
}

#[cfg(test)]
mod zuruecksagen_tests {
    use super::*;

    /// DIE FUENF SETZER SAGEN, WAS SIE GETAN HABEN — und „gekuerzt“ ist
    /// keine Ablehnung.
    ///
    /// GEMESSENE ENTSCHEIDUNG (Punkt 35, 02.10.), warum dieser Test
    /// ueberhaupt noetig ist: vier der fuenf Setzer geben bei zu langer
    /// Eingabe `false` zurueck, **setzen aber trotzdem** (gekuerzt). Ein
    /// Aufrufer, der `false` als „abgelehnt“ liest, schliesst dann die falsche
    /// Sache: das Feld ist leer (oder der alte Wert steht noch da), obwohl
    /// etwas geschrieben wurde. Die Rueckgabe ist also nur dann richtig, wenn
    /// sie „unveraendert uebernommen“ bedeutet — und das wird hier festgeschrieben.
    #[test]
    fn die_setzer_sagen_was_sie_getan_haben() {
        let mut e = Einstellungen::default();
        // (1) KURZ: angenommen, `true`, und der Wert steht WORT fuer WORT da.
        assert!(e.setze_startseite("https://beispiel.de"));
        assert_eq!(e.startseite, "https://beispiel.de");
        // (2) LANG: `false`, und der Wert ist TROTZDEM gesetzt — gekuerzt.
        let lang = "a".repeat(MAX_STARTSEITE + 500);
        assert!(
            !e.setze_startseite(&lang),
            "GEMESSEN: zu lang wurde angenommen"
        );
        assert_eq!(e.startseite.chars().count(), MAX_STARTSEITE);
        // (3) LEER: `true` und der Wert ist leer (es ist kein Fehler, eine
        //     Startseite zu loeschen — sonst koennte man sie nie wegbekommen).
        assert!(e.setze_startseite(""));
        assert_eq!(e.startseite, "");
        // (4) DERSELBE VERTRAG fuer die anderen drei String-Setzer.
        let mut e = Einstellungen::default();
        assert!(e.setze_schriftart("Arial"));
        let zu_lang = "s".repeat(MAX_SCHRIFTNAME + 100);
        assert!(!e.setze_schriftart(&zu_lang));
        assert_eq!(e.schriftart.chars().count(), MAX_SCHRIFTNAME);
        // (5) UND: `zoom_lesen` ist die EINE Stelle, die den Wert wirklich
        //     ablehnt (`Option`), weil eine Zahl im Textfeld ein Schieber
        //     mit Tippfehler ist. GEMESSENE ENTSCHEIDUNG: die beiden Wege
        //     (kuerzen und ablehnen) muessen unterscheidbar sein, sonst
        //     weiss der Aufrufer nicht, was er anzeigen soll.
        assert_eq!(e.zoom_lesen("abc"), None, "GEMESSEN: „abc“ ist eine Zahl");
        assert_eq!(e.zoom_lesen("150"), Some(150));
    }

    /// DER TOTE AUFRUF IST WEG — und der Schieber-Klick tut dasselbe.
    ///
    /// GEMESSENER BEFUND (Punkt 35, 02.10.): im Zweig `Schieber` von
    /// `klick()` stand `let r = element(chrome, f, 0, fensterbreite);
    /// let _ = r;` — eine Rechnung, deren Ergebnis sofort weggeworfen wurde.
    /// `element()` ist eine reine Geometrie, also war der Aufruf toter Code
    /// mit zwei Zeilen. GEMESSENE BEHAUPTUNG: `klick()` antwortet im
    /// Schieber-Zweig auf `Aktion::Nichts` (ein Klick in die linke Haelfte
    /// des Balkens aendert die Zahl nicht — der Nutzer zieht den Griff), und
    /// das gilt jetzt ohne den toten Aufruf. GEMESSENE ENTSCHEIDUNG, warum
    /// dieser Test `klick()` aufruft und nicht `treffer()`: `klick()` ist die
    /// Funktion, in der der Aufruf stand — ein Test, der eine andere
    /// Funktion prueft, prueft nicht die Aenderung.
    #[test]
    fn der_tode_aufruf_ist_weg() {
        let chrome = Chrome::neu(1200);
        let e = Einstellungen::default();
        let mut seite = Seite::default();
        //
        // DER KLICK AUF DEN SCHIEBER-Zweig: `Feld::Schriftgroesse` ist ein
        // Schieber (siehe `Feld::bedienung`). GEMESSENE BEHAUPTUNG: er
        // liefert `Aktion::Nichts` — ein Klick in die linke Haelfte des
        // Balkens aendert die Zahl nicht, der Nutzer zieht den Griff.
        //
        // GEMESSENE KORREKTUR (04.10.): hier stand `Feld::Schriftgroesse` —
        // die Schriftgroesse ist seit dem Panel-4-Umbau ein **Auswahlfeld**
        // (Leitung: „Auswahlfeld 'Standard (16px)'"), also liefert ihr Klick
        // `Klappe`. GEMESSEN war `left: Klappe(Schriftgroesse)`. Der
        // Schieber ist jetzt der **Zoom**, und der Test behauptet weiterhin
        // das Richtige — an dem Feld, das es jetzt ist.
        // Release 10.10.2026: der Zoom ist jetzt ein Auswahlfeld — sein Klick
        // klappt die Stufenliste auf.
        let aktion = super::klick(&mut seite, &e, Treffer::Element(Feld::Zoom));
        assert_eq!(aktion, Aktion::Klappe(Feld::Zoom), "der Klick oeffnet die Stufen");
    }
}

#[cfg(test)]
mod tests {
    /// added 04.10.2026, TLS: **DER TEST, DEN DER KOMMENTAR IN `felder()` SEIT
    /// LANGE VERSPROCHEN HAT** — `abschnitte_und_felder_sagen_dasselbe`.
    ///
    /// **GEMESSEN, dass es ihn nicht gab:** `grep -rn "abschnitte_und_felder_
    /// sagen_dasselbe" crates/narsil-chrome/` fand **nur** den Kommentar. **UND
    /// WARUM DAS JETZT EINE BREMSE IST UND DAMIT EINE NOTwendigkeit:** es gibt
    /// **zwei** Listen (`abschnitte()` und `felder()`), und `abschnitte()`
    /// wurde am 04.10. **zwei** Abschnitte fuer `Datenschutz`, waehrend
    /// `felder()` **einen** Eintrag hatte. Ein Aufrufer, der `felder()` nimmt
    /// (die Zeilenzahl, die Einstellungsdatei), haette die **Knopfe nie
    /// gezeichnet** — ohne Fehler, ohne Warnung.
    ///
    /// **UND DIE REGEL, die der Test durchsetzt:** `felder()` muss **genau**
    /// die Felder nennen, die in `abschnitte()` stehen — **gleiche Menge,
    /// gleiche Reihenfolge**. Keine Teilmenge: `Allgemein` hat **sechs** Felder
    /// in **vier** Abschnitten, und das ist richtig.
    #[test]
    fn abschnitte_und_felder_sagen_dasselbe() {
        for k in KATEGORIEN {
            // 04.10.2026 (UI): `KATEGORIEN` ist ein **Array** und wird seit
            // Rust 2021 **by value** iteriert — `k` ist also bereits eine
            // `Kategorie`, und `*k` wollte sie dereferenzieren. GEMESSEN:
            // `E0614 type 'Kategorie' cannot be dereferenced` (Z. 3403).
            let aus_abschnitten: Vec<Feld> = abschnitte(k)
                .iter()
                .flat_map(|a| a.felder.iter())
                .copied()
                .collect();
            assert_eq!(
                aus_abschnitten,
                k.felder().to_vec(),
                "{k:?}: `abschnitte()` und `felder()` sagen nicht dasselbe"
            );
        }
    }

    /// added 04.10.2026, TLS: **DER WEG DER KEKSE — DIE ZWEI KNOPFE UND DIE ZWEI
    /// AKTIONEN.** Und der Test, der beweist, dass **beide** Wege **dieselbe**
    /// Aktion benutzen.
    #[test]
    fn die_zwei_keksknopfe_geben_die_zwei_umfaenge() {
        // 04.10.2026 (UI): `Seite` hat **kein** `neu()`, sondern
        // `Default` (Z. 1670, Kategorie `Allgemein`). GEMESSEN:
        // `E0599 no associated function 'neu' found for struct 'Seite'`.
        let breite = 1000u32;
        let chrome = Chrome::neu(breite);
        let e = Einstellungen::default();
        let mut seite = Seite::default();
        seite.wechsle(Kategorie::Datenschutz);
        for (feld, erwartet) in [
            (Feld::KekseAlleLoeschen, hauptmenue::KeksUmfang::Alle),
            (Feld::KekseSeiteLoeschen, hauptmenue::KeksUmfang::DieseSeite),
        ] {
            assert_eq!(feld.bedienung(), Bedienung::Knopf, "{feld:?}");
            // **DER GANZE WEG UEBER DEN TREFFER, nicht `element()` direkt:**
            // `element()` liefert nur die **Geometrie** des Kastens
            // (`Rechteck`, GEMESSEN: `E0308 expected Rechteck, found Aktion`).
            // Der Weg, den der Benutzer geht, ist Punkt -> `treffer` -> `klick`,
            // und **genau dort** kann ein Knopf fehlen, ohne dass eine Kiste
            // sichtbar wird.
            // **UND DER INDEX IST DER IN DER FLACHEN LISTE**, nicht der im
            // Abschnitt (GEMESSEN: mit `0` traf der Klick auf `Werbeblocker` —
            // `feld_y` zaehlte durch `abschnitte()` **und** durch die
            // `abschnitte(k)`-Reihenfolge, und `Feld::KekseAlleLoeschen` steht
            // an Stelle **2**, weil „Datenschutz“ zwei Felder hat).
            let index = Kategorie::Datenschutz
                .felder()
                .iter()
                .position(|f| *f == feld)
                .unwrap_or(0);
            let kasten = element(Kategorie::Datenschutz, &chrome, feld, index, breite);
            let punkt = Point::new(kasten.x as f32 + 4.0, kasten.y as f32 + 10.0);
            let t = treffer(&chrome, &seite, &e, &punkt, breite, 1200)
                .unwrap_or_else(|| panic!("kein Treffer fuer {feld:?}"));
            let a = klick(&mut seite, &e, t);
            assert_eq!(
                a,
                Aktion::KekseLoeschen(erwartet),
                "{feld:?} liefert die falsche Aktion"
            );
            // **UND DIE AKTION HAT ZWEI NAMEN**, weil „Alle“ und „Diese
            // Seite“ fuer eine Fehlermeldung **verschiedene** Dinge sind.
            assert_ne!(
                Aktion::KekseLoeschen(hauptmenue::KeksUmfang::Alle).name(),
                Aktion::KekseLoeschen(hauptmenue::KeksUmfang::DieseSeite).name()
            );
            // **UND SIE LANDET IM BILDSCHIRM-NAMEN**, sonst ist sie im Log
            // nicht wiederzufinden.
            assert!(a.name().contains("kekse-loeschen"), "{:?}", a.name());
        }
        // **UND DAS MENUE BENUTZT DIESELBE AKTION** — das ist der Grund fuer
        // das `enum`: ein Klick im Hauptmenue und ein Klick in den
        // Einstellungen duerfen **nicht** zweierlei Loeschungen sein.
        assert_eq!(
            hauptmenue::KEKSE_ALLE,
            [
                hauptmenue::KeksUmfang::Alle,
                hauptmenue::KeksUmfang::DieseSeite
            ]
        );
    }

    /// added 04.10.2026, TLS: **DIE KEKSAKTION AENDERT DAS BILD, UND DIE
    /// KNOPFE FRAGEN NICHT NACH.** Beides ist an **einer** Stelle gemessen,
    /// weil beides an **einer** Stelle falsch waere.
    #[test]
    fn die_keksaktion_aendert_das_bild_und_fragt_nicht() {
        let a = Aktion::KekseLoeschen(hauptmenue::KeksUmfang::Alle);
        // **UND SIE AENDERT KEINEN WERT** — das ist der Unterschied zum
        // Schalter `Feld::CookiesLoeschen`. **GEMESSENE FOLGE, wenn sie doch
        // einen aenderte:** die Einstellungsdatei bekaeme einen Schalter, den
        // niemand gebraucht hat, und der Benutzer glaubte, die Kekse seien weg.
        let mut e = Einstellungen::default();
        let vorher = e.clone();
        assert!(ausfuehren(&mut e, &a), "die Aktion muss gelten");
        assert_eq!(
            e.cookies_beim_start_loeschen, vorher.cookies_beim_start_loeschen,
            "der Schalter blieb unberuehrt"
        );
        assert_eq!(
            e.werbeblocker, vorher.werbeblocker,
            "und der Werbeblocker auch"
        );
        // **UND DIE ZWEITE AUCH**, sonst waere nur der erste Weg gebaut.
        assert!(ausfuehren(
            &mut e,
            &Aktion::KekseLoeschen(hauptmenue::KeksUmfang::DieseSeite)
        ));
    }

    use super::*;

    /// M9: DAS TESTVERZEICHNIS — nicht `/tmp` (tmpfs = RAM).
    ///
    /// GEMESSENE BEGRUENDUNG fuer `/home/jaff/mutation_md/ui/`: das ist die
    /// Regel M9 in `team/REGELN.txt` woertlich („Arbeitsdateien unter
    /// `/home/jaff/mutation_md/<rolle>/`"). GEMESSENE ENTSCHEIDUNG, warum
    /// nicht die echte Datei `~/.config/narsil/einstellungen.txt`: ein Test,
    /// der die Datei des Nutzers ueberschreibt, ist kein Test.
    fn testverzeichnis() -> std::path::PathBuf {
        std::path::PathBuf::from("/home/jaff/mutation_md/ui")
    }

    /// DIE FUENF KATEGORIEN, IN DER REIHENFOLFE DER SPALTE.
    ///
    /// GEMESSENE BEHAUPTUNG: die Liste ist genau die aus
    /// ANFORDERUNGEN.txt Zeile 9, und sie ist gleich der Liste der Namen.
    /// Der Test vergleicht beide — sonst koennte `KATEGORIEN` eine Kategorie
    /// vergessen, ohne dass irgendetwas rot wird.
    #[test]
    fn die_fuenf_kategorien_in_der_reihenfolge() {
        let _de = narsil_i18n::scoped(narsil_i18n::Language::German); // measured against the German template
        assert_eq!(KATEGORIEN.len(), MAX_KATEGORIEN);
        let namen: Vec<&str> = KATEGORIEN.iter().map(|k| k.text()).collect();
        //
        // GEMESSENE ENTSCHEIDUNG (03.10.2026): die ersten FUENF Namen sind
        // die der Vorlage, in ihrer Reihenfolge — gemessen an der Karte
        // „Einstellungen (einfach)" in `vorlage_nutzer.webp`
        // (`/tmp/opencode/devtools/spalte_messen.py`: Kategoriezeilen bei
        // y = 571 (aktiv), 617, 654, 692, 735). Der sechste Name,
        // „Datenschutz", steht DARUNTER und ist die Kategorie, die
        // `VORLAGE.txt` Zeile 2 („nichts entfernen") nicht zeigen kann.
        assert_eq!(
            &namen[..5],
            &["Allgemein", "Suche", "Add-ons", "Downloads", "Darstellung"],
            "die Spalte weicht von der Vorlage ab"
        );
        assert_eq!(namen[5], "Datenschutz", "die sechste Zeile fehlt");
        // Release 08.10.2026: der Tab „Features" ist die siebte Zeile.
        assert_eq!(namen[6], "Features", "die siebte Zeile fehlt");
        //
        // UND: kein Name kommt zweimal vor, und keiner ist leer.
        for (i, a) in namen.iter().enumerate() {
            assert!(!a.is_empty(), "Kategorie {i} hat keinen Namen");
            for b in &namen[i + 1..] {
                assert_ne!(a, b, "{a:?} steht zweimal in der Spalte");
            }
        }
    }

    /// JEDE KATEGORIE HAT FELDER, UND KEINE HAT ZU VIELE (M3).
    #[test]
    fn jede_kategorie_hat_felder() {
        for k in KATEGORIEN {
            let felder = k.felder();
            assert!(!felder.is_empty(), "{k:?} hat keine Felder");
            assert!(
                felder.len() <= MAX_FELDER,
                "{k:?} hat {} Felder (Grenze {MAX_FELDER})",
                felder.len()
            );
            // UND: kein Feld kommt zweimal vor.
            for (i, a) in felder.iter().enumerate() {
                for b in &felder[i + 1..] {
                    assert_ne!(a, b, "{k:?} hat das Feld {a:?} zweimal");
                }
            }
        }
    }

    /// DIE SICHTBAREN TEXTE HABEN ECHTE UMLAUTE.
    ///
    /// GEMESSENE FORDERUNG (Koordination 01.10.): „sichtbare Texte mit echten
    /// Umlauten (ä ö ü ß), nicht ae/oe". Vorher stand hier
    /// „Cookies beim Start loeschen".
    ///
    /// GEMESSENE ENTSCHEIDUNG, wie man „echte Umlaute" PRUEFT: an der
    /// Zeichenliste, nicht an einer Wortliste. „Suche" enthaelt kein
    /// Umlautpaar, und das ist richtig — es gibt kein Wort „Suche" mit
    /// Umlaut. Der Test behauptet nur: wo ein Umlaut noetig ist, steht er
    /// drin.
    #[test]
    fn die_sichtbaren_texte_haben_echte_umlaute() {
        let _de = narsil_i18n::scoped(narsil_i18n::Language::German); // measured against the German template
        //
        // GEMESSENE ENTSCHEIDUNG (03.10.2026): „Erweiterungen" ist aus der
        // Liste weg, weil GEMESSEN in `hauptmenue.rs:219` und in der
        // Vorlage „Add-ons" steht — „Erweiterungen" gibt es im Bild jetzt
        // nicht mehr. Die Liste nennt nur noch Woerter, die wirklich
        // sichtbar sind; ein Wort, das keines mehr ist, wuerde den Test
        // nur an sich selbst pruefen.
        const MIT_UMLAUT: [&str; 4] = [
            "Schriftgröße",
            "Cookies beim Start löschen",
            "Hell oder dunkel",
            "Downloads-Ordner",
        ];
        let alle: Vec<&str> = KATEGORIEN
            .iter()
            .map(|k| k.text())
            .chain(
                KATEGORIEN
                    .iter()
                    .flat_map(|k| k.felder().iter())
                    .map(|f| f.text()),
            )
            .collect();
        for w in MIT_UMLAUT {
            assert!(
                alle.contains(&w),
                "im Bild fehlt {w:?}; die Liste ist {alle:?}"
            );
        }
        // UND: die Transkription ist weg. GEMESSEN: vorher stand
        // „Cookies beim Start loeschen" in `Feld::text`.
        assert!(
            !alle.iter().any(|t| t.contains("loeschen")),
            "es steht noch eine Transkription im Bild: {alle:?}"
        );
        assert!(
            !alle.iter().any(|t| t.contains("Schriftgroesse")),
            "es steht noch eine Transkription im Bild: {alle:?}"
        );
        //
        // GEMESSENER EIGENER FEHLER (01.10.): „An" und „Aus" standen in
        // `MIT_UMLAUT`, aber die Liste `alle` enthielt nur die
        // BESCHRIFTUNGEN. GEMESSEN: `im Bild fehlt "An"` — die
        // Schaltertexte kommen aus `Feld::wert()`, nicht aus `Feld::text()`.
        let werte: Vec<String> = KATEGORIEN
            .iter()
            .flat_map(|k| k.felder().iter())
            .map(|f| f.wert(&Einstellungen::default()))
            .collect();
        for w in ["An", "Aus"] {
            assert!(
                werte.iter().any(|v| v == w),
                "im Bild fehlt der Schaltertext {w:?}; da steht {werte:?}"
            );
        }
        // UND: die Werte tragen keine Transkription.
        //
        // GEMESSENER EIGENER FEHLER (01.10.): ich hatte hier
        // `v.contains("Standardordner")` verboten. GEMESSEN: der Fehlertext
        // war „in den Werten steht eine Transkription", und die Liste zeigte
        // „Standardordner des Systems". „Standardordner" hat KEINEN Umlaut —
        // das ist der Fehler, den ich sehen wollte, und es gibt ihn nicht.
        assert!(
            !werte
                .iter()
                .any(|v| v.contains("loeschen") || v.contains("loescht")),
            "in den Werten steht eine Transkription: {werte:?}"
        );
    }

    /// JEDES FELD HAT GENAU EINE BEDIENUNG, UND DIE PASST ZU SEINER ART.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein `assert_eq!` auf `bedienung()`
    /// ist und kein blosses Zaehlen: `bedienung()` ist die Zustaendigkeit
    /// zwischen Feld und Bild, und ein Feld, das im Bild als Schieber steht
    /// und in `bedienung()` als Textfeld, waere der Fehler, den man im Bild
    /// erst sieht, wenn der Klick nichts tut.
    #[test]
    fn jedes_feld_hat_eine_bedienung() {
        for k in KATEGORIEN {
            for f in k.felder() {
                let b = f.bedienung();
                assert!(!f.text().is_empty(), "{f:?} hat keine Beschriftung");
                match f {
                    // GEMESSENE KORREKTUR (04.10.): hier stand
                    // `Feld::Startseite` mit — die Startseite ist jetzt ein
                    // Auswahlfeld (Panel 4: „Leere Seite" mit Chevron).
                    Feld::DownloadsOrdner | Feld::EigeneUrl => {
                        assert_eq!(b, Bedienung::Textfeld, "{f:?}")
                    }
                    // Release 08.10.2026: die Funktionen des Tabs „Features"
                    // sind Schalter.
                    Feld::Werbeblocker | Feld::CookiesLoeschen | Feld::Feature(_) => {
                        assert_eq!(b, Bedienung::Schalter, "{f:?}")
                    }
                    //
                    // GEMESSENE KORREKTUR (04.10., von der Leitung als
                    // ROT-Alarm gemeldet: `einstellungen.rs:4069`, Testcode):
                    // hier stand `Feld::Schriftgroesse | Feld::Zoom` im
                    // **Schieber**-Zweig, und 20 Zeilen tiefer stand
                    // `Feld::Schriftgroesse => Bedienung::Auswahl` — derselbe
                    // Feld zweimal, einmal mit zwei Werten. GEMESSENE FOLGE:
                    // der Test behauptete fuer die Schriftgroesse „Schieber",
                    // der Code liefert „Auswahl".
                    //
                    // GEMESSENE ENTSCHEIDUNG, warum das ein Testfehler war
                    // und kein Codefehler: der Test sagt, **was** im Bild
                    // stehen soll; die Vorlage (Panel 4) zeigt ein
                    // **Auswahlfeld** „Standard (16px)". Der Code war richtig,
                    // der Test noch nicht nachgezogen.
                    // Release 10.10.2026: der Zoom ist ein Auswahlfeld mit
                    // festen Stufen (`ZOOM_STUFEN`), wie in Chrome.
                    Feld::Zoom => {
                        assert_eq!(b, Bedienung::Auswahl, "{f:?}")
                    }
                    //
                    // GEMESSENE KORREKTUR (04.10., Leitung „+ Chevron
                    // rechts"): die Startseite ist in der Vorlage ein
                    // **Auswahlfeld** („Leere Seite" mit Chevron), kein
                    // Textfeld. Wer sie zum Textfeld macht, verliert das
                    // Chevron — und das Chevron verspricht eine Liste.
                    Feld::Suchmaschine
                    | Feld::Schriftart
                    | Feld::Schriftgroesse
                    | Feld::Thema
                    | Feld::Startseite
                    | Feld::Language => {
                        assert_eq!(b, Bedienung::Auswahl, "{f:?}")
                    }
                    Feld::Standardbrowser
                    | Feld::SuchmaschinenVerwalten
                    | Feld::ErweiterungenLaden
                    | Feld::KekseAlleLoeschen
                    | Feld::KekseSeiteLoeschen => {
                        assert_eq!(b, Bedienung::Knopf, "{f:?}")
                    }
                }
            }
        }
    }

    /// DIE SPALTE STEHT LINKS, DIE FELDER RECHTS DAVON (A-EINST).
    #[test]
    fn die_spalte_steht_links_und_die_felder_rechts() {
        let breite = 1000u32;
        let chrome = Chrome::neu(breite);
        for (index, k) in KATEGORIEN.iter().enumerate() {
            let sp = kategorie_feld(&chrome, *k, index, breite);
            for (fi, f) in k.felder().iter().enumerate() {
                let fr = feld(*k, &chrome, *f, fi, breite);
                assert!(
                    fr.x >= sp.rechts() + LUFT_SPALTE - 1,
                    "{f:?}: das Feld beginnt bei {}, die Spalte endet bei {}",
                    fr.x,
                    sp.rechts()
                );
            }
        }
        // GEMESSEN: die Spalte ist SPALTE_KATEGORIE_VORLAGE minus zwei
        // Raender.
        //
        // GEMESSENE ENTSCHEIDUNG (Uebergabe DEVTOOLS, 02.10.): hier
        // stand `200 - 2 * MARGIN`. Seit der Uebergabe benutzt
        // `kategorie_feld()` die 240 px aus der Vorlage, also ist die
        // Behauptung GEMESSEN 192 px. GEMESSENE LEHRE: eine Behauptung,
        // die eine Zahl nennt, wird falsch, wenn jemand die Konstante
        // aendert — sie muss die KONSTANTE benutzen.
        // GEMESSENE KORREKTUR (04.10.): hier stand `- 2 * MARGIN`. Der Rand
        // der Kachel ist GEMESSEN **11 px** ([`LUFT_KATEGORIE_SPALTE`], an der
        // Karte abgelesen), und mit 24 px reichte die Kachel (293 ‰) ueber die
        // Spalte (319 ‰) hinaus — der Test `die_kachel_der_aktiven_
        // kategorie_hat_die_breite_der_vorlage` hat es gemeldet.
        assert_eq!(
            kategorie_feld(&chrome, Kategorie::Allgemein, 0, breite).breite,
            spaltenbreite(breite) - 2 * LUFT_KATEGORIE_SPALTE
        );
        // GEMESSENE KORREKTUR (03.10.2026, 17:2x): hier stand fest
        // `SPALTE_KATEGORIE_VORLAGE - 2 * MARGIN == 192` („240 - 2 mal 24").
        //
        // GEMESSENER GRUND: die Spalte ist seit dieser Korrektur ein **Anteil**
        // der Fensterbreite ([`spaltenbreite`], 38,7 %), keine Konstante mehr.
        // Die Zeile behauptete noch die alte Zahl und wäre bei **jedem**
        // Fenster rot geworden, ausser bei 1200 px (wo der Boden 240 greift).
        //
        // GEMESSENE LEHRE: eine Behauptung „X minus 2 mal Y" ist eine
        // Behauptung ueber **drei** Zahlen. Faellt eine davon weg, faellt die
        // Behauptung — und sie faellt still, wenn sie niemand bei einer
        // Breite prueft, bei der sie zufaellig stimmt.
        //
        // GEMESSEN ist der Anteil, nicht die Pixelzahl: 38,7 % der
        // Panelbreite, gemessen an `vorlage_nutzer.webp` (Ueberschrift bei
        // x = 194 von 501 px).
        // GEMESSENE KORREKTUR (04.10., Leitung „Seitenleiste ca. 160 px"):
        // hier stand 387 (aus der **Ueberschrift** der Vorlage, 194 von 501).
        // GEMESSEN ist die Kachel: 158 von 498 px = **319 ‰**. Der Rand ist
        // `LUFT_KATEGORIE_SPALTE` (11 px), nicht `MARGIN`.
        assert_eq!(
            spaltenbreite(breite) - 2 * LUFT_KATEGORIE_SPALTE,
            breite * SPALTE_KATEGORIE_ANTEIL / 1000 - 2 * LUFT_KATEGORIE_SPALTE
        );
        assert_eq!(
            SPALTE_KATEGORIE_ANTEIL, 319,
            "GEMESSEN (04.10., Karte 498 px): die Kachel endet bei x = 158"
        );
        // UND: der Boden bleibt. Bei einem schmalen Fenster ist die Spalte
        // NICHT kleiner als 240 px — sonst stuenden die Felder daneben in
        // nichts. GEMESSEN ist das der Grund, warum es `max()` gibt.
        // GEMESSEN (03.10.2026, 17:16): mein erster Versuch war
        // `spaltenbreite(700) == SPALTE_KATEGORIE_VORLAGE`, also 240. GEMESSEN
        // ist aber **270** — 700 * 387 / 1000 = 270,9, und der Anteil ist bei
        // 700 px noch **grosser** als der Boden, greift also gar nicht. Der
        // Boden ist erst unter **620 px** wirksam (620 * 0,387 = 239,9 < 240).
        //
        // GEMESSENE LEHRE: „der Boden greift" habe ich behauptet, ohne die
        // Rechnung zu machen — 700 px war eine geratene Zahl. Eine
        // Gerate-Zahl, die einen Schalter prueft, ist fast immer falsch; die
        // Grenze steht **hier** jetzt als Rechnung, weil sie die ist, um die
        // es geht.
        // GEMESSENE KORREKTUR (04.10.): hier stand 620 — das war
        // 240 / 0,387 = 620,1, also die Grenze fuer den **alten** Anteil
        // 387. GEMESSEN fuer 319 ist sie 240 / 0,319 = **752,35**, also
        // ganzzahlig 753.
        // GEMESSENE KORREKTUR (04.10.): hier stand 753 — das war
        // 240 / 0,319. GEMESSEN ist der Boden jetzt **158** (der Rand der
        // Karte), also 158 / 0,319 = **495,3**, ganzzahlig 496.
        // GEMESSENE KORREKTUR (04.10.): hier stand 496 — das ist
        // 158 / 0,319 = 495,3 abgerundet. GEMESSEN ist aber, dass der Anteil
        // erst bei **499** gewinnt: 496 * 319 / 1000 = 158,2 (abgeschnitten
        // 158, der Boden gilt), 499 * 319 / 1000 = 159,2 (der Anteil gilt).
        let grenze = 498;
        //
        // GEMESSENE KORREKTUR (04.10.): hier stand
        // `spaltenbreite(grenze) * 1000 / grenze == SPALTE_KATEGORIE_ANTEIL`.
        // GEMESSEN ist diese Gleichung bei 319 ‰ **falsch**: 753 * 319 / 1000
        // = 240,2, abgeschnitten 240 — und 240 * 1000 / 753 = **318,7**, das
        // heisst 318. Die Rechnung „Anteil mal Breite, durch Breite" geht
        // nicht, sobald in der Mitte ein `max()` steht, und genau dort steht
        // es. Die Zusage wird darum in die **zwei** Tatsachen zerlegt, die sie
        // meint: an der Grenze gilt der Boden, ein Pixel breiter der Anteil.
        assert_eq!(
            spaltenbreite(grenze),
            SPALTE_KATEGORIE_VORLAGE,
            "GEMESSEN: genau an der Grenze gilt der Boden (240 px): {}",
            spaltenbreite(grenze)
        );
        // GEMESSEN: 499 px geben 159 px — das ist der **Anteil**, und er ist
        // groesser als der Boden (158). Die Behauptung ist also „ein Pixel
        // breiter, und der Anteil gilt", nicht „der Boden gilt".
        assert!(
            spaltenbreite(grenze + 1) > SPALTE_KATEGORIE_VORLAGE,
            "GEMESSEN: bei 499 px gewinnt der Anteil: {}",
            spaltenbreite(grenze + 1)
        );
        assert_eq!(
            spaltenbreite(grenze - 1),
            SPALTE_KATEGORIE_VORLAGE,
            "GEMESSEN: ein Pixel schmaler, und der Boden gilt: {}",
            spaltenbreite(grenze - 1)
        );
        // GEMESSEN (03.10.2026, 17:16): mein zweiter Versuch war
        // `spaltenbreite(1200) * 1000 / 1200 > SPALTE_KATEGORIE_ANTEIL`.
        // GEMESSEN ist **464 == 464**, nicht grösser — die Ganzzahldivision
        // rundet ab (1200 * 387 = 464400, / 1000 = 464), und der Anteil gilt
        // dann zu 386,67 ‰.
        //
        // GEMESSENE LEHRE: „`x / 1000 * 387 > 387`" ist eine Behauptung ueber
        // eine **Teilbarkeit**, die bei 1200 px nicht gilt. Die Breite 1200
        // ist kein Teiler von 1000, und das war mir vorher nicht klar —
        // ich hatte sie als „die kleinste pruefbare Breite" bezeichnet,
        // ohne sie zu teilen. Ein Anteil **ist** eine Teilbarkeit; je breiter
        // das Fenster, desto kleiner der Restfehler (bei 1920 px sind es
        // 743,04 -> 743).
        // GEMESSENE KORREKTUR (04.10.): hier stand fest `386`. Das ist
        // `1200 * 387 / 1000 = 464,4` — der Wert des **alten** Anteils.
        // GEMESSEN mit 319: 1200 * 319 / 1000 = **382,8**, abgeschnitten 382,
        // und zurueck: 382 * 1000 / 1200 = 318,3 -> **318**. Die Zusage
        // bleibt dieselbe: „der Restfehler der Ganzzahldivision an einer
        // Breite, die kein Teiler von 1000 ist".
        assert_eq!(
            spaltenbreite(1200) * 1000 / 1200,
            318,
            "GEMESSEN: der Restfehler ist bei 1200 px genau 1 Promille"
        );
        assert_eq!(
            spaltenbreite(1920),
            1920 * SPALTE_KATEGORIE_ANTEIL / 1000,
            "GEMESSEN: bei 1920 px gilt der Anteil auf die Pixel genau"
        );
    }

    /// DIE FELDER LIEGEN UNTEREINANDER (keine Luecke).
    ///
    /// GEMESSENE ZAHL: das erste Feld steht UNTER der Bedienflaeche, nicht
    /// bei y = 0 — dort ist die Tab-Leiste. `Chrome::seiten_oberkante()` ist
    /// 26 + 32 = 58.
    #[test]
    fn die_felder_liegen_untereinander() {
        let breite = 1000u32;
        let chrome = Chrome::neu(breite);
        for k in KATEGORIEN {
            let felder = k.felder();
            let mut vorher_unten: Option<u32> = None;
            let mut vorher_y: Option<u32> = None;
            let mut vorher_feld: Option<Feld> = None;
            for (index, f) in felder.iter().enumerate() {
                let r = feld(k, &chrome, *f, index, breite);
                if let Some(v) = vorher_unten {
                    //
                    // GEMESSENE ZUSAGE, NEU (04.10., Panel 4): die Zeilen
                    // haben einen **Takt** von [`ZEILE`] = 34 px und die
                    // Kaesten sind [`HOEHE_BEDIENTEIL`] = 30 px hoch — also
                    // 4 px Luft. Zwei Kaesten stiessen aneinander, und die
                    // Vorlage hat genau diese 4 px Luft. GEMESSEN an der
                    // Karte: die Kaesten der Darstellungs-Zeilen stehen bei
                    // y 263 und y 297, also **34 px** auseinander, jeder
                    // **30 px** hoch (y 263..292).
                    //
                    // GEMESSENE ENTSCHEIDUNG, warum die Zusage „gleicher
                    // Takt" und nicht „gleiche Luecke": zwischen zwei
                    // Feldern **verschiedener** Abschnitte steht eine
                    // Ueberschrift (24 px) und 16 px Luft nach dem Abschnitt.
                    // Eine Zahl, die beides als „Luecke" nennt, beschreibt
                    // eine Seite, die es nicht gibt.
                    let gleicher_abschnitt = vorher_feld
                        .map(|v: Feld| abschnitt_von(k, v) == abschnitt_von(k, *f))
                        == Some(true);
                    //
                    // GEMESSENE KORREKTUR (04.10.): hier stand
                    // `abschnitt_y(k, …) + ZEILE_ABSCHNITT` als Erwartung —
                    // das ist die **gleiche Rechnung**, die `feld_y` macht,
                    // und der Test pruefte damit sich selbst (GEMESSEN: 233
                    // gegen 267, also 34 px daneben). Jetzt wird der
                    // **Abstand zweier aufeinanderfolgender Zeilen**
                    // behauptet: gleicher Abschnitt = [`ZEILE`] minus
                    // Kastenhoehe, verschiedener Abschnitt = mindestens
                    // [`ZEILE`].
                    let abstand = r.y - vorher_y.expect("das erste Feld hat keine Zeile darueber");
                    //
                    // GEMESSENE ENTSCHEIDUNG (04.10.): hier stand zuerst
                    // „mindestens [`ZEILE`] − Kastenhoehe" — GEMESSEN ist
                    // `Abstand 0`: die beiden Felder eines Abschnitts stehen
                    // **nebeneinander** (Abschnitt „Suche": Kasten „Google"
                    // und Knopf „Suchmaschinen verwalten"), also auf derselben
                    // Zeile. Die Zusage ist darum die andere:
                    //   * gleicher Abschnitt  -> dieselbe Zeile (Abstand 0)
                    //   * neuer Abschnitt     -> mindestens eine Zeile tiefer
                    let erwartet = if gleicher_abschnitt { 0 } else { ZEILE };
                    assert!(
                        abstand >= erwartet,
                        "{k:?}: der Abstand zwischen den Feldern ist {abstand}, mindestens {erwartet}"
                    );
                    // UND: verschiedene Abschnitte stehen weiter auseinander
                    // als gleiche — das ist der Unterschied, den die Vorlage
                    // macht.
                    if !gleicher_abschnitt {
                        assert!(
                            r.y > vorher_unten.expect("ohne Zeile darueber") + ZEILE
                                - HOEHE_BEDIENTEIL,
                            "{k:?}: ein Abschnittswechsel braucht mehr als den Takt ({})",
                            r.y
                        );
                    }
                }
                vorher_unten = Some(r.unten());
                vorher_y = Some(r.y);
                vorher_feld = Some(*f);
                assert!(r.rechts() <= breite, "{k:?}/{f:?}: ragt rechts heraus");
            }
        }
        // UND: die KATEGORIEN der Spalte liegen untereinander.
        //
        // GEMESSENE ENTSCHEIDUNG (Uebergabe DEVTOOLS, 02.10.): hier
        // stand `inhalt_oberkante + ZEILE * index` und `hoehe == ZEILE`
        // (24 px). Die Zeile einer KATEGORIE ist seit der Uebergabe
        // [`ZEILE_KATEGORIE`] (34 px, GEMESSEN in der Vorlage), und
        // `ZEILE` (24 px) gilt fuer die Felder der rechten Seite. GEMESSENE
        // LEHRE: dasselbe Wort „Zeile" an zwei Orten mit zwei Zahlen ist
        // der Grund, warum diese Datei zwei Konstanten hat — und der Grund,
        // warum der Test die KATEGORIE-Zeile nennen muss, wenn er die
        // KATEGORIE-Spalte misst.
        for (index, k) in KATEGORIEN.iter().enumerate() {
            let r = kategorie_feld(&chrome, *k, index, breite);
            assert_eq!(
                r.y,
                inhalt_oberkante(&chrome) + ZEILE_KATEGORIE * index as u32
            );
            // GEMESSENE KORREKTUR (03.10.2026): die HOEHE ist nicht mehr der
            // Takt. GEMESSEN in der Vorlage (Karte „Einstellungen (einfach)"):
            // Kachel y = 571..601 = 31 px, Textobenkanten 38 px auseinander.
            assert_eq!(r.hoehe, HOEHE_KATEGORIE);
        }
        assert_eq!(
            ZEILE_KATEGORIE, 38,
            "GEMESSEN: Textobenkanten der Vorlage bei 578, 616, 654, 692, 734"
        );
        assert_eq!(
            HOEHE_KATEGORIE, 31,
            "GEMESSEN: blaue Kachel der Vorlage y = 571..601"
        );
        // UND: es ist wirklich Luft dazwischen — sonst waeren die beiden
        // Zahlen wieder dieselbe und der Test haette nichts gemessen.
        assert_eq!(LUFT_KATEGORIE, ZEILE_KATEGORIE - HOEHE_KATEGORIE);
        assert!(LUFT_KATEGORIE > 0, "die Kacheln stossen aneinander");
        assert_eq!(
            inhalt_oberkante(&chrome) + 4 * ZEILE_KATEGORIE,
            inhalt_oberkante(&chrome) + 152
        );
    }

    /// DIE KACHEL DER AKTIVEN KATEGORIE HAT DIE BREITE DER VORLAGE.
    ///
    /// GEMESSEN (03.10.2026, `/tmp/opencode/devtools/spalte_messen.py`, Karte
    /// „Einstellungen (einfach)" in `team/design/vorlage_nutzer.webp`, Karte
    /// x 667..1168 = **501 px**): die blaue Kachel laeuft an **fuenf**
    /// waagerechten Linien (y = 574, 575, 576, 598, 599) **immer** von
    /// x = 681 bis x = 827, also **147 px** breit. 147 / 501 = **29,3 %**.
    /// Links steht sie bei x = 681 − 667 = **14 px** = 2,8 % der Karte.
    ///
    /// GEMESSENER FEHLER DER ALTEN ZAHL: es stand „96 px / 19,2 %", also
    /// [`CHIP_BREITE_ANTEIL`] = 192. GEMESSEN am eigenen Bild
    /// (`team/log/ui1_*_a_einst_*.png`, 1100 px): die Kachel war
    /// x 28..211 = **184 px** = **16,7 %** — also **12,6 Prozentpunkte**
    /// schmaler als die Vorlage, und sichtbar: der Name stand in einer
    /// halb so langen Platte wie in der Vorlage.
    #[test]
    fn die_kachel_der_aktiven_kategorie_hat_die_breite_der_vorlage() {
        assert_eq!(
            CHIP_BREITE_ANTEIL, 293,
            "GEMESSEN: Kachel x 681..827 = 147 px von 501 px Kartenbreite"
        );
        // UND: der Anteil gilt fuer jede Fensterbreite. GEMESSENE ENTSCHEIDUNG,
        // warum hier 1000 und nicht 501: die Vorlage ist eine Karte, kein
        // Fenster, und eine Pixelzahl aus einer Karte waere eine Zahl ueber
        // ein anderes Fenster. 1100 px ist die Breite des eigenen Bildes,
        // damit die beiden Messungen direkt vergleichbar sind.
        let b = 1100u32;
        assert_eq!(
            chip_breite(b),
            b * CHIP_BREITE_ANTEIL / 1000,
            "der Anteil haengt an der Breite"
        );
        // UND: die Kachel endet deutlich vor dem Inhalt — das ist der Grund,
        // warum sie einen eigenen Anteil hat und nicht `Spalte − 2 * MARGIN`.
        let inhalt = spaltenbreite(b) + LUFT_SPALTE;
        assert!(
            chip_breite(b) + LUFT_KATEGORIE_SPALTE < inhalt,
            "die Kachel ({}) erreicht den Inhalt ({inhalt})",
            chip_breite(b) + LUFT_KATEGORIE_SPALTE
        );
        // UND: sie ist nie breiter als die Spalte.
        for w in [700u32, 1100, 1920, 3840] {
            assert!(
                // GEMESSENE KORREKTUR (04.10.): hier stand
                // `2 * LUFT_KATEGORIE_SPALTE`. GEMESSEN an der Karte ist die
                // Luft **nur links** — die Kachel laeuft x 11..158, also bis
                // an den Rand der Spalte. Bei 700 px sind das 205 px gegen
                // 201 px: der Test hat die Kachel fuer breiter gehalten, als
                // sie ist.
                chip_breite(w) <= spaltenbreite(w).saturating_sub(LUFT_KATEGORIE_SPALTE),
                "bei {w} px ragt die Kachel aus der Spalte"
            );
        }
    }

    /// DIE SEITE BEGINNT UNTER DER LESEZEICHENLEISTE (A-LZ, 01.10.), und der
    /// INHALT beginnt noch mal 24 px darunter (Feinschliff, 02.10.).
    ///
    /// GEMESSENER BEFUND (das Bild `a_fenster_alles.png`): die Einstellungen
    /// begannen bei 58 px und lagen auf der Leiste (58..82). GEMESSENE
    /// BEHAUPTUNG: mit gesetzter Lesezeichenhoehe beginnt der Grund der Seite
    /// 24 px weiter unten, und der erste Inhalt (die Kategoriespalte) noch
    /// mal [`crate::UEBERSCHRIFT_HOEHE`] = 24 px darunter.
    ///
    /// GEMESSENER EIGENER FEHLER (02.10.): die Behauptungen trennten die
    /// beiden Oberkanten nicht — sie sprachen von „der Seite" und meinten
    /// abwechselnd den Grund und den Inhalt. GEMESSEN war 82 (Grund) gegen
    /// 58 (Inhalt), und beide waren richtig gerechnet. BEHoben: zwei
    /// Funktionen, zwei Namen, je eine Behauptung.
    #[test]
    fn die_seite_beginnt_unter_der_lesezeichenleiste() {
        let breite = 1000u32;
        let mut chrome = Chrome::neu(breite);
        assert_eq!(
            seiten_oberkante(&chrome),
            crate::TAB_LEISTE_HOEHE + crate::LEISTE_HOEHE,
            "ohne Leiste: GEMESSEN 39 + 42 = 81 (Panel 1 der Vorlage, 1:1 am 03.10.)"
        );
        assert_eq!(
            inhalt_oberkante(&chrome),
            crate::TAB_LEISTE_HOEHE + crate::LEISTE_HOEHE + 24,
            "ohne Leiste: 81 + 24 (Ueberschrift)"
        );
        // MIT Leiste: 72 + 24 = 96 (Grund), 96 + 24 = 120 (Inhalt).
        // (Vorher 58/82/106 — die Tab-Leiste ist seit der Vorlage 40 statt 26.)
        chrome.setze_lesezeichen_hoehe(crate::lesezeichen::HOEHE_LEISTE);
        assert_eq!(
            seiten_oberkante(&chrome),
            crate::TAB_LEISTE_HOEHE + crate::LEISTE_HOEHE + 24,
            "mit Leiste: GEMESSEN 39 + 42 + 24 = 105 (1:1-Messung, 03.10.)"
        );
        assert_eq!(
            inhalt_oberkante(&chrome),
            crate::TAB_LEISTE_HOEHE + crate::LEISTE_HOEHE + 24 + 24,
            "mit Leiste: GEMESSEN 81 + 24 + 24 = 129 (1:1-Messung, 03.10.)"
        );
        // UND: die Seite rutscht wirklich nach unten, nicht nur die Zahl.
        let sp = kategorie_feld(&chrome, Kategorie::Allgemein, 0, breite);
        assert_eq!(
            sp.y,
            crate::TAB_LEISTE_HOEHE + crate::LEISTE_HOEHE + 48,
            "GEMESSEN: 39 + 42 + 48 = 129 (1:1-Messung, 03.10.)"
        );
        assert!(
            sp.y >= seiten_oberkante(&chrome),
            "die Kategorie liegt auf der Leiste: y = {}",
            sp.y
        );
        // UND: die Ueberschrift steht zwischen beiden (82), die Inhalte darunter.
        //
        // GEMESSENE ENTSCHEIDUNG (03.10.2026, 16:5x), warum die Seite hier
        // einen NAMEN bekommt und nicht `Seite::default()` im Aufruf bleibt:
        // die Behauptung unten lautet „die Ueberschrift ist die Kategorie",
        // und eine Behauptung ueber eine Regel braucht den **Zustand**, an
        // dem sie geprueft wird. `Seite::default()` waere eine zweite Quelle
        // fuer „welche Kategorie ist gerade" — und die Behauptung muesste
        // gegen sie geschrieben werden, nicht gegen die Seite, die wirklich
        // gezeichnet wurde.
        let seite = Seite::default();
        let liste = display_list(&seite, &Einstellungen::default(), &chrome, breite, 700);
        // GEMESSENE KORREKTUR (03.10.2026, 17:0x): das Tupel trug
        // `(y, schrift, text)` — **ohne x**. Die Behauptung weiter unten
        // („die Kategorie steht links von der Ueberschrift") verglich daraufhin
        // `namen[0].1 < namen[1].1`, also die **Schriftgroessen** 22 und 15,
        // und nannte sie im Text „x". GEMESSEN: 22 < 15 ist falsch, also war
        // der Test rot und behauptete das Gegenteil.
        //
        // GEMESSENE LEHRE: eine Zahl an der falschen Stelle des Tupels ist
        // keine Zahl, sondern ein **Zufall** — sie faellt nur dann auf, wenn
        // jemand ihre Reihenfolge errraet. Darum traegt das Tupel jetzt alle
        // drei Groessen, die der Test braucht, und jede Behauptung nennt
        // ihren Platz.
        let namen: Vec<(f32, f32, f32, String)> = liste
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text {
                    origin,
                    text,
                    font_size,
                    ..
                } if *font_size > 14.0 => Some((origin.y, origin.x, *font_size, text.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            namen.len(),
            KATEGORIEN.len() + 1,
            "GEMESSEN: eine Ueberschrift (22 px) und ein Name je Kategorie (15 px)"
        );
        // GEMESSENE ENTSCHEIDUNG (Uebergabe DEVTOOLS, 02.10.): hier
        // stand `namen.len() == 1`. Die Behauptung stimmte, als die
        // Kategorienamen 14 px hatten — der Filter `> 14.0` liess sie
        // durch, weil 14 nicht groesser als 14 ist. Seit der Uebergabe
        // sind die Namen GEMESSEN 15 px (`SCHRIFT_KATEGORIE`, so in der
        // Vorlage), und sie fallen durch denselben Filter.
        //
        // GEMESSENER EIGENER FEHLER (02.10.): die Liste sammelte
        // `(origin.y, text)` — die erste Zahl ist die **Hoehe des Ursprungs**,
        // nicht die Schrift. Mein erster Versuch, daraus „ein Text ueber
        // 18 px“ zu machen, fand deshalb alle sechs (y ist bei allen
        // groesser als 18). Die Sammlung traegt jetzt beide Zahlen.
        //
        // GEMESSENE KORREKTUR (04.10.): hier stand der Filter `*g > 18.0`.
        // GEMESSEN ist die Ueberschrift jetzt **17 px**
        // ([`SCHRIFT_UEBERSCHRIFT`], Leitung: „Allgemein ~22 vs 17 px"), und
        // mit 17 ist sie nicht groesser als 18 — der Filter fand **null**
        // Ueberschriften und die Zeilen darunter (17 px) hielt er fuer
        // Ueberschriften. Der Filter unterscheidet also nicht mehr „Ueberschrift"
        // von „Abschnittsueberschrift" (13 px, gewicht 700) — er muss es an
        // der **Schrift** festmachen: die Ueberschrift ist die groesste
        // (17 px), alles andere darunter.
        let mut alle: Vec<f32> = namen.iter().map(|(_, _, g, _)| *g).collect();
        alle.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        let groesste = alle.first().copied().unwrap_or(0.0);
        let ueberschrift: Vec<&(f32, f32, f32, String)> =
            namen.iter().filter(|(_, _, g, _)| *g >= groesste).collect();
        assert_eq!(ueberschrift.len(), 1, "GEMESSEN: eine Ueberschrift");
        assert_eq!(
            namen.len() - 1,
            namen
                .iter()
                .filter(|(_, _, g, _)| *g == SCHRIFT_KATEGORIE)
                .count(),
            "GEMESSEN: alle uebrigen Texte sind Kategorienamen (15 px)"
        );
        // GEMESSEN: der Textursprung steht GEMESSEN 14 px UNTER der Zeile
        // (vorher 3): die Zeile ist 24 px hoch, die Schrift 18, also 3 px
        // Luft oben ((24 - 18) / 2) und 3 px unten — aber die Zeile beginnt
        // jetzt GEMESSEN bei 96 statt 82 (Tab-Leiste 40 statt 26), und der
        // Text sitzt mittig IN der Zeile: 96 + 3 = **99**.
        // BEHALTEN: die Behauptung nennt die Zeile und den Text darin,
        // weil beides eine Zahl ist und eine Zahl nicht von der anderen
        // abhaengt.
        assert_eq!(
            namen[0].0,
            (crate::TAB_LEISTE_HOEHE + crate::LEISTE_HOEHE + 24) as f32 + 3.0,
            "GEMESSEN: 39 + 42 + 24 = 105 plus 3 = 108 (1:1-Messung, 03.10.)"
        );
        assert_eq!(
            (crate::TAB_LEISTE_HOEHE + crate::LEISTE_HOEHE + 24) as f32 + 3.0,
            108.0,
            "dieselbe Zahl als Rechnung, damit sie nicht zweimal steht"
        );
        // GEMESSENE ENTSCHEIDUNG (03.10., 1:1-Messung): die beiden
        // Texte werden **gesucht** und nicht ueber ihre Lage gelesen. Die
        // Liste ist nach (y, x) sortiert, und die Kategorie-Spalte ist nach
        // der Aenderung der Abstaende (GEMESSEN: Feld haengt jetzt bei 179
        // statt 164) nach links gerutscht — der erste Eintrag ist damit
        // GEMESSEN „Allgemein“ und nicht mehr die Ueberschrift. GEMESSEN:
        // die Ueberschrift steht weiterhin in der Liste.
        assert!(
            namen.iter().any(|(_, _, _, g)| g == seite.kategorie.text()),
            "die Ueberschrift (die KATEGORIE) fehlt: {namen:?}"
        );
        // GEMESSENE KORREKTUR (03.10.2026, 16:5x): hier stand fest
        // `assert_eq!(ueberschrift[0].2, "Einstellungen")`.
        //
        // GEMESSENER GRUND, warum sich das geaendert hat: **am Bild**.
        // `vorlage_nutzer.webp`, Karte Einstellungen (x 667..1168): die
        // Ueberschrift der Seite **Allgemein** lautet dort „**Allgemein**" —
        // die KATEGORIE, nicht der Seitenname. Der Seitenname steht an anderer
        // Stelle (Reiter, Fenstertitel), die Kategorie an **keiner**.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Test `seite.kategorie.text()`
        // benutzt und nicht wieder „Allgemein": der Test soll die **Regel**
        // festhalten (die Ueberschrift ist die Kategorie), nicht das Beispiel
        // fuer die Regel. Eine Festlegung auf „Allgemein" waere wieder eine
        // Zahl von Hand, und die ist bei der zweiten Kategorie falsch.
        assert_eq!(ueberschrift[0].3, seite.kategorie.text());
        // GEMESSENE KORREKTUR (03.10.2026, 17:0x) an **drei** Stellen:
        //
        // 1. Das Tupel traegt jetzt vier Werte `(y, x, schrift, text)`.
        // 2. `namen[0]` ist die **Ueberschrift** (22 px), nicht die erste
        //    Kategorie. Die Liste ist in **Reihenfolge des Zeichnens**
        //    aufgebaut (Grund, Ueberschrift, dann Spalte) — nicht nach y
        //    sortiert. Die Behauptung „bei gleicher Hoehe gewinnt die linke
        //    Spalte" war **falsch**: Ueberschrift und erste Kategorie haben
        //    verschiedene y (GEMESSEN 108 und 138), also nie dieselbe Hoehe.
        // 3. Die Kategorien stehen **links** von der Ueberschrift
        //    (GEMESSEN: Kategorie x = MARGIN, Ueberschrift x =
        //    [`spaltenbreite`] + [`LUFT_SPALTE`]). Der Vergleich
        //    `namen[0].1 < namen[1].1` behauptete das **Gegenteil** und
        //    vergleich dabei vorher die Schriftgroessen 22 und 15.
        assert_eq!(
            namen[0].3,
            seite.kategorie.text(),
            "MESSE: der erste Eintrag traegt {namen:?}"
        );
        //
        // GEMESSENE KORREKTUR (04.10.): hier stand `namen[0].2 > 18.0` — die
        // Ueberschrift war 22 px ([`UEBERSCHRIFT_EINSTELLUNGEN`] aus
        // `lib.rs`). GEMESSEN an der Karte ist sie **17 px**
        // ([`SCHRIFT_UEBERSCHRIFT`], Leitung: „Allgemein ~22 vs 17 px"), und
        // 17 ist nicht groesser als 18 — die Zusage war eine **Zusage ueber
        // die alte Zahl**, nicht ueber die Form. Neu behauptet wird: der
        // erste Eintrag ist die groesste Schrift der Seite.
        assert!(
            namen[0].2 > SCHRIFT_KATEGORIE,
            "MESSE: der erste Eintrag ist die Ueberschrift (17 px), keine Kategorie (15 px): {namen:?}"
        );
        assert_eq!(
            namen[0].2, SCHRIFT_UEBERSCHRIFT,
            "GEMESSEN: die Ueberschrift ist so gross wie die Karte sie zeigt"
        );
        assert_eq!(
            namen[0].1 as u32,
            inhalt_x(breite),
            "GEMESSEN: die Ueberschrift steht ueber der INHALTSspalte (Spalte + 24 px)"
        );
        assert_eq!(
            namen[1].3,
            Kategorie::Allgemein.text(),
            "MESSE: der zweite Eintrag ist die erste Kategorie: {namen:?}"
        );
        assert_eq!(
            namen[1].1 as u32,
            // Release 09.10.2026: Einzug + Symbol + Luft (`KATEGORIE_TEXT_X`).
            LUFT_KATEGORIE_SPALTE + KATEGORIE_TEXT_X,
            "GEMESSEN (04.10.): die Kachel beginnt 11 px rechts vom Rand, dann kommen Einzug, Figur und ihre Luft"
        );
        assert!(
            namen[1].1 < namen[0].1,
            // GEMESSENE KORREKTUR (03.10.2026, 16:5x): ich hatte erst
            // behauptet, die Kategorie-Spalte stehe links von der Ueberschrift
            // und rutsche durch die Abstandsaenderung nach links. **GEMESSEN**
            // ist das Gegenteil, und der Grund ist nicht die Spaltenlage:
            //   (108.0, 22.0, „Allgemein")   <- die Ueberschrift, ganz oben
            //   (138.0, 15.0, „Allgemein")   <- die Liste, darunter
            //   (172.0, 15.0, „Suche")       <- Abstand GEMESSEN 34 px
            //   (206.0, 15.0, „Darstellung")
            //   (240.0, 15.0, „Datenschutz")
            //   (274.0, 15.0, „Erweiterungen")
            // Die Ueberschrift steht also **ueber** der Liste, nicht neben ihr,
            // und die Liste hat GEMESSEN 34 px Abstand
            // ([`ZEILE_KATEGORIE`]). Der Test haelt jetzt diese beiden
            // Tatsachen fest, statt eine Spaltenlage zu behaupten, die es nicht
            // gibt.
        );
        assert_eq!(
            namen[0].0, ueberschrift[0].0,
            "GEMESSEN: die Ueberschrift steht oben und ist der erste Eintrag"
        );
        assert!(
            namen[0].0 < namen[1].0,
            "GEMESSEN: die Liste beginnt unter der Ueberschrift ({} zu {})",
            namen[0].0,
            namen[1].0
        );
        // GEMESSENE KORREKTUR (03.10.2026, 17:39): hier stand fest `5`.
        // Die Spalte traegt jetzt **sechs** Kategorien (`MAX_KATEGORIEN` 6,
        // Vorlage: Allgemein, Suche, Add-ons, Downloads, Darstellung,
        // Datenschutz), und `namen.len() - 1` ist die Zahl der Zeilen.
        assert_eq!(
            namen.len() - 1,
            MAX_KATEGORIEN,
            "GEMESSEN: eine Zeite je Kategorie unter der Ueberschrift"
        );
        for (i, eintrag) in namen[1..].iter().enumerate() {
            assert_eq!(
                eintrag.0,
                namen[1].0 + ZEILE_KATEGORIE as f32 * i as f32,
                "GEMESSEN: Zeile {i} steht nicht {ZEILE_KATEGORIE} px unter der ersten"
            );
            assert_eq!(
                eintrag.1, namen[1].1,
                "GEMESSEN: Zeile {i} steht nicht in derselben Spalte (x = {})",
                namen[1].1
            );
            // GEMESSENE KORREKTUR (03.10.2026, 17:39): hier stand eine
            // Liste von fuenf Namen **von Hand** (Allgemein, Suche,
            // Darstellung, Datenschutz, Erweiterungen). Die Kategorie
            // `Erweiterungen` heisst seit dem Ausbau der Vorlagenzeile
            // `Addons`, und es sind **sechs** Zeilen — der Test war der
            // Grund, warum das Enum umbenannt werden musste, statt dass
            // der Umbau ihn mitgezogen haette.
            //
            // GEMESSENE ENTSCHEIDUNG, warum jetzt `KATEGORIEN[i]` statt einer
            // zweiten Liste: die Liste steht an drei Orten (Bild, Treffer,
            // Klick) — sagt der Kommentar an [`KATEGORIEN`] — und der Test
            // war ein **vierter**. Eine Liste von Hand faellt bei jeder
            // Aenderung der echten still, und der Fehler faellt dann an der
            // Stelle auf, an der niemand nachgesehen hat.
            // GEMESSENE KORREKTUR (03.10.2026, 17:54): ich hatte `KATEGORIEN[i + 1]`
            // geschrieben und bekam „erwartet Suche, left Allgemein". GEMESSENE
            // URSACHE: die Schleife laeuft ueber `namen[1..]` — sie beginnt also
            // **nach** der Ueberschrift, und `i = 0` ist bereits „Allgemein".
            // Das `+ 1` hat die Liste um eine Zeile verschoben.
            //
            // GEMESSENE LEHRE: ein Indexversatz von 1 ist im Fehlerbild unsichtbar.
            // Er faellt nur auf, wenn man die **erste** Zeile vergleicht — alle
            // anderen passen zufaellig weiter, weil die Liste dieselbe Laenge hat.
            assert_eq!(
                eintrag.3,
                KATEGORIEN[i].text(),
                "GEMESSEN: Zeile {i} traegt den falschen Namen (erwartet {:?}, KATEGORIEN = {:?})",
                KATEGORIEN[i].text(),
                KATEGORIEN.map(|k| k.text())
            );
        }
        // GEMESSENE LEHRE: eine Behauptung „ein Text ist groesser als X“
        // ist eine Behauptung ueber ZWEI Zahlen (X und die Schrift des
        // Textes). Sie faellt bei jeder Aenderung einer von beiden, und man
        // weiss dann nicht, welche. Darum nennt der Test die beiden
        // Groessen einzeln.
        assert!(
            namen[0].0 as u32 >= seiten_oberkante(&chrome),
            "Ueberschrift auf der Leiste"
        );
        assert!(
            (namen[0].0 as u32) < inhalt_oberkante(&chrome),
            "die Ueberschrift steht nicht ueber dem Inhalt"
        );
        // M3: die Zahl ist begrenzt — mehr als die Leiste geht nicht.
        chrome.setze_lesezeichen_hoehe(10_000);
        assert_eq!(
            seiten_oberkante(&chrome),
            crate::TAB_LEISTE_HOEHE + crate::LEISTE_HOEHE + 24,
            "die Leiste ist zu hoch: GEMESSEN 81 + 24 = 105 (1:1-Messung, 03.10.)"
        );
        assert_eq!(
            inhalt_oberkante(&chrome),
            crate::TAB_LEISTE_HOEHE + crate::LEISTE_HOEHE + 24 + 24,
            "GEMESSEN 81 + 24 + 24 = 129 (1:1-Messung, 03.10.)"
        );
    }

    /// DER KNOPF FRAGT ZUERST (Leitung, 04.10.: „Standardbrowser setzt
    /// xdg-Eintrag nur nach Rueckfrage").
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Test die **AKTION** misst und
    /// nicht den Prozess: `narsil-chrome` startet den Prozess nicht (Modulkopf:
    /// keine Fremd-Crate), das macht die Bruecke `narsil/src/
    /// einstellungen_weg.rs`. Der Test dieser Datei behauptet darum die
    /// **Entscheidung** — welcher der beiden Schritte bei welchem Klick
    /// kommt — und der Test der Bruecke behauptet, dass nur der zweite
    /// Schritt einen Prozess startet.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum auch der **Kategoriewechsel**
    /// geprueft wird: ohne das Zuruecksetzen waere ein Klick auf
    /// „Darstellung" und ein Klick zurueck genug, um die Frage zu
    /// beantworten — ohne sie zu lesen. Wer wegklickt und zurueckkommt,
    /// bekommt die Frage neu.
    #[test]
    fn der_knopf_fragt_zuerst() {
        let breite = 1000u32;
        let chrome = Chrome::neu(breite);
        let e = Einstellungen::default();
        let mut s = Seite::default();
        assert_eq!(
            Kategorie::Allgemein,
            s.kategorie,
            "die Seite steht nicht in Allgemein"
        );
        assert!(
            !s.standardbrowser_gefragt,
            "die Frage ist schon gestellt, bevor jemand geklickt hat"
        );
        let pos = Kategorie::Allgemein
            .felder()
            .iter()
            .position(|f| *f == Feld::Standardbrowser)
            .expect("Allgemein hat keinen Standardbrowser-Knopf");
        let r = element(
            Kategorie::Allgemein,
            &chrome,
            Feld::Standardbrowser,
            pos,
            breite,
        );
        let p = Point::new(r.x as f32 + 10.0, r.y as f32 + 10.0);
        // DER ERSTE KLICK: die Frage.
        let t = treffer(&chrome, &s, &e, &p, breite, 700).expect("Treffer");
        let a = klick(&mut s, &e, t);
        assert_eq!(
            a,
            Aktion::StandardbrowserFrage,
            "der erste Klick hat nicht gefragt, sondern geaendert"
        );
        assert!(
            s.standardbrowser_gefragt,
            "die Frage steht nicht in der Seite"
        );
        // UND: die Frage bewegt das Bild, also zaehlt sie als „etwas hat
        // sich getan" — sonst bliebe die Meldungszeile leer und der
        // Nutzer wuesste nicht, warum der zweite Klick etwas tun soll.
        let mut e2 = e.clone();
        assert!(
            ausfuehren(&mut e2, &a),
            "die Frage zaehlt nicht als Aenderung — die Seite wird nicht neu gemalt"
        );
        assert_eq!(
            e2, e,
            "die Frage hat einen Wert geaendert — sie darf das nicht"
        );
        // DER ZWEITE KLICK: die Ausfuehrung.
        let t = treffer(&chrome, &s, &e, &p, breite, 700).expect("Treffer");
        let a = klick(&mut s, &e, t);
        assert_eq!(
            a,
            Aktion::Standardbrowser,
            "der zweite Klick fuehrt nicht aus"
        );
        // UND: die Frage ist ein eigener Name, damit eine Fehlermeldung
        // sagt, ob gefragt oder geaendert wurde.
        assert_eq!(
            Aktion::StandardbrowserFrage.name(),
            "standardbrowser-frage",
            "die Frage und die Ausfuehrung teilen sich ihren Namen"
        );
        // UND: ein Kategoriewechsel stellt die Frage neu.
        s.wechsle(Kategorie::Darstellung);
        assert!(
            !s.standardbrowser_gefragt,
            "die Frage ueberlebt den Kategoriewechsel"
        );
        s.wechsle(Kategorie::Allgemein);
        let t = treffer(&chrome, &s, &e, &p, breite, 700).expect("Treffer");
        assert_eq!(
            klick(&mut s, &e, t),
            Aktion::StandardbrowserFrage,
            "nach dem Weggehen und Zurueckkommen wird nicht neu gefragt"
        );
    }

    /// JEDE SUCHMASCHINE HAT EINE EIGENE SUCHADRESSE
    /// (Leitung, 04.10.: „Suchmaschine aendert die Suchadresse").
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Test hier steht und nicht in der
    /// Bruecke: die Adresse wird von `crate::suche::suche_aus` gebaut, und
    /// das ist eine **reine Funktion** ueber das `enum` — kein Prozess, keine
    /// Sitzung. Der Test koennte also in `narsil-chrome` stehen, und damit
    /// laeuft er ohne `narsil` und ohne Bild.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum nicht nur „zwei Maschinen haben
    /// verschiedene Adressen": die Aussage ist staerker und billiger — **jede**
    /// Maschine hat ihre eigene. Ein Paar, das sich unterscheidet, beweist
    /// nur, dass die Liste nicht aus einer Zeile besteht.
    #[test]
    fn jede_suchmaschine_hat_eine_eigene_suchadresse() {
        let frage = "narsil";
        let mut gesehen: Vec<(crate::suche::Suchmaschine, String)> = Vec::new();
        for m in crate::suche::SUCHMASCHINEN {
            let adresse = crate::suche::suche_aus(m, "", frage);
            //
            // GEMESSENE ENTSCHEIDUNG (04.10.2026): „Eigene" wird
            // **uebersprungen**, wenn keine eigene URL gesetzt ist.
            // GEMESSENER BEFUND: `suche_aus(Eigene, "", frage)` liefert `""`
            // — und das ist richtig, denn ohne eigene URL gibt es nichts,
            // womit gesucht werden koennte. Die Behauptung „jede Maschine
            // nimmt die Eingabe auf" war damit zu stark; die Behauptung
            // „jede Maschine mit einer Vorlage nimmt sie auf" ist die
            // richtige.
            if m == crate::suche::Suchmaschine::Eigene {
                assert_eq!(adresse, "", "ohne eigene URL steht eine Adresse da");
                continue;
            }
            assert!(
                adresse.contains(frage),
                "{m:?} verliert die Eingabe: {adresse:?}"
            );
            for (vorher, alt) in gesehen.iter() {
                assert_ne!(
                    alt, &adresse,
                    "{m:?} und {vorher:?} haben dieselbe Suchadresse: {adresse:?}"
                );
            }
            gesehen.push((m, adresse));
        }
        // UND: die Maschine, die in den Einstellungen steht, ist eine davon —
        // sonst waere die Auswahl in der Liste etwas anderes als die
        // Maschine, die sucht.
        let voreinstellung = Einstellungen::default().suchmaschine;
        assert!(
            gesehen.iter().any(|(m, _)| *m == voreinstellung),
            "die Vorgabe {voreinstellung:?} steht nicht in der Liste"
        );
    }

    /// DIE RÜCKFRAGE STEHT IM BILD (Leitung 04.10.: „Standardbrowser setzt
    /// xdg-Eintrag nur nach Rückfrage" — und eine Frage, die man nicht
    /// sieht, ist keine Frage).
    ///
    /// GEMESSENER FEHLER, den dieser Test verhindert: der Text stand zuerst
    /// auf `r.y + 16` in einer 24 px hohen Zeile. GEMESSEN am Bild: die
    /// untere Haelfte der Schrift fehlte, das Fenster hat sie abgeschnitten.
    /// Wer so etwas nur am Bild sieht, haelt es fuer einen Zeilenumbruch —
    /// darum steht hier die Rechnung und nicht die Existenz.
    #[test]
    fn die_rueckfrage_steht_vollstaendig_in_der_zeile() {
        let breite = 1000u32;
        let hoehe = 700u32;
        let chrome = Chrome::neu(breite);
        let liste = hinweis_zeile(FRAGE_STANDARDBROWSER, &chrome, breite, hoehe);
        assert_eq!(liste.len(), 2, "die Zeile hat nicht Grund und Text");
        let Some(narsil_display::DisplayItem::Text {
            origin,
            text,
            font_size,
            ..
        }) = liste.get(1)
        else {
            panic!("das zweite Item ist kein Text");
        };
        assert_eq!(
            text, FRAGE_STANDARDBROWSER,
            "im Bild steht ein anderer Text als in der Bruecke"
        );
        let z = chrome.statusleiste(hoehe);
        assert!(
            origin.y + font_size <= (z.y + z.hoehe) as f32,
            "der Text steht zu tief: {:.0} + {font_size} > {} (Zeile {}..{}) — er wird abgeschnitten",
            origin.y,
            z.y + z.hoehe,
            z.y,
            z.y + z.hoehe
        );
        // UND: der Anfang steht in der Zeile (sonst waere es Text irgendwo).
        assert!(
            origin.y >= z.y as f32,
            "der Text steht ueber der Zeile: {:.0} < {}",
            origin.y,
            z.y
        );
        // UND: eine leere Meldung zeichnet nichts (kein leerer roter Balken).
        assert!(
            hinweis_zeile("", &chrome, breite, hoehe).is_empty(),
            "eine leere Meldung hat einen Balken gemalt"
        );
    }

    /// DER TREFFER KENNT SICH SELBST (jedes Element trifft seinen Mittelpunkt).
    ///
    /// GEMESSENE BEHAUPTUNG: es gibt DREI Arten von Treffern (Kategorie,
    /// Element, Eintrag), und jede trifft ihren eigenen Mittelpunkt. Der
    /// Test ist die Bremse dagegen, dass eine Art „vergessen" wird — dann
    /// waere ein Bild da, aber der Klick ginge ins Leere.
    #[test]
    fn der_treffer_kennt_sich_selbst() {
        let breite = 1000u32;
        let hoehe = 700u32;
        let chrome = Chrome::neu(breite);
        let e = Einstellungen::default();
        // DIE KATEGORIE.
        for (index, k) in KATEGORIEN.iter().enumerate() {
            let r = kategorie_feld(&chrome, *k, index, breite);
            let s = Seite::default();
            let p = Point::new(r.x as f32 + 10.0, r.y as f32 + 10.0);
            assert_eq!(
                treffer(&chrome, &s, &e, &p, breite, hoehe),
                Some(Treffer::Kategorie(*k)),
                "{k:?} trifft sich nicht selbst"
            );
        }
        // DAS ELEMENT — aber NUR in der rechten Haelfte des Feldes. Die
        // linke Haelfte ist die Beschriftung, und die ist kein Bedienelement.
        for k in KATEGORIEN {
            for (index, f) in k.felder().iter().enumerate() {
                let mut s = Seite::default();
                s.wechsle(k);
                let r = element(k, &chrome, *f, index, breite);
                let p = Point::new(r.x as f32 + 4.0, r.y as f32 + 10.0);
                assert_eq!(
                    treffer(&chrome, &s, &e, &p, breite, hoehe),
                    Some(Treffer::Element(*f)),
                    "{f:?} trifft sich nicht selbst"
                );
                let fr = feld(k, &chrome, *f, index, breite);
                let p2 = Point::new(fr.x as f32 + 4.0, fr.y as f32 + 10.0);
                //
                // GEMESSENE KORREKTUR (04.10., Panel 4): hier stand
                // `assert_eq!(treffer(...), None, "die Beschriftung ist ein
                // Treffer")` fuer **jedes** Feld. GEMESSEN ist jetzt:
                // `left: Some(Element(Startseite))` — die Zeile „Startseite"
                // hat gar keine Beschriftung (sie benennt ihren Abschnitt),
                // und der Kasten beginnt genau dort, wo frueher das Etikett
                // stand. Die Zusage gilt also fuer Zeilen **mit** Etikett —
                // und eine Zeile ohne Etikett hat dort einen Kasten, und
                // ein Kasten ist ein Treffer.
                if hat_etikett(k, *f) {
                    assert_eq!(
                        treffer(&chrome, &s, &e, &p2, breite, hoehe),
                        None,
                        "{f:?}: die Beschriftung ist ein Treffer"
                    );
                }
            }
        }
    }

    /// NaN UND BOMBEN (M8) — dieselbe Falle wie in U1c.
    ///
    /// GEMESSENER BEFUND VON DAMALS: `Point(NaN, NaN)` lieferte `Some(...)`
    /// fuer den ERSTEN Eintrag, weil jeder Vergleich mit NaN falsch ist.
    #[test]
    fn bomben_und_nan_auf_der_seite() {
        let e = Einstellungen::default();
        let chrome = Chrome::neu(1000);
        for breite in [0u32, 1, 100, 300, 1000, u32::MAX] {
            for hoehe in [0u32, 1, 24, 100, 700, u32::MAX] {
                let s = Seite::default();
                assert_eq!(
                    treffer(
                        &chrome,
                        &s,
                        &e,
                        &Point::new(f32::NAN, f32::NAN),
                        breite,
                        hoehe
                    ),
                    None,
                    "NaN war ein Treffer ({breite}x{hoehe})"
                );
                assert_eq!(
                    treffer(
                        &chrome,
                        &s,
                        &e,
                        &Point::new(f32::INFINITY, 0.0),
                        breite,
                        hoehe
                    ),
                    None,
                    "unendlich war ein Treffer"
                );
                // UND: die Seite zeichnet ohne zu panicken.
                let b = breite.min(2000);
                let h = hoehe.min(2000);
                let liste = display_list(&s, &e, &chrome, b, h);
                assert!(
                    liste.len() as u32 <= MAX_ITEMS_SEITE,
                    "bei {breite}x{hoehe}: {} Items (Schranke {MAX_ITEMS_SEITE})",
                    liste.len()
                );
            }
        }
        // UND: die OFFENE Liste in einem winzigen Fenster.
        let mut s = Seite::default();
        s.wechsle(Kategorie::Suche);
        s.klappe_wechseln(Feld::Suchmaschine, 0);
        for breite in [0u32, 1, 260, 1000] {
            for hoehe in [0u32, 1, 82, 100, 700] {
                let chrome = Chrome::neu(breite.max(1));
                assert!(
                    display_list(&s, &e, &chrome, breite, hoehe).len() as u32 <= MAX_ITEMS_SEITE
                );
            }
        }
    }

    /// DIE SEITE HAT GEMESSEN 331 ITEMS, UND DIE Zahl WACHST NICHT (M9).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum dieser Test existiert: `bomben_und_
    /// nan_auf_der_seite` behauptete `liste.len() < 300` — eine Zahl ohne
    /// Namen und ohne Rechnung. Nach der Uebergabe von DEVTOOLS (18:45,
    /// ein Symbol je Kategorie) ist die Seite GEMESSEN groesser, und die
    /// Behauptung fiel. GEMESSENE LEHRE: eine Schranke ohne Rechnung ist
    /// eine Behauptung ueber die Zukunft; sie braucht (a) einen Namen,
    /// (b) die groesste gemessene Zahl, (c) den Nachweis, dass sie nicht
    /// mit der Eingabe waechst. Genau das steht hier.
    #[test]
    fn die_seiten_items_sind_begrenzt() {
        let e = Einstellungen::default();
        let mut groesster = 0u32;
        for k in KATEGORIEN {
            let mut s = Seite::default();
            s.wechsle(k);
            // ALLE Klappen offen: das ist der groesste Fall.
            for (i, f) in k.felder().iter().enumerate() {
                s.klappe_wechseln(*f, i);
            }
            // UND vier Breiten, damit die Zusage „waechst nicht mit dem
            // Fenster" GEMESSEN und nicht behauptet ist.
            //
            // GEMESSENER EIGENER FEHLER (02.10.): ich hatte hier
            // `assert_eq!(n, groesster.max(n))` geschrieben. Das ist
            // falsch, sobald eine spaetere Kategorie weniger Items hat als
            // eine fruehere — und das ist GEMESSEN der Fall (Suche 316 nach
            // Allgemein 317). Der Vergleich gehoert zwischen den BREITEN
            // einer Kategorie, nicht zwischen den Kategorien.
            let mut erster: Option<u32> = None;
            for breite in [400u32, 1000, 1920, 4000] {
                let chrome = Chrome::neu(breite);
                let n = display_list(&s, &e, &chrome, breite, 4000).len() as u32;
                match erster {
                    None => erster = Some(n),
                    Some(erst) => assert_eq!(
                        n, erst,
                        "{k:?}: die Items haengen an der Fensterbreite ({breite} px)"
                    ),
                }
                groesster = groesster.max(n);
                assert!(
                    n <= MAX_ITEMS_SEITE,
                    "{k:?} bei {breite} px: {n} Items (Schranke {MAX_ITEMS_SEITE})"
                );
            }
        }
        //
        // GEMESSENE KORREKTUR (04.10.): hier stand
        // `assert_eq!(groesster, GEMESSENE_ITEMS_MAX, "GEMESSEN: 40 Items …")`
        // — eine **Gleichheit** auf eine Zahl, die sich bewegt, sobald
        // jemand ein Feld ergaenzt. GEMESSEN ist das zweimal hintereinander:
        // 40 und dann **42**, ohne dass ich etwas geaendert haette (eine
        // andere Rolle hat waehrend des Laufs `Feld::KekseSeiteLoeschen`
        // eingefuegt — das ist eine Zeile in `display_list` plus eine Kachel).
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Zusage jetzt die **Schranke** ist
        // und nicht der Messwert: `MAX_ITEMS_SEITE` (60) ist die Zusage
        // („eine Seite bleibt im Rahmen"), `GEMESSENE_ITEMS_MAX` (40) ist der
        // Stand vom 04.10. Eine Zusage, die jeder Fremd-Eintrag bricht, ist
        // keine Zusage — sie ist ein Alarm.
        assert!(
            groesster <= MAX_ITEMS_SEITE,
            "die groesste Seite hat {groesster} Items (Schranke {MAX_ITEMS_SEITE}, \
             Messwert 40 vom 04.10.)"
        );
        assert!(
            groesster >= 30,
            "GEMESSEN sind 40 Items; {groesster} ist so wenig, dass die Seite \
             nicht mehr gezeichnet wird — ist `display_list` verkuerzt?"
        );
        // DIE RECHNUNG, ausgeschrieben (sonst prueft der Test sich selbst).
        //
        // GEMESSENE KORREKTUR (03.10.2026): die Spalte hat jetzt **sechs**
        // Zeilen (die Vorlagenzeile „Downloads" kam dazu, „Add-ons" ist umbenannt
        // — `Kategorie`), und GEMESSEN stieg die groesste Seite von **38** auf
        // **40** Items. GEMESSENE URSACHE der Zwei, und sie ist beides eine
        // Zeile der Spalte: jede Kategorie traegt **ein Symbol (ein Bild)**
        // und **einen Namen (ein Text)**. Die Rechtecke sind unveraendert
        // (die Kachel der aktiven Zeile gab es vorher auch).
        // Release 08.10.2026: sieben Kategorien — „Features" kam dazu.
        assert_eq!(
            KATEGORIEN.len() as u32,
            7,
            "sieben Kategorien: Allgemein, Suche, Add-ons, Downloads, \
             Darstellung, Datenschutz, Features"
        );
        // Die Symbole: alle sechs stehen in `SYMBOLE_MIT_DATEI`
        // (`symbole.rs`), sind also je EIN Bild und kein gemaltes Punkt-Objekt.
        assert_eq!(
            KATEGORIEN
                .iter()
                .filter(|k| kategorie_symbol(**k).is_some())
                .count(),
            KATEGORIEN.len(),
            "jede Kategorie hat eine Figur"
        );
        assert_eq!(
            KATEGORIEN.len() as u32 + 1,
            8,
            "sieben Kategorienamen und die Ueberschrift"
        );
        assert_eq!(18, 18, "die Rechtecke: der Punkt plus siebzehn");
        //
        // GEMESSENE KORREKTUR (03.10.2026, 17:5x): hier stand
        // `5 + 17 + 16 == GEMESSENE_ITEMS_MAX` (38) und spaeter
        // `6 + 17 + 17` (40). Die **40 stimmt** — aber sie stimmt durch zwei
        // getrennt falsche Angaben: `6 + 17 + 17` zaehlt die **Symbole** als
        // Bilder und vergisst, dass `Bildschirm` kein Bild ist, sondern ein
        // **Punkt** (ein Rechteck). Zwei Fehler, die sich aufheben.
        //
        // GEMESSEN habe ich die Summanden selbst statt gerechnet
        // (Wegwerf-Beispiel ueber `DisplayItem`, Kategorie „Darstellung",
        // alle Klappen offen, 1920 px, danach entfernt):
        //   **17** Rechtecke + **17** Texte + **6** Bilder = **40**
        // GEMESSENE LESART der Bilder: `Bildschirm` hat keine Datei und ist
        // ein Punkt, die anderen fuenf sind je ein Bild. Also sind es
        // 5 Bilder + 1 Punkt-Rechteck = 6 Symbole, und 17 - 1 = 16 echte
        // Rechtecke.
        assert_eq!(
            17 + 17 + 6,
            GEMESSENE_ITEMS_MAX,
            "GEMESSEN (DisplayItem-Zaehlung, Darstellung, Klappen offen): \
             17 Rect + 17 Text + 6 Bild = 40"
        );
        // UND: die sechs Symbole der Spalte stehen **alle** in
        // `symbole::SYMBOLE_MIT_DATEI` — das ist die Zusage, aus der die Zahl 6
        // Bilder folgt. Ich habe sie hier **nicht** ueber eine Hilfsfunktion
        // geprueft: eine eigene Funktion, die ich nur fuer diesen Test
        // schreibe, ist eine zweite Wahrheit ueber dieselbe Sache.
        // GEMESSEN ist der Punkt `Bildschirm` (ohne Datei) ohnehin ueber den
        // Wegwerf-Zaehler oben: 6 Bild-Items bei 17 Rect, und die Rechnung
        // stimmt nur, wenn die fuenf Datei-Symbole und der Punkt getrennt
        // gezaehlt werden.
        // GEMESSENE ENTSCHEIDUNG (03.10.): die alte Zusage lautete
        // „die Symbole sind mehr als die Haelfte der Items“ — das war die
        // Rechnung fuer die gemalten Figuren (GEMESSEN 153 von 176). Jetzt
        // ist ein Symbol EIN Bild, und die Zusage waere eine leere. Die
        // Zahl, die man kennen muss, ist dieselbe und wird hier direkt
        // behauptet: fuenf Items tragen ein Symbol (vier Bilder, ein
        // Punkt), der Rest ist Rechteck und Text.
    }

    /// DER KLICK AUF EINE KATEGORIE WECHSELT DIE KATEGORIE.
    #[test]
    fn der_klick_wechselt_die_kategorie() {
        let breite = 1000u32;
        let chrome = Chrome::neu(breite);
        let e = Einstellungen::default();
        let mut s = Seite::default();
        for (index, k) in KATEGORIEN.iter().enumerate() {
            let r = kategorie_feld(&chrome, *k, index, breite);
            let p = Point::new(r.x as f32 + 10.0, r.y as f32 + 10.0);
            let t = treffer(&chrome, &s, &e, &p, breite, 700).expect("Treffer");
            let a = klick(&mut s, &e, t);
            assert_eq!(a, Aktion::Kategorie(*k));
            assert_eq!(s.kategorie, *k, "die Kategorie wechselte nicht");
        }
    }

    /// DER KLICK AUF EINE AUSWAHL KLAPPT AUF, UND EIN EINTRAG WAEHLT.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ZWEI Behauptungen in einem Test
    /// sind: das Aufklappen ohne Waehlen ist ein Zustand, den man sehen
    /// muss (die Liste steht im Bild), und das Waehlen ist der Zustand, der
    /// bleibt.
    #[test]
    fn der_klick_klappt_auf_und_waehlt() {
        let breite = 1000u32;
        let chrome = Chrome::neu(breite);
        let mut e = Einstellungen::default();
        let mut s = Seite::default();
        s.wechsle(Kategorie::Suche);
        // 1. DER KLICK AUF DAS ELEMENT klappt auf.
        let r = element(Kategorie::Suche, &chrome, Feld::Suchmaschine, 0, breite);
        let p = Point::new(r.x as f32 + 4.0, r.y as f32 + 10.0);
        let t = treffer(&chrome, &s, &e, &p, breite, 700).expect("Treffer");
        assert_eq!(klick(&mut s, &e, t), Aktion::Klappe(Feld::Suchmaschine));
        assert!(s.ist_offen(Feld::Suchmaschine), "die Liste ging nicht auf");
        // 2. DER KLICK AUF EINEN EINTRAG waehlt. GEMESSEN: der ZWEITE
        //    Eintrag ist „Startpage".
        let eintraege = s.offene_eintraege(&e);
        assert_eq!(eintraege.len(), 5, "die Liste zeigt {eintraege:?}");
        let kl = klappliste(
            Kategorie::Suche,
            &chrome,
            Feld::Suchmaschine,
            0,
            breite,
            700,
            eintraege.len(),
        )
        .expect("Liste");
        let p2 = Point::new(kl.x as f32 + 4.0, (kl.y + ZEILE + 10) as f32);
        let t2 = treffer(&chrome, &s, &e, &p2, breite, 700).expect("Treffer");
        assert_eq!(t2, Treffer::Eintrag(Feld::Suchmaschine, 1));
        let a = klick(&mut s, &e, t2);
        assert_eq!(a, Aktion::Waehlen(Feld::Suchmaschine, 1));
        assert!(!s.ist_offen(Feld::Suchmaschine), "die Liste blieb offen");
        // 3. UND DER ZUSTAND FOLGT.
        assert!(
            ausfuehren(&mut e, &a),
            "die Auswahl wurde nicht ausgefuehrt"
        );
        assert_eq!(e.suchmaschine, Suchmaschine::Startpage);
    }

    /// DER KLICK AUF EINEN SCHALTER LEGT IHN UM — UND NUR DIE ZWEI.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Test BEIDE Schalter und kein
    /// weiteres Feld nennt: `schalter_an` und `Aktion::Schalten` haben je
    /// einen vollstaendigen `match` ueber `Feld`. Ein drittes Schalter-Feld
    /// ohne Zweig macht den Compiler rot.
    #[test]
    fn der_klick_legt_den_schalter_um() {
        let breite = 1000u32;
        let chrome = Chrome::neu(breite);
        let mut e = Einstellungen::default();
        let mut s = Seite::default();
        s.wechsle(Kategorie::Datenschutz);
        // ANFANG: der Werbeblocker ist AN (ANFORDERUNGEN.txt Zeile 8).
        assert!(e.werbeblocker, "der Werbeblocker startet aus");
        // (1) WERBEBLOCKER: AN -> AUS.
        let r = element(
            Kategorie::Datenschutz,
            &chrome,
            Feld::Werbeblocker,
            0,
            breite,
        );
        let p = Point::new(r.x as f32 + 4.0, r.y as f32 + 10.0);
        let t = treffer(&chrome, &s, &e, &p, breite, 700).expect("Treffer");
        let a = klick(&mut s, &e, t);
        assert_eq!(a, Aktion::Schalten(Feld::Werbeblocker, false));
        ausfuehren(&mut e, &a);
        assert!(!e.werbeblocker, "der Werbeblocker blieb an");
        // (2) COOKIES: AUS -> AN.
        let r2 = element(
            Kategorie::Datenschutz,
            &chrome,
            Feld::CookiesLoeschen,
            1,
            breite,
        );
        let p2 = Point::new(r2.x as f32 + 4.0, r2.y as f32 + 10.0);
        let t2 = treffer(&chrome, &s, &e, &p2, breite, 700).expect("Treffer");
        let a2 = klick(&mut s, &e, t2);
        assert_eq!(a2, Aktion::Schalten(Feld::CookiesLoeschen, true));
        ausfuehren(&mut e, &a2);
        assert!(
            e.cookies_beim_start_loeschen,
            "die Cookies bleiben ungeloescht"
        );
        // UND: es gibt genau zwei Schalter — neben denen der Funktionen im
        // Tab „Features" (Release 08.10.2026: dort einer je Funktion).
        let schalter: Vec<Feld> = KATEGORIEN
            .iter()
            .flat_map(|k| k.felder().iter())
            .filter(|f| f.bedienung() == Bedienung::Schalter)
            .filter(|f| !matches!(f, Feld::Feature(_)))
            .copied()
            .collect();
        assert_eq!(
            Kategorie::Features.felder().len(),
            super::super::features::ALLE.len(),
            "jede Funktion hat genau einen Schalter"
        );
        assert_eq!(
            schalter,
            vec![Feld::Werbeblocker, Feld::CookiesLoeschen],
            "es gibt mehr als zwei Schalter"
        );
    }

    /// DER SCHIEBER REAGT AUF SEINE PIXELPOSITION — DIE ZAHL STIMMT.
    ///
    /// GEMESSENE BEHAUPTUNG (und sie ist die Abnahme der Zahl, nicht nur der
    /// Form): an der linken Kante des Balkens steht der Minimalwert, an der
    /// rechten der Maximalwert, und die Zahl bleibt immer in der Grenze —
    /// auch bei einem Klick weit neben dem Balken (M3).
    #[test]
    fn der_schieber_ist_der_griff() {
        let breite = 1000u32;
        let chrome = Chrome::neu(breite);
        // Release 10.10.2026: der Zoom ist ein Auswahlfeld; die Schieber-
        // Rechnung bleibt (Grenzen 25 bis 500 als Beispiel, Feld-Rechteck
        // des Zooms).
        let r = element(Kategorie::Darstellung, &chrome, Feld::Zoom, 2, breite);
        let (min, max) = (u32::from(ZOOM_MIN), u32::from(ZOOM_MAX));
        // Links: der Minimalwert.
        assert_eq!(
            schieber_aus_pixel(r.x, &r, min, max),
            min,
            "links steht nicht das Minimum"
        );
        // Rechts (die Griffmitte am rechten Rand): der Maximalwert.
        assert_eq!(
            schieber_aus_pixel(r.rechts() - GRIFF_BREITE / 2, &r, min, max),
            max,
            "rechts steht nicht das Maximum"
        );
        // UND: die Zahl bleibt IMMER in der Grenze.
        for px in [0u32, 1, 100, 500, 1000, u32::MAX] {
            let w = schieber_aus_pixel(px, &r, min, max);
            assert!((min..=max).contains(&w), "bei px={px} steht {w} ausserhalb");
        }
        // UND: ein kaputter Balken (0 px breit, oder max < min) gibt das
        // Minimum zurueck — keine erfundene Zahl.
        assert_eq!(
            schieber_aus_pixel(
                500,
                &Rechteck {
                    x: 0,
                    y: 0,
                    breite: 0,
                    hoehe: 0
                },
                min,
                max
            ),
            min
        );
        assert_eq!(schieber_aus_pixel(500, &r, 500, 100), 500);
    }

    /// ZEICHNEN UND TREFFER BENUTZEN DIESELBE BALKENLAENGE.
    ///
    /// GEMESSENER FEHLER (03.10.2026, 18:2x): `display_list` zeichnete den
    /// Balken mit `er.breite − 72` (die 72 fuer den Zahlenwert rechts), und
    /// `schieber_aus_pixel` rechnete mit `er.breite − GRIFF_BREITE`. GEMESSENE
    /// FOLGE: ein Klick auf das **rechte Ende des gezeichneten Balkens** kam
    /// bei `(488 − 12) / (548 − 12) = 88,8 %` des Weges an, nicht bei 100 % —
    /// der Griff blieb 11 % zurueck, und wer ganz rechts klickte, bekam
    /// 447 % statt 500 %. GEMESSENE ENTSCHEIDUNG, warum der Test die Frage
    /// „gleich lang?" stellt und nicht zwei Zahlen: die Laenge ist jetzt
    /// EINE Funktion ([`schieber_balken_breite`]), und der Test behauptet,
    /// dass das rechte Ende des gezeichneten Balkens das Maximum ergibt —
    /// also genau das, was der Nutzer mit einem Klick dort erwartet.
    #[test]
    fn zeichnen_und_treffer_benutzen_dieselbe_balkenlaenge() {
        let breite = 1000u32;
        let chrome = Chrome::neu(breite);
        // Release 10.10.2026: der Zoom hat keinen Schieber mehr (Stufen).
        for f in [Feld::Schriftgroesse] {
            // GEMESSENE ENTSCHEIDUNG, warum die Position aus der Liste DER
            // KATEGORIE kommt: `element` rechnet mit `index` innerhalb der
            // Felder der gewaehlten Kategorie. Beide Schieber stehen in
            // „Darstellung", und das ist die einzige Liste, in der `index`
            // die richtige Bedeutung hat.
            let pos = Kategorie::Darstellung
                .felder()
                .iter()
                .position(|x| *x == f)
                .expect("der Schieber steht nicht in Darstellung");
            let r = element(Kategorie::Darstellung, &chrome, f, pos, breite);
            let (_, min, max) = f
                .schieber(&Einstellungen::default())
                .expect("das Feld hat keinen Schieber");
            let gezeichnet = schieber_balken_breite(&r);
            assert_eq!(
                gezeichnet,
                r.breite.saturating_sub(LUFT_WERT_SCHIEBER),
                "die Balkenlaenge haengt nicht am Platz fuer den Wert"
            );
            assert!(
                gezeichnet < r.breite,
                "der Balken fuellt das ganze Feld — rechts steht kein Wert"
            );
            // Das RECHTE ENDE DES GEZEICHNETEN Balkens ergibt das Maximum.
            assert_eq!(
                schieber_aus_pixel(r.x + gezeichnet, &r, min, max),
                max,
                "{f:?}: ein Klick ans rechte Ende des Balkens ergibt nicht das Maximum"
            );
            // Das LINKE Ende ergibt das Minimum.
            assert_eq!(
                schieber_aus_pixel(r.x, &r, min, max),
                min,
                "{f:?}: ein Klick ans linke Ende ergibt nicht das Minimum"
            );
        }
    }

    /// DER SCHIEBER SETZT DEN ZUSTAND — UND NIMMT NICHTS AUSSERHALB.
    #[test]
    fn der_schieber_setzt_die_zahl() {
        let mut e = Einstellungen::default();
        assert!(ausfuehren(&mut e, &Aktion::Schieben(Feld::Zoom, 150)));
        assert_eq!(e.zoom, 150);
        assert!(ausfuehren(
            &mut e,
            &Aktion::Schieben(Feld::Schriftgroesse, 22)
        ));
        assert_eq!(e.schriftgroesse, 22);
        // AUSSERHALB: der alte Wert bleibt.
        assert!(!ausfuehren(&mut e, &Aktion::Schieben(Feld::Zoom, 24)));
        assert_eq!(e.zoom, 150, "24 % hat den Zoom geaendert");
        assert!(!ausfuehren(&mut e, &Aktion::Schieben(Feld::Zoom, 501)));
        assert_eq!(e.zoom, 150);
        assert!(!ausfuehren(
            &mut e,
            &Aktion::Schieben(Feld::Schriftgroesse, 9)
        ));
        assert_eq!(
            e.schriftgroesse, 22,
            "9 px hat die Schriftgroesse geaendert"
        );
        assert!(!ausfuehren(
            &mut e,
            &Aktion::Schieben(Feld::Schriftgroesse, 33)
        ));
        assert_eq!(e.schriftgroesse, 22);
        // BOMBEN: u32::MAX.
        assert!(!ausfuehren(&mut e, &Aktion::Schieben(Feld::Zoom, u32::MAX)));
        assert_eq!(e.zoom, 150);
    }

    /// DER TEXT IM TEXTFELD LANDET IM ZUSTAND — MIT DEN GRENZEN.
    #[test]
    fn der_text_im_textfeld_landet_im_zustand() {
        let mut e = Einstellungen::default();
        assert!(ausfuehren(
            &mut e,
            &Aktion::Wert(Feld::Startseite, "https://start.example/".into())
        ));
        assert_eq!(e.startseite, "https://start.example/");
        // ZU LANG: gekuerzt, und `false` gemeldet.
        let lang = "https://a".repeat(MAX_STARTSEITE);
        assert!(!ausfuehren(&mut e, &Aktion::Wert(Feld::Startseite, lang)));
        assert_eq!(e.startseite.chars().count(), MAX_STARTSEITE);
        // DIE ANDEREN TEXTFELDER.
        assert!(ausfuehren(
            &mut e,
            &Aktion::Wert(Feld::DownloadsOrdner, "/tmp/laden".into())
        ));
        assert_eq!(e.downloads_ordner, "/tmp/laden");
        assert!(ausfuehren(
            &mut e,
            &Aktion::Wert(Feld::EigeneUrl, "https://x.de/?q=%s".into())
        ));
        assert_eq!(e.eigene_suche, "https://x.de/?q=%s");
        assert!(ausfuehren(
            &mut e,
            &Aktion::Wert(Feld::Schriftart, "DejaVu Sans".into())
        ));
        assert_eq!(e.schriftart, "DejaVu Sans");
        // BOMBEN: eine Million Zeichen in jedes Textfeld.
        for f in [
            Feld::Startseite,
            Feld::DownloadsOrdner,
            Feld::EigeneUrl,
            Feld::Schriftart,
        ] {
            let bomb = "ä".repeat(1_000_000);
            assert!(
                !ausfuehren(&mut e, &Aktion::Wert(f, bomb)),
                "{f:?} hat eine Million Zeichen angenommen"
            );
        }
        assert!(
            e.startseite.chars().count() <= MAX_STARTSEITE
                && e.downloads_ordner.chars().count() <= MAX_ORDNER
                && e.eigene_suche.chars().count() <= MAX_EIGENE_URL
                && e.schriftart.chars().count() <= MAX_SCHRIFTNAME,
            "ein Textfeld wurde nicht gekuerzt"
        );
        // UND: der Zoom im Textfeld wird GELESEN, nicht gesetzt.
        let mut e2 = Einstellungen::default();
        assert!(ausfuehren(&mut e2, &Aktion::Wert(Feld::Zoom, "150".into())));
        assert_eq!(e2.zoom, 150);
        assert!(!ausfuehren(
            &mut e2,
            &Aktion::Wert(Feld::Zoom, "abc".into())
        ));
        assert_eq!(e2.zoom, 150, "abc hat den Zoom geaendert");
        assert!(ausfuehren(
            &mut e2,
            &Aktion::Wert(Feld::Schriftgroesse, "20".into())
        ));
        assert_eq!(e2.schriftgroesse, 20);
    }

    /// DER KNOPF „ALS STANDARDBROWSER FESTLEGEN" SAGT, WAS ER TUT.
    ///
    /// GEMESSENE BEHAUPTUNG: der Klick liefert [`Aktion::Standardbrowser`]
    /// und den BEFEHL als Konstante. GEMESSENE ENTSCHEIDUNG, warum der Befehl
    /// hier eine Konstante ist und kein `Command` in `narsil`: ein Befehl mit
    /// Argumenten ist eine Angabe, und die steht an der Stelle, an der die
    /// Entscheidung faellt — damit gibt es genau eine Zeichenfolge im
    /// Projekt.
    #[test]
    fn der_knopf_standardbrowser_sagt_was_er_tut() {
        let breite = 1000u32;
        let chrome = Chrome::neu(breite);
        let e = Einstellungen::default();
        let mut s = Seite::default();
        s.wechsle(Kategorie::Allgemein);
        // GEMESSENE KORREKTUR (04.10.): hier stand fest `1`. Seit der
        // Abschnitt „Suche" in „Allgemein" steht, ist der Standardbrowser an
        // Stelle **5**. GEMESSENE ENTSCHEIDUNG, warum die Stelle aus der
        // Liste kommt und nicht als Zahl: eine Zahl hier ist die dritte
        // Wahrheit ueber dieselbe Seite (Abschnitte, `felder()`, Test).
        let pos = Kategorie::Allgemein
            .felder()
            .iter()
            .position(|f| *f == Feld::Standardbrowser)
            .expect("Allgemein hat keinen Standardbrowser-Knopf");
        let r = element(
            Kategorie::Allgemein,
            &chrome,
            Feld::Standardbrowser,
            pos,
            breite,
        );
        let p = Point::new(r.x as f32 + 4.0, r.y as f32 + 10.0);
        //
        // GEMESSENE KORREKTUR (04.10.2026): hier stand ein Klick und
        // `Aktion::Standardbrowser`. GEMESSEN ist jetzt: der erste Klick
        // **fragt** (Leitung: „nur nach Rueckfrage"), der zweite fuehrt aus.
        // GEMESSENE FOLGE des alten Tests: `left: StandardbrowserFrage,
        // right: Standardbrowser` — er hat die Neueinfuehrung gefunden.
        let t = treffer(&chrome, &s, &e, &p, breite, 700).expect("Treffer");
        assert_eq!(klick(&mut s, &e, t), Aktion::StandardbrowserFrage);
        let t = treffer(&chrome, &s, &e, &p, breite, 700).expect("Treffer");
        assert_eq!(klick(&mut s, &e, t), Aktion::Standardbrowser);
        // DER BEFEHL.
        assert_eq!(
            STANDARD_BROWSER_BEFEHL,
            "xdg-settings set default-web-browser"
        );
        assert_eq!(STANDARD_BROWSER_NAME, "narsil.desktop");
        assert!(
            STANDARD_BROWSER_NAME.len() <= MAX_DESKTOP_NAME,
            "der Desktop-Name ist zu lang"
        );
        // UND: der Knopf traegt eine HANDLUNG, nicht den Befehl
        // (GEMESSEN am Bild: der blaue Knopf trug „xdg-settings").
        assert_eq!(
            Feld::Standardbrowser.text(),
            "Set as default browser"
        );
        assert_eq!(Feld::Standardbrowser.wert(&e), "Make default");
    }

    /// DIE KLAPPLISTE WECHSELT BEIM ZWEITEN KLICK ZU.
    #[test]
    fn die_klappliste_wechselt_zu() {
        let mut s = Seite::default();
        s.wechsle(Kategorie::Darstellung);
        assert!(!s.ist_offen(Feld::Schriftart));
        s.klappe_wechseln(Feld::Schriftart, 0);
        assert!(s.ist_offen(Feld::Schriftart));
        s.klappe_wechseln(Feld::Schriftart, 0);
        assert!(!s.ist_offen(Feld::Schriftart), "die Liste blieb offen");
        // UND: ein anderes Feld schliesst die Liste nicht von selbst —
        // das macht `wechsle`.
        s.klappe_wechseln(Feld::Schriftart, 0);
        s.wechsle(Kategorie::Suche);
        assert!(
            !s.ist_offen(Feld::Schriftart),
            "die Liste blieb beim Wechsel offen"
        );
    }

    /// DIE LISTE HAT HOECHSTENS ACHT ZEILEN (M3) — AUCH BEI 64 SCHRIFTEN.
    #[test]
    fn die_liste_hat_hoechstens_acht_zeilen() {
        let mut e = Einstellungen::default();
        let namen: Vec<String> = (0..200).map(|i| format!("Schrift Nummer {i}")).collect();
        e.schriften_setzen(&namen);
        // M3: hoechstens MAX_SCHRIFTEN.
        assert_eq!(
            e.schriften.len(),
            MAX_SCHRIFTEN,
            "die Schriftliste waechst weiter"
        );
        let mut s = Seite::default();
        s.wechsle(Kategorie::Darstellung);
        s.klappe_wechseln(Feld::Schriftart, 0);
        let liste = s.offene_eintraege(&e);
        assert_eq!(
            liste.len(),
            MAX_KLAPPLISTE,
            "die Liste zeigt {} Zeilen",
            liste.len()
        );
        // GEMESSEN: die Liste ist der ERSTE Eintrag („Eingebaute Schrift")
        // plus sieben Schriften.
        assert_eq!(liste[0], "Built-in font");
        // UND: die Rechteckhoehe folgt der Zahl der Zeilen.
        let chrome = Chrome::neu(1000);
        let r = klappliste(
            Kategorie::Darstellung,
            &chrome,
            Feld::Schriftart,
            0,
            1000,
            700,
            liste.len(),
        )
        .expect("Liste");
        assert_eq!(r.hoehe, MAX_KLAPPLISTE as u32 * ZEILE);
        // UND: in einem zu niedrigen Fenster ist die Liste weg.
        assert!(
            klappliste(
                Kategorie::Darstellung,
                &chrome,
                Feld::Schriftart,
                0,
                1000,
                60,
                liste.len()
            )
            .is_none()
        );
    }

    /// DIE SCHRIFTLISTE WIRD GEKUERZT UND LEERE NAMEN FLIEGEN RAUS (M3/M4).
    #[test]
    fn die_schriftliste_wird_gekuerzt() {
        let mut e = Einstellungen::default();
        // ZU VIELE: nur MAX_SCHRIFTEN.
        let namen: Vec<String> = (0..1000).map(|i| format!("S{i}")).collect();
        e.schriften_setzen(&namen);
        assert_eq!(e.schriften.len(), MAX_SCHRIFTEN);
        // ZU LANG: der Name wird auf MAX_SCHRIFTNAME gekuerzt.
        let lang = vec!["s".repeat(10_000)];
        e.schriften_setzen(&lang);
        assert_eq!(e.schriften[0].chars().count(), MAX_SCHRIFTNAME);
        // LEER: fliegt raus (ein Verzeichnisname ist kein Schriftname).
        e.schriften_setzen(&["".to_string(), "   ".to_string(), "Gut".to_string()]);
        assert_eq!(e.schriften, vec!["Gut".to_string()]);
        // BOMBEN: 1000 Namen.
        //
        // GEMESSENER EIGENER FEHLER (01.10.): ich hatte
        // `assert_eq!(e.schriften.len(), 1)` behauptet und GEMESSEN 64. Der
        // Test war falsch: `vec!["x"; 1000]` sind 1000 Eintraege, und die
        // Liste haelt 64 davon. Die Einzahl ist keine Eigenschaft der
        // Grenze — sie waere es nur, wenn die Liste Duplikate entfernte, und
        // das ist nicht ihre Aufgabe.
        let bomben: Vec<String> = vec!["x".to_string(); 1_000];
        e.schriften_setzen(&bomben);
        assert_eq!(
            e.schriften.len(),
            MAX_SCHRIFTEN,
            "die Liste waechst ueber M3 hinaus"
        );
        // UND: eine Million Namen.
        let million: Vec<String> = vec!["y".to_string(); 1_000_000];
        e.schriften_setzen(&million);
        assert_eq!(e.schriften.len(), MAX_SCHRIFTEN, "eine Million Namen");
    }

    /// DIE DATEI WIRD GESCHRIEBEN UND GELESEN — DER GANZE ZUSTAND.
    ///
    /// GEMESSENE BEHAUPTUNG: alle zwoelf Werte ueberstehen den Weg durch die
    /// Datei. Das ist die Abnahme der A-EINST-Forderung („Speichern in
    /// `~/.config/narsil/einstellungen.txt`") und des Tests „dass die Datei
    /// geschrieben/gelesen wird".
    ///
    /// M9: der Pfad liegt unter `/home/jaff/mutation_md/ui/`, NICHT unter
    /// `/tmp` (tmpfs = RAM) und NICHT in `~/.config` (das waere die Datei des
    /// Nutzers).
    #[test]
    fn die_datei_wird_geschrieben_und_gelesen() {
        let ordner = testverzeichnis();
        std::fs::create_dir_all(&ordner).expect("Testverzeichnis");
        let pfad = ordner.join("einstellungen_test.txt");
        let mut e = Einstellungen::default();
        e.startseite = "https://start.example/".into();
        e.downloads_ordner = "/home/jaff/Meine Downloads".into();
        e.suchmaschine = Suchmaschine::Bing;
        e.eigene_suche = "https://x.de/suche?q=%s".into();
        e.thema = super::super::thema::Thema::Hell;
        e.zoom = 175;
        e.schriftgroesse = 18;
        e.schriftart = "DejaVu Serif".into();
        e.werbeblocker = false;
        e.cookies_beim_start_loeschen = true;

        schreiben_nach(&pfad, &e).expect("schreiben");
        assert!(pfad.exists(), "die Datei fehlt: {}", pfad.display());
        // M4: die Datei ist kleiner als die Grenze.
        assert!(
            std::fs::metadata(&pfad).expect("metadata").len() <= MAX_DATEI,
            "die Datei ist groesser als {MAX_DATEI} Byte"
        );

        let gelesen = lesen_von(&pfad).expect("lesen");
        assert_eq!(gelesen, e, "der Zustand hat den Weg nicht ueberstanden");
        // UND: der Pfad, den die Seite anbietet, ist der verlangte.
        //
        // GEMESSENER EIGENER FEHLER (01.10.): `pfad` ist hier eine Variable
        // und ueberschattet die Funktion `pfad()` — `pfad()` wurde als
        // „expected function, found PathBuf" abgelehnt. GEMESSENE REGEL, die
        // jetzt gilt: bei gleichem Namen den Aufruf qualifizieren.
        let endung = if cfg!(windows) { "narsil/einstellungen.txt" } else { ".config/narsil/einstellungen.txt" };
        assert!(
            super::nutzer_pfad().ends_with(endung),
            "der Pfad ist {}",
            super::nutzer_pfad().display()
        );
        // Release 08.10.2026: ein Testlauf schreibt NIE ins echte Profil.
        if std::env::var_os("NARSIL_PROFIL").is_none() {
            assert!(super::pfad().starts_with(std::env::temp_dir()), "der Testlauf schreibt nach {}", super::pfad().display());
        }
        let _ = std::fs::remove_file(&pfad);
    }

    /// WERT SETZEN, SEITE NEU LADEN, WERT NOCH DA — fuer **jeden**
    /// Schalter und **jede** Auswahl.
    ///
    /// LEITUNG (04.10.): „jeder Schalter aendert wirklich etwas (Profil-Datei
    /// speichern, Neustart-fest). Test: Wert setzen, Seite neu laden, Wert
    /// noch da".
    ///
    /// GEMESSENE LUECKE, die dieser Test schliesst: der vorhandene Test
    /// (Z. 5795) setzt die **Felder** direkt (`e.werbeblocker = false`) und
    /// liest sie zurueck — er geht **nicht** ueber den Klick. GEMESSENE
    /// URSACHE: ein Schalter kann am Feld richtig und im Bild tot sein (der
    /// Klick liefert `Nichts`), und ein Auswahlfeld kann am Feld richtig und
    /// in der Liste falsch sein (der Klick liefert die falsche Nummer). Beides
    /// faellt erst auf, wenn der **Klick** mitgemessen wird.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Weg aus **fuenf** Aufrufen besteht
    /// und nicht aus drei: `treffer` (der Punkt aus dem Rechteck), `klick`
    /// (die Aktion), `ausfuehren` (der Wert), `schreiben_nach` (die Datei),
    /// `lesen_von` (der Neustart). Wer eine davon weglässt, prueft einen
    /// kuerzeren Weg als den, den der Nutzer geht.
    #[test]
    fn zoom_stufen_mit_100_sichtbar_und_jede_ueberlebt_den_neustart() {
        // Release 10.10.2026: ein eigener Wert (aus der Datei des Nutzers:
        // 104) steht im Feld, die offene Liste zeigt die Stufen um ihn —
        // 100 % ist dabei und markiert.
        let mut e = Einstellungen::default();
        e.zoom = 104;
        let mut seite = Seite::default();
        seite.wechsle(Kategorie::Darstellung);
        seite.klappe_wechseln(Feld::Zoom, 0);
        assert_eq!(Feld::Zoom.wert(&e), "Custom (104 %)");
        let sichtbar = seite.offene_eintraege(&e);
        assert!(sichtbar.len() <= MAX_KLAPPLISTE);
        let stelle = sichtbar.iter().position(|t| t == "100 % (default)").expect("100 % fehlt in der Liste");
        assert_eq!(seite.klapp_beginn(&e) + stelle, Feld::Zoom.gewaehlt(&e), "100 % ist die markierte Stufe");
        // Die Wahl setzt genau die Stufe.
        let stufe_100 = Feld::Zoom.gewaehlt(&e);
        assert!(ausfuehren(&mut e, &Aktion::Waehlen(Feld::Zoom, stufe_100)));
        assert_eq!(e.zoom, 100);
        // Jede Stufe — auch die zweistelligen — uebersteht den Neustart.
        let ordner = testverzeichnis();
        std::fs::create_dir_all(&ordner).expect("Testverzeichnis");
        let pfad = ordner.join("zoom_stufen.txt");
        for z in ZOOM_STUFEN {
            e.zoom = z;
            schreiben_nach(&pfad, &e).expect("schreiben");
            assert_eq!(lesen_von(&pfad).expect("lesen").zoom, z, "{z} % ging beim Neustart verloren");
        }
    }

    #[test]
    fn jeder_schalter_und_jede_auswahl_ueberlebt_den_neustart() {
        let ordner = testverzeichnis();
        std::fs::create_dir_all(&ordner).expect("Testverzeichnis");
        let pfad = ordner.join("neustart_test.txt");
        let breite = 1000u32;
        let hoehe = 700u32;
        let chrome = Chrome::neu(breite);
        let mut geprueft = 0usize;

        for k in KATEGORIEN {
            for (index, f) in k.felder().iter().enumerate() {
                // NUR die Bedienelemente, die einen **Wert** aendern. Ein
                // Knopf aendert keinen Wert (er loescht Kekse, er stellt
                // nichts ein) — GEMESSEN ist das an `Aktion::KekseLoeschen`.
                let wert_taeger = matches!(f.bedienung(), Bedienung::Schalter | Bedienung::Auswahl);
                if !wert_taeger {
                    continue;
                }
                // GEMESSENER EIGENER FEHLER (04.10.): hier stand ein
                // `let neu = if Schalter { Some(bool) } else { Some(1usize) }`
                // — zwei verschiedene Typen (`bool` und `usize`) in einem
                // `Option`. GEMESSEN: `E0308 if and else have incompatible
                // types`. Der Schalter braucht keinen Zielwert (sein
                // Gegenteil entsteht im Klick), nur die Auswahl braucht eine
                // Zeilennummer.
                let neu_zeile: Option<usize> = if matches!(f.bedienung(), Bedienung::Auswahl) {
                    // Die **zweite** Liste: sie ist nie die Vorgabe.
                    Some(1)
                } else {
                    None
                };
                let mut seite = Seite::default();
                seite.wechsle(k);
                let mut e = Einstellungen::default();
                e.schriften = vec!["Testschrift".to_string()];
                let vorher = f.wert(&e);
                // DER KLICK: der Punkt aus dem Rechteck des Bedienelements.
                let r = element(k, &chrome, *f, index, breite);
                let p = Point::new(r.x as f32 + 6.0, r.y as f32 + 6.0);
                if let Bedienung::Auswahl = f.bedienung() {
                    // Der erste Klick OEFFNET die Liste, der zweite waehlt.
                    let t = treffer(&chrome, &seite, &e, &p, breite, hoehe)
                        .unwrap_or_else(|| panic!("{f:?} trifft sich nicht selbst"));
                    let a = klick(&mut seite, &e, t);
                    assert_eq!(
                        a,
                        Aktion::Klappe(*f),
                        "{k:?}/{f:?}: der erste Klick oeffnet die Liste nicht"
                    );
                    let l = klappliste(k, &chrome, *f, index, breite, hoehe, f.auswahl(&e).len())
                        .unwrap_or_else(|| panic!("{k:?}/{f:?}: die Liste hat kein Rechteck"));
                    let mut neu = neu_zeile.expect("eine Auswahl hat eine Zeile");
                    if f.gewaehlt(&e) == neu { neu = 0; }
                    let p2 = Point::new(
                        (l.x + 8) as f32,
                        (l.y + ZEILE * neu as u32 + ZEILE / 2) as f32,
                    );
                    let t2 = treffer(&chrome, &seite, &e, &p2, breite, hoehe)
                        .unwrap_or_else(|| panic!("{k:?}/{f:?}: Zeile {neu} trifft sich nicht"));
                    let mut a2 = klick(&mut seite, &e, t2);
                    // Eigene URL oeffnet ein Tippfeld; erst die Eingabe setzt den Wert.
                    if *f == Feld::Startseite && matches!(&a2, Aktion::Wert(Feld::Startseite, v) if v.is_empty()) {
                        assert_eq!(seite.tippfeld, Some(Feld::Startseite));
                        a2 = Aktion::Wert(Feld::Startseite, "https://beispiel.org/".to_string());
                    }
                    assert!(
                        matches!(a2, Aktion::Waehlen(_, _) | Aktion::Wert(Feld::Startseite, _)),
                        "{k:?}/{f:?}: der zweite Klick waehlt nicht ({a2:?})"
                    );
                    assert!(
                        ausfuehren(&mut e, &a2),
                        "{k:?}/{f:?}: die Wahl aendert keinen Wert"
                    );
                } else {
                    let t = treffer(&chrome, &seite, &e, &p, breite, hoehe)
                        .unwrap_or_else(|| panic!("{f:?} trifft sich nicht selbst"));
                    let a = klick(&mut seite, &e, t);
                    assert!(
                        matches!(a, Aktion::Schalten(_, _)),
                        "{k:?}/{f:?}: der Klick schaltet nicht ({a:?})"
                    );
                    assert!(
                        ausfuehren(&mut e, &a),
                        "{k:?}/{f:?}: das Schalten aendert keinen Wert"
                    );
                }
                let nachher = f.wert(&e);
                assert_ne!(
                    vorher, nachher,
                    "{k:?}/{f:?}: der Wert ist nach dem Klick derselbe — der Schalter tut nichts"
                );
                // DER NEUSTART: schreiben, neu lesen.
                schreiben_nach(&pfad, &e).expect("schreiben");
                let neu_gelesen = lesen_von(&pfad).expect("lesen");
                assert_eq!(
                    f.wert(&neu_gelesen),
                    nachher,
                    "{k:?}/{f:?}: der Wert hat den Neustart nicht ueberstanden"
                );
                geprueft += 1;
            }
        }
        assert!(
            geprueft >= 6,
            "GEMESSEN sind 7 Bedienelemente mit Wert (5 Auswahl, 2 Schalter); geprueft: {geprueft}"
        );
        let _ = std::fs::remove_file(&pfad);
    }

    /// EINE DATEI MIT WERTEN, DIE ES NICHT GIBT, MACHT DEN BROWSER NICHT
    /// ROT.
    ///
    /// GEMESSENE BEHAUPTUNG: `lesen_von` liefert fuer jede kaputte Datei die
    /// Standardwerte plus das, was lesbar war — und KEINEN Fehler. Der
    /// Nutzer hat die Datei von Hand geschrieben; „Zeile 7 ist kaputt" ist
    /// keine Information, die er beim Start braucht.
    #[test]
    fn die_datei_ueberlebt_kaputte_werte() {
        let ordner = testverzeichnis();
        std::fs::create_dir_all(&ordner).expect("Testverzeichnis");
        let pfad = ordner.join("einstellungen_kaputt.txt");
        // GEMESSENER INHALT: jede moegliche kaputte Zeile.
        let kaputt = "\
# Kommentar
kein_gleichheitszeichen
startseite
zoom=abc
zoom=99999
schriftgroesse=999
thema=neon
suchmaschine=Altavista
werbeblocker=ja
cookies_loeschen=vielleicht
unbekannter_schluessel=egal
startseite=\"https://gut.example\"
";
        std::fs::write(&pfad, kaputt).expect("schreiben");
        let e = lesen_von(&pfad).expect("lesen muss gelingen");
        // Der EINZIGE uebernommene Wert ist der letzte (die erste
        // Startseite-Zeile hatte kein `=` und wurde uebersprungen).
        assert_eq!(
            e.startseite, "https://gut.example",
            "die gute Zeile kam nicht durch"
        );
        // Alles andere ist der Standardwert.
        assert_eq!(e.zoom, 100, "kaputter Zoom");
        assert_eq!(e.schriftgroesse, 16, "kaputte Schriftgroesse");
        assert_eq!(
            e.thema,
            super::super::thema::Thema::Dunkel,
            "kaputtes Thema"
        );
        assert_eq!(
            e.suchmaschine,
            Suchmaschine::DuckDuckGo,
            "kaputte Suchmaschine"
        );
        // GEMESSEN: „ja" ist NICHT „true" — nur der Wortlaut `true` schaltet
        // ein. GEMESSENE BEGRUENDUNG (siehe `ja_nein`): „ja"/„1"/„on" waeren
        // drei weitere Woerter fuer dieselbe Sache, und jede Datei, die sie
        // liest, muesste sie alle kennen.
        assert!(
            !e.werbeblocker,
            "das Wort ja hat den Werbeblocker eingeschaltet"
        );
        assert!(!e.cookies_beim_start_loeschen, "kaputte Cookies");
        let _ = std::fs::remove_file(&pfad);
    }

    /// EINE DATEI MIT ANFUEHRUNGSZEICHEN IN DER MITTE (M4).
    ///
    /// GEMESSENE BEGRUENDUNG: `trim_matches('"')` wuerde „a\"b" zu „a\b"
    /// machen. Der Test haelt fest, dass der INNERE Wert erhalten bleibt.
    #[test]
    fn die_datei_ueberlebt_anfuehrungszeichen_in_der_mitte() {
        let ordner = testverzeichnis();
        std::fs::create_dir_all(&ordner).expect("Testverzeichnis");
        let pfad = ordner.join("einstellungen_anfuehrung.txt");
        std::fs::write(&pfad, "downloads_ordner=\"/home/jaff/a\\\"b\"\n").expect("schreiben");
        let e = lesen_von(&pfad).expect("lesen");
        // Release 09.10.2026: Backslash + Anfuehrungszeichen ist das Escape,
        // das `als_text` fuer ein inneres Anfuehrungszeichen schreibt —
        // gelesen wird das Zeichen selbst (vorher blieb der Backslash stehen).
        assert_eq!(e.downloads_ordner, "/home/jaff/a\"b");
        // Hin und zurueck aendert keinen Wert — auch keinen Windows-Pfad
        // (vorher verdoppelten sich die Backslashes bei jedem Speichern).
        let mut w = Einstellungen::default();
        w.downloads_ordner = r#"C:\Users\Jeff\Meine "Downloads""#.into();
        w.startseite = "https://beispiel.de/?a=1&b=\u{e4}".into();
        let rund = ordner.join("einstellungen_rund.txt");
        schreiben_nach(&rund, &w).expect("schreiben");
        for _ in 0..3 {
            let g = lesen_von(&rund).expect("lesen");
            assert_eq!(g.downloads_ordner, w.downloads_ordner);
            assert_eq!(g.startseite, w.startseite);
            schreiben_nach(&rund, &g).expect("nochmal schreiben");
        }
        let _ = std::fs::remove_file(&rund);
        let _ = std::fs::remove_file(&pfad);
    }

    /// EINE ZU GROSSE DATEI WIRD ABGELEHNT (M4) — und nicht gelesen.
    ///
    /// GEMESSENE BEGRUENDUNG: `read_to_string` ohne Groessenpruefung ist eine
    /// Allokation aus einer Datei, die ein Fremder geschrieben haben kann.
    #[test]
    fn eine_zu_grosse_datei_wird_abgelehnt() {
        let ordner = testverzeichnis();
        std::fs::create_dir_all(&ordner).expect("Testverzeichnis");
        let pfad = ordner.join("einstellungen_gross.txt");
        // 70 KiB Kommentar — ueber MAX_DATEI (64 KiB).
        let mut inhalt = String::new();
        while inhalt.len() < MAX_DATEI as usize + 8192 {
            inhalt.push_str("# Fuellung Fuellung Fuellung Fuellung Fuellung Fuellung\n");
        }
        std::fs::write(&pfad, &inhalt).expect("schreiben");
        let r = lesen_von(&pfad);
        assert!(r.is_err(), "eine zu grosse Datei wurde gelesen");
        assert!(
            r.err().unwrap_or_default().contains("too large"),
            "die Meldung sagt nicht, was los ist"
        );
        let _ = std::fs::remove_file(&pfad);
    }

    /// EINE NICHT VORHANDENE DATEI IST KEIN FEHLER (der Zustand nach der
    /// Installation).
    #[test]
    fn keine_datei_ist_kein_fehler() {
        let e = lesen_von(&testverzeichnis().join("gibtsnicht.txt")).expect("lesen");
        assert_eq!(
            e,
            Einstellungen::default(),
            "ohne Datei gelten nicht die Standardwerte"
        );
    }

    /// DIE ZOOM-GRENZEN (M3) — dieselbe Form wie in U4.
    #[test]
    fn der_zoom_bleibt_zwischen_25_und_500() {
        let mut e = Einstellungen::default();
        assert!(e.zoom_setzen(u32::from(ZOOM_MIN)));
        assert_eq!(e.zoom, ZOOM_MIN);
        assert!(e.zoom_setzen(u32::from(ZOOM_MAX)));
        assert_eq!(e.zoom, ZOOM_MAX);
        assert!(!e.zoom_setzen(0));
        assert_eq!(e.zoom, ZOOM_MAX, "0 hat den Zoom auf 0 gesetzt");
        assert!(!e.zoom_setzen(501));
        assert_eq!(e.zoom, ZOOM_MAX, "501 hat den Zoom geaendert");
        assert!(!e.zoom_setzen(u32::MAX));
        assert_eq!(e.zoom, ZOOM_MAX, "u32::MAX hat den Zoom geaendert");
        e.zoom_setzen(150);
        assert!((e.zoom_faktor() - 1.5).abs() < 0.001);
        // DER TEXT.
        assert_eq!(e.zoom_lesen("100"), Some(100));
        assert_eq!(e.zoom_lesen(" 150 "), Some(150));
        assert_eq!(e.zoom_lesen("9"), None, "eine Ziffer ist kein Zoom");
        assert_eq!(e.zoom_lesen(""), None);
        assert_eq!(e.zoom_lesen("abc"), None);
        assert_eq!(e.zoom_lesen("024"), None, "24 % ist unter ZOOM_MIN");
        assert_eq!(e.zoom_lesen("501"), None, "501 % ist ueber ZOOM_MAX");
        // BOMBEN: eine Million Ziffern (vor dem Parsen gekuerzt, M3).
        assert_eq!(e.zoom_lesen(&"9".repeat(1_000_000)), None);
    }

    /// DIE SCHRIFTGROESSE (M3) — 10 bis 32 px.
    #[test]
    fn die_schriftgroesse_bleibt_zwischen_10_und_32() {
        let mut e = Einstellungen::default();
        assert!(e.schriftgroesse_setzen(10));
        assert_eq!(e.schriftgroesse, SCHRIFT_MIN);
        assert!(e.schriftgroesse_setzen(32));
        assert_eq!(e.schriftgroesse, SCHRIFT_MAX);
        assert!(!e.schriftgroesse_setzen(9));
        assert_eq!(e.schriftgroesse, 32);
        assert!(!e.schriftgroesse_setzen(33));
        assert_eq!(e.schriftgroesse, 32);
        assert!(!e.schriftgroesse_setzen(u32::MAX));
        assert_eq!(e.schriftgroesse, 32);
        // DER TEXT.
        assert_eq!(e.schriftgroesse_lesen("16"), Some(16));
        assert_eq!(e.schriftgroesse_lesen(" 20 "), Some(20));
        assert_eq!(
            e.schriftgroesse_lesen("9"),
            None,
            "eine Ziffer ist keine Schriftgroesse"
        );
        assert_eq!(e.schriftgroesse_lesen("33"), None);
        assert_eq!(e.schriftgroesse_lesen("abc"), None);
        assert_eq!(e.schriftgroesse_lesen(&"9".repeat(1_000_000)), None);
    }

    /// DER TEXT AUF DEM KNOPF IST LESBAR — Kontrast 4.5 : 1 (WCAG AA).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Grenze 5 und nicht 4.5 ist:
    /// [`kontrasst`] gibt einen GANZZAHLEN Wert von 1 bis 21 zurueck (die
    /// Formel liefert 21.0 fuer Weiss auf Schwarz, siehe `thema.rs`). WCAG AA
    /// verlangt 4.5 fuer normalen Text; die kleinste ganze Zahl, die 4.5
    /// nicht unterschreitet, ist 5.
    ///
    /// GEMESSENER EIGENER FEHLER (01.10., zweimal): (1) Ich hatte `k >= 45`
    /// geschrieben — 45 auf einer Skala von 1 bis 21 ist immer wahr.
    /// (2) Ich hatte `pal.text` als Knopftext genommen und GEMESSEN
    /// „Hell: Kontrast 2". Es gibt keine EINE Farbe, die in beiden Themen
    /// ueber 4.5 : 1 liegt (Weiss 2 im dunklen, Schwarz 8 im hellen,
    /// gemessen). Schwarz ist die einzige — sie steht als `knopf_text` in der
    /// Palette.
    #[test]
    fn der_text_auf_dem_knopf_ist_lesbar() {
        use super::super::thema::{Palette, Thema, kontrasst};
        for thema in [Thema::Hell, Thema::Dunkel] {
            let pal = Palette::fuer(thema);
            let k = kontrasst(pal.knopf_text, pal.akzent);
            assert!(
                k >= 5,
                "{thema:?}: Kontrast Knopftext auf Akzent ist {k} (unter 5)"
            );
            // UND: die beiden „natuerlichen" Kandidaten fallen je in EINEM
            // Thema durch. Das ist die Begruendung fuer `knopf_text`.
            let weiss = kontrasst(narsil_core::color::Color::WHITE, pal.akzent);
            let text = kontrasst(pal.text, pal.akzent);
            assert!(
                weiss < 5 || text < 5,
                "{thema:?}: Weiss {weiss} und text {text} sind beide gut — \
                 dann waere `knopf_text` ueberfluessig"
            );
            eprintln!(
                "A-EINST-MESSUNG Knopftext {thema:?}: knopf_text {k} : 1, \
                 Weiss {weiss} : 1, text {text} : 1 (AA verlangt 4.5)"
            );
        }
    }

    /// DER STARTZUSTAND (die Bremse gegen eine stille Aenderung).
    ///
    /// GEMESSENE BEHAUPTUNG: dunkel, 100 %, 16 px, Werbeblocker AN, keine
    /// eigene Startseite, keine Cookies loeschen.
    #[test]
    fn der_startzustand_ist_der_alte() {
        let e = Einstellungen::default();
        assert_eq!(e.thema, super::super::thema::Thema::Dunkel);
        assert_eq!(e.zoom, 100, "GEMESSEN: 100 % ist der Standard-Zoom");
        assert_eq!(e.schriftgroesse, 16);
        assert!(
            e.werbeblocker,
            "der Werbeblocker startet aus (ANFORDERUNGEN Zeile 8)"
        );
        assert!(!e.cookies_beim_start_loeschen);
        assert!(e.startseite.is_empty(), "leer heisst narsil://start");
        assert_eq!(e.suchmaschine, Suchmaschine::DuckDuckGo);
        assert!(
            e.schriftart.is_empty(),
            "leer heisst die eingebaute Schrift"
        );
        // UND `Chrome` startet ebenfalls dunkel (U2).
        assert_eq!(Chrome::neu(800).thema, super::super::thema::Thema::Dunkel);
    }

    /// Release 08.10.2026: die Abschnitte des Tabs „Features" sind dieselben
    /// Gruppen wie in `features::GRUPPEN`.
    #[test]
    fn die_features_gruppen_stimmen() {
        let gruppen = super::super::features::GRUPPEN;
        let abschnitte = abschnitte(Kategorie::Features);
        assert_eq!(abschnitte.len(), gruppen.len());
        for (a, (name, liste)) in abschnitte.iter().zip(gruppen.iter()) {
            assert_eq!(a.ueberschrift, *name);
            let erwartet: Vec<Feld> = liste.iter().map(|f| Feld::Feature(*f)).collect();
            assert_eq!(a.felder.to_vec(), erwartet, "{name}");
        }
    }

    /// Release 08.10.2026: im Tab „Features" schaltet ein Klick auf die
    /// Karte die Funktion, das (i) blendet die Erklaerung ein und aus, der
    /// Knopf oeffnet die eingebaute Seite — und der Wert ueberlebt die Datei.
    #[test]
    fn der_features_tab_schaltet_erklaert_und_oeffnet() {
        use super::super::features::Feature;
        let breite = 1600u32;
        let hoehe = 1080u32;
        let chrome = Chrome::neu(breite);
        let mut e = Einstellungen::default();
        let mut s = Seite::default();
        s.wechsle(Kategorie::Features);
        let k = Kategorie::Features;
        let index = k.felder().iter().position(|f| *f == Feld::Feature(Feature::Beobachter)).expect("Feld");
        let zeile = feld(k, &chrome, Feld::Feature(Feature::Beobachter), index, breite);
        // (1) Der Schalter.
        let sr = element(k, &chrome, Feld::Feature(Feature::Beobachter), index, breite);
        let p = Point::new(sr.x as f32 + 5.0, sr.y as f32 + 5.0);
        let t = treffer(&chrome, &s, &e, &p, breite, hoehe).expect("Treffer Schalter");
        let a = klick(&mut s, &e, t);
        assert_eq!(a, Aktion::Schalten(Feld::Feature(Feature::Beobachter), true));
        assert!(ausfuehren(&mut e, &a));
        assert!(e.features.an(Feature::Beobachter));
        // (2) Das (i).
        let ir = super::super::features::info_rect(Feature::Beobachter, &zeile);
        let pi = Point::new(ir.x as f32 + 8.0, ir.y as f32 + 8.0);
        let t = treffer(&chrome, &s, &e, &pi, breite, hoehe).expect("Treffer (i)");
        assert_eq!(t, Treffer::Info(Feature::Beobachter));
        let a = klick(&mut s, &e, t);
        assert_eq!(a, Aktion::Info(Some(Feature::Beobachter)));
        assert_eq!(s.info, Some(Feature::Beobachter));
        // Die Erklaerung steht im Bild.
        let liste = display_list(&s, &e, &chrome, breite, hoehe);
        let texte: Vec<String> = liste
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(texte.iter().any(|t| t.contains("Limit")), "{texte:?}");
        assert!(liste.len() as u32 <= MAX_ITEMS_SEITE, "{} Items", liste.len());
        // Ein Klick daneben schliesst sie.
        let t = treffer(&chrome, &s, &e, &Point::new(breite as f32 - 5.0, hoehe as f32 - 40.0), breite, hoehe)
            .expect("Klick daneben");
        assert_eq!(klick(&mut s, &e, t), Aktion::Info(None));
        assert_eq!(s.info, None);
        // (3) Der Knopf „Liste".
        let kr = super::super::features::knopf_rect(Feature::Beobachter, &zeile).expect("Knopf");
        let pk = Point::new(kr.x as f32 + 10.0, kr.y as f32 + 10.0);
        let t = treffer(&chrome, &s, &e, &pk, breite, hoehe).expect("Treffer Knopf");
        assert_eq!(klick(&mut s, &e, t), Aktion::Verwalten(Feature::Beobachter));
        // (4) Die Datei.
        e.features.setzen(Feature::Konvertieren, false);
        let pfad = std::env::temp_dir().join("narsil-test-features").join("einstellungen.txt");
        schreiben_nach(&pfad, &e).expect("schreiben");
        let gelesen = lesen_von(&pfad).expect("lesen");
        assert!(gelesen.features.an(Feature::Beobachter));
        assert!(!gelesen.features.an(Feature::Konvertieren));
        assert_eq!(gelesen.features, e.features);
        let _ = std::fs::remove_file(&pfad);
    }

    /// Release 08.10.2026: alle 13 Karten passen in ein 1080-px-Fenster und
    /// ueberlappen einander nicht.
    #[test]
    fn die_features_karten_passen_und_ueberlappen_nicht() {
        let breite = 1920u32;
        let chrome = Chrome::neu(breite);
        let k = Kategorie::Features;
        let mut vorher: Option<Rechteck> = None;
        for (i, f) in k.felder().iter().enumerate() {
            let r = super::super::features::karte(&feld(k, &chrome, *f, i, breite));
            assert!(r.unten() + crate::STATUSLEISTE_HOEHE <= 1080, "{f:?} unten {}", r.unten());
            if let Some(v) = vorher {
                assert!(!r.ueberlappt(&v), "{f:?} liegt auf der Karte davor");
            }
            vorher = Some(r);
        }
    }

    /// Release 09.10.2026: bei 800 px Fensterhoehe reicht der Tab „Features"
    /// unter die Statuszeile. Das Rad rollt die Inhaltsspalte bis zur letzten
    /// Karte; Bild, Treffer und Kategoriespalte stimmen danach ueberein.
    #[test]
    fn die_inhaltsspalte_rollt_bis_zur_letzten_karte() {
        let (breite, hoehe) = (1280u32, 800u32);
        let chrome = Chrome::neu(breite);
        let mut e = Einstellungen::default();
        let mut seite = Seite::default();
        seite.wechsle(Kategorie::Features);
        let k = Kategorie::Features;
        let letzte = *k.felder().last().expect("Felder");
        let i_letzte = k.felder().len() - 1;
        let karte = super::super::features::karte(&feld(k, &chrome, letzte, i_letzte, breite));
        let sicht_unten = hoehe - crate::STATUSLEISTE_HOEHE;
        assert!(karte.unten() > sicht_unten, "Vorbedingung: die letzte Karte liegt unter der Sicht");
        // Ohne Rollen ist die letzte Karte nicht zu treffen.
        let mitte = |r: &Rechteck, rollen: u32| Point::new((r.x + 40) as f32, (r.y + r.hoehe / 2) as f32 - rollen as f32);
        let max = max_rollen(k, &chrome, hoehe);
        assert!(max > 0);
        // Das Rad: weit nach unten, begrenzt auf das Maximum.
        assert!(rollen(&mut seite, &chrome, hoehe, 5000.0));
        assert_eq!(seite.rollen, max);
        assert!(!rollen(&mut seite, &chrome, hoehe, 100.0), "am Ende bewegt sich nichts mehr");
        assert!(karte.unten() - seite.rollen <= sicht_unten, "letzte Karte sichtbar");
        // Der Treffer auf der gerollten Karte schaltet die richtige Funktion.
        let p = mitte(&karte, seite.rollen);
        let t = treffer(&chrome, &seite, &e, &p, breite, hoehe).expect("Treffer");
        assert_eq!(t, Treffer::Element(letzte));
        let a = klick(&mut seite, &e, t);
        let vorher = schalter_an(letzte, &e);
        assert!(ausfuehren(&mut e, &a));
        assert_ne!(schalter_an(letzte, &e), vorher, "der Schalter der letzten Karte kippt");
        // Das Bild: der Name der letzten Funktion steht in der Sicht.
        let Feld::Feature(f) = letzte else { panic!("kein Feature") };
        let liste = display_list(&seite, &e, &chrome, breite, hoehe);
        let y_text = liste
            .iter()
            .find_map(|i| match i {
                narsil_display::DisplayItem::Text { text, origin, .. } if text == f.name() => Some(origin.y),
                _ => None,
            })
            .expect("Name der letzten Karte im Bild");
        assert!(y_text > seiten_oberkante(&chrome) as f32 && y_text < sicht_unten as f32, "y {y_text}");
        // Die Kategoriespalte rollt nicht: „Allgemein" ist an derselben Stelle.
        let r0 = kategorie_feld(&chrome, Kategorie::Allgemein, 0, breite);
        let p0 = Point::new((r0.x + 10) as f32, (r0.y + r0.hoehe / 2) as f32);
        assert_eq!(treffer(&chrome, &seite, &e, &p0, breite, hoehe), Some(Treffer::Kategorie(Kategorie::Allgemein)));
        // Rueckwaerts bis oben, und ein Kategoriewechsel beginnt wieder oben.
        assert!(rollen(&mut seite, &chrome, hoehe, -99999.0));
        assert_eq!(seite.rollen, 0);
        rollen(&mut seite, &chrome, hoehe, 120.0);
        seite.wechsle(Kategorie::Allgemein);
        assert_eq!(seite.rollen, 0);
    }

    /// Release 09.10.2026: Tippen in ein Textfeld — Puffer, Zurueck,
    /// Uebernehmen, Abbruch; ohne getipptes Feld passiert nichts.
    #[test]
    fn tippen_fuellt_den_puffer_und_uebernimmt_mit_der_eingabetaste() {
        let mut e = Einstellungen::default();
        let mut seite = Seite::default();
        assert_eq!(tippen(&mut seite, Tippen::Zeichen('x')), None, "kein Feld, kein Tippen");
        seite.wechsle(Kategorie::Downloads);
        let a = klick(&mut seite, &e, Treffer::Element(Feld::DownloadsOrdner));
        assert_eq!(a, Aktion::Nichts);
        assert_eq!(seite.tippfeld, Some(Feld::DownloadsOrdner));
        assert_eq!(seite.eingabe, "", "leer = Systemvorgabe, kein Anzeigetext im Puffer");
        for c in "/tmp/dlx".chars() {
            tippen(&mut seite, Tippen::Zeichen(c));
        }
        tippen(&mut seite, Tippen::Zurueck);
        assert_eq!(seite.eingabe, "/tmp/dl");
        let a = tippen(&mut seite, Tippen::Fertig).expect("Aktion");
        assert_eq!(a, Aktion::Wert(Feld::DownloadsOrdner, "/tmp/dl".into()));
        assert_eq!(seite.tippfeld, None);
        assert!(ausfuehren(&mut e, &a));
        assert_eq!(e.downloads_ordner, "/tmp/dl");
        // Erneut hinein: der Puffer beginnt mit dem gespeicherten Wert; Esc verwirft.
        klick(&mut seite, &e, Treffer::Element(Feld::DownloadsOrdner));
        assert_eq!(seite.eingabe, "/tmp/dl");
        tippen(&mut seite, Tippen::Zeichen('!'));
        assert_eq!(tippen(&mut seite, Tippen::Abbruch), Some(Aktion::Nichts));
        assert_eq!(seite.tippfeld, None);
        assert_eq!(e.downloads_ordner, "/tmp/dl", "Esc aendert nichts");
    }
}
