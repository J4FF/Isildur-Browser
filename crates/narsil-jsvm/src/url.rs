//! J23: `URL`, `URLSearchParams`, `TextEncoder`, `TextDecoder` — die
//! Web-APIs, die **echte** Seiten fast immer brauchen.
//!
//! # WARUM GERADE DIE (gemessen, nicht geraten)
//!
//! `grep` über die 120 Skriptdateien von 15 Seiten (J22, /tmp/opencode/j22):
//!
//! | API | Treffer | | API | Treffer |
//! |---|---|---|---|---|
//! | `location.` | 278 | | `localStorage` | 53 |
//! | `new URL` | 135 | | `URLSearchParams` | 50 |
//! | `navigator.` | 97 | | `CustomEvent` | 59 |
//! | `matchMedia` | 65 | | `document.cookie` | 22 |
//!
//! Ohne `URL`/`location`/`navigator` bricht **jede** dieser Seiten sofort.
//!
//! # DIE MESSREIHE
//!
//! `/tmp/opencode/url.txt` (66 Zeilen) ist die Fallliste; die Erwartungen
//! stammen aus `node 22` (`node /tmp/opencode/url_orakel.cjs`) und stehen
//! als `tests/j23_webapis.rs`. Jeder Test **behauptet ein Ergebnis**.
//!
//! # BEWUSSTE NAEHERUNGEN (mit node gemessen)
//!
//! * `URL` kennt nur `http`/`https` (die "special schemes"). Andere
//!   Schemata werden wie in node mit `origin === "null"` serialisiert.
//! * `location.assign/replace/reload` setzen die **Adresse** im Motor und
//!   melden dem Host (`location_meldung`); das **echte** Nachladen ist
//!   Host-Sache (eine Zeile in VERTRAG_JS.txt / PROBLEME.txt).

use crate::heap::{GVal, HObj, Heap, HeapRef};
use crate::interp::Engine;
use crate::interp::Stoerung;

// =====================================================================
// REINER TEIL: URL zerlegen und wieder zusammenbauen (ohne Heap)
// =====================================================================

/// Die Teile einer URL (WHATWG-Nah, ohne den Zustand im Heap).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct UrlTeile {
    pub schema: String,
    pub nutzer: String,
    pub passwort: String,
    pub host: String,
    /// Leer, wenn der Standardport der Schemagewiesen wurde.
    pub port: String,
    pub pfad: String,
    /// Ohne `?`.
    pub abfrage: String,
    /// Ohne `#`.
    pub fragment: String,
    /// Release 10.10.2026: OPAKER Pfad (`mailto:a@b`, `data:…`,
    /// `javascript:…`, `blob:…`) — ohne Authority, ohne `//`.
    pub opak: bool,
}

/// Standardport einer Schemagewiesen (gemessen: `https://a.b:443/x` -> `""`).
fn standardport(schema: &str) -> &'static str {
    match schema {
        "http" | "ws" => "80",
        "https" | "wss" => "443",
        "ftp" => "21",
        _ => "",
    }
}

/// Zeichen, die im **Pfad** percent-kodiert werden (WHATWG
/// "path percent-encode set"). GEMESSEN: `new URL('https://a.b/a b').pathname`
/// -> `/a%20b`, `new URL('https://a.b/ä').pathname` -> `/%C3%A4`.
fn pfad_kodieren(s: &str) -> String {
    let mut r = String::with_capacity(s.len());
    for b in s.as_bytes() {
        let c = *b;
        let kodieren = c < 0x21
            || c == b'"'
            || c == b'#'
            || c == b'<'
            || c == b'>'
            || c == b'?'
            || c == b'`'
            || c == b'{'
            || c == b'}'
            || c >= 0x7f;
        if kodieren {
            r.push('%');
            r.push_str(&format!("{:02X}", c));
        } else {
            r.push(c as char);
        }
    }
    r
}

/// Abfrage/Fragment percent-kodieren (WHATWG "query percent-encode set";
/// `?` wird dabei **nicht** kodiert — GEMESSEN: `?d=%20x` bleibt unberuehrt).
fn abfrage_kodieren(s: &str) -> String {
    let mut r = String::with_capacity(s.len());
    for b in s.as_bytes() {
        let c = *b;
        if c < 0x21 || c == b'"' || c == b'#' || c == b'<' || c == b'>' || c >= 0x7f {
            r.push('%');
            r.push_str(&format!("{:02X}", c));
        } else {
            r.push(c as char);
        }
    }
    r
}

/// `application/x-www-form-urlencoded`: Leerzeichen wird `+`.
pub(crate) fn form_kodieren(s: &str) -> String {
    let mut r = String::with_capacity(s.len());
    for b in s.as_bytes() {
        let c = *b;
        match c {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => {
                r.push(c as char)
            }
            b' ' => r.push('+'),
            _ => {
                r.push('%');
                r.push_str(&format!("{:02X}", c));
            }
        }
    }
    r
}

/// Die Umkehrung von [`form_kodieren`] (GEMESSEN: `new URLSearchParams('a=%20b')
/// .get('a')` -> `" b"` und `'+'` -> Leerzeichen).
pub(crate) fn form_dekodieren(s: &str) -> String {
    let b = s.as_bytes();
    let mut r: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => {
                r.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(w) => {
                        r.push(w);
                        i += 3;
                    }
                    Err(_) => {
                        r.push(b[i]);
                        i += 1;
                    }
                }
            }
            c => {
                r.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&r).into_owned()
}

/// Pfad aufloesen (`.`/`..`). GEMESSEN: `new URL('..','https://a.b/x/y/z')`
/// -> `/x/`, `new URL('./w','https://a.b/x/y/z')` -> `/x/y/w`,
/// `new URL('https://a.b/a/b/c/../../../z')` -> `/z`.
fn pfad_normieren(pfad: &str) -> String {
    if !pfad.starts_with('/') {
        return pfad.to_string();
    }
    // GEMESSEN (node):
    //   'https://a.b/a/b/c/../../../z' -> '/z'      (absolute Pfade auch!)
    //   '..' gegen '/x/y/z'             -> '/x/'    (`..` behaelt den Slash)
    //   '/a//b'                         -> '/a//b'  (leere Glieder bleiben)
    let schluss = pfad.ends_with('/') || pfad.ends_with("/.") || pfad.ends_with("/..");
    let mut teile: Vec<&str> = Vec::new();
    // GEMESSEN/GEFUNDEN: das **erste** Glied ist leer (der Pfad beginnt mit
    // `/`) und wird uebersprungen — mitgenommen ergaebe `/c` die beiden
    // Slashes `//c`. Leere Glieder **dazwischen** bleiben (Spec: `/a//b`
    // bleibt `/a//b`).
    for t in pfad.split('/').skip(1) {
        match t {
            "." => {}
            ".." => {
                teile.pop();
            }
            t => teile.push(t),
        }
    }
    let mut r = String::from("/");
    r.push_str(&teile.join("/"));
    if schluss && !r.ends_with('/') {
        r.push('/');
    }
    r
}

/// Release 10.10.2026: die "special schemes" (WHATWG URL §4.1).
fn ist_spezial(schema: &str) -> bool {
    matches!(schema, "http" | "https" | "ws" | "wss" | "ftp" | "file")
}

/// Release 10.10.2026: Laenge des Schemas, wenn `roh` mit `schema:`
/// beginnt (ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )).
fn schema_laenge(roh: &str) -> Option<usize> {
    let mut it = roh.char_indices();
    match it.next() {
        Some((_, c)) if c.is_ascii_alphabetic() => {}
        _ => return None,
    }
    for (i, c) in it {
        if c == ':' {
            return Some(i);
        }
        if !(c.is_ascii_alphanumeric() || "+-.".contains(c)) {
            return None;
        }
    }
    None
}

/// Release 10.10.2026: ein opaker Pfad kodiert nur Steuerzeichen und
/// Nicht-ASCII (C0 control percent-encode set) — `data:text/plain,a b`
/// behaelt das Leerzeichen (node).
fn opak_kodieren(s: &str) -> String {
    let mut r = String::with_capacity(s.len());
    for c in s.chars() {
        if (c as u32) < 0x20 || c as u32 == 0x7f || !c.is_ascii() {
            let mut b = [0u8; 4];
            for x in c.encode_utf8(&mut b).bytes() {
                r.push_str(&format!("%{x:02X}"));
            }
        } else {
            r.push(c);
        }
    }
    r
}

/// Eine absolute URL zerlegen. `basis` fuer relative Angaben.
///
/// Release 10.10.2026, GEMESSEN gegen node 22: vorher erkannte nur `://`
/// ein Schema — `mailto:`, `data:`, `blob:`, `about:` und `javascript:`
/// warfen „Invalid URL"; `https://a b.c/` und `http://a.b:99999/` galten
/// als gueltig. Jetzt: Schema nach §4.4 („scheme state"), opake Pfade fuer
/// die nicht-speziellen Schemata ohne `//`, Host- und Portpruefung.
pub(crate) fn url_parse(text: &str, basis: Option<&UrlTeile>) -> Result<UrlTeile, String> {
    // GEMESSEN (node): `new URL('HTTPS://A.B/C')` -> Schema und Host klein.
    let roh = text.trim_matches(|c: char| c.is_control() || c == ' ');
    let ohne_tab: String;
    let roh = if roh.contains(['\t', '\n', '\r']) {
        ohne_tab = roh.chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect();
        ohne_tab.as_str()
    } else {
        roh
    };
    let Some(i) = schema_laenge(roh) else {
        // Kein Schema: eine relative Angabe braucht eine Basis, sonst
        // ist es ein Fehler (GEMESSEN: `new URL('a b')` -> TypeError
        // „Invalid URL").
        let basis = basis.ok_or_else(|| "Invalid URL".to_string())?;
        if basis.opak {
            // Gegen einen opaken Pfad geht nur ein Fragment.
            if let Some(f) = roh.strip_prefix('#') {
                let mut t = basis.clone();
                t.fragment = abfrage_kodieren(f);
                return Ok(t);
            }
            return Err("Invalid URL".to_string());
        }
        let roh2 = if ist_spezial(&basis.schema) { roh.replace('\\', "/") } else { roh.to_string() };
        // Ein Protokoll- relatives `//host/pfad` erbt das Schema.
        if let Some(ohne) = rest_strip_scheme(&roh2) {
            return url_parse(&format!("{}://{}", basis.schema, ohne), Some(basis));
        }
        return url_relativ(&roh2, basis);
    };
    let schema = roh[..i].to_ascii_lowercase();
    let rest_roh = &roh[i + 1..];
    let spezial = ist_spezial(&schema);
    let rest_besitz: String;
    let rest: &str = if spezial {
        rest_besitz = rest_roh.replace('\\', "/");
        // `http:pfad` gegen eine Basis desselben Schemas ist relativ.
        if let Some(b) = basis
            && b.schema == schema
            && !rest_besitz.starts_with('/')
        {
            return url_relativ(&rest_besitz, b);
        }
        if schema == "file" {
            rest_besitz.strip_prefix("//").unwrap_or(&rest_besitz)
        } else {
            rest_besitz.trim_start_matches('/')
        }
    } else if let Some(o) = rest_roh.strip_prefix("//") {
        o
    } else {
        // Opaker Pfad.
        let (pfad, abfrage, fragment) = zerlege_rest(rest_roh);
        return Ok(UrlTeile {
            schema,
            pfad: opak_kodieren(&pfad),
            abfrage: abfrage_kodieren(&abfrage),
            fragment: abfrage_kodieren(&fragment),
            opak: true,
            ..UrlTeile::default()
        });
    };
    // authority bis zum ersten `/`, `?` oder `#`.
    let (mut authority, nach) = match rest.find(['/', '?', '#']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let mut nutzer = String::new();
    let mut passwort = String::new();
    if let Some(i) = authority.rfind('@') {
        let userinfo = &authority[..i];
        authority = &authority[i + 1..];
        match userinfo.split_once(':') {
            Some((u, p)) => {
                nutzer = u.to_string();
                passwort = p.to_string();
            }
            None => nutzer = userinfo.to_string(),
        }
    }
    let (host_roh, port_roh) = if authority.starts_with('[') {
        // IPv6: `[::1]` oder `[::1]:80`.
        match authority.find(']') {
            Some(e) => {
                let r = &authority[e + 1..];
                if r.is_empty() {
                    (&authority[..=e], "")
                } else if let Some(p) = r.strip_prefix(':') {
                    (&authority[..=e], p)
                } else {
                    return Err("Invalid URL".to_string());
                }
            }
            None => return Err("Invalid URL".to_string()),
        }
    } else {
        match authority.rfind(':') {
            Some(i) => (&authority[..i], &authority[i + 1..]),
            None => (authority, ""),
        }
    };
    if !port_roh.chars().all(|c| c.is_ascii_digit()) {
        return Err("Invalid URL".to_string());
    }
    let host = host_roh.to_ascii_lowercase();
    if !host.starts_with('[')
        && host.chars().any(|c| {
            c.is_control() || " #/:<>?@[\\]^|".contains(c)
        })
    {
        return Err("Invalid URL".to_string());
    }
    if host.is_empty() && spezial && schema != "file" {
        return Err("Invalid URL".to_string());
    }
    let port_zahl = if port_roh.is_empty() {
        String::new()
    } else {
        match port_roh.trim_start_matches('0').parse::<u32>() {
            Ok(n) if n <= 65535 => n.to_string(),
            Err(_) if port_roh.trim_start_matches('0').is_empty() => "0".to_string(),
            _ => return Err("Invalid URL".to_string()),
        }
    };
    let port = if port_zahl == standardport(&schema) {
        String::new()
    } else {
        port_zahl
    };
    // Rest: Pfad / Abfrage / Fragment.
    let (pfad_roh, abfrage, fragment) = zerlege_rest(nach);
    let pfad = if pfad_roh.is_empty() {
        // Nicht-spezielle Schemata behalten den leeren Pfad (`foo://h`).
        if spezial { "/".to_string() } else { String::new() }
    } else {
        pfad_normieren(&pfad_kodieren(&pfad_roh))
    };
    Ok(UrlTeile {
        schema,
        nutzer,
        passwort,
        host,
        port,
        pfad,
        abfrage: abfrage_kodieren(&abfrage),
        fragment: abfrage_kodieren(&fragment),
        opak: false,
    })
}

/// `//host/pfad` -> `host/pfad` (nur wenn die Angabe so beginnt).
fn rest_strip_scheme(s: &str) -> Option<&str> {
    s.strip_prefix("//")
}

/// Pfad/Abfrage/Fragment eines Restes trennen (nach der Authority).
/// GEMESSEN: `?` **gewinnt** gegen `#`, das Fragment beginnt am ersten `#`
/// nach der Abfrage.
fn zerlege_rest(rest: &str) -> (String, String, String) {
    match rest.find('#') {
        None => match rest.find('?') {
            None => (rest.to_string(), String::new(), String::new()),
            Some(i) => (
                rest[..i].to_string(),
                rest[i + 1..].to_string(),
                String::new(),
            ),
        },
        Some(h) => {
            let vor = &rest[..h];
            let fragment = rest[h + 1..].to_string();
            match vor.find('?') {
                None => (vor.to_string(), String::new(), fragment),
                Some(i) => (vor[..i].to_string(), vor[i + 1..].to_string(), fragment),
            }
        }
    }
}

/// Relative Angabe gegen eine Basis aufloesen (WHATWG „relative state").
fn url_relativ(roh: &str, basis: &UrlTeile) -> Result<UrlTeile, String> {
    let (ohne_query, query, fragment) = zerlege_rest(roh);
    let mut t = basis.clone();
    t.abfrage = abfrage_kodieren(&query);
    t.fragment = abfrage_kodieren(&fragment);
    if ohne_query.is_empty() {
        // GEMESSEN: `new URL('','https://a.b/x/y')` -> `/x/y` (der Pfad
        // bleibt), `?q=2` und `#f` aendern nur ihr Feld.
        return Ok(t);
    }
    if ohne_query.starts_with('/') {
        t.pfad = pfad_normieren(&pfad_kodieren(&ohne_query));
        return Ok(t);
    }
    // Verzeichnis der Basis (letztes `/` einschliessen).
    let kopf = match basis.pfad.rfind('/') {
        Some(i) => &basis.pfad[..=i],
        None => "/",
    };
    t.pfad = pfad_normieren(&pfad_kodieren(&format!("{kopf}{ohne_query}")));
    Ok(t)
}

/// Die URL wieder als Text (GEMESSEN: `href` und `toString()` sind gleich).
pub(crate) fn url_text(t: &UrlTeile) -> String {
    let mut r = String::new();
    r.push_str(&t.schema);
    r.push(':');
    if !t.opak && (!t.host.is_empty() || t.port.is_empty()) {
        r.push_str("//");
    }
    if !t.nutzer.is_empty() || !t.passwort.is_empty() {
        r.push_str(&t.nutzer);
        if !t.passwort.is_empty() {
            r.push(':');
            r.push_str(&t.passwort);
        }
        r.push('@');
    }
    r.push_str(&t.host);
    if !t.port.is_empty() {
        r.push(':');
        r.push_str(&t.port);
    }
    r.push_str(&t.pfad);
    if !t.abfrage.is_empty() {
        r.push('?');
        r.push_str(&t.abfrage);
    }
    if !t.fragment.is_empty() {
        r.push('#');
        r.push_str(&t.fragment);
    }
    r
}

/// `origin` (GEMESSEN: `https://a.b` — ohne Pfad, mit Port wenn nicht
/// Standard).
pub(crate) fn url_origin(t: &UrlTeile) -> String {
    // Release 10.10.2026: `blob:https://a.b/…` hat die Herkunft der inneren
    // URL (WHATWG URL §4.7).
    if t.schema == "blob" {
        if let Ok(innen) = url_parse(&t.pfad, None)
            && matches!(innen.schema.as_str(), "http" | "https")
        {
            return url_origin(&innen);
        }
        return "null".to_string();
    }
    if standardport(&t.schema).is_empty() {
        return "null".to_string();
    }
    let mut r = format!("{}://{}", t.schema, t.host);
    if !t.port.is_empty() {
        r.push(':');
        r.push_str(&t.port);
    }
    r
}

// =====================================================================
// PARAMETER: Liste aus Text, Liste als Text
// =====================================================================

/// `a=1&b=2` (mit fuehrendem `?`) -> Paare. GEMESSEN: `?a=1` und `a` ohne
/// `=` sind beide erlaubt (dann ist der Wert leer).
pub(crate) fn params_aus_text(text: &str) -> Vec<(String, String)> {
    let t = text.strip_prefix('?').unwrap_or(text);
    if t.is_empty() {
        return Vec::new();
    }
    t.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (form_dekodieren(k), form_dekodieren(v)),
            None => (form_dekodieren(p), String::new()),
        })
        .collect()
}

/// Paare -> `a=1&b=2` (GEMESSEN: `toString` ohne fuehrendes `?`).
pub(crate) fn params_als_text(paare: &[(String, String)]) -> String {
    paare
        .iter()
        .map(|(k, v)| format!("{}={}", form_kodieren(k), form_kodieren(v)))
        .collect::<Vec<String>>()
        .join("&")
}
// =====================================================================
// MOTOR-ANBINDUNG (der Zustand liegt als versteckte Felder im Objekt)
// =====================================================================

/// Das URL-Objekt haelt nur zwei Felder: `__href` (den **Text**) und
/// `__params` (den lebenden `URLSearchParams`-Korb, wenn er schon geholt
/// wurde). GEMESSEN/ENTSCHIEDEN: die sieben Einzelteile NICHT zu speichern —
/// `url_parse` ist billig (~100 ns), und ein **eine** Feld kann nicht
/// auseinanderlaufen. Die Lebendigkeit von `searchParams` (Spec: `url.search`
/// liest aus dem Korb) ist ueber die Richtung geregelt: `search` liest den
/// Korb, wenn er existiert, sonst `__href`.
// Release 15.10.2026: private names, so `Reflect.ownKeys(url)` stays empty
// as in browsers (MEASURED before: ["__href"], params ["__paare"]).
const F_URL_HREF: &str = "#es_href";
const F_URL_PARAMS: &str = "#es_params";
/// Paare als flaches Feld: `k0, v0, k1, v1, …`
const F_PARAMS_PAARE: &str = "#es_paare";

impl Engine {
    /// `__href` lesen (mit `_`-Praefix, damit kein Skript daran kommt).
    fn url_href(&self, this: GVal) -> String {
        let GVal::Ref(r) = this else {
            return String::new();
        };
        match self.eigene_lese(r, F_URL_HREF) {
            Some(GVal::Ref(t)) => match self.heap.holen(t) {
                HObj::Zeichenkette(s) => s.clone(),
                _ => String::new(),
            },
            _ => String::new(),
        }
    }

    /// `__href` schreiben.
    fn url_href_setzen(&mut self, this: GVal, wert: &str) -> Result<(), Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(());
        };
        let v = self.text_wert(wert).map_err(Stoerung::Fehler)?;
        self.versteckt_setzen(r, F_URL_HREF, v)
    }

    /// Die Teile eines URL-Objekts (aus `__href`).
    fn url_teile_von(&mut self, this: GVal) -> Result<UrlTeile, Stoerung> {
        let t = self.url_href(this);
        url_parse(&t, None).map_err(|m| Stoerung::Wurf(self.fehler_wert("TypeError", &m)))
    }

    /// Die Abfrage des URL-Objekts — aus dem Korb, wenn es ihn gibt
    /// (GEMESSEN: `url.search` folgt `searchParams`).
    fn url_abfrage_von(&mut self, this: GVal) -> Result<String, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(String::new());
        };
        if let Some(GVal::Ref(k)) = self.eigene_lese(r, F_URL_PARAMS) {
            return Ok(self.params_text(k));
        }
        Ok(self
            .url_teile_von(this)
            .map(|t| t.abfrage)
            .unwrap_or_default())
    }

    /// `href` **mit** Korb-Abfrage (der lebende Weg).
    fn url_href_aktuell(&mut self, this: GVal) -> Result<String, Stoerung> {
        let mut t = self.url_teile_von(this)?;
        t.abfrage = self.url_abfrage_von(this)?;
        Ok(url_text(&t))
    }

    /// Ein `URLSearchParams` bauen (Objekt mit flachem Paar-Feld).
    fn params_anlegen(&mut self, paare: Vec<(String, String)>) -> Result<GVal, Stoerung> {
        let feld = self.neues_feld().map_err(Stoerung::Fehler)?;
        let mut flach: Vec<GVal> = Vec::with_capacity(paare.len() * 2);
        for (k, v) in &paare {
            flach.push(self.text_wert(k).map_err(Stoerung::Fehler)?);
            flach.push(self.text_wert(v).map_err(Stoerung::Fehler)?);
        }
        for w in flach {
            self.feld_anhaengen(feld, w)?;
        }
        let obj = HObj::Gegenstand {
            proto: Some(self.proto_params),
            eigenschaften: Default::default(),
            sperre: crate::heap::Sperre::Offen,
            reihenfolge: Vec::new(),
        };
        let kosten = Heap::kosten_fuer(&obj);
        self.vor_allok(kosten).map_err(Stoerung::Fehler)?;
        let r = self.belege(obj, kosten);
        self.versteckt_setzen(r, F_PARAMS_PAARE, GVal::Ref(feld))?;
        Ok(GVal::Ref(r))
    }

    /// Die Paare aus einem Korb lesen.
    fn params_paare(&self, r: HeapRef) -> Vec<(String, String)> {
        let Some(GVal::Ref(f)) = self.eigene_lese(r, F_PARAMS_PAARE) else {
            return Vec::new();
        };
        let elemente = match self.heap.holen(f) {
            HObj::Feld { elemente, .. } => elemente.clone(),
            _ => return Vec::new(),
        };
        let text = |v: &GVal| match v {
            GVal::Ref(t) => match self.heap.holen(*t) {
                HObj::Zeichenkette(s) => s.clone(),
                _ => String::new(),
            },
            GVal::Zahl(z) => crate::wert::zahl_text(*z),
            _ => String::new(),
        };
        let mut r = Vec::with_capacity(elemente.len() / 2);
        for paar in elemente.chunks(2) {
            r.push((text(&paar[0]), text(&paar[1])));
        }
        r
    }

    /// Die Paare in einen Korb schreiben (flaches Feld neu belegen).
    fn params_paare_setzen(
        &mut self,
        r: HeapRef,
        paare: &[(String, String)],
    ) -> Result<(), Stoerung> {
        let feld = self.neues_feld().map_err(Stoerung::Fehler)?;
        for (k, v) in paare {
            let a = self.text_wert(k).map_err(Stoerung::Fehler)?;
            self.feld_anhaengen(feld, a)?;
            let b = self.text_wert(v).map_err(Stoerung::Fehler)?;
            self.feld_anhaengen(feld, b)?;
        }
        self.versteckt_setzen(r, F_PARAMS_PAARE, GVal::Ref(feld))
    }

    /// `URLSearchParams#toString`.
    fn params_text(&mut self, r: HeapRef) -> String {
        params_als_text(&self.params_paare(r))
    }

    /// Der lebende Korb eines URL-Objekts (bei Bedarf anlegen).
    fn url_params_holen(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(GVal::Undefiniert);
        };
        if let Some(v) = self.eigene_lese(r, F_URL_PARAMS) {
            return Ok(v);
        }
        let abfrage = self
            .url_teile_von(this)
            .map(|t| t.abfrage)
            .unwrap_or_default();
        let korb = self.params_anlegen(params_aus_text(&abfrage))?;
        self.versteckt_setzen(r, F_URL_PARAMS, korb)?;
        Ok(korb)
    }

    /// Ein Feld (Array) befuellen — der Motor hat keinen Rust-Helfer
    /// dafuer (GEMESSEN: nur `dom.rs` haengt selbst an).
    pub(crate) fn feld_anhaengen(&mut self, feld: HeapRef, wert: GVal) -> Result<(), Stoerung> {
        let kosten = crate::heap::KOSTEN_EINTRAG;
        self.vor_allok(kosten).map_err(Stoerung::Fehler)?;
        if let HObj::Feld { elemente, .. } = self.heap.holen_mut(feld) {
            elemente.push(wert);
            self.heap.kosten_verbuchen(kosten);
        }
        Ok(())
    }

    /// Ein Feld aus Texten bauen (`keys()`/`values()`).
    fn array_aus_texten(&mut self, werte: Vec<String>) -> Result<GVal, Stoerung> {
        let feld = self.neues_feld().map_err(Stoerung::Fehler)?;
        for w in &werte {
            let v = self.text_wert(w).map_err(Stoerung::Fehler)?;
            self.feld_anhaengen(feld, v)?;
        }
        Ok(GVal::Ref(feld))
    }

    // --- Natives: URL ------------------------------------------------

    pub(crate) fn n_url_ctor(
        &mut self,
        _this: GVal,
        args: &[GVal],
        _als_konstruktor: bool,
    ) -> Result<GVal, Stoerung> {
        let eingabe = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        // GEMESSEN: `new URL('x', basis)` — die Basis darf ein **Text** sein
        // (node akzeptiert beides). Fehlt sie, gilt die Herkunft der Seite.
        // Release 10.10.2026: ohne Basis wird NICHT gegen die Herkunft
        // aufgeloest (Chrome/node: `new URL('/x')` -> TypeError) — Pruefungen
        // wie `try { new URL(s) } catch { relativ }` brauchen den Wurf.
        // Eine ungueltige Basis ist ein Fehler, nicht „keine Basis".
        let basis = match self.arg(args, 1) {
            GVal::Undefiniert => None,
            v => {
                let t = self.zu_text(v).unwrap_or_default();
                match url_parse(&t, None) {
                    Ok(b) => Some(b),
                    Err(m) => return Err(Stoerung::Wurf(self.fehler_wert("TypeError", &m))),
                }
            }
        };
        match url_parse(&eingabe, basis.as_ref()) {
            Ok(t) => {
                let obj = HObj::Gegenstand {
                    proto: Some(self.proto_url),
                    eigenschaften: Default::default(),
                    sperre: crate::heap::Sperre::Offen,
                    reihenfolge: Vec::new(),
                };
                let kosten = Heap::kosten_fuer(&obj);
                self.vor_allok(kosten).map_err(Stoerung::Fehler)?;
                let r = self.belege(obj, kosten);
                let out = GVal::Ref(r);
                self.url_href_setzen(out, &url_text(&t))?;
                Ok(out)
            }
            Err(m) => Err(Stoerung::Wurf(self.fehler_wert("TypeError", &m))),
        }
    }

    /// `URL.canParse` (statisch).
    pub(crate) fn n_url_kann(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let eingabe = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let basis = match self.arg(args, 1) {
            GVal::Undefiniert => None,
            v => {
                let t = self.zu_text(v).unwrap_or_default();
                match url_parse(&t, None) {
                    Ok(b) => Some(b),
                    Err(_) => return Ok(GVal::Bool(false)),
                }
            }
        };
        Ok(GVal::Bool(url_parse(&eingabe, basis.as_ref()).is_ok()))
    }

    /// Die Herkunft der Seite als URL-Teile (`new URL(rel)` ohne Basis
    /// braucht sie — GEMESSEN: die Seiten-Skripte rufen `new URL(x)`
    /// ueberall **ohne** zweite Argument auf).
    pub(crate) fn herkunft_als_url(&self) -> Option<UrlTeile> {
        let h = self.herkunft.as_ref()?;
        let port = if (h.schema == "http" && h.port == 80) || (h.schema == "https" && h.port == 443)
        {
            String::new()
        } else {
            h.port.to_string()
        };
        Some(UrlTeile {
            schema: h.schema.clone(),
            host: h.host.clone(),
            port,
            pfad: "/".to_string(),
            ..UrlTeile::default()
        })
    }

    pub(crate) fn n_url_text(&mut self, this: GVal, _args: &[GVal]) -> Result<GVal, Stoerung> {
        let t = self.url_href_aktuell(this)?;
        self.text_wert(&t).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_url_schema(&mut self, this: GVal, _args: &[GVal]) -> Result<GVal, Stoerung> {
        let t = self.url_teile_von(this)?;
        self.text_wert(&format!("{}:", t.schema))
            .map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_url_host(&mut self, this: GVal, _args: &[GVal]) -> Result<GVal, Stoerung> {
        let t = self.url_teile_von(this)?;
        let s = if t.port.is_empty() {
            t.host.clone()
        } else {
            format!("{}:{}", t.host, t.port)
        };
        self.text_wert(&s).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_url_hostname(&mut self, this: GVal, _args: &[GVal]) -> Result<GVal, Stoerung> {
        let t = self.url_teile_von(this)?;
        self.text_wert(&t.host).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_url_port(&mut self, this: GVal, _args: &[GVal]) -> Result<GVal, Stoerung> {
        let t = self.url_teile_von(this)?;
        self.text_wert(&t.port).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_url_pfad(&mut self, this: GVal, _args: &[GVal]) -> Result<GVal, Stoerung> {
        let t = self.url_teile_von(this)?;
        self.text_wert(&t.pfad).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_url_abfrage(&mut self, this: GVal, _args: &[GVal]) -> Result<GVal, Stoerung> {
        let a = self.url_abfrage_von(this)?;
        let s = if a.is_empty() {
            String::new()
        } else {
            format!("?{a}")
        };
        self.text_wert(&s).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_url_fragment(&mut self, this: GVal, _args: &[GVal]) -> Result<GVal, Stoerung> {
        let t = self.url_teile_von(this)?;
        let s = if t.fragment.is_empty() {
            String::new()
        } else {
            format!("#{}", t.fragment)
        };
        self.text_wert(&s).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_url_origin(&mut self, this: GVal, _args: &[GVal]) -> Result<GVal, Stoerung> {
        let t = self.url_teile_von(this)?;
        self.text_wert(&url_origin(&t)).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_url_nutzer(&mut self, this: GVal, _args: &[GVal]) -> Result<GVal, Stoerung> {
        let t = self.url_teile_von(this)?;
        self.text_wert(&t.nutzer).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_url_passwort(&mut self, this: GVal, _args: &[GVal]) -> Result<GVal, Stoerung> {
        let t = self.url_teile_von(this)?;
        self.text_wert(&t.passwort).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_url_params(&mut self, this: GVal, _args: &[GVal]) -> Result<GVal, Stoerung> {
        self.url_params_holen(this)
    }

    /// `url.hash = '…'` / `url.search = '…'` (GEMESSEN: beide setzen die
    /// Adresse, ohne Neuladen — `history.pushState` macht dasselbe).
    pub(crate) fn n_url_setzen(
        &mut self,
        this: GVal,
        args: &[GVal],
        art: crate::builtin::UrlFeld,
    ) -> Result<GVal, Stoerung> {
        let wert = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let mut t = self.url_teile_von(this)?;
        match art {
            crate::builtin::UrlFeld::Hash => {
                t.fragment = abfrage_kodieren(wert.strip_prefix('#').unwrap_or(&wert));
            }
            crate::builtin::UrlFeld::Search => {
                // Der Korb folgt der Adresse (hier ist es umgekehrt).
                let text = wert.strip_prefix('?').unwrap_or(&wert).to_string();
                if let GVal::Ref(r) = this
                    && let Some(GVal::Ref(k)) = self.eigene_lese(r, F_URL_PARAMS)
                {
                    self.params_paare_setzen(k, &params_aus_text(&text))?;
                }
                t.abfrage = text;
            }
            crate::builtin::UrlFeld::Href => {
                let neu = url_parse(&wert, None)
                    .map_err(|m| Stoerung::Wurf(self.fehler_wert("TypeError", &m)))?;
                t = neu;
                if let GVal::Ref(r) = this {
                    self.versteckt_setzen(r, F_URL_PARAMS, GVal::Undefiniert)?;
                }
            }
            crate::builtin::UrlFeld::Pfad => {
                let mit_slash = if wert.starts_with('/') {
                    wert
                } else {
                    format!("/{}", wert.trim_start_matches('/'))
                };
                t.pfad = pfad_normieren(&pfad_kodieren(&mit_slash));
            }
            crate::builtin::UrlFeld::Host => {
                t.host = wert.to_ascii_lowercase();
                t.port = String::new();
            }
        }
        self.url_href_setzen(this, &url_text(&t))?;
        Ok(GVal::Undefiniert)
    }

    // --- Natives: URLSearchParams -------------------------------------

    pub(crate) fn n_params_ctor(
        &mut self,
        _this: GVal,
        args: &[GVal],
        _als_konstruktor: bool,
    ) -> Result<GVal, Stoerung> {
        // GEMESSEN/GEFUNDEN: `GVal::Ref` allein sagt nichts — ein **Text**
        // ist auch ein `Ref` (`HObj::Zeichenkette`). Ohne diese Zeile
        // landete `new URLSearchParams('a=1')` im Objekt-Zweig, lief ueber
        // `eigene_schluessel` (leer) und lieferte `get('a') === null`.
        let paare = match self.arg(args, 0) {
            GVal::Undefiniert => Vec::new(),
            GVal::Ref(r) if self.ist_zeichenkette(r) => {
                let t = self.text_von(GVal::Ref(r));
                params_aus_text(&t)
            }
            GVal::Ref(r) if self.ist_params(r) => self.params_paare(r),
            GVal::Ref(r) if self.ist_feld(r) || self.hat_iterator(GVal::Ref(r))? => {
                // URL-Standard §6.2 Schritt 1: ein Objekt MIT `@@iterator` ist
                // eine Folge von Paaren — `[['a','1'],['b','2']]`, eine `Map`,
                // `Object.entries(…)`. Jedes Paar hat GENAU zwei Teile.
                //
                // Release 10.10.2026, GEMESSEN vorher (heise.de): hier wurden die
                // ELEMENTE je zwei zu einem Paar gefasst — aus
                // `[['width',336],['q',50]]` wurde `width,336=q,50`, die
                // Bildadressen der Startseite waren unbrauchbar.
                let marke = self.wurzel_merke();
                let erg = (|| {
                    let roh = self.iter_werte(GVal::Ref(r))?;
                    self.wurzeln.extend(roh.iter().copied());
                    let mut v = Vec::new();
                    for paar in roh {
                        let teile = self.iter_werte(paar)?;
                        if teile.len() != 2 {
                            return Err(Stoerung::Wurf(self.fehler_wert(
                                "TypeError",
                                "Failed to construct 'URLSearchParams': Sequence initializer must only contain pair elements",
                            )));
                        }
                        self.wurzeln.extend(teile.iter().copied());
                        let k = self.zu_text(teile[0])?;
                        let w = self.zu_text(teile[1])?;
                        v.push((k, w));
                    }
                    Ok(v)
                })();
                self.wurzel_zurueck(marke);
                erg?
            }
            GVal::Ref(r) => {
                // GEMESSEN: `new URLSearchParams({x:'1',y:'2'})` — ein
                // **Objekt** (eigene, aufzaehlbare Eigenschaften).
                // `eigene_schluessel` (builtin.rs) nimmt einen `GVal`.
                let schluessel = self.eigene_schluessel(GVal::Ref(r)).unwrap_or_default();
                let mut v = Vec::new();
                for k in schluessel {
                    // GEMESSEN: die Werte kommen ueber die **normale**
                    // Aufloesung (Getter zaehlen mit).
                    let wert = self.hole(GVal::Ref(r), &k)?;
                    let w = self.zu_text(wert).unwrap_or_default();
                    v.push((k, w));
                }
                v
            }
            v => {
                let t = self.zu_text(v).unwrap_or_default();
                params_aus_text(&t)
            }
        };
        self.params_anlegen(paare)
    }

    /// Hat `v` eine aufrufbare `@@iterator`-Methode?
    fn hat_iterator(&mut self, v: GVal) -> Result<bool, Stoerung> {
        let sym = self.n_symbol_bekannt(GVal::Undefiniert, &[], crate::builtin::Nativ::SymIterator)?;
        let schluessel = self.schluessel_text(sym)?;
        let meth = self.hole(v, &schluessel)?;
        Ok(self.ist_aufrufbar(meth))
    }

    /// Ist der Slot eine Zeichenkette? (Ein Text ist `GVal::Ref`!)
    fn ist_zeichenkette(&self, r: HeapRef) -> bool {
        matches!(self.heap.holen(r), HObj::Zeichenkette(_))
    }

    fn ist_params(&self, r: HeapRef) -> bool {
        matches!(
            self.heap.holen(r),
            HObj::Gegenstand {
                proto: Some(p),
                ..
            } if *p == self.proto_params
        )
    }

    pub(crate) fn ist_feld(&self, r: HeapRef) -> bool {
        matches!(self.heap.holen(r), HObj::Feld { .. })
    }

    pub(crate) fn n_params_get(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(GVal::Null);
        };
        let k = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        Ok(
            match self.params_paare(r).into_iter().find(|(a, _)| *a == k) {
                Some((_, v)) => self.text_wert(&v).map_err(Stoerung::Fehler)?,
                None => GVal::Null,
            },
        )
    }

    pub(crate) fn n_params_get_all(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(GVal::Undefiniert);
        };
        let k = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let paare = self.params_paare(r);
        self.array_aus_texten(
            paare
                .into_iter()
                .filter(|(a, _)| *a == k)
                .map(|(_, v)| v)
                .collect(),
        )
    }

    pub(crate) fn n_params_hat(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(GVal::Bool(false));
        };
        let k = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        // Release 10.10.2026 (URL-Standard): ein zweites Argument prueft
        // auch den WERT (`has(name, value)`).
        let wert = match self.arg(args, 1) {
            GVal::Undefiniert => None,
            w => Some(self.zu_text(w).unwrap_or_default()),
        };
        Ok(GVal::Bool(
            self.params_paare(r).iter().any(|(a, w)| *a == k && wert.as_ref().is_none_or(|x| x == w)),
        ))
    }

    pub(crate) fn n_params_set(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(GVal::Undefiniert);
        };
        let k = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let v = self.zu_text(self.arg(args, 1)).unwrap_or_default();
        let mut paare = self.params_paare(r);
        // GEMESSEN (Spec): `set` ersetzt den **ersten** Treffer und loescht die
        // weiteren (Reihenfolge bleibt).
        let mut ersetzt = false;
        paare.retain_mut(|(a, w)| {
            if *a == k {
                if !ersetzt {
                    *w = v.clone();
                    ersetzt = true;
                    true
                } else {
                    false
                }
            } else {
                true
            }
        });
        if !ersetzt {
            paare.push((k, v));
        }
        self.params_paare_setzen(r, &paare)?;
        Ok(GVal::Undefiniert)
    }

    pub(crate) fn n_params_anhaengen(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(GVal::Undefiniert);
        };
        let k = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let v = self.zu_text(self.arg(args, 1)).unwrap_or_default();
        let mut paare = self.params_paare(r);
        paare.push((k, v));
        self.params_paare_setzen(r, &paare)?;
        Ok(GVal::Undefiniert)
    }

    pub(crate) fn n_params_loeschen(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(GVal::Undefiniert);
        };
        let k = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        // Release 10.10.2026 (URL-Standard): `delete(name, value)` loescht nur
        // die Paare mit diesem Wert.
        let wert = match self.arg(args, 1) {
            GVal::Undefiniert => None,
            w => Some(self.zu_text(w).unwrap_or_default()),
        };
        let mut paare = self.params_paare(r);
        paare.retain(|(a, w)| !(*a == k && wert.as_ref().is_none_or(|x| x == w)));
        self.params_paare_setzen(r, &paare)?;
        Ok(GVal::Undefiniert)
    }

    /// `URLSearchParams#sort` (URL-Standard §6.2): STABIL nach den
    /// UTF-16-Codeeinheiten des Namens.
    ///
    /// Release 10.10.2026, GEMESSEN vorher (heise.de): die Bildadressen der
    /// `<a-img>`-Elemente entstehen mit `new URLSearchParams(..).sort()` —
    /// ohne `sort` warf das ein TypeError, kein Bild der Startseite lud.
    pub(crate) fn n_params_sortieren(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(GVal::Undefiniert);
        };
        let mut paare = self.params_paare(r);
        paare.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
        self.params_paare_setzen(r, &paare)?;
        Ok(GVal::Undefiniert)
    }

    pub(crate) fn n_params_text(&mut self, this: GVal, _args: &[GVal]) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(GVal::Undefiniert);
        };
        let t = self.params_text(r);
        self.text_wert(&t).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_params_groesse(
        &mut self,
        this: GVal,
        _args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(GVal::Zahl(0.0));
        };
        Ok(GVal::Zahl(self.params_paare(r).len() as f64))
    }

    pub(crate) fn n_params_schluessel(
        &mut self,
        this: GVal,
        _args: &[GVal],
        art: crate::builtin::ParamsLauf,
    ) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(GVal::Undefiniert);
        };
        let paare = self.params_paare(r);
        let feld = match art {
            crate::builtin::ParamsLauf::Schluessel => {
                self.array_aus_texten(paare.into_iter().map(|(k, _)| k).collect())?
            }
            crate::builtin::ParamsLauf::Werte => {
                self.array_aus_texten(paare.into_iter().map(|(_, v)| v).collect())?
            }
            crate::builtin::ParamsLauf::Eintraege => {
                let feld = self.neues_feld().map_err(Stoerung::Fehler)?;
                for (k, v) in paare {
                    let p = self.neues_feld().map_err(Stoerung::Fehler)?;
                    let a = self.text_wert(&k).map_err(Stoerung::Fehler)?;
                    self.feld_anhaengen(p, a)?;
                    let b = self.text_wert(&v).map_err(Stoerung::Fehler)?;
                    self.feld_anhaengen(p, b)?;
                    self.feld_anhaengen(feld, GVal::Ref(p))?;
                }
                GVal::Ref(feld)
            }
        };
        // WebIDL §3.7.9 („iterable"): `keys`/`values`/`entries` liefern einen
        // ITERATOR, kein Feld. Release 10.10.2026, GEMESSEN vorher (WPT
        // url/urlsearchparams-*): `params.entries().next()` war undefined,
        // `for (… of params)` warf „nicht iterierbar".
        let GVal::Ref(f) = feld else { return Ok(feld) };
        let marke = self.wurzel_merke();
        self.wurzeln.push(feld);
        let it = self.feld_zum_iterator(f);
        self.wurzel_zurueck(marke);
        it
    }

    pub(crate) fn n_params_fuer_alle(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let GVal::Ref(r) = this else {
            return Ok(GVal::Undefiniert);
        };
        let paare = self.params_paare(r);
        let f = self.arg(args, 0);
        // Release 12.10.2026: WebIDL §3.7.9 `forEach(callback(value, key,
        // parent), thisArg)` — erst der WERT. GEMESSEN vorher (node 22):
        // `new URLSearchParams('a=1').forEach((v, k) => …)` gab v = "a".
        let this_k = self.arg(args, 1);
        for (k, v) in paare {
            let a = self.text_wert(&k).map_err(Stoerung::Fehler)?;
            let b = self.text_wert(&v).map_err(Stoerung::Fehler)?;
            self.aufruf_synchron(f, this_k, &[b, a, this], false)?;
        }
        Ok(GVal::Undefiniert)
    }

    // --- Natives: TextEncoder / TextDecoder ---------------------------

    /// `new TextEncoder()` / `new TextDecoder()`: ein Objekt mit dem
    /// `prototype` des Konstruktors — dort liegen `encode`/`decode`.
    ///
    /// GEMESSEN/GEFUNDEN: `TextEncoderCtor => Ok(this)` gab ein Objekt mit
    /// `Object.prototype` zurueck, und `new TextEncoder().encode` war
    /// darum `undefined` („kein Zugriff auf Undefiniert").
    pub(crate) fn n_text_encoder_ctor(&mut self, ctor: crate::heap::HeapRef) -> Result<GVal, Stoerung> {
        let proto = match self.hole(GVal::Ref(ctor), "prototype")? {
            GVal::Ref(p) => p,
            _ => self.proto_objekt,
        };
        let o = self.neues_objekt(Some(proto)).map_err(Stoerung::Fehler)?;
        // Release 11.10.2026: `encoding` ist immer "utf-8" (Encoding §8.2).
        let name = self.text_wert("utf-8").map_err(Stoerung::Fehler)?;
        self.setze(GVal::Ref(o), "encoding", name)?;
        Ok(GVal::Ref(o))
    }

    /// Release 11.10.2026: `new TextDecoder(label, { fatal, ignoreBOM })`
    /// (Encoding §8.1). Ein unbekanntes Etikett und die Ersatz-Kodierung sind
    /// ein RangeError. GEMESSEN vorher (WPT `api-invalid-label`, 3 421
    /// Teiltests): jedes Etikett wurde angenommen, `encoding` fehlte, und es
    /// wurde immer UTF-8 dekodiert.
    pub(crate) fn n_text_decoder_ctor(&mut self, ctor: crate::heap::HeapRef, args: &[GVal]) -> Result<GVal, Stoerung> {
        use narsil_fmt_html::kodierung::{Kodierung, kodierung_aus_label};
        let label = match self.arg(args, 0) {
            GVal::Undefiniert => String::from("utf-8"),
            v => self.zu_text(v)?,
        };
        let kod = match kodierung_aus_label(&label) {
            Some(k) if k != Kodierung::Ersatz => k,
            _ => {
                return Err(Stoerung::Wurf(self.fehler_wert(
                    "RangeError",
                    &format!("Failed to construct 'TextDecoder': The encoding label provided ('{label}') is invalid."),
                )));
            }
        };
        let opt = self.arg(args, 1);
        let (fatal, ohne_bom) = match opt {
            GVal::Ref(_) => {
                let f = self.hole(opt, "fatal")?;
                let b = self.hole(opt, "ignoreBOM")?;
                (self.zu_bool(f), self.zu_bool(b))
            }
            _ => (false, false),
        };
        let proto = match self.hole(GVal::Ref(ctor), "prototype")? {
            GVal::Ref(p) => p,
            _ => self.proto_objekt,
        };
        let o = self.neues_objekt(Some(proto)).map_err(Stoerung::Fehler)?;
        let name = self.text_wert(kod.name()).map_err(Stoerung::Fehler)?;
        self.setze(GVal::Ref(o), "encoding", name)?;
        self.setze(GVal::Ref(o), "fatal", GVal::Bool(fatal))?;
        self.setze(GVal::Ref(o), "ignoreBOM", GVal::Bool(ohne_bom))?;
        self.versteckt_setzen(o, "__kodierung", name)?;
        let leer = self.text_wert("").map_err(Stoerung::Fehler)?;
        self.versteckt_setzen(o, "__rest", leer)?;
        self.versteckt_setzen(o, "__bom_offen", GVal::Bool(true))?;
        Ok(GVal::Ref(o))
    }

    /// Release 11.10.2026: `encodeInto(text, ziel)` (Encoding §8.2) — so viel
    /// UTF-8, wie ganz hineinpasst; `read` zaehlt UTF-16-Einheiten.
    pub(crate) fn n_text_encoder_encode_into(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let s = match self.arg(args, 0) {
            GVal::Undefiniert => String::new(),
            v => self.zu_text(v)?,
        };
        let GVal::Ref(ziel) = self.arg(args, 1) else {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "encodeInto: Ziel ist kein Uint8Array")));
        };
        let platz = match self.heap.typedarray_bytes_mut(ziel) {
            Some(b) => b.len(),
            None => return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "encodeInto: Ziel ist kein Uint8Array"))),
        };
        let (mut gelesen, mut geschrieben) = (0usize, 0usize);
        let mut puffer = Vec::with_capacity(platz.min(s.len()));
        for c in s.chars() {
            let mut b = [0u8; 4];
            let t = c.encode_utf8(&mut b);
            if geschrieben + t.len() > platz {
                break;
            }
            puffer.extend_from_slice(t.as_bytes());
            geschrieben += t.len();
            gelesen += c.len_utf16();
        }
        if let Some(bytes) = self.heap.typedarray_bytes_mut(ziel) {
            bytes[..geschrieben].copy_from_slice(&puffer);
        }
        let o = self.neues_objekt(Some(self.proto_objekt)).map_err(Stoerung::Fehler)?;
        self.setze(GVal::Ref(o), "read", GVal::Zahl(gelesen as f64))?;
        self.setze(GVal::Ref(o), "written", GVal::Zahl(geschrieben as f64))?;
        Ok(GVal::Ref(o))
    }

    pub(crate) fn n_text_encoder_encode(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let input = self.arg(args, 0);
        let s = if matches!(input, GVal::Undefiniert) { String::new() } else { self.zu_text(input)? };
        let ctor = self.hole(GVal::Ref(self.global_objekt), "Uint8Array")?;
        let GVal::Ref(ctor) = ctor else { return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Uint8Array fehlt"))); };
        let result = self.n_typedarray_ctor_uint8(ctor, &[GVal::Zahl(s.len() as f64)])?;
        if let GVal::Ref(r) = result {
            if let Some(bytes) = self.heap.typedarray_bytes_mut(r) { bytes.copy_from_slice(s.as_bytes()); }
        }
        Ok(result)
    }

    pub(crate) fn n_text_decoder_decode(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let input = self.arg(args, 0);
        let bytes = match input {
            GVal::Undefiniert => Vec::new(),
            GVal::Ref(r) => match self.heap.holen(r) {
                HObj::BytePuffer { daten, .. } => daten.clone(),
                HObj::TypedArray { puffer, versatz, laenge, art, .. } => {
                    let HObj::BytePuffer { daten, .. } = self.heap.holen(*puffer) else { unreachable!() };
                    daten[*versatz..*versatz + *laenge * art.bytes()].to_vec()
                }
                HObj::Gegenstand { eigenschaften, proto, .. } if *proto == Some(self.proto_dataview) => {
                    let Some(GVal::Ref(buffer)) = eigenschaften.get("#es_dv_puffer").map(|e| e.wert) else { unreachable!() };
                    let Some(GVal::Zahl(offset)) = eigenschaften.get("#es_dv_versatz").map(|e| e.wert) else { unreachable!() };
                    let Some(GVal::Zahl(len)) = eigenschaften.get("#es_dv_umfang").map(|e| e.wert) else { unreachable!() };
                    let HObj::BytePuffer { daten, .. } = self.heap.holen(buffer) else { unreachable!() };
                    daten[offset as usize..offset as usize + len as usize].to_vec()
                }
                _ => return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "TextDecoder braucht BufferSource"))),
            },
            _ => return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "TextDecoder braucht BufferSource"))),
        };
        // Release 11.10.2026: die Kodierung des Dekodierers, `fatal`,
        // `ignoreBOM` und `{ stream: true }` (Encoding §8.1.3). Ein Rest
        // (unvollstaendiges Zeichen am Ende eines Stroms) wartet als
        // Latin-1-Text in `__rest`.
        use narsil_fmt_html::kodierung::{Kodierung, dekodiere};
        let GVal::Ref(ich) = this else {
            let text = String::from_utf8_lossy(&bytes);
            return self.text_wert(text.strip_prefix('\u{feff}').unwrap_or(&text)).map_err(Stoerung::Fehler);
        };
        let opt = self.arg(args, 1);
        let strom = match opt {
            GVal::Ref(_) => {
                let v = self.hole(opt, "stream")?;
                self.zu_bool(v)
            }
            _ => false,
        };
        let lies_text = |e: &mut Self, n: &str| match e.eigene_lese(ich, n) {
            Some(v) => e.zu_text(v).ok(),
            None => None,
        };
        let name = lies_text(self, "__kodierung").unwrap_or_else(|| String::from("utf-8"));
        let kod = Kodierung::aus_name(&name).unwrap_or(Kodierung::Utf8);
        let fatal = self.eigene_lese(ich, "fatal") == Some(GVal::Bool(true));
        let ohne_bom = self.eigene_lese(ich, "ignoreBOM") == Some(GVal::Bool(true));
        let bom_offen = self.eigene_lese(ich, "__bom_offen") != Some(GVal::Bool(false));
        let mut daten: Vec<u8> = lies_text(self, "__rest").unwrap_or_default().chars().map(|c| c as u32 as u8).collect();
        daten.extend_from_slice(&bytes);
        let schnitt = if strom { strom_schnitt(&daten, kod) } else { daten.len() };
        let rest: String = daten[schnitt..].iter().map(|&b| b as char).collect();
        daten.truncate(schnitt);
        let mut anfang = 0usize;
        if bom_offen && !ohne_bom {
            let bom: &[u8] = match kod {
                Kodierung::Utf8 => &[0xEF, 0xBB, 0xBF],
                Kodierung::Utf16Le => &[0xFF, 0xFE],
                Kodierung::Utf16Be => &[0xFE, 0xFF],
                _ => &[],
            };
            if !bom.is_empty() && daten.starts_with(bom) {
                anfang = bom.len();
            }
        }
        let teil = &daten[anfang..];
        if fatal {
            let kaputt = match kod {
                Kodierung::Utf8 => std::str::from_utf8(teil).is_err(),
                _ => dekodiere(teil, kod).contains('\u{FFFD}'),
            };
            if kaputt {
                return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Failed to execute 'decode' on 'TextDecoder': The encoded data was not valid.")));
            }
        }
        // UTF-16 hier selbst: `dekodiere` ueberspringt eine BOM immer (auch
        // mitten im Strom und bei `ignoreBOM`).
        let text = match kod {
            Kodierung::Utf8 => String::from_utf8_lossy(teil).into_owned(),
            Kodierung::Utf16Le | Kodierung::Utf16Be => {
                let le = kod == Kodierung::Utf16Le;
                let einheiten = teil.chunks(2).map(|c| match c {
                    [a, b] if le => u16::from_le_bytes([*a, *b]),
                    [a, b] => u16::from_be_bytes([*a, *b]),
                    _ => 0xFFFD,
                });
                char::decode_utf16(einheiten).map(|r| r.unwrap_or('\u{FFFD}')).collect()
            }
            k => dekodiere(teil, k),
        };
        // Der BOM-Zustand: nach dem ersten Inhalt eines Stroms ist er zu; ein
        // Aufruf ohne `stream` beendet den Strom (alles zurueck).
        let zu = !daten.is_empty();
        self.versteckt_setzen(ich, "__bom_offen", GVal::Bool(!(strom && zu) && (bom_offen || !strom)))?;
        let rest_wert = self.text_wert(if strom { &rest } else { "" }).map_err(Stoerung::Fehler)?;
        self.versteckt_setzen(ich, "__rest", rest_wert)?;
        self.text_wert(&text).map_err(Stoerung::Fehler)
    }

}

/// Release 11.10.2026: wo ein Strom-Stueck enden darf, ohne ein Zeichen zu
/// zerschneiden (UTF-8: unvollstaendige Folge am Ende; UTF-16: ungerade Laenge
/// und ein hohes Surrogat am Ende). Andere Kodierungen dekodieren ganz.
fn strom_schnitt(daten: &[u8], kod: narsil_fmt_html::kodierung::Kodierung) -> usize {
    use narsil_fmt_html::kodierung::Kodierung;
    let n = daten.len();
    match kod {
        Kodierung::Utf8 => {
            for zurueck in 1..=3.min(n) {
                let b = daten[n - zurueck];
                if b & 0xC0 == 0x80 {
                    continue;
                }
                let laenge = if b >= 0xF0 { 4 } else if b >= 0xE0 { 3 } else if b >= 0xC0 { 2 } else { 1 };
                return if laenge > zurueck { n - zurueck } else { n };
            }
            n
        }
        Kodierung::Utf16Le | Kodierung::Utf16Be => {
            let gerade = n - n % 2;
            if gerade >= 2 {
                let (a, b) = (daten[gerade - 2], daten[gerade - 1]);
                let einheit = if kod == Kodierung::Utf16Le { u16::from_le_bytes([a, b]) } else { u16::from_be_bytes([a, b]) };
                if (0xD800..0xDC00).contains(&einheit) {
                    return gerade - 2;
                }
            }
            gerade
        }
        _ => n,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// J23: die reine URL-Funktion gegen die **gemessenen** node-Werte
    /// (Quelle: /tmp/opencode/url_soll.txt). Ohne Heap — wenn hier etwas
    /// stimmt und im Motor nicht, liegt der Fehler in der Anbindung.
    #[test]
    fn url_parse_messwerte() {
        let t = url_parse("https://a.b/c?d=1#e", None).expect("absolut");
        assert_eq!(t.schema, "https");
        assert_eq!(t.host, "a.b");
        assert_eq!(t.port, "");
        assert_eq!(t.pfad, "/c");
        assert_eq!(t.abfrage, "d=1");
        assert_eq!(t.fragment, "e");
        assert_eq!(url_text(&t), "https://a.b/c?d=1#e");
        assert_eq!(url_origin(&t), "https://a.b");

        assert_eq!(url_parse("https://a.b:8443/x", None).unwrap().port, "8443");
        assert_eq!(url_parse("https://a.b:443/x", None).unwrap().port, "");
        assert_eq!(url_parse("http://a.b:80/x", None).unwrap().port, "");
        assert_eq!(url_parse("https://a.b", None).unwrap().pfad, "/");
        assert_eq!(
            url_text(&url_parse("http://user:pw@a.b/x", None).unwrap()),
            "http://user:pw@a.b/x"
        );

        // Pfade (GEMESSEN gegen node).
        let b = url_parse("https://a.b/x/y/z", None).unwrap();
        assert_eq!(
            url_text(&url_parse("/pfad", Some(&b)).unwrap()),
            "https://a.b/pfad"
        );
        assert_eq!(url_parse("rel", Some(&b)).unwrap().pfad, "/x/y/rel");
        // GEMESSEN (node 22, `new URL('', 'https://a.b/x/y/z').pathname`):
        // ein **leerer** Bezug behält den ganzen Pfad (`/x/y/z`) — die alte
        // Erwartung `/x/y` war falsch (der Test lief bisher nie, weil die
        // Modultests nur mit `--lib` laufen).
        assert_eq!(url_parse("", Some(&b)).unwrap().pfad, "/x/y/z");
        assert_eq!(url_parse("..", Some(&b)).unwrap().pfad, "/x/");
        assert_eq!(url_parse("./w", Some(&b)).unwrap().pfad, "/x/y/w");
        assert_eq!(
            url_parse("https://a.b/a/b/c/../../../z", None)
                .unwrap()
                .pfad,
            "/z"
        );
        assert_eq!(url_parse("https://a.b/ä", None).unwrap().pfad, "/%C3%A4");
        assert_eq!(url_parse("HTTPS://A.B/C", None).unwrap().schema, "https");
        assert_eq!(url_parse("HTTPS://A.B/C", None).unwrap().host, "a.b");
        assert!(url_parse("a b", None).is_err(), "ohne Basis: Fehler");
    }

    /// J23: Parameter — Text hin, Text her (GEMESSEN gegen node).
    #[test]
    fn params_messwerte() {
        assert_eq!(params_als_text(&params_aus_text("a=1&b=2")), "a=1&b=2");
        assert_eq!(params_aus_text("?a=1")[0].0, "a");
        assert_eq!(params_aus_text("a")[0], ("a".to_string(), String::new()));
        assert_eq!(params_aus_text("a=%20b")[0].1, " b");
        assert_eq!(params_aus_text("a=1&a=2").len(), 2);
        assert_eq!(form_dekodieren("a+b"), "a b");
        assert_eq!(form_kodieren("a b"), "a+b");
        assert!(params_aus_text("").is_empty());
    }
}
