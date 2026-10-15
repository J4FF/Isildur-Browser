//! Begrenzte GraphQL-Auswahl ueber einem vorhandenen JSON-Wert.
//!
//! Unterstuetzt `query { ... }`, verschachtelte Felder sowie `id` und
//! `limit` als Argumente fuer Listen. Schema, Mutationen und Variablen
//! gehoeren bewusst nicht zu dieser kleinen API.

use std::collections::HashMap;

use crate::json::Value;

/// Ein HTTP-Body ist groesser moeglich, eine Abfrage selbst aber nicht.
pub const MAX_ABFRAGE_BYTES: usize = 16 * 1024;
/// Begrenzt die Rekursion des Parsers und der Projektion (M5).
pub const MAX_ABFRAGE_TIEFE: usize = 12;
/// Gesamtzahl der Felder, nicht je Ebene (M3/M4).
pub const MAX_ABFRAGE_FELDER: usize = 256;
/// Hoechstens diese Anzahl Argumente je Feld (M3/M4).
pub const MAX_ARGUMENTE: usize = 16;
/// Groesste explizite oder implizite Liste pro Feld (M3/M4).
pub const MAX_LISTE: usize = 1000;
/// Harte Grenze aller in einer Antwort erzeugten Werte (M3/M4).
pub const MAX_ERGEBNIS_KNOTEN: usize = 10_000;
/// Auch bei 10.000 Ergebnisknoten bleibt Text damit auf hoechstens
/// 40 MiB begrenzt, bevor die Antwort aufgebaut wird (M4).
pub const MAX_ERGEBNIS_TEXT: usize = 4 * 1024;
/// Statische Kosten: ein Feld plus je eine Einheit pro Unterfeld und
/// zehn Einheiten fuer jede Listenebene. Eine billige Abfrage bleibt
/// auch bei vielen Wiederholungen innerhalb dieser Grenze.
pub const MAX_ABFRAGE_KOSTEN: usize = 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct Auswahl {
    pub name: String,
    pub argumente: HashMap<String, Value>,
    pub unterfelder: Vec<Auswahl>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Abfrage {
    pub felder: Vec<Auswahl>,
    pub kosten: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbfrageFehler {
    ZuLang,
    ZuTief,
    ZuVieleFelder,
    ZuVieleArgumente,
    ZuTeuer,
    ZuGrossesErgebnis,
    Syntax,
    Argument,
}

struct Parser<'a> {
    text: &'a str,
    pos: usize,
    felder: usize,
    kosten: usize,
}

impl Parser<'_> {
    fn leerraum(&mut self) {
        let bytes = self.text.as_bytes();
        // MAX_ABFRAGE_BYTES deckelt die Iterationen vorab (M3).
        while self.pos < bytes.len()
            && (bytes[self.pos].is_ascii_whitespace() || bytes[self.pos] == b',')
        {
            self.pos += 1;
        }
    }

    fn zeichen(&mut self, c: u8) -> bool {
        self.leerraum();
        if self.text.as_bytes().get(self.pos) == Some(&c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn name(&mut self) -> Result<String, AbfrageFehler> {
        self.leerraum();
        let bytes = self.text.as_bytes();
        let start = self.pos;
        if !matches!(bytes.get(self.pos), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_')) {
            return Err(AbfrageFehler::Syntax);
        }
        self.pos += 1;
        while matches!(
            bytes.get(self.pos),
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
        ) {
            self.pos += 1;
        }
        Ok(self.text[start..self.pos].to_string())
    }

    fn wert(&mut self) -> Result<Value, AbfrageFehler> {
        self.leerraum();
        let bytes = self.text.as_bytes();
        if bytes.get(self.pos) == Some(&b'"') {
            let start = self.pos;
            self.pos += 1;
            let mut escaped = false;
            while let Some(&b) = bytes.get(self.pos) {
                self.pos += 1;
                if escaped {
                    escaped = false;
                    continue;
                }
                if b == b'\\' {
                    escaped = true;
                    continue;
                }
                if b == b'"' {
                    return crate::json::parse(&self.text[start..self.pos])
                        .map_err(|_| AbfrageFehler::Syntax);
                }
            }
            return Err(AbfrageFehler::Syntax);
        }
        let start = self.pos;
        if bytes.get(self.pos) == Some(&b'-') {
            self.pos += 1;
        }
        while matches!(bytes.get(self.pos), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if self.pos > start && &self.text[start..self.pos] != "-" {
            return self.text[start..self.pos]
                .parse::<i64>()
                .map(Value::Ganz)
                .map_err(|_| AbfrageFehler::Argument);
        }
        self.pos = start;
        match self.name()?.as_str() {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            "null" => Ok(Value::Null),
            _ => Err(AbfrageFehler::Argument),
        }
    }

    fn argumente(&mut self) -> Result<HashMap<String, Value>, AbfrageFehler> {
        let mut out = HashMap::new();
        if !self.zeichen(b'(') {
            return Ok(out);
        }
        for _ in 0..MAX_ARGUMENTE {
            if self.zeichen(b')') {
                return Ok(out);
            }
            let name = self.name()?;
            if !self.zeichen(b':') {
                return Err(AbfrageFehler::Syntax);
            }
            let wert = self.wert()?;
            if out.insert(name, wert).is_some() {
                return Err(AbfrageFehler::Argument);
            }
            if self.zeichen(b')') {
                return Ok(out);
            }
        }
        Err(AbfrageFehler::ZuVieleArgumente)
    }

    fn auswahl(&mut self, tiefe: usize) -> Result<Vec<Auswahl>, AbfrageFehler> {
        if tiefe > MAX_ABFRAGE_TIEFE {
            return Err(AbfrageFehler::ZuTief);
        }
        if !self.zeichen(b'{') {
            return Err(AbfrageFehler::Syntax);
        }
        let mut out = Vec::new();
        // Das Gesamtlimit wird vor jedem neuen Feld geprueft (M4).
        while !self.zeichen(b'}') {
            if self.felder >= MAX_ABFRAGE_FELDER {
                return Err(AbfrageFehler::ZuVieleFelder);
            }
            let name = self.name()?;
            self.felder += 1;
            self.kosten += 1;
            if self.kosten > MAX_ABFRAGE_KOSTEN {
                return Err(AbfrageFehler::ZuTeuer);
            }
            let argumente = self.argumente()?;
            self.leerraum();
            let unterfelder = if self.text.as_bytes().get(self.pos) == Some(&b'{') {
                self.kosten = self.kosten.saturating_add(10);
                if self.kosten > MAX_ABFRAGE_KOSTEN {
                    return Err(AbfrageFehler::ZuTeuer);
                }
                self.auswahl(tiefe + 1)?
            } else {
                Vec::new()
            };
            out.push(Auswahl {
                name,
                argumente,
                unterfelder,
            });
        }
        if out.is_empty() {
            return Err(AbfrageFehler::Syntax);
        }
        Ok(out)
    }
}

/// Eine kleine GraphQL-Abfrage parsen. Mutationen, Aliase, Direktiven und
/// Variablen werden als Syntaxfehler gemeldet.
pub fn parsen(text: &str) -> Result<Abfrage, AbfrageFehler> {
    if text.len() > MAX_ABFRAGE_BYTES {
        return Err(AbfrageFehler::ZuLang);
    }
    let mut p = Parser {
        text,
        pos: 0,
        felder: 0,
        kosten: 0,
    };
    p.leerraum();
    if p.text.as_bytes().get(p.pos) != Some(&b'{') && p.name()? != "query" {
        return Err(AbfrageFehler::Syntax);
    }
    let felder = p.auswahl(1)?;
    p.leerraum();
    if p.pos != text.len() {
        return Err(AbfrageFehler::Syntax);
    }
    Ok(Abfrage {
        felder,
        kosten: p.kosten,
    })
}

fn projekt(
    wert: &Value,
    felder: &[Auswahl],
    knoten: &mut usize,
    tiefe: usize,
) -> Result<Value, AbfrageFehler> {
    if tiefe > MAX_ABFRAGE_TIEFE {
        return Err(AbfrageFehler::ZuTief);
    }
    if felder.len() > MAX_ABFRAGE_FELDER {
        return Err(AbfrageFehler::ZuVieleFelder);
    }
    *knoten += 1;
    if *knoten > MAX_ERGEBNIS_KNOTEN {
        return Err(AbfrageFehler::ZuGrossesErgebnis);
    }
    match wert {
        Value::Objekt(map) => {
            let mut out = HashMap::new();
            for feld in felder {
                let gefunden = map.get(&feld.name).unwrap_or(&Value::Null);
                let ausgewertet = feldwert(gefunden, feld, knoten, tiefe + 1)?;
                out.insert(feld.name.clone(), ausgewertet);
            }
            Ok(Value::Objekt(out))
        }
        _ => Err(AbfrageFehler::Argument),
    }
}

fn feldwert(
    wert: &Value,
    feld: &Auswahl,
    knoten: &mut usize,
    tiefe: usize,
) -> Result<Value, AbfrageFehler> {
    for key in feld.argumente.keys() {
        if key != "id" && key != "limit" {
            return Err(AbfrageFehler::Argument);
        }
    }
    if let Some(limit) = feld.argumente.get("limit") {
        if !matches!(limit, Value::Ganz(n) if (0..=MAX_LISTE as i64).contains(n)) {
            return Err(AbfrageFehler::Argument);
        }
    }
    match wert {
        Value::Feld(liste) => {
            let limit = feld
                .argumente
                .get("limit")
                .and_then(Value::als_i64)
                .unwrap_or(MAX_LISTE as i64) as usize;
            let mut out = Vec::new();
            for eintrag in liste.iter().take(MAX_LISTE) {
                if let Some(id) = feld.argumente.get("id") {
                    if eintrag.feld("id") != Some(id) {
                        continue;
                    }
                }
                if out.len() >= limit {
                    break;
                }
                out.push(if feld.unterfelder.is_empty() {
                    gekappt_klonen(eintrag, knoten, tiefe)?
                } else {
                    projekt(eintrag, &feld.unterfelder, knoten, tiefe)?
                });
            }
            Ok(Value::Feld(out))
        }
        Value::Objekt(_) if !feld.unterfelder.is_empty() => {
            projekt(wert, &feld.unterfelder, knoten, tiefe)
        }
        Value::Null => Ok(Value::Null),
        _ if feld.unterfelder.is_empty() && feld.argumente.is_empty() => {
            gekappt_klonen(wert, knoten, tiefe)
        }
        _ => Err(AbfrageFehler::Argument),
    }
}

fn gekappt_klonen(wert: &Value, knoten: &mut usize, tiefe: usize) -> Result<Value, AbfrageFehler> {
    if tiefe > MAX_ABFRAGE_TIEFE {
        return Err(AbfrageFehler::ZuTief);
    }
    *knoten += 1;
    if *knoten > MAX_ERGEBNIS_KNOTEN {
        return Err(AbfrageFehler::ZuGrossesErgebnis);
    }
    match wert {
        Value::Feld(v) => {
            if v.len() > MAX_LISTE {
                return Err(AbfrageFehler::ZuGrossesErgebnis);
            }
            let mut out = Vec::new();
            for x in v {
                out.push(gekappt_klonen(x, knoten, tiefe + 1)?);
            }
            Ok(Value::Feld(out))
        }
        Value::Objekt(m) => {
            if m.len() > MAX_ABFRAGE_FELDER {
                return Err(AbfrageFehler::ZuGrossesErgebnis);
            }
            let mut out = HashMap::new();
            for (k, v) in m {
                out.insert(k.clone(), gekappt_klonen(v, knoten, tiefe + 1)?);
            }
            Ok(Value::Objekt(out))
        }
        Value::Text(s) if s.len() > MAX_ERGEBNIS_TEXT => Err(AbfrageFehler::ZuGrossesErgebnis),
        _ => Ok(wert.clone()),
    }
}

/// Eine gepruefte Abfrage gegen ein JSON-Objekt ausfuehren.
pub fn ausfuehren(wert: &Value, abfrage: &Abfrage) -> Result<Value, AbfrageFehler> {
    if abfrage.kosten > MAX_ABFRAGE_KOSTEN || abfrage.felder.len() > MAX_ABFRAGE_FELDER {
        return Err(AbfrageFehler::ZuTeuer);
    }
    projekt(wert, &abfrage.felder, &mut 0, 1)
}

/// Eine GraphQL-Abfrage aus einem Text auf vorhandene JSON-Daten anwenden.
/// Fehler erhalten eine JSON-Antwort mit 400 oder 413.
pub fn antwort(wert: &Value, text: &str) -> crate::http::Response {
    match parsen(text).and_then(|q| ausfuehren(wert, &q)) {
        Ok(v) => super::json_antwort(200, &v),
        Err(e) => {
            let code = match e {
                AbfrageFehler::ZuLang | AbfrageFehler::ZuGrossesErgebnis => 413,
                _ => 400,
            };
            super::json_fehler(code, &format!("GraphQL: {e:?}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echte_notizen_mit_argumenten_und_unterfeldern() {
        let daten = crate::json::parse(include_str!("../../examples/graphql_daten.json"))
            .expect("echte JSON-Datei");
        let q =
            parsen("query { notizen(id: 2, limit: 1) { id titel } }").expect("gueltige Abfrage");
        let ergebnis = ausfuehren(&daten, &q).expect("ausfuehren");
        let liste = ergebnis.feld("notizen").expect("Feld notizen");
        let Value::Feld(liste) = liste else {
            panic!("Liste erwartet")
        };
        assert_eq!(liste.len(), 1);
        assert_eq!(liste[0].feld("id"), Some(&Value::Ganz(2)));
        assert_eq!(
            liste[0].feld("titel").and_then(Value::als_text),
            Some("Einkauf")
        );
        assert!(liste[0].feld("intern").is_none());
    }

    #[test]
    fn tiefe_und_feldzahl_werden_abgelehnt() {
        let tief = format!(
            "{}id{}",
            "{ a ".repeat(MAX_ABFRAGE_TIEFE + 1),
            " }".repeat(MAX_ABFRAGE_TIEFE + 1)
        );
        assert_eq!(parsen(&tief), Err(AbfrageFehler::ZuTief));
        let viele = format!("{{ {} }}", "a ".repeat(MAX_ABFRAGE_FELDER + 1));
        assert_eq!(parsen(&viele), Err(AbfrageFehler::ZuVieleFelder));
    }

    #[test]
    fn kosten_und_listengrenze_werden_abgelehnt() {
        let teuer = format!("{{ {} }}", "a { b { c { d { e { f } } } } } ".repeat(100));
        assert_eq!(parsen(&teuer), Err(AbfrageFehler::ZuTeuer));
        let q = parsen("{ notizen(limit: 1001) { id } }").expect("Syntax");
        let daten = crate::json::parse(include_str!("../../examples/graphql_daten.json"))
            .expect("echte JSON-Datei");
        assert_eq!(ausfuehren(&daten, &q), Err(AbfrageFehler::Argument));
    }

    #[test]
    fn kaputte_syntax_hat_benannten_fehler() {
        assert_eq!(parsen("{ notizen("), Err(AbfrageFehler::Syntax));
        assert_eq!(parsen("mutation { notizen }"), Err(AbfrageFehler::Syntax));
        assert_eq!(
            parsen("{ notizen(id: 1, id: 2) }"),
            Err(AbfrageFehler::Argument)
        );
    }

    #[test]
    fn antwort_gibt_gueltiges_json_und_status() {
        let daten = crate::json::parse(include_str!("../../examples/graphql_daten.json"))
            .expect("echte JSON-Datei");
        let ok = antwort(&daten, "{ notizen(limit: 1) { titel } }");
        assert_eq!(ok.status.0, 200);
        let inhalt = crate::json::parse(std::str::from_utf8(&ok.body).expect("UTF-8"))
            .expect("JSON-Antwort");
        let Some(Value::Feld(liste)) = inhalt.feld("notizen") else {
            panic!("Liste erwartet")
        };
        assert_eq!(liste.len(), 1);
        let fehler = antwort(&daten, "{ notizen(limit: 1001) { titel } }");
        assert_eq!(fehler.status.0, 400);
    }
}
