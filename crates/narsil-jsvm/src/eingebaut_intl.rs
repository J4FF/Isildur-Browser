//! J-CACHE (04.10.2026): **Intl-Minimum als eigene Datei.**
//!
//! Auftrag Leitung (CACHE_24): `Intl.DateTimeFormat` (format,
//! formatToParts, resolvedOptions), `Intl.NumberFormat` (de-DE/en-US,
//! currency/percent), `Intl.PluralRules`, `Intl.Collator` (compare),
//! `Intl.RelativeTimeFormat` — Grundformate, Sonden gegen `node 22`.
//!
//! BAUWEISE: reine Funktionen ohne Engine (testbar ueber `--lib`);
//! die Nativ-Verdrahtung (globales `Intl`-Objekt, Konstruktoren) folgt
//! mit Freigabe. Jede Funktion traegt ihre node-Referenz als Test.
//! Referenzen ermittelt 04.10.2026 (Container-TZ, Datum 03.10.2026):
//!   de-date=3.10.2026  us-date=10/3/2026  de-ymd=03.10.2026
//!   us-full=Saturday, October 3, 2026
//!   de-num=1.234,5  us-num=1,234.5  de-eur=1.234,50 €  us-usd=$1,234.50
//!   de-pct=42 %  us-pct=42%  plural-de/en: 1->one, 2->other
//!   collator(ä,z)=-1  rel=vor 3 Tagen|in 2 days
//!
//! HINWEIS: `dead_code`-Warnungen bis zur Verdrahtung sind bekannt und
//! dokumentiert (CACHE_24) — kein Aufräumen nötig, die Funktionen sind
//! der Zweck dieser Datei.
#![allow(dead_code)]

/// Tage seit 1970-01-01 (civil -> days, Howard Hinnant). Heute ungenutzt
/// (Reserve fuer Datumsarithmetik); bewusst behalten.
#[allow(dead_code)]
fn tage_seit_epoche(j: i64, m: u32, t: u32) -> i64 {
    let jj = if m <= 2 { j - 1 } else { j };
    let era = jj.div_euclid(400);
    let yoe = jj - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + t as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Tage seit Epoche -> (Jahr, Monat, Tag) (civil_from_days).
fn zivil_aus_tagen(z: i64) -> (i64, u32, u32) {
    let zz = z + 719468;
    let era = zz.div_euclid(146097);
    let doe = zz - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let j = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let t = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { j + 1 } else { j }, m, t)
}

/// Millis -> lokale Wandzeit-Teile (Jahr, Monat, Tag, Std, Min, Sek).
/// Die Verschiebung liefert die Host-Zone (zone::verschiebung_s).
pub(crate) fn lokale_teile(millis: f64) -> (i64, u32, u32, u32, u32, u32) {
    let off = crate::zone::verschiebung_s(millis) as f64 * 1000.0;
    lokale_teile_off(millis, off)
}

/// Millis -> Wandzeit-Teile mit GEGEBENEM Offset (fuer timeZone-Option).
/// Teile mit optionalem Offset (None = Host-Zone, Some = timeZone).
fn teile(millis: f64, off: Option<f64>) -> (i64, u32, u32, u32, u32, u32) {
    match off {
        Some(o) => lokale_teile_off(millis, o),
        None => lokale_teile(millis),
    }
}

pub(crate) fn lokale_teile_off(millis: f64, off_ms: f64) -> (i64, u32, u32, u32, u32, u32) {
    let lokal = millis + off_ms;
    let tage = lokal.div_euclid(86_400_000.0) as i64;
    let rest = lokal.rem_euclid(86_400_000.0) as i64;
    let (j, mo, tg) = zivil_aus_tagen(tage);
    let std = (rest / 3_600_000) as u32;
    let min = ((rest % 3_600_000) / 60_000) as u32;
    let sek = ((rest % 60_000) / 1000) as u32;
    (j, mo, tg, std, min, sek)
}

/// Wochentag 0=Sonntag..6=Samstag aus Tagen seit Epoche (01.01.1970=Do=4).
fn wochentag(tage: i64) -> u32 {
    (tage + 4).rem_euclid(7) as u32
}

const TAGE_DE: [&str; 7] = [
    "Sonntag",
    "Montag",
    "Dienstag",
    "Mittwoch",
    "Donnerstag",
    "Freitag",
    "Samstag",
];
const TAGE_EN: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const MONATE_DE: [&str; 12] = [
    "Januar",
    "Februar",
    "März",
    "April",
    "Mai",
    "Juni",
    "Juli",
    "August",
    "September",
    "Oktober",
    "November",
    "Dezember",
];
const MONATE_EN: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const MONATE_KURZ_EN: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONATE_KURZ_DE: [&str; 12] = [
    "Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez",
];

/// Locale-Kennung auf de/en falten (de-DE/de/… -> de, Rest -> en).
pub(crate) fn sprache(locale: &str) -> &'static str {
    let k = locale.to_lowercase();
    let k = k.split(['-', '_']).next().unwrap_or("");
    if k == "de" { "de" } else { "en" }
}

fn zweistellig(n: u32) -> String {
    format!("{n:02}")
}

/// Datum Grundform: de „3.10.2026", en „10/3/2026" (lokale Zone).
pub(crate) fn datum_format(locale: &str, millis: f64, off: Option<f64>) -> String {
    let (j, mo, tg, _, _, _) = teile(millis, off);
    if sprache(locale) == "de" {
        format!("{tg}.{mo}.{j}")
    } else {
        format!("{mo}/{tg}/{j}")
    }
}

/// Datum mit Optionen (nur belegte Kombinationen der Suite/Realwege):
/// year/month/day jeweils "numeric" oder "2-digit".
pub(crate) fn datum_mit_optionen(
    locale: &str,
    millis: f64,
    jahr: &str,
    monat: &str,
    tag: &str,
    off: Option<f64>,
) -> String {
    let (j, mo, tg, _, _, _) = teile(millis, off);
    let fj = if jahr == "2-digit" {
        format!("{:02}", (j % 100 + 100) % 100)
    } else {
        format!("{j}")
    };
    let fmo = if monat == "2-digit" {
        zweistellig(mo)
    } else {
        format!("{mo}")
    };
    let ftg = if tag == "2-digit" {
        zweistellig(tg)
    } else {
        format!("{tg}")
    };
    if sprache(locale) == "de" {
        format!("{ftg}.{fmo}.{fj}")
    } else {
        format!("{fmo}/{ftg}/{fj}")
    }
}

/// Zeit Grundform: de „14:05:09", en „2:05:09 PM" (lokale Zone).
pub(crate) fn zeit_format(locale: &str, millis: f64, off: Option<f64>) -> String {
    let (_, _, _, std, min, sek) = teile(millis, off);
    if sprache(locale) == "de" {
        format!(
            "{}:{}:{}",
            zweistellig(std),
            zweistellig(min),
            zweistellig(sek)
        )
    } else {
        let (h12, ab) = match std {
            0 => (12, "AM"),
            1..=11 => (std, "AM"),
            12 => (12, "PM"),
            _ => (std - 12, "PM"),
        };
        format!("{h12}:{}:{} {ab}", zweistellig(min), zweistellig(sek))
    }
}

/// Wochentag lang zum Millis-Wert (lokal).
pub(crate) fn wochentag_lang(locale: &str, millis: f64, off: Option<f64>) -> String {
    let off_ms = match off {
        Some(o) => o,
        None => crate::zone::verschiebung_s(millis) as f64 * 1000.0,
    };
    let tage = (millis + off_ms).div_euclid(86_400_000.0) as i64;
    let w = wochentag(tage) as usize;
    if sprache(locale) == "de" {
        TAGE_DE[w].to_string()
    } else {
        TAGE_EN[w].to_string()
    }
}

/// Volldatum mit Wochentag/Monat lang: en „Saturday, October 3, 2026",
/// de „Samstag, 3. Oktober 2026" (node-Referenz en verifiziert; de analog).
pub(crate) fn datum_voll(locale: &str, millis: f64, off: Option<f64>) -> String {
    let (j, mo, tg, _, _, _) = teile(millis, off);
    if sprache(locale) == "de" {
        format!(
            "{}, {tg}. {} {j}",
            wochentag_lang(locale, millis, off),
            monat_lang(locale, mo)
        )
    } else {
        format!(
            "{}, {} {tg}, {j}",
            wochentag_lang(locale, millis, off),
            monat_lang(locale, mo)
        )
    }
}

/// Monat kurz (1-12): en „Oct", de „Okt" (node-Referenz medium-Format).
pub(crate) fn monat_kurz(locale: &str, monat: u32) -> String {
    let i = (monat.saturating_sub(1).min(11)) as usize;
    if sprache(locale) == "de" {
        MONATE_KURZ_DE[i].to_string()
    } else {
        MONATE_KURZ_EN[i].to_string()
    }
}

/// Monat lang (1-12) zum Locale.
pub(crate) fn monat_lang(locale: &str, monat: u32) -> String {
    let i = (monat.saturating_sub(1).min(11)) as usize;
    if sprache(locale) == "de" {
        MONATE_DE[i].to_string()
    } else {
        MONATE_EN[i].to_string()
    }
}

/// Zahl gruppieren: 1234.5 -> de „1.234,5", en „1,234.5".
/// `min_nach`/`max_nach`: Nachkommastellen (auf max runden half-up ueber
/// Format, min auffuellen). NaN/Infinity wie ToString.
pub(crate) fn zahl_format(locale: &str, n: f64, min_nach: usize, max_nach: usize) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    let neg = n < 0.0 || (n == 0.0 && 1.0 / n < 0.0);
    let v = n.abs();
    // Runden auf max_nach via Format (exakt), dann ggf. kuerzen/auffuellen.
    let s = format!("{v:.max_nach$}", max_nach = max_nach);
    let (vor, nach) = match s.split_once('.') {
        Some((a, b)) => (a, b),
        None => (s.as_str(), ""),
    };
    // Gruppieren (Tausender).
    let mut gruppe = String::new();
    for (i, c) in vor.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            gruppe.push(if sprache(locale) == "de" { '.' } else { ',' });
        }
        gruppe.push(c);
    }
    let vor: String = gruppe.chars().rev().collect();
    // Nachkomma kuerzen (rechts Nullen weg bis min_nach), dann auffuellen.
    let mut nach = nach.to_string();
    while nach.len() > min_nach && nach.ends_with('0') {
        nach.pop();
    }
    while nach.len() < min_nach {
        nach.push('0');
    }
    let (dez, nach) = if sprache(locale) == "de" {
        ("", nach)
    } else {
        ("", nach)
    };
    let _ = dez;
    let mut out = vor;
    if !nach.is_empty() {
        out.push(if sprache(locale) == "de" { ',' } else { '.' });
        out.push_str(&nach);
    }
    if neg {
        out = format!("-{out}");
    }
    out
}

/// Waehrung: de „1.234,50 €", en „$1,234.50" (Symbole, node-gemessen).
pub(crate) fn waehrung_format(locale: &str, code: &str, n: f64) -> String {
    fn symbol(code: &str) -> &str {
        match code {
            "EUR" => "€",
            "USD" => "$",
            "GBP" => "£",
            "JPY" => "¥",
            sonst => sonst,
        }
    }
    let betrag = zahl_format(locale, n, 2, 2);
    let sym = symbol(code);
    if sprache(locale) == "de" {
        format!("{betrag}\u{a0}{sym}")
    } else if sym.len() == code.len() {
        // Unbekannter Code: „CODE Betrag".
        format!("{code} {betrag}")
    } else {
        format!("{sym}{betrag}")
    }
}

/// Prozent: de „42 %", en „42%".
pub(crate) fn prozent_format(locale: &str, n: f64) -> String {
    let betrag = zahl_format(locale, n * 100.0, 0, 0);
    if sprache(locale) == "de" {
        format!("{betrag}\u{a0}%")
    } else {
        format!("{betrag}%")
    }
}

/// Plural-Grundregel (CLDR cardinal, de/en ohne Fraktionen): 1 -> one.
/// node-Referenz: de 1->one 2->other, en 1->one 2->other.
pub(crate) fn plural_waehlen(locale: &str, n: f64) -> &'static str {
    let _ = locale;
    if n == 1.0 { "one" } else { "other" }
}

/// Collator-Naeherung (Grundform): Kleinbuchstaben + deutsche Umlaut-
/// Faltung (ä->a, ö->o, ü->u, ß->ss), dann Byte-Vergleich. Echte CLDR-
/// Sortierung (ä=a, locale-Stufen) ist das nicht — dokumentiert.
/// node-Referenz: compare("ä","z")=-1, compare("a","a")=0.
pub(crate) fn collator_vergleichen(a: &str, b: &str) -> i32 {
    fn falten(s: &str) -> String {
        let k = s.to_lowercase();
        k.replace('ä', "a")
            .replace('ö', "o")
            .replace('ü', "u")
            .replace('ß', "ss")
    }
    let (fa, fb) = (falten(a), falten(b));
    if fa == fb {
        0
    } else if fa < fb {
        -1
    } else {
        1
    }
}

/// Relativzeit Grundform: de „vor 3 Tagen"/„in 3 Tagen", en „3 days
/// ago"/„in 3 days". Einheiten day/hour/minute/second/month/year.
pub(crate) fn relativ_format(locale: &str, wert: f64, einheit: &str) -> String {
    let n = wert.abs() as i64;
    let de = sprache(locale) == "de";
    let (einh, form) = match (einheit, n) {
        ("day", 1) => (if de { "Tag" } else { "day" }, "ein"),
        ("day", _) => (if de { "Tagen" } else { "days" }, "mehr"),
        ("hour", 1) => (if de { "Stunde" } else { "hour" }, "ein"),
        ("hour", _) => (if de { "Stunden" } else { "hours" }, "mehr"),
        ("minute", 1) => (if de { "Minute" } else { "minute" }, "ein"),
        ("minute", _) => (if de { "Minuten" } else { "minutes" }, "mehr"),
        ("second", 1) => (if de { "Sekunde" } else { "second" }, "ein"),
        ("second", _) => (if de { "Sekunden" } else { "seconds" }, "mehr"),
        ("month", 1) => (if de { "Monat" } else { "month" }, "ein"),
        ("month", _) => (if de { "Monaten" } else { "months" }, "mehr"),
        ("year", 1) => (if de { "Jahr" } else { "year" }, "ein"),
        (_, _) => (if de { "Jahren" } else { "years" }, "mehr"),
    };
    if de {
        if wert < 0.0 {
            if form == "ein" {
                format!("vor einem {einh}")
            } else {
                format!("vor {n} {einh}")
            }
        } else if form == "ein" {
            format!("in einem {einh}")
        } else {
            format!("in {n} {einh}")
        }
    } else if wert < 0.0 {
        if form == "ein" {
            format!("1 {einh} ago")
        } else {
            format!("{n} {einh} ago")
        }
    } else if form == "ein" {
        format!("in 1 {einh}")
    } else {
        format!("in {n} {einh}")
    }
}

/// dateStyle: full -> Volldatum, long -> „3. Oktober 2026"/"October 3,
/// 2026", medium -> „03.10.2026"/"Oct 3, 2026", short -> „03.10.26"/
/// "10/3/26" (node-Referenzen medium/full verifiziert).
pub(crate) fn datum_stil(locale: &str, millis: f64, stil: &str, off: Option<f64>) -> String {
    let (j, mo, tg, _, _, _) = teile(millis, off);
    match stil {
        "full" => datum_voll(locale, millis, off),
        "long" => {
            if sprache(locale) == "de" {
                format!("{tg}. {} {j}", monat_lang(locale, mo))
            } else {
                format!("{} {tg}, {j}", monat_lang(locale, mo))
            }
        }
        "short" => {
            let fj = format!("{:02}", (j % 100 + 100) % 100);
            datum_mit_optionen(locale, millis, "short", "numeric", "numeric", off)
                .replace(&format!("{j}"), &fj)
        }
        _ => {
            // medium (Vorgabe).
            if sprache(locale) == "de" {
                datum_mit_optionen(locale, millis, "numeric", "2-digit", "2-digit", off)
            } else {
                format!("{} {tg}, {j}", monat_kurz(locale, mo))
            }
        }
    }
}

/// timeStyle: medium/full/long -> Zeit Grundform, short -> ohne Sekunden.
pub(crate) fn zeit_stil(locale: &str, millis: f64, stil: &str, off: Option<f64>) -> String {
    let s = zeit_format(locale, millis, off);
    if stil == "short" {
        // Letztes ":SS" streichen (de) bzw. vor " AM/PM" (en).
        if sprache(locale) == "de" {
            s.rsplit_once(':').map(|(a, _)| a.to_string()).unwrap_or(s)
        } else if let Some((a, b)) = s.rsplit_once(' ') {
            a.rsplit_once(':')
                .map(|(x, _)| format!("{x} {b}"))
                .unwrap_or(s)
        } else {
            s
        }
    } else {
        s
    }
}

/// Locale kanonisieren („EN-us" -> „en-US"): Sprache klein, Region gross,
/// Rest Titel-Schreibung. node-Referenz getCanonicalLocales.
pub(crate) fn kanonisch(locale: &str) -> String {
    let mut teile = locale.split(['-', '_']);
    let mut out = teile.next().unwrap_or("").to_lowercase();
    for (i, t) in teile.enumerate() {
        out.push('-');
        if i == 0 && t.len() == 2 {
            out.push_str(&t.to_uppercase());
        } else if t.len() == 4 {
            let mut c = t.chars();
            if let Some(e) = c.next() {
                out.extend(e.to_uppercase());
                out.push_str(&c.as_str().to_lowercase());
            }
        } else {
            out.push_str(&t.to_lowercase());
        }
    }
    out
}

/// Bekannte Sprachen (Naeherung fuer supportedLocalesOf — dokumentiert).
const SPRACHEN: [&str; 24] = [
    "de", "en", "fr", "es", "it", "pt", "nl", "pl", "sv", "da", "fi", "no", "ru", "uk", "ja", "zh",
    "ko", "ar", "he", "tr", "el", "hu", "cs", "ro",
];

/// Locale gestuetzt? Sprache bekannt (Region egal).
pub(crate) fn gestuetzt(locale: &str) -> bool {
    let k = locale.to_lowercase();
    let s = k.split(['-', '_']).next().unwrap_or("");
    SPRACHEN.contains(&s)
}

// Intl-Engine-Teil (J-CACHE 04.10.2026, Freigabe Leitung).
// Aufbau + Handler; reine Formatierung oben in dieser Datei.
use crate::builtin::Nativ;
use crate::heap::{Eigenschaft, GVal, HObj, Heap, HeapRef};
use crate::interp::{Engine, Stoerung};

impl Engine {
    /// Globales `Intl`-Objekt mit Konstruktoren anlegen und binden.
    /// EINE Aufrufzeile in builtin.rs-aufbauen (Freigabe: nur Registrierung).
    pub(crate) fn intl_aufbauen(&mut self) -> Result<(), Stoerung> {
        let po = self.proto_objekt;
        let intl = self.neues_objekt(Some(po)).map_err(Stoerung::Fehler)?;
        self.wurzeln.push(GVal::Ref(intl));
        // Konstruktor + Proto + Methoden je Art.
        let bau = |e: &mut Engine,
                   name: &str,
                   ctor_nat: Nativ,
                   methoden: &[(&str, Nativ)]|
         -> Result<HeapRef, Stoerung> {
            let proto = e.neues_objekt(Some(po)).map_err(Stoerung::Fehler)?;
            // Release 15.10.2026: rooted — Intl is built lazily at run time
            // now, when a GC can run in between (the caller resets the roots).
            e.wurzeln.push(GVal::Ref(proto));
            for (mn, mnat) in methoden {
                let f = e.nativ_anlegen(mn, mnat.clone(), e.proto_funktion);
                e.setze_eigen(proto, mn, GVal::Ref(f));
            }
            let c = e.nativ_anlegen(name, ctor_nat, e.proto_funktion);
            e.wurzeln.push(GVal::Ref(c));
            e.nativ_prototyp(c, proto);
            // length der Konstruktoren (node: alle 0).
            e.setze_eigen(c, "length", GVal::Zahl(0.0));
            // prototype-Eigenschaft (nicht-aufzaehlbar wie ueblich).
            if let HObj::Nativ { eigenschaften, .. } | HObj::Funktion { eigenschaften, .. } =
                e.heap.holen_mut(c)
            {
                eigenschaften.insert(
                    "prototype".to_string(),
                    Eigenschaft {
                        wert: GVal::Ref(proto),
                        getter: None,
                        setter: None,
                        aufzählbar: false,
                        beschreibbar: false,
                        konfigurierbar: false,
                    },
                );
            }
            Ok(c)
        };
        let dtf = bau(
            self,
            "DateTimeFormat",
            Nativ::IntlDatumZeit,
            &[
                ("format", Nativ::IntlFormat),
                ("formatToParts", Nativ::IntlTeile),
                ("resolvedOptions", Nativ::IntlOptionen),
            ],
        )?;
        let nf = bau(
            self,
            "NumberFormat",
            Nativ::IntlZahl,
            &[
                ("format", Nativ::IntlFormat),
                ("formatToParts", Nativ::IntlTeile),
                ("resolvedOptions", Nativ::IntlOptionen),
            ],
        )?;
        let pr = bau(
            self,
            "PluralRules",
            Nativ::IntlPlural,
            &[
                ("select", Nativ::IntlWaehlen),
                ("resolvedOptions", Nativ::IntlOptionen),
            ],
        )?;
        let co = bau(
            self,
            "Collator",
            Nativ::IntlVergleicher,
            &[
                ("compare", Nativ::IntlVergleichen),
                ("resolvedOptions", Nativ::IntlOptionen),
            ],
        )?;
        let rf = bau(
            self,
            "RelativeTimeFormat",
            Nativ::IntlRelativ,
            &[
                ("format", Nativ::IntlFormat),
                ("formatToParts", Nativ::IntlTeile),
                ("resolvedOptions", Nativ::IntlOptionen),
            ],
        )?;
        let dn = bau(
            self,
            "DisplayNames",
            Nativ::IntlAnzeige,
            &[("of", Nativ::IntlAnzeigeVon)],
        )?;
        let lf = bau(
            self,
            "ListFormat",
            Nativ::IntlListe,
            &[
                ("format", Nativ::IntlFormat),
                ("formatToParts", Nativ::IntlTeile),
                ("resolvedOptions", Nativ::IntlOptionen),
            ],
        )?;
        // Statische: supportedLocalesOf je Konstruktor.
        for c in [dtf, nf, pr, co, rf, lf, dn] {
            let f = self.nativ_anlegen(
                "supportedLocalesOf",
                Nativ::IntlGestuetzt,
                self.proto_funktion,
            );
            self.setze_eigen(c, "supportedLocalesOf", GVal::Ref(f));
        }
        self.setze_eigen(intl, "DateTimeFormat", GVal::Ref(dtf));
        self.setze_eigen(intl, "NumberFormat", GVal::Ref(nf));
        self.setze_eigen(intl, "PluralRules", GVal::Ref(pr));
        self.setze_eigen(intl, "Collator", GVal::Ref(co));
        self.setze_eigen(intl, "RelativeTimeFormat", GVal::Ref(rf));
        self.setze_eigen(intl, "ListFormat", GVal::Ref(lf));
        self.setze_eigen(intl, "DisplayNames", GVal::Ref(dn));
        let gcl = self.nativ_anlegen(
            "getCanonicalLocales",
            Nativ::IntlKanonisch,
            self.proto_funktion,
        );
        self.setze_eigen(intl, "getCanonicalLocales", GVal::Ref(gcl));
        // Release 15.10.2026: `Intl[@@toStringTag]` is "Intl" (ECMA-402 8.1.1).
        // MEASURED before: `Object.prototype.toString.call(Intl)` was
        // "[object Object]" (node 22: "[object Intl]").
        let tag = self.bekanntes_symbol_text(Nativ::SymToStringTag)?;
        let t = self.text_wert("Intl").map_err(Stoerung::Fehler)?;
        self.es_eigen(intl, &tag, t, false, false, true);
        self.global_binden("Intl", GVal::Ref(intl));
        Ok(())
    }

    /// Locale-Arg (0) als kanonisierten Text, Vorgabe "en-US".
    fn intl_locale(&mut self, args: &[GVal]) -> String {
        match self.arg(args, 0) {
            GVal::Undefiniert => "en-US".to_string(),
            v => self
                .schluessel_text(v)
                .map(|s| crate::eingebaut_intl::kanonisch(&s))
                .unwrap_or_else(|_| "en-US".to_string()),
        }
    }

    /// Options-Text holen (options = args[1] als Objekt).
    fn intl_opt(&mut self, args: &[GVal], name: &str) -> Option<String> {
        match self.arg(args, 1) {
            GVal::Ref(_) => {
                let o = self.arg(args, 1);
                self.hole(o, name).ok().and_then(|v| match v {
                    GVal::Undefiniert => None,
                    x => self.schluessel_text(x).ok(),
                })
            }
            _ => None,
        }
    }

    /// Instanz anlegen: Nativ mit Proto + Options-Feld als ziel.
    fn intl_instanz(
        &mut self,
        art: u8,
        proto_von_ctor: HeapRef,
        locale: &str,
        extra: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let loc = self.text_wert(locale).map_err(Stoerung::Fehler)?;
        let mut werte = vec![GVal::Zahl(art as f64), loc];
        werte.extend_from_slice(extra);
        let ctx = self.feld_aus_werten(&werte)?;
        let obj = HObj::Nativ {
            name: "intl".to_string(),
            proto: Some(proto_von_ctor),
            ziel: Some(ctx),
            eigenschaften: Default::default(),
        };
        let kosten = Heap::kosten_fuer(&obj);
        let _ = self.vor_allok(kosten);
        let r = self.belege(obj, kosten);
        Ok(GVal::Ref(r))
    }

    /// Proto des Konstruktors lesen (ctor.prototype).
    fn intl_ctor_proto(&mut self, ctor_ref: HeapRef) -> Option<HeapRef> {
        match self.heap.holen(ctor_ref).clone() {
            HObj::Nativ { eigenschaften, .. } | HObj::Funktion { eigenschaften, .. } => {
                match eigenschaften.get("prototype") {
                    Some(e) => match e.wert {
                        GVal::Ref(p) => Some(p),
                        _ => None,
                    },
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Konstruktor-Aufruf (new ODER Call — Spec erlaubt beides): Art 0 =
    /// DateTimeFormat, 1 = NumberFormat, 2 = PluralRules, 3 = Collator,
    /// 4 = RelativeTimeFormat, 5 = ListFormat.
    pub(crate) fn n_intl_ctor(
        &mut self,
        ctor_ref: HeapRef,
        args: &[GVal],
        art: u8,
    ) -> Result<GVal, Stoerung> {
        let locale = self.intl_locale(args);
        let proto = self.intl_ctor_proto(ctor_ref).unwrap_or(self.proto_objekt);
        // J-CACHE (Weisung CACHE_29): DisplayNames (Art 6) speichert
        // type (language/region) als Extra-Index 0.
        let extra: Vec<GVal> = match art {
            6 => {
                let typ = self.intl_opt(args, "type").unwrap_or_else(|| "language".to_string());
                let t = self.text_wert(&typ).unwrap_or(GVal::Undefiniert);
                vec![t]
            }
            0 => {
                // dateStyle/timeStyle/year/month/day/weekday/hour/minute/second.
                let keys = [
                    "dateStyle",
                    "timeStyle",
                    "year",
                    "month",
                    "day",
                    "weekday",
                    "hour",
                    "minute",
                    "second",
                    "timeZone",
                ];
                let mut v = Vec::new();
                for k in keys {
                    let o = self.intl_opt(args, k);
                    v.push(match o {
                        Some(s) => self.text_wert(&s).unwrap_or(GVal::Undefiniert),
                        None => GVal::Undefiniert,
                    });
                }
                v
            }
            1 => {
                // J-CACHE (Weisung CACHE_29): notation/unit/signDisplay
                // dazu (Indizes 2/3/4; 0/1 bleiben style/currency).
                let style = self
                    .intl_opt(args, "style")
                    .unwrap_or_else(|| "decimal".to_string());
                let cur = self.intl_opt(args, "currency").unwrap_or_default();
                let nota = self.intl_opt(args, "notation").unwrap_or_default();
                let unit = self.intl_opt(args, "unit").unwrap_or_default();
                let sdis = self.intl_opt(args, "signDisplay").unwrap_or_default();
                let t = |e: &mut Engine, o: String| -> GVal {
                    if o.is_empty() {
                        GVal::Undefiniert
                    } else {
                        e.text_wert(&o).unwrap_or(GVal::Undefiniert)
                    }
                };
                // Borrow-Reihenfolge: erst lesen, dann bauen.
                let t1 = t(self, style);
                let t2 = t(self, cur);
                let t3 = t(self, nota);
                let t4 = t(self, unit);
                let t5 = t(self, sdis);
                vec![t1, t2, t3, t4, t5]
            }
            _ => Vec::new(),
        };
        self.intl_instanz(art, proto, &locale, &extra)
    }

    /// Instanz-Feld lesen: (art, locale, extra...).
    fn intl_ctx(&mut self, fn_ref: HeapRef) -> Option<(u8, String, Vec<GVal>)> {
        let ctx = match self.heap.holen(fn_ref) {
            HObj::Nativ { ziel: Some(z), .. } => *z,
            _ => return None,
        };
        let el = match self.heap.holen(ctx).clone() {
            HObj::Feld { elemente, .. } => elemente,
            _ => return None,
        };
        let art = match el.first() {
            Some(GVal::Zahl(n)) => *n as u8,
            _ => return None,
        };
        let locale = match el.get(1) {
            Some(v) => self
                .schluessel_text(*v)
                .unwrap_or_else(|_| "en-US".to_string()),
            None => "en-US".to_string(),
        };
        Some((art, locale, el.into_iter().skip(2).collect()))
    }

    /// format(this=Instanz, wert): je Art delegieren.
    pub(crate) fn n_intl_format(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let fr = match this {
            GVal::Ref(r) => r,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Intl-format braucht Objekt"),
                ));
            }
        };
        let (art, locale, extra) = match self.intl_ctx(fr) {
            Some(x) => x,
            None => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Intl-format korrupt"),
                ));
            }
        };
        let txt = |e: &mut Engine, g: &GVal| -> String {
            // J-CACHE: Undefiniert ist „keine Option", nicht der Text
            // „undefined" — sonst hinge die Zeit an jedem Datum.
            match *g {
                GVal::Undefiniert => String::new(),
                x => e.schluessel_text(x).unwrap_or_default(),
            }
        };
        let s = match art {
            0 => {
                // DateTimeFormat: Datum/Zeit nach Optionen.
                let m = match self.arg(args, 0) {
                    GVal::Undefiniert => crate::builtin3::jetzt_millis(),
                    v => self.zu_zahl(v)?,
                };
                let mut g = |i: usize| extra.get(i).map(|v| txt(self, v)).unwrap_or_default();
                let (ds, ts, y, mo, d, wd, h, mi, se) =
                    (g(0), g(1), g(2), g(3), g(4), g(5), g(6), g(7), g(8));
                // J-CACHE: timeZone-Option (IANA, eigener TZif-Leser) —
                // alle Format-Aufrufe unten laufen mit diesem Offset.
                let tz = g(9);
                let off = if tz.is_empty() || tz == "UTC" {
                    None
                } else {
                    crate::eingebaut_intl::zone_verschiebung(&tz, m)
                        .map(|s| s as f64 * 1000.0)
                };
                if !ds.is_empty() || !ts.is_empty() {
                    let dt = if !ds.is_empty() {
                        crate::eingebaut_intl::datum_stil(&locale, m, &ds, off)
                    } else {
                        String::new()
                    };
                    let tm = if !ts.is_empty() {
                        crate::eingebaut_intl::zeit_stil(&locale, m, &ts, off)
                    } else {
                        String::new()
                    };
                    if dt.is_empty() {
                        tm
                    } else if tm.is_empty() {
                        dt
                    } else {
                        format!("{dt}, {tm}")
                    }
                } else if !wd.is_empty() {
                    crate::eingebaut_intl::datum_voll(&locale, m, off)
                } else if !y.is_empty() || !mo.is_empty() || !d.is_empty() {
                    crate::eingebaut_intl::datum_mit_optionen(
                        &locale,
                        m,
                        if y.is_empty() { "numeric" } else { &y },
                        if mo.is_empty() { "numeric" } else { &mo },
                        if d.is_empty() { "numeric" } else { &d },
                        off,
                    )
                } else if !h.is_empty() || !mi.is_empty() || !se.is_empty() {
                    // J-CACHE: ohne second-Option keine Sekunden
                    // (timeZone-Stunde node-gleich: „9:00 AM").
                    if se.is_empty() {
                        crate::eingebaut_intl::zeit_stil(&locale, m, "short", off)
                    } else {
                        crate::eingebaut_intl::zeit_format(&locale, m, off)
                    }
                } else {
                    // Vorgabe: Datum Grundform (node dtf-call).
                    crate::eingebaut_intl::datum_format(&locale, m, off)
                }
            }
            1 => {
                // NumberFormat: decimal/currency/percent/compact/unit +
                // signDisplay (Weisung CACHE_29). Extra: [style, currency,
                // notation, unit, signDisplay].
                let n = self.zu_zahl(self.arg(args, 0))?;
                let style = extra.first().map(|v| txt(self, v)).unwrap_or_default();
                let cur = extra.get(1).map(|v| txt(self, v)).unwrap_or_default();
                let nota = extra.get(2).map(|v| txt(self, v)).unwrap_or_default();
                let unit = extra.get(3).map(|v| txt(self, v)).unwrap_or_default();
                let sd = extra.get(4).map(|v| txt(self, v)).unwrap_or_default();
                let mut s = if nota == "compact" {
                    crate::eingebaut_intl::kompakt(&locale, n)
                } else if style == "currency" {
                    crate::eingebaut_intl::waehrung_format(&locale, &cur, n)
                } else if style == "percent" {
                    crate::eingebaut_intl::prozent_format(&locale, n)
                } else if style == "unit" && !unit.is_empty() {
                    crate::eingebaut_intl::mass(&locale, &unit, n)
                } else {
                    crate::eingebaut_intl::zahl_format(&locale, n, 0, 3)
                };
                if !sd.is_empty()
                    && crate::eingebaut_intl::vorzeichen_bei(n, &sd)
                    && !s.starts_with('-')
                {
                    s = format!("+{s}");
                }
                s
            }
            4 => {
                // RelativeTimeFormat: (wert, einheit).
                let w = self.zu_zahl(self.arg(args, 0))?;
                let ein = match self.arg(args, 1) {
                    GVal::Undefiniert => "day".to_string(),
                    v => txt(self, &v),
                };
                crate::eingebaut_intl::relativ_format(&locale, w, &ein)
            }
            5 => {
                // ListFormat: Feld verbinden (de „ und ", en „, and ").
                let liste = match self.arg(args, 0) {
                    GVal::Ref(r) => match self.heap.holen(r).clone() {
                        HObj::Feld { elemente, .. } => {
                            elemente.iter().map(|v| txt(self, v)).collect::<Vec<_>>()
                        }
                        _ => vec![txt(self, &self.arg(args, 0))],
                    },
                    v => vec![txt(self, &v)],
                };
                let de = crate::eingebaut_intl::sprache(&locale) == "de";
                match liste.len() {
                    0 => String::new(),
                    1 => liste.into_iter().next().unwrap_or_default(),
                    _ => {
                        let (kopf, letzter) = liste.split_at(liste.len() - 1);
                        // J-CACHE: de ohne Oxford-Komma („A, B und C"),
                        // en mit („A, B, and C").
                        if de {
                            format!("{} und {}", kopf.join(", "), letzter.join(", "))
                        } else {
                            format!("{}, and {}", kopf.join(", "), letzter.join(", "))
                        }
                    }
                }
            }
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Intl-format unbekannte Art"),
                ));
            }
        };
        self.text_wert(&s).map_err(Stoerung::Fehler)
    }

    /// select/compare (PluralRules/Collator).
    pub(crate) fn n_intl_wahl(
        &mut self,
        this: GVal,
        args: &[GVal],
        vergleich: bool,
    ) -> Result<GVal, Stoerung> {
        let fr = match this {
            GVal::Ref(r) => r,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Intl braucht Objekt"),
                ));
            }
        };
        let (art, locale, _) = match self.intl_ctx(fr) {
            Some(x) => x,
            None => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Intl korrupt"),
                ));
            }
        };
        if vergleich || art == 3 {
            let a = self.schluessel_text(self.arg(args, 0)).unwrap_or_default();
            let b = self.schluessel_text(self.arg(args, 1)).unwrap_or_default();
            let _ = locale;
            return Ok(GVal::Zahl(
                crate::eingebaut_intl::collator_vergleichen(&a, &b) as f64,
            ));
        }
        let n = self.zu_zahl(self.arg(args, 0))?;
        let s = crate::eingebaut_intl::plural_waehlen(&locale, n);
        self.text_wert(s).map_err(Stoerung::Fehler)
    }

    /// DisplayNames.of(code): Anzeigename oder undefined.
    pub(crate) fn n_intl_anzeige_von(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let fr = match this {
            GVal::Ref(r) => r,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Intl braucht Objekt"),
                ))
            }
        };
        let (art, locale, extra) = match self.intl_ctx(fr) {
            Some(x) => x,
            None => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Intl korrupt"),
                ))
            }
        };
        let _ = art;
        let txt = |e: &mut Engine, g: &GVal| -> String {
            match *g {
                GVal::Undefiniert => String::new(),
                x => e.schluessel_text(x).unwrap_or_default(),
            }
        };
        let typ = extra.first().map(|v| txt(self, v)).unwrap_or_default();
        let typ = if typ.is_empty() { "language".to_string() } else { typ };
        let code = self.schluessel_text(self.arg(args, 0)).unwrap_or_default();
        // J-CACHE: unbekannter Code faellt auf sich selbst zurueck (node:
        // of("XX") -> "XX"), nicht undefined.
        match crate::eingebaut_intl::anzeige_name(&locale, &typ, &code) {
            Some(s) => self.text_wert(&s).map_err(Stoerung::Fehler),
            None => self.text_wert(&code).map_err(Stoerung::Fehler),
        }
    }

    /// formatToParts: Feld aus {type, value}-Objekten.
    pub(crate) fn n_intl_teile(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        // format wiederverwenden, dann nach Typen zerlegen (Naeherung:
        // Datum/Zahl-Teile aus der formatierten Zeichenkette ableiten ist
        // zerbrechlich — stattdessen baut DTF die Teile direkt).
        let fr = match this {
            GVal::Ref(r) => r,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Intl braucht Objekt"),
                ));
            }
        };
        let (art, locale, extra) = match self.intl_ctx(fr) {
            Some(x) => x,
            None => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Intl korrupt"),
                ));
            }
        };
        // Teile als (Typ, Wert)-Paare sammeln.
        let mut paare: Vec<(String, String)> = Vec::new();
        if art == 0 {
            let m = match self.arg(args, 0) {
                GVal::Undefiniert => crate::builtin3::jetzt_millis(),
                v => self.zu_zahl(v)?,
            };
            let txt = |e: &mut Engine, g: &GVal| -> String {
                // Wie in n_intl_format: Undefiniert ist keine Option.
                match *g {
                    GVal::Undefiniert => String::new(),
                    x => e.schluessel_text(x).unwrap_or_default(),
                }
            };
            let mut g = |i: usize| extra.get(i).map(|v| txt(self, v)).unwrap_or_default();
            let (y, mo, d) = (
                if g(2).is_empty() { "numeric" } else { &g(2) },
                if g(3).is_empty() { "numeric" } else { &g(3) },
                if g(4).is_empty() { "numeric" } else { &g(4) },
            );
            let _ = (y, mo, d);
            let (jj, mmo, tg, _, _, _) = crate::eingebaut_intl::lokale_teile(m);
            // Reihenfolge de: Tag.Monat.Jahr, sonst Monat/Tag/Jahr.
            let de = crate::eingebaut_intl::sprache(&locale) == "de";
            let fmo = if g(3) == "2-digit" {
                format!("{mmo:02}")
            } else {
                format!("{mmo}")
            };
            let ftg = if g(4) == "2-digit" {
                format!("{tg:02}")
            } else {
                format!("{tg}")
            };
            if de {
                paare.push(("day".to_string(), ftg));
                paare.push(("literal".to_string(), ".".to_string()));
                paare.push(("month".to_string(), fmo));
                paare.push(("literal".to_string(), ".".to_string()));
                paare.push(("year".to_string(), format!("{jj}")));
            } else {
                paare.push(("month".to_string(), fmo));
                paare.push(("literal".to_string(), "/".to_string()));
                paare.push(("day".to_string(), ftg));
                paare.push(("literal".to_string(), "/".to_string()));
                paare.push(("year".to_string(), format!("{jj}")));
            }
        } else {
            // NumberFormat/Andere: ein Teil "literal" mit format()-Wert.
            let fr = self.n_intl_format(this, args)?;
            let s = self.schluessel_text(fr).unwrap_or_default();
            paare.push(("literal".to_string(), s));
        }
        // Objekte {type, value} ins Feld.
        let mut werte: Vec<GVal> = Vec::new();
        for (typ, wert) in paare {
            let o = self
                .neues_objekt(Some(self.proto_objekt))
                .map_err(Stoerung::Fehler)?;
            let t1 = self.text_wert(&typ).map_err(Stoerung::Fehler)?;
            let t2 = self.text_wert(&wert).map_err(Stoerung::Fehler)?;
            self.setze(GVal::Ref(o), "type", t1)?;
            self.setze(GVal::Ref(o), "value", t2)?;
            werte.push(GVal::Ref(o));
        }
        let f = self.feld_aus_werten(&werte)?;
        Ok(GVal::Ref(f))
    }

    /// resolvedOptions: Objekt mit locale (+ Art-Kennung).
    pub(crate) fn n_intl_optionen(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let fr = match this {
            GVal::Ref(r) => r,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Intl braucht Objekt"),
                ));
            }
        };
        let (_, locale, _) = match self.intl_ctx(fr) {
            Some(x) => x,
            None => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Intl korrupt"),
                ));
            }
        };
        let o = self
            .neues_objekt(Some(self.proto_objekt))
            .map_err(Stoerung::Fehler)?;
        let t = self.text_wert(&locale).map_err(Stoerung::Fehler)?;
        self.setze(GVal::Ref(o), "locale", t)?;
        Ok(GVal::Ref(o))
    }

    /// supportedLocalesOf (statisch): Feld oder Text filtern.
    pub(crate) fn n_intl_gestuetzt(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let mut out: Vec<GVal> = Vec::new();
        let nimm = |e: &mut Engine, v: GVal, out: &mut Vec<GVal>| {
            if let Ok(s) = e.schluessel_text(v) {
                let k = crate::eingebaut_intl::kanonisch(&s);
                if crate::eingebaut_intl::gestuetzt(&k) {
                    if let Ok(t) = e.text_wert(&k) {
                        out.push(t);
                    }
                }
            }
        };
        match self.arg(args, 0) {
            GVal::Ref(r) => match self.heap.holen(r).clone() {
                HObj::Feld { elemente, .. } => {
                    for v in elemente {
                        nimm(self, v, &mut out);
                    }
                }
                _ => nimm(self, GVal::Ref(r), &mut out),
            },
            v => nimm(self, v, &mut out),
        }
        let f = self.feld_aus_werten(&out)?;
        Ok(GVal::Ref(f))
    }

    /// getCanonicalLocales: kanonisiertes Feld.
    pub(crate) fn n_intl_kanonisch(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let mut out: Vec<GVal> = Vec::new();
        let nimm = |e: &mut Engine, v: GVal, out: &mut Vec<GVal>| {
            if let Ok(s) = e.schluessel_text(v) {
                let k = crate::eingebaut_intl::kanonisch(&s);
                if let Ok(t) = e.text_wert(&k) {
                    out.push(t);
                }
            }
        };
        match self.arg(args, 0) {
            GVal::Ref(r) => match self.heap.holen(r).clone() {
                HObj::Feld { elemente, .. } => {
                    for v in elemente {
                        nimm(self, v, &mut out);
                    }
                }
                _ => nimm(self, GVal::Ref(r), &mut out),
            },
            GVal::Undefiniert => {}
            v => nimm(self, v, &mut out),
        }
        let f = self.feld_aus_werten(&out)?;
        Ok(GVal::Ref(f))
    }
}

/// Kompaktzahl: 1234567 -> de „1,2 Mio.", en „1.2M" (node-Referenz).
/// Schwellen 1e3/1e6/1e9/1e12, eine Nachkommastelle (weg bei .0).
pub(crate) fn kompakt(locale: &str, n: f64) -> String {
    if !n.is_finite() {
        return zahl_format(locale, n, 0, 3);
    }
    let v = n.abs();
    let de = sprache(locale) == "de";
    let (teiler, kurz_de, kurz_en) = if v >= 1e12 {
        (1e12, "Bio.", "T")
    } else if v >= 1e9 {
        (1e9, "Mrd.", "B")
    } else if v >= 1e6 {
        (1e6, "Mio.", "M")
    } else if v >= 1e3 {
        (1e3, "Tsd.", "K")
    } else {
        return zahl_format(locale, n, 0, 3);
    };
    // Eine Nachkommastelle, half-up ueber Format (wie expo_text).
    let s = format!("{:.1}", v / teiler);
    let s = s.strip_suffix(".0").or(s.strip_suffix(",0")).unwrap_or(&s);
    // Dezimaltrennzeichen der Locale.
    let s = if de { s.replace('.', ",") } else { s.to_string() };
    let k = if de { kurz_de } else { kurz_en };
    let vor = if n < 0.0 { "-" } else { "" };
    if de {
        format!("{vor}{s} {k}")
    } else {
        format!("{vor}{s}{k}")
    }
}

/// Einheiten-Kuerzel (CLDR-Grundformen, dokumentierte Naeherung).
fn einheit_kuerzel(locale: &str, einheit: &str) -> String {
    let de = sprache(locale) == "de";
    match einheit {
        "kilometer-per-hour" => "km/h".to_string(),
        "meter-per-second" => {
            if de {
                "m/s".to_string()
            } else {
                "m/s".to_string()
            }
        }
        "kilometer" => "km".to_string(),
        "meter" => "m".to_string(),
        "centimeter" => {
            if de {
                "cm".to_string()
            } else {
                "cm".to_string()
            }
        }
        "millimeter" => "mm".to_string(),
        "gram" => "g".to_string(),
        "kilogram" => "kg".to_string(),
        "liter" => {
            if de {
                "l".to_string()
            } else {
                "L".to_string()
            }
        }
        "milliliter" => "ml".to_string(),
        "byte" => "Byte".to_string(),
        "percent" => "%".to_string(),
        "celsius" => {
            if de {
                "°C".to_string()
            } else {
                "°C".to_string()
            }
        }
        "hour" => {
            if de {
                "Std.".to_string()
            } else {
                "hr".to_string()
            }
        }
        "minute" => {
            if de {
                "Min.".to_string()
            } else {
                "min".to_string()
            }
        }
        "second" => {
            if de {
                "Sek.".to_string()
            } else {
                "sec".to_string()
            }
        }
        "day" => {
            if de {
                "Tg.".to_string()
            } else {
                "days".to_string()
            }
        }
        _ => einheit.to_string(),
    }
}

/// Masszahl: de „123 km/h" (node-Referenz).
pub(crate) fn mass(locale: &str, einheit: &str, n: f64) -> String {
    let betrag = zahl_format(locale, n, 0, 3);
    let k = einheit_kuerzel(locale, einheit);
    if sprache(locale) == "de" {
        format!("{betrag} {k}")
    } else {
        format!("{betrag} {k}")
    }
}

/// Vorzeichen (signDisplay): always -> immer, exceptZero -> ausser 0,
/// never -> nie. node-Referenz: always+42 -> „+42".
pub(crate) fn vorzeichen_bei(n: f64, art: &str) -> bool {
    match art {
        "always" => true,
        "exceptZero" => n != 0.0,
        "never" => false,
        _ => n < 0.0,
    }
}

/// Anzeigenamen minimal (de/en): Sprachen (~16) + Regionen (~16).
/// node-Referenz: de/en -> „Englisch", en/DE -> „Germany".
pub(crate) fn anzeige_name(locale: &str, art: &str, code: &str) -> Option<String> {
    const SPRACHEN: [(&str, &str, &str); 16] = [
        ("de", "Deutsch", "German"),
        ("en", "Englisch", "English"),
        ("fr", "Französisch", "French"),
        ("es", "Spanisch", "Spanish"),
        ("it", "Italienisch", "Italian"),
        ("pt", "Portugiesisch", "Portuguese"),
        ("nl", "Niederländisch", "Dutch"),
        ("pl", "Polnisch", "Polish"),
        ("ru", "Russisch", "Russian"),
        ("ja", "Japanisch", "Japanese"),
        ("zh", "Chinesisch", "Chinese"),
        ("ko", "Koreanisch", "Korean"),
        ("ar", "Arabisch", "Arabic"),
        ("tr", "Türkisch", "Turkish"),
        ("el", "Griechisch", "Greek"),
        ("hi", "Hindi", "Hindi"),
    ];
    const REGIONEN: [(&str, &str, &str); 16] = [
        ("DE", "Deutschland", "Germany"),
        ("US", "Vereinigte Staaten", "United States"),
        ("FR", "Frankreich", "France"),
        ("ES", "Spanien", "Spain"),
        ("IT", "Italien", "Italy"),
        ("GB", "Vereinigtes Königreich", "United Kingdom"),
        ("JP", "Japan", "Japan"),
        ("CN", "China", "China"),
        ("AT", "Österreich", "Austria"),
        ("CH", "Schweiz", "Switzerland"),
        ("NL", "Niederlande", "Netherlands"),
        ("PL", "Polen", "Poland"),
        ("RU", "Russland", "Russia"),
        ("BR", "Brasilien", "Brazil"),
        ("IN", "Indien", "India"),
        ("CA", "Kanada", "Canada"),
    ];
    let de = sprache(locale) == "de";
    let k = if art == "region" { code.to_uppercase() } else { code.to_lowercase() };
    let tabelle = if art == "region" { &REGIONEN[..] } else { &SPRACHEN[..] };
    for (c, d, e) in tabelle {
        if *c == k {
            return Some(if de { d.to_string() } else { e.to_string() });
        }
    }
    None
}

/// Graphem-Naeherung (Segmenter): Kombinierende Zeichen (U+0300–U+036F,
/// U+1AB0–U+1AFF, U+1DC0–U+1DFF, U+20D0–U+20FF, U+FE20–U+FE2F) haengen am
/// Vorzeichen. HINWEIS: Rust-`char` kennt keine Surrogate (astrale Zeichen
/// sind bereits EIN char) — kein Ersatzpaar-Code noetig. Echte UAX-29-Regeln
/// (Emoji-ZWJ, Regional-Indikatoren, Hangul) NICHT — dokumentiert.
pub(crate) fn grapheme_teilen(text: &str) -> Vec<String> {
    let komb = |c: char| -> bool {
        matches!(c,
            '\u{300}'..='\u{36f}' | '\u{1ab0}'..='\u{1aff}'
            | '\u{1dc0}'..='\u{1dff}' | '\u{20d0}'..='\u{20ff}'
            | '\u{fe20}'..='\u{fe2f}')
    };
    let mut out: Vec<String> = Vec::new();
    let mut laeufer = String::new();
    for c in text.chars() {
        if !laeufer.is_empty() && komb(c) {
            laeufer.push(c);
            continue;
        }
        if !laeufer.is_empty() {
            out.push(std::mem::take(&mut laeufer));
        }
        laeufer.push(c);
    }
    if !laeufer.is_empty() {
        out.push(laeufer);
    }
    out
}

/// Named IANA zone through the shared 64-bit TZif reader and portable fallback.
pub(crate) fn zone_verschiebung(name: &str, zeit_ms: f64) -> Option<i64> {
    crate::zone::benannte_verschiebung(name, zeit_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    // node-Referenzen vom 04.10.2026 (Container-TZ, Datum 03.10.2026).
    const OKT3_1405_UTC: f64 = 1791026709000.0;

    #[test]
    fn datum_grundformen() {
        assert_eq!(datum_format("de-DE", OKT3_1405_UTC, None), "3.10.2026");
        assert_eq!(datum_format("en-US", OKT3_1405_UTC, None), "10/3/2026");
        assert_eq!(
            datum_mit_optionen("de-DE", OKT3_1405_UTC, "numeric", "2-digit", "2-digit", None),
            "03.10.2026"
        );
    }

    #[test]
    fn zeit_grundformen() {
        // 14:05:09 UTC — in TZ westlich abweichend; die Trennung AM/PM
        // und die de-Form sind zonenstabil an diesem Datum.
        let z = zeit_format("de-DE", OKT3_1405_UTC, None);
        assert!(z.len() == 8 && z.chars().nth(2) == Some(':'));
        let u = zeit_format("en-US", OKT3_1405_UTC, None);
        assert!(u.ends_with("AM") || u.ends_with("PM"));
    }

    #[test]
    fn wochentag_monat() {
        // 03.10.2026 ist ein Samstag (node: Saturday/Oktober/October).
        assert_eq!(wochentag_lang("en-US", OKT3_1405_UTC, None), "Saturday");
        assert_eq!(wochentag_lang("de-DE", OKT3_1405_UTC, None), "Samstag");
        assert_eq!(monat_lang("en-US", 10), "October");
        assert_eq!(monat_lang("de-DE", 10), "Oktober");
        assert_eq!(
            datum_voll("en-US", OKT3_1405_UTC, None),
            "Saturday, October 3, 2026"
        );
    }

    #[test]
    fn zahlen() {
        assert_eq!(zahl_format("de-DE", 1234.5, 0, 3), "1.234,5");
        assert_eq!(zahl_format("en-US", 1234.5, 0, 3), "1,234.5");
        assert_eq!(zahl_format("de-DE", 1234.5, 2, 2), "1.234,50");
        assert_eq!(zahl_format("en-US", 1234.5, 2, 2), "1,234.50");
    }

    #[test]
    fn waehrung_prozent() {
        assert_eq!(waehrung_format("de-DE", "EUR", 1234.5), "1.234,50\u{a0}€");
        assert_eq!(waehrung_format("en-US", "USD", 1234.5), "$1,234.50");
        assert_eq!(prozent_format("de-DE", 0.42), "42\u{a0}%");
        assert_eq!(prozent_format("en-US", 0.42), "42%");
    }

    #[test]
    fn zone_tokyo() {
        // Asia/Tokyo = +9h ganzjaehrig (node: 9:00 AM bei 00:00 UTC).
        let ms = 1767225600000.0; // 2026-01-01T00:00:00Z
        assert_eq!(zone_verschiebung("Asia/Tokyo", ms), Some(9 * 3600));
        assert_eq!(zone_verschiebung("../etc/passwd", ms), None);
        assert_eq!(zone_verschiebung("Xx/Nein", ms), None);
    }

    #[test]
    fn luecken_compact_unit_anzeige() {
        // node-Referenzen 04.10.2026.
        assert_eq!(kompakt("de", 1234567.0), "1,2 Mio.");
        assert_eq!(kompakt("en", 1234567.0), "1.2M");
        assert_eq!(kompakt("de", 1000000.0), "1 Mio.");
        assert_eq!(mass("de-DE", "kilometer-per-hour", 123.0), "123 km/h");
        assert!(vorzeichen_bei(42.0, "always"));
        assert!(!vorzeichen_bei(0.0, "exceptZero"));
        assert!(!vorzeichen_bei(42.0, "never"));
        assert_eq!(anzeige_name("de", "language", "en"), Some("Englisch".to_string()));
        assert_eq!(anzeige_name("en", "region", "DE"), Some("Germany".to_string()));
        assert_eq!(anzeige_name("de", "region", "XX"), None);
        assert_eq!(grapheme_teilen("a\u{301}b"), vec!["a\u{301}".to_string(), "b".to_string()]);
        assert_eq!(grapheme_teilen("hi"), vec!["h".to_string(), "i".to_string()]);
    }

    #[test]
    fn stile_kanon() {
        // node: medium en "Oct 3, 2026", de "03.10.2026"; full en s. oben.
        assert_eq!(datum_stil("en-US", OKT3_1405_UTC, "medium", None), "Oct 3, 2026");
        assert_eq!(datum_stil("de-DE", OKT3_1405_UTC, "medium", None), "03.10.2026");
        assert_eq!(kanonisch("EN-us"), "en-US");
        assert_eq!(kanonisch("de-de"), "de-DE");
        assert!(gestuetzt("de-DE"));
        assert!(!gestuetzt("xx-YY"));
    }

    #[test]
    fn plural_collator_relativ() {
        assert_eq!(plural_waehlen("de", 1.0), "one");
        assert_eq!(plural_waehlen("de", 2.0), "other");
        assert_eq!(plural_waehlen("en", 1.0), "one");
        assert_eq!(collator_vergleichen("ä", "z"), -1);
        assert_eq!(collator_vergleichen("a", "a"), 0);
        assert_eq!(relativ_format("de", -3.0, "day"), "vor 3 Tagen");
        assert_eq!(relativ_format("en", 2.0, "day"), "in 2 days");
    }
}
