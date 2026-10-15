//! Arabisches Shaping: Verbindungsklassen, Praesentationsformen und
//! RTL-Lauf-Reihenfolge fuer die Textausgabe.
//!
//! Dieses Modul ist kein vollstaendiger Schriftsatz. Es formt die Zeichen
//! U+0600..=U+06FF nach den Verbindungsregeln aus Unicode 15.1, ersetzt
//! sie durch ihre Praesentationsformen (U+FB50..=U+FEFF) und dreht
//! arabische Laeufe in die visuelle Reihenfolge, die
//! [`crate::layout_text`] erwartet (Glyphen von links nach rechts).
//!
//! # Aufruf in der Textausgabe (Rolle SCHRIFT)
//!
//! ```text
//! let sichtbar = narsil_text::arabisch::ausgabe(text);
//! // sichtbar an layout_text / layout_text_mit_breite / measure geben
//! ```
//!
//! Fuer die Anbindung in `shaping.rs` und der Breitenrechnung siehe
//! team/WUNSCH_DEEPSEEK.txt (Rolle SCHRIFT, A1).
//!
//! # Quellen (nachpruefbar auf dieser Maschine)
//!
//! * `/usr/share/unicode/ArabicShaping.txt` (15.1.0) — Joining_Type
//!   fuer U+0600..=U+06FF.
//! * `/usr/share/unicode/extracted/DerivedJoiningType.txt` — die
//!   transparenten Zeichen (T), z. B. Harakat.
//! * `/usr/share/unicode/UnicodeData.txt` — die Namen der
//!   Praesentationsformen U+FB50..=U+FEFF; daraus ist `FORMEN` erzeugt.
//! * mpcabd/python-arabic-reshaper (MIT), `letters.py`/`ligatures.py` —
//!   Gegenprobe: alle 76 Basiszeichen aus U+0600..=U+06FF stimmen mit
//!   `FORMEN` ueberein (0 Abweichungen); die Lam-Alef-Paare
//!   U+FEF5..=U+FEFC stehen dort genauso.
//!
//! # Grenzen (bewusst)
//!
//! * Kein vollstaendiger Bidi-Algorithmus (UAX #9). Behandelt wird der
//!   einfache Fall aus dem Auftrag: ein RTL-Lauf in LTR-Text. Ziffern und
//!   LTR-Text behalten ihre Reihenfolge.
//! * Harakat bleiben unveraendert. Die Formen U+FE70..=U+FE7F sind
//!   Sonderzeichen (u. a. fuer Tatweel) und werden von der
//!   Verbindungsformung nicht benutzt.
//! * Nur U+0600..=U+06FF wird geformt; andere arabische Bloecke bleiben
//!   unangetastet (offener Punkt im Wunsch an SCHRIFT).
//! * Keine Panics, lineare Laufzeit, Ausgabe nie laenger als die Eingabe.

use std::cmp::Ordering;

/// Verbindungsklasse eines Zeichens (Unicode-Eigenschaft `Joining_Type`).
///
/// Die Werte entsprechen der Spalte 2 in `ArabicShaping.txt`:
/// `D` = Beidseitig, `R` = Rechtsverbindend, `U` = Nichtverbindend,
/// `C` = Verursachend, `T` = Transparent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verbindungsklasse {
    /// `D` (Dual_Joining): schliesst nach beiden Seiten an (z. B. BEH).
    Beidseitig,
    /// `R` (Right_Joining): schliesst nur an das Zeichen davor an
    /// (z. B. ALEF, REH) und nimmt dann die Endform an.
    Rechtsverbindend,
    /// `U` (Non_Joining): schliesst nicht an (z. B. HAMZA, Ziffern).
    Nichtverbindend,
    /// `C` (Join_Causing): verursacht die Verbindung der Nachbarn, ohne
    /// eigene Form (TATWEEL U+0640, ZWJ U+200D).
    Verursachend,
    /// `T` (Transparent): Harakat; wird fuer die Verbindung uebersprungen
    /// und bleibt selbst unveraendert.
    Transparent,
}

/// Stellung eines Zeichens im Wort; bestimmt die Praesentationsform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Form {
    /// Alleinstehend (keine Verbindung auf beiden Seiten).
    Isoliert,
    /// Wortanfang: nur nach links (zum naechsten Zeichen) verbunden.
    Anfang,
    /// Wortmitte: nach beiden Seiten verbunden.
    Mitte,
    /// Wortende: nur nach rechts (zum vorigen Zeichen) verbunden.
    Ende,
}

/// Hoechstzahl der Formen je Basiszeichen (isoliert, Anfang, Mitte, Ende).
pub const MAX_FORMEN: usize = 4;

/// (erster, letzter, Klasse) fuer U+0600..=U+06FF.
///
/// ERZEUGT aus /usr/share/unicode/ArabicShaping.txt und
/// /usr/share/unicode/extracted/DerivedJoiningType.txt (Unicode 15.1.0).
/// Gemessene Verteilung im Bereich: 96 Beidseitig, 52 Rechtsverbindend,
/// 54 Nichtverbindend, 1 Verursachend, 53 Transparent.
const VERBINDUNGEN: &[(u32, u32, Verbindungsklasse)] = &[
    (0x0600, 0x060F, Verbindungsklasse::Nichtverbindend),
    (0x0610, 0x061A, Verbindungsklasse::Transparent),
    (0x061B, 0x061B, Verbindungsklasse::Nichtverbindend),
    (0x061C, 0x061C, Verbindungsklasse::Transparent),
    (0x061D, 0x061F, Verbindungsklasse::Nichtverbindend),
    (0x0620, 0x0620, Verbindungsklasse::Beidseitig),
    (0x0621, 0x0621, Verbindungsklasse::Nichtverbindend),
    (0x0622, 0x0625, Verbindungsklasse::Rechtsverbindend),
    (0x0626, 0x0626, Verbindungsklasse::Beidseitig),
    (0x0627, 0x0627, Verbindungsklasse::Rechtsverbindend),
    (0x0628, 0x0628, Verbindungsklasse::Beidseitig),
    (0x0629, 0x0629, Verbindungsklasse::Rechtsverbindend),
    (0x062A, 0x062E, Verbindungsklasse::Beidseitig),
    (0x062F, 0x0632, Verbindungsklasse::Rechtsverbindend),
    (0x0633, 0x063F, Verbindungsklasse::Beidseitig),
    (0x0640, 0x0640, Verbindungsklasse::Verursachend),
    (0x0641, 0x0647, Verbindungsklasse::Beidseitig),
    (0x0648, 0x0648, Verbindungsklasse::Rechtsverbindend),
    (0x0649, 0x064A, Verbindungsklasse::Beidseitig),
    (0x064B, 0x065F, Verbindungsklasse::Transparent),
    (0x0660, 0x066D, Verbindungsklasse::Nichtverbindend),
    (0x066E, 0x066F, Verbindungsklasse::Beidseitig),
    (0x0670, 0x0670, Verbindungsklasse::Transparent),
    (0x0671, 0x0673, Verbindungsklasse::Rechtsverbindend),
    (0x0674, 0x0674, Verbindungsklasse::Nichtverbindend),
    (0x0675, 0x0677, Verbindungsklasse::Rechtsverbindend),
    (0x0678, 0x0687, Verbindungsklasse::Beidseitig),
    (0x0688, 0x0699, Verbindungsklasse::Rechtsverbindend),
    (0x069A, 0x06BF, Verbindungsklasse::Beidseitig),
    (0x06C0, 0x06C0, Verbindungsklasse::Rechtsverbindend),
    (0x06C1, 0x06C2, Verbindungsklasse::Beidseitig),
    (0x06C3, 0x06CB, Verbindungsklasse::Rechtsverbindend),
    (0x06CC, 0x06CC, Verbindungsklasse::Beidseitig),
    (0x06CD, 0x06CD, Verbindungsklasse::Rechtsverbindend),
    (0x06CE, 0x06CE, Verbindungsklasse::Beidseitig),
    (0x06CF, 0x06CF, Verbindungsklasse::Rechtsverbindend),
    (0x06D0, 0x06D1, Verbindungsklasse::Beidseitig),
    (0x06D2, 0x06D3, Verbindungsklasse::Rechtsverbindend),
    (0x06D4, 0x06D4, Verbindungsklasse::Nichtverbindend),
    (0x06D5, 0x06D5, Verbindungsklasse::Rechtsverbindend),
    (0x06D6, 0x06DC, Verbindungsklasse::Transparent),
    (0x06DD, 0x06DE, Verbindungsklasse::Nichtverbindend),
    (0x06DF, 0x06E4, Verbindungsklasse::Transparent),
    (0x06E5, 0x06E6, Verbindungsklasse::Nichtverbindend),
    (0x06E7, 0x06E8, Verbindungsklasse::Transparent),
    (0x06E9, 0x06E9, Verbindungsklasse::Nichtverbindend),
    (0x06EA, 0x06ED, Verbindungsklasse::Transparent),
    (0x06EE, 0x06EF, Verbindungsklasse::Rechtsverbindend),
    (0x06F0, 0x06F9, Verbindungsklasse::Nichtverbindend),
    (0x06FA, 0x06FC, Verbindungsklasse::Beidseitig),
    (0x06FD, 0x06FE, Verbindungsklasse::Nichtverbindend),
    (0x06FF, 0x06FF, Verbindungsklasse::Beidseitig),
];

/// (Basiszeichen, Form, Praesentationsform).
///
/// ERZEUGT aus /usr/share/unicode/UnicodeData.txt (Namen der Zeichen
/// U+FB50..=U+FEFF); 255 Paare. Die beiden Formen von ALEF MAKSURA
/// (U+FBE8/FBE9) heissen dort "ARABIC LETTER UIGHUR KAZAKH KIRGHIZ ALEF
/// MAKSURA INITIAL/MEDIAL FORM" und folgen damit nicht dem Namensschema;
/// sie sind von Hand nachgetragen (Gegenprobe: arabic_reshaper
/// `letters.py`, 0 Abweichungen).
const FORMEN: &[(char, Form, char)] = &[
    ('\u{0621}', Form::Isoliert, '\u{FE80}'),
    ('\u{0622}', Form::Isoliert, '\u{FE81}'),
    ('\u{0622}', Form::Ende, '\u{FE82}'),
    ('\u{0623}', Form::Isoliert, '\u{FE83}'),
    ('\u{0623}', Form::Ende, '\u{FE84}'),
    ('\u{0624}', Form::Isoliert, '\u{FE85}'),
    ('\u{0624}', Form::Ende, '\u{FE86}'),
    ('\u{0625}', Form::Isoliert, '\u{FE87}'),
    ('\u{0625}', Form::Ende, '\u{FE88}'),
    ('\u{0626}', Form::Isoliert, '\u{FE89}'),
    ('\u{0626}', Form::Anfang, '\u{FE8B}'),
    ('\u{0626}', Form::Mitte, '\u{FE8C}'),
    ('\u{0626}', Form::Ende, '\u{FE8A}'),
    ('\u{0627}', Form::Isoliert, '\u{FE8D}'),
    ('\u{0627}', Form::Ende, '\u{FE8E}'),
    ('\u{0628}', Form::Isoliert, '\u{FE8F}'),
    ('\u{0628}', Form::Anfang, '\u{FE91}'),
    ('\u{0628}', Form::Mitte, '\u{FE92}'),
    ('\u{0628}', Form::Ende, '\u{FE90}'),
    ('\u{0629}', Form::Isoliert, '\u{FE93}'),
    ('\u{0629}', Form::Ende, '\u{FE94}'),
    ('\u{062A}', Form::Isoliert, '\u{FE95}'),
    ('\u{062A}', Form::Anfang, '\u{FE97}'),
    ('\u{062A}', Form::Mitte, '\u{FE98}'),
    ('\u{062A}', Form::Ende, '\u{FE96}'),
    ('\u{062B}', Form::Isoliert, '\u{FE99}'),
    ('\u{062B}', Form::Anfang, '\u{FE9B}'),
    ('\u{062B}', Form::Mitte, '\u{FE9C}'),
    ('\u{062B}', Form::Ende, '\u{FE9A}'),
    ('\u{062C}', Form::Isoliert, '\u{FE9D}'),
    ('\u{062C}', Form::Anfang, '\u{FE9F}'),
    ('\u{062C}', Form::Mitte, '\u{FEA0}'),
    ('\u{062C}', Form::Ende, '\u{FE9E}'),
    ('\u{062D}', Form::Isoliert, '\u{FEA1}'),
    ('\u{062D}', Form::Anfang, '\u{FEA3}'),
    ('\u{062D}', Form::Mitte, '\u{FEA4}'),
    ('\u{062D}', Form::Ende, '\u{FEA2}'),
    ('\u{062E}', Form::Isoliert, '\u{FEA5}'),
    ('\u{062E}', Form::Anfang, '\u{FEA7}'),
    ('\u{062E}', Form::Mitte, '\u{FEA8}'),
    ('\u{062E}', Form::Ende, '\u{FEA6}'),
    ('\u{062F}', Form::Isoliert, '\u{FEA9}'),
    ('\u{062F}', Form::Ende, '\u{FEAA}'),
    ('\u{0630}', Form::Isoliert, '\u{FEAB}'),
    ('\u{0630}', Form::Ende, '\u{FEAC}'),
    ('\u{0631}', Form::Isoliert, '\u{FEAD}'),
    ('\u{0631}', Form::Ende, '\u{FEAE}'),
    ('\u{0632}', Form::Isoliert, '\u{FEAF}'),
    ('\u{0632}', Form::Ende, '\u{FEB0}'),
    ('\u{0633}', Form::Isoliert, '\u{FEB1}'),
    ('\u{0633}', Form::Anfang, '\u{FEB3}'),
    ('\u{0633}', Form::Mitte, '\u{FEB4}'),
    ('\u{0633}', Form::Ende, '\u{FEB2}'),
    ('\u{0634}', Form::Isoliert, '\u{FEB5}'),
    ('\u{0634}', Form::Anfang, '\u{FEB7}'),
    ('\u{0634}', Form::Mitte, '\u{FEB8}'),
    ('\u{0634}', Form::Ende, '\u{FEB6}'),
    ('\u{0635}', Form::Isoliert, '\u{FEB9}'),
    ('\u{0635}', Form::Anfang, '\u{FEBB}'),
    ('\u{0635}', Form::Mitte, '\u{FEBC}'),
    ('\u{0635}', Form::Ende, '\u{FEBA}'),
    ('\u{0636}', Form::Isoliert, '\u{FEBD}'),
    ('\u{0636}', Form::Anfang, '\u{FEBF}'),
    ('\u{0636}', Form::Mitte, '\u{FEC0}'),
    ('\u{0636}', Form::Ende, '\u{FEBE}'),
    ('\u{0637}', Form::Isoliert, '\u{FEC1}'),
    ('\u{0637}', Form::Anfang, '\u{FEC3}'),
    ('\u{0637}', Form::Mitte, '\u{FEC4}'),
    ('\u{0637}', Form::Ende, '\u{FEC2}'),
    ('\u{0638}', Form::Isoliert, '\u{FEC5}'),
    ('\u{0638}', Form::Anfang, '\u{FEC7}'),
    ('\u{0638}', Form::Mitte, '\u{FEC8}'),
    ('\u{0638}', Form::Ende, '\u{FEC6}'),
    ('\u{0639}', Form::Isoliert, '\u{FEC9}'),
    ('\u{0639}', Form::Anfang, '\u{FECB}'),
    ('\u{0639}', Form::Mitte, '\u{FECC}'),
    ('\u{0639}', Form::Ende, '\u{FECA}'),
    ('\u{063A}', Form::Isoliert, '\u{FECD}'),
    ('\u{063A}', Form::Anfang, '\u{FECF}'),
    ('\u{063A}', Form::Mitte, '\u{FED0}'),
    ('\u{063A}', Form::Ende, '\u{FECE}'),
    ('\u{0641}', Form::Isoliert, '\u{FED1}'),
    ('\u{0641}', Form::Anfang, '\u{FED3}'),
    ('\u{0641}', Form::Mitte, '\u{FED4}'),
    ('\u{0641}', Form::Ende, '\u{FED2}'),
    ('\u{0642}', Form::Isoliert, '\u{FED5}'),
    ('\u{0642}', Form::Anfang, '\u{FED7}'),
    ('\u{0642}', Form::Mitte, '\u{FED8}'),
    ('\u{0642}', Form::Ende, '\u{FED6}'),
    ('\u{0643}', Form::Isoliert, '\u{FED9}'),
    ('\u{0643}', Form::Anfang, '\u{FEDB}'),
    ('\u{0643}', Form::Mitte, '\u{FEDC}'),
    ('\u{0643}', Form::Ende, '\u{FEDA}'),
    ('\u{0644}', Form::Isoliert, '\u{FEDD}'),
    ('\u{0644}', Form::Anfang, '\u{FEDF}'),
    ('\u{0644}', Form::Mitte, '\u{FEE0}'),
    ('\u{0644}', Form::Ende, '\u{FEDE}'),
    ('\u{0645}', Form::Isoliert, '\u{FEE1}'),
    ('\u{0645}', Form::Anfang, '\u{FEE3}'),
    ('\u{0645}', Form::Mitte, '\u{FEE4}'),
    ('\u{0645}', Form::Ende, '\u{FEE2}'),
    ('\u{0646}', Form::Isoliert, '\u{FEE5}'),
    ('\u{0646}', Form::Anfang, '\u{FEE7}'),
    ('\u{0646}', Form::Mitte, '\u{FEE8}'),
    ('\u{0646}', Form::Ende, '\u{FEE6}'),
    ('\u{0647}', Form::Isoliert, '\u{FEE9}'),
    ('\u{0647}', Form::Anfang, '\u{FEEB}'),
    ('\u{0647}', Form::Mitte, '\u{FEEC}'),
    ('\u{0647}', Form::Ende, '\u{FEEA}'),
    ('\u{0648}', Form::Isoliert, '\u{FEED}'),
    ('\u{0648}', Form::Ende, '\u{FEEE}'),
    ('\u{0649}', Form::Isoliert, '\u{FEEF}'),
    ('\u{0649}', Form::Anfang, '\u{FBE8}'),
    ('\u{0649}', Form::Mitte, '\u{FBE9}'),
    ('\u{0649}', Form::Ende, '\u{FEF0}'),
    ('\u{064A}', Form::Isoliert, '\u{FEF1}'),
    ('\u{064A}', Form::Anfang, '\u{FEF3}'),
    ('\u{064A}', Form::Mitte, '\u{FEF4}'),
    ('\u{064A}', Form::Ende, '\u{FEF2}'),
    ('\u{064B}', Form::Isoliert, '\u{FE70}'),
    ('\u{064C}', Form::Isoliert, '\u{FE72}'),
    ('\u{064D}', Form::Isoliert, '\u{FE74}'),
    ('\u{064E}', Form::Isoliert, '\u{FE76}'),
    ('\u{064E}', Form::Mitte, '\u{FE77}'),
    ('\u{064F}', Form::Isoliert, '\u{FE78}'),
    ('\u{064F}', Form::Mitte, '\u{FE79}'),
    ('\u{0650}', Form::Isoliert, '\u{FE7A}'),
    ('\u{0650}', Form::Mitte, '\u{FE7B}'),
    ('\u{0651}', Form::Isoliert, '\u{FE7C}'),
    ('\u{0651}', Form::Mitte, '\u{FE7D}'),
    ('\u{0652}', Form::Isoliert, '\u{FE7E}'),
    ('\u{0652}', Form::Mitte, '\u{FE7F}'),
    ('\u{0671}', Form::Isoliert, '\u{FB50}'),
    ('\u{0671}', Form::Ende, '\u{FB51}'),
    ('\u{0677}', Form::Isoliert, '\u{FBDD}'),
    ('\u{0679}', Form::Isoliert, '\u{FB66}'),
    ('\u{0679}', Form::Anfang, '\u{FB68}'),
    ('\u{0679}', Form::Mitte, '\u{FB69}'),
    ('\u{0679}', Form::Ende, '\u{FB67}'),
    ('\u{067A}', Form::Isoliert, '\u{FB5E}'),
    ('\u{067A}', Form::Anfang, '\u{FB60}'),
    ('\u{067A}', Form::Mitte, '\u{FB61}'),
    ('\u{067A}', Form::Ende, '\u{FB5F}'),
    ('\u{067B}', Form::Isoliert, '\u{FB52}'),
    ('\u{067B}', Form::Anfang, '\u{FB54}'),
    ('\u{067B}', Form::Mitte, '\u{FB55}'),
    ('\u{067B}', Form::Ende, '\u{FB53}'),
    ('\u{067E}', Form::Isoliert, '\u{FB56}'),
    ('\u{067E}', Form::Anfang, '\u{FB58}'),
    ('\u{067E}', Form::Mitte, '\u{FB59}'),
    ('\u{067E}', Form::Ende, '\u{FB57}'),
    ('\u{067F}', Form::Isoliert, '\u{FB62}'),
    ('\u{067F}', Form::Anfang, '\u{FB64}'),
    ('\u{067F}', Form::Mitte, '\u{FB65}'),
    ('\u{067F}', Form::Ende, '\u{FB63}'),
    ('\u{0680}', Form::Isoliert, '\u{FB5A}'),
    ('\u{0680}', Form::Anfang, '\u{FB5C}'),
    ('\u{0680}', Form::Mitte, '\u{FB5D}'),
    ('\u{0680}', Form::Ende, '\u{FB5B}'),
    ('\u{0683}', Form::Isoliert, '\u{FB76}'),
    ('\u{0683}', Form::Anfang, '\u{FB78}'),
    ('\u{0683}', Form::Mitte, '\u{FB79}'),
    ('\u{0683}', Form::Ende, '\u{FB77}'),
    ('\u{0684}', Form::Isoliert, '\u{FB72}'),
    ('\u{0684}', Form::Anfang, '\u{FB74}'),
    ('\u{0684}', Form::Mitte, '\u{FB75}'),
    ('\u{0684}', Form::Ende, '\u{FB73}'),
    ('\u{0686}', Form::Isoliert, '\u{FB7A}'),
    ('\u{0686}', Form::Anfang, '\u{FB7C}'),
    ('\u{0686}', Form::Mitte, '\u{FB7D}'),
    ('\u{0686}', Form::Ende, '\u{FB7B}'),
    ('\u{0687}', Form::Isoliert, '\u{FB7E}'),
    ('\u{0687}', Form::Anfang, '\u{FB80}'),
    ('\u{0687}', Form::Mitte, '\u{FB81}'),
    ('\u{0687}', Form::Ende, '\u{FB7F}'),
    ('\u{0688}', Form::Isoliert, '\u{FB88}'),
    ('\u{0688}', Form::Ende, '\u{FB89}'),
    ('\u{068C}', Form::Isoliert, '\u{FB84}'),
    ('\u{068C}', Form::Ende, '\u{FB85}'),
    ('\u{068D}', Form::Isoliert, '\u{FB82}'),
    ('\u{068D}', Form::Ende, '\u{FB83}'),
    ('\u{068E}', Form::Isoliert, '\u{FB86}'),
    ('\u{068E}', Form::Ende, '\u{FB87}'),
    ('\u{0691}', Form::Isoliert, '\u{FB8C}'),
    ('\u{0691}', Form::Ende, '\u{FB8D}'),
    ('\u{0698}', Form::Isoliert, '\u{FB8A}'),
    ('\u{0698}', Form::Ende, '\u{FB8B}'),
    ('\u{06A4}', Form::Isoliert, '\u{FB6A}'),
    ('\u{06A4}', Form::Anfang, '\u{FB6C}'),
    ('\u{06A4}', Form::Mitte, '\u{FB6D}'),
    ('\u{06A4}', Form::Ende, '\u{FB6B}'),
    ('\u{06A6}', Form::Isoliert, '\u{FB6E}'),
    ('\u{06A6}', Form::Anfang, '\u{FB70}'),
    ('\u{06A6}', Form::Mitte, '\u{FB71}'),
    ('\u{06A6}', Form::Ende, '\u{FB6F}'),
    ('\u{06A9}', Form::Isoliert, '\u{FB8E}'),
    ('\u{06A9}', Form::Anfang, '\u{FB90}'),
    ('\u{06A9}', Form::Mitte, '\u{FB91}'),
    ('\u{06A9}', Form::Ende, '\u{FB8F}'),
    ('\u{06AD}', Form::Isoliert, '\u{FBD3}'),
    ('\u{06AD}', Form::Anfang, '\u{FBD5}'),
    ('\u{06AD}', Form::Mitte, '\u{FBD6}'),
    ('\u{06AD}', Form::Ende, '\u{FBD4}'),
    ('\u{06AF}', Form::Isoliert, '\u{FB92}'),
    ('\u{06AF}', Form::Anfang, '\u{FB94}'),
    ('\u{06AF}', Form::Mitte, '\u{FB95}'),
    ('\u{06AF}', Form::Ende, '\u{FB93}'),
    ('\u{06B1}', Form::Isoliert, '\u{FB9A}'),
    ('\u{06B1}', Form::Anfang, '\u{FB9C}'),
    ('\u{06B1}', Form::Mitte, '\u{FB9D}'),
    ('\u{06B1}', Form::Ende, '\u{FB9B}'),
    ('\u{06B3}', Form::Isoliert, '\u{FB96}'),
    ('\u{06B3}', Form::Anfang, '\u{FB98}'),
    ('\u{06B3}', Form::Mitte, '\u{FB99}'),
    ('\u{06B3}', Form::Ende, '\u{FB97}'),
    ('\u{06BA}', Form::Isoliert, '\u{FB9E}'),
    ('\u{06BA}', Form::Ende, '\u{FB9F}'),
    ('\u{06BB}', Form::Isoliert, '\u{FBA0}'),
    ('\u{06BB}', Form::Anfang, '\u{FBA2}'),
    ('\u{06BB}', Form::Mitte, '\u{FBA3}'),
    ('\u{06BB}', Form::Ende, '\u{FBA1}'),
    ('\u{06BE}', Form::Isoliert, '\u{FBAA}'),
    ('\u{06BE}', Form::Anfang, '\u{FBAC}'),
    ('\u{06BE}', Form::Mitte, '\u{FBAD}'),
    ('\u{06BE}', Form::Ende, '\u{FBAB}'),
    ('\u{06C0}', Form::Isoliert, '\u{FBA4}'),
    ('\u{06C0}', Form::Ende, '\u{FBA5}'),
    ('\u{06C1}', Form::Isoliert, '\u{FBA6}'),
    ('\u{06C1}', Form::Anfang, '\u{FBA8}'),
    ('\u{06C1}', Form::Mitte, '\u{FBA9}'),
    ('\u{06C1}', Form::Ende, '\u{FBA7}'),
    ('\u{06C5}', Form::Isoliert, '\u{FBE0}'),
    ('\u{06C5}', Form::Ende, '\u{FBE1}'),
    ('\u{06C6}', Form::Isoliert, '\u{FBD9}'),
    ('\u{06C6}', Form::Ende, '\u{FBDA}'),
    ('\u{06C7}', Form::Isoliert, '\u{FBD7}'),
    ('\u{06C7}', Form::Ende, '\u{FBD8}'),
    ('\u{06C8}', Form::Isoliert, '\u{FBDB}'),
    ('\u{06C8}', Form::Ende, '\u{FBDC}'),
    ('\u{06C9}', Form::Isoliert, '\u{FBE2}'),
    ('\u{06C9}', Form::Ende, '\u{FBE3}'),
    ('\u{06CB}', Form::Isoliert, '\u{FBDE}'),
    ('\u{06CB}', Form::Ende, '\u{FBDF}'),
    ('\u{06CC}', Form::Isoliert, '\u{FBFC}'),
    ('\u{06CC}', Form::Anfang, '\u{FBFE}'),
    ('\u{06CC}', Form::Mitte, '\u{FBFF}'),
    ('\u{06CC}', Form::Ende, '\u{FBFD}'),
    ('\u{06D0}', Form::Isoliert, '\u{FBE4}'),
    ('\u{06D0}', Form::Anfang, '\u{FBE6}'),
    ('\u{06D0}', Form::Mitte, '\u{FBE7}'),
    ('\u{06D0}', Form::Ende, '\u{FBE5}'),
    ('\u{06D2}', Form::Isoliert, '\u{FBAE}'),
    ('\u{06D2}', Form::Ende, '\u{FBAF}'),
    ('\u{06D3}', Form::Isoliert, '\u{FBB0}'),
    ('\u{06D3}', Form::Ende, '\u{FBB1}'),
];

/// Verbindungsklasse eines Zeichens.
///
/// Fuer Zeichen ausserhalb U+0600..=U+06FF gilt die Fehlregel der
/// Unicode-Daten: `Nichtverbindend` — mit einer Ausnahme, den unsichtbaren
/// Verbindern (Quelle: `extracted/DerivedJoiningType.txt`):
///
/// * U+200D ZWJ -> [`Verbindungsklasse::Verursachend`] (verbindet),
/// * U+200C ZWNJ -> [`Verbindungsklasse::Nichtverbindend`] (trennt).
pub fn verbindungsklasse(ch: char) -> Verbindungsklasse {
    let cp = ch as u32;
    if let Ok(i) = VERBINDUNGEN.binary_search_by(|&(erst, letzt, _)| {
        if cp < erst {
            Ordering::Greater
        } else if cp > letzt {
            Ordering::Less
        } else {
            Ordering::Equal
        }
    }) {
        return VERBINDUNGEN[i].2;
    }
    // Die Tabelle deckt U+0600..=U+06FF vollstaendig ab; hier landen nur
    // Zeichen ausserhalb.
    match cp {
        0x200D => Verbindungsklasse::Verursachend,
        _ => Verbindungsklasse::Nichtverbindend,
    }
}

/// Liefert die Praesentationsform eines Basiszeichens, falls es sie gibt.
///
/// Quelle sind die Namen in `/usr/share/unicode/UnicodeData.txt`
/// (U+FB50..=U+FEFF). Beispiele: BEH U+0628 -> U+FE8F/FE90/FE91/FE92,
/// HAMZA U+0621 -> nur U+FE80. Fuer Zeichen ohne Form (Ziffern,
/// Satzzeichen, Latein) kommt `None`.
pub fn praesentationsform(ch: char, form: Form) -> Option<char> {
    FORMEN
        .binary_search_by(|&(basis, gesucht, _)| (basis, gesucht).cmp(&(ch, form)))
        .ok()
        .map(|i| FORMEN[i].2)
}

/// Ist das Zeichen rechts-nach-links-relevant ("arabischer Laufkern")?
///
/// Wahr fuer U+0600..=U+06FF (ohne Ziffern), fuer die Praesentationsformen
/// U+FB50..=U+FDFF und U+FE70..=U+FEFF. Harakat sind enthalten, weil sie
/// zum Lauf gehoeren; siehe auch [`verbindungsklasse`].
pub fn ist_arabisch(ch: char) -> bool {
    let cp = ch as u32;
    if (0x0600..=0x06FF).contains(&cp) {
        return !ist_ziffer(ch);
    }
    (0xFB50..=0xFDFF).contains(&cp) || (0xFE70..=0xFEFF).contains(&cp)
}

/// Formt arabische Zeichen in Praesentationsformen (logische Reihenfolge).
///
/// Die Ersetzung folgt den Verbindungsregeln aus `ArabicShaping.txt`:
///
/// * beidseitig verbundene Zeichen: isoliert/Anfang/Mitte/Ende,
/// * rechtsverbindende Zeichen: isoliert oder Ende,
/// * nicht verbindende Zeichen: isolierte Form, falls vorhanden (HAMZA),
/// * Tatweel und ZWJ bleiben stehen,
/// * Harakat bleiben an ihrem Platz und werden bei der Verbindung
///   uebersprungen,
/// * LAM + ALEF (U+0644 mit U+0622/0623/0625/0627) wird zur Ligatur
///   U+FEF5..=U+FEFC, LAM + ALEF MAKSURA (U+0649) zu U+FC43/U+FC86.
///
/// Die Reihenfolge der Zeichen bleibt logisch — fuer die Ausgabe folgt
/// [`ausgabe`]. Nichtarabische Zeichen bleiben unveraendert.
pub fn forme(text: &str) -> String {
    let zeichen: Vec<char> = text.chars().collect();
    let mut aus = String::with_capacity(text.len());
    let mut i = 0;
    while i < zeichen.len() {
        let ch = zeichen[i];
        match verbindungsklasse(ch) {
            Verbindungsklasse::Beidseitig => {
                if ch == '\u{0644}' {
                    if let Some(j) = naechstes_nicht_transparent(&zeichen, i + 1) {
                        if let Some((isoliert, ende)) = lam_alef(zeichen[j]) {
                            let ligatur = if verbindet_zurueck(&zeichen, i) {
                                ende
                            } else {
                                isoliert
                            };
                            aus.push(ligatur);
                            // Harakat zwischen LAM und ALEF bleiben erhalten.
                            aus.extend(zeichen[i + 1..j].iter());
                            i = j + 1;
                            continue;
                        }
                    }
                }
                let stellung = match (verbindet_zurueck(&zeichen, i), verbindet_vor(&zeichen, i)) {
                    (false, false) => Form::Isoliert,
                    (false, true) => Form::Anfang,
                    (true, false) => Form::Ende,
                    (true, true) => Form::Mitte,
                };
                aus.push(praesentationsform(ch, stellung).unwrap_or(ch));
            }
            Verbindungsklasse::Rechtsverbindend => {
                let stellung = if verbindet_zurueck(&zeichen, i) {
                    Form::Ende
                } else {
                    Form::Isoliert
                };
                aus.push(praesentationsform(ch, stellung).unwrap_or(ch));
            }
            Verbindungsklasse::Nichtverbindend => {
                aus.push(praesentationsform(ch, Form::Isoliert).unwrap_or(ch));
            }
            // Tatweel/ZWJ tragen keine eigene Form; Harakat bleiben stehen.
            Verbindungsklasse::Verursachend | Verbindungsklasse::Transparent => {
                aus.push(ch);
            }
        }
        i += 1;
    }
    aus
}

/// Bringt arabische Laeufe in die visuelle Reihenfolge (rechts nach links),
/// sodass [`crate::layout_text`] die Zeichen links-nach-rechts setzen kann.
///
/// Ablauf: [`forme`] in logischer Reihenfolge, danach wird jeder maximale
/// Lauf aus arabischen Zeichen (mit Harakat, unsichtbaren Verbindern,
/// Ziffern und trennenden Leerzeichen dazwischen) umgedreht. Dabei bleiben
///
/// * Ziffernfolgen in ihrer LTR-Reihenfolge,
/// * Harakat und ZWJ/ZWNJ bei ihrem Buchstaben (eine Einheit),
/// * Ziffern/Leerzeichen nach dem letzten arabischen Kern ausserhalb des
///   Laufs,
/// * LTR-Text vor und nach dem Lauf unveraendert.
pub fn ausgabe(text: &str) -> String {
    let geformt = forme(text);
    let zeichen: Vec<char> = geformt.chars().collect();
    let mut aus = String::with_capacity(geformt.len());
    let mut i = 0;
    while i < zeichen.len() {
        if !ist_arabisch(zeichen[i]) {
            aus.push(zeichen[i]);
            i += 1;
            continue;
        }
        // Lauf sammeln: Kerne, Harakat, unsichtbare Verbinder, Ziffern und
        // trennende Leerzeichen.
        let mut j = i;
        while j < zeichen.len()
            && (ist_arabisch(zeichen[j])
                || verbindungsklasse(zeichen[j]) == Verbindungsklasse::Transparent
                || ist_unsichtbar(zeichen[j])
                || ist_ziffer(zeichen[j])
                || ist_trenner(zeichen[j]))
        {
            j += 1;
        }
        // Am Ende alles abschneiden, was kein Kern/Harakat/Verbinder ist:
        // Ziffern und Leerzeichen nach dem letzten arabischen Zeichen
        // bleiben LTR.
        let mut ende = j;
        while ende > i
            && !(ist_arabisch(zeichen[ende - 1])
                || verbindungsklasse(zeichen[ende - 1]) == Verbindungsklasse::Transparent
                || ist_unsichtbar(zeichen[ende - 1]))
        {
            ende -= 1;
        }
        aus.push_str(&umdrehen(&zeichen[i..ende]));
        aus.extend(zeichen[ende..j].iter());
        i = j;
    }
    aus
}

/// Ist das Zeichen eine Ziffer, die ihre LTR-Reihenfolge behaelt?
///
/// ASCII 0-9, arabisch-indische Ziffern U+0660..=U+0669 und
/// U+06F0..=U+06F9.
fn ist_ziffer(ch: char) -> bool {
    ch.is_ascii_digit()
        || ('\u{0660}'..='\u{0669}').contains(&ch)
        || ('\u{06F0}'..='\u{06F9}').contains(&ch)
}

/// Unsichtbarer Verbinder (ZWJ/ZWNJ) — gehoert zur Einheit seines Buchstabens.
fn ist_unsichtbar(ch: char) -> bool {
    ch == '\u{200C}' || ch == '\u{200D}'
}

/// Trennendes Leerzeichen, das einen Lauf nicht zerreisst.
fn ist_trenner(ch: char) -> bool {
    ch == ' ' || ch == '\t' || ch == '\u{00A0}'
}

/// Naechstes Zeichen ab `ab`, das nicht transparent (Haraka) ist.
fn naechstes_nicht_transparent(zeichen: &[char], ab: usize) -> Option<usize> {
    (ab..zeichen.len()).find(|&k| verbindungsklasse(zeichen[k]) != Verbindungsklasse::Transparent)
}

/// Voriges Zeichen vor `bis` (ausschliesslich), das nicht transparent ist.
fn voriges_nicht_transparent(zeichen: &[char], bis: usize) -> Option<usize> {
    (0..bis)
        .rev()
        .find(|&k| verbindungsklasse(zeichen[k]) != Verbindungsklasse::Transparent)
}

/// Schliesst das Zeichen an das vorige (nicht transparente) Zeichen an?
///
/// Dafuer muss das vorige linksverbindend (D oder C) und das aktuelle
/// rechtsverbindend (D, R oder C) sein.
fn verbindet_zurueck(zeichen: &[char], i: usize) -> bool {
    let vor = match voriges_nicht_transparent(zeichen, i) {
        Some(k) => k,
        None => return false,
    };
    let links = verbindungsklasse(zeichen[vor]);
    let rechts = verbindungsklasse(zeichen[i]);
    //
    // `Rechtsverbindend` GEHOERT NICHT HIER HER, und ich habe es am
    // 04.10.2026 ausprobiert und wieder zurueckgenommen.
    //
    // GEMESSEN, was mein Versuch bewirkt hat: "العربية" ging von 8 auf 2
    // leere Spalten. Das sah nach einem Fortschritt aus und war es nicht —
    // es war eine andere Sprache. Die Koordination hat es am Bild erkannt
    // und entschieden: nach Unicode (ArabicShaping.txt, `Joining_Type` R)
    // verbindet ein rechtsverbindendes Zeichen NICHT mit dem folgenden
    // Buchstaben. Alef, Dal, Reh, Waw und Hamza sind alle R, und R heisst
    // ausdruecklich "haengt nur an dem Zeichen DAHINTER".
    //
    // GEMESSEN, woran ich mich stoeren habe: das ALEF hat keine Form mit
    // xMin nahe 0 (xMin 193 bei U+FE8D und U+FE8E), also faengt es niemand
    // links. Daraus habe ich geschlossen, es muesse dennoch zaehlen. Der
    // Fehlschluss war, "faengt niemanden" und "zaehlt nicht" zu verwechseln:
    // das ALEF haengt ja selbst am LAM, und genau das ist der Spalt, den
    // man sieht. Die Verbindungsrichtung ist eine EIGENSCHAFT des Zeichens,
    // nicht etwas, was man aus dem Umfeld rechnet.
    //
    // Die 6 leeren Spalten sind also korrektes Arabisch, und die
    // Kontrollgroesse steht in crates/narsil-text/tests/arabisch_shaping.rs:
    // der echte Wortabstand ist 12 Spalten, die 6 im Wort sind die
    // Verbindungsstellen zwischen zwei R-zeichen bzw. deren Nachbarn.
    matches!(
        links,
        Verbindungsklasse::Beidseitig | Verbindungsklasse::Verursachend
    ) && matches!(
        rechts,
        Verbindungsklasse::Beidseitig
            | Verbindungsklasse::Rechtsverbindend
            | Verbindungsklasse::Verursachend
    )
}

/// Schliesst das Zeichen an das naechste (nicht transparente) Zeichen an?
///
/// Dafuer muss das aktuelle linksverbindend (D oder C) und das naechste
/// rechtsverbindend (D, R oder C) sein.
fn verbindet_vor(zeichen: &[char], i: usize) -> bool {
    let nach = match naechstes_nicht_transparent(zeichen, i + 1) {
        Some(k) => k,
        None => return false,
    };
    let links = verbindungsklasse(zeichen[i]);
    let rechts = verbindungsklasse(zeichen[nach]);
    matches!(
        links,
        Verbindungsklasse::Beidseitig | Verbindungsklasse::Verursachend
    ) && matches!(
        rechts,
        Verbindungsklasse::Beidseitig
            | Verbindungsklasse::Rechtsverbindend
            | Verbindungsklasse::Verursachend
    )
}

/// Lam-Alef-Ligatur: liefert (isolierte Form, Endform) fuer LAM + ALEF.
///
/// Quelle: UnicodeData.txt (U+FEF5..=U+FEFC sowie U+FC43/U+FC86) und
/// arabic_reshaper `ligatures.py`.
fn lam_alef(nach: char) -> Option<(char, char)> {
    match nach {
        '\u{0622}' => Some(('\u{FEF5}', '\u{FEF6}')),
        '\u{0623}' => Some(('\u{FEF7}', '\u{FEF8}')),
        '\u{0625}' => Some(('\u{FEF9}', '\u{FEFA}')),
        '\u{0627}' => Some(('\u{FEFB}', '\u{FEFC}')),
        '\u{0649}' => Some(('\u{FC43}', '\u{FC86}')),
        _ => None,
    }
}

/// Dreht einen Lauf in visuelle Reihenfolge; Einheiten bleiben in Ordnung.
///
/// Eine Einheit ist ein Ziffernlauf, ein Kernzeichen mit seinen Harakat
/// und unsichtbaren Verbindern, oder ein einzelnes Zeichen. Die Einheiten
/// werden rueckwaerts aneinandergehaengt; innerhalb einer Einheit bleibt
/// die Reihenfolge erhalten.
fn umdrehen(zeichen: &[char]) -> String {
    let mut grenzen = Vec::with_capacity(zeichen.len() + 1);
    grenzen.push(0);
    let mut k = 0;
    while k < zeichen.len() {
        if ist_ziffer(zeichen[k]) {
            while k < zeichen.len() && ist_ziffer(zeichen[k]) {
                k += 1;
            }
        } else if ist_arabisch(zeichen[k]) {
            k += 1;
            while k < zeichen.len()
                && (verbindungsklasse(zeichen[k]) == Verbindungsklasse::Transparent
                    || ist_unsichtbar(zeichen[k]))
            {
                k += 1;
            }
        } else {
            k += 1;
        }
        grenzen.push(k);
    }
    let mut aus = String::with_capacity(zeichen.len());
    for u in (0..grenzen.len() - 1).rev() {
        aus.extend(zeichen[grenzen[u]..grenzen[u + 1]].iter());
    }
    aus
}
