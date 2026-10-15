//! HTTPS ueber `narsil_tls::server` — echter Anschluss, kein Stub mehr.
//!
//! added 01.10.2026, R3 (Auftrag aus `team/AUTONOM.txt`, R3: "HTTPS in
//! narsil-server einbauen (Codex S2): `crates/narsil-server/src/tls.rs` vom
//! Stub zum echten Anschluss an `narsil_tls::server`").
//!
//! VORHER stand hier eine Datei, die `https://` **erkannte und ablehnte** —
//! mit dem Argument, TLS sei ein eigenes Projekt. Das Argument war richtig
//! und ist ueberholt: `narsil_tls::server` steht (R1d, mit `openssl s_client`
//! als Zeuge, `team/BERICHT_TLS_R1d.txt`). Was jetzt hier steht, ist die
//! **Verdrahtung**: derselbe Router, dieselben Handler, nur der Sockel wird
//! zur TLS-Sitzung.
//!
//! DER WEG, in drei Schritten:
//! 1. `narsil_tls::server::TlsServer::handshake` ueber den Sockel.
//! 2. `AnwendungsStrom` daraus — der ist `Read` **und** `Write` (R3, in
//!    `narsil-tls/src/server.rs`).
//! 3. Zwei duenne Haelften ueber **einen** `Mutex`, damit `bediene` beide
//!    Merkmale bekommt und beide an denselben TLS-Strom gehen.
//!
//! WARUM EIN `MUTEX` UND NICHT `std::io::split`: `split` ist seit
//! Rust 1.63 **unstable** (`io_split`), und es gibt hier keinen
//! nightly. Die Alternative waere ein zweiter `AnwendungsStrom` mit
//! eigenen Sequenznummern gewesen — und genau das waere der Fehler, den
//! man auf localhost **nicht** sieht: die erste Anfrage ginge durch, die
//! zweite schlaegt mit einem AEAD-Fehler fehl, weil Sende- und
//! Empfangszaehler auseinanderlaufen. Der `Mutex` haelt **einen** Strom,
//! also **einen** Zaehlersatz.

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::rc::Rc;
use std::time::Duration;

use narsil_tls::server::{AnwendungsStrom, ServerAngebot, ServerFehler, TlsServer};

/// Wie lange `https_laufen_uf` zwischen zwei `accept`-Versuchen wartet,
/// wenn nichts kommt (added SERVER V14).
///
/// DER WERT, und er ist eine Abwaegung: 200 ms ist lang genug, dass
/// ein Localhost-Nutzer nichts bemerkt (die CPU schlaeft in dieser
/// Zeit, es ist kein Warten auf Arbeit), und kurz genug, dass
/// `herunterfahren` in unter einer Viertelsekunde durchkommt. Groesser
/// macht den Shutdown trage, kleiner macht die Schleife heiss.
pub const ANNAHME_PRUEFUNG_MS: u64 = 200;

/// M3: so viele Verbindungen bedient [`https_laufen_uf`] hoechstens.
///
/// added 01.10.2026, R3. DER WERT 64: genug fuer einen Test und fuer ein
/// Skript mit `curl`, das den Server nach dem Beweis beenden will. `0`
/// heisst "unbegrenzt" (dann braucht es ein `stoppen()` von aussen).
pub const MAX_VERBINDUNGEN_HTTPS: usize = 64;

/// Was der HTTPS-Server anbietet.
///
/// added 01.10.2026, R3. Es sind genau die drei Dinge, die ein TLS-Server
/// braucht: Zertifikat, privater Schluessel, ALPN-Liste. Die Grenzen prueft
/// `narsil_tls::server::ServerAngebot::pruefen` — eine zweite Pruefung hier
/// waere eine zweite Wahrheit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsAngebot {
    /// Das Zertifikat in DER-Form (ein Blatt oder eine Kette).
    pub zertifikat: Vec<u8>,
    /// Der private Schluessel (P-256, 32 Byte).
    pub privat: Vec<u8>,
    /// Die ALPN-Protokolle in der Reihenfolge der Vorliebe des Servers.
    pub alpn: Vec<Vec<u8>>,
}

impl TlsAngebot {
    /// Baut ein Angebot mit `http/1.1` und `h2`.
    ///
    /// added 01.10.2026, R3. DIE REIHENFOLGE, und sie ist **nicht**
    /// `h2` zuerst: der HTTP-Pfad spricht heute HTTP/1.1, und ein Server,
    /// dass `h2` zuerst anbietet, wuerde von einem HTTP/2-Client auf
    /// `h2` gesetzt bekommen und dann **HTTP/1.1**-Bytes in `h2`-Frames
    /// erwarten. Das ist genau die Falle, vor der RFC 9113 3.1 warnt
    /// (ALPN muss zur tatsaechlich gesprochenen Version passen).
    ///
    /// **STAND 01.10.2026, SERVER V2:** HTTP/2 steht (siehe
    /// [`crate::h2`]), und der Standard ist deshalb
    /// [`TlsAngebot::h2_dann_http1`]. Diese Fabrik bleibt fuer
    /// Aufrufer, die HTTP/1.1 **bevorzugen wollen** (etwa weil ihr
    /// eigener Client noch kein `h2` kann) — sie ist nicht mehr der
    /// Standard, aber sie ist nicht falsch.
    pub fn http1_dann_h2() -> TlsAngebot {
        TlsAngebot {
            zertifikat: Vec::new(),
            privat: Vec::new(),
            alpn: vec![b"http/1.1".to_vec(), b"h2".to_vec()],
        }
    }

    /// Baut ein Angebot mit `h2` und `http/1.1`.
    ///
    /// added 01.10.2026, SERVER V2 (S7). **DAS IST JETZT DIE
    /// VORZUGSREIHENFOLGE**, und der Kommentar in
    /// [`TlsAngebot::http1_dann_h2`] sagte seit R3 genau das voraus:
    /// "Sobald HTTP/2 steht, dreht die Reihenfolge um - mit einem
    /// Kommentar hier, damit die Aenderung nicht still passiert."
    /// Sie passiert hier, und der Test
    /// `alpn_steht_auf_h2_zuerst_weil_der_server_h2_spricht` haelt sie
    /// fest.
    ///
    /// DER BEWEIS, dass der Server `h2` wirklich spricht, ist nicht
    /// dieses Stueck Text, sondern `curl -k --http2
    /// https://127.0.0.1:PORT/` (siehe `team/BERICHT_SERVER_V2.txt`).
    ///
    /// ACHTUNG: die Fabrik liefert **nur die ALPN-Liste**. Zertifikat
    /// und privater Schluessel kommen vom Aufrufer:
    /// `angebot.zertifikat = ...; angebot.privat = ...;`. Ohne sie sagt
    /// [`ServerAngebot::pruefen`] "leeres Zertifikat" — der Server
    /// nimmt dann eine Verbindung an und beendet sie sofort, was beim
    /// Beispiel erst im Log auffaellt.
    pub fn h2_dann_http1() -> TlsAngebot {
        TlsAngebot {
            zertifikat: Vec::new(),
            privat: Vec::new(),
            alpn: vec![b"h2".to_vec(), b"http/1.1".to_vec()],
        }
    }

    /// Das Angebot als `narsil_tls::server::ServerAngebot`, geprueft.
    fn als_server(&self) -> Result<TlsServer, TlsFehler> {
        let a = ServerAngebot {
            zertifikat: self.zertifikat.clone(),
            privat: self.privat.clone(),
            alpn: self.alpn.clone(),
        };
        TlsServer::neu(a).map_err(|e| TlsFehler::Angebot(e.to_string()))
    }
}

/// Warum HTTPS nicht zustande kam.
///
/// added 01.10.2026, R3. DER GRUND FUER DREI VARIANTEN: es gibt jetzt
/// drei **verschiedene** Zeitpunkte, an denen es scheitern kann — das
/// Angebot (falsches Zertifikat), der Handshake (der Client) und die
/// Leitung. Ein einziger Fehlername wuerde sagen "irgendwas", und die
/// Suche nach der Ursache faengt bei "unbekannter Fehler" an.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TlsFehler {
    /// Das Angebot selbst ist unbrauchbar (leeres Zertifikat, falsche
    /// Schluessellaenge, zu viele ALPN-Protokolle).
    Angebot(String),
    /// Der Handshake mit dem Client ist gescheitert.
    Handshake(String),
    /// Fehler der Leitung darunter.
    Netz(String),
}

impl std::fmt::Display for TlsFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TlsFehler::Angebot(was) => write!(f, "das TLS-Angebot ist unbrauchbar: {was}"),
            TlsFehler::Handshake(was) => write!(f, "der TLS-Handshake scheiterte: {was}"),
            TlsFehler::Netz(was) => write!(f, "Netzfehler bei HTTPS: {was}"),
        }
    }
}

impl std::error::Error for TlsFehler {}

impl From<ServerFehler> for TlsFehler {
    fn from(e: ServerFehler) -> TlsFehler {
        TlsFehler::Handshake(e.to_string())
    }
}

/// Fehler, wenn jemand HTTPS verlangt, aber es ist **kein** TLS-Server
/// eingerichtet.
///
/// added 01.10.2026, R3. DER TYP BLEIBT, aus zwei Gruenden: (a) fremder
/// Code matcht auf `TlsFehlt`, und (b) die Meldung ist weiterhin
/// richtig — `braucht_tls` sagt, dass eine Adresse TLS **braucht**, nicht
/// dass es geht. Neu ist nur, dass die Meldung nicht mehr behauptet,
/// HTTPS sei nicht gebaut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsFehlt {
    /// Was verlangt wurde (`https://...` oder `wss://...`).
    pub adresse: String,
}

impl std::fmt::Display for TlsFehlt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: Diese Adresse braucht TLS, aber hier laeuft der Klartext-Server. \
             Starte den Server mit `https_laufen_uf` (siehe narsil_server::tls) \
             oder benutze http://.",
            self.adresse
        )
    }
}

impl std::error::Error for TlsFehlt {}

/// Prueft eine Adresse auf TLS-Bedarf.
///
/// added 01.10.2026, R3, unveraendert in der **Frage**: `Ok` heisst "kein
/// TLS noetig" (`http://`, `ws://`, Pfade), `Err` heisst "TLS noetig"
/// (`https://`, `wss://`). Ob TLS zur Verfuegung steht, entscheidet der
/// Aufrufer: der Klartext-Server ruft hier und antwortet mit `TlsFehlt`,
/// der HTTPS-Server gar nicht.
pub fn braucht_tls(adresse: &str) -> Result<(), TlsFehlt> {
    let klein = adresse.trim().to_ascii_lowercase();
    if klein.starts_with("https://") || klein.starts_with("wss://") {
        Err(TlsFehlt {
            adresse: adresse.to_string(),
        })
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Die zwei Haelften ueber einem Strom
// ---------------------------------------------------------------------------

/// Die Lesehaelfte: `Read` ueber den gemeinsamen TLS-Strom.
///
/// added 01.10.2026, R3. Sie ist ein `Mutex` und sonst nichts — die
/// Sperre wird pro Lesevorgang genommen und sofort wieder freigegeben,
/// damit ein Handler, der waehrend des Lesens schreibt, nicht auf sich
/// selbst wartet.
///
/// **OEFFENTLICH**, und nicht aus Versehen: `HttpsVerbindung` gibt sie
/// ueber `leser()`/`teile()` zurueck, und ein oeffentlicher Typ, der
/// einen privaten zurueckgibt, ist eine Warnung des Compilers
/// ("more private than the item"). Die Alternative waere, die Rueckgabe
/// zu verstecken — dann koennte aber niemand `bediene` von Hand rufen,
/// und genau das ist der Weg, den ein Aufrufer mit eigenem
/// Lese-Zyklus braucht.
pub struct LeserHaelfte<'s> {
    gemeinsam: Rc<RefCell<AnwendungsStrom<'s>>>,
}

impl Read for LeserHaelfte<'_> {
    fn read(&mut self, ziel: &mut [u8]) -> std::io::Result<usize> {
        // DIE LEIHEN, und `try_borrow_mut` statt `borrow_mut`: waere
        // die Zelle gerade anderweitig geliehen (etwa weil ein Handler
        // mitten im Lesen schreibt und wieder liest), wuerde
        // `borrow_mut` **panicken** und den Faden abwuergen. Als
        // `io::Error` bleibt es ein Fehler, an dem man etwas sehen kann.
        let mut strom = self.gemeinsam.try_borrow_mut().map_err(|_| {
            std::io::Error::other("der TLS-Strom ist gerade in Gebrauch (Reentrant-Lesen)")
        })?;
        strom.read(ziel)
    }
}

/// Die Schreibhaelfte: `Write` ueber denselben TLS-Strom.
///
/// added 01.10.2026, R3. Siehe `LeserHaelfte` (ein Strom, ein
/// Sequenzzahlsatz; oeffentlich, weil `HttpsVerbindung` sie zurueckgibt).
/// `flush` ist **absichtlich** leer — `AnwendungsStrom` schreibt jeden
/// Record sofort auf die Leitung, es gibt also nichts zu treiben. Ein
/// `flush` mit Wartezeit wuerde hier nur eine zweite Stelle sein, an der
/// ein haengender Server entsteht.
pub struct SchreiberHaelfte<'s> {
    gemeinsam: Rc<RefCell<AnwendungsStrom<'s>>>,
}

impl Write for SchreiberHaelfte<'_> {
    fn write(&mut self, daten: &[u8]) -> std::io::Result<usize> {
        let mut strom = self.gemeinsam.try_borrow_mut().map_err(|_| {
            std::io::Error::other("der TLS-Strom ist gerade in Gebrauch (Reentrant-Schreiben)")
        })?;
        strom.write(daten)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Das Ergebnis eines gelungenen [`https_aufbauen`]: alles, was
/// [`https_bediene_eine`] fuer den HTTP-Pfad braucht.
///
/// added 01.10.2026, R3. Es ist ein eigener Typ und kein Tupel, weil an
/// drei Stellen darauf zugegriffen wird und ein `let (a, b) = ...` an der
/// dritten Stelle in `b` ohne Bedeutung ist.
pub struct HttpsVerbindung<'s> {
    /// Gepuffertes Lesen ueber den TLS-Strom.
    leser: BufReader<LeserHaelfte<'s>>,
    /// Schreiben ueber denselben TLS-Strom.
    schreiber: SchreiberHaelfte<'s>,
    /// Das ausgehandelte ALPN-Protokoll (`None`, wenn der Client keines
    /// geschickt hat).
    ///
    /// added 01.10.2026, SERVER V2. DER GRUND FUER DAS FELD: `h2` darf
    /// auf einer Verbindung **nur** laufen, wenn der Client es
    /// ausgehandelt hat (RFC 7301 3.2: ein Server, der `h2` anbietet
    /// und es nicht bekommt, redet HTTP/1.1). Ohne dieses Feld muesste
    /// `https_bediene_eine` den Handshake selbst wiederholen — und
    /// damit zwei TLS-Sitzungen auf einem Sockel bauen.
    pub alpn: Option<Vec<u8>>,
}

impl HttpsVerbindung<'_> {
    /// Wurde `h2` ausgehandelt?
    ///
    /// added 01.10.2026, SERVER V2. DER WEG: Byte-fuer-Byte-Vergleich
    /// auf `b"h2"` — die Kennung ist nach RFC 7301 **immer** genau
    /// `h2`, ohne Version und ohne Zusatz.
    pub fn ist_h2(&self) -> bool {
        self.alpn.as_deref() == Some(b"h2".as_slice())
    }
}

impl<'s> HttpsVerbindung<'s> {
    /// Der Leser, fuer `Server::bediene`.
    ///
    /// added 01.10.2026, R3. DER TYP HAT EINE LEBENSZEIT, und sie ist die
    /// des **Sockels** (`'s`): die Haelften muessen nicht laenger leben
    /// als die Anleihe, die `AnwendungsStrom` darin haelt.
    pub fn leser(&mut self) -> &mut BufReader<LeserHaelfte<'s>> {
        &mut self.leser
    }
    /// Der Schreiber, fuer `Server::bediene`.
    pub fn schreiber(&mut self) -> &mut SchreiberHaelfte<'s> {
        &mut self.schreiber
    }
    /// Beide Haelften **gleichzeitig**, als zwei Felder.
    ///
    /// added 01.10.2026, R3. DER GRUND FUER DEN ZWEITEN WEG: zwei
    /// `&mut`-Methoden auf dieselbe Struktur kann man nicht gleichzeitig
    /// halten ("cannot borrow as mutable more than once"). Die beiden
    /// **Felder** sind aber getrennt, und deshalb darf man sie zusammen
    /// herausgeben — das ist der ganze Trick, und er ist der Grund, warum
    /// der Zustand in zwei Feldern und nicht in einem `Option` steckt.
    pub fn teile(&mut self) -> (&mut BufReader<LeserHaelfte<'s>>, &mut SchreiberHaelfte<'s>) {
        (&mut self.leser, &mut self.schreiber)
    }
}

/// Baut aus einem angenommenen Sockel die beiden Haelften.
///
/// added 01.10.2026, R3. DIE ZEITGRENZEN, und sie kommen **vor** dem
/// Handshake auf den Sockel: ein Handshake ohne Grenze ist der haeufigste
/// Weg, auf dem ein HTTPS-Server haengt (der Client verbindet sich und
/// schweigt). Danach setzt `AnwendungsStrom` nichts mehr — die Fristen
/// bleiben die des Sockels, und `AnwendungsStrom` hat ausdruecklich keine
/// eigenen.
pub fn https_aufbauen<'s>(
    stream: &'s mut TcpStream,
    angebot: &TlsAngebot,
    lese_frist: Duration,
) -> Result<HttpsVerbindung<'s>, TlsFehler> {
    let _ = stream.set_read_timeout(Some(lese_frist));
    let _ = stream.set_write_timeout(Some(lese_frist));
    let _ = stream.set_nodelay(true);
    let server = angebot.als_server()?;
    let ausgehandelt = server.handshake(stream)?;
    let strom = AnwendungsStrom::neu(stream, &ausgehandelt);
    // DER `Rc`/`RefCell`, und die Wahl ist eine **Aussage**:
    //
    // * `Rc` statt `Arc`, weil clippy einen `Arc` verlangt, dessen Inhalt
    //   `Send + Sync` ist — und `AnwendungsStrom` haelt eine Anleihe
    //   (`&mut dyn LeseSchreiben`), die das nicht ist. Der `Arc` wuerde
    //   etwas versprechen, das der Typ nicht liefern kann.
    // * `Rc` reicht, weil der HTTPS-Weg **pro Verbindung** arbeitet und
    //   damit in **einem** Faden: `https_bediene_eine` baut die Haelften
    //   und gibt sie im selben Aufruf an `bediene` weiter.
    // * `Rc` ist noetig (statt einer lokalen Zelle, die man ausleihen
    //   wuerde), weil die Zelle sonst beim Zurueckgeben der Funktion
    //   weggaellt: "cannot return value referencing local variable".
    let gemeinsam = Rc::new(RefCell::new(strom));
    Ok(HttpsVerbindung {
        leser: BufReader::new(LeserHaelfte {
            gemeinsam: Rc::clone(&gemeinsam),
        }),
        schreiber: SchreiberHaelfte { gemeinsam },
        // added 01.10.2026, SERVER V2: die Kennung wandert aus dem
        // Handshake hierher — der Aufrufer entscheidet, welches
        // Protokoll er darauf spricht.
        alpn: ausgehandelt.alpn,
    })
}

/// Nimmt **eine** HTTPS-Verbindung an und bedient sie.
///
/// added 01.10.2026, R3. DAS IST DER BAUSTEIN, aus dem [`https_laufen_uf`]
/// besteht, und der einzige, den ein Test braucht: der `TcpListener` kommt
/// von aussen, damit im Test kein Port belegt werden muss.
///
/// DIE ANZAHL ANFRAGEN, und sie kommt aus `server.config().max_anfragen`
/// statt aus einer eigenen Zahl: `max_anfragen` ist die Keep-Alive-Grenze
/// des Klartext-Weges, und ein HTTPS-Server mit einer anderen Grenze waere
/// eine zweite Wahrheit ueber dieselbe Sache.
///
/// DER ENDE, und er ist nicht "Fehler = auf": wenn der Client die
/// Verbindung schliesst (EOF), endet die Schleife **ohne** Fehler. Das ist
/// der normale Fall bei `curl` nach der Antwort und darf kein Fehler sein.
pub fn https_bediene_eine(
    server: &crate::Server,
    stream: &mut TcpStream,
    angebot: &TlsAngebot,
    peer: &str,
) -> Result<(), TlsFehler> {
    let frist = server.config().lese_timeout;
    let mut verbindung =
        https_aufbauen(stream, angebot, frist).map_err(|e| TlsFehler::Handshake(e.to_string()))?;
    // added 01.10.2026, SERVER V2: DER H2-ZWEIG. Er steht **vor** der
    // HTTP/1.1-Schleife, und der Grund ist die Natur von HTTP/2: der
    // Client schickt nach dem Handshake die 24-Byte-Vorfahrt
    // (`PRI * HTTP/2.0`), und jeder Versuch, das als HTTP/1.1 zu lesen,
    // endet mit `400` — der Client wuerde es nie sehen.
    if verbindung.ist_h2() {
        let (leser, schreiber) = verbindung.teile();
        return match crate::h2::h2_bediene(server, leser, schreiber, peer) {
            Ok(antworten) => {
                eprintln!("h2: {peer}: {antworten} Antworten");
                Ok(())
            }
            Err(e) => {
                // Ein Fehler beendet die **Verbindung**, nicht den
                // Server: der naechste `accept` ist eine neue.
                eprintln!("h2: Verbindung von {peer} beendet: {e}");
                Ok(())
            }
        };
    }
    let max = server.config().max_anfragen;
    let mut nummer = 0usize;
    // added 01.10.2026, SERVER V1 (S3): dieselben beiden Puffer wie im
    // Klartext-Server, aus demselben Grund — sie leben ueber alle
    // Anfragen der Verbindung, nicht ueber eine.
    let mut puffer = crate::http::LesePuffer::neu(peer);
    let mut kopf = String::with_capacity(crate::server::KOPF_PUFFER_BYTES);
    while nummer < max {
        // DER ENDE-CHECK, und er ist ein eigener Aufruf: `bediene` liest
        // eine ganze Anfrage; steht am Ende nur noch `close_notify`,
        // gibt es keine Anfrage und `bediene` antwortet mit einem Fehler.
        let (leser, schreiber) = verbindung.teile();
        if ist_zu(leser) {
            break;
        }
        let weiter = match server
            .anfrage_verarbeiten(
                leser,
                schreiber,
                &mut puffer,
                &mut kopf,
                peer,
                nummer + 1,
                true,
            )
            .ok()
        {
            Some(w) => w,
            None => break,
        };
        nummer += 1;
        if !weiter {
            break;
        }
    }
    Ok(())
}

/// Steht in der Verbindung nur noch ein Endezeichen?
///
/// added 01.10.2026, R3. DER WEG: ein `fill_buf` ueber `BufRead` und
/// `Puffer().is_empty()`. Das ist **kein** Lesevorgang auf dem TLS-Strom
/// (der wuerde einen Record entschluesseln und den Zaehler bewegen), nur
/// ein Blick in den Puffer — deshalb `fill_buf` und nicht `read`. Fuer den
/// Sockel macht `server.rs` dasselbe ueber `peek` (`warte_auf_anfrage`).
fn ist_zu<R: BufRead>(leser: &mut R) -> bool {
    match leser.fill_buf() {
        Ok(b) => b.is_empty(),
        Err(_) => true,
    }
}

/// Laeuft ein HTTPS-Server auf einem `TcpListener`.
///
/// added 01.10.2026, R3. `max_verbindungen == 0` heisst "bis zum
/// `stoppen()` von aussen"; sonst werden hoechstens so viele Verbindungen
/// angenommen und der Server kehrt zurueck. Das ist die Form, die ein
/// Skript braucht: es startet den Server, macht ein `curl -k` und beendet
/// ihn danach, ohne einen Restprozess zu hinterlassen.
///
/// DAS ANGEBOT WIRD VOR DER SCHLEIFE GEPRUEFT: ein unbrauchbares
/// Zertifikat soll nicht erst nach dem ersten `accept` auffallen.
pub fn https_laufen_uf(
    server: &crate::Server,
    horcher: &TcpListener,
    angebot: &TlsAngebot,
    max_verbindungen: usize,
) -> Result<(), TlsFehler> {
    angebot.als_server()?;
    let mut gezaehlt = 0usize;
    // added 02.10.2026, SERVER V14: DER `accept` WAR BLOCKIEREND, und
    // damit konnte `herunterfahren` nicht durchkommen.
    //
    // GEMESSEN: `examples/serve_https.rs --pruefen` beantwortete alle
    // sechs Pruefungen mit „JA" und lief dann in den Timeout (rc=124).
    // Grund: `herunterfahren(frist)` setzt `stoppen`, und die Schleife
    // prueft `soll_stoppen()` — aber nur **nach** `horcher.accept()`
    // zurueckkommt. Ohne neue Verbindung kommt dort nie etwas, also
    // laeuft der Faden ewig.
    //
    // DAS IST ABER GENAU DER BEFUND AUS V5 IN ANDERER FORM: eine
    // Schleife, die auf aeusseren Input wartet, muss den Warteweg
    // begrenzen, sonst ist sie nicht abbrechbar. Also **nonblocking**
    // mit einer kurzen Wartezeit:
    //
    // * 200 ms: gross genug, dass ein Localhost-Nutzer keine
    //   Verzoegerung merkt (es wird nur der `accept` geprueft, nicht
    //   gewartet), klein genug, dass `herunterfahren` in unter einer
    //   Viertelsekunde durchkommt.
    // * M8: die Wartezeit ist eine Konstante, nicht die Eingabe.
    // * `set_nonblocking(true)` wird **einmal** gesetzt und bleibt
    //   stehen. Die erste Fassung tat pro Runde `true` und dann
    //   `false` — damit war der naechste `accept` wieder blockierend
    //   und der ganze Weg umsonst.
    let _ = horcher.set_nonblocking(true);
    while max_verbindungen == 0 || gezaehlt < max_verbindungen {
        if server.soll_stoppen() {
            break;
        }
        let (mut stream, von) = match horcher.accept() {
            Ok(paar) => paar,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(ANNAHME_PRUEFUNG_MS));
                continue;
            }
            Err(e) => return Err(TlsFehler::Netz(format!("accept: {e}"))),
        };
        // Ab hier blockierend: `https_aufbauen` setzt Lese- und
        // Schreibfristen auf dem Sockel (siehe dort), also haengt der
        // Handshake nicht.
        let _ = stream.set_nonblocking(false);
        gezaehlt += 1;
        let peer = von.to_string();
        if let Err(e) = https_bediene_eine(server, &mut stream, angebot, &peer) {
            // EIN FEHLER BEENDET DIE VERBINDUNG, NICHT DEN SERVER. Der
            // naechste `accept` ist eine neue Verbindung; ein Handshake
            // Fehler (der Gast spricht kein TLS) darf den Server nicht
            // beenden. Der Fehler geht auf stderr, damit ein Skript ihn
            // sieht.
            eprintln!("HTTPS: Verbindung von {peer} beendet: {e}");
        }
    }
    Ok(())
}

/// Wie [`https_laufen_uf`], aber mit eigener Adressangabe.
///
/// added 01.10.2026, R3: `127.0.0.1:0` ist der Weg fuer Tests und
/// Skripte — der Port wird vom Betriebssystem vergeben und ist ueber
/// `local_addr()` abfragbar, ohne dass eine Zahl im Weg steht.
pub fn https_laufen(
    server: &crate::Server,
    adresse: impl ToSocketAddrs,
    angebot: &TlsAngebot,
    max_verbindungen: usize,
) -> Result<(), TlsFehler> {
    let horcher = TcpListener::bind(adresse).map_err(|e| TlsFehler::Netz(format!("bind: {e}")))?;
    https_laufen_uf(server, &horcher, angebot, max_verbindungen)
}

/// Hinweis fuer gRPC-Anfragen (HTTP/2 mit `application/grpc`).
///
/// gRPC braucht HTTP/2 (Multiplexing, HPACK, Flow-Control). `h2` wird
/// seit R3 in ALPN **angeboten**, aber der HTTP-Pfad spricht noch
/// HTTP/1.1 — wer `application/grpc` schickt, bekommt `501` mit diesem
/// Text, nicht ein halb gelesenes HTTP/2-Frame.
pub fn grpc_hinweis() -> &'static str {
    "gRPC braucht HTTP/2. ALPN `h2` wird angeboten, aber der HTTP-Pfad spricht noch HTTP/1.1. \
     Nutze REST + JSON (narsil_server::api) oder WebSocket (narsil_server::ws)."
}

/// Die Roadmap: was als Naechstes kommt, und warum.
///
/// added 01.10.2026, R3 aktualisiert. **Punkt 1 (TLS 1.3) und Punkt 2
/// (HTTPS darauf) sind erledigt**: `narsil_tls::server` steht (R1d, mit
/// `openssl s_client` als Zeuge) und ist hierueber verdrahtet
/// ([`https_laufen`]). Was bleibt:
///
/// 1. **HTTP/2 ueber `h2`.** `alpn` wird bereits ausgehandelt und gemeldet,
///    der HTTP-Pfad spricht aber noch HTTP/1.1. Deshalb steht in
///    `TlsAngebot::http1_dann_h2` `http/1.1` **zuerst**: ein Server, der
///    `h2` zuerst anbietet, wuerde einem HTTP/2-Client `h2` versprechen
///    und dann HTTP/1.1-Bytes schicken.
/// 2. **WSS.** WebSocket steht ueber `ws`, aber nur auf `ws://`. WSS ist
///    derselbe TLS-Sockel mit dem WebSocket-Pfad darüber.
/// 3. **HTTP/3/QUIC danach.** Braucht UDP + QUIC (RFC 9000/9001) + QPACK.
///    `narsil-tls` liefert dafuer schon die `quic_transport_parameters`
///    (R2, `team/BERICHT_TLS_R2.txt`).
pub fn roadmap_http3() -> &'static str {
    "HTTPS steht (narsil_tls::server + narsil_server::tls). Offen: HTTP/2 ueber h2, dann WSS, dann HTTP/3/QUIC. Siehe narsil_server::tls."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_wird_erkannt_nicht_geraten() {
        assert!(braucht_tls("http://x/").is_ok());
        assert!(braucht_tls("/pfad").is_ok());
        assert!(braucht_tls("ws://x/chat").is_ok());
        assert!(braucht_tls("https://x/").is_err());
        assert!(braucht_tls("wss://x/chat").is_err());
        assert!(braucht_tls("HTTPS://x/").is_err());
    }

    #[test]
    fn grpc_hinweis_nennt_alternative() {
        assert!(grpc_hinweis().contains("REST"));
    }

    #[test]
    fn die_meldung_verspricht_kein_ungebautes_https_mehr() {
        // added 01.10.2026, R3. DER TEST, und er ist eine Text-Zusage:
        // die Meldung von `TlsFehlt` hat "HTTPS ist noch nicht gebaut"
        // gesagt, und das war ab R3 **falsch**. Ein alter Text, der die
        // eigene Wirklichkeit leugnet, ist schlimmer als gar keiner.
        let meldung = TlsFehlt {
            adresse: "https://x/".to_string(),
        }
        .to_string();
        assert!(
            !meldung.contains("noch nicht gebaut"),
            "die Meldung behauptet immer noch, HTTPS sei nicht gebaut: {meldung}"
        );
        assert!(
            meldung.contains("https_laufen_uf"),
            "die Meldung nennt den Weg, der jetzt funktioniert: {meldung}"
        );
    }

    #[test]
    fn alpn_steht_auf_h2_zuerst_weil_der_server_h2_spricht() {
        // added 01.10.2026, SERVER V2. DIESER TEST IST UMGEDREHT, und
        // der Grund steht in `TlsAngebot::h2_dann_http1`: seit V2
        // spricht der Server `h2` wirklich (`crate::h2`, mit
        // `curl -k --http2` als Beweis). Die alte Fassung nagelte
        // `http/1.1` nach vorn fest, "solange kein h2 da ist" — das
        // "solange" ist jetzt vorbei, und ein Test, der eine
        // Zwischenstufe festhält, waere eine Luege.
        let angebot = TlsAngebot::h2_dann_http1();
        assert_eq!(angebot.alpn[0], b"h2".to_vec());
        assert_eq!(angebot.alpn[1], b"http/1.1".to_vec());
    }

    #[test]
    fn http1_dann_h2_bleibt_fuer_aufrufer_die_http1_wollen() {
        // added 01.10.2026, SERVER V2. DER GRUND FUER BEIDE Fabriken:
        // `http1_dann_h2` ist oeffentlich und wurde seit R3 benutzt.
        // Es zu entfernen waere ein API-Bruch; es nur noch anzubieten
        // und den Standard zu drehen, ist die ehrliche Form.
        let angebot = TlsAngebot::http1_dann_h2();
        assert_eq!(angebot.alpn[0], b"http/1.1".to_vec());
        assert_eq!(angebot.alpn[1], b"h2".to_vec());
    }

    #[test]
    fn ein_angebot_ohne_zertifikat_wird_zurueckgewiesen() {
        // added 01.10.2026, R3. DER FALL, und er ist der wahrscheinlichste
        // beim ersten Versuch: das Angebot wird gebaut, das Zertifikat
        // aber aus einem Pfad gelesen, der nicht existiert. Dann ist
        // `zertifikat` **leer** — und der Server muss das **vor** dem
        // ersten `accept` sagen, nicht danach.
        let angebot = TlsAngebot::http1_dann_h2();
        assert!(matches!(angebot.als_server(), Err(TlsFehler::Angebot(_))));
        // Und mit einem Zertifikat, aber ohne Schluessel, auch.
        let mut mit_zertifikat = angebot.clone();
        mit_zertifikat.zertifikat = vec![0x30, 0x00];
        assert!(matches!(
            mit_zertifikat.als_server(),
            Err(TlsFehler::Angebot(_))
        ));
    }
}
