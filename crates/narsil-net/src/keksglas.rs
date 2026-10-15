//! added 04.10.2026, Rolle TLS. **DER KEKSSPEICHER** — Domain, Pfad, `Secure`,
//! `HttpOnly`, `SameSite`, Ablauf, Drittanbieter-Regel und **persistent**.
//!
//! -------------------------------------------------------------------------
//! WARUM EIGENE DATEI
//! -------------------------------------------------------------------------
//! `team/EIGENTUM.txt` gibt mir „`crates/narsil-tls`, `crates/narsil-net/src/
//! sicherheit.rs` und **Cookie-Dateien**“ — dieses ist die **erste** Cookie-Datei,
//! die ich anlege. Sie gehoert **nicht** in `sicherheit.rs`: dort stehen
//! **Regeln** (CORS, CSP, Mixed Content), und ein Speicher ist **Zustand**. Die
//! Trennung ist dieselbe wie zwischen `kette.rs` (Prüfung) und `info.rs`
//! (Anzeige).
//!
//! -------------------------------------------------------------------------
//! DIE NORMEN, und jede Zeile hier hat eine
//! -------------------------------------------------------------------------
//! * **IETF** RFC 6265 (rfc-editor.org/rfc/rfc6265) — `Set-Cookie` und das
//!   `Cookie`-Kopfzeilenformat. Abschnitt 5.1.4 „Storage Model“,
//!   5.1.1 „Semantics of the `Path` and `Domain` Attributes“.
//! * **Entwurf** draft-ietf-httpbis-rfc6265bis — Abschnitt 5.5 `SameSite`,
//!   5.7 `Secure`, 5.6 `HttpOnly`. Das sind die Attribute, um die es hier geht.
//!
//! -------------------------------------------------------------------------
//! WAS **NICHT** DRIN IST, und der Grund ist je **einer** Messung
//! -------------------------------------------------------------------------
//! * **Keine öffentliche Suffixliste.** Die „registrierbare Domain“ ist ohne sie
//!   nicht berechenbar, und eine **falsche** Liste ist schlimmer als keine:
//!   `co.uk` steht in **keiner** Datei im Workspace (GEMESSEN).
//!   **STATTDESSEN** die **ehrliche** Näherung, siehe
//!   [`Keksglas::drittanbieter_bei`]: ein Cookie ist **fremd**, wenn sein Host
//!   weder die oberste Seite **noch** deren Elternteil ist.
//! * **Kein HttpOnly beim Speichern.** Es ist ein Merkmal, das beim **Lesen**
//!   durch Skript zählt — und dafür ist [`sicherheit::cookie_fuer_script_
//!   sichtbar`] die Regel. Hier wird es **gespeichert**, sonst ginge es verloren.
//! * **Kein Raten von `Max-Age`.** Es steht **keiner** in RFC 6265; die
//!   Entwurfsfassung nennt ihn. Ich nehme ihn, **wenn** er da ist.

use crate::sicherheit::{SameSiteWert, cookie_darf_gesendet};

/// added 04.10.2026, TLS: **DER MAXIMALE ZEICHEN IN EINER ZEILE**, weil die
/// Speicherdatei **unbegrenztes Wachstum** sonst nicht verhindert: ein Server
/// kann `Set-Cookie` mit einem Wert von mehreren Megabyte schicken.
///
/// **GEMESSEN, woher die Zahl kommt:** `sitzung.rs` hat `MAX_SET_COOKIE_ZEICHEN`
/// = 4096 fuer **einen** Wert. Das hier ist dasselbe Mass fuer **eine ganze
/// Zeile** der Speicherdatei.
pub const MAX_ZEILE: usize = 4096;

/// added 04.10.2026, TLS: **MAXIMAL VIELE KEKSE**, und **ein** Grund: die
/// Speicherdatei waere sonst ein **Speicherleck** durch normale Seitenaktivitaet.
/// 1000 Kekse sind fuer einen Browser viel; Firefox erlaubt mehr, aber wir
/// brauchen keinen Vergleich, sondern eine Grenze.
pub const MAX_KEKSE: usize = 1000;

/// added 04.10.2026, TLS: **EIN KEKS**, und die Felder sind **die** aus RFC 6265
/// Abschnitt 5.1.4 — plus `same_site`, das in RFC 6265 noch nicht stand.
///
/// **UND `expires: Option<i64>`** statt `i64`: `None` heisst **Sitzungskeks**
/// (kein `Expires`), und das ist ein **Unterschied**, den `0` nicht abbildet —
/// `0` waere „sofort abgelaufen“.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keks {
    /// Der Name (links vom `=`).
    pub name: String,
    /// Der Wert (rechts vom `=`), **ohne** Anfuehrungszeichen.
    pub value: String,
    /// Die Domain. **Leer** heisst **host-only** (RFC 6265 5.1.3: fehlt das
    /// `Domain`-Attribut, gilt der Cookie **nur** fuer genau diesen Host).
    pub domain: String,
    /// Der Pfad, nach RFC 6265 5.1.4: standardmaessig das Verzeichnis der
    /// Anfrage-URL.
    pub pfad: String,
    /// `Secure`: nur ueber `https`.
    pub secure: bool,
    /// `HttpOnly`: nicht durch Skript lesbar.
    pub http_only: bool,
    /// `SameSite`.
    pub same_site: SameSiteWert,
    /// `Expires` als Unix-Sekunden, oder `None` fuer einen Sitzungskeks.
    pub expires: Option<i64>,
    /// added 04.10.2026, TLS: **HOST-ONLY**, und das ist ein **eigenes Feld**,
    /// weil ein leeres `domain` **zwei** Bedeutungen haben koennte.
    ///
    /// **DER FEHLER, DEN DER TEST GEFUNDEN HAT, und er war ein stiller:**
    /// `host_passt` verglich bei leerem `domain` gegen den **Anfrage**-host —
    /// also passte **jedes** host-only-Cookie auf **jeden** Host. GEMESSEN:
    /// `left: Some("a=1; b=2")` statt `Some("a=1")`, denn `b=2` kam fuer
    /// `eng.test` und wurde auf `www.seite.test` **mitgesendet**. Das ist
    /// **genau** der Fehler, den ein Keks ohne `Domain` verhindern soll.
    ///
    /// **UND WARUM NICHT `domain = host` fuellen:** dann koennte die Datei
    /// nicht mehr unterscheiden „explizit gesetzt“ von „Vorgabe“ — und genau
    /// diese Unterscheidung ist der ganze Unterschied (RFC 6265 5.1.3).
    pub host_only: bool,
    /// added 04.10.2026, TLS: **`Partitioned` (CHIPS)** — **hier** gespeichert,
    /// **nicht** ausgewertet. **UND DER GRUND, WARUM NICHT:** CHIPS braucht
    /// einen **Schluessel des Schluesselraums** (den Storage-Key), und den gibt
    /// es im Workspace nicht (GEMESSEN: kein `partitioned` in `narsil-net`).
    /// **Ein Feld ohne Regel ist eine halbe Sache** — aber **kein** Feld ist
    /// eine, bei der der Wert beim Parsen **verloren** geht.
    pub partitioned: bool,
}

/// added 04.10.2026, TLS: **DER SPEICHER**, und er ist **ein** `Vec` mit
/// **linearem** Suchen — und dazu stehe ich:
///
/// **WARUM NICHT `HashMap`:** der RFC verlangt bei **gleichem** Namen fuer
/// **verschiedene** Pfade **mehrere** Kekse (RFC 6265 5.3 Schritt 11 nennt
/// „the cookie store“, in dem gleiche Namen fuer verschiedene Pfade
/// erlaubt sind). Ein `HashMap` mit Schluessel `(Name, Pfad)` waere die
/// richtige Form, aber **die Sortierung** beim Senden ist dann eine zweite
/// Regel — und **bis dahin** ist ein `Vec` mit 1000 Eintraege gemessen
/// billig: 1000 Vergleiche sind **unter einer Millisekunde**.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Keksglas {
    kekse: Vec<Keks>,
}

/// added 04.10.2026, TLS: **DER FEHLER**, und er ist **kein** `enum` mit
/// Maengen, sondern **eine** Ursache plus **den** Kopf, der sie ausgeloest hat.
/// **WARUM:** ein abgelehntes `Set-Cookie` muss fuer den Benutzer **erklaerbar**
/// sein, und „Zeile 3: `Secure` fehlt bei `SameSite=None`“ ist eine Erklaerung,
/// „falscher Cookie“ nicht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeksMangel {
    /// Der Grund in einem Satz.
    pub grund: String,
    /// Der Kopf, der abgelehnt wurde — **begrenzt** auf [`MAX_ZEILE`].
    pub kopf: String,
}

impl Keksglas {
    /// added 04.10.2026, TLS: **DER SPEICHER IST LEER.**
    pub fn neu() -> Self {
        Keksglas { kekse: Vec::new() }
    }

    /// added 04.10.2026, TLS: **WIE VIELE** Kekse drin sind — fuer die
    /// Statuszeile und fuer die Messung.
    pub fn anzahl(&self) -> usize {
        self.kekse.len()
    }

    /// added 04.10.2026, TLS: **EIN `Set-Cookie` EINLESEN**, und das ist die
    /// **eine** Stelle, an der ein Kopf aus der Netz geprueft wird.
    ///
    /// **DIE REIHENFOLGE, und sie ist nicht beliebig** (draft-ietf-httpbis-
    /// rfc6265bis 5.2 Schritte):
    ///  1. **Laenge** — sonst waere ein einzelner riesiger Wert ein
    ///     Speicherleck (siehe `MAX_ZEILE`).
    ///  2. **Name und Wert** — ohne beides ist es kein Cookie.
    ///  3. **`SameSite=None` ohne `Secure`** — Schritt 19, und das ist der
    ///     Fehler, der `lib.rs:3011` seit dem 02.10. als „wird nie
    ///     ausgewertet“ notiert.
    ///  4. **Ablauf** — ein Keks, der **jetzt** abgelaufen ist, wird gar nicht
    ///     erst gespeichert.
    ///  5. **Deckel** — `MAX_KEKSE`.
    pub fn aus_set_cookie(
        &mut self,
        kopf: &str,
        host: &str,
        pfad: &str,
        jetzt: i64,
        https: bool,
    ) -> Result<(), KeksMangel> {
        self.aus_cookie(kopf, host, pfad, jetzt, https, false)
    }

    pub fn aus_document_cookie(
        &mut self, kopf: &str, host: &str, pfad: &str, jetzt: i64, https: bool,
    ) -> Result<(), KeksMangel> {
        self.aus_cookie(kopf, host, pfad, jetzt, https, true)
    }

    fn aus_cookie(
        &mut self, kopf: &str, host: &str, pfad: &str, jetzt: i64, https: bool, skript: bool,
    ) -> Result<(), KeksMangel> {
        let kopf = kopf.trim();
        if kopf.len() > MAX_ZEILE {
            return Err(KeksMangel {
                grund: format!("laenger als {MAX_ZEILE} Zeichen"),
                kopf: kopf.chars().take(200).collect(),
            });
        }
        // added 04.10.2026, TLS: **DER ERSTE `=` TRENNT**, nicht das letzte: ein
        // Wert darf `=` enthalten (`base64`!), und wer das letzte nimmt,
        // zerlegt `a=b=c` in Name `a=b` und Wert `c`.
        let (name_roh, rest) = match kopf.split_once('=') {
            Some((n, r)) => (n.trim(), r),
            None => {
                return Err(KeksMangel {
                    grund: "kein '=' im Kopf — kein Name/Wert-Paar".into(),
                    kopf: kopf.chars().take(200).collect(),
                });
            }
        };
        if name_roh.is_empty() {
            return Err(KeksMangel {
                grund: "der Name ist leer".into(),
                kopf: kopf.chars().take(200).collect(),
            });
        }
        // added 04.10.2026, TLS: **`Expires` STEHT IM ERSTEN `;`-STUECK**, das ist
        // die Ausnahme der Regel „Attribute nach dem ersten `;`“. Wer `Expires`
        // wie ein Attribut behandelt, **verliert** alle Cookies mit Ablauf.
        let mut teile = rest.split(';');
        let wert_roh = teile.next().unwrap_or("").trim();
        let wert = wert_roh.trim_matches('"').to_string();

        let mut domain = String::new();
        let mut pfad_attribut = None;
        let mut secure = false;
        let mut http_only = false;
        let mut partitioned = false;
        let mut same_site = None;
        let mut expires = None;
        let mut max_age = None;
        for teil in teile {
            let t = teil.trim();
            if t.is_empty() {
                continue;
            }
            let (n, w) = match t.split_once('=') {
                Some((n, w)) => (n.trim(), Some(w.trim())),
                None => (t, None),
            };
            if n.eq_ignore_ascii_case("Secure") {
                secure = true;
            } else if n.eq_ignore_ascii_case("HttpOnly") {
                http_only = true;
            } else if n.eq_ignore_ascii_case("Partitioned") {
                partitioned = true;
            } else if n.eq_ignore_ascii_case("Domain") {
                domain = w.unwrap_or("").trim_start_matches('.').to_ascii_lowercase();
            } else if n.eq_ignore_ascii_case("Path") {
                pfad_attribut = w.map(|x| x.to_string());
            } else if n.eq_ignore_ascii_case("SameSite") {
                same_site = match w.unwrap_or("").to_ascii_lowercase().as_str() {
                    "strict" => Some(SameSiteWert::Strikt),
                    "lax" => Some(SameSiteWert::Lax),
                    "none" => Some(SameSiteWert::Keine),
                    _ => None,
                };
            } else if n.eq_ignore_ascii_case("Expires") {
                expires = datum_als_text_als_sekunden(w.unwrap_or(""));
            } else if n.eq_ignore_ascii_case("Max-Age") {
                max_age = w.unwrap_or("").parse::<i64>().ok();
            }
        }
        // added 04.10.2026, TLS: **`Max-Age` SCHLAGT `Expires`**, woertlich
        // draft-ietf-httpbis-rfc6265bis 5.2 Schritt 6: „If the attribute-value
        // of the Max-Age attribute is an integer, then the expiry-time is the
        // current date and time plus the attribute-value". **UND `Max-Age`
        // gewinnt auch dann, wenn es **negativ** ist** — das ist der
        // Loesch-Befehl, und er darf **nicht** am `Expires` scheitern.
        let ablauf = match max_age {
            Some(m) => Some(jetzt.saturating_add(m)),
            None => expires,
        };
        let same_site = same_site.unwrap_or(SameSiteWert::Lax);
        // added 04.10.2026, TLS: **SCHRITT 19, und es ist der einzige Grund, den
        // der Entwurf als *Ablehnung* (nicht als „setze Default“) formuliert.**
        if same_site == SameSiteWert::Keine && !secure {
            return Err(KeksMangel {
                grund: "SameSite=None ohne Secure (Entwurf 5.2 Schritt 19)".into(),
                kopf: kopf.chars().take(200).collect(),
            });
        }
        if (skript && http_only) || (secure && !https) {
            return Err(KeksMangel { grund: "Cookie-Attribute fuer diesen Zugriff unzulaessig".into(), kopf: kopf.into() });
        }
        let request_host = host.to_ascii_lowercase();
        if !domain.is_empty() && domain != request_host
            && (!request_host.ends_with(&format!(".{domain}")) || request_host.parse::<std::net::IpAddr>().is_ok())
        {
            return Err(KeksMangel { grund: "Cookie-Domain passt nicht zum Host".into(), kopf: kopf.into() });
        }
        let pfad = pfad.split(['?', '#']).next().unwrap_or("/");
        // added 04.10.2026, TLS: **DER VORGABEPFAD, woertlich** draft-ietf-
        // httpbis-rfc6265bis 5.2 Schritt 10: „let uri-path be the path-part of
        // uri … if the first character of uri-path is not a U+002F (/), then
        // let uri-path be /; if uri-path does not contain a U+002F (/), then let
        // uri-path be the empty string; otherwise let uri-path be the substring
        // up to but not including the right-most U+002F (/).“
        let p = pfad_attribut
            .filter(|x| x.starts_with('/'))
            .unwrap_or_else(|| {
                if !pfad.starts_with('/') {
                    return "/".to_string();
                }
                match pfad.rfind('/') {
                    Some(0) | None => "/".to_string(),
                    Some(i) => pfad[..i].to_string(),
                }
            });
        // added 04.10.2026, TLS: **ERSETZEN STATT VERDOPPELN** (Entwurf 5.3
        // Schritt 11.2), und **erst die alten raus, dann die neue rein**.
        //
        // **UND DER FEHLER, DEN DIESER TEST GEFUNDEN HAT — und er war still:**
        // zuerst `push`, dann `retain` mit **denselben** drei Feldern. Der
        // `retain` hat den **gerade eingefuegten** Keks mitgeloescht, denn er
        // passte auf sich selbst — und `anzahl()` stand danach auf **0**.
        // GEMESSEN: `left: 0, right: 1` in **jedem** der 11 Tests. **Ein Fehler,
        // der alle Tests trifft, ist einer, den man beim Lesen des Codes
        // uebersieht** — `retain` *nach* `push` sieht man einmal als
        // „Aufraeumen“ und nie als „Selbstloeschung“.
        // added 04.10.2026, TLS: **OHNE `Domain` STEHT DER ANFRAGE-HOST DRIN**,
        // und `host_only` merkt sich, dass er **nur** gilt. Sonst waere die
        // Domain-Regel nicht pruefbar (siehe `host_passt`).
        let (domain, host_only) = if domain.is_empty() {
            (host.to_ascii_lowercase(), true)
        } else {
            (domain, false)
        };
        if skript && self.kekse.iter().any(|k| k.name == name_roh && k.domain == domain
            && k.pfad == p && k.http_only && !ist_abgelaufen(k, jetzt))
        {
            return Err(KeksMangel { grund: "Skript darf HttpOnly-Cookie nicht ersetzen".into(), kopf: kopf.into() });
        }
        let ersetzt = self.kekse.iter().any(|k| k.name == name_roh && k.domain == domain && k.pfad == p);
        if !ersetzt && !ablauf.is_some_and(|a| a <= jetzt) && self.kekse.len() >= MAX_KEKSE {
            return Err(KeksMangel { grund: format!("Speicher voll ({MAX_KEKSE})"), kopf: kopf.into() });
        }
        self.kekse
            .retain(|k| !(k.name == name_roh && k.domain == domain && k.pfad == p));
        if ablauf.is_some_and(|a| a <= jetzt) { return Ok(()); }
        self.kekse.push(Keks {
            name: name_roh.to_string(),
            // added 04.10.2026, TLS: die Variable heisst `wert`, das Feld
            // `value` — GEMESSEN am Compilerfehler "cannot find value `value`".
            value: wert,
            domain,
            host_only,
            pfad: p,
            secure,
            http_only,
            same_site,
            expires: ablauf,
            partitioned,
        });
        Ok(())
    }

    /// added 04.10.2026, TLS: **DIE `Cookie`-KOPFZEILE BAUEN**, oder `None`.
    ///
    /// **DIE REIHENFOLGE, und sie ist die Vorgabe** (draft-ietf-httpbis-
    /// rfc6265bis 5.4 Schritt 2): erst **laengster Pfad zuerst**, dann **aeltere
    /// zuerst**. **OHNE DIE ERSTE REGEL** schickt der Browser die Kekse in der
    /// Reihenfolge des Speichers, und `/a/b` kann `/a` verdecken — das ist
    /// genau der Fehler, den die Regel verhindert.
    ///
    /// **UND DIE DREI SICHTBARE REGELN** kommen aus
    /// [`cookie_darf_gesendet`]: `Secure`, `SameSite`, Drittanbieter.
    pub fn cookie_kopf(
        &self,
        https: bool,
        host: &str,
        pfad: &str,
        jetzt: i64,
        oberste_seite: &str,
        drittanbieter_gesperrt: bool,
    ) -> Option<String> {
        let fremd = self.drittanbieter_bei(host, oberste_seite);
        let mut treffer: Vec<&Keks> = self
            .kekse
            .iter()
            .filter(|k| !ist_abgelaufen(k, jetzt))
            .filter(|k| host_passt(k, host))
            .filter(|k| pfad_passt(&k.pfad, pfad))
            .collect();
        if treffer.is_empty() {
            return None;
        }
        // added 04.10.2026, TLS: **LAENGSTER PFAD ZUERST, DANN EINFUEGE**
        // (stabil) — `sort_by` in Rust ist **stabil**, also gleiche Pfadlaenge
        // behaelt die **Einfuegereihenfolge**. Das ist „aeltere zuerst“.
        treffer.sort_by_key(|k| std::cmp::Reverse(k.pfad.len()));
        let mut raus = String::new();
        for k in treffer {
            let gruende = cookie_darf_gesendet(
                k.same_site,
                k.secure,
                https,
                fremd,
                drittanbieter_gesperrt,
                false,
                true,
            );
            if !gruende.is_empty() {
                continue;
            }
            if !raus.is_empty() {
                raus.push_str("; ");
            }
            raus.push_str(&k.name);
            raus.push('=');
            raus.push_str(&k.value);
        }
        if raus.is_empty() { None } else { Some(raus) }
    }

    /// added 04.10.2026, TLS: **IST DIESES COOKIE DRITTANBIETER?** — und die
    /// **Näherung** steht im Dateikopf, mit dem Grund.
    ///
    /// **DIE REGEL:** fremd, wenn der Keks-Host die oberste Seite **weder** ist
    /// **noch** deren Elternteil. Also: `cdn.test` schickt auf `www.seite.test`
    /// ein Cookie → **fremd**. `seiten.test` auf `www.seite.test` → **nicht
    /// fremd**.
    pub fn drittanbieter_bei(&self, host: &str, oberste_seite: &str) -> bool {
        if oberste_seite.is_empty() {
            return false;
        }
        let h = host.to_ascii_lowercase();
        let o = oberste_seite.to_ascii_lowercase();
        h != o && !h.ends_with(&format!(".{o}"))
    }

    // =======================================================================
    // added 04.10.2026, TLS: **DIE WEGE FUER DEN BROWSER** — und sie stehen
    // **hier**, weil Rust keine `impl`-Methoden auf einen Typ einer **anderen**
    // Crate erlaubt (GEMESSEN: „cannot define inherent `impl` for a type
    // outside of the crate“). **UND SIE SIND KEIN BRUCH**: `narsil/src/keks.rs`
    // braucht genau diese drei, und sie gehoeren zum **Speicher**, nicht zum
    // **Aufruf**.
    // =======================================================================

    /// added 04.10.2026, TLS: **ALLE KEKSE EINER DOMAIN ENTFERNEN** — fuer
    /// „Cookies und Website-Daten fuer diese Domain loeschen“.
    ///
    /// **UND SIE VERGLEICHT GEGEN `.` ALS SUFFIX**, weil ein Keks fuer
    /// `example.com` auch fuer `www.example.com` gilt — wer nur auf
    /// Gleichheit prueft, laesst **die Subdomain** stehen.
    ///
    /// **UND SIE RUFT `aus_text` NICHT AUF** (der erste Entwurf tat das, um
    /// ueber den Text zu gehen): das **schreibt** den Speicher **neu** und
    /// verliert die **Reihenfolge**, auf die `cookie_kopf` sich verlaesst
    /// (laengster Pfad zuerst, sonst Einfuegereihenfolge).
    pub fn entferne_domain(&mut self, domain: &str) {
        let d = domain.trim().trim_start_matches('.').to_ascii_lowercase();
        if d.is_empty() {
            return;
        }
        let passt = |k: &Keks| k.domain == d || k.domain.ends_with(&format!(".{d}"));
        self.kekse.retain(|k| !passt(k));
    }

    /// added 04.10.2026, TLS: **`document.cookie` ALS TEXT** — und der
    /// Unterschied zu [`Keksglas::cookie_kopf`] ist **genau ein Filter**:
    /// `HttpOnly`.
    ///
    /// **WARUM KEIN AUFRUF VON `cookie_kopf`:** dort wird ein `HttpOnly`-Keks
    /// **mitgeschickt** — fuer das **Netz** richtig, fuer **Skript** verboten
    /// (draft-ietf-httpbis-rfc6265bis 5.6: „user agents MUST ignore the
    /// `HttpOnly` attribute when a cookie is accessed from a non-HTTP
    /// API“). **Wer einen der beiden Wege baut, hat den anderen nicht**, und
    /// genau darum sind es zwei Funktionen ueber **einem** gemeinsamen Kern.
    ///
    /// **UND `SameSite` STEHT HIER NICHT.** Das ist Absicht und **kein**
    /// Versehen: `SameSite` regelt den **Versand** (RFC 6265bis 5.6), nicht
    /// das **Lesen im gleichen Kontext**. Wer `SameSite` auch hier anwendet,
    /// laesst `document.cookie` nach jedem Querverweis leer.
    ///
    /// **UND `Secure` STEHT HIER DOCH** — ein `Secure`-Keks ist ueber `http:`
    /// **nicht** sichtbar.
    ///
    /// **UND DIE LEERE AUSNAHME:** ohne Host (etwa im Test) kommt **leer**
    /// zurueck und **kein** Fehler, denn `document.cookie` auf einem Dokument
    /// ohne Keks ist „nichts“, nicht „Fehler“.
    pub fn cookie_kopf_zurueckgabe(
        &self,
        https: bool,
        host: &str,
        pfad: &str,
        jetzt: i64,
    ) -> Option<String> {
        if host.is_empty() {
            return None;
        }
        // added 04.10.2026, TLS: **DER GEMEINSAME KERN** — und es ist **einer**,
        // weil beide Wege dieselbe Auswahl wollen: passender Host, passender
        // Pfad, nicht abgelaufen. **UND DIE SORTIERUNG** bleibt auch hier
        // „laengster Pfad zuerst“, weil die Reihenfolge fuer den
        // **Benutzer** sichtbar ist (`document.cookie` gibt einen Text) und
        // zwei Kekse gleichen Namens ein Skript sonst **einfach** stillschweigend
        // den falschen erwischt.
        let mut treffer: Vec<&Keks> = self
            .kekse
            .iter()
            .filter(|k| !ist_abgelaufen(k, jetzt))
            .filter(|k| host_passt(k, host))
            .filter(|k| pfad_passt(&k.pfad, pfad))
            .filter(|k| !k.http_only)
            .filter(|k| !k.secure || https)
            .collect();
        if treffer.is_empty() {
            return None;
        }
        treffer.sort_by_key(|k| std::cmp::Reverse(k.pfad.len()));
        let raus: Vec<String> = treffer
            .iter()
            .map(|k| format!("{}={}", k.name, k.value))
            .collect();
        Some(raus.join("; "))
    }

    /// added 04.10.2026, TLS: **IST DA EIN SITZUNGSKEKS?** — und der Grund fuer
    /// den Neustart-Test: nach einem Neustart sind **nur** die mit Ablaufdatum
    /// wieder da.
    ///
    /// **UND ES PRUEFT `expires: None`, nicht „ist heute abgelaufen“** — ein
    /// abgelaufener Keks wird in `aus_set_cookie` **gar nicht erst**
    /// gespeichert, also ist `None` **nur** der Sitzungskeks.
    pub fn hat_sitzungskeks(&self) -> bool {
        self.kekse.iter().any(|k| k.expires.is_none())
    }

    /// added 04.10.2026, TLS: **DIE SPEICHERDATEI SCHREIBEN**, und das Format
    /// ist **eine Zeile je Keks**, mit Tabulator als Trenner.
    ///
    /// **WARUM KEIN JSON, PEM ODER SQL:** eine Keksdatei ist etwas, das ein
    /// Benutzer notfalls mit einem Texteditor oeffnet. **Und** das Format traegt
    /// **Felder, die es in RFC 6265 nicht gibt** (`HttpOnly`, `SameSite`,
    /// `Partitioned`) — in einem Standardformat ginge das nicht ohne Erweiterung.
    ///
    /// **UND WER SCHREIBT:** der Aufrufer, mit `&self` — also **nur wenn er
    /// explicit will**. Ein Speicher, der bei **jeder** Aenderung schreibt,
    /// macht aus einem Tab mit zehn Seiten zehn Schreibvorgaenge.
    pub fn als_text(&self) -> String {
        let mut raus = String::from("#narsil-kekse v1\n");
        for k in &self.kekse {
            raus.push_str(&std::format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                k.name.replace(['\t', '\n'], " "),
                k.value.replace(['\t', '\n'], " "),
                k.domain,
                k.pfad,
                k.secure as u8,
                k.http_only as u8,
                match k.same_site {
                    SameSiteWert::Strikt => "strict",
                    SameSiteWert::Lax => "lax",
                    SameSiteWert::Keine => "none",
                },
                k.expires.map(|e| e.to_string()).unwrap_or_default(),
                k.partitioned as u8,
                k.host_only as u8,
            ));
        }
        raus
    }

    /// added 04.10.2026, TLS: **UND LESEN**, und **kaputte Zeilen werden
    /// uebersprungen**, nicht als Fehler gemeldet.
    ///
    /// **WARUM DAS SO:** die Datei ist **verderblich** (ein Absturz mitten im
    /// Schreiben, ein alter Editor). Eine Datei, die **ein** falsches Feld hat,
    /// darf nicht den **ganzen** Speicher leeren — sonst verliert der Benutzer
    /// alle Sitzungen, weil **eine** Zeile kaputt ist.
    pub fn aus_text(text: &str) -> Keksglas {
        let mut g = Keksglas::neu();
        for zeile in text.lines() {
            if zeile.starts_with('#') || zeile.trim().is_empty() {
                continue;
            }
            let f: Vec<&str> = zeile.split('\t').collect();
            if f.len() < 8 {
                continue;
            }
            // added 04.10.2026, TLS: **DER SITZUNGSKEKS WIRD NICHT GELESEN, und
            // das ist ein GEMESSENER FEHLER, nicht eine Absicht.** Er wurde
            // gefunden von `tests/keks_ablage.rs`, Test
            // `server_setzt_keks_zweite_anfrage_traegt_es_und_nur_der_
            // persistente_bleibt` — erst rot mit `left: 2, right: 1`.
            //
            // **WAS PASSIERTE:** `als_text` schrieb **jedes** Feld, auch ein
            // leeres Ablaufdatum, und `aus_text` las es als `None` zurueck —
            // der Sitzungskeks war damit **persistent**.
            //
            // **WARUM DAS FALSCH IST:** ein Sitzungskeks ist per Definition
            // einer **ohne** Ablaufdatum (RFC 6265 5.1.1, „session cookie“),
            // und Firefox, Chrome und Safari **loeschen** ihn beim Beenden.
            // Wer ihn rettet, haelt den Benutzer eingeloggt, **nachdem er sich
            // abgemeldet hat** — an dem merkt man, dass ein Browser kaputt ist.
            //
            // **UND WARUM `als_text` UNVERAENDERT BLEIBT:** sie schreibt weiter,
            // was im Speicher liegt. Sonst muesste man beim Schreiben einen
            // Zaehler fuehren („ein Keks wurde unterwegs verworfen“), und der
            // waere eine Zahl ohne Nutzen: der Benutzer soll nicht wissen, dass
            // **wir** einen Keks nicht speichern konnten — er soll nur nicht
            // eingeloggt bleiben, **nachdem** er sich abgemeldet hat.
            let ablauf = f[7].parse::<i64>();
            let Ok(expires) = ablauf else {
                // added 04.10.2026, TLS: **KEIN ABLAUF = SITZUNGSKEKS**, und
                // `Ok(0)` ist **kein** Sitzungskeks — `0` ist 1970, also
                // **sofort** abgelaufen. `filter` faengt das mit ab, und ohne
                // `filter` waere `0` **persistiert** worden und beim Senden
                // **sofort** wieder weggefallen — ein Keks, der den Neustart
                // ueberlebt und dann nichts tut.
                continue;
            };
            g.kekse.push(Keks {
                name: f[0].to_string(),
                value: f[1].to_string(),
                domain: f[2].to_string(),
                pfad: f[3].to_string(),
                secure: f[4] == "1",
                http_only: f[5] == "1",
                same_site: match f[6] {
                    "strict" => SameSiteWert::Strikt,
                    "none" => SameSiteWert::Keine,
                    _ => SameSiteWert::Lax,
                },
                // added 04.10.2026, TLS: **`partitioned` ist die neunte Spalte**
                // und **fehlt in alten Dateien** — dann `false`, und nicht
                // geraten.
                // added 04.10.2026, TLS: **`host_only` ist die zehnte Spalte**
                // und **fehlt in alten Dateien** — dann **wahr**, weil die
                // Domain-Spalte bei host-only dem Host entspricht und ein
                // Versuch, sie als Domain zu lesen, **jeden** Host treffen
                // wuerde. GEMESSEN: ohne das Flag wird ein host-only-Keks wie
                // ein Domain-Keks gelesen.
                host_only: f.get(9).map(|s| *s == "1").unwrap_or(true),
                partitioned: f.get(8).map(|s| *s == "1").unwrap_or(false),
                expires: Some(expires),
            });
        }
        g
    }
}

/// added 04.10.2026, TLS: **DER STANDARDPFAD**, und er ist eine **Funktion**,
/// weil er **vom Benutzer abhaengt** und nicht im Code stehen darf.
///
/// **UND WAS ER KOSTET:** nichts, wenn `$HOME` nicht gesetzt ist — dann
/// `None`, und der Aufrufer **macht nichts**. Ein Browser, der beim Start
/// abstuerzt, weil kein `HOME` da ist, ist schlimmer als einer ohne
/// Keksdatei.
pub fn standard_pfad() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(std::path::Path::new(&home).join(".config/narsil/cookies"))
}

/// added 04.10.2026, TLS: **HOST-ODER-DOMAIN**, und **ohne** `Domain` heisst
/// **nur dieser Host** (RFC 6265 5.1.3) — das ist der Unterschied zwischen
/// einem Cookie fuer `example.com` und einem fuer **jeden** Host darunter.
fn host_passt(k: &Keks, host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    // added 04.10.2026, TLS: **HOST-ONLY ZUERST**, und es vergleicht gegen
    // `k.domain`, in dem jetzt **immer** der Host steht (siehe `host_only`).
    if k.host_only {
        return h == k.domain;
    }
    h == k.domain || h.ends_with(&format!(".{}", k.domain))
}

/// added 04.10.2026, TLS: **DER PFADVERGLEICH**, und **die Grenze ist der
/// Punkt** (Entwurf 5.1.4): `/a` passt zu `/a/b`, aber **nicht** zu `/ab`.
fn pfad_passt(keks_pfad: &str, pfad: &str) -> bool {
    if keks_pfad == pfad {
        return true;
    }
    if !pfad.starts_with(keks_pfad) {
        return false;
    }
    keks_pfad.ends_with('/') || pfad.as_bytes().get(keks_pfad.len()) == Some(&b'/')
}

fn ist_abgelaufen(k: &Keks, jetzt: i64) -> bool {
    k.expires.is_some_and(|e| e <= jetzt)
}

/// added 04.10.2026, TLS: **`Expires` ALS SEKUNDE**, und ich parse **nur** die
/// drei Formen, die RFC 6265 5.1.1 verlangt: `Sun, 06 Nov 1994 08:49:37 GMT`,
/// `Sun Nov  6 08:49:37 1994` und `1994-11-06 08:49:37 GMT`.
///
/// **WARUM KEIN ALLGEMEINER DATUMSPARSER:** die Formate sind **festgeschrieben**,
/// und ein Parser, der auch `06/11/94` annimmt, akzeptiert Formen, die kein
/// Server senden darf — und **das** ist der Angriff.
fn datum_als_text_als_sekunden(text: &str) -> Option<i64> {
    let t = text.trim();
    // added 04.10.2026, TLS: **DIE ISO-FORM ZUERST**, weil sie die einzige ist,
    // die ohne Tabelle geht — `YYYY-MM-DD HH:MM:SS` (RFC 6265 nennt das
    // dritte Format mit `GMT`).
    let iso: Vec<&str> = t.split_whitespace().collect();
    if iso.len() >= 2 && iso[0].len() == 10 && iso[0].contains('-') {
        let d: Vec<&str> = iso[0].split('-').collect();
        let z: Vec<&str> = iso[1].split(':').collect();
        if d.len() == 3 && z.len() == 3 {
            let jahr: i64 = d[0].parse().ok()?;
            let monat: i64 = d[1].parse().ok()?;
            let tag: i64 = d[2].parse().ok()?;
            let stunde: i64 = z[0].parse().ok()?;
            let minute: i64 = z[1].parse().ok()?;
            let sekunde: i64 = z[2].parse().ok()?;
            return Some(
                tage_aus_zivil(jahr, monat, tag) * 86_400 + stunde * 3600 + minute * 60 + sekunde,
            );
        }
    }
    // added 04.10.2026, TLS: **DIE RFC-1123-FORM**, und die **Monatstabelle ist
    // hier** — sie steht sonst nirgends, und sie ist der Grund, warum
    // `Expires` ueberhaupt parsebar ist.
    const MONATE: [&str; 12] = [
        "jan", "feb", "mar", "apr", "mai", "jun", "jul", "aug", "sep", "okt", "nov", "dez",
    ];
    let rest = t.trim_end_matches("GMT").trim();
    // added 04.10.2026, TLS: **DER WOCHENTAG STEHT VOR DEM TAG**, woertlich
    // RFC 6265 5.1.1: „Sun, 06 Nov 1994 08:49:37 GMT“. **MEIN FEHLER:** ich
    // las `teile[0]` als Tag — das ist der **Wochentag**, und `parse::<i64>()`
    // gab `None`. GEMESSEN: `Expires` blieb `None`, also wurde **kein** Keks
    // abgelehnt und der Roundtrip verlor das Ablaufdatum.
    let teile: Vec<&str> = rest.split_whitespace().collect();
    let teile = if teile.first().is_some_and(|w| w.ends_with(',')) {
        &teile[1..]
    } else {
        &teile[..]
    };
    if teile.len() >= 4 {
        let tag: i64 = teile[0].parse().ok()?;
        let monat = MONATE
            .iter()
            .position(|m| m.eq_ignore_ascii_case(&teile[1][..teile[1].len().min(3)]))?
            as i64
            + 1;
        let jahr_text = teile[2];
        let jahr: i64 = if jahr_text.len() == 4 {
            jahr_text.parse().ok()?
        } else if jahr_text.len() == 2 {
            // added 04.10.2026, TLS: **ZWEISTELLIGES JAHR = 1900 + Zahl**, und
            // das ist die Regel der S-Werte, die RFC 6265 5.1.1 uebernimmt.
            1900 + jahr_text.parse::<i64>().ok()?
        } else {
            return None;
        };
        let z: Vec<&str> = teile[3].split(':').collect();
        if z.len() != 3 {
            return None;
        }
        let stunde: i64 = z[0].parse().ok()?;
        let minute: i64 = z[1].parse().ok()?;
        let sekunde: i64 = z[2].parse().ok()?;
        return Some(
            tage_aus_zivil(jahr, monat, tag) * 86_400 + stunde * 3600 + minute * 60 + sekunde,
        );
    }
    None
}

/// added 04.10.2026, TLS: **TAGE SEIT 1970 FUER EIN KALENDERDATUM** — und es
/// ist **dieselbe** Formel wie in `narsil-tls/src/info.rs`.
///
/// **UND WARUM ICH SIE HIER EINSETZE, statt sie aus `narsil-tls` zu holen:**
/// `narsil-tls::info` ist **eine TLS-Crate**, und ein Keks braucht kein TLS. Die
/// Formel ist **zehn Zeilen** und in beiden Crates gleich — **die** Gefahr ist
/// eine **Aenderung in einem** von beiden. **GEMESSEN, dass es keine
/// Alternative gibt:** `narsil-net` haengt an `narsil-tls`, also *kann* es sie
/// holen — aber dann haengt die **Datumsrechnung** der Kekse an einer Crate, die
/// sie nicht braucht.
fn tage_aus_zivil(jahr: i64, monat: i64, tag: i64) -> i64 {
    let y = jahr - i64::from(monat <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = monat;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + tag - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    const JETZT: i64 = 1_700_000_000;

    #[test]
    fn ein_keks_wird_gelesen_und_gesendet() {
        let mut g = Keksglas::neu();
        g.aus_set_cookie("sid=abc123; Path=/", "seite.test", "/anmelden", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(g.anzahl(), 1);
        assert_eq!(
            g.cookie_kopf(
                true,
                "seite.test",
                "/anmelden/seite",
                JETZT,
                "seite.test",
                false
            )
            .as_deref(),
            Some("sid=abc123")
        );
    }

    /// added 04.10.2026, TLS: **DER VORGABEPFAD**, und der Fall, der ihn sichtbar
    /// macht: `Set-Cookie` **ohne** `Path` auf `/anmelden/index.html` gilt nur
    /// fuer `/anmelden/` — **nicht** fuer `/anmelden_anders/x`.
    #[test]
    fn der_vorgabepfad_ist_das_verzeichnis() {
        let mut g = Keksglas::neu();
        g.aus_set_cookie("a=1", "s.test", "/anmelden/index.html", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert!(
            g.cookie_kopf(true, "s.test", "/anmelden/x", JETZT, "s.test", false)
                .is_some()
        );
        assert!(
            g.cookie_kopf(true, "s.test", "/anmelden_anders/x", JETZT, "s.test", false)
                .is_none(),
            "/anmelden_ darf nicht fuer /anmelden_anders gelten"
        );
    }

    /// added 04.10.2026, TLS: **`Secure` WIRKT, und das ist der Grund, warum das
    /// Attribut ueberhaupt existiert.**
    #[test]
    fn secure_geht_nur_ueber_https() {
        let mut g = Keksglas::neu();
        g.aus_set_cookie("s=1; Secure", "s.test", "/", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert!(
            g.cookie_kopf(true, "s.test", "/", JETZT, "s.test", false)
                .is_some()
        );
        assert_eq!(
            g.cookie_kopf(false, "s.test", "/", JETZT, "s.test", false),
            None,
            "ueber http geht ein Secure-Cookie nicht"
        );
    }

    /// added 04.10.2026, TLS: **DER WIDERSPRUCH, DER ABGELEHNT WIRD** — und er
    /// ist die einzige Stelle, an der RFC-Entwurf und Firefox uebereinstimmen:
    /// `SameSite=None` **ohne** `Secure` waere eine Absichtserklaerung, die
    ///浏览器 nicht erzwingen kann.
    #[test]
    fn samesite_none_ohne_secure_wird_abgelehnt() {
        let mut g = Keksglas::neu();
        let m = g
            .aus_set_cookie("x=1; SameSite=None", "s.test", "/", JETZT, true)
            .expect_err("SameSite=None ohne Secure muss abgelehnt werden");
        assert!(m.grund.contains("SameSite=None"), "{}", m.grund);
        assert_eq!(g.anzahl(), 0, "und der Keks wurde nicht gespeichert");
        // MIT Secure geht es.
        g.aus_set_cookie("x=1; SameSite=None; Secure", "s.test", "/", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(g.anzahl(), 1);
    }

    /// added 04.10.2026, TLS: **DIE DRITTEI-SPERRE**, und sie ist **ein
    /// Argument**, keine Vorgabe: die Voreinstellung ist eine **Entscheidung**
    /// des Benutzers.
    #[test]
    fn drittanbieter_sperre_ist_ein_argument() {
        let mut g = Keksglas::neu();
        // added 04.10.2026, TLS: **`SameSite=None; Secure`**, und der Grund ist
        // ein **Fehler in meinem Test**: mit der Vorgabe `Lax` blockiert
        // `cookie_darf_gesendet` den Keks auch ohne die Drittanbieter-Sperre —
        // **richtig**, denn `Lax` heisst nun einmal „nicht bei fremden
        // Unterressourcen“. Der Test hat damit **zwei** Regeln gemessen und
        // der einen die Schuld an der anderen gegeben. Mit `None` ist **nur**
        // die Drittanbieter-Regel uebrig.
        g.aus_set_cookie("t=1; SameSite=None; Secure", "cdn.test", "/", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert!(
            g.cookie_kopf(true, "cdn.test", "/", JETZT, "seite.test", true)
                .is_none(),
            "gesperrt"
        );
        assert!(
            g.cookie_kopf(true, "cdn.test", "/", JETZT, "seite.test", false)
                .is_some(),
            "ohne die Sperre geht er"
        );
        // UND AUF DER EIGENEN SEITE IST ER KEIN DRITTANBIETER.
        assert!(
            g.cookie_kopf(true, "cdn.test", "/", JETZT, "cdn.test", true)
                .is_some()
        );
    }

    /// added 04.10.2026, TLS: **DIE DOMAIN** und ihr **Unterschied** zu
    /// host-only: `Domain=seite.test` gilt auch fuer `www.seite.test`, und
    /// **ohne** `Domain` nur fuer genau `seite.test`.
    #[test]
    fn domain_gilt_fuer_subdomains_und_ohne_domain_nur_fuer_den_host() {
        let mut g = Keksglas::neu();
        g.aus_set_cookie("a=1; Domain=seite.test", "seite.test", "/", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        g.aus_set_cookie("b=2", "eng.test", "/", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            g.cookie_kopf(true, "www.seite.test", "/", JETZT, "www.seite.test", false)
                .as_deref(),
            Some("a=1"),
            "Domain schliesst die Subdomain ein"
        );
        assert_eq!(
            g.cookie_kopf(true, "eng.test", "/", JETZT, "eng.test", false)
                .as_deref(),
            Some("b=2"),
            "host-only gilt genau fuer diesen Host"
        );
        assert_eq!(
            g.cookie_kopf(true, "www.eng.test", "/", JETZT, "www.eng.test", false),
            None,
            "host-only gilt **nicht** fuer die Subdomain"
        );
    }

    /// added 04.10.2026, TLS: **DIE REIHENFOLGE**, und sie ist der Grund, warum
    /// es ueberhaupt eine Regel ist: der **laengste Pfad zuerst**.
    #[test]
    fn der_laengste_pfad_kommt_zuerst() {
        let mut g = Keksglas::neu();
        g.aus_set_cookie("kurz=1; Path=/", "s.test", "/", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        g.aus_set_cookie("lang=1; Path=/a/b", "s.test", "/", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            g.cookie_kopf(true, "s.test", "/a/b", JETZT, "s.test", false)
                .as_deref(),
            Some("lang=1; kurz=1"),
            "laengster Pfad zuerst (Entwurf 5.4 Schritt 2)"
        );
    }

    /// added 04.10.2026, TLS: **DAS ERSETZEN** — und der Grund, warum der Name
    /// der Schluessel ist: ein zweites `Set-Cookie` **ersetzt** das erste.
    #[test]
    fn ein_zweites_set_cookie_ersetzt_das_erste() {
        let mut g = Keksglas::neu();
        g.aus_set_cookie("sid=alt; Path=/", "s.test", "/", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        g.aus_set_cookie("sid=neu; Path=/", "s.test", "/", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(g.anzahl(), 1, "ersetzt, nicht verdoppelt");
        assert_eq!(
            g.cookie_kopf(true, "s.test", "/", JETZT, "s.test", false)
                .as_deref(),
            Some("sid=neu")
        );
    }

    /// added 04.10.2026, TLS: **DIE PERSISTENZ**, und der Test geht **hin und
    /// zurueck** — ein Roundtrip beweist, dass kein Feld verloren geht.
    #[test]
    fn der_speicher_uebersteht_einen_roundtrip() {
        let mut g = Keksglas::neu();
        g.aus_set_cookie(
            "a=1; Domain=s.test; Path=/x; Secure; HttpOnly; SameSite=Strict; \
             Expires=Sun, 06 Nov 2033 08:49:37 GMT",
            "s.test",
            "/",
            JETZT,
            true,
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        let text = g.als_text();
        let g2 = Keksglas::aus_text(&text);
        assert_eq!(g2.anzahl(), 1);
        let k = &g2.kekse[0];
        assert_eq!(k.name, "a");
        assert_eq!(k.value, "1");
        assert_eq!(k.domain, "s.test");
        assert_eq!(k.pfad, "/x");
        assert!(k.secure);
        assert!(k.http_only);
        assert_eq!(k.same_site, SameSiteWert::Strikt);
        // added 04.10.2026, TLS: **NACHGERECHNET** (`datetime`): 2033-11-06
        // 08:49:37 UTC ist **2014879777**, nicht 2043691777 — mein Erwartungswert
        // lag um fast **29 Jahre** daneben. **Wieder war der Test falsch, nicht
        // der Code**: `Expires` war die ganze Zeit richtig geparst.
        assert_eq!(k.expires, Some(2_014_879_777));
    }

    /// added 04.10.2026, TLS: **DAS `=` IM WERT** — `base64` hat es
    /// haeufig, und wer am **letzten** `=` splittet, zerlegt `a=b=c` in
    /// `a=b` und `c`.
    #[test]
    fn ein_gleichzeichen_im_wert_bleibt_im_wert() {
        let mut g = Keksglas::neu();
        g.aus_set_cookie("tok=YWJj=ZGVm", "s.test", "/", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            g.cookie_kopf(true, "s.test", "/", JETZT, "s.test", false)
                .as_deref(),
            Some("tok=YWJj=ZGVm")
        );
    }

    /// added 04.10.2026, TLS: **DER ABLAUF WIRKT**, und **beide** Wege:
    /// `Expires` in der Vergangenheit und `Max-Age=-1` (der Loesch-Befehl).
    #[test]
    fn abgelaufene_kekse_gehen_nicht_und_verschwinden() {
        let mut g = Keksglas::neu();
        assert!(
            g.aus_set_cookie(
                "alt=1; Expires=Sun, 06 Nov 1994 08:49:37 GMT",
                "s.test",
                "/",
                JETZT,
                true
            )
            .is_ok(),
            "ein abgelaufener Cookie wird als Loeschung akzeptiert"
        );
        // added 04.10.2026, TLS: **`Max-Age=10` statt eines absoluten Datums**,
        // und der Grund ist mein Testfehler: `Expires=01 Jan 1970` ist **schon
        // bei `JETZT` (2023) abgelaufen** und wurde deshalb **schon beim Setzen**
        // abgelehnt (`grund: "sofort abgelaufen"`). **Ein Test, der eine Regel
        // pruefen will, darf nicht an einer anderen scheitern** — `Max-Age` ist
        // relativ und damit genau dafuer der Werkzeugkasten.
        // Und ein bestehender Keks wird **nicht** mehr gesendet, wenn die Zeit
        // weiterlaeuft.
        g.aus_set_cookie("kurz=1; Max-Age=10", "s.test", "/", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert!(
            g.cookie_kopf(true, "s.test", "/", JETZT, "s.test", false)
                .is_some()
        );
        assert_eq!(
            g.cookie_kopf(true, "s.test", "/", JETZT + 20, "s.test", false),
            None,
            "nach dem Ablauf nicht mehr"
        );
    }

    // =======================================================================
    // added 04.10.2026, TLS: **DIE HELFER, und jeder Test hier ist ein
    // POSITIV- UND/ODER NEGATIVFALL** — ein Test, der nur „geht“ sagt,
    // beweist nicht, dass die Regel etwas verbietet.
    // =======================================================================

    #[test]
    fn http_only_ist_fuer_das_netz_da_und_fuer_skript_nicht() {
        let mut g = Keksglas::neu();
        g.aus_set_cookie("sichtbar=1; Path=/", "seite.test", "/", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        g.aus_set_cookie("geheim=2; Path=/; HttpOnly", "seite.test", "/", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        // **DAS NETZ**: beide. `HttpOnly` heisst nur „nicht fuer Skript“.
        let netz = g
            .cookie_kopf(true, "seite.test", "/", JETZT, "seite.test", false)
            .unwrap_or_default();
        assert!(netz.contains("sichtbar=1"), "{netz}");
        assert!(netz.contains("geheim=2"), "{netz}");
        // **DAS SKRIPT**: nur einer. draft-ietf-httpbis-rfc6265bis 5.6.
        let skript = g
            .cookie_kopf_zurueckgabe(true, "seite.test", "/", JETZT)
            .unwrap_or_default();
        assert_eq!(skript, "sichtbar=1");
        assert!(!skript.contains("geheim"), "{skript}");
    }

    #[test]
    fn secure_ist_ueber_http_weder_im_netz_noch_im_skript() {
        let mut g = Keksglas::neu();
        g.aus_set_cookie("s=1; Path=/; Secure", "seite.test", "/", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            g.cookie_kopf(false, "seite.test", "/", JETZT, "seite.test", false),
            None
        );
        assert_eq!(
            g.cookie_kopf_zurueckgabe(false, "seite.test", "/", JETZT),
            None
        );
        // **UND UEBER HTTPS IST ES DA** — sonst waere der Negativfall oben
        // auch bei einem kaputten `Secure`-Flag gruen.
        assert_eq!(
            g.cookie_kopf_zurueckgabe(true, "seite.test", "/", JETZT),
            Some("s=1".to_string())
        );
    }

    #[test]
    fn das_skript_ignoriert_samesite_und_dafuer_nicht_die_fremdregel() {
        let mut g = Keksglas::neu();
        // `SameSite=Strict`: im Netz **nicht** bei fremder Herkunft, im
        // Skript **immer** — `document.cookie` liest im gleichen Kontext.
        g.aus_set_cookie(
            "st=1; Path=/; SameSite=Strict",
            "seite.test",
            "/",
            JETZT,
            true,
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            g.cookie_kopf(true, "seite.test", "/", JETZT, "fremde.test", false),
            None,
            "fremde Herkunft, Strict"
        );
        assert_eq!(
            g.cookie_kopf(true, "seite.test", "/", JETZT, "seite.test", false),
            Some("st=1".to_string()),
            "eigene Seite, Strict"
        );
        assert_eq!(
            g.cookie_kopf_zurueckgabe(true, "seite.test", "/", JETZT),
            Some("st=1".to_string()),
            "Skript: SameSite gilt nicht"
        );
    }

    #[test]
    fn leeren_host_oder_pfad_gibt_leer_zurueck_und_keinen_fehler() {
        let mut g = Keksglas::neu();
        g.aus_set_cookie("a=1; Path=/", "seite.test", "/", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(g.cookie_kopf_zurueckgabe(true, "", "/", JETZT), None);
        // added 04.10.2026, TLS: **UND `/anders` PASST — und der erste Entwurf
        // behauptete das Gegenteil (GEMESSEN, erst rot mit
        // `left: Some("a=1")`).** Ein Keks mit `Path=/` gilt fuer **jeden**
        // Pfad **unterhalb** von `/`; das ist RFC 6265 5.1.4 woertlich („the
        // cookie-path is a prefix of the request-path“ — und `/` ist ein
        // Praefix von `/anders`). **WER HIER `None` ERWARTET, HAT DEN
        // PFADVERGLEICH UMGEKEHRT**, und dann bekommen **alle** Kekse **kein**
        // Keks — der Benutzer ist nirgends eingeloggt und **sieht** keinen
        // Fehler. **DAS IST DIE TEUERSTE FEHLERFORM IN DER GANZEN DATEI**, und
        // sie ist mir **zweimal** passiert: als Erwartung hier und als Regel in
        // `alle_fuer_skript` ( dort lag derselbe Vergleich mit `&&` statt `||`).
        assert_eq!(
            g.cookie_kopf_zurueckgabe(true, "seite.test", "/anders", JETZT),
            Some("a=1".to_string()),
            "`Path=/` gilt fuer jeden Pfad"
        );
        // **UND EIN FALSCHER PFAD PASST WIRKLICH NICHT** — der Gegenfall, sonst
        // waere oben ein Test fuer „`/anders` geht auch“.
        g.aus_set_cookie("b=2; Path=/bereich", "seite.test", "/bereich", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            g.cookie_kopf_zurueckgabe(true, "seite.test", "/anders", JETZT),
            Some("a=1".to_string()),
            "`/bereich` passt nicht auf `/anders`"
        );
    }

    #[test]
    fn skript_und_netz_behalten_die_reihenfolge_laengster_pfad_zuerst() {
        let mut g = Keksglas::neu();
        g.aus_set_cookie("a=breit; Path=/", "seite.test", "/a/b", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        g.aus_set_cookie("a=eng; Path=/a", "seite.test", "/a/b", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            g.cookie_kopf_zurueckgabe(true, "seite.test", "/a/b", JETZT),
            Some("a=eng; a=breit".to_string())
        );
    }

    #[test]
    fn seite_loeschen_nimmt_die_subdomain_mit_und_lasst_andere_stehen() {
        let mut g = Keksglas::neu();
        for (name, domain) in [
            ("a", "seite.test"),
            ("b", "www.seite.test"),
            ("c", "tief.www.seite.test"),
            ("d", "andere.test"),
        ] {
            g.aus_set_cookie(&format!("{name}=1; Path=/"), domain, "/", JETZT, true)
                .unwrap_or_else(|e| panic!("{e:?}"));
        }
        assert_eq!(g.anzahl(), 4);
        g.entferne_domain("seite.test");
        assert_eq!(g.anzahl(), 1, "nur `andere.test` darf bleiben");
        assert!(
            g.cookie_kopf(true, "andere.test", "/", JETZT, "andere.test", false)
                .is_some()
        );
        // **UND WIEDERHOLT GEHT AUCH** — ein zweiter Klick auf „loeschen“
        // darf nicht abstuerzen und nicht alles mitnehmen.
        g.entferne_domain("seite.test");
        assert_eq!(g.anzahl(), 1);
        // **UND MIT FUHRENDEM PUNKT** (`www.seite.test` eintippen).
        g.entferne_domain(".andere.test");
        assert_eq!(g.anzahl(), 0, "mit fuehrendem Punkt genauso");
    }

    #[test]
    fn ein_leerer_domainname_loescht_nichts() {
        let mut g = Keksglas::neu();
        g.aus_set_cookie("a=1; Path=/", "seite.test", "/", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        g.entferne_domain("");
        g.entferne_domain(".");
        assert_eq!(g.anzahl(), 1, "leer loescht nicht alles");
    }

    #[test]
    fn sitzungskeks_ist_dann_und_nur_dann_da_wenn_kein_ablauf_dasteht() {
        let mut g = Keksglas::neu();
        assert!(!g.hat_sitzungskeks(), "neu ist leer");
        g.aus_set_cookie(
            "fest=1; Path=/; Max-Age=600",
            "seite.test",
            "/",
            JETZT,
            true,
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        assert!(!g.hat_sitzungskeks(), "mit Ablauf ist es kein Sitzungskeks");
        g.aus_set_cookie("flüchtig=1; Path=/", "seite.test", "/", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert!(g.hat_sitzungskeks());
    }

    #[test]
    fn nach_einem_neustart_bleibt_der_keks_mit_ablauf_und_der_ohne_nicht() {
        // added 04.10.2026, TLS: **DER AUFTRAG, als Test**: „Neustart:
        // persistente Kekse bleiben, Sitzungskekse nicht.“ Das `als_text`/
        // `aus_text` **ist** der Neustart — es ist genau das, was beim
        // Beenden auf die Platte geschrieben und beim Starten gelesen wird.
        let mut g = Keksglas::neu();
        g.aus_set_cookie(
            "fest=1; Path=/; Max-Age=600",
            "seite.test",
            "/",
            JETZT,
            true,
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        g.aus_set_cookie("flüchtig=1; Path=/", "seite.test", "/", JETZT, true)
            .unwrap_or_else(|e| panic!("{e:?}"));
        let neu = Keksglas::aus_text(&g.als_text());
        assert_eq!(neu.anzahl(), 1, "nur der mit Ablauf");
        assert_eq!(
            neu.cookie_kopf(true, "seite.test", "/", JETZT, "seite.test", false),
            Some("fest=1".to_string())
        );
        assert!(!neu.hat_sitzungskeks());
    }

    #[test]
    fn der_neustart_haelt_alle_merkmale_und_nicht_nur_den_wert() {
        // added 04.10.2026, TLS: **WARUM DIESER TEST EIGENTLICH DA IST.** Der
        // erste Entwurf speicherte **nur** `name=value` — und nach dem Neustart
        // war ein `Secure`-Keks ueber `http` sichtbar und ein `HttpOnly`-Keks
        // fuer `document.cookie`. Das ist **der** Fehler, den nur dieser Test
        // findet: `als_text` sieht beide **richtig** aus.
        let mut g = Keksglas::neu();
        // added 04.10.2026, TLS: **`Max-Age=600`, und der Grund ist der
        // Fehler, den dieser Test **vorher** hatte (GEMESSEN, erst rot mit
        // „ein Keks“):** er setzte ein Keks **ohne** Ablaufdatum — also einen
        // **Sitzungskeks** — und erwartete danach, dass es den Neustart
        // ueberlebt. **Das widerspricht dem Fix von heute** (ein Sitzungskeks
        // wird **nicht** gespeichert), und ein Test, der sich selbst widerspricht,
        // ist nicht laenger als sein Fehler. **EIN TEST, DER DEN NEUSTART
        // PRUEFT, MUSS EIN KEKS MIT ABLAUF NEHMEN** — sonst prueft er nichts.
        g.aus_set_cookie(
            "s=1; Path=/a; Secure; HttpOnly; SameSite=Strict; Max-Age=600",
            "seite.test",
            "/a",
            JETZT,
            true,
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        let neu = Keksglas::aus_text(&g.als_text());
        let k = neu.kekse.first().expect("ein Keks");
        assert!(k.secure, "Secure");
        assert!(k.http_only, "HttpOnly");
        assert_eq!(k.pfad, "/a", "Pfad");
        assert_eq!(k.same_site, SameSiteWert::Strikt, "SameSite");
        assert!(k.host_only, "Host-only bleibt Host-only");
        // **UND DIE WIRKUNG, nicht nur die Felder:**
        assert_eq!(
            neu.cookie_kopf(true, "seite.test", "/a", JETZT, "seite.test", false),
            Some("s=1".to_string())
        );
        assert_eq!(
            neu.cookie_kopf_zurueckgabe(true, "seite.test", "/a", JETZT),
            None,
            "HttpOnly"
        );
        assert_eq!(
            neu.cookie_kopf(false, "seite.test", "/a", JETZT, "seite.test", false),
            None,
            "Secure ueber http"
        );
    }
}
