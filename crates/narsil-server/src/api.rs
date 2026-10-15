//! REST-Helfer: Query, Pfad, JSON-Antworten, Fehlerabbildung.
//!
//! Der Router liefert [`crate::http::Request`], diese Crate macht daraus
//! API-Antworten. Kein Netz, kein Socket — nur Abbildung. Die Regel aus
//! `narsil-page` gilt auch hier: was reinkommt, ist Text; was rausgeht,
//! ist eine Antwort mit Typ.
//!
//! Plus: minimale GraphQL-Feld-Auswahl ([`graphql`]) — kein vollstaendiges
//! GraphQL (das waere ein eigenes Projekt), sondern die eine Eigenschaft,
//! die REST nicht hat: der Client waehlt Felder.

use std::collections::HashMap;

use crate::http::{Request, Response};
use crate::json::Value;

/// Maximale Anzahl von Query-Paaren (Regel M3).
///
/// `?a=1&a=1&...` mit 1 Mio Teilen waere sonst 1 Mio Paare mit je
/// zwei String-Allokationen — und die Schleife haengt von der Eingabe
/// ab. 4096 Paare sind mehr, als jede echte Seite braucht (Browser
/// begrenzen URLs ohnehin auf ~2.000 Zeichen).
pub const MAX_QUERY_PAARE: usize = 4096;

/// Maximale Anzahl ausgewaehlter Felder bei `?felder=...` (M3).
///
/// Ein Filter mit 100.000 Namen waere eine Schleife ueber die
/// Antwort, deren Laenge der Angreifer bestimmt.
pub const MAX_FELDER: usize = 256;

/// Query-String in Paare (`a=1&b=2` -> [("a","1"),("b","2")]).
///
/// `+` wird zu Leerzeichen (Query-Regel, nicht Pfad-Regel — siehe
/// `router::prozent_dekodieren` fuer den Unterschied). Kaputte
/// Sequenzen bleiben wie getippt. `?a` ohne `=` ist `(a, "")`.
///
/// **Obergrenze**: [`MAX_QUERY_PAARE`]. Ein Query mit 100.000 `&`
/// wuerde sonst 100.000 Paare und ebenso viele Allocationen erzeugen —
/// die Grenze kommt aus der Eingabe, also steht sie als Konstante da.
pub fn query_paare(query: &str) -> Vec<(String, String)> {
    if query.is_empty() {
        return Vec::new();
    }
    query
        .split('&')
        .take(MAX_QUERY_PAARE)
        .filter(|p| !p.is_empty())
        .map(|paar| match paar.split_once('=') {
            Some((k, v)) => (query_dekodieren(k), query_dekodieren(v)),
            None => (query_dekodieren(paar), String::new()),
        })
        .collect()
}

/// Query als Map (letzter Wert gewinnt bei Doppelten — wie Browser).
pub fn query_map(query: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for (k, v) in query_paare(query) {
        m.insert(k, v);
    }
    m
}

fn query_dekodieren(s: &str) -> String {
    let mit_leer = s.replace('+', " ");
    crate::router::prozent_dekodieren(&mit_leer)
}

/// JSON-Antwort aus einem [`Value`] (immer `200` oder der gegebene Code).
pub fn json_antwort(code: u16, wert: &Value) -> Response {
    Response::json(code, crate::json::stringify(wert))
}

/// Fehler als JSON (`{"fehler": "...", "code": 404}`), damit API-Clients
/// nicht Text parsen muessen, um zu erfahren, was schiefging.
pub fn json_fehler(code: u16, meldung: &str) -> Response {
    let mut m = HashMap::new();
    m.insert("fehler".to_string(), Value::Text(meldung.to_string()));
    m.insert("code".to_string(), Value::Ganz(code as i64));
    json_antwort(code, &Value::Objekt(m))
}

/// Body als JSON parsen. Leerer Body ist `None` (kein Fehler) — wer
/// JSON braucht, prueft danach selbst. Kaputtes JSON ist `400` als
/// JSON-Fehler, nicht als Text.
pub fn body_als_json(req: &Request) -> Result<Option<Value>, Response> {
    if req.body.is_empty() {
        return Ok(None);
    }
    match crate::json::parse(req.body_text().unwrap_or("")) {
        Ok(v) => Ok(Some(v)),
        Err(e) => Err(json_fehler(400, &format!("JSON-Fehler: {e}"))),
    }
}

/// Formular-Body (`application/x-www-form-urlencoded`) in Paare.
///
/// Das ist der native Ersatz fuer JS-Formulare (Vision: Alltagstauglich
/// ohne JavaScript): ein `<form method="POST">` schickt genau das, und
/// der Server liest es hier — kein Skript dazwischen.
pub fn formular_paare(req: &Request) -> Vec<(String, String)> {
    let typ = req.header("content-type").unwrap_or("");
    if !typ.contains("application/x-www-form-urlencoded") {
        return Vec::new();
    }
    query_paare(req.body_text().unwrap_or(""))
}

// ---------------------------------------------------------------------------
// Minimal-GraphQL: Feld-Auswahl, kein Query-Parser
// ---------------------------------------------------------------------------

/// Das GraphQL-Modul: der Client waehlt Felder, der Server filtert.
///
/// Vollstaendiges GraphQL (Schema, Typen, Resolver, Introspektion) ist
/// ein eigenes Projekt. Was hier steht, ist die EIGENSCHAFT, die REST
/// nicht hat: `?felder=name,preis` schneidet die Antwort zu. Das ist
/// ehrlich benannt (`graphql`-Modul mit einer Funktion), statt ein
/// halbes GraphQL zu behaupten.
pub mod graphql {
    use super::*;

    // added 02.10.2026, SERVER V13 (neunte und zehnte Stufe): die
    // Grenzen gehoeren in die oeffentliche Liste.
    //
    // `graphql_abfrage` ist ein **privates** Modul (`mod`, nicht
    // `pub mod`), also ist diese Liste der einzige Weg, auf dem ein
    // Aufrufer die Zahlen sehen kann, an denen die Abfrage scheitert.
    //
    // DER GRUND, und es ist kein Kosmetik-Argument: eine Grenze, die
    // niemand aus dem Crate heraus auslesen kann, ist eine Zahl, die
    // man raten muss — und „raten" ist der Grund, warum
    // `tests/v13_grenzen_reichen.rs` den privaten Weg gehen
    // **musste**. Der private Modulpfad war der Beweis, dass die Zahl
    // draussen keiner kannte.
    //
    // Die Liste waechst mit dem, was nachgewiesen wird. Wer sie nicht
    // braucht, laesst sie ungelesen — das kostet nichts.
    pub use super::graphql_abfrage::{
        Abfrage,
        AbfrageFehler,
        Auswahl,
        MAX_ABFRAGE_BYTES,
        MAX_ABFRAGE_FELDER,
        MAX_ABFRAGE_KOSTEN,
        MAX_ABFRAGE_TIEFE,
        MAX_ARGUMENTE,
        MAX_ERGEBNIS_KNOTEN,
        MAX_ERGEBNIS_TEXT,
        MAX_LISTE,
        // added 02.10.2026, SERVER V13 (neunte und zehnte Stufe):
        // die Funktionen gehoeren seit jeher hierher und sind beim
        // Umschreiben der Liste mit verschwunden. Sie sind die
        // **oeffentliche** Flaeche des Moduls — ohne `parsen` und
        // `ausfuehren` ist `api::graphql` ein leerer Namensraum mit
        // Zahlen darin, und das war kurz der Zustand (23:55).
        antwort,
        ausfuehren,
        parsen,
    };

    /// Filtert ein JSON-Objekt auf die gewaehlten Felder.
    ///
    /// `felder` ist die `felder`-Query (`name,preis`). Leer oder `*`
    /// heisst: alles. Unbekannte Felder werden ignoriert (nicht als
    /// Fehler gemeldet — der Client bekommt, was es gibt).
    pub fn waehle_felder(wert: &Value, felder: &str) -> Value {
        let f = felder.trim();
        if f.is_empty() || f == "*" {
            return wert.clone();
        }
        let gewollt: Vec<&str> = f
            .split(',')
            .take(MAX_FELDER)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        match wert {
            Value::Objekt(m) => {
                let mut neu = HashMap::new();
                for k in gewollt {
                    if let Some(v) = m.get(k) {
                        neu.insert(k.to_string(), v.clone());
                    }
                }
                Value::Objekt(neu)
            }
            Value::Feld(liste) => {
                Value::Feld(liste.iter().map(|e| waehle_felder(e, felder)).collect())
            }
            _ => wert.clone(),
        }
    }

    /// Antwortet mit Feld-Auswahl aus der Query (`?felder=...`).
    pub fn antwort_mit_feldern(code: u16, wert: &Value, req: &Request) -> Response {
        let q = query_map(&req.query);
        let felder = q.get("felder").map(String::as_str).unwrap_or("*");
        json_antwort(code, &waehle_felder(wert, felder))
    }
}

mod graphql_abfrage;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::anfrage_aus_text;

    #[test]
    fn query_mit_plus_und_prozent() {
        let paare = query_paare("a=1&b=Hallo+Welt&c=%C3%A4");
        assert_eq!(
            paare,
            vec![
                ("a".to_string(), "1".to_string()),
                ("b".to_string(), "Hallo Welt".to_string()),
                ("c".to_string(), "ä".to_string()),
            ]
        );
    }

    #[test]
    fn json_fehler_hat_code() {
        let r = json_fehler(404, "fehlt");
        assert_eq!(r.status.0, 404);
        assert!(r.header("content-type").unwrap().contains("json"));
        assert!(String::from_utf8(r.body.clone()).unwrap().contains("fehlt"));
    }

    #[test]
    fn formular_wird_gelesen() {
        let req = anfrage_aus_text(
            "POST",
            "/form",
            &[("Content-Type", "application/x-www-form-urlencoded")],
            b"name=Max&alter=7",
        );
        let paare = formular_paare(&req);
        assert!(paare.contains(&("name".to_string(), "Max".to_string())));
    }

    #[test]
    fn graphql_schneidet_zu() {
        let mut m = HashMap::new();
        m.insert("name".to_string(), Value::Text("Brot".to_string()));
        m.insert("preis".to_string(), Value::Zahl(2.0));
        m.insert("intern".to_string(), Value::Text("x".to_string()));
        let v = Value::Objekt(m);
        let klein = graphql::waehle_felder(&v, "name,preis");
        assert!(klein.feld("name").is_some());
        assert!(klein.feld("intern").is_none());
        let alles = graphql::waehle_felder(&v, "*");
        assert_eq!(alles, v);
    }

    #[test]
    fn body_leer_ist_none() {
        let req = anfrage_aus_text("POST", "/", &[], b"");
        assert!(body_als_json(&req).unwrap().is_none());
        let req = anfrage_aus_text("POST", "/", &[], b"{kaputt");
        assert!(body_als_json(&req).is_err());
    }
}
