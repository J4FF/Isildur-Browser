//! Hebraeische Kombinationszeichen und ihre Lage (oben/unten/mittig).
//!
//! ERZEUGT aus /usr/share/unicode/UnicodeData.txt (Unicode 15.1.0)
//! durch /tmp/opencode/gen_i4.py. Die Lage der Vokalzeichen stammt aus
//! dem Unicode-Standard, Kapitel 9.1, Tabelle 9-2; die UCD haben fuer
//! "ueber/unter der Grundlinie" keine maschinenlesbare Datei. Die
//! Kantillationsakzente U+0591..=U+05AF gelten als "Mitte"
//! (unspezifiziert).

/// Lage einer hebraeischen Marke relativ zur Grundlinie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Markenlage {
    /// Ueber der Grundlinie.
    Oben,
    /// Unter der Grundlinie.
    Unten,
    /// Auf der Grundlinie (z. B. PASEQ).
    Mitte,
    /// Keine hebraeische Marke.
    Keine,
}

/// (Zeichen, Lage), sortiert.
const MARKEN: &[(char, Markenlage)] = &[
    ('\u{0591}', Markenlage::Mitte),
    ('\u{0592}', Markenlage::Mitte),
    ('\u{0593}', Markenlage::Mitte),
    ('\u{0594}', Markenlage::Mitte),
    ('\u{0595}', Markenlage::Mitte),
    ('\u{0596}', Markenlage::Mitte),
    ('\u{0597}', Markenlage::Mitte),
    ('\u{0598}', Markenlage::Mitte),
    ('\u{0599}', Markenlage::Mitte),
    ('\u{059A}', Markenlage::Mitte),
    ('\u{059B}', Markenlage::Mitte),
    ('\u{059C}', Markenlage::Mitte),
    ('\u{059D}', Markenlage::Mitte),
    ('\u{059E}', Markenlage::Mitte),
    ('\u{059F}', Markenlage::Mitte),
    ('\u{05A0}', Markenlage::Mitte),
    ('\u{05A1}', Markenlage::Mitte),
    ('\u{05A2}', Markenlage::Mitte),
    ('\u{05A3}', Markenlage::Mitte),
    ('\u{05A4}', Markenlage::Mitte),
    ('\u{05A5}', Markenlage::Mitte),
    ('\u{05A6}', Markenlage::Mitte),
    ('\u{05A7}', Markenlage::Mitte),
    ('\u{05A8}', Markenlage::Mitte),
    ('\u{05A9}', Markenlage::Mitte),
    ('\u{05AA}', Markenlage::Mitte),
    ('\u{05AB}', Markenlage::Mitte),
    ('\u{05AC}', Markenlage::Mitte),
    ('\u{05AD}', Markenlage::Mitte),
    ('\u{05AE}', Markenlage::Mitte),
    ('\u{05AF}', Markenlage::Mitte),
    ('\u{05B0}', Markenlage::Unten),
    ('\u{05B1}', Markenlage::Unten),
    ('\u{05B2}', Markenlage::Unten),
    ('\u{05B3}', Markenlage::Unten),
    ('\u{05B4}', Markenlage::Unten),
    ('\u{05B5}', Markenlage::Unten),
    ('\u{05B6}', Markenlage::Unten),
    ('\u{05B7}', Markenlage::Unten),
    ('\u{05B8}', Markenlage::Unten),
    ('\u{05B9}', Markenlage::Oben),
    ('\u{05BA}', Markenlage::Oben),
    ('\u{05BB}', Markenlage::Unten),
    ('\u{05BC}', Markenlage::Mitte),
    ('\u{05BD}', Markenlage::Unten),
    ('\u{05BF}', Markenlage::Oben),
    ('\u{05C1}', Markenlage::Oben),
    ('\u{05C2}', Markenlage::Oben),
    ('\u{05C4}', Markenlage::Oben),
    ('\u{05C5}', Markenlage::Unten),
    ('\u{05C7}', Markenlage::Unten),
];

/// Liefert die Lage einer hebraeischen Marke.
pub fn marken_lage(ch: char) -> Markenlage {
    MARKEN
        .binary_search_by(|&(c, _)| c.cmp(&ch))
        .ok()
        .map(|i| MARKEN[i].1)
        .unwrap_or(Markenlage::Keine)
}

/// Ist das Zeichen eine hebraeische Marke (Punktierung)?
pub fn ist_hebraeische_marke(ch: char) -> bool {
    marken_lage(ch) != Markenlage::Keine
}
