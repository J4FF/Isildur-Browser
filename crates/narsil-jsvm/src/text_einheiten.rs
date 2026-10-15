//! Release 15.10.2026: UTF-16 positions on the engine's UTF-8 strings.
//!
//! ECMAScript string positions count UTF-16 code units (`length`,
//! `charCodeAt`, RegExp `index`/`lastIndex`). The engine stores strings as
//! UTF-8 and the index methods (`charAt`, `slice`, `substring`, `substr`,
//! `indexOf`, `lastIndexOf`, `includes`, `startsWith`, `endsWith`, `at`,
//! `padStart`, `padEnd`) counted code points instead. MEASURED before (node
//! 22 in brackets) on `s = '\u{1F600}ab'` (length 4): `s.slice(2)` "b"
//! ("ab"), `s.indexOf('a')` 1 (2), `s.charAt(2)` "b" ("a"), `s.at(2)` "b"
//! ("a") — so `s.slice(m.index)` after a regex match was off by one per
//! emoji before the match.
//!
//! THE LIMIT, unchanged: a lone surrogate cannot live in a Rust `String`
//! (the lexer already turns `'\uD800'` into U+FFFD). A range that cuts a
//! surrogate pair therefore yields U+FFFD for the cut half (one unit, so all
//! lengths stay those of node; browsers render a lone surrogate as U+FFFD
//! too). A single-unit read (`s[i]`, `charAt`, `at`) at the FIRST unit of a
//! pair keeps returning the whole character, as `s[i]` always did, and the
//! second unit reads as U+FFFD — so a character-by-character copy keeps the
//! character instead of losing it.
//!
//! Every function has an ASCII fast path: for ASCII text byte, code point
//! and UTF-16 positions coincide (`str::is_ascii` is a word-wise scan).

/// Release 15.10.2026 (JSVM speed): the measures of the last string read
/// by position (`s.length`, `s[i]`, `charCodeAt` …), so a loop over one
/// string costs O(1) per step: ASCII-ness and UTF-16 length are computed
/// once, and the last (UTF-16, byte) position lets a forward walk over a
/// non-ASCII string continue instead of starting at 0. A heap slot keeps
/// its string until a GC frees it, so the slot plus the GC run count
/// identify the string. MEASURED before (youtube.com, DOMContentLoaded
/// handlers at the 15 s wall clock): stack samples sat in `this_text`
/// (a copy of the whole string per `charCodeAt`), `encode_utf16().nth(i)`
/// and the UTF-16 length scan — a char loop over a long string was
/// quadratic.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextZeiger {
    r: crate::heap::HeapRef,
    gc: u64,
    ascii: bool,
    laenge: usize,
    u16_pos: usize,
    byte_pos: usize,
}

/// What stands at a UTF-16 position: the character that starts there, or
/// the second unit (a trail surrogate) of a pair.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Stelle {
    Zeichen(char),
    Zweite(u16),
}

impl crate::interp::Engine {
    /// (ASCII?, UTF-16 length) of the string in slot `r` (cached).
    pub(crate) fn text_masse(&mut self, r: crate::heap::HeapRef) -> Option<(bool, usize)> {
        if let Some(z) = self.text_zeiger
            && z.r == r
            && z.gc == self.gc_laeufe
        {
            return Some((z.ascii, z.laenge));
        }
        let crate::heap::HObj::Zeichenkette(s) = self.heap.holen(r) else {
            return None;
        };
        let ascii = s.is_ascii();
        let laenge = if ascii { s.len() } else { s.chars().map(char::len_utf16).sum() };
        self.text_zeiger = Some(TextZeiger { r, gc: self.gc_laeufe, ascii, laenge, u16_pos: 0, byte_pos: 0 });
        Some((ascii, laenge))
    }

    /// The `Stelle` at UTF-16 position `i` of the string in slot `r`
    /// (`None` past the end or for a non-string).
    pub(crate) fn text_stelle(&mut self, r: crate::heap::HeapRef, i: usize) -> Option<Stelle> {
        let (ascii, laenge) = self.text_masse(r)?;
        if i >= laenge {
            return None;
        }
        let z = self.text_zeiger?;
        let crate::heap::HObj::Zeichenkette(s) = self.heap.holen(r) else {
            return None;
        };
        if ascii {
            return Some(Stelle::Zeichen(s.as_bytes()[i] as char));
        }
        let (mut u, mut b) = if z.u16_pos <= i { (z.u16_pos, z.byte_pos) } else { (0, 0) };
        let mut ergebnis = None;
        while let Some(c) = s[b..].chars().next() {
            let l = c.len_utf16();
            if u == i {
                ergebnis = Some(Stelle::Zeichen(c));
                break;
            }
            if u + l > i {
                let mut puffer = [0u16; 2];
                c.encode_utf16(&mut puffer);
                ergebnis = Some(Stelle::Zweite(puffer[1]));
                break;
            }
            u += l;
            b += c.len_utf8();
        }
        if let Some(z) = self.text_zeiger.as_mut() {
            z.u16_pos = u;
            z.byte_pos = b;
        }
        ergebnis
    }
}

/// The length in UTF-16 code units.
#[inline]
pub(crate) fn laenge(s: &str) -> usize {
    if s.is_ascii() {
        s.len()
    } else {
        s.chars().map(char::len_utf16).sum()
    }
}

/// The byte offset of UTF-16 position `i` (clamped to the end) and whether
/// `i` is the second unit of the character that starts at that byte.
pub(crate) fn zu_byte(s: &str, i: usize) -> (usize, bool) {
    if s.is_ascii() {
        return (i.min(s.len()), false);
    }
    let mut u = 0usize;
    for (b, c) in s.char_indices() {
        if u == i {
            return (b, false);
        }
        let l = c.len_utf16();
        if u + l > i {
            return (b, true);
        }
        u += l;
    }
    (s.len(), false)
}

/// The UTF-16 position of byte offset `b` (a char boundary).
#[inline]
pub(crate) fn von_byte(s: &str, b: usize) -> usize {
    let t = &s[..b.min(s.len())];
    if t.is_ascii() {
        t.len()
    } else {
        t.chars().map(char::len_utf16).sum()
    }
}

/// The units `[a, b)` as a string; a half of a cut surrogate pair becomes
/// U+FFFD.
pub(crate) fn teil(s: &str, a: usize, b: usize) -> String {
    if a >= b {
        return String::new();
    }
    if s.is_ascii() {
        return s[a.min(s.len())..b.min(s.len())].to_string();
    }
    let (ba, a_mitten) = zu_byte(s, a);
    let (bb, b_mitten) = zu_byte(s, b);
    let mut out = String::with_capacity(bb.saturating_sub(ba) + 6);
    let start = if a_mitten {
        out.push('\u{FFFD}');
        ba + s[ba..].chars().next().map_or(0, char::len_utf8)
    } else {
        ba
    };
    if start < bb {
        out.push_str(&s[start..bb]);
    }
    if b_mitten {
        out.push('\u{FFFD}');
    }
    out
}

/// The single unit at `i` as a string (`s[i]`, `charAt`, `at`): the whole
/// character at the first unit of a pair, U+FFFD at the second; `None`
/// past the end.
pub(crate) fn einheit(s: &str, i: usize) -> Option<String> {
    if s.is_ascii() {
        return s.get(i..i + 1).map(str::to_string);
    }
    let (b, mitten) = zu_byte(s, i);
    if b >= s.len() {
        return None;
    }
    if mitten {
        return Some('\u{FFFD}'.to_string());
    }
    s[b..].chars().next().map(|c| c.to_string())
}

/// StringIndexOf (6.1.4.1): the first match of `muster` at a position
/// `>= von` (UTF-16), or `None`.
pub(crate) fn finden(s: &str, muster: &str, von: usize) -> Option<usize> {
    let n = laenge(s);
    if von > n {
        return None;
    }
    if muster.is_empty() {
        return Some(von);
    }
    let (mut b, mitten) = zu_byte(s, von);
    if mitten {
        b += s[b..].chars().next().map_or(0, char::len_utf8);
    }
    s[b..].find(muster).map(|p| von_byte(s, b + p))
}

/// The last match of `muster` starting at a position `<= bis` (UTF-16).
pub(crate) fn rueckwaerts_finden(s: &str, muster: &str, bis: usize) -> Option<usize> {
    let n = laenge(s);
    let bis = bis.min(n);
    if muster.is_empty() {
        return Some(bis);
    }
    let (b, _) = zu_byte(s, bis);
    let mut ende = (b + muster.len()).min(s.len());
    while !s.is_char_boundary(ende) {
        ende -= 1;
    }
    s[..ende].rfind(muster).map(|p| von_byte(s, p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positionen_in_utf16() {
        let s = "\u{1F600}ab";
        assert_eq!(laenge(s), 4);
        assert_eq!(teil(s, 2, 4), "ab");
        assert_eq!(teil(s, 1, 3), "\u{FFFD}a");
        assert_eq!(teil(s, 0, 1), "\u{FFFD}");
        assert_eq!(teil(s, 0, 2), "\u{1F600}");
        assert_eq!(einheit(s, 0).as_deref(), Some("\u{1F600}"));
        assert_eq!(einheit(s, 1).as_deref(), Some("\u{FFFD}"));
        assert_eq!(einheit(s, 2).as_deref(), Some("a"));
        assert_eq!(einheit(s, 4), None);
        assert_eq!(finden(s, "a", 0), Some(2));
        assert_eq!(finden(s, "a", 1), Some(2));
        assert_eq!(finden(s, "", 4), Some(4));
        assert_eq!(finden(s, "", 5), None);
        assert_eq!(rueckwaerts_finden("aXaXa\u{e4}", "a", 9), Some(4));
        assert_eq!(rueckwaerts_finden("aaa", "aa", 0), Some(0));
        assert_eq!(rueckwaerts_finden("\u{1F600}a\u{1F600}a", "\u{1F600}", 4), Some(3));
        assert_eq!(teil("abc", 1, 9), "bc");
    }
}
