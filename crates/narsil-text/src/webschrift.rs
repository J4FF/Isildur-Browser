//! Release 09.10.2026: WEBSCHRIFTEN (`@font-face`).
//!
//! # WARUM
//!
//! Bis heute wurde `@font-face` gelesen und dann vergessen: jede Seite
//! rechnete mit Ersatzschriften (Arial statt Roboto bei t-online.de). Das
//! verschiebt jede Breite und jeden Zeilenumbruch gegenueber Firefox und
//! Chrome — die Navigation von t-online.de lief z. B. rechts aus dem Bild.
//!
//! # WIE
//!
//! Der Ladeweg (`narsil`) holt die Schriftdateien einer Seite und meldet sie
//! hier an ([`registrieren`]). Messung (`stapel_fuer_familie`) und Zeichnung
//! (`narsil_raster::Schrift::mit_familie`) fragen ZUERST hier ([`finden`]),
//! in der Reihenfolge der `font-family`-Liste: die erste Familie, die es als
//! Webschrift gibt, gewinnt — es sei denn, davor steht eine generische
//! Familie oder eine installierte Systemschrift (dann gilt der alte Weg).
//!
//! Hinter jede Webschrift kommt der Systemstapel, damit Zeichen, die die
//! Webschrift nicht hat (Emoji, CJK), trotzdem eine Glyphe finden.
//!
//! # GRENZEN (M3/M4)
//!
//! Hoechstens [`MAX_WEBSCHRIFTEN`] Eintraege (der aelteste faellt), je Datei
//! hoechstens [`MAX_WEBSCHRIFT_BYTES`]. Das Register ist prozessweit: zwei
//! Seiten mit gleichem Familiennamen teilen sich den Eintrag (der zuletzt
//! angemeldete gewinnt).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use crate::stapel::{Schriftstapel, Stapelschrift, generische_familie, system_stapel};
use crate::system::FontCategory;

/// Hoechstens so viele angemeldete Schriftschnitte.
pub const MAX_WEBSCHRIFTEN: usize = 96;
/// Hoechstens so gross darf eine Schriftdatei sein.
pub const MAX_WEBSCHRIFT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone)]
struct Eintrag {
    familie: String,
    gewicht: (u16, u16),
    kursiv: bool,
    /// `font-stretch` in tenths of a percent (1000 = normal).
    stretch: u16,
    schrift: Stapelschrift,
    stapel: Arc<Schriftstapel>,
    alter: u64,
}

/// `font-stretch` of a normal-width face, in tenths of a percent.
pub const NORMAL_STRETCH: u16 = 1000;

static REGISTER: RwLock<Vec<Eintrag>> = RwLock::new(Vec::new());
static STAND: AtomicU64 = AtomicU64::new(0);

/// Wechselt bei jeder Anmeldung: wer Schriften je Familie zwischenspeichert,
/// vergleicht damit und baut neu.
#[must_use]
pub fn stand() -> u64 {
    STAND.load(Ordering::Relaxed)
}

/// Ein Treffer aus [`finden`].
#[derive(Clone)]
pub struct Treffer {
    /// Familienname (klein).
    pub name: String,
    /// Die Webschrift selbst.
    pub schrift: Stapelschrift,
    /// Webschrift + Systemstapel dahinter.
    pub stapel: Arc<Schriftstapel>,
    /// Fett verlangt, aber nur ein duennerer Schnitt da (synthetisch fett).
    pub fett_fehlt: bool,
    /// Kursiv verlangt, aber nur ein aufrechter Schnitt da.
    pub kursiv_fehlt: bool,
}

/// Normalisiert einen Familiennamen: Anfuehrungszeichen weg, klein.
#[must_use]
pub fn familien_name(roh: &str) -> String {
    roh.trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .trim()
        .to_ascii_lowercase()
}

/// Meldet eine Schriftdatei an (WOFF2, WOFF, TTF/OTF).
///
/// `gewicht` ist der Bereich aus `font-weight` (`400 700` bei variablen
/// Schriften, sonst `(w, w)`).
pub fn registrieren(familie: &str, gewicht: (u16, u16), kursiv: bool, daten: &[u8]) -> Result<(), String> {
    register_with_stretch(familie, gewicht, kursiv, NORMAL_STRETCH, daten)
}

/// Registers a font file with a `font-stretch` descriptor (tenths of a
/// percent, 1000 = normal), so faces of one family that differ only in
/// width coexist (`FontFace.stretch`, canvas `fontStretch`).
pub fn register_with_stretch(familie: &str, gewicht: (u16, u16), kursiv: bool, stretch: u16, daten: &[u8]) -> Result<(), String> {
    let name = familien_name(familie);
    if name.is_empty() {
        return Err("Webschrift ohne Familiennamen".into());
    }
    if daten.len() > MAX_WEBSCHRIFT_BYTES {
        return Err(format!("Webschrift {name}: {} Byte (erlaubt {MAX_WEBSCHRIFT_BYTES})", daten.len()));
    }
    let sfnt;
    let daten = if daten.starts_with(b"wOFF") {
        sfnt = woff1_zu_sfnt(daten).ok_or_else(|| format!("Webschrift {name}: WOFF nicht lesbar"))?;
        &sfnt[..]
    } else {
        daten
    };
    let schrift = Stapelschrift::von_bytes(&name, daten).ok_or_else(|| {
        // Release 15.10.2026: the reason too (BBC's ReithSerif woff2 files
        // were "unreadable" without a word why).
        let grund = if daten.starts_with(b"wOF2") {
            crate::woff2::woff2_schrift(daten).err().map(|e| format!("{e:?}"))
        } else {
            crate::TrueTypeFont::from_bytes(daten).err().map(|e| format!("{e:?}"))
        };
        format!("Webschrift {name}: Datei nicht lesbar ({} Byte{})", daten.len(), grund.map(|g| format!(": {g}")).unwrap_or_default())
    })?;
    let (lo, hi) = (gewicht.0.min(gewicht.1).clamp(1, 1000), gewicht.0.max(gewicht.1).clamp(1, 1000));
    let fett = hi >= 600;
    let mut stapel = Schriftstapel::neu();
    let _ = stapel.hinzufuegen(schrift.clone());
    if let Ok(sys) = system_stapel(FontCategory::SansSerif, fett, kursiv) {
        for i in 0..sys.len() {
            if let Some(s) = sys.schrift(i) {
                let _ = stapel.hinzufuegen(s.clone());
            }
        }
    }
    let alter = STAND.fetch_add(1, Ordering::Relaxed) + 1;
    let neu = Eintrag {
        familie: name,
        gewicht: (lo, hi),
        kursiv,
        stretch: stretch.clamp(500, 2000),
        schrift,
        stapel: Arc::new(stapel),
        alter,
    };
    let mut reg = REGISTER.write().unwrap_or_else(|e| e.into_inner());
    if let Some(alt) = reg
        .iter_mut()
        .find(|e| e.familie == neu.familie && e.gewicht == neu.gewicht && e.kursiv == neu.kursiv && e.stretch == neu.stretch)
    {
        *alt = neu;
    } else {
        if reg.len() >= MAX_WEBSCHRIFTEN {
            if let Some((i, _)) = reg.iter().enumerate().min_by_key(|(_, e)| e.alter) {
                reg.swap_remove(i);
            }
        }
        reg.push(neu);
    }
    // Release 15.10.2026: change the generation again once the register holds
    // the new face, so a cache filled between the first change and the write
    // (`familie::stapel_fuer_familie`) is rebuilt.
    drop(reg);
    STAND.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Ist diese Familie (mindestens ein Schnitt) angemeldet?
#[must_use]
pub fn ist_angemeldet(familie: &str) -> bool {
    let name = familien_name(familie);
    REGISTER
        .read()
        .map(|r| r.iter().any(|e| e.familie == name))
        .unwrap_or(false)
}

/// Zahl der angemeldeten Schnitte.
#[must_use]
pub fn anzahl() -> usize {
    REGISTER.read().map(|r| r.len()).unwrap_or(0)
}

/// Sucht fuer eine `font-family`-Liste die Webschrift (CSS Fonts 4 §5).
///
/// `None` heisst: nimm den Systemweg — keine Webschrift, oder vor ihr steht
/// eine generische Familie bzw. eine installierte Systemschrift.
#[must_use]
pub fn finden(liste: &str, gewicht: u16, kursiv: bool) -> Option<Treffer> {
    find_with_stretch(liste, gewicht, kursiv, NORMAL_STRETCH)
}

/// [`finden`] for a requested `font-stretch` (tenths of a percent).
#[must_use]
pub fn find_with_stretch(liste: &str, gewicht: u16, kursiv: bool, stretch: u16) -> Option<Treffer> {
    let reg = REGISTER.read().ok()?;
    if reg.is_empty() {
        return None;
    }
    for roh in liste.split(',').take(16) {
        let name = familien_name(roh);
        if name.is_empty() {
            continue;
        }
        let schnitte: Vec<&Eintrag> = reg.iter().filter(|e| e.familie == name).collect();
        if !schnitte.is_empty() {
            let e = beste_schnitt(&closest_stretch(&schnitte, stretch), gewicht, kursiv)?;
            return Some(Treffer {
                name: e.familie.clone(),
                schrift: e.schrift.clone(),
                stapel: e.stapel.clone(),
                fett_fehlt: gewicht >= 600 && e.gewicht.1 < 600,
                kursiv_fehlt: kursiv && !e.kursiv,
            });
        }
        if generische_familie(&name).is_some() || system_hat_familie(&name) {
            return None;
        }
    }
    None
}

/// CSS Fonts 4 section 5.2 step 4a: the faces with the closest width.
/// Narrower faces are preferred for widths up to normal, wider ones above.
fn closest_stretch<'a>(faces: &[&'a Eintrag], wanted: u16) -> Vec<&'a Eintrag> {
    let key = |s: u16| -> (u8, u16) {
        if s == wanted {
            (0, 0)
        } else if wanted <= NORMAL_STRETCH {
            if s < wanted { (1, wanted - s) } else { (2, s - wanted) }
        } else if s > wanted {
            (1, s - wanted)
        } else {
            (2, wanted - s)
        }
    };
    let Some(best) = faces.iter().map(|e| key(e.stretch)).min() else { return Vec::new() };
    faces.iter().copied().filter(|e| key(e.stretch) == best).collect()
}

/// CSS Fonts 4 §5.2 (vereinfacht): passende Neigung zuerst, dann das
/// Gewicht — im Bereich, sonst der naechste (bei Gleichstand der schwerere
/// fuer >= 500, sonst der leichtere).
fn beste_schnitt<'a>(schnitte: &[&'a Eintrag], gewicht: u16, kursiv: bool) -> Option<&'a Eintrag> {
    let passend: Vec<&&Eintrag> = schnitte.iter().filter(|e| e.kursiv == kursiv).collect();
    let auswahl: Vec<&&Eintrag> = if passend.is_empty() { schnitte.iter().collect() } else { passend };
    auswahl
        .into_iter()
        .min_by_key(|e| {
            let (lo, hi) = e.gewicht;
            let abstand = if gewicht < lo {
                lo - gewicht
            } else if gewicht > hi {
                gewicht - hi
            } else {
                0
            };
            // Gleichstand: fuer schwere Wuensche den schwereren Schnitt.
            let neben = if gewicht >= 500 { u16::MAX - hi } else { lo };
            (abstand, neben)
        })
        .copied()
}

fn system_hat_familie(name: &str) -> bool {
    crate::familie::systemschriften()
        .iter()
        .any(|f| f.family.eq_ignore_ascii_case(name))
}

/// WOFF 1.0 -> sfnt (W3C WOFF File Format 1.0): Kopf 44 Byte, je Tabelle
/// (Tag, Versatz, gepackte Laenge, Laenge, Pruefsumme); gepackte Tabellen
/// sind zlib.
fn woff1_zu_sfnt(d: &[u8]) -> Option<Vec<u8>> {
    let u32_at = |i: usize| -> Option<u32> { Some(u32::from_be_bytes(d.get(i..i + 4)?.try_into().ok()?)) };
    let u16_at = |i: usize| -> Option<u16> { Some(u16::from_be_bytes(d.get(i..i + 2)?.try_into().ok()?)) };
    let flavor = u32_at(4)?;
    let n = u16_at(12)? as usize;
    if n == 0 || n > 256 {
        return None;
    }
    let mut tabellen: Vec<([u8; 4], Vec<u8>, u32)> = Vec::with_capacity(n);
    let mut summe = 0usize;
    for t in 0..n {
        let b = 44 + t * 20;
        let tag: [u8; 4] = d.get(b..b + 4)?.try_into().ok()?;
        let off = u32_at(b + 4)? as usize;
        let komp = u32_at(b + 8)? as usize;
        let lang = u32_at(b + 12)? as usize;
        let pruef = u32_at(b + 16)?;
        if lang > MAX_WEBSCHRIFT_BYTES || komp > d.len() {
            return None;
        }
        summe += lang;
        if summe > 4 * MAX_WEBSCHRIFT_BYTES {
            return None;
        }
        let roh = d.get(off..off + komp)?;
        let daten = if komp < lang {
            let z = narsil_compress_inflate_zlib(roh, lang)?;
            if z.len() != lang {
                return None;
            }
            z
        } else {
            roh.to_vec()
        };
        tabellen.push((tag, daten, pruef));
    }
    // sfnt schreiben: Kopf + Verzeichnis + Tabellen (4-Byte-ausgerichtet).
    let mut raus = Vec::with_capacity(12 + 16 * n + summe + 4 * n);
    let mut pot = 1u16;
    let mut log = 0u16;
    while (pot as usize) * 2 <= n {
        pot *= 2;
        log += 1;
    }
    raus.extend_from_slice(&flavor.to_be_bytes());
    raus.extend_from_slice(&(n as u16).to_be_bytes());
    raus.extend_from_slice(&(pot * 16).to_be_bytes());
    raus.extend_from_slice(&log.to_be_bytes());
    raus.extend_from_slice(&((n as u16) * 16 - pot * 16).to_be_bytes());
    let mut versatz = 12 + 16 * n;
    for (tag, daten, pruef) in &tabellen {
        raus.extend_from_slice(tag);
        raus.extend_from_slice(&pruef.to_be_bytes());
        raus.extend_from_slice(&(versatz as u32).to_be_bytes());
        raus.extend_from_slice(&(daten.len() as u32).to_be_bytes());
        versatz += daten.len().div_ceil(4) * 4;
    }
    for (_, daten, _) in &tabellen {
        raus.extend_from_slice(daten);
        while raus.len() % 4 != 0 {
            raus.push(0);
        }
    }
    Some(raus)
}

fn narsil_compress_inflate_zlib(roh: &[u8], erwartet: usize) -> Option<Vec<u8>> {
    let mut z = Vec::with_capacity(erwartet);
    narsil_compress::zlib_entpacken(roh, &mut z, erwartet.saturating_add(16)).ok()?;
    z.truncate(erwartet);
    Some(z)
}

/// Alle Webschriften abmelden (Tests).
pub fn leeren() {
    if let Ok(mut r) = REGISTER.write() {
        r.clear();
    }
    STAND.fetch_add(1, Ordering::Relaxed);
}
