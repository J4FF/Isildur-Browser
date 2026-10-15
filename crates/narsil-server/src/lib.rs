//! narsil-server — Webserver- & API-Bibliothek in reinem Rust.
//!
//! # WOFUER DIESE CRATE EXISTIERT
//!
//! Punkt 2 der Vision: ein extrem performanter, modularer Webserver,
//! geschrieben in Rust, optimiert fuer minimalen Speicherverbrauch und
//! maximale Durchsatzraten — plus integrierte API-Services (REST,
//! minimal GraphQL, WebSocket), nahtlos integriert.
//!
//! Bis hierher konnte der Browser nur LADEN (`narsil-net` als Client).
//! Es gab nichts, das ANTWORTET. Diese Crate ist die Gegenseite:
//! sie nimmt TCP-Verbindungen an, parst HTTP/1.1, routet typsicher,
//! liefert statische Dateien (`.narsil`, `.html`, Klartext) und
//! JSON-APIs — alles mit `std` allein, ohne ein einziges Fremdcrate
//! (ADR-030 bleibt unangetastet).
//!
//! # DIE MODULE, in Aufrufreihenfolge
//!
//! ```text
//! TcpStream -> http::parse_request -> router::Router::route
//!   -> Handler(&Request) -> Response -> http::write_response -> TcpStream
//! ```
//!
//! | Modul | Inhalt |
//! |---|---|
//! | [`http`] | Methode, Status, Anfrage, Antwort, Parsing, Serialisierung |
//! | [`router`] | Typsicherer Router mit Pfadparametern (`:id`), `routes!`-Makro |
//! | [`server`] | `TcpListener`-Server, fester Worker-Pool, Keep-Alive, Limits |
//! | [`middleware`] | Kette: Logging, Sicherheits-Header, CORS |
//! | [`ratelimit`] | Rate-Limit je IP: Token-Bucket, begrenzte Tabelle |
//! | [`protokoll`] | Zugriffsprotokoll als Ringpuffer mit fester Grenze |
//! | [`static_files`] | Statische Dateien mit Traversal-Schutz + Content-Type |
//! | [`json`] | Minimal-JSON (`Value`, Parser, Serializer) fuer REST |
//! | [`api`] | REST-Helfer (Query, Pfadparam, JSON-Antwort, Fehlerabbildung) |
//! | [`kompression`] | gzip fuer dynamische Antworten (JSON, erzeugte Seiten) |
//! | [`sitzung`] | Cookies, HMAC-signierte Sitzungen, sichere Merkmale |
//! | [`ws`] | WebSocket (RFC 6455): Handshake + Frames, ohne Fremdcrate |
//! | [`sha1`] | SHA-1 in reinem Rust (nur fuer WS-Handshake, RFC 6455) |
//! | [`base64`] | Base64 in reinem Rust (nur fuer WS-Handshake) |
//! | [`h2`] | HTTP/2 (RFC 9113): Frames, Streams, Flusskontrolle, Angriffsschutz |
//! | [`hpack`] | HPACK-Decoder (RFC 7541) mit Tabellengrenze, Huffman |
//! | [`tls`] | HTTPS ueber `narsil_tls::server`, ALPN waehlt HTTP/1.1 oder h2 |
//!
//! # GRENZEN, benannt statt verschwiegen
//!
//! * **Kein TLS/HTTPS.** Wie der Client (`narsil-net`, ADR-046/047):
//!   TLS ist 20–40 Bausteine (Zustandsmaschine, Schluesselaustausch,
//!   Zertifikatskette, Chiffren, Integritaet). Wer es halb baut, baut
//!   eine Sicherheitsluecke mit Dokumentation. Siehe [`tls`].
//! * **HTTP/2 ja, HTTP/3/QUIC nein.** HTTP/2 steht in [`h2`] (Frames,
//!   HPACK, Streams, Flusskontrolle, Rapid-Reset- und Flood-Schutz) und
//!   laeuft ueber `h2` aus ALPN. HTTP/3 braucht UDP + QUIC + QPACK —
//!   das ist ein eigenes Projekt. Siehe [`tls::roadmap_http3`].
//! * **Kein Tokio, kein async.** `tokio` ist ein Fremdcrate. Der Server
//!   nutzt `std::net::TcpListener` + einen FESTEN Worker-Pool
//!   ([`ServerConfig::worker`]) statt Thread-pro-Verbindung. Die Zahl der
//!   Threads haengt von der Konfiguration ab, NICHT von der Last — das
//!   ist der Unterschied, der diese Maschine ueberleben laesst (Regel M6).
//! * **Kein Fremd-Crate, auch kein `num_cpus`.** Die Poolgroesse kommt aus
//!   [`std::thread::available_parallelism`].
//! * **Kein vollstaendiges GraphQL, kein gRPC.** REST ist vollstaendig
//!   (Router + JSON + Query + Fehler). GraphQL bietet eine begrenzte
//!   Abfrage mit Feldern und Argumenten ([`api::graphql`]); gRPC als expliziten Fehler mit
//!   Begruendung ([`tls::grpc_hinweis`]). Ein halbes gRPC waere
//!   Protokollbruch.
//!
//! # REGELN
//!
//! * Kein `unsafe` (Workspace-Lint `forbid`).
//! * Kein `unwrap`/`expect`/`panic` auf Netz- oder Dateieingaben.
//!   Kaputte Anfragen sind die Norm, nicht die Ausnahme.
//! * Limits sind Konstanten mit Tests: [`MAX_HEADER_BYTES`],
//!   [`MAX_BODY_BYTES`], [`MAX_ROUTEN`].
//! * Jede Antwort hat einen Typ, jeder Fehler einen Grund.
//!
//! # BEISPIEL
//!
//! ```rust
//! use narsil_server::{Router, Server, http::{Method, Response}};
//!
//! let mut router = Router::neu();
//! router.get("/hello/:name", |req| {
//!     let name = req.param("name").unwrap_or("Welt");
//!     Response::text(200, format!("Hallo {name}"))
//! });
//! router.get("/api/zeit", |_req| {
//!     Response::json(200, r#"{"server":"narsil"}"#)
//! });
//!
//! // Server::neu(router) + server.serve_once(&listener) in Tests;
//! // produktiv: server.laufen("127.0.0.1:8080").unwrap();
//! let _server = Server::neu(router);
//! ```

/// Die Beispiele aus `README.md` als **laufende** Tests.
///
/// added 01.10.2026, Rolle SERVER, Punkt V4 (AUS `team/AUTONOM.txt`:
/// "README.md mit lauffaehigen Beispielen - jedes Beispiel muss
/// wirklich bauen"). DER WEG: `#[cfg(doctest)]` macht dieses Feld zum
/// Traeger der Doc-Tests aus `README.md`, und `werkzeuge/bau.sh
/// narsil-server test` fuehrt sie mit. Ein Beispiel, das nicht
/// kompiliert oder das behauptete Ergebnis nicht liefert, faellt also
/// beim Testen auf — nicht beim Reviewen.
///
/// DER BISHERIGE ZUSTAND war eine README ohne diesen Haken: sie
/// behauptete, die Beispiele bauten, und nichts pruefte es. Genau das
/// wurde in `team/BERICHT_SERVER_V4_ABGELEHNT.txt bemaengelt
/// ("README beeinflusst nicht den Build").
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeBeispiele;

pub mod api;
pub mod base64;
pub mod extraktor;
pub mod h2;
pub mod h3;
pub mod hpack;
pub mod http;
pub mod json;
pub mod kompression;
pub mod middleware;
pub mod protokoll;
pub mod ratelimit;
pub mod router;
pub mod server;
pub mod sha1;
pub mod sitzung;
pub mod sitzung_hmac;
pub mod static_files;
pub mod tls;
pub mod ws;

pub use http::{Method, Request, Response, Status};
pub use router::Router;
pub use server::{Server, ServerConfig};

/// Maximale Header-Groesse einer Anfrage in Bytes.
///
/// 16 KiB reichen fuer jede ehrliche Anfrage (Zeile + ~30 Header).
/// Darueber ist es kein Browser, sondern ein Angriff oder ein Fehler —
/// und beides beantwortet man mit `431`, nicht mit mehr Speicher.
pub const MAX_HEADER_BYTES: usize = 16 * 1024;

/// Maximale Body-Groesse in Bytes (10 MiB, wie `narsil-net`).
///
/// Schutz gegen endlose Streams. Wer mehr braucht, streamt — das ist
/// ein anderes Projekt, kein groesseres Limit.
pub const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;

/// Maximale Routen im Router.
///
/// 1024 Routen sind mehr als jede Narsil-Seite braucht. Die Grenze
/// schuetzt vor versehentlichem Routen-Wachstum in generiertem Code.
pub const MAX_ROUTEN: usize = 1024;

// ---------------------------------------------------------------------------
// WEITERE GRENZEN (Regel M3: jede Schleife, die von Eingabe abhaengt, hat
// eine harte Obergrenze als benannte Konstante mit Begruendung.)
// ---------------------------------------------------------------------------

/// Maximale Laenge der Anfragezeile in Bytes (`METHODE ZIEL VERSION`).
///
/// 8 KiB sind mehr, als jeder ehrliche Pfad mit Query braucht. Darueber
/// ist es kein Pfad, sondern ein Angriff — Antwort `414`, nicht `400`,
/// weil der Unterschied fuer die Fehlersuche zaehlt.
pub const MAX_ANFRAGEZEILE: usize = 8 * 1024;

/// Maximale ANZAHL von Headern je Anfrage.
///
/// 16 KiB reichen fuer ~30 echte Header. Ohne diese Zahl waere die
/// Grenze nur BYTESCHNELL: 500 winzige Zeilen koennten unter dem
/// Byte-Limit bleiben und trotzdem die HashMap zerlegen. Antwort `431`.
pub const MAX_HEADER_ANZAHL: usize = 200;

/// Maximale Anzahl von Chunks in einem `Transfer-Encoding: chunked`-Body.
///
/// Jeder Chunk kostet eine Zeile + einen Leseaufruf. Bei 1 Byte je Chunk
/// waeren das 10 Mio Runden fuer einen 10-MiB-Body — also hart
/// begrenzt. Darueber: `400` (kein ehrlicher Client schickt das).
pub const MAX_CHUNKS: usize = 100_000;

/// Maximale Anzahl von Trailer-Zeilen nach dem letzten Chunk.
///
/// Ein Trailer-Block ohne Obergrenze waere ein zweiter Header-Satz
/// ohne Byte-Limit. Darueber: `400`.
pub const MAX_TRAILER: usize = 64;

/// Maximale Anzahl LEERER Zeilen vor der Anfragezeile (RFC 9112: eine).
///
/// Vier sind Grosszuegigkeit gegenueber echten Clients; mehr ist
/// Rauschen, das den Parser hin und her schickt. Danach `400`.
pub const MAX_LEERE_ZEILEN: usize = 4;

/// Gesamtdauer EINER Anfrage (Slowloris-Schutz).
///
/// Lese-Timeouts allein helfen gegen Slowloris nicht: 1 Byte pro
/// 100 ms halt jeden einzelnen Read frisch. Nur eine UHR ueber die
/// ganze Anfrage faengt das. Darueber: `408` und Verbindung zu.
pub const MAX_ANFRAGE_DAUER: std::time::Duration = std::time::Duration::from_secs(10);

/// Maximale Anzahl von Middleware-Gliedern je Server.
///
/// Die Kette wird rekursiv abgearbeitet (M5: Tiefe begrenzt), und jedes
/// Glied kostet einen Funktionsaufruf pro Anfrage. Darueber: das
/// Eintragen gibt `false` zurueck, der Server bleibt bedienbar.
pub const MAX_MIDDLEWARE: usize = 16;
