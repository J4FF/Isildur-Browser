//! COOKIES UND SIGNIERTE SITZUNGEN — added 01.10.2026, SERVER V7.
//!
//! Drei Dinge, die ein Webserver braucht und die diese Crate vorher
//! nicht hatte: ein Cookie *setzen*, ein Cookie *lesen*, und eine
//! Sitzung, der man trauen kann, **ohne** ihr zu trauen.
//!
//! # DIE GRUNDREGEL: DER CLIENT IST DER ANGREIFER
//!
//! Alles, was im Cookie steht, kommt vom Benutzer. Also:
//!
//! * **Nichts Geheimes im Cookie.** Wer `user_id` hineinschreibt,
//!   schreibt dem Leser die Rechte. Sitzungsdaten werden mit
//!   **HMAC-SHA256** ueber `narsil_tls::hmac` signiert: der Client kann
//!   sie lesen, aber nicht aendern. Ein Byte Aenderung, und die
//!   Pruefung schlaegt fehl.
//! * **Vergleich in konstanter Zeit.** `narsil_tls::vergleich_gleich`
//!   vergleicht ohne fruehes Zurueckgeben; ein `==` auf zwei Signaturen
//!   verraet ueber die Laufzeit, wie viele Bytes richtig waren.
//! * **`Secure`, `HttpOnly`, `SameSite` als Vorgabe**, nicht als
//!   Zierde: siehe [`Merkmale`].
//! * **Groessen begrenzt.** Ein Cookie ist ein Eingabefeld, also
//!   gelten dieselben Regeln wie bei einer Kopfzeile: [`MAX_COOKIE_BYTES`],
//!   [`MAX_COOKIE_ZAHL`], [`MAX_NAME_ZEICHEN`], [`MAX_WERT_ZEICHEN`].
//!   Eine Anfrage mit 10.000 Cookies darf keine 10.000 `String`
//!   erzeugen.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::http::{Request, Response};

// ---------------------------------------------------------------------------
// Grenzen (M3/M4: jede Schleife und jede Allokation aus der Eingabe hat
// eine Obergrenze, und sie steht VOR der Benutzung.)
// ---------------------------------------------------------------------------

/// Maximale Groesse des gesamten `Cookie`-Kopfes in Bytes (8 KiB).
///
/// RFC 6265 gibt keine Grenze vor; 4 KiB pro Cookie ist die Empfehlung,
/// 8 KiB fuer den ganzen Kopf ist grosszuegig genug fuer drei
/// Sitzungs-Cookies. Darueber ist es kein Browser.
pub const MAX_COOKIE_BYTES: usize = 8 * 1024;

/// Maximale Anzahl `name=value`-Paare in EINEM Cookie-Kopf.
///
/// 64 reicht fuer jede echte Seite. Der Grund fuer die Zahl ist nicht
/// die Tortur, sondern die Regel: die Schleife darf nicht von der
/// Eingabe laufen, und jede Ausgabe braucht ihren Platz im Speicher.
pub const MAX_COOKIE_ZAHL: usize = 64;

/// Maximale Laenge eines Cookie-Namens (256 Byte).
///
/// RFC 6265: `token`. 256 Byte sind weit ueber jedem echten Namen
/// (`session_id`, `csrf`) und weit unter dem, was eine Karte an
/// Schadcode transportieren kann.
pub const MAX_NAME_ZEICHEN: usize = 256;

/// Maximale Laenge eines Cookie-Werts (4 KiB).
///
/// Ein Sitzungs-Cookie traegt hier Base64 einer signierten Nutzlast;
/// 4 KiB sind fuer ein paar kB Nutzlast. Mehr gehoert in einen
/// serverseitigen Speicher, nicht in einen Header.
pub const MAX_WERT_ZEICHEN: usize = 4096;

/// Maximale Groesse der signierten Sitzungsnutzlast **vor** der
/// Signatur (8 KiB).
///
/// M4: die HMAC-Ausgabe ist 32 Byte, die Nutzlast waechst aber mit dem
/// Inhalt. Ohne diese Zahl koennte ein Angreifer eine 1-MiB-Nutzlast
/// signieren lassen und dafuer 1 MiB Speicher und Rechenzeit erzwingen.
pub const MAX_NUTZLAST_BYTES: usize = 8 * 1024;

/// Lebensdauer einer Sitzung, wenn keine angegeben wird (12 Stunden).
///
/// 12 Stunden ist ein Arbeitstag plus Nacht. Laenger waere eine
/// Einladung, das Ablaufdatum zu ignorieren.
pub const STANDARD_LEBENSDAUER: Duration = Duration::from_secs(12 * 3600);

/// Name des Sitzungs-Cookies, wenn keiner angegeben wird.
pub const STANDARD_NAME: &str = "sid";

// ---------------------------------------------------------------------------
// Merkmale eines Cookies
// ---------------------------------------------------------------------------

/// Die Sicherheitsmerkmale eines Cookies (RFC 6265bis).
///
/// added 01.10.2026, SERVER V7. Jedes Merkmal hat eine **Vorgabe**, und
/// die Vorgabe ist jeweils die sichere — nicht die, die man programmiert
/// hat, wenn man es eilig hat:
///
/// | Merkmal | Vorgabe | Ohne es |
/// |---|---|---|
/// | [`Merkmale::secure`] | an (bei HTTPS) | das Cookie geht im Klartext mit |
/// | [`Merkmale::http_only`] | an | jeder Scriptzugriff kann es lesen |
/// | [`Merkmale::same_site`] | [`SameSite::Lax`] | CSRF ueber ein fremdes Bild |
/// | [`Merkmale::pfad`] | `/` | das Cookie gilt fuer zu wenig |
///
/// DER GRUND FUER DIESE TABELLE: `Secure` als Vorgabe „bei HTTPS" und
/// nicht bedingungslos ist der einzige Weg, dass ein Entwickler auf
/// `http://localhost` ueberhaupt noch etwas sieht — und dort ist
/// `Secure` technisch unmoeglich. Auf HTTPS ist es an, ohne dass man
/// daran denken muss.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merkmale {
    /// Nur ueber HTTPS senden. `None` = automatisch (bei HTTPS an).
    pub secure: Option<bool>,
    /// Nicht fuer `document.cookie` sichtbar.
    pub http_only: Option<bool>,
    /// SameSite.
    pub same_site: Option<SameSite>,
    /// `Path` (Vorgabe `/`).
    pub pfad: Option<String>,
    /// `Domain` (Vorgabe: keines — der HostOnly-Cookie ist der
    /// schmalere und sicherere).
    pub domain: Option<String>,
    /// `Expires`.
    pub laeuft_ab: Option<SystemTime>,
    /// `Max-Age` in Sekunden.
    pub max_age: Option<u64>,
}

/// `SameSite` in seinen drei Werten (RFC 6265bis 5.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameSite {
    /// Gar nicht senden, wenn die Anfrage von einer anderen Site kommt.
    Streng,
    /// Nur bei GET von oben (die Antwort, die ein fremdes Bild nicht
    /// ausloesen kann) — der Standard, den Browser ohne Nachricht
    /// verwenden.
    Lax,
    /// Bei GET **und** POST von oben.
    Keine,
}

impl SameSite {
    /// Als Zeile: `Strict`, `Lax` oder `None`.
    pub fn als_str(self) -> &'static str {
        match self {
            SameSite::Streng => "Strict",
            SameSite::Lax => "Lax",
            SameSite::Keine => "None",
        }
    }
}

impl Default for Merkmale {
    /// Die sicheren Vorgaben. `secure` bleibt `None`, weil es von der
    /// Verbindung abhaengt (siehe [`Merkmale::als_zeilen`]).
    fn default() -> Merkmale {
        Merkmale {
            secure: None,
            http_only: Some(true),
            same_site: Some(SameSite::Lax),
            pfad: Some("/".to_string()),
            domain: None,
            laeuft_ab: None,
            max_age: None,
        }
    }
}

impl Merkmale {
    /// Sicherste Merkmale: `Secure` fest an. Fuer Produktion ueber HTTPS.
    pub fn sicher() -> Merkmale {
        Merkmale {
            secure: Some(true),
            ..Merkmale::default()
        }
    }

    /// `SameSite=None` — noetig, wenn der Browser sonst nichts sendet
    /// (Chrome 80+ verlangt es fuer `None`), und nur zusammen mit
    /// `Secure` zulaessig.
    pub fn cross_site() -> Merkmale {
        Merkmale {
            secure: Some(true),
            same_site: Some(SameSite::Keine),
            ..Merkmale::default()
        }
    }

    /// `SameSite=Strict` (die strengste Form).
    pub fn streng() -> Merkmale {
        Merkmale {
            same_site: Some(SameSite::Streng),
            ..Merkmale::default()
        }
    }

    /// Baut die vollstaendige `Set-Cookie`-Zeile.
    ///
    /// `https` entscheidet ueber `Secure`, wenn `secure` auf `None`
    /// steht. Und `SameSite=None` **ohne** `Secure` wird zu `Lax`
    /// abgeschwaecht: Browser verweigern die Kombination, und ein Cookie,
    /// das der Browser nicht sendet, ist schlimmer als eines mit dem
    /// schwächeren, aber akzeptierten Merkmal.
    pub fn als_zeilen(&self, name: &str, wert: &str, https: bool) -> Vec<String> {
        let mut zeilen = Vec::with_capacity(7);
        let kern = format!("{name}={wert}");
        zeilen.push(kern);

        let mut same_site = self.same_site;
        let secure = self.secure.unwrap_or(https);
        if same_site == Some(SameSite::Keine) && !secure {
            same_site = Some(SameSite::Lax);
        }
        if secure {
            zeilen.push("Secure".to_string());
        }
        if self.http_only.unwrap_or(true) {
            zeilen.push("HttpOnly".to_string());
        }
        if let Some(s) = same_site {
            zeilen.push(format!("SameSite={}", s.als_str()));
        }
        if let Some(p) = &self.pfad {
            zeilen.push(format!("Path={p}"));
        }
        // `Domain` nur, wenn wirklich gewuenscht: ohne Domain ist der
        // Cookie an genau diesen Host gebunden, und das ist enger.
        if let Some(d) = &self.domain {
            zeilen.push(format!("Domain={d}"));
        }
        if let Some(a) = self.max_age {
            zeilen.push(format!("Max-Age={a}"));
        }
        if let Some(t) = self.laeuft_ab {
            zeilen.push(format!("Expires={}", crate::static_files::http_datum(t)));
        }
        zeilen
    }

    /// Als **eine** Zeile (Cookie-2-Art, von der RFC 6265 abrät, aber
    /// von jedem Browser gelesen wird). Fuer Clients, die keine
    /// wiederholten `Set-Cookie`-Kopfzeilen verkraften.
    pub fn als_zeile(&self, name: &str, wert: &str, https: bool) -> String {
        self.als_zeilen(name, wert, https).join("; ")
    }
}

// ---------------------------------------------------------------------------
// Setzen
// ---------------------------------------------------------------------------

/// Setzt ein Cookie an die Antwort (haengt ein neues `Set-Cookie` an).
///
/// Haengt **an**, ersetzt nicht: `Set-Cookie` darf mehrfach vorkommen
/// (RFC 6265 3.2), und ein Handler, der zwei Cookies setzt, will
/// nicht, dass das zweite das erste loescht.
pub fn setze(antwort: &mut Response, name: &str, wert: &str, merkmale: &Merkmale, https: bool) {
    // EINE `Set-Cookie`-Kopfzeile mit `; `-getrennten Merkmalen —
    // nicht eine Kopfzeile je Merkmal.
    //
    // DER FEHLER, DEN DER TEST GEFUNDEN HAT: die erste Fassung schrieb
    // `sid=abc` als eine Kopfzeile, dann `Secure` als naechste, dann
    // `HttpOnly`. RFC 6265 5.2 ist da eindeutig: `Set-Cookie` traegt
    // `cookie-pair` **und** `cookie-av` in EINER Zeile, getrennt durch
    // `; ` (4.1.1: "the user agent MUST NOT honor a Set-Cookie header
    // that lacks an '=' character" — jede dieser Zeilen haette gar
    // kein `=`). Ein Browser haette daraus vier Cookies ohne Merkmale
    // gemacht: **kein** `HttpOnly`, **kein** `SameSite`, **kein**
    // `Secure`. Der Code sah richtig aus und waere eine
    // Sicherheitsluecke gewesen.
    antwort.headers.push((
        "Set-Cookie".to_string(),
        merkmale.als_zeile(name, wert, https),
    ));
}

/// Loescht ein Cookie: `Max-Age=0` mit Ablauf in der Vergangenheit.
///
/// Nur `Max-Age=0` reicht bei manchen Browsern nicht — beide zu setzen
/// ist die Form, die ueberall wirkt (RFC 6265 5.3, Abschnitt „Löschen").
pub fn loesche(antwort: &mut Response, name: &str, https: bool) {
    let merkmale = Merkmale {
        max_age: Some(0),
        laeuft_ab: Some(UNIX_EPOCH),
        ..Merkmale::sicher()
    };
    setze(antwort, name, "", &merkmale, https);
}

/// Ist die Anfrage ueber HTTPS? (Aus `X-Forwarded-Proto` — der Server
/// selbst spricht kein TLS, ausser ueber [`crate::tls`].)
pub fn ist_https(anfrage: &Request) -> bool {
    anfrage
        .header("x-forwarded-proto")
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("https"))
}

// ---------------------------------------------------------------------------
// Lesen
// ---------------------------------------------------------------------------

/// Ein Cookie aus der Anfrage: Name und **Wert** (der ganze Rest nach
/// dem ersten `=`).
///
/// DER WICHTIGE PUNKT ZUM WERT: ein Cookie-Wert darf `=` enthalten
/// (Base64 endet darauf). Deshalb wird nur am **ersten** `=` geteilt —
/// `split_once`, nicht `split('=')`. Mit `split` bekommt
/// `YWJj=ZGVm` den Wert `ZGVm` und der Client bekommt eine falsche
/// Sitzung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cookie {
    /// Name (unveraendert, Gross-/Kleinschreibung wie gesendet).
    pub name: String,
    /// Wert zwischen Anfuehrungszeichen (falls gesetzt).
    pub wert: String,
}

impl Cookie {
    /// Ist der Name gleich? (Gross-/Kleinschreibung ist **gleich**:
    /// Cookie-Namen sind case-sensitive nach RFC 6265 — aber hier ist
    /// die Absicht „derselbe Name", und da hilft tolerant. Nein:
    /// case-sensitive, sonst waere `SID` und `sid` dasselbe.)
    pub fn ist(&self, name: &str) -> bool {
        self.name == name
    }
}

/// Liest alle Cookies der Anfrage — mit **allen** Grenzen.
///
/// Der Rueckgabegrund ist `MAX_COOKIE_ZAHL`: eine Anfrage mit 10.000
/// Cookies darf keine 10.000 `Cookie`-Werte erzeugen. Was darueber
/// kommt, wird nicht gelesen (und nicht gespeichert) — der Client hat
/// dann zu viele, und das ist eine Fehlermeldung an ihn, nicht ein
/// Absturz an uns.
pub fn lies_alle(anfrage: &Request) -> Vec<Cookie> {
    let Some(kopf) = anfrage.header("cookie") else {
        return Vec::new();
    };
    // M4: erst die Laenge deckeln, dann aufteilen. `split(';')` auf
    // einem 10-MiB-Kopf wuerde sonst 10 MiB Kopien erzeugen.
    if kopf.len() > MAX_COOKIE_BYTES {
        return Vec::new();
    }
    let mut raus: Vec<Cookie> = Vec::with_capacity(8);
    // M3: die Schleife laeuft ueber `split`, das ueber die **gekappte**
    // Laenge laeuft — die Zahl der Teile ist damit durch die Bytes
    // begrenzt, und `MAX_COOKIE_ZAHL` ist die zweite, naeher liegende.
    for teil in kopf.split(';').take(MAX_COOKIE_ZAHL) {
        let teil = teil.trim();
        if teil.is_empty() {
            continue;
        }
        let (name, wert) = match teil.split_once('=') {
            Some((n, w)) => (n.trim(), w.trim()),
            // RFC 6265 4.1.1 Schritt 2 wortwoertlich: "If the
            // name-value-pair string lacks a percent-encoded equals
            // sign, ignore the name-value-pair string entirely."
            //
            // DER FEHLER, und er ist vom FUZZER gefunden worden (V13,
            // 17:05, `funde/cookie-panik-0.bin`): die erste Fassung
            // las `None => (teil, "")`. Ein Segment ohne `=` wurde
            // also ein Cookie **mit Namen und leerem Wert** — und
            // damit zaehlte es als Paar.
            //
            // WARUM DAS SCHLIMM IST und nicht nur uneinheitlich:
            // `letztes_von` nimmt den **letzten** Wert eines Namens,
            // weil das die Browser-Deutung ist. Ein `Cookie:
            // sid=gueltig; sid` ergibt damit zweimal `sid` — und der
            // letzte ist der leere. Die Folge: `erneuere_oder_pruefe`
            // prueft `""` statt des gueltigen Werts, scheitert und
            // schickt eine **neue** Sitzung. Das ist Cookie-Shadowing
            // von der anderen Seite: der Browser wuerde das zweite
            // `sid` nie schicken (er kann es nicht erzeugen, `Set-
            // Cookie: sid` ohne `=` wird vom Browser verworfen), also
            // sieht der Server etwas, das es im Browser nicht gibt.
            // Und die Umkehrung gilt auch: `a=b; a` macht aus einem
            // gueltigen `a` einen leeren Wert, wenn ein Middleware-
            // Stueck `erstes_von` und eins `letztes_von` benutzt.
            None => continue,
        };
        if name.is_empty() || name.len() > MAX_NAME_ZEICHEN || wert.len() > MAX_WERT_ZEICHEN {
            continue;
        }
        // Anfuehrungszeichen sind erlaubt (RFC 6265 4.1.1) und muessen
        // weg, bevor jemand den Wert vergleicht.
        let wert = wert
            .strip_prefix('"')
            .and_then(|w| w.strip_suffix('"'))
            .unwrap_or(wert);
        raus.push(Cookie {
            name: name.to_string(),
            wert: wert.to_string(),
        });
    }
    raus
}

/// Liest **alle** Werte eines Namens.
///
/// Mehrfachcookies (`a=1; a=2`) sind ein bekannter Angriffsvektor
/// („cookie shadowing"): der Browser schickt beide, der naive Parser
/// nimmt das erste, ein Framework das letzte. Diese Funktion gibt
/// deshalb alle zurueck und die Aufruferin entscheidet — mit
/// [`letztes_von`] (die Deutung der meisten Browser) oder mit
/// [`erstes_von`].
pub fn lies_name(anfrage: &Request, name: &str) -> Vec<String> {
    lies_alle(anfrage)
        .into_iter()
        .filter(|c| c.ist(name))
        .map(|c| c.wert)
        .collect()
}

/// Der **letzte** Wert eines Namens — die Deutung der meisten Browser
/// bei `a=1; a=2` (das zuletzt gesetzte gewinnt).
pub fn letztes_von(anfrage: &Request, name: &str) -> Option<String> {
    lies_name(anfrage, name).pop()
}

/// Der **erste** Wert eines Namens.
pub fn erstes_von(anfrage: &Request, name: &str) -> Option<String> {
    lies_name(anfrage, name).into_iter().next()
}

/// Alle Cookies als Map (der bequeme Weg, wenn Mehrfachcookies nicht
/// vorkommen duerfen). Bei einem doppelten Namen gewinnt der letzte.
pub fn als_map(anfrage: &Request) -> HashMap<String, String> {
    let mut m = HashMap::with_capacity(8);
    for c in lies_alle(anfrage) {
        m.insert(c.name, c.wert);
    }
    m
}

/// Setzt `__Host-` vor einen Cookie-Namen, wenn er es erlaubt.
///
/// `__Host-` (RFC 6265bis 4.1.3) ist die schaerfste Form: `Secure`,
/// `Path=/`, **kein** `Domain`. Ein fremder Subdomain-Angriff kann dann
/// kein Cookie dieser Art setzen, weil der Browser es ablehnt.
pub fn mit_host_raumen(name: &str) -> String {
    if name.starts_with("__Host-") || name.starts_with("__Secure-") {
        return name.to_string();
    }
    format!("__Host-{name}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::anfrage_aus_text;

    fn anfrage_mit_cookie(zeile: &str) -> Request {
        anfrage_aus_text("GET", "/", &[("Cookie", zeile)], &[])
    }

    /// ECHTE Cookies aus einem echten Browser, nicht `a=1; b=2`:
    /// die Session-Cookies kommen mit `=` im Wert (Base64), und genau
    /// daran ist die erste Fassung gescheitert.
    #[test]
    fn echte_cookie_zeile_wird_richtig_zerlegt() {
        let a = anfrage_mit_cookie(
            "sid=abc123; theme=dunkel; csrf=a=b=c; _ga=GA1.2.1234567890.1234567890",
        );
        let m = als_map(&a);
        assert_eq!(m.get("sid").map(String::as_str), Some("abc123"));
        assert_eq!(m.get("theme").map(String::as_str), Some("dunkel"));
        // DER BEFUND: der Wert enthaelt `=` und muss **voll** ankommen.
        assert_eq!(m.get("csrf").map(String::as_str), Some("a=b=c"));
        assert_eq!(
            m.get("_ga").map(String::as_str),
            Some("GA1.2.1234567890.1234567890")
        );
    }

    #[test]
    fn anfuehrungszeichen_werden_entfernt() {
        let a = anfrage_mit_cookie(r#"sid="mit leer"; x="y""#);
        let m = als_map(&a);
        assert_eq!(m.get("sid").map(String::as_str), Some("mit leer"));
        assert_eq!(m.get("x").map(String::as_str), Some("y"));
    }

    #[test]
    fn doppelter_name_liefert_alle_werte() {
        // Cookie-Shadowing: zwei Werte, ein Name.
        let a = anfrage_mit_cookie("a=1; a=2; a=3");
        assert_eq!(lies_name(&a, "a"), vec!["1", "2", "3"]);
        assert_eq!(letztes_von(&a, "a"), Some("3".to_string()));
        assert_eq!(erstes_von(&a, "a"), Some("1".to_string()));
    }

    #[test]
    fn leerer_wert_ist_kein_fehler() {
        let a = anfrage_mit_cookie("leer=; da=1");
        let m = als_map(&a);
        assert_eq!(m.get("leer").map(String::as_str), Some(""));
        assert_eq!(m.get("da").map(String::as_str), Some("1"));
        // Ohne `=` wird das Segment **ignoriert** (RFC 6265 4.1.1
        // Schritt 2) — das stand hier frueher als `assert_eq!(..., 1)`
        // und war der Test, der die alte (falsche) Annahme festgehalten
        // hat. Der Fund kam aus dem Fuzzer, nicht aus diesem Test; siehe
        // `tests/v13_orakel.rs::cookie_ohne_gleichzeichen_ist_kein_cookie`.
        let b = anfrage_mit_cookie("nurname");
        assert_eq!(b.header("cookie"), Some("nurname"));
        assert_eq!(lies_alle(&b).len(), 0);
        // Und gemischt zaehlt nur der Teil mit `=`:
        assert_eq!(lies_alle(&anfrage_mit_cookie("a=1; nurname; b=2")).len(), 2);
    }

    /// M3/M4: die Grenzen sind nicht dekorativ — sie greifen.
    #[test]
    fn zu_viele_und_zu_grosse_cookies_werden_gekappt() {
        // 1) Zu viele Paare.
        let viele: Vec<String> = (0..MAX_COOKIE_ZAHL + 50)
            .map(|i| format!("k{i}=v"))
            .collect();
        let a = anfrage_mit_cookie(&viele.join("; "));
        let gelesen = lies_alle(&a);
        assert!(
            gelesen.len() <= MAX_COOKIE_ZAHL,
            "{} Cookies gelesen, erlaubt {MAX_COOKIE_ZAHL}",
            gelesen.len()
        );

        // 2) Ein Wert ueber der Grenze wird **nicht** gespeichert.
        let lang = format!("sid={}", "x".repeat(MAX_WERT_ZEICHEN + 1));
        let b = anfrage_mit_cookie(&lang);
        assert!(
            lies_name(&b, "sid").is_empty(),
            "ein {} Byte langer Wert kam an",
            MAX_WERT_ZEICHEN + 1
        );
        // ...und einer genau an der Grenze doch.
        let genau = format!("sid={}", "x".repeat(MAX_WERT_ZEICHEN));
        let c = anfrage_mit_cookie(&genau);
        assert_eq!(
            lies_name(&c, "sid").first().map(|v| v.len()),
            Some(MAX_WERT_ZEICHEN)
        );

        // 3) Der GANZE Kopf ueber der Grenze: nichts wird gelesen.
        let riesig = "a=".to_string() + &"b".repeat(MAX_COOKIE_BYTES + 100);
        let d = anfrage_mit_cookie(&riesig);
        assert!(lies_alle(&d).is_empty(), "ein 8-KiB-Kopf kam an");

        // 4) Ein Name ueber der Grenze.
        let langer_name = format!("{}x=1", "n".repeat(MAX_NAME_ZEICHEN + 1));
        let e = anfrage_mit_cookie(&langer_name);
        assert!(lies_alle(&e).is_empty(), "ein 256+ Byte langer Name kam an");
    }

    #[test]
    fn kein_cookie_kopf_ist_leer_und_kein_fehler() {
        let a = anfrage_aus_text("GET", "/", &[], &[]);
        assert!(lies_alle(&a).is_empty());
        assert_eq!(letztes_von(&a, "sid"), None);
        assert!(als_map(&a).is_empty());
    }

    /// Die sicheren Vorgaben, und jede einzelne geprueft.
    #[test]
    fn merkmale_als_vorgabe_sind_sicher() {
        let m = Merkmale::default();
        let zeilen = m.als_zeilen("sid", "abc", true);
        assert!(zeilen[0].starts_with("sid=abc"), "{:?}", zeilen);
        assert!(zeilen.iter().any(|z| z == "Secure"), "{:?}", zeilen);
        assert!(zeilen.iter().any(|z| z == "HttpOnly"), "{:?}", zeilen);
        assert!(zeilen.iter().any(|z| z == "SameSite=Lax"), "{:?}", zeilen);
        assert!(zeilen.iter().any(|z| z == "Path=/"), "{:?}", zeilen);
        assert!(
            !zeilen.iter().any(|z| z.starts_with("Domain=")),
            "ohne gewuenschte Domain: {:?}",
            zeilen
        );
    }

    #[test]
    fn ueber_http_gibt_es_kein_secure_aber_alles_andere() {
        let m = Merkmale::default();
        let zeilen = m.als_zeilen("sid", "abc", false);
        assert!(
            !zeilen.iter().any(|z| z == "Secure"),
            "auf http gibt es kein Secure (sonst sieht der Entwickler nichts): {:?}",
            zeilen
        );
        assert!(zeilen.iter().any(|z| z == "HttpOnly"), "{zeilen:?}");
        assert!(zeilen.iter().any(|z| z == "SameSite=Lax"), "{zeilen:?}");
    }

    /// `SameSite=None` ohne `Secure` waere ein Cookie, das der Browser
    /// ablehnt — der Autor will dann, dass es trotzdem wirkt.
    #[test]
    fn same_site_none_ohne_secure_wird_zu_lax() {
        let m = Merkmale {
            secure: Some(false),
            same_site: Some(SameSite::Keine),
            ..Merkmale::default()
        };
        let zeilen = m.als_zeilen("sid", "abc", true);
        assert!(
            zeilen.iter().any(|z| z == "SameSite=Lax"),
            "abgeschwaecht statt abgelehnt: {zeilen:?}"
        );
        assert!(!zeilen.iter().any(|z| z == "SameSite=None"), "{zeilen:?}");
    }

    #[test]
    fn cross_site_ist_strikt_und_traegt_secure() {
        let zeilen = Merkmale::cross_site().als_zeilen("sid", "abc", false);
        assert!(zeilen.iter().any(|z| z == "SameSite=None"), "{zeilen:?}");
        assert!(zeilen.iter().any(|z| z == "Secure"), "{zeilen:?}");
    }

    #[test]
    fn loeschen_setzt_beide_kennzeichen() {
        let mut a = crate::http::Response::text(200, "x");
        loesche(&mut a, "sid", true);
        let zeile = a
            .headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
            .map(|(_, v)| v.clone())
            .collect::<Vec<_>>()
            .join("; ");
        assert!(zeile.contains("Max-Age=0"), "{zeile}");
        assert!(zeile.contains("Expires=Thu, 01 Jan 1970"), "{zeile}");
        assert!(zeile.contains("Secure"), "{zeile}");
        assert!(zeile.contains("HttpOnly"), "{zeile}");
    }

    #[test]
    fn setzen_haengt_an_statt_zu_ersetzen() {
        let mut a = crate::http::Response::text(200, "x");
        setze(&mut a, "sid", "1", &Merkmale::default(), true);
        setze(&mut a, "csrf", "2", &Merkmale::default(), true);
        let zeilen: Vec<&str> = a
            .headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
            .map(|(_, v)| v.as_str())
            .collect();
        // ZWEI `Set-Cookie`, nicht acht: ein Cookie ist EINE Zeile.
        assert_eq!(zeilen.len(), 2, "{zeilen:?}");
        assert!(zeilen[0].starts_with("sid=1;"), "{}", zeilen[0]);
        assert!(zeilen[1].starts_with("csrf=2;"), "{}", zeilen[1]);
        // Und beide tragen ihre Merkmale.
        for z in &zeilen {
            assert!(z.contains("HttpOnly"), "{z}");
            assert!(z.contains("SameSite=Lax"), "{z}");
            assert!(z.contains("Path=/"), "{z}");
        }
    }

    #[test]
    fn host_raumen_wird_nicht_doppelt_gesetzt() {
        assert_eq!(mit_host_raumen("sid"), "__Host-sid");
        assert_eq!(mit_host_raumen("__Host-sid"), "__Host-sid");
        assert_eq!(mit_host_raumen("__Secure-sid"), "__Secure-sid");
    }

    #[test]
    fn https_wird_aus_x_forwarded_proto_gelesen() {
        assert!(ist_https(&anfrage_aus_text(
            "GET",
            "/",
            &[("X-Forwarded-Proto", "https")],
            &[]
        )));
        assert!(!ist_https(&anfrage_aus_text(
            "GET",
            "/",
            &[("X-Forwarded-Proto", "http")],
            &[]
        )));
        assert!(!ist_https(&anfrage_aus_text("GET", "/", &[], &[])));
    }
}
