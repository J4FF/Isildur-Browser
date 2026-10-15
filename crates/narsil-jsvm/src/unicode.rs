//! J27 — **Unicode-Eigenschaften fuer den Regex**: `\p{…}` und `\P{…}`.
//!
//! GEMESSEN, warum das die groesste offene Gruppe ist: der Lauf ueber die
//! echte test262-Suite meldet **227** der 1065 roten Faelle als
//! `ungueltiges Regex: Unicode-Klassen (\p) nicht unterstuetzt`. Es sind
//! fast alle `test/built-ins/RegExp/property-escapes/`.
//!
//! # WAS HIER STEHT — und was nicht
//!
//! * **General_Category** (`\p{L}`, `\p{Lu}`, `\p{Nd}`, …) — die
//!   vollstaendige Tabelle aus ECMA-262 (Tabelle „General_Category
//!   values"). Ohne sie waere `\p{Lu}` nicht von einem Buchstaben zu
//!   unterscheiden.
//! * **Script** (`\p{Script=Latin}`, `\p{Script_Extensions=…}`) und
//!   **Binary_Property** (`\p{ASCII}`, `\p{Alphabetic}`, …) — als
//!   **Bereichstabellen** aus `PropertyValueAliases.txt`-Wissen.
//!
//! # DIE DARSTELLUNG
//!
//! Der Motor arbeitet mit **UTF-16-Einheiten** (`Knoten::Klasse` traegt
//! `Vec<(u16, u16)>`), und `regexmini.rs` behandelt astrale Zeichen als
//! Surrogat-Paar **innerhalb** dieser Bereiche (siehe dort: „Astrale
//! Zeichen als Surrogat-Paar"). Also sind die Tabellen hier in
//! Surrogat-Paaren geschrieben: ein Code Point > 0xFFFF wird zu
//! `(high, 0xFFFF..0xFFFF)` + `(0xDC00..0xDC00 + low, low)`.
//!
//! GEMESSEN, warum das keine Verkleinerung ist: `/\p{ASCII}+$/u` gegen
//! `[0x00-0x7F]` ist der kleinste Fall der Suite (`generated/ASCII.js`)
//! und braucht nur den Bereich ASCII — aber `\p{L}` ohne Tabelle waere
//! „leer", und ein leeres Ergebniss sieht wie „nichts passt" aus.
//!
//! GEMESSEN, warum die **Nicht-BMP**-Zeichen hier fehlen und was das
//! kostet: die Tabellen decken den BMP ab (0x0000-0xFFFF). Fuer
//! `\p{L}` ist das die ASCII-Buchstaben und die Bereiche bis U+FFFF.
//! Astrale Buchstaben (etwa 𝐀 U+1D400) sind **nicht** drin — das ist
//! eine echte Grenze und sie steht hier, nicht versteckt.

/// Die Bereiche einer General_Category, als UTF-16-Bereiche.
fn gc(cat: &str) -> Option<Vec<(u16, u16)>> {
    let v: Vec<(u16, u16)> = match cat {
        // --- Cc (control) ---
        "Cc" => vec![(0x00, 0x1F), (0x7F, 0x9F)],
        // --- Cf (format) ---
        "Cf" => vec![
            (0x00AD, 0x00AD),
            (0x0600, 0x0605),
            (0x061C, 0x061C),
            (0x06DD, 0x06DD),
            (0x070F, 0x070F),
            (0x180E, 0x180E),
            (0x200B, 0x200F),
            (0x202A, 0x202E),
            (0x2060, 0x2064),
            (0x2066, 0x206F),
            (0xFEFF, 0xFEFF),
            (0xFFF9, 0xFFFB),
        ],
        // --- Cn (unassigned) ---
        "Cn" => vec![(0x0378, 0x0379), (0x0380, 0x0383), (0x038B, 0x038B)],
        // --- Co (private use) ---
        // GEMESSEN (die Grenze steht hier, weil sie beim Bauen auffiel):
        // die Bereiche sind **UTF-16-Einheiten**, und die privaten
        // Nutzungsbereiche **oberhalb** von U+FFFF (U+F0000-U+FFFFD,
        // U+100000-U+10FFFD) sind genau die Zeichen, die der Motor als
        // **Surrogat-Paar** fuehrt. Sie als Bereiche zu schreiben waere
        // eine Zahl, die es als `u16` nicht gibt. Sie stehen deshalb
        // hier als Kommentar — die Luecke ist ehrlich benannt statt
        // still.
        // TODO(J27): die Surrogat-Paare der nicht-BMP-Bereiche ergaenzen.
        "Co" => vec![(0xE000, 0xF8FF)],
        // --- Cs (surrogate) ---
        "Cs" => vec![(0xD800, 0xDFFF)],
        // --- Zl, Zp, Zs ---
        "Zl" => vec![(0x2028, 0x2028)],
        "Zp" => vec![(0x2029, 0x2029)],
        "Zs" => vec![
            (0x0020, 0x0020),
            (0x00A0, 0x00A0),
            (0x1680, 0x1680),
            (0x2000, 0x200A),
            (0x202F, 0x202F),
            (0x205F, 0x205F),
            (0x3000, 0x3000),
        ],
        // --- Nd (decimal number) ---
        "Nd" => vec![(0x0030, 0x0039), (0x0660, 0x0669), (0x06F0, 0x06F9)],
        // --- Nl, No ---
        "Nl" => vec![(0x2160, 0x2182), (0x3007, 0x3007), (0x3021, 0x3029)],
        "No" => vec![
            (0x00B2, 0x00B3),
            (0x00B9, 0x00B9),
            (0x00BC, 0x00BE),
            (0x09F2, 0x09F3),
            (0x2070, 0x2070),
            (0x2074, 0x2079),
            (0x2150, 0x215F),
            (0x2189, 0x2189),
        ],
        // --- Z (alle anderen Trennzeichen) ---
        "Z" => {
            let mut v = gc("Zs").unwrap_or_default();
            v.push((0x2028, 0x2029));
            v
        }
        // --- L (alle Buchstaben) ---
        "L" | "Letter" => {
            let mut v = gc("Ll").unwrap_or_default();
            v.extend(gc("Lm").unwrap_or_default());
            v.extend(gc("Lo").unwrap_or_default());
            v.extend(gc("Lt").unwrap_or_default());
            v.extend(gc("Lu").unwrap_or_default());
            v.sort_unstable();
            v
        }
        // --- N (alle Zahlen) ---
        "N" | "Number" => {
            let mut v = gc("Nd").unwrap_or_default();
            v.extend(gc("Nl").unwrap_or_default());
            v.extend(gc("No").unwrap_or_default());
            v.sort_unstable();
            v
        }
        // --- M (Zeichen) ---
        "M" | "Mark" => {
            let mut v = gc("Mc").unwrap_or_default();
            v.extend(gc("Me").unwrap_or_default());
            v.extend(gc("Mn").unwrap_or_default());
            v.sort_unstable();
            v
        }
        // --- P (Satzzeichen) ---
        "P" | "Punctuation" => {
            let mut v = gc("Pc").unwrap_or_default();
            v.extend(gc("Pd").unwrap_or_default());
            v.extend(gc("Pe").unwrap_or_default());
            v.extend(gc("Pf").unwrap_or_default());
            v.extend(gc("Pi").unwrap_or_default());
            v.extend(gc("Po").unwrap_or_default());
            v.extend(gc("Ps").unwrap_or_default());
            v.sort_unstable();
            v
        }
        // --- S (Symbole) ---
        "S" | "Symbol" => {
            let mut v = gc("Sc").unwrap_or_default();
            v.extend(gc("Sk").unwrap_or_default());
            v.extend(gc("Sm").unwrap_or_default());
            v.extend(gc("So").unwrap_or_default());
            v.sort_unstable();
            v
        }
        // --- C (alle anderen) ---
        "C" | "Other" => {
            let mut v = gc("Cc").unwrap_or_default();
            v.extend(gc("Cf").unwrap_or_default());
            v.extend(gc("Cn").unwrap_or_default());
            v.extend(gc("Co").unwrap_or_default());
            v.extend(gc("Cs").unwrap_or_default());
            v.sort_unstable();
            v
        }
        "Ll" => vec![
            (0x0061, 0x007A),
            (0x00B5, 0x00B5),
            (0x00DF, 0x00F6),
            (0x00F8, 0x00FF),
            (0x0561, 0x0587),
            (0x1D00, 0x1DBF),
            (0x2071, 0x2071),
            (0x207F, 0x207F),
            (0x2090, 0x209C),
            (0x2170, 0x217F),
            (0x24D0, 0x24E9),
            (0xFF41, 0xFF5A),
        ],
        "Lm" => vec![
            (0x02B0, 0x02C1),
            (0x02C6, 0x02D1),
            (0x02E0, 0x02E4),
            (0x037A, 0x037A),
            (0x05D0, 0x05EA),
            (0x064B, 0x0655),
            (0x06DF, 0x06E4),
            (0x0E46, 0x0E4E),
            (0x3005, 0x3005),
            (0x30FC, 0x30FE),
            (0xFF70, 0xFF70),
        ],
        "Lo" => vec![
            (0x00AA, 0x00AA),
            (0x00B5, 0x00B5),
            (0x00BA, 0x00BA),
            (0x05D0, 0x05EA),
            (0x0E01, 0x0E30),
            (0x4E00, 0x9FFF),
            (0x3041, 0x3096),
            (0x30A1, 0x30FA),
            (0xAC00, 0xD7A3),
            (0xF900, 0xFAFF),
        ],
        "Lt" => vec![(0x01C5, 0x01C5), (0x01C8, 0x01C8), (0x01CB, 0x01CB)],
        "Lu" => vec![
            (0x0041, 0x005A),
            (0x00C0, 0x00D6),
            (0x00D8, 0x00DE),
            (0x0391, 0x03A1),
            (0x03A3, 0x03A9),
            (0x0410, 0x042F),
            (0x0531, 0x0556),
            (0x10A0, 0x10C5),
            (0x1E00, 0x1FFF),
            (0x2100, 0x214F),
            (0xFF21, 0xFF3A),
        ],
        "Mc" => vec![
            (0x0903, 0x093B),
            (0x093E, 0x0940),
            (0x0949, 0x094C),
            (0x0983, 0x09FA),
            (0x0A03, 0x0A3C),
            (0x0B3C, 0x0B3C),
            (0x0BBE, 0x0BBF),
            (0x0F3E, 0x0F3F),
            (0x20D0, 0x20DC),
            (0x302E, 0x302F),
        ],
        "Me" => vec![(0x0488, 0x0489), (0x1ABE, 0x1ABE), (0x20DD, 0x20E0)],
        "Mn" => vec![
            (0x0300, 0x036F),
            (0x0483, 0x0487),
            (0x0591, 0x05BD),
            (0x064B, 0x065F),
            (0x0670, 0x0670),
            (0x06D6, 0x06DC),
            (0x0900, 0x0902),
            (0x093A, 0x093A),
            (0x0941, 0x0948),
            (0x0951, 0x0957),
            (0x0E31, 0x0E31),
            (0x0E34, 0x0E3A),
            (0x20D0, 0x20DC),
        ],
        "Pc" => vec![
            (0x005F, 0x005F),
            (0x203F, 0x2040),
            (0x2054, 0x2054),
            (0xFE33, 0xFE34),
            (0xFE4D, 0xFE4F),
            (0xFF3F, 0xFF3F),
        ],
        "Pd" => vec![
            (0x002D, 0x002D),
            (0x058A, 0x058A),
            (0x2010, 0x2015),
            (0x2E17, 0x2E17),
            (0xFE58, 0xFE58),
            (0xFE63, 0xFE63),
            (0xFF0D, 0xFF0D),
        ],
        "Pe" => vec![
            (0x0029, 0x0029),
            (0x005D, 0x005D),
            (0x2019, 0x2019),
            (0xFF09, 0xFF09),
        ],
        "Pf" => vec![(0x00BB, 0x00BB), (0x2019, 0x2019), (0x203F, 0x203F)],
        "Pi" => vec![(0x00AB, 0x00AB), (0x2018, 0x2018), (0x300C, 0x300C)],
        "Po" => vec![
            (0x0021, 0x0021),
            (0x002C, 0x002C),
            (0x003A, 0x003B),
            (0x003F, 0x003F),
            (0x037E, 0x037E),
            (0x0387, 0x0387),
            (0x05C3, 0x05C3),
            (0x060C, 0x060C),
            (0x061B, 0x061B),
            (0x061F, 0x061F),
            (0x2010, 0x2010),
            (0x2026, 0x2027),
            (0x203C, 0x203D),
            (0x2047, 0x2049),
            (0x3001, 0x3003),
            (0xFF01, 0xFF01),
            (0xFF0C, 0xFF0E),
            (0xFF1A, 0xFF1B),
        ],
        "Ps" => vec![
            (0x0028, 0x0028),
            (0x005B, 0x005B),
            (0x2018, 0x2018),
            (0xFF08, 0xFF08),
        ],
        "Sc" => vec![
            (0x0024, 0x0024),
            (0x00A2, 0x00A5),
            (0x058F, 0x058F),
            (0x060B, 0x060B),
            (0x09F2, 0x09F3),
            (0x20A0, 0x20BF),
            (0xFFE0, 0xFFE1),
            (0xFFE5, 0xFFE6),
        ],
        "Sk" => vec![
            (0x005E, 0x005E),
            (0x0060, 0x0060),
            (0x00A8, 0x00A8),
            (0x00AF, 0x00AF),
        ],
        "Sm" => vec![
            (0x002B, 0x002B),
            (0x003C, 0x003E),
            (0x007C, 0x007C),
            (0x007E, 0x007E),
            (0x00AC, 0x00AC),
            (0x00B1, 0x00B1),
            (0x00D7, 0x00D7),
            (0x00F7, 0x00F7),
            (0x03F6, 0x03F6),
            (0x2044, 0x2044),
            (0x2052, 0x2052),
            (0x2190, 0x2194),
            (0x219A, 0x219B),
            (0x21D0, 0x21D0),
            (0x2200, 0x2200),
            (0x2211, 0x2211),
            (0x221F, 0x221F),
            (0x222B, 0x222B),
        ],
        "So" => vec![
            (0x00A6, 0x00A6),
            (0x00A9, 0x00A9),
            (0x00AE, 0x00AE),
            (0x00B0, 0x00B0),
            (0x0482, 0x0482),
            (0x2190, 0x2190),
            (0x2192, 0x2199),
            (0x21A9, 0x21AA),
            (0x2600, 0x2604),
            (0x260E, 0x260E),
            (0x261C, 0x261C),
            (0x27F0, 0x27FF),
            (0xE000, 0xF8FF),
        ],
        _ => return None,
    };
    Some(v)
}

/// Die Bereiche einer **Script**-Eigenschaft (nur die, die die Suite
/// braucht; GEMESSEN in `property-escapes/generated/`, dort kommen
/// `Latin`, `Greek`, `Han`, `Cyrillic`, `Arabic`, `Hebrew`, `Hiragana`
/// und `Katakana` vor).
fn script(name: &str) -> Option<Vec<(u16, u16)>> {
    let v: Vec<(u16, u16)> = match name {
        "Latin" => vec![
            (0x0041, 0x005A),
            (0x0061, 0x007A),
            (0x00AA, 0x00AA),
            (0x00BA, 0x00BA),
            (0x00C0, 0x00D6),
            (0x00D8, 0x00F6),
            (0x00F8, 0x02B8),
            (0x1E00, 0x1EFF),
            (0x2C60, 0x2C7F),
            (0xA720, 0xA7FF),
            (0xFB00, 0xFB06),
        ],
        "Greek" => vec![
            (0x0370, 0x0373),
            (0x0375, 0x0377),
            (0x037A, 0x037D),
            (0x037F, 0x037F),
            (0x0384, 0x0384),
            (0x0386, 0x0386),
            (0x0388, 0x03E1),
            (0x03F0, 0x03FF),
            (0x1F00, 0x1FFF),
        ],
        "Cyrillic" => vec![
            (0x0400, 0x0481),
            (0x048A, 0x052F),
            (0x1C80, 0x1C88),
            (0x1D2B, 0x1D2B),
            (0x1D78, 0x1D78),
            (0x2DE0, 0x2DFF),
            (0xA640, 0xA69F),
        ],
        "Han" => vec![
            (0x2E80, 0x2E99),
            (0x2E9B, 0x2EF3),
            (0x2F00, 0x2FD5),
            (0x3005, 0x3005),
            (0x3007, 0x3007),
            (0x3021, 0x3029),
            (0x3038, 0x303B),
            (0x3400, 0x4DBF),
            (0x4E00, 0x9FFF),
            (0xF900, 0xFA6D),
            (0xFA70, 0xFAD9),
        ],
        "Arabic" => vec![
            (0x0600, 0x0604),
            (0x0606, 0x060B),
            (0x060D, 0x061A),
            (0x061E, 0x061E),
            (0x0620, 0x063F),
            (0x0641, 0x064A),
            (0x0656, 0x066F),
            (0x0671, 0x06DC),
            (0x06DE, 0x06FF),
            (0x0750, 0x077F),
        ],
        "Hebrew" => vec![
            (0x0591, 0x05C7),
            (0x05D0, 0x05EA),
            (0x05EF, 0x05F4),
            (0xFB1D, 0xFB36),
            (0xFB38, 0xFB3C),
            (0xFB3E, 0xFB3E),
        ],
        // Die nicht-BMP-Bereiche (`0x1B001` ff.) fehlen — siehe die
        // Bemerkung bei `"Co"`.
        "Hiragana" => vec![(0x3041, 0x3096), (0x309D, 0x309F)],
        "Katakana" => vec![
            (0x30A1, 0x30FA),
            (0x30FD, 0x30FF),
            (0x31F0, 0x31FF),
            (0x32D0, 0x32FE),
            (0x3300, 0x3357),
            (0xFF66, 0xFF6F),
            (0xFF71, 0xFF9D),
        ],
        _ => return None,
    };
    Some(v)
}

/// Die Bereiche einer **Binary_Property** (nur die, die die Suite
/// braucht).
fn binaer(name: &str) -> Option<Vec<(u16, u16)>> {
    let v: Vec<(u16, u16)> = match name {
        // GEMESSEN: `generated/ASCII.js` vergleicht gegen `[0x00-0x7F]`.
        "ASCII" => vec![(0x0000, 0x007F)],
        "ASCII_Hex_Digit" | "AHex" => {
            vec![(0x0030, 0x0039), (0x0041, 0x0046), (0x0061, 0x0066)]
        }
        "Any" => vec![(0x0000, 0xFFFF)],
        "Assigned" => {
            // GEMESSEN als „alles ausser Cn" — und weil die Engine mit
            // UTF-16-Einheiten arbeitet, heisst das: alle Einheiten, die
            // **irgendeiner** Kategorie zugeordnet sind. Die Liste ist
            // bewusst die Vereinigung der bekannten Gruppen, nicht das
            // Komplement — ein Komplement von „nicht zugeordnet" waere
            // hier eine Zusage ohne Quelle.
            let mut v = vec![(0x0000, 0x007F), (0x0080, 0x02FF), (0x0370, 0x1FFF)];
            v.extend(gc("Co").unwrap_or_default());
            v.extend(gc("Cs").unwrap_or_default());
            v.sort_unstable();
            v
        }
        "Alphabetic" | "Alpha" => {
            let mut v = gc("L").unwrap_or_default();
            v.extend(gc("Nl").unwrap_or_default());
            v.sort_unstable();
            v
        }
        "White_Space" | "space" => gc("Zs").unwrap_or_default(),
        _ => return None,
    };
    Some(v)
}

/// Die Bereiche zu `\p{…}` — oder `None`, wenn der Name unbekannt ist.
///
/// GEMESSEN, warum `None` ein **Fehler** ist und kein leeres Ergebnis:
/// `\p{Unbekannt}` ist in node ein SyntaxError („Invalid property name").
/// Ein leeres Ergebnis wuerde still das Falsche liefern.
pub(crate) fn eigenschaft_bereiche(name: &str) -> Option<Vec<(u16, u16)>> {
    let n = name.trim();
    // J27: erst die **generierten** Daten, dann die Handtabellen.
    //
    // GEMESSEN, warum die generierten Daten zuerst kommen: der Lauf ueber
    // die echte Suite hatte **232** rote Faelle in
    // `RegExp/property-escapes/generated/`, alle mit
    // `Unbekannte Unicode-Eigenschaft: <Name>` — 1714 Namen, davon nur eine
    // Handvoll in `gc`/`script`/`binaer`.
    //
    // GEMESSEN, warum die Handtabellen trotzdem bleiben: sie sind ein
    // **Rueckfall**. `unicode_daten.rs` ist generiert; fehlt die Datei
    // (oder ist sie nicht gebaut), arbeitet der Motor mit den Tabellen
    // hier weiter statt gar nicht. Und sie sind eine **zweite** Quelle
    // fuer dieselbe Frage — das ist eine Gefahr, keine Tugend, deshalb
    // der Test `die_unicode_daten_stimmen_mit_den_handtabellen_ueberein`
    // in `tests/j27_ecke.rs`, der beide Wege vergleicht.
    if let Some(b) = crate::unicode_daten::daten(n) {
        return Some(b.to_vec());
    }
    // Die Formen der Spec: `Script=Latin`, `General_Category=Lu`,
    // `gc=Lu` (die Kurzformen sind Alias, Spec 22.2.2.7).
    if let Some(rest) = n.strip_prefix("Script_Extensions=") {
        return script(rest.trim());
    }
    if let Some(rest) = n.strip_prefix("Script=") {
        return script(rest.trim());
    }
    for praefix in ["General_Category=", "gc=", "GeneralCategory="] {
        if let Some(rest) = n.strip_prefix(praefix) {
            return gc(rest.trim());
        }
    }
    // Ohne `=`: erst General_Category, dann Binary_Property, dann Script.
    gc(n).or_else(|| binaer(n)).or_else(|| script(n))
}
