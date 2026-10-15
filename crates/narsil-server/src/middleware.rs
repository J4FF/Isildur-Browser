//! Middleware-Kette: Logging, Sicherheits-Header, CORS.
//!
//! Die Kette liegt ZWISCHEN Socket und Router: was hier steht, sieht
//! jede Antwort — auch `404` und `500`, denn ein Sicherheits-Header,
//! der nur auf Erfolg gilt, schuetzt nur die Seiten, die keine Hilfe
//! brauchen.
//!
//! Aufbau (rekursiv, Tiefe = Anzahl der Glieder, also durch
//! [`crate::MAX_MIDDLEWARE`] gedeckelt — Regel M5):
//!
//! ```text
//! glied[0](anfrage, weiter) -> glied[1](anfrage, weiter) -> ... -> handler
//! ```
//!
//! `weiter` ist KEIN Trait-Objekt mit dynamischem Dispatch zur Laufzeit
//! auf unbegrenzter Ebene, sondern ein Closure-Stack, den die Kette
//! selbst aufbaut. Ein Glied, das `weiter` nie aufruft, antwortet
//! selbst (das ist genau die Vorbedingung bei CORS-Preflight).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::http::{Request, Response};
use crate::router::Handler;

/// Ein Middleware-Glied: Anfrage rein, `weiter` aufrufen, Antwort raus.
pub type Glied = Arc<dyn Fn(&Request, &dyn Fn(&Request) -> Response) -> Response + Send + Sync>;

/// Die Kette eines Servers: Glieder in Reihenfolge, danach der Router.
#[derive(Clone)]
pub struct Kette {
    glieder: Vec<Glied>,
    handler: Handler,
}

impl Kette {
    /// Neue Kette mit dem Router als letztem Glied.
    pub fn neu(handler: Handler) -> Self {
        Kette {
            glieder: Vec::new(),
            handler,
        }
    }

    /// Glied hinten anhaengen. Gibt `false` zurueck, wenn
    /// [`crate::MAX_MIDDLEWARE`] erreicht ist — kein Panic, kein
    /// stilles Weiterwachsen (Regel M6: nichts ohne Obergrenze).
    ///
    /// Das Glied kommt als [`Glied`] (also als `Arc<dyn Fn ...>`) —
    /// genau das, was [`cors_glied`], [`sicherheits_header`] und
    /// [`logging`] bauen. Wer eine rohe Closure hat, wickelt sie mit
    /// `Arc::new(f)` ein; ein zusatzlicher generischer Weg daneben
    /// wuerde zwei Stellen bedeuten, an denen dieselbe Grenze gilt.
    pub fn hinzufuegen(&mut self, g: Glied) -> bool {
        if self.glieder.len() >= crate::MAX_MIDDLEWARE {
            return false;
        }
        self.glieder.push(g);
        true
    }

    /// Wie viele Glieder (fuer Tests und Diagnose).
    pub fn laenge(&self) -> usize {
        self.glieder.len()
    }

    /// Die Kette abarbeiten bis zum Router.
    pub fn ausfuehren(&self, anfrage: &Request) -> Response {
        self.stufe(0, anfrage)
    }

    /// Rekursion mit fester Tiefe (M5): `i` zaehlt bis `laenge()`.
    fn stufe(&self, i: usize, anfrage: &Request) -> Response {
        if i >= self.glieder.len() {
            return (self.handler)(anfrage);
        }
        let weiter = |a: &Request| self.stufe(i + 1, a);
        (self.glieder[i])(anfrage, &weiter)
    }
}

// ---------------------------------------------------------------------------
// CORS
// ---------------------------------------------------------------------------

/// Cross-Origin-Einstellungen. Konfigurierbar, ABER:
/// `credentials: true` zusammen mit `origin: *` ist der Klassiker, mit
/// dem Server Cookies an jeden dahergelaufenen weitergeben — dieses
/// Paar wird deshalb abgelehnt (das Glied bleibt dann ohne Wirkung,
/// statt es heimlich zu erlauben).
#[derive(Debug, Clone)]
pub struct Cors {
    /// Erlaubte Origin (`https://beispiel.de`). `*` heisst: alle.
    /// Leer heisst: kein CORS-Header, Anfragen anderer Origin kommen
    /// nicht durch (Same-Origin-Schutz des Browsers greift dann).
    pub origin: Vec<String>,
    /// Erlaubte Methoden fuer Preflight.
    pub methoden: Vec<String>,
    /// Erlaubte Header fuer Preflight.
    pub header: Vec<String>,
    /// Ob Cookies/Zertifikate mitgeschickt werden duerfen.
    pub credentials: bool,
    /// Wie lange ein Preflight gecacht werden darf.
    pub max_alt: Duration,
}

impl Cors {
    /// Alles erlauben (fuer lokale Entwicklung, ohne Zugangsdaten).
    pub fn offen() -> Self {
        Cors {
            origin: vec!["*".to_string()],
            methoden: vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "DELETE".to_string(),
                "OPTIONS".to_string(),
            ],
            header: vec!["content-type".to_string()],
            credentials: false,
            max_alt: Duration::from_secs(600),
        }
    }

    /// Nur diese Origin, mit Zugangsdaten (der sichere Fall).
    pub fn fuer(origin: &str) -> Self {
        let mut c = Cors::offen();
        c.origin = vec![origin.to_string()];
        c.credentials = true;
        c
    }

    /// Ob `origin` erlaubt ist (kleingeschrieben verglichen).
    fn erlaubt(&self, origin: &str) -> bool {
        self.origin
            .iter()
            .any(|e| e == "*" || e.eq_ignore_ascii_case(origin))
    }

    /// Der gemeinsame Teil beider Antworten.
    fn kopf(&self, anfrage: &Request) -> Vec<(String, String)> {
        let Some(origin) = anfrage.header("origin") else {
            return Vec::new();
        };
        if !self.erlaubt(origin) {
            return Vec::new();
        }
        // `*` + credentials geht nicht — dann wird die konkrete Origin
        // zurueckgeeschickt, statt die Regel still aufzuweichen.
        let erlaubte_origin = if self.credentials && self.origin.iter().any(|e| e == "*") {
            origin.to_string()
        } else if self.origin.iter().any(|e| e == "*") {
            "*".to_string()
        } else {
            origin.to_string()
        };
        let mut h = vec![("Access-Control-Allow-Origin".to_string(), erlaubte_origin)];
        if self.credentials {
            h.push((
                "Access-Control-Allow-Credentials".to_string(),
                "true".to_string(),
            ));
        }
        h
    }
}

/// CORS-Glied: beantwortet Preflight SELBST (ohne den Router) und
/// haengt die Header an alle anderen Antworten an.
///
/// added 01.10.2026, SERVER V7: `Vary: Origin` steht jetzt auf **jeder**
/// Antwort dieses Glieds, nicht nur auf dem Preflight.
///
/// DER BEFUND: die einfache Antwort eines CORS-Gliedes haengt
/// `Access-Control-Allow-Origin: https://a.de` an — und nicht an die
/// Antwort, bei der `Origin: https://b.de` (fremd) kommt. Ohne `Vary`
/// darf ein gemeinsamer Cache beide unter demselben Schluessel halten.
/// Das ist kein Theoriestueck: `Vary` fehlte, und genau das ist der
/// Fall, in dem ein fremder Client die Antwort eines erlaubten Origins
/// aus einem Cache liest.
pub fn cors_glied(cors: Cors) -> Glied {
    Arc::new(move |anfrage, weiter| {
        let kopf = cors.kopf(anfrage);
        let ist_preflight = anfrage.methode == crate::http::Method::Options
            && anfrage.header("access-control-request-method").is_some();
        if ist_preflight {
            let mut antwort = Response::status(204);
            for (k, v) in kopf {
                antwort.headers.push((k, v));
            }
            antwort.headers.push((
                "Access-Control-Allow-Methods".to_string(),
                cors.methoden.join(", "),
            ));
            antwort.headers.push((
                "Access-Control-Allow-Headers".to_string(),
                cors.header.join(", "),
            ));
            antwort.headers.push((
                "Access-Control-Max-Age".to_string(),
                cors.max_alt.as_secs().to_string(),
            ));
            antwort.headers.push((
                "Vary".to_string(),
                "Origin, Access-Control-Request-Method, Access-Control-Request-Headers".to_string(),
            ));
            return antwort;
        }
        let mut antwort = weiter(anfrage);
        // `Vary` auf jeder Antwort, auch ohne `Origin`: die Antwort
        // unterscheidet sich danach, ob einer da war. Wer die
        // vorhandenen `Vary`-Werte nicht ueberschreibt, haengt an.
        vary_anhaengen(&mut antwort, "Origin");
        for (k, v) in kopf {
            antwort.headers.push((k, v));
        }
        antwort
    })
}

/// Haengt `wert` an ein vorhandenes `Vary` an (RFC 9110 5.3) oder
/// legt eines an.
///
/// DER GRUND FUER DIE EIGENE STELLE: `Vary: Accept-Encoding` (aus
/// [`crate::kompression`]) und `Vary: Origin` (aus CORS) koennen an
/// derselben Antwort stehen. Wer sie einfach zweimal schreibt, hat
/// zwei `Vary`-Kopfzeilen — erlaubt ist das laut Spezifikation nicht
/// (es ist eine Liste, keine Wiederholung), und `narsil-net` liest nur
/// die erste.
pub fn vary_anhaengen(antwort: &mut Response, wert: &str) {
    let vorhanden = antwort.header("vary").map(str::to_string);
    match vorhanden {
        None => antwort.headers.push(("Vary".into(), wert.into())),
        Some(list) => {
            if list.split(',').any(|t| t.trim().eq_ignore_ascii_case(wert)) {
                return; // steht schon drin
            }
            antwort
                .headers
                .retain(|(k, _)| !k.eq_ignore_ascii_case("vary"));
            antwort
                .headers
                .push(("Vary".into(), format!("{list}, {wert}")));
        }
    }
}

// ---------------------------------------------------------------------------
// Sicherheits-Header
// ---------------------------------------------------------------------------

/// Sicherheits-Header an JEDE Antwort.
///
/// * `X-Content-Type-Options: nosniff` — der Browser darf den Typ nicht
///   umdeuten, sonst wird aus `text/plain` ein Skript.
/// * `Referrer-Policy` — wie viel von der eigenen URL mitgeht.
/// * `X-Frame-Options: DENY` — keine Einbettung in fremde Frames.
/// * `Content-Security-Policy` — wenn konfiguriert (Option).
/// * `Strict-Transport-Security` — NUR, wenn die Anfrage ueber HTTPS
///   kam (`X-Forwarded-Proto: https`). Der Server selbst spricht in
///   Welle 4 kein TLS, also waere ein HSTS-Header auf HTTP eine Luege;
///   vorbereitet ist er trotzdem (Welle 5 dreht nur den Schalter um).
pub fn sicherheits_header(csp: Option<String>, hsts: bool) -> Glied {
    Arc::new(move |anfrage, weiter| {
        let mut antwort = weiter(anfrage);
        antwort
            .headers
            .push(("X-Content-Type-Options".into(), "nosniff".into()));
        antwort.headers.push((
            "Referrer-Policy".into(),
            "strict-origin-when-cross-origin".into(),
        ));
        antwort
            .headers
            .push(("X-Frame-Options".into(), "DENY".into()));
        if let Some(csp) = &csp {
            if !csp.is_empty() {
                antwort
                    .headers
                    .push(("Content-Security-Policy".into(), csp.clone()));
            }
        }
        let ist_https = anfrage.header("x-forwarded-proto") == Some("https");
        if hsts && ist_https {
            antwort.headers.push((
                "Strict-Transport-Security".into(),
                "max-age=31536000; includeSubDomains".into(),
            ));
        }
        antwort
    })
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

/// Zaehler fuer Logs und Tests (`anfragen_gesamt`).
#[derive(Debug, Default)]
pub struct Zaehler {
    /// Wie Antworten geschrieben wurden.
    pub antworten: AtomicU64,
    /// Wie viele davon Fehler waren (4xx/5xx).
    pub fehler: AtomicU64,
}

impl Zaehler {
    pub fn neu() -> Self {
        Zaehler::default()
    }
    pub fn antworten(&self) -> u64 {
        self.antworten.load(Ordering::SeqCst)
    }
    pub fn fehler(&self) -> u64 {
        self.fehler.load(Ordering::SeqCst)
    }
}

/// Logging-Glied: zaehlt jede Antwort und schreibt auf Wunsch eine
/// Zeile nach stderr. Schreiben ist abstellbar (`an = false`), zaehlen
/// nicht — ein Server ohne Zaehler behauptet Durchsatz, statt ihn zu
/// messen (Regel: Zahlen werden gemessen).
pub fn logging(zaehler: Arc<Zaehler>, an: bool) -> Glied {
    Arc::new(move |anfrage, weiter| {
        let antwort = weiter(anfrage);
        zaehler.antworten.fetch_add(1, Ordering::SeqCst);
        if antwort.status.0 >= 400 {
            zaehler.fehler.fetch_add(1, Ordering::SeqCst);
        }
        if an {
            // added 02.10.2026, SERVER V14: DIE LAENGE IST `body.len()`
            // — und fuer eine statische Datei ist das **null**.
            //
            // GEMESSEN beim ersten Lauf von `examples/serve_https.rs`:
            //   GET /index.html -> 200 (0 Bytes)
            // bei 1.297 Byte im Koerper. Der Betreiber sieht eine
            // Zeile, die „nichts ausgeliefert" sagt, und der Server
            // hat 1.297 Byte ausgeliefert. Das ist die schlimmste
            // Sorte Log: nicht falsch im Vergleich zu den anderen
            // Zeilen, sondern falsch im Vergleich zur **Wirklichkeit**.
            //
            // DIE RICHTIGE ZAHL: bei einer Datei ist
            // `DateiLesen::laenge` die ausgelieferte Laenge (bei
            // `Range` die Teilstuecke), und das ist genau die Zahl,
            // die `protokoll::eintrag_aus` seit V1 benutzt. Also
            // dieselbe Quelle — ein Betreiber vergleicht Protokoll
            // und stderr und bekommt dieselbe Zahl.
            let laenge: u64 = match &antwort.datei {
                Some(d) => d.laenge,
                None => antwort.body.len() as u64,
            };
            eprintln!(
                "{} {} -> {} ({} Bytes)",
                anfrage.methode.als_str(),
                anfrage.pfad,
                antwort.status.0,
                laenge
            );
        }
        antwort
    })
}
