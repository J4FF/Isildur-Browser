//! Typsicherer Router: Methode + Pfad -> Handler.
//!
//! Typsicher heisst hier dreierlei, und alles drei steht im Typ, nicht
//! in der Doku:
//!
//! 1. Die Methode ist [`crate::http::Method`], kein String. `GET` und
//!    `get` sind nicht dasselbe — das zweite ist `Unbekannt` und bekommt
//!    `501`, nicht die Route.
//! 2. Der Pfad ist ein Muster (`/hello/:name`, `/dateien/*`), kein
//!    String-Vergleich. `:name` faengt EIN Segment, `*` den Rest.
//! 3. Der Handler ist `fn(&Request) -> Response`, kein Trait-Objekt mit
//!    Downcast. Was reinkommt, ist eine Anfrage; was rausgeht, ist eine
//!    Antwort — dazwischen liegt kein `Any`.
//!
//! Der Router kennt kein Netz und kein Dateisystem. Er routet.

use std::collections::HashMap;
use std::sync::Arc;

use crate::http::{Method, Request, Response};

/// Handler-Funktion: Anfrage rein, Antwort raus.
///
/// `Send + Sync`, damit der Server sie aus Threads aufrufen kann.
/// Kein `async` (kein Tokio in v0) — wer warten will, wartet im Handler.
pub type Handler = Arc<dyn Fn(&Request) -> Response + Send + Sync>;

/// Eine Route: Methode + Muster + Handler.
#[derive(Clone)]
struct Route {
    methode: Method,
    teile: Vec<Segment>,
    handler: Handler,
    muster: String,
}

/// Ein Pfadsegment im Muster.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    /// Fester Text (`hello`).
    Fest(String),
    /// Parameter (`:id` faengt ein Segment bis `/`).
    Param(String),
    /// Rest (`*` faengt alles ab hier, inkl. `/`).
    Rest,
}

impl Segment {
    fn aus(s: &str) -> Option<Self> {
        if s == "*" {
            Some(Segment::Rest)
        } else if let Some(name) = s.strip_prefix(':') {
            if name.is_empty() || name.contains('/') {
                None
            } else {
                Some(Segment::Param(name.to_string()))
            }
        } else if s.is_empty() {
            None
        } else {
            Some(Segment::Fest(s.to_string()))
        }
    }
}

/// Der Router. Routen in Registrierreihenfolge — die erste passende
/// gewinnt. Wer Prioritaeten will, registriert in Prioritaetsreihenfolge.
#[derive(Clone, Default)]
pub struct Router {
    routen: Vec<Route>,
}

impl Router {
    /// Leerer Router.
    pub fn neu() -> Self {
        Router { routen: Vec::new() }
    }

    /// Wie viele Routen registriert sind.
    pub fn len(&self) -> usize {
        self.routen.len()
    }

    /// Ob keine Route da ist.
    pub fn is_empty(&self) -> bool {
        self.routen.is_empty()
    }

    /// Route eintragen. Gibt `false`, wenn das Muster ungueltig ist oder
    /// [`crate::MAX_ROUTEN`] erreicht ist — kein Panic, kein stilles
    /// Ueberschreiben. Doppelte Muster sind erlaubt (die erste gewinnt),
    /// weil das beim Zusammenbauen aus Modulen die Norm ist.
    pub fn eintragen<F>(&mut self, methode: Method, muster: &str, handler: F) -> bool
    where
        F: Fn(&Request) -> Response + Send + Sync + 'static,
    {
        if self.routen.len() >= crate::MAX_ROUTEN {
            return false;
        }
        let Some(teile) = zerlege_muster(muster) else {
            return false;
        };
        self.routen.push(Route {
            methode,
            teile,
            handler: Arc::new(handler),
            muster: muster.to_string(),
        });
        true
    }

    /// Bequemlichkeit: GET-Route.
    pub fn get<F>(&mut self, muster: &str, handler: F) -> bool
    where
        F: Fn(&Request) -> Response + Send + Sync + 'static,
    {
        self.eintragen(Method::Get, muster, handler)
    }

    /// Bequemlichkeit: POST-Route.
    pub fn post<F>(&mut self, muster: &str, handler: F) -> bool
    where
        F: Fn(&Request) -> Response + Send + Sync + 'static,
    {
        self.eintragen(Method::Post, muster, handler)
    }

    /// Bequemlichkeit: HEAD-Route.
    pub fn head<F>(&mut self, muster: &str, handler: F) -> bool
    where
        F: Fn(&Request) -> Response + Send + Sync + 'static,
    {
        self.eintragen(Method::Head, muster, handler)
    }

    /// Bequemlichkeit: PUT-Route.
    pub fn put<F>(&mut self, muster: &str, handler: F) -> bool
    where
        F: Fn(&Request) -> Response + Send + Sync + 'static,
    {
        self.eintragen(Method::Put, muster, handler)
    }

    /// Bequemlichkeit: DELETE-Route.
    pub fn delete<F>(&mut self, muster: &str, handler: F) -> bool
    where
        F: Fn(&Request) -> Response + Send + Sync + 'static,
    {
        self.eintragen(Method::Delete, muster, handler)
    }

    /// Eine Anfrage routen: erste passende Route gewinnt.
    ///
    /// * Pfad passt + Methode passt -> Handler laeuft, Params gesetzt.
    /// * Pfad passt + Methode passt nicht -> `405` (nicht `404` — der
    ///   Pfad existiert, die Methode ist falsch).
    /// * Nichts passt -> `404` mit Pfad im Body (damit man sieht, WAS
    ///   fehlt — ein leeres 404 ist schlimmer als ein lautes).
    /// * Methode `Unbekannt` -> `501`, egal ob der Pfad passt.
    pub fn route(&self, anfrage: &Request) -> Response {
        if anfrage.methode == Method::Unbekannt {
            return Response::fehler(
                501,
                format!("Methode {} nicht unterstuetzt", anfrage.methode_roh),
            );
        }
        let mut pfad_da_aber_methode_falsch = false;
        for route in &self.routen {
            if let Some(params) = passt(&route.teile, &anfrage.pfad) {
                if route.methode == anfrage.methode {
                    let mut mit_params = anfrage.clone();
                    mit_params.params = params;
                    return (route.handler)(&mit_params);
                }
                pfad_da_aber_methode_falsch = true;
            }
        }
        if pfad_da_aber_methode_falsch {
            Response::fehler(
                405,
                format!(
                    "{} {}: Methode nicht erlaubt",
                    anfrage.methode.als_str(),
                    anfrage.pfad
                ),
            )
        } else {
            Response::fehler(404, format!("{}: nicht gefunden", anfrage.pfad))
        }
    }

    /// Alle Muster (fuer `/api/routen`-Diagnose und Tests).
    pub fn muster(&self) -> Vec<String> {
        self.routen.iter().map(|r| r.muster.clone()).collect()
    }
}

/// Muster in Segmente. `/` -> leer (Root), sonst pro `/`-Teil eines.
/// `*` nur als letztes Segment — mitten im Muster waere es keine
/// Entscheidung, sondern ein Raetsel.
fn zerlege_muster(muster: &str) -> Option<Vec<Segment>> {
    if muster == "/" {
        return Some(Vec::new());
    }
    if !muster.starts_with('/') {
        return None;
    }
    let roh: Vec<&str> = muster.split('/').skip(1).collect();
    let mut teile = Vec::with_capacity(roh.len());
    for (i, s) in roh.iter().enumerate() {
        let seg = Segment::aus(s)?;
        if matches!(seg, Segment::Rest) && i + 1 != roh.len() {
            return None;
        }
        teile.push(seg);
    }
    Some(teile)
}

/// Pfad gegen Muster. Gibt Params zurueck (`:id` dekodiert, `*` roh).
fn passt(muster: &[Segment], pfad: &str) -> Option<HashMap<String, String>> {
    let ziel: Vec<&str> = if pfad == "/" {
        Vec::new()
    } else {
        let p = pfad.strip_prefix('/')?;
        p.split('/').collect()
    };
    let mut params = HashMap::new();
    let mut zi = 0;
    for seg in muster {
        match seg {
            Segment::Fest(f) => {
                if ziel.get(zi) != Some(&f.as_str()) {
                    return None;
                }
                zi += 1;
            }
            Segment::Param(name) => {
                let wert = *ziel.get(zi)?;
                if wert.is_empty() {
                    return None;
                }
                params.insert(name.clone(), prozent_dekodieren(wert));
                zi += 1;
            }
            Segment::Rest => {
                let rest = ziel[zi..].join("/");
                params.insert("*".to_string(), rest);
                zi = ziel.len();
                break;
            }
        }
    }
    if zi != ziel.len() {
        return None;
    }
    Some(params)
}

/// Prozentdekodierung fuer Pfadparameter (`%20` -> Leerzeichen).
///
/// Fehlertolerant: kaputte Sequenzen bleiben wie getippt (`%ZZ` bleibt
/// `%ZZ`), statt die ganze Anfrage zu verwerfen. `+` wird NICHT zu
/// Leerzeichen (das gilt nur in Query, nicht im Pfad — und hier steht
/// der Unterschied, damit er nicht geraten wird).
pub fn prozent_dekodieren(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut raus = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() + 1 && i + 2 <= bytes.len() {
            if let (Some(h), Some(l)) = (hex_wert(bytes.get(i + 1)), hex_wert(bytes.get(i + 2))) {
                raus.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        raus.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&raus).into_owned()
}

fn hex_wert(b: Option<&u8>) -> Option<u8> {
    match b.copied() {
        Some(v @ b'0'..=b'9') => Some(v - b'0'),
        Some(v @ b'a'..=b'f') => Some(v - b'a' + 10),
        Some(v @ b'A'..=b'F') => Some(v - b'A' + 10),
        _ => None,
    }
}

/// Baut einen Router aus Routen-Tupeln — die Entwicklerfreundlichkeit
/// aus Punkt 2 der Vision (klare Makros, typsichere Routen).
///
/// ```rust
/// use narsil_server::{Router, routes, http::{Method, Response}};
/// let r: Router = routes![
///     (Method::Get, "/a", |_q| Response::text(200, "a")),
///     (Method::Post, "/b/:id", |q| Response::text(200, "b")),
/// ];
/// assert_eq!(r.len(), 2);
/// ```
#[macro_export]
macro_rules! routes {
    [ $( ($methode:expr, $muster:expr, $handler:expr) ),* $(,)? ] => {{
        let mut __router = $crate::Router::neu();
        $(
            let __ok = __router.eintragen($methode, $muster, $handler);
            assert!(__ok, "Route konnte nicht eingetragen werden");
        )*
        __router
    }};
}

/// Methode fuer [`api_routen!`]. Was hier nicht steht, ist ein
/// COMPILE-FEHLER — ein `GETT` kann nicht zur Laufzeit als `501`
/// enden, wenn es stattdessen schon beim Uebersetzen auffaellt.
///
/// ```compile_fail
/// use narsil_server::{api_routen, http::Response};
/// // GETT statt GET: der Compiler muss das ablehnen.
/// let _router = api_routen![ GETT "/x" => |_req| Response::text(200, "x") ];
/// ```
#[macro_export]
macro_rules! methode_verlangen {
    (GET) => {
        $crate::http::Method::Get
    };
    (HEAD) => {
        $crate::http::Method::Head
    };
    (POST) => {
        $crate::http::Method::Post
    };
    (PUT) => {
        $crate::http::Method::Put
    };
    (DELETE) => {
        $crate::http::Method::Delete
    };
    (OPTIONS) => {
        $crate::http::Method::Options
    };
    ($falsch:tt) => {
        compile_error!(::std::concat!(
            "unbekannte Methode im Routen-Makro: ",
            ::std::stringify!($falsch),
            " — erlaubt: GET, HEAD, POST, PUT, DELETE, OPTIONS"
        ))
    };
}

/// Routen deklarativ beschreiben — mit Methoden, die der COMPILER
/// prueft (S2).
///
/// Reihenfolge = Registrierreihenfolge: die erste passende Route
/// gewinnt. Ein Muster, das der Router nicht versteht (kein fuehrendes
/// `/`, `*` nicht am Ende), ist ebenfalls ein Laufzeitfehler beim
/// Aufbau — bevor der Server eine einzige Anfrage sieht.
///
/// ```rust
/// use narsil_server::{api_routen, http::Response};
///
/// let router = api_routen![
///     GET "/api/notizen" => |_req| Response::json(200, "[]"),
///     GET "/api/notizen/:id" => |req| {
///         Response::json(200, format!("{{\"id\":{:?}}}", req.param("id")))
///     },
///     POST "/api/notizen" => |_req| Response::text(201, "erstellt"),
///     DELETE "/api/notizen/:id" => |_req| Response::text(204, ""),
/// ];
/// assert_eq!(router.len(), 4);
/// ```
#[macro_export]
macro_rules! api_routen {
    ( $( $methode:ident $muster:literal => $handler:expr ),+ $(,)? ) => {{
        let mut __router = $crate::Router::neu();
        $(
            let __ok = __router.eintragen(
                $crate::methode_verlangen!($methode),
                $muster,
                $handler,
            );
            ::std::assert!(__ok, "Route nicht eintragbar: {}", $muster);
        )+
        __router
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::anfrage_aus_text;

    #[test]
    fn param_wird_extrahiert_und_dekodiert() {
        let mut r = Router::neu();
        r.get("/hello/:name", |req| {
            Response::text(200, req.param("name").unwrap_or("?").to_string())
        });
        let req = anfrage_aus_text("GET", "/hello/Welt%202", &[], &[]);
        let antwort = r.route(&req);
        assert_eq!(antwort.status.0, 200);
        assert_eq!(antwort.body, b"Welt 2");
    }

    #[test]
    fn falsche_methode_ist_405_nicht_404() {
        let mut r = Router::neu();
        r.get("/a", |_| Response::text(200, "a"));
        let req = anfrage_aus_text("POST", "/a", &[], &[]);
        assert_eq!(r.route(&req).status.0, 405);
        let req = anfrage_aus_text("POST", "/fehlt", &[], &[]);
        assert_eq!(r.route(&req).status.0, 404);
    }

    #[test]
    fn stern_faengt_rest() {
        let mut r = Router::neu();
        r.get("/dateien/*", |req| {
            Response::text(200, req.param("*").unwrap_or("").to_string())
        });
        let req = anfrage_aus_text("GET", "/dateien/a/b/c", &[], &[]);
        assert_eq!(r.route(&req).body, b"a/b/c");
    }

    #[test]
    fn unbekannte_methode_ist_501() {
        let r = Router::neu();
        let req = anfrage_aus_text("PATCH", "/", &[], &[]);
        assert_eq!(r.route(&req).status.0, 501);
    }

    #[test]
    fn muster_ohne_slash_wird_abgelehnt() {
        let mut r = Router::neu();
        assert!(!r.eintragen(Method::Get, "ohne-slash", |_| Response::text(200, "x")));
        assert!(!r.eintragen(Method::Get, "/a/*/b", |_| Response::text(200, "x")));
    }

    #[test]
    fn prozent_kaputt_bleibt() {
        assert_eq!(prozent_dekodieren("%ZZ"), "%ZZ");
        assert_eq!(prozent_dekodieren("a+b"), "a+b");
    }
}
