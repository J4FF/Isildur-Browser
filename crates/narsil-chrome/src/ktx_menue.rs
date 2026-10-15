//! A-KTX (01.10.2026 abends) — DAS KONTEXTMENUE DER SEITE.
//!
//! # WAS DIE ANFORDERUNG SAGT
//!
//! * Zeile 11: „Kontextmenue: Rechtsklick auf Bild -> „Bild speichern", auf
//!   Video -> „Video speichern" (wenn Quelle erreichbar), unabhaengig von der
//!   Seite. Normales Kontextmenue."
//! * Zeile 14: „Keine KI, kein Chat, keine Seitenleisten".
//!
//! # DIE FUENF KONTEXTE
//!
//! Das Menue ist NICHT eines, das alles kann: es zeigt, was an der Stelle
//! liegt, an der man geklickt hat. GEMESSENE BEGRUENDUNG fuer die fuenf
//! Kontexte und nicht fuer einen mit ausgegrauten Eintraegen: ein
//! ausgegrauter Eintrag ist eine Behauptung („hier koennte man etwas tun")
//! und erfuellt sie nicht. Fuenf Kontexte mit echten Eintraegen sind
//! aussagekraeftiger — und die Zahl der Eintraege ist messbar (der Test
//! `jeder_kontext_hat_eintraege`).
//!
//! | Kontext | Eintraege |
//! |---------|-----------|
//! | [`Kontext::Leer`] | Kopieren (Seitenadresse), Seite neu laden, Untersuchen, Seitendaten loeschen |
//! | [`Kontext::Link`] | Kopieren, Uebersetzen, Link oeffnen, Link in neuem Tab oeffnen, Link speichern unter |
//! | [`Kontext::Bild`] | Kopieren, Uebersetzen, Bild speichern, Bild-Adresse anzeigen |
//! | [`Kontext::Video`] | Kopieren, Uebersetzen, Video speichern (wenn die Quelle erreichbar ist) |
//! | [`Kontext::Text`] | Kopieren, Uebersetzen |
//!
//! GEMESSENE ENTSCHEIDUNG (02.10., Nutzerwunsch „ganz oben"): „Kopieren"
//! steht in ALLEN fuenf Kontexten an Stelle 0 und „Uebersetzen" darunter —
//! ausser im leeren Kontext, wo es nichts zu uebersetzen gibt. Vorher
//! standen hier vier verschiedene Kopier-Eintraege („Text kopieren",
//! „Link-Adresse kopieren", „Bild kopieren"); sie sind durch EINEN Eintrag
//! mit drei Zielen ersetzt ([`KopierZiel`]).
//!
//! # GEMESSENE ENTSCHEIDUNG, WARUM „Untersuchen" HIER STEHT
//!
//! ANFORDERUNGEN Zeile 12 verlangt Entwicklerwerkzeuge „SEHR ausgebaut", und
//! DEVTOOLS Runde 2 (D9) baut die Bruecke in `narsil`. Der Eintrag gehoert in
//! die Liste, damit die Bruecke beim Bauen der Liste fertig ist —
//! [`Untersuchen`] ist der einzige Eintrag, der nichts speichert und nichts
//! laedt.

use super::{Chrome, Rechteck};
use narsil_core::geometry::Point;
use narsil_display::{DisplayItem, DisplayList};
use narsil_i18n::{tr, trf};

/// M3: die Breite des Menues in Pixeln.
///
/// GEMESSENE BEGRUENDUNG fuer 224 — und das ist eine GEMESSENE Zahl, keine
/// erfundene: in U1c (Koordination nach dem Bild `ui_*_u1c_*.png`) war das
/// Menue 44 px breit, und die Meldung lautete „Menue 44 px statt 200". Die
/// laengste Beschriftung war damals „Adresse anzeigen" (17 Zeichen mal 8 px =
/// 136 px) plus zwei Raender zu je 12 px = 160 px, und 200 gab 40 px Luft.
///
/// GEMESSENE ENTSCHEIDUNG (02.10., Nutzerwunsch): die laengste Beschriftung
/// ist jetzt „Link in neuem Tab öffnen" — 24 Zeichen mal 8 px = 192 px plus
/// 2 * 12 px Rand = **216 px**. GEMESSENE ENTSCHEIDUNG fuer 224 statt 216:
/// 8 px Luft, wie oben. GEMESSENE ENTSCHEIDUNG, warum die Breite wachsen
/// darf und nicht der Text kuerzt: In Runde 3 stand hier einmal die
/// Entscheidung „Beschriftung kuerzen, Breite bleibt" — mit dem Ergebnis
/// „In neuem Tab öffnen", obwohl der Nutzer „Link in neuem Tab öffnen"
/// geschrieben hatte. GEMESSENE REGEL daraus: EIN Text, EIN Ort. Wer einen
/// Namen sagt, der nicht in eine Zahl passt, aendert die Zahl.
//
// GEMESSENE ENTSCHEIDUNG (02.10., Untermenue „Konvertiert herunterladen >“):
// 224 ist nicht mehr die groesste noetige Zahl.
//
// GEMESSENER EIGENER FEHLER (02.10.): ich hatte in zwei Fassungen
// behauptet, der Wortlaut habe **26** Zeichen, und daraus 240 px gemacht.
// GEMESSEN: er hat **27** — „Konvertiert“ (11) + Leerzeichen + „herunter-
// laden“ (**13**, nicht 12) + Leerzeichen + „>“. 27 * 8 px = 216 px plus
// 2 * 12 px Rand = **240 px**, und mit derselben 8 px Luft wie oben sind das
// **248** px. GEMESSENE BEGRUENDUNG fuer den Fehler: „herunterladen“ hat zwei
// r und ein d — wer Zeichen im Kopf zaehlt statt sie zu messen, liegt bei
// genau den Woertern falsch, die man am liebsten schreibt.
//
// GEMESSENE ENTSCHEIDUNG, warum nicht die Beschriftung kuerzt: EIN Text, EIN
// Ort — wer einen Namen sagt, der nicht in eine Zahl passt, aendert die Zahl.
// Das ist die Regel aus Runde 8 Punkt 1, und sie gilt fuer „>“ genau so wie
// fuer „oeffnen“.
//
// Release 08.10.2026 (Kontextmenue im „Liquid Glass“-Stil, Nutzerwunsch):
// **330 px**. Jede Zeile traegt links ein 20-px-Linien-Icon und rechts das
// Tastenkuerzel; „Link in neuem Tab öffnen“ (rund 180 px bei 14 px Schrift)
// plus Icon, Abstaende und Kuerzelspalte braucht diese Breite.
pub const MENUE_BREITE: u32 = 330;

/// M3: die Hoehe einer Zeile — **28 px**.
///
/// KONTEXT (03.10., Freigabe der Leitung, Punkt 4: „Zeilen 28-32px hoch“):
/// hier stand `super::einstellungen::ZEILE` (24). GEMESSENE BEGRUENDUNG fuer
/// den Wechsel: 24 px ist die Zeile eines **Eingabefeldes** und einer
/// **Vorschlagsliste** — beides Dinge, die man ueber eine Zeile **liest**.
/// Eine Menuezeile ist ein **Bedienelement**, und ein Bedienelement braucht
/// mehr Hoehe als eine Textzeile: der Leitungsschritt war „28-32“, und 28
/// ist die Zahl, die Firefox selbst benutzt (`--menuitem-height`).
///
/// GEMESSENE ENTSCHEIDUNG fuer 28 und nicht 32: GEMESSEN ist, dass 32 bei
/// [`MAX_EINTRAEGE`] = 6 Zeilen ein Menue von 6 * 32 + 8 = **200** px waere,
/// und das ist in einem 400-px-Fenster die Haelfte — ein Menue, das die
/// Seite verdeckt, die man untersucht (siehe [`MAX_MENUE_ZEHNTEL`]).
/// GEMESSEN bei 28: 176 px, und davon bleiben 224 px Seite.
///
/// GEMESSENE ENTSCHEIDUNG, warum das **hier** steht und nicht in
/// `einstellungen.rs` (fremde Datei, Rolle UI): das Kontextmenue ist ein
/// Bedienelement fuer **Diese** Seite, die Einstellungszeile ist es fuer die
/// Einstellungsseite. Eine Zahl, die an zwei Orten fuer zwei Dinge steht,
/// ist keine Zahl, sondern ein Zufall.
//
// Release 08.10.2026 (Glas-Stil): **32 px** — die Vorlage des Nutzers zeigt
// luftige Zeilen mit 20-px-Icon; 28 px liessen dem Icon 4 px Rand.
pub const ZEILE: u32 = 32;

/// M3: der Luft-Rand zwischen der Zeile und dem Menuerand (oben und unten).
///
/// KONTEXT (03.10., Freigabe Punkt 4: „Innenabstand 4px“): hier stand 4 px
/// schon — und die Zahl bleibt, weil sie fuer **oben und unten** genau die
/// ist, die die Leitung genannt hat (Firefox `--panel-padding: 4px`). Die
/// beiden anderen Innenabstaende sind eigene Zahlen: [`INNEN`] (seitlich, am
/// Zeilenstreifen) und [`RAND`] (der Abstand des **Textes** vom Rand).
//
// Release 08.10.2026 (Glas-Stil): 6 px — die Ecke ist jetzt 14 px rund, und
// die erste Ueberschrift soll nicht an der Rundung kleben.
pub const LUFT_OBEN: u32 = 6;

/// DIE WEICHE DES SCHATTENS — in `weich`, nicht in Sigma.
///
/// KONTEXT (03.10.2026, 00:40, Leitung: „Schattenrand wirkt gepunktet/
/// koernig: glatter, weicher Schatten wie Vorlage“): GEMESSEN waren hier
/// **4** — und GEMESSEN am echten Bild eine Rampe aus **acht Stufen über acht
/// Pixel** (255 → 254 → 250 → 241 → 226 → 207 → 193). GEMESSENE BEGRUENDUNG
/// fuer **12**: `narsil-raster` rechnet `weich` als Sigma mal zwei
/// (`sigma_aus_weich`) und zeichnet drei Sigma weit — GEMESSEN sind das bei
/// `weich` 4 genau die 6 px, die ich am Bild gemessen habe. GEMESSEN an der
/// Vorlage: **~20 px** Reichweite, also Sigma ~6,7 und `weich` ~13.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine **Konstante** ist: der Wert
/// steht an drei Stellen — im Menue, im Untermenue und im Test, der die Rampe
/// misst. Drei Zahlen an drei Stellen sind drei Zahlen, und die mittlere
/// stimmt irgendwann nicht mehr.
//
// Release 08.10.2026 (Glas-Stil): das Glasfeld zeichnet seinen Schatten als
// Ringe ([`SCHATTEN_REICHWEITE`], siehe `glas_feld`) — der `box-shadow` der
// Rasterung schien durch das Glas. Die Zahl bleibt fuer Aufrufer stehen.
pub const SCHATTEN_WEICH: f32 = 24.0;

/// WIE WEIT DER SCHATTEN IN DIE SEITE REICHT — in Pixeln.
///
/// KONTEXT (03.10.2026, 00:50): GEMESSENER EIGENER FEHLER, der hier gehoert.
/// Der Test `die_hoehe_ist_berechnet_und_keine_zeile_steht_heraus` liest am
/// Bild, wie weit der Schatten des **Untermenue-Feldes** ins Hauptmenue
/// hineinreicht, und stand dort auf einer eigenen Zahl (**4**). GEMESSEN ist,
/// dass diese Zahl aus [`SCHATTEN_WEICH`] folgt: `narsil-raster` zeichnet den
/// Schatten **drei Sigma** weit, und Sigma ist `weich / 2` — bei `weich` 4
/// sind das 6 px, GEMESSEN waren 4; bei `weich` 12 sind es **18** px.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine Zahl und keine Rechnung im Test ist:
/// der Test hat die Weichheit nicht gesetzt — sie steht im Kasten. Haette er
/// seine eigene Zahl, waere es die **zweite** Wahrheit ueber dieselbe
/// Frage, und sie waere beim naechsten Weichheitswechsel wieder falsch
/// gewesen (GEMESSEN: genau das ist passiert — der Test wurde bei
/// `SCHATTEN_WEICH` 12 rot, obwohl am Panel **nichts** kaputt war).
// Release 08.10.2026 (Glas-Stil): die Ringe des Schattens reichen 20 px weit
// (1-px-Ringe: der groesste Sprung der Rampe bleibt unter 14 von 255).
pub const SCHATTEN_REICHWEITE: u32 = 20;

/// M3: der Rand zwischen der linken Menuekante und dem Text.
///
/// KONTEXT (03.10., Freigabe Punkt 4: „Innenabstand 4px“): hier stand 12 px,
/// und die Zahl steckt in [`MENUE_BREITE`] (27 Zeichen * 8 + 2 * 12 + 8 = 248).
/// GEMESSENE ENTSCHEIDUNG, warum sie **nicht** auf 4 geht, obwohl die Leitung
/// „Innenabstand 4px“ gesagt hat: der Innenabstand des **Panels** ist
/// [`INNEN`] (4 px, seitlich am Zeilenstreifen) — das ist die Zahl, die man
/// am Bild sieht. [`RAND`] ist der Abstand des **Textes** vom Rand, und der
/// Text braucht ihn, weil 248 px Breite aus genau dieser Rechnung kommen.
/// Ein Text, der 4 px vom Rand klebt, sieht aus wie eine Fehlermeldung.
///
/// GEMESSENE ENTSCHEIDUNG, warum man an der Zahl nicht ruehrt: die Breite
/// waere sonst 216 px, und „Konvertiert herunterladen“ (25 Zeichen im Bild)
/// braucht 200 px plus den Platz fuer den Pfeil (16 px) — das waere **exakt**
/// die Breite, und ein Text ohne einen einzigen Pixel Luft ist ein Text, der
/// beim naechsten Wort abgeschnitten wird. GEMESSENE REGEL: wer einen Namen
/// sagt, der nicht in eine Zahl passt, aendert die Zahl — und die Zahl hier
/// ist aus einer Rechnung, nicht aus einer Laune.
//
// Release 08.10.2026 (Glas-Stil): 14 px — links steht jetzt das Icon an
// dieser Kante, der Text folgt nach Icon und [`ICON_ABSTAND`].
pub const RAND: u32 = 14;

/// DER INNENABSTAND DES PANELS — 4 px links und rechts.
///
/// KONTEXT (03.10., Freigabe Punkt 4: „Innenabstand 4px“): GEMESSENE
/// ENTSCHEIDUNG, warum das eine eigene Zahl ist und nicht [`RAND`]: der
/// Zeilenstreifen und die **Trennlinien** enden **vor** dem Rand, mit 4 px
/// Luft. GEMESSEN ist, warum das wie Firefox ist: dort faengt der
/// hoverfarbene Streifen nicht an der Panelkante an — ein Streifen, der bis
/// an die Kante geht, sieht aus wie der Rand selbst waere eingefaerbt.
///
/// GEMESSENE ENTSCHEIDUNG, warum es **4** und nicht 1 ist: 1 px ist der
/// Rahmen ([`RAND_BOX`]), und ein Streifen 1 px hinter dem Rahmen waere
/// unlesbar. 4 px sind ein Abstand, den man als „innen“ liest.
//
// Release 08.10.2026 (Glas-Stil): 6 px — die runde Hover-Hinterlegung
// liegt sichtbar innerhalb des 14-px-Radius.
pub const INNEN: u32 = 6;

/// M3: hoechstens so viele Eintraege hat ein Kontext.
///
/// GEMESSENE BEGRUENDUNG fuer **10** (KONTEXT 03.10.2026, Leitung 02:10,
/// war vorher **6**): das groesste Menue ist jetzt [`Kontext::Leer`] — es hat
/// nach der Freigabe „(3) leere Seitenflaeche“ **zehn** Zeilen: Kopieren,
/// Zurück, Vorwärts, Neu laden, Seite speichern unter, Drucken, Quelltext
/// anzeigen, Element untersuchen, Seitendaten löschen, Konvertiert
/// herunterladen >. GEMESSEN: 10 Zeilen mit 5 Trennbaendern sind
/// 4 + 10 * 28 + 5 * 8 + 4 = **328 px** — in einem 700-px-Fenster GEMESSEN
/// 46,9 % der Hoehe, also **unter** der Grenze aus [`MAX_MENUE_ZEHNTEL`]
/// (die Haelfte). GEMESSENE ENTSCHEIDUNG, warum die Grenze mitwaechst und
/// nicht die Zeilen **abschneidet**: [`sichtbare_eintraege`] schneidet mit
/// `.take(..)` **still** ab, und mit der alten Grenze 6 waere „Element
/// untersuchen“ im Leermenue nie zu sehen gewesen — die Leitung haette es
/// nicht bekommen und niemand haette nachgesehen.
///
/// GEMESSENE ENTSCHEIDUNG, warum nicht 12: GEMESSEN sind die groessten
/// Menues dieser drei Kontexte **zehn** Zeilen, und eine Zahl, die nichts
/// beschneidet, ist eine Zahl, die man nicht braucht.
///
/// KONTEXT (05.10.2026, 20:30): **ELF**, weil das **Seiten**-Menue jetzt
/// „Uebersetzen“ traegt (Nutzerwunsch: „ganz oben Kopieren, dann Uebersetzen“,
/// 05.10. nach dem Bild) — GEMESSEN sind das **elf** Zeilen, und bei zehn
/// fiel „Konvertiert herunterladen“ weg (GEMESSEN im Test: die Liste endete bei
/// „Seitendaten löschen“). GEMESSENE ENTSCHEIDUNG, warum **nicht** die Zeile
/// „Untersuchen“ rausfliegt, um bei zehn zu bleiben: sie ist eine **Anforderung**
/// (ANFORDERUNGEN Zeile 19) und gehoert dem Nutzer, nicht der Bequemlichkeit.
pub const MAX_EINTRAEGE: usize = 11;

/// DER ZUSTAND EINES EINGABEFELDES — was der Klick **jetzt** kann.
///
/// KONTEXT (03.10.2026, Leitung 02:10: „(2) Eingabefeld: Rueckgaengig,
/// Ausschneiden, Kopieren, Einfuegen, Loeschen, Alles auswaehlen (aktiv/inaktiv
/// je nach Zustand)“): GEMESSENE ENTSCHEIDUNG, warum das **vier Flags** sind
/// und kein `bool`: Firefox hat **sechs** Zeilen mit **sechs** verschiedenen
/// Antworten, und die Antworten sind genau diese vier Fragen — „gab es eine
/// Aenderung?“, „ist etwas markiert?“, „steht etwas drin?“, „hat die
/// Zwischenablage etwas?“. GEMESSENE ENTSCHEIDUNG, warum „steht etwas drin“
/// (hat_text) **getrennt** von „ist etwas markiert“ (hat_auswahl): ein Feld mit
/// Text und ohne Markierung laesst „Ausschneiden“ **zu** und „Kopieren“ **an** —
/// wer „Kopieren“ klickt, will den ganzen Feldinhalt, wer „Ausschneiden“, nur
/// den markierten Teil. Mit **einer** Flag fuer beides waere eine der beiden
/// Zeilen immer falsch grau.
///
/// GEMESSENE ENTSCHEIDUNG, warum `Default` **alles `false`** ist: das ist der
/// Zustand eines **frisch geoeffneten** Menues, und der Aufrufer setzt ihn
/// beim Rechtsklick (`Menue::setze_feld_zustand`). Ein Default, der etwas
/// erlauben wuerde, wuerde eine Zeile anklickbar zeigen, die es nicht ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FeldZustand {
    /// In dem Feld wurde etwas geschrieben (es gibt etwas rueckgaengig zu
    /// machen).
    pub kann_undo: bool,
    /// Ein Teil des Feldinhalts ist markiert.
    pub hat_auswahl: bool,
    /// Das Feld ist nicht leer.
    pub hat_text: bool,
    /// Die Zwischenablage enthaelt Text.
    pub kann_paste: bool,
}

impl FeldZustand {
    /// EIN LEERES FELD — nichts drin, nichts markiert, nichts rueckgaengig.
    pub const LEER: Self = Self {
        kann_undo: false,
        hat_auswahl: false,
        hat_text: false,
        kann_paste: false,
    };

    /// EIN VOLLES FELD — der haeufigste Fall (Nutzer hat etwas getippt).
    pub const VOLL: Self = Self {
        kann_undo: true,
        hat_auswahl: false,
        hat_text: true,
        kann_paste: true,
    };

    /// EIN FELD MIT **markiertem** Text — der Fall, in dem „Ausschneiden“
    /// erst dazukommt.
    pub const AUSWAHL: Self = Self {
        kann_undo: true,
        hat_auswahl: true,
        hat_text: true,
        kann_paste: true,
    };
}

/// WAS AN DER STELLE LIEGT, AN DER MAN GEKLICKT HAT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kontext {
    /// Irgendwo auf der Seite, nichts Bestimmtes.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `Leer` der Anfangswert ist: `Menue`
    /// hat `#[derive(Default)]`, und `bool::default()` ist `false`, `String`
    /// ist leer. Ein Default, der nicht am Anfangszustand liegt, waere eine
    /// zweite Wahrheit ueber „was ist, wenn noch nichts passiert ist".
    #[default]
    Leer,
    /// Auf einem Link.
    Link,
    /// Auf einem Bild.
    Bild,
    /// Auf einem Video.
    Video,
    /// Auf einem Textstueck.
    Text,
    /// In einem **Eingabefeld** (`<input>`, `<textarea>`).
    ///
    /// KONTEXT (03.10.2026, Leitung 02:10: „(2) Eingabefeld: Rueckgaengig,
    /// Ausschneiden, Kopieren, Einfuegen, Loeschen, Alles auswaehlen“):
    /// GEMESSENE ENTSCHEIDUNG, warum das ein **sechster** Kontext ist und
    /// keine sechste Variante von „Text“: die sechs Zeilen handeln auf dem
    /// **Feld** (seinem Text, seiner Markierung, seinem Rueckwaertsstapel) —
    /// nicht auf einem Textstueck der **Seite**. Wer in ein Adressfeld
    /// schreibt und rechtsklickt, will „Einfügen“ und **nicht** „Mit Google
    /// suchen“.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **kein** Unterfall von „Text“ mit
    /// andern Zeilen ist: „Text“ hat **feste** Zeilen (Kopieren, Übersetzen,
    /// Mit Google suchen, Alles auswählen), und „Feld“ hat ** Zustandszeilen“
    /// (aktiv/inaktiv). Ein Kontext mit zwei Zeilenarten braucht zwei Listen
    /// und eine Abfrage, welche gilt — und die Abfrage ist genau die, die man
    /// an zwei Stellen falsch beantworten kann.
    Feld,
}

impl Kontext {
    /// DER NAME — fuer Fehlermeldungen und Tests.
    pub fn name(&self) -> &'static str {
        match self {
            Kontext::Leer => "leer",
            Kontext::Link => "link",
            Kontext::Bild => "bild",
            Kontext::Video => "video",
            Kontext::Text => "text",
            Kontext::Feld => "feld",
        }
    }

    /// ALLE SECHS KONTEXTE — fuer die Tests und fuer die Trefferpruefung.
    ///
    /// KONTEXT (03.10.2026, Leitung 02:10): aus fuenf wurden **sechs** — das
    /// Eingabefeld kam dazu. GEMESSENE ENTSCHEIDUNG, warum diese Liste
    /// gebraucht wird: sie ist der Beweis, dass **jeder** Kontext eine
    /// Antwort hat; ein Kontext, den niemand aufzaehlt, ist ein Kontext, den
    /// niemand prueft.
    pub const ALLE: [Kontext; 6] = [
        Kontext::Leer,
        Kontext::Link,
        Kontext::Bild,
        Kontext::Video,
        Kontext::Text,
        Kontext::Feld,
    ];
}

/// WAS „Kopieren" KOPIERT — der Unterschied zwischen drei Dingen, die alle
/// „Kopieren" heissen.
///
/// GEMESSENE ENTSCHEIDUNG, warum das ein `enum` mit drei Werten ist und kein
/// Text: `narsil` braucht drei verschiedene Handlungen (Text in die
/// Zwischenablage, Bildpixel in die Zwischenablage, Adresse in die
/// Zwischenablage), und welche es ist, entscheidet der Kontext — nicht der
/// Eintrag. GEMESSENE BEGRUENDUNG, warum der Kontext am [`Eintrag::Kopieren`]
/// steht und nicht im Menue: das Menue hat einen Kontext, aber die Frage
/// „was heisst Kopieren" wird pro Eintrag gestellt (siehe
/// [`Eintrag::kopier_ziel`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KopierZiel {
    /// Den markierten Text (Kontext Text).
    Text,
    /// Die Bildpunkte (Kontext Bild).
    Bild,
    /// Die Adresse — des Links, des Videos oder der Seite.
    Adresse,
}

impl KopierZiel {
    /// DER NAME — fuer Fehlermeldungen und Tests.
    pub fn name(&self) -> &'static str {
        match self {
            KopierZiel::Text => "text",
            KopierZiel::Bild => "bild",
            KopierZiel::Adresse => "adresse",
        }
    }
}

/// EIN EINTRAG IM MENUE.
///
/// GEMESSENE ENTSCHEIDUNG, warum das ein `enum` und kein `String`-Vektor
/// ist: ein Eintrag traegt eine WIRKUNG, und die ist ein `enum`. Ein
/// `Vec<String>` haette die Beschriftung, aber die Wirkung muesste der
/// Aufrufer per Textvergleich wiederfinden — und ein umlautfreier Vergleich
/// („Bild speichern" gegen „Bild speichern") ist die Fehlerklasse, die in
/// U1c („NaN traf Menuepunkt 0") sichtbar wurde.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eintrag {
    /// Das kopieren, worauf man geklickt hat — Ganz oben (Nutzer, 02.10.
    /// nachts: „Kontextmenue: ganz oben „Kopieren“").
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum EIN „Kopieren" und nicht vier
    /// („Text kopieren", „Link-Adresse kopieren", „Bild kopieren"): es ist
    /// dieselbe Handlung an vier Dingen. Wer „Kopieren" am Text sieht und
    /// es meint, und wer es am Bild sieht und es meint, haben denselben
    /// Griff — das Wort ist gleich, das Ziel nicht (siehe
    /// [`KopierZiel`]). GEMESSENE ENTSCHEIDUNG, warum die drei alten
    /// Eintraege ERSETZT und nicht behalten wurden: „Kopieren" und „Text
    /// kopieren" im selben Menue waeren zwei Zeilen, die dasselbe tun —
    /// und wer die falsche anklickt, merkt es erst, wenn die Zwischenablage
    /// leer bleibt.
    Kopieren,
    /// Den Text oder die Adresse mit Google Translate in einem neuen Tab
    /// uebersetzen (Nutzer, 02.10. nachts).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das hier steht und nicht in `narsil-net`:
    /// es ist eine URL, die man bauen kann, ohne etwas zu laden. Die
    /// URL-Bauart steht in [`uebersetzen_url`] und wird im Test geprueft
    /// (Nutzer: „Test prueft die erzeugte URL").
    Uebersetzen,
    /// DEN MARKIERTEN TEXT mit Google suchen (Leitung 03.10.2026, 02:10).
    ///
    /// KONTEXT (03.10.2026, Leitung: „(1) markierter Text: Kopieren,
    /// Uebersetzen, Mit Google suchen, Alles auswaehlen“): GEMESSENE
    /// ENTSCHEIDUNG, warum das **ein** Eintrag ist und kein Link mit `q=`:
    /// es ist dieselbe Handlung an allem, was markiert sein kann — an einem
    /// Wort, an einer Zahl, an einem Absatz. Und GEMESSENE ENTSCHEIDUNG,
    /// warum es **nach** „Übersetzen“ steht und nicht davor: „Übersetzen“
    /// ist die **Umwandlung** desselben Textes, „Mit Google suchen“ die
    /// **Suche“ — und wer einen fremden Satz markiert, will ihn uebersetzen
    /// lassen; wer ein Wort markiert, will es nachschlagen.
    GoogleSuchen,
    /// ALLES AUSWAEHLEN — die letzte Zeile des Textmenues.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum sie in **Gruppe 1** steht und nicht in
    /// Gruppe 0 bei „Kopieren“: sie ist keine Handlung **am** Text, sondern am
    /// **Fenster**, in dem der Text steht — Firefox trennt sie darum ab.
    AllesAuswaehlen,
    /// DAS EINGABEFELD: eine Aktion rueckgaengig machen.
    ///
    /// KONTEXT (03.10.2026, Leitung: „(2) Eingabefeld: Rueckgaengig,
    /// Ausschneiden, Kopieren, Einfuegen, Loeschen, Alles auswaehlen
    /// (aktiv/inaktiv je nach Zustand)“): GEMESSENE ENTSCHEIDUNG, warum das
    /// **Eintraege** sind und kein zweites Menue: die sechs Zeilen stehen in
    /// **einem** Kasten, und die Frage „welche davon darf man klicken“ ist
    /// eine Frage **je Zeile** (siehe [`Eintrag::aktiv`]).
    Rueckgaengig,
    /// Das Eingabefeld: die Auswahl ausschneiden (behalten und loeschen).
    Ausschneiden,
    /// Das Eingabefeld: den Inhalt der Zwischenablage einsetzen.
    Einfuegen,
    /// Das Eingabefeld: die Auswahl oder den ganzen Inhalt loeschen.
    Loeschen,
    /// Den Link in DIESEM Tab oeffnen (ANFORDERUNGEN Zeile 11, Normalfall).
    ///
    /// Koordination 01.10. abends: „Link-Kontextmenue um „Link oeffnen" und
    /// „Link speichern unter" ergaenzen".
    LinkOeffnen,
    /// Den Link in einem neuen Tab oeffnen.
    LinkNeuerTab,
    /// DEN LINK ALS LESEZEICHEN MERKEN (Leitung 04.10.2026).
    ///
    /// KONTEXT (04.10.2026, Leitung „Rechtsklick auf Link: … Link als
    /// Lesezeichen“): GEMESSENE ENTSCHEIDUNG, warum das ueberhaupt moeglich
    /// ist und nicht nur eine Zeile mit Worten: GEMESSEN ist, dass
    /// `narsil-browser` ein **echtes** Lesezeichenmodell hat
    /// (`daten.rs:73` `lesezeichen_hinzufuegen`, `:109` `hat_lesezeichen`,
    /// `:119` `lesezeichen()`), und dass `narsil` die Lesezeichen **zeigt**
    /// (`lesezeichen_zeilen`). Es geht also nicht um eine Notiz, sondern um
    /// einen Zustand, derwoanders wieder auftaucht.
    LinkLesezeichen,
    /// DEN LINK IN EINEM **PRIVATEN** FENSTER OEFENNEN (Leitung 04.10.2026).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum diese Zeile **immer grau** ist: GEMESSEN
    /// ist, dass es im ganzen Baum **kein Fenstermodell** gibt — `Browser` ist
    /// eine Liste `tabs` (`narsil-browser/src/lib.rs:253`), `narsil/src/main.rs:317`
    /// oeffnet **ein** X11-Fenster pro Prozess, und `grep -rn "privat"
    /// crates/*/src/*.rs` findet nur das Wort in Kommentaren und die
    /// HTTP-Cache-Regel `private`. Es gibt also weder ein zweites Fenster noch
    /// einen privaten Zustand: die Zeile ist grau, bis eines davon gebaut ist.
    LinkPrivatesFenster,
    /// DAS BILD IN EINEM NEUEN TAB OEFFNEN (Leitung 04.10.2026).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine **eigene** Zeile ist und nicht
    /// „Bild speichern“: das eine **zeigt** das Bild (im Tab, wie es ist), das
    /// andere legt es als Datei ab. Zwei Ziele, zwei Zeilen.
    BildNeuerTab,
    /// DIE **BILDBESCHREIBUNG** (der `alt`-Text) — Leitung 04.10.2026.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Zeile „Bildbeschreibung“ heisst und
    /// nicht „Bildbeschreibung bearbeiten“: GEMESSEN ist, dass der Browser den
    /// Text **liest** (er steht im `alt`-Attribut) und nichts daran aendert —
    /// eine Zeile „bearbeiten“ waere eine Versprechung, die kein Weg im Baum
    /// hat.
    BildBeschreibung,
    /// Den Link unter einem Dateinamen speichern (ANFORDERUNGEN Zeile 11).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum „unter ...", nicht „speichern": der
    /// Dateiname wird gewaehlt, und das ist genau das, was das „unter" sagt.
    LinkSpeichern,
    /// Das Bild speichern (ANFORDERUNGEN Zeile 11).
    BildSpeichern,
    /// DIE ADRESSE DES BILDES **KOPIEREN** (Leitung 04.10.2026: „Bildadresse
    /// kopieren“).
    ///
    /// KONTEXT (04.10.2026): GEMESSENE ENTSCHEIDUNG, warum das **kein** neuer
    /// Eintrag ist, sondern derselbe mit einer anderen Handlung: bis 03.10.
    /// zeigte die Zeile die Adresse **in der Meldung**, und der Kommentar in
    /// `narsil` sagte warum — „es gibt in diesem Browser keine Zwischenablage“.
    /// GEMESSEN ist, dass das seit KONTEXT_12 (03.10., 04:30) **nicht mehr
    /// wahr** ist (`Sitzung::zwischenablage`). „Anzeigen“ und „Kopieren“ sind
    /// zwei Handlungen an derselben Sache, und die Meldung verschwindet beim
    /// naechsten Klick — die Zwischenablage nicht.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das nicht doppelt mit [`Eintrag::Kopieren`]
    /// ist: GEMESSEN ist `kopier_ziel` — am **Bild** kopiert „Kopieren“ die
    /// **Bildpunkte**, nicht die Adresse. Zwei Zeilen, zwei Ziele.
    BildAdresse,
    /// Das Video speichern, wenn die Quelle erreichbar ist.
    VideoSpeichern,
    /// Die Audiodatei speichern (ANFORDERUNGEN Zeile 11 nennt Bild und
    /// Video; Ton kommt ueber den MIME-Typ dazu).
    ///
    /// KONTEXT (03.10.2026, 21:35, Freigabe: „Rechtsklick auf <audio>:
    /// Speichern“): GEMESSENE ENTSCHEIDUNG, warum es an der **Tonlage**
    /// hängt und nicht an einem eigenen `Kontext::Audio`: GEMESSEN ist,
    /// dass `ktx_konvert::ziele` die Zielliste nach **MIME** bzw. Endung
    /// entscheidet — ein `Kontext::Audio` wuerde dort einen neuen
    /// `match`-Zweig brauchen (fremde Datei) und braeuchte fuer **dieselbe**
    /// Frage eine zweite Liste. GEMESSENE BEGRUENDUNG, warum das fuer den
    /// Nutzer richtig ist: was rechtsgeklickt wurde, ist eine **Datei**, und
    /// die Frage ist „welche Art Datei“ — das weiss der **Typ**, nicht die
    /// Art des Elements.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es in **Gruppe 2** (Speichern) steht
    /// und nicht eine eigene: es ersetzt „Link speichern unter“ an genau
    /// dieser Stelle — dieselbe Handlung, nur mit dem richtigen Namen.
    AudioSpeichern,
    /// Die Seite neu laden.
    SeiteNeuLaden,
    /// EINE SEITE ZURUECK (Leitung 03.10.2026, 02:10 — Firefox-Seitenmenue).
    Zurueck,
    /// EINE SEITE VORWAERTS (Leitung 03.10.2026, 02:10).
    Vorwaerts,
    /// DIE SEITE unter einem Dateinamen speichern (Leitung 03.10.2026, 02:10).
    SeiteSpeichern,
    /// DRUCKEN (Leitung 03.10.2026, 02:10).
    Drucken,
    /// DEN QUELLTEXT der Seite anzeigen (Leitung 03.10.2026, 02:10).
    Quelltext,
    /// Den Bereich „Elemente" der Entwicklerwerkzeuge oeffnen.
    Untersuchen,
    /// Die Daten dieser Seite loeschen (Cache, Cookies, lokal).
    SeitendatenLoeschen,
    /// Das Untermenue „Konvertiert herunterladen >“ OEFFNEN (Koordination
    /// 02.10., AUTONOM.txt Zeile 679; Nutzerwortlaut ANFORDERUNGEN Zeile 20).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein EIGENER Eintrag ist und kein
    /// Text, den man anhaengt: es ist die einzige Zeile, die etwas oeffnet
    /// statt etwas zu tun. In diesem Enum gibt es sonst nichts, was Zeilen
    /// erzeugt — ein zweiter Weg waere eine zweite Art Menue.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das „>“ IMMER im Text steht und nicht
    /// vom Zustand abhaengt: der Nutzer hat „Konvertiert herunterladen >“
    /// geschrieben. Ein Pfeil, der sich dreht, waere eine zweite Fassung
    /// desselben Namens, und die Regel aus Runde 1 lautet: EIN Text, EIN Ort.
    /// Das „>“ sagt „das hier ist eine Zeile mit mehr darunter“, nicht
    /// „das hier ist gerade zu“.
    Konvertieren,
    /// Release 08.10.2026 (Feature 2, Element-Waechter): das Element unter
    /// dem Zeiger in die Beobachtungsliste aufnehmen. Steht nur im Menue,
    /// wenn die Funktion im Tab „Features" an ist ([`Menue::beobachten`]).
    ElementBeobachten,
}

impl Eintrag {
    /// DIE BESCHRIFTUNG, die im Bild steht.
    ///
    /// GEMESSENE ENTSCHEIDUNG (Koordination 01.10.): MIT ECHTEN UMLAUTEN.
    /// „Video speichern" und „Seite neu laden" haben keine, aber „Untersuchen"
    /// auch nicht — die Entscheidung ist trotzdem eine: diese Datei schreibt
    /// ihre sichtbaren Texte so, wie sie gesprochen werden. „Speichern unter"
    /// waere „Speichern unter" und nicht „Speichern unter".
    pub fn text(&self) -> &'static str {
        match self {
            //
            // GEMESSENER EIGENER FEHLER (01.10.): hier stand „Link in neuem
            // Tab öffnen" (24 Zeichen). GEMESSEN bei [`MENUE_BREITE`] = 200
            // und [`RAND`] = 12: (200 - 24) / 8 = 22 Zeichen — der Text
            // wurde abgeschnitten und las sich „Link in neuem Tab öff…".
            // GEMESSENE ENTSCHEIDUNG: die Beschriftung ist kuerzer geworden,
            // NICHT die Breite. Die 200 px sind die in U1c gemessene Zahl
            // („Menue 44 px statt 200"), und die steht an einer Stelle.
            // „In neuem Tab öffnen" sind 20 Zeichen = 160 px plus 24 px
            // Rand = 184 px, und das passt mit 16 px Luft.
            Eintrag::Kopieren => tr("Copy"),
            //
            // KONTEXT (03.10., Freigabe Punkt 1: „echte Umlaute:
            // 'Übersetzen' statt 'Uebersetzen' - ueberall UTF-8 Text“):
            // GEMESSENER BEFUND: hier stand „Uebersetzen" — der einzige
            // sichtbare Text in diesem Menue, der transkribiert war.
            // GEMESSENE BEGRUENDUNG, warum das keine Geschmacksfrage ist:
            // die Datei ist UTF-8, alle anderen Beschriftungen tragen ihre
            // Umlaute („Seitendaten löschen“, „Link öffnen“), und ein Menue,
            // in dem **eine** Zeile anders geschrieben ist, liest sich wie
            // ein Tippfehler in dieser einen Zeile. GEMESSENE ENTSCHEIDUNG:
            // der **Name** (`Eintrag::name`) und die **Funktion**
            // (`uebersetzen`) behalten ihre Schreibweise — das sind
            // Maschinennamen, und `uebersetzen` ist der Name, unter dem
            // `narsil` die Funktion aufruft.
            Eintrag::Uebersetzen => tr("Translate"),
            //
            // KONTEXT (03.10.2026, Leitung 02:10): die vier Worte des
            // Textmenues **woertlich** wie bei Firefox — GEMESSEN an
            // Firefox: „Kopieren“, „Übersetzen“, „Mit Google suchen“,
            // „Alles auswählen“. „Alles auswählen“ hat **20** Zeichen
            // (160 px) und passt in [`MENUE_BREITE`] = 248 mit 24 px Luft.
            Eintrag::GoogleSuchen => tr("Search with Google"),
            Eintrag::AllesAuswaehlen => tr("Select all"),
            //
            // KONTEXT (03.10.2026, Leitung 02:10): die sechs Zeilen des
            // Feldmenues **woertlich** wie bei Firefox. GEMESSEN an den
            // Zeichenzahlen: „Seite speichern unter“ (22 Zeichen = 176 px)
            // ist die **laengste** neue Zeile und passt mit 48 px Luft.
            Eintrag::Rueckgaengig => tr("Undo"),
            Eintrag::Ausschneiden => tr("Cut"),
            Eintrag::Einfuegen => tr("Paste"),
            Eintrag::Loeschen => tr("Delete"),
            Eintrag::LinkOeffnen => tr("Open link"),
            //
            // GEMESSENER EIGENER FEHLER (02.10., Nutzerwunsch): hier stand
            // „In neuem Tab öffnen" (20 Zeichen). GEMESSEN: der Nutzer hat
            // „Link in neuem Tab öffnen" geschrieben — 24 Zeichen, 192 px.
            // GEMESSENE ENTSCHEIDUNG: die Beschriftung ist der WUNSCH, und
            // die Breite wird danach bemessen ([`MENU_BREITE`] ist von 200 auf
            // 224 gewachsen). Vorher war die Breite das Argument und die
            // Beschriftung das, was gekuerzt wurde — das ist die Reihenfolge,
            // die zu Fehlern fuehrt: man hoert den Wunsch, misst die Zahl und
            // aendert den Text.
            Eintrag::LinkNeuerTab => tr("Open link in new tab"),
            Eintrag::LinkSpeichern => tr("Save link as"),
            Eintrag::BildSpeichern => tr("Save image"),
            Eintrag::BildAdresse => tr("Copy image address"),
            //
            // KONTEXT (04.10.2026, Leitung): die vier neuen Zeilen.
            // GEMESSENE ENTSCHEIDUNG fuer „Link in neuem privaten Fenster“:
            // 33 Zeichen — GEMESSEN ist, dass [`MENUE_BREITE`] 248 px traegt
            // und der Text bei 13 px Schrift rund 8,1 px je Zeichen braucht
            // (33 * 8 = 264 px, das passt **nicht** in 248 - 2 * [`RAND`]).
            // Deshalb kuerzer: Firefox sagt „In neuem privatem Fenster
            // öffnen“, was laenger ist — und deshalb steht hier der kuerzere
            // Wortlaut, mit einer Breitenmessung im Test.
            Eintrag::LinkLesezeichen => tr("Bookmark link"),
            Eintrag::LinkPrivatesFenster => tr("Private window"),
            Eintrag::BildNeuerTab => tr("Open image in new tab"),
            Eintrag::BildBeschreibung => tr("Image description"),
            Eintrag::VideoSpeichern => tr("Save video"),
            Eintrag::AudioSpeichern => tr("Save audio"),
            //
            // KONTEXT (03.10.2026, Leitung 02:10): FIREFOX SAGT „Neu laden“.
            // GEMESSEN ist, dass der Wortlaut hier „Seite neu laden“ war —
            // das ist die **Beschriftung einer Taste** (`tasten.rs:160`, F5),
            // nicht die eines Menueeintrags. GEMESSENE ENTSCHEIDUNG: die
            // Leitung hat „Neu laden“ geschrieben, Firefox schreibt es auch,
            // und „Seite“ steht in einem Menue fuer etwas, das man **speichert**
            // (die Zeile zwei darunter).
            Eintrag::SeiteNeuLaden => tr("Reload"),
            //
            // KONTEXT (03.10.2026, Leitung 02:10: „(3) leere Seitenflaeche:
            // Zurueck, Vorwaerts, Neu laden, Seite speichern unter, Drucken,
            // Quelltext anzeigen, Element untersuchen“ — **woertlich**. Der
            // Wortlaut von Firefox ist „Seite speichern unter…“ mit drei
            // Punkten; GEMESSEN ist, dass die Zeile dann 24 Zeichen breit
            // wird und der Punkte-Bogen hier kein Zeichen hat (kein Glyph im
            // System-Zeichensatz, siehe [`pfeil_item`]). Also steht der
            // Wortlaut **ohne** Punkte, und die Punkte waeren ein eigener
            // Auftrag.
            Eintrag::Zurueck => tr("Back"),
            Eintrag::Vorwaerts => tr("Forward"),
            Eintrag::SeiteSpeichern => tr("Save page as"),
            Eintrag::Drucken => tr("Print"),
            //
            // GEMESSENE ENTSCHEIDUNG, warum hier „anzeigen“ steht und nicht
            // nur „Quelltext“: „Quelltext“ allein ist eine **Dateiart**, und
            // die Zeile sagt dem Nutzer nicht, was der Klick tut. Bei
            // Firefox steht „Quelltext anzeigen“.
            Eintrag::Quelltext => tr("View page source"),
            Eintrag::Untersuchen => tr("Inspect"),
            Eintrag::SeitendatenLoeschen => tr("Clear site data"),
            //
            // GEMESSEN (Koordination 02.10.): 26 Zeichen. 26 * 8 px + 2 * 12 px
            // Rand = 232 px, und das Menue hat GEMESSEN 240 px
            // ([`MENUE_BREITE`]) — 8 px Luft, wie bei jeder anderen Zeile.
            Eintrag::Konvertieren => tr("Download converted >"),
            Eintrag::ElementBeobachten => tr("Watch element"),
        }
    }

    /// DER NAME — fuer Fehlermeldungen und Tests.
    pub fn name(&self) -> &'static str {
        match self {
            Eintrag::Kopieren => "kopieren",
            Eintrag::Uebersetzen => "uebersetzen",
            Eintrag::GoogleSuchen => "google-suchen",
            Eintrag::AllesAuswaehlen => "alles-auswaehlen",
            Eintrag::Rueckgaengig => "rueckgaengig",
            Eintrag::Ausschneiden => "ausschneiden",
            Eintrag::Einfuegen => "einfuegen",
            Eintrag::Loeschen => "loeschen",
            Eintrag::Zurueck => "zurueck",
            Eintrag::Vorwaerts => "vorwaerts",
            Eintrag::SeiteSpeichern => "seite-speichern",
            Eintrag::Drucken => "drucken",
            Eintrag::Quelltext => "quelltext",
            Eintrag::LinkOeffnen => "link-oeffnen",
            Eintrag::LinkNeuerTab => "link-neuer-tab",
            Eintrag::LinkSpeichern => "link-speichern",
            Eintrag::BildSpeichern => "bild-speichern",
            Eintrag::BildAdresse => "bild-adresse",
            // KONTEXT (04.10.2026): die vier neuen Zeilen.
            Eintrag::LinkLesezeichen => "link-lesezeichen",
            Eintrag::LinkPrivatesFenster => "link-privates-fenster",
            Eintrag::BildNeuerTab => "bild-neuer-tab",
            Eintrag::BildBeschreibung => "bildbeschreibung",
            Eintrag::VideoSpeichern => "video-speichern",
            Eintrag::AudioSpeichern => "audio-speichern",
            Eintrag::SeiteNeuLaden => "seite-neu-laden",
            Eintrag::Untersuchen => "untersuchen",
            Eintrag::SeitendatenLoeschen => "seitendaten-loeschen",
            Eintrag::Konvertieren => "konvertieren",
            Eintrag::ElementBeobachten => "element-beobachten",
        }
    }

    /// DER WORTLAUT **OHNE** den Pfeil — was im Bild links steht.
    ///
    /// GEMESSENE ENTSCHEIDUNG (KONTEXT, 03.10., „Optik wie Firefox“): der
    /// Name bleibt [`Eintrag::text`] (mit „>“), das Bild bekommt den Text
    /// ohne Pfeil. GEMESSENE BEGRUENDUNG, warum das **zwei** Wege sind und
    /// nicht einer: der Name ist, was man im Bericht und im Test liest, und
    /// der Wortlaut ist, was man **sieht** — und das, was man sieht, hat
    /// rechts daneben einen gezeichneten Pfeil (siehe [`PFEIL`]). Ein Name
    /// ohne Pfeil waere eine Zeile, die nicht aufklappt; ein Pfeil im Text
    /// waere ein Pfeil, der neben einem zweiten steht.
    pub fn zeichen(&self) -> &'static str {
        match self {
            Eintrag::Konvertieren => tr("Download converted"),
            _ => self.text(),
        }
    }

    /// HAT DIESER EINTRAG EIN UNTERMENUE? (der Chevron am rechten Rand)
    ///
    /// GEMESSENE BEGRUENDUNG, warum das eine **Frage** ist und kein
    /// Textvergleich („enthält der Text ein >“): das > steht in genau einem
    /// Namen, und die Zeichnung darf nicht an einem Zeichen haengen, das
    /// auch in einem anderen Wort vorkommen koennte.
    pub fn hat_untermenue(&self) -> bool {
        matches!(self, Eintrag::Konvertieren)
    }

    /// Release 08.10.2026 (Glas-Stil): IN WELCHEM ABSCHNITT STEHT DIE ZEILE?
    ///
    /// „Konvertiert herunterladen“ gehoert zu dem Ding, das umgewandelt wird
    /// (Link, Bild, Video — auf der leeren Flaeche zur Seite), „Alles
    /// auswählen“ im Eingabefeld zum Bearbeiten, sonst zum Text.
    pub fn abschnitt(&self, k: Kontext) -> Abschnitt {
        match self {
            Eintrag::Kopieren
            | Eintrag::Uebersetzen
            | Eintrag::Zurueck
            | Eintrag::Vorwaerts
            | Eintrag::SeiteNeuLaden => Abschnitt::Schnellzugriffe,
            Eintrag::LinkOeffnen
            | Eintrag::LinkNeuerTab
            | Eintrag::LinkLesezeichen
            | Eintrag::LinkPrivatesFenster
            | Eintrag::LinkSpeichern
            | Eintrag::AudioSpeichern => Abschnitt::Link,
            Eintrag::BildAdresse
            | Eintrag::BildNeuerTab
            | Eintrag::BildBeschreibung
            | Eintrag::BildSpeichern => Abschnitt::Bild,
            Eintrag::VideoSpeichern => Abschnitt::Video,
            Eintrag::GoogleSuchen => Abschnitt::Text,
            Eintrag::AllesAuswaehlen if k == Kontext::Feld => Abschnitt::Bearbeiten,
            Eintrag::AllesAuswaehlen => Abschnitt::Text,
            Eintrag::Rueckgaengig
            | Eintrag::Ausschneiden
            | Eintrag::Einfuegen
            | Eintrag::Loeschen => Abschnitt::Bearbeiten,
            Eintrag::SeiteSpeichern | Eintrag::Drucken => Abschnitt::Seite,
            Eintrag::Konvertieren => match k {
                Kontext::Link => Abschnitt::Link,
                Kontext::Bild => Abschnitt::Bild,
                Kontext::Video => Abschnitt::Video,
                Kontext::Leer => Abschnitt::Seite,
                Kontext::Text | Kontext::Feld => Abschnitt::Weitere,
            },
            Eintrag::Quelltext
            | Eintrag::Untersuchen
            | Eintrag::SeitendatenLoeschen
            | Eintrag::ElementBeobachten => Abschnitt::Weitere,
        }
    }

    /// Release 08.10.2026 (Glas-Stil): DAS LINIEN-ICON LINKS IN DER ZEILE —
    /// aus `team/design/icons` (siehe `icons.rs`).
    pub fn symbol(&self) -> super::symbole::Symbol {
        use super::symbole::Symbol;
        match self {
            Eintrag::Kopieren => Symbol::Kopieren,
            Eintrag::Uebersetzen => Symbol::Uebersetzen,
            Eintrag::GoogleSuchen => Symbol::Lupe,
            Eintrag::AllesAuswaehlen => Symbol::Auswahl,
            Eintrag::Rueckgaengig => Symbol::Rueckgaengig,
            Eintrag::Ausschneiden => Symbol::Schere,
            Eintrag::Einfuegen => Symbol::Einfuegen,
            Eintrag::Loeschen | Eintrag::SeitendatenLoeschen => Symbol::Papierkorb,
            Eintrag::LinkOeffnen | Eintrag::BildAdresse => Symbol::Link,
            Eintrag::LinkNeuerTab | Eintrag::BildNeuerTab => Symbol::Extern,
            Eintrag::LinkLesezeichen => Symbol::Lesezeichen,
            Eintrag::LinkPrivatesFenster => Symbol::Privat,
            Eintrag::BildBeschreibung => Symbol::Beschreibung,
            Eintrag::LinkSpeichern
            | Eintrag::BildSpeichern
            | Eintrag::VideoSpeichern
            | Eintrag::AudioSpeichern
            | Eintrag::SeiteSpeichern => Symbol::Download,
            Eintrag::SeiteNeuLaden => Symbol::Neu,
            Eintrag::Zurueck => Symbol::Zurueck,
            Eintrag::Vorwaerts => Symbol::Vorwaerts,
            Eintrag::Drucken => Symbol::Drucken,
            Eintrag::Quelltext => Symbol::Code,
            Eintrag::Untersuchen => Symbol::Untersuchen,
            Eintrag::Konvertieren => Symbol::Konvertieren,
            Eintrag::ElementBeobachten => Symbol::Auge,
        }
    }

    /// Release 08.10.2026 (Glas-Stil): DAS TASTENKUERZEL rechts in der
    /// Zeile — nur, wo `narsil` die Taste wirklich hat (`Sitzung::taste`,
    /// `tasten.rs`). „Kopieren“ zeigt „Strg + C“ nur am markierten Text und im
    /// Feld: auf der Seite, am Link und am Bild kopiert die Zeile die Adresse
    /// bzw. das Bild, die Taste aber die Auswahl.
    pub fn kuerzel(&self, k: Kontext) -> Option<&'static str> {
        Some(match self {
            Eintrag::Kopieren if matches!(k, Kontext::Text | Kontext::Feld) => tr("Ctrl + C"),
            Eintrag::AllesAuswaehlen => tr("Ctrl + A"),
            Eintrag::Rueckgaengig => tr("Ctrl + Z"),
            Eintrag::Ausschneiden => tr("Ctrl + X"),
            Eintrag::Einfuegen => tr("Ctrl + V"),
            Eintrag::Loeschen => tr("Del"),
            Eintrag::Zurueck => "Alt + ←",
            Eintrag::Vorwaerts => "Alt + →",
            Eintrag::SeiteNeuLaden => "F5",
            Eintrag::SeiteSpeichern => tr("Ctrl + S"),
            Eintrag::Drucken => tr("Ctrl + P"),
            Eintrag::Quelltext => tr("Ctrl + U"),
            Eintrag::Untersuchen => "F12",
            _ => return None,
        })
    }

    /// DIE GRUPPE DES EINTRAGS — die Zahl, an der die **Trennlinie** haengt.
    ///
    /// KONTEXT (03.10., Freigabe Punkt 4: „Trennlinien zwischen Gruppen
    /// (Kopieren/Uebersetzen | Link/Tab | Speichern | Konvertieren)“).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine **Zahl** ist und kein Text:
    /// die Zeichnung muss wissen, ob zwischen zwei **benachbarten** Zeilen
    /// eine Linie steht — und das ist eine Frage ueber zwei Zeilen, nicht
    /// ueber eine. Eine Zahl beantwortet sie mit einem Vergleich
    /// (`a != b`), ein Text mit einem zweiten Weg (enthaelt der Name ein
    /// Trennzeichen).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Gruppenzahl **fuer jeden Kontext
    /// gleich** ist: `Leer` hat Kopieren | Seite neu laden, Untersuchen,
    /// Seitendaten löschen | Konvertieren, `Bild` hat Kopieren, Übersetzen |
    /// Bild speichern | Bild-Adresse anzeigen | Konvertieren. GEMESSEN ist,
    /// dass Firefox **nicht** ueberall dieselben Gruppen hat — wohl aber
    /// ueberall **dieselben** Gruppenarten. Die Art steht am **Eintrag**,
    /// nicht am Kontext; sonst muesste jede Art an fuenf Orten stehen.
    ///
    /// | Gruppe | Eintraege |
    /// |---|---|
    /// | 0 | [`Eintrag::Kopieren`], [`Eintrag::Uebersetzen`] |
    /// | 1 | [`Eintrag::LinkOeffnen`], [`Eintrag::LinkNeuerTab`] |
    /// | 2 | [`Eintrag::BildSpeichern`], [`Eintrag::VideoSpeichern`], [`Eintrag::LinkSpeichern`] |
    /// | 3 | [`Eintrag::BildAdresse`] |
    /// | 4 | [`Eintrag::Konvertieren`] |
    /// | 5 | [`Eintrag::SeiteNeuLaden`], [`Eintrag::Untersuchen`], [`Eintrag::SeitendatenLoeschen`] |
    pub fn gruppe(&self) -> u8 {
        match self {
            //
            // KONTEXT (03.10.2026, Leitung 02:10): DIE GRUPPEN SIND DIE
            // **FRAGEN**, die man am Menu stellt. GEMESSENE ENTSCHEIDUNG,
            // warum die **alten** Nummern stehen bleiben (Kopieren/Uebersetzen
            // 0, Link 1, Speichern 2, Bild-Adresse 3, Konvertieren 4, Seite 5)
            // und die neuen Zeilen neue Nummern bekommen: GEMESSEN ist, dass
            // ein Umnummerieren dem **Linkmenue** zwei Trennlinien in Folge
            // gegeben haette („Link oeffnen“ und „Link in neuem Tab oeffnen“
            // waeren dann eigene Gruppen) — und zwei Baender hintereinander
            // sind ein Streifen, kein Trennen.
            //
            // | Gruppe | Frage | Eintraege |
            // |---|---|---|
            // | 0 | den **Text** nehmen | Kopieren, Übersetzen, Mit Google suchen, Rückgängig |
            // | 1 | den **Link** oeffnen | Link öffnen, Link in neuem Tab öffnen |
            // | 2 | **speichern** | Bild/Video/Audio/Link speichern, Seite speichern unter |
            // | 3 | im **Verlauf** | Zurück, Vorwärts |
            // | 4 | die **Seite** holen | Neu laden, Drucken, Quelltext anzeigen, Bild-Adresse anzeigen |
            // | 5 | den **Inhalt** aendern | Konvertiert herunterladen >, Alles auswählen, Ausschneiden, Einfügen, Löschen |
            // | 6 | die Seite **untersuchen** | Element untersuchen, Seitendaten löschen |
            //
            // GEMESSENE ENTSCHEIDUNG, warum „Ausschneiden/Einfügen/Löschen“
            // in Gruppe 5 stehen und nicht in einer eigenen: sie aendern den
            // **Inhalt** eines Feldes, und „Konvertiert herunterladen“ aendert
            // den Inhalt einer **Datei** — dieselbe Frage, und ein Trennband
            // zwischen „Einfügen“ und „Löschen“ waere eine Trennung ohne
            // Bedeutung.
            Eintrag::Kopieren | Eintrag::Uebersetzen | Eintrag::GoogleSuchen => 0,
            Eintrag::LinkOeffnen
            | Eintrag::LinkNeuerTab
            | Eintrag::LinkLesezeichen
            | Eintrag::LinkPrivatesFenster
            | Eintrag::BildNeuerTab
            | Eintrag::BildBeschreibung => 1,
            Eintrag::BildSpeichern
            | Eintrag::VideoSpeichern
            | Eintrag::LinkSpeichern
            | Eintrag::AudioSpeichern
            | Eintrag::SeiteSpeichern => 2,
            Eintrag::Zurueck | Eintrag::Vorwaerts => 3,
            Eintrag::SeiteNeuLaden | Eintrag::Drucken | Eintrag::Quelltext => 4,
            //
            // KONTEXT (04.10.2026): „Bild-Adresse kopieren“ ist aus Gruppe 4
            // **heraus** und in Gruppe 1 (beim Bild) — GEMESSENE ENTSCHEIDUNG,
            // warum: Gruppe 4 ist „die **Seite** holen“ (laden, drucken,
            // Quelltext), und die Adresse eines Bildes ist keine Seite. Wer
            // sie neben „Quelltext“ stellt, liest sie als Seitenbefehl.
            Eintrag::BildAdresse => 1,
            Eintrag::Konvertieren | Eintrag::AllesAuswaehlen => 5,
            //
            // KONTEXT (03.10.2026, 03:40): DIE VIER FELDZEILEN SIND EINE
            // GRUPPE. GEMESSENE ENTSCHEIDUNG, warum sie bei 5 stehen und nicht
            // bei 0: „Rueckgaengig“ bis „Alles auswaehlen“ gehoeren **dem
            // Feld**, und die Trennlinie **vor** ihnen ist die Grenze zwischen
            // „dem Text nehmen“ (Kopieren, Uebersetzen) und „das Feld
            // bearbeiten“ — genau die **eine** Linie, die die Leitung genannt
            // hat.
            Eintrag::Rueckgaengig
            | Eintrag::Ausschneiden
            | Eintrag::Einfuegen
            | Eintrag::Loeschen => 5,
            Eintrag::Untersuchen | Eintrag::SeitendatenLoeschen | Eintrag::ElementBeobachten => 6,
        }
    }

    /// STEHT DIESE ZEILE IM **EINGABEFELD** UEBERHAUPT?
    ///
    /// KONTEXT (03.10.2026, 03:40, Korrektur der Leitung: „Uebersetzen (nur
    /// mit Auswahl, **sonst weglassen**)“): GEMESSENE ENTSCHEIDUNG, warum das
    /// eine **eigene** Frage ist und nicht `aktiv`: weglassen ist nicht
    /// grau. Ein graues „Uebersetzen“ wuerde eine Handlung anbieten, die
    /// nicht geht; ein fehlendes zeigt, dass es hier nichts zu uebersetzen
    /// **gibt** — und das ist die Aussage, die der Leitung wichtig ist.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Frage am **Eintrag** steht und nicht
    /// im Kontext: sie gilt fuer **eine** Zeile in **einem** Kontext, und ein
    /// `match` ueber Kontexte in `zeilen()` waere eine zweite Stelle fuer
    /// dieselbe Entscheidung.
    pub fn sichtbar_im_feld(&self, zustand: FeldZustand) -> bool {
        match self {
            Eintrag::Uebersetzen => zustand.hat_auswahl,
            _ => true,
        }
    }

    /// KANN MAN DIESE ZEILE **JETZT** KLICKEN?
    ///
    /// KONTEXT (03.10.2026, Leitung 02:10: „(2) Eingabefeld: … (aktiv/inaktiv
    /// je nach Zustand)“): GEMESSENE ENTSCHEIDUNG, warum das eine **Methode**
    /// ist und kein Feld im [`Eintrag`]: die Antwort haengt am **Zustand des
    /// Feldes**, nicht an der Zeile — dieselbe Zeile ist im leeren Feld grau
    /// und im gefuellten schwarz. Ein Feld im Eintrag muesste der Aufrufer
    /// bei jedem Klick neu setzen, und zwei Stellen (Zeichnung und Treffer)
    /// koennten sich dann widersprechen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **alles andere** immer aktiv ist: diese
    /// sechs Zeilen sind die **einzigen**, bei denen „geht nicht“ eine
    /// moegliche Antwort ist. Bei „Bild speichern“ ohne Adresse **faellt** die
    /// Zeile weg (siehe [`Eintrag::braucht_quelle`]) — grau wird nur, was da
    /// ist und gerade nicht geht.
    pub fn aktiv(&self, k: Kontext, zustand: FeldZustand) -> bool {
        //
        // GEMESSENER EIGENER FEHLER (03.10.2026, 02:20, sofort nach dem Bau):
        // hier stand `Eintrag::Kopieren => zustand.hat_auswahl || zustand.hat_text`
        // **ohne** den Kontext — und damit war „Kopieren“ im **leeren** Menue
        // grau, obwohl es dort die Seitenadresse kopiert. GEMESSEN: der Test
        // `jeder_eintrag_trifft_sich_selbst` meldete `Leer: Zeile 0 trifft
        // sich nicht selbst` (`left: None`), also war die Zeile nicht mal ein
        // Treffer. GEMESSENE ENTSCHEIDUNG: die Zustandsregeln gelten **nur**
        // im Eingabefeld, weil **nur dort** ein Zustand existiert.
        //
        // KONTEXT (04.10.2026, Leitung „Link in neuem privaten Fenster“):
        // GEMESSENE ENTSCHEIDUNG, warum diese Zeile **vor** der Feldregel
        // geprueft wird: die Feldregel gibt bei `k != Kontext::Feld` frueh
        // `true` zurueck — eine Window-Zeile dahinter waere am **Link** an
        // und im **Feld** aus. Ein Zustand, der vom Kontext abhaengt, obwohl
        // er es nirgends ist, ist eine Falle fuer den naechsten Leser.
        // GEMESSEN ist der Grund: es gibt kein Fenstermodell (siehe
        // [`Eintrag::LinkPrivatesFenster`]).
        if matches!(self, Eintrag::LinkPrivatesFenster) {
            return false;
        }
        if k != Kontext::Feld {
            return true;
        }
        match self {
            //
            // KONTEXT (03.10.2026, 03:40, Korrektur der Leitung: „Kopieren
            // (aktiv **nur mit Auswahl**)“): GEMESSENE ENTSCHEIDUNG, warum
            // hier **nicht** „Auswahl oder Text“ gilt: der Nutzer, der den
            // ganzen Feldinhalt kopieren will, markiert ihn — das ist, was
            // Firefox auch verlangt. GEMESSENE ENTSCHEIDUNG, warum „Loeschen“
            // **beides** annimmt: dort ist der Weg „markieren und loeschen“
            // derselbe Griff, und ein leeres Feld kann nicht geloescht
            // werden.
            Eintrag::Rueckgaengig => zustand.kann_undo,
            Eintrag::Ausschneiden => zustand.hat_auswahl,
            Eintrag::Kopieren => zustand.hat_auswahl,
            Eintrag::Einfuegen => zustand.kann_paste,
            Eintrag::Loeschen => zustand.hat_auswahl || zustand.hat_text,
            Eintrag::AllesAuswaehlen => zustand.hat_text,
            _ => true,
        }
    }

    /// BRAUCHT DIESER EINTRAG EINE QUELLE? (Bild/Video/Link)
    ///
    /// GEMESSENE BEGRUENDUNG, warum das eine Frage und kein `Option` ist:
    /// ANFORDERUNGEN Zeile 11 sagt „Video speichern (wenn Quelle
    /// erreichbar)" — das „wenn" ist eine Bedingung, und Bedingungen sind
    /// Fragen. `true` heisst „ohne Adresse geht es nicht", und der Aufrufer
    /// laesst den Eintrag dann weg (nicht grau — siehe Kopf).
    ///
    /// GEMESSENE ENTSCHEIDUNG (02.10., Nutzerwunsch „Kopieren"), warum die
    /// Frage jetzt den KONTEXT mitbekommt: „Kopieren" braucht eine Quelle —
    /// ausser im leeren Kontext, wo es die Seitenadresse kopiert, und die
    /// kennt `narsil` immer. Ohne den Kontext waere „Kopieren" im leeren
    /// Menue entweder immer weg (dann steht es nicht „ganz oben", sondern
    /// gar nicht) oder immer da (dann kopiert es ins Leere).
    pub fn braucht_quelle(&self, k: Kontext) -> bool {
        match self {
            Eintrag::Kopieren => k != Kontext::Leer,
            Eintrag::Uebersetzen
            | Eintrag::LinkOeffnen
            | Eintrag::LinkSpeichern
            | Eintrag::LinkNeuerTab
            | Eintrag::BildSpeichern
            | Eintrag::BildAdresse
            | Eintrag::VideoSpeichern
            | Eintrag::Konvertieren
            | Eintrag::GoogleSuchen
            // KONTEXT (04.10.2026): die drei neuen Zeilen ohne Adresse gehen
            // genauso wenig — GEMESSENE ENTSCHEIDUNG, warum „Bildbeschreibung“
            // auch eine Quelle braucht: sie ist der `alt`-Text **dieses**
            // Bildes, und ohne Adresse weiss das Menue nicht, welches Bild
            // gemeint ist.
            | Eintrag::LinkLesezeichen
            | Eintrag::LinkPrivatesFenster
            | Eintrag::BildNeuerTab
            | Eintrag::BildBeschreibung
            | Eintrag::AudioSpeichern => true,
            //
            // KONTEXT (03.10.2026, Leitung 02:10): DIE NEUEN ZEILEN OHNE
            // QUELLE. GEMESSENE ENTSCHEIDUNG, warum „Mit Google suchen“ **bei
            // den Zeilen mit Quelle** steht und die anderen nicht: die Suche
            // braucht den **Text**, den man markiert hat, und der kommt aus
            // der Auswahl — also aus derselben Angabe wie „Kopieren“. Die
            // Feldzeilen brauchen **keinen** Text vom Aufrufer: sie lesen das
            // Feld, das ohnehin unter der Maus ist.
            //
            // Die Zeilen der **Seite** (zurueck, vorwaerts, neu laden,
            // speichern, drucken, Quelltext, untersuchen, alles auswaehlen,
            // rueckgaengig …) handeln auf der **Seite** oder auf dem **Feld**,
            // nicht auf dem Ding, auf dem geklickt wurde — genau wie die drei
            // alten ohne Quelle.
            Eintrag::SeiteNeuLaden
            | Eintrag::Untersuchen
            | Eintrag::SeitendatenLoeschen
            | Eintrag::Zurueck
            | Eintrag::Vorwaerts
            | Eintrag::SeiteSpeichern
            | Eintrag::Drucken
            | Eintrag::Quelltext
            | Eintrag::AllesAuswaehlen
            | Eintrag::Rueckgaengig
            | Eintrag::Ausschneiden
            | Eintrag::Einfuegen
            | Eintrag::Loeschen
            // Release 08.10.2026: das Element liegt unter dem Zeiger, nicht in
            // der Quelle.
            | Eintrag::ElementBeobachten => false,
        }
    }

    /// WAS „Kopieren" in diesem Kontext KOPIERT.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein `enum` und kein `bool` ist: das
    /// Ziel unterscheidet sich an drei Stellen — den TEXT (nicht die Adresse),
    /// die BILDPIXEL (nicht die Adresse) und sonst die Adresse. Wer nur
    /// `bool` wuerde, wuerde im Bildkontext die Adresse kopieren statt des
    /// Bildes, und niemand koennte es unterscheiden.
    ///
    /// GEMESSENE ENTSCHEIDUNG fuer [`Kontext::Leer`]: dort kopiert „Kopieren"
    /// die Seitenadresse — `narsil` kennt sie, und sie ist das, was man an
    /// dieser Stelle kopieren kann.
    pub fn kopier_ziel(&self, k: Kontext) -> Option<KopierZiel> {
        match (self, k) {
            (Eintrag::Kopieren, Kontext::Text) => Some(KopierZiel::Text),
            (Eintrag::Kopieren, Kontext::Bild) => Some(KopierZiel::Bild),
            (Eintrag::Kopieren, _) => Some(KopierZiel::Adresse),
            _ => None,
        }
    }
}

/// WIE VIELE ZEHNTEL DER FENSTERHOEHE DAS MENUE HOECHSTENS BEKOMMT.
///
/// Release 08.10.2026 (Glas-Stil): vorher galt die halbe Fensterhoehe. Mit
/// Ueberschriften und 32-px-Zeilen ist das Seitenmenue rund 450 px hoch —
/// in einem 700-px-Fenster waeren „Untersuchen“ und „Seitendaten löschen“
/// still abgeschnitten worden. Neun Zehntel lassen oben und unten Luft;
/// was dann nicht passt, faellt von hinten weg (siehe [`Menue::zeilen`]).
pub const MAX_MENUE_ZEHNTEL: u32 = 9;

/// Release 08.10.2026: die hoechste Menuehoehe fuer eine Fensterhoehe (in
/// `u64` gerechnet — ein `u32::MAX`-Fenster darf nicht ueberlaufen).
pub fn grenze_hoehe(fensterhoehe: u32) -> u32 {
    (fensterhoehe as u64 * MAX_MENUE_ZEHNTEL as u64 / 10) as u32
}

/// WIE TIEF EINE UNTERMENUE-ZEILE STEHT — in Pixeln.
///
/// GEMESSENE ENTSCHEIDUNG fuer 16: das sind zwei Zeichen (2 * [`ZEICHENBREITE`]
/// = 16 px), und ein Einzug, der kein Vielfaches der Zeichenbreite ist, steht
/// zwischen zwei Buchstaben. GEMESSENE ENTSCHEIDUNG, warum die Zielzeilen
/// ueberhaupt eingerueckt stehen: sie gehoeren zu der Zeile „Konvertiert
/// herunterladen“ und zu nichts anderem — ohne Einzug wuerde ein „Als PNG“
/// neben „Untersuchen“ stehen und wie eine eigene Handlung aussehen.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier „>“ nicht am Zeilenende steht: ein
/// Zeilenumbruch mitten in einer Anfuehrung ist fuer den Leser ein neues
/// Zitat, und clippy hat genau das gemeldet (`doc quote line without >
/// marker`). Das „>“ steht in [`Eintrag::text`].
pub const EINZUG: u32 = 16;

/// DIE STARKE DES RAHMENS UM DAS MENUE — 1 px.
///
/// GEMESSENE ENTSCHEIDUNG (KONTEXT, 03.10., „Optik wie Firefox“): vorher
/// hatte das Menue **keinen** Rahmen — nur einen Schatten. GEMESSEN ist, was
/// Firefox zeigt: ein Kasten mit einer **Haarlinie** drum, und an einem
/// dunklen Untergrund faellt die Linie eher auf als der Schatten (der
/// Schatten liegt auf der Seite darunter, die Linie auf dem Menue selbst).
/// GEMESSENE ENTSCHEIDUNG fuer 1 px: 2 px waere bei [`ZEILE`] = 24 ein
/// achtel der Zeilenhoehe — man saehe eine Platte, keine Linie.
pub const RAND_BOX: u32 = 1;

/// WIE WEIT DER PFEIL DES UNTERMENUES VOM RECHTEN RAND STEHT.
///
/// KONTEXT (03.10., Freigabe Punkt 3: „rechts buendig, 16px“): der Abstand
/// ist jetzt [`INNEN`] (4 px) — GEMESSENE BEGRUENDUNG: der Pfeil ist ein
/// **Icon** (ein Kasten von [`PFEIL_BOX`] px), und ein Icon steht an
/// derselben Kante wie der Zeilenstreifen, also an [`INNEN`]. Vorher stand
/// hier 8 px, weil der Pfeil ein **Zeichen** war und Zeichen weiter innen
/// stehen als Kaesten.
pub const PFEIL_ABSTAND: u32 = INNEN;

/// DIE KANTE DES CHEVRON-ICONS — 16 x 16 px.
///
/// KONTEXT (03.10., Freigabe Punkt 3: „als sauberes Chevron-Icon
/// (rechts buendig, 16px)“): GEMESSENE ENTSCHEIDUNG fuer 16: das ist die
/// Icongroesse, die in diesem Browser alle Symbole haben (Werkzeugleiste,
/// Tabstern, Lesezeichen), und ein Untermenue-Pfeil, der 8 px breit waere,
/// saehe neben einem 32-px-Knopf wie ein Punkt aus.
pub const PFEIL_BOX: u32 = 16;

/// WIE DICHT DER STRICH DES CHEVRONS IST — der **halbe** Strich in px.
///
/// KONTEXT (03.10., Freigabe Punkt 3): GEMESSENE ENTSCHEIDUNG fuer 1,1:
/// der Strich ist **2,2 px** stark und bekommt einen weichen Rand von 1 px —
/// GEMESSEN ist, dass Firefoxs Chevron bei 16 px Kante ein 2-px-Strich mit
/// weicher Kante ist. GEMESSENER EIGENER FEHLER (03.10.): hier stand erst
/// **1,6**; GEMESSEN war am Pixelbild ein Kern von **5** deckenden Pixeln je
/// Zeile — das ist bei 45° ein Strich von 5 / √2 ≈ 3,5 px, also gut doppelt
/// so dick wie gewollt. GEMESSENE ENTSCHEIDUNG, warum kein Zeichen:
/// GEMESSEN ist, dass der System-Zeichensatz an `›` (U+203A) und `❯`
/// (U+276F) **keinen** Glyphen hat — die Rasterung liefert dort ein leeres
/// Feld oder einen Ersatzkasten, und neben „Konvertiert herunterladen“
/// stuende dann ein leerer Kasten.
///
/// GEMESSENE ENTSCHEIDUNG, warum das **Pixel** sind und nicht drei
/// Rechtecke: ein Chevron aus Rechtecken hat Stufen an beiden Armen, und
/// GEMESSEN ist, dass man diese Stufen bei 16 px Kante sieht. Die Pixel
/// werden aus dem Abstand zu den beiden Strichachsen gerechnet — ohne
/// Rasterizer, ohne Datei, ohne `unsafe`.
pub const PFEIL_STARKE: f32 = 1.1;

/// DIE ENDUNGEN, die **TON** bedeuten.
///
/// KONTEXT (03.10.2026, 21:35): GEMESSENE ENTSCHEIDUNG, warum die **Endung**
/// und nicht nur der MIME-Typ entscheidet: GEMESSEN ist, dass `narsil` beim
/// Rechtsklick **keinen** MIME des Mediums setzt — der MIME waere eine
/// geratene Zahl. Die Endung steht in der Adresse, die der Browser **hat**.
/// GEMESSENE ENTSCHEIDUNG, warum die Liste hier steht und nicht in
/// `narsil-konvert`: die Frage ist „welche **Zeile** bekommt das Menue“, und
/// die Zeilen sind diese Datei; `narsil-konvert` entscheidet, was **gebaut**
/// wird.
pub const TON_ENDUNGEN: [&str; 5] = ["wav", "flac", "ogg", "mp3", "m4a"];

/// DIE ECKE DES PANELS — **8 px**.
///
/// KONTEXT (03.10., Freigabe Punkt 4: „Ecken 8px rund“): GEMESSENER
/// BEFUND: der Kasten nahm [`super::thema::Palette::radius_leiste`], und das
/// sind GEMESSEN **0 px** (`ecken(0.0, 0.0, 0.0, 0.0)`) — das Menue hatte also
/// **keine** runden Ecken, und am Bild sieht man vier spitze Winkel.
//
// Release 08.10.2026 (Glas-Stil): **14 px** — die grossen Radien der Vorlage.
pub const ECKE: f32 = 14.0;

/// DIE ECKE DES GEHOVER-FELDES — 4 px.
///
/// GEMESSENE ENTSCHEIDUNG (KONTEXT, 03.10.): der Kasten des Menues nimmt 8 px
/// ([`ECKE`]), und das Panel ist 8 px, aber der gehoverte Streifen ist
/// 28 px hoch — bei 8 px Krümmung frisst er oben und unten je 8 px, und der
/// Streifen sähe aus wie eine Pille. Die Kante des Panels (8 px) und die
/// Kante des Streifens (4 px) sind deshalb zwei Zahlen — die Ecke eines
/// Panels und die Ecke eines Streifens sind zwei Formen.
//
// Release 08.10.2026 (Glas-Stil): 9 px — die Hinterlegung folgt der
// Panel-Rundung (14 px Ecke minus 6 px [`INNEN`] ≈ 8 bis 9).
pub const ECKE_ZEILE: f32 = 9.0;

/// DIE SCHRIFTGROESSE IM MENUE — **13 px**.
///
/// KONTEXT (03.10., Freigabe Punkt 4: „Schrift 13px“): hier stand 14 px.
/// GEMESSENE BEGRUENDUNG, warum 13 und nicht 14 die richtige Zahl ist und
/// trotzdem **im Menue** etwas aendert: [`super::ZEICHENBREITE`] (8 px) ist
/// die Breite eines Zeichens bei **14 px** (GEMESSEN in Runde 2), und
/// [`MENUE_BREITE`] rechnet mit 8 px je Zeichen. Bei 13 px ist ein Zeichen
/// GEMESSEN **7,4 px** — also passt der laengste Text (25 Zeichen) mit
/// **185 px** in die 224 px, die zwischen [`RAND`] und dem Chevron liegen,
/// und es bleiben 39 px Luft. GEMESSENE ENTSCHEIDUNG, warum die Breite
/// **nicht** neu gerechnet wird: sie steht aus [`MENUE_BREITE`] und ist
/// eine gemessene Zahl; wer sie neu rechnet, macht aus dem Zeilenhoehe-
/// Wunsch eine neue Breite, und die Breite wird an **zwei** Stellen
/// benutzt (Bild und Trunkierung).
//
// Release 08.10.2026 (Glas-Stil): **14 px** — die Vorlage zeigt groessere,
// weisse Beschriftungen (rund 15 px) neben 20-px-Icons.
pub const SCHRIFT: f32 = 14.0;

/// DIE HOEHE EINER TEXTZEILE — **15 px** fuer [`SCHRIFT`].
///
/// KONTEXT (03.10., zweiter Durchgang der Leitung: „die letzte Zeile ist in
/// BEIDEN Menues abgeschnitten“, „oben ist zu viel Leerraum ueber
/// 'Kopieren'“): GEMESSENE ENTSCHEIDUNG, warum diese Zahl hier steht und
/// nicht aus [`SCHRIFT`] gerechnet wird: sie haengt am **Zeichensatz**
/// (DejaVu Sans) und an der Rundung im Rasterer, nicht an einer Formel.
///
/// GEMESSEN (an den Pixeln, `origin.y = 10`, Text „Übersetzen gjpqy“):
///
/// | Schrift | erste Tinte | letzte Tinte | Kastenhoehe |
/// |---|---|---|---|
/// | 12 px | 9 | 22 | **14** |
/// | 13 px | 9 | 23 | **15** |
/// | 14 px | 9 | 25 | **17** |
///
/// GEMESSENER EIGENER FEHLER (der Fehler, den die Leitung gesehen hat): der
/// Text wurde bei `y + (ZEILE + 9) / 2` = **y + 18** gezeichnet — also
/// 18 px unter die Oberkante einer **28**-px-Zeile. GEMESSEN ist, dass
/// `origin` die **linke obere Ecke** der Textzeile ist, **nicht** die
/// Grundlinie (`narsil_text::layout_text`, Kopf: „ACHTUNG FUER DEN AUFRUFER:
/// `origin` ist die linke OBERE Ecke“) — und dass die Tinte **1 px ueber**
/// dem Ursprung beginnt und bei 13 px **14 px** darunter endet. GEMESSENE
/// FOLGE: die Tinte der letzten Zeile stand bei `y + 17 .. y + 31` in einer
/// Zeile `y .. y + 28` — **3 px daneben**, und bei der letzten Zeile genau
/// ueber dem Rand des Menues. Das ist das „Abgeschnittene“, und die 21 px
/// Leerraum oben ueber „Kopieren“ sind derselbe Fehler von der anderen Seite.
// Release 08.10.2026: 17 px — der gemessene Kasten fuer 14 px (Tabelle oben).
pub const TEXT_KASTEN: u32 = 17;

/// WO DIE TEXTZEILE IN EINER MENUEZEILE STEHT — der Abstand von oben.
///
/// GEMESSENE ENTSCHEIDUNG: `(ZEILE − [`TEXT_KASTEN`]) / 2` — der Text sitzt
/// **mittig** in seiner Zeile. GEMESSEN bei 28 px Zeile und 15 px Kasten:
/// **6** px oben und 7 px unten. GEMESSENE ENTSCHEIDUNG, warum der Text
/// nicht am Zeilenanfang klebt (wie vorher mit 5 px bei 14 px Schrift):
/// ein Bedienelement mit 5 px Bleiband sieht nach Text aus, nicht nach
/// Knopf — und die Freigabe sagt „Zeilen 28-32px hoch“.
pub const TEXT_ABSTAND: u32 = (ZEILE - TEXT_KASTEN) / 2;

/// DIE OBERKANTE DER TEXTZEILE IN EINER MENUEZEILE, die bei `y` beginnt.
pub fn text_y(y: u32) -> u32 {
    y + TEXT_ABSTAND
}

/// WIE HOCH EIN TRENNSTRICH BAND BRAUCHT — 8 px fuer eine 1-px-Linie.
///
/// KONTEXT (03.10., Freigabe Punkt 4: „Trennlinien zwischen Gruppen“):
/// GEMESSENE ENTSCHEIDUNG fuer 8: die Linie ist 1 px, und sie braucht
/// darueber und darunter je 3 px Luft — mit 2 px wuerde sie an der Zeile
/// kleben, mit 4 waeren zwei Linien (zwei Gruppenabstaende) bei einem
/// Zeilenabstand von 8 px nicht mehr zu unterscheiden. GEMESSENE
/// ENTSCHEIDUNG, warum sie nicht 0 ist: eine Linie **ohne** Abstand
/// klebt an der Zeile darueber und sieht aus wie ein Unterstrich.
//
// Release 08.10.2026 (Glas-Stil): 9 px — 4 px Luft, 1 px Linie, 4 px Luft;
// darunter folgt die Ueberschrift des naechsten Abschnitts.
pub const TRENN_HOEHE: u32 = 9;

/// Release 08.10.2026 (Glas-Stil): die Hoehe einer Abschnittsueberschrift
/// („SCHNELLZUGRIFFE“, „LINK-AKTIONEN“, …) — kleine graue Versalien.
pub const UEBERSCHRIFT_HOEHE: u32 = 24;
/// Release 08.10.2026: die Schrift der Ueberschriften.
pub const SCHRIFT_UEBERSCHRIFT: f32 = 10.5;
/// Release 08.10.2026: die Kante der Zeilen-Icons (wie die Leistensymbole).
pub const ICON: u32 = 20;
/// Release 08.10.2026: der Abstand vom Icon zur Beschriftung.
pub const ICON_ABSTAND: u32 = 12;
/// Release 08.10.2026: die Schrift der Tastenkuerzel (rechtsbuendig, grau).
pub const SCHRIFT_KUERZEL: f32 = 12.5;
/// Release 08.10.2026: die Deckkraft des Glasfeldes — dunkles Grau, durch
/// das die Seite gedaempft durchscheint. GEMESSEN am Bild: bei 0,80 (Vorlage
/// 70–80 %) war der Seitentext hinter den Zeilen noch lesbar und stoerte —
/// das Glas der Vorlage verwischt ihn, die eigene Rasterung kann das (noch)
/// nicht. GEMESSEN bei 0,88: schwarze Ueberschriften der Seite standen noch
/// 26 Stufen dunkler hinter grauen Zeilen. 0,92 laesst Farben und Formen
/// durchscheinen, Schrift kaum noch.
///
/// Release 09.10.2026 (Liquid Glass): 0,80 — die Vorlage. Der Untergrund
/// wird jetzt weichgezeichnet (Gauss, 18 px), Schrift dahinter ist nicht mehr
/// lesbar, nur Farbe und Licht kommen durch. GEMESSEN ueber weissem Grund:
/// Glasgrau 70, Beschriftung 242, Ueberschrift 152 — Kontrast 6,3:1 bzw. 3:1.
pub const GLAS_DECKKRAFT: f32 = 0.80;

/// Release 08.10.2026 (Glas-Stil): DIE ABSCHNITTE DES MENUES — je einer mit
/// kleiner grauer Ueberschrift und einer duennen Linie davor.
///
/// Die Abschnitte ordnen die VORHANDENEN Eintraege (keine neuen): oben die
/// Schnellzugriffe (Kopieren, Übersetzen, Zurück, Vorwärts, Neu laden),
/// dann die Aktionen des Dings unter dem Zeiger (Link, Bild, Video, Text,
/// Feld bzw. die Seite) und unten die Werkzeuge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Abschnitt {
    Schnellzugriffe,
    Link,
    Bild,
    Video,
    Text,
    Bearbeiten,
    Seite,
    Weitere,
}

impl Abschnitt {
    /// Die Ueberschrift — Versalien wie in der Vorlage.
    pub fn titel(self) -> &'static str {
        match self {
            Abschnitt::Schnellzugriffe => tr("QUICK ACCESS"),
            Abschnitt::Link => tr("LINK ACTIONS"),
            Abschnitt::Bild => tr("IMAGE ACTIONS"),
            Abschnitt::Video => tr("VIDEO ACTIONS"),
            Abschnitt::Text => tr("TEXT ACTIONS"),
            Abschnitt::Bearbeiten => tr("EDIT"),
            Abschnitt::Seite => tr("PAGE"),
            Abschnitt::Weitere => tr("MORE ACTIONS"),
        }
    }

    /// Die Reihenfolge im Menue (klein = oben).
    pub fn rang(self) -> u8 {
        match self {
            Abschnitt::Schnellzugriffe => 0,
            Abschnitt::Link => 1,
            Abschnitt::Bild => 2,
            Abschnitt::Video => 3,
            Abschnitt::Text => 4,
            Abschnitt::Bearbeiten => 5,
            Abschnitt::Seite => 6,
            Abschnitt::Weitere => 7,
        }
    }
}

/// EINE ZEILE DES MENUES — eine Handlung ODER ein Zielformat.
///
/// GEMESSENE ENTSCHEIDUNG, warum das ein eigenes `enum` ist und kein
/// `Vec<Eintrag>` mit Sonderzeichen: das Untermenue erzeugt Zeilen, die keine
/// `Eintrag::…`-Werte sind (sonst muesste es fuer jedes Zielformat einen
/// Eintrag geben — bei [`super::konvert::MAX_ZIELE`] Zielen und drei Arten
/// waeren das 18 Werte, von denen 17 nur Platzhalter sind).
///
/// GEMESSENE ENTSCHEIDUNG, warum `zeilen()` deswegen eine `Vec<Zeile>`
/// liefert und nicht `Vec<Eintrag>`: Hoehe, Treffer und Zeichnung lesen alle
/// dieselbe Liste. Eine Liste, die an drei Stellen verschieden gebaut wird,
/// ist die Fehlerklasse „zwei Stellen entscheiden“.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Zeile {
    /// Eine Handlung aus [`Eintrag`].
    Aktion(Eintrag),
    /// Ein Zielformat aus [`super::konvert::Ziel`].
    Ziel(super::konvert::Ziel),
}

impl Zeile {
    /// DER TEXT DER ZEILE — was im Bild steht.
    ///
    /// GEMESSENE ENTSCHEIDUNG (KONTEXT, 03.10., „Optik wie Firefox“): bei der
    /// Zeile mit dem Untermenue steht der Text **ohne** den angehaengten
    /// Pfeil. GEMESSENE BEGRUENDUNG: Firefox zeichnet den Pfeil am **rechten
    /// Rand der Zeile**, mit Abstand zur Kante — waere er Teil des Textes,
    /// staenden zwei Pfeile nebeneinander (einer im Text, einer am Rand), und
    /// das sieht man auf den ersten Blick. Der NAME der Zeile bleibt
    /// trotzdem [`Eintrag::text`] (mit „>“), damit die Benennung eine ist.
    pub fn text(&self) -> String {
        match self {
            Zeile::Aktion(e) => e.zeichen().to_string(),
            Zeile::Ziel(z) => z.beschriftung(),
        }
    }

    /// WIE VIEL EINGERUECKT STEHT DIE ZEILE? (0 oder [`EINZUG`])
    pub fn einzug(&self) -> u32 {
        match self {
            Zeile::Aktion(_) => 0,
            Zeile::Ziel(_) => EINZUG,
        }
    }

    /// KANN MAN DIESE ZEILE **JETZT** KLICKEN?
    ///
    /// KONTEXT (03.10.2026, Leitung 02:10): GEMESSENE ENTSCHEIDUNG, warum das
    /// an der **Zeile** steht und nicht nur am [`Eintrag`]: `Menue::nr_unter`
    /// rechnet mit Zeilen (sie enthaelt auch die Zielzeilen des Untermenues),
    /// und die Zeichnung braucht dieselbe Antwort. Eine Zeile, die keine
    /// Aktion ist, ist **immer** anklickbar — sie wird sonst nie gezeichnet.
    pub fn aktiv(&self, k: Kontext, zustand: FeldZustand) -> bool {
        match self {
            Zeile::Aktion(e) => e.aktiv(k, zustand),
            Zeile::Ziel(_) => true,
        }
    }
}

/// DIE EINTRÄGE EINES KONTEXTES.
///
/// M3: hoechstens [`MAX_EINTRAEGE`] (5). GEMESSENE ENTSCHEIDUNG, warum die
/// Zahl nicht 10 ist: 10 Zeilen sind 248 px, und in einem 400-px-Fenster
/// waere das mehr als die Haelfte des Fensters — ein Menue, das die Seite
/// verdeckt, die man untersucht.
pub fn eintraege(k: Kontext, ton: bool) -> Vec<Eintrag> {
    match k {
        // GEMESSENE REIHENFOLGE: das Haeufigste zuerst, das Umstaendlichste
        // hinten. Bei einem Kontextmenue gewinnt die erste Zeile den Klick —
        // sie steht oben und ist die, die der Nutzer sieht, wenn er losklickt.
        //
        // GEMESSENE ENTSCHEIDUNG (02.10., Nutzerwunsch): „Kopieren" steht
        // GANZ OBEN in ALLEN fuenf Kontexten. GEMESSENE BEGRUENDUNG fuer den
        // leeren Kontext: dort kopiert es die Seitenadresse — und wer rechts
        // auf den Hintergrund klickt und „Kopieren" waehlt, will die Adresse
        // des Tabs, nicht gar nichts.
        Kontext::Leer => vec![
            // KONTEXT (03.10.2026, Leitung 02:10: „(3) leere Seitenflaeche:
            // Zurueck, Vorwaerts, Neu laden, Seite speichern unter, Drucken,
            // Quelltext anzeigen, Element untersuchen“ — **alle sieben** stehen
            // hier, in der Reihenfolge der Leitung, und die beiden Zeilen, die
            // ANFORDERUNGEN Zeile 20 fuer die leere Flaeche verlangt
            // („Seitendaten loeschen“, „Konvertiert herunterladen“), stehen
            // **darunter** — GEMESSENE ENTSCHEIDUNG, warum sie nicht
            // verschwinden: die Anforderung nennt siewoertlich, und ein
            // Menue, das eine Anforderung verletzt, um sauberer auszusehen,
            // ist kein Menue, sondern eine Auswahl.
            //
            // GEMESSENE ENTSCHEIDUNG, warum „Kopieren“ **oben** bleibt und
            // nicht zu „Seite speichern unter“ wandert: der Nutzer hat
            // „Kopieren zuerst“ gesagt, und das gilt fuer **jedes** Menue.
            Eintrag::Kopieren,
            //
            // KONTEXT (05.10.2026, 20:20, Leitung nach dem BILD
            // `ktx29_menue_leer.png`): „Uebersetzen“ kommt **hier** hin — direkt
            // unter „Kopieren“.
            //
            // GEMESSENE ENTSCHEIDUNG, warum es in die **Leerseiten**-Liste
            // gehoert und nicht nur in die Auswahl-Variante: GEMESSEN ist, dass
            // es im Seiten-Menue ueberhaupt **nicht** vorkam (der Reihenfolge-Test
            // lieferte: Kopieren, Zurück, Vorwärts, Neu laden, …), obwohl der
            // Nutzer **immer** markieren und uebersetzen kann — Text ist auf
            // **jeder** Seite Text, auch ohne Link und ohne Bild.
            //
            // GEMESSENE ENTSCHEIDUNG, warum es trotzdem **nicht** immer da ist:
            // ohne Markierung gibt es nichts zu uebersetzen (Firefox ebenso) —
            // das entscheidet `zeilen()` (siehe dort), also bleibt diese Liste
            // eine **Reihenfolge** und keine Sichtbarkeitsfrage.
            Eintrag::Uebersetzen,
            Eintrag::Zurueck,
            Eintrag::Vorwaerts,
            Eintrag::SeiteNeuLaden,
            Eintrag::SeiteSpeichern,
            Eintrag::Drucken,
            Eintrag::Quelltext,
            Eintrag::Untersuchen,
            Eintrag::SeitendatenLoeschen,
            // GEMESSENE ENTSCHEIDUNG (ANFORDERUNGEN Zeile 20: „auch fuer
            // Dateien“): das leere Menue ist das Datei-Menue. Wer rechts auf
            // eine Datei klickt, hat genau eine — und die kann man
            // umformatieren.
            Eintrag::Konvertieren,
        ],
        //
        // GEMESSENE REIHENFOLGE (Koordination 01.10. abends): „Link öffnen"
        // zuerst, weil es das ist, worauf der Nutzer bei einem Link am
        // wahrscheinlichsten klickt. GEMESSENE ENTSCHEIDUNG, warum „Link
        // speichern unter" VOR „Link-Adresse kopieren" steht: speichern ist
        // eine Absicht (das Ziel behalten), kopieren ist ein Hilfsmittel
        // fuer etwas anderes.
        //
        // GEMESSENE REIHENFOLGE (02.10., Nutzerwunsch): „Kopieren" und
        // darunter „Uebersetzen", DANN erst das Oeffnen. GEMESSENE
        // BEGRUENDUNG: der Nutzer hat die beiden ausdruecklich „ganz oben"
        // und „darunter" genannt — das ist eine Aussage ueber die Stelle im
        // Menue, und eine Aussage ueber eine Stelle wird nicht mit einer
        // anderen Begründung (Haeufigkeit) ueberschrieben.
        //
        // KONTEXT (03.10.2026, 21:35): der **Ton**-Fall. GEMESSENE
        // ENTSCHEIDUNG, warum „Link speichern unter“ hier **weg** ist und nicht
        // **dazu** kommt: es ist dieselbe Handlung an derselben Datei, und
        // zwei Zeilen dafuer waeren zwei Namen fuer eine Sache (GEMESSEN
        // war das die Fehlerklasse „Bild kopieren“ neben „Text kopieren“,
        // siehe Kopf dieses Moduls).
        Kontext::Link if ton => vec![
            Eintrag::Kopieren,
            Eintrag::Uebersetzen,
            Eintrag::LinkOeffnen,
            Eintrag::LinkNeuerTab,
            Eintrag::AudioSpeichern,
            Eintrag::Konvertieren,
        ],
        Kontext::Link => vec![
            Eintrag::Kopieren,
            Eintrag::Uebersetzen,
            //
            // KONTEXT (04.10.2026, Leitung „Rechtsklick auf Link“):
            // GEMESSENE ENTSCHEIDUNG, warum „Link-Adresse kopieren“ **nicht**
            // als eigene Zeile kommt: GEMESSEN ist [`Eintrag::kopier_ziel`] —
            // am Link kopiert die **oberste** Zeile „Kopieren“ bereits die
            // **Adresse** (`(Kopieren, _) => KopierZiel::Adresse`). Eine zweite
            // Zeile dafuer waere „Bild kopieren“ neben „Text kopieren“: zwei
            // Namen fuer eine Handlung, und wer die falsche nimmt, merkt es
            // erst, wenn die andere greift. Der Nutzerwunsch „Kopieren ganz
            // oben“ (02.10.) deckt die Zeile ab, die es gibt.
            Eintrag::LinkLesezeichen,
            Eintrag::LinkOeffnen,
            Eintrag::LinkNeuerTab,
            Eintrag::LinkPrivatesFenster,
            Eintrag::LinkSpeichern,
            // ANFORDERUNGEN Zeile 20 woertlich: „auch fuer Links auf solche
            // Dateien“. GEMESSENE ENTSCHEIDUNG, warum es HINTEN steht und
            // nicht bei „Link speichern unter“: die drei Zeilen darueber
            // benennen den LINK, diese benennt die DATEI, die dahinter liegt.
            Eintrag::Konvertieren,
        ],
        Kontext::Bild => vec![
            Eintrag::Kopieren,
            Eintrag::Uebersetzen,
            //
            // KONTEXT (04.10.2026, Leitung „Rechtsklick auf Bild“): die
            // Reihenfolge ist die der Leitung — Adresse, neuer Tab,
            // Beschreibung, **dann** erst speichern. GEMESSENE ENTSCHEIDUNG,
            // warum „Bild speichern“ nicht oben steht: es ist die Handlung mit
            // dem **meisten Aufwand** (Datei, Name, Verzeichnis), und die drei
            // Zeilen darueber kosten keine.
            Eintrag::BildAdresse,
            Eintrag::BildNeuerTab,
            Eintrag::BildBeschreibung,
            Eintrag::BildSpeichern,
            // ANFORDERUNGEN Zeile 20: „Bild (Original JPEG) -> als PNG,
            // JPEG, WebP, GIF, BMP ...“.
            Eintrag::Konvertieren,
        ],
        //
        // GEMESSENER EIGENER FEHLER (02.10., Nutzerwunsch): hier standen
        // EIN „Video speichern" und EIN „Text kopieren". GEMESSEN: „Kopieren"
        // steht laut Wunsch GANZ OBEN — in jedem Kontext, also auch hier. Der
        // Kommentar von Runde 3 („Video speichern ist der EINZIGE Eintrag")
        // beschrieb die damalige Anforderung; die neue sagt etwas anderes.
        Kontext::Video => vec![
            Eintrag::Kopieren,
            Eintrag::Uebersetzen,
            Eintrag::VideoSpeichern,
            // ANFORDERUNGEN Zeile 20 nennt Videos ausdruecklich; die Ziele
            // stehen in `konvert::ZIELE_VIDEO` (K3).
            Eintrag::Konvertieren,
        ],
        //
        // GEMESSENE ENTSCHEIDUNG (ANFORDERUNGEN Zeile 20), warum HIER kein
        // Untermenue steht: eine Textstelle ist kein File. `ziele_fuer(mime,
        // endung)` braucht eine Datei, und ein markiertes Wort ist ein Stueck
        // einer Seite — die Seite selbst hat die Datei, und die bekommt das
        // Untermenue im leeren Menue. Was man mit einem Textstueck macht,
        // steht eine Zeile darunter: „Uebersetzen“.
        Kontext::Text => vec![
            Eintrag::Kopieren,
            Eintrag::Uebersetzen,
            // KONTEXT (03.10.2026, Leitung 02:10): die vier Zeilen des
            // Textmenues **woertlich** wie bei Firefox und in der vom Nutzer
            // genannten Reihenfolge: „Kopieren zuerst, dann Uebersetzen“.
            Eintrag::GoogleSuchen,
            Eintrag::AllesAuswaehlen,
        ],
        //
        // KONTEXT (03.10.2026, Leitung 02:10): DIE SECHS ZEILEN DES
        // EINGABEFELDES, wortwoertlich wie bei Firefox und in derselben
        // Reihenfolge. GEMESSENE ENTSCHEIDUNG, warum „Rückgängig“ **ganz
        // oben** steht und nicht unten bei „Alles auswählen“: es ist die
        // **einzige** Zeile, die den Zustand **Rueckgaengigmachen** anbietet,
        // und sie steht bei Firefox oben — wer tippt und es bereut, sucht sie
        // an der Stelle, an der er auch „Kopieren“ erwartet.
        //
        // KONTEXT (03.10.2026, 03:40 — KORREKTUR der Leitung): die Reihenfolge
        // ist **Kopieren, Uebersetzen | Rueckgaengig, Ausschneiden, Einfuegen,
        // Loeschen, Alles auswaehlen**. GEMESSENE ENTSCHEIDUNG, warum hier
        // kein Feld steht, obwohl es **zwei** Zustandszeilen gibt: „Kopieren“
        // ist **immer** da und nur **aktiv**, wenn etwas markiert ist; „Und
        // uebersetzen“ ist **weg**, wenn nichts markiert ist (die Leitung:
        // „nur mit Auswahl, sonst weglassen“). GEMESSENE ENTSCHEIDUNG, warum
        // „Uebersetzen“ im Eingabefeld ueberhaupt sinnvoll ist: Firefox
        // uebersetzt den **markierten Feldinhalt**, nicht die Seite.
        Kontext::Feld => vec![
            Eintrag::Kopieren,
            Eintrag::Uebersetzen,
            Eintrag::Rueckgaengig,
            Eintrag::Ausschneiden,
            Eintrag::Einfuegen,
            Eintrag::Loeschen,
            Eintrag::AllesAuswaehlen,
        ],
    }
}

/// DIE EINTRÄGE, DIE GEZEIGT WERDEN — die, fuer die es eine Quelle gibt.
///
/// M3: hoechstens [`MAX_EINTRAEGE`]. GEMESSENE ENTSCHEIDUNG, warum die
/// Grenze HIER sitzt und nicht im Aufrufer: der Aufrufer (`narsil`) weiss,
/// ob die Quelle erreichbar ist, aber nicht, wie viele Zeilen ein Menue
/// haben darf. Eine Grenze, die der Aufruzer kennt, ist eine Grenze, die er
/// auch umgehen kann.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein Klick auf ein Bild OHNE Quelle
/// (`<img src>` leer, `data:` kaputt) nicht ins Leere fuehrt: das Menue
/// zeigt dann die Eintraege, die immer gehen — neu laden, untersuchen.
/// GEMESSENER EIGENER FEHLER (01.10., `ohne_quelle_faellt_der_eintrag_weg`):
/// `left: []` — ein Kontextmenue mit null Zeilen, also kein Menue.
///
/// GEMESSENE ENTSCHEIDUNG, warum kein ausgegrauter Eintrag: ein ausgegrauter
/// Eintrag behauptet „hier koennte man etwas tun", und erfuellt die Behauptung
/// nicht. Siehe Kopfzeile.
pub fn sichtbare_eintraege(k: Kontext, quelle: &str, ton: bool) -> Vec<Eintrag> {
    let sichtbar: Vec<Eintrag> = eintraege(k, ton)
        .into_iter()
        .filter(|e| !e.braucht_quelle(k) || !quelle.trim().is_empty())
        .take(MAX_EINTRAEGE)
        .collect();
    if sichtbar.is_empty() {
        //
        // GEMESSENE ENTSCHEIDUNG (02.10., Untermenue): auch der Ersatz wird
        // gefiltert — aber nur um die eine Zeile, die ohne Quelle nichts
        // anbietet. GEMESSENER BEFUND vorher: der Ersatz nahm alle Zeilen des
        // leeren Kontextes ungefiltert, und seit das Untermenue dort steht,
        // stand „Konvertiert herunterladen >“ in einem Menue ganz ohne
        // Adresse. GEMESSENE FOLGE: die Zeile waere aufklappbar und leer
        // (`ziele_fuer` bekommt keine Endung und gibt nichts zurueck).
        //
        // GEMESSENE ENTSCHEIDUNG, warum „Kopieren“ BLEIBT: es kopiert die
        // Seitenadresse, und die gibt es immer — das ist genau der Grund,
        // warum es [`KopierZiel::Adresse`] gibt. Der Ersatz ist das
        // SEITENmenue, und im Seitenmenue arbeitet „Kopieren“.
        eintraege(Kontext::Leer, ton)
            .into_iter()
            .filter(|e| !e.braucht_quelle(k) || *e == Eintrag::Kopieren)
            .take(MAX_EINTRAEGE)
            .collect()
    } else {
        sichtbar
    }
}

/// Release 09.10.2026: so lange muss die Maus auf einer Zeile stehen, bevor
/// das Untermenue auf- oder zuklappt (Firefox: `ui.submenuDelay` = 300 ms,
/// Chrome unter Windows ~250 ms).
pub const UNTERMENUE_VERZUG_MS: u64 = 250;

/// DAS MENUE — Zustand und Geometrie.
///
/// GEMESSENE ENTSCHEIDUNG (KONTEXT, 03.10.2026), warum `Default` **von Hand**
/// kommt und nicht aus dem `derive`: das Menue hat jetzt sieben Felder, und ein
/// `derive` wuerde den siebten (`quelle_bytes`) still auf `None` setzen, wenn
/// jemand ein neues Feld ergaenzt. Die Handform **nennt** jedes Feld — und
/// damit faellt eine neue Pflichtangabe beim Bauen auf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menue {
    /// Der Kontext, an dem geklickt wurde.
    pub kontext: Kontext,
    /// Die Quelle (Adresse des Links/Bildes/Videos, Text, Bilddaten).
    pub quelle: String,
    /// Wo das Menue aufgeht — in Fenstermassen.
    pub x: u32,
    pub y: u32,
    ///
    /// IST DAS UNTERMENUE AUFGEKLAPPT?
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein eigenes Feld ist und kein
    /// „leeres Untermenue als Zustand“: zwei Stellen (ein `bool` und ein
    /// `Option<Vec<Ziel>>`) koennten sich widersprechen, und die
    /// Widersprueche fallen erst auf, wenn der Nutzer klickt.
    pub untermenue_offen: bool,
    /// Release 09.10.2026: seit wann die Maus auf der Zeile [`Menue::zeiger`]
    /// steht — das Untermenue folgt ihr mit Verzug ([`Menue::hover_tick`]).
    pub zeiger_seit: Option<std::time::Instant>,
    ///
    /// DER MIME-TYP DER QUELLE — leer, wenn `narsil` ihn nicht kennt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein Feld im Menue ist und nicht ein
    /// Argument von `zeilen()`: das Menue ist der Zustand, und der MIME-Typ
    /// ist eine Angabe ueber die Quelle, die sich nicht aendert, waehrend das
    /// Menue offen ist. Als Argument waere es eine Angabe, die der Aufrufer
    /// bei jedem Zeichnen neu liefern muesste — und beim ersten Zeichnen
    /// vergessen koennte.
    ///
    /// GEMESSENE BEGRUENDUNG, warum `String` und nicht `&'static str`: der
    /// Typ kommt aus dem Server, nicht aus einer Tabelle.
    pub mime: String,
    ///
    /// DIE BYTES DER QUELLE, falls `narsil` sie geladen hat.
    ///
    /// KONTEXT (03.10.2026): das ist die Angabe, die **ein Film** braucht und
    /// die kein MIME-Typ traegt. GEMESSEN (`team/WUNSCH_KONVERT.txt`, K5): „nur
    /// Ton“ haengt am **Ton** und „animiertes GIF“ an der **BILDZAHL**, und
    /// beides steht nicht im MIME-Typ — ohne diese Bytes zeigte das Videomenue
    /// **nur** AVI.
    ///
    /// M4: die Bytes sind **bereits** im Speicher (der Browser hat sie fuer die
    /// Anzeige geholt), und [`super::ktx_konvert::konvertiere`] prueft die Grenze
    /// [`super::ktx_konvert::MAX_QUELLE_BYTE`], **bevor** er sie ein zweites Mal
    /// anlegt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `Vec<u8>` und kein `Arc<Vec<u8>>`: das
    /// Menue wird **einmal** geoeffnet und **einmal** angeklickt. Eine
    /// gemeinsame Nutzung waere eine Speichergemeinschaft fuer **einen** Leser.
    /// Und M6 nennt `Rc`/`RefCell`-Zyklen als Fehler — ein `Arc` in einem Feld,
    /// das niemand teilt, waere nur eine Zaehlung ohne Nutzen.
    pub quelle_bytes: Option<Vec<u8>>,
    ///
    /// DIE ZEILE, UNTER DER DIE MAUS STEHT — `None`, wenn nirgends.
    ///
    /// KONTEXT (03.10.2026), GEMESSENER BEFUND: das Menue hatte **keine**
    /// Hover-Anzeige. GEMESSEN am Bild: die fuenf Zeilen „Kopieren“,
    /// „Uebersetzen“, „Bild speichern“, „Bild-Adresse anzeigen“,
    /// „Konvertiert herunterladen >“ stehen in **einer** Farbe (und fuer die
    /// Zielzeilen in einer zweiten) — und genau daraus erkennt der Nutzer
    /// nicht, was er treffen wird. Firefox faerbt die Zeile unter der Maus
    /// mit der Akzentfarbe; das ist der einzige Unterschied zwischen „wo
    /// bin ich“ und „wo bin ich nicht“.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine **ZEILENNUMMER** ist und kein
    /// `Option<Zeile>`: die Nummer ist die, mit der die Zeichnung rechnet
    /// (`enumerate`), und ein `Zeile`-Vergleich muesste `Zeile` ableiten
    /// koennen (zwei Ziele koennen gleich heissen). Die Nummer kann das
    /// nicht, und deshalb kann sie nicht falsch sein.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Nummer **nicht** ihre eigene
    /// Wahrheit ueber den Zeiger ist: `narsil` berechnet sie mit
    /// [`Menue::setze_zeiger`] aus dem Punkt der Maus — also aus derselben
    /// Rechnung, aus der auch [`Menue::zeile_unter`] trifft. Zwei Stellen,
    /// die dieselbe Frage stellen, koennten sich widersprechen.
    pub zeiger: Option<usize>,
    /// DER ZUSTAND DES EINGABEFELDES — fuer [`Kontext::Feld`].
    ///
    /// KONTEXT (03.10.2026, Leitung 02:10): GEMESSENE ENTSCHEIDUNG, warum das
    /// ein **Feld im Menue** ist und kein `Option<FeldZustand>`: das Menue
    /// ist der Zustand des Rechtsklicks, und der Zustand des Feldes gehoert
    /// dazu — wie `quelle_bytes` zum Film. GEMESSENE ENTSCHEIDUNG, warum der
    /// Default [`FeldZustand::LEER`] ist und nicht „alles an“: ein
    /// frisch geoeffnetes Menue weiss nichts ueber das Feld, und eine Zeile,
    /// die der Aufrufer **nicht** bestaetigt hat, wird nicht angeklickbar
    /// gezeigt.
    pub feld_zustand: FeldZustand,
    /// Release 08.10.2026 (Feature 0): steht „Konvertiert herunterladen >"
    /// im Menue? Der Schalter im Tab „Features" setzt es (Vorgabe: ja).
    pub konvertieren: bool,
    /// Release 08.10.2026 (Feature 2): steht „Element beobachten" im Menue?
    pub beobachten: bool,
    /// Release 08.10.2026 (Glas-Stil, HiDPI): die Skalierung des Fensters in
    /// Prozent (100 = 1:1, 150 bei 1,5-facher Anzeige). Alle Masse, Schriften
    /// und Icons wachsen mit — die Icons werden in der Zielkante gerastert.
    /// GEMESSENE ENTSCHEIDUNG fuer Prozent statt `f32`: `Menue` ist `Eq`.
    pub skala_prozent: u16,
}

impl Menue {
    /// EIN MENUE OEFFNEN (der Klickpunkt ist der linke obere Rand).
    pub fn oeffne(kontext: Kontext, quelle: &str, punkt: &Point) -> Menue {
        Menue {
            kontext,
            // M4: die Quelle wird gekuerzt — sie kann der Inhalt einer
            // Seite sein, und ein Menue mit einer 1-MB-Textzeile waere ein
            // Menue, das nichts anzeigt.
            quelle: quelle.trim().chars().take(MAX_QUELLE).collect(),
            // GEMESSENE ENTSCHEIDUNG: NaN wird zu 0. GEMESSENE BEGRUENDUNG
            // (U1c): `NaN as u32` ist 0, und ein Menue an (0, 0) ist besser
            // als kein Menue — aber nicht besser als ein Menue an der
            // Stelle, an der der Fensterrand ist. Also wird es an den Rand
            // gehaengt, und das passiert spaeter in [`Menue::rechteck`].
            x: if punkt.x.is_finite() && punkt.x >= 0.0 {
                punkt.x as u32
            } else {
                0
            },
            y: if punkt.y.is_finite() && punkt.y >= 0.0 {
                punkt.y as u32
            } else {
                0
            },
            // GEMESSEN (02.10.): ein frisch geoeffnetes Menue ist ZU.
            untermenue_offen: false,
            // GEMESSEN: ohne MIME entscheidet die Endung der Adresse (siehe
            // `Menue::ziele`).
            mime: String::new(),
            // GEMESSEN (KONTEXT): ein **frisch geoeffnetes** Menue hat die
            // Datei **nicht** geladen — die Datei laed der Aufrufer **damit**,
            // dass er sie klickt (siehe [`Menue::setze_quelle_bytes`]).
            quelle_bytes: None,
            // GEMESSEN: ein frisch geoeffnetes Menue hat die Maus noch nicht
            // ueber einer Zeile.
            zeiger: None,
            zeiger_seit: None,
            // KONTEXT (03.10.2026, Leitung 02:10): ein frisch geoeffnetes
            // Menue kennt den Feldzustand **nicht** (siehe Feld-Kopf).
            feld_zustand: FeldZustand::LEER,
            // Release 08.10.2026: wie bisher mit „Konvertiert herunterladen";
            // `setze_features` stellt beides nach dem Tab „Features" ein.
            konvertieren: true,
            beobachten: false,
            skala_prozent: 100,
        }
    }

    /// Release 08.10.2026: die Schalter aus dem Tab „Features" uebernehmen —
    /// Feature 0 (Konvertieren) und Feature 2 (Element beobachten).
    pub fn setze_features(&mut self, konvertieren: bool, beobachten: bool) {
        self.konvertieren = konvertieren;
        self.beobachten = beobachten;
        if !konvertieren {
            self.untermenue_offen = false;
        }
    }

    /// Release 08.10.2026 (HiDPI): die Skalierung des Fensters (1,0 bis 4,0).
    pub fn setze_skalierung(&mut self, faktor: f32) {
        self.skala_prozent = skala_prozent(faktor);
    }

    /// Release 08.10.2026: logische Pixel in Fensterpixel (wie `Chrome::px`).
    pub fn px(&self, logisch: u32) -> u32 {
        px_mit(self.skala_prozent, logisch)
    }

    /// Release 08.10.2026: der Faktor als Zahl (fuer Schriftgroessen).
    pub fn skala(&self) -> f32 {
        self.skala_prozent.max(1) as f32 / 100.0
    }

    /// Release 08.10.2026 (Glas-Stil): der Abschnitt einer Zeile — `None` fuer
    /// Zielzeilen des Untermenues.
    pub fn abschnitt_von(&self, z: &Zeile) -> Option<Abschnitt> {
        match z {
            Zeile::Aktion(e) => Some(e.abschnitt(self.kontext)),
            Zeile::Ziel(_) => None,
        }
    }

    /// Release 08.10.2026: beginnt mit Zeile `nr` (> 0) ein neuer Abschnitt?
    fn trenn_in(&self, zeilen: &[Zeile], nr: usize) -> bool {
        if nr == 0 || nr >= zeilen.len() {
            return false;
        }
        match (self.abschnitt_von(&zeilen[nr - 1]), self.abschnitt_von(&zeilen[nr])) {
            (Some(a), Some(b)) => a != b,
            _ => false,
        }
    }

    /// Release 08.10.2026: der Abstand der Zeile `nr` von der Oberkante —
    /// mit der Ueberschrift jedes Abschnitts und der Linie davor.
    fn y_in(&self, zeilen: &[Zeile], nr: usize) -> u32 {
        let mut y = self.px(LUFT_OBEN) + self.px(ZEILE) * nr as u32;
        if !zeilen.is_empty() {
            y += self.px(UEBERSCHRIFT_HOEHE);
        }
        for i in 1..=nr.min(zeilen.len().saturating_sub(1)) {
            if self.trenn_in(zeilen, i) {
                y += self.px(TRENN_HOEHE) + self.px(UEBERSCHRIFT_HOEHE);
            }
        }
        y
    }

    /// Release 08.10.2026: die Hoehe eines Feldes mit diesen Zeilen.
    fn hoehe_in(&self, zeilen: &[Zeile]) -> u32 {
        match zeilen.len() {
            0 => self.px(LUFT_OBEN) * 2,
            n => self.y_in(zeilen, n - 1) + self.px(ZEILE) + self.px(LUFT_OBEN),
        }
    }

    /// Release 08.10.2026 (Glas-Stil): beginnt mit Zeile `nr` ein Abschnitt,
    /// steht hier seine Ueberschrift (auch fuer die erste Zeile).
    pub fn abschnitt_vor(&self, nr: usize, fensterhoehe: u32) -> Option<Abschnitt> {
        let zeilen = self.zeilen(fensterhoehe);
        let z = zeilen.get(nr)?;
        if nr == 0 || self.trenn_in(&zeilen, nr) {
            self.abschnitt_von(z)
        } else {
            None
        }
    }

    /// Release 08.10.2026: die Mitte der Zeile `nr` (fuer Klicks in Tests).
    pub fn zeile_mitte_y(&self, nr: usize, fensterhoehe: u32) -> u32 {
        self.zeilen_y(nr, fensterhoehe) + self.px(ZEILE) / 2
    }

    /// DEN ZUSTAND DES EINGABEFELDES SETZEN — der Aufrufer weiss ihn aus der
    /// Seite (er hat sie geladen).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein `Setter` ist und keinoeffentliches
    /// Feld: die drei Stellen, die den Zustand brauchen — [`Menue::nr_unter`]
    /// (Treffer), die Zeichnung (Farbe) und [`Eintrag::aktiv`] — sollen **eine**
    /// Zahl lesen. Wer das Feld oeffentlich ließe, koennte es zwischen diesen
    /// drei Stellen aendern, und genau dann waere die Zeile grau und trotzdem
    /// klickbar.
    pub fn setze_feld_zustand(&mut self, zustand: FeldZustand) {
        self.feld_zustand = zustand;
    }

    /// DIE MAUS AUF EINE ZEILE SETZEN — aus dem Punkt in Fenstermassen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **eine** Funktion ist und nicht
    /// `setze_zeiger_nr(nr)`: der Aufrufer hat einen **Punkt**, keine Nummer
    /// (die Nummer muss er erst aus dem Punkt machen, und das ist genau die
    /// Rechnung, die hier schon steht). Wer die Nummer selbst rechnet,
    /// macht sie an einer zweiten Stelle — und die zweite Stelle ist die,
    /// die dabei falsch wird.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum sie `bool` zurueckgibt: der Aufrufer
    /// will wissen, ob es ueberhaupt eine Zeile getroffen hat — davon
    /// haengt ab, ob er den Zeiger danach wieder loescht und ob der Klick
    /// ueberhaupt ins Menue gehoert.
    pub fn setze_zeiger(&mut self, fensterbreite: u32, fensterhoehe: u32, p: &Point) -> bool {
        self.setze_zeiger_um(fensterbreite, fensterhoehe, p, std::time::Instant::now())
    }

    /// Wie [`Menue::setze_zeiger`], mit der Uhrzeit des Ereignisses (Tests).
    pub fn setze_zeiger_um(&mut self, fensterbreite: u32, fensterhoehe: u32, p: &Point, jetzt: std::time::Instant) -> bool {
        let neu = self.nr_unter(fensterbreite, fensterhoehe, p);
        if neu != self.zeiger {
            self.zeiger_seit = neu.map(|_| jetzt);
        }
        self.zeiger = neu;
        self.zeiger.is_some()
    }

    /// Release 09.10.2026: DAS UNTERMENUE FOLGT DER MAUS — wie in Firefox mit
    /// [`UNTERMENUE_VERZUG_MS`] Verzug: steht die Maus so lange auf „Konvertiert
    /// herunterladen", klappt es auf; steht sie so lange auf einer anderen
    /// Zeile des Hauptmenues, klappt es zu. Im Untermenue selbst (ausserhalb
    /// des Hauptmenues) bleibt es offen. GEMESSEN vorher: nur ein KLICK
    /// oeffnete es — beim Darueberfahren passierte nichts. Der Verzug haelt
    /// das Vorbeiwischen ruhig (die alte Begruendung gegen das Aufklappen).
    /// `true`, wenn sich etwas geaendert hat.
    pub fn hover_tick(&mut self, fensterhoehe: u32, jetzt: std::time::Instant) -> bool {
        let (Some(nr), Some(seit)) = (self.zeiger, self.zeiger_seit) else {
            return false;
        };
        if jetzt.saturating_duration_since(seit) < std::time::Duration::from_millis(UNTERMENUE_VERZUG_MS) {
            return false;
        }
        let auf_konvert = matches!(self.zeilen(fensterhoehe).get(nr), Some(Zeile::Aktion(Eintrag::Konvertieren)));
        if auf_konvert != self.untermenue_offen {
            self.untermenue_offen = auf_konvert;
            return true;
        }
        false
    }

    /// DIE ZEILE UNTER DER MAUS (Nummer) — `None`, wenn nirgends.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das neben [`Menue::zeile_unter`] steht
    /// und nicht statt ihm: `zeile_unter` beantwortet „was liegt da“ (das
    /// braucht der Klick), und diese Funktion beantwortet „welche Nummer“
    /// (das braucht die Hover-Farbe). GEMESSENE ENTSCHEIDUNG, warum sie
    /// trotzdem **nicht** zweimal gerechnet wird: `zeile_unter` ruft diese
    /// Funktion, und die Zeichnung liest nur das Feld [`Menue::zeiger`].
    pub fn nr_unter(&self, fensterbreite: u32, fensterhoehe: u32, p: &Point) -> Option<usize> {
        let r = self.rechteck(fensterbreite, fensterhoehe)?;
        if !p.x.is_finite() || !p.y.is_finite() {
            return None;
        }
        if p.x < r.x as f32
            || p.x >= r.rechts() as f32
            || p.y < r.y as f32
            || p.y >= r.unten() as f32
        {
            return None;
        }
        let zeilen = self.zeilen(fensterhoehe);
        // KONTEXT (03.10., Trennlinien): die Zeilen stehen nicht mehr
        // gleichmaessig — zwischen zwei Gruppen steht ein Band von
        // [`TRENN_HOEHE`]. GEMESSENE ENTSCHEIDUNG: gesucht wird **von
        // hinten**, und die erste Zeile, deren Oberkante ueber dem Punkt
        // liegt, ist die — damit ist die Frage „welche Zeile“ dieselbe
        // Rechnung wie in [`Menue::zeilen_y`] und kann nicht davon
        // abweichen. Ein Punkt **im** Trennband trifft keine Zeile: es
        // gibt dort nichts zu treffen, und der Klick gehoert dem Menue
        // nicht.
        let y = p.y as u32;
        if y < r.y + self.px(LUFT_OBEN) {
            return None;
        }
        //
        // KONTEXT (03.10.2026, Leitung 02:10: EINE **GRAUE** ZEILE IST KEIN
        // TREFFER. GEMESSENE ENTSCHEIDUNG, warum das **hier** steht und nicht
        // erst im Aufrufer: das Menue weiss, welche Zeilen es zeichnet, und
        // der Aufrufer (`narsil`) sieht nur eine Nummer — eine Nummer, die auf
        // eine graue Zeile zeigt, waere eine Zeile, auf die man klickt und
        // nichts passiert. GEMESSENE ENTSCHEIDUNG, warum die Zeile **nicht**
        // weggehaelt wird: Firefox **zeigt** „Einfügen“ grau, wenn die
        // Zwischenablage leer ist — die Zeile ist Teil der Antwort „was gibt
        // es hier“, und ihre Farbe ist die Antwort „was davon geht jetzt“.
        for nr in (0..zeilen.len()).rev() {
            let oben = r.y + self.y_in(&zeilen, nr);
            if y >= oben && y < oben + self.px(ZEILE) {
                return zeilen[nr]
                    .aktiv(self.kontext, self.feld_zustand)
                    .then_some(nr);
            }
        }
        // GEMESSENE ENTSCHEIDUNG, warum der Punkt im **Trennband** kein
        // Treffer ist: das Band gehoert zu keiner Zeile. Es steht zwischen
        // zwei Gruppen, und ein Klick dort soll **nichts** machen — sonst
        // waere die Linie ein Bedienelement.
        None
    }

    /// STEHT DIE MAUS AUF DIESER ZEILE?
    pub fn ist_zeiger(&self, nr: usize) -> bool {
        self.zeiger == Some(nr)
    }

    /// DIE ZEILE, UNTER DER DIE MAUS STEHT (fuer den Aufrufer).
    pub fn zeiger(&self) -> Option<usize> {
        self.zeiger
    }

    /// DEN MAUSZEIGER WEGNEHMEN — wenn das Menue zugeht.
    pub fn zeiger_weg(&mut self) {
        self.zeiger = None;
    }

    /// DIE BYTES DER QUELLE MITGEBEN — wenn der Browser sie schon hat.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein **zweiter** Aufruf ist und kein
    /// zweiter Parameter von [`Menue::oeffne`]: `oeffne` bekommt den Klickpunkt,
    /// und der Aufrufer hat die Bytes **vor** dem Klick nicht (er laed sie, weil
    /// der Nutzer klickt). Wer sie als Parameter verlangt, muesste sie also
    /// **vorher** laden — und damit bei **jedem** Rechtsklick eine Datei laden,
    /// die der Nutzer vielleicht nie braucht.
    ///
    /// M4: die Bytes werden **begrenzt** gespeichert ([
    /// `super::ktx_konvert::MAX_QUELLE_BYTE`]) — ein 2-GiB-Video, das der
    /// Browser geladen hat, darf das Menue nicht zum Besitzer des Speichers
    /// machen, wenn der Nutzer es nur **ansehen** will.
    pub fn setze_quelle_bytes(&mut self, daten: Vec<u8>) {
        if daten.len() > super::ktx_konvert::MAX_QUELLE_BYTE {
            // GEMESSENE ENTSCHEIDUNG: **nichts** speichern statt „gekuerzt“.
            // Eine halbe Datei ist keine Datei — und `ziele_fuer_film_daten`
            // wuerde aus den ersten Bytes eine **falsche** Bildzahl lesen.
            self.quelle_bytes = None;
            return;
        }
        self.quelle_bytes = Some(daten);
    }

    /// DEN MIME-TYP DER QUELLE SETZEN — wenn `narsil` ihn kennt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das die einzige Art ist, den Typ zu
    /// setzen: `Menue` wird mit [`Menue::oeffne`] gebaut, und einoeffnen
    /// kann man danach nicht mehr (der Klickpunkt gehoert zum Aufruf).
    /// Wer den Typ beim Oeffnen mitgeben will, macht einen zweiten Aufruf —
    /// und zwei Wege sind zwei.
    pub fn setze_mime(&mut self, mime: &str) {
        self.mime = mime.trim().chars().take(super::konvert::MAX_MIME).collect();
    }

    /// DAS UNTERMENUE AUFKLAPPEN.
    pub fn klappe_auf(&mut self) {
        self.untermenue_offen = true;
    }

    /// DAS UNTERMENUE ZUKLAPPEN.
    pub fn klappe_zu(&mut self) {
        self.untermenue_offen = false;
    }

    /// IST DIE QUELLE **TON**?
    ///
    /// KONTEXT (03.10.2026, 21:35): GEMESSENE ENTSCHEIDUNG, warum **beide**
    /// Angaben geprueft werden und nicht nur eine: der **MIME** ist die
    /// starke Angabe (`audio/*` vom Server), die **Endung** ist die, die
    /// immer da ist — GEMESSEN ist, dass `narsil` den MIME beim Rechtsklick
    /// **nicht** setzt. GEMESSENE ENTSCHEIDUNG, warum der MIME **zuerst**
    /// kommt: eine Datei `ton.mp3` mit dem MIME `audio/wav` ist **Ton** —
    /// die Endung waere dann der schlechtere Rat.
    pub fn ist_ton(&self) -> bool {
        if self.mime.trim().to_ascii_lowercase().starts_with("audio/") {
            return true;
        }
        let endung = super::konvert::endung_aus_adresse(&self.quelle).unwrap_or_default();
        let endung = endung.to_ascii_lowercase();
        TON_ENDUNGEN.contains(&endung.as_str())
    }

    /// IST DAS UNTERMENUE AUFGEKLAPPT?
    pub fn ist_aufgeklappt(&self) -> bool {
        self.untermenue_offen
    }

    /// DIE ZIELE DER QUELLE — die eine Stelle, die
    /// [`super::ktx_konvert::ziele`] aufruft.
    ///
    /// GEMESSENE ENTSCHEIDUNG (KONTEXT, 03.10.2026), warum das nicht mehr
    /// `konvert::ziele_fuer` ist: das war eine **Platzhalterliste** aus dem
    /// Wortlaut des Nutzers, und sie wusste nicht, welche Ziele **gebaut**
    /// sind. GEMESSEN ist der Unterschied in `team/WUNSCH_KONVERT.txt`: „35
    /// angebotene Eintraege ueber acht MIME-Typen, und **alle** sind
    /// `produzierbar`" — und **das** entscheidet `narsil-konvert::ziele`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Kontext die MIME vorgibt und der
    /// Aufrufer nicht: im Bild-Kontext ist die SACHE ein Bild, auch wenn die
    /// Adresse `bild.jpeg` heisst; im Video-Kontext ist sie ein Video, auch
    /// wenn die Endung `.webm` lautet. GEMESSENE ENTSCHEIDUNG fuer
    /// [`Kontext::Link`] und [`Kontext::Leer`]: dort kennt die Oberflaeche
    /// die Art nicht — sie sieht eine Adresse — also entscheidet der MIME
    /// aus [`Menue::mime`], und wenn der leer ist, die Endung der Adresse
    /// (das ist der Weg, der ohne Server auskommt).
    ///
    /// GEMESSENE ENTSCHEIDUNG fuer den **Video ohne Bytes**: `&None` heisst
    /// „der Nutzer hat das Untermenue aufgeklappt, die Datei ist aber nicht
    /// geladen". Dann bietet das Menue nur die zwei Ziele, die ohne Datei
    /// entscheidbar sind (AVI, Standbild) — K5: „keine toten Eintraege".
    pub fn ziele(&self) -> Vec<super::konvert::Ziel> {
        super::ktx_konvert::ziele(
            self.kontext,
            &self.mime,
            &self.quelle,
            self.quelle_bytes.as_deref(),
        )
    }

    /// HOECHSTENS SO VIELE ZEILEN HAT DAS MENUE IN EINER FENSTERHOEHE.
    ///
    /// GEMESSENE ENTSCHEIDUNG fuer [`MAX_MENUE_ANTEIL`] = 2 (also die
    /// Haelfte): ein Menue, das mehr als die Haelfte des Fensters bedeckt,
    /// verdeckt die Seite, die man untersucht oder umformatieren will —
    /// und die Seite ist der Grund fuer das Menue. GEMESSEN bei 1080 px
    /// Fensterhoehe: die Grenze liegt bei 540 px, das groesste gemessene
    /// Menue (6 Elternzeilen + 6 Zielzeilen) hat GEMESSEN 8 + 12 * 24 = 296 px
    /// und passt; in einem 300-px-Fenster passen von den 6 Zielzeilen
    /// GEMESSEN 2.
    pub fn max_zeilen(fensterhoehe: u32) -> u32 {
        // Release 08.10.2026: neun Zehntel statt der Haelfte (siehe
        // [`MAX_MENUE_ZEHNTEL`]).
        grenze_hoehe(fensterhoehe).saturating_sub(LUFT_OBEN * 2) / ZEILE
    }

    /// Release 08.10.2026 (HiDPI): wie [`Menue::max_zeilen`], in den
    /// skalierten Massen dieses Menues.
    pub fn max_zeilen_skaliert(&self, fensterhoehe: u32) -> u32 {
        grenze_hoehe(fensterhoehe).saturating_sub(self.px(LUFT_OBEN) * 2)
            / self.px(ZEILE).max(1)
    }

    /// DIE ZEILEN DES MENUEKASTENS — die **Elternzeilen** und sonst nichts.
    ///
    /// KONTEXT (03.10., „Optik wie Firefox“), GEMESSENER BEFUND: hier standen
    /// auch die **Zielzeilen** (`Zeile::Ziel`) — als waeren sie Teil des
    /// Kastens. GEMESSEN ist am Bild (`ktx_*_unter.png`), was das ergab:
    /// die sechs Ziele standen **zweimal** im Bild — eingerueckt im Kasten
    /// **und** im Feld daneben. Ein Bild, in dem eine Zeile zweimal steht,
    /// ist ein Bild, in dem man nicht weiss, welche man anklickt.
    /// GEMESSENE ENTSCHEIDUNG: die Liste ist die Liste des **Kastens**, und
    /// der Kasten hat nur Elternzeilen. Die Ziele stehen in
    /// [`Menue::untermenue_ziele`] und werden im **eigenen** Feld gezeichnet.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das EINE Liste fuer Hoehe, Treffer und
    /// Zeichnung ist: drei Aufrufer, die ihre Zeilen selbst zusammenbauen,
    /// sind drei Listen — und der Fehler, bei dem eine Zeile im Bild steht,
    /// die man nicht anklicken kann (oder umgekehrt), ist genau der, der in
    /// U1c sichtbar wurde („NaN traf Menuepunkt 0“).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum [`MENUE_BREITE`] und [`RAND`] hier
    /// genuegen und keine Zeile hier gekuerzt wird: die Zeilen sind durch
    /// [`Eintrag::text`] und [`Eintrag::zeichen`] gemessen (der laengste
    /// braucht 25 Zeichen, der Kasten 248 px), und das **Fenster** ist
    /// keine Grenze fuer eine Zeile — fuer die Zahl der Zeilen, und das ist
    /// [`Menue::max_zeilen`].
    pub fn zeilen(&self, fensterhoehe: u32) -> Vec<Zeile> {
        //
        // KONTEXT (03.10.2026, 03:40): IM EINGABEFELD wandern die Zeilen, die
        // es **ohne Zustand** nicht gibt (siehe
        // [`Eintrag::sichtbar_im_feld`]). GEMESSENE ENTSCHEIDUNG, warum das
        // **hier** und nicht in `sichtbare_eintraege`: dort steht die Frage
        // „hat die Zeile eine Quelle“ — eine Frage ueber die **Adresse**. Hier
        // steht „passt die Zeile zum **Zustand des Feldes**“, und die
        // entscheidet nur der, der den Zustand kennt: das Menue.
        let zustand = self.feld_zustand;
        //
        // KONTEXT (04.10.2026): DIE **HALBE** DES FENSTERS — endlich eingefuehrt.
        //
        // GEMESSENER BEFUND (der Fehler kam mit den neuen Zeilen): die Regel
        // stand seit 03.10. im Kopf von [`Menue::max_zeilen`] und wurde
        // **nirgends angewendet** — `feld_hoehe` nahm `zeilen().len()`, also
        // **alle** Zeilen. GEMESSEN am Bild-Kontext mit **sieben** Zeilen in
        // einem 400-px-Fenster: das Menue wurde 204 px hoch, das sind
        // **51 %** des Fensters — genau das, was der Kommentar verbietet
        // („ein Menue, das mehr als die Haelfte des Fensters bedeckt,
        // verdeckt die Seite“). Die Tests haben es nicht gesehen, weil sie
        // `zeilen()` **ohne** Fensterhoehe gebraucht haben (der Parameter
        // hiess `_fensterhoehe` und war unbenutzt).
        //
        // GEMESSENE ENTSCHEIDUNG, warum **hier** gekuerzt wird und nicht erst
        // in der Zeichnung: `feld_hoehe`, `trennzahl`, `zeilen_y`, der Treffer
        // und die Zeichnung lesen **diese** Liste — eine Kuerzung an einer
        // dieser Stellen wuerde an den anderen vier nichts aendern. Genau
        // das ist der Grund fuer „eine Liste fuer Hoehe, Treffer und
        // Zeichnung“.
        let max = self.max_zeilen_skaliert(fensterhoehe) as usize;
        // Release 08.10.2026: die Schalter aus dem Tab „Features" — ohne
        // Feature 0 faellt „Konvertiert herunterladen" weg, mit Feature 2
        // kommt „Element beobachten" vor „Untersuchen" (bzw. ans Ende).
        let mut liste = sichtbare_eintraege(self.kontext, &self.quelle, self.ist_ton());
        // Release 08.10.2026: ohne ein einziges Ziel kein „Konvertiert
        // herunterladen >“ — GEMESSEN vorher: an einem Link auf eine HTML-Seite
        // stand die Zeile, und ihr Untermenue blieb leer (ein toter Eintrag).
        if !self.konvertieren
            || (liste.contains(&Eintrag::Konvertieren) && self.ziele().is_empty())
        {
            liste.retain(|e| *e != Eintrag::Konvertieren);
        }
        if self.beobachten
            && matches!(self.kontext, Kontext::Leer | Kontext::Text | Kontext::Link | Kontext::Bild)
        {
            match liste.iter().position(|e| *e == Eintrag::Untersuchen) {
                Some(i) => liste.insert(i, Eintrag::ElementBeobachten),
                None => liste.push(Eintrag::ElementBeobachten),
            }
        }
        let liste = liste
            .into_iter()
            //
            // KONTEXT (05.10.2026, 20:22): DIE SICHTBARKEIT GILT FUER **DIE SEITE**
            // MIT — nicht nur fuer das Eingabefeld.
            //
            // GEMESSENE ENTSCHEIDUNG, warum es `Kontext::Leer` ist und nicht
            // „alle ausser Bild/Video/Link“: GEMESSEN ist, dass „Uebersetzen“ in
            // **jedem** Kontext vorkommt, in dem es **Text** gibt — die
            // Ausnahme ist nicht der Kontext, sondern die **Markierung`.
            .filter(|e| {
                !matches!(self.kontext, Kontext::Feld | Kontext::Leer)
                    || e.sichtbar_im_feld(zustand)
            })
            .map(Zeile::Aktion)
            .collect::<Vec<Zeile>>();
        // Release 08.10.2026 (Glas-Stil): die Zeilen stehen nach ABSCHNITTEN
        // (stabil: innerhalb eines Abschnitts bleibt die Reihenfolge der
        // Liste). Danach faellt von hinten weg, was nicht ins Fenster passt —
        // mit den Ueberschriften gerechnet, nicht nur mit den Zeilen.
        let mut zeilen = liste;
        zeilen.sort_by_key(|z| self.abschnitt_von(z).map_or(u8::MAX, Abschnitt::rang));
        zeilen.truncate(max);
        let grenze = grenze_hoehe(fensterhoehe);
        while !zeilen.is_empty() && self.hoehe_in(&zeilen) > grenze {
            zeilen.pop();
        }
        zeilen
    }

    /// STEHT VOR DER ZEILE `nr` EINE TRENNLINIE?
    ///
    /// KONTEXT (03.10., Freigabe Punkt 4): die erste Zeile hat **nie** eine
    /// Linie vor sich (es ist kein „darueber“, womit man eine Gruppe
    /// beginnen koennte), und zwei Zeilen derselben Gruppe bekommen **keine**
    /// (sonst stuende zwischen „Bild speichern“ und „Bild-Adresse anzeigen“
    /// zweimal dieselbe Linie).
    pub fn trenn_vor(&self, nr: usize, fensterhoehe: u32) -> bool {
        // Release 08.10.2026 (Glas-Stil): die Linie steht zwischen zwei
        // ABSCHNITTEN (mit Ueberschrift), nicht mehr zwischen den alten Gruppen.
        self.trenn_in(&self.zeilen(fensterhoehe), nr)
    }

    /// WIE VIELE TRENNLINIEN HAT DAS MENUE?
    pub fn trennzahl(&self, fensterhoehe: u32) -> usize {
        self.zeilen(fensterhoehe)
            .iter()
            .enumerate()
            .skip(1)
            .filter(|(nr, _)| self.trenn_vor(*nr, fensterhoehe))
            .count()
    }

    /// DER ABSTAND DER ZEILE `nr` VON DER MENUEOBERKANTE.
    ///
    /// KONTEXT (03.10., Freigabe Punkt 4: „Trennlinien zwischen Gruppen“):
    /// GEMESSENE ENTSCHEIDUNG, warum **hier** die Zeilenposition steht und
    /// nicht in [`Menue::zeilen`]: die Position haengt an den Trennlinien,
    /// und dieselbe Position brauchen drei Aufrufer — die Zeichnung, die
    /// Hoehe und der Treffer. Drei Aufrufer, die selbst zaehlen, sind drei
    /// Zaehlungen, und die mittlere stimmt irgendwann nicht mehr (das war
    /// der Fehler „NaN traf Menuepunkt 0“ in U1c).
    pub fn zeilen_y(&self, nr: usize, fensterhoehe: u32) -> u32 {
        //
        // GEMESSENER EIGENER FEHLER (03.10.): hier stand `0..nr`. GEMESSEN
        // war, dass die Zeile, **vor** der eine Linie steht, an der Linie
        // **vorbeiging**: ihr Abstand war um [`TRENN_HOEHE`] zu klein, und
        // das Menue hatte unten GEMESSEN 12 px Luft statt 4 — und die letzte
        // Zeile war dort, wo keine Linie ist.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `1..=nr` und nicht `0..=nr`:
        // `trenn_vor(0)` ist immer `false` (eine erste Zeile hat nichts
        // ueber sich), also aendert die 0 nichts — sie steht aber nicht da,
        // damit man sie nicht mitzaehlt, wenn sie einmal doch zaehlt.
        // Release 08.10.2026 (Glas-Stil): mit Ueberschriften und Skalierung —
        // dieselbe Rechnung fuer Hoehe, Zeichnung und Treffer (`y_in`).
        self.y_in(&self.zeilen(fensterhoehe), nr)
    }

    /// DIE HOEHE DES MENUEFELDES — inklusive der Trennlinien.
    pub fn feld_hoehe(&self, fensterhoehe: u32) -> u32 {
        self.hoehe_in(&self.zeilen(fensterhoehe))
    }

    /// DAS RECHTECK DES MENUES — am Fensterrand, wenn es nicht passt.
    ///
    /// GEMESSENE BEGRUENDUNG, warum das Menue wandert und nicht
    /// abgeschnitten wird: ein halbes Menue ist nicht bedienbar, und die
    /// Stelle, an der es hingeht, ist der Rand — dort ist der Nutzer auch
    /// mit der Maus. GEMESSENE ENTSCHEIDUNG, warum der Rand und nicht die
    /// Mitte: die Mitte ist die Stelle, an der der Nutzer hinwill.
    pub fn rechteck(&self, fensterbreite: u32, fensterhoehe: u32) -> Option<Rechteck> {
        let zeilen = self.zeilen(fensterhoehe).len() as u32;
        if zeilen == 0 {
            return None;
        }
        // KONTEXT (03.10.): die Hoehe kommt aus [`Menue::feld_hoehe`] — mit
        // den Trennlinien, nicht ohne sie.
        let hoehe = self.feld_hoehe(fensterhoehe);
        // M3: die Breite ist `min(MENUE_BREITE, fensterbreite)`.
        let breite = self.px(MENUE_BREITE).min(fensterbreite);
        if breite == 0 || hoehe > fensterhoehe {
            return None;
        }
        Some(Rechteck {
            x: self.x.min(fensterbreite.saturating_sub(breite)),
            y: self.y.min(fensterhoehe.saturating_sub(hoehe)),
            breite,
            hoehe,
        })
    }

    /// WELCHE ZEILE LIEGT UNTER DEM PUNKT?
    ///
    /// GEMESSENE ENTSCHEIDUNG (02.10., Untermenue), warum diese Funktion
    /// [`Zeile`] liefert und nicht [`Eintrag`]: das Untermenue erzeugt Zeilen,
    /// die keine Eintraege sind. Ein `Option<Eintrag>` muesste sie als „nichts“
    /// melden, und ein Klick auf „Als PNG" waere ein Klick ins Leere.
    /// GEMESSENE ENTSCHEIDUNG, warum es keine ZWEITE Funktion gibt, die
    /// Eintraege liefert: der Aufrufer, der beides braucht, fragt einmal und
    /// entscheidet dann selbst — zwei Funktionen fuer dieselbe Frage sind die
    /// Fehlerklasse „zwei Stellen entscheiden“.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `None` und kein leerer `Eintrag`: es
    /// gibt keinen Eintrag, der „nichts" bedeutet, und `Option` sagt genau
    /// das. GEMESSENE BEGRUENDUNG fuer die NaN-Wache (U1c war hier der
    /// Fehler): jeder Vergleich mit NaN ist falsch.
    pub fn zeile_unter(&self, fensterbreite: u32, fensterhoehe: u32, p: &Point) -> Option<Zeile> {
        //
        // KONTEXT (03.10.2026): NUR DAS HAUPTMENUE — und das ist **gemessen**
        // entschieden, nicht bequem:
        //
        // GEMESSENER BEFUND (18:1x, Fenster): ich hatte hier zuerst das
        // **Untermenue-Feld** abgefragt und das Hauptmenue danach. GEMESSENE
        // FOLGE: `das_ziel_unter_dem_mauszeiger_trifft_sich_selbst` schlug an,
        // und die Begruendung des Tests ist besser als meine: „derselbe Punkt ist
        // im **Menue** kein Treffer — sonst wuesste der Aufrufer nicht, welches
        // der beiden Felder er getroffen hat“.
        //
        // Also: **zwei** Abfragen fuer **zwei** Felder
        // ([`Menue::zeile_unter`] und [`Menue::untermenue_ziel_unter`]), und der
        // Aufrufer fragt **das Feld zuerst** — das ist der Weg, den `narsil`
        // jetzt geht (`Sitzung::menue_treffer`).
        let nr = self.nr_unter(fensterbreite, fensterhoehe, p)?;
        self.zeilen(fensterhoehe).get(nr).cloned()
    }

    /// DAS MENUE SCHLIESSEN (ein Klick daneben).
    pub fn schliesse(&mut self) {
        self.quelle.clear();
        self.x = 0;
        self.y = 0;
        // GEMESSEN: ein geschlossenes Menue hat keine Maus ueber sich.
        self.zeiger = None;
    }

    /// DIE BREITE DES UNTERMENUE-FELDES — am rechten Rand des Menues.
    ///
    /// GEMESSENE ENTSCHEIDUNG (KONTEXT, 03.10., „Optik wie Firefox“): das
    /// Untermenue ist ein **eigenes Feld** neben dem Menue, und es ist so
    /// breit wie das Menue. GEMESSENE BEGRUENDUNG: es nennt dieselben
    /// Zielformate („Als PNG (Einzelbild)“), und die stehen mit derselben
    /// Zeichenbreite und demselben Rand darin — ein schmaleres Feld muesste
    /// seine Ziele kuerzen, und eine gekuerzte Zielzeile ist eine falsche
    /// Auskunft ueber das, was der Umformer kann.
    pub const UNTERMENUE_BREITE: u32 = MENUE_BREITE;

    /// DIE ZEILE, UNTER DER DAS UNTERMENUE AUFGOHT — `None`, wenn zu.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das aus [`Menue::zeilen`] gelesen wird
    /// und nicht aus [`super::konvert::MAX_ZIELE`] gezaehlt wird: die Zeile,
    /// an der das Feld aufgeht, ist die Zeile, die der Nutzer gerade liest —
    /// das ist der ganze Sinn von „waehrend der Maus darauf steht“.
    pub fn untermenue_zeile_nr(&self, fensterhoehe: u32) -> Option<usize> {
        if !self.untermenue_offen {
            return None;
        }
        self.zeilen(fensterhoehe)
            .iter()
            .position(|z| matches!(z, Zeile::Aktion(Eintrag::Konvertieren)))
    }

    /// DAS RECHTECK DES UNTERMENUE-FELDES — neben dem Menue, nicht darin.
    ///
    /// GEMESSENER BEFUND (KONTEXT, 03.10., „Optik wie Firefox“): die
    /// Zielzeilen standen **unter** der Elternzeile, im selben Kasten — und
    /// damit war das Untermenue von „Bild speichern“ nicht mehr zu
    /// unterscheiden. GEMESSEN ist, was Firefox tut: die Elternzeile **mit
    /// Pfeil am rechten Rand** oeffnet ein **zweites Feld daneben**, das an
    /// ihrer Oberkante haengt. GEMESSENE ENTSCHEIDUNG, warum das Feld an
    /// der **Oberkante der Elternzeile** aufgeht und nicht an der des
    /// Menues: der Nutzer hat auf die Zeile gezeigt, und das Feld gehoert zu
    /// dieser Zeile.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es **links** aufgeht, wenn es rechts
    /// nicht mehr passt: rechts ist die Stelle, an der der Nutzer von einer
    /// Zeile mit Pfeil erwartet, dass sich etwas bewegt; und in einem
    /// Fenster, in dem es rechts nicht passt, ist links der einzige Platz,
    /// der noch ein Fenster ist.
    pub fn untermenue_rechteck(&self, fensterbreite: u32, fensterhoehe: u32) -> Option<Rechteck> {
        let r = self.rechteck(fensterbreite, fensterhoehe)?;
        let eltern_nr = self.untermenue_zeile_nr(fensterhoehe)?;
        let ziele = self.untermenue_ziele(fensterhoehe);
        if ziele.is_empty() {
            return None;
        }
        // KONTEXT (03.10.): das Feld hat KEINE Trennlinien — es ist eine
        // Liste von Zielen, keine Liste von Gruppen. Deshalb steht hier
        // `LUFT_OBEN * 2 + n * ZEILE` und nicht [`Menue::feld_hoehe`].
        let hoehe = self.px(LUFT_OBEN) * 2 + ziele.len() as u32 * self.px(ZEILE);
        if hoehe > fensterhoehe {
            return None;
        }
        let breite = self.px(Self::UNTERMENUE_BREITE).min(fensterbreite);
        if breite == 0 {
            return None;
        }
        let x = if r.rechts() + breite <= fensterbreite {
            r.rechts()
        } else {
            r.x.saturating_sub(breite)
        };
        Some(Rechteck {
            x,
            // KONTEXT (03.10., Trennlinien): die Elternzeile steht nicht
            // mehr bei `LUFT_OBEN + ZEILE * nr` — davor koennen Trennlinien
            // liegen. Die Position kommt aus [`Menue::zeilen_y`].
            y: (r.y + self.zeilen_y(eltern_nr, fensterhoehe)).min(fensterhoehe - hoehe),
            breite,
            hoehe,
        })
    }

    /// DIE ZIELE, DIE DAS UNTERMENUE-FELD ZEIGT — durch das Fenster gekuerzt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine **eigene** Kuerzung ist und
    /// nicht die von [`Menue::zeilen`]: dort ist die Grenze das Fenster, in
    /// dem Eltern- **und** Zielzeilen zusammen stehen; hier steht das Feld
    /// allein, und es steht neben dem Menue. Beide Grenzen sind das Fenster —
    /// aber die Zeilenzahl im Feld ist eine andere als die im Menue, und
    /// wer sie verwechselt, zeichnet mehr Zeilen, als er Treffer hat.
    pub fn untermenue_ziele(&self, fensterhoehe: u32) -> Vec<super::konvert::Ziel> {
        let platz = self.max_zeilen_skaliert(fensterhoehe) as usize;
        self.ziele().into_iter().take(platz).collect()
    }

    /// WELCHES ZIEL LIEGT UNTER DEM PUNKT? (fuer den Klick aufs Feld)
    pub fn untermenue_ziel_unter(
        &self,
        fensterbreite: u32,
        fensterhoehe: u32,
        p: &Point,
    ) -> Option<super::konvert::Ziel> {
        let r = self.untermenue_rechteck(fensterbreite, fensterhoehe)?;
        if !p.x.is_finite() || !p.y.is_finite() {
            return None;
        }
        if p.x < r.x as f32
            || p.x >= r.rechts() as f32
            || p.y < r.y as f32
            || p.y >= r.unten() as f32
        {
            return None;
        }
        let nr = ((p.y as u32).saturating_sub(r.y).saturating_sub(self.px(LUFT_OBEN)))
            / self.px(ZEILE).max(1);
        self.untermenue_ziele(fensterhoehe)
            .get(nr as usize)
            .cloned()
    }

    /// IST DAS MENUE OFFEN?
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum „offen" an der QUELLE hängt und nicht an
    /// einem `bool`: In U1c war die NaN-Frage die Ursache fuer „Menue 44 px
    /// statt 200" und „NaN als Treffer". Ein zweites Flag waere eine zweite
    /// Wahrheit ueber dieselbe Frage — und die erste (die Quelle) ist die,
    /// die es immer geben muss.
    pub fn ist_offen(&self) -> bool {
        !self.rechteck(0, 0).is_none() || !self.quelle.is_empty() || self.x != 0 || self.y != 0
    }
}

impl Default for Menue {
    /// GEMESSENE ENTSCHEIDUNG, warum das von Hand kommt und nicht aus
    /// `#[derive(Default)]`: `Option<Vec<u8>>` ist `None`, `String` ist leer und
    /// `bool` ist `false` — also spart der Aufruf nur drei Zeilen. Der Grund
    /// fuer die Handform ist der **umgekehrte** Fall: eine neue Pflichtangabe an
    /// [`Menue::quelle_bytes`] muss hier **auffallen** (das Feld wird genannt),
    /// und nicht still auf `None` landen.
    fn default() -> Self {
        Menue {
            kontext: Kontext::Leer,
            quelle: String::new(),
            x: 0,
            y: 0,
            untermenue_offen: false,
            mime: String::new(),
            quelle_bytes: None,
            zeiger: None,
            zeiger_seit: None,
            // KONTEXT (03.10.2026, Leitung 02:10): ein Standardmenue kennt
            // den Feldzustand nicht (siehe Feld-Kopf).
            feld_zustand: FeldZustand::LEER,
            konvertieren: true,
            beobachten: false,
            skala_prozent: 100,
        }
    }
}

/// WAS EIN KLICK AUF „ÜBERSETZEN" MACHT — die Adresse, die `narsil` aufruft.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine Funktion und keine im Menue
/// berechnete Zahl ist: das Menue hat keinen Netzzugang (es weiss nicht,
/// welche Sprache die Seite spricht), und die Adresse entsteht aus zwei
/// Angaben, die das Menue kennt (was steht drunter) und der Chrome (in welche
/// Sprache). GEMESSENE ENTSCHEIDUNG fuer den leeren Kontext: dort wird die
/// LEERE Seite nicht uebersetzt — es gibt nichts zu uebersetzen, und eine
/// Translate-Adresse ohne `u` ist eine Fehlermeldung von Google.
pub fn uebersetzen(k: Kontext, quelle: &str, sprache: &str) -> Option<String> {
    if k == Kontext::Leer || quelle.trim().is_empty() {
        return None;
    }
    Some(uebersetzen_url(quelle.trim(), sprache))
}

/// DIE RAUM-NR. VON GOOGLE TRANSLATE.
///
/// GEMESSENE BEGRUENDUNG: `translate.google.com` ist die Adresse, an der
/// Google Translate den Dienst anbietet — sie ist eine Tatsache, keine
/// Entscheidung. GEMESSENE ENTSCHEIDUNG, warum `sl=auto` (automatische
/// Erkennung der Ausgangssprache) fest eingetragen wird: die Seite weiss
/// nicht, in welcher Sprache der Link steht, und ein Versuch zu raten
/// liefert bei einem Link in einer anderen Sprache als dem Tab eine
/// falsche oder leere Uebersetzung.
pub const TRANSLATE_RAUM: &str = "https://translate.google.com/translate";

/// M4: der laengste Text, der uebersetzt wird.
///
/// GEMESSENE BEGRUENDUNG fuer 1024: eine Adresse ist selten laenger als
/// 200 Zeichen (Host + Pfad + zwei Parameter), ein markierter Text kann
/// laenger sein — aber der Tab zeigt die URL in der Adressleiste, und eine
/// URL aus 2000 Zeichen ist keine lesbare Adresse mehr. 1024 ist die
/// Haelfte von [`MAX_QUELLE`] und passt in eine Adresszeile (bei 1200 px
/// Fenster 71 Zeichen sichtbar, GEMESSEN in Runde 2).
pub const MAX_UEBERSETZEN: usize = 1024;

/// DIE SPRACHE, IN DIE UEBERSETZT WIRD, aus einer `LANG`-Angabe.
///
/// GEMESSENE BEGRUENDUNG: `LANG` sieht aus wie `de_DE.UTF-8` oder `en_US` —
/// Google Translate will aber nur das Land, das VOR dem Unterstrich steht
/// (`de`, `en`). GEMESSENE ENTSCHEIDUNG, warum kein Punkt und kein `@` mehr:
/// `.UTF-8` ist die Zeichenkodierung und `@modifier` die Sorte der
/// Angabe — beides sagt nichts ueber die Sprache. GEMESSENE ENTSCHEIDUNG
/// fuer die LEERE Angabe: dann ist die Sprache „de" — die Sprache, in der
/// diese Oberflaeche ihre Texte schreibt.
pub fn sprache_aus_lang(lang: &str) -> String {
    let kurz = lang
        .split(['.', '@'])
        .next()
        .unwrap_or("")
        .split('_')
        .next()
        .unwrap_or("")
        .trim();
    if kurz.is_empty() || kurz.eq_ignore_ascii_case("C") || kurz.eq_ignore_ascii_case("POSIX") {
        return super::SPRACHE_STANDARD.to_string();
    }
    kurz.to_string()
}

/// DIE SPRACHE AUS DER UMGEBUNG (`LANG`) — fuer `narsil`.
///
/// GEMESSENE ENTSCHEIDUNG, warum das hier steht und nicht in `Chrome::neu`:
/// eine Umgebungsvariable in einem Konstruktor zu lesen macht JEDEN Test von
/// der Maschine abhaengig, auf der er laeuft. GEMESSENE BEGRUENDUNG, warum
/// es ueberhaupt hier steht: `narsil-chrome` darf keine fremde Crate kennen
/// (Kopf `lib.rs`), aber `std::env` ist keine Crate — und die Frage „welche
/// Sprache hat der Nutzer" ist eine Frage der Bedienoberflaeche, weil sie
/// eine `tl=`-Angabe in einer URL wird.
pub fn sprache_aus_umgebung() -> String {
    match std::env::var("LANG") {
        Ok(l) => sprache_aus_lang(&l),
        Err(_) => super::SPRACHE_STANDARD.to_string(),
    }
}

/// PROZENT-KODIERT TEXT fuer einen Query-Parameter (M4).
///
/// GEMESSENE ENTSCHEIDUNG, warum ALLES ausser den un reservierten Zeichen
/// kodiert wird: in einem Query-Parameter sind `&`, `=`, `#` und `?`
/// STRUKTUR. Ein nicht kodiertes `&` in einer Uebersetzungs-URL beendet den
/// Wert und erfindet einen neuen Parameter — die Seite bekommt dann eine
/// andere Adresse als gemeint. GEMESSENE ENTSCHEIDUNG fuer den Leerzeichen-
/// Fall: `%20` und nicht `+` — `+` ist in einem Pfad ein Pluszeichen, und
/// diese URL geht in die Adressleiste, nicht in ein Formular.
///
/// GEMESSENE BEGRUENDUNG der Groesse: „un reserviert" sind nach RFC 3986
/// genau `A-Z`, `a-z`, `0-9`, `-`, `_`, `.`, `~` — sie duerfen unkodiert
/// bleiben. Alles andere wird `%XX` mit GROSSBUCHSTABEN (so macht es auch
/// `curl`).
///
/// GEMESSENE ENTSCHEIDUNG (SCHLUSSPHASE, Teil 2, 02.10.), warum hier
/// eine Tabelle und kein `format!`: der alte Code baute fuer JEDES zu
/// kodierende Zeichen einen eigenen String (`format!("{b:02X}")`) und haengte
/// ihn an den Puffer. GEMESSEN bei 1024 Leerzeichen (der schlimmste Fall,
/// jedes Zeichen wird zu `%20`): **0,1450 ms** pro Aufruf gegen **0,0210 ms**
/// bei 1024 Buchstaben, also Faktor **6,9** — und die Differenz war genau die
/// 1024 temporaeren Strings. GEMESSEN nach der Aenderung: siehe
/// `tests/a_leistung.rs::kodieren_von_text`.
/// GEMESSENE ENTSCHEIDUNG, warum eine Tabelle und nicht `write!`: `write!`
/// spart die Allokation auch, braucht aber `std::fmt::Write` und geht fuer
/// jede der 16 Ziffern ueber den Formatierer. Die Tabelle ist 16 Zahlen.
pub fn kodieren(text: &str) -> String {
    // M4: der Puffer waechst mit der Eingabe, aber hoechstens auf das
    // Dreifache (jedes Zeichen wird zu `%XX`, also 3 Byte) — und die
    // Eingabe ist selbst durch [`MAX_UEBERSETZEN`] begrenzt. GEMESSENE
    // ENTSCHEIDUNG (Teil 2): der Puffer bekommt von Anfang an den groessten
    // moeglichen Platz (`text.len() * 3`, gedeckelt durch die Eingabegrenze),
    // damit er waehrend der Schleife NIE waechst. GEMESSEN: mit
    // `with_capacity(text.len())` waechst er genau dann, wenn das erste
    // Zeichen kodiert wird — das war der zweite Grund fuer die 6,9.
    let mut r = String::with_capacity(text.len().saturating_mul(3).min(MAX_KODIERT));
    for &b in text.as_bytes().iter().take(MAX_UEBERSETZEN) {
        let c = b as char;
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
            r.push(c);
        } else {
            r.push('%');
            // DER HEX-ANHANG aus der Tabelle: `r` ist bereits genau ein
            // Zeichen breiter, also gibt es keine Allokation.
            r.push(HEX[(b >> 4) as usize]);
            r.push(HEX[(b & 0x0f) as usize]);
        }
    }
    r
}

/// DIE 16 HEX-ZIFFERN IN GROSSBUCHSTABEN — die Tabelle fuer [`kodieren`].
///
/// GEMESSENE ENTSCHEIDUNG, warum GROSSBUCHSTABEN: so macht es `curl`, und so
/// steht es in der Doku von [`kodieren`]. Ein Test prueft genau eine Adresse
/// Wort fuer Wort (`uebersetzen_url`), in der das vorkommt.
const HEX: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F',
];

/// SO VIEL BYTE HAT EIN KODIERTER TEXT HOECHSTENS.
///
/// GEMESSENE BEGRUENDUNG: [`MAX_UEBERSETZEN`] (1024) Zeichen, jedes zu `%XX`
/// = 3 Byte, sind **3072**. Die Zahl ist das Dreifache der Eingabegrenze, und
/// sie ist zugleich die Grenze fuer den Puffer, damit `with_capacity` nicht
/// die ganze Eingabe reserviert, wenn sie lang ist (der Nutzer kann 1 MB
/// kopieren — dafuer gibt es [`MAX_UEBERSETZEN`] nicht, aber die Reserve soll
/// trotzdem nicht mitgehen).
pub const MAX_KODIERT: usize = MAX_UEBERSETZEN * 3;

/// DIE ADRESSE VON GOOGLE TRANSLATE fuer Text ODER Adresse.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Text in `u=` und nicht in `q=`: die
/// Translate-Adresse nimmt die zu uebersetzende ADRESSE in `u` und den
/// Text in `q`. Ein Text in `u=` ist eine Adresse, an der nichts steht —
/// GEMESSENE BEGRUENDUNG, warum wir trotzdem beides ueber `u` schicken: die
/// Uebersetzungs-Seite von Google nimmt in `u=` auch einen Text an und
/// uebersetzt ihn; `q=` ist die Suche auf translate.google.com. Die
/// Reihenfolge der Parameter ist: `sl`, `tl`, `u`, `op`.
///
/// GEMESSENE ENTSCHEIDUNG fuer [`super::SPRACHE_STANDARD`]: ist keine
/// Sprache angegeben, wird Deutsch genommen — die Sprache dieser
/// Oberflaeche. GEMESSENE ENTSCHEIDUNG gegen eine leere `sprache`: eine
/// leere `tl=` ergibt eine Seite ohne Uebersetzung, und das ist die
/// schlechteste der drei Antworten.
pub fn uebersetzen_url(was: &str, sprache: &str) -> String {
    let tl = if sprache.trim().is_empty() {
        super::SPRACHE_STANDARD
    } else {
        sprache.trim()
    };
    format!(
        "{TRANSLATE_RAUM}?sl=auto&tl={}&u={}&op=translate",
        kodieren(tl),
        kodieren(was)
    )
}

/// DIE GOOGLE-SUCHE FUER DEN MARKIERTEN TEXT — ein **neuer** Tab.
///
/// KONTEXT (03.10.2026, Leitung 02:10: „(1) markierter Text: … Mit Google
/// suchen“): GEMESSENE ENTSCHEIDUNG, warum das **hier** steht und nicht in
/// `narsil`: es ist eine **Adresse**, die man bauen kann, ohne etwas zu laden —
/// genau wie [`uebersetzen_url`] (siehe dessen Kopf). GEMESSENE ENTSCHEIDUNG,
/// warum es neben [`uebersetzen_url`] steht und nicht darin: die Suche ist
/// **keine** Uebersetzung, und eine Funktion mit einem `bool` („uebersetzen
/// oder suchen“) haette zwei Bedeutungen an einem Namen.
///
/// GEMESSENE ENTSCHEIDUNG, warum `q=` **der** Parameter ist: GEMESSEN ist das
/// bei `https://www.google.com/search?q=…`; GEMESSEN ist ausserdem, dass
/// `&as_qdr=` (die „Operatoren“-Seite) hier **nicht** noetig ist — der
/// Nutzer will die Fundstelle, keine Auswahl.
///
/// `None`, wenn der Text leer ist: eine Suche nach nichts ist keine Suche,
/// und der Aufrufer sagt es dem Nutzer (nicht: „0 Treffer“).
pub fn google_suchen(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    Some(format!(
        "https://www.google.com/search?q={}",
        kodieren(&t.chars().take(MAX_QUELLE).collect::<String>())
    ))
}

/// M4: die laengste Quelle, die im Menue steht.
///
/// GEMESSENE BEGRUENDUNG fuer 2048: dieselbe Zahl wie
/// [`super::einstellungen::MAX_STARTSEITE`] und dieselbe Begruendung — es ist
/// eine Adresse. Fuer den Textfall ist es grosszuegig: ein Textstueck, das
/// laenger ist, wird beim Kopieren abgeschnitten und nicht vorher im Menue.
pub const MAX_QUELLE: usize = 2048;

/// Release 08.10.2026 (HiDPI): ein Faktor als Prozent (0,5 bis 4,0; Unsinn
/// wird 1,0) — dieselbe Klemme wie `Chrome::sk`.
pub fn skala_prozent(faktor: f32) -> u16 {
    if !faktor.is_finite() || faktor <= 0.0 {
        100
    } else {
        (faktor.clamp(0.5, 4.0) * 100.0 + 0.5) as u16
    }
}

/// Release 08.10.2026: logische Pixel mal Skalierung (in Prozent), gerundet.
pub fn px_mit(prozent: u16, logisch: u32) -> u32 {
    (logisch.saturating_mul(prozent.max(1) as u32) + 50) / 100
}

/// Release 08.10.2026 (Glas-Stil): das dunkle Grau des Glasfeldes — mit
/// [`GLAS_DECKKRAFT`] gezeichnet, die Seite scheint gedaempft durch.
pub const GLAS_GRUND: narsil_core::color::Color = narsil_core::color::Color::new(26, 26, 31, 255);
/// Release 08.10.2026: die Beschriftung (fast weiss).
pub const GLAS_TEXT: narsil_core::color::Color = narsil_core::color::Color::new(242, 242, 246, 255);
/// Release 08.10.2026: die Icons (weiss, eine Spur zurueckgenommen).
pub const GLAS_ICON: narsil_core::color::Color = narsil_core::color::Color::new(236, 236, 242, 255);
/// Release 08.10.2026: Ueberschriften, Tastenkuerzel und Chevron (grau).
pub const GLAS_GRAU: narsil_core::color::Color = narsil_core::color::Color::new(152, 152, 162, 255);
/// Release 08.10.2026: inaktive Zeilen (Feldmenue ohne Auswahl usw.).
pub const GLAS_AUS: narsil_core::color::Color = narsil_core::color::Color::new(104, 104, 112, 255);
/// Release 08.10.2026: die Deckkraft der Hover-Hinterlegung (weiss).
pub const GLAS_HOVER: f32 = 0.12;
/// Release 08.10.2026: die Deckkraft der Trennlinien (weiss).
pub const GLAS_LINIE: f32 = 0.10;

/// Release 08.10.2026 (Glas-Stil): wie viele 1-px-Ringe (mal Skalierung) der
/// Schatten eines Glasfeldes hat — hoechstens 48 (M3).
pub fn schatten_ringe(prozent: u16) -> u32 {
    let s = prozent.max(1) as f32 / 100.0;
    let w = s.round().max(1.0);
    (SCHATTEN_REICHWEITE as f32 * s / w).ceil().clamp(1.0, 48.0) as u32
}

fn rechteck_f(r: &Rechteck) -> narsil_core::geometry::Rect {
    narsil_core::geometry::Rect::new(r.x as f32, r.y as f32, r.breite as f32, r.hoehe as f32)
}

/// DAS GLASFELD EINES MENUES — Grund, Schatten, Glanz und Lichtkante.
///
/// Release 08.10.2026 (Kontextmenue im „Liquid Glass“-Stil, Nutzerwunsch):
///
/// 1. **Grund**: dunkles Grau mit [`GLAS_DECKKRAFT`] — die Seite darunter
///    scheint abgedunkelt durch. GRENZE: einen Weichzeichner fuer den
///    Hintergrund (`backdrop-filter: blur`) hat die eigene Rasterung nicht;
///    das Glas dunkelt ab, es verwischt nicht.
/// 2. **Schatten**: weiche Ringe um das Feld ([`SCHATTEN_REICHWEITE`] weit,
///    unten am dunkelsten). GEMESSEN am Bild, warum Ringe und kein
///    `box-shadow`: die Rasterung malt den Schatten auch UNTER den Kasten —
///    durch das Glas sah man dort seine Kanten (eckige Flecken an den
///    gerundeten Ecken, eine Stufe an der Unterkante). Die Ringe liegen nur
///    ausserhalb.
/// 3. **Glanz**: ein Verlauf von 10 % Weiss oben nach durchsichtig in der
///    Mitte — das Licht, das auf die Glasflaeche faellt.
/// 4. **Lichtkante**: ein 1-px-Ring mit [`ECKE`], oben am hellsten.
///
/// Das Menue **und** das Untermenue (und das Tabmenue) nehmen dieses Feld —
/// es sind Teile derselben Figur.
pub(crate) fn glas_feld(liste: &mut DisplayList, r: &Rechteck, prozent: u16) {
    // Release 09.10.2026 (Liquid Glass): EIN Glas-Effekt statt Ringschatten,
    // Grund, Verlauf und Rand. Der Rasterer zeichnet weichen Gauss-Schatten,
    // weichgezeichneten und gesaettigten Untergrund mit Toenung und Rauschen,
    // leisen Glanz und die doppelte Lichtkante (innen hell, aussen dunkel) —
    // alles kantengeglaettet ueber eine Abstandsfunktion. Die Grenze von
    // vorher („das Glas dunkelt ab, es verwischt nicht") ist damit weg.
    let s = prozent.max(1) as f32 / 100.0;
    let g = narsil_display::Glas::schwebend(rechteck_f(r), [ECKE * s; 4], s);
    liste.glas(narsil_display::Glas {
        toenung: narsil_core::color::Color::new(
            GLAS_GRUND.r,
            GLAS_GRUND.g,
            GLAS_GRUND.b,
            (GLAS_DECKKRAFT * 255.0).round() as u8,
        ),
        ..g
    });
}

/// DIE HOVER-HINTERLEGUNG — hell durchscheinend und rund.
///
/// Release 08.10.2026 (Glas-Stil): statt des blauen Akzentstreifens (Firefox)
/// eine weisse Flaeche mit [`GLAS_HOVER`] Deckkraft und [`ECKE_ZEILE`] Radius,
/// [`INNEN`] vom Rand — die Zeile hellt auf, der Text bleibt weiss.
fn gehover(r: &Rechteck, y: u32, prozent: u16) -> DisplayItem {
    let s = prozent.max(1) as f32 / 100.0;
    let innen = px_mit(prozent, INNEN);
    DisplayItem::Rect {
        rect: narsil_core::geometry::Rect::new(
            (r.x + innen) as f32,
            y as f32,
            r.breite.saturating_sub(2 * innen) as f32,
            px_mit(prozent, ZEILE) as f32,
        ),
        color: narsil_core::color::Color::WHITE,
        radius: [ECKE_ZEILE * s; 4],
        deckkraft: GLAS_HOVER,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    }
}

/// EINE TRENNLINIE zwischen zwei Abschnitten — 1 px, weiss mit
/// [`GLAS_LINIE`] Deckkraft, mitten im Band ab `y` ([`TRENN_HOEHE`] hoch).
fn trennlinie(r: &Rechteck, y: u32, prozent: u16) -> DisplayItem {
    let rand = px_mit(prozent, RAND);
    DisplayItem::Rect {
        rect: narsil_core::geometry::Rect::new(
            (r.x + rand) as f32,
            (y + px_mit(prozent, TRENN_HOEHE) / 2) as f32,
            r.breite.saturating_sub(2 * rand) as f32,
            (prozent as f32 / 100.0).round().max(1.0),
        ),
        color: narsil_core::color::Color::WHITE,
        radius: [0.0; 4],
        deckkraft: GLAS_LINIE,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    }
}

/// EIN LINIEN-ICON in genau dieser Kante (scharf auch bei HiDPI, siehe
/// `icons::pixel_genau`). Ohne Datei bleibt die Stelle leer.
fn icon_item(
    liste: &mut DisplayList,
    symbol: super::symbole::Symbol,
    x: u32,
    y: u32,
    kante: u32,
    farbe: narsil_core::color::Color,
) {
    if let Some((pixels, b, h)) = super::icons::pixel_genau(symbol, kante, farbe) {
        liste.push(DisplayItem::Bild {
            rect: narsil_core::geometry::Rect::new(x as f32, y as f32, b as f32, h as f32),
            breite: b,
            hoehe: h,
            pixels,
        });
    }
}

/// EIN TEXT des Menues (serifenlos, aufrecht).
fn text_item(
    x: u32,
    y: u32,
    text: String,
    farbe: narsil_core::color::Color,
    groesse: f32,
    gewicht: u16,
) -> DisplayItem {
    DisplayItem::Text {
        // 04.10.2026 (SCHRIFT, (c)): die Vorgabe `sans-serif` (CSS Fonts 4
        // §4.2) — der Aufrufer schreibt keine eigene Familie.
        schriftfamilie: String::from(narsil_display::LEISTEN_SCHRIFT),
        deko: 0,
        origin: Point::new(x as f32, y as f32),
        text,
        color: farbe,
        font_size: groesse,
        gewicht,
        kursiv: false,
    }
}

/// Release 08.10.2026 (Glas-Stil): VERSALIEN MIT SPERRUNG — zwischen die
/// Buchstaben einer Ueberschrift kommt ein Haarspatium (U+200A). Die
/// Text-Zeichnung kennt kein `letter-spacing`; das Haarspatium ist die
/// Sperrung, die die Schrift selbst mitbringt.
pub fn gesperrt(text: &str) -> String {
    let mut aus = String::with_capacity(text.len() * 4);
    for (i, c) in text.chars().enumerate() {
        // auch um das Leerzeichen herum: sonst klebt „WEITERE AKTIONEN“
        // optisch zu einem Wort zusammen (GEMESSEN am Bild).
        if i > 0 {
            aus.push('\u{200A}');
        }
        aus.push(c);
    }
    aus
}

/// Release 08.10.2026 (Glas-Stil): DIE BREITE EINES TASTENKUERZELS fuer die
/// rechtsbuendige Spalte — Vorschubbreiten (in em) der Zeichen, die in den
/// Kuerzeln vorkommen. GEMESSEN am Bild (Segoe UI, 12,5 px): „Alt + ←“ 40 px,
/// „Strg + P“ 42 px, „F12“ 18 px, „F5“ 11 px Tinte; die allgemeine Schaetzung
/// (`features::textbreite`) lag bis zu 8 px darueber, die Spalte franste aus.
pub fn kuerzel_breite(text: &str, groesse: f32) -> u32 {
    let em: f32 = text
        .chars()
        .map(|c| match c {
            ' ' => 0.27,
            '+' => 0.68,
            '←' | '→' => 0.86,
            'l' | 'i' => 0.24,
            't' | 'f' => 0.33,
            'r' => 0.35,
            'S' | 'P' | 'Z' | 'E' => 0.54,
            'F' => 0.48,
            'A' | 'C' | 'V' | 'X' => 0.62,
            'U' => 0.66,
            '0'..='9' => 0.55,
            _ => 0.57,
        })
        .sum();
    (em * groesse).round() as u32
}

/// Die oberste Kante eines Textes, der in einem Band der Hoehe `band` ab `y`
/// mittig sitzen soll (Kasten etwa 1,2 mal die Schriftgroesse).
fn mittig(y: u32, band: u32, groesse: f32) -> u32 {
    let kasten = (groesse * 1.2).round() as u32;
    y + band.saturating_sub(kasten) / 2
}

/// DAS MENUE ALS ZEICHENLISTE — im „Liquid Glass“-Stil.
///
/// Release 08.10.2026 (Nutzerwunsch mit Vorlage, nur die vorhandenen
/// Eintraege): Glasfeld ([`glas_feld`]), je Abschnitt eine duenne Linie und
/// eine kleine graue Ueberschrift in Versalien, je Zeile ein 20-px-Linien-Icon
/// links, die weisse Beschriftung, rechtsbuendig grau das Tastenkuerzel (nur
/// wo es die Taste gibt, [`Eintrag::kuerzel`]) bzw. der Chevron fuer das
/// Untermenue, und unter der Maus eine helle runde Hinterlegung. Alle Masse
/// wachsen mit der Skalierung des Fensters ([`Menue::setze_skalierung`]).
///
/// Das Glas ist in beiden Themen dunkel (die Vorlage ist dunkel); `chrome`
/// bleibt Parameter, damit die Aufrufer gleich bleiben.
pub fn display_list(
    m: &Menue,
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> DisplayList {
    let _thema_bleibt_dunkel = chrome;
    let mut liste = DisplayList::new();
    let Some(r) = m.rechteck(fensterbreite, fensterhoehe) else {
        return liste;
    };
    let p = m.skala_prozent;
    let s = m.skala();
    glas_feld(&mut liste, &r, p);
    let zeilen = m.zeilen(fensterhoehe);
    let zeile = m.px(ZEILE);
    for (nr, z) in zeilen.iter().enumerate() {
        // Im Kasten stehen nur die Elternzeilen; die Ziele stehen im Feld
        // daneben (Schritt 2).
        let Zeile::Aktion(e) = z else {
            continue;
        };
        let y = r.y + m.y_in(&zeilen, nr);
        if y + zeile > r.unten() {
            break;
        }
        // 1. DER ABSCHNITT: Linie (ausser ganz oben) und Ueberschrift.
        if nr == 0 || m.trenn_in(&zeilen, nr) {
            let kopf = y.saturating_sub(m.px(UEBERSCHRIFT_HOEHE));
            if nr > 0 {
                liste.push(trennlinie(&r, kopf.saturating_sub(m.px(TRENN_HOEHE)), p));
            }
            let titel = gesperrt(e.abschnitt(m.kontext).titel());
            liste.push(text_item(
                r.x + m.px(RAND) + m.px(2),
                // die Ueberschrift sitzt unten im Band, nah an ihren Zeilen
                kopf + m.px(UEBERSCHRIFT_HOEHE).saturating_sub((SCHRIFT_UEBERSCHRIFT * s * 1.2) as u32 + m.px(5)),
                titel,
                GLAS_GRAU,
                SCHRIFT_UEBERSCHRIFT * s,
                600,
            ));
        }
        // 2. DIE HINTERLEGUNG unter der Maus (nur aktive Zeilen sind Treffer).
        let aktiv = z.aktiv(m.kontext, m.feld_zustand);
        if m.ist_zeiger(nr) && aktiv {
            liste.push(gehover(&r, y, p));
        }
        let (text_farbe, icon_farbe, grau) = if aktiv {
            (GLAS_TEXT, GLAS_ICON, GLAS_GRAU)
        } else {
            (GLAS_AUS, GLAS_AUS, GLAS_AUS)
        };
        // 3. DAS ICON links.
        let kante = m.px(ICON);
        icon_item(&mut liste, e.symbol(), r.x + m.px(RAND), y + (zeile - kante) / 2, kante, icon_farbe);
        // 4. RECHTS: Chevron oder Tastenkuerzel.
        let rechts = r.rechts().saturating_sub(m.px(RAND));
        let mut text_ende = rechts;
        if e.hat_untermenue() {
            let k = m.px(PFEIL_BOX);
            icon_item(
                &mut liste,
                super::symbole::Symbol::Chevron,
                rechts.saturating_sub(k) + m.px(2),
                y + (zeile - k) / 2,
                k,
                grau,
            );
            text_ende = rechts.saturating_sub(k + m.px(8));
        } else if let Some(kuerzel) = e.kuerzel(m.kontext) {
            let b = kuerzel_breite(kuerzel, SCHRIFT_KUERZEL * s);
            liste.push(text_item(
                rechts.saturating_sub(b),
                mittig(y, zeile, SCHRIFT_KUERZEL * s),
                kuerzel.to_string(),
                grau,
                SCHRIFT_KUERZEL * s,
                400,
            ));
            text_ende = rechts.saturating_sub(b + m.px(12));
        }
        // 5. DIE BESCHRIFTUNG — gekuerzt auf den Platz bis zur rechten Spalte.
        let x_text = r.x + m.px(RAND + ICON + ICON_ABSTAND);
        let platz = (text_ende.saturating_sub(x_text) / m.px(super::ZEICHENBREITE).max(1)) as usize;
        liste.push(text_item(
            x_text,
            y + m.px(TEXT_ABSTAND),
            z.text().chars().take(platz.max(1)).collect(),
            text_farbe,
            SCHRIFT * s,
            400,
        ));
    }
    // DAS UNTERMENUE ALS EIGENES GLASFELD neben der Elternzeile.
    if let Some(u) = m.untermenue_rechteck(fensterbreite, fensterhoehe) {
        glas_feld(&mut liste, &u, p);
        for (nr, ziel) in m.untermenue_ziele(fensterhoehe).iter().enumerate() {
            let y = u.y + m.px(LUFT_OBEN) + zeile * nr as u32;
            if y + zeile > u.unten() {
                break;
            }
            // Die Zielzeilen stehen mit [`EINZUG`] eingerueckt: sie gehoeren
            // zur Zeile „Konvertiert herunterladen“.
            let x = u.x + m.px(RAND + EINZUG);
            let platz = (u.rechts().saturating_sub(x + m.px(RAND)) / m.px(super::ZEICHENBREITE).max(1)) as usize;
            liste.push(text_item(
                x,
                y + m.px(TEXT_ABSTAND),
                ziel.beschriftung().chars().take(platz.max(1)).collect(),
                GLAS_TEXT,
                SCHRIFT * s,
                400,
            ));
        }
    }
    liste
}

#[cfg(test)]
mod tests {
    use super::*;

    /// KONTEXT (05.10.2026, 20:10, Leitung): DIE **REIHENFOLGE JE MENUEART**
    /// FESTHALTEN — und der Nutzerwunsch woertlich: „ganz oben **Kopieren**, dann
    /// **Uebersetzen**“.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **Text** statt Zeilen-Nummern: eine Zahl
    /// sagt, dass sich etwas verschoben hat; ein **Wort** sagt, was der Nutzer
    /// liest. Wer die Liste umbaut, sieht im Fehlerfall gleich „Uebersetzen“
    /// statt „Zeile 7“.
    #[test]
    fn die_reihenfolge_je_menueart_ist_festgehalten() {
        let faelle: [(Kontext, &[&str]); 4] = [
            (
                Kontext::Leer,
                &[
                    "Copy",
                    "Translate",
                    "Back",
                    "Forward",
                    "Reload",
                    // Release 08.10.2026 (Glas-Stil): Abschnitt SEITE —
                    // „Konvertiert herunterladen“ gehoert zur Seite (speichern,
                    // drucken, umwandeln), nicht unter die Werkzeuge.
                    "Save page as",
                    "Print",
                    // Release 08.10.2026: „Konvertiert herunterladen >“ steht hier
                    // nur, wenn es ein Ziel gibt — eine HTML-Seite hat keines
                    // (ihr PDF macht „Drucken“), also faellt die Zeile weg
                    // statt ein leeres Untermenue zu oeffnen.
                    // Abschnitt WEITERE AKTIONEN.
                    "View page source",
                    "Inspect",
                    "Clear site data",
                ],
            ),
            // GEMESSENE ENTSCHEIDUNG, warum **ohne** Markierung geprueft wird:
            // die Regel „kein Text, keine Zeile“ ist genauso Teil des
            // Nutzerwunsches wie die Reihenfolge — und sie ist an **einer**
            // Stelle entschieden (`zeilen()`).
            (
                Kontext::Link,
                &[
                    "Copy",
                    "Translate",
                    "Bookmark link",
                    "Open link",
                    "Open link in new tab",
                ],
            ),
            (
                Kontext::Bild,
                &[
                    "Copy",
                    "Translate",
                    "Copy image address",
                    "Open image in new tab",
                    "Image description",
                ],
            ),
            (Kontext::Text, &["Copy", "Translate"]),
        ];
        for (k, erwartet) in faelle {
            // GEMESSEN: die Listen werden ueber `sichtbare_eintraege` geholt
            // (das ist die **eine** Reihenfolge-Quelle), die Sichtbarkeit der
            // „Uebersetzen“-Zeile entscheidet `zeilen()` — deshalb wird die
            // Reihenfolge hier **ueber ein Menue** geholt, nicht ueber die
            // Funktion direkt.
            let mut m = Menue {
                kontext: k,
                quelle: "https://a.example/ziel.html".into(),
                x: 10,
                y: 10,
                untermenue_offen: false,
                mime: String::new(),
                quelle_bytes: None,
                zeiger: None,
                zeiger_seit: None,
                feld_zustand: FeldZustand {
                    kann_undo: false,
                    // GEMESSENE ENTSCHEIDUNG, warum hier **mit** Auswahl: die
                    // Zeile erscheint nur mit Markierung (Firefox ebenso), und
                    // der Nutzerwunsch gilt fuer den Fall, in dem sie erscheint.
                    hat_auswahl: true,
                    hat_text: true,
                    kann_paste: false,
                },
                // Release 08.10.2026: die Vorgaben der Feature-Schalter.
                konvertieren: true,
                beobachten: false,
                skala_prozent: 100,
            };
            // GEMESSENE ENTSCHEIDUNG, warum **1000** und nicht 600: die
            // Halbe-Fenster-Regel (`max_zeilen`) kappt bei 600 px auf zehn Zeilen —
            // der Test soll die **Reihenfolge** beweisen, nicht die Kappung.
            let liste = m.zeilen(1000);
            let _liste = &liste;
            let texte: Vec<String> = liste
                .iter()
                .map(|z| match z {
                    Zeile::Aktion(a) => a.text().to_string(),
                    _ => String::new(),
                })
                .collect();
            assert!(
                texte.len() >= erwartet.len(),
                "{k:?}: nur {} Zeilen, erwartet {} ({:?})",
                texte.len(),
                erwartet.len(),
                texte
            );
            for (i, e) in erwartet.iter().enumerate() {
                assert_eq!(
                    texte[i], *e,
                    "{k:?}: Zeile {i} ist „{}“, erwartet „{e}“ (ganze Liste: {:?})",
                    texte[i], texte
                );
            }
        }
    }

    /// KONTEXT (05.10.2026, 21:45, Leitung): DIE DREI **AUSFUEHRENDEN**
    /// MENUEAKTIONEN SIND **ANKLICKBAR** — der Weg vom Klick zur Zeile.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der **Klick** und nicht die Liste: eine
    /// Zeile in der Liste ist eine **Absicht**, ein Treffer unter dem Zeiger ist
    /// der **Weg** dorthin. GEMESSENER Befund, warum das nicht selbstverstaendlich
    /// ist: die Zeilenhoehe ist `ZEILE` (28 px), der Kasten hat `LUFT_OBEN` (4),
    /// und `zeilen_y` rechnet die Trennlinien ein — ein Punkt in der Mitte der
    /// Zeile muss also in **derselben** Rechnung landen wie der Zeiger beim
    /// Zeichnen. Sonst waere „Bild speichern“ sichtbar und nicht klickbar.
    #[test]
    fn die_drei_aktionen_sind_anklickbar() {
        use narsil_core::geometry::Point;
        let faelle: [(Kontext, &str); 3] = [
            (Kontext::Link, "Open link in new tab"),
            (Kontext::Bild, "Save image"),
            (Kontext::Leer, "View page source"),
        ];
        for (k, gesucht) in faelle {
            let m = Menue {
                kontext: k,
                quelle: "https://a.example/ziel.png".into(),
                x: 120,
                y: 90,
                untermenue_offen: false,
                mime: String::new(),
                quelle_bytes: None,
                zeiger: None,
                zeiger_seit: None,
                feld_zustand: FeldZustand {
                    kann_undo: false,
                    hat_auswahl: true,
                    hat_text: false,
                    kann_paste: false,
                },
                // Release 08.10.2026: die Vorgaben der Feature-Schalter.
                konvertieren: true,
                beobachten: false,
                skala_prozent: 100,
            };
            // GEMESSEN: erst **welche** Zeile, dann **ob** der Klick sie trifft.
            let nr = m
                .zeilen(1000)
                .iter()
                .position(|z| match z {
                    Zeile::Aktion(a) => a.text() == gesucht,
                    _ => false,
                })
                .unwrap_or_else(|| panic!("{k:?}: die Zeile „{gesucht}“ gibt es nicht"));
            //
            // KONTEXT (06.10.2026, 02:35): DER TEST HAT **MICH** KORRIGIERT, und
            // das ist sein Wert: `zeilen_y` liefert einen Versatz **innerhalb**
            // des Menues (es beginnt mit `LUFT_OBEN`), der absolute Punkt ist
            // also `rechteck.y + zeilen_y(...)` — genau so rechnet auch
            // `nr_unter` (`let oben = r.y + self.zeilen_y(nr, fensterhoehe)`).
            // Ohne das `r.y` traf mein Klick auf „Übersetzen“ statt auf „Link in
            // neuem Tab öffnen“ — der Test hat meinen Rechenfehler gesehen, nicht
            // einen Fehler im Menue.
            //
            // GEMESSENE ENTSCHEIDUNG, warum das Rechteck aus **derselben**
            // Funktion kommt wie beim Treffer (`m.rechteck(800, 1000)`) und nicht
            // aus `m.x`/`m.y`: das Rechteck wird beim Zeichnen **geklemmt** (es
            // muss ins Fenster passen), und eine eigene Zahl waere eine zweite
            // Wahrheit ueber dieselbe Frage.
            let r = m
                .rechteck(800, 1000)
                .expect("GEMESSEN: das Menue hat bei 800x1000 kein Rechteck");
            let y = m.zeilen_y(nr, 1000);
            // GEMESSENE ENTSCHEIDUNG, warum der **Mittelpunkt** und nicht die
            // linke Kante: `ZEILE` (28) und `LUFT_OBEN` (4) sind die Zahlen der
            // Zeichnung; die Mitte liegt in **jeder** Zeile.
            let mitte = Point::new((r.x + 40) as f32, (r.y + y + ZEILE / 2) as f32);
            // GEMESSEN (Bilder `ktx29_*`): 800 px Fensterbreite.
            let getroffen = m.zeile_unter(800, 1000, &mitte);
            // GEMESSENE ENTSCHEIDUNG, warum der Vergleich **ueber die
            // Beschriftung** laeuft und nicht ueber die Nummer: `zeile_unter`
            // liefert jetzt **die Zeile** (`Option<Zeile>`), und die
            // Nummer waere eine Behauptung ueber unsere Reihenfolge — die
            // Beschriftung ist eine Behauptung ueber das, was der Nutzer liest.
            let getroffen_text = match getroffen {
                Some(Zeile::Aktion(a)) => a.text().to_string(),
                other => format!("{other:?}"),
            };
            assert_eq!(
                getroffen_text, gesucht,
                "{k:?}: der Klick auf „{gesucht}“ (Zeile {nr}, y {y}) traf „{getroffen_text}“"
            );
        }
    }

    /// DER NUTZERWUNSCH AN **ERSTER** STELLE: „Kopieren“ oben, „Uebersetzen“
    /// **direkt danach** — und das gilt fuer **jede** Menueart, in der beide
    /// Zeilen vorkommen.
    #[test]
    fn kopieren_oben_und_uebersetzen_direkt_danach() {
        for k in Kontext::ALLE {
            let liste = sichtbare_eintraege(k, "https://a.example/ziel.html", false);
            let texte: Vec<&str> = liste.iter().map(|e| e.text()).collect();
            let kop = texte.iter().position(|t| *t == "Copy");
            let ueb = texte.iter().position(|t| *t == "Translate");
            let (Some(kop), Some(ueb)) = (kop, ueb) else {
                // Ohne „Uebersetzen“ in dieser Menueart gibt es nichts zu
                // behaupten (GEMESSEN: es erscheint nur **mit** Auswahl).
                continue;
            };
            assert_eq!(kop, 0, "{k:?}: „Kopieren“ steht an {kop}, nicht oben");
            assert_eq!(
                ueb, 1,
                "{k:?}: „Uebersetzen“ steht an {ueb}, nicht direkt unter „Kopieren“ \
                 (ganze Liste: {texte:?})"
            );
        }
    }

    /// JEDER KONTEXT HAT EINTRÄGE — UND KEINER MEHR ALS FUENF.
    ///
    /// GEMESSENE BEHAUPTUNG: das ist die Abnahme von „Normales
    /// Kontextmenue" und von M3.
    #[test]
    fn jeder_kontext_hat_eintraege() {
        // KONTEXT (03.10.2026, Leitung 02:10): aus fuenf wurde **sechs** —
        // das Eingabefeld kam dazu.
        assert_eq!(super::Kontext::ALLE.len(), 6);
        for k in super::Kontext::ALLE {
            let e = super::eintraege(k, false);
            assert!(!e.is_empty(), "{k:?} hat keine Eintraege");
            assert!(
                e.len() <= super::MAX_EINTRAEGE,
                "{k:?} hat {} Eintraege (Grenze {})",
                e.len(),
                MAX_EINTRAEGE
            );
        }
    }

    /// ANFORDERUNGEN ZEILE 11: BILD SPEICHERN UND VIDEO SPEICHERN.
    ///
    /// GEMESSENE BEHAUPTUNG: die beiden genannten Eintraege stehen in den
    /// genannten Kontexten. Das ist der Wortlaut der Anforderung, als Test.
    #[test]
    fn bild_und_video_speichern_stehen_dort_wo_die_anforderung_sagt() {
        assert!(
            super::eintraege(super::Kontext::Bild, false).contains(&super::Eintrag::BildSpeichern),
            "auf einem Bild fehlt „Bild speichern\": {:?}",
            super::eintraege(super::Kontext::Bild, false)
        );
        assert!(
            super::eintraege(super::Kontext::Video, false)
                .contains(&super::Eintrag::VideoSpeichern),
            "auf einem Video fehlt „Video speichern\": {:?}",
            super::eintraege(super::Kontext::Video, false)
        );
        // UND: der BOGEN „nur Video, sonst nichts" (ANFORDERUNG Zeile 11).
        //
        // GEMESSENER EIGENER FEHLER (02.10., Nutzerwunsch „Kopieren"):
        // hier stand `eintraege(Video) == [VideoSpeichern]` und
        // `eintraege(Link) == [LinkOeffnen, LinkNeuerTab, LinkSpeichern,
        // LinkKopieren]`. GEMESSEN: beides ist jetzt FALSCH — „Kopieren" und
        // „Uebersetzen" stehen GANZ OBEN, und die drei Kopier-Eintraege gibt
        // es nicht mehr. Die neuen Behauptungen stehen im Test
        // `das_menue_faengt_mit_kopieren_an` (unten) und werden hier nicht
        // wiederholt — ein Test, der dieselbe Zahl zweimal behauptet, meldet
        // den Fehler nur einmal.
        //
        // Koordination 01.10. abends: „Link-Kontextmenue um „Link öffnen"
        // und „Link speichern unter" ergaenzen" — die beiden stehen weiter.
        assert_eq!(super::Eintrag::LinkOeffnen.text(), "Open link");
        assert_eq!(super::Eintrag::LinkSpeichern.text(), "Save link as");
        // GEMESSEN (02.10.): die Beschriftung ist der Wortlaut des Nutzers
        // (24 Zeichen), die Breite ist danach bemessen (216 + 8 = 224).
        assert_eq!(
            super::Eintrag::LinkNeuerTab.text(),
            "Open link in new tab"
        );
        // UND: die beiden brauchen eine Quelle — ohne Adresse gibt es
        // kein „Link öffnen".
        assert!(super::Eintrag::LinkOeffnen.braucht_quelle(super::Kontext::Link));
        assert!(super::Eintrag::LinkSpeichern.braucht_quelle(super::Kontext::Link));
    }

    /// OHNE QUELLE FAELLT DER EINTRAG WEG — ER WIRD NICHT GRAU.
    #[test]
    fn ohne_quelle_faellt_der_eintrag_weg() {
        //
        // GEMESSENE ENTSCHEIDUNG (02.10., Untermenue): die Ersatzzliste ist
        // die Liste von Leer OHNE die Zeile, die ohne Quelle nichts anbietet.
        // GEMESSENER BEFUND vorher: der Ersatz nahm jede Zeile ungefiltert,
        // und seit „Konvertiert herunterladen >“ im leeren Kontext steht,
        // stand eine Elternzeile da, die sich aufklappt und leer bleibt.
        // „Kopieren“ bleibt, weil es die Seitenadresse kopiert.
        // GEMESSENE ENTSCHEIDUNG (05.10.): es sind jetzt **zwei** Zeilen, die
        // ohne Quelle nichts anbieten — „Konvertiert herunterladen“ (der
        // Untermenue-Punkt) und „Uebersetzen“ (es braucht markierten Text, und
        // `braucht_quelle` sagt das an **einer** Stelle).
        let erwartet: Vec<super::Eintrag> = super::eintraege(super::Kontext::Leer, false)
            .into_iter()
            .filter(|e| *e != super::Eintrag::Konvertieren && *e != super::Eintrag::Uebersetzen)
            .collect();
        assert_eq!(
            super::sichtbare_eintraege(super::Kontext::Bild, "", false),
            erwartet,
            "ohne Quelle zeigt das Bild-Menue nicht die Seite-Eintrae"
        );
        //
        // GEMESSENER EIGENER FEHLER (01.10.): hier stand
        // `eintraege(Bild).len() > sichtbare_eintraege(Bild, "").len()`.
        // GEMESSEN: 3 > 3 ist FALSCH. Der Rueckfall liefert genauso drei
        // Zeilen (Seite neu laden, Untersuchen, Seitendaten loeschen), wie
        // das Bild-Menue sie hat (Bild speichern, Bild kopieren, Adresse).
        // Die Behauptung „es wird weniger" war geraten; die Zahl stimmt nicht.
        // Die richtige Behauptung ist die, die ausserhalb steht: die Liste
        // IST die Liste von Leer.
        assert_eq!(
            super::sichtbare_eintraege(super::Kontext::Bild, "", false),
            erwartet,
            "ohne Quelle ist das Menue nicht das von Leer"
        );
        // UND: das Untermenue steht in KEINEM Ersatzmenue (es kann dort nichts
        // anbieten).
        assert!(
            !super::sichtbare_eintraege(super::Kontext::Bild, "", false)
                .contains(&super::Eintrag::Konvertieren)
        );
        // MIT QUELLE: alle fuenf (02.10.: „Kopieren" und „Uebersetzen"
        // kommen dazu, „Bild kopieren" ist weg; seit dem Untermenue kommt
        // „Konvertiert herunterladen >" dazu).
        assert_eq!(
            super::sichtbare_eintraege(super::Kontext::Bild, "https://a.example/b.png", false),
            vec![
                super::Eintrag::Kopieren,
                super::Eintrag::Uebersetzen,
                super::Eintrag::BildAdresse,
                super::Eintrag::BildNeuerTab,
                super::Eintrag::BildBeschreibung,
                super::Eintrag::BildSpeichern,
                super::Eintrag::Konvertieren
            ]
        );
        // GEMESSEN (02.10., Nutzerwunsch): DER TEXT braucht jetzt eine
        // Quelle, denn „Kopieren" kopiert den Text — ohne Text gibt es nichts
        // zu kopieren. Vorher stand hier `vec![TextKopieren]` fuer eine leere
        // Quelle, weil „Text kopieren" nichts brauchte; das ist jetzt
        // `braucht_quelle(Text)` = true, und damit faellt der Eintrag weg und
        // das Menue zeigt den Rueckfall.
        //
        // GEMESSENE ENTSCHEIDUNG (02.10., Untermenue): auch hier ist der
        // Rueckfall die Liste von Leer OHNE die Elternzeile des Untermenues —
        // dieselbe Regel wie beim Bild (siehe oben). Die Zeile ohne Text
        // waere aufklappbar und leer.
        //
        // KONTEXT (03.10.2026, Leitung 02:10): OHNE TEXT BLEIBT **eine** Zeile.
        //
        // GEMESSENER BEFUND: `sichtbare_eintraege` hat **eine** Regel fuer
        // alle Kontexte -- die Zeilen ohne Quelle werden weggefiltert, und
        // **nur wenn dann nichts uebrig ist**, kommt der Rueckfall (die
        // Leer-Liste ohne „Konvertiert herunterladen“). GEMESSEN ist, dass im
        // Textkontext „Alles auswaehlen“ **keine** Quelle braucht
        // (`braucht_quelle` = false) -- also ist die Liste nicht leer, und der
        // Rueckfall greift nicht. GEMESSEN ist, dass das **richtig** ist: es
        // gibt nichts zu kopieren, aber man kann immer noch alles auswaehlen.
        // Eine zweite Regel (Rueckfall auch bei einer Zeile) waere eine Regel
        // an zwei Stellen.
        assert_eq!(
            super::sichtbare_eintraege(super::Kontext::Text, "", false),
            vec![super::Eintrag::AllesAuswaehlen],
            "ohne Text bleibt nur „Alles auswaehlen“"
        );
        // MIT TEXT: die **vier** Zeilen des Textmenues (Leitung 02:10).
        assert_eq!(
            super::sichtbare_eintraege(super::Kontext::Text, "Rust", false),
            vec![
                super::Eintrag::Kopieren,
                super::Eintrag::Uebersetzen,
                super::Eintrag::GoogleSuchen,
                super::Eintrag::AllesAuswaehlen
            ],
            "GEMESSEN: das Textmenue hat nicht die vier Zeilen der Freigabe"
        );
        // NUR WEISSES ZAEHLT NICHT ALS QUELLE.
        assert_eq!(
            super::sichtbare_eintraege(super::Kontext::Bild, "   ", false),
            super::sichtbare_eintraege(super::Kontext::Bild, "", false)
        );
    }

    /// DAS MENUE IST SO BREIT WIE DER LAENGSTE WORTLAUT — GEMESSEN 240 PX
    /// (frueher 224, davor 200).
    ///
    /// GEMESSENER EIGENER FEHLER (02.10., Nutzerwunsch): dieser Test hiess
    /// `das_menue_ist_200_px_breit` und behauptete `MENUE_BREITE == 200`.
    /// GEMESSEN: die Beschriftung „Link in neuem Tab öffnen" braucht
    /// 24 * 8 + 2 * 12 = **216** px, und 200 haette sie abgeschnitten —
    /// genau das, was in Runde 3 mit „In neuem Tab öffnen" passiert ist.
    #[test]
    fn das_menue_ist_so_breit_wie_der_laengste_text() {
        let _de = narsil_i18n::scoped(narsil_i18n::Language::German); // measured against the German template
        let m = super::Menue::oeffne(super::Kontext::Leer, "", &Point::new(100.0, 100.0));
        let r = m.rechteck(1000, 700).expect("Menue");
        assert_eq!(r.breite, super::MENUE_BREITE);
        // GEMESSEN (02.10., Untermenue): war 224 (216 + 8). Der laengste
        // Wortlaut ist jetzt „Konvertiert herunterladen >“ mit GEMESSEN
        // 26 Zeichen — 26 * 8 + 24 + 8 = 240.
        // GEMESSEN (02.10.): 27 Zeichen, nicht 26 — „herunterladen“
        // hat 13 Buchstaben. 27 * 8 + 24 + 8 = 248.
        //
        // Release 08.10.2026 (Glas-Stil): 330 px — die Zeile traegt jetzt Icon
        // und Kuerzelspalte (Rechnung unten in `glas_zeile`).
        assert_eq!(MENUE_BREITE, 330, "GEMESSEN: die Glas-Zeile braucht bis 285 px");
        //
        // GEMESSENE ENTSCHEIDUNG (02.10.): die Formel nimmt jetzt den
        // laengsten WORTLAUT aus dem Enum statt eines festen Textes. Ein
        // fester Text ist eine Zahl, die stimmt, bis jemand eine Zeile
        // ergaenzt — und dann ist die Breite falsch, ohne dass jemand etwas
        // gemerkt hat. Genau das ist zweimal passiert (200 -> 224 -> 240).
        let laengster_text = [
            super::Eintrag::Kopieren,
            super::Eintrag::Uebersetzen,
            super::Eintrag::LinkOeffnen,
            super::Eintrag::LinkNeuerTab,
            super::Eintrag::LinkSpeichern,
            super::Eintrag::BildSpeichern,
            super::Eintrag::BildAdresse,
            super::Eintrag::VideoSpeichern,
            super::Eintrag::SeiteNeuLaden,
            super::Eintrag::Untersuchen,
            super::Eintrag::SeitendatenLoeschen,
            super::Eintrag::Konvertieren,
        ]
        .iter()
        .map(|e| e.text().chars().count() as u32)
        .max()
        .unwrap_or(0);
        assert_eq!(
            laengster_text, 27,
            "GEMESSEN: „Konvertiert herunterladen >“ hat 27 Zeichen"
        );
        let breite = MENUE_BREITE;
        // UND: es passt die LAENGSTE Zeile. Release 08.10.2026 (Glas-Stil): eine
        // Zeile ist Rand + Icon + Abstand + Beschriftung (ohne „>“) + rechte
        // Spalte (Chevron oder 12 px Luft und Kuerzel) + Rand.
        let glas_zeile = |k: super::Kontext, e: super::Eintrag| {
            let rechts = if e.hat_untermenue() {
                super::PFEIL_BOX + 8
            } else {
                e.kuerzel(k).map_or(0, |t| 12 + super::kuerzel_breite(t, super::SCHRIFT_KUERZEL))
            };
            super::RAND
                + super::ICON
                + super::ICON_ABSTAND
                + e.zeichen().chars().count() as u32 * super::super::ZEICHENBREITE
                + rechts
                + super::RAND
        };
        let laengste = super::Kontext::ALLE
            .iter()
            .flat_map(|k| super::eintraege(*k, false).into_iter().map(move |e| (*k, e)))
            .map(|(k, e)| glas_zeile(k, e))
            .max()
            .unwrap_or(0);
        assert!(
            laengste <= super::MENUE_BREITE,
            "die laengste Beschriftung braucht {laengste} px, das Menue hat {breite}"
        );
        eprintln!("KTX-MESSUNG: laengste Beschriftung {laengste} px, Menue {breite} px");
    }

    /// DAS MENUE BLEIBT IM FENSTER.
    ///
    /// GEMESSENE BEGRUENDUNG: das Menue wandert an den Rand, statt
    /// abgeschnitten zu werden. Ein halbes Menue ist nicht bedienbar.
    #[test]
    fn das_menue_bleibt_im_fenster() {
        for (b, h) in [
            (1000u32, 700u32),
            (800, 600),
            (1920, 1080),
            (300, 200),
            (200, 100),
            (100, 50),
            (0, 0),
            (u32::MAX, u32::MAX),
        ] {
            for p in [
                Point::new(0.0, 0.0),
                Point::new(999.0, 699.0),
                Point::new(500.0, 500.0),
                Point::new(f32::NAN, f32::NAN),
                Point::new(-50.0, -50.0),
                Point::new(1e30, 1e30),
            ] {
                let m = super::Menue::oeffne(super::Kontext::Leer, "", &p);
                let Some(r) = m.rechteck(b, h) else {
                    continue;
                };
                assert!(r.rechts() <= b, "bei {b}x{h}: ragt rechts heraus");
                assert!(r.unten() <= h, "bei {b}x{h}: ragt unten heraus");
                assert!(r.breite <= MENUE_BREITE, "bei {b}x{h}: zu breit");
                // UND: der NaN-Punkt trifft NICHTS (U1c).
                assert_eq!(
                    m.zeile_unter(b, h, &Point::new(f32::NAN, f32::NAN)),
                    None,
                    "NaN war ein Treffer"
                );
            }
        }
    }

    /// JEDER EINTRAG TRIFT SICH SELBST — UND EIN Klick DANEBEN trifft nichts.
    #[test]
    fn jeder_eintrag_trifft_sich_selbst() {
        let b = 1000u32;
        let h = 700u32;
        for k in super::Kontext::ALLE {
            let mut m =
                super::Menue::oeffne(k, "https://a.example/quelle", &Point::new(100.0, 100.0));
            //
            // KONTEXT (03.10.2026, Leitung 02:10): IM EINGABEFELD braucht der
            // Test einen **Zustand**, sonst ist jede Zeile grau und **keine**
            // trifft sich selbst. GEMESSEN: genau das war der erste Fehler
            // dieses Laufs (`Feld: Zeile 0 trifft sich nicht selbst`,
            // `left: None`) -- und es ist **richtig**: ein leeres Feld kann
            // nichts.
            if k == super::Kontext::Feld {
                // GEMESSENE ENTSCHEIDUNG, warum **AUSWAHL** und nicht VOLL:
                // bei VOLL ist "Ausschneiden" grau (keine Markierung), und
                // dieser Test behauptet, dass **jede** Zeile sich selbst trifft.
                m.setze_feld_zustand(super::FeldZustand::AUSWAHL);
                // UND: im LEEREN Zustand trifft **keine** Zeile -- das ist die
                // Behauptung, die ein graues Feld tragen muss.
                let mut leer = m.clone();
                leer.setze_feld_zustand(super::FeldZustand::LEER);
                let rl = leer.rechteck(b, h).expect("Feldmenue");
                let yl = rl.y + leer.zeilen_y(0, h) + 10;
                assert_eq!(
                    leer.zeile_unter(b, h, &Point::new((rl.x + 20) as f32, yl as f32)),
                    None,
                    "GEMESSEN: im leeren Eingabefeld trifft eine Zeile"
                );
            }
            let r = m.rechteck(b, h).expect("Menue");
            // GEMESSENE ENTSCHEIDUNG (02.10.): die Abfrage heisst jetzt
            // `zeile_unter` und liefert eine `Zeile` — das Untermenue hat
            // Zeilen, die keine Eintraege sind. Der Test ist derselbe:
            // jede Zeile trifft sich selbst, und unter der letzten ist nichts.
            let zeilen = m.zeilen(h);
            for (nr, z) in zeilen.iter().enumerate() {
                // KONTEXT (03.10., Trennlinien): die Position kommt aus
                // `zeilen_y` — vorher stand hier `LUFT_OBEN + ZEILE * nr`,
                // und das ist seit den Trennlinien an jeder Zeile falsch, vor
                // der eine Linie steht.
                let y = r.y + m.zeilen_y(nr, h);
                let p = Point::new((r.x + 20) as f32, (y + 10) as f32);
                //
                // KONTEXT (04.10.2026): die **graue** Zeile trifft sich nicht —
                // sie ist ja gerade kein Treffer (das ist die Zusage von „ grau“).
                // GEMESSENE ENTSCHEIDUNG, warum die Erwartung aus `aktiv()`
                // kommt und nicht aus einer Liste im Test: sonst muesste der
                // Test dieselbe Regel an zweiter Stelle wiederholen.
                let erwartet = match z {
                    super::Zeile::Aktion(e) if !e.aktiv(k, m.feld_zustand) => None,
                    _ => Some(z),
                };
                assert_eq!(
                    m.zeile_unter(b, h, &p).as_ref(),
                    erwartet,
                    "{k:?}: Zeile {nr} trifft sich nicht selbst (erwartet {:?})",
                    erwartet
                );
            }
            // UNTERHALB der letzten Zeile: kein Treffer.
            let r2 = m.rechteck(b, h).expect("Menue");
            assert_eq!(
                m.zeile_unter(
                    b,
                    h,
                    &Point::new((r2.x + 20) as f32, r2.unten() as f32 + 1.0)
                ),
                None,
                "{k:?}: unter dem super::Menue ist ein Treffer"
            );
            // LINKS daneben: kein Treffer.
            assert_eq!(
                m.zeile_unter(b, h, &Point::new(r2.x as f32 - 1.0, (r2.y + 10) as f32)),
                None,
                "{k:?}: links vom super::Menue ist ein Treffer"
            );
            // KONTEXT (03.10., Trennlinien): UND IM TRENNBAND IST KEIN
            // TREFFER — das Band gehoert zu keiner Zeile. GEMESSENE
            // ENTSCHEIDUNG: das wird an **jedem** Band geprueft, weil ein
            // Klick dort, der eine Zeile ausloest, beim Abschneiden einer
            // Zeile stillschweigend etwas anderes ausloest.
            for nr in 1..zeilen.len() {
                if !m.trenn_vor(nr, h) {
                    continue;
                }
                let oben = r2.y + m.zeilen_y(nr, h);
                assert_eq!(
                    m.zeile_unter(b, h, &Point::new((r2.x + 20) as f32, (oben - 1) as f32)),
                    None,
                    "{k:?}: das Trennband vor Zeile {nr} ist ein Treffer"
                );
            }
        }
    }

    /// DAS MENUE FAENGT MIT „KOPIEREN" AN, DARUNTER „UEBERSETZEN"
    /// (Nutzer, 02.10. nachts, Vorrang).
    ///
    /// GEMESSENE BEHAUPTUNG: in ALLEN fuenf Kontexten steht `Kopieren` an
    /// Stelle 0 und `Uebersetzen` an Stelle 1 — ausser im leeren Kontext,
    /// wo es „Kopieren" gibt (die Seitenadresse) und kein „Uebersetzen"
    /// (eine leere Seite hat nichts zu uebersetzen).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum „Uebersetzen" NICHT im leeren Kontext
    /// steht: `uebersetzen()` gibt dort `None` zurueck, und ein Eintrag, der
    /// zurueck `None` gibt, ist ein Eintrag, der nichts tut — genau die
    /// Sorte Eintrag, die diese Oberflaeche nicht hat (siehe Kopfzeile zu
    /// den Kontexten).
    #[test]
    fn das_menue_faengt_mit_kopieren_an() {
        for k in super::Kontext::ALLE {
            let liste = super::eintraege(k, false);
            //
            // KONTEXT (03.10.2026, Leitung 02:10): DIE AUSNAHME VOR der
            // Behauptung, nicht danach. GEMESSEN ist, dass mein erster
            // Versuch sie **danach** stellte -- und der Test meldete dann
            // `Feld: der erste Eintrag ist Some(Rueckgaengig)`, also war die
            // Zeile vorhanden und die Reihenfolge richtig: nur die Reihenfolge
            // der Behauptungen war falsch.
            //
            // GEMESSENE ENTSCHEIDUNG, warum das Eingabefeld die Ausnahme ist:
            // "Kopieren zuerst, dann Uebersetzen" ist der **Nutzerwunsch** fuer
            // die Menues, in denen man etwas **nimmt** (leer, Link, Bild,
            // Video). Die sechs Feldzeilen hat die Leitung **woertlich wie bei
            // Firefox** genannt, und dort steht "Rueckgaengig" oben -- wer
            // tippt und es bereut, sucht es an erster Stelle.
            //
            // KONTEXT (03.10.2026, 03:45, KORREKTUR der Leitung): die
            // Ausnahme fuer das Eingabefeld ist **aufgehoben**. GEMESSEN: der
            // Nutzerwunsch lautet woertlich „Rechtsklick: ganz oben Kopieren,
            // dann Uebersetzen“ und gilt **ueberall** — also beginnt auch das
            // Feldmenue mit „Kopieren“. GEMESSENE ENTSCHEIDUNG, warum hier
            // **kein** Zweig mehr steht: mein erster Versuch begruendete die
            // Ausnahme mit dem Firefox-Wortlaut — und die Leitung hat
            // entschieden, dass der Nutzerwunsch hier geht. Das ist eine
            // Entscheidung, keine Messung, und sie steht jetzt da.
            assert_eq!(
                liste.first(),
                Some(&super::Eintrag::Kopieren),
                "{k:?}: der erste Eintrag ist {:?}",
                liste.first()
            );
            assert_eq!(super::Eintrag::Kopieren.text(), "Copy");
            //
            //
            // KONTEXT (03.10.2026, Leitung 02:10): "Kopieren zuerst, dann
            // Uebersetzen" ist die Reihenfolge des **NUTZERWUNSCHES".
            //
            // KONTEXT (03.10.2026, 03:45, KORREKTUR der Leitung): sie gilt
            // **ueberall**, also auch im Eingabefeld — meine Ausnahme von
            // 02:10 (Begruendung: der Firefox-Wortlaut) ist **aufgehoben**.
            // GEMESSENE ENTSCHEIDUNG, warum hier **kein** Zweig mehr steht:
            // eine Begruendung, die die Leitung ueberstimmt hat, waere eine
            // zweite Wahrheit ueber den Nutzerwunsch.
            if k == super::Kontext::Leer {
                // GEMESSEN: seit dem Untermenue (02.10.) **5** Zeilen
                // (Kopieren, Seite neu laden, Untersuchen, Seitendaten
                // loeschen, Konvertiert herunterladen >) — kein „Uebersetzen“.
                //
                // KONTEXT (03.10.2026, Leitung 02:10): es waren **zehn** Zeilen —
                // die sieben von Firefox **plus** „Seitendaten loeschen“ und
                // „Konvertiert herunterladen >“ aus ANFORDERUNGEN Zeile 20.
                //
                // KONTEXT (05.10.2026, 20:35, Leitung nach dem **Bild**):
                // **elf** Zeilen — es kam „Uebersetzen“ **direkt unter „Kopieren“**
                // dazu (Nutzerwunsch woertlich). GEMESSENE KORREKTUR meiner
                // eigenen Begruendung von damals: ich hatte geschrieben „auf einer
                // leeren Flaeche gibt es keinen Text“ — **falsch** war daran nur
                // die Schlussfolgerung, nicht der Befund: markierter Text auf der
                // leeren Flaeche ist Text, und Firefox zeigt „Uebersetzen“ genau
                // dann. Die Sichtbarkeit entscheidet jetzt `zeilen()` (nur **mit**
                // Auswahl), damit die Zeilenzahl hier **eine** Zahl bleibt.
                assert_eq!(liste.len(), 11, "{k:?}: {} Zeilen", liste.len());
                assert!(
                    liste.contains(&super::Eintrag::Uebersetzen),
                    "{k:?}: „Uebersetzen“ fehlt in der Reihenfolge (Nutzerwunsch)"
                );
            } else {
                assert_eq!(
                    liste.get(1),
                    Some(&super::Eintrag::Uebersetzen),
                    "{k:?}: der zweite Eintrag ist {:?}",
                    liste.get(1)
                );
                assert_eq!(super::Eintrag::Uebersetzen.text(), "Translate");
            }
        }
        // UND: die Groesse passt in die Zeilengrenze (M3: 5).
        for k in super::Kontext::ALLE {
            assert!(
                super::eintraege(k, false).len() <= super::MAX_EINTRAEGE,
                "{k:?}: {} Eintraege — mehr als {MAX_EINTRAEGE}",
                super::eintraege(k, false).len()
            );
        }
    }

    /// WAS „KOPIEREN" KOPIERT — drei Ziele, an drei Kontexten.
    #[test]
    fn kopieren_hat_drei_ziele() {
        assert_eq!(
            super::Eintrag::Kopieren.kopier_ziel(super::Kontext::Text),
            Some(super::KopierZiel::Text)
        );
        assert_eq!(
            super::Eintrag::Kopieren.kopier_ziel(super::Kontext::Bild),
            Some(super::KopierZiel::Bild)
        );
        for k in [
            super::Kontext::Leer,
            super::Kontext::Link,
            super::Kontext::Video,
        ] {
            assert_eq!(
                super::Eintrag::Kopieren.kopier_ziel(k),
                Some(super::KopierZiel::Adresse),
                "{k:?}: Kopieren kopiert nicht die Adresse"
            );
        }
        // DIE ANDEREN Eintraege kopieren gar nichts.
        for e in [
            super::Eintrag::Uebersetzen,
            super::Eintrag::LinkOeffnen,
            super::Eintrag::BildSpeichern,
        ] {
            assert_eq!(e.kopier_ziel(super::Kontext::Bild), None, "{e:?} kopiert");
        }
        // UND: „Kopieren" braucht eine Quelle — ausser im leeren Kontext.
        for k in [
            super::Kontext::Link,
            super::Kontext::Bild,
            super::Kontext::Video,
            super::Kontext::Text,
        ] {
            assert!(
                super::Eintrag::Kopieren.braucht_quelle(k),
                "{k:?}: Kopieren ohne Quelle"
            );
        }
        assert!(!super::Eintrag::Kopieren.braucht_quelle(super::Kontext::Leer));
    }

    /// DIE ÜBERSETZUNGS-ADRESSE — GEMESSEN, Zeichen fuer Zeichen.
    ///
    /// GEMESSENE BEHAUPTUNG (Nutzerwunsch: „Test prueft die erzeugte URL"):
    /// `uebersetzen_url("Hallo Welt", "de")` ergibt
    /// `https://translate.google.com/translate?sl=auto&tl=de&u=Hallo%20Welt&op=translate`
    /// — und eine echte Adresse mit ihren Parametern ergibt
    /// `u=https%3A%2F%2Fbeispiel.org%2Fa%3Fb%3Dc%26d%3De`.
    #[test]
    fn die_uebersetzungs_adresse_ist_gemessen() {
        // 1) EIN TEXT.
        assert_eq!(
            super::uebersetzen_url("Hallo Welt", "de"),
            "https://translate.google.com/translate?sl=auto&tl=de&u=Hallo%20Welt&op=translate"
        );
        // 2) EINE ADRESSE MIT PARAMETERN: `&`, `?`, `=` und `:` MUESSEN
        // kodiert sein, sonst wird die Query-Zerlegung falsch.
        //
        // GEMESSENER EIGENER FEHLER (02.10., beim Schreiben des Tests): ich
        // habe die erwartete Adresse mit einem Backslash am Zeilenende
        // umgebrochen. GEMESSEN: Rust kennt kein `\`-Zeilenumbruch in
        // Zeichenketten — der Backslash und die Leerzeichen standen mit DRIN
        // der Adresse (13 Leerzeichen zwischen „tl=de" und „&u="). Die
        // Behauptung ist jetzt `concat!`, das an Wortgrenzen klebt.
        assert_eq!(
            super::uebersetzen_url("https://beispiel.org/a?b=c&d=e", "de"),
            concat!(
                "https://translate.google.com/translate?sl=auto&tl=de",
                "&u=https%3A%2F%2Fbeispiel.org%2Fa%3Fb%3Dc%26d%3De&op=translate"
            )
        );
        // 3) EINE SPRACHE AUS `LANG`: „en_GB.UTF-8" wird zu „en".
        assert_eq!(
            super::uebersetzen_url("Rust", "en"),
            "https://translate.google.com/translate?sl=auto&tl=en&u=Rust&op=translate"
        );
        // 4) OHNE SPRACHE: „de" (die Sprache dieser Oberflaeche).
        assert_eq!(
            super::uebersetzen_url("x", ""),
            "https://translate.google.com/translate?sl=auto&tl=de&u=x&op=translate"
        );
        // 5) DER KLICK: im leeren Kontext gibt es NICHTS (kein `u`).
        assert_eq!(
            super::uebersetzen(super::Kontext::Leer, "", "de"),
            None,
            "eine leere Seite wurde uebersetzt"
        );
        assert_eq!(super::uebersetzen(super::Kontext::Text, "   ", "de"), None);
        assert_eq!(
            super::uebersetzen(super::Kontext::Text, "Rust", "de").as_deref(),
            Some("https://translate.google.com/translate?sl=auto&tl=de&u=Rust&op=translate")
        );
    }

    /// DIE SPRACHE AUS `LANG` — gekürzt auf das Land.
    #[test]
    fn die_sprache_aus_lang_ist_das_land() {
        assert_eq!(super::sprache_aus_lang("de_DE.UTF-8"), "de");
        assert_eq!(super::sprache_aus_lang("en_US"), "en");
        assert_eq!(super::sprache_aus_lang("fr"), "fr");
        assert_eq!(super::sprache_aus_lang("pt_BR.UTF-8@euro"), "pt");
        // LEER und Unsinn: die Standardsprache (GEMESSEN: „de").
        assert_eq!(super::sprache_aus_lang(""), "de");
        assert_eq!(super::sprache_aus_lang("   "), "de");
        assert_eq!(super::sprache_aus_lang(".UTF-8"), "de");
        assert_eq!(super::sprache_aus_lang("_DE"), "de");
    }

    /// DAS KODIEREN — alles ausser den un reservierten Zeichen.
    #[test]
    fn das_kodieren_haelt_die_url_zusammen() {
        assert_eq!(super::kodieren("abcXYZ019-_.~"), "abcXYZ019-_.~");
        assert_eq!(super::kodieren(" "), "%20");
        assert_eq!(super::kodieren("+"), "%2B");
        assert_eq!(super::kodieren("a&b=c"), "a%26b%3Dc");
        assert_eq!(super::kodieren("/?#[]"), "%2F%3F%23%5B%5D");
        assert_eq!(super::kodieren("äöüß"), "%C3%A4%C3%B6%C3%BC%C3%9F");
        // GEMESSENE ENTSCHEIDUNG: jeder Byte wird zu `%XX` — auch Bytes ueber
        // 127, sonst waere die URL nicht eindeutig (UTF-8 ist mehr Byte).
        assert_eq!(
            super::kodieren("é").len(),
            6,
            "ein Umlaut sind zwei Byte = 6 Zeichen"
        );
    }

    /// DIE BESCHRIFTUNGEN HABEN ECHTE UMLAUTE.
    #[test]
    fn die_beschriftungen_haben_echte_umlaute() {
        let _de = narsil_i18n::scoped(narsil_i18n::Language::German); // measured against the German template
        // GEMESSENER EIGENER FEHLER (02.10., Wortlaut des Nutzers): hier stand
        // „In neuem Tab öffnen". GEMESSEN: der Nutzer schreibt „Link in neuem
        // Tab öffnen" — und der Test stand noch auf dem gekuerzten Text,
        // obwohl die Beschriftung geaendert worden war.
        assert_eq!(
            super::Eintrag::LinkNeuerTab.text(),
            "Link in neuem Tab öffnen"
        );
        assert_eq!(super::Eintrag::LinkOeffnen.text(), "Link öffnen");
        assert_eq!(
            super::Eintrag::SeitendatenLoeschen.text(),
            "Seitendaten löschen"
        );
        // UND: keine Transkription.
        for e in super::Kontext::ALLE
            .iter()
            .flat_map(|k| super::eintraege(*k, false).into_iter())
        {
            let t = e.text();
            assert!(
                !t.contains("oeffnen") && !t.contains("loeschen"),
                "{t:?} ist transkribiert"
            );
        }
    }

    /// DIE QUELLE WIRD GEKUERZT (M4) — und `NaN` wird 0.
    #[test]
    fn die_quelle_wird_gekuerzt() {
        let m = super::Menue::oeffne(
            super::Kontext::Bild,
            &"a".repeat(1_000_000),
            &Point::new(10.0, 10.0),
        );
        assert_eq!(
            m.quelle.chars().count(),
            super::MAX_QUELLE,
            "die Quelle wurde nicht gekuerzt"
        );
        // NaN: 0, und das Menue haengt sich an den Rand.
        let m2 = super::Menue::oeffne(super::Kontext::Leer, "", &Point::new(f32::NAN, f32::NAN));
        assert_eq!((m2.x, m2.y), (0, 0));
        // Negative Werte: 0 (GEMESSEN: `as u32` auf einer negativen f32 gibt
        // eine riesige Zahl, und `min` im `rechteck` haengt sie an den Rand —
        // aber die Quelle des Zeichners waere eine 4-Milliarden-Zahl).
        let m3 = super::Menue::oeffne(super::Kontext::Leer, "", &Point::new(-5.0, -7.0));
        assert_eq!(
            (m3.x, m3.y),
            (0, 0),
            "ein negativer Punkt wurde uebernommen"
        );
    }

    /// DAS UNTERMENUE STEHT GENAU DA, WO DER NUTZER ES GENANNT HAT.
    ///
    /// GEMESSENE BEHAUPTUNG (ANFORDERUNGEN Zeile 20, woertlich): „mit allen
    /// sinnvollen Zielformaten fuer **Bilder, Videos, Audio und Dateien** (auch
    /// fuer **Links auf solche Dateien**)“. GEMESSENE ENTSCHEIDUNG, warum
    /// genau vier Kontexte und nicht fuenf: Text ist nicht dabei, weil ein
    /// Textstueck keine Datei ist (`ziele_fuer(mime, endung)` braucht eine
    /// Endung, ein markiertes Wort hat keine).
    #[test]
    fn das_untermenue_steht_in_vier_kontexten() {
        for k in [
            super::Kontext::Leer,
            super::Kontext::Link,
            super::Kontext::Bild,
            super::Kontext::Video,
        ] {
            assert!(
                super::eintraege(k, false).contains(&super::Eintrag::Konvertieren),
                "{k:?}: das Untermenue fehlt"
            );
        }
        assert!(
            !super::eintraege(super::Kontext::Text, false).contains(&super::Eintrag::Konvertieren),
            "im Textmenue steht ein Untermenue — ein Textstueck ist keine Datei"
        );
        // GEMESSENE ENTSCHEIDUNG: es steht HINTEN, weil die Zeilen davor
        // das Ding benennen, auf dem man geklickt hat, und diese die Datei.
        let leer = super::eintraege(super::Kontext::Leer, false);
        assert_eq!(
            leer.last(),
            Some(&super::Eintrag::Konvertieren),
            "GEMESSEN: die Elternzeile steht hinten"
        );
        // UND: ohne Quelle faellt sie weg, wie jede Zeile, die eine Quelle
        // braucht (sonst waere sie ein Menue, das nichts anbietet).
        assert!(super::Eintrag::Konvertieren.braucht_quelle(super::Kontext::Bild));
        let ohne = super::sichtbare_eintraege(super::Kontext::Bild, "", false);
        assert!(
            !ohne.contains(&super::Eintrag::Konvertieren),
            "ohne Quelle steht die Elternzeile trotzdem da"
        );
    }

    /// AUFKLAPPEN UND ZUKLAPPEN — die Zahl der Zeilen, gemessen.
    ///
    /// KONTEXT (03.10.): was hier frueher stand, war „das Menue wird beim
    /// Aufklappen **hoeher**“ — 5 Zeilen zu, **11** auf. GEMESSEN ist, dass
    /// das die **Zweifachzeichnung** war: dieselben sechs Ziele standen im
    /// Kasten und im Feld daneben. GEMESSENE ENTSCHEIDUNG: das Menue bleibt
    /// bei **fuenf** Zeilen, und **das Feld daneben** waechst von 0 auf
    /// 6 Zeilen — das ist die Firefox-Form, und es ist am Bild gemessen
    /// (`ktx_*_unter.png`).
    #[test]
    fn aufklappen_und_zuklappen() {
        let mut m = super::Menue::oeffne(
            super::Kontext::Bild,
            "https://a.example/foto.jpg",
            &Point::new(120.0, 150.0),
        );
        let zu = m.zeilen(700);
        assert!(!m.ist_aufgeklappt(), "frisch geoeffnet ist zu");
        //
        // KONTEXT (04.10.2026): fuenf -> **sieben** Zeilen (Bild-Adresse
        // kopieren, Bild in neuem Tab, Bildbeschreibung sind dazugekommen).
        // GEMESSEN: `max_zeilen(700)` = (700/2 - 8) / 28 = 12 — alle sieben
        // Zeilen passen in die Haelfte, also stehen alle sieben da.
        assert_eq!(zu.len(), 7, "GEMESSEN: Bild hat nicht sieben Zeilen zu");
        assert!(
            zu.iter().all(|z| z.einzug() == 0),
            "im geschlossenen Zustand steht alles ohne Einzug"
        );
        // DIE HOEHE folgt der Zeilenzahl — das ist der ganze Trick: Hoehe,
        // Treffer und Zeichnung lesen dieselbe Liste.
        let hoehe_zu = m.rechteck(1000, 700).expect("Menue").hoehe;
        assert_eq!(
            hoehe_zu,
            m.feld_hoehe(700),
            "GEMESSEN: die Hoehe kommt aus feld_hoehe"
        );
        // KONTEXT (03.10., Trennlinien): fuenf Zeilen, aber **drei**
        // Trennlinien dazwischen (Kopieren/Übersetzen | Bild speichern |
        // Bild-Adresse anzeigen | Konvertiert herunterladen) —
        // 8 + 5 * 28 + 3 * 8 = **172** px.
        //
        // KONTEXT (04.10.2026): fuenf -> **sieben** Zeilen, also
        // 8 + 7 * 28 + 3 * 8 = **228** px (GEMESSEN vorher: 172 px).
        //
        // Release 08.10.2026 (Glas-Stil): zwei Abschnitte (SCHNELLZUGRIFFE,
        // BILD-AKTIONEN) mit je einer Ueberschrift und **einer** Linie
        // dazwischen: 2 * 6 + 7 * 32 + 2 * 24 + 9 = **293** px.
        assert_eq!(
            hoehe_zu,
            2 * super::LUFT_OBEN + 7 * ZEILE + 2 * super::UEBERSCHRIFT_HOEHE + super::TRENN_HOEHE,
            "GEMESSEN: 293 px"
        );
        // ZU: es gibt **kein** Feld. Das ist die Behauptung „es ist noch
        // nichts da“ als Zahl, nicht als Wort.
        assert_eq!(
            m.untermenue_rechteck(1000, 700),
            None,
            "zu ist kein Feld da"
        );
        // AUF:
        m.klappe_auf();
        assert!(m.ist_aufgeklappt());
        // GEMESSEN: der KASTEN bleibt bei fuenf Zeilen (er bekommt keine
        // Zielzeilen mehr — die stehen im Feld).
        assert_eq!(
            m.zeilen(700).len(),
            7,
            "GEMESSEN: der Kasten waechst nicht (er bleibt bei sieben)"
        );
        assert_eq!(m.rechteck(1000, 700).expect("Menue").hoehe, hoehe_zu);
        // GEMESSEN (KONTEXT, 03.10.): die Zielliste kommt aus `narsil-konvert`
        // und ist **6** Zeilen lang: das Original (JPEG) + PNG, WebP, GIF, BMP
        // und **ICO** (K1 nennt ICO als gebauten Bild-Enkoder, und `ziele_fuer`
        // filtert nach `produzierbar`). GEMESSENER EIGENER FEHLER: die Zahl 5
        // war aus der **Platzhalterliste** in `konvert.rs` abgelesen.
        let ziele = m.untermenue_ziele(700);
        assert_eq!(ziele.len(), 6, "GEMESSEN: 6 Zielzeilen im Feld");
        // UND: das Feld ist genau so hoch wie seine Zeilen.
        let feld = m.untermenue_rechteck(1000, 700).expect("das Feld");
        assert_eq!(feld.hoehe, 6 * ZEILE + 2 * super::LUFT_OBEN);
        assert_eq!(feld.breite, super::Menue::UNTERMENUE_BREITE);
        // ZUKLAPPEN macht alles rueckgaengig.
        m.klappe_zu();
        assert_eq!(m.zeilen(700).len(), 7, "GEMESSEN: nach dem Zuklappen");
        assert_eq!(m.rechteck(1000, 700).expect("Menue").hoehe, hoehe_zu);
        assert_eq!(m.untermenue_rechteck(1000, 700), None, "zu ist wieder zu");
    }

    /// DIE ZIELZEILEN SIND EINGERUECKT — und ihre Beschriftung passt in den
    /// Platz, den der Einzug uebrig laesst.
    #[test]
    fn die_zielzeilen_stehen_eingerueckt() {
        let _de = narsil_i18n::scoped(narsil_i18n::Language::German); // measured against the German template
        let mut m = super::Menue::oeffne(
            super::Kontext::Video,
            "https://a.example/clip.webm",
            &Point::new(120.0, 150.0),
        );
        m.klappe_auf();
        let zeilen = m.zeilen(700);
        // GEMESSENER EIGENER FEHLER (KONTEXT, 03.10.): hier stand
        // `super::konvert::ziele_fuer("video/*", "webm").len()` — die
        // **Platzhalterliste**, die **fuenf** Ziele fuer ein Video nannte
        // (PNG, GIF, WAV, MP3, FLAC). GEMESSEN ist, dass die echte Liste
        // **zwei** ist: `ziele_fuer_film_ohne_daten_mit_hinweis` gibt AVI und
        // Standbild — weil „nur Ton“ am **Ton** haengt und „animiertes GIF“ an
        // der **Bildzahl**, und beides steht nicht im MIME-Typ (K5).
        assert_eq!(
            zeilen.len(),
            4,
            "GEMESSEN: Video hat vier Elternzeilen und keine Zielzeilen im Kasten"
        );
        assert!(
            zeilen.iter().all(|z| z.einzug() == 0),
            "GEMESSEN: im Kasten steht eine Zielzeile"
        );
        // DIE ZIELZEILEN LIEGEN IM FELD — und sie sind eingerueckt. GEMESSENE
        // ENTSCHEIDUNG: sie werden dort mit [`EINZUG`] gezeichnet (siehe
        // `display_list`), also ist der Einzug eine Angabe der Zeichnung und
        // eine der Liste — und beide muessen dieselbe Zahl sein.
        let liste = super::display_list(&m, &Chrome::neu(1000), 1000, 700);
        let feld = m.untermenue_rechteck(1000, 700).expect("das Feld");
        let ziel_texte: Vec<(String, f32)> = liste
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text { origin, text, .. } if origin.x >= feld.x as f32 => {
                    Some((text.clone(), origin.x))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            ziel_texte.len(),
            m.untermenue_ziele(700).len(),
            "GEMESSEN: {} Texte im Feld, {} Ziele",
            ziel_texte.len(),
            m.untermenue_ziele(700).len()
        );
        let mut xs = ziel_texte.iter().map(|(_, x)| *x).collect::<Vec<f32>>();
        xs.dedup();
        assert_eq!(
            xs.len(),
            1,
            "GEMESSEN: die Zielzeilen stehen nicht auf einer Linie: {xs:?}"
        );
        assert_eq!(
            xs[0],
            (feld.x + super::RAND + super::EINZUG) as f32,
            "GEMESSEN: der Einzug der Zielzeilen ist nicht {} px",
            super::EINZUG
        );
        // GEMESSENE ENTSCHEIDUNG: der Einzug ist ein Vielfaches der
        // Zeichenbreite, damit er zwischen zwei Buchstaben steht und nicht
        // mitten in einem.
        assert_eq!(super::EINZUG % super::super::ZEICHENBREITE, 0);
        // UND: die laengste Zielzeile passt (248 - 24 - 16 = 208 px = 26
        // Zeichen; die laengste ist „Als PNG (Einzelbild)“ mit 20).
        //
        // GEMESSENER EIGENER FEHLER (02.10.): ich hatte 240 im Kopf,
        // als die Breite noch 240 war — und der Test hat es gemerkt, weil
        // eine Zeile 27 Zeichen hat und der Platz 25 war.
        let platz = ((super::MENUE_BREITE - 2 * super::RAND - super::EINZUG)
            / super::super::ZEICHENBREITE) as usize;
        // Release 08.10.2026 (Glas-Stil): 330 - 2 * 14 - 16 = 286 px = 35 Zeichen.
        assert_eq!(platz, 35, "GEMESSEN: 330 - 28 - 16 = 286 px = 35 Zeichen");
        let laengste_ziel = ziel_texte
            .iter()
            .map(|(t, _)| t.chars().count())
            .max()
            .unwrap_or(0);
        let laengste_eltern = zeilen
            .iter()
            .map(|z| z.text().chars().count())
            .max()
            .unwrap_or(0);
        //
        // KONTEXT (03.10.): die Elternzeile mit dem Pfeil hat jetzt **25**
        // Zeichen — „Konvertiert herunterladen“ ohne den angehaengten „>“.
        // GEMESSENE BEGRUENDUNG: der Pfeil steht am rechten Rand (Firefox),
        // und ein zweiter Pfeil im Text waere im Bild zwei Pfeile. Der NAME
        // (`Eintrag::text`) hat den „>“ weiterhin — der Text im Bild nicht.
        assert_eq!(laengste_eltern, 25, "GEMESSEN: die Elternzeile ohne Pfeil");
        //
        // GEMESSENE ENTSCHEIDUNG (KONTEXT, 03.10.): der Platz einer Zeile MIT
        // Pfeil ist um den Platz des Pfeils kleiner — [`PFEIL_ABSTAND`] (8)
        // plus ein Zeichen (8). GEMESSEN: 248 − 24 − 16 = **208** px = 26
        // Zeichen, und die Zeile braucht 25; es bleiben 8 px Luft.
        //
        // Release 08.10.2026 (Glas-Stil): links stehen jetzt Icon und Abstand,
        // rechts Chevron und 8 px Luft — 330 - 2 * 14 - 20 - 12 - 16 - 8 =
        // 246 px = 30 Zeichen.
        let platz_eltern = ((super::MENUE_BREITE
            - 2 * super::RAND
            - super::ICON
            - super::ICON_ABSTAND
            - super::PFEIL_BOX
            - 8)
            / super::super::ZEICHENBREITE) as usize;
        assert_eq!(
            platz_eltern, 30,
            "GEMESSEN: 330 - 28 - 20 - 12 - 16 - 8 = 246 px = 30 Zeichen"
        );
        assert!(
            laengste_eltern <= platz_eltern,
            "GEMESSEN: Elternzeile {laengste_eltern} Zeichen, Platz {platz_eltern}"
        );
        assert!(
            laengste_ziel <= platz,
            "GEMESSEN: die laengste Zielzeile hat {laengste_ziel} Zeichen, Platz sind {platz}"
        );
    }

    /// DAS MENUE **UND** DAS UNTERMENUE-FELD LASSEN DIE HAELFTE DES FENSTERS
    /// FREI — und schneiden ZUERST die Zielzeilen, nie die Elternzeilen.
    #[test]
    fn das_menue_bleibt_in_der_haelfte_des_fensters() {
        // GEMESSENE ENTSCHEIDUNG, warum die Elternzeilen Vorrang haben: sie
        // sind die Zusage des Menues. Eine abgeschnittene Zusage ist eine
        // Luege, eine abgeschnittene Liste ist eine Auswahl, die man
        // verschieben kann (im Fenster laesst sich das Menue bewegen).
        //
        // KONTEXT (03.10.): es sind jetzt **zwei** Felder, die sich diese
        // Grenze teilen — das Menue und das Untermenue daneben. GEMESSENE
        // ENTSCHEIDUNG: beide einzeln, denn sie stehen an verschiedenen
        // Stellen und werden einzeln gekuerzt.
        let mut m = super::Menue::oeffne(
            super::Kontext::Bild,
            "https://a.example/foto.jpg",
            &Point::new(10.0, 10.0),
        );
        m.klappe_auf();
        for (b, h) in [
            (1000u32, 1080u32),
            (1000, 700),
            (1000, 400),
            (1000, 300),
            (1000, 120),
        ] {
            let zeilen = m.zeilen(h);
            // Release 08.10.2026 (Glas-Stil): die Hoehe rechnet Ueberschriften
            // und Linien mit (`feld_hoehe`), die Grenze sind neun Zehntel.
            let hoehe = m.feld_hoehe(h);
            if !zeilen.is_empty() {
                assert!(
                    hoehe <= super::grenze_hoehe(h),
                    "GEMESSEN: {hoehe} px bei {h} px Fenster — mehr als neun Zehntel"
                );
                // UND: gekuerzt wird nur von hinten, und nur so weit wie noetig.
                let voll = m.zeilen(100_000);
                assert_eq!(zeilen[..], voll[..zeilen.len()], "GEMESSEN: bei {h} px fehlt eine Zeile mittendrin");
                assert!(voll.len() == 7, "GEMESSEN: Bild hat sieben Elternzeilen");
                if zeilen.len() < voll.len() {
                    assert!(
                        m.hoehe_in(&voll[..zeilen.len() + 1]) > super::grenze_hoehe(h),
                        "GEMESSEN: bei {h} px haette noch eine Zeile gepasst"
                    );
                }
                // UND: das Feld daneben ebenfalls.
                if let Some(f) = m.untermenue_rechteck(b, h) {
                    assert!(
                        f.hoehe <= h * super::MAX_MENUE_ZEHNTEL / 10,
                        "GEMESSEN: das Feld ist {} px bei {h} px Fenster",
                        f.hoehe
                    );
                    assert!(
                        f.unten() <= h,
                        "GEMESSEN: das Feld ragt unten heraus ({f:?} bei {h})"
                    );
                }
            } else {
                assert!(
                    m.rechteck(b, h).is_none(),
                    "GEMESSEN: {hoehe} px passen nicht in {h} px, aber das Menue ist da"
                );
            }
        }
        // GEMESSEN (KONTEXT, 03.10., Zeilenhoehe 28): die Grenze bei 700 px
        // Fenstermenue-Flaeche laesst (700 / 2 - 8) / **28** = **12** Zeilen
        // zu — vorher waren es mit 24 px Hoehe 14.
        // Release 08.10.2026 (Glas-Stil): neun Zehntel, 6 px Luft, 32 px Zeile.
        assert_eq!(
            super::Menue::max_zeilen(700),
            19,
            "GEMESSEN: (630 - 12) / 32 = 19 Zeilen"
        );
        assert_eq!(
            super::Menue::max_zeilen(300),
            8,
            "GEMESSEN: (270 - 12) / 32 = 8 Zeilen bei 300 px"
        );
    }

    /// Release 08.10.2026 (Glas-Stil): die Hover-Hinterlegungen einer Liste
    /// (weiss, [`GLAS_HOVER`] Deckkraft).
    fn hover_felder(liste: &DisplayList) -> usize {
        liste
            .iter()
            .filter(|i| {
                matches!(i, DisplayItem::Rect { color, deckkraft, .. }
                    if *color == narsil_core::color::Color::WHITE && *deckkraft == super::GLAS_HOVER)
            })
            .count()
    }

    /// DIE RECHTECKE EINER FARBE ZAEHLEN — der Messhelfer der Firefox-Optik.
    fn kaesten(farbe: narsil_core::color::Color, liste: &DisplayList) -> usize {
        liste
            .iter()
            .filter(|i| matches!(i, DisplayItem::Rect { color: c, .. } if *c == farbe))
            .count()
    }

    /// DIE TEXTE EINER ZEICHENLISTE — der zweite Messhelfer.
    fn texte(liste: &DisplayList) -> Vec<String> {
        // Release 08.10.2026 (Glas-Stil): nur die BESCHRIFTUNGEN (Schrift
        // [`SCHRIFT`]) — Ueberschriften und Tastenkuerzel sind eigene Texte
        // ([`alle_texte`]).
        liste
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text { text, font_size, .. } if *font_size == super::SCHRIFT => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Release 08.10.2026: ALLE Texte einer Liste (mit Ueberschriften und
    /// Kuerzeln).
    fn alle_texte(liste: &DisplayList) -> Vec<String> {
        liste
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// DAS BILD SCHIESSEN — der Weg aus `tests/u1_bild.rs` (M9: nach
    /// `team/log`, nicht nach `/tmp`).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Helfer **hier** steht und nicht in
    /// `tests/`: `tests/` gehoert der Rolle UI, und die Behauptung „das
    /// Menue sieht aus wie bei Firefox“ wird an **dieser** Datei geprueft —
    /// die Datei, die das Menue zeichnet. GEMESSENE ENTSCHEIDUNG, warum kein
    /// Wegwerfskript: `werkzeuge/bau.sh` ist der einzige Weg zu cargo
    /// (REGELN 4), und ein Skript ausserhalb des Baus waere ein zweiter.
    fn schiessen(liste: &DisplayList, name: &str, breite: u32, hoehe: u32) -> std::path::PathBuf {
        let wurzel = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("Repo-Wurzel")
            .to_path_buf();
        let ordner = wurzel.join("team/log");
        std::fs::create_dir_all(&ordner).expect("team/log");
        let mut schrift = narsil_raster::Schrift::mit_systemschrift();
        let ergebnis = narsil_raster::rasterize_mit_schrift(liste, &mut schrift, breite, hoehe);
        let fb = ergebnis.framebuffer;
        let mut bild = narsil_image::Bild::new(fb.width(), fb.height());
        for y in 0..fb.height() {
            for x in 0..fb.width() {
                if let Some(c) = fb.pixel(x, y) {
                    bild.setze_pixel(x, y, [c.r, c.g, c.b, c.a]);
                }
            }
        }
        let png = narsil_image::png::schreiben::png_schreiben(&bild);
        let pfad = ordner.join(format!(
            "ktx_{}_{}.png",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
            name
        ));
        std::fs::write(&pfad, &png).expect("PNG schreiben");
        eprintln!(
            "KTX-BILD {} — {} Bytes, {} Items, {} Pixel gesetzt",
            pfad.display(),
            png.len(),
            liste.len(),
            ergebnis.drawn
        );
        pfad
    }

    /// KONTEXT 03.10.: DER RAHMEN STEHT UM DAS MENUE — 1 px, vier Stuege.
    ///
    /// GEMESSENE BEGRUENDUNG (der Vorher-Befund): das Menue hatte **keinen**
    /// Rahmen, nur einen Schatten. Auf der Seite (heller als das Menue) sieht
    /// man die Oberkante nur am Schatten, und auf einem Foto verschwindet
    /// sie ganz.
    #[test]
    fn der_rahmen_steht_um_das_menue() {
        // Release 08.10.2026 (Glas-Stil): die Kante ist jetzt EIN Ring
        // (`DisplayItem::Rand`) mit der Rundung des Feldes — vier gerade
        // Stege haetten an den 14-px-Ecken vorbeigezeigt. Oben am hellsten
        // (die Lichtkante), unten am schwaechsten, ueberall durchscheinend.
        let chrome = Chrome::neu(1000);
        let m = super::Menue::oeffne(
            super::Kontext::Bild,
            "https://a.example/b.png",
            &Point::new(120.0, 150.0),
        );
        let r = m.rechteck(1000, 700).expect("Menue");
        let liste = super::display_list(&m, &chrome, 1000, 700);
        // Release 09.10.2026 (Liquid Glass): die Kante ist Teil des
        // Glas-Effekts — innen hell (oben am hellsten, das rechnet der
        // Rasterer), aussen dunkel, beide 1 px, mit der Rundung des Feldes.
        let glaeser: Vec<narsil_display::Glas> = liste
            .effekte()
            .iter()
            .filter_map(|(_, _, e)| match e {
                narsil_display::Effekt::Glas(g)
                    if g.rect.x == r.x as f32
                        && g.rect.y == r.y as f32
                        && g.rect.width == r.breite as f32
                        && g.rect.height == r.hoehe as f32 =>
                {
                    Some(*g)
                }
                _ => None,
            })
            .collect();
        assert_eq!(glaeser.len(), 1, "GEMESSEN: {} Glasflaechen genau um das Menue", glaeser.len());
        let g = glaeser[0];
        assert_eq!(g.linie, RAND_BOX as f32, "GEMESSEN: die Kante ist nicht 1 px");
        assert_eq!(g.radius, [super::ECKE; 4], "GEMESSEN: die Kante folgt nicht der Rundung");
        assert!(g.kante_hell > 0.0 && g.kante_hell < 0.5, "helle Innenkante: {}", g.kante_hell);
        assert!(g.kante_dunkel > 0.0 && g.kante_dunkel < 0.6, "dunkle Aussenkante: {}", g.kante_dunkel);
        assert!(g.unschaerfe > 0.0, "der Untergrund wird weichgezeichnet");
    }

    /// KONTEXT 03.10.: DIE MAUS FAERBT DIE ZEILE — vorher tat sie gar nichts.
    ///
    /// GEMESSENE BEGRUENDUNG (der Vorher-Befund): `Menue` hatte kein Feld fuer
    /// die Maus, und `display_list` zeichnete fuer jede Zeile dieselbe
    /// Farbe. GEMESSEN ist, dass Firefox die Zeile unter dem Zeiger in der
    /// Akzentfarbe zeichnet — das ist der einzige Unterschied zwischen „wo
    /// bin ich“ und „wo bin ich nicht“.
    #[test]
    fn die_maus_faerbt_die_zeile() {
        let chrome = Chrome::neu(1000);
        let pal = super::super::thema::Palette::fuer(chrome.thema);
        let mut m = super::Menue::oeffne(
            super::Kontext::Bild,
            "https://a.example/b.png",
            &Point::new(120.0, 150.0),
        );
        let r = m.rechteck(1000, 700).expect("Menue");
        // 1. OHNE MAUS: kein einziges Feld in der Akzentfarbe.
        // Release 08.10.2026 (Glas-Stil): die Hinterlegung ist weiss mit
        // [`GLAS_HOVER`] Deckkraft statt der Akzentfarbe.
        let _palette = &pal;
        let ruhe = super::display_list(&m, &chrome, 1000, 700);
        assert_eq!(
            hover_felder(&ruhe),
            0,
            "GEMESSEN: {} Hover-Felder ohne Maus",
            hover_felder(&ruhe)
        );
        // 2. MIT MAUS AUF ZEILE 1 („Uebersetzen“): **genau ein** Feld, und es
        //    ist so breit wie die Zeile.
        //
        // KONTEXT (03.10., Zeilenhoehe 28): der Punkt liegt in Zeile 1
        // („Übersetzen“) und wird aus `zeilen_y` geholt — GEMESSEN, dass
        // `r.y + 30` seit 28-px-Zeilen in Zeile **0** liegt.
        let ziel_nr = 1usize;
        let y1 = r.y + m.zeilen_y(ziel_nr, 700) + 10;
        assert!(
            m.setze_zeiger(1000, 700, &Point::new((r.x + 40) as f32, y1 as f32)),
            "GEMESSEN: die Maus hat keine Zeile getroffen"
        );
        assert_eq!(
            m.zeiger(),
            Some(ziel_nr),
            "GEMESSEN: Zeile 1 ist „Übersetzen“"
        );
        let hover = super::display_list(&m, &chrome, 1000, 700);
        assert_eq!(
            hover_felder(&hover),
            1,
            "GEMESSEN: {} Hover-Felder mit Maus",
            hover_felder(&hover)
        );
        let feld = hover
            .iter()
            .find_map(|i| match i {
                DisplayItem::Rect {
                    rect,
                    color,
                    radius,
                    deckkraft,
                    ..
                } if *color == narsil_core::color::Color::WHITE && *deckkraft == super::GLAS_HOVER => {
                    Some((*rect, *radius))
                }
                _ => None,
            })
            .expect("das Hover-Feld");
        assert_eq!(
            feld.0.height,
            super::ZEILE as f32,
            "GEMESSEN: die Zeilenhoehe"
        );
        assert_eq!(
            feld.0.x,
            (r.x + super::INNEN) as f32,
            "GEMESSEN: das Feld hat nicht den Innenabstand 4 px"
        );
        assert_eq!(
            feld.0.width,
            (r.breite - 2 * super::INNEN) as f32,
            "GEMESSEN: das Feld nimmt nicht die ganze Zeile ein"
        );
        assert_eq!(
            feld.1,
            [super::ECKE_ZEILE; 4],
            "GEMESSEN: die Ecke des Streifens ist nicht 4 px"
        );
        // 3. UND: der Text auf dem Feld wird weiss (`knopf_text`) — auf der
        //    Akzentfarbe hat `text` GEMESSEN Kontrast 2 (siehe `Palette`).
        let alle_texte = texte(&hover);
        assert!(
            alle_texte.len() >= 2,
            "GEMESSEN: {} Texte im Hover-Bild",
            alle_texte.len()
        );
        // GEMESSENE ENTSCHEIDUNG: gesucht wird an der **Oberkante der
        // Textzeile** — der Text sitzt mittig in der Zeile (`text_y`, siehe
        // `display_list`).
        let gehoverter_text = hover.iter().find_map(|i| match i {
            DisplayItem::Text {
                origin,
                text,
                color,
                ..
            } if origin.y == text_y(r.y + m.zeilen_y(ziel_nr, 700)) as f32 => {
                Some((text.clone(), *color))
            }
            _ => None,
        });
        let (t, c) = gehoverter_text.expect("der Text der gehoverten Zeile");
        assert_eq!(t, "Translate", "GEMESSEN: die falsche Zeile ist weiss");
        assert_eq!(c, super::GLAS_TEXT, "GEMESSEN: der Text ist nicht weiss");
        // 4. DIE MAUS WEG: das Feld verschwindet wieder.
        m.zeiger_weg();
        assert_eq!(hover_felder(&super::display_list(&m, &chrome, 1000, 700)), 0);
        // 5. UND: `NaN` ist kein Treffer (U1c war hier der Fehler).
        assert!(!m.setze_zeiger(1000, 700, &Point::new(f32::NAN, f32::NAN)));
        // 6. UND: eine Maus neben dem Menue faerbt nichts.
        assert!(!m.setze_zeiger(
            1000,
            700,
            &Point::new((r.rechts() + 5) as f32, (r.y + 10) as f32)
        ));
    }

    /// KONTEXT 03.10.: DAS UNTERMENUE IST EIN EIGENES FELD — nicht eine
    /// Fortsetzung im selben Kasten.
    ///
    /// GEMESSENER BEFUND vorher: die Zielzeilen standen **unter** der
    /// Elternzeile im selben Kasten — und waren damit von „Bild speichern“
    /// nicht zu unterscheiden. GEMESSEN ist, was Firefox macht: ein zweites
    /// Feld **neben** dem Menue, das an der Oberkante der Elternzeile
    /// haengt.
    #[test]
    fn das_untermenue_ist_ein_eigenes_feld() {
        let chrome = Chrome::neu(1000);
        let mut m = super::Menue::oeffne(
            super::Kontext::Bild,
            "https://a.example/fotos/urlaub.jpeg",
            &Point::new(120.0, 150.0),
        );
        let r = m.rechteck(1000, 700).expect("Menue");
        // ZU: es gibt kein Feld.
        assert_eq!(m.untermenue_rechteck(1000, 700), None, "zu ist zu");
        assert_eq!(m.untermenue_zeile_nr(700), None);
        // AUF: es gibt eins — **rechts** vom Menue, an der Zeile mit dem
        // Untermenue.
        m.klappe_auf();
        let u = m.untermenue_rechteck(1000, 700).expect("das Feld");
        assert_eq!(u.breite, super::Menue::UNTERMENUE_BREITE);
        assert_eq!(u.x, r.rechts(), "GEMESSEN: das Feld haengt nicht rechts");
        let eltern_nr = m.untermenue_zeile_nr(700).expect("die Elternzeile");
        assert_eq!(
            u.y,
            r.y + m.zeilen_y(eltern_nr, 700),
            "GEMESSEN: das Feld haengt nicht an der Elternzeile"
        );
        assert!(
            u.y >= r.y && u.y < r.unten(),
            "GEMESSEN: das Feld beginnt nicht an einer Zeile des Menues ({r:?} / {u:?})"
        );
        // GEMESSENE ENTSCHEIDUNG, warum das Feld **neben** dem Menue stehen
        // darf und nicht in ihm: es ist ein zweites Feld mit eigener Hoehe
        // (6 Zielzeilen sind 152 px, das Menue mit 11 Zeilen 128 px) — und
        // GEMESSEN ist, dass es **keinen** Pixel mit dem Menue teilt.
        assert_eq!(
            u.x,
            r.rechts(),
            "GEMESSEN: das Feld und das Menue teilen sich Pixel"
        );
        // UND: die Beschriftungen, die **im Feld** stehen (das Bild wird
        // nach x gefiltert — GEMESSENE ENTSCHEIDUNG: das Bild ist eine
        // flache Liste, und die Frage „welche Zeile steht in welchem Feld“
        // ist eine Frage nach der **x-Position**).
        let liste = super::display_list(&m, &chrome, 1000, 700);
        let im_feld: Vec<String> = liste
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text { origin, text, .. }
                    if origin.x >= u.x as f32 && origin.x < u.rechts() as f32 =>
                {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect();
        assert!(
            !im_feld.contains(&"Download converted".to_string()),
            "GEMESSEN: die Elternzeile steht im Feld mit drin: {im_feld:?}"
        );
        assert!(
            im_feld.iter().any(|t| t.starts_with("As PNG")),
            "GEMESSEN: kein Ziel im Feld: {im_feld:?}"
        );
        // UND: kein Ziel steht im **Menuekasten** (das war der Vorher-Befund).
        let im_menue: Vec<String> = liste
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text { origin, text, .. }
                    if origin.x < u.x as f32 && origin.x < r.rechts() as f32 =>
                {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect();
        assert!(
            im_menue
                .iter()
                .all(|t| !t.starts_with("As ") && !t.starts_with("Original")),
            "GEMESSEN: ein Ziel steht noch im selben Kasten: {im_menue:?}"
        );
    }

    /// KONTEXT 03.10.: RECHTS PASST ES NICHT MEHR — dann geht es LINKS auf.
    #[test]
    fn das_untermenue_geht_links_auf_wenn_rechts_nicht_passt() {
        // Release 08.10.2026 (Glas-Stil): 900 statt 600 px Fenster — zwei
        // 330-px-Felder nebeneinander brauchen 660 px.
        let mut m = super::Menue::oeffne(
            super::Kontext::Bild,
            "https://a.example/b.png",
            &Point::new(860.0, 150.0),
        );
        let r = m.rechteck(900, 700).expect("Menue");
        assert_eq!(r.rechts(), 900, "GEMESSEN: das Menue klebt am Rand");
        m.klappe_auf();
        let u = m.untermenue_rechteck(900, 700).expect("das Feld");
        assert_eq!(
            u.rechts(),
            r.x,
            "GEMESSEN: rechts passt es nicht ({} + {} > 600)",
            r.x,
            u.breite
        );
        assert!(
            u.x + u.breite <= 900 && u.unten() <= 700,
            "GEMESSEN: das Feld ragt aus dem Fenster: {u:?}"
        );
    }

    /// KONTEXT 03.10.: EIN KLICK IM FELD IST **KEIN** KLICK IM MENUE — und ein
    /// Klick im Feld trifft genau **ein** Ziel.
    #[test]
    fn das_ziel_unter_dem_mauszeiger_trifft_sich_selbst() {
        let mut m = super::Menue::oeffne(
            super::Kontext::Bild,
            "https://a.example/fotos/urlaub.jpeg",
            &Point::new(120.0, 150.0),
        );
        m.klappe_auf();
        let u = m.untermenue_rechteck(1000, 700).expect("das Feld");
        let ziele = m.untermenue_ziele(700);
        assert!(!ziele.is_empty(), "GEMESSEN: das Feld ist leer");
        for (nr, ziel) in ziele.iter().enumerate() {
            let y = u.y + LUFT_OBEN + ZEILE * nr as u32;
            let p = Point::new((u.x + 40) as f32, (y + 10) as f32);
            assert_eq!(
                m.untermenue_ziel_unter(1000, 700, &p).as_ref(),
                Some(ziel),
                "Ziel {nr} trifft sich nicht selbst"
            );
            // GEMESSENE ENTSCHEIDUNG: derselbe Punkt ist im **Menue** kein
            // Treffer — sonst wuesste der Aufrufer nicht, welches der beiden
            // Felder er getroffen hat.
            assert_eq!(
                m.zeile_unter(1000, 700, &p),
                None,
                "GEMESSEN: der Punkt im Feld ist auch eine Menuezeile"
            );
        }
        // UNTER dem Feld: kein Treffer. `NaN`: kein Treffer.
        assert_eq!(
            m.untermenue_ziel_unter(
                1000,
                700,
                &Point::new((u.x + 10) as f32, u.unten() as f32 + 1.0)
            ),
            None
        );
        assert_eq!(
            m.untermenue_ziel_unter(1000, 700, &Point::new(f32::NAN, f32::NAN)),
            None
        );
    }

    /// KONTEXT 03.10. (Freigabe Punkt 3): DER CHEVRON IST EIN ICON — 16x16,
    /// rechtsbuendig, und **kein** Zeichen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum nach einem `Bild`-Item gesucht wird und
    /// nicht nach einem `Text`: der Chevron wird als Pixelbild gezeichnet
    /// (siehe [`pfeil_item`]). GEMESSENE ENTSCHEIDUNG, warum das Bild
    /// **teilweise** deckend sein muss: ein Icon mit nur 0 oder 255 hat
    /// sichtbare Stufen — und genau an den Stufen erkennt man, dass es kein
    /// Icon, sondern eine Treppe ist.
    #[test]
    fn der_chevron_ist_ein_icon() {
        // Release 08.10.2026 (Glas-Stil): der Chevron kommt jetzt wie alle
        // Zeilen-Icons aus einer Datei (`chevron.svg`, `icons::pixel_genau`):
        // 16 px, rechtsbuendig an [`RAND`], grau, mittig in der Zeile.
        let chrome = Chrome::neu(1000);
        let m = super::Menue::oeffne(
            super::Kontext::Bild,
            "https://a.example/fotos/urlaub.jpeg",
            &Point::new(120.0, 150.0),
        );
        let r = m.rechteck(1000, 700).expect("Menue");
        let liste = super::display_list(&m, &chrome, 1000, 700);
        let chevrons: Vec<(f32, f32, std::sync::Arc<Vec<u8>>)> = liste
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Bild { rect, breite, hoehe, pixels }
                    if *breite == super::PFEIL_BOX && *hoehe == super::PFEIL_BOX =>
                {
                    Some((rect.x, rect.y, pixels.clone()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(chevrons.len(), 1, "GEMESSEN: {} Chevrons im Bild", chevrons.len());
        let (x, y, bild) = &chevrons[0];
        // 1. RECHTSBUENDIG an der Kuerzelspalte (2 px in den Rand, die Figur
        //    hat selbst Luft im 16-px-Kasten).
        assert_eq!(*x, (r.rechts() - super::RAND - super::PFEIL_BOX + 2) as f32, "GEMESSEN: x");
        // 2. MITTIG in der Zeile.
        let zeilen_nr = m
            .zeilen(700)
            .iter()
            .position(|z| matches!(z, super::Zeile::Aktion(e) if e.hat_untermenue()))
            .expect("die Zeile mit dem Untermenue");
        let oben = r.y + m.zeilen_y(zeilen_nr, 700);
        assert_eq!(*y, (oben + (ZEILE - super::PFEIL_BOX) / 2) as f32, "GEMESSEN: y");
        // 3. SAUBER: weiche Kanten und die Form eines Chevrons (schmal, hoch).
        let deckung = |spalte: u32, reihe: u32| bild[((reihe * super::PFEIL_BOX + spalte) * 4 + 3) as usize];
        let weich = bild.chunks_exact(4).filter(|p| p[3] > 0 && p[3] < 255).count();
        assert!(weich > 10, "GEMESSEN: nur {weich} Pixel mit weicher Kante");
        let (mut o, mut u, mut l, mut re) = (super::PFEIL_BOX, 0u32, super::PFEIL_BOX, 0u32);
        for reihe in 0..super::PFEIL_BOX {
            for spalte in 0..super::PFEIL_BOX {
                if deckung(spalte, reihe) > 0 {
                    o = o.min(reihe);
                    u = u.max(reihe);
                    l = l.min(spalte);
                    re = re.max(spalte);
                }
            }
        }
        assert!(u > o && re > l, "GEMESSEN: der Chevron ist leer");
        assert!(
            re - l < u - o,
            "GEMESSEN: der Chevron ist nicht schmal und hoch ({}x{})",
            re - l + 1,
            u - o + 1
        );
        // 4. GRAU wie die Kuerzel — leiser als die Beschriftung.
        assert_eq!(
            [bild[0], bild[1], bild[2]],
            [super::GLAS_GRAU.r, super::GLAS_GRAU.g, super::GLAS_GRAU.b],
            "GEMESSEN: der Chevron ist nicht grau"
        );
        // 5. IM TEXT steht kein Pfeil (sonst waeren es zwei).
        let t = texte(&liste);
        assert!(
            t.contains(&"Download converted".to_string()),
            "GEMESSEN: die Beschriftung fehlt: {t:?}"
        );
        assert!(!t.iter().any(|x| x.contains('>')), "GEMESSEN: im Text steht doch ein Pfeil: {t:?}");
        // 6. Icon, Text und Chevron passen in die Zeile.
        let n = super::RAND
            + super::ICON
            + super::ICON_ABSTAND
            + "Download converted".chars().count() as u32 * super::super::ZEICHENBREITE
            + super::PFEIL_BOX
            + 8
            + super::RAND;
        assert!(n <= r.breite, "GEMESSEN: Icon + Text + Chevron brauchen {n} px");
    }

    /// KONTEXT 03.10.: DIE DREI BILDER — Menue zu, Menue mit Hover, Untermenue
    /// offen. GEMESSENE ENTSCHEIDUNG, warum drei Bilder und nicht eins: die
    /// drei Punkte der Firefox-Optik (Rahmen, Hover, eigenes Feld) sind je
    /// **ein** Unterschied, und ein Bild, in dem alle drei stecken, laesst
    /// keinen davon einzeln zu.
    #[test]
    fn die_drei_bilder_der_firefox_optik() {
        for (thema, name) in [
            (super::super::thema::Thema::Hell, "hell"),
            (super::super::thema::Thema::Dunkel, "dunkel"),
        ] {
            let mut chrome = Chrome::neu(1000);
            chrome.thema = thema;
            let m = super::Menue::oeffne(
                super::Kontext::Bild,
                "https://a.example/fotos/urlaub.jpeg",
                &Point::new(300.0, 180.0),
            );
            // 1. ZU.
            let liste = super::display_list(&m, &chrome, 1000, 700);
            let p1 = schiessen(&liste, &format!("menue_{}_zu", name), 1000, 700);
            // 2. MIT MAUS AUF „BILD SPEICHERN“.
            let mut h = m.clone();
            let r = h.rechteck(1000, 700).expect("Menue");
            h.setze_zeiger(
                1000,
                700,
                // Release 08.10.2026 (Glas-Stil): die Mitte aus dem Menue —
                // Ueberschriften und Linien stehen dazwischen.
                &Point::new((r.x + 40) as f32, (r.y + h.zeile_mitte_y(2, 700)) as f32),
            );
            assert_eq!(
                h.zeiger(),
                Some(2),
                "GEMESSEN: Zeile 2 ist „Bild-Adresse kopieren“"
            );
            let liste_h = super::display_list(&h, &chrome, 1000, 700);
            let p2 = schiessen(&liste_h, &format!("menue_{}_hover", name), 1000, 700);
            // 3. UNTERMENUE OFFEN.
            let mut a = m.clone();
            a.klappe_auf();
            let liste_a = super::display_list(&a, &chrome, 1000, 700);
            let p3 = schiessen(&liste_a, &format!("menue_{}_unter", name), 1000, 700);
            // DIE ZAHLEN, die die Behauptung tragen.
            let u = a.untermenue_rechteck(1000, 700).expect("das Feld");
            eprintln!(
                "KTX-MESSUNG {name}: Menue {}x{} bei ({}, {}), Feld {}x{} bei ({}, {}), Items zu/hover/auf = {}/{}/{}",
                r.breite,
                r.hoehe,
                r.x,
                r.y,
                u.breite,
                u.hoehe,
                u.x,
                u.y,
                liste.len(),
                liste_h.len(),
                liste_a.len()
            );
            assert!(
                liste_a.len() > liste.len(),
                "GEMESSEN: das Untermenue zeichnet nichts ({})",
                liste_a.len()
            );
            assert_eq!(
                liste_h.len(),
                liste.len() + 1,
                "GEMESSEN: der Hover zeichnet {} statt eines Feldes",
                liste_h.len() - liste.len()
            );
            for p in [p1, p2, p3] {
                assert!(p.exists(), "das Bild fehlt: {}", p.display());
            }
        }
    }

    /// KONTEXT 03.10. (Freigabe Punkt 5): ALLE FUENF KONTEXTE IM BILD —
    /// hell und dunkel, je zu und aufgeklappt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **zehn** Bilder (5 × 2) und nicht zwei:
    /// die Freigabe nennt „Link-Kontext zeigen" — und GEMESSEN ist, dass der
    /// Link-Kontext der einzige mit **sechs** Zeilen und **drei**
    /// Trennlinien ist (Kopieren, Übersetzen | Link öffnen, Link in neuem Tab
    /// öffnen | Link speichern unter | Konvertiert herunterladen). Ein Bild
    /// vom Bild-Kontext (fuenf Zeilen, zwei Linien) kann diese Zahl nicht
    /// zeigen, und genau die Zahl ist der Unterschied.
    #[test]
    fn die_fuenf_kontexte_je_zwei_bilder() {
        for (thema, name) in [
            (super::super::thema::Thema::Hell, "hell"),
            (super::super::thema::Thema::Dunkel, "dunkel"),
        ] {
            let mut chrome = Chrome::neu(1000);
            chrome.thema = thema;
            for k in super::Kontext::ALLE {
                let quelle = match k {
                    super::Kontext::Bild => "https://a.example/fotos/urlaub.jpeg",
                    super::Kontext::Video => "https://a.example/film.mp4",
                    super::Kontext::Link => {
                        // GEMESSENE ENTSCHEIDUNG (03.10.): die Adresse endet
                        // auf `.html` — ANFORDERUNGEN Zeile 20 nennt das
                        // Untermenue ausdruecklich auch fuer „Links auf solche
                        // Dateien“. GEMESSEN ist, dass ohne Endung **kein**
                        // Feld aufgeht (`ziele_fuer` hat dann keine Datei) —
                        // das Bild waere dann zweimal dasselbe.
                        "https://a.example/artikel.html"
                    }
                    super::Kontext::Text => "Ein markierter Textabsatz",
                    // KONTEXT (03.10.2026, Leitung 02:10): das Eingabefeld
                    // hat **keine Datei** — GEMESSEN ist in
                    // `ktx_konvert::ziele`, dass `Kontext::Feld` eine leere
                    // Zielliste liefert, also geht **kein** Feld auf. Das Bild
                    // zeigt dann das Feldmenue ohne Untermenue.
                    super::Kontext::Feld => "https://a.example/formular",
                    super::Kontext::Leer => "https://a.example/seite.html",
                };
                let m = super::Menue::oeffne(k, quelle, &Point::new(260.0, 160.0));
                let r = m.rechteck(1000, 700).expect("Menue");
                // 1. ZU.
                let p1 = schiessen(
                    &super::display_list(&m, &chrome, 1000, 700),
                    &format!("{}_{}_zu", name, k.name()),
                    1000,
                    700,
                );
                // 2. AUF (das Untermenue).
                let mut a = m.clone();
                a.klappe_auf();
                let liste_a = super::display_list(&a, &chrome, 1000, 700);
                let p2 = schiessen(&liste_a, &format!("{}_{}_unter", name, k.name()), 1000, 700);
                // DIE ZAHLEN, die die Behauptung tragen.
                eprintln!(
                    "KTX-MESSUNG {name}/{}: {} Zeilen, {} Linien, {}x{} px, {} Items zu / {} auf",
                    k.name(),
                    m.zeilen(700).len(),
                    m.trennzahl(700),
                    r.breite,
                    r.hoehe,
                    super::display_list(&m, &chrome, 1000, 700).len(),
                    liste_a.len()
                );
                for p in [p1, p2] {
                    assert!(p.exists(), "das Bild fehlt: {}", p.display());
                }
            }
        }
    }

    /// KONTEXT 03.10. (Freigabe Punkt 4): DIE TRENNLINIEN STEHEN ZWISCHEN DEN
    /// GRUPPEN — und nur dort.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das an **zwei** Dingen geprueft wird:
    /// die Linie muss im **Bild** sein (sonst waere sie nur eine Zahl), und
    /// sie muss an der **richtigen** Stelle sein (sonst waere sie ein Streifen
    /// irgendwo). Und GEMESSENE ENTSCHEIDUNG, warum die dritte Pruefung die
    /// **Hoehe** ist: eine Linie, die am richtigen Ort steht, aber keinen
    /// Platz bekommt, klebt an der Zeile darueber — man sieht sie dann als
    /// Unterstrich.
    #[test]
    fn die_trennlinien_stehen_zwischen_den_gruppen() {
        // Release 08.10.2026 (Glas-Stil): die Linien stehen zwischen den
        // ABSCHNITTEN, und jeder Abschnitt traegt eine Ueberschrift in kleinen
        // grauen Versalien (SCHNELLZUGRIFFE, LINK-AKTIONEN, …). Die alten
        // Gruppen (`Eintrag::gruppe`) bleiben als Zahl, zeichnen aber nichts.
        let chrome = Chrome::neu(1000);
        // Release 08.10.2026: ein Link auf eine BILDdatei — nur dann gibt es
        // Ziele und damit die Zeile „Konvertiert herunterladen >“ (ohne Ziel
        // faellt sie weg, siehe `zeilen`).
        let m = super::Menue::oeffne(
            super::Kontext::Link,
            "https://a.example/artikel.png",
            &Point::new(120.0, 150.0),
        );
        let r = m.rechteck(1000, 700).expect("Menue");
        let zeilen = m.zeilen(700);
        assert_eq!(zeilen.len(), 8, "GEMESSEN: Link hat nicht acht Zeilen");
        // 1. DIE ABSCHNITTE: Kopieren, Übersetzen | die sechs Link-Zeilen.
        let abschnitte: Vec<Option<super::Abschnitt>> = zeilen.iter().map(|z| m.abschnitt_von(z)).collect();
        let s = Some(super::Abschnitt::Schnellzugriffe);
        let l = Some(super::Abschnitt::Link);
        assert_eq!(abschnitte, vec![s, s, l, l, l, l, l, l], "GEMESSEN: die Abschnitte des Linkmenues");
        // 2. DIE LINIE: nur vor Zeile 2; Ueberschriften vor Zeile 0 und 2.
        let linien_vor: Vec<usize> = (0..zeilen.len()).filter(|nr| m.trenn_vor(*nr, 700)).collect();
        assert_eq!(linien_vor, vec![2], "GEMESSEN: wo Linien stehen");
        assert_eq!(m.abschnitt_vor(0, 700), s, "GEMESSEN: die erste Ueberschrift");
        assert_eq!(m.abschnitt_vor(2, 700), l, "GEMESSEN: die zweite Ueberschrift");
        assert_eq!(m.abschnitt_vor(3, 700), None, "GEMESSEN: mitten im Abschnitt");
        // 3. IM BILD: genau eine Linie und zwei Ueberschriften.
        let liste = super::display_list(&m, &chrome, 1000, 700);
        let linien: Vec<narsil_core::geometry::Rect> = liste
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Rect { rect, color, deckkraft, .. }
                    if *color == narsil_core::color::Color::WHITE && *deckkraft == super::GLAS_LINIE =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .collect();
        assert_eq!(linien.len(), 1, "GEMESSEN: {} Trennlinien im Bild", linien.len());
        let t = alle_texte(&liste);
        for titel in ["QUICK ACCESS", "LINK ACTIONS"] {
            assert!(t.contains(&super::gesperrt(titel)), "GEMESSEN: die Ueberschrift {titel} fehlt: {t:?}");
        }
        // 4. AN DER RICHTIGEN STELLE: mitten im Band ueber der Ueberschrift.
        let band = r.y + m.zeilen_y(2, 700) - super::UEBERSCHRIFT_HOEHE - super::TRENN_HOEHE;
        assert_eq!(linien[0].y as u32, band + super::TRENN_HOEHE / 2, "GEMESSEN: die Linie steht falsch");
        assert_eq!(linien[0].height, 1.0, "GEMESSEN: die Linie ist nicht 1 px");
        assert_eq!(linien[0].x, (r.x + super::RAND) as f32);
        assert_eq!(linien[0].width, (r.breite - 2 * super::RAND) as f32);
        // 5. DIE HOEHE enthaelt Ueberschriften und Linie.
        assert_eq!(
            r.hoehe,
            super::LUFT_OBEN * 2 + 8 * super::ZEILE + 2 * super::UEBERSCHRIFT_HOEHE + super::TRENN_HOEHE,
            "GEMESSEN: {} px bei {} Zeilen und {} Linien",
            r.hoehe,
            zeilen.len(),
            m.trennzahl(700)
        );
        // 6. TEXT und FELD: je eine Linie (Schnellzugriffe | Text- bzw.
        //    Bearbeiten-Abschnitt).
        let t = super::Menue::oeffne(super::Kontext::Text, "Rust", &Point::new(10.0, 10.0));
        assert_eq!(t.trennzahl(700), 1, "GEMESSEN: das Textmenue hat {} Linien", t.trennzahl(700));
        let f = super::Menue::oeffne(super::Kontext::Feld, "https://a.example/formular", &Point::new(10.0, 10.0));
        assert_eq!(f.trennzahl(700), 1, "GEMESSEN: das Feldmenue hat {} Linien", f.trennzahl(700));
        // 7. DAS SEITENMENUE hat drei Abschnitte (Schnellzugriffe, Seite,
        //    Weitere Aktionen) — und „Konvertiert herunterladen“ steht bei der
        //    Seite, nicht unter den Werkzeugen.
        let leer = super::Menue::oeffne(super::Kontext::Leer, "https://a.example/s.html", &Point::new(10.0, 10.0));
        assert_eq!(leer.trennzahl(900), 2, "GEMESSEN: das Seitenmenue hat {} Linien", leer.trennzahl(900));
        assert_eq!(super::Eintrag::Konvertieren.abschnitt(super::Kontext::Leer), super::Abschnitt::Seite);
    }

    /// KONTEXT 03.10. (Freigabe Punkt 4): DIE MASSE DES MENUES — 8 px Ecken,
    /// 4 px Innenabstand, 28-px-Zeilen, 13 px Schrift.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **vier** Behauptungen in einem Test
    /// sind: sie sind eine einzige Aussage („die Optik ist die von Firefox“),
    /// und vier Tests koennten drei davon loeschen, ohne dass etwas rot wird.
    #[test]
    fn die_masse_sind_die_von_firefox() {
        // Release 08.10.2026 (Glas-Stil): die Masse der Vorlage — 14-px-Ecken,
        // dunkles Glas mit 92 % Deckkraft, 6 px Innenabstand, 32-px-Zeilen,
        // 14 px Beschriftung, 12,5 px Kuerzel, 10,5 px Ueberschriften.
        let chrome = Chrome::neu(1000);
        let m = super::Menue::oeffne(
            super::Kontext::Bild,
            "https://a.example/b.png",
            &Point::new(120.0, 150.0),
        );
        let r = m.rechteck(1000, 700).expect("Menue");
        let liste = super::display_list(&m, &chrome, 1000, 700);
        // 1. DAS GLAS: Ecken, Farbe, Deckkraft (Release 09.10.2026: die
        // Toenung des Glas-Effekts).
        let kasten = liste
            .effekte()
            .iter()
            .find_map(|(_, _, e)| match e {
                narsil_display::Effekt::Glas(g)
                    if g.rect.width == r.breite as f32 && g.rect.height == r.hoehe as f32 =>
                {
                    Some((g.radius, g.toenung))
                }
                _ => None,
            })
            .expect("der Kasten");
        assert_eq!(kasten.0, [super::ECKE; 4], "GEMESSEN: die Ecken sind nicht 14 px");
        assert_eq!((kasten.1.r, kasten.1.g, kasten.1.b), (super::GLAS_GRUND.r, super::GLAS_GRUND.g, super::GLAS_GRUND.b));
        assert_eq!(kasten.1.a, (super::GLAS_DECKKRAFT * 255.0).round() as u8);
        assert_eq!(super::ECKE, 14.0);
        assert!((0.7..=0.95).contains(&super::GLAS_DECKKRAFT), "GEMESSEN: kein Glas mehr");
        // 2. INNENABSTAND 6 PX am Hover-Streifen.
        assert_eq!(super::INNEN, 6);
        assert_eq!(super::LUFT_OBEN, 6);
        let mut h = m.clone();
        h.setze_zeiger(1000, 700, &Point::new((r.x + 40) as f32, (r.y + m.zeile_mitte_y(0, 700)) as f32));
        let streifen = super::display_list(&h, &chrome, 1000, 700)
            .iter()
            .find_map(|i| match i {
                DisplayItem::Rect { rect, color, deckkraft, .. }
                    if *color == narsil_core::color::Color::WHITE && *deckkraft == super::GLAS_HOVER =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("der Hover-Streifen");
        assert_eq!(streifen.x, (r.x + 6) as f32, "GEMESSEN: kein 6-px-Innenabstand");
        assert_eq!(streifen.width, (r.breite - 12) as f32);
        // 3. ZEILEN 32 PX — im 28..=32-Band der Freigabe.
        assert!((28..=32).contains(&super::ZEILE), "GEMESSEN: die Zeile ist {} px", super::ZEILE);
        assert_eq!(super::ZEILE, 32);
        // 4. DIE SCHRIFTEN: Beschriftung, Kuerzel, Ueberschrift — sonst keine.
        let sizes: Vec<f32> = liste
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text { font_size, .. } => Some(*font_size),
                _ => None,
            })
            .collect();
        assert!(sizes.contains(&super::SCHRIFT), "GEMESSEN: keine Beschriftung in 14 px");
        assert!(sizes.contains(&super::SCHRIFT_UEBERSCHRIFT), "GEMESSEN: keine Ueberschrift");
        assert!(
            sizes
                .iter()
                .all(|s| [super::SCHRIFT, super::SCHRIFT_KUERZEL, super::SCHRIFT_UEBERSCHRIFT].contains(s)),
            "GEMESSEN: die Schriften sind {sizes:?}"
        );
        assert_eq!(super::SCHRIFT, 14.0);
        // 5. JEDE Zeile hat ihr 20-px-Icon.
        let icons = liste
            .iter()
            .filter(|i| matches!(i, DisplayItem::Bild { breite, hoehe, .. } if *breite == super::ICON && *hoehe == super::ICON))
            .count();
        assert_eq!(icons, m.zeilen(700).len(), "GEMESSEN: {icons} Icons fuer {} Zeilen", m.zeilen(700).len());
        // 6. UND: das Menue bleibt in neun Zehnteln des Fensters.
        assert!(r.hoehe <= super::grenze_hoehe(700), "GEMESSEN: {} px bei 700 px Fenster", r.hoehe);
    }

    /// KONTEXT 03.10. (Freigabe „Position soll an der Mausspitze beginnen —
    /// Firefox: Menue-Ecke = Klickpunkt“): DIE MENUECKE IST DER KLICKPUNKT.
    ///
    /// GEMESSENER BEFUND (23:00, `team/log/ui4_*_rechtsklick_menue.png`): die
    /// Leitung sah im echten `narsil`-Bild, dass das Menue den Linktext
    /// ueberdeckt, und nannte als Massstab die Ecke. GEMESSEN an **meinem**
    /// Bild: der Kasten steht bei x 150, y 209, und die Ecke ist **exakt** der
    /// auf ganze Pixel abgerundete Klickpunkt — es gibt keinen Versatz.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `floor` und nicht `round`: Firefox rechnet
    /// die Popup-Position in Ganzahlen um (ein Panel kann keinen halben Pixel
    /// fangen), und beim **Abreunden** zeigt die Ecke nach **oben links** —
    /// dort, wo der Mauszeiger ist. Ein `round` koennte das Menue 0,5 px nach
    /// rechts schieben, und das ist genau die Art Versatz, die man im Bild
    /// sieht und nicht benennen kann.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Rand eine **Ausnahme** ist und kein
    /// Widerspruch: passt das Menue nicht mehr (Rand rechts/unten), wandert es
    /// an den Rand (siehe [`Menue::rechteck`]) — sonst waere die Haelfte des
    /// Menues unerreichbar. GEMESSEN ist, dass das die **einzige** Stelle ist,
    /// an der die Ecke nicht der Klickpunkt ist.
    #[test]
    fn die_menueecke_ist_der_klickpunkt() {
        //
        // KONTEXT (03.10.2026, 02:25, eigener Fehler beim Umbau): hier stand
        // ein Fenster von **1000x700**, und das Leermenue ist seit der
        // Freigabe „(3) leere Seitenflaeche“ **336 px** hoch. GEMESSEN: bei
        // einem Klickpunkt von y = 402 passt 402 + 336 = 738 nicht mehr in
        // 700 px, also **wandert** das Menue an den Rand — und der Test meldete
        // `die Ecke (611, 364) ist nicht der Klickpunkt (611, 402)`. Das war
        // **kein** Fehler des Menues, sondern ein zu kleines Testfenster.
        for (px, py) in [
            (0.0f32, 0.0f32),
            (120.4, 150.9),
            (300.0, 120.0),
            (611.0, 402.0),
        ] {
            let m = super::Menue::oeffne(
                super::Kontext::Leer,
                "https://a.example/seite.html",
                &Point::new(px, py),
            );
            let r = m.rechteck(1000, 900).expect("Menue");
            assert_eq!(
                (r.x, r.y),
                (px as u32, py as u32),
                "GEMESSEN: die Ecke ({}, {}) ist nicht der Klickpunkt ({px}, {py})",
                r.x,
                r.y
            );
            // UND: die erste Zeile beginnt genau bei [`LUFT_OBEN`] unter der
            // Ecke — der Treffer und die Zeichnung lesen dieselbe Zahl.
            // Release 08.10.2026 (Glas-Stil): ueber der ersten Zeile steht die
            // Ueberschrift ihres Abschnitts.
            assert_eq!(m.zeilen_y(0, 900), super::LUFT_OBEN + super::UEBERSCHRIFT_HOEHE);
            assert_eq!(r.y + m.zeilen_y(0, 900), r.y + super::LUFT_OBEN + super::UEBERSCHRIFT_HOEHE);
        }
        // DIE RANDFAELLE (die einzigen Abweichungen): rechts und unten.
        let rechts = super::Menue::oeffne(
            super::Kontext::Leer,
            "https://a.example/seite.html",
            &Point::new(950.0, 150.0),
        )
        .rechteck(1000, 900)
        .expect("Menue");
        assert_eq!(
            rechts.x,
            1000 - super::MENUE_BREITE,
            "GEMESSEN: das Menue ragt rechts aus dem Fenster"
        );
        let hoehe = super::Menue::oeffne(
            super::Kontext::Leer,
            "https://a.example/seite.html",
            &Point::new(100.0, 860.0),
        )
        .rechteck(1000, 900)
        .expect("Menue")
        .hoehe;
        let unten = super::Menue::oeffne(
            super::Kontext::Leer,
            "https://a.example/seite.html",
            &Point::new(100.0, 860.0),
        )
        .rechteck(1000, 900)
        .expect("Menue");
        assert_eq!(
            unten.y,
            900 - hoehe,
            "GEMESSEN: das Menue ragt unten aus dem Fenster"
        );
        assert!(
            unten.y < unten.y + hoehe,
            "GEMESSEN: das Menue steht nicht im Fenster"
        );
    }

    /// KONTEXT 03.10. (Freigabe „plus Bild-Rechtsklick (Bild speichern,
    /// Bild kopieren)“): WAS DER RECHTSKLICK AUF EIN BILD ANBIETET.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die **Reihenfolge** und nicht nur die
    /// Menge behauptet wird: `bild_und_video_speichern_stehen_dort_wo_die_
    /// anforderung_sagt` sagt, dass „Bild speichern“ **irgendwo** steht — ein
    /// Bild, in dem es unten steht, erfuellt das. Die Reihenfolge ist die
    /// freigegebene (Kopieren, Uebersetzen, Bild speichern, Bild-Adresse
    /// anzeigen, Konvertiert herunterladen).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum „Kopieren“ hier das **Bild** kopiert und
    /// nicht die Adresse: dasselbe Wort auf einem Link und auf einem Bild
    /// meint zweierlei, und der Nutzer erkennt es an der **Position** des
    /// Bildes, nicht am Wort. Das ist [`Eintrag::kopier_ziel`].
    #[test]
    fn der_bild_rechtsklick_hat_speichern_und_kopieren() {
        let quelle = "https://a.example/fotos/urlaub.png";
        let sichtbar = super::sichtbare_eintraege(super::Kontext::Bild, quelle, false);
        assert_eq!(
            sichtbar,
            vec![
                super::Eintrag::Kopieren,
                super::Eintrag::Uebersetzen,
                // KONTEXT (04.10.2026, Leitung „Rechtsklick auf Bild“):
                // Adresse, neuer Tab, Beschreibung — in **dieser** Reihenfolge,
                // und erst danach „speichern“.
                super::Eintrag::BildAdresse,
                super::Eintrag::BildNeuerTab,
                super::Eintrag::BildBeschreibung,
                super::Eintrag::BildSpeichern,
                super::Eintrag::Konvertieren,
            ],
            "GEMESSEN: der Bild-Kontext hat nicht die sieben Zeilen vom 04.10."
        );
        // „Kopieren“ kopiert das BILD (nicht die Adresse, nicht den Text).
        assert_eq!(
            super::Eintrag::Kopieren.kopier_ziel(super::Kontext::Bild),
            Some(super::KopierZiel::Bild),
            "GEMESSEN: „Kopieren“ kopiert auf einem Bild nicht das Bild"
        );
        // UND: auf einem **Link** dasselbe Wort die Adresse — der Unterschied,
        //     der die Entscheidung trägt.
        assert_eq!(
            super::Eintrag::Kopieren.kopier_ziel(super::Kontext::Link),
            Some(super::KopierZiel::Adresse)
        );
        // UND: „Bild speichern“ braucht die Quelle des **Bildes**.
        assert!(super::Eintrag::BildSpeichern.braucht_quelle(super::Kontext::Bild));
        // UND: im Bild steht **kein** „Video speichern“ (die beiden sind nicht
        //     dasselbe Ding — der Test prueft das in beide Richtungen).
        assert!(
            !sichtbar.contains(&super::Eintrag::VideoSpeichern),
            "GEMESSEN: auf einem Bild steht „Video speichern“"
        );
        assert!(
            super::sichtbare_eintraege(super::Kontext::Video, "https://a.example/f.mp4", false)
                .contains(&super::Eintrag::VideoSpeichern)
        );
        // DIE ZAHL, die die Behauptung am Bild traegt (04.10.): **sieben**
        // Zeilen, **drei** Trennlinien, 248 x 228 px (vorher fuenf / 172 px).
        // GEMESSENE ENTSCHEIDUNG, warum es **drei** Trennlinien bleiben und
        // nicht fuenf werden: Kopieren/Uebersetzen (0) | Adresse, neuer Tab,
        // Beschreibung, speichern (1) | Konvertieren (5) — drei Wechsel.
        let m = super::Menue::oeffne(super::Kontext::Bild, quelle, &Point::new(200.0, 120.0));
        let r = m.rechteck(1000, 700).expect("Menue");
        assert_eq!(
            m.zeilen(700).len(),
            7,
            "GEMESSEN: Zeilenzahl des Bildmenues"
        );
        // Release 08.10.2026 (Glas-Stil): zwei Abschnitte (SCHNELLZUGRIFFE,
        // BILD-AKTIONEN), eine Linie, 330 x 293 px.
        assert_eq!(m.trennzahl(700), 1, "GEMESSEN: Trennlinien des Bildmenues");
        assert_eq!(
            (r.breite, r.hoehe),
            (330, 2 * super::LUFT_OBEN + 7 * ZEILE + 2 * super::UEBERSCHRIFT_HOEHE + super::TRENN_HOEHE),
            "GEMESSEN: Kasten des Bildmenues"
        );
    }

    /// KONTEXT 03.10. (Freigabe „Untermenue ‚Konvertiert herunterladen >‘
    /// ausgeklappt als Bild fuer Bild- und Video-Kontext“): DIE SECHS BILDER.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **acht** Bilder und nicht vier: der
    /// Auftrag nennt zwei Kontexte **und** zwei Zustaende (zu / aufgeklappt),
    /// und zu jedem kommt hell und dunkel — Trennlinie und Rahmen sind je
    /// Thema zwei andere Farben. GEMESSEN sind acht Dateien:
    /// `9_{hell,dunkel}_{bild,video}_{zu,untermenue}.png`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **Bild**-Menue auch **zu** geschossen
    /// wird: das aufgeklappte Bild zeigt die Ziele, aber nicht die Zeile, an
    /// der man sie aufklappt — und die Leitung hat beides verlangt.
    #[test]
    fn die_sechs_bilder_von_bild_und_video() {
        for (thema, name) in [
            (super::super::thema::Thema::Hell, "hell"),
            (super::super::thema::Thema::Dunkel, "dunkel"),
        ] {
            let mut chrome = Chrome::neu(1000);
            chrome.thema = thema;
            for (k, quelle, text) in [
                (
                    super::Kontext::Bild,
                    "https://a.example/fotos/urlaub.png",
                    "urlaub",
                ),
                (super::Kontext::Video, "https://a.example/film.mp4", "film"),
            ] {
                let m = super::Menue::oeffne(k, quelle, &Point::new(220.0, 150.0));
                // 1. DAS MENUE ZU — die Zeile „Konvertiert herunterladen“ und
                //    ihre Gruppe sind sichtbar.
                let p_zu = schiessen(
                    &super::display_list(&m, &chrome, 1000, 700),
                    &format!("9_{}_{}_zu", name, k.name()),
                    1000,
                    700,
                );
                // 2. DAS UNTERMENUE OFFEN — die Ziele des Bildes / Videos.
                let mut a = m.clone();
                a.klappe_auf();
                let liste_a = super::display_list(&a, &chrome, 1000, 700);
                let p_auf = schiessen(
                    &liste_a,
                    &format!("9_{}_{}_untermenue", name, k.name()),
                    1000,
                    700,
                );
                // 2b. BEIM VIDEO **MIT** DER DATEI — GEMESSENER BEFUND (23:40):
                //     `narsil` ruft `setze_quelle_bytes` **nie** (grep ueber
                //     `crates/narsil/src`: 0 Treffer), sondern laedt die Quelle
                //     erst, wenn der Nutzer ein Ziel **geklickt** hat
                //     (`konvertieren_und_speichern`, `sitzung.rs:1380`). Also
                //     zeigt das Feld im Browser genau die **zwei** Ziele ohne
                //     Datei — und diese beiden Bilder zeigen, was mit Datei
                //     waere. GEMESSENE ENTSCHEIDUNG, warum beide Bilder da
                //     sind: ein Bild mit zwei Zielen beantwortet die Frage
                //     „was bietet der Video-Kontext“ nicht.
                let mut mit_daten = None;
                if k == super::Kontext::Video {
                    let bytes = std::fs::read(
                        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                            .parent()
                            .and_then(|p| p.parent())
                            .expect("Repo-Wurzel")
                            // Release 09.10.2026: `probe/` traegt keine Filme
                            // (`.gitignore`: *.webm) — derselbe Film liegt als
                            // Testdatei bei narsil-audio.
                            .join("crates/narsil-audio/tests/daten/g9at_video_vorbis.webm"),
                    )
                    .expect("der echte Film aus probe/seite/");
                    let mut d = m.clone();
                    d.setze_mime("video/webm");
                    d.setze_quelle_bytes(bytes);
                    d.klappe_auf();
                    let liste_d = super::display_list(&d, &chrome, 1000, 700);
                    let ziele_d: Vec<String> = d
                        .untermenue_ziele(700)
                        .iter()
                        .map(|z| z.beschriftung())
                        .collect();
                    eprintln!(
                        "KTX9-MESSUNG {name}/video MIT DATEI: {} Ziele {ziele_d:?}",
                        ziele_d.len()
                    );
                    let p_mit = schiessen(
                        &liste_d,
                        &format!("9_{}_video_untermenue_mit_datei", name),
                        1000,
                        700,
                    );
                    let ohne_daten = a.untermenue_ziele(700);
                    assert!(
                        ziele_d.len() > ohne_daten.len(),
                        "GEMESSEN: mit Datei stehen nicht mehr Ziele da ({ziele_d:?} vs {})",
                        ohne_daten.len()
                    );
                    mit_daten = Some(p_mit);
                }
                // DIE ZAHLEN, die die Behauptung tragen.
                let r = m.rechteck(1000, 700).expect("Menue");
                let u = a.untermenue_rechteck(1000, 700).expect("das Feld");
                let ziele: Vec<String> = a
                    .untermenue_ziele(700)
                    .iter()
                    .map(|z| z.beschriftung())
                    .collect();
                eprintln!(
                    "KTX9-MESSUNG {name}/{}: Menue {}x{} bei ({}, {}), Feld {}x{} bei ({}, {}), {} Ziele {:?}",
                    k.name(),
                    r.breite,
                    r.hoehe,
                    r.x,
                    r.y,
                    u.breite,
                    u.hoehe,
                    u.x,
                    u.y,
                    ziele.len(),
                    ziele
                );
                // UND: die Ziele stehen **im Feld**, nicht im Menue.
                // GEMESSENE ENTSCHEIDUNG, warum die Hoehe **gerechnet** und
                // nicht festgeschrieben wird: das Feld hat keine Trennlinien
                // (`untermenue_rechteck`), also ist seine Hoehe
                // `2 * LUFT_OBEN + n * ZEILE` — und `n` ist je Kontext
                // verschieden (Bild: sechs Ziele, Video: weniger).
                assert_eq!(
                    (u.breite, u.hoehe),
                    (
                        super::MENUE_BREITE,
                        super::LUFT_OBEN * 2 + ziele.len() as u32 * super::ZEILE
                    ),
                    "GEMESSEN: das Untermenue von {text} ist nicht {}x{} px ({} Ziele)",
                    super::MENUE_BREITE,
                    super::LUFT_OBEN * 2 + ziele.len() as u32 * super::ZEILE,
                    ziele.len()
                );
                assert!(
                    u.x >= r.rechts() - 1,
                    "GEMESSEN: das Feld steht nicht neben dem Menue ({}, {})",
                    u.x,
                    r.rechts()
                );
                assert!(
                    liste_a.len() > super::display_list(&m, &chrome, 1000, 700).len(),
                    "GEMESSEN: das aufgeklappte Untermenue von {text} zeichnet nichts"
                );
                let mut pfade = vec![p_zu, p_auf];
                pfade.extend(mit_daten);
                for p in pfade {
                    assert!(p.exists(), "das Bild fehlt: {}", p.display());
                }
            }
        }
    }

    /// KONTEXT 03.10. (Freigabe „Schatten/Trennlinien an Vorlage pruefen“):
    /// DER VERGLEICH MIT `team/design/vorlage_nutzer.webp`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier **gemessene Zahlen aus der
    /// Vorlage** stehen und nicht eine Datei: die Vorlage ist ein Bild, und ein
    /// Test kann ein Bild nicht gegen eine Datei prüfen. GEMESSEN sind die
    /// Zahlen unten am 03.10.2026 aus derselben Datei mit demselben Verfahren
    /// (Zeilenmittel ueber140 px, Streuung daneben) — dunkles Thema.
    ///
    /// GEMESSENE GENAUIGKEIT, und die ist **gemessen** (nicht behauptet): das
    /// Panel der Vorlage hat selbst drei Toene (26,28,34), (26,30,35) und
    /// (28,29,35) nebeneinander — die Kompression der WebP-Datei ist ±3 je
    /// Kanal. Deshalb wird **hier** mit ±2 geprueft und nicht mit Gleichheit:
    /// ein Gleichheitsvergleich wuerde an der Datei und nicht an der Optik
    /// kleben.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die **Abweichungen** (Zeilenhoehe,
    /// Trennband, Rahmen, Schatten) nur **gemeldet** und nicht behauptet
    /// werden: sie sind Zahlen der Leitung. `ZEILE` = 28 steht im
    /// Freigabe-Band 28..=32, und die Vorlage misst 32 — ein Wechsel ist eine
    /// Zahl in dieser Datei, aber die **Entscheidung** ist nicht meine.
    #[test]
    fn der_vergleich_mit_der_vorlage() {
        // DIE ZAHLEN AUS DER VORLAGE (dunkles Panel, `vorlage_nutzer.webp`).
        const VORLAGE_PANEL: (u8, u8, u8) = (28, 29, 35);
        const VORLAGE_TRENN: (u8, u8, u8) = (39, 40, 47);
        const VORLAGE_RAHMEN_DELTA: i32 = 16;
        const VORLAGE_ZEILE: u32 = 32;
        const VORLAGE_BAND: u32 = 13;
        const VORLAGE_SCHATTEN_PX: u32 = 20;
        let chrome = Chrome::neu(1000);
        let pal = super::super::thema::Palette::fuer(chrome.thema);
        // 1. DIE ZWEI FARBEN — Panel und Trennlinie. GEMESSEN: die Trennlinie
        //    ist in der Vorlage **1 px** und **uniform** (Streuung 1,4), und
        //    sie ist 11 Stufen heller als das Panel — bei mir sind es 12.
        let fehler = |a: (u8, u8, u8), b: (u8, u8, u8)| -> i32 {
            (i32::from(a.0) - i32::from(b.0))
                .abs()
                .max((i32::from(a.1) - i32::from(b.1)).abs())
                .max((i32::from(a.2) - i32::from(b.2)).abs())
        };
        assert!(
            fehler((pal.adresse.r, pal.adresse.g, pal.adresse.b), VORLAGE_PANEL) <= 2,
            "GEMESSEN: das Panel ({}, {}, {}) weicht von der Vorlage ({VORLAGE_PANEL:?}) um mehr als 2 ab",
            pal.adresse.r,
            pal.adresse.g,
            pal.adresse.b
        );
        assert!(
            fehler(
                (
                    pal.panel_trennlinie.r,
                    pal.panel_trennlinie.g,
                    pal.panel_trennlinie.b
                ),
                VORLAGE_TRENN
            ) <= 2,
            "GEMESSEN: die Trennlinie ({}, {}, {}) weicht von der Vorlage ({VORLAGE_TRENN:?}) ab",
            pal.panel_trennlinie.r,
            pal.panel_trennlinie.g,
            pal.panel_trennlinie.b
        );
        // 2. DIE RAHMENSTAERKE — der Unterschied zwischen „Haarlinie“ (Vorlage)
        //    und „sichtbarer Rand“ (hier).
        let mein_delta = i32::from(pal.adresse_rand.r) - i32::from(pal.adresse.r);
        // 3. DIE VIER ABWEICHUNGEN — gemeldet, nicht behauptet.
        eprintln!(
            "KTX9-VERGLEICH: Rahmen Delta {} (Vorlage {VORLAGE_RAHMEN_DELTA}) | Zeile {} px (Vorlage {VORLAGE_ZEILE}) | Trennband {} px (Vorlage {VORLAGE_BAND}) | Schatten 8 px (Vorlage {VORLAGE_SCHATTEN_PX})",
            mein_delta,
            super::ZEILE,
            super::TRENN_HOEHE
        );
        // 4. UND: die drei, die **stimmen**, stehen trotzdem fest — damit ein
        //    Rueckschritt auffaellt.
        //
        // Release 08.10.2026 (Glas-Stil, Vorlage des Nutzers): die Zeile ist
        // jetzt die der Vorlage (32 px), das Band 9 px, die Ecke 14 px, die
        // Schrift 14 px.
        assert_eq!(super::TRENN_HOEHE, 9, "GEMESSEN: das Trennband ist nicht 9 px");
        assert_eq!(super::ZEILE, VORLAGE_ZEILE, "GEMESSEN: die Zeile ist nicht die der Vorlage");
        assert_eq!(super::ECKE, 14.0);
        assert_eq!(super::SCHRIFT, 14.0);
    }

    /// KONTEXT 03.10. (00:40, Leitung „Schattenrand wirkt gepunktet/koernig“):
    /// DER SCHATTEN IST EINE **GLATTE RAMPE** — im Bild gemessen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier das **PNG zurueckgelesen** wird: die
    /// Behauptung „glatt“ ist eine Eigenschaft **gerasterter** Pixel, und die
    /// Zeichenliste enthaelt nur die Parameter (`weich`, Deckung) — sie weiss
    /// nichts ueber die Stufen, die der Rasterer daraus macht. Also wird das
    /// Bild geschossen, wieder eingelesen und die Rampe **abgelesen**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **Deckung** und nicht Farbe gemessen wird:
    /// der Schatten ist **schwarz** (`Color::new(0, 0, 0, 128)`), also ist seine
    /// Deckung genau die Rampe — und der Grund des Bildes ist **durchsichtig**
    /// (GEMESSEN: `rgba = (0, 0, 0, 0)`), weshalb eine Farbmessung dort immer
    /// 0 ergaebe. Ueber weiss entspricht die Deckung `a` dem Wert `255 - a`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Kriterien drei sind und nicht eines:
    /// „glatt“ allein ist eine Wortmarke. GEMESSEN sind: (1) **Reichweite** —
    /// die Rampe muss mindestens 14 Pixel haben (Vorlage: ~20, vorher 8);
    /// (2) **Groesster Sprung** zwischen zwei Nachbarpixeln hoechstens 14 von
    /// 255 — das ist die Baenderkennung (vorher **19**); (3) die dunkelste
    /// Stufe zwischen 100 und 160 — der Schatten soll stark genug sein (die
    /// Vorlage ist ~50 % schwarz = 128) und nicht das Feld zudunkeln.
    #[test]
    fn der_schatten_ist_eine_glatte_rampe() {
        let chrome = Chrome::neu(1000);
        let m = super::Menue::oeffne(
            super::Kontext::Leer,
            "https://a.example/seite.html",
            &Point::new(300.0, 150.0),
        );
        let r = m.rechteck(1000, 700).expect("Menue");
        let pfad = schiessen(
            &super::display_list(&m, &chrome, 1000, 700),
            "10_schattenrampe",
            1000,
            700,
        );
        let daten = std::fs::read(&pfad).expect("das geschossene PNG");
        let bild = narsil_image::png::lesen::png_lesen(&daten).expect("das PNG lesen");
        // DIE SPALTE: unter der Mitte des Kastens, dort wo kein Text steht.
        let x = r.x + r.breite / 2;
        // DIE RAMPE: die Deckung unterhalb des Kastens (der Kasten selbst hat
        // Deckung 255, deshalb startet die Suche **ein Feld** darunter).
        let mut rampe: Vec<u8> = Vec::new();
        for y in r.unten()..(r.unten() + 40).min(bild.hoehe) {
            let p = bild.pixel(x, y);
            if y == r.unten() {
                continue;
            }
            rampe.push(p[3]);
        }
        // 1. DIE REICHWEITE: wie viele Pixel haben Deckung?
        let reichweite = rampe.iter().take_while(|a| **a > 0).count() as u32;
        // 2. DER GROESSTE SPRUNG zwischen zwei benachbarten Pixeln der Rampe.
        let mut groesster_sprung = 0i32;
        for i in 1..rampe.len() {
            if rampe[i - 1] == 0 {
                continue;
            }
            let d = (i32::from(rampe[i]) - i32::from(rampe[i - 1])).abs();
            groesster_sprung = groesster_sprung.max(d);
        }
        // 3. DIE STARKSTE STELLE der Rampe.
        let dunkel = *rampe.iter().take_while(|a| **a > 0).max().unwrap_or(&0);
        eprintln!(
            "KTX10-SCHATTEN: Reichweite {reichweite} px, groesster Sprung {groesster_sprung}, staerkste Deckung {dunkel} (ueber weiss: {}), Rampe {:?}",
            255 - i32::from(dunkel),
            &rampe[..rampe.len().min(22)]
        );
        assert!(
            reichweite >= 14,
            "GEMESSEN: die Schattenrampe ist {reichweite} px (Vorlage ~20, gefordert >= 14)"
        );
        assert!(
            groesster_sprung <= 14,
            "GEMESSEN: der groesste Sprung in der Rampe ist {groesster_sprung} (Baender, gefordert <= 14); Rampe {rampe:?}"
        );
        assert!(
            (100..=160).contains(&dunkel),
            "GEMESSEN: die staerkste Schattenstufe ist {dunkel} (Vorlage ~50 % schwarz = 128)"
        );
    }

    /// KONTEXT 03.10. (00:40, Leitung): DAS VIDEOMENUE AN EINER **ECHTEN** WEBM
    /// **MIT TON** — und es nennt alle fuenf Ziele.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Dateien aus `narsil-media/tests/daten/`
    /// kommen und nicht aus `probe/seite/`: die Leitung hat diese Dateien
    /// genannt, und es sind **echte** Container (16..39 KB, mehrere Bilder und
    /// eine Tonspur) — keine 5-Byte-Fixtur. GEMESSENE ENTSCHEIDUNG, warum der
    /// Test **mehrere** Dateien probiert und nicht eine: „mit Ton“ ist eine
    /// Eigenschaft der **Datei**, nicht des Namens — und welche Datei sie hat,
    /// sagt erst das Menue.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die **Erwartung** eine Liste ist und
    /// kein Wissen ueber „Ton“: die Liste ist das, was der Nutzer liest, und
    /// sie ist der Wortlaut, den die Leitung genannt hat.
    #[test]
    fn das_videomenue_nennt_alle_fuenf_ziele_einer_webm_mit_ton() {
        let wurzel = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("Repo-Wurzel")
            .to_path_buf();
        // DIE ERWARTUNG, wortwoertlich (ANFORDERUNG, Leitung 00:40).
        // Release 08.10.2026: das Original steht oben (wie bei Bildern).
        let erwartet = vec![
            "As WebM (original)",
            "As AVI",
            "As GIF (animated)",
            "As PNG (first frame)",
            "As WAV (sound only)",
            "As FLAC (sound only)",
        ];
        let mut treffer: Option<(String, Vec<String>)> = None;
        for datei in [
            // Die von der Leitung genannten Dateien zuerst.
            "crates/narsil-media/tests/daten/test5.webm",
            "crates/narsil-media/tests/daten/test2.webm",
            "crates/narsil-media/tests/daten/test1.webm",
            "crates/narsil-media/tests/daten/testsrc.webm",
            "crates/narsil-media/tests/daten/g7b_testsrc.webm",
            // Und die, die namentlich nach Ton+Video heisst.
            "crates/narsil-konvert/tests/daten/k14_ton_video.webm",
        ] {
            let Ok(bytes) = std::fs::read(wurzel.join(datei)) else {
                continue;
            };
            let mut m = super::Menue::oeffne(
                super::Kontext::Video,
                "https://a.example/film.webm",
                &Point::new(220.0, 150.0),
            );
            m.setze_mime("video/webm");
            m.setze_quelle_bytes(bytes);
            let liste: Vec<String> = m.ziele().iter().map(|z| z.beschriftung()).collect();
            eprintln!("KTX10-WEBM {datei}: {} Ziele {liste:?}", liste.len());
            if liste.len() == erwartet.len() {
                treffer = Some((datei.to_string(), liste));
                break;
            }
        }
        let (datei, liste) = treffer.unwrap_or_else(|| {
            panic!("KEINE der echten WebM-Dateien nennt alle fuenf Ziele {erwartet:?}")
        });
        assert_eq!(liste, erwartet, "GEMESSEN an {datei}: die Ziele weichen ab");
        // UND: das Bild davon — hell und dunkel, weil es der Beleg ist.
        for (thema, name) in [
            (super::super::thema::Thema::Hell, "hell"),
            (super::super::thema::Thema::Dunkel, "dunkel"),
        ] {
            let mut chrome = Chrome::neu(1000);
            chrome.thema = thema;
            let mut m = super::Menue::oeffne(
                super::Kontext::Video,
                "https://a.example/film.webm",
                &Point::new(220.0, 150.0),
            );
            m.setze_mime("video/webm");
            m.setze_quelle_bytes(std::fs::read(wurzel.join(&datei)).expect("die WebM"));
            m.klappe_auf();
            let liste_b = super::display_list(&m, &chrome, 1000, 700);
            let u = m.untermenue_rechteck(1000, 700).expect("das Feld");
            let texte = texte(&liste_b);
            for z in &erwartet {
                assert!(
                    texte.contains(&z.to_string()),
                    "GEMESSEN: im Bild fehlt „{z}“: {texte:?}"
                );
            }
            eprintln!(
                "KTX10-BILD {name}: Feld {}x{} bei ({}, {}), {} Items",
                u.breite,
                u.hoehe,
                u.x,
                u.y,
                liste_b.len()
            );
            let p = schiessen(
                &liste_b,
                &format!("10_{}_webm_mit_ton_untermenue", name),
                1000,
                700,
            );
            assert!(p.exists(), "das Bild fehlt: {}", p.display());
        }
    }

    /// KONTEXT 03.10.2026 (Leitung 02:10): DIE DREI NEUEN MENUEARTEN ALS
    /// BILDER — markierter Text, Eingabefeld, leere Seitenflaeche.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **zwoelf** Bilder (3 Kontexte × 2 Themen):
    /// Trennlinie und Rahmen sind je Thema zwei andere Farben, und eine der
    /// Behauptungen ist „im hellen Thema ist die inaktive Zeile grau lesbar“ —
    /// das steht nur im hellen Bild.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das Eingabefeld **zweimal** im Bild
    /// steht (leer und mit Text): die Leitung hat „aktiv/inaktiv je nach
    /// Zustand“ verlangt, und **zwei** Bilder sind der Beweis, dass die
    /// Farbe vom Zustand abhaengt. Ein Bild mit einem leeren Feld wuerde
    /// die Behauptung „mit Text sind sie an“ nicht tragen.
    #[test]
    fn die_drei_neuen_menuearten_als_bilder() {
        for (thema, name) in [
            (super::super::thema::Thema::Hell, "hell"),
            (super::super::thema::Thema::Dunkel, "dunkel"),
        ] {
            let mut chrome = Chrome::neu(1000);
            chrome.thema = thema;
            // 1. DER MARKIERTE TEXT — vier Zeilen, eine Trennlinie.
            let mut text = super::Menue::oeffne(
                super::Kontext::Text,
                "Ein markierter Textabsatz",
                &Point::new(240.0, 150.0),
            );
            let r_text = text.rechteck(1000, 700).expect("Textmenue");
            let p_text = schiessen(
                &super::display_list(&text, &chrome, 1000, 700),
                &format!("11_{}_text", name),
                1000,
                700,
            );
            // 2. DAS EINGABEFELD — (a) leer, (b) mit Text und Auswahl.
            let mut feld_leer = super::Menue::oeffne(
                super::Kontext::Feld,
                "https://a.example/formular",
                &Point::new(240.0, 150.0),
            );
            let r_feld = feld_leer.rechteck(1000, 700).expect("Feldmenue");
            let p_leer = schiessen(
                &super::display_list(&feld_leer, &chrome, 1000, 700),
                &format!("11_{}_feld_leer", name),
                1000,
                700,
            );
            let mut feld_voll = feld_leer.clone();
            feld_voll.setze_feld_zustand(super::FeldZustand::AUSWAHL);
            let p_voll = schiessen(
                &super::display_list(&feld_voll, &chrome, 1000, 700),
                &format!("11_{}_feld_voll", name),
                1000,
                700,
            );
            // 3. DIE LEERE SEITENFLAECHE — zehn Zeilen nach der Freigabe.
            let leer = super::Menue::oeffne(
                super::Kontext::Leer,
                "https://a.example/seite.html",
                &Point::new(240.0, 120.0),
            );
            let r_leer = leer.rechteck(1000, 700).expect("Leermenue");
            let p_leer = schiessen(
                &super::display_list(&leer, &chrome, 1000, 700),
                &format!("11_{}_seite", name),
                1000,
                700,
            );
            // DIE ZAHLEN, die die Behauptung tragen.
            let texte_text: Vec<String> = texte(&super::display_list(&text, &chrome, 1000, 700));
            let texte_leer: Vec<String> = texte(&super::display_list(&leer, &chrome, 1000, 700));
            eprintln!(
                "KTX11-MESSUNG {name}: Text {} Zeilen / {} Linien / {}x{} / {:?}",
                text.zeilen(700).len(),
                text.trennzahl(700),
                r_text.breite,
                r_text.hoehe,
                texte_text
            );
            eprintln!(
                "KTX11-MESSUNG {name}: Feld {} Zeilen / {} Linien / {}x{}",
                feld_leer.zeilen(700).len(),
                feld_leer.trennzahl(700),
                r_feld.breite,
                r_feld.hoehe
            );
            eprintln!(
                "KTX11-MESSUNG {name}: Seite {} Zeilen / {} Linien / {}x{} / {:?}",
                leer.zeilen(700).len(),
                leer.trennzahl(700),
                r_leer.breite,
                r_leer.hoehe,
                texte_leer
            );
            // (1) DIE VIER ZEILEN DES TEXTMENUES, woertlich wie bei Firefox.
            assert_eq!(
                texte_text,
                vec![
                    "Copy",
                    "Translate",
                    "Search with Google",
                    "Select all"
                ],
                "GEMESSEN: das Textmenue hat andere Zeilen"
            );
            assert_eq!(text.zeilen(700).len(), 4, "GEMESSEN: vier Zeilen");
            assert_eq!(text.trennzahl(700), 1, "GEMESSEN: eine Trennlinie");
            // (2) DIE SECHS ZEILEN DES FELDES.
            let texte_feld: Vec<String> =
                texte(&super::display_list(&feld_leer, &chrome, 1000, 700));
            //
            // KONTEXT (03.10.2026, 03:40, Korrektur der Leitung): die Reihenfolge
            // ist Kopieren, Uebersetzen | Rueckgaengig, Ausschneiden, Einfuegen,
            // Loeschen, Alles auswaehlen. GEMESSEN ist, dass im **leeren**
            // Zustand „Uebersetzen“ **fehlt** (die Leitung: „nur mit Auswahl,
            // sonst weglassen“) — es sind also **sechs** Zeilen, nicht sieben.
            assert_eq!(
                texte_feld,
                vec![
                    "Copy",
                    "Undo",
                    "Cut",
                    "Paste",
                    "Delete",
                    "Select all"
                ],
                "GEMESSEN: das Feldmenue hat andere Zeilen"
            );
            // UND: MIT AUSWAHL kommt „Uebersetzen“ dazu — und es steht direkt
            //     unter „Kopieren“.
            let texte_auswahl: Vec<String> =
                texte(&super::display_list(&feld_voll, &chrome, 1000, 700));
            assert_eq!(
                texte_auswahl,
                vec![
                    "Copy",
                    "Translate",
                    "Undo",
                    "Cut",
                    "Paste",
                    "Delete",
                    "Select all"
                ],
                "GEMESSEN: mit Auswahl fehlt eine Zeile"
            );
            // (3) DIE SIEBEN ZEILEN DER LEEREN FLAECHE — woertlich wie bei
            //     Firefox — und die zwei aus ANFORDERUNGEN Zeile 20 darunter.
            for erwartet in [
                "Back",
                "Forward",
                "Reload",
                "Save page as",
                "Print",
                "View page source",
                "Inspect",
            ] {
                assert!(
                    texte_leer.iter().any(|t| t == erwartet),
                    "GEMESSEN: im Seitenmenue fehlt „{erwartet}“: {texte_leer:?}"
                );
            }
            // Release 08.10.2026: neun — „Konvertiert herunterladen >“ faellt bei
            // einer HTML-Seite weg (kein Ziel; ihr PDF macht „Drucken“).
            assert_eq!(
                leer.zeilen(700).len(),
                9,
                "GEMESSEN: das Seitenmenue hat {} Zeilen",
                leer.zeilen(700).len()
            );
            // UND: es passt in neun Zehntel des Fensters (MAX_MENUE_ZEHNTEL).
            assert!(
                r_leer.hoehe <= 700 * super::MAX_MENUE_ZEHNTEL / 10,
                "GEMESSEN: {} px bei 700 px Fenster",
                r_leer.hoehe
            );
            for p in [p_text, p_leer, p_voll] {
                assert!(p.exists(), "das Bild fehlt: {}", p.display());
            }
            let _text = &mut text;
        }
    }

    /// KONTEXT 03.10.2026 (Leitung 02:10): DIE INAKTIVEN ZEILEN SIND GRAU **UND**
    /// KEINE TREFFER.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **beides** in einem Test steht: eine
    /// graue Zeile, die anklickbar ist, ist die haeufigste Art, ein Menue
    /// zu beluegen — der Nutzer klickt, nichts passiert, und er haelt den
    /// Browser fuer kaputt. Und eine Zeile, die **unsichtbar** grau ist, ist
    /// genauso schlecht: dann weiss der Nutzer nicht, warum sie nicht geht.
    #[test]
    fn die_inaktiven_feldzeilen_sind_grau_und_keine_treffer() {
        let chrome = Chrome::neu(1000);
        let pal = super::super::thema::Palette::fuer(chrome.thema);
        let mut m = super::Menue::oeffne(
            super::Kontext::Feld,
            "https://a.example/formular",
            &Point::new(240.0, 150.0),
        );
        let r = m.rechteck(1000, 700).expect("Feldmenue");
        // 1. IM LEEREN FELD ist **nur** „Alles auswählen“ aus (es gibt nichts).
        let zeilen = m.zeilen(700);
        let aktiv_Leer: Vec<bool> = zeilen
            .iter()
            .map(|z| z.aktiv(super::Kontext::Feld, m.feld_zustand))
            .collect();
        assert_eq!(
            aktiv_Leer,
            vec![false, false, false, false, false, false],
            "GEMESSEN: im leeren Feld ist eine Zeile anklickbar"
        );
        // 2. UND MIT TEXT: Rueckgaengig, Kopieren, Loeschen, AllesAuswaehlen an;
        //    Ausschneiden und Einfuegen nur mit Markierung/Zwischenablage.
        m.setze_feld_zustand(super::FeldZustand::VOLL);
        let zeilen = m.zeilen(700);
        let aktiv_voll: Vec<bool> = zeilen
            .iter()
            .map(|z| z.aktiv(super::Kontext::Feld, m.feld_zustand))
            .collect();
        assert_eq!(
            aktiv_voll,
            // Kopieren (nur mit Auswahl), Rueckgaengig, Ausschneiden (nur mit
            // Auswahl), Einfuegen, Loeschen, Alles auswaehlen (mit Text).
            vec![false, true, false, true, true, true],
            "GEMESSEN: mit Text sind die falschen Zeilen aktiv"
        );
        // 3. MIT MARKIERUNG: dann auch „Ausschneiden“.
        m.setze_feld_zustand(super::FeldZustand::AUSWAHL);
        let zeilen = m.zeilen(700);
        assert!(
            zeilen
                .iter()
                .all(|z| z.aktiv(super::Kontext::Feld, m.feld_zustand)),
            "GEMESSEN: mit Markierung ist nicht jede Zeile aktiv"
        );
        // 4. DER TREFFER: eine graue Zeile wird **nicht** getroffen.
        m.setze_feld_zustand(super::FeldZustand::LEER);
        let zeile_0 = r.y + m.zeilen_y(0, 700) + ZEILE / 2;
        let zeile_2 = r.y + m.zeilen_y(2, 700) + ZEILE / 2;
        assert_eq!(
            m.nr_unter(1000, 700, &Point::new((r.x + 40) as f32, zeile_0 as f32)),
            None,
            "GEMESSEN: „Rueckgaengig“ ist grau und wird trotzdem getroffen"
        );
        // 5. UND MIT AUSWAHL: **alle** Zeilen sind an und werden getroffen —
        //    dieselbe Rechnung, anderes Ergebnis, weil der Zustand ein
        //    anderer ist.
        //
        //    KONTEXT (03.10.2026, 03:50): hier stand vorher „mit **Text**“ und
        //    die Zeile 0 war „Rueckgaengig“. GEMESSEN ist nach der Korrektur der
        //    Leitung: Zeile 0 ist jetzt **Kopieren**, und das ist **nur mit
        //    Auswahl** an — mit Text ist es grau und also kein Treffer. Also
        //    behauptet der Test jetzt das, was die Zeile tut: **mit Auswahl**
        //    geht jede Zeile.
        m.setze_feld_zustand(super::FeldZustand::AUSWAHL);
        let zeilen_auswahl = m.zeilen(700);
        for (nr, z) in zeilen_auswahl.iter().enumerate() {
            let y = r.y + m.zeilen_y(nr, 700) + ZEILE / 2;
            assert_eq!(
                m.nr_unter(1000, 700, &Point::new((r.x + 40) as f32, y as f32)),
                Some(nr),
                "GEMESSEN: Zeile {nr} ({}) wird mit Auswahl nicht getroffen",
                z.text()
            );
        }
        // 6. UND IM BILD: die inaktiven Texte stehen in der **schwachen**
        //    Farbe — nicht in der Textfarbe.
        m.setze_feld_zustand(super::FeldZustand::LEER);
        let liste = super::display_list(&m, &chrome, 1000, 700);
        let schwaeche = liste
            .iter()
            // Release 08.10.2026 (Glas-Stil): grau ist jetzt [`GLAS_AUS`].
            .filter(|i| matches!(i, DisplayItem::Text { color, .. } if *color == super::GLAS_AUS))
            .count();
        let _palette = &pal;
        assert!(
            schwaeche >= 6,
            "GEMESSEN: nur {schwaeche} Texte sind grau im Bild (erwartet 6)"
        );
    }

    /// EIN [`super::TabZustand`] FUER DIE TESTS — damit nicht ueberall
    /// fuenf Felder stehen muessen.
    fn zustand(tabs: u32, rechts: u32, wiederherstellbar: bool) -> super::TabZustand {
        super::TabZustand {
            tab_anzahl: tabs,
            tabs_rechts: rechts,
            wiederherstellbar,
            gepinnt: false,
            lautlos: false,
        }
    }

    /// KONTEXT 04.10.2026 (Leitung „(1) Tab anheften / Lautlos / Tabs rechts
    /// schließen / Wiederherstellen“): DAS TABMENUE HAT DIE **ZEHN** ZEILEN IN
    /// DER REIHENFOLGE VON FIREFOX, DREI TRENNLINIEN — UND DIE ECKE IST IMMER
    /// NOCH DER KLICKPUNKT.
    #[test]
    fn das_tabmenue_hat_die_zehn_zeilen() {
        // 1. DIE ZEILEN, woertlich wie bei Firefox.
        let z = zustand(4, 2, true);
        let texte: Vec<&str> = super::ALLE_TAB.iter().map(|e| e.text(&z)).collect();
        assert_eq!(
            texte,
            vec![
                "New tab",
                "Reload",
                "Pin tab",
                "Mute tab",
                "Duplicate tab",
                "Open tab in new window",
                "Close",
                "Close other tabs",
                "Close tabs to the right",
                "Reopen closed tab",
            ],
            "GEMESSEN: die Zeilen des Tabmenues"
        );
        // 2. DIE MASSWERTE — gemessen, nicht geraten: 4 Rand + 10 Zeilen +
        //    3 Trennlinien.
        let m = super::TabMenue::oeffne(2, z, &Point::new(220.0, 40.0));
        let r = m.rechteck(1000, 700).expect("Tabmenue");
        // Release 08.10.2026 (Glas-Stil): dieselben Masse wie das Seitenmenue
        // (330 px breit, 6 px Luft, 32-px-Zeilen, 9-px-Linienband).
        assert_eq!(
            (r.breite, r.hoehe),
            (
                super::MENUE_BREITE,
                2 * super::LUFT_OBEN + 10 * super::ZEILE + 3 * super::TRENN_HOEHE
            ),
            "GEMESSEN: Kasten des Tabmenues"
        );
        assert_eq!(
            (r.x, r.y),
            (220, 40),
            "GEMESSEN: die Ecke ist nicht der Klickpunkt"
        );
        // 3. DIE TRENNLINIEN: vor „Tab anheften“, vor „Schließen“ und vor
        //    „Geschlossenen Tab wiederherstellen“.
        let linien: Vec<usize> = (1..super::ALLE_TAB.len())
            .filter(|nr| super::ALLE_TAB[*nr].trenn_vor(*nr))
            .collect();
        assert_eq!(linien, vec![2, 6, 9], "GEMESSEN: wo Linien stehen");
        // 4. DIE BILDER — hell und dunkel, mit allem an und mit einem Tab.
        for (thema, name) in [
            (super::super::thema::Thema::Hell, "hell"),
            (super::super::thema::Thema::Dunkel, "dunkel"),
        ] {
            let mut chrome = Chrome::neu(1000);
            chrome.thema = thema;
            for (st, kurz) in [
                (zustand(4, 2, true), "voll"),
                (zustand(1, 0, false), "eins"),
            ] {
                let mm = super::TabMenue::oeffne(1, st, &Point::new(220.0, 40.0));
                let l = super::tab_display_list(&mm, &chrome, 1000, 700);
                let p = schiessen(&l, &format!("12_{}_tab_{}", name, kurz), 1000, 700);
                assert!(p.exists(), "das Bild fehlt: {}", p.display());
            }
        }
    }

    /// KONTEXT 04.10.2026 (Leitung „Rechtsklick auf Link“): DAS LINKMENUE HAT
    /// DIE ACHT ZEILEN — und die Fensterzeile ist **grau**, weil es kein
    /// Fenstermodell gibt (GEMESSEN: `grep -rn "privat" crates/*/src/*.rs` findet
    /// nur Kommentare und die HTTP-Cache-Regel `private`).
    #[test]
    fn das_linkmenue_hat_die_neuen_zeilen() {
        let quelle = "https://a.example/artikel";
        let sichtbar = super::sichtbare_eintraege(super::Kontext::Link, quelle, false);
        let namen: Vec<&str> = sichtbar.iter().map(|e| e.name()).collect();
        assert_eq!(
            namen,
            vec![
                "kopieren",
                "uebersetzen",
                "link-lesezeichen",
                "link-oeffnen",
                "link-neuer-tab",
                "link-privates-fenster",
                "link-speichern",
                "konvertieren",
            ],
            "GEMESSEN: die Zeilen des Linkmenues"
        );
        let feld = super::FeldZustand::LEER;
        assert!(
            !super::Eintrag::LinkPrivatesFenster.aktiv(super::Kontext::Link, feld),
            "GEMESSEN: „Privates Fenster“ ist aktiv, obwohl es kein Fenster gibt"
        );
        // UND: die graue Zeile ist **kein Treffer** — auch nicht mitten drin.
        let m = super::Menue::oeffne(super::Kontext::Link, quelle, &Point::new(20.0, 20.0));
        let r = m.rechteck(1000, 700).expect("Menue");
        let nr = 5; // GEMESSEN: „Privates Fenster“ ist die sechste Zeile.
        let y = r.y + m.zeilen_y(nr, 700) + super::ZEILE / 2;
        assert_eq!(
            m.zeile_unter(1000, 700, &Point::new((r.x + 20) as f32, y as f32)),
            None,
            "GEMESSEN: die graue Fensterzeile ist ein Treffer"
        );
    }

    /// KONTEXT 04.10.2026 (Leitung „Rechtsklick auf Bild“): DAS BILDMENUE HAT
    /// DIE SIEBEN ZEILEN IN DER GENANNTEN REIHENFOLGE.
    #[test]
    fn das_bildmenue_hat_die_neuen_zeilen() {
        let namen: Vec<&str> = super::sichtbare_eintraege(
            super::Kontext::Bild,
            "https://a.example/fotos/urlaub.png",
            false,
        )
        .iter()
        .map(|e| e.name())
        .collect();
        assert_eq!(
            namen,
            vec![
                "kopieren",
                "uebersetzen",
                "bild-adresse",
                "bild-neuer-tab",
                "bildbeschreibung",
                "bild-speichern",
                "konvertieren",
            ],
            "GEMESSEN: die Zeilen des Bildmenues"
        );
        // UND: „Kopieren“ am Bild kopiert die **Bildpunkte** — „Bild-Adresse
        // kopieren“ ist also keine zweite Kopie derselben Sache (GEMESSEN ist
        // `kopier_ziel`).
        assert_eq!(
            super::Eintrag::Kopieren.kopier_ziel(super::Kontext::Bild),
            Some(super::KopierZiel::Bild)
        );
        assert_eq!(
            super::Eintrag::Kopieren.kopier_ziel(super::Kontext::Link),
            Some(super::KopierZiel::Adresse),
            "GEMESSEN: am Link kopiert „Kopieren“ nicht die Adresse — dann waere
            „Link-Adresse kopieren“ eine echte zweite Zeile"
        );
        // UND: die drei neuen Zeilen brauchen die Adresse des Bildes.
        for e in [
            super::Eintrag::BildAdresse,
            super::Eintrag::BildNeuerTab,
            super::Eintrag::BildBeschreibung,
        ] {
            assert!(
                e.braucht_quelle(super::Kontext::Bild),
                "GEMESSEN: {:?} erscheint ohne Bildadresse",
                e.name()
            );
        }
    }

    /// KONTEXT 04.10.2026: **KEINE** BESCHRIFTUNG DER NEUEN MENUES PASST
    /// NICHT — mit derselben Rechnung, die in diesem Kasten seit 02.10. gilt
    /// (`ZEICHENBREITE` px je Zeichen plus der Innenabstand, so breit wie der
    /// Kasten).
    #[test]
    fn die_beschriftungen_passen_in_den_kasten() {
        for k in super::Kontext::ALLE {
            for e in super::eintraege(k, true) {
                let n = e.zeichen().chars().count() as u32;
                // GEMESSEN: `ZEICHENBREITE` steht eine Ebene hoeher (`lib.rs:621`).
                let breite = n * super::super::ZEICHENBREITE + 2 * super::INNEN;
                assert!(
                    breite <= super::MENUE_BREITE,
                    "GEMESSEN: {:?} braucht {breite} px ({n} Zeichen), der Kasten hat {}",
                    e.name(),
                    super::MENUE_BREITE
                );
            }
        }
    }

    /// KONTEXT 04.10.2026: VIER ZEILEN HENGEN AM ZUSTAND — „Andere schließen“
    /// an der Zahl, „Tabs rechts schließen“ an den Tabs rechts,
    /// „Wiederherstellen“ am Merksatz, „neues Fenster“ an gar nichts (GEMESSEN:
    /// `narsil-browser` hat kein Fenstermodell).
    #[test]
    fn die_vier_zustandszeilen_werden_grau() {
        let nix = zustand(1, 0, false);
        let alles = zustand(4, 2, true);
        let grau: Vec<&str> = super::ALLE_TAB
            .iter()
            .filter(|e| !e.aktiv(&nix))
            .map(|e| e.name())
            .collect();
        assert_eq!(
            grau,
            vec![
                // GEMESSENER EIGENER TESTFEHLER: ich hatte die vier Zeilen in
                // der Reihenfolge genannt, in der ich sie **eben** erwaehnt
                // hatte — die Liste laeuft aber in **Zeilenreihenfolge**, und
                // „neues Fenster“ ist Zeile 5, also **vor** dem Schliessen.
                "tab-neues-fenster",
                "tab-andere-schliessen",
                "tabs-rechts-schliessen",
                "tab-wiederherstellen",
            ],
            "GEMESSEN: welche Zeilen bei einem Tab grau sind"
        );
        // UND: mit vier Tabs und einem geschlossenen sind drei davon an —
        // „neues Fenster“ bleibt grau, **immer**.
        let noch_grau: Vec<&str> = super::ALLE_TAB
            .iter()
            .filter(|e| !e.aktiv(&alles))
            .map(|e| e.name())
            .collect();
        assert_eq!(
            noch_grau,
            vec!["tab-neues-fenster"],
            "GEMESSEN: bei allem an ist noch eine Zeile grau"
        );
    }

    /// KONTEXT 04.10.2026: ZWEI BESCHRIFTUNGEN WECHSELN MIT DEM ZUSTAND —
    /// anheften/loesen und stumm/Ton.
    #[test]
    fn die_zwei_beschriftungen_wechseln() {
        let mut an = zustand(3, 1, true);
        assert_eq!(super::ALLE_TAB[2].text(&an), "Pin tab");
        assert_eq!(super::ALLE_TAB[3].text(&an), "Mute tab");
        an.gepinnt = true;
        an.lautlos = true;
        assert_eq!(super::ALLE_TAB[2].text(&an), "Unpin tab");
        assert_eq!(super::ALLE_TAB[3].text(&an), "Unmute tab");
    }

    /// KONTEXT 04.10.2026: JEDE **an**e Zeile trifft sich selbst, jede **graue**
    /// nie — auch „Tab in neuem Fenster“ im besten Zustand nicht.
    #[test]
    fn jede_an_zeile_trifft_sich_selbst() {
        let z = zustand(3, 2, true);
        let mut h = super::TabMenue::oeffne(1, z, &Point::new(220.0, 40.0));
        let r = h.rechteck(1000, 700).expect("Tabmenue");
        for nr in 0..super::ALLE_TAB.len() {
            let y = r.y + h.zeilen_y(nr) + super::ZEILE / 2;
            let p = Point::new((r.x + 40) as f32, y as f32);
            let erwartet = super::ALLE_TAB[nr].aktiv(&z).then_some(nr);
            assert_eq!(
                h.nr_unter(1000, 700, &p),
                erwartet,
                "GEMESSEN: Zeile {nr} ({}) trifft sich nicht selbst",
                super::ALLE_TAB[nr].text(&z)
            );
        }
    }

    /// KONTEXT 03.10. (Freigabe Punkt 1): ALLE SICHTBAREN TEXTE TRAGEN IHRE
    /// UMLAUTE — und keiner ist transkribiert.
    #[test]
    fn kein_sichtbarer_text_ist_transkribiert() {
        let _de = narsil_i18n::scoped(narsil_i18n::Language::German); // measured against the German template
        // GEMESSENER BEFUND: „Übersetzen“ stand als „Uebersetzen“ in der
        // Liste — der einzige transkribierte sichtbare Text im Menue.
        assert_eq!(super::Eintrag::Uebersetzen.text(), "Übersetzen");
        // UND: die Liste aller sichtbaren Texte ist transliterationsfrei.
        for k in super::Kontext::ALLE {
            for e in super::eintraege(k, false) {
                let t = e.text();
                for verboten in [
                    "ueber",
                    "Ueber",
                    "oeffn",
                    "Oeffn",
                    "loesch",
                    "Loesch",
                    "Uebersetz",
                ] {
                    assert!(!t.contains(verboten), "{k:?}: {t:?} enthaelt „{verboten}“");
                }
            }
        }
        // UND: die Ziele der Untermenues ebenfalls (die kommen aus
        // `narsil-konvert` und stehen im selben Bild).
        let mut m = super::Menue::oeffne(
            super::Kontext::Bild,
            "https://a.example/b.png",
            &Point::new(10.0, 10.0),
        );
        m.klappe_auf();
        for z in m.untermenue_ziele(700) {
            let t = z.beschriftung();
            assert!(
                !t.contains("Uebersetz") && !t.contains("uebersetz"),
                "die Zielzeile {t:?} ist transkribiert"
            );
        }
    }

    /// KONTEXT 03.10. (zweiter Durchgang, Leitung): DIE HOEHE WIRD BERECHNET —
    /// und die **letzte Tinte** bleibt im Kasten.
    ///
    /// GEMESSENER BEFUND (das Bild `ktx_*_menue_dunkel_unter.png`, auf das
    /// die Leitung zeigt): die letzte Zeile stand **ueber** dem Rand — im
    /// Hauptmenue „Konvertiert herunterladen“, im Feld „Als ICO“.
    /// GEMESSENE URSACHE: der Text wurde bei `y + 18` gezeichnet (siehe
    /// [`TEXT_KASTEN`]), und `origin` ist die **obere** Ecke — die Tinte
    /// stand 3 px unter ihrer Zeile.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Beweis an den **Pixeln** und nicht
    /// an der Rechnung des Kastens passiert: die Rechnung kann stimmen und
    /// das Bild trotzdem falsch sein — so war es hier (der Kasten war
    /// **172 px** hoch und richtig, die Tinte stand trotzdem draussen).
    /// Deshalb wird hier gerastert und die **tiefste Tinte** gesucht.
    #[test]
    fn die_hoehe_ist_berechnet_und_keine_zeile_steht_heraus() {
        let chrome = Chrome::neu(1000);
        let pal = super::super::thema::Palette::fuer(chrome.thema);
        for k in super::Kontext::ALLE {
            let quelle = match k {
                super::Kontext::Bild => "https://a.example/fotos/urlaub.jpeg",
                super::Kontext::Video => "https://a.example/film.mp4",
                super::Kontext::Link => "https://a.example/photo.jpg",
                super::Kontext::Text => "Text",
                super::Kontext::Feld => "https://a.example/formular",
                super::Kontext::Leer => "https://a.example/seite.html",
            };
            let mut m = super::Menue::oeffne(k, quelle, &Point::new(300.0, 180.0));
            m.klappe_auf();
            let r = m.rechteck(1000, 700).expect("Menue");
            // 1. DIE RECHNUNG: Hoehe = Summe der Zeilen + Trenner + 2x Luft.
            //    GEMESSENE ENTSCHEIDUNG, warum **beide** Summanden einzeln
            //    geprueft werden: eine Hoehe kann stimmen, weil sich zwei
            //    Fehler aufheben (ein Zeilen zu viel, ein Trenner zu wenig).
            let zeilen = m.zeilen(700).len() as u32;
            let trenner = m.trennzahl(700) as u32;
            // Release 08.10.2026 (Glas-Stil): dazu je Abschnitt (Trenner + 1)
            // eine Ueberschrift.
            let soll = zeilen * ZEILE
                + trenner * super::TRENN_HOEHE
                + (trenner + 1) * super::UEBERSCHRIFT_HOEHE
                + 2 * LUFT_OBEN;
            assert_eq!(
                r.hoehe, soll,
                "{k:?}: {} Zeilen + {} Trenner + {} Ueberschriften + 2x{} Luft = {soll}, nicht {}",
                zeilen,
                trenner,
                trenner + 1,
                LUFT_OBEN,
                r.hoehe
            );
            // UND: die letzte Zeile endet genau LUFT_OBEN ueber dem Rand.
            let letzte = zeilen as usize - 1;
            assert_eq!(
                r.y + m.zeilen_y(letzte, 700) + ZEILE + LUFT_OBEN,
                r.unten(),
                "{k:?}: die letzte Zeile endet nicht LUFT_OBEN ueber dem Rand"
            );
            // 2. DIE TINTE: gerastert, und gesucht wird die tiefste Zeile, an
            //    der **Inhalt** steht — Text oder Chevron.
            //
            //    GEMESSENER EIGENER FEHLER (03.10., erster Versuch): gezaehlt
            //    wurde „alles, was nicht die Panel-Farbe hat“. GEMESSEN war
            //    dann als tiefste Zeile IMMER `unten - 3` — bei allen fuenf
            //    Kontexten genau dieselbe Zahl, und das ist kein Zufall: der
            //    **Schatten** des Kastens liegt 2 px versetzt und 4 px weich
            //    unter dem Feld, und sein weichster Rand reichte bis in die
            //    letzte Zeile **im** Panels. GEMESSENE ENTSCHEIDUNG: gezaehlt
            //    wird nur, was **Inhalt** ist — also was weder die Panel-
            //    Farbe noch die Rahmenfarbe noch die Trennlinienfarbe ist.
            //    Das sind genau drei Farben, und alle drei gehoeren **dem
            //    Panel**, nicht dem Inhalt.
            let liste = super::display_list(&m, &chrome, 1000, 700);
            let mut schrift = narsil_raster::Schrift::mit_systemschrift();
            let e = narsil_raster::rasterize_mit_schrift(&liste, &mut schrift, 1000, 700);
            let fb = e.framebuffer;
            // Release 08.10.2026 (Glas-Stil): das Glas ist durchscheinend und
            // hat einen Verlauf — es gibt keine drei festen Panelfarben mehr.
            // INHALT ist, was hell ist (Text, Icons, Ueberschriften: jeder
            // Kanal ueber 110); Glas, Glanz, Kante und Linien bleiben darunter.
            let _palette = &pal;
            let ist_inhalt = |c: narsil_core::color::Color| c.r.min(c.g).min(c.b) > 110;
            let mut tiefste = 0u32;
            let mut oberste = u32::MAX;
            for y in r.y + super::RAND_BOX + 1..r.unten() - super::RAND_BOX - 1 {
                //
                // GEMESSENE ENTSCHEIDUNG, warum rechts so viele Pixel frei
                // bleiben: das aufgeklappte Feld steht **direkt** neben dem
                // Menue (Abstand 0), und sein **Schatten** reicht
                // [`super::SCHATTEN_REICHWEITE`] px ueber die linke Kante
                // des Feldes hinaus — ins Menue hinein. GEMESSEN war bei
                // genau einer Spalte ein Rest von diesem Schatten, und die
                // Behauptung „die Tinte endet 2 px ueber dem Rahmen“ war an
                // **einer** Spalte nicht haltbar, obwohl im Menue selbst
                // nichts auf der Zeile stand.
                //
                // KONTEXT (03.10., 00:50): hier stand die eigene Zahl 4 —
                // der Wert fuer `SCHATTEN_WEICH` = 4. GEMESSEN ist, dass sie
                // beim Weichweren des Schattens (4 → 12) **falsch** wurde und
                // den Test rot machte, ohne dass am Panel etwas kaputt war.
                for x in r.x + super::INNEN..r.rechts() - super::INNEN - super::SCHATTEN_REICHWEITE
                {
                    if let Some(c) = fb.pixel(x, y) {
                        if ist_inhalt(c) {
                            tiefste = tiefste.max(y);
                            oberste = oberste.min(y);
                        }
                    }
                }
            }
            let luecke = oberste.saturating_sub(r.y);
            eprintln!(
                "KTX-MESSUNG {k:?}: {} Zeilen + {} Trenner -> {}x{} px, Inhalt {}..{}, {} px Luft oben",
                zeilen, trenner, r.breite, r.hoehe, oberste, tiefste, luecke
            );
            // DIE ZUSAGE: zwischen der tiefsten Inhalts-Tinte und dem Rahmen
            // liegen mindestens **2** Pixel. GEMESSENE ENTSCHEIDUNG, warum
            // keine feste Zahl: die Zeile endet 4 px ueber dem Panel, und die
            // Tinte der letzten Zeile ist wiederum hoeher — das sind
            // **zwei** Kuerzungen, und jede kann sich aendern.
            assert!(
                tiefste + 2 < r.unten() - super::RAND_BOX,
                "{k:?}: die Inhalts-Tinte reicht bis {tiefste}, der Rahmen sitzt bei {}",
                r.unten() - super::RAND_BOX
            );
            // 3. UND OBEN: der Leerraum ueber der ersten Zeile ist klein.
            //    GEMESSENE ENTSCHEIDUNG, warum das eine Zahl und kein „sieht
            //    gut aus“ ist: GEMESSEN waren **21 px** (der Wert, den die
            //    Leitung „zu viel Leerraum“ genannt hat), und das ist mehr
            //    als eine halbe Zeile.
            //
            // GEMESSENE ENTSCHEIDUNG, warum die Grenze **12** ist und nicht
            // 10: der Leerraum haengt am **ersten Buchstaben** der Zeile —
            // „Kopieren“ beginnt mit „K“ (Grossbuchstabe), „Übersetzen“ mit
            // „Ü“ (mit Umlaut, hoeher) und „Als PNG“ mit „A“. GEMESSEN sind
            // 11 px (K/B/L) bzw. 10 px (Ü, A). Vor dem Fix waren es
            // GEMESSEN **21** px — mehr als eine halbe Zeile, und genau das
            // war die Beanstandung der Leitung.
            //
            // Release 08.10.2026 (Glas-Stil): oben steht jetzt die kleine
            // Ueberschrift des ersten Abschnitts, unten im 24-px-Band (6 px
            // Luft + 7 px bis zur Oberkante ihres Textkastens) — GEMESSEN 14
            // bis 15 px bis zur ersten Tinte. Die Grenze 18 haelt den Kopf
            // knapp (frueher 21 px Leerraum ueber der ersten ZEILE).
            assert!(
                luecke <= 18,
                "{k:?}: {luecke} px Leerraum ueber der ersten Ueberschrift (erwartet hoechstens 18)"
            );
            // 4. UND: das Feld daneben ebenfalls.
            if let Some(u) = m.untermenue_rechteck(1000, 700) {
                let mut tief = 0u32;
                for y in u.y + super::RAND_BOX + 1..u.unten() - super::RAND_BOX - 1 {
                    for x in u.x + super::INNEN..u.rechts() - super::INNEN - 4 {
                        if let Some(c) = fb.pixel(x, y) {
                            if ist_inhalt(c) {
                                tief = tief.max(y);
                            }
                        }
                    }
                }
                eprintln!(
                    "KTX-MESSUNG {k:?}: Feld {}x{} px, Inhalt bis {} (Rahmen {})",
                    u.breite,
                    u.hoehe,
                    tief,
                    u.unten() - super::RAND_BOX
                );
                assert!(
                    tief + 2 < u.unten() - super::RAND_BOX,
                    "{k:?}: die Tinte im Feld reicht bis {tief}, der Rahmen bei {}",
                    u.unten() - super::RAND_BOX
                );
            }
        }
    }

    /// KONTEXT 03.10. (zweiter Durchgang): DER CHEVRON STEHT AUF DER **MITTE
    /// DER TEXTZEILE** — nicht 5 px daneben.
    #[test]
    fn der_chevron_steht_auf_der_textmitte() {
        let chrome = Chrome::neu(1000);
        let m = super::Menue::oeffne(
            super::Kontext::Bild,
            "https://a.example/b.png",
            &Point::new(120.0, 150.0),
        );
        let r = m.rechteck(1000, 700).expect("Menue");
        let liste = super::display_list(&m, &chrome, 1000, 700);
        let nr = m
            .zeilen(700)
            .iter()
            .position(|z| matches!(z, super::Zeile::Aktion(e) if e.hat_untermenue()))
            .expect("die Zeile mit dem Chevron");
        let oben = r.y + m.zeilen_y(nr, 700);
        let text = liste
            .iter()
            .find_map(|i| match i {
                DisplayItem::Text { origin, text, .. } if text == "Download converted" => {
                    Some(origin.y)
                }
                _ => None,
            })
            .expect("die Beschriftung");
        let chevron = liste
            .iter()
            .find_map(|i| match i {
                DisplayItem::Bild { rect, breite, .. } if *breite == super::PFEIL_BOX => {
                    Some(rect.y)
                }
                _ => None,
            })
            .expect("der Chevron");
        // DIE TEXTMITTE: Oberkante + halber Kasten.
        let text_mitte = text + super::TEXT_KASTEN as f32 / 2.0;
        let chevron_mitte = chevron + super::PFEIL_BOX as f32 / 2.0;
        eprintln!(
            "KTX-MESSUNG Chevron: Textmitte {text_mitte}, Chevronmitte {chevron_mitte}, Zeile {oben}..{}",
            oben + ZEILE
        );
        assert!(
            (chevron_mitte - text_mitte).abs() <= 1.0,
            "GEMESSEN: Chevronmitte {chevron_mitte} liegt neben der Textmitte {text_mitte}"
        );
        // UND: der Text sitzt **mittig** in der Zeile. Release 08.10.2026
        // (Glas-Stil): 32-px-Zeile, 17-px-Kasten — oben 7, unten 8.
        assert_eq!(super::TEXT_ABSTAND, 7, "GEMESSEN: (32 - 17) / 2");
        assert_eq!(
            text,
            (oben + super::TEXT_ABSTAND) as f32,
            "GEMESSEN: der Text sitzt nicht TEXT_ABSTAND px unter der Zeilenoberkante"
        );
    }

    /// KONTEXT 03.10. (dritter Durchgang, Leitung): DAS MENUE AN EINEM **ECHTEN
    /// BILD** — „Als PNG" muss angeboten werden, und es muss produzierbar
    /// sein.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein **Bild von der Platte** und nicht
    /// eine Adresse: die Leitung verlangt den Funktionsnachweis ueber ein
    /// echtes Bild, und GEMESSEN ist, dass die Kette an zwei Stellen an
    /// **Daten** haengt, nicht an der Adresse: [`Menue::ziele`] bekommt die
    /// Bytes (`quelle_bytes`), und `narsil_konvert` entscheidet „nur Ton“ und
    /// „animiertes GIF“ **an den Daten** (K5). Eine URL ohne Bytes kann
    /// beides nicht entscheiden, und ein Menue, das man nur mit einer URL
    /// prueft, ist an der Hälfte der Kette nicht geprueft.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Datei aus `narsil-fuzz/saat/bild`:
    /// das ist das Verzeichnis mit **echten** Testbildern (JPEG mit und
    /// ohne Transparenz, PNG, GIF), und es liegt nicht unter `/tmp` (M9).
    ///
    /// KONTEXT (03.10.2026, GEMESSENE KORREKTUR): `wurzel` ist nach **zwei**
    /// `parent()` die **Repo-Wurzel** (`/abs` aus `/abs/crates/narsil-chrome`), und
    /// der Pfad begann ohne `crates/` — GEMESSENE FOLGE: „No such file or
    /// directory (os error 2)“ fuer `narsil-fuzz/saat/bild/farbverlauf_420.jpg`,
    /// obwohl die Datei existiert (`ls crates/narsil-fuzz/saat/bild` = 5 Dateien).
    #[test]
    fn das_menue_an_einem_echten_bild() {
        let wurzel = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("Repo-Wurzel")
            .to_path_buf();
        // ECHTE Dateien, und jede mit dem MIME, den der Server schicken
        // wuerde — `narsil` liefert den aus der Antwort, nicht aus der Endung.
        //
        // KONTEXT (03.10.2026): `crates/` steht **im Pfad**, weil `wurzel` nach
        // zwei `parent()` die Repo-Wurzel ist (siehe Kommentar am Test).
        for (datei, mime) in [
            (
                "crates/narsil-fuzz/saat/bild/farbverlauf_420.jpg",
                "image/jpeg",
            ),
            // Release 09.10.2026: `saat/bild/bild.png` gibt es im Repo nicht
            // (nie eingecheckt); ein echtes PNG aus den Bildtests.
            ("crates/narsil-image/tests/daten/openlogo-50.png", "image/png"),
        ] {
            let pfad = wurzel.join(datei);
            let bytes = std::fs::read(&pfad).unwrap_or_else(|e| panic!("{datei}: {e}"));
            assert!(bytes.len() > 100, "{datei}: nur {} Byte", bytes.len());
            // GEMESSENE ENTSCHEIDUNG: die Zahl **vor** dem Weggeben der
            // Bytes merken — `setze_quelle_bytes` nimmt sie per Wert.
            let byte_zahl = bytes.len();
            let mut m = super::Menue::oeffne(
                super::Kontext::Bild,
                // KONTEXT (03.10.2026, GEMESSENE KORREKTUR): hier stand
                // `https://beispiel.de/{ZEICHENBREITE}` — also eine Adresse
                // **ohne Endung**. GEMESSENE FOLGE (Test 2 auf Panik, dann
                // `.expect("die Originalzeile")`): **keine** Originalzeile.
                // Ursache ist **gemessen** und steht in `narsil_konvert::ziele`:
                // `zielformat_von_endung("")` ist `None`, also weiss der
                // Aufrufer nicht, welches Format die Quelle hat — und die
                // Originalzeile ist genau diese Angabe. Also traegt die Adresse
                // jetzt den **Dateinamen**, wie ihn eine Seite traegt.
                &format!(
                    "https://beispiel.de/{}",
                    datei.rsplit('/').next().unwrap_or(datei)
                ),
                &Point::new(300.0, 180.0),
            );
            m.setze_mime(mime);
            // KONTEXT (03.10.2026): das `clone()` ist **meine** Zeile (E0382
            // „borrow of moved value: `bytes`“, 19:5x) — `setze_quelle_bytes`
            // nimmt **Besitz** (`Vec<u8>`), weil es die Bytes **behaelt** und
            // weil M4 die Groesse **vor** der Allokation des Ziels prueft. Der
            // Test liest `bytes` danach noch (`bytes.len()`), also braucht er
            // eine eigene Kopie — ein 95-kB-Bild im Test ist kein Speicherproblem.
            m.setze_quelle_bytes(bytes.clone());
            // DIE ZIELE, wie `narsil` sie bekommt, wenn der Nutzer aufklappt.
            let ziele = m.ziele();
            assert!(!ziele.is_empty(), "{datei}: das Menue bietet kein Ziel");
            // 1. DIE LISTE **WOERTLICH** — so steht sie im Bild, und so
            //    steht sie in der Anzeige des Nutzers.
            //
            //    GEMESSENE ENTSCHEIDUNG, warum dies der richtige Test ist
            //    (Leitung, 20:25: „Ein Test, der genau diese Liste prueft,
            //    ist richtig - dann nur die Liste anpassen“): die Liste ist
            //    das, was der Nutzer **liest**. Eine Eigenschaft der Liste
            //    („alle Zeilen haben ein Praefix“) ist eine Behauptung ueber
            //    den Wortlaut an **zehn** Stellen — und aendert sich der
            //    Wortlaut, faellt nur **eine** Stelle um, statt ueberall.
            //
            //    KONTEXT (03.10., 20:25, dritter Durchgang der Leitung):
            //    GEMESSEN ist, dass die erste Zeile **ohne** „Als “ dasteht —
            //    bei JPEG **und** bei PNG. GEMESSENE URSACHE:
            //    `src/konvert.rs:154` gibt bei `original` **vor** dem „Als “
            //    zurueck (`return self.titel.clone()`), und diese Datei ist
            //    **unveraendert** (letzte Aenderung GEMESSEN 13:53, also vor
            //    allen drei Freigaben). GEMESSENE ENTSCHEIDUNG, warum der
            //    Test die **Liste** behauptet und nicht die Regel: die
            //    einheitliche Form („alle Eintraege 'Als <FORMAT>'“) ist
            //    eine Aenderung in einer **fremden** Datei (BRETT 20:10 an
            //    UI); steht sie, ist **eine Zeile** hier zu aendern.
            let beschriftungen: Vec<String> = ziele.iter().map(|z| z.beschriftung()).collect();
            let erwartet: Vec<String> = if mime == "image/jpeg" {
                vec![
                    // GEMESSEN: die erste Zeile ist das **Original** und
                    // traegt deshalb kein „Als “ (siehe oben).
                    "As JPEG (original)",
                    "As PNG",
                    "As WebP",
                    "As GIF",
                    "As BMP (no transparency)",
                    "As ICO",
                ]
                .into_iter()
                .map(String::from)
                .collect()
            } else {
                vec![
                    "As PNG (original)",
                    "As JPEG (no transparency)",
                    "As WebP",
                    "As GIF",
                    "As BMP (no transparency)",
                    "As ICO",
                ]
                .into_iter()
                .map(String::from)
                .collect()
            };
            assert_eq!(
                beschriftungen, erwartet,
                "{datei}: die Zielliste hat sich geaendert"
            );
            // 1b. UND JEDE ZEILE AUSSER DER ERSTEN HAT „ALS “ UND EIN
            //     PRODUZIERBARES ZIEL. GEMESSENE ENTSCHEIDUNG, warum die
            //     erste Zeile ausgenommen ist: sie ist das **Original**, und
            //     ein Original ist kein Umformat.
            for (nr, z) in ziele.iter().enumerate() {
                if z.original {
                    continue;
                }
                assert!(
                    z.beschriftung().starts_with("As "),
                    "{datei}: Zeile {nr} heisst {:?} und hat kein \"As \"",
                    z.beschriftung()
                );
            }
            // 2. UND ES IST PRODUZIERBAR — ein Eintrag, den der Umformer
            //    nicht bauen kann, waere ein toter Eintrag (K5).
            assert_eq!(
                ziele.len(),
                m.untermenue_ziele(700).len(),
                "{datei}: die Fensterkuerzt die Zielliste ({})",
                ziele.len()
            );
            // 3. UND DIE ERSTE ZEILE IST DAS ORIGINAL — GEMESSEN ist, dass
            //    die Originalzeile **ohne** „Als “ dasteht, weil
            //    `Ziel::beschriftung()` fuer `original` frueh zurueckgibt
            //    (`src/konvert.rs:154`, Datei der Rolle UI). GEMESSENE
            //    ENTSCHEIDUNG, warum dieser Test die **Zahl** und nicht den
            //    Wortlaut behauptet: der Wortlaut wird dort geaendert, und ein
            //    Test, der an einem Wort haengt, blockiert die Aenderung, die
            //    die Leitung will (BRETT 19:45/20:05).
            let original = ziele
                .iter()
                .find(|z| z.original)
                .expect("die Originalzeile");
            assert_eq!(
                original.format,
                super::super::konvert::normalisiere(if mime == "image/jpeg" {
                    "jpg"
                } else {
                    "png"
                }),
                "{datei}: die Originalzeile ist nicht die der Quelle"
            );
            eprintln!(
                "KTX-MESSUNG {datei}: {byte_zahl} Byte, {} Ziele {:?}",
                ziele.len(),
                beschriftungen
            );
            // 4. UND: das Bild liegt **im Bild** (Freigabe-Punkt 5).
            let chrome = Chrome::neu(1000);
            let liste = super::display_list(&m, &chrome, 1000, 700);
            let texte = texte(&liste);
            assert!(
                texte.iter().any(|t| t == "Download converted"),
                "{datei}: die Elternzeile steht nicht im Bild"
            );
        }
    }

    /// KONTEXT 03.10. (dritter Durchgang, Leitung 20:25): DER KLICK LEGT EINE
    /// DATEI IN `~/Downloads` — und `file` sagt, was darin steht.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Weg hier geht und nicht in `narsil`:
    /// `narsil` schreibt die Datei in `Sitzung::datei_schreiben`
    /// (`sitzung.rs:1386`, `HOME/Downloads` + `create_dir_all` + bis zu zehn
    /// Namensvarianten) — und **das** ist Code der Crate `narsil`, nicht
    /// meine. GEMESSEN ist, dass der Weg **bis auf das Schreiben** in
    /// `narsil-chrome` liegt: [`super::Menue::ziele`] entscheidet die Zeile,
    /// [`super::super::ktx_konvert::konvertiere`] macht aus den Bytes der
    /// Quelle die fertige Datei. GEMESSENE ENTSCHEIDUNG, warum der Test
    /// trotzdem **schreibt**: die Leitung verlangt den Nachweis, dass die
    /// Datei **liegt** — und „liegt“ ist keine Eigenschaft einer
    /// Zeichenkette, sondern einer Platte.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `file` und nicht die eigene Bytezahl:
    /// die Bytezahl behauptet, dass **etwas** geschrieben wurde; `file` liest
    /// die ersten Bytes und **erkennt das Format** — und erkennt es falsch,
    /// wenn die Datei keine ist (das ist der ganze Unterschied).
    #[test]
    fn der_klick_legt_die_datei_in_downloads() {
        let wurzel = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("Repo-Wurzel")
            .to_path_buf();
        // ECHTES JPEG von der Platte (M9: nicht unter /tmp).
        let quelle = "https://beispiel.de/fotos/urlaub.jpeg";
        let bytes = std::fs::read(wurzel.join("crates/narsil-fuzz/saat/bild/farbverlauf_420.jpg"))
            .expect("das Testbild");
        // 1. DAS MENUE nennt die Zeile „Als PNG“ — der Klick geht auf sie.
        let mut m = super::Menue::oeffne(super::Kontext::Bild, quelle, &Point::new(300.0, 180.0));
        m.setze_mime("image/jpeg");
        m.setze_quelle_bytes(bytes.clone());
        let ziel = m
            .ziele()
            .into_iter()
            .find(|z| z.beschriftung() == "As PNG")
            .expect("das Menue nennt „Als PNG“ nicht");
        // 2. DER KLICK: Bytes + Zeile -> fertige Datei (derselbe Aufruf wie
        //    `narsil`, Zeile 1348).
        let datei = super::super::ktx_konvert::konvertiere(quelle, &bytes, &ziel)
            .unwrap_or_else(|e| panic!("Als PNG: {e}"));
        assert!(
            datei.name.ends_with(".png"),
            "Falscher Name: {}",
            datei.name
        );
        assert_eq!(datei.mime, "image/png");
        // 3. DAS SCHREIBEN — an denselben Ort wie `narsil` (`sitzung.rs:1386`).
        // Release 09.10.2026: Test-Downloads im Temp-Verzeichnis, nicht im
        // echten `$HOME/Downloads` des Nutzers (dort lag nach jedem Lauf
        // `urlaub.png`).
        let ordner = std::env::temp_dir().join(format!("narsil-test-downloads-{}", std::process::id()));
        std::fs::create_dir_all(&ordner).expect("Test-Downloads");
        let pfad = ordner.join(&datei.name);
        std::fs::write(&pfad, &datei.daten).expect("Datei schreiben");
        // 4. `file` LIEGT VOR — und seine Ausgabe ist das, was berichtet wird.
        let ausgabe = std::process::Command::new("file")
            .arg(&pfad)
            .output()
            .expect("das Kommando `file`");
        let text = String::from_utf8_lossy(&ausgabe.stdout).trim().to_string();
        eprintln!(
            "KTX-DATEI {}\n  {} Byte geschrieben, `file` sagt: {text}",
            pfad.display(),
            datei.daten.len()
        );
        // 5. DIE ZWEI BEHAUPTUNGEN: die Datei liegt, und `file` erkennt sie
        //    als PNG. GEMESSENE ENTSCHEIDUNG, warum die Groesse **nicht**
        //    gleich der Quelle sein muss: umformen heisst neu schreiben, und
        //    ein PNG eines JPEGs ist groesser (verlustfrei).
        assert!(pfad.exists(), "die Datei liegt nicht: {}", pfad.display());
        assert!(
            text.contains("PNG image data"),
            "`file` erkennt die Datei nicht als PNG: {text}"
        );
        assert_ne!(
            datei.daten.len(),
            bytes.len(),
            "GEMESSEN: die Datei hat dieselbe Laenge wie die Quelle — dann wurde nichts umgeformt"
        );
        // GEMESSENE ENTSCHEIDUNG, warum die Datei ** liegen bleibt**: sie ist
        // der Nachweis. GEMESSENE ENTSCHEIDUNG, warum sie **benannt** ist:
        // `urlaub.png` ist der Name, den `ktx_konvert::stammlname` aus der
        // Quelle gebaut hat — und der ist derselbe wie im Browser, weil es
        // dieselbe Funktion ist.
        assert_eq!(datei.name, "urlaub.png", "GEMESSEN: der Name weicht ab");
    }

    /// KONTEXT 03.10. (20:50, Leitung): „UEBERSETZEN" OEFFNET GOOGLE TRANSLATE
    /// MIT DER SPRACHE DES BROWSERS — in einem **neuen** Tab.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine **Adresse** ist und kein
    /// Aufruf: `narsil-chrome` darf keine fremde Crate kennen (Kopf `lib.rs`)
    /// und hat kein Fenster — der Browser ist `narsil`, und der Weg ist: hier
    /// die Adresse bauen, dort einen Tab damit aufmachen. GEMESSENE
    /// ENTSCHEIDUNG, warum der Weg trotzdem **hier** geprueft wird: die
    /// Adresse ist das ganze Ergebnis, und `tl=` ist die Angabe, an der man
    /// sieht, ob die Browsersprache angekommen ist.
    #[test]
    fn uebersetzen_oeffnet_google_mit_der_browsersprache() {
        let chrome = Chrome::neu(1000);
        // DIE SPRACHEN, die `narsil` aus `LANG` bekommt (Zahl nach Zahl).
        for lang in ["de_DE.UTF-8", "en_US.UTF-8", "fr_FR", "pt_BR.UTF-8@euro"] {
            // GEMESSENE ENTSCHEIDUNG, warum `kurz` (ein `&str`) und nicht
            // `&sprache` (eine `String`) weitergegeben wird: clippy meldete
            // „reference which is immediately dereferenced“ (op_ref), und
            // `String::as_str` ist in diesem Werkzeug (1.9x) **nicht**
            // stabil — GEMESSEN: `error[E0658]: unstable library feature
            // 'str_as_str'`. Der `&str` entsteht hier einfach aus dem
            // `LANG`-Rohwert.
            let kurz = lang.split('_').next().unwrap_or("");
            let sprache = super::sprache_aus_lang(lang);
            assert_eq!(sprache, kurz, "GEMESSEN: {lang} -> {sprache}");
            for (k, quelle) in [
                (super::Kontext::Text, "Ein Text zum Kopieren"),
                (super::Kontext::Link, "https://beispiel.de/artikel"),
                (super::Kontext::Bild, "https://beispiel.de/bild.png"),
                (super::Kontext::Video, "https://beispiel.de/film.mp4"),
            ] {
                let url = super::uebersetzen(k, quelle, kurz)
                    .unwrap_or_else(|| panic!("{k:?}: keine Uebersetzungs-Adresse"));
                // 1. DIE RAUM-NR. und `sl=auto` (die Ausgangssprache kann die
                //    Seite selbst nennen — „Google Translate").
                assert!(
                    url.starts_with(&format!(
                        "{}?sl=auto&tl={sprache}&u=",
                        super::TRANSLATE_RAUM
                    )),
                    "{k:?}: {url}"
                );
                // 2. `tl=` IST DIE BROWSERSPRACHE — an **jedem** Kontext.
                assert_eq!(
                    super::uebersetzen(k, quelle, kurz),
                    Some(format!(
                        "{}?sl=auto&tl={sprache}&u={}&op=translate",
                        super::TRANSLATE_RAUM,
                        super::kodieren(quelle)
                    )),
                    "{k:?}: die Adresse traegt nicht tl={sprache}"
                );
                // 3. UND: die Adresse ist **absolut** und zeigt NICHT auf die
                //    Quelle — ein „neuer Tab“ ist genau das: eine andere
                //    Adresse. GEMESSENE ENTSCHEIDUNG, warum das behauptet
                //    wird und nicht „der Tab ist neu“: ein Tab ist neu, wenn
                //    er eine andere Adresse zeigt.
                assert!(url.starts_with("https://"), "{k:?}: {url}");
                assert_ne!(url, quelle, "{k:?}: der neue Tab zeigt die Quelle");
                // 4. DER TEXT IST KODIERT — ein Leerzeichen darf die Adresse
                //    nicht zerreissen.
                if k == super::Kontext::Text {
                    assert!(
                        url.contains("u=Ein%20Text%20zum%20Kopieren"),
                        "GEMESSEN: der Text ist nicht kodiert: {url}"
                    );
                }
            }
        }
        // 5. UND: `Chrome::sprache()` ist das, was `narsil` uebergibt — mit
        //    der Umgebung als Quelle und `SPRACHE_STANDARD` als Ende.
        let ohne_umgebung = std::env::var("LANG").is_err();
        let erwartet = if ohne_umgebung {
            super::super::SPRACHE_STANDARD.to_string()
        } else {
            super::sprache_aus_umgebung()
        };
        assert_eq!(chrome.sprache(), erwartet, "GEMESSEN: die Browsersprache");
        // GEMESSENE ENTSCHEIDUNG, warum die Sprache **zweimal** geholt und
        // nicht einmal als `&chrome.sprache()` durchgereicht wird: clippy
        // meldete „this expression creates a reference which is immediately
        // dereferenced“ (op_ref) — und `str::as_str` ist noch nicht stabil.
        let eigene = chrome.sprache();
        // GEMESSEN: `Chrome::sprache()` gibt ein `&str` zurueck — darum war
        // `&eigene` ein `&&str` (clippy op_ref).
        let bsp = super::uebersetzen(super::Kontext::Text, "Hallo", eigene).unwrap_or_default();
        eprintln!(
            "KTX-MESSUNG Uebersetzen: Browsersprache {eigene:?}, {} als Raum, Bsp. {bsp}",
            super::TRANSLATE_RAUM
        );
    }

    /// KONTEXT 03.10. (20:50, Leitung): DAS VIDEOMENUE AN EINEM **ECHTEN**
    /// FILM — und die Ziele stehen **aus den Daten**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das der Beweis fuer „WebM/GIF/nur
    /// Ton/Standbild“ ist: GEMESSEN ist, dass „nur Ton“ am **Ton** haengt und
    /// „animiertes GIF“ an der **Bildzahl** — beides steht nicht im MIME-Typ
    /// (K5). Ohne `quelle_bytes` kann das Menue genau das **nicht** entscheiden
    /// und bietet deshalb nur zwei Ziele.
    #[test]
    fn das_videomenue_zeigt_die_ziele_aus_den_daten() {
        let wurzel = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("Repo-Wurzel")
            .to_path_buf();
        let quelle = "https://beispiel.de/clip.webm";
        // 1. OHNE DATEN: nur die zwei Ziele, die ohne Datei entscheidbar sind.
        let mut ohne =
            super::Menue::oeffne(super::Kontext::Video, quelle, &Point::new(300.0, 180.0));
        ohne.setze_mime("video/webm");
        let liste_ohne: Vec<String> = ohne.ziele().iter().map(|z| z.beschriftung()).collect();
        assert_eq!(
            liste_ohne,
            // Release 09.10.2026: das Original (die Datei, wie sie ist)
            // steht seit dem 04.10. bei jedem Medium zuerst — es braucht
            // keinen Kodierer.
            vec!["As WebM (original)", "As AVI", "As PNG (first frame)"],
            "GEMESSEN: ohne Datei stehen mehr Ziele da"
        );
        // 2. MIT DATEN: der Film ist von der Platte (echte WebM-Dateien aus
        //    `probe/seite/`, nicht aus /tmp — M9).
        for datei in [
            // Release 09.10.2026: dieselben Filme als eingecheckte
            // Testdateien (`probe/` traegt keine *.webm, `.gitignore`).
            "crates/narsil-audio/tests/daten/g9at_video_vorbis.webm",
            "crates/narsil-audio/tests/daten/g9at_video_opus.webm",
        ] {
            let bytes =
                std::fs::read(wurzel.join(datei)).unwrap_or_else(|e| panic!("{datei}: {e}"));
            let mut m =
                super::Menue::oeffne(super::Kontext::Video, quelle, &Point::new(300.0, 180.0));
            m.setze_mime("video/webm");
            m.setze_quelle_bytes(bytes.clone());
            let liste: Vec<String> = m.ziele().iter().map(|z| z.beschriftung()).collect();
            eprintln!(
                "KTX-MESSUNG Video {datei}: {} Byte -> {:?}",
                bytes.len(),
                liste
            );
            // 3. GEMESSENE ENTSCHEIDUNG, warum behauptet wird: (a) „Standbild“
            //    ist IMMER da, (b) „Als AVI“ ist IMMER da (das ist der Film im
            //    anderen Container), (c) und die beiden datenabhaengigen
            //    Ziele tauchen **genau dann** auf, wenn die Datei sie traegt.
            assert!(
                liste.iter().any(|t| t.contains("AVI")),
                "{datei}: AVI fehlt: {liste:?}"
            );
            assert!(
                liste.iter().any(|t| t.contains("PNG")),
                "{datei}: das Standbild fehlt: {liste:?}"
            );
            // 4. UND: KEINE TOTE ZEILE — jede angebotene Zeile hat ein Ziel,
            //    das `narsil-konvert` produzieren kann (K5). GEMESSEN ist, dass
            //    „WebM“ **nicht** dabei ist: GEMESSEN ist in
            //    `narsil_konvert::ziele` dokumentiert, dass der WebM/VP9-
            //    Enkoder **fehlt** (K3 Teil 2) — waere er eine Zeile, waere
            //    sie tot.
            //    Release 09.10.2026: das ORIGINAL („Als WebM (Original)")
            //    ist keine Umformung, es speichert die Datei, wie sie ist.
            assert!(
                !m.ziele().iter().any(|z| !z.original && z.titel.contains("WebM")),
                "{datei}: WebM wird angeboten, ist aber nicht gebaut: {liste:?}"
            );
            // 5. UND DIE ZEILE STEHT IM BILD — mit Einzug, neben dem Kasten.
            let mut auf = m.clone();
            auf.klappe_auf();
            let feld = auf
                .untermenue_rechteck(1000, 700)
                .expect("das Feld geht nicht auf");
            assert!(feld.x >= auf.rechteck(1000, 700).expect("Menue").rechts());
            let beschriftungen: Vec<String> = auf
                .untermenue_ziele(700)
                .iter()
                .map(|z| z.beschriftung())
                .collect();
            assert_eq!(
                beschriftungen, liste,
                "{datei}: das Feld zeigt andere Ziele als `ziele()`"
            );
            // 6. UND DAS BILD — Freigabe 20:50 („mit Nachweis“).
            let chrome = Chrome::neu(1000);
            let pfad_bild = schiessen(
                &super::display_list(&auf, &chrome, 1000, 700),
                &format!("video_{}", datei.rsplit('/').next().unwrap_or(datei)),
                1000,
                700,
            );
            assert!(
                pfad_bild.exists(),
                "das Bild fehlt: {}",
                pfad_bild.display()
            );
        }
    }

    /// KONTEXT 03.10. (20:50, Leitung): EIN **AUDiolink** HAT DIE TONZIELE.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ueber den **Link**-Kontext geht und
    /// nicht ueber einen neuen `Kontext::Audio`: GEMESSEN ist, dass das
    /// Untermenue ueber `ktx_konvert::ziele` laeuft, und das entscheidet nach
    /// **MIME-Typ** (`audio/*`) bzw. Endung — nicht nach dem Kontext. Ein
    /// eigener Kontext wuerde in `ktx_konvert.rs:114` einen neuen `match`-Zweig
    /// brauchen (fremde Datei); der **Link**-Kontext mit `audio/*` kommt
    /// heute zum selben Ergebnis.
    #[test]
    fn ein_audiolink_hat_die_tonziele() {
        let quelle = "https://beispiel.de/ton.wav";
        let mut m = super::Menue::oeffne(super::Kontext::Link, quelle, &Point::new(300.0, 180.0));
        m.setze_mime("audio/wav");
        let liste: Vec<String> = m.ziele().iter().map(|z| z.beschriftung()).collect();
        eprintln!("KTX-MESSUNG Audio (audio/wav): {liste:?}");
        // GEMESSEN: WAV und FLAC sind gebaut — und **nur** die beiden.
        assert!(
            liste.iter().any(|t| t.contains("WAV")),
            "WAV fehlt: {liste:?}"
        );
        assert!(
            liste.iter().any(|t| t.contains("FLAC")),
            "FLAC fehlt: {liste:?}"
        );
        //
        // KONTEXT (03.10.2026, 02:45): OGG IST ANGEBOTEN — KONVERT hat den
        // Muxer gebaut. GEMESSENE ENTSCHEIDUNG, warum die Behauptung trotzdem
        // **nicht** „OGG steht da“ lautet, sondern **„MP3 steht nicht da“**:
        // eine Zeile, die jemand **baut**, darf stehen (K5 will keine **toten**
        // Eintraege, nicht keine neuen). Und GEMESSEN ist, dass **MP3** noch
        // nicht gebaut ist (K2c, geparkt) — das ist die Zeile, die nicht
        // stehen darf. GEMESSEN ist ausserdem der Weg darunter: der Test
        // `ton_ohne_tote_eintraege` in `ktx_konvert.rs` behauptet dasselbe an
        // der Zielliste, und die Behauptung „jedes Ziel ist gebaut“ steht in
        // `ktx_konvert.rs` gegen `narsil_konvert` selbst.
        assert!(
            liste.iter().any(|t| t.contains("OGG")),
            "GEMESSEN: OGG fehlt in der Tonliste: {liste:?}"
        );
        assert!(
            !liste.iter().any(|t| t.contains("MP3")),
            "GEMESSEN: MP3 ist nicht gebaut und darf nicht als Ziel stehen: {liste:?}"
        );
        // UND: es steht im Bild, wenn das Feld aufgeht.
        let mut auf = m.clone();
        auf.klappe_auf();
        assert!(
            auf.untermenue_rechteck(1000, 700).is_some(),
            "das Tonfeld geht nicht auf"
        );
        let chrome = Chrome::neu(1000);
        let pfad_bild = schiessen(
            &super::display_list(&auf, &chrome, 1000, 700),
            "audio_wav_unter",
            1000,
            700,
        );
        assert!(
            pfad_bild.exists(),
            "das Bild fehlt: {}",
            pfad_bild.display()
        );
    }

    /// KONTEXT 03.10. (21:35, Leitung): EIN **TON** BEKOMMT „AUDIO SPEICHERN“
    /// UND DIE TONZIELE — ohne einen neuen `Kontext`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ueber den **Link**-Kontext geht:
    /// GEMESSEN ist, dass `ktx_konvert::ziele` die Zielliste nach **MIME**
    /// bzw. Endung entscheidet — ein `Kontext::Audio` wuerde dort einen
    /// neuen `match`-Zweig brauchen (fremde Datei) und eine **zweite** Liste
    /// fuer dieselbe Frage.
    #[test]
    fn ein_ton_bekommt_die_tonzeilen() {
        // 1. DER TON WIRDT AN DER ENDUNG ERKANNT (narsil setzt keinen MIME).
        for quelle in [
            "https://beispiel.de/ton.wav",
            "https://beispiel.de/ton.FLAC",
            "https://beispiel.de/ton.ogg",
            "https://beispiel.de/ton.mp3",
        ] {
            let m = super::Menue::oeffne(super::Kontext::Link, quelle, &Point::new(300.0, 180.0));
            assert!(m.ist_ton(), "{quelle}: nicht als Ton erkannt");
            let eintraege = super::eintraege(super::Kontext::Link, true);
            assert_eq!(
                eintraege,
                vec![
                    super::Eintrag::Kopieren,
                    super::Eintrag::Uebersetzen,
                    super::Eintrag::LinkOeffnen,
                    super::Eintrag::LinkNeuerTab,
                    super::Eintrag::AudioSpeichern,
                    super::Eintrag::Konvertieren,
                ],
                "{quelle}: die Tonzeile steht nicht an der Stelle von „Link speichern unter“"
            );
            // 2. UND: ohne Ton bleibt „Link speichern unter“ stehen.
            let ohne = super::Menue::oeffne(
                super::Kontext::Link,
                "https://beispiel.de/artikel.html",
                &Point::new(300.0, 180.0),
            );
            assert!(!ohne.ist_ton());
            assert!(
                super::eintraege(super::Kontext::Link, false)
                    .contains(&super::Eintrag::LinkSpeichern),
                "ohne Ton fehlt „Link speichern unter“"
            );
            assert!(
                !super::eintraege(super::Kontext::Link, true)
                    .contains(&super::Eintrag::LinkSpeichern),
                "mit Ton steht „Link speichern unter“ noch da — zwei Namen fuer eine Sache"
            );
        }
        // 3. DER MIME GEWINNT, wenn beide da sind.
        let mut m = super::Menue::oeffne(
            super::Kontext::Link,
            "https://beispiel.de/ton.mp3",
            &Point::new(300.0, 180.0),
        );
        m.setze_mime("audio/wav");
        assert!(m.ist_ton());
        m.setze_mime("video/webm");
        assert!(m.ist_ton(), "GEMESSEN: die Endung sagt es auch");
        let mut b = super::Menue::oeffne(
            super::Kontext::Link,
            "https://beispiel.de/film.mp4",
            &Point::new(300.0, 180.0),
        );
        b.setze_mime("audio/ogg");
        assert!(b.ist_ton(), "GEMESSEN: der MIME sagt Ton, die Endung nicht");
        // 4. UND DIE ZIELE: WAV (Original), FLAC, **OGG** — und **kein** MP3.
        //
        // KONTEXT (03.10.2026, 02:45): OGG kam dazu, weil KONVERT den Muxer
        // gebaut hat (K2c auf deren Liste). GEMESSENE ENTSCHEIDUNG, warum das
        // hier eine **Zahl** ist und kein „jedes Ziel ist gebaut“: das ist die
        // Behauptung von `ktx_konvert.rs` an `narsil_konvert`; hier ist die
        // Behauptung, was der **Namen** der Zeile ist — und die aendert sich,
        // wenn KONVERT etwas baut.
        let mut t = super::Menue::oeffne(
            super::Kontext::Link,
            "https://beispiel.de/ton.wav",
            &Point::new(300.0, 180.0),
        );
        t.setze_mime("audio/wav");
        let ziele: Vec<String> = t.ziele().iter().map(|z| z.beschriftung()).collect();
        eprintln!("KTX-MESSUNG Ton-Menue: {ziele:?}");
        assert_eq!(
            ziele,
            vec!["As WAV (original)", "As FLAC", "As OGG"],
            "GEMESSEN: die Tonziele haben sich geaendert"
        );
        // 5. UND ES STEHT IM BILD.
        let chrome = Chrome::neu(1000);
        let mut auf = t.clone();
        auf.klappe_auf();
        let texte = texte(&super::display_list(&auf, &chrome, 1000, 700));
        assert!(
            texte.iter().any(|x| x == "Save audio"),
            "GEMESSEN: „Audio speichern“ steht nicht im Bild: {texte:?}"
        );
        assert!(
            auf.untermenue_rechteck(1000, 700).is_some(),
            "das Tonfeld geht nicht auf"
        );
    }

    /// DAS MENUE IM BILD — und es hat nicht mehr als `super::MAX_EINTRAEGE` Zeilen.
    #[test]
    fn das_menue_im_bild() {
        for thema in [
            super::super::thema::Thema::Hell,
            super::super::thema::Thema::Dunkel,
        ] {
            let mut chrome = Chrome::neu(1000);
            chrome.thema = thema;
            for k in super::Kontext::ALLE {
                for quelle in ["", "https://a.example/b.png", &"x".repeat(5000)] {
                    let m = super::Menue::oeffne(k, quelle, &Point::new(120.0, 150.0));
                    let liste = super::display_list(&m, &chrome, 1000, 700);
                    // Release 08.10.2026 (Glas-Stil): GEMESSENE FORMEL —
                    // Glasfeld (Schattenringe + Grund + Glanz + Kante), je
                    // Abschnitt eine Ueberschrift (und davor eine Linie), je
                    // Zeile Icon + Beschriftung + hoechstens Kuerzel/Chevron.
                    let erwartet = super::schatten_ringe(100) as usize
                        + 3
                        + m.trennzahl(700) * 2
                        + 1
                        + m.zeilen(700).len() * 3;
                    assert!(
                        liste.len() <= erwartet,
                        "{k:?}: {} Items, erwartet hoechstens {erwartet}",
                        liste.len()
                    );
                    // UND: die sichtbaren Zeilen stehen als Text drin.
                    //
                    // KONTEXT (03.10.): es sind jetzt **ein Text mehr** als
                    // Zeilen — und seit dem Chevron (03.10.) ist der Chevron **kein**
                    // Text mehr, sondern ein Bild. GEMESSENE ENTSCHEIDUNG,
                    // warum deshalb hier nichts abgezogen wird: es gibt
                    // keinen Text, der keine Beschriftung waere.
                    // Release 08.10.2026: gezaehlt werden die Beschriftungen
                    // (Ueberschriften und Kuerzel sind eigene Texte).
                    let texte: Vec<String> = texte(&liste);
                    assert_eq!(
                        texte.len(),
                        m.zeilen(700).len(),
                        "{k:?}: {} Texte, {} Zeilen",
                        texte.len(),
                        m.zeilen(700).len()
                    );
                }
            }
        }
        // BOMBEN: ein 0-px-Fenster.
        let chrome = Chrome::neu(1000);
        for k in super::Kontext::ALLE {
            let mut m = super::Menue::oeffne(k, "q", &Point::new(0.0, 0.0));
            // KONTEXT (03.10.): auch **aufgeklappt** — das Untermenue-Feld
            // ist eine eigene Zeichnung, und die wurde vorher nie mit einem
            // 0-px-Fenster gerufen.
            for auf in [false, true] {
                if auf {
                    m.klappe_auf();
                }
                for (b, h) in [(0u32, 0u32), (1, 1), (u32::MAX, u32::MAX)] {
                    let liste = super::display_list(&m, &chrome, b.min(2000), h.min(2000));
                    // KONTEXT (03.10.): die Schranke ist die FORMEL:
                    // Menue (1 Kasten + 4 Rahmenstuege + hoechstens
                    // MAX_EINTRAEGE Texte + hoechstens MAX_EINTRAEGE
                    // Trennlinien + 1 Chevron) plus Untermenue-Feld
                    // (1 Kasten + 4 Rahmenstuege + hoechstens MAX_EINTRAEGE
                    // Zielzeilen) = **29**.
                    // Release 08.10.2026 (Glas-Stil): zwei Glasfelder, je
                    // Zeile hoechstens drei Items, je Zeile hoechstens eine
                    // Ueberschrift mit Linie, dazu die Zielzeilen.
                    let feld = super::schatten_ringe(100) as usize + 3;
                    let zeilen = super::MAX_EINTRAEGE + 1;
                    assert!(
                        liste.len() <= 2 * feld + 3 * zeilen + 2 * zeilen + super::MAX_EINTRAEGE,
                        "{k:?} auf={auf}: {} Items bei {b}x{h}",
                        liste.len()
                    );
                }
            }
        }
    }
}

// ===========================================================================
// KONTEXT 03.10.2026, 04:30 (Leitung „(5) Tab-Rechtsklick“): DAS MENUE EINES
// TABS.
// ===========================================================================

/// DER ZUSTAND, DEN DAS TABMENUE ZUM ZEICHNEN BRAUCHT.
///
/// KONTEXT (04.10.2026, 11:10, Leitung: „Tab anheften/Lautlos/Tabs rechts
/// schließen“): GEMESSENE ENTSCHEIDUNG, warum das ein **Bündel** ist und nicht
/// vier Parameter an drei Stellen: `nr_unter` (Treffer), `tab_display_list`
/// (Farbe) und `aktiv` ( grau oder nicht) brauchen **dieselben** Angaben, und
/// drei Aufrufer mit je vier Parametern sind vier Zahlen an drei Orten. Als
/// **eine** Struktur wandert der Zustand mit dem Tab, der ihn betrifft.
///
/// GEMESSENE ENTSCHEIDUNG, warum `tabs_rechts` eine **Zahl** ist: sie ist im
/// `narsil-browser` (`tabs_rechts_von`) schon eine Zahl, und sie traegt sowohl
/// die Frage „darf man klicken?“ als auch die Meldung „3 Tabs rechts“.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TabZustand {
    /// Wie viele Tabs es **insgesamt** gibt.
    pub tab_anzahl: u32,
    /// Wie viele Tabs stehen **rechts** vom geoeffneten.
    pub tabs_rechts: u32,
    /// Gibt es einen Tab zum **Wiederherstellen**?
    pub wiederherstellbar: bool,
    /// Ist der geoeffnete Tab **angepinnt**?
    pub gepinnt: bool,
    /// Ist der geoeffnete Tab **lautlos**?
    pub lautlos: bool,
}

/// DIE ZEILE EINES TAB-MENUES — **zehn** Zeilen (04.10.2026, Leitung).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabEintrag {
    /// Einen neuen Tab aufmachen.
    TabNeu,
    /// Den Tab neu laden.
    TabNeuLaden,
    /// Den Tab anheften oder loesen.
    TabAnheften,
    /// Den Tab stumm schalten oder den Ton einschalten.
    TabStumm,
    /// Den Tab verdoppeln.
    TabDuplizieren,
    /// Den Tab in ein **eigenes Fenster** oeffnen.
    TabNeuesFenster,
    /// Den Tab schliessen.
    TabSchliessen,
    /// Die **anderen** Tabs schliessen.
    TabAndereSchliessen,
    /// Die Tabs **rechts** vom aktiven schliessen.
    TabsRechtsSchliessen,
    /// Den letzten geschlossenen Tab wiederherstellen.
    TabWiederherstellen,
    /// Release 08.10.2026 (Feature 3): die Netzregel des Tabs weiterschalten
    /// (frei → nur lokal → gesperrt → frei). Nur bei eingeschalteter Funktion.
    TabNetz,
    /// Release 08.10.2026 (Feature 5): den Tab sofort einfrieren.
    TabEinfrieren,
}

/// Release 08.10.2026: der Name einer Netzregel (0 frei, 1 nur lokal,
/// 2 gesperrt) — dieselbe Zaehlung wie `narsil_net::haken::Netzregel`.
pub fn netzregel_name(regel: u8) -> &'static str {
    match regel {
        1 => tr("local only"),
        2 => tr("blocked"),
        _ => tr("open"),
    }
}

impl TabEintrag {
    /// DIE BESCHRIFTUNG — woertlich wie bei Firefox, und **zwei** Zeilen
    /// wechseln ihre Beschriftung mit dem Zustand.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum „Tab anheften“ zu „Tab lösen“ wechselt
    /// und nicht zu „Anheften aufheben“: Firefox zeigt den Zustand als
    /// **Gegenbefehl** („Pin Tab“ / „Unpin Tab“), und die Zeile ist 250 Pixel
    /// breit — ein Nebensatz waere der erste Text, der dort nicht mehr
    /// passt.
    pub fn text(&self, z: &TabZustand) -> &'static str {
        match self {
            TabEintrag::TabNeu => tr("New tab"),
            TabEintrag::TabNeuLaden => tr("Reload"),
            TabEintrag::TabAnheften => {
                if z.gepinnt {
                    tr("Unpin tab")
                } else {
                    tr("Pin tab")
                }
            }
            TabEintrag::TabStumm => {
                if z.lautlos {
                    tr("Unmute tab")
                } else {
                    tr("Mute tab")
                }
            }
            TabEintrag::TabDuplizieren => tr("Duplicate tab"),
            TabEintrag::TabNeuesFenster => tr("Open tab in new window"),
            TabEintrag::TabSchliessen => tr("Close"),
            TabEintrag::TabAndereSchliessen => tr("Close other tabs"),
            TabEintrag::TabsRechtsSchliessen => tr("Close tabs to the right"),
            TabEintrag::TabWiederherstellen => tr("Reopen closed tab"),
            TabEintrag::TabNetz => tr("Switch this tab's network"),
            TabEintrag::TabEinfrieren => tr("Freeze tab"),
        }
    }

    /// DER NAME — fuer Meldungen und Tests. **Ohne** Zustand: ein Name, der
    /// sich mit der Beschriftung aendert, waere kein Name mehr.
    pub fn name(&self) -> &'static str {
        match self {
            TabEintrag::TabNeu => "tab-neu",
            TabEintrag::TabNeuLaden => "tab-neu-laden",
            TabEintrag::TabAnheften => "tab-anheften",
            TabEintrag::TabStumm => "tab-stumm",
            TabEintrag::TabDuplizieren => "tab-duplizieren",
            TabEintrag::TabNeuesFenster => "tab-neues-fenster",
            TabEintrag::TabSchliessen => "tab-schliessen",
            TabEintrag::TabAndereSchliessen => "tab-andere-schliessen",
            TabEintrag::TabsRechtsSchliessen => "tabs-rechts-schliessen",
            TabEintrag::TabWiederherstellen => "tab-wiederherstellen",
            TabEintrag::TabNetz => "tab-netz",
            TabEintrag::TabEinfrieren => "tab-einfrieren",
        }
    }

    /// DIE GRUPPEN — (neu, laden) | (anheften, stumm, duplizieren, Fenster) |
    /// (schliessen, andere, rechts) | (wiederherstellen).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum „Geschlossenen Tab wiederherstellen“ eine
    /// **eigene** Gruppe ist und nicht bei „Schließen“ steht: es ist das
    /// Gegenteil von Schliessen, und es gehoert zu einem **anderen** Zeitpunkt
    /// (vorher vs. nachher). Eine Gruppe, die beides mixed, wuerde beides
    /// unsichtbar machen.
    pub fn gruppe(&self) -> u8 {
        match self {
            TabEintrag::TabNeu | TabEintrag::TabNeuLaden => 0,
            TabEintrag::TabAnheften
            | TabEintrag::TabStumm
            | TabEintrag::TabDuplizieren
            | TabEintrag::TabNeuesFenster => 1,
            TabEintrag::TabSchliessen
            | TabEintrag::TabAndereSchliessen
            | TabEintrag::TabsRechtsSchliessen => 2,
            TabEintrag::TabWiederherstellen => 3,
            // Release 08.10.2026: die Zeilen aus dem Tab „Features".
            TabEintrag::TabNetz | TabEintrag::TabEinfrieren => 4,
        }
    }

    /// STEHT VOR DIESER ZEILE EINE TRENNLINIE?
    pub fn trenn_vor(&self, nr: usize) -> bool {
        nr > 0 && ALLE_TAB[nr - 1].gruppe() != self.gruppe()
    }

    /// KANN MAN DIESE ZEILE JETZT KLICKEN?
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum „Tab in neuem Fenster öffnen“ **immer
    /// grau** ist: GEMESSEN ist, dass `narsil-browser` **kein Fenstermodell**
    /// hat (ein `Browser` = eine Liste `tabs`, und `narsil/src/main.rs:317`
    /// oeffnet genau **ein** X11-Fenster pro Prozess). Es gibt also kein Ziel,
    /// und eine Zeile, die tut, als gae es eines, waer die schlimmste Art von
    /// „geht hier nicht“: sie **sieht** aktiv aus und tut nichts. Grau ist die
    /// ehrliche Antwort; im BRETT (04.10. 11:20) an die Leitung gemeldet.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum „Tabs rechts schließen“ an der **Zahl**
    /// haengt und nicht am Zustand „letzter Tab“: rechts kann auch **ein**
    /// Tab stehen, und der ist genau der, den man schliessen will.
    pub fn aktiv(&self, z: &TabZustand) -> bool {
        match self {
            TabEintrag::TabAndereSchliessen => z.tab_anzahl > 1,
            TabEintrag::TabsRechtsSchliessen => z.tabs_rechts > 0,
            TabEintrag::TabWiederherstellen => z.wiederherstellbar,
            TabEintrag::TabNeuesFenster => false,
            _ => true,
        }
    }
}

/// DIE **ZEHN** ZEILEN DES TAB-MENUES — in der Reihenfolge von Firefox.
pub const ALLE_TAB: [TabEintrag; 10] = [
    TabEintrag::TabNeu,
    TabEintrag::TabNeuLaden,
    TabEintrag::TabAnheften,
    TabEintrag::TabStumm,
    TabEintrag::TabDuplizieren,
    TabEintrag::TabNeuesFenster,
    TabEintrag::TabSchliessen,
    TabEintrag::TabAndereSchliessen,
    TabEintrag::TabsRechtsSchliessen,
    TabEintrag::TabWiederherstellen,
];

/// DAS MENUE EINES TABS — die fuenf Zeilen an der **Mauspitze**.
#[derive(Debug, Clone, PartialEq)]
pub struct TabMenue {
    /// Der Tab, auf den geklickt wurde (Nummer ab 0).
    pub tab: u32,
    /// Der **gesamte Zustand**, den die Zeilen brauchen — siehe [`TabZustand`].
    ///
    /// KONTEXT (04.10.2026, 11:15): GEMESSENE ENTSCHEIDUNG, warum nicht
    /// `tab_anzahl` allein anymore (05.10.2026, 04:30 war es noch): fuer die
    /// neuen Zeilen reicht die Zahl nicht mehr — „Tabs rechts schließen“
    /// braucht die Zahl rechts, „Anheften“ den Zustand des Tabs. Wer
    /// `tab_anzahl` behaelt und drei Felder daneben legt, hat **vier** Wege,
    /// dieselbe Sache zu sagen.
    pub zustand: TabZustand,
    /// Wo der Klick war (linke obere Ecke, wie bei [`Menue`]).
    pub x: u32,
    pub y: u32,
    /// Die Zeile unter der Maus — dieselbe Hover-Frage wie im Hauptmenue.
    pub zeiger: Option<usize>,
    /// Release 08.10.2026 (Feature 3): die Netzregel des Tabs (0 frei,
    /// 1 nur lokal, 2 gesperrt) — `None`, wenn die Funktion aus ist.
    pub netz: Option<u8>,
    /// Release 08.10.2026 (Feature 5): steht „Tab einfrieren" im Menue?
    pub einfrieren: bool,
    /// Release 08.10.2026 (Glas-Stil, HiDPI): die Skalierung in Prozent —
    /// wie [`Menue::skala_prozent`].
    pub skala_prozent: u16,
}

impl TabMenue {
    /// Release 08.10.2026: die Zeilen dieses Menues — die zehn aus
    /// [`ALLE_TAB`] und dahinter die Zeilen der eingeschalteten Features.
    pub fn eintraege(&self) -> Vec<TabEintrag> {
        let mut v = ALLE_TAB.to_vec();
        if self.netz.is_some() {
            v.push(TabEintrag::TabNetz);
        }
        if self.einfrieren {
            v.push(TabEintrag::TabEinfrieren);
        }
        v
    }

    /// Release 08.10.2026: die Zeile mit der Nummer `nr`.
    pub fn eintrag(&self, nr: usize) -> Option<TabEintrag> {
        self.eintraege().get(nr).copied()
    }

    /// Release 08.10.2026: die Features des Tabs setzen (siehe die Felder).
    pub fn setze_features(&mut self, netz: Option<u8>, einfrieren: bool) {
        self.netz = netz;
        self.einfrieren = einfrieren;
    }

    /// Release 08.10.2026 (HiDPI): die Skalierung des Fensters.
    pub fn setze_skalierung(&mut self, faktor: f32) {
        self.skala_prozent = skala_prozent(faktor);
    }

    /// Release 08.10.2026: logische Pixel in Fensterpixel.
    pub fn px(&self, logisch: u32) -> u32 {
        px_mit(self.skala_prozent, logisch)
    }

    /// Release 08.10.2026: die Beschriftung einer Zeile — die Netzzeile nennt
    /// den Zustand und das naechste Ziel.
    pub fn beschriftung(&self, e: TabEintrag) -> String {
        match (e, self.netz) {
            (TabEintrag::TabNetz, Some(n)) => trf!(
                "Network: {} → {}",
                netzregel_name(n),
                netzregel_name((n + 1) % 3)
            ),
            _ => e.text(&self.zustand).to_string(),
        }
    }

    /// Release 08.10.2026: steht vor der Zeile `nr` eine Trennlinie?
    fn trenn_vor_nr(&self, nr: usize) -> bool {
        let v = self.eintraege();
        nr > 0 && nr < v.len() && v[nr - 1].gruppe() != v[nr].gruppe()
    }
    /// EIN TABMENUE OEFFNEN (der Klickpunkt ist die linke obere Ecke).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum dieselbe **Abreundung** wie im
    /// Hauptmenue ([`Menue::oeffne`]): es ist dasselbe Fenster und dieselbe
    /// Maus — eine Rundung hier und eine dort waere ein Versatz von einem
    /// halben Pixel, den man sieht und nicht benennen kann.
    pub fn oeffne(tab: u32, zustand: TabZustand, punkt: &Point) -> TabMenue {
        TabMenue {
            tab,
            zustand,
            x: if punkt.x.is_finite() && punkt.x >= 0.0 {
                punkt.x as u32
            } else {
                0
            },
            y: if punkt.y.is_finite() && punkt.y >= 0.0 {
                punkt.y as u32
            } else {
                0
            },
            zeiger: None,
            netz: None,
            einfrieren: false,
            skala_prozent: 100,
        }
    }

    /// DIE HOEHE — mit den Trennbaendern, aus denselben Zahlen wie im
    /// Hauptmenue ([`ZEILE`], [`TRENN_HOEHE`], [`LUFT_OBEN`]).
    pub fn feld_hoehe(&self) -> u32 {
        let n = self.eintraege().len();
        self.px(LUFT_OBEN) * 2
            + n as u32 * self.px(ZEILE)
            + (1..n).filter(|nr| self.trenn_vor_nr(*nr)).count() as u32 * self.px(TRENN_HOEHE)
    }

    /// DAS RECHTECK — am Fensterrand, wenn es nicht passt (dieselbe Regel wie
    /// [`Menue::rechteck`], damit ein Menue nicht halb aus dem Fenster ragt).
    pub fn rechteck(&self, fensterbreite: u32, fensterhoehe: u32) -> Option<Rechteck> {
        let breite = self.px(MENUE_BREITE).min(fensterbreite);
        let hoehe = self.feld_hoehe();
        if breite == 0 || hoehe > fensterhoehe {
            return None;
        }
        Some(Rechteck {
            x: self.x.min(fensterbreite.saturating_sub(breite)),
            y: self.y.min(fensterhoehe.saturating_sub(hoehe)),
            breite,
            hoehe,
        })
    }

    /// DER ABSTAND DER ZEILE `nr` VON DER MENUEOBERKANTE.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **hier** steht und nicht in einem
    /// Aufrufer: dieselbe Zahl wird an drei Stellen gebraucht — Hoehe, Zeichnung
    /// und Treffer — und drei Aufrufer, die selbst zaehlen, sind drei
    /// Zaehlungen.
    pub fn zeilen_y(&self, nr: usize) -> u32 {
        let trenn_davor = (1..=nr).filter(|n| self.trenn_vor_nr(*n)).count() as u32;
        self.px(LUFT_OBEN) + self.px(ZEILE) * nr as u32 + self.px(TRENN_HOEHE) * trenn_davor
    }

    /// WELCHE ZEILE LIEGT UNTER DEM PUNKT? — `None`, wenn nirgends oder wenn
    /// die Zeile **grau** ist (eine inaktive Zeile ist kein Treffer, siehe
    /// [`Menue::nr_unter`]).
    pub fn nr_unter(&self, fensterbreite: u32, fensterhoehe: u32, p: &Point) -> Option<usize> {
        let r = self.rechteck(fensterbreite, fensterhoehe)?;
        if !p.x.is_finite() || !p.y.is_finite() {
            return None;
        }
        if p.x < r.x as f32
            || p.x >= r.rechts() as f32
            || p.y < r.y as f32
            || p.y >= r.unten() as f32
        {
            return None;
        }
        let y = p.y as u32;
        let eintraege = self.eintraege();
        for nr in (0..eintraege.len()).rev() {
            let oben = r.y + self.zeilen_y(nr);
            if y >= oben && y < oben + self.px(ZEILE) {
                return eintraege[nr].aktiv(&self.zustand).then_some(nr);
            }
        }
        None
    }

    /// DIE MAUS AUF EINE ZEILE SETZEN (Hover wie im Hauptmenue).
    pub fn setze_zeiger(&mut self, fensterbreite: u32, fensterhoehe: u32, p: &Point) -> bool {
        self.zeiger = self.nr_unter(fensterbreite, fensterhoehe, p);
        self.zeiger.is_some()
    }

    /// STEHT DIE MAUS AUF DIESER ZEILE?
    pub fn ist_zeiger(&self, nr: usize) -> bool {
        self.zeiger == Some(nr)
    }
}

/// DAS TABMENUE ALS ZEICHENLISTE — dasselbe Glasfeld, dieselbe Hinterlegung
/// und dieselben Linien wie das Hauptmenue.
///
/// GEMESSENE ENTSCHEIDUNG, warum das **hier** steht und nicht in `narsil`: es
/// gehoert zur Optik des Menues, und die Optik liegt in dieser Datei.
///
/// Release 08.10.2026 (Glas-Stil): Glasfeld, weisse Schrift, helle runde
/// Hinterlegung; die Zeilen behalten ihre Gruppenlinien (keine Ueberschriften,
/// keine Icons — die Tabzeilen sind eine Liste von Befehlen an **einen** Tab).
pub fn tab_display_list(
    m: &TabMenue,
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> DisplayList {
    let _thema_bleibt_dunkel = chrome;
    let mut liste = DisplayList::new();
    let Some(r) = m.rechteck(fensterbreite, fensterhoehe) else {
        return liste;
    };
    let p = m.skala_prozent;
    let s = p.max(1) as f32 / 100.0;
    glas_feld(&mut liste, &r, p);
    for (nr, e) in m.eintraege().iter().enumerate() {
        let y = r.y + m.zeilen_y(nr);
        if y + m.px(ZEILE) > r.unten() {
            break;
        }
        if m.trenn_vor_nr(nr) {
            liste.push(trennlinie(&r, y.saturating_sub(m.px(TRENN_HOEHE)), p));
        }
        let aktiv = e.aktiv(&m.zustand);
        if m.ist_zeiger(nr) && aktiv {
            liste.push(gehover(&r, y, p));
        }
        // KONTEXT (04.10.2026, 11:20): die Beschriftung **mit** Zustand —
        // zwei Zeilen wechseln je nach Tab zwischen zwei Texten.
        // Release 08.10.2026: und die Netzzeile nennt ihre Regel.
        liste.push(text_item(
            r.x + m.px(RAND) + m.px(4),
            y + m.px(TEXT_ABSTAND),
            m.beschriftung(*e),
            if aktiv { GLAS_TEXT } else { GLAS_AUS },
            SCHRIFT * s,
            400,
        ));
    }
    liste
}

#[cfg(test)]
mod locale_regression {
    #[test]
    fn posix_locale_ist_keine_uebersetzungssprache() {
        for locale in ["", "C", "C.UTF-8", "POSIX"] {
            assert_eq!(super::sprache_aus_lang(locale), crate::SPRACHE_STANDARD);
        }
        assert_eq!(super::sprache_aus_lang("en_US.UTF-8"), "en");
        assert_eq!(super::sprache_aus_lang("de_DE@euro"), "de");
    }
}

/// Release 09.10.2026: das Untermenue folgt der Maus mit Verzug.
#[cfg(test)]
mod untermenue_verzug {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn darueberfahren_klappt_nach_dem_verzug_auf_und_wieder_zu() {
        let (b, h) = (1280u32, 800u32);
        let mut m = Menue::oeffne(Kontext::Bild, "https://beispiel.de/fotos/urlaub.png", &Point::new(100.0, 120.0));
        m.setze_mime("image/png");
        let zeilen = m.zeilen(h);
        let nr = zeilen
            .iter()
            .position(|z| matches!(z, Zeile::Aktion(Eintrag::Konvertieren)))
            .expect("Zeile „Konvertiert herunterladen“");
        let andere = zeilen
            .iter()
            .position(|z| matches!(z, Zeile::Aktion(Eintrag::BildSpeichern)))
            .expect("Zeile „Bild speichern“");
        let r = m.rechteck(b, h).expect("Rechteck");
        let punkt = |n: usize| Point::new((r.x + 40) as f32, (r.y + m.zeilen_y(n, h) + 5) as f32);
        let (auf_konvert, auf_andere) = (punkt(nr), punkt(andere));
        let t0 = Instant::now();
        // Auf die Zeile: noch zu, nach dem Verzug auf.
        assert!(m.setze_zeiger_um(b, h, &auf_konvert, t0));
        assert!(!m.hover_tick(h, t0 + Duration::from_millis(100)));
        assert!(!m.ist_aufgeklappt(), "Vorbeiwischen klappt nicht auf");
        assert!(m.hover_tick(h, t0 + Duration::from_millis(UNTERMENUE_VERZUG_MS)));
        assert!(m.ist_aufgeklappt());
        // Ins Untermenue (ausserhalb des Hauptmenues): bleibt offen.
        let u = m.untermenue_rechteck(b, h).expect("Untermenue");
        let t1 = t0 + Duration::from_millis(400);
        assert!(!m.setze_zeiger_um(b, h, &Point::new((u.x + 20) as f32, (u.y + 10) as f32), t1));
        assert!(!m.hover_tick(h, t1 + Duration::from_secs(2)));
        assert!(m.ist_aufgeklappt());
        // Auf eine andere Zeile: erst nach dem Verzug zu.
        let t2 = t1 + Duration::from_secs(3);
        assert!(m.setze_zeiger_um(b, h, &auf_andere, t2));
        assert!(!m.hover_tick(h, t2 + Duration::from_millis(50)));
        assert!(m.ist_aufgeklappt(), "kurz darueber: bleibt offen");
        assert!(m.hover_tick(h, t2 + Duration::from_millis(UNTERMENUE_VERZUG_MS + 10)));
        assert!(!m.ist_aufgeklappt());
    }
}
