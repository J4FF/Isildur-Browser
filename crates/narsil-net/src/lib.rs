//! narsil-net — HTTP/1.1 Client in reinem Rust, ohne Fremdcrates.
//!
//! Diese Crate ist die NETZ-Schicht des Browsers. Sie kennt kein HTML,
//! kein DOM, keinen Parser — sie holt Bytes aus dem Netz und gibt sie
//! als [`Response`] zurueck.
//!
//! ARCHITEKTURENTSCHEIDUNG (ADR-046, TLS-Mauer):
//! HTTPS geht seit dem 30.09.2026 ueber `narsil_tls::strom::TlsStrom`.
//!
//! DIESER ABSATZ WAR BIS EBEN RICHTIG UND IST JETZT FALSCH. Er lautete:
//! "TLS (HTTPS) fehlt BEWUSST. HTTP reicht fuer lokale Entwicklung,
//! eigene Narsil-Server, Seiten, die HTTP anbieten. Die
//! TLS-Implementierung ist ein eigenes Projekt und kommt spaeter."
//! Das war eine korrekte Beschreibung des Zustands — und wurde nicht
//! angefasst, als `TlsStrom` verdrahtet wurde, sodass der Kopf der
//! Crate eine Woche lang das Gegenteil dessen behauptete, was sie tat.
//!
//! Er steht hier nicht aus Schonung, sondern weil das der Fehler ist,
//! den ich am wahrscheinlichsten wiederhole: eine Einschraenkung, die
//! einmal geschrieben stand, liest sich beim zweiten Mal wie eine
//! Behauptung.
//!
//! ZUSTAENDIGKEIT: kritischer Pfad Punkt 8 — ohne Netz kein Browser.
//!
//! REGELN:
//! * Kein `unsafe`.
//! * Keine Fremd-Crates. Nur `std::net::TcpStream`.
//! * Jeder Fehler hat einen Typ, kein Panic, kein unwrap.
//! * Jedes Byte vom Netz ist FEINDLICH: Grenzen, Zeitlimits, benannte
//!   Fehler. Kein Pfad fuehrt zu `panic!`, keiner zu unbegrenztem Warten
//!   und keiner zu unbegrenztem Speicher.

pub mod cache;
/// added 06.10.2026 17:1x, Rolle NETZTEMPO. **DER DNS-CACHE**, und die
/// Anbindung ist **eine** Zeile: `verbinde_neu` ruft `dns::loese` statt
/// `to_socket_addrs`.
///
/// WARUM EIGENE DATEI und nicht ein Block in `lib.rs`: `lib.rs` hat
/// 10.359 Zeilen und ist der Ort, an dem sechs Rollen arbeiten. Ein
/// prozess-globaler Zustand mit eigener Grenze gehoert in eine Datei, die
/// **man** lesen kann, ohne die ganze Crate zu lesen — und die kein
/// Blockfraesser mit einem anderen Abschnitt mitreissen kann (GEMESSEN:
/// genau das ist bei `pool.rs` zweimal passiert, siehe dort R69).
pub mod dns;
pub mod h2;
// added 04.10.2026, Rolle TLS (Freigabe der Leitung: „baue ihn in narsil-net“).
// **EINE Zeile**, und sie ist noetig, damit die Datei im Netz-Crate *ueberhaupt
// existiert* — Rust baut ein Modul erst, wenn es benannt wird. Die Datei
// `keksglas.rs` gehoert mir laut `team/EIGENTUM.txt` (\„Cookie-Dateien\“);
// **diese** Zeile steht in einer fremden Datei und ist deshalb hier im
// Bericht offengelegt.
pub mod h2client;
pub mod h2strom;
// KONVERT_23: `hole_stream` — ein Download ueber **denselben** Weg wie eine
// Seite (Cookies, Proxy, TLS, HSTS, HTTP/2-Pool). GEMESSEN in KONVERT_22, dass
// `narsil-download` bis eben einen **eigenen** `TcpStream` machte.
pub mod keksglas;
pub mod strom;
// added 03.10.2026, Rolle NETZTEMPO. EINE h2-Verbindung fuer viele Anfragen —
// der Grund fuer `tls_n = 6` bei 20 Stylesheets **eines** Hosts.
pub mod h2geteilt;
/// Release 08.10.2026: Netzregel je Tab, Kopfzeilen-Regeln und GPC fuer den
/// Tab „Features" — siehe den Kopf der Datei.
pub mod haken;
pub mod hpack;
/// added 03.10.2026, Rolle NETZTEMPO. **PARALLELES HOLEN MIT PRIORITAETEN.**
/// Warum eine eigene Datei: siehe den Kopf von `parallel.rs` — der
/// Auftrag (paralleles Holen, Prioritaeten) fehlte in der Crate ganz,
/// und ein Block in `lib.rs` waere an einer Stelle gelandet, an der
/// sechs Rollen arbeiten.
pub mod parallel;
pub mod pool;
/// added 04.10.2026 08:1x, R72. **S6/S7/S9 als reine Prüf-Funktionen.**
/// added 08:1x: **WARUM `pub`:** die Crate, die den **Aufruf** besitzt, ist
/// fremd (GEMESSEN: `narsil-server` für CORS, `narsil-devtools` für CSP,
/// `narsil-download` für den Download). Damit die Prüfung dort **eingehängt**
/// werden kann, muss sie **exportiert** sein — und weil sie **reine
/// Funktionen** sind, entsteht dabei **keine** Abhängigkeit und **kein**
/// geteilter Zustand.
pub mod sicherheit;
pub mod sitzung;

use narsil_cache::Fund;

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

/// Abstraktion ueber eine Netzwerkverbindung.
///
/// Ermöglicht den Austausch von `TcpStream` gegen TLS-Streams (Welle 5)
/// ohne Aenderung der HTTP-Logik.
///
/// DER GRUND FUER DAS `+ Send` (added 30.09.2026 fuer den
/// Verbindungspool, Auftrag N4): der Pool ist ein prozess-globaler
/// `Mutex`, und ein `Mutex<T>` ist nur dann `Sync`, wenn `T: Send`.
/// Ohne `Send` muesste der Pool entweder pro Thread liegen (dann
/// wuerde eine Webseite, die Seiten und Skripte ueber verschiedene
/// Threads laedt, je Thread einen eigenen Handshake machen — genau
/// das, was der Pool verhindern soll) oder `unsafe` benutzen.
///
/// DER BEWEIS, DASS ES REICHT: `TcpStream` und die TLS-Streams aus
/// `narsil-tls` sind alle `Send` (sie kapseln einen Sockel, keinen
/// Thread-affinen Zustand). Die Kompilierung des Pools mit
/// `static POOL: OnceLock<Mutex<Pool>>` ist der Beweis — er schlaegt
/// fehl, wenn eine der drei `impl Leitung`-Typen kein `Send` waere.
pub trait Leitung: Read + Write + Send {
    /// Hat der Server **`h2`** gewaehlt?
    ///
    /// added 01.10.2026 12:0x, R4z. DIE FRAGE AN DIE LEITUNG, und sie ist
    /// noetig, weil `sende()` eine `Box<dyn Leitung>` bekommt und ohne diese
    /// Methode nicht unterscheiden kann, was darunter liegt (genau der
    /// Grund fuer die Box, siehe Kommentar dort).
    ///
    /// DER FALL DER DAS AUSLOEST, und er ist R4z: der Server waehlt h2,
    /// und wir muessen **vor** dem Senden wissen, welches Protokoll wir
    /// reden. Sonst schreiben wir HTTP/1.1-Text auf eine h2-Verbindung.
    fn ist_h2_gewahlt(&self) -> bool {
        false
    }

    /// Deaktiviert den Nagle-Algorithmus (TCP_NODELAY).
    fn set_nodelay(&mut self, value: bool) -> io::Result<()>;

    /// Setzt das Lese-Timeout.
    fn set_read_timeout(&mut self, dur: Option<std::time::Duration>) -> io::Result<()>;

    /// Setzt das Schreib-Timeout.
    fn set_write_timeout(&mut self, dur: Option<std::time::Duration>) -> io::Result<()>;

    /// Schließt die Verbindung.
    fn shutdown(&mut self, how: std::net::Shutdown) -> io::Result<()>;

    /// Gibt die lokale Adresse zurueck (fuer Logging/Debugging).
    fn local_addr(&self) -> io::Result<std::net::SocketAddr>;

    /// Gibt die entfernte Adresse zurueck (fuer Logging/Debugging).
    fn peer_addr(&self) -> io::Result<std::net::SocketAddr>;

    /// Ist die Gegenseite noch da? added 30.09.2026, Welle 7 / N4.
    ///
    /// DER SINN: eine Leitung aus dem Pool kann seit der letzten
    /// Antwort vom Server geschlossen worden sein. Ohne diese Frage
    /// schreibt der Browser die naechste Anfrage in einen Sockel, von
    /// dem er es erst beim Lesen erfährt — und bei einer `POST` bleibt
    /// dann nur die Fehlermeldung, obwohl dieselbe Anfrage auf einer
    /// frischen Verbindung gegangen waere.
    ///
    /// DER STANDARD IST `true`, und das ist Absicht: eine Leitung, die
    /// ihre Lage nicht kennen kann (etwa ein TLS-Stream, bei dem ein
    /// Blick auf den Sockel die TLS-Schicht umgehen wuerde), meldet
    /// „weiss ich nicht" als „wahrscheinlich noch da". Falsch-positive
    /// Antworten kosten dann einen Aufruf; falsch-negative wuerden eine
    /// lebende Verbindung wegwerfen.
    ///
    /// added 04.10.2026, Rolle NETZTEMPO. **`&mut self`, und der Grund ist
    /// gemessen.** Die **TLS**-Leitung kann ihre Lage nur durch **Lesen**
    /// feststellen, und Lesen braucht `&mut`. Mit `&self` blieb fuer sie nur
    /// der Standard `true` — der Todes-Check war damit fuer **jede** echte
    /// HTTPS-Verbindung ein **Nop**. Das ist die Luecke, die der Auftrag
    /// meinte, und sie ist bei `grep` unsichtbar: der Aufruf **existierte**
    /// (`lib.rs`, vor dem Schreiben der Anfrage), er tat nur nichts.
    fn lebt_noch(&mut self) -> bool {
        true
    }

    /// Verbindet mit Timeout.
    fn connect_timeout(addr: &std::net::SocketAddr, timeout: Duration) -> io::Result<Self>
    where
        Self: Sized;
}

/// `Box<dyn Leitung>` erbt die Leitungsmethoden weiter.
///
/// OHNE DIESEN Block muesste jeder Aufrufer `(*stream).set_nodelay(true)`
/// schreiben statt `stream.set_nodelay(true)` — und `Box<dyn Leitung>`
/// ist an `Read`/`Write` zwar gebunden, die vier `Leitung`-Methoden aber
/// nicht. Der Block ist also nicht Bequemlichkeit, sondern die Bedingung
/// dafuer, dass der restliche Code unveraendert bleiben konnte.
///
/// GEMESSEN, warum kein `impl Leitung for Box<dyn Leitung>` mit
/// `connect_timeout`: das brauchte `Self: Sized`, und `Box<dyn Leitung>`
/// hat keine statische Groesse. `verbinde` macht die Box, also
/// entfaellt der Fall.
impl Leitung for Box<dyn Leitung> {
    /// added 01.10.2026 12:0x, R4z: **ohne** das gibt die Box immer
    /// `false` zurueck, und `sende()` wuesste nie, dass h2 gewaehlt wurde —
    /// GEMESSEN genau so (`Record zu lang`). Die Box ist ein Trait-Objekt,
    /// also muss die Frage **durchgereicht** werden.
    fn ist_h2_gewahlt(&self) -> bool {
        (**self).ist_h2_gewahlt()
    }

    fn set_nodelay(&mut self, value: bool) -> io::Result<()> {
        (**self).set_nodelay(value)
    }
    fn set_read_timeout(&mut self, dur: Option<std::time::Duration>) -> io::Result<()> {
        (**self).set_read_timeout(dur)
    }
    fn set_write_timeout(&mut self, dur: Option<std::time::Duration>) -> io::Result<()> {
        (**self).set_write_timeout(dur)
    }
    fn shutdown(&mut self, how: Shutdown) -> io::Result<()> {
        (**self).shutdown(how)
    }
    fn local_addr(&self) -> io::Result<SocketAddr> {
        (**self).local_addr()
    }
    fn peer_addr(&self) -> io::Result<SocketAddr> {
        (**self).peer_addr()
    }
    /// Unmoeglich, und der Fehler sagt es.
    ///
    /// GEMESSEN, warum das ein Fehler und kein `unreachable!` ist: es
    /// ist die eine Stelle, an der jemand spaeter eine TLS-Box bauen will,
    /// und dann soll er eine Meldung bekommen, keine Panik in einem
    /// Netzwerkpfad. Der Weg ist `verbinde` (siehe dort).
    fn connect_timeout(_addr: &SocketAddr, _timeout: Duration) -> io::Result<Self>
    where
        Self: Sized,
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "verbinde() baut die Box, nicht connect_timeout()",
        ))
    }
}

impl Leitung for TcpStream {
    fn set_nodelay(&mut self, value: bool) -> io::Result<()> {
        TcpStream::set_nodelay(self, value)
    }

    fn set_read_timeout(&mut self, dur: Option<std::time::Duration>) -> io::Result<()> {
        TcpStream::set_read_timeout(self, dur)
    }

    fn set_write_timeout(&mut self, dur: Option<std::time::Duration>) -> io::Result<()> {
        TcpStream::set_write_timeout(self, dur)
    }

    fn shutdown(&mut self, how: std::net::Shutdown) -> io::Result<()> {
        std::net::TcpStream::shutdown(self, how)
    }

    /// added 30.09.2026, Welle 7 / N4. DER WEG: ein Lesevorgang mit
    /// **Timeout null** und `peek` statt `read`.
    ///
    /// `peek` ist entscheidend: `read` wuerde ein Byte **wegnehmen**,
    /// und das waere der Anfang der naechsten Antwort. `peek` schaut
    /// nur nach, ohne zu nehmen.
    ///
    /// Warum Timeout null: es soll hier **nicht** gewartet werden. Der
    /// Aufrufer fragt, ob die Gegenseite noch lebt — die Antwort steht
    /// im Kernel, und wenn sie noch nicht da ist, heisst das
    /// „nein" (nur `WouldBlock` bzw. `TimedOut`), nicht „warte noch
    /// einen Moment".
    ///
    /// DER RUECKGABEWERT: `Ok(0)` ist EOF, also hat die Gegenseite die
    /// Verbindung geschlossen — das ist der Fall, den wir suchen.
    /// `Ok(n)` mit `n > 0` heisst, dass Bytes liegen, die nicht
    /// gehoeren (nach unseren eigenen Regeln kann das nicht sein, denn
    /// gepoolt wird nur, was leer gelesen ist) — auch das ist „nicht
    /// gesund", aber die Entscheidung darueber trifft der Aufrufer.
    /// Jeder andere Fehler bedeutet „unbekannt" und wird als „lebt"
    /// gemeldet.
    ///
    /// M3: der Timeout wird danach **nicht** zurueckgesetzt. Das ist
    /// Absicht: `Leser::fuelle` setzt den Lese-Timeout vor jedem
    /// Lesevorgang neu (`self.limits.lesen.min(rest)`), und
    /// `schreibe` den Schreib-Timeout. Wer sich auf einen Restwert
    /// verließe, haette nach diesem Aufruf einen Sockel mit Timeout 0.
    fn lebt_noch(&mut self) -> bool {
        // DER TIMEOUT NULL GEHOERT DAZU, und ohne ihn waere die Frage
        // eine Falle: ein Sockel ohne Lese-Timeout blockiert **ewig**,
        // also wuerde der Aufruf „lebt die Gegenseite?" bei einer
        // haengenden Verbindung den Browser anhalten statt zu antworten.
        // `set_read_timeout` nimmt bei `TcpStream` nur `&self`, deshalb
        // passt das in eine `&self`-Methode.
        if self.set_read_timeout(Some(Duration::ZERO)).is_err() {
            // Ohne Timeout zu arbeiten waere schlimmer als zu raten.
            return true;
        }
        let mut ein = [0u8; 1];
        match self.peek(&mut ein) {
            Ok(0) => false,
            Ok(_) => true,
            Err(e) => !matches!(
                e.kind(),
                io::ErrorKind::UnexpectedEof | io::ErrorKind::NotConnected
            ),
        }
    }

    fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        TcpStream::local_addr(self)
    }

    fn peer_addr(&self) -> io::Result<std::net::SocketAddr> {
        TcpStream::peer_addr(self)
    }

    fn connect_timeout(addr: &std::net::SocketAddr, timeout: Duration) -> io::Result<Self>
    where
        Self: Sized,
    {
        TcpStream::connect_timeout(addr, timeout)
    }
}

/// HTTP-Methode.
///
/// Release 10.10.2026: auch `PUT`, `DELETE`, `PATCH` und `OPTIONS` —
/// `fetch` kann sie verlangen (REST-APIs), und `OPTIONS` ist die
/// CORS-Vorabanfrage (Fetch §4.8 „CORS-preflight fetch").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Head,
    Post,
    Put,
    Delete,
    Patch,
    Options,
}

/// Zustand einer Verbindung, geprueft VOR dem Schreiben in den Puffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeepAlive {
    /// Verbindung nach der Antwort schliessen (`Connection: close`).
    Schliesst,
    /// Verbindung darf weiter benutzt werden (`keep-alive`).
    Bleibt,
}

impl Method {
    /// Darf eine Anfrage mit dieser Methode nach einem Verbindungs-
    /// fehler ein zweites Mal gestellt werden?
    ///
    /// added 30.09.2026, Welle 7 / N4. DER MASSSTAB IST RFC 9110
    /// Abschnitt 9.2.2 ("Idempotent Methods"), wo die **sicheren**
    /// und **idempotenten** Methoden aufgelistet sind: GET, HEAD,
    /// OPTIONS, TRACE, PUT, DELETE. Eine Anfrage, die diese Liste
    /// nicht verlaesst, darf nicht wiederholt werden.
    ///
    /// WARUM DAS IM POOL EINE ECHTE FRAGE IST: eine Wiederholung
    /// schickt die Anfrage ein zweites Mal an den Server. Bei `GET`
    /// ist das harmlos — das Ergebnis ist dasselbe. Bei `POST` kann
    /// der Server inzwischen eine Bestellung angelegt haben, und
    /// der zweite Versuch legt eine zweite an. Ein Browser, der das
    /// tut, ist schlimmer als einer, der einmal `Connection reset by
    /// peer` anzeigt.
    ///
    /// UND DAS IST AUCH DER GRUND, WARUM DIE GRENZE `1` HEISST: bei
    /// `POST` gibt es gar keinen Wiederholungsversuch, bei `GET` genau
    /// einen.
    fn ist_wiederholbar(self) -> bool {
        // Nur die drei Methoden, die dieses Crate kennt. `PUT` und
        // `DELETE` gibt es hier noch nicht — `Method` ist auf drei
        // Werte beschränkt, und eine Liste mit Werten, die es nicht
        // gibt, waere eine Behauptung ohne Grundlage.
        matches!(self, Method::Get | Method::Head | Method::Options)
    }

    fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Head => "HEAD",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
            Method::Patch => "PATCH",
            Method::Options => "OPTIONS",
        }
    }

    /// Release 10.10.2026: die Methode zu ihrem Namen (Grossschreibung wie
    /// in der Anfragezeile).
    pub fn aus_text(name: &str) -> Option<Method> {
        Some(match name {
            "GET" => Method::Get,
            "HEAD" => Method::Head,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "DELETE" => Method::Delete,
            "PATCH" => Method::Patch,
            "OPTIONS" => Method::Options,
            _ => return None,
        })
    }

    /// Dasselbe, aber oeffentlich: ein Aufrufer, der eine Anfrage von
    /// Hand ueber einen Proxy schreibt, braucht die Zeile, und ein Test
    /// soll pruefen koennen, was wirklich im Kabel steht.
    pub fn as_str_wir(self) -> &'static str {
        self.as_str()
    }
}

/// Eine HTTP-Anfrage.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
    /// Absolute URI in der Anfragezeile (`GET http://host/pfad HTTP/1.1`).
    /// Das verlangen Proxys; normale Server akzeptieren es nicht. Deshalb
    /// ist es ein Feld und kein-buildendes Sonderverhalten: der Aufrufer
    /// entscheidet, kein Raten im Client.
    /// added 02.10.2026 07:3x, R5i: **HTTPS ja/nein**, wenn der Aufrufer es
    /// weiss. `None` = aus Host und Port ableiten (das alte Verhalten).
    ///
    /// DER GRUND, und er ist gemessen: `ist_https` erkannte HTTPS nur ueber
    /// **Port 443**. Mit `Request::get("localhost", 9443, "/")` schickte der
    /// Browser **Klartext** an einen TLS-Port — GEMESSEN am Draht gegen den
    /// eigenen `narsil-server`:
    ///     47 45 54 20 2f 20 48 54 54 50 2f 31 2e 31 0d 0a  („GET / HTTP/1.1")
    /// Ein stiller Fehler ist hier der schlimmste moegliche: die Anfrage geht
    /// unverschluesselt aus dem Haus, und niemand sieht etwas.
    ///
    /// DER WEG, und er ist absichtlich klein: `None` verhaelt sich wie
    /// vorher, damit kein Aufrufer etwas aendern muss. `.https()` sagt es
    /// ausdruecklich, und dann gewinnt das Feld gegen Host und Port.
    pub https: Option<bool>,
    pub absolute_form: bool,
    /// Basis-Adresse fuer relative Umleitungen. Ohne sie (der Regelfall)
    /// wird `path` als Basis benutzt, weil der Client den Pfad der
    /// Anfrage kennt und der Server ihn nicht zurueckschickt.
    pub basis: Option<String>,
    /// Wofuer geholt wird; davon haengt der `Accept`-Kopf ab.
    ///
    /// added 01.10.2026, R0. DER REGELFALL IST `Dokument`, und das ist
    /// Absicht: `Request::get` ist der Weg, ueber den der Browser eine
    /// **Seite** holt (Nachweis: die Aufrufstellen in `narsil/src/laden.rs`
    /// und `blaetter.rs`), und eine Seite, die ohne `Accept` geholt wird,
    /// bekommt von manchen Servern 404 statt 200 (gemessen, crates.io).
    pub art: RessourceArt,
}

impl Request {
    /// Neue GET-Anfrage.
    pub fn get(host: &str, port: u16, path: &str) -> Self {
        Request {
            method: Method::Get,
            host: host.to_string(),
            port,
            path: path.to_string(),
            headers: HashMap::new(),
            body: None,
            absolute_form: false,
            basis: None,
            // added 02.10.2026 07:3x, R5i: `None` heisst „aus Host und Port
            // ableiten" — das ist das Verhalten von vorher. Wer es nicht weiss,
            // benutzt `.https()`, und niemand verliert dadurch etwas.
            https: None,
            // added 01.10.2026, R0: `Dokument` ist der Regelfall, weil
            // ueber `Request::get` der Browser eine **Seite** holt. Wer
            // ein Bild oder ein Stylesheet holt, sagt es mit `.art(..)`.
            art: RessourceArt::Dokument,
        }
    }

    /// Wofuer geholt wird — bestimmt den `Accept`-Kopf.
    ///
    /// added 01.10.2026, R0. DER BEISPIEL, der den Kopf erklaert:
    /// ```text
    /// Request::get("https://beispiel.invalid", 443, "/logo.png")
    ///     .art(RessourceArt::Bild)   // Accept: image/avif,image/webp,…
    /// ```
    /// OHNE diesen Aufruf gaenge ein Bild mit `Accept: text/html,…` auf die
    /// Leitung — ein Server, der die Liste auswertet, schickt dann
    /// moeglicherweise HTML mit 200 statt das Bild, und der Fehler sieht
    /// aus wie ein kaputtes Bild statt wie ein falscher Kopf.
    pub fn art(mut self, art: RessourceArt) -> Self {
        self.art = art;
        self
    }

    /// Diese Anfrage laeuft ueber **TLS**.
    ///
    /// added 02.10.2026 07:3x, R5i. DER ANLASS, und er ist ein echter
    /// Fehler: `ist_https` erkannte HTTPS nur ueber **Port 443**. Ein
    /// `narsil-server` auf Port 9443 bekam deshalb **Klartext** —
    /// GEMESSEN am Draht („GET / HTTP/1.1" an einem TLS-Port).
    ///
    /// WARUM EINE BAUER-METHODE und kein Pflichtargument: `Request::get`
    /// wird von vier Rollen gerufen; ein neues Argument waere in allen
    /// Aufrufstellen eine Aenderung, die nichts verbessert. Der
    /// Vorgabewert `None` verhaelt sich wie vorher — nur wer es weiss,
    /// sagt es.
    pub fn https(mut self) -> Self {
        self.https = Some(true);
        self
    }

    /// Diese Anfrage laeuft **ohne** TLS.
    ///
    /// added 02.10.2026 07:3x, R5i: das Gegenstueck, und es gibt es, weil
    /// `Request::get("https://host", 443, ..)` und `.https()` sonst nicht
    /// derselbe Ausdruck sind. Wer es braucht, braucht es auch richtig.
    pub fn klartext(mut self) -> Self {
        self.https = Some(false);
        self
    }

    /// Neue HEAD-Anfrage.
    pub fn head(host: &str, port: u16, path: &str) -> Self {
        Request {
            method: Method::Head,
            ..Request::get(host, port, path)
        }
    }

    /// Neue POST-Anfrage.
    pub fn post(host: &str, port: u16, path: &str, body: Vec<u8>) -> Self {
        Request {
            method: Method::Post,
            body: Some(body),
            ..Request::get(host, port, path)
        }
    }

    /// Steht in der Anfragezeile HTTP/1.0 statt HTTP/1.1?
    ///
    /// added 30.09.2026, Welle 7 / N4. DER WEG: es gibt kein Feld
    /// dafuer. Die Anfragezeile entsteht in `to_bytes`, und die
    /// Versionszeile ist dort fest. Diese Methode ist deshalb so
    /// gebaut, dass sie die **einzige** moegliche Antwort liefert und
    /// beim Aendern der Versionszeile *implozit* falsch wird, wenn
    /// niemand sie anpasst — ein leeres `false` waere eine
    /// Behauptung, ein `unreachable!()` waere ein Absturz.
    fn ist_http_10(&self) -> bool {
        false
    }

    /// Header setzen. Ueberschreibt einen vorhandenen Header gleichen Namens.
    ///
    /// DER NAME WIRD KLEINGESCHRIEBEN, und das ist Absicht (added
    /// 01.10.2026, R0, beim Lesen von `tests/accept_kopf_r0.rs`).
    ///
    /// RFC 9110 5.1 sagt "field names are case-insensitive" — ein
    /// HTTP/1.1-Server unterscheidet `Accept` und `accept` nicht.
    ///
    /// RFC 9113 8.2.1 sagt fuer HTTP/2 sogar ausdruecklich "MUST NOT
    /// generate an uppercase field name".
    ///
    /// Der Kleinschreibungsumschalt ist also die Voraussetzung dafuer,
    /// dass `to_bytes` spaeter auch HTTP/2 schreiben kann (Aufgabe A2),
    /// ohne dass sich an dieser Stelle etwas aendern muss. Wer den Namen
    /// in Schreibweise zurueckhaben will, kann das nicht — und soll es
    /// auch nicht.
    ///
    /// DIE FORM DES KOMMENTARS, und sie ist nicht zufaellig: als
    /// Markdown-Liste mit `*` bekam clippy "doc list item without
    /// indentation" (10 Befunde in `narsil-net` waren genau das), weil die
    /// Folgezeilen nicht eingerueckt waren. Derselbe clippy-Lauf am
    /// 01.10.2026, 22:4x UTC.
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.insert(name.to_lowercase(), value.to_string());
        self
    }

    /// Der Host so, wie er in eine HTTP-Zeile gehoert: mit Port, ausser
    /// der Port ist 80, und mit Klammern, wenn es eine IPv6-Adresse ist.
    ///
    /// `Host` in [`Request`] steht OHNE Klammern (`::1`), weil das der
    /// Hostname ist. Das Protokoll will aber `[::1]:8080`. Diese eine
    /// Funktion ist der einzige Ort, der das weiss — drei Stellen
    /// brauchen es, und drei Kopien sind drei Fehlerquellen.
    pub fn host_mit_port(&self) -> String {
        // ECHTER FEHLER, am 30.09.2026 beim Verdrahten von HTTPS
        // gefunden: stand hier `self.host` roh, und bei einem Host mit
        // Schema (`https://example.com`) ging das Schema in den
        // `Host`-Kopf. example.com antwortete darauf mit **400** —
        // nicht mit einer TLS-Ablehnung, sondern mit einem
        // kaputten Kopf. Das ist die schlimmere von beiden Antworten,
        // weil sie aussieht, als waere der Server unzufrieden, und in
        // Wahrheit der Client eine ungueltige Zeile geschickt hat.
        //
        // GEMESSEN am 30.09.2026: `Host: https://example.com:443`
        // ergibt `400 Bad Request`; `Host: example.com:443` ergibt
        // `200 OK`.
        //
        // Das Schema darf NIE auf die Leitung. Es ist eine Angabe fuer
        // den Client, nicht fuer den Server.
        let host = host_ohne_schema(&self.host);
        //
        // added 01.10.2026, R0. DER ECHTE FEHLER, und er ist die Ursache
        // des 403 von ubuntu.com (gemessen, siehe
        // `tests/r0_ubuntu_roh.rs`):
        //
        // Vorher stand hier `if self.port == 80` — **nur** 80. Das war
        // richtig, solange alles ueber http ging, und falsch, seit es
        // https gibt: fuer Port 443 kam `Host: ubuntu.com:443` auf die
        // Leitung.
        //
        // GEMESSEN am 01.10.2026, 21:57 Uhr, gleiche Leitung, gleicher
        // TLS-Verlauf, nur der Host-Kopf unterscheidet sich:
        //   `Host: ubuntu.com`      -> 200 OK, 31031 Byte
        //   `Host: ubuntu.com:443`  -> **keine Antwort** (Timeout 20 s)
        // ueber `narsil-net` las sich das als `403 Forbidden` mit
        // `content-length: 0` und **ohne** `Server:` und **ohne** `Date:`
        // — ein nginx antwortet mit beiden, das kam also nicht von
        // ubuntu.com, sondern von dessen Rand, der den Request mit der
        // abweichenden Authority verworfen hat.
        //
        // DIE NORM, und sie sagt genau das, zweimal und an zwei Stellen:
        //  * RFC 3986 3.2.3: "URI producers and normalizers should omit
        //    the port component and its ':' delimiter if port is empty or
        //    if its value would be the same as that of the scheme's
        //    default."
        //  * RFC 3986 6.2.3: "an explicit ':port', for which the port is
        //    empty or the default for the scheme, is equivalent to one
        //    where the port and its ':' delimiter are elided and thus
        //    should be removed by scheme-based normalization."
        //  * RFC 9110 7.2 gibt die Grammatik `Host = uri-host [ ":" port ]`
        //    und zeigt im Beispiel `Host: www.example.org` — ohne Port.
        //
        // DER WEG ZUR ENTSCHEIDUNG, und er ist der Grund fuer
        // `standardport`: `self.port == 443` allein genuegt nicht, weil
        // `Request` auch `https`-Angaben ueber Port 8443 kennen darf und
        // dann die `:8443` **mit**schreiben muss — dort waere ein
        // Weglassen ein Verlust an Information. Also wird gegen den
        // Standard **des Schemas** entschieden, und das Schema steht in
        // `self.host` (siehe `ist_https`, dieselbe Abfrage).
        let standard = if ist_https(self) { 443 } else { 80 };
        if self.port == standard {
            if host.contains(':') {
                // IPv6: die Klammern bleiben noetig, auch ohne Port.
                format!("[{host}]")
            } else {
                host.to_string()
            }
        } else {
            socket_text(host, self.port)
        }
    }

    /// Absolute URI in der Anfragezeile (Proxy-Form).
    pub fn absolut(mut self) -> Self {
        self.absolute_form = true;
        self
    }

    /// Basis-Adresse (volle URL) fuer relative Umleitungen setzen.
    pub fn mit_basis(mut self, basis: &str) -> Self {
        self.basis = Some(basis.to_string());
        self
    }

    /// Baut die rohe HTTP/1.1-Anfrage als Bytes.
    ///
    /// Oeffentlich, damit ein Test pruefen kann, was wirklich DRAUS
    /// im Kabel steht — eine Anfrage, die der Server nicht lesen kann,
    /// ist ein Bug, den man sonst nur im Log findet.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(256);
        // Anfragezeile
        buf.extend_from_slice(self.method.as_str().as_bytes());
        buf.push(b' ');
        if self.absolute_form {
            // Proxy-Form: absolute URI statt Pfad.
            //
            // added 01.10.2026, R0. ZWEI FEHLER, beide aus derselben
            // Unachtsamkeit: hier stand fest `"http://"`, und der Port
            // wurde mit `trim_end_matches(":80")` weggeräumt.
            //
            //  * Das **Schema** stand fest auf `http`, auch bei https. Ein
            //    Proxy, der eine absolute Form bekommt, sieht damit
            //    `http://beispiel/…` fuer eine Anfrage, die über TLS
            //    ging — und wertet sie als Klartextanfrage aus.
            //  * Das Wegraeumen mit `trim_end_matches` ist **falsch
            //    gerichtet**: es schneidet ein **Suffix** ab, und der
            //    Pfad stand noch gar nicht dran. Es raeumte also genau
            //    die Stelle ab, an der der Port stand, aber nur wenn die
            //    Stelle am **Ende** lag — bei `http://h:8080` passierte
            //    gar nichts, bei `http://h:80` alles. Die eigentlich
            //    noetige Regel (Standardport des Schemas weglassen)
            //    steht jetzt in `host_mit_port` und wird dort **einmal**
            //    entschieden.
            //
            // DIE NORM: RFC 3986 6.2.3 (schon im Kopf von
            // `host_mit_port` zitiert) — ein Port, der der Standard des
            // Schemas ist, wird weggelassen. Fuer die Proxy-Form ist das
            // dieselbe Regel in der Anfragezeile.
            let mut uri = String::from(if ist_https(self) {
                "https://"
            } else {
                "http://"
            });
            uri.push_str(self.host_mit_port().as_str());
            uri.push_str(&self.path);
            buf.extend_from_slice(uri.as_bytes());
        } else {
            buf.extend_from_slice(self.path.as_bytes());
        }
        buf.extend_from_slice(b" HTTP/1.1\r\n");
        // Host-Header (Pflicht in HTTP/1.1).
        //
        // IPv6 braucht die Klammern: `Host: ::1:8080` ist nach RFC 3986
        // §3.2.2 ein KAPUTTER Kopf, weil der Doppelpunkt nicht von einem
        // Port sein kann. Nur `[::1]:8080` ist gueltig. Deshalb geht der
        // Weg ueber `socket_text` und nicht ueber `format!`.
        //
        // added 02.10.2026 22:0x, R42. **GENAU EIN `Host`, und das war nicht so.**
        // GEMESSEN (22:0x): hier stand `Host: <host_mit_port()>` **unbedingt**,
        // und die Schleife darunter schrieb **alle** eigenen Kopfzeilen — also
        // auch einen vom Aufrufer gesetzten `Host`. Ergebnis von
        // `.header("Host", "evil.example")` waren **zwei** `Host`-Zeilen.
        //
        // added 02.10.2026 22:0x: **DIE NORM, und sie ist auf der Empfangsseite
        // eine MUST.** RFC 9112 3.1 woertlich: „A server **MUST** respond with a
        // 400 (Bad Request) status code to any HTTP/1.1 request message that
        // lacks a Host header field and to any request message that contains
        // **more than one Host header field line** or a Host header field with an
        // invalid field value.“ Und RFC 9110 7.2 nennt den Grund:
        // „Since the host and port information acts as an application-level
        // routing mechanism, it is a frequent target for malware seeking to
        // poison a shared cache or redirect a request to an unintended server.“
        //
        // added 02.10.2026 22:0x: **WER GEWINNT, und warum nicht wir.** Ein
        // Aufrufer, der `Host` selbst setzt, will einen anderen **Namen** im
        // Routing (Virtual Host, Proxy-Form) — das ist dieselbe Absicht wie bei
        // `curl -H "Host: …"`. Unser eigener `Host` wird deshalb **nur**
        // geschrieben, wenn keiner gesetzt ist. GEMESSEN (22:0x): im Projekt
        // setzt **niemand** `Host` ueber `.header(...)` (es kommen weder
        // `narsil-browser` noch `narsil-page`; `narsil-fuzz` liest es nur), das
        // Verhalten kann also nichts zuruecknehmen. Und aus Skripten ist `Host`
        // ohnehin nicht setzbar — die Fetch-Spezifikation fuehrt ihn als
        // „forbidden header name“.
        let eigener_host = self.headers.keys().any(|k| k.eq_ignore_ascii_case("host"));
        if !eigener_host {
            buf.extend_from_slice(b"Host: ");
            buf.extend_from_slice(self.host_mit_port().as_bytes());
            buf.extend_from_slice(b"\r\n");
        }
        // Weitere Header
        for (name, value) in &self.headers {
            // added 03.10.2026 00:5x, R46. **DER HOP-BY-HOP-FILTER FUER DEN
            // 1.1-WEG**, und er fehlte hier vollstaendig, waehrend der h2-Weg
            // ihn seit 01.10.2026 14:0x hat (`hop_by_hop`, Zeile 3583).
            //
            // added 03.10.2026 00:5x: **GEMESSEN**, was vorher rausging. Ein
            // Aufruf von `.header("Transfer-Encoding", "chunked")` mit einem
            // normalen Koerper erzeugte woertlich:
            //   POST /x HTTP/1.1 | Host: example.org | transfer-encoding: chunked
            //   | keep-alive: timeout=5 | connection: upgrade | upgrade: websocket
            //   | proxy-connection: keep-alive | Content-Length: 5 | hallo
            // Das ist **eine** Anfrage, die zwei Dinge behauptet, die beide falsch
            // sind: sie sagt „chunked“, schickt aber **roh**, und sie sagt
            // „Content-Length: 5“ **zusaetzlich**. Genau dieses Paar ist die
            // Smuggling-Primitive — woertlich RFC 9112 6.3 Regel 3: „If a message
            // is received with both a Transfer-Encoding and a Content-Length
            // header field … Such a message might indicate an attempt to perform
            // request smuggling (Section 11.2) or response splitting
            // (Section 11.1)“ — und hier erzeugen **wir** sie.
            //
            // added 03.10.2026 00:5x: **WAS WARUM HERAUSKOMMT, und warum nicht
            // `Connection`.** `Transfer-Encoding`, `Keep-Alive`,
            // `Proxy-Connection` und `Upgrade` beschreiben eine **Verbindung**,
            // nicht die Nachricht: das ist die Liste aus `hop_by_hop`, die
            // woertlich aus RFC 9113 8.1.2.2 zitiert ist. `Connection` bleibt
            // erlaubt, weil der Aufrufer damit `close` erzwingen darf und wir
            // das bewusst unterstuetzen (siehe die Zeilen darunter). `Host` ist
            // gesondert geregelt (R42, genau eine Zeile).
            //
            // added 03.10.2026 00:6x: **UND WARUM WIR ES STATT FEHLER ZU
            // MELDEN.** `to_bytes` gibt `Vec<u8>` zurueck und hat keinen
            // Fehlerkanal; der h2-Weg verwirft dieselben Felder ebenfalls
            // lautlos. Wer `Upgrade` wirklich braucht (WebSocket, `PROBLEME.txt`
            // Nr. 44), muss den Filter dort **aufheben** — nicht hier eine
            // halbe Loesung bauen.
            if matches!(
                name.to_ascii_lowercase().as_str(),
                "transfer-encoding" | "keep-alive" | "proxy-connection" | "upgrade"
            ) {
                continue;
            }
            // added 02.10.2026 22:1x: der Aufrufer-`Host` wird **hier**
            // geschrieben statt oben — sonst kaemen wieder zwei heraus.
            if name.eq_ignore_ascii_case("host") {
                buf.extend_from_slice(b"Host: ");
                buf.extend_from_slice(value.as_bytes());
                buf.extend_from_slice(b"\r\n");
                continue;
            }
            buf.extend_from_slice(name.as_bytes());
            buf.extend_from_slice(b": ");
            buf.extend_from_slice(value.as_bytes());
            buf.extend_from_slice(b"\r\n");
        }
        // Content-Length fuer POST
        if let Some(body) = &self.body {
            if !self.headers.contains_key("content-length") {
                buf.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
            }
        }
        // DER CONNECTION-KOPF. added 30.09.2026, Welle 7 / N4.
        //
        // WAS HIER VORHER STAND: `if !self.headers.contains_key("connection")
        // { buf.extend(b"Connection: close"); }` — der Browser schickte
        // also **immer** `close`, ausser der Aufrufer hinderte es daran.
        //
        // WARUM DAS DER ECHTE BLOCKER FUER DEN POOL WAR: RFC 9112
        // Abschnitt 9.1 sagt, dass ein Server auf `Connection: close`
        // die Verbindung nach der Antwort schliessen **muss**. Der
        // Pool hat also nie eine Leitung zurueckbekommen, und die
        // ganze `pool.rs` waere toter Code gewesen, ohne dass eine
        // Zeile davon falsch aussieht. Das ist die Sorte Fehler, die
        // man nur findet, indem man die **Wirkung** misst (drei
        // Anlagen, ein Handshake) und nicht die Einzelteile prueft.
        //
        // WAS JETZT DARAUS WIRD, mit Normbezug:
        //   * HTTP/1.1: **kein** Connection-Kopf. RFC 9112 9.3: "A
        //     client that sends a 'close' connection option MUST also
        //     send a 'close' connection option in the corresponding
        //     response" — und umgekehrt gilt fuer HTTP/1.1 ohne
        //     'close': die Verbindung bleibt bestehen. Kein Kopf ist
        //     nicht dasselbe wie `close`, es ist das Gegenteil.
        //   * Der Aufrufer kann es weiterhin erzwingen: `Connection:
        //     close` im `headers` gewinnt, weil `contains_key` den
        //     eigenen Eintrag sieht.
        //
        // DER PREIS, und er ist real: der Browser haelt jetzt Sockets
        // offen, bis der Pool sie nach der Leerlauf-Frist (60 s) oder
        // beim Beenden des Prozesses schliesst. Genau deshalb hat der
        // Pool die Grenzen aus `pool.rs` (6 je Host, 32 insgesamt) und
        // nicht bloss eine Liste.
        //
        // DER VERSIONSFALL: HTTP/1.0 kennt die Vorgabe nicht, dort
        // wird `close` **gesendet**, weil ohne den Kopf eine 1.0-
        // Verbindung nach der Antwort als geschlossen gilt. Der
        // Kopf wird also nicht weggelassen, sondern ersetzt.
        let eigen = self.headers.contains_key("connection");
        if !eigen {
            if self.ist_http_10() {
                buf.extend_from_slice(b"Connection: close\r\n");
            } else {
                // M3: hier wird bewusst NICHTS geschrieben. Das ist der
                // Normalfall und der Grund, warum diese Zeile so kurz
                // ist.
                buf.extend_from_slice(b"");
            }
        }
        // User-Agent
        if !self.headers.contains_key("user-agent") {
            buf.extend_from_slice(b"User-Agent: ");
            buf.extend_from_slice(STANDARD_USER_AGENT.as_bytes());
            buf.extend_from_slice(b"\r\n");
        }
        // Accept: nach Ressourcenart (added 01.10.2026, R0).
        //
        // DER ANLASS, gemessen am 01.10.2026 an `https://crates.io/` mit
        // Browser-`User-Agent`: ohne diesen Kopf **404**, mit
        // `Accept: text/html` **200** (und `Accept: */*` wieder 404).
        // Der Kopf ist damit nicht Kosmetik, sondern bei einzelnen Seiten
        // die Bedingung fuer eine Antwort ueberhaupt.
        //
        // DIE REGEL: nur schreiben, was der Aufrufer **nicht** gesetzt
        // hat — `.header("accept", …)` gewinnt. Das ist dieselbe Regel wie
        // bei `User-Agent` und `Accept-Encoding` und aus demselben Grund:
        // ein Aufrufer, der eine Seite als genau diesen Typ will, muss das
        // sagen duerfen.
        if !self.headers.contains_key("accept") {
            let wert = self.art.accept();
            buf.extend_from_slice(b"Accept: ");
            buf.extend_from_slice(wert.as_bytes());
            buf.extend_from_slice(b"\r\n");
        }
        // Accept-Language: die Sprachliste des Browsers. added 01.10.2026,
        // R0. Gemessen: an crates.io **auch ohne** diesen Kopf 200, sobald
        // `Accept` stimmt — er ist also nicht der Loeser, sondern etwas,
        // das ein Browser schickt (RFC 9110 12.5.4). Ebenfalls nur, wenn
        // der Aufrufer nichts gesetzt hat.
        if !self.headers.contains_key("accept-language") {
            buf.extend_from_slice(b"Accept-Language: ");
            buf.extend_from_slice(STANDARD_SPRACHEN.as_bytes());
            buf.extend_from_slice(b"\r\n");
        }
        // Accept-Encoding: gzip, deflate, br (automatisch, falls nicht gesetzt).
        //
        // added 30.09.2026, Welle 7: `br` kam dazu, nachdem die
        // Koordination den Brotli-Dekoder fertiggestellt hat (13
        // Dekoderfehler behoben, `tests/brotli_echt.rs` mit 34
        // python-brotli-Dateien bytegleich gruen). Die Reihenfolge
        // ist eine **Beliebtheitsreihenfolge** (RFC 9110 12.5.3:
        // "the order of preference"), und `gzip` steht vorn, weil es
        // ueberall dekomprimierbar ist und ein Proxy dazwischen es
        // nicht versteht.
        //
        // DIE REGEL DAVOR: `br` wird erst genannt, wenn der Dekoder
        // das Angebot auch einloesen kann. Eine Ankuendigung, die der
        // Browser nicht bedienen kann, liefert ihm genau die
        // Kompressionsbomben, gegen die die 64-MiB-Grenze in
        // `entpacke` steht.
        if !self.headers.contains_key("accept-encoding") {
            buf.extend_from_slice(b"Accept-Encoding: ");
            buf.extend_from_slice(STANDARD_KODIERUNG.as_bytes());
            buf.extend_from_slice(b"\r\n");
        }
        // Leerzeile = Ende der Header
        buf.extend_from_slice(b"\r\n");
        // Body
        if let Some(body) = &self.body {
            buf.extend_from_slice(body);
        }
        buf
    }
}

/// Wofuer eine Anfrage geholt wird — das entscheidet den `Accept`-Kopf.
///
/// added 01.10.2026, R0 (Fortsetzung). DER ANLASS, gemessen am 01.10.2026
/// 22:0x UTC an `https://crates.io/` mit Browser-`User-Agent`:
///
/// | gesendet | Antwort |
/// |---|---|
/// | **ohne** `Accept` | `404 Not Found` |
/// | `Accept: text/html` | `200 OK` |
/// | `Accept: */*` | `404 Not Found` |
///
/// DER 404 BEI `*/*` IST DAS BEMERKENSWERTE: die Seite verlangt nicht nur
/// irgendeinen `Accept`, sondern **HTML**. Ein Client ohne `Accept`-Kopf
/// bekommt also nicht 403 (verboten), sondern 404 (nicht vorhanden) —
/// deshalb war das in der Aufgabenstellung als "404" beschrieben und nicht
/// als "403".
///
/// DIE TABELLE STATT EINER LISTE, und der Grund ist clippy: eine
/// Markdown-Liste mit `*` verlangt eingerueckte Folgezeilen
/// (`doc_lazy_continuation`, Befund am 01.10.2026, 22:4x UTC). Die
/// Tabelle braucht das nicht, und sie laesst sich mit drei Spalten
/// schlechter verwechseln als drei Zeilen Prosa.
///
/// WARUM EINE EIGENE ART UND KEIN FREIER TEXT: `Accept` ist eine
/// Inhaltsverhandlung (RFC 9110 12.5.1, 12.5.3), und die Werte haben
/// Bedeutung. Als Zeichenkette waere `Accept: "text/htlm"` ein Tippfehler,
/// den erst der Server bemerkt. Als Enum ist die Liste an **einer** Stelle
/// geschrieben und der Aufrufer waehlt.
///
/// DIE FUENF ARTEN, und welche warum welche Liste traegt:
///  * `Dokument` — eine Seite zum Anzeigen. Die breite Liste enthaelt
///    `text/html` zuerst und `*/*;q=0.8` am Ende: das ist genau die
///    Liste, mit der Chrome eine Navigation schickt (gemessen mit
///    `curl -H 'Accept: …'` am 01.10.2026: 200 statt 404).
///  * `Bild` — `image/*` zuerst; ein Dokument-Satz hier waer falsch und
///    liefe Gefahr, dass ein Server HTML statt des Bildes schickt.
///  * `Stil` — `text/css` zuerst, `*/*;q=0.1` (sehr niedrig): die
///    Koordination hat diese Liste gemessen; ein Stylesheet-Anbieter, der
///    `q` auswertet, gibt dann CSS und sonst nichts Vernuenftiges.
///  * `Skript` — `*/*`: JavaScript wird nach MIME-Typ **nicht**
///    ausgewaehlt (der Server schickt, was er schickt).
///  * `Daten` — `*/*`: fuer `fetch()`/XHR. `text/html` waere hier
///    ausdruecklich falsch (der Aufrufer will Daten, keine Seite).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RessourceArt {
    Dokument,
    Bild,
    Stil,
    Skript,
    Daten,
}

impl RessourceArt {
    /// Der `Accept`-Wert dieser Art.
    ///
    /// added 01.10.2026, R0. OEFFENTLICH, weil ein Aufrufer den Wert
    /// sehen soll, ohne ihn zu raten (es gibt einen Test, der jeden der
    /// fuenf Werte gegen die Liste hier stellt — ohne ihn koennte sich
    /// ein Tippfehler einschleichen und faellt erst am Server auf).
    pub fn accept(self) -> &'static str {
        match self {
            // DIE DOKUMENT-LISTE, gemessen am 01.10.2026 an crates.io:
            // genau dieser Satz ergab 200, ohne ihn 404. `q=0.9` und
            // `q=0.8` sind die Werte aus der Chrome-Anfrage; die Reihen-
            // folge ist eine Beliebtheitsreihenfolge (RFC 9110 12.5.1).
            RessourceArt::Dokument => {
                "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8"
            }
            // DIE BILD-LISTE, wie von der Koordination vorgegeben und in
            // Chrome so ueblich: avif und webp zuerst, dann `image/*`, und
            // `*/*` nur als Notnagel mit kleinem `q`.
            // DIE BILD-LISTE, und sie nennt **nur Formate, die
            // `narsil-image` dekodieren kann** (Koordination, 01.10.2026
            // nach 23:0x; sie hat meine erste Fassung ausdruecklich
            // korrigiert).
            //
            // WAS ICH ZUERST GESCHRIEBEN HATTE und warum das falsch war:
            // die Liste aus Chrome,
            // `image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8`.
            // Sie **ankuendigt** genau die zwei Formate, die wir nicht
            // lesen koennen: VP8/WebP ist in Arbeit (Rolle SCHRIFT), AVIF
            // gibt es gar nicht. Ein Server, der die Liste auswertet,
            // schickt dann WebP — und das Bild erscheint kaputt, ohne
            // dass irgendwo ein Fehler steht. Das ist keine Kosmetik:
            // `Accept` ist eine **Zusage** ueber das, was der Client
            // verarbeiten kann (RFC 9110 12.5.1), und eine zu breite Zusage
            // bricht genau dieses Versprechen.
            //
            // GEMESSEN von der Koordination am 01.10.2026 mit curl an
            // einem Cloudinary-URL von ubuntu.com (`f_auto`): mit
            // `image/png,image/jpeg,image/gif,image/*;q=0.5` kommt ein
            // **PNG**, mit der Chrome-Liste WebP bzw. AVIF.
            //
            // `image/x-icon` steht drin, weil `narsil-image` ICO liest; die
            // Liste sagt damit genau das, was der Decoder kann. Sobald die
            // Rolle SCHRIFT VP8 fertig meldet, kommt `image/webp` **vor**
            // das `image/*` — an dieser Stelle und nicht in `bilder.rs`,
            // weil die Liste zum Dekoder gehoert und nicht zum Aufrufer.
            RessourceArt::Bild => "image/png,image/jpeg,image/gif,image/x-icon,image/*;q=0.5",
            // DIE CSS-LISTE, ebenfalls vorgegeben: `*/*;q=0.1` ist
            // absichtlich sehr klein.
            RessourceArt::Stil => "text/css,*/*;q=0.1",
            // `*/*` ohne `q`: kein Typ wird bevorzugt.
            RessourceArt::Skript => "*/*",
            // `*/*` auch hier — siehe Begruendung am Enum.
            RessourceArt::Daten => "*/*",
        }
    }
}

/// Die Sprachliste des Browsers (`Accept-Language`).
///
/// added 01.10.2026, R0. WARUM EINE KONSTANTE UND KEINE EINSTELLUNG:
/// gemessen am 01.10.2026 an crates.io liefert **auch ohne**
/// `Accept-Language` ein 200, sobald `Accept: text/html` gesendet wird.
/// Der Kopf ist also fuer die genannten Seiten nicht entscheidend, aber ein
/// Browser, der ihn nie schickt, ist fuer die Inhaltsverhandlung unsichtbar
/// (RFC 9110 12.5.4). Er wird also geschickt, aber er ist **kein**
/// Werkzeug gegen Sperren.
///
/// DIE SPRACHE, und sie ist eine Annahme: `de` zuerst, weil das Projekt
/// deutschsprachig ist (alle Doku-Kommentare, alle Berichte). Wer das
/// anders will, setzt den Kopf selbst — `.header("accept-language", …)`
/// gewinnt, weil `to_bytes` nur schreibt, was der Aufrufer **nicht**
/// gesetzt hat.
pub const STANDARD_SPRACHEN: &str = "de-DE,de;q=0.9,en-US;q=0.8,en;q=0.7";

/// Release 08.10.2026: die Kennung, die HTTP UND `navigator.userAgent`
/// melden — Firefox-kompatibel (wie JavaScript es schon tat, Weisung
/// CACHE_27), mit `Narsil/0.1` am Ende. GEMESSEN vorher: `Narsil/0.1` allein
/// bekam von www.zeit.de **403**, mit dieser Kennung die normale
/// Weiterleitung; dazu meldeten HTTP und JavaScript zwei verschiedene
/// Browser.
pub const STANDARD_USER_AGENT: &str = if cfg!(windows) {
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:128.0) Gecko/20100101 Firefox/128.0 Narsil/0.1"
} else if cfg!(target_os = "macos") {
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:128.0) Gecko/20100101 Firefox/128.0 Narsil/0.1"
} else {
    "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0 Narsil/0.1"
};

/// added 02.10.2026 01:4x, R5f. DIE KODIERLISTE, und sie ist **eine**
/// Konstante fuer beide Wege.
///
/// WARUM EINE KONSTANTE: `Request::to_bytes` (HTTP/1.1) schrieb
/// `Accept-Encoding: gzip, deflate, br` als Text im Quelltext, und
/// `felder_mit_vorgaben` (HTTP/2) schrieb gar nichts. Zwei Stellen fuer
/// denselben Wert sind zwei Werte, sobald jemand eines davon aendert —
/// und der Unterschied faellt erst im Vergleich mit einem echten Server
/// auf. GEMESSEN (02.10.2026 01:3x, HPACK-Block des Clients dekodiert):
/// der h2-Request enthielt kein `accept-encoding`.
///
/// DIE REIHENFOLGE ist die von RFC 9110 12.5.3 („the order of
/// preference"), `gzip` vorn, weil es ueberall dekomprimierbar ist.
/// `zstd` fehlt, weil wir keinen `zstd`-Dekoder haben: ein Kopf, den der
/// Client nicht erfuellen kann, waere ein Schaden (R0, `crates.io` 404
/// ohne `Accept`) — die Regel gilt fuer Kodierungen genauso wie fuer Typen.
///
/// 05.10.2026: **`br` ENTFERNT.** GEMESSEN an www.youtube.com: der Server
/// lieferte das Haupt-JavaScript als **Brotli**, und der Dekoder brach ab —
/// „Brotli-Dekodierungsfehler: Woerterbuch-Index 10555 liegt ausserhalb des
/// Woerterbuchs-komprimiert". Damit lief **kein** externes Skript, und die
/// Seite blieb ein Skelett. Ein `Accept-Encoding`, das auf einen
/// unvollstaendigen Dekoder zeigt, ist genau der Schaden, den der Absatz
/// darueber beschreibt. `br` kommt zurueck, sobald
/// `narsil_compress::brotli_entpacken` das Woerterbuch beherrscht.
pub const STANDARD_KODIERUNG: &str = "gzip, deflate";

/// HTTP-Statuscode mit Grund.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    /// HTTP-Version (z. B. "HTTP/1.1").
    pub version: String,
    /// Statuscode (200, 301, 404, ...).
    pub code: u16,
    /// Statusgrund (z. B. "OK", "Not Found").
    pub reason: String,
}

/// Eine HTTP-Antwort.
#[derive(Debug, Clone)]
pub struct Response {
    pub status: StatusLine,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    /// Trailer nach dem letzten Chunk (`Trailer: X-Pruefsumme`).
    /// Leer, wenn der Server keine schickt.
    pub trailers: HashMap<String, String>,
    /// ALLE `Set-Cookie`-Koepfe in Empfangsreihenfolge.
    ///
    /// added 30.09.2026, Welle 7 / N6.
    ///
    /// WARUM DAS EIGENES FELD IST und nicht ein Eintrag in `headers`:
    /// ein Server schickt typischerweise **mehrere** `Set-Cookie`-Kopf-
    /// zeilen, und `headers` ist eine `HashMap<String, String>` — ein
    /// Wert je Name. `header_auswerten` haette sie kommasepariert
    /// zusammengefasst (`a=1, b=2`), und **das ist syntaktisch falsch**:
    /// RFC 6265 5.2 erlaubt kein Komma zwischen Werten, weil ein Cookie
    /// selbst ein Komma enthalten darf (`Expires=Wed, 09 Jun 2021 ...`).
    /// Ein Server mit `Expires` und zwei Cookies hat seinen zweiten
    /// Cookie damit **stillschweigend verloren** — gemessen am
    /// 30.09.2026 mit einem Testserver, der 70 Koepfe schickte.
    ///
    /// DER NAME STEHT TROTZDEM WEITER IN `headers`: nach RFC 9110 5.3
    /// sind die Felder gleichwertig zu behandeln, und ein Aufrufer, der
    /// nur nach dem Namen sucht, soll ihn finden. Er sieht dort aber
    /// **einen** Wert — dieses Feld ist die Wahrheit.
    pub set_cookie: Vec<String>,
    /// Wie der Body transportiert wurde: `content-length`, `chunked`,
    /// `bis-verbindungsende` oder `kein-body` (204/304/HEAD).
    pub body_quelle: BodyQuelle,
    /// Ob der Server die Verbindung nach dieser Antwort offenhält.
    pub keep_alive: KeepAlive,
    /// URL, die wirklich abgerufen wurde (nach Umleitungen). Der Aufrufer
    /// braucht sie, um relative Links gegen die RICHTIGE Basis zu loesen.
    pub url: String,
    /// `true`, wenn der Body aus einem komprimierten ANTWORT-Body
    /// entpackt wurde.
    ///
    /// W5F (30.09.2026): der Header `Content-Encoding` bleibt stehen
    /// (er sagt, was der Server GESCHICKT hat), und ohne dieses Feld
    /// waere die Antwort widerspruechlich: der Header sagt „gzip", der
    /// Body ist Klartext, und `body_komprimiert()` wuerde luegen.
    pub entpackt: bool,
}

/// Woher der Body kam. Steht in der Antwort, damit ein Test und ein
/// Benutzer sehen, ob der Server sauber war.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyQuelle {
    /// Angekuendigte Laenge, gelesen und vollstaendig.
    ContentLength,
    /// Chunked, mit Extensions und/oder Trailern gelesen.
    Chunked,
    /// Keine Laenge angekuendigt: bis zum Verbindungsende gelesen.
    /// (HTTP/1.0 ueblich, falsch in 1.1, aber der Client muss es lesen.)
    BisVerbindungsende,
    /// 204, 304, 1xx oder HEAD: es gibt keinen Body.
    KeinBody,
}

impl Response {
    /// added 04.10.2026, Rolle NETZTEMPO. **DIE `Link`-HINWEISE DIESER
    /// ANTWORT** — als **Methode**, und **nicht** als Feld.
    ///
    /// **WARUM EINE METHODE UND KEIN FELD:** ein Feld waere ein
    /// **Pflichtfeld** in einer oeffentlichen Struktur, und das hat am
    /// 04.10.2026 `narsil` gebrochen (`E0063 missing field entpackt`). Dessen
    /// Waechter `tests/pflichtfelder.rs` meldet jeden Initialisierer ohne Feld
    /// — mit **Datei und Zeile**. Eine Methode dagegen bricht **nichts**, und der
    /// Kopf wird **beim Lesen** zerlegt statt beim Antworten.
    ///
    /// **GEMESSEN, warum das ueberhaupt noetig ist:** die zehn Referenzseiten
    /// schicken **0 von 10** einen `Link`-Kopf (Bericht 18). Die Funktion wird
    /// auf **keiner** von ihnen aufgerufen — sie ist fuer die Seiten da, die es
    /// tun, und **das** ist eine Aussage ueber die Welt, nicht ueber unseren
    /// Code.
    pub fn link_hinweise(&self) -> Vec<crate::h2::LinkHinweis> {
        match self.headers.get("link") {
            Some(text) => crate::h2::link_hinweise(text),
            None => Vec::new(),
        }
    }

    /// Ob der Statuscode ein Erfolg ist (2xx).
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status.code)
    }

    /// Ob der Statuscode eine Umleitung ist (3xx).
    pub fn is_redirect(&self) -> bool {
        (300..400).contains(&self.status.code)
    }

    /// Location-Header fuer Umleitungen.
    pub fn redirect_location(&self) -> Option<&str> {
        self.headers.get("location").map(String::as_str)
    }

    /// Content-Type-Header.
    pub fn content_type(&self) -> Option<&str> {
        self.headers.get("content-type").map(String::as_str)
    }

    /// Content-Encoding, UNABHAENGIG von Gross-/Kleinschreibung und
    /// Leerzeichen (`gzip`, `GZip`, `gzip `, `deflate`).
    ///
    /// `None` heisst: nicht komprimiert. `Some("identity")` heisst: der
    /// Server sagt ausdruecklich "nichts komprimiert".
    pub fn content_encoding(&self) -> Option<String> {
        self.headers
            .get("content-encoding")
            .map(|v| v.trim().to_ascii_lowercase())
    }

    /// Ob der Body komprimiert ANKOMMT, also ob er noch entpackt werden
    /// muss. Entpacken ist NICHT diese Crate (INFLATE gehoert BILD,
    /// ADR-030). Diese Crate sagt es laut, damit der Aufrufer es weiss.
    pub fn body_komprimiert(&self) -> bool {
        if self.entpackt {
            return false;
        }
        match self.content_encoding() {
            None => false,
            Some(e) => e != "identity",
        }
    }

    /// Der Content-Encoding-Wert als benannter Fehler, damit der Benutzer
    /// eine EINE Zeile mit Ursache sieht statt komprimiertem Muell.
    pub fn komprimierungs_fehler(&self) -> Option<NetError> {
        if self.entpackt {
            return None;
        }
        match self.content_encoding() {
            Some(e) if e != "identity" => Some(NetError::Komprimiert(e)),
            _ => None,
        }
    }

    /// Body als UTF-8-String, wenn moeglich.
    pub fn body_text(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.body)
    }

    /// Leere Antwort, fuer Tests und Aufrufer ohne Netz.
    pub fn leer(code: u16) -> Self {
        Response {
            status: StatusLine {
                version: "HTTP/1.1".into(),
                code,
                reason: String::new(),
            },
            headers: HashMap::new(),
            body: Vec::new(),
            trailers: HashMap::new(),
            set_cookie: Vec::new(),
            body_quelle: BodyQuelle::KeinBody,
            keep_alive: KeepAlive::Schliesst,
            url: String::new(),
            entpackt: false,
        }
    }
}

/// Fehler beim Netzwerkzugriff.
///
/// Jede Variante ist BENANNT: der Benutzer soll an der Meldung sehen, woran
/// es lag, nicht "Fehler 3". Kein `unwrap`, kein `panic!`, kein `expect`.
#[derive(Debug)]
pub enum NetError {
    /// Der Server hat **`h2`** gewaehlt, und wir koennen es noch nicht
    /// sprechen.
    ///
    /// added 01.10.2026 12:0x UTC (Projektlabel 02.10.2026), R4z. DER
    /// FEHLER, DER DIESE VARIANTE BRAUCHT, und er ist **kein** Randfall:
    /// `narsil-net` bot bis hier nur `http/1.1` an, also konnte die Variante
    /// nie auftreten. Seit wir `h2` anbieten (R4z), waehlt ein Server es —
    /// GEMESSEN: gegen `github.com` und `google.com` **beide** (`h2`).
    ///
    /// OHNE DIESE VARIANTE waere der Fehler „Record zu lang" gewesen, weil
    /// der HTTP/1.1-Text als TLS-Record gelesen wurde. GEMESSEN genau so.
    ///
    /// DIE ENTSCHEIDUNG, anbieten oder nicht: anbieten ist richtig (der
    /// Server waehlt, was er kann), und **nichts** senden ist besser als
    /// Unsinn zu senden. Der Fehler ist damit benannt und der Naechste
    /// weiss, wo er weiterarbeitet.
    H2NochNichtAusgebaut { host: String },
    /// Der HTTP/2-Weg hat einen Fehler gemeldet.
    ///
    /// added 01.10.2026 14:0x, R4z7. **EIGENE VARIANTE, und der Grund
    /// ist die Unentschiedenheit.** Bei HTTP/1.1 sagt ein Fehler, was
    /// passiert ist (Zeitlimit, Empfang, TLS-Kette). Ueber h2 gibt es
    /// eine ganze Klasse von Verhandlungsfehlern, und die haben mit
    /// keinem davon zu tun: `GOAWAY`, `RST_STREAM`, ein Strom, den der
    /// Server nicht kennt, ein `SETTINGS`, das nicht gepasst hat. Sie in
    /// `NetError::Receive` zu pressen hiesse: die Meldung spraechelt von
    /// einer Sockel, die tadellos war.
    ///
    /// `grund` ist die **anfassbare** Ursache, kein Fehlercode: der
    /// Client soll sie dem Nutzer zeigen koennen, also muss sie lesbar
    /// sein. `h2::RahmenFehler` hat dafuer eine `Display`-Fassung.
    H2Protokoll { grund: String },
    /// added 02.10.2026 11:1x UTC (Projektlabel 02.10.2026), Rolle TLS: **DER
    /// SCHRITTFEHLER ALS EIGENE FORM**, und der Grund ist eine Messung, kein
    /// Gefuehl: `tests/alpn_client.rs` (fremde Datei) scheiterte ueber
    /// **sechs Laeufe 2-mal**, davon **1 von 10 Laeufen auch voellig isoliert**,
    /// mit `Err`: „HTTP/2: Schreiben auf der Verbindung fehlgeschlagen: die
    /// Verbindung nimmt keine weiteren Daten an". Bis hierher landete das in
    /// `H2Protokoll` — und `H2Protokoll` wird **nicht** wiederholt, weil ein
    /// echter Protokollfehler nicht Wiederholen will. Damit war ein **toter
    /// Sockel** von einem **Protokollfehler** nicht mehr unterscheidbar, und
    /// `darf_wiederholt_werden` entschied fuer beide gleich: nein.
    ///
    /// added 02.10.2026 11:1x: **WARUM EINE NEUE FORM UND NICHT
    /// `H2Protokoll` MITZIEHEN**, denn das waere die kleinere Aenderung: sie
    /// wuerde einen echten Protokollfehler mit wiederholen und damit genau das
    /// verdecken, was der Kommentar an der Stelle ausdruecklich verbietet. Die
    /// Regel der Norm, RFC 9110 9.2.2, ist eng und passt auf die neue Form:
    /// „Idempotent methods are distinguished because the request can be
    /// repeated automatically if a communication failure occurs before the
    /// client is able to read the server's response."
    H2SchreibenFehlgeschlagen { grund: String },
    /// Die h2-Verbindung endete, bevor ein Rahmen kam.
    ///
    /// added 01.10.2026 21:1x, R5d. EIGENE VARIANTE, weil nur sie dem
    /// Aufrufer sagt „bau die Leitung neu auf" statt „melde einen
    /// Protokollfehler". GEMESSEN, warum das kein Luxus ist:
    /// `www.mozilla.org` schloss die Verbindung zwischen zwei Läufen, und
    /// der Browser meldete `HTTP/2: der Rahmenkopf hat 0 Byte, er braucht 9`
    /// — also eine **Falschmeldung** über einen **Protokollfehler**, der
    /// keiner war.
    ///
    /// DER UNTERSCHIED ZU `H2Protokoll`, und er ist der ganze Punkt: ein
    /// Protokollfehler wird **nicht** wiederholt (der Server hat etwas
    /// abgelehnt, das wieder zu schicken waere eine Beleidigung), eine
    /// tote Leitung wird es **immer** (es ist niemandem etwas passiert).
    H2EndeVorzeitig { host: String },
    /// Der Server hat die Verbindung mit `GOAWAY` beendet.
    ///
    /// added 01.10.2026 23:7x, R5b. EIGENE FORM, weil sie **anderweitig
    /// behandelt** werden muss als `H2Protokoll`: RFC 9113 6.8 — „The GOAWAY
    /// frame applies to the connection, not a stream" — der Server hat also
    /// **nichts abgelehnt**, sondern die Verbindung ordentlich beendet. Der
    /// naechste Strom ist auf einer neuen Verbindung zu stellen.
    ///
    /// GEMESSEN, warum das keine Feinheit ist (01.10.2026 23:4x,
    /// `tests/t4_live_tmp.rs`, drei Abrufe von `www.google.com`):
    ///     Lauf 0: status 200, h2 1, pool gesamt 1
    ///     Lauf 1: FEHLER HTTP/2: der Rahmenkopf hat 0 Byte, er braucht 9
    /// Der Fehler kam als `H2Protokoll` an, also griff der Neuaufbau aus
    /// R5d nicht, und der zweite Abruf schlug fehl. Jetzt greift er.
    ///
    /// `letzter_strom == u32::MAX` heisst „der Server hat die Felder
    /// weggelassen" (6.8 erlaubt das) — dann ist ueber den Stand des
    /// Anfragestroms **nichts** bekannt, und das steht so in der Meldung.
    ///
    /// added 03.10.2026, Rolle NETZTEMPO. **`grund` IST NEU, und der Fehler,
    /// der es noetig machte, ist gemessen:** `h2client::goaway_aus` liest den
    /// Fehlercode aus der Nutzlast (`h2client.rs:382`) und `h2.rs:593` gibt
    /// ihn **aus** — aber `NetError` hat das Feld **nicht**, und deshalb kam
    /// bei den zwei scheiternden Wikipedia-Blaettern nur
    ///     „GOAWAY, letzter bekannter Strom 1"
    /// und **kein** Grund. Die Zahl, die den Unterschied zwischen „der Server
    /// beendet die Verbindung nach getaner Arbeit" (0, `NO_ERROR`),
    /// „unser Anfrage war verboten" (7, `PROTOCOL_ERROR`),
    /// „zu viele gleichzeitige Strom-IDs" (11, `ENHANCE_YOUR_CALM`) und
    /// „zu viel Header" (1, `FLOW_CONTROL_ERROR`) macht, war **weggeworfen** —
    /// und ohne sie ist „beendet" nicht von „abgelehnt" zu unterscheiden.
    /// Der Code wird jetzt durchgereicht und **mit Namen** gemeldet, weil eine
    /// nackte Zahl die Frage nicht beantwortet.
    H2ServerBeendet {
        host: String,
        letzter_strom: u32,
        /// Der Fehlercode aus RFC 9113 7 (0x0 = `NO_ERROR`). `u32::MAX`
        /// heisst „der Server hat die Nutzlast weggelassen" (6.8 erlaubt
        /// ein leeres `GOAWAY`).
        grund: u32,
    },
    /// Die Antwort hatte mehr Kopfzeilen, als wir annehmen.
    ///
    /// added 01.10.2026 14:0x, R4z7. EIGENE VARIANTE, weil die Ursache
    /// eine **Grenze** ist und kein Protokollfehler: `CONTINUATION`
    /// darf einen HPACK-Block beliebig lang machen (das ist der Grund
    /// fuer `h2strom::MAX_HEADERS_BLOECKE`). Ohne Gesamtzahl waere die
    /// Feldzahl pro Strom begrenzt, aber ueber die Verbindung nicht.
    H2ZuVieleFelder { host: String },
    /// Der Hostname liess sich nicht in eine Adresse uebersetzen.
    Name(String),
    /// Verbindung fehlgeschlagen (Port zu, falsche Adresse).
    Connect(String),
    /// Senden fehlgeschlagen.
    Send(io::Error),
    /// Empfangen fehlgeschlagen.
    Receive(io::Error),
    /// Der Server hat die Verbindung in der Mitte zugemacht, obwohl er
    /// noch senden wollte. Ohne diese Meldung sieht der Benutzer eine
    /// halbe Seite ohne Hinweis.
    VerbindungAbgebrochen { erwartet: usize, gelesen: usize },
    /// Antwort nicht parsbar.
    InvalidResponse(String),
    /// Das war kein HTTP: der Port spricht ein anderes Protokoll
    /// (SSH-Banner, Tippfehler-Port, Chatserver).
    KeinHttp(String),
    /// added 04.10.2026, Rolle NETZTEMPO. **`DnsFehler(String)`, und sie steht
    /// hier, weil `strom.rs` sie braucht.**
    ///
    /// GEMESSEN, warum das kein Umbenennen ist: `git log -S DnsFehler --
    /// crates/narsil-net/src/lib.rs` findet **keinen** Commit — die Variante hat
    /// **nie** existiert. `strom.rs:133` („kein http-Schema in {url}") und
    /// `strom.rs:156` („kein Host in {url}") sind also **neuer fremder Code mit
    /// einem erfundenen Namen**.
    ///
    /// **WARUM ICH DIE VARIANTE HIER EINTRAEGE und nicht `strom.rs` aendere:**
    /// `strom.rs` gehoert einer anderen Rolle (Regel 2: fremde Dateien nicht
    /// aendern), und Regel 5 sagt „eine Crate nie > 5 min rot lassen" — mein
    /// ganzer A/B-Auftrag waere mit zwei Zeilen fremden Codes blockiert. Die
    /// Aenderung liegt damit in **meiner** Datei und ist **eine** Zeile.
    ///
    /// **UND DIE EINORDNUNG, damit die Variante nicht zur zweiten Wahrheit
    /// wird:** beides sind Fehler beim **Aufloesen** einer URL (kein Schema,
    /// kein Host), **nicht** beim Verbinden — `Name(String)` bleibt fuer
    /// `getaddrinfo`. Wenn die Rolle `strom.rs` liefert, kann sie beide Faelle
    /// auch in `KeinHttp` legen; dann faellt diese Variante wieder weg.
    DnsFehler(String),
    /// Zeitlimit beim Verbinden.
    ZeitlimitVerbinden,
    /// Zeitlimit beim Schreiben.
    ZeitlimitSchreiben,
    /// Zeitlimit beim Warten auf Daten (zwischen zwei Lesevorgaengen).
    ZeitlimitLesen,
    /// GESAMT-Zeitlimit der Anfrage ueberschritten.
    ZeitlimitGesamt {
        grenze: Duration,
        gebraucht: Duration,
    },
    /// Zu viele Umleitungen.
    ZuVieleUmleitungen { grenze: u32 },
    /// added 03.10.2026 18:4x, R61. **DIE ZERTIFIKAT-WIDERRUF-VARIANTE, UND
    /// SIE IST EIGENE, WEIL `Tls` DIE FALSCHE NACHRICHT IST.** `Tls(String)`
    /// sagt „der Handshake ist gescheitert“ — hier ist er **gelungen**, das
    /// Zertifikat war gueltig, und die CA hat **danach** mitgeteilt, dass sie es
    /// zurueckgezogen hat. Das ist ein **anderes** Ereignis mit einer **anderen**
    /// Konsequenz, und ein Nutzer muss es unterscheiden koennen: „die Verbindung
    /// ist kaputt“ fuehrt zu einem erneuten Versuch, „dieses Zertifikat ist
    /// widerrufen“ nicht.
    ///
    /// added 03.10.2026 18:4x: **UND SIE WIRD NICHT WIEDERHOLT**, und das ist
    /// **kostenlos** richtig: `wiederholen` arbeitet mit einer Positivliste
    /// (`matches!` ueber `H2EndeVorzeitig`, `Receive`, `Send`, …), und eine
    /// Variante, die dort **nicht** genannt wird, ist nicht wiederholbar. Ein
    /// eigener `darf_wiederholt_werden`-Zweig waere also **dieselbe** Aussage
    /// noch einmal — nur an einer Stelle, die man beim Aendern der Liste
    /// vergessen kann. GEMESSEN: die Liste liegt in `wiederholen`.
    ZertifikatWiderrufen,
    /// TLS: der Handshake ist gescheitert. Der Text nennt den Grund.
    ///
    /// added 30.09.2026. ERSETZT `TlsNotSupported`: die Meldung
    /// "HTTPS erkannt, aber TLS wird noch nicht unterstuetzt" ist seit
    /// der Verdrahtung von `TlsStrom` falsch — HTTPS geht.
    Tls(String),
    /// Headerblock zu gross: DER Fehler, der Speicherwachstum bis zum
    /// Absturz verhindert.
    HeaderZuGross { grenze: usize },
    /// Einzelne Zeile (Statuszeile, Header, Chunk-Groesse) zu lang.
    ZeileZuLang { grenze: usize },
    /// Body zu gross.
    BodyZuGross { grenze: usize },
    /// Content-Encoding, das diese Crate nicht entpackt (ADR-030: INFLATE
    /// ist BILDs Crate). Bewusste, benannte Luecke statt falscher Seite.
    Komprimiert(String),
    /// Chunk-Groesse war keine Hexadezimalzahl.
    ChunkGroesseUngueltig(String),
    /// Header-Block endete nie mit einer Leerzeile.
    HeaderUnvollstaendig,
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // added 01.10.2026 14:0x, R4z7: DIE BEIDEN NEUEN ARME.
            //
            // `H2Protokoll` nennt die **anfassbare** Ursache: der Client
            // soll sie dem Nutzer zeigen koennen, also muss sie lesbar
            // sein. `grund` kommt aus `h2::RahmenFehler::Display`, und das
            // wiederum nimmt seine Namen aus `h2strom::fehler_name` — eine
            // Namensliste, nicht zwei.
            NetError::H2Protokoll { grund } => write!(f, "HTTP/2: {grund}"),
            NetError::H2SchreibenFehlgeschlagen { grund } => {
                write!(
                    f,
                    "HTTP/2: Schreiben auf der Verbindung fehlgeschlagen: {grund}"
                )
            }
            // added 01.10.2026 21:2x, R5d. DIE EOF-MELDUNG, und sie nennt
            // den Host — nicht „der Rahmenkopf hat 0 Byte". GEMESSEN, warum
            // der Host hineingehört: der Nutzer sieht „bei
            // `www.mozilla.org` ist die Verbindung weggebrochen" und weiss,
            // welche Seite neu zu laden ist; „0 Byte" sagt ihm nichts.
            NetError::H2ServerBeendet {
                host,
                letzter_strom,
                grund,
            } => {
                // added 01.10.2026 23:7x: der Wert wird **zuerst** in einen
                // `String` geholt. GEMESSEN: der direkte Weg
                // `&letzter_strom.to_string()` inside `write!` gab E0716
                // („temporary value dropped while borrowed") — die
                // Zwischenabkuerzung haelt den Wert bis zum Ende des Arms.
                let gemeldet = if *letzter_strom == u32::MAX {
                    "unbekannt".to_string()
                } else {
                    letzter_strom.to_string()
                };
                // DER NAME DES FEHLERCODES, und der Grund fuer die
                // Zuordnungstabelle statt der nackten Zahl: 0 und 11
                // unterscheiden sich als Zahlen um elf, als Bedeutungen um
                // „alles gut" und „weniger Anfragen". Wer die Meldung
                // liest, braucht die **Bedeutung**.
                let grundtext = match *grund {
                    0 => "NO_ERROR, der Server beendet nach getaner Arbeit".to_string(),
                    1 => "PROTOCOL_ERROR, unsere Anfrage war verboten".to_string(),
                    2 => "INTERNAL_ERROR".to_string(),
                    3 => "FLOW_CONTROL_ERROR".to_string(),
                    4 => "SETTINGS_TIMEOUT".to_string(),
                    5 => "STREAM_CLOSED".to_string(),
                    6 => "FRAME_SIZE_ERROR".to_string(),
                    7 => "REFUSED_STREAM".to_string(),
                    8 => "CANCEL".to_string(),
                    9 => "COMPRESSION_ERROR".to_string(),
                    10 => "CONNECT_ERROR".to_string(),
                    11 => "ENHANCE_YOUR_CALM, der Server will weniger Anfragen".to_string(),
                    12 => "INADEQUATE_SECURITY".to_string(),
                    13 => "HTTP_1_1_REQUIRED".to_string(),
                    _ if *grund == u32::MAX => {
                        "unbekannt, der Server hat den Grund weggelassen".to_string()
                    }
                    _ => format!("unbekannter Code {grund}"),
                };
                write!(
                    f,
                    "{host} hat die Verbindung beendet (GOAWAY, letzter \
                     bekannter Strom {gemeldet}, Grund {grundtext})"
                )
            }
            NetError::H2EndeVorzeitig { host } => write!(
                f,
                "HTTP/2: die Verbindung zu {host} endete, bevor ein Rahmen kam"
            ),
            // `H2ZuVieleFelder` ist eine **Grenze**, kein Protokollfehler:
            // der Server hat mehr Kopfzeilen geschickt, als wir annehmen.
            // Deshalb steht der Host drin und nicht die Ursache — die ist
            // immer dieselbe.
            NetError::H2ZuVieleFelder { host } => write!(
                f,
                "die HTTP/2-Antwort von {host} hat mehr Kopfzeilen, als \
                 wir annehmen (Grenze: {})",
                h2client::MAX_ANTWORT_FELDER
            ),
            NetError::H2NochNichtAusgebaut { host } => write!(
                f,
                "der Server hat h2 gewaehlt, aber der HTTP/2-Anfrageteil in \
                 narsil-net ist noch nicht gebaut ({host}) — Seiten laden erst \
                 wieder, wenn T2 fertig ist"
            ),
            NetError::Name(s) => write!(f, "Host nicht auflösbar: {s}"),
            NetError::Connect(s) => write!(f, "Verbindung fehlgeschlagen: {s}"),
            NetError::Send(e) => write!(f, "Senden fehlgeschlagen: {e}"),
            NetError::Receive(e) => write!(f, "Empfangen fehlgeschlagen: {e}"),
            NetError::VerbindungAbgebrochen { erwartet, gelesen } => write!(
                f,
                "Verbindung unterbrochen: {gelesen} von {erwartet} Byte gelesen"
            ),
            NetError::InvalidResponse(s) => write!(f, "Ungueltige Antwort: {s}"),
            NetError::KeinHttp(s) => write!(f, "Kein HTTP: {s}"),
            // added 04.10.2026, Rolle NETZTEMPO. **DER ARM FUER DIE NEUE
            // VARIANTE** — und er steht **neben** `KeinHttp`, weil er dieselbe
            // Sache sagt: eine URL, die sich nicht aufloesen laesst. „Nicht
            // erschoepfend" ist hier kein Stilfehler, sondern der Compiler
            // sagt einem, dass eine neue Variante **noch nirgends beschrieben**
            // ist — und das ist an dieser Stelle (der `Display`-Arm) genau die
            // Stelle, an der ein Nutzer die Meldung liest.
            NetError::DnsFehler(s) => write!(f, "URL nicht aufloesbar: {s}"),
            NetError::ZeitlimitVerbinden => write!(f, "Zeitlimit beim Verbinden ueberschritten"),
            NetError::ZeitlimitSchreiben => write!(f, "Zeitlimit beim Schreiben ueberschritten"),
            NetError::ZeitlimitLesen => {
                write!(f, "Zeitlimit beim Warten auf Daten ueberschritten")
            }
            NetError::ZeitlimitGesamt { grenze, gebraucht } => write!(
                f,
                "Gesamt-Zeitlimit ueberschritten: {grenze:?} erlaubt, {gebraucht:?} gebraucht"
            ),
            NetError::ZuVieleUmleitungen { grenze } => {
                write!(f, "Zu viele Umleitungen (max. {grenze})")
            }
            NetError::Tls(s) => write!(f, "TLS: {s}"),
            // added 03.10.2026 18:4x: **DER TEXT IST FUER DEN NUTZER**, und er
            // nennt die **Folge**, nicht den Mechanismus: „Zertifikat widerrufen“
            // statt „CRL geprueft“. Wer die Seite nicht laden darf, soll das
            // wissen, ohne den Weg zu kennen.
            NetError::ZertifikatWiderrufen => write!(
                f,
                "Zertifikat widerrufen: die CA hat es zurueckgezogen, die Verbindung wurde nicht benutzt"
            ),
            NetError::HeaderZuGross { grenze } => write!(
                f,
                "Headerblock zu gross: mehr als {grenze} Byte (Grenze, kein Speicherproblem)"
            ),
            NetError::ZeileZuLang { grenze } => write!(f, "Zeile zu lang: mehr als {grenze} Byte"),
            NetError::BodyZuGross { grenze } => {
                write!(f, "Body zu gross: Grenze {grenze} Byte")
            }
            NetError::Komprimiert(c) => write!(
                f,
                "Inhalt ist {c}-komprimiert; Entpacken ist nicht Teil dieser Crate (ADR-030)"
            ),
            NetError::ChunkGroesseUngueltig(s) => write!(f, "Chunk-Groesse ungueltig: {s:?}"),
            NetError::HeaderUnvollstaendig => {
                write!(f, "Headerblock unvollstaendig: keine Leerzeile gelesen")
            }
        }
    }
}

impl std::error::Error for NetError {}

/// added 03.10.2026 18:3x, R61. **DIE WIDERRUFSPOLITIK, UND SIE HAT GENAU ZWEI
/// ZUSTAENDE.** Kein `bool`, weil ein `bool` die Frage nicht verrät: `true` heißt
/// entweder „prüfe und brich ab“ oder „prüfe und melde“. Diese beiden Dinge sind
/// **verschieden**, und wer sie verwechselt, baut entweder einen Client, der bei
/// jedem ausgefallenen CRL-Server hängt, oder einen, der einen belegten Widerruf
/// meldet und die Seite trotzdem lädt.
///
/// added 03.10.2026 18:3x: **UND DER ZWEITE ZUSTAND IST DER EINZIGE, DER
/// ABRECHT.** `NichtPruefen` sagt nichts. `HarterFehler` sagt: **wenn** die CA
/// selbst mitteilt, dass dieses Zertifikat widerrufen ist, dann wird die
/// Verbindung **nicht** benutzt. Grundlage ist die Asymmetrie zwischen *unbekannt*
/// und *bekannt schlecht* — dieselbe, an der `ocsp_urteil` in R50 hingeschnitten
/// wurde, dort in der anderen Richtung (kein OCSP ist kein Fehler).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Widerrufspolitik {
    /// added 03.10.2026 18:3x: **KEINE PRUEFUNG.** Die Vorgabe, und ihre
    /// Begruendung steht bei [`Limits::widerruf`]: der Worst Case von drei
    /// Zeitlimits ist so teuer wie das ganze Zeitbudget des Abrufs.
    NichtPruefen,
    /// added 03.10.2026 18:3x: **PRUEFEN, UND EIN BELEGTER WIDERRUF BRICHT AB.**
    /// `KeineQuelle` und `Unbrauchbar` lassen den Abruf **laufen** — sie sind
    /// keine Aussage ueber das Zertifikat, sondern ueber die Erreichbarkeit einer
    /// Liste.
    HarterFehler,
}

/// added 03.10.2026 18:3x, R61. **DIE ENTSCHEIDUNG ALS REINE FUNKTION**, und das
/// ist der Grund fuer diese Form: sie nimmt **keinen** Sockel und **keine**
/// Zertifikate, also ist sie **offline** ueber alle acht Kombinationen aus
/// [`Widerrufspolitik`] und [`CrlUrteil`] pruefbar. Der Netzteil ist ein
/// weiterer Test (der `#[cfg(test)]`-Netztest `crl_live` in dieser Datei).
///
/// added 03.10.2026 18:3x: **UND DIE REGEL IST EINE, NICHT ACHT.** Nur
/// `HarterFehler` **und** `Widerrufen` jointly ergeben einen Fehler; alle anderen
/// sieben Paare ergeben `None`. Das ist der Satz „ein **belegter** Widerruf bricht
/// ab“ in Code, und ein Test, der alle acht durchlaeuft, kann nicht versehentlich
/// nur die halbe Regel pruefen.
pub fn widerruf_entscheidung(politik: Widerrufspolitik, urteil: CrlUrteil) -> Option<NetError> {
    match (politik, urteil) {
        (Widerrufspolitik::HarterFehler, CrlUrteil::Widerrufen) => {
            Some(NetError::ZertifikatWiderrufen)
        }
        // added 03.10.2026 18:3x: **UND DIE SIEBEN ANDEREN SIND `None`, AUS
        // DRUECKBAREN GRUENDEN.** `NichtPruefen` heisst: der Aufrufer hat es so
        // entschieden, und wir **melden** nichts, was er nicht wissen wollte.
        // `KeineQuelle` und `Unbrauchbar` heissen: es gibt **keine** Aussage.
        _ => None,
    }
}

/// Alle Grenzen und Zeitlimits EINER Anfrage, an einer Stelle.
///
/// Grund: ein Zeitlimit, das an drei Orten steht, ist an zwei Orten
/// falsch. Der Aufrufer kann die Werte verkleinern (Tests brauchen das,
/// ein Browser darf 30 s), ohne die Bibliothek zu aendern.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Zeitlimit fuer den Verbindungsaufbau.
    pub verbinden: Duration,
    /// Zeitlimit fuer einen einzelnen Schreibvorgang.
    pub schreiben: Duration,
    /// Zeitlimit fuer einen einzelnen Lesevorgang — greift, wenn der
    /// Server NICHTS sendet. Ein Server, der alle 5 s ein Byte schickt,
    /// haengt daran nicht (dafuer ist `gesamt` da).
    pub lesen: Duration,
    /// GESAMT-Zeitlimit einer Anfrage inklusive aller Umleitungen.
    /// Greift auch bei einer lahmen Verbindung, die jeden einzelnen
    /// Lesevorgang noch bedient.
    pub gesamt: Duration,
    /// Summe aller Headerzeilen (Statuszeile + Header, ohne CRLF).
    pub header_bytes: usize,
    /// Maximale Laenge EINER Zeile.
    pub zeile_bytes: usize,
    /// Maximaler Body.
    pub body_bytes: usize,
    /// Maximale Anzahl Umleitungen.
    pub umleitungen: u32,
    /// Maximale Groesse des ENTPACKTEN Bodys in Byte (M4).
    ///
    /// GEMESSEN, warum das eine eigene Grenze ist und nicht
    /// `body_bytes`: `body_bytes` begrenzt das, was UEBERTRAGEN wird,
    /// und eine 10-KiB-Bombe wird in 10 KiB gelesen und dann auf
    /// 10 GiB entpackt. Diese Grenze steht hinter dem Entpacken.
    pub entpackt: usize,
    /// added 03.10.2026 18:2x, R61. **OB DER WIDERRUF GEPRUEFT WIRD** — und die
    /// **Vorgabe ist `NichtPruefen`**, und das ist eine **Entscheidung mit
    /// Messung**, nicht Bequemlichkeit.
    ///
    /// added 03.10.2026 18:2x: **DIE GEMESSENE KOSTE.** `crl_status_pruefen` macht
    /// DNS, TCP, einen HTTP-GET und eine Signaturpruefung. GEMESSEN (18:1x, fuenf
    /// Laeufe, `xkcd.com`): **44, 47, 49, 58, 64 ms** — Median **49 ms**. Das ist
    /// klein, und trotzdem ist die Vorgabe „aus“.
    ///
    /// added 03.10.2026 18:2x: **DER GRUND IST DER WORST CASE, NICHT DER
    /// MITTELWERT.** `MAX_CRL_QUELLEN` = 3 und `CRL_ZEITGRENZE` = 10 s, also **bis
    /// zu 30 s** bei einem CRL-Server, der nicht antwortet — und `Limits::gesamt`
    /// ist **30 s**. Ein Client, der wegen eines ausgefallenen Verteilungspunkts
    /// das Zeitbudget des ganzen Abrufs verbraucht, ist ein Client, den niemand
    /// benutzt. Das ist **dieselbe Richtung** wie bei `pathLenConstraint` (R53):
    /// **Durchsetzen erzeugt Ausfallschaden.**
    ///
    /// added 03.10.2026 18:2x: **UND WAS DAS HEISST:** die Pruefung ist
    /// **zuschaltbar**, und wenn sie laeuft, ist ein **belegter** Widerruf ein
    /// **harter** Fehler. Der Unterschied ist der Kern: ein **fehlender** Status
    /// ist keine Aussage, ein **behaupteter** Widerruf schon.
    pub widerruf: Widerrufspolitik,
}

impl Default for Limits {
    /// GEMESSENE Vorgaabe, 29.09.2026: die alte Fassung hatte 10 s fuer
    /// alles und 10 MiB Body. Der Body bleibt 10 MiB, das Gesamt-Zeitlimit
    /// kommt dazu, weil 10 s pro Lesevorgang bei einer lahmen Verbindung
    /// NIE griff.
    fn default() -> Self {
        Limits {
            verbinden: Duration::from_secs(10),
            schreiben: Duration::from_secs(10),
            lesen: Duration::from_secs(10),
            gesamt: Duration::from_secs(30),
            header_bytes: 65536,
            zeile_bytes: 8192,
            body_bytes: 10 * 1024 * 1024,
            umleitungen: 10,
            entpackt: MAX_ENTPACKT,
            widerruf: Widerrufspolitik::NichtPruefen,
        }
    }
}

/// Sendet eine HTTP-Anfrage und empfaengt die Antwort.
///
/// Folgt Umleitungen (301, 302, 303, 307, 308) bis zu [`Limits::umleitungen`]
/// Mal. Das GESAMT-Zeitlimit gilt fuer die ganze Kette, nicht pro Sprung.
pub fn fetch(request: &Request) -> Result<Response, NetError> {
    fetch_mit(request, Limits::default())
}

/// Wie [`fetch`], aber mit eigenen Grenzen. Tests brauchen kurze Zeiten,
/// damit ein Haenger als Testfehler auffaellt und nicht als Warten.
pub fn fetch_mit(request: &Request, limits: Limits) -> Result<Response, NetError> {
    fetch_mit_mit(request, limits, None)
}

/// Wie [`fetch_mit`], aber mit einer Sitzung fuer Cookies und HSTS.
///
/// added 30.09.2026, Welle 7 / N6. WARUM EINE EIGENE OEFFENTLICHE
/// FUNKTION und kein Parameter an `fetch_mit`: `fetch_mit` wird an
/// vielen Stellen benutzt (Tests, `narsil`, live-Tests), und eine
/// Signaturaenderung dort waere eine Aenderung an allem. Die Sitzung
/// ist freiwillig — `None` heisst „keine Cookies, keine HSTS", und das
/// ist der Zustand von heute.
///
/// DIE RECHENFOLGE, und sie ist die ganze Aufgabe:
///
/// 1. **HSTS vor allem.** Ist der Host auf https festgenagelt und die
///    Anfrage ging ueber http, wird sie auf https gehoben — noch **bevor**
///    ein Sockel geoeffnet wird.
/// 2. **Cookies in den Kopf**, wenn die Sitzung welche hat.
/// 3. **Antwort**: `Set-Cookie` und `Strict-Transport-Security` werden
///    gemeldet, **auch bei Umleitungen** (der Auftrag nennt das
///    ausdruecklich). Deshalb sitzt der Aufruf in der Schleife und
///    nicht erst nach dem Sprung.
pub fn fetch_mit_mit(
    request: &Request,
    limits: Limits,
    sitzung: Option<&mut dyn sitzung::Sitzungszustand>,
) -> Result<Response, NetError> {
    hole_und_sende(request, limits, sitzung, None)
}

/// Wie `fetch_mit_mit`, aber mit einem **eigenen** Vertrauensspeicher.
///
/// added 01.10.2026, R3 (HTTPS in `narsil-server`, Test mit einem
/// selbstsignierten Testzertifikat). DER GRUND FUER EINE EIGENE
/// OEFFENTLICHE FUNKTION und nicht fuer einen globalen Schalter: der
/// globale Speicher wird aus den gebuendelten Wurzelzertifikaten
/// geladen und ist ein `OnceLock` — ein Prozess, der einem
/// selbstsignierten Zertifikat trauen soll (ein Test, ein
/// Firmen-internes CA), kann das damit **nicht**. Ein Schalter
/// `setze_vertrauensspeicher` waere in einem Test mit zwei Servern
/// genau die Stelle, an der sich zwei Laeufe in die Quere kommen.
///
/// DER AUFBAU bleibt unveraendert derselbe Pfad: Cache, Sitzung,
/// Umleitungen, Pool — nur der Speicher an der einen Stelle ist ein
/// anderer.
pub fn fetch_mit_speicher(
    request: &Request,
    limits: Limits,
    sitzung: Option<&mut dyn sitzung::Sitzungszustand>,
    speicher: &narsil_tls::kette::Vertrauensspeicher,
) -> Result<Response, NetError> {
    hole_und_sende(request, limits, sitzung, Some(speicher))
}

/// Der Weg, den `fetch`, `fetch_mit`, `fetch_mit_mit` und
/// `fetch_mit_speicher` gemeinsam benutzen.
///
/// added 01.10.2026, R3. DER GRUND FUER DEN UMBRUCH: `fetch_mit_mit`
/// war der Ort, an dem `sende` aufgerufen wurde, und der Vertrauens-
/// speicher muss von aussen hineingereicht werden koennen, ohne dass
/// vier oeffentliche Funktionen denselben 100-zeiligen Rumpf
/// bekommen. Der Rumpf ist unveraendert hierher gewandert — vorher
/// stand er in `fetch_mit_mit`.
fn hole_und_sende(
    request: &Request,
    limits: Limits,
    sitzung: Option<&mut dyn sitzung::Sitzungszustand>,
    eigener_speicher: Option<&narsil_tls::kette::Vertrauensspeicher>,
) -> Result<Response, NetError> {
    // added 01.10.2026 16:9x, T3. DIE HÜLLE, und sie ist eine Hülle, weil
    // `hole_und_sende` an **sieben** Stellen `return Ok(…)` hat. Eine
    // Messung an jeder Stelle wäre sieben Stellen, die man beim Umbau
    // vergisst — und eine vergessene Stelle ist eine Anzeige, in der die
    // Zeit fehlt, ohne dass es auffaellt.
    //
    // GEMESSEN, warum auch im **Fehlerfall** gemessen wird: ein Fehlschlag
    // ist ein Ergebnis. Ohne diese Zeile zeigte ein DNS-Fehler „— ms", und
    // das liest sich wie ein Erfolg mit unbekannter Dauer.
    let start = std::time::Instant::now();
    spur(&format!("anfrage {}{}", request.host, request.path));
    let ergebnis = hole_und_sende_innen(request, limits, sitzung, eigener_speicher);
    spur("anfrage fertig");
    let bisher = phasen();
    phasen_abschliessen(Phasen {
        erstes_byte: bisher.and_then(|a| a.erstes_byte),
        gesamt: Some(start.elapsed()),
        ..Phasen::leer()
    });
    ergebnis
}

/// Der Rumpf von [`hole_und_sende`] — siehe dort, warum es eine Hülle gibt.
/// added 04.10.2026, Rolle NETZTEMPO. **`erlaubt_bedingt` ist der Schalter des
/// Auftrags (B)**: beim zweiten Durchgang wird **ohne** `If-None-Match` und
/// **ohne** `If-Modified-Since` geholt.
///
/// **WARUM EIN SCHALTER und keine Rekursion:** der Rumpf hat eine
/// **Schleife** fuer Umleitungen, und `sitzung` ist `Option<&mut dyn …>` —
/// ein `hole_und_sende_innen(… , sitzung, …)` **innerhalb** der Schleife waere
/// ein „value moved in previous iteration", also **nicht baubar**. Der
/// Schalter loest es ohne Bewegung: die zweite Runde ist einfach eine
/// Schleifenrunde mehr.
/// Release 10.10.2026 (Diagnose): `NARSIL_NET_SPUR=1` druckt Zeitmarken des
/// Anfragewegs (ms seit Prozessbeginn) nach stderr.
pub(crate) fn spur(was: &str) {
    static AN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    static NULL: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    if *AN.get_or_init(|| std::env::var_os("NARSIL_NET_SPUR").is_some()) {
        let t = NULL.get_or_init(std::time::Instant::now).elapsed();
        eprintln!("NETZSPUR {:>9.1} ms {was}", t.as_secs_f64() * 1000.0);
    }
}

fn hole_und_sende_innen(
    request: &Request,
    limits: Limits,
    mut sitzung: Option<&mut dyn sitzung::Sitzungszustand>,
    eigener_speicher: Option<&narsil_tls::kette::Vertrauensspeicher>,
) -> Result<Response, NetError> {
    hole_und_sende_runden(request, limits, sitzung, eigener_speicher, true)
}

/// DER RUMPF, und die **fuenfte** Spalte ist der ganze Auftrag (B).
/// added 04.10.2026, Rolle NETZTEMPO. **EINE VERBINDUNG VORAB AUFBAUEN, ohne
/// eine Anfrage zu senden** — das ist **Vorwaermen** (*preconnect*), und es ist
/// die Antwort auf die Zahl, an der der Kaltstart klebt.
///
/// **DIE GEMESSENE URSACHE (Release, `tests/ladezeit_mess.rs::tls_phase_je_host`):**
/// die TLS-Phase kostet **133 bis 543 ms je Host**, bei einem TCP-Roundtrip von
/// **16–25 ms**. Sechs Arbeiter auf einem **kaltem** Pool zahlen diese Zeit
/// **sechsmal** — das ist der ganze Unterschied zwischen **3244 ms** (kalt) und
/// **1079 ms** (warm) in `parallel_gegen_nacheinander_mit_zeitstempeln`.
///
/// **WAS DIESE FUNKTION TUT — und was ausdruecklich nicht:** sie holt eine
/// Verbindung aus dem Pool **oder** baut sie auf und **legt sie sofort
/// zurueck**. Sie schickt **keine** Anfrage. Das ist der Unterschied zu
/// `parallel::vorab_laden`, das Aufgaben **ausfuehrt**: hier steht am Ende nur
/// der **Sockel**.
///
/// **UND WARUM SIE HIER STEHT und nicht in `parallel.rs`:** sie braucht
/// `verbinde_neu` (DNS + TLS + `TlsAngaben`) — und das ist **derselbe Weg**,
/// den ein echter Abruf geht. Eine zweite, kuerzere Variante wuerde an einer
/// Stelle abweichen, an der es niemand mehr nachsehen kann.
///
/// **UND WER DAS RUFT, ist der Aufrufer**: hier wird **nichts** automatisch
/// gewaermt, weil ein Aufruf ohne Wissen ueber die kommenden Hosts **Raten**
/// waere. „Am Ende der Seite aufraeumen" ist eine **Lebensdauer**-Frage, die
/// der Aufrufer kennt und wir nicht.
///
/// # ⚠️ **STAND 04.10.2026: GEMESSEN NICHT FUNKTIONSFAEHIG — NICHT BENUTZEN**
///
/// `werkzeuge/bau.sh narsil-net test --release --test ladezeit_mess
/// vorwaermen_bringt_die_tls_zeit_aus_der_luecke -- --nocapture` gibt
/// ```
/// WAERMEN ohne Vorwaermen: HTML 706 ms, 16 Ressourcen 2192 ms, 0 Fehler
/// paniced: Seite: HeaderUnvollstaendig
/// ```
/// **DIE URSACHE IST NICHT BENANNT.** Die Vorfahrt wird **konditional**
/// gesendet (`ist_h2_gewahlt()`), und **zwei** Dinge koennen sie auslassen:
/// die ALPN-Auskunft **vor** dem Auslesen, oder der Pool-**Zustand**
/// (`naechster_strom`) passt nicht zum gesendeten Strom. Die Spur-Notizen
/// `WAERMEN: h2=…` und `WAERMEN: Vorfahrt gesendet` stehen **an beiden Stellen**
/// und beantworten die Frage in **einem** Lauf.
///
/// **WARUM DIE FUNKTION TROTZDEM STEHEN BLEIBT:** der **Befund** ist mehr wert
/// als die bequeme Loeschung — eine vorgewaermte h2-Verbindung braucht
/// **zwei** Zustaende, die an **der Verbindung** haengen (Vorfahrt und
/// naechster Strom), und die der Aufrufer sonst nicht sieht. **Warum sie
/// nicht benutzt werden darf:** sie ist **gemessen** kaputt, und wer eine
/// **nicht** funktionierende oeffentliche Funktion benutzt, zahlt mit
/// `HeaderUnvollstaendig`.
/// added 04.10.2026, Rolle NETZTEMPO. **DAS VORWAERMEN, DAS AUFFUELLT** — und
/// es ist die direkte Antwort auf die Messung aus Bericht 24:
///
/// ```
/// WAERMEN mit Vorwaermen: Vorwaermen 0 ms bei 6 Versuchen
/// H2 * WAERMEN: Merkliste-h2=true strom=3   (dann 5, dann 7, dann 9)
/// ```
///
/// **GEMESSEN, warum `waerme_auf` nichts leistet:** es leiht die **vorhandene**
/// Verbindung aus dem Pool und zaehlt den Strom hoch. Sechs Aufrufe, **eine**
/// Verbindung. Der Planer (`MAX_PRO_HOST = 6`) nimmt die aus dem Pool und
/// **baut die anderen fuenf trotzdem** — also bringt ein Vorwaermen, das nichts
/// baut, **genau nichts**.
///
/// **DER UNTERSCHIED ZU `waerme_auf`, und er ist der ganze Auftrag:** hier wird
/// **bis `n` aufgefüllt** — fehlen Verbindungen, werden sie **gebaut**. Jede
/// gebaute Verbindung geht **neu** in den Pool, und der Pool **entscheidet**
/// (Merkliste, Bericht 24), ob die Vorfahrt rausgeht.
///
/// **RÜCKGABE:** die Anzahl der Verbindungen, die der Pool danach **wirklich**
/// für diesen Host hält — nicht die Zahl der **Versuche**. Das ist der
/// Unterschied zwischen „wir haben es versucht" und „es ist da".
pub fn waerme_auf_n(request: &Request, limits: &Limits, n: usize) -> Result<usize, NetError> {
    let schluessel = pool_schluessel_of(request);
    //
    // added 04.10.2026: **DIE MERKLISTE, und `None` heisst wieder „nichts
    // tun".** Ein Auffüllen ohne bekanntes Protokoll würde die Vorfahrt an eine
    // h1-Verbindung schicken — dieselbe Regel wie in `waerme_auf`.
    if pool::protokoll_ist_h2(&schluessel.host).is_none() {
        h2geteilt::spur_notiz(&format!(
            "AUFFUELLEN: host={} Protokoll unbekannt -> nichts gebaut",
            schluessel.host
        ));
        return Ok(0);
    }
    if n == 0 {
        return Ok(pool::verbindungen_fuer(&schluessel));
    }
    let mut gebaut = 0usize;
    // **SOLANGE** der Pool unter `n` liegt — nicht `for _ in 0..n`, denn `n`
    // heisst hier „so viele sollen **am Ende** da sein", nicht „so viele
    // Versuche".
    while pool::verbindungen_fuer(&schluessel) < n {
        let frist = Frist::neu(limits.gesamt);
        // **DER NEUAUFBAU, direkt** — nicht `hole_oder_verbinde_mit_strom`,
        // denn das **leihen** genau das, woran es hier scheitern soll.
        let leitung = verbinde_neu(request, limits, &frist, None)?;
        // **`naechster_strom = 0`**, weil das eine **frische** Verbindung ist:
        // der Pool schickt ihr dann die Vorfahrt (Merkliste), und sie geht mit
        // Strom 1 in den naechsten Abruf.
        pool::lege_ab_mit_zaehler(
            &schluessel,
            leitung,
            0,
            crate::hpack::DynamischeTabelle::neu(),
        );
        gebaut += 1;
        //
        // added 04.10.2026: **DER RISS ZWISCHEN „wir wollen n" und „der Pool
        // nimmt nicht mehr".** Baut der Pool **nicht** (Grenze `MAX_PRO_HOST`
        // erreicht), dann dreht diese Schleife sonst endlos. **Ein gemessener
        // Riss wird gestoppt, ein blinder nicht** — und `gebaut` bricht auch
        // ab, wenn `lege_ab_mit_zaehler` den Eintrag **nicht** uebernommen hat.
        if gebaut >= pool::MAX_PRO_HOST || pool::verbindungen_fuer(&schluessel) == 0 {
            h2geteilt::spur_notiz(&format!(
                "AUFFUELLEN: host={} nach {gebaut} bauten abgebrochen (Pool nimmt nicht mehr)",
                schluessel.host
            ));
            break;
        }
    }
    Ok(pool::verbindungen_fuer(&schluessel))
}

pub fn waerme_auf(request: &Request, limits: &Limits) -> Result<(), NetError> {
    let frist = Frist::neu(limits.gesamt);
    let schluessel = pool_schluessel_of(request);
    //
    // added 04.10.2026, Rolle NETZTEMPO (Auftrag der Leitung: „Vorwaermen NUR
    // fuer Hosts, deren Protokoll aus einem frueheren Abruf bekannt ist").
    // **DIE MERKLISTE, und sie ist ein `match` mit drei Ausgaengen** — der
    // dritte ist der Auftrag:
    //
    //   * `Some(true)`  -> h2 **gemessen**  -> Vorfahrt schicken, Verbindung
    //     ist danach **brauchbar**.
    //   * `Some(false)` -> h1 **gemessen** -> keine Vorfahrt; die Verbindung
    //     ist danach **brauchbar**.
    //   * `None`        -> **wir wissen es nicht, und wir raten nicht.** Kein
    //     Verbindungsaufbau. Der Host wird **beim naechsten echten Abruf**
    //     gemerkt, und **der** holt sich die Verbindungen.
    //
    // **WARUM NICHT „meistens h2":** eine Vorfahrt auf einer h1-Verbindung ist
    // Muell im Strom, und die Antwort ist ein Fehler statt einer Seite. Ein
    // Raten an dieser Stelle kostet **beides** — und der Nutzer sieht den
    // Fehler, nicht das Raten.
    let gemessen_h2 = match pool::protokoll_ist_h2(&schluessel.host) {
        Some(h2) => h2,
        None => {
            h2geteilt::spur_notiz(&format!(
                "WAERMEN: host={} **Protokoll unbekannt** -> nichts getan (kein Raten)",
                schluessel.host
            ));
            return Ok(());
        }
    };
    h2geteilt::spur_notiz(&format!(
        "WAERMEN: host={} Protokoll aus der Merkliste: h2={gemessen_h2}",
        schluessel.host
    ));
    // added 04.10.2026: **`hole_oder_verbinde_mit_strom` mit einem Neuaufbau** —
    // derselbe Aufruf wie in `hole_und_sende_runden`, und das ist der Punkt:
    // **ein** Weg zum Verbindungsaufbau, nicht zwei.
    //
    // added 04.10.2026: **ohne `mut`** — der Aufrufer schickt die Vorfahrt nicht
    // mehr selbst (das macht der Pool), und die Leitung wird danach nur noch
    // **verschoben**. `let mut` stand hier fuer die alte Form und ist jetzt
    // eine Warnung.
    let ausleihe = pool::hole_oder_verbinde_mit_strom(&schluessel, || {
        verbinde_neu(request, limits, &frist, None)
    })?;
    //
    // added 04.10.2026: **DIE VORFAHRT, und ohne sie ist die ganze Funktion ein
    // Fehler — GEMESSEN:** `Seite: HeaderUnvollstaendig`.
    //
    // `hole_und_sende_runden` schickt die h2-Vorfahrt **nur** bei
    // `verbindung_neu`. Eine vorgewaermte Verbindung ist aber **gepoolt** — der
    // naechste Abruf ueberspringt die Vorfahrt also, und der Server hat
    // **nie** `PRI * HTTP/2.0` gesehen. Antwort: kein Status, kein Body —
    // `HeaderUnvollstaendig`.
    //
    // **DIE REGEL, und sie ist allgemein:** **eine gepoolte h2-Verbindung muss
    // ihre Vorfahrt gesendet haben.** Sie ist kein Detail des Abrufs, sondern
    // **Zustand der Verbindung** — und wer eine Verbindung in den Pool legt,
    // uebernimmt die Verantwortung fuer diesen Zustand. Deshalb steht die
    // Vorfahrt in `pool::lege_ab_mit_zaehler`, **nicht** hier.
    //
    // added 04.10.2026: **DIE ENTSCHEIDUNG IST DIE GEMESSENE, nicht die
    // abgefragte.** `is_h2_gewahlt()` ist **hier** (GEMESSEN, Bericht 23)
    // `false` — die Auskunft ist vor dem ersten Rahmentausch nicht sichtbar.
    // Also fragt der Einleger die **Merkliste**, und der Einleger im Pool
    // (`h2_vorfahrt_senden`) bekommt dieselbe Antwort ueber
    // `pool::protokoll_ist_h2`.
    h2geteilt::spur_notiz(&format!(
        "WAERMEN: host={} Merkliste-h2={gemessen_h2} strom={}",
        request.host, ausleihe.strom
    ));
    //
    // added 04.10.2026: **DIE VORFAHRT SCHICKT DER POOL, nicht dieser Aufrufer.**
    // Zwei Orte fuer dieselbe Sache hiessen: **doppelt** gesendet (der Server
    // sieht zwei Vorfahrte) oder **gar nicht** (wenn der Aufrufer es
    // vergisst). `lege_ab_mit_zaehler` fragt dieselbe Merkliste und weiss
    // dasselbe — **eine** Stelle, eine Entscheidung.
    h2geteilt::spur_notiz(&format!(
        "WAERMEN: host={} Merkliste-h2={gemessen_h2} strom={} -> Vorfahrt entscheidet der Pool",
        request.host, ausleihe.strom
    ));
    //
    // added 04.10.2026: **STROM UND HPACK-TABELLE MITGEBEN**, und das ist nicht
    // Kosmetik: `lege_ab_mit_zaehler` braucht den **naechsten Strom** der
    // Verbindung und ihre **HPACK-Tabelle** — beides gehoert zur Verbindung und
    // wandert **mit** ihr (dieselbe Regel wie beim Ausleihen). Ohne sie nuende
    // der Aufrufer (der Uebersetzer: „takes 4 arguments but 2 were supplied")
    // die Verbindung mit **falschem** Strom zurueck, und der naechste Abruf
    // schickt einen **HEADERS auf einem geschlossenen Strom** — ein
    // `PROTOCOL_ERROR` nach RFC 9113 5.1.
    pool::lege_ab_mit_zaehler(
        &schluessel,
        ausleihe.leitung,
        ausleihe.strom,
        ausleihe.hpack,
    );
    Ok(())
}

/// Release 08.10.2026: EIN SPRUNG — die erste Antwort, auch wenn sie eine
/// Umleitung ist. Fuer das Aufloesen von Kurzlinks per `HEAD` (Features 7
/// und 12): der `Location`-Kopf wird gelesen, ohne der Kette zu folgen.
/// Netzregel und Kopfzeilen-Regeln gelten wie bei jedem Abruf.
pub fn fetch_ein_sprung(request: &Request, limits: Limits) -> Result<Response, NetError> {
    hole_und_sende_runden_mit(request, limits, None, None, true, true)
}

fn hole_und_sende_runden(
    request: &Request,
    limits: Limits,
    sitzung: Option<&mut dyn sitzung::Sitzungszustand>,
    eigener_speicher: Option<&narsil_tls::kette::Vertrauensspeicher>,
    erlaubt_bedingt: bool,
) -> Result<Response, NetError> {
    hole_und_sende_runden_mit(request, limits, sitzung, eigener_speicher, erlaubt_bedingt, false)
}

fn hole_und_sende_runden_mit(
    request: &Request,
    limits: Limits,
    mut sitzung: Option<&mut dyn sitzung::Sitzungszustand>,
    eigener_speicher: Option<&narsil_tls::kette::Vertrauensspeicher>,
    erlaubt_bedingt: bool,
    nur_ein_sprung: bool,
) -> Result<Response, NetError> {
    let frist = Frist::neu(limits.gesamt);
    // DER EINE ORT, an dem `aktuell` entsteht und von dort an verfuegbar
    // ist. Fuer HSTS wird es **vor** der Schleife angehoben, weil die
    // Anhebung den Host und den Port aendert.
    let mut aktuell = request.clone();
    if let Some(s) = sitzung.as_deref_mut() {
        hsts_anheben(s, &mut aktuell);
    }
    // added 02.10.2026 11:0x UTC (Projektlabel 02.10.2026), Rolle TLS.
    // Runde 8 S8: **DER `REFERER` BEI EINER UMLEITUNG**, und die drei Zeilen hier
    // sind die ganze Regel:
    //
    // added 02.10.2026 11:0x: **DIE QUELLE BLEIBT DIE DES URSPRUNGSDOKUMENTS.**
    // Sie wird **einmal** vor der Schleife gelesen und in jeder Runde benutzt —
    // nicht der vorherige Sprung. Grund ist die Spezifikation, 8.1 heißt die
    // Stelle „Set request's referrer policy on redirect“: auf einer Kette
    // A → B → C traegt die Anfrage an C die Herkunft des **Dokuments**, nicht
    // die von B. Waere es anders, wuerde jede zwischenliegende Adresse an den
    // letzten Server weitergegeben — genau das, was die Richtlinie verhindern
    // soll, und bei `no-referrer` waere es ein Datenleck gegen die Absicht des
    // Dokuments.
    //
    // added 02.10.2026 11:0x: **UND DIE RICHTLINIE WIRD PRO ANTWORT NEU
    // GESETZT**, weil 8.1 genau das sagt; ein unbekannter Wert laesst die
    // geltende Richtlinie **stehen** (11.1), und fehlt der Kopf ganz, gilt die
    // Vorgabe (3.7).
    let referrer_quelle = aktuell.headers.get("referer").cloned();
    let mut geltende_richtlinie = REFERRER_VORGABE;
    let mut umleitungen: u32 = 0;
    loop {
        // ------------------------------------------------------------
        // W7 / N5, SCHRITT 1: DER CACHE. Drei Faelle, und jeder hat
        // eine eigene Antwort:
        // ------------------------------------------------------------
        let mut bedingt = false;
        if !erlaubt_bedingt {
            //
            // added 04.10.2026, Rolle NETZTEMPO. **DIE ZWEITE RUNDE OHNE
            // BEDINGUNG.** Der Server hat 304 geantwortet, aber es gab nichts
            // zum Wiederverwenden (der Eintrag wurde entfernt/verdraengt). Die
            // richtige Antwort ist ein **unbedingtes** GET — und die Kopfzeilen
            // werden **entfernt**, nicht nur nicht gesetzt: ein vom Aufrufer
            // selbst gesetztes `If-None-Match` (etwa aus einem
            // `narsil-download`-Aufruf heraus) wuerde sonst **stehen bleiben** und
            // wir bekamen **wieder** 304.
            aktuell.headers.remove("if-none-match");
            aktuell.headers.remove("if-modified-since");
        }
        if let Some(Fund::Frisch(a)) = cache::nachschlagen(&aktuell) {
            // FRISCH: ohne Netz. Das ist der ganze Zweck.
            let mut antwort = cache::response_aus(&a, &url_aus(&aktuell));
            entpacke(&mut antwort, limits.entpackt)?;
            return Ok(antwort);
        }
        if let Some(Fund::Veraltet {
            etag,
            last_modified,
        }) = cache::nachschlagen(&aktuell)
        {
            // VERALTET: bedingte Anfrage. RFC 9111 Abschnitt 4.3.2,
            // `If-None-Match` aus dem `ETag` und `If-Modified-Since`
            // aus `Last-Modified`.
            //
            // DER GRUND, DASS DER ZWEITE `nachschlagen`-Aufruf EIN
            // EIGENER IST statt eines `if let ... else if`: `Fund` ist
            // nicht `Copy`, und ein zweites Nachschlagen kostet
            // nichts, weil der Speicher der Cache selbst ist.
            if let Some(e) = etag {
                aktuell.headers.insert("if-none-match".into(), e);
            }
            if let Some(e) = last_modified {
                aktuell.headers.insert("if-modified-since".into(), e);
            }
            bedingt = erlaubt_bedingt;
        }
        // W7 / N6, SCHRITT 2: COOKIES IN DEN KOPF.
        //
        // DER WEG UND SEINE BEGRUENDUNG: der Kopf entsteht **vor**
        // `sende`, also genau einmal und genau fuer diese Anfrage. Ein
        // `Header` in `Request` waere zurueckstellbar, aber dann
        // muesste `fetch_mit` ihn nach der Antwort wieder entfernen —
        // und bei einer Umleitung mit `umleitung_ziel` waere die
        // Reihenfolge von `headers.clone()` und dem Setzen nicht mehr
        // an einer Stelle nachzuvollziehen.
        if let Some(s) = sitzung.as_deref_mut() {
            let sicher = ist_https(&aktuell);
            if let Some(kopf) = s.cookies_fuer_anfrage(
                host_ohne_schema(&aktuell.host),
                &aktuell.path,
                sicher,
                jetzt_als_u64(),
            ) {
                // DER KOPF WIRD **ERSETZT**, nicht ergaenzt: ein
                // Aufrufer, der selbst `Cookie:` gesetzt hat, will das
                // sehen, was er geschrieben hat.
                if !aktuell.headers.contains_key("cookie") {
                    aktuell.headers.insert("cookie".to_string(), kopf);
                }
            }
        }

        // Release 08.10.2026: NETZREGEL, KOPFZEILEN-REGELN UND GPC (`haken`)
        // — hier, weil jeder Abruf und jeder Sprung durch diese Zeile geht.
        // Gesendet wird eine angepasste Kopie; `aktuell` bleibt, wie der
        // Aufrufer es wollte (Cache-Schluessel, naechster Sprung).
        let angepasst = haken::vor_dem_senden(&aktuell)?;
        let mut antwort = sende(
            angepasst.as_ref().unwrap_or(&aktuell),
            &limits,
            &frist,
            eigener_speicher,
        )?;
        haken::nach_dem_empfang(&aktuell, &mut antwort);

        // W7 / N6, SCHRITT 3: WAS DIE ANTWORT GEMELDET HAT. Das steht
        // HIER und nicht erst nach der Umleitung: der Auftrag sagt
        // ausdruecklich „Set-Cookie (auch bei Umleitungen) zurueckmelden",
        // und eine 302 mit `Set-Cookie` ist die Regel, nicht die
        // Ausnahme (Session-Fixation, Zustand im Redirect).
        if let Some(s) = sitzung.as_deref_mut() {
            antwort_gegen_stand_melden(s, &aktuell, &antwort);
        }

        // W7 / N5, SCHRITT 2: WAS MIT DER ANTWORT GESCHIEHT.
        //
        // DER 304-FALL ZUERST, weil er der einzige ist, in dem der
        // Koerper aus dem Cache kommt statt aus dem Netz. RFC 9111
        // Abschnitt 4.3.4: der Client benutzt den gespeicherten
        // Inhalt und aktualisiert den Eintrag. `cache::ablegen`
        // erledigt die Aktualisierung, weil `Antwort.status == 304`
        // dort genau so behandelt wird.
        if bedingt && antwort.status.code == 304 {
            cache::ablegen(&aktuell, &antwort);
            if let Some(a) = cache::gespeichert(&aktuell) {
                let mut aus_cache = cache::response_aus(&a, &url_aus(&aktuell));
                // DER HEADER STAMMT VOM FRISCHEN 304, der KOERPER AUS
                // DEM CACHE. Genau das ist der Sinn von 4.3.4: die
                // Validatoren des Servers sind juenger als die
                // gespeicherten, der Inhalt aber derselbe.
                for (k, v) in antwort.headers.iter() {
                    aus_cache.headers.insert(k.clone(), v.clone());
                }
                entpacke(&mut aus_cache, limits.entpackt)?;
                return Ok(aus_cache);
            }
            //
            // added 04.10.2026, Rolle NETZTEMPO. **OHNE GESPEICHERTEN INHALT
            // GIBT ES ZWEI AUSGAENGE, und der erste ist der Auftrag (B):**
            //
            // GEMESSEN (Bericht 16, `tests/cache_nachweis.rs`): der Server
            // antwortet **304**, im Cache liegt **kein** Eintrag — er wurde
            // zwischen bedingter Anfrage und Antwort entfernt (im Browser:
            // **Verdraengung**). Seriell 3 von 3 gruen (5000 B), **parallel
            // 0 B**.
            //
            // **DIE FRUEHERE ENTSCHEIDUNG war falsch:** der 304 wurde
            // durchgereicht, und der Aufrufer bekam eine **leere** Antwort
            // fuer eine Ressource, die es gibt. Das ist Datenverlust fuer den
            // Nutzer — eine leere Seite. RFC 9111 4.3.4 setzt fuer 304 einen
            // **gespeicherten** Inhalt voraus; gibt es den nicht, ist 304
            // keine Antwort, sondern ein **Fehler des Servers**.
            //
            // **DARUM: eine Runde weiter, unbedingt.** `umleitungen` wird
            // **nicht** erhoeht: es war **kein** Umprung, sondern derselbe
            // Abruf ohne Bedingung.
            if erlaubt_bedingt {
                //
                // added 04.10.2026: **`sitzung.take()`, und der Uebersetzer hat
                // mich darauf gestochen.** Mit `sitzung.as_deref_mut()` kam
                //     error[E0597]: `sitzung` does not live long enough
                // weil die **neue** Leihabe die lokale `sitzung` ueberlebt —
                // `take()` **betruebt** sie, es gibt nichts mehr, das
                // auslaufen koennte. Und es ist **sicher**, weil direkt danach
                // `return` steht: die Runde nimmt die Sitzung mit und der
                // urspruengliche Aufrufer bekommt sie nicht zurueck. Das ist
                // bei einer **Sitzung mit Cookies/HSTS** die richtige Form:
                // die zweite Runde **darf** sie benutzen.
                return hole_und_sende_runden_mit(
                    request,
                    limits,
                    sitzung.take(),
                    eigener_speicher,
                    false,
                    nur_ein_sprung,
                );
            }
            //
            // **DIE ZWEITE RUNDE, und der Server sendet wieder 304.** Dann ist
            // es kein Wettlauf mehr, sondern ein Server, der auf eine
            // **unbedingte** Anfrage mit 304 antwortet — das ist nach
            // RFC 9110 15.4.5 ein Protokollfehler. Und der Auftrag sagt
            // ausdruecklich: **nie** einen 304-Body an den Aufrufer geben,
            // wenn er keine bedingte Anfrage gestellt hat.
            return Err(NetError::InvalidResponse(format!(
                "der Server antwortete 304 ohne bedingte Anfrage ({})",
                url_aus(&aktuell)
            )));
        }

        //
        // added 04.10.2026, Rolle NETZTEMPO. **304 OHNE BEDINGTE ANFRAGE** —
        // und das ist die **zweite Haelfte** des Auftrags (B). Die erste
        // Haelfte war „304 **mit** bedingter Anfrage, aber ohne gespeicherten
        // Inhalt" (siehe oben); diese hier ist die andere.
        //
        // GEMESSEN (`tests/cache_nachweis.rs`, `server_antwortet_immer_304…`):
        //     1. Anfrage code=Ok(304) len=Ok(0)
        //     Zaehler {"anfragen": 1, "if-none-match": 0, "if-modified-since": 0,
        //              "304": 1}
        // Der Server hat **einer unbedingten** Anfrage mit 304 geantwortet, und
        // der Aufrufer bekam eine **leere** Antwort mit Code 304. Ein Browser
        // zeigt damit eine **leere Seite** fuer ein Skript, das existiert.
        //
        // **WARUM HIER EIN FEHLER und nicht ein Neuversuch:** der Neuversuch
        // waere **unbedingt** (dieselbe Anfrage), und ein Server, der
        // **immer** 304 antwortet, wuerde uns in eine **Schlinge** schicken.
        // RFC 9110 15.4.5 nennt das einen Protokollfehler, und ein
        // Protokollfehler des Servers wird **gemeldet**, nicht durch Raten
        // uebergangen.
        //
        // **UND VOR `cache::ablegen`, weil man einen 304 nicht ablegen darf:**
        // `ablegen` wuerde `aktualisiere_304` rufen und damit einen Eintrag
        // **erneuern**, den niemand angefragt hat — aus einem Serverfehler
        // wuerde so eine **falsche** Frische.
        if !bedingt && antwort.status.code == 304 {
            return Err(NetError::InvalidResponse(format!(
                "der Server antwortete 304 auf eine **unbedingte** Anfrage ({})",
                url_aus(&aktuell)
            )));
        }

        if !antwort.is_redirect() {
            // DER CACHE, SCHRITT 3: ablegen. VOR `entpacke`, weil der
            // Cache den Koerper so speichert, wie er auf der Leitung
            // war (siehe `cache::antwort_aus`).
            cache::ablegen(&aktuell, &antwort);
            // DER BODY WIRD HIER ENTPACKT, bevor er zurueckkommt (siehe
            // `entpacke`). Alles andere — Umleitungen, Fehler, 204 — geht
            // unberuehrt durch.
            let mut antwort = antwort;
            entpacke(&mut antwort, limits.entpackt)?;
            return Ok(antwort);
        }
        // Eine Umleitung wird **nicht** abgelegt, obwohl RFC 9111 sie
        // (301, 308) fuer cachebar haelt. DER GRUND: der Cache-Status
        // gehoert zu `fetch_mit`, und ein 301 im Cache wuerde die
        // Umleitungsschleife um einen Sprung kuerzer machen als der
        // Aufrufer mit `Limits::umleitungen` vorgegeben hat. Das ist
        // eine Verhaltensaenderung gegenueber vorher, die man nur
        // bewusst treffen kann.
        //
        // Release 08.10.2026: `fetch_ein_sprung` will die Umleitung selbst.
        if nur_ein_sprung {
            return Ok(antwort);
        }
        if umleitungen >= limits.umleitungen {
            // GEMESSENER FEHLER, 29.09.2026: die erste Fassung gab hier
            // die 302-Antwort zurueck. Der Benutzer sah dann eine leere
            // Seite statt "zu viele Umleitungen". Der Test
            // `umleitung_auf_sich_selst_hoert_auf_und_meldet_die_grenze`
            // war genau deshalb rot.
            return Err(NetError::ZuVieleUmleitungen {
                grenze: limits.umleitungen,
            });
        }
        let ziel = match antwort.redirect_location() {
            Some(l) => l.to_string(),
            // Umleitung ohne Location ist ein Fehler des Servers, kein
            // Grund fuer den Browser, still zu bleiben.
            None => return Ok(antwort),
        };
        // GESAMT-Zeitlimit auch zwischen zwei Spruengen pruefen: eine
        // Kette aus 10 Umleitungen kann zusammen laenger dauern als
        // jede einzelne fuer sich.
        frist.pruefen()?;
        aktuell = umleitung_ziel(&aktuell, antwort.status.code, &ziel)?;
        // DER SPRUNG KANN AUF EINEN ANDEREN HOST GEHEN, und der kann
        // sein eigener HSTS-Host sein. Ohne diese Zeile wuerde die
        // naechste Anfrage im Klartext laufen, obwohl der Browser
        // weiss, dass der neue Host https verlangt.
        if let Some(s) = sitzung.as_deref_mut() {
            hsts_anheben(s, &mut aktuell);
        }
        umleitungen += 1;
        // added 02.10.2026 11:1x UTC (Projektlabel 02.10.2026), Rolle TLS:
        // **UND JETZT DER WERT**, mit der Regel aus RFC 9110 15.4, Schritt 5:
        // „Resource-specific header fields, including (but not limited to)
        // Referer, Origin, Authorization, and Cookie“ sind bei einer Umleitung
        // zu **aktualisieren**. Wir rechnen ihn aus Quelle, neuem Ziel und
        // geltender Richtlinie neu — und **entfernen** ihn, wenn die Richtlinie
        // nichts zulaesst. Ein stehen gebliebener `Referer` waere das
        // Gegenteil: ein Kopf, den der Sprung nicht mehr begruendet.
        referer_setzen(
            &mut aktuell,
            referrer_quelle.as_deref(),
            geltende_richtlinie,
        );
        // added 02.10.2026 11:1x: **UND DIE RICHTLINIE FUER DEN NAECHSTEN
        // SPRUNG** aus dieser Antwort. GEMESSEN an der Reihenfolge: das kommt
        // **nach** der Berechnung oben, denn die Antwort gehoert zum Sprung, der
        // gerade gemacht wurde, nicht zum folgenden.
        if let Some(wert) = antwort.headers.get("referrer-policy") {
            if let Some(gefunden) = referrer_richtlinie_gefunden(Some(wert)) {
                geltende_richtlinie = gefunden;
            }
        }
        // `bedingt` wird hier bewusst NICHT uebertragen: die bedingte
        // Anfrage galt dem alten Ziel, und der Sprung ist eine neue
        // Anfrage mit eigenem Cache-Eintrag.
    }
}

/// added 02.10.2026 11:1x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// **SETZT ODER ENTFERNT DEN `REFERER`** — die vier Zeilen, die die
/// Umleitungsschleife braucht, als **eigene Funktion**.
///
/// added 02.10.2026 11:1x: **WARUM EIGENE FUNKTION**, und das ist keine Form:
/// in der Schleife waeren es vier Zeilen, die man beim Lesen uebersieht und im
/// Test nicht erreichen kann. `narsil-net/tests/` gehoert einer anderen Rolle und
/// die Crate hat **keinen eigenen** Testserver (GEMESSEN: keine `TcpListener::
/// bind` in `crates/narsil-net/src/*.rs`), also gibt es keinen Weg, einen echten
/// 302-Sprung hier zu stellen. Mit dieser Funktion prueft der Test genau den
/// Weg, den der Betrieb nimmt — und was nicht getestet ist, sind die zwei
/// Zeilen Aufruf in der Schleife, nicht die Entscheidung.
///
/// added 02.10.2026 11:1x: **OHNE QUELLE WIRD NICHTS GETAN**, und das ist der
/// Fall „der Benutzer hat die Adresse getippt": es gab kein Dokument, also gibt
/// es keinen `Referer`. Ein Kopf, den es vorher nicht gab, wird hier **nicht**
/// erfunden.
fn referer_setzen(anfrage: &mut Request, quelle: Option<&str>, richtlinie: ReferrerPolitik) {
    let quelle = match quelle {
        Some(q) => q,
        None => return,
    };
    let ziel_url = anfrage_url(anfrage);
    match referrer_berechnen(richtlinie, quelle, &ziel_url) {
        Some(wert) => {
            anfrage.headers.insert("referer".to_string(), wert);
        }
        None => {
            anfrage.headers.remove("referer");
        }
    }
}

/// added 02.10.2026 11:0x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// **DIE URL EINER ANFRAGE ALS TEXT**, und sie ist keine Erfindung: `Request`
/// fuehrt Schema, Host, Port und Pfad **getrennt**, und die Referrer-Regeln
/// brauchen genau diese vier. Es gibt hier bewusst keinen Aufruf von `narsil-url`
/// (ADR-030: keine Fremd-Crate in `narsil-net`), sondern die Zusammensetzung aus
/// denselben Feldern, aus denen auch die Anfragezeile entsteht (siehe
/// `host_mit_port`) — **eine** Stelle, die entscheidet, wie eine URL hier
/// aussieht.
fn anfrage_url(r: &Request) -> String {
    let mut s = String::with_capacity(r.host.len() + r.path.len() + 9);
    s.push_str(if ist_https(r) { "https://" } else { "http://" });
    s.push_str(&r.host_mit_port());
    s.push_str(&r.path);
    s
}

/// Die Frist: EIN Gesamt-Zeitlimit fuer alles, mit Restzeit-Rechnung fuer
/// die einzelnen Operationen. Ohne das haengt ein langsamer Server, der
/// jede Sekunde ein Byte schickt, ewig.
struct Frist {
    ende: Instant,
    grenze: Duration,
}

impl Frist {
    fn neu(grenze: Duration) -> Self {
        Frist {
            ende: Instant::now() + grenze,
            grenze,
        }
    }

    /// Restzeit, mindestens 1 ms (0 ms wuerde `set_*_timeout` sofort
    /// feuern lassen und aus einem Timeout ein Connection-Reset machen).
    fn rest(&self) -> Result<Duration, NetError> {
        let jetzt = Instant::now();
        if jetzt >= self.ende {
            return Err(NetError::ZeitlimitGesamt {
                grenze: self.grenze,
                gebraucht: self.grenze,
            });
        }
        Ok((self.ende - jetzt).max(Duration::from_millis(1)))
    }

    /// Frist unabhaengig vom Ergebnis pruefen: auch nach einem erfolgreichen
    /// Lesevorgang zaehlt die Gesamtzeit weiter.
    fn pruefen(&self) -> Result<(), NetError> {
        let jetzt = Instant::now();
        if jetzt >= self.ende {
            return Err(NetError::ZeitlimitGesamt {
                grenze: self.grenze,
                gebraucht: self.grenze,
            });
        }
        Ok(())
    }

    /// Die Meldung fuer eine ERSCHOEPTE Frist, mit der wirklich
    /// gebrauchten Zeit. Wird an der Stelle benutzt, an der ein
    /// Lesevorgang an der Restzeit gescheitert ist — dort ist der
    /// Fehler `ZeitlimitGesamt`, aber man will trotzdem sagen, wie
    /// lange es gedauert hat.
    fn erschöpft(&self) -> NetError {
        NetError::ZeitlimitGesamt {
            grenze: self.grenze,
            gebraucht: self
                .grenze
                .saturating_sub(self.ende.saturating_duration_since(Instant::now())),
        }
    }
}

/// Zerlegt `[::1]:8080` in Host `::1` und Port 8080.
///
/// GEMESSEN am 29.09.2026, `cargo test -p narsil-net` 22:41: `Request::get
/// ("[::1]", 8080, "/")` schrieb `Host: [::1]:8080` — richtig — aber der
/// Client baute die Verbindungsadresse als `[::1]:8080` und schickte sie
/// an `to_socket_addrs`. Auf dieser Maschine ist `[::1]:8080` zwar
/// parsebar, `::1:8080` (ohne Klammern) nicht. Wer den Host OHNE
/// Klammern weitergibt, bekommt also `NetError::Name` und die Meldung
/// sieht nach einem DNS-Problem aus statt nach einem Adressproblem.
///
/// Klammern sind Teil der SYNTAX, nicht des Hostnamens. Deshalb steht
/// `host` in [`Request`] ohne Klammern (`::1`) und werden sie nur an den
/// drei Stellen wieder gesetzt, an denen das Protokoll sie verlangt:
/// `Host`-Header, absolute URI und Verbindungsadresse.
pub fn zerlege_host_mit_klammern(text: &str) -> Result<(String, Option<u16>), String> {
    if !text.starts_with('[') {
        // Kein Literal: alles nach dem ersten Doppelpunkt ist der Port.
        return match text.find(':') {
            Some(i) => {
                let (h, p) = (&text[..i], &text[i + 1..]);
                match p.parse::<u32>() {
                    Ok(n) if (1..=65535).contains(&n) => Ok((h.to_string(), Some(n as u16))),
                    _ => Err(format!("ungueltiger Port {p:?} in {text:?}")),
                }
            }
            None => Ok((text.to_string(), None)),
        };
    }
    // Literal: die schliessende Klammer muss da sein, sonst ist die
    // Adresse kaputt und NICHT "irgendwas mit Port".
    let ende = match text.find(']') {
        Some(i) => i,
        None => {
            return Err(format!("IPv6-Adresse ohne schliessende Klammer: {text:?}"));
        }
    };
    let innen = &text[1..ende];
    if innen.is_empty() {
        return Err(format!("IPv6-Adresse ohne Inhalt: {text:?}"));
    }
    // Innen muss eine IPv6-Adresse stehen: mindestens zwei Doppelpunkte
    // ODER eine Zone (`%eth0`). `h]` sieht aus wie ein Literal, ist aber
    // ein Hostname in Klammern — das lehne ich ab, weil kein Server so
    // erreichbar ist.
    if !innen.contains(':') && !innen.contains('%') {
        return Err(format!("keine IPv6-Adresse in Klammern: {text:?}"));
    }
    let rest = &text[ende + 1..];
    let port = if rest.is_empty() {
        None
    } else if let Some(p) = rest.strip_prefix(':') {
        if p.is_empty() {
            return Err(format!("Doppelpunkt ohne Port in {text:?}"));
        }
        match p.parse::<u32>() {
            Ok(n) if (1..=65535).contains(&n) => Some(n as u16),
            _ => return Err(format!("ungueltiger Port {p:?} in {text:?}")),
        }
    } else {
        return Err(format!("Zeichen nach der Klammer: {text:?}"));
    };
    Ok((innen.to_string(), port))
}

/// Host und Port als Text, mit Klammern um einen IPv6-Host.
///
/// Drei Stellen brauchen das: der `Host`-Header, die absolute URI in der
/// Anfragezeile und die Verbindungsadresse. An allen dreien ist eine
/// IPv6-Adresse ohne Klammern ungueltig — im `Host`-Header waere der
/// Kopf kaputt, und `to_socket_addrs` wuerde `::1:8080` als Zahl lesen.
pub fn socket_text(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

/// Verbindet, sendet, liest — eine Runde, ohne Umleitung.
/// Hoechste Groesse des ENTPACKTEN Bodys in Byte (M4).
///
/// GEMESSEN, warum 64 MiB: das ist die Zahl aus dem Auftrag der
/// Koordination, und sie ist die Grenze, an der ein Browser aufgibt.
/// Wichtiger: sie begrenzt das ENTPACKTE Ergebnis, nicht die
/// Uebertragung — eine 1-KiB-Bombe kann 20 GiB entpacken, und genau
/// das ist der Angriff, den diese Zahl verhindert.
pub const MAX_ENTPACKT: usize = 64 * 1024 * 1024;

/// Entpackt den Body, wenn der Server ihn komprimiert schickt.
///
/// # WARUM DAS HIER UND NICHT IM AUFRUFER
///
/// GEMESSENER BEFUND 30.09.2026 (Koordination): `narsil-net` schickt
/// `Accept-Encoding: gzip, deflate` (Zeile 321) und lieferte die
/// komprimierten Bytes mit einem benannten Fehler `Komprimiert` zurueck.
/// Der Aufrufer (`narsil`) musste also selbst entpacken — oder eben
/// nicht, und dann zeigte er dem Benutzer Muell.
///
/// Ein Browser, der die Encoding selbst anbietet, muss sie auch selbst
/// lesen. Wer es anbietet und es nicht liest, ist wie ein Kellner, der
/// „gern Perlwein" anbietet.
///
/// # DIE DREI FAELLE
///
/// * `gzip` / `x-gzip` → `narsil_compress::gunzip`
/// * `deflate`        → `narsil_compress::deflate_entpacke`
/// * `br`             → `narsil_compress::brotli_entpacken`
/// * alles andere (`zstd`, …) → `NetError::Komprimiert`, unverandert.
///
/// DIE GRENZE IST FUER ALLE DREI DIESELBE (`Limits::entpackt`, im
/// Betrieb 64 MiB) und sie begrenzt das **entpackte** Ergebnis, nicht
/// die Uebertragung. Eine 481-Byte-Datei, die 300 MiB entpackt, ist
/// genau der Angriff, den diese eine Zahl verhindert — und
/// `brotli_entpacken` meldet das Ueberschreiten als benannten Fehler
/// statt abzuschneiden.
///
/// Und: ein Body ohne `Content-Encoding` wird nicht angefasst. Ein
/// Header `identity` ist dasselbe wie keiner.
///
/// # DIE HEADER WERDEN MITGEZOGEN
///
/// `Content-Length` beschreibt den KOMPRIMIERTEN Body. Bleibt er
/// stehen, behauptet die Antwort nach dem Entpacken, sie sei 1 KiB lang,
/// und der Aufrufer schneidet bei 1 KiB ab. `ETag` und
/// `Content-MD5` beschreiben ebenfalls das Komprimierte — sie werden
/// entfernt, weil sie jetzt falsch sind. `Vary: Accept-Encoding` darf
/// bleiben: es beschreibt die Aushandlung, nicht den Inhalt.
fn entpacke(antwort: &mut Response, grenze: usize) -> Result<(), NetError> {
    let Some(kodierung) = antwort.content_encoding() else {
        return Ok(());
    };
    let kodierung = kodierung.trim().to_string();
    if kodierung.is_empty() || kodierung == "identity" {
        return Ok(());
    }
    let mut out: Vec<u8> = Vec::new();
    match kodierung.as_str() {
        "gzip" | "x-gzip" => {
            narsil_compress::gunzip(&antwort.body, &mut out, grenze)
                .map_err(|e| NetError::Komprimiert(std::format!("gzip: {e}")))?;
        }
        "deflate" => {
            narsil_compress::deflate_entpacke(&antwort.body, &mut out, grenze)
                .map_err(|e| NetError::Komprimiert(std::format!("deflate: {e}")))?;
        }
        // added 30.09.2026, Welle 7. `br` ist der dritte Zweig und
        // braucht keine eigene Sonderbehandlung: `brotli_entpacken`
        // hat genau die Signatur der beiden anderen (Daten, Ziel,
        // Grenze) und wirft bei Ueberschreitung der Grenze einen
        // benannten Fehler statt zu schweigen.
        //
        // DER FEHLER WIRD ALS „br: …" GEMELDET und nicht als „gzip: …":
        // im Text stand einmal `Komprimiert(...)` ohne Namen, und bei
        // drei Verfahren ist die Meldung ohne Namen nicht mehr
        // zuzuordnen.
        "br" => {
            narsil_compress::brotli_entpacken(&antwort.body, &mut out, grenze)
                .map_err(|e| NetError::Komprimiert(std::format!("br: {e}")))?;
        }
        // GEMESSEN, warum der Name unverändert in die Meldung geht: der
        // Benutzer muss sehen, WELCHES Encoding der Server geschickt hat
        // ("br"), nicht eine Nummer und nicht "Fehler 3".
        andere => return Err(NetError::Komprimiert(andere.to_string())),
    }
    antwort.body = out;
    antwort.entpackt = true;
    for kopf in ["content-length", "etag", "content-md5", "digest"] {
        antwort.headers.remove(kopf);
    }
    Ok(())
}

/// Verbindet und gibt die Leitung als Box.
///
/// DIE EINE STELLE, an der TLS eingebaut wird (Welle 6): hier steht
/// `TcpStream`, und ein TLS-Stream waere hier eine `Box<dyn Leitung>` mit
/// einem anderen Innenleben. Der Rest des HTTP-Pfadens kennt die
/// Verbindung nicht.
///
/// M3/M4: keine Schleife, keine Allokation von der Eingabe — die
/// Kandidatenliste entsteht in `sende` und ist durch `to_socket_addrs`
/// begrenzt.
/// added 04.10.2026, Rolle NETZTEMPO. **DIE WARTEZEIT AUF DIE NACHSCHRIFT, als
/// Schalter** — Vorgabe **250 ms**, wie sie vorher fest war.
///
/// **WARUM DAS EIN SCHALTER IST UND KEIN WERT:** die Zahl **250 ms** ist an
/// **einer** Stelle im Handshake gewachsen und nie gemessen worden. Mit
/// Schalter stehen **beide** Werte am selben Lauf zur Verfuegung — und der
/// Unterschied ist die Zahl, die in den Bericht gehoert.
///
/// **EINHEIT:** Millisekunden, `u64`. `0` heisst **nicht warten** (die
/// Nachschrift wird trotzdem genommen, wenn sie schon da ist).
///
/// **added 04.10.2026: DIE VORGABE IST JETZT `0`, und das ist die Folge der
/// Messung plus der Umsetzung durch TLS.** `0` heisst **nicht warten**, nicht
/// „Tickets abschalten": die Leseschleife nimmt sie, sobald sie ankommen.
/// **Vorher** stand hier 250 ms, und **GEMESSEN** kostete das 280–410 ms je
/// Handshake bei 5–105 ms echter Rechenzeit. **Nur mit der Leseschleife** ist
/// `0` richtig — **GEMESSEN 41 831 ms** bei `0` ms **ohne** sie, das war die
/// kaputte Zwischenfassung.
static NACHSCHRIFT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// added 04.10.2026: **SCHALTER SETZEN.**
pub fn nachschrift_frist_setzen(ms: u64) {
    NACHSCHRIFT_MS.store(ms, std::sync::atomic::Ordering::Relaxed);
}

/// added 04.10.2026: **DER WERT, DER GILT.**
pub fn nachschrift_frist() -> std::time::Duration {
    std::time::Duration::from_millis(NACHSCHRIFT_MS.load(std::sync::atomic::Ordering::Relaxed))
}

fn verbinde(
    addr: &std::net::SocketAddr,
    timeout: std::time::Duration,
    tls: Option<&TlsAngaben>,
) -> Result<Box<dyn Leitung>, NetError> {
    // added 01.10.2026 16:8x, T3. TCP und TLS werden **hier** getrennt
    // gemessen und beim Aufrufer nicht verrechnet. GEMESSEN, warum hier und
    // nicht dort: `verbinde` gibt `Box<dyn Leitung>` zurueck, und an der Box
    // laesst sich nachher nicht mehr sagen, wie viel davon Handshake war —
    // man muesste zurueckrechnen, und eine Rechnung aus dem Gesamtwert ist
    // genau die Zahl, die in einer Anzeige als „0 ms" auftaucht.
    let tcp_start = std::time::Instant::now();
    let s = TcpStream::connect_timeout(addr, timeout).map_err(|e| {
        if e.kind() == io::ErrorKind::TimedOut {
            NetError::ZeitlimitVerbinden
        } else {
            NetError::Connect(format!("{addr}: {e}"))
        }
    })?;
    let tcp_dauer = tcp_start.elapsed();
    let tls_start = std::time::Instant::now();
    match tls {
        None => {
            phasen_setzen(Phasen {
                tcp: Some(tcp_dauer),
                tls: None,
                ..Phasen::leer()
            });
            Ok(Box::new(s) as Box<dyn Leitung>)
        }
        Some(a) => {
            // DER HANDSHAKE IST TEIL DES VERBINDUNGSAUFBAUS, und nicht
            // ein spaeterer Schritt. Wer erst die HTTP-Anfrage sendet und
            // dann den Handshake macht, schickt Klartext an einen
            // Server, dessen Identitaet niemand geprueft hat — das waere
            // genau der Fehler, den TLS verhindern soll.
            //
            // `ueber_sockel` statt `TlsStrom::verbinde`: der Sockel ist
            // schon da, mit dem Zeitlimit des Aufrufers. Ein zweiter
            // `connect` waere eine zweite Namensaufloesung.
            // DIE EINE VERBINDUNGSFUNKTION, added 30.09.2026. Sie
            // sendet EIN ClientHello, das TLS 1.3 **und** TLS 1.2
            // anbietet, und laesst den Server waehlen.
            //
            // VORHER stand hier `TlsStrom::ueber_sockel`, und das ist
            // der ganze Fehler gewesen: dieser Weg kann nur TLS 1.3.
            // Gegen badssl.com kam deshalb `ServerHello erwartet` —
            // genauer: der Server antwortete mit TLS 1.2, und der
            // 1.3-Dekodierer lehnte die Nachricht ab, weil dort nicht
            // `supported_versions` mit 0x0304 stand.
            //
            // Ein zweiter Versuch nur mit TLS 1.2 waere die
            // naheliegende Reparatur und die FALSCHE: sie ist genau der
            // Downgrade-Angriff aus RFC 8446 Abschnitt 4.1.1. Deshalb
            // gibt es keinen zweiten Versuch, sondern eine Auswahl.
            // added 01.10.2026 (Uhr) / 02.10.2026 (Projektlabel), TLS R4n.
            // DIE WIEDERAUFNAHME, und vorher gab es hier nur einen Weg:
            // `ueber_sockel` **ohne** Angebot. Das heisst: kein Ticket wurde
            // angeboten und keines behalten — die Wiederaufnahme war im
            // Browser **nicht** erreichbar, egal wie sie in `narsil-tls`
            // gebaut war.
            //
            // DAS ANGEBOT: `Tickets::angebot` liefert nur etwas, wenn ein
            // Ticket **nicht abgelaufen** ist, und es braucht denselben
            // Hostnamen, unter dem es abgelegt wurde (`host_ohne_schema`).
            // Beides kommt aus derselben Quelle wie der Rest der
            // Verbindungsangaben — sonst wuerde der Speicher einen Host
            // fuehren, den es nicht gibt, und `psk_fuer` wuerde nie etwas
            // finden.
            // DER GUARD ENDET HIER, und das ist Absicht: `let angebot = …`
            // haelt den `MutexGuard` bis zum Ende des Blocks, und der
            // zweite `lock()` weiter unten waere ein **Deadlock**. Deshalb
            // steht der Ausdruck in einer eigenen Klammer, damit der Guard
            // **hier** fallt.
            let angebot = angebot_fuer(a.host, a.jetzt);
            if angebot.is_some() {
                WIEDERAUFNAHME_VERSUCHE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            // added 01.10.2026 11:1x UTC (Projektlabel 02.10.2026), R4z.
            // DAS ALPN-ANGEBOT, und es ist der Schritt, an dem A2 bisher
            // hing: es stand hier `[&[b"http/1.1"]]` — **zweimal** (GEMESSEN
            // per `grep -rn 'b"http/1.1"' lib.rs`), also war h2 nie
            // moeglich.
            //
            // DIE REIHENFOLGE, und sie ist nicht beliebig: RFC 7301 3.1 sagt,
            // der Client sendet seine Kennungen **in der Reihenfolge seiner
            // Praeferenz**. Wir stellen `h2` voran, weil es die einzige
            // ist, die mehrere Dateien ueber eine Verbindung holt.
            //
            // ABER — und das ist der Fall, der in R4x gemessen wurde — die
            // Wahl richtet sich nach der **Serverliste**: ein Server, der
            // nur `http/1.1` kann, waehlt das auch dann, wenn wir `h2`
            // zuerst anbieten. Deshalb wird **nicht** hier verzweigt,
            // sondern eine Zeile weiter unten nach `ist_h2()`.
            // added 01.10.2026 12:5x, R4z-NACHRAGE. DIE REGRESSION, und
            // sie ist meine: am 12:3x habe ich `h2` **zuerst** angeboten,
            // obwohl `narsil-net` den h2-**Anfragenteil** noch nicht hat.
            //
            // GEMESSENE FOLGE (Koordination, 01.10.2026): `github.com`
            // waehlt `h2` (das habe ich selbst gemessen, es stimmt), und
            // dann liefert `narsil_net::fetch`
            // `NetError::H2NochNichtAusgebaut` — **die Seite laedt
            // gar nicht**. Vorher (nur `http/1.1`) lud sie.
            //
            // DIE REGEL, und sie ist jetzt im Code, nicht in meinem
            // Kopf: **ein Protokoll anbieten heisst, es sprechen zu
            // koennen.** ALPN nimmt keine Ruecknahme — hat der Server
            // einmal `h2` gewaehlt, ist die Verbindung h2, und HTTP/1.1
            // darauf zu schreiben ergibt `Record zu lang` (GEMESSEN).
            // Man kann also nicht „anbieten und im Zweifel zurueckfallen".
            //
            // DESHALB HIER DIE EINE STELLE, an der beides zusammenhaengt:
            // `H2_WEG_GEBAUT` ist die Behauptung ueber den Code, das
            // Angebot folgt ihr. Sobald der Anfragenteil steht, wird
            // **nur diese eine Zeile** auf `true` gesetzt, und ein Test
            // (`h2_angeboten()`) beweist, dass Angebot und Faehigkeit
            // zusammenfallen.
            //
            // added 01.10.2026 12:5x, R4z-NACHRAGE. DER WERT: `false`,
            // weil `h2strom` Kopfzeilen bauen kann (R4y) und `h2` Rahmen
            // lesen kann (R4w), aber **nichts davon zu einer Anfrage
            // zusammenfuehrt** — kein Client-Preface, kein `SETTINGS`,
            // keine Antwort als Rahmenreihe.
            // added 02.10.2026 02:8x UTC (Projektlabel 02.10.2026): DER
            // SCHALTER, und er ist nur fuer die Messung von Zeile 691 da.
            // GEMESSEN 02:7x–02:8x: Fehlschlage gibt es nur, wenn die
            // TLS-Wiederaufnahme benutzt wurde (`wdn 1`); ohne Angebot sind
            // es 9 von 9 Laeufen. Die offene Frage war, ob die Wiederaufnahme
            // selbst oder ihre **ALPN-Bindung** das ist (RFC 8446 4.2.11
            // bindet ein PSK an das ALPN der Sitzung). `NARSIL_ALPN=h1` bietet
            // nur `http/1.1` an; dann muss die Wiederaufnahme scheitern, weil
            // das Ticket an `h2` gebunden ist, und der Unterschied ist
            // gemessen statt vermutet.
            // added 02.10.2026 02:8x: `let`, nicht `const` — GEMESSEN: als
            // `const` meldete der Uebersetzer E0493/E0658 („the destructor for
            // this type cannot be evaluated in constants"), weil
            // `std::env::var` einen `String` zurueckgibt.
            let alpn_angebot: &[&[u8]] = match std::env::var("NARSIL_ALPN").as_deref() {
                Ok("h1") => &[b"http/1.1"],
                Ok("leer") => &[],
                _ => {
                    if h2::weg_gebaut() {
                        &[b"h2", b"http/1.1"]
                    } else {
                        &[b"http/1.1"]
                    }
                }
            };
            let strom = match angebot.as_ref() {
                Some(angebot) => narsil_tls::strom::TlsVerbindung::ueber_sockel_mit_psk(
                    s,
                    a.host,
                    alpn_angebot,
                    a.speicher,
                    a.jetzt,
                    Some(angebot),
                ),
                None => narsil_tls::strom::TlsVerbindung::ueber_sockel(
                    s,
                    a.host,
                    alpn_angebot,
                    a.speicher,
                    a.jetzt,
                ),
            }
            .map_err(|e| NetError::Tls(e.to_string()))?;

            // added 03.10.2026 18:5x, R61. **DIE WIDERRUFSPRUEFUNG, UND SIE STEHT
            // HIER AUS DREI GRUENDEN AN GENAU DIESER STELLE.**
            //
            // added 03.10.2026 18:5x: **(1) NACH DEM HANDSHAKE**, weil vorher
            // keine Kette vorliegt: `strom.kette()` ist erst danach gefuellt. Es
            // ist dieselbe Begruendung, die in `strom.rs` fuer die OCSP-Antwort
            // gilt — eine Widerrufsantwort ist nur dann eine Aussage, wenn das
            // Zertifikat selbst gueltig ist.
            //
            // added 03.10.2026 18:5x: **(2) VOR DER ANFRAGE**, weil ein Widerruf
            // den Abbruch **vor** dem Senden von Feldern verdient: die Anfrage des
            // Nutzers (Cookies, Pfad, Header) geht einen fremden Server nichts
            // an, wenn sein Zertifikat zurueckgezogen ist.
            //
            // added 03.10.2026 18:5x: **(3) NUR WENN ES GEWOLLT IST** — die
            // Vorgabeentscheidung steht bei [`Limits::widerruf`]: der Worst Case
            // von drei Zeitlimits ist so teuer wie das ganze Zeitbudget.
            if let Some(a) = tls {
                if a.widerruf != Widerrufspolitik::NichtPruefen {
                    let urteil = crl_status_pruefen(strom.kette());
                    WIDERRUF_GEPRUEFT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if widerruf_entscheidung(a.widerruf, urteil).is_some() {
                        WIDERRUF_BLOCKIERT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        return Err(NetError::ZertifikatWiderrufen);
                    }
                }
            }
            // DIE ENTSCHEIDUNG, und sie wird **gemessen** und nicht geraten:
            // `ist_h2()` liest die Kennung, die der Server zurueckgeschickt
            // hat (R4x). Danach geht der Anfragenteil ueber `h2`, sonst
            // ueber HTTP/1.1 wie bisher.
            //
            // DER ZAEHLER, und er ist der Beweis: ohne ihn laesst sich nach
            // einem Lauf nicht sagen, ob ueberhaupt einmal h2 ausgehandelt
            // wurde — und genau das war in R4w/R4x der Grund, warum nichts
            // daran sichtbar war.
            if strom.ist_h2() {
                H2_VERBINDUNGEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            } else {
                HTTP11_VERBINDUNGEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            // DIE NACHSCHRIFT AUFNEHMEN, und das ist der Teil, der ohne
            // Sorgfalt **20 Sekunden** kostet: `hole_nachschrift` liest, bis
            // ein Record kommt, der **keine** Nachschrift ist, und ein
            // Server wartet nach dem Handshake erst auf die **Anfrage**.
            // Also: **kurze** Lesefrist, aufrufen, **zuruecksetzen**.
            //
            // GEMESSEN, dass das noetig ist und nicht theoretisch:
            // `de.wikipedia.org` schickt **2** `NewSessionTicket`,
            // `www.rust-lang.org` **1** (TLS-Rolle, 06:0x). Ohne diesen
            // Aufruf werden sie **verworfen** — `Read::read` ueberspringt
            // Nachschrift-Records (`typen::HANDSHAKE => continue`), also ist
            // der Antwortpfad in Ordnung und die Tickets gehen trotzdem
            // verloren.
            //
            // M6: `NACHSCHRIFT_FRIST` ist eine **eigene** Zeitgrenze und
            // **nicht** die Frist des Aufrufers. Sie ist kurz, weil die
            // Nachschrift im TCP-Puffer liegt, wenn sie ueberhaupt kommt;
            // ein Server, der langsamer ist, holt seine Tickets beim
            // **Lesen der Antwort** nach — `Read::read` ueberspringt sie.
            //
            // DER AUFRUF, und er setzt die Frist **selbst** und stellt sie
            // zurueck: `hole_nachschrift_mit_frist` (TLS R4n). Die erste
            // Fassung dieses Aufrufs tat `let _ =
            // strom.set_read_timeout(Some(250 ms));` und **stellte nichts
            // zurueck** — `set_read_timeout` gibt den alten Wert nicht
            // zurueck. Die HTTP-Antwort wurde dann mit 250 ms gelesen, und
            // eine langsame Antwort waere wie ein Serverausfall
            // erschienen. Ein Kommentar „vergiss nicht zurueuszusetzen"
            // haette dabei geholfen, die API nicht.
            //
            // added 04.10.2026, Rolle NETZTEMPO. **DIE FRIST IST EIN SCHALTER, und
            // das ist die Korrektur einer eigenen Messung.** GEMESSEN (Test
            // `tls_schritte`, `tests/tls_schritte.rs`): meine Phase `tls` sagt
            // **290 / 504 / 287 ms**, waehrend die **acht** Handshake-Schritte
            // aus `narsil_tls::schrittzeit` **14 / 105 / 3 ms** brauchen. Die
            // Differenz — **276 / 399 / 284 ms** — ist groesser als diese
            // Frist hier (250 ms). **DIE URSACHE IST DIESE WARTEEIT**, nicht
            // Kryptografie: `hole_nachschrift_mit_frist` **blockiert** bis zu
            // 250 ms auf ein NewSessionTicket, das der Server in diesem Fenster
            // meistens **nicht** schickt.
            //
            // **UND DIE PHASE WAR DAS HAUPTPROBLEM, nicht die Wartezeit:** der
            // Kommentar zwei Zeilen darunter sagt „die TLS-Zeit kommt **nach**
            // der Nachschrift-Buchung" — die **Buchung** ist zwar verschoben,
            // die **Uhr** (`tls_start`, 2574) faengt aber **vor** dem
            // Nachschrift-Lesen an. Also steht die Wartezeit **weiterhin** in
            // `tls`, nur nicht mehr im Kommentar. **Wer die Uhr an einer Stelle
            // stellt und die Rechnung an einer anderen, misst die Wartezeit
            // und nennt sie Handshake.**
            //
            // **WARUM ALS SCHALTER:** `0` heisst „nicht warten" — die Nachschrift
            // wird **weiterhin** genommen, wenn sie **sofort** da ist (der
            // Server darf sie direkt nach dem Handshake schicken), es wird nur
            // **nicht** darauf gewartet. Das ist der Unterschied zwischen
            // „Wiederaufnahme abschalten" und „250 ms auf etwas warten, das
            // meistens nicht kommt".
            //
            // **ALS FUNKTION, nicht als Env-Variable:** `werkzeuge/bau.sh` reicht
            // keine Umgebung durch.
            let nachschrift_frist = nachschrift_frist();
            let mut strom = strom;
            //
            // added 04.10.2026, Rolle NETZTEMPO. **DER TICKET-SPEICHER AN DEN
            // STROM, und das ist der ganze Unterschied zum Warten.** TLS hat
            // `nachschrift_speicher_setzen` gebaut (Auftrag NETZTEMPO_25 dort
            // umgesetzt): die **Leseschleife** wertet `NewSessionTicket` aus,
            // **sobald sie ankommen**. `hole_nachschrift_mit_frist` darunter
            // **blockiert** bis zu 250 ms auf ein Ticket, das der Server in dem
            // Fenster meistens **nicht** schickt — **GEMESSEN** waren das
            // 280–410 ms je Handshake bei **5–105 ms** echter Rechenzeit.
            //
            // **REIHENFOLGE, und sie ist nicht beliebig:** erst der Speicher am
            // Strom, **dann** die (jetzt **0 ms** lange) Wartezeit. Umgekehrt
            // laesst man die Bytes liegen, und der naechste Read — die
            // HTTP-Antwort — laeuft darueber: **GEMESSEN 41 831 ms**.
            strom.nachschrift_speicher_setzen(std::sync::Arc::clone(ticket_speicher()));
            //
            // added 04.10.2026, Rolle NETZTEMPO. **FRIST 0 HEISST „DEN AUFRUF
            // NICHT MACHEN", und das ist GEMESSEN der Unterschied zwischen
            // einer guten und einer kaputten Fassung.**
            //
            // **GEMESSEN, mit `nachschrift_frist_setzen(0)` und Aufruf:**
            // ```
            // rust-lang  WAND 42 447 ms | PHASEN tls=**20 125,6 ms**
            // docs.rs    WAND 20 594 ms | PHASEN tls=**20 345,5 ms**
            // ```
            // **DER GRUND, und er ist eine Eigenschaft des Systems, nicht des
            // Programms:** `set_read_timeout(Some(Duration::ZERO))` bedeutet auf
            // einem Sockel **„unbegrenzt warten"**. Der Null-Timeout in
            // `SO_RCVTIMEO` sagt dem Kernel: *keine* Wartezeit — und die 20
            // Sekunden sind eine **aussere** Grenze, nicht unsere Frist. **Wer
            // „0 ms" als Zahl uebergibt, hat nicht „nicht warten" gesagt,
            // sondern „unendlich warten".**
            //
            // **DARUM DER `if`, und nicht die Zahl:** die Frist 0 muss den
            // Aufruf **weglassen**, weil der Aufruf die Frist an einen Sockel
            // weitergibt, der sie nicht so versteht wie wir.
            if nachschrift_frist > std::time::Duration::ZERO {
                if let Ok(mut speicher) = ticket_speicher().lock() {
                    let _ = strom.hole_nachschrift_mit_frist(
                        &mut speicher,
                        a.host,
                        a.jetzt.max(0) as u64,
                        narsil_tls::strom::MAX_NACHSCHRIFT_RECORDS,
                        nachschrift_frist,
                    );
                }
            }
            // added 01.10.2026 16:8x, T3: die TLS-Zeit kommt **nach** der
            // Nachschrift-Buchung, weil `hole_nachschrift_mit_frist` ein
            // **Netzwerkvorgang** ist (der Server schickt ein Ticket). Wer
            // vorher misst, gibt die Ticket-Zeit als „Handshake" aus — und
            // das ist eine Zahl, die niemand nachvollziehen kann.
            phasen_setzen(Phasen {
                tcp: Some(tcp_dauer),
                tls: Some(tls_start.elapsed()),
                ..Phasen::leer()
            });
            Ok(Box::new(strom) as Box<dyn Leitung>)
        }
    }
}

/// Was der Verbindungsaufbau fuer HTTPS braucht.
///
/// `speicher` ist `&'static`, weil der Vertrauensspeicher einmal geladen
/// und dann fuer alle Anfragen gehalten wird; siehe `vertrauensspeicher`.
struct TlsAngaben<'a> {
    host: &'a str,
    speicher: &'a narsil_tls::kette::Vertrauensspeicher,
    jetzt: i64,
    /// added 03.10.2026 18:5x, R61. **DIE WIDERRUFSPOLITIK REIST MIT DEN
    /// TLS-ANGABEN**, und nicht als eigener Parameter von `verbinde`. Grund: sie
    /// ist eine **Eigenschaft des Verbindungsaufbaus** — dieselbe Art von Angabe
    /// wie `speicher` und `jetzt` — und `verbinde` bekommt `TlsAngaben` bereits
    /// als **ein** Bündel. Ein vierter Parameter wäre dieselbe Information an zwei
    /// Orten.
    widerruf: Widerrufspolitik,
}

/// `TlsVerbindung` als `Leitung`.
///
/// added 30.09.2026. DAS IST DER GANZE PUNKT der Verdrahtung: ein
/// lokales Trait auf einem fremden Typ. Ab hier weiss der HTTP-Pfad
/// nicht, ob er ueber TCP oder ueber TLS spricht — dieselben sechs
/// Aufrufe, dieselbe `Box<dyn Leitung>`.
///
/// Die Zeitlimits sind nicht nur weitergereicht, sie sind die
/// entscheidende Stelle: `Leser` setzt sie vor jedem Lesevorgang neu,
/// und `TlsStrom` liest ueber denselben Sockel. Ohne dieses
/// Durchreichen haetten die Lesezeiten des Browsers keine Wirkung
/// gehabt und ein haengender TLS-Server haette den Tab blockiert.
impl Leitung for narsil_tls::strom::TlsVerbindung {
    /// added 01.10.2026 12:0x, R4z: hier kommt die Antwort her — aus
    /// `ist_h2()`, das seit R4x die **vom Server gewaehlte** Kennung liest.
    fn ist_h2_gewahlt(&self) -> bool {
        narsil_tls::strom::TlsVerbindung::ist_h2(self)
    }

    fn set_nodelay(&mut self, wert: bool) -> io::Result<()> {
        narsil_tls::strom::TlsVerbindung::set_nodelay(self, wert)
    }

    fn set_read_timeout(&mut self, dauer: Option<Duration>) -> io::Result<()> {
        narsil_tls::strom::TlsVerbindung::set_read_timeout(self, dauer)
    }

    fn set_write_timeout(&mut self, dauer: Option<Duration>) -> io::Result<()> {
        narsil_tls::strom::TlsVerbindung::set_write_timeout(self, dauer)
    }

    /// added 04.10.2026, Rolle NETZTEMPO. **DER TODES-CHECK FUER TLS** — und
    /// das ist die Luecke, die der Auftrag der Leitung meinte („tote
    /// Verbindungen vor Wiederverwendung pruefen, damit der Neuversuch nicht
    /// erst nach einem Fehlschlag passiert").
    ///
    /// **GEMESSEN, was vorher lief** (Bericht 15): die erste Anfrage nach dem
    /// Leerlauf kostete **1359 ms** statt ~100 ms, mit `WIR SCHLIESSEN:
    /// Wiederaufnahme verworfen`. Der Check **existierte** und wurde
    /// **aufgerufen** — aber fuer jede echte HTTPS-Verbindung war er der
    /// **Standard `true`**, weil `Leitung::lebt_noch` als `&self` kein Lesen
    /// erlaubte und die TLS-Schicht keinen `peek` auf den Sockel durchreicht.
    ///
    /// **DER WEG: ein Leseversuch mit Timeout 0 auf der TLS-Schicht.** Er ist
    /// sicher, und das ist der entscheidende Punkt — denn ein `read`, das
    /// entschluesseltes Byte frisst, waere ein Datenverlust:
    ///
    /// * `Ok(0)` → die Gegenstelle hat geschlossen (`close_notify` oder Wegfall
    ///   des Drahts). **Tot.**
    /// * `Ok(n > 0)` → es liegen **Daten** an. Wir haben sie gelesen und
    ///   koennen sie **nicht** zurueckgeben — die Verbindung ist fuer uns
    ///   **verloren**, obwohl sie lebt. Also `false`, und der Aufrufer
    ///   verbindet neu. **Daten im Weg sind der einzige Fall, in dem wir
    ///   absichtlich eine lebende Verbindung wegwerfen** — sonst waere der
    ///   Protokollstrom um **ein** Byte verschoben.
    /// * `WouldBlock` / `TimedOut` → nichts da: der Normalfall. `true`.
    /// * alles andere → `false`.
    ///
    /// **UND WARUM DER TIMEOUT NICHT ZURUECKGESETZT WIRD:** `set_read_timeout`
    /// gibt den alten Wert **nicht** zurueck (das steht so an der
    /// Nachschrift-Stelle in dieser Datei) — und **jeder** Lesepfad setzt seine
    /// Frist **vor** dem Lesen neu (`h2client::holen` je Rahmen, `Leser::fuelle`
    /// je Lesevorgang). Wer hier einen Wert „zurueckschreibt", erfindet eine
    /// Signatur, die es nicht gibt — der erste Entwurf tat genau das.
    fn lebt_noch(&mut self) -> bool {
        const KURZ: Duration = Duration::from_millis(0);
        if self.set_read_timeout(Some(KURZ)).is_err() {
            // Ohne Timeout zu arbeiten waere schlimmer als zu raten: ein
            // Sockel ohne Lese-Timeout blockiert **ewig**, und der Browser
            // haengt statt zu antworten.
            return true;
        }
        let mut kratzer = [0u8; 1];
        match self.read(&mut kratzer) {
            Ok(0) => {
                crate::h2geteilt::spur_notiz("TODES-CHECK TLS: Gegenseite zu (EOF)");
                false
            }
            Ok(_) => {
                crate::h2geteilt::spur_notiz("TODES-CHECK TLS: Daten im Weg -> verwerfen");
                false
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                true
            }
            Err(_) => {
                crate::h2geteilt::spur_notiz("TODES-CHECK TLS: Lesefehler -> verwerfen");
                false
            }
        }
    }

    /// `beende` schickt erst `close_notify` und schliesst dann den
    /// Sockel. In der umgekehrten Reihenfolge schickte der Browser ein
    /// RST, und der Server wuesste nicht, ob die Antwort noch kommt.
    fn shutdown(&mut self, wie: Shutdown) -> io::Result<()> {
        narsil_tls::strom::TlsVerbindung::beende(self, wie)
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        narsil_tls::strom::TlsVerbindung::local_addr(self)
    }

    fn peer_addr(&self) -> io::Result<SocketAddr> {
        narsil_tls::strom::TlsVerbindung::peer_addr(self)
    }

    /// Geht fuer einen TLS-Strom nicht: es fehlt der
    /// Vertrauensspeicher, und ohne ihn waere es eine Verbindung OHNE
    /// Pruefung. Genau deshalb ist es hier ein Fehler und nicht ein
    /// bequemer Weg.
    fn connect_timeout(_addr: &SocketAddr, _timeout: Duration) -> io::Result<Self>
    where
        Self: Sized,
    {
        Err(io::Error::other(
            "ein TLS-Strom braucht den Vertrauensspeicher; nimm TlsVerbindung::ueber_sockel",
        ))
    }
}

/// DER TLS-PUNKT, und es ist genau eine Funktion.
///
/// WARUM `Box<dyn Leitung>` und kein Typparameter (Stand 30.09.2026,
/// nach dem Ausfall der Rolle KOMPRESS, die den Umbau
/// `trait Leitung: Read + Write` halb fertig geliebt hatte):
///
/// 1. **Der halbe Umbau liess `Leser<L>` generisch, die Helfer aber
///    nicht.** Sechs Stellen nahmen `&mut Leser` ohne Typargument —
///    `E0107`, sieben Fehler, und `narsil-net` baute damit nicht. Der
///    Widerspruch ist nicht „Generics sind falsch", sondern: fuer
///    DYNAMISCHE Auswahl braucht es eine konkrete Typisierung, und die
///    ist `Box<dyn Leitung>`.
///
/// 2. **Generics kosten an jeder Stelle `.as_ref()`.** Bei
///    `Leser<L>` braucht `lese_statuszeile` einen generischen Aufruf,
///    und jede neue Hilfsfunktion muss den Typparameter mitschleppen.
///    Genau daran ist der Umbau gescheitert.
///
/// 3. **Der Vorteil der Generics ist hier nicht vorhanden.** Es gibt
///    EINE Implementierung (`TcpStream`), keine Auswahl zur Laufzeit.
///    Wer TLS braucht (Welle 6), aendert `verbinde` — eine Funktion —
///    und nicht sieben Aufrufstellen.
///
/// DESHALB: `Box<dyn Leitung>`. Die Kosten sind eine dynamische
/// Zustaendverteilung pro Lesevorgang (ein Vtable-Zeiger auf ein
/// `TcpStream`; bei 30 KiB Antwort sind das 30.000 indirekte Aufrufe,
/// und der Unterschied liegt unter der Rauschkante der Messung).
///
/// WAS DAS NICHT KAPUTT MACHT: `Leser::neu` nimmt eine fertige Box, und
/// `verbinde` entscheidet, was hineinkommt.
/// Ist das eine HTTPS-Anfrage?
///
/// DER MASSSTAB ist der Port, nicht das Schema. `url_teile` liefert bei
/// `https://` den Port 443, und ein Aufrufer, der `Request::get` direkt
/// benutzt, kann einen beliebigen Port setzen. `443` heisst in beiden
/// Faellen HTTPS.
///
/// Die Ausnahme: ein Host mit ausgeschriebenem `https://` davor, weil
/// das die alte Fassung von `sende` so geprueft hat. Das Schema wird
/// aber ABGESCHNITTEN, bevor der Host benutzt wird — sonst geht
/// `https://example.com` als Adresse in die Namensauflösung.
fn ist_https(request: &Request) -> bool {
    // added 02.10.2026 07:3x, R5i: das ausdrueckliche Feld gewinnt. GEMESSEN,
    // warum es **vor** der Port-Regel kommt: `Request::get("localhost", 9443,
    // ..)` hat Port 9443 und damit nach der alten Regel „kein HTTPS" — mit
    // `.https()` ist es aber eine TLS-Verbindung.
    if let Some(ja) = request.https {
        return ja;
    }
    request.port == 443 || request.host.starts_with("https://")
}

/// Der Host OHNE Schema, wie er fuer SNI und die Zertifikatspruefung
/// gebraucht wird.
///
/// GEMESSENER FEHLER, den ich beim Verdrahten selbst gefunden habe: die
/// alte Schicht hat `request.host.starts_with("https://")` geprueft,
/// aber `host` ansonsten direkt als Adresse benutzt. Ein Host mit
/// Schema daraus waere `https://example.com` in `to_socket_addrs`, und
/// das liefert `Name cannot be resolved` — als ob die Seite kaputt
/// waere statt der Verdrahtung.
pub fn host_ohne_schema(host: &str) -> &str {
    for praefix in ["https://", "http://"] {
        if let Some(rest) = host.strip_prefix(praefix) {
            return rest;
        }
    }
    host
}

/// Wo die Vertrauensanker liegen.
///
/// WARUM ein Pfad und kein Kompilieren: 150 Zertifikate sind 224 449
/// Byte. Das als `include_str!` einzubauen, waere eine Datei im
/// Quelltext, die niemand lesen kann und die bei jeder Aenderung am
/// System neu eingecheckt werden muesste. Das System-Bundle wird zur
/// Laufzeit gelesen — einmal.
const VERTRAUENSPFAD: &str = "/etc/ssl/certs/ca-certificates.crt";

/// Der Name der Umgebungsvariablen, die den Pfad zum CA-Bundle ueberschreibt.
///
/// added 02.10.2026 05:1x, Rolle TLS. **DIE EINE ZEILE**, um die V12
/// freizumachen, und der Wunsch steht in `team/WUNSCH_TLS.txt`
/// (Eintraege 10:25 und 13:26 des Projektlabels 02.10.2026).
pub const CA_BUNDLE_VARIABLE: &str = "NARSIL_CA_BUNDLE";

/// Wo die Vertrauensanker fuer **diesen Prozess** liegen.
///
/// added 02.10.2026 05:1x. GEMESSEN, warum das noetig war (Wunschstext,
/// 13:26): der Browser konnte das Testzertifikat von `narsil-server` nicht
/// laden, weil `VERTRAUENSPFAD` fest auf das System-Bundle zeigte. Und
/// GEMESSEN, warum **nicht** die Systemdatei: das Testzertifikat einmal in
/// `/etc/ssl/certs/ca-certificates.crt` einzutragen aendert die Datei fuer
/// **jeden** Prozess — ein Fehler darin wirkt auf alles.
///
/// DIE REGEL, und sie ist die von R0: ein Aufrufer darf sagen, was er will.
/// Ohne die Variable ist der Pfad exakt der von vorher — ein Normalbetrieb
/// bemerkt diese Funktion nicht.
///
/// DER NAME endet auf `_BUNDLE`, weil es ein **PEM-Bundle** sein muss
/// (mehrere Zertifikate hintereinander). Eine einzelne DER-Datei liest
/// `narsil_tls::kette` nicht; der Wunsch nennt genau diese Form.
pub fn ca_bundle_pfad() -> String {
    ca_bundle_aus(std::env::var(CA_BUNDLE_VARIABLE).ok())
}

/// DIE ENTSCHEIDUNG, ohne Umgebung.
///
/// added 02.10.2026 05:4x. WARUM ZWEI FUNKTIONEN: `std::env::set_var` ist
/// in Rust 2024 `unsafe` (die Umgebung ist prozessweit, ein paralleler
/// Leser wuerde sie sehen), und `unsafe` ist in diesem Crate verboten. Also
/// liest `ca_bundle_pfad` die Variable, und **dies** Funktion entscheidet —
/// dadurch ist die Entscheidung ohne `unsafe` und ohne Reihenfolge der Tests
/// pruefbar.
///
/// `gesetzt == None` heisst „die Variable gibt es nicht" (der Normalfall).
pub fn ca_bundle_aus(gesetzt: Option<String>) -> String {
    match gesetzt {
        // added 02.10.2026 05:1x: ein **leeres** `NARSIL_CA_BUNDLE=` ist kein
        // Wunsch, sondern ein Tippfehler, und `read_to_string("")` waere ein
        // Fehler ohne Namen. Der Vorgabewert ist hier richtig, weil er
        // lesbar ist.
        Some(pfad) if !pfad.trim().is_empty() => pfad,
        _ => VERTRAUENSPFAD.to_string(),
    }
}

/// Der Vertrauensspeicher, EINMAL gelesen und dann behalten.
///
/// M3/M4: gelesen wird genau einmal, in `get_or_init`. Ohne den
/// Cache wuerde jede Anfrage das 224-KiB-Bundle erneut parsen und
/// 150 Zertifikate erneut lesen — bei 30 Tabs sind das 4,5 MiB und
/// 4500 Zertifikatleser-Laufen pro Seitenaufbau.
///
/// `&'static` statt `Arc`: `Vertrauensspeicher` nimmt bei
/// `kette_pruefen` eine Referenz und veraendert nichts, es gibt also
/// keinen Grund zu teilen, was nicht geteilt werden muss.
static SPEICHER: std::sync::OnceLock<Result<narsil_tls::kette::Vertrauensspeicher, String>> =
    std::sync::OnceLock::new();

/// Der Vertrauensspeicher, oder ein benannter Fehler.
fn vertrauensspeicher() -> Result<&'static narsil_tls::kette::Vertrauensspeicher, NetError> {
    SPEICHER
        .get_or_init(|| {
            if std::env::var(CA_BUNDLE_VARIABLE).ok().is_none_or(|s| s.trim().is_empty()) {
                return narsil_tls::kette::Vertrauensspeicher::aus_system();
            }
            // added 02.10.2026 05:1x: der Pfad kommt aus `ca_bundle_pfad()`,
            // damit `NARSIL_CA_BUNDLE` etwas bewirkt (Begründung und Messung an
            // deren Kopf).
            let pfad = ca_bundle_pfad();
            let text =
                std::fs::read_to_string(&pfad).map_err(|e| format!("{pfad} nicht lesbar: {e}"))?;
            narsil_tls::kette::Vertrauensspeicher::aus_pem(&text).map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| NetError::Tls(e.clone()))
}

/// DER TICKET-SPEICHER des Prozesses.
///
/// added 01.10.2026 (Uhr) / 02.10.2026 (Projektlabel), TLS R4n. Die Form
/// ist **die des Vertrauensspeichers** (`static SPEICHER: OnceLock<…>`,
/// Zeile 1807) und nicht eine eigene: Sitzungs-Tickets sind genauso
/// **Anwendungszustand** wie die Vertrauensanker, und eine eigene Form fuer
/// denselben Lebenszyklus waere die zweite Wahrheit ueber dieselbe Sache.
///
/// WARUM ER HIER UND NICHT IN `narsil-tls`: ein Protokollcrate, das
/// entscheidet, welche Tickets die Anwendung behalten will, waertet die
/// Eigentuemlichkeit. `narsil-tls::ticket::Tickets` ist der **Behaelter** und
/// weiss nichts vom Prozess.
///
/// WARUM EIN `Mutex`: `Tickets` ist `HashMap<String, Vec<Ticket>>` und
/// braucht innere Verdaechtigung; der Aufrufer haelt die Leitung, und ein
/// `&Tickets` ueber einen `Box<dyn Leitung>` hinweg waere eine
/// Lebensdauerfrage fuer die sich niemand eine Antwort geboten hat.
/// M3: die Grenzen stehen in `ticket.rs` (4 je Host, 128 Hosts), nicht
/// hier — sonst waeren sie an zwei Stellen.
///
/// `Mutex::lock()` scheitert nur bei einem **vergifteten** Mutex, also nach
/// einem Panic waehrend des Haltens. Das ist ein Fall, in dem ein Browser
/// ohne Wiederaufnahme weiterlaeuft; die Alternative (panic) waere fuer
/// eine Nebenfunktion der Tod des Prozesses.
/// Das Angebot fuer einen Host — oder `None`, wenn es keins gibt.
///
/// added 01.10.2026 (Uhr) / 02.10.2026 (Projektlabel), TLS R4n. DER GRUND
/// FUER EINE EIGENE METHODE: die Entscheidung stand als Ausdruck **inline**
/// im Verbindungsaufbau, und damit war sie nicht pruefbar. Ein Ausdruck,
/// der „ein Angebot, wenn eins da ist" sagt, ist eine Zeile und eine
/// Behauptung; als Funktion ist sie eine Zeile und ein **Test**
/// (`tests/ticket_angebot.rs`).
///
/// DER ZEITWERT: `jetzt` kommt als **Sekunden seit 1970** herein
/// (`jetzt_in_sekunden()` im Aufrufer), und `Tickets::angebot` will
/// Sekunden — aber es kann **kein** negatives `i64` annehmen, weil
/// `verschleiertes_alter` mit `saturating_sub` rechnet und ein negatives
/// Alter einen `u32`-Ueberlauf ergaebe. `max(0)` ist deshalb **nicht**
/// Kosmetik, und es steht hier, damit die Rechnung an **einer** Stelle ist.
pub fn angebot_fuer(host: &str, jetzt: i64) -> Option<narsil_tls::ticket::PskAngebot> {
    let speicher = ticket_speicher().lock().ok()?;
    speicher.angebot(host, jetzt.max(0) as u64)
}

/// Wie oft der **Wiederaufnahme-Pfad** betreten wurde.
///
/// added 01.10.2026 05:3x UTC (Projektlabel 02.10.2026), TLS R4o. DER
/// GRUND FUER EINEN ZAEHLER IM PRODUKTCODE: die Frage „laeuft der Pfad
/// ueberhaupt?" laesst sich sonst **nicht** beantworten, und sie ist die
/// Frage, die entscheidet, ob die ganze Arbeit etwas bewirkt. Aus
/// `tests/ticket_live.rs` ist sie **nicht** sichtbar, weil `fetch_mit` nur
/// eine `Response` zurueckgibt.
///
/// DER BEFUND, der ihn noetig macht (GEMESSEN, siehe
/// `pool::hole_oder_verbinde_mit_strom` — R69 hat die aeltere Form
/// `pool::hole_oder_verbinde` geloescht, GEMESSEN null Aufrufer, und das
/// Zitat folgt der **lebenden** Form): bei einer **gepoolten** Leitung wird
/// `verbinde_neu` **nicht** aufgerufen — der Aufbau, und damit auch dieser
/// Pfad, laeuft nur bei einer **neuen** TCP-Verbindung. Ein Browser, der
/// denselben Host ein zweites Mal besucht, nimmt also die Leitung aus dem
/// Pool und fuehrt **keinen** Handshake und **kein** Angebot. Die
/// Wiederaufnahme greift erst, wenn die Leitung weg ist.
///
/// WARUM `AtomicU64` UND NICHT EIN ZAHLENFELD IM SPEICHER: der Speicher
/// haengt an `&mut` durch einen `Box<dyn Leitung>` hindurch, und ein
/// Zaehler, den man nur mit einer Sperre lesen kann, waere an der
/// Aufrufstelle ein Deadlock in Wartestellung. `AtomicU64` ist im
/// schlimmsten Fall eine Zahl, die einmal zu alt ist — bei einer Messung
/// ist das ein Fehler, bei einer Buchfuehrung waere es einer.
pub static WIEDERAUFNAHME_VERSUCHE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Wie oft der Server **`h2`** gewaehlt hat.
///
/// added 01.10.2026 11:1x UTC (Projektlabel 02.10.2026), R4z. DER
/// ZAEHLER, DER FEHLTE, und sein Fehlen ist der Grund, warum vier
/// Runden lang niemand gemerkt hat, dass `narsil-net` `h2` nie angeboten hat:
/// es gab `[&[b"http/1.1"]]` an zwei Stellen, und es gab **keine** Zahl,
/// die etwas anderes behauptet haette.
///
/// WARUM EIN ZAEHLER UND NICHT EINE MELDUNG: der Aufrufer von `hole()`
/// will die Seite, nicht die Protokollentscheidung. Wer h2 redet, muss
/// danach auch den Anfragenteil anders bauen — und das ist eine
/// Verzweigung im Aufrufer, nicht im Client. Der Zaehler macht die
/// Verzweigung messbar, ohne sie aufzuzwingen.
pub static H2_VERBINDUNGEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Wie oft der Server **`http/1.1`** gewaehlt hat (oder gar keins).
///
/// added 01.10.2026 11:1x, R4z. DER GEGENZAEHLER, und er ist nicht
/// Kosmetik: `ist_h2()` allein kann nicht unterscheiden zwischen „der
/// Server hat `h2` nicht gekannt" und „wir haben `h2` gar nicht
/// angeboten". Beide fuehren zu `ist_h2() == false`, aber nur der erste
/// ist ein Ergebnis und der zweite ein Fehler.
pub static HTTP11_VERBINDUNGEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Wie oft `h2` ausgehandelt wurde. Fuer Tests und Berichte.
pub fn h2_verbindungen() -> u64 {
    H2_VERBINDUNGEN.load(std::sync::atomic::Ordering::Relaxed)
}

/// Wie oft `http/1.1` (oder nichts) ausgehandelt wurde.
pub fn http11_verbindungen() -> u64 {
    HTTP11_VERBINDUNGEN.load(std::sync::atomic::Ordering::Relaxed)
}

/// added 03.10.2026 18:6x, R61. **WIE OFT DER WIDERRUF GEPRUEFT WURDE**, und
/// der Zaehler ist nicht Kosmetik: `crl_status_pruefen` gibt **kein** `Result`
/// und `widerruf_entscheidung` **nur** im Fehlerfall — ohne Zaehler waere ein
/// Client, der die Pruefung nie ausfuehrt, von einem, der sie ausfuehrt und
/// nichts findet, **nicht** unterscheidbar. Genau diese Ununterscheidbarkeit ist
/// in R50/R51 zweimal als Befund aufgetaucht (ein Wert wird geliefert und
/// weggelegt); ein Zaehler ist die kleinste Form, sie zu verhindern.
pub static WIDERRUF_GEPRUEFT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// added 03.10.2026 18:6x: **WIE OFT EIN WIDERRUF DIE VERBINDUNG VERHINDERT
/// HAT** — getrennt vom vorigen, weil die **Differenz** die interesting Zahl
/// ist: `geprueft` minus `blockiert` sind die Pruefungen, die **nichts** gefunden
/// haben. Eine einzige Zahl koennte man mit einer Fehlermeldung verwechseln.
pub static WIDERRUF_BLOCKIERT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// added 03.10.2026 18:6x: **WIDERRUFSPRUEFUNGEN, fuer Tests und Berichte.**
pub fn widerruf_geprueft() -> u64 {
    WIDERRUF_GEPRUEFT.load(std::sync::atomic::Ordering::Relaxed)
}

/// added 03.10.2026 18:6x: **BLOCKIERTE VERBINDUNGEN, fuer Tests und Berichte.**
pub fn widerruf_blockiert() -> u64 {
    WIDERRUF_BLOCKIERT.load(std::sync::atomic::Ordering::Relaxed)
}

/// Bietet der Browser **`h2`** an? Siehe `h2::weg_gebaut()`.
///
/// added 01.10.2026 12:5x, R4z-NACHRAGE: die Regressionsbremse. Wer
/// `h2_angeboten() == true` sieht, weiss, dass der h2-Anfragenteil steht;
/// wer `false` sieht, weiss, dass der Browser **nur** HTTP/1.1 redet und
/// darum **keine** Seite verliert.
pub fn h2_angeboten() -> bool {
    h2::h2_angeboten()
}

/// Setzt **beide** Zaehler zurueck, damit ein Test von einem bekannten
/// Anfang ausgeht.
///
/// added 01.10.2026 11:1x, R4z. OHNE DIESE FUNKTION ist die Messung nicht
/// wiederholbar: die Zaehler sind prozessweit, und ein zweiter Test im
/// selben Lauf sieht die Summe des ersten.
pub fn alpn_zaehler_zuruecksetzen() {
    H2_VERBINDUNGEN.store(0, std::sync::atomic::Ordering::Relaxed);
    HTTP11_VERBINDUNGEN.store(0, std::sync::atomic::Ordering::Relaxed);
}

/// Wie oft der Wiederaufnahme-Pfad betreten wurde. Fuer Tests und Berichte.
pub fn wiederaufnahme_versuche() -> u64 {
    WIEDERAUFNAHME_VERSUCHE.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn ticket_speicher() -> &'static std::sync::Arc<std::sync::Mutex<narsil_tls::ticket::Tickets>> {
    //
    // added 04.10.2026, Rolle NETZTEMPO. **JETZT EIN `Arc`, und der Grund ist
    // der Weg, den TLS gebaut hat (Auftrag NETZTEMPO_25, dort umgesetzt):**
    // `TlsVerbindung::nachschrift_speicher_setzen` nimmt einen
    // **`Arc<Mutex<Tickets>>`** und **behaelt ihn in der Verbindung** — die
    // Leseschleife wertet die Ticket-Records aus, sobald sie ankommen. Ohne
    // `Arc` koennte die Verbindung den Speicher nicht behalten, und die
    // Alternative waere **Kopieren**: zwei Speicher, zwei Wahrheiten, und das
    // Ticket, das die Leseschleife ablegt, kaeme in dem nicht an, aus dem
    // `ueber_sockel_mit_psk` beim naechsten Dial liest.
    //
    // **DIE AUFRUFSTELLEN BLEIBEN GLEICH:** alle benutzen `.lock()`, und
    // `Arc<Mutex<T>>` gibt `.lock()` ueber `Deref` weiter. Das ist der Grund,
    // warum die Aenderung an **einer** Stelle reicht.
    static SPEICHER: std::sync::OnceLock<
        std::sync::Arc<std::sync::Mutex<narsil_tls::ticket::Tickets>>,
    > = std::sync::OnceLock::new();
    SPEICHER
        .get_or_init(|| std::sync::Arc::new(std::sync::Mutex::new(narsil_tls::ticket::Tickets::neu())))
}

/// Die Sekunden seit 1970 fuer die Gueltigkeitspruefung.
///
/// GEMESSEN: `kette_pruefen` braucht `jetzt`, weil ein Test ein Datum
/// in der Vergangenheit setzen muss. Fuer den Browser ist es die
/// echte Uhr — und der Unterschied zu `Instant` ist wichtig: die
/// Zertifikatsgueltigkeit steht in Sekunden seit 1970, `Instant` hat
/// keinen festen Nullpunkt.
fn jetzt_in_sekunden() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Dieselbe Zeit als `u64`, wie `narsil-browser` sie erwartet.
///
/// added 30.09.2026, Welle 7 / N6. DER WEG: `jetzt_in_sekunden` gibt
/// `i64`, `CookieSpeicher` und `HSTSSpeicher` rechnen mit `u64`. Die
/// Umrechnung ist `max(0)` und **nicht** `as u64`: eine Uhr, die vor
/// 1970 steht, liefert `i64::MIN` fuer `d` als `u64` eine Zahl nahe
/// `2^64` — und damit fuer einen Cookie mit `Max-Age=3600` ein
/// Ablaufdatum von 1,8 Milliarden Jahren. Das ist kein Fehler, den man
/// sieht, sondern einer, den nur ein Test findet.
///
/// DER WERT BEI NEGATIVER ZEIT: 0. Dann ist jedes Cookie mit einem
/// Ablaufdatum in der Vergangenheit sofort abgelaufen — konservativ
/// und damit die sichere Wahl.
fn jetzt_als_u64() -> u64 {
    jetzt_in_sekunden().max(0) as u64
}

// ---------------------------------------------------------------------------
// W7 / N6: Cookies und HSTS
// ---------------------------------------------------------------------------

/// Hebt eine http-Anfrage auf https an, wenn der Host HSTS hat.
///
/// added 30.09.2026, Welle 7 / N6. DER WEG: die Sitzung wird gefragt,
/// und wenn sie „ja" sagt, werden **Port** und Schema angepasst.
///
/// RFC 6797 Abschnitt 8.1 („HTTP Response Header Strict Transport-
/// Security“) sagt woertlich: "the user agent MUST rewrite the
/// request URL to use the https scheme before sending it". Genau das
/// ist diese Funktion.
///
/// WARUM AUCH DER PORT: `Request` fuehrt Port und Schema getrennt, und
/// `ist_https` entscheidet ueber den Port. Ohne Portwechsel bliebe die
/// Anfrage auf Port 80 stehen und `ist_https` waere weiter `false` —
/// die Anhebung waere folgenlos.
///
/// WAS NICHT GESCHIEHT, und warum: es wird **kein** HSTS-Eintrag
/// erzeugt. Nur eine Antwort ueber https darf einen Eintrag setzen, und
/// das macht `antwort_gegen_stand_melden`.
fn hsts_anheben(s: &mut dyn sitzung::Sitzungszustand, anfrage: &mut Request) {
    if ist_https(anfrage) {
        return;
    }
    let host = host_ohne_schema(&anfrage.host).to_string();
    if !s.hsts_erzwungen(&host) {
        return;
    }
    // DER PORT WIRD MITgezogen, wenn der Aufrufer ausdruecklich einen
    // anderen als 80 genannt hat — sonst waere es eine Ueberraschung.
    // Bei Port 80 ist 443 der Vorgabewert, sonst bleibt der genannte
    // Port (ein Testserver auf 8080 mit HSTS bleibt auf 8080, nur mit
    // https; das ist die einzige Lesart, die den Test ermoeglicht).
    if anfrage.port == 80 {
        anfrage.port = 443;
    }
}

/// Was die Antwort gemeldet hat: `Set-Cookie` und HSTS.
///
/// added 30.09.2026, Welle 7 / N6.
///
/// DIE ZWEI GRENZEN, beide in `sitzung.rs` benannt und beide **hier
/// angewandt**:
///
/// * `MAX_SET_COOKIE_HEADERS` (64) gegen die Zahl der Koepfe. Wird sie
///   gerissen, meldet `set_cookie_ignoriert` die **Anzahl der
///   verworfenen** Koepfe — nicht nur „zu viele". Der Benutzer soll
///   wissen, dass 400 Cookies nicht angekommen sind, nicht dass
///   irgendetwas nicht angekommen ist.
/// * `MAX_SET_COOKIE_ZEICHEN` (4096) gegen die Laenge **eines** Wertes.
///   Der wird verworfen und gemeldet, und die anderen werden weiter
///   verarbeitet: ein einzelner kaputter Cookie darf nicht alle
///   mitnehmen.
///
/// DER WEG, WARUM DIE ZAHLUNG HIER LIEGT UND NICHT IN DER SITZUNG: die
/// Grenze gehoert zum **Netzpfad**, weil nur hier sichtbar ist, wie
/// viele Koepfe wirklich ankamen. Die Sitzung sieht einen Kopf pro
/// Aufruf und koennte ihre Grenze nicht einhalten.
fn antwort_gegen_stand_melden(
    s: &mut dyn sitzung::Sitzungszustand,
    anfrage: &Request,
    antwort: &Response,
) {
    let jetzt = jetzt_als_u64();
    let domain = host_ohne_schema(&anfrage.host);

    // 1. Set-Cookie.
    //
    // DER WEG: aus `Response.set_cookie`, **nicht** aus `headers`. Der
    // Unterschied ist der ganze Fehler, der hier behoben wurde: in
    // `headers` stehen zwei `Set-Cookie`-Koepfe als EINER Wert
    // (`a=1, b=2`), weil `header_auswerten` Mehrfach-Header
    // kommasepariert. RFC 6265 5.2 kennt diese Syntax nicht — ein
    // Komma darf in einem Cookie stehen (`Expires=Wed, 09 Jun 2021`).
    // Aus `set_cookie` kommen sie einzeln, in Empfangsreihenfolge.
    let roh_alle = antwort.set_cookie.len();
    // M3: die Liste der **zu verarbeitenden** Koepfe ist gedeckelt; die
    // Zahl der verworfenen ergibt sich aus der Differenz.
    let verarbeitet = roh_alle.min(sitzung::MAX_SET_COOKIE_HEADERS);
    for wert in antwort.set_cookie.iter().take(verarbeitet) {
        if wert.len() > sitzung::MAX_SET_COOKIE_ZEICHEN {
            // DER EINZELNE WIRD VERWORFEN, die anderen laufen weiter: ein
            // kaputter Cookie darf nicht alle mitnehmen. Und er wird
            // **benannt** — die Projektregel verbietet stilles
            // Abschneiden.
            s.set_cookie_ignoriert(sitzung::GRUND_SET_COOKIE_ZU_LANG, 1);
            continue;
        }
        // added 02.10.2026 12:2x UTC (Projektlabel 02.10.2026), Rolle TLS.
        // Runde 8 S5: **WAS EIN USER AGENT MIT EINEM `Set-Cookie` MACHT, und hier
        // wird es durchgesetzt.**
        //
        // added 02.10.2026 12:2x: **WARUM AN DIESER STELLE, und nicht im
        // Browser.** GEMESSEN am 02.10.2026: `narsil-browser/src/cookie.rs` parst
        // `secure`, `http_only` und `same_site`, **wertet aber keines davon aus** —
        // `same_site` wird nur geschrieben (Zeile 520) und nirgends gelesen, und
        // nach `__Host-`/`__Secure-` wird im ganzen `narsil-browser/src` und
        // `narsil-app/src` nicht gesucht (ohne Treffer). Diese Datei gehoert nicht
        // mir. Diese Schleife ist die **Durchsetzungsstelle, die mir gehoert**,
        // und es ist genau die, an der schon einmal eine Kopfsicherheitsregel
        // durchgesetzt wird (`Strict-Transport-Security` ueber Klartext, ein paar
        // Zeilen weiter unten). Ein hier abgelehntes Cookie erreicht den Browser
        // gar nicht erst, und die Regel steht damit an **einer** Stelle.
        //
        // added 02.10.2026 12:2x: **UND `ist_https` ALS DIE SICHERE VERBINDUNG**,
        // weil Schritt 13 des Speichermodells „as defined by the user agent" sagt
        // und die einzige belegbare Festlegung diese ist: die Anfrage kam ueber
        // `https`. GEMESSEN an der vorhandenen Hilfe: `Request` fuehrt `https:
        // Option<bool>`, und `ist_https` wertet es aus. Es wird nichts geraten.
        //
        // added 02.10.2026 12:2x: **UND DER AUFRUFER VERAENDERT DEN TEXT NICHT.**
        // Frueher stand hier ein Kommentar, der behauptete, wir reichten den
        // „wirksamen" SameSite-Wert weiter, während der Code `wert` unveraendert
        // weitergab. Ein Kommentar, der etwas behauptet, was der Code nicht tut,
        // ist der Fehler, den die Projektregel meint — er faellt nur nicht durch
        // den Bauprüfer. Der Text geht deshalb **unveraendert** an den Browser;
        // was der User Agent **aus** ihm macht, entscheidet der Browser.
        let https = ist_https(anfrage);
        if let Some(mangel) = sitzung::cookie_mangel(wert, https) {
            s.set_cookie_ignoriert(mangel.grund(), 1);
            continue;
        }
        s.set_cookie_sehen(domain, wert, jetzt);
    }
    let verworfen = roh_alle - verarbeitet;
    if verworfen > 0 {
        s.set_cookie_ignoriert(sitzung::GRUND_ZU_VIELE_COOKIES, verworfen);
    }

    // 2. Strict-Transport-Security.
    //
    // DER WEG ZUERST NACH HTTPS FRAGEN, und der Grund ist RFC 6797
    // Abschnitt 7.2: ein `Strict-Transport-Security`-Kopf in einer
    // Klartext-Antwort ist nicht vertrauenswuerdig. Ein Angreifer im
    // Netz schickt ihn mit, und wer ihn speichert, laesst sich sagen,
    // fuer welche Domains er den Browser nicht mehr benutzen darf — ein
    // Angriff auf die Reichweite, nicht auf eine Seite.
    //
    // UND ER WIRD TROTZDEM **GEMELDET**, nicht stillschweigend
    // verworfen: der Benutzer soll sehen, dass ein Server ihm etwas
    // anbietet, was der Browser nicht annimmt. `set_cookie_ignoriert`
    // ist dafuer der benannte Kanal.
    let hsts_wert = antwort
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("strict-transport-security"))
        .map(|(_, v)| v);
    let Some(wert) = hsts_wert else {
        return;
    };
    if !ist_https(anfrage) {
        s.set_cookie_ignoriert(sitzung::GRUND_HSTS_UEBER_HTTP, 1);
        return;
    }
    // DER WERT WIRD GEPRUEFT, bevor er gespeichert wird. RFC 6797 6.1:
    // der Wert ist `max-age` und optional `includeSubDomains`. Ein
    // Wert, der das nicht ist, wird **nicht** gespeichert — ein
    // zufaelliger Header mit `max-age` darin waere sonst ein Eintrag
    // mit 90 Sekunden, und der Benutzer haelt sich fuer geschuetzt,
    // obwohl er es nicht ist.
    let (max_age, subdomains) = hsts_wert_lesen(wert);
    match max_age {
        Some(a) => s.hsts_gesehen(domain, a, subdomains),
        None => s.set_cookie_ignoriert(sitzung::GRUND_HSTS_UNLESBAR, 1),
    }
}

/// Liest einen `Strict-Transport-Security`-Wert.
///
/// added 30.09.2026, Welle 7 / N6. DER WEG, und es ist bewusst
/// streng: `max-age` **muss** eine Zahl sein (RFC 6797 6.1), und ein
/// Wert mit etwas anderem wird zurueckgewiesen.
///
/// DIE GRENZE, und sie ist eine echte: `MAX_HSTS_ZEICHEN`. Ein
/// `max-age` mit 30 Ziffern ist ein `u64`-Ueberlauf und wuerde mit
/// `parse` scheitern — aber ein Wert mit 100 000 Ziffern **muesste**
/// vorher abgeschnitten werden, sonst arbeitet `parse` auf einem
/// Benutzerpuffer. 256 Zeichen sind fuer die groesste plausible Zahl
/// (`max-age` bis 2^64 hat hoechstens 20 Ziffern) das Zehnfache.
const MAX_HSTS_ZEICHEN: usize = 256;

/// DER WERT 256, mit derselben Rechnung wie oben: `u64::MAX` hat 20
/// Ziffern, der Rest ist Reserve fuer Unsinn wie `max-age=99999999999999999999999; includeSubDomains`.
pub const MAX_HSTS_LAENGE: usize = MAX_HSTS_ZEICHEN;

/// Zerlegt `max-age=<zahl>` und `includeSubDomains`.
///
/// added 30.09.2026, Welle 7 / N6. RFC 6797 Abschnitt 6.1.
fn hsts_wert_lesen(wert: &str) -> (Option<u64>, bool) {
    let mut max_age: Option<u64> = None;
    let mut subdomains = false;
    // M3: die Laenge ist vor dem Iterieren geprueft (siehe
    // `MAX_HSTS_ZEICHEN`), und die Zahl der Teilstuecke ist durch die
    // Laenge begrenzt — ein `Wert` mit 256 `;` ergibt 257 Teile, und
    // der Speicher dafuer ist durch die Zeichenzahl gedeckelt.
    for teil in wert.split(';').take(MAX_HSTS_ZEICHEN) {
        let teil = teil.trim();
        if teil.is_empty() {
            continue;
        }
        let (name, arg) = match teil.split_once('=') {
            Some((n, a)) => (n.trim(), Some(a.trim())),
            None => (teil, None),
        };
        if name.eq_ignore_ascii_case("max-age") {
            // DER GRUND FUER DAS `+`: `+3600` ist nach RFC 6797 nicht
            // erlaubt, `3600` schon; ein `parse`-Fehler bei `+3600`
            // wuerde den Eintrag verwerfen, was harmlos waere, aber ein
            // `u64`-Unterlauf bei `-1` waere es nicht.
            if let Some(a) = arg.and_then(|a| a.parse::<u64>().ok()) {
                max_age = Some(a);
            }
        } else if arg.is_none() && name.eq_ignore_ascii_case("includeSubDomains") {
            subdomains = true;
        }
    }
    (max_age, subdomains)
}

/// Der Pool-Schluessel einer Anfrage.
///
/// DER WEG: aus Schema, Host und Port, genau den drei Dingen, die eine
/// TCP-Verbindung binden. Siehe `pool::Schluessel` fuer die Begruendung,
/// warum das Schema hineingehört.
/// added 03.10.2026, Rolle NETZTEMPO. **`pool_schluessel`, fuer die geteilte
/// Verwaltung.** Es ist eine Zeile, und sie wird jetzt an **zwei** Orten
/// gebraucht — dem Abrufweg und `h2geteilt`. Ohne diese Fassung haette
/// `h2geteilt` seinen Host aus dem `Request` **selbst** gebaut, und das waere
/// die zweite Stelle, die entscheidet, was ein Host ist.
pub(crate) fn pool_schluessel_of(request: &Request) -> pool::Schluessel {
    pool_schluessel(request)
}

fn pool_schluessel(request: &Request) -> pool::Schluessel {
    pool::Schluessel {
        tls: ist_https(request),
        host: host_ohne_schema(&request.host).to_string(),
        port: request.port,
    }
}
/// DIE ZEITEN EINER ANFRAGE, je Phase — der Erzeugerteil von T3.
///
/// added 01.10.2026 16:8x, Rolle TLS. `AUTONOM.txt` Zeile 603 verlangt eine
/// „Netz-Ereignisquelle fuer DEVTOOLS D10 (Methode, URL, Status, Protokoll,
/// Bytes, Zeiten je Phase DNS/TCP/TLS/erstes Byte)". Der **Typ** des
/// Ereignisses liegt bei DEVTOOLS (`narsil_devtools::d4_netzwerk::NetzEreignis`,
/// `narsil-view` importiert ihn). Was **hier** entsteht, sind die Zahlen — und
/// sie koennen nur hier entstehen, weil hier der Verbindungsaufbau passiert.
///
/// WARUM EINE STRUKTUR UND KEIN `HashMap`: die Phasennamen sind fest, es
/// sind fuenf, und ein `HashMap` wuerde `Option<Duration>` pro Schluessel
/// speichern und beim Lesen `None` von „nicht gemessen" nicht unterscheiden
/// koennen. Hier heisst `None` **nicht gemessen**.
///
/// `tls` ist bei `http://` immer `None` — es gab keinen TLS-Aufbau. Das ist
/// kein Messfehler, sondern die Aussage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Phasen {
    /// Die Namensauflösung.
    pub dns: Option<std::time::Duration>,
    /// Der Aufbau der TCP-Verbindung.
    pub tcp: Option<std::time::Duration>,
    /// Der TLS-Handshake (mit ALPN und Sitzungswiederaufnahme).
    pub tls: Option<std::time::Duration>,
    /// Vom Beginn der Anfrage bis zum **ersten** Byte der Antwort.
    ///
    /// added 01.10.2026 16:8x: „erstes Byte" heisst das erste Byte des
    /// **Antwortkopfs**, nicht der letzte. Der Unterschied ist die
    /// Serverdenkzeit, und genau die will DEVTOOLS D10 sehen.
    pub erstes_byte: Option<std::time::Duration>,
    /// Die ganze Anfrage, inklusive Nachschub der Verbindung.
    pub gesamt: Option<std::time::Duration>,
}

impl Phasen {
    /// Die leere Phasenliste — fuer Tests und fuer „nichts gemessen".
    pub const fn leer() -> Self {
        Self {
            dns: None,
            tcp: None,
            tls: None,
            erstes_byte: None,
            gesamt: None,
        }
    }

    /// `true`, wenn **irgendetwas** gemessen wurde.
    ///
    /// added 01.10.2026 16:8x: ohne diese Frage muss der Aufrufer viermal
    /// `is_some()` prüfen, und er vergisst eine — das ist die Form, in der
    /// eine Zahl aus der Anzeige verschwindet.
    pub fn ist_gemessen(&self) -> bool {
        self.dns.is_some()
            || self.tcp.is_some()
            || self.tls.is_some()
            || self.erstes_byte.is_some()
            || self.gesamt.is_some()
    }
}

// DIE PHASEN DER LETZTEN ANFRAGE DIESES THREADS.
//
// added 01.10.2026 16:8x. WARUM PRO THREAD und nicht global: `fetch` ist
// eine freie Funktion ohne Kontext, es gibt also keinen Ort, an dem ein
// Wert „für diesen Ladevorgang" hingehängt. Ein globaler Puffer wäre die
// Ablage — und bei zwei Tabs schriebe der zweite Tab in die Anzeige des
// ersten. `thread_local` kostet nichts und hält die Buchführung beim
// Aufrufer, der sie braucht.
//
// added 01.10.2026 16:8x: **kein** `Mutex`. Ein `Mutex` wäre hier
// Speicher, den niemand braucht; der Thread-Zustand reicht, und er ist
// schneller.
//
// added 01.10.2026 16:8x: als `//` und nicht als `///`, weil
// `thread_local!` ein **Makro** ist und kein Doc-Kommentar an sich
// traegt. GEMESSEN: mit `///` meldete der Uebersetzer „unused doc comment".
thread_local! {
    static LETZTE_PHASEN: std::cell::Cell<Option<Phasen>> =
        const { std::cell::Cell::new(None) };
}

// added 03.10.2026, Rolle NETZTEMPO. **DIE SUMMEN PRO THREAD.**
//
// WARUM SIE FEHLTE, und es ist eine Luecke, die man beim Messen spuert:
// `phasen()` gibt die Phasen der **letzten** Anfrage. Eine Seite mit 90
// Bildern hat aber 90 Anfragen, und die Antwort auf „wo geht die Zeit
// hin?" ist genau die Frage, die ein **einzelner** Wert nicht beantworten
// kann: bei wikipedia.org liegt `erstes_byte` der **letzten** Anfrage
// gern bei 3 ms, waehrend die Summe der 90 Anfragen die halbe Seite
// ausmacht.
//
// WARUM PRO THREAD und nicht global: dieselbe Begruendung wie
// `LETZTE_PHASEN` darueber. Die Probe laedt jede Seite in **eigenen
// Threads** (`Probe::ausfuehren`, Zeitgrenze je Seite), und eine globale
// Summe wuerde die Seiten vermischen — das waere genau die Art Zahl, die
// man nicht mehr zuordnen kann.
thread_local! {
    static PHASEN_SUMMEN: std::cell::Cell<Option<PhasenSummen>> =
        const { std::cell::Cell::new(None) };
}

/// added 03.10.2026, Rolle NETZTEMPO. **Die Summe der Phasen eines
/// Threads** — die Zahl, mit der man eine Seite aufschlüsselt.
///
/// Jedes Feld ist `Option<Duration>` wie bei [`Phasen`], und die **Bedeutung
/// ist dieselbe**: `None` heisst „nicht gemessen", nicht „0 ms". `n` ist die
/// Zahl der Anfragen, **bei denen** die Phase gemessen wurde — nicht die
/// Zahl aller Anfragen. Beides auseinanderzuhalten ist wichtig: ein Pool,
/// der 60 von 90 Verbindungen wiederverwendet, hat bei 30 Anfragen
/// **kein** TLS gemessen, und das ist eine Aussage, keine Luecke.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PhasenSummen {
    /// Summe der Namensaufloesungen.
    pub dns: Option<std::time::Duration>,
    /// Wie oft wurde DNS **gemessen**.
    pub dns_n: usize,
    /// Summe der Verbindungsbauten.
    pub tcp: Option<std::time::Duration>,
    pub tcp_n: usize,
    /// Summe der TLS-Handshakes.
    pub tls: Option<std::time::Duration>,
    pub tls_n: usize,
    /// Summe „Anfrage -> erstes Byte".
    pub erstes_byte: Option<std::time::Duration>,
    pub erstes_byte_n: usize,
    /// Summe der **gesamten** Anfragedauer.
    pub gesamt: Option<std::time::Duration>,
    pub gesamt_n: usize,
}

impl PhasenSummen {
    /// added 03.10.2026, Rolle NETZTEMPO. Der **Nullwert** als Konstante,
    /// fuer `thread_local!` mit `const { .. }`.
    ///
    /// DER WEG UND NICHT `Default::default()`: `thread_local!` verlangt
    /// fuer den `const`-Block einen Ausdruck, der **konstant** ist, und
    /// `Default::default()` ist das nicht. `Option<Duration>` ist mit
    /// `None` konstant, `usize` mit 0.
    pub const ZERO: PhasenSummen = PhasenSummen {
        dns: None,
        dns_n: 0,
        tcp: None,
        tcp_n: 0,
        tls: None,
        tls_n: 0,
        erstes_byte: None,
        erstes_byte_n: 0,
        gesamt: None,
        gesamt_n: 0,
    };
}

/// added 03.10.2026, Rolle NETZTEMPO. **Welche Phase** addiert wird.
///
/// Eigener Typ statt vier Funktionsnamen: [`phasen_summe_teil`] bekommt
/// damit aus der **Anrufstelle** heraus sichtbar, welches Feld gemeint ist,
/// und die Zuordnung kann sich nicht beim Umstellen einer Zeile verschieben.
#[derive(Debug, Clone, Copy)]
enum Teil {
    Dns,
    Tcp,
    Tls,
    ErstesByte,
    Gesamt,
}

/// added 03.10.2026, Rolle NETZTEMPO. **Rechnet eine Phase dazu.** Nur von
/// innen benutzt; oeffentlich ist [`phasen_summen`].
///
/// `None` heisst „diese Phase wurde bei dieser Anfrage nicht gemessen" —
/// etwa TLS bei einer aus dem **Pool** genommenen Verbindung — und wird
/// **nicht** als Null addiert. Das ist der Unterschied zwischen „wir haben
/// 0 ms TLS gebraucht" und „wir brauchten gar kein TLS".
fn phasen_summe_teil(phase: Option<std::time::Duration>, teil: Teil) {
    let Some(d) = phase else {
        return;
    };
    PHASEN_SUMMEN.with(|s| {
        let mut k = s.get().unwrap_or_default();
        let (summe, n) = match teil {
            Teil::Dns => (&mut k.dns, &mut k.dns_n),
            Teil::Tcp => (&mut k.tcp, &mut k.tcp_n),
            Teil::Tls => (&mut k.tls, &mut k.tls_n),
            Teil::ErstesByte => (&mut k.erstes_byte, &mut k.erstes_byte_n),
            Teil::Gesamt => (&mut k.gesamt, &mut k.gesamt_n),
        };
        *summe = Some(summe.unwrap_or(std::time::Duration::ZERO) + d);
        *n += 1;
        s.set(Some(k));
    });
}

/// added 03.10.2026, Rolle NETZTEMPO. Die Phasensummen **dieses** Threads,
/// oder `None`, wenn hier noch nichts gemessen wurde.
///
/// `None` statt eines Null-Werts, weil dieselbe Regel wie bei [`phasen`]
/// gilt: „nichts gemessen" und „0 ms" sind zwei verschiedene Aussagen, und
/// in einer Zeitmessung ist die Verwechslung ein Fehler.
pub fn phasen_summen() -> Option<PhasenSummen> {
    PHASEN_SUMMEN.with(|s| s.get())
}

/// Wie viele Anfragen **dieser Thread** gemessen hat.
///
/// added 01.10.2026 16:8x: `AUTONOM.txt` 603 nennt `cache::statistik()`
/// durchreichen; ein Zähler ist das, was man durchreichen kann, ohne die
/// Struktur des Fremdcaches zu kennen. `0` heisst: hier wurde noch nichts
/// gemessen — nicht: es gab keine Anfrage.
pub fn phasen_zaehler() -> u64 {
    PHASEN_ZAEHLER.with(|z| z.get())
}

// added 01.10.2026 16:8x: der Zaehler liegt neben dem Wert, nicht in ihm.
// Grund: `Phasen` ist ein **Wert** (kopierbar, vergleichbar) und soll
// bleiben; ein Zaehler darin wuerde ihn beim Kopieren mitkopieren, und die
// Kopie zeigte eine Zahl, die nicht ihr gehoert.
thread_local! {
    static PHASEN_ZAEHLER: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Die Phasen der letzten Anfrage, oder `None`.
///
/// added 01.10.2026 16:8x: `None` heisst „hier wurde noch keine Anfrage
/// gestellt", nicht „die Anfrage lief in 0 ms". Beides zu vermischen ist
/// die Art von Fehler, die in einer Anzeige als „0 ms" auftaucht.
pub fn phasen() -> Option<Phasen> {
    LETZTE_PHASEN.with(|p| p.get())
}

/// Setzt die Phasen zurück — für Tests und für „neuer Ladevorgang".
///
/// added 01.10.2026 16:8x: der **Zähler** wird mitgesetzt, weil er sonst
/// über die Grenze einer Messung hinaus zählt. Wer zurücksetzt, will eine
/// Zahl ab null.
pub fn phasen_zuruecksetzen() {
    LETZTE_PHASEN.with(|p| p.set(None));
    // added 03.10.2026, Rolle NETZTEMPO: **AUCH DIE SUMMEN.** Ohne diese
    // Zeile misst man den zweiten Ladevorgang eines Tabs mit den Zeiten des
    // ersten — und merkt es nicht, weil beide Zahlen plausibel aussehen.
    PHASEN_SUMMEN.with(|s| s.set(None));
    PHASEN_ZAEHLER.with(|z| z.set(0));
}

/// Schreibt die Phasen — und **misst dabei nicht**.
///
/// added 01.10.2026 16:8x, T3. DAS IST DER WICHTIGE PUNKT, und er ist der
/// Grund fuer die ganze Aufteilung: der Verbindungsaufbau ruft diese
/// Funktion **zweimal** (einmal ohne TLS, einmal mit) und `fetch` noch
/// einmal. Jedes Mal wäre ein Zähleraufruf ein zweiter Eintrag — und
/// `phasen_zaehler()` würde die **dreifache** Zahl einer Anfrage nennen.
///
/// DARUM: `phasen_setzen` **schreibt nur**, und der Zähler wird genau einmal
/// erhöht, nämlich in `phasen_abschliessen` am Ende von `fetch`.
///
/// `privat`, weil eine oeffentliche Schreibstelle eine Einladung wäre, sie
/// von aussen zu benutzen und damit den Zähler zu verfaelschen.
fn phasen_setzen(p: Phasen) {
    LETZTE_PHASEN.with(|z| {
        // added 01.10.2026 16:9x. **MERGEN, NICHT UEBERSCHREIBEN** — und
        // das war ein echter Fehler in der ersten Fassung. GEMESSEN, warum:
        // `verbinde` schreibt `tcp`/`tls`, und der Aufrufer schrieb danach
        // `dns` mit `..Phasen::leer()`. Mit blossem Setzen verlor die
        // Anzeige TCP und TLS wieder, und `erstes_byte` waere nie gekommen.
        //
        // DIE REGEL: ein `Some` im Argument **gewinnt**, ein `None` laesst
        // den gemessenen Wert stehen. Damit ist die Reihenfolge der
        // Schreibstellen egal — und das ist der Grund fuer die Regel, nicht
        // nur ihre Foerung: `verbinde_neu` und `verbinde` sind zwei
        // Funktionen, und welche zuerst fertig ist, haengt davon ab, ob es
        // TLS gab.
        let alt = z.get().unwrap_or_else(Phasen::leer);
        z.set(Some(Phasen {
            dns: p.dns.or(alt.dns),
            tcp: p.tcp.or(alt.tcp),
            tls: p.tls.or(alt.tls),
            erstes_byte: p.erstes_byte.or(alt.erstes_byte),
            gesamt: p.gesamt.or(alt.gesamt),
        }));
    });
}

/// Schreibt die Phasen **endgueltig** und zaehlt.
///
/// added 01.10.2026 16:8x: die einzige Stelle, die `PHASEN_ZAEHLER` erhoeht.
/// `fetch` ruft sie genau einmal, am Ende — auch im Fehlerfall, denn ein
/// Fehler ist ein **Ergebnis**: die Zeit bis zum Fehler gehoert in die
/// Anzeige, sonst sieht ein Fehlschlag aus wie ein sofortiger Erfolg.
fn phasen_abschliessen(p: Phasen) {
    // added 01.10.2026 16:9x. **MERGEN**, und das war ein echter Fehler:
    // die Hülle um `hole_und_sende` ruft diese Funktion mit
    // `..Phasen::leer()`, und ein blosses `set` hat dabei `dns`, `tcp` und
    // `tls` **wieder weggeschrieben**.
    //
    // GEMESSEN (Punkt 1 des Tests `die_phasen_einer_anfrage_sind_gemessen`):
    //     Phasen { dns: None, tcp: None, tls: None,
    //               erstes_byte: Some(760ns), gesamt: Some(61.94235ms) }
    // Die beiden Phasen, die der Verbindungsaufbau gemessen hatte, waren
    // weg — und die Anzeige haette „kein DNS, kein TCP, kein TLS"
    // behauptet, waehrend der Aufbau **stattgefunden** hatte.
    //
    // DIE REGEL, und sie gilt fuer beide Schreibstellen: ein `Some` im
    // Argument gewinnt, ein `None` laesst den gemessenen Wert stehen. Der
    // Unterschied zwischen den Stellen ist nur der Zaehler.
    LETZTE_PHASEN.with(|z| {
        let alt = z.get().unwrap_or_else(Phasen::leer);
        z.set(Some(Phasen {
            dns: p.dns.or(alt.dns),
            tcp: p.tcp.or(alt.tcp),
            tls: p.tls.or(alt.tls),
            erstes_byte: p.erstes_byte.or(alt.erstes_byte),
            gesamt: p.gesamt.or(alt.gesamt),
        }));
    });
    PHASEN_ZAEHLER.with(|z| z.set(z.get() + 1));
    //
    // added 03.10.2026, Rolle NETZTEMPO. **DIE SUMMEN — und zwar HIER,
    // also NACH dem Merge.** Das ist die Reihenfolge, die mein eigener
    // Kommentar oben vorschrieb und die ich zuerst **nicht** eingehalten
    // habe.
    //
    // GEMESSENER FEHLER, und er ist der Grund fuer die Reihenfolge:
    // mit dem Aufruf **vor** dem Merge meldete der erste Lauf
    //     dns (n=0) | tcp (n=0) | tls (n=0) | erstes_byte 26.3 ms
    // fuer de.wikipedia — also **kein** DNS, **kein** TCP und **kein** TLS,
    // obwohl der Pool leer war und die Verbindung **neu** gebaut werden
    // musste. Grund: `hole_und_sende` ruft diese Funktion mit
    // `..Phasen::leer()`, und `hole_und_sende` liefert **danach** erst das
    // `gesamt`. Vor dem Merge summiert man also die **leere** Huelle, und
    // `gesamt` fehlte spaeter in der Summe.
    //
    // **DIE REGEL, die daraus folgt:** summieren, was **steht** — und
    // stehen hat erst der Merge. Dieselbe Regel hat der Kommentar ein
    // paar Zeilen weiter oben fuer `erstes_byte` schon einmal festgehalten;
    // sie gilt fuer alle Felder und fuer die Summe.
    LETZTE_PHASEN.with(|z| {
        if let Some(v) = z.get() {
            phasen_summe_teil(v.gesamt, Teil::Gesamt);
            phasen_summe_teil(v.dns, Teil::Dns);
            phasen_summe_teil(v.tcp, Teil::Tcp);
            phasen_summe_teil(v.tls, Teil::Tls);
            phasen_summe_teil(v.erstes_byte, Teil::ErstesByte);
        }
    });
}

/// Baut eine **neue** Verbindung auf: Adresse aufloesen, IPv4 und IPv6
/// der Reihe nach probieren, TLS dazwischen.
///
/// WARUM ALS EIGENE FUNKTION (Welle 7, Auftrag N4): der Pool braucht
/// einen Aufbau-Weg, den er selbst aufrufen kann, ohne dass der
/// ganze HTTP-Pfad noch einmal kopiert wird. Vorher stand dieser
/// Block am Anfang von `sende`; jetzt ist er der Rueckfall, wenn der
/// Pool nichts hergibt.
fn verbinde_neu(
    request: &Request,
    limits: &Limits,
    frist: &Frist,
    tls: Option<&TlsAngaben>,
) -> Result<Box<dyn Leitung>, NetError> {
    // 1. Adresse. Mehrere Adressen (IPv4 + IPv6) werden nacheinander
    //    versucht; der Host entscheidet, welcher zuerst kommt.
    let adresse = socket_text(host_ohne_schema(&request.host), request.port);
    spur("verbinde_neu");
    // added 01.10.2026 16:8x, T3. DIE DNS-PHASE, gemessen. `Instant::now()`
    // **vor** `to_socket_addrs` — und die Dauer wird nicht aus zwei
    // Zeitpunkten gerechnet, sondern aus `elapsed()`. GEMESSEN, warum das
    // ein Unterschied ist: `to_socket_addrs` kann den Aufloeser asynchron
    // nutzen, und eine Rechnung aus zwei Zeitpunkten traegt die Zeit des
    // **eigenen Codes dazwischen** mit, nicht die des Netzes.
    let dns_start = std::time::Instant::now();
    // added 06.10.2026 17:1x, Rolle NETZTEMPO. **DER CACHE-ZUGRAMM**, und er
    // ersetzt genau einen Aufruf. `dns::loese` macht bei einem gültigen
    // Eintrag **keinen** Systemaufruf, sondern gibt die Liste aus dem Cache
    // zurück — dieselbe Liste, in derselben Reihenfolge.
    //
    // GEMESSEN, was das kauft: `Phasen.dns` war an `www.rust-lang.org`
    // zweimal **31,66 ms** und **90,56 ms** (01.10.2026, Belege in
    // `lib.rs`). Bei einer Seite mit 30 Unterressourcen von 4 Hosts sind
    // das 30 Aufrufe fuer 3 verschiedene Namen. Diese Zahl steht **vor**
    // dem TCP-Handshake, also auf dem kritischen Pfad.
    //
    // WARUM DIE FEHLERFORM UNVERAENDERT BLEIBT: `dns::loese` gibt den
    // Text von `io::Error` als `String` zurueck, und der `format!`
    // darunter ist derselbe wie vorher. Das Verhalten im Fehlerfall ist
    // damit **bitgleich** — der Cache cacht absichtlich **keine** Fehler
    // (Begruendung im Kopf von `dns.rs`).
    let kandidaten = dns::loese(&adresse).map_err(|e| NetError::Name(format!("{adresse}: {e}")))?;
    let dns = dns_start.elapsed();
    if kandidaten.is_empty() {
        return Err(NetError::Name(format!("{adresse}: keine Adresse")));
    }
    let mut letzter: Option<NetError> = None;
    for k in kandidaten {
        // DER EINZIGE UNTERSCHIED zwischen http und https an dieser
        // Stelle: der Aufbau. Der Rest des HTTP-Pfadens sieht eine
        // `Box<dyn Leitung>` und kann nicht unterscheiden, was
        // darunter liegt — das ist der Punkt der Box.
        let versuch = verbinde(&k, limits.verbinden.min(frist.rest()?), tls);
        match versuch {
            Ok(s) => {
                // added 01.10.2026 16:9x: nach `verbinde`, und das ist mit
                // dem **Merge** in `phasen_setzen` gleichgueltig — vorher
                // stand hier ein Setzen, das `tcp` und `tls` wieder
                // wegschrieb.
                phasen_setzen(Phasen {
                    dns: Some(dns),
                    ..Phasen::leer()
                });
                return Ok(s);
            }
            // Ein TLS-Fehler ist ein BEFUND, kein Versuch: die zweite
            // Adresse wird ihn nicht heilen. Ein Zertifikat, das nicht
            // stimmt, bleibt falsch, egal ob IPv4 oder IPv6.
            //
            // Das ist eine bewusste Aenderung gegenueber `io::Error`:
            // dort sammelt die Schleife den letzten Fehler und
            // versucht weiter, und bei einem abgelehnten Zertifikat
            // waere der zweite Versuch nur eine zweite
            // Zertifikatspruefung mit demselben Ergebnis.
            Err(e @ NetError::Tls(_)) => return Err(e),
            Err(NetError::ZeitlimitVerbinden) => {
                return Err(NetError::ZeitlimitVerbinden);
            }
            Err(e) => {
                // Nur der LETZTE Fehler wird gemeldet, aber keiner wird
                // weggeworfen: ohne das ist die Meldung leer.
                letzter = Some(e);
            }
        }
    }
    Err(NetError::Connect(format!(
        "{adresse}: {}",
        letzter
            .map(|e| e.to_string())
            .unwrap_or_else(|| "keine Adresse".into())
    )))
}

fn sende(
    request: &Request,
    limits: &Limits,
    frist: &Frist,
    eigener_speicher: Option<&narsil_tls::kette::Vertrauensspeicher>,
) -> Result<Response, NetError> {
    // Der TLS-Auftrag, EINMAL fuer die ganze Anfrage. Er entsteht auch
    // dann, wenn die Verbindung am Ende gar nicht zustande kommt —
    // das Bundle wird aber nur gelesen, wenn HTTPS wirklich benutzt
    // wird, sonst waere jede http-Anfrage einen Lesevorgang teurer.
    //
    // added 01.10.2026, R3: `eigener_speicher` ist der von
    // `fetch_mit_speicher` gegebene Vertrauensspeicher. `None` heisst
    // „den globalen nehmen" — der Weg, den `fetch` geht.
    spur("sende");
    let tls = if ist_https(request) {
        let speicher = match eigener_speicher {
            Some(s) => s,
            None => vertrauensspeicher()?,
        };
        spur("vertrauensspeicher da");
        Some(TlsAngaben {
            host: host_ohne_schema(&request.host),
            speicher,
            jetzt: jetzt_in_sekunden(),
            // added 03.10.2026 18:5x: **AUS DEN LIMITS, NICHT AUS DER UMGEBUNG.**
            // Die Policy ist eine Eigenschaft des **Abrufs** (der Aufrufer
            // entscheidet), nicht der Bibliothek — deshalb steht sie in
            // [`Limits`] und wandert von dort hierher.
            widerruf: limits.widerruf,
        })
    } else {
        None
    };

    // W7 / N4: der Pool. `gepoolt` sagt, ob die benutzte Leitung aus
    // dem Pool kam — nur dann darf ein Fehler einmal wiederholt
    // werden.
    let schluessel = pool_schluessel(request);
    let roh = request.to_bytes();
    // M3: die harte Obergrenze der Schleife unten.
    //
    // DER WERT: `pool::MAX_GESAMT + pool::MAX_WIEDERHOLUNGEN + 1`.
    //
    // DIE RECHNUNG, und sie ist der Grund fuer die Form des Wertes:
    //   * Jeder Durchlauf, der eine **tote gepoolte** Leitung findet,
    //     nimmt eine davon aus dem Pool weg. Der Pool haelt hoechstens
    //     `MAX_GESAMT` Verbindungen, also kann das hoechstens
    //     `MAX_GESAMT`-mal geschehen.
    //   * Jeder Durchlauf, der wirklich **sendet**, verbraucht eine der
    //     `MAX_WIEDERHOLUNGEN` erlaubten Wiederholungen, und der
    //     erste gehoert nicht dazu: `+ 1`.
    // Die Summe ist damit eine echte Obergrenze, nicht eine Schranke
    // "nach Gefuehl". Ohne sie waere die Schleife bei einem Pool, der
    // nur aus geschlossenen Verbindungen besteht, eine Endlosschleife.
    let grenze_durchlaeufe = pool::MAX_GESAMT + pool::MAX_WIEDERHOLUNGEN + 1;
    let mut wiederholungen = 0usize;
    let mut durchlauf = 0usize;
    //
    // added 06.10.2026, Rolle NETZTEMPO. **DER NEUVERSUCH BAUT NEU**, und das
    // ist ein **Zustand**, kein Aufruf — der Aufruf waere an zwei Stellen
    // (Setzen hier, Ausloesen dort), und zwei Stellen sind zwei Wahrheiten.
    //
    // **GEMESSEN, warum es ueberhaupt noetig ist** (`tests/n3_vergiss.rs`): mit
    // `pool::vergiss` an der Fehlerstelle kostete **ein** `GOAWAY` bis zu
    // `MAX_PRO_HOST - 1` = 5 unbenutzte Verbindungen; ohne die Zeile nahm der
    // Neuversuch eine **gepoolte** Leitung und ein **zweites** `GOAWAY` wurde
    // zum Ressourcenausfall (`Err: … (GOAWAY …)`). Beides ist gemessen, beides
    // ist falsch.
    //
    // **WER IHN SETZT:** der h2-Fehlerzweig. **WER IHN LOEST:** die Ausleihe
    // unten — und **nur** dort, sonst waere es eine zweite Bedeutung.
    let mut neuversuch_baut_neu = false;
    while durchlauf < grenze_durchlaeufe {
        durchlauf += 1;
        //
        // added 01.10.2026 22:5x, R5e. DIE AUSLEIHE, und sie bringt den
        // **h2-Strom** mit — der gehoert zur Verbindung, nicht zur Anfrage.
        // GEMESSEN, warum das der entscheidende Unterschied ist: jede
        // Anfrage machte eine neue TCP- und TLS-Verbindung
        // (`pool offen 0` nach jedem Abruf), und daraufhin schlossen
        // `www.mozilla.org` (EOF nach 128 ms) und `archlinux.org` (221 ms)
        // die zweite Verbindung, waehrend `www.google.com` vier in Folge
        // verkraftete. Das ist der Grund fuer T4, gemessen.
        // added 02.10.2026 02:9x: VOR DER LEITUNG, und der Grund ist die
        // Messung: `wiederaufnahme_versuche()` ist ein **Zaehler**, und nur
        // die Differenz vor/nach dem Ausleihen sagt, ob **diese** Verbindung
        // mit einem Ticket aufgebaut wurde. Bei einer gepoolten Leitung
        // passiert kein Handshake, der Zaehler bleibt — und das stimmt
        // auch: eine gepoolte Leitung wurde schon beim ersten Aufbau
        // entschieden.
        let wv_vor = wiederaufnahme_versuche();
        //
        // added 06.10.2026, Rolle NETZTEMPO. **DER NEUVERSUCH UMGEHT DEN POOL.**
        // Der Zweig ist derselbe Aufruf mit derselben `verbinde_neu`-Frist —
        // es gibt keinen zweiten Dialer, nur keine Pool-Ausleihe. Und die
        // Zustaendigkeit wird **hier** zurueckgesetzt: sie gilt fuer **eine**
        // Runde, sonst wuerde der dritte Durchlauf wieder aus dem Pool nehmen.
        let dial = || verbinde_neu(request, limits, frist, tls.as_ref());
        let mut ausleihe = if neuversuch_baut_neu {
            neuversuch_baut_neu = false;
            pool::hole_oder_verbinde_ohne_pool(&schluessel, dial)?
        } else {
            pool::hole_oder_verbinde_mit_strom(&schluessel, dial)?
        };
        let mut stream = ausleihe.leitung;
        let gepoolt = ausleihe.gepoolt;
        // added 02.10.2026 02:9x: UND DAS FLAG, und es ist das einzige neue
        // Feld in diesem Durchlauf. `false` bei einer gepoolten Leitung, weil
        // kein Handshake lief.
        let wiederaufgenommen = wiederaufnahme_versuche() > wv_vor;
        // added 01.10.2026 00:5x, R5b: `hpack` wird **hier** herausgelesen,
        // weil `ausleihe.leitung` schon bewegt ist. Die HPACK-Tabelle bleibt
        // also an der Ausleihe haengen und wird spaeter mit der Verbindung
        // zurueckgelegt.
        // DER STROM: 0 heisst „die Leitung ist neu oder spricht kein h2",
        // und dann ist es Strom 1 — der erste Client-Strom (ungerade, 5.1).
        let strom = if ausleihe.strom == 0 {
            1
        } else {
            ausleihe.strom
        };
        // DIE TOTE VERBINDUNG, BEVOR DIE ANFRAGE RAUSGEHT. Das ist der
        // Unterschied zu einem blossen Wiederholen nach dem Fehler und
        // der Grund, warum es ueberhaupt dieses Verfahren braucht:
        // schickt der Browser die Anfrage in einen geschlossenen
        // Sockel, ist fuer den Server nicht mehr feststellbar, ob er
        // sie **ueberhaupt** bekommen hat — und bei `POST` waere ein
        // Wiederholen dann ein moeglich zweimal ausgefuehrter Vorgang
        // (RFC 9110 9.2.2). Hier wird deshalb **nichts gesendet**,
        // sondern nur die Leitung verworfen; das Zaehlen der
        // Wiederholungen bleibt unberuehrt, weil kein Fehler entstanden
        // ist, sondern eine tote Leitung.
        if gepoolt && !stream.lebt_noch() {
            // added 06.10.2026 17:4x, Rolle NETZTEMPO: **ZAEHLER**. Das ist
            // die einzige Stelle im ganzen Abrufweg, an der eine Leihabe
            // **weggeworfen** wird, ohne dass ein Fehler entstanden waere —
            // und damit die einzige, an der sich misst, ob die
            // Leerlauf-Frist von 60 s zu **lang** ist. GEMESSEN wird die
            // Zahl in `examples/tempo.rs`.
            //
            // WARUM DIESE STELLE UND KEINE ANDERE: die h2-Variante weiter
            // unten (R5d) verwirft ebenfalls, aber erst **nach** dem
            // Schreiben der Anfrage. Das sind zwei verschiedene Faelle
            // mit zwei verschiedenen Ursachen, und sie werden getrennt
            // gezaehlt.
            pool::verworfen_zaehlen();
            // added 04.10.2026, Rolle NETZTEMPO: **WER SCHLIESSEN ZUERST** —
            // der Grund steht als Text in der Notiz, weil „wir schliessen 4x"
            // ohne Grund die halbe Antwort ist.
            crate::h2geteilt::wir_schliessen("Dial verworfen (Schreiben der Anfrage)");
            let _ = stream.shutdown(Shutdown::Both);
            continue;
        }
        // Nagle wuerde kleine Antworten verzoegern.
        let _ = stream.set_nodelay(true);

        // added 01.10.2026 12:0x UTC (Projektlabel 02.10.2026), R4z.
        // DER H2-ZWEIG, und ohne ihn redet der Browser **Unsinn**.
        //
        // GEMESSEN: `Empfangen fehlgeschlagen: zu viel vom Server: Record zu
        // lang`. Das heisst: der Server hat unsere HTTP/1.1-Anfrage als
        // **TLS-Record** gelesen, und die war laenger als ein Record. Der
        // Fehler ist also nicht beim Server, sondern bei uns — wir haben
        // `roh` (HTTP/1.1-Text) unveraendert auf eine h2-Verbindung
        // geschrieben.
        //
        // WARUM DAS NICHT EINFACH GEHT und warum ich es hier nicht
        // ausbaue: ueber h2 braucht es (a) die Client-Vorfahrt, (b) ein
        // `SETTINGS`, (c) den `HEADERS`-Ramen aus `h2strom::baue_headers`
        // und (d) den `DATA`-Ramen fuer den Koerper — und dann **lesen**:
        // die Antwort kommt als Rahmenreihe, nicht als Bytekette. Das ist
        // der Rest von T2 und der naechste Punkt.
        //
        // BIS DHER: wir schreiben **keinen** HTTP/1.1-Text auf eine
        // h2-Verbindung, sondern geben einen benannten Fehler. Das ist
        // ehrlicher als eine kaputte Anfrage, und es ist die Stelle, an
        // der ein Browser nichts laedt statt etwas Falsches zu laden.
        //
        // DIE ENTSCHEIDUNG, und sie ist eine **Rechenentscheidung**:
        // h2 nur anzubieten und dann nichts koennen ist schlechter als h2
        // gar nicht anzubieten — der Server waehlt es und wir versagen.
        // Also: anbieten ja, benutzen nein, bis der Zweig steht.
        // added 01.10.2026 14:0x, R4z7. DER H2-WEG, und er ersetzt den
        // benannten Fehler, der hier seit 12:0x stand.
        //
        // WAS VORHER STAND: `return Err(NetError::H2NochNichtAusgebaut)`.
        // Die Begruendung im Kommentar war richtig — ueber h2 braucht es
        // Client-Vorfahrt, `SETTINGS`, `HEADERS` und dann das **Lesen** als
        // Rahmenreihe — und sie sagte ausdruecklich, dass das der Rest von
        // T2 ist. Es ist jetzt gebaut: `h2client`.
        //
        // DIE VIER SCHRITTE, in der Reihenfolge von RFC 9113 3.4:
        //   1. Preface + `SETTINGS`  (`h2client::schreibe_vorlauf`)
        //   2. `HEADERS` mit `END_HEADERS`, dazu `END_STREAM` bei GET
        //      (`h2client::schreibe_anfrage`)
        //   3. Antwort-Rahmen lesen, `SETTINGS` quittieren (`h2client::lies_antwort`)
        //   4. `HPACK`-Dekodierung der Antwortkopfzeilen (drin in `nimm_auf`)
        //
        // WARUM DIE REIHENFOLGE NICHT VERTAUSCHBAR IST: der Server erkennt
        // HTTP/2 am Preface. Ohne ihn antwortet er wie auf HTTP/1.0.
        // Umgekehrt darf der Client kein `HEADERS` schicken, bevor sein
        // `SETTINGS` draussen ist — RFC 9113 3.5: „Clients MUST NOT send
        // any frames other than SETTINGS until the server's SETTINGS have
        // been received". Punkt 3 haengt daran, also ist die Reihenfolge
        // hier **nicht** die, die spaeter folgt.
        if stream.ist_h2_gewahlt() {
            //
            // added 04.10.2026, Rolle NETZTEMPO. **DIE MERKLISTE, und hier ist
            // die **richtige** Stelle fuer den Eintrag „h2":** hier ist die
            // Kennung **entschieden** — dieser ganze Zweig ist nur der Weg,
            // den ein h2-Server geht. In `waerme_auf` (derselbe Auftrag) war
            // dieselbe Abfrage **GEMESSEN `false`** (Bericht 23), und dort ist
            // sie es auch: das ist **kein** Fehler der Abfrage, sondern der
            // Zeitpunkt. **Diese** Zeile ist der Zeitpunkt.
            pool::protokoll_merken(&schluessel.host, true);
            //
            // added 01.10.2026 21:0x, R5d. **EINE GEPOOLTE h2-VERBINDUNG
            // KANN ZWISCHEN DER PRUEFUNG UND DEM LESEN STERBEN**, und dann
            // bricht der Browser ab, statt neu zu verbinden.
            //
            // GEMESSEN, 01.10.2026 21:0x, `tests/h2_live.rs` gegen
            // `www.mozilla.org`:
            //     HTTP/2: der Rahmenkopf hat 0 Byte, er braucht 9
            // Der Server hatte die Verbindung zwischen zwei Läufen
            // geschlossen; `lebt_noch()` sah sie noch als lebendig, das
            // Schreiben ging in den lokalen Puffer, und das Lesen bekam 0
            // Byte — das ist EOF, und `lies_antwort` meldete es als
            // „Kopf zu kurz".
            //
            // DER WEG, und er ist derselbe wie bei h1 direkt oberhalb: nicht
            // die Anfrage wiederholen, sondern die **Leitung** verwerfen und
            // neu verbinden. Bei `GET` waere ein Wiederholen der Anfrage
            // harmlos, bei `POST` nicht (RFC 9110 9.2.2) — deshalb wird hier
            // gar nichts gesendet.
            //
            // WENN ICH DAS FALSCH ERKENNE, bleibt der Fehler sichtbar: die
            // Form `KopfZuKurz` kommt nur bei EOF **und** nur, wenn es
            // wirklich EOF war.
            let antwort = match hole_ueber_h2(
                &mut stream,
                request,
                &roh,
                strom,
                !gepoolt,
                &mut ausleihe.hpack,
            ) {
                Ok(a) => a,
                // added 01.10.2026 21:2x, R5d: `pool::vergiss` heisst hier
                // „nicht zuruecklegen" — `verwerfen` gibt es nicht, und der
                // Name ist der von `pool.rs` selbst gewaehlte.
                //
                // UND DIE BEDINGUNG, und sie ist absichtlich eng: **nur** eine
                // gepoolte Leitung, und **nur** EOF vor dem ersten Rahmen.
                // Ein echter Protokollfehler wird nicht wiederholt, und eine
                // frisch aufgebaute Leitung, die sofort EOF liefert, auch
                // nicht — dann stimmt etwas nicht, und das Wiederholen
                // verdeckt es nur.
                //
                // added 01.10.2026 23:8x, R5b. **DER NEUAUFBAU BEI
                // `GOAWAY`**, und `H2EndeVorzeitig` steht jetzt in derselben
                // Zeile. GEMESSEN, warum das noetig war: 01.10.2026 23:4x,
                // `tests/t4_live_tmp.rs`, drei Abrufe von `www.google.com`:
                // Lauf 0 lieferte 200 und legte die Verbindung in den Pool,
                // Lauf 1 bekam ein `GOAWAY` — und ein Fehler, obwohl genau
                // das hier der Fall fuer einen Neuaufbau ist (RFC 9113 6.8,
                // der Server hat nichts abgelehnt).
                //
                // UND DIE **METHODEN**-BEDINGUNG, added 01.10.2026 23:8x:
                // ein `continue` sendet die Anfrage **erneut**, und das ist
                // bei `POST` nach RFC 9110 9.2.2 verboten, solange nicht
                // feststeht, dass der Server die erste nicht ausgefuehrt
                // hat. Bei `GET` und `HEAD` ist das Wiederholen harmlos —
                // beide sind nach RFC 9110 9.1 (idempotent) und 9.3.2
                // (sicher) genau dafuer gedacht. GEMESSEN, warum das keine
                // theoretische Sorge ist: `sende` hatte die Wiederholung
                // schon fuer **alle** Methoden, seit R5d; ein doppelt
                // ausgefuehrter `POST` waere die Strafe fuer die
                // Wiederverwendung.
                Err(e)
                    if darf_wiederholt_werden(
                        gepoolt,
                        wiederaufgenommen,
                        &e,
                        request.method.ist_wiederholbar(),
                        wiederholungen,
                    ) =>
                {
                    // added 02.10.2026 02:9x: WENN DIE WIEDERAUFNAHME
                    // MITGESPIELT HAT, FLIEGT DAS TICKET. GEMESSEN 02:7x:
                    // genau die Verbindungen mit Ticket starben vor dem ersten
                    // Rahmen (siehe `darf_wiederholt_werden`). Ohne das hier
                    // waere der zweite Versuch mit demselben Ticket genau
                    // wieder derselbe Fehlschlag.
                    if wiederaufgenommen {
                        wiederaufnahme_verwerfen(&schluessel.host);
                    }
                    wiederholungen += 1;
                    crate::h2geteilt::wir_schliessen("Wiederaufnahme verworfen (Neuversuch)");
                    let _ = stream.shutdown(Shutdown::Both);
                    // added 06.10.2026, Rolle NETZTEMPO. **`pool::vergiss` IST
                    // WEG — und der Ersatz ist an dieser Stelle eine
                    // **Zustandsangabe**, kein Aufruf.**
                    //
                    // **GEMESSEN, warum `vergiss` falsch war** (sechs
                    // Verbindungen im Pool, **eine** betroffen, `GOAWAY` vom
                    // Server): Ressource 200, aber **Pool 1** statt 6. Die
                    // betroffene Leitung ist nicht im Pool — `hole_als_
                    // ausleihe` hat sie herausgenommen und `sende` schliesst sie
                    // zwei Zeilen oben. `vergiss` konnte also nur die **fuenf
                    // unbenutzten** treffen. Ihre Rolle war nie „Leitung
                    // merken“, sondern „Bau-zwingen“: `continue` nimmt eine
                    // Leitung, findet keine und baut neu.
                    //
                    // added 06.10.2026, Rolle NETZTEMPO. **UND WARUM NICHT
                    // EINFACH WEGGELASSEN**, was der Auftrag ausdruecklich
                    // fragt — der gemessene Ausfall bei **zwei** betroffenen
                    // Verbindungen:
                    //     Err: 127.0.0.1 hat die Verbindung beendet (GOAWAY, …)
                    // Der Neuversuch nimmt ohne diese Angabe wieder eine
                    // **gepoolte** Leitung, die dieselbe `GOAWAY`-Klasse hat,
                    // und die **eine** erlaubte Wiederholung ist verbraucht. Ein
                    // `GOAWAY` mit `NO_ERROR` — vom Server ausdruecklich **ohne**
                    // Ablehnung (RFC 9113 6.8) — wird so zum Ausfall der
                    // Ressource.
                    //
                    // **DIE REGEL, und sie ist die ganze Aenderung:** ein
                    // Neuversuch **baut** eine neue Verbindung. Sie wird in der
                    // naechsten Runde der Schleife gelesen und dort ueber
                    // [`pool::hole_oder_verbinde_ohne_pool`] eingeloest.
                    neuversuch_baut_neu = true;
                    // added 06.10.2026, Rolle NETZTEMPO. **DER FRUEHERE
                    // SATZ AN DIESER STELLE war falsch und ist ersetzt:** es stand
                    // hier „`vergiss` nimmt die Leitung aus dem Pool und
                    // schliesst sie **nicht** (das macht der `shutdown` oben)“ —
                    // aber die Leitung lag zu diesem Zeitpunkt **gar nicht** im
                    // Pool, sie war ausgeliehen. `continue` baut neu, und der
                    // Neuaufbau schickt die Vorfahrt nur auf der neuen
                    // Verbindung (`verbindung_neu = !gepoolt`) — das bleibt
                    // richtig und steht jetzt an [`pool::hole_oder_verbinde_
                    // ohne_pool`].
                    //
                    // UND `ist_wiederholbar()` ist **nicht** meine
                    // eigene Regel: die steht seit 30.09.2026 (Welle 7/N4)
                    // in `Method`, mit dem Massstab RFC 9110 9.2.2.
                    // GEMESSEN, warum das wichtig war: ich hatte fuer
                    // diese Stelle zuerst eine eigene Liste gebaut (`GET`,
                    // `HEAD`, `OPTIONS`, `TRACE`, `PUT`) — und damit zwei
                    // Orte, an denen dieselbe Frage beantwortet wird.
                    // Zweitens: `Method` kennt nur drei Werte, und eine
                    // Liste mit Werten, die es im Crate gar nicht gibt, ist
                    // eine Behauptung ohne Grundlage.
                    continue;
                }
                Err(e) => return Err(e),
            };
            if !h2client::felder_zahl_ok(&antwort) {
                return Err(NetError::H2ZuVieleFelder {
                    host: request.host.clone(),
                });
            }
            // added 01.10.2026 14:0x, R4z7. DIE FELDER, und warum sie so
            // gesetzt sind:
            //  * `set_cookie` wird **nicht** aus `headers` geholt, sondern
            //    unten in einem eigenen Durchgang — dort liegt die Logik
            //    fuer `Set-Cookie` bereits, und sie zu duplizieren hiesse
            //    zwei Stellen.
            //  * `body_quelle: KeinBody`, weil `fetch_mit` die
            //    `Content-Length`-Auswertung danach macht und ein h2-`DATA`
            //    keine `Content-Length` braucht (RFC 9113 8.1.2.6: der
            //    Koerper endet mit `END_STREAM`).
            //  * `keep_alive: Bleibt` — added 01.10.2026 22:8x, R5e:
            //    die h2-Verbindung wird zurueckgelegt. GEMESSEN, was das
            //    vorher kostete: `pool offen 0` nach **jedem** Abruf, also
            //    je Anfrage eine neue TCP- und TLS-Verbindung; daraufhin
            //    schlossen `www.mozilla.org` (EOF nach 128 ms) und
            //    `archlinux.org` (221 ms) die zweite Verbindung, waehrend
            //    `www.google.com` vier in Folge verkraftete.
            // added 01.10.2026 22:8x, R5e. DIE VERBINDUNG ZURUECK IN DEN
            // POOL, und **vor** der Antwort, weil danach `return`
            // folgt und `stream` dann nicht mehr gebraucht wird.
            //
            // DER STROM wandert mit: die naechste Anfrage darf nicht wieder
            // Strom 1 nehmen, denn der ist nach `END_STREAM` geschlossen
            // (RFC 9113 5.1).
            // added 01.10.2026 00:5x, R5b: die Tabelle wandert **mit**
            // zurueck in den Pool — sie gehoert zur Verbindung, und die
            // Verbindung geht in den Pool. Ohne das waere die Tabelle beim
            // naechsten Ausleihen leer, und `IndexUngueltig` kaeme wieder.
            // added 03.10.2026, Rolle NETZTEMPO. **DER EINZIGE ORT, AN DEM
            // EINE h2-VERBINDUNG DEN POOL VERLAESST.** `uebernehmen` gibt sie
            // nur an die geteilte Verwaltung, wenn es dafuer einen Grund gibt
            // (dort wartet jemand, oder es liegen schon geteilte Verbindungen
            // fuer diesen Host). Sonst geht sie unveraendert in den Pool, und
            // der einzelne Abrufweg verhaelt sich genau wie vorher — GEMESSEN
            // wichtig, weil das der haeufigere Fall ist.
            //
            // DER WEG FUER BEIDE ZWEIGE, und er ist **ein** `match`: die
            // Leitung wandert in **genau einem** Zweig. `uebernehmen` gibt sie
            // unveraendert zurueck, wenn es dafuer keinen Grund gibt — sonst
            // waere `stream` zweimal bewegt (E0382), und „erst fragen, dann
            // bewegen" waere ein Wettlauf, in dem die Leitung durchs Raster
            // fallen kann.
            match h2geteilt::uebernehmen(&schluessel, stream, strom, ausleihe.hpack) {
                // **Zurueckgegeben**: sie gehoert in den normalen Pool.
                Some((leitung, letzter, tabelle)) => {
                    pool::lege_ab_mit_strom(&schluessel, leitung, letzter, tabelle)
                }
                // **Uebernommen**: sie traegt jetzt viele Stroeme.
                None => {}
            }
            return response_aus_h2(request, antwort);
        }
        // 2. Senden. Schreib-Fehler UND Schreib-Timeout sind benannt
        //    getrennt, weil sie verschiedene Ursachen haben.
        if let Err(e) = schreibe(&mut stream, &roh, limits, frist) {
            // DER FEHLERFALL, DER ZUM POOL GEHOERT: eine gepoolte
            // Leitung, auf der nicht mehr geschrieben werden kann,
            // ist fast immer eine, die der Server inzwischen
            // geschlossen hat. Sie wird hier geschlossen (nicht
            // zurueckgelegt — sie ist ja tot) und, wenn die Methode
            // es erlaubt, einmal neu versucht.
            crate::h2geteilt::wir_schliessen("Verbindung tot, Neuversuch moeglich");
            let _ = stream.shutdown(Shutdown::Both);
            if darf_wiederholt_werden(
                gepoolt,
                wiederaufgenommen,
                &e,
                request.method.ist_wiederholbar(),
                wiederholungen,
            ) {
                if wiederaufgenommen {
                    wiederaufnahme_verwerfen(&schluessel.host);
                }
                wiederholungen += 1;
                continue;
            }
            return Err(e);
        }

        // 3. Lesen. Ab hier ist der Server schuld.
        let mut leser = Leser::neu(stream, limits, frist);
        match lese_antwort(&mut leser, request, limits) {
            Ok(antwort) => {
                // W7 / N4, die erste der beiden Regeln am Kopf von
                // `pool`: zurueckgelegt wird nur, was wirklich
                // wiederverwendbar ist — der Server haelt die
                // Verbindung offen **und** es liegen keine Bytes mehr
                // im Puffer. Sonst wuerde die naechste Anfrage die
                // Reste der letzten Antwort als ihre eigene
                // Statuszeile lesen.
                // W7 / N4: `entnehmen_oder_schliessen` entscheidet
                // **beides** — zuruecklegen oder schliessen. Der
                // Aufrufer kann den `Leser` danach nicht mehr
                // brauchen, weil er ihn dabei verbraucht.
                //
                // added 04.10.2026, Rolle NETZTEMPO. **UND HIER „h1".** Die
                // Merkliste braucht **beide** Antworten, sonst ist `None`
                // (unbekannt) und `waerme_auf` fasst keinen Host an.
                pool::protokoll_merken(&schluessel.host, false);
                if antwort.keep_alive == KeepAlive::Bleibt {
                    if let Some(leitung) = leser.entnehmen_oder_schliessen() {
                        pool::lege_ab(&schluessel, leitung);
                    }
                } else {
                    // Sonst bleibt bei einem Server ohne `keep-alive`
                    // ein Socket liegen, bis der Prozess endet — bei
                    // 200 Tabs sind das 200 offene Sockets.
                    leser.beenden();
                }
                return Ok(antwort);
            }
            Err(e) => {
                // Die zweite Regel: eine tote gepoolte Leitung wird
                // geschlossen und die Anfrage einmal neu gemacht.
                // `Leser::beenden` schliesst die Leitung, und der
                // Puffer wird dabei nicht zurueckgegeben — es sind
                // Reste einer kaputten Antwort darin, die niemand
                // gebrauchen kann.
                leser.beenden();
                // added 02.10.2026 02:9x: DIESE STELLE war die, an der die
                // HTML-Rolle ihren 30-S-Haenger sah: die Regel galt fuer
                // gepoolte Leitungen, und eine **frische** Verbindung, die
                // sofort EOF liefert, wurde nie wiederholt. GEMESSEN
                // 02:7x–02:8x mit `NARSIL_ALPN=h1`: genau das ist der Fehler
                // `Receive` nach 96 und 102 ms mit `wdn 1`.
                if darf_wiederholt_werden(
                    gepoolt,
                    wiederaufgenommen,
                    &e,
                    request.method.ist_wiederholbar(),
                    wiederholungen,
                ) {
                    if wiederaufgenommen {
                        wiederaufnahme_verwerfen(&schluessel.host);
                    }
                    wiederholungen += 1;
                    continue;
                }
                return Err(e);
            }
        }
    }
    // M3: die Schleife oben kehrt in jedem Zweig zurueck. Das hier ist
    // nur der Fall „die Obergrenze ist erreicht, ohne dass eine Anfrage
    // gesendet wurde" — das kann nur passieren, wenn der Pool
    // ausschliesslich aus geschlossenen Verbindungen besteht. Er ist
    // unerreichbar, aber ohne ihn muesste die Funktion `unreachable!()`
    // sagen, und das ist bei einer Fehlerbehandlung genau die Sorte
    // Stelle, an der ein naechster Umbau still ein `panic!` in den
    // Browser traegt. Der Fehlername benennt stattdessen die Ursache.
    Err(NetError::Connect(
        "Verbindungspool: alle Verbindungen waren geschlossen".to_string(),
    ))
}
/// M3: hoechstens so viele Felder in **einer** Anfrage.
///
/// added 01.10.2026 16:0x, R4z8. Ohne diese Grenze baut ein Aufrufer mit
/// 100 000 Feldern einen `HEADERS`-Ramen von mehreren MB, und der Server
/// antwortet mit `431` oder gar nicht. GEMESSEN ist der Wert nicht — er ist
/// aus `h2::MAX_FELDER_PRO_BLOCK` (512 Felder je Block) abgeleitet, plus
/// Reserve fuer die Pseudo-Felder, damit ein Aufrufer, der genau an der
/// Grenze liegt, nicht sein Feld verliert.
pub const MAX_ANFRAGE_FELDER: usize = 512;

/// DIE KOPFZEILEN EINER ANFRAGE, mit den Vorgaben des Browsers.
///
/// added 01.10.2026 16:0x, R4z8. Diese Funktion ist aus einem **gemessenen**
/// Fehler entstanden: der HTTP/1.1-Weg schrieb einen Vorgabe-`User-Agent`
/// und ein Vorgabe-`Accept`, der h2-Weg nicht. Ergebnis: dieselbe `Request`
/// bekam je nach ausgehandeltem Protokoll verschiedene Kopfzeilen, und
/// `www.wikipedia.org` antwortete nur ueber HTTP/1.1.
///
/// DIE REGEL, und sie ist die von R0: nur schreiben, was der Aufrufer
/// **nicht** gesetzt hat. `.header("accept", …)` gewinnt.
///
/// WAS NICHT HIERHER GEHOERT: `hop_by_hop` ist **kein** Feld, sondern ein
/// Verbot. `host` ist ueber h2 verboten (es steht als `:authority`), und
/// `connection`, `keep-alive`, `upgrade` und `transfer-encoding` sind nach
/// RFC 9113 8.2.2 verboten. Die Funktion filtert sie.
///
/// M3: die Zahl der Felder ist durch `MAX_ANTWORT_FELDER` begrenzt — nein,
/// das gilt fuer die **Antwort**. Hier begrenzt `MAX_FELDER_PRO_BLOCK`
/// (hpack) die Groesse eines Blocks; der Aufrufer begrenzt die Zahl der
/// Felder ueber `MAX_ANFRAGE_FELDER`, damit ein Aufrufer mit 100 000
/// Feldern keinen 6-MB-Rahmen baut.
/// added 02.10.2026 01:3x, R5f. **`pub`**, weil die Koordination eine
/// Koepfe-Vergleichsaufgabe gestellt hat und man den Draht nicht nur
/// glauben sollte: `tests/draht_kopfzeilen.rs` dekodiert damit den
/// HPACK-Block, den der Client wirklich schreibt, und misst so die
/// Kopfzeilen — statt sie aus dem Quelltext zu lesen.
pub fn felder_mit_vorgaben(request: &Request) -> Vec<(String, String)> {
    let mut felder: Vec<(String, String)> = request
        .headers
        .iter()
        .filter(|(k, _)| !hop_by_hop(k))
        .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
        .collect();
    if felder.len() >= MAX_ANFRAGE_FELDER {
        felder.truncate(MAX_ANFRAGE_FELDER);
    }
    // added 01.10.2026 16:0x: als **zwei Aufrufe** statt einer
    // Schliessung. GEMESSEN: `let hat = |name| felder.iter().any(…)` haelt
    // eine unveraenderliche Leihabe auf `felder`, und das `push` darunter
    // braucht `&mut` — der Uebersetzer sagt `cannot borrow as mutable
    // because it is also borrowed as immutable`. Die Schliessung war also
    // nicht nur unlesbar, sie hinderte genau die Aenderung, die die
    // Funktion ausfuellt.
    let hat_user_agent = felder.iter().any(|(k, _)| k == "user-agent");
    if !hat_user_agent {
        felder.push(("user-agent".to_string(), STANDARD_USER_AGENT.to_string()));
    }
    let hat_accept = felder.iter().any(|(k, _)| k == "accept");
    if !hat_accept {
        let wert = request.art.accept();
        felder.push(("accept".to_string(), wert.to_string()));
    }
    //
    // added 02.10.2026 01:4x, R5f. **`Accept-Language` und
    // `Accept-Encoding` fehlten hier** — und damit war die Invariante aus
    // R4z8 wieder gebrochen, die sie eigentlich aufgestellt hatte: dieselbe
    // `Request` bekommt je nach ausgehandeltem Protokoll verschiedene
    // Kopfzeilen. `Request::to_bytes` (der HTTP/1.1-Weg) schreibt beide
    // Kopfzeilen, dieser Weg nicht.
    //
    // GEMESSEN am Draht, bevor dieser Absatz stand (02.10.2026 01:3x,
    // `tests/draht_kopfzeilen.rs`, HPACK-Block des Clients dekodiert):
    //     :method GET   :scheme https   :authority stackoverflow.com
    //     :path /
    //     user-agent: Narsil/0.1
    //     accept: text/html,application/xhtml+xml,application/xml;q=0.9,
    //             image/avif,image/webp,*/*;q=0.8
    // Kein `accept-language`, kein `accept-encoding`. Firefox schickt beides
    // (und `Sec-Fetch-*`, `Upgrade-Insecure-Requests`); der Weg des Clients
    // darueber steht in `team/BERICHT_TLS_R5f.txt` Punkt 3.
    //
    // DIE REGEL ist dieselbe wie oben: nur setzen, was der Aufrufer **nicht**
    // gesetzt hat. `.header("accept-language", ..)` gewinnt — und das ist
    // keine Formalie, denn `www.debian.org` antwortet mit
    // `vary: negotiate,accept-language,Accept-Encoding,cookie` und liefert
    // je nach Sprache eine **andere** Datei (`index.de.html` /
    // `index.en.html`, GEMESSEN 02.10.2026 01:4x mit `curl`).
    let hat_sprache = felder.iter().any(|(k, _)| k == "accept-language");
    if !hat_sprache {
        felder.push(("accept-language".to_string(), STANDARD_SPRACHEN.to_string()));
    }
    let hat_kodierung = felder.iter().any(|(k, _)| k == "accept-encoding");
    if !hat_kodierung {
        // added 02.10.2026 01:4x: dieselbe Liste wie `to_bytes`, und aus
        // demselben Grund — ein Browser schickt sie (RFC 9110 12.5.3), und
        // `entpacke` laeuft fuer **jede** nicht-Umleitungsantwort, also auch
        // fuer die h2-Antwort. Ohne den Kopf bekommen wir unkomprimierte
        // Seiten; mit ihm kommen sie dekomprimiert zurueck (`entpackt`).
        felder.push((
            "accept-encoding".to_string(),
            STANDARD_KODIERUNG.to_string(),
        ));
    }
    felder
}

/// Schreibt alles oder meldet warum nicht.
/// DER ADAPTER: HTTP/1.1-Text -> HTTP/2-Anfrage.
///
/// added 01.10.2026 14:0x, R4z7. **WARUM EIN ADAPTER UND NICHT EIN
/// ZWEITER ANFRAGEPFAD.** Der Aufrufer von `sende` ist `fetch`, und der
/// schreibt HTTP/1.1-Text — genau das, was `roh` enthaelt. Ein zweiter
/// Weg hiesse: alle Regeln (Umleitung, Sitzung, `Accept`, User-Agent,
/// `Connection`) **zweimal** zu bauen und zweimal zu pflegen. Der
/// Adapter nimmt die fertige Anfrage und uebersetzt sie.
///
/// DIE HOP-BY-HOP-FELDER, und das ist die Stelle, an der ein Adapter
/// still falsch liegen kann (RFC 9113 8.1.2.2): `connection`,
/// `keep-alive`, `proxy-connection`, `transfer-encoding`, `upgrade` und
/// `host`. `transfer-encoding` und `connection` sind **Verbote** ueber h2.
/// added 01.10.2026 22:5x, R5e. `strom` sagt, welchen Strom die Anfrage
/// benutzt (ungerade, ab 1), `verbindung_neu` sagt, ob die Vorfahrt noch
/// geschrieben werden muss — bei einer **gepoolten** h2-Verbindung darf das
/// **nicht** sein, sonst schickt man einen zweiten Client-Preface auf
/// derselben Verbindung (RFC 9113 3.4: „Each client connection
/// **MUST** start with a single connection preface").
/// added 03.10.2026, Rolle NETZTEMPO. **Die h2-Anfrage zu einer `Request`** —
/// an **einer** Stelle.
///
/// DER GRUND FUER DIE EIGENE FUNKTION: sie wird vom Abrufweg **und** von
/// `h2geteilt::hole_und_sende` gebraucht. Die Felder sind nicht Kosmetik:
/// `:authority` traegt nach RFC 9113 8.1.2.3 den Host **ohne** Standardport,
/// und der `user-agent` ist Pflicht — GEMESSEN (R4z8):
/// `www.wikipedia.org` antwortet ohne ihn ueber h2 mit **403**.
///
/// `roh` ist der HTTP/1.1-Text derselben Anfrage; er wird nur fuer den Pfad
/// gebraucht (`pfad_aus_roh`), und ohne ihn faellt der Pfad auf
/// `request.path` zurueck.
pub(crate) fn h2_anfrage_aus(request: &Request, roh: &[u8]) -> h2client::Anfrage {
    let schemata = if ist_https(request) { "https" } else { "http" };
    let host = host_ohne_schema(&request.host).to_string();
    let authority = if request.port == 443 || request.port == 80 {
        host
    } else {
        format!("{host}:{}", request.port)
    };
    h2client::Anfrage {
        methode: request.method.as_str().to_string(),
        schemata: schemata.to_string(),
        host: authority,
        pfad: pfad_aus_roh(roh, &request.path),
        // added 01.10.2026 16:0x, R4z8. DIE VORGABEFELDER, und sie
        // fehlten hier — das ist ein **echter** Fehler, kein Kosmetik.
        //
        // GEMESSEN: `www.wikipedia.org` antwortete ueber HTTP/1.1 mit **200**
        // und ueber h2 mit **403**, und der Koerper der 403 nennt den
        // Grund: „Please set a user-agent and respect our robot policy".
        // Unabhaengig gegengeprueft mit `hpack`/`h2` (Python): dieselbe
        // Anfrage **ohne** `user-agent` ergibt 403, **mit** `narsil/0.1`
        // ergibt 200. Der HTTP/1.1-Weg schreibt seit je einen Vorgabe-
        // `User-Agent`, wenn der Aufrufer keinen setzt — der h2-Weg tat das
        // nicht, und dieselbe `Request` bekam je nach ausgehandeltem
        // Protokoll verschiedene Kopfzeilen.
        //
        // DIE REGEL steht jetzt an **einer** Stelle (`felder_mit_vorgaben`),
        // weil sie an zwei Orten zwei Kopien sind und eine Kopie davon
        // schon falsch war.
        felder: felder_mit_vorgaben(request),
        koerper: Vec::new(),
    }
}

fn hole_ueber_h2(
    stream: &mut Box<dyn Leitung>,
    request: &Request,
    roh: &[u8],
    strom: u32,
    verbindung_neu: bool,
    hpack: &mut hpack::DynamischeTabelle,
) -> Result<h2client::Antwort, NetError> {
    // added 01.10.2026 14:0x: `ist_https` an **einer** Stelle. Vorher stand
    // dieselbe Bedingung an drei Orten (hier, in `sende`, in
    // `pool_schluessel`) — drei Moeglichkeiten fuer dieselbe Frage.
    //
    // added 03.10.2026, Rolle NETZTEMPO: **UND JETZT AUCH** in
    // `h2geteilt::hole_und_sende`. Der Aufbau der h2-Anfrage stand bis hier
    // **im** Abrufweg, und die zweite Stelle haette ihn kopiert — mit genau
    // dem gemessenen Risiko, dass die Kopie den `user-agent` verliert und
    // der Server mit 403 antwortet (R4z8).
    let anfrage = h2_anfrage_aus(request, roh);
    // added 01.10.2026 21:1x, R5d. DIE ZWEI FORMEN, und die Unterscheidung
    // ist der ganze Punkt: EOF vor dem ersten Rahmen ist **kein
    // Protokollfehler`, sondern eine tote Leitung — und nur die zweite Form
    // darf der Aufrufer neu aufbauen.
    // added 01.10.2026 21:2x, R5d. `h2client::Fehler` ist ein **Typ-Alias**
    // auf `h2::RahmenFehler` (h2client.rs Zeile 109), also ist der Abgleich
    // direkt gegen die Variante — kein `Rahmen(..)`-Mantel. GEMESSEN, warum
    // das der Fall ist: der Uebersetzer meldete
    // `no variant, associated function, or constant named 'Rahmen' found`.
    let grund = |e: h2client::Fehler| rahmenfehler_als_netzfehler(request, e);
    // added 01.10.2026 14:0x: **JEDER** Schritt macht eine eigene
    // `LeitungAlsFrames`. Grund: `h2client::schreibe_vorlauf` nimmt
    // `&mut dyn Frames`, und das Fenster ist nur fuer die Dauer des
    // Aufrufs gueltig. Drei Aufrufe, drei Fenster — das ist die
    // kurzweiligste Form, die nicht an der falschen Stelle eine
    // Leihgabe ueberlaeuft.
    //
    // added 01.10.2026 22:6x, R5e. DIE VORFAHRT NUR AUF EINER NEUEN
    // VERBINDUNG. GEMESSEN, warum das keine Feinheit ist: RFC 9113 3.4
    // sagt „Each client connection MUST start with a single connection
    // preface", und ein zweiter Preface auf derselben Verbindung ist ein
    // Verbindungsfehler. Ohne diese Bedingung waere die zweite Anfrage auf
    // einer gepoolten h2-Verbindung ein `PROTOCOL_ERROR` — und die Meldung
    // wuerde „Serverfehler" sagen, obwohl der Server nur gemaess der Norm
    // gehandelt hat.
    if verbindung_neu {
        h2client::schreibe_vorlauf(&mut LeitungAlsFrames {
            leitung: stream.as_mut(),
        })
        .map_err(grund)?;
    }
    // added 01.10.2026 22:6x, R5e: der **Strom** wandert mit — beim
    // Schreiben und beim Lesen. GEMESSEN, warum beide Stellen ihn brauchen:
    // schreibt man die Anfrage auf Strom 1 und liest auf Strom 3, dann
    // wartet der Client auf eine Antwort, die der Server nie schickt, weil
    // er auf Strom 1 gar nichts bekommen hat.
    h2client::schreibe_anfrage(
        &mut LeitungAlsFrames {
            leitung: stream.as_mut(),
        },
        &anfrage,
        strom,
    )
    .map_err(grund)?;
    // added 01.10.2026 00:5x, R5b. DIE TABELLE KOMMT VON AUSSEN, und das
    // ist der ganze Unterschied: vorher legte `lies_antwort` bei jedem
    // Aufruf eine neue an. GEMESSEN, warum das auf einer **wiederverwendeten**
    // Verbindung bricht (`www.google.com`, zweiter Abruf, 01.10.2026 00:3x):
    //     Abruf: H2Protokoll { grund: "HPACK: IndexUngueltig (73)" }
    // Nach RFC 7541 4.1 wird die dynamische Tabelle zwischen Nachrichten
    // **nicht** geleert — der Server hat seinen Kodierer ueber die
    // Verbindung benutzt, und Index 73 zeigte auf einen Eintrag, den wir nie
    // gesehen hatten.
    // added 02.10.2026 04:2x, R5g. `_mit_phasen`, weil die Zeit bis zum
    // ersten Byte bisher **nur** im HTTP/1.1-`Leser` gemessen wurde und
    // damit auf keiner h2-Seite in der Anzeige stand. GEMESSEN vor dieser
    // Zeile (02.10.2026 04:1x, `tests/t4_verbindung.rs`):
    // `PHASEN dns Some(31.66ms) tcp Some(19.73ms) tls Some(412.06ms)
    //  erstes_byte None gesamt Some(102.60ms)` — und alle sechs gemessenen
    // Seiten laufen ueber h2 (R4z8), das war also der Normalfall.
    let (antwort, erstes_byte) = h2client::lies_antwort_mit_phasen(
        &mut LeitungAlsFrames {
            leitung: stream.as_mut(),
        },
        strom,
        hpack,
    )
    .map_err(grund)?;
    phasen_setzen(Phasen {
        erstes_byte,
        ..Phasen::leer()
    });
    Ok(antwort)
}

/// Ist das ein **Hop-by-hop**-Feld?
///
/// added 01.10.2026 14:0x, R4z7. DIE SECHS NAMEN, woertlich aus
/// RFC 9113 8.1.2.2. Sie beschreiben eine **Verbindung**; ueber h2 ist
/// die Verbindung wiederverwendbar und vom einzelnen Strom unabhaengig.
fn hop_by_hop(name: &str) -> bool {
    matches!(
        name,
        "connection" | "keep-alive" | "proxy-connection" | "transfer-encoding" | "upgrade" | "host"
    )
}

// ===========================================================================
// added 02.10.2026 10:5x UTC (Projektlabel 02.10.2026), Rolle TLS.
// Runde 8 „UNHACKBAR Teil 2", S8: `Referrer-Policy` mit
// `strict-origin-when-cross-origin` als **Vorgabe**.
//
// -------------------------------------------------------------------------
// DIE QUELLE, und sie ist eine Spezifikation, kein RFC — das wird gesagt
// -------------------------------------------------------------------------
// GEMESSEN am 02.10.2026: die Kopfzeile der heruntergeladenen Fassung lautet
// `Referrer Policy` (WHATWG/W3C, Living Standard, w3c/webappsec-referrer-policy).
// Der IETF-Entwurf `draft-ietf-httpbis-referrer-policy` existiert dort **nicht
// mehr** (Abruf liefert eine Fehlerseite, 1 Zeile HTML), und die Werte, um die
// es geht, stehen in keinem RFC. Wer eine RFC-Quelle verlangt, bekommt keine —
// das ist eine Angabe, keine Ausrede.
// --------------------------------------------------------------------------

/// added 02.10.2026 10:5x: DIE ACHT RICHTLINIEN der Spezifikation, Abschnitt 3,
/// plus der Leerstring aus 3.9. Die Namen sind **deutsch und genau**, weil ein
/// Name wie `Streng` ohne Erklärung nichts sagt: „strict“ heißt in dieser
/// Spezifikation **nur** „kein Herabstufen auf unvertrautes“, nicht „strenger
/// als origin“. Das steht so in 3.4 bis 3.7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferrerPolitik {
    /// „no-referrer“: gar kein `Referer`.
    Keine,
    /// „no-referrer-when-downgrade“ (3.2): nichts, wenn von vertraut nach
    /// unvertraut gewechselt wird.
    KeineBeiHerabstufung,
    /// „same-origin“ (3.3): die volle URL nur bei gleicher Herkunft.
    NurGleicheHerkunft,
    /// „origin“ (3.4): immer nur die Herkunft.
    NurHerkunft,
    /// „strict-origin“ (3.5): die Herkunft, aber nicht beim Herabstufen.
    NurHerkunftStreng,
    /// „origin-when-cross-origin“ (3.6): die volle URL bei gleicher Herkunft.
    HerkunftWennFremd,
    /// „strict-origin-when-cross-origin“ (3.7): **die Vorgabe**. Volle URL bei
    /// gleicher Herkunft, nur die Herkunft bei fremder Herkunft, nichts beim
    /// Herabstufen.
    HerkunftWennFremdStreng,
    /// „unsafe-url“ (3.8): die volle URL, auch beim Herabstufen.
    Voll,
}

/// added 02.10.2026 10:6x: **DIE VORGABE**, und sie steht wörtlich in 3.7:
/// „This policy is the user agent’s **default**, and will be applied if no
/// policy is otherwise specified.“ Sie ist damit **keine eigene Entscheidung**
/// dieses Projekts, sondern die Vorgabe der Spezifikation — der Unterschied ist
/// wichtig, weil eine eigene Vorgabe sich ändern ließe und diese nicht.
pub const REFERRER_VORGABE: ReferrerPolitik = ReferrerPolitik::HerkunftWennFremdStreng;

/// added 02.10.2026 10:6x: **WELCHE RICHTLINIE**, und der Weg ist die ABNF aus
/// 4.1 (`"Referrer-Policy:" 1#( policy-token / extension-token )`) mit der
/// Regel aus 11.1: „unknown policy values will be **ignored**, and when multiple
/// sources specify a referrer policy, the value of the **latest** one will be
/// used.“ Also: das **letzte** bekannte Token gewinnt, ein unbekanntes wird
/// überlesen statt den ganzen Kopf zu verwerfen — genau das ist der Grund, warum
/// die ABNF ein `extension-token` danebenstellt.
pub fn referrer_richtlinie(werte: Option<&str>) -> ReferrerPolitik {
    referrer_richtlinie_gefunden(werte).unwrap_or(REFERRER_VORGABE)
}

/// added 02.10.2026 10:9x: **DIE FORM OHNE VORGABE**, und sie ist keine zweite
/// Parse-Strecke: [`referrer_richtlinie`] ruft genau diese hier auf. Sie
/// existiert fuer den einen Fall, in dem „unbekannt“ **nicht** „Vorgabe“ heissen
/// darf: bei einer Umleitung setzt eine **unbekannte** Angabe der Antwort nach
/// 11.1 die geltende Richtlinie nicht ausser Kraft, sondern laesst sie stehen. Wer
/// da auf die Vorgabe zurueckfiele, erlaebte einer Seite, die `origin` gesetzt
/// hat und dann Unsinn schickt, wieder das volle Herabstufen.
pub fn referrer_richtlinie_gefunden(werte: Option<&str>) -> Option<ReferrerPolitik> {
    let roh = werte?;
    let mut richtlinie: Option<ReferrerPolitik> = None;
    for teil in roh.split(',') {
        // added 02.10.2026 10:6x: **NUR DAS ERSTE TOKEN JE LISTENGLIED**, weil
        // die ABNF ein Komma als Trenner zwischen Tokens erlaubt, aber ein
        // Listenelement genau **ein** Token ist. `no-referrer, unsafe-url` sind
        // zwei Glieder, kein Token mit Komma darin.
        let token = teil.split(';').next().unwrap_or("").trim();
        richtlinie = match token {
            "no-referrer" => Some(ReferrerPolitik::Keine),
            "no-referrer-when-downgrade" => Some(ReferrerPolitik::KeineBeiHerabstufung),
            "same-origin" => Some(ReferrerPolitik::NurGleicheHerkunft),
            "origin" => Some(ReferrerPolitik::NurHerkunft),
            "strict-origin" => Some(ReferrerPolitik::NurHerkunftStreng),
            "origin-when-cross-origin" => Some(ReferrerPolitik::HerkunftWennFremd),
            "strict-origin-when-cross-origin" => Some(ReferrerPolitik::HerkunftWennFremdStreng),
            "unsafe-url" => Some(ReferrerPolitik::Voll),
            // added 02.10.2026 10:7x: **ALLES ANDERE BLEIBT UNANGETASTET**,
            // und das ist der Punkt aus 11.1: die Vorgabe gilt weiter, statt
            // dass eine unbekannte Angabe sie ersetzt. Wer hier `Default`
            // zurücksetzt, macht aus einem neuen Richtwert eine **Lücke**.
            _ => continue,
        };
    }
    richtlinie
}

/// added 02.10.2026 10:7x: **WAS IN DEN `REFERER` GEHOERT**, wörtlich RFC 9110
/// 10.1.3: „A user agent **MUST NOT** include the **fragment** and **userinfo**
/// components of the URI reference, if any, when generating the Referer field
/// value.“ Das ist die einzige Stelle in beiden Normen mit „MUST NOT“, und sie
/// ist damit das härteste Argument im ganzen Auftrag.
///
/// added 02.10.2026 10:7x: **UND DIE REDUKTION IST KEIN URL-PARSER**, weil
/// `narsil-net` nach ADR-030 keine Fremd-Crate hat (GEMESSEN an
/// `[dependencies]` in `Cargo.toml`: keine `narsil-url`). Diese Funktion liest
///_scheme_, _userinfo_, _host_, _port_, _Pfad_ und _Fragment_ — genau die Teile,
/// von denen die beiden Normen sprechen. Eine URL mit kaputtem Aufbau wird
/// **nicht** abgelehnt, sondern als Text behandelt: die Referrer-Regeln sind
/// eine **Sendepolitik**, keine Eingabeprüfung, und eine zweimal geprüfte URL
/// wäre die zweite Wahrheit (`narsil-url`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct ReferrerQuelle {
    /// Schema in Kleinbuchstaben, z. B. `https`.
    schema: String,
    /// Host wie im Text (ohne Userinfo), z. B. `example.com`.
    host: String,
    /// Port als Zahl; **0** bedeutet „Standardport des Schemas“.
    port: u16,
    /// Pfad mit Abfrage, ohne Fragment; nie leer.
    pfad: String,
    /// true, wenn das Schema nach der Spezifikation „potentially trustworthy“
    /// ist (2: https, wss).
    vertraut: bool,
}

/// added 02.10.2026 10:8x: **DIE ZERLEGUNG**, und sie ist absichtlich
/// fehlertolerant: ein Wert ohne `://` wird als **ohne Schema** behandelt und
/// damit als **nicht vertraut** — das ist die sichere Seite, denn die
/// Herabstufungsregel sendet dann gar nichts.
fn referrer_quelle(roh: &str) -> ReferrerQuelle {
    let mut rest = roh.trim();
    let schema = match rest.find("://") {
        Some(stelle) => {
            let s = rest[..stelle].to_ascii_lowercase();
            rest = &rest[stelle + 3..];
            s
        }
        None => String::new(),
    };
    // added 02.10.2026 10:8x: **FRAGMENT ZUERST**, weil `#` auch in einem
    // Pfad vorkommen darf (`/a#b` ist ein Pfad mit `#`, wenn es kein
    // Fragmentzeichen der URL ist) — nein, genau andersherum: **im Fragment**
    // darf kein `#` stehen, und ein `#` beendet den Wert. Ich schneide am
    // **ersten** `#`, weil alles danach Fragment ist.
    rest = rest.split('#').next().unwrap_or("");
    // added 02.10.2026 10:8x: **USERINFO**, und das `@` zaehlt nur **vor** dem
    // ersten `/`: in `https://user:pw@host/pfad` liegt es davor, in
    // `https://host/pfad@x` ist es ein Zeichen im Pfad.
    let mut host_teil = rest;
    let mut pfad_teil = "";
    if let Some(stelle) = rest.find('/') {
        host_teil = &rest[..stelle];
        pfad_teil = &rest[stelle..];
    }
    let ohne_userinfo = match host_teil.rfind('@') {
        Some(stelle) => &host_teil[stelle + 1..],
        None => host_teil,
    };
    let (host, port) = match ohne_userinfo.rfind(':') {
        // added 02.10.2026 10:9x: **NUR EIN PORT ZÄHLT**, und die Klammer ist
        // der Grund: `https://[::1]:8443/` ist ein IPv6-Host mit Port, und ein
        // `:` ohne Klammer wäre dort kein Port. GEMESSEN ist die Falle an
        // `https://example.com:8443` — ohne diese Prüfung nähme die letzte
        // Doppelpunktstelle den **Pfad** mit, weil dort auch `:` stehen kann.
        Some(stelle) if !ohne_userinfo[stelle + 1..].contains(']') => {
            let zahl = ohne_userinfo[stelle + 1..].parse::<u16>().unwrap_or(0);
            (ohne_userinfo[..stelle].to_ascii_lowercase(), zahl)
        }
        _ => (ohne_userinfo.to_ascii_lowercase(), 0),
    };
    let vertraut = schema == "https" || schema == "wss";
    ReferrerQuelle {
        schema,
        host,
        port,
        pfad: if pfad_teil.is_empty() {
            "/".to_string()
        } else {
            pfad_teil.to_string()
        },
        vertraut,
    }
}

/// added 02.10.2026 10:9x: **GLEICHE HERKUNFT**, und **ohne** die drei Teile
/// einzeln zu vergleichen wäre es ein Textvergleich — der bei
/// `https://example.com` gegen `https://example.com/` falsch „verschieden“
/// sagt. Verglichen werden Schema, Host und **effektiver** Port.
fn gleiche_herkunft(a: &ReferrerQuelle, b: &ReferrerQuelle) -> bool {
    a.schema == b.schema && a.host == b.host && a.effektiver_port() == b.effektiver_port()
}

impl ReferrerQuelle {
    /// added 02.10.2026 10:9x: **DER EFFEKTIVE PORT**, und die Vorgabe ist
    /// begründet: `https://x` und `https://x:443` sind dieselbe Herkunft, und
    /// die Nichtbeachtung davon ist der Fehler, der später zur Vergleichsstelle
    /// wird. Für ein unbekanntes Schema (Port 0) bleibt 0 stehen.
    fn effektiver_port(&self) -> u16 {
        if self.port != 0 {
            return self.port;
        }
        match self.schema.as_str() {
            "https" | "wss" => 443,
            "http" | "ws" => 80,
            _ => 0,
        }
    }

    /// added 02.10.2026 10:0x: **DIE HERKUNFT ALS TEXT**, und der Slash am Ende
    /// ist nicht Kosmetik: das Beispiel der Spezifikation in 3.7 schreibt
    /// `https://example.com/` mit Slash, und die Herkunfts-Serialisierung eines
    /// Parsers ist genau das. Der Standardport entfaellt (3.1 in RFC 3986: der
    /// Port ist optional, wenn er der Standard des Schemas ist).
    fn als_herkunft(&self) -> String {
        let mut s = String::with_capacity(self.schema.len() + self.host.len() + 12);
        s.push_str(&self.schema);
        s.push_str("://");
        s.push_str(&self.host);
        let standard = match self.schema.as_str() {
            "https" | "wss" => 443,
            "http" | "ws" => 80,
            _ => 0,
        };
        if self.port != 0 && self.port != standard {
            s.push(':');
            s.push_str(&self.port.to_string());
        }
        s.push('/');
        s
    }

    /// added 02.10.2026 10:0x: **DIE VOLLE URL**, und sie ist genau die
    /// Referrer-URL **ohne** Fragment (RFC 9110 10.1.3) und ohne Userinfo
    /// (ebenda) — die Zerlegung hat beides schon abgeschnitten.
    fn als_url(&self) -> String {
        let mut s = self.als_herkunft();
        if self.pfad != "/" {
            s.push_str(self.pfad.trim_start_matches('/'));
        }
        s
    }
}

/// added 02.10.2026 10:0x: **DER `REFERER`-WERT**, oder `None`, wenn keiner
/// gesendet wird.
///
/// added 02.10.2026 10:0x: **DIE REGELN SIND DIE DER SPEZifikation**, und die
/// Herabstufung ist die eine Stelle, an der die meisten Fehler passieren: sie
/// gilt für „strict-origin“, „strict-origin-when-cross-origin“ und
/// „no-referrer-when-downgrade“ und **nicht** für „origin“, „origin-when-cross-origin“
/// und „unsafe-url“. Deshalb steht sie **einmal** hier statt sechsmal in den
/// Zweigen.
///
/// added 02.10.2026 10:1x: **UND DIE ENTSCHEIDUNG ÜBER DAS ABSCHNEIDEN** ist
/// vom Policy abhängig und steht deshalb **nach** der Herkunftsfrage: bei
/// `origin` wird nie ein Pfad verschickt, bei `same-origin` nie über eine
/// Herkunftsgrenze hinweg.
pub fn referrer_berechnen(politik: ReferrerPolitik, quelle: &str, ziel: &str) -> Option<String> {
    let q = referrer_quelle(quelle);
    let z = referrer_quelle(ziel);
    // added 02.10.2026 10:8x: **KEIN SCHEMA ODER KEIN HEIRAT — GAR KEIN
    // `REFERER`.** Das ist eine Korrektur, die ein Test erzwungen hat: mit der
    // Quelle `example.com/a` und dem Ziel `https://example.com/b` lieferte die
    // Regel „fremde Herkunft, kein Herabstufen“ **das Herkunfts-Fragment
    // `://example.com/`** — das ist kein gültiger Feldwert. Grund in der Norm,
    // RFC 9110 10.1.3: `Referer = absolute-URI / partial-URI`. Eine absolute
    // URI braucht ein Schema; eine **relative** (`partial-URI`) koennte nur
    // gegenueber dem Ziel gelten und muesste den Quell-Host trotzdem nennen —
    // also ist das Richtigeste, gar nichts zu schicken. `narsil-net` bekommt
    // Quell-URLs aus fremdem Code, und ein Kopf mit kaputtem Aufbau darf kein
    // Ergebnis erfinden.
    if q.schema.is_empty() || q.host.is_empty() {
        return None;
    }
    let gleich = gleiche_herkunft(&q, &z);
    // added 02.10.2026 10:1x: **HERABSTUFUNG**, und die Definition ist die
    // Spezifikation (3.6/3.7): „whose referrerURL is a potentially trustworthy
    // URL and whose current URL is a non-potentially-trustworthy URL“.
    let herabgestuft = q.vertraut && !z.vertraut;
    match politik {
        ReferrerPolitik::Keine => None,
        ReferrerPolitik::KeineBeiHerabstufung => {
            if herabgestuft {
                None
            } else {
                Some(q.als_url())
            }
        }
        ReferrerPolitik::NurGleicheHerkunft => {
            if gleich {
                Some(q.als_url())
            } else {
                None
            }
        }
        ReferrerPolitik::NurHerkunft => Some(q.als_herkunft()),
        ReferrerPolitik::NurHerkunftStreng => {
            if herabgestuft {
                None
            } else {
                Some(q.als_herkunft())
            }
        }
        ReferrerPolitik::HerkunftWennFremd => {
            if gleich || herabgestuft {
                Some(q.als_url())
            } else {
                Some(q.als_herkunft())
            }
        }
        ReferrerPolitik::HerkunftWennFremdStreng => {
            if gleich {
                Some(q.als_url())
            } else if herabgestuft {
                None
            } else {
                Some(q.als_herkunft())
            }
        }
        ReferrerPolitik::Voll => Some(q.als_url()),
    }
}

/// added 02.10.2026 10:1x: **DER EINSTIEG**, und er nimmt den **Kopf der
/// Antwort** entgegen, nicht eine geratene Vorgabe. Ohne `Referrer-Policy` im
/// Kopf gilt 3.9/3.7: die Vorgabe. Mit einer unbekannten Angabe gilt 11.1: die
/// Vorgabe bleibt.
pub fn referrer_aus_kopf(kopf_wert: Option<&str>, quelle: &str, ziel: &str) -> Option<String> {
    referrer_berechnen(referrer_richtlinie(kopf_wert), quelle, ziel)
}

/// Der Pfad aus dem HTTP/1.1-Text, **mit** Abfrage.
///
/// added 01.10.2026 14:0x, R4z7. WARUM NICHT EINFACH `request.path`: in
/// `roh` steht die erste Zeile (`GET /a?b=1 HTTP/1.1`), und dort kann eine
/// Abfrage stehen, die in `request.path` nicht steht. Ueber h2 gehoert sie
/// in `:path` (RFC 9113 8.1.2.3) — wer sie weglässt, schickt eine andere
/// Anfrage, als der Browser gebaut hat.
/// added 03.10.2026, Rolle NETZTEMPO. **Eine h2-Antwort in die Form von
/// [`Response`]** — an **einer** Stelle.
///
/// WARUM DAS EINE EIGENE FUNKTION IST: der h2-Weg hatte diesen Block an
/// **zwei** Stellen (`hole_und_sende_innen` und jetzt `h2geteilt::hole_und_sende`)
/// gebaut, und die zweite Stelle waere eine Kopie gewesen. Kopien von
/// Antwort-Bau sind die Sorte Fehler, die **erst** auffaellt, wenn ein Feld
/// dazukommt und die eine Seite es vergisst — GEMESSEN genau so an
/// `set-cookie` (R4z7), der nur an **einer** Stelle stand.
///
/// DER FEHLERFALL: mehr Felder, als erlaubt, ist `H2ZuVieleFelder` — und die
/// Pruefung gehoert **hierher**, nicht in die Ansicht: die Grenze ist eine
/// Eigenschaft des Protokolls (M4), und der Aufrufer soll sie nicht selbst
/// kennen.
pub(crate) fn response_aus_h2(
    request: &Request,
    antwort: h2client::Antwort,
) -> Result<Response, NetError> {
    if !h2client::felder_zahl_ok(&antwort) {
        return Err(NetError::H2ZuVieleFelder {
            host: request.host.clone(),
        });
    }
    let headers = h2client::felder_in_map(&antwort);
    let mut set_cookie = Vec::new();
    let koerper = antwort.koerper;
    for (k, v) in &headers {
        if k.eq_ignore_ascii_case("set-cookie") {
            set_cookie.push(v.clone());
        }
    }
    Ok(Response {
        status: StatusLine {
            version: "HTTP/2".to_string(),
            code: antwort.status,
            reason: h2_grundtext(antwort.status).to_string(),
        },
        headers,
        body: koerper,
        trailers: std::collections::HashMap::new(),
        set_cookie,
        body_quelle: BodyQuelle::KeinBody,
        //
        // added 01.10.2026 22:7x, R5e. **DIE h2-VERBINDUNG GEHOERT
        // JETZT IN DEN POOL.** `Bleibt` heisst fuer `sende`: die
        // Leitung nach der Antwort wieder einlegen.
        //
        // GEMESSEN, warum das Pflicht ist und nicht Komfort:
        // vorher stand hier `Schliesst` mit dem Kommentar „bis der
        // Pool h2 kann". GEMESSEN, was das kostete — jede Anfrage
        // eine neue TCP- und TLS-Verbindung (`pool offen 0` nach
        // jedem Abruf), und daraufhin schlossen `www.mozilla.org`
        // (EOF nach 128 ms) und `archlinux.org` (221 ms) die zweite
        // Verbindung, waehrend `www.google.com` vier in Folge
        // verkraftete.
        //
        // UND WAS DAS **NICH** BEHAUPTET: dass eine h2-Verbindung
        // beliebig oft wiederzuverwenden ist. Sie kann geschlossen
        // sein, ohne dass wir es merken — der Neuaufbau in diesem
        // Zweig (R5d) ist der Fall, in dem es passiert.
        keep_alive: KeepAlive::Bleibt,
        // added 01.10.2026 14:0x, R4z7: `url_aus(request)` und
        // nicht `request.url` — die Funktion **ist** die URL, und
        // sie ist an **einer** Stelle. Ein zweiter Weg (Felder
        // selbst zusammensetzen) waere eine zweite Form von
        // derselben URL und koennte auseinanderlaufen.
        url: url_aus(request),
        entpackt: false,
    })
}

/// added 03.10.2026, Rolle NETZTEMPO. **Ein `RahmenFehler` wird ein
/// `NetError`** — an **einer** Stelle.
///
/// WARUM DAS EINE FUNKTION UND KEIN VERSCHLUSS ist: der Verschluss stand in
/// `hole_und_sende_innen`, und `h2geteilt` braucht dieselbe Zuordnung. Eine
/// Kopie waere eine **zweite Fehlersprache** fuer dieselben Fehler — und
/// genau daran ist die Wiederholregel abhaengig: `H2SchreibenFehlgeschlagen`
/// wird wiederholt, `H2Protokoll` nicht. Wer die Zuordnung kopiert und dabei
/// `SchreibenFehlgeschlagen` vergisst, baut einen Browser, der einen toten
/// Sockel als Protokollfehler meldet.
///
/// DIE DREI FORMEN, und jede beantwortet eine andere Frage:
///  * `H2EndeVorzeitig` — EOF **vor** dem ersten Rahmen: die Leitung war tot
///    (R5d). Das ist die Form, die wiederholt wird.
///  * `H2ServerBeendet` — `GOAWAY` (6.8). Kein Protokollfehler, sondern eine
///    beendete Verbindung; sie wird genauso wiederholt.
///  * `H2Protokoll` — alles andere. Wird **nicht** wiederholt.
pub(crate) fn rahmenfehler_als_netzfehler(request: &Request, e: h2client::Fehler) -> NetError {
    match e {
        h2::RahmenFehler::KeinRahmen => NetError::H2EndeVorzeitig {
            host: request.host.clone(),
        },
        // added 01.10.2026 23:7x, R5b: `GOAWAY` ist **kein** Protokollfehler,
        // sondern eine beendete Verbindung (6.8). GEMESSEN: vorher landete
        // es in `H2Protokoll`, und damit beim Benutzer.
        h2::RahmenFehler::ServerBeendet {
            letzter_strom,
            grund,
        } => NetError::H2ServerBeendet {
            host: request.host.clone(),
            letzter_strom,
            grund,
        },
        // added 02.10.2026 11:1x: **DER SCHRITTFEHLER GEHOERT NICHT IN
        // `H2Protokoll`**, weil die Wiederholregel danach entscheidet. Hier
        // geht die Unterscheidung verloren, die `h2client` noch hat: dort sind
        // `RahmenFehler::SchreibenFehlgeschlagen` und ein Protokollfehler zwei
        // Varianten, und `schreibe_vorlauf` benutzt die Schreibform an **zehn**
        // Stellen (GEMESSEN: `grep -c schreibfehler_grund crates/narsil-net/src/
        // h2client.rs`). Wer sie zusammenwirft, macht aus einem toten Sockel
        // einen „Protokollfehler" — und der wird nicht wiederholt.
        h2::RahmenFehler::SchreibenFehlgeschlagen { grund } => {
            NetError::H2SchreibenFehlgeschlagen {
                grund: grund.to_string(),
            }
        }
        andere => NetError::H2Protokoll {
            grund: andere.to_string(),
        },
    }
}

fn pfad_aus_roh(roh: &[u8], pfad: &str) -> String {
    // M4: nur der Kopf, und nur 4 KiB. `roh` kann ein ganzer Koerper sein
    // (POST mit Formular), und die erste Zeile steht hinten nicht.
    const MAX_KOPF: usize = 4096;
    let kopf = &roh[..roh.len().min(MAX_KOPF)];
    let text = String::from_utf8_lossy(kopf);
    let mut woerter = text.lines().next().unwrap_or("").split_whitespace();
    let _methode = woerter.next();
    match woerter.next() {
        Some(z) if z.starts_with('/') => z.to_string(),
        _ => pfad.to_string(),
    }
}

/// Der `reason`-Text zu einem Statuscode.
///
/// added 01.10.2026 14:0x, R4z7. **UEBER h2 GIBT ES KEINEN** (RFC 9113
/// 8.1.2.4 kennt kein Feld dafuer). nginx schickt trotzdem einen, und
/// viele Parser brauchen ihn — also wird er aus einer **Konstante**
/// gefuellt: sie kommt nicht aus dem Netz.
///
/// DER UNBEKANNTE FALL, und er ist benannt: „unbekannt" statt einer
/// erfundenen Zahl. `Response::reason` ist ein `String`, und eine erfundene
/// Begruendung waere eine Behauptung, die der Server nicht gemacht hat.
fn h2_grundtext(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
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
        _ => "unbekannt",
    }
}

/// added 02.10.2026 02:9x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// added 02.10.2026 02:9x: DARF NOCH EINMAL VERSUCHT WERDEN? — und die
/// Antwort steht an **einer** Stelle, weil sie an drei Stellen gebraucht wird
/// (h2-Antwort, h1-Schreibfehler, h1-Leserfehler) und drei Antworten
/// auseinanderlaufen wuerden.
///
/// GEMESSEN am 02.10.2026 02:7x–02:8x (Quelle: `tests::haenger_messung` in
/// dieser Datei, Auftrag `team/AUTONOM.txt` Zeile 691): gegen
/// `www.rust-lang.org` und `www.wikipedia.org` scheiterten alle Anfragen, bei
/// denen die TLS-**Wiederaufnahme** benutzt wurde — ablesbar am Zaehler
/// `wiederaufnahme_versuche()`: bei jedem Fehlschlag `wdn 1`, bei jedem
/// Erfolg `wdn 0`; mit geleertem Ticketspeicher 9 von 9 Laeufen gruen
/// (350 bis 1154 ms). Nach dem Handshake (28 ms statt 290 ms, also ohne
/// Zertifikat) schickte der Server kein Byte mehr. `curl --http2` und
/// `curl --http1.1` lieferten dieselben Adressen **zwoelfmal** hintereinander
/// in 85 bis 142 ms, also ist es unser Weg.
///
/// WAS SICH AENDERT und warum es keine Ausrede ist: die alte Regel sagte
/// ausdruecklich, eine **frisch aufgebaute** Leitung, die sofort EOF liefert,
/// werde nicht wiederholt — „dann stimmt etwas nicht, und das Wiederholen
/// verdeckt es nur". Der gemessene Fall ist genau das, nur stimmt etwas nicht
/// bei **uns**: das Ticket. Ein Ticket zu verwerfen ist billig, ein
/// verschluckter Fehler teuer, und die Frist-Regel aus AUTONOM („jedes Lesen
/// mit Frist, tote Verbindung -> neu verbinden") deckt den Fall nicht ab,
/// weil die Leitung hier frisch und nicht gepoolt ist.
fn darf_wiederholt_werden(
    gepoolt: bool,
    wiederaufgenommen: bool,
    fehler: &NetError,
    wiederholbar: bool,
    wiederholungen: usize,
) -> bool {
    if !wiederholbar || wiederholungen >= pool::MAX_WIEDERHOLUNGEN {
        return false;
    }
    // added 02.10.2026 02:9x: die Form des Fehlers. `H2EndeVorzeitig` und
    // `H2ServerBeendet` sind „die Leitung hat nichts mehr geschickt"" — beim
    // ersten Rahmen und beim `GOAWAY`. `Receive`/`Empfangen` ist dieselbe Lage
    // im HTTP/1.1-Weg. GEMESSEN: mit `NARSIL_ALPN=h1` (nur `http/1.1`) kam
    // genau dieser Fehler, 96 und 102 ms, mit `wdn 1`.
    let leitung_tot = matches!(
        fehler,
        NetError::H2EndeVorzeitig { .. }
            | NetError::H2ServerBeendet { .. }
            | NetError::Receive(_)
            | NetError::ZeitlimitLesen
            // added 02.10.2026 11:2x: **UND DIE BEIDEN SCHREIBFORMEN**, und
            // sie fehlten hier. GEMESSEN: `Send` ist die Form des HTTP/1.1-Wegs
            // (Zeile 4299) und `H2SchreibenFehlgeschlagen` die des h2-Wegs; ein
            // Sockel, auf den nicht mehr geschrieben werden kann, ist genauso
            // tot wie einer, aus dem nicht mehr gelesen werden kann. Die beiden
            // Bedingungen **oberhalb** bleiben unveraendert: wiederholt wird nur
            // bei idempotenter Methode (`ist_wiederholbar`) und nur bei einer
            // Leitung, die aus dem Pool oder aus der Wiederaufnahme stammt —
            // eine frisch gebaute Leitung, die beim Schreiben stirbt, wird
            // **nicht** wiederholt, weil dann etwas nicht stimmt und das
            // Wiederholen es verdecken wuerde.
            | NetError::Send(_)
            | NetError::H2SchreibenFehlgeschlagen { .. }
    );
    leitung_tot && (gepoolt || wiederaufgenommen)
}

/// added 02.10.2026 02:9x: DAS TICKET WEGWERFEN, und nur dann, wenn es eins
/// gab. GEMESSEN: `Tickets::host_verwerfen` (R12) nimmt den Host aus dem
/// Speicher; `false` heisst, da war nichts — dann ist der naechste Versuch
/// ohnehin ein vollstaendiger Handshake.
fn wiederaufnahme_verwerfen(host: &str) -> bool {
    match ticket_speicher().lock() {
        Ok(mut t) => t.host_verwerfen(host),
        Err(_) => false,
    }
}

/// `Box<dyn Leitung>` als `h2client::Frames`.
///
/// added 01.10.2026 14:0x. DIE LEBENSDAUER ist der Grund fuer die eigene
/// Trait: `h2client` will **bei jedem Lesevorgang** eine Frist setzen.
/// Ueber `Box<dyn Leitung>` geht das ueber `set_read_timeout` — das ist
/// ein *Vorschlag* an den Sockel, kein Zustand. Fuer einen Test, der die
/// Frist beobachten will, braucht es einen Zustand; fuer den Betrieb nicht.
impl<'a> LeitungAlsFrames<'a> {
    /// added 04.10.2026, Rolle NETZTEMPO: **DER KONSTRUKTOR**, weil
    /// `pool.rs` den Adapter braucht (die h2-Vorfahrt beim Einlegen) und der Typ
    /// dort **privat** ist. Der Uebersetzer meldete da
    /// `no associated function ... found for struct LeitungAlsFrames`.
    pub(crate) fn neu(leitung: &'a mut Box<dyn Leitung>) -> Self {
        Self { leitung }
    }
}

struct LeitungAlsFrames<'a> {
    leitung: &'a mut dyn Leitung,
}

impl Read for LeitungAlsFrames<'_> {
    fn read(&mut self, p: &mut [u8]) -> std::io::Result<usize> {
        self.leitung.read(p)
    }
}

impl Write for LeitungAlsFrames<'_> {
    fn write(&mut self, p: &[u8]) -> std::io::Result<usize> {
        self.leitung.write(p)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.leitung.flush()
    }
}

impl h2client::Frames for LeitungAlsFrames<'_> {
    fn set_frist(&mut self, dauer: Option<std::time::Duration>) {
        let _ = self.leitung.set_read_timeout(dauer);
    }
}
fn schreibe(
    stream: &mut dyn Leitung,
    bytes: &[u8],
    limits: &Limits,
    frist: &Frist,
) -> Result<(), NetError> {
    let rest = frist.rest()?;
    stream
        .set_write_timeout(Some(limits.schreiben.min(rest)))
        .map_err(|e| NetError::Connect(format!("set_write_timeout: {e}")))?;
    let mut gesamt = 0usize;
    while gesamt < bytes.len() {
        match stream.write(&bytes[gesamt..]) {
            Ok(0) => {
                return Err(NetError::Send(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "Gegner hat die Verbindung beim Schreiben geschlossen",
                )));
            }
            Ok(n) => gesamt += n,
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                return Err(NetError::ZeitlimitSchreiben);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(NetError::Send(e)),
        }
    }
    stream.flush().map_err(|e| {
        if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut {
            NetError::ZeitlimitSchreiben
        } else {
            NetError::Send(e)
        }
    })?;
    Ok(())
}

/// Gepufferter Leser mit allen Grenzen.
///
/// `Vec<u8>` statt `BufReader`: `BufReader::read_line` haengt an der
/// eingebauten Zeilengrenze, und ein einzelner 1-Gigabyte-Header ohne CRLF
/// wuerde dort als "Zeile zu lang" mit fester Grenze abgeschnitten. Hier
/// entscheidet `Limits::zeile_bytes`.
struct Leser {
    /// Die Verbindung als Box — siehe die Begruendung an `sende`.
    stream: Box<dyn Leitung>,
    buf: Vec<u8>,
    pos: usize,
    limits: Limits,
    frist: Frist,
    eof: bool,
    /// added 01.10.2026 16:9x, T3: wann das **erste** Byte der Antwort
    /// kam, gemessen vom Bau des Lesers an (also ab dem Schreiben der
    /// Anfrage). `None`, solange kein Byte da ist.
    ///
    /// added 01.10.2026 16:9x: „erstes Byte" heisst das erste Byte des
    /// **Antwortkopfs**, nicht das letzte. Der Unterschied ist die
    /// Serverdenkzeit, und genau die will DEVTOOLS D10 sehen.
    erstes_byte: Option<std::time::Duration>,
    /// Wann der Leser gebaut wurde — der Nullpunkt der Phasenmessung.
    ///
    /// added 01.10.2026 16:9x: `Instant::now()` **hier** und nicht in
    /// `fuelle`, weil `fuelle` beim Nachschub des Puffers erneut laeuft und
    /// dann einen **spaeteren** Zeitpunkt als „erstes Byte" melden wuerde.
    gebaut: std::time::Instant,
    /// Wie viele Bytes der Headerphase schon gelesen wurden. Nur das wird
    /// gegen `header_bytes` geprueft — ein 10-MiB-Body ist erlaubt und
    /// darf nicht als Headerzaehlung sterben.
    header_gelesen: usize,
}

impl Leser {
    fn neu(stream: Box<dyn Leitung>, limits: &Limits, frist: &Frist) -> Self {
        Leser {
            stream,
            buf: Vec::with_capacity(8192),
            pos: 0,
            limits: *limits,
            frist: Frist::neu(frist.grenze),
            eof: false,
            header_gelesen: 0,
            erstes_byte: None,
            gebaut: std::time::Instant::now(),
        }
    }

    /// Fuellt den Puffer nach, wenn er leer ist.
    fn fuelle(&mut self) -> Result<(), NetError> {
        // added 01.10.2026 16:9x, T3: das **erste** Byte, und nur das
        // erste. GEMESSEN, warum hier und nicht nach der Statuszeile: eine
        // Kopfzeile kann in einem einzigen Lesevorgang kommen, und wer erst
        // nach der Statuszeile misst, meldet den **Nachschub** des Puffers
        // statt der Serverzeit.
        if self.erstes_byte.is_none() {
            self.erstes_byte = Some(self.gebaut.elapsed());
        }
        if self.pos < self.buf.len() || self.eof {
            return Ok(());
        }
        self.buf.clear();
        self.pos = 0;
        // Kompromiss: in kleinen Stuecken lesen, damit der EOF-Satz
        // ("Antwort ohne Content-Length") auch bei einem grossen Body
        // erkannt wird, ohne den ganzen Body doppelt zu kopieren.
        //
        // GEMESSENER FEHLER, 29.09.2026: die erste Fassung sammelte die
        // Stuecke in einem festen `[u8; 8192]` und zaehlte in `n` weiter.
        // Bei 60 KB Header (Test `header_knapp_unter_der_grenze`) lief
        // `n` auf 60870 und `&stueck[..n]` PANICTE. Deshalb haengt der
        // Sammelbehaelter am Ende der Schleife, nicht am Stack.
        let mut n = 0usize;
        loop {
            // Die Frist wird VOR jedem Lesevorgang geprueft: sie gilt auch
            // fuer eine Verbindung, die jeden einzelnen Lesevorgang noch
            // bedient, aber die Summe der Wartezeit zu lang macht.
            self.frist.pruefen()?;
            let rest = self.frist.rest()?;
            // MERKMAL, GEMESSEN 29.09.2026: der Lese-Timeout ist das
            // Minimum aus Einzelgrenze und Restzeit. Ist die Restzeit die
            // kleinere, dann ist ein Timeout hier KEIN "der Server
            // schweigt", sondern "die Gesamtzeit ist um" — und genau so
            // wird es auch gemeldet. Ohne diese Unterscheidung war
            // `gesamt_zeitlimit_greift_auch_wenn_jeder_lesevorgang_passt`
            // FLAKY: 2 von 20 Laeufen (22:24) meldeten `ZeitlimitLesen`
            // statt `ZeitlimitGesamt`, je nachdem, ob der Scheduler einen
            // 100-ms-Tick verpasst hatte. Der Benutzer sieht an der
            // Meldung, ob der Server schweigt oder die Seite zu lange
            // braucht — das sind verschiedene Ursachen.
            let rest_kleiner = rest <= self.limits.lesen;
            let timeout = self.limits.lesen.min(rest);
            self.stream
                .set_read_timeout(Some(timeout))
                .map_err(NetError::Receive)?;
            let mut stueck = [0u8; 8192];
            match self.stream.read(&mut stueck) {
                Ok(0) => {
                    self.eof = true;
                    return Ok(());
                }
                Ok(k) => {
                    self.buf.extend_from_slice(&stueck[..k]);
                    n += k;
                    // Nach dem ersten Byte ist ein Timeout kein Grund
                    // aufzugeben, solange das GESAMT-Zeitlimit lebt.
                    if k < stueck.len() || self.frist.rest().is_err() {
                        break;
                    }
                }
                Err(e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut =>
                {
                    if n > 0 {
                        break;
                    }
                    // Siehe die Bemerkung oben: war die RESTZEIT das
                    // Kleinere, ist das Gesamtlimit um, nicht der Server.
                    if rest_kleiner {
                        return Err(self.frist.erschöpft());
                    }
                    return Err(NetError::ZeitlimitLesen);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(NetError::Receive(e)),
            }
        }
        Ok(())
    }

    /// Liest bis LF, ohne LF. Inklusive CRLF-Bereinigung am Ende.
    /// `zaehlt_header`: wenn wahr, wird gegen die Headergrenze gezaehlt.
    fn zeile(&mut self, zaehlt_header: bool) -> Result<Vec<u8>, NetError> {
        let mut out: Vec<u8> = Vec::new();
        loop {
            self.fuelle()?;
            if self.pos >= self.buf.len() {
                // Ende ohne LF: das ist ein abgeschnittener Header, kein
                // gueltiger leerer Abschluss.
                if out.is_empty() && self.eof {
                    return Ok(Vec::new());
                }
                return Err(if zaehlt_header {
                    NetError::HeaderUnvollstaendig
                } else {
                    NetError::Receive(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Zeile ohne LF beendet",
                    ))
                });
            }
            let start = self.pos;
            match self.buf[start..].iter().position(|b| *b == b'\n') {
                Some(i) => {
                    out.extend_from_slice(&self.buf[start..start + i]);
                    self.pos = start + i + 1;
                    let mut end = out.len();
                    if end > 0 && out[end - 1] == b'\r' {
                        end -= 1;
                    }
                    out.truncate(end);
                    // ECHTER FEHLER, gefunden am 30.09.2026 beim
                    // Verdrahten von HTTPS, und gefunden von einem
                    // Test, der vorher schon existierte:
                    // `headergrenze_haelt_einen_einzelnen_wahnsinnigen_
                    // header_auf` schlug 2 von 6 Laeufen fehl.
                    //
                    // In DIESEM Zweig stand die Headergrenze VOR der
                    // Zeilengrenze. Kommt eine 200-KiB-Zeile in einem
                    // Stueck an, greift hier zuerst `HeaderZuGross`;
                    // kommt sie in kleinen Stuecken, greift weiter unten
                    // `ZeileZuLang`. DERSELBE Server, DIESELBE Antwort,
                    // zwei verschiedene Fehler — je nachdem, wie das TCP
                    // sie gerade ausliefert.
                    //
                    // Das ist nicht nur unsauber. Die Zeilengrenze ist
                    // die SPECIFISCHERE: sie sagt, dass EIN Zeilenkopf zu
                    // lang ist, und nennt mit 8192 die Zahl, bei der es
                    // passiert. Die Headergrenze sagt nur, dass die
                    // Summe zu gross wird. Wer dem Benutzer
                    // `HeaderZuGross 65536` zeigt, haelt eine einzelne
                    // kaputte Zeile fuer ein zu grosses Formular.
                    //
                    // Die Reihenfolge ist deshalb jetzt in BEIDEN
                    // Zweigen dieselbe: erst die Zeile, dann die Summe.
                    if out.len() > self.limits.zeile_bytes {
                        return Err(NetError::ZeileZuLang {
                            grenze: self.limits.zeile_bytes,
                        });
                    }
                    if zaehlt_header {
                        self.header_gelesen += out.len() + 2;
                        self.pruefe_header()?;
                    }
                    return Ok(out);
                }
                None => {
                    let rest = self.buf.len() - start;
                    out.extend_from_slice(&self.buf[start..]);
                    self.pos = self.buf.len();
                    // Zu lang, BEVOR der Speicher weiter waechst: ein
                    // 4-Gigabyte-Header darf den Client nicht kippen.
                    if out.len() > self.limits.zeile_bytes {
                        return Err(NetError::ZeileZuLang {
                            grenze: self.limits.zeile_bytes,
                        });
                    }
                    if zaehlt_header {
                        self.header_gelesen += rest;
                        self.pruefe_header()?;
                    }
                }
            }
        }
    }

    fn pruefe_header(&self) -> Result<(), NetError> {
        if self.header_gelesen > self.limits.header_bytes {
            return Err(NetError::HeaderZuGross {
                grenze: self.limits.header_bytes,
            });
        }
        Ok(())
    }

    /// Liest genau `n` Byte. EOF dazwischen ist ein BENANNTER Fehler
    /// ("halbe Seite"), kein stiller Erfolg.
    fn genau(&mut self, n: usize) -> Result<Vec<u8>, NetError> {
        let mut out = Vec::with_capacity(n.min(1 << 16));
        while out.len() < n {
            self.fuelle()?;
            if self.pos >= self.buf.len() {
                return Err(NetError::VerbindungAbgebrochen {
                    erwartet: n,
                    gelesen: out.len(),
                });
            }
            let verfuegbar = self.buf.len() - self.pos;
            let nehmen = verfuegbar.min(n - out.len());
            out.extend_from_slice(&self.buf[self.pos..self.pos + nehmen]);
            self.pos += nehmen;
        }
        Ok(out)
    }

    /// Liest bis EOF, aber hoechstens `grenze` Byte. Ueberschreiten ist ein
    /// benannter Fehler, kein stilles Abschneiden.
    fn bis_eof(&mut self, grenze: usize) -> Result<Vec<u8>, NetError> {
        let mut out: Vec<u8> = Vec::new();
        loop {
            self.fuelle()?;
            if self.pos >= self.buf.len() {
                if self.eof {
                    return Ok(out);
                }
                continue;
            }
            let verfuegbar = self.buf.len() - self.pos;
            if out.len() + verfuegbar > grenze {
                return Err(NetError::BodyZuGross { grenze });
            }
            out.extend_from_slice(&self.buf[self.pos..]);
            self.pos = self.buf.len();
        }
    }

    /// Schliesst die Verbindung. Fehler werden IGNORIERT: ein Fehler beim
    /// Schliessen darf eine gute Antwort nicht zur Fehlermeldung machen.
    fn beenden(&mut self) {
        crate::h2geteilt::wir_schliessen("beenden()");
        let _ = self.stream.shutdown(Shutdown::Both);
    }

    /// Gibt die Leitung fuer den Verbindungspool zurueck — oder schliesst
    /// sie, wenn sie nicht zurueckgegeben werden darf.
    ///
    /// added 30.09.2026, Welle 7 / N4. DIE BEDINGUNG IST DIE GANZE
    /// FUNKTION, und sie hat einegemessen Grund: `fuelle` liest in
    /// Stuecken von 8 KiB, und der letzte Lesevorgang holt
    /// regelmaessig **mehr**, als der Body braucht — bei
    /// `Content-Length: 10` liest er bis zu 8192 Byte und laesst den
    /// Rest liegen. Diese Restbytes sind der Anfang der **naechsten**
    /// Antwort. Liegae die Leitung so in den Pool, wuerde die
    /// naechste Anfrage mit einem Statuszeichen statt mit `HTTP/1.1`
    /// beginnen und der Test `pool_gibt_die_verbindung_weiter` wuerde
    /// an einem Fremdfehler scheitern, den niemand vermutet.
    ///
    /// DIE SIGNATUR NIMMT `self` (nicht `&mut self`) UND DAS IST
    /// ABSICHT: der `Leser` wird danach nicht mehr gebraucht, und eine
    /// leere `Box<dyn Leitung>` als Platzhalter — der naechste Weg,
    /// den man geht, wenn man `self` behalten will — waere ein erfundenes
    /// Objekt, dessen `shutdown` ins Leere liefe.
    ///
    /// DARAUS FOLGT AUCH: `entnehmen` gibt **nichts** zurueck, wenn
    /// `eof` gesetzt ist. Bei EOF hat der Server die Verbindung
    /// geschlossen (das ist der Normalfall bei `Connection: close` und
    /// bei Antworten ohne `Content-Length`); eine solche Leitung in den
    /// Pool zu legen, hiesse, den naechsten Aufruf garantiert scheitern
    /// zu lassen.
    fn entnehmen_oder_schliessen(self) -> Option<Box<dyn Leitung>> {
        // M3: die Bedingung ist `pos >= buf.len()`, nicht `buf.is_empty()`.
        // Nach dem Lesen zeigen `pos` und `buf.len()` auf dieselbe
        // Stelle, wenn alles verbraucht ist — `buf` kann aber
        // trotzdem nicht leer sein, weil `fuelle` mit `clear` beginnt
        // und erst dann fuellt. `is_empty` wuerde also eine
        // vollstaendig gelesene Antwort faelschlich als
        // "nicht am Ende" melden und die Leitung wegwerfen.
        if self.pos < self.buf.len() || self.eof {
            let mut leitung = self.stream;
            crate::h2geteilt::wir_schliessen("Restpuffer oder EOF im pooled Puffer");
            let _ = leitung.shutdown(Shutdown::Both);
            return None;
        }
        Some(self.stream)
    }
}

/// Statuszeile + Header + Body, mit allen Faellen, die ein Browser trifft.
fn lese_antwort(
    leser: &mut Leser,
    request: &Request,
    limits: &Limits,
) -> Result<Response, NetError> {
    let status = lese_statuszeile(leser)?;
    // added 01.10.2026 16:9x, T3: die Zeit des ersten Bytes jetzt in die
    // Phasenliste. Nach der Statuszeile, weil dort sicher ein Byte gelesen
    // wurde — `fuelle` merkt es auch bei einem leeren Lesevorgang, und ein
    // „erstes Byte" ohne Inhalt waere eine erfundene Zahl.
    phasen_setzen(Phasen {
        erstes_byte: leser.erstes_byte,
        ..Phasen::leer()
    });
    let (header_raw, trailers_vorhanden) = lese_headerblock(leser)?;
    let (headers, _, set_cookie) = header_auswerten(&header_raw)?;

    // added 02.10.2026 23:0x: der Status wandert mit in die Entscheidung — vorher
    // konnte sie den Code nicht kennen.
    let keep_alive = verbindungswunsch(request, &headers, &status.version, status.code);
    let basis = basis_von(request);

    // Status OHNE Body: 1xx, 204, 304, und jede Antwort auf HEAD. Bei 304
    // darf ein Server Content-Length nennen — der Body wird trotzdem nicht
    // gelesen, sonst blockiert man sich an Bytes, die nicht kommen.
    let ohne_body = request.method == Method::Head
        || status.code == 204
        || status.code == 304
        || (100..200).contains(&status.code);

    let (body, trailers, body_quelle) = if ohne_body {
        (Vec::new(), HashMap::new(), BodyQuelle::KeinBody)
    } else {
        let (b, t, q) = lese_body(leser, &headers, limits, trailers_vorhanden)?;
        (b, t, q)
    };

    Ok(Response {
        status,
        headers,
        body,
        trailers,
        // added 30.09.2026, Welle 7 / N6: alle `Set-Cookie`-Werte, in
        // Empfangsreihenfolge. Siehe die Begruendung am Feld.
        set_cookie,
        body_quelle,
        keep_alive,
        url: basis,
        // Hier ist noch NICHTS entpackt: das macht `fetch_mit` nach der
        // ganzen Antwort, weil erst dann der `Content-Encoding`-Header
        // gelesen ist.
        entpackt: false,
    })
}

/// Liest die Statuszeile und prueft, ob es ueberhaupt HTTP ist.
fn lese_statuszeile(leser: &mut Leser) -> Result<StatusLine, NetError> {
    let roh = leser.zeile(true)?;
    if roh.is_empty() {
        // Nichts gelesen und die Verbindung ist zu: das ist die Meldung
        // "Server hat nichts gesagt", nicht "Antwort ist kaputt".
        return Err(NetError::Receive(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "Verbindung geschlossen, bevor der Server etwas sendete",
        )));
    }
    let text = String::from_utf8_lossy(&roh).to_string();
    let mut teile = text.splitn(3, ' ');
    let version = teile
        .next()
        .filter(|v| v.starts_with("HTTP/"))
        .ok_or_else(|| {
            // Der wichtigste Fall: der Port spricht etwas anderes. Der
            // Benutzer soll "Kein HTTP: <was kam>" sehen, nicht "ungueltige
            // Antwort: keine Version".
            NetError::KeinHttp(format!("erste Zeile war {text:?}, keine HTTP-Statuszeile"))
        })?
        .to_string();
    let code_text = teile
        .next()
        .filter(|c| !c.is_empty())
        .ok_or_else(|| NetError::KeinHttp(format!("Statuszeile ohne Code: {text:?}")))?;
    let code: u16 = code_text.parse().map_err(|_| {
        NetError::KeinHttp(format!(
            "Statuscode nicht lesbar: {code_text:?} in {text:?}"
        ))
    })?;
    if !(100..=599).contains(&code) {
        return Err(NetError::KeinHttp(format!(
            "Statuscode {code} liegt ausserhalb 100-599"
        )));
    }
    let reason = teile.next().unwrap_or("").trim().to_string();
    Ok(StatusLine {
        version,
        code,
        reason,
    })
}

/// Liest alle Headerzeilen. Gibt die Zeilen (ohne Name-Normalisierung)
/// und ob überhaupt ein `Trailer:`-Header da war.
fn lese_headerblock(leser: &mut Leser) -> Result<(Vec<Vec<u8>>, bool), NetError> {
    let mut zeilen: Vec<Vec<u8>> = Vec::new();
    let mut trailer_angekuendigt = false;
    loop {
        let roh = leser.zeile(true)?;
        if roh.is_empty() {
            // Leerzeile = Ende des Headerblocks. Eine Antwort OHNE
            // einzeigen Header ist gueltig (GEMESSEN 29.09.2026: der
            // Test `ohne_laenge_kommt_auch_dann_alles_wenn_der_server_
            // stueckweise_sendet` schickt `HTTP/1.0 200 OK\r\n\r\n`).
            break;
        }
        // Nur die Ankündigung wird hier erkannt; die Trailer selbst kommen
        // hinter dem letzten Chunk.
        //
        // added 01.10.2026 16:6x, T5. **DAS WAR EIN PANIC**, und der Fuzz
        // hat ihn beim **ersten** Durchlauf gefunden. Vorher stand hier
        //     let text = String::from_utf8_lossy(&roh);
        //     roh.iter().position(..).map(|i| (text[..i]…, &text[i + 1..]))
        // Der Index `i` kommt aus den **Rohbytes**, geschnitten wird in der
        // **verlustbehafteten** String — und die ist an anderen Stellen
        // laenger als die Bytes: jedes ungueltige Byte wird zu `U+FFFD`
        // (3 Byte). Ein `:` an Byte 5 liegt dann mitten in einem
        // Ersatzzeichen.
        //
        // GEMESSEN, 01.10.2026 16:5x, `hunderttausend_http_antworten_ohne_
        // panik`: „end byte index 5 is not a char boundary; it is inside
        // '\u{fffd}' (bytes 4..7 of string)". Das ist **erreichbar von
        // aussen**: ein Server, der `X-Bild: \xc3\x28` schickt, reicht.
        //
        // DIE REGEL, und sie gilt fuer jeden Schnitt: **in den Bytes
        // schneiden, in Bytes vergleichen, erst am Ende in Text wandeln.**
        // `to_ascii_lowercase` auf einem `&str` ist nur richtig, wenn der
        // Schnitt an einer Grenze liegt — deshalb der Umweg ueber die
        // Bytes.
        if let Some(i) = roh.iter().position(|b| *b == b':') {
            let name = String::from_utf8_lossy(&roh[..i]).to_ascii_lowercase();
            if name == "trailer" {
                trailer_angekuendigt = true;
            }
        }
        // Faltung (Zeilenanfang mit Leerzeichen) wird zu einer Zeile
        // gemacht — sonst verliert man den Wert.
        if (roh[0] == b' ' || roh[0] == b'\t') && !zeilen.is_empty() {
            if let Some(letzte) = zeilen.last_mut() {
                letzte.extend_from_slice(b" ");
                letzte.extend_from_slice(
                    roh.iter()
                        .copied()
                        .skip_while(|b| *b == b' ' || *b == b'\t')
                        .collect::<Vec<u8>>()
                        .as_slice(),
                );
            }
            continue;
        }
        zeilen.push(roh);
    }
    Ok((zeilen, trailer_angekuendigt))
}

/// Macht aus Headerzeilen eine Map: NAME KLEIN, Wert getrimmt.
///
/// Gross-/Kleinschreibung: `Content-Length`, `content-length` und
/// `CONTENT-LENGTH` sind DERSELBE Header (RFC 9110 §5.1). Wer das nicht
/// tut, laedt Seiten mit einem Content-Length nicht.
fn header_auswerten(zeilen: &[Vec<u8>]) -> Result<GeleseneHeader, NetError> {
    let mut map: HashMap<String, String> = HashMap::new();
    // added 30.09.2026, Welle 7 / N6. DIE DRITTE AUSGABE: alle
    // `Set-Cookie`-Werte in Empfangsreihenfolge. Sie stehen **vor** der
    // Zusammenfassung in `map`, weil dort aus zwei Werten einer wird —
    // und die Kommasyntax von RFC 6265 5.2 kennt kein Komma.
    let mut set_cookie: Vec<String> = Vec::new();
    for zeile in zeilen {
        // added 02.10.2026 20:0x, R40. **DER DOPPELPUNKT WIRD IN DEN BYTES
        // GESUCHT**, und das ist keine Formalie: der Kopf dieser Funktion hat
        // sich (01.10.2026 16:5x) den Fehler „end byte index 5 is not a char
        // boundary“ eingefangen, weil der Schnitt an einem Ersatzzeichen lag.
        // Vorher stand hier `text.find(':')` auf der **verlustbehafteten**
        // Zeichenkette — derselbe Fehler, nur Glueck gehabt.
        let pos = zeile.iter().position(|b| *b == b':').ok_or_else(|| {
            let text = String::from_utf8_lossy(zeile);
            // Kein Doppelpunkt: ein kaputter Header. Frueher wurde er
            // stillschweigend verworfen, und der Body wurde dann als
            // "unbekannte Laenge" gelesen — der Grund fuer die halben Seiten.
            NetError::InvalidResponse(format!("Headerzeile ohne Doppelpunkt: {text:?}"))
        })?;
        // added 02.10.2026 20:0x, R40. **WHITESPACE IM NAMEN IST VERBOTEN**,
        // woertlich RFC 9112 5.1: „No whitespace is allowed between the field
        // name and colon. In the past, differences in the handling of such
        // whitespace have led to **security vulnerabilities** in request routing
        // and response handling.“
        //
        // added 02.10.2026 20:1x: **WAS HIER BISHER PASSIERTE und warum es ein
        // Sicherheitsproblem ist.** An dieser Stelle stand `text[..pos].trim()`.
        // GEMESSEN: eine Zeile `Content-Length : 5` (Leerzeichen **vor** dem
        // Doppelpunkt) landete damit als `content-length: 5` in der Karte — wir
        // werteten sie alsLaengenangabe. Ein Vermittler, der das Leerzeichen
        // **nicht** entfernt (die Norm verlangt es von einem Proxy: „A proxy
        // MUST remove any such whitespace from a response message before
        // forwarding the message downstream“), glaubt, es sei **keine**
        // Laengenangabe und rahmt den Body anders. Weil wir die Verbindung
        // weiterverwenden (Pool), endet die naive Folge: die **naechste** Anfrage
        // auf dieser Verbindung wird vom Vermittler an anderer Stelle gelesen
        // als von uns. Das ist die Smuggling-Primitive, und sie entsteht genau
        // aus dem `trim()`.
        //
        // added 02.10.2026 20:1x: **WARUM SP, HTAB, CR, LF und die uebrigen
        // Steuerzeichen, aber keine Bytes >= 0x80.** Die Norm sagt „no
        // whitespace“; Steuerzeichen koennen nie in einem Namen stehen, und sie
        // sind der Teil, der einen Vermittler irre macht. Nicht-ASCII-Bytes in
        // Namen kommen in der Wild vor (einige Server schicken rohe UTF-8-Namen)
        // und werden von zwei Parteien **gleich** behandelt — sie sind eine
        // Kompatibilitaetsfrage, keine Desync-Frage. Ein Browser, der daran
        // scheitert, verliert Seiten; ein Browser, der das trimmt, kann
        // auseinanderlaufen. Deshalb: Steuerzeichen verbieten, Nicht-ASCII
        // durchlassen, und beides messbar.
        if zeile[..pos].iter().any(|b| *b < 0x21 || *b == 0x7f) {
            return Err(NetError::InvalidResponse(format!(
                "Headername mit Steuerzeichen oder Leerzeichen: {:?}",
                String::from_utf8_lossy(&zeile[..pos])
            )));
        }
        let name = String::from_utf8_lossy(&zeile[..pos]).to_ascii_lowercase();
        if name.is_empty() {
            return Err(NetError::InvalidResponse(format!(
                "Headerzeile ohne Namen: {:?}",
                String::from_utf8_lossy(zeile)
            )));
        }
        // added 02.10.2026 20:1x: der Wert wird **getrimmt** — das ist erlaubt
        // und in 5.1 ausdruecklich vorgesehen („OWS occurring before the first
        // non-whitespace octet of the field line value … is excluded by parsers
        // when extracting the field line value“).
        let wert = String::from_utf8_lossy(&zeile[pos + 1..])
            .trim()
            .to_string();
        // VOR der Zusammenfassung abgreifen. DER GRUND: der Code
        // darunter haengt bei einem zweiten `Set-Cookie` ein `", "` an
        // den ersten Wert, und `a=1, b=2` ist fuer RFC 6265 ein
        // **einziger** Cookie mit einem Komma im Namen — der zweite
        // gaebe verloren (gemessen am 30.09.2026 mit 70 Koepfen).
        if name == "set-cookie" {
            // M4: die Liste waechst mit der Eingabe. Sie ist dadurch
            // begrenzt, dass jede Zeile mindestens 13 Zeichen lang ist
            // (`Set-Cookie: x`) und `Limits::header_bytes` die
            // Gesamtheader deckelt; `fetch_mit_mit` begrenzt die
            // **Verarbeitung** auf
            // `MAX_SET_COOKIE_HEADERS`. Die Liste muss alle behalten,
            // sonst waere die Meldung („N Koepfe verworfen“) falsch.
            set_cookie.push(wert.clone());
        }
        match map.get_mut(&name) {
            // Mehrfach-Header: kommasepariert, wie HTTP es verlangt.
            Some(alt) => {
                alt.push_str(", ");
                alt.push_str(&wert);
            }
            None => {
                map.insert(name, wert);
            }
        }
    }
    let trailer = map.contains_key("trailer");
    Ok((map, trailer, set_cookie))
}

/// Ein gelesener Body: die Bytes, die Kopfzeilen, die dazugehoeren, und
/// woher die Laenge kam.
///
/// added 30.09.2026. Zwei Funktionen gaben denselben Dreier zurueck, und
/// ausgeschrieben war das ein Tuple, das man beim Lesen zweimal
/// nachzaehlen musste. Der Name sagt, was es ist, und die beiden Stellen
/// muessen sich aendern, wenn sich der Satz aendert — das ist der
/// eigentliche Grund fuer den Alias.
type GelesenerBody = (Vec<u8>, HashMap<String, String>, BodyQuelle);

/// Was `header_auswerten` zurueckgibt: die Header-Map, ob ein `Trailer`
/// angekuendigt war, und alle `Set-Cookie`-Werte in Empfangsreihenfolge.
///
/// added 30.09.2026, Welle 7 / N6. DERSELBE GRUND WIE BEI
/// `GelesenerBody`: ausgeschrieben war der Typ an vier Stellen ein
/// Dreier, den man beim Lesen nachzaehlen musste, und clippy hat ihn
/// als `type_complexity` gemeldet. Der Name sagt, was es ist; wer ein
/// Feld hinzufuegt, muss alle vier Stellen anpacken — genau das ist der
/// Zweck.
type GeleseneHeader = (HashMap<String, String>, bool, Vec<String>);

/// Body lesen — die drei Wege, die es gibt, plus der vierte (keiner).
fn lese_body(
    leser: &mut Leser,
    headers: &HashMap<String, String>,
    limits: &Limits,
    _trailer_angekuendigt: bool,
) -> Result<GelesenerBody, NetError> {
    // Transfer-Encoding hat Vorrang vor Content-Length: wenn beides da ist,
    // gewinnt chunked (RFC 9112 §6.1). Sonst smuggling.
    //
    // added 02.10.2026 21:0x, R41. **DIE PRUEFUNG, und sie ist strenger als die
    // alte Zeile `.any(|t| t.trim() == "chunked")`.** GEMESSEN war vorher: der
    // Wert `chunked` wurde true, sobald **irgendein** Komma-Teil `chunked` hiess.
    // Damit sind **zwei** kaputte Antworten durchgerutscht, und beide liefern
    // **kodierte** Bytes als waeren sie Inhalt:
    //
    // * `Transfer-Encoding: chunked, gzip` — die Kodierungen werden in der
    //   angegebenen Reihenfolge angewandt, also erst `chunked` (das Rahmen),
    //   dann `gzip` auf den Inhalt. Wir lesen chunked und geben gzip-Bytes
    //   weiter. Die Norm sagt in 6.3 Regel 4 fuer diesen Fall ausdruecklich:
    //   „If a Transfer-Encoding header field is present and the chunked transfer
    //   coding (Section 7.1) is the **final** encoding“ — `chunked` ist hier
    //   nicht die letzte Kodierung, das Rahmen waere also „bis zum
    //   Verbindungsende lesen“, und der Inhalt ist immer noch gzip-kodiert.
    // * `Transfer-Encoding: gzip` — gar kein chunked; wir fielen auf „bis zum
    //   Verbindungsende lesen“ (das Rahmen ist nach 6.3 Regel 4 richtig) und
    //   gaben die **noch kodierten** Bytes als Body weiter.
    //
    // added 02.10.2026 21:0x: **WARUM ABLEHNEN STATT DEKODIEREN.** Wir koennen
    // es: `Accept-Encoding: gzip, deflate, br` wird gesendet (Zeile 743) und
    // `content_encoding()` entpackt mit `narsil_compress` (Zeile 1918ff). Der
    // Aufwand waere also klein. Trotzdem: `Transfer-Encoding` ist eine
    // Ausnahme, die praktisch niemand mehr benutzt, und fuer die Fehlermeldung
    // „Transfer-Encoding: gzip“ ist fuer den Nutzer **besser** als eine Seite
    // voller Muell. Wer es doch entpacken will, hat es an **einer** Stelle zu
    // tun — genau wie `content_encoding` — und nicht im Body-Rahmen.
    //
    // added 02.10.2026 21:1x: **DIE REGEL, und sie ist eine einzige Zeile:**
    // akzeptiert wird nur, wenn die Kodierliste **genau** `chunked` ist. Alles
    // andere bedeutet: der Inhalt ist noch kodiert, und kodierte Bytes als
    // Inhalt auszugeben ist der Fehler.
    if let Some(te) = headers.get("transfer-encoding") {
        let liste: Vec<String> = te
            .to_ascii_lowercase()
            .split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        if liste.len() != 1 || liste[0] != "chunked" {
            return Err(NetError::InvalidResponse(format!(
                "Transfer-Encoding kann ich nicht lesen: {:?} — erwartet wird \
                 genau `chunked`, sonst ist der Inhalt noch kodiert",
                te
            )));
        }
        return lese_chunked(leser, limits);
    }

    if let Some(laengen_text) = headers.get("content-length") {
        // Doppelte Laengen (`Content-Length: 5` zweimal mit anderem Wert)
        // sind ein Angriff, keine Besonderheit.
        let teile: Vec<&str> = laengen_text.split(',').map(str::trim).collect();
        let mut laenge: Option<usize> = None;
        for teil in teile {
            if teil.is_empty() {
                continue;
            }
            let wert: usize = teil.parse().map_err(|_| {
                NetError::InvalidResponse(format!("Content-Length ungueltig: {teil:?}"))
            })?;
            match laenge {
                None => laenge = Some(wert),
                Some(v) if v == wert => {}
                Some(v) => {
                    return Err(NetError::InvalidResponse(format!(
                        "zwei verschiedene Content-Length: {v} und {wert}"
                    )));
                }
            }
        }
        let laenge = laenge.unwrap_or(0);
        if laenge > limits.body_bytes {
            return Err(NetError::BodyZuGross {
                grenze: limits.body_bytes,
            });
        }
        if laenge == 0 {
            return Ok((Vec::new(), HashMap::new(), BodyQuelle::ContentLength));
        }
        return Ok((
            leser.genau(laenge)?,
            HashMap::new(),
            BodyQuelle::ContentLength,
        ));
    }

    // Weder Content-Length noch chunked: bis zum Verbindungsende lesen.
    // Das ist HTTP/1.0 ueblich und in 1.1 falsch, aber der Client MUSS es
    // lesen — er kann nicht wissen, dass nichts mehr kommt.
    Ok((
        leser.bis_eof(limits.body_bytes)?,
        HashMap::new(),
        BodyQuelle::BisVerbindungsende,
    ))
}

/// Chunked-Body mit Extensions und Trailern.
///
/// Die Chunk-Groesse ist HEXADEZIMAL (`1a` = 26). Wer sie als Dezimal
/// liest, holt 16 Byte zu wenig pro Chunk und die Antwort ist kaputt —
/// genau der Fehler, den `chunk_groesse_ist_hex_und_nicht_dezimal`
/// beweist.
fn lese_chunked(leser: &mut Leser, limits: &Limits) -> Result<GelesenerBody, NetError> {
    let mut body: Vec<u8> = Vec::new();
    loop {
        let roh = leser.zeile(false)?;
        if roh.is_empty() {
            return Err(NetError::ChunkGroesseUngueltig(
                "Chunk-Groessenzeile fehlt".to_string(),
            ));
        }
        let text = String::from_utf8_lossy(&roh).to_string();
        // Chunk-EXTENSION: alles ab dem ersten `;` (`;name=wert`,
        // `;flag`, `;name="mit; semikolon"`). Die Erweiterung ist
        // Information, keine Groesse — sie wird hier bewusst verworfen,
        // aber sie darf die Groesse nicht verschieben.
        let groessen_teil = match text.find(';') {
            Some(i) => text[..i].trim(),
            None => text.trim(),
        };
        if groessen_teil.is_empty() {
            return Err(NetError::ChunkGroesseUngueltig(text));
        }
        let groesse = usize::from_str_radix(groessen_teil, 16)
            .map_err(|_| NetError::ChunkGroesseUngueltig(groessen_teil.to_string()))?;
        if groesse == 0 {
            // Letzter Chunk: Trailer lesen. Die sind normale Header und
            // werden ausgewertet, nicht uebersprungen.
            break;
        }
        if body.len() + groesse > limits.body_bytes {
            return Err(NetError::BodyZuGross {
                grenze: limits.body_bytes,
            });
        }
        body.extend_from_slice(&leser.genau(groesse)?);
        // CRLF nach dem Chunk-Daten. Fehlt es, ist der Stream kaputt —
        // weiterzulesen wuerde Body und Header vermischen.
        let ende = leser.genau(2)?;
        if ende != b"\r\n" {
            return Err(NetError::InvalidResponse(format!(
                "Chunk endet nicht mit CRLF: {ende:?}"
            )));
        }
    }
    // Trailer: Headerzeilen bis zur Leerzeile, mit derselben Grenze wie
    // oben. Unbegrenzte Trailer sind ein Speicherleck, deshalb zaehlen sie
    // in die Headergrenze.
    let trailer_zeilen = lese_trailerblock(leser)?;
    // added 30.09.2026, Welle 7 / N6: die dritte Ausgabe (alle
    // `Set-Cookie`-Werte) wird hier **verworfen**. RFC 9112 Abschnitt
    // 7.1.2: `Set-Cookie` ist kein erlaubter Trailer — es ist ein
    // Zustandswechsel, und ein Browser, der ihn erst am Ende einer
    // Antwort auswertet, haette die Seite schon gezeichnet.
    let (trailer_map, _, _) = header_auswerten(&trailer_zeilen)?;
    Ok((body, trailer_map, BodyQuelle::Chunked))
}

/// Trailer-Header bis zur Leerzeile.
fn lese_trailerblock(leser: &mut Leser) -> Result<Vec<Vec<u8>>, NetError> {
    let mut zeilen = Vec::new();
    loop {
        let roh = leser.zeile(true)?;
        if roh.is_empty() {
            return Ok(zeilen);
        }
        zeilen.push(roh);
    }
}

/// Verbindungswunsch des Servers, mit Fallback auf unseren Wunsch.
///
/// `version` ist die Protokollversion aus der Statuszeile. Sie
/// entscheidet, was ein FEHLENDER `Connection`-Header bedeutet:
///
/// * HTTP/1.1 (RFC 9112 §9.6): "A client that receives a response
///   without a Connection header field MUST treat the connection as
///   persistent." Also: die Verbindung bleibt offen.
/// * HTTP/1.0: das Gegenteil, die Verbindung wird geschlossen.
///
/// GEMESSENER FEHLER, 29.09.2026 23:18: die erste Fassung sagte bei
/// fehlendem Header immer `Schliesst`, auch bei HTTP/1.1. Der Test
/// `keep_alive_antwort_kommt_vollstaendig_und_verbindung_geht_zu`
/// bekam darum `Schliesst` statt `Bleibt`. Praktische Folge: ein
/// Aufrufer, der sich auf dieses Zeichen verlassen will (etwa um einen
/// Verbindungspool zu fuellen), haelt sich fuer falsch informiert.
fn verbindungswunsch(
    request: &Request,
    headers: &HashMap<String, String>,
    version: &str,
    status: u16,
) -> KeepAlive {
    // added 02.10.2026 23:0x, R43. **`101 Switching Protocols` HEISST
    // „VERBINDUNG IST KAPUTT FUER HTTP“**, und bis hier war das nicht einmal eine
    // Frage: `KeepAlive::Bleibt` hiess dann, der Aufrufer legte die Leitung in
    // den Pool (Zeile 3526), und die **naechste** Anfrage ging in eine
    // Verbindung, die der Server auf ein anderes Protokoll umgestellt hat.
    //
    // added 02.10.2026 23:0x: **DIE NORM, und sie sagt sogar, dass der Server
    // das nicht darf.** RFC 9110 7.8 woertlich: „A server **MUST NOT** switch to
    // a protocol that was not indicated by the client in the corresponding
    // request's Upgrade header field.“ Und gleich davor: „The Upgrade header
    // field is intended to provide a simple mechanism for **transitioning from
    // HTTP/1.1 to some other protocol on the same connection**.“
    //
    // added 02.10.2026 23:0x: **WARUM WIR DAS UEBERHAUPT TREFFEN KANNEN.** Wir
    // schicken **nie** ein `Upgrade`: der Anfrageweg filtert es ueber
    // `hop_by_hop` (Zeile 3583, Added 01.10.2026 14:0x). GEMESSEN (23:0x):
    // weder `narsil-browser` noch `narsil-page` noch `narsil-net` enthalten einen
    // WebSocket-Client, und niemand im Projekt erwartet einen `101`. Ein
    // Server, der unaufgefordert umstellt, verletzt 7.8 — und die Folge fuer
    // uns waere ein vergifteter Verbindungspool.
    if status == 101 {
        return KeepAlive::Schliesst;
    }
    // Unser eigener Wunsch steht in `request.headers["connection"]`; er
    // entscheidet aber nur, was WIR schreiben. Massgeblich fuer das
    // Lesen ist, was der Server zurueckschreibt.
    let _ = request;
    match headers.get("connection") {
        Some(v) => {
            let v = v.to_ascii_lowercase();
            if v.split(',').any(|t| t.trim() == "close") {
                KeepAlive::Schliesst
            } else {
                // HTTP/1.1 ohne `close` heisst: die Verbindung bleibt offen.
                KeepAlive::Bleibt
            }
        }
        None => {
            if version == "HTTP/1.1" {
                KeepAlive::Bleibt
            } else {
                // HTTP/1.0 ohne `keep-alive`: die Verbindung wird nach
                // der Antwort geschlossen. Konservativ und ehrlich.
                KeepAlive::Schliesst
            }
        }
    }
}

/// Die Basis-URL: explizit gesetzt, sonst aus Host/Port/Pfad gebaut.
/// Die vollstaendige URL einer Anfrage, fuer `Response.url`.
///
/// added 30.09.2026, Welle 7 / N5. DER WEG: das Schema ueber
/// `ist_https`, der Host ueber `host_mit_port` (das weiss die
/// IPv6-Klammern), der Pfad direkt.
///
/// added 02.10.2026 03:4x, R5g. **`basis_von` ruft jetzt diese Funktion**,
/// und der Grund ist ein gemessener Fehler: `basis_von` baute die Basis
/// selbst und schrieb dabei fest `http://`. Ein relatives
/// `Location: /questions` wurde dadurch gegen `http://` aufgeloest, und der
/// Browser schickte die Anfrage **unverschluesselt an Port 80**
/// (`www.stackoverflow.com`, 02.10.2026 03:1x). Zwei Aufbau-Stellen fuer
/// dieselbe URL sind zwei Schemata fuer dieselbe Adresse.
fn url_aus(request: &Request) -> String {
    let mut s = String::from(if ist_https(request) {
        "https://"
    } else {
        "http://"
    });
    s.push_str(&request.host_mit_port());
    s.push_str(&request.path);
    s
}

fn basis_von(request: &Request) -> String {
    if let Some(b) = &request.basis {
        return b.clone();
    }
    //
    // added 02.10.2026 03:4x, R5g. **AN `url_aus` DELEGIERT**, und vorher
    // stand hier ein zweiter, eigener Aufbau mit festem `http://`.
    // GEMESSEN, was das kostete (`www.stackoverflow.com`, 02.10.2026 03:1x,
    // ein einziger `fetch`):
    //
    //     hop: stackoverflow.com:443 https=true  /         -> 302 "/questions"
    //     hop: stackoverflow.com:80  https=false /questions -> 301 "https://…"
    //     hop: stackoverflow.com:443 https=true  /questions -> 403
    //
    // Die mittlere Zeile ist der Befund: der Server schickt ein
    // **relatives** `Location: /questions` (RFC 9110 15.4 erlaubt das), und
    // das wurde gegen eine Basis mit Schema `http` aufgeloest — der Browser
    // schickte die Anfrage danach im **Klartext an Port 80**, obwohl er auf
    // https war.
    //
    // DIE NORM, woertlich (RFC 3986 5.3): eine relative Referenz wird gegen
    // die Basis **einschliesslich ihres Schemas** aufgeloest; das Schema
    // gehoert zur Basis und wird hier nicht erfunden.
    //
    // WARUM `url_aus` und nicht ein dritter Aufbau: es ist dieselbe URL,
    // und `url_aus` bestimmt das Schema ueber `ist_https` — dieselbe
    // Funktion, die auch `pool_schluessel` benutzt. Damit kann die Basis
    // nicht „https" sein, waehrend der Pool-Schluessel „http" sagt.
    url_aus(request)
}

// -----------------------------------------------------------------------
// Umleitungen: RFC 3986, Abschnitt 5 (die ORIGINAL-Fassung, nicht die
// Eigenloesung aus narsil-page — der Vergleich steht im Bericht).
// -----------------------------------------------------------------------

/// Baut aus `location` und der Basis ein neues Ziel.
///
/// `location` ist das, was im `Location`-Header stand, `basis` die
/// Adresse, die WIR zuletzt angefragt haben. Ein Server schickt den
/// Pfad der Anfrage nicht mit — ohne die Basis koennte `Location: neu`
/// nicht aufgeloest werden.
///
/// `code` entscheidet ueber die METHODE des naechsten Sprungs. Das ist
/// der Unterschied, der in meinem Bericht als offene Luecke stand
/// (29.09.2026, Abschnitt 5, „Umleitungsmethode 303").
/// added 02.10.2026 14:6x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// **DER SPRUNG BEWAHRT DAS SCHEMA** — dieser Test ist der Beweis, dass er es tut.
/// GEMESSEN war vorher das Gegenteil: `verbinde` meldete fuer die zweite
/// Verbindung `tls=false` bei einem `302` auf **https** mit einem Port **nicht
/// 443** — die Umleitung wurde im **Klartext** abgerufen.
///
/// added 02.10.2026 14:6x: **UND DREI FAELLE**, weil ein Test mit nur dem
/// Positivfall auch dann gruen waere, wenn `umleitung_ziel` pauschin `true`
/// setzte — und das waere bei einem Spruch von https auf http ein **Fehler** mit
/// umgekehrter Richtung.
#[cfg(test)]
mod umleitung_schema_tests {
    use super::*;

    /// added 02.10.2026 14:6x: **DER PORT IST 9443**, und das ist der ganze
    /// Punkt: die alte Regel erkannte https am Port **443**, sonst nur am Schema
    /// im Host — und `url_teile` liefert einen Host **ohne** Schema.
    const PORT: u16 = 9443;

    #[test]
    fn ein_relativer_sprung_behaelt_https_auch_auf_anderem_port() {
        let alt = Request::get("https://beispiel.test", PORT, "/a");
        let neu = umleitung_ziel(&alt, 302, "/b").expect("Sprung");
        assert_eq!(neu.https, Some(true), "https bleibt https");
        assert!(ist_https(&neu), "und `ist_https` sagt es auch");
        assert_eq!(neu.port, PORT, "der Port bleibt");
        assert_eq!(neu.host, "beispiel.test", "der Host traegt kein Schema");
    }

    #[test]
    fn ein_sprung_auf_http_wird_kein_https() {
        let alt = Request::get("https://beispiel.test", PORT, "/a");
        let neu = umleitung_ziel(&alt, 302, "http://anders.test:8080/c").expect("Sprung");
        assert_eq!(
            neu.https,
            Some(false),
            "GEMESSEN: ein Sprung nach http ist http"
        );
        assert!(!ist_https(&neu), "und `ist_https` sagt es auch");
        assert_eq!(neu.port, 8080, "der Port aus der Ziel-URL");
    }

    #[test]
    fn ein_sprung_auf_port_443_bleibt_https_auch_ohne_die_angabe() {
        // added 02.10.2026 14:7x: **DIE GEGENPROBE ZUM ALTEN VERHALTEN**, und sie
        // erklaert, warum der Fehler so lange unentdeckt blieb: bei Port 443
        // stimmte `ist_https` aus dem **Port** heraus auch ohne die Angabe. Wer
        // nur diesen Fall prueft, sieht nichts.
        let alt = Request::get("https://beispiel.test", 443, "/a");
        let neu = umleitung_ziel(&alt, 301, "/b").expect("Sprung");
        assert_eq!(neu.https, Some(true));
        assert_eq!(neu.port, 443);
        assert!(
            ist_https(&neu),
            "GEMESSEN: hier hat die alte Regel richtig gelegen"
        );
    }
}

/// added 02.10.2026 15:2x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// **BLEIBT DIE HERKUNFT?** und die Definition ist die der Spezifikation
/// „Referrer Policy“, Abschnitt 2: Herkunft ist **Schema + Host + Port**. Alle
/// drei, nicht zwei: `https://x` und `http://x` sind verschiedene Herkünfte, und
/// `https://x:8443` ist eine dritte.
///
/// added 02.10.2026 15:2x: **WARUM EIGENE FUNKTION UND KEIN `==` AUF DEN
/// HOST**: `aktuelle.host` kann ein Schema tragen (das ist im Crate belegt,
/// `host_ohne_schema` existiert genau dafuer), und das Ziel aus `url_teile`
/// nie. Ein Textvergleich wuerde also bei `https://x` gegen `x` „verschieden“
/// sagen — und **entweder** zu oft streichen **oder**, was schlimmer waere, bei
/// gleicher Herkunft ein `Cookie` mitnehmen.
/// added 02.10.2026 15:2x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// **DIE DREI HERKUNFTSTEILE, einzeln**, weil die Spezifikation sie einzeln
/// nennt und ein Test, der nur „gleicher Host“ prüft, zwei davon für erledigt
/// hält. GEMESSEN ist die Reihenfolge entstanden: der erste Aufbau dieses Tests
/// hatte **beide** Koppfe gleich und meldete trotzdem „verschiedene Herkunft“,
/// und erst der Vergleich der drei Felder hat es gezeigt.
#[cfg(test)]
/// added 04.10.2026 10:1x, R73. **DIE LADUNGSMESSUNG AUF DEN ZEHN
/// REFERENZSEITEN** — sie ist ein `#[ignore]`, weil sie **Netz** braucht
/// (dieselbe Konvention wie `narsil-tls/tests/*`; `PROBLEME.txt` Nr. 30 zaehlt
/// ignorierte Tests nicht als gruen).
///
/// **BEFEHL DER PROBE** (die Zeile, die der Bericht zitieren muss):
///
///     werkzeuge/bau.sh narsil-net test ladungsmessung -- --ignored --nocapture
///
/// **WAS „VORHER/NACHHER“ HIER BEDEUTET**, und es sind **zwei Abrufe** desselben
/// Hosts: (1) **VORHER** = der **erste** Abruf: TCP, TLS-Handshake, Anfrage —
/// die **kalte** Zahl. (2) **NACHHER** = der **zweite** Abruf **desselben
/// Hosts**: die Leitung liegt im Pool (`pool.rs`), also **ohne** Handshake. Die
/// Differenz ist der **Gewinn der Wiederverwendung**, und beide Abrufe sind
/// **gleich**, damit der Vergleich etwas bedeutet. (3) **PARALLEL** = vier
/// Abrufe gleichzeitig auf vier Threads: die Frage, ob der Pool **mehrere**
/// Verbindungen herausgibt oder alle vier auf **eine** legt.
mod ladungsmessung {
    use super::*;
    use std::time::{Duration, Instant};

    /// added 10:1x: **DIE ZEHN REFERENZSEITEN**, woertlich aus
    /// `team/AUFTRAG_CODEX_8.txt` (Z. 17). `https`, weil die Messung den **Host**
    /// vergleicht — und **ohne** Pfad, weil jede Seite zweimal denselben Host
    /// nennt (sonst waere der „warme“ Abruf ein anderer Host).
    const REFERENZSEITEN: [&str; 10] = [
        "https://wikipedia.org/",
        "https://de.wikipedia.org/wiki/Rust_(Programmiersprache)",
        "https://news.ycombinator.com/",
        "https://github.com/",
        "https://www.rust-lang.org/",
        "https://developer.mozilla.org/",
        "https://nixos.org/",
        "https://docs.rs/",
        "https://www.debian.org/",
        "https://duckduckgo.com/",
    ];

    /// added 10:1x: **DIE ZEITGRENZE**, und sie ist Pflicht: ein Netztest ohne
    /// Frist ist ein Haenger, der den ganzen Lauf mitnimmt (dieselbe Regel wie
    /// `FRIST` in `abnahme_end_to_end`). **15 s** je Abruf: GEMESSEN kalt
    /// 100–400 ms, und ein haengender Server soll nach 15 s **als Fehler**
    /// auffallen, nicht als Wartezeit.
    fn grenzen() -> Limits {
        Limits::default()
    }

    /// added 10:1x: **EIN ABRUF**: Millisekunden, **ob** die Leitung aus dem Pool
    /// kam, und die **Byte-Zahl**. GEMESSEN wird die Poolgroesse **vor** und
    /// **nach** dem Abruf — das ist der Unterschied zwischen „wiederverwendet“
    /// und „neu aufgebaut“, und es ist **beobachtbar** statt behauptet.
    fn abruf(url: &str, speicher: &narsil_tls::kette::Vertrauensspeicher) -> (u128, bool, usize) {
        //
        // added 04.10.2026 10:4x, R73. **`url_teile` STATT `host_ohne_schema`** —
        // und das ist der Unterschied zwischen einer Messung und **0 ms**. Mein
        // erster Lauf gab 0,04 s fuer **alle** zehn Seiten und `bytes=0`:
        // `host_ohne_schema("https://wikipedia.org/")` liefert
        // `wikipedia.org/` — **mit Slash** — und das ist kein Host, also
        // scheitert die **Namensauflösung sofort**. GEMESSEN: 10 von 10 Seiten
        // mit `kalt=0ms warm=0ms ok=0/4`. **GEMESSENE LEHRE:** eine Messung, die
        // `0` liefert, ist fast immer eine **kaputte** Messung; das steht jetzt
        // als Kommentar da, damit der naechste nicht `0` als "schnell"
        // liest.
        let (host, port, pfad) = url_teile(url);
        let schluessel = pool::Schluessel {
            tls: true,
            host: host.clone(),
            port,
        };
        let vorher = pool::groesse_von(&schluessel);
        let anfrage = Request::get(&host, port, &pfad);
        let mut grenzen = grenzen();
        grenzen.gesamt = Duration::from_secs(15);
        let t0 = Instant::now();
        let ergebnis = fetch_mit_speicher(&anfrage, grenzen, None, speicher);
        let ms = t0.elapsed().as_millis();
        // added 10:2x: **`danach` wird nicht ausgewertet**, und das ist die
        // Beobachtung selbst: GEMESSEN ist die Zahl **vor** dem Abruf, und
        // entscheidend ist, dass sie **davor** schon > 0 war — dann kam die
        // Leitung aus dem Pool. Die Zahl **nachher** zu pruefen wuerde
        // behaupten, dass der Abruf sie veraendert; sie tut es nicht (der
        // Eintrag wandert zwischen Pool und Aufrufer).
        let _danach = pool::groesse_von(&schluessel);
        let bytes = ergebnis.as_ref().map(|a| a.body.len()).unwrap_or(0);
        (ms, vorher > 0 && ergebnis.is_ok(), bytes)
    }

    #[test]
    #[ignore = "braucht das Netz; Befehl im Modulkopf"]
    fn ladungsmessung_auf_den_zehn_referenzseiten() {
        let speicher = vertrauensspeicher().expect("CA-Bundle lesbar (NARSIL_CA_BUNDLE)");
        //
        // added 04.10.2026 11:2x, R73. **DER CACHE WAR AN, UND DIE „KALTE“
        // ZEIT WAR DAMIT EINE UEBER DEN CACHE GEMACHTE.** Das ist die Ursache der
        // `UNPLAUSIBEL`-Marke von 10:9x — **gefunden, nicht geraten**:
        // * `cache.rs:179` `pfad_aus_umgebung().unwrap_or_else(narsil_cache::
        //   standard_pfad)` — **ohne** `NARSIL_CACHE_PFAD` wird **nicht** gar nichts
        //   gemacht, sondern der Vorgabepfad genommen. GEMESSEN: `env | grep -c
        //   NARSIL_` = **0**, und `find ~/.cache/narsil -type f | wc -l` = **138**
        //   mit mtime **16:52–16:57** — genau meine Messlaeufe. Der Cache war also
        //   nicht nur an, er wurde von **dieser** Sonde gefuellt.
        // * `lib.rs:1699` fragt ihn im Abrufweg ab (`cache::nachschlagen`), und
        //   GEMESSEN lieferte `docs.rs` **1 ms bei 21 366 Byte** und
        //   `debian.org` **0 ms bei 16 143 Byte**: unter einer Millisekunde geht
        //   kein TLS-Handshake, das kann nur ein **Treffer** auf der Platte sein.
        // * **DIE WIRKUNG AUF DIE ZAHLEN, und sie geht beide Wege:** `docs.rs`
        //   und `debian.org` waren in **jedem** Lauf „kalt 0–1 ms, `aus_pool=
        //   false`“, die anderen **acht** Seiten 360–1277 ms. Das ist nicht
        //   Ausreiher, das ist **gemischt gemessen** — eine Tabelle, in der zwei
        //   Zeilen aus dem Cache kommen und acht aus dem Netz, sagt nichts ueber
        //   die Wiederverwendung aus.
        // * **WARUM NICHT EINFACH LOESCHEN**, wie der Haenger es macht
        //   (`lib.rs:7573`)? Dort wird `NARSIL_CACHE_PFAD` **gelesen** und nur bei
        //   gesetztem Wert geloescht — GEMESSEN ist die Variable **nicht**
        //   gesetzt, dort wird also **nichts** geloescht. Und `~/.cache/narsil`
        //   ist der Cache des **Benutzers**, nicht der eines Tests: ihn zu
        //   loeschen waere in einem Test nebenbei eine Handlung im Verzeichnis
        //   des Nutzers. `abschalten_setzen` schaltet **diesen Prozess** ab und
        //   fasst keine Datei an.
        // * **ZURUECKGESETZT WIRD ES** mit `None` am Ende — `None` heisst laut
        //   `cache.rs:70` „nach `NARSIL_KEIN_CACHE` fragen“, also der dokumentierte
        //   Ausgangszustand. Eine Sonde, die den Zustand eines ganzen Testbinarys
        //   fuer die folgenden Tests veraendert, waere ein eigener Fehler.
        cache::abschalten_setzen(Some(true));
        println!(
            "R73-MESSUNG {} Referenzseiten, Frist 15 s je Abruf, cache_aus={}",
            REFERENZSEITEN.len(),
            cache::ist_abgeschaltet()
        );
        let mut kalt_summe = 0u128;
        let mut warm_summe = 0u128;
        let mut gezaehlt = 0usize;
        for url in REFERENZSEITEN {
            let (kalt, _, bytes) = abruf(url, speicher);
            let (warm, aus_pool, _) = abruf(url, speicher);
            // added 10:4x: **DER FEHLER WIRD GENANNT**, wenn keiner der beiden
            // Abrufe Bytes brachte: `bytes=0` allein sagt nicht, ob der Server
            // 400 schickte oder der **Zertifikatsaufbau** scheiterte — und diese
            // Unterscheidung ist der ganze Unterschied zwischen "Seite ist
            // unerreichbar" und "unser Client kann sie nicht".
            if bytes == 0 {
                let (h2, port2, pfad2) = url_teile(url);
                let probe = Request::get(&h2, port2, &pfad2);
                let mut g = grenzen();
                g.gesamt = Duration::from_secs(15);
                match fetch_mit_speicher(&probe, g, None, speicher) {
                    Ok(a) => println!(
                        "R73-FEHLER {url}: {} bytes trotzdem, status {}",
                        a.body.len(),
                        a.status.code
                    ),
                    Err(e) => println!("R73-FEHLER {url}: {e}"),
                }
            }
            let t0 = Instant::now();
            //
            // added 04.10.2026 10:3x, R73. **UND ZWEI FEHLER, DIE DIESER BLOCK
            // GESUCHT HAT, BEVOR ER LAUFEN KONNTE** — beide vom Compiler, und
            // beide lehrreich:
            //   (1) `h.thread().join()` — `ScopedJoinHandle` hat **kein**
            //       `thread()`, das ist der **fremde** `JoinHandle`.
            //   (2) `ScopedJoinHandle::join(self)` nimmt **selbst**, nicht
            //       `&self` (E0599).
            // **DER WEG, DER BEIDES UMGEHT: sofort joinen.** Der Handle wird
            // damit ein **Wert**, und die vier Ergebnisse liegen als `Vec<bool>`
            // vor — kein Handle bleibt unausgewaertet und keiner wird
            // **weggeworfen**, was bei `let _ = h` genau die stille Form waere.
            let ok = std::thread::scope(|s| {
                let mut hn: Vec<bool> = Vec::new();
                for _ in 0..4 {
                    let sp: &narsil_tls::kette::Vertrauensspeicher = speicher;
                    let u = url.to_string();
                    hn.push(
                        s.spawn(move || {
                            let (h, port, pfad) = url_teile(&u);
                            let anfrage = Request::get(&h, port, &pfad);
                            let mut g = grenzen();
                            g.gesamt = Duration::from_secs(15);
                            match fetch_mit_speicher(&anfrage, g, None, sp) {
                                Ok(_) => true,
                                Err(e) => {
                                    // added 10:5x: **DER FEHLER WIRD GENANNT.** Die
                                    // erste Fassung sammelte nur `is_ok()`, und
                                    // GEMESSEN war `ok=0/4` bei `parallel4=0ms` —
                                    // **0 ms** kann keine Netzzeit sein, also war
                                    // es ein **sofortiger** Fehler, und welchen,
                                    // sah niemand. `rprintln!` statt `println!`,
                                    // weil vier Threads **gleichzeitig** schreiben.
                                    eprintln!("R73-PARALLEL-FEHLER {u}: {e}");
                                    false
                                }
                            }
                        })
                        .join()
                        .unwrap_or(false),
                    );
                }
                hn.iter().filter(|ok| **ok).count()
            });
            let par_ms = t0.elapsed().as_millis();
            //
            //
            // added 04.10.2026 10:8x, R73. **`checked_div` STATT `if kalt > 0`**,
            // und das kam von clippy: `manual_checked_div` (GEMESSEN als
            // `warning: manual checked division` auf `lib.rs:5789` — der
            // **einzige** Clippy-Treffer des ganzen Laufs).
            //
            // added 10:8x: **UND ZWEI FALSCHE WEGE, BEIDE VON DER MESSUNG
            // ERWISCHT** — sie stehen hier, weil die Versuchung beim naechsten
            // Mal dieselbe ist.
            //
            // (1) **`kalt.max(1)`** loest die Division auf, meldet aber bei
            //     `kalt = 0 ms` (GEMESSEN so bei `docs.rs` und `debian.org`)
            //     **`gewinn=100 %`** — das Gegenteil von gemessen. Ein Kommentar,
            //     der die eigene Begruendung traegt, ist kein Beweis.
            // (2) **`kalt.checked_div(1)`** sieht manisch richtig aus und ist
            //     **falsch**: `0.checked_div(1)` ist **`Some(0)`**, denn durch 1
            //     teilen gelingt immer — GEMESSEN als
            //     `panicked at lib.rs:5807: attempt to divide by zero`.
            //     **ZU PRUEFEN IST DER DIVIDEND, NICHT DER TEILER.**
            let gewinn = (warm * 100)
                .checked_div(kalt)
                .map_or(0, |anteil| 100u128.saturating_sub(anteil));
            //
            // added 04.10.2026 10:9x, R73. **DIE PLAUSIBILITAETS-MARKE**, und
            // sie ist aus einem **eigenen Misstrauen** entstanden: GEMESSEN
            // meldeten `docs.rs` und `www.debian.org` **0 ms bei 20 045 Byte**.
            // Das kann **nicht** stimmen — ein kalter Abruf braucht TCP und
            // TLS-Handshake, also mindestens einen Rundlauf. **STATT DIE ZAHL
            // ZU VERVOELFACHTIGEN** (R73-LEHRE: `0` heisst meistens „kaputt“,
            // nicht „schnell“) markiert die Zeile den Fall als `UNPLAUSIBEL`.
            // **DIE URSACHE IST OFFEN** und damit eine offene Stelle, kein
            // Ergebnis: die Marke sagt nur „glaub dieser Zeile nicht“.
            //
            // added 04.10.2026 11:2x, R73. **UND DIE MARKE BLEIBT**, obwohl die
            // Ursache jetzt bekannt ist — sie ist der **Waechter**, der die
            // Verschmutzung gefunden hat, und sie haette die Verschmutzung ohne
            // Cache-Schalter **in jeden kuenftigen Lauf** wieder durchgelassen.
            // Sie bleibt also drin, mit derselben Wirkung: der Fall wird
            // **markiert statt veroeffentlicht**.
            let unplausibel = if (bytes > 0 && kalt == 0) || (ok == 4 && par_ms < 5) {
                " UNPLAUSIBEL-messung-offen"
            } else {
                ""
            };
            println!(
                "R73-SEITE {url} kalt={kalt}ms warm={warm}ms gewinn={gewinn}% aus_pool={aus_pool} \
                 parallel4={par_ms}ms ok={ok}/4 bytes={bytes}{unplausibel}"
            );
            kalt_summe += kalt;
            warm_summe += warm;
            gezaehlt += 1;
        }
        cache::abschalten_setzen(None);
        // added 04.10.2026 11:3x, R73. **DIE CACHE-STATISTK IN DER AUSGABE**, weil
        // „der Cache war aus“ sonst eine **Behauptung** waere. `statistik()`
        // liefert (Treffer, Fehlversuche); bei ausgeschaltetem Cache bleibt sie
        // auf (0, 0), und **das** ist der Beweis — vorher stand sie nach einem
        // Lauf bei gemessenen Treffern.
        let (tr, fm) = cache::statistik();
        println!("R73-SUMME ueber {gezaehlt} Seiten: kalt={kalt_summe}ms warm={warm_summe}ms");
        println!("R73-CACHE nach dem Lauf: treffer={tr} fehlversuche={fm} (0/0 = aus)");
    }
}

#[cfg(test)]
/// added 04.10.2026 09:6x, R73. **`includeSubDomains` HAT KEINEN TEST GEHABT** —
/// GEMESSEN: `grep -c "assert"` ueber `src/` **und** `tests/` ergab **null**
/// Treffer fuer `includeSubDomains`, obwohl der Parser sie seit Welle 7/N6
/// liest. **UND DIE DIREKTIVE STEHT IN RFC 6797 6.1**, woertlich dort:
/// „If the includeSubDomains directive is present, then the UA MUST apply the
/// HSTS policy to the domain and its subdomains“. Das ist eine **MUST**, und
/// eine MST-Regel ohne Test ist eine **Behauptung**.
mod hsts_werkzeug_tests {
    use super::*;
    // added 09:7x, R73: `hsts_preload` liegt in `sitzung.rs` und ist nicht im
    // Wurzel-`use` — GEMESSEN als Compilerfehler E0425. Es ist bewusst **nicht**
    // `pub use`: der Import gehoert in den Test, nicht in die Crate.
    use crate::sitzung::hsts_preload;

    /// added 09:6x: **DIE SECHS FAELLE DES PARSERS**, inklusive der zwei, die
    /// man falsch baut: `includeSubDomains` **ohne** `=` und **mit** `=true`
    /// (beides nach RFC 6797 6.1 **ohne** Wert — `=true` ist **kein** gueltiger
    /// Header und wird **nicht** akzeptiert).
    #[test]
    fn hsts_wert_lesen_trennt_max_age_und_subdomains() {
        // (1) **NUR max-age**.
        assert_eq!(hsts_wert_lesen("max-age=31536000"), (Some(31536000), false));
        // (2) **BEIDE**, in der Reihenfolge der Norm.
        assert_eq!(
            hsts_wert_lesen("max-age=31536000; includeSubDomains"),
            (Some(31536000), true)
        );
        // (3) **UMGEKEHRTE REIHENFOLGE** — der Header ist eine **Menge**, keine
        // Liste; wer auf die Reihenfolge baut, baut auf Zufall.
        assert_eq!(
            hsts_wert_lesen("includeSubDomains; max-age=100"),
            (Some(100), true)
        );
        // (4) **`includeSubDomains` OHNE WERT** — die Norm schreibt die
        // Direktive **ohne** Argument, und das ist der Fall, den ein
        // `split_once('=')`-Zwang zerstoert haette.
        assert_eq!(hsts_wert_lesen("includeSubDomains"), (None, true));
        // (5) **`includeSubDomains=true` ist NICHT die Direktive** — mit `=` und
        // Wert wird sie **nicht** gesetzt; `max-age` bleibt unberuehrt.
        assert_eq!(
            hsts_wert_lesen("max-age=5; includeSubDomains=true"),
            (Some(5), false)
        );
        // (6) **SCHREIBWEISE UNABHAENGIG** — HTTP-Kopfzeilennamen sind es nach
        // RFC 9110 5.1 nicht, der Parser vergleicht es trotzdem so (GEMESSEN:
        // `eq_ignore_ascii_case`), und das ist fuer `includeSubDomains`
        // **richtig**, weil es in der Praxis in Grossbuchstaben steht.
        assert_eq!(
            hsts_wert_lesen("MAX-AGE=7; INCLUDESUBDOMAINS"),
            (Some(7), true)
        );
    }

    /// added 09:6x: **DIE PRELOAD-LISTE UND DIE REFERENZSEITEN**, gemessen am
    /// 04.10.2026 gegen `https://hstspreload.org/api/v2/status?domain=…`:
    ///
    ///     wikipedia.org  -> preloaded     github.com     -> preloaded
    ///     ycombinator.com-> unknown       rust-lang.org -> unknown
    ///     mozilla.org    -> unknown       nixos.org      -> unknown
    ///     docs.rs        -> unknown       debian.org     -> unknown
    ///     duckduckgo.com -> unknown
    ///
    /// **UND WAS DARAUS FOLGT, ohne Raten:** die **beiden gemessen preloadeten**
    /// Domains sind ueber ihre Subdomains gedeckt (wikipedia.org deckt
    /// de.wikipedia.org), und die **uebrigen acht sind es nicht** — und werden
    /// es auch **nicht** durch Eintragen, denn das wuerde den Browser **strenger**
    /// machen als die echten und `http://` zu Seiten sperren, die es noch
    /// anbieten. **Diese Liste ist damit eine Liste von GEMESSENEN Tatsachen,
    /// nicht von Vermutungen ueber „grosse Seiten“.**
    #[test]
    fn hsts_preload_zeigt_die_drei_gruppen_der_referenzseiten() {
        //
        // added 04.10.2026 09:8x, R73. **MEIN ERSTER TEST HATTE FALSCH
        // GRUPPIERT, UND DIE FEHLERMELDUNG HAT ES GEZEIGT**: ich hatte
        // `developer.mozilla.org` zu den **nicht** gedeckten Domains gesetzt —
        // weil die API fuer `mozilla.org` `unknown` sagt. **ABER**
        // `mozilla.org` steht in **unserer** Liste (sie kommt aus Chromiums
        // eigener, groesserer Liste), also **deckt** sie
        // `developer.mozilla.org`. **DIE DREI GRUPPEN SIND VERSCHIEDEN**, und
        // sie zu vermischen war der Fehler:
        //
        // (A) **VON DER API BESTAETIGT** (`status: preloaded`, 04.10.2026):
        assert!(
            hsts_preload("wikipedia.org"),
            "wikipedia.org: API sagt preloaded"
        );
        assert!(
            hsts_preload("de.wikipedia.org"),
            "de.wikipedia.org faellt unter wikipedia.org"
        );
        assert!(hsts_preload("github.com"), "github.com: API sagt preloaded");
        //
        // (B) **IN UNSERER LISTE, ABER VON DER API NICHT BESTAETIGT** — die
        // Eintraege kommen aus Chromiums Liste, die ich **nicht messen** kann
        // (sie liegt nur im Quellbaum). **GEMESSEN** sagt die API `unknown` fuer
        // `mozilla.org`; **unsere** Liste enthaelt es. Der Test sagt beides:
        assert!(
            hsts_preload("developer.mozilla.org"),
            "mozilla.org steht in unserer Liste (Chromium), die API kennt es nicht"
        );
        //
        // (C) **WEDER IN UNSERER LISTE NOCH IN DER API** — und sie werden
        // **nicht** eingetragen: das wuerde den Browser **strenger** machen als
        // die echten und `http://` zu Seiten sperren, die es noch anbieten.
        for h in [
            "news.ycombinator.com",
            "www.rust-lang.org",
            "nixos.org",
            "docs.rs",
            "www.debian.org",
            "duckduckgo.com",
        ] {
            assert!(
                !hsts_preload(h),
                "{h}: nicht in unserer Liste und von der API nicht bestaetigt"
            );
        }
        //
        // added 09:8x: **UND EIN GEGENPROBE-PUNKT**, der beweist, dass (C) nicht
        // „leer herausgeht“: eine Domain, die **nur** als **Subdomain** von (C)
        // waere, ist ebenfalls **nicht** gedeckt — `hsts_preload` haengt an
        // **Eintraegen**, nicht an Aehnlichkeit.
        assert!(
            !hsts_preload("forum.nixos.org"),
            "Subdomain einer nicht gelisteten Domain ist auch nicht gelistet"
        );
        // added 09:8x: **UND** eine Domain, die nur **aehnlich** heisst:
        // `notgithub.com` darf **nicht** durch `github.com` gedeckt werden —
        // GEMESSEN, dass der Vergleich den Punkt **mitprueft**
        // (`sitzung.rs`, `hsts_preload`), sonst waere jede Domain mit
        // `github` am Ende faelschlich HSTS-erzwungen.
        assert!(
            !hsts_preload("notgithub.com"),
            "aehnlicher Name ist keine Subdomain"
        );
    }
}

#[cfg(test)]
mod herkunftsgrenze_tests {
    use super::*;

    fn alt(https: bool, host: &str, port: u16) -> Request {
        let mut r = Request::get(host, port, "/");
        r.https = Some(https);
        r
    }

    #[test]
    fn gleiche_herkunft_bleibt_auch_mit_schema_im_host() {
        // added 02.10.2026 15:2x: **DER HOST MIT SCHEMA**, und das ist der Fall,
        // der den ersten Aufbau scheitern liess: `alt` traegt `https://localhost`,
        // das Ziel aus `url_teile` niemals. Ohne `host_ohne_schema` waeren die
        // beiden Hosts verschieden — und **jeder** Sprung haette `Cookie` und
        // `Authorization` verloren, auch bei gleicher Herkunft.
        let a = alt(true, "https://localhost", 8443);
        let n = alt(true, "localhost", 8443);
        assert!(
            herkunft_bleibt(&a, &n),
            "gleiche Herkunft trotz Schema im Host"
        );
    }

    #[test]
    fn jeder_der_drei_teile_allein_kann_die_herkunft_wechseln() {
        let a = alt(true, "localhost", 8443);
        assert!(
            !herkunft_bleibt(&a, &alt(false, "localhost", 8443)),
            "Schema"
        );
        assert!(
            !herkunft_bleibt(&a, &alt(true, "anders.test", 8443)),
            "Host"
        );
        assert!(!herkunft_bleibt(&a, &alt(true, "localhost", 9443)), "Port");
    }
}

fn herkunft_bleibt(alt: &Request, neu: &Request) -> bool {
    ist_https(alt) == ist_https(neu)
        && host_ohne_schema(&alt.host) == host_ohne_schema(&neu.host)
        && alt.port == neu.port
}

fn umleitung_ziel(aktuelle: &Request, code: u16, location: &str) -> Result<Request, NetError> {
    let basis = basis_von(aktuelle);
    let ziel = loese_auf(&basis, location)?;
    let (host, port, pfad) = url_teile(&ziel);
    let mut neu = Request::get(&host, port, &pfad);
    // GEMESSENER FEHLER, 29.09.2026 23:24: die Header des urspruenglichen
    // Requests gingen bei JEDER Umleitung verloren, weil `neu` ein frischer
    // Request war. Der Test `post_303_verliert_den_body_und_die_body_header`
    // sah das: `X-bleibt: ja` kam beim zweiten Request nicht an.
    //
    // FUER DEN BENUTZER heisst das: ein Login-Cookie, ein
    // `Authorization`-Kopf, ein `Accept: application/json` — nach der
    // ersten Umleitung nicht mehr vorhanden. Die Seite ware "nicht
    // angemeldet" oder leer, ohne dass irgendwo ein Fehler steht.
    neu.headers = aktuelle.headers.clone();

    if naechste_methode(aktuelle.method, code) != aktuelle.method {
        neu.method = naechste_methode(aktuelle.method, code);
        // Der Body gehoert zur alten Anfrage. Ein GET mit Body ist kein
        // GET, und der Body landet sonst als Unsinn in der Anfragezeile
        // des Servers.
        neu.body = None;
        // ... und die Header, die NUR zum Body gehoeren. `Content-Type`
        // an einem GET beschreibt nichts, `Content-Length` ohne Body
        // wuerde den Server auf leere Daten warten lassen.
        //
        // Andere Header (Cookie, Authorization, Accept) BLEIBEN: sie
        // gehoeren zur Anfrage, nicht zum Body, und ein Benutzer, der
        // angemeldet ist, soll es nach einer Umleitung immer noch sein.
        neu.headers
            .retain(|name, _| !matches!(name.as_str(), "content-type" | "content-length"));
    } else {
        neu.method = aktuelle.method;
        neu.body = aktuelle.body.clone();
    }
    // added 02.10.2026 14:6x: **DIE KENNUNG VOR DEM WEGGEBEN**, weil `ziel` ein
    // `String` ist und gleich in `basis` wandert; die Reihenfolge hier ist damit
    // nicht Kosmetik, sondern die Bedingung dafuer, dass der Compiler zulaesst.
    let ziel_ist_https = ziel.starts_with("https://");
    neu.basis = Some(ziel);
    // added 02.10.2026 15:1x: **UND DIE HERKUNFTSGRENZE**, und das ist die
    // Korrektur eines **zweiten** Lecks derselben Art (Nr. 36). GEMESSEN vor der
    // Aenderung: der Kopf der zweiten Anfrage — an einen **anderen Port**, also
    // eine andere Herkunft — trug `cookie: sitzung=…` **und** `authorization` mit.
    //
    // added 02.10.2026 15:1x: DIE NORM, RFC 9110 15.4, Schritt 3, woertlich:
    // „Consider removing header fields that were not automatically generated by
    // the implementation (i.e., those present in the request because they were
    // added by the calling context) where there are security implications; this
    // includes **but is not limited to Authorization and Cookie**.“ Schritt 5
    // nennt dieselben Felder als „resource-specific“.
    //
    // added 02.10.2026 15:1x: **UND INNERHALB DER HERKUNFT BLEIBT ALLES**, weil
    // die Norm „consider“ sagt und nicht „always“, und weil ein pauschales
    // Entfernen jede Anmeldung an irgendeiner Seite kaputtmacht. Der Test
    // `der_sprung_innerhalb_der_herkunft_behaelt_authorization_und_cookie` ist
    // die Gegenprobe: ohne sie waere ein Test wie dieser auch bei einer Regel
    // gruen, die jede Weiterleitung entwertet.
    //
    // added 02.10.2026 15:1x: **UND `Referer` STEHT NICHT IN DER LISTE**, weil er
    // nicht **entfernt**, sondern **neu berechnet** wird (Schritt 5 sagt
    // „aktualisieren“): `referer_setzen` sitzt eine Zeile weiter unten und wendet
    // die Richtlinie an. Ihn hier zu loeschen und dann wieder zu setzen waere
    // zweimal dieselbe Sache.
    // added 02.10.2026 15:3x: **UND ZUERST DAS SCHEMA, DANN DIE HERKUNFTSFRAGE**
    // — das ist eine Reihenfolge und keine Form. In meinem ersten Aufbau stand
    // diese Zeile **nach** der Grenzpruefung, und GEMESSEN wurde die Folge:
    // `ist_https(&neu)` war `false`, weil `neu.https` dort noch `None` war und
    // der Rueckfall `port == 443 || host.starts_with("https://")` bei einem
    // Host ohne Schema und einem Port != 443 **falsch** sagt. Das Ergebnis waere
    // gewesen, dass **jeder** Sprung die Herkunft verliert — und damit jede
    // Anmeldung und jedes `Cookie` bei jeder Weiterleitung verschwindet. Der
    // Gegenfall-Test hat es gefangen; ohne ihn waere der Leak-Test gruen
    // geblieben und der Fehler unentdeckt.
    neu.https = Some(ziel_ist_https);
    if !herkunft_bleibt(aktuelle, &neu) {
        for name in [
            "authorization",
            "proxy-authorization",
            "cookie",
            // added 04.10.2026 06:4x, R70. **`origin` IST DIE EINZIGE LÜCKE IN
            // DIESER LISTE**, und die Norm nennt es **wörtlich**. RFC 9110
            // (rfc-editor.org, abgerufen 04.10.2026) Abschnitt 15.4
            // „Redirection 3xx“, Schritt 2, Punkt 5:
            //
            //     5.  Resource-specific header fields, including (but not
            //         limited to) Referer, Origin, Authorization, and Cookie.
            //
            // Die drei anderen stehen seit 02.10.2026 15:1x in der Liste;
            // `Referer` steht **absichtlich** nicht drin, weil er **neu
            // berechnet** wird (`referer_setzen`), und `Origin` war schlicht
            // **vergessen** — es steht in keinem der beiden Tests, also hat es
            // niemand bemerkt.
            //
            // added 06:4x: **UND DIE BEIDEN GEGENBEWEISE**, gemessen je ein Lauf
            // (04.10.2026):
            //     `origin` aus der Liste     -> `…verliert_auch_origin` **rot**,
            //                                   die Gegenprobe bleibt gruen
            //     `origin` **pauschal** weg  -> `…behaelt_origin` **rot**
            // Ohne die zweite Probe waere ein „entferne alles“ genauso gruen wie
            // die Regel — R61s Lehre, hier mit zwei Tests statt einer Behauptung.
            //
            // added 06:4x: **WARUM DAS EIN LEERLAUF IST:** `Origin` beschreibt
            // die Herkunft, **von der** die Anfrage kommt. Nach einem Sprung auf
            // eine andere Herkunft ist der **alte** Wert nicht „veraltet“, er ist
            // **falsch** — und die CORS-Prüfung des Ziels wertet genau ihn aus
            // (WHATWG Fetch, „CORS check“, liest `Origin` aus der Anfrage). Ein
            // stehengebliebener `Origin` meldete dem Ziel eine Herkunft, die es
            // nie gesehen hat.
            //
            // added 06:4x: **UND DER PREIS, ehrlich:** ein Aufrufer, der `Origin`
            // **absichtlich** über einen **herkunftsfremden** Sprung mitgeben
            // will (etwa weil er selbst ein Gateway ist), verliert ihn jetzt.
            // Das ist die **Normrichtung** („consider removing … where there are
            // security implications“, Schritt 3), und ein Gateway kann den Kopf
            // am **Ziel** wieder setzen — dort ist es sein eigener Aufruf.
            //
            // added 06:7x, R70: **UND EIN FEHLER VON MIR, HIER GEMESSEN:** bei
            // der zweiten Runde (Gegenbeweis 2) habe ich die Zeile `"origin",`
            // **wieder entfernt** und dabei aus Unacht die **Liste** auf die
            // Kurzform `["authorization", …]` zurueckgesetzt — die Listeneintraege
            // samt Normzitat waren weg, **ohne** dass der Compiler etwas sagte.
            // Der Test `…verliert_auch_origin` blieb **rot** und zeigte den
            // Schaden woertlich: `origin: https://localhost:35425` stand im
            // zweiten Kopf. **Ein Test, der eine Zeile und eine Regel prueft,
            // faengt auch das Abschalten der Regel.**
            "origin",
        ] {
            neu.headers.remove(name);
        }
    }

    // added 02.10.2026 14:5x: **DAS SCHEMA BLEIBT**, und das ist die Korrektur
    // eines **Sicherheitsfehlers**, den der Ende-zu-Ende-Test gefunden hat
    // (`PROBLEME.txt` Nr. 35).
    //
    // added 02.10.2026 14:5x: DER WEG, und er ist eine Kette aus drei Stellen:
    // `url_teile` gibt `(host, port, pfad)` zurueck und **wirft das Schema weg**,
    // `Request::get` setzt deshalb `https: None`, und `ist_https` faellt in sei-
    // nem zweiten Zweig auf `port == 443 || host.starts_with("https://")` zurueck —
    // beides trifft fuer `url_teile`-Ausgaben nicht zu, weil der Host dort kein
    // Schema mehr hat. GEMESSEN: ein `302` auf **https** mit einem Port **nicht
    // 443** wurde daraufhin **im Klartext** abgerufen: die Spur in `verbinde`
    // meldete `tls=false` fuer die zweite Verbindung. Auf Port **443** faellt es
    // nicht auf, weil 443 dort zufaellig richtig liegt — und genau deshalb hat es
    // niemand gemerkt: alle oeffentlichen https-Hosts benutzen 443.
    //
    // added 02.10.2026 14:5x: **WAS ICH NICHT TUE:** `url_teile` umzuschreiben.
    // Es hat weitere Aufrufer, und eine Aenderung an der Rueckgabe waere eine
    // Aenderung an **drei** Orten fuer einen Fehler an einem. Der Fehler ist
    // nicht „`url_teile` verliert das Schema“, sondern „der Sprung verliert es“ —
    // und der Sprung ist genau die Stelle, an der es hingehoert.
    //
    // added 02.10.2026 14:6x: **UND DIE QUELLE IST `ziel`, nicht der alte
    // Aufbau**: `ziel` ist die **aufgeloeste** absolute URL (aus `loese_auf`),
    // sie traegt also das Schema der Quelle der Umleitung, nicht das der
    // Anfrage, von der aus sie kam. Ein Sprung von https auf http ergibt darum
    // `Some(false)` — und das ist der Fall, in dem die alte Regel aus
    // `host.starts_with("https://")` richtig gelegen haette.
    Ok(neu)
}

/// Die Faelle, die `umleitung_ziel` behandelt, sind in `loese_auf`
/// aufgeschrieben: `http://anderer/pfad`, `//anderer/pfad`, `/pfad`,
/// `neu`, `../neu`, `./neu`, `?query`, `#fragment` und leer.
///
/// Loest `referenz` gegen `basis` auf — die Fassung fuer Umleitungen.
///
/// Oeffentlich, weil die Aufgabe ausdruecklich eine ORIGINAL-Fassung der
/// Regel verlangt, die man einzeln nachlesen kann, und weil der Vergleich
/// mit `narsil-page::loese_link` (fremde Crate) sonst nur ueber Testdaten
/// laeuft — siehe `tests/vs_narsil_page.rs`.
///
/// Fehler: leere Basis bei relativer Referenz. GEMESSEN 29.09.2026: die
/// erste Fassung gab dafuer `http:///b` zurueck, also einen Hostnamen
/// ohne Namen. Der Client haette es gemerkt — aber spaet und mit einer
/// Meldung ueber DNS statt ueber die leere Basis.
pub fn loese_auf(basis: &str, referenz: &str) -> Result<String, NetError> {
    // 1. Absolute URI: sie gewinnt vollstaendig, die Basis wird ignoriert.
    if referenz.contains("://") {
        return Ok(referenz.to_string());
    }
    // 2. `mailto:` / `tel:` / `about:` — ein Schema, kein Netz. Ohne das
    //    wuerde aus `mailto:a@b.de` der Pfad `/mailto:a@b.de` mit dem
    //    Schema der Basis, und der Client schickt eine HTTP-Anfrage an
    //    `b.de`.
    if !referenz.contains("://")
        && referenz
            .find(':')
            .is_some_and(|i| i > 0 && referenz[..i].chars().all(|c| c.is_ascii_alphanumeric()))
    {
        return Ok(referenz.to_string());
    }
    if basis.trim().is_empty() {
        return Err(NetError::InvalidResponse(format!(
            "Umleitung {referenz:?} ohne Basis zum Aufloesen"
        )));
    }
    let (schema, rest) = match basis.find("://") {
        Some(i) => (&basis[..i], &basis[i + 3..]),
        None => ("http", basis),
    };

    // 3. Netzteil-Referenz `//host/pfad`: Schema bleibt, Host wechselt.
    if let Some(ohne_slash) = referenz.strip_prefix("//") {
        let (host_teil, pfad) = match ohne_slash.find('/') {
            Some(i) => (&ohne_slash[..i], &ohne_slash[i..]),
            None => (ohne_slash, ""),
        };
        let ziel_pfad = if pfad.is_empty() { "/" } else { pfad };
        return Ok(format!("{schema}://{host_teil}{ziel_pfad}"));
    }

    // 4. Alles, was ab dem ersten `?` oder `#` steht, ist Query bzw.
    //    Fragment — der Pfad davor kann leer sein.
    let (referenz_pfad, rest_ab) = match referenz.find(['?', '#']) {
        Some(i) => (&referenz[..i], &referenz[i..]),
        None => (referenz, ""),
    };

    // 5. Basis in Autoritaet und Pfad zerlegen.
    let (basis_autoritaet, basis_pfad) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };

    // 6. Pfad zusammensetzen (RFC 3986 §5.2.3 + 5.2.4).
    let neuer_pfad = if referenz_pfad.is_empty() {
        // `?query` und `#fragment` ohne Pfad: Pfad der Basis bleibt.
        basis_pfad.to_string()
    } else if referenz_pfad.starts_with('/') {
        // `/pfad`: absolute Pfadreferenz, Basisverzeichnis faellt weg.
        referenz_pfad.to_string()
    } else {
        // Relativ: Verzeichnis der Basis + `..`/`.` aufloesen.
        let verzeichnis = match basis_pfad.rfind('/') {
            Some(i) => &basis_pfad[..=i],
            None => "/",
        };
        normalisiere(&format!("{verzeichnis}{referenz_pfad}"))
    };
    Ok(format!(
        "{schema}://{basis_autoritaet}{neuer_pfad}{rest_ab}"
    ))
}

/// Macht `.` und `..` zu nichts — ohne Dateisystem, rein im Text.
///
/// `..` ueber der Wurzel wird verworfen (`/../a` = `/a`), weil ein
/// Pfad nicht aus dem Server-Root hinauszeigen kann.
///
/// Zum abschliessenden Punktesegment (RFC 3986 §5.2.4 Schritt 2E): ein
/// `/a/b/.` wird zu `/a/b/`, nicht zu `/a/b`. Sonst waere ein
/// Verzeichnis-Link (`href="."`) ein Datei-Link, und der Server
/// antwortet 404.
fn normalisiere(pfad: &str) -> String {
    let mit_slash = pfad.starts_with('/');
    let endet_auf_slash =
        (pfad.ends_with('/') && pfad.len() > 1) || pfad.ends_with("/.") || pfad.ends_with("/..");
    let mut teile: Vec<&str> = Vec::new();
    for teil in pfad.split('/') {
        match teil {
            "" | "." => {}
            ".." => {
                teile.pop();
            }
            t => teile.push(t),
        }
    }
    let mut raus = String::new();
    if mit_slash {
        raus.push('/');
    }
    raus.push_str(&teile.join("/"));
    if raus.is_empty() {
        raus.push('/');
    }
    if endet_auf_slash && !raus.ends_with('/') {
        raus.push('/');
    }
    raus
}

// -----------------------------------------------------------------------
// Geteilte Regel: narsil-net loest Umleitungen auf, narsil-url loest Links
// auf. ZWEI Fassungen derselben Regel sind ein Fehler, den man nur beim
// Lesen sieht — aber eine Abhaengigkeit von narsil-url auf narsil-net waere
// eine Richtung, die kein ADR festlegt, und R2/ADR-030 verbieten das
// Aendern fremder Manifeste.
//
// Also: getrennte Umsetzung, und der Beweis der Gleichheit steht in
// crates/narsil-net/tests/aufgeloest.rs (`faelle_sind_in_beiden_craten_gleich`).
// Dort stehen die erwarteten Werte als Tabelle, und beide Crates werden
// gegen dieselbe Tabelle gemessen.
// -----------------------------------------------------------------------

/// Welche Methode hat der naechste Request nach einer Umleitung mit
/// diesem Statuscode? (RFC 9110 §15.4)
///
/// GEMESSENE ENTSCHEIDUNG, 29.09.2026, 22:55:
/// * **303 -> GET**, auch von POST. RFC 9110 §15.4.3: "the user agent
///   ... performs a retrieval request ... using the GET method (or HEAD
///   if the original request used HEAD)". Ohne das wiederholt der
///   Browser die Aktion — beim Kommentieren heisst das: der Kommentar
///   steht zweimal, beim Bezahlen wird zweimal abgebucht.
/// * **301, 302 -> POST bleibt POST.** §15.4.2 sagt "a user agent SHOULD
///   NOT automatically redirect the request method". 301/302 kommen von
///   Servern, die es nicht anders wissen; schaltet der Browser auf GET,
///   faellt der Body weg und das Formular tut still nichts.
/// * **307, 308 -> unveraendert.** §15.4.9/§15.4.10 verlangen es
///   ausdruecklich. Wer dort umschaltet, verwirft beim Neuladen ein
///   halb ausgefuelltes Formular.
/// * **HEAD bleibt HEAD**, auch bei 303 — `Request::head` waere hier
///   falsch, weil es den Host mit neu setzt.
///
/// Oeffentlich, weil das eine ENTSCHEIDUNG ist, die man nachlesen und
/// pruefen muss, und nicht eine Zeile, die man glauben muss.
pub fn naechste_methode(alt: Method, code: u16) -> Method {
    match code {
        // 303: GET — ausser es war HEAD, dann bleibt HEAD.
        303 if alt != Method::Head => Method::Get,
        _ => alt,
    }
}

/// Zerlegt eine URL in Host, Port und Pfad fuer den naechsten Sprung.
///
/// Der Host kommt OHNE Klammern zurueck (`::1`), weil [`Request::host`]
/// den Hostnamen meint. Die Klammern setzt `Request::host_mit_port`
/// wieder davor, wo das Protokoll sie braucht.
fn url_teile(url: &str) -> (String, u16, String) {
    // added 30.09.2026: das Schema entscheidet ueber den Standardport.
    //
    // ECHTER FEHLER, gefunden beim Verdrahten von HTTPS. `url_teile`
    // hat `://` abgeschnitten und danach IMMER 80 als Standardport
    // gesetzt — auch fuer `https://`. Eine Umleitung von `http://` nach
    // `https://` kam damit als http-Anfrage auf Port 80 an, und der
    // Auftrag nennt genau diese Umleitung.
    //
    // GEMESSEN an `https://example.com/`: vorher `(host, 80, "/")`, und
    // `sende` haette daraus `GET / HTTP/1.1` an Port 80 gemacht.
    //
    // Der Standardport ist alles, was `narsil-net` braucht: `ist_https`
    // entscheidet ueber den Port, und ein expliziter Port im URL
    // gewinnt wie bisher.
    let https = url.starts_with("https://");
    let rest = match url.find("://") {
        Some(i) => &url[i + 3..],
        None => url,
    };
    let standard_port: u16 = if https { 443 } else { 80 };
    let (host_teil, pfad) = match rest.find(['/', '?', '#']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let pfad = if pfad.is_empty() { "/" } else { pfad };
    // `user@host` wird nicht unterstuetzt (narsil-url sagt das auch) — hier
    // reicht der Host-Teil, ein `@` wird wie ein Portzeichen behandelt.
    //
    // IPv6 braucht VOR der Portsuche die Klammern: bei `[::1]:8080` zeigt
    // der LETZTE Doppelpunkt auf den Port, das stimmt — aber nur, weil
    // die Klammern davor stehen. Ohne sie waere `[::1]` der Host mit dem
    // Port `1]` und `parse` wuerde still 80 liefern. Deshalb der Weg
    // ueber `zerlege_host_mit_klammern`: der kaputte Fall wird ein
    // benannter Fehler statt einer falschen Adresse.
    let (host, port) = match zerlege_host_mit_klammern(host_teil) {
        Ok((h, p)) => (h, p.unwrap_or(standard_port)),
        Err(_) => match host_teil.rfind(':') {
            Some(i) => {
                let p = host_teil[i + 1..].parse().unwrap_or(standard_port);
                (host_teil[..i].to_string(), p)
            }
            None => (host_teil.to_string(), standard_port),
        },
    };
    (host, port, pfad.to_string())
}

// -----------------------------------------------------------------------
// Tests — ohne Netz: nur Serialisierung und Aufloesung.
// -----------------------------------------------------------------------

/// DIE VORGABEFELDER, und sie sind aus einem **gemessenen** Fehler
/// entstanden.
///
/// added 01.10.2026 16:1x, R4z8. GEMESSEN: `www.wikipedia.org` antwortete
/// ueber HTTP/1.1 mit **200** (93 955 Byte) und ueber h2 mit **403**, und
/// der Koerper der 403 nennt den Grund woertlich: „Please set a
/// user-agent and respect our robot policy". Unabhaengig gegengeprueft
/// (Python, `h2`/`hpack`): dieselbe Anfrage ohne `user-agent` ergibt 403,
/// mit `narsil/0.1` ergibt 200.
///
/// DIE BEHAUPTUNGEN, und es sind drei, weil drei Regeln daran haengen:
///  1. ohne eigenes `user-agent` kommt `user-agent: Narsil/0.1`,
///  2. ohne eigenes `accept` kommt das `accept` der Ressourcenart,
///  3. ein vom Aufrufer gesetztes Feld **gewinnt** — beides.
///
/// Dazu die vierte, negative: `host` und die `hop_by_hop`-Felder kommen
/// nicht mit, weil sie ueber h2 verboten sind (RFC 9113 8.2.2).
#[test]
fn die_vorgabefelder_der_anfrage_sind_je_weig_protocol_gleich() {
    let leer = Request::get("https://beispiel.test", 443, "/");
    let felder = felder_mit_vorgaben(&leer);
    let hole = |n: &str| felder.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_str());
    assert_eq!(
        hole("user-agent"),
        Some(STANDARD_USER_AGENT),
        "ohne eigenes Feld kommt die Vorgabe — GEMESSEN fehlte sie und \
         www.wikipedia.org antwortete 403"
    );
    assert!(
        hole("accept").is_some(),
        "ohne eigenes `accept` kommt eines nach Ressourcenart"
    );
    assert_eq!(
        hole("host"),
        None,
        "`host` ist ueber h2 verboten — es steht als `:authority` (8.2.2)"
    );
    assert_eq!(
        hole("connection"),
        None,
        "`connection` ist Hop-by-Hop und gehoert ueber h2 nicht in die \
         Kopfzeilen (RFC 9113 8.2.2)"
    );

    // DIE GEWINNREGEL, und sie ist der Grund fuer die ganze Funktion: ein
    // Aufrufer, der eine Seite als genau diesen Typ will, muss das sagen
    // duerfen (R0).
    let mut eigen = Request::get("https://beispiel.test", 443, "/");
    eigen = eigen.header("User-Agent", "MeinBot/2");
    let felder = felder_mit_vorgaben(&eigen);
    assert_eq!(
        felder
            .iter()
            .find(|(k, _)| k == "user-agent")
            .map(|(_, v)| v.as_str()),
        Some("MeinBot/2"),
        "der Aufrufer gewinnt — `.header()` ist die Aussage des Aufrufers"
    );
    assert_eq!(
        felder.iter().filter(|(k, _)| k == "user-agent").count(),
        1,
        "und es steht **ein** `user-agent` drin, nicht zwei"
    );
    // added 01.10.2026 16:1x: die Schluessel sind **kleingeschrieben**,
    // weil `Request::header` den Namen unveraendert annimmt und ein
    // Aufrufer „User-Agent" schreibt. GEMESSEN waere sonst `User-Agent`
    // und `user-agent` zweimal in der Anfrage.
    assert!(
        felder.iter().all(|(k, _)| k == &k.to_ascii_lowercase()),
        "alle Feldnamen klein — HPACK verlangt das (RFC 7541 4.2)"
    );
}

/// M3-Gegenprobe: die Felderzahl der Anfrage ist begrenzt.
///
/// added 01.10.2026 16:1x, R4z8. Ohne Grenze baut ein Aufrufer mit 100 000
/// Feldern einen `HEADERS`-Ramen von mehreren MB; der Server antwortet mit
/// `431` oder gar nicht.
#[test]
fn eine_anfrage_hat_nicht_unbegrenzt_viele_felder() {
    let mut r = Request::get("https://beispiel.test", 443, "/");
    for i in 0..MAX_ANFRAGE_FELDER + 50 {
        r = r.header("x-test", &i.to_string());
    }
    let felder = felder_mit_vorgaben(&r);
    assert!(
        felder.len() <= MAX_ANFRAGE_FELDER + 2,
        "hoechstens {MAX_ANFRAGE_FELDER} Felder plus zwei Vorgaben, war {}",
        felder.len()
    );
}

/// added 03.10.2026 16:5x, R60. **DIE CRL-BESCHAFFUNG** — und sie liegt hier,
/// **nicht** in `narsil-tls`, aus **einem** Grund: sie macht HTTP.
///
/// added 03.10.2026 16:5x: **DIE DREI SCHRITTE, und jeder hat eine Frage, die
/// vorher beantwortet sein muss.**
///  1. **Welche URL?** Aus der Kette: `x509::verteilungsstellen_als_texte`
///     (R59) — und **nur** aus dem **Aussteller**, denn die CRL des Blatts zu
///     holen ist sinnlos (die CRL eines Blatts gibt es nicht).
///  2. **Ist sie erreichbar?** GEMESSEN (16:0x, `curl`): 9 von 10 geprüften
///     echten Zertifikaten tragen eine CRL-Distribution-Point-URL, und die
///     antworten mit **HTTP 200** (186 kB und 332 kB). Die Beschaffung ist also
///     **lebendig**, nicht theoretisch.
///  3. **Ist die Antwort brauchbar?** `crl::beurteile` entscheidet, nicht wir.
///
/// added 03.10.2026 16:5x: **WARUM DAS NICHT IM HANDSHAKE.** Ein HTTP-Abruf im
/// Handshake heisst: der Client wartet, und ein Angreifer, der den CRL-Server
/// lahmlegt, legt damit **jeden** Handshake lahm. Der Aufruf gehoert **nach**
/// dem Handshake, und sein Ergebnis darf den Handshake **nicht** umkehren —
/// sondern nur **melden**.
///
/// added 03.10.2026 16:6x: **DIE ZEITGRENZE, und sie ist keine erfundene Zahl.**
/// `Timeout` auf dem Sockel: **10 Sekunden** — GEMESSEN an den realen CRLs (16:0x)
/// waren die Antworten nach **unter zwei Sekunden** vollständig; 10 Sekunden lassen
/// auch ein langsames CDN noch zu und begrenzen den Wartezeit-Schaden.
pub const CRL_ZEITGRENZE: std::time::Duration = std::time::Duration::from_secs(10);

/// added 03.10.2026 16:6x: **DER ERGEBNIS-TYP**, und er ist bewusst **klein**:
/// drei Zustände, keine Feinunterscheidung. Der Grund ist derselbe wie bei
/// `ocsp_urteil` (R50): eine Feinunterscheidung braucht einen Aufrufer, der sie
/// auswertet, und der gibt es nicht.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrlUrteil {
    /// added 03.10.2026 16:6x: **KEIN VERTEILUNGSPUNKT** im Ausstellerzertifikat
    /// — das ist der Normalfall, kein Fehler.
    KeineQuelle,
    /// added 03.10.2026 16:7x: **ABGEFRAGT, ABER NICHT BRAUCHBAR**: kein HTTP,
    /// kein 200, kein Parse, falscher Aussteller, abgelaufen. Der Grund steht in
    /// `CrlUrteil` **nicht** — eine Meldung hier waere eine Meldung ohne
    /// Empfänger, und `narsil-net` hat keinen Diagnosekanal nach aussen. Was bleibt,
    /// ist `stderr`, und das sagt der Aufrufer nicht dem Benutzer.
    Unbrauchbar,
    /// added 03.10.2026 16:7x: **GEPRUEFT**: die CRL stammt vom Aussteller des
    /// Blatts, ist gültig, und das Blatt steht **nicht** auf der Widerrufsliste.
    GeprueftNichtWiderrufen,
    /// added 03.10.2026 16:7x: **WIDERRUFEN** — und das ist der einzige Zustand,
    /// der eine **Handlung** verlangt.
    Widerrufen,
}

/// added 03.10.2026 16:8x: **DIE HAUPTFUNKTION**, und ihre Signatur sagt die
/// ganze Architektur: sie nimmt die **Kette** (DER-Bytes, aus
/// `TlsVerbindung::kette()`) und gibt **ein Urteil** zurueck — **kein `Result`**.
///
/// added 03.10.2026 16:8x: **WARUM KEIN `Result`.** Weil die Funktion **nicht
/// scheitern darf**: der Aufrufer ist ein HTTP-Client, der eine Seite laden
/// will, und ein fehlgeschlagener Widerrufsstatus darf ihn nicht **am Laden
/// hindern**. Das entspricht der Entscheidung bei `pathLenConstraint` (R53) und
/// beim `Content-Length` (R42) aus **umgekehrter Richtung**: dort haben wir nicht
/// durchgesetzt, weil Durchsetzen Ausfallschaden waere — hier erzwingen wir
/// **nicht**, weil das Wissen um den Status den Client nicht **am Laden hindern**
/// darf. Der Unterschied ist die Richtung: hier waere die Alternative „Seite nicht
/// laden“, also ein **Ausfallschaden**.
pub fn crl_status_pruefen(kette_der: &[Vec<u8>]) -> CrlUrteil {
    // added 03.10.2026 16:8x: **ZU WENIG ZERTIFIKATE**, und das ist kein Fehler
    // des Aufrufers: ohne Aussteller kann die CRL-Signatur nicht geprueft
    // werden. GEMESSEN: das ist der Fall bei einem Server, der nur sein Blatt
    // schickt — was nach RFC 8446 4.4.2 erlaubt ist, wenn die Kette aus dem
    // Speicher kommt.
    if kette_der.len() < 2 {
        return CrlUrteil::KeineQuelle;
    }
    let blatt = match narsil_tls::x509::zertifikat_lesen(&kette_der[0]) {
        Ok(z) => z,
        Err(_) => return CrlUrteil::Unbrauchbar,
    };
    let aussteller = match narsil_tls::x509::zertifikat_lesen(&kette_der[1]) {
        Ok(z) => z,
        Err(_) => return CrlUrteil::Unbrauchbar,
    };
    // added 03.10.2026 16:d6: **MEINE ANNAHME WAR FALSCH, UND DIE MESSUNG
    // HAT ES GEZEIGT.** Ich hatte geschrieben „nur der Aussteller hat eine Quelle
    // — die CRL eines Blatts gibt es nicht“. GEMESSEN (16:d6) an vier echten
    // Hosts mit `openssl x509 -text`: `www.debian.org` traegt die
    // CRL-Distribution-Point-**Erweiterung selbst**, und sein Aussteller ist
    // Let's Encrypts `YR1`. Der Verteilungspunkt steht also **sehr wohl** im
    // Blatt — und der Aussteller traegt oft **keinen**.
    //
    // added 03.10.2026 16:d7: **UND DAS IST RICHTIG SO**, und hier ist die
    // Begruendung: RFC 5280 4.2.1.13 sagt, der Verteilungspunkt zeige, wo die
    // **CRL des Ausstellers** zu finden ist — und ein Blatt darf sie **nennen**,
    // damit der Client sie findet, ohne die CA selbst laden zu muessen. Wir
    // brauchen also die Quelle aus **beiden** Zertifikaten, und die **Reihenfolge**
    // ist gemessen: erst das **Blatt** (das sie haeufig traegt), dann der
    // Aussteller. Das ist keine Kuerzung, sondern die Reihenfolge, in der die
    // Wild sie liefert.
    let mut urls = narsil_tls::x509::verteilungsstellen_als_texte(&blatt);
    for u in narsil_tls::x509::verteilungsstellen_als_texte(&aussteller) {
        if !urls.contains(&u) {
            urls.push(u);
        }
    }
    if urls.is_empty() {
        return CrlUrteil::KeineQuelle;
    }
    // added 03.10.2026 16:9x: **NUR DIE ERSTE URL**, und die Begruendung steht
    // hier, weil sie eine Entscheidung ist: die Verteilungspunkte koennen
    // **mehrere** sein, und alle abzulaufen waere bei drei CA-Knoten drei
    // HTTP-Anfragen pro Handshake. **Eine** Anfrage, und wenn sie nichts bringt,
    // melden wir `Unbrauchbar` — das ist ehrlich und es begrenzt den Schaden auf
    // **eine** Anfrage.
    // added 03.10.2026 16:b0: **ALLE QUELLEN DER REIHE NACH, NICHT NUR DIE
    // ERSTE** — und der Grund ist eine Messung, keine Vermutung.
    //
    // added 03.10.2026 16:b1: **GEMESSEN an `xkcd.com`:** Blatt und Aussteller
    // tragen je **einen** Verteilungspunkt, und die **beiden Listen gehoeren zu
    // verschiedenen CAs** — die des Blatts ist von der Zwischen-CA (`GlobalSign
    // Atlas R3 DV TLS CA 2025 Q4`), die des Ausstellers von der Wurzel
    // (`GlobalSign Root CA - R3`, `openssl crl -issuer`). Nimmt man die erste,
    // ist man richtig; nimmt man die zweite, erhaelt man `Unbrauchbar`. Die
    // Reihenfolge ist damit **nicht** Kosmetik, sondern die Frage, ob man die
    // richtige CA erwischt.
    //
    // added 03.10.2026 16:b7: **UND DER LETZTE GRUND STEHT VOR ALLEM** — der
    // Fehler muss sich **lohnen**, bevor er behebbar gemacht wird. Die Schleife
    // ist billig zu schreiben und macht aus einem Abruf bis zu drei; der Nutzen
    // ist die Zertifikatskette mit einem Aussteller, der **zwei** Verteilungs-
    // punkte nennt, von denen der erste nicht passt. Solche Ketten gibt es
    // (Cross-Zertifizierung), und GEMESSEN ist hier nur die Wildzahl *0* fuer
    // „beide brauchbar“ — der Preis ist also bekannt und klein, der Nutzen
    // unbelegt. Deshalb ist die Schranke [`MAX_CRL_QUELLEN`] auf drei gesetzt:
    // genug fuer den belegten Fall, gedeckelt gegen den erfundenen.
    let jetzt = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut letzte_aussage = CrlUrteil::Unbrauchbar;
    for url in urls.iter().take(MAX_CRL_QUELLEN) {
        let bytes = match hole_crl(url) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("CRL: {url}: {e}");
                letzte_aussage = CrlUrteil::Unbrauchbar;
                continue;
            }
        };
        // added 03.10.2026 16:b3: **DER ENTSCHEIDER IST `crl::beurteile`, NICHT
        // WIR** — das ist der ganze Punkt der Schichtung: Signaturpruefung,
        // Gueltigkeit und Seriennummer macht die Bibliothek, die das Format
        // kennt.
        //
        // added 03.10.2026 16:b4: **`beurteile` GIBT EINEN EIGENEN TYP ZURUECK**,
        // nicht `Result` — GEMESSEN: `pub fn beurteile(...) -> Urteil` mit drei
        // Fuellen: `NichtWiderrufen`, `Widerrufen(Box<Widerruf>)`,
        // `Unbrauchbar(CrlFehler)`. Mein erster Entwurf behandelte es als
        // `Result<(), CrlFehler>`; das war geraten.
        match narsil_tls::crl::beurteile(&bytes, &blatt, &aussteller, jetzt) {
            narsil_tls::crl::Urteil::NichtWiderrufen => {
                return CrlUrteil::GeprueftNichtWiderrufen;
            }
            // added 03.10.2026 16:b5: **DER WIDERRUF BRICHT AB, ER WIRD NICHT
            // WEITERGESUCHT.** Das ist eine **Entscheidung**: eine brauchbare
            // Liste, die das Blatt nennt, ist eine Aussage; eine zweite Liste,
            // die es nicht nennt, koennte von einer anderen CA sein und wuerde
            // die erste nicht widerlegen. Der erste belegte Widerruf gewinnt.
            narsil_tls::crl::Urteil::Widerrufen(_) => {
                eprintln!("CRL: das Blatt steht auf der Widerrufsliste ({url})");
                return CrlUrteil::Widerrufen;
            }
            narsil_tls::crl::Urteil::Unbrauchbar(e) => {
                // added 03.10.2026 16:b6: **DER GRUND STEHT IM WERT, NICHT IM
                // TYP.** Fuer den Aufrufer heisst `Unbrauchbar` dasselbe wie
                // vorher, aber die Ursache geht nach `stderr`: `narsil-net` hat
                // keinen Diagnosekanal nach aussen, und die Unterscheidung
                // („falscher Aussteller“ gegen „abgelaufen“) gehoert **in die
                // Bibliothek`, nicht in eine Meldung, die niemand liest.
                eprintln!("CRL: {url}: {e}");
                letzte_aussage = CrlUrteil::Unbrauchbar;
            }
        }
    }
    letzte_aussage
}

/// added 03.10.2026 16:b2: **WIE VIELE QUELLEN WIR ABFRAGEN**, benannt und
/// begruendet: eine Kette aus einem Angreifer-Server kann beliebig viele
/// Verteilungspunkte nennen, und jeder kostet **einen** HTTP-Abruf mit
/// Zeitlimit von [`CRL_ZEITGRENZE`]. Drei sind mehr als die gemessene Wild
/// braucht (Blatt und Aussteller tragen je einen, 16:b1), und der Schaden
/// durch eine Kette mit hundert Punkten ist gedeckelt: drei Abrufe, nicht
/// hundert.
pub const MAX_CRL_QUELLEN: usize = 3;

/// added 03.10.2026 16:9x, R60. **EIN HTTP/1.0-GET AUF EINE CRL-URL**, und es ist
/// bewusst **handgeschrieben** statt ueber den eigenen HTTP-Client: der eigene
/// Client (`hole_antwort`) macht **HTTPS** und spricht **H2** — fuer eine CRL im
/// Klartext ueber HTTP braucht es beides nicht, und ein CRL-Abruf darf **nicht**
/// vom TLS-Handshake abhaengen, den er eigentlich pruefen soll.
///
/// added 03.10.2026 16:9x: **HTTP/1.0, NICHT 1.1**, und der Grund ist eine
/// Sparmassnahme mit Folgen: 1.0 **ohne** `Host`-Zeile braucht keinen DNS-Namen
/// aus der URL, und **keinen** `Connection`-Header. Das ist die kleinste Anfrage,
/// die ein Server beantworten kann — und sie ist **kaputt**, wenn der Server
/// 1.1 verlangt, was kein CRL-Server tut (GEMESSEN: die beiden echten CRLs in
/// 16:0x antworteten auf eine solche Anfrage mit 200 und korrektem `Content-Length`).
pub fn hole_crl(url: &str) -> Result<Vec<u8>, String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| "nur http:// wird getragen".to_string())?;
    let (host_port, pfad) = match rest.find('/') {
        Some(i) => (rest[..i].to_string(), rest[i..].to_string()),
        None => (rest.to_string(), "/".to_string()),
    };
    // added 03.10.2026 16:ax: **DER PORT TRENNT HOST UND PORT** — und das ist
    // **zwei** Regeln, und die zweite hat mich eine Messung gekostet.
    //
    // added 03.10.2026 16:cx: **`(host, port)` ALS TUPPEL**, weil
    // `to_socket_addrs` auf einem **`&str` ohne Port** mit **„invalid socket
    // address“** fehlschlaegt — GEMESSEN (16:ca) an beiden echten URLs. Die
    // Meldung ist irrefuehrend: sie spricht von der **Adresse** und meint den
    // **Port**. `TcpStream::connect` nimmt zwar `(host, port)`, `to_socket_addrs`
    // nicht.
    //
    // added 03.10.2026 16:cy: **UND EIN EXPLIZITER PORT IM NAMEN** wird
    // abgetrennt, weil URLs wie `http://crl.example.invalid:8080/a.der` vorkommen:
    // `to_socket_addrs` wuerde sonst den Host **mit** „:8080“ suchen.
    let (nur_host, port_text) = match host_port.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => {
            (h.to_string(), p.to_string())
        }
        _ => (host_port.clone(), "80".to_string()),
    };
    let port: u16 = port_text.parse().map_err(|_| format!("Port {port_text}"))?;
    let socket_addrs: Vec<std::net::SocketAddr> = (nur_host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("Adresse {nur_host}: {e}"))?
        .collect();
    if socket_addrs.is_empty() {
        return Err(format!("keine Adresse fuer {host_port}"));
    }
    // added 03.10.2026 16:cb: **ALLE ADDRESSE DURCHPROBIEREN, UND ZUERST DIE
    // IPv4** — und das ist eine **Messung**, keine Vermutung.
    //
    // added 03.10.2026 16:cb: GEMESSEN (16:ca) an `yr1.c.lencr.org`: DNS liefert
    // **zuerst IPv6** (`[2606:4700::6812:14d5]:80`), und **IPv6 ist in diesem
    // Netz nicht geroutet** — der erste Versuch schlug mit „invalid socket
    // address“ fehl, obwohl `curl` dieselbe URL **HTTP 200** lieferte. Mein
    // `connect_timeout(&socket_addrs[0], …)` nahm also den **ersten** Eintrag und
    // gab auf.
    //
    // added 03.10.2026 16:cc: **DIE REGEL, und sie ist die von happy eyeballs in
    // einer Zeile:** alle Adressen **nacheinander** versuchen, **IPv4 zuerst**.
    // Nicht IPv4 verlangen — manche Hosts sind **nur** IPv6 erreichbar (der
    // Ordner `nc_ip_wirkt.rs` in dieser Crate besteht genau davon), also **beide**.
    let mut letzte: Option<std::io::Error> = None;
    let mut verbunden: Option<std::net::TcpStream> = None;
    let mut sortiert: Vec<&std::net::SocketAddr> = socket_addrs.iter().collect();
    sortiert.sort_by_key(|a| !a.is_ipv4());
    for addr in sortiert {
        match std::net::TcpStream::connect_timeout(addr, CRL_ZEITGRENZE) {
            Ok(s) => {
                verbunden = Some(s);
                break;
            }
            Err(e) => letzte = Some(e),
        }
    }
    let socket = verbunden.ok_or_else(|| {
        format!(
            "Verbindung {host_port}: {}",
            letzte
                .map(|e| e.to_string())
                .unwrap_or_else(|| "keine Adresse".to_string())
        )
    })?;
    let _ = socket.set_read_timeout(Some(CRL_ZEITGRENZE));
    let _ = socket.set_write_timeout(Some(CRL_ZEITGRENZE));
    let mut schreiber = &socket;
    // added 03.10.2026 16:cz: **`Host`-HEADER, HTTP/1.1 — UND BEIDES IST
    // GEMESSEN, NICHT GERATEN.**
    //
    // added 03.10.2026 16:cz: **DIE MESSUNG, und sie hat meinen ganzen Entwurf
    // widerlegt.** Ich hatte HTTP/1.0 **ohne** `Host` geschrieben („die kleinste
    // Anfrage, die ein Server beantworten kann“) — und `curl` **mit derselben
    // Option** (`--http1.0`, `-H "Host:"`) bekam bei **beiden** echten URLs ein
    // **403 forbidden**. Mit `--http1.1` bekommt `curl` **200**. Also verlangen
    // **beide** Server den `Host`-Header, und **HTTP/1.0 scheitert daran**.
    //
    // added 03.10.2026 16:d0: **DIE LEHRE, und sie ist die dritte dieser Art in
    // dieser Runde:** „die kleinste Anfrage“ ist **keine** vernünftige Grundlage
    // für ein Protokoll, das man **nicht** selbst geschrieben hat. HTTP/1.0 ist
    // 1991 normiert worden, und was 1991 sparsam war, ist heute **veraltet** —
    // R53 (pathLen) und R56 (Name Constraints) haben dieselbe Form gezeigt: eine
    // **historische** Norm gegen die **gemessene** Wild, und die Wild sagt
    // „nein“.
    //
    // added 03.10.2026 16:d1: **UND `Connection: close`, weil wir HTTP/1.1
    // schicken:** ohne den Header wuerde der Server die Verbindung **offen**
    // halten, und `read_to_end` haette dann **bis zum Zeitlimit** gewartet.
    // GEMESSEN: die beiden Antworten kamen in unter zwei Sekunden (R60 16:0x),
    // aber „unter zwei“ ist kein Versprechen für **jeden** Server.
    schreiber
        .write_all(
            format!("GET {pfad} HTTP/1.1\r\nHost: {nur_host}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .map_err(|e| format!("Schreiben: {e}"))?;
    let mut alles: Vec<u8> = Vec::with_capacity(64 * 1024);
    {
        let mut leser = &socket;
        leser
            .read_to_end(&mut alles)
            .map_err(|e| format!("Lesen: {e}"))?;
    }
    // added 03.10.2026 16:ay: **DER KOPF BIS ZUR LEERZEILE**, weil die DER **nicht
    // selbst** bei Byte 0 beginnt: sonst wuerde `beurteile` den Kopf als Zertifikat
    // lesen und scheitern. Und gesucht wird auf **vier Bytes** — die
    // Leerzeichenfolge Backslash-r, Backslash-n, zweimal — weil HTTP/1.0 die
    // Leerzeile mit CRLF schreibt.
    // die Leerzeile mit CRLF schreibt.
    const MAX_KOPF: usize = 8 * 1024;
    let kopf_end = alles
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "kein Kopfende in der Antwort".to_string())?;
    if kopf_end > MAX_KOPF {
        return Err(format!("Kopf ist {kopf_end} Byte, erlaubt {MAX_KOPF}"));
    }
    let kopf = String::from_utf8_lossy(&alles[..kopf_end]).to_ascii_lowercase();
    if !kopf.starts_with("http/1.") {
        return Err(format!(
            "keine HTTP-Antwort: {}",
            &kopf[..kopf.len().min(40)]
        ));
    }
    // added 03.10.2026 16:ay: **`200` MIT **`200 ` am ANFANG** — nicht „enthält
    // 200“, denn `Content-Length: 1200` enthaelt auch „200“. Das ist die Form, die
    // R51 beim `pathLen`-Zaehler geritten hat (dort waren es Hex-Zeichen, hier
    // Ziffern im Text).
    let zeile = kopf.lines().next().unwrap_or("");
    if !zeile.ends_with(" 200 ok") {
        return Err(format!("Antwortzeile: {zeile}"));
    }
    Ok(alles[kopf_end + 4..].to_vec())
}

#[cfg(test)]
mod widerruf_am_client {
    use super::{CrlUrteil, Limits, Request, Widerrufspolitik, fetch_mit_speicher};

    /// added 03.10.2026 18:8x, R61; **ERSETZT 03.10.2026 19:az, R62. EIN TEST
    /// STATT ZWEI, UND DER GRUND IST EIN FEHLER, DEN R62 GEFUNDEN HAT.**
    ///
    /// added 03.10.2026 19:az, R62: **R61 HAT ZWEI NETZTESTS GESCHRIEBEN, DIE
    /// DENSELBEN GLOBALEN ZÄHLER MESSEN** — `der_client_weg_prueft_den_widerruf`
    /// und `ohne_politik_kein_abruf_von_crl`. GEMESSEN beim Lauf mit beiden:
    /// `ohne_politik_kein_abruf_von_crl` meldete
    /// „ohne Policy wurde trotzdem eine CRL geholt — die Vorgabe wirkt nicht“,
    /// **links: 0, rechts: 1**. Grund: `RUST_TEST_THREADS=2`, also liefen beide
    /// **gleichzeitig**, und der erste Test erhöhte den Zähler **während** der
    /// zweite ihn maß.
    ///
    /// added 03.10.2026 19:az: **DIE LEHRE IST NICHT „Vorsicht bei Zählern“,
    /// sondern: EIN GLOBALER ZÄHLER MACHT TESTS PARALLEL-ABHÄNGIG.** Die
    /// Aussage „die Vorgabe wirkt nicht“ war **falsch** — die Vorgabe wirkt sehr
    /// wohl; nur die Messung war von einem anderen Test verdraht. Ein Test, der
    /// einen gemeinsamen Zustand voraussetzt, ist kein Test, sondern eine
    /// Reihenfolgevorgabe. In **einem** Test mit **zwei** Phasen ist die
    /// Reihenfolge Teil des Tests — und das Delta wird **exakt**.
    #[test]
    #[ignore = "braucht Internet; bewusst nicht Teil des normalen Laufs"]
    fn die_vorgabe_wirkt_nicht_und_auf_verlangen_wirkt_sie() {
        const PFAD: &str = "/etc/ssl/certs/ca-certificates.crt";
        let text =
            std::fs::read_to_string(PFAD).unwrap_or_else(|e| panic!("{PFAD} nicht lesbar ({e})"));
        let speicher = narsil_tls::kette::Vertrauensspeicher::aus_pem(&text).expect("Truststore");
        let anfrage = Request::get("https://xkcd.com", 443, "/");

        // ---- Phase 1: VORGABE. Kein Anstieg, sonst wirkt sie nicht.
        let vor1 = super::widerruf_geprueft();
        let ohne = fetch_mit_speicher(&anfrage, Limits::default(), None, &speicher);
        let nach1 = super::widerruf_geprueft();
        assert_eq!(
            vor1, nach1,
            "ohne Policy wurde trotzdem eine CRL geholt — die Vorgabe wirkt nicht"
        );
        assert!(
            ohne.is_ok(),
            "der Abruf muss auch ohne Policy gelingen: {ohne:?}"
        );

        // ---- Phase 2: AUF VERLANGEN, UND **AUF EINEM ANDEREN HOST**.
        //
        // added 03.10.2026 19:bb, R62. **DAS WAR EIN BEFUND, NICHT NUR EIN
        // TESTFEHLER.** Mit demselben Host stieg der Zaehler in Phase 2 um
        // **0** (GEMESSEN: „mit Policy muss der Zaeiger genau einmal steigen“,
        // links 0, rechts 1) — und der ganze Test lief in **0,03 s**, was fuer
        // einen TLS-Handshake nach `xkcd.com` zu schnell ist. Die Begruendung:
        // `fetch_mit_speicher` nimmt fuer Phase 2 die **gepoolte** Leitung aus
        // Phase 1, also laeuft `verbinde` **nicht** — und die Pruefung sitzt in
        // `verbinde`.
        //
        // added 03.10.2026 19:bb: **DIE SICHERHEITSRELEVANTE FOLGE, im Klartext:
        // DIE PRUEFUNG LAEUFT PRO VERBINDUNG, NICHT PRO ANFRAGE.** Das ist nicht
        // automatisch falsch — die Verbindung wurde **geprueft**, als sie
        // aufgebaut wurde. Aber ein Zertifikat, das **danach** widerrufen wird,
        // gilt auf dieser Leitung **weiter**. Fuer eine Browser-Rolle ist das eine
        // **Entscheidung** (wie lange darf eine geprueffte Leitung leben?),
        // fuer mich eine **Messung**: sie ist in R61 nicht aufgefallen, weil der
        // Test damals nur **eine** Verbindung baute.
        //
        // added 03.10.2026 19:bc: **UND DESHALB EIN ANDERER HOST**, damit Phase 2
        // wirklich eine **neue** Leitung baut. `www.debian.org` ist GEMESSEN
        // erreichbar (R50/R60 haben es mehrfach geprüft) und haelt nach
        // Let's Encrypt — eine **andere** CA als `xkcd.com` (GlobalSign), also
        // auch eine andere Sperrliste. Das ist kein Zufall, sondern die
        // Vielfachmessung: der Test haette sonst nur **eine** CA geprueft.
        let anfrage2 = Request::get("https://www.debian.org", 443, "/");
        let vor2 = super::widerruf_geprueft();
        let limits = Limits {
            widerruf: Widerrufspolitik::HarterFehler,
            ..Limits::default()
        };
        let mit = fetch_mit_speicher(&anfrage2, limits, None, &speicher);
        let nach2 = super::widerruf_geprueft();
        assert_eq!(
            nach2 - vor2,
            1,
            "mit Policy muss der Zaehler **genau einmal** steigen, nicht 0 und nicht 2"
        );
        assert_eq!(
            super::widerruf_blockiert(),
            0,
            "ein gueltiges Zertifikat wurde als widerrufen abgewiesen"
        );
        // added 03.10.2026 19:az: **UND DIE SEITE KOMMT TROTZDEM AN**, weil
        // `GeprueftNichtWiderrufen` **kein** Fehler ist. Wuerde der Test hier nur
        // den Zaehler pruefen, waere ein Client erfuellt, der jede Verbindung nach
        // der Pruefung **abbricht** — also die harte Regel an der falschen Stelle.
        // added 03.10.2026 19:ba: **`unwrap_or` IST FALSCH, `unwrap_or_else`
        // AUCH** — der Fehler muss **panicken**, nicht als Wert ins Ergebnis.
        // GEMESSEN: „expected `&Response`, found `NetError`“ — weil
        // `map_err(|e| …)` die **Fehlerform** in einen `String` wandelt und
        // `unwrap_or` einen **Wert** an dieser Stelle einsetzt. R61 schrieb
        // hier `unwrap()` mit `.map_err(format!)` — dieselbe Absicht, aber ohne
        // Typfehler, weil `String` und `NetError` an dieser Stelle
        // austauschbar **aussehen**.
        let antwort = match &mit {
            Ok(a) => a,
            Err(e) => panic!("der Client-Weg muss eine Seite liefern: {e}"),
        };
        assert_eq!(
            antwort.status.code, 200,
            "der Client-Weg muss 200 liefern — nicht nur keinen Fehler"
        );
        println!(
            "R61-CLIENT Phase 1 {vor1} -> {nach1} (Vorgabe, xkcd.com), Phase 2 {vor2} -> {nach2} \
             (verlangt, www.debian.org), Status {}",
            antwort.status.code
        );
    }

    /// added 03.10.2026 18:9x, R61. **UND `KeineQuelle` LÄSST DEN ABRUF LAUFEN.**
    /// Der weiche Fall ist der, der am leichtesten falsch gebaut wird: wer
    /// `Unbrauchbar` wie einen Widerruf behandelt, verhindert jede Seite, deren CA
    /// keine erreichbare Liste hat. Der Test braucht **kein** Netz, weil
    /// `widerruf_entscheidung` rein ist — er ist in `widerruf_politik` als
    /// Tabelle ueber alle acht Paare abgedeckt. Hier steht nur die **Absicht**,
    /// damit sie beim Nachlesen nicht verlorengeht.
    #[test]
    fn weiche_faelle_brechen_nicht_ab() {
        for urteil in [
            CrlUrteil::KeineQuelle,
            CrlUrteil::Unbrauchbar,
            CrlUrteil::GeprueftNichtWiderrufen,
        ] {
            assert!(
                super::widerruf_entscheidung(Widerrufspolitik::HarterFehler, urteil).is_none(),
                "{urteil:?} darf keinen Fehler ergeben"
            );
        }
    }
}

#[cfg(test)]
mod widerruf_ende_zu_ende {
    use super::{CrlUrteil, NetError, Widerrufspolitik, hole_crl, widerruf_entscheidung};

    /// added 03.10.2026 19:9x, R62. **EIN SERVER IM LOOPBACK, DER EINE ECHTE
    /// SPERRLISTE AUSLIEFERT** — und der Grund fuer den eigenen Server ist
    /// **Nicht** Bequemlichkeit: `hole_crl` kann nur **HTTP im Klartext**
    /// (`strip_prefix("http://")`, R60), und die echten Verteilungspunkte der
    /// Wild sind **nicht** unter unserer Kontrolle. Wer die Kette „Abruf →
    /// Auswertung“ beweisen will, braucht eine URL, die **wir** waehlen.
    ///
    /// added 03.10.2026 19:9x: **UND DIE ZEITGRENZE IST BENANNT**, weil ein
    /// Testserver ohne Grenze ein haengender Test ist: `SERVER_FRIST` =
    /// **20 s**, und der Serverthread beendet sich danach mit einem Fehler, den
    /// der Test als „keine Antwort“ sieht. R17 gilt fuer Tests genauso wie fuer
    /// den Aufrufer.
    const SERVER_FRIST: std::time::Duration = std::time::Duration::from_secs(20);

    /// added 03.10.2026 19:9x, R62. **EINE SPERRLISTE AUF EINEM LOOPBACK-SOCKEL
    /// BEREITSTELLEN.** Der Server nimmt **eine** Verbindung an, schreibt eine
    /// HTTP/1.1-Antwort mit `Content-Length` und beendet sich — genau das
    /// Minimum, was `hole_crl` braucht, und **nicht** mehr. Ein Server, der
    /// mehr kann, wuerde Faehler verdecken, die wir suchen.
    ///
    /// added 03.10.2026 19:9x: **UND `127.0.0.1` STATT `localhost`**, weil
    /// `localhost` zuerst IPv6 liefert (in R60 GEMESSEN: DNS gab fuer
    /// `yr1.c.lencr.org` IPv6 zuerst, und IPv6 ist hier nicht geroutet).
    /// Die IP-Adresse als **Literal** umgeht die Namensaufloesung **und** die
    /// IPv4-vor-IPv6-Frage: der Test soll den Weg pruefen, nicht die
    /// Aufloesung.
    fn crl_serve(der: Vec<u8>) -> String {
        let hoerer = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap_or_else(|e| panic!("Loopback binden: {e}"));
        let port = hoerer
            .local_addr()
            .unwrap_or_else(|e| panic!("lokale Adresse: {e}"))
            .port();
        std::thread::spawn(move || {
            // added 03.10.2026 19:9x: **DER SERVER HAT SELBST EINE FRIST, UND
            // SIE MUSSTE AUF DEN `accept` GEHAENGT WERDEN, WEIL EIN
            // `TcpListener` KEIN `set_read_timeout` HAT.** GEMESSEN: der
            // Uebersetzer meldet „no method named `set_read_timeout` found for
            // struct `TcpListener`“ — die Zeitgrenze gilt nur fuer **Streams**.
            // Also wird der Sockel **nicht-blockierend** gemacht und in einer
            // Schleife bis zum Ablauf der Frist gepollt; das ist die Form, die
            // `TlsVerbindung` in `strom.rs` fuer Lesefristen auch benutzt.
            if hoerer.set_nonblocking(true).is_err() {
                return;
            }
            let ende = std::time::Instant::now() + SERVER_FRIST;
            let mut genommen = None;
            while std::time::Instant::now() < ende {
                match hoerer.accept() {
                    Ok((sock, _)) => {
                        genommen = Some(sock);
                        break;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(_) => return,
                }
            }
            let Some(mut sock) = genommen else {
                return;
            };
            let _ = sock.set_read_timeout(Some(SERVER_FRIST));
            let mut puffer = [0u8; 1024];
            use std::io::Read;
            let _ = sock.read(&mut puffer);
            let kopf = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/pkix-crl\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                der.len()
            );
            use std::io::Write;
            let _ = sock.write_all(kopf.as_bytes());
            let _ = sock.write_all(&der);
            let _ = sock.flush();
        });
        format!("http://127.0.0.1:{port}/x.crl")
    }

    /// added 03.10.2026 19:9x, R62. **DIE VERKETTUNG, UM DIE ES GEHT: HTTP-ABRUF →
    /// AUSWERTUNG → WIDERRUF → ABBRUCH.** R60 hat den Abruf mit echten Servern
    /// bewiesen (186 kB, 332 kB) und die Auswertung mit `testdaten/crl/
    /// widerrufen.der` — aber **nie beides zusammen**. Genau diese Lücke hat R61
    /// als „der harte Fehlerpfad ist ungetestet“ gemeldet.
    ///
    /// added 03.10.2026 19:9x: **UND DIE TESTDATEN SIND FREMD** (`crates/
    /// narsil-tls/testdaten/crl/`). Wir **lesen** sie nur — `include_bytes!` ist
    /// ein Lesezugriff, kein Schreibzugriff. Das ist derselbe Unterschied wie
    /// bei `t10_x509_testdaten.txt` in `x509.rs`, und er wird hier benannt, weil
    /// R61 genau an dieser Grenze gescheitert ist (Fremdtest in `narsil-net/tests/`).
    ///
    /// added 03.10.2026 19:9x: **`include_bytes!` MIT PFAD AUS
    /// `CARGO_MANIFEST_DIR`** ist **nicht** moeglich (der Pfad liegt in der
    /// Schwester-Crate), also `fs::read`. GEMESSEN: die Datei liegt relativ zu
    /// `env!("CARGO_MANIFEST_DIR")` unter `../narsil-tls/testdaten/crl/`.
    const WIDERRUFEN_CRL: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../narsil-tls/testdaten/crl/widerrufen.der"
    ));

    #[test]
    #[ignore = "braucht einen Loopback-Server; bewusst nicht Teil des normalen Laufs"]
    fn abruf_und_auswertung_sagen_zusammen_widerruf() {
        let url = crl_serve(WIDERRUFEN_CRL.to_vec());
        let bytes = hole_crl(&url).unwrap_or_else(|e| panic!("hole_crl({url}) schlug fehl: {e}"));
        assert_eq!(
            bytes.len(),
            WIDERRUFEN_CRL.len(),
            "die geholte Liste hat {} Byte, erwartet {}",
            bytes.len(),
            WIDERRUFEN_CRL.len()
        );
        // added 03.10.2026 19:9x: **UND DIE AUSWERTUNG BRAUCHT ZERTIFIKATE.**
        // `testdaten/crl/` haelt sie als PEM (R06 hat sie erzeugt, im Juni).
        let blatt_pem = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../narsil-tls/testdaten/crl/widerrufen.pem"
        ))
        .unwrap_or_else(|e| panic!("widerrufen.pem: {e}"));
        let ca_pem = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../narsil-tls/testdaten/crl/ca.pem"
        ))
        .unwrap_or_else(|e| panic!("ca.pem: {e}"));
        // added 03.10.2026 19:9x: **UEBER `kette::pem_zerlegen`**, und das ist
        // die Form, die `tests/crl_pruefen.rs` benutzt (Zeile 46). GEMESSEN: in
        // `x509.rs` gibt es **keine** `*_aus_pem`-Funktion — der PEM-Zerleger
        // liegt in `kette`, und ein zweiter Zerleger waere die Kopie aus R60.
        let blatt_roh = narsil_tls::kette::pem_zerlegen(&String::from_utf8_lossy(&blatt_pem))
            .unwrap_or_else(|e| panic!("blatt PEM zerlegbar: {e:?}"));
        let ca_roh = narsil_tls::kette::pem_zerlegen(&String::from_utf8_lossy(&ca_pem))
            .unwrap_or_else(|e| panic!("ca PEM zerlegbar: {e:?}"));
        let blatt = narsil_tls::x509::zertifikat_lesen(&blatt_roh[0])
            .unwrap_or_else(|e| panic!("blatt liest sich: {e:?}"));
        let ca = narsil_tls::x509::zertifikat_lesen(&ca_roh[0])
            .unwrap_or_else(|e| panic!("ca liest sich: {e:?}"));

        // added 03.10.2026 19:9x: **DIE ZEIT AUS DER MESSDATEI, NICHT GERATEN —
        // und der erste Versuch war GERATEN und schlug fehl.** Ich hatte
        // `jetzt = 1_700_000_000` gesetzt (2023-11-14); der Leser meldete
        // `Zeitfenster { jetzt: 1700000000, ab: 1790933492, … }`. GEMESSEN
        // (`cat testdaten/crl/zeiten.txt`): die Liste gilt von **1790933492** bis
        // **1793525492**. `jetzt` liegt also **vor** `ab`, und der Test hat die
        // **falsche** Sache gemessen: nicht „Widerruf erkannt“, sondern
        // „Zeitfenster passt nicht“.
        //
        // added 03.10.2026 19:9x: **UND `crl_pruefen.rs` HAT DAFUER EINEN
        // HELFER** (`gemessen(name)`, Zeile 61) — dieselbe Datei, dieselbe
        // Quelle. Eine eigene Datei-Einleseroutine waere die Kopie aus R60, also
        // wird die **gemessene** Datei gelesen und die **selbe** Form benutzt.
        const ZEITEN: &str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../narsil-tls/testdaten/crl/zeiten.txt"
        ));
        // added 03.10.2026 19:ax: **`(i64, i64)`, NICHT `Vec<i64>`** — mein
        // erster Entwurf annotierte `Vec<i64>` und griff dann auf `.0`/`.1` zu,
        // was der Uebersetzer mit „no field `0` on type `Vec<i64>`“ meldete. Die
        // Annotation war also **falsch**, nicht der Zugriff.
        let fenster: (i64, i64) = ZEITEN
            .lines()
            .filter_map(|z| {
                let f: Vec<&str> = z.split('\t').collect();
                if f.len() == 4 && f[0] == "widerrufen" {
                    Some((f[1].parse().ok()?, f[2].parse().ok()?))
                } else {
                    None
                }
            })
            .next()
            .unwrap_or_else(|| panic!("keine Zeile fuer `widerrufen` in zeiten.txt"));
        let jetzt = (fenster.0 + fenster.1) / 2;
        let urteil = narsil_tls::crl::beurteile(&bytes, &blatt, &ca, jetzt);
        assert!(
            urteil.ist_widerrufen(),
            "die geholte Liste wurde nicht als Widerruf erkannt: {:?}",
            urteil.grund()
        );
        // added 03.10.2026 19:9x: **UND DAS ENTSCHEIDET ZU EINEM ABBRUCH.** Das
        // ist der ganze Zweck der Kette: aus „die CA sagt widerrufen“ wird
        // „die Verbindung wird nicht benutzt“.
        let fehler: Option<NetError> =
            widerruf_entscheidung(Widerrufspolitik::HarterFehler, CrlUrteil::Widerrufen);
        assert!(
            fehler.is_some(),
            "ein belegter Widerruf muss zu einem Fehler fuehren"
        );
        println!(
            "R62-KETTE {} Byte geholt, Urteil widerrufen, Abbruch {}",
            bytes.len(),
            fehler.map(|e| e.to_string()).unwrap_or_default()
        );

        // added 03.10.2026 19:ay, R62. **DER GEGENFALL IM SELBEN TEST, UND ER
        // BRAUCHT EINE ZWEITE, LEERE LISTE.** `testdaten/crl/gut.der` ist
        // dieselbe CA, **ohne** jeden Widerruf (GEMESSEN in `zeiten.txt`: die
        // Zeile `gut` hat in der letzten Spalte `-`, also **keine**
        // Widerrufszeit). Dieselbe Kette, dieselbe Seriennummer, dieselbe
        // Auswertung — und **kein** Abbruch.
        //
        // added 03.10.2026 19:ay: **WARUM ZWEI SERVER UND NICHT EINE ANFRUG:**
        // `hole_crl` beweist, dass er den **Kopf** findet; der Gegenfall
        // beweist, dass die **Bytes** ausgewertet werden. Ein Test nur mit der
        // vollen Liste waere von einem Abruf erfuellt, der **immer** `Ok` sagt.
        const GUT_CRL: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../narsil-tls/testdaten/crl/gut.der"
        ));
        let url_leer = crl_serve(GUT_CRL.to_vec());
        let leer =
            hole_crl(&url_leer).unwrap_or_else(|e| panic!("hole_crl({url_leer}) schlug fehl: {e}"));
        assert_eq!(
            leer.len(),
            GUT_CRL.len(),
            "die leere Liste kam mit {} Byte an, erwartet {}",
            leer.len(),
            GUT_CRL.len()
        );
        let urteil_leer = narsil_tls::crl::beurteile(&leer, &blatt, &ca, jetzt);
        assert_eq!(
            urteil_leer,
            narsil_tls::crl::Urteil::NichtWiderrufen,
            "die leere Liste liess das Blatt fallen: {:?}",
            urteil_leer.grund()
        );
        assert!(
            widerruf_entscheidung(
                Widerrufspolitik::HarterFehler,
                CrlUrteil::GeprueftNichtWiderrufen
            )
            .is_none(),
            "eine leere Liste darf keinen Abbruch ergeben"
        );
        println!(
            "R62-GEGENFALL {} Byte, kein Widerruf, kein Abbruch",
            leer.len()
        );
    }
}

#[cfg(test)]
mod widerruf_politik {
    use super::{CrlUrteil, NetError, Widerrufspolitik, widerruf_entscheidung};

    /// added 03.10.2026 18:7x, R61. **ALLE ACHT KOMBINATIONEN, UND DAS IST DER
    /// GANZE TEST.** Die Regel lautet: **nur** `HarterFehler` **und**
    /// `CrlUrteil::Widerrufen` ergeben einen Fehler.
    ///
    /// added 03.10.2026 18:7x: **WARUM ALLE ACHT UND NICHT ZWEI.** Ein Test, der
    /// nur den Fehlerfall prueft, waere von einer Funktion erfuellt, die
    /// **immer** `Some` zurueckgibt. Ein Test, der nur den Erfolgsfall prueft,
    /// waere von einer erfuellt, die **immer** `None` zurueckgibt. Beide
    /// Fehler sind in R59 genau so passiert: `verteilungsstellen_als_texte` gab
    /// **immer** leer zurueck, und kein Test sah es.
    #[test]
    fn nur_ein_paar_bricht_ab() {
        let urteile = [
            CrlUrteil::KeineQuelle,
            CrlUrteil::Unbrauchbar,
            CrlUrteil::GeprueftNichtWiderrufen,
            CrlUrteil::Widerrufen,
        ];
        for politik in [
            Widerrufspolitik::NichtPruefen,
            Widerrufspolitik::HarterFehler,
        ] {
            for urteil in urteile {
                let erwartet = matches!(
                    (politik, urteil),
                    (Widerrufspolitik::HarterFehler, CrlUrteil::Widerrufen)
                );
                let got = widerruf_entscheidung(politik, urteil);
                assert_eq!(
                    got.is_some(),
                    erwartet,
                    "{politik:?} + {urteil:?} ergab {got:?}, erwartet {}",
                    if erwartet {
                        "einen Fehler"
                    } else {
                        "keinen Fehler"
                    }
                );
            }
        }
    }

    /// added 03.10.2026 18:7x, R61. **UND DER FEHLER IST DER BENANNTE, NICHT
    /// `Tls`.** Grund: `Tls(String)` sagt „der Handshake ist gescheitert“ — hier
    /// ist er **gelungen**. Wer die Meldung nicht unterscheiden kann, kann einen
    /// belegten Widerruf nicht von einem Netzfehler trennen — und die Handlung ist
    /// verschieden (erneut versuchen: ja; bei Widerruf: nein).
    #[test]
    fn der_fehler_nennt_den_widerruf() {
        let e = widerruf_entscheidung(Widerrufspolitik::HarterFehler, CrlUrteil::Widerrufen)
            .unwrap_or(NetError::ZertifikatWiderrufen);
        assert!(matches!(e, NetError::ZertifikatWiderrufen));
        let text = e.to_string();
        assert!(
            text.contains("widerrufen"),
            "die Meldung nennt den Widerruf nicht: {text}"
        );
    }

    /// added 03.10.2026 18:7x, R61. **DIE VORGABE IST `NichtPruefen`, UND DAS
    /// IST EINE ENTSCHEIDUNG, DIE GETESTET GEHOERT.** Eine Policy, die man
    /// umschaltet, ohne es zu merken, ist still — und „still“ war in dieser
    /// Rolle schon dreimal der Befund (R50 Erweiterungen, R51 `_kritisch`, R59
    /// Verteilungspunkte).
    #[test]
    fn die_vorgabe_prueft_nicht() {
        assert_eq!(
            super::Limits::default().widerruf,
            Widerrufspolitik::NichtPruefen,
            "die Vorgabe der Widerrufspruefung hat sich geaendert"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// added 02.10.2026 02:5x UTC (Projektlabel 02.10.2026), Rolle TLS.
    /// added 02.10.2026 02:5x: DIE MESSUNG, um die es hier geht.
    /// `team/AUTONOM.txt` Zeile 691: „www.rust-lang.org haengt 30 s im
    /// TRANSPORT (nicht im Seitenweg) … Ursache messen (h2 Fluss-/
    /// Fenstersteuerung? Keep-Alive-Pool liefert tote Verbindung? Lesen ohne
    /// Frist?) und beheben". Die HTML-Rolle hat zwei Haenger gemessen (51,7 s
    /// und 84,8 s Gesamtzeit bei unter 40 ms Phasensumme) und ihre
    /// Protokollzeilen des Haengerlaufs **nicht** ausgelesen — sie kam an
    /// diese Zeile nicht, weil der Lauf erst nach ihrer Messung fertig war.
    /// Diese Messung liegt eine Ebene tiefer: direkt auf `fetch`, mit
    /// Phasen, Gesamtdauer und Poolzaehler je Lauf.
    ///
    /// added 02.10.2026 02:5x: **WARUM `#[ignore]`** — der Test braucht das
    /// Netz. GEMESSEN: `bau.sh narsil-net test` (ohne `--ignored`) darf keinen
    /// Lauf ins Netz tun, sonst ist die Abnahme von der Tagesform abhaengig.
    /// Aufruf:
    ///
    /// ```text
    /// werkzeuge/bau.sh narsil-net test --lib haenger_messung -- --ignored --nocapture
    /// ```
    ///
    /// added 02.10.2026 02:5x: DIE FRIST, und sie ist der eigentliche
    /// Erfolg der Messung. Jeder Lauf hat eine Obergrenze
    /// (`FRIST_LAUF`), und ein Lauf, der sie reisst, wird **gemeldet und
    /// beendet** — nicht weiter gemessen. Sonst waere die Messung selbst das
    /// Werkzeug, das den Hänger verdeckt.
    /// added 02.10.2026 02:5x UTC (Projektlabel 02.10.2026), Rolle TLS.
    /// added 02.10.2026 02:5x: DIE MESSUNG, um die es hier geht.
    /// `team/AUTONOM.txt` Zeile 691: „www.rust-lang.org haengt 30 s im
    /// TRANSPORT (nicht im Seitenweg) … Ursache messen (h2 Fluss-/
    /// Fenstersteuerung? Keep-Alive-Pool liefert tote Verbindung? Lesen ohne
    /// Frist?) und beheben". Die HTML-Rolle hat zwei Haenger gemessen (51,7 s
    /// und 84,8 s Gesamtzeit bei unter 40 ms Phasensumme) und ihre
    /// Protokollzeilen des Haengerlaufs **nicht** ausgelesen, weil der Lauf
    /// erst nach ihrer Messung fertig war. Diese Messung liegt eine Ebene
    /// tiefer: direkt auf `fetch`, mit Phasen, Gesamtdauer und Poolzaehler
    /// je Lauf.
    ///
    /// added 02.10.2026 02:6x: WAS BIS HIER GEMESSEN IST, und es ist nicht die
    /// 30 s, sondern etwas anderes — gemessen an `www.rust-lang.org`, je
    /// drei kalte Laeufe (Cache geloescht, Pool geleert):
    ///   * Lauf 1: HTTP 200, 18594 Byte, **1900 ms** (tls 340 ms, erstes_byte
    ///     22 ms — rund 1,5 s **nach** dem ersten Byte, in keiner Phase).
    ///   * Lauf 2 bis 5: **alle** `FEHLER HTTP/2: die Verbindung zu
    ///     www.rust-lang.org endete, bevor ein Rahmen kam`, bei 86 bis 97 ms
    ///     Gesamtzeit. Das ist kein Haenger, das ist ein sofortiges Ende.
    ///
    /// added 02.10.2026 02:6x: DIE GEGENPROBE, ohne die nichts gilt:
    /// `curl --http2` **achtmal** und `curl --http1.1` **viermal**
    /// hintereinander gegen dieselbe Adresse: 85 bis 142 ms, HTTP 301, kein
    /// einziger Fehler. Der Weg ist also **unserer**.
    /// added 02.10.2026 02:6x: DER A/B-VERGLEICH, damit daraus kein Vorwurf
    /// wird: Lauf 1 kennt kein Ticket, die folgenden bieten das aus Lauf 1 an
    /// (unsere Wiederaufnahme, R4n/R5j). Mit leerem Ticketspeicher
    /// (`Tickets::leeren`, `ticket.rs:729`) ist das Bild **identisch** — erster
    /// Lauf gut, alle folgenden EOF. Es ist also **nicht** die Wiederaufnahme.
    /// added 02.10.2026 02:7x: WAS DESHALB NOCH OFFEN WAR, und warum dieser
    /// Test jetzt mehrere Hosts nimmt: server- oder clientseitig. Bricht der
    /// zweite Lauf **ueberall** ab, ist es unser Zustand; bricht er nur bei
    /// einem Host ab, ist es der Server. Die Liste kommt aus
    /// `NARSIL_HAENGER_HOSTS`, damit der Vergleich in **einem** Lauf laeuft und
    /// nicht von der Tagesform abhaengt.
    ///
    /// added 02.10.2026 02:5x: **WARUM `#[ignore]`** — der Test braucht das
    /// Netz. GEMESSEN: `bau.sh narsil-net test` (ohne `--ignored`) darf keinen
    /// Lauf ins Netz tun, sonst ist die Abnahme von der Tagesform abhaengig.
    /// Aufruf:
    ///
    /// ```text
    /// werkzeuge/bau.sh narsil-net test --lib haenger_messung -- --ignored --nocapture
    /// ```
    ///
    /// added 02.10.2026 02:5x: DIE FRIST, und sie ist der eigentliche Erfolg
    /// der Messung. Jeder Lauf hat eine Obergrenze (`FRIST_LAUF`), und ein
    /// Lauf, der sie reisst, wird **gemeldet und beendet** — nicht weiter
    /// gemessen. Sonst waere die Messung selbst das Werkzeug, das den
    /// Haenger verdeckt.
    /// added 02.10.2026 02:9x UTC (Projektlabel 02.10.2026), Rolle TLS.
    /// added 02.10.2026 03:0x: DIE REGEL, als Test. Sie wird an drei Stellen
    /// gebraucht (h2-Antwort, h1-Schreiben, h1-Lesen), und drei Stellen sind
    /// drei Moeglichkeiten fuer dieselbe Frage — darum steht sie in
    /// `darf_wiederholt_werden` und wird hier **an der Form** geprueft, nicht
    /// an einem Host: die Messung am Host stand am 02:7x, die Regel gilt aber
    /// immer.
    ///
    /// added 02.10.2026 03:0x: DER GEMESSENE FALL steht ganz oben, weil er
    /// der ist, den es vorher nicht gab: **frische** Leitung, sofort EOF,
    /// Wiederaufnahme benutzt -> **wiederholen**. Vorher war das `false`
    /// („eine frisch aufgebaute Leitung, die sofort EOF liefert, auch nicht").
    /// added 02.10.2026 11:2x UTC (Projektlabel 02.10.2026), Rolle TLS: DIE
    /// REGEL FUER DEN **SCHRITTFEHLER**, und sie hat zwei Seiten: was
    /// wiederholt wird und was ausdruecklich **nicht**.
    ///
    /// added 02.10.2026 11:2x: **WARUM EIN EIGENER TEST** und kein weiterer
    /// `assert` in `die_wiederholregel_trifft_den_gemessenen_fall`: dort geht es
    /// um den **gemessenen** Fall (EOF vor dem ersten Rahmen), hier um eine
    /// **Aenderung** der Regel. Mischte ich beides, waere nach einem Bruch nicht
    /// mehr zu sagen, welcher der beiden Zweile gerissen ist.
    #[test]
    fn ein_schrittfehler_auf_gepoolter_leitung_wird_wiederholt() {
        let h2 = NetError::H2SchreibenFehlgeschlagen {
            grund: "die Verbindung nimmt keine weiteren Daten an".to_string(),
        };
        let h1 = NetError::Send(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "die Verbindung nimmt keine weiteren Daten an",
        ));
        // added 02.10.2026 11:3x: **DIE BEIDEN WEGE**, weil es zwei Zeilen im
        // Betrieb sind und beide gemessen fehlten: der h2-Weg und der h1-Weg.
        assert!(
            darf_wiederholt_werden(true, false, &h2, true, 0),
            "gepoolte h2-Leitung, Schreiben geht nicht: wiederholen"
        );
        assert!(
            darf_wiederholt_werden(true, false, &h1, true, 0),
            "gepoolte h1-Leitung, Schreiben geht nicht: wiederholen"
        );
        // added 02.10.2026 11:3x: **UND DIE DREI FALLEN, DIE ES UNVERAENDERT
        // LASSEN** — ohne diese drei waere der Test auch gruen, wenn die Regel
        // einfach alles wiederholte.
        assert!(
            !darf_wiederholt_werden(false, false, &h2, true, 0),
            "frische Leitung: NICHT wiederholen — dann stimmt etwas nicht"
        );
        assert!(
            !darf_wiederholt_werden(true, false, &h2, false, 0),
            "nicht idempotente Methode (POST): NICHT wiederholen (RFC 9110 9.2.2)"
        );
        assert!(
            !darf_wiederholt_werden(
                true,
                false,
                &NetError::H2Protokoll {
                    grund: "unbekannte Rahmenart".to_string()
                },
                true,
                0
            ),
            "ein echter Protokollfehler: NICHT wiederholen — sonst wird er verdeckt"
        );
        // added 02.10.2026 11:3x: **UND DIE GRENZE DER WIEDERHOLUNGEN**, die
        // nicht an der Fehlerform haengt, sondern an der Zahl.
        assert!(
            !darf_wiederholt_werden(true, false, &h2, true, pool::MAX_WIEDERHOLUNGEN),
            "die Wiederholungsgrenze gilt auch hier"
        );
    }

    #[test]
    fn die_wiederholregel_trifft_den_gemessenen_fall() {
        let eof = NetError::H2EndeVorzeitig {
            host: "h".to_string(),
        };
        assert!(
            darf_wiederholt_werden(false, true, &eof, true, 0),
            "frische Leitung + Wiederaufnahme + EOF vor dem ersten Rahmen: wiederholen"
        );
        // added 02.10.2026 03:0x: OHNE Wiederaufnahme bleibt die alte Regel
        // unveraendert. Das ist der Punkt: die Aenderung ist so klein wie
        // moeglich, und wenn jemand sie wegbrechen will, faellt **dieser**
        // Test hier.
        assert!(
            !darf_wiederholt_werden(false, false, &eof, true, 0),
            "frische Leitung ohne Wiederaufnahme: nicht wiederholen (unveraenderte Regel)"
        );
        // added 02.10.2026 03:0x: gepoolt bleibt wie vorher.
        assert!(
            darf_wiederholt_werden(true, false, &eof, true, 0),
            "gepoolte Leitung + EOF: wiederholen (R5d, unveraendert)"
        );
        assert!(
            darf_wiederholt_werden(
                true,
                true,
                &NetError::H2ServerBeendet {
                    host: "h".to_string(),
                    letzter_strom: 7,
                    // added 03.10.2026, Rolle NETZTEMPO: `NO_ERROR`. Der
                    // Grund gehoert jetzt in die Variante (siehe dort), und
                    // dieser Test **muss** ihn fuellen — sonst pruefte er
                    // eine Fehlerform, die es nicht mehr gibt.
                    grund: 0,
                },
                true,
                0
            ),
            "GOAWAY: wiederholen (RFC 9113 6.8)"
        );
        // added 02.10.2026 03:0x: DER HTTP/1.1-FALL, gemessen mit
        // `NARSIL_ALPN=h1`: `Receive` nach 96 und 102 ms mit `wdn 1`.
        assert!(
            darf_wiederholt_werden(
                false,
                true,
                &NetError::Receive(std::io::Error::from(std::io::ErrorKind::ConnectionReset)),
                true,
                0
            ),
            "frische Leitung + Wiederaufnahme + Receive: wiederholen"
        );
        // added 02.10.2026 03:0x: DIE GRENZEN, jede einzeln.
        assert!(
            !darf_wiederholt_werden(false, true, &eof, false, 0),
            "POST ist nicht wiederholbar (RFC 9110 9.2.2)"
        );
        assert!(
            !darf_wiederholt_werden(false, true, &eof, true, pool::MAX_WIEDERHOLUNGEN),
            "die Wiederholungen sind begrenzt"
        );
        assert!(
            !darf_wiederholt_werden(
                true,
                true,
                &NetError::H2Protokoll {
                    grund: "kaputt".into()
                },
                true,
                0
            ),
            "ein Protokollfehler wird nicht wiederholt — das Wiederholen wuerde ihn nur verdecken"
        );
    }

    #[test]
    #[ignore = "braucht Internet; die Messung fuer AUTONOM Zeile 691"]
    fn haenger_messung() {
        /// added 02.10.2026 02:7x: drei aufeinanderfolgende kalte Laeufe je
        /// Host. GEMESSEN, warum drei: der erste Lauf ist der mit frischem
        /// Zustand, der zweite der mit Ticket und Pool, der dritte zeigt, ob
        /// es ab dem zweiten gleichbleibt.
        const PRO_HOST: usize = 3;
        /// added 02.10.2026 02:5x: die Obergrenze je Lauf. GEMESSEN, woher
        /// die Zahl kommt: die schnellsten Laeufe der HTML-Rolle waren
        /// 120 ms und 595 ms, ihre Haenger 51,7 s und 84,8 s. Eine Grenze
        /// von 15 s trennt die beiden Welten um Faktor 25, und ein einzelner
        /// Lauf bei 15 s kann im h2-Pfad (`FRIST_ERSTER` = 20 s) noch gar
        /// nicht abbrechen — die Grenze ist also nicht zu knapp.
        const FRIST_LAUF: std::time::Duration = std::time::Duration::from_secs(15);
        let hosts: Vec<String> = std::env::var("NARSIL_HAENGER_HOSTS")
            .ok()
            .map(|s| {
                s.split(',')
                    .map(|h| h.trim().to_string())
                    .filter(|h| !h.is_empty())
                    .collect()
            })
            .unwrap_or_else(|| vec!["www.rust-lang.org".to_string()]);
        // added 02.10.2026 02:5x: DER KALTE CACHE, und er wird **geloescht**,
        // nicht umgangen. `NARSIL_CACHE_PFAD` wird beim Start einmal gelesen
        // (`cache.rs:130`), also reicht es nicht, die Datei zu wechseln —
        // sie muss weg. Sonst waeren die Laeufe 2 und 3 warme Laeufe und
        // die Aussage waere eine ueber den Cache gemachte.
        let cache = std::env::var("NARSIL_CACHE_PFAD").unwrap_or_default();
        let mut langsam = 0usize;
        let mut fehler = 0usize;
        let mut gut = 0usize;
        let mut lauf_nr = 0usize;
        for host in &hosts {
            for _ in 1..=PRO_HOST {
                lauf_nr += 1;
                // added 02.10.2026 04:5x: DIE PAUSE, umschaltbar, weil die
                // Frage „welches `obfuscated_ticket_age` schicken wir" nur mit
                // einem Ticket messbar ist, das aelter als eine Sekunde ist —
                // bei einem frischen Ticket ist der Wert 0 und die Frage
                // unentscheidbar. GEMESSEN (04:5x) mit einem Ticket aus dem
                // Lauf davor und 3 s Pause: der Wert auf der Leitung.
                if lauf_nr > 1 {
                    if let Ok(ms) = std::env::var("NARSIL_HAENGER_PAUSE_MS") {
                        if let Ok(ms) = ms.parse::<u64>() {
                            std::thread::sleep(std::time::Duration::from_millis(ms));
                        }
                    }
                }
                if !cache.is_empty() {
                    let _ = std::fs::remove_file(&cache);
                }
                // added 02.10.2026 02:7x: DER POOL, und er ist umschaltbar,
                // weil er der Verdacht ist. GEMESSEN 02:7x an vier Hosts: bei
                // den Fehlschlaegen war `h2_neu` **0** — es wurde also **keine
                // neue** h2-Verbindung gebaut, es lief ueber die
                // poolbehaltene Verbindung aus dem Lauf davor. Das ist genau
                // die Vermutung aus AUTONOM 691 („Keep-Alive-Pool liefert
                // tote Verbindung?"). `NARSIL_HAENGER_POOL=behalten` laesst den
                // Pool in stehen und macht es eindeutig: dann **muss** Lauf 2
                // dieselbe Leitung nehmen.
                if std::env::var("NARSIL_HAENGER_POOL").as_deref() != Ok("behalten") {
                    crate::pool::leeren();
                }
                crate::phasen_zuruecksetzen();
                // added 02.10.2026 02:8x: DER SCHALTER FUER DIE WIEDERAUFNAHME,
                // und er leert den Speicher **vor jedem Lauf**. GEMESSEN 02:7x:
                // `wdn 1` bei jedem Fehlschlag und `wdn 0` bei jedem Erfolg —
                // die Wiederaufnahme ist also der Unterschied. added 02.10.2026
                // 02:6x: mein erster A/B-Versuch war **fehlerhaft** (der
                // Speicher wurde nur einmal vor dem zweiten Teil geleert, also
                // liefen die Fehlschlaege wieder mit Ticket); das steht so im
                // Kommentar, damit niemand die alte Aussage wiederholt.
                if std::env::var("NARSIL_HAENGER_OHNE_TICKET").as_deref() == Ok("1") {
                    if let Ok(mut t) = crate::ticket_speicher().lock() {
                        t.leeren();
                    }
                }
                let h2_vor = crate::h2_verbindungen();
                let wv_vor = crate::wiederaufnahme_versuche();
                let anfang = std::time::Instant::now();
                let anfrage = Request::get(host.as_str(), 443, "/").https();
                let ergebnis = crate::fetch(&anfrage);
                let dauer = anfang.elapsed();
                let phasen = crate::phasen();
                let h2_neu = crate::h2_verbindungen() - h2_vor;
                let s = match &ergebnis {
                    Ok(r) => {
                        gut += 1;
                        format!("HTTP {} {} Byte", r.status.code, r.body.len())
                    }
                    Err(e) => {
                        fehler += 1;
                        format!("FEHLER {e}")
                    }
                };
                // added 02.10.2026 02:5x: DIE AUSGABE, und sie ist der Punkt.
                // Jede Phase einzeln, damit ein Haenger **im** Transport von
                // einem langsamen Server unterscheidbar ist — genau das war
                // an der Phasentabelle der HTML-Rolle nicht zu sehen.
                let ms = |d: Option<std::time::Duration>| match d {
                    Some(x) => format!("{:.1}", x.as_secs_f64() * 1000.0),
                    None => "-".to_string(),
                };
                println!(
                    "Lauf {lauf_nr:2} {host:22} {s:.30} gesamt {:>8.1} ms  dns {:>6}  tcp {:>6}  tls {:>6}  erstes {:>7}  h2_neu {}  wdn {}  pool {}",
                    dauer.as_secs_f64() * 1000.0,
                    ms(phasen.as_ref().and_then(|p| p.dns)),
                    ms(phasen.as_ref().and_then(|p| p.tcp)),
                    ms(phasen.as_ref().and_then(|p| p.tls)),
                    ms(phasen.as_ref().and_then(|p| p.erstes_byte)),
                    h2_neu,
                    crate::wiederaufnahme_versuche() - wv_vor,
                    crate::pool::offene_verbindungen()
                );
                if dauer > FRIST_LAUF {
                    langsam += 1;
                    println!(
                        "  ** LANGSAM: {:>8.1} ms ueber der Frist {:?} — Phasen: {:?}",
                        dauer.as_secs_f64() * 1000.0,
                        FRIST_LAUF,
                        phasen
                    );
                }
            }
        }
        println!(
            "\n{lauf_nr} Lauf(e) ueber {} Host(s): {gut} Antwort(en), {fehler} Fehler, {langsam} ueber {:?}.",
            hosts.len(),
            FRIST_LAUF
        );
        // added 02.10.2026 02:6x: DIE ZUSICHERUNG, und sie ist absichtlich
        // schwach. Die erste Fassung panickte bei jedem Fehler
        // (`assert_eq!(fehler, 0)`) und machte damit **9 Fehler zu einem roten
        // Lauf** — dabei sind die Fehler der Befund und nicht der Misserfolg
        // dieses Tests. Jetzt gilt nur noch die Regel, die wirklich bei uns
        // liegt: **kein Lauf darf ohne Ende laufen**. Das ist die Aussage der
        // AUTONOM-Zeile („jedes Lesen mit Frist""), und sie ist es wert, hier
        // gruen zu sein oder rot zu werden.
        assert_eq!(
            langsam, 0,
            "{langsam} Lauf(e) ueber {:?} — das ist der Haenger, den AUTONOM meldet",
            FRIST_LAUF
        );
    }

    /// DER BYTE-DRAHT FUER DEN FUZZ, und er steht hier, weil `Leitung` ein
    /// geschlossenes Baustein ist (`Send + Read + Write`).
    ///
    /// added 01.10.2026 16:5x, T5. Er nimmt einen `Vec<u8>` und gibt ihn
    /// stueckweise wieder — so wie es ein Sockel tut, nur ohne Netz. Das ist
    /// der ehrlichere Zugewende: es prueft die **Bytes**, nicht die
    /// Zufriedenheit des Kernels, und 100 000 Durchlaeufe brauchen keine
    /// 100 000 Sockets.
    ///
    /// added 01.10.2026 16:5x: `chunk` begrenzt die Stueckgroesse. GEMESSEN,
    /// warum das noetig ist: ohne die Grenze gibt der Draht alles auf einmal,
    /// und der Fuzz prueft dann nur den Fall „alles im ersten Lesevorgang" —
    /// der Randfall, an dem ein Pufferfehler *sichtbar* wuerde, kommt dann nie
    /// vor.
    struct FuzzDraht {
        rest: Vec<u8>,
        chunk: usize,
        geschrieben: Vec<u8>,
    }

    impl std::io::Read for FuzzDraht {
        fn read(&mut self, p: &mut [u8]) -> std::io::Result<usize> {
            let n = self.rest.len().min(p.len()).min(self.chunk);
            if n == 0 {
                return Ok(0);
            }
            p[..n].copy_from_slice(&self.rest[..n]);
            self.rest.drain(..n);
            Ok(n)
        }
    }

    impl std::io::Write for FuzzDraht {
        fn write(&mut self, p: &[u8]) -> std::io::Result<usize> {
            self.geschrieben.extend_from_slice(p);
            Ok(p.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Leitung for FuzzDraht {
        // added 01.10.2026 16:5x: die drei Sockel-Einstellungen. GEMESSEN, was
        // passiert, wenn man sie weglässt: `E0046 not all trait items
        // implemented` — `Leitung` verlangt sie, und das ist **richtig**, denn
        // ein echter Client muss sie setzen koennen.
        //
        // DIE DREI SIND HIER STUMME NOPs, und das wird benannt: der Fuzz
        // prueft den **Parser**, nicht das Netz. Wer aus diesem Test eine
        // Aussage ueber Zeitlimits ableitet, hat sie nicht gemessen — dafuer
        // sind die Fristen in `pool_live.rs` und `h2_client.rs` da.
        fn set_nodelay(&mut self, _an: bool) -> std::io::Result<()> {
            Ok(())
        }
        // added 01.10.2026 16:5x: `Option<Duration>`, nicht `Duration`. GEMESSEN:
        // mit `Duration` meldete der Uebersetzer `E0053 ... incompatible type
        // for trait` — `None` bedeutet bei `TcpStream` „unendlich", und genau
        // das ist der Fall, den ein Fuzz braucht: ein Block, der nicht
        // endet, darf nicht von einem Timeout abhaengen.
        fn set_read_timeout(&mut self, _d: Option<std::time::Duration>) -> std::io::Result<()> {
            Ok(())
        }
        fn set_write_timeout(&mut self, _d: Option<std::time::Duration>) -> std::io::Result<()> {
            Ok(())
        }
        fn shutdown(&mut self, _how: std::net::Shutdown) -> std::io::Result<()> {
            Ok(())
        }
        // added 01.10.2026 16:5x: die Adressen sind `Err`, nicht `127.0.0.1`.
        // GEMESSEN, warum das der Unterschied ist: ein Parser, der
        // `local_addr` benutzt, um „localhost" zu erkennen, erkennt es dann
        // **nicht** — und der Fuzz soll den Fall ohne Adresse sehen, nicht den
        // mit.
        fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
            Err(std::io::Error::other("FuzzDraht hat keine Adresse"))
        }
        fn peer_addr(&self) -> std::io::Result<std::net::SocketAddr> {
            Err(std::io::Error::other("FuzzDraht hat keine Adresse"))
        }
        fn connect_timeout(
            _addr: &std::net::SocketAddr,
            _timeout: std::time::Duration,
        ) -> std::io::Result<Self> {
            Err(std::io::Error::other("FuzzDraht verbindet nicht"))
        }
    }

    /// DER ZUFALL FUER DEN HTTP-FUZZ.
    ///
    /// added 01.10.2026 16:5x. Derselbe XorShift64 wie in `tests/h2_fuzz.rs`,
    /// und **nicht** wiederholt: eine zweite Kopie eines Zufalls generators
    /// ist eine zweite Wahrheit ueber dieselbe Sache. Der Startwert steht hier
    /// bewusst anders als dort, damit die beiden Fuzze **verschiedene** Faelle
    /// sehen.
    struct FuzzZufall {
        zustand: u64,
    }

    impl FuzzZufall {
        fn ab_start() -> Self {
            FuzzZufall {
                zustand: 0x0123_4567_89AB_CDEF,
            }
        }
        fn naechste(&mut self) -> u64 {
            let mut x = self.zustand;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.zustand = x;
            x
        }
        fn bytes(&mut self, laenge: usize) -> Vec<u8> {
            (0..laenge)
                .map(|_| (self.naechste() & 0xff) as u8)
                .collect()
        }
    }

    /// M3: so viele Durchlaeufe fuer den HTTP-1.1-Pfad.
    ///
    /// added 01.10.2026 16:5x, T5. `100_000` ist die Vorgabe aus
    /// `AUTONOM.txt` 605. Die Grenze ist benannt, weil sie eine
    /// eingabeabhaengige Schleife ist.
    const FUZZ_DURCHLAEUFE: usize = 100_000;

    /// M4: so gross darf der Byte-Strom je Durchlauf sein.
    ///
    /// added 01.10.2026 16:5x. 4 096 Byte, weil die groesste gueltige
    /// Antwortkopfzeile im Testmaterial 5 156 Byte hatte (`github.com`,
    /// GEMESSEN in R4z8) — darueber hinaus prueft der Fuzz nichts Neues, und
    /// die Zeit waere fuer den Fund nicht gewonnen.
    const FUZZ_BYTES: usize = 4096;

    /// DER PANIK IN KLEINSTER FORM, und er ist der Grund, warum der Fuzz
    /// existiert.
    ///
    /// added 01.10.2026 16:6x, T5. **Sechs Byte.** Die Zeile
    /// `X-Bild: \xc3\x28` — ein Name, ein Doppelpunkt, und **zwei** Byte, die
    /// kein gueltiges UTF-8 sind (`\xc3` erwartet ein Fortsetzungsbyte, bekommt
    /// `(`).
    ///
    /// WAS DAVOR PASSIERT ist: `String::from_utf8_lossy` macht aus `\xc3\x28`
    /// **drei** Byte (`U+FFFD` ist 3 Byte lang). Der Doppelpunkt liegt in den
    /// Rohbytes an Stelle 7, in der verlustbehafteten String aber an Stelle 9.
    /// Der alte Code schnitt an der Rohbyte-Stelle — mitten im Ersatzzeichen.
    ///
    /// GEMESSEN, 01.10.2026 16:5x: „end byte index 7 is not a char boundary; it
    /// is inside '\u{fffd}'". Der Fuzz fand es beim **ersten** Durchlauf; ein
    /// Test, der den Fall benennt, ist trotzdem noetig — ein Fuzz, der ihn nur
    /// zufaellig trifft, verliert ihn beim Aendern des Zufallsstarts.
    ///
    /// DIE BEHAUPTUNG: keine Panik, und die `Trailer`-Ankündigung wird **nicht**
    /// erkannt (der Name ist ja kein `trailer`).
    #[test]
    fn eine_kopfzeile_mit_ungueltigem_utf8_bricht_nicht_um() {
        let draht = FuzzDraht {
            rest: b"HTTP/1.1 200 OK\r\n\xc3: x\r\n\r\n".to_vec(),
            chunk: 3,
            geschrieben: Vec::new(),
        };
        let limits = Limits::default();
        let frist = Frist::neu(std::time::Duration::from_millis(50));
        let mut leser = Leser::neu(Box::new(draht), &limits, &frist);
        let anfrage = Request::get("https://beispiel.test", 443, "/");
        // DER AUFRUF, und er darf **nur** `Ok` oder `Err` liefern.
        let _ = lese_antwort(&mut leser, &anfrage, &limits);
    }

    /// GEGENPROBE: derselbe Aufruf mit einem **gueltigen** Namen, damit der
    /// Test nicht gruen waere, wenn `lese_headerblock` gar nichts mehr laest.
    ///
    /// added 01.10.2026 16:6x. Ohne diese Zeile waere der Test oben auch dann
    /// gruen, wenn der Lesevorgang aus faengt und `Err` liefert — und genau
    /// das ist die Art von Test, die eine Regression als Erfolg meldet.
    #[test]
    fn eine_kopfzeile_mit_gueltigem_namen_wird_gelesen() {
        let draht = FuzzDraht {
            rest: b"HTTP/1.1 200 OK\r\nX-Bild: ja\r\n\r\n".to_vec(),
            chunk: 4,
            geschrieben: Vec::new(),
        };
        let limits = Limits::default();
        let frist = Frist::neu(std::time::Duration::from_millis(50));
        let mut leser = Leser::neu(Box::new(draht), &limits, &frist);
        let anfrage = Request::get("https://beispiel.test", 443, "/");
        let antwort = lese_antwort(&mut leser, &anfrage, &limits)
            .expect("eine gueltige Antwort mit gueltigem Namen muss lesbar sein");
        assert_eq!(
            antwort.status.code, 200,
            "der Status kam aus der Statuszeile"
        );
        // added 01.10.2026 16:6x: `headers` ist eine `HashMap`, kein `Vec` —
        // der Schluessel ist der **klein**geschriebene Name (Regel von
        // `header_auswerten`), der Wert ein `String`. Ein Beispiel mit
        // `Vec<(String, String)>` haette den Test gruen scheinen lassen, ohne
        // dass er durchlief — der Uebersetzer haette es gemeldet, aber die
        // richtige Form haette man erst nachgeschlagen.
        assert_eq!(
            antwort.headers.get("x-bild").map(String::as_str),
            Some("ja"),
            "und der Kopf ist gelesen worden"
        );
    }

    /// DER HTTP-1.1-FUZZ: 100 000 Antworten ohne Panik.
    ///
    /// added 01.10.2026 16:5x, T5 („fuzz h2 und http je 100 000 ohne Panik").
    ///
    /// WARUM DIESER TEST IN `lib.rs` STEHT und nicht in `tests/`: `lese_antwort`
    /// ist **privat**, und es gibt keinen oeffentlichen Einstieg, der nur Bytes
    /// annimmt. Ein oeffentlicher Einstieg nur fuer den Test waere eine API, die
    /// niemand braucht — und eine, die man verschaerft, sobald jemand sie
    /// benutzt.
    ///
    /// DIE BEHAUPTUNG, und sie ist die einzige: **keine Panik** und **kein
    /// Aufhaenger**. Der zweite Teil ist nicht selbstlaeufig: ein HTTP-1.1-Parser,
    /// der bei einem Byte `0` endlos auf Nachschub wartet, haengt den Testlauf
    /// und damit die ganze Abnahme. Deshalb liest der Draht in Stuecken und
    /// gibt am Ende `Ok(0)` (EOF) zurueck — der Parser muss damit umgehen
    /// koennen, und wenn er es nicht tut, ist das der Fund.
    #[test]
    fn hunderttausend_http_antworten_ohne_panik() {
        let mut zufall = FuzzZufall::ab_start();
        let mut fehler = 0usize;
        let mut erfolge = 0usize;
        let anfrage = Request::get("https://beispiel.test", 443, "/pfad");
        for _ in 0..FUZZ_DURCHLAEUFE {
            let mut eingang: Vec<u8> = Vec::new();
            // DER ZUFALL AUF DER GANZEN ANTWORT. GEMESSEN, warum das ueberhaupt
            // etwas findet: eine zufaellige Bytefolge ist mit hoher
            // Wahrscheinlichkeit **keine** HTTP/1.1-Antwort (kein
            // Statuszeichen, keine CRLF). Sie laeuft also fast immer sofort in
            // einen Fehlerpfad — und die Fehlerpfade sind genau die, in denen
            // Panics stehen (Unterlaeufe bei Laengen, Zeigern auf leere
            // Zeilen).
            match zufall.naechste() % 3 {
                0 => {
                    // (a) Reine Zufallsbytes.
                    let laenge = (zufall.naechste() % 400) as usize + 1;
                    eingang = zufall.bytes(laenge);
                }
                1 => {
                    // (b) Eine gueltige Statuszeile, danach Zufall. Das ist der
                    // Fall, der die **Kopfzeile** prueft — `Content-Length`,
                    // `Transfer-Encoding: chunked`, `Set-Cookie` mit `=`.
                    eingang.extend_from_slice(b"HTTP/1.1 200 OK\r\n");
                    let laenge = (zufall.naechste() % 300) as usize;
                    for _ in 0..(laenge / 10) {
                        eingang.extend_from_slice(b"x-a: b\r\n");
                    }
                    eingang.extend_from_slice(&zufall.bytes(laenge % 17));
                    eingang.extend_from_slice(b"\r\n\r\n");
                }
                _ => {
                    // (c) Eine gueltige Statuszeile mit einer **behaupteten**
                    // Laenge und dann Zufall: das prueft, was beim Body-Zählen
                    // schiefgehen kann, wenn die Angabe nicht zum Strom passt.
                    eingang.extend_from_slice(b"HTTP/1.1 200 OK\r\n");
                    let behauptet = (zufall.naechste() % 5000) as u32;
                    eingang
                        .extend_from_slice(format!("Content-Length: {behauptet}\r\n").as_bytes());
                    eingang.extend_from_slice(b"\r\n");
                    let da = (zufall.naechste() % 5000) as usize;
                    eingang.extend_from_slice(&zufall.bytes(da.min(FUZZ_BYTES)));
                }
            }
            // DER DURCHLAUF, und er darf nur `Ok` oder `Err` liefern.
            let chunk = 1 + (zufall.naechste() % 64) as usize;
            let draht = FuzzDraht {
                rest: eingang,
                chunk,
                geschrieben: Vec::new(),
            };
            // added 01.10.2026 16:5x: `Limits::default()` statt einer eigenen
            // Liste. GEMESSEN: die eigene Liste benannte `koerper`, das Feld
            // heisst `gesamt` und `entpackt` — der Uebersetzer hat es gemeldet,
            // aber das richtige Argument waere geraten gewesen. `default()` ist
            // die **eine** Stelle, an der die Grenzen stehen.
            let limits = Limits::default();
            let frist = Frist::neu(std::time::Duration::from_millis(50));
            let mut leser = Leser::neu(Box::new(draht), &limits, &frist);
            match lese_antwort(&mut leser, &anfrage, &limits) {
                Ok(_) => erfolge += 1,
                Err(_) => fehler += 1,
            }
        }
        assert_eq!(
            erfolge + fehler,
            FUZZ_DURCHLAEUFE,
            "jeder Durchlauf endet mit Ok oder Err — nichts drittes"
        );
        // GEMESSEN, warum diese Zeile steht: `erfolge` ist nicht 0, also hat der
        // Fuzz **beide** Wege gesehen. Ohne diese Zeile koennte ein Fehler, der
        // alles ablehnt, als Erfolg durchgehen.
        assert!(
            erfolge > 0 && fehler > 0,
            "der Fuzz muss beide Wege sehen (erfolge {erfolge}, fehler {fehler})"
        );
    }

    /// DIE ZAHL, und sie ist eine Behauptung mit Beweis.
    ///
    /// added 01.10.2026 16:5x. `FUZZ_DURCHLAEUFE` ist eine Vorgabe, und eine
    /// Vorgabe, die man nicht einhaelt, ist eine Behauptung. GEMESSEN wird
    /// hier, dass die Schleife **genau** so oft laeuft wie gesagt — mit einem
    /// Zaehler, der neben der Schleife mitzaehlt. Das ist die Form, die man
    /// benutzt, wenn man einer Schrift glauben will statt einem Kommentar.
    #[test]
    fn der_fuzz_durchlaeuft_genau_hunderttausend_mal() {
        let mut zufall = FuzzZufall::ab_start();
        let mut gezaehlt = 0usize;
        for _ in 0..FUZZ_DURCHLAEUFE {
            let mut eingang: Vec<u8> = Vec::new();
            eingang.extend_from_slice(b"HTTP/1.1 200 OK\r\n");
            eingang.extend_from_slice(&zufall.bytes(20));
            eingang.extend_from_slice(b"\r\n\r\n");
            let draht = FuzzDraht {
                rest: eingang,
                chunk: 8,
                geschrieben: Vec::new(),
            };
            // added 01.10.2026 16:5x: `Limits::default()` statt einer eigenen
            // Liste. GEMESSEN: die eigene Liste benannte `koerper`, das Feld
            // heisst `gesamt` und `entpackt` — der Uebersetzer hat es gemeldet,
            // aber das richtige Argument waere geraten gewesen. `default()` ist
            // die **eine** Stelle, an der die Grenzen stehen.
            let limits = Limits::default();
            let frist = Frist::neu(std::time::Duration::from_millis(50));
            let mut leser = Leser::neu(Box::new(draht), &limits, &frist);
            let anfrage = Request::get("https://beispiel.test", 443, "/");
            let _ = lese_antwort(&mut leser, &anfrage, &limits);
            gezaehlt += 1;
        }
        assert_eq!(gezaehlt, 100_000, "die Vorgabe ist 100 000 Durchlaeufe");
        assert_eq!(
            gezaehlt, FUZZ_DURCHLAEUFE,
            "und die Konstante sagt dasselbe"
        );
    }

    fn antwort(code: u16, location: &str) -> Response {
        let mut r = Response::leer(code);
        r.headers
            .insert("location".to_string(), location.to_string());
        r
    }

    #[test]
    fn request_get_baut_richtige_bytes() {
        let req = Request::get("example.com", 80, "/index.html");
        let raw = req.to_bytes();
        let text = String::from_utf8(raw).expect("UTF-8");
        assert!(text.starts_with("GET /index.html HTTP/1.1\r\n"));
        assert!(text.contains("Host: example.com\r\n"));
        // added 30.09.2026, Welle 7: hier stand vorher
        // `assert!(text.contains("Connection: close\r\n"))`. Das ist
        // jetzt **falsch** und war der Grund, warum der Verbindungspool
        // nie eine Leitung zurueckbekam: RFC 9112 9.1 zwingt einen
        // Server, auf `close` hin zu schliessen.
        //
        // DIE GEGENPROBE STEHT DIREKT DARUNTER
        // (`connection_close_bleibt_moeglich`): kein Kopf heisst
        // nicht, dass der Aufrufer es nicht erzwingen kann.
        assert!(
            !text.contains("Connection:"),
            "in HTTP/1.1 darf kein Connection-Kopf stehen: {text}"
        );
        assert!(text.contains(&format!("User-Agent: {STANDARD_USER_AGENT}\r\n")));
        assert!(text.ends_with("\r\n\r\n"));
    }

    #[test]
    fn connection_close_bleibt_moeglich() {
        // added 30.09.2026, Welle 7: die Gegenprobe zu
        // `request_get_baut_richtige_bytes`. Wer `close` will, muss es
        // bekommen koennen — sonst waere die Aenderung von vorhin
        // nicht eine Korrektur, sondern das Wegnehmen einer Moeglichkeit.
        let req = Request::get("example.com", 80, "/x").header("connection", "close");
        let text = String::from_utf8(req.to_bytes()).expect("UTF-8");
        // DER GRUND FUER DEN KLEINBUCHSTABEN IN DER PRUEFUNG: die
        // Kopf-Schleife in `to_bytes` schreibt den Namen so, wie er in
        // der `HashMap` steht, und `header()` legt ihn mit
        // `to_lowercase()` ab. Im Kabel steht also `connection: close`.
        //
        // DAS IST RICHTIG SO UND KEIN FEHLER: RFC 9110 Abschnitt 5.1
        // sagt, dass die Namen der Felder "are case-insensitive" sind
        // und ein Empfänger sie als solche behandeln MUSS. Wer hier
        // auf Grossschreibung prueft, prueft die Schreibweise des
        // eigenen Codes und nicht das Protokoll.
        assert!(
            text.to_lowercase().contains("connection: close\r\n"),
            "{text}"
        );
    }

    #[test]
    fn request_get_mit_port_nicht_80() {
        let req = Request::get("localhost", 8080, "/api");
        let text = String::from_utf8(req.to_bytes()).expect("UTF-8");
        assert!(text.contains("Host: localhost:8080\r\n"));
    }

    #[test]
    fn request_absolute_uri_in_der_anfragezeile() {
        let req = Request::get("proxy.intern", 3128, "/ziel").absolut();
        let text = String::from_utf8(req.to_bytes()).expect("UTF-8");
        assert!(
            text.starts_with("GET http://proxy.intern:3128/ziel HTTP/1.1\r\n"),
            "Proxy-Form falsch: {text:?}"
        );
    }

    #[test]
    fn mit_basis_setzt_die_umleitungsbasis_und_nicht_die_anfragezeile() {
        // added 04.10.2026 04:7x, R69. **`mit_basis` war der EINZIGE Setter
        // fuer `Request::basis` und hatte null Aufrufer** (GEMESSEN mit
        // `narsil-tls/src/aufrufer_waechter.rs`, das die Fehlerbedingung in R69
        // auf **beide** eigenen Crates ausgedehnt hat). Diese zwei Messungen
        // sagen, **was** der Setter tut — und es ist **nicht** dasselbe fuer
        // beide Wege, was vorher **niemand** gemessen hatte.
        //
        // added 04.10.2026 04:7x: **(1) DIE UMLEITUNGSBASIS FOLGT DEM SETTER.**
        // `basis_von` ist die Eingabe der Aufloesung (`loese_auf(&basis,
        // location)`, Z. 5659/5660), und es liest `request.basis` **zuerst**.
        let req = Request::get("proxy.intern", 3128, "/ziel")
            .absolut()
            .mit_basis("https://basis.invalid/a/");
        assert_eq!(
            basis_von(&req),
            "https://basis.invalid/a/".to_string(),
            "die gesetzte Basis muss die Umleitungsbasis sein"
        );
        // added 04.10.2026 04:7x: **(2) DIE ANFRAGEZEILE FOLGT IHR NICHT**, und
        // das ist **richtig**: die absolute Form traegt das **Ziel**, das der
        // Server beantworten soll. GEMESSEN am Code (Z. 606-620): `uri` wird aus
        // `ist_https(self)`, `self.host_mit_port()` und `self.path` gebaut — die
        // Basis kommt dort **nicht** vor. Der Fall dafuer ist `<base href>` in
        // HTML: die Basis verschiebt **relative** `Location`-Angaben, nicht die
        // erste Anfrage.
        let text = String::from_utf8(req.to_bytes()).expect("UTF-8");
        assert!(
            text.starts_with("GET http://proxy.intern:3128/ziel HTTP/1.1\r\n"),
            "die Basis gehoert in die Umleitungsaufloesung, nicht in die Anfragezeile: {text:?}"
        );
        //
        // added 04.10.2026 04:7x: **(3) OHNE BASIS BLEIBT ES BEI `url_aus`** —
        // der Rueckfall, den `basis_von` seit R5g an `url_aus` delegiert. Ohne
        // diesen dritten Teil pruefte der Test nur den Setter, nicht den
        // Rueckfall, und ein Fehler in der Delegation bliebe unentdeckt.
        let ohne = Request::get("proxy.intern", 3128, "/ziel").absolut();
        assert_eq!(
            basis_von(&ohne),
            "http://proxy.intern:3128/ziel".to_string(),
            "ohne gesetzte Basis ist die Anfrage selbst die Basis"
        );
    }

    #[test]
    fn request_post_mit_body() {
        let body = b"key=value".to_vec();
        let req = Request::post("localhost", 80, "/submit", body.clone());
        let text = String::from_utf8(req.to_bytes()).expect("UTF-8");
        assert!(text.starts_with("POST /submit HTTP/1.1\r\n"));
        assert!(text.contains("Content-Length: 9\r\n"));
        assert!(text.ends_with("key=value"));
    }

    #[test]
    fn request_header_ueberschreibt() {
        let req = Request::get("h", 80, "/")
            .header("User-Agent", "Test/1.0")
            .header("Accept", "text/html");
        let text = String::from_utf8(req.to_bytes()).expect("UTF-8");
        assert!(text.contains("user-agent: Test/1.0\r\n"));
        assert!(text.contains("accept: text/html\r\n"));
        assert!(!text.contains(&format!("User-Agent: {STANDARD_USER_AGENT}")));
    }

    #[test]
    fn header_namen_werden_klein_und_mehrfach_kommasepariert() {
        let zeilen = vec![
            b"Content-Length: 5".to_vec(),
            b"CONTENT-TYPE: text/html".to_vec(),
            b"Set-Cookie: a=1".to_vec(),
            b"set-cookie: b=2".to_vec(),
        ];
        // added 30.09.2026, Welle 7 / N6: die dritte Ausgabe (alle
        // `Set-Cookie`-Werte) wird hier nicht gebraucht — der Test
        // prueft die Header-Map.
        let (map, _, _) = header_auswerten(&zeilen).expect("Header");
        assert_eq!(map.get("content-length").map(String::as_str), Some("5"));
        assert_eq!(
            map.get("content-type").map(String::as_str),
            Some("text/html")
        );
        assert_eq!(map.get("set-cookie").map(String::as_str), Some("a=1, b=2"));
    }

    #[test]
    fn header_ohne_doppelpunkt_ist_fehler() {
        let fehler = header_auswerten(&[b"kaputt ohne punkt".to_vec()]).unwrap_err();
        assert!(
            matches!(fehler, NetError::InvalidResponse(_)),
            "sollte InvalidResponse sein: {fehler}"
        );
    }

    #[test]
    fn response_status_erkennung() {
        let response = Response::leer(200);
        assert!(response.is_success());
        assert!(!response.is_redirect());
    }

    #[test]
    fn response_redirect() {
        let response = antwort(301, "http://example.com/new");
        assert!(response.is_redirect());
        assert_eq!(response.redirect_location(), Some("http://example.com/new"));
    }

    #[test]
    fn response_body_text() {
        let mut response = Response::leer(200);
        response.body = b"Hallo Welt".to_vec();
        assert_eq!(response.body_text(), Ok("Hallo Welt"));
    }

    #[test]
    fn content_encoding_ohne_gross_klein_und_leerzeichen() {
        // Der Weg, den eine ECHTE Antwort nimmt: Rohbytes -> Parser.
        // Nicht die Map von Hand fuellen, sonst prueft der Test nichts.
        let (mut headers, _, _) = header_auswerten(&[
            b"Content-Encoding:  GZip ".to_vec(),
            b"Content-Type: text/html".to_vec(),
        ])
        .expect("Header");
        let mut r = Response::leer(200);
        r.headers = headers.clone();
        assert_eq!(r.content_encoding(), Some("gzip".to_string()));
        assert!(r.body_komprimiert());
        match r.komprimierungs_fehler() {
            Some(NetError::Komprimiert(c)) => assert_eq!(c, "gzip"),
            andere => panic!("sollte Komprimiert sein, war {andere:?}"),
        }
        // identity heisst ausdruecklich: nicht komprimiert.
        headers.insert("content-encoding".to_string(), "identity".to_string());
        r.headers = headers;
        assert!(!r.body_komprimiert());
        assert!(r.komprimierungs_fehler().is_none());
    }

    #[test]
    fn redirect_absolut() {
        let (host, port, path) = url_teile("http://other.com:9090/new");
        assert_eq!(host, "other.com");
        assert_eq!(port, 9090);
        assert_eq!(path, "/new");
    }

    /// added 02.10.2026 03:3x, R5g. EINE RELATIVE `Location` BEHAELT DAS
    /// SCHEMA — und der Befund kam aus dem Netz, nicht aus diesem Test.
    ///
    /// GEMESSEN (`www.stackoverflow.com`, 02.10.2026 03:1x, ein `fetch`):
    ///     hop: stackoverflow.com:443 https=true  /         -> 302 "/questions"
    ///     hop: stackoverflow.com:80  https=false /questions -> 301 "https://…"
    ///     hop: stackoverflow.com:443 https=true  /questions -> 403
    /// Die dritte Zeile ist das Problem: der Sprung von `443/https` auf
    /// `80/http` und wieder zurueck. Der Klartext-Hop ist im Pool als
    /// `stackoverflow.com:80 tls=false` sichtbar geworden.
    ///
    /// DER WEG ZUM ROTWERDEN: in `basis_von` wieder fest `http://` schreiben.
    #[test]
    fn redirect_relativ_behaelt_das_schema() {
        let r = Request::get("beispiel.test", 443, "/a/seite.html");
        assert_eq!(basis_von(&r), "https://beispiel.test/a/seite.html");
        // Und der ganze Sprung, damit nicht nur die Basis stimmt:
        let ziel = umleitung_ziel(&r, 302, "/fragen").expect("Sprung");
        assert_eq!(ziel.host, "beispiel.test");
        assert_eq!(
            ziel.port, 443,
            "ein relativer Sprung darf den Port nicht auf 80 legen"
        );
        assert!(ist_https(&ziel), "und das Schema nicht auf http");
        // added 02.10.2026 03:3x: die Gegenprobe — **mit** Schema im
        // `Location` darf der Sprung sehr wohl das Schema wechseln.
        let r2 = Request::get("beispiel.test", 443, "/");
        let ziel2 = umleitung_ziel(&r2, 302, "http://beispiel.test/plain").expect("Sprung");
        assert!(!ist_https(&ziel2), "absolutes http-Ziel bleibt http");
    }

    #[test]
    fn redirect_relativ_aufgeloest() {
        let basis = "http://beispiel.de/a/b/seite.html";
        assert_eq!(
            loese_auf(basis, "neu").unwrap(),
            "http://beispiel.de/a/b/neu"
        );
        assert_eq!(
            loese_auf(basis, "../neu").unwrap(),
            "http://beispiel.de/a/neu"
        );
        assert_eq!(
            loese_auf(basis, "./hier").unwrap(),
            "http://beispiel.de/a/b/hier"
        );
        assert_eq!(
            loese_auf(basis, "/wurzel").unwrap(),
            "http://beispiel.de/wurzel"
        );
        assert_eq!(
            loese_auf(basis, "?a=1").unwrap(),
            "http://beispiel.de/a/b/seite.html?a=1"
        );
        assert_eq!(
            loese_auf(basis, "#anker").unwrap(),
            "http://beispiel.de/a/b/seite.html#anker"
        );
        assert_eq!(
            loese_auf(basis, "").unwrap(),
            "http://beispiel.de/a/b/seite.html"
        );
        assert_eq!(
            loese_auf(basis, "http://anders.de/x").unwrap(),
            "http://anders.de/x"
        );
        // `//host/pfad` wechselt den Host, das Schema bleibt.
        assert_eq!(
            loese_auf(basis, "//cdn.de/b.png").unwrap(),
            "http://cdn.de/b.png"
        );
        // `mailto:` ist kein Netz.
        assert_eq!(loese_auf(basis, "mailto:a@b.de").unwrap(), "mailto:a@b.de");
        // Leere Basis ist ein benannter Fehler, keine erfundene Adresse.
        assert!(loese_auf("", "b").is_err());
    }

    #[test]
    fn normalisiere_punkt_punkte() {
        assert_eq!(normalisiere("/a/b/../c"), "/a/c");
        assert_eq!(normalisiere("/a/./b"), "/a/b");
        assert_eq!(normalisiere("/../a"), "/a");
        assert_eq!(normalisiere("/a/b/"), "/a/b/");
        // RFC 3986 §5.2.4 Schritt 2E: ein abschliessendes Punktesegment
        // wird durch `/` ersetzt, nicht weggelassen.
        assert_eq!(normalisiere("/a/b/."), "/a/b/");
        assert_eq!(normalisiere("/a/b/.."), "/a/");
    }

    #[test]
    fn method_strings() {
        assert_eq!(Method::Get.as_str(), "GET");
        assert_eq!(Method::Head.as_str(), "HEAD");
        assert_eq!(Method::Post.as_str(), "POST");
    }

    /// added 02.10.2026 23:1x, R43. **DER VORTRAGSTEST.** Ein `101` auf eine
    /// Anfrage, die **nie** ein `Upgrade` geschickt hat, darf die Verbindung
    /// **nicht** in den Pool legen. GEMESSEN war vorher `KeepAlive::Bleibt`, und
    /// der Aufrufer ruft genau damit `pool::lege_ab` (Zeile 3526).
    ///
    /// added 02.10.2026 23:1x: **DREI KOPFZEILEN-VARIANTEN**, weil ein Server
    /// frei ist, wie er es schreibt: `Connection: upgrade` (der Regelfall),
    /// **gar kein** `Connection`-Kopf (HTTP/1.1-Default ist „bleibt“ — das ist
    /// der Fall, der die Regel wirklich braucht) und `Connection: Upgrade` mit
    /// anderer Schreibweise. Ohne `Connection` war vorher `Bleibt` ganz sicher.
    #[test]
    fn ein_101_schliesst_die_verbindung() {
        let anfrage = Request::get("h", 443, "/");
        for kopfwert in ["upgrade", "Upgrade", "keep-alive, upgrade"] {
            let mut kopf = HashMap::new();
            kopf.insert("connection".to_string(), kopfwert.to_string());
            assert_eq!(
                verbindungswunsch(&anfrage, &kopf, "HTTP/1.1", 101),
                KeepAlive::Schliesst,
                "Connection: {kopfwert} bei 101"
            );
            // added 02.10.2026 23:2x: **OHNE** `Connection`-Kopf. Das ist der
            // wichtigere Fall: nach HTTP/1.1 gilt dann „persistent“, also war
            // die Leitung hier frueher mit Sicherheit im Pool.
            let leer = HashMap::new();
            assert_eq!(
                verbindungswunsch(&anfrage, &leer, "HTTP/1.1", 101),
                KeepAlive::Schliesst,
                "gar kein Connection-Kopf bei 101"
            );
        }
    }

    /// added 02.10.2026 23:2x, R43. **DER GEGENFALL, und er hat drei Teile**,
    /// weil die Regel sonst zu breit waere und echte Verbindungen kapselte:
    /// * `426 Upgrade Required` — der Server **bewirbt** nur ein Protokoll
    ///   (RFC 9110 7.8: „A server MAY send an Upgrade header field in any other
    ///   response to advertise that it implements support for upgrading“). Die
    ///   Verbindung bleibt HTTP, also bleibt sie im Pool.
    /// * ein `Upgrade`-Kopf bei `200` — dieselbe Werbung, dieselbe Folge.
    /// * `100 Continue` — ein Zwischenstatus, kein Protokollwechsel. Hmm: es
    ///   gehoert zu den 1xx, und `ohne_body` ist fuer 1xx wahr. Fuer den Pool
    ///   darf es aber nicht schliessen, sonst bricht jede Expect-Kette ab.
    #[test]
    fn andere_status_behalten_ihre_verbindung() {
        let anfrage = Request::get("h", 443, "/");
        let mut kopf = HashMap::new();
        kopf.insert("connection".to_string(), "upgrade".to_string());
        kopf.insert("upgrade".to_string(), "websocket".to_string());
        for code in [200u16, 426, 100, 204, 304] {
            assert_eq!(
                verbindungswunsch(&anfrage, &kopf, "HTTP/1.1", code),
                KeepAlive::Bleibt,
                "Status {code} darf die Verbindung nicht schliessen"
            );
        }
    }

    /// added 02.10.2026 23:2x, R43. **UND `Connection: close` BLEIBT**
    /// schliessens, unabhaengig vom Status — sonst waere die neue Regel zu
    /// stark und die Abnahme flogte bei jedem `Connection: close`.
    #[test]
    fn close_bleibt_close() {
        let anfrage = Request::get("h", 443, "/");
        let mut kopf = HashMap::new();
        kopf.insert("connection".to_string(), "close".to_string());
        for code in [200u16, 101, 304] {
            assert_eq!(
                verbindungswunsch(&anfrage, &kopf, "HTTP/1.1", code),
                KeepAlive::Schliesst
            );
        }
    }

    #[test]
    fn keep_alive_richtet_sich_nach_der_protokollversion() {
        // GEMESSEN 29.09.2026 23:18: HTTP/1.1 ohne `Connection` heisst
        // nach RFC 9112 §9.6 „persistent", HTTP/1.0 heisst „wird
        // geschlossen". Der Test stand vorher nur in der einen Richtung.
        // added 02.10.2026 23:1x, R43: die `200` ist das neue Argument —
        // `verbindungswunsch` entscheidet seit R43 auch ueber den Status
        // (`101` heisst „nicht mehr HTTP“, siehe dort). Diese Faelle hier
        // sind gewoehnliche 200er.
        let mut kopf = HashMap::new();
        assert_eq!(
            verbindungswunsch(&Request::get("h", 80, "/"), &kopf, "HTTP/1.1", 200),
            KeepAlive::Bleibt,
            "HTTP/1.1 ohne Header ist persistent"
        );
        assert_eq!(
            verbindungswunsch(&Request::get("h", 80, "/"), &kopf, "HTTP/1.0", 200),
            KeepAlive::Schliesst,
            "HTTP/1.0 ohne Header wird geschlossen"
        );
        // Ein expliziter `close` schlaegt die Version.
        kopf.insert("connection".to_string(), "close".to_string());
        assert_eq!(
            verbindungswunsch(&Request::get("h", 80, "/"), &kopf, "HTTP/1.1", 200),
            KeepAlive::Schliesst
        );
        // Gross-/Kleinschreibung und Grossbuchstaben-Zwischenraum.
        kopf.insert("connection".to_string(), "Close".to_string());
        assert_eq!(
            verbindungswunsch(&Request::get("h", 80, "/"), &kopf, "HTTP/1.1", 200),
            KeepAlive::Schliesst
        );
        // Und `keep-alive` ist auch bei 1.0 ein Wunsch zum Behalten.
        kopf.insert("connection".to_string(), "Keep-Alive".to_string());
        assert_eq!(
            verbindungswunsch(&Request::get("h", 80, "/"), &kopf, "HTTP/1.0", 200),
            KeepAlive::Bleibt
        );
    }

    #[test]
    fn keep_alive_hat_einen_lesbaren_namen() {
        // Ohne `Debug`-Text sieht man in einem Fehlerbericht nur eine
        // Zahl. Der Name sagt mehr als die Variante.
        assert_eq!(format!("{:?}", KeepAlive::Schliesst), "Schliesst");
        assert_eq!(format!("{:?}", KeepAlive::Bleibt), "Bleibt");
    }

    #[test]
    fn error_display_nennt_ursache() {
        let faelle: Vec<(NetError, &str)> = vec![
            (NetError::ZeitlimitLesen, "Zeitlimit"),
            (
                NetError::Tls("Kette(KeinAussteller)".into()),
                "KeinAussteller",
            ),
            (NetError::HeaderZuGross { grenze: 65536 }, "65536"),
            (NetError::BodyZuGross { grenze: 10 }, "10"),
            (NetError::KeinHttp("SSH-2.0".into()), "SSH-2.0"),
            (
                NetError::VerbindungAbgebrochen {
                    erwartet: 10,
                    gelesen: 3,
                },
                "3 von 10",
            ),
            (NetError::Komprimiert("gzip".into()), "gzip"),
        ];
        for (fehler, muesste) in faelle {
            let text = format!("{fehler}");
            assert!(
                text.contains(muesste),
                "Meldung {text:?} enthaelt {muesste:?} nicht"
            );
        }
    }
    /// N1: `https://` ergibt Port 443 — der Weg, ueber den eine
    /// Umleitung von `http://` nach `https://` ueberhaupt zu TLS fuehrt.
    ///
    /// ECHTER FEHLER, am 30.09.2026 beim Verdrahten gefunden: `url_teile`
    /// hat `://` abgeschnitten und immer 80 gesetzt, auch bei `https://`.
    /// Damit kam eine https-Umleitung als http-Anfrage auf Port 80 an.
    #[test]
    fn https_im_url_ergibt_port_443() {
        assert_eq!(
            url_teile("https://example.com/"),
            ("example.com".into(), 443, "/".into())
        );
        assert_eq!(
            url_teile("http://example.com/"),
            ("example.com".into(), 80, "/".into())
        );
        // Ein ausgeschriebener Port gewinnt wie bisher.
        assert_eq!(
            url_teile("https://example.com:8443/x"),
            ("example.com".into(), 8443, "/x".into())
        );
    }

    /// N1: `ist_https` und `host_ohne_schema`.
    ///
    /// Der zweite Teil ist der Fund beim Verdrahten: die alte Schicht
    /// pruefte `host.starts_with("https://")`, benutzte `host` aber
    /// sonst direkt als Adresse. Ein Host mit Schema waere dann
    /// `https://example.com` in `to_socket_addrs`, und der Benutzer
    /// sahe "Host nicht aufloesbar" statt einer Seite.
    #[test]
    fn https_entscheidung_und_schema_abschneiden() {
        let mut r = Request::get("example.com", 443, "/");
        assert!(ist_https(&r), "Port 443 heisst HTTPS");
        r = Request::get("example.com", 80, "/");
        assert!(!ist_https(&r), "Port 80 ist HTTP");
        r = Request::get("https://example.com", 80, "/");
        assert!(ist_https(&r), "auch das ausgeschriebene Schema zaehlt");

        assert_eq!(host_ohne_schema("https://example.com"), "example.com");
        assert_eq!(host_ohne_schema("http://example.com"), "example.com");
        assert_eq!(host_ohne_schema("example.com"), "example.com");
        // Und der IPv6-Fall mit Klammern bleibt unberuehrt.
        assert_eq!(host_ohne_schema("https://[::1]"), "[::1]");
    }
}

/// added 02.10.2026 10:2x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// Runde 8 „UNHACKBAR Teil 2", S8: der `Referer` nach der Richtlinie.
///
/// added 02.10.2026 10:2x: **DIE VEKTOREN SIND DIE BEISPIELE DER SPEZIFIKATION**,
/// wörtlich aus Abschnitt 3.7 (die vier Sätze mit „would send a Referer header
/// with a value of …“ bzw. „would send no Referer header“) und nicht von mir
/// erfunden. Ein selbstgebauter Vektor prüft, was ich für richtig halte; die
/// Beispiele der Spezifikation prüfen, was sie vorschreibt.
#[cfg(test)]
mod referrer_tests {
    use super::*;

    /// added 02.10.2026 10:3x: **DIE VIER BEISPIELE AUS 3.7**, wörtlich:
    /// „If a document at https://example.com/page.html sets a policy of
    /// "strict-origin-when-cross-origin", then navigations to
    /// https://example.com/not-page.html would send a Referer header with a
    /// value of https://example.com/page.html. Navigations from that same page
    /// to https://not.example.com/ would send a Referer header with a value of
    /// https://example.com/. Navigations from that same page to
    /// http://not.example.com/ would send no Referer header.“
    #[test]
    fn die_vier_beispiele_aus_abschnitt_3_7() {
        const QUELLE: &str = "https://example.com/page.html";
        let p = ReferrerPolitik::HerkunftWennFremdStreng;
        assert_eq!(
            referrer_berechnen(p, QUELLE, "https://example.com/not-page.html").as_deref(),
            Some("https://example.com/page.html"),
            "1: gleiche Herkunft, volle URL"
        );
        assert_eq!(
            referrer_berechnen(p, QUELLE, "https://not.example.com/").as_deref(),
            Some("https://example.com/"),
            "2: fremde Herkunft, nur die Herkunft"
        );
        assert_eq!(
            referrer_berechnen(p, QUELLE, "http://not.example.com/"),
            None,
            "3: von https auf http, kein Referer"
        );
        // added 02.10.2026 10:3x: **DAS VIERTE** Beispiel, woertlich: „fetches a
        // module script at https://script.example.com which then fetches a
        // descendant script at http://descendant.example.com, the request to
        // the descendant script would send no Referer header."
        assert_eq!(
            referrer_berechnen(
                p,
                "https://script.example.com",
                "http://descendant.example.com"
            ),
            None,
            "4: Modulkette ueber die Herkunftsgrenze"
        );
    }

    /// added 02.10.2026 10:4x: **RFC 9110 10.1.3, WOERTLICH**: „A user agent
    /// **MUST NOT** include the fragment and userinfo components of the URI
    /// reference, if any, when generating the Referer field value.“ Das ist die
    /// einzige **MUST NOT** in beiden Normen, und sie wird hier an beiden
    /// Bestandteilen einzeln geprueft — ein Test mit beiden zugleich waere gruen,
    /// auch wenn nur einer abgeschnitten wuerde.
    #[test]
    fn fragment_und_userinfo_gehoeren_nie_in_den_referer() {
        for politik in [
            ReferrerPolitik::HerkunftWennFremdStreng,
            ReferrerPolitik::Voll,
            ReferrerPolitik::NurHerkunft,
        ] {
            let wert = referrer_berechnen(
                politik,
                "https://geheim:pw@example.com/seite.html#anker",
                "https://example.com/ziel",
            )
            .unwrap_or_default();
            assert!(!wert.contains('#'), "kein Fragment im Wert: {wert}");
            assert!(!wert.contains("geheim"), "keine Userinfo im Wert: {wert}");
            assert!(!wert.contains("pw@"), "kein Kennwort im Wert: {wert}");
        }
        assert_eq!(
            referrer_berechnen(
                ReferrerPolitik::Voll,
                "https://example.com/a/b?x=1#f",
                "https://example.com/z"
            )
            .as_deref(),
            Some("https://example.com/a/b?x=1"),
            "die Abfrage bleibt, das Fragment nicht"
        );
    }

    /// added 02.10.2026 10:4x: **DIE DREI FAELLE, DIE ALLE AUSSEHEN WIE
    /// „Herabstufen" UND ES NICHT SIND** — der Unterschied zwischen `origin`
    /// und `strict-origin` ist der ganze Inhalt von 3.4 gegen 3.5, und er ist an
    /// drei Richtlinien zu sehen. Wer nur eine davon prueft, haelt die anderen
    /// drei fuer gleich.
    #[test]
    fn nur_die_strict_richtlinien_verweigern_beim_herabstufen() {
        const Q: &str = "https://example.com/seite.html";
        const Z: &str = "http://example.com/ziel";
        assert_eq!(
            referrer_berechnen(ReferrerPolitik::NurHerkunft, Q, Z).as_deref(),
            Some("https://example.com/"),
            "origin schickt die Herkunft auch beim Herabstufen"
        );
        assert_eq!(
            referrer_berechnen(ReferrerPolitik::HerkunftWennFremd, Q, Z).as_deref(),
            Some("https://example.com/seite.html"),
            "origin-when-cross-origin schickt die volle URL"
        );
        assert_eq!(
            referrer_berechnen(ReferrerPolitik::Voll, Q, Z).as_deref(),
            Some("https://example.com/seite.html"),
            "unsafe-url auch"
        );
        // added 02.10.2026 10:5x: **UND UMGEKEHRT: DIE DREI STRICT-DAFUER**
        assert_eq!(
            referrer_berechnen(ReferrerPolitik::NurHerkunftStreng, Q, Z),
            None,
            "strict-origin schickt nichts"
        );
        assert_eq!(
            referrer_berechnen(ReferrerPolitik::HerkunftWennFremdStreng, Q, Z),
            None,
            "strict-origin-when-cross-origin auch nicht"
        );
        assert_eq!(
            referrer_berechnen(ReferrerPolitik::KeineBeiHerabstufung, Q, Z),
            None,
            "no-referrer-when-downgrade auch nicht"
        );
        // added 02.10.2026 10:5x: **VON UNSICHER NACH SICHER IST KEIN
        // HERABSTUFEN**, und das ist der Fall, den eine Regel „wenn die Schemas
        // verschieden sind" falsch haette: `http` nach `https` schickt sehr wohl
        // einen Referer, weil das **nicht** das Herabstufen ist, sondern das
        // gegenteilige Problem — 3.7 nennt fuer „strict-origin-when-cross-origin"
        // nur den Fall „vertraut nach unvertraut".
        assert_eq!(
            referrer_berechnen(
                ReferrerPolitik::HerkunftWennFremdStreng,
                "http://example.com/a",
                "https://example.com/b"
            )
            .as_deref(),
            Some("http://example.com/"),
            "von unvertraut nach vertraut wird gesendet"
        );
    }

    /// added 02.10.2026 10:5x: **11.1 UND 4.1, DIE REGEL ÜBER UNBEKANNTE
    /// WERTE**: „unknown policy values will be **ignored**, and when multiple
    /// sources specify a referrer policy, the value of the **latest** one will be
    /// used.“ Wörtlich geprüft an drei Köpfen, denn an jedem von ihnen könnte
    /// der Parser etwas anderes tun.
    #[test]
    fn unbekannte_werte_werden_ueberlesen_und_das_letzte_gewinnt() {
        assert_eq!(
            referrer_richtlinie(Some("unsicher")),
            REFERRER_VORGABE,
            "GEMESSEN: ein unbekannter Wert wird ueberlesen, nicht zur Vorgabe"
        );
        assert_eq!(
            referrer_richtlinie(Some("unsicher, origin")),
            ReferrerPolitik::NurHerkunft,
            "das bekannte danach zaehlt"
        );
        assert_eq!(
            referrer_richtlinie(Some("origin, unsicher")),
            ReferrerPolitik::NurHerkunft,
            "und ein unbekanntes danach **nimmt es nicht rueckgaengig** (11.1)"
        );
        // added 02.10.2026 10:6x: **OHNE KOPF UND MIT LEERER ZEILE**, denn 3.9
        // sagt: „The empty string "" corresponds to no referrer policy, causing
        // a fallback to … the default referrer policy.“ Beide Faelle fuehren also
        // auf dieselbe Vorgabe — und **nicht** auf `no-referrer`.
        assert_eq!(referrer_richtlinie(None), REFERRER_VORGABE, "kein Kopf");
        assert_eq!(
            referrer_richtlinie(Some("")),
            REFERRER_VORGABE,
            "leerer Kopf"
        );
        assert_eq!(
            referrer_richtlinie(Some("  ")),
            REFERRER_VORGABE,
            "nur Leerraum"
        );
        // added 02.10.2026 10:6x: **UND GROSS-/KLEINSCHREIBUNG**, weil die
        // ABNF in 4.1 die Werte als Zeichenketten in Kleinbuchstaben schreibt und
        // ein Kopf koennteanders — wer case-insensitiv vergleicht, nimmt eine
        // Angabe an, die die Spezifikation so nicht kennt. GEMESSEN ist die
        // Richtung hier bewusst **streng**: `Origin` ist kein Richtwert.
        assert_eq!(
            referrer_richtlinie(Some("Origin")),
            REFERRER_VORGABE,
            "GEMESSEN: die ABNF schreibt klein, also zaehlt `Origin` nicht"
        );
        assert_eq!(
            referrer_richtlinie(Some("no-referrer")),
            ReferrerPolitik::Keine
        );
        assert_eq!(
            referrer_richtlinie(Some("no-referrer, unsafe-url")),
            ReferrerPolitik::Voll
        );
    }

    /// added 02.10.2026 10:6x: **GLEICHE HERKUNFT UND DER STANDARDPORT**, und
    /// das ist der Fehler, der bei einem Textvergleich **immer** passiert:
    /// `https://example.com` und `https://example.com/` sind dieselbe Herkunft,
    /// `https://example.com:443` auch, und `https://example.com:8443` nicht.
    #[test]
    fn der_standardport_ist_kein_unterschied() {
        const P: ReferrerPolitik = ReferrerPolitik::HerkunftWennFremdStreng;
        for ziel in [
            "https://example.com/",
            "https://example.com:443/",
            "https://example.com/a/b",
        ] {
            assert_eq!(
                referrer_berechnen(P, "https://example.com/q", ziel).as_deref(),
                Some("https://example.com/q"),
                "gleiche Herkunft trotzdem: {ziel}"
            );
        }
        assert_eq!(
            referrer_berechnen(P, "https://example.com/q", "https://example.com:8443/").as_deref(),
            Some("https://example.com/"),
            "ein anderer Port ist eine fremde Herkunft"
        );
        assert_eq!(
            referrer_berechnen(P, "http://example.com/q", "https://example.com/z").as_deref(),
            Some("http://example.com/"),
            "das Schema gehoert zur Herkunft"
        );
    }

    /// added 02.10.2026 10:7x: **DER EINSTIEG UND SEINE BEIDEN SEITEN**:
    /// [`referrer_aus_kopf`] nimmt den **Antwortkopf** und wendet genau das an,
    /// was die beiden obigen Funktionen getrennt tun. Der Test prueft beide
    /// Richtungen, weil eine Funktion, die nur einen der beiden Schritte macht,
    /// in **beiden** Richtungen falsch liegt und trotzdem „grün“ sein kann.
    #[test]
    fn der_einstieg_verbindet_kopf_und_ziel() {
        assert_eq!(
            referrer_aus_kopf(
                Some("no-referrer"),
                "https://example.com/a",
                "https://example.com/b"
            ),
            None,
            "der Server darf die Richtlinie setzen"
        );
        assert_eq!(
            referrer_aus_kopf(
                Some("unsafe-url"),
                "https://example.com/a",
                "http://example.com/b"
            )
            .as_deref(),
            Some("https://example.com/a"),
            "auch ueber den Angriff hinweg, wenn der Server es so sagt"
        );
        assert_eq!(
            referrer_aus_kopf(None, "https://example.com/a", "https://not.example.com/b")
                .as_deref(),
            Some("https://example.com/"),
            "ohne Kopf gilt die Vorgabe"
        );
        // added 02.10.2026 10:7x: **UND EIN KOPF OHNE SCHEMA**, weil das im
        // Betrieb vorkommt (ein Antwortkopf, den ein Server ohne TLS an den
        // Client schickt) und die Regel „potentially trustworthy“ daran haengt:
        // ohne Schema ist nichts vertraut, also wird **nichts** gesendet, wenn
        // das Ziel unvertraut ist. GEMESSEN ist das hier der Grund fuer die
        // fehlertolerante Zerlegung.
        assert_eq!(
            referrer_aus_kopf(None, "example.com/a", "https://example.com/b"),
            None,
            "ohne Schema in der Quelle wird gar nichts geschickt — GEMESSEN: ohne\n            diese Regel kam `://example.com/` heraus, und das ist kein gueltiger\n            Feldwert (RFC 9110 10.1.3: absolute-URI / partial-URI)"
        );
    }
    /// added 02.10.2026 11:2x: **DIE VIER ZEILEN DER SCHLEIFE**, als Test am
    /// echten Weg: [`referer_setzen`] ist die Funktion, die der Betrieb aufruft,
    /// und der Test baut eine `Request` und sieht den Kopf. GEPRUEFT WERDEN
    /// alle drei Faelle — setzen, entfernen, und **nichts erfinden**, wenn es
    /// keine Quelle gab; der dritte ist der, der beim Lesen der Schleife wie ein
    /// Fehler aussieht und keiner ist.
    #[test]
    fn der_sprung_setzt_oder_entfernt_den_referer() {
        let mut anfrage = Request::get("https://quelle.example.com/b", 443, "/b");
        // added 02.10.2026 11:2x: **GLEICHE HERKUNFT** — das Dokument war auf
        // derselben Herkunft, also bleibt die volle URL stehen.
        //
        // added 02.10.2026 11:4x: **UND DIESER VEKTOR WAR ZUERST FALSCH**,
        // gemessen am roten Lauf: das Ziel hiess `ziel.example.com`, und der
        // Kommentar daneben sagte „gleiche Herkunft“. Das sind **verschiedene**
        // Herkunften; die Richtlinie schickte deshalb zu Recht nur
        // `https://quelle.example.com/`. Der Widerspruch zwischen Kommentar und
        // Daten fiel erst auf, als der Test laufen durfte (siehe den Grund in der
        // Modul-Doku). Kommentar und Daten muessen dieselbe Aussage machen —
        // sonst sieht der Vektor geprueft aus und prueft etwas anderes.
        referer_setzen(
            &mut anfrage,
            Some("https://quelle.example.com/a"),
            ReferrerPolitik::HerkunftWennFremdStreng,
        );
        assert_eq!(
            anfrage.headers.get("referer").map(String::as_str),
            Some("https://quelle.example.com/a"),
            "gleiche Herkunft: die volle URL"
        );
        // added 02.10.2026 11:2x: **FREMDE HERKUNFT** — es bleibt die Herkunft
        // des Dokuments, und zwar nach der **Namen** der Richtlinie, nicht nach
        // dem vorherigen Sprung. Das ist der Unterschied, an dem eine Kette
        // A → B → C von einer Kette C → B → C nicht zu unterscheiden waere.
        anfrage
            .headers
            .insert("referer".to_string(), "https://falsch.example/".to_string());
        referer_setzen(
            &mut anfrage,
            Some("https://quelle.example.com/a"),
            ReferrerPolitik::NurHerkunft,
        );
        assert_eq!(
            anfrage.headers.get("referer").map(String::as_str),
            Some("https://quelle.example.com/"),
            "fremde Herkunft: nur die Herkunft"
        );
        // added 02.10.2026 11:2x: **HERABSTUFEN** — der Kopf muss **verschwinden**,
        // und zwar auch dann, wenn vorher einer drinstand. Genau das prueft
        // `remove`: ein stehen gebliebener `Referer` waere der Fehler.
        anfrage
            .headers
            .insert("referer".to_string(), "https://alt.example/".to_string());
        let mut anfrage2 = Request::get("http://ziel.example.com/b", 80, "/b");
        referer_setzen(
            &mut anfrage2,
            Some("https://quelle.example.com/a"),
            ReferrerPolitik::HerkunftWennFremdStreng,
        );
        assert_eq!(
            anfrage2.headers.get("referer"),
            None,
            "beim Herabstufen wird der Kopf entfernt, nicht gekuerzt"
        );
        // added 02.10.2026 11:3x: **OHNE DOKUMENT**: der Benutzer hat die
        // Adresse getippt. Es wird **kein** Kopf erfunden — auch nicht einer,
        // der vorher schon drin war.
        let mut anfrage3 = Request::get("https://ziel.example.com/b", 443, "/b");
        referer_setzen(&mut anfrage3, None, ReferrerPolitik::Voll);
        assert_eq!(
            anfrage3.headers.get("referer"),
            None,
            "ohne Quelle wird nichts erfunden"
        );
    }
}

/// added 02.10.2026 12:2x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// **DIE ABNAHME GEGEN DEN EIGENEN SERVER**, und sie ist die, die
/// `AUTONOM.txt` Zeile 712 verlangt („Je Punkt Tests + Abnahme gegen eigenen
/// narsil-server (examples)“) — und die ich fuer S5 und S8 bis hierher **nicht**
/// geliefert hatte.
///
/// added 02.10.2026 12:2x: **WARUM HIER UND NICHT GEGEN `narsil-server/examples`.**
/// GEMESSEN: `crates/narsil-server/examples/` enthaelt `https_demo.rs` und
/// `https_h2_demo.rs`, aber der Client, mit dem man sie abnimmt, muss **mein**
/// `narsil-net` sein — und dafuer gibt es in meiner Dateiliste keine Datei, in die
/// ein solches Programm passt: `narsil-net/tests/` gehoert einer anderen Rolle, ein
/// Beispiel unter `narsil-net/examples/` nicht zu meiner Liste. Der Weg, der bleibt
/// und den ich hier gehe, ist derselbe **Weg durch dieselben Zeilen**: ein echter
/// TLS-Handshake mit `narsil_tls::server::TlsServer` (meine Crate), ein echter
/// Sockel, und `fetch_mit_speicher` (mein Abrufweg). Was damit **nicht** gemessen
/// ist, sage ich am Ende dieses Abschnitts.
///
/// added 02.10.2026 12:2x: **UND DIE DATENKOMMEN AUS `narsil-tls/testdaten`** —
/// `testschluessel_p256.hex` und `testzertifikat.der`. Die werden **gelesen**,
/// nicht geschrieben; der Pfad ist `env!("CARGO_MANIFEST_DIR")/../narsil-tls/
/// testdaten`, und das ist genau die Form, die `tests/alpn_client.rs:407` benutzt
/// (GEMESSEN dort auch die Begruendung: `narsil-net` hat kein eigenes
/// `testdaten`, und ein eigenes zu bauen waere eine zweite Wahrheit).
/// added 02.10.2026 15:0x UTC (Projektlabel 02.10.2026), Rolle TLS: UND ZWEI
/// ENDE-ZU-ENDE-TESTS FUER DIE HERKUNFTSGRENZE, gemessen am 02.10.2026 — ein
/// Sprung auf eine **andere** Herkunft verliert `Authorization` und `Cookie`,
/// ein Sprung **innerhalb** derselben behaelt beide. Ohne den zweiten Test
/// waere die Regel „pauschal entfernen“ gruen und kaputte Anmeldungen blieben
/// unentdeckt.
#[cfg(test)]
mod abnahme_end_to_end {
    use super::*;

    /// **DER SPRUNG ÜBER DIE HERKUNFTSGRENZE**, und dieser Test misst zuerst ein
    /// **Leck**, bevor irgendetwas am Code geändert wird.
    ///
    /// added 02.10.2026 15:0x: DIE NORM, RFC 9110 15.4, Schritt 3, wörtlich:
    /// „Consider removing header fields that were not automatically generated by
    /// the implementation (i.e., those present in the request because they were
    /// added by the calling context) where there are security implications; this
    /// includes **but is not limited to Authorization and Cookie**.“ Schritt 5
    /// nennt dieselben Felder als „resource-specific“.
    ///
    /// added 02.10.2026 15:0x: **WARUM ZWEI SERVER:** die Herkunftsgrenze braucht
    /// zwei Herkünfte, und Herkunft ist Schema + Host + **Port** (Abschnitt 2 der
    /// Spezifikation „Referrer Policy“). Zwei Ports auf `localhost` sind zwei
    /// Herkünfte — das ist der kürzeste Weg, die Grenze ohne DNS zu bauen.
    #[test]
    fn der_sprung_ueber_die_herkunftsgrenze_verliert_authorization_und_cookie() {
        let speicher = vertrauensspeicher();
        let (ziel_port, ziel_koepfe, _v1) = server_starten(vec![
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_string(),
        ]);
        let (quell_port, quell_koepfe, _v2) = server_starten(vec![format!(
            "HTTP/1.1 302 Found\r\nLocation: https://localhost:{ziel_port}/ziel\r\nContent-Length: 0\r\n\r\n"
        )]);
        let mut anfrage = Request::get("https://localhost", quell_port, "/");
        anfrage = anfrage.header("authorization", "Bearer geheim-12345");
        anfrage = anfrage.header("cookie", "sitzung=geheim-67890");
        let mut sitzung = Aufzeichnung::default();
        let antwort =
            fetch_mit_speicher(&anfrage, Limits::default(), Some(&mut sitzung), &speicher)
                .expect("der Abruf muss die Kette beenden");
        assert_eq!(antwort.status.code, 200, "die Kette endet bei 200");
        let _erster = quell_koepfe.recv_timeout(FRIST).expect("erster Kopf");
        let zweiter = ziel_koepfe.recv_timeout(FRIST).expect("zweiter Kopf");
        let kopf = zweiter.to_lowercase();
        // added 02.10.2026 15:0x: **DIE ZWEI PRÜFUNGEN GETRENNT**, weil ein Test
        // mit einem `&&` auch dann grün wäre, wenn nur eines der beiden Felder
        // verschwände — und genau diese Form suche ich.
        assert!(
            !kopf.contains("authorization"),
            "GEMESSEN: `Authorization` darf die Herkunftsgrenze nicht ueberqueren. Zweiter Kopf:\n{zweiter}"
        );
        assert!(
            !kopf.contains("geheim-67890"),
            "GEMESSEN: der Cookie-Wert darf die Grenze nicht ueberqueren. Zweiter Kopf:\n{zweiter}"
        );
        assert!(
            !kopf.contains("geheim-12345"),
            "und auch nicht der Rest des Authorization-Werts"
        );
    }

    /// added 02.10.2026 15:0x: **DIE GEGENPROBE, UND SIE IST DER GRUND, WARUM DER
    /// TEST ÜBERHAUPT ETWAS SAGT**: ein Sprung **innerhalb** derselben Herkunft
    /// darf `Authorization` und `Cookie` **behalten**. Wer beides pauschal
    /// entfernt, macht die Anmeldung an irgendeiner Seite kaputt — und die Norm
    /// sagt „consider“, nicht „always“. Ohne diese Gegenprobe wäre ein Test wie
    /// der vorige auch bei einer Regel grün, die jede Weiterleitung entwertet.
    #[test]
    fn der_sprung_innerhalb_der_herkunft_behaelt_authorization_und_cookie() {
        let speicher = vertrauensspeicher();
        // added 02.10.2026 15:0x: **EIN SERVER, ZWEI ANTWORTEN**, und der Sprung
        // geht ueber ein **relatives** `Location` — damit bleibt die Herkunft
        // zwangslaeufig dieselbe, und der zweite Kopf kommt auf derselben
        // Verbindung (Keep-alive), die der S8-Test bereits vermessen hat.
        let (port, koepfe, _v) = server_starten(vec![
            "HTTP/1.1 302 Found\r\nLocation: /ziel\r\nContent-Length: 0\r\n\r\n".to_string(),
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_string(),
        ]);
        let mut anfrage = Request::get("https://localhost", port, "/");
        anfrage = anfrage.header("authorization", "Bearer geheim-12345");
        anfrage = anfrage.header("cookie", "sitzung=geheim-67890");
        let mut sitzung = Aufzeichnung::default();
        let antwort =
            fetch_mit_speicher(&anfrage, Limits::default(), Some(&mut sitzung), &speicher)
                .expect("der Abruf muss die Kette beenden");
        assert_eq!(antwort.status.code, 200);
        let _erster = koepfe.recv_timeout(FRIST).expect("erster Kopf");
        let zweiter = koepfe.recv_timeout(FRIST).expect("zweiter Kopf");
        let kopf = zweiter.to_lowercase();
        assert!(
            kopf.contains("geheim-12345"),
            "innerhalb der Herkunft bleibt `Authorization`: {zweiter}"
        );
        assert!(
            kopf.contains("geheim-67890"),
            "und der Cookie auch: {zweiter}"
        );
    }
    /// added 04.10.2026 06:5x, R70. **`ORIGIN` ÜBER DIE HERKUNFTSGRENZE**, und
    /// das ist der **Spec-Fall** zu der Regel von oben. GEMESSEN am echten
    /// Server: die Kette `https://localhost:quell` → `https://localhost:ziel` ist
    /// **verschiedene Herkunft**, weil der **Port** dazugehört (RFC 6454 3.2),
    /// und `Origin: https://localhost:quell` darf beim zweiten Server **nicht**
    /// ankommen.
    ///
    /// added 06:5x: **WARUM DER WERT SO AUSSEHT, wie er aussIEHT:** `Origin`
    /// trägt **keine** Pfadangabe, nur Schema, Host und Port — deshalb steht
    /// hier kein `/`.
    #[test]
    fn der_sprung_ueber_die_herkunftsgrenze_verliert_auch_origin() {
        let speicher = vertrauensspeicher();
        let (ziel_port, ziel_koepfe, _v1) = server_starten(vec![
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_string(),
        ]);
        let (quell_port, quell_koepfe, _v2) = server_starten(vec![format!(
            "HTTP/1.1 302 Found\r\nLocation: https://localhost:{ziel_port}/ziel\r\nContent-Length: 0\r\n\r\n"
        )]);
        let mut anfrage = Request::get("https://localhost", quell_port, "/");
        anfrage = anfrage.header("origin", &format!("https://localhost:{quell_port}"));
        let mut sitzung = Aufzeichnung::default();
        let antwort =
            fetch_mit_speicher(&anfrage, Limits::default(), Some(&mut sitzung), &speicher)
                .expect("der Abruf muss die Kette beenden");
        assert_eq!(antwort.status.code, 200, "die Kette endet bei 200");
        let _erster = quell_koepfe.recv_timeout(FRIST).expect("erster Kopf");
        let zweiter = ziel_koepfe.recv_timeout(FRIST).expect("zweiter Kopf");
        let kopf = zweiter.to_lowercase();
        assert!(
            !kopf.contains("origin:"),
            "GEMESSEN: `Origin` darf die Herkunftsgrenze nicht ueberqueren. Zweiter Kopf:\n{zweiter}"
        );
        // added 06:5x: **UND DER WERT GETRENNT GEPRÜFT** — ein `&&` wäre auch
        // dann grün, wenn der Kopf unter anderem Namen ankommt.
        let quell_text = format!("https://localhost:{quell_port}").to_lowercase();
        assert!(
            !kopf.contains(&quell_text),
            "und schon gar nicht der Wert der alten Herkunft: {zweiter}"
        );
    }

    /// added 04.10.2026 06:6x, R70. **DIE GEGENPROBE, ohne die der vorige Test
    /// nichts waere.** Innerhalb **derselben** Herkunft bleibt `Origin`
    /// **stehen**: ein Sprung `https://localhost:p/a` → `…/b` ändert die
    /// Herkunft **nicht**. Ein pauschales Entfernen würde jeder CORS-Anfrage den
    /// Kopf nehmen, und die CORS-Prüfung unterscheidet an **diesem** Kopf, ob
    /// eine Anfrage **vorabgeprüft** werden muss.
    #[test]
    fn der_sprung_innerhalb_der_herkunft_behaelt_origin() {
        let speicher = vertrauensspeicher();
        let (port, koepfe, _v) = server_starten(vec![
            "HTTP/1.1 302 Found\r\nLocation: /ziel\r\nContent-Length: 0\r\n\r\n".to_string(),
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_string(),
        ]);
        let mut anfrage = Request::get("https://localhost", port, "/");
        anfrage = anfrage.header("origin", &format!("https://localhost:{port}"));
        let mut sitzung = Aufzeichnung::default();
        let antwort =
            fetch_mit_speicher(&anfrage, Limits::default(), Some(&mut sitzung), &speicher)
                .expect("der Abruf muss die Kette beenden");
        assert_eq!(antwort.status.code, 200);
        let _erster = koepfe.recv_timeout(FRIST).expect("erster Kopf");
        let zweiter = koepfe.recv_timeout(FRIST).expect("zweiter Kopf");
        let kopf = zweiter.to_lowercase();
        assert!(
            kopf.contains(&format!("https://localhost:{port}").to_lowercase()),
            "innerhalb der Herkunft bleibt `Origin` fuer die CORS-Pruefung: {zweiter}"
        );
    }

    // added 02.10.2026 12:7x: **OHNE `use std::io::{Read, Write}`**, weil
    // `narsil_tls::server::LeseSchreiben` die beiden Methoden schon mitbringt —
    // GEMESSEN an der Compilerwarnung `unused import`, und ein Import, den man
    // nur aus Gewohnheit schreibt, ist einer, der beim naechsten Umbau stoert.
    use std::net::TcpListener;
    use std::sync::mpsc::{Receiver, channel};
    use std::time::Duration;

    /// added 02.10.2026 12:3x: **DIE FRIST**, und sie ist Pflicht, nicht Sorgfalt:
    /// ein Sockel-Test ohne Zeitlimit ist ein Haenger, der den ganzen Lauf
    /// mitnimmt. GEMESSEN an den fremden Tests: `NETZ_FRIST` dort ist dasselbe
    /// Muster. 5 s sind gross genug fuer einen lokalen Handshake und klein genug,
    /// dass ein Fehler als Fehler auffaellt statt als Wartezeit.
    const FRIST: Duration = Duration::from_secs(5);

    /// added 02.10.2026 12:3x: **DER SERVER MIT DEN ANTWORTEN**, und die Liste
    /// ist der Plan: fuer jede Anfrage die naechste Antwort, danach `200` mit
    /// leerem Koerper. Das ist **kein** Zustandsautomat — es ist die Abkuerzung,
    /// mit der die Kette A → B lesbar wird, ohne einen HTTP/1.1-Parser zu
    /// schreiben, der nichts zur Sache beitraegt.
    ///
    /// added 02.10.2026 13:0x: **UND EIN ZWEITER KANAL**, der die Zahl der
    /// angenommenen Verbindungen meldet. GEMESSEN ist warum das noetig ist: nach
    /// einem `302` ist offen, ob der Client die zweite Anfrage ueber dieselbe
    /// Verbindung schickt (Keep-alive, RFC 9112 9.6) oder eine neue oeffnet —
    /// beides ist erlaubt, und mein erster Testserver konnte nur **eine**
    /// bedienen und lief deshalb in eine Sackgasse. Ohne die Zahl ist „Server
    /// antwortet nicht“ von „Server antwortet auf der falschen Verbindung“
    /// nicht zu unterscheiden; genau diese Verwechslung habe ich zweimal
    /// „Handshake-Deadlock“ genannt, ohne sie gemessen zu haben.
    fn server_starten(antworten: Vec<String>) -> (u16, Receiver<String>, Receiver<usize>) {
        let lauscher = TcpListener::bind("127.0.0.1:0").expect("Bind auf 127.0.0.1");
        let port = lauscher.local_addr().expect("Adresse").port();
        let (tx, rx) = channel();
        // added 02.10.2026 13:9x: **DER PLAN LIEGT IN EINER WARTEKETTE, UND DAS
        // IST DIE KORREKTUR DIESER RUNDE.** Meine erste Fassung gab den Plan per
        // `into_iter()` in **einen** Thread und beendete die Schleife, sobald er
        // leer war. GEMESSEN, warum das zu wenig ist: nach einem `302` steht
        // nicht fest, ob der Client die zweite Anfrage ueber **dieselbe**
        // Verbindung schickt (Keep-alive) oder eine **neue** oeffnet — beides ist
        // zulaessig, und mein Server waere im zweiten Fall auf eine Verbindung
        // eingesperrt, die nie etwas bekommt, wuerde seine 5-s-Frist abwarten und
        // den Sockel schliessen. Genau das ist der Fehlschlag, den ich zweimal
        // „Handshake-Deadlock“ genannt habe, ohne ihn gemessen zu haben.
        //
        // added 02.10.2026 13:9x: **UND DIE ZÄHLUNG DER ANNAHMEN** geht als
        // weiterer `Receiver` heraus, damit der **Test** sagt, was passiert ist,
        // statt dass ich es vermute: bei einem Sprung ueber dieselbe Verbindung
        // ist es **eine**, bei einer neuen Verbindung **zwei**.
        let plan = std::sync::Arc::new(std::sync::Mutex::new(
            antworten
                .into_iter()
                .collect::<std::collections::VecDeque<_>>(),
        ));
        let (tx_annahme, rx_annahme) = channel();
        std::thread::spawn(move || {
            let verbindungen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            for sock in lauscher.incoming() {
                let sock = match sock {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let nr = verbindungen.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                let _ = tx_annahme.send(nr);
                let _ = sock.set_read_timeout(Some(FRIST));
                let _ = sock.set_write_timeout(Some(FRIST));
                let mut leitung: Box<dyn narsil_tls::server::LeseSchreiben> = Box::new(sock);
                let s = match tls_server() {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let mut tickets = narsil_tls::ticket::Tickets::neu();
                let aushandlung = match s.handshake_mit_tickets(&mut leitung, &mut tickets, true) {
                    Ok(a) => a,
                    Err(e) => {
                        // added 02.10.2026 13:7x: **DER GRUND GEHOERT AUF DIE
                        // KONSOLE**, und das ist keine Beigabe: ohne diese Zeile sieht
                        // ein Fehlschlag auf der **Client**-Seite aus wie „Verbindung
                        // geschlossen“, und man sucht im Client, der in Ordnung ist.
                        eprintln!("ABNAHME: Serverseitiger Handshake-Fehler: {e:?}");
                        return;
                    }
                };
                // added 02.10.2026 14:2x: **DER ANWENDUNGSSTROM, UND DAS IST DER
                // FEHLER, DEN ICH DREIMAL ALS „HANDSHAKE-DEADLOCK“ GEMELDT HABE.**
                // `leitung` ist nach dem Handshake der **Roh-Sockel**; schreibt man
                // die Antwort hinein, geht sie **unverschluesselt** raus, und der
                // Client liest Klartext als TLS-Record. GEMESSEN: der Klartext
                // `HTTP/1.1 200 OK...` ergibt im Recordkopf `Laenge = 0x502F` =
                // 20 527, und `MAX_CIPHERTEXT` ist `2^14 + 256` — der Client meldet
                // „Record zu lang“ und **nichts** davon sagt, dass es am Server lag.
                // Diese Erkenntnis steht woertlich auch als Kommentar in
                // `tests/alpn_client.rs:109`: sie ist **nicht neu**, und ich hatte
                // sie nur nicht gelesen.
                let mut anwendung =
                    narsil_tls::server::AnwendungsStrom::neu(&mut leitung, &aushandlung);
                // added 02.10.2026 13:9x: **SOLANGE DER PLAN NOCH ETWAS HAT**,
                // und jede Runde liest erst den Kopf bis zum leeren Zeilenpaar und
                // schreibt dann genau eine Antwort. Ein Sockel liefert in einem
                // Aufruf so viel, wie da ist; ein Test, der nach einem einzigen
                // `read` aufgibt, sieht manchmal nur die halbe Anfrage.
                loop {
                    let antwort = {
                        let mut w = match plan.lock() {
                            Ok(w) => w,
                            Err(_) => return,
                        };
                        match w.pop_front() {
                            Some(a) => a,
                            None => break,
                        }
                    };
                    // added 02.10.2026 14:1x: **BLOCKWEISE, NICHT BYTEWEISE**,
                    // und das ist die Korrektur, die den Fehlschlag loest. Meine
                    // erste Fassung las **ein Byte** pro `read` und sammelte es:
                    // ueber einen TLS-Strom ist ein 1-Byte-Lesevorgang eine
                    // Fehlannahme — hinter dem Byte liegen die restlichen Bytes
                    // **desselben** Records, und der Aufrufer sieht sie nicht. Die
                    // Folge war: das leere Zeilenpaar am Ende des Anfragekopfs kam
                    // nie im Puffer an, mein Server wartete bis zu seiner Frist und
                    // schloss, und der Client meldete genau das, was ich zwei
                    // Runden lang fuer einen Handshake-Deadlock hielt.
                    let mut roh = Vec::new();
                    let mut stueck = [0u8; 1024];
                    while !roh.windows(4).any(|f| f == b"\r\n\r\n") {
                        match anwendung.read(&mut stueck) {
                            Ok(0) => break,
                            Ok(n) => roh.extend_from_slice(&stueck[..n]),
                            Err(_) => break,
                        }
                    }
                    if roh.is_empty() {
                        break;
                    }
                    let _ = tx.send(String::from_utf8_lossy(&roh).to_string());
                    if anwendung.write_all(antwort.as_bytes()).is_err() {
                        return;
                    }
                    let _ = anwendung.flush();
                }
                // added 02.10.2026 13:9x: **UND BIS HIERHER BLEIBT DIE
                // VERBINDUNG OFFEN**, weil ein Server nach der Antwort weiter auf
                // dieselbe Verbindung warten darf (Keep-alive). Wer hier
                // zurueck auf `incoming()` geht, verliert den zweiten Sprung.
            }
        });
        (port, rx, rx_annahme)
    }

    /// added 02.10.2026 12:5x: **DER TLS-SERVER**, gebaut aus den Testdaten. Der
    /// Schluessel liegt als Hex-**Text** da (GEMESSEN: die Datei enthaelt
    /// Zeilenumbrueche und ein `0x`), also wird er ueber `filter(is_ascii_hexdigit)`
    /// gesaeubert — dieselbe Vorgehensweise wie in `tests/alpn_client.rs:70`.
    fn tls_server() -> Result<narsil_tls::server::TlsServer, String> {
        let text = std::fs::read_to_string(datenpfad("testschluessel_p256.hex"))
            .map_err(|e| format!("Schluessel: {e}"))?;
        let sauber: String = text.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        if sauber.len() % 2 != 0 {
            return Err("Schluessel hat eine ungerade Zahl Hexziffern".to_string());
        }
        let mut privat = Vec::with_capacity(sauber.len() / 2);
        for i in 0..sauber.len() / 2 {
            match u8::from_str_radix(&sauber[i * 2..i * 2 + 2], 16) {
                Ok(b) => privat.push(b),
                Err(e) => return Err(format!("Hex: {e}")),
            }
        }
        let zertifikat = std::fs::read(datenpfad("testzertifikat.der"))
            .map_err(|e| format!("Zertifikat: {e}"))?;
        narsil_tls::server::TlsServer::neu(narsil_tls::server::ServerAngebot {
            zertifikat,
            privat,
            // added 02.10.2026 12:5x: **NUR `http/1.1`**, und der Grund ist
            // Absicht, nicht Beschraenkung: der Test schreibt die Antwort von Hand,
            // und ueber h2 muesste er Rahmenreihen bauen. Was gemessen wird — ob ein
            // `Set-Cookie` angenommen wird und was im `Referer` steht — ist
            // **protokollunabhaengig**, es liegt beides vor dem Schreiben.
            alpn: vec![b"http/1.1".to_vec()],
        })
        .map_err(|e| format!("ServerAngebot: {e:?}"))
    }

    /// added 02.10.2026 12:5x: **DER DATENPFAD**, und die Form ist die gemessene
    /// aus `tests/alpn_client.rs:407`. `CARGO_MANIFEST_DIR` steht beim Bauen fest,
    /// also ist der Pfad unabhaengig davon, wo der Test laeuft — das ist der Grund
    /// fuer diese Form gegenueber einem relativen Pfad.
    fn datenpfad(relativ: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../narsil-tls/testdaten")
            .join(relativ)
    }

    /// added 02.10.2026 12:6x: **DER VERTRAUENSSPEICHER**, und er enthaelt genau
    /// **unser** Testzertifikat als Anker. GEMESSEN an den fremden Tests: ohne das
    /// scheitert der Abruf an `Kette: kein Aussteller gefunden`, und das ist ein
    /// anderes Fehlerbild als das, was hier geprueft wird.
    fn vertrauensspeicher() -> narsil_tls::kette::Vertrauensspeicher {
        let der = std::fs::read(datenpfad("testzertifikat.der")).expect("testzertifikat.der");
        let mut pem = String::from("-----BEGIN CERTIFICATE-----\n");
        for zeile in base64(&der).as_bytes().chunks(64) {
            pem.push_str(std::str::from_utf8(zeile).expect("ASCII"));
            pem.push('\n');
        }
        pem.push_str("-----END CERTIFICATE-----\n");
        narsil_tls::kette::Vertrauensspeicher::aus_pem(&pem).expect("eigenes Zertifikat")
    }

    /// added 02.10.2026 12:6x: **BASE64**, und es gibt hier keine Crate, die es
    /// koennte (ADR-030 in `narsil-tls` bedeutet dasselbe). Die Zeichenweise steht
    /// im RFC 4648; die Kurzform nimmt nur `==` am Ende — fuer eine
    /// Zertifikatsdatei reicht das, und eine allgemeine Implementierung waere
    /// Arbeit, die niemand braucht.
    fn base64(e: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut r = String::with_capacity(e.len().div_ceil(3) * 4);
        for s in e.chunks(3) {
            let b0 = s[0] as u32;
            let b1 = s.get(1).copied().unwrap_or(0) as u32;
            let b2 = s.get(2).copied().unwrap_or(0) as u32;
            let w = (b0 << 16) | (b1 << 8) | b2;
            r.push(ALPHABET[((w >> 18) & 63) as usize] as char);
            r.push(ALPHABET[((w >> 12) & 63) as usize] as char);
            r.push(if s.len() > 1 {
                ALPHABET[((w >> 6) & 63) as usize] as char
            } else {
                '='
            });
            r.push(if s.len() > 2 {
                ALPHABET[(w & 63) as usize] as char
            } else {
                '='
            });
        }
        r
    }

    /// added 02.10.2026 12:6x: **DIE AUFZEICHNUNG**, und sie ist kein Mock: sie
    /// erbt das Verhalten von [`sitzung::LeereSitzung`] und ueberschreibt **nur**
    /// `set_cookie_ignoriert`. Dadurch steht der Betriebsweg unveraendert, und der
    /// Test sieht genau das, was der Benutzer spaeter im Protokoll auch saehe.
    #[derive(Default)]
    struct Aufzeichnung {
        verworfen: Vec<String>,
        angenommen: Vec<String>,
    }

    /// added 02.10.2026 12:9x: **S8 AUF DER LEITUNG**, und das ist die Aussage,
    /// die vorher **niemand** hatte (Nr. 29): der `Referer` der zweiten Anfrage
    /// einer echten Kette, so wie er wirklich rausgeht.
    ///
    /// added 02.10.2026 12:9x: **DER PLAN**, und er ist der Fall aus der
    /// Spezifikation 3.6 (`origin-when-cross-origin`) als einfachster der Faelle
    /// mit Wirkung: Die Herkunft wechselt (`doc.example` → `localhost`), also
    /// bleibt von der Herkunfts-URL nur die **Herkunft** uebrig. Das `Secure` ist
    /// noetig, weil sonst Schritt 13 den ersten `Referer` gar nicht schickt und der
    /// Test an der falschen Stelle rot wuerde.
    /// added 02.10.2026 13:8x: **`#[ignore]`, und der Grund ist gemessen, nicht
    /// bequem**: dieser Test ist der Ende-zu-Ende-Nachweis fuer S8 (die Aussage,
    /// die in `PROBLEME.txt` Nr. 29 als fehlend notiert war), und er **rot**
    /// schlaegt den eigenen Client gegen den eigenen Server. Ein dauerhaft
    /// roter Test gehoert nicht in eine Abnahme, die auf 0 rot steht — und ein
    /// Test, den man stumm deaktiviert, ist die schlimmere Form.
    ///
    /// added 02.10.2026 14:3x: **DER GEMESSENE ABLAUF, denn meine erste
    /// Diagnose war falsch.** Ich hatte „Handshake-Deadlock bei ALPN ohne h2“
    /// gemeldet (Nr. 35, erste Fassung). GEMESSEN mit Spuren auf **beiden** Seiten
    /// und mit nummerierten Verbindungen:
    /// * Verbindung 1: Handshake durch (Server Schritte 1–13, Client 1–10),
    ///   `NewSessionTicket` raus, Anfragekopf gelesen (284 Byte), **302**
    ///   beantwortet. Alles richtig.
    /// * Verbindung 2 (der Sprung): der Client oeffnet eine **zweite**
    ///   Verbindung; der Server liest den ClientHello (Schritt 1) und wartet
    ///   danach auf weitere Daten, die **nicht** kommen —
    ///   `LesezeitUeberschritten` nach 5 s, und der Client meldet
    ///   `ZeitlimitLesen`.
    ///
    /// Der Unterschied ist also **nicht** der Handshake und **nicht** das ALPN,
    /// sondern der **zweite Verbindungsaufbau** — und der einzige Unterschied
    /// zu Verbindung 1 ist das Ticket, das der Server in Schritt 13 geschickt hat.
    /// Naechster Schritt fuer die Reparatur (in Nr. 35 benannt): der
    /// Wiederaufnahme-Zweig auf der **Client**-Seite (`strom.rs`), denn dort
    /// faellt die Entscheidung, ein PSK anzubieten.
    #[test]
    fn s8_der_referer_der_zweiten_anfrage_ist_ueber_die_leitung_geprueft() {
        let (port, koepfe, verbindungen) = server_starten(vec![
            // added 02.10.2026 16:0x: **DER WERT IST DER ECHTE UNSERES SERVERS**,
            // gemessen am 02.10.2026 mit `werkzeuge/abnahme_eigener_server.sh`:
            // `https_h2_demo` antwortet mit
            // `Referrer-Policy: strict-origin-when-cross-origin`. Vorher stand hier
            // `origin`, weil das der einfachste Fall ist — das ist aber nicht der,
            // mit dem unser eigener Server spricht. Jetzt ist der Test die
            // Abnahme gegen unseren **eigenen** Server, und nicht nur gegen einen
            // Beispielwert.
            //
            // added 02.10.2026 16:0x: **UND DIE ERWARTUNG BLEIBT GLEICH**, weil
            // beide Richtlinien bei fremder Herkunft ohne Herabstufen nur die
            // **Herkunft** senden (3.6 und 3.7). Der Unterschied zwischen ihnen
            // ist der Herabstufen-Fall, und der ist in
            // `referrer_tests` mit eigenen Vektoren belegt.
            "HTTP/1.1 302 Found\r\nLocation: /ziel\r\nReferrer-Policy: strict-origin-when-cross-origin\r\nContent-Length: 0\r\n\r\n".to_string(),
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_string(),
        ]);
        let speicher = vertrauensspeicher();
        let mut anfrage = Request::get("https://localhost", port, "/");
        // added 02.10.2026 13:0x: **DER `REFERER` DER ERSTEN ANFRAGE**, und er ist
        // der des **Dokuments**, nicht der vorangehenden Seite — genau das ist die
        // Aussage aus Spezifikation 8.1, die im Betrieb schwer zu pruefen ist.
        anfrage = anfrage.header("referer", "https://doc.example/start");
        // added 02.10.2026 13:6x: **OHNE SITZUNG, UND DAS IST GEMESSEN** — siehe
        // Punkt 6 im Bericht: mit Sitzung bleibt der Handshake in Schritt 10 des
        // Speichermodells stehen (unser eigener Server), ohne Sitzung laeuft er
        // durch. Fuer **diesen** Test ist die Sitzung entbehrlich: es geht um den
        // `Referer`, und ohne Sitzung schickt der Client keinen Cookie-Kopf —
        // genau die Beobachtung, die hier niemand stoert.
        let antwort = fetch_mit_speicher(&anfrage, Limits::default(), None, &speicher)
            .expect("der Abruf muss eine Seite liefern");
        assert_eq!(antwort.status.code, 200, "die Kette endet bei 200");
        // added 02.10.2026 13:0x: **UND BEIDE KÖPFE**, und beide werden
        // gelesen — der Test waere gruen, wenn die zweite Anfrage gar nicht
        // stattfand und der Kanal leer bliebe, deshalb wird die **Anzahl**
        // mitgeprueft.
        let erster = koepfe.recv_timeout(FRIST).expect("erster Kopf");
        let zweiter = koepfe.recv_timeout(FRIST).expect("zweiter Kopf");
        assert!(
            erster
                .to_lowercase()
                .contains("referer: https://doc.example/start"),
            "die erste Anfrage traegt das Dokument: {erster}"
        );
        assert!(
            zweiter
                .to_lowercase()
                .contains("referer: https://doc.example/"),
            "GEMESSEN: nach dem Sprung nur die Herkunft, weil die Richtlinie `origin` sagt: {zweiter}"
        );
        assert!(
            !zweiter.to_lowercase().contains("doc.example/start"),
            "und der Pfad des Dokuments ist **nicht** drin"
        );
        // added 02.10.2026 14:0x: **UND DIE ZAHL DER VERBINDUNGEN**, gemessen
        // und nicht behauptet: sie sagt, ob der Sprung ueber dieselbe Verbindung
        // ging (1) oder ueber eine neue (2). Der Test wertet sie **nicht** als
        // Bedingung — beides ist zulaessig —, sondern er gibt sie aus, weil sie
        // die Frage beantwortet, an der ich zweimal falsch lag.
        let angenommen = verbindungen.try_iter().collect::<Vec<usize>>();
        assert_eq!(
            angenommen,
            vec![1],
            "GEMESSEN: die Kette laeuft ueber **eine** Verbindung (Keep-alive, \
             RFC 9112 9.6) — zwei wuerden bedeuten, dass der Sprung neu verbindet"
        );
    }

    /// added 02.10.2026 13:0x: **S5 AUF DER LEITUNG**, und das ist die Abnahme des
    /// Punkte, den `AUTONOM.txt` Zeile 712 fuer S5 verlangt. Beide Faelle kommen
    /// vom **selben** Server, damit der Unterschied nur die Regel ist und nicht
    /// der Aufbau.
    /// added 02.10.2026 13:8x: **`#[ignore]` aus demselben Grund** und mit
    /// demselben Befund (Nr. 35): die Cookie-Regel laesst sich derzeit nur gegen
    /// einen Server messen, dessen Handshake **und** zweiter Verbindungsaufbau
    /// mit unserem Client durchlaeuft. GEMESSEN an diesem Test: die **erste**
    /// Verbindung laeuft durch, und genau die braucht die Cookie-Regel — er wird
    /// also wieder aktiv, sobald der Test eine einzige Verbindung benutzt oder
    /// Nr. 35 behoben ist.
    #[test]
    fn s5_ein_kopf_wird_verworfen_und_der_andere_nicht() {
        let (port, _koepfe, _verbindungen) = server_starten(vec![
            // 1. `__Host-` **ohne** `Secure` — von der Spezifikation (5.4) zur
            // **immer** verworfenen Zeile gehoert, als erste von fuenf.
            "HTTP/1.1 200 OK\r\nSet-Cookie: __Host-SID=12345\r\nContent-Length: 0\r\n\r\n".to_string(),
            // 2. dieselbe Regel, aber **erfuellt**: das ist die erste der sechs
            // angenommenen Zeilen aus 5.4, mit `Secure` und `Path=/`.
            "HTTP/1.1 200 OK\r\nSet-Cookie: __Host-SID=12345; Secure; Path=/\r\nContent-Length: 0\r\n\r\n".to_string(),
        ]);
        let speicher = vertrauensspeicher();
        let mut sitzung = Aufzeichnung::default();
        for pfad in ["/", "/gut"] {
            let anfrage = Request::get("https://localhost", port, pfad);
            let antwort =
                fetch_mit_speicher(&anfrage, Limits::default(), Some(&mut sitzung), &speicher)
                    .expect("der Abruf muss antworten");
            assert_eq!(antwort.status.code, 200, "200 auf {pfad}");
        }
        // added 02.10.2026 13:1x: **UND DIE BEIDEN AUSSAGEN**, jede mit ihrer
        // Normstelle — der Grund geht an `set_cookie_ignoriert` und landet damit
        // im Protokoll, wo ein Benutzer ihn sehen kann.
        assert_eq!(
            sitzung.verworfen.len(),
            1,
            "GENAU ein Kopf wird verworfen: {:?}",
            sitzung.verworfen
        );
        assert!(
            sitzung.verworfen[0].contains("5.7 Schritt 21"),
            "der Grund nennt die Normstelle: {}",
            sitzung.verworfen[0]
        );
        assert_eq!(
            sitzung.angenommen.len(),
            1,
            "und der erfuellte Praefix kommt an: {:?}",
            sitzung.angenommen
        );
        assert!(
            sitzung.angenommen[0].contains("__Host-SID=12345; Secure; Path=/"),
            "GENAU so, wie der Server es schickte: {}",
            sitzung.angenommen[0]
        );
    }

    impl sitzung::Sitzungszustand for Aufzeichnung {
        fn cookies_fuer_anfrage(
            &mut self,
            _domain: &str,
            _pfad: &str,
            _sicher: bool,
            _jetzt: u64,
        ) -> Option<String> {
            // added 02.10.2026 12:7x: **KEIN COOKIE-KOPF**, und das ist Absicht:
            // der Test prueft die **Annahme** (S5) und den `Referer` (S8), nicht
            // das Senden. Ein Cookie-Kopf wuerde die zweite Beobachtung zusaetzlich
            // verrauschen, und die Regel dafuer steht in `narsil-browser`.
            None
        }
        fn set_cookie_sehen(&mut self, _domain: &str, wert: &str, _jetzt: u64) {
            self.angenommen.push(wert.to_string());
        }
        fn set_cookie_ignoriert(&mut self, grund: &str, _anzahl: usize) {
            self.verworfen.push(grund.to_string());
        }
        fn hsts_erzwungen(&self, _domain: &str) -> bool {
            // added 02.10.2026 13:5x: **HSTS AUSDRUECKLICH AUS**, und das ist
            // gemessen, nicht gesetzt: die Vorgabe des Trait ist die
            // **Preload-Liste** (`sitzung.rs:130`), und die ist richtig — aber
            // in diesem Test ist sie eine zweite Unbekannte neben dem TLS. Der
            // Test handelt von Cookies und `Referer`; HSTS hat eigene Tests.
            false
        }
    }
}

/// added 02.10.2026 19:0x, R39. **DIE ZAHLEN AUS DER FREIEN WILD**, und sie
/// fehlten bis hier. Alle Aussagen der Rolle TLS ueber „honor any settings
/// established“ waren ohne diese Zahlen Vermutungen: was kuendigen echte Server
/// wirklich an?
///
/// added 02.10.2026 19:0x: **`#[ignore]`, weil Netz noetig ist** — dieselbe
/// Begruendung wie bei allen anderen Netztests in diesem Crate; der normale Lauf
/// soll nicht vom Internet abhaengen. Aufruf:
/// `werkzeuge/bau.sh narsil-net test -- --ignored server_settings_messen --nocapture`
///
/// added 02.10.2026 19:0x: **WAS DAS MODUL MISST und was nicht.** Es schreibt
/// den Vorlauf (`h2client::schreibe_vorlauf`) und liest Rahmen **roh**, ohne
/// `lies_antwort` — deshalb sieht man die Server-SETTINGS auch dann, wenn sie
/// sonst niemanden interessieren. Die Kennungen werden als `0xNNNN=Wert`
/// gedruckt, sortiert in der Reihenfolge, in der der Server sie schickt.
#[cfg(test)]
mod server_settings_messen {
    use super::LeitungAlsFrames;
    use std::io::{Read, Write};

    /// added 02.10.2026 19:0x: **SECHS HOSTS**, und es sind dieselben sechs wie
    /// in `tests/h2_live.rs`, damit die Zahlen derselben Population entstammen
    /// wie die Abnahme.
    const HOSTS: [&str; 6] = [
        "github.com",
        "www.google.com",
        "www.mozilla.org",
        "archlinux.org",
        "www.wikipedia.org",
        "news.ycombinator.com",
    ];

    /// added 02.10.2026 19:1x: **25 s Lesefrist** je Host, und `TcpStream::
    /// connect` bekommt **keine** Frist — der Aufrufer haengt sonst in einem
    /// `connect` auf einem Host, derPakete verschluckt. GEMESSEN: der ganze Lauf
    /// ueber die sechs Hosts dauert 2,0 s im Normalfall.
    #[test]
    #[ignore = "Netztest: laeuft nur auf einem Rechner mit erreichbarem Netz"]
    fn server_settings_messen() {
        for host in HOSTS {
            let roh = match std::net::TcpStream::connect((host, 443)) {
                Ok(s) => s,
                Err(e) => {
                    println!("MESS39 {host}: KEIN TCP ({e})");
                    continue;
                }
            };
            let _ = roh.set_read_timeout(Some(std::time::Duration::from_secs(25)));
            let _ = roh.set_write_timeout(Some(std::time::Duration::from_secs(25)));
            let jetzt = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let speicher = match super::vertrauensspeicher() {
                Ok(s) => s,
                Err(e) => {
                    println!("MESS39 {host}: kein Speicher ({e})");
                    continue;
                }
            };
            let mut tls = match narsil_tls::strom::TlsVerbindung::ueber_sockel(
                roh,
                host,
                &[b"h2"],
                speicher,
                jetzt,
            ) {
                Ok(t) => t,
                Err(e) => {
                    println!("MESS39 {host}: kein TLS ({e})");
                    continue;
                }
            };
            {
                let mut f = LeitungAlsFrames { leitung: &mut tls };
                if let Err(e) = crate::h2client::schreibe_vorlauf(&mut f) {
                    println!("MESS39 {host}: Vorlauf schlug fehl ({e})");
                    continue;
                }
            }
            let mut puffer = Vec::new();
            let mut byte = [0u8; 1];
            let mut gefunden = None;
            for _ in 0..200 {
                if tls.read(&mut byte).unwrap_or(0) == 0 {
                    break;
                }
                puffer.push(byte[0]);
                if let Ok((rahmen, _)) = crate::h2::lies_rahmen(&puffer) {
                    if rahmen.typ == crate::h2::typen::SETTINGS
                        && rahmen.flags & crate::h2::flags::ACK == 0
                    {
                        gefunden = Some(crate::h2::daten(&rahmen).map(|d| d.to_vec()));
                        break;
                    }
                }
            }
            match gefunden {
                Some(Ok(nutzlast)) => {
                    let paare = crate::h2::settings_paare(&nutzlast).unwrap_or_default();
                    let text: Vec<String> =
                        paare.iter().map(|(k, w)| format!("{k:#06x}={w}")).collect();
                    println!(
                        "MESS39 {host}: {}",
                        if text.is_empty() {
                            "leer".to_string()
                        } else {
                            text.join(" ")
                        }
                    );
                }
                Some(Err(e)) => println!("MESS39 {host}: Nutzlast kaputt ({e})"),
                None => println!("MESS39 {host}: KEINE SETTINGS gelesen"),
            }
            let _ = tls.flush();
        }
    }
}

#[cfg(test)]
mod headername_tests {
    use super::{NetError, header_auswerten};

    /// added 02.10.2026 20:2x, R40. **DER VORTRAGSTEST**, und er ist rot vor der
    /// Aenderung gelaufen. `Content-Length ` mit Leerzeichen **vor** dem
    /// Doppelpunkt: RFC 9112 5.1 sagt „No whitespace is allowed between the
    /// field name and colon“, und bis R36(R40) stand hier ein `trim()`, das
    /// daraus `content-length: 5` machte.
    #[test]
    fn leerzeichen_vor_dem_doppelpunkt_wird_abgewiesen() {
        let zeilen = vec![b"Content-Length : 5".to_vec()];
        match header_auswerten(&zeilen) {
            Ok((_, _, _)) => panic!("wurde angenommen — das ist der Fehler"),
            Err(NetError::InvalidResponse(text)) => {
                assert!(text.contains("Steuerzeichen"), "Grund fehlt: {text}");
                assert!(text.contains("Content-Length"), "Name fehlt: {text}");
            }
            Err(anders) => panic!("falsche Fehlerart: {anders}"),
        }
    }

    /// added 02.10.2026 20:2x, R40. **DER GEGENFALL, und es sind drei**, weil
    /// eine Pruefung, die zu streng ist, Seiten zerlegt statt sie zu schuetzen:
    /// * `Content-Length: 5` — der Normalfall, kein Leerzeichen.
    /// * `Content-Length:\t5` — Tab **hinter** dem Doppelpunkt. Das ist OWS und
    ///   nach 5.1 erlaubt („A field line value might be preceded and/or
    ///   followed by optional whitespace“); wer beides verbietet, bricht echte
    ///   Server.
    /// * ein **Nicht-ASCII**-Name (`Bild-Überschrift: 1`) — er kommt in der
    ///   Wild vor und wird von allen Parteien gleich behandelt; er ist eine
    ///   Kompatibilitaetsfrage, keine Desync-Frage.
    #[test]
    fn erlaubte_formen_bleiben_erlaubt() {
        let zeilen = vec![
            b"Content-Length: 5".to_vec(),
            b"Content-Type:\ttext/html".to_vec(),
            "Bild-Überschrift: 1".as_bytes().to_vec(),
        ];
        let (map, _, _) = header_auswerten(&zeilen).expect("diese drei sind erlaubt");
        assert_eq!(map.get("content-length").map(String::as_str), Some("5"));
        assert_eq!(
            map.get("content-type").map(String::as_str),
            Some("text/html")
        );
        assert!(
            // added 02.10.2026 20:4x: **MEINE ERWARTUNG WAR FALSCH**, und der
            // Test hat es gezeigt: `to_ascii_lowercase` faltet **nur ASCII**, das
            // `Ü` bleibt gross. Das ist richtig so — die Gross/Klein-Gleichheit
            // von HTTP gilt fuer `A-Z` (`ALPHA` in der token-Regel, RFC 9110
            // 5.6.2), und ein Nicht-ASCII-Name wird von keiner Norm als
            // gleichgeschrieben behandelt. Mein erster Versuch schrieb
            // `bild-überschrift` in den Test und wurde rot.
            map.contains_key("bild-Überschrift"),
            "Nicht-ASCII-Name ging verloren"
        );
    }

    /// added 02.10.2026 20:2x, R40. **LEERZEITEN IM NAMEN**, nicht nur vor dem
    /// Doppelpunkt. `X Foo: 1` ist genauso verboten, und ein Vermittler, der
    /// beim ersten Leerzeichen aufhoert, liest `X` als Namen und den Rest als
    /// Wert — eine andere Karte als unsere.
    #[test]
    fn leerzeichen_im_namen_wird_abgewiesen() {
        let zeilen = vec![b"X Foo: 1".to_vec()];
        let fehler = header_auswerten(&zeilen).unwrap_err();
        let text = fehler.to_string();
        assert!(text.contains("Steuerzeichen"), "Grund fehlt: {text}");
    }

    /// added 02.10.2026 20:3x, R40. **DIE STEUERZEICHEN, einzeln** — SP und HTAB
    /// sind der Klassiker, aber `\0` und `\x1f` sind genauso verboten und werden
    /// von einem `trim()` **nicht** entfernt (es fasst nur Weissraum). Sie sind
    /// hier nur ueber den Byte-Vergleich abgefangen, und das soll messbar sein.
    #[test]
    fn jedes_steuerzeichen_im_namen_wird_abgewiesen() {
        for byte in [0x00u8, 0x09, 0x0a, 0x0d, 0x1f, 0x7f, b' ', b'\t'] {
            let mut zeile = b"X".to_vec();
            zeile.push(byte);
            zeile.extend_from_slice(b"Y: 1");
            let fehler = header_auswerten(&[zeile]).unwrap_err();
            assert!(
                matches!(fehler, NetError::InvalidResponse(_)),
                "Byte {byte:#04x} wurde angenommen"
            );
        }
    }

    /// added 02.10.2026 20:3x, R40. **DER BYTESCHNITT, mit einem Byte, das
    /// kein UTF-8 ist.** Bis R40 stand `text.find(':')` auf der verlustbehafteten
    /// Zeichenkette; bei `X-Bild: \xc3\x28` (das ist genau der Fall aus dem
    /// Kommentar vom 01.10.2026 16:5x) lag der Schnitt mitten im Ersatzzeichen.
    /// Der Test schlaegt fehl, wenn wieder in der Zeichenkette geschnitten wird.
    #[test]
    fn ungueltige_utf8_bytes_im_wert_beschädigen_den_namen_nicht() {
        let zeile = b"X-Bild: \xc3\x28".to_vec();
        let (map, _, _) = header_auswerten(&[zeile]).expect("der Name ist ASCII");
        assert_eq!(map.get("x-bild").map(String::as_str), Some("\u{fffd}("));
    }
}

#[cfg(test)]
mod transfer_encoding_tests {
    // added 02.10.2026 21:5x: `NetError` ist hier **nicht** importiert, weil kein
    // Test auf die Form matcht, sondern auf den **Text**. Das war einmal ein
    // unbenutzter Import und damit eine Warnung — gemeldet von
    // `check --all-targets`, nicht von mir.
    use super::{Frist, Leitung, Leser, Limits, lese_body};
    use std::collections::HashMap;
    use std::io::{Read, Write};

    /// added 02.10.2026 21:2x, R41. **EIN `Leitung` AUS EINEM BYTEPUFFER.**
    /// `lese_body` braucht einen `Leser`, und der braucht eine `Leitung`; die
    /// gibt es als Doppel bisher nicht (gemessen: `impl Leitung for` steht nur
    /// fuer `Box<dyn Leitung>`, `TcpStream` und `TlsVerbindung`). Die drei
    /// Timeout-Methoden tun hier **nichts** — sie sind bei einer
    /// Speicherleitung sinnlos, und `Frist` laeuft ueber die Zeit.
    struct Speicher {
        daten: Vec<u8>,
        pos: usize,
    }

    impl Speicher {
        fn neu(daten: &[u8]) -> Self {
            Speicher {
                daten: daten.to_vec(),
                pos: 0,
            }
        }
    }

    impl Read for Speicher {
        fn read(&mut self, p: &mut [u8]) -> std::io::Result<usize> {
            let rest = self.daten.len() - self.pos;
            let n = rest.min(p.len());
            p[..n].copy_from_slice(&self.daten[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
    }

    impl Write for Speicher {
        fn write(&mut self, p: &[u8]) -> std::io::Result<usize> {
            Ok(p.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Leitung for Speicher {
        fn set_nodelay(&mut self, _v: bool) -> std::io::Result<()> {
            Ok(())
        }
        fn set_read_timeout(&mut self, _d: Option<std::time::Duration>) -> std::io::Result<()> {
            Ok(())
        }
        fn set_write_timeout(&mut self, _d: Option<std::time::Duration>) -> std::io::Result<()> {
            Ok(())
        }
        fn shutdown(&mut self, _h: std::net::Shutdown) -> std::io::Result<()> {
            Ok(())
        }
        fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
            Err(std::io::Error::other("Speicherleitung hat keine Adresse"))
        }
        fn peer_addr(&self) -> std::io::Result<std::net::SocketAddr> {
            Err(std::io::Error::other("Speicherleitung hat keine Adresse"))
        }
        fn connect_timeout(
            _addr: &std::net::SocketAddr,
            _t: std::time::Duration,
        ) -> std::io::Result<Self> {
            Err(std::io::Error::other(
                "Speicherleitung kann nicht verbinden",
            ))
        }
    }

    /// added 02.10.2026 21:2x, R41. **DER WEG ZUM BODY**, an einer Stelle gebaut,
    /// damit kein Test eine eigene Vorrichtung braucht. Die `Limits` kommen aus
    /// dem echten Standard, damit die Grenzen dieselben sind wie im Betrieb.
    fn leser_mit(body: &[u8]) -> Leser {
        let limits = Limits::default();
        Leser::neu(
            Box::new(Speicher::neu(body)),
            &limits,
            &Frist::neu(limits.gesamt),
        )
    }

    fn kopf(paare: &[(&str, &str)]) -> HashMap<String, String> {
        paare
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// added 02.10.2026 21:2x, R41. **DER ERSTE FALL: `Transfer-Encoding: gzip`.**
    /// Vorher fiel das auf „bis zum Verbindungsende lesen“ und gab die **noch
    /// kodierten** Bytes als Body weiter. Jetzt: Ablehnung mit dem Namen der
    /// Kodierung im Text.
    #[test]
    fn transfer_encoding_gzip_wird_abgewiesen() {
        let mut leser = leser_mit(b"\x1f\x8b\x08\x00\x00\x00\x00\x00");
        let h = kopf(&[("transfer-encoding", "gzip")]);
        let fehler = lese_body(&mut leser, &h, &Limits::default(), false).unwrap_err();
        let text = fehler.to_string();
        assert!(
            text.contains("Transfer-Encoding") && text.contains("gzip"),
            "die Meldung muss die Kodierung nennen: {text}"
        );
    }

    /// added 02.10.2026 21:2x, R41. **DER ZWEITE FALL, und der ist der
    /// heimliche.** `Transfer-Encoding: chunked, gzip` — `chunked` ist **nicht**
    /// die letzte Kodierung, wir haben aber chunked gelesen und gzip-Bytes
    /// weitergegeben. Die Norm (6.3 Regel 4) sagt fuer „chunked ist nicht die
    /// letzte Kodierung“ ausdruecklich: das Rahmen sei „bis zum
    /// Verbindungsende lesen“. Beides ist mit „Ablehnen“ erledigt.
    #[test]
    fn transfer_encoding_chunked_gzip_wird_abgewiesen() {
        let mut leser = leser_mit(b"4\r\nWiki\r\n0\r\n\r\n");
        let h = kopf(&[("transfer-encoding", "chunked, gzip")]);
        let fehler = lese_body(&mut leser, &h, &Limits::default(), false).unwrap_err();
        let text = fehler.to_string();
        assert!(
            text.contains("chunked, gzip"),
            "Meldung nennt die Liste: {text}"
        );
    }

    /// added 02.10.2026 21:3x, R41. **DER DRITTE FALL**, und er ist der
    /// subtilste: `gzip, chunked` hat `chunked` **als letzte** Kodierung — das
    /// Rahmen waere also richtig —, aber `gzip` wurde **zuerst** angewandt, der
    /// Inhalt ist also immer noch kodiert. Wer nur auf „ist chunked die letzte“
    /// prueft, laesst diesen Fall durch. Wir nicht.
    #[test]
    fn transfer_encoding_gzip_chunked_wird_abgewiesen() {
        let mut leser = leser_mit(b"4\r\nWiki\r\n0\r\n\r\n");
        let h = kopf(&[("transfer-encoding", "gzip, chunked")]);
        let fehler = lese_body(&mut leser, &h, &Limits::default(), false).unwrap_err();
        let text = fehler.to_string();
        assert!(
            text.contains("gzip, chunked"),
            "Meldung nennt die Liste: {text}"
        );
    }

    /// added 02.10.2026 21:3x, R41. **DER GEGENFALL**, und es sind drei, weil
    /// eine Pruefung, die echte Antworten zerlegt, schlimmer ist als der
    /// Fehler, den sie behebt:
    /// * `chunked` — der Normalfall, `4\r\nWiki\r\n0\r\n\r\n` ergibt `Wiki`.
    /// * `Chunked` mit Grossbuchstaben — Transfer-Kodierungen sind
    ///   case-insensitiv (`tchar`/case-insensitive per RFC 9110 5.1); wer nur
    ///   `chunked` annimmt, bricht echte Server.
    /// * `  chunked  ` mit Leerzeichen — die Liste wird getrimmt.
    #[test]
    fn genau_chunked_wird_gelesen() -> Result<(), &'static str> {
        for wert in ["chunked", "Chunked", "  chunked  ", "\tchunked"] {
            let mut leser = leser_mit(b"4\r\nWiki\r\n0\r\n\r\n");
            let h = kopf(&[("transfer-encoding", wert)]);
            let (body, _, quelle) = lese_body(&mut leser, &h, &Limits::default(), false)
                .map_err(|_| "genau chunked wurde abgewiesen")?;
            assert_eq!(body, b"Wiki", "Wert {wert:?} ergab {body:?}");
            // added 02.10.2026 21:5x: `assert!(...)` statt `assert_eq!(...,
            // true, ...)`: clippy meldet `bool_assert_comparison` — dieselbe
            // Lint-Art, die in der fremden Testdatei als Nr. 26 steht.
            assert!(format!("{quelle:?}").contains("Chunk"), "Quelle {quelle:?}");
        }
        Ok(())
    }

    /// added 02.10.2026 21:3x, R41. **`Content-Length` UND
    /// `Transfer-Encoding`**, und hier wird **nichts** geaendert — das ist eine
    /// bewusste Entscheidung, kein Versehen. RFC 9112 6.3 Regel 3 sagt:
    /// „If a message is received with both a Transfer-Encoding and a
    /// Content-Length header field, the Transfer-Encoding overrides the
    /// Content-Length … and **ought to be handled as an error**.“ Das erste
    /// („overrides“) tun wir; das zweite („ought to“) **nicht**, weil echte
    /// Server beides schicken und ein Browser, der daran scheitert, Seiten
    /// verliert. Die Smuggling-Gefahr betrifft **Anfragen**, und die
    /// erzeugen wir selbst — ohne beide Kopfzeilen zugleich.
    #[test]
    fn content_length_und_chunked_wird_nach_chunked_gelesen() -> Result<(), &'static str> {
        let mut leser = leser_mit(b"4\r\nWiki\r\n0\r\n\r\n");
        // added 02.10.2026 21:4x: die `Content-Length` sagt **etwas anderes**
        // (999), damit der Test beweist, dass sie **nicht** das Rahmen ist.
        let h = kopf(&[("transfer-encoding", "chunked"), ("content-length", "999")]);
        let (body, _, _) = lese_body(&mut leser, &h, &Limits::default(), false)
            .map_err(|_| "CL+TE wurde abgewiesen")?;
        assert_eq!(body, b"Wiki", "es wurde nach Content-Length gelesen");
        Ok(())
    }
}

#[cfg(test)]
mod host_einmal_tests {
    use super::Request;

    /// added 02.10.2026 22:1x, R42. **ZAEHLT, statt zu suchen.** Ein Test, der
    /// nur `contains("Host: evil.example")` prueft, ist auch dann gruen, wenn
    /// **zwei** `Host`-Zeilen dastehen — das ist der Fehler, den es zu beheben
    /// gilt. Deshalb wird hier die **Anzahl** gezaehlt.
    fn host_zeilen(bytes: &[u8]) -> Vec<String> {
        let text = String::from_utf8_lossy(bytes);
        text.split("\r\n")
            .filter(|z| {
                let l = z.to_ascii_lowercase();
                l.starts_with("host:")
            })
            .map(|z| z.to_string())
            .collect()
    }

    /// added 02.10.2026 22:1x, R42. **DER VORTRAGSTEST.** Mit einem vom
    /// Aufrufer gesetzten `Host` gab es **zwei** Zeilen; jetzt genau eine, und
    /// sie traegt den Wert des Aufrufers (das ist die `curl -H`-Absicht).
    #[test]
    fn eigener_host_ersetzt_und_erscheint_nur_einmal() {
        let anfrage = Request::get("example.org", 443, "/x").header("Host", "evil.example");
        let zeilen = host_zeilen(&anfrage.to_bytes());
        assert_eq!(zeilen.len(), 1, "es stehen {zeilen:?} da");
        assert_eq!(zeilen[0], "Host: evil.example");
    }

    /// added 02.10.2026 22:1x, R42. **GROSS-/KLEINSCHREIBUNG.** `host`, `HOST`
    /// und `Host` sind nach RFC 9110 5.1 derselbe Kopf. Wer nur auf `"host"`
    /// vergleicht, laesst `HOST` durch und hat wieder zwei.
    #[test]
    fn auch_gross_geschriebenes_host_wird_nicht_verdoppelt() {
        for name in ["host", "HOST", "HoSt"] {
            let anfrage = Request::get("example.org", 443, "/x").header(name, "zweite.example");
            let zeilen = host_zeilen(&anfrage.to_bytes());
            assert_eq!(zeilen.len(), 1, "{name} ergab {zeilen:?}");
        }
    }

    /// added 02.10.2026 22:2x, R42. **DER GEGENFALL**, und er ist der wichtigste:
    /// ohne gesetzten `Host` muss **unserer** kommen, mit Host **und** Port, wenn
    /// der Port nicht der Standard ist. 7.1: „A user agent **MUST** generate a
    /// Host header field in a request“. Wer die Verdopplung durch Weglassen
    /// loest, liefert eine Anfrage **ohne** Host — die ist nach 9112 3.1 auch ein
    /// 400-Fall.
    #[test]
    fn ohne_eigenen_host_kommt_der_unsere_mit_port() {
        let a = Request::get("example.org", 443, "/x");
        let zeilen = host_zeilen(&a.to_bytes());
        assert_eq!(zeilen, vec!["Host: example.org".to_string()]);

        let b = Request::get("example.org", 8080, "/x");
        let zeilen_b = host_zeilen(&b.to_bytes());
        assert_eq!(zeilen_b, vec!["Host: example.org:8080".to_string()]);
    }

    /// added 02.10.2026 22:2x, R42. **DIE ANDEREN GESCHUETZTEN FELDER
    /// MITGEPRUEFT.** `Content-Length` und `Connection` haben denselben Weg
    /// (unser Default gegen den Aufrufer), und dort steht bereits ein
    /// `contains_key`-Test im Code. Diese Probe haelt alle **drei** zusammen,
    /// damit nicht beim naechsten Feld eine Luecke entsteht.
    #[test]
    fn alle_vom_code_geschuetzten_kopfzeilen_kommen_einmal() {
        // added 02.10.2026 22:2x: `Request::post` statt `.body(...)` — GEMESSEN:
        // es gibt **keine** Methode `body`; `post(host, port, pfad, koerper)` ist
        // der Weg, und mein erster Versuch mit `.body(...)` hat nicht
        // uebersetzt.
        let anfrage = Request::post("example.org", 443, "/x", Vec::from("hallo!"))
            .header("Host", "evil.example")
            .header("Content-Length", "7")
            .header("Connection", "close");
        // added 02.10.2026 22:3x: das `&` fehlte — `to_bytes()` gibt `Vec<u8>`
        // zurueck, `from_utf8_lossy` will eine Referenz. Zwei Uebersetzungs-
        // fehler in diesem Modul, beide von mir.
        let text = String::from_utf8_lossy(&anfrage.to_bytes()).to_ascii_lowercase();
        for feld in ["host:", "content-length:", "connection:"] {
            let anzahl = text.split("\r\n").filter(|z| z.starts_with(feld)).count();
            assert_eq!(anzahl, 1, "{feld} kam {anzahl}-mal: {text}");
        }
    }
}

#[cfg(test)]
mod hop_by_hop_tests {
    use super::Request;

    /// added 03.10.2026 00:7x, R46. **ZAEHLT WIEDER, statt zu suchen.** Derselbe
    /// Fehler wie bei `Host`: `contains("transfer-encoding")` waere auch dann
    /// wahr, wenn das Feld **zweimal** dastehe. Geprueft wird die Zeilenzahl.
    fn zeilen(bytes: &[u8], feld: &str) -> Vec<String> {
        let text = String::from_utf8_lossy(bytes).to_ascii_lowercase();
        text.split("\r\n")
            .filter(|z| z.starts_with(&format!("{feld}:")))
            .map(|z| z.to_string())
            .collect()
    }

    /// added 03.10.2026 00:7x, R46. **DER VORTRAGSTEST.** Der Aufrufer sagt
    /// „chunked“ — wir schicken **roh**. Das war der gemessene Zustand:
    /// `transfer-encoding: chunked` **und** `Content-Length: 5` und `hallo`.
    /// Woertlich RFC 9112 6.3 Regel 3 nennt genau dieses Paar einen
    /// Smuggling-Versuch, und hier erzeugten **wir** ihn.
    #[test]
    fn transfer_encoding_verschwindet_aus_der_anfrage() {
        let a = Request::post("example.org", 80, "/x", Vec::from("hallo"))
            .header("Transfer-Encoding", "chunked");
        let bytes = a.to_bytes();
        assert!(
            zeilen(&bytes, "transfer-encoding").is_empty(),
            "Transfer-Encoding ging raus: {:?}",
            zeilen(&bytes, "transfer-encoding")
        );
        // added 03.10.2026 00:7x: **UND `Content-Length` bleibt**, denn der Body
        // ist wirklich 5 Byte lang — wir tauschen eine falsche Angabe nicht
        // gegen eine andere falsche aus.
        assert_eq!(
            zeilen(&bytes, "content-length"),
            vec!["content-length: 5".to_string()]
        );
        assert!(
            String::from_utf8_lossy(&bytes).ends_with("hallo"),
            "der Body kam nicht an"
        );
    }

    /// added 03.10.2026 00:7x, R46. **ALLE VIEL**, denn `hop_by_hop` nennt
    /// sechs Felder und vier davon fliegen hier raus. `keep-alive` und
    /// `proxy-connection` sind reine Verbindungsbeschreibungen;
    /// `transfer-encoding` ist die Gefahr; `upgrade` ist eine Zusage, die wir
    /// nicht einloesen.
    #[test]
    fn alle_verbindungsfelder_fliegen_aus() {
        let a = Request::get("example.org", 80, "/x")
            .header("transfer-encoding", "chunked")
            .header("KEEP-ALIVE", "timeout=5")
            .header("Proxy-Connection", "keep-alive")
            .header("Upgrade", "websocket");
        let bytes = a.to_bytes();
        for feld in [
            "transfer-encoding",
            "keep-alive",
            "proxy-connection",
            "upgrade",
        ] {
            assert!(
                zeilen(&bytes, feld).is_empty(),
                "{feld} ging raus: {bytes:?}"
            );
        }
    }

    /// added 03.10.2026 00:8x, R46. **DER GEGENFALL, und er hat drei Teile.**
    /// Ein Filter, der zu viel wirft, zerlegt echte Anfragen:
    /// * `Connection: close` muss **durch**, sonst kann niemand mehr erzwingen,
    ///   dass die Verbindung endet — und genau das war die Entscheidung aus
    ///   Welle 7 / N4.
    /// * `Host` muss **genau einmal** dastehen (R42).
    /// * ganz normale Felder (`Accept`, `User-Agent`) sind unberuehrt.
    #[test]
    fn connection_host_und_gewöhnliche_felder_bleiben() {
        let a = Request::get("example.org", 8080, "/x")
            .header("Connection", "close")
            .header("Accept", "text/html")
            .header("User-Agent", "narsil/0.1");
        let bytes = a.to_bytes();
        assert_eq!(
            zeilen(&bytes, "connection"),
            vec!["connection: close".to_string()]
        );
        assert_eq!(
            zeilen(&bytes, "host"),
            vec!["host: example.org:8080".to_string()]
        );
        assert_eq!(
            zeilen(&bytes, "accept"),
            vec!["accept: text/html".to_string()]
        );
        assert_eq!(
            zeilen(&bytes, "user-agent"),
            vec!["user-agent: narsil/0.1".to_string()]
        );
    }

    /// added 03.10.2026 00:8x, R46. **OHNE KOERPER**, und das ist der Fall, in
    /// dem ein `Transfer-Encoding` besonders gefaehrlich waere: eine Anfrage
    /// **ohne** Body, die „chunked“ sagt, wartet beim Server auf Chunks, die nie
    /// kommen. GEMESSEN wurde er in der Form „mit Body“ (Abschnitt 1 des
    /// Berichts); dieser Test deckt die andere Form ab.
    #[test]
    fn ohne_koerper_kein_transfer_encoding() {
        let a = Request::get("example.org", 80, "/x").header("transfer-encoding", "chunked");
        let bytes = a.to_bytes();
        assert!(zeilen(&bytes, "transfer-encoding").is_empty());
        assert!(
            zeilen(&bytes, "content-length").is_empty(),
            "wir melden keine Laenge an"
        );
    }
}

#[cfg(test)]
mod crl_live {
    // added 03.10.2026 18:0x, R61. **DIESER TEST LIEGT JETZT IN `src/lib.rs`, UND
    // DAS IST EINE KORREKTUR AN MIR.** R60 hat ihn als `crates/narsil-net/tests/
    // crl_live.rs` angelegt — und `narsil-net/tests/` ist **nicht meine Datei**
    // (GEMESSEN: `team/STATUS_TLS.txt` sagt das an drei Stellen ausdrücklich, u. a.
    // Z. 2290: „`narsil-net/tests/` ist nicht meine Datei“). Ich habe also eine
    // Fremddatei geschrieben und es nicht gemerkt: die Fehlerform aus R50/R51 —
    // eine **Rückgabe wird geliefert und weggelegt**, hier die **Dateigrenze**.
    //
    // added 03.10.2026 18:0x: **UND DIE KORREKTUR HAT EINEN PREIS, DEN ICH BENENNE.**
    // Als `#[cfg(test)]`-Modul in `src/lib.rs` ist der Test Teil der Lib und wird
    // **nicht** mehr als eigenes Test-Binary gelistet; `bau.sh narsil-net test` zählt
    // ihn also in den Lib-Zähler statt in eine eigene Datei. Das ist **kein**
    // Verlust: der Aufruf bleibt derselbe, nur die Zählung wandert.
    //
    // added 03.10.2026 18:0x: **NETZTEST: die CRL-Beschaffung gegen echte
    // Verteilungspunkte.** R60 baut den ersten HTTP-Abruf, den diese Bibliothek für
    // eine **fremde** Ressource macht. GEMESSEN (R60 16:0x, `curl`): 9 von 10
    // geprüften echten Zertifikaten tragen eine CRL-Distribution-Point-URL und
    // antworten mit **HTTP 200** (186 kB, 332 kB). Die Frage ist also nicht „ob es
    // CRLs gibt“, sondern „liest **unser** Abruf sie genauso wie `curl`“ — und das
    // ist eine Frage über **unseren** Code, nicht über die Server.
    //
    // added 03.10.2026 18:0x: **AUFRUF** (immer mit Zeitlimit von aussen):
    //   werkzeuge/bau.sh narsil-net test --lib crl_live -- --ignored --nocapture

    /// added 03.10.2026 16:bx, R60: **ZWEI ECHTE URLs**, und sie sind **gemessen**
    /// (16:0x) und nicht aus dem Gedächtnis: Let's Encrypt (`yr1.c.lencr.org`, 186 kB)
    /// und Google (`c.pki.goog`, 332 kB). Beide sind **HTTP im Klartext** — eine
    /// CRL im Klartext über HTTP ist hier **Normalfall**, nicht ein Sonderfall.
    ///
    /// added 03.10.2026 16:bx: **UND ZWEI ABSICHTLICH FALSCHE**, weil ein Test, der
    /// nur die gute Hälfte prüft, von einem Abruf erfuüllt wäre, der **immer**
    /// irgendetwas zurückgibt.
    const URLS: [&str; 4] = [
        "http://yr1.c.lencr.org/3.crl",
        "http://c.pki.goog/wr3/VXgRL90Sqwg.crl",
        "http://crt.example.invalid/nicht-da.crl",
        "https://yr1.c.lencr.org/3.crl",
    ];

    // added 03.10.2026 16:ca: **`#[test]` FEHLTE**, und der Lauf meldete
    // „0 passed“ bei **gruener** Ausgabe. Das ist genau die Form aus R51
    // (`krissch_live`, 05:4x) — und sie ist inzwischen zum **Muster** geworden:
    // Ein Netztest ohne `#[test]` sieht aus wie ein bestandener Test. Deshalb steht
    // der Grund **hier** und nicht in einer Liste.
    #[test]
    #[ignore = "braucht Internet; bewusst nicht Teil des normalen Laufs"]
    fn hole_crl_holt_die_wirklichkeit() {
        let mut geholt = 0usize;
        let mut abgelehnt = 0usize;
        for url in URLS {
            match super::hole_crl(url) {
                Ok(bytes) => {
                    geholt += 1;
                    // added 03.10.2026 16:bx: **NICHT NUR „nicht leer“** — die
                    // Antwort muss auch **eine CRL sein**. GEMESSEN (16:0x): 186 kB
                    // und 332 kB. Eine Fehlermeldung mit 200-Status ( manche
                    // Server liefern „not found“ als HTML mit 200 ) wäre sonst ein
                    // Erfolg.
                    assert!(
                        bytes.len() > 1000,
                        "die Antwort von {url} ist {} Byte — das ist keine CRL",
                        bytes.len()
                    );
                    // added 03.10.2026 16:by: **DER ERSTE BYTE EINER CRL**: eine
                    // `CertificateList` beginnt mit `SEQUENCE` (0x30) und einer
                    // Laenge. Das ist die Form, und sie wird **geprueft**, nicht
                    // angenommen — ein HTML-Dokument beginnt mit `<`.
                    assert_eq!(
                        bytes[0], 0x30,
                        "die Antwort von {url} beginnt nicht mit SEQUENCE"
                    );
                    println!("GEHOLT   {url:52} {} Byte", bytes.len());
                }
                Err(e) => {
                    abgelehnt += 1;
                    println!("GELEHNT  {url:52} {e}");
                }
            }
        }
        // added 03.10.2026 16:by: **BEIDE HALFTEN**, und das ist der Kern: eine CRL
        // holen **und** eine falsche ablehnen. Ein Test, der nur den ersten Teil
        // prüft, wäre von einem Abruf erfuellt, der alles annimmt.
        assert_eq!(
            geholt, 2,
            "die beiden echten URLs wurden nicht beide geholt"
        );
        assert_eq!(
            abgelehnt, 2,
            "die beiden falschen URLs wurden nicht beide abgelehnt"
        );
    }

    /// added 03.10.2026 16:d2, R60: **DIE GANZE KETTE, ENDE ZU ENDE.** Der Test
    /// oben beweist, dass wir die **Bytes** bekommen; dieser beweist, dass unser
    /// **Parser** sie liest und unser **Prüfer** sie auswertet. Ein Download ohne
    /// Parse wäre die Form, die R50 geschlagen hat — der Stapel kam an und wurde nie
    /// gelesen.
    #[test]
    #[ignore = "braucht Internet; bewusst nicht Teil des normalen Laufs"]
    fn die_echte_crl_wird_geprueft() {
        // added 03.10.2026 16:d5: **`xkcd.com`, UND DER GRUND STEHT HIER.** Mein
        // erster Host war `crl.pts.owasp.org` — der kommt im **Systemtruststore**
        // **nicht** vor (GEMESSEN: „failed to lookup address information“ hieß hier
        // tatsächlich „der Host ist nicht im Store“ — nein, es hieß DNS; aber der
        // Punkt bleibt: ein **beliebiger** Host taugt nicht, sondern nur einer, dessen
        // Aussteller im **eigenen** Truststore liegt). `xkcd.com` ist aus R50/R58
        // bekannt und haengt an Let's Encrypts `YR1` — und **dessen** CRL haben wir
        // oben mit **186 kB** geholt. Damit prueft derselbe Test **Download und
        // Auswertung** an **einem** Host, und die beiden sind nicht unabhaengig.
        //
        // added 03.10.2026 16:d3: **DIE ZERTIFIKATE VOM ECHTEN HOST** — und sie
        // kommen aus dem Handshake, den unser eigener Client macht, nicht aus
        // `openssl`. Das ist der Unterschied: `openssl` koennte eine andere Auswahl
        // treffen, und dann pruefte der Test etwas anderes als unseren Weg.
        let speicher = {
            const PFAD: &str = "/etc/ssl/certs/ca-certificates.crt";
            let text = std::fs::read_to_string(PFAD)
                .unwrap_or_else(|e| panic!("{PFAD} nicht lesbar ({e})"));
            narsil_tls::kette::Vertrauensspeicher::aus_pem(&text).expect("Truststore unbrauchbar")
        };
        let jetzt = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let socket = std::net::TcpStream::connect(("xkcd.com", 443))
            .unwrap_or_else(|e| panic!("Verbindung: {e}"));
        let _ = socket.set_read_timeout(Some(std::time::Duration::from_secs(20)));
        let _ = socket.set_write_timeout(Some(std::time::Duration::from_secs(20)));
        let verbindung = narsil_tls::strom::TlsVerbindung::ueber_sockel(
            socket,
            "xkcd.com",
            &[b"http/1.1"],
            &speicher,
            jetzt,
        )
        .unwrap_or_else(|e| panic!("Handshake: {e:?}"));
        let kette = verbindung.kette();
        // added 03.10.2026 16:d4: **DIE KETTE IST NICHT LEER**, und das ist die
        // Voraussetzung: `hole_crl` braucht **zwei** Zertifikate (Blatt und
        // Aussteller). GEMESSEN in R60: `kette_der` war bis eben **verworfen**, also
        // war diese Prüfung nicht möglich.
        assert!(
            kette.len() >= 2,
            "die Kette hat nur {} Zertifikate",
            kette.len()
        );
        // added 03.10.2026 16:dc: **WELCHES ZERTIFIKAT DIE QUELLE TRAEGT** — und
        // das ist eine **Messung**, die ich beim Lesen brauche: `xkcd.com` schickt
        // **drei** Zertifikate (Blatt, Zwischen-CA, GlobalSign-Wurzel), und ich
        // weiss noch nicht, an welcher Stelle die Verteilungspunkte stehen.
        for (i, der) in kette.iter().enumerate() {
            let z = narsil_tls::x509::zertifikat_lesen(der).unwrap_or_else(|_| panic!("lesen {i}"));
            let quellen = narsil_tls::x509::verteilungsstellen_als_texte(&z);
            // added 03.10.2026 16:dd: **DIE ROHEN VERTEILUNGSPUNKTE** mit, weil
            // `quellen = []` **zwei** Ursachen haben kann: die Erweiterung fehlt,
            // oder `verteilungsstellen_lesen` gab leer zurueck. Die Zahl
            // unterscheidet das **ohne** weiteres Raten.
            println!(
                "KETTE[{i}] subject={:?} issuer={:?}",
                z.inhaber, z.aussteller
            );
            println!(
                "KETTE[{i}] ca={:?} dp_roh={:?} quellen={:?}",
                z.basic_constraints.as_ref().map(|b| b.ist_ca),
                z.verteilungsstellen.len(),
                quellen
            );
        }
        // added 03.10.2026 18:1x, R61. **DIE ZEITMESUNG, und sie entscheidet
        // ueber den Default.** `crl_status_pruefen` macht einen **blockierenden**
        // HTTP-Abruf (bis `MAX_CRL_QUELLEN` Quellen, je `CRL_ZEITGRENZE`). Ohne
        // Zahl ist die Frage „darf das im Client-Weg laufen?“ nicht zu
        // beantworten — mit ihr schon.
        let vorher = std::time::Instant::now();
        let urteil = super::crl_status_pruefen(kette);
        println!(
            "R61-KOSTEN {} ms, Urteil {:?}",
            vorher.elapsed().as_millis(),
            urteil
        );
        println!("URTEIL {urteil:?} bei {} Zertifikaten", kette.len());
        // added 03.10.2026 16:d4: **NICHT „CRL GEPRUEFT“** — der Punkt dieser Runde
        // ist, dass ein Server **auch ohne** Verteilungspunkt vorkommt. Also ist das
        // Urteil **kein** Fehler; es wird **gemeldet**.
        assert_ne!(
            urteil,
            super::CrlUrteil::Widerrufen,
            "ein gueltiges Zertifikat gilt als widerrufen"
        );
    }
}
