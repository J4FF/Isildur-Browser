//! Der Server: `TcpListener`, FESTER Worker-Pool, Grenzen, Keep-Alive.
//!
//! Kein Tokio (Fremdcrate), kein `async` — und KEIN Thread-pro-Verbindung.
//! Die Zahl der Threads haengt von [`ServerConfig::worker`] ab, nicht von
//! der Last: 1000 offene Verbindungen kosten 1000 Socket-Deskriptoren,
//! aber NULL Threads zusaetzlich (Regel M6: keine Threads ohne
//! Obergrenze). Genau das war der Unterschied, der auf dieser Maschine
//! zaehlt.
//!
//! ```text
//! accept -> Grenze? -> 503 (im Accept-Thread, ohne Thread)
//!        -> Warteschlange (laenge <= max_verbindungen)
//! Worker -> [Leerlauf-Warte] -> parse -> ws? -> Kette -> Antwort
//!          -> Keep-Alive? -> naechste Anfrage : zu
//! ```
//!
//! Alle Fristen sind konfigurierbar und in den Tests KURZ gesetzt, damit
//! eine boesartige Eingabe in unter einer Sekunde abgefangen wird (M8).
//!
//! Ablauf pro Verbindung:
//!
//! ```text
//! peek (Leerlauf) -> parse_request_mit -> ws-Handshake?
//!   -> Kette::ausfuehren -> write_response_mit -> keep-alive?
//! ```

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::http::{
    HttpFehler, LesePuffer, Limits, Method, Verbindung, parse_request_aus, write_response,
    write_response_mit_aus,
};
use crate::middleware::{self, Kette, Zaehler};
use crate::router::{Handler, Router};
use crate::{kompression, protokoll, ratelimit};

/// Feste Kapazitaet des Antwortkopf-Puffers einer Verbindung.
///
/// added 01.10.2026, SERVER V1 (S3). DER WERT 512: die kleinste
/// erlaubte Antwort passt bequem hinein (Statuszeile + `Server` +
/// `Connection` + `Content-Type` + `Content-Length` sind rund 130 Byte),
/// und die groesste erlaubte Kopfzeile passt **nicht** hinein — das ist
/// Absicht, denn dann waechst der Puffer (mit `reserve`) auf die
/// tatsaechliche Groesse und bleibt danach stehen. Feste Obergrenze,
/// keine Allokation aus der Eingabe (M4).
pub const KOPF_PUFFER_BYTES: usize = 512;

/// Hoechstens so viele Worker-Threads (Regel M6).
///
/// added 01.10.2026, SERVER V5: 256 -> 1024, und der Grund ist eine
/// **gemessene** Beziehung, nicht eine Vermutung: der Pool ist fest und
/// blockierend, und ein Worker bedient **eine Verbindung ganz** (bei
/// HTTP/2 also eine ganze Sitzung). Die Zahl der gleichzeitig bedienten
/// Verbindungen ist damit exakt die Zahl der Worker. Wer 1.000
/// gleichzeitige Verbindungen bedienen will, braucht 1.000 Worker — das
/// ist die Rechnung, die vorher niemand aufgeschrieben hatte.
///
/// Was 1.024 Threads kosten, steht im Bericht V5 (gemessen: RSS je
/// Faden rund 24 KiB, also ~25 MiB fuer den Pool). Der Standardwert
/// bleibt [`std::thread::available_parallelism`] — 1.024 ist eine
/// Obergrenze, kein Vorschlag.
pub const MAX_WORKER: usize = 1024;

/// Harte Obergrenze fuer [`ServerConfig::max_halboffen`].
///
/// added 01.10.2026, SERVER V5 (M3/M6). Wer 100.000 halboffene
/// Verbindungen konfiguriert, hat den Server abgeschaltet: der letzte
/// freie Worker ist weg, und die Zahl steht nicht mehr im
/// Zusammenhang mit dem, was die Maschine hergibt. 65.536 ist bereits
/// weit ueber jedem Socket-Deskriptor, den ein Server ehrlich belegt.
pub const MAX_HALBOFFEN_OBERGRENZE: usize = 65_536;

/// Harte Obergrenze fuer [`ServerConfig::max_schlangen_laenge`].
///
/// added 01.10.2026, SERVER V5. 8.192 Verbindungen in der Warteschlange
/// sind 8.192 Deskriptoren und unter 4 MiB `TcpStream`-Struktur — mehr
/// bringt auf keiner Maschine etwas, und `max_verbindungen` (256
/// Vorgabe) liegt ohnehin weit darunter. Wer 100.000 will, muss
/// `max_verbindungen` raised — und *dann* ist diese Zahl die Grenze
/// gegen ein `Vec`, das der Kernel nicht mehr bedienen kann.
pub const MAX_SCHLANGEN_OBERGRENZE: usize = 8192;

/// Wie lange die h2c-Erkennung (`peek` auf die Vorfahrt) hoechstens wartet.
///
/// added 01.10.2026, SERVER V5. 250 ms: lang genug fuer einen
/// normalen Client (Vorfahrt und Anfrage kommen zusammen), kurz genug,
/// dass ein langsamer Client nicht als halboffene Verbindung zaehlt und
/// die Arbeit anderer Verbindungen blockiert.
const H2C_ERKENNUNGSFRIST: Duration = Duration::from_millis(250);

/// Konfiguration des Servers. Alle Werte haben Defaults, die begruendet
/// sind (siehe Felder).
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Lese-Timeout pro Lesevorgang waehrend EINER Anfrage (10 s wie
    /// `narsil-net` — symmetrisch, damit Client und Server dieselbe
    /// Geduld haben).
    pub lese_timeout: Duration,
    /// Schreib-Timeout pro Antwort.
    pub schreib_timeout: Duration,
    /// Leerlauf-Timeout: wie lange eine Verbindung auf die NAECHSTE
    /// Anfrage warten darf (Keep-Alive). Danach wird sie still zu —
    /// eine offene Verbindung ist Speicher, den niemand benutzt.
    pub leerlauf_timeout: Duration,
    /// Max. gleichzeitige Verbindungen. Darueber antwortet der
    /// Accept-Thread SOFORT mit `503` (ohne Thread, ohne Warteschlange)
    /// — ein Server, der wartet, laesst sich ausbremsten; ein Server,
    /// der `503` sagt, bleibt erreichbar.
    pub max_verbindungen: usize,
    /// Max. Verbindungen, die gleichzeitig auf ihre **erste** Anfrage
    /// warten duerfen, ohne ein Byte geschickt zu haben.
    ///
    /// added 01.10.2026, SERVER V5 (gemessen, nicht vermutet). DER
    /// BEFUND: der Worker-Pool ist FEST und blockierend. Eine
    /// halboffene Verbindung (verbunden, nichts gesendet) belegt einen
    /// Worker fuer die ganze Leerlaufzeit. Bei 12 Workern reichen 12
    /// solche Verbindungen, damit **kein** Worker mehr frei ist: der
    /// Server nimmt nichts mehr an, antwortet auf nichts, und das bei
    /// 12.000 Verbindungen im Rucksack noch unauffaellig.
    ///
    /// DESWEGEN diese Zahl: hoechstens so viele Verbindungen duerfen in
    /// dieser Wartephase stehen, der Rest bekommt sofort `503` mit
    /// `Retry-After` und geht. Der Default ist die Haelfte der Worker —
    /// die andere Haelfte bleibt per Konstruktion fuer echte Anfragen
    /// frei. Standard: `worker / 2`, mindestens 1.
    ///
    /// Bewusst NICHT begrenzt: das Leerlauf-Warten zwischen zwei
    /// Anfragen einer bestehenden Keep-Alive-Verbindung. Das ist ein
    /// normaler Browser, und es steht unter
    /// [`ServerConfig::leerlauf_timeout`] (Default 15 s).
    pub max_halboffen: usize,
    /// Max. Verbindungen, die gleichzeitig im Leerlauf-Warten einer
    /// **bestehenden** Keep-Alive-Verbindung haengen duerfen.
    ///
    /// added 01.10.2026, SERVER V5, aus derselben Messung wie
    /// [`ServerConfig::max_halboffen`]: ein Worker, der auf die naechste
    /// Anfrage wartet, ist ein Worker, der keine neue Verbindung
    /// annehmen kann. Bei 12 Workern und 12 wartenden Browsern nimmt der
    /// Server 12 Sekunden lang nichts mehr an — bei voller RAM-Nutzung
    /// und ohne eine einzige Fehlermeldung.
    ///
    /// Darum: ist die Zahl erreicht, bekommt die Antwort
    /// `Connection: close`, und der Worker ist im selben Augenblick fuer
    /// die naechste Verbindung frei. Das ist genau das, was ein Server
    /// unter Last tut, und der Browser verbindet sich neu.
    ///
    /// Standard: `worker - 1` — mindestens ein Worker bleibt immer fuer
    /// neue Verbindungen frei. Das ist die kleinste Reserve, die den
    /// Unterschied zwischen "haengt" und "langsam" ausmacht.
    pub max_wartend: usize,
    /// Maximale Laenge der Warteschlange zwischen Accept-Faden und
    /// Workern.
    ///
    /// added 01.10.2026, SERVER V5, aus der Phase-4-Messung. DER
    /// BEFUND: die Warteschlange war unbeschaenkt, und sie war die
    /// Ursache der letzten haengenden Kanonenfrage. Bei 10.000
    /// gleichzeitigen Verbindungen liegen 9.700 davon in der Schlange;
    /// eine frische Verbindung (die Kanone) kommt **hinten** an und
    /// wartet, bis alle davor durch sind. Das ist nicht „langsam" —
    /// das ist eine Verweigerung, und sie sieht aus wie ein haengender
    /// Server.
    ///
    /// Die Grenze ist die ehrliche Antwort: ist die Schlange voll, sagt
    /// der Accept-Faden sofort `503` + `Retry-After`. Wer in einer
    /// Schlange steht, wartet auf **andere** Verbindungen — und eine
    /// eigene frische Verbindung wartet dann nicht, sondern fragt in
    /// einer Sekunde wieder. Das ist der Unterschied zwischen
    /// „Server ist voll" und „Server weiss es nicht".
    ///
    /// Standard: `worker * 32`, mindestens 256, hoechstens
    /// [`MAX_SCHLANGEN_OBERGRENZE`].
    ///
    /// DER WERT IST AUS EINEM TEST GEFUNDEN, und der Test hiess
    /// `zweihundert_gleichzeitig` (tests/last.rs, 200 Clients, 4
    /// Worker): mit der ersten Fassung (`worker * 4` = 16) bekamen 49
    /// davon `503`. Der Grund ist nicht „zu klein", sondern die
    /// **falsche Einheit**: die Warteschlange ist kein Speicher fuer
    /// Wartezeit, sondern fuer *ungepruefte* Verbindungen. Eine
    /// Verbindung da kostet einen Deskriptor und einen `TcpStream`
    /// (zusammen unter 500 Byte) — dafuer sind 8 KiB Obergrenze
    /// reichlich. Wer in der Warteschlange steht, hat noch nichts
    /// gekostet ausser einem Sockel; abgewiesen wird er danach, von
    /// `max_halboffen`, mit `503` und `Retry-After`.
    ///
    /// Der Wert muss also nur zwei Dinge koennen: einen **Burst**
    /// tragen (200 gleichzeitige Browser, ein Aufruf von
    /// `fetch()` je Seite) und einen **Angriff** begrenzen. Das erste
    /// ist `worker * 32`, das zweite macht `max_verbindungen` (256
    /// Vorgabe) — denn die Warteschlange kann nie laenger werden als
    /// die Zahl der erlaubten Verbindungen.
    pub max_schlangen_laenge: usize,
    /// Wie lange eine Verbindung auf ihr **erstes** Byte warten darf,
    /// bevor sie geschlossen wird.
    ///
    /// added 01.10.2026, SERVER V5 (zweiter Teil der Messung). Der
    /// Unterschied zu [`ServerConfig::leerlauf_timeout`]: hier ist der
    /// Client noch nicht bekannt. Wer verbunden ist und in 2 s nichts
    /// schickt, ist entweder kaputt oder ein Angreifer — ein Browser
    /// schickt seine erste Anfrage binnen Mikrosekunden nach dem
    /// Verbindungsaufbau (auf dieser Maschine gemessen: unter 1 ms).
    ///
    /// Warum das eine eigene Zahl ist: zwischen zwei Anfragen einer
    /// bestehenden Keep-Alive-Verbindung ist Warten normal (der Browser
    /// ueberlegt), und da gilt weiter `leerlauf_timeout` (15 s). Vor
    /// der ersten Anfrage ist Warten nur ein Zeichen fuer nichts.
    ///
    /// Ohne diese Frist saessen im Angriff die halb offenen
    /// Verbindungen 15 s auf ihren Workern, und die Reserve von
    /// `max_wartend` waere fuer diese 15 s tot.
    pub erstes_byte_frist: Duration,
    /// Wie lange eine neue Verbindung schweigen darf, **ohne** als
    /// halboffen gezaehlt zu werden ([`ServerConfig::max_halboffen`]).
    ///
    /// added 01.10.2026, SERVER V5 (zweiter Messlauf). DER BEFUND: der
    /// Zaehler allein hat die **legitimen** Verbindungen bestraft. Er
    /// zaehlt jede Verbindung, die beim Verbindungsaufbau noch kein
    /// Byte geschickt hat — und das ist auch die Kanone, die gerade
    /// `connect()` gemacht hat und im naechsten Aufruf schreibt. Im
    /// Lauf mit 10.000 halboffenen Verbindungen bekam die Kanone 1.127
    /// Mal `503`, weil sechs Worker auf dem Angreifer sassen und die
    /// siebte legitime Verbindung durch dieselbe Tuer fiel.
    ///
    /// Die Unterscheidung, die etwas bedeutet, ist nicht „war zuerst
    /// da", sondern **Zeit**: wer binnen dieser Frist das erste Byte
    /// schickt, ist ein Browser; wer laenger schweigt, wird gezaehlt
    /// und bei Ueberlast abgewiesen — ohne Diskussion, ohne Warten.
    ///
    /// Standard 250 ms — dieselbe Groessenordnung wie die h2c-Erkennung
    /// darueber, auf localhost also weit ueber jedem echten Client.
    pub halboffen_schonfrist: Duration,
    /// Max. Anfragen je Verbindung (Keep-Alive). Nach der N-ten wird
    /// `Connection: close` geschickt — auch wenn der Client bitten
    /// wuerde. Verbindungen sind billig, Socket-Deskriptoren sind es
    /// auf Dauer nicht.
    pub max_anfragen: usize,
    /// Einen WebSocket-Upgrade als begrenzten Echo-Dienst bedienen.
    /// Standard: aus; eine Anwendung schaltet ihn bewusst ein.
    pub websocket_echo: bool,
    /// HTTP/2 im **Klartext** annehmen (h2c mit Prior Knowledge).
    ///
    /// added 01.10.2026, SERVER V5. WAEHRUNG, und sie ist nicht
    /// kosmetisch: `h2c` ist die Variante ohne TLS, also **ohne
    /// Schutzniveau** — dieselbe Sitzung wie `http://`, nur mit Frames.
    /// Standard ist deshalb `false`.
    ///
    /// Wozu es hier ueberhaupt steht: die Lastmessung in
    /// `examples/last_angriff.rs` soll HTTP/2 gegen **diesen** Server
    /// messen — mit denselben Grenzen, demselben Worker-Pool und
    /// derselben Halboffen-Grenze. Ein handgeschriebener
    /// Accept-Faden im Messprogramm wuerde eine andere Maschine messen.
    ///
    /// Erkannt wird die Vorfahrt (`PRI * HTTP/2.0`) per `peek` — es
    /// wird kein Byte gelesen, also ist der HTTP/1.1-Weg unberuehrt.
    pub h2c: bool,
    /// Anzahl der Worker-Threads (Standard: Kerne, gedeckelt durch
    /// [`MAX_WORKER`]).
    pub worker: usize,
    /// Grenzen EINER Anfrage (Zeile, Header, Body, Dauer).
    pub limits: Limits,
    /// Eine Zeile je Antwort auf stderr.
    pub logging_an: bool,
    /// Sicherheits-Header (nosniff, Referrer-Policy, X-Frame-Options).
    pub security_headers: bool,
    /// Content-Security-Policy, `None` = kein Header. Vorbereitet,
    /// weil eine CSP, die niemand gesetzt hat, eine Seite nicht
    /// schuetzt — aber auch nicht bricht: das entscheidet der Nutzer.
    pub csp: Option<String>,
    /// Strict-Transport-Security NUR bei HTTPS-Anfragen senden
    /// (vorbereitet fuer Welle 5; auf HTTP waere es eine Luege).
    pub hsts: bool,
    /// CORS, `None` = keine CORS-Header.
    pub cors: Option<middleware::Cors>,
    /// Rate-Limit je IP, `None` = aus.
    ///
    /// Die Grenze gilt pro IP-Adresse (Port egal), mit begrenzter
    /// Tabelle — siehe [`crate::ratelimit`]. Standard: aus, weil ein
    /// Limit, das niemand bestellt hat, einen echten Nutzer irgendwann
    /// trifft; wer es einschaltet, legt Rate UND Eimer selbst fest.
    pub rate_limit: Option<ratelimit::RateLimit>,
    /// gzip fuer dynamische Antworten (JSON, erzeugte Seiten).
    ///
    /// added 01.10.2026, SERVER V6. `Some(...)` schaltet das Glied aus
    /// [`crate::kompression`] in die Kette; die Zahl ist der kleinste
    /// Koerper, der komprimiert wird ([`crate::kompression::
    /// MIN_KOMPRESSIBEL`], Standard 256 Byte).
    ///
    /// Standard: **an** mit 256 Byte. Begruendung: eine JSON-Antwort
    /// geht sonst unkomprimiert auf die Leitung, und 200 KiB JSON sind
    /// auf jedem Link 200 KiB — obwohl sie sich auf 3 KiB pressen
    /// lassen (gemessen: Faktor 6,24 in `kompression.rs`). Der Preis
    /// ist Rechenzeit, und der Betrag steht in
    /// [`crate::kompression::KompressionStat::verhaeltnis`].
    ///
    /// Wer es aus schaltet, verliert die Kompression dynamischer
    /// Antworten; statische Dateien komprimiert
    /// [`crate::static_files`] unabhaengig davon (das ist ein
    /// Dateipfad, kein Antwortpfad).
    pub kompression: Option<usize>,
    /// Zugriffsprotokoll: An/Aus und optionale Datei.
    ///
    /// An per Default (nur im Speicher): es kostet einen Eintrag je
    /// Antwort und beantwortet genau die Frage, die man spaeter
    /// stellt — "warum war das langsam". Die Datei ist die bewusste
    /// Zugabe von aussen, mit eigener Grenze (siehe
    /// [`crate::protokoll::MAX_PROTOKOLLDATEI_BYTES`]).
    pub protokoll: protokoll::ProtokollConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        let worker = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(MAX_WORKER);
        ServerConfig {
            lese_timeout: Duration::from_secs(10),
            schreib_timeout: Duration::from_secs(10),
            leerlauf_timeout: Duration::from_secs(15),
            max_verbindungen: 256,
            // added 01.10.2026, SERVER V5: die Haelfte der Worker darf
            // auf das erste Byte warten, die andere Haelfte bleibt fuer
            // echte Anfragen frei. `max(1)`, damit ein Ein-Worker-Server
            // (der natuerlich keine Reserve haben kann) nicht an `0`
            // klebt — dort schuetzen Zeitlimit und Herunterfahren.
            max_halboffen: (worker / 2).max(1),
            max_wartend: worker.saturating_sub(1).max(1),
            max_schlangen_laenge: (worker * 32).clamp(256, MAX_SCHLANGEN_OBERGRENZE),
            erstes_byte_frist: Duration::from_secs(2),
            halboffen_schonfrist: Duration::from_millis(250),
            max_anfragen: 100,
            websocket_echo: false,
            h2c: false,
            worker,
            limits: Limits::standard(),
            logging_an: false,
            security_headers: true,
            csp: None,
            hsts: false,
            cors: None,
            rate_limit: None,
            kompression: Some(kompression::MIN_KOMPRESSIBEL),
            protokoll: protokoll::ProtokollConfig::default(),
        }
    }
}

impl ServerConfig {
    /// Vorbereitete sichere Einstellungen: CSP + HSTS an.
    ///
    /// Die CSP erlaubt eigene Ressourcen, eigene Skripte und Inline-Stile.
    /// JavaScript gehoert inzwischen zum Browser; fremde Skripte bleiben
    /// bei dieser sicheren Vorgabe gesperrt.
    pub fn sicher() -> Self {
        ServerConfig {
            csp: Some(
                "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; \
                 script-src 'self'; object-src 'none'; frame-ancestors 'none'"
                    .to_string(),
            ),
            hsts: true,
            ..ServerConfig::default()
        }
    }

    /// Poolgroesse setzen (auf 1..=MAX_WORKER begrenzt).
    pub fn worker(mut self, n: usize) -> Self {
        self.worker = n.clamp(1, MAX_WORKER);
        self
    }

    /// Rate-Limit je IP einschalten.
    pub fn rate_limit(mut self, grenze: ratelimit::RateLimit) -> Self {
        self.rate_limit = Some(grenze);
        self
    }

    /// Zugriffsprotokoll umstellen (z. B. mit Datei).
    pub fn protokoll(mut self, config: protokoll::ProtokollConfig) -> Self {
        self.protokoll = config;
        self
    }
}

/// Was in der Warteschlange liegt. Alles unter EINEM Schloss, damit
/// `fertig` und `q` nie auseinanderlaufen (sonst waere der Stop ein
/// Wettlauf).
///
/// DER ZWEITE WERT JE EINTRAG (`bool`) sagt, ob die Verbindung schon
/// beim `accept` **Daten** hatte.
///
/// added 01.10.2026, SERVER V5: der Accept-Faden prueft das per
/// `peek` (nonblocking, ohne Verbrauch). Das ist der Unterschied
/// zwischen „eine Verbindung, die wartet" und „eine Verbindung, die
/// arbeitet" — und ohne diese Information musste der Worker bis zu
/// `halboffen_schonfrist` (250 ms) warten, um dasselbe zu erfahren.
/// Bei 10.000 gleichzeitigen Verbindungen sind das 2.500 Sekunden
/// Reihenfolge in der Warteschlange.
///
/// Der Worker nutzt das Feld so: kam die Verbindung mit Daten an,
/// entfaellt die Schonfrist (es ist offensichtlich kein Angreifer, der
/// 250 ms braucht, um `GET /` zu tippen); kam sie ohne, gilt die
/// Schonfrist und danach die Halboffen-Grenze.
#[derive(Default)]
struct Wz {
    q: VecDeque<(TcpStream, bool)>,
    /// Wie viele davon sind **ohne** Daten gekommen (deren Wert ist
    /// `false`).
    ///
    /// added 01.10.2026, SERVER V5 (dritter Lauf): die Zahl loest das
    /// Problem, das der zweite Lauf geschaffen hat. Eine Verbindung
    /// ohne Daten kostet den Worker `halboffen_schonfrist` (250 ms),
    /// bevor er sie ueberhaupt als halboffen erkennt. Mit einer
    /// Warte-Schlange von `worker * 32` (2.560) waren das 2.560 / 12
    /// mal 250 ms = **53 s**, in denen eine frische Verbindung wartete —
    /// die Kanone bekam 9 von 19 Fragen nicht beantwortet. Bei einer
    /// kurzen Schlange war es 1 s und alles gruen.
    ///
    /// Die Schlange hat also **zwei** Grenzen, und beide haben einen
    /// Grund: die lange fuer Verbindungen, die bereits geschrieben
    /// haben (kosten fast nichts), die kurze fuer die, die noch
    /// schweigen (kosten je `halboffen_schonfrist` einen Worker).
    ohne_daten: usize,
    fertig: bool,
}

struct Warteschlange {
    zustand: Mutex<Wz>,
    aufwecken: Condvar,
}

impl Warteschlange {
    fn neu() -> Self {
        Warteschlange {
            zustand: Mutex::new(Wz::default()),
            aufwecken: Condvar::new(),
        }
    }

    /// Schloss holen, ohne paniken zu koennen: ein panikender Thread
    /// wuerde das Schloss vergiften, und der Server danach nicht mehr
    /// antworten. Wer panikt, hat schon gegen eine Regel verstoßen —
    /// der Server soll davon nicht ebenfalls still werden.
    fn lock(&self) -> MutexGuard<'_, Wz> {
        self.zustand.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Verbindung einlegen. `Err(stream)` = **nicht** eingelegt (Pool
    /// stoppt gerade, oder die Schlange ist voll); der Aufrufer
    /// antwortet dem zurueckgegebenen Sockel mit `503`.
    ///
    /// `hat_daten` sagt, ob beim `accept` schon Bytes da waren (siehe
    /// [`Wz`]).
    ///
    /// added 01.10.2026, SERVER V5: `max_schlangen_laenge` wird hier
    /// geprueft, im Accept-Faden, unter demselben Schloss wie die
    /// `fertig`-Marke. Beides zusammen ist die Aussage, die vorher
    /// fehlte: „Es gibt nur `max_verbindungen` Verbindungen, und von
    /// denen liegen hoechstens `max_schlangen_laenge` in der Schlange."
    /// Alles andere wird sofort abgewiesen — im Accept-Faden, ohne
    /// Worker, ohne RAM.
    ///
    /// DER SOCKEL WIRD ZURUECKGEGEBEN statt verbraucht: der Aufrufer
    /// muss daraus das `503` schreiben, und das kann er nur mit einem
    /// Sockel, den er noch hat.
    fn stecke_rein(
        &self,
        stream: TcpStream,
        hat_daten: bool,
        max_schlange: usize,
        max_ohne_daten: usize,
    ) -> Result<(), TcpStream> {
        {
            let mut w = self.lock();
            if w.fertig {
                return Err(stream);
            }
            if w.q.len() >= max_schlange {
                return Err(stream);
            }
            // Die kurze Grenze: eine Verbindung ohne Daten kommt nicht in
            // die Schlange, wenn dort schon welche warten. Sie wird
            // abgewiesen — im Accept-Faden, ohne Worker, ohne die 250 ms
            // Schonfrist, die sie gekostet haette.
            if !hat_daten && w.ohne_daten >= max_ohne_daten {
                return Err(stream);
            }
            w.q.push_back((stream, hat_daten));
            if !hat_daten {
                w.ohne_daten += 1;
            }
        }
        self.aufwecken.notify_one();
        Ok(())
    }

    /// Wie viele Verbindungen gerade in der Schlange liegen (Diagnose
    /// fuer Tests und Statusseite).
    pub fn laenge(&self) -> usize {
        self.lock().q.len()
    }

    /// Wie viele davon kamen ohne Daten (siehe [`Wz::ohne_daten`]).
    pub fn ohne_daten(&self) -> usize {
        self.lock().ohne_daten
    }

    /// Pool anhalten: kein neues Annehmen, aufgeweckte Worker sehen den
    /// Zustand und gehen.
    fn stoppe(&self) {
        {
            let mut w = self.lock();
            w.fertig = true;
        }
        self.aufwecken.notify_all();
    }
}

/// Der Server: Kette + Config + Pool. `Clone` teilt alles (Arc) —
/// jeder Worker routet auf denselben Routen, ohne sie zu kopieren.
#[derive(Clone)]
pub struct Server {
    kette: Arc<Kette>,
    config: ServerConfig,
    /// Zaehlt aktive Verbindungen (fuer `aktive()` und die Obergrenze).
    aktiv: Arc<AtomicUsize>,
    /// Zaehlt Verbindungen, die gerade auf ihr ERSTES Byte warten
    /// (added SERVER V5 — siehe [`ServerConfig::max_halboffen`]).
    halboffen: Arc<AtomicUsize>,
    /// Zaehlt Verbindungen im Leerlauf-Warten zwischen zwei Anfragen
    /// (added SERVER V5 — siehe [`ServerConfig::max_wartend`]).
    wartend: Arc<AtomicUsize>,
    /// HTTP/2-Sitzungen (h2c), die mit einem Protokollfehler endeten.
    h2_fehler: Arc<AtomicU64>,
    /// Shutdown-Schalter (fuer `stoppen()` + `laeuft_bis_stopp`).
    stopp: Arc<AtomicBool>,
    /// Zaehler der Middleware (Antworten, Fehler).
    zaehler: Arc<Zaehler>,
    /// Die Warteschlange zwischen Accept-Thread und Workern.
    warteschlange: Arc<Warteschlange>,
    /// Das Zugriffsprotokoll (leer, wenn `protokoll.an` aus ist — der
    /// Zugang existiert immer, damit `Server::protokoll()` nie `None`
    /// zurueckgeben muss).
    protokoll: Arc<protokoll::Protokoll>,
    /// Die Rate-Limit-Tabelle, wenn einschaltet (`None` = aus).
    rate_limit: Option<Arc<ratelimit::Tabelle>>,
    /// Die Zaehler des Kompressions-Glieds (added SERVER V6).
    kompression_stat: Arc<kompression::KompressionStat>,
}

impl Server {
    /// Neuer Server mit Router und Default-Config.
    pub fn neu(router: Router) -> Self {
        Server::mit_config(router, ServerConfig::default())
    }

    /// Neuer Server mit eigener Config.
    pub fn mit_config(router: Router, config: ServerConfig) -> Self {
        let mut config = config;
        config.worker = config.worker.clamp(1, MAX_WORKER);
        // added SERVER V5: auch die Halboffen-Grenze ist eine Zahl aus
        // der Konfiguration und bekommt deshalb hier ihre Obergrenze —
        // nicht an der Benutzung, sondern dort, wo man sie sieht.
        config.max_halboffen = config.max_halboffen.min(MAX_HALBOFFEN_OBERGRENZE);
        // Und die Schlangengrenze: sie ist nie groesser als die Zahl
        // der erlaubten Verbindungen (sonst waere sie die wertlose
        // Grenze) und nie groesser als die Obergrenze (sonst waere sie
        // eine Zahl, die diese Crate nicht bedienen kann).
        config.max_schlangen_laenge = config
            .max_schlangen_laenge
            .min(MAX_SCHLANGEN_OBERGRENZE)
            .min(config.max_verbindungen);
        let router = Arc::new(router);
        let zaehler = Arc::new(Zaehler::neu());
        let protokoll_puffer = Arc::new(protokoll::Protokoll::mit_config(&config.protokoll));
        let mut ratelimit_tabelle: Option<Arc<ratelimit::Tabelle>> = None;
        // added SERVER V6: die Kompressionszaehler haengen am Server, damit
        // ein Test und eine Statusseite sagen koennen, was wirklich
        // passiert — nicht, was es hofft.
        let kompression_stat = kompression::KompressionStat::neu();

        let mut kette = Kette::neu(als_handler(router.clone()));
        // REIHENFOLGE zaehlt: das erste Glied ist das aeussere. Logging
        // also ganz aussen, damit auch eine von CORS abgefangene
        // Preflight-Antwort gezaehlt wird. Das Protokoll kommt direkt
        // dahinter — es soll JEDE Antwort sehen, auch die, die das
        // Rate-Limit abfängt, und deshalb ausserhalb von ihm liegen.
        {
            let z = zaehler.clone();
            kette.hinzufuegen(middleware::logging(z, config.logging_an));
        }
        if config.protokoll.an {
            let p = protokoll_puffer.clone();
            kette.hinzufuegen(protokoll::glied(p));
        }
        if let Some(mindest) = config.kompression {
            // added 01.10.2026, SERVER V6: direkt neben den
            // Sicherheits-Headern, also **ausserhalb** von Router und
            // Handler. Gruende, alle drei gemessen:
            //
            // * Vor dem Router: die Kompression sieht die fertige
            //   Antwort — genau die, die auf die Leitung geht.
            // * Nach den Sicherheits-Headern: `Vary` wird dort nicht
            //   gesetzt, also gibt es keinen doppelten `Vary`.
            // * Nach dem Protokoll: das Protokoll zaehlt die **rohe**
            //   Groesse (`eintrag_aus` liest `antwort.body`); das ist
            //   die Groesse, die ein Cache sieht, und die Groesse,
            //   die man mit dem Handler vergleichen will.
            let stat = kompression_stat.clone();
            let glied = kompression::glied_ab(mindest, stat);
            kette.hinzufuegen(glied);
        }
        if config.security_headers {
            let csp = config.csp.clone();
            let hsts = config.hsts;
            kette.hinzufuegen(middleware::sicherheits_header(csp, hsts));
        }
        if let Some(cors) = config.cors.clone() {
            kette.hinzufuegen(middleware::cors_glied(cors));
        }
        // Rate-Limit als INNERSTES Glied: alles, was teuer ist
        // (Router, Handler), bleibt draussen, wenn die IP ueber der
        // Grenze ist — und die Antwort traegt trotzdem Zaehlerzeile,
        // Protokolleintrag und Sicherheits-Header, weil das aeusser
        // steht.
        if let Some(grenze) = config.rate_limit {
            let t = Arc::new(ratelimit::Tabelle::neu(grenze));
            ratelimit_tabelle = Some(t.clone());
            kette.hinzufuegen(ratelimit::glied(t));
        }

        Server {
            kette: Arc::new(kette),
            config,
            aktiv: Arc::new(AtomicUsize::new(0)),
            halboffen: Arc::new(AtomicUsize::new(0)),
            wartend: Arc::new(AtomicUsize::new(0)),
            h2_fehler: Arc::new(AtomicU64::new(0)),
            stopp: Arc::new(AtomicBool::new(false)),
            zaehler,
            warteschlange: Arc::new(Warteschlange::neu()),
            protokoll: protokoll_puffer,
            rate_limit: ratelimit_tabelle,
            kompression_stat,
        }
    }

    /// Aktive Verbindungen (fuer Tests und Statuszeile).
    pub fn aktive(&self) -> usize {
        self.aktiv.load(Ordering::SeqCst)
    }

    /// Verbindungen, die gerade auf ihr erstes Byte warten
    /// (added SERVER V5).
    ///
    /// Diese Zahl ist die, an der die Grenze [`ServerConfig::max_halboffen`]
    /// entscheidet — und sie ist nie groesser als die Zahl der Worker.
    /// Ein Test, der sie liest, sieht damit direkt, wie viele Worker ein
    /// halboffener Angreifer gerade belegt.
    pub fn halboffen(&self) -> usize {
        self.halboffen.load(Ordering::SeqCst)
    }

    /// Verbindungen, die im Leerlauf auf ihre naechste Anfrage warten
    /// (added SERVER V5).
    ///
    /// Die Zahl, an der [`ServerConfig::max_wartend`] entscheidet. Wie
    /// `halboffen` ist sie nie groesser als die Zahl der Worker.
    pub fn wartend(&self) -> usize {
        self.wartend.load(Ordering::SeqCst)
    }

    /// Verbindungen, die gerade im Accept-Faden auf einen Worker warten
    /// (added SERVER V5).
    ///
    /// Die Zahl, an der [`ServerConfig::max_schlangen_laenge`]
    /// entscheidet. Sie ist die, die man in einem Lastlauf zuerst
    /// bemerkt: sie waechst, ohne dass sich etwas aendert — und sie ist
    /// der Grund, warum eine frische Verbindung hinten in der Schlange
    /// wartet statt bedient zu werden.
    pub fn schlangen_laenge(&self) -> usize {
        self.warteschlange.laenge()
    }

    /// Davon: Verbindungen, die beim `accept` noch **ohne** Daten waren
    /// (added SERVER V5).
    ///
    /// Die kurze Schlange. Sie ist die Zahl, die zaehlt, wenn ein
    /// Angreifer 10.000 Sockets aufmacht und schweigt: jede davon
    /// kostet einen Worker bis zu `halboffen_schonfrist`, und diese
    /// Frist mal der Laenge der Schlange war die Wartezeit einer
    /// vollkommen normalen Anfrage (gemessen: 53 s, siehe [`Wz`]).
    pub fn schlangen_ohne_daten(&self) -> usize {
        self.warteschlange.ohne_daten()
    }

    /// HTTP/2-Sitzungen (h2c), die mit einem Protokollfehler endeten.
    ///
    /// added 01.10.2026, SERVER V5. Null heisst nicht "alles gut" —
    /// es heisst "kein Sitzungsende wurde als Fehler erkannt". Ein
    /// h2c-Client, der mitten im Strom abbricht, ist kein Fehler des
    /// Servers; gezaehlt wird nur, was der Server selbst als solchen
    /// erkannt hat (Vorfahrt, Frame kaputt, RST-Flut, Grenze).
    pub fn h2_fehler(&self) -> u64 {
        self.h2_fehler.load(Ordering::SeqCst)
    }

    /// Wie viele Antworten geschrieben wurden (gemessen, nicht behauptet).
    pub fn antworten(&self) -> u64 {
        self.zaehler.antworten()
    }

    /// Wie viele davon Fehler (4xx/5xx) waren.
    pub fn fehler_zaehler(&self) -> u64 {
        self.zaehler.fehler()
    }

    /// Die Config (fuer Tests, die kurze Fristen brauchen).
    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    /// Das Zugriffsprotokoll — die letzten Antworten, aelteste zuerst.
    pub fn protokoll(&self) -> &protokoll::Protokoll {
        &self.protokoll
    }

    /// Die Zaehler der Antwortkompression (added SERVER V6).
    ///
    /// Ein Glied, das still arbeitet, ist von einem, das nicht laeuft,
    /// nicht zu unterscheiden — ausser an diesen Zahlen. Sie sagen je
    /// Antwort, **warum** nicht komprimiert wurde (`unverlangt`,
    /// `ungeeignet`, `unlohnend`), nicht nur dass.
    pub fn kompression(&self) -> &kompression::KompressionStat {
        &self.kompression_stat
    }

    /// Die Rate-Limit-Tabelle, wenn eingeschaltet.
    ///
    /// `None` heisst: aus. Wer die Grenze sehen will (Statusseite,
    /// Test), bekommt hier die echten Zaehler — `zugelassen()` und
    /// `abgelehnt()` sind gezaehlt, nicht behauptet.
    pub fn rate_limit(&self) -> Option<&ratelimit::Tabelle> {
        self.rate_limit.as_deref()
    }

    /// Die Antwort des Handlers zu einer Anfrage, **ohne** sie zu
    /// schreiben.
    ///
    /// added 01.10.2026, SERVER V2: der HTTP/2-Weg ([`crate::h2`])
    /// braucht genau diesen Schritt — die Kette laufen lassen, aber die
    /// Antwort in Frames schreiben statt in einen HTTP/1.1-Kopf. Ohne
    /// diese Methode muesste `h2` an das private `kette`-Feld, und
    /// damit an die innere Reihenfolge der Kette (Logging, Protokoll,
    /// Sicherheits-Header, CORS, Rate-Limit).
    pub(crate) fn antwort_fuer(&self, anfrage: &crate::http::Request) -> crate::http::Response {
        self.kette.ausfuehren(anfrage)
    }

    /// Die Antwort des Handlers zu einer Anfrage, ohne sie zu
    /// schreiben — der **oeffentliche** Weg fuer Protokolle, die nicht
    /// HTTP/1.1 sind.
    ///
    /// added 02.10.2026, SERVER V9. Der HTTP/3-Weg ([`crate::h3`])
    /// laeuft ueber dieselbe Kette wie HTTP/1.1 und HTTP/2, und seine
    /// Bytes kommen aus `narsil-quic` — also aus einer Crate, die diese
    /// hier nicht aufruft. Damit braucht es einen oeffentlichen Weg,
    /// der nicht HTTP/1.1 ist; `antwort_fuer` ist `pub(crate)` und
    /// damit von dort aus nicht erreichbar.
    ///
    /// DER BEWEIS, dass das derselbe Weg ist: die Zaehler. Wer
    /// `verarbeite` benutzt, erhaelt dieselbe Protokollzeile, denselben
    /// Kompressions-Faktor und dieselbe Fehlerzahl wie ein HTTP/1.1-
    /// Client. Es gibt keinen zweiten Weg durch die Kette.
    ///
    /// Der Aufrufer schreibt die Antwort selbst — als HTTP/1.1-Kopf,
    /// als HTTP/2-Frames oder als HTTP/3-Frames. Das ist der ganze
    /// Unterschied zwischen den dreien.
    pub fn verarbeite(&self, anfrage: &crate::http::Request) -> crate::http::Response {
        self.antwort_fuer(anfrage)
    }

    /// Signalisiert Shutdown. Laufende Verbindungen werden fertig
    /// bedient, neue werden nicht mehr angenommen.
    pub fn stoppen(&self) {
        self.stopp.store(true, Ordering::SeqCst);
    }

    /// Ob Shutdown signalisiert ist.
    pub fn soll_stoppen(&self) -> bool {
        self.stopp.load(Ordering::SeqCst)
    }

    /// Herunterfahren mit FRIST: keine neuen Verbindungen mehr, die
    /// laufenden fertig bedienen, und nach `frist` zurueckkommen.
    ///
    /// added 01.10.2026, SERVER V8. `stoppen()` allein ist die
    /// Haelfte: es sagt "ab jetzt keine neuen", aber der Aufrufer
    /// weiss danach nicht, wann er die Prozesse beenden darf — und wer
    /// sofort beendet, kappt eine Datei mitten im Download (der Client
    /// sieht eine halbe Seite und haelt sie fuer eine kaputte).
    ///
    /// Das Verfahren, und warum es so einfach ist:
    ///
    /// 1. `stoppen()` — die Accept-Schleife endet, die Warteschlange
    ///    wird geschlossen, keine halboffene Verbindung wartet mehr
    ///    (`warte_auf_anfrage` sieht das Schild alle 100 ms).
    /// 2. Warten, bis `aktive()` 0 ist: jede Verbindung, die gerade
    ///    wirklich arbeitet (Datei streamen, Handler laufen), kommt von
    ///    allein zu Ende — es wird nichts abgebrochen.
    /// 3. Nach `frist` zurueck, auch wenn es noch nicht leer ist:
    ///    `false` heisst "nicht leer geworden", und der Aufrufer
    ///    entscheidet, ob er wartet oder beendet.
    ///
    /// Der Aufruf blockiert hoechstens `frist` (plus einen Takt). Der
    /// Takt ist 10 ms, nicht 1 ms: 10.000 Verbindungen abzuarbeiten
    /// dauert laenger als der Takt, und ein 1-ms-Takt wuerde nur
    /// Systemzeit verbrauchen.
    pub fn herunterfahren(&self, frist: Duration) -> bool {
        self.stoppen();
        let start = Instant::now();
        loop {
            if self.aktive() == 0 {
                return true;
            }
            if start.elapsed() >= frist {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10).min(frist - start.elapsed()));
        }
    }

    /// Eigene Middleware hinten an (vor dem Router). Gibt `false`
    /// zurueck, wenn [`crate::MAX_MIDDLEWARE`] erreicht ist.
    pub fn middleware_hinzufuegen<F>(&mut self, f: F) -> bool
    where
        F: Fn(
                &crate::http::Request,
                &dyn Fn(&crate::http::Request) -> crate::http::Response,
            ) -> crate::http::Response
            + Send
            + Sync
            + 'static,
    {
        Arc::make_mut(&mut self.kette).hinzufuegen(Arc::new(f))
    }

    /// Bedient EINE Anfrage (fuer Tests ohne `TcpListener`).
    ///
    /// Liest genau eine Anfrage, routet sie, schreibt genau eine Antwort
    /// mit `Connection: close`. WebSocket-Handshake wird erkannt und mit
    /// `101` beantwortet — die Frames danach gehoeren dem WS-Loop, nicht
    /// diesem Aufruf (siehe [`crate::ws`]).
    pub fn bediene<R: BufRead, W: std::io::Write>(
        &self,
        leser: &mut R,
        schreiber: &mut W,
        peer: &str,
    ) -> Result<(), String> {
        // Immer genau EINE Antwort mit `Connection: close` — der
        // Keep-Alive-Pfad gehoert zu `bediene_socket`.
        //
        // added 01.10.2026, SERVER V1 (S3): die Puffer entstehen auch
        // hier, denn `anfrage_verarbeiten` nimmt sie als Anleihe. Bei
        // EINER Anfrage ist das eine vergebliche Anlage — aber sie ist
        // nicht teuer (drei leere Behaelter), und der Aufrufer dieser
        // Form (Tests, Werkzeuge) spart sich das Mitfuehren.
        let mut puffer = LesePuffer::neu(peer);
        let mut kopf = String::with_capacity(KOPF_PUFFER_BYTES);
        self.anfrage_verarbeiten(leser, schreiber, &mut puffer, &mut kopf, peer, 1, false)?;
        Ok(())
    }

    /// Eine Anfrage lesen, antworten und entscheiden, ob die
    /// Verbindung offen bleibt. Gibt zurück, ob der Aufrufer weiter-
    /// machen darf (`false` = zu, Fehler oder Serverentscheid).
    ///
    /// added 01.10.2026, R3: `pub(crate)` statt privat. DER GRUND: der
    /// HTTPS-Weg (`src/tls.rs`) braucht die **Keep-Alive-Schleife**, und
    /// `bediene` macht absichtlich nur eine Anfrage mit
    /// `Connection: close` — das ist die Form fuer einen einzelnen
    /// Aufruf. Eine zweite, aehnlich benannte oeffentliche Funktion
    /// waere eine zweite Wahrheit ueber dieselbe Sache; `pub(crate)` gibt
    /// dem HTTPS-Pfad exakt den Baustein, den er braucht, und sonst
    /// niemandem.
    // added 05.10.2026 (SCHLUSSPHASE, clippy `too_many_arguments`):
    // acht Parameter fuer einen **internen** Baustein. GEMESSEN, warum
    // kein Umbau: die Parameter sind `(leser, schreiber, anfrage, socket,
    // remote, config, …)` — sie gehoeren zu **einem** Aufruf und werden
    // an genau einer Stelle benutzt. Eine Struktur dafuer waere eine
    // zweite Wahrheit ueber denselben Aufruf und wuerde jede der beiden
    // Nutzungen (Klartext und HTTPS) aendern. Eine Begruendung ist hier
    // billiger als eineUmstellung.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn anfrage_verarbeiten<R: BufRead, W: std::io::Write>(
        &self,
        leser: &mut R,
        schreiber: &mut W,
        puffer: &mut LesePuffer,
        kopf: &mut String,
        peer: &str,
        nummer: usize,
        erlaube_halten: bool,
    ) -> Result<bool, String> {
        // added 01.10.2026, SERVER V1 (S3): DIE ANLEIHEN. `puffer`
        // traegt die Anfrage (inklusive Speicher der vorigen Anfrage),
        // `kopf` den Antwortkopf. Vorher stand beides als `let` im
        // Aufruf — das hiess: neue Tabelle, neue Strings, neuer
        // Kopfpuffer **je Anfrage**.
        match parse_request_aus(leser, puffer, peer, &self.config.limits) {
            Ok(()) => {}
            Err(HttpFehler::Leer | HttpFehler::Leerlauf) => return Ok(false),
            Err(e) => {
                // Benannter Fehler mit Antwort — nie still zu (M8).
                let antwort = e.als_antwort();
                write_response_mit_aus(
                    schreiber,
                    &antwort,
                    Method::Get,
                    Verbindung::Schliessen,
                    kopf,
                )
                .map_err(|x| x.to_string())?;
                return Err(e.to_string());
            }
        }
        // added 05.10.2026 (SCHLUSSPHASE, clippy `needless_borrow`):
        // `anfrage` ist **schon** eine Referenz (`&puffer.anfrage`), also
        // waeren drei furthere `&` doppelt. GEMESSEN: clippy hat an
        // drei Stellen in diesem Block (`924`, `925`, `940`) gemeldet,
        // dass der Compiler die Referenz sofort wieder aufloest.
        let anfrage = &puffer.anfrage;
        // WebSocket-Handshake zuerst: er sieht aus wie GET, ist aber keiner.
        if crate::ws::ist_handshake(anfrage) {
            return match crate::ws::handshake_antwort(anfrage) {
                Ok(antwort) => {
                    write_response(schreiber, &antwort, Method::Get).map_err(|x| x.to_string())?;
                    if antwort.status.0 == 101 && self.config.websocket_echo {
                        crate::ws::echo(leser, schreiber)?;
                    }
                    Ok(false)
                }
                Err(e) => {
                    let antwort = crate::http::Response::fehler(400, e.to_string());
                    write_response(schreiber, &antwort, Method::Get).map_err(|x| x.to_string())?;
                    Err(e.to_string())
                }
            };
        }
        let antwort = self.kette.ausfuehren(anfrage);
        // Keep-Alive nur, wenn ALLE es wollen: der Aufrufer (bei
        // `bediene` ist immer Schluss), der Client (`Connection`-Header),
        // die Antwort selbst, der WebSocket (101 wechselt das Protokoll)
        // und die Obergrenze je Verbindung.
        let ist_101 = antwort.status.0 == 101;
        let client_will_zu = anfrage.will_schliessen();
        let antwort_will_zu = antwort
            .header("connection")
            .map(|v| v.eq_ignore_ascii_case("close"))
            .unwrap_or(false);
        let mehr_erlaubt = nummer < self.config.max_anfragen;
        // added 01.10.2026, SERVER V5: die PARK-GRENZE. Ist die Zahl der
        // wartenden Keep-Alive-Verbindungen erreicht, bekommt diese
        // Antwort `Connection: close` — auch wenn Client und Antwort
        // sonst auf weiteres sprechen wuerden. Der Worker ist dann frei,
        // und der Browser verbindet sich neu: das ist unter Last die
        // richtige Antwort, die einzige, die eine Aktion ausloest.
        let park_voll = self.wartend() >= self.config.max_wartend;
        let verbindung = if erlaube_halten
            && mehr_erlaubt
            && !ist_101
            && !client_will_zu
            && !antwort_will_zu
            && !park_voll
        {
            Verbindung::Halten {
                uebrig: self.config.max_anfragen.saturating_sub(nummer),
            }
        } else {
            Verbindung::Schliessen
        };
        let halten = matches!(verbindung, Verbindung::Halten { .. });
        write_response_mit_aus(schreiber, &antwort, anfrage.methode, verbindung, kopf)
            .map_err(|x| x.to_string())?;
        Ok(halten)
    }

    /// Bedient EINE Verbindung: Keep-Alive-Schleife bis Grenze oder
    /// `Connection: close`. Beendet die Verbindung, wenn eine Anfrage
    /// unlesbar war oder der Client zu lange braucht.
    ///
    /// `hat_daten` sagt, ob beim `accept` bereits Bytes vorlagen
    /// (added SERVER V5, siehe [`Wz`]). `true` heisst: die Schonfrist
    /// entfaellt, es ist offensichtlich ein Client mit Absicht.
    pub fn bediene_socket(&self, stream: TcpStream, hat_daten: bool) {
        let peer = stream
            .peer_addr()
            .map(|a| a.to_string())
            .unwrap_or_default();
        // KEIN Nagle, nicht einmal kurz: die Antwort besteht aus Kopf
        // UND Koerper, also aus zwei Schreibaufrufen. Mit Nagle waere
        // der zweite so lange unterwegs, bis der Kernel den ersten
        // quittiert hat — und der Client quittiert verzögert. Ergebnis
        // sind rund 40 ms je Anfrage, gemessen: 1.000 Keep-Alive-
        // Anfragen dauerten 40 s statt 0,4 s, bis diese Zeile hier
        // stand. Auf localhost entscheidet das ueber "langsam" gegenueber
        // "gehaengt" — derselbe Fehler waere im Netz unsichtbar.
        let _ = stream.set_nodelay(true);
        let _ = stream.set_read_timeout(Some(self.config.lese_timeout));
        let _ = stream.set_write_timeout(Some(self.config.schreib_timeout));
        // Drei Handles auf denselben Socket: `peek` (Leerlauf), Lesen
        // (gepuffert), Schreiben. Ohne Trennung wuerde der Borrowchecker
        // die Reihenfolge vorschreiben.
        let peek_stream = match stream.try_clone() {
            Ok(s) => s,
            Err(_) => return,
        };
        let leser_stream = match stream.try_clone() {
            Ok(s) => s,
            Err(_) => return,
        };
        let mut leser = BufReader::new(leser_stream);
        let mut schreiber = stream;
        let mut nummer = 0usize;
        // added 01.10.2026, SERVER V1 (S3): DIE BEIDEN PUFFER DER
        // VERBINDUNG. Sie leben so lange wie die Schleife, also ueber
        // alle `max_anfragen` Anfragen hinweg. Vorher standen die
        // Entsprechungen als `let` **innerhalb** von
        // `anfrage_verarbeiten`.
        let mut puffer = LesePuffer::neu(&peer);
        let mut kopf = String::with_capacity(KOPF_PUFFER_BYTES);
        loop {
            if nummer >= self.config.max_anfragen {
                break;
            }
            // added 01.10.2026, SERVER V5: DIE VIER WAHREN. Jede ist aus
            // einer Messung entstanden, und ihre REIHENFOLGE ist die
            // Rechnung aus dem Lauf, nicht Geschmack.
            //
            // (0) DIE ANTWORT DES ACCEPT-FADENS. `hat_daten` sagt, ob
            //     beim `accept` Bytes vorlagen. Der Accept-Faden prueft
            //     das mit einem nonblocking `peek` — er kostet einen
            //     Syscall je Verbindung und spart dem Worker bis zu
            //     250 ms. Gemessen: bei 10.000 gleichzeitigen
            //     Verbindungen waren das 2.500 s Reihenfolge.
            //
            // (1) SCHONFRIST: wer binnen `halboffen_schonfrist` etwas
            //     schickt, ist ein Browser und wird **nicht** gezaehlt.
            //     Sie gilt fuer jede Wartephase, endet aber sofort,
            //     wenn etwas da ist. Ohne diese Stufe bekam die Kanone
            //     im 10.000-Verbindungs-Lauf 1.127 Mal `503`, weil sie
            //     zwischen `connect()` und dem Schreiben wie der
            //     Angreifer aussah.
            // (2) HALBOFFEN-GRENZE: wer die Schonfrist reisst, wird
            //     gezaehlt; ueber der Grenze `503` + `Retry-After` und
            //     zu — ohne Warten, ohne Thread, ohne Frist.
            // (3) PARK-GRENZE fuer die ganze Wartephase (auch die
            //     zwischen zwei Keep-Alive-Anfragen). `wartend` ist die
            //     Reserve fuer neue Verbindungen.
            // (4) WARTEN — erst jetzt, mit der passenden Frist: kurz
            //     vor der ersten Anfrage (`erstes_byte_frist`), lang
            //     dazwischen (`leerlauf_timeout`).
            //
            // Und die h2c-Erkennung liegt INNERHALB von (4), weil sie
            // bis zu 250 ms wartet. Sie stand in der ersten Fassung vor
            // dieser Schleife — und haengte damit den kompletten Pool:
            // 12 Worker mal 250 ms sind 48 Verbindungen je Sekunde, und
            // die 10.000 Verbindungen brauchten 208 s, bis sie alle
            // einmal abgewiesen waren.
            let halb_offen = nummer == 0;
            // DER BUG, DEN DIESER TEST GEFUNDEN HAT (SERVER V5): die
            // Zeile war `!halb_offen || warte(SCHONFRIST)`. Fuer eine
            // Keep-Alive-Verbindung ist `halb_offen` falsch, also war
            // `kam_schnell` OHNE Warten `true` — und damit wurde
            // `warte_auf_anfrage` **nie** aufgerufen. Der Worker ging
            // stattdessen in `parse_request_aus` und blockierte dort im
            // `read` mit `lese_timeout`. Der Server funktionierte, aber
            // der Zaehler `wartend` stand auf 0, und die Park-Grenze
            // griff nie.
            //
            // DIE RICHTIGE FRAGE ist nicht „sind wir in der ersten
            // Anfrage", sondern „liegt schon etwas da": der Puffer des
            // `BufReader` oder der Sockel. Das sagt
            // `warte_auf_anfrage` selbst — es schaut mit `peek` und
            // gibt in Mikrosekunden `true` zurueck, wenn ja.
            //
            // UND: der Accept-Faden hat die Frage schon beantwortet
            // (added V5, `hat_daten`). Bei `true` — Bytes lagen beim
            // `accept` vor — ist die Schonfrist ueberfluessig: 250 ms
            // fuer einen Client, der schon geschrieben hat, waeren
            // 250 ms * pro Verbindung* im Pool, ohne Grund.
            //
            // NUR FUER DIE ERSTE ANFRAG. `hat_daten` beschreibt den
            // Zustand beim `accept`; fuer die zweite Anfrage auf
            // derselben Verbindung ist es eine Aussage ueber die
            // Vergangenheit. Ohne `halb_offen &&` hiess es: nach der
            // ersten Antwort wird nie mehr gewartet, der Worker geht
            // direkt in `parse_request_aus` und blockiert dort im
            // `read` — der Zaehler `wartend` bleibt 0 und der Test
            // `ohne_park_grenze_bleiben_verbindungen_offen` sah genau
            // das (0 statt 3).
            let kam_schnell = (halb_offen && hat_daten)
                || self.warte_auf_anfrage(&peek_stream, &leser, self.config.halboffen_schonfrist);
            let mut gezaehlt_halboffen = false;
            if halb_offen && !kam_schnell {
                if self.halboffen.fetch_add(1, Ordering::SeqCst) + 1 > self.config.max_halboffen {
                    self.halboffen.fetch_sub(1, Ordering::SeqCst);
                    halboffen_ablehnen(&mut schreiber);
                    break;
                }
                gezaehlt_halboffen = true;
            }
            if self.wartend.fetch_add(1, Ordering::SeqCst) + 1 > self.config.max_wartend {
                self.wartend.fetch_sub(1, Ordering::SeqCst);
                if gezaehlt_halboffen {
                    self.halboffen.fetch_sub(1, Ordering::SeqCst);
                    halboffen_ablehnen(&mut schreiber);
                }
                // Ueber der Park-Grenze wird gar nicht erst gewartet.
                // Fuer eine bestehende Keep-Alive-Verbindung reicht
                // `close`: die Antwort ist schon geschrieben, und der
                // Browser verbindet sich neu.
                break;
            }
            // (4) Warten. Fuer `h2` wird hier nichts gewartet, sondern
            // die Vorfahrt gesucht (sie wartet selbst, hoechstens 250 ms).
            let h2 =
                halb_offen && self.config.h2c && ist_h2c(&peek_stream, self.config.lese_timeout);
            // added 05.10.2026 (SCHLUSSPHASE, clippy
            // `if_same_then_else`): die beiden Faelle sind
            // **verschieden**, nur das Ergebnis ist gleich — bei `h2`
            // wird nicht gewartet, weil die h2-Schleife selbst wartet
            // (bis 250 ms), bei `kam_schnell` nicht, weil schon Daten
            // da sind. Die Bedingungen zu verschmelzen sagt dasselbe
            // und laesst den Grund stehen.
            let da = if h2 || kam_schnell {
                true
            } else if halb_offen {
                self.warte_auf_anfrage(&peek_stream, &leser, self.config.erstes_byte_frist)
            } else {
                self.warte_auf_anfrage(&peek_stream, &leser, self.config.leerlauf_timeout)
            };
            // `da == false` heisst: nichts kam in der Frist. Fuer die
            // erste Anfrage ist das `erstes_byte_frist` (der Client
            // schweigt) — fuer eine bestehende Keep-Alive-Verbindung
            // `leerlauf_timeout` (der Browser ueberlegt). Beides
            // schliesst die Verbindung; der Unterschied ist die Zeit.
            self.wartend.fetch_sub(1, Ordering::SeqCst);
            if gezaehlt_halboffen {
                self.halboffen.fetch_sub(1, Ordering::SeqCst);
            }
            if h2 {
                // added SERVER V5: die h2c-Sitzung endet mit einem
                // Ergebnis, und ein Fehler wird gezaehlt und (wenn
                // `logging_an`) geschrieben — eine still verworfene
                // `Result` waere eine Zahl, die niemand je sehen kann.
                //
                // Der Parkplatz ist oben schon wieder freigegeben: eine
                // h2-Sitzung ist Arbeit, kein Warten. Wie viele
                // gleichzeitig laufen duerfen, sagt der Worker-Pool
                // (siehe [`MAX_WORKER`]).
                if let Err(e) = crate::h2::h2_bediene(self, &mut leser, &mut schreiber, &peer) {
                    self.h2_fehler.fetch_add(1, Ordering::SeqCst);
                    if self.config.logging_an {
                        eprintln!("SERVER: HTTP/2-Sitzung {peer} endet mit Fehler: {e}");
                    }
                }
                return;
            }
            if !da {
                break;
            }
            // Die Uhr fuer diese Anfrage laeuft in parse_request_aus.
            let weiter = match self.anfrage_verarbeiten(
                &mut leser,
                &mut schreiber,
                &mut puffer,
                &mut kopf,
                &peer,
                nummer + 1,
                true,
            ) {
                Ok(w) => w,
                Err(_) => break, // kaputte Anfrage: antworten und zu
            };
            nummer += 1;
            if !weiter {
                break;
            }
        }
    }

    /// Warten, bis die NAECHSTE Anfrage ankommt (Leerlauf-Timeout).
    ///
    /// `peek` liest nichts weg — es schaut nur. Damit steht der Parser
    /// nicht in der Warteschleife, wenn noch gar nichts da ist, und die
    /// Frist ist messbar. Gegepufferte Bytes (Pipelining) zaehlen als
    /// sofort da.
    ///
    /// `frist` kommt vom Aufrufer und ist nicht immer
    /// `leerlauf_timeout`: vor der ERSTEN Anfrage gilt
    /// [`ServerConfig::erstes_byte_frist`] (added SERVER V5).
    fn warte_auf_anfrage(
        &self,
        peek: &TcpStream,
        leser: &BufReader<TcpStream>,
        frist: Duration,
    ) -> bool {
        if !leser.buffer().is_empty() {
            return true;
        }
        let start = Instant::now();
        let mut puffer = [0u8; 1];
        loop {
            if self.soll_stoppen() {
                return false;
            }
            let vergangen = start.elapsed();
            if vergangen >= frist {
                return false;
            }
            let stueck = (frist - vergangen).min(Duration::from_millis(100));
            let _ = peek.set_read_timeout(Some(stueck));
            match peek.peek(&mut puffer) {
                Ok(0) => return false, // Peer hat zugemacht
                Ok(_) => {
                    let _ = peek.set_read_timeout(Some(self.config.lese_timeout));
                    return true;
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    // 5 ms Schonfrist: haelt die Schleife auch dann klein,
                    // wenn der Socket NONBLOCKING waere (Timeouts gelten
                    // dann naemlich nicht, und `peek` waere sofort leer).
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => return false,
            }
        }
    }

    /// Laeuft, bis `stoppen()` aufgerufen wird (blockiert).
    ///
    /// Non-blocking `accept` mit kurzer Pause: ohne Pause wuerde die
    /// Schleife den Kern blockieren, mit blockierendem `accept` kaeme
    /// `stoppen()` nie an.
    pub fn laufen<A: ToSocketAddrs>(&self, adresse: A) -> std::io::Result<()> {
        let listener = TcpListener::bind(adresse)?;
        self.laufen_auf(&listener)
    }

    /// Laeuft auf einem vorhandenen Listener (fuer Tests mit Port 0).
    ///
    /// Beendet den Pool sauber: nach dem RUECKKEHR ist kein Worker-Thread
    /// mehr da und `aktive()` ist 0 — sonst waere der Test, der das
    /// prueft, selbst die Ursache des Threads.
    pub fn laufen_auf(&self, listener: &TcpListener) -> std::io::Result<()> {
        listener.set_nonblocking(true)?;
        let pool = self.starte_pool()?;
        loop {
            if self.soll_stoppen() {
                break;
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    // Grenze: SOFORT 503, ohne Thread und ohne Warteschlange.
                    // (Pruefung + Erhoehung im selben Thread, also ohne Luecke.)
                    if self.aktive() >= self.config.max_verbindungen {
                        ablehnen(stream);
                        continue;
                    }
                    self.aktiv.fetch_add(1, Ordering::SeqCst);
                    // added SERVER V5: Lagen beim `accept` schon Bytes im
                    // Sockel? Ein nonblocking `peek` beantwortet das,
                    // ohne etwas zu verbrauchen. Das ist die Information,
                    // die der Worker sonst erst nach bis zu 250 ms
                    // (`halboffen_schonfrist`) haette.
                    let hat_daten = socket_hat_daten(&stream);
                    // Schlange voll oder der Pool stoppt: `503` mit
                    // `Retry-After`, im selben Faden, ohne Worker. Beim
                    // Stoppen wird der Sockel einfach fallengelassen —
                    // es gibt keinen Client mehr, dem man antworten
                    // koennte, und `503` waere nur eine halbe Antwort
                    // an niemanden.
                    if let Err(untergekommen) = self.warteschlange.stecke_rein(
                        stream,
                        hat_daten,
                        self.config.max_schlangen_laenge,
                        self.config.max_halboffen,
                    ) {
                        self.aktiv.fetch_sub(1, Ordering::SeqCst);
                        if !self.soll_stoppen() {
                            ablehnen(untergekommen);
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // 1 ms: `accept` ist nonblocking, damit `stoppen()`
                    // ankommt. Die Pause haelt die Schleife klein UND
                    // haelt die Antwortzeit niedrig — 5 ms wuerde bei
                    // 1.000 Verbindungen eine ganze Sekunde Anschluss-
                    // verzug bauen, den niemand bezahlt haben will.
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }
        self.warteschlange.stoppe();
        for h in pool {
            let _ = h.join();
        }
        Ok(())
    }

    /// Worker-Pool starten (feste Anzahl, [`MAX_WORKER`] gedeckelt).
    fn starte_pool(&self) -> std::io::Result<Vec<std::thread::JoinHandle<()>>> {
        let mut handgriffe = Vec::with_capacity(self.config.worker);
        for i in 0..self.config.worker {
            let ich = self.clone();
            let builder = std::thread::Builder::new()
                .name(format!("narsil-worker-{i}"))
                .spawn(move || {
                    ich.worker();
                });
            match builder {
                Ok(h) => handgriffe.push(h),
                Err(e) => {
                    // Teils gestartet: trotzdem sauber stoppen, sonst
                    // laeuft ein Pool ohne Besitzer weiter.
                    self.warteschlange.stoppe();
                    for h in handgriffe {
                        let _ = h.join();
                    }
                    return Err(e);
                }
            }
        }
        Ok(handgriffe)
    }

    /// EIN Worker: zieht Verbindungen aus der Schlange, bis sie leer
    /// und gestoppt ist. Die Anzahl der Threads bleibt konstant.
    fn worker(&self) {
        loop {
            let eintrag = {
                let mut w = self.warteschlange.lock();
                loop {
                    // DER EINTRAG IST `(Sockel, hat_daten)` — und wird
                    // auch so durchgereicht.
                    //
                    // DER FEHLER, DEN DER TEST GEFUNDEN HAT (SERVER V5):
                    // hier stand `!ohne_daten`. Das Flag in der Schlange
                    // ist aber `hat_daten`, nicht `ohne_daten` — also
                    // kam jede stille Verbindung beim Worker als
                    // „hat Daten" an, ohne Wartephase und ohne
                    // Halboffen-Grenze, direkt in `parse_request_aus`.
                    // Dort blockierte sie im `read` fuer `lese_timeout`
                    // (10 s) — vier Worker, ein Server, der auf eine
                    // frische Verbindung nicht antwortet.
                    if let Some((s, hat_daten)) = w.q.pop_front() {
                        if !hat_daten {
                            w.ohne_daten = w.ohne_daten.saturating_sub(1);
                        }
                        break Some((s, hat_daten));
                    }
                    if w.fertig {
                        break None;
                    }
                    w = self
                        .warteschlange
                        .aufwecken
                        .wait(w)
                        .unwrap_or_else(|p| p.into_inner());
                }
            };
            match eintrag {
                Some((s, hat_daten)) => {
                    self.bediene_socket(s, hat_daten);
                    self.aktiv.fetch_sub(1, Ordering::SeqCst);
                }
                None => return,
            }
        }
    }

    /// Bedient GENAU eine Verbindung auf dem Listener (fuer Tests).
    ///
    /// Blockiert bis eine kommt (mit Timeout). Gibt `false` bei Timeout
    /// zurueck, `true` wenn eine bedient wurde.
    pub fn serve_once(&self, listener: &TcpListener, timeout: Duration) -> bool {
        listener.set_nonblocking(true).ok();
        let start = Instant::now();
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    // added SERVER V5: derselbe nonblocking `peek` wie
                    // im Accept-Faden. Hier ist es nur der Vollstaendigkeit
                    // halber — `serve_once` ist der Testweg und hat nur
                    // eine Verbindung — aber eine andere Antwort als im
                    // Pool waere eine zweite Wahrheit.
                    let hat_daten = socket_hat_daten(&stream);
                    self.bediene_socket(stream, hat_daten);
                    return true;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if start.elapsed() > timeout {
                        return false;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => {
                    if start.elapsed() > timeout {
                        return false;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }
}

/// Die Antwort, die eine Verbindung ueber einer Grenze bekommt —
/// `max_verbindungen` im Accept-Thread, `max_halboffen` im Worker.
///
/// `503` plus `Retry-After`, weil es ein Zustand ist, der von selbst
/// vorbeigeht: Der Client darf in einer Sekunde wiederkommen, und dann
/// ist die Verbindung eine normale.
const UEBER_GRENZE_ANTWORT: &[u8] = b"HTTP/1.1 503 Service Unavailable\r\n\
  Server: Narsil/0.1\r\n\
  Connection: close\r\n\
  Content-Length: 0\r\n\
  Retry-After: 1\r\n\
  \r\n";

/// Liegen im Sockel schon Bytes? (`peek`, nonblocking, nichts gelesen.)
///
/// added 01.10.2026, SERVER V5. Ein Aufruf im Accept-Faden, einmal je
/// Verbindung.
///
/// DER WEG, und warum `nonblocking` statt eines Timeouts: der
/// Accept-Faden darf unter keinen Umstaenden blockieren — er ist der
/// einzige Faden, der neue Verbindungen annimmt. Ein Timeout von
/// 250 ms wuerde bei 10.000 gleichzeitigen Verbindungen 2.500 s
/// bedeuten; `peek` auf einem nonblocking Sockel antwortet sofort mit
/// `WouldBlock`, also „noch nichts da".
///
/// DER FALL, UM DER ES GEHT: ein Browser (oder `curl`) schreibt seine
/// Anfrage praktisch im selben Moment wie der Verbindungsaufbau. Die
/// Bytes liegen dann **schon** im Puffer, wenn `accept` zurueckkommt.
/// Ein Angreifer, der 10.000 Sockets aufmacht und schweigt, laesst
/// den Puffer leer. Der Unterschied ist ein `peek` — und er spart dem
/// Worker die Wartezeit auf die Feststellung.
fn socket_hat_daten(peek: &TcpStream) -> bool {
    // `try_clone` kostet einen Deskriptor. Alternative waere
    // `set_nonblocking` am Original — das wuerde aber die Zeitouts der
    // spaeteren Schritte zerstoeren, und die gehoeren zu diesem Sockel.
    let Ok(klon) = peek.try_clone() else {
        return false;
    };
    if klon.set_nonblocking(true).is_err() {
        return false;
    }
    let mut ein_byte = [0u8; 1];
    // M3: genau ein Byte. Alles, was mehr laest, ist eine Allokation
    // aus der Eingabe — und wir wollen hier nur ein Ja oder Nein.
    let da = match klon.peek(&mut ein_byte) {
        Ok(n) => n > 0,
        Err(_) => false,
    };
    // DER FEHLER, DEN DIESER AUFRUFMACHER HIER ERZEUGT HAT (SERVER V5):
    // `try_clone` dupliziert nur den Deskriptor — auf Linux teilen
    // Original und Klon dieselbe **open file description**, und
    // `O_NONBLOCK` ist ein Merkmal genau davon. Das `set_nonblocking`
    // oben hat also nicht den Klon kaputt gemacht, sondern den
    // echten Sockel: jeder folgende `read` lieferte sofort
    // `WouldBlock`, und der WebSocket-Echo in
    // `tests/codex_grenzen.rs` bekam `UnexpectedEof`.
    //
    // Der Test hat es gefunden; die Ursache ist eine Freigabe, kein
    // Ratsen. Also wird der Zustand hier ausdruecklich zurueckgesetzt —
    // und der naechste Block ist nicht optional.
    let _ = klon.set_nonblocking(false);
    da
}

/// Steht auf dem Sockel die HTTP/2-Vorfahrt? (`peek`, kein Byte gelesen.)
///
/// added 01.10.2026, SERVER V5 (h2c). Die drei Faelle, und jeder hat eine
/// eigene Antwort — dazwischen liegt die falsche Vereinfachung "einmal
/// `peek` und dann vergleichen":
///
/// * **Weicht schon ein Byte ab** -> definitiv kein h2c. `GET ` und
///   `POST ` unterscheiden sich in der ersten Sekunde von `PRI `.
///   Ergebnis `false`, und der HTTP/1.1-Weg laeuft unveraendert.
/// * **Passt bisher, ist aber unvollstaendig** (die Vorfahrt kam in zwei
///   Segmenten) -> weiter warten, bis entweder alle 24 Byte da sind oder
///   die Frist ablaeuft. Genau das ist der Grund fuer die Schleife.
/// * **Frist abgelaufen / Timeout** -> `false`. Der Client hat noch
///   nichts oder zu langsam geschickt; er wird nicht bestraft, er
///   wartet im normalen Weg weiter.
///
/// M3: die Schleife ist auf [`crate::h2::CLIENT_VORFAHRT`] + 1 Runden
/// gedeckelt UND auf die Frist — zwei Grenzen fuer eine Schleife, die
/// von einem Sockel abhaengt, sind keine Zufallsgrenze, sondern die
/// Antwort auf die Frage, was sie sonst tun koennte.
fn ist_h2c(peek: &TcpStream, lese_timeout: Duration) -> bool {
    let vorfahrt = crate::h2::CLIENT_VORFAHRT;
    let mut puffer = [0u8; 24];
    debug_assert_eq!(puffer.len(), vorfahrt.len());
    let frist = Instant::now() + H2C_ERKENNUNGSFRIST;
    for _ in 0..=vorfahrt.len() {
        let uebrig = frist.saturating_duration_since(Instant::now());
        if uebrig.is_zero() {
            return false;
        }
        let _ = peek.set_read_timeout(Some(uebrig));
        let n = match peek.peek(&mut puffer) {
            Ok(n) => n,
            Err(_) => break,
        };
        if n == 0 {
            return false; // Peer hat zugemacht
        }
        let verglichen = n.min(vorfahrt.len());
        if vorfahrt[..verglichen] != puffer[..verglichen] {
            return false; // definitiv HTTP/1.1
        }
        if n >= vorfahrt.len() {
            return true;
        }
    }
    let _ = peek.set_read_timeout(Some(lese_timeout));
    false
}

/// `503` auf eine Verbindung, die ueber der Grenze ist — ohne Thread.
///
/// added 01.10.2026, SERVER V5: der Sockel geht hier auf **NONBLOCKING**,
/// und der Grund ist eine Messung. Die erste Fassung setzte nur einen
/// 250-ms-Schreib-Timeout — und dieser Pfad laeuft im ACCEPT-FADEN.
/// Bei 10.000 gleichzeitig eingehenden Verbindungen schreibt dieser Faden
/// 9.744 Mal ein `503`; sobald ein Gegner einen davon nicht liest
/// (sein Puffer ist voll, also liest er nie), steht der Faden 250 ms pro
/// Versuch. 40 solcher Verbindungen sind 10 s, in denen **keine** neue
/// Verbindung mehr angenommen wird — der Server nimmt nichts mehr an,
/// obwohl alle 12 Worker frei sind.
///
/// Nonblocking macht denselben Fall harmlos: der Schreibvorgang
/// gelingt (128 Byte passen in jeden Puffer) oder schlaegt sofort mit
/// `WouldBlock` fehl. In beiden Faellen ist der Faden im Mikrosekunden-
/// Bereich weiter. Der Client, der sein `503` nicht bekommt, sieht
/// `Connection reset` — bei 10.000 Angreifer-Verbindungen ist der
/// Unterschied nicht messbar, bei einem echten Client schon; und fuer
/// den echten Client ist der Puffer nie voll.
fn ablehnen(mut stream: TcpStream) {
    schreibe_ablehnung(&mut stream);
}

/// Dasselbe fuer eine halboffene Verbindung, diesmal am bereits
/// geoeffneten Schreib-Socket des Workers (added SERVER V5).
///
/// Der Unterschied zu `ablehnen` ist nur, wer den Socket haelt — die
/// Antwort ist bitweise dieselbe, damit ein Client an beiden Grenzen
/// genau eine Sache zu verstehen hat. Auch hier nonblocking: der Faden
/// gehoert einem Worker, und der Worker ist die knappste Ressource
/// der ganzen Crate.
fn halboffen_ablehnen(stream: &mut TcpStream) {
    schreibe_ablehnung(stream);
}

/// Das `503` auf einen nonblocking Sockel — **ohne** Endlosschleife.
///
/// DER FEHLER, DEN DER TEST GEFUNDEN HAT (SERVER V5): die Fassung
/// davor war `set_nonblocking(true)` + `write_all`. Und `write_all`
/// wiederholt bei `WouldBlock` **unbegrenzt** — auf einem
/// nonblocking Sockel ist `WouldBlock` aber der Normalfall, sobald der
/// Gegner nicht liest. Ergebnis: der Accept-Faden (bzw. der Worker)
/// drehte sich in einer engen Schleife, bis die 3-s-Frist des
/// Kanonenclients ablief, und keine weitere Verbindung kam an. Das ist
/// genau die Verweigerung, die V5 verhindern wollte.
///
/// DER WEG: `write` (ein Aufruf, kein Wiederholen), und wenn `n` Byte
/// fehlen, wird der Rest **einmal** versucht — dann aufgegeben. Ein
/// unvollstaendiges `503` an einen Client, der seinen Puffer nicht
/// liest, ist harmlos; ein blockierter Faden nicht.
fn schreibe_ablehnung(stream: &mut TcpStream) {
    let _ = stream.set_nonblocking(true);
    let mut geschrieben = 0usize;
    // M3: hoechstens drei Versuche. Die Schleife laeuft ueber `write`
    // (nicht ueber die Laenge der Antwort), also die Grenze ist eine
    // Zahl und nicht die Eingabe.
    for _ in 0..3 {
        match stream.write(&UEBER_GRENZE_ANTWORT[geschrieben..]) {
            Ok(0) => break, // Gegner liest nicht mehr
            Ok(n) => {
                geschrieben += n;
                if geschrieben >= UEBER_GRENZE_ANTWORT.len() {
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(_) => break,
        }
    }
    let _ = stream.flush();
}

/// Router als Handler (die Kette braucht eine Funktion, der Server
/// braucht keinen zweiten Router).
fn als_handler(router: Arc<Router>) -> Handler {
    Arc::new(move |anfrage| router.route(anfrage))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Response;
    use std::io::Cursor;

    fn test_server() -> Server {
        let mut r = Router::neu();
        r.get("/hello/:name", |req| {
            Response::text(200, format!("Hallo {}", req.param("name").unwrap_or("?")))
        });
        r.post("/echo", |req| {
            Response::text(200, req.body_text().unwrap_or("").to_string())
        });
        Server::neu(r)
    }

    #[test]
    fn get_wird_geroutet() {
        let s = test_server();
        let raw = b"GET /hello/Welt HTTP/1.1\r\nHost: x\r\n\r\n";
        let mut leser = BufReader::new(Cursor::new(raw.to_vec()));
        let mut raus = Vec::new();
        s.bediene(&mut leser, &mut raus, "test").unwrap();
        let text = String::from_utf8(raus).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK"));
        assert!(text.ends_with("Hallo Welt"));
    }

    #[test]
    fn kaputte_anfrage_gibt_400() {
        let s = test_server();
        let raw = b"KAPUTT\r\n\r\n";
        let mut leser = BufReader::new(Cursor::new(raw.to_vec()));
        let mut raus = Vec::new();
        assert!(s.bediene(&mut leser, &mut raus, "test").is_err());
        assert!(String::from_utf8(raus).unwrap().contains("400"));
    }

    #[test]
    fn post_echo() {
        let s = test_server();
        let raw = b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\n\r\nabc";
        let mut leser = BufReader::new(Cursor::new(raw.to_vec()));
        let mut raus = Vec::new();
        s.bediene(&mut leser, &mut raus, "test").unwrap();
        assert!(String::from_utf8(raus).unwrap().ends_with("abc"));
    }

    #[test]
    fn websocket_handshake_gibt_101() {
        let s = test_server();
        let raw = b"GET /chat HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n";
        let mut leser = BufReader::new(Cursor::new(raw.to_vec()));
        let mut raus = Vec::new();
        s.bediene(&mut leser, &mut raus, "test").unwrap();
        assert!(String::from_utf8(raus).unwrap().starts_with("HTTP/1.1 101"));
    }

    #[test]
    fn echter_socket_runde() {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let s = test_server();
        let handle = std::thread::spawn(move || {
            s.serve_once(&listener, Duration::from_secs(5));
        });
        let mut client = TcpStream::connect(addr).unwrap();
        client
            .write_all(b"GET /hello/Socket HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
            .unwrap();
        let mut antwort = Vec::new();
        client.read_to_end(&mut antwort).unwrap();
        let text = String::from_utf8(antwort).unwrap();
        assert!(text.contains("Hallo Socket"));
        handle.join().unwrap();
    }

    #[test]
    fn keep_alive_zwei_anfragen_eine_verbindung() {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let cfg = ServerConfig {
            leerlauf_timeout: Duration::from_millis(700),
            ..ServerConfig::default()
        };
        let s = Server::mit_config(test_router_keepalive(), cfg);
        let handle = std::thread::spawn(move || {
            s.serve_once(&listener, Duration::from_secs(5));
        });
        let mut client = TcpStream::connect(addr).unwrap();
        client
            .write_all(b"GET /eins HTTP/1.1\r\nHost: x\r\n\r\n")
            .unwrap();
        // Erste Antwort lesen, bis der Kopf vollstaendig da ist und der
        // Server auf die zweite Anfrage wartet.
        let mut puffer = [0u8; 512];
        let mut text = String::new();
        client
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        for _ in 0..4 {
            match client.read(&mut puffer) {
                Ok(n) => text.push_str(&String::from_utf8_lossy(&puffer[..n])),
                Err(_) => break,
            }
            if text.contains("eins") {
                break;
            }
        }
        assert!(text.contains("Connection: keep-alive"), "{text}");
        assert!(text.contains("HTTP/1.1 200"), "{text}");
        // Zweite Anfrage auf derselben Verbindung.
        client
            .write_all(b"GET /zwei HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
            .unwrap();
        let mut rest = String::new();
        client.read_to_string(&mut rest).unwrap();
        assert!(rest.contains("HTTP/1.1 200"), "{rest}");
        assert!(rest.contains("zwei"), "{rest}");
        assert!(rest.contains("Connection: close"), "{rest}");
        handle.join().unwrap();
    }

    fn test_router_keepalive() -> Router {
        let mut r = Router::neu();
        r.get("/eins", |_| Response::text(200, "eins"));
        r.get("/zwei", |_| Response::text(200, "zwei"));
        r
    }

    #[test]
    fn worker_ist_fest_und_deckelt() {
        assert_eq!(
            ServerConfig::default().worker,
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .min(MAX_WORKER)
        );
        let cfg = ServerConfig {
            worker: 10_000,
            ..ServerConfig::default()
        };
        let s = Server::mit_config(Router::neu(), cfg);
        assert_eq!(s.config.worker, MAX_WORKER);
    }

    #[test]
    fn security_header_sind_auf_jeder_antwort() {
        let s = test_server();
        let raw = b"GET /gibt-es-nicht HTTP/1.1\r\nHost: x\r\n\r\n";
        let mut leser = BufReader::new(Cursor::new(raw.to_vec()));
        let mut raus = Vec::new();
        let _ = s.bediene(&mut leser, &mut raus, "test");
        let text = String::from_utf8(raus).unwrap();
        assert!(text.contains("X-Content-Type-Options: nosniff"));
        assert!(text.contains("X-Frame-Options: DENY"));
        assert!(text.contains("Referrer-Policy:"));
        // Zaehler laeuft auch bei 404 mit.
        assert_eq!(s.antworten(), 1);
        assert_eq!(s.fehler_zaehler(), 1);
    }
}
