//! Minimal-JSON in reinem Rust: Wert, Parser, Serializer.
//!
//! Fuer REST braucht der Server JSON — aber keine JSON-Crate (ADR-030).
//! Also steht hier das kleinste JSON, das ehrlich ist: Objekte,
//! Arrays, Strings (mit Escapes), Zahlen, `true`/`false`/`null`.
//! Kein Streaming, keine Comments, keine Trailing Commas. Was nicht
//! JSON ist, ist ein Fehler mit Position — kein Panic, kein `null`
//! als Antwort auf alles.
//!
//! Zahlen werden als `f64` gespeichert (wie JavaScript, das der Browser
//! gerade NICHT ausfuehrt — aber die API-Clients rechnen damit). Wer
//! ganze Zahlen braucht, fragt [`Value::als_i64`].

use std::collections::HashMap;

/// Maximale Verschachtelungstiefe (Regel M5).
///
/// JSON-Parser sind REKURSIV, und die Tiefe kommt aus der Eingabe:
/// 100.000 `[` wuerden den Stack leeren, bevor irgendein Limit greift.
/// 128 Ebenen sind mehr, als jede ehrliche API-Antwort braucht.
pub const MAX_JSON_TIEFE: usize = 128;

/// Maximale Anzahl von Knoten je Dokument (Regel M4/M6).
///
/// Nicht die Eingabegroesse allein reicht nicht: 10 MiB voller Ziffern
/// sind eine einzige Zahl, 10 MiB voller `[1,1,1,...]` sind 5 Mio
/// Knoten mit je einem Heap-Eintrag. Diese Grenze deckelt die ERZEUGTEN
/// Objekte, nicht nur die Bytes. Rechnung: 500.000 * ~32 Byte
/// (Enum + Vec/HashMap-Handle) = rund 16 MiB im schlimmsten Fall —
/// das ist die Obergrenze, nicht die Eingabe.
pub const MAX_JSON_KNOTEN: usize = 500_000;

/// Maximale Anzahl von Feldern je Objekt.
///
/// Ein Objekt mit 5 Mio Schluesseln ist keine Antwort, sondern ein
/// Angriff auf die HashMap — und die Schleife, die sie einliest, haengt
/// von der Eingabe ab (M3).
pub const MAX_JSON_FELDER: usize = 100_000;

/// Ein JSON-Wert.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    /// Ganze Zahl OHNE Genauigkeitsverlust. Ein `f64` kann nur bis
    /// 2^53 exakt zaehlen — `9007199254740993` waere still `...92`
    /// geworden, und eine ID, die sich um eins verschoben hat, ist
    /// die schlimmste Art von falsch. Deshalb eigener Arm.
    Ganz(i64),
    /// Kommazahl oder Zahl mit Exponent (f64, wie JavaScript).
    Zahl(f64),
    Text(String),
    Feld(Vec<Value>),
    Objekt(HashMap<String, Value>),
}

impl Value {
    /// Objekt-Feld lesen.
    pub fn feld(&self, name: &str) -> Option<&Value> {
        match self {
            Value::Objekt(m) => m.get(name),
            _ => None,
        }
    }

    /// Als Text, wenn es einer ist.
    pub fn als_text(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }

    /// Als Bool, wenn es einer ist.
    pub fn als_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Als `f64`, wenn es eine Zahl ist (Ganzzahlen werden umgerechnet).
    pub fn als_f64(&self) -> Option<f64> {
        match self {
            Value::Ganz(n) => Some(*n as f64),
            Value::Zahl(n) => Some(*n),
            _ => None,
        }
    }

    /// Als `i64`, wenn es eine ganze Zahl im Bereich ist.
    ///
    /// `Ganz` liefert den Wert EXAKT, `Zahl` nur, wenn er wirklich
    /// ganzzahlig und im Bereich liegt (dann war er es auch als f64).
    pub fn als_i64(&self) -> Option<i64> {
        match self {
            Value::Ganz(n) => Some(*n),
            Value::Zahl(n)
                if n.fract() == 0.0 && *n >= i64::MIN as f64 && *n <= i64::MAX as f64 =>
            {
                Some(*n as i64)
            }
            _ => None,
        }
    }

    /// Array-Laenge, wenn es eines ist.
    pub fn laenge(&self) -> Option<usize> {
        match self {
            Value::Feld(v) => Some(v.len()),
            _ => None,
        }
    }
}

/// Fehler beim Parsen, mit Byteposition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonFehler {
    /// Was schiefging.
    pub meldung: String,
    /// Byteposition im Eingang.
    pub position: usize,
}

impl std::fmt::Display for JsonFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} bei Byte {}", self.meldung, self.position)
    }
}

impl std::error::Error for JsonFehler {}

impl JsonFehler {
    /// Fehler ohne Byteposition — fuer [`AusJson`]-Typen, die aus einem
    /// fertigen Wert gebaut werden statt aus einem Text.
    pub fn neu(meldung: impl Into<String>) -> Self {
        JsonFehler {
            meldung: meldung.into(),
            position: 0,
        }
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    /// Aktuelle Verschachtelungstiefe (M5: gegen [`MAX_JSON_TIEFE`]).
    tiefe: usize,
    /// Erzeugte Knoten gesamt (M4: gegen [`MAX_JSON_KNOTEN`]).
    knoten: usize,
}

impl<'a> Parser<'a> {
    fn neu(s: &'a str) -> Self {
        Parser {
            bytes: s.as_bytes(),
            pos: 0,
            tiefe: 0,
            knoten: 0,
        }
    }

    fn fehler(&self, meldung: &str) -> JsonFehler {
        JsonFehler {
            meldung: meldung.to_string(),
            position: self.pos,
        }
    }

    fn rest(&self) -> &'a [u8] {
        &self.bytes[self.pos.min(self.bytes.len())..]
    }

    fn whitespace(&mut self) {
        while matches!(self.rest().first(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn erwartet(&mut self, c: u8) -> Result<(), JsonFehler> {
        self.whitespace();
        match self.rest().first() {
            Some(&b) if b == c => {
                self.pos += 1;
                Ok(())
            }
            _ => Err(self.fehler(&format!("erwartet '{}'", c as char))),
        }
    }

    fn wert(&mut self) -> Result<Value, JsonFehler> {
        self.whitespace();
        // M4: die Anzahl der ERZEUGTEN Knoten wird geprueft, bevor der
        // naechste entsteht — nicht erst, wenn der Speicher schon weg ist.
        self.knoten += 1;
        if self.knoten > MAX_JSON_KNOTEN {
            return Err(self.fehler("JSON zu komplex (Knotenzahl)"));
        }
        match self.rest().first() {
            Some(b'{') => self.objekt(),
            Some(b'[') => self.feld(),
            Some(b'"') => Ok(Value::Text(self.string()?)),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'n') => self.literal("null", Value::Null),
            Some(b'-') | Some(b'0'..=b'9') => self.zahl(),
            _ => Err(self.fehler("Wert erwartet")),
        }
    }

    fn literal(&mut self, wort: &str, wert: Value) -> Result<Value, JsonFehler> {
        if self.rest().starts_with(wort.as_bytes()) {
            self.pos += wort.len();
            Ok(wert)
        } else {
            Err(self.fehler("Literal erwartet"))
        }
    }

    fn objekt(&mut self) -> Result<Value, JsonFehler> {
        self.erwartet(b'{')?;
        // M5: Tiefe BEVOR rekursiv gelesen wird, danach zaehlen.
        if self.tiefe >= MAX_JSON_TIEFE {
            return Err(self.fehler("JSON zu tief verschachtelt"));
        }
        self.tiefe += 1;
        let mut map = HashMap::new();
        self.whitespace();
        if self.rest().first() == Some(&b'}') {
            self.pos += 1;
            self.tiefe -= 1;
            return Ok(Value::Objekt(map));
        }
        loop {
            self.whitespace();
            if self.rest().first() != Some(&b'"') {
                return Err(self.fehler("Objektschluessel muss ein String sein"));
            }
            let schluessel = self.string()?;
            self.whitespace();
            self.erwartet(b':')?;
            let wert = self.wert()?;
            // Doppelte Schluessel: der letzte gewinnt (wie JavaScript).
            map.insert(schluessel, wert);
            if map.len() > MAX_JSON_FELDER {
                return Err(self.fehler("Objekt zu gross (Feldzahl)"));
            }
            self.whitespace();
            match self.rest().first() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b'}') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(self.fehler("erwartet ',' oder '}'")),
            }
        }
        self.tiefe -= 1;
        Ok(Value::Objekt(map))
    }

    fn feld(&mut self) -> Result<Value, JsonFehler> {
        self.erwartet(b'[')?;
        if self.tiefe >= MAX_JSON_TIEFE {
            return Err(self.fehler("JSON zu tief verschachtelt"));
        }
        self.tiefe += 1;
        let mut werte = Vec::new();
        self.whitespace();
        if self.rest().first() == Some(&b']') {
            self.pos += 1;
            self.tiefe -= 1;
            return Ok(Value::Feld(werte));
        }
        loop {
            werte.push(self.wert()?);
            // Schutz gegen Speicher-Explosion: 100k Elemente sind keine
            // API-Antwort, sondern ein Angriff.
            if werte.len() > MAX_JSON_FELDER {
                return Err(self.fehler("Array zu gross"));
            }
            self.whitespace();
            match self.rest().first() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b']') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(self.fehler("erwartet ',' oder ']'")),
            }
        }
        self.tiefe -= 1;
        Ok(Value::Feld(werte))
    }

    fn string(&mut self) -> Result<String, JsonFehler> {
        self.erwartet(b'"')?;
        let mut raus = String::new();
        loop {
            let Some(&b) = self.rest().first() else {
                return Err(self.fehler("String nicht geschlossen"));
            };
            match b {
                b'"' => {
                    self.pos += 1;
                    break;
                }
                b'\\' => {
                    self.pos += 1;
                    let Some(&e) = self.rest().first() else {
                        return Err(self.fehler("Escape nicht geschlossen"));
                    };
                    match e {
                        b'"' => raus.push('"'),
                        b'\\' => raus.push('\\'),
                        b'/' => raus.push('/'),
                        b'b' => raus.push('\u{0008}'),
                        b'f' => raus.push('\u{000C}'),
                        b'n' => raus.push('\n'),
                        b'r' => raus.push('\r'),
                        b't' => raus.push('\t'),
                        b'u' => {
                            // \uXXXX, inklusive Surrogatpaar: die
                            // Kombination zweier 16-Bit-Halbziffern zu
                            // einem Zeichen uber 0xFFFF ist GENAU der
                            // Fall, den ein eigener Escapeteil sonst
                            // still zerlegt (Test: Emoji in API-Antwort).
                            let code = self.unicode_escape()?;
                            let c = if (0xD800..0xDC00).contains(&code) {
                                if !self.rest().starts_with(b"\\u") {
                                    return Err(self.fehler("Surrogatpaar unvollstaendig"));
                                }
                                self.pos += 1; // jetzt auf dem 'u'
                                let niedrig = self.unicode_escape()?;
                                if !(0xDC00..0xE000).contains(&niedrig) {
                                    return Err(self.fehler("Surrogatpaar: zweites Zeichen falsch"));
                                }
                                let kombiniert =
                                    0x10000 + ((code - 0xD800) << 10) + (niedrig - 0xDC00);
                                char::from_u32(kombiniert)
                                    .ok_or_else(|| self.fehler("Surrogatpaar ungueltig"))?
                            } else if (0xDC00..0xE000).contains(&code) {
                                return Err(self.fehler("niedriger Surrogat ohne hohen Partner"));
                            } else {
                                char::from_u32(code)
                                    .ok_or_else(|| self.fehler("Unicode-Escape kein Zeichen"))?
                            };
                            raus.push(c);
                            continue;
                        }
                        _ => return Err(self.fehler("unbekannte Escape-Sequenz")),
                    }
                    self.pos += 1;
                }
                0x00..=0x1F => return Err(self.fehler("Kontrollzeichen im String")),
                _ => {
                    // UTF-8-Zeichen am Stueck (nicht Byte-fuer-Byte, sonst
                    // zerschneidet es Umlaute an der Bytegrenze).
                    let rest_str = std::str::from_utf8(self.rest())
                        .map_err(|_| self.fehler("kein gueltiges UTF-8"))?;
                    let c = rest_str
                        .chars()
                        .next()
                        .ok_or_else(|| self.fehler("String nicht geschlossen"))?;
                    raus.push(c);
                    self.pos += c.len_utf8();
                }
            }
        }
        Ok(raus)
    }

    /// `\uXXXX` lesen. `pos` zeigt dabei auf das `u`; danach steht es
    /// hinter den vier Ziffern. Fehlt eines davon: benannter Fehler.
    fn unicode_escape(&mut self) -> Result<u32, JsonFehler> {
        if self.pos + 5 > self.bytes.len() {
            return Err(self.fehler("Unicode-Escape zu kurz"));
        }
        let hex = &self.bytes[self.pos + 1..self.pos + 5];
        let hex_str =
            std::str::from_utf8(hex).map_err(|_| self.fehler("Unicode-Escape kein ASCII"))?;
        let code = u32::from_str_radix(hex_str, 16)
            .map_err(|_| self.fehler("Unicode-Escape keine Hexzahl"))?;
        self.pos += 5;
        Ok(code)
    }

    fn zahl(&mut self) -> Result<Value, JsonFehler> {
        let rest = self.rest();
        let mut i = 0;
        if rest.first() == Some(&b'-') {
            i += 1;
        }
        // Vorkomma: 0 oder [1-9][0-9]* (keine fuehrenden Nullen wie `01`).
        match rest.get(i) {
            Some(b'0') => i += 1,
            Some(b'1'..=b'9') => {
                while matches!(rest.get(i), Some(b'0'..=b'9')) {
                    i += 1;
                }
            }
            _ => return Err(self.fehler("Zahl erwartet")),
        }
        if rest.get(i) == Some(&b'.') {
            i += 1;
            let nachkomma_start = i;
            while matches!(rest.get(i), Some(b'0'..=b'9')) {
                i += 1;
            }
            if i == nachkomma_start {
                return Err(self.fehler("Nachkommastellen erwartet"));
            }
        }
        if matches!(rest.get(i), Some(b'e' | b'E')) {
            i += 1;
            if matches!(rest.get(i), Some(b'+' | b'-')) {
                i += 1;
            }
            let exp_start = i;
            while matches!(rest.get(i), Some(b'0'..=b'9')) {
                i += 1;
            }
            if i == exp_start {
                return Err(self.fehler("Exponent erwartet"));
            }
        }
        let text = std::str::from_utf8(&rest[..i]).map_err(|_| self.fehler("Zahl kein UTF-8"))?;
        // OHNE Punkt und OHNE Exponent ist es eine ganze Zahl — die wird
        // EXAKT gespeichert. f64 ist erst bis 2^53 vollstaendig; eine
        // fortlaufende Nummer, die sich um eins verschoben hat, ist die
        // schlimmste Art von falsch (deshalb [`Value::Ganz`]).
        if !text.bytes().any(|b| matches!(b, b'.' | b'e' | b'E')) {
            if let Ok(ganz) = text.parse::<i64>() {
                self.pos += i;
                return Ok(Value::Ganz(ganz));
            }
        }
        let n: f64 = text.parse().map_err(|_| self.fehler("Zahl nicht lesbar"))?;
        if !n.is_finite() {
            return Err(self.fehler("Zahl nicht endlich"));
        }
        self.pos += i;
        Ok(Value::Zahl(n))
    }
}

/// JSON parsen. Der GANZE Eingang muss EIN Wert sein — danach darf nur
/// noch Whitespace kommen. `{} garbage` ist kein Objekt, sondern ein Fehler.
pub fn parse(text: &str) -> Result<Value, JsonFehler> {
    if text.len() > crate::MAX_BODY_BYTES {
        return Err(JsonFehler {
            meldung: "JSON zu gross".to_string(),
            position: 0,
        });
    }
    let mut p = Parser::neu(text);
    let wert = p.wert()?;
    p.whitespace();
    if !p.rest().is_empty() {
        return Err(p.fehler("Ueberschuessige Zeichen nach dem Wert"));
    }
    Ok(wert)
}

/// JSON schreiben (kompakt, ohne Leerzeichen — der Server schreibt fuer
/// Maschinen, nicht fuer Menschen).
pub fn stringify(wert: &Value) -> String {
    let mut raus = String::new();
    schreibe(wert, &mut raus);
    raus
}

fn schreibe(wert: &Value, raus: &mut String) {
    match wert {
        Value::Null => raus.push_str("null"),
        Value::Bool(true) => raus.push_str("true"),
        Value::Bool(false) => raus.push_str("false"),
        Value::Ganz(n) => {
            // Ganzzahlen EXAKT raus — kein f64-Zwischenweg.
            raus.push_str(&n.to_string());
        }
        Value::Zahl(n) => {
            if n.fract() == 0.0 && n.abs() < 1e15 {
                raus.push_str(&format!("{}", *n as i64));
            } else {
                raus.push_str(&format!("{n}"));
            }
        }
        Value::Text(s) => {
            raus.push('"');
            for c in s.chars() {
                match c {
                    '"' => raus.push_str("\\\""),
                    '\\' => raus.push_str("\\\\"),
                    '\n' => raus.push_str("\\n"),
                    '\r' => raus.push_str("\\r"),
                    '\t' => raus.push_str("\\t"),
                    c if (c as u32) < 0x20 => raus.push_str(&format!("\\u{:04x}", c as u32)),
                    c => raus.push(c),
                }
            }
            raus.push('"');
        }
        Value::Feld(v) => {
            raus.push('[');
            for (i, e) in v.iter().enumerate() {
                if i > 0 {
                    raus.push(',');
                }
                schreibe(e, raus);
            }
            raus.push(']');
        }
        Value::Objekt(m) => {
            raus.push('{');
            // Sortiert, damit Tests deterministisch sind — HashMap allein
            // wuerde bei jedem Lauf anders schreiben.
            let mut schluessel: Vec<&String> = m.keys().collect();
            schluessel.sort();
            for (i, k) in schluessel.iter().enumerate() {
                if i > 0 {
                    raus.push(',');
                }
                schreibe(&Value::Text((*k).clone()), raus);
                raus.push(':');
                schreibe(&m[*k], raus);
            }
            raus.push('}');
        }
    }
}

// ---------------------------------------------------------------------------
// Traits + Ableitungs-Makro (S3): kein serde, kein proc-macro.
// ---------------------------------------------------------------------------

/// Ein Typ, der JSON geschrieben werden kann.
pub trait ZuJson {
    /// Den Typ als [`Value`] (der danach mit [`stringify`] auf die
    /// Leitung geht).
    fn zu_json(&self) -> Value;
}

/// Ein Typ, der aus JSON gebaut wird.
///
/// Der Fehler ist [`JsonFehler`] mit `position: 0` — die Position ist
/// hier nicht eine Byte-Adresse im Text, sondern die Feldkette, die
/// in der Meldung steht (z. B. `Feld "alter": ...`).
pub trait AusJson: Sized {
    /// Aus einem fertigen [`Value`]. Falscher Typ oder fehlendes Feld
    /// ist ein FEHLER, kein `Default` — sonst erfindet der Server
    /// Werte, die nie geschickt wurden.
    fn aus_json(wert: &Value) -> Result<Self, JsonFehler>;
}

impl ZuJson for Value {
    fn zu_json(&self) -> Value {
        self.clone()
    }
}
impl AusJson for Value {
    fn aus_json(wert: &Value) -> Result<Self, JsonFehler> {
        Ok(wert.clone())
    }
}

impl ZuJson for String {
    fn zu_json(&self) -> Value {
        Value::Text(self.clone())
    }
}
impl AusJson for String {
    fn aus_json(wert: &Value) -> Result<Self, JsonFehler> {
        wert.als_text()
            .map(str::to_string)
            .ok_or_else(|| JsonFehler::neu("Text erwartet"))
    }
}

impl ZuJson for &str {
    fn zu_json(&self) -> Value {
        Value::Text((*self).to_string())
    }
}

impl ZuJson for bool {
    fn zu_json(&self) -> Value {
        Value::Bool(*self)
    }
}
impl AusJson for bool {
    fn aus_json(wert: &Value) -> Result<Self, JsonFehler> {
        wert.als_bool()
            .ok_or_else(|| JsonFehler::neu("bool erwartet"))
    }
}

impl ZuJson for i64 {
    fn zu_json(&self) -> Value {
        Value::Ganz(*self)
    }
}
impl AusJson for i64 {
    fn aus_json(wert: &Value) -> Result<Self, JsonFehler> {
        wert.als_i64()
            .ok_or_else(|| JsonFehler::neu("ganze Zahl erwartet"))
    }
}

impl ZuJson for i32 {
    fn zu_json(&self) -> Value {
        Value::Ganz(*self as i64)
    }
}
impl AusJson for i32 {
    fn aus_json(wert: &Value) -> Result<Self, JsonFehler> {
        let n = wert
            .als_i64()
            .ok_or_else(|| JsonFehler::neu("ganze Zahl erwartet"))?;
        i32::try_from(n).map_err(|_| JsonFehler::neu("Zahl zu gross fuer i32"))
    }
}

impl ZuJson for u32 {
    fn zu_json(&self) -> Value {
        Value::Ganz(*self as i64)
    }
}
impl AusJson for u32 {
    fn aus_json(wert: &Value) -> Result<Self, JsonFehler> {
        let n = wert
            .als_i64()
            .ok_or_else(|| JsonFehler::neu("ganze Zahl erwartet"))?;
        u32::try_from(n).map_err(|_| JsonFehler::neu("Zahl ist negativ oder zu gross"))
    }
}

impl ZuJson for u64 {
    fn zu_json(&self) -> Value {
        // u64 oberhalb von i64::MAX waere fuer `Ganz` zu gross — dann
        // bewusst als f64, mit dem dokumentierten Preis: ab 2^53 keine
        // Einzelheiten mehr. Wer IDs so gross macht, hat ein Design-
        // Problem, kein Serialisierungs-Problem.
        if *self <= i64::MAX as u64 {
            Value::Ganz(*self as i64)
        } else {
            Value::Zahl(*self as f64)
        }
    }
}
impl AusJson for u64 {
    fn aus_json(wert: &Value) -> Result<Self, JsonFehler> {
        match wert {
            Value::Ganz(n) if *n >= 0 => Ok(*n as u64),
            Value::Ganz(_) => Err(JsonFehler::neu("Zahl ist negativ")),
            Value::Zahl(n) if n.fract() == 0.0 && *n >= 0.0 && *n <= u64::MAX as f64 => {
                Ok(*n as u64)
            }
            _ => Err(JsonFehler::neu("u64 erwartet")),
        }
    }
}

impl ZuJson for usize {
    fn zu_json(&self) -> Value {
        (*self as u64).zu_json()
    }
}
impl AusJson for usize {
    fn aus_json(wert: &Value) -> Result<Self, JsonFehler> {
        u64::aus_json(wert).map(|n| n as usize)
    }
}

impl ZuJson for f64 {
    fn zu_json(&self) -> Value {
        Value::Zahl(*self)
    }
}
impl AusJson for f64 {
    fn aus_json(wert: &Value) -> Result<Self, JsonFehler> {
        let n = wert
            .als_f64()
            .ok_or_else(|| JsonFehler::neu("Zahl erwartet"))?;
        if n.is_finite() {
            Ok(n)
        } else {
            Err(JsonFehler::neu("Zahl nicht endlich"))
        }
    }
}

impl<T: ZuJson> ZuJson for Option<T> {
    fn zu_json(&self) -> Value {
        match self {
            Some(w) => w.zu_json(),
            None => Value::Null,
        }
    }
}
impl<T: AusJson> AusJson for Option<T> {
    fn aus_json(wert: &Value) -> Result<Self, JsonFehler> {
        match wert {
            Value::Null => Ok(None),
            _ => T::aus_json(wert).map(Some),
        }
    }
}

impl<T: ZuJson> ZuJson for Vec<T> {
    fn zu_json(&self) -> Value {
        Value::Feld(self.iter().map(ZuJson::zu_json).collect())
    }
}
impl<T: AusJson> AusJson for Vec<T> {
    fn aus_json(wert: &Value) -> Result<Self, JsonFehler> {
        match wert {
            Value::Feld(felder) => felder.iter().map(T::aus_json).collect(),
            _ => Err(JsonFehler::neu("Array erwartet")),
        }
    }
}

impl<T: ZuJson> ZuJson for std::collections::HashMap<String, T> {
    fn zu_json(&self) -> Value {
        Value::Objekt(self.iter().map(|(k, v)| (k.clone(), v.zu_json())).collect())
    }
}
impl<T: AusJson> AusJson for std::collections::HashMap<String, T> {
    fn aus_json(wert: &Value) -> Result<Self, JsonFehler> {
        match wert {
            Value::Objekt(m) => m
                .iter()
                .map(|(k, v)| T::aus_json(v).map(|w| (k.clone(), w)))
                .collect(),
            _ => Err(JsonFehler::neu("Objekt erwartet")),
        }
    }
}

/// Deklarative Ableitung fuer einfache Structs — KEIN proc-macro, kein
/// serde, keine Abhaengigkeit. Nur `macro_rules!` (ADR-030: nur std).
///
/// ```rust
/// use narsil_server::json::{self, AusJson, ZuJson};
///
/// #[derive(Debug, PartialEq)]
/// struct Notiz { id: u64, titel: String, erledigt: bool }
/// narsil_server::ableiten_json!(Notiz { id: u64, titel: String, erledigt: bool });
///
/// let n = Notiz { id: 7, titel: "Kaufen".into(), erledigt: false };
/// let text = json::stringify(&n.zu_json());
/// assert_eq!(text, r#"{"erledigt":false,"id":7,"titel":"Kaufen"}"#);
/// let zurueck = Notiz::aus_json(&json::parse(&text).unwrap()).unwrap();
/// assert_eq!(zurueck, n);
/// ```
///
/// FEHLENDES Feld ist ein Fehler mit Feldnamen, nicht `Default::default()`.
#[macro_export]
macro_rules! ableiten_json {
    ($name:ident { $( $feld:ident : $typ:ty ),+ $(,)? }) => {
        impl $crate::json::ZuJson for $name {
            fn zu_json(&self) -> $crate::json::Value {
                let mut __m: ::std::collections::HashMap<::std::string::String, $crate::json::Value> =
                    ::std::collections::HashMap::new();
                $(
                    __m.insert(
                        ::std::stringify!($feld).to_string(),
                        $crate::json::ZuJson::zu_json(&self.$feld),
                    );
                )+
                $crate::json::Value::Objekt(__m)
            }
        }

        impl $crate::json::AusJson for $name {
            fn aus_json(
                __wert: &$crate::json::Value,
            ) -> ::std::result::Result<Self, $crate::json::JsonFehler> {
                $(
                    let $feld: $typ = match __wert.feld(::std::stringify!($feld)) {
                        Some(__v) => $crate::json::AusJson::aus_json(__v).map_err(|__e| {
                            $crate::json::JsonFehler::neu(::std::format!(
                                "Feld {:?}: {}",
                                ::std::stringify!($feld),
                                __e.meldung
                            ))
                        })?,
                        None => {
                            return Err($crate::json::JsonFehler::neu(::std::format!(
                                "Feld {:?} fehlt",
                                ::std::stringify!($feld)
                            )));
                        }
                    };
                )+
                Ok(Self { $( $feld ),+ })
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objekt_mit_allen_arten() {
        let v = parse(r#"{"a":1,"b":"x","c":true,"d":null,"e":[1,2]}"#).unwrap();
        assert_eq!(v.feld("a").and_then(|x| x.als_i64()), Some(1));
        assert_eq!(v.feld("b").and_then(|x| x.als_text()), Some("x"));
        assert_eq!(v.feld("c").and_then(|x| x.als_bool()), Some(true));
        assert_eq!(v.feld("e").and_then(|x| x.laenge()), Some(2));
    }

    #[test]
    fn kaputtes_json_hat_position() {
        let f = parse(r#"{"a":}"#).unwrap_err();
        assert!(f.position > 0);
        assert!(parse(r#"{"a":1} trailing"#).is_err());
    }

    #[test]
    fn runde_schreibt_und_liest() {
        let mut m = HashMap::new();
        m.insert(
            "gruss".to_string(),
            Value::Text("Hallo \"Welt\"\n".to_string()),
        );
        m.insert("n".to_string(), Value::Ganz(42));
        let v = Value::Objekt(m);
        let s = stringify(&v);
        assert_eq!(s, r#"{"gruss":"Hallo \"Welt\"\n","n":42}"#);
        let zurueck = parse(&s).unwrap();
        assert_eq!(zurueck, v);
    }

    #[test]
    fn ganzzahlen_bleiben_exakt() {
        // 2^53 + 1 ist die kleinste Zahl, die ein f64 NICHT mehr darstellen
        // kann. Als Ganz muss sie dennoch stimmen.
        let gross = "9007199254740993";
        let v = parse(gross).unwrap();
        assert_eq!(v, Value::Ganz(9007199254740993));
        assert_eq!(v.als_i64(), Some(9007199254740993));
        assert_eq!(stringify(&v), gross);
        // Und eine Zahl, die eine f64-Eingabe war, bleibt Kommazahl.
        assert_eq!(parse("1.5").unwrap(), Value::Zahl(1.5));
        // i64-Enden duerfen nicht ueberlaufen.
        assert_eq!(
            parse("-9223372036854775808").unwrap().als_i64(),
            Some(i64::MIN)
        );
    }

    #[test]
    fn surrogatpaare_werden_kombiniert() {
        // U+1F600 = 😀 = \uD83D\uDE00
        let v = parse(r#""😀""#).unwrap();
        assert_eq!(v.als_text(), Some("😀"));
        // Und ein Einzel-Surrogat ist ein FEHLER, kein Ersatzzeichen.
        assert!(parse(r#""\uD83D""#).is_err());
        assert!(parse(r#""\uDE00""#).is_err());
        // BMP-Zeichen funktionieren weiterhin.
        assert_eq!(parse(r#""ä""#).unwrap().als_text(), Some("ä"));
    }

    #[test]
    fn tiefe_und_knoten_sind_begrenzt() {
        let tief = "[".repeat(500) + &"]".repeat(500);
        let f = parse(&tief).unwrap_err();
        assert!(f.meldung.contains("tief"), "{f}");
        // Grenze selbst: 128 geht, 129 nicht.
        let ok = "[".repeat(128) + &"]".repeat(128);
        assert!(parse(&ok).is_ok());
        let zu_tief = "[".repeat(129) + &"]".repeat(129);
        assert!(parse(&zu_tief).is_err());
        // Knotenzahl: aus 1 Mio Ziffern wird EIN Wert (und die Grenze
        // ist die Zahl selbst, nicht die Speichermenge). Aus 600k
        // Elementen wird die Feldgrenze — benannt, kein Panic, kein
        // wachsender Speicher.
        assert!(parse(&"1".repeat(1_000_000)).is_err()); // > f64-Bereich
        let viele = "[".to_string() + &"1,".repeat(600_000) + "1]";
        let f = parse(&viele).unwrap_err();
        assert!(f.meldung.contains("Array"), "{f}");
    }

    #[test]
    fn zahl_mit_exponent() {
        assert_eq!(parse("1e3").unwrap(), Value::Zahl(1000.0));
        assert!(parse("01").is_err());
    }

    #[test]
    fn escapes() {
        assert_eq!(parse(r#""a\nb""#).unwrap(), Value::Text("a\nb".to_string()));
        assert_eq!(parse(r#""\u00e4""#).unwrap(), Value::Text("ä".to_string()));
        assert!(parse("\"\\q\"").is_err());
    }
}
