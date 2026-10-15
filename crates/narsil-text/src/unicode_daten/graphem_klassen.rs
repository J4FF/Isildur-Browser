//! Grapheme_Cluster_Break, Extended_Pictographic und InCB
//! (UAX #29, Unicode 15.1).
//!
//! ERZEUGT (nicht abgetippt) aus den offiziellen Dateien auf dieser
//! Maschine; Erzeuger: /tmp/opencode/graphem_tabellen.py.
//!
//! * `/usr/share/unicode/auxiliary/GraphemeBreakProperty.txt`
//!   Kopfzeile: GraphemeBreakProperty-15.1.0.txt
//! * `/usr/share/unicode/emoji/emoji-data.txt`
//!   Kopfzeile: emoji-data.txt
//! * `/usr/share/unicode/DerivedCoreProperties.txt`
//!   Kopfzeile: DerivedCoreProperties-15.1.0.txt
//!
//! GCB: 1391 Bereiche aus der Datei, nach dem Zusammenfassen
//! benachbarter Bereiche 1371 Bereiche / 18003 Codepunkte.
//! EP: 511 Bereiche -> 78 / 3537 Codepunkte.
//! InCB: 202 Bereiche -> 201 / 1130 Codepunkte.
//!
//! Der Fehlwert ist ueberall der jeweils neutrale Wert (`Other`,
//! `false`, `None`); die Bereiche sind aufsteigend und disjunkt, die
//! Suche ist eine Binaersuche (O(log n), kein Alloc).

/// Grapheme_Cluster_Break-Eigenschaft (UAX #29, Tabelle 1).
///
/// Der Fehlwert fuer nicht gelistete Codepunkte ist `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Graphemklasse {
    Other,
    Control,
    Cr,
    Extend,
    L,
    Lf,
    Lv,
    Lvt,
    Prepend,
    RegionalIndicator,
    SpacingMark,
    T,
    V,
    Zwj,
}

/// Indic_Conjunct_Break (InCB) fuer Regel GB9c.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Incb {
    None,
    Linker,
    Consonant,
    Extend,
}

/// Ein zusammenhaengender Codepunkt-Bereich `start..=ende` mit Wert `wert`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Bereich<K> {
    start: u32,
    ende: u32,
    wert: K,
}

/// Sucht den Bereich, der `cp` enthaelt (Bereiche aufsteigend, disjunkt).
///
/// Binaersuche: hoechstens `log2(Laenge)` Schritte, keine Allokation.
fn bereich_suche<K: Copy>(bereiche: &[Bereich<K>], cp: u32) -> Option<K> {
    let mut unten = 0usize;
    let mut oben = bereiche.len();
    while unten < oben {
        let mitte = unten + (oben - unten) / 2;
        let bereich = &bereiche[mitte];
        if cp < bereich.start {
            oben = mitte;
        } else if cp > bereich.ende {
            unten = mitte + 1;
        } else {
            return Some(bereich.wert);
        }
    }
    None
}

/// Grapheme_Cluster_Break-Bereiche aus GraphemeBreakProperty.txt.
static GCB_BEREICHE: [Bereich<Graphemklasse>; 1371] = [
    Bereich {
        start: 0x0000,
        ende: 0x0009,
        wert: Graphemklasse::Control,
    },
    Bereich {
        start: 0x000A,
        ende: 0x000A,
        wert: Graphemklasse::Lf,
    },
    Bereich {
        start: 0x000B,
        ende: 0x000C,
        wert: Graphemklasse::Control,
    },
    Bereich {
        start: 0x000D,
        ende: 0x000D,
        wert: Graphemklasse::Cr,
    },
    Bereich {
        start: 0x000E,
        ende: 0x001F,
        wert: Graphemklasse::Control,
    },
    Bereich {
        start: 0x007F,
        ende: 0x009F,
        wert: Graphemklasse::Control,
    },
    Bereich {
        start: 0x00AD,
        ende: 0x00AD,
        wert: Graphemklasse::Control,
    },
    Bereich {
        start: 0x0300,
        ende: 0x036F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0483,
        ende: 0x0489,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0591,
        ende: 0x05BD,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x05BF,
        ende: 0x05BF,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x05C1,
        ende: 0x05C2,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x05C4,
        ende: 0x05C5,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x05C7,
        ende: 0x05C7,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0600,
        ende: 0x0605,
        wert: Graphemklasse::Prepend,
    },
    Bereich {
        start: 0x0610,
        ende: 0x061A,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x061C,
        ende: 0x061C,
        wert: Graphemklasse::Control,
    },
    Bereich {
        start: 0x064B,
        ende: 0x065F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0670,
        ende: 0x0670,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x06D6,
        ende: 0x06DC,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x06DD,
        ende: 0x06DD,
        wert: Graphemklasse::Prepend,
    },
    Bereich {
        start: 0x06DF,
        ende: 0x06E4,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x06E7,
        ende: 0x06E8,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x06EA,
        ende: 0x06ED,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x070F,
        ende: 0x070F,
        wert: Graphemklasse::Prepend,
    },
    Bereich {
        start: 0x0711,
        ende: 0x0711,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0730,
        ende: 0x074A,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x07A6,
        ende: 0x07B0,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x07EB,
        ende: 0x07F3,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x07FD,
        ende: 0x07FD,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0816,
        ende: 0x0819,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x081B,
        ende: 0x0823,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0825,
        ende: 0x0827,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0829,
        ende: 0x082D,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0859,
        ende: 0x085B,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0890,
        ende: 0x0891,
        wert: Graphemklasse::Prepend,
    },
    Bereich {
        start: 0x0898,
        ende: 0x089F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x08CA,
        ende: 0x08E1,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x08E2,
        ende: 0x08E2,
        wert: Graphemklasse::Prepend,
    },
    Bereich {
        start: 0x08E3,
        ende: 0x0902,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0903,
        ende: 0x0903,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x093A,
        ende: 0x093A,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x093B,
        ende: 0x093B,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x093C,
        ende: 0x093C,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x093E,
        ende: 0x0940,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0941,
        ende: 0x0948,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0949,
        ende: 0x094C,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x094D,
        ende: 0x094D,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x094E,
        ende: 0x094F,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0951,
        ende: 0x0957,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0962,
        ende: 0x0963,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0981,
        ende: 0x0981,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0982,
        ende: 0x0983,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x09BC,
        ende: 0x09BC,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x09BE,
        ende: 0x09BE,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x09BF,
        ende: 0x09C0,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x09C1,
        ende: 0x09C4,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x09C7,
        ende: 0x09C8,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x09CB,
        ende: 0x09CC,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x09CD,
        ende: 0x09CD,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x09D7,
        ende: 0x09D7,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x09E2,
        ende: 0x09E3,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x09FE,
        ende: 0x09FE,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0A01,
        ende: 0x0A02,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0A03,
        ende: 0x0A03,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0A3C,
        ende: 0x0A3C,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0A3E,
        ende: 0x0A40,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0A41,
        ende: 0x0A42,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0A47,
        ende: 0x0A48,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0A4B,
        ende: 0x0A4D,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0A51,
        ende: 0x0A51,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0A70,
        ende: 0x0A71,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0A75,
        ende: 0x0A75,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0A81,
        ende: 0x0A82,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0A83,
        ende: 0x0A83,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0ABC,
        ende: 0x0ABC,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0ABE,
        ende: 0x0AC0,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0AC1,
        ende: 0x0AC5,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0AC7,
        ende: 0x0AC8,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0AC9,
        ende: 0x0AC9,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0ACB,
        ende: 0x0ACC,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0ACD,
        ende: 0x0ACD,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0AE2,
        ende: 0x0AE3,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0AFA,
        ende: 0x0AFF,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0B01,
        ende: 0x0B01,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0B02,
        ende: 0x0B03,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0B3C,
        ende: 0x0B3C,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0B3E,
        ende: 0x0B3F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0B40,
        ende: 0x0B40,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0B41,
        ende: 0x0B44,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0B47,
        ende: 0x0B48,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0B4B,
        ende: 0x0B4C,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0B4D,
        ende: 0x0B4D,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0B55,
        ende: 0x0B57,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0B62,
        ende: 0x0B63,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0B82,
        ende: 0x0B82,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0BBE,
        ende: 0x0BBE,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0BBF,
        ende: 0x0BBF,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0BC0,
        ende: 0x0BC0,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0BC1,
        ende: 0x0BC2,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0BC6,
        ende: 0x0BC8,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0BCA,
        ende: 0x0BCC,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0BCD,
        ende: 0x0BCD,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0BD7,
        ende: 0x0BD7,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0C00,
        ende: 0x0C00,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0C01,
        ende: 0x0C03,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0C04,
        ende: 0x0C04,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0C3C,
        ende: 0x0C3C,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0C3E,
        ende: 0x0C40,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0C41,
        ende: 0x0C44,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0C46,
        ende: 0x0C48,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0C4A,
        ende: 0x0C4D,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0C55,
        ende: 0x0C56,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0C62,
        ende: 0x0C63,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0C81,
        ende: 0x0C81,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0C82,
        ende: 0x0C83,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0CBC,
        ende: 0x0CBC,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0CBE,
        ende: 0x0CBE,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0CBF,
        ende: 0x0CBF,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0CC0,
        ende: 0x0CC1,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0CC2,
        ende: 0x0CC2,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0CC3,
        ende: 0x0CC4,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0CC6,
        ende: 0x0CC6,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0CC7,
        ende: 0x0CC8,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0CCA,
        ende: 0x0CCB,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0CCC,
        ende: 0x0CCD,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0CD5,
        ende: 0x0CD6,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0CE2,
        ende: 0x0CE3,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0CF3,
        ende: 0x0CF3,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0D00,
        ende: 0x0D01,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0D02,
        ende: 0x0D03,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0D3B,
        ende: 0x0D3C,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0D3E,
        ende: 0x0D3E,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0D3F,
        ende: 0x0D40,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0D41,
        ende: 0x0D44,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0D46,
        ende: 0x0D48,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0D4A,
        ende: 0x0D4C,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0D4D,
        ende: 0x0D4D,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0D4E,
        ende: 0x0D4E,
        wert: Graphemklasse::Prepend,
    },
    Bereich {
        start: 0x0D57,
        ende: 0x0D57,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0D62,
        ende: 0x0D63,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0D81,
        ende: 0x0D81,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0D82,
        ende: 0x0D83,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0DCA,
        ende: 0x0DCA,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0DCF,
        ende: 0x0DCF,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0DD0,
        ende: 0x0DD1,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0DD2,
        ende: 0x0DD4,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0DD6,
        ende: 0x0DD6,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0DD8,
        ende: 0x0DDE,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0DDF,
        ende: 0x0DDF,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0DF2,
        ende: 0x0DF3,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0E31,
        ende: 0x0E31,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0E33,
        ende: 0x0E33,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0E34,
        ende: 0x0E3A,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0E47,
        ende: 0x0E4E,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0EB1,
        ende: 0x0EB1,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0EB3,
        ende: 0x0EB3,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0EB4,
        ende: 0x0EBC,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0EC8,
        ende: 0x0ECE,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0F18,
        ende: 0x0F19,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0F35,
        ende: 0x0F35,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0F37,
        ende: 0x0F37,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0F39,
        ende: 0x0F39,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0F3E,
        ende: 0x0F3F,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0F71,
        ende: 0x0F7E,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0F7F,
        ende: 0x0F7F,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x0F80,
        ende: 0x0F84,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0F86,
        ende: 0x0F87,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0F8D,
        ende: 0x0F97,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0F99,
        ende: 0x0FBC,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x0FC6,
        ende: 0x0FC6,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x102D,
        ende: 0x1030,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1031,
        ende: 0x1031,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1032,
        ende: 0x1037,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1039,
        ende: 0x103A,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x103B,
        ende: 0x103C,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x103D,
        ende: 0x103E,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1056,
        ende: 0x1057,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1058,
        ende: 0x1059,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x105E,
        ende: 0x1060,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1071,
        ende: 0x1074,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1082,
        ende: 0x1082,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1084,
        ende: 0x1084,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1085,
        ende: 0x1086,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x108D,
        ende: 0x108D,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x109D,
        ende: 0x109D,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1100,
        ende: 0x115F,
        wert: Graphemklasse::L,
    },
    Bereich {
        start: 0x1160,
        ende: 0x11A7,
        wert: Graphemklasse::V,
    },
    Bereich {
        start: 0x11A8,
        ende: 0x11FF,
        wert: Graphemklasse::T,
    },
    Bereich {
        start: 0x135D,
        ende: 0x135F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1712,
        ende: 0x1714,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1715,
        ende: 0x1715,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1732,
        ende: 0x1733,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1734,
        ende: 0x1734,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1752,
        ende: 0x1753,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1772,
        ende: 0x1773,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x17B4,
        ende: 0x17B5,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x17B6,
        ende: 0x17B6,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x17B7,
        ende: 0x17BD,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x17BE,
        ende: 0x17C5,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x17C6,
        ende: 0x17C6,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x17C7,
        ende: 0x17C8,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x17C9,
        ende: 0x17D3,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x17DD,
        ende: 0x17DD,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x180B,
        ende: 0x180D,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x180E,
        ende: 0x180E,
        wert: Graphemklasse::Control,
    },
    Bereich {
        start: 0x180F,
        ende: 0x180F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1885,
        ende: 0x1886,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x18A9,
        ende: 0x18A9,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1920,
        ende: 0x1922,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1923,
        ende: 0x1926,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1927,
        ende: 0x1928,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1929,
        ende: 0x192B,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1930,
        ende: 0x1931,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1932,
        ende: 0x1932,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1933,
        ende: 0x1938,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1939,
        ende: 0x193B,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1A17,
        ende: 0x1A18,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1A19,
        ende: 0x1A1A,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1A1B,
        ende: 0x1A1B,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1A55,
        ende: 0x1A55,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1A56,
        ende: 0x1A56,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1A57,
        ende: 0x1A57,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1A58,
        ende: 0x1A5E,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1A60,
        ende: 0x1A60,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1A62,
        ende: 0x1A62,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1A65,
        ende: 0x1A6C,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1A6D,
        ende: 0x1A72,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1A73,
        ende: 0x1A7C,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1A7F,
        ende: 0x1A7F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1AB0,
        ende: 0x1ACE,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1B00,
        ende: 0x1B03,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1B04,
        ende: 0x1B04,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1B34,
        ende: 0x1B3A,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1B3B,
        ende: 0x1B3B,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1B3C,
        ende: 0x1B3C,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1B3D,
        ende: 0x1B41,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1B42,
        ende: 0x1B42,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1B43,
        ende: 0x1B44,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1B6B,
        ende: 0x1B73,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1B80,
        ende: 0x1B81,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1B82,
        ende: 0x1B82,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1BA1,
        ende: 0x1BA1,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1BA2,
        ende: 0x1BA5,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1BA6,
        ende: 0x1BA7,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1BA8,
        ende: 0x1BA9,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1BAA,
        ende: 0x1BAA,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1BAB,
        ende: 0x1BAD,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1BE6,
        ende: 0x1BE6,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1BE7,
        ende: 0x1BE7,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1BE8,
        ende: 0x1BE9,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1BEA,
        ende: 0x1BEC,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1BED,
        ende: 0x1BED,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1BEE,
        ende: 0x1BEE,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1BEF,
        ende: 0x1BF1,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1BF2,
        ende: 0x1BF3,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1C24,
        ende: 0x1C2B,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1C2C,
        ende: 0x1C33,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1C34,
        ende: 0x1C35,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1C36,
        ende: 0x1C37,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1CD0,
        ende: 0x1CD2,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1CD4,
        ende: 0x1CE0,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1CE1,
        ende: 0x1CE1,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1CE2,
        ende: 0x1CE8,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1CED,
        ende: 0x1CED,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1CF4,
        ende: 0x1CF4,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1CF7,
        ende: 0x1CF7,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1CF8,
        ende: 0x1CF9,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1DC0,
        ende: 0x1DFF,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x200B,
        ende: 0x200B,
        wert: Graphemklasse::Control,
    },
    Bereich {
        start: 0x200C,
        ende: 0x200C,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x200D,
        ende: 0x200D,
        wert: Graphemklasse::Zwj,
    },
    Bereich {
        start: 0x200E,
        ende: 0x200F,
        wert: Graphemklasse::Control,
    },
    Bereich {
        start: 0x2028,
        ende: 0x202E,
        wert: Graphemklasse::Control,
    },
    Bereich {
        start: 0x2060,
        ende: 0x206F,
        wert: Graphemklasse::Control,
    },
    Bereich {
        start: 0x20D0,
        ende: 0x20F0,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x2CEF,
        ende: 0x2CF1,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x2D7F,
        ende: 0x2D7F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x2DE0,
        ende: 0x2DFF,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x302A,
        ende: 0x302F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x3099,
        ende: 0x309A,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xA66F,
        ende: 0xA672,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xA674,
        ende: 0xA67D,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xA69E,
        ende: 0xA69F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xA6F0,
        ende: 0xA6F1,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xA802,
        ende: 0xA802,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xA806,
        ende: 0xA806,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xA80B,
        ende: 0xA80B,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xA823,
        ende: 0xA824,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0xA825,
        ende: 0xA826,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xA827,
        ende: 0xA827,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0xA82C,
        ende: 0xA82C,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xA880,
        ende: 0xA881,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0xA8B4,
        ende: 0xA8C3,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0xA8C4,
        ende: 0xA8C5,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xA8E0,
        ende: 0xA8F1,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xA8FF,
        ende: 0xA8FF,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xA926,
        ende: 0xA92D,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xA947,
        ende: 0xA951,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xA952,
        ende: 0xA953,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0xA960,
        ende: 0xA97C,
        wert: Graphemklasse::L,
    },
    Bereich {
        start: 0xA980,
        ende: 0xA982,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xA983,
        ende: 0xA983,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0xA9B3,
        ende: 0xA9B3,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xA9B4,
        ende: 0xA9B5,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0xA9B6,
        ende: 0xA9B9,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xA9BA,
        ende: 0xA9BB,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0xA9BC,
        ende: 0xA9BD,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xA9BE,
        ende: 0xA9C0,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0xA9E5,
        ende: 0xA9E5,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xAA29,
        ende: 0xAA2E,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xAA2F,
        ende: 0xAA30,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0xAA31,
        ende: 0xAA32,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xAA33,
        ende: 0xAA34,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0xAA35,
        ende: 0xAA36,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xAA43,
        ende: 0xAA43,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xAA4C,
        ende: 0xAA4C,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xAA4D,
        ende: 0xAA4D,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0xAA7C,
        ende: 0xAA7C,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xAAB0,
        ende: 0xAAB0,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xAAB2,
        ende: 0xAAB4,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xAAB7,
        ende: 0xAAB8,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xAABE,
        ende: 0xAABF,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xAAC1,
        ende: 0xAAC1,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xAAEB,
        ende: 0xAAEB,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0xAAEC,
        ende: 0xAAED,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xAAEE,
        ende: 0xAAEF,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0xAAF5,
        ende: 0xAAF5,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0xAAF6,
        ende: 0xAAF6,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xABE3,
        ende: 0xABE4,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0xABE5,
        ende: 0xABE5,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xABE6,
        ende: 0xABE7,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0xABE8,
        ende: 0xABE8,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xABE9,
        ende: 0xABEA,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0xABEC,
        ende: 0xABEC,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0xABED,
        ende: 0xABED,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xAC00,
        ende: 0xAC00,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAC01,
        ende: 0xAC1B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAC1C,
        ende: 0xAC1C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAC1D,
        ende: 0xAC37,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAC38,
        ende: 0xAC38,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAC39,
        ende: 0xAC53,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAC54,
        ende: 0xAC54,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAC55,
        ende: 0xAC6F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAC70,
        ende: 0xAC70,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAC71,
        ende: 0xAC8B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAC8C,
        ende: 0xAC8C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAC8D,
        ende: 0xACA7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xACA8,
        ende: 0xACA8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xACA9,
        ende: 0xACC3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xACC4,
        ende: 0xACC4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xACC5,
        ende: 0xACDF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xACE0,
        ende: 0xACE0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xACE1,
        ende: 0xACFB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xACFC,
        ende: 0xACFC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xACFD,
        ende: 0xAD17,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAD18,
        ende: 0xAD18,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAD19,
        ende: 0xAD33,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAD34,
        ende: 0xAD34,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAD35,
        ende: 0xAD4F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAD50,
        ende: 0xAD50,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAD51,
        ende: 0xAD6B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAD6C,
        ende: 0xAD6C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAD6D,
        ende: 0xAD87,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAD88,
        ende: 0xAD88,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAD89,
        ende: 0xADA3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xADA4,
        ende: 0xADA4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xADA5,
        ende: 0xADBF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xADC0,
        ende: 0xADC0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xADC1,
        ende: 0xADDB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xADDC,
        ende: 0xADDC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xADDD,
        ende: 0xADF7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xADF8,
        ende: 0xADF8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xADF9,
        ende: 0xAE13,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAE14,
        ende: 0xAE14,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAE15,
        ende: 0xAE2F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAE30,
        ende: 0xAE30,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAE31,
        ende: 0xAE4B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAE4C,
        ende: 0xAE4C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAE4D,
        ende: 0xAE67,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAE68,
        ende: 0xAE68,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAE69,
        ende: 0xAE83,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAE84,
        ende: 0xAE84,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAE85,
        ende: 0xAE9F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAEA0,
        ende: 0xAEA0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAEA1,
        ende: 0xAEBB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAEBC,
        ende: 0xAEBC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAEBD,
        ende: 0xAED7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAED8,
        ende: 0xAED8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAED9,
        ende: 0xAEF3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAEF4,
        ende: 0xAEF4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAEF5,
        ende: 0xAF0F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAF10,
        ende: 0xAF10,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAF11,
        ende: 0xAF2B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAF2C,
        ende: 0xAF2C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAF2D,
        ende: 0xAF47,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAF48,
        ende: 0xAF48,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAF49,
        ende: 0xAF63,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAF64,
        ende: 0xAF64,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAF65,
        ende: 0xAF7F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAF80,
        ende: 0xAF80,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAF81,
        ende: 0xAF9B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAF9C,
        ende: 0xAF9C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAF9D,
        ende: 0xAFB7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAFB8,
        ende: 0xAFB8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAFB9,
        ende: 0xAFD3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAFD4,
        ende: 0xAFD4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAFD5,
        ende: 0xAFEF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xAFF0,
        ende: 0xAFF0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xAFF1,
        ende: 0xB00B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB00C,
        ende: 0xB00C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB00D,
        ende: 0xB027,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB028,
        ende: 0xB028,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB029,
        ende: 0xB043,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB044,
        ende: 0xB044,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB045,
        ende: 0xB05F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB060,
        ende: 0xB060,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB061,
        ende: 0xB07B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB07C,
        ende: 0xB07C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB07D,
        ende: 0xB097,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB098,
        ende: 0xB098,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB099,
        ende: 0xB0B3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB0B4,
        ende: 0xB0B4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB0B5,
        ende: 0xB0CF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB0D0,
        ende: 0xB0D0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB0D1,
        ende: 0xB0EB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB0EC,
        ende: 0xB0EC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB0ED,
        ende: 0xB107,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB108,
        ende: 0xB108,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB109,
        ende: 0xB123,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB124,
        ende: 0xB124,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB125,
        ende: 0xB13F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB140,
        ende: 0xB140,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB141,
        ende: 0xB15B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB15C,
        ende: 0xB15C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB15D,
        ende: 0xB177,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB178,
        ende: 0xB178,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB179,
        ende: 0xB193,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB194,
        ende: 0xB194,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB195,
        ende: 0xB1AF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB1B0,
        ende: 0xB1B0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB1B1,
        ende: 0xB1CB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB1CC,
        ende: 0xB1CC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB1CD,
        ende: 0xB1E7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB1E8,
        ende: 0xB1E8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB1E9,
        ende: 0xB203,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB204,
        ende: 0xB204,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB205,
        ende: 0xB21F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB220,
        ende: 0xB220,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB221,
        ende: 0xB23B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB23C,
        ende: 0xB23C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB23D,
        ende: 0xB257,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB258,
        ende: 0xB258,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB259,
        ende: 0xB273,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB274,
        ende: 0xB274,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB275,
        ende: 0xB28F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB290,
        ende: 0xB290,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB291,
        ende: 0xB2AB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB2AC,
        ende: 0xB2AC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB2AD,
        ende: 0xB2C7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB2C8,
        ende: 0xB2C8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB2C9,
        ende: 0xB2E3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB2E4,
        ende: 0xB2E4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB2E5,
        ende: 0xB2FF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB300,
        ende: 0xB300,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB301,
        ende: 0xB31B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB31C,
        ende: 0xB31C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB31D,
        ende: 0xB337,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB338,
        ende: 0xB338,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB339,
        ende: 0xB353,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB354,
        ende: 0xB354,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB355,
        ende: 0xB36F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB370,
        ende: 0xB370,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB371,
        ende: 0xB38B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB38C,
        ende: 0xB38C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB38D,
        ende: 0xB3A7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB3A8,
        ende: 0xB3A8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB3A9,
        ende: 0xB3C3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB3C4,
        ende: 0xB3C4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB3C5,
        ende: 0xB3DF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB3E0,
        ende: 0xB3E0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB3E1,
        ende: 0xB3FB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB3FC,
        ende: 0xB3FC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB3FD,
        ende: 0xB417,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB418,
        ende: 0xB418,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB419,
        ende: 0xB433,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB434,
        ende: 0xB434,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB435,
        ende: 0xB44F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB450,
        ende: 0xB450,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB451,
        ende: 0xB46B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB46C,
        ende: 0xB46C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB46D,
        ende: 0xB487,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB488,
        ende: 0xB488,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB489,
        ende: 0xB4A3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB4A4,
        ende: 0xB4A4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB4A5,
        ende: 0xB4BF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB4C0,
        ende: 0xB4C0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB4C1,
        ende: 0xB4DB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB4DC,
        ende: 0xB4DC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB4DD,
        ende: 0xB4F7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB4F8,
        ende: 0xB4F8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB4F9,
        ende: 0xB513,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB514,
        ende: 0xB514,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB515,
        ende: 0xB52F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB530,
        ende: 0xB530,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB531,
        ende: 0xB54B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB54C,
        ende: 0xB54C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB54D,
        ende: 0xB567,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB568,
        ende: 0xB568,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB569,
        ende: 0xB583,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB584,
        ende: 0xB584,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB585,
        ende: 0xB59F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB5A0,
        ende: 0xB5A0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB5A1,
        ende: 0xB5BB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB5BC,
        ende: 0xB5BC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB5BD,
        ende: 0xB5D7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB5D8,
        ende: 0xB5D8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB5D9,
        ende: 0xB5F3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB5F4,
        ende: 0xB5F4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB5F5,
        ende: 0xB60F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB610,
        ende: 0xB610,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB611,
        ende: 0xB62B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB62C,
        ende: 0xB62C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB62D,
        ende: 0xB647,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB648,
        ende: 0xB648,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB649,
        ende: 0xB663,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB664,
        ende: 0xB664,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB665,
        ende: 0xB67F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB680,
        ende: 0xB680,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB681,
        ende: 0xB69B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB69C,
        ende: 0xB69C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB69D,
        ende: 0xB6B7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB6B8,
        ende: 0xB6B8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB6B9,
        ende: 0xB6D3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB6D4,
        ende: 0xB6D4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB6D5,
        ende: 0xB6EF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB6F0,
        ende: 0xB6F0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB6F1,
        ende: 0xB70B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB70C,
        ende: 0xB70C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB70D,
        ende: 0xB727,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB728,
        ende: 0xB728,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB729,
        ende: 0xB743,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB744,
        ende: 0xB744,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB745,
        ende: 0xB75F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB760,
        ende: 0xB760,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB761,
        ende: 0xB77B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB77C,
        ende: 0xB77C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB77D,
        ende: 0xB797,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB798,
        ende: 0xB798,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB799,
        ende: 0xB7B3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB7B4,
        ende: 0xB7B4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB7B5,
        ende: 0xB7CF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB7D0,
        ende: 0xB7D0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB7D1,
        ende: 0xB7EB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB7EC,
        ende: 0xB7EC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB7ED,
        ende: 0xB807,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB808,
        ende: 0xB808,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB809,
        ende: 0xB823,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB824,
        ende: 0xB824,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB825,
        ende: 0xB83F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB840,
        ende: 0xB840,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB841,
        ende: 0xB85B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB85C,
        ende: 0xB85C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB85D,
        ende: 0xB877,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB878,
        ende: 0xB878,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB879,
        ende: 0xB893,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB894,
        ende: 0xB894,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB895,
        ende: 0xB8AF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB8B0,
        ende: 0xB8B0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB8B1,
        ende: 0xB8CB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB8CC,
        ende: 0xB8CC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB8CD,
        ende: 0xB8E7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB8E8,
        ende: 0xB8E8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB8E9,
        ende: 0xB903,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB904,
        ende: 0xB904,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB905,
        ende: 0xB91F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB920,
        ende: 0xB920,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB921,
        ende: 0xB93B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB93C,
        ende: 0xB93C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB93D,
        ende: 0xB957,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB958,
        ende: 0xB958,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB959,
        ende: 0xB973,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB974,
        ende: 0xB974,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB975,
        ende: 0xB98F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB990,
        ende: 0xB990,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB991,
        ende: 0xB9AB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB9AC,
        ende: 0xB9AC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB9AD,
        ende: 0xB9C7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB9C8,
        ende: 0xB9C8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB9C9,
        ende: 0xB9E3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xB9E4,
        ende: 0xB9E4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xB9E5,
        ende: 0xB9FF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBA00,
        ende: 0xBA00,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBA01,
        ende: 0xBA1B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBA1C,
        ende: 0xBA1C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBA1D,
        ende: 0xBA37,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBA38,
        ende: 0xBA38,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBA39,
        ende: 0xBA53,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBA54,
        ende: 0xBA54,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBA55,
        ende: 0xBA6F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBA70,
        ende: 0xBA70,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBA71,
        ende: 0xBA8B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBA8C,
        ende: 0xBA8C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBA8D,
        ende: 0xBAA7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBAA8,
        ende: 0xBAA8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBAA9,
        ende: 0xBAC3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBAC4,
        ende: 0xBAC4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBAC5,
        ende: 0xBADF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBAE0,
        ende: 0xBAE0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBAE1,
        ende: 0xBAFB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBAFC,
        ende: 0xBAFC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBAFD,
        ende: 0xBB17,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBB18,
        ende: 0xBB18,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBB19,
        ende: 0xBB33,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBB34,
        ende: 0xBB34,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBB35,
        ende: 0xBB4F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBB50,
        ende: 0xBB50,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBB51,
        ende: 0xBB6B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBB6C,
        ende: 0xBB6C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBB6D,
        ende: 0xBB87,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBB88,
        ende: 0xBB88,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBB89,
        ende: 0xBBA3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBBA4,
        ende: 0xBBA4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBBA5,
        ende: 0xBBBF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBBC0,
        ende: 0xBBC0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBBC1,
        ende: 0xBBDB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBBDC,
        ende: 0xBBDC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBBDD,
        ende: 0xBBF7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBBF8,
        ende: 0xBBF8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBBF9,
        ende: 0xBC13,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBC14,
        ende: 0xBC14,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBC15,
        ende: 0xBC2F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBC30,
        ende: 0xBC30,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBC31,
        ende: 0xBC4B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBC4C,
        ende: 0xBC4C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBC4D,
        ende: 0xBC67,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBC68,
        ende: 0xBC68,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBC69,
        ende: 0xBC83,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBC84,
        ende: 0xBC84,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBC85,
        ende: 0xBC9F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBCA0,
        ende: 0xBCA0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBCA1,
        ende: 0xBCBB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBCBC,
        ende: 0xBCBC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBCBD,
        ende: 0xBCD7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBCD8,
        ende: 0xBCD8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBCD9,
        ende: 0xBCF3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBCF4,
        ende: 0xBCF4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBCF5,
        ende: 0xBD0F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBD10,
        ende: 0xBD10,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBD11,
        ende: 0xBD2B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBD2C,
        ende: 0xBD2C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBD2D,
        ende: 0xBD47,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBD48,
        ende: 0xBD48,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBD49,
        ende: 0xBD63,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBD64,
        ende: 0xBD64,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBD65,
        ende: 0xBD7F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBD80,
        ende: 0xBD80,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBD81,
        ende: 0xBD9B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBD9C,
        ende: 0xBD9C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBD9D,
        ende: 0xBDB7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBDB8,
        ende: 0xBDB8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBDB9,
        ende: 0xBDD3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBDD4,
        ende: 0xBDD4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBDD5,
        ende: 0xBDEF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBDF0,
        ende: 0xBDF0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBDF1,
        ende: 0xBE0B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBE0C,
        ende: 0xBE0C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBE0D,
        ende: 0xBE27,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBE28,
        ende: 0xBE28,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBE29,
        ende: 0xBE43,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBE44,
        ende: 0xBE44,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBE45,
        ende: 0xBE5F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBE60,
        ende: 0xBE60,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBE61,
        ende: 0xBE7B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBE7C,
        ende: 0xBE7C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBE7D,
        ende: 0xBE97,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBE98,
        ende: 0xBE98,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBE99,
        ende: 0xBEB3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBEB4,
        ende: 0xBEB4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBEB5,
        ende: 0xBECF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBED0,
        ende: 0xBED0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBED1,
        ende: 0xBEEB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBEEC,
        ende: 0xBEEC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBEED,
        ende: 0xBF07,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBF08,
        ende: 0xBF08,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBF09,
        ende: 0xBF23,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBF24,
        ende: 0xBF24,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBF25,
        ende: 0xBF3F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBF40,
        ende: 0xBF40,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBF41,
        ende: 0xBF5B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBF5C,
        ende: 0xBF5C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBF5D,
        ende: 0xBF77,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBF78,
        ende: 0xBF78,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBF79,
        ende: 0xBF93,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBF94,
        ende: 0xBF94,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBF95,
        ende: 0xBFAF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBFB0,
        ende: 0xBFB0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBFB1,
        ende: 0xBFCB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBFCC,
        ende: 0xBFCC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBFCD,
        ende: 0xBFE7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xBFE8,
        ende: 0xBFE8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xBFE9,
        ende: 0xC003,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC004,
        ende: 0xC004,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC005,
        ende: 0xC01F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC020,
        ende: 0xC020,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC021,
        ende: 0xC03B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC03C,
        ende: 0xC03C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC03D,
        ende: 0xC057,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC058,
        ende: 0xC058,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC059,
        ende: 0xC073,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC074,
        ende: 0xC074,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC075,
        ende: 0xC08F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC090,
        ende: 0xC090,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC091,
        ende: 0xC0AB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC0AC,
        ende: 0xC0AC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC0AD,
        ende: 0xC0C7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC0C8,
        ende: 0xC0C8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC0C9,
        ende: 0xC0E3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC0E4,
        ende: 0xC0E4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC0E5,
        ende: 0xC0FF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC100,
        ende: 0xC100,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC101,
        ende: 0xC11B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC11C,
        ende: 0xC11C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC11D,
        ende: 0xC137,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC138,
        ende: 0xC138,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC139,
        ende: 0xC153,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC154,
        ende: 0xC154,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC155,
        ende: 0xC16F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC170,
        ende: 0xC170,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC171,
        ende: 0xC18B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC18C,
        ende: 0xC18C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC18D,
        ende: 0xC1A7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC1A8,
        ende: 0xC1A8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC1A9,
        ende: 0xC1C3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC1C4,
        ende: 0xC1C4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC1C5,
        ende: 0xC1DF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC1E0,
        ende: 0xC1E0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC1E1,
        ende: 0xC1FB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC1FC,
        ende: 0xC1FC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC1FD,
        ende: 0xC217,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC218,
        ende: 0xC218,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC219,
        ende: 0xC233,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC234,
        ende: 0xC234,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC235,
        ende: 0xC24F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC250,
        ende: 0xC250,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC251,
        ende: 0xC26B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC26C,
        ende: 0xC26C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC26D,
        ende: 0xC287,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC288,
        ende: 0xC288,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC289,
        ende: 0xC2A3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC2A4,
        ende: 0xC2A4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC2A5,
        ende: 0xC2BF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC2C0,
        ende: 0xC2C0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC2C1,
        ende: 0xC2DB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC2DC,
        ende: 0xC2DC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC2DD,
        ende: 0xC2F7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC2F8,
        ende: 0xC2F8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC2F9,
        ende: 0xC313,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC314,
        ende: 0xC314,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC315,
        ende: 0xC32F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC330,
        ende: 0xC330,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC331,
        ende: 0xC34B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC34C,
        ende: 0xC34C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC34D,
        ende: 0xC367,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC368,
        ende: 0xC368,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC369,
        ende: 0xC383,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC384,
        ende: 0xC384,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC385,
        ende: 0xC39F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC3A0,
        ende: 0xC3A0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC3A1,
        ende: 0xC3BB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC3BC,
        ende: 0xC3BC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC3BD,
        ende: 0xC3D7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC3D8,
        ende: 0xC3D8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC3D9,
        ende: 0xC3F3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC3F4,
        ende: 0xC3F4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC3F5,
        ende: 0xC40F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC410,
        ende: 0xC410,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC411,
        ende: 0xC42B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC42C,
        ende: 0xC42C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC42D,
        ende: 0xC447,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC448,
        ende: 0xC448,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC449,
        ende: 0xC463,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC464,
        ende: 0xC464,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC465,
        ende: 0xC47F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC480,
        ende: 0xC480,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC481,
        ende: 0xC49B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC49C,
        ende: 0xC49C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC49D,
        ende: 0xC4B7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC4B8,
        ende: 0xC4B8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC4B9,
        ende: 0xC4D3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC4D4,
        ende: 0xC4D4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC4D5,
        ende: 0xC4EF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC4F0,
        ende: 0xC4F0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC4F1,
        ende: 0xC50B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC50C,
        ende: 0xC50C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC50D,
        ende: 0xC527,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC528,
        ende: 0xC528,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC529,
        ende: 0xC543,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC544,
        ende: 0xC544,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC545,
        ende: 0xC55F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC560,
        ende: 0xC560,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC561,
        ende: 0xC57B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC57C,
        ende: 0xC57C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC57D,
        ende: 0xC597,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC598,
        ende: 0xC598,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC599,
        ende: 0xC5B3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC5B4,
        ende: 0xC5B4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC5B5,
        ende: 0xC5CF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC5D0,
        ende: 0xC5D0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC5D1,
        ende: 0xC5EB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC5EC,
        ende: 0xC5EC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC5ED,
        ende: 0xC607,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC608,
        ende: 0xC608,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC609,
        ende: 0xC623,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC624,
        ende: 0xC624,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC625,
        ende: 0xC63F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC640,
        ende: 0xC640,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC641,
        ende: 0xC65B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC65C,
        ende: 0xC65C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC65D,
        ende: 0xC677,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC678,
        ende: 0xC678,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC679,
        ende: 0xC693,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC694,
        ende: 0xC694,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC695,
        ende: 0xC6AF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC6B0,
        ende: 0xC6B0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC6B1,
        ende: 0xC6CB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC6CC,
        ende: 0xC6CC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC6CD,
        ende: 0xC6E7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC6E8,
        ende: 0xC6E8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC6E9,
        ende: 0xC703,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC704,
        ende: 0xC704,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC705,
        ende: 0xC71F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC720,
        ende: 0xC720,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC721,
        ende: 0xC73B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC73C,
        ende: 0xC73C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC73D,
        ende: 0xC757,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC758,
        ende: 0xC758,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC759,
        ende: 0xC773,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC774,
        ende: 0xC774,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC775,
        ende: 0xC78F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC790,
        ende: 0xC790,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC791,
        ende: 0xC7AB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC7AC,
        ende: 0xC7AC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC7AD,
        ende: 0xC7C7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC7C8,
        ende: 0xC7C8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC7C9,
        ende: 0xC7E3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC7E4,
        ende: 0xC7E4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC7E5,
        ende: 0xC7FF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC800,
        ende: 0xC800,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC801,
        ende: 0xC81B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC81C,
        ende: 0xC81C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC81D,
        ende: 0xC837,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC838,
        ende: 0xC838,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC839,
        ende: 0xC853,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC854,
        ende: 0xC854,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC855,
        ende: 0xC86F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC870,
        ende: 0xC870,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC871,
        ende: 0xC88B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC88C,
        ende: 0xC88C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC88D,
        ende: 0xC8A7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC8A8,
        ende: 0xC8A8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC8A9,
        ende: 0xC8C3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC8C4,
        ende: 0xC8C4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC8C5,
        ende: 0xC8DF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC8E0,
        ende: 0xC8E0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC8E1,
        ende: 0xC8FB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC8FC,
        ende: 0xC8FC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC8FD,
        ende: 0xC917,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC918,
        ende: 0xC918,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC919,
        ende: 0xC933,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC934,
        ende: 0xC934,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC935,
        ende: 0xC94F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC950,
        ende: 0xC950,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC951,
        ende: 0xC96B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC96C,
        ende: 0xC96C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC96D,
        ende: 0xC987,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC988,
        ende: 0xC988,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC989,
        ende: 0xC9A3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC9A4,
        ende: 0xC9A4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC9A5,
        ende: 0xC9BF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC9C0,
        ende: 0xC9C0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC9C1,
        ende: 0xC9DB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC9DC,
        ende: 0xC9DC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC9DD,
        ende: 0xC9F7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xC9F8,
        ende: 0xC9F8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xC9F9,
        ende: 0xCA13,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCA14,
        ende: 0xCA14,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCA15,
        ende: 0xCA2F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCA30,
        ende: 0xCA30,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCA31,
        ende: 0xCA4B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCA4C,
        ende: 0xCA4C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCA4D,
        ende: 0xCA67,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCA68,
        ende: 0xCA68,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCA69,
        ende: 0xCA83,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCA84,
        ende: 0xCA84,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCA85,
        ende: 0xCA9F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCAA0,
        ende: 0xCAA0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCAA1,
        ende: 0xCABB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCABC,
        ende: 0xCABC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCABD,
        ende: 0xCAD7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCAD8,
        ende: 0xCAD8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCAD9,
        ende: 0xCAF3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCAF4,
        ende: 0xCAF4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCAF5,
        ende: 0xCB0F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCB10,
        ende: 0xCB10,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCB11,
        ende: 0xCB2B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCB2C,
        ende: 0xCB2C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCB2D,
        ende: 0xCB47,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCB48,
        ende: 0xCB48,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCB49,
        ende: 0xCB63,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCB64,
        ende: 0xCB64,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCB65,
        ende: 0xCB7F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCB80,
        ende: 0xCB80,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCB81,
        ende: 0xCB9B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCB9C,
        ende: 0xCB9C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCB9D,
        ende: 0xCBB7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCBB8,
        ende: 0xCBB8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCBB9,
        ende: 0xCBD3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCBD4,
        ende: 0xCBD4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCBD5,
        ende: 0xCBEF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCBF0,
        ende: 0xCBF0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCBF1,
        ende: 0xCC0B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCC0C,
        ende: 0xCC0C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCC0D,
        ende: 0xCC27,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCC28,
        ende: 0xCC28,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCC29,
        ende: 0xCC43,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCC44,
        ende: 0xCC44,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCC45,
        ende: 0xCC5F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCC60,
        ende: 0xCC60,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCC61,
        ende: 0xCC7B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCC7C,
        ende: 0xCC7C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCC7D,
        ende: 0xCC97,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCC98,
        ende: 0xCC98,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCC99,
        ende: 0xCCB3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCCB4,
        ende: 0xCCB4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCCB5,
        ende: 0xCCCF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCCD0,
        ende: 0xCCD0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCCD1,
        ende: 0xCCEB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCCEC,
        ende: 0xCCEC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCCED,
        ende: 0xCD07,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCD08,
        ende: 0xCD08,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCD09,
        ende: 0xCD23,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCD24,
        ende: 0xCD24,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCD25,
        ende: 0xCD3F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCD40,
        ende: 0xCD40,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCD41,
        ende: 0xCD5B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCD5C,
        ende: 0xCD5C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCD5D,
        ende: 0xCD77,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCD78,
        ende: 0xCD78,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCD79,
        ende: 0xCD93,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCD94,
        ende: 0xCD94,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCD95,
        ende: 0xCDAF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCDB0,
        ende: 0xCDB0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCDB1,
        ende: 0xCDCB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCDCC,
        ende: 0xCDCC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCDCD,
        ende: 0xCDE7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCDE8,
        ende: 0xCDE8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCDE9,
        ende: 0xCE03,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCE04,
        ende: 0xCE04,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCE05,
        ende: 0xCE1F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCE20,
        ende: 0xCE20,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCE21,
        ende: 0xCE3B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCE3C,
        ende: 0xCE3C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCE3D,
        ende: 0xCE57,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCE58,
        ende: 0xCE58,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCE59,
        ende: 0xCE73,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCE74,
        ende: 0xCE74,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCE75,
        ende: 0xCE8F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCE90,
        ende: 0xCE90,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCE91,
        ende: 0xCEAB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCEAC,
        ende: 0xCEAC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCEAD,
        ende: 0xCEC7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCEC8,
        ende: 0xCEC8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCEC9,
        ende: 0xCEE3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCEE4,
        ende: 0xCEE4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCEE5,
        ende: 0xCEFF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCF00,
        ende: 0xCF00,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCF01,
        ende: 0xCF1B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCF1C,
        ende: 0xCF1C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCF1D,
        ende: 0xCF37,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCF38,
        ende: 0xCF38,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCF39,
        ende: 0xCF53,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCF54,
        ende: 0xCF54,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCF55,
        ende: 0xCF6F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCF70,
        ende: 0xCF70,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCF71,
        ende: 0xCF8B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCF8C,
        ende: 0xCF8C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCF8D,
        ende: 0xCFA7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCFA8,
        ende: 0xCFA8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCFA9,
        ende: 0xCFC3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCFC4,
        ende: 0xCFC4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCFC5,
        ende: 0xCFDF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCFE0,
        ende: 0xCFE0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCFE1,
        ende: 0xCFFB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xCFFC,
        ende: 0xCFFC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xCFFD,
        ende: 0xD017,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD018,
        ende: 0xD018,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD019,
        ende: 0xD033,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD034,
        ende: 0xD034,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD035,
        ende: 0xD04F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD050,
        ende: 0xD050,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD051,
        ende: 0xD06B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD06C,
        ende: 0xD06C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD06D,
        ende: 0xD087,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD088,
        ende: 0xD088,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD089,
        ende: 0xD0A3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD0A4,
        ende: 0xD0A4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD0A5,
        ende: 0xD0BF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD0C0,
        ende: 0xD0C0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD0C1,
        ende: 0xD0DB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD0DC,
        ende: 0xD0DC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD0DD,
        ende: 0xD0F7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD0F8,
        ende: 0xD0F8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD0F9,
        ende: 0xD113,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD114,
        ende: 0xD114,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD115,
        ende: 0xD12F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD130,
        ende: 0xD130,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD131,
        ende: 0xD14B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD14C,
        ende: 0xD14C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD14D,
        ende: 0xD167,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD168,
        ende: 0xD168,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD169,
        ende: 0xD183,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD184,
        ende: 0xD184,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD185,
        ende: 0xD19F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD1A0,
        ende: 0xD1A0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD1A1,
        ende: 0xD1BB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD1BC,
        ende: 0xD1BC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD1BD,
        ende: 0xD1D7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD1D8,
        ende: 0xD1D8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD1D9,
        ende: 0xD1F3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD1F4,
        ende: 0xD1F4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD1F5,
        ende: 0xD20F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD210,
        ende: 0xD210,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD211,
        ende: 0xD22B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD22C,
        ende: 0xD22C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD22D,
        ende: 0xD247,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD248,
        ende: 0xD248,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD249,
        ende: 0xD263,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD264,
        ende: 0xD264,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD265,
        ende: 0xD27F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD280,
        ende: 0xD280,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD281,
        ende: 0xD29B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD29C,
        ende: 0xD29C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD29D,
        ende: 0xD2B7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD2B8,
        ende: 0xD2B8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD2B9,
        ende: 0xD2D3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD2D4,
        ende: 0xD2D4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD2D5,
        ende: 0xD2EF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD2F0,
        ende: 0xD2F0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD2F1,
        ende: 0xD30B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD30C,
        ende: 0xD30C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD30D,
        ende: 0xD327,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD328,
        ende: 0xD328,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD329,
        ende: 0xD343,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD344,
        ende: 0xD344,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD345,
        ende: 0xD35F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD360,
        ende: 0xD360,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD361,
        ende: 0xD37B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD37C,
        ende: 0xD37C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD37D,
        ende: 0xD397,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD398,
        ende: 0xD398,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD399,
        ende: 0xD3B3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD3B4,
        ende: 0xD3B4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD3B5,
        ende: 0xD3CF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD3D0,
        ende: 0xD3D0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD3D1,
        ende: 0xD3EB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD3EC,
        ende: 0xD3EC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD3ED,
        ende: 0xD407,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD408,
        ende: 0xD408,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD409,
        ende: 0xD423,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD424,
        ende: 0xD424,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD425,
        ende: 0xD43F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD440,
        ende: 0xD440,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD441,
        ende: 0xD45B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD45C,
        ende: 0xD45C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD45D,
        ende: 0xD477,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD478,
        ende: 0xD478,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD479,
        ende: 0xD493,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD494,
        ende: 0xD494,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD495,
        ende: 0xD4AF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD4B0,
        ende: 0xD4B0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD4B1,
        ende: 0xD4CB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD4CC,
        ende: 0xD4CC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD4CD,
        ende: 0xD4E7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD4E8,
        ende: 0xD4E8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD4E9,
        ende: 0xD503,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD504,
        ende: 0xD504,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD505,
        ende: 0xD51F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD520,
        ende: 0xD520,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD521,
        ende: 0xD53B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD53C,
        ende: 0xD53C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD53D,
        ende: 0xD557,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD558,
        ende: 0xD558,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD559,
        ende: 0xD573,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD574,
        ende: 0xD574,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD575,
        ende: 0xD58F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD590,
        ende: 0xD590,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD591,
        ende: 0xD5AB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD5AC,
        ende: 0xD5AC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD5AD,
        ende: 0xD5C7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD5C8,
        ende: 0xD5C8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD5C9,
        ende: 0xD5E3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD5E4,
        ende: 0xD5E4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD5E5,
        ende: 0xD5FF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD600,
        ende: 0xD600,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD601,
        ende: 0xD61B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD61C,
        ende: 0xD61C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD61D,
        ende: 0xD637,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD638,
        ende: 0xD638,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD639,
        ende: 0xD653,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD654,
        ende: 0xD654,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD655,
        ende: 0xD66F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD670,
        ende: 0xD670,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD671,
        ende: 0xD68B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD68C,
        ende: 0xD68C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD68D,
        ende: 0xD6A7,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD6A8,
        ende: 0xD6A8,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD6A9,
        ende: 0xD6C3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD6C4,
        ende: 0xD6C4,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD6C5,
        ende: 0xD6DF,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD6E0,
        ende: 0xD6E0,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD6E1,
        ende: 0xD6FB,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD6FC,
        ende: 0xD6FC,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD6FD,
        ende: 0xD717,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD718,
        ende: 0xD718,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD719,
        ende: 0xD733,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD734,
        ende: 0xD734,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD735,
        ende: 0xD74F,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD750,
        ende: 0xD750,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD751,
        ende: 0xD76B,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD76C,
        ende: 0xD76C,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD76D,
        ende: 0xD787,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD788,
        ende: 0xD788,
        wert: Graphemklasse::Lv,
    },
    Bereich {
        start: 0xD789,
        ende: 0xD7A3,
        wert: Graphemklasse::Lvt,
    },
    Bereich {
        start: 0xD7B0,
        ende: 0xD7C6,
        wert: Graphemklasse::V,
    },
    Bereich {
        start: 0xD7CB,
        ende: 0xD7FB,
        wert: Graphemklasse::T,
    },
    Bereich {
        start: 0xFB1E,
        ende: 0xFB1E,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xFE00,
        ende: 0xFE0F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xFE20,
        ende: 0xFE2F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xFEFF,
        ende: 0xFEFF,
        wert: Graphemklasse::Control,
    },
    Bereich {
        start: 0xFF9E,
        ende: 0xFF9F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xFFF0,
        ende: 0xFFFB,
        wert: Graphemklasse::Control,
    },
    Bereich {
        start: 0x101FD,
        ende: 0x101FD,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x102E0,
        ende: 0x102E0,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x10376,
        ende: 0x1037A,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x10A01,
        ende: 0x10A03,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x10A05,
        ende: 0x10A06,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x10A0C,
        ende: 0x10A0F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x10A38,
        ende: 0x10A3A,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x10A3F,
        ende: 0x10A3F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x10AE5,
        ende: 0x10AE6,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x10D24,
        ende: 0x10D27,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x10EAB,
        ende: 0x10EAC,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x10EFD,
        ende: 0x10EFF,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x10F46,
        ende: 0x10F50,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x10F82,
        ende: 0x10F85,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11000,
        ende: 0x11000,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11001,
        ende: 0x11001,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11002,
        ende: 0x11002,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11038,
        ende: 0x11046,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11070,
        ende: 0x11070,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11073,
        ende: 0x11074,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1107F,
        ende: 0x11081,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11082,
        ende: 0x11082,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x110B0,
        ende: 0x110B2,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x110B3,
        ende: 0x110B6,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x110B7,
        ende: 0x110B8,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x110B9,
        ende: 0x110BA,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x110BD,
        ende: 0x110BD,
        wert: Graphemklasse::Prepend,
    },
    Bereich {
        start: 0x110C2,
        ende: 0x110C2,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x110CD,
        ende: 0x110CD,
        wert: Graphemklasse::Prepend,
    },
    Bereich {
        start: 0x11100,
        ende: 0x11102,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11127,
        ende: 0x1112B,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1112C,
        ende: 0x1112C,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1112D,
        ende: 0x11134,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11145,
        ende: 0x11146,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11173,
        ende: 0x11173,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11180,
        ende: 0x11181,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11182,
        ende: 0x11182,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x111B3,
        ende: 0x111B5,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x111B6,
        ende: 0x111BE,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x111BF,
        ende: 0x111C0,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x111C2,
        ende: 0x111C3,
        wert: Graphemklasse::Prepend,
    },
    Bereich {
        start: 0x111C9,
        ende: 0x111CC,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x111CE,
        ende: 0x111CE,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x111CF,
        ende: 0x111CF,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1122C,
        ende: 0x1122E,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1122F,
        ende: 0x11231,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11232,
        ende: 0x11233,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11234,
        ende: 0x11234,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11235,
        ende: 0x11235,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11236,
        ende: 0x11237,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1123E,
        ende: 0x1123E,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11241,
        ende: 0x11241,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x112DF,
        ende: 0x112DF,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x112E0,
        ende: 0x112E2,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x112E3,
        ende: 0x112EA,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11300,
        ende: 0x11301,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11302,
        ende: 0x11303,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1133B,
        ende: 0x1133C,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1133E,
        ende: 0x1133E,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1133F,
        ende: 0x1133F,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11340,
        ende: 0x11340,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11341,
        ende: 0x11344,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11347,
        ende: 0x11348,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1134B,
        ende: 0x1134D,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11357,
        ende: 0x11357,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11362,
        ende: 0x11363,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11366,
        ende: 0x1136C,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11370,
        ende: 0x11374,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11435,
        ende: 0x11437,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11438,
        ende: 0x1143F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11440,
        ende: 0x11441,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11442,
        ende: 0x11444,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11445,
        ende: 0x11445,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11446,
        ende: 0x11446,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1145E,
        ende: 0x1145E,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x114B0,
        ende: 0x114B0,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x114B1,
        ende: 0x114B2,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x114B3,
        ende: 0x114B8,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x114B9,
        ende: 0x114B9,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x114BA,
        ende: 0x114BA,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x114BB,
        ende: 0x114BC,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x114BD,
        ende: 0x114BD,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x114BE,
        ende: 0x114BE,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x114BF,
        ende: 0x114C0,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x114C1,
        ende: 0x114C1,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x114C2,
        ende: 0x114C3,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x115AF,
        ende: 0x115AF,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x115B0,
        ende: 0x115B1,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x115B2,
        ende: 0x115B5,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x115B8,
        ende: 0x115BB,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x115BC,
        ende: 0x115BD,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x115BE,
        ende: 0x115BE,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x115BF,
        ende: 0x115C0,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x115DC,
        ende: 0x115DD,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11630,
        ende: 0x11632,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11633,
        ende: 0x1163A,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1163B,
        ende: 0x1163C,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1163D,
        ende: 0x1163D,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1163E,
        ende: 0x1163E,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1163F,
        ende: 0x11640,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x116AB,
        ende: 0x116AB,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x116AC,
        ende: 0x116AC,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x116AD,
        ende: 0x116AD,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x116AE,
        ende: 0x116AF,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x116B0,
        ende: 0x116B5,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x116B6,
        ende: 0x116B6,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x116B7,
        ende: 0x116B7,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1171D,
        ende: 0x1171F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11722,
        ende: 0x11725,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11726,
        ende: 0x11726,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11727,
        ende: 0x1172B,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1182C,
        ende: 0x1182E,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1182F,
        ende: 0x11837,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11838,
        ende: 0x11838,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11839,
        ende: 0x1183A,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11930,
        ende: 0x11930,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11931,
        ende: 0x11935,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11937,
        ende: 0x11938,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1193B,
        ende: 0x1193C,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1193D,
        ende: 0x1193D,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1193E,
        ende: 0x1193E,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1193F,
        ende: 0x1193F,
        wert: Graphemklasse::Prepend,
    },
    Bereich {
        start: 0x11940,
        ende: 0x11940,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11941,
        ende: 0x11941,
        wert: Graphemklasse::Prepend,
    },
    Bereich {
        start: 0x11942,
        ende: 0x11942,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11943,
        ende: 0x11943,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x119D1,
        ende: 0x119D3,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x119D4,
        ende: 0x119D7,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x119DA,
        ende: 0x119DB,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x119DC,
        ende: 0x119DF,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x119E0,
        ende: 0x119E0,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x119E4,
        ende: 0x119E4,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11A01,
        ende: 0x11A0A,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11A33,
        ende: 0x11A38,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11A39,
        ende: 0x11A39,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11A3A,
        ende: 0x11A3A,
        wert: Graphemklasse::Prepend,
    },
    Bereich {
        start: 0x11A3B,
        ende: 0x11A3E,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11A47,
        ende: 0x11A47,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11A51,
        ende: 0x11A56,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11A57,
        ende: 0x11A58,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11A59,
        ende: 0x11A5B,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11A84,
        ende: 0x11A89,
        wert: Graphemklasse::Prepend,
    },
    Bereich {
        start: 0x11A8A,
        ende: 0x11A96,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11A97,
        ende: 0x11A97,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11A98,
        ende: 0x11A99,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11C2F,
        ende: 0x11C2F,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11C30,
        ende: 0x11C36,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11C38,
        ende: 0x11C3D,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11C3E,
        ende: 0x11C3E,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11C3F,
        ende: 0x11C3F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11C92,
        ende: 0x11CA7,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11CA9,
        ende: 0x11CA9,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11CAA,
        ende: 0x11CB0,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11CB1,
        ende: 0x11CB1,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11CB2,
        ende: 0x11CB3,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11CB4,
        ende: 0x11CB4,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11CB5,
        ende: 0x11CB6,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11D31,
        ende: 0x11D36,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11D3A,
        ende: 0x11D3A,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11D3C,
        ende: 0x11D3D,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11D3F,
        ende: 0x11D45,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11D46,
        ende: 0x11D46,
        wert: Graphemklasse::Prepend,
    },
    Bereich {
        start: 0x11D47,
        ende: 0x11D47,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11D8A,
        ende: 0x11D8E,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11D90,
        ende: 0x11D91,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11D93,
        ende: 0x11D94,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11D95,
        ende: 0x11D95,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11D96,
        ende: 0x11D96,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11D97,
        ende: 0x11D97,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11EF3,
        ende: 0x11EF4,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11EF5,
        ende: 0x11EF6,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11F00,
        ende: 0x11F01,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11F02,
        ende: 0x11F02,
        wert: Graphemklasse::Prepend,
    },
    Bereich {
        start: 0x11F03,
        ende: 0x11F03,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11F34,
        ende: 0x11F35,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11F36,
        ende: 0x11F3A,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11F3E,
        ende: 0x11F3F,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11F40,
        ende: 0x11F40,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x11F41,
        ende: 0x11F41,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x11F42,
        ende: 0x11F42,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x13430,
        ende: 0x1343F,
        wert: Graphemklasse::Control,
    },
    Bereich {
        start: 0x13440,
        ende: 0x13440,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x13447,
        ende: 0x13455,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x16AF0,
        ende: 0x16AF4,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x16B30,
        ende: 0x16B36,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x16F4F,
        ende: 0x16F4F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x16F51,
        ende: 0x16F87,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x16F8F,
        ende: 0x16F92,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x16FE4,
        ende: 0x16FE4,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x16FF0,
        ende: 0x16FF1,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1BC9D,
        ende: 0x1BC9E,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1BCA0,
        ende: 0x1BCA3,
        wert: Graphemklasse::Control,
    },
    Bereich {
        start: 0x1CF00,
        ende: 0x1CF2D,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1CF30,
        ende: 0x1CF46,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1D165,
        ende: 0x1D165,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1D166,
        ende: 0x1D166,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1D167,
        ende: 0x1D169,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1D16D,
        ende: 0x1D16D,
        wert: Graphemklasse::SpacingMark,
    },
    Bereich {
        start: 0x1D16E,
        ende: 0x1D172,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1D173,
        ende: 0x1D17A,
        wert: Graphemklasse::Control,
    },
    Bereich {
        start: 0x1D17B,
        ende: 0x1D182,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1D185,
        ende: 0x1D18B,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1D1AA,
        ende: 0x1D1AD,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1D242,
        ende: 0x1D244,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1DA00,
        ende: 0x1DA36,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1DA3B,
        ende: 0x1DA6C,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1DA75,
        ende: 0x1DA75,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1DA84,
        ende: 0x1DA84,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1DA9B,
        ende: 0x1DA9F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1DAA1,
        ende: 0x1DAAF,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1E000,
        ende: 0x1E006,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1E008,
        ende: 0x1E018,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1E01B,
        ende: 0x1E021,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1E023,
        ende: 0x1E024,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1E026,
        ende: 0x1E02A,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1E08F,
        ende: 0x1E08F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1E130,
        ende: 0x1E136,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1E2AE,
        ende: 0x1E2AE,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1E2EC,
        ende: 0x1E2EF,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1E4EC,
        ende: 0x1E4EF,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1E8D0,
        ende: 0x1E8D6,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1E944,
        ende: 0x1E94A,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0x1F1E6,
        ende: 0x1F1FF,
        wert: Graphemklasse::RegionalIndicator,
    },
    Bereich {
        start: 0x1F3FB,
        ende: 0x1F3FF,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xE0000,
        ende: 0xE001F,
        wert: Graphemklasse::Control,
    },
    Bereich {
        start: 0xE0020,
        ende: 0xE007F,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xE0080,
        ende: 0xE00FF,
        wert: Graphemklasse::Control,
    },
    Bereich {
        start: 0xE0100,
        ende: 0xE01EF,
        wert: Graphemklasse::Extend,
    },
    Bereich {
        start: 0xE01F0,
        ende: 0xE0FFF,
        wert: Graphemklasse::Control,
    },
];

/// Extended_Pictographic-Bereiche aus emoji-data.txt.
static EP_BEREICHE: [Bereich<()>; 78] = [
    Bereich {
        start: 0x00A9,
        ende: 0x00A9,
        wert: (),
    },
    Bereich {
        start: 0x00AE,
        ende: 0x00AE,
        wert: (),
    },
    Bereich {
        start: 0x203C,
        ende: 0x203C,
        wert: (),
    },
    Bereich {
        start: 0x2049,
        ende: 0x2049,
        wert: (),
    },
    Bereich {
        start: 0x2122,
        ende: 0x2122,
        wert: (),
    },
    Bereich {
        start: 0x2139,
        ende: 0x2139,
        wert: (),
    },
    Bereich {
        start: 0x2194,
        ende: 0x2199,
        wert: (),
    },
    Bereich {
        start: 0x21A9,
        ende: 0x21AA,
        wert: (),
    },
    Bereich {
        start: 0x231A,
        ende: 0x231B,
        wert: (),
    },
    Bereich {
        start: 0x2328,
        ende: 0x2328,
        wert: (),
    },
    Bereich {
        start: 0x2388,
        ende: 0x2388,
        wert: (),
    },
    Bereich {
        start: 0x23CF,
        ende: 0x23CF,
        wert: (),
    },
    Bereich {
        start: 0x23E9,
        ende: 0x23F3,
        wert: (),
    },
    Bereich {
        start: 0x23F8,
        ende: 0x23FA,
        wert: (),
    },
    Bereich {
        start: 0x24C2,
        ende: 0x24C2,
        wert: (),
    },
    Bereich {
        start: 0x25AA,
        ende: 0x25AB,
        wert: (),
    },
    Bereich {
        start: 0x25B6,
        ende: 0x25B6,
        wert: (),
    },
    Bereich {
        start: 0x25C0,
        ende: 0x25C0,
        wert: (),
    },
    Bereich {
        start: 0x25FB,
        ende: 0x25FE,
        wert: (),
    },
    Bereich {
        start: 0x2600,
        ende: 0x2605,
        wert: (),
    },
    Bereich {
        start: 0x2607,
        ende: 0x2612,
        wert: (),
    },
    Bereich {
        start: 0x2614,
        ende: 0x2685,
        wert: (),
    },
    Bereich {
        start: 0x2690,
        ende: 0x2705,
        wert: (),
    },
    Bereich {
        start: 0x2708,
        ende: 0x2712,
        wert: (),
    },
    Bereich {
        start: 0x2714,
        ende: 0x2714,
        wert: (),
    },
    Bereich {
        start: 0x2716,
        ende: 0x2716,
        wert: (),
    },
    Bereich {
        start: 0x271D,
        ende: 0x271D,
        wert: (),
    },
    Bereich {
        start: 0x2721,
        ende: 0x2721,
        wert: (),
    },
    Bereich {
        start: 0x2728,
        ende: 0x2728,
        wert: (),
    },
    Bereich {
        start: 0x2733,
        ende: 0x2734,
        wert: (),
    },
    Bereich {
        start: 0x2744,
        ende: 0x2744,
        wert: (),
    },
    Bereich {
        start: 0x2747,
        ende: 0x2747,
        wert: (),
    },
    Bereich {
        start: 0x274C,
        ende: 0x274C,
        wert: (),
    },
    Bereich {
        start: 0x274E,
        ende: 0x274E,
        wert: (),
    },
    Bereich {
        start: 0x2753,
        ende: 0x2755,
        wert: (),
    },
    Bereich {
        start: 0x2757,
        ende: 0x2757,
        wert: (),
    },
    Bereich {
        start: 0x2763,
        ende: 0x2767,
        wert: (),
    },
    Bereich {
        start: 0x2795,
        ende: 0x2797,
        wert: (),
    },
    Bereich {
        start: 0x27A1,
        ende: 0x27A1,
        wert: (),
    },
    Bereich {
        start: 0x27B0,
        ende: 0x27B0,
        wert: (),
    },
    Bereich {
        start: 0x27BF,
        ende: 0x27BF,
        wert: (),
    },
    Bereich {
        start: 0x2934,
        ende: 0x2935,
        wert: (),
    },
    Bereich {
        start: 0x2B05,
        ende: 0x2B07,
        wert: (),
    },
    Bereich {
        start: 0x2B1B,
        ende: 0x2B1C,
        wert: (),
    },
    Bereich {
        start: 0x2B50,
        ende: 0x2B50,
        wert: (),
    },
    Bereich {
        start: 0x2B55,
        ende: 0x2B55,
        wert: (),
    },
    Bereich {
        start: 0x3030,
        ende: 0x3030,
        wert: (),
    },
    Bereich {
        start: 0x303D,
        ende: 0x303D,
        wert: (),
    },
    Bereich {
        start: 0x3297,
        ende: 0x3297,
        wert: (),
    },
    Bereich {
        start: 0x3299,
        ende: 0x3299,
        wert: (),
    },
    Bereich {
        start: 0x1F000,
        ende: 0x1F0FF,
        wert: (),
    },
    Bereich {
        start: 0x1F10D,
        ende: 0x1F10F,
        wert: (),
    },
    Bereich {
        start: 0x1F12F,
        ende: 0x1F12F,
        wert: (),
    },
    Bereich {
        start: 0x1F16C,
        ende: 0x1F171,
        wert: (),
    },
    Bereich {
        start: 0x1F17E,
        ende: 0x1F17F,
        wert: (),
    },
    Bereich {
        start: 0x1F18E,
        ende: 0x1F18E,
        wert: (),
    },
    Bereich {
        start: 0x1F191,
        ende: 0x1F19A,
        wert: (),
    },
    Bereich {
        start: 0x1F1AD,
        ende: 0x1F1E5,
        wert: (),
    },
    Bereich {
        start: 0x1F201,
        ende: 0x1F20F,
        wert: (),
    },
    Bereich {
        start: 0x1F21A,
        ende: 0x1F21A,
        wert: (),
    },
    Bereich {
        start: 0x1F22F,
        ende: 0x1F22F,
        wert: (),
    },
    Bereich {
        start: 0x1F232,
        ende: 0x1F23A,
        wert: (),
    },
    Bereich {
        start: 0x1F23C,
        ende: 0x1F23F,
        wert: (),
    },
    Bereich {
        start: 0x1F249,
        ende: 0x1F3FA,
        wert: (),
    },
    Bereich {
        start: 0x1F400,
        ende: 0x1F53D,
        wert: (),
    },
    Bereich {
        start: 0x1F546,
        ende: 0x1F64F,
        wert: (),
    },
    Bereich {
        start: 0x1F680,
        ende: 0x1F6FF,
        wert: (),
    },
    Bereich {
        start: 0x1F774,
        ende: 0x1F77F,
        wert: (),
    },
    Bereich {
        start: 0x1F7D5,
        ende: 0x1F7FF,
        wert: (),
    },
    Bereich {
        start: 0x1F80C,
        ende: 0x1F80F,
        wert: (),
    },
    Bereich {
        start: 0x1F848,
        ende: 0x1F84F,
        wert: (),
    },
    Bereich {
        start: 0x1F85A,
        ende: 0x1F85F,
        wert: (),
    },
    Bereich {
        start: 0x1F888,
        ende: 0x1F88F,
        wert: (),
    },
    Bereich {
        start: 0x1F8AE,
        ende: 0x1F8FF,
        wert: (),
    },
    Bereich {
        start: 0x1F90C,
        ende: 0x1F93A,
        wert: (),
    },
    Bereich {
        start: 0x1F93C,
        ende: 0x1F945,
        wert: (),
    },
    Bereich {
        start: 0x1F947,
        ende: 0x1FAFF,
        wert: (),
    },
    Bereich {
        start: 0x1FC00,
        ende: 0x1FFFD,
        wert: (),
    },
];

/// Indic_Conjunct_Break-Bereiche aus DerivedCoreProperties.txt.
static INCB_BEREICHE: [Bereich<Incb>; 201] = [
    Bereich {
        start: 0x0300,
        ende: 0x034E,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0350,
        ende: 0x036F,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0483,
        ende: 0x0487,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0591,
        ende: 0x05BD,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x05BF,
        ende: 0x05BF,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x05C1,
        ende: 0x05C2,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x05C4,
        ende: 0x05C5,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x05C7,
        ende: 0x05C7,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0610,
        ende: 0x061A,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x064B,
        ende: 0x065F,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0670,
        ende: 0x0670,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x06D6,
        ende: 0x06DC,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x06DF,
        ende: 0x06E4,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x06E7,
        ende: 0x06E8,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x06EA,
        ende: 0x06ED,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0711,
        ende: 0x0711,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0730,
        ende: 0x074A,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x07EB,
        ende: 0x07F3,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x07FD,
        ende: 0x07FD,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0816,
        ende: 0x0819,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x081B,
        ende: 0x0823,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0825,
        ende: 0x0827,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0829,
        ende: 0x082D,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0859,
        ende: 0x085B,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0898,
        ende: 0x089F,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x08CA,
        ende: 0x08E1,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x08E3,
        ende: 0x08FF,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0915,
        ende: 0x0939,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x093C,
        ende: 0x093C,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x094D,
        ende: 0x094D,
        wert: Incb::Linker,
    },
    Bereich {
        start: 0x0951,
        ende: 0x0954,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0958,
        ende: 0x095F,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x0978,
        ende: 0x097F,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x0995,
        ende: 0x09A8,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x09AA,
        ende: 0x09B0,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x09B2,
        ende: 0x09B2,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x09B6,
        ende: 0x09B9,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x09BC,
        ende: 0x09BC,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x09CD,
        ende: 0x09CD,
        wert: Incb::Linker,
    },
    Bereich {
        start: 0x09DC,
        ende: 0x09DD,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x09DF,
        ende: 0x09DF,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x09F0,
        ende: 0x09F1,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x09FE,
        ende: 0x09FE,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0A3C,
        ende: 0x0A3C,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0A95,
        ende: 0x0AA8,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x0AAA,
        ende: 0x0AB0,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x0AB2,
        ende: 0x0AB3,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x0AB5,
        ende: 0x0AB9,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x0ABC,
        ende: 0x0ABC,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0ACD,
        ende: 0x0ACD,
        wert: Incb::Linker,
    },
    Bereich {
        start: 0x0AF9,
        ende: 0x0AF9,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x0B15,
        ende: 0x0B28,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x0B2A,
        ende: 0x0B30,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x0B32,
        ende: 0x0B33,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x0B35,
        ende: 0x0B39,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x0B3C,
        ende: 0x0B3C,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0B4D,
        ende: 0x0B4D,
        wert: Incb::Linker,
    },
    Bereich {
        start: 0x0B5C,
        ende: 0x0B5D,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x0B5F,
        ende: 0x0B5F,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x0B71,
        ende: 0x0B71,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x0C15,
        ende: 0x0C28,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x0C2A,
        ende: 0x0C39,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x0C3C,
        ende: 0x0C3C,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0C4D,
        ende: 0x0C4D,
        wert: Incb::Linker,
    },
    Bereich {
        start: 0x0C55,
        ende: 0x0C56,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0C58,
        ende: 0x0C5A,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x0CBC,
        ende: 0x0CBC,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0D15,
        ende: 0x0D3A,
        wert: Incb::Consonant,
    },
    Bereich {
        start: 0x0D3B,
        ende: 0x0D3C,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0D4D,
        ende: 0x0D4D,
        wert: Incb::Linker,
    },
    Bereich {
        start: 0x0E38,
        ende: 0x0E3A,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0E48,
        ende: 0x0E4B,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0EB8,
        ende: 0x0EBA,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0EC8,
        ende: 0x0ECB,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0F18,
        ende: 0x0F19,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0F35,
        ende: 0x0F35,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0F37,
        ende: 0x0F37,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0F39,
        ende: 0x0F39,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0F71,
        ende: 0x0F72,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0F74,
        ende: 0x0F74,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0F7A,
        ende: 0x0F7D,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0F80,
        ende: 0x0F80,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0F82,
        ende: 0x0F84,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0F86,
        ende: 0x0F87,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x0FC6,
        ende: 0x0FC6,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1037,
        ende: 0x1037,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1039,
        ende: 0x103A,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x108D,
        ende: 0x108D,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x135D,
        ende: 0x135F,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1714,
        ende: 0x1714,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x17D2,
        ende: 0x17D2,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x17DD,
        ende: 0x17DD,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x18A9,
        ende: 0x18A9,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1939,
        ende: 0x193B,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1A17,
        ende: 0x1A18,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1A60,
        ende: 0x1A60,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1A75,
        ende: 0x1A7C,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1A7F,
        ende: 0x1A7F,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1AB0,
        ende: 0x1ABD,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1ABF,
        ende: 0x1ACE,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1B34,
        ende: 0x1B34,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1B6B,
        ende: 0x1B73,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1BAB,
        ende: 0x1BAB,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1BE6,
        ende: 0x1BE6,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1C37,
        ende: 0x1C37,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1CD0,
        ende: 0x1CD2,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1CD4,
        ende: 0x1CE0,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1CE2,
        ende: 0x1CE8,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1CED,
        ende: 0x1CED,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1CF4,
        ende: 0x1CF4,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1CF8,
        ende: 0x1CF9,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1DC0,
        ende: 0x1DFF,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x200D,
        ende: 0x200D,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x20D0,
        ende: 0x20DC,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x20E1,
        ende: 0x20E1,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x20E5,
        ende: 0x20F0,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x2CEF,
        ende: 0x2CF1,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x2D7F,
        ende: 0x2D7F,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x2DE0,
        ende: 0x2DFF,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x302A,
        ende: 0x302F,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x3099,
        ende: 0x309A,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0xA66F,
        ende: 0xA66F,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0xA674,
        ende: 0xA67D,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0xA69E,
        ende: 0xA69F,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0xA6F0,
        ende: 0xA6F1,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0xA82C,
        ende: 0xA82C,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0xA8E0,
        ende: 0xA8F1,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0xA92B,
        ende: 0xA92D,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0xA9B3,
        ende: 0xA9B3,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0xAAB0,
        ende: 0xAAB0,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0xAAB2,
        ende: 0xAAB4,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0xAAB7,
        ende: 0xAAB8,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0xAABE,
        ende: 0xAABF,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0xAAC1,
        ende: 0xAAC1,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0xAAF6,
        ende: 0xAAF6,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0xABED,
        ende: 0xABED,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0xFB1E,
        ende: 0xFB1E,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0xFE20,
        ende: 0xFE2F,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x101FD,
        ende: 0x101FD,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x102E0,
        ende: 0x102E0,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x10376,
        ende: 0x1037A,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x10A0D,
        ende: 0x10A0D,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x10A0F,
        ende: 0x10A0F,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x10A38,
        ende: 0x10A3A,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x10A3F,
        ende: 0x10A3F,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x10AE5,
        ende: 0x10AE6,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x10D24,
        ende: 0x10D27,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x10EAB,
        ende: 0x10EAC,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x10EFD,
        ende: 0x10EFF,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x10F46,
        ende: 0x10F50,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x10F82,
        ende: 0x10F85,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x11070,
        ende: 0x11070,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1107F,
        ende: 0x1107F,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x110BA,
        ende: 0x110BA,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x11100,
        ende: 0x11102,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x11133,
        ende: 0x11134,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x11173,
        ende: 0x11173,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x111CA,
        ende: 0x111CA,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x11236,
        ende: 0x11236,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x112E9,
        ende: 0x112EA,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1133B,
        ende: 0x1133C,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x11366,
        ende: 0x1136C,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x11370,
        ende: 0x11374,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x11446,
        ende: 0x11446,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1145E,
        ende: 0x1145E,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x114C3,
        ende: 0x114C3,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x115C0,
        ende: 0x115C0,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x116B7,
        ende: 0x116B7,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1172B,
        ende: 0x1172B,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1183A,
        ende: 0x1183A,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1193E,
        ende: 0x1193E,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x11943,
        ende: 0x11943,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x11A34,
        ende: 0x11A34,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x11A47,
        ende: 0x11A47,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x11A99,
        ende: 0x11A99,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x11D42,
        ende: 0x11D42,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x11D44,
        ende: 0x11D45,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x11D97,
        ende: 0x11D97,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x11F42,
        ende: 0x11F42,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x16AF0,
        ende: 0x16AF4,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x16B30,
        ende: 0x16B36,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1BC9E,
        ende: 0x1BC9E,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1D165,
        ende: 0x1D165,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1D167,
        ende: 0x1D169,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1D16E,
        ende: 0x1D172,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1D17B,
        ende: 0x1D182,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1D185,
        ende: 0x1D18B,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1D1AA,
        ende: 0x1D1AD,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1D242,
        ende: 0x1D244,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1E000,
        ende: 0x1E006,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1E008,
        ende: 0x1E018,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1E01B,
        ende: 0x1E021,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1E023,
        ende: 0x1E024,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1E026,
        ende: 0x1E02A,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1E08F,
        ende: 0x1E08F,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1E130,
        ende: 0x1E136,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1E2AE,
        ende: 0x1E2AE,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1E2EC,
        ende: 0x1E2EF,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1E4EC,
        ende: 0x1E4EF,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1E8D0,
        ende: 0x1E8D6,
        wert: Incb::Extend,
    },
    Bereich {
        start: 0x1E944,
        ende: 0x1E94A,
        wert: Incb::Extend,
    },
];

/// Liefert die Grapheme_Cluster_Break-Klasse eines Zeichens.
pub fn graphem_klasse(ch: char) -> Graphemklasse {
    bereich_suche(&GCB_BEREICHE, ch as u32).unwrap_or(Graphemklasse::Other)
}

/// Ist das Zeichen Extended_Pictographic?
pub fn ist_extended_pictographic(ch: char) -> bool {
    bereich_suche(&EP_BEREICHE, ch as u32).is_some()
}

/// Liefert die Indic_Conjunct_Break-Eigenschaft eines Zeichens.
pub fn incb(ch: char) -> Incb {
    bereich_suche(&INCB_BEREICHE, ch as u32).unwrap_or(Incb::None)
}
