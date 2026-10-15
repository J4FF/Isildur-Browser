//! OpenType glyph substitution (GSUB) for canvas text: the `locl`,
//! `ccmp`, `rlig`, `liga` and `clig` features with single (type 1) and
//! ligature (type 4) lookups, also behind extension lookups (type 7).
//!
//! Script and language system follow the text and `ctx.lang`, so a
//! Turkish context does not form the `fi` ligature in fonts whose `TRK`
//! language system leaves it out (WPT 2d.text.measure.lang).

/// Features applied by default, in this order of preference (the lookups
/// themselves run in lookup-list order, as OpenType requires).
const FEATURES: [&[u8; 4]; 5] = [b"locl", b"ccmp", b"rlig", b"liga", b"clig"];

/// Upper bound for lookups and glyphs touched per run (malformed fonts).
const MAX_LOOKUPS: usize = 256;
const MAX_RUN: usize = 4096;

struct Table<'a> {
    d: &'a [u8],
}

impl Table<'_> {
    fn u16(&self, at: usize) -> Option<usize> {
        let b = self.d.get(at..at + 2)?;
        Some(u16::from_be_bytes([b[0], b[1]]) as usize)
    }

    fn i16(&self, at: usize) -> Option<i32> {
        let b = self.d.get(at..at + 2)?;
        Some(i16::from_be_bytes([b[0], b[1]]) as i32)
    }

    fn u32(&self, at: usize) -> Option<usize> {
        let b = self.d.get(at..at + 4)?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }

    fn tag(&self, at: usize) -> Option<&[u8]> {
        self.d.get(at..at + 4)
    }

    /// Coverage index of `glyph` in the coverage table at `at`.
    fn coverage(&self, at: usize, glyph: u32) -> Option<usize> {
        match self.u16(at)? {
            1 => {
                let n = self.u16(at + 2)?;
                let (mut lo, mut hi) = (0usize, n);
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    let g = self.u16(at + 4 + 2 * mid)? as u32;
                    match g.cmp(&glyph) {
                        std::cmp::Ordering::Equal => return Some(mid),
                        std::cmp::Ordering::Less => lo = mid + 1,
                        std::cmp::Ordering::Greater => hi = mid,
                    }
                }
                None
            }
            2 => {
                let n = self.u16(at + 2)?;
                let (mut lo, mut hi) = (0usize, n);
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    let r = at + 4 + 6 * mid;
                    let (start, end) = (self.u16(r)? as u32, self.u16(r + 2)? as u32);
                    if glyph < start {
                        hi = mid;
                    } else if glyph > end {
                        lo = mid + 1;
                    } else {
                        return Some(self.u16(r + 4)? + (glyph - start) as usize);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// The LangSys table for a script and language (default otherwise).
    fn lang_sys(&self, script: &[u8; 4], lang: Option<&[u8; 4]>) -> Option<usize> {
        let list = self.u16(4)?;
        let n = self.u16(list)?;
        let mut found = None;
        for want in [&script[..], b"DFLT", b"latn"] {
            for i in 0..n {
                let rec = list + 2 + 6 * i;
                if self.tag(rec)? == want {
                    found = Some(list + self.u16(rec + 4)?);
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        let s = found?;
        if let Some(lang) = lang {
            for i in 0..self.u16(s + 2)? {
                let rec = s + 4 + 6 * i;
                if self.tag(rec)? == &lang[..] {
                    return Some(s + self.u16(rec + 4)?);
                }
            }
        }
        let default = self.u16(s)?;
        (default != 0).then_some(s + default)
    }

    /// Lookup indices of the wanted features in a LangSys, sorted.
    fn lookups(&self, lang_sys: usize) -> Option<Vec<usize>> {
        let features = self.u16(6)?;
        let count = self.u16(features)?;
        let mut out = Vec::new();
        let n = self.u16(lang_sys + 4)?;
        for i in 0..n {
            let fi = self.u16(lang_sys + 6 + 2 * i)?;
            if fi >= count {
                continue;
            }
            let rec = features + 2 + 6 * fi;
            let tag = self.tag(rec)?;
            if !FEATURES.iter().any(|f| &f[..] == tag) {
                continue;
            }
            let f = features + self.u16(rec + 4)?;
            for k in 0..self.u16(f + 2)? {
                out.push(self.u16(f + 4 + 2 * k)?);
            }
        }
        out.sort_unstable();
        out.dedup();
        out.truncate(MAX_LOOKUPS);
        Some(out)
    }

    /// Subtables of a lookup as (type, offset), extensions resolved.
    fn subtables(&self, lookup_index: usize) -> Option<Vec<(usize, usize)>> {
        let list = self.u16(8)?;
        if lookup_index >= self.u16(list)? {
            return None;
        }
        let l = list + self.u16(list + 2 + 2 * lookup_index)?;
        let kind = self.u16(l)?;
        let mut out = Vec::new();
        for i in 0..self.u16(l + 4)?.min(64) {
            let s = l + self.u16(l + 6 + 2 * i)?;
            if kind == 7 {
                if self.u16(s)? != 1 {
                    continue;
                }
                out.push((self.u16(s + 2)?, s + self.u32(s + 4)?));
            } else {
                out.push((kind, s));
            }
        }
        Some(out)
    }

    /// Applies a single (1) or ligature (4) subtable at `i`; returns how
    /// many glyphs the substitution consumed (0 = no match).
    fn apply_at(&self, kind: usize, s: usize, glyphs: &mut Vec<u32>, clusters: &mut Vec<Vec<usize>>, i: usize) -> Option<usize> {
        let g = glyphs[i];
        match kind {
            1 => {
                let cov = self.coverage(s + self.u16(s + 2)?, g)?;
                let new = match self.u16(s)? {
                    1 => (g as i32 + self.i16(s + 4)?).rem_euclid(65536) as u32,
                    2 => {
                        if cov >= self.u16(s + 4)? {
                            return None;
                        }
                        self.u16(s + 6 + 2 * cov)? as u32
                    }
                    _ => return None,
                };
                glyphs[i] = new;
                Some(1)
            }
            4 => {
                if self.u16(s)? != 1 {
                    return None;
                }
                let cov = self.coverage(s + self.u16(s + 2)?, g)?;
                if cov >= self.u16(s + 4)? {
                    return None;
                }
                let set = s + self.u16(s + 6 + 2 * cov)?;
                for k in 0..self.u16(set)? {
                    let lig = set + self.u16(set + 2 + 2 * k)?;
                    let comps = self.u16(lig + 2)?;
                    if comps == 0 || i + comps > glyphs.len() {
                        continue;
                    }
                    let mut ok = true;
                    for c in 1..comps {
                        if self.u16(lig + 4 + 2 * (c - 1))? as u32 != glyphs[i + c] {
                            ok = false;
                            break;
                        }
                    }
                    if !ok {
                        continue;
                    }
                    glyphs[i] = self.u16(lig)? as u32;
                    for _ in 1..comps {
                        glyphs.remove(i + 1);
                        let merged = clusters.remove(i + 1);
                        clusters[i].extend(merged);
                    }
                    return Some(1);
                }
                None
            }
            _ => None,
        }
    }
}

/// Runs the default substitutions over one font run. `clusters[i]` lists
/// the input positions behind output glyph `i` (ligatures merge them).
pub(crate) fn substitute(gsub: &[u8], script: &[u8; 4], lang: Option<&[u8; 4]>, glyphs: &mut Vec<u32>, clusters: &mut Vec<Vec<usize>>) {
    if glyphs.is_empty() || glyphs.len() > MAX_RUN || glyphs.len() != clusters.len() {
        return;
    }
    let t = Table { d: gsub };
    let Some(lang_sys) = t.lang_sys(script, lang) else { return };
    let Some(lookups) = t.lookups(lang_sys) else { return };
    for li in lookups {
        let Some(subs) = t.subtables(li) else { continue };
        let mut i = 0;
        while i < glyphs.len() {
            let mut step = 1;
            for &(kind, s) in &subs {
                if let Some(n) = t.apply_at(kind, s, glyphs, clusters, i) {
                    step = n;
                    break;
                }
            }
            i += step;
        }
    }
}

/// OpenType script tag for a character run.
pub(crate) fn script_for(text: &[char]) -> [u8; 4] {
    for &c in text {
        let u = c as u32;
        if (0x0400..=0x052F).contains(&u) {
            return *b"cyrl";
        }
        if (0x0370..=0x03FF).contains(&u) || (0x1F00..=0x1FFF).contains(&u) {
            return *b"grek";
        }
    }
    *b"latn"
}

/// OpenType language system tag for a BCP 47 language (`ctx.lang`).
pub(crate) fn language_tag(lang: &str) -> Option<[u8; 4]> {
    let primary = lang.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
    Some(*match primary.as_str() {
        "tr" => b"TRK ",
        "az" => b"AZE ",
        "crh" => b"CRT ",
        "tt" => b"TAT ",
        "kk" => b"KAZ ",
        "ro" => b"ROM ",
        "mo" => b"MOL ",
        "sr" => b"SRB ",
        "mk" => b"MKD ",
        "bg" => b"BGR ",
        "ca" => b"CAT ",
        "nl" => b"NLD ",
        "pl" => b"PLK ",
        "de" => b"DEU ",
        "fr" => b"FRA ",
        "es" => b"ESP ",
        "it" => b"ITA ",
        "pt" => b"PTG ",
        "hu" => b"HUN ",
        "cs" => b"CSY ",
        "sk" => b"SKY ",
        "lt" => b"LTH ",
        "lv" => b"LVI ",
        "is" => b"ISL ",
        "fi" => b"FIN ",
        "sv" => b"SVE ",
        "da" => b"DAN ",
        "nb" | "no" => b"NOR ",
        "ru" => b"RUS ",
        "uk" => b"UKR ",
        "el" => b"ELL ",
        "en" => b"ENG ",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GSUB table with script `latn`, a default and a `TRK ` language
    /// system: `liga` with an f+i ligature only in the default one.
    fn table() -> Vec<u8> {
        let mut d = Vec::new();
        let p16 = |d: &mut Vec<u8>, v: u16| d.extend_from_slice(&v.to_be_bytes());
        // Header: version 1.0, ScriptList @10, FeatureList @?, LookupList @?
        p16(&mut d, 1);
        p16(&mut d, 0);
        p16(&mut d, 10); // script list
        p16(&mut d, 0); // feature list (patched)
        p16(&mut d, 0); // lookup list (patched)
        // ScriptList @10: 1 record latn -> Script @ +8
        p16(&mut d, 1);
        d.extend_from_slice(b"latn");
        p16(&mut d, 8);
        // Script @18: default LangSys @+10, 1 LangSysRecord TRK @+16
        p16(&mut d, 10);
        p16(&mut d, 1);
        d.extend_from_slice(b"TRK ");
        p16(&mut d, 18);
        // default LangSys @28: feature 0
        p16(&mut d, 0);
        p16(&mut d, 0xFFFF);
        p16(&mut d, 1);
        p16(&mut d, 0);
        // TRK LangSys @36: no features
        p16(&mut d, 0);
        p16(&mut d, 0xFFFF);
        p16(&mut d, 0);
        // FeatureList @42: 1 record liga -> Feature @ +8
        let fl = d.len() as u16;
        d[6..8].copy_from_slice(&fl.to_be_bytes());
        p16(&mut d, 1);
        d.extend_from_slice(b"liga");
        p16(&mut d, 8);
        // Feature: params 0, 1 lookup: 0
        p16(&mut d, 0);
        p16(&mut d, 1);
        p16(&mut d, 0);
        // LookupList: 1 lookup @ +4
        let ll = d.len() as u16;
        d[8..10].copy_from_slice(&ll.to_be_bytes());
        p16(&mut d, 1);
        p16(&mut d, 4);
        // Lookup: type 4, flag 0, 1 subtable @ +8
        p16(&mut d, 4);
        p16(&mut d, 0);
        p16(&mut d, 1);
        p16(&mut d, 8);
        // Ligature subtable: format 1, coverage @ +8, 1 set @ +14
        p16(&mut d, 1);
        p16(&mut d, 8);
        p16(&mut d, 1);
        p16(&mut d, 14);
        // Coverage format 1: glyph 10 (f)
        p16(&mut d, 1);
        p16(&mut d, 1);
        p16(&mut d, 10);
        // LigatureSet: 1 ligature @ +4
        p16(&mut d, 1);
        p16(&mut d, 4);
        // Ligature: glyph 99 = f(10) + i(11)
        p16(&mut d, 99);
        p16(&mut d, 2);
        p16(&mut d, 11);
        d
    }

    #[test]
    fn ligatures_follow_the_language_system() {
        let gsub = table();
        let mut g = vec![10, 11];
        let mut c = vec![vec![0], vec![1]];
        substitute(&gsub, b"latn", None, &mut g, &mut c);
        assert_eq!(g, vec![99]);
        assert_eq!(c, vec![vec![0, 1]]);
        let mut g = vec![10, 11];
        let mut c = vec![vec![0], vec![1]];
        substitute(&gsub, b"latn", language_tag("tr").as_ref(), &mut g, &mut c);
        assert_eq!(g, vec![10, 11]);
    }
}
