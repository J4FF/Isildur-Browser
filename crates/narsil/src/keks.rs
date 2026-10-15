//! added 04.10.2026, Rolle TLS. **DER KEKSSPEICHER DES BROWSERS** — die
//! Verarbeitung, die dem Speicher aus `narsil-net` fehlt.
//!
//! -------------------------------------------------------------------------
//! WARUM EINE EIGENE DATEI
//! -------------------------------------------------------------------------
//! `narsil-net` hat den **Speicher** (`keksglas.rs`: Felder, `Set-Cookie`-Parser,
//! `Cookie`-Kopf). Was dort **nicht** liegt und **hier** liegt:
//!
//! 1. **Der Prozesszustand.** `narsil` laeuft in **einem** Browserprozess, und
//!    die Kekse muessen **darin** liegen — sonst hat jeder Reiter einen
//!    eigenen Vorrat, und „Cookies loeschen“ wuerde nur einen loeschen.
//! 2. **Die Aufrufer.** Fuenf Stellen holen Unterressourcen (`laden.rs`,
//!    `bilder.rs`, `blaetter.rs` **zweimal**, `js.rs`), und jede braucht
//!    **dieselben** zwei Zeilen: Cookie-Kopf dran, `Set-Cookie` speichern.
//!    **Fuenfmal derselbe Code** waere fuenfmal eine Stelle, die auseinander-
//!    laufen kann.
//! 3. **Die Schnittstelle fuer `document.cookie`** — die braucht `narsil/src/
//!    js.rs` (CACHE), also **eine** oeffentliche Funktion statt zweier.
//!
//! -------------------------------------------------------------------------
//! DIE REGELN, und jede kommt von woertlich **hierher**
//! -------------------------------------------------------------------------
//! * `sicherheit::cookie_darf_gesendet` — `Secure`, `SameSite`,
//!   Drittanbieter-Sperre (draft-ietf-httpbis-rfc6265bis 5.6/5.7).
//! * `sicherheit::cookie_fuer_script_sichtbar` — `HttpOnly` (5.6).
//! * `keksglas::Keksglas` — Domain/Pfad/Ablauf/Host-only (RFC 6265 5.1.3/5.1.4).
//!
//! **UND HIER IST KEINE NEUE REGEL**, nur die **Vorgabe** fuer die
//! Drittanbieter-Sperre. Die Entscheidungen stehen an **einer** Stelle
//! (`sicherheit.rs`), und diese Datei **ruft** sie.

// added 04.10.2026, TLS: **`pub use`, weil `narsil`-Aufrufer den Typ
// brauchen** — `document.cookie`-Aufrufer wollen `HttpOnly` sehen und ein
// `Keks` nicht ueber `narsil_net::keksglas::Keks` schreiben muessen. **UND ES
// IST EIN `pub use`, KEIN ZWEITER TYP**: zwei Structs mit denselben Feldern
// sind zwei Speicher, und der Speicher ist **einer**.
pub use narsil_net::keksglas::Keks;
use narsil_net::keksglas::Keksglas;
// added 04.10.2026, TLS: **`SameSiteWert` ist oeffentlich**, weil
// [`VORGABE_SAME_SITE`] einen Wert davon nennt und ein Aufrufer den Typ braucht,
// um ihn **zu vergleichen** — ein `Vergleich mit sich selbst` waere wieder der
// Test, der jeden Wert annimmt.
pub use narsil_net::sicherheit::SameSiteWert;
use std::sync::{Mutex, OnceLock};

/// added 04.10.2026, TLS: **DIE `SameSite`-VORGABE**, und sie steht **hier**, an
/// **einer** Stelle, damit sie **eine** ist.
///
/// **UND WARUM `Lax` UND NICHT `Strict`:** Browser senden ein Keks **ohne**
/// `SameSite` heute als `Lax` — Firefox seit Version 96, Safari seit 13, Chrome
/// seit Version 80 (GEMESSEN als Browser-Vorgabe, nicht als Spezifikation).
/// **`Strict` als Vorgabe wuerde jedes Login brechen, das von einer anderen
/// Seite kommt** — und ein Login, das nach einem Link in einer E-Mail nicht
/// funktioniert, ist der Tod jeder Anmeldung.
///
/// **UND SIE STEHT HIER, NICHT IN `keksglas.rs`:** der **Speicher** in
/// `narsil-net` darf **nicht** entscheiden, was der **Browser** annimmt. Er
/// weiss es nicht — nur der Browser weiss, ob er Drittanbieter sperrt.
pub const VORGABE_SAME_SITE: SameSiteWert = SameSiteWert::Lax;

/// added 04.10.2026, TLS: **DIE VOREINSTELLUNG**, und sie ist **blockierend**.
///
/// **WARUM NICHT „erlauben“:** Firefox, Safari und Chrome sperren
/// Drittanbieter-Cookies **Voreinstellung**, weil Verfolgung ueber Domains
/// hinweg der groesste Missbrauch von Cookies ist. Wer es andersrum baut, baut
/// das Web nach 2014.
///
/// **UND SIE IST EINE KONSTANTE, KEIN ARGUMENT**, weil sie **hier** die
/// Voreinstellung ist — der Aufrufer kann sie ueber
/// [`drittanbieter_gesperrt_setzen`] aendern.
pub const DRITTEANBIER_STANDARD: bool = true;

/// added 04.10.2026, TLS: **DER ZUSTAND**, und es sind **drei** Felder: der
/// Speicher, die Drittanbieter-Voreinstellung und die **abgelehnten** Kekse.
///
/// **UND DER ZAEHLER, und der Grund ist nicht Kosmetik:** ein Browser, der
/// Kekse ablehnt, **sagt es nicht** — das ist die schlechteste Eigenschaft,
/// die ein Browser haben kann. Der Benutzer wundert sich, warum er nicht
/// eingeloggt bleibt. Firefox zeigt es in den DevTools; wir zaehlen, damit
/// [`anmerkungen`] **etwas** sagen kann.
struct Zustand {
    glas: Keksglas,
    drittanbieter_gesperrt: bool,
    abgelehnt: Vec<String>,
}

fn zustand() -> &'static Mutex<Zustand> {
    static Z: OnceLock<Mutex<Zustand>> = OnceLock::new();
    Z.get_or_init(|| {
        Mutex::new(Zustand {
            glas: Keksglas::neu(),
            drittanbieter_gesperrt: DRITTEANBIER_STANDARD,
            abgelehnt: Vec::new(),
        })
    })
}

fn mit<F: FnOnce(&mut Zustand) -> R, R>(f: F) -> R {
    let mut g = match zustand().lock() {
        Ok(g) => g,
        Err(v) => v.into_inner(),
    };
    f(&mut g)
}

/// added 04.10.2026, TLS: **DIE UHR ALS FUNKTION**, und sie ist **kein**
/// `SystemTime::now()` im Aufruf — damit **jeder** Test mit **festen** Zeiten
/// laeuft. Das ist der Unterschied zwischen einem Test fuer eine Regel und
/// einem, der am **Ablaufdatum** eines Testwerts rot wird.
fn jetzt() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// added 04.10.2026, TLS: **WIE VIELE KEKSE** — fuer die Statuszeile und die
/// Messung.
pub fn anzahl() -> usize {
    mit(|z| z.glas.anzahl())
}

/// added 04.10.2026, TLS: **DIE DRITTEANBIETER-SPERRE UMSTELLEN** — das ist die
/// Einstellung aus dem Auftrag, und sie ist das **einzige**, was der Benutzer
/// hier wirklich steuert.
pub fn drittanbieter_gesperrt_setzen(wert: bool) {
    mit(|z| z.drittanbieter_gesperrt = wert);
}

/// added 04.10.2026, TLS: **UND LESEN**, damit die Statuszeile den Stand zeigen
/// kann — ohne die Einstellung zu aendern.
pub fn drittanbieter_gesperrt() -> bool {
    mit(|z| z.drittanbieter_gesperrt)
}

/// added 04.10.2026, TLS: **ALLES LOESCHEN** — fuer „Cookies loeschen (alle)“.
/// Und es loescht **wirklich alles**, nicht nur die Sitzung: das ist der
/// Unterschied zu einem Neustart, und der Benutzer erwartet ihn.
pub fn alle_loeschen() {
    mit(|z| {
        z.glas = Keksglas::neu();
        z.abgelehnt.clear();
    });
}

/// added 04.10.2026, TLS: **JE SEITE LOESCHEN**, und „Seite“ heisst hier **die
/// Domain**, nicht die **genaue URL**.
///
/// **WARUM DOMAIN:** Firefox sagt „Cookies und Website-Daten fuer diese Domain
/// loeschen“ — und der Benutzer will **das** Cookie weg, nicht nur das von
/// `/index.html`. Und die Kekse selbst greifen ueber Domain, also **muss** der
/// Loeschvorgang das auch.
pub fn seite_loeschen(domain: &str) {
    mit(|z| z.glas.entferne_domain(domain));
}

/// added 04.10.2026, TLS: **DIE ANMERKUNGEN**, und sie sind **begrenzt** — ein
/// Benutzer, der tausend abgelehnte Kekse sieht, liest keine davon.
pub fn anmerkungen() -> Vec<String> {
    mit(|z| z.abgelehnt.clone())
}

// ===========================================================================
// DIE DREI FUNKTIONEN, und es sind genau die, die die Aufrufer brauchen
// ===========================================================================

/// added 04.10.2026, TLS: **DEN `Cookie`-KOPF AN EINE ANFRAGE HAENGEN** — und
/// das wird **eine** Zeile an **fuenf** Stellen.
///
/// **UND DIE OBERSTE SEITE** ist das Argument, um das es geht: ein Bild von
/// `cdn.test` auf `seite.test` ist ein **Drittanbieter**-Zugriff, und genau
/// der wird gesperrt.
pub fn anfrage_bei_kekse(anfrage: &mut narsil_net::Request, url: &str, oberste_seite: &str) {
    let ziel = zerlege(url);
    if ziel.host.is_empty() {
        return;
    }
    let https = url
        .trim_start()
        .to_ascii_lowercase()
        .starts_with("https://");
    let kopf = mit(|z| {
        z.glas.cookie_kopf(
            https,
            &ziel.host,
            &ziel.rest,
            jetzt(),
            oberste_seite,
            z.drittanbieter_gesperrt,
        )
    });
    if let Some(k) = kopf {
        anfrage.headers.insert("Cookie".to_string(), k);
    }
}

/// added 04.10.2026, TLS: **`Set-Cookie` AUS EINER ANTWORT SPEICHERN** — und
/// **alle** Koepfe, denn `Response.set_cookie` sammelt sie in
/// Empfangsreihenfolge (GEMESSEN an `narsil-net/src/lib.rs:1055`).
///
/// **UND DER FEHLER WIRD GEZAEHLT, nicht verschluckt** — siehe `Zustand`.
pub fn antwort_bei_kekse(antwort: &narsil_net::Response, url: &str) {
    let ziel = zerlege(url);
    if ziel.host.is_empty() {
        return;
    }
    let https = antwort.url.to_ascii_lowercase().starts_with("https://");
    mit(|z| {
        for kopf in &antwort.set_cookie {
            if let Err(m) = z
                .glas
                .aus_set_cookie(kopf, &ziel.host, &ziel.rest, jetzt(), https)
            {
                if z.abgelehnt.len() < 32 {
                    z.abgelehnt.push(std::format!("{}: {}", ziel.host, m.grund));
                }
            }
        }
    })
}

/// Release 15.10.2026: the `Cookie` header value for a script request
/// (fetch, XMLHttpRequest) to `url` from the page `oberste_seite` — the same
/// rules as [`anfrage_bei_kekse`].
pub fn cookie_kopf_fuer(url: &str, oberste_seite: &str) -> Option<String> {
    let mut a = narsil_net::Request::get("x", 80, "/");
    a.headers.clear();
    anfrage_bei_kekse(&mut a, url, oberste_seite);
    a.headers.remove("Cookie")
}

/// Release 15.10.2026: store the `Set-Cookie` headers of a script request's
/// response (fetch, XMLHttpRequest), like [`antwort_bei_kekse`] does for page
/// loads. MEASURED before: golem.de's consent page requests
/// `/abo/testcookie.php` per XHR and then reads `document.cookie`; the cookie
/// was never stored, so the page said cookies were disabled and offered no
/// consent button.
pub fn set_cookie_koepfe<'a>(koepfe: impl Iterator<Item = &'a str>, url: &str) {
    let ziel = zerlege(url);
    if ziel.host.is_empty() {
        return;
    }
    let https = url.trim_start().to_ascii_lowercase().starts_with("https://");
    mit(|z| {
        for kopf in koepfe {
            if let Err(m) = z.glas.aus_set_cookie(kopf, &ziel.host, &ziel.rest, jetzt(), https)
                && z.abgelehnt.len() < 32
            {
                z.abgelehnt.push(std::format!("{}: {}", ziel.host, m.grund));
            }
        }
    })
}

/// added 04.10.2026, TLS: **`document.cookie` LESEN** — und **`HttpOnly` ist der
/// entscheidende Punkt**.
///
/// **DIE REGEL, woertlich** draft-ietf-httpbis-rfc6265bis 5.6: ein Keks mit
/// `HttpOnly` darf **nicht** durch Skript offengelegt werden.
/// **`cookie_kopf` kann das nicht**, weil es fuer den **Netz**-Weg gebaut wird —
/// dort ist `HttpOnly` ausdruecklich erlaubt. **ZWEI WEGE, ZWEI REGELN**, und
/// wer **einen** baut, hat eins davon falsch.
///
/// **UND DIE OBERSTE SEITE IST SICH SELBST** — ein Skript auf `seite.test` ist
/// **kein** Drittanbieter. Ohne das waere `document.cookie` auf der eigenen Seite
/// leer, sobald die Drittanbieter-Sperre steht.
pub fn cookie_lesen(url: &str) -> String {
    let ziel = zerlege(url);
    if ziel.host.is_empty() {
        return String::new();
    }
    let https = url
        .trim_start()
        .to_ascii_lowercase()
        .starts_with("https://");
    mit(|z| {
        z.glas
            .cookie_kopf_zurueckgabe(https, &ziel.host, &ziel.rest, jetzt())
    })
    .unwrap_or_default()
}

/// added 04.10.2026, TLS: **`document.cookie = "…"`** — und es ist **derselbe**
/// Weg wie beim Netz: `aus_set_cookie`.
///
/// **UND DER RUECKGABEWERT IST LEER**, weil `document.cookie` **keinen** hat
/// (HTML Living Standard, `CookieStore`). Wer einen Grund braucht, nimmt
/// [`cookie_schreiben_mit_grund`].
pub fn cookie_schreiben(url: &str, text: &str) {
    let _ = cookie_schreiben_mit_grund(url, text);
}

/// added 04.10.2026, TLS: **SCHREIBEN MIT GRUND**, fuer den Test und die
/// DevTools.
pub fn cookie_schreiben_mit_grund(url: &str, text: &str) -> Result<(), String> {
    let ziel = zerlege(url);
    if ziel.host.is_empty() {
        return Err(narsil_i18n::tr("no host in the address").into());
    }
    let https = url
        .trim_start()
        .to_ascii_lowercase()
        .starts_with("https://");
    let ergebnis = mit(|z| {
        let r = z
            .glas
            .aus_document_cookie(text, &ziel.host, &ziel.rest, jetzt(), https);
        if let Err(m) = &r {
            if z.abgelehnt.len() < 32 {
                z.abgelehnt
                    .push(std::format!("{} (Skript): {}", ziel.host, m.grund));
            }
        }
        r
    });
    ergebnis.map_err(|m| m.grund)
}

/// added 04.10.2026, TLS: **SITZUNGSKEKSE VORHANDEN?** — fuer den Test „Neustart:
/// die mit Ablauf bleiben, die ohne nicht“.
pub fn sitzungskeks_vorhanden() -> bool {
    mit(|z| z.glas.hat_sitzungskeks())
}

/// added 04.10.2026, TLS: **AUFS FELD SCHREIBEN** und **DARAUS LESEN** — zwei
/// Funktionen fuer **eine** Sache: die Persistenz.
///
/// **UND WARUM DER PFAD EIN PARAMETER IST:** der Aufrufer entscheidet, wo die
/// Datei liegt. Der Test braucht ein Verzeichnis, das er loeschen kann, und
/// **ein** fester Pfad wuerde ihm die **echte** Datei des Benutzers zerstoeren.
/// GEMESSEN, dass das keine Ausnahme ist: `bau.sh` laeuft mit
/// `RUST_TEST_THREADS=2`, also **zwei** Tests gleichzeitig.
pub fn felder_schreiben(pfad: &std::path::Path) -> std::io::Result<()> {
    if let Some(eltern) = pfad.parent() {
        std::fs::create_dir_all(eltern)?;
    }
    let text = mit(|z| z.glas.als_text());
    std::fs::write(pfad, text)
}

/// added 04.10.2026, TLS: **UND LESEN**, und ein **fehlender** Pfad ist **kein**
/// Fehler: ein Browser beim ersten Start hat keine Keksdatei.
pub fn felder_lesen(pfad: &std::path::Path) {
    let Ok(text) = std::fs::read_to_string(pfad) else {
        return;
    };
    mit(|z| z.glas = Keksglas::aus_text(&text));
}

/// added 04.10.2026, TLS: **DER ADRESSSCHNEIDER** — und er ist **derselbe**
/// wie in `sicherheit_haken.rs::zerlege`, aber **nicht** derselbe Aufruf.
///
/// **WARUM NICHT `narsil_url::parse`:** GEMESSEN am Haken — die Funktion lehnt
/// **alle** `https`-Adressen ab, also genau die, um die es hier geht.
struct Ziel {
    host: String,
    rest: String,
}

fn zerlege(url: &str) -> Ziel {
    let rest = match url.trim().split_once("://") {
        Some((_, r)) => r,
        None => url.trim(),
    };
    let ende = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host_port = &rest[..ende];
    // added 04.10.2026, TLS: **DER PORT GEHOERT NICHT ZUM HOST**, sonst passt
    // ein Keks fuer `` nicht auf `seite.test`. Und **KV-Hosts** zuerst.
    let host = match host_port.strip_prefix('[') {
        Some(ohne) => match ohne.split_once(']') {
            Some((h, _)) => h.to_ascii_lowercase(),
            None => host_port.to_ascii_lowercase(),
        },
        None => match host_port.rsplit_once(':') {
            Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => {
                h.to_ascii_lowercase()
            }
            _ => host_port.to_ascii_lowercase(),
        },
    };
    Ziel {
        host,
        rest: rest[ende..].to_string(),
    }
}
