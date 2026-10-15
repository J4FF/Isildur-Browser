//! Gepaarte Klammern fuer Regel N0 (Bidi_Paired_Bracket).
//!
//! ERZEUGT aus /usr/share/unicode/BidiBrackets.txt (Unicode 15.1.0)
//! durch /tmp/opencode/gen_bidi.py.
//!
//! BD16 vergleicht Schliesser mit dem Bidi_Paired_Bracket-Wert des
//! Oeffners; dabei sind U+3008/U+2329 und U+3009/U+232A kanonisch
//! gleichwertig (siehe Regel BD16). Diese zwei Paare sind daher in
//! [`kanonisch`] zusammengefasst.

/// Art einer Klammer nach Bidi_Paired_Bracket_Type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Klammerart {
    Oeffnend,
    Schliessend,
}

/// (Zeichen, Art, Bidi_Paired_Bracket-Wert), sortiert.
const KLAMMERN: &[(char, Klammerart, char)] = &[
    ('\u{0028}', Klammerart::Oeffnend, '\u{0029}'),
    ('\u{0029}', Klammerart::Schliessend, '\u{0028}'),
    ('\u{005B}', Klammerart::Oeffnend, '\u{005D}'),
    ('\u{005D}', Klammerart::Schliessend, '\u{005B}'),
    ('\u{007B}', Klammerart::Oeffnend, '\u{007D}'),
    ('\u{007D}', Klammerart::Schliessend, '\u{007B}'),
    ('\u{0F3A}', Klammerart::Oeffnend, '\u{0F3B}'),
    ('\u{0F3B}', Klammerart::Schliessend, '\u{0F3A}'),
    ('\u{0F3C}', Klammerart::Oeffnend, '\u{0F3D}'),
    ('\u{0F3D}', Klammerart::Schliessend, '\u{0F3C}'),
    ('\u{169B}', Klammerart::Oeffnend, '\u{169C}'),
    ('\u{169C}', Klammerart::Schliessend, '\u{169B}'),
    ('\u{2045}', Klammerart::Oeffnend, '\u{2046}'),
    ('\u{2046}', Klammerart::Schliessend, '\u{2045}'),
    ('\u{207D}', Klammerart::Oeffnend, '\u{207E}'),
    ('\u{207E}', Klammerart::Schliessend, '\u{207D}'),
    ('\u{208D}', Klammerart::Oeffnend, '\u{208E}'),
    ('\u{208E}', Klammerart::Schliessend, '\u{208D}'),
    ('\u{2308}', Klammerart::Oeffnend, '\u{2309}'),
    ('\u{2309}', Klammerart::Schliessend, '\u{2308}'),
    ('\u{230A}', Klammerart::Oeffnend, '\u{230B}'),
    ('\u{230B}', Klammerart::Schliessend, '\u{230A}'),
    ('\u{2329}', Klammerart::Oeffnend, '\u{232A}'),
    ('\u{232A}', Klammerart::Schliessend, '\u{2329}'),
    ('\u{2768}', Klammerart::Oeffnend, '\u{2769}'),
    ('\u{2769}', Klammerart::Schliessend, '\u{2768}'),
    ('\u{276A}', Klammerart::Oeffnend, '\u{276B}'),
    ('\u{276B}', Klammerart::Schliessend, '\u{276A}'),
    ('\u{276C}', Klammerart::Oeffnend, '\u{276D}'),
    ('\u{276D}', Klammerart::Schliessend, '\u{276C}'),
    ('\u{276E}', Klammerart::Oeffnend, '\u{276F}'),
    ('\u{276F}', Klammerart::Schliessend, '\u{276E}'),
    ('\u{2770}', Klammerart::Oeffnend, '\u{2771}'),
    ('\u{2771}', Klammerart::Schliessend, '\u{2770}'),
    ('\u{2772}', Klammerart::Oeffnend, '\u{2773}'),
    ('\u{2773}', Klammerart::Schliessend, '\u{2772}'),
    ('\u{2774}', Klammerart::Oeffnend, '\u{2775}'),
    ('\u{2775}', Klammerart::Schliessend, '\u{2774}'),
    ('\u{27C5}', Klammerart::Oeffnend, '\u{27C6}'),
    ('\u{27C6}', Klammerart::Schliessend, '\u{27C5}'),
    ('\u{27E6}', Klammerart::Oeffnend, '\u{27E7}'),
    ('\u{27E7}', Klammerart::Schliessend, '\u{27E6}'),
    ('\u{27E8}', Klammerart::Oeffnend, '\u{27E9}'),
    ('\u{27E9}', Klammerart::Schliessend, '\u{27E8}'),
    ('\u{27EA}', Klammerart::Oeffnend, '\u{27EB}'),
    ('\u{27EB}', Klammerart::Schliessend, '\u{27EA}'),
    ('\u{27EC}', Klammerart::Oeffnend, '\u{27ED}'),
    ('\u{27ED}', Klammerart::Schliessend, '\u{27EC}'),
    ('\u{27EE}', Klammerart::Oeffnend, '\u{27EF}'),
    ('\u{27EF}', Klammerart::Schliessend, '\u{27EE}'),
    ('\u{2983}', Klammerart::Oeffnend, '\u{2984}'),
    ('\u{2984}', Klammerart::Schliessend, '\u{2983}'),
    ('\u{2985}', Klammerart::Oeffnend, '\u{2986}'),
    ('\u{2986}', Klammerart::Schliessend, '\u{2985}'),
    ('\u{2987}', Klammerart::Oeffnend, '\u{2988}'),
    ('\u{2988}', Klammerart::Schliessend, '\u{2987}'),
    ('\u{2989}', Klammerart::Oeffnend, '\u{298A}'),
    ('\u{298A}', Klammerart::Schliessend, '\u{2989}'),
    ('\u{298B}', Klammerart::Oeffnend, '\u{298C}'),
    ('\u{298C}', Klammerart::Schliessend, '\u{298B}'),
    ('\u{298D}', Klammerart::Oeffnend, '\u{2990}'),
    ('\u{298E}', Klammerart::Schliessend, '\u{298F}'),
    ('\u{298F}', Klammerart::Oeffnend, '\u{298E}'),
    ('\u{2990}', Klammerart::Schliessend, '\u{298D}'),
    ('\u{2991}', Klammerart::Oeffnend, '\u{2992}'),
    ('\u{2992}', Klammerart::Schliessend, '\u{2991}'),
    ('\u{2993}', Klammerart::Oeffnend, '\u{2994}'),
    ('\u{2994}', Klammerart::Schliessend, '\u{2993}'),
    ('\u{2995}', Klammerart::Oeffnend, '\u{2996}'),
    ('\u{2996}', Klammerart::Schliessend, '\u{2995}'),
    ('\u{2997}', Klammerart::Oeffnend, '\u{2998}'),
    ('\u{2998}', Klammerart::Schliessend, '\u{2997}'),
    ('\u{29D8}', Klammerart::Oeffnend, '\u{29D9}'),
    ('\u{29D9}', Klammerart::Schliessend, '\u{29D8}'),
    ('\u{29DA}', Klammerart::Oeffnend, '\u{29DB}'),
    ('\u{29DB}', Klammerart::Schliessend, '\u{29DA}'),
    ('\u{29FC}', Klammerart::Oeffnend, '\u{29FD}'),
    ('\u{29FD}', Klammerart::Schliessend, '\u{29FC}'),
    ('\u{2E22}', Klammerart::Oeffnend, '\u{2E23}'),
    ('\u{2E23}', Klammerart::Schliessend, '\u{2E22}'),
    ('\u{2E24}', Klammerart::Oeffnend, '\u{2E25}'),
    ('\u{2E25}', Klammerart::Schliessend, '\u{2E24}'),
    ('\u{2E26}', Klammerart::Oeffnend, '\u{2E27}'),
    ('\u{2E27}', Klammerart::Schliessend, '\u{2E26}'),
    ('\u{2E28}', Klammerart::Oeffnend, '\u{2E29}'),
    ('\u{2E29}', Klammerart::Schliessend, '\u{2E28}'),
    ('\u{2E55}', Klammerart::Oeffnend, '\u{2E56}'),
    ('\u{2E56}', Klammerart::Schliessend, '\u{2E55}'),
    ('\u{2E57}', Klammerart::Oeffnend, '\u{2E58}'),
    ('\u{2E58}', Klammerart::Schliessend, '\u{2E57}'),
    ('\u{2E59}', Klammerart::Oeffnend, '\u{2E5A}'),
    ('\u{2E5A}', Klammerart::Schliessend, '\u{2E59}'),
    ('\u{2E5B}', Klammerart::Oeffnend, '\u{2E5C}'),
    ('\u{2E5C}', Klammerart::Schliessend, '\u{2E5B}'),
    ('\u{3008}', Klammerart::Oeffnend, '\u{3009}'),
    ('\u{3009}', Klammerart::Schliessend, '\u{3008}'),
    ('\u{300A}', Klammerart::Oeffnend, '\u{300B}'),
    ('\u{300B}', Klammerart::Schliessend, '\u{300A}'),
    ('\u{300C}', Klammerart::Oeffnend, '\u{300D}'),
    ('\u{300D}', Klammerart::Schliessend, '\u{300C}'),
    ('\u{300E}', Klammerart::Oeffnend, '\u{300F}'),
    ('\u{300F}', Klammerart::Schliessend, '\u{300E}'),
    ('\u{3010}', Klammerart::Oeffnend, '\u{3011}'),
    ('\u{3011}', Klammerart::Schliessend, '\u{3010}'),
    ('\u{3014}', Klammerart::Oeffnend, '\u{3015}'),
    ('\u{3015}', Klammerart::Schliessend, '\u{3014}'),
    ('\u{3016}', Klammerart::Oeffnend, '\u{3017}'),
    ('\u{3017}', Klammerart::Schliessend, '\u{3016}'),
    ('\u{3018}', Klammerart::Oeffnend, '\u{3019}'),
    ('\u{3019}', Klammerart::Schliessend, '\u{3018}'),
    ('\u{301A}', Klammerart::Oeffnend, '\u{301B}'),
    ('\u{301B}', Klammerart::Schliessend, '\u{301A}'),
    ('\u{FE59}', Klammerart::Oeffnend, '\u{FE5A}'),
    ('\u{FE5A}', Klammerart::Schliessend, '\u{FE59}'),
    ('\u{FE5B}', Klammerart::Oeffnend, '\u{FE5C}'),
    ('\u{FE5C}', Klammerart::Schliessend, '\u{FE5B}'),
    ('\u{FE5D}', Klammerart::Oeffnend, '\u{FE5E}'),
    ('\u{FE5E}', Klammerart::Schliessend, '\u{FE5D}'),
    ('\u{FF08}', Klammerart::Oeffnend, '\u{FF09}'),
    ('\u{FF09}', Klammerart::Schliessend, '\u{FF08}'),
    ('\u{FF3B}', Klammerart::Oeffnend, '\u{FF3D}'),
    ('\u{FF3D}', Klammerart::Schliessend, '\u{FF3B}'),
    ('\u{FF5B}', Klammerart::Oeffnend, '\u{FF5D}'),
    ('\u{FF5D}', Klammerart::Schliessend, '\u{FF5B}'),
    ('\u{FF5F}', Klammerart::Oeffnend, '\u{FF60}'),
    ('\u{FF60}', Klammerart::Schliessend, '\u{FF5F}'),
    ('\u{FF62}', Klammerart::Oeffnend, '\u{FF63}'),
    ('\u{FF63}', Klammerart::Schliessend, '\u{FF62}'),
];

/// Liefert (Art, Bidi_Paired_Bracket) einer Klammer, falls sie eine ist.
pub fn klammer(ch: char) -> Option<(Klammerart, char)> {
    KLAMMERN
        .binary_search_by(|&(c, _, _)| c.cmp(&ch))
        .ok()
        .map(|i| (KLAMMERN[i].1, KLAMMERN[i].2))
}

/// Kanonisch gleichwertige Klammerzeichen nach BD16
/// (U+3008/U+2329 und U+3009/U+232A). Beide Zeichen einer Klasse werden
/// auf denselben Vertreter abgebildet, damit ein einfacher Vergleich
/// genuegt.
pub fn kanonisch(ch: char) -> char {
    match ch {
        '\u{2329}' => '\u{3008}',
        '\u{3008}' => '\u{3008}',
        '\u{232A}' => '\u{3009}',
        '\u{3009}' => '\u{3009}',
        _ => ch,
    }
}

/// Passen Oeffner und Schliesser zusammen (BD16, mit kanonischer
/// Gleichwertigkeit)?
pub fn passt(oeffner: char, schliesser: char) -> bool {
    match klammer(oeffner) {
        Some((Klammerart::Oeffnend, partner)) => {
            partner == schliesser || kanonisch(partner) == kanonisch(schliesser)
        }
        _ => false,
    }
}
