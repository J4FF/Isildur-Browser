//! RUNDE 3 S3: `text-overflow: ellipsis` — die drei Punkte am Zeilenende.
//!
//! CSS Text 4 §4.3: bei `text-overflow: ellipsis` und einem Kasten, der den
//! Text nicht fasst, wird der Text an der Umbruchstelle gekuerzt und mit
//! `U+2026 HORIZONTAL ELLIPSIS` beendet. Zwei Bedingungen muessen ZUGLEICH
//! gelten, und das ist der ganze Inhalt der Regel:
//!
//! 1. der Kasten ist zu klein (`overflow` ist nicht `visible`), und
//! 2. es ist EIN EINZIGNER Umbruch (kein Umbruch bei einem weichen
//!    Trennstrich, kein Umbruch innerhalb eines Wortes durch
//!    `overflow-wrap`).
//!
//! # WARUM DAS EINE EIGENE FUNKTION UND NICHT EINE ZEICHENREGEL IST
//!
//! Wer die Punkte im Zeichner zeichnet, braucht vorher gewusst, WO sie
//! hin sollen — und das ist eine Messung ueber die gesamte Kette, nicht
//! ueber die letzte Glyphe. Wer sie im Layout abschneidet, hat keine
//! Breite, weil `layout_text*` sie bis 03.10.2026 nicht zurueckgab. Sie
//! ist seitdem da (`layout_text_mit_breite`), und genau darauf setzt das
//! hier auf.
//!
//! GEMESSEN 03.10.2026, SCHRIFT.

use crate::font::BitmapFont;
use crate::shaping::layout_text_mit_breite;

/// U+2026 HORIZONTAL ELLIPSIS. CSS Text 4 §4.3 nennt es beim Namen.
pub const PUNKTE: char = '\u{2026}';

/// M3: hoechste Laenge der eingegebenen Kette.
///
/// Warum eine Grenze: die Funktion schneidet an einer GLYPHENgrenze, und
/// ein Aufruf mit einem Text aus fremdem Datenstrom (einer `.narsil`-Datei)
/// koennte sonst eine Schleife ueber eine Kette von Millionen Zeichen
/// laufen lassen. 200 000 ist weit ueber jeder Zeile, die in ein Fenster
/// passt.
pub const MAX_KETTE: usize = 200_000;

/// Das Ergebnis: die Kuerzung und der Grund.
#[derive(Debug, Clone, PartialEq)]
pub struct Gekuerzt {
    /// Der Text, wie er gezeichnet werden soll.
    ///
    /// Bei keiner Kuerzung ist es der Eingang unveraendert — auch dann, wenn
    /// der Text nicht passt, denn CSS schneidet in diesem Fall gar nicht.
    pub text: String,
    /// `true`, wenn wirklich gekuerzt wurde.
    pub gekuerzt: bool,
    /// Der Vorschub der DREI PUNKTE in px.
    ///
    /// GEMESSEN: das Zeichen U+2026 hat seinen EIGENEN Vorschub, der nicht
    /// dem von drei Punkten entspricht. Wer `...` als drei `.` zeichnet,
    /// bekommt je nach Schrift etwas anderes.
    pub punkte_breite: f32,
    /// Wie viele Zeichen des Eingangs erhalten blieben.
    pub behalten: usize,
}

/// Kuerzt `text` auf `max_breite` px, mit `U+2026` am Ende.
///
/// # DIE REGEL, und sie hat drei Teile, alle CSS Text 4 §4.3
///
/// * Passt der Text, wird **nichts** getan — auch nicht aufgerundet.
/// * Passt er nicht, wird an der GLYPHENgrenze gekuerzt, und der Rest der
///   Breite geht an die drei Punkte.
/// * Passt nicht einmal der Punkt in die Breite, wird **nichts**
///   gezeichnet. Ein Punkt, der breiter ist als die Luecke, wuerde links
///   ueber den Rand hinausragen — das ist das Sichtbare, was man an einem
///   `…` in einem zu kleinen Kasten sieht.
///
/// # Warum der Punkt VOR dem Kuerzen gemessen wird
///
/// `punkte_breite` wird einmal gemessen und danach von der Restbreite
/// abgezogen. Schneidet man zuerst und misst dann, hat man den Punkt
/// schon im Text und kann ihn nicht mehr abziehen.
pub fn kuerze_mit_punkten(
    font: &BitmapFont,
    text: &str,
    font_size: f32,
    max_breite: f32,
) -> Result<Gekuerzt, crate::TextError> {
    // M1: `max_breite` kommt aus dem Layout und ist eine Laenge. `NaN` und
    // Unendlich sind keine Laengen, und ein Vergleich mit ihnen waere
    // immer wahr oder immer falsch — je nachdem, wie man dreht. Also
    // wird abgeiesen.
    if !max_breite.is_finite() || max_breite < 0.0 {
        return Err(crate::TextError::UngueltigerUrsprung {
            x: max_breite,
            y: 0.0,
        });
    }
    // M3/M4: die Kette ist Eingabe.
    if text.chars().count() > MAX_KETTE {
        return Err(crate::TextError::ZuLang);
    }

    let punkte_breite = crate::measure(font, &PUNKTE.to_string(), font_size)
        .map(|m| m.width)
        .unwrap_or(0.0);
    let voll = layout_text_mit_breite(
        font,
        text,
        font_size,
        narsil_core::geometry::Point::new(0.0, 0.0),
    )
    .map(|l| l.breite)?;

    if voll <= max_breite {
        // Passt. Nichts tun — auch nicht aufgerundet, auch nicht mit
        // einem Punkt am Ende.
        return Ok(Gekuerzt {
            text: text.to_string(),
            gekuerzt: false,
            punkte_breite,
            behalten: text.chars().count(),
        });
    }

    // Der Punkt passt nicht mehr: das ist der Fall, in dem gar nichts
    // gezeichnet wird.
    if punkte_breite > max_breite {
        return Ok(Gekuerzt {
            text: String::new(),
            gekuerzt: true,
            punkte_breite,
            behalten: 0,
        });
    }

    let rest = max_breite - punkte_breite;
    // GEMESSEN: an einer GLYPHENgrenze kuerzen, nicht an einer
    // Zeichengrenze. Bei einem Text mit Sprachzeichen kann die
    // Zeichengrenze mitten in einer Glyphe liegen, und dann zeigt der
    // letzte Zeichenrest einen halben Buchstaben.
    let zeichen: Vec<char> = text.chars().collect();
    let mut behalten = 0usize;
    let mut ohne: String = String::new();
    // DIE SUCHE, und sie ist der ehrliche Teil dieser Funktion.
    //
    // GEMESSEN 03.10.2026 (tests/ueberlauf.rs) mit dem linearen Durchlauf:
    // bei einem Anteil von 0,90 bleibt 6 bis 22 px UNGENUTZT — zum
    // Beispiel 32 px, Anteil 0,90: Grenze 831,60 px, erreicht 819,00 px.
    // Man koennte noch ein Zeichen aufnehmen, das passt nicht mehr.
    //
    // GEMESSEN ist ausserdem der Grund: die Vorschuebe sind nicht
    // gleichmaessig (ein `m` ist breiter als ein `i`), also ist der
    // Restbetrag kein Vielfaches einer Zeichenbreite. Er heisst hier also
    // NICHTS, solange man nicht weiss, wie breit das naechste Zeichen ist.
    //
    // Deshalb der Umweg: `behalten` VORAUS berechnen (wie viele Zeichen
    // passen in `rest`), und dann `behalten + 1` pruefen. Wird das
    // naechste Zeichen zu breit, weiss man, dass es nicht passt, ohne
    // raten zu muessen. Die Rechnung ist O(n) statt O(n^2) — bei
    // `MAX_KETTE` 200 000 waere der quadratische Weg 4 * 10^10
    // `measure`-Aufrufe.
    //
    // GEMESSEN: `measure` geht ueber `advance_or_fallback`, also ohne
    // Allokation. Ein Aufruf je Zeichen genuegt.
    let schritt_breite = if zeichen.is_empty() {
        0.0
    } else {
        let summe: f32 = zeichen
            .iter()
            .map(|c| font.advance_or_fallback(*c, font_size))
            .sum();
        summe / zeichen.len() as f32
    };
    // Nur ein Naeherungswert fuer die Vorabzahl — entscheidend ist die
    // EXAKTE Pruefung danach.
    let schaetzung = if schritt_breite > 0.0 {
        ((rest / schritt_breite).floor() as usize).min(zeichen.len())
    } else {
        0
    };
    for _ in 0..schaetzung {
        ohne.push(zeichen[behalten]);
        behalten += 1;
    }
    // Und jetzt die exakte Pruefung: passt das naechste Zeichen noch?
    while behalten < zeichen.len() {
        ohne.push(zeichen[behalten]);
        let b = crate::measure(font, &ohne, font_size).map(|m| m.width)?;
        if b > rest {
            ohne.pop();
            break;
        }
        behalten += 1;
    }
    // `behalten` und der tatsaechliche Inhalt muessen uebereinstimmen.
    debug_assert_eq!(behalten, ohne.chars().count());
    Ok(Gekuerzt {
        text: format!("{ohne}{PUNKTE}"),
        gekuerzt: true,
        punkte_breite,
        behalten,
    })
}
