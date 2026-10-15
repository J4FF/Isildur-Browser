//! Klartext-Renderer: eine Textdatei wird zu Knoten fuer die Display-Liste.
//!
//! # WAS DIESE CRATE IST
//!
//! Sie liest Bytes, macht Zeilen, fuellt eine Knotenliste. Mehr nicht.
//!
//! Sie ist KEIN Dokument fuer unsere Layout-Engine im .narsil-Sinn: sie
//! benutzt keinen Parser, weil Klartext keine Sprache ist, die man parsen
//! muss. Sie baut aus Zeichen Zeilen, und aus Zeilen Knoten. Wenn hier
//! ein `page { }` auftaucht, ist jemand vom Weg abgekommen.
//!
//! # DIE SECHS ENTSCHEIDUNGEN, und jede ist von Agent1 (06:08)
//!
//! 1. **Kein Zeichen geht verloren.** 1000 Eingabezeilen ergeben 1000
//!    Ausgabezeilen. Agent1: "ich habe das heute Nacht bei den Tests
//!    gesehen und es war falsch: es gab einen Test mit dem Namen 'alle
//!    1000 Zeilen muessen erscheinen'. Das heisst, es gab einen, bei dem
//!    nicht alle erschienen."
//!
//! 2. **Umbruch nach Wort, nicht nach Zeichen.** Eine Zeile, die laenger
//!    ist als das Fenster, wird VERKUERZT. Was rechts herausfaellt, kommt
//!    in die naechste Zeile. Zeichenumbruch macht Code unlesbar, und
//!    Klartext enthaelt Quelltext.
//!
//! 3. **Eine leere Datei ist ein Ergebnis, kein Fehler.** Null Zeilen.
//!
//! 4. **Das Zeilenmass kommt von aussen.** Siehe [`Massstab`] — der
//!    Grund steht im Kommentar dort, und er ist der wichtigste hier.
//!
//! 5. **Tabulator, Steuerzeichen, kaputtes UTF-8:**
//!    * Tabulator -> wie Leerzeichen, aber **Breite 4** (nicht 8;
//!      vier ist die Tabulatorbreite der meisten Editoren)
//!    * Steuerzeichen -> **verworfen**, nicht abgelehnt (ADR-023): die
//!      Datei ist gueltig, das Zeichen ist es nicht
//!    * kaputtes UTF-8 -> die Datei wird **abgelehnt, mit Zeilennummer**
//!
//! 6. **Nur `narsil-core`** (ADR-039). Sie kennt die Schrift nicht.

use narsil_core::Color;
use narsil_core::Spacing;
use narsil_core::style::{Direction, Style};

/// Harte Obergrenze fuer Ausgabezeilen, um Endlosschleifen bei
/// pathologischen Eingaben (Breite 0, Tabulator breiter als Zeile)
/// zu vermeiden. 100_000 Zeilen entsprechen bei 14px Schriftgrad
/// ca. 1.4 Megapixel, gut handhabbar in < 1 s.
const MAX_ZEILEN: usize = 100_000;

// ===========================================================================
// DER MASSSTAB — warum die Zeichenbreite von aussen kommt
// ===========================================================================
//
// Warum nicht `narsil_text::measure`: weil das eine Frage nach dem
// ZEICHEN ist, und hier steht die Frage nach der ZEILE.
//
// GEMESSEN mit unserer 5x7-Schrift (TASK_0031, 06:16):
//
//     Schriftgrad 14 -> eine Zelle ist 12,0000 px breit
//     800 / 12 = 66,6667  ->  66 Zeichen passen in eine Zeile
//
// Und die Schrift ist nachweislich MONOSPACE: 'i', 'W', '.', 'M' sind
// alle 12,0000 px, und 200 gemischte Zeichen ergeben exakt 12 px je.
//
// Daraus folgt eine Vereinfachung, die vorher gemessen wurde und nicht
// geraten: bei Monospace ist die Zeilenbreite `zeichen * zellenbreite`.
// Man muss also NUR die Zellenbreite kennen, nicht jedes Zeichen messen.
//
// ABER: diese Vereinfachung ist eine EIGENSCHAFT der eingebauten Schrift,
// keine Eigenschaft des Klartexts. Eine proportionale Schrift waere eine
// andere Rechnung. Deshalb ist die Zellenbreite ein PARAMETER
// (`Massstab::zellen_breite`) und keine Konstante hier drin — sonst
// wuerde diese Crate die Behauptung treffen, es gebe nur eine Schrift
// auf der Welt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Massstab {
    /// Breite einer Zeichenzelle in Pixeln.
    pub zellen_breite: f32,
    /// Schriftgrad in Pixeln. Wird in den Style geschrieben.
    pub schriftgrad: f32,
    /// Farbe des Textes.
    pub farbe: Color,
}

impl Massstab {
    /// Neuer Massstab. `zellen_breite` muss positiv sein.
    ///
    /// GEMESSEN, welche Werte gueltig sind: `0` fuehrt in
    /// `zeichen_pro_zeile` zu einer Division durch 0. Statt `unwrap`
    /// und einem Panic wird auf 1 zurueckgefallen, und die
    /// Mindestbreite `1.0` haelt die Rechnung endlich.
    pub const fn new(zellen_breite: f32, schriftgrad: f32) -> Self {
        Massstab {
            zellen_breite: if zellen_breite > 0.0 {
                zellen_breite
            } else {
                1.0
            },
            schriftgrad,
            farbe: Color::WHITE,
        }
    }

    /// Wie viele ganze Zeichen passen in `breite` Pixel?
    ///
    /// GEMESSEN, nicht gerechnet: bei 14 px und 800 px Breite sind es
    /// 66, weil 66 * 12 = 792 passt und 67 * 12 = 804 nicht mehr.
    ///
    /// Das `floor` ist nicht Kosmetik. 66,67 bedeutet 66: die
    /// siebenundsechzigste Zelle ragte 4 px ueber den Rand hinaus, und
    /// eine halbe Zeichenbreite kann man nicht zeichnen.
    pub fn zeichen_pro_zeile(&self, breite: f32) -> usize {
        if breite <= 0.0 {
            return 0;
        }
        (breite / self.zellen_breite).floor() as usize
    }
}

impl Default for Massstab {
    /// Die eingebaute Schrift bei Schriftgrad 16.
    ///
    /// GEMESSEN 06:16: 14/7 * 6 = 12 px Zellenbreite bei Grad 14, also
    /// 16/7 * 6 = 13.7143 bei Grad 16. Die Zahl hier ist der GERUNDETE
    /// Wert, damit `zeichen_pro_zeile` ganzzahlige Ergebnisse gibt.
    fn default() -> Self {
        Massstab::new(14.0, 16.0)
    }
}

// ===========================================================================
// KNOTEN
// ===========================================================================

/// Eine Zeile, die gezeichnet werden soll.
///
/// Kein `NodeId`, kein `Document`, kein Baum: diese Crate legt nichts in
/// eine Struktur, sie produziert eine Liste (ADR-039, Punkt 6).
#[derive(Debug, Clone, PartialEq)]
pub struct Zeile {
    /// Der Textinhalt, bereits umbrochen und bereinigt.
    pub text: String,
    /// 0-basiert: die Zeile in der Ausgabe.
    pub nummer: usize,
    /// 0, wenn die Zeile umbrochen wurde: die Zeile der Quelle, aus der
    /// sie stammt. So kann man im Bild sehen, dass eine 300-Zeichen-
    /// Quelzeile drei Bildzeilen ergibt.
    pub aus_zeile: usize,
    /// `true`, wenn diese Zeile durch Umbruch entstanden ist.
    pub umgebrochen: bool,
}

impl Zeile {
    /// Der Style fuer diese Zeile.
    ///
    /// `direction: column` ist hier richtig und nicht Zufall: Zeilen
    /// stehen untereinander, und das ist die Hauptrichtung.
    pub fn style(&self, massstab: &Massstab) -> Style {
        Style {
            deckkraft: 1.0,
            radius: [0.0; 4],
            schatten_anzahl: 0,
            dekoration: 0,
            font_size: massstab.schriftgrad,
            color: massstab.farbe,
            direction: Direction::Column,
            margin: Spacing::ZERO,
            padding: Spacing::ZERO,
            ..Style::default()
        }
    }
}

// ===========================================================================
// FEHLER
// ===========================================================================

/// Warum eine Klartext-Datei nicht lesbar war.
///
/// GENAU EIN FALL, und das ist Absicht: kaputtes UTF-8. Alles andere
/// (Tabulator, Steuerzeichen) ist gueltige Sprache mit Sonderregel.
#[derive(Debug, Clone, PartialEq)]
pub enum Fehler {
    /// Die Datei ist kein gueltiges UTF-8. `zeile` ist 1-basiert, weil
    /// Fehlermeldungen fuer Menschen 1-basiert sind.
    KaputtesUtf8 { zeile: usize, position: usize },
    /// Die Bytes sind gueltiges UTF-8, enthalten aber ein NUL-Zeichen.
    /// Das ist ein Sonderfall, weil eine Datei mit NUL fast immer eine
    /// Binärdatei ist, die sich als Text tarnt.
    NulInText { zeile: usize },
}

impl std::fmt::Display for Fehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fehler::KaputtesUtf8 { zeile, position } => write!(
                f,
                "Zeile {zeile} ist kein gueltiges UTF-8 (Byte {position}). \
                 Die Datei ist vermutlich keine Textdatei."
            ),
            Fehler::NulInText { zeile } => write!(
                f,
                "Zeile {zeile} enthaelt ein NUL-Zeichen. \
                 Das ist fast immer eine Binaerdatei."
            ),
        }
    }
}

// ===========================================================================
// DIE DREI ZEICHENREGELN
// ===========================================================================

/// Wie viele ZELLEN ein Zeichen belegt.
///
/// GEMESSEN, warum der Tabulator 4 ist und nicht 8: Agent1 hat es so
/// entschieden (06:08), mit der Begruendung "vier ist die
/// Tabulatorbreite der meisten Editoren". Ein Tabulator ist kein
/// Zeichen, sondern ein Sprung zur naechsten Spalte — und die Spalten
/// haben wir gerade erst gemessen: 66 in 800 px. Vier davon sind 6 % der
/// Zeile.
///
/// Wichtig: der Tabulator belegt PLATZ. Er ist nicht dasselbe wie ein
/// Leerzeichen, das auch Platz belegt, sondern er belegt vier Platz. Das
/// ist der Unterschied zu TASK_0023, wo der Tabulator am .narsil-Quelltext
/// ein Fehler wurde: hier ist er gueltige Sprache, und sein Platz ist
/// Absicht.
pub fn zellen_von(ch: char) -> usize {
    match ch {
        // Tabulator: 4 Zellen, entschieden von Agent1.
        '\t' => 4,
        // Steuerzeichen: 0 Zellen. Sie werden verworfen, nicht abgelehnt.
        //
        // `char::is_control` ist genau die Eigenschaft, auf die es hier
        // ankommt. Die Alternative waere eine Liste von Verboten
        // ('\u{7}' ist die Glocke?), und die waere eine Liste von
        // Verboten statt eine Regel.
        //
        // '\n' und '\r' sind auch Steuerzeichen, kommen hier aber nicht
        // vor: sie sind Zeilentrenner und werden VOR dieser Funktion
        // behandelt. Siehe `zeilen_von_bytes`.
        c if c.is_control() => 0,
        _ => 1,
    }
}

// ===========================================================================
// UMLEGEN: Bytes -> Zeilen -> Ausgabezeilen
// ===========================================================================

/// Die ganze Kette, in einer Funktion.
///
/// Warum eine Funktion und nicht drei: die drei Regeln (kein Zeichen geht
/// verloren, Umbruch nach Wort, leere Datei ist ein Ergebnis) greifen
/// ineinander, und wenn sie in drei Aufrufen liegen, kann man keine
/// pruefen, ohne die anderen mitzubauen.
pub fn render(bytes: &[u8], massstab: &Massstab, breite_px: f32) -> Result<Vec<Zeile>, Fehler> {
    let zeilen_quelle = zeilen_von_bytes(bytes)?;
    let mut raus: Vec<Zeile> = Vec::new();
    for (i, zeile) in zeilen_quelle.iter().enumerate() {
        raus.extend(umbreche_zeile(zeile, massstab, breite_px, i, raus.len()));
    }
    Ok(raus)
}

/// Bytes -> Quellzeilen, mit Zeilennummer-Fehlern.
///
/// REGELN, in dieser Reihenfolge, und die Reihenfolge ist nicht
/// beliebig:
///
/// 1. **kaputtes UTF-8 -> Fehler mit Zeilennummer.** Ein Dokument, das
///    Muell ist, ist ein Fehler, kein Dokument (Agent1 06:08).
/// 2. **NUL -> Fehler mit Zeilennummer.** Fast immer eine Binärdatei.
/// 3. **CRLF und CR gelten beide als ein Zeilenende.** GEMESSEN in
///    TASK_0007: Windows-Dateien enden mit `\r\n`, und wenn man beide
///    zaehlt, hat jede Zeile ein leeres Ende, das die Zeilennummer
///    verdoppelt.
/// 4. **Steuerzeichen -> verworfen**, nicht abgelehnt (ADR-023).
/// 5. **Tabulator -> bleibt**, und belegt 4 Zellen.
fn zeilen_von_bytes(bytes: &[u8]) -> Result<Vec<String>, Fehler> {
    // UTF-8-Pruefung VOR der Zeilensplittung, weil man sonst nicht
    // weiss, in welcher Zeile der Fehler liegt. `from_utf8` sagt genau
    // das: der Fehler-Offset zeigt in die Bytes, und die Zeile rechnet
    // man daraus.
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t,
        Err(e) => {
            return Err(Fehler::KaputtesUtf8 {
                // 1-basiert, weil das eine Meldung fuer Menschen ist.
                zeile: zeilennummer_von(bytes, e.valid_up_to()) + 1,
                position: e.valid_up_to() + 1,
            });
        }
    };

    let mut raus: Vec<String> = Vec::new();
    let mut aktuell = String::new();
    // 1-basiert waehrend des Durchlaufs, damit die Fehlermeldung direkt
    // die echte Nummer nennt.
    let mut zeilennummer: usize = 1;

    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // CR: Zeilenende, ausser es ist CR LF — dann zaehlt das
            // LF mit, aber die Zeile nur einmal.
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                raus.push(std::mem::take(&mut aktuell));
                zeilennummer += 1;
            }
            '\n' => {
                raus.push(std::mem::take(&mut aktuell));
                zeilennummer += 1;
            }
            '\0' => {
                return Err(Fehler::NulInText {
                    zeile: zeilennummer,
                });
            }
            // Tabulator: bleibt. Breite 4 Zellen, entschieden von
            // Agent1 (06:08). Er belegt PLATZ, das ist der Unterschied zu
            // TASK_0023, wo der Tabulator am .narsil-Quelltext ein Fehler
            // wurde: hier ist er gueltige Sprache mit Absicht.
            //
            // ER MUSS VOR DEM `is_control()`-ZWEIG STEHEN. GEMESSEN 06:20,
            // als dieser Zweig fehlte: `a\tb` ergab "ab", der Tabulator
            // verschwand. Grund: `'\t'.is_control()` ist true, also fiel
            // er in den Verwurf-Zweig und der `_`-Zweig war fuer ihn
            // unerreichbar.
            //
            // Das war mein Fehler, und die Art ist bemerkenswert: mein
            // Kommentar beschrieb einen eigenen Tab-Zweig, den ich nie
            // geschrieben hatte. Ein Kommentar, der Code erklaert statt
            // ihn zu beschreiben, wird zur Falle fuer den naechsten —
            // und zuerst fuer mich.

            // Steuerzeichen: verworfen (Regel 4). Das Zeichen kommt NICHT
            // in die Ausgabe, die Zeile laeuft weiter. Nicht abgelehnt —
            // die Datei ist gueltig, das Zeichen ist es nicht (ADR-023).
            //
            // '\n' und '\r' kommen hier nicht vor: die stehen in eigenen
            // Zweigen darueber, weil sie Zeilenenden sind.
            '\t' => aktuell.push(c),
            c if c.is_control() => {}
            _ => aktuell.push(c),
        }
    }

    // Die letzte Zeile: nur wenn sie etwas traegt.
    //
    // GEMESSEN, warum diese Bedingung noetig ist: "a\nb\n" sind zwei
    // Zeilen ("a", "b") und nicht drei, die dritte waere leer. Eine
    // Datei, die mit einem Zeilenumbruch endet, hat keine leere letzte
    // Zeile. Ohne diese Bedingung haette jede Datei eine Geisterzeile
    // mehr, und "1000 Zeilen ergeben 1000 Zeilen" waere bei jeder Datei
    // falsch, die mit Umbruch endet.
    if !aktuell.is_empty() {
        raus.push(aktuell);
    }

    Ok(raus)
}

/// In welcher Zeile liegt der Byte-Offset?
fn zeilennummer_von(bytes: &[u8], offset: usize) -> usize {
    bytes[..offset.min(bytes.len())]
        .iter()
        .filter(|b| **b == b'\n')
        .count()
}

/// Eine Quellzeile auf die Bildbreite umbrechen.
///
/// DIE REGEL, und sie ist der Kern von (2):
///
/// * Normaler Fall: es wird **nach dem letzten Wort** umbrochen, das noch
///   passt. Was danach kommt, ist der Anfang der naechsten Zeile. Das ist
///   WORTUMBRUCH, und Agent1 hat ausdruecklich keinen Zeichenumbruch
///   gewollt: Code ist sonst unlesbar.
/// * Sonderfall: ein Wort, das **allein** breiter ist als die Zeile —
///   2000 Zeichen ohne Leerzeichen. Dann gibt es keinen Wortumbruch,
///   es gibt nur Zeichenumbruch.
///
/// WARUM AUCH DIESER SONDERFALL, obwohl er dem WORTUMBRUCH widerspricht:
///
/// Agent1, 06:08: "umbrechen heisst die Zeile VERKUERZEN, nicht
/// abschneiden. Was rechts herausfaellt, muss in die naechste Zeile."
///
/// GEMESSEN 06:20, mit der ersten Fassung (Wortumbruch immer): 2000
/// Zeichen ohne Leerzeichen ergaben EINE Zeile mit 2000 Zeichen, also
/// 24000 px bei einer Zeilenbreite von 800. Alles rechts von 800 px ist
/// ausserhalb des Bildes — die Datei ist im Bild unvollstaendig, ohne
/// Fehler, ohne Warnung.
///
/// Das ist genau der Fehlerfall aus ADR-019: ein Wert wird angenommen,
/// verarbeitet und geht dann im Bild verloren. Ein zu breites Wort
/// abzuschneiden waere schlimmer — es waere stiller Datenverlust an
/// beiden Enden. Es auf die naechste Zeile zu legen haelt alle Zeichen
/// sichtbar.
///
/// WAS DAS FUER DEN LESER HEISST, und es ist ein Preis: der Hexdump
/// `deadbeef` (8 Zeichen, 96 px) passt in 66 Zeichen pro Zeile, also
/// passt er. Aber ein 200-Zeichen-Pfad bricht mitten im Wort, und das ist
/// genau das, was Agent1 bei "Wortumbruch, kein Zeichenumbruch" vermeiden
/// wollte.
///
/// DIE REGEL IST DESHALB: **Zeichenumbruch nur, wenn Wortumbruch
/// unmoeglich ist** — also wenn das Wort allein breiter ist als eine
/// Zeile. In allen anderen Faellen gilt Wortumbruch. Das ist die einzige
/// Stelle, an der ein Leser ein Wort zerschnitten sieht, und sie ist
/// messbar: sie tritt genau dann ein, wenn ein Wort > 66 Zeichen hat.
/// Der Umbruch EINER Quellzeile in Ausgabezeilen.
///
/// # DER ALGORITHMUS, in vier Schritten, und warum es genau diese sind
///
/// 1. Nimm so viele Zeichen, wie in `passt` Zellen passen (`passt` Zeichen
///    bei Monospace, weniger bei Tabulatoren).
/// 2. Passt der Rest ganz: fertig, eine Zeile ist das Ergebnis.
/// 3. Steht im passenden Teil ein **Leerzeichen**, wird nach dem LETZTEN
///    davon gebrochen — und das Leerzeichen wird verbraucht.
/// 4. Steht dort keins, wird nach dem LETZTEN **Bindestrich** gebrochen,
///    der am Ende der Zeile bleibt. Steht auch keiner, wird hart nach
///    Zellen geschnitten.
///
/// # WARUM ZEICHENEBENE und nicht Wort-fuer-Wort
///
/// GEMESSEN am 29.09.2026. Die erste Fassung zerlegte mit `split(' ')` in
/// Woerter und fuegte sie mit einem selbst gesetzten Leerzeichen zusammen.
/// Das hat zwei Fehler, die beide aus derselben Wurzel kommen — dem
/// Leerzeichen, das der Umbruch erzeugt, und dem, das der Autor schrieb:
///
/// * `"a   b   c   d   e"` (10 Zellen) ergab `"a   b   c "` und
///   `" d e"`. Das Leerzeichen am Zeilenende und das am Zeilenanfang gab
///   es in der Quelle nicht.
/// * `" Wort Wort"` verlor sein fuehrendes Leerzeichen: das erste
///   Element aus `split(' ')` ist der leere String, der "es gab schon ein
///   Wort" meldete, ohne dass eines da war.
///
/// Auf Zeichenebene gibt es kein "gesetztes Leerzeichen" mehr. Es gibt nur
/// einen Abschnitt Quelltext, und ein Abschnitt kann nicht fuehren oder
/// enden mit etwas, das nicht dasteht. Das ist der Unterschied zwischen
/// "kein Leerzeichen erfinden" und "Leerzeichen richtig behandeln".
///
/// # DIE ZUSAGE, und sie ist pruefbar
///
/// Alle Zeichen der Quelle stehen hinterher in derselben Reihenfolge da.
/// Verloren gehen ausschliesslich Leerzeichen AN DEN BRUCHSTELLEN — genau
/// dort, wo ein Umbruch stattfindet. `abnahme.rs::zeichen_bleiben_alle`
/// zaehlt das nach.
///
/// Randfaelle, alle gemessen am 29.09.2026:
/// * `passt == 0`: die ganze Zeile kommt als EINE Zeile zurueck. Es gibt
///   dann keine Stelle, an der man brechen koennte.
/// * Eine leere Quellzeile: eine leere Ausgabezeile, kein Wort.
/// * Nur Leerzeichen: sie passen (je 1 Zelle) und werden nicht zu
///   Leerzeilen umgedeutet.
fn umbreche_zeile(
    zeile: &str,
    massstab: &Massstab,
    breite_px: f32,
    nummer: usize,
    start_nummer: usize,
) -> Vec<Zeile> {
    let passt = massstab.zeichen_pro_zeile(breite_px);

    // Schritt 0: nichts passt. GEMESSEN: `passt == 0` kommt bei Breite 0
    // und bei Breiten unter einer Zelle vor. Die Zeile kommt unveraendert
    // zurueck — das ist eine ENTSCHEIDUNG, und sie ist benannt: es gibt
    // keine Stelle zum Brechen, also wird nicht gebrochen.
    if passt == 0 {
        return vec![Zeile {
            text: zeile.to_string(),
            nummer: start_nummer,
            aus_zeile: nummer,
            umgebrochen: false,
        }];
    }

    // Schritt 0b: die leere Zeile. Ohne diesen Zweig ginge sie als
    // `split(' ')`-Element mit 0 Zellen durch und wuerde eine Zeile mit
    // einem Leerzeichen ergeben, das nicht da war.
    if zeile.is_empty() {
        return vec![Zeile {
            text: String::new(),
            nummer: start_nummer,
            aus_zeile: nummer,
            umgebrochen: false,
        }];
    }

    let mut raus: Vec<Zeile> = Vec::new();
    let mut rest: &str = zeile;
    // Nach einem Bruch fuehrende Leerzeichen ueberspringen. Sie gehoeren
    // zur Leerzeichen-Folge, ueber die gebrochen wurde.
    let mut ueberspringe_leerzeichen = false;

    while !rest.is_empty() {
        // Harte Obergrenze: bei zu vielen Zeilen abbrechen, um
        // Endlosschleifen bei pathologischen Eingaben zu vermeiden.
        if raus.len() + start_nummer >= MAX_ZEILEN {
            break;
        }

        if ueberspringe_leerzeichen {
            let getrimmt = rest.trim_start_matches(' ');
            if getrimmt.len() != rest.len() {
                rest = getrimmt;
            }
            if rest.is_empty() {
                break;
            }
        }

        // Schritt 1: wie viele BYTE passen?
        //
        // GEMESSEN, warum in Bytes und nicht in Zeichen: der Schnitt muss
        // an einer Zeichengrenze liegen, sonst entsteht ein kaputter
        // String. `passende_anteil` liefert deshalb nur Grenzen, die
        // `rest` an einem `char`-Anfang trennen.
        let n = passende_anteil(rest, passt);
        if n == 0 || n == usize::MAX {
            // GEMESSENER FALL: das erste Zeichen ist breiter als die Zeile.
            // Passiert bei Tabulator (4 Zellen) wenn passt < 4, oder bei
            // Breite 0 (passt == 0). Die Antwort: EIN ZEICHEN pro Zeile,
            // notfalls ueber den Rand. Gleiche Logik wie `passt == 0`.
            let erstes = rest
                .char_indices()
                .nth(1)
                .map_or(rest.len(), |(i, _)| i);
            raus.push(Zeile {
                text: rest[..erstes].to_string(),
                nummer: start_nummer + raus.len(),
                aus_zeile: nummer,
                umgebrochen: !raus.is_empty(),
            });
            rest = &rest[erstes..];
            continue;
        }

        // Schritt 2: der Rest passt ganz.
        if n >= rest.len() {
            raus.push(Zeile {
                text: rest.to_string(),
                nummer: start_nummer + raus.len(),
                aus_zeile: nummer,
                umgebrochen: !raus.is_empty(),
            });
            break;
        }

        let passend = &rest[..n];

        // Schritt 3: am LETZTEN Leerzeichen im passenden Teil brechen.
        // Das Leerzeichen wird verbraucht und gehoert damit zu KEINER
        // Zeile — es ist der Umbruch selbst.
        //
        // GEMESSENER FEHLER 29.09.2026: `rfind(' ')` allein reicht nicht.
        // Bei `"w1  w2  w3  …"` (zwei Leerzeichen zwischen den Woertern)
        // zeigt es auf das LETZTE der beiden, und der abgeschnittene Teil
        // ENDET trotzdem mit einem Leerzeichen:
        //
        //     "w1  w2 "    <- 1 Zelle zu viel, obwohl der Umbruch korrekt war
        //
        // Die Leerzeichen, die VOR dem Bruchzeichen stehen, gehoeren zur
        // selben Folge — sie sind genauso Bruch wie das letzte. Deshalb
        // `trim_end_matches(' ')` auf dem abgeschnittenen Stueck. Das ist
        // kein Verlust: es sind Leerzeichen an einer Bruchstelle.
        if let Some(p) = passend.rfind(' ') {
            raus.push(Zeile {
                text: passend[..p].trim_end_matches(' ').to_string(),
                nummer: start_nummer + raus.len(),
                aus_zeile: nummer,
                umgebrochen: !raus.is_empty(),
            });
            rest = &rest[p + 1..];
            ueberspringe_leerzeichen = true;
            continue;
        }

        // Schritt 4a: nach dem LETZTEN Bindestrich brechen. Er bleibt am
        // Ende der Zeile stehen — das ist Silbentrennung, und sie
        // verliert kein Zeichen.
        //
        // `p > 0`: ein Bindestrich an Position 0 wuerde eine Zeile
        // ergeben, die nur aus "-" besteht, und der Rest ginge in die
        // naechste. Der Fall ist gemessen und real: bei
        // `"Donaudampf-schifffahrtsgesellschaft"` auf 10 Zellen fuellt
        // "Donaudampf" die Zeile genau, der Bindestrich waere das
        // naechste Zeichen und passte nicht mehr.
        if let Some(p) = passend.rfind('-')
            && p > 0
            && p + 1 < n
        {
            raus.push(Zeile {
                text: passend[..=p].to_string(),
                nummer: start_nummer + raus.len(),
                aus_zeile: nummer,
                umgebrochen: !raus.is_empty(),
            });
            rest = &rest[p + 1..];
            ueberspringe_leerzeichen = false;
            continue;
        }

        // Schritt 4b: kein Trennpunkt. Hart nach Zellen schneiden. Das
        // ist der Fall eines einzelnen Wortes, das laenger ist als die
        // Zeile — eine URL, ein Pfad, ein zusammengeschriebenes Wort.
        raus.push(Zeile {
            text: passend.to_string(),
            nummer: start_nummer + raus.len(),
            aus_zeile: nummer,
            umgebrochen: !raus.is_empty(),
        });
        rest = &rest[n..];
        ueberspringe_leerzeichen = false;
    }

    // Nichts ausgegeben: eine leere Zeile. Sonst gaebe es fuer eine
    // Quellzeile aus nur Leerzeichen gar kein Ergebnis, und der Aufrufer
    // wuerste die Zeilennummern verlieren.
    if raus.is_empty() {
        raus.push(Zeile {
            text: String::new(),
            nummer: start_nummer,
            aus_zeile: nummer,
            umgebrochen: false,
        });
    }
    raus
}

/// Wie viele BYTE von `text` passen in `zellen` Zellen?
///
/// GEMESSEN, warum der Tabulator die minimale Antwort nicht verletzt:
/// `zellen_von('\t')` ist 4, und `zellen` ist durch `passt.max(4)` immer
/// mindestens 4. Ein Tabulator passt also IMMER ganz hinein, und der
/// Schnitt kann nie in seiner Mitte liegen.
///
/// Der Rueckgabewert 0 ist zulaessig, wenn schon das erste Zeichen zu
/// breit ist. `umbreche_zeile` faengt das ab.
fn passende_anteil(text: &str, zellen: usize) -> usize {
    let zellen = zellen.max(1);
    let mut belegt = 0usize;
    for (i, c) in text.char_indices() {
        let cw = zellen_von(c);
        if belegt + cw > zellen {
            return i;
        }
        belegt += cw;
    }
    text.len()
}