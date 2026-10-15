//! Die Regeln aus RFC 9111 — geparst, nicht behauptet.
//!
//! Drei Fragen beantwortet dieses Modul:
//!
//! 1. **Darf die Antwort gespeichert werden?** (RFC 9111, Abschnitt 3)
//! 2. **Wie lange ist sie frisch?** (RFC 9111, Abschnitt 4.2.1)
//! 3. **Passt eine gespeicherte Antwort zu einer Anfrage?** (Vary,
//!    RFC 9111, Abschnitt 4.1)
//!
//! Was hier bewusst NICHT drin ist: `stale-while-revalidate` und
//! `stale-if-error` werden geparst, aber nicht ausgewertet. Dieser Cache
//! bedient eine veraltete Antwort NIE ohne erneute Pruefung — strenger als
//! das RFC verlangt, aber nie falsch. `must-revalidate` ist damit
//! automatisch erfuellt.
//!
//! Die Funktionen nehmen Header-Maps und Statuscodes statt `Response`,
//! damit derselbe Code fuer eine frische Antwort und einen gespeicherten
//! Eintrag gilt.

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use crate::{Anfrage, Methode};

/// Obergrenze der Cache-Control-Direktiven je Header (M3).
///
/// Ein Header mit mehr Direktiven als das ist kein normaler Server, sondern
/// ein Versuch, die Pars-Schleife lange laufen zu lassen. Die Zahl ist
/// willkuehrlich hoch genug fuer jeden echten Server.
const MAX_DIREKTIVE: usize = 64;

/// Obergrenze der Vary-Eintraege je Header (M3).
const MAX_VARY: usize = 32;

/// Obergrenze eines Header-Werts in Byte (M4).
///
/// Ein `Cache-Control` mit 100 KB ist kein Cache-Control. Die Grenze steht
/// VOR jedem `with_capacity`.
const MAX_WERT: usize = 16 * 1024;

/// Statuscodes, die standardmaessig cachebar sind (RFC 9111, Abschnitt 3).
///
/// Alles andere wird nicht gespeichert, es sei denn, der Server sagt
/// ausdruecklich `public` oder nennt eine Frische.
const STATUS_DEFAULT_CACHEBAR: &[u16] = &[200, 203, 204, 206, 300, 301, 404, 405, 410, 414, 501];

/// Geparste Cache-Control-Direktiven einer Antwort.
///
/// `None` bei `max_age` heisst: der Server hat kein `max-age` gesendet —
/// nicht "unendlich". Der Unterschied ist der Unterschied zwischen einer
/// Angabe und einer Luecke.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Steuerung {
    /// `max-age=N` — Frische in Sekunden.
    pub max_age: Option<Duration>,
    /// `s-maxage=N` — nur fuer geteilte Caches (RFC 9111, 5.2.2.10).
    pub s_maxage: Option<Duration>,
    /// `no-store` — nicht speichern (RFC 9111, 5.2.2.5).
    pub no_store: bool,
    /// `no-cache` — speichern, aber immer neu pruefen (RFC 9111, 5.2.2.4).
    pub no_cache: bool,
    /// `private` — nur der Nutzer-Cache darf speichern (RFC 9111, 5.2.2.7).
    pub private: bool,
    /// `public` — darf auch hinter `Authorization` gespeichert werden.
    pub public: bool,
    /// `must-revalidate` — veraltet, dann neu pruefen (RFC 9111, 5.2.2.1).
    pub must_revalidate: bool,
    /// `proxy-revalidate` — dasselbe, nur fuer geteilte Caches.
    pub proxy_revalidate: bool,
    /// `immutable` — der Koerper aendert sich nicht, solange er frisch ist.
    pub immutable: bool,
    /// `stale-while-revalidate=N` — geparst, nicht ausgewertet (siehe Modul).
    pub stale_while_revalidate: Option<Duration>,
    /// `stale-if-error=N` — geparst, nicht ausgewertet (siehe Modul).
    pub stale_if_error: Option<Duration>,
}

/// Cache-Control aus einer Header-Map lesen.
///
/// Unbekannte Direktiven werden ignoriert (RFC 9111, Abschnitt 5.2: "A cache
/// MUST ignore unrecognized directives"). Ein kaputtes `max-age=abc` ist
/// kein `max-age` — die Direktive gilt als nicht gesendet.
pub fn steuerung(headers: &HashMap<String, String>) -> Steuerung {
    let Some(wert) = headers.get("cache-control") else {
        return Steuerung::default();
    };
    // M4: ein Header-Wert ist begrenzt, bevor er verarbeitet wird.
    let wert = &wert[..wert.len().min(MAX_WERT)];
    let mut s = Steuerung::default();
    for (i, teil) in wert.split(',').enumerate() {
        if i >= MAX_DIREKTIVE {
            break;
        }
        let teil = teil.trim();
        if teil.is_empty() {
            continue;
        }
        // Direktive mit Wert: `max-age=300`. Ohne Wert: `no-store`.
        let (name, wert) = match teil.split_once('=') {
            Some((n, w)) => (n.trim().to_ascii_lowercase(), Some(w.trim())),
            None => (teil.to_ascii_lowercase(), None),
        };
        let sekunden = |w: Option<&str>| -> Option<Duration> {
            let z = w?.trim_matches('"').parse::<u64>().ok()?;
            Some(Duration::from_secs(z))
        };
        match name.as_str() {
            "max-age" => s.max_age = sekunden(wert),
            "s-maxage" => s.s_maxage = sekunden(wert),
            "no-store" => s.no_store = true,
            "no-cache" => s.no_cache = true,
            "private" => s.private = true,
            "public" => s.public = true,
            "must-revalidate" => s.must_revalidate = true,
            "proxy-revalidate" => s.proxy_revalidate = true,
            "immutable" => s.immutable = true,
            "stale-while-revalidate" => s.stale_while_revalidate = sekunden(wert),
            "stale-if-error" => s.stale_if_error = sekunden(wert),
            _ => {} // Unbekannt: ignorieren, nicht raten.
        }
    }
    s
}

/// `no-store` in der ANFRAGE? Dann darf die Antwort nicht gespeichert
/// werden (RFC 9111, Abschnitt 5.2.2.5 — gilt fuer Anfrage und Antwort).
pub fn anfrage_speichert_nicht(anfrage: &Anfrage) -> bool {
    anfrage
        .headers
        .get("cache-control")
        .map(|w| {
            w.split(',')
                .any(|t| t.trim().eq_ignore_ascii_case("no-store"))
        })
        .unwrap_or(false)
}

/// Darf diese Antwort gespeichert werden? (RFC 9111, Abschnitt 3)
///
/// Die Reihenfolge ist die des RFC: Methode, Status, dann die Ausnahmen.
pub fn ist_cachebar(
    anfrage: &Anfrage,
    headers: &HashMap<String, String>,
    status: u16,
    geteilt: bool,
) -> bool {
    // Nur GET wird gespeichert. HEAD hat keinen Koerper, POST ist keine
    // Cache-Methode (RFC 9111, Abschnitt 3).
    if anfrage.methode != Methode::Get {
        return false;
    }
    if !STATUS_DEFAULT_CACHEBAR.contains(&status) {
        return false;
    }
    let s = steuerung(headers);
    if s.no_store || anfrage_speichert_nicht(anfrage) {
        return false;
    }
    // `private`: ein geteilter Cache darf das nicht speichern. Ein
    // Nutzer-Cache (geteilt == false) darf es — das ist sein Zweck.
    if s.private && geteilt {
        return false;
    }
    // `Authorization` in der Anfrage: nur mit ausdruecklicher Erlaubnis
    // (RFC 9111, Abschnitt 3: "public", "s-maxage" oder "max-age").
    if anfrage.headers.contains_key("authorization")
        && !s.public
        && s.max_age.is_none()
        && s.s_maxage.is_none()
    {
        return false;
    }
    true
}

/// HTTP-Datum lesen — nur IMF-fixdate (RFC 9111, Abschnitt 5.3).
///
/// Das ist das einzige Format, das ein Server nach RFC 7231 senden DARF. Die
/// beiden veralteten Formate (RFC 850, asctime) werden nicht geparst: sie
/// gelten als nicht vorhanden, und dann faellt man auf die Heuristik
/// zurueck. Das ist erlaubt — die Heuristik ist genau dafuer da.
pub fn http_datum(text: &str) -> Option<SystemTime> {
    // "Sun, 06 Nov 1994 08:49:37 GMT"
    let teile: Vec<&str> = text.split_whitespace().collect();
    if teile.len() != 6 {
        return None;
    }
    let tag: u32 = teile[1].parse().ok()?;
    let monat = match teile[2] {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let jahr: i64 = teile[3].parse().ok()?;
    let zeit: Vec<&str> = teile[4].split(':').collect();
    if zeit.len() != 3 {
        return None;
    }
    let h: u32 = zeit[0].parse().ok()?;
    let m: u32 = zeit[1].parse().ok()?;
    let s: u32 = zeit[2].parse().ok()?;
    if teile[5] != "GMT" {
        return None;
    }
    if tag == 0 || h > 23 || m > 59 || s > 60 {
        return None; // 60 = Schaltsekunde, erlaubt
    }
    Some(unix_zeit(jahr, monat, tag, h, m, s))
}

/// SystemTime als IMF-fixdate schreiben (RFC 9111, Abschnitt 5.3).
pub fn schreibe_datum(zeit: SystemTime) -> String {
    let sekunden = zeit
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let tage = sekunden.div_euclid(86400);
    let rest = sekunden.rem_euclid(86400);
    let (jahr, monat, tag) = zivil_von_tagen(tage);
    // 1970-01-01 war ein Donnerstag: (tage + 4) % 7, 0 = Sonntag.
    let wochentag =
        ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][(tage.rem_euclid(7) + 4) as usize % 7];
    let monatsname = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ][(monat - 1) as usize];
    format!(
        "{wochentag}, {tag:02} {monatsname} {jahr:04} {:02}:{:02}:{:02} GMT",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    )
}

/// Tage seit 1970-01-01 aus Kalenderdatum (Howard Hinnants Algorithmus).
///
/// Der Algorithmus verlegt das Jahresanfang auf Maerz, damit der
/// SchaltTag innerhalb des Jahres an der gleichen Stelle des
/// 153-Tage-Zyklus liegt. Fuer Jan/Feb wird das Jahr um eins zurueck-
/// gesetzt und der Monat um 9 erhoeht (Jan wird 10, Feb wird 11).
fn tage_von_zivil(jahr: i64, monat: u32, tag: u32) -> i64 {
    let j = jahr - i64::from(monat <= 2);
    let era = j.div_euclid(400);
    let yoe = j - era * 400; // [0, 399]
    let m = if monat <= 2 { monat + 9 } else { monat - 3 } as i64;
    let doy = (153 * m + 2) / 5 + i64::from(tag) - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

/// Kalenderdatum aus Tagen seit 1970-01-01.
fn zivil_von_tagen(tage: i64) -> (i64, u32, u32) {
    let z = tage + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let tag = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let monat = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if monat <= 2 { y + 1 } else { y }, monat, tag)
}

fn unix_zeit(jahr: i64, monat: u32, tag: u32, h: u32, m: u32, s: u32) -> SystemTime {
    let tage = tage_von_zivil(jahr, monat, tag);
    let sek = tage * 86_400 + i64::from(h) * 3600 + i64::from(m) * 60 + i64::from(s);
    SystemTime::UNIX_EPOCH + Duration::from_secs(sek.max(0) as u64)
}

/// Frische-Lebensdauer einer Antwort (RFC 9111, Abschnitt 4.2.1).
///
/// `None` heisst: der Server hat keine Frische gesagt. Der Cache darf dann
/// NICHT raten — ausser die Heuristik (unten) traegt.
pub fn frischheit(
    headers: &HashMap<String, String>,
    status: u16,
    empfangen: SystemTime,
    jetzt: SystemTime,
    geteilt: bool,
) -> Option<Duration> {
    let s = steuerung(headers);
    // `no-cache`: speichern, aber nie frisch. `Some(ZERO)` — nicht `None`,
    // sonst wuerde die Heuristik greifen.
    if s.no_cache {
        return Some(Duration::ZERO);
    }
    // `max-age` gewinnt ueber alles andere (RFC 9111, 5.2.2.9).
    if let Some(m) = s.max_age {
        return Some(m);
    }
    // `s-maxage` nur in einem geteilten Cache (RFC 9111, 5.2.2.10).
    if geteilt {
        if let Some(m) = s.s_maxage {
            return Some(m);
        }
    }
    // `Expires`: absolute Grenze.
    if let Some(expires) = headers.get("expires").and_then(|w| http_datum(w)) {
        if let Ok(dauer) = expires.duration_since(jetzt) {
            return Some(dauer);
        }
        // In der Vergangenheit: sofort veraltet.
        return Some(Duration::ZERO);
    }
    // Heuristik (RFC 9111, 4.2.2): 10% der Zeit seit `Last-Modified`.
    // Nur wenn der Status cachebar ist — sonst wuerde die Heuristik
    // genau die Antwort frisch halten, die der Server nicht speichern
    // liess.
    //
    // Die Zeitbasis ist der EMPFANG, nicht `jetzt`: die Heuristik bemisst,
    // wie lange der Server die Datei schon hatte, als er sie schickte.
    // Gemessen am Empfang ist die Frischheit unabhaengig davon, wann der
    // Cache sie sich ansieht — und genau das will der RFC.
    if !STATUS_DEFAULT_CACHEBAR.contains(&status) {
        return None;
    }
    if let Some(lm) = headers.get("last-modified").and_then(|w| http_datum(w)) {
        if let Ok(alt) = empfangen.duration_since(lm) {
            return Some(alt / 10);
        }
    }
    None
}

/// Wie alt ist die Antwort JETZT? (RFC 9111, Abschnitt 4.2.3, vereinfacht)
///
/// `Age`-Header gewinnt (der Server hat den Weg durch seine eigenen Caches
/// gemessen). Sonst: `Date`-Header gegen die Empfangszeit. Ohne beides:
/// die Zeit seit dem Empfang.
pub fn aktuelle_alter(
    headers: &HashMap<String, String>,
    empfangen: SystemTime,
    jetzt: SystemTime,
) -> Duration {
    if let Some(a) = headers
        .get("age")
        .and_then(|w| w.trim().parse::<u64>().ok())
    {
        return Duration::from_secs(a);
    }
    if let Some(d) = headers.get("date").and_then(|w| http_datum(w)) {
        if let Ok(alt) = empfangen.duration_since(d) {
            let resident = jetzt.duration_since(empfangen).unwrap_or(Duration::ZERO);
            return alt.max(resident);
        }
    }
    jetzt.duration_since(empfangen).unwrap_or(Duration::ZERO)
}

/// Die Vary-Namen aus der Antwort (RFC 9111, Abschnitt 4.1).
///
/// `Vary: *` wird als `["*"]` zurueckgegeben — der Aufrufer entscheidet.
pub fn vary_namen(headers: &HashMap<String, String>) -> Vec<String> {
    let Some(wert) = headers.get("vary") else {
        return Vec::new();
    };
    let wert = &wert[..wert.len().min(MAX_WERT)];
    wert.split(',')
        .take(MAX_VARY)
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Die Request-Header, die zu einem Vary gespeichert werden muessen.
///
/// `Vary: *` speichert ALLE Header der Anfrage — dann passt die Antwort nur
/// zu einer identischen Anfrage, was genau das ist, was `*` sagt.
pub fn vary_speichern(anfrage: &Anfrage, namen: &[String]) -> Vec<(String, String)> {
    if namen.iter().any(|n| n == "*") {
        return anfrage
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
    }
    let mut raus = Vec::new();
    for name in namen {
        if let Some(wert) = anfrage.headers.get(name) {
            raus.push((name.clone(), wert.clone()));
        }
    }
    raus
}

/// Passt die gespeicherte Antwort zu dieser Anfrage? (RFC 9111, 4.1)
///
/// Jeder gespeicherte Header muss in der Anfrage gleich sein. Fehlt er,
/// passt er nicht — ein fehlender Header ist ein anderer Header.
pub fn vary_trifft(gespeichert: &[(String, String)], anfrage: &Anfrage) -> bool {
    for (name, wert) in gespeichert {
        if anfrage.headers.get(name) != Some(wert) {
            return false;
        }
    }
    true
}

/// Der Statusgrund zu einem Code — fuer die rekonstruierte Antwort.
///
/// Nur die, die ein Cache wirklich zurueckgibt. Unbekannte Codes bekommen
/// einen leeren Grund; der Code selbst ist das, was zaehlt.
pub fn grund(code: u16) -> &'static str {
    match code {
        200 => "OK",
        203 => "Non-Authoritative Information",
        204 => "No Content",
        206 => "Partial Content",
        300 => "Multiple Choices",
        301 => "Moved Permanently",
        404 => "Not Found",
        405 => "Method Not Allowed",
        410 => "Gone",
        414 => "URI Too Long",
        501 => "Not Implemented",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Antwort;

    fn antwort(code: u16, headers: &[(&str, &str)]) -> Antwort {
        let mut h = HashMap::new();
        for (k, v) in headers {
            h.insert((*k).to_string(), (*v).to_string());
        }
        Antwort {
            status: code,
            headers: h,
            koerper: Vec::new(),
        }
    }

    fn anfrage(headers: &[(&str, &str)]) -> Anfrage {
        let mut h = HashMap::new();
        for (k, v) in headers {
            h.insert((*k).to_string(), (*v).to_string());
        }
        Anfrage {
            methode: Methode::Get,
            host: "example.com".into(),
            port: 80,
            pfad: "/".into(),
            headers: h,
        }
    }

    fn headers_von(antwort: &Antwort) -> HashMap<String, String> {
        antwort.headers.clone()
    }

    #[test]
    fn cache_control_parst_alle_direktiven() {
        let a = antwort(
            200,
            &[(
                "cache-control",
                "max-age=300, no-store, private, must-revalidate",
            )],
        );
        let s = steuerung(&headers_von(&a));
        assert_eq!(s.max_age, Some(Duration::from_secs(300)));
        assert!(s.no_store);
        assert!(s.private);
        assert!(s.must_revalidate);
    }

    #[test]
    fn unbekannte_direktiven_werden_ignoriert() {
        let a = antwort(
            200,
            &[("cache-control", "max-age=60, was-ist-das=1, no-store")],
        );
        let s = steuerung(&headers_von(&a));
        assert_eq!(s.max_age, Some(Duration::from_secs(60)));
        assert!(s.no_store);
    }

    #[test]
    fn kaputtes_max_age_ist_kein_max_age() {
        let a = antwort(200, &[("cache-control", "max-age=abc")]);
        assert_eq!(steuerung(&headers_von(&a)).max_age, None);
    }

    #[test]
    fn max_age_gewinnt_ueber_expires() {
        let jetzt = SystemTime::now();
        let a = antwort(
            200,
            &[
                ("cache-control", "max-age=100"),
                (
                    "expires",
                    &schreibe_datum(jetzt + Duration::from_secs(5000)),
                ),
            ],
        );
        assert_eq!(
            frischheit(&headers_von(&a), 200, jetzt, jetzt, false),
            Some(Duration::from_secs(100))
        );
    }

    #[test]
    fn no_cache_ist_nie_frisch() {
        let a = antwort(200, &[("cache-control", "no-cache")]);
        assert_eq!(
            frischheit(
                &headers_von(&a),
                200,
                SystemTime::now(),
                SystemTime::now(),
                false
            ),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn expires_in_der_vergangenheit_ist_veraltet() {
        let jetzt = SystemTime::now();
        let a = antwort(
            200,
            &[("expires", &schreibe_datum(jetzt - Duration::from_secs(10)))],
        );
        assert_eq!(
            frischheit(&headers_von(&a), 200, jetzt, jetzt, false),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn heuristik_zehn_prozent() {
        let jetzt = SystemTime::now();
        // Last-Modified vor 100 Tagen -> Frische 10 Tage.
        let a = antwort(
            200,
            &[(
                "last-modified",
                &schreibe_datum(jetzt - Duration::from_secs(100 * 86400)),
            )],
        );
        // Toleranz: `frischheit` misst `jetzt` selbst, ein paar Mikrosekunden
        // nach dem Test-`jetzt`. Eine exakte Gleichheit waere fluechtig.
        let ergebnis = frischheit(&headers_von(&a), 200, jetzt, jetzt, false);
        let erwartet = Duration::from_secs(10 * 86400);
        let diff = match ergebnis {
            Some(e) => e.abs_diff(erwartet),
            None => panic!("erwartet: Some({erwartet:?})"),
        };
        assert!(
            diff < Duration::from_secs(5),
            "{ergebnis:?} != {erwartet:?}"
        );
    }

    #[test]
    fn s_maxage_gilt_nur_geteilt() {
        let a = antwort(200, &[("cache-control", "s-maxage=42")]);
        assert_eq!(
            frischheit(
                &headers_von(&a),
                200,
                SystemTime::now(),
                SystemTime::now(),
                false
            ),
            None
        );
        assert_eq!(
            frischheit(
                &headers_von(&a),
                200,
                SystemTime::now(),
                SystemTime::now(),
                true
            ),
            Some(Duration::from_secs(42))
        );
    }

    #[test]
    fn authorization_braucht_oeffentliche_erlaubnis() {
        let an = anfrage(&[("authorization", "Basic abc")]);
        // Ohne Erlaubnis: nicht cachebar.
        assert!(!ist_cachebar(
            &an,
            &headers_von(&antwort(200, &[])),
            200,
            false
        ));
        // Mit max-age: cachebar.
        assert!(ist_cachebar(
            &an,
            &headers_von(&antwort(200, &[("cache-control", "max-age=60")])),
            200,
            false
        ));
        // Mit public: cachebar.
        assert!(ist_cachebar(
            &an,
            &headers_von(&antwort(200, &[("cache-control", "public")])),
            200,
            false
        ));
    }

    #[test]
    fn private_in_geteiltem_cache_nicht_cachebar() {
        let an = anfrage(&[]);
        let a = antwort(200, &[("cache-control", "private, max-age=60")]);
        assert!(!ist_cachebar(&an, &headers_von(&a), 200, true));
        assert!(ist_cachebar(&an, &headers_von(&a), 200, false));
    }

    #[test]
    fn post_wird_nicht_gespeichert() {
        let mut an = anfrage(&[]);
        an.methode = Methode::Post;
        assert!(!ist_cachebar(
            &an,
            &headers_von(&antwort(200, &[("cache-control", "max-age=60")])),
            200,
            false
        ));
    }

    #[test]
    fn vary_speichert_genannte_header() {
        let an = anfrage(&[
            ("accept-encoding", "gzip"),
            ("accept-language", "de"),
            ("user-agent", "narsil"),
        ]);
        let a = antwort(200, &[("vary", "Accept-Encoding")]);
        let namen = vary_namen(&headers_von(&a));
        assert_eq!(namen, vec!["accept-encoding"]);
        let gespeichert = vary_speichern(&an, &namen);
        assert_eq!(
            gespeichert,
            vec![("accept-encoding".to_string(), "gzip".to_string())]
        );
        assert!(vary_trifft(&gespeichert, &an));
    }

    #[test]
    fn vary_trifft_nicht_bei_anderem_wert() {
        let an = anfrage(&[("accept-encoding", "gzip")]);
        let gespeichert = vec![("accept-encoding".to_string(), "deflate".to_string())];
        assert!(!vary_trifft(&gespeichert, &an));
    }

    #[test]
    fn vary_stern_speichert_alles() {
        let an = anfrage(&[("accept-encoding", "gzip"), ("user-agent", "narsil")]);
        let a = antwort(200, &[("vary", "*")]);
        let namen = vary_namen(&headers_von(&a));
        let gespeichert = vary_speichern(&an, &namen);
        assert_eq!(gespeichert.len(), 2);
        assert!(vary_trifft(&gespeichert, &an));
    }

    #[test]
    fn http_datum_rundlauf() {
        let jetzt = SystemTime::now();
        let text = schreibe_datum(jetzt);
        let zurueck = http_datum(&text).expect("Datum lesbar");
        // Aufloesung der HTTP-Daten: Sekunden.
        let diff = zurueck.duration_since(jetzt).unwrap_or_default();
        assert!(diff < Duration::from_secs(2), "{diff:?}");
    }

    #[test]
    fn http_datum_bekannte_zeit() {
        // 1970-01-01 war ein Donnerstag, 00:00:00 UTC.
        let t = http_datum("Thu, 01 Jan 1970 00:00:00 GMT").expect("lesbar");
        assert_eq!(t, SystemTime::UNIX_EPOCH);
        // 2000-01-01 war ein Samstag.
        let t2 = http_datum("Sat, 01 Jan 2000 00:00:00 GMT").expect("lesbar");
        assert_eq!(
            t2.duration_since(SystemTime::UNIX_EPOCH).unwrap(),
            Duration::from_secs(946_684_800)
        );
    }

    #[test]
    fn http_datum_kaputt_ist_kein_datum() {
        assert_eq!(http_datum("garbage"), None);
        assert_eq!(http_datum("Thu, 01 Jan 1970 00:00:00"), None); // ohne GMT
        assert_eq!(http_datum(""), None);
    }

    #[test]
    fn alter_aus_age_header() {
        let a = antwort(200, &[("age", "120")]);
        assert_eq!(
            aktuelle_alter(&headers_von(&a), SystemTime::now(), SystemTime::now()),
            Duration::from_secs(120)
        );
    }

    #[test]
    fn grund_der_statuscodes() {
        assert_eq!(grund(200), "OK");
        assert_eq!(grund(301), "Moved Permanently");
        assert_eq!(grund(404), "Not Found");
        assert_eq!(grund(599), "");
    }
}
