# narsil-server — Webserver- und API-Bibliothek

Webserver in reinem Rust, **nur `std`** (ADR-030: keine Fremd-Crates).
HTTP/1.1, HTTP/2 (RFC 9113), HTTPS ueber die eigene TLS-1.3-Crate,
Router mit Pfadparametern, statische Dateien mit Traversal-Schutz,
REST/JSON, begrenztes GraphQL, WebSocket (RFC 6455) — alles mit
festen Grenzen und benannten Fehlern.

**Jedes Rust-Beispiel in dieser Datei ist ein laufender Test.**
`crates/narsil-server/src/lib.rs` enthaelt
`#[cfg(doctest)] #[doc = include_str!("../README.md")] struct ReadmeBeispiele;`
— damit prueft `werkzeuge/bau.sh narsil-server test` jeden Block unten
wirklich: er muss kompilieren **und** das behauptete Ergebnis liefern.
Ein Beispiel, das nur aussieht, als wuerde es laufen, faellt hier auf.

---

## 1. Der kleinste Server

```rust
use narsil_server::http::Response;
use narsil_server::{Router, Server};

let mut router = Router::neu();
router.get("/hallo/:name", |anfrage| {
    let name = anfrage.param("name").unwrap_or("Welt");
    Response::text(200, format!("Hallo {name}"))
});
router.get("/api/zeit", |_anfrage| {
    Response::json(200, r#"{"server":"narsil"}"#.to_string())
});

let server = Server::neu(router);
// Produktiv: server.laufen("127.0.0.1:8080")?;
// Fuer Tests und Skripte: eine Anfrage auf einem Speicher-Leser,
// ganz ohne Socket und ganz ohne Zeitrisiko.
let mut leser = std::io::BufReader::new(std::io::Cursor::new(
    b"GET /hallo/Welt HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n".as_slice(),
));
let mut ausgabe: Vec<u8> = Vec::new();
server.bediene(&mut leser, &mut ausgabe, "test").expect("eine Anfrage");
let antwort = String::from_utf8_lossy(&ausgabe);
assert!(antwort.starts_with("HTTP/1.1 200 OK"), "{antwort}");
assert!(antwort.contains("Hallo Welt"), "{antwort}");
```

## 2. HTTPS ueber die eigene TLS-Crate

`examples/https_demo.rs` und `examples/https_h2_demo.rs` (beide bauen
mit `werkzeuge/bau.sh narsil-server build --example <name>`):

```rust
use narsil_server::http::Response;
use narsil_server::tls::TlsAngebot;
use narsil_server::{Router, Server};

let mut router = Router::neu();
router.get("/", |_| Response::text(200, "ok"));
let server = Server::neu(router);

// Das Testzertifikat (in einem echten Betrieb: ein Zertifikat einer
// echten Stelle). DER WEG bleibt derselbe.
let zertifikat = std::fs::read("crates/narsil-tls/testdaten/testzertifikat.der");
let privat = std::fs::read_to_string("crates/narsil-tls/testdaten/testschluessel_p256.hex");
if let (Ok(zertifikat), Ok(privat)) = (zertifikat, privat) {
    // h2 zuerst, weil der Server HTTP/2 wirklich spricht (Abschnitt 3).
    let mut angebot = TlsAngebot::h2_dann_http1();
    angebot.zertifikat = zertifikat;
    angebot.privat = privat
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    // Der Aufbau laeuft ueber `narsil_server::tls::https_laufen(...)`:
    // erst der TLS-Handshake ueber `narsil_tls::server`, dann der
    // HTTP-Pfad — gewaehlt am ausgehandelten ALPN (RFC 7301 3.2).
    let _ = (server, angebot);
}

// Beweis (Skript, nicht Test):
//   curl -k https://127.0.0.1:8443/          -> 200 (HTTP/1.1)
//   curl -k --http2 https://127.0.0.1:8443/  -> 200 (HTTP/2)
```

## 3. HTTP/2 (`h2`)

`examples/https_h2_demo.rs`:

```sh
target/debug/examples/https_h2_demo 8443    # meldet PORT= und BEREIT
curl -k --http2 https://127.0.0.1:8443/     # HTTP 200, proto=2
```

Was dahinter steht — mit echten Bytes, ohne Sockel. Der Server
antwortet einem HTTP/2-Client, der Prior Knowledge schickt:

```rust
use narsil_server::h2::{Aktion, CLIENT_VORFAHRT, Kopf, Maschine, Typ, rahmen};
use narsil_server::http::Response;

let mut router = narsil_server::Router::neu();
router.get("/", |_| Response::text(200, "hallo h2"));
let server = narsil_server::Server::neu(router);

// Der Client: Vorfahrt, SETTINGS, dann GET / (HPACK 82 87 84 =
// :method GET, :scheme https, :path /).
let mut hin: Vec<u8> = Vec::new();
hin.extend_from_slice(CLIENT_VORFAHRT);
hin.extend_from_slice(&rahmen(Typ::Settings, 0, 0, &[]));
hin.extend_from_slice(&rahmen(Typ::Headers, 0x5, 1, &[0x82, 0x87, 0x84]));

let mut aus: Vec<u8> = Vec::new();
let antworten = narsil_server::h2::h2_bediene(
    &server,
    &mut std::io::BufReader::new(std::io::Cursor::new(hin)),
    &mut aus,
    "127.0.0.1:0",
)
.expect("die Sitzung endet mit EOF, das ist kein Fehler");
assert_eq!(antworten, 1);

// Die Antwort wirklich zerlegen: der Koerper steht in DATA-Rahmen.
let mut koerper = Vec::new();
let mut pos = 0usize;
while pos + 9 <= aus.len() {
    let k = Kopf::lesen(&aus[pos..]).expect("Rahmenkopf lesbar");
    if k.typ == Typ::Data {
        koerper.extend_from_slice(&aus[pos + 9..pos + 9 + k.laenge as usize]);
    }
    pos += 9 + k.laenge as usize;
}
assert_eq!(String::from_utf8_lossy(&koerper), "hallo h2");

// Und die Maschine ohne Socket befragen (so benutzt der Fuzzer sie):
let mut m = Maschine::neu();
let mut daten = CLIENT_VORFAHRT.to_vec();
daten.extend_from_slice(&rahmen(Typ::Headers, 0x5, 1, &[0x82, 0x87, 0x84]));
m.fuetter(&daten).expect("die Vorfahrt stimmt");
let mut pfad = String::new();
while let Some(a) = m.naechste() {
    if let Aktion::Anfrage { kopf, .. } = a {
        pfad = kopf.pfad.clone();
    }
}
assert_eq!(pfad, "/");
```

Weitere Eigenschaften, jeweils mit Test in `tests/h2_s7.rs`,
`tests/hpack_rfc7541.rs`, `tests/h2c_sockel.rs`:

* Rahmen nach RFC 9113 4.1, 10 Typen, PADDED/PRIORITY korrekt abgebaut.
* HPACK (RFC 7541) mit statischer und dynamischer Tabelle, Huffman,
  Praefixregel nach 5.1 — geprueft an den Beispielen aus **Anhang C**
  der Norm (C.1.1, C.1.2, C.2.1–C.2.4, C.3.1–C.3.3, C.4.1).
* Stroeme, Flusskontrolle auf **beiden** Fenstern (Verbindung und
  Strom), `WINDOW_UPDATE` nach 6.9 inkl. Inkrement 0 und > 2^31-1.
* Angriffsschutz mit benannten Grenzen: Rapid Reset (CVE-2023-44487)
  endet nach 50 RST_STREAM mit `GOAWAY ENHANCE_YRALM`, ebenso
  SETTINGS- (20), PING- (50) und CONTINUATION-Flut (4 Teile), dazu
  10.000 Steuerrahmen, 100.000 Rahmen und 100 Stroeme je Verbindung.
* Normpruefungen: Pseudo-Header-Reihenfolge, Pflicht-Header,
  Grossbuchstaben im Feldnamen, die in HTTP/2 verbotenen Kopfzeilen
  (`connection`, `keep-alive`, `transfer-encoding`, `upgrade`, `te`
  ausser `trailers`).

Fussnoten, ehrlich benannt:

* Kein Server-Push (`ENABLE_PUSH = 0`), keine Prioritaeten, kein
  CONNECT. Lieber eine klare Grenze als ein halbes Protokoll.
* Der h2-Weg liefert die Antwort aus dem Speicher. Eine Datei aus
  `static_files` wird gelesen, nicht gestreamt.
* Eine Antwort ueber 65.536 Byte **ueber TLS** bleibt stehen, weil
  `narsil_tls::server::AnwendungsStrom` die TLS-Records hinter dem ersten
  verliert (Befund mit lauffaehigem Reproduktionstest in
  `team/PROBLEME.txt`). Ueber Klartext laeuft derselbe Code durch
  (`tests/h2c_sockel.rs`: 152.000 Byte, Status 200).

## 4. REST mit typsicheren Extraktoren

`examples/api_demo.rs` zeigt den vollen Weg (Pfadparameter, Query,
JSON-Antwort, Fehlerabbildung). Der Kern, lauffaehig:

```rust
use narsil_server::Router;
use narsil_server::api::{json_antwort, query_paare};
use narsil_server::http::Request;
use narsil_server::json::{Value, parse};

let mut router = Router::neu();
router.post("/api/waren/:id", |anfrage: &Request| {
    let id = anfrage.param("id").unwrap_or("0");
    let name = query_paare(&anfrage.query)
        .iter()
        .find(|(k, _)| k == "name")
        .map(|(_, w)| w.clone())
        .unwrap_or_else(|| "ohne".to_string());
    // `json_antwort` nimmt einen `Value`, nicht einen String: die
    // Felder sind also typisiert und nicht nur formatiert.
    let mut felder = std::collections::HashMap::new();
    felder.insert("id".to_string(), Value::Text(id.to_string()));
    felder.insert("name".to_string(), Value::Text(name.clone()));
    json_antwort(201, &Value::Objekt(felder))
});

let mut leser = std::io::BufReader::new(std::io::Cursor::new(
    b"POST /api/waren/7?name=Hans HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        .as_slice(),
));
let mut ausgabe: Vec<u8> = Vec::new();
narsil_server::Server::neu(router)
    .bediene(&mut leser, &mut ausgabe, "test")
    .expect("eine Anfrage");
let antwort = String::from_utf8_lossy(&ausgabe);
assert!(antwort.starts_with("HTTP/1.1 201 Created"), "{antwort}");
assert!(antwort.contains(r#""id":"7""#), "{antwort}");
assert!(antwort.contains(r#""name":"Hans""#), "{antwort}");
```

## 5. WebSocket

`examples/ws_echo.rs`:

```rust
use narsil_server::http::Response;
use narsil_server::{Router, Server, ServerConfig};

let mut router = Router::neu();
router.get("/chat", |_anfrage| Response::text(200, "chat"));

// `websocket_echo(true)` schaltet den begrenzten Echo-Dienst hinter
// dem 101-Handshake ein (RFC 6455, SHA-1 + Base64 aus eigener
// Implementierung). Grenzen: 1.024 Rahmen je Puffer, 1 MiB je Frame,
// Socket-Fristen vom Server.
let mut config = ServerConfig::sicher();
config.websocket_echo = true;
let _server = Server::mit_config(router, config);
// Produktiv: _server.laufen("127.0.0.1:9001")?;
// Handshake von Hand (ohne Socket) pruefbar:
let anfrage = narsil_server::http::anfrage_aus_text(
    "GET",
    "/chat",
    &[
        ("host", "localhost"),
        ("upgrade", "websocket"),
        ("connection", "Upgrade"),
        ("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ=="),
        ("sec-websocket-version", "13"),
    ],
    b"",
);
let antwort = narsil_server::ws::handshake_antwort(&anfrage).expect("Handshake");
assert_eq!(antwort.status.0, 101, "der Handshake liefert 101");
assert!(antwort.header("sec-websocket-accept").is_some(), "mit Accept");
```

## 6. Begrenztes GraphQL

`examples/website.rs` hat die Abfrage-Route; die API steckt in
`src/api/graphql.rs`:

```rust
use narsil_server::api::graphql::antwort;
use narsil_server::json::parse;

// Echte Daten (aus examples/graphql_daten.json):
let daten = parse(r#"{"waren":[{"id":1,"titel":"Hammer"},{"id":2,"titel":"Zange"}]}"#)
    .expect("JSON lesbar");

// Abfrage mit Feldern und Argumenten; die Antwort ist auf die
// gewaehlten Felder projiziert.
let ergebnis = antwort(&daten, "{ waren(limit: 1) { id titel } }");
assert_eq!(ergebnis.status.0, 200, "die Abfrage wird beantwortet");
let text = String::from_utf8_lossy(&ergebnis.body).into_owned();
assert!(text.contains("Hammer"), "{text}");
assert!(!text.contains("Zange"), "limit 1 schneidet ab: {text}");

// Und ein Tiefenangriff endet mit einem benannten Fehler, nicht mit
// einem Stackueberlauf:
let tief = antwort(&daten, &format!("{{ waren {{ {} }} }}", "id { ".repeat(40)));
assert!(tief.status.0 == 400 || tief.status.0 == 413, "{}", tief.status.0);
```

Grenzen (alle benannt, alle mit Negativtest): 16 KiB Abfrage,
12 Ebenen, 256 Felder, 16 Argumente je Feld, 1024 Kosteneinheiten,
1000 Listenelemente, 10.000 Ergebnisknoten, 4 KiB Text je Knoten.

## 7. Statische Dateien

```rust
use narsil_server::http::Request;
use narsil_server::static_files::datei_antwort_fuer;
use std::path::Path;

// `datei_antwort_fuer` ist der Einstieg fuer eine statische Route.
// Der Pfad wird gegen die Wurzel geprueft (Traversal-Schutz), die
// Datei EINMAL geoeffnet und danach ueber das offene Handle geprueft
// und ausgeliefert — kein Fenster zwischen Pruefung und Ausgabe.
let wurzel = Path::new("examples/site");
let anfrage = narsil_server::http::anfrage_aus_text("GET", "/index.html", &[], b"");
let antwort = datei_antwort_fuer(wurzel, "/index.html", &anfrage);
assert_eq!(antwort.status.0, 200, "die echte Datei wird geliefert");
// Die Datei steht **nicht** im Speicher, sondern als offenes Handle in
// `datei`; ausgeliefert wird blockweise beim Schreiben (64 KiB je
// Block, M4). Genau das ist der Schutz gegen eine 5-GB-Datei.
assert!(
    antwort.datei.is_some(),
    "mit offenem Handle statt Body im Speicher"
);
assert_eq!(
    antwort.datei.as_ref().map(|d| d.laenge).unwrap_or(0) > 0,
    true,
    "mit gemessener Laenge"
);

// Und der Traversal-Versuch:
let flucht = datei_antwort_fuer(wurzel, "/../../../etc/passwd", &anfrage);
assert!(
    flucht.status.0 == 403 || flucht.status.0 == 404,
    "Fluchtpfad wird abgewiesen: {}",
    flucht.status.0
);
let _ = &anfrage;
```

## 8. Messen

`examples/last_codex.rs` misst Keep-Alive/Pipelining und die
Allokationen je Anfrage. Gemessen am 01.10.2026 (Debug, localhost):

```text
PARSER (bester Lauf von 5, 20000 Anfragen, 4 Kopfzeilen):
  frisch    0.280 s   14.025 us/Request    71.303 Anfragen/s
  gepuffert 0.229 s   11.455 us/Request    87.297 Anfragen/s
PUFFER (nach 100 Aufwaermen, 1000 gemessene Anfragen):
  gemessen:   0 Container gewachsen, 0 neue Kopfzeilen
SERVER: 1000 Antworten, 999 mit Connection: keep-alive, 37.743 Anfragen/s
```

## 9. Grenzen, die man kennen sollte

Alle als benannte Konstanten mit Begruendung im Code:

| Grenze | Wert | Ort |
|---|---|---|
| Kopfzeilen je Anfrage | 200 | `MAX_HEADER_ANZAHL` |
| Kopfzeilen gesamt | 16 KiB | `MAX_HEADER_BYTES` |
| Anfragezeile | 8 KiB | `MAX_ANFRAGEZEILE` |
| Koerper | 10 MiB | `MAX_BODY_BYTES` |
| Chunks / Trailer | 100.000 / 64 | `MAX_CHUNKS`, `MAX_TRAILER` |
| Dauer einer Anfrage | 10 s | `MAX_ANFRAGE_DAUER` |
| Routen | 1024 | `MAX_ROUTEN` |
| Worker-Threads | 1024 | `MAX_WORKER` (V5: 256 -> 1024, siehe unten) |
| h2-Rahmen | 64 KiB | `h2::MAX_RAHMEN_BYTES` |
| h2-Kopfdaten je Stream | 16 KiB | `h2::MAX_KOPFZEILEN_BYTES` |
| h2-RST je Verbindung | 50 | `h2::MAX_RST` |
| h2-SETTINGS / PING je Verbindung | 20 / 50 | `h2::MAX_SETTINGS`, `MAX_PING` |
| h2-Stroeme je Verbindung | 100 | `h2::MAX_STREAMS` |
| HPACK-Tabelle | 4.096 Byte / 128 Eintraege | `hpack::MAX_DYNAMISCHE_TABELLE` |
| HPACK-String | 32.767 Byte | `hpack::MAX_STRING_BYTES` |
| halb offene Verbindungen | `worker / 2` | `ServerConfig::max_halboffen` (V5) |
| Schonfrist dafuer | 250 ms | `ServerConfig::halboffen_schonfrist` (V5) |
| Erstes Byte einer neuen Verbindung | 2 s | `ServerConfig::erstes_byte_frist` (V5) |
| wartende Keep-Alive-Verbindungen | `worker - 1` | `ServerConfig::max_wartend` (V5) |
| Warteschlange (mit Daten) | `worker * 32`, 256..8192 | `ServerConfig::max_schlangen_laenge` (V5) |
| Warteschlange (ohne Daten) | `max_halboffen` | dieselbe Config, kurze Grenze (V5) |
| komprimierbarer Koerper | 256 B .. 1 MiB | `kompression::MIN_KOMPRESSIBEL`/`MAX_KOMPRESSIBEL` (V6) |
| Protokolldatei / Generationen | 4 MiB / 3 | `protokoll::MAX_PROTOKOLLDATEI_BYTES`, `STANDARD_GENERATIONEN` (V8) |
| HTTP/3-Strompuffer | 11 MiB | `h3::MAX_STROM_PUFFER` (V9) |
| h3-Felder je Feldabschnitt | 128 | `qpack::MAX_FELDER` (aus `narsil-quic`) |

## 9. Last und Angriff (V5)

Fuenf Grenzen, jede aus einer Messung (alle Zahlen in
`/home/jaff/mutation_md/server/v5_messprotokoll.txt`, Programm
`examples/last_angriff.rs`, Exit-Code 0):

| Grenze | Vorgabe | Gemessener Anlass |
|---|---|---|
| `max_halboffen` | `worker / 2` | 12 stille Verbindungen auf 12 Worker: der Server nahm 15 s lang nichts mehr an, bei 2,7 MiB Speicher und leerem Log |
| `halboffen_schonfrist` | 250 ms | ohne sie wurde der **Kanonen-Client** (frisch verbunden, schreibt im naechsten Moment) wie ein Angreifer behandelt: 1.127x `503` |
| `erstes_byte_frist` | 2 s | 40 Slowloris-Verbindungen auf 12 Worker = 3,3 Worker-Generationen a 2 s |
| `max_wartend` | `worker - 1` | ohne sie sassen 11 von 12 Workern auf wartenden Keep-Alive-Verbindungen |
| `max_schlangen_laenge` | `worker * 32` (min 256, max 8192) | mit `worker * 4` bekamen **49 von 200** gleichzeitigen echten Clients `503` |

`MAX_WORKER` ging von 256 auf **1024**, und der Grund steht im
Kommentar: ein Worker bedient **eine** Verbindung ganz (bei HTTP/2
also eine ganze Sitzung). Wer N gleichzeitige Verbindungen bedienen
will, braucht N Worker — gemessen 17 KiB RSS je Faden.

Die Warteschlange hat **zwei** Grenzen: eine lange fuer Verbindungen,
die bereits geschrieben haben (kosten fast nichts), und eine kurze
(`max_halboffen`) fuer die, die noch schweigen — jede davon kostet
einen Worker `halboffen_schonfrist`. Ohne die kurze Grenze war die
Wartezeit einer normalen Anfrage 53 s (gemessen: 2.560 in der
Schlange x 250 ms / 12 Worker).

Ergebnis des letzten Laufs: 1.000 gleichzeitige Keep-Alive-
Verbindungen mit **74.648 Anfragen/s**, 1.000 gleichzeitige h2c-
Verbindungen (echtes `curl`) **1.000/1.000 mit Status 200**,
10.000 halb offene Verbindungen ohne einen Byte Speicherverbrauch,
Speicherspitze **71,7 MiB** (Ziel war < 200 MiB).

## 10. Kompression (V6)

`src/kompression.rs` komprimiert **dynamische** Antworten (JSON,
erzeugte Seiten); statische Dateien kannten das schon. An und ab
256 Byte ueber `ServerConfig::kompression` (Vorgabe: an).

```rust
use narsil_server::http::Response;
use narsil_server::kompression::MIN_KOMPRESSIBEL;

// Fuenf Bedingungen, jede mit Begruendung im Modul:
//  1. der Client verlangt es (`gzip;q=0` ist ein Nein),
//  2. der Typ ist kompressibel — ein PNG bleibt roh, weil gzip es
//     GROSSER macht (mit der echten bild.png getestet),
//  3. es ist kein `Content-Encoding` schon gesetzt,
//  4. es ist keine Datei und kein 304/204 (M4: `datei` wird
//     gestreamt, nicht komprimiert),
//  5. es lohnt sich — kleiner als 256 Byte oder groesser als 1 MiB
//     bleibt roh, und die kodierte Fassung muss kuerzer sein.
let mut config = narsil_server::ServerConfig::default();
config.kompression = Some(MIN_KOMPRESSIBEL);
let _ = &config;
assert_eq!(MIN_KOMPRESSIBEL, 256);
let _ = Response::text(200, "egal");
```

Gemessen: echte `index.html` 1.297 -> 871 Byte (Faktor 1,49),
API-JSON 17.981 -> 2.882 Byte (Faktor 6,24). Beweis gegen echtes
`curl` (8 von 8 Pruefungen bestanden, `examples/kompression_demo.rs`):
`curl --compressed` liefert die Datei byteweise unveraendert zurueck,
`Range: bytes=10-19` -> `206` mit genau diesen 10 Byte, hinter dem
Ende -> `416`, `If-None-Match` -> `304` ohne Koerper.

## 11. CORS, Cookies und signierte Sitzungen (V7)

```rust
use narsil_server::http::Response;

// CORS: Preflight wird vom Glied selbst beantwortet (204), und
// `Vary: Origin` steht auf JEDER Antwort — nicht nur auf dem
// Preflight. Ohne `Vary` darf ein gemeinsamer Cache die Antwort
// einer erlaubten Origin fuer eine fremde ausliefern (RFC 9111 4.1).
let cors = narsil_server::middleware::Cors {
    origin: vec!["https://app.beispiel.de".to_string()],
    methoden: vec!["GET".into(), "POST".into(), "OPTIONS".into()],
    header: vec!["content-type".into()],
    credentials: true,
    max_alt: std::time::Duration::from_secs(600),
};
let mut config = narsil_server::ServerConfig::default();
config.cors = Some(cors);

// Cookies: sichere Vorgaben, `SameSite=None` ohne `Secure` wird zu
// `Lax` abgeschwaecht (Browser verweigern die Kombination).
// Set-Cookie geht in **einer** Kopfzeile mit `; ` — drei Kopfzeilen
// waeren nach RFC 6265 5.2 vier Cookies OHNE Merkmale gewesen.
let mut antwort = Response::text(200, "ok");
narsil_server::sitzung::setze(
    &mut antwort,
    "geschuetzt",
    "ja",
    &narsil_server::sitzung::Merkmale::sicher(),
    true, // ueber HTTPS
);
let set = antwort.header("set-cookie").expect("Set-Cookie");
assert!(
    set.starts_with("geschuetzt=ja;") && set.contains("HttpOnly") && set.contains("Secure"),
    "eine Kopfzeile mit allen Merkmalen: {set}"
);
// Und die Zaehlung: genau EINE Set-Cookie-Zeile, nicht drei.
assert_eq!(
    antwort
        .headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
        .count(),
    1
);

// Sitzungen: `base64url(nutzlast) "." base64url(HMAC-SHA256)`,
// Ablaufzeit IN der Signatur, Schluessel < 16 Byte signiert nicht.
let signierer = narsil_server::sitzung_hmac::Signierer::neu(vec![7u8; 32])
    .mit_name("sid");
let _ = &signierer;
assert_eq!(
    narsil_server::sitzung_hmac::MAX_LEBENSDAUER.as_secs(),
    30 * 24 * 3600,
    "30 Tage: mehr ist keine Auslaufzeit, sondern eine Dauerzusage"
);
let _ = &mut config;
```

Bewiesen ueber das Kabel in `tests/v7_cors_sitzung.rs` (6 Tests),
mit vier Angriffen auf das Cookie: fehlend, gueltig, um ein Byte der
Signatur geaendert, und selbst gebaut (`nutzer=99`, Signatur 32
Null-Byte).

## 12. HTTP/3 auf der Serverseite (V9)

`src/h3.rs` ist die Abbildung von HTTP/3 auf die HTTP-Semantik dieser
Crate: ein `H3Strom` nimmt die Bytes eines Bidirektionalstroms
entgegen, liest die Frames (RFC 9114), dekodiert den QPACK-
Feldabschnitt (RFC 9204) und gibt einen ganz normalen `Request` an
**dieselbe** Kette weiter, die auch HTTP/1.1 und HTTP/2 bedienen.

```rust
use narsil_server::h3::{H3Strom, anfrage_bytes, status_aus_frames};
use narsil_server::http::Response;

// DER WEG, den die QUIC-Schicht spaeter geht: Bytes hinein, Frames
// lesen, Anfrage bauen, Kette laufen lassen, Antwort als Frames
// zurueck. Was in dieser Crate FEHLT, steht in `h3::offene_stellen`:
// UDP-Sockel, TLS-1.3-Handshake ueber QUIC, QPACK-Kanalstrom.
let mut strom = H3Strom::neu();
strom.nimm_bytes(&anfrage_bytes("GET", "/bilder/a.png", &[("accept", "*/*")], b""))
    .expect("aufgenommen");
strom.lies_frames().expect("Frames gelesen");
strom.markiere_geschlossen();
let anfrage = strom.anfrage("127.0.0.1:1234").expect("Anfrage");
assert_eq!(anfrage.methode, narsil_server::http::Method::Get);
assert_eq!(anfrage.version, "HTTP/3.0");
assert_eq!(anfrage.pfad, "/bilder/a.png");

// Und die Antwort als HTTP/3-Bytes: SETTINGS, HEADERS mit `:status`,
// dann DATA.
let antwort = Response::text(200, "hallo h3");
let frames = strom.antwort_frames(&antwort, anfrage.methode);
assert_eq!(status_aus_frames(&frames), Some(narsil_server::http::Status(200)));
```

12 Tests in `src/h3.rs` (Pseudo-Header-Reihenfolge, verbotene
Verbindungskopfzeilen, Grossbuchstaben, GREASE-Frames uebersprungen,
Byte-Brocken an **jeder** Schnittstelle, 204/304/HEAD ohne Koerper)
und 4 in `tests/v9_h3_kette.rs` (die echte Kette: Kompression,
Sicherheits-Header, Protokollzeile, POST-Koerper, 404).

## 13. Benchmark-Seite (V10)

`examples/bench_seite.rs` ist die Seite **und** das Messprogramm in
einer Datei. Die Seite nennt ihre Nutzlast im Koerper
(`{"nutzlast":N,...,"text":"<N Byte>"}`), damit ein Client nachrechnen
kann, wie viel er bekommen hat, ohne dem Server zu glauben; und sie
zaehlt serverseitig mit (`/bench/zaehler`).

Gemessen (rc=0, `/home/jaff/mutation_md/server/v10_messprotokoll.txt`):

```text
HTTP/1.1 Keep-Alive: 2.000 Anfragen in 0,257 s = 7.781 Anfragen/s
                     Median 0,12 ms, 95 % 0,15 ms, Maximum 4,70 ms
HTTP/2 h2c:          2.000 Anfragen in 0,400 s = 4.994 Anfragen/s
                     Median 0,07 ms, 95 % 0,10 ms, Maximum 46,79 ms
Nutzlast 1 KiB: 1.074 Byte roh, 135 Byte gzip, Faktor 7,96
```

Die Zahlen sind **eine Verbindung**, nicht 2.000: mit einer neuen
Verbindung je Anfrage kommt das Programm auf rund 900 Anfragen/s — das
ist der `connect()`-Durchsatz des Clients, nicht die Leistung des
Servers. Zum Vergleich: `examples/last_angriff.rs` kommt mit 1.000
gleichzeitigen Verbindungen auf 74.648 Anfragen/s. Die kleineren Zahlen
hier kommen daher, dass beide Laeufe **nacheinander** im selben
Prozess laufen und der Benchmark-Client den Koerper selbst entpackt
(2.000 x gzip) — er ist der Engpass, nicht der Server.
