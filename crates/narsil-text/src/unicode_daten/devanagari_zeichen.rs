//! Devanagari: Position und Silbenrolle der Zeichen U+0900..=U+097F.
//!
//! ERZEUGT aus /usr/share/unicode/IndicPositionalCategory.txt und
//! /usr/share/unicode/IndicSyllabicCategory.txt (Unicode 15.1.0) durch
//! /tmp/opencode/gen_i4.py. Keine Handarbeit.

/// Position eines Vokalzeichens (Indic_Positional_Category).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vokallage {
    /// Links vom Konsonanten (prae-basisch), z. B. U+093F.
    Links,
    /// Oben.
    Oben,
    /// Unten.
    Unten,
    /// Rechts.
    Rechts,
    /// Ueberlagert (auf dem Konsonanten, Indic_Positional_Category=Overstruck).
    Ueberlagert,
    /// Keine Vokalposition (Konsonant, Halant, Ziffer ...).
    Keine,
}

/// Rolle im Silbenaufbau (Indic_Syllabic_Category, ausgewaehlt).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Silbenrolle {
    /// Konsonant (einschliesslich RA).
    Konsonant,
    /// Halant/Virama (tote Konsonantenmarke).
    Halant,
    /// unabhaengiges Vokalzeichen.
    Vokal,
    /// abhaengiges Vokalzeichen (Matra).
    Matra,
    /// Nukta, Anusvara, Visarga, Chandrabindu und aehnliche Zeichen.
    Zeichen,
    /// Alles andere (Ziffern, Satzzeichen ...).
    Sonstiges,
}

/// (Zeichen, Vokallage, Silbenrolle), sortiert.
const DEVA: &[(char, Vokallage, Silbenrolle)] = &[
    ('\u{0900}', Vokallage::Oben, Silbenrolle::Zeichen),
    ('\u{0901}', Vokallage::Oben, Silbenrolle::Zeichen),
    ('\u{0902}', Vokallage::Oben, Silbenrolle::Zeichen),
    ('\u{0903}', Vokallage::Rechts, Silbenrolle::Zeichen),
    ('\u{0904}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{0905}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{0906}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{0907}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{0908}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{0909}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{090A}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{090B}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{090C}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{090D}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{090E}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{090F}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{0910}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{0911}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{0912}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{0913}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{0914}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{0915}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0916}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0917}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0918}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0919}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{091A}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{091B}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{091C}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{091D}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{091E}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{091F}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0920}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0921}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0922}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0923}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0924}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0925}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0926}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0927}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0928}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0929}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{092A}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{092B}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{092C}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{092D}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{092E}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{092F}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0930}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0931}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0932}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0933}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0934}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0935}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0936}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0937}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0938}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0939}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{093A}', Vokallage::Oben, Silbenrolle::Matra),
    ('\u{093B}', Vokallage::Rechts, Silbenrolle::Matra),
    ('\u{093C}', Vokallage::Unten, Silbenrolle::Zeichen),
    ('\u{093D}', Vokallage::Keine, Silbenrolle::Sonstiges),
    ('\u{093E}', Vokallage::Rechts, Silbenrolle::Matra),
    ('\u{093F}', Vokallage::Links, Silbenrolle::Matra),
    ('\u{0940}', Vokallage::Rechts, Silbenrolle::Matra),
    ('\u{0941}', Vokallage::Unten, Silbenrolle::Matra),
    ('\u{0942}', Vokallage::Unten, Silbenrolle::Matra),
    ('\u{0943}', Vokallage::Unten, Silbenrolle::Matra),
    ('\u{0944}', Vokallage::Unten, Silbenrolle::Matra),
    ('\u{0945}', Vokallage::Oben, Silbenrolle::Matra),
    ('\u{0946}', Vokallage::Oben, Silbenrolle::Matra),
    ('\u{0947}', Vokallage::Oben, Silbenrolle::Matra),
    ('\u{0948}', Vokallage::Oben, Silbenrolle::Matra),
    ('\u{0949}', Vokallage::Rechts, Silbenrolle::Matra),
    ('\u{094A}', Vokallage::Rechts, Silbenrolle::Matra),
    ('\u{094B}', Vokallage::Rechts, Silbenrolle::Matra),
    ('\u{094C}', Vokallage::Rechts, Silbenrolle::Matra),
    ('\u{094D}', Vokallage::Unten, Silbenrolle::Halant),
    ('\u{094E}', Vokallage::Links, Silbenrolle::Matra),
    ('\u{094F}', Vokallage::Rechts, Silbenrolle::Matra),
    ('\u{0951}', Vokallage::Oben, Silbenrolle::Zeichen),
    ('\u{0952}', Vokallage::Unten, Silbenrolle::Zeichen),
    ('\u{0955}', Vokallage::Oben, Silbenrolle::Matra),
    ('\u{0956}', Vokallage::Unten, Silbenrolle::Matra),
    ('\u{0957}', Vokallage::Unten, Silbenrolle::Matra),
    ('\u{0958}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0959}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{095A}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{095B}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{095C}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{095D}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{095E}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{095F}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0960}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{0961}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{0962}', Vokallage::Unten, Silbenrolle::Matra),
    ('\u{0963}', Vokallage::Unten, Silbenrolle::Matra),
    ('\u{0966}', Vokallage::Keine, Silbenrolle::Sonstiges),
    ('\u{0967}', Vokallage::Keine, Silbenrolle::Sonstiges),
    ('\u{0968}', Vokallage::Keine, Silbenrolle::Sonstiges),
    ('\u{0969}', Vokallage::Keine, Silbenrolle::Sonstiges),
    ('\u{096A}', Vokallage::Keine, Silbenrolle::Sonstiges),
    ('\u{096B}', Vokallage::Keine, Silbenrolle::Sonstiges),
    ('\u{096C}', Vokallage::Keine, Silbenrolle::Sonstiges),
    ('\u{096D}', Vokallage::Keine, Silbenrolle::Sonstiges),
    ('\u{096E}', Vokallage::Keine, Silbenrolle::Sonstiges),
    ('\u{096F}', Vokallage::Keine, Silbenrolle::Sonstiges),
    ('\u{0972}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{0973}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{0974}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{0975}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{0976}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{0977}', Vokallage::Keine, Silbenrolle::Vokal),
    ('\u{0978}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{0979}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{097A}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{097B}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{097C}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{097D}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{097E}', Vokallage::Keine, Silbenrolle::Konsonant),
    ('\u{097F}', Vokallage::Keine, Silbenrolle::Konsonant),
];

/// Liefert (Vokallage, Silbenrolle) eines Devanagari-Zeichens.
pub fn deva_zeichen(ch: char) -> (Vokallage, Silbenrolle) {
    DEVA.binary_search_by(|&(c, _, _)| c.cmp(&ch))
        .ok()
        .map(|i| (DEVA[i].1, DEVA[i].2))
        .unwrap_or((Vokallage::Keine, Silbenrolle::Sonstiges))
}

/// Ist das Zeichen im Devanagari-Block U+0900..=U+097F?
pub fn ist_devanagari(ch: char) -> bool {
    ('\u{0900}'..='\u{097F}').contains(&ch)
}
