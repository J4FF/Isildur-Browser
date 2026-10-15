//! A4: `fetch()` — Anfragen gehen an den Host, nie direkt ins Netz.
//!
//! WARUM SO EIN RAHMEN (und nicht `narsil_net` direkt aus der VM heraus):
//! Die Engine kennt keine Sockets. Sie baut eine **Anfrage** im Klartext,
//! legt sie in eine Warteschlange und gibt ein **Versprechen** zurueck —
//! genau wie im Browser, wo `fetch` sofort ein `Promise` liefert und die
//! Antwort spaeter eintrifft. Das haelt drei Eigenschaften, die alle
//! messbar sind:
//!
//! 1. **Gleiche Herkunft Pflicht.** Bevor ueberhaupt eine Anfrage entsteht,
//!    wird die URL aufgeloest (absolut oder relativ zur Herkunft der Seite)
//!    und mit [`Herkunft`] verglichen. Passt sie nicht, wird das
//!    Versprechen **abgelehnt**, ohne dass der Host etwas tun muss. Der
//!    Browser schickt so eine Anfrage erst gar nicht — hier ebenfalls.
//! 2. **Grenzen fuer die Groesse.** Der Host nennt seine Grenzen
//!    ([`Grenzen::netz_antwort_bytes`], [`Grenzen::netz_frist_ms`]), und
//!    die Engine lehnt eine zu grosse Antwort ab, BEVOR sie in den Heap
//!    wandert. So kann ein Skript den Speicher nicht vollstopfen.
//! 3. **Kein Blockieren.** Waehrend die Anfrage laeuft, rechnet die Engine
//!    weiteres JS; das Ergebnis kommt als Meldung herein
//!    ([`Engine::netz_ergebnis`]) und laeuft danach durch die
//!    Mikrotask-Schlange, wie im Browser.
//!
//! Die Uebertragung ist ein Trait ([`Netzbau`]). Wer `fetch` benutzen will,
//! setzt eine Implementierung ([`Engine::mit_netz`]); ohne Implementierung
//! lehnt `fetch` mit einem `TypeError` ab — ehrlich statt still.
//!
//! GEMESSEN, warum die Daten im Objekt stehen (`__status`, `__koerper` …):
//! Die erste Fassung hatte eine zweite Tabelle `antworten: HashMap<HeapRef,
//! …>` im Motor. Die waechst mit jedem `fetch` und wird nur durch die
//! Grenze begrenzt, nie freigegeben. Jetzt liegen die Daten als
//! **nicht aufzaehlbare** eigene Felder im Antwort-Objekt — der GC raeumt
//! sie mit dem Objekt ab, und `Object.keys(r)` zeigt sie nicht.
//!
//! Beipackzettel: `AbortController`, `FormData`, `Blob` und Streams sind
//! NICHT Teil von A4.

#[cfg(feature = "narsil-net")]
use crate::grenzen::Grenzen;
use crate::heap::{GVal, HObj, HeapRef, VersprechenZustand};
use crate::interp::{Engine, Stoerung};

/// Harte Obergrenze fuer die Laenge einer Netz-URL in Bytes (M3/M4).
///
/// Eine URL ist keine Schleife, aber sie ist Eingabe: ohne Deckel
/// alloziert `fetch` so gross, wie das Skript will.
///
/// Release 12.10.2026: 2 MiB wie Chrome (`url::kMaxURLChars`). GEMESSEN
/// vorher (8 KiB): ein GET-Formular mit 400 Zeichen in ISO-2022-JP (rund
/// 11 KB Abfrage) scheiterte mit „URL zu lang" (WPT
/// iso2022jp-encode-form*, je rund 1.200 Teiltests).
pub(crate) const MAX_NETZ_URL: usize = 2 * 1024 * 1024;
/// Harte Obergrenze fuer die Zahl der Kopfzeilen EINER Anfrage oder
/// Antwort (M4). 64 reichen fuer jeden echten Dienst.
pub(crate) const MAX_NETZ_KOPFZEILEN: usize = 64;
/// Harte Obergrenze fuer die Laenge EINER Kopfzeile in Bytes (M4).
pub(crate) const MAX_NETZ_KOPFZEILE: usize = 8 * 1024;
/// Harte Obergrenze fuer den Anfragetext in Bytes (M4).
///
/// Release 10.10.2026: 128 MiB statt 1 MiB — ein Hochladen (`FormData` mit
/// einem Foto, 3-10 MB) scheiterte an der Grenze.
pub(crate) const MAX_NETZ_KOERPER: usize = 128 * 1024 * 1024;

/// Die Herkunft der Seite — Basis fuer relative URLs und der Vergleich,
/// an dem die Herkunftsgrenze haengt.
///
/// ```text
/// engine.mit_herkunft("http://127.0.0.1:8080/index.html")?;
/// ```
/// Verglichen werden Schema, Host und Port; der Default-Port (80/443)
/// wird eingesetzt, damit `http://a` und `http://a:80` dasselbe sind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Herkunft {
    /// `http` oder `https`.
    pub schema: String,
    /// Host ohne Port, kleingeschrieben.
    pub host: String,
    /// Port, immer aufgeloest (Default 80).
    pub port: u16,
}

impl Herkunft {
    /// Herkunft aus einer URL ziehen. Ohne Schema -> Fehlertext.
    pub fn aus_url(url: &str) -> Result<Self, String> {
        let (schema, rest) = url
            .split_once("://")
            .ok_or_else(|| format!("Herkunft ohne Schema: {url}"))?;
        let schema = schema.to_ascii_lowercase();
        // 06.10.2026: DER STANDARDPORT HAENGT AM SCHEMA. GEMESSEN: der
        // gemeinsame Parser (`narsil_url::loese_auf`) normalisiert
        // `https://b.example:443/` zu `https://b.example/`; dieser Parser
        // setzte aber IMMER 80 als Default. Damit war `ziel` (80) ungleich
        // `herkunft` (443), und `fetch` lehnte jede HTTPS-Anfrage auf der
        // Standardport-Form ab (Test `netz::herkunftsformen`).
        let standard_port = standard_port_fuer(&schema);
        let (host, port) = host_port(rest, standard_port)?;
        Ok(Herkunft { schema, host, port })
    }

    /// `http://host:port` — die Form, die der Host zum Verbinden braucht.
    /// IPv6-Hosts bekommen ihre Klammern wieder (GEMESSEN: `http://::1:8080`
    /// liess sich nicht auflösen).
    pub fn quelle(&self) -> String {
        if self.host.contains(':') {
            format!("{}://[{}]:{}", self.schema, self.host, self.port)
        } else {
            format!("{}://{}:{}", self.schema, self.host, self.port)
        }
    }

    /// Release 10.10.2026: die SERIALISIERTE Herkunft (HTML §7.5, ohne
    /// Standardport) — so steht sie im `Origin`-Kopf und wird mit
    /// `Access-Control-Allow-Origin` verglichen.
    pub fn als_origin(&self) -> String {
        let host = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        if self.port == standard_port_fuer(&self.schema) {
            format!("{}://{}", self.schema, host)
        } else {
            format!("{}://{}:{}", self.schema, host, self.port)
        }
    }
}

// ===========================================================================
// CORS (Fetch §3.2, §4.8) — Release 10.10.2026
// ===========================================================================
//
// GEMESSEN vorher: jede Anfrage an eine fremde Herkunft wurde abgelehnt
// („Herkunft verboten") — bbc.com bekam weder die Optimizely-Datei noch
// seine Nutzerinfo (`dotcom.userinfo` mit Vorgabewerten -> drohende
// Umleitung nach .co.uk). Ein Browser schickt die Anfrage ab und gibt die
// Antwort nur heraus, wenn der Server sie freigibt. Nicht einfache Anfragen
// bekommen vorher eine Vorabanfrage (`OPTIONS`) — ohne sie waeren Server
// schutzloser als in Chrome. Zugangsdaten (Cookies) schickt der Transport
// ohnehin nicht mit.

/// Die CORS-Pruefung einer Anfrage an eine fremde Herkunft.
#[derive(Debug, Clone)]
pub(crate) struct CorsPruefung {
    /// Die serialisierte Herkunft der Seite (`Origin`).
    pub origin: String,
    /// `credentials: 'include'`.
    pub mit_daten: bool,
    /// `mode: 'no-cors'` -> opake Antwort.
    pub ohne_cors: bool,
    /// Die Vorabanfrage (nur bei nicht einfachen Anfragen).
    pub vorab: Option<NetzAnfrage>,
    /// Die Vorabanfrage ist bestanden.
    pub vorab_fertig: bool,
    /// Methode der eigentlichen Anfrage.
    pub methode: String,
    /// Die nicht freigegebenen Kopfnamen (klein, sortiert).
    pub unsichere_koepfe: Vec<String>,
}

/// Fetch §2.2.2 „forbidden request-header name" — ein Skript setzt sie nicht.
fn kopf_verboten(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    n.starts_with("proxy-")
        || n.starts_with("sec-")
        || matches!(
            n.as_str(),
            "accept-charset"
                | "accept-encoding"
                | "access-control-request-headers"
                | "access-control-request-method"
                | "connection"
                | "content-length"
                | "cookie"
                | "cookie2"
                | "date"
                | "dnt"
                | "expect"
                | "host"
                | "keep-alive"
                | "origin"
                | "referer"
                | "set-cookie"
                | "te"
                | "trailer"
                | "transfer-encoding"
                | "upgrade"
                | "via"
        )
}

/// Fetch §2.2.2 „CORS-unsafe request-header byte".
fn unsicheres_byte(b: u8) -> bool {
    (b < 0x20 && b != 0x09) || matches!(b, b'"' | b'(' | b')' | b':' | b'<' | b'>' | b'?' | b'@' | b'[' | b'\\' | b']' | b'{' | b'}' | 0x7f)
}

/// Fetch §2.2.2 „CORS-safelisted request-header".
fn kopf_freigegeben(name: &str, wert: &str) -> bool {
    if wert.len() > 128 {
        return false;
    }
    match name.trim().to_ascii_lowercase().as_str() {
        "accept" => !wert.bytes().any(unsicheres_byte),
        "accept-language" | "content-language" => wert
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'*' | b',' | b'-' | b'.' | b';' | b'=')),
        "content-type" => {
            if wert.bytes().any(unsicheres_byte) {
                return false;
            }
            let essenz = wert.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
            matches!(essenz.as_str(), "application/x-www-form-urlencoded" | "multipart/form-data" | "text/plain")
        }
        _ => false,
    }
}

/// Fetch §2.2.1 „CORS-safelisted method".
fn methode_einfach(m: &str) -> bool {
    matches!(m, "GET" | "HEAD" | "POST")
}

/// Ein Antwortkopf (ohne Gross-/Kleinschreibung; mehrere -> mit `, `).
fn antwort_kopf(kopf: &[(String, String)], name: &str) -> Option<String> {
    let mut raus: Option<String> = None;
    for (n, w) in kopf {
        if n.eq_ignore_ascii_case(name) {
            match raus.as_mut() {
                Some(r) => {
                    r.push_str(", ");
                    r.push_str(w);
                }
                None => raus = Some(w.clone()),
            }
        }
    }
    raus
}

/// Steht `wert` in einer Komma-Liste?
fn in_liste(liste: &str, wert: &str, gross_egal: bool) -> bool {
    liste.split(',').map(str::trim).any(|t| if gross_egal { t.eq_ignore_ascii_case(wert) } else { t == wert })
}

/// Fetch §4.9 „CORS check".
fn cors_antwort_pruefen(c: &CorsPruefung, a: &NetzAntwort) -> Result<(), String> {
    let acao = antwort_kopf(&a.kopf, "access-control-allow-origin");
    match acao.as_deref().map(str::trim) {
        None => return Err("kein Access-Control-Allow-Origin-Kopf".to_string()),
        Some("*") if !c.mit_daten => {}
        Some("*") => {
            return Err("Access-Control-Allow-Origin '*' ist mit credentials 'include' nicht erlaubt".to_string());
        }
        Some(w) if w == c.origin => {}
        Some(w) => return Err(format!("Access-Control-Allow-Origin '{w}' ist nicht die Herkunft")),
    }
    if c.mit_daten
        && antwort_kopf(&a.kopf, "access-control-allow-credentials").as_deref().map(str::trim) != Some("true")
    {
        return Err("Access-Control-Allow-Credentials ist nicht 'true'".to_string());
    }
    Ok(())
}

/// Fetch §4.8 „CORS-preflight fetch", Schritte 7-8.
fn cors_vorab_pruefen(c: &CorsPruefung, a: &NetzAntwort) -> Result<(), String> {
    if !(200..300).contains(&a.status) {
        return Err(format!("Vorabanfrage mit Status {}", a.status));
    }
    cors_antwort_pruefen(c, a)?;
    if !methode_einfach(&c.methode) {
        let erlaubt = antwort_kopf(&a.kopf, "access-control-allow-methods").unwrap_or_default();
        if !(in_liste(&erlaubt, &c.methode, false) || (!c.mit_daten && in_liste(&erlaubt, "*", false))) {
            return Err(format!("Methode {} nicht in Access-Control-Allow-Methods", c.methode));
        }
    }
    let erlaubt = antwort_kopf(&a.kopf, "access-control-allow-headers").unwrap_or_default();
    for k in &c.unsichere_koepfe {
        let stern = !c.mit_daten && k != "authorization" && in_liste(&erlaubt, "*", false);
        if !(stern || in_liste(&erlaubt, k, true)) {
            return Err(format!("Kopf '{k}' nicht in Access-Control-Allow-Headers"));
        }
    }
    Ok(())
}

/// Fetch §2.2.6 „CORS-safelisted response-header name" plus
/// `Access-Control-Expose-Headers` — nur diese Koepfe sieht das Skript.
fn cors_koepfe_filtern(c: &CorsPruefung, mut a: NetzAntwort) -> NetzAntwort {
    let frei = antwort_kopf(&a.kopf, "access-control-expose-headers").unwrap_or_default();
    let stern = !c.mit_daten && in_liste(&frei, "*", false);
    a.kopf.retain(|(n, _)| {
        let n = n.to_ascii_lowercase();
        if n == "set-cookie" || n == "set-cookie2" {
            return false;
        }
        matches!(
            n.as_str(),
            "cache-control" | "content-language" | "content-length" | "content-type" | "expires" | "last-modified" | "pragma"
        ) || stern
            || in_liste(&frei, &n, true)
    });
    a
}

/// Release 10.10.2026: die gelesenen `fetch`-Optionen.
pub(crate) struct FetchOptionen {
    pub methode: String,
    pub kopf: Vec<(String, String)>,
    pub koerper: Option<Vec<u8>>,
    /// `cors` (Vorgabe), `no-cors`, `same-origin`, `navigate`.
    pub modus: String,
    /// `credentials: 'include'`.
    pub mit_daten: bool,
}

/// Der Standardport eines Schemas (RFC 3986 3.1): 80 fuer http/ws,
/// 443 fuer https/wss. Alles andere (unseriöse Eingabe) nimmt 80.
fn standard_port_fuer(schema: &str) -> u16 {
    match schema {
        "https" | "wss" => 443,
        _ => 80,
    }
}

/// `host[:port]` aus `rest` schneiden; `standard_port` gilt ohne Angabe.
fn host_port(rest: &str, standard_port: u16) -> Result<(String, u16), String> {
    let rest = rest.split(['/', '?', '#']).next().unwrap_or("");
    let rest = rest.rsplit('@').next().unwrap_or(rest); // user@host
    if rest.is_empty() {
        return Err("Herkunft ohne Host".to_string());
    }
    if let Some(klammern_offen) = rest.strip_prefix('[') {
        // IPv6: [::1]:8080
        let ende = klammern_offen
            .find(']')
            .ok_or_else(|| "IPv6 ohne ']'".to_string())?;
        let host = klammern_offen[..ende].to_string();
        let port = match klammern_offen[ende + 1..].strip_prefix(':') {
            Some(p) => port_aus(p)?,
            None => standard_port,
        };
        return Ok((host, port));
    }
    match rest.rsplit_once(':') {
        // GEMESSEN (A5): `http://a:notaport/` wurde als Host
        // „a:notaport" mit Port 80 gelesen. Ein Doppelpunkt ohne Zahl ist
        // jetzt ein Fehler.
        Some((_h, p)) if !p.is_empty() && !p.chars().all(|c| c.is_ascii_digit()) => {
            Err(format!("Port keine Zahl: {p}"))
        }
        Some((h, p)) => Ok((h.to_ascii_lowercase(), port_aus(p)?)),
        None => Ok((rest.to_ascii_lowercase(), standard_port)),
    }
}

fn port_aus(text: &str) -> Result<u16, String> {
    text.parse::<u16>()
        .map_err(|_| format!("Port keine Zahl: {text}"))
}

/// Eine Anfrage, wie sie die Engine dem Host vorlegt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetzAnfrage {
    /// Nummer der Anfrage. Das Skript sieht sie nicht, der Host braucht
    /// sie, um die Antwort zuzuordnen.
    pub id: u64,
    /// Absolute URL nach dem Aufloesen gegen die Herkunft.
    pub url: String,
    /// `GET` (Standard) oder `POST` — A4 kennt nur diese beiden.
    pub methode: String,
    /// Zusatzliche Kopfzeilen in der Schreibweise des Skripts.
    pub kopf: Vec<(String, String)>,
    /// Anfragetext (`POST`).
    pub koerper: Option<Vec<u8>>,
}

/// Release 11.10.2026: der „data: URL processor" (Fetch §4.? / RFC 2397):
/// MIME-Typ vor dem Komma, der Koerper prozent-dekodiert, mit `;base64`
/// nach „forgiving-base64". `None` ist ein Fehler (kein Komma, kaputtes
/// Base64).
pub fn daten_url_lesen(url: &str) -> Option<(String, Vec<u8>)> {
    let rest = url.get(5..)?;
    let rest = match rest.find('#') {
        Some(i) => &rest[..i],
        None => rest,
    };
    let komma = rest.find(',')?;
    let mut mime = rest[..komma].trim_matches(|c: char| c.is_ascii_whitespace()).to_string();
    let roh = &rest.as_bytes()[komma + 1..];
    let mut koerper = Vec::with_capacity(roh.len());
    let mut i = 0;
    while i < roh.len() {
        if roh[i] == b'%'
            && i + 2 < roh.len()
            && let (Some(h), Some(l)) = ((roh[i + 1] as char).to_digit(16), (roh[i + 2] as char).to_digit(16))
        {
            koerper.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        koerper.push(roh[i]);
        i += 1;
    }
    let klein = mime.to_ascii_lowercase();
    if klein.ends_with("base64") {
        let ohne = klein[..klein.len() - 6].trim_end_matches(' ');
        if ohne.ends_with(';') {
            koerper = vergebend_base64(&koerper)?;
            mime.truncate(ohne.len() - 1);
        }
    }
    if mime.starts_with(';') {
        mime.insert_str(0, "text/plain");
    }
    let (typ, param) = match mime.find(';') {
        Some(i) => (mime[..i].trim().to_ascii_lowercase(), mime[i..].trim().to_string()),
        None => (mime.trim().to_ascii_lowercase(), String::new()),
    };
    let gueltig = typ.split_once('/').is_some_and(|(a, b)| {
        let token = |t: &str| !t.is_empty() && t.bytes().all(|c| c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c));
        token(a) && token(b)
    });
    let mime = if gueltig {
        let param: String = param.split(';').map(str::trim).filter(|p| !p.is_empty()).map(|p| format!(";{p}")).collect();
        format!("{typ}{param}")
    } else {
        "text/plain;charset=US-ASCII".to_string()
    };
    Some((mime, koerper))
}

/// „forgiving-base64 decode" (Infra §4.6): Leerraum weg, `=` am Ende
/// optional; `None` bei Ungueltigem.
fn vergebend_base64(roh: &[u8]) -> Option<Vec<u8>> {
    let mut d: Vec<u8> = roh.iter().copied().filter(|c| !matches!(c, b' ' | b'\t' | b'\n' | b'\x0c' | b'\r')).collect();
    if d.len() % 4 == 0 {
        if d.ends_with(b"==") {
            d.truncate(d.len() - 2);
        } else if d.ends_with(b"=") {
            d.truncate(d.len() - 1);
        }
    }
    if d.len() % 4 == 1 {
        return None;
    }
    let wert = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let mut aus = Vec::with_capacity(d.len() * 3 / 4);
    let mut puffer = 0u32;
    let mut bits = 0;
    for &c in &d {
        puffer = (puffer << 6) | wert(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            aus.push((puffer >> bits) as u8);
            puffer &= (1 << bits) - 1;
        }
    }
    Some(aus)
}

/// Die Antwort des Hosts, unveraendert uebernommen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetzAntwort {
    /// HTTP-Status (200, 404, 500 …).
    pub status: u16,
    /// Kopfzeilen der Antwort in der Reihenfolge des Hosts.
    pub kopf: Vec<(String, String)>,
    /// Antworttext als Bytes.
    pub koerper: Vec<u8>,
}

/// Das Ergebnis einer Anfrage, wenn der Host fertig ist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetzErgebnis {
    /// Die Antwort ist da.
    Antwort(NetzAntwort),
    /// Der Host konnte die Anfrage nicht ausfuehren (Text = Grund).
    Fehler(String),
}

/// Die Uebertragung. Wer `fetch` benutzt, baut eine Implementierung.
///
/// Die Engine ruft `hole` **ausserhalb** ihrer eigenen Ausfuehrung auf:
/// waehrend des Aufrufs wird kein JS gerechnet, die Engine wird also nicht
/// gleichzeitig benutzt. Fehler kommen als Text zurueck (sie landen als
/// `TypeError` im Skript), niemals als Panic.
pub trait Netzbau {
    /// Eine Anfrage ausfuehren. `Ok` heisst Antwort, `Err` Fehlertext.
    fn hole(&mut self, anfrage: &NetzAnfrage) -> Result<NetzAntwort, String>;

    /// Release 15.10.2026: all requests of one round, results in the same
    /// order. The default runs them one after the other; a transport that
    /// can, overrides it to fetch them in parallel (see [`parallel_holen`]).
    fn hole_alle(&mut self, anfragen: &[NetzAnfrage]) -> Vec<Result<NetzAntwort, String>> {
        anfragen.iter().map(|a| self.hole(a)).collect()
    }
}

/// Release 15.10.2026: run `hole` for every request on up to
/// `NETZ_PARALLEL` threads (scoped; each thread works on its own clone of
/// the transport), results in request order. MEASURED before (chip.de):
/// one script round issued 45 API requests, fetched one after the other at
/// ~200 ms each (a TLS handshake per request) — 9.1 s of the 20.5 s JS
/// phase; in parallel the round takes about the time of its slowest batch.
pub fn parallel_holen<T: Netzbau + Clone + Send>(bau: &T, anfragen: &[NetzAnfrage]) -> Vec<Result<NetzAntwort, String>> {
    if anfragen.len() <= 1 {
        let mut b = bau.clone();
        return anfragen.iter().map(|a| b.hole(a)).collect();
    }
    let naechste = std::sync::atomic::AtomicUsize::new(0);
    let ergebnisse: std::sync::Mutex<Vec<Option<Result<NetzAntwort, String>>>> =
        std::sync::Mutex::new((0..anfragen.len()).map(|_| None).collect());
    let faeden = anfragen.len().min(NETZ_PARALLEL);
    std::thread::scope(|s| {
        for _ in 0..faeden {
            let mut b = bau.clone();
            let naechste = &naechste;
            let ergebnisse = &ergebnisse;
            s.spawn(move || loop {
                let i = naechste.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if i >= anfragen.len() {
                    break;
                }
                let r = b.hole(&anfragen[i]);
                if let Ok(mut e) = ergebnisse.lock() {
                    e[i] = Some(r);
                }
            });
        }
    });
    ergebnisse
        .into_inner()
        .unwrap_or_default()
        .into_iter()
        .map(|r| r.unwrap_or_else(|| Err("Anfrage nicht ausgefuehrt (intern)".to_string())))
        .collect()
}

/// Release 15.10.2026: requests of one round fetched at the same time
/// (browsers keep 6 per host and many more in total).
pub const NETZ_PARALLEL: usize = 8;

/// Die Anbindung an `narsil-net` (A4: „fetch() ueber narsil-net").
///
/// Nur mit dem Merkmal `narsil-net` vorhanden (siehe `Cargo.toml`: ohne das
/// Merkmal haengt diese Crate an keiner fremden).
///
/// Sie uebersetzt die Grenzen der Engine in `narsil_net::Limits`:
/// Antwortgroesse, Gesamtfrist, Umleitungen. Das ist der einzige Ort, an
/// dem `narsil-net` vorkommt — Tests benutzen statt dessen eine eigene
/// [`Netzbau`]-Implementierung und brauchen kein Netz.
#[cfg(feature = "narsil-net")]
#[derive(Debug, Clone, Copy, Default)]
pub struct NarsilNet {
    /// `https` braucht TLS; die Standardanbindung nimmt nur `http` (das
    /// genuegt fuer Seiten des Browsers und fuer Tests).
    pub https_erlaubt: bool,
}

#[cfg(feature = "narsil-net")]
impl Netzbau for NarsilNet {
    fn hole(&mut self, anfrage: &NetzAnfrage) -> Result<NetzAntwort, String> {
        self.hole_mit(anfrage, Grenzen::standard())
    }

    fn hole_alle(&mut self, anfragen: &[NetzAnfrage]) -> Vec<Result<NetzAntwort, String>> {
        parallel_holen(self, anfragen)
    }
}

#[cfg(feature = "narsil-net")]
impl NarsilNet {
    /// Ausfuehren mit den Grenzen der Engine.
    pub fn hole_mit(
        &mut self,
        anfrage: &NetzAnfrage,
        grenzen: Grenzen,
    ) -> Result<NetzAntwort, String> {
        let (schema, rest) = anfrage
            .url
            .split_once("://")
            .ok_or_else(|| format!("URL ohne Schema: {}", anfrage.url))?;
        let (host, port) = host_port(rest, standard_port_fuer(&schema.to_ascii_lowercase()))?;
        if schema.eq_ignore_ascii_case("https") && !self.https_erlaubt {
            return Err("https ist in dieser Engine nicht freigegeben".to_string());
        }
        let pfad = match rest.find('/') {
            Some(i) => &rest[i..],
            None => "/",
        };
        // Release 10.10.2026: jede Methode, die `fetch` zulaesst (vorher
        // GET und POST), samt der CORS-Vorabanfrage `OPTIONS`.
        let methode = narsil_net::Method::aus_text(&anfrage.methode).unwrap_or(narsil_net::Method::Get);
        let anfrage_text = match methode {
            narsil_net::Method::Get => {
                narsil_net::Request::get(&host, port, pfad).art(narsil_net::RessourceArt::Daten)
            }
            narsil_net::Method::Post => narsil_net::Request::post(
                &host,
                port,
                pfad,
                anfrage.koerper.clone().unwrap_or_default(),
            ),
            m => {
                let mut r = narsil_net::Request::get(&host, port, pfad).art(narsil_net::RessourceArt::Daten);
                r.method = m;
                if !matches!(m, narsil_net::Method::Head | narsil_net::Method::Options) {
                    r.body = anfrage.koerper.clone();
                }
                r
            }
        };
        let mut mit_kopf = anfrage_text;
        // Release 10.10.2026: das Schema ausdruecklich mitgeben — `narsil-net`
        // erkennt TLS sonst nur an Port 443 (`ist_https`). GEMESSEN vorher:
        // `https://localhost:8443/` ging als KLARTEXT-HTTP an den TLS-Port.
        mit_kopf.https = Some(schema.eq_ignore_ascii_case("https"));
        for (n, w) in &anfrage.kopf {
            mit_kopf = mit_kopf.header(n, w);
        }
        let limits = narsil_net::Limits {
            gesamt: std::time::Duration::from_millis(grenzen.netz_frist_ms),
            body_bytes: grenzen.netz_antwort_bytes,
            ..narsil_net::Limits::default()
        };
        let antwort = narsil_net::fetch_mit(&mit_kopf, limits).map_err(|e| e.to_string())?;
        let mut kopf = Vec::new();
        for (n, w) in antwort.headers.iter() {
            let _ = &antwort.status.reason;
            // M3: Die Zahl der Kopfzeilen ist gedeckelt.
            if kopf.len() >= MAX_NETZ_KOPFZEILEN {
                break;
            }
            kopf.push((n.clone(), w.clone()));
        }
        let koerper = antwort.body.clone();
        if koerper.len() > grenzen.netz_antwort_bytes {
            return Err(format!(
                "Antwort zu gross: {} Bytes (Grenze {})",
                koerper.len(),
                grenzen.netz_antwort_bytes
            ));
        }
        Ok(NetzAntwort {
            status: antwort.status.code,
            kopf,
            koerper,
        })
    }
}

/// Eine wartende Netzaufgabe: Versprechen + Anfrage.
pub(crate) struct NetzAuftrag {
    /// Nummer der Anfrage (siehe [`NetzAnfrage::id`]).
    pub id: u64,
    /// Was der Host tun soll.
    pub anfrage: NetzAnfrage,
    /// Das offene Versprechen, das die Antwort erhaelt.
    pub versprechen: HeapRef,
    /// `true`, sobald der Host die Anfrage bekommen hat. Die Aufgabe
    /// bleibt in der Liste, bis das Ergebnis da ist — `netz_ergebnis`
    /// sucht sie nach der Nummer.
    pub gesendet: bool,
    /// Release 10.10.2026: die CORS-Pruefung (nur fremde Herkunft).
    pub cors: Option<CorsPruefung>,
}

impl Engine {
    /// Den Host fuer `fetch` setzen. Ohne Netz lehnt `fetch` ab.
    pub fn mit_netz(&mut self, bau: Box<dyn Netzbau + Send>) -> &mut Self {
        self.netzbau = Some(bau);
        self
    }

    /// Release 10.10.2026: Anfragen an fremde Herkuenfte nach CORS (Fetch
    /// §4.8) statt grundsaetzlich verboten — fuer die Seiten-Engine. Ohne den
    /// Schalter bleibt die strikte Herkunftsgrenze (Tests `j25_sicherheit`).
    pub fn cors_erlauben(&mut self, an: bool) -> &mut Self {
        self.cors_an = an;
        self
    }

    /// Die Herkunft der Seite setzen (Pflicht fuer `fetch`).
    ///
    /// Ohne Herkunft lehnt `fetch` jede Anfrage ab: die Herkunftsgrenze
    /// laesst sich sonst nicht pruefen, und „immer erlauben" waere genau
    /// die Luecke, die die Regel schliessen soll.
    /// J10: die Herkunft als Text (mit `/` am Ende) — der Fallback, wenn
    /// der Host eine Modul-URL **relativ** uebergibt. Die Herkunft selbst
    /// merkt sich nur Schema/Host/Port (der Pfad gehoert dem Host), also
    /// ist das nur die Basis fuer `./x` aus dem Wurzelverzeichnis.
    pub(crate) fn herkunft_text(&self) -> Option<String> {
        self.herkunft.as_ref().map(|h| format!("{}/", h.quelle()))
    }

    pub fn mit_herkunft(&mut self, url: &str) -> Result<&mut Self, String> {
        self.herkunft = Some(Herkunft::aus_url(url)?);
        Ok(self)
    }

    /// Die Anfragen herausholen, die der Host noch nicht hat. Die Aufgaben
    /// bleiben in der Liste (bis `netz_ergebnis` sie abholt), damit das
    /// Ergebnis zugeordnet werden kann.
    ///
    /// GEMESSEN/ENTSCHIEDEN: die erste Fassung hat die Liste hier geleert
    /// — dann fand `netz_ergebnis` nichts mehr, das Versprechen blieb
    /// offen und die `.then`-Kette lief nie.
    pub fn netz_abarbeiten(&mut self) -> Vec<NetzAnfrage> {
        // M3: Die Liste ist durch `max_netz_aufgaben` gedeckelt (max 64),
        // deshalb ist das Kopieren hier gefahrlos.
        let mut raus: Vec<NetzAnfrage> = Vec::new();
        for a in self.netz.iter_mut() {
            if !a.gesendet {
                a.gesendet = true;
                // Release 10.10.2026: erst die CORS-Vorabanfrage (gleiche Id).
                let vorab = a
                    .cors
                    .as_ref()
                    .filter(|c| !c.vorab_fertig)
                    .and_then(|c| c.vorab.clone());
                match vorab {
                    Some(mut v) => {
                        v.id = a.id;
                        raus.push(v);
                    }
                    None => raus.push(a.anfrage.clone()),
                }
            }
        }
        raus
    }

    /// Wie viele Anfragen noch nicht beantwortet sind (wartend oder beim
    /// Host).
    pub fn netz_offen(&self) -> usize {
        self.netz.len()
    }

    /// Das Ergebnis einer Anfrage melden: das Versprechen wird erfuellt
    /// oder abgelehnt, danach laufen die Mikrotasks (wie im Browser).
    ///
    /// Unbekannte `id` -> `Ok(())` ohne Wirkung (der Host darf nach einem
    /// Abbruch nicht das Programm sprengen).
    pub fn netz_ergebnis(&mut self, id: u64, ergebnis: NetzErgebnis) -> Result<(), String> {
        let Some(stelle) = self.netz.iter().position(|a| a.id == id) else {
            return Ok(());
        };
        // Release 10.10.2026: die Antwort auf eine CORS-VORABANFRAGE.
        let vorab_offen = self.netz[stelle]
            .cors
            .as_ref()
            .is_some_and(|c| c.vorab.is_some() && !c.vorab_fertig);
        if vorab_offen {
            let pruefung = match (&ergebnis, self.netz[stelle].cors.as_ref()) {
                (NetzErgebnis::Antwort(a), Some(c)) => cors_vorab_pruefen(c, a),
                (NetzErgebnis::Fehler(t), _) => Err(format!("Vorabanfrage gescheitert: {t}")),
                _ => Err("Vorabanfrage ohne Pruefung (intern)".to_string()),
            };
            match pruefung {
                Ok(()) => {
                    if let Some(c) = self.netz[stelle].cors.as_mut() {
                        c.vorab_fertig = true;
                    }
                    // Die eigentliche Anfrage geht in der naechsten Runde raus.
                    self.netz[stelle].gesendet = false;
                    return Ok(());
                }
                Err(grund) => {
                    let auftrag = self.netz.remove(stelle);
                    self.cors_blockiert(&auftrag, &grund);
                    return Ok(());
                }
            }
        }
        let auftrag = self.netz.remove(stelle);
        match ergebnis {
            NetzErgebnis::Antwort(a) => {
                let (a, art) = match &auftrag.cors {
                    None => (a, "basic"),
                    Some(c) if c.ohne_cors => (
                        NetzAntwort {
                            status: 0,
                            kopf: Vec::new(),
                            koerper: Vec::new(),
                        },
                        "opaque",
                    ),
                    Some(c) => match cors_antwort_pruefen(c, &a) {
                        Ok(()) => (cors_koepfe_filtern(c, a), "cors"),
                        Err(grund) => {
                            self.cors_blockiert(&auftrag, &grund);
                            return Ok(());
                        }
                    },
                };
                let url = if art == "opaque" { String::new() } else { auftrag.anfrage.url.clone() };
                let marke = self.wurzel_merke();
                self.wurzeln.push(GVal::Ref(auftrag.versprechen));
                let objekt = match self.antwort_objekt(&a, &url, art) {
                    Ok(o) => o,
                    Err(Stoerung::Wurf(w)) => {
                        self.wurzeln.push(w);
                        self.versprechen_settle(auftrag.versprechen, false, w);
                        self.wurzel_zurueck(marke);
                        self.netz_mikro();
                        return Ok(());
                    }
                    Err(Stoerung::Fehler(f)) => {
                        let w = self.fehler_wert("Error", &f.to_string());
                        self.versprechen_settle(auftrag.versprechen, false, w);
                        self.wurzel_zurueck(marke);
                        self.netz_mikro();
                        return Ok(());
                    }
                };
                self.versprechen_settle(auftrag.versprechen, true, objekt);
                self.wurzel_zurueck(marke);
            }
            NetzErgebnis::Fehler(text) => {
                let marke = self.wurzel_merke();
                self.wurzeln.push(GVal::Ref(auftrag.versprechen));
                let wert = self.fehler_wert("TypeError", &text);
                self.versprechen_settle(auftrag.versprechen, false, wert);
                self.wurzel_zurueck(marke);
            }
        }
        self.netz_mikro();
        Ok(())
    }

    /// Release 10.10.2026: eine CORS-Ablehnung — `TypeError: Failed to
    /// fetch` fuer das Skript (wie Chrome, ohne Einzelheiten) und der Grund
    /// in der Konsole.
    fn cors_blockiert(&mut self, auftrag: &NetzAuftrag, grund: &str) {
        let origin = auftrag.cors.as_ref().map(|c| c.origin.clone()).unwrap_or_default();
        if self.konsole.len() < 10_000 {
            self.konsole.push(format!(
                "CORS: Zugriff auf '{}' von der Herkunft '{origin}' blockiert: {grund}",
                auftrag.anfrage.url
            ));
        }
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(auftrag.versprechen));
        let wert = self.fehler_wert("TypeError", "Failed to fetch");
        self.versprechen_settle(auftrag.versprechen, false, wert);
        self.wurzel_zurueck(marke);
        self.netz_mikro();
    }

    /// Bequemer Weg fuer die Einbettung: alle wartenden Anfragen ueber den
    /// gesetzten Transport ausfuehren und die Ergebnisse melden.
    ///
    /// Ohne Transport passiert nichts (die Anfragen bleiben liegen) — der
    /// Aufrufer merkt es an [`Engine::netz_offen`].
    pub fn netz_schleife(&mut self) -> Result<(), String> {
        if self.netzbau.is_none() {
            return Ok(());
        }
        // M3: hoechstens `max_netz_aufgaben` (+1) Runden — eine Anfrage
        // kann im `.then` weitere erzeugen, und die Schleife muss enden.
        let mut reste = self.grenzen.max_netz_aufgaben.saturating_add(1);
        while reste > 0 {
            let offen = self.netz_abarbeiten();
            if offen.is_empty() {
                break; // nichts Neues mehr (alles beim Host oder fertig)
            }
            reste -= 1;
            // Release 15.10.2026: the whole round at once (`hole_alle`, in
            // parallel where the transport can), then the results in
            // request order — the same order as before.
            let Some(mut bau) = self.netzbau.take() else {
                return Ok(());
            };
            let ergebnisse = bau.hole_alle(&offen);
            self.netzbau = Some(bau);
            for (anfrage, ergebnis) in offen.iter().zip(ergebnisse) {
                let ergebnis = match ergebnis {
                    Ok(a) => NetzErgebnis::Antwort(a),
                    Err(m) => NetzErgebnis::Fehler(m),
                };
                self.netz_ergebnis(anfrage.id, ergebnis)?;
            }
        }
        Ok(())
    }

    /// Mikrotasks nach einer Netzaufgabe laufen lassen (A4). Die Grenze
    /// ist dieselbe wie im Hauptlauf (`max_mikrotasks`).
    fn netz_mikro(&mut self) {
        let deckel = self.grenzen.max_mikrotasks.max(8);
        let _ = self.mikro_abarbeiten(deckel);
    }

    /// `fetch(url, opts)` — gibt sofort ein Versprechen zurueck.
    pub(crate) fn n_fetch(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        // M4: Allokation des Versprechens zaehlt mit.
        let p = self.versprechen_anlegen(VersprechenZustand::Offen);
        let GVal::Ref(pr) = p else {
            return Err(Stoerung::Wurf(
                self.fehler_wert("Error", "Versprechen nicht gebaut (intern)"),
            ));
        };
        // Alles Fehlerhafte wird ABGELEHNT, nicht geworfen (wie in node).
        // GEMESSEN/ENTSCHIEDEN: `fetch(42)` wandte `String(42)` an und
        // holte `/42` — node lehnt mit „Failed to parse URL" ab. Deshalb
        // muss das erste Argument Text sein.
        // Release 10.10.2026: auch `fetch(new URL(…))` und
        // `fetch(new Request(…))` (Fetch §5.3: RequestInfo). GEMESSEN vorher:
        // „URL ist kein Text".
        let mut anfrage_objekt: Option<GVal> = None;
        let roh_url = match self.arg(args, 0) {
            GVal::Ref(r) => match self.heap.holen(r).clone() {
                HObj::Zeichenkette(t) => t,
                HObj::Nativ { name, .. } if name == "Request" => {
                    anfrage_objekt = Some(GVal::Ref(r));
                    let marke = self.wurzel_merke();
                    self.wurzeln.push(p);
                    let u = self.hole(GVal::Ref(r), "url").unwrap_or(GVal::Undefiniert);
                    let t = self.zu_text(u).unwrap_or_default();
                    self.wurzel_zurueck(marke);
                    t
                }
                _ => {
                    let marke = self.wurzel_merke();
                    self.wurzeln.push(p);
                    let t = self.zu_text(GVal::Ref(r));
                    self.wurzel_zurueck(marke);
                    match t {
                        Ok(t) => t,
                        Err(_) => {
                            return self.fetch_ablehnen(
                                pr,
                                "TypeError",
                                "fetch: URL ist kein Text (Failed to parse URL)",
                            );
                        }
                    }
                }
            },
            GVal::Undefiniert | GVal::Null => {
                return self.fetch_ablehnen(pr, "TypeError", "fetch: URL fehlt");
            }
            // J24: ein BigInt ist auch keine URL (node lehnt ebenso ab).
            GVal::Zahl(_) | GVal::Bool(_) | GVal::Ganz(_) => {
                return self.fetch_ablehnen(
                    pr,
                    "TypeError",
                    "fetch: URL ist kein Text (Failed to parse URL)",
                );
            }
        };
        // Release 09.10.2026: das Versprechen ist eine Wurzel, solange die
        // Optionen gelesen werden (`fetch_vorbereiten` kann allozieren und
        // sammeln). GEMESSEN vorher: youtube.com (`_.lt.fetch(b, Y).then(…)`)
        // endete im asynchronen Betrieb mit „Heap: toter Verweis" — `.then`
        // lief auf dem schon eingesammelten Versprechen.
        // Release 11.10.2026: `data:`-URLs (Fetch §4.2 „scheme fetch") —
        // ohne Netz und ohne Herkunft. GEMESSEN vorher: „fetch kennt keine
        // Herkunft" bzw. eine Anfrage an den Server; `track.src =
        // 'data:text/vtt,…'` (WPT track, 30+ Dateien) lud nie.
        if roh_url.trim_start().get(..5).is_some_and(|k| k.eq_ignore_ascii_case("data:")) {
            return self.fetch_daten_url(pr, roh_url.trim());
        }
        let marke = self.wurzel_merke();
        self.wurzeln.push(p);
        let init = args.get(1).copied().filter(|v| !matches!(v, GVal::Undefiniert | GVal::Null)).or(anfrage_objekt);
        let vorbereitet = self.fetch_vorbereiten(&roh_url, init);
        self.wurzel_zurueck(marke);
        match vorbereitet {
            Ok((mut a, cors)) => {
                // Grenze M4: hoechstens `max_netz_aufgaben` gleichzeitig.
                if self.netz.len() >= self.grenzen.max_netz_aufgaben {
                    return self.fetch_ablehnen(
                        pr,
                        "Error",
                        "zu viele gleichzeitige Anfragen (Grenze erreicht)",
                    );
                }
                let id = self.naechste_netz_id;
                self.naechste_netz_id += 1;
                a.id = id;
                self.netz.push(NetzAuftrag {
                    id,
                    anfrage: a.clone(),
                    versprechen: pr,
                    gesendet: false,
                    cors,
                });
            }
            Err(msg) => return self.fetch_ablehnen(pr, "TypeError", &msg),
        };
        // DIE ANTWORT KOMMT VON AUSSEN. `fetch` ruft hier bewusst NICHT
        // `netz_schleife` auf: im Browser ist die Antwort eine spaetere
        // Aufgabe, und die Reihenfolge von Mikrotasks und Timern ist
        // messbar. GEMESSEN: mit dem sofortigen Abarbeiten lief die
        // `.then`-Kette VOR dem 0-ms-Timer (`a d c f b` statt
        // `a b c d f`). Der Host ruft `netz_ergebnis` (oder
        // `netz_schleife`) auf, wenn er antwortet.
        Ok(p)
    }

    /// Release 11.10.2026: eine `data:`-URL als Antwort (Status 200, der
    /// MIME-Typ als `Content-Type`); eine kaputte URL ist ein Netzfehler.
    fn fetch_daten_url(&mut self, pr: HeapRef, url: &str) -> Result<GVal, Stoerung> {
        let Some((mime, koerper)) = daten_url_lesen(url) else {
            return self.fetch_ablehnen(pr, "TypeError", "Failed to fetch (data: URL unbrauchbar)");
        };
        let a = NetzAntwort { status: 200, kopf: vec![("content-type".to_string(), mime)], koerper };
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(pr));
        match self.antwort_objekt(&a, url, "basic") {
            Ok(o) => {
                self.wurzeln.push(o);
                self.versprechen_settle(pr, true, o);
            }
            Err(Stoerung::Wurf(w)) => {
                self.wurzeln.push(w);
                self.versprechen_settle(pr, false, w);
            }
            Err(e) => {
                self.wurzel_zurueck(marke);
                return Err(e);
            }
        }
        if self.rahmen.is_empty() {
            self.netz_mikro();
        }
        self.wurzel_zurueck(marke);
        Ok(GVal::Ref(pr))
    }

    /// Das Versprechen mit einem Fehler zurueckgeben (statt zu werfen).
    fn fetch_ablehnen(&mut self, pr: HeapRef, art: &str, text: &str) -> Result<GVal, Stoerung> {
        // Release 09.10.2026: das Versprechen bleibt Wurzel, bis es beim
        // Aufrufer auf dem Stapel liegt, und die Reaktionen laufen NICHT
        // mitten in `fetch()`: Mikrotasks gehoeren an den Punkt, an dem der
        // JS-Stapel leer ist (HTML §8.1.7.3 „perform a microtask checkpoint").
        //
        // GEMESSEN vorher: youtube.com ruft bei abgelehntem `fetch` aus
        // `.catch` erneut `fetch` — jede Ablehnung arbeitete die Schlange
        // synchron ab, die Aufrufe schachtelten sich 500+ Rahmen tief, und
        // ein GC dazwischen raeumte das ungeschuetzte Versprechen ab
        // („Heap: toter Verweis" bei `.then`).
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(pr));
        let f = self.fehler_wert(art, text);
        self.wurzeln.push(f);
        self.versprechen_settle(pr, false, f);
        // Ohne laufendes Skript (Host-Aufruf) gibt es keinen spaeteren
        // Kontrollpunkt — dann jetzt.
        if self.rahmen.is_empty() {
            self.netz_mikro();
        }
        self.wurzel_zurueck(marke);
        Ok(GVal::Ref(pr))
    }

    /// URL aufloesen, Herkunft pruefen, Optionen lesen.
    fn fetch_vorbereiten(
        &mut self,
        url: &str,
        opts: Option<GVal>,
    ) -> Result<(NetzAnfrage, Option<CorsPruefung>), String> {
        if url.len() > MAX_NETZ_URL {
            return Err(format!("URL zu lang (Grenze {MAX_NETZ_URL} Bytes)"));
        }
        let url = url.trim();
        if url.is_empty() {
            return Err("fetch ohne URL".to_string());
        }
        let herkunft = self
            .herkunft
            .clone()
            .ok_or_else(|| "fetch kennt keine Herkunft (mit_herkunft fehlt)".to_string())?;
        // E (06.10.2026): DER GEMEINSAME URL-PARSER statt des
        // handgeschnittenen Pfades.
        //
        // GEMESSEN an der alten Fassung (Masterplan E): der Pfad entstand
        // als `url[url.find('/')..]` — `fetch('daten')` wurde damit zu `/`
        // (der Name fiel weg), `fetch('a/b')` zu `/b`, und
        // `fetch('liste?seite=2')` verlor die Anfrage. `narsil_url::loese_auf`
        // macht die Aufloesung nach RFC 3986 (Verzeichnis, `.`/`..`,
        // Query) — dieselbe Wahrheit ueber URLs wie Adressleiste und
        // Formulare.
        //
        // KEIN `narsil_url::parse` FUER DEN PFAD: das lehnt `https` mit
        // `TlsNotSupported` ab (die URL-Crate kennt TLS absichtlich
        // nicht). Nach der Aufloesung ist das Abtrennen der Autoritaet
        // eine reine Textentscheidung — und der alte Fehler („erster
        // Schraegstrich") kann dabei nicht zurueckkommen, weil die URL
        // schon absolut und normalisiert ist.
        let basis = format!("{}/", herkunft.quelle());
        let absolut =
            narsil_url::loese_auf(&basis, url).map_err(|m| format!("URL unbrauchbar: {m}"))?;
        // Ein Fragment gehoert nicht in die Anfrage.
        let ohne_anker = match absolut.find('#') {
            Some(i) => absolut[..i].to_string(),
            None => absolut.clone(),
        };
        let ziel =
            Herkunft::aus_url(&ohne_anker).map_err(|m| format!("URL unbrauchbar: {m}"))?;
        // DIE HERKUNFTSGRENZE. Vor dem Anlegen der Anfrage — so macht es
        // der Browser, und so kann ein Skript den Host nicht zum Proxy in
        // fremde Netze machen.
        let fremd = ziel != herkunft;
        if fremd && !self.cors_an {
            return Err(format!(
                "Herkunft verboten: {} (erlaubt ist {})",
                ziel.quelle(),
                herkunft.quelle()
            ));
        }
        let pfad = {
            let nach_schema = ohne_anker
                .find("://")
                .map(|i| &ohne_anker[i + 3..])
                .unwrap_or("");
            match nach_schema.find(['/', '?']) {
                // `?x=1` ohne Pfad: der Pfad ist `/`.
                Some(i) if nach_schema[i..].starts_with('?') => {
                    format!("/{}", &nach_schema[i..])
                }
                Some(i) => nach_schema[i..].to_string(),
                None => "/".to_string(),
            }
        };
        let o = self.fetch_opts(opts)?;
        let adresse = format!("{}{}", ziel.quelle(), pfad);
        let mut kopf = o.kopf;
        let mut cors = None;
        if fremd {
            if o.modus == "same-origin" {
                return Err(format!(
                    "fetch: mode 'same-origin' verbietet die fremde Herkunft {} (Failed to fetch)",
                    ziel.als_origin()
                ));
            }
            let einfach = methode_einfach(&o.methode) && kopf.iter().all(|(n, w)| kopf_freigegeben(n, w));
            let ohne_cors = o.modus == "no-cors";
            if ohne_cors && !einfach {
                return Err("fetch: mode 'no-cors' erlaubt nur einfache Anfragen (Failed to fetch)".to_string());
            }
            let origin = herkunft.als_origin();
            let mut unsichere: Vec<String> = kopf
                .iter()
                .filter(|(n, w)| !kopf_freigegeben(n, w))
                .map(|(n, _)| n.to_ascii_lowercase())
                .collect();
            unsichere.sort();
            unsichere.dedup();
            kopf.push(("Origin".to_string(), origin.clone()));
            let vorab = if !einfach && !ohne_cors {
                let mut vk = vec![
                    ("Origin".to_string(), origin.clone()),
                    ("Access-Control-Request-Method".to_string(), o.methode.clone()),
                ];
                if !unsichere.is_empty() {
                    vk.push(("Access-Control-Request-Headers".to_string(), unsichere.join(",")));
                }
                Some(NetzAnfrage {
                    id: 0,
                    url: adresse.clone(),
                    methode: "OPTIONS".to_string(),
                    kopf: vk,
                    koerper: None,
                })
            } else {
                None
            };
            cors = Some(CorsPruefung {
                origin,
                mit_daten: o.mit_daten,
                ohne_cors,
                vorab,
                vorab_fertig: false,
                methode: o.methode.clone(),
                unsichere_koepfe: unsichere,
            });
        }
        Ok((
            NetzAnfrage {
                id: 0,
                url: adresse,
                methode: o.methode,
                kopf,
                koerper: o.koerper,
            },
            cors,
        ))
    }

    /// `{method, headers, body}` lesen (A4 kennt nur diese drei Felder).
    fn fetch_opts(&mut self, opts: Option<GVal>) -> Result<FetchOptionen, String> {
        let mut methode = "GET".to_string();
        let mut kopf: Vec<(String, String)> = Vec::new();
        let mut koerper: Option<Vec<u8>> = None;
        let mut modus = "cors".to_string();
        let mut mit_daten = false;
        let Some(o) = opts else {
            return Ok(FetchOptionen { methode, kopf, koerper, modus, mit_daten });
        };
        let GVal::Ref(r) = o else {
            return Err("fetch: Optionen muessen ein Objekt sein".to_string());
        };
        if let Some(m) = self.eigene_lese(r, "method") {
            let Ok(m) = self.zu_text(m) else {
                return Err("fetch: method ist kein Text".to_string());
            };
            // Release 10.10.2026: alle Methoden, die der Transport kennt
            // (vorher nur GET und POST); CONNECT/TRACE/TRACK sind verboten
            // (Fetch §2.2.1 „forbidden method").
            let m = m.trim().to_ascii_uppercase();
            if matches!(m.as_str(), "CONNECT" | "TRACE" | "TRACK") {
                return Err(format!("fetch: Methode {m} ist verboten"));
            }
            if !matches!(m.as_str(), "GET" | "HEAD" | "POST" | "PUT" | "DELETE" | "PATCH" | "OPTIONS") {
                return Err(format!("fetch: Methode {m} gibt es hier nicht"));
            }
            methode = m;
        }
        // Release 10.10.2026: `mode` und `credentials` (Fetch §5.3).
        if let Some(m) = self.eigene_lese(r, "mode") {
            if let Ok(t) = self.zu_text(m) {
                modus = t.trim().to_ascii_lowercase();
            }
        }
        if let Some(cr) = self.eigene_lese(r, "credentials") {
            if let Ok(t) = self.zu_text(cr) {
                mit_daten = t.trim().eq_ignore_ascii_case("include");
            }
        }
        // Release 10.10.2026: `headers` als `Headers`-Objekt oder Liste von
        // Paaren (vorher nur als einfaches Objekt).
        let kopf_paare: Option<Vec<(String, String)>> = match self.eigene_lese(r, "headers") {
            Some(GVal::Ref(hr)) => match self.heap.holen(hr).clone() {
                HObj::Nativ { name, ziel: Some(z), .. } if name == "Headers" => {
                    let el = match self.heap.holen(z) {
                        HObj::Feld { elemente, .. } => elemente.clone(),
                        _ => Vec::new(),
                    };
                    let mut v = Vec::new();
                    for paar in el.chunks(2) {
                        if let [n, w] = paar {
                            v.push((self.zu_text(*n).unwrap_or_default(), self.zu_text(*w).unwrap_or_default()));
                        }
                    }
                    Some(v)
                }
                HObj::Feld { elemente, .. } => {
                    let mut v = Vec::new();
                    for e in elemente {
                        if let GVal::Ref(pr) = e
                            && let HObj::Feld { elemente: p, .. } = self.heap.holen(pr).clone()
                            && p.len() >= 2
                        {
                            v.push((self.zu_text(p[0]).unwrap_or_default(), self.zu_text(p[1]).unwrap_or_default()));
                        }
                    }
                    Some(v)
                }
                _ => None,
            },
            _ => None,
        };
        if let Some(paare) = kopf_paare {
            if paare.len() > MAX_NETZ_KOPFZEILEN {
                return Err(format!("fetch: zu viele Kopfzeilen (Grenze {MAX_NETZ_KOPFZEILEN})"));
            }
            for (k, w) in paare {
                if w.len() > MAX_NETZ_KOPFZEILE {
                    return Err(format!("fetch: Kopfzeile {k} zu lang (Grenze {MAX_NETZ_KOPFZEILE} Bytes)"));
                }
                if !kopf_verboten(&k) {
                    kopf.push((k, w));
                }
            }
        } else if let Some(GVal::Ref(hr)) = self.eigene_lese(r, "headers") {
            // M3/M4: Die Zahl der Kopfzeilen ist gedeckelt.
            let Ok(schluessel) = self.schluessel_werte(GVal::Ref(hr)) else {
                return Err("fetch: headers ist nicht lesbar".to_string());
            };
            if schluessel.len() > MAX_NETZ_KOPFZEILEN {
                return Err(format!(
                    "fetch: zu viele Kopfzeilen (Grenze {MAX_NETZ_KOPFZEILEN})"
                ));
            }
            for k in schluessel {
                let v = self.eigene_lese(hr, &k).unwrap_or(GVal::Undefiniert);
                let Ok(w) = self.zu_text(v) else {
                    return Err(format!("fetch: Kopfzeile {k} ist kein Text"));
                };
                if w.len() > MAX_NETZ_KOPFZEILE {
                    return Err(format!(
                        "fetch: Kopfzeile {k} zu lang (Grenze {MAX_NETZ_KOPFZEILE} Bytes)"
                    ));
                }
                // Release 10.10.2026: verbotene Koepfe (Cookie, Origin, Host …)
                // setzt ein Skript nicht (Fetch §2.2.2) — sie fallen still weg.
                if !kopf_verboten(&k) {
                    kopf.push((k, w));
                }
            }
        }
        // Release 10.10.2026: der Koerper eines `Request`-Objekts steht in
        // seinem Kontext (`[url, method, headers, body]`).
        let koerper_wert = match self.eigene_lese(r, "body") {
            Some(b) => Some(b),
            None => match self.heap.holen(r).clone() {
                HObj::Nativ { name, ziel: Some(z), .. } if name == "Request" => match self.heap.holen(z) {
                    HObj::Feld { elemente, .. } => elemente.get(3).copied(),
                    _ => None,
                },
                _ => None,
            },
        };
        if let Some(b) = koerper_wert {
            if !matches!(b, GVal::Undefiniert | GVal::Null) {
                // Release 10.10.2026: der Koerper nach Fetch §5.2 (FormData
                // mehrteilig, Blob/Puffer roh) — vorher wurde alles Text,
                // `FormData` ging als „[object FormData]".
                let Ok((bytes, typ)) = self.koerper_extrahieren(b) else {
                    return Err("fetch: body ist nicht lesbar".to_string());
                };
                if bytes.len() > MAX_NETZ_KOERPER {
                    return Err(format!(
                        "fetch: body zu gross (Grenze {MAX_NETZ_KOERPER} Bytes)"
                    ));
                }
                if let Some(t) = typ
                    && !kopf.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                {
                    kopf.push(("Content-Type".to_string(), t));
                }
                koerper = Some(bytes);
            }
        }
        Ok(FetchOptionen { methode, kopf, koerper, modus, mit_daten })
    }

    /// Das `Response`-Objekt bauen.
    ///
    /// Die Daten liegen als **nicht aufzaehlbare** eigene Felder
    /// `__status`, `__kopf`, `__koerper` im Objekt selbst (siehe der
    /// Modulkopf: die erste Fassung hatte eine zweite Tabelle im Motor).
    fn antwort_objekt(&mut self, a: &NetzAntwort, url: &str, art: &str) -> Result<GVal, Stoerung> {
        if a.koerper.len() > self.grenzen.netz_antwort_bytes {
            return Err(Stoerung::Wurf(self.fehler_wert(
                "Error",
                &format!(
                    "Antwort zu gross: {} Bytes (Grenze {})",
                    a.koerper.len(),
                    self.grenzen.netz_antwort_bytes
                ),
            )));
        }
        let kopf_text = kopf_als_text(&a.kopf);
        // M4: Allokation VOR dem Anlegen (Antworttext + Kopftext + 2 Objekte).
        self.vor_allok(a.koerper.len() + kopf_text.len() + 1024)
            .map_err(Stoerung::Fehler)?;
        let koerper = self
            .text_wert(&String::from_utf8_lossy(&a.koerper))
            .map_err(Stoerung::Fehler)?;
        let kopf_wert = self.text_wert(&kopf_text).map_err(Stoerung::Fehler)?;
        // M6: die Zwischenwerte stehen als Wurzeln, waehrend weiter
        // alloziert wird.
        self.wurzeln.push(koerper);
        self.wurzeln.push(kopf_wert);
        let antwort = self
            .neues_objekt(Some(self.proto_antwort))
            .map_err(Stoerung::Fehler)?;
        let kopf_obj = self
            .neues_objekt(Some(self.proto_kopf))
            .map_err(Stoerung::Fehler)?;
        self.wurzeln.push(GVal::Ref(antwort));
        self.wurzeln.push(GVal::Ref(kopf_obj));
        // Status 2xx = ok (wie im Netz, nicht geraten).
        let ok = (200..300).contains(&a.status);
        let status = GVal::Zahl(f64::from(a.status));
        let status_text = self
            .text_wert(&status_text(a.status))
            .map_err(Stoerung::Fehler)?;
        let url_wert = self.text_wert(url).map_err(Stoerung::Fehler)?;
        self.wurzeln.push(status);
        self.wurzeln.push(status_text);
        self.wurzeln.push(url_wert);
        for (n, w) in [
            ("ok", GVal::Bool(ok)),
            ("status", status),
            ("statusText", status_text),
            ("url", url_wert),
            ("bodyUsed", GVal::Bool(false)),
        ] {
            self.setze(GVal::Ref(antwort), n, w)?;
        }
        // Release 10.10.2026: `type` (`basic`, `cors`, `opaque`) und
        // `redirected` (Fetch §6.4) — NICHT aufzaehlbar (in Chrome Getter am
        // Prototyp; `JSON.stringify(antwort)` bleibt wie vorher).
        let art_wert = self.text_wert(art).map_err(Stoerung::Fehler)?;
        self.wurzeln.push(art_wert);
        self.eigenschaft_mit_attributen(antwort, "type", art_wert, false, false)?;
        self.eigenschaft_mit_attributen(antwort, "redirected", GVal::Bool(false), false, false)?;
        self.versteckt_setzen(antwort, "__kopf", kopf_wert)?;
        self.versteckt_setzen(antwort, "__koerper", koerper)?;
        // Release 11.10.2026: ein Koerper, der kein UTF-8 ist (Bilder,
        // Medien, WASM), behaelt seine Bytes als Puffer — der Text oben ist
        // verlustbehaftet. GEMESSEN vorher: `arrayBuffer()` einer WAV-Datei
        // lieferte ein Array aus den Bytes des Ersatztextes (0xEF 0xBF 0xBD
        // statt 0x80), `new Uint8Array(b)` warf „braucht ArrayBuffer".
        if std::str::from_utf8(&a.koerper).is_err() {
            let roh = self.neuer_bytepuffer(a.koerper.len(), false)?;
            if let HObj::BytePuffer { daten, .. } = self.heap.holen_mut(roh) {
                daten.copy_from_slice(&a.koerper);
            }
            self.wurzeln.push(GVal::Ref(roh));
            self.versteckt_setzen(antwort, "__roh", GVal::Ref(roh))?;
        }
        self.versteckt_setzen(kopf_obj, "__kopf", kopf_wert)?;
        self.setze(GVal::Ref(antwort), "headers", GVal::Ref(kopf_obj))?;
        Ok(GVal::Ref(antwort))
    }

    /// `response.text()` -> Versprechen mit dem Text.
    pub(crate) fn n_antwort_text(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let text = self
            .versteckt_lesen(this, "__koerper")
            .unwrap_or(GVal::Undefiniert);
        Ok(self.versprechen_erfuellt(text))
    }

    /// `response.json()` -> Versprechen mit dem geparsten Wert.
    pub(crate) fn n_antwort_json(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let text = self
            .versteckt_lesen(this, "__koerper")
            .unwrap_or(GVal::Undefiniert);
        // node: lehnt mit SyntaxError ab, wirf hier, der Aufrufer macht
        // daraus ein abgelehntes Versprechen (siehe `AntwortJson` im
        // Dispatcher: der Wurf wird vom Aufrufweg gefangen).
        // GEMESSEN/ENTSCHIEDEN: `json()` lehnt mit einem SyntaxError AB
        // (node wirft nicht), darum wird der Wurf hier gefangen und als
        // Ablehnung eingereicht.
        let p = self.versprechen_anlegen(VersprechenZustand::Offen);
        let GVal::Ref(pr) = p else {
            return Ok(p);
        };
        // Release 15.10.2026: the promise and the parsed value stay rooted
        // (until this native returns) — parsing allocates. MEASURED before
        // (chip.de, NARSIL_JS_GC_STRESS=64): `await (await fetch(u)).json()`
        // awaited a swept promise.
        self.wurzeln.push(p);
        self.wurzeln.push(text);
        match self.n_json_lesen(&[text]) {
            Ok(wert) => {
                self.wurzeln.push(wert);
                self.versprechen_settle(pr, true, wert);
            }
            Err(Stoerung::Wurf(w)) => {
                self.wurzeln.push(w);
                self.versprechen_settle(pr, false, w);
            }
            Err(e) => return Err(e),
        }
        self.netz_mikro();
        Ok(p)
    }

    /// `response.arrayBuffer()` -> Versprechen mit einem `ArrayBuffer`
    /// (Fetch §5.3 „consume body"). Release 11.10.2026: ein echter Puffer
    /// (vorher ein Array) aus den Rohbytes, wenn es sie gibt (`__roh`),
    /// sonst aus dem UTF-8 des Textes (dann verlustfrei).
    pub(crate) fn n_antwort_bytes(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let bytes = self.antwort_rohbytes(this);
        let p = self.neuer_bytepuffer(bytes.len(), false)?;
        if let HObj::BytePuffer { daten, .. } = self.heap.holen_mut(p) {
            daten.copy_from_slice(&bytes);
        }
        Ok(self.versprechen_erfuellt(GVal::Ref(p)))
    }

    /// 2026-10-15: `response.bytes()` (Fetch §5.3): a Uint8Array of the body.
    pub(crate) fn n_antwort_bytes_u8(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let bytes = self.antwort_rohbytes(this);
        let buf = self.new_array_buffer(bytes)?;
        // Release 15.10.2026: rooted while the view is built.
        self.wurzeln.push(buf);
        let out = self.uint8_array_over(buf)?;
        Ok(self.versprechen_erfuellt(out))
    }

    /// Release 15.10.2026: `response.blob()` (Fetch §5.3 "consume body"):
    /// a `Blob` of the raw bytes with the response's `Content-Type`.
    /// MEASURED before: 333 WPT canvas tests rejected with "blob is not a
    /// function" (they fetch an image and decode it via createImageBitmap).
    pub(crate) fn n_antwort_klecks(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let bytes = self.antwort_rohbytes(this);
        let headers = self.hole(this, "headers").unwrap_or(GVal::Undefiniert);
        let ct = self.text_wert("content-type").map_err(Stoerung::Fehler)?;
        let typ = match self.n_kopf_lesen(headers, &[ct], false) {
            Ok(v @ GVal::Ref(_)) => self.zu_text(v).unwrap_or_default(),
            _ => String::new(),
        };
        let Some(proto) = self.schnittstellen_prototyp("Blob") else {
            return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Blob is not available")));
        };
        let typ = typ.to_ascii_lowercase();
        let b = self.klecks_bauen(proto, &bytes, typ.split(';').next().unwrap_or("").trim())?;
        Ok(self.versprechen_erfuellt(b))
    }

    /// The raw body bytes of a fetch response.
    fn antwort_rohbytes(&mut self, this: GVal) -> Vec<u8> {
        match self.versteckt_lesen(this, "__roh") {
            Some(GVal::Ref(r)) if matches!(self.heap.holen(r), HObj::BytePuffer { .. }) => match self.heap.holen(r) {
                HObj::BytePuffer { daten, .. } => daten.clone(),
                _ => Vec::new(),
            },
            _ => {
                let text = self
                    .versteckt_lesen(this, "__koerper")
                    .unwrap_or(GVal::Undefiniert);
                self.zu_text(text).unwrap_or_default().into_bytes()
            }
        }
    }

    /// `headers.get(name)` / `headers.entries()`.
    pub(crate) fn n_kopf_lesen(
        &mut self,
        this: GVal,
        args: &[GVal],
        alle: bool,
    ) -> Result<GVal, Stoerung> {
        let name = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let text = self
            .versteckt_lesen(this, "__kopf")
            .and_then(|t| self.zu_text(t).ok())
            .unwrap_or_default();
        let such = name.trim().to_ascii_lowercase();
        let mut erster: Option<String> = None;
        let mut paare: Vec<(String, String)> = Vec::new();
        for zeile in text.lines().take(MAX_NETZ_KOPFZEILEN) {
            let Some((n, w)) = zeile.split_once(':') else {
                continue;
            };
            // `entries()` nimmt ALLE Paare, `get(name)` nur die passenden.
            if alle || n.trim().eq_ignore_ascii_case(&such) {
                let w = w.trim().to_string();
                if erster.is_none() {
                    erster = Some(w.clone());
                }
                paare.push((n.trim().to_ascii_lowercase(), w));
            }
        }
        if !alle {
            return match erster {
                Some(w) => self.text_wert(&w).map_err(Stoerung::Fehler),
                None => Ok(GVal::Null),
            };
        }
        // `entries()`: ein Feld aus [name, wert]-Paaren.
        let feld = self.neues_feld().map_err(Stoerung::Fehler)?;
        self.wurzeln.push(GVal::Ref(feld));
        for (n, w) in paare {
            let paar = self.neues_feld().map_err(Stoerung::Fehler)?;
            let a = self.text_wert(&n).map_err(Stoerung::Fehler)?;
            let b = self.text_wert(&w).map_err(Stoerung::Fehler)?;
            self.wurzeln.push(a);
            self.wurzeln.push(b);
            for v in [a, b] {
                let o = self.heap.holen_mut(paar);
                if let HObj::Feld { elemente, .. } = o {
                    elemente.push(v);
                }
            }
            let o = self.heap.holen_mut(feld);
            if let HObj::Feld { elemente, .. } = o {
                elemente.push(GVal::Ref(paar));
            }
        }
        Ok(GVal::Ref(feld))
    }
}

impl Engine {
    /// Ein verstecktes Feld (`__…`) lesen; `None`, wenn `this` kein Objekt
    /// ist oder das Feld fehlt.
    fn versteckt_lesen(&mut self, this: GVal, name: &str) -> Option<GVal> {
        let GVal::Ref(r) = this else {
            return None;
        };
        self.eigene_lese(r, name)
    }

    /// Ein Feld setzen, das NICHT aufzaehlbar ist — so wie `constructor`
    /// und die `RegExp`-Felder. Es taucht nicht in `Object.keys(r)` und
    /// nicht in `JSON.stringify(r)` auf, bleibt aber fuer `headers`/
    /// `response` erreichbar.
    ///
    /// GEMESSEN/ENTSCHIEDEN: ohne `aufzählbar: false` zeigte
    /// `Object.keys(r)` die Maschinendaten des `Response`.
    pub(crate) fn versteckt_setzen(
        &mut self,
        obj: HeapRef,
        name: &str,
        wert: GVal,
    ) -> Result<(), Stoerung> {
        let kosten = crate::heap::KOSTEN_EINTRAG;
        self.vor_allok(kosten).map_err(Stoerung::Fehler)?;
        // J21: BEIDE Werte parken, solange hier alloziert wird — `vor_allok`
        // kann sammeln, und `holen_mut` braucht einen lebenden `obj`.
        // GEMESSEN/GEFUNDEN (Abnahme 11:07, wikipedia.org): vorher war nur
        // `wert` geparkt — und **ohne Ruecknahme**, jeder Knoten also 8
        // Werte dauerhaft im Wurzelstapel. `obj` war gar nicht geparkt: der
        // Sweep raeumte den Slot des Knotens, und `holen_mut` sagte
        // „Heap: toter Verweis (interner Fehler)".
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(obj));
        self.wurzeln.push(wert);
        let r = self.versteckt_setzen_innen(obj, name, wert);
        self.wurzel_zurueck(marke);
        r
    }

    /// Der Teil von `versteckt_setzen` **ohne** Wurzelverwaltung (die
    /// aufrufende Funktion hat `obj` und `wert` bereits geparkt).
    fn versteckt_setzen_innen(
        &mut self,
        obj: HeapRef,
        name: &str,
        wert: GVal,
    ) -> Result<(), Stoerung> {
        let kosten = crate::heap::KOSTEN_EINTRAG;
        let o = self.heap.holen_mut(obj);
        // J5: auch ein **Feld** traegt versteckte Felder (der Zaehler eines
        // Feld-Iterators liegt in `__pos`). GEMESSEN: `a.entries().next()`
        // scheiterte an „kein Objekt fuer die Antwort" — die Meldung kam
        // daher, dass hier nur `Gegenstand` zugelassen war.
        let eigen: &mut crate::heap::Karte = match o {
            HObj::Gegenstand { eigenschaften, .. } => eigenschaften,
            HObj::Feld { rest, .. } => rest,
            _ => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "kein Objekt fuer die Antwort"),
                ));
            }
        };
        eigen.insert(
            name.to_string(),
            crate::heap::Eigenschaft {
                wert,
                getter: None,
                setter: None,
                aufzählbar: false,
                beschreibbar: true,
                konfigurierbar: true,
            },
        );
        self.heap.kosten_verbuchen(kosten);
        Ok(())
    }
}

/// Kopfzeilen in die Form „Name: Wert" je Zeile (was `__kopf` haelt).
///
/// M3: hoechstens [`MAX_NETZ_KOPFZEILEN`] Zeilen, jede hoechstens
/// [`MAX_NETZ_KOPFZEILE`] Bytes — der Text kann also nicht von der
/// Laenge eines einzelnen Kopfes abhaengen.
fn kopf_als_text(kopf: &[(String, String)]) -> String {
    let mut s = String::new();
    for (n, w) in kopf.iter().take(MAX_NETZ_KOPFZEILEN) {
        if n.len() + w.len() > MAX_NETZ_KOPFZEILE {
            continue;
        }
        s.push_str(n);
        s.push_str(": ");
        s.push_str(w);
        s.push('\n');
    }
    s
}

/// Die kurzen Statustexte (Browser/node haben dieselben Formen).
/// Unbekannter Code -> `""`.
fn status_text(code: u16) -> String {
    match code {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        410 => "Gone",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
    .to_string()
}
