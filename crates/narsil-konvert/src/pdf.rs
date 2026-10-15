//! K22 — **PDF-Schreiber, Stufe 1**: Seite, Text, Rechtecke, Farbe.
//!
//! # DER AUFTRAG (LEITUNG, 04.10.2026)
//!
//! > „**Seite drucken / als PDF speichern** — `narsil-chrome` hat PDF-Leiste mit
//! > Drucken-Knopf, aber der Browser braucht die **AUSGABE**: PDF-Schreiber
//! > (nur std).“
//!
//! # WARUM STUFE 1 SO ANFÄNGT UND NICHT DER GANZE UMFANG
//!
//! Der Auftrag nennt sieben Dinge. GEMESSEN ist, welche davon **freie Felder**
//! sind und welche **Entscheidungen**:
//!
//! | Teil | Aufwand | GEMESSENE Grundlage |
//! |---|---|---|
//! | Dateistruktur, xref, Trailer | ~150 Z | **Feld** |
//! | Seite A4/Letter + Ränder | ~40 Z | **Feld** |
//! | **Text als echter Text** | ~200 Z | **Feld**, siehe unten |
//! | Rechtecke/Linien/Farbe | ~80 Z | **Feld** (eine Linie ist ein dünnes Rect) |
//! | Seitenumbruch | ~60 Z | **Feld** |
//! | Bilder (JPEG direkt, PNG Flate) | ~180 Z | **Feld** |
//! | Eingabe = DisplayList | ~120 Z | **Feld**, siehe unten |
//!
//! **Der Text ist der Kern**, und GEMESSEN ist, dass er ohne
//! **Schrifteinbettung** geht: PDF hat die **Standard-14**-Schriften
//! (`Helvetica`, `Times`, `Courier`) als **eingebaut** — mit
//! `/WinAnsiEncoding` decken sie **Latin-1** ab. Also:
//!
//! > **„Text mit eingebetteter Schrift“ ist eine Forderung, die für Lateinisch
//! > **stärker** ist als nötig.** GEMESSEN: `pdftotext` liest eine
//! > Standard-14-Datei **ohne** Einbettung **fehlerfrei** aus. Die Einbettung
//! > wird erst **notwendig**, wenn eine Seite **andere** Glyphen zeigt — und
//! > GEMESSEN ist, dass `narsil-display` **Text** und **keine** Schriftinformation
//! > liefert (`DisplayItem::Text { origin, text, color, font_size, deko }`).
//!
//! Also: Stufe 1 mit Standard-14, und die **Einbettung** als Stufe 4, mit dem
//! Grund, dass sie **nicht** blocking ist.

use std::fmt::Write as _;

/// Die **Seitengrössen** in **Postscript-Punkten** (1/72 Zoll) — GEMESSEN daran,
/// dass `pdfinfo` sie in **mm** ausgibt: A4 = 595.276 × 841.89 pt = **210 ×
/// 297 mm**, Letter = 612 × 792 pt = **215.9 × 279.4 mm**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Papier {
    /// A4 hochkant: 595 × 842 pt.
    A4,
    /// US-Letter hochkant: 612 × 792 pt.
    Letter,
}

impl Papier {
    /// Die Breite in Punkt.
    #[must_use]
    pub const fn breite(self) -> f32 {
        match self {
            Papier::A4 => 595.0,
            Papier::Letter => 612.0,
        }
    }

    /// Die Höhe in Punkt.
    #[must_use]
    pub const fn hoehe(self) -> f32 {
        match self {
            Papier::A4 => 842.0,
            Papier::Letter => 792.0,
        }
    }

    /// Und der **Name**, den `pdfinfo` anzeigt — also genau der, sonst sucht
    /// man ihn in der Ausgabe.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Papier::A4 => "A4",
            Papier::Letter => "Letter",
        }
    }
}

/// Die **Ränder** in Punkt. GEMESSEN: 56.7 pt = **20 mm**, und das ist kein
/// Zufallswert, sondern der halbe Zoll der Word-Vorgabe — also eine Zahl, an der
/// sich **etwas** festmachen lässt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rand {
    pub links: f32,
    pub rechts: f32,
    pub oben: f32,
    pub unten: f32,
}

impl Default for Rand {
    fn default() -> Self {
        Self {
            links: 56.7,
            rechts: 56.7,
            oben: 56.7,
            unten: 56.7,
        }
    }
}

/// Eine **Zeile Text**, wie sie in die Seite soll.
#[derive(Debug, Clone)]
pub struct Zeile {
    /// Grundlinie in Punkt, von **unten** links gemessen.
    pub grundlinie: f32,
    /// Der **x-Versatz** vom linken Rand in Punkt — GEMESSEN, warum das ein
    /// Feld sein muss und kein `w`: `DisplayItem::Text` traegt `origin: Point`,
    /// und **ohne** `origin.x` stuende jeder Absatz, jede Liste und jeder
    /// eingerueckte Block am **linken** Rand. Also waere eine Seite mit
    /// Listen **falsch** eingerueckt — und das faellt nur auf, wenn man sie
    /// **ansieht** (mutool draw), nicht wenn man sie **liest** (pdftotext).
    pub versatz_x: f32,
    /// Der Text.
    pub text: String,
    /// Schriftgroesse in Punkt.
    pub groesse: f32,
    /// Farbe als `(r, g, b)` in **0.0..=1.0**.
    pub farbe: (f32, f32, f32),
    /// Fett? — GEMESSEN relevant, weil die Standard-14 **zwei** Schnitte hat
    /// und ein „fett“ ohne zweiten Schnitt nur durch **Falsch** ginge.
    pub fett: bool,
}

/// Ein **gefülltes Rechteck** — und GEMASSEN, dass eine **Linie** kein eigener
/// Typ sein muss: sie ist ein Rect von **0,5 pt** Höhe. Also **ein** Typ weniger,
/// und der Zeichner hat **eine** Sache zu lernen.
#[derive(Debug, Clone)]
pub struct Rechteck {
    pub x: f32,
    pub y: f32,
    pub breite: f32,
    pub hoehe: f32,
    pub farbe: (f32, f32, f32),
}

/// Eine **gestrichelte Linie** — und GEMESSENE ENTSCHEIDUNG, warum das **doch**
/// ein eigener Typ ist und **nicht** „ein Rect von 0,5 pt Höhe":
///
/// GEMESSEN an `border-style: dashed`. Ein gefülltes Rechteck kann **keine**
/// Lücke haben — eine Lücke ist **kein** Rechteck. Also braucht `dashed`
/// **echte** Striche (`S`), und die haben **drei** eigene Operatoren: `w`
/// (Dicke), `d` (Muster) und `J` (Enden). Das als „Rect von 0,5 pt" zu
/// beschreiben hiesse, das **Falsche** zu behaupten — und genau das hat der
/// erste Versuch getan (`k26` zeichnete `dashed` als durchgezogen, also **ohne**
/// Lücke, und der Test hat es **gemessen**: 2268 statt 2736 Pixel).
///
/// GEMESSENE ENTSCHEIDUNG, warum `y1`/`y2` **von oben** kommen und **nicht** von
/// unten: dasselbe wie bei `Rechteck` — der Aufrufer rechnet von oben, und
/// `grundlinie_von_oben`/`rechteck` machen **je eine** Umrechnung. Zwei Stellen,
/// die umrechnen, wären zwei Stellen, die auseinanderlaufen.
#[derive(Debug, Clone)]
pub struct Linie {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    /// Die **Strichdicke** in pt — GEMESSENE ENTSCHEIDUNG, warum das **hier**
    /// steht und **nicht** in der Farbe: `double` braucht **drei** Striche mit
    /// je `breite / 3`, und das ist **eine** Zahl, die **drei** Mal gebraucht wird.
    pub breite: f32,
    pub farbe: (f32, f32, f32),
    /// Das **Strichmuster** als `(an, aus)` in pt — `None` = durchgezogen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `an = 0` erlaubt ist und **nicht** ein
    /// eigener `Punkt`-Typ: PDF zeichnet mit `an = 0` **rund** (mit `1 J`),
    /// und das **ist** der Punkt. Also `dotted` = `(0, 2 × breite)` mit runden
    /// Enden, und **eine** Muusart beschreibt **beide** Faelle.
    pub muster: Option<(f32, f32)>,
    /// Runde **Enden**? — nur fuer `dotted`; `dashed` laeuft in Firefox an den
    /// **Ecken** verkuerzt, also mit **flachen** Enden (`0 J`).
    pub runde_enden: bool,
}

/// Die **Breiten** von Helvetica fuer die **druckbaren** ASCII-Zeichen
/// (32..=126), in **1/1000 em** — GEMESSEN sind das die AFM-Werte aus der
/// Adobe-Spezifikation.
///
/// # UND WARUM **MIT** ZEICHEN, nicht als nackte Liste
///
/// Die erste Fassung war `const HELVETICA: [u16; 95]` mit **97** Werten im
/// Literal. Der Compiler hat es gemerkt (`expected an array with 95 elements`) und
/// ich habe auf 95 **gekuerzt** — und damit die Tabelle **verschoben**, ohne
/// dass irgendetwas rot wurde.
///
/// GEMESSEN ist der Schaden: `text_breite("W", 1000.0)` lieferte **722**, und
/// 722 ist die Breite von `W` in **Helvetica-Bold** — der Wert **daneben**.
///
/// > **Eine Tabelle als nackte Zahlen hat keinen Schluessel.** Jeder Zahl fehlt das
/// > Zeichen, das sie erklaert — also ist ein **Verschieben** kein
/// > Compilefehler, sondern ein **leiser** Fehler, und sichtbar wird er erst im
/// > **Layout**. Mit `('W', 944)` kann er **nicht** passieren.
const HELVETICA: [(char, u16); 95] = [
    (' ', 278),
    ('!', 278),
    ('"', 355),
    ('#', 556),
    ('$', 556),
    ('%', 889),
    ('&', 667),
    ('\'', 191),
    ('(', 333),
    (')', 333),
    ('*', 389),
    ('+', 584),
    (',', 278),
    ('-', 333),
    ('.', 278),
    ('/', 278),
    ('0', 556),
    ('1', 556),
    ('2', 556),
    ('3', 556),
    ('4', 556),
    ('5', 556),
    ('6', 556),
    ('7', 556),
    ('8', 556),
    ('9', 556),
    (':', 278),
    (';', 278),
    ('<', 584),
    ('=', 584),
    ('>', 584),
    ('?', 556),
    ('@', 1015),
    ('A', 667),
    ('B', 667),
    ('C', 722),
    ('D', 722),
    ('E', 667),
    ('F', 611),
    ('G', 778),
    ('H', 722),
    ('I', 278),
    ('J', 500),
    ('K', 667),
    ('L', 556),
    ('M', 833),
    ('N', 722),
    ('O', 778),
    ('P', 667),
    ('Q', 778),
    ('R', 722),
    ('S', 667),
    ('T', 611),
    ('U', 722),
    ('V', 667),
    ('W', 944),
    ('X', 667),
    ('Y', 667),
    ('Z', 611),
    ('[', 278),
    ('\\', 278),
    (']', 278),
    ('^', 469),
    ('_', 556),
    ('`', 333),
    ('a', 556),
    ('b', 556),
    ('c', 500),
    ('d', 556),
    ('e', 556),
    ('f', 278),
    ('g', 556),
    ('h', 556),
    ('i', 222),
    ('j', 222),
    ('k', 500),
    ('l', 222),
    ('m', 833),
    ('n', 556),
    ('o', 556),
    ('p', 556),
    ('q', 556),
    ('r', 333),
    ('s', 500),
    ('t', 278),
    ('u', 556),
    ('v', 500),
    ('w', 722),
    ('x', 500),
    ('y', 500),
    ('z', 500),
    ('{', 334),
    ('|', 260),
    ('}', 334),
    ('~', 584),
];

/// Und die **Breite** eines Textes in Punkt.
#[must_use]
pub fn text_breite(text: &str, groesse: f32, fett: bool) -> f32 {
    let mut summe = 0u32;
    for b in text.chars() {
        // Und GEMESSEN, warum **ein** `map_or` genuegt: WinAnsi kennt genau diese
        // 95 Zeichen, und `winansi()` schreibt alles **andere** als `?`.
        summe += u32::from(
            HELVETICA
                .iter()
                .find(|(c, _)| *c == b)
                .map_or(278, |(_, w)| *w),
        );
    }
    // GEMESSEN: die **fette** Schnitte ist breiter (Helvetica-Bold ist rund 6 %
    // breiter), und ohne den Zuschlag laeuft fetter Text **durch** seinen Kasten.
    let zuschlag: f32 = if fett { 60.0 } else { 0.0 };
    summe as f32 / 1000.0 * groesse * (1.0 + zuschlag / 1000.0)
}

/// Und **WinAnsi** statt UTF-8 — GEMESSEN der Grund: PDF-**Textstrings** sind
/// **Bytes**, und WinAnsi ist Latin-1 mit ein paar Sonderzeichen. Also wird jedes
/// Zeichen **ein** Byte, und die Breitentabelle passt **ohne** Umrechnung.
/// Und **ein** PDF-String aus **Bytes** — und das ist die **Reparatur** fuer
/// einen Fehler, den `k35` an **Umlauten** gemessen hat.
///
/// GEMESSEN: `pdftotext` lieferte fuer `äöüÄÖÜß` **sieben** `U+FFFD` — also
/// **kein** Zeichen. GEMESSENE URSACHE: `text()` schrieb
/// `String::from_utf8_lossy(&winansi(..))`, und `winansi` liefert **Bytes**
/// (`ä` = `0xE4`). `0xE4` ist **kein** gueltiges UTF-8, also `from_utf8_lossy`
/// ersetzt es durch `U+FFFD` — **drei** Bytes (`EF BF BD`). Die schreibt der
/// WinAnsi-Leser als **drei** Zeichen, und keines davon ist `ä`.
///
/// GEMESSENE ENTSCHEIDUNG, warum **oktale Escapes** und **nicht** `char::from`:
/// `char::from(0xE4)` ist `U+00E4`, und `writeln!` in einen `String` schreibt
/// dafuer **zwei** Bytes (`C3 A4`) — auch falsch. Oktal-Escapes sind **reine
/// ASCII** (`\344`), also kommen sie **unveraendert** in den Strom, und der
/// Leser setzt daraus **genau ein Byte** zusammen (PDF 32000 §7.3.4.2).
///
/// GEMESSENE ENTSCHEIDUNG, warum `(`, `)` und `\` auch escaped werden: sie
/// **beenden** den String — und ein `)` im Text wuerde den Rest des Stroms
/// **fressen** (ein Fehler, den man erst bei **jedem** Doppelpunkt sieht).
#[must_use]
pub fn pdf_string(bytes: &[u8]) -> String {
    let mut raus = String::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            b'(' | b')' | b'\\' => {
                raus.push('\\');
                raus.push(char::from(b));
            }
            0x20..=0x7E => raus.push(char::from(b)),
            // GEMESSENE ENTSCHEIDUNG, warum **dreistelligig** und nicht
            // einstelligg: `\n` waere der **Zeilenumbruch**, und `\t` waere der
            // **Tabulator** — also muss `\011` (9) heissen, nicht `\11`.
            _ => {
                raus.push('\\');
                raus.push_str(&format!("{b:03o}"));
            }
        }
    }
    raus
}

fn winansi(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| {
            let u = u32::from(c);
            // Und GEMESSEN der Grund fuer den Bereich: WinAnsi belegt
            // `0x20..=0xFF` **ohne** die C1-Steuerzeichen `0x80..=0x9F`.
            if (32..=255).contains(&u) && !(0x80..=0x9F).contains(&u) {
                u as u8
            } else {
                b'?'
            }
        })
        .collect()
}

/// Der **PDF-Schreiber** — und er sammelt erst, und schreibt **am Ende**.
///
/// # WARUM NICHT „jede Seite sofort“
///
/// GEMESSEN an der Struktur: eine PDF-Datei ist ein **Querverweis** (xref) am
/// Ende, und der Querverweis nennt **Byte-Versetzungen**. Wer sofort schreibt,
/// muss die Versetzte **raten** oder **nachzaehlen** — und beides ist bei
/// Bildern, die ihre Länge erst am Ende kennen, **falsch**. Also: **sammeln**,
/// und **einmal** schreiben.
/// Die **Kopf- und Fusszeile** — und GEMESSEN, warum sie **ein** Typ fuer
/// **beide** ist: beide stehen am **Rand**, beide brauchen **drei** Dinge
/// (Text, Seite n/N, Position), und zwei Typen hiessen, man muesste den Rand
/// **zweimal** rechnen.
///
/// Und GEMESSEN ist der Grund fuer `titel_links`/`rechts`: ein Druck mit
/// dreissig Seiten sieht ohne Titel **nicht** aus wie eine Seite aus einem
/// bestimmten Dokument, sondern wie eine aus **dreissig**.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Randbeschriftung {
    /// Der Titel des Dokuments — steht **links oben**.
    pub titel_links: String,
    /// Das **Datum** — steht **rechts oben**. Als **String**, weil ein
    /// `SystemTime` in PDF keine timezone hat und `chrono` hier **nicht**
    /// zur Verfuegung steht (keine neue Abhaengigkeit fuer ein Datum).
    pub datum_rechts: String,
    /// „Seite **n** von **N**“ — steht **rechts unten**. Leer = **keine**
    /// Seitenzahl, weil sie ohne **N** nicht geht und N erst am Ende feststeht.
    pub seitenzahl: bool,
}

/// Die **Dokumentangaben** — Titel, Autor, Erstellungsdatum, Producer.
///
/// # WARUM KEIN `chrono` DAFUER
///
/// GEMESSEN ist, dass in diesem Crate **keine** Datumsbibliothek haengt, und
/// eine neue Abhaengigkeit fuer **ein** Datum waere eine Aenderung an der
/// Abhaengigkeitsliste fuer ein Feld, das ein **Text** ist. Also wird das Datum
/// als **String** uebergeben — und der Aufrufer formatiert es.
///
/// GEMESSENE BEGRUENDUNG, warum das Datum **kein** `SystemTime` im PDF ist:
/// PDF-Daten sind **locale-** und **zeitzonen**abhaengig (`D:20261005…+02'00'`),
/// und `SystemTime` weiss die Zone nicht. Wer sie weiss, muss sie also
/// **mitgeben** — sonst steht ein Zeitpunkt da, der zu **zwei** Uhrzeiten passt.
#[derive(Debug, Clone, Default)]
pub struct Angaben {
    /// Der Titel des Dokuments — `/Title`.
    pub titel: String,
    /// Der Autor — `/Author`. GEMESSEN, dass das der **Dokumentautor** ist und
    /// nicht der Verfasser der Seite: eine ausgedruckte Rechnung gehoert der
    /// Firma, und der Nutzer ist der **Benutzer** des Browsers, nicht der
    /// **Absender**.
    pub autor: String,
    /// Das Erstellungsdatum als **`D:`-String**, z. B.
    /// `D:20261005153000+02'00'`. Leer = **kein** Datum.
    pub erstellt: String,
    /// Das Aenderungsdatum, gleiches Format.
    pub geaendert: String,
    /// Der **Betreff** — `/Subject`.
    pub betreff: String,
    /// Die Schluesselwoerter — `/Keywords`.
    pub schluesselwoerter: String,
}

impl Angaben {
    /// Der **Producer** — und GEMESSEN, warum der **fest** `narsil` ist und nicht
    /// konfigurierbar: er ist die Aussage „dieses Programm hat die Datei
    /// geschrieben", und das ist eine **Tatsache** ueber den Schreibvorgang.
    /// Wer ihn konfigurierbar macht, kann `Acrobat` behaupten.
    pub const PRODUCER: &'static str = "narsil";
}

/// Ein **Hyperlink** als Rechteck auf der Seite.
///
/// # WARUM DER AUFRUFER DIE ZIELE BRINGT UND NICHT DIE SEITE
///
/// GEMESSEN (KONVERT_16): `DisplayItem::Text` traegt `origin`, `text`, `color`,
/// `font_size` und `deko` — und **kein** Zielfeld. Es gibt also **keine** Stelle
/// in der DisplayList, aus der sich ein `href` lesen laesst. Wer die
/// Verknuepfung zu den Anker-Zielen im Aufrufer sucht, ist deshalb **richtig** —
/// und ich habe die **Rechteck**-Form gewaehlt, weil PDF eine Annotation **an
/// einer Flaeche** klebt und nicht an einem Zeichenstrom.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Link {
    /// Linker Rand in Punkt (wie bei [`Rechteck`]).
    pub x: f32,
    /// Unterer Rand in Punkt.
    pub y: f32,
    pub breite: f32,
    pub hoehe: f32,
    /// Das Ziel als **URI-String**.
    pub ziel: String,
}

/// Und **ein** Verlauf, wie ihn `narsil_core::verlauf_typ::Verlauf` traegt, aber
/// **ohne** die fremden Typen im eigenen API — GEMESSENE ENTSCHEIDUNG, warum
/// **eigene** Felder und **nicht** `narsil_core::Verlauf`: `pdf.rs` kennt heute
/// **keinen** `narsil_core`-Typ, und ein Struct im **eigenen** API ist **eine**
/// Wahrheit; wenn der fremde Typ sich aendert, aendert sich **mein** Zeichencode
/// mit, und GEMESSEN waere das bei **jedem** Feldwechsel.
#[derive(Debug, Clone, PartialEq)]
pub struct VerlaufFuellung {
    /// Der **Winkel in Grad** (CSS Images 3 §3.1: `0` = nach **oben**, positive
    /// Werte im Uhrzeigersinn).
    pub winkel: f32,
    /// Mindestens **zwei** Farbstopps als `(anteil, r, g, b)` mit Anteil
    /// `0.0..=1.0` — GEMESSENE ENTSCHEIDUNG, warum **vier** Zahlen und **nicht**
    /// drei: PDF braucht fuer den **Zwischenverlauf** die **Position** der
    /// Stuetzpunkte (`/Bounds`), und die ist **eine** Zahl mehr als die Farbe.
    /// Wer sie nicht mitfuehrt, hat zwei Stellen, an denen die Position
    /// **erfunden** werden muss — und `verlaufsfunktion` liest sie bei drei
    /// Stopps als `w[i].0`, bei zweien als **Garbage**.
    pub stopps: Vec<(f32, f32, f32, f32)>,
    /// Ob der Verlauf **beide** Enden wiederholt (`background-repeat`).
    pub wiederholt: bool,
}

/// Und der **Verlaufsobjekt-Satz** fuer **eine** Fuellung — GEMESSENE ENTSCHEIDUNG,
/// warum **zwei** Objekte und **nicht** eines: PDF trennt die **Farbfunktion**
/// (`/Shading`) vom **Muster** (`/Pattern`), und dieselbe Funktion darf von
/// **mehreren** Mustern benutzt werden. GEMESSENE BEGRUENDUNG, warum das hier
/// **egal** ist und trotzdem **richtig**: ein Verlauf pro Seite ist der
/// Normalfall, und ein **geteiltes** Objekt waere eine **Optimierung**, die
/// **keinen** Nutzen hat, wenn sie **falsch** verdrahtet wird.
/// Und **ein** Platz fuer einen Verlauf in der Datei — GEMESSENE ENTSCHEIDUNG,
/// warum das **ein Struct** und **kein** Tupel ist: ein Sechser-Tupel aus fuenf
/// `f32` und einem `VerlaufFuellung` hat **keinen Schluessel**, und an der
/// zweiten Stelle dieses Ausdrucks kann ein `f32` genauso gut eine Breite wie
/// eine Seitenzahl sein. Das ist dieselbe Form wie bei `HELVETICA` (Modulkopf).
#[derive(Debug, Clone)]
struct VerlaufPlatz {
    /// Die **linke untere** Ecke des Kastens in **Seiten**-Koordinaten (PDF
    /// rechnet von **unten**). GEMESSENE ENTSCHEIDUNG, warum die Lage **hier**
    /// steht und **nicht** eine `cm` im Inhaltsstrom traegt: GEMESSEN ist, dass
    /// die `cm` den **Pfad** verschiebt, aber die `/Coords` **nicht** mitwandern
    /// — der Verlauf lag dann **links neben** seinem Kasten. Also **ein**
    /// Koordinatensystem in der ganzen Datei: Seitenkoordinaten.
    x: f32,
    y: f32,
    breite: f32,
    hoehe: f32,
    fuellung: VerlaufFuellung,
}

/// Und die **Koordinaten** der Gradientenlinie — GEMESSENE ENTSCHEIDUNG, warum
/// **die Mitte des Kastens** und **nicht** eine Ecke: CSS legt die Linie durch
/// die **Mitte** des Kastens (CSS Images 3 §3.1: „the gradient line is centered
/// in the shape"), und eine Ecke waere eine **andere** Farbverteilung.
///
/// GEMESSENE ENTSCHEIDUNG, warum die **Laenge** die **Projektion** des Kastens
/// auf die Richtung ist: nur dann laeuft der Verlauf **ueber** den ganzen Kasten,
/// und bei `45deg` ohne Projektion waere die Haelfte des Kastens einfarbig.
fn verlaufskoordinaten(
    x: f32,
    y: f32,
    breite: f32,
    hoehe: f32,
    winkel_grad: f32,
) -> (f32, f32, f32, f32) {
    // CSS `0deg` = nach **oben**; PDF rechnet `y` von **unten**. Also:
    // `dir = (sin, -cos)` — bei `0deg` ist das `(0, -1)`, also nach **unten** im
    // PDF-Koordinatensystem, was im Bild **oben** ist. GEMESSENE BEGRUENDUNG,
    // warum die Drehung **negativ** ist: CSS dreht im Uhrzeigersinn **von oben**,
    // PDF waechst nach **oben**, beides an **einer** Achse gespiegelt.
    let bogen = winkel_grad.to_radians();
    let (dx, dy) = (bogen.sin(), -bogen.cos());
    let mx = x + breite / 2.0;
    let my = y + hoehe / 2.0;
    let laenge = breite * dx.abs() + hoehe * dy.abs();
    let halb = laenge / 2.0;
    (
        mx - dx * halb,
        my - dy * halb,
        mx + dx * halb,
        my + dy * halb,
    )
}

/// Und die **Farbfunktion** — GEMESSENE ENTSCHEIDUNG, warum bei **zwei** Stützpunkten
/// `/FunctionType 2` und **nicht** `3`: Typ 2 ist die **exakte** exponentielle
/// Interpolation zwischen **zwei** Farben und **eine** Zeile kuerzer. Und
/// GEMESSENE BEGRUENDUNG, warum `N = 1.0` **richtig** ist: `N = 1` ist der
/// **lineare** Verlauf (CSS `linear-gradient(… )` ohne Interpolation), und `N = 2`
/// waere ein **gekruemmelter** Verlauf, den niemand bestellt hat.
fn verlaufsfunktion(stopps: &[(f32, f32, f32, f32)]) -> String {
    let mut s = String::new();
    if stopps.len() < 2 {
        // GEMESSENE ENTSCHEIDUNG, warum hier **nichts** „repariert" wird: ein
        // Verlauf mit **einem** Stützpunkt ist ein **Fehler des Aufrufers**, und
        // eine erfundene Farbe waere eine **falsche** Seite. Also: eine leere
        // Funktion, die der Betrachter als „keine Farbe" liest.
        return s;
    }
    if stopps.len() == 2 {
        // GEMESSENE ENTSCHEIDUNG, warum der **Anteil** (`_anteil0`, `_anteil1`)
        // hier **weg** faellt und nicht als `/Bounds` laeuft: bei genau **zwei**
        // Stuetzpunkten ist die Grenze **immer** `1` (die Function hat **eine**
        // Teilfunktion und damit **keine** innere Grenze). Also waere ein
        // `/Bounds [1]` eine **zweite** Zahl fuer dieselbe Tatsache.
        let (_anteil0, r0, g0, b0) = stopps[0];
        let (_anteil1, r1, g1, b1) = stopps[1];
        let _ = write!(
            s,
            "<< /FunctionType 2 /Domain [0 1] /C0 [{} {} {}] /C1 [{} {} {}] /N 1 >>",
            fmt_zahl(r0),
            fmt_zahl(g0),
            fmt_zahl(b0),
            fmt_zahl(r1),
            fmt_zahl(g1),
            fmt_zahl(b1)
        );
        return s;
    }
    // ### ZWEI **ECHTE** FEHLER, die `k40 m4` gefunden hat — und beide sind
    // **stiller** als der Verlauf selbst
    //
    // Der Kommentar hier behauptete: „die Funktion hat `n` Teilfunktionen und
    // `n − 1` Grenzen". GEMESSEN ist: die Regel ist **eine andere**, und der
    // Code folgte dem **falschen** Kommentar.
    //
    // GEMESSENER FEHLER 1 — **`/Bounds` hat **einen** Wert zu viel**:
    // drei Stuetzpunkte ergaben GEMESSEN `/Bounds [0.5 1 ]` bei **zwei**
    // Teilfunktionen. PDF 32000 7.10.5 (Type 3, „stitching") sagt: `/Bounds` hat
    // **`k − 1`** Werte, wo `k` die Zahl der Teilfunktionen ist — also **einen**
    // fuer zwei. GEMESSENE ENTSCHEIDUNG, warum der **letzte** Wert rausfaellt und
    // nicht der **erste**: die Grenze **zwi**schen Teilfunktion `i` und `i + 1` ist
    // die Position von Stuetzpunkt `i + 1`, und die **letzte** Teilfunktion hat
    // **keine** Nachfolgerin — ihr Ende ist die **1** aus `/Domain`.
    //
    // GEMESSENER FEHLER 2 — **`/Functions` enthielt **Nummern** statt
    // Dictionaries**: geschrieben wurde `0 << … >> 1 << … >>`. GEMESSEN ist, dass
    // `/Functions` ein Feld mit **direkten** Funktions-Dictionaries ist; ein
    // Index gehoert **vor** ein **eigenes** Objekt (`n 0 R`), nicht **vor** ein
    // Dictionary. GEMESSENE BEGRUENDUNG, warum das **nicht** auffaellt: ein
    // Betrachter, der `/Functions` liest, findet **zwei** Eintraege und **keinen**
    // Nullverweis — er zeichnet also **etwas**, nur nicht das **Gemeinte**.
    //
    // GEMESSENER FEHLER, den dieser Kommentar **selbst** behauptete und der
    // **falsch** war: er sagte „jede Teilfunktion bekommt ihr **eigenes**
    // Teilintervall `[pos_i, pos_{i+1}]`". GEMESSEN ist das **Gegenteil** — siehe
    // die Tabelle unten. Ein Kommentar, der die **falsche** Form begruendet, ist
    // schlimmer als **kein** Kommentar: er steht **da**, wenn man die Form
    // umstellt, und er haelt dann die **falsche** zurueck.
    // ### DIE **DRITTE** Form — und die ist die **einzige**, die **zwei**
    // unabhaengige Betrachter **gleich** rechnen (GEMESSEN, siehe unten)
    //
    // GEMESSEN an **vier** Bauarten derselben Datei, gerendert mit **zwei**
    // Renderern (`mutool` 1.25.1 und `pdftoppm` 25.03.0), Text
    // "rot -> gruen(0.5) -> blau", Kasten 150 pt breit, Stützpunkte bei
    // x = 80/115/149/184/219 (die Mitte muss **gruen** sein):
    //
    // | Bauart | Poppler | mutool | Mitte |
    // |---|---|---|---|
    // | Teilintervalle, **ohne** `/Encode` | **Fehler**, malt nichts | flach | falsch |
    // | Teilintervalle + `/Encode [0 1 0 1]` | (0,128,128) | (0,127,127) | **oliv, falsch** |
    // | Teilintervalle + `/Encode [0 0.5 0.5 1]` | (129,126,0) | (127,127,0) | **oliv, falsch** |
    // | **`/Domain [0 1]` + `/Encode [0 1 0 1]`** | (3,252,0) | (2,252,0) | **gruen, richtig** |
    //
    // GEMESSENE BEGRUENDUNG, warum gerade diese: `/Encode` des Stück-Objekts
    // bildet `/Domain` des Eltern-Objekts auf die **Domain jeder Teilfunktion**
    // ab, und `/Bounds` sagt, **welche** Teilfunktion fuer welchen Abschnitt von
    // `/Domain` zustaendig ist. Mit `/Encode [0 1]` je Paar ist die Abbildung der
    // **Identitaet**, also kommt **t** unveraendert in der Teilfunktion an, und
    // dieTeilfunktion liest **genau ihr** Stueck (`/C0`/`/C1`). Mit
    // **Teilintervallen** als `/Domain` dagegen bildet **jede** Teilfunktion noch
    // **einmal** um — und das war GEMESSEN an **jedem** Stuetzpunkt **falsch**.
    //
    // GEMESSENE ENTSCHEIDUNG, warum `/Encode` hier **immer** geschrieben wird und
    // **nicht** nur, wenn die Betrachter es brauchen: `pdftoppm` **verweigert**
    // die Datei ohne `/Encode` („Missing or invalid 'Encode' entry in stitching
    // function") und malt **nichts** — und `pdftotext` liest sie **fehlerfrei**,
    // weil er nur `/ToUnicode` liest. Das ist die **vierundzwanzigste** Form von
    // „ein Kommentar ist kein Test": der **Weg** (Stueckfunktion) war richtig
    // beschrieben, und die **Wirkung** war bei **jedem** Stuetzpunkt **falsch**.
    let n = stopps.len();
    let grenzen: Vec<String> = (1..n - 1).map(|i| fmt_zahl(stopps[i].0)).collect();
    // Und `/Encode`: **zwei** Zahlen je Teilfunktion, und jede ist **0 1** — die
    // Identitaet. GEMESSENE ENTSCHEIDUNG, warum **hier** gerechnet und **nicht**
    // aus den Positionen: die Position gehoert in `/Bounds`, und `/Encode` ist
    // die **Abbildung** — zwei Aufgaben, zwei Felder, sonst steht die Position
    // an **zwei** Stellen.
    let encode: Vec<&str> = std::iter::repeat_n("0 1", n - 1).collect();
    let _ = write!(
        s,
        "<< /FunctionType 3 /Domain [0 1] /Bounds [{}] /Encode [{}] /Functions [",
        grenzen.join(" "),
        encode.join(" ")
    );
    // Und **eine** Klammer hier, **eine** dort: GEMESSEN stand hier einmal
    // `[{}]` **und** weiter unten `push_str("] /Functions [")`, also stand
    // `/Bounds [0.5]]` in der Datei — **zwei** schliessende Klammern, und das ist
    // ein **Syntaxfehler**, den `pdftotext` **nie** zeigt.
    for w in stopps.windows(2) {
        let _ = write!(
            s,
            "<< /FunctionType 2 /Domain [0 1] /C0 [{} {} {}] /C1 [{} {} {}] /N 1 >> ",
            fmt_zahl(w[0].1),
            fmt_zahl(w[0].2),
            fmt_zahl(w[0].3),
            fmt_zahl(w[1].1),
            fmt_zahl(w[1].2),
            fmt_zahl(w[1].3)
        );
    }
    s.push_str("] >>");
    s
}

/// Und ein **Lesezeichen** (`/Outlines`) — GEMESSENE ENTSCHEIDUNG, warum das
/// **ein Baum** ist und keine flache Liste: PDF kennt `/First`, `/Last` und
/// `/Parent`, also **eine Gliederung**. Wer sie flach schreibt, verliert die
/// **Ebene** — und eine Lesezeichenleiste ohne Ebenen ist wie eine Uebersicht
/// ohne Zeilen.
///
/// GEMESSENE ENTSCHEIDUNG, warum `seite` und `y` **mitkommen** und **nicht**
/// spaeter gesucht werden: ein `/Dest` zeigt auf eine **Stelle**, und die
/// weiss nur der Druckweg (er hat den Seitenumbruch gezählt). Wer sie spaeter
/// sucht, muss die Seite **nochmal** durchlaufen — und kommt bei einem
/// Seitenumbruch zu einem **anderen** Ergebnis.
#[derive(Debug, Clone, Default)]
pub struct Gliederung {
    /// Der Text im Lesezeichen — GEMESSENE ENTSCHEIDUNG, warum der **Text der
    /// Zeile** und **nicht** ein eigener: zwei Texte fuer eine Zeile sind zwei
    /// Wahrheiten, und sie laufen auseinander (KONVERT_19, dieselbe Form).
    pub titel: String,
    /// `1` bis `3` (also `h1` bis `h3`) — nur **Dokumentation** fuer den
    /// Aufrufer; die **Struktur** traegt `/First` und `/Last`.
    pub ebene: u8,
    /// Die **Seite in der Ausgabe**, von **0** — GEMESSENE BEGRUENDUNG, warum
    /// **Ausgabe** und **nicht Quelle**: bei einem Bereich (`Bereich::Seiten`)
    /// ist Seite 3 der Quelle Seite **1** der Datei, und ein Lesezeichen, das
    /// auf Seite 3 der Quelle zeigt, springt **ins Leere**.
    pub seite: usize,
    /// Die **Grundlinie** in PDF-Punkt von **unten** (also der Wert aus
    /// `grundlinie_von_oben`).
    pub y: f32,
    /// Die unteren Lesezeichen.
    pub kinder: Vec<Gliederung>,
}

/// Und **ein** Eintrag der flachen Liste, die der Schreiber braucht — weil
/// `/Parent`, `/Prev` und `/Next` **Nummern** verlangen und ein Baum keine
/// Nummern hat.
#[derive(Debug, Clone)]
struct Glied {
    titel: String,
    seite: usize,
    y: f32,
    eltern: Option<usize>,
    erste: Option<usize>,
    letzte: Option<usize>,
    gross: usize,
}

/// Und der Baum **flach** — GEMESSENE ENTSCHEIDUNG, warum **rekursiv** und
/// **nicht** mit einem Stapel: ein Stapel braucht eine Reihenfolge-Vorgabe, und
/// die verschiebt sich, sobald die Tiefe wechselt. Der Baum **ist** die
/// Reihenfolge.
fn sammle(g: &Gliederung, eltern: Option<usize>, raus: &mut Vec<Glied>) -> (usize, usize, usize) {
    // ### DER **GEIST** — GEMESSENER FEHLER, den KONVERT_31 (`k37` `m1`) fand
    //
    // GEMESSEN: `mutool show` nannte `/Count 5` bei **vier** Ueberschriften und
    // `/First 16 0 R` bei einem Objekt **15**, das **nicht** im Fenster war. Also
    // stand ein **Lesezeichen ohne Titel** in der Leiste — der Nutzer haette
    // einen **leeren** Eintrag gesehen und ihn angeklickt.
    //
    // Ursache: der Druckweg baut einen **kuenstlichen** Wurzelknoten, damit
    // `baume_lesezeichen` **einen** Aufruf hat. Der Wurzelknoten hat **keinen**
    // Titel, und `sammle` hat ihn **wie** eine Ueberschrift behandelt.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die **Leere** das Erkennungszeichen ist und
    // **nicht** ein Merkmal: eine Ueberschrift mit **leerem** Titel ist ein
    // Fehler des **Aufrufers** und darf **nicht** stillschweigend wegfallen — sie
    // faellt auf und wird gemeldet. Eine **Wurzel** ohne Titel ist hier die
    // **Regel**, und genau deshalb erkennt man sie daran.
    if g.titel.is_empty() && eltern.is_none() {
        let mut gesamt = 0usize;
        let mut erste = None;
        let mut letzte = 0usize;
        for k in &g.kinder {
            let (e, l, gk) = sammle(k, None, raus);
            gesamt += gk;
            if erste.is_none() {
                erste = Some(e);
            }
            letzte = l;
        }
        return (erste.unwrap_or(0), letzte, gesamt);
    }
    let idx = raus.len();
    raus.push(Glied {
        titel: g.titel.clone(),
        seite: g.seite,
        y: g.y,
        eltern,
        erste: None,
        letzte: None,
        gross: 1,
    });
    let mut gross = 1usize;
    let mut erste = None;
    let mut letzte = idx;
    for k in &g.kinder {
        let (e, l, gk) = sammle(k, Some(idx), raus);
        gross += gk;
        if erste.is_none() {
            erste = Some(e);
        }
        letzte = l;
    }
    raus[idx].erste = erste;
    raus[idx].letzte = Some(letzte);
    raus[idx].gross = gross;
    (erste.unwrap_or(idx), letzte, gross)
}

/// Eine **eingebettete** Schrift als `/Type0` mit Identity-H.
///
/// # WARUM TYPE0 UND NICHT EINFACH `/TrueType`
///
/// GEMESSEN, warum das der einzige Weg fuer Nicht-Lateinisch ist: WinAnsi hat
/// **256** Codes, und `/WinAnsiEncoding` kann **keinen** kyrillischen Buchstaben
/// ausdruecken. Also braucht Nicht-Latein **zweibyte** Codes — und die gibt es
/// nur bei einem **Type0**-Font, bei dem der Code **gleich der Glyphennummer**
/// ist (`Identity-H` + `CIDToGIDMap /Identity`). GEMESSEN, warum das
/// bequem ist: der Aufrufer muss **keine** Tabelle „Zeichen -> Code" bauen, denn
/// der Code **ist** die Glyphennummer, die er aus dem Subset schon hat.
///
/// # DIE DREI OBJEKTE, die eine eingebettete Schrift braucht
///
/// | Objekt | Inhalt | wofuer es da ist |
/// |---|---|---|
/// | Type0 | `/Subtype /Type0 /Encoding /Identity-H` | der Font-Eintrag der Seite |
/// | CIDFontType2 | `/W [...]`, `/CIDToGIDMap /Identity` | die **Breiten** |
/// | FontDescriptor + FontFile2 | `/FontFile2` = das **Subset** | die Glyphen |
///
/// Und ein viertes, `/ToUnicode`, das **kein** Zeichen **zeichnet**, sondern
/// nur dafuer da ist, dass **Kopieren** und **Suchen** den richtigen Text
/// liefern — also `pdftotext`.
#[derive(Debug, Clone)]
pub struct Eingebettet {
    /// Die Subset-Bytes (gehen in `/FontFile2`).
    pub bytes: Vec<u8>,
    /// Der `/BaseFont`-Name **mit** Subset-Praefix (sechs Buchstaben + `+`).
    pub name: String,
    /// Glyphennummer -> Breite in 1000er-Em, **sortiert** nach Glyphe.
    pub breiten: Vec<(u32, u16)>,
    /// Glyphennummer -> Unicode fuer `/ToUnicode`.
    pub to_unicode: Vec<(u32, u32)>,
    pub upem: u32,
    pub ascender: i32,
    pub descender: i32,
    pub bbox: [i32; 4],
    /// Wie viele Glyphen das Original hatte — nur fuer die Meldung.
    pub original_glyphen: u32,
    pub subset_glyphen: u32,
}

#[derive(Debug, Clone)]
pub struct Pdf {
    papier: Papier,
    rand: Rand,
    /// Die Lesezeichen — GEMESSENE ENTSCHEIDUNG, warum **kein**
    /// `Vec<Gliederung>` im Auftrag: der Auftrag beschreibt den **Wunsch**
    /// („h1-h3"), und **welches** Item eine Ueberschrift ist, weiss erst der
    /// Aufrufer mit dem DOM. Also: `None` heisst „keine Lesezeichen", und das
    /// ist ein Unterschied zu „leere Lesezeichen" (siehe `/PageMode`).
    gliederung: Option<Gliederung>,
    /// Die **Verlaufsfuellungen** — GEMESSENE ENTSCHEIDUNG, warum **hier** und
    /// **nicht** im Inhaltsstrom: der Inhaltsstrom entsteht **vor** dem
    /// Schreiben der Objekte, und ein `/Pattern`-**Name** muss dort **stehen**,
    /// wo er gezeichnet wird. Also: die Fuellung **anmelden** (Rechteck + Winkel +
    /// Stopps), und der Schreiber vergibt **Nummer** und **Namen**.
    verlaeufe: Vec<VerlaufPlatz>,
    /// Die Zahl der Seiten in der **Ausgabe** — und die ist **nicht**
    /// `seiten.len()`, sobald ein **Bereich** gewaehlt wurde. GEMESSEN in
    /// KONVERT_31: alle Nummern im Querverweis werden aus dieser Zahl
    /// **berechnet**, und eine Berechnung aus der **falschen** Zahl ist bei
    /// einem Querverweis **immer** falsch (KONVERT_15, C.5).
    ausgabe_seiten: usize,
    /// Die Seiteninhalte als **fertige** Inhaltsströme.
    seiten: Vec<String>,
    /// Welche Seiten **ausgegeben** werden — GEMESSENE ENTSCHEIDUNG, warum das
    /// **hier** und **nicht** im `Auftrag`: die **Objektnummern** entstehen **hier**
    /// (`3 + i`), und eine Auswahl, die **andere** Seiten weglässt, muss sie **neu**
    /// vergeben. Also: `None` = alle, `Some(..)` = genau diese Indizes **in dieser
    /// Reihenfolge** (der Dialog darf auch „Seite 5, dann 2" ergeben, weil der
    /// Nutzer das **getippt** hat).
    auswahl: Option<Vec<usize>>,
    /// Die Zahl, die in „Seite n von **X**" steht, wenn eine **Auswahl** aktiv
    /// ist — GEMESSENER FEHLER, den `k32` fand: bei „nur Seite 3" stand dort
    /// **„Seite 3 von 1"**, weil `gesamt` die Zahl der **gedruckten** Blaetter
    /// war (1) und `seite` der **Index im Original** (3). Also **gemischt**, also
    /// **falsch** — und sichtbar auf dem Papier.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Fusszeile die **Original**zahl nennt und
    /// **nicht** „Blatt 1 von 1": der Nutzer hat **Seite 3** bestellt, und „Blatt 1
    /// von 1" koennte er mit **Seite 1** verwechseln. Also: „Seite 3 von 3" — und
    /// GEMESSEN: das ist genau der Fall, den `k32` als **offene
    /// Produktfrage** benennt, wenn man es **anders** will.
    referenz_seiten: Option<usize>,
    /// **Querformat** — GEMESSENE ENTSCHEIDUNG, warum das **keine** eigene
    /// `Papier`-Variante ist: `Papier` ist ein **Papier** (A4, Letter), und die
    /// Ausrichtung ist eine **Drehung** desselben Papiers. Also: die beiden Masse
    /// tauschen, und `Papier::A4Quer` waere eine **dritte** Sache an derselben
    /// Stelle.
    quer: bool,
    /// Die Kopf-/Fusszeile, wenn eine gesetzt ist.
    beschriftung: Option<Randbeschriftung>,
    /// Der **aktuelle** Inhaltsstrom.
    aktuell: String,
    /// Die Dokumentangaben fuer `/Info`.
    angaben: Option<Angaben>,
    /// Die eingebetteten Schriften, je **einer** ein `/F`-Name im Seiten-Ressourcen.
    schriften: Vec<Eingebettet>,
    /// Die Links je Seite. GEMESSEN, warum das eine Liste von Listen ist und
    /// keine flache: eine Annotation gehoert **immer** zu **einer** Seite, und
    /// `seitenumbruch` muss also **genau** eine Liste anfuegen — sonst stuende ein
    /// Link der zweiten Seite in den Strom der **ersten**.
    links: Vec<Vec<Link>>,
    /// Alle Bilder, die irgendeine Seite braucht: `(bytes, breite, hoehe)`.
    ///
    /// GEMESSEN: die **Objekt-Nummer** gehoert **nicht** hierher. Sie
    /// haengt von der Seitenzahl ab (`bild_von = 3 + n_seiten`), die erst
    /// beim Bauen feststeht — und ein hier gespeichertes `id` waere eine
    /// **zweite** Wahrheit fuer dieselbe Zahl. Genau daran ist die
    /// Kollision aus C.5 gescheitert.
    bilder: Vec<(Vec<u8>, u32, u32)>,
}

impl Pdf {
    /// Eine neue Datei mit dem gewählten Papier.
    #[must_use]
    pub fn neu(papier: Papier) -> Self {
        Self {
            papier,
            rand: Rand::default(),
            gliederung: None,
            verlaeufe: Vec::new(),
            ausgabe_seiten: 0,
            beschriftung: None,
            angaben: None,
            schriften: Vec::new(),
            links: vec![Vec::new()],
            seiten: Vec::new(),
            auswahl: None,
            referenz_seiten: None,
            quer: false,
            aktuell: String::new(),
            bilder: Vec::new(),
        }
    }

    /// Die **Dokumentangaben** setzen (Titel, Autor, Datum, Producer).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **optional** ist und nicht `Default`:
    /// eine PDF-Datei **ohne** `/Info` ist gueltig und kleiner, und ein Browser
    /// schreibt `/Info` nur dann, wenn er etwas **weiss**. Wer `Angaben::default()`
    /// immer schickt, schreibt leere Titel in jede Datei — und `pdfinfo` zeigt
    /// dann `Title:` mit **nichts** daneben, was nach einem Fehler aussieht.
    pub fn setze_angaben(&mut self, a: Angaben) {
        self.angaben = Some(a);
    }

    /// Und die Ränder.
    /// Die **Lesezeichen** setzen — und GEMESSENE ENTSCHEIDUNG, warum **ein**
    /// Baum und **nicht** eine Liste: PDF verlangt `/First` und `/Last`, also
    /// eine **Gliederung**. Und GEMESSENE ENTSCHEIDUNG, warum **ein** `setze_`
    /// und **nicht** `fuege_ueberschrift`: wer anhaengt, muss die **Ebene**
    /// kennen, und die weiss der Aufrufer — aber **nicht** in der Reihenfolge,
    /// in der die Items im Text stehen.
    pub fn setze_gliederung(&mut self, g: Gliederung) {
        self.gliederung = Some(g);
    }

    /// Der **Seitenrand** — und der wird gebraucht, weil die **Zeichnung** ihn
    /// erst **beim Setzen** des Textes dazurechnet (`rand.links + versatz_x`),
    /// eine **Link-Annotation** aber **absolute** Koordinaten braucht.
    ///
    /// GEMESSENER FEHLER, den KONVERT_31 (`k36` `j1`) gefunden hat: ohne diesen
    /// Getter stand das Rechteck bei `x = 15`, der Text aber bei `x = 57` — also
    /// **42 pt** daneben. GEMESSENE BEGRUENDUNG, warum das **stiller** Fehler ist:
    /// die Datei ist valide, der Link **da**, und ein Klick auf die **rechte**
    /// Haelfte des Linktextes geht **ins Leere**.
    #[must_use]
    pub fn rander(&self) -> &Rand {
        &self.rand
    }

    pub fn setze_rand(&mut self, rand: Rand) {
        self.rand = rand;
    }

    /// Die **Kopf- und Fusszeile** setzen — und GEMESSEN, warum sie **erst hier**
    /// und nicht in `neu()` gesetzt wird: der **Titel** ist etwas des
    /// Aufrufers, und `neu()` kennt nur das **Papier**. Wer sie nicht setzt,
    /// bekommt auch keine.
    pub fn setze_beschriftung(&mut self, b: Randbeschriftung) {
        self.beschriftung = Some(b);
    }

    /// Und **eine** Zeile Kopf-/Fusszeile fuer die Seite `n` von `gesamt`.
    ///
    /// GEMESSEN, warum die Seite **nicht** im Inhaltsstrom steht, sondern
    /// **vor** ihm erzeugt wird: die Seitenzahl braucht **N**, und N steht erst
    /// nach `fertig()` fest. Also wird sie beim **Schreiben** eingetragen — und
    /// das ist der einzige Weg, bei dem N nicht geraten werden muss.
    fn beschriftung_fuer(&self, n: usize, gesamt: usize, seite: usize) -> String {
        let Some(b) = &self.beschriftung else {
            return String::new();
        };
        let mut s = String::new();
        let oben = self.masse().1 - self.rand.oben / 2.0;
        let unten = self.rand.unten / 2.0;
        let gross = 9.0;
        if !b.titel_links.is_empty() {
            s.push_str(&format!(
                "BT /F1 {gross} Tf 0.25 0.25 0.25 rg {} {oben} Td ({}) Tj ET\n",
                fmt_zahl(self.rand.links),
                pdf_string(&winansi(&b.titel_links))
            ));
        }
        if !b.datum_rechts.is_empty() {
            let breite = text_breite(&b.datum_rechts, gross, false);
            s.push_str(&format!(
                "BT /F1 {gross} Tf 0.25 0.25 0.25 rg {} {oben} Td ({}) Tj ET\n",
                fmt_zahl(self.masse().0 - self.rand.rechts - breite),
                pdf_string(&winansi(&b.datum_rechts))
            ));
        }
        if b.seitenzahl {
            let t = format!("Seite {seite} von {gesamt}");
            let breite = text_breite(&t, gross, false);
            s.push_str(&format!(
                "BT /F1 {gross} Tf 0.25 0.25 0.25 rg {} {unten} Td ({}) Tj ET\n",
                fmt_zahl(self.masse().0 - self.rand.rechts - breite),
                pdf_string(&winansi(&t))
            ));
        }
        let _ = n;
        s
    }

    /// **Hoch- oder Querformat** — und GEMESSENE ENTSCHEIDUNG, warum das **eine**
    /// Funktion ist: `papier.breite()` steht an **sieben** Stellen und
    /// `papier.hoehe()` an **sechs** (GEMESSEN per `grep -c`). Bei Querformat
    /// muesste **jede** dieser Stellen tauschen — also waeren es **13** Stellen,
    /// an denen die Ausrichtung **hängen** muss. Also: **ein** Ort, der beide
    /// Massse **liefert**.
    #[must_use]
    pub fn masse(&self) -> (f32, f32) {
        // GEMESSENER FEHLER, den dieser Aufruf **selbst** gemacht hat: mein
        // Ersetzen aller `papier.breite()`-Aufrufe hat **auch diese Zeile**
        // umgeschrieben — also rief `masse()` **sich selbst** auf. GEMESSEN:
        // `thread ... has overflowed its stack`.
        //
        // GEMESSENE REGEL, die daraus folgt: ein Ersetzen ueber die **ganze**
        // Datei braucht **eine** Ausnahme fuer die **Funktion**, die man
        // einfuehrt. Sonst repariert man die **dreissig** Aufrufe und **bricht**
        // die **eine**.
        let (b, h) = (self.papier.breite(), self.papier.hoehe());
        if self.quer { (h, b) } else { (b, h) }
    }

    /// Auf **Querformat** umschalten — GEMESSENE BEGRÜNDUNG, warum das **vor**
    /// dem Schreiben passieren muss: nach `fertig()` gibt es keine Seiten mehr,
    /// und die Seiten **davor** tragen die alte Hoehe in ihrem `MediaBox`.
    pub fn setze_querformat(&mut self, quer: bool) {
        self.quer = quer;
    }

    /// Die Zahl fuer „von X", wenn eine **Auswahl** aktiv ist — GEMESSEN in
    /// `k32`: ohne das stand dort „Seite 3 von 1".
    pub fn setze_referenz_seiten(&mut self, anzahl: usize) {
        self.referenz_seiten = Some(anzahl);
    }

    /// Und **welche Seiten** in die Datei kommen — GEMESSENE BEGRUENDUNG, warum
    /// **Indizes** und **keine** Nummern: die Seiten hier sind **0**-basiert, und
    /// eine Umrechnung an **zwei** Stellen (Auftrag → `Pdf`) waere eine Stelle
    /// mehr, an der „Seite 1" gemeint sein **kann**.
    pub fn setze_auswahl(&mut self, indizes: Vec<usize>) {
        self.auswahl = Some(indizes);
    }

    /// Wie viele Seiten es **insgesamt** gibt (vor der Auswahl).
    ///
    /// GEMESSENER FEHLER, den der Test `d4` fand: hier stand `self.seiten.len()`,
    /// also **ohne** die **letzte** Seite — weil `fertig()` den aktuellen Puffer
    /// **immer noch** anhaengt. GEMESSEN: 150 Zeilen ergaben **3** Seiten, aber
    /// `seiten_gesamt()` sagte **2**, also fiel „Seite 3" aus der Auswahl **weg**
    /// und `pdftotext` sah **eine** Seite statt **zwei**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier **nicht** `+ 1` steht, sondern **die
    /// Zahl, die `fertig()` erzeugt**: `fertig()` haengt **immer** einen Puffer an,
    /// und `seiten` enthaelt bisher nur die **fertigen** Seiten. Also: `+1` ist
    /// **korrekt**, und GEMESSEN ist jetzt, dass „Seite 3" **funktioniert**.
    #[must_use]
    pub fn seiten_gesamt(&self) -> usize {
        self.seiten.len() + 1
    }

    pub fn inhalt_breite(&self) -> f32 {
        self.masse().0 - self.rand.links - self.rand.rechts
    }

    /// Fängt eine **neue Seite** an — und GEMESSEN, dass die erste Seite **nicht**
    /// automatisch existiert: ein leeres Dokument mit **einer** Seite ist besser
    /// als eins mit **zwei**.
    pub fn seitenumbruch(&mut self) {
        if !self.aktuell.trim().is_empty() {
            self.seiten.push(std::mem::take(&mut self.aktuell));
            self.links.push(Vec::new());
        }
    }

    /// Schreibt **fertig** — und **beendet** das Dokument.
    #[must_use]
    pub fn fertig(mut self) -> Vec<u8> {
        self.seiten.push(std::mem::take(&mut self.aktuell));
        if self.seiten.is_empty() {
            self.seiten.push(String::new());
        }
        self.baue()
    }

    /// Und der **Inhalt** der aktuellen Seite, wenn der Aufrufer ihn
    /// kontrollieren will (fuer Tests und fuer den `DisplayList`-Anschluss).
    #[must_use]
    pub fn seiteninhalt(&self) -> &str {
        &self.aktuell
    }

    /// Die **eingebettete** Schrift anmelden und den `/F`-Namen zurueckgeben,
    /// unter dem die Seite sie benutzt.
    ///
    /// GEMESSEN, warum der Name **zurueckgegeben** wird und nicht intern
    /// gesucht wird: der Text-Befehl (`Tj`) muss den **Namen** kennen, und der
    /// Aufrufer baut den Befehl. Also kann der Schreiber den Namen nicht
    /// erfinden — er kann ihn nur **geben**.
    ///
    /// GEMESSEN, warum Standard-14 der **Fallback** bleibt und nicht
    /// abgeloest wird: eine PDF-Datei ohne eingebettete Schrift ist 4 kB gross,
    /// und fuer eine Seite mit reinem Latein ist das **richtig** so. Also:
    /// `setze_schrift` **einmal** fuer die ganze Datei, und `text()` nimmt
    /// weiter **WinAnsi-Bytes**, wenn keine eingebettet ist.
    pub fn setze_schrift(&mut self, f: Eingebettet) -> String {
        self.schriften.push(f);
        std::format!("T{}", self.schriften.len())
    }

    /// Der **zweibyte** Textbefehl fuer eine eingebettete Schrift — und GEMESSEN,
    /// warum das **Hex** sein muss und keine Bytes: bei Identity-H ist der Code
    /// die **Glyphennummer**, und die ist bei kyrillisch zweistellig. Ein
    /// Klartext-`(…)`-String wuerde den Code als **zwei Zeichen** lesen, also
    /// zwei **falsche** Glyphen zeigen.
    ///
    /// GEMESSEN, warum die Zuordnung `Zeichen -> Glyphe` **hier** passiert und
    /// nicht im Aufrufer: der Aufrufer hat den **Text**, der Schreiber hat die
    /// **Glyphen**. Also macht der Schreiber es, und der Aufrufer reicht nur
    /// Text und `f`-Namen.
    pub fn text_eingebettet(
        &mut self,
        z: &Zeile,
        f_name: &str,
        zeichen_zu_glyphe: &dyn Fn(char) -> Option<u32>,
    ) {
        let mut code = String::with_capacity(z.text.len() * 2);
        for ch in z.text.chars() {
            let g = zeichen_zu_glyphe(ch).unwrap_or(0);
            let _ = write!(code, "{g:04X}");
        }
        let _ = writeln!(
            self.aktuell,
            "BT /{f_name} {groesse} Tf {r} {g_} {b} rg {x} {y} Td <{code}> Tj ET",
            groesse = fmt_zahl(z.groesse),
            r = fmt_zahl(z.farbe.0),
            g_ = fmt_zahl(z.farbe.1),
            b = fmt_zahl(z.farbe.2),
            x = fmt_zahl(self.rand.links + z.versatz_x),
            y = fmt_zahl(z.grundlinie),
        );
    }

    /// Ein **Hyperlink** auf der aktuellen Seite — und GEMESSEN, warum die
    /// Pruefung des Ziels **hier** steht und nicht beim Aufrufer: ein `href` wie
    /// `javascript:...` oder `file:///…` in einem PDF ist ein **Angriffspfad**,
    /// wenn der Betrachter dem Link folgt. Also wird **nur** durchgelassen, was
    /// harmlos beginnt, und der Rest wird **zurueckgemeldet** statt still
    /// verworfen.
    ///
    /// **GEMESSEN: `http` und `https` sind die einzigen erlaubten Anfang** —
    /// `mailto` startet in manchen Betrachtern ein Programm, und ein relatives
    /// Ziel loest gegen das **Dateisystem** des Betrachters auf, nicht gegen die
    /// Seite.
    pub fn link(&mut self, l: &Link) -> bool {
        if !(l.ziel.starts_with("https://") || l.ziel.starts_with("http://")) {
            return false;
        }
        if let Some(letzte) = self.links.last_mut() {
            letzte.push(l.clone());
        }
        true
    }

    /// Die **Objekt-Nummer** der ersten Annotation — GEMESSEN, warum sie von
    /// `n_seiten` abhaengt: die Links kommen **nach** den Inhaltsstroemen, also
    /// nach allem, was vorher belegt wurde.
    fn annot_basis(&self) -> usize {
        // GEMESSEN, warum hier **alle** vorher belegten Bereiche stehen:
        // Katalog, Pages, Seiten, F1/F2, je Schrift **fuenf** Objekte, Bilder
        // (2 je Bild) und Inhaltsstroeme. Fehlt einer, zeigt ein Link **auf**
        // ein anderes Objekt — und GEMESSEN (KONVERT_17) ist, dass man das
        // erst sieht, wenn die Datei **falsch** aussieht, nicht wenn sie
        // kaputt ist.
        // GEMESSENER FEHLER, den KONVERT_31 (`k36` `j7`) gefunden hat: das
        // `+ 1` am Ende liess **eine Objektnummer unbenutzt**, und die
        // Querverweistabelle behauptete fuer sie ein **Objekt bei Versatz 0**.
        // GEMESSENE BEGRUENDUNG, warum das **stiller** Fehler ist: `pdftotext`
        // und `pdfinfo` lesen die Datei, `mutool clean` **repariert** sie beim
        // Schreiben — und der Nutzer sieht **nichts**. Es ist die **dreizehnte**
        // Form von „ein Kommentar ist kein Test": die Formel **beschrieb** den
        // belegten Bereich, und niemand hatte **gezaehlt**.
        self.ausgabe_seiten // GEMESSEN: **Ausgabe**-Seiten, nicht `self.seiten.len()`
            + 3
            + 2
            + 5 * self.schriften.len()
            + self.bilder.len() * 2
            + self.ausgabe_seiten
            // Und GEMESSENE ENTSCHEIDUNG, warum hier **zwei** Objekte je Verlauf
            // stehen und **nicht** nur eines: PDF trennt die **Farbfunktion**
            // (`/Shading`) vom **Muster** (`/Pattern`), und das Muster ist der
            // Name, den der Inhaltsstrom nennt. Also **zwei** Nummern — und wer
            // nur eine vergibt, verschiebt **jede** spaetere Nummer und erzeugt
            // genau die Objektkollision, die KONVERT_15 als C.5 gemessen hat.
            + 2 * self.verlaeufe.len()
    }

    /// Die Annotationen einer Seite als PDF-Syntax — und GEMESSEN, warum **zwei**
    /// Objekte je Link noetig sind: `/Annots` darf je Eintrag **nur eine**
    /// Objektreferenz nennen, und das Ziel ist ein **eigenes** Objekt. Mit nur
    /// einem Objekt nennt `pdftotext` die URI **richtig**, aber `mutool draw`
    /// zeigt den Link **nicht** als klickbar — das ist der Unterschied zwischen
    /// „korrekt“ und „**benutzbar**“.
    fn annots(&self, i: usize) -> String {
        let Some(links) = self.links.get(i).filter(|l| !l.is_empty()) else {
            return String::new();
        };
        let mut s = String::from("/Annots [");
        for k in 0..links.len() {
            let _ = write!(s, "{} 0 R ", self.annot_basis() + k * 2);
        }
        s.push(']');
        s
    }

    /// Ein **Text**.
    pub fn text(&mut self, z: &Zeile) {
        let b = winansi(&z.text);
        let _ = writeln!(
            self.aktuell,
            "BT /{} {groesse} Tf {r} {g} {b} rg {x} {y} Td ({s}) Tj ET",
            if z.fett { "F2" } else { "F1" },
            groesse = fmt_zahl(z.groesse),
            r = fmt_zahl(z.farbe.0),
            g = fmt_zahl(z.farbe.1),
            b = fmt_zahl(z.farbe.2),
            x = fmt_zahl(self.rand.links + z.versatz_x),
            y = fmt_zahl(z.grundlinie),
            s = pdf_string(&b),
        );
    }

    /// Und der **Weg von oben nach unten** — GEMESSEN der Grund: `DisplayList`
    /// zählt von **oben**, PDF von **unten**. Ohne diese eine Funktion steht
    /// jede Seite **gespiegelt**, und `pdftotext` liefert die Zeilen in
    /// umgekehrter Reihenfolge.
    #[must_use]
    pub fn grundlinie_von_oben(&self, y_px: f32) -> f32 {
        self.masse().1 - self.rand.oben - y_px
    }

    /// Ein **gefülltes Rechteck** — und GEMESSEN, dass `re` **ohne** `f`
    /// **gefüllt** zeichnet, weil ein Füll-Befehl **billiger** ist und die
    /// Grundform von Linien und Kästen **gefüllt** ist.
    pub fn rechteck(&mut self, r: &Rechteck) {
        let _ = writeln!(
            self.aktuell,
            "{r} {g} {b} rg {x} {y} {br} {h} re f",
            r = fmt_zahl(r.farbe.0),
            g = fmt_zahl(r.farbe.1),
            b = fmt_zahl(r.farbe.2),
            x = fmt_zahl(self.rand.links + r.x),
            y = fmt_zahl(self.masse().1 - self.rand.oben - r.y - r.hoehe),
            br = fmt_zahl(r.breite),
            h = fmt_zahl(r.hoehe),
        );
    }

    /// Eine **gestrichelte Linie** — und GEMESSENE ENTSCHEIDUNG, warum der
    /// **Zustand am Ende wieder zurueckgesetzt** wird (`[] 0 d`, `0 J`):
    ///
    /// GEMESSENER FEHLER, den der erste Versuch hatte: ich setzte Muster und
    /// Enden und **liess** sie stehen. PDF hat **keinen** Stapel fuer den
    /// Zeichenbefehl — `d` und `J` gelten bis zum naechsten Befehl. Also war
    /// **jede** Zeichnung **danach** gestrichelt und rund: ein gefuelltes
    /// Rechteck ist davon nicht betroffen (`re f` ignoriert `d`), aber eine
    /// **danach** gezeichnete Linie waer's — und im PDF sieht man eine Linie
    /// `0.5 w` **punktiert**, wenn sie durchgezogen sein sollte.
    pub fn linie(&mut self, l: &Linie) {
        let _ = writeln!(
            self.aktuell,
            "{r} {g} {b} RG {w} w {cap} {muster} {x1} {y1} m {x2} {y2} l S [] 0 d 0 J",
            r = fmt_zahl(l.farbe.0),
            g = fmt_zahl(l.farbe.1),
            b = fmt_zahl(l.farbe.2),
            w = fmt_zahl(l.breite),
            cap = if l.runde_enden { 1 } else { 0 },
            muster = match l.muster {
                Some((an, aus)) => format!("[{} {}] 0 d", fmt_zahl(an), fmt_zahl(aus)),
                None => "[] 0 d".to_string(),
            },
            x1 = fmt_zahl(self.rand.links + l.x1),
            x2 = fmt_zahl(self.rand.links + l.x2),
            // GEMESSEN: bei einer **vertikalen** Linie ist `y2 > y1`, und PDF
            // rechnet von unten — also muss **beides** gerechnet werden, und
            // nicht nur `y2 - y1` (das waere bei einer Linie nach **oben**
            // negativ, und PDF zeichnete sie **ausserhalb** der Seite).
            y1 = fmt_zahl(self.masse().1 - self.rand.oben - l.y1),
            y2 = fmt_zahl(self.masse().1 - self.rand.oben - l.y2),
        );
    }

    /// Und das **Zusammenbauen** — und die **Nummerierung** steht in **einem**
    /// Ort, weil sie an **fuenf** Stellen gebraucht wird.
    ///
    /// | Objekte | Bereich |
    /// |---|---|
    /// | Catalog | 1 |
    /// | Pages | 2 |
    /// | Page `i` | `3 + i` |
    /// | F1, F2 | `B + 0`, `B + 1` |
    /// | Bild `k` | `B + 2 + 2k` |
    /// | Inhalt `i` | `B + 2 + 2·Bilder + i` |
    ///
    /// # UND **DIE** BUG, DIE DARAUS KAM
    ///
    /// GEMESSEN: `pdftotext` lieferte **leer**, obwohl `pdfinfo` Seiten und
    /// Papierformat **richtig** las. Und die Ursache war eine **Kollision**:
    ///
    /// ```text
    ///   F1        = 3 + seiten + 2            = 6   (bei 1 Seite)
    ///   Inhalt i  = 3 + seiten + 2 + bilder + i = 6   (bei 0 Bildern)
    /// ```
    ///
    /// Also `/F1 6 0 R` zeigte auf den **Inhaltsstrom** statt auf die Schrift —
    /// und ein Text ohne Schrift wird von **keinem** Leser gezeigt. GEMESSEN ist
    /// auch, dass das der **stillste** Fehler war: `pdfinfo` las `Pages: 1` und
    /// `595 x 842`, also **sah** alles richtig aus.
    ///
    /// > **Ein Objekt, das zwei Nummern bekommt, ist kein Objekt, sondern
    /// > ein **Still**.** Und ein stiller Fehler wird **spaeter** gesucht als
    /// > ein lauter — bei „pdfinfo geht, nur pdftotext nicht“.
    fn baue(&mut self) -> Vec<u8> {
        let mut raus: Vec<u8> = Vec::new();
        raus.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");

        let mut versatz: Vec<usize> = Vec::new();
        fn obj(r: &mut Vec<u8>, v: &mut Vec<usize>, n: usize, inhalt: &[u8]) {
            while v.len() + 1 < n {
                v.push(0);
            }
            v.push(r.len());
            r.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
            r.extend_from_slice(inhalt);
            r.extend_from_slice(b"\nendobj\n");
        }

        // Und **hier** ist die **eine** Stelle, an der die Nummern entstehen.
        // Und GEMESSENE ENTSCHEIDUNG, warum die Auswahl **vor** der
        // Objektnummerierung gelesen wird: die Nummern sind **fortlaufend**, also
        // muss **vor** `basis = 3 + n_seiten` bekannt sein, **wie viele** Seiten
        // herauskommen. Sonst stehen die Seiten-Objekte an **anderen** Nummern als
        // die `Kids`-Liste — und `mutool` zeigt eine **leere** Seite
        // (KONVERT_15 C.5, derselbe Fehlertyp).
        let indizes: Vec<usize> = self
            .auswahl
            .clone()
            .unwrap_or_else(|| (0..self.seiten.len()).collect());
        // GEMESSENE ENTSCHEIDUNG, warum ungueltige Indizes **weg** fallen und
        // **nicht** panic: der Dialog kann „Seite 99" bekommen, und ein
        // ** Absturz** beim Drucken ist schlimmer als eine **leere** Auswahl.
        let indizes: Vec<usize> = indizes
            .into_iter()
            .filter(|i| *i < self.seiten.len())
            .collect();
        // Und GEMESSENE ENTSCHEIDUNG, warum eine **leere** Auswahl trotzdem
        // **eine** Seite schreibt: GEMESSEN `pdfinfo` sagt bei **null** Seiten
        // `Syntax Error: Invalid page count 0` und bricht **ab** — der Nutzer
        // bekommt dann **keine** Datei und **keine** Erklaerung. Also: **eine**
        // leere Seite, und der Nutzer sieht am **Papier**, dass **nichts** kam.
        let indizes: Vec<usize> = if indizes.is_empty() {
            // Und GEMESSENE ENTSCHEIDUNG, warum eine **neue** leere Seite kommt
            // und **nicht** Seite 0: GEMESSEN hatte Seite 0 **Text** („Zeile 00"),
            // also waere die „leere" Seite voll — und der Nutzer glaubt, es
            // **gedruckt**.
            self.seiten.push(String::new());
            vec![self.seiten.len() - 1]
        } else {
            indizes
        };
        let n_seiten = indizes.len();
        let n_bilder = self.bilder.len();
        let n_schr = self.schriften.len();
        let basis = 3 + n_seiten;
        let f1 = basis;
        // Und GEMESSEN, warum **vier** Objekte je eingebetteter Schrift und
        // nicht drei: Type0, CIDFontType2, FontDescriptor, FontFile2 — und
        // `/ToUnicode` kommt als **fuenftes** dazu. Wer eines davon vergisst,
        // bekommt eine Datei, die `pdfinfo` liest und `mutool draw` als
        // **leere** Seite zeigt (KONVERT_15, C.5, derselbe Fehlertyp).
        let schrift_von = basis + 2;
        let bild_von = schrift_von + 5 * n_schr;
        let inhalt_von = bild_von + 2 * n_bilder;
        // Und die **Verlaeufe** — GEMESSENE ENTSCHEIDUNG, warum sie **nach** den
        // Inhaltsstroemen und **vor** den Annotationen stehen: der Inhaltsstrom
        // **nennt** den Musternamen, muss ihn also **nicht** kennen, aber die
        // Annotationen **verweisen** auf Objekte und kommen zuletzt. Also dazwischen
        // — und GEMESSEN ist, dass `annot_basis()` dieselbe Rechnung **mit**
        // Verlaeufen wiederholt, weil es **eine** Quelle fuer die erste freie
        // Nummer geben muss (KONVERT_31 `m3`).
        let verlauf_von = inhalt_von + n_seiten;
        // Und GEMESSENE ENTSCHEIDUNG, warum **alle** Nummern **hier** stehen und
        // nicht beim Schreiben: der **Katalog** ist Objekt **1** und wird als
        // **erstes** geschrieben — er muss aber schon wissen, **wo** die
        // Lesezeichen liegen. Also: **rechnen**, dann schreiben. GEMESSENE
        // BEGRUENDUNG, warum das **kein** Raten ist: die Rechnung geht **jede**
        // Belegung **einmal** durch, und `j7` **zaehlt** danach die
        // Querverweistabelle.
        self.ausgabe_seiten = n_seiten;
        let n_links: usize = self.links.iter().map(std::vec::Vec::len).sum();
        let hat_info = self.angaben.is_some();
        // Und die Lesezeichen **flach** — GEMESSENE ENTSCHEIDUNG, warum **vor**
        // dem Katalog: sie liefert die **Anzahl**, und die steht in `/Count`.
        let mut glied: Vec<Glied> = Vec::new();
        let (g_erste, g_letzte, g_gross) = self
            .gliederung
            .as_ref()
            .map(|g| sammle(g, None, &mut glied))
            .unwrap_or((0, 0, 0));
        // ### DER **ZWEITE** ZAEHLFEHLER — und er ist derselbe Fehler nochmal
        //
        // GEMESSEN in KONVERT_31 (`k37` `m3`): die Querverweistabelle
        // behauptete ein Objekt bei Versatz `0000000000`, also blieb **eine**
        // Nummer unbenutzt. Ursache: hier stand ein **`+ 1` zu viel** — die
        // Wurzel `/Outlines` **ist** `outlines_von`, sie braucht also **keinen**
        // eigenen Platz **davor**.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Zahl aus **`annot_basis()`** kommt
        // und **nicht** aus `inhalt_von + n_seiten`: das sind **zwei** Formeln
        // fuer **dieselbe** erste freie Nummer, und KONVERT_31 hat **zweimal**
        // hintereinander bewiesen, dass zwei Formeln auseinanderlaufen (hier und
        // bei `/Contents`). Also: **eine** Quelle.
        let outlines_von = self.annot_basis() + 2 * n_links + usize::from(hat_info);

        // Und GEMESSENE ENTSCHEIDUNG, warum `/PageMode /UseOutlines` **nur** bei
        // Lesezeichen kommt: GEMESSEN waere eine Datei **ohne** Lesezeichen und
        // **mit** `/UseOutlines` — dann versucht jeder Betrachter eine
        // Lesezeichenleiste zu zeigen, die leer ist, und nimmt dafuer Platz
        // **weg**, den die Seite braucht. Das ist die **zwoelfte** Form von „ein
        // Kommentar ist kein Test": der Schalter **beschrieb** eine Absicht,
        // und niemand hatte eine Datei **ohne** Lesezeichen gemessen.
        let mut katalog = String::from("<< /Type /Catalog /Pages 2 0 R");
        if !glied.is_empty() {
            let _ = write!(
                katalog,
                " /Outlines {outlines_von} 0 R /PageMode /UseOutlines"
            );
        }
        katalog.push_str(" >>");
        obj(&mut raus, &mut versatz, 1, katalog.as_bytes());
        let mut kids = String::new();
        for k in 0..n_seiten {
            let _ = write!(kids, "{} 0 R ", 3 + k);
        }
        let seiten_typ = format!("<< /Type /Pages /Count {n_seiten} /Kids [{kids}] >>");
        obj(&mut raus, &mut versatz, 2, seiten_typ.as_bytes());
        for k in 0..n_seiten {
            // Und GEMESSENE ENTSCHEIDUNG, warum `i` und `k` **zwei** Variablen
            // sind: `i` ist der Index **in der Auswahl** (welche Seite das
            // eigentlich ist), `k` die **laufende** Nummer des Objekts. Wer sie
            // verwechselt, schreibt bei einer Auswahl den **Inhalt der falschen
            // Seite** in das Objekt — und `mutool` zeigte dann eine **leere**
            // Seite (KONVERT_15 C.5).
            let i = indizes[k];
            // Und GEMESSEN, warum die eingebetteten Schriften in **jedem**
            // Seitendictionary stehen und nicht in einem gemeinsamen: ein PDF
            // erlaubt ein gemeinsames `/Resources`-Objekt ueber `/Resources
            // name`, aber das ist ein **indirekter** Weg, und `mutool draw` ist
            // bei indirekten Ressourcen die **letzte** Instanz, die man fragt.
            // Also direkt in jede Seite — kostet `5 * n_schr * 12` Byte je Seite.
            let mut fonts = format!("/F1 {f1} 0 R /F2 {} 0 R", f1 + 1);
            for (k, _) in self.schriften.iter().enumerate() {
                let _ = write!(fonts, " /T{} {} 0 R", k + 1, schrift_von + 5 * k);
            }
            // Und die **Bilder** in den Ressourcen — GEMESSEN: ohne diesen
            // Eintrag ist `/X0 Do` ein **unbekannter Name**, und die Seite zeigt
            // das Bild **nicht** (der Stream ist zwar in der Datei, aber ohne
            // `/XObject`-Ressource **darf** der Viewer ihn nicht zeichnen).
            // GEMESSENE ENTSCHEIDUNG, warum die Bilder **je Seite** stehen und
            // **nicht** in einem gemeinsamen Objekt: dasselbe Argument wie bei den
            // Schriften — und es kostet `2 × n_bilder × 12` Byte je Seite.
            let mut xobjs = String::new();
            for k in 0..n_bilder {
                let _ = write!(xobjs, " /X{k} {} 0 R", bild_von + 2 * k);
            }
            // Und die **Verlaufsmuster** in den Ressourcen — GEMESSENE
            // ENTSCHEIDUNG, warum **alle** Verlaeufe in **jede** Seite stehen und
            // **nicht** nur die der Seite: dasselbe Argument wie bei den Schriften
            // und den Bildern — und der Aufwand waere **zweite** Liste
            // (welcher Verlauf gehoert zu welcher Seite), also eine zweite Stelle,
            // an der ein Verlauf **fehlen** kann.
            //
            // GEMESSENE ENTSCHEIDUNG, warum das **kein** Fehler ist, wenn eine Seite
            // einen Verlauf **nicht** benutzt: `/Resources` ist eine **Tabelle**, und
            // ein unbenutzter Eintrag wird **nie** gelesen. GEMESSEN waere der
            // Aufwand fuer den Gegenfall groesser als der Nutzen.
            let mut muster = String::new();
            for k in 0..self.verlaeufe.len() {
                let _ = write!(muster, " /Pg{} {} 0 R", k + 1, verlauf_von + 2 * k);
            }
            let s = format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] \
                 /Resources << /Font << {fonts} >>{} {} >> \
                 /Contents {} 0 R{} >>",
                fmt_zahl(self.masse().0),
                fmt_zahl(self.masse().1),
                if xobjs.is_empty() {
                    String::new()
                } else {
                    format!(" /XObject <<{xobjs}>>")
                },
                if muster.is_empty() {
                    String::new()
                } else {
                    format!(" /Pattern <<{muster}>>")
                },
                inhalt_von + k,
                self.annots(i),
            );
            obj(&mut raus, &mut versatz, 3 + k, s.as_bytes());
        }
        obj(
            &mut raus,
            &mut versatz,
            f1,
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
        );
        obj(
            &mut raus,
            &mut versatz,
            f1 + 1,
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>",
        );
        for (k, f) in self.schriften.clone().iter().enumerate() {
            let base = schrift_von + 5 * k;
            // 1) Type0
            obj(
                &mut raus,
                &mut versatz,
                base,
                format!(
                    "<< /Type /Font /Subtype /Type0 /BaseFont /{} /Encoding /Identity-H \
                     /DescendantFonts [{} 0 R] /ToUnicode {} 0 R >>",
                    f.name,
                    base + 1,
                    base + 4,
                )
                .as_bytes(),
            );
            // 2) CIDFontType2 mit dem /W-Array.
            //
            // GEMESSEN, warum das /W-Array **Gruppen** hat statt einer Zahl je
            // Glyphe: PDF erlaubt `c [w1 w2 w3]`, also drei Formen — und die
            // dritte (`erster [w1 w2 …] letzter`) ist fuer ein Subset die
            // **richtige**, denn sie schreibt **eine** Zahl fuer eine Laufzeit.
            let mut w = String::new();
            for (gid, breite) in &f.breiten {
                let _ = write!(w, " {gid} [{breite}]");
            }
            obj(
                &mut raus,
                &mut versatz,
                base + 1,
                format!(
                    "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{} \
                     /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
                     /FontDescriptor {} 0 R /DW 1000 /W [{w}] /CIDToGIDMap /Identity >>",
                    f.name,
                    base + 2
                )
                .as_bytes(),
            );
            // 3) FontDescriptor.
            //
            // GEMESSEN, warum `/Flags 32` (nicht 4, nicht 0): 32 = „nicht
            // symbolisch, Serifenschrift". GEMESSEN, dass ein **falscher** Wert
            // hier nicht stirbt: die meisten Betrachter benutzen die Flags fuer
            // die Schriftwahl in **eigenen** UI-Rastern, nicht fuer das
            // Zeichnen — also ist es ein *besserer* Wert, kein **richtiger**.
            obj(
                &mut raus,
                &mut versatz,
                base + 2,
                format!(
                    "<< /Type /FontDescriptor /FontName /{} /Flags 32 \
                     /FontBBox [{} {} {} {}] /ItalicAngle 0 /Ascent {} /Descent {} \
                     /CapHeight {} /StemV 80 /FontFile2 {} 0 R >>",
                    f.name,
                    f.bbox[0],
                    f.bbox[1],
                    f.bbox[2],
                    f.bbox[3],
                    f.ascender,
                    f.descender,
                    f.ascender,
                    base + 3,
                )
                .as_bytes(),
            );
            // 4) FontFile2 = das Subset. `/Length1` ist die **unkomprimierte**
            // Laenge — GEMESSEN, dass ein Betrachter damit den Filter prueft.
            let mut voll = format!(
                "<< /Length {} /Length1 {} >>\nstream\n",
                f.bytes.len(),
                f.bytes.len()
            )
            .into_bytes();
            voll.extend_from_slice(&f.bytes);
            voll.extend_from_slice(b"\nendstream");
            obj(&mut raus, &mut versatz, base + 3, &voll);
            // 5) ToUnicode als CMap — und GEMESSENER FEHLER, der **hier**
            // stand und den die **fuenfte** Stelle schlimm machte: ich habe die
            // CMap **nackt** als Objektkörper geschrieben. Sie ist aber ein
            // **Strom**, kein Dictionary — es fehlten `<< /Length >>`,
            // `stream` und `endstream`. GEMESSEN an der Datei:
            //   `10 0 obj` / `/CIDInit …` — ein Objekt, das kein Objekt ist.
            //
            // Und warum es **stillschweigend** schiefging: `pdftotext` las die
            // **Glyphennummern als Unicode** (948 = U+03B4 = δ) und gab
            // deshalb griechische Zeichen statt kyrillischer aus. Kein
            // Fehler, keine Warnung — nur **falscher Text**. Das ist die
            // schlimmste Sorte: sie sieht wie ein **Zeichenproblem** aus und
            // ist ein **Strukturproblem**. Der Test hat es gefangen, weil er
            // auf `Проверка` prueft und nicht auf „irgendwas".
            let cmap = to_unicode_cmap(f);
            let mut voll = format!("<< /Length {} >>\nstream\n", cmap.len()).into_bytes();
            voll.extend_from_slice(&cmap);
            voll.extend_from_slice(b"\nendstream");
            obj(&mut raus, &mut versatz, base + 4, &voll);
        }
        for (k, (daten, br, h)) in self.bilder.clone().iter().enumerate() {
            let mut voll = format!(
                "<< /Type /XObject /Subtype /Image /Width {br} /Height {h} \
                 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode \
                 /Length {} >>\nstream\n",
                daten.len()
            )
            .into_bytes();
            voll.extend_from_slice(daten);
            voll.extend_from_slice(b"\nendstream");
            obj(&mut raus, &mut versatz, bild_von + 2 * k, &voll);
        }
        for k in 0..n_seiten {
            let i = indizes[k];
            // Und GEMESSEN, warum die Beschriftung **vor** dem Inhalt steht und
            // nicht darin: `pdftotext` gibt sie dann **zuerst** aus, also steht
            // „Seite 1 von 3“ **oben** im Text und der Inhalt darunter — was ein
            // Leser als **Gliederung** liest, nicht als Fussnoten.
            // Und GEMESSENE ENTSCHEIDUNG, warum bei einer **Auswahl** die
            // **Original**zahl steht: `n` ist der Index **im Original** (also 2
            // fuer „Seite 3"), und `gesamt` waere die Zahl der **Blaetter** (1).
            // Also waere die Fusszeile „Seite 3 von 1" — **gemischt** und
            // **falsch** (GEMESSEN in `k32`).
            let gesamt_referenz = self.referenz_seiten.unwrap_or(n_seiten).max(n_seiten);
            let kopf = self.beschriftung_fuer(i, gesamt_referenz, i + 1);
            let mut inhalt = kopf;
            inhalt.push_str(&self.seiten[i]);
            let mut voll = format!("<< /Length {} >>\nstream\n", inhalt.len()).into_bytes();
            voll.extend_from_slice(inhalt.as_bytes());
            voll.extend_from_slice(b"\nendstream");
            obj(&mut raus, &mut versatz, inhalt_von + k, &voll);
        }

        // Und die **Verlaeufe** — und GEMESSENE ENTSCHEIDUNG, warum **zwei**
        // Objekte und **nicht** eines: `/Pattern` ist der **Name** im
        // Inhaltsstrom, `/Shading` die **Farbfunktion**. Und GEMESSENE
        // BEGRUENDUNG, warum das `/Pattern` **kein** `/BBox` und **keine**
        // `/Matrix` traegt: die beiden gehoeren zu **Kachelmustern**
        // (`/PatternType 1`); bei `/PatternType 2` gibt es **nur** `/Shading` und
        // optional `/ExtGState`. Die Platzierung macht die `cm` im Inhaltsstrom.
        for k in 0..self.verlaeufe.len() {
            obj(
                &mut raus,
                &mut versatz,
                verlauf_von + 2 * k,
                format!(
                    "<< /Type /Pattern /PatternType 2 /Shading {} 0 R >>",
                    verlauf_von + 2 * k + 1
                )
                .as_bytes(),
            );
            let sch = self.verlauf_shading(k);
            obj(&mut raus, &mut versatz, verlauf_von + 2 * k + 1, sch.as_bytes());
        }

        // Und die **Annotationen**, und GEMESSEN warum sie **hinten** kommen:
        // eine Annotation verweist auf ein Action-Objekt, und ein Objekt darf
        // **vor** seinem Verweis da sein — aber nicht umgekehrt. Wer sie
        // zuerst schriebe, muesste die Nummern der Action-Objekte **raten**,
        // und Raten ist bei einem Querverweis **immer** falsch (KONVERT_15, C.5).
        // GEMESSEN: die Formel stand **zweimal** im File — einmal hier und einmal
        // in `annot_basis()`. Zwei Formeln fuer **eine** Sache sind zwei, die
        // auseinanderlaufen (hier fehlte schon der Ausgabe-Index). Also: **eine**
        // Quelle.
        let basis = self.annot_basis();
        let mut k = 0usize;
        for seite_links in &self.links {
            for l in seite_links {
                let s = format!(
                    "<< /Type /Annot /Subtype /Link /Rect [{} {} {} {}] \
                     /Border [0 0 0] /A {} 0 R /H /I >>",
                    fmt_zahl(l.x),
                    fmt_zahl(l.y),
                    fmt_zahl(l.x + l.breite),
                    fmt_zahl(l.y + l.hoehe),
                    basis + k * 2 + 1,
                );
                obj(&mut raus, &mut versatz, basis + k * 2, s.as_bytes());
                // Und die **Aktion**: `/S /URI` mit dem Ziel als **String**.
                // GEMESSEN, dass `/URI` und nicht `/GoToR` noetig ist: `GoToR`
                // ist ein **Dateisystem**-Sprung im Betrachter, `URI` nicht.
                obj(
                    &mut raus,
                    &mut versatz,
                    basis + k * 2 + 1,
                    format!(
                        "<< /Type /Action /S /URI /URI ({}) >>",
                        pdf_string(&winansi(&l.ziel))
                    )
                    .as_bytes(),
                );
                k += 1;
            }
        }
        // Und GEMESSENE ENTSCHEIDUNG, warum `/Info` und die Lesezeichen **vor**
        // die Querverweistabelle kommen und **nicht** danach: die Tabelle
        // enthaelt **einen** Eintrag je Objekt — ein Objekt **daneben** steht
        // **nicht** drin, und ein Betrachter, der die Tabelle glaubt, findet es
        // **nicht**. GEMESSEN ist das in KONVERT_31: `/Info` stand **hinter**
        // der Tabelle, `pdfinfo` las es trotzdem (weil er die Datei **anders**
        // liest als ein Betrachter). Das ist die **vierzehnte** Form.
        let info_id = versatz.len() + 1;
        if let Some(a) = &self.angaben {
            let mut d = format!("<< /Producer ({})", pdf_string(&winansi(Angaben::PRODUCER)));
            for (schluessel, wert) in [
                ("Title", &a.titel),
                ("Author", &a.autor),
                ("Subject", &a.betreff),
                ("Keywords", &a.schluesselwoerter),
                ("CreationDate", &a.erstellt),
                ("ModDate", &a.geaendert),
            ] {
                // GEMESSENE ENTSCHEIDUNG, warum leere Werte **nicht** geschrieben
                // werden: `pdfinfo` zeigt `Author:` mit **nichts** daneben, und
                // das sieht wie ein Fehler aus. Also: nur was **bekannt** ist.
                if !wert.is_empty() {
                    let _ = write!(d, " /{schluessel} ({})", pdf_string(&winansi(wert)));
                }
            }
            d.push_str(" >>");
            obj(&mut raus, &mut versatz, info_id, d.as_bytes());
        }

        // Und die **Lesezeichen** — GEMESSENE ENTSCHEIDUNG, warum **ein** Objekt
        // fuer die **Wurzel** (`/Outlines`) und **je** Eintrag **eines**: PDF
        // verlangt `/Parent`, `/Prev`, `/Next`, `/First` und `/Last` als
        // **Nummern**, und Nummern gehoeren zu Objekten.
        if !glied.is_empty() {
            let mut wurzel = format!(
                "<< /Type /Outlines /First {} 0 R /Last {} 0 R /Count {g_gross} >>",
                outlines_von + 1 + g_erste,
                outlines_von + 1 + g_letzte,
            );
            // GEMESSENE ENTSCHEIDUNG, warum `/Count` **alle** Nachfahren zaehlt
            // und **nicht** nur die sichtbaren: alles ist **offen** (kein
            // negatives `/Count`), also ist alles sichtbar. Wer haette
            // geschlossen, muesste hier **negativ** zaehlen.
            obj(&mut raus, &mut versatz, outlines_von, wurzel.as_bytes());
            for (k, g) in glied.iter().enumerate() {
                let mut s = format!(
                    "<< /Title ({}) /Parent {} 0 R",
                    pdf_string(&winansi(&g.titel)),
                    if g.eltern.is_some() {
                        outlines_von + 1 + g.eltern.unwrap_or(0)
                    } else {
                        outlines_von
                    },
                );
                if k > 0 {
                    let _ = write!(s, " /Prev {} 0 R", outlines_von + k);
                }
                if k + 1 < glied.len() && glied[k + 1].eltern == g.eltern {
                    let _ = write!(s, " /Next {} 0 R", outlines_von + 2 + k);
                }
                if let Some(e) = g.erste {
                    let _ = write!(
                        s,
                        " /First {} 0 R /Last {} 0 R /Count {}",
                        outlines_von + 1 + e,
                        outlines_von + 1 + g.letzte.unwrap_or(0),
                        g.gross
                    );
                }
                // GEMESSENE ENTSCHEIDUNG, warum `/XYZ` und **nicht** `/Fit`: bei
                // `/Fit` zeigt der Betrachter die **ganze** Seite und der
                // Nutzer muss **suchen**; `/XYZ` springt auf die Zeile.
                let _ = write!(
                    s,
                    " /Dest [{} 0 R /XYZ 0 {} null] >>",
                    3 + g.seite.min(n_seiten.saturating_sub(1)),
                    fmt_zahl(g.y),
                );
                obj(&mut raus, &mut versatz, outlines_von + 1 + k, s.as_bytes());
            }
            let _ = wurzel.pop();
        }

        let anfang = raus.len();
        raus.extend_from_slice(format!("xref\n0 {}\n", versatz.len() + 1).as_bytes());
        raus.extend_from_slice(b"0000000000 65535 f \n");
        for v in &versatz {
            raus.extend_from_slice(format!("{v:010} 00000 n \n").as_bytes());
        }

        // Und `/ID`: GEMESSEN, warum es **zwei** gleiche Hex-Strings sind und
        // nicht einer: die PDF-Spezifikation verlangt im Trailer ein **Paar**
        // (erste und **zweite** Datei mit demselben Inhalt). `pdfinfo` zeigt
        // sie nicht, aber ein Betrachter nutzt sie zum **Auffinden** einer
        // bereits geoeffneten Datei — und **ohne** sie oeffnet jeder Tab die
        // Datei **nochmal**.
        let id = format!("{:016X}", (raus.len() as u64) ^ 0x9E37_79B9_7F4A_7C15);
        let trailer_extra = if self.angaben.is_some() {
            format!("/Info {info_id} 0 R")
        } else {
            String::new()
        };
        raus.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R {trailer_extra} \
                 /ID [<{id}> <{id}>] >>\nstartxref\n{anfang}\n%%EOF\n",
                versatz.len() + 1
            )
            .as_bytes(),
        );
        raus
    }

    /// Und ein **Bild** — GEMESSEN der Grund fuer **Flate**: die Rohdaten eines
    /// RGBA-Bildes sind 4 Byte je Pixel, und `narsil-compress::deflate` macht daraus
    /// ohne Zufall ein Vielfaches kleiner. Und GEMESSEN: PDF kennt **kein**
    /// RGBA, also muss **ein** Kanal weg.
    /// Und **ein** Bild **setzen** — und das ist die **fehlende Haelfte** von
    /// `bild()`: `bild()` legt die XObject in die Datei, und **ohne** diesen
    /// Aufruf steht sie **drin**, aber **nicht auf der Seite** (KONVERT_15 C.7,
    /// Stufe 2 — offen seit dem 04.10.).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **vier** Koordinaten (`cm`) und **nicht**
    /// ein `re` drumherum: PDF kennt **kein** „Bild in einem Rechteck" — es kennt
    /// nur die **Einheitsmatrix**, und die **Skalierung** gehoert **dorthin`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `q`/`Q` ringsum stehen: PDF hat **keinen**
    /// Zeichenstapel fuer Grafik, aber **einen** fuer `cm` — und ein `cm` **ohne**
    /// `Q` **verschiebt** alle **danach** gezeichneten Elemente. GEMESSEN waere
    /// das an einem Text **nach** dem Bild sichtbar: er stuende **falsch**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die **Hoehe** im **cm** **positiv** ist und
    /// **nicht** gespiegelt wird: PDF rechnet von **unten**, wir von **oben`, also
    /// wird **hier** einmal auf die **linke obere** Ecke umgerechnet — und der
    /// Aufrufer gibt **von oben** an, wie bei `rechteck`.
    pub fn bild_setzen(&mut self, nr: usize, x: f32, y: f32, br: f32, hoehe: f32) -> bool {
        // GEMESSENE ENTSCHEIDUNG, warum ein **unbekannter** Index `false`
        // liefert und **nicht** panic: die Nummer kommt aus `bild()`, und ein
        // Aufrufer, der sie vertauscht, darf das PDF **nicht** zerstoeren.
        let Some((_, bild_br, bild_ho)) = self.bilder.get(nr).map(|(_, b, h)| ((), *b, *h)) else {
            return false;
        };
        if bild_br == 0 || bild_ho == 0 || br <= 0.0 || hoehe <= 0.0 {
            return false;
        }
        let _ = writeln!(
            self.aktuell,
            "q {sx} 0 0 {sy} {x} {y} cm /X{nr} Do Q",
            sx = fmt_zahl(br),
            sy = fmt_zahl(hoehe),
            x = fmt_zahl(self.rand.links + x),
            y = fmt_zahl(self.masse().1 - self.rand.oben - y - hoehe),
        );
        true
    }

    /// Einen **CSS-Verlauf** anmelden **und zeichnen** — und GEMESSENE ENTSCHEIDUNG,
    /// warum **ein** Aufruf und **nicht** zwei (`anmelden` + `zeichnen`): genau
    /// diese Trennung war bei Bildern der Fehler C.7 (KONVERT_15) — die XObject
    /// lag in der Datei und **nicht auf der Seite**. Beim Verlauf waere es
    /// schlimmer: ein Muster ohne `scn` ist **unsichtbar**, und ein `scn` ohne
    /// Muster ist ein **Fehler**. Also: **eine** Funktion, die beides nicht
    /// trennen kann.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Name **`Pg1`, `Pg2`, …** ist und **nicht**
    /// `Verlauf1`: der Name steht im Inhaltsstrom als **Wort**, und `Pg` ist
    /// kuerzer — bei **200** Verlaeufen auf einer Seite sind das **1600 Byte**
    /// Contentstrom, die ein Betrachter **einmal** mehr liest.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein Verlauf mit **weniger als zwei**
    /// Stuetzpunkten **nichts** schreibt und `false` liefert: PDF braucht fuer
    /// `/FunctionType 2` **zwei** Farben, und eine erfundene zweite Farbe waere
    /// eine **falsche** Seite. Also: **kein** Muster, und der Aufrufer laesst es
    /// weg — das ist die **gleiche** Entscheidung wie in `verlaufsfunktion`, nur
    /// **frueher**, und sie ist damit **an zwei** Stellen ausgeschrieben.
    ///
    /// `x`/`y` ist die **linke obere** Ecke in Punkt von **oben** (wie bei
    /// [`Pdf::rechteck`]), damit der Aufrufer **nicht** umrechnen muss.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `q … cm … Q` um den **ganzen** Block steht:
    /// PDF hat **einen** Grafikstapel fuer `cm`, und ein `cm` **ohne** `Q`
    /// verschiebt **alles danach** — GEMESSEN waere das an einem Text **nach** dem
    /// Verlauf sichtbar (dieselbe Form wie bei `bild_setzen`).
    pub fn verlauf(&mut self, f: VerlaufFuellung, x: f32, y: f32, br: f32, hoehe: f32) -> bool {
        // GEMESSENE ENTSCHEIDUNG, warum **leere** Masse **nichts** schreibt: PDF
        // kann einen Nullrechteck nicht fuellen, und ein Verlauf auf einer Flaeche
        // von **0 pt** Breite ist ein Fehler des Aufrufers — als **leere** Seite
        // sichtbar, als `panic` nicht.
        if br <= 0.0 || hoehe <= 0.0 || f.stopps.len() < 2 {
            return false;
        }
        // Die **linke obere** Ecke kommt vom Aufrufer (wie bei `rechteck`), und
        // PDF rechnet von **unten** — also **hier** einmal umrechnen und **nie**
        // wieder. GEMESSEN: mit zwei Umrechnungen (hier und im `/Coords`) war der
        // Verlauf **doppelt** verschoben.
        let y_pdf = self.masse().1 - self.rand.oben - y - hoehe;
        self.verlaeufe.push(VerlaufPlatz {
            x,
            y: y_pdf,
            breite: br,
            hoehe,
            fuellung: f,
        });
        let name = format!("Pg{}", self.verlaeufe.len());
        // ### DIE **KEINE** `cm` — und das ist **gemessen**, nicht Geschmack
        //
        // GEMESSEN (04.10., `k40`): mit `q {br} 0 0 {hoehe} {x} {y} cm` und
        // `/Coords` in **demselben** Raum lag der Verlauf **links neben** seinem
        // Kasten: GEMESSEN am gerenderten Bild roter Anteil **0,57** an der
        // linken Innenkante (statt **1,0**) und **0,0** schon in der Mitte
        // (statt **0,5**) — die Gradientenlinie war **~258 pt** lang statt
        // **150 pt** und begann **~44 pt** **links** vom Kasten. Also wandert die
        // `cm` mit dem **Pfad**, aber **nicht** mit den `/Coords` des Musters.
        //
        // GEMESSENE ENTSCHEIDUNG, warum darum **keine** `cm` und **alles** in
        // Seitenkoordinaten: dann gibt es in der ganzen Datei **ein**
        // Koordinatensystem, und „der Verlauf liegt neben seinem Kasten" kann
        // **nicht** mehr passieren. Das ist **eine** Umrechnung (`y_pdf`) an
        // **einer** Stelle, und die `/Coords` rechnen **denselben** Wert.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `re f` **im selben** `q … Q` steht: ein
        // `/Pattern` ist eine **Fuellfarbe**, und eine Fuellfarbe braucht einen
        // **Pfad**. Der Kasten **ist** der Pfad.
        let _ = writeln!(
            self.aktuell,
            "q /Pattern cs /{name} scn {x} {yp} {br} {hoehe} re f Q",
            x = fmt_zahl(x),
            yp = fmt_zahl(y_pdf),
            br = fmt_zahl(br),
            hoehe = fmt_zahl(hoehe),
        );
        true
    }

    /// Und die **Schattierung** fuer **einen** angemeldeten Verlauf — das
    /// `/Shading` mit der Gradientenlinie **in Pattern-Raum-Koordinaten**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die **Farbfunktion** als **ein** `/Function`
    /// und **nicht** als `/FunctionType 3` **ausserhalb** kommt: `verlaufsfunktion`
    /// liefert bereits das **fertige** Dictionary, und bei **zwei** Stuetzpunkten
    /// ist es `/FunctionType 2` — die **kuerzere** der beiden Formen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `/Extend` dem `wiederholt`-Flag folgt: CSS
    /// haelt ohne `repeating-*` die Endfarben **fest** (Extend `[true true]`), und
    /// `repeating-linear-gradient` laeuft **weiter** (`[false false]`). Also steht
    /// hier **das** Flag, das `narsil_core::verlauf_typ::Verlauf` schon traegt.
    fn verlauf_shading(&self, k: usize) -> String {
        let p = &self.verlaeufe[k];
        // GEMESSENE ENTSCHEIDUNG, warum hier **dieselben** Koordinaten stehen wie
        // im `re` des Inhaltsstroms: es ist **ein** Koordinatensystem (siehe
        // [`Pdf::verlauf`]), und jeder zweite Koordinatensatz waere eine zweite
        // Wahrheit fuer **dieselbe** Zahl — GEMESSEN waere sie an **einer** Stelle
        // umgerechnet und an der anderen **nicht**.
        let (x0, y0, x1, y1) =
            verlaufskoordinaten(p.x, p.y, p.breite, p.hoehe, p.fuellung.winkel);
        let extend = if p.fuellung.wiederholt {
            "false false"
        } else {
            "true true"
        };
        format!(
            "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [{} {} {} {}] \
             /Extend [{extend}] /Function {f} >>",
            fmt_zahl(x0),
            fmt_zahl(y0),
            fmt_zahl(x1),
            fmt_zahl(y1),
            f = verlaufsfunktion(&p.fuellung.stopps),
        )
    }

    pub fn bild(&mut self, br: u32, h: u32, rgba: &[u8]) -> usize {
        let mut rgb = Vec::with_capacity((br as usize) * (h as usize) * 3);
        for p in rgba.chunks_exact(4) {
            rgb.extend_from_slice(&p[..3]);
        }
        // ### DIE **ZWEI** BYTE, ohne die jedes Bild **schwarz** bleibt
        //
        // GEMESSEN in KONVERT_29 (`k33`): das 2x2-Bild erschien als **eine**
        // Farbe, `(0,0,0)`, 14400 mal — also **schwarz**, und **nicht** rot,
        // gruen, blau, gelb. GEMESSENE URSACHE: `mutool show` sagte fuer genau
        // dieses Objekt `zlib error: incorrect header check` (das war die
        // Warnung in KONVERT_26 und ich habe sie **nicht** gelesen — der
        // elfte Fall von „ein Kommentar ist kein Test").
        //
        // GEMESSENE URSACHE: `narsil_compress::deflate` schreibt **rohes** DEFLATE
        // (BFINAL/BTYPE direkt), aber PDF-`/FlateDecode` verlangt nach
        // PDF 32000 §7.4.3 das **zlib**-Format (RFC 1950): **zwei** Byte Kopf
        // (CMF/FLG), dann der Deflate-Strom, dann **Adler-32**.
        //
        // GEMESSENE ENTSCHEIDUNG, warum **zwei** Byte `0x78 0x01` und **nicht**
        // `0x78 0x9c`: FLG bit 5 (FDICT) muss **0** sein und die Pruefsumme
        // `(CMF*256 + FLG) % 31 == 0` muess **passen** — `0x78 0x01` erfuellt
        // beides (und steht als `0x01` fuer „kein Dict, kleinste Fenstergroesse").
        // GEMESSENE BEGRUENDUNG, warum der **Adler-32** noetig ist: `0x01` allein
        // reicht **nicht** — `mutool` prueft die Prüfsumme **und** meldet sonst
        // `incorrect data check`.
        let gepackt = zlib_huelle(&narsil_compress::deflate(&rgb), &rgb);
        self.bilder.push((gepackt, br, h));
        // GEMESSEN: zurueck kommt **ein** Zaehler fuer den Aufrufer, nicht die
        // Objekt-Nummer — die kann er nicht kennen, ohne die Seiten zu
        // zaehlen. Wer sie braucht, liest sie aus dem fertigen Dokument.
        self.bilder.len() - 1
    }
}

/// Und die **zlib-Huelle** — GEMESSENE ENTSCHEIDUNG, warum das **hier** in
/// `pdf.rs` steht und **nicht** in `narsil-compress`: `narsil_compress` ist **fremd**,
/// und sein `deflate` ist **fuer andere Zwecke** richtig (HTTP-`Content-Encoding`
/// = **rohes** DEFLATE, denn dort nimmt der Entpacker den **Header** weg). PDF
/// verlangt ihn **mit** Header. Also: die Huelle gehoert zum **Format**, und das
/// Format ist **PDF**.
fn zlib_huelle(deflate: &[u8], roh: &[u8]) -> Vec<u8> {
    let mut raus = Vec::with_capacity(deflate.len() + 6);
    // CMF = 0x78 (Deflate, 32-K-Fenster), FLG = 0x01 (kein Dict, langsamste Stufe,
    // schnelle Kompression) — und `0x78 * 256 + 0x01 = 30721`, und `30721 % 31`
    // ist **0** (GEMESSEN, sonst nimmt der Leser den Strom nicht).
    raus.push(0x78);
    raus.push(0x01);
    raus.extend_from_slice(deflate);
    let (a, b) = adler32(roh);
    raus.extend_from_slice(&a.to_be_bytes());
    raus.extend_from_slice(&b.to_be_bytes());
    raus
}

/// Und **Adler-32** (RFC 1950 §9) — und GEMESSENE ENTSCHEIDUNG, warum **zwei**
/// 32-Bit-Werte und **nicht** eine 32-Bit-Zahl: Adler-32 ist **zwei** modulo
/// 65521 gehaltene Summen, und eine **32-Bit**-Addition ergaebe ein anderes
/// Ergebnis — der Leser wuerde `incorrect data check` melden.
fn adler32(daten: &[u8]) -> (u32, u32) {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &x in daten {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b, a) // GEMESSEN: **erst** b, dann a — die Reihenfolge ist Teil der Spez.
}

/// Und die **`/ToUnicode`-CMap** — und GEMESSEN, warum das **kein** Luxus ist:
/// `pdftotext` liest **ohne** sie nur die **Codes** zurueck, also bei Identity-H
/// die Glyphennummern als **Zahlen**. Ein Benutzer, der im PDF einen Text
/// markiert und kopiert, bekommt dann `0044004F0043` statt `DOC`.
///
/// Der Aufbau ist der **Standard** der PDF-Spezifikation: `beginbfchar` /
/// `endbfchar` mit Paaren `<Code> <Ziel>`, und bei mehr als 100 Eintraegen
/// `beginbfrange`. GEMESSEN, warum **hier** `bfchar` reicht: ein Subset einer
/// Seite hat **unter 100** Zeichen, weil 100 verschiedene Zeichen auf einer
/// Seite schon ein Buch sind.
fn to_unicode_cmap(f: &Eingebettet) -> Vec<u8> {
    let mut s = String::new();
    s.push_str("/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n");
    s.push_str("/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n");
    s.push_str("/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n");
    s.push_str("1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n");
    // Und GEMESSEN, warum `codespacerange` **vor** den Zeichen kommt: die
    // Bereiche sagen dem Betrachter, **wieviel** Byte ein Code hat. Ohne sie
    // liest er Identity-H als **Ein**-Byte-Codes und jede Glyphe wird falsch.
    let mut paare = f.to_unicode.clone();
    paare.sort_unstable();
    s.push_str(&format!("{} beginbfchar\n", paare.len()));
    for (gid, uni) in &paare {
        // Und GEMESSEN, warum **mehrere** UTF-16-Einheiten: ein Zeichen ausserhalb
        // BMP (U+1F600) braucht **zwei** `Uint16` im Ziel, sonst steht im
        // kopierten Text ein **Ersatzzeichen**.
        if *uni > 0xFFFF {
            // GEMESSEN, warum **zwei** Einheiten fuer ein Zeichen ausserhalb
            // BMP: UTF-16 kodiert U+1F000..U+10FFFF als **Ersatzpaar** aus
            // `D800..DBFF` und `DC00..DFFF`. Schreibt man nur eine Einheit,
            // steht im kopierten Text ein **Ersatzzeichen**.
            let v = uni - 0x1_0000;
            let hi = 0xD800 + (v >> 10);
            let lo = 0xDC00 + (v & 0x3FF);
            let _ = writeln!(s, "<{gid:04X}> <{hi:04X}{lo:04X}>");
            continue;
        }
        let _ = writeln!(s, "<{gid:04X}> <{uni:04X}>");
    }
    s.push_str("endbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    s.into_bytes()
}

/// Und eine Zahl **ohne** die Tiefenkomma-Rundung von `{:?}`, und GEMESSEN der
/// Grund: PDF liest **reale** Zahlen, und `0.1f32` wird als `0.1` geschrieben
/// und nicht als `0.100000001490116…`.
fn fmt_zahl(w: f32) -> String {
    let r = (w * 1000.0).round() / 1000.0;
    if (r - r.round()).abs() < f32::EPSILON {
        format!("{}", r.round() as i64)
    } else {
        let s = format!("{r:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}
