//! Hebraeischer Text: Punktierung, Lage der Marken und kanonische Ordnung.
//!
//! Hebraeisch ist keine verbindende Schrift; die eigentliche Arbeit fuer
//! eine korrekte Anzeige sind die Kombinationszeichen (Punktierung).
//! Dieses Modul liefert dafuer drei Bausteine:
//!
//! * [`ist_hebraeisch`] und [`ist_hebraeische_marke`],
//! * [`marken_lage`] (oben/unten/mittig) fuer die Positionierung,
//! * [`kanonisch_ordnen`] (UAX #15): innerhalb einer Folge aus
//!   Grundzeichen und Marken werden die Marken stabil nach ihrer
//!   Canonical_Combining_Class sortiert.
//!
//! Quellen (Unicode 15.1.0, /usr/share/unicode):
//! `UnicodeData.txt` (Namen, CCC), `Blocks.txt` (Blockgrenzen). Die Lage
//! der Vokalzeichen stammt aus dem Unicode-Standard, Kapitel 9.1
//! (Tabelle 9-2); die UCD haben dafuer keine maschinenlesbare Datei —
//! die kleine Tabelle steht deshalb im Generator (siehe Modulkopf von
//! `unicode_daten/hebraeisch_marken.rs`).
//!
//! Die Neuordnung des Bidi-Algorithmus (L2/L3) macht `crate::bidi`; hier
//! geht es um die Zeichenfolge innerhalb eines Laufs.

pub use crate::unicode_daten::ccc::ccc;
pub use crate::unicode_daten::hebraeisch_marken::{Markenlage, ist_hebraeische_marke, marken_lage};

/// Ist das Zeichen hebraeisch?
///
/// Block U+0590..=U+05FF (Hebraisch) und U+FB1D..=U+FB4F (hebraeische
/// Praesentationsformen in den Alphabetic Presentation Forms); Quellen:
/// `Blocks.txt`.
pub fn ist_hebraeisch(ch: char) -> bool {
    ('\u{0590}'..='\u{05FF}').contains(&ch) || ('\u{FB1D}'..='\u{FB4F}').contains(&ch)
}

/// Kanonische Ordnung nach UAX #15 (stabile Sortierung nach CCC).
///
/// Zerlegt den Text in Bloecke aus einem Grundzeichen und den direkt
/// folgenden Kombinationszeichen (CCC > 0) und sortiert die Marken
/// stabil nach ihrer CCC. Gleiche CCC behalten ihre Reihenfolge; alles
/// andere bleibt unveraendert. Fuer hebraeische Punktierung heisst das
/// z. B. Qamats (CCC 18) vor Schin-Punkt (CCC 24).
pub fn kanonisch_ordnen(text: &str) -> String {
    let zeichen: Vec<char> = text.chars().collect();
    let mut aus = String::with_capacity(text.len());
    let mut i = 0;
    while i < zeichen.len() {
        let start = i;
        i += 1;
        while i < zeichen.len() && ccc(zeichen[i]) != 0 {
            i += 1;
        }
        if i - start == 1 {
            aus.push(zeichen[start]);
            continue;
        }
        let mut block: Vec<char> = zeichen[start..i].to_vec();
        if ccc(block[0]) != 0 {
            // Der Block beginnt mit einer Marke (z. B. zerschrittener
            // Text): alle Marken gemeinsam sortieren.
            block.sort_by_key(|&c| ccc(c));
        } else {
            let mut marken = block.split_off(1);
            marken.sort_by_key(|&c| ccc(c));
            block.extend(marken);
        }
        for c in block {
            aus.push(c);
        }
    }
    aus
}
