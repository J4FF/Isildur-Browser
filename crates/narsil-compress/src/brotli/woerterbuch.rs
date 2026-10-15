//! Das statische Woerterbuch von Brotli (RFC 7932 Abschnitt 8).
//!
//! added 30.09.2026 bei der Uebernahme des Brotli-Auftrags von der
//! Rolle KOMPRESS.
//!
//! DIE DREI AUFGABEN DIESER DATEI:
//!
//! 1. Die 122.784 Byte DICT aus Anhang A bereitstellen.
//! 2. Daraus ein Wort nach `(Laenge, Index)` herausschneiden.
//! 3. Die 121 Worttransformationen anwenden.
//!
//! ZU 1 UND 2, und das ist der nicht offensichtliche Teil: die
//! Wortgrenzen stehen **nirgends** als Offset-Tabelle. Der RFC gibt
//! stattdessen eine Rekursion an (Abschnitt 8):
//!
//! ```text
//! DOFFSET[0] = 0
//! DOFFSET[length + 1] = DOFFSET[length] + length * NWORDS[length]
//! DICTSIZE = DOFFSET[24] + 24 * NWORDS[24]
//! offset(length, index) = DOFFSET[length] + index * length
//! ```
//!
//! mit `NWORDS[length] = 1 << NDBITS[length]`. Die Rechnung ergibt
//! 122.784 — dieselbe Zahl, die der RFC als Laenge des DICT nennt.
//! Das ist die Gegenprobe: eine abgedruckte Offset-Tabelle koennte
//! Tippfehler enthalten, die Rekursion nicht.
//!
//! ZU 3, und die Transformationen sind der Grund, warum Brotli auf
//! Text besser ist als gzip: aus einem Wort macht der Kompressor
//! " the ", " of ", "s " und aehnliches. Die Liste steht in Anhang B.

use crate::FehlerZlib;
use crate::brotli::dict::{DICTSIZE, DOFFSET, NDBITS};
use crate::brotli::transforms::TRANSFORMS;

pub use crate::brotli::dict::DICT_HEX;

/// M3: die groesste Woerterbuch-Laenge.
///
/// DER WERT 24: "The copy length for a static dictionary reference
/// must be between 4 and 24" (Abschnitt 8). Steht im RFC.
pub const MAX_WORT_LAENGE: usize = 24;

/// M3: die kleinste Woerterbuch-Laenge.
///
/// DER WERT 4: dieselbe Stelle im RFC.
pub const MIN_WORT_LAENGE: usize = 4;

/// Die DICT-Bytes, einmal dekodiert.
///
/// DER WEG: `DICT_HEX` ist ein `&str` aus `dict.hex`, weil eine
/// 122.784-Byte-`const` in einer Rust-Datei jede Anzeige sprengt und
/// ein Buildskript verboten ist (kein Fremd-Crate, und der Generator
/// laeuft nicht beim Bauen). Der Preis ist eine Dekodierung beim
/// ersten Zugriff.
///
/// DIE PRUEFUNG DER LAENGE ist noetig und nicht Formsache: die Datei
/// wird per `include_str!` eingebunden, und ein gekuerzter
/// Wertobjekt-Import schlaegt nicht beim Bauen fehl, sondern erst
/// beim Entpacken — dann aber mit einer falschen Seite und keinem
/// Hinweis, warum.
fn dict_bytes() -> Result<&'static [u8], FehlerZlib> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<Vec<u8>> = OnceLock::new();
    let roh = CACHE.get_or_init(|| {
        let ohne_ws: String = DICT_HEX
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .collect();
        // M3: `ohne_ws` hat doppelte Laenge, wenn man sie nicht
        // kennt. Der Sollwert steht im RFC.
        let mut v = Vec::with_capacity(ohne_ws.len() / 2);
        let b = ohne_ws.as_bytes();
        let mut i = 0;
        while i + 1 < b.len() {
            let hi = (b[i] as char).to_digit(16);
            let lo = (b[i + 1] as char).to_digit(16);
            match (hi, lo) {
                (Some(h), Some(l)) => v.push((h * 16 + l) as u8),
                _ => break,
            }
            i += 2;
        }
        v
    });
    if roh.len() as u32 != DICTSIZE {
        return Err(FehlerZlib::BrotliDekodierFehler {
            grund: format!(
                "das statische Woerterbuch hat {} Byte, der RFC nennt {DICTSIZE}",
                roh.len()
            ),
        });
    }
    Ok(roh)
}

/// Die Anzahl der Woerter einer Laenge.
///
/// DER WERT: `1 << NDBITS[laenge]`, fuer Laengen 4 bis 24. Fuer
/// Laengen unter 4 gibt es keine Woerter ("NWORDS[length] = 0 (if
/// length < 4)", Abschnitt 8).
pub fn anzahl_woerter(laenge: usize) -> usize {
    if !(MIN_WORT_LAENGE..=MAX_WORT_LAENGE).contains(&laenge) {
        return 0;
    }
    1usize << NDBITS[laenge]
}

/// Das Wort mit Nummer `index` und Laenge `laenge`.
///
/// DER INDEX, und er ist nicht der, den man zuerst vermutet:
/// `word_id = distance - (maximale Distanz + 1)`, dann
/// `index = word_id % NWORDS[length]` und
/// `transform_id = word_id >> NDBITS[length]` (Abschnitt 8). Der
/// Transform steht also in den **hohen** Bits und der Index in den
/// niedrigen — wer es umdreht, holt bei jedem zweiten Wort das
/// falsche und traegt eine Transformation auf, die nicht dazugehoert.
pub fn hole(word_id: usize) -> Option<&'static [u8]> {
    // M3: die Laenge ist noch nicht bekannt, wenn nur `word_id`
    // vorliegt. Der Aufrufer muss sie kennen, weil sie in
    // `word_id % NWORDS[length]` steckt.
    None.or_else(|| hole_mit_laenge(word_id, None))
}

/// Wie `hole`, aber mit der Ziel-Laenge, wenn sie bekannt ist.
///
/// DER GRUND FUER DIESE ZWEITE FORM: ohne die Laenge laesst sich der
/// Index nicht berechnen, weil `NWORDS[length]` von der Laenge
/// abhaengt. Der Aufrufer kennt sie (aus dem Insert-und-Kopieren-
/// Symbol), also bekommt er sie hier.
pub fn hole_mit_laenge(word_id: usize, laenge: Option<usize>) -> Option<&'static [u8]> {
    let laenge = laenge?;
    // DER BEREICH, woertlich aus Abschnitt 8: "The copy length for a
    // static dictionary reference must be between 4 and 24".
    if !(MIN_WORT_LAENGE..=MAX_WORT_LAENGE).contains(&laenge) {
        return None;
    }
    let nwords = anzahl_woerter(laenge);
    if nwords == 0 {
        return None;
    }
    // M3: `word_id` kommt aus dem Strom. Ohne diese Pruefung waere
    // `word_id >> NDBITS[laenge]` ein beliebig grosses Wort, und die
    // Wordszahl ist klein — der Index muesste begrenzt werden.
    if word_id >> NDBITS[laenge] >= TRANSFORMS.len() {
        return None;
    }
    let index = word_id & (nwords - 1);
    // M4: `DOFFSET[laenge] + index * laenge` muss im DICT liegen.
    // Die Pruefung faellt bei einem Strom, der ein Wort verlangt,
    // das es nicht gibt.
    let start = DOFFSET[laenge] as usize + index * laenge;
    let end = start + laenge;
    let d = dict_bytes().ok()?;
    if end > d.len() {
        return None;
    }
    Some(&d[start..end])
}

/// Die Nummer des Transform, der zu `word_id` gehoert.
///
/// DER INDEX: `transform_id = word_id >> NDBITS[length]` (Abschnitt 8).
/// Der Wert 0 bedeutet "keine Transformation".
pub fn transform_id(word_id: usize, laenge: usize) -> usize {
    if !(MIN_WORT_LAENGE..=MAX_WORT_LAENGE).contains(&laenge) {
        return 0;
    }
    word_id >> NDBITS[laenge]
}

/// Wendet eine Worttransformation an (Anhang B).
///
/// DIE TRANSFORMATIONEN, und sie sind nicht alle einfach:
///
///  * Identity: das Wort unveraendert.
///  * FermentFirst: das erste Zeichen gross schreiben.
///  * FermentAll: **alle** Zeichen gross.
///  * OmitFirst1..9: die ersten 1 bis 9 Zeichen streichen.
///  * OmitLast1..9: die letzten 1 bis 9 Zeichen streichen.
///
/// Dazu kommt bei jeder Transformation ein Praefix und ein Suffix, die
/// an das (gekuerzte) Wort gehaengt werden — **alle** 121
/// Transformationen haben beides, auch die mit leerem Praefix und
/// leerem Suffix.
///
/// DER FALL, DER HIER AUFTRETEN KANN: OmitFirst9 auf einem Wort der
/// Laenge 4 gibt ein leeres Wort. Das ist erlaubt, und der Suffix
/// bleibt. Ein Wort, das laenger streift als es lang ist, ist also
/// kein Fehler.
pub fn transformiere(wort: &[u8], transform: usize) -> Vec<u8> {
    let t = match TRANSFORMS.get(transform) {
        Some(t) => *t,
        // M3: der Transformindex kam aus dem Strom. Ohne diese
        // Rueckgabe wuerde `TRANSFORMS[transform]` paniken.
        None => return wort.to_vec(),
    };
    let (praefix, art, suffix) = t;
    let mut kern: Vec<u8> = Vec::with_capacity(wort.len() + praefix.len() + suffix.len());
    match art {
        0 => kern.extend_from_slice(wort),
        1 => {
            // FermentFirst: nur das erste Zeichen.
            for (i, &b) in wort.iter().enumerate() {
                if i == 0 {
                    kern.push(b.to_ascii_uppercase());
                } else {
                    kern.push(b);
                }
            }
        }
        2 => {
            // FermentAll: alle Zeichen.
            for &b in wort {
                kern.push(b.to_ascii_uppercase());
            }
        }
        // 3 bis 11: OmitFirst1 bis OmitFirst9.
        3..=11 => {
            let streichen = (art - 2) as usize;
            for &b in wort.iter().skip(streichen) {
                kern.push(b);
            }
        }
        // 12 bis 20: OmitLast1 bis OmitLast9.
        12..=20 => {
            let streichen = (art - 11) as usize;
            let behalten = wort.len().saturating_sub(streichen);
            kern.extend_from_slice(&wort[..behalten]);
        }
        // M3: ein unbekannter Art-Wert. Die Tabelle hat 0 bis 20;
        // alles darueber kann nicht vorkommen, und ein Panic waere
        // hier der falsche Ort.
        _ => kern.extend_from_slice(wort),
    }
    let mut raus = Vec::with_capacity(praefix.len() + kern.len() + suffix.len());
    raus.extend_from_slice(praefix);
    raus.extend_from_slice(&kern);
    raus.extend_from_slice(suffix);
    raus
}
