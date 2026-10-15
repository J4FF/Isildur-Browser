//! Extraktoren: aus rohen Anfrage-Teilen werden TYPEN — mit `400`, wenn
//! die Zahl keine ist.
//!
//! Der Unterschied zu `req.param("id").unwrap_or("0")`: dort wird aus
//! `GET /notizen/abc` still `0`, und der Fehler taucht drei Schleifen
//! weiter auf. Hier bekommt `abc -> u64` sofort `400` mit der Stelle im
//! Pfad — der Client erfahrt, WAS falsch war, der Server speichert
//! keinen halbgueltigen Wert.
//!
//! ```rust
//! use narsil_server::{Router, http::Response};
//! use narsil_server::extraktor::pfad_param;
//!
//! let mut r = Router::neu();
//! r.get("/notizen/:id", |req| {
//!     match pfad_param::<u64>(req, "id") {
//!         Ok(id) => Response::text(200, format!("Notiz {id}")),
//!         Err(fehler) => fehler,
//!     }
//! });
//! ```

use crate::api::query_map;
use crate::http::{Request, Response};
use crate::json::{self, AusJson};

/// Was aus einem rohen String gemacht wird.
///
/// `aus_roh` darf nie paniken und nie raten: `u64::from_str` auf
/// `"abc"` ist ein FEHLER, kein `0`.
pub trait Extraktor: Sized {
    /// Aus dem rohen Wert (Pfadsegment, Query-Wert, Header-Wert).
    fn aus_roh(roh: &str) -> Result<Self, String>;
}

impl Extraktor for String {
    fn aus_roh(roh: &str) -> Result<Self, String> {
        Ok(roh.to_string())
    }
}

impl Extraktor for u64 {
    fn aus_roh(roh: &str) -> Result<Self, String> {
        roh.trim()
            .parse::<u64>()
            .map_err(|_| format!("{roh:?} ist keine u64"))
    }
}

impl Extraktor for u32 {
    fn aus_roh(roh: &str) -> Result<Self, String> {
        roh.trim()
            .parse::<u32>()
            .map_err(|_| format!("{roh:?} ist keine u32"))
    }
}

impl Extraktor for usize {
    fn aus_roh(roh: &str) -> Result<Self, String> {
        roh.trim()
            .parse::<usize>()
            .map_err(|_| format!("{roh:?} ist keine usize"))
    }
}

impl Extraktor for i64 {
    fn aus_roh(roh: &str) -> Result<Self, String> {
        roh.trim()
            .parse::<i64>()
            .map_err(|_| format!("{roh:?} ist keine i64"))
    }
}

impl Extraktor for i32 {
    fn aus_roh(roh: &str) -> Result<Self, String> {
        roh.trim()
            .parse::<i32>()
            .map_err(|_| format!("{roh:?} ist keine i32"))
    }
}

impl Extraktor for f64 {
    fn aus_roh(roh: &str) -> Result<Self, String> {
        let n: f64 = roh
            .trim()
            .parse()
            .map_err(|_| format!("{roh:?} ist keine Zahl"))?;
        if n.is_finite() {
            Ok(n)
        } else {
            // `inf`/`nan` sind gueltige f64-Literale, aber keine
            // Antwort, die ein Client verarbeiten kann.
            Err(format!("{roh:?} ist nicht endlich"))
        }
    }
}

impl Extraktor for bool {
    fn aus_roh(roh: &str) -> Result<Self, String> {
        match roh.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "ja" | "yes" | "an" | "on" => Ok(true),
            "0" | "false" | "nein" | "no" | "aus" | "off" => Ok(false),
            _ => Err(format!("{roh:?} ist kein bool")),
        }
    }
}

/// Pfadparameter typisiert (`:id` -> `u64`). Fehlt der Parameter oder
/// ist die Zahl falsch: `400`.
pub fn pfad_param<T: Extraktor>(req: &Request, name: &str) -> Result<T, Response> {
    match req.param(name) {
        None => Err(crate::api::json_fehler(
            400,
            &format!("Pfadparameter {name:?} fehlt"),
        )),
        Some(roh) => T::aus_roh(roh).map_err(|grund| crate::api::json_fehler(400, &grund)),
    }
}

/// Query-Parameter, der DA SEIN MUSS (`?seite=3`). Fehlt er: `400`.
///
/// Ein fehlender Query-Parameter ist ein Fehler, kein `0` — sonst
/// antwortet der Server auf `GET /suche` mit der ersten Seite und tut,
/// als haette jemand `seite=0` geschickt.
pub fn query_param<T: Extraktor>(req: &Request, name: &str) -> Result<T, Response> {
    match query_map(&req.query).get(name) {
        None => Err(crate::api::json_fehler(
            400,
            &format!("Query-Parameter {name:?} fehlt"),
        )),
        Some(roh) => T::aus_roh(roh).map_err(|grund| crate::api::json_fehler(400, &grund)),
    }
}

/// Query-Parameter, der auch fehlen darf.
pub fn query_param_opt<T: Extraktor>(req: &Request, name: &str) -> Result<Option<T>, Response> {
    match query_map(&req.query).get(name) {
        None => Ok(None),
        Some(roh) => match T::aus_roh(roh) {
            Ok(wert) => Ok(Some(wert)),
            Err(grund) => Err(crate::api::json_fehler(400, &grund)),
        },
    }
}

/// Header typisiert. Fehlt er: `400` (bei `Header`-Pflicht wie
/// `Content-Type` ist das der richtige Code, nicht `415` — der Server
/// hat die Anfrage verstanden, nur ihr Feld fehlt).
pub fn header_param<T: Extraktor>(req: &Request, name: &str) -> Result<T, Response> {
    match req.header(name) {
        None => Err(crate::api::json_fehler(
            400,
            &format!("Header {name:?} fehlt"),
        )),
        Some(roh) => T::aus_roh(roh).map_err(|grund| crate::api::json_fehler(400, &grund)),
    }
}

/// JSON-Body in einen eigenen Typ. Kaputtes JSON oder fehlendes Feld:
/// `400` als JSON-Fehler (der Client muss keinen Text parsen).
pub fn json_body<T: AusJson>(req: &Request) -> Result<T, Response> {
    let roh = match std::str::from_utf8(&req.body) {
        Ok(s) if !s.trim().is_empty() => s,
        Ok(_) => {
            return Err(crate::api::json_fehler(400, "JSON-Body fehlt"));
        }
        Err(_) => {
            return Err(crate::api::json_fehler(400, "JSON-Body ist kein UTF-8"));
        }
    };
    let wert =
        json::parse(roh).map_err(|e| crate::api::json_fehler(400, &format!("JSON-Fehler: {e}")))?;
    T::aus_json(&wert).map_err(|e| crate::api::json_fehler(400, &format!("JSON-Feld: {e}")))
}

/// JSON-Body, der auch fehlen darf (`None` bei leerem Body).
pub fn json_body_opt<T: AusJson>(req: &Request) -> Result<Option<T>, Response> {
    if req.body.is_empty() {
        return Ok(None);
    }
    json_body(req).map(Some)
}
