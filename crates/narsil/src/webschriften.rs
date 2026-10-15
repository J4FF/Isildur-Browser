//! Release 09.10.2026: die WEBSCHRIFTEN einer Seite holen und anmelden.
//!
//! `@font-face` stand bisher nur im geparsten Blatt — keine Datei wurde
//! geholt, jede Seite rechnete mit Ersatzschriften (t-online.de: Arial
//! statt Roboto, die Navigation lief rechts aus dem Bild). Dieser Weg laeuft
//! nach dem Holen der Blaetter und VOR dem Seitenbau:
//!
//! 1. alle `<style>`-Inhalte (die geholten Blaetter stehen dann schon im
//!    Baum, ihre `url()` sind absolut, siehe `blaetter::urls_absolut`) lesen
//!    und die `@font-face`-Regeln sammeln,
//! 2. je Regel die beste Quelle waehlen (WOFF2 > WOFF > TTF/OTF; `data:`
//!    direkt), nur Bereiche, die lateinische Buchstaben enthalten (Google
//!    Fonts teilt eine Familie in viele `unicode-range`-Stuecke),
//! 3. parallel holen, parallel dekodieren und in
//!    `narsil_text::webschrift` anmelden — Messung und Raster sehen sie dann.
//!
//! Bereits geholte Adressen werden nicht noch einmal geholt (prozessweit).
//!
//! M3/M4: hoechstens [`MAX_SCHRIFTEN_JE_SEITE`] Dateien je Seite.

use std::collections::HashSet;
use std::sync::Mutex;

use narsil_fmt_html::HtmlNode;

/// Hoechstens so viele Schriftdateien je Seite.
pub const MAX_SCHRIFTEN_JE_SEITE: usize = 24;
/// Hoechstens so viele `<style>`-Bloecke werden durchsucht.
const MAX_STILBLOECKE: usize = 256;

static GEHOLT: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Was geschah.
#[derive(Debug, Default)]
pub struct Ergebnis {
    /// Neu angemeldete Schnitte.
    pub angemeldet: usize,
    /// Schon bekannt (frueher geholt).
    pub bekannt: usize,
    /// Fehler (Adresse: Grund).
    pub fehler: Vec<String>,
}

struct Plan {
    familie: String,
    gewicht: (u16, u16),
    kursiv: bool,
    url: String,
}

/// Sammelt, holt und meldet an.
pub fn holen_und_anmelden(baum: &[HtmlNode], basis: &str) -> Ergebnis {
    let mut r = Ergebnis::default();
    let mut texte: Vec<&str> = Vec::new();
    stile_sammeln(baum, &mut texte, 0);
    let mut plaene: Vec<Plan> = Vec::new();
    let mut gesehen: HashSet<String> = HashSet::new();
    for text in texte.iter().take(MAX_STILBLOECKE) {
        if !text.contains("@font-face") && !text.contains("@FONT-FACE") {
            continue;
        }
        let blatt = narsil_css::stylesheet_lesen_gemerkt(text);
        for regel in &blatt.regeln {
            let narsil_css::Regel::FontFace(q) = regel else { continue };
            if q.familie.trim().is_empty() || !bereich_hat_latein(&q.zeichen_bereiche) {
                continue;
            }
            let Some(url) = beste_quelle(&q.quellen, &q.formate, basis) else { continue };
            if !gesehen.insert(url.clone()) {
                continue;
            }
            let gewicht = q.gewicht.unwrap_or(400).clamp(1, 1000);
            let stil = q.stil.trim().to_ascii_lowercase();
            plaene.push(Plan {
                familie: q.familie.clone(),
                gewicht: (gewicht, gewicht),
                kursiv: stil.starts_with("italic") || stil.starts_with("oblique"),
                url,
            });
        }
    }
    // Schon geholte nicht noch einmal.
    {
        let mut g = GEHOLT.lock().unwrap_or_else(|e| e.into_inner());
        let set = g.get_or_insert_with(HashSet::new);
        plaene.retain(|p| {
            if set.contains(&p.url) {
                r.bekannt += 1;
                false
            } else {
                true
            }
        });
    }
    plaene.truncate(MAX_SCHRIFTEN_JE_SEITE);
    if plaene.is_empty() {
        return r;
    }

    // Holen: `data:` direkt, alles andere parallel ueber das Netz.
    let mut daten: Vec<Option<Vec<u8>>> = vec![None; plaene.len()];
    let mut aufgaben: Vec<narsil_net::parallel::Aufgabe> = Vec::new();
    let mut aufgabe_zu_plan: Vec<usize> = Vec::new();
    for (i, p) in plaene.iter().enumerate() {
        if let Some(rest) = p.url.strip_prefix("data:") {
            match data_url_bytes(rest) {
                Some(b) => daten[i] = Some(b),
                None => r.fehler.push(format!("{}: data-URL nicht lesbar", p.familie)),
            }
            continue;
        }
        match crate::blaetter::vorbereite(&p.url) {
            Ok(anfrage) => {
                aufgaben.push(narsil_net::parallel::Aufgabe::neu(narsil_net::parallel::Rang::Sichtbar, anfrage));
                aufgabe_zu_plan.push(i);
            }
            Err(e) => r.fehler.push(format!("{}: {e}", p.url)),
        }
    }
    if !aufgaben.is_empty() {
        let antworten = narsil_net::parallel::hole_alle(&aufgaben, narsil_net::Limits::default());
        for (k, antwort) in antworten.iter().enumerate() {
            let Some(&i) = aufgabe_zu_plan.get(k) else { continue };
            match &antwort.antwort {
                Ok(a) if a.status.code < 400 => daten[i] = Some(a.body.clone()),
                Ok(a) => r.fehler.push(format!("{}: HTTP {}", plaene[i].url, a.status.code)),
                Err(e) => r.fehler.push(format!("{}: {e}", plaene[i].url)),
            }
        }
    }

    // Dekodieren (WOFF2 = Brotli + glyf-Transform) parallel, Anmelden danach.
    let ergebnisse: Vec<Result<(), String>> = std::thread::scope(|sc| {
        let griffe: Vec<_> = plaene
            .iter()
            .zip(daten.iter())
            .map(|(p, d)| {
                sc.spawn(move || match d {
                    Some(b) => narsil_text::webschrift::registrieren(&p.familie, p.gewicht, p.kursiv, b),
                    None => Err(String::new()),
                })
            })
            .collect();
        griffe
            .into_iter()
            .map(|g| g.join().unwrap_or_else(|_| Err("Dekoder abgestuerzt".into())))
            .collect()
    });
    let mut g = GEHOLT.lock().unwrap_or_else(|e| e.into_inner());
    let set = g.get_or_insert_with(HashSet::new);
    for (p, e) in plaene.iter().zip(ergebnisse) {
        set.insert(p.url.clone());
        match e {
            Ok(()) => r.angemeldet += 1,
            Err(f) if f.is_empty() => {}
            Err(f) => r.fehler.push(f),
        }
    }
    if set.len() > 4096 {
        set.clear();
    }
    r
}

fn stile_sammeln<'a>(knoten: &'a [HtmlNode], raus: &mut Vec<&'a str>, tiefe: usize) {
    if tiefe > 64 || raus.len() >= MAX_STILBLOECKE {
        return;
    }
    for k in knoten {
        if let HtmlNode::Element { tag, children, .. } = k {
            if tag.eq_ignore_ascii_case("style") {
                for c in children {
                    if let HtmlNode::Text(t) = c {
                        raus.push(t.as_str());
                    }
                }
            } else if !tag.eq_ignore_ascii_case("script") {
                stile_sammeln(children, raus, tiefe + 1);
            }
        }
    }
}

/// Enthaelt `unicode-range` lateinische Grossbuchstaben (U+0041)? Leer = ja.
fn bereich_hat_latein(bereiche: &[String]) -> bool {
    if bereiche.is_empty() {
        return true;
    }
    let ziel = 0x41u32;
    bereiche.iter().any(|b| {
        let b = b.trim().trim_start_matches(['U', 'u']).trim_start_matches('+');
        if b.contains('?') {
            let lo = u32::from_str_radix(&b.replace('?', "0"), 16).unwrap_or(u32::MAX);
            let hi = u32::from_str_radix(&b.replace('?', "F"), 16).unwrap_or(0);
            return lo <= ziel && ziel <= hi;
        }
        match b.split_once('-') {
            Some((a, e)) => {
                let lo = u32::from_str_radix(a.trim(), 16).unwrap_or(u32::MAX);
                let hi = u32::from_str_radix(e.trim().trim_start_matches(['U', 'u']).trim_start_matches('+'), 16).unwrap_or(0);
                lo <= ziel && ziel <= hi
            }
            None => u32::from_str_radix(b, 16).is_ok_and(|v| v == ziel),
        }
    })
}

/// WOFF2 > WOFF > TTF/OTF; unbekannte Formate (eot, svg) nie.
fn beste_quelle(quellen: &[String], formate: &[String], basis: &str) -> Option<String> {
    let rang = |url: &str, format: &str| -> Option<u8> {
        let f = format.to_ascii_lowercase();
        let u = url.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
        let ist = |endung: &str, name: &str| f == name || (f.is_empty() && u.ends_with(endung));
        if ist(".woff2", "woff2") || (url.starts_with("data:") && url.contains("woff2")) {
            Some(0)
        } else if ist(".woff", "woff") || (url.starts_with("data:") && url.contains("woff")) {
            Some(1)
        } else if ist(".ttf", "truetype") || ist(".otf", "opentype") || f == "truetype" || f == "opentype" {
            Some(2)
        } else {
            None
        }
    };
    let mut beste: Option<(u8, &String)> = None;
    for (i, q) in quellen.iter().enumerate() {
        let f = formate.get(i).map(String::as_str).unwrap_or("");
        if let Some(r) = rang(q, f)
            && beste.is_none_or(|(b, _)| r < b)
        {
            beste = Some((r, q));
        }
    }
    let (_, url) = beste?;
    if url.starts_with("data:") || url.starts_with("http://") || url.starts_with("https://") {
        return Some(url.clone());
    }
    narsil_url::loese_auf(basis, url).ok()
}

/// `data:[<mime>][;base64],<daten>` -> Bytes (nur base64).
fn data_url_bytes(rest: &str) -> Option<Vec<u8>> {
    let (kopf, inhalt) = rest.split_once(',')?;
    if !kopf.to_ascii_lowercase().contains(";base64") {
        return None;
    }
    base64(inhalt)
}

fn base64(s: &str) -> Option<Vec<u8>> {
    let mut raus = Vec::with_capacity(s.len() * 3 / 4);
    let mut puffer = 0u32;
    let mut bits = 0u32;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b' ' | b'\n' | b'\r' | b'\t' => continue,
            b'%' => return None,
            _ => return None,
        } as u32;
        puffer = (puffer << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            raus.push((puffer >> bits) as u8);
            puffer &= (1 << bits) - 1;
        }
        if raus.len() > narsil_text::webschrift::MAX_WEBSCHRIFT_BYTES {
            return None;
        }
    }
    Some(raus)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latein_bereiche() {
        assert!(bereich_hat_latein(&[]));
        assert!(bereich_hat_latein(&["U+0000-00FF".into(), "U+0131".into()]));
        assert!(!bereich_hat_latein(&["U+0400-045F".into()]));
        assert!(bereich_hat_latein(&["U+00??".into()]));
    }

    #[test]
    fn quelle_woff2_vor_woff() {
        let q = vec!["a.woff".to_string(), "a.woff2".to_string(), "a.eot".to_string()];
        let f = vec!["woff".to_string(), "woff2".to_string(), String::new()];
        assert_eq!(beste_quelle(&q, &f, "https://x.de/css/s.css").as_deref(), Some("https://x.de/css/a.woff2"));
    }

    #[test]
    fn base64_rund() {
        assert_eq!(base64("aGFsbG8=").unwrap(), b"hallo");
    }
}
