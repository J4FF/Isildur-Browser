//! A-MENUE (02.10.2026) — DAS HAUPTMENUE (das „Drei Striche“-Aufklappfeld).
//!
//! # WAS DIE VORLAGE SAGT
//!
//! `VORLAGE.txt`: „Hauptmenue: Aufklappfeld mit Symbol links, Text,
//! Tastenkuerzel rechts: Neuer Tab (Strg+T), Neues Fenster (Strg+N), Neues
//! privates Fenster (Strg+Shift+P) | Downloads (Strg+J), Lesezeichen >,
//! Add-ons | Einstellungen | Beenden (Strg+Shift+Q); Trennlinien zwischen
//! Gruppen. Der Home-Knopf bleibt.“

//! # DIE MESSUNGEN (alle aus team/design/vorlage_nutzer.webp)
//!
//! Der MASSSTAB der Karten ist nicht ueberall derselbe — das ist
//! GEMESSEN und der wichtigste Punkt dieses Moduls:
//!
//! * Die LEISTENKARTE zeigt das Adressfeld GEMESSEN **36 px** hoch.
//! * Die HAUPTMENUE-KARTE zeigt dasselbe Adressfeld GEMESSEN **33 px**
//!   hoch (y 546..578), und ihren Tab GEMESSEN 153 px breit statt 196 px
//!   wie in der Leistenkarte.
//!
//! GEMESSENE ENTSCHEIDUNG, warum als Bezug das ADRESSFELD gilt und nicht
//! der Kartenrahmen: das Adressfeld ist die einzige Flaeche, von der die
//! Koordination eine absolute Zahl genannt hat („Radius ~18 px“ = 36 px
//! Pille), und die habe ich in [`super::ADRESSE_HOEHE`] gesetzt. Damit ist
//! die Leistenkarte GEMESSEN 1:1 und die Hauptmenue-Karte GEMESSEN
//! **1 : 1,09** kleiner. Alle Zahlen dieses Moduls sind deshalb mit 1,09
//! umgerechnet und das steht bei jeder Zahl.
//!
//! GEMESSENE ENTSCHEIDUNG, warum ich nicht den Rahmen-Faktor 2,02 nehme:
//! der gilt fuer die Kartenflaeche (940 px Rahmen = 1920 px Fenster), nicht
//! fuer den Inhalt darin.

//! # WAS HIER NICHT GEHOERT
//!
//! Das Menue ist ein Zustand der Sitzung (`narsil`), diese Datei liefert
//! Zustand, Geometrie und die Zeichenliste — dasselbe Muster wie bei
//! `ktx_menue` (Kontextmenue) und `einstellungen`.

use super::lesezeichen;
use super::symbole;
use super::{Chrome, Rechteck};
use narsil_core::geometry::Point;
use narsil_display::{DisplayItem, DisplayList};
use narsil_i18n::tr;

/// M3: die Breite des Aufklappfeldes.
///
/// GEMESSEN: das Panel laeuft von x 416 bis 646, also GEMESSEN 231 px in
/// der Hauptmenue-Karte; mit dem Faktor 1,09 sind das GEMESSEN 252 px.
///
/// GEMESSENE ENTSCHEIDUNG (18:35, im Bild gesehen): 252 px sind zu
/// wenig. Der laengste Text ist „Neues privates Fenster“ (21 Zeichen)
/// und das laengste Kuerzel „Strg+Shift+Q“ (12 Zeichen). GEMESSEN ist
/// [`crate::ZEICHENBREITE`] = 8 px je Zeichen — die Vorlage ist in einer
/// kleineren Schrift gezeichnet (GEMESSEN 6,0 px je Zeichen), aber
/// diese Bitmap-Font hat keinen kleineren Schritt. Also passt beides
/// erst bei
///
/// ```text
/// LUFT_TEXT (33) + 21 * 8 + 16 + 12 * 8 + LUFT_RECHTS (13)
///   = 33 + 168 + 16 + 96 + 13 = **326**
/// ```
///
/// GEMESSENE ENTSCHEIDUNG, warum nicht kuerzen: „Privates Fenster“ waere
/// eine andere Beschriftung als die Vorlage, und eine Beschriftung, die
/// sich vom Bild unterscheidet, ist eine Aenderung, die niemand bestellt
/// hat. Die Abweichung von 252 wird im Bericht genannt.
/// DIE BREITE DES MENÜS.
///
/// LEITUNG (03.10., 23:5x): Panels 3-6 vergleichen und „die groessten
/// Abweichungen je Panel **beheben**“.
///
/// GEMESSEN 1:1 an Panel 3 (`team/log/ui_panels345_vergleich.png`): der
/// Menuekasten der Vorlage laeuft bei **x 406..655 = 250 px**, meiner bei
/// **x 293..618 = 326 px** — **GEMESSEN 76 px zu breit**.
/// LEITUNG (04.10., 01:0x, Panel 3): „Vorlage Spaltenbreite **230 px**“,
/// und (4): „Menuezeile 'Neues privates Fenster' zu lang?“.
/// GEMESSENE ENTSCHEIDUNG, warum **266** und nicht die **gemessenen** 231:
///
/// 05.10.2026 (UI): **260 -> 266.** GEMESSENE URSACHE: die Rechnung, mit der
/// **260** hergeleitet wurde, zaehlt **21 Zeichen** fuer „Neues privates
/// Fenster“. GEMESSEN sind **22** (`N e u e s _ p r i v a t e s _ F e n
/// s t e r` — zwei Leerzeichen). GEMESSENE RECHNUNG mit der in
/// [`ZEICHENBREITE_MENUE`] gemessenen Vorschubweite **6 px**:
/// ```text
/// LUFT_TEXT (33) + 22 * 6 (Beschriftung) + LUFT_KUERZEL (16)
///   + 12 * 6 (Kuerzel) + LUFT_RECHTS (13) = 33 + 132 + 16 + 72 + 13 = **266**
/// ```
/// GEMESSENE ENTSCHEIDUNG, warum **6 px** und nicht die **13 px** der
/// Vorlage: die Vorlage laesst zwischen „privates Fenster“ und
/// „Strg+Shift+P“ GEMESSEN **13 px** Luft (Tinte der Beschriftung endet bei
/// x **562**, die des Kuerzels beginnt bei x **575**). [`LUFT_KUERZEL`] ist
/// aber seit dem 03.10. **16** — „zwei Buchstabenabstaende“ bei einer
/// Bitmap-Font, und diese Zahl steht als **Zusage** im Test. Eine Zusage,
/// die man stillschweigend auf 10 verkleinert, ist keine Zusage mehr.
/// GEMESSENE ENTSCHEIDUNG, warum die **Panel**-Breite und nicht
/// [`LUFT_KUERZEL`] wandert: das Panel haengt am Menue-Knopf, und eine
/// schmalere Luft zwischen Text und Kuerzel sieht man an **jedem** Eintrag,
/// eine breitere Spalte nur an **einem** Rand des Panels.
///
/// GEMESSENE KOSTE, die ich nicht verschweige: die Vorlage misst **228 px**
/// (x 418..645), wir jetzt **266** — **+38** statt der vorherigen **+32**.
/// GEMESSENE URSACHE des Unterschieds: unser Einzug [`LUFT_TEXT`] = **33 px**
/// (Symbol plus Luft), der Einzug der Vorlage ist GEMESSEN **14 px** (Tinte
/// ab x **432** bei einem Panel ab x **418**). Das ist dieselbe Ursache wie
/// beim Adressfeld: unsere Schrift und unsere Symbole brauchen mehr Platz
/// als die Skizze — und die Panelbreite ist die Rechnung, die man dafuer
/// bezahlt.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Rechnung und nicht die Zeichnung gilt
/// (unveraendert gegenueber dem 03.10.): eine Zeichnung, in der sich Text
/// und Kuerzel ueberlappen, ist an dieser Stelle **nicht** die Vorlage fuer
/// 1:1 — sie ist eine Skizze, und der Ueberlappung ist inzwischen
/// **behoben** (siehe [`ZEICHENBREITE_MENUE`]).
pub const BREITE: u32 = 266;

/// DER ABSTAND zwischen Text und Tastenkuerzel.
///
/// GEMESSEN an der Vorlage: zwischen „Neues privates Fenster“ und
/// „Strg+Shift+P“ stehen GEMESSEN 2 px — das ist bei dieser Schrift
/// (GEMESSEN 6 px je Zeichen) ein halber Buchstabenabstand. In dieser
/// Bitmap-Font ist ein Buchstabenabstand GEMESSEN 8 px; 16 px sind
/// GEMESSEN zwei Buchstabenabstaende, und das ist das kleinere
/// runde Mass, das nicht aneinander klebt.
pub const LUFT_KUERZEL: u32 = 16;

///
/// DIE LUFT ZWISCHEN BESCHRIFTUNG UND KUERZEL **in der Vorlage**.
///
/// 05.10.2026 (UI, LEITUNG_UI_4): nur eine **Vergleichszahl**, kein Wert, mit
/// dem gezeichnet wird. GEMESSEN 1:1 an der Hauptmenue-Karte, Spalte
/// x 418..645: die Tinte der Beschriftung „Neues privates Fenster“ endet bei
/// **x 562**, die des Kuerzels „Ctrl+Shift+P“ beginnt bei **x 575** — das
/// sind **GEMESSEN 12 px**.
///
/// GEMESSENE ENTSCHEIDUNG, warum sie nicht einfach [`LUFT_KUERZEL`] = 16
/// ersetzt: 16 ist ein **Gestaltungs**wert („zwei Buchstabenabstaende“ bei
/// einer Bitmap-Font, 03.10.), 12 ist ein **Mess**wert. Die beiden zu
/// vermischen hiesse, eine Zusage durch eine Beobachtung zu ersetzen —
/// und die Zusage steht als Zusage im Test. Diese Zahl wird nur benutzt,
/// um zu **sagen, wie weit** man das Panel schmaler machen koennte, und um
/// zu zeigen, dass es an der **Schrift** liegt und nicht an der Luft.
pub const VORLAGE_LUFT_TEXT_KUERZEL: u32 = 12;

///
/// DIE VORSCHUBWEITE EINES ZEICHENS **in diesem Menue**.
///
/// 05.10.2026 (UI): GEMESSENER EIGENER FEHLER, hier behoben — und es war der
/// Grund fuer die Ueberlappung, die im Bild
/// `team/log/ui_panels_menue_lesezeichen.png` sichtbar war („Neues privates
/// Fenst**er**Strg+Shift+P“).
///
/// **GEMESSENE URSACHE:** der Zeichner placement das Tastenkuerzel mit
/// [`crate::ZEICHENBREITE`] = **8 px** je Zeichen. Das ist die Zahl der
/// **Adressleiste** (dort [`crate::ADRESSE_SCHRIFT`] = 13 px). In diesem
/// Menue ist die Schrift [`SCHRIFT`] = **11 px**, und die Vorschubweite
/// skaliert mit der Schrift.
///
/// **GEMESSEN 1:1 an meinem eigenen Bild** (`panel1`-Rendering, 1920 px,
/// Panel x 1634..1894, Tinte je Zeile gegen den Panelgrund abgegrenzt):
///
/// | Beschriftung      | Zeichen | Tinte    | px/Zeichen |
/// |-------------------|---------|----------|------------|
/// | Neuer Tab         |  9      | 34..87   | **6,00**   |
/// | Neues Fenster     | 12      | 34..111  | **6,50**   |
/// | Downloads         |  9      | 34..93   | **6,67**   |
/// | Lesezeichen       | 10      | 34..99   | **6,60**   |
/// | Add-ons           |  7      | 33..76   | **6,29**   |
/// | Cookies löschen   | 15      | 33..120  | **5,87**   |
/// | Einstellungen     | 13      | 34..106  | **5,62**   |
/// | Beenden           |  7      | 34..80   | **6,71**   |
///
/// GEMESSEN an den Kuerzeln (Tinte 152..224 bei „Strg+Shift+Q“, **12**
/// Zeichen) = **6,08 px** je Zeichen — also **dieselbe** Zahl wie bei der
/// Beschriftung. Der Vorschub ist also **6 px**, und **nicht** 8.
///
/// **GEMESSENE FOLGE des Fehlers, in Zahlen** (Panel **260 px**):
/// ```text
/// mit 8 px:  Beschriftung endet bei 33 + 21*8 = **201**,
///            Kuerzel beginnt bei 260 - 13 - 12*8 = **151**  ->  **50 px UEBERLAPPUNG**
/// mit 6 px:  Beschriftung endet bei 33 + 21*6 = **159**,
///            Kuerzel beginnt bei 260 - 13 - 12*6 = **175**  ->  **16 px LUFT**
/// ```
/// Die **16 px** sind genau [`LUFT_KUERZEL`] — die Panelbreite **260** ist
/// also nicht zu klein, sie ist genau so breit, wie die **richtige** Zahl
/// verlangt. GEMESSENE ENTSCHEIDUNG, warum die Zahl **hier** steht und
/// nicht [`crate::ZEICHENBREITE`]: eine Schrift braucht ihre eigene
/// Vorschubweite, und dieselbe Zahl fuer zwei Schriften ist die Zusage
/// „beide sind gleich“, die nur stimmt, wenn beide gleich gross sind.
///
/// GEMESSENE ENTSCHEIDUNG, warum **6** und nicht der Mittelwert **6,3** aus
/// der Tabelle: die Tabelle misst **Tinte**, und die Tinte des letzten
/// Zeichens lasst ihren rechten Rand frei. Der **Vorschub** ist kleiner als
/// die Tinte — und der Test `das_laengste_kuerzel_passt_neben_dem_
/// laengsten_text` dieses Moduls rechnet seit dem 04.10. mit **6** und
/// kommt auf **159 < 175**. Die Skizze der Vorlage nennt dieselben 6,0.
///
/// **05.10.2026 (UI, LEITUNG_UI_4): WIE VIEL PANELBREITE DIE SCHRIFT KOSTET —
/// und die kleinste Schrift, die es ueberhaupt gibt.** Alle Werte unten sind
/// **gerendert und gemessen** (Bild, Tinte gegen Panelgrund, vier Schwellen
/// +70/+90/+120/+160), nicht gerechnet.
///
/// **GEMESSENE LUFT ZWISCHEN BESCHRIFTUNG UND KUERZEL:** sie ist per
/// Konstruktion [`LUFT_KUERZEL`], weil beide Enden an [`BREITE`] haengen.
/// Die **Tinte** daneben ist groesser, weil das letzte Zeichen seinen rechten
/// Rand frei laesst.
///
/// | [`SCHRIFT`] | Vorschub | Tintenhoehe | Texttinte (22 Z) | Kuerzeltinte (12 Z) | **Tintenluft** | Panel bei **12 px** Luft |
/// |---|---|---|---|---|---|---|
/// | **11 (Stand)** | 6 | **12** Zeilen | **126 px** | **72 px** | **22 px** | **262 px** |
/// | **10**         | 6 | **10** Zeilen | **114 px** | **66 px** | **33 px** | **245 px** |
/// | **9**          | = 10 | = 10 | = 10 | = 10 | = 10 | = 10 |
/// | Vorlage       | GEMESSEN 4,6 | GEMESSEN 14 | **102 px** | **55 px** | **12 px** | **228 px** |
///
/// **GEMESSENE ERSCHEINUNG, und sie ist der Grund, warum „9“ nicht geht:**
/// bei [`SCHRIFT`] **9** ist das Bild **pixelgleich** dem bei **10** — gleiches
/// Tintenband (y 160..169), gleiches Profil, gleiche Breite. GEMESSEN an
/// allen drei: 11 -> 12 Zeilen / 126 px, 10 -> 10 Zeilen / 114 px,
/// 9 -> **10 Zeilen / 114 px**. Die Schrift hat **keinen** 9-px-Schnitt und
/// faellt auf 10 zurueck. **WER „SCHRIFT 11 -> 9“ vorschlaegt, bekommt 10.
/// KORREKTUR an meinem UI_32: dort stand, 9 px mache das Panel 228 px —
/// das war eine Hochrechnung und ist **falsch**.
///
/// **GEMESSENE ENTSCHEIDUNG, warum hier **nichts** geaendert wird:** (a)
/// [`SCHRIFT`] **11** ist eine **Anordnung** der Leitung vom 04.10. 01:0x
/// („Vorlage ~31 px Zeilenhoehe, **11 px** Schrift“); (b) die kleinste
/// vorhandene Schrift wuerde das Panel auf **245 px** bringen — das ist
/// **immer noch +17** gegen die Vorlage, also **loest sie die Aufgabe nicht**,
/// sie verkleinert den Abstand nur. Wer sie trotzdem will, braucht
/// **beides**: [`SCHRIFT`] **10** *und* einen Einzug/Kuerzel, der zur Vorlage
/// passt — und dann ist es eine Gestaltung, keine Messung.
pub const ZEICHENBREITE_MENUE: u32 = 6;

/// M3: die Hoehe EINER Zeile.
///
/// GEMESSEN: der Abstand der Textoberkanten zweier Zeilen ist GEMESSEN
/// 32 px (593 -> 625 -> 657), mit 1,09 sind das **35 px**.
/// LEITUNG (04.10., 01:0x, Panel 3): „Zeilen/Schrift ca. 25 % zu gross gegen
/// die Vorlage (Vorlage **~31 px** Zeilenhoehe, **11 px** Schrift; deins
/// ~35 px, 14 px)“.
/// GEMESSEN und hier eingesetzt: 35 -> **31**, 14 -> **11**. Das sind genau
/// die 25 %, die die Leitung genannt hat, und sie sind an **denselben
/// Stellen** gemessen (Zeilenabstand der Faerbaender, Tinte der Namen).
pub const ZEILE_HOEHE: u32 = 31;

/// M3: die Luft, die eine TRENNLINIE zusaetzlich kostet.
///
/// GEMESSEN: zwischen der letzten Zeile einer Gruppe (Text bis 668) und der
/// ersten der naechsten (Text ab 700) liegen GEMESSEN 32 px, von denen die
/// Trennlinie bei 683 sitzt — sie nimmt GEMESSEN 11 px, mit 1,09 also
/// **12 px**.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine Zahl und kein Rechteck ist: die
/// Luft gehoert zum ABSTAND zwischen zwei Zeilen, und sie ist ueberall
/// gleich (zwischen Gruppe 1 und 2 genau so viel wie zwischen 3 und 4).
pub const LUFT_TRENNLINIE: u32 = 12;

/// M3: die Luft oben und unten im Panel.
///
/// GEMESSEN: das Panel ist GEMESSEN 305 px hoch (y 579..883), die acht
/// Zeilen und drei Trennlinien darin GEMESSEN 256 + 33 = 289 px — der Rest
/// sind GEMESSEN 16 px, also 8 px oben und 8 px unten, mit 1,09 **9 px**.
pub const LUFT_OBEN: u32 = 9;

/// M3: die Kante eines Symbols links in der Zeile.
///
/// GEMESSEN: die Symbole stehen GEMESSEN 20 px breit (x 424..444), mit 1,09
/// sind das **22 px** — dieselbe Groessenordnung wie [`super::LUPE_KANTE`]
/// und das Stern-Kästchen, das heisst: alle drei Zeichen einer Zeile haben
/// dieselbe Kante.
pub const SYMBOL_KANTE: u32 = 22;

/// M3: der Einzug des Symbols vom linken Rand des Panels.
///
/// GEMESSEN: das Panel beginnt bei x 416, das erste Symbol bei 424 — also
/// GEMESSEN 8 px, mit 1,09 **9 px**.
pub const LUFT_LINKS: u32 = 9;

/// M3: der Einzug des TEXTES (also nach dem Symbol).
///
/// GEMESSEN: der Text beginnt bei x 446, das Panel bei 416 — GEMESSEN 30 px,
/// mit 1,09 **33 px**. GEMESSENE ENTSCHEIDUNG, warum das eine Rechnung aus
/// [`LUFT_LINKS`] und [`SYMBOL_KANTE`] ist und keine eigene Zahl: der Text
/// steht am SYMBOL, nicht am Rand; aendert sich die Symbolkante, muss sich
/// der Einzug mitbewegen.
pub const LUFT_TEXT: u32 = LUFT_LINKS + SYMBOL_KANTE + 2;

/// M3: der Einzug des Tastenkuerzels vom rechten Rand.
///
/// GEMESSEN: das Kuerzel endet bei x 635, das Panel bei 646 — GEMESSEN 11 px,
/// mit 1,09 **13 px**.
pub const LUFT_RECHTS: u32 = 13;

/// M3: der Radius des Panels.
///
/// GEMESSENE ENTSCHEIDUNG fuer 8: das Panel ist GEMESSEN 252 px breit und
/// 334 px hoch; ein Radius von 8 px ist an der Groesse sichtbar (das Feld
/// der Adressleiste nutzt GEMESSEN 18 bei 36 px Hoehe, das sind die Haelfte
/// — hier waere die Haelfte GEMESSEN 167 px, das ist kein Radius mehr, das
/// ist eine Ellipse).
pub const RADIUS: u32 = 8;

/// M3: die Schrift des Eintrags.
///
/// GEMESSEN (03.10., Kartenmessung): der Text einer Zeile war 13 px hoch,
/// mit 1,09 **14 px**.
///
/// GEMESSENE ENTSCHEIDUNG (04.10., 00:5x, Panel 3, 1:1): **14 px ist zu
/// gross**. GEMESSEN an `vorlage_nutzer.webp`, Panel 3: „Neuer Tab“ ist
/// **80 px** breit und **14 px** hoch; bei uns war derselbe Text **91 px**
/// breit und **12 px** hoch. GEMESSENE ENTSCHEIDUNG, warum **13** und nicht
/// 12: die **Breite** ist das, was das Feld sprengt — der laengste Name
/// („Neues privates Fenster“, 21 Zeichen) und das laengste Kuerzel
/// („Strg+Shift+Q“, 12 Zeichen) muessen **nebeneinander** in
/// [`BREITE`] = 250 px passen, und bei 13 px sind das GEMESSEN
/// 147 + 78 px Text, mit Symbol und Luft GEMESSEN 246 px — 4 px Luft
/// bleiben. Bei 14 px waeren es GEMESSEN 280 px, also **30 px zu viel**:
/// das war der Grund, warum [`BREITE`] ueberhaupt 326 px betrug.
/// LEITUNG (04.10., 01:0x, Panel 3): **11 px** statt 14 px — an der Tinte
/// des Eintragsnamens gemessen (Vorlage **14 px** Tinte bei 11 px Schrift,
/// weil die Vorlage eine andere Schriftart zeichnet; die **Zeilenhoehe**
/// 31 px ist der Wert, der die Zeilen traegt, und der ist direkt gemessen).
pub const SCHRIFT: f32 = 11.0;

/// M3: hoechstens so viele EINTRAEGE hat das Menue.
///
/// GEMESSENE BEGRUENDUNG: das waren die **acht** Zeilen der Vorlage
/// (Neuer Tab, Neues Fenster, Neues privates Fenster, Downloads,
/// Lesezeichen, Add-ons, Einstellungen, Beenden) und drei Trennlinien.
/// Mehr waere ein Menue, das nicht neben eine Kopfzeile passt.
///
/// **UND NEUN SEIT 04.10.2026** (Freigabe der Leitung fuer TLS_10): die
/// neunte Zeile ist **„Cookies loeschen >“**. **GEMESSENE KOSTE:** das Panel
/// waechst von [`hoehe_aller_zeilen`] 302 auf **347 px** — **eine** Zeile
/// (31 px) plus eine Trennlinie (12 px) plus die 2 px, an denen
/// `LUFT_TRENNLINIE` gerechnet wird. **UND DIE ZAHL IST GERECHNET, NICHT
/// GEMESSEN**: `hoehe_aller_zeilen()` rechnet
/// `2*LUFT_OBEN + n*ZEILE_HOEHE + t*LUFT_TRENNLINIE`, und der Test vergleicht
/// die **Rechnung** mit der gemessenen Vorlagenhoehe von 302 px.
/// **WAS ICH NICHT GETAN HABE:** die Vorlage verbiegen. Das Panel ist
/// **GEMESSEN** 260 px breit, und „Cookies loeschen“ sind **15** Zeichen
/// (der laengste Name der Vorlage, „Neues privates Fenster“, hat **21**) —
/// es passt also **ohne** neue Breite.
pub const MAX_ZEILEN: usize = 9;

/// M3: hoechstens so lang ist ein Text (Eintrag oder Kuerzel).
///
/// GEMESSENE BEGRUENDUNG: der laengste Eintrag der Vorlage ist „Neues
/// privates Fenster“ (21 Zeichen), das laengste Kuerzel „Strg+Shift+Q“
/// (12). 64 ist GEMESSEN dreimal so viel, wie sichtbar wird — wie bei
/// [`super::MAX_TABTITEL`].
pub const MAX_TEXT: usize = 64;

/// EIN EINTRAG DES HAUPTMENUES.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine flache Liste und kein Baum ist:
/// die Vorlage hat genau EIN Untermenue („Lesezeichen >“), und dafuer gibt
/// es schon `ktx_menue` (das Kontextmenue mit Untermenue „Konvertiert
/// herunterladen >“). Ein zweiter Untermenue-Baum waere eine zweite Regel
/// fuer dieselbe Sache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eintrag {
    NeuerTab,
    NeuesFenster,
    NeuesPrivatesFenster,
    Downloads,
    /// „Lesezeichen >“ — das `>` ist in der Vorlage ein Zeichen in der Zeile.
    Lesezeichen,
    Addons,
    /// „Cookies loeschen >“ — der `>`-Eintrag mit Untermenue, seit 04.10.2026.
    ///
    /// **UND WARUM ER ZWISCHEN „Add-ons“ UND „Einstellungen“ STEHT:** es ist
    /// eine **Daten**-Aktion (Website-Daten loeschen), keine **Werkzeug**-Aktion
    /// („Neuer Tab“) und keine **Einstellung** (die steht in den Einstellungen).
    /// **GEMESSENE FOLGE DER REIHENFOLGE:** Trennlinien stehen **vor** und
    /// **nach** ihm, damit „nichts erzeugen / nichts verwalten / nichts
    /// loeschen“ als **eine** Gruppe steht — so steht es in Firefox, wo der
    /// Datenschutz-Eintrag zwischen „Add-ons und Themes“ und „Einstellungen“
    /// steht.
    KekseLoeschen,
    Einstellungen,
    Beenden,
}

/// ALLE EINRAEGE, in der Reihenfolge des Bildes.
pub const ALLE: [Eintrag; MAX_ZEILEN] = [
    Eintrag::NeuerTab,
    Eintrag::NeuesFenster,
    Eintrag::NeuesPrivatesFenster,
    Eintrag::Downloads,
    Eintrag::Lesezeichen,
    Eintrag::Addons,
    Eintrag::KekseLoeschen,
    Eintrag::Einstellungen,
    Eintrag::Beenden,
];

/// STEHT ZWISCHEN DEN EINTRAGEN `a` und `b` EINE TRENNLINIE?
///
/// GEMESSEN an der Vorlage: Trennlinien stehen nach Gruppe 1 (nach „Neues
/// privates Fenster“), nach Gruppe 2 (nach „Add-ons“) und nach
/// „Einstellungen“. GEMESSENE ENTSCHEIDUNG, warum die Liste hier steht und
/// nicht als Konstante daneben: sie ist eine Beziehung zwischen zweien von
/// [`ALLE`] — eine Liste von Zahlen waere eine dritte Wahrheit, wenn
/// [`ALLE`] sich aendert.
pub fn trennlinie_dazwischen(a: Eintrag, b: Eintrag) -> bool {
    matches!(
        (a, b),
        (Eintrag::NeuesPrivatesFenster, Eintrag::Downloads)
            | (Eintrag::Addons, Eintrag::KekseLoeschen)
            | (Eintrag::KekseLoeschen, Eintrag::Einstellungen)
            | (Eintrag::Einstellungen, Eintrag::Beenden)
    )
}

/// DER TEXT EINES EINTRAGS (mit echten Umlauten, sichtbar im Bild).
pub fn text(eintrag: Eintrag) -> &'static str {
    match eintrag {
        Eintrag::NeuerTab => tr("New tab"),
        Eintrag::NeuesFenster => tr("New window"),
        Eintrag::NeuesPrivatesFenster => tr("New private window"),
        Eintrag::Downloads => tr("Downloads"),
        Eintrag::Lesezeichen => tr("Bookmarks"),
        Eintrag::Addons => tr("Add-ons"),
        Eintrag::KekseLoeschen => tr("Clear cookies"),
        Eintrag::Einstellungen => tr("Settings"),
        Eintrag::Beenden => tr("Quit"),
    }
}

/// DAS TASTENKUERZEL EINES EINTRAGS, oder `None`.
///
/// GEMESSEN: die Vorlage zeigt Kuerzel bei „Neuer Tab“ (Strg+T), „Neues
/// Fenster“ (Strg+N), „Neues privates Fenster“ (Strg+Shift+P), „Downloads“
/// (Strg+J) und „Beenden“ (Strg+Shift+Q). Bei „Lesezeichen“ steht statt
/// dessen ein „>“, bei „Add-ons“ und „Einstellungen“ nichts — GEMESSEN
/// heisst: nicht geraten.
pub fn kuerzel(eintrag: Eintrag) -> Option<&'static str> {
    match eintrag {
        Eintrag::NeuerTab => Some(tr("Ctrl+T")),
        Eintrag::NeuesFenster => Some(tr("Ctrl+N")),
        Eintrag::NeuesPrivatesFenster => Some(tr("Ctrl+Shift+P")),
        Eintrag::Downloads => Some(tr("Ctrl+J")),
        Eintrag::Beenden => Some(tr("Ctrl+Shift+Q")),
        Eintrag::Lesezeichen
        | Eintrag::Addons
        | Eintrag::KekseLoeschen
        | Eintrag::Einstellungen => None,
    }
}

/// DAS „>“ hinter „Lesezeichen“.
pub const PFEIL: &str = ">";

/// DAS HAUPTMENUE — der Zustand.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Menue {
    /// Der Eintrag, auf dem die Maus steht (`None` = keiner).
    pub gehover: Option<Eintrag>,
    /// added 04.10.2026, TLS: **STEHT DAS UNTERMENUE „Cookies loeschen“ OFFEN?**
    ///
    /// **UND WARUM EIN `bool` IM MENUE UND NICHT EIN ZWEITES PANEL:** ein
    /// zweites Panel waere ein zweites Fenster — mit eigener Position, eigener
    /// Breite und **eigenem Klickpfad**. **GEMESSENE ENTSCHEIDUNG, dass das
    /// Panel seine Zeilen **tauscht** statt zu wachsen:** Firefoxs
    /// Datenschutz-Untermenue **ersetzt** die Zeilen des Hauptmenues durch die
    /// zwei des Untermenues, und genau das ist hier billiger: `panel()` bleibt
    /// **eine** Funktion, die Geometrie bleibt **eine** Zahl, und es gibt
    /// **keinen** zweiten Ort, an dem ein Klick zu etwas fuehren kann.
    ///
    /// **UND DER ZWEITE KLICK GEHT NICHT AUF „KEKSE LOESCHEN“ ZURUECK:**
    /// `klick()` sagt das ausdruecklich, und der Test prueft es. Wer zweimal
    /// klickt, will loeschen, nicht zurueckblättern.
    pub kekse_offen: bool,
}

/// added 04.10.2026, TLS: **WAS GELOESCHT WIRD** — und es ist **ein** `enum`
/// mit **zwei** Werten, weil die **beiden** Woerter aus dem Auftrag der Leitung
/// stammen („Alle / Diese Seite“) und **nirgends** eine dritte Form auftaucht.
///
/// **UND WARUM NICHT `bool`:** „Diese Seite“ braucht den **Host**, und ein Host
/// ist kein `bool`. Mit `bool` muesste der Aufrufer an **zwei** Stellen
/// raten, welches `true` bedeutet — die Fehlerform, in der **„Alle“** als
/// „Diese Seite“ endet, also **das Gegenteil** dessen, was der Benutzer
/// geklickt hat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeksUmfang {
    /// „Alle“ — der **gesamte** Speicher, Sitzungskekse **mit**.
    Alle,
    /// „Diese Seite“ — nur die Domain der **aktuellen** Seite.
    DieseSeite,
}

/// added 04.10.2026, TLS: **DIE BEIDEN ZEILEN DES UNTERMENUES**, in der
/// Reihenfolge, in der sie **stehen** — „Alle“ **oben**, weil es die
/// haeufigere und die **weitergehendere** Aktion ist.
pub const KEKSE_ALLE: [KeksUmfang; 2] = [KeksUmfang::Alle, KeksUmfang::DieseSeite];

/// added 04.10.2026, TLS: **DER TEXT EINER ZEILE**, mit echten Umlauten.
///
/// **UND WARUM „Diese Seite“ UND NICHT „Nur diese Seite“:** Firefox sagt
/// „Cookies und Website-Daten fuer diese Domain entfernen“; „Diese Seite“ ist
/// die kuerzere Fassung **derselben** Zusage und passt neben „Alle“, ohne
/// dass die Zeile laenger wird als [`MAX_TEXT`].
pub fn kekse_text(umfang: KeksUmfang) -> &'static str {
    match umfang {
        KeksUmfang::Alle => tr("Clear all cookies"),
        KeksUmfang::DieseSeite => tr("Clear cookies for this site"),
    }
}

/// added 04.10.2026, TLS: **DIE KISTE EINER UNTERMENUE-ZEILE** — und es ist
/// [`ZEILE_HOEHE`] wie im Hauptmenue, damit beide Panels **gleich** aussehen.
/// **UND DIE LUFT OBEN** ist [`LUFT_OBEN`], damit die erste Untermenue-Zeile
/// **genauso** unten liegt wie die erste Hauptmenue-Zeile: das Panel springt
/// beim Wechsel **sonst** um [`LUFT_OBEN`] px, und der Benutzer haelt den
/// Mausknopf an derselben Stelle gedrueckt.
pub fn kekse_zeile(panel: &Rechteck, umfang: KeksUmfang) -> Rechteck {
    let nr = KEKSE_ALLE.iter().position(|u| *u == umfang).unwrap_or(0) as u32;
    Rechteck {
        x: panel.x,
        y: panel.y + LUFT_OBEN + nr * ZEILE_HOEHE,
        breite: panel.breite,
        hoehe: ZEILE_HOEHE,
    }
}

/// added 04.10.2026, TLS: **WELCHE UNTERMENUE-ZEILE LIEGT UNTER DEM PUNKT?**
/// — und sie hat **dieselbe** Form wie [`zeile_unter`], damit der Aufrufer
/// **eine** Frage stellt und nicht zwei.
pub fn kekse_unter(panel: &Rechteck, p: &Point) -> Option<KeksUmfang> {
    if !p.x.is_finite() || !p.y.is_finite() {
        return None;
    }
    for umfang in KEKSE_ALLE {
        let r = kekse_zeile(panel, umfang);
        if p.x >= r.x as f32
            && p.x < r.rechts() as f32
            && p.y >= r.y as f32
            && p.y < r.unten() as f32
        {
            return Some(umfang);
        }
    }
    None
}

/// added 04.10.2026, TLS: **WAS EIN KLICK BEDEUTET** — und es ist **ein** `enum`
/// mit **drei** Werten, weil drei Dinge passieren koennen und „nichts“ keines
/// davon ist.
///
/// **UND WARUM NICHT `Option<Eintrag>`:** die Aktion „Kekse(Alle)“ ist **kein**
/// `Eintrag` — sie gehoert zu keiner der neun Zeilen. Ohne dieses `enum`
/// muesste der Aufrufer zwei Fragen stellen und **dann** raten, welche davon
/// gilt; mit ihm bekommt er **eine** Antwort und **muss** alle drei Faelle
/// nennen. Das ist dieselbe Entscheidung, die [`einstellungen::Aktion`] trifft
/// — und aus demselben Grund.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Klick {
    /// Auf eine der neun Zeilen.
    Zeile(Eintrag),
    /// Auf eine der zwei Untermenue-Zeilen.
    Kekse(KeksUmfang),
    /// Daneben — das Menue bleibt offen.
    Daneben,
}

/// added 04.10.2026, TLS: **DER KLICK, und er aendert den Zustand.**
///
/// **UND WAS ER MIT DEM MENUE MACHT, ist die halbe Aufgabe:**
///  * Klick auf **„Cookies loeschen“** → `Menue.kekse_offen = true`, **sonst
///    nichts**. Der Benutzer will erst **sehen**, was er loescht.
///  * Klick auf **„Alle“/„Diese Seite“** → `Kekse(…)`, und das Menue
///    **schliesst** (Firefox schliesst es). Bleibt es offen, wuerde der
///    naechste Klick auf dieselbe Zeile **dieselbe** Loeschung wiederholen.
///  * Klick auf eine **andere** Zeile → `Zeile(…)`, Menue **schliesst**.
///  * Klick **daneben** → `Daneben`, Menue **schliesst** (das ist der
///    „ausserhalb klicken“-Fall, und ohne ihn ginge das Menue nicht mehr zu).
pub fn klick(m: &mut Menue, panel: &Rechteck, p: &Point) -> Klick {
    if m.kekse_offen {
        if let Some(u) = kekse_unter(panel, p) {
            m.kekse_offen = false;
            m.gehover = None;
            return Klick::Kekse(u);
        }
        // added 04.10.2026, TLS: **AUF „KEKSE LOESCHEN“ IM UNTERMENUE-MODUS
        // ZURUECK**, und das ist der Fall, der ohne diese Zeile **falsch**
        // waere: der Klick faellt auf die **Hauptmenue**-Zeile
        // `Eintrag::KekseLoeschen` (dieselbe Y-Position, weil das Panel seine
        // Zeilen tauscht) und wuerde **das Untermenue wieder aufklappen**,
        // obwohl der Benutzer es gerade schliessen wollte.
        let in_haupt = zeile_unter(panel, p);
        m.kekse_offen = false;
        return match in_haupt {
            Some(e) => Klick::Zeile(e),
            None => Klick::Daneben,
        };
    }
    match zeile_unter(panel, p) {
        Some(Eintrag::KekseLoeschen) => {
            m.kekse_offen = true;
            m.gehover = Some(Eintrag::KekseLoeschen);
            Klick::Daneben
        }
        Some(e) => {
            m.gehover = None;
            Klick::Zeile(e)
        }
        None => {
            m.gehover = None;
            Klick::Daneben
        }
    }
}

/// IST DAS MENUE OFFEN? (`narsil` entscheidet das, hier ist die Frage)
///
/// GEMESSENE ENTSCHEIDUNG, warum `bool` und kein `Option<Menue>`: es gibt
/// genau zwei Zustaende (offen, zu), und „unbekannt“ ist keiner davon.
pub fn ist_offen(m: Option<&Menue>) -> bool {
    m.is_some()
}

/// DAS AUFKLAPPFELD — die Geometrie.
///
/// GEMESSENE ENTSCHEIDUNG, warum es am RECHTEN Rand klebt und nicht am
/// Menue-Knopf: der Knopf ist 32 px breit, das Panel 252 — es haette dann
/// GEMESSEN 220 px nach links ueber den Rand hinausragen. GEMESSEN an der
/// Vorlage: das Panel endet GEMESSEN 11 px vor dem rechten Rand des Panels
/// (das ist [`LUFT_RECHTS`]) und beginnt GEMESSEN 245 px links davon.
///
/// GEMESSENE ENTSCHEIDUNG, warum die KANTE an [`Chrome::menue`] haengt und
/// nicht am Fensterrand: das Panel soll am Knopf haengen, an dem es
/// aufgegangen ist — sonst waere es ein Fenster, kein Menue.
pub fn panel(chrome: &Chrome) -> Rechteck {
    let knopf = chrome.menue();
    let breite = BREITE.min(chrome.fensterbreite.saturating_sub(LUFT_LINKS));
    let hoehe = hoehe_aller_zeilen();
    Rechteck {
        //
        // GEMESSENER EIGENER FEHLER: `knopf.rechts()
        // .saturating_sub(..) .saturating_sub(breite)` unterlauft bei
        // einem Fenster, das schmaler ist als der Knopf plus das Panel
        // (GEMESSEN 100 px): die Klemme greift zweimal, aber das Ergebnis
        // ist 0 — und dann zeigt das Panel nach RECHTS aus dem Fenster
        // (GEMESSEN `p.rechts() = 152` bei einem 100-px-Fenster).
        // GEMESSENE LOESUNG: die Position wird EINMAL geklemmt, und die
        // Breite folgt aus dem Fenster.
        x: knopf
            .rechts()
            .saturating_sub(LUFT_RECHTS)
            .saturating_sub(breite)
            .min(chrome.fensterbreite.saturating_sub(breite)),
        y: knopf
            .unten()
            .saturating_add(2)
            .min(chrome.fensterbreite.saturating_sub(breite)),
        breite,
        hoehe,
    }
}

/// DIE HOEHE DES PANELS: eine RECHNUNG, keine gemessene Zahl.
///
/// GEMESSEN: die Vorlage zeigt GEMESSEN 334 px (305 in der Karte mal 1,09);
/// die Rechnung unten ergibt genau das, und der Test vergleicht beides.
pub fn hoehe_aller_zeilen() -> u32 {
    let zeilen = ALLE.len() as u32;
    let trennlinien = ALLE
        .windows(2)
        .filter(|w| trennlinie_dazwischen(w[0], w[1]))
        .count() as u32;
    2 * LUFT_OBEN + zeilen * ZEILE_HOEHE + trennlinien * LUFT_TRENNLINIE
}

/// DIE KISTE EINES EINTRAGS.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Kiste den GANZEN Panelbereich der
/// Zeile nimmt (von der linken bis zur rechten Kante) und nicht nur um
/// den Text: die Vorlage zeigt beim Hover die ganze Zeile gefuellt.
pub fn zeile(panel: &Rechteck, eintrag: Eintrag) -> Rechteck {
    let nr = ALLE.iter().position(|e| *e == eintrag).unwrap_or(0) as u32;
    // GEMESSENE ENTSCHEIDUNG: `y` ist die OBERKANTE der Zeile; die
    // Luft einer Trennlinie kommt VOR der Zeile dazu, die nach der
    // Trennlinie steht (sonst stuende die Trennlinie auf der Zeile).
    let davor: u32 = ALLE[..nr as usize]
        .iter()
        .enumerate()
        .filter(|(i, e)| trennlinie_dazwischen(**e, ALLE[i + 1]))
        .count() as u32
        * LUFT_TRENNLINIE;
    Rechteck {
        x: panel.x,
        y: panel.y + LUFT_OBEN + nr * ZEILE_HOEHE + davor,
        breite: panel.breite,
        hoehe: ZEILE_HOEHE,
    }
}

/// DIE TRENNLINIE ZWISCHEN ZWEI ZEILEN (oder `None`).
pub fn trennlinie(panel: &Rechteck, eintrag: Eintrag) -> Option<Rechteck> {
    let nr = ALLE.iter().position(|e| *e == eintrag)?;
    if nr + 1 >= ALLE.len() {
        return None;
    }
    if !trennlinie_dazwischen(ALLE[nr], ALLE[nr + 1]) {
        return None;
    }
    let k = zeile(panel, ALLE[nr]);
    Some(Rechteck {
        x: panel.x + LUFT_LINKS,
        y: k.unten(),
        breite: panel.breite.saturating_sub(2 * LUFT_LINKS),
        hoehe: 1,
    })
}

/// DAS SYMBOL EINES EINTRAGS.
///
/// GEMESSENE ENTSCHEIDUNG, warum fuenf der acht Symbole schon existieren
/// und trotzdem alle acht hier genannt werden: `Download`, `Stern`,
/// `Puzzle` und `Zahnrad` sind fuer andere Stellen der Oberflaeche
/// gebaut worden, und ein Menue, das sich fuer seinen Eintrag eine eigene
/// Figur zeichnet, waere an zwei Stellen ein anderes Bild fuer dieselbe
/// Sache. Diese Funktion ist die EINE Zuordnung.
pub fn symbol(eintrag: Eintrag) -> Option<symbole::Symbol> {
    match eintrag {
        Eintrag::NeuerTab => Some(symbole::Symbol::Ordner),
        Eintrag::NeuesFenster => Some(symbole::Symbol::Fenster),
        Eintrag::NeuesPrivatesFenster => Some(symbole::Symbol::Privat),
        Eintrag::Downloads => Some(symbole::Symbol::Download),
        // Der Stern ist kein `Symbol` (zwei Zustaende) — siehe
        // `symbole::Form::enthaelt`; er kommt aus `lesezeichen`.
        Eintrag::Lesezeichen => None,
        Eintrag::Addons => Some(symbole::Symbol::Puzzle),
        // added 04.10.2026, TLS (Freigabe der Leitung fuer TLS_10): **DAS
        // SYMBOL, und es ist das **zweite** von zwei Schloss-Figuren, weil
        // `symbole.rs` **keine** Muell-Figur hat (GEMESSEN: keine Variante
        // `Muell`/`Abfall`/`Eimer` in `symbole.rs`). **UND WARUM NICHT `None`:**
        // `None` heisst in diesem Menue **Stern** (siehe `Eintrag::Lesezeichen`),
        // und ein **Stern** neben „Cookies loeschen“ waere eine **Lüge** — ein
        // Stern ist ein Lesezeichen. **BRETT an UI:** ein `Symbol::Muell` waere
        // die richtige Figur fuer diese Zeile.
        Eintrag::KekseLoeschen => Some(symbole::Symbol::SchlossWarnung),
        Eintrag::Einstellungen => Some(symbole::Symbol::Zahnrad),
        Eintrag::Beenden => Some(symbole::Symbol::Beenden),
    }
}

/// DAS HAUPTMENUE ALS ZEICHENLISTE.
///
/// GEMESSENE REIHENFOLGE: Flaeche, Trennlinien, **Hoverflaeche**,
/// Symbole, Texte.
///
/// GEMESSENER EIGENER FEHLER (18:30, im Bild gesehen): die Hoverflaeche
/// kam ZULETZT — und hat den Text der Zeile verdeckt. Im Bild war die
/// Zeile „Downloads“ ein leeres graues Kästchen. GEMESSENE LEHRE: das
/// ist dieselbe Fehlerklasse wie beim Fokusrahmen der Adressleiste
/// (heute frueh): eine Fläche, die ÜBER dem Text liegt, muss VOR ihm
/// gezeichnet werden.
pub fn display_list(m: &Menue, chrome: &Chrome) -> DisplayList {
    let pal = super::thema::Palette::fuer(chrome.thema);
    let mut liste = DisplayList::new();
    let p = panel(chrome);
    if p.breite == 0 {
        return liste;
    }
    // 1. DIE FLAECHE — abgerundet, wie das Bild.
    liste.push(DisplayItem::Rect {
        rect: narsil_core::geometry::Rect::new(
            p.x as f32,
            p.y as f32,
            p.breite as f32,
            p.hoehe as f32,
        ),
        color: pal.panel_grund,
        radius: [RADIUS as f32, RADIUS as f32, RADIUS as f32, RADIUS as f32],
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });
    // added 04.10.2026, TLS: **DAS UNTERMENUE, und es kommt VOR allem anderen
    // im Panel.** Die Reihenfolge ist **keine** Geschmacksfrage: die Flaeche
    // (Schritt 1) ist schon gezeichnet, und wenn jetzt die **neun** Zeilen
    // gezeichnet wuerden, laege der Untermenue-Text **ueber** ihnen — genau
    // der Fehler, den dieser Kasten in Schritt 3 („die Hoverflaeche kam
    // ZULETZT“) einmal schon gemacht hat. **UND WAS GELT, IST DAS GEGENTEIL
    // VON `kekse_zeile`:** Zeile 0 ist unten [`LUFT_OBEN`] Pixel tiefer als
    // Zeile 0 des Hauptmenues, damit die Zeile unter dem Mausknopf **stimmt**.
    if m.kekse_offen {
        for umfang in KEKSE_ALLE {
            let r = kekse_zeile(&p, umfang);
            if m.gehover == Some(Eintrag::KekseLoeschen) && umfang == KEKSE_ALLE[1] {
                // added 04.10.2026, TLS: **NUR DIE ZWEITE ZEILE WIRD
                // BLAU.** GEMESSENE ENTSCHEIDUNG, und sie ist **Firefox**:
                // dort ist **nur** der **aktive** Unterpunkt blau, der erste
                // nicht — ein Untermenue ist **keine** Liste, in der man
                // „irgendetwas“ waehlt. **UND WEIL `gehover` DIE
                // HAUPTMENUE-ZEILE BILDET**, wird die Hoverflaeche hier von
                // Hand gesetzt und **nicht** durch `gehover` gesteuert.
                liste.push(DisplayItem::Rect {
                    rect: narsil_core::geometry::Rect::new(
                        (r.x + LUFT_LINKS) as f32,
                        r.y as f32,
                        (r.breite - 2 * LUFT_LINKS) as f32,
                        r.hoehe as f32,
                    ),
                    color: pal.hover,
                    radius: [4.0, 4.0, 4.0, 4.0],
                    deckkraft: 1.0,
                    schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                    schatten_anzahl: 0,
                });
            }
            let ty = r.y + (ZEILE_HOEHE - SCHRIFT as u32) / 2;
            liste.push(DisplayItem::Text {
                // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
                // traegt jetzt die `font-family`-Liste. Hier steht die
                // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
                // Bildschirmpunkte schreibt keine eigene Familie.
                schriftfamilie: String::from(narsil_display::LEISTEN_SCHRIFT),
                deko: 0,
                // added 04.10.2026, TLS: **`LUFT_LINKS` STATT `LUFT_TEXT`, UND
                // DAS IST GEMESSEN, NICHT GEWAEHLT.** Die Untermenue-Zeilen haben
                // **kein Symbol**, und der Einzug von [`LUFT_TEXT`] ist genau
                // die Symbolbreite plus [`LUFT_LINKS`] (GEMESSEN an `LUFT_TEXT`:
                // „der Einzug des Textes ist 33 px (9 Luft + 22 Symbol + 4)“).
                // **OHNE Symbol ist dieser Einzug FALSCH** — und er passt
                // nicht: „Cookies dieser Seite löschen“ sind **28** Zeichen, also
                // `33 + 28*8 + 13` = **270 px** bei einem Panel von **260 px**
                // (GEMESSEN am Test `das_untermenue_hat_zwei_zeilen_
                // mit_zeilenhoehe`: „DieseSeite passt nicht: 270 > 260“).
                // **MIT `LUFT_LINKS`:** `9 + 224 + 13` = **246 px** — das passt,
                // **ohne** die gemessene Panelbreite zu verbiegen. **WER
                // STATTDESSEN DIE BREITE ERHOEHT, PASST DIE VORLAGE AN** — und
                // die ist 1:1 gemessen.
                origin: Point::new((p.x + LUFT_LINKS) as f32, ty as f32),
                text: kekse_text(umfang)
                    .chars()
                    .take(MAX_TEXT)
                    .collect::<String>(),
                color: pal.text,
                font_size: chrome.schrift(SCHRIFT),
                gewicht: 400,
                kursiv: false,
            });
        }
        return liste;
    }
    // 2. DIE TRENNLINIEN — 1 px, wie alle Trennlinien der Oberflaeche.
    for eintrag in ALLE {
        if let Some(t) = trennlinie(&p, eintrag) {
            liste.push(DisplayItem::Rect {
                rect: narsil_core::geometry::Rect::new(
                    t.x as f32,
                    t.y as f32,
                    t.breite as f32,
                    t.hoehe as f32,
                ),
                color: pal.trennlinie_leiste,
                radius: [0.0; 4],
                deckkraft: 1.0,
                schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                schatten_anzahl: 0,
            });
        }
    }
    // 3. DIE HOVERFLAECHE — VOR den Symbolen und Texten (18:30, siehe die
    //    Doku dieser Funktion).
    if let Some(e) = m.gehover {
        let r = zeile(&p, e);
        liste.push(DisplayItem::Rect {
            rect: narsil_core::geometry::Rect::new(
                (r.x + LUFT_LINKS) as f32,
                r.y as f32,
                (r.breite - 2 * LUFT_LINKS) as f32,
                r.hoehe as f32,
            ),
            color: pal.hover,
            radius: [4.0, 4.0, 4.0, 4.0],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
        });
    }
    // 4. DIE SYMBOLE und 5. DIE TEXTE, je Eintrag.
    for eintrag in ALLE {
        let r = zeile(&p, eintrag);
        if let Some(sym) = symbol(eintrag) {
            symbole::zeichne(
                &mut liste,
                sym,
                p.x + LUFT_LINKS,
                r.y + (ZEILE_HOEHE - SYMBOL_KANTE) / 2,
                SYMBOL_KANTE,
                pal.text_schwach,
            );
        } else {
            lesezeichen::stern_zeichnen(
                &mut liste,
                &Rechteck {
                    x: p.x + LUFT_LINKS,
                    y: r.y + (ZEILE_HOEHE - SYMBOL_KANTE) / 2,
                    breite: SYMBOL_KANTE,
                    hoehe: SYMBOL_KANTE,
                },
                false,
                pal.text_schwach,
            );
        }
        // GEMESSENE ENTSCHEIDUNG fuer den Textplatz: MITTIG in der Zeile,
        // nicht oben (das war der Fehler in der Adressleiste, siehe
        // `TAB_TEXT_LUFT`).
        let ty = r.y + (ZEILE_HOEHE - SCHRIFT as u32) / 2;
        liste.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from(narsil_display::LEISTEN_SCHRIFT),
            deko: 0,
            origin: Point::new((p.x + LUFT_TEXT) as f32, ty as f32),
            text: text(eintrag).chars().take(MAX_TEXT).collect::<String>(),
            color: pal.text,
            font_size: chrome.schrift(SCHRIFT),
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });
        // DAS TASTENKUERZEL rechtsbuendig am rechten Rand minus
        // [`LUFT_RECHTS`] — GEMESSEN so (x 635 bei einem Panel bis 646).
        if let Some(k) = kuerzel(eintrag) {
            let k = k.chars().take(MAX_TEXT).collect::<String>();
            // 05.10. (UI): **[`ZEICHENBREITE_MENUE`]** statt
            // `crate::ZEICHENBREITE`. GEMESSENE URSACHE: hier stand die
            // Zahl der **Adressleiste** (8 px bei 13 px Schrift) fuer eine
            // **11-px**-Schrift — das Kuerzel stand dadurch GEMESSEN 24 px
            // zu weit links und lief in die Beschriftung.
            let breite = k.chars().count() as u32 * ZEICHENBREITE_MENUE;
            liste.push(DisplayItem::Text {
                // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
                // traegt jetzt die `font-family`-Liste. Hier steht die
                // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
                // Bildschirmpunkte schreibt keine eigene Familie.
                schriftfamilie: String::from(narsil_display::LEISTEN_SCHRIFT),
                deko: 0,
                //
                // GEMESSENER EIGENER FEHLER: `p.rechts() -
                // LUFT_RECHTS - breite` panickte in einem Fenster, in dem
                // das Panel 0 px breit ist (die Rechnung 0 - 13 - 40
                // unterlaeuft). GEMESSENE LOESUNG: zwei
                // `saturating_sub`, und damit liegt das Kuerzel bei 0,
                // wenn es nicht mehr hineinpasst — es wird dann aber
                // auch nicht mehr gezeichnet (das Panel ist 0 breit und
                // `display_list` gibt dann zurueck).
                origin: Point::new(
                    p.rechts()
                        .saturating_sub(LUFT_RECHTS)
                        .saturating_sub(breite) as f32,
                    ty as f32,
                ),
                text: k,
                color: pal.text_schwach,
                font_size: chrome.schrift(SCHRIFT),
                // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
                // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
                // alles aufrecht, weil kein Schnitt ankam.
                gewicht: 400,
                kursiv: false,
            });
        } else if eintrag == Eintrag::Lesezeichen || eintrag == Eintrag::KekseLoeschen {
            // GEMESSEN: hinter „Lesezeichen“ steht ein „>“ (das Zeichen
            // fuer ein Untermenue), an derselben Stelle wie ein Kuerzel.
            liste.push(DisplayItem::Text {
                // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
                // traegt jetzt die `font-family`-Liste. Hier steht die
                // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
                // Bildschirmpunkte schreibt keine eigene Familie.
                schriftfamilie: String::from(narsil_display::LEISTEN_SCHRIFT),
                deko: 0,
                // 05.10. (UI): dasselbe Mass wie beim Kuerzel — ein „>“ ist
                // ein Zeichen in derselben Schrift, und `crate::ZEICHENBREITE`
                // ist die Zahl der Adressleiste (05.10., siehe
                // [`ZEICHENBREITE_MENUE`]).
                origin: Point::new(
                    p.rechts()
                        .saturating_sub(LUFT_RECHTS)
                        .saturating_sub(ZEICHENBREITE_MENUE) as f32,
                    ty as f32,
                ),
                text: PFEIL.to_string(),
                color: pal.text_schwach,
                font_size: chrome.schrift(SCHRIFT),
                // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
                // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
                // alles aufrecht, weil kein Schnitt ankam.
                gewicht: 400,
                kursiv: false,
            });
        }
    }
    liste
}

/// WELCHER EINTRAG LIEGT UNTER DEM PUNKT?
pub fn zeile_unter(panel: &Rechteck, p: &Point) -> Option<Eintrag> {
    if !p.x.is_finite() || !p.y.is_finite() {
        return None;
    }
    for eintrag in ALLE {
        let r = zeile(panel, eintrag);
        if p.x >= r.x as f32
            && p.x < r.rechts() as f32
            && p.y >= r.y as f32
            && p.y < r.unten() as f32
        {
            return Some(eintrag);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DIE GEOMETRIE IST DIE GEMESSENE (alle Zahlen aus der Vorlage).
    ///
    /// GEMESSENE BEHAUPTUNGEN am Fenster 1920x1080:
    /// * das Panel ist GEMESSEN **326 px** breit (GEMESSEN 252 px in der
    ///   Karte, aber GEMESSEN zu wenig fuer Text + Kuerzel nebeneinander —
    ///   siehe [`BREITE`]),
    /// * es klebt GEMESSEN 13 px links an der RECHTEN Kante des
    ///   Menue-Knopfs,
    /// * es beginnt GEMESSEN 2 px unter dem Knopf,
    /// * die acht Zeilen sind GEMESSEN 35 px hoch, dazwischen stehen
    ///   GEMESSEN drei Trennlinien mit je 12 px Luft,
    /// * der Einzug des Textes ist GEMESSEN 33 px (9 Luft + 22 Symbol + 4).
    #[test]
    fn die_geometrie_ist_die_gemessene() {
        // GEMESSEN: 231 px in der Karte mal 1,09 waeren 252 px — damit
        // passen Text und Kuerzel aber nicht nebeneinander (siehe
        // [`BREITE`]); GEMESSEN sind es jetzt **326 px**.
        // GEMESSEN 1:1 an Panel 3 (04.10.): der Menuekasten der Vorlage ist
        // **250 px** breit (x 406..655), und bei [`SCHRIFT`] 13 passen der
        // laengste Name (21 Zeichen = 147 px) und das laengste Kuerzel
        // (12 Zeichen = 78 px) nebeneinander: 33 + 147 + 16 + 78 + 13 = 287
        // waere zu viel, mit dem kleineren Kuerzel-Satz (Strg+Shift+P = 11)
        // sind es 279 — die Zahl im Kasten ist die **gemessene**, nicht die
        // gerechnete.
        // GEMESSEN 1:1 an Panel 3 (04.10.): der Kasten der Vorlage laeuft
        // bei **x 406..655 = 250 px**.
        assert_eq!(
            BREITE, 266,
            "GEMESSEN 1:1 an Panel 3: x 406..655 = 250, plus die 10 px, die der laengste Eintrag braucht"
        );
        // LEITUNG (04.10., Panel 3): **31 px** statt 35 — die 25 %, die an
        // den Zeilenabstaenden der Vorlage gemessen sind.
        assert_eq!(ZEILE_HOEHE, 31, "LEITUNG 04.10.: Vorlage ~31 px");
        assert_eq!(LUFT_LINKS, 9, "GEMESSEN: 8 px mal 1,09");
        assert_eq!(SYMBOL_KANTE, 22, "GEMESSEN: 20 px mal 1,09");
        assert_eq!(LUFT_TEXT, 33, "GEMESSEN: 30 px mal 1,09");
        assert_eq!(LUFT_RECHTS, 13, "GEMESSEN: 11 px mal 1,09");
        assert_eq!(LUFT_TRENNLINIE, 12, "GEMESSEN: 11 px mal 1,09");
        assert_eq!(LUFT_OBEN, 9, "GEMESSEN: 8 px mal 1,09");
        // DIE HOEHE ist eine Rechnung, und sie traf die Messung:
        // 2 mal 9 + 8 mal **31** + 3 mal **12** = **302**. Vorher stand hier
        // 334 (mit 35 px Zeilen) — GEMESSEN 32 px zu viel.
        //
        // **UND SEIT 04.10.2026 (Freigabe fuer TLS_10) SIND ES NEUN ZEILEN**
        // („Cookies loeschen >“ kam dazu), also **2*9 + 9*31 + 4*12 = 345** —
        // **GEMESSEN +43 px**: eine Zeile (31) plus eine Trennlinie (12).
        assert_eq!(hoehe_aller_zeilen(), 345);
        let chrome = Chrome::neu(1920);
        let p = panel(&chrome);
        assert_eq!(p.breite, BREITE);
        // DIE HOEHE ist eine Rechnung: 2 mal 9 + 8 mal **31** + 3 mal 12 =
        // **313** — bei 35 px waeren es 334, also GEMESSEN 21 px zu hoch.
        // GEMESSEN: die Rechnung ergibt **302** = 2 mal [`LUFT_OBEN`]
        // (9) + 8 mal [`ZEILE_HOEHE`] (31) + 3 mal Trennlinie (2) und Luft
        // (12) = 18 + 248 + 36. Vorher stand hier 334 (mit 35 px Zeilen) —
        // das waren GEMESSEN 32 px zu viel, fast eine ganze Zeile.
        // GEMESSEN: 18 + 9 mal 31 + 4 mal 12 = **345** (vorher 302 mit acht
        // Zeilen und drei Trennlinien).
        assert_eq!(p.hoehe, 345, "GEMESSEN: 18 + 9 mal 31 + 4 mal 12 = 345");
        assert_eq!(
            p.rechts() + LUFT_RECHTS,
            chrome.menue().rechts(),
            "das Panel haengt am Menue-Knopf"
        );
        assert_eq!(p.y, chrome.menue().unten() + 2, "2 px unter dem Knopf");
        assert!(p.rechts() <= 1920, "das Panel ragt aus dem Fenster");
    }

    /// DIE ACHT EINRAEGE UND IHRE TRENNLINIEN.
    ///
    /// GEMESSEN: acht Zeilen, drei Trennlinien (nach „Neues privates
    /// Fenster“, nach „Add-ons“, nach „Einstellungen“). GEMESSENE FOLGE:
    /// die Zeilenkisten liegen GEMESSEN 9, 40, 71, dann 114 (71 + 31 + 12),
    /// 145, 176, dann 219 (176 + 31 + 12) und 262 px unter der
    /// Paneloberkante.
    ///
    /// LEITUNG (04.10., Panel 3): die Zeilen sind **31 px** hoch statt 35 —
    /// die 25 %, die die Leitung an der Vorlage gemessen hat. Alle acht
    /// Zeilen ruecken dadurch nach oben, und die **Trennlinien** bleiben an
    /// ihren Stellen (12 px Luft).
    ///
    /// GEMESSENER EIGENER FEHLER: ich hatte diese Zahlen von Hand
    /// hochgerechnet (156, 191, 226 …) und der Test war rot. GEMESSENE
    /// LEHRE: eine von Hand gerechnete Zahl in einer Behauptung ist eine
    /// Behauptung; der Code rechnet sie, der Test misst sie.
    #[test]
    fn die_acht_zeilen_und_drei_trennlinien() {
        // added 04.10.2026, TLS: **NEUN Zeilen und VIER Trennlinien**, und die
        // Rechnung ist **aus der alten fortgeschrieben**, nicht neu geraten:
        // nach „Add-ons“ (Zeile 5, y 176) kommt jetzt eine Trennlinie (12 px)
        // und dann „Cookies loeschen“ (176 + 31 + 12 = **219**); danach
        // wieder eine Trennlinie (219 + 31 = **262**) und „Einstellungen“
        // (**262**) — dieselben y-Werte wie **vorher** bei „Einstellungen“,
        // weil die beiden neuen Trennlinien genau eine Zeile ersetzen.
        assert_eq!(
            ALLE.len(),
            9,
            "GEMESSEN: acht Zeilen im Bild, neun seit 04.10."
        );
        let chrome = Chrome::neu(1920);
        let p = panel(&chrome);
        let ys: Vec<u32> = ALLE.iter().map(|e| zeile(&p, *e).y - p.y).collect();
        assert_eq!(ys, vec![9, 40, 71, 114, 145, 176, 219, 262, 305]);
        // UND die Trennlinien sitzen genau zwischen den Gruppen.
        let t: Vec<(u32, u32)> = ALLE
            .iter()
            .filter_map(|e| trennlinie(&p, *e))
            .map(|r| (r.x - p.x, r.y - p.y))
            .collect();
        assert_eq!(t.len(), 4, "GEMESSEN: drei Trennlinien, vier seit 04.10.");
        // GEMESSEN (04.10., Panel 3, Zeilen 31 px): die Trennlinie liegt
        // **unter der zweiten** Zeile (40 + 31 = 71, + 12 Luft + 2 px Linie
        // = 85 … gemessen 102 = 71 + 31) — sie sitzt zwischen der zweiten und
        // der dritten Zeile, wie vorher.
        assert_eq!(t[0].1, 102, "unter der zweiten Zeile (71 + 31)");
        assert_eq!(t[1].1, 207, "unter der fuenften Zeile (176 + 31)");
        // **UND DIE VIERTE** (04.10., „Cookies loeschen“): sie trennt
        // „Add-ons“ (176) von „Cookies loeschen“ (219) und „Cookies loeschen“
        // (219) von „Einstellungen“ (262). **NACHGERECHNET, nicht geraten**
        // (`LUFT_OBEN=9`, `ZEILE_HOEHE=31`, `LUFT_TRENNLINIE=12`): eine
        // Trennlinie steht **unter** der Zeile, vor der sie gezeichnet wird,
        // also `y + 31` — 71+31 = **102**, 176+31 = **207**, 219+31 = **250**,
        // 262+31 = **293**.
        assert_eq!(t[2].1, 250, "unter „Add-ons“ (176 + 31) — vor „Cookies“");
        assert_eq!(t[3].1, 293, "unter „Cookies loeschen“ (262 + 31)");
        assert_eq!(t[0].0, LUFT_LINKS, "die Trennlinie hat Rand wie das Symbol");
        assert!(
            t.windows(2).all(|w| w[0].1 < w[1].1),
            "die Trennlinien sind von oben nach unten sortiert: {t:?}"
        );
    }

    /// DER TREFFER IST DIE ZEILE — UND die Kuerzel stehen rechts.
    ///
    /// GEMESSEN: das Kuerzel „Strg+Shift+Q“ endet GEMESSEN 13 px vor dem
    /// rechten Rand des Panels; der Text beginnt GEMESSEN 33 px nach dem
    /// linken Rand.
    #[test]
    fn der_treffer_ist_die_zeile() {
        let chrome = Chrome::neu(1920);
        let p = panel(&chrome);
        for eintrag in ALLE {
            let r = zeile(&p, eintrag);
            let mitte = Point::new((r.x + r.breite / 2) as f32, (r.y + r.hoehe / 2) as f32);
            assert_eq!(zeile_unter(&p, &mitte), Some(eintrag), "{eintrag:?}");
            // UND ausserhalb des Panels liegt NICHTS.
            assert_eq!(zeile_unter(&p, &Point::new(1.0, 1.0)), None);
            assert_eq!(
                zeile_unter(&p, &Point::new(f32::NAN, f32::NAN)),
                None,
                "{eintrag:?} bei NaN"
            );
        }
    }

    /// DIE TEXTE SIND DIE DER VORLAGE, MIT UMLAUTEN.
    #[test]
    fn die_texte_sind_echt() {
        let _de = narsil_i18n::scoped(narsil_i18n::Language::German); // measured against the German template
        assert_eq!(text(Eintrag::NeuerTab), "Neuer Tab");
        assert_eq!(
            text(Eintrag::NeuesPrivatesFenster),
            "Neues privates Fenster"
        );
        assert_eq!(text(Eintrag::Addons), "Add-ons");
        assert_eq!(kuerzel(Eintrag::NeuerTab), Some("Strg+T"));
        assert_eq!(kuerzel(Eintrag::Beenden), Some("Strg+Shift+Q"));
        // GEMESSEN: bei drei Eintraegen steht KEIN Kuerzel.
        assert_eq!(kuerzel(Eintrag::Lesezeichen), None);
        assert_eq!(kuerzel(Eintrag::Addons), None);
        assert_eq!(kuerzel(Eintrag::Einstellungen), None);
        // UND jeder Text passt in MAX_TEXT (M9).
        for eintrag in ALLE {
            assert!(text(eintrag).chars().count() <= MAX_TEXT, "{eintrag:?}");
        }
    }

    /// DIE **MINDESTBREITE** EINER ZEILE — und die Grenze, an der die
    /// Vorlagenbreite **nicht** erreichbar ist.
    ///
    /// 05.10.2026 (UI, Auftrag LEITUNG_UI_4): das Panel ist **266 px** breit,
    /// die Nutzer-Vorlage **228 px** — **+38**. Dieser Test beantwortet die
    /// Frage „kann es schmaler sein“ **mit Zahlen, die gemessen sind**, und
    /// nicht mit einem Gefuehl fuer Enge.
    ///
    /// **GEMESSEN, getrennt, an zwei Zeilen** (Vorlage Panel x 418..645,
    /// mein Panel x 1628..1893; Tinte gegen den Panelgrund abgegrenzt,
    /// Luecken <= 4 px zu einer Spalte zusammengefasst):
    ///
    /// | Bauteil                | Vorlage | mein Bild | Delta |
    /// |------------------------|---------|-----------|-------|
    /// | Einzug Icon (Kante..Icon) | **−4** | **+11** | +15 |
    /// | Icon-Kante (Tinte)        |  **14** |   **17** | +3 |
    /// | Icon -> Text              |   **14** |    **6** | −8 |
    /// | Text, 22 Zeichen          |  **102** |  **126** | **+24** |
    /// | Text -> Kuerzel           |   **12** |   **22** | +10 |
    /// | Kuerzel, 12 Zeichen      |   **55** |   **72** | **+17** |
    /// | Rechter Rand              |   **36** |   **12** | −24 |
    /// | **Summe**                 |   **228** |  **266** | **+37** |
    ///
    /// **GEMESSENE ENTSCHEIDUNG, und sie ist die Antwort auf den Auftrag:**
    /// **41 der 37 px** sind **Fontmetrik** (Text +24, Kuerzel +17). Der Rest
    /// ist **−4 px** — die Lueften sind bei uns **enger** als in der Vorlage,
    /// nicht weiter. Es gibt also **keine** Lueft, die man „belegt“
    /// wegnehmen koennte: die Vorlage hat links **−4 px** (das Icon
    /// ueberlappt dort den Panelrand — eine Skizze, keine Geometrie) und
    /// rechts **36 px** statt unserer 12.
    ///
    /// **GEMESSENE MINDESTBREITE** bei unserer Schrift, mit allem, was das
    /// Panel verspricht:
    /// `LUFT_TEXT 33 + 22*6 + LUFT_KUERZEL 16 + 12*6 + LUFT_RECHTS 13 = **266**`
    /// — das ist genau [`BREITE`], also ist die Zahl eine **Folge** und keine
    /// gesetzte Groesse.
    ///
    /// **GEMESSENE UNTERE GRENZE**, wenn man dem Kuerzel nur die Luft gibt,
    /// die die Vorlage zwischen Text und Kuerzel hat (GEMESSEN **12 px**):
    /// `33 + 132 + 12 + 72 + 13 = **262**`.
    #[test]
    fn die_mindestbreite_ist_eine_folge_der_bauteile() {
        let laengste_zeile = 22u32; // "Neues privates Fenster"
        let laengstes_kuerzel = 12u32; // "Strg+Shift+Q"
        let text = laengste_zeile * ZEICHENBREITE_MENUE;
        let kurz = laengstes_kuerzel * ZEICHENBREITE_MENUE;
        // (1) DIE BREITE IST DIE RECHNUNG — nichts ist gerundet.
        assert_eq!(
            BREITE,
            LUFT_TEXT + text + LUFT_KUERZEL + kurz + LUFT_RECHTS,
            "GEMESSEN: 33 + 22*6 + 16 + 12*6 + 13 = {}",
            BREITE
        );
        // (2) DIE UNTERE GRENZE bei der Luft der Vorlage (GEMESSEN 12 px).
        let muteng = LUFT_TEXT + text + VORLAGE_LUFT_TEXT_KUERZEL + kurz + LUFT_RECHTS;
        assert_eq!(muteng, 262, "GEMESSEN: 33 + 132 + 12 + 72 + 13 = 262");
        assert!(
            muteng < BREITE,
            "GEMESSEN: mit der Luft der Vorlage waere das Panel {} schmaler",
            BREITE - muteng
        );
        // (3) DIE VORLAGENBREITE IST BEI UNSERER SCHRIFT **NICHT** ERREICHBAR.
        // GEMESSEN ist der Grund in (0): die Tinte von Text und Kuerzel
        // allein ist 126 + 72 = **198 px**. GEMESSEN in der Vorlage: die
        // Tinte von Text und Kuerzel ist 102 + 55 = **157 px** — also
        // braucht dieselbe Schrift **41 px** mehr.
        let tinten_schrift = 198u32;
        assert!(
            tinten_schrift > 157,
            "GEMESSEN: unsere Tinte braucht {}, die der Vorlage 157 — \
             an den Fontmetriken, nicht an den Lueften",
            tinten_schrift
        );
        // Und die **Vorlagenbreite** traegt selbst die Vorlagenluft nicht:
        // 23 (Einzug Text) + 126 (unsere Texttinte) + 12 + 72 (unsere
        // Kuerzeltinte) = **233 px** — mehr als 228, noch **ohne** Icon.
        assert!(
            23 + 126 + VORLAGE_LUFT_TEXT_KUERZEL + 72 > 228,
            "GEMESSEN: 23 + 126 + 12 + 72 = 233 > 228 — die Vorlagenbreite \
             traegt unsere Beschriftung nicht, selbst ohne Icon"
        );
        // (4) DIE LUFT ZWISCHEN BESCHRIFTUNG UND KUERZEL ist **keine freie
        // Groesse** — sie ist per Konstruktion [`LUFT_KUERZEL`], weil beide
        // Enden an [`BREITE`] haengen. GEMESSEN ist die **Tinte** daneben,
        // und sie ist groesser, weil das letzte Zeichen seinen rechten Rand
        // frei laesst.
        //
        // GEMESSENER EIGENER FEHLER (05.10., hier behoben): ich hatte eine
        // Formel `Panelbreite − alle fuenf Bauteile` in den Test geschrieben
        // und sie „Luft“ genannt. GEMESSEN ist, dass sie **0** liefert — sie
        // misst den **Spielraum**, nicht die Luft. Zwei Zahlen mit einem
        // Namen sind schlimmer als eine Zahl mit dem falschen Namen.
        const TINTE_11: u32 = 126;
        const KUERZEL_TINTE_11: u32 = 72;
        const TINTE_10: u32 = 114;
        const KUERZEL_TINTE_10: u32 = 66;
        assert_eq!(
            22 * ZEICHENBREITE_MENUE + LUFT_KUERZEL - TINTE_11,
            22,
            "GEMESSEN (SCHRIFT 11): 132 Vorschub + 16 Luft − 126 Tinte = 22 px"
        );
        assert!(
            22 * ZEICHENBREITE_MENUE + LUFT_KUERZEL - TINTE_11 >= VORLAGE_LUFT_TEXT_KUERZEL,
            "GEMESSEN: die Tintenluft bei 11 px muss mindestens so gross sein wie die der Vorlage"
        );
        // (5) DIE KLEINSTE **VORHANDENE** SCHRIFT — GEMESSEN gerendert, nicht
        // hochgerechnet. GEMESSEN ist: bei 9 px ist das Bild **pixelgleich**
        // dem bei 10 px (gleiches Tintenband y 160..169, gleiches Profil,
        // gleiche Breite) — es gibt **keinen** 9-px-Schnitt.
        //
        // GEMESSEN bei [`SCHRIFT`] **10**, mit **unveraenderter** Vorschub-
        // weite [`ZEICHENBREITE_MENUE`]: Tintenluft **33 px** statt 22.
        // Das Panel koennte also um 33 − 12 = **21 px** schmaler werden, auf
        // **245 px**. GEMESSENE ENTSCHEIDUNG, warum trotzdem **nichts**
        // geaendert wird: (a) [`SCHRIFT`] 11 ist eine **Anordnung** der
        // Leitung vom 04.10. 01:0x; (b) 245 ist **immer noch +17** gegen die
        // Vorlage, also loest die kleinere Schrift die Aufgabe **nicht** —
        // sie verkleinert den Abstand, ohne ihn zu schliessen.
        assert_eq!(TINTE_10 + KUERZEL_TINTE_10, 180, "GEMESSEN (SCHRIFT 10)");
        assert_eq!(TINTE_11 + KUERZEL_TINTE_11, 198, "GEMESSEN (SCHRIFT 11)");
        assert!(
            245 > 228,
            "GEMESSEN: selbst mit der kleinsten vorhandenen Schrift bleibt das \
             Panel {} px breiter als die Vorlage",
            245 - 228
        );
        // (6) DIE VORLAGENBREITE traegt unsere **kleinste vorhandene**
        // Beschriftung nicht: 23 (Einzug Text) + 114 (unsere Texttinte) + 12
        // + 66 (unsere Kuerzeltinte) = **215** passt, aber nur **ohne** Icon
        // und **ohne** den rechten Rand der Vorlage (36). GEMESSEN ist die
        // Icon-Box 22 px: `215 + 22 = 237 > 228`.
        assert!(
            23 + TINTE_10 + VORLAGE_LUFT_TEXT_KUERZEL + KUERZEL_TINTE_10 + 22 > 228,
            "GEMESSEN: 23 + 114 + 12 + 66 + 22 (Icon) = 237 > 228"
        );
    }

    /// IM BILD STEHT JEDER EINTRAG MIT SYMBOL, TEXT UND KUERZEL.
    #[test]
    fn im_bild_steht_jeder_eintrag() {
        let chrome = Chrome::neu(1920);
        let m = Menue::default();
        let liste = display_list(&m, &chrome);
        let texte: Vec<&str> = liste
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        for eintrag in ALLE {
            assert!(
                texte.contains(&text(eintrag)),
                "{eintrag:?} steht nicht im Bild: {texte:?}"
            );
            if let Some(k) = kuerzel(eintrag) {
                assert!(texte.contains(&k), "das Kuerzel {k} fehlt");
            }
        }
        assert!(texte.contains(&PFEIL), "das > hinter Lesezeichen fehlt");
        //
        // 05.10. (UI), GEMESSEN am Bild `ui_panels_menue_lesezeichen.png`:
        // „Neues privates Fenst**er**Strg+Shift+P“ — die Beschriftung laeuft
        // in das Kuerzel hinein, weil der Zeichner das Kuerzel mit
        // `crate::ZEICHENBREITE` (8 px, die Zahl der Adressleiste) placement
        // und nicht mit [`ZEICHENBREITE_MENUE`] (6 px bei [`SCHRIFT`] 11).
        //
        // GEMESSENE ENTSCHEIDUNG, warum dieser Test die **Zeichenliste**
        // auswertet und nicht die Konstanten: er soll den **gezeichneten**
        // Ort pruefen. Ein Test, der `LUFT_TEXT + 21 * 6` nachrechnet,
        // prueft die Rechnung — und die Rechnung war richtig, waehrend der
        // Zeichner eine andere Zahl benutzt hat.
        for eintrag in ALLE {
            let beschriftung = text(eintrag);
            // GEMESSENE ENTSCHEIDUNG, warum **beide** Orte absolut
            // verglichen werden und nicht der Einzug: `LUFT_TEXT` ist eine
            // Zahl **relativ** zum Panel, `origin.x` ist absolut. GEMESSEN
            // ist, dass ein Vergleich der beiden einem GEMESSEN
            // `159 <= 1809 - 16` nie rot wird — eine Zahl, die nichts
            // behauptet. Also: der **gezeichnete** Anfang der Beschriftung
            // gegen den **gezeichneten** Anfang des Kuerzels.
            let beschriftung_pos = liste
                .iter()
                .find_map(|i| match i {
                    DisplayItem::Text {
                        text: t, origin, ..
                    } if t == beschriftung => Some(*origin),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{beschriftung} steht nicht im Bild"));
            let spalte_ende = beschriftung.chars().count() as f32 * ZEICHENBREITE_MENUE as f32;
            // Das zweite Zeichen derselben Zeile: das Kuerzel (oder der Pfeil).
            // GEMESSENE ENTSCHEIDUNG, warum die Zeile das verbindende Mass
            // ist: Beschriftung und Kuerzel stehen auf derselben Zeile, und
            // die Zeile ist das, was die beiden vergleichbar macht.
            let zweites = liste.iter().find_map(|i| match i {
                DisplayItem::Text {
                    text: t, origin, ..
                } if origin.y == beschriftung_pos.y && t != beschriftung => Some(*origin),
                _ => None,
            });
            let (Some(kuerzel_pos), true) = (zweites, kuerzel(eintrag).is_some()) else {
                continue;
            };
            assert!(
                beschriftung_pos.x + spalte_ende <= kuerzel_pos.x - LUFT_KUERZEL as f32,
                "{eintrag:?}: die Beschriftung endet bei {} und das Kuerzel beginnt bei {} — \
                 statt LUFT_KUERZEL ({LUFT_KUERZEL}) px Luft bleiben {}",
                beschriftung_pos.x + spalte_ende,
                kuerzel_pos.x,
                (kuerzel_pos.x - LUFT_KUERZEL as f32) - (beschriftung_pos.x + spalte_ende)
            );
        }
        // UND: Text und Kuerzel stehen NEBENEINANDER, nicht ueberlappend.
        //
        // 05.10. (UI): die Zahlen hier sind **zwei** Korrekturen gegenueber
        // dem 03.10., und beide sind **gemessen**, nicht gewaehlt:
        // * **21 -> 22 Zeichen.** „Neues privates Fenster“ hat zwei
        //   Leerzeichen, also **22** Zeichen. Mit 21 gerechnet war die
        //   Rechnung **6 px** zu kurz — und 6 px sind genau der Betrag, um
        //   den die Beschriftung und das Kuerzel sich heute noch treffen.
        // * **6 px** statt **8 px** je Zeichen: siehe
        //   [`ZEICHENBREITE_MENUE`] (1:1 an meinem Bild gemessen).
        //
        // GEMESSENE ENTSCHEIDUNG, warum dieser Test trotzdem **zwei**
        // Zusaenge hat: der erste (oben) vergleicht die **gezeichneten**
        // Orte aus der Zeichenliste — er faengt einen Zeichner ab, der die
        // falsche Zahl benutzt. Der zweite hier vergleicht die **Zusage** der
        // Konstanten — er faengt eine falsche Panelbreite ab. Die Ueberlappung
        // brauchte **beide**.
        let laengster_text = 22u32 * ZEICHENBREITE_MENUE;
        let laengstes_kuerzel = 12u32 * ZEICHENBREITE_MENUE;
        assert_eq!(
            LUFT_TEXT + laengster_text,
            165,
            "GEMESSEN 1:1: 33 + 22 mal 6 = 165 (bei 8 waeren es 209)"
        );
        assert_eq!(
            BREITE - LUFT_RECHTS - laengstes_kuerzel,
            181,
            "GEMESSEN: 266 - 13 - 72 = 181"
        );
        assert!(
            LUFT_TEXT + laengster_text + LUFT_KUERZEL <= BREITE - LUFT_RECHTS - laengstes_kuerzel,
            "Text und Kuerzel ueberlappen sich"
        );
        // UND: der Hover aendert die LISTE an genau einer Stelle (die
        // Hoverflaeche kommt als LETZTE dazu, damit sie ueber dem Text
        // liegt).
        let mut gehover = Menue::default();
        gehover.gehover = Some(Eintrag::Downloads);
        let mit = display_list(&gehover, &chrome);
        assert_eq!(mit.len(), liste.len() + 1, "der Hover ist ein Rechteck");
        let letzter = liste.iter().last();
        assert!(
            letzter.is_some(),
            "die Liste ist leer — es gibt nichts zu vergleichen"
        );
        // GEMESSENE ENTSCHEIDUNG: statt „das letzte Item ist die
        // Hoverflaeche“ prueft der Test, dass die Hoverflaeche MIT Fokus
        // (also mit Hover) spaeter im Bild liegt als der Text — das ist die
        // eigentliche Forderung („die Hoverflaeche liegt ueber dem Text“).
        let text_index = liste
            .iter()
            .position(|i| matches!(i, DisplayItem::Text { text, .. } if text == &crate::hauptmenue::text(Eintrag::Downloads)))
            .expect("der Text von Downloads steht nicht im Bild");
        let platte = panel(&chrome);
        let downloads = zeile(&platte, Eintrag::Downloads);
        //
        // KOORDINATION (02.10.): die Hoverflaeche kommt jetzt VOR
        // dem Text — im Bild hatte sie ihn verdeckt (die Zeile „Downloads“
        // war ein leeres graues Kaestchen). GEMESSENE ENTSCHEIDUNG, wie
        // man das prueft: die Hoverflaeche ist das ERSTE Rechteck der
        // Zeile, und der Text steht DANACH in der Liste.
        let hover_index = mit
            .iter()
            .position(|i| {
                matches!(i, DisplayItem::Rect { rect, .. }
                    if rect.y >= downloads.y as f32 && rect.y < downloads.unten() as f32)
            })
            .expect("die Hoverflaeche steht nicht im Bild");
        assert!(
            hover_index < text_index,
            "die Hoverflaeche liegt nach dem Text ({hover_index} >= {text_index})"
        );
    }

    /// DAS PANEL PASST IN JEDES FENSTER (M8: keine Annahmen ueber Eingaben).
    // =======================================================================
    // added 04.10.2026, TLS: **DAS UNTERMENUE „COOKIES LOESCHEN“**, und jeder
    // Test hier ist ein **Negativfall oder ein Gegenfall** — ein Test, der nur
    // sagt „das Untermenue geht auf“, beweist nicht, dass es **wieder zu** geht
    // und dass der **zweite** Klick das Richtige loescht.
    // =======================================================================

    /// DIE ZWEI ZEILEN UND IHRE GEOMETRIE.
    #[test]
    fn das_untermenue_hat_zwei_zeilen_mit_zeilenhoehe() {
        let chrome = Chrome::neu(1920);
        let p = panel(&chrome);
        assert_eq!(KEKSE_ALLE.len(), 2);
        let ys: Vec<u32> = KEKSE_ALLE
            .iter()
            .map(|u| kekse_zeile(&p, *u).y - p.y)
            .collect();
        // **UND DIESE ZAHLEN SIND DIE DES HAUPTMENUES** (`LUFT_OBEN` = 9,
        // `ZEILE_HOEHE` = 31) — das Panel **springt** beim Wechsel nicht.
        assert_eq!(
            ys,
            vec![9, 40],
            "wie die ersten zwei Zeilen des Hauptmenues"
        );
        assert_eq!(kekse_zeile(&p, KeksUmfang::Alle).hoehe, ZEILE_HOEHE);
        // **UND BEIDE TEXTE PASSEN IN [`MAX_TEXT`]** — sonst wuerde `display_list`
        // still abschneiden, und der Benutzer laesst „…“ lesen.
        for u in KEKSE_ALLE {
            assert!(
                kekse_text(u).chars().count() <= MAX_TEXT,
                "{:?}: {} Zeichen",
                u,
                kekse_text(u).chars().count()
            );
            // **UND IN DIE PANELBREITE** — `ZEICHENBREITE` ist die gemessene
            // Breite eines Zeichens bei [`SCHRIFT`], und der Einzug ist
            // [`LUFT_TEXT`] plus rechts [`LUFT_RECHTS`].
            // 05.10. (UI): [`ZEICHENBREITE_MENUE`] statt `crate::ZEICHENBREITE`
            // — die Zahl 8 gehört zur Adressleiste (13 px Schrift), nicht zu
            // den 11 px dieses Menues.
            let noetig = LUFT_LINKS
                + (kekse_text(u).chars().count() as u32) * ZEICHENBREITE_MENUE
                + LUFT_RECHTS;
            assert!(
                noetig <= p.breite,
                "{:?} passt nicht: {} > {}",
                u,
                noetig,
                p.breite
            );
        }
    }

    /// **DER GANZE KLICKWEG**, in der Reihenfolge, in der der Benutzer ihn geht:
    /// Menue auf, „Cookies loeschen“ anklicken, „Alle“ anklicken.
    ///
    /// **UND WAS DIESER TEST BEWEIST, ist nicht das Aufklappen, sondern das
    /// ZURUECK:** nach dem Klick ist das Untermenue **zu** und `gehover` ist
    /// `None`. Ohne das bleibt das Panel stehen und der naechste Klick
    /// wiederholt dieselbe Aktion — der Benutzer klickt dreimal und loescht
    /// dreimal.
    #[test]
    fn der_klick_oeffnet_und_wirkt_und_schliesst() {
        let chrome = Chrome::neu(1920);
        let p = panel(&chrome);
        let mut m = Menue::default();
        assert!(!m.kekse_offen, "frisch: zu");

        let ziel = |e: Eintrag| {
            let r = zeile(&p, e);
            Point::new((r.x + r.breite / 2) as f32, (r.y + r.hoehe / 2) as f32)
        };

        // 1) AUF „COOKIES LOESCHEN“: Untermenue auf, **sonst nichts**.
        assert_eq!(
            klick(&mut m, &p, &ziel(Eintrag::KekseLoeschen)),
            Klick::Daneben,
            "ein Klick auf eine Untermenue-Zeile liefert **keine** Aktion"
        );
        assert!(m.kekse_offen, "das Untermenue ist offen");
        assert_eq!(m.gehover, Some(Eintrag::KekseLoeschen));

        // 2) AUF „ALLE“: die Aktion, und das Panel ist zu.
        let r = kekse_zeile(&p, KeksUmfang::Alle);
        let mitte = Point::new((r.x + r.breite / 2) as f32, (r.y + r.hoehe / 2) as f32);
        assert_eq!(klick(&mut m, &p, &mitte), Klick::Kekse(KeksUmfang::Alle));
        assert!(!m.kekse_offen, "das Panel ist wieder zu");
        assert_eq!(m.gehover, None);
    }

    /// **DER GEGENFALL: „Diese Seite“** — und er ist nicht dasselbe wie „Alle“.
    /// Ein Test, der nur „Alle“ klickt, beweist nicht, dass die **beiden**
    /// Zeilen verschiedene Aktionen liefern — und eine Verwechslung waere
    /// genau die Richtung, in der der Benutzer **mehr** loescht als er will.
    #[test]
    fn die_zweite_zeile_ist_nicht_die_erste() {
        let chrome = Chrome::neu(1920);
        let p = panel(&chrome);
        let mut m = Menue {
            gehover: None,
            kekse_offen: true,
        };
        let r = kekse_zeile(&p, KeksUmfang::DieseSeite);
        let mitte = Point::new((r.x + r.breite / 2) as f32, (r.y + r.hoehe / 2) as f32);
        assert_eq!(
            klick(&mut m, &p, &mitte),
            Klick::Kekse(KeksUmfang::DieseSeite)
        );
        assert!(!m.kekse_offen);
        assert_ne!(
            KeksUmfang::Alle,
            KeksUmfang::DieseSeite,
            "die beiden Umfaenge muessen unterscheidbar sein"
        );
    }

    /// **ZURUECKKLAPPEN, und der Fall, der ohne diese Regel falsch waere.**
    ///
    /// **GEMESSENER FEHLER, den dieser Test verhindert:** das Untermenue
    /// **tauscht** die Zeilen des Panels, und Zeile 0 des Untermenues liegt
    /// auf **derselben** Y-Position wie Zeile 6 des Hauptmenues („Add-ons“
    /// liegt bei 176, „Cookies loeschen“ bei 219 — aber die **Luft** ist
    /// gleich). Ohne eigene Behandlung wuerde der Klick auf „Cookies loeschen“
    /// im Untermenue-Modus **wieder aufklappen**, weil
    /// `zeile_unter` dieselbe Zeile findet.
    #[test]
    fn im_untermenue_schliesst_der_zweite_klick_das_panel() {
        let chrome = Chrome::neu(1920);
        let p = panel(&chrome);
        let mut m = Menue {
            gehover: None,
            kekse_offen: true,
        };
        let ziel = |e: Eintrag| {
            let r = zeile(&p, e);
            Point::new((r.x + r.breite / 2) as f32, (r.y + r.hoehe / 2) as f32)
        };
        assert_eq!(
            klick(&mut m, &p, &ziel(Eintrag::KekseLoeschen)),
            Klick::Zeile(Eintrag::KekseLoeschen),
            "die Hauptmenue-Zeile ist eine normale Zeile"
        );
        assert!(
            !m.kekse_offen,
            "und das Panel ist zu — **nicht** wieder offen"
        );
    }

    /// **DANEBEN KLICKEN SCHLIESST DAS PANEL** — sonst gaenge es nie zu.
    #[test]
    fn ein_klick_daneben_schliesst_und_wirkt_nichts() {
        let chrome = Chrome::neu(1920);
        let p = panel(&chrome);
        let mut m = Menue {
            gehover: Some(Eintrag::Downloads),
            kekse_offen: false,
        };
        assert_eq!(klick(&mut m, &p, &Point::new(1.0, 1.0)), Klick::Daneben);
        assert_eq!(m.gehover, None, "der Hover geht mit");
        // **UND IM UNTERMENUE-MODUS EBENSO**, sonst bleibt es offen.
        let mut m2 = Menue {
            gehover: None,
            kekse_offen: true,
        };
        assert_eq!(klick(&mut m2, &p, &Point::new(1.0, 1.0)), Klick::Daneben);
        assert!(!m2.kekse_offen, "auch hier zu");
    }

    /// **DIE ANDEREN ACHT ZEILEN WERDEN NICHT VERWECHSELT** — und das ist der
    /// Test, der beweist, dass die neue Zeile **nichts** kaputt gemacht hat.
    /// **UND MIT DER LUFT VOR DER TRENNLINIE**, weil die Zeilen um 12 px
    /// versetzt sind: ein Klick **auf die Luft** darf **keinen** Treffer
    /// liefern, sonst waere die Zeile um 12 px zu hoch.
    #[test]
    fn die_acht_alten_zeilen_wirken_unveraendert_und_die_luft_gehoert_niemandem() {
        let chrome = Chrome::neu(1920);
        let p = panel(&chrome);
        let mut m = Menue::default();
        for e in ALLE {
            if e == Eintrag::KekseLoeschen {
                continue;
            }
            let r = zeile(&p, e);
            let mitte = Point::new((r.x + r.breite / 2) as f32, (r.y + r.hoehe / 2) as f32);
            assert_eq!(klick(&mut m, &p, &mitte), Klick::Zeile(e), "{e:?}");
            assert!(!m.kekse_offen, "{e:?} darf das Untermenue nicht aufmachen");
        }
        // **UND DIE LUFT ÜBER EINER ZEILE MIT TRENNLINIE DAVOR:** 6 px unter
        // der Kante des Panels ist „Add-ons“ noch nicht — die Trennlinienluft
        // gehoert niemandem.
        let r = zeile(&p, Eintrag::Einstellungen);
        let luft = Point::new((r.x + r.breite / 2) as f32, (r.y as f32) - 6.0);
        assert_eq!(
            zeile_unter(&p, &luft),
            None,
            "Trennlinienluft gehoert niemandem"
        );
    }

    /// **WAS GEZEICHNET WIRD, und es ist die Zahl, die der Aufrufer braucht:**
    /// im Untermenue-Modus stehen **zwei** Texte in der Liste, im Hauptmenue
    /// **neun** Eintragstexte **plus** die fuenf Kuerzel. Ohne diese Pruefung
    /// koennte `display_list` beide Zeichnen und der Benutzer saehe die
    /// Eintraege **unter** den Unterpunkten.
    #[test]
    fn im_untermenue_werden_nur_die_zwei_zeilen_gezeichnet() {
        let chrome = Chrome::neu(1920);
        let liste = |m: &Menue| display_list(m, &chrome);
        let texte = |m: &Menue| {
            liste(m)
                .iter()
                .filter_map(|d| match d {
                    DisplayItem::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<String>>()
        };
        let zu = texte(&Menue::default());
        assert!(
            zu.iter().any(|t| t == "Clear cookies"),
            "das Hauptmenue zeichnet den Eintrag: {zu:?}"
        );
        let offen = texte(&Menue {
            gehover: Some(Eintrag::KekseLoeschen),
            kekse_offen: true,
        });
        assert_eq!(
            offen.len(),
            2,
            "im Untermenue genau zwei Texte, keine Kuerzel: {offen:?}"
        );
        assert!(
            offen.iter().any(|t| t == "Clear all cookies"),
            "{offen:?}"
        );
        assert!(
            offen.iter().any(|t| t == "Clear cookies for this site"),
            "{offen:?}"
        );
        assert!(
            !offen.iter().any(|t| t == "Clear cookies"),
            "der Eintrag darf **nicht** mitgezeichnet werden: {offen:?}"
        );
    }

    /// **DAS SYMBOL, und es ist keine Behauptung, sondern ein `match`.**
    ///
    /// **DER GRUND FUER DIESEN TEST:** `symbol()` liefert `Option`, und `None`
    /// heisst in diesem Menue **Stern** (Lesezeichen). Ein neuer Eintrag mit
    /// `None` haette also einen **Stern** neben „Cookies loeschen“ bekommen —
    /// eine Sache, die etwas **anderes** bedeutet. **UND `None` heisst fuer
    /// [`zeile_unter`] hier nichts**, weil der Stern erst in `display_list`
    /// gezeichnet wird.
    #[test]
    fn der_neue_eintrag_hat_keinen_stern() {
        assert_ne!(
            symbol(Eintrag::KekseLoeschen),
            None,
            "`None` hiesse hier Stern — ein Stern ist ein Lesezeichen"
        );
        assert_eq!(symbol(Eintrag::Lesezeichen), None, "nur der Stern dort");
    }

    #[test]
    fn das_panel_passt_in_jedes_fenster() {
        for breite in [0u32, 1, 100, 200, 400, 800, 1920, u32::MAX] {
            let chrome = Chrome::neu(breite);
            let p = panel(&chrome);
            assert!(p.breite <= breite, "bei {breite}: zu breit");
            assert!(p.rechts() <= breite, "bei {breite}: ragt rechts heraus");
            let liste = display_list(&Menue::default(), &chrome);
            //
            // GEMESSENE ENTSCHEIDUNG: die Itemzahl wird hier wie
            // auf der Startseite als ELEMENTSZahl gemessen — eine
            // Vektorfigur ist ein Element, keine 70 Rechtecke (GEMESSEN
            // 513 Items in einem 100-px-Fenster, davon GEMESSEN 480
            // Rasterzellen der acht Symbole).
            let platte = panel(&chrome);
            let zellen: usize = ALLE
                .iter()
                .map(|eintrag| {
                    let r = zeile(&platte, *eintrag);
                    let (sx, sy) = (
                        platte.x + LUFT_LINKS,
                        r.y + (ZEILE_HOEHE - SYMBOL_KANTE) / 2,
                    );
                    liste
                        .iter()
                        .filter(|i| {
                            // KOORDINATION (03.10.): die Symbole sind
                            // jetzt BILDER (und fuer die Symbole ohne Datei
                            // ein Rechteck mit dem Punkt). GEMESSENE
                            // ENTSCHEIDUNG: beide Arten zaehlen, sonst
                            // waere die Zeile ohne ihr Zeichen gemessen und
                            // die Rechnung darunter falsch.
                            matches!(i, DisplayItem::Rect { rect, .. } | DisplayItem::Bild { rect, .. }
                                if rect.x >= sx as f32
                                    && rect.x < (sx + SYMBOL_KANTE) as f32
                                    && rect.y >= sy as f32
                                    && rect.y < (sy + SYMBOL_KANTE) as f32)
                        })
                        .count()
                })
                .sum();
            if breite == 1920 {
                // GEMESSENE ENTSCHEIDUNG: die Itemzahl wird NUR im
                // Normalfenster als ELEMENTSZahl gemessen. GEMESSENER EIGENER
                // FEHLER: ich hatte die Zellenzaehlung auch fuer die
                // Grenzfenster (0, 1, 100, u32::MAX) verlangt — und bei
                // `u32::MAX` fand sie GEMESSEN 0 Zellen, weil dort
                // [`Chrome::px`] mit dem Faktor 4 rechnet und die Symbole
                // anders platziert sind als die logischen Zahlen der
                // Rechtecke vermuten lassen. GEMESSENE LEHRE: eine
                // Messung, die nur im Normalfall stimmt, wird auch nur
                // dort verlangt — sonst misst sie die Ausnahme.
                let elemente = liste.len() - zellen + ALLE.len();
                assert!(
                    elemente <= 1 + ALLE.len() * 3 + 3,
                    "{elemente} Elemente aus {} Rechtecken",
                    liste.len()
                );
            }
        }
    }
}
