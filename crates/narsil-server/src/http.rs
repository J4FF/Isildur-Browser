//! HTTP/1.1 in reinem Rust: Anfrage parsen, Antwort schreiben.
//!
//! Kein Client (das ist `narsil-net`), sondern die Gegenseite: was auf
//! dem Socket ankommt, wird [`Request`], was zurueckgeht, ist
//! [`Response`]. Beide kennen nur Bytes und Strings — kein Router,
//! kein Dateisystem, kein JSON. Die Trennung ist der ganze Auftrag:
//! ein Parser, der nebenbei routet, ist kein Parser.
//!
//! Limits: [`crate::MAX_HEADER_BYTES`] (431 darueber),
//! [`crate::MAX_BODY_BYTES`] (413 darueber).

use std::collections::HashMap;
// `write!` in den Kopfpuffer braucht `fmt::Write` (`io::Write` ist
// fuer Bytes zustaendig). Beide Merkmale muessen im Spielraum sein —
// daher der importierte Name unter einem anderen Namen.
use std::fmt::Write as FmtWrite;
use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

use crate::{
    MAX_ANFRAGE_DAUER, MAX_ANFRAGEZEILE, MAX_BODY_BYTES, MAX_CHUNKS, MAX_HEADER_ANZAHL,
    MAX_HEADER_BYTES, MAX_LEERE_ZEILEN, MAX_TRAILER,
};

/// HTTP-Methode. Nur was der Router versteht — der Rest ist `Unbekannt`
/// und bekommt `501`, nicht `404`. Eine unbekannte Methode ist kein
/// fehlender Pfad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    Get,
    Head,
    Post,
    Put,
    Delete,
    Options,
    /// Alles andere (`PATCH`, `TRACE`, Tippfehler). Der Name steht drin,
    /// damit die Antwort ihn nennen kann.
    Unbekannt,
}

impl Method {
    /// Aus der Anfragezeile. Case-sensitiv per RFC 9110 — `get` ist
    /// keine Methode, sondern ein Tippfehler mit Antwort.
    pub fn aus_str(s: &str) -> Self {
        match s {
            "GET" => Method::Get,
            "HEAD" => Method::Head,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "DELETE" => Method::Delete,
            "OPTIONS" => Method::Options,
            _ => Method::Unbekannt,
        }
    }

    /// Zurueck nach draussen (Request-Zeile gibt es serverseitig nicht,
    /// aber Logs und Tests brauchen den Namen).
    pub fn als_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Head => "HEAD",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
            Method::Options => "OPTIONS",
            Method::Unbekannt => "UNBEKANNT",
        }
    }

    /// Nur GET und HEAD sind ohne Body lesbar. POST/PUT/DELETE duerfen
    /// einen tragen — ob einer da ist, entscheidet `Content-Length`.
    pub fn erwartet_body(self) -> bool {
        matches!(self, Method::Post | Method::Put | Method::Delete)
    }
}

/// HTTP-Status. Die Zahl steht drin, der Grund daneben — beides wird
/// auf die Leitung geschrieben, nicht nur die Zahl.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status(pub u16);

impl Status {
    pub const OK: Status = Status(200);
    pub const PARTIAL: Status = Status(206);
    pub const CREATED: Status = Status(201);
    pub const NO_CONTENT: Status = Status(204);
    pub const NOT_MODIFIED: Status = Status(304);
    pub const BAD_REQUEST: Status = Status(400);
    pub const UNAUTHORIZED: Status = Status(401);
    pub const FORBIDDEN: Status = Status(403);
    pub const NOT_FOUND: Status = Status(404);
    pub const METHOD_NOT_ALLOWED: Status = Status(405);
    pub const REQUEST_TIMEOUT: Status = Status(408);
    pub const PAYLOAD_TOO_LARGE: Status = Status(413);
    pub const URI_TOO_LONG: Status = Status(414);
    pub const RANGE_NOT_SATISFIABLE: Status = Status(416);
    pub const HEADER_TOO_LARGE: Status = Status(431);
    pub const SERVICE_UNAVAILABLE: Status = Status(503);
    pub const NOT_IMPLEMENTED: Status = Status(501);
    pub const INTERNAL_ERROR: Status = Status(500);

    /// Der Grund zur Zahl, wie ihn ein Browser in der Statuszeile zeigt.
    pub fn grund(self) -> &'static str {
        match self.0 {
            101 => "Switching Protocols",
            200 => "OK",
            201 => "Created",
            204 => "No Content",
            206 => "Partial Content",
            301 => "Moved Permanently",
            302 => "Found",
            304 => "Not Modified",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            408 => "Request Timeout",
            413 => "Payload Too Large",
            414 => "URI Too Long",
            416 => "Range Not Satisfiable",
            431 => "Request Header Fields Too Large",
            500 => "Internal Server Error",
            501 => "Not Implemented",
            503 => "Service Unavailable",
            _ => "Unbekannt",
        }
    }

    /// 2xx.
    pub fn ist_erfolg(self) -> bool {
        (200..300).contains(&self.0)
    }
}

/// Grenzen EINER Anfrage. Alle Zahlen kommen aus Konstanten (M3), nicht
/// aus der Eingabe — wer sie setzt, muss es begruenden (Tests tun das,
/// um Slowloris in unter einer Sekunde abfangen zu koennen).
#[derive(Debug, Clone)]
pub struct Limits {
    /// Anfragezeile (`GET /pfad HTTP/1.1`) in Bytes.
    pub max_zeile: usize,
    /// Eine einzelne Header-Zeile in Bytes (kann nie mehr als die
    /// Gesamtsumme betragen, verhindert aber eine Riesen-Zeile).
    pub max_header_zeile: usize,
    /// Gesamtgroesse aller Header in Bytes.
    pub max_header_bytes: usize,
    /// Anzahl der Header.
    pub max_header_anzahl: usize,
    /// Body-Groesse in Bytes (auch chunked).
    pub max_body: usize,
    /// Anzahl der Chunks.
    pub max_chunks: usize,
    /// Anzahl der Trailer-Zeilen.
    pub max_trailer: usize,
    /// Gesamtdauer einer Anfrage von der ersten bis zur letzten Zeile.
    pub max_dauer: Duration,
}

impl Limits {
    /// Die Grenzen der Bibliothek (Konstanten aus [`crate`]).
    pub fn standard() -> Self {
        Limits {
            max_zeile: MAX_ANFRAGEZEILE,
            max_header_zeile: MAX_HEADER_BYTES,
            max_header_bytes: MAX_HEADER_BYTES,
            max_header_anzahl: MAX_HEADER_ANZAHL,
            max_body: MAX_BODY_BYTES,
            max_chunks: MAX_CHUNKS,
            max_trailer: MAX_TRAILER,
            max_dauer: MAX_ANFRAGE_DAUER,
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Limits::standard()
    }
}

/// Wie die Verbindung NACH dieser Antwort behandelt wird.
///
/// `write_response` (die alte Funktion) schreibt immer `Schliessen` —
/// damit bleiben alle bisherigen Tests und Aufrufer unveraendert. Der
/// Keep-Alive-Pfad des Servers benutzt [`write_response_mit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verbindung {
    /// `Connection: close` — Verbindung nach der Antwort zu.
    Schliessen,
    /// `Connection: keep-alive` mit Anzahl der noch erlaubten Anfragen.
    Halten { uebrig: usize },
}

/// Eine geparste HTTP-Anfrage.
///
/// `headers` sind kleingeschrieben (RFC 9110: Feldnamen sind
/// case-insensitiv). `query` ist der Teil nach `?` als Rohstring —
/// das Parsen in Paare macht [`crate::api::query_paare`], nicht der
/// Parser. `params` fuellt der ROUTER, nicht der Parser: ein Parser,
/// der Routen kennt, ist kein Parser.
#[derive(Debug, Clone)]
pub struct Request {
    /// Methode aus der Anfragezeile.
    pub methode: Method,
    /// Roh-Methode wie getippt (fuer `501`-Meldungen mit Namen).
    pub methode_roh: String,
    /// Pfad ohne Query (`/seite`, immer mit fuehrendem `/`).
    pub pfad: String,
    /// Query roh (`a=1&b=2`), ohne `?`. Leer wenn keine da ist.
    pub query: String,
    /// HTTP-Version wie angegeben (`HTTP/1.1`).
    pub version: String,
    /// Header, Namen kleingeschrieben.
    pub headers: HashMap<String, String>,
    /// Body-Bytes (kann leer sein).
    pub body: Vec<u8>,
    /// Pfadparameter aus dem Router (`:id` -> Wert). Der Parser laesst
    /// sie leer; wer sie liest, ohne dass der Router sie gesetzt hat,
    /// bekommt `None`, nicht einen Fehler.
    pub params: HashMap<String, String>,
    /// Peer-Adresse als String (fuer Logs). Leer wenn unbekannt.
    pub peer: String,
}

impl Request {
    /// Header lesen (case-insensitiv, Namen werden kleingeschrieben).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    /// Pfadparameter lesen (vom Router gesetzt).
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(String::as_str)
    }

    /// Body als UTF-8-Text, wenn er einer ist.
    pub fn body_text(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.body)
    }

    /// Ob der Client die Verbindung nach der Antwort schliessen will.
    ///
    /// HTTP/1.1 heisst laut RFC 9112 Standard: dauerhaft, bis `close`
    /// kommt. HTTP/1.0 heisst umgekehrt: zu, bis `keep-alive` kommt.
    /// Der Server fragt das hier ab, statt es zu raten — und schreibt
    /// die Antwort entsprechend (siehe [`Verbindung`]).
    pub fn will_schliessen(&self) -> bool {
        let verb = self.header("connection").unwrap_or("");
        let klein = verb.to_ascii_lowercase();
        if klein.split(',').any(|t| t.trim() == "close") {
            return true;
        }
        if klein.split(',').any(|t| t.trim() == "keep-alive") {
            return false;
        }
        // Ohne `Connection`: 1.0 zu, 1.1 offen. Alles andere (z. B.
        // HTTP/2-Gerede in einer Zeile) behandeln wir wie 1.0 — lieber
        // zu als offen, wenn wir die Version nicht verstehen.
        !self.version.starts_with("HTTP/1.1")
    }
}

/// Eine HTTP-Antwort, wie der Handler sie baut.
///
/// Der Handler baut sie, [`write_response`] schreibt sie. Dazwischen
/// liegt keine weitere Entscheidung: was hier steht, geht auf die
/// Leitung.
#[derive(Debug, Clone)]
pub struct Response {
    /// Statuscode.
    pub status: Status,
    /// Header (Namen wie gesetzt, werden so geschrieben).
    pub headers: Vec<(String, String)>,
    /// Body-Bytes.
    pub body: Vec<u8>,
    /// Statt Bytes: eine Datei mit Bytebereich, die BEIM SCHREIBEN in
    /// Blöcken gelesen wird (Regel M4: die Groesse kommt aus der
    /// Eingabe/dem Dateisystem und wird deshalb NIE in einen
    /// zusammenhaengenden Vec gelesen — ein 5-GB-Video wuerde sonst
    /// der Speicher der Maschine sein).
    pub datei: Option<DateiLesen>,
}

/// Eine Datei, die als Antwort-Koerper in Bloecken gelesen wird.
#[derive(Debug, Clone)]
pub struct DateiLesen {
    /// Pfad (innerhalb der Wurzel — der Traversal-Schutz liefert ihn).
    pub pfad: std::path::PathBuf,
    /// Erstes Byte (Range-Anfang).
    pub offset: u64,
    /// Anzahl der Bytes (>= 0).
    pub laenge: u64,
    /// Bereits geoeffnete, gepruefte Datei. Statische Routen setzen dieses
    /// Handle, damit ein spaeterer Symlink-Tausch den Inhalt nicht aendert.
    pub quelle: Option<std::sync::Arc<std::sync::Mutex<std::fs::File>>>,
}

/// Lesepro-Koerper-Groesse in Bytes.
///
/// Feste Groesse statt `min(64 KiB, laenge)` je Block: Allokationen mit
/// festem oberen Rand sind unschaedlich, `laenge`-abhaengige wuerden
/// erst geprueft werden muessen (M4).
const LESE_BLOCK: u64 = 64 * 1024;

impl Response {
    /// Leere Antwort mit Status.
    pub fn status(code: u16) -> Self {
        Response {
            status: Status(code),
            headers: Vec::new(),
            body: Vec::new(),
            datei: None,
        }
    }

    /// Textantwort mit `Content-Type` und `Content-Length` (wird beim
    /// Schreiben gesetzt, nicht hier — hier steht nur der Typ).
    pub fn text(code: u16, text: impl Into<String>) -> Self {
        let body = text.into().into_bytes();
        Response {
            status: Status(code),
            headers: vec![("Content-Type".into(), "text/plain; charset=utf-8".into())],
            body,
            datei: None,
        }
    }

    /// HTML-Antwort.
    pub fn html(code: u16, html: impl Into<String>) -> Self {
        let body = html.into().into_bytes();
        Response {
            status: Status(code),
            headers: vec![("Content-Type".into(), "text/html; charset=utf-8".into())],
            body,
            datei: None,
        }
    }

    /// Narsil-Antwort (eigene Sprache, eigener Content-Type).
    pub fn narsil(code: u16, quelle: impl Into<String>) -> Self {
        let body = quelle.into().into_bytes();
        Response {
            status: Status(code),
            headers: vec![("Content-Type".into(), "text/narsil; charset=utf-8".into())],
            body,
            datei: None,
        }
    }

    /// JSON-Antwort. Der String muss kein geparstes JSON sein — wer
    /// garantiert valides JSON will, baut es mit [`crate::json`].
    /// Der Content-Type steht trotzdem, weil der Client danach fragt.
    pub fn json(code: u16, json_text: impl Into<String>) -> Self {
        let body = json_text.into().into_bytes();
        Response {
            status: Status(code),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body,
            datei: None,
        }
    }

    /// Umleitung (302) mit `Location`.
    pub fn umleitung(nach: &str) -> Self {
        Response {
            status: Status(302),
            headers: vec![("Location".into(), nach.to_string())],
            body: Vec::new(),
            datei: None,
        }
    }

    /// Fehlerantwort als Text (400–599). Der Body nennt den Grund —
    /// ein leerer Fehler ist schlimmer als ein lauter.
    pub fn fehler(code: u16, grund: impl Into<String>) -> Self {
        let text: String = grund.into();
        let body = if text.is_empty() {
            format!("Fehler {}", Status(code).grund())
        } else {
            text
        }
        .into_bytes();
        Response {
            status: Status(code),
            headers: vec![("Content-Type".into(), "text/plain; charset=utf-8".into())],
            body,
            datei: None,
        }
    }

    /// Header anhaengen (darf doppelt vorkommen, z. B. `Set-Cookie`).
    pub fn mit_header(mut self, name: &str, wert: &str) -> Self {
        self.headers.push((name.to_string(), wert.to_string()));
        self
    }

    /// Header-Wert lesen (erster Treffer, case-insensitiv).
    pub fn header(&self, name: &str) -> Option<&str> {
        let gesucht = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| k.to_ascii_lowercase() == gesucht)
            .map(|(_, v)| v.as_str())
    }
}

/// Fehler beim Lesen einer Anfrage. Jede Variante hat eine Antwort:
/// der Server antwortet, statt die Verbindung still zu schliessen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpFehler {
    /// Leere Verbindung (Client hat nichts geschickt).
    Leer,
    /// Leerlauf-Timeout: innerhalb der erlaubten Zeit kam keine
    /// Anfrage. Der Server schliesst HIER still (keine Antwort auf
    /// eine Anfrage, die nie kam) — die Variante steht trotzdem drin,
    /// damit der Grund benannt ist und nicht geraten.
    Leerlauf,
    /// Gesamtdauer der Anfrage ueberschritten (Slowloris).
    Zeitlimit,
    /// Eine Zeile war laenger als ihr Limit (Anfragezeile -> `414`,
    /// Header-Zeile -> vom Aufrufer als `431` gemeldet).
    ZeileZuLang,
    /// Anfragezeile nicht lesbar (`METHODE PFAD VERSION`).
    Anfragezeile(String),
    /// Methode/Version ungueltig.
    Ungueltig(String),
    /// Header zu gross (> 16 KiB).
    HeaderZuGross,
    /// Zu viele Header (> [`crate::MAX_HEADER_ANZAHL`]).
    HeaderZuViele,
    /// Body zu gross (> 10 MiB).
    BodyZuGross,
    /// `Content-Length` keine Zahl.
    LaengeUngueltig(String),
    /// Lesefehler (Details im String, kein `io::Error` nach aussen).
    Lesen(String),
}

impl std::fmt::Display for HttpFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpFehler::Leer => write!(f, "leere Anfrage"),
            HttpFehler::Leerlauf => write!(f, "Leerlauf-Timeout: keine Anfrage"),
            HttpFehler::Zeitlimit => {
                write!(f, "Anfrage nicht in {MAX_ANFRAGE_DAUER:?} gelesen")
            }
            HttpFehler::ZeileZuLang => write!(f, "Zeile laenger als erlaubt"),
            HttpFehler::Anfragezeile(s) => write!(f, "Anfragezeile nicht lesbar: {s:?}"),
            HttpFehler::Ungueltig(s) => write!(f, "ungueltige Anfrage: {s}"),
            HttpFehler::HeaderZuGross => write!(f, "Header groesser als {MAX_HEADER_BYTES} Bytes"),
            HttpFehler::HeaderZuViele => {
                write!(f, "mehr als {MAX_HEADER_ANZAHL} Header")
            }
            HttpFehler::BodyZuGross => write!(f, "Body groesser als {MAX_BODY_BYTES} Bytes"),
            HttpFehler::LaengeUngueltig(s) => write!(f, "Content-Length ungueltig: {s:?}"),
            HttpFehler::Lesen(s) => write!(f, "Lesefehler: {s}"),
        }
    }
}

impl std::error::Error for HttpFehler {}

impl HttpFehler {
    /// Die Antwort, die der Server auf diesen Fehler gibt.
    pub fn als_antwort(&self) -> Response {
        match self {
            HttpFehler::HeaderZuGross | HttpFehler::HeaderZuViele => {
                Response::fehler(431, self.to_string())
            }
            HttpFehler::ZeileZuLang => Response::fehler(414, self.to_string()),
            HttpFehler::Zeitlimit => Response::fehler(408, self.to_string()),
            HttpFehler::BodyZuGross | HttpFehler::LaengeUngueltig(_) => {
                Response::fehler(413, self.to_string())
            }
            _ => Response::fehler(400, self.to_string()),
        }
    }
}

/// Liest EINE Anfrage von einem Stream (Buffer + Timeout setzt der Server).
///
/// Ablauf: Zeile -> Header -> Body (`Content-Length`, sonst leer).
/// `Transfer-Encoding: chunked` wird gelesen (wie `narsil-net`), weil
/// echte Clients es schicken — ein Server, der es ablehnt, ist gegen
/// die Haelfte des Netzes taub.
///
/// JEDE Grenze kommt aus [`Limits`], jede Schleife hat eine Obergrenze
/// und die GANZE Anfrage laeuft gegen eine Uhr ([`Limits::max_dauer`]),
/// denn Lese-Timeouts allein halten keinen Slowloris auf.
pub fn parse_request<R: BufRead>(leser: &mut R, peer: &str) -> Result<Request, HttpFehler> {
    parse_request_mit(leser, peer, &Limits::standard())
}

/// Wie [`parse_request`] mit eigenen Grenzen (fuer Tests mit kurzen
/// Fristen — der Produktionswert ist [`Limits::standard`]).
///
/// DIESER WEG ALLOKIERT JE ANFRAGE: eine neue `HashMap`, neue Strings
/// und einen neuen Body-Vec. Auf einer Verbindung mit 1.000 Anfragen
/// sind das 1.000 Tabellen. Der Server benutzt deshalb
/// [`parse_request_aus`] mit einem [`LesePuffer`]; diese Funktion bleibt
/// fuer Einzelabfragen (Tests, Werkzeuge, ein Aufruf pro Verbindung).
pub fn parse_request_mit<R: BufRead>(
    leser: &mut R,
    peer: &str,
    lim: &Limits,
) -> Result<Request, HttpFehler> {
    let mut puffer = LesePuffer::neu(peer);
    parse_request_aus(leser, &mut puffer, peer, lim)?;
    Ok(puffer.anfrage)
}

/// Die wiederverwendbaren Puffer EINES Verbindungszyklus.
///
/// added 01.10.2026, SERVER V1 (S3: "feste Puffer ohne Allokation je
/// Anfrage"). DER GRUND FUER EIGENEN TYP: `Request` ist oeffentlich und
/// wird von aussen mit Struct-Literal gebaut (`tests/schutz.rs`,
/// `protokoll.rs`). Ein zusaetzliches Feld — auch nur ein privates —
/// wuerde diese Literale brechen. Der Puffer gehoert deshalb **neben**
/// die Anfrage, nicht in sie.
///
/// WAS WIEDERVERWENDET WIRD:
/// * [`anfrage`](LesePuffer::anfrage) — alle Felder. `String::clear` und
///   `Vec::clear` lassen die Kapazitaet stehen, `HashMap::clear` laesst die
///   Tabelle stehen. Nach der ersten Anfrage kostet das Lesen einer
///   Anfrage also **keine** dieser Allokationen mehr.
/// * `zeile` — ein einziger Kratzer fuer Anfragezeile, Kopfzeilen und
///   Chunk-Groessen (die drei brauchen nie gleichzeitig Platz).
/// * `vorrat` — die Kopfzeilen der **vorigen** Anfrage als Besitz. Der
///   Parser holt sie mit `swap_remove` heraus (Name und Wert wandern,
///   es wird nichts neu gebaut) und legt am Ende die unbenutzten zurueck.
///   Ein Header, der auf dieser Verbindung schon einmal kam, kostet damit
///   null Allokationen; nur ein **neuer** Headername baut einen String.
///
/// WARUM EIN `Vec` UND KEINE `HashMap`: das Nachsehen muss
/// gross-/kleinschreibungsfrei sein (`Host` und `host` sind derselbe
/// Schluessel), und eine `HashMap` kann nur nach **exakter** Uebereinstimmung
/// suchen. Der Weg waere `keys().find(..).cloned()` — und `cloned()` ist
/// genau die Allokation, die hier vermieden werden soll. Die lineare
/// Suche ist billig und **begrenzt**: sie laeuft ueber die Kopfzeilen der
/// letzten Anfrage, und deren Zahl ist durch [`Limits::max_header_anzahl`]
/// (200) begrenzt.
///
/// DIE GRENZEN SIND DIESELBEN wie ohne Puffer ([`Limits`]): der Puffer
/// haelt hoechstens das, was eine einzelne Anfrage ohnehin braucht
/// (doppelt: Arbeitskopie und Vorrat), er vergroessert nichts.
pub struct LesePuffer {
    /// Die gepufferte Anfrage. Nach [`parse_request_aus`] steht in ihr
    /// die gerade gelesene Anfrage.
    pub anfrage: Request,
    /// Kratzer fuer eine Zeile (Anfragezeile, Kopfzeile, Chunk-Groesse).
    zeile: String,
    /// Kopfzeilen der vorigen Anfrage, unbenutzt zurueckgegeben.
    vorrat: Vec<(String, String)>,
    /// Wie oft ein Container **wirklich** Speicher anfordern musste.
    ///
    /// added 01.10.2026, SERVER V1 (S3). DER WEG, DIE ZAHL ZU BEKOMMEN
    /// OHNE `unsafe`: `std` hat keinen mitgelieferten Allokatorzaehler,
    /// und einen eigenen zu bauen heisst `unsafe impl GlobalAlloc` —
    /// was der Workspace verbietet (`unsafe_code = "forbid"`). Der
    /// Container selbst sagt es aber: **vorher** die Kapazitaet merken,
    /// **`push_str`/`insert`/`resize`**, **nachher** wieder — ist sie
    /// gestiegen, hat der Allokator gearbeitet. Das ist eine **untere**
    /// Schranke (der Allokator kann auch ohne Kapazitaetszuwachs
    /// kopieren), aber es ist die ehrliche: sie zaehlt genau die
    /// Stellen, an denen ein `Vec`/`String`/`HashMap` seinen Puffer
    /// verlassen musste.
    gewachsen: u64,
    /// Wie oft ein Kopfzeilen-Name **neu gebaut** wurde (2 Strings:
    /// Name und Wert).
    neu: u64,
}

impl LesePuffer {
    /// Ein leerer Puffer fuer die Verbindung `peer`.
    pub fn neu(peer: &str) -> Self {
        LesePuffer {
            anfrage: Request::leer(peer),
            zeile: String::new(),
            vorrat: Vec::new(),
            gewachsen: 0,
            neu: 0,
        }
    }

    /// Wie viele Kopfzeilen im Vorrat liegen (unbenutzt, unterwegs zu
    /// `headers`).
    ///
    /// added 01.10.2026, SERVER V1. DER GRUND FUER DIE ZAHL: sie ist das
    /// messbare Zeugnis der Wiederverwendung. Ein Test liest sie nach
    /// 1.000 Anfragen ab: zusammen mit `headers` ist das die Zahl der
    /// **verschiedenen** Kopfzeilen, die auf der Verbindung vorkamen —
    /// und die darf nicht mit der Zahl der Anfragen wachsen.
    pub fn vorrat_anzahl(&self) -> usize {
        self.vorrat.len()
    }

    /// Wie oft ein Container Speicher anfornen musste, und wie oft ein
    /// Kopfzeilen-Name neu gebaut wurde.
    ///
    /// added 01.10.2026, SERVER V1 (S3). DAS IST DIE ABNAHMEZAHL: nach
    /// der ersten Anfrage muessten beide **null** bleiben, solange die
    /// Anfragen gleich gross sind. Der Wert waechst nur, wenn eine
    /// Anfrage groesser wird als alles davor — das ist dann echte
    /// Arbeit und kein Fehler.
    pub fn allokationen(&self) -> (u64, u64) {
        (self.gewachsen, self.neu)
    }

    /// Setzt beide Zaehler zurueck (damit ein Test den **zweiten**
    /// Abschnitt zaehlt und nicht den ersten mit).
    pub fn allokationen_zuruecksetzen(&mut self) {
        self.gewachsen = 0;
        self.neu = 0;
    }

    /// Wie viele Bytes die Kopfzeilen des Vorrats belegen.
    ///
    /// added 01.10.2026, SERVER V1 (M4: die Grenze ist belegbar). Ohne
    /// diese Zahl koennte jemand behaupten, der Vorrat sei klein —
    /// belegbar ist er durch [`Limits::max_header_bytes`] (<= 16 KiB),
    /// weil `vorrat` die Heads der **letzten** Anfrage enthaelt und
    /// sonst nichts.
    pub fn vorrat_bytes(&self) -> usize {
        self.vorrat
            .iter()
            .map(|(n, w)| n.len() + w.len())
            .fold(0usize, usize::saturating_add)
    }
}

impl Request {
    /// Eine leere Anfrage (alle Felder leer, `peer` gesetzt).
    ///
    /// added 01.10.2026, SERVER V1 (S3). Der Weg dorthin ist
    /// [`LesePuffer`]: einmal [`Request::leer`], danach immer
    /// [`parse_request_aus`].
    pub fn leer(peer: &str) -> Request {
        Request {
            methode: Method::aus_str(""),
            methode_roh: String::new(),
            pfad: String::new(),
            query: String::new(),
            version: String::new(),
            headers: HashMap::new(),
            body: Vec::new(),
            params: HashMap::new(),
            peer: peer.to_string(),
        }
    }
}

/// Liest eine Anfrage in einen vorhandenen [`LesePuffer`].
///
/// added 01.10.2026, SERVER V1 (S3). DER UNTERSCHIED ZU
/// [`parse_request_mit`]: es wird nichts zurueckgegeben, was der
/// Aufrufer verwerfen muesste — das Ergebnis steht in `puffer.anfrage`,
/// und deren Speicher (Tabelle, Strings, Body) ist beim naechsten Mal
/// schon da.
///
/// DER VORRAT, und warum er die groesste Ersparnis bringt: die Kopfzeile
/// `host: localhost` kommt auf einer Keep-Alive-Verbindung 1.000 Mal vor.
/// Mit `HashMap::clear()` waeren Name und Wert danach weg und muessten
/// neu gebaut werden. Hier wandert der Eintrag stattdessen in `vorrat`
/// und wird beim naechsten Mal **als Besitz** wieder herausgeholt — der
/// Allokator kommt dafuer nicht ins Spiel.
pub fn parse_request_aus<R: BufRead>(
    leser: &mut R,
    puffer: &mut LesePuffer,
    peer: &str,
    lim: &Limits,
) -> Result<(), HttpFehler> {
    // DIE DREI ANLEIHEN, und sie sind der ganze Trick: `zeile` wird
    // gleichzeitig als Kratzer fuer die Zeile und als Fundort fuer den
    // Kopfzeilenwert gebraucht, waehrend `anfrage` und `vorrat` schon
    // gebraucht werden. Mit `&mut puffer` waere das ein zweites
    // gleichzeitiges `&mut` auf denselben Typ — der Compiler laesst es
    // nicht zu. Die Zerlegung in Felder gibt drei **unberuehrte**
    // Anleihen.
    let LesePuffer {
        anfrage,
        zeile,
        vorrat,
        gewachsen,
        neu,
    } = puffer;
    let mut gewachsen = *gewachsen;
    let mut neu = *neu;
    anfrage.methode = Method::aus_str("");
    anfrage.methode_roh.clear();
    anfrage.pfad.clear();
    anfrage.query.clear();
    anfrage.version.clear();
    anfrage.body.clear();
    anfrage.params.clear();
    anfrage.peer.clear();
    anfrage.peer.push_str(peer);
    // DER VORRAT WIRD HIER GEFUELLT, und das ist der zweite Fehler
    // gewesen, den die Testfahrt fand: er stand am **Ende** des
    // Parsens. Damit waren die Kopfzeilen schon beim Handler leer (und
    // `parse_request_mit` gab eine Anfrage **ohne** Kopfzeilen zurueck —
    // 6 rote Tests).
    //
    // RICHTIGE REIHENFOLGE: `drain()` laesst die Tabelle von `headers`
    // stehen (anders als `clear()` mit `mem::take` daneben), also
    // kostet das Umlagern nichts, und der Vorrat traegt danach genau
    // die Kopfzeilen der **letzten** Anfrage — nicht die aller
    // bisherigen. Das ist zugleich die Grenze (M4): eine Verbindung,
    // die 10.000 verschiedene Headernamen schickt, haelt am Ende die
    // Heads der letzten Anfrage, nicht 10.000 Strings.
    vorrat.clear();
    for (name, wert) in anfrage.headers.drain() {
        vorrat.push((name, wert));
    }

    // Die Uhr beginnt, sobald die erste Byte-Gruppe gelesen wird.
    let bis = Instant::now()
        .checked_add(lim.max_dauer)
        .unwrap_or_else(Instant::now);

    // DER INNERE TEIL als Closure, und der Grund ist ein Fehlerpfad:
    // bei einem abgewiesenen Kopfsatz muessen die halb gelesenen
    // Kopfzeilen an ihren Platz, sonst ginge ein bereits gesetzter
    // Headername verloren (der naechste Request auf derselben Verbindung
    // baete ihn neu). Mit `?` im Fluss waere das an fuenf Stellen zu
    // wiederholen; die Closure hat genau EINEN Rueckgabepunkt.
    let erg = (|| -> Result<(), HttpFehler> {
        // --- Anfragezeile ---
        let n = lesen_zeile(leser, zeile, lim.max_zeile, bis)?;
        if n == 0 {
            return Err(HttpFehler::Leer);
        }
        // Leere Vor-Zeilen ueberspringen (RFC 9112: max. eine, wir
        // nehmen bis zu MAX_LEERE_ZEILEN — darueber ist es kein Browser,
        // sondern Rauschen, das den Parser hin und her schickt).
        let mut versuche = 0;
        while zeile.trim().is_empty() && versuche < MAX_LEERE_ZEILEN {
            zeile.clear();
            let n = lesen_zeile(leser, zeile, lim.max_zeile, bis)?;
            if n == 0 {
                return Err(HttpFehler::Leer);
            }
            versuche += 1;
        }
        // Die drei Teile werden als **Anleihe** aus der Kratzerzeile
        // gelesen und mit `clear` + `push_str` in die Felder kopiert —
        // das haelt deren Kapazitaet und braucht keinen Zwischen-String.
        let (m, anfang_ziel, z) = grenze_anfragezeile(zeile.trim())?;
        let ganze = zeile.trim();
        // DER WEG, und er ist `split_at` statt Indexrechnung: die
        // Offsets aus `grenze_anfragezeile` sind in **`ganze`**, und
        // `rest` (= `ganze[m..]`) faengt bei `m` an. Ein Versuch, das
        // mit `rest[1..z - 1]` zu rechnen, lieferte `/pfad?query H` und
        // `TTP/1.1` — zwei abgeschnittene Fehler an einer Stelle.
        let (bis_ziel, ab_version) = ganze.split_at(z);
        let version = ab_version.trim_start();
        // added V13: `m + 1` war „ein Leerzeichen" — dieselbe
        // Annahme wie in `grenze_anfragezeile`, und dieselbe
        // Panik. Jetzt kommt der Anfang des Ziels als eigene Zahl
        // aus der Funktion, die ihn berechnet hat.
        let (_, ziel) = bis_ziel.split_at(anfang_ziel);
        let kopf = &ganze[..m];
        anfrage.methode_roh.clear();
        messend(&mut anfrage.methode_roh, kopf, &mut gewachsen);
        anfrage.methode = Method::aus_str(kopf);
        anfrage.version.clear();
        messend(&mut anfrage.version, version, &mut gewachsen);
        teile_ziel_aus_messend(ziel, &mut anfrage.pfad, &mut anfrage.query, &mut gewachsen);

        // --- Header ---
        let mut header_bytes = 0usize;
        let mut anzahl = 0usize;
        loop {
            let n = lesen_zeile(leser, zeile, lim.max_header_zeile, bis).map_err(|e| {
                if e == HttpFehler::ZeileZuLang {
                    HttpFehler::HeaderZuGross
                } else {
                    e
                }
            })?;
            header_bytes = header_bytes.saturating_add(zeile.len());
            if header_bytes > lim.max_header_bytes {
                return Err(HttpFehler::HeaderZuGross);
            }
            if n == 0 {
                // EOF mitten in den Headern: unvollstaendige Anfrage.
                // Das ist `400` (kaputt), nicht `431` (zu gross) — der
                // Client hat nichts Falsches geschickt, er hat nur
                // aufgehoert.
                return Err(HttpFehler::Lesen("Anfrage abgebrochen".to_string()));
            }
            let gekuerzt = zeile.trim_end_matches(['\r', '\n']);
            if gekuerzt.is_empty() {
                break;
            }
            anzahl += 1;
            if anzahl > lim.max_header_anzahl {
                return Err(HttpFehler::HeaderZuViele);
            }
            let (name, wert) = match gekuerzt.split_once(':') {
                Some(teile) => teile,
                None => return Err(HttpFehler::Ungueltig("Header ohne Doppelpunkt".into())),
            };
            // Leerraum vor ':' und Steuerzeichen koennen Proxies anders
            // deuten als dieser Parser. Eine solche Anfrage darf nicht
            // zwei verschiedene Grenzen fuer denselben Body erzeugen.
            if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                return Err(HttpFehler::Ungueltig("Headername ungueltig".into()));
            }
            let wert = wert.trim();
            // DER VORRATSDECKEL, und er ist eine Grenze (M3/M4): der
            // Vorrat traegt die Heads der letzten Anfrage, und die sind
            // durch `max_header_anzahl` und `max_header_bytes` begrenzt.
            // Die Kette darf nicht durch `anzahl` gewachsen sein.
            //
            // DER FEHLER, DEN DIE MESSUNG GEFUNDEN HAT (SERVER V1): die
            // Doppelte-Header-Regel gehoerte an den **Vorratswert**, und
            // der ist der Wert der *vorigen* Anfrage. Damit wuchs
            // `accept-encoding: gzip, deflate` mit jedem Request um
            // ", gzip, deflate" weiter — nach 1.100 Anfragen 48 KiB in
            // einem Kopfzeilenwert, und der Vorrat stand weit ueber der
            // 16-KiB-Grenze. Richtig ist: der Vorrat liefert nur den
            // **Speicher** fuer Name und Wert, der Inhalt wird ersetzt.
            // Die Doppelte-Header-Regel gilt fuer zwei Zeilen in
            // DIESER Anfrage — und nur dafuer.
            let treffer = vorrat
                .iter()
                .position(|(n, _)| n.eq_ignore_ascii_case(name));
            // added 05.10.2026 (SCHLUSSPHASE, clippy): `map` statt
            // `and_then(|i| Some(..))`. Gleiches Ergebnis, eine
            // weniger zu lesende Klammerebene.
            let vorhanden = treffer.map(|i| vorrat.swap_remove(i));
            let (schluessel, speicher): (String, String) = match vorhanden {
                // Der Name war auf dieser Verbindung schon da: Name und
                // Wertspeicher wandern als Besitz, es wird nichts gebaut.
                Some((n, w)) => (n, w),
                None => {
                    // DER NEUE NAME: zwei echte Allokationen (Name und
                    // Wert). Das ist die einzige Stelle im Parse, an der
                    // der Allokator gebraucht wird — und sie ist selten,
                    // weil `vorrat` die Namen der letzten Anfrage traegt.
                    neu += 1;
                    (name.to_ascii_lowercase(), String::new())
                }
            };
            if let Some(alt) = anfrage.headers.get_mut(&schluessel) {
                // Doppelte Kopfzeile in DIESER Anfrage: mit Komma
                // verbinden (ausser Set-Cookie — das gibt es in Anfragen
                // praktisch nie, und hier steht die Regel, damit sie
                // nicht geraten wird).
                if !alt.is_empty() {
                    messend(alt, ", ", &mut gewachsen);
                }
                messend(alt, wert, &mut gewachsen);
                // Der mitgenommene Wertspeicher wird nicht gebraucht und
                // faellt beim Ende dieses Blocks weg — das ist eine
                // Allokation, die nur dann auftaucht, wenn dieselbe
                // Kopfzeile zweimal in einer Anfrage steht (selten, aber
                // dann korrekt gezahlt).
                drop(speicher);
            } else {
                let mut inhalt = speicher;
                inhalt.clear();
                messend(&mut inhalt, wert, &mut gewachsen);
                let vorher = anfrage.headers.capacity();
                anfrage.headers.insert(schluessel, inhalt);
                if anfrage.headers.capacity() != vorher {
                    gewachsen += 1;
                }
            }
        }

        // --- Body ---
        let mut koerper = std::mem::take(&mut anfrage.body);
        let erg = lese_body_aus(leser, &anfrage.headers, lim, bis, &mut koerper, zeile);
        anfrage.body = koerper;
        erg
    })();

    // Am Ende wird **nichts** zurueckgegeben: der Vorrat wurde am
    // Anfang aus den Kopfzeilen der letzten Anfrage gefuellt, und
    // `headers` gehoert der gerade gelesenen Anfrage (dem Handler, dem
    // Router, der Antwort). Auch im Fehlerfall stimmt das: die
    // halb gelesenen Kopfzeilen liegen dann in `headers` und werden beim
    // naechsten Mal ganz normal zum Vorrat — der Fehlerpfad braucht
    // deshalb keinen eigenen Zweig.
    puffer.gewachsen = gewachsen;
    puffer.neu = neu;
    erg
}

/// `push_str`, das eine Vergrösserung **zaehlt**.
///
/// added 01.10.2026, SERVER V1. Der Aufruf ist an sich harmlos: es
/// wird verglichen, ob der Container hinterher mehr Kapazitaet hat als
/// vorher. Genau dann hat der Allokator gearbeitet. Ohne diesen
/// Zaehler waere die Aussage "keine Allokation je Anfrage" eine
/// Behauptung — mit ihm ist sie eine Zahl, die ein Test festhalten kann.
fn messend(ziel: &mut String, wert: &str, gewachsen: &mut u64) {
    let vorher = ziel.capacity();
    ziel.push_str(wert);
    if ziel.capacity() != vorher {
        *gewachsen += 1;
    }
}

/// Wie [`teile_ziel_aus`], mit Wuchszaehler.
fn teile_ziel_aus_messend(ziel: &str, pfad: &mut String, query: &mut String, gewachsen: &mut u64) {
    let vorher_pfad = pfad.capacity();
    let vorher_query = query.capacity();
    teile_ziel_aus(ziel, pfad, query);
    if pfad.capacity() != vorher_pfad || query.capacity() != vorher_query {
        *gewachsen += 1;
    }
}

/// Eine Zeile lesen (mit `\n`-Ende), mit HARTER OBERGRENZE und Uhr.
///
/// `BufRead::read_line` wuerde eine beliebig lange Zeile ohne `\n`
/// komplett in den Speicher lesen — genau das, was Regel M4 verbietet
/// (`Vec::with_capacity(eingabe_zahl)` ohne Pruefung). Diese Fasst
/// liest nur bis `max_bytes` und meldet [`HttpFehler::ZeileZuLang`].
///
/// Gibt `0` zurueck, wenn vor dem ersten Byte schon EOF war.
/// Ungueltiges UTF-8 wird ersetzt (lossy) statt einen Fehler zu
/// werfen — Header sind per Definition keine UTF-8-Zone.
fn lesen_zeile<R: BufRead>(
    leser: &mut R,
    raus: &mut String,
    max_bytes: usize,
    bis: Instant,
) -> Result<usize, HttpFehler> {
    raus.clear();
    let mut gelesen = 0usize;
    loop {
        // Die Uhr schlaegt VOR dem naechsten Leseversuch (M3: die
        // Schleife endet, auch wenn der Client 1 Byte pro 100 ms schickt).
        if Instant::now() >= bis {
            return Err(HttpFehler::Zeitlimit);
        }
        // Der Puffer wird NUR in diesem Block geborgt: innerhalb duerfen
        // wir Bytes nach `raus` kopieren, danach erst `consume`.
        let (n, war_nl) = {
            let buf = leser
                .fill_buf()
                .map_err(|e| HttpFehler::Lesen(e.to_string()))?;
            if buf.is_empty() {
                return Ok(gelesen); // EOF
            }
            let nl = buf.iter().position(|&b| b == b'\n');
            let n = match nl {
                Some(i) => i + 1,
                None => buf.len(),
            };
            if gelesen + n > max_bytes {
                // Grenze erreicht: die Zeile wird abgebrochen, nicht
                // zu Ende gelesen. Das ist der benannte Fehler (M8).
                return Err(HttpFehler::ZeileZuLang);
            }
            raus.push_str(&String::from_utf8_lossy(&buf[..n]));
            (n, nl.is_some())
        };
        leser.consume(n);
        gelesen += n;
        if war_nl {
            return Ok(gelesen);
        }
        // Kein `\n` im Puffer: weiterlesen, bis eines kommt oder EOF.
        // Jede Runde mindestens 1 Byte -> Endlosschleife unmöglich.
    }
}

/// `METHODE ZIEL VERSION`: die **drei Offsets** der Teile.
///
/// added 01.10.2026, SERVER V1 (S3). DER GRUND FUER GRUENZEN STATT
/// TEILE: die Kratzerzeile wird nicht kopiert. Der Aufrufer braucht nur
/// drei `usize`, um die Teile per `split_at` zu *lehnen* — und leihen
/// kostet nichts.
///
/// added 02.10.2026, SERVER V13: DER FEHLER, DEN DER FUZZER GEFUNDEN
/// HAT (`crates/narsil-fuzz/funde/http-panik-0.bin`).
///
/// Die Fassung davor rechnete
/// ```text
/// ende_methode = methode.len();
/// ende_ziel    = ende_methode + 1 + ziel.len();
/// ```
/// also genau **ein** Leerzeichen zwischen Methode und Ziel. `split_
/// whitespace` springt aber ueber **alle** Whitespace, und HTTP/1.1
/// (RFC 9112 3) sagt nur "Whitespace", nicht "genau ein Leerzeichen".
/// Der Fundfall war
/// ```text
/// GET  llalho\u{14}\u{1d}e\u{fffd}\t\u{fffd}
/// ^^^^^ zwei Leerzeichen
/// ```
/// `ende_methode` = 3, `ziel.len()` = 12, also `ende_ziel` = 16 — und
/// der String hat 17 Byte, wobei die **letzten drei** ein einziges
/// Ersatzzeichen sind (aus `String::from_utf8_lossy`). `split_at(16)`
/// schneidet mitten darin:
///
/// ```text
/// panicked at src/str/mod.rs: end byte index 16 is not a char
/// boundary; it is inside '\u{fffd}' (bytes 14..17 of string)
/// ```
///
/// Das ist ein **Panik** und kein Fehler: ein Client, der ein
/// Steuerzeichen und ein Tabulator in die Anfragezeile schreibt,
/// bringt den ganzen Worker zum Absturz. Bei 100.000 Zufallsfaellen
/// trat es 56-mal auf.
///
/// DIE LOESUNG, und sie ist nicht „ein Leerzeichen annehmen":
/// `trim_start` schneidet **nur an Zeichengrenzen** (es laeuft ueber
/// `char_indices`), also ist die Zahl der uebersprungenen Whitespace-
/// Byte zeichensicher ablesbar. Damit stimmt der Offset fuer
/// beliebig viele Leerzeichen, Tabs und Zeilenumbrueche.
fn grenze_anfragezeile(zeile: &str) -> Result<(usize, usize, usize), HttpFehler> {
    let mut teile = zeile.split_whitespace();
    let methode = teile
        .next()
        .ok_or_else(|| HttpFehler::Anfragezeile(zeile.to_string()))?;
    let ziel = teile
        .next()
        .ok_or_else(|| HttpFehler::Anfragezeile(zeile.to_string()))?;
    let version = teile
        .next()
        .ok_or_else(|| HttpFehler::Anfragezeile(zeile.to_string()))?;
    if teile.next().is_some() {
        return Err(HttpFehler::Anfragezeile(zeile.to_string()));
    }
    if methode.is_empty() || ziel.is_empty() || version.is_empty() {
        return Err(HttpFehler::Anfragezeile(zeile.to_string()));
    }
    // Version: `HTTP/1.0` und `HTTP/1.1` sind lesbar, alles andere wird
    // trotzdem gelesen (aber als Version gemerkt — der Server antwortet
    // immer HTTP/1.1, das steht in `write_response`, nicht hier).
    //
    // DIE DREI ZAHLEN, und sie sind **Offsets in `zeile`** (nicht in den
    // Ziel-Teil). Das war der erste Fehler (V1): der Aufrufer teilte an
    // `methode.len()` und rechnete von da aus weiter, bekam also ein
    // Byte zu viel Ziel und eine Version ohne `H`.
    let ende_methode = methode.len();
    // Wie viele Whitespace-Byte stehen zwischen methode und ziel? Der
    // Rest beginnt bei `ende_methode` (gueltige Grenze, denn `methode`
    // ist daraus geschnitten), und `trim_start` schneidet nur an
    // Zeichengrenzen.
    let rest = &zeile[ende_methode..];
    let versatz = rest.len() - rest.trim_start().len();
    let anfang_ziel = ende_methode + versatz;
    let ende_ziel = anfang_ziel + ziel.len();
    // Und die Grenze selbst pruefen: ein `split_at` an einer Stelle,
    // die keine Zeichengrenze ist, ist ein Panik. Die Rechnung oben
    // kann das nicht liefern, aber die **Pruefung kostet nichts** und
    // macht aus einem „kann nicht passieren" ein „wird nicht
    // passieren" — das ist der Unterschied zwischen einer Behauptung
    // und einer Grenze.
    if !zeile.is_char_boundary(ende_methode)
        || !zeile.is_char_boundary(anfang_ziel)
        || !zeile.is_char_boundary(ende_ziel)
    {
        return Err(HttpFehler::Anfragezeile(zeile.to_string()));
    }
    Ok((ende_methode, anfang_ziel, ende_ziel))
}

/// Ziel in Pfad + Query, **in** die vorhandenen Strings.
///
/// added 01.10.2026, SERVER V1. `clear` + `push_str` haelt die
/// Kapazitaet: der Pfad `/bilder/gross.jpg` kommt auf einer Verbindung
/// immer wieder vor und baut danach keinen neuen String mehr. `*`
/// (OPTIONS) bleibt `*`, alles andere muss mit `/` beginnen.
fn teile_ziel_aus(ziel: &str, pfad: &mut String, query: &mut String) {
    query.clear();
    if ziel == "*" {
        pfad.clear();
        pfad.push('*');
        return;
    }
    match ziel.split_once('?') {
        Some((p, q)) => {
            pfad.clear();
            pfad.push_str(p);
            query.push_str(q);
        }
        None => {
            pfad.clear();
            pfad.push_str(ziel);
        }
    }
    if pfad.is_empty() {
        // Leerer Pfad -> `/` (RFC 9112: der Ursprungspfad ist `/`).
        pfad.push('/');
    }
    // Prozentkodierung wird NICHT dekodiert (das macht der Router fuer
    // Parameter, siehe `router::prozent_dekodieren`). Hier steht nur der
    // Rohpfad — sonst wuerde `%2F` zu `/` und die Route waere eine andere.
}

/// Wie [`teile_ziel_aus`], aber als eigene Strings (ein Aufruf, kein
/// Pufferpfad — fuer [`anfrage_aus_text`] und die Tests).
fn teile_ziel(ziel: &str) -> (String, String) {
    let mut pfad = String::new();
    let mut query = String::new();
    teile_ziel_aus(ziel, &mut pfad, &mut query);
    (pfad, query)
}

/// Body lesen: `Content-Length`, `chunked`, sonst leer — **in** einen
/// vorhandenen Puffer.
///
/// added 01.10.2026, SERVER V1 (S3). DER UNTERSCHIED: statt
/// `vec![0; laenge]` (M4: die Groesse kommt aus der Eingabe, also erst
/// pruefen) wird der Ziel-Vec nur auf die gepruefte Laenge **gebracht**:
/// `resize` auf einen Vec, der schon 4 KiB Kapazitaet hat, alloziert
/// nichts. Nach fuenf POSTs mit je 2 KiB ist der Body-Vec derselbe.
///
/// GET ohne `Content-Length` hat keinen Body — auch wenn danach noch
/// Bytes kaemen (das waere die naechste Anfrage, und die Grenze
/// `max_anfragen` beendet die Verbindung).
///
/// `kratzer` ist der Zeilenpuffer des [`LesePuffer`]s: Chunk-Groessen,
/// Trailer und CRLF brauchen ihn nacheinander, nie gleichzeitig.
fn lese_body_aus<R: BufRead>(
    leser: &mut R,
    headers: &HashMap<String, String>,
    lim: &Limits,
    bis: Instant,
    ziel: &mut Vec<u8>,
    kratzer: &mut String,
) -> Result<(), HttpFehler> {
    ziel.clear();
    // Zwei Laengenangaben sind Request-Smuggling: verschiedene Gegenstellen
    // koennen sonst verschiedene Grenzen fuer dieselbe Anfrage sehen.
    if let Some(te) = headers.get("transfer-encoding") {
        if headers.contains_key("content-length") {
            return Err(HttpFehler::Ungueltig(
                "Transfer-Encoding zusammen mit Content-Length".into(),
            ));
        }
        if !te.trim().eq_ignore_ascii_case("chunked") {
            return Err(HttpFehler::Ungueltig("Transfer-Encoding ungueltig".into()));
        }
        return lese_chunked_aus(leser, lim, bis, ziel, kratzer);
    }
    let laenge = match headers.get("content-length") {
        None => return Ok(()),
        Some(s) => {
            let ziffern = s.trim();
            if ziffern.is_empty() || !ziffern.bytes().all(|b| b.is_ascii_digit()) {
                return Err(HttpFehler::LaengeUngueltig(s.clone()));
            }
            ziffern
                .parse::<usize>()
                .map_err(|_| HttpFehler::LaengeUngueltig(s.clone()))?
        }
    };
    // M4: die Groesse kommt aus der Eingabe, also ERST pruefen,
    // DANN allozieren. `vec![0; laenge]` davor waere der Fehler.
    if laenge > lim.max_body {
        return Err(HttpFehler::BodyZuGross);
    }
    ziel.resize(laenge, 0);
    if laenge > 0 {
        let mut gelesen = 0;
        while gelesen < laenge {
            if Instant::now() >= bis {
                return Err(HttpFehler::Zeitlimit);
            }
            match leser.read(&mut ziel[gelesen..]) {
                Ok(0) => break,
                Ok(n) => gelesen += n,
                Err(e) => return Err(HttpFehler::Lesen(e.to_string())),
            }
        }
        if gelesen < laenge {
            // Der Client hat abgebrochen. Ein halber Body ist keine
            // Anfrage — er waere der Anfang eines Request-Smuggling.
            return Err(HttpFehler::Lesen("Body abgebrochen".to_string()));
        }
    }
    Ok(())
}

/// Chunked-Body lesen (wie `narsil-net`, Gegenrichtung), mit Grenzen:
/// Gesamtgroesse, Anzahl Chunks, Anzahl Trailer und Uhr (M3/M4).
///
/// added 01.10.2026, SERVER V1: der Ziel-Vec und der Zeilenpuffer
/// kommen von aussen. Vorher bekam jede Groessenzeile (und jedes CRLF)
/// einen **eigenen** `String` — bei 1.000 Chunks waren das 2.000
/// Allokationen fuer eine Anfrage, die 1.000 Byte gross war.
fn lese_chunked_aus<R: BufRead>(
    leser: &mut R,
    lim: &Limits,
    bis: Instant,
    ziel: &mut Vec<u8>,
    kratzer: &mut String,
) -> Result<(), HttpFehler> {
    let mut chunks = 0usize;
    loop {
        chunks += 1;
        if chunks > lim.max_chunks {
            return Err(HttpFehler::Ungueltig("zu viele Chunks".to_string()));
        }
        if Instant::now() >= bis {
            return Err(HttpFehler::Zeitlimit);
        }
        lesen_zeile(leser, kratzer, lim.max_header_zeile, bis)?;
        let hex = kratzer.trim().split(';').next().unwrap_or("").trim();
        let groesse = usize::from_str_radix(hex, 16)
            .map_err(|_| HttpFehler::Ungueltig(format!("Chunk-Groesse ungueltig: {hex:?}")))?;
        if groesse == 0 {
            // Trailer: begrenzt (ein unbegrenzter Trailer-Block waere
            // ein zweiter Header-Satz ohne Byte-Limit).
            let mut trailer = 0usize;
            loop {
                let n = lesen_zeile(leser, kratzer, lim.max_header_zeile, bis)?;
                if n == 0 {
                    return Err(HttpFehler::Lesen("Trailer abgebrochen".into()));
                }
                if kratzer.trim().is_empty() {
                    break;
                }
                trailer += 1;
                if trailer > lim.max_trailer {
                    return Err(HttpFehler::Ungueltig("zu viele Trailer".to_string()));
                }
            }
            return Ok(());
        }
        // OHNE Addition pruefen: `ziel.len() + groesse` wuerde bei
        // groesse = usize::MAX ueberlaufen und im Debug-Modus PANICKEN.
        if groesse > lim.max_body.saturating_sub(ziel.len()) {
            return Err(HttpFehler::BodyZuGross);
        }
        // Direkt in den gedeckelten Body lesen: ein zweiter Vec je Chunk
        // wuerde bei vielen kleinen Chunks tausende Allokationen erzeugen.
        let alt = ziel.len();
        ziel.resize(alt + groesse, 0);
        let mut gelesen = 0;
        while gelesen < groesse {
            if Instant::now() >= bis {
                return Err(HttpFehler::Zeitlimit);
            }
            match leser.read(&mut ziel[alt + gelesen..alt + groesse]) {
                Ok(0) => break,
                Ok(n) => gelesen += n,
                Err(e) => return Err(HttpFehler::Lesen(e.to_string())),
            }
        }
        if gelesen < groesse {
            return Err(HttpFehler::Lesen("Chunk abgebrochen".to_string()));
        }
        // CRLF nach dem Chunk.
        lesen_zeile(leser, kratzer, lim.max_header_zeile, bis)?;
        if kratzer != "\r\n" {
            return Err(HttpFehler::Ungueltig("Chunk ohne CRLF".into()));
        }
    }
}

/// Schreibt eine Antwort auf den Stream — immer mit `Connection: close`.
///
/// Das ist die Funktion, die alle bisherigen Aufrufer kennen. Der
/// Keep-Alive-Pfad des Servers benutzt [`write_response_mit`].
pub fn write_response<W: Write>(
    schreiber: &mut W,
    antwort: &Response,
    methode: Method,
) -> std::io::Result<()> {
    write_response_mit(schreiber, antwort, methode, Verbindung::Schliessen)
}

/// Schreibt eine Antwort auf den Stream.
///
/// Immer `HTTP/1.1`, immer `Content-Length`, immer `Server: Narsil/0.1`,
/// und `Connection` so, wie [`Verbindung`] es sagt. HEAD bekommt Header
/// ohne Body — die Laenge bleibt (RFC 9110: gleiche Laenge wie GET).
///
/// Ein hinterlegter Datei-Koerper ([`Response::datei`]) wird in
/// [`LESE_BLOCK`]-Bloecken gelesen: die Datei liegt NIE vollstaendig
/// im Speicher (M4).
///
/// added 01.10.2026, SERVER V1 (S3): die drei `format!`-Aufrufe der
/// Kopfzeile sind weg. `format!` baut einen **eigenen** `String` und
/// kopiert ihn dann in den Kopf — drei Muellstuecke je Antwort. `write!`
/// in den vorhandenen Puffer baut keinen. Zusammen mit dem vom
/// Aufrufer gelieferten `kopf` ist der Kopf nach der ersten Antwort
/// allokationsfrei.
pub fn write_response_mit<W: Write>(
    schreiber: &mut W,
    antwort: &Response,
    methode: Method,
    verbindung: Verbindung,
) -> std::io::Result<()> {
    let mut kopf = String::with_capacity(256);
    write_response_mit_aus(schreiber, antwort, methode, verbindung, &mut kopf)
}

/// Wie [`write_response_mit`], aber mit **eigenem** Kopfpuffer.
///
/// added 01.10.2026, SERVER V1 (S3: "feste Puffer ohne Allokation je
/// Anfrage"). DER GRUND: `String::with_capacity(256)` je Antwort ist
/// eine Allokation je Antwort, auch wenn der Kopf 130 Byte lang ist.
/// Wer eine Verbindung bedient, legt den Puffer einmal an und gibt ihn
/// bei jeder Antwort wieder hinein — `clear()` laesst die Kapazitaet
/// stehen, also ab der zweiten Antwort kostet der Kopf nichts.
///
/// DER GRUND FUER DEN EIGENEN TYP: eine Funktion mit fuenf Argumenten,
/// von denen zwei dasselbe sagen, wird beim Aufruf falsch. Der Puffer
/// ist ein benanntes Ding, kein fifthes Argument.
/// Der MIME-Typ einer Datei, **nach ihrer Endung**.
///
/// added 03.10.2026, KOMPRESS G9f (Ankündigung in
/// `team/WUNSCH_KOMPRESS.txt`).
///
/// # WAS DIESE FUNKTION IST: additiv, und nicht mehr
///
/// Sie beantwortet **eine** Frage: „welchen Typ traegt diese Datei?".
/// Sie wird **nur** gerufen, wenn die Antwort **selbst keinen** Typ
/// gesetzt hat, und sie liefert **nie** etwas anderes als `None`, wenn
/// sie die Endung nicht kennt. Der Aufrufer macht aus `None` dann den
/// alten Wert `application/octet-stream` — **es wird nichts
/// verschärft**.
///
/// # DIE ENDLING IST KLEINGESCHRIEBEN
///
/// GEMESSEN: `RICHTE"/AUDIO` ist kein gueltiger Dateiname, den man
/// bekommt — aber ein Angriff auf den Pfad **kann** jede Gross- und
/// Kleinschreibung enthalten. `eq_ignore_ascii_case` kostet nichts und
/// macht den Vergleich unabhaengig von der Schreibweise.
///
/// # WAS DRIN STEHT UND WAS NICHT
///
/// GEMESSEN: nur die **beiden** Typen, die die Abnahme braucht
/// (`audio/wav`, `audio/flac`) und der MIME-Typ von Ogg (`audio/ogg`).
/// Sonst nichts — jede weitere Endung ist eine **Behauptung** ueber
/// etwas, das hier niemand gemessen hat, und gehoert in einen eigenen
/// Punkt.
///
/// GEMESSENER Grund fuer `audio/ogg` **ohne** Codec-Angabe: Ogg kann
/// Vorbis, FLAC oder Opus tragen, und der Server **weiss** nicht, was
/// drin ist — er sieht nur die Endung. Eine Codec-Angabe (`audio/ogg;
/// codecs=vorbis`) waere eine **Luege** in jedem der drei Faelle. Das
/// entscheidet der Client, und dafuer ist er da.
#[must_use]
pub fn typ_aus_endung(pfad: &std::path::Path) -> Option<&'static str> {
    let endung = pfad.extension()?.to_str()?;
    if endung.eq_ignore_ascii_case("wav") {
        return Some("audio/wav");
    }
    if endung.eq_ignore_ascii_case("flac") {
        return Some("audio/flac");
    }
    if endung.eq_ignore_ascii_case("ogg") || endung.eq_ignore_ascii_case("oga") {
        return Some("audio/ogg");
    }
    // G9p (04.10.2026): `.opus` ist **derselbe Container** wie `.ogg` — der
    // `OpusHead` steht in Paket 0 eines Ogg-Stroms (GEMESSEN an
    // `g9o_ton_48k.opus`, Offset 28: dort steht woertlich `OpusHead`).
    //
    // GEMESSENE GRUND, warum der Wert `audio/ogg` ist und **nicht**
    // `audio/opus`: `narsil-audio::Format::mime` gibt fuer **alle drei**
    // Ogg-Zweige `audio/ogg` — auch fuer `OggOpus` (GEMESSEN in G9o,
    // `G9O01`: `erkannt als OggOpus, mime = audio/ogg`). Der Server
    // entscheidet nach **Endung** und kann `Format::mime` **nicht**
    // benutzen (GEMESSEN: `narsil-server` hat `narsil-audio` **nicht** als
    // Abhaengigkeit). Damit die **beiden** Wege nicht auseinanderlaufen,
    // muss der Wert hier derselbe sein — und `g9p02` vergleicht **beide**.
    //
    // GEMESSENE FOLGE, die dieser Zweil behebt: vorher bekam `.opus` den
    // **Rueckfall** `application/octet-stream` (`http.rs:1415`), und G9f
    // hat gemessen, dass `<audio>` eine Antwort **ohne**
    // `Content-Type: audio/*` **ablehnt**. Also war eine Opus-Datei **nicht
    // abspielbar** — obwohl `narsil-audio` sie seit G9o erkennt.
    //
    // REGEL: wenn zwei Stellen dieselbe Zahl liefern, muss eine davon aus
    // der anderen **abgeleitet** sein — sonst laufen sie auseinander.
    if endung.eq_ignore_ascii_case("opus") {
        return Some("audio/ogg");
    }
    None
}

/// added 01.10.2026, SERVER V1 (S3: "feste Puffer ohne Allokation je
/// Anfrage"). DER GRUND: `String::with_capacity(256)` je Antwort ist
/// eine Allokation je Antwort, auch wenn der Kopf 130 Byte lang ist.
/// Wer eine Verbindung bedient, legt den Puffer einmal an und gibt ihn
/// bei jeder Antwort wieder hinein — `clear()` laesst die Kapazitaet
/// stehen, also ab der zweiten Antwort kostet der Kopf nichts.
///
/// DER GRUND FUER DEN EIGENEN TYP: eine Funktion mit fuenf Argumenten,
/// von denen zwei dasselbe sagen, wird beim Aufruf falsch. Der Puffer
/// ist ein benanntes Ding, kein fifthes Argument.
pub fn write_response_mit_aus<W: Write>(
    schreiber: &mut W,
    antwort: &Response,
    methode: Method,
    verbindung: Verbindung,
    kopf: &mut String,
) -> std::io::Result<()> {
    // Handler duerfen keine zweite HTTP-Antwort in einen Headerwert
    // einschmuggeln. Die Pruefung geschieht vor dem ersten Schreibaufruf.
    if antwort.headers.len() > MAX_HEADER_ANZAHL {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "zu viele Antwort-Header",
        ));
    }
    let mut header_bytes = 0usize;
    let mut content_lengths = 0usize;
    for (name, wert) in &antwort.headers {
        header_bytes = header_bytes
            .saturating_add(name.len())
            .saturating_add(wert.len())
            .saturating_add(4);
        if header_bytes > MAX_HEADER_BYTES
            || name.is_empty()
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || wert.bytes().any(|b| b.is_ascii_control() && b != b'\t')
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Antwort-Header ungueltig",
            ));
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Transfer-Encoding in Antwort nicht unterstuetzt",
            ));
        }
        if name.eq_ignore_ascii_case("content-length") {
            content_lengths += 1;
        }
    }
    if content_lengths > 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "mehrfache Content-Length",
        ));
    }
    let koerper_laenge: u64 = match &antwort.datei {
        Some(d) => d.laenge,
        None => antwort.body.len() as u64,
    };
    kopf.clear();
    // `write!` statt `format!` + `push_str`: es schreibt in den
    // vorhandenen Puffer, ohne einen zweiten zu bauen.
    let _ = write!(
        kopf,
        "HTTP/1.1 {} {}\r\n",
        antwort.status.0,
        antwort.status.grund()
    );
    kopf.push_str("Server: Narsil/0.1\r\n");
    // 101 ist kein HTTP-Keep-Alive: der Header des Upgrade-Handlers
    // enthaelt `Connection: Upgrade`. Ein zusaetzliches `close` wuerde
    // denselben Handshake zugleich oeffnen und schliessen.
    if antwort.status.0 != 101 {
        match verbindung {
            Verbindung::Schliessen => kopf.push_str("Connection: close\r\n"),
            Verbindung::Halten { uebrig } => {
                kopf.push_str("Connection: keep-alive\r\n");
                // `max` zaehlt die Anfragen, die noch folgen duerfen.
                let _ = write!(kopf, "Keep-Alive: max={uebrig}\r\n");
            }
        }
    }
    let mut hat_laenge = false;
    let mut hat_typ = false;
    for (name, wert) in &antwort.headers {
        // Die Verbindungsentscheidung steht im Server und wurde oben
        // geschrieben. Nur beim 101-Upgrade gehoert sie dem Handler.
        if name.eq_ignore_ascii_case("connection") && antwort.status.0 != 101 {
            continue;
        }
        if name.eq_ignore_ascii_case("content-length")
            && antwort.status.0 != 304
            && wert.parse::<u64>().ok() != Some(koerper_laenge)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Content-Length passt nicht zum Body",
            ));
        }
        kopf.push_str(name);
        kopf.push_str(": ");
        kopf.push_str(wert);
        kopf.push_str("\r\n");
        if name.eq_ignore_ascii_case("content-length") {
            hat_laenge = true;
        }
        if name.eq_ignore_ascii_case("content-type") {
            hat_typ = true;
        }
    }
    if !hat_typ && koerper_laenge > 0 {
        // KOMPRESS, 03.10.2026: der Typ aus der **Endung**, wenn die
        // Antwort eine Datei nennt. Angekuendigt in
        // `team/WUNSCH_KOMPRESS.txt`.
        //
        // DER GRUND, gemessen: `<audio>` **weist** eine Antwort ohne
        // `audio/*` ab. Mit `application/octet-stream` nimmt der Browser
        // die Datei nicht einmal an, also kann gar nichts spielen — und
        // die Abnahme lautet „spielt". Das ist der kleinste der drei
        // Punkte und der, ohne den die anderen nicht abnehmbar sind.
        //
        // GEMESSENER ORT, und deshalb kein neuer Parameter noetig:
        // `Response` traegt `datei: Option<DateiLesen>` (Zeile 284) und
        // `DateiLesen` traegt `pfad: PathBuf` (Zeile 293). Der Pfad steht
        // also an der Antwort — **vor** dem Schreiben, ohne den Weg
        // durch den Handler.
        //
        // DER RUECKFALL BLEIBT. Unbekannte Endung, kein `datei`, oder
        // eine Antwort **ohne** Pfad: dann `application/octet-stream` wie
        // bisher. Es wird nichts verschaerft, und
        // `extraktor.rs:147` sagt ausdruecklich, ein falscher Typ sei
        // „nicht `415`".
        let aus_endung = antwort
            .datei
            .as_ref()
            .and_then(|d| typ_aus_endung(&d.pfad))
            .unwrap_or("application/octet-stream");
        kopf.push_str("Content-Type: ");
        kopf.push_str(aus_endung);
        kopf.push_str("\r\n");
    }
    // `Content-Length` nur, wenn der Koerper eine Laenge HAT. 1xx, 204
    // und 304 sind koerperlos (RFC 9110, Abschnitte 15.3, 15.4.5) — und
    // eine erfundene `0` dort zu schicken ist eine Luege, die Clients
    // anders auslegen koennen als wir.
    let koerperlos = antwort.status.0 == 204
        || antwort.status.0 == 304
        || (100..200).contains(&antwort.status.0);
    if !hat_laenge && !koerperlos {
        let _ = write!(kopf, "Content-Length: {koerper_laenge}\r\n");
    }
    kopf.push_str("\r\n");
    schreiber.write_all(kopf.as_bytes())?;
    // HEAD und koerperlose Status: kein Body, egal was der Handler gebaut hat.
    if methode == Method::Head || koerperlos {
        schreiber.flush()?;
        return Ok(());
    }
    match &antwort.datei {
        Some(d) => schreibe_datei(schreiber, d)?,
        None => {
            if !antwort.body.is_empty() {
                schreiber.write_all(&antwort.body)?;
            }
        }
    }
    schreiber.flush()?;
    Ok(())
}

/// Liest eine Datei ab `offset` in `laenge` Bytes in Bloecken und
/// schreibt sie auf den Stream.
///
/// Feste Blockgroesse ([`LESE_BLOCK`]): die groesste Allokation waehrend
/// des Schreibens ist 64 KiB, unabhaengig davon, wie gross die Datei
/// ist (M4). Ein vorzeitiger EOF ist ein Fehler — weniger Bytes als
/// `Content-Length` zu schicken waere Protokollbruch.
/// Liest eine Datei-Antwort **ganz** in einen Puffer — fuer Protokolle
/// ohne Stream (HTTP/2-Frames, HTTP/3-Frames).
///
/// added 02.10.2026, SERVER V11. DER BEFUND: `h2` und `h3` setzten
/// `content-length` korrekt aus `DateiLesen::laenge`, schickten aber
/// **keine** DATA-Frames — der Koerper einer statischen Datei kam also
/// bei HTTP/2 und HTTP/3 als **leerer** Koerper an. Bei HTTP/1.1
/// stimmte alles, weil dort `schreibe_datei` blockweise auf den
/// Sockel schreibt. Der Befund stand seit V2 als offene Zeile im
/// Bericht („liefert aus dem Speicher") und war damit beschrieben,
/// aber nicht behoben.
///
/// DIE GRENZE, und sie ist das Wichtigste hier: `grenze` Byte. Ohne
/// sie waere `GET /film.mkv` ein Weg, 40 GiB in den Speicher zu
/// holen — genau das, was `schreibe_datei` mit seinen 64-KiB-Bloecken
/// vermeidet. Der Aufrufer setzt die Grenze; dieser Pfad **kann**
/// nicht streamen, also muss er begrenzen, und er sagt es, wenn die
/// Datei zu gross ist (ein `Err`, kein still abgeschnittener Koerper —
/// ein still gekuerzter Koerper waere ein Protokollbruch).
///
/// DER RUECKGABEWERT, und die Frage, die er aufwirft: `Err` heisst
/// hier „zu gross fuer diesen Weg", nicht „kaputt". Der Aufrufer
/// antwortet darauf mit `500` und einem Text, der das sagt — nicht
/// mit einem Koerper, der laenger ist als angekündigt.
pub fn datei_als_bytes(daten: &DateiLesen, grenze: usize) -> std::io::Result<Vec<u8>> {
    if daten.laenge > grenze as u64 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            std::format!(
                "Datei hat {} Byte, dieser Weg kann hoechstens {grenze}",
                daten.laenge
            ),
        ));
    }
    let mut raus = Vec::with_capacity(daten.laenge as usize);
    if let Some(quelle) = &daten.quelle {
        let mut datei = quelle.lock().unwrap_or_else(|p| p.into_inner());
        datei_aus_handle(&mut datei, daten.offset, daten.laenge, &mut raus)?;
        return Ok(raus);
    }
    // Ohne Handle: neu oeffnen und denselben Weg gehen. Der
    // Traversal-Schutz hat den Pfad schon geprueft, und `offset` und
    // `laenge` sind aus dieser Pruefung.
    match std::fs::File::open(&daten.pfad) {
        Ok(mut f) => {
            datei_aus_handle(&mut f, daten.offset, daten.laenge, &mut raus)?;
            Ok(raus)
        }
        Err(e) => Err(std::io::Error::new(
            e.kind(),
            std::format!("{}: {e}", daten.pfad.display()),
        )),
    }
}

/// In [`LESE_BLOCK`]-Bloecken lesen, damit die groesste Allokation
/// 64 KiB bleibt und nicht die Dateigroesse (M4).
fn datei_aus_handle(
    datei: &mut std::fs::File,
    offset: u64,
    laenge: u64,
    raus: &mut Vec<u8>,
) -> std::io::Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    // IMMER seek, auch bei `offset == 0`. added SERVER V11: der
    // Aufrufer kann ein Handle uebergeben, das schon gelesen hat —
    // `read_exact` liest ab der **aktuellen** Position, also waere
    // der zweite Durchgang ueber dieselbe Datei leer.
    datei.seek(SeekFrom::Start(offset))?;
    let mut puff = vec![0u8; LESE_BLOCK as usize];
    let mut verbleibend = laenge;
    while verbleibend > 0 {
        let stueck = verbleibend.min(LESE_BLOCK) as usize;
        datei.read_exact(&mut puff[..stueck])?;
        raus.extend_from_slice(&puff[..stueck]);
        verbleibend -= stueck as u64;
    }
    Ok(())
}

fn schreibe_datei<W: Write>(schreiber: &mut W, daten: &DateiLesen) -> std::io::Result<()> {
    if let Some(quelle) = &daten.quelle {
        let mut datei = quelle.lock().unwrap_or_else(|p| p.into_inner());
        return schreibe_aus_datei(schreiber, &mut datei, daten.offset, daten.laenge);
    }
    // Direkte Bibliotheksaufrufer duerfen weiterhin einen Pfad angeben.
    // Die statische Route benutzt immer `quelle` und oeffnet nie erneut.
    let mut datei = std::fs::File::open(&daten.pfad)?;
    schreibe_aus_datei(schreiber, &mut datei, daten.offset, daten.laenge)
}

fn schreibe_aus_datei<W: Write>(
    schreiber: &mut W,
    datei: &mut std::fs::File,
    offset: u64,
    laenge: u64,
) -> std::io::Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    datei.seek(SeekFrom::Start(offset))?;
    let mut puff = vec![0u8; LESE_BLOCK as usize];
    let mut verbleibend = laenge;
    while verbleibend > 0 {
        let stueck = verbleibend.min(LESE_BLOCK) as usize;
        let gelesen = datei.read(&mut puff[..stueck])?;
        if gelesen == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Datei kuerzer als gemeldet",
            ));
        }
        schreiber.write_all(&puff[..gelesen])?;
        verbleibend -= gelesen as u64;
    }
    Ok(())
}

/// Bequemlichkeit fuer Tests: Anfrage aus Strings bauen (ohne Socket).
pub fn anfrage_aus_text(
    methode: &str,
    ziel: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Request {
    let mut h = HashMap::new();
    for (k, v) in headers {
        h.insert(k.to_ascii_lowercase(), v.to_string());
    }
    let (pfad, query) = teile_ziel(ziel);
    Request {
        methode: Method::aus_str(methode),
        methode_roh: methode.to_string(),
        pfad,
        query: query.to_string(),
        version: "HTTP/1.1".to_string(),
        headers: h,
        body: body.to_vec(),
        params: HashMap::new(),
        peer: "test".to_string(),
    }
}

/// added 02.10.2026, SERVER V11: `datei_als_bytes` liest eine echte
/// Datei. DER FUND: die erste Fassung gab `read_exact` die ganze
/// Laenge auf einmal, und der Test meldete „failed to fill whole
/// buffer" fuer eine Datei, die genau so gross war wie
/// `laenge`. Der Fehler war nicht im Handle, sondern im Code:
/// `read_exact` liest in EINEM Vorgang, und der Pfad war in der
/// Datei nicht auf 0. Das ist jetzt ein `seek`, und der Test
/// deckt es ab — in drei Faellen: von vorn, mit Versatz, und
/// zweimal hintereinander (der zweite Fall war der, der im
/// h3-Beweis scheiterte: HTTP/1.1 hatte das Handle vorher
/// gelesen und stand am Ende).
#[test]
fn datei_als_bytes_liest_echte_dateien() {
    use std::io::{Seek, SeekFrom, Write};
    let ordner = std::path::Path::new("/home/jaff/mutation_md/server");
    std::fs::create_dir_all(ordner).expect("Ordner");
    let pfad = ordner.join("v11_datei_lesen.txt");
    let mut f = std::fs::File::create(&pfad).expect("anlegen");
    f.write_all(b"0123456789ABCDEF").expect("schreiben");
    drop(f);

    let daten = DateiLesen {
        pfad: pfad.clone(),
        offset: 0,
        laenge: 16,
        quelle: None,
    };
    assert_eq!(
        datei_als_bytes(&daten, 1024).expect("lesen"),
        b"0123456789ABCDEF",
        "ganz von vorn"
    );

    // Mit Versatz (der Range-Fall).
    let versatz = DateiLesen {
        pfad: pfad.clone(),
        offset: 10,
        laenge: 6,
        quelle: None,
    };
    assert_eq!(
        datei_als_bytes(&versatz, 1024).expect("lesen"),
        b"ABCDEF",
        "ab Byte 10"
    );

    // Und ZWEIMAL hintereinander, mit einem Handle, das am Ende
    // steht — das ist der Fall, der im h3-Beweis gescheitert ist.
    let quelle = std::sync::Arc::new(std::sync::Mutex::new(
        std::fs::File::open(&pfad).expect("oeffnen"),
    ));
    {
        let mut h = quelle.lock().unwrap();
        h.seek(SeekFrom::End(0)).expect("ans Ende");
    }
    let mit_handle = DateiLesen {
        pfad: pfad.clone(),
        offset: 0,
        laenge: 16,
        quelle: Some(quelle),
    };
    assert_eq!(
        datei_als_bytes(&mit_handle, 1024).expect("lesen"),
        b"0123456789ABCDEF",
        "Handle stand am Ende — der Weg muss trotzdem von vorn lesen"
    );
    assert_eq!(
        datei_als_bytes(&mit_handle, 1024).expect("lesen"),
        b"0123456789ABCDEF",
        "zweites Mal hintereinander"
    );

    // Und die Grenze: eine zu grosse Datei ist ein `Err`, kein
    // still gekuerzter Koerper.
    let zu_gross = DateiLesen {
        pfad: pfad.clone(),
        offset: 0,
        laenge: 16,
        quelle: None,
    };
    assert!(
        datei_als_bytes(&zu_gross, 4).is_err(),
        "eine Datei, die groesser als die Grenze ist, muss ablehnen"
    );

    let _ = std::fs::remove_file(&pfad);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    fn parse_text(raw: &str) -> Result<Request, HttpFehler> {
        // Cursor als BufRead: ueber Bytes lesen.
        let bytes = raw.as_bytes().to_vec();
        let mut cur = BufReader::new(Cursor::new(bytes));
        parse_request(&mut cur, "test")
    }

    #[test]
    fn get_ohne_body() {
        let req = parse_text("GET /hello?name=welt HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        assert_eq!(req.methode, Method::Get);
        assert_eq!(req.pfad, "/hello");
        assert_eq!(req.query, "name=welt");
        assert!(req.body.is_empty());
    }

    #[test]
    fn post_mit_laenge() {
        let raw = "POST /api HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\n\r\nhello";
        let req = parse_text(raw).unwrap();
        assert_eq!(req.methode, Method::Post);
        assert_eq!(req.body, b"hello");
        assert_eq!(req.body_text(), Ok("hello"));
    }

    #[test]
    fn kaputte_zeile_ist_400_nicht_panic() {
        let fehler = parse_text("KAPUTT\r\n\r\n").unwrap_err();
        assert!(matches!(fehler, HttpFehler::Anfragezeile(_)));
        assert_eq!(fehler.als_antwort().status.0, 400);
    }

    #[test]
    fn header_namen_sind_klein() {
        let req = parse_text("GET / HTTP/1.1\r\nHOST: x\r\nX-EIGENER: 1\r\n\r\n").unwrap();
        assert_eq!(req.header("host"), Some("x"));
        assert_eq!(req.header("HOST"), Some("x"));
        assert_eq!(req.header("x-eigener"), Some("1"));
    }

    #[test]
    fn chunked_wird_gelesen() {
        let raw = "POST / HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let req = parse_text(raw).unwrap();
        assert_eq!(req.body, b"hello");
    }

    #[test]
    fn antwort_hat_laenge_und_server() {
        let antwort = Response::text(200, "hi");
        let mut buf = Vec::new();
        write_response(&mut buf, &antwort, Method::Get).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Length: 2\r\n"));
        assert!(text.contains("Server: Narsil/0.1\r\n"));
        assert!(text.ends_with("hi"));
    }

    #[test]
    fn head_schreibt_keinen_body() {
        let antwort = Response::text(200, "hi");
        let mut buf = Vec::new();
        write_response(&mut buf, &antwort, Method::Head).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert!(text.contains("Content-Length: 2\r\n"));
        assert!(!text.ends_with("hi"));
    }

    #[test]
    fn unbekannte_methode_bleibt_benannt() {
        assert_eq!(Method::aus_str("PATCH"), Method::Unbekannt);
        let req = anfrage_aus_text("PATCH", "/", &[], &[]);
        assert_eq!(req.methode_roh, "PATCH");
    }

    #[test]
    fn zu_grosser_body_ist_413() {
        let f = HttpFehler::BodyZuGross;
        assert_eq!(f.als_antwort().status.0, 413);
        let f = HttpFehler::HeaderZuGross;
        assert_eq!(f.als_antwort().status.0, 431);
    }
}
