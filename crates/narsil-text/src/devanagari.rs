//! Devanagari-Grundshaping (Aufgabe I4).
//!
//! Drei Bausteine, alle ohne Fremd-Crate und ohne unsafe:
//!
//! 1. [`silben`] zerlegt Text in Aksara-Cluster (Konsonant mit Halant-
//!    Ketten, Nukta, Matras und Zeichen) und liefert fuer jede Silbe die
//!    Anzeige-Reihenfolge:
//!    * Reph (U+0930 + Halant am Silbenanfang) wandert ans Ende,
//!    * prae-basische Vokalzeichen (Indic_Positional_Category=Left)
//!      wandern vor den Konsonanten.
//! 2. [`anzeige`] setzt das fuer den ganzen Text zusammen.
//! 3. [`forme_glyphen`] bildet Zeichen ueber das cmap der Schrift auf
//!    Glyphen ab und wendet die Devanagari-GSUB-Merkmale der Schrift an
//!    (Ligaturen vom Typ 1/2/4, u. a. akhn, rphf, half, cjct, pres).
//!    Fehlt die Schrift oder ihre GSUB-Daten, kommt `None` zurueck —
//!    die Anzeige-Reihenfolge aus 1./2. bleibt trotzdem nutzbar.
//!
//! Quellen: Unicodedaten aus /usr/share/unicode
//! (IndicPositionalCategory.txt, IndicSyllabicCategory.txt, Blocks.txt)
//! und die GSUB-Tabelle der jeweiligen Schrift (OpenType, frei lesbar
//! ueber `TrueTypeFont::table(b"GSUB")`).
//!
//! Grenzen: Es ist ausdruecklich ein GRUND-Shaping. Lookup-Typ 6
//! (kontextuelle Ketten) und die volle Feature-Reihenfolge eines
//! OpenType-Satzers fehlen; Ligaturen, die nur ueber Typ 6 entstehen,
//! bleiben aus. Die Reihenfolge der Merkmale ist die uebliche
//! Indic-Reihenfolge (nukt, rphf, akhn, blwf, half, cjct, vatu, pres,
//! abvs, blws, psts, haln). Keine Panics, lineare Laufzeit.

use crate::truetype::TrueTypeFont;
use crate::unicode_daten::devanagari_zeichen::{
    Silbenrolle, Vokallage, deva_zeichen, ist_devanagari,
};

/// Halant/Virama U+094D.
pub const HALANT: char = '\u{094D}';

/// RA U+0930 (Ausgangszeichen des Reph).
pub const RA: char = '\u{0930}';

/// Merkmale in der Reihenfolge, in der sie angewendet werden.
const MERKMALE: [&[u8; 4]; 12] = [
    b"nukt", b"rphf", b"akhn", b"blwf", b"half", b"cjct", b"vatu", b"pres", b"abvs", b"blws",
    b"psts", b"haln",
];

/// Eine Silbe (Aksara) mit logischer und Anzeige-Reihenfolge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Silbe {
    /// Die Zeichen in logischer Reihenfolge.
    pub logisch: String,
    /// Die Zeichen in Anzeige-Reihenfolge.
    pub anzeige: String,
    /// Beginnt die Silbe mit Reph (RA + Halant)?
    pub hat_reph: bool,
}

/// Ist das Zeichen ein Devanagari-Konsonant oder ein selbststaendiger
/// Vokal (Silbenanfang)?
fn ist_silbenkern(ch: char) -> bool {
    matches!(
        deva_zeichen(ch).1,
        Silbenrolle::Konsonant | Silbenrolle::Vokal
    )
}

fn ist_halant(ch: char) -> bool {
    deva_zeichen(ch).1 == Silbenrolle::Halant
}

fn ist_matra(ch: char) -> bool {
    deva_zeichen(ch).1 == Silbenrolle::Matra
}

fn ist_zeichen(ch: char) -> bool {
    deva_zeichen(ch).1 == Silbenrolle::Zeichen
}

/// Zerlegt Text in Silben (Aksara) mit Anzeige-Reihenfolge.
///
/// Nicht-Devanagari-Zeichen werden zu je einer Ein-Zeichen-Silbe; sie
/// verbinden keine Konsonanten.
pub fn silben(text: &str) -> Vec<Silbe> {
    let zeichen: Vec<char> = text.chars().collect();
    let mut ergebnis = Vec::new();
    let mut i = 0;
    while i < zeichen.len() {
        if !ist_devanagari(zeichen[i]) || !ist_silbenkern(zeichen[i]) {
            ergebnis.push(Silbe {
                logisch: zeichen[i].to_string(),
                anzeige: zeichen[i].to_string(),
                hat_reph: false,
            });
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        loop {
            if i < zeichen.len() && ist_zeichen(zeichen[i]) {
                i += 1;
                continue;
            }
            if i + 1 < zeichen.len() && ist_halant(zeichen[i]) && ist_silbenkern(zeichen[i + 1]) {
                i += 2;
                continue;
            }
            break;
        }
        while i < zeichen.len()
            && (ist_matra(zeichen[i]) || ist_zeichen(zeichen[i]) || ist_halant(zeichen[i]))
        {
            i += 1;
        }
        let logisch: Vec<char> = zeichen[start..i].to_vec();
        ergebnis.push(silbe_aus(&logisch));
    }
    ergebnis
}

fn silbe_aus(logisch: &[char]) -> Silbe {
    let hat_reph = logisch.len() >= 3 && logisch[0] == RA && ist_halant(logisch[1]);
    let (reph, rest) = if hat_reph {
        (&logisch[0..2], &logisch[2..])
    } else {
        (&logisch[0..0], logisch)
    };
    let mut vorne = Vec::new();
    let mut hinten = Vec::new();
    for &c in rest {
        if deva_zeichen(c).0 == Vokallage::Links {
            vorne.push(c);
        } else {
            hinten.push(c);
        }
    }
    let mut anzeige: Vec<char> = Vec::with_capacity(logisch.len());
    anzeige.extend(vorne);
    anzeige.extend(hinten);
    anzeige.extend(reph);
    Silbe {
        logisch: logisch.iter().collect(),
        anzeige: anzeige.iter().collect(),
        hat_reph,
    }
}

/// Anzeige-Reihenfolge fuer den ganzen Text.
pub fn anzeige(text: &str) -> String {
    silben(text).iter().map(|s| s.anzeige.clone()).collect()
}

/// Bildet den Text auf Glyphen ab und wendet die Devanagari-GSUB-
/// Merkmale der Schrift an.
///
/// Rueckgabe: `None`, wenn die Schrift kein GSUB mit einem Devanagari-
/// Skript (`dev2` oder `deva`) hat — dann uebernimmt der Aufrufer die
/// Anzeige-Reihenfolge aus [`anzeige`] und zeichnet die Grundzeichen.
pub fn forme_glyphen(font: &TrueTypeFont, text: &str) -> Option<Vec<u16>> {
    let gsub = Gsub::neu(font)?;
    let mut aus: Vec<u16> = Vec::new();
    for silbe in silben(text) {
        if !silbe.logisch.chars().any(ist_devanagari) {
            for c in silbe.logisch.chars() {
                aus.push(glyph(font, c));
            }
            continue;
        }
        // Shaping-Reihenfolge: Reph vorne (fuer rphf), prae-basische
        // Matras davor, dann der Rest.
        let hat_reph = silbe.hat_reph;
        let mut form: Vec<char> = Vec::new();
        let logisch: Vec<char> = silbe.logisch.chars().collect();
        if hat_reph {
            form.push(logisch[0]);
            form.push(logisch[1]);
            let rest = &logisch[2..];
            form.extend(
                rest.iter()
                    .filter(|&&c| deva_zeichen(c).0 == Vokallage::Links),
            );
            form.extend(
                rest.iter()
                    .filter(|&&c| deva_zeichen(c).0 != Vokallage::Links),
            );
        } else {
            form.extend(logisch.iter());
        }
        let mut glyphen: Vec<u16> = form.iter().map(|&c| glyph(font, c)).collect();
        gsub.wende_an(&mut glyphen);
        if hat_reph && !glyphen.is_empty() {
            // Wenn rphf gegriffen hat, ist das erste Zeichen die
            // Reph-Glyphe; sie wird ans Silbenende gestellt.
            let ra_glyph = glyph(font, RA);
            if glyphen[0] != ra_glyph {
                let reph = glyphen.remove(0);
                glyphen.push(reph);
            } else if glyphen.len() >= 2 {
                // Kein Reph-Glyph: RA + Halant ans Ende der Silbe.
                let ra = glyphen.remove(0);
                let hal = glyphen.remove(0);
                glyphen.push(ra);
                glyphen.push(hal);
            }
        }
        aus.extend(glyphen);
    }
    Some(aus)
}

fn glyph(font: &TrueTypeFont, ch: char) -> u16 {
    font.glyph_index(ch).unwrap_or(0) as u16
}

/// Hat die Schrift GSUB-Daten fuer Devanagari (`dev2` oder `deva`)?
pub fn hat_gsub(font: &TrueTypeFont) -> bool {
    Gsub::neu(font).is_some()
}

// ==========================================================================
// Minimaler GSUB-Leser: Merkmale auswaehlen, Lookups Typ 1/2/4 anwenden.
// ==========================================================================

struct Gsub<'a> {
    daten: &'a [u8],
}

impl<'a> Gsub<'a> {
    fn neu(font: &'a TrueTypeFont) -> Option<Gsub<'a>> {
        let daten = font.table(b"GSUB")?;
        let g = Gsub { daten };
        // Ein Devanagari-Skript muss vorhanden sein.
        let skript = g
            .skript_offset(b"dev2", 0)
            .or_else(|| g.skript_offset(b"deva", 0))?;
        if skript == 0 {
            return None;
        }
        Some(g)
    }

    fn u16(&self, off: usize) -> Option<u16> {
        let b = self.daten.get(off..off + 2)?;
        Some(u16::from_be_bytes([b[0], b[1]]))
    }

    fn i16_wert(&self, off: usize) -> Option<i16> {
        self.u16(off).map(|v| v as i16)
    }

    /// Offset der LangSys-Tabelle innerhalb von GSUB fuer ein Skript.
    fn skript_offset(&self, skript: &[u8; 4], basis: usize) -> Option<usize> {
        let sl = self.u16(basis + 4)? as usize;
        let anzahl = self.u16(sl)? as usize;
        for i in 0..anzahl {
            let p = sl + 2 + 6 * i;
            let tag = self.daten.get(p..p + 4)?;
            if tag == &skript[..] {
                let soff = self.u16(p + 4)? as usize;
                let st = sl + soff;
                // DefaultLangSys (muss laut Spezifikation vorhanden sein).
                let dls = self.u16(st)? as usize;
                if dls == 0 {
                    return None;
                }
                return Some(st + dls);
            }
        }
        None
    }

    /// Lookup-Indizes eines Merkmals im Default-LangSys von `dev2`/`deva`.
    fn merkmals_lookups(&self, basis: usize, tag: &[u8; 4]) -> Vec<u16> {
        let mut aus = Vec::new();
        let fl = match self.u16(basis + 6) {
            Some(b) => b as usize,
            None => return aus,
        };
        let skript = match self
            .skript_offset(b"dev2", basis)
            .or_else(|| self.skript_offset(b"deva", basis))
        {
            Some(s) => s,
            None => return aus,
        };
        let anzd = match self.u16(skript + 4) {
            Some(a) => a as usize,
            None => return aus,
        };
        for i in 0..anzd {
            let fi = match self.u16(skript + 6 + 2 * i) {
                Some(a) => a as usize,
                None => return aus,
            };
            let fp = fl + 2 + 6 * fi;
            let ftag = match self.daten.get(fp..fp + 4) {
                Some(t) => t,
                None => continue,
            };
            if ftag != &tag[..] {
                continue;
            }
            let foff = match self.u16(fp + 4) {
                Some(a) => a as usize,
                None => continue,
            };
            let ft = fl + foff;
            let lc = match self.u16(ft + 2) {
                Some(a) => a as usize,
                None => continue,
            };
            for j in 0..lc {
                if let Some(l) = self.u16(ft + 4 + 2 * j) {
                    aus.push(l);
                }
            }
        }
        aus
    }

    /// Wendet alle bekannten Devanagari-Merkmale auf die Glyphenfolge an.
    fn wende_an(&self, glyphen: &mut Vec<u16>) {
        let basis = 0;
        for tag in MERKMALE {
            for lookup in self.merkmals_lookups(basis, tag) {
                self.lookup(basis, lookup as usize, glyphen);
            }
        }
    }

    fn lookup(&self, basis: usize, index: usize, glyphen: &mut Vec<u16>) {
        let ll = match self.u16(basis + 8) {
            Some(a) => a as usize,
            None => return,
        };
        let offset = match self.u16(ll + 2 + 2 * index) {
            Some(a) => a as usize,
            None => return,
        };
        let lp = ll + offset;
        let typ = match self.u16(lp) {
            Some(t) => t,
            None => return,
        };
        let subc = match self.u16(lp + 4) {
            Some(c) => c as usize,
            None => return,
        };
        for s in 0..subc {
            let sub = match self.u16(lp + 6 + 2 * s) {
                Some(o) => o as usize,
                None => continue,
            };
            let st = lp + sub;
            match typ {
                1 => self.single(st, glyphen),
                2 => self.mehrfach(st, glyphen),
                4 => self.ligatur(st, glyphen),
                _ => {} // Typ 6 und andere werden bewusst nicht angewendet.
            }
        }
    }

    /// Coverage-Index eines Glyphs (Format 1 und 2).
    fn coverage_index(&self, basis: usize, cov: usize, g: u16) -> Option<usize> {
        let p = basis + cov;
        match self.u16(p)? {
            1 => {
                let anz = self.u16(p + 2)? as usize;
                for i in 0..anz {
                    if self.u16(p + 4 + 2 * i)? == g {
                        return Some(i);
                    }
                }
                None
            }
            2 => {
                let anz = self.u16(p + 2)? as usize;
                for i in 0..anz {
                    let r = p + 4 + 6 * i;
                    let start = self.u16(r)?;
                    let ende = self.u16(r + 2)?;
                    let idx = self.u16(r + 4)? as usize;
                    if (start..=ende).contains(&g) {
                        return Some(idx + (g - start) as usize);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Lookup-Typ 1: Einzelersetzung.
    fn single(&self, st: usize, glyphen: &mut [u16]) {
        let format = match self.u16(st) {
            Some(f) => f,
            None => return,
        };
        let cov = match self.u16(st + 2) {
            Some(c) => c as usize,
            None => return,
        };
        for g in glyphen.iter_mut() {
            let idx = match self.coverage_index(st, cov, *g) {
                Some(i) => i,
                None => continue,
            };
            match format {
                1 => {
                    if let Some(delta) = self.i16_wert(st + 4) {
                        *g = ((*g as i32) + delta as i32) as u16;
                    }
                }
                2 => {
                    if let Some(neu) = self.u16(st + 6 + 2 * idx) {
                        *g = neu;
                    }
                }
                _ => {}
            }
        }
    }

    /// Lookup-Typ 2: Mehrfachersetzung (1 Glyphe -> n Glyphen).
    fn mehrfach(&self, st: usize, glyphen: &mut Vec<u16>) {
        let format = self.u16(st).unwrap_or(0);
        if format != 1 {
            return;
        }
        let cov = match self.u16(st + 2) {
            Some(c) => c as usize,
            None => return,
        };
        let mut i = 0;
        while i < glyphen.len() {
            let idx = match self.coverage_index(st, cov, glyphen[i]) {
                Some(x) => x,
                None => {
                    i += 1;
                    continue;
                }
            };
            let anzahl_seq = match self.u16(st + 4) {
                Some(a) => a as usize,
                None => return,
            };
            if idx >= anzahl_seq {
                i += 1;
                continue;
            }
            let seq = match self.u16(st + 6 + 2 * idx) {
                Some(s) => st + s as usize,
                None => {
                    i += 1;
                    continue;
                }
            };
            let anz = match self.u16(seq) {
                Some(a) => a as usize,
                None => {
                    i += 1;
                    continue;
                }
            };
            let mut ersatz = Vec::with_capacity(anz);
            for k in 0..anz {
                match self.u16(seq + 2 + 2 * k) {
                    Some(g) => ersatz.push(g),
                    None => break,
                }
            }
            let n = ersatz.len();
            glyphen.splice(i..i + 1, ersatz);
            i += n.max(1);
        }
    }

    /// Lookup-Typ 4: Ligaturersetzung.
    fn ligatur(&self, st: usize, glyphen: &mut Vec<u16>) {
        let format = self.u16(st).unwrap_or(0);
        if format != 1 {
            return;
        }
        let cov = match self.u16(st + 2) {
            Some(c) => c as usize,
            None => return,
        };
        let mut i = 0;
        while i < glyphen.len() {
            if self.coverage_index(st, cov, glyphen[i]).is_none() {
                i += 1;
                continue;
            }
            let sets = match self.u16(st + 4) {
                Some(s) => s as usize,
                None => return,
            };
            'sets: for s in 0..sets {
                let set = match self.u16(st + 6 + 2 * s) {
                    Some(o) => st + o as usize,
                    None => continue,
                };
                let ligaturen = match self.u16(set) {
                    Some(l) => l as usize,
                    None => continue,
                };
                for l in 0..ligaturen {
                    let lig = match self.u16(set + 2 + 2 * l) {
                        Some(o) => set + o as usize,
                        None => continue,
                    };
                    let lig_glyph = match self.u16(lig) {
                        Some(g) => g,
                        None => continue,
                    };
                    let teile = match self.u16(lig + 2) {
                        Some(c) => c as usize,
                        None => continue,
                    };
                    if teile < 2 || i + teile > glyphen.len() {
                        continue;
                    }
                    let mut passt = true;
                    for k in 1..teile {
                        match self.u16(lig + 2 + 2 * k) {
                            Some(erwartet) if erwartet == glyphen[i + k] => {}
                            _ => {
                                passt = false;
                                break;
                            }
                        }
                    }
                    if passt {
                        glyphen[i] = lig_glyph;
                        glyphen.drain(i + 1..i + teile);
                        break 'sets;
                    }
                }
            }
            i += 1;
        }
    }
}
