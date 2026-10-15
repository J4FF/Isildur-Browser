//! T13 — `TlsStrom`: ein TLS-1.3-Client ueber `TcpStream`.
//!
//! DIESE DATEI ENTHALT KEINEN EIGENEN KRYTOGRAPHISCHEN CODE. Alles,
//! was rechnet, liegt in `sha2.rs`, `hkdf.rs`, `aes.rs`, `chacha.rs`,
//! `x25519.rs`, `record.rs`, `signatur.rs` und `ecdsa.rs`, und jedes
//! dieser Stuecke ist an offiziellen Vektoren geprueft — die
//! Schluesselberechnung und die 19 Stationen aus RFC 8448 Abschnitt 3.
//! Hier wird nur die REIHENFOLGE gebaut, und die ist genau der Teil,
//! den ein Vektor nicht abdeckt.
//!
//! DIE REIHENFOLGE, nach RFC 8446 Abschnitt 2. Mit Pfeil die Richtung:
//!
//! ```text
//!   ClientHello          -->  ServerHello
//!   (verschluesselt)     <--  EncryptedExtensions
//!                         <--  Certificate
//!                         <--  CertificateVerify
//!                         <--  Finished
//!   Finished             -->
//!   application_data     -->  application_data   <--
//! ```
//!
//! DREI EIGENTUMLICHKEITEN, die nicht aus der Norm folgen, sondern aus
//! der Tatsache, dass hier `std::net` benutzt wird:
//!
//!  1. **Es gibt keinen Weg an der Kettenpruefung vorbei.** Wird die
//!     Kette nicht bestaetigt, ist der Handshake vorbei. Ein `bool`,
//!     den man auf `false` setzen kann, gibt es nicht — auch nicht
//!     fuer Tests.
//!  2. **Jeder Lesevorgang hat eine Zeitgrenze.** `TcpStream` blockiert
//!     sonst fuer immer, und ein Browser, der auf einen schweigenden
//!     Server wartet, ist ein haengender Browser (M6).
//!  3. **Jeder Puffer wird vor der Allokation geprueft** (M4), und
//!     jede Schleife ueber eingababhaengige Daten hat eine benannte
//!     Grenze (M3).

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::aes;
use crate::chacha;
use crate::kette::{Vertrauensspeicher, kette_pruefen};
use crate::nachricht::{
    self, Certificate, CertificateVerify, ClientHello, EncryptedExtensions, Finished, ServerHello,
};
use crate::record::{Schluesselplan, Suite, baue_record, typen};
use crate::sha2::Sha256;
use crate::signatur::certificate_verify_inhalt;
use crate::x509::{SchluesselArt, zertifikat_lesen};
use crate::x25519;
use crate::zufall;

/// Zeitgrenze fuer das Verbinden.
///
/// M6: ohne diese Grenze wartet `connect` bei einer geschlossenen
/// Firewall so lange, wie das Betriebssystem es fuer richtig haelt.
pub const VERBINDUNGSZEIT: Duration = Duration::from_secs(20);

/// Zeitgrenze fuer einen Lesevorgang.
///
/// M6: ein Server, der den Handshake beginnt und dann schweigt, darf
/// den Client nicht festhalten.
pub const LESEZEIT: Duration = Duration::from_secs(20);

/// Obergrenze fuer die Zahl der Adressen, die zu einem Namen
/// aufgeloest werden.
///
/// M3/M4: `to_socket_addrs` liefert eine Liste, und `take` begrenzt
/// sie. Die Zahl ist eine benannte Konstante.
pub const MAX_ADRESSEN: usize = 8;

/// Obergrenze fuer die Groesse des eingehenden TCP-Puffers.
///
/// M4: 256 KiB reichen fuer einen HTTP-Get mit Antwort. Der Wert
/// begrenzt jede Allokation, die aus einer Laengenangabe folgt.
pub const MAX_ANTWORT: usize = 256 * 1024;

/// Obergrenze fuer die Zahl der Records in einem Handshake-Flug.
///
/// M3: der Server schickt `EncryptedExtensions`, `Certificate`,
/// `CertificateVerify` und `Finished` — vier. Zwei sind genug Reserve
/// fuer ein `CertificateRequest` und eine unbekannte, harmlose
/// Zwischennachricht. Die Zahl ist eine Konstante.
pub const MAX_FLUG_RECORDS: usize = 8;

/// Obergrenze fuer die Records, die als **Nachschrift** gelesen werden.
///
/// added 02.10.2026, R4k. M3: die Nachschrift ist etwas, das ein Server
/// schickt **wenn er will** (RFC 8446 4.6.1: „MAY"), und die Zahl der
/// Tickets je Sitzung ist nicht normiert.
///
/// **GEMESSEN** am 02.10.2026, 06:0x an drei echten Servern: 0
/// (`example.com` ohne Ticket), 2 (`de.wikipedia.org`) und 1
/// (`www.rust-lang.org`). Acht ist die Grenze, weil der Wert eine
/// **Messung mit Reserve** sein soll und nicht die Zahl, die man zuletzt
/// gesehen hat — mit 2 als Grenze wuerde ein Server, der drei schickt, am
/// Ende seines Angebots abgeschnitten, ohne dass jemand es merkt.
pub const MAX_NACHSCHRIFT_RECORDS: usize = 8;

/// Obergrenze fuer die Zahl der Bytes in einem Handshake-Flug.
pub const MAX_FLUG: usize = 256 * 1024;

/// Obergrenze fuer die Zahl der Zertifikate in der `Certificate`-Nachricht.
pub const MAX_ZERTIFIKATE_KETTE: usize = 8;

/// Warum der Handshake nicht durchging.
#[derive(Debug)]
pub enum StromFehler {
    /// Der Server liess sich nicht erreichen.
    KeineVerbindung(String),
    /// Der Server hat zu lange geschwiegen.
    Zeitueberschreitung,
    /// Die Verbindung wurde vom Server geschlossen.
    VerbindungZu,
    /// Auf dem TCP-Puffer war unlesbarer Inhalt.
    Netz(String),
    /// Die Nachrichtenstruktur war falsch.
    /// added 02.10.2026 03:5x UTC (Projektlabel 02.10.2026), Rolle TLS.
    /// added 02.10.2026 03:5x: Der Schluesselplan liess sich nicht rechnen
    /// (HKDF). added 02.10.2026 03:5x: die Form ist **eigene**, nicht
    /// `Record(...)`: ein Record-Fehler heisst „die Bytes liessen sich nicht
    /// lesen", ein Schluesselplan-Fehler heisst „die Rechnung ging nicht" —
    /// zwei Ursachen, zwei Namen. Vorher stand an dieser Stelle ein `expect`
    /// **in** `record.rs`, also ein Panic.
    Schluesselplan(crate::record::RecordFehler),
    Nachricht(nachricht::NachrichtFehler),
    /// Das PSK-Angebot liess sich nicht bauen (Binder oder Layout).
    ///
    /// added 01.10.2026, R4 Teil 2. EIGENE VARIANTE UND KEIN
    /// `Nachricht(String)`: `psk_anbieten` scheitert an
    /// `ticket::BinderFehler`, nicht an einer Nachricht, und ein Aufrufer,
    /// der den Fehler sieht, soll wissen, ob er den **Binder** oder die
    /// Nachricht reparieren muss. Das war in diesem Projekt dreimal
    /// durcheinander: eine Kette, die an drei Stellen lag, hatte an einer
    /// Stelle einen Binder-Fehler und an der anderen einen
    /// Nachrichten-Fehler, und beide sahen gleich aus.
    Binder(crate::ticket::BinderFehler),
    /// Der Record war falsch.
    Record(crate::record::RecordFehler),
    /// Die Kette hat nicht gestimmt.
    Kette(crate::kette::KetteFehler),
    /// Die Signatur hat nicht gestimmt.
    SignaturFalsch,
    /// Das `finished` des Servers war falsch.
    FinishedFalsch,
    /// Das Zufallsgeraet liess sich nicht lesen.
    Zufall,
    /// X25519.
    X25519,
    /// Die Verschluesselung schlug fehl.
    ///
    /// Die Cipher Suite steht MIT drin. Ohne sie ist die Meldung
    /// nutzlos: „AEAD" allein sagt nicht, ob der Schluessel falsch
    /// war, die Nonce, das AAD oder der Transkript.
    AEAD {
        suite: &'static str,
        richtung: &'static str,
    },
    /// Der Server nannte keine der beiden Cipher Suites.
    KeineSuite,
    /// Der Server schickte kein X25519-Angebot.
    KeinX25519,
    /// Der Server hat eine Protokollversion gewaehlt, die weder
    /// angeboten noch implementiert ist.
    ///
    /// added 30.09.2026. DER NAME sagt bewusst nicht "Downgrade": der
    /// Fall, dass ein Server auf TLS 1.2 zurueckfaellt, ist per
    /// RFC 8446 Abschnitt 4.1.3 erlaubt und wird an anderer Stelle
    /// behandelt. Hier steht ein Server, der etwas antwortet, was
    /// nicht in `supported_versions` stand — das ist keine
    /// Downgrade-Marke, sondern eine Nachricht, die es nicht gibt.
    VersionUnerwartet {
        ist: u16,
    },
    /// **Downgrade-Angriff** nach RFC 8446 Abschnitt 4.1.3.
    ///
    /// Wir haben TLS 1.3 angeboten, der Server hat TLS 1.2 geantwortet,
    /// und die letzten acht Byte seines ServerRandom sind
    /// `44 4f 57 4e 47 52 44 01` ("DOWNGRD" gefolgt von 0x01).
    ///
    /// Diese acht Byte kann ein MITM nicht erzeugen, ohne den Handshake
    /// zu besitzen: es ist die ServerHello-Nachricht, die in der
    /// `finished`-Pruefdaten des Servers eingeht. Ein Mittelaenger,
    /// der die Version zuruecksetzt, kann sie also nicht passend
    /// ausfuellen, und genau darum sagt RFC 8446, dass ein Client
    /// TLS 1.2 nur akzeptiert, wenn die Marke fehlt.
    ///
    /// DER NAME ist nicht "VeralteterServer": ein Server, der
    /// absichtlich DOWNGRD setzt, macht genau das hier.
    Downgrade,
    /// Der Server schickt einen HelloRetryRequest ohne Cookie.
    ///
    /// added 30.09.2026. RFC 8446 Abschnitt 4.1.4: "If the server
    /// selects a group for which the client did not send a key share
    /// ... the server MUST send a cookie". Ohne Cookie kann der
    /// zweite ClientHello nicht gebaut werden, und ein leeres Cookie
    /// waere nach RFC 8446 Abschnitt 4.2.2 ohnehin unzulaessig
    /// (`cookie<1..2^16-1>`).
    HrrOhneCookie,
    /// Der Server verlangt eine Gruppe, die diese Crate nicht rechnen
    /// kann. Im HelloRetryRequest nennt er sie im `key_share`.
    ///
    /// added 30.09.2026. Ohne diese Meldung endete der Fall mit
    /// `KeinX25519`, und das ist die falsche Antwort: x25519 war hier
    /// gar nicht die Frage, sondern secp521r1 oder x448.
    GruppeGewuenscht {
        code: u16,
    },
    /// Der oeffentliche Schluessel des Servers hat nicht die Form der
    /// gewaehlten Gruppe.
    ///
    /// added 30.09.2026. Fuer P-256 sind das 65 Byte mit dem Praefix
    /// 0x04 (RFC 5480 Abschnitt 2.1), fuer x25519 genau 32 (RFC 8446
    /// Abschnitt 4.2.11.2). `war` nennt die Laenge, die kam.
    PunktFalsch {
        war: usize,
    },
    /// Der TLS-1.2-Weg ist gescheitert. Die Ursache steht **unveraendert**
    /// drin (`strom12::Strom12Fehler`), damit die Meldung nicht erst
    /// durch eine Kette von `format!` ihre Information verliert.
    ///
    /// added 30.09.2026 mit der gemeinsamen Verbindungsfunktion. Ohne
    /// diese Variante gaebe es zwei Wege, aus einer Verbindung einen
    /// Fehler zu machen, und der Aufrufer muesste raten, welcher
    /// gemeint ist.
    Tls12(Box<crate::strom12::Strom12Fehler>),
    /// Der Server schickte mehr, als er darf.
    ZuViel(&'static str),
    /// Es kam etwas, das nicht erwartet wurde.
    Unerwartet(&'static str),
    /// Der Record hatte einen Typ, der hier nicht hingehört.
    ///
    /// Der Typ steht MIT in der Meldung. Vorher stand hier nur
    /// "verschluesselter Record erwartet", und damit musste man
    /// raten, welcher Typ es war — bei einem Server im Netz genau die
    /// Auskunft, die man zum Finden des Fehlers braucht.
    FalscherRecord {
        erwartet: &'static str,
        bekommen: u8,
    },
}

impl std::fmt::Display for StromFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StromFehler::KeineVerbindung(w) => write!(f, "keine Verbindung: {w}"),
            StromFehler::Zeitueberschreitung => write!(f, "Zeitueberschreitung"),
            StromFehler::VerbindungZu => write!(f, "der Server hat die Verbindung geschlossen"),
            StromFehler::Netz(w) => write!(f, "Netz: {w}"),
            StromFehler::Schluesselplan(e) => {
                write!(f, "der Schluesselplan liess sich nicht rechnen: {e}")
            }
            StromFehler::Nachricht(e) => write!(f, "Nachricht: {e}"),
            StromFehler::Binder(e) => write!(f, "PSK-Angebot: {e}"),
            StromFehler::Record(e) => write!(f, "Record: {e}"),
            StromFehler::Kette(e) => write!(f, "Kette: {e}"),
            StromFehler::SignaturFalsch => write!(f, "die Signatur des Servers gilt nicht"),
            StromFehler::FinishedFalsch => write!(f, "das finished des Servers stimmt nicht"),
            StromFehler::Zufall => write!(f, "das Zufallsgeraet liess sich nicht lesen"),
            StromFehler::X25519 => write!(f, "X25519"),
            StromFehler::AEAD { suite, richtung } => {
                write!(f, "Verschluesselung ({suite}, {richtung})")
            }
            StromFehler::KeineSuite => write!(f, "keine der angebotenen Cipher Suites"),
            StromFehler::KeinX25519 => write!(f, "kein X25519-Schluessel"),
            StromFehler::VersionUnerwartet { ist } => {
                write!(
                    f,
                    "der Server waehlte Version 0x{ist:04x}, angeboten waren 0x0303 und 0x0304"
                )
            }
            StromFehler::Tls12(e) => write!(f, "TLS 1.2: {e}"),
            StromFehler::HrrOhneCookie => write!(
                f,
                "der Server schickt einen HelloRetryRequest ohne Cookie (RFC 8446 Abschnitt 4.1.4)"
            ),
            StromFehler::GruppeGewuenscht { code } => write!(
                f,
                "der Server verlangt die Gruppe {code}, die diese Crate nicht rechnen kann"
            ),
            StromFehler::PunktFalsch { war } => write!(
                f,
                "der oeffentliche Schluessel des Servers hat nicht die Form der Gruppe ({war} Byte)"
            ),
            StromFehler::Downgrade => write!(
                f,
                "Downgrade-Angriff (RFC 8446 Abschnitt 4.1.3): der Server antwortete mit TLS 1.2 und setzte die DOWNGRD-Marke"
            ),
            StromFehler::ZuViel(w) => write!(f, "zu viel vom Server: {w}"),
            StromFehler::Unerwartet(w) => write!(f, "unerwartet: {w}"),
            StromFehler::FalscherRecord { erwartet, bekommen } => write!(
                f,
                "{erwartet} erwartet, aber ein Record vom Typ {bekommen} (0x{bekommen:02x})"
            ),
        }
    }
}

impl From<nachricht::NachrichtFehler> for StromFehler {
    fn from(e: nachricht::NachrichtFehler) -> StromFehler {
        StromFehler::Nachricht(e)
    }
}
impl From<crate::record::RecordFehler> for StromFehler {
    fn from(e: crate::record::RecordFehler) -> StromFehler {
        StromFehler::Record(e)
    }
}
impl From<crate::kette::KetteFehler> for StromFehler {
    fn from(e: crate::kette::KetteFehler) -> StromFehler {
        StromFehler::Kette(e)
    }
}

/// Die XOR-Nonce, nach RFC 8446 Abschnitt 5.3.
///
/// `nonce = iv XOR (sequence number, linksbuendig auf 12 Byte)`.
fn nonce(iv: &[u8; 12], seq: u64) -> [u8; 12] {
    let mut n = *iv;
    let s = seq.to_be_bytes();
    for i in 0..8 {
        n[4 + i] ^= s[i];
    }
    n
}

/// Der TLS-1.3-Strom.
///
/// `Read` und `Write` arbeiten in der Anwendungsphase, also nach dem
/// Handshake. Davor sind beide gesperrt.
pub struct TlsStrom {
    /// added 02.10.2026 23:jx UTC (Projektlabel 02.10.2026), S4. Die stapelte
    /// OCSP-Antwort aus der `Certificate`-Nachricht, **unveraendert**.
    ///
    /// WARUM SIE HIER LIEGT: ausgewertet wird sie nach der Kettenpruefung, denn
    /// da sind erst das Blatt und sein Aussteller gelesen. Ohne die kann man
    /// weder die CertID vergleichen noch die Signatur der CA nachrechnen.
    ///
    /// M4: die Laenge ist durch `nachricht::ocsp_aus_zertifikatsblock`
    /// gegen den Block der Nachricht geprueft, und der wiederum durch
    /// `MAX_NACHRICHT`.
    ocsp_nutzlast: Option<Vec<u8>>,
    /// added 02.10.2026 23:jx, S4. Die Stufe und ihr Grund, siehe
    /// `ocsp_stufe` und `TlsStrom::ocsp_urteil`.
    ocsp_stufe: &'static str,
    ocsp_grund: &'static str,

    pub(crate) socket: TcpStream,
    suite: Suite,
    // --- Schreibeseite
    s_schluessel: [u8; 32],
    s_iv: [u8; 12],
    s_seq: u64,
    // --- Leseseite
    e_schluessel: [u8; 32],
    e_iv: [u8; 12],
    e_seq: u64,
    /// Klartext aus gelesenen Records, aus dem `read` bedient.
    klattr: Vec<u8>,
    /// Puffer fuer das Lesen eines Recordkopfs und -inhalts.
    roh: Vec<u8>,
    /// `close_notify` schon gesendet.
    gemeldet: bool,
    /// Der geprueffte Hostname.
    pub host: String,
    /// Das Serverzertifikat, fuer Anwendungen, die es zeigen wollen.
    pub blatt_der: Vec<u8>,
    /// added 03.10.2026 16:0x, R60. **DIE GANZE KETTE ALS DER**, Blatt zuerst —
    /// und das ist die **Vertragsflaeche** fuer die CRL-Beschaffung in
    /// `narsil-net`.
    ///
    /// added 03.10.2026 16:0x: **WARUM DIE KETTE UND NICHT NUR DAS BLATT.** Fuer
    /// die CRL-Pruefung braucht man **zwei** Zertifikate: das Blatt (dessen
    /// Seriennummer in der CRL steht) und den **Aussteller** (dessen Schluessel
    /// die CRL-Signatur prueft). `blatt_der` allein reicht also nicht, und
    /// `kette_pruefen` liefert die Kette bereits — sie wurde nur **verworfen**
    /// (`let _kette = kette_pruefen(...)`, R60-Messung).
    ///
    /// added 03.10.2026 16:0x: **UND DER WEG IST DER, DEN `narsil-net` SONST
    /// GEHEN WUERDE.** `crl::beurteile` nimmt `&Zertifikat`, nicht DER. Also
    /// koennte ich `Vec<Zertifikat>` durchreichen — aber `Zertifikat` enthaelt
    /// geparste Felder und ist ein **grosses** Objekt, das der Aufrufer nicht
    /// braucht. DER ist die **unveraenderte** Form: `narsil-net` liest sie mit dem
    /// **oeffentlichen** `x509::zertifikat_lesen` und hat damit **dieselbe**
    /// Sicht auf die Kette wie der Handshake. Eine zweite Lesart gibt es nicht.
    ///
    /// added 03.10.2026 16:1x: **UND DIE KETTE IST AUCH FUER DEN ANGREIFER
    /// NICHT GEHEIM** — sie steht im Klartext im `Certificate`-Block, den der
    /// Server ohnehin schickt. Wir geben nichts weiter, was der Aufrufer nicht
    /// schon mit einem `MitM` laesst.
    pub kette_der: Vec<Vec<u8>>,
    /// Die vom Server gewaehlte ALPN-Kennung, oder `None`, wenn der
    /// Server keine geschickt hat (dann gilt HTTP/1.1, RFC 7301 3.2).
    ///
    /// added 30.09.2026, Runde 2, TLS R1 (S7). Das Feld wird aus den
    /// `EncryptedExtensions` gefuellt; siehe dort die Begruendung.
    pub alpn: Option<Vec<u8>>,
    /// added 04.10.2026, Rolle TLS. **WO DIE POST-HANDSHAKE-TICKETS HIN
    /// GEHEN, wenn sie in der Anwendungsphase ankommen.**
    ///
    /// **WARUM EIN FELD AM STROM UND NICHT EIN PARAMETER AN `read`:** `Read`
    /// hat eine feste Signatur (`read(&mut self, buf)`), und `narsil-net`
    /// benutzt genau die (`Leitung: Read + Write`). **Ein zweiter
    /// Lese-Eingang** — `lies_mit_tickets(&mut self, buf, tickets)` —
    /// **wuerde** funktionieren, **wuerde aber** neben `read` stehen, und
    /// **wer `read` benutzt, haette die Tickets nicht**. GEMESSEN ist genau das
    /// der heutige Zustand: `read` verwirft die Nachschrift
    /// (`typen::HANDSHAKE => continue`), also hat **kein** Seitenabruf ein
    /// Ticket.
    ///
    /// **`None` heisst „es gibt keinen Speicher“**, und dann bleibt das alte
    /// Verwerfen. Das ist **kein** Fehler: ein Test, der nur HTTP macht, soll
    /// keinen Speicher aufbauen muessen.
    ///
    /// **`Arc<Mutex<…>>` UND NICHT `Rc<RefCell<…>>`:** `TlsStrom` muss
    /// **`Send`** bleiben — `narsil-net` legt Verbindungen in einen
    /// `static POOL: OnceLock<Mutex<Pool>>`, und die Kompilierung dieses Pools
    /// ist der Beweis. **`Rc` ist nicht `Send`.**
    nachschrift_speicher: Option<std::sync::Arc<std::sync::Mutex<crate::ticket::Tickets>>>,
    /// added 04.10.2026: **ZAEHLER der in der Anwendungsphase ausgewerteten
    /// Tickets.** Er wird in [`nachschrift_auswerten`](Self::nachschrift_auswerten)
    /// **um `abgelegt` erhoeht** — und **nicht** beim Handshake, denn dort gibt es
    /// keine Post-Handshake-Nachricht.
    nachschrift_ausgewertet: usize,
    /// added 04.10.2026, Rolle TLS. **DIE ZEITBASIS DER TICKET-ABLEGUNG**, in
    /// Sekunden seit der Unix-Epoche.
    ///
    /// **WARUM SIE AM STROM STEHT:** `new_session_ticket` braucht eine Zeit, um
    /// `ticket_lifetime` zu pruefen (RFC 8446 4.6.1: ein Ticket ist nach seiner
    /// Lebensdauer **verfallen**). `hole_nachschrift` bekommt sie vom Aufrufer —
    /// **`read` nicht**, und `read` ist der Weg, der ohne Warten arbeiten soll.
    /// **Ohne dieses Feld** muesste `read` die Systemuhr lesen, und dann waere
    /// die Ablage von einem **anderen** `jetzt` abhaengig als beim Handshake —
    /// **zwei Zeitbasen fuer dieselbe Sache**.
    ///
    /// **UND ES IST DIE ZEIT DES HANDSHAKES, nicht „jetzt“:** der Aufrufer hat
    /// sie beim Bau uebergeben (`ueber_sockel(…, jetzt)`).
    nachschrift_zeit: u64,
    /// Der `resumption_master_secret` dieser Sitzung.
    ///
    /// added 01.10.2026 12:5x, R4z2 Teil C. **WARUM DAS FELD NEBEN
    /// `alpn` noetig ist:** RFC 8446 4.6.1 leitet die Wiederaufnahme-PSK
    /// aus diesem Geheimnis und dem `ticket_nonce` ab — sie steht in
    /// **keinem** Ticket. Das Geheimnis entsteht im Schluesselplan, und
    /// der Schluesselplan ist bisher **lokal** im Handshake. Ohne dieses
    /// Feld kann der Client eine `NewSessionTicket` nicht auswerten: er
    /// haette das Ticket, aber nicht das, woraus sich die PSK rechnet.
    ///
    /// GEMESSEN, warum das kein Detail ist: vorher stand die PSK als
    /// Zufallswert **im Ticket** (`psk_aus_ticket`, `PSK_VERSATZ` = 8).
    /// Diese Runde hat dasselbe Ticket bei `openssl` und bei uns
    /// verglichen — zwei verschiedene PSKs (`PROBLEME.txt` Nr. 10), und
    /// deshalb kein `Reused`.
    ///
    /// `None` bei TLS 1.2: dort gibt es keine Wiederaufnahme ueber
    /// `pre_shared_key` (das ist eine TLS-1.3-Erweiterung), also auch
    /// kein Geheimnis, das man brauchte.
    pub(crate) resumption_master_secret: Option<[u8; 32]>,
    /// `selected_identity` aus dem ServerHello, oder `None`.
    ///
    /// added 02.10.2026, R4d Stufe 2. DER GRUND FUER DAS FELD AUF DEM
    /// STROM, und es ist nicht Bequemlichkeit: **ohne** dieses Feld kann
    /// ein Client nicht wissen, ob der Server sein Angebot angenommen hat.
    /// RFC 8446 4.2.11.3 sagt beides: schickt der Server keine
    /// `pre_shared_key`-Erweiterung, hat er abgelehnt — und ein Client, der
    /// es ignoriert, rechnet mit einem PSK weiter, den der Server nie
    /// benutzt hat. Der erste Record schlaegt dann fehl, und die Meldung
    /// sieht nach Verschluesselung aus.
    ///
    /// DER WEG ZUM WERT: aus dem **ServerHello**, also im Klartext. Es
    /// steht ausdruecklich **nicht** in den `EncryptedExtensions`
    /// (RFC 8446 4.2.11.3), und ein Dekodierer, der es dort suchte,
    /// wuerde es nie finden.
    pub selected_identity: Option<u16>,
    /// Das PSK-Angebot dieses Handshakes, oder `None`.
    ///
    /// added 02.10.2026, R4d Stufe 2. **PRIVAT**, und der Grund ist die
    /// Eigentuemerfrage: das Angebot gehoert zu **diesen** Handshake.
    /// Als Parameter an `handshake_ab_serverhello` gereicht wuerde es
    /// drei Aufrufstellen aendern (die dritte liegt im TLS-1.2-Zweig und
    /// braucht es nie) — als Feld ist es dort, wo es hingehört, und der
    /// Plan kann es ohne jeden weiteren Parameter lesen.
    ///
    /// DIE LEBENSDAUER, und sie ist kurz: es stirbt mit dem Handshake. Nach
    /// `finished` wird es nicht mehr gebraucht und nicht mehr
    /// angetastet; es steht hier nur, damit die Rechnung **während** des
    /// Handshakes darauf zugreifen kann.
    psk_angebot: Option<crate::ticket::PskAngebot>,
    /// Die Sequenzzahl der **Anwendungs**-Phase beim Lesen.
    ///
    /// added 02.10.2026, R4d Stufe 2. DER GRUND FUER EIN EIGENES FELD
    /// und nicht die Fortsetzung der Handshake-Sequenzzahl: RFC 8446 5.5
    /// zaehlt je Phase neu (`sequence_number = 0` bei jedem Wechsel der
    /// Schreibrichtung oder der Schluesselphase). Ein Fortsetzen waere an
    /// der einen Stelle richtig und an der anderen ein AEschluessel mit
    /// falscher Nonce.
    ap_lese_seq: u64,
}

impl TlsStrom {
    /// Wie steht der stapelte Widerrufsstatus? `(Stufe, Grund)`.
    ///
    /// added 02.10.2026 23:jx UTC (Projektlabel 02.10.2026), S4. Siehe
    /// `strom12::Tls12Strom::ocsp_urteil` — dieselben drei Stufen und
    /// dieselben Bedeutungen. In TLS 1.3 ist der stapelte Status der Normalfall
    /// (GEMESSEN 23:fx: `xkcd.com`, `www.netbsd.org` liefern in 1.3 eine
    /// Antwort), deshalb steht die Abfrage hier nicht hinter einem „nur 1.2".
    pub fn ocsp_urteil(&self) -> (&'static str, &'static str) {
        (self.ocsp_stufe, self.ocsp_grund)
    }

    /// Verbindet, macht den Handshake und liefert einen fertigen Strom.
    ///
    /// `jetzt` sind Sekunden seit 1970 und werden uebergeben, damit
    /// ein Test ein Datum setzen kann. `jetzt` wandert an die
    /// Kettenpruefung — die Gueltigkeit wird also an genau einer
    /// Stelle entschieden, hier.
    pub fn verbinde(
        host: &str,
        port: u16,
        alpn: &[&[u8]],
        speicher: &Vertrauensspeicher,
        jetzt: i64,
    ) -> Result<TlsStrom, StromFehler> {
        let mut strom = Self::verbinde_ohne_handshake(host, port)?;
        strom.handshake(host, alpn, speicher, jetzt, None)?;
        Ok(strom)
    }

    /// Nimmt einen BEREITS VERBUNDENEN Sockel und macht nur den
    /// Handshake.
    ///
    /// added 30.09.2026 fuer `narsil-net`. WARUM: der Aufrufer hat dort
    /// eine eigene Adresssuche (IPv4 und IPv6 nacheinander, jede mit
    /// eigenem Zeitlimit) und einen Sockel mit gesetzten Zeitgrenzen.
    /// Baute `TlsStrom` den Sockel selbst, waere die Adresssuche
    /// zweimal gelaufen — und `narsil-net` haette an zwei Stellen eine
    /// DNS-Aufloesung, die auseinanderlaufen kann.
    ///
    /// Deshalb: der Sockel kommt von aussen, `TlsStrom` setzt nur die
    /// Zeitgrenzen, die er selbst braucht, und haelt sie NICHT kleiner
    /// als die des Aufrufers. Die Zeitgrenzen des Aufrufers sind die
    /// weiterhin fuehrenden; ein `set_*_timeout` verkleinert sie nur,
    /// wenn der Wert kleiner ist.
    pub fn ueber_sockel(
        socket: std::net::TcpStream,
        host: &str,
        alpn: &[&[u8]],
        speicher: &Vertrauensspeicher,
        jetzt: i64,
    ) -> Result<TlsStrom, StromFehler> {
        // M6: beide Richtungen bekommen eine Zeitgrenze. Der Aufrufer
        // hat sie vielleicht schon gesetzt; wir setzen sie neu, aber
        // nur, wenn er keine gesetzt hat — sonst wuerde eine kurze
        // Frist des Aufrufers still auf unsere 20 s waechsen.
        if socket.read_timeout().ok().flatten().is_none() {
            socket
                .set_read_timeout(Some(LESEZEIT))
                .map_err(|e| StromFehler::Netz(e.to_string()))?;
        }
        if socket.write_timeout().ok().flatten().is_none() {
            socket
                .set_write_timeout(Some(LESEZEIT))
                .map_err(|e| StromFehler::Netz(e.to_string()))?;
        }
        let mut strom = Self::aus_sockel(socket, host);
        strom.handshake(host, alpn, speicher, jetzt, None)?;
        Ok(strom)
    }

    /// Wie `ueber_sockel`, **mit** einem PSK-Angebot.
    ///
    /// added 01.10.2026, R4 Teil 2. DIE ZWEI BAUFORMEN UND IHR GRUND:
    /// `ueber_sockel` gibt es schon und wird von `narsil-net` und von rund
    /// einem Dutzend Tests benutzt. Ein zusaetzlicher Parameter dort
    /// heisse, jede dieser Stellen umzuschreiben — fuer einen Aufruf, den
    /// nur die Wiederaufnahme braucht. Deshalb der Weg ueber
    /// `mit_psk_gesetzt`:
    ///
    /// ```text
    /// // vollstaendiger Handshake (unveraendert):
    /// TlsStrom::ueber_sockel(s, host, alpn, speicher, jetzt)
    ///
    /// // mit Wiederaufnahme:
    /// TlsStrom::ueber_sockel_mit_psk(s, host, alpn, speicher, jetzt, &angebot)
    /// ```
    ///
    /// DER AUFBAU DES ANGEBOTS ist Sache des Aufrufers:
    /// `Tickets::angebot(host, jetzt)` liefert `Option<PskAngebot>` — also
    /// auch `None`, wenn es kein Ticket gibt, und dann ist es derselbe
    /// Weg wie ohne Aufruf.
    pub fn ueber_sockel_mit_psk(
        socket: std::net::TcpStream,
        host: &str,
        alpn: &[&[u8]],
        speicher: &Vertrauensspeicher,
        jetzt: i64,
        angebot: Option<&crate::ticket::PskAngebot>,
    ) -> Result<TlsStrom, StromFehler> {
        if socket.read_timeout().ok().flatten().is_none() {
            socket
                .set_read_timeout(Some(LESEZEIT))
                .map_err(|e| StromFehler::Netz(e.to_string()))?;
        }
        if socket.write_timeout().ok().flatten().is_none() {
            socket
                .set_write_timeout(Some(LESEZEIT))
                .map_err(|e| StromFehler::Netz(e.to_string()))?;
        }
        let mut strom = Self::aus_sockel(socket, host);
        strom.handshake(host, alpn, speicher, jetzt, angebot)?;
        Ok(strom)
    }

    /// Baut den Strom aus einem Sockel, ohne Handshake.
    pub fn aus_sockel(socket: std::net::TcpStream, host: &str) -> TlsStrom {
        TlsStrom {
            ocsp_nutzlast: None,
            ocsp_stufe: crate::strom12::ocsp_stufe::KEINE,
            ocsp_grund: "",
            socket,
            suite: Suite::Aes128GcmSha256,
            s_schluessel: [0u8; 32],
            s_iv: [0u8; 12],
            s_seq: 0,
            e_schluessel: [0u8; 32],
            e_iv: [0u8; 12],
            e_seq: 0,
            klattr: Vec::new(),
            roh: Vec::new(),
            gemeldet: false,
            host: host.to_string(),
            blatt_der: Vec::new(),
            kette_der: Vec::new(),
            alpn: None,
            nachschrift_speicher: None,
            nachschrift_ausgewertet: 0,
            nachschrift_zeit: 0,
            // added 01.10.2026 12:5x, R4z2 Teil C: vor dem Handshake
            // gibt es kein Geheimnis. Es wird in `hallo_tauschen`
            // gesetzt, sobald der Schluesselplan den `master_secret`
            // kennt.
            resumption_master_secret: None,
            selected_identity: None,
            psk_angebot: None,
            ap_lese_seq: 0,
        }
    }

    /// Nur die TCP-Verbindung, ohne TLS.
    ///
    /// Getrennt vom Handshake, damit ein Test pruefen kann, ob der
    /// Fehler im Netz oder in der Kryptographie liegt.
    pub fn verbinde_ohne_handshake(host: &str, port: u16) -> Result<TlsStrom, StromFehler> {
        // added 04.10.2026, Rolle TLS. **DNS UND TCP ALS ZWEI SCHRITTE**, weil
        // die Frage getrennt beantwortet werden muss: „ist 1 s DNS?“ laesst
        // sich nur beantworten, wenn man **beide** getrennt misst.
        //
        // **GEMESSEN (Release, Mittel aus 3 Laeufen, 04.10.2026):** zusammen
        // **36 ms** Wandzeit bei `www.rust-lang.org` — **nicht** die 1048 ms,
        // die dieselbe Messung am 04.10. frueh am Tag im Debug-Build unter
        // Last zeigte. **Die 1 s waren die Warteschlange vor der CPU, nicht die
        // Adressauflösung.**
        //
        // **UND WAS HIER NOCH FEHLT, benannt statt behauptet:** `to_socket_addrs`
        // fragt **A und AAAA nacheinander** (GEMESSEN an der Reihenfolge der
        // `use`-Anweisung: es ist ein einziger Aufruf, und die Aufloesung liegt
        // in `getaddrinfo`). Ein **toter IPv6-Pfad** kostet also **einen
        // ganzen Timeout** — das ist der **Happy-Eyeballs**-Fehler, den die
        // Leitung vermutet. **GEMESSEN ist er hier noch nicht**, und ich
        // behaupte ihn nicht.
        let adresse = crate::schritt!("NETZ: DNS (to_socket_addrs)", {
            (host, port)
                .to_socket_addrs()
                .map_err(|e| StromFehler::KeineVerbindung(e.to_string()))?
                .take(MAX_ADRESSEN)
                .next()
                .ok_or_else(|| StromFehler::KeineVerbindung("keine Adresse".to_string()))?
        });
        let socket = crate::schritt!("NETZ: TCP verbinden", {
            TcpStream::connect_timeout(&adresse, VERBINDUNGSZEIT)
                .map_err(|e| StromFehler::KeineVerbindung(e.to_string()))?
        });
        // M6: beide Richtungen bekommen eine Zeitgrenze.
        socket
            .set_read_timeout(Some(LESEZEIT))
            .map_err(|e| StromFehler::Netz(e.to_string()))?;
        socket
            .set_write_timeout(Some(LESEZEIT))
            .map_err(|e| StromFehler::Netz(e.to_string()))?;
        Ok(Self::aus_sockel(socket, host))
    }

    /// Der Handshake.
    fn handshake(
        &mut self,
        host: &str,
        alpn: &[&[u8]],
        speicher: &Vertrauensspeicher,
        jetzt: i64,
        angebot: Option<&crate::ticket::PskAngebot>,
    ) -> Result<(), StromFehler> {
        // --- 1) Der eigene Schluessel -----------------------------------
        let mut geheim = [0u8; 32];
        geheim.copy_from_slice(&zufall::bytes(32).map_err(|_| StromFehler::Zufall)?);
        let eigener_public = x25519::oeffentlich(&geheim);
        let mut random = [0u8; 32];
        random.copy_from_slice(&zufall::bytes(32).map_err(|_| StromFehler::Zufall)?);

        // --- 2) ClientHello ---------------------------------------------
        let mut hello = ClientHello::baue(
            random,
            Some(host.as_bytes()),
            alpn,
            &geheim,
            &eigener_public,
        )?;
        //
        // added 01.10.2026, R4 Teil 2: DAS ANGEBOT. Die Kette
        // (Block bauen -> kodieren -> Transkript -> Binder -> eintragen)
        // steht **vollstaendig** in `ticket::psk_anbieten`; hier wird sie
        // nur aufgerufen. DER GRUND FUER DEN EINZIGEN AUFruf: diese Kette
        // stand in diesem Projekt schon dreimal falsch an drei Stellen
        // (2 Byte zu langer Schnitt, fehlende zweite Expansionsstufe,
        // leerer statt gehashter Kontext). Eine Kette an einer Stelle.
        //
        // UND DIE BYTES SIND DIE WICHTIGE AUSGABE: `ch` ist, was **wirklich
        // gesendet** wurde, und genau das kommt in den Transkript
        // (RFC 8446 4.4.1). Ein zweites `hello.kodiere()` im Aufrufer
        // waere eine andere Nachricht, sobald `kodiere` sich aendert.
        let ch = match angebot {
            None => hello.kodiere(),
            Some(a) => crate::ticket::psk_anbieten(&mut hello, &a.vorschlaege, &a.psk)
                .map_err(StromFehler::Binder)?,
        };
        //
        // added 02.10.2026, R4k. DAS ANGEBOT AUCH **MERKEN**, und das war
        // ein Fehler: `handshake` baute die Angebot-Bytes, speicherte das
        // Angebot aber **nicht**. `handshake_ab_serverhello` braucht es fuer
        // den Schluesselplan (Zeile 916) und meldete danach
        // „der Server hat eine PSK-Identitaet gewaehlt, aber wir haben
        // keine angeboten (illegal_parameter)" — obwohl **wir** es gerade
        // angeboten hatten, im selben Aufruf.
        //
        // WARUM ES SO LANGE UNENTDECKT BLEIB: der gruene Test
        // `psk_rundlauf.rs` benutzt `TlsVerbindung::ueber_sockel_mit_psk`
        // — **eine andere Typ** mit demselben Methodennamen, deren Pfad das
        // Angebot richtig merkt. Zwei Typen, ein Name, ein gruener Test auf
        // dem einen und ein kaputter auf dem anderen: der Name hat den
        // anderen verdeckt.
        self.psk_angebot = angebot.cloned();
        self.sende_klartext(typen::HANDSHAKE, &ch)?;

        // Der Transkript laeuft in einem eigenen `Sha256`, weil er an
        // ZWEI Stellen gebraucht wird: einmal bis einschliesslich
        // `Certificate` (fuer die CertificateVerify-Signatur) und
        // einmal bis einschliesslich `CertificateVerify` (fuer das
        // `finished` beider Seiten). `Sha256` ist `Clone` und
        // `fertig` nimmt nichts weg, also wird genau an der richtigen
        // Stelle eine Zwischenkopie gemacht.
        //
        // DER PLAN (`Schluesselplan`) rechnet den Transkript intern
        // mit und kann ihn nur am Ende liefern. Ihn fuer beide
        // Zeitpunkte zu benutzen hiesse, ihn zweimal neu aufzubauen
        // — und das ist die Stelle, an der ein Fehler am billigsten
        // zu machen ist.
        // Der Transkript entsteht in `handshake_ab_serverhello`, dort
        // aus den gesendeten und den gelesenen Bytes.

        // --- 3) ServerHello ----------------------------------------------
        let sh_nachricht = self.lese_hello()?;
        let sh = ServerHello::dekodiere(&sh_nachricht)?;
        self.pskserverhello_pruefen(&sh)?;
        // added 02.10.2026, R4k. DIE ANTWORT DES SERVERS **MERKEN**, und
        // das war ein Fehler: `handshake` hat `selected_identity` gelesen und
        // dann **nicht** gespeichert. Ein `TlsStrom`, der mit Angebot
        // handshake, meldete danach `None` — auch wenn der Server das Angebot
        // **angenommen** hatte (GEMESSEN: `angenommen = true` im
        // Serverprotokoll, `None` beim Client).
        //
        // WARUM ES UNENTDECKT BLEIB: `tests/psk_rundlauf.rs` behauptet
        // `selected_identity == Some(0)` und benutzt dabei
        // `TlsVerbindung::ueber_sockel_mit_psk` — **eine andere Typ** mit
        // derselben Methode. Drei Pfade (`handshake`,
        // `hallo_tauschen_mit_psk`, der von `TlsVerbindung`) und **eine**
        // Behauptung in einem davon. Ohne den Rundlauf ueber die ganze Kette
        // — Handshake, Nachschrift, Angebot, zweiter Handshake — gibt es
        // keinen Test, der diesen Pfad sieht.
        self.selected_identity = sh.selected_identity;
        self.handshake_ab_serverhello(
            host,
            speicher,
            jetzt,
            &Hallo {
                ch,
                sh_nachricht,
                sh,
                geheim,
                hrr: None,
            },
        )
    }

    /// Schickt EIN ClientHello, das TLS 1.3 und TLS 1.2 anbietet, und
    /// liest den ServerHello.
    ///
    /// added 30.09.2026, der Kern der gemeinsamen Verbindungsfunktion.
    /// DER NAMEN ist zweiteilig: getauscht wird nichts, gesendet wird
    /// ein ClientHello und gelesen ein ServerHello. "Hallo" ist die
    /// Form, die TLS 1.3 in der Norm uebrig gelassen hat.
    ///
    /// Was hier zurueckkommt:
    /// * `ch` — die **gesendeten** ClientHello-Bytes, mit denen der
    ///   Transkript rechnet.
    /// * `sh_nachricht` — die gelesenen ServerHello-Bytes.
    /// * `sh` — der dekodierte ServerHello, mit `version`.
    /// * `geheim` — der x25519-Privatwert. Er wird im TLS-1.2-Pfad
    ///   gebraucht, und er ist derselbe, mit dem der `key_share`
    ///   gebaut wurde: es MUSS derselbe sein, sonst stimmt das
    ///   Premaster auf keiner Seite.
    ///
    /// Wie `hallo_tauschen`, aber mit PSK-Angebot.
    ///
    /// added 02.10.2026, R4d Stufe 2. DER WEG UND SEIN GRUND: die
    /// Versionsentscheidung (`TlsVerbindung`) sitzt **hinter** dem
    /// Hello-Tausch, also muss das Angebot **vor** dieser Entscheidung in
    /// den ClientHello. Ein `angebot`-Parameter an `hallo_tauschen` waere
    /// an allen Aufrufen ein `None`, das man beim Lesen nicht sieht.
    ///
    /// DER EINZIGE UNTERSCHIED zum Rumpf ist die ClientHello-Zeile: mit
    /// Angebot laeuft sie ueber `psk_anbieten` und gibt die **gesendeten**
    /// Bytes zurueck (siehe dort, warum gerade die).
    fn hallo_tauschen_mit_psk(
        &mut self,
        host: &str,
        alpn: &[&[u8]],
        angebot: Option<&crate::ticket::PskAngebot>,
    ) -> Result<Hallo, StromFehler> {
        // M3: EINER (RFC 8446 4.1.4: "A client MUST NOT attempt more than
        // one retry"). Mit PSK gibt es keine Wiederholung — ein HRR bei
        // einer Wiederaufnahme ist nach 4.1.4 verboten zu **begruenden**,
        // und wir begruenden ihn nicht, also gehen wir keinen ein.
        const MAX_HRR: u8 = 0;
        let _ = MAX_HRR;
        let mut geheim = [0u8; 32];
        geheim.copy_from_slice(&zufall::bytes(32).map_err(|_| StromFehler::Zufall)?);
        let eigener_public = x25519::oeffentlich(&geheim);
        let mut random = [0u8; 32];
        random.copy_from_slice(&zufall::bytes(32).map_err(|_| StromFehler::Zufall)?);
        let mut hello = ClientHello::baue_beide(random, host, alpn, &eigener_public)?;
        // added 02.10.2026, R4d Stufe 2: das Angebot **ablegen**, bevor
        // der ClientHello geht. `handshake_ab_serverhello` braucht es fuer
        // den Schluesselplan, und zwar nur, wenn der Server eine Identitaet
        // gewaehlt hat.
        self.psk_angebot = angebot.cloned();
        let ch = match angebot {
            None => hello.kodiere(),
            Some(a) => crate::ticket::psk_anbieten(&mut hello, &a.vorschlaege, &a.psk)
                .map_err(StromFehler::Binder)?,
        };
        self.sende_klartext(typen::HANDSHAKE, &ch)?;
        let sh_nachricht = self.lese_hello()?;
        let sh = ServerHello::dekodiere(&sh_nachricht)?;
        self.pskserverhello_pruefen(&sh)?;
        self.selected_identity = sh.selected_identity;
        if sh.hrr {
            // DER HELLO-RETRY WIRD ABGELEHNT, und das ist eine Entscheidung,
            // keine Vereinfachung: bei einem Angebot duerfen wir die
            // Gruppen neu verhandeln nicht (RFC 8446 4.1.4 verlangt eine
            // Begruendung), und ohne Angebot waere der Weg derselbe wie in
            // `hallo_tauschen` — den nehmen wir **hier** nicht, weil
            // `TlsVerbindung::ueber_sockel_mit_psk` die Wiederaufnahme
            // meint und ein HRR sie zunichte machen wuerde.
            return Err(StromFehler::Binder(
                crate::ticket::BinderFehler::LayoutFalsch {
                    stelle: 0,
                    was: "HelloRetryRequest bei einem PSK-Angebot (nicht begruendet)",
                },
            ));
        }
        Ok(Hallo {
            ch,
            sh_nachricht,
            sh,
            geheim,
            hrr: None,
        })
    }

    /// added 01.10.2026, R4t. **ACHTUNG: an dieser Stelle stand kurz die
    /// BEHAUPTUNG, dieser Weg habe keine Aufrufer. Sie war FALSCH**, und
    /// zwar an der gefaehrlichsten Stelle: `ueber_sockel_mit` (Zeile 2797)
    /// ruft `hallo_tauschen` **ohne jede Bedingung** — das ist der Weg, den
    /// `TlsVerbindung::ueber_sockel` nimmt, also der Weg von `narsil-net`.
    ///
    /// WIE DER FEHLER ZUSTANDE KAM: eine `grep`-Zaehlung nach Aufrufern von
    /// `ueber_sockel_mit` zeigte zwei Aufrufe, beide mit
    /// `verlange_ems = false`, und ich schloss daraus „also laeuft alles
    /// ueber `hallo_tauschen_mit_psk`". Der Fehler: `hallo_tauschen_
    /// mit_psk` wird von **`ueber_sockel_mit_psk`** gerufen, einer
    /// **anderen** Funktion. Das `false` bei `ueber_sockel_mit` sagt nichts
    /// ueber die Wahl zwischen den beiden Hallo-Wegen, denn diese Wahl wird
    /// dort gar nicht getroffen.
    /// GEMESSEN am 01.10.2026, 06:5x, statt behauptet: eine Marke
    /// (`eprintln!("PFAD-MESSUNG: ...")`) in jeder der drei Dekodierstellen,
    /// ein Lauf ueber `tests/psk_ohne_angebot.rs` mit `--nocapture`.
    /// Ergebnis:
    ///   * `TlsStrom::ueber_sockel`            -> `handshake`
    ///   * `TlsVerbindung::ueber_sockel`       -> `hallo_tauschen` (narsil-net)
    ///   * `TlsVerbindung::ueber_sockel_mit_psk` -> `hallo_tauschen_mit_psk`
    ///
    /// DIE REGEL (`pskserverhello_pruefen`) wird an allen drei gerufen, und
    /// `tests/psk_ohne_angebot.rs` deckt alle drei ab (je ein Weg je Test).
    /// DIE LEHRE STEHT HIER UND NICHT IM TEST, weil sie eine Aussage ueber
    /// den Bau des Codes ist: eine **Zaehlung** von Aufrufern beweist
    /// Benutzung nur, wenn man **innerhalb** der aufrufenden Funktion
    /// nachgesehen hat, welcher Weg dort ueberhaupt gewaehlt wird.
    fn hallo_tauschen(&mut self, host: &str, alpn: &[&[u8]]) -> Result<Hallo, StromFehler> {
        // M3: die Anzahl der erlaubten HelloRetryRequests. RFC 8446
        // Abschnitt 4.1.4: "A client MUST NOT attempt more than one
        // retry". Also EINER, und das ist eine Grenze mit einer Quelle
        // in der Norm, nicht mit einer Zahl, die ich gewaehlt habe.
        const MAX_HRR: u8 = 1;
        let mut geheim = [0u8; 32];
        geheim.copy_from_slice(&zufall::bytes(32).map_err(|_| StromFehler::Zufall)?);
        let eigener_public = x25519::oeffentlich(&geheim);
        let mut random = [0u8; 32];
        random.copy_from_slice(&zufall::bytes(32).map_err(|_| StromFehler::Zufall)?);
        // EIN ClientHello fuer beide Versionen. `ClientHello::baue`
        // (nur 1.3) bleibt fuer den reinen TLS-1.3-Weg, damit der
        // Test gegen example.com unveraendert laeuft.
        let hello = ClientHello::baue_beide(random, host, alpn, &eigener_public)?;
        let ch = hello.kodiere();
        // added 04.10.2026, Rolle TLS. **DER ERSTE SCHRITT, und er ist der
        // WICHTIGSTE, weil in ihm die Zeit steckt, die niemand zuordnet.**
        //
        // **GEMESSEN am 04.10.2026** (die Tabelle in `tests/schrittzeit_live.rs`):
        // die uebrigen Schritte summieren sich bei `www.rust-lang.org` auf
        // **14 ms** bei **76 ms** Gesamtdauer. **Die fehlenden 60 ms sind
        // fast alle hier** — ein Rundlauf zum Server, der sein ServerHello
        // erst **nach** eigener Rechnung schickt. **WER NUR DIE SIGNATUREN
        // MESSST, SIEHT 14 ms UND SCHLIESST „unter 80 ms" DARAUS.**
        //
        // **UND DIE TRENNUNG, um die es geht:** in diesem Schritt liegen
        // **Netz-RTT** und **Serverrechnung** **ungetrennt**. Das ist eine
        // Grenze des Werkzeugs, keine Eigenschaft des Handshakes, und sie
        // steht hier, weil sie im Bericht sonst **übersehen** wird.
        // **UND DER MAKRO, nicht eine Uhr mit Handgriff** (das ist der Fehler,
        // den diese Messung zuerst hatte): `let _uhr = uhr(..)` misst **bis zum
        // Ende des umgebenden Blocks**, nicht bis zur naechsten Zeile. Ohne
        // `{ }` hat die erste Fassung **alle** folgenden Schritte mitgemessen
        // und „Schluesselplan: 106 ms“ gemeldet — **das war die ganze restliche
        // Handshake**. Ein Schritt ohne Block ist **keine** Messung, sondern
        // eine **Summe** mit einem Namen darauf.
        // **UND DER BLOCK GIBT BEIDES ZURUECK**, weil `sh_nachricht` die
        // **unveraenderte Nachricht** sein muss und nicht die dekodierte Form:
        // der Transkript-Hash laeuft ueber die **Bytes**. Das ist kein
        // Zufallsfehler beim Umbau, sondern die Regel, an der dieser Block
        // haengt — die erste Fassung gab nur `sh` zurueck und der Uebersetzer
        // meldete „cannot find value `sh_nachricht` in this scope“.
        let (sh_nachricht, sh) =
            crate::schritt!("NETZ+SERVER: ClientHello hin, ServerHello her", {
                self.sende_klartext(typen::HANDSHAKE, &ch)?;
                let sh_nachricht = self.lese_hello()?;
                let sh = ServerHello::dekodiere(&sh_nachricht)?;
                (sh_nachricht, sh)
            });
        self.pskserverhello_pruefen(&sh)?;
        if !sh.hrr {
            return Ok(Hallo {
                ch,
                sh_nachricht,
                sh,
                geheim,
                hrr: None,
            });
        }
        // --- EIN HelloRetryRequest (RFC 8446 Abschnitt 4.1.4) -------
        if MAX_HRR != 1 {
            return Err(StromFehler::ZuViel("mehr als ein HelloRetryRequest"));
        }
        let cookie = sh
            .cookie
            .clone()
            .filter(|c| !c.is_empty())
            .ok_or(StromFehler::HrrOhneCookie)?;
        // Die Gruppe, die der Server verlangt. Er nennt sie im
        // `key_share` des HRR, und der Eintrag ist ZWEI Byte ohne
        // Schluessel.
        let gruppe = sh.gruppe;
        // M3: die Laenge des gewaehlten Schluessels wird geprueft, BEVOR
        // gerechnet wird. `ecdh` und `x25519` moegen sich nicht, ein
        // falscher Laengenwert als Fehler zu melden.
        let (geheim2, share2) = match gruppe {
            nachricht::GRUPPE_X25519 => {
                let mut g = [0u8; 32];
                g.copy_from_slice(&zufall::bytes(32).map_err(|_| StromFehler::Zufall)?);
                let p = x25519::oeffentlich(&g);
                (g, p.to_vec())
            }
            nachricht::GRUPPE_SECP256R1 => {
                let mut g = [0u8; 32];
                g.copy_from_slice(&zufall::bytes(32).map_err(|_| StromFehler::Zufall)?);
                let p = crate::ecdsa::oeffentlich_p256(&g)
                    .map_err(|_| StromFehler::KeinX25519)?
                    .to_vec();
                (g, p)
            }
            other => return Err(StromFehler::GruppeGewuenscht { code: other }),
        };
        let ch2 = hello.wiederholung(gruppe, share2, &cookie).kodiere();
        self.sende_klartext(typen::HANDSHAKE, &ch2)?;
        let sh2_nachricht = self.lese_hello()?;
        let sh2 = ServerHello::dekodiere(&sh2_nachricht)?;
        if sh2.hrr {
            return Err(StromFehler::ZuViel(
                "der Server schickt einen zweiten HelloRetryRequest",
            ));
        }
        Ok(Hallo {
            ch: ch2,
            sh_nachricht: sh2_nachricht,
            sh: sh2,
            geheim: geheim2,
            hrr: Some(Hrr {
                erste_ch: ch,
                hrr_sh: sh_nachricht,
            }),
        })
    }

    /// Der Teil des TLS-1.3-Handshakes **ab dem gelesenen ServerHello**.
    ///
    /// added 30.09.2026 fuer die gemeinsame Verbindungsfunktion: dort
    /// wird genau einmal verbunden und genau einmal ein ClientHello
    /// gesendet, das beide Protokollversionen anbietet. Erst der
    /// ServerHello entscheidet, welcher Weg geht — und bis dahin darf
    /// kein Zustand feststehen, der nur fuer 1.3 gilt.
    ///
    /// `ch` sind die **gesendeten** ClientHello-Bytes, `sh_nachricht`
    /// die gelesenen ServerHello-Bytes. Beide gehen in den Transkript
    /// und in die Schluesselableitung; ein neu gebautes ClientHello
    /// waere geraten.
    /// DER DRITTE TEIL DER WIEDERAUFNAHME, und er ist die haeufigste
    /// Stelle fuer einen Fehler, der sich als Verschluesselung ausgibt:
    /// der **Server** rechnet mit dem PSK, wenn er unser Angebot annimmt —
    /// und der Client muss dasselbe tun, sonst stimmen die Schluessel nicht
    /// und der erste Record des Servers laesst sich nicht entschluesseln.
    ///
    /// added 02.10.2026, R4d Stufe 2. GEMESSEN, und die Meldung war
    /// `Verschluesselung (TLS_AES_128_GCM_SHA256, lesen)` — die denkt
    /// man an einen Fehler in `aes.rs`, und sie entsteht aus zwei
    /// Early Secrets. Genau darum wird hier der Index **geprueft** und
    /// nicht bloss uebernommen: `selected_identity` nennt, **welche**
    /// Identitaet der Server gewaehlt hat (RFC 8446 4.2.11.3), und ein
    /// Index, den es nicht gibt, waere ein Fehler im Server — oder ein
    /// Fehler in uns, und beides sieht von hier gleich aus.
    fn handshake_ab_serverhello(
        &mut self,
        host: &str,
        speicher: &Vertrauensspeicher,
        jetzt: i64,
        hallo: &Hallo,
    ) -> Result<(), StromFehler> {
        let Hallo {
            ch,
            sh_nachricht,
            sh,
            geheim,
            hrr,
        } = hallo;
        // `ServerHello` ist nicht `Copy` (es hat Felder vom Typ
        // `Vec`), deshalb wird es als Referenz weitergereicht. Die
        // beiden Nachrichten brauchen nur ihre Bytes.
        // `geheim` ist durch die Zuweisung an `&Hallo` bereits
        // `&[u8; 32]` — genau die Form, die `x25519::gemeinsam` nimmt.
        let (ch, sh_nachricht) = (ch.as_slice(), sh_nachricht.as_slice());
        // Der Transkript wird HIER angelegt, aus den gesendeten und den
        // gelesenen Bytes — nicht vorher in `handshake`. Er wird an
        // zwei Stellen gebraucht (bis einschliesslich `Certificate` und
        // bis einschliesslich `CertificateVerify`), und ein zweiter
        // Aufbau waere die Stelle, an der ein Fehler am billigsten zu
        // machen ist.
        // added 04.10.2026: **DIE ZEITBASIS DER TICKETS BLEIBT AM STROM.**
        // `jetzt` kommt vom Aufrufer (`ueber_sockel(…, jetzt)`) und wird fuer
        // die Ticket-Ablage in der Anwendungsphase gebraucht — dort hat `read`
        // **keinen** Aufrufwert mehr.
        self.nachschrift_zeit = jetzt.max(0) as u64;
        let mut transkript = Sha256::neu();
        // DER TRANSCRIPT MIT HELLORETRYREQUEST (RFC 8446 Abschnitt
        // 4.1.1): der erste ClientHello und der HRR selbst stehen
        // **nicht** im Klartext im Transkript, sondern als
        // `message_hash` bzw. als empfangene Nachricht:
        //
        //   message_hash(ClientHello 1) || HelloRetryRequest ||
        //   message_hash(ClientHello 2) || ServerHello || ...
        //
        // DER FEHLER, DEN DIESER ZWEIG VERHINDERT: nimmt man
        // ClientHello 1 und ClientHello 2 **beide im Klartext**, stimmt
        // keine der Schluesselableitungen, und der Fehler ist ein
        // AEAD-Fehler an einem Record — "Verschluesselung", nicht
        // "Transkript".
        match hrr {
            None => {
                transkript.aufnehmen(ch);
            }
            Some(vorg) => {
                transkript.aufnehmen(&message_hash(&vorg.erste_ch));
                transkript.aufnehmen(&vorg.hrr_sh);
                transkript.aufnehmen(&message_hash(ch));
            }
        }
        transkript.aufnehmen(sh_nachricht);
        self.suite = Suite::aus_code(sh.cipher_suite).ok_or(StromFehler::KeineSuite)?;
        // DAS GEMEINSAME GEHEIMNIS, je nach Gruppe des ServerHello.
        //
        // added 30.09.2026 fuer W6. Vorher stand hier fest x25519,
        // und ein Server, der nach einem HelloRetryRequest P-256
        // wollte, schickt 65 Byte statt 32 — der Fehler war
        // `KeinX25519`, und die Meldung sagte nichts ueber die Gruppe.
        //
        // FUER BEIDE GRUPPEN GILT: das Geheimnis ist die x-Koordinate,
        // 32 Byte (RFC 8446 Abschnitt 4.2.11.2 fuer x25519, und
        // `ecdsa::ecdh` liefert sie fuer P-256 genauso). Deshalb passt
        // es in beide Schluesselplaene, die jeweils `[u8; 32]` nehmen.
        let shared: [u8; 32] = match sh.gruppe {
            nachricht::GRUPPE_X25519 => {
                let server_public = sh.x25519().ok_or(StromFehler::KeinX25519)?;
                let mut gegenschluessel = [0u8; 32];
                gegenschluessel.copy_from_slice(&server_public);
                x25519::gemeinsam(geheim, &gegenschluessel).map_err(|_| StromFehler::X25519)?
            }
            nachricht::GRUPPE_SECP256R1 => {
                // M3/M4: die Laenge des Punkts wird VOR dem Rechnen
                // geprueft. `ecdh` liest [1..33] und [33..65], und ein
                // 32-Byte-Feld wuerde dort eine Panic ausloesen statt
                // eines Fehlers.
                if sh.key_share.len() != 65 || sh.key_share[0] != 0x04 {
                    return Err(StromFehler::PunktFalsch {
                        war: sh.key_share.len(),
                    });
                }
                let k = crate::ecdsa::kurve_p256().map_err(|_| StromFehler::KeinX25519)?;
                let v = crate::ecdsa::ecdh(geheim, &sh.key_share[1..33], &sh.key_share[33..65], &k)
                    .map_err(|_| StromFehler::X25519)?;
                // M4: `ecdh` gibt 32 Byte, die Laenge wird aber
                // nachgeprueft — eine Aenderung an `ecdh` darf nicht
                // unmerklich einen Puffer schlichten.
                if v.len() != 32 {
                    return Err(StromFehler::PunktFalsch { war: v.len() });
                }
                let mut raus = [0u8; 32];
                raus.copy_from_slice(&v);
                raus
            }
            code => return Err(StromFehler::GruppeGewuenscht { code }),
        };

        // Der Schluesselplan braucht ClientHello..ServerHello im
        // Transkript; er behaelt seinen eigenen, und genau deshalb
        // ist der zweite Transkript (`transkript` hier) noetig.
        //
        // DER PLAN MIT PSK, und die Bedingung ist **beide** Seiten:
        // der Server muss eine Identitaet gewaehlt haben (`sh`), **und**
        // wir muessen ein Angebot gemacht haben (`self.psk_angebot`).
        //
        // DER FALL „Server sagt `Some(0)`, wir haben nichts angeboten" ist
        // ein Fehler und wird als solcher gemeldet: ein Server, der eine
        // Identitaet waehlt, die es nicht gibt, redet entweder mit einem
        // anderen Client oder er ist kaputt. Beides ist eine Alert
        // (`illegal_parameter`), kein stiller Rueckfall.
        //
        // DER FALL „Server sagt `None`, wir haben angeboten" ist der
        // **normale** Rueckfall: der Server kann ablehnen (RFC 8446
        // 4.2.11.3), und dann machen wir einen vollstaendigen Handshake.
        let mut plan = match (sh.selected_identity, self.psk_angebot.as_ref()) {
            (None, _) => Schluesselplan::neu(shared),
            (Some(_), None) => {
                return Err(StromFehler::Unerwartet(
                    "der Server hat eine PSK-Identitaet gewaehlt, aber wir haben keine \
                     angeboten (illegal_parameter nach RFC 8446 4.2.11.3)",
                ));
            }
            (Some(index), Some(angebot)) => {
                // DER INDEX WIRD GEPRUEFT, und die Fehlerart ist benannt:
                // ein Index ausserhalb der Liste heisst, dass Server und
                // Client von verschiedenen Angeboten reden.
                if index as usize >= angebot.vorschlaege.len() {
                    return Err(StromFehler::Unerwartet(
                        "selected_identity zeigt auf eine Identitaet, die wir nicht \
                         angeboten haben",
                    ));
                }
                Schluesselplan::mit_psk(shared, angebot.psk.clone()).map_err(|e| {
                    StromFehler::Binder(match e {
                        crate::record::PlanFehler::PskLeer => crate::ticket::BinderFehler::Leer,
                        crate::record::PlanFehler::PskZuLang { .. } => {
                            crate::ticket::BinderFehler::ZuLang {
                                war: angebot.psk.len(),
                                max: crate::ticket::MAX_TICKET_BYTES,
                            }
                        }
                    })
                })?
            }
        };
        // Derselbe Transkript wie oben, mit dem HRR-Ersatz.
        match hrr {
            None => plan.aufnehmen(ch),
            Some(vorg) => {
                plan.aufnehmen(&message_hash(&vorg.erste_ch));
                plan.aufnehmen(&vorg.hrr_sh);
                plan.aufnehmen(&message_hash(ch));
            }
        }
        plan.aufnehmen(sh_nachricht);
        plan.handshakephase().map_err(StromFehler::Schluesselplan)?;
        // added 02.10.2026 05:1x: DER SCHLUESSELLOG, und er kommt **hier**,
        // weil das der erste Punkt ist, an dem beide Handshake-Geheimnisse
        // feststehen. Die Anwendungsgeheimnisse kommen weiter unten, nach
        // `anwendungsphase_client` — dort wird der zweite Satz geschrieben.
        // added 02.10.2026 05:1x: der Zufall wird aus `ch` gelesen, dem
        // **gesendeten** ClientHello, statt als weiteres Feld im `TlsStrom`
        // mitgeschleppt zu werden; die Nachricht liegt ohnehin vor.
        if let (Some(pfad), Some(r)) = (
            crate::record::keylog_pfad(),
            crate::record::client_random_aus_hello(ch),
        ) {
            crate::record::keylog_schreiben(
                &pfad,
                &r,
                &plan.client_hs_traffic,
                &plan.server_hs_traffic,
                // added 02.10.2026 05:1x: die Anwendungsgeheimnisse sind zu
                // diesem Zeitpunkt **null** (sie kommen erst nach dem
                // ServerFinished). Deshalb werden sie hier nicht geschrieben
                // und der Log bekommt seine vier Zeilen an **zwei** Stellen —
                // das ist der Grund fuer `SERVER_TRAFFIC_SECRET_0` nicht beim
                // Handshake zu stehen.
                &[0u8; 32],
                &[0u8; 32],
            );
        }

        // Und die Handshake-Schluessel fuer beide Richtungen.
        //
        // ECHTER FEHLER: hier standen die beiden vertauscht. Der
        // Client SENDET mit `client_hs_traffic` und EMPFAENGT mit
        // `server_hs_traffic` — die Namen stehen fuer den ABSENDER
        // (RFC 8446 Abschnitt 7.1), nicht fuer den Empfaenger. Ich
        // hatte umgekehrt gelesen und damit mit dem Schluessel des
        // Servers gelesen und mit dem des Clients geschrieben.
        //
        // Wie es sich geaeussert hat: die Entschluesselung des ersten
        // Server-Records schlug fehl, mit der Meldung
        // "Verschluesselung (TLS_AES_128_GCM_SHA256, lesen)". Ein
        // AEAD-Fehler sieht genauso aus wie ein falscher Schluessel,
        // ein falsches AAD und eine falsche Nonce — vier Moeglichkeiten,
        // eine Meldung.
        //
        // DIE REGEL, die daraus folgt: bei einem AEAD-Fehler die
        // Reihenfolge abfragen, BEVOR man an der Implementierung
        // zweifelt. Die vier Kandidaten sind Transkript, Schluessel
        // der Richtung, Nonce und AAD — und die Richtung wird am
        // leichtesten vertauscht.
        // added 04.10.2026, Rolle TLS. **DER SCHLUESSELPLAN ALS SCHRITT.**
        // Erwartet: **unter 0,5 ms** (HKDF ueber 32 Byte, viermal). Und
        // **gerade deshalb** wird er gemessen: **eine** Stelle in diesem
        // Handshake ist bisher **nie** teuer gewesen, und ohne Messung ist das
        // eine **Annahme**.
        // **DER BLOCK, aus demselben Grund wie oben.** Erwartet: **unter
        // 0,5 ms** — vier HKDF-Aufrufe ueber 32 Byte. **GEMESSEN in der
        // ersten Fassung waren 4 bis 106 ms**, und das waren **nicht** die
        // HKDF-Aufrufe, sondern **der Rest des Handshakes**, den der Wächter
        // mitgemessen hat. Die Zahl war **falsch**, und sie sah aus wie ein
        // Befund.
        let (ks, ivs, ke, ive) = crate::schritt!("RECHNEN: Schluesselplan (HKDF)", {
            let (ks, ivs) = self
                .suite
                .keying_material(&plan.client_hs_traffic)
                .map_err(StromFehler::Schluesselplan)?;
            let (ke, ive) = self
                .suite
                .keying_material(&plan.server_hs_traffic)
                .map_err(StromFehler::Schluesselplan)?;
            (ks, ivs, ke, ive)
        });
        self.s_schluessel = ks;
        self.s_iv = ivs;
        self.e_schluessel = ke;
        self.e_iv = ive;

        // --- 4) Der verschluesselte Flug ---------------------------------
        // added 04.10.2026, Rolle TLS. **DIE NETZZEIT ALS EIGENER SCHRITT**,
        // und das ist der wichtigste Schritt in dieser Liste, weil er **die
        // ganze Wartezeit** enthaelt — Server denkt nach, Signatur berechnet,
        // Antwort geht zurueck. **GEMESSEN (Bericht 22): 16–25 ms RTT bei
        // 133–543 ms Handshake**, also ist der Rest **Rechnen**, und diesen
        // Rest findet man nur, wenn das Netz **getrennt** ausgewiesen wird.
        //
        // **UND OHNE DIESE TRENNUNG** waere jede spaetere Zahl eine Zahl
        // **inklusive** Netz — und man wuerde die 40 ms Signaturpruefung fuer
        // den Engpass halten, waehrend es 400 ms Kette waren.
        let nachrichten = crate::schritt!("NETZ: Flug des Servers lesen", { self.lese_flug()? });
        self.klattr.clear();
        let mut ende_gefunden = false;
        for n in &nachrichten {
            if !n.is_empty() && n[0] == nachricht::typen::FINISHED {
                ende_gefunden = true;
            }
        }
        if !ende_gefunden {
            return Err(StromFehler::ZuViel("kein finished im Flug"));
        }

        // --- 5) Die Nachrichten auswerten --------------------------------
        // M7: es wird nichts geraten. `EncryptedExtensions` wird
        // geparst und verworfen, `Certificate` ergibt die Kette, und
        // alles, was nicht gelesen werden kann, ist ein Fehler.
        let mut zertifikate: Vec<Vec<u8>> = Vec::new();
        let mut cv: Option<CertificateVerify> = None;
        let mut finished: Option<Finished> = None;
        // Der Index der CertificateVerify-Nachricht im Flug.
        let mut cv_index = 0usize;
        // Der Transkript bis einschliesslich `Certificate`: das ist die
        // Grundlage fuer die Signaturpruefung.
        let mut transkript_bis_cert = transkript.clone();
        // Der Transkript bis einschliesslich `CertificateVerify`. Der
        // SERVER prueft sein `finished` darueber.
        let mut transkript_bis_cv = transkript.clone();
        // Der Transkript bis einschliesslich dem `finished` des SERVERS.
        // Der CLIENT prueft sein `finished` darueber, und aus genau
        // diesem Stand kommen auch die Anwendungsgeheimnisse.
        //
        // ECHTER FEHLER, und der letzte, der am Handshake hing. Ich
        // hatte beide Seiten denselben Transkript benutzen lassen und
        // damit das `finished` des Clients auf CH..CertificateVerify
        // gebaut. Das ist ein anderes Geheimnis.
        //
        // RFC 8446 Abschnitt 4.4.4 schreibt fuer `verify_data` nur
        //
        //     HMAC(finished_key, Transcript-Hash(Handshake Context,
        //                     Certificate*, CertificateVerify*))
        //
        // und laesst offen, was der Client zusaetzlich einbezieht. Die
        // Antwort steht nicht in diesem Abschnitt, sondern in RFC 8448
        // Abschnitt 3, und sie ist eindeutig: der Client rechnet ueber
        // `Hash(ClientHello .. ServerFinished)`.
        //
        // GEMESSEN am 30.09.2026 mit
        // `/home/jaff/mutation_md/tls/t13_client_finished.py`, das jeden
        // Wert aus `rfc/rfc8448.txt` liest und nachzaehlt:
        //
        //     HMAC(finished_key_client, 9608102a…)  = a8ec436d…
        //     a8ec436d… ist die verify_data des Client-`finished`
        //     aus RFC 8448
        //
        // Derselbe Hash `9608102a…` ist dort als Transkript fuer
        // `c ap traffic` ausgewiesen. Es ist also EIN Stand, und er
        // endet beim Server-`finished` — nicht beim
        // `CertificateVerify` und selbstverstaendlich nicht beim
        // Client-`finished`, das zu diesem Zeitpunkt noch gar nicht
        // existiert.
        let mut transkript_bis_server_finished = transkript.clone();

        for (i, n) in nachrichten.iter().enumerate() {
            if n.is_empty() {
                continue;
            }
            match n[0] {
                nachricht::typen::ENCRYPTED_EXTENSIONS => {
                    // DIE ALPN-KENNUNG BLEIBT STEHEN.
                    //
                    // added 30.09.2026, Runde 2, TLS R1. Bis hier stand
                    // `let _ee = ...`: die `EncryptedExtensions` wurden
                    // gelesen, geprueft (die Reihenfolge im Flug) und
                    // dann **verworfen**. Damit wusste niemand, ob der
                    // Server `h2` oder `http/1.1` gewaehlt hat — und
                    // genau das ist die Groesse, an der `narsil` und
                    // `narsil-net` entscheiden (RFC 7301 3.2: ohne ALPN
                    // gilt HTTP/1.1). Der Gegenbeweis ist der Rundlauf
                    // in `tests/rundlauf.rs`, der auf beiden Seiten
                    // dieselbe Kennung sehen will.
                    let ee = EncryptedExtensions::dekodiere(n)?;
                    if let Some(gewaehlt) = &ee.alpn {
                        self.alpn = Some(gewaehlt.clone());
                    }
                    transkript_bis_cert.aufnehmen(n);
                    transkript_bis_cv.aufnehmen(n);
                    transkript_bis_server_finished.aufnehmen(n);
                }
                nachricht::typen::CERTIFICATE => {
                    // added 04.10.2026, Rolle TLS. **DIE X509-DEKODIERUNG ALS
                    // EIGENER SCHRITT** und **nicht** als Teil der Kettenpruefung:
                    // ein Blatt mit zwei Erweiterungen ist 1 KB, die Kette
                    // darum herum ist **dieselbe** Zahl — wer beides in eine
                    // Zeile schreibt, kann nach dem Optimieren nicht sagen, ob
                    // der Leser oder die Pruefung teuer war.
                    let c = crate::schritt!("RECHNEN: Zertifikate dekodieren", {
                        Certificate::dekodiere(n)?
                    });
                    // M3/M4: die Liste kommt vom Server.
                    if c.zertifikate.len() > MAX_ZERTIFIKATE_KETTE {
                        return Err(StromFehler::ZuViel("zu viele Zertifikate"));
                    }
                    for z in &c.zertifikate {
                        if z.len() > crate::x509::MAX_ZERTIFIKAT {
                            return Err(StromFehler::ZuViel("Zertifikat zu lang"));
                        }
                    }
                    // added 02.10.2026 23:jx UTC (Projektlabel 02.10.2026),
                    // S4. DIE STAPELTE ANTWORT SICHERN. In TLS 1.3 steckt sie in
                    // der Erweiterung `status_request` dieser Nachricht
                    // (RFC 8446 4.4.3) — es gibt dort keine
                    // `CertificateStatus`-Nachricht. GEMESSEN 23:fx: `xkcd.com`
                    // und `www.netbsd.org` liefern in TLS 1.3 beide eine
                    // OCSP-Antwort, in TLS 1.2 aber in 0 von 5 Handshakes eine.
                    // Wer nur den 1.2-Weg liest, wertet die Ausnahme aus.
                    //
                    // added 02.10.2026 23:jx: DIE FEHLER BEENDEN DEN
                    // HANDSHAKE NICHT. `ocsp_aus_zertifikatsblock` kann einen
                    // kaputten Block melden; das ist eine kaputte
                    // Zertifikatsnachricht, aber der Server hat das Zertifikat
                    // schon gesendet und die Kette steht schon. Wir merken uns
                    // den Grund und melden `Unbekannt` — ein Absturz waere
                    // hier eine **neue** Abhaengigkeit von einem Server, dessen
                    // OCSP-Antwort wir sowieso nicht auswerten koennen.
                    match nachricht::ocsp_aus_zertifikatsblock(&c.erweiterungen) {
                        Ok(Some(nutzlast)) => self.ocsp_nutzlast = Some(nutzlast),
                        Ok(None) => {}
                        Err(e) => {
                            self.ocsp_grund = "die Zertifikats-Erweiterungen sind unlesbar";
                            eprintln!("OCSP: Erweiterungsblock: {e}");
                        }
                    }
                    zertifikate = c.zertifikate;
                    transkript_bis_cert.aufnehmen(n);
                    transkript_bis_cv.aufnehmen(n);
                    transkript_bis_server_finished.aufnehmen(n);
                }
                nachricht::typen::CERTIFICATE_VERIFY => {
                    cv = Some(CertificateVerify::dekodiere(n)?);
                    cv_index = i;
                    // Ab hier gehoert `CertificateVerify` in den
                    // `finished`-Transkript, aber nicht mehr in den
                    // Signatur-Transkript.
                    transkript_bis_cv.aufnehmen(n);
                    transkript_bis_server_finished.aufnehmen(n);
                }
                nachricht::typen::FINISHED => {
                    finished = Some(Finished::dekodiere(n)?);
                    // Ab hier endet der Transkript. Das eigene
                    // `finished` des Clients gehoert NICHT mehr hinein
                    // (RFC 8446 Abschnitt 4.4.4, Fussnote), sonst
                    // waere es sein eigener Transkript.
                    transkript_bis_server_finished.aufnehmen(n);
                }
                _ => {
                    // Alles andere wird uebersprungen, aber mitgenommen
                    // — eine unbekannte Nachricht zwischen `Certificate`
                    // und `CertificateVerify` aendert den Transkript.
                    transkript_bis_cert.aufnehmen(n);
                    transkript_bis_cv.aufnehmen(n);
                    transkript_bis_server_finished.aufnehmen(n);
                }
            }
        }
        //
        // added 01.10.2026, R4q: DER KURZE HANDSHAKE, Client-Seite.
        //
        // RFC 8446 Abschnitt 4.2.11: bei einer Wiederaufnahme schickt der
        // Server **kein** `Certificate` und **kein** `CertificateVerify`.
        // Der Client darf sie deshalb **nicht verlangen** — genau das war
        // der Fehler, als der Server sie zum ersten Mal weggelassen hat:
        // `Unerwartet("kein Zertifikat")` (GEMESSEN 01.10.2026, 05:5x).
        //
        // DIE FLAGGE: ein Angebot **und** eine vom Server gewaehlte
        // Identitaet. WARUM NICHT NUR `psk_angebot.is_some()`: ein Client
        // kann ein Angebot machen und der Server es ablehnen; dann kommt
        // ein **vollstaendiger** Handshake mit Zertifikat, und der Client
        // muss es pruefen. Verlangte er dann keins, wuerde er einen Server
        // ohne Identitaetspruefung akzeptieren — eine echte
        // Sicherheitsluecke, und sie waere still.
        let wiederaufgenommen = self.psk_angebot.is_some() && sh.selected_identity.is_some();
        if !wiederaufgenommen && zertifikate.is_empty() {
            return Err(StromFehler::Unerwartet("kein Zertifikat"));
        }
        //
        // DIE `CertificateVerify`, und bei einer Wiederaufnahme ist sie
        // **leer statt fehlend**: der spaetere Block rechnet
        // `cv_gueltig(blatt, &cv, …)`, und ein `Option` dort waere eine
        // Aenderung an drei weiteren Stellen. Leer ist hier in diesem einen
        // Zweig richtig, weil der Block gar nicht laeuft — siehe dort.
        let cv = match cv {
            Some(v) => v,
            // DER PLATZHALTER, und er wird **nicht** benutzt: der Block, der
            // `cv` liest, laeuft bei einer Wiederaufnahme nicht. Eine leere
            // Signatur waere an dieser Stelle ein Wert, den jemand spaeter
            // pruefen koennte — und der Fehler, den ich in R4f hatte, war
            // genau ein Platzhalter, der aussieht wie ein Geheimnis
            // (`s_ap=[00, 00, 00, 00]`). Deshalb `Schatten` **und** nicht
            // `[0u8; 64]`, und der Kommentar sagt, dass es unbenutzt ist.
            None if wiederaufgenommen => CertificateVerify {
                algorithmus: crate::nachricht::SCHEMA_ECDSA_P256_SHA256,
                signatur: Vec::new(),
            },
            None => return Err(StromFehler::Unerwartet("kein CertificateVerify")),
        };
        let finished = finished.ok_or(StromFehler::Unerwartet("kein Finished"))?;
        let _ = cv_index;

        //
        // added 01.10.2026, R4q: DIE PRUEFUNGEN 6 UND 7, und sie laufen
        // **nur** bei einem vollen Handshake.
        //
        // DER GRUND, und er ist nicht „der Server schickt es nicht mehr",
        // sondern: bei einer Wiederaufnahme gibt es **nichts zu pruefen**.
        // Der Sitzungsschluessel kam aus einem Ticket, das dieser Server
        // selbst ausgestellt hat; die Kette hat der Client bei der ersten
        // Sitzung geprueft. Ein zweites Pruefen waere Arbeit ohne
        // Information — und ein **leeres** Zertifikat durch `kette_pruefen`
        // zu schicken waere ein Fehler, der aussieht wie ein gueltiger
        // Handshake.
        //
        // DIE EINZIGE SACHE, DIE TROTZDEM IMMER GILT: das `finished` des
        // Servers (Abschnitt 8) wird **immer** geprueft, auch bei einer
        // Wiederaufnahme. Es belegt, dass der Server den Sitzungsschluessel
        // hat — und genau das ist die Aussage, auf die es hier ankommt.
        if !wiederaufgenommen {
            // --- 6) Die Blatt-Signatur ---------------------------------------
            // Die ganze Liste wird gelesen, nicht nur das Blatt: der
            // Aussteller des Blatts steht in der `Certificate`-Nachricht,
            // und ohne die Liste gibt es fuer die meisten Server keinen
            // Pfad. `zertifikat_lesen` prueft `MAX_ZERTIFIKAT` selbst, und
            // die Zahl der Eintraege ist schon bei `Certificate::dekodiere`
            // gegen `MAX_ZERTIFIKATE_KETTE` begrenzt.
            //
            // M4: `Vec::with_capacity` folgt aus dieser bereits geprueften
            // Zahl, nicht aus der Laenge der Nachricht.
            let mut ganze_kette: Vec<crate::x509::Zertifikat> =
                Vec::with_capacity(zertifikate.len());
            for der in &zertifikate {
                ganze_kette.push(
                    zertifikat_lesen(der)
                        .map_err(|_| StromFehler::Unerwartet("Zertifikat unlesbar"))?,
                );
            }
            let blatt = ganze_kette
                .first()
                .ok_or(StromFehler::Unerwartet("kein Zertifikat"))?;
            let th_cv = transkript_bis_cert.fertig();
            let inhalt = certificate_verify_inhalt(crate::signatur::VORLAGE_SERVER, &th_cv)
                .map_err(|_| StromFehler::SignaturFalsch)?;
            // added 04.10.2026, Rolle TLS. **DIE EINZIGE SIGNATURPRUEFUNG, die
            // ein Browser bei jedem TLS-1.3-Handshake macht** (eine pro
            // Handshake, nicht eine pro Zertifikat — die Kette hat ihre eigene
            // Stelle). **RSA-PSS oder ECDSA P-256**, und **hier** ist der Punkt,
            // an dem eine langsame Big-Integer-Potenzierung oder eine
            // Skalarmultiplikation **auffällt**.
            crate::schritt!("RECHNEN: Signatur des Servers (CertificateVerify)", {
                if !cv_gueltig(&blatt.schluessel, &cv, &inhalt) {
                    return Err(StromFehler::SignaturFalsch);
                }
            });

            // --- 7) Die Kette und der Hostname ------------------------------
            // VOR dem `finished`, und ohne Umweg. Ohne gueltige Kette
            // entsteht kein Strom. Uebergeben wird die Liste aus der
            // `Certificate`-Nachricht, Blatt zuerst.
            // added 03.10.2026 16:1x: **DIE KETTE WIRD GESPEICHERT, STATT
            // VERWORFEN** — und das ist dieselbe Fehlerform, die R51 am Feld
            // `_kritisch` und R53 an `pfad_laenge` gefunden hat: ein Wert wird
            // **geliefert** und **weggeworfen**. `kette_pruefen` liefert die
            // geprueffte Kette, und sie geht jetzt in `kette_der` — damit
            // `narsil-net` die CRL beschaffen kann (GEMESSEN 16:0x: 9 von 10
            // echten Zertifikaten tragen eine CRL-Distribution-Point-URL, und
            // die antworten mit HTTP 200 — die CRL-Abfrage ist **lebendig**).
            // added 04.10.2026, Rolle TLS. **DIE KETTE ALS EIGENER SCHRITT.**
            // **WARUM SIE GETRENNT VOM BLATT:** `kette_pruefen` macht fuer
            // **jedes** Zertifikat der Kette eine Signaturpruefung und eine
            // Gueltigkeitspruefung. Bei einer vierstelligen Kette sind das
            // **vier** Big-Integer-Potenzierungen — und **die** sind der
            // Grund, warum ein Handshake 400 ms statt 80 ms dauern kann.
            // **GEMESSEN in `kette::wo_geht_die_zeit_im_handshake_hin`:** eine
            // kalte Kettenpruefung **23,6 ms**, eine warme **9,7 us**. Die
            // Differenz ist die **Memo-Tabelle**, nicht die Kryptografie.
            // Release 10.10.2026: eine gescheiterte Kette ist mit einer
            // SITZUNGSAUSNAHME fuer genau dieses Blatt erlaubt (`ausnahme`,
            // „Risiko akzeptieren“); sonst wird das Blatt fuer die Freigabe
            // gemerkt und der Fehler gemeldet wie bisher.
            let kette = crate::schritt!("RECHNEN: Kette pruefen (alle Zertifikate)", {
                match kette_pruefen(&ganze_kette, speicher, jetzt, host) {
                    Ok(k) => Some(k),
                    Err(e) => {
                        let fp = crate::ausnahme::fingerabdruck(&ganze_kette[0].der);
                        if !crate::ausnahme::erlaubt(host, &fp) {
                            crate::ausnahme::abgelehnt_merken(host, fp);
                            return Err(e.into());
                        }
                        None
                    }
                }
            });
            // added 03.10.2026 16:1x: **`MAX_KETTEN_TIEFE` ALS SCHRANKE**, und
            // sie ist **nicht** kosmetisch: `kette_pruefen` kann eine Kette
            // zurueckgeben, die laenger ist als unsere eigene Schranke, wenn der
            // Vertrauensspeicher eine ungewoehnlich tiefe Kette anbietet. Ohne
            // `take` waere das eine **unbegrenzte** Liste auf einem Sockel, auf
            // dem ein Angreifer Server spielt.
            self.kette_der = match &kette {
                Some(kette) => kette
                    .von_blatt_nach_wurzel
                    .iter()
                    .take(crate::kette::MAX_KETTEN_TIEFE)
                    .map(|z| z.der.clone())
                    .collect(),
                // Mit Ausnahme: die vorgelegte Kette, wie sie kam.
                None => ganze_kette
                    .iter()
                    .take(crate::kette::MAX_KETTEN_TIEFE)
                    .map(|z| z.der.clone())
                    .collect(),
            };

            // added 02.10.2026 23:jx, S4. DIE STAPELTE ANTWORT AUSWERTEN, und
            // zwar **nach** der Kette. Das ist Absicht: eine Widerrufsantwort
            // ist nur dann eine Aussage, wenn das Zertifikat selbst gueltig
            // ist. Ein abgelaufenes Zertifikat mit „gueltig" im OCSP ist kein
            // Widerspruch, sondern zwei Fehler — der schwerere kommt zuerst.
            //
            // added 02.10.2026 23:jx: DIE REIHENFOLGE IM CODE folgt der im
            // 1.2-Pfad: `revoked` bricht ab, `Gut` wird gemerkt,
            // `Unbekannt` wird gemerkt. Sonst muesste man an zwei Stellen
            // nachlesen, was ein Urteil bedeutet.
            // added 03.10.2026 02:11x, R48. **MUST-STAPLE, UND ZWAR VOR DEM
            // OCSP-BLOCK** — das ist die Korrektur, und sie kam aus einem
            // **gemessenen** Fehlschlag: mein Test `must_staple_bricht_den_
            // handshake_ab` schlug fehl mit „must-staple wurde angenommen“.
            // Grund: die Pruefung stand **innen** im `if let Some(nutzlast)`
            // (R44) — sie lief also **nur**, wenn der Server ueberhaupt eine
            // OCSP-Antwort stapelt. Genau der Fall, den must-staple verhindern
            // soll (der Angreifer schneidet die Antwort ab), liess die Pruefung
            // **nie** laufen. Ein Merkmal, das nur wirkt, wenn die
            // Gegenleistung da ist, ist kein Merkmal.
            //
            // added 03.10.2026 02:11x: DIE REGEL: verlangt das Blatt die
            // Stapelung (Merkmalsnummer 5, RFC 7633) und kam **keine**
            // brauchbare Antwort, ist die Verbindung abzulehnen. `ocsp_stufe`
            // ist zu diesem Zeitpunkt noch der Anfangswert, also ist „keine
            // Antwort“ gleichbedeutend mit „nicht gut“.
            // added 03.10.2026 03:8x, R49: **DIE MUST-STAPLE-PRUEFUNG STEHT
            // JETZT HINTER DER OCSP-AUSWERTUNG**, und ihre alte Stelle war ein
            // **Fehler von mir**, den erst der neue Test aufgedeckt hat.
            //
            // added 03.10.2026 03:8x: WAS ICH DAMALSENTSCHIEDEN HABE. In R44 habe
            // ich die Pruefung bewusst **vor** den OCSP-Block gesetzt — mit der
            // Begruendung, sie solle greifen, bevor irgendetwas anderes passiert.
            // GEMESSEN ist das die **falsche** Reihenfolge: `ocsp_stufe` wird
            // **erst** im OCSP-Block gesetzt, und der Block laeuft ueber
            // `self.ocsp_nutzlast.take()` — steht die Pruefung davor, sieht sie
            // ausnahmslos den Anfangswert `KEINE`, und **jede** must-staple-
            // Verbindung bricht ab, egal ob der Server eine Antwort stapelt.
            // Das war bis eben unbemerkt, weil beide messbaren Faelle „keine
            // Antwort“ ergaben: der Test `must_staple_mit_gelieferter_
            // stapelung_steht_die_verbindung` war **rot** (03:7x) und ist der
            // Beweis — er ist genau der Fall, den es vorher nicht gab.
            //
            // added 03.10.2026 03:8x: **DIE LEHRE, die ueber diesen Block
            // hinausgeht.** Eine Pruefung, deren Eingang eine **Zustandsvariable**
            // ist, gehoert **nach** die Stelle, die diese Variable setzt. Sonst
            // prueft man nicht das, was man zu pruefen glaubt — man prueft den
            // Anfangswert. Das ist dasselbe Muster wie in R37 bei der
            // `MAX_FRAME_SIZE`-Durchsetzung (erzwungen, aber nicht gesetzt).
            if let Some(nutzlast) = self.ocsp_nutzlast.take() {
                // added 02.10.2026 23:jx: DER AUSSTELLER MUSS MITKOMMEN. Ohne
                // das zweite Zertifikat fehlt der Schluessel der CA, und damit
                // sind CertID und Signatur nicht nachrechenbar. Der Server
                // darf das verschweigen (RFC 8446 4.4.2 verlangt nur die
                // Zertifikate, die der Client zum Aufbau der Kette braucht) —
                // wir melden es dann als `Unbekannt` und sterben nicht daran.
                let a = if ganze_kette.len() >= 2 {
                    crate::ocsp::antwort_lesen(&nutzlast)
                } else {
                    Err(crate::ocsp::OcspFehler::Aufbau(
                        "die Kette traegt den Aussteller nicht mit".into(),
                    ))
                };
                let urteil = match a {
                    Err(crate::ocsp::OcspFehler::KeineAntwort(code)) => {
                        eprintln!("OCSP: der Responder hat nicht geantwortet, Code {code}");
                        crate::ocsp::Urteil::Unbekannt("der Responder hat nicht geantwortet")
                    }
                    Err(e) => {
                        eprintln!("OCSP: {e}");
                        crate::ocsp::Urteil::Unbekannt("die Antwort war unlesbar")
                    }
                    Ok(a) => crate::ocsp::beweise(&a, &ganze_kette[0], &ganze_kette[1], jetzt),
                };
                // added 02.10.2026 23:9x, S4: **MUST-STAPLE (RFC 7633)**. Nach
                // dem Urteil, aber **vor** `Finished` — die Reihenfolge ist
                // wichtig: wir geben die Verbindung auf, ohne dem Server zu
                // bestaetigen, dass sie steht.
                //
                // added 02.10.2026 23:9x: DIE REGEL. Das Zertifikat kann per
                // TLS-Feature-Erweiterung (1.3.6.1.5.5.7.1.24) die
                // status_request-Merkmalsnummer 5 verlangen. Dann ist eine
                // brauchbare OCSP-Antwort Pflicht; woertlich RFC 7633 4.2.3.1:
                // „it **SHOULD reject** the TLS configuration“, und 2.3: „permits
                // a client to fail immediately if the certificate status
                // information is not provided by the server“.
                //
                // added 02.10.2026 23:9x: **GEMESSEN, dass es nichts kostet**
                // (02.10.2026, `openssl s_client -status` gegen sechs Hosts):
                // **0 von 6** tragen die Erweiterung, und **0 von 6** stapeln
                // eine OCSP-Antwort. Die Regel loest bei diesen sechs nichts aus;
                // sie schliesst die Luecke, in der ein Angreifer die gestapelte
                // Antwort weglässt.
                match urteil {
                    crate::ocsp::Urteil::Widerrufen => {
                        // added 02.10.2026 23:kx: DIE FEHLERFORM. In
                        // `strom12.rs` heisst derselbe Fall
                        // `Strom12Fehler::Handshake(String)`; im 1.3-Pfad gibt
                        // es keine `Handshake`-Variante, sondern
                        // `Unerwartet(&'static str)`. GEMESSEN: der Uebersetzer
                        // meldete „no variant `Handshake` found for enum
                        // `StromFehler`". Die Meldung bleibt derselbe Wortlaut,
                        // damit beide Pfade vergleichbar sind.
                        return Err(StromFehler::Unerwartet(
                            "der Server stapelt einen WIDERRUF fuer dieses Zertifikat",
                        ));
                    }
                    crate::ocsp::Urteil::Gut => {
                        self.ocsp_stufe = crate::strom12::ocsp_stufe::GUT;
                    }
                    crate::ocsp::Urteil::Unbekannt(grund) => {
                        self.ocsp_stufe = crate::strom12::ocsp_stufe::UNBEKANNT;
                        self.ocsp_grund = grund;
                    }
                }
            }
            // added 03.10.2026 03:8x: **NACH** der Auswertung, und die Bedingung
            // ist dieselbe: verlangt das Blatt die Stapelung und ist die Stufe
            // nicht `GUT`, ist die Verbindung abzulehnen. **OHNE** Nutzlast
            // laeuft der Block gar nicht, `ocsp_stufe` bleibt `KEINE`, und die
            // Ablehnung folgt — das ist R48s gemessener Fall, jetzt mit
            // erhaltener Reihenfolge statt mit einem Zufall.
            if crate::x509::verlangt_status_request(&ganze_kette[0])
                && self.ocsp_stufe != crate::strom12::ocsp_stufe::GUT
            {
                return Err(StromFehler::Unerwartet(crate::x509::MUST_STAPLE_FEHLER));
            }
        }

        // --- 8) Das `finished` des Servers -------------------------------
        let th_fin = transkript_bis_cv.fertig();
        // Der SERVER-Schluessel prueft das `finished` des Servers.
        // added 04.10.2026, Rolle TLS. **DER FINISHED-VERGLEICH ALS SCHRITT.**
        // Erwartet: **unter 0,1 ms** (ein HMAC ueber 32 Byte). **GEMESSEN wird
        // er trotzdem**, und zwar aus einem Grund, der nicht messen ist:
        // **wenn hier 300 ms stehen, ist es kein HMAC**, sondern ein
        // **abgebrochener Lesevorgang** — der Server hat die Antwort
        // aufgeschoben und die Zeit landet in dieser Zeile.
        crate::schritt!("RECHNEN: Server-Finished pruefen", {
            let erwartet = crate::hmac::HmacSha256::berechne(&plan.finished_key_server, &th_fin);
            // Der Vergleich ist in konstanter Zeit.
            if !crate::hmac::vergleich_gleich(&erwartet, &finished.verify_data) {
                return Err(StromFehler::FinishedFalsch);
            }
        });

        // --- 9) Das `finished` des Clients ------------------------------
        // Es traegt DENSELBE Hash wie das des Servers: beide stehen auf
        // ClientHello..CertificateVerify (RFC 8446 Abschnitt 4.4.4).
        // Das eigene `finished` wird mit dem CLIENT-Schluessel und
        // ueber den Transkript BIS EINSCHLIESSLICH DAS SERVER-FINISHED
        // gebildet. Das ist ein ANDERER Transkript als der, ueber den
        // der Server sein eigenes `finished` gebildet hat, und das ist
        // der Fehler, der den Live-Handshake scheitern liess.
        //
        // Warum zwei Staende: RFC 8446 Abschnitt 4.4.4 nennt fuer
        // `verify_data` den Stand bis `CertificateVerify*` — das gilt
        // fuer die Nachricht, die der Sender gerade selbst gebaut hat.
        // Das `finished` des Clients traegt dagegen das `finished` des
        // Servers mit, weil es beides authentifiziert. Nachgewiesen in
        // RFC 8448 Abschnitt 3; `t13_client_finished.py` rechnet es
        // nach.
        let th_client_finished = transkript_bis_server_finished.fertig();
        let cf = crate::hmac::HmacSha256::berechne(&plan.finished_key_client, &th_client_finished);
        // RFC 8446 Anhang D.4: der Client schickt unmittelbar VOR
        // seinem `finished` einen leeren `change_cipher_spec`, damit
        // eine Middlebox den Handshake nicht fuer abgebrochen haelt.
        //
        // EIGENER FEHLER: ich hatte ihn NACH dem `finished`
        // eingebaut. "Immediately prior to" ist "davor". Und im
        // ersten Versuch habe ich ihn ueberhaupt nicht geschickt,
        // weil ich ihn fuer optional hielt — er ist es nach D.4
        // nicht, er ist fuer den Kompatibilitaetsmodus vorgesehen.
        // added 04.10.2026, Rolle TLS. **DER LETZTE SCHRITT**, und er ist
        // der **einzige, der den Server noch einmal belastet**: er schickt das
        // `finished` des Clients, und **der Server muss darauf antworten**
        // (NewSessionTicket, HTTP/2-SETTINGS — beides spaeter). Erwartet:
        // **unter 1 ms**, denn ein Handshake endet mit dem Senden, nicht mit
        // der Antwort des Servers.
        // **UND DER BLOCK GIBT `fertig` ZURUECK**: die Bytes gehen **nach**
        // dem Senden noch in den Transkript (`plan.aufnehmen(&fertig)`), und
        // das ist **kein** Zufallsdetail, sondern die Regel von RFC 8446
        // 4.4.4 — der Clientfinished-Hash enthaelt das eigene `finished` **nicht**,
        // die Anwendungsgeheimnisse **doch**. Ohne die Rueckgabe meldete der
        // Uebersetzer „cannot find value `fertig` in this scope" (Zeile 1890).
        let fertig = crate::schritt!("NETZ: Client-Finished senden", {
            self.sende_klartext(typen::CHANGE_CIPHER_SPEC, &[0x01])?;
            let mut fertig = Vec::with_capacity(36);
            fertig.push(nachricht::typen::FINISHED);
            fertig.extend_from_slice(&[0, 0, 32]);
            fertig.extend_from_slice(&cf);
            self.sende_verschluesselt(typen::HANDSHAKE, &fertig)?;
            fertig
        });

        // --- 10) In die Anwendungsphase ---------------------------------
        // Das `master secret` haengt am Transkript bis ClientHello ..
        // server Finished.
        //
        // ECHTER FEHLER, und der letzte am Handshake: `ClientHello` und
        // `ServerHello` standen hier ZWEIMAL im Plan. Sie waren
        // Zeilen 140 hoeher schon aufgenommen worden, unmittelbar vor
        // `handshakephase()` — was richtig ist, denn die
        // Handshake-Verkehrsgeheimnisse brauchen genau diesen Stand.
        // Hier wurden sie ein zweites Mal hinzugefuegt, und damit war
        // der Transkript fuer die Anwendungsgeheimnisse
        //
        //     CH || SH || CH || SH || EE || Cert || CV || Finished
        //
        // statt
        //
        //     CH || SH || EE || Cert || CV || Finished
        //
        // Jeder Anwendungs-Schluessel war damit aus dem falschen
        // Geheimnis abgeleitet, und die Entschluesselung des ersten
        // Records nach dem `finished` schlug fehl.
        //
        // GEMESSEN am 30.09.2026: `plan.transkript_hash()` ergab
        // `6310d60e…`, die unabhaengige Rechnung in Python aus den
        // nachgerechneten Nachrichtenbytes ergab `cfc4d064…`. Zwei
        // Zahlen, und die Differenz war genau der doppelte Anfang.
        //
        // Warum es die ganze Zeit unentdeckt blieb: `handshakephase()`
        // sah den richtigen Transkript, und `station_5a` in
        // `trace.rs` prueft `Schluesselplan` gegen die RFC-Werte mit
        // einem frisch gebauten Plan. Nur dieser eine Aufrufer
        // haeuft Nachrichten zweimal auf, und genau dieser Aufrufer
        // spricht mit einem echten Server.
        for n in nachrichten.iter() {
            plan.aufnehmen(n);
        }
        //
        // added 01.10.2026 12:5x, R4z2 Teil C. DAS TRANSKRIPT, und es
        // endet beim **Client**-`finished` — nicht beim Server-`finished`.
        //
        // GEMESSEN: mit `plan.transkript_hash()` direkt hier (also
        // CH..server Finished) blieb der Binder falsch. Die Norm nennt
        // aber ausdruecklich `ClientHello...client Finished`, und die
        // Nachricht `client Finished` steht hier **nicht** im Plan: sie
        // wurde in Schritt 9 nur **gesendet**, nicht aufgenommen.
        //
        // DER ORT DER AUFNAHME, und die Reihenfolge ist der ganze
        // Unterschied: **nach** `anwendungsphase_client()` und **nach**
        // der Schluesselentnahme. Die Anwendungsgeheimnisse haengen an
        // CH..server Finished; nimmt man die Client-`finished` vorher auf,
        // aendern sie sich (das ist der Fehler, den der Kommentar 20
        // Zeilen weiter oben beschreibt) und die erste
        // Anwendungsnachricht schlaegt fehl.
        //
        // Die Norm, woertlich (`rfc8446.txt` Zeile 5203-5205):
        //
        // RFC 8446 Abschnitt 7.2 (woertlich, `rfc8446.txt` Zeile 5203-5205):
        //     Derive-Secret(., "res master", ClientHello...client Finished)
        //         = resumption_master_secret
        //
        // ZWEI SACHEN, DIE ICH ZUERST FALSCH HATTE und die man **nur** durch
        // Nachschlagen findet, nicht durch Nachdenken:
        //  1. das Label ist `"res master"`, **nicht** `"resumption"` —
        //     „resumption" ist das Label des **zweiten** Schritts
        //     (Abschnitt 4.6.1, Zeile 4176: `HKDF-Expand-Label(rms,
        //     "resumption", ticket_nonce, Hash.length)`). Zwei aehnliche
        //     Labels, eine Zeile Abstand.
        //  2. der Kontext ist `Hash(ClientHello...client Finished)` — das
        //     **volle** Transkript, nicht `Hash("")`.
        //
        // GEMESSEN: mit `Hash("")` und Label `"resumption"` blieb der
        // Binder falsch (`openssl s_client -sess_in` -> `New`), obwohl die
        // Ticketform inzwischen normgerecht war.
        plan.anwendungsphase_client()
            .map_err(StromFehler::Schluesselplan)?;
        // added 02.10.2026 05:1x: UND HIER DIE ANWENDUNGSGEHEIMNISSE, mit
        // demselben Zufall. `CLIENT_HANDSHAKE_TRAFFIC_SECRET` ist damit zwei
        // Mal im Log (diesmal mit dem fertigen Anwendungsgeheimnis unter
        // `CLIENT_TRAFFIC_SECRET_0`) — genau so, wie Wireshark es liest: die
        // Marke entscheidet, nicht die Reihenfolge.
        if let (Some(pfad), Some(r)) = (
            crate::record::keylog_pfad(),
            crate::record::client_random_aus_hello(ch),
        ) {
            crate::record::keylog_schreiben(
                &pfad,
                &r,
                &plan.client_hs_traffic,
                &plan.server_hs_traffic,
                &plan.client_ap_traffic,
                &plan.server_ap_traffic,
            );
        }
        //
        // added 01.10.2026 12:5x, R4z2 Teil C. DAS GEHEIMNIS BLEIBT
        // STEHEN, und das ist der ganze Unterschied zu vorher: RFC 8446
        // 4.6.1 leitet die Wiederaufnahme-PSK aus dem
        // `resumption_master_secret` ab, und der liegt **nur** hier.
        // Vorher stand die PSK als Zufallswert im Ticket, deshalb
        // brauchte der Client nichts zu behalten — und deshalb war unser
        // Ticket fuer jeden fremden Client tot (GEMESSEN: `openssl
        // s_client -sess_in` sagte `New`).
        //
        // DIE FORMEL steht drei Zeilen weiter oben woertlich aus der
        // Norm: `Derive-Secret(., "res master", ClientHello...client
        // Finished)`.

        let (ks, ivs) = self
            .suite
            .keying_material(&plan.client_ap_traffic)
            .map_err(StromFehler::Schluesselplan)?;
        let (ke, ive) = self
            .suite
            .keying_material(&plan.server_ap_traffic)
            .map_err(StromFehler::Schluesselplan)?;
        self.s_schluessel = ks;
        self.s_iv = ivs;
        self.e_schluessel = ke;
        self.e_iv = ive;
        // ECHTER FEHLER: die Sequenzzahlen wurden hier NICHT
        // zurueckgesetzt, sie liefen aus der Handshake-Phase weiter.
        //
        // RFC 8446 Abschnitt 5.3: die Nonce ist `iv XOR
        // sequence_number`, und die Sequenzzahl beginnt bei JEDEM
        // Schluesselwechsel wieder bei null. "The per-record nonce ...
        // is formed by XORing the ... sequence number with the static
        // ... iv ... The sequence number is set to zero whenever a
        // connection's encryption keys are changed."
        //
        // Wie es sich geaeussert hat: das `finished` des Clients war
        // noch richtig, weil es noch mit den Handshake-Schluesseln und
        // deren Zaehlern gesendet wurde. Danach kam der HTTP-Get mit
        // einem Zaehler, der um die Zahl der Handshake-Records zu
        // hoch war, und der Server entschluesselte einen Muellblock —
        // oder gar nichts, wenn er die Laenge prueft. Die Antwort
        // kam nicht, und der Test meldete "es kam etwas zurueck".
        //
        // DER WEG HIN: die Diagnose war, einen Schritt zurueckzugehen
        // und zu fragen, was zwischen dem funktionierenden
        // `finished` und der ersten Anwendungsnachricht anders ist.
        // Es sind genau zwei Dinge — Schluessel und Zaehler — und der
        // Schluesselwechsel war die offensichtliche Stelle, an der man
        // auch den Zaehler zuruecksetzt.
        self.s_seq = 0;
        self.e_seq = 0;
        //
        // added 01.10.2026 12:5x, R4z2 Teil C. HIER, und nicht weiter
        // oben: die Client-`finished`-Nachricht (`fertig`, Schritt 9)
        // kommt jetzt in das Transkript. Erst sind die
        // Anwendungsgeheimnisse entnommen — ab jetzt kann es kein
        // Anwendungsgeheimnis mehr verfaelschen.
        //
        // M3: `MAX_HANDSHAKE_NACHRICHTEN` gilt fuer eingehende Nachrichten;
        // `fertig` ist eine ausgehende, von uns selbst gebaute und
        // bereits gesendete Nachricht. Sie zu begrenzen waere eine Zahl
        // ohne Gegenstand — sie kann nicht unbegrenzt wachsen, weil sie
        // eine feste Laenge hat.
        plan.aufnehmen(&fertig);
        self.resumption_master_secret = Some(
            crate::record::derive_secret(
                &plan.master_secret().map_err(StromFehler::Schluesselplan)?,
                b"res master",
                &plan.transkript_hash(),
            )
            .map_err(StromFehler::Schluesselplan)?,
        );
        //
        // added 01.10.2026, R4q: DAS BLATT, und bei einer Wiederaufnahme
        // gibt es keins. `zertifikate[0]` war hier ein **Panik** — und die
        // Meldung `index out of bounds: the len is 0 but the index is 0`
        // sagt nichts darueber, dass die Ursache ein **fehlendes
        // Zertifikat** ist, sondern sieht nach einem Arrayfehler aus.
        //
        // `blatt_der` bleibt dann **leer**, und das ist richtig so: es ist
        // das Blatt des zuletzt geprueften Zertifikats, und bei einer
        // Wiederaufnahme wurde keines geprueft. Ein Aufrufer, der ein Blatt
        // braucht (etwa fuer eine Anzeige), muss das an der **Anzeige**
        // merken — ein leeres `blatt_der` ist ein Fehler, den man dort
        // bemerkt, und ein erfundenes waere es nicht.
        if !wiederaufgenommen && let Some(blatt) = zertifikate.first() {
            self.blatt_der = blatt.clone();
        }
        // Die Handshake-Schluessel merken, BEVOR sie ueberschrieben
        // werden. Nur fuer die Messung: damit laesst sich offline
        // ententschluesseln, mit welchem Schluessel der Server den
        // ersten Record nach dem `finished` wirklich verschluesselt
        // hat. Genau das ist die Frage, die die Koordination gestellt
        // hat, und sie ist ohne diese beiden Zahlen nicht
        // beantwortbar.
        Ok(())
    }

    /// Liest das `ServerHello`. Es ist der einzige unverschluesselte
    /// Record, den der Client erwartet.
    /// Liest **einen** Anwendungsrecord und gibt seinen Klartext zurueck.
    ///
    /// added 02.10.2026, R4d Stufe 2. DER GRUND: `TlsStrom` las bisher nur
    /// Handshake-Nachrichten. Das reichte, solange nichts nach dem Handshake
    /// kam — und seit der Server am Ende eine `NewSessionTicket` schickt, ist
    /// das zu wenig: der Client wuesste nicht einmal, dass eines da ist.
    ///
    /// DER WEG, und er ist derselbe wie beim Schreiben auf der Serverseite
    /// (`AnwendungsStrom::schreiben`, `server.rs`): Record lesen, AEAD
    /// entschluesseln, Klartext zurueck. Die Sequenzzahl beginnt bei **null**
    /// fuer die Anwendungsphase (RFC 8446 5.5: sie wird je Phase neu
    /// gezaehlt, nicht ueber den Handshake fortgesetzt) — der
    /// Handshake-Abschnitt hat seine eigene Folge.
    ///
    /// DER RUECKGABEWERT, und die Alternative waere `None`: `None` heisst
    /// „nichts mehr da" (Ende), `Ok(vec![])` heisst „ein Record, aber kein
    /// Anwendungsinhalt" (etwa `close_notify`). Das unterscheidet ein Ende von
    /// einem leeren Ergebnis, und ein Aufrufer, der das verwechselt, laeuft in
    /// eine Schleife.
    ///
    /// M3: `max` begrenzt die Gelesenesmenge. Ein Server kann beliebig viel
    /// schicken; wer eine **eine** Nachricht liest, braucht keine Obergrenze —
    /// aber die Aufrufstelle, die in einem Rennen viele liest, schon.
    /// Liest die **Nachschrift** und legt die Tickets ab — und reicht die
    /// erste Anwendungsnachricht **durch**.
    ///
    /// added 02.10.2026, R4k. DER GRUND: `Read::read` **verwarf** die
    /// Nachschrift (siehe dort: `typen::HANDSHAKE => continue`). Das ist
    /// richtig, damit die HTTP-Antwort nicht kaputtgeht — und es heisst, dass
    /// ein Browser, der echte Seiten laedt, **kein einziges** Ticket
    /// behält. GEMESSEN 02.10.2026, 06:0x: `de.wikipedia.org` schickt **2**
    /// `NewSessionTicket`, `www.rust-lang.org` **1** — und beide werden
    /// verworfen. Ohne diese Methode ist Wiederaufnahme im Browser nicht
    /// moeglich, egal wie sie sonst gebaut ist.
    ///
    /// **DIE ANWENDUNGSNACHRICHT DARF NICHT VERLOREN GEHEN.** Der erste
    /// Record, der **keine** Nachschrift ist, wird in `self.klattr` gelegt
    /// und die Schleife beendet — `Read::read` bedient sich dann daraus, als
    /// waere nichts gewesen. Ohne das wuerde der HTTP-Leser auf den
    /// Antwort-Headern warten, waehrend die Antwort schon angekommen ist:
    /// ein Fehler, der sich wie ein haengender Server zeigt.
    ///
    /// `jetzt` sind Sekunden seit 1970, wie in `Tickets::angebot`.
    ///
    /// DER RÜCKGABEWERT ist die Zahl der **abgelegten** Tickets, nicht die
    /// Zahl der gelesenen Records: ein Server darf beliebig viele
    /// `KeyUpdate` schicken, und die sind fuer den Aufrufer nichts.
    ///
    /// M3: `max_records` begrenzt die Schleife. **GEMESSEN** am 02.10.2026
    /// waren es 1 bis 2 Nachschrift-Records je Sitzung; 8 ist die Grenze,
    /// weil ein Server auch mehr schicken darf und ein Aufrufer eine
    /// Messung will, kein Geduldstest.
    ///
    ///
    /// ### DIE FALLE, und sie ist die wichtigste Zeile in diesem Kommentar
    ///
    /// **DIESE METHODE DARF NICHT MIT DER STANDARD-FRIST GERUFEN WERDEN.**
    /// Die Schleife liest, bis ein Record kommt, der **keine** Nachschrift
    /// ist — und ein Server, der nach dem Handshake erst auf die **Anfrage**
    /// wartet, schickt diesen Record **nicht vorher**. Mit der
    /// Standard-Lesefrist (20 Sekunden) haengt der Aufrufer also **20
    /// Sekunden**, bevor er ueberhaupt eine Anfrage senden kann, und jeder
    /// Browser sieht danach wie bei einer sehr langsamen Verbindung aus.
    ///
    /// DER WEG, DER FUNKTIONIERT, und er ist der einzige: **kurze**
    /// Lesefrist setzen (ein paar hundert Millisekunden genuegen — die
    /// Nachschrift liegt im TCP-Puffer, wenn sie ueberhaupt kommt) und die
    /// Methode aufrufen. Was nicht da ist, wird beim **Lesen der Antwort**
    /// geholt, denn `Read::read` ueberspringt Nachschrift-Records
    /// (`typen::HANDSHAKE => continue`).
    ///
    /// **WAS BEIM TIMEOUT VERLOREN GEHT: nichts.** Der Record wird erst
    /// konsumiert, wenn er **vollstaendig** gelesen wurde; ein Timeout
    /// mitten im Record laesst ihn im Sockel, und die Sequenzzahl ist dann
    /// nicht weitergerueckt. Genau das behauptet
    /// `tests/nachschrift_ueber_enum.rs::zweites_lesen_holt_nichts_zweites`.
    ///
    /// EIN LESEFEHLER IST **KEIN** FEHLER: die Nachschrift ist etwas, das
    /// ein Server schickt **wenn er will** (RFC 8446 4.6.1: „MAY"), und
    /// die meisten schicken gar keins. Ein Timeout nach dem Handshake ist
    /// deshalb das normale Ende und wird als `Ok` gemeldet — sonst wuerde
    /// jede Seite ohne Ticket als Fehler enden.
    /// Die aktuelle **Lese**frist des Sockels, oder `None` (unbegrenzt).
    ///
    /// added 01.10.2026 (Uhr) / 02.10.2026 (Projektlabel), TLS R4n. DER
    /// GRUND: `set_read_timeout` gibt den **alten** Wert nicht zurueck, und
    /// ohne Getter ist „die Frist wurde zurueckgesetzt" eine **Behauptung**.
    /// Sie zu pruefen geht nur, wenn man sie lesen kann — und ein Test, der
    /// eine Behauptung prueft, ist besser als keiner.
    pub fn lesefrist(&self) -> Option<std::time::Duration> {
        self.socket.read_timeout().ok().flatten()
    }

    pub fn hole_nachschrift(
        &mut self,
        tickets: &mut crate::ticket::Tickets,
        host: &str,
        jetzt: u64,
        max_records: usize,
    ) -> Result<usize, StromFehler> {
        let mut abgelegt = 0usize;
        for _ in 0..max_records {
            let (typ, nutzlast) = match self.lese_naechsten_verschluesselten() {
                Ok(v) => v,
                // `VerbindungZu` und ein Timeout sind beide „jetzt nichts
                // mehr" — siehe die Begruendung am Kopf.
                // added 02.10.2026, R4k: EIN FEHLER **NACH** DEM
                // ABBILDUNG, WEIL ER AUCH DORT AUFTRITT: In
                // `lese_naechsten_verschluesselten` ist ein Timeout kein
                // Fehler, sondern ein `continue` — der naechste Versuch
                // wartet erneut. In einer Schleife mit einer Gesamtfrist ist
                // das ein Fehler. Beide Stellen muessen **verschieden**
                // entscheiden, und deshalb wird hier **nicht** der Text
                // gesucht, sondern die Fehlerart: `StromFehler::
                // Zeitueberschreitung` sagt, dass die **Sockel**-Frist
                // abgelaufen ist. `VerbindungZu` ist das Ende der Richtung.
                //
                // DER TEXT WIRD NICHT GESUCHT, weil eine Uebersetzung oder
                // einneue `Display`-Fassung ihn aendern wuerde — und dann
                // wuerde ein Timeout wieder wie ein Fehler aussehen und
                // jede Seite ohne Ticket als Abbruch melden.
                Err(e) => {
                    if matches!(
                        e,
                        StromFehler::Zeitueberschreitung | StromFehler::VerbindungZu
                    ) {
                        return Ok(abgelegt);
                    }
                    return Err(e);
                }
            };
            if typ != typen::HANDSHAKE {
                // DIE ERSTE ANWENDUNGSNACHRICHT: durchreichen und Schluss.
                self.klattr = nutzlast;
                return Ok(abgelegt);
            }
            // added 04.10.2026, Rolle TLS. **DER WEG, UND ER IST DER
            // ZWEITE ORT DIESER LOGIK — also der Punkt, an dem zwei Orte
            // gleich sein muessen.**
            //
            // **WARUM UEBERHAupt EIN ZWEITER WEG:** der Auftrag der Leitung
            // (NETZTEMPO_25) ist, **nach dem Handshake nicht mehr zu warten**.
            // GEMESSEN dort: jede neue TLS-1.3-Verbindung **wartet bis zu 250 ms**
            // auf ein `NewSessionTicket` — 280–410 ms je Handshake, gebucht als
            // „tls“. **Die Tickets kommen aber in der Anwendungsphase**, meist
            // **gleich mit der ersten Antwort**, und werden dort **verworfen**
            // (`read`, `typen::HANDSHAKE => continue`). **Ohne Warten** bekommen
            // wir sie also **nur**, wenn die Leseschleife sie **auswertet**.
            //
            // **UND DESHALB EINE FUNKTION, KEIN ZWEITER KOPIE:** das Ablegen
            // (PSK ableiten, `ablegen`, kaputte Tickets verwerfen) steht **hier**
            // und wird von `read` **aufgerufen**. **Zwei Abschriften sind zwei
            // Wahrheiten ueber „wie wird ein Ticket nutzbar“** — und die erste
            // hat in diesem Bereich schon einmal gezeigt, dass sie falsch sein
            // kann (R4k, Kommentar unten).
            let abgelegt_neu = self.nachschrift_auswerten(&nutzlast, tickets, host, jetzt)?;
            abgelegt += abgelegt_neu;
            if abgelegt_neu > 0 {
                continue;
            }
        }
        Ok(abgelegt)
    }

    /// added 04.10.2026, Rolle TLS. **DIE EINZIGE STELLE, AN DER EIN
    /// `NewSessionTicket` AUSWERTBAR WIRD** — und damit der Grund, warum es
    /// diese Funktion gibt und nicht einen zweiten Block in `read`.
    ///
    /// **SIE NIMMT DEN RECORD-INHALT** (die Klartextnutzlast eines
    /// `handshake`-Records) und **nicht** den Record-Typ: Aufrufer sind
    /// `hole_nachschrift` (der Record kam aus `lese_naechsten_verschluesselten`)
    /// und `read` (der Record kam aus derselben Funktion). **Ein zweiter Typ
    /// waere eine zweite Bedingung dafuer, wann ausgewertet wird** — und die
    /// ist genau die, die schon einmal falsch war.
    ///
    /// **RÜCKGABE:** die Zahl der abgelegten Tickets. **Kein Fehler bei einem
    /// kaputten Ticket** — siehe den Grund im historischen Block.
    fn nachschrift_auswerten(
        &mut self,
        nutzlast: &[u8],
        tickets: &mut crate::ticket::Tickets,
        host: &str,
        jetzt: u64,
    ) -> Result<usize, StromFehler> {
        let mut abgelegt = 0usize;
        {
            // DER PUFFER, und er ist **notwendig**: Nachschrift-Nachrichten
            // duerfen ueber die Recordgrenze zerschnitten sein, darum wird
            // gesammelt und dann zerlegt (wie in `lese_flug`).
            let mut puffer = nutzlast.to_vec();
            let teile = zerlege_nachrichten(&mut puffer)?;
            for nachricht in teile {
                // `NewSessionTicket` ist Nachrichtentyp 4 (RFC 8446 4.6.1).
                // Alles andere in der Nachschrift (`KeyUpdate`, `NewSession-
                // Ticket` in einem anderen Entwurf) wird **verworfen**, und
                // nicht als Fehler gemeldet: die Norm verbietet kein
                // Verwerfen, und ein Client, der sich hier weigert, bricht
                // ab, sobald ein Server etwas Neues schickt.
                if nachricht.first() != Some(&4) {
                    continue;
                }
                // DER INHALT OHNE KOPF, und das war ein **echter** Fehler
                // (02.10.2026, R4k): `zerlege_nachrichten` gibt Nachrichten
                // **mit** Kopf zurueck (`puffer[pos..pos + 4 + laenge]`),
                // und `new_session_ticket` will ab `ticket_lifetime` lesen —
                // also den **Inhalt**. Mit `&nachricht[1..]` bekam es den
                // Kopf mit und meldete `erwartet 11138 Byte, vorhanden 13`.
                // Die Zahl steht jetzt als `HANDSHAKE_KOPF` an **einer**
                // Stelle und nicht als `4` an fuenf.
                match crate::ticket::new_session_ticket(
                    &nachricht[crate::nachricht::HANDSHAKE_KOPF..],
                    jetzt,
                ) {
                    // added 01.10.2026 12:5x, R4z2 Teil C. DIE PSK
                    // ABLEITEN, und das ist der Unterschied zu vorher:
                    // der Decoder liest sie nicht mehr aus dem
                    // Ticketinhalt, sondern sie wird hier aus dem
                    // `resumption_master_secret` **dieser** Sitzung
                    // gerechnet (RFC 8446 4.6.1).
                    //
                    // DER WEG, und er ist nicht `unwrap`: ohne
                    // Geheimnis (TLS 1.2, oder ein Strom ohne
                    // Handshake) gibt es keine ableitbare PSK, und ein
                    // Ticket ohne PSK ist fuer uns wertlos. Solche
                    // Tickets werden **nicht** abgelegt, statt sie
                    // abzulegen und spaeter ein leeres Geheimnis
                    // anzubieten.
                    //
                    // GEMESSEN, warum das der entscheidende Schritt
                    // ist: unser Server und `openssl` bekamen dasselbe
                    // Ticket und kamen zu **verschiedenen** PSKs
                    // (`a332f48f…` gegen `7A04648D…`), deshalb kein
                    // `Reused`. Siehe `PROBLEME.txt` Nr. 10.
                    Ok(t) => {
                        //
                        // added 01.10.2026 12:5x: `continue` statt `?`.
                        // Der Grund steht 20 Zeilen weiter unten und gilt
                        // hier genauso: EIN KAPUTTES TICKET IST KEIN
                        // FEHLER. Der Client laedt keine Seite weniger,
                        // weil er ein fremdes Ticket verwirft.
                        let t = match self.resumption_master_secret {
                            Some(rms) => match t.mit_psk(&rms) {
                                Ok(t) => t,
                                Err(_) => continue,
                            },
                            None => t,
                        };
                        if !t.hat_psk() {
                            continue;
                        }
                        tickets.ablegen(host, t);
                        abgelegt += 1;
                        // added 04.10.2026: **DER ZAEHLER FUER DIE TESTFRAGE**
                        // „wird der neue Weg ueberhaupt betreten?“. Er steht
                        // **hier**, bei der einzigen Stelle, an der ein Ticket
                        // wirklich nutzbar wird — nicht im `read`-Zweig, der
                        // auch ohne Ticket durchlaufen kann.
                        self.nachschrift_ausgewertet += 1;
                        // added 04.10.2026: **DER ZAEHLER FUER DIE TESTFRAGE**
                        // „wird der neue Weg ueberhaupt betreten?“. Er steht
                        // **hier**, bei der einzigen Stelle, an der ein Ticket
                        // wirklich nutzbar wird — nicht im `read`-Zweig, der
                        // auch ohne Ticket durchlaufen kann.
                        self.nachschrift_ausgewertet += 1;
                    }
                    // EIN KAPUTTES TICKEL IST KEIN FEHLER: es ist ein
                    // Datensatz eines fremden Servers, und ein Browser, der
                    // daran scheitert, laedt keine Seite mehr.
                    Err(fehler) => {
                        // added 02.10.2026, R4k: DER GRUND, WARUM HIER
                        // `eprintln!` STEHT UND NICHT `continue` OHNE
                        // WORT: `Err(_) => continue` hat mir in diesem
                        // Durchlauf den **ganzen** Fehler versteckt — der
                        // Test meldete „der Server hat kein Ticket
                        // geschickt", und tatsaechlich war eines da und
                        // nur die Auswertung war kaputt. Ein fremder Server
                        // mit einem kaputten Ticket ist selten, aber wenn es
                        // passiert, ist es der einzige Hinweis, und es ist
                        // einer pro Sitzung, nicht einer pro Record.
                        eprintln!("ein NewSessionTicket ist nicht lesbar: {fehler}");
                        continue;
                    }
                }
            }
        }
        Ok(abgelegt)
    }

    pub fn lies_anwendungsrecord(&mut self) -> Result<Vec<u8>, StromFehler> {
        // M6: Frist, wenn der Aufrufer keine gesetzt hat. Ohne sie blockiert
        // das Lesen, und der Aufrufer haengt.
        if self.socket.read_timeout().ok().flatten().is_none() {
            self.socket
                .set_read_timeout(Some(LESEZEIT))
                .map_err(|e| StromFehler::Netz(e.to_string()))?;
        }
        // DER ROHBLOCK, und er wird von Hand geschnitten: der Record ist
        // `typ (1) || laenge (2) || chiffrat`. `MAX_CIPHERTEXT` ist die Grenze
        // aus `record.rs`; ein groesserer Wert auf der Leitung ist ein
        // Fehler, keine grosse Nachricht.
        let mut kopf = [0u8; 5];
        self.socket
            .read_exact(&mut kopf)
            .map_err(|e| StromFehler::Netz(e.to_string()))?;
        let laenge = u16::from_be_bytes([kopf[3], kopf[4]]) as usize;
        if laenge == 0 || laenge > crate::record::MAX_CIPHERTEXT {
            return Err(StromFehler::ZuViel("Record laenger als er sein darf"));
        }
        let mut body = vec![0u8; laenge];
        self.socket
            .read_exact(&mut body)
            .map_err(|e| StromFehler::Netz(e.to_string()))?;
        // DER AEUSSERE TYP: 23 (`application_data`) und 24 (`AEAD_CHIFFERTEXT`)
        // sind dasselbe bei TLS 1.3 — der Klartexttyp steht im AEAD-Block.
        // 21 (`alert`) wird durchgereicht, damit `close_notify` erkennbar
        // bleibt.
        match kopf[0] {
            20 => {
                return Err(StromFehler::Unerwartet(
                    "Handshake-Record nach dem Handshake",
                ));
            }
            22 => {
                return Err(StromFehler::Unerwartet(
                    "ChangeCipherSpec nach dem Handshake",
                ));
            }
            21 => {
                // DER INNERE TYP ist hier `alert`, nicht `application_data`:
                // ein `close_notify` traegt im Klartext den Alert-Typ, und der
                // stimmt nicht mit dem aeusseren ueberein. Die erste Fassung
                // verlangte hier `application_data` und wuerde eine
                // `close_notify` als falschen Inhaltstyp melden.
                let (klartext, _) =
                    self.entschluessele_anwendung(self.ap_lese_seq, Some(typen::ALERT), &body)?;
                self.ap_lese_seq += 1;
                return Ok(klartext);
            }
            23 | 24 => {}
            _ => {
                return Err(StromFehler::Unerwartet(
                    "unbekannter Record-Typ nach dem Handshake",
                ));
            }
        }
        //
        // DER SCHLUESSEL, und das ist die Stelle, an der es scheitern kann:
        // der Client **liest** mit dem Server-zu-Client-Anwendungsgeheimnis,
        // und in `handshake_ab_serverhello` steht es in `e_schluessel`
        // (`ke`), das Schreibgeheimnis in `s_schluessel` (`ks`).
        //
        // DER WEG, und er ist bewusst **nicht** `record::
        // schluessel_unprotect`: das expandiert aus dem Argument noch einmal
        // den Schluessel und erwartet deshalb das Geheimnis, nicht den
        // Schluessel. `entschluessele_anwendung` ist das Spiegelbild von
        // `sende_verschluesselt` und benutzt `e_schluessel`/`e_iv` direkt.
        let (klartext, _) = self.entschluessele_anwendung(self.ap_lese_seq, None, &body)?;
        self.ap_lese_seq += 1;
        Ok(klartext)
    }

    /// Prueft die `ServerHello` gegen das, was **wir** angeboten haben.
    ///
    /// added 01.10.2026 06:2x UTC (Projektlabel 02.10.2026), R4s. DER
    /// REGELBRUCH, und es ist ein **Verbot** der Norm: RFC 8446 Abschnitt
    /// 4.2.11 — "A server MUST NOT send a \"pre_shared_key\" extension in
    /// the ServerHello if the client did not send a \"pre_shared_key\"
    /// extension."
    ///
    /// WARUM EINE EIGENE METHODE UND NICHT DREI `if`-ZEILEN: es gibt
    /// **drei** Clientpfade (`handshake`, `hallo_tauschen_mit_psk`,
    /// `hallo_tauschen_mit`), und alle drei dekodieren ein ServerHello. Die
    /// erste Fassung stand **inline** in einem davon — und der Test, der
    /// genau diesen Fall baut, meldete `X25519` statt des Regelbruchs,
    /// weil `TlsVerbindung::ueber_sockel` ueber `hallo_tauschen_mit` geht.
    /// Das ist **genau** die Falle aus R4k (drei Pfade, eine Behauptung), und
    /// sie hatte sich schon wieder gestellt: eine Regel in einem Pfad ist
    /// eine Regel, an der niemand sie angewendet sieht.
    ///
    /// DER FEHLERTYP: `StromFehler` hat kein `illegal_parameter`, und eine
    /// neue Variante waere fuer **einen** Fall eine Aufwaendung. Der Text
    /// nennt den Regelbruch und die Normstelle, damit er beim Lesen
    /// erkennbar ist.
    fn pskserverhello_pruefen(&self, sh: &ServerHello) -> Result<(), StromFehler> {
        if sh.selected_identity.is_some() && self.psk_angebot.is_none() {
            return Err(StromFehler::Unerwartet(
                "der Server hat eine PSK-Identitaet gewaehlt, obwohl wir keine \
                 angeboten haben (RFC 8446 4.2.11)",
            ));
        }
        Ok(())
    }

    /// Wie `lies_anwendungsrecord`, **mit** dem inneren Inhaltstyp.
    ///
    /// added 02.10.2026, R4j. DER GRUND FUER DIE ZWEITE METHODE: nach dem
    /// Handshake kommen in **einem** Strom zwei Sorten — Anwendungsdaten
    /// (HTTP-Antwort, Innenstyp 23) und `NewSessionTicket` (Innenstyp 22),
    /// in beliebiger Reihenfolge. Wer sie unterscheiden muss, braucht den
    /// Typ; wer nur den naechsten Record liest, nicht.
    ///
    /// GEMESSEN, warum das kein Luxus ist: `openssl s_client -msg` sieht von
    /// `www.rust-lang.org` **1** `NewSessionTicket`, und `example.com` und
    /// `de.wikipedia.org` **0** (02.10.2026, 05:5x). Wer nur einen Host
    /// mitmisst, misst die Server und nicht den Client.
    ///
    /// DIE ZWEI METHODEN STATT EINER MIT PARAMETER: ein Aufrufer, der den
    /// Typ nicht braucht, soll ihn nicht erraten muessen, und ein
    /// Aufrufer, der ihn braucht, soll ihn nicht wegwerfen. Ein
    /// `Option<u8>`-Parameter an der oeffentlichen Methode wuerde beides
    /// erzwingen und den Aufrufer zum Wegwerfen verpflichten.
    pub fn lies_anwendungsrecord_mit_typ(&mut self) -> Result<(Vec<u8>, u8), StromFehler> {
        let mut kopf = [0u8; 5];
        self.socket
            .read_exact(&mut kopf)
            .map_err(|e| StromFehler::Netz(e.to_string()))?;
        let laenge = ((kopf[3] as usize) << 8) | kopf[4] as usize;
        if laenge == 0 || laenge > crate::record::MAX_CIPHERTEXT {
            return Err(StromFehler::ZuViel("Record laenger als er sein darf"));
        }
        let mut body = vec![0u8; laenge];
        self.socket
            .read_exact(&mut body)
            .map_err(|e| StromFehler::Netz(e.to_string()))?;
        let seq = self.ap_lese_seq;
        let (klartext, innerer_typ) = self.entschluessele_anwendung(seq, None, &body)?;
        self.ap_lese_seq += 1;
        Ok((klartext, innerer_typ))
    }

    /// Entschluesselt einen Anwendungsrecord — das **Spiegelbild** von
    /// `sende_verschluesselt`.
    ///
    /// added 02.10.2026, R4d Stufe 2. DER FEHLER, den dieser Aufruf
    /// behebt, war ein **doppeltes Expandieren**: `lies_anwendungsrecord`
    /// ging ueber `record::schluessel_unprotect`, und das erwartet — so wie
    /// `record::schluessel_protect` und `AnwendungsStrom` in `server.rs` —
    /// das **Verkehrsgeheimnis**. `TlsStrom` haelt aber bereits den daraus
    /// expandierten Schluessel (`e_schluessel`) und die dazugehoerige IV. Es
    /// wurde also ein Schluessel expandiert, der selbst schon das Ergebnis
    /// einer Expandierung war.
    ///
    /// GEMESSEN, warum das so lange unentdeckt blieb (02.10.2026, 23:4x,
    /// im selben Lauf beide Seiten ausgegeben): **alle** Groessen stimmten.
    /// Suite `1301`, Sequenzzahl `0`, Schluessel `49 34 21 b4`, IV
    /// `26 d3 80 f1` und das Geheimnis `s ap traffic` `84 4c d5 16` waren auf
    /// beiden Seiten **byteweise gleich** — und die Bytes auf der Leitung
    /// ebenfalls (`170303004a…`, Hexvergleich). Die Authentifizigung schlug
    /// trotzdem fehl, weil der **Schluessel** im Spiel war: genau der Kandidat,
    /// den die Meldung am wenigsten verraten hat. `Record(Authentifizierung)`
    /// heisst nur, dass einer der vier aus `sende_verschluesselt` stimmt nicht
    /// — Schluessel, Nonce, AAD, Sequenzzahl.
    ///
    /// `typ` wird wie beim Schreiben **nicht** in den AAD geschrieben (RFC
    /// 8446 5.2: der AAD ist der aeussere Kopf, der innere Typ steht im
    /// letzten Klartextbyte), sondern nur als Sollwert geprueft: ein
    /// abweichender Typ ist ein Protokollfehler und kein Kryptographiefehler,
    /// und die Meldung muss ihn unterscheiden koennen.
    fn entschluessele_anwendung(
        &self,
        seq: u64,
        erwartet: Option<u8>,
        chiffrat: &[u8],
    ) -> Result<(Vec<u8>, u8), StromFehler> {
        // M3: Obergrenze und Mindestlaenge vor der Entschluesselung. Ohne die
        // untere Grenze ist ein 3-Byte-Block kein Fehler, sondern eine
        // Panik in der Slice-Teilung weiter unten.
        if chiffrat.len() > crate::record::MAX_CIPHERTEXT {
            return Err(StromFehler::ZuViel("Ciphertext laenger als er sein darf"));
        }
        if chiffrat.len() < 17 {
            return Err(StromFehler::ZuViel("Ciphertext kuerzer als Tag und Typ"));
        }
        let n = nonce(&self.e_iv, seq);
        // Derselbe AAD wie beim Schreiben, Zeile fuer Zeile: `typen::
        // AEAD_CHIFFERTEXT`, die beiden Versionsbytes aus derselben Quelle
        // (`record::VERSION`) und die **Ciphertext**-Laenge, nicht die des
        // Klartexts.
        let mut aad = [0u8; 5];
        aad[0] = typen::AEAD_CHIFFERTEXT;
        aad[1] = crate::record::VERSION[0];
        aad[2] = crate::record::VERSION[1];
        let laenge = chiffrat.len() as u16;
        aad[3] = (laenge >> 8) as u8;
        aad[4] = laenge as u8;
        let mut innen = match self.suite {
            Suite::Aes128GcmSha256 => {
                // Die letzten 16 Byte sind der Tag (RFC 8446 5.2); der Rest
                // ist das Chiffrat. Beides wird **vor** der Entschluesselung
                // getrennt, damit der Tag geprueft wird, bevor Klartext
                // entsteht (M4).
                let n_body = chiffrat.len() - 16;
                let (koerper, tag) = chiffrat.split_at(n_body);
                let mut tag16 = [0u8; 16];
                tag16.copy_from_slice(tag);
                aes::gcm_dechiffre(
                    &self.e_schluessel[..self.suite.schluessel_laenge()],
                    &n,
                    &aad,
                    koerper,
                    &tag16,
                )
                .map_err(|_| StromFehler::AEAD {
                    suite: self.suite_name(),
                    richtung: "lesen",
                })?
            }
            Suite::ChaCha20Poly1305Sha256 => {
                chacha::aead_dechiffre(&self.e_schluessel, &n, &aad, chiffrat).map_err(|_| {
                    StromFehler::AEAD {
                        suite: self.suite_name(),
                        richtung: "lesen",
                    }
                })?
            }
        };
        // Das LETZTE Klartextbyte ist der innere Typ.
        let innerer_typ = match innen.pop() {
            Some(b) => b,
            None => return Err(StromFehler::Unerwartet("Record ohne inneren Typ")),
        };
        // DER INNERE TYP wird **nur** geprueft, wenn der Aufrufer einen
        // nennt. added 02.10.2026, R4j: vorher war die Pruefung
        // **zwingend**, und das war ein Fehler von mir — `lies_anwendungs
        // record` verlangte damit `handshake` von **jedem** Record nach
        // dem Handshake. Ein echter Server schickt aber Anwendungsdaten
        // (HTTP-Antwort) und dazwischen oder danach `NewSessionTicket`
        // (Innenstyp `handshake`), in **einem** Strom. Wer nur einen der
        // beiden Typen akzeptiert, kann den anderen nicht lesen — und der
        // Aufrufer, der es nicht besser weiss, meldet dafuer
        // `innerer Inhaltstyp passt nicht zum erwarteten`.
        //
        // DER GRUND FUER DAS BEIBEHALTEN, wenn ein Aufrufer einen Typ
        // nennt: ein abweichender Typ ist ein **Protokollfehler** und kein
        // Kryptographiefehler, und die Meldung muss die beiden unterscheiden
        // koennen.
        if let Some(want) = erwartet
            && innerer_typ != want
        {
            return Err(StromFehler::Unerwartet(
                "innerer Inhaltstyp passt nicht zum erwarteten",
            ));
        }
        Ok((innen, innerer_typ))
    }

    // ------------------------------------------------------------------------
    // DIE RICHTUNG DER ANWENDUNGSSCHLUESSEL, an EINER Stelle
    //
    // added 02.10.2026, R4d Stufe 2. Diese Doku stand vorher an
    // `schluessel_fuer_lesen`, und sie war **in sich widerspruechlich**:
    // die Tabelle behauptete `s_schluessel` = `s ap traffic` und
    // `e_schluessel` = `c ap traffic`, fuenfzehn Zeilen darunter stand der
    // gemessene Gegenbeweis (`e[..4] = 02 21 14 5b = s ap traffic`). Ich habe
    // der Tabelle geglaubt und `s_schluessel` als Leseschluessel genommen; die
    // Meldung war `die Authentifizierung schlug fehl` — eine Meldung, die nach
    // Kryptographie klingt und ein Vertauschen zweier Feldnamen ist. Die
    // Methode ist mit dem Fix weggefallen, ihr Schluessel steht jetzt in
    // `entschluessele_anwendung`, und die Tabelle steht hier, **einmal**:
    //
    // | Feld           | aus            | wofuer                    |
    // |----------------|----------------|---------------------------|
    // | `s_schluessel` | `c ap traffic` | Client schreibt           |
    // | `e_schluessel` | `s ap traffic` | Client **liest**          |
    //
    // `s` heisst also **schreiben**, `e` **empfangen**, nicht
    // „Server-zu-Client". Auf der **Server**-Seite ist es umgekehrt, und
    // `AnwendungsStrom` hat darum zwei Felder (`geheimnis`,
    // `lese_geheimnis`). GEMESSEN am 02.10.2026, Server und Client im selben
    // Lauf: Server schreibt mit `keying_material(s_ap_traffic)[0][..4] =
    // 49 34 21 b4`, und der Client liest mit `e_schluessel[..4] = 49 34 21 b4`
    // — dieselben vier Byte.
    //
    fn lese_hello(&mut self) -> Result<Vec<u8>, StromFehler> {
        // M3: hoechstens `MAX_FLUG_RECORDS` Records, denn ein Server
        // darf ein `change_cipher_spec` dazwischenschieben.
        for _ in 0..MAX_FLUG_RECORDS {
            let (typ, inhalt) = self.lese_record_klartext()?;
            match typ {
                typen::CHANGE_CIPHER_SPEC => continue,
                typen::HANDSHAKE => {
                    // M4: die Nachricht muss in den gepufferten Bereich
                    // passen.
                    if inhalt.len() > crate::record::MAX_NACHRICHT {
                        return Err(StromFehler::ZuViel("ServerHello zu lang"));
                    }
                    return Ok(inhalt);
                }
                _ => return Err(StromFehler::Unerwartet("ServerHello erwartet")),
            }
        }
        Err(StromFehler::ZuViel("kein ServerHello"))
    }

    /// Liest den verschluesselten Flug, bis ein `finished` fertig ist.
    fn lese_flug(&mut self) -> Result<Vec<Vec<u8>>, StromFehler> {
        let mut nachrichten: Vec<Vec<u8>> = Vec::new();
        let mut gesamt = 0usize;
        let mut puffer: Vec<u8> = Vec::new();
        // M3: die Schleife laeuft hoechstens `MAX_FLUG_RECORDS` Mal.
        for _ in 0..MAX_FLUG_RECORDS {
            let (typ, nutzlast) = self.lese_naechsten_verschluesselten()?;
            if typ != typen::HANDSHAKE {
                return Err(StromFehler::Unerwartet("Handshake-Record erwartet"));
            }
            gesamt += nutzlast.len();
            // M4: die Grenze wird nach dem Lesen geprueft, aber vor
            // der naechsten Allokation.
            if gesamt > MAX_FLUG {
                return Err(StromFehler::ZuViel("Handshake-Flug zu gross"));
            }
            puffer.extend_from_slice(&nutzlast);
            // Nachrichten koennen an der Recordgrenze zerschnitten
            // sein, darum wird gesammelt und dann zerlegt.
            let teile = zerlege_nachrichten(&mut puffer)?;
            for t in teile {
                if nachrichten.len() >= MAX_FLUG_RECORDS * 4 {
                    return Err(StromFehler::ZuViel("zu viele Nachrichten"));
                }
                let ist_finished = !t.is_empty() && t[0] == nachricht::typen::FINISHED;
                nachrichten.push(t);
                if ist_finished {
                    return Ok(nachrichten);
                }
            }
        }
        Err(StromFehler::ZuViel("Flug ohne finished"))
    }

    /// Liest genau einen Record im Klartext.
    ///
    /// EIGENER FEHLER, und der hat den Handshake gegen example.com
    /// scheitern lassen: `self.roh` wurde am Anfang NICHT geleert.
    /// `lese_genau` haengt an das, was schon drin ist, also las der
    /// zweite Aufruf die fuenf Byte an Position `5 + Laenge` des
    /// ersten Records und las dann `self.roh[0]` — das war der Typ des
    /// ERSTEN Records. Jeder zweite Record war damit eine Kopie des
    /// ersten, und der Handshake brach ab, sobald der Server
    /// verschluesselt hat.
    ///
    /// Gemeldet hat sich das als "ein Record vom Typ 22 erwartet
    /// nicht" — also als ob der Server kein TLS 1.3 spräche. Der
    /// Server tat es; diese Datei las zweimal dieselben fuenf Byte.
    fn lese_record_klartext(&mut self) -> Result<(u8, Vec<u8>), StromFehler> {
        // Ohne dieses Leeren ist der zweite Aufruf kein zweiter Record.
        self.roh.clear();
        self.lese_genau(5)?;
        if self.roh.len() < 5 {
            return Err(StromFehler::VerbindungZu);
        }
        let typ = self.roh[0];
        let laenge = u16::from_be_bytes([self.roh[3], self.roh[4]]) as usize;
        // M4: vor der Allokation. `MAX_CIPHERTEXT` ist kleiner als
        // 0xFFFF, eine Ankündigung von 0xFFFF ist also ein Fehler.
        if laenge > crate::record::MAX_CIPHERTEXT {
            return Err(StromFehler::ZuViel("Record zu lang"));
        }
        self.lese_genau(laenge)?;
        Ok((typ, self.roh[5..5 + laenge].to_vec()))
    }

    /// Liest den naechsten verschluesselten Record, ueberspringt
    /// `change_cipher_spec`.
    ///
    /// ECHTER BEFUND am 30.09.2026 gegen example.com: der Server
    /// schickt direkt nach dem `ServerHello` einen
    /// `change_cipher_spec` (Record-Typ 20, ein Byte Inhalt). Ohne
    /// diese Uebersprungstelle bricht der Handshake dort ab.
    ///
    /// DER NORMGRUND: RFC 8446 Anhang D.4 ("Middlebox
    /// Compatibility Mode") verlangt vom Client ausdruecklich, einen
    /// unverschluesselten `change_cipher_spec` zu akzeptieren und zu
    /// ignorieren. Er traegt keine Information und ist in TLS 1.3
    /// ueberflüssig, aber er zu erwarten ist Pflicht.
    ///
    /// M3: die Schleife ist gegen `MAX_FLUG_RECORDS` begrenzt. Ohne
    /// diese Grenze koennte ein Server unendlich viele
    /// `change_cipher_spec` schicken, und der Client haette eine
    /// Endlosschleife ohne Speicherverbrauch — die billigste Art, sich
    /// aufzuhaengen.
    fn lese_naechsten_verschluesselten(&mut self) -> Result<(u8, Vec<u8>), StromFehler> {
        for _ in 0..MAX_FLUG_RECORDS {
            let (aussen, inhalt) = self.lese_record_klartext()?;
            match aussen {
                typen::CHANGE_CIPHER_SPEC => continue,
                // 23 ist die Schlussfassung, 25 der Wert aus
                // draft-29. Beide werden gelesen; siehe
                // `typen::AEAD_CHIFFERTEXT_ENTWURF`.
                typen::AEAD_CHIFFERTEXT | typen::AEAD_CHIFFERTEXT_ENTWURF => {
                    return self.entschluessele(aussen, inhalt);
                }
                _ => {
                    return Err(StromFehler::FalscherRecord {
                        erwartet: "ein verschluesselter Record",
                        bekommen: aussen,
                    });
                }
            }
        }
        Err(StromFehler::ZuViel("nur change_cipher_spec"))
    }

    /// Entschluesselt einen Record mit dem aktuellen Leseschluessel.
    fn entschluessele(
        &mut self,
        aussen: u8,
        inhalt: Vec<u8>,
    ) -> Result<(u8, Vec<u8>), StromFehler> {
        let _ = aussen;
        // M3: nach 2^64 Records pro Schluessel muss neu verhandelt
        // werden (RFC 8446 Abschnitt 5.5). Abgelehnt, nicht ignoriert.
        if self.e_seq == u64::MAX {
            return Err(StromFehler::ZuViel("Sequenzzahl erschoepft"));
        }
        let seq = self.e_seq;
        self.e_seq += 1;
        let n = nonce(&self.e_iv, seq);
        // Das AAD ist der Recordkopf (RFC 8446 Abschnitt 5.2): Typ,
        // `legacy_record_version` und Laenge — die Laenge des CHIFFRATS,
        // also `Klartext + 16`.
        //
        // ECHTER FEHLER: `aad` war `[23, 0, 0, hi, lo]`. Die beiden
        // Versionsbytes muessen aber `03 03` sein, weil der Record so
        // auch wirklich auf der Leitung steht (`17 03 03 …`). Mit
        // Nullen darin war das AAD falsch, und AES-GCM rechnet es in
        // die Authentifizierung ein — die Entschluesselung schlug also
        // an einer Stelle fehl, die drei Zeilen unter der Zeile lag,
        // an der ich gezweifelt habe.
        //
        // GEFUNDEN, ohne die Kryptografie zu verdächtigen: der
        // `record.rs`-Bauter `baue_record` schreibt an Position 1 und
        // 2 `0x03 0x03`. Dieselben zwei Byte müssen ins AAD. Ein
        // zweiter Ort für dieselbe Zahl ist immer auch eine zweite
        // Stelle, an der sie falsch sein kann — also beide mit
        // `record::VERSION` aus einer Quelle.
        let mut aad = [0u8; 5];
        aad[0] = typen::AEAD_CHIFFERTEXT;
        aad[1] = crate::record::VERSION[0];
        aad[2] = crate::record::VERSION[1];
        let laenge = inhalt.len();
        aad[3] = (laenge >> 8) as u8;
        aad[4] = laenge as u8;
        let mut klartext = match self.suite {
            Suite::Aes128GcmSha256 => {
                if inhalt.len() < 16 {
                    return Err(StromFehler::AEAD {
                        suite: self.suite_name(),
                        richtung: "lesen",
                    });
                }
                let (c, tag) = inhalt.split_at(inhalt.len() - 16);
                let mut t = [0u8; 16];
                t.copy_from_slice(tag);
                aes::gcm_dechiffre(
                    &self.e_schluessel[..self.suite.schluessel_laenge()],
                    &n,
                    &aad,
                    c,
                    &t,
                )
                .map_err(|_| StromFehler::AEAD {
                    suite: self.suite_name(),
                    richtung: "lesen",
                })?
            }
            Suite::ChaCha20Poly1305Sha256 => {
                chacha::aead_dechiffre(&self.e_schluessel, &n, &aad, &inhalt).map_err(|_| {
                    StromFehler::AEAD {
                        suite: self.suite_name(),
                        richtung: "lesen",
                    }
                })?
            }
        };
        // Der letzte von null verschiedene Byte ist der innere
        // Inhaltstyp; danach ist Polster (RFC 8446 Abschnitt 5.2).
        let mut i = klartext.len();
        while i > 0 {
            i -= 1;
            if klartext[i] != 0 {
                let inner = klartext[i];
                klartext.truncate(i);
                return Ok((inner, klartext));
            }
        }
        Err(StromFehler::Unerwartet("kein Inhaltstyp im Record"))
    }

    /// Liest genau `n` Byte in `self.roh`. `n` ist bereits begrenzt.
    fn lese_genau(&mut self, n: usize) -> Result<(), StromFehler> {
        // M4: die Zielgroesse wird geprueft, BEVOR der Puffer waechst.
        if self.roh.len() + n > MAX_ANTWORT {
            return Err(StromFehler::ZuViel("Antwort zu lang"));
        }
        let von = self.roh.len();
        self.roh.resize(von + n, 0);
        #[allow(unreachable_code)]
        self.socket.read_exact(&mut self.roh[von..]).map_err(|e| {
            if e.kind() == std::io::ErrorKind::TimedOut
                || e.kind() == std::io::ErrorKind::WouldBlock
            {
                StromFehler::Zeitueberschreitung
            } else if e.kind() == std::io::ErrorKind::UnexpectedEof {
                StromFehler::VerbindungZu
            } else {
                StromFehler::Netz(e.to_string())
            }
        })?;
        Ok(())
    }

    /// Sendet einen Record im Klartext.
    fn sende_klartext(&mut self, typ: u8, inhalt: &[u8]) -> Result<(), StromFehler> {
        let r = baue_record(typ, inhalt)?;
        self.socket
            .write_all(&r)
            .map_err(|e| StromFehler::Netz(e.to_string()))?;
        self.socket
            .flush()
            .map_err(|e| StromFehler::Netz(e.to_string()))?;
        Ok(())
    }

    /// Sendet einen Record verschluesselt.
    fn sende_verschluesselt(&mut self, typ: u8, inhalt: &[u8]) -> Result<(), StromFehler> {
        // Der Klartext ist `TLSInnerPlaintext`: die Nutzlast plus der
        // innere Inhaltstyp (RFC 8446 Abschnitt 5.2).
        let mut innen = Vec::with_capacity(inhalt.len() + 1);
        innen.extend_from_slice(inhalt);
        innen.push(typ);
        // M4: plus die 16 Byte der Authentifizierung.
        if innen.len() + 16 > crate::record::MAX_CIPHERTEXT {
            return Err(StromFehler::ZuViel("Klartext zu lang"));
        }
        if self.s_seq == u64::MAX {
            return Err(StromFehler::ZuViel("Sequenzzahl erschoepft"));
        }
        let seq = self.s_seq;
        self.s_seq += 1;
        let n = nonce(&self.s_iv, seq);
        // Die beiden Versionsbytes aus `record::VERSION`, derselben
        // Quelle, aus der `baue_record` den Kopf schreibt. Standen
        // hier Nullen, waere das AAD ein anderes als der Kopf, den
        // der Server mitgerechnet hat — und genau daran scheiterte der
        // erste verschluesselte Record.
        let mut aad = [0u8; 5];
        aad[0] = typen::AEAD_CHIFFERTEXT;
        aad[1] = crate::record::VERSION[0];
        aad[2] = crate::record::VERSION[1];
        // Die beiden Versionsbytes, aus derselben Quelle wie im Leser.
        // Siehe dort die Begruendung: ohne sie war das AAD falsch und
        // damit die ganze Authentifizierung.
        aad[1] = crate::record::VERSION[0];
        aad[2] = crate::record::VERSION[1];
        // Die Laenge im AAD ist die des CHIFFRATS, also `innen + 16`.
        // Sie wird VOR der Verschluesselung gesetzt, weil GCM und
        // Poly1305 das AAD in die Berechnung des Tags einbeziehen —
        // nachher waere es zu spaet.
        let laenge = innen.len() + 16;
        aad[3] = (laenge >> 8) as u8;
        aad[4] = laenge as u8;
        let c = match self.suite {
            Suite::Aes128GcmSha256 => {
                let (mut c, tag) = aes::gcm_chiffre(
                    &self.s_schluessel[..self.suite.schluessel_laenge()],
                    &n,
                    &aad,
                    &innen,
                )
                .map_err(|_| StromFehler::AEAD {
                    suite: self.suite_name(),
                    richtung: "lesen",
                })?;
                c.extend_from_slice(&tag);
                c
            }
            Suite::ChaCha20Poly1305Sha256 => {
                chacha::aead_chiffre(&self.s_schluessel, &n, &aad, &innen).map_err(|_| {
                    StromFehler::AEAD {
                        suite: self.suite_name(),
                        richtung: "lesen",
                    }
                })?
            }
        };
        let r = baue_record(typen::AEAD_CHIFFERTEXT, &c)?;
        self.socket
            .write_all(&r)
            .map_err(|e| StromFehler::Netz(e.to_string()))?;
        self.socket
            .flush()
            .map_err(|e| StromFehler::Netz(e.to_string()))?;
        Ok(())
    }

    /// Der Name der Suite, fuer die Fehlermeldung.
    fn suite_name(&self) -> &'static str {
        match self.suite {
            Suite::Aes128GcmSha256 => "TLS_AES_128_GCM_SHA256",
            Suite::ChaCha20Poly1305Sha256 => "TLS_CHACHA20_POLY1305_SHA256",
        }
    }

    // ------------------------------------------------------------------
    // Zugriff auf den Sockel unter dem TLS.
    //
    // added 30.09.2026 fuer `narsil-net`: dort ist die Verbindung ein
    // `Box<dyn Leitung>`, und dieses Trait verlangt `set_nodelay`,
    // `set_read_timeout`, `set_write_timeout`, `local_addr` und
    // `peer_addr`. Ohne diese fuenf Zeilen kann ein TLS-Strom nicht
    // hinter der HTTP-Logik liegen, und die Logik soll von TLS nichts
    // wissen.
    //
    // Die Zeitlimits sind die ECHTEN Grenzen: `TlsStrom` liest und
    // schreibt ueber denselben Sockel, also greifen die Grenzen an
    // beiden Enden. Sie werden VOR dem Handshake gesetzt und danach
    // noch einmal, weil `verbinde` eigene setzt.
    //
    // M6/M3: `set_nodelay(false)` ist erlaubt, es schaltet nur ein
    // Socket-Optimum ab. Sonst nichts.
    pub fn set_nodelay(&mut self, wert: bool) -> std::io::Result<()> {
        self.socket.set_nodelay(wert)
    }

    pub fn set_read_timeout(&mut self, dauer: Option<std::time::Duration>) -> std::io::Result<()> {
        self.socket.set_read_timeout(dauer)
    }

    pub fn set_write_timeout(&mut self, dauer: Option<std::time::Duration>) -> std::io::Result<()> {
        self.socket.set_write_timeout(dauer)
    }

    pub fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        self.socket.local_addr()
    }

    pub fn peer_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        self.socket.peer_addr()
    }

    // // added 04.10.2026 01:4x, R67. **`roh_nachschrift` GELOESCHT** — GEMESSEN:
    // // **ein** Treffer in allen Crates, die Definition. Erwaehnt wird der Name nur
    // // in einem **Kommentar** (`tests/ticket_live.rs:106`), und genau dort steht
    // // auch, dass es nicht gebraucht wird — der Kommentar ist der Grabstein des
    // // Funktionskoerpers.
    // //
    // // added 04.10.2026 01:4x: **WAS DAS KOSTET, ehrlich:** es war der einzige Weg,
    // // rohe Records **mit** Frist zu lesen (26 Zeilen, eigener Lesezeit-Setzer).
    // // Fuer eine kuenftige Diagnose, die den Wire-Verlauf eines Handshakes zeigt,
    // // ist das wieder noetig — dann mit **Aufrufer** und **Test**, nicht als
    // // public-Schatten mit leerem Koerper.

    pub fn beende(&mut self, wie: std::net::Shutdown) -> std::io::Result<()> {
        // Fehler vom `close_notify` werden NICHT gemeldet: der Aufrufer
        // will den Sockel schliessen, und wenn das TLS-Alert nicht
        // ankommt, ist der Sockel trotzdem zu. Der Fehler wird hier
        // bewusst geschluckt — mit einem Kommentar, weil ein
        // geschluckter Fehler sonst wie ein Verlust aussieht.
        let _ = self.close_notify();
        self.socket.shutdown(wie)
    }

    /// Schliesst mit `close_notify` (Alert, Warnung, 0).
    ///
    /// Ein Strom, der im Fehlerfall einfach den Sockel fallen laesst,
    /// sieht fuer den Server wie ein Abbruch aus. Der Alert sagt ihm,
    /// dass nichts kaputt ist.
    pub fn close_notify(&mut self) -> Result<(), StromFehler> {
        if self.gemeldet {
            return Ok(());
        }
        self.gemeldet = true;
        self.sende_verschluesselt(typen::ALERT, &[1, 0])
    }
}

/// Prueft die `CertificateVerify`-Signatur gegen den oeffentlichen
/// Schluessel des Blatts.
///
/// Zwei Verfahren, und beide werden an dem EINEN Inhalt geprueft, den
/// `certificate_verify_inhalt` gebaut hat. Es gibt keinen Pfad ohne
/// Pruefung.
/// added 01.10.2026 22:5x UTC (Projektlabel 02.10.2026), Rolle TLS. GILT die Signatur des Servers?
///
/// added 01.10.2026 22:5x UTC (Projektlabel 02.10.2026): WAS HIER VORHER FEHLTE, und es war ein Loch,
/// keine Kleinigkeit. Der Aufruf `signaturalgorithmen_beide()` (Zeile 594
/// in `nachricht.rs`) bietet **fuenf** Verfahren an:
///
/// ```text
/// 0x0403  ecdsa_secp256r1_sha256   <- geprueft worden
/// 0x0503  ecdsa_secp384r1_sha384   <- vorher NICHT geprueft
/// 0x0804  rsa_pss_rsae_sha256      <- geprueft worden
/// 0x0805  rsa_pss_rsae_sha384      <- vorher NICHT geprueft
/// 0x0401  rsa_pkcs1_sha256         <- geprueft worden
/// ```
///
/// Alles, was nicht in der Liste stand, fiel in den Arm `_ => false` und
/// brach den Handshake mit `StromFehler::SignaturFalsch` ab — ohne zu
/// sagen, welches Verfahren der Server gewaehlt hatte.
///
/// GEMESSEN 01.10.2026 22:4x UTC (Projektlabel 02.10.2026): `www.opensuse.org` (TLS 1.3,
/// `TLS_AES_128_GCM_SHA256`, Zertifikat `CN=opensuse.org` mit P-384) brach
/// genau so ab: „TLS: die Signatur des Servers gilt nicht". Der Server hat
/// 0x0503 gewaehlt, weil **wir** es angeboten haben.
///
/// DIE REGEL, DIE DARAUS FOLGT: was wir anbieten, muessen wir auch
/// pruefen koennen. Sonst entscheidet der Server, ob unsere Seite
/// laedt — und zwar ueber ein Verfahren, das wir nicht beurteilen
/// koennen.
///
/// added 01.10.2026 22:5x UTC (Projektlabel 02.10.2026): DER PARAMETER. Die Fassung davor nahm das ganze
/// `Zertifikat` und las daraus nur `schluessel`. Fuer den Test mit einem
/// `openssl`-Vektor heisst das, ein `Zertifikat` zu bauen — und davon
/// braucht der Test 14 Felder, von denen keines echt waere. Der
/// Schluessel genuegt, also nimmt die Funktion ihn jetzt direkt; das ist
/// auch die ehrlichere Form: die Pruefung behauptet nichts ueber Namen
/// oder Gueltigkeit, also nimmt sie nichts davon entgegen.
fn cv_gueltig(schluessel: &SchluesselArt, cv: &CertificateVerify, inhalt: &[u8]) -> bool {
    use crate::der::oids;
    match (schluessel, cv.algorithmus) {
        // --- RSA ---------------------------------------------------------
        // 0x0804 rsa_pss_rsae_sha256, 0x0805 rsa_pss_rsae_sha384 und
        // 0x0401 rsa_pkcs1_sha256.
        //
        // added 01.10.2026 22:5x UTC (Projektlabel 02.10.2026): 0x0805 kommt dazu. Die PSS-Pruefung
        // nimmt Hash und Salzlaenge als Parameter (`signatur.rs`), also
        // reicht `rsa_pss_pruefen_sha256` mit SHA-384 und Salz 48 —
        // genau so, wie `strom12.rs` (Zeile 1450) es fuer 1.2 macht.
        (SchluesselArt::Rsa { n, e }, 0x0804 | 0x0805 | 0x0401) => {
            crate::signatur::cv_pruefen(n, e, &cv.signatur, inhalt, cv.algorithmus).is_ok()
        }
        // --- ECDSA P-256 mit SHA-256 (0x0403) ----------------------------
        (SchluesselArt::Ecdsa { x, y, kurve }, 0x0403) => {
            if kurve.as_slice() != oids::SECP256R1 {
                return false;
            }
            let kurve_parameter = match crate::ecdsa::kurve_p256() {
                Ok(k) => k,
                Err(_) => return false,
            };
            // Die ECDSA-Signatur ist `SEQUENCE { INTEGER r, INTEGER s }`.
            let teile = match crate::kette::ecdsa_teile_oeffentlich(&cv.signatur) {
                Ok(t) => t,
                Err(_) => return false,
            };
            let h = crate::sha2::sha256(inhalt);
            crate::ecdsa::ecdsa_p256_pruefen_sha256(&kurve_parameter, x, y, &teile.0, &teile.1, &h)
                .is_ok()
        }
        // --- ECDSA P-384 mit SHA-384 (0x0503) ----------------------------
        //
        // added 01.10.2026 22:5x UTC (Projektlabel 02.10.2026): der Zweig, der `www.opensuse.org`
        // brauchte. Kurve **und** Hash muessen zusammenpassen (RFC 8446
        // Abschnitt 4.4.3): 0x0503 nennt P-384 *und* SHA-384, und ein
        // P-384-Schluessel mit SHA-256 zu pruefen waere eine Pruefung
        // ohne Aussage — der Hash waere der falsche, und die Signatur
        // faellt immer durch, was wie ein Serverfehler aussieht.
        //
        // added 01.10.2026 22:5x UTC (Projektlabel 02.10.2026): beide OIDs fuer P-384 werden genommen,
        // genau wie in `kette.rs` Zeile 508. Die alte Fassung kannte
        // `[1, 3, 132, 0, 7]`, was **keine** Kurve ist.
        (SchluesselArt::Ecdsa { x, y, kurve }, 0x0503) => {
            if !matches!(
                kurve.as_slice(),
                [1, 3, 132, 0, 34] | [1, 2, 840, 10045, 3, 1, 31]
            ) {
                return false;
            }
            let kurve_parameter = match crate::ecdsa::kurve_p384() {
                Ok(k) => k,
                Err(_) => return false,
            };
            let teile = match crate::kette::ecdsa_teile_oeffentlich(&cv.signatur) {
                Ok(t) => t,
                Err(_) => return false,
            };
            let h = crate::sha2::sha384(inhalt);
            crate::ecdsa::ecdsa_p384_pruefen_sha384(&kurve_parameter, x, y, &teile.0, &teile.1, &h)
                .is_ok()
        }
        // Alles andere ist ein Fehler, kein `Ok`. Ein unbekanntes
        // Verfahren durchzulassen waere genau die Luecke, um die ein
        // Server eine ungeprueffte Signatur durchbringe.
        _ => false,
    }
}

/// Zerlegt einen Puffer mit Handshake-Nachrichten.
///
/// M4: die Laenge wird aus den Bytes gelesen und VOR dem Kopieren
/// geprueft. M3: die Schleife laeuft ueber `puffer.len()`, und der
/// Puffer ist durch `MAX_ANTWORT` begrenzt.
fn zerlege_nachrichten(puffer: &mut Vec<u8>) -> Result<Vec<Vec<u8>>, StromFehler> {
    let mut raus: Vec<Vec<u8>> = Vec::new();
    let mut pos = 0usize;
    while puffer.len() - pos >= crate::nachricht::HANDSHAKE_KOPF {
        let laenge = ((puffer[pos + 1] as usize) << 16)
            | ((puffer[pos + 2] as usize) << 8)
            | (puffer[pos + 3] as usize);
        if laenge > crate::record::MAX_NACHRICHT {
            return Err(StromFehler::ZuViel("Nachricht zu lang"));
        }
        if puffer.len() - pos < crate::nachricht::HANDSHAKE_KOPF + laenge {
            break;
        }
        raus.push(puffer[pos..pos + crate::nachricht::HANDSHAKE_KOPF + laenge].to_vec());
        pos += crate::nachricht::HANDSHAKE_KOPF + laenge;
    }
    if pos > 0 {
        puffer.drain(..pos);
    }
    Ok(raus)
}

/// Was der Hallo-Tausch zurueckgibt: die gesendeten und die
/// gelesenen Nachrichten und den benutzten privaten Schluessel.
///
/// added 30.09.2026. DER GRUND FUER EINE BENANNTE STRUKTUR: als
/// Tupel `(Vec<u8>, Vec<u8>, ServerHello, [u8; 32])` brauchte die
/// Fortsetzungsfunktion acht Argumente (clippy meldete "too many
/// arguments (8/7)"), und beim Aufruf standen vier `Vec<u8>` und ein
/// Array in einer Reihe — die Reihenfolge zu verwechseln waere beim
/// Lesen des Aufrufs nicht aufgefallen, beim Lauf aber sofort.
///
/// Jedes Feld sagt, OBJEKT es ist: `sh` ist der dekodierte ServerHello,
/// `sh_nachricht` seine Bytes. Beides wird gebraucht — das eine fuer
/// die Pruefungen, das andere fuer den Transkript.
pub struct Hallo {
    /// Die **gesendeten** ClientHello-Bytes.
    pub ch: Vec<u8>,
    /// Die **gelesenen** ServerHello-Bytes.
    pub sh_nachricht: Vec<u8>,
    /// Der dekodierte ServerHello, mit `version`.
    pub sh: ServerHello,
    /// Der x25519-Privatwert, mit dem der `key_share` gebaut wurde.
    pub geheim: [u8; 32],
    /// Die beiden Nachrichten vor dem zweiten ClientHello, wenn ein
    /// HelloRetryRequest kam. Fuer den Transkript.
    pub hrr: Option<Hrr>,
}

/// Die beiden Nachrichten, die ein HelloRetryRequest zwischen
/// ClientHello und ClientHello schiebt.
///
/// added 30.09.2026 fuer W6. DER TRANSCRIPT-ERSATZ: RFC 8446
/// Abschnitt 4.1.1 verlangt, dass im Transkript **weder** der erste
/// ClientHello **noch** der HRR im Klartext stehen, sondern
///
/// ```text
/// message_hash ||        // 0xfe 00 00 01 || len || Hash(ClientHello 1)
/// HelloRetryRequest ||    // die Nachricht, wie sie empfangen wurde
/// message_hash ||        // 0xfe 00 00 01 || len || Hash(ClientHello 2)
/// ```
///
/// `message_hash` ist eine erfundene Handshake-Nachricht: Typ 254 und
/// ein leerer Koerper. Warum: der Client soll den ersten ClientHello
/// nicht zweimal senden muessen, und der Server soll den Transkript
/// trotzdem eindeutig binden koennen.
#[derive(Clone)]
pub struct Hrr {
    /// Der erste, gesendete ClientHello.
    pub erste_ch: Vec<u8>,
    /// Die ServerHello, die das HelloRetryRequest IST.
    pub hrr_sh: Vec<u8>,
}

/// Die Ersatznachricht `message_hash` fuer eine Handshake-Nachricht.
///
/// RFC 8446 Abschnitt 4.4.1: `message_hash` besteht aus dem
/// Nachrichtentyp 254, einer 4-Byte-Laenge und dem Hash der Nachricht.
pub fn message_hash(nachricht: &[u8]) -> Vec<u8> {
    let mut s = Sha256::neu();
    s.aufnehmen(nachricht);
    let h = s.fertig();
    // DER FEHLER, DEN DIESE FUNKTION HATTE: die 4-Byte-Laenge wurde
    // nie geschrieben. Es kam `fe || hash` heraus (33 Byte) statt
    // `fe || 00 00 00 20 || hash` (37 Byte), und der Test
    // `message_hash_hat_typ_254_und_vier_byte_laenge` fiel sofort dar
    // ueber. Erfunden ist die Nachricht, richtig kodiert sein muss sie.
    let mut v = Vec::with_capacity(4 + h.len());
    v.push(254);
    v.extend_from_slice(&(h.len() as u32).to_be_bytes());
    v.extend_from_slice(&h);
    v
}

/// Die DOWNGRD-Marke nach RFC 8446 Abschnitt 4.1.3.
///
/// `44 4f 57 4e 47 52 44 01` = "DOWNGRD" und danach die Minor-Version
/// von TLS 1.2 (0x0303 -> 0x01 steht fuer die Minor-Version 3? nein:
/// der Wert ist `0x01`, weil die Marke fuer TLS 1.2 gilt und der
/// Server die Minor-Version **nicht** in die Marke schreibt).
///
/// GENAU: RFC 8446 Abschnitt 4.1.3 sagt woertlich
///   "the last 8 bytes of the server random field
///    ... are set to the value 0x44 0x4F 0x57 0x4E 0x47 0x52 0x44 0x01"
/// Das letzte Byte ist also fest 0x01 und NICHT die Minor-Version.
/// Diese acht Byte werden hier als Konstante geschrieben, nicht aus
/// dem RFC-Text abgetippt an drei Stellen.
pub const DOWNGRD: [u8; 8] = [0x44, 0x4F, 0x57, 0x4E, 0x47, 0x52, 0x44, 0x01];

/// Prueft die DOWNGRD-Marke in einem ServerRandom.
///
/// `true` heisst: der Server hat die Marke gesetzt. Das ist nach
/// RFC 8446 Abschnitt 4.1.3 ein Angriff, wenn der Client TLS 1.3
/// angeboten hat.
pub fn downgrade_marke(random: &[u8; 32]) -> bool {
    // M3/M4: der Vergleich ueber die letzten acht Byte, ohne dass
    // irgendetwas alloziert wird.
    let mut gleich: u8 = 0;
    for i in 0..8 {
        gleich |= random[24 + i] ^ DOWNGRD[i];
    }
    gleich == 0
}

/// Eine fertige TLS-Verbindung, die **beide** Protokollversionen
/// bedienen kann.
///
/// added 30.09.2026. DER AUFTRAG war: eine Verbindungsfunktion, die ein
/// ClientHello schickt, das TLS 1.3 UND TLS 1.2 anbietet, und dann dem
/// Server die Wahl laesst — ohne einen zweiten Verbindungsversuch mit
/// schwächerer Version.
///
/// WARUM EINE EIGENE TYPE und nicht ein Feld in `TlsStrom`: die beiden
/// Protokollversionen brauchen verschiedene Zustaende. TLS 1.3 hat
/// Schluesselbloecke aus HKDF und einen Record ohne Klartext-Padding,
/// TLS 1.2 hat Schluessel aus der PRF, einen Schluesselblock mit vier
/// Feldern und AEAD mit einem expliziten Nonce im Record. Beides in
/// EINEN Zustand zu legen hiesse: jedes Feld waere optional, und an
/// jeder Verwendung muesste stehen, welcher Pfad gilt. Die Fehler, die
/// daraus entstehen, sind still — ein 1.2-Record mit dem 1.3-Satz
/// scheitert an der Authentifizierung, und die Meldung lautet
/// "Verschluesselung", nicht "falscher Pfad".
///
/// DER AUFRUFER merkt keinen Unterschied: `TlsVerbindung` ist
/// `Read + Write` und hat dieselben sechs Sockelmethoden wie
/// `TlsStrom`.
pub enum TlsVerbindung {
    /// Der Server hat TLS 1.3 (0x0304) gewaehlt.
    Tls13(TlsStrom),
    /// Der Server hat TLS 1.2 (0x0303) gewaehlt.
    Tls12(crate::strom12::Tls12Strom),
}

impl TlsVerbindung {
    /// Die Version, die der Server gewaehlt hat: 0x0303 oder 0x0304.
    pub fn version(&self) -> u16 {
        match self {
            TlsVerbindung::Tls13(_) => crate::nachricht::VERSION_TLS13,
            TlsVerbindung::Tls12(_) => crate::nachricht::VERSION_LEGACY,
        }
    }

    /// Wie steht der stapelte Widerrufsstatus? Siehe
    /// `strom12::Tls12Strom::ocsp_urteil`.
    ///
    /// added 02.10.2026 23:gx UTC (Projektlabel 02.10.2026), S4. DER GRUND
    /// FUER DIE DUENNUNG: `narsil-net` baut seine HTTPS-Verbindung ueber
    /// `TlsVerbindung` (siehe `lib.rs`, `fn verbinde`), **nicht** ueber
    /// `Tls12Strom` — wer nach dem Urteil fragen will, hat also den Enum in
    /// der Hand. Ohne diese Methode muesste `narsil-net` das Enum aufmachen
    /// (public, aber das ist ein zweiter Weg in dieselbe Sache).
    ///
    /// added 02.10.2026 23:gx: **TLS 1.3** liefert
    /// `("nicht ausgewertet", …)`. Das ist keine Luecke, die man hinter
    /// „TLS 1.3 ist neuer" versteckt: In 1.3 steckt der stapelte Status in
    /// der `Certificate`-Nachricht (RFC 8446 4.4.3) und **nicht** in einer
    /// eigenen `CertificateStatus`-Nachricht — die gibt es dort gar nicht. Wir
    /// bieten `status_request` an (seit R7) und lesen die Antwort noch nicht.
    /// Das steht so im Bericht, damit es keine Ueberraschung ist.
    pub fn ocsp_urteil(&self) -> (&'static str, &'static str) {
        match self {
            // added 02.10.2026 23:kx: DER 1.3-PFAD REICHT DAS URTEIL WEITER.
            // Die Fassung davor gab hier einen festen Text zurueck („nicht
            // ausgewertet") — das war richtig, solange der 1.3-Pfad die
            // Zertifikatsnachricht nicht las. Jetzt tut er es (RFC 8446
            // 4.4.3), und der feste Text waere eine **Behauptung**, die der
            // Code nicht mehr verdient.
            TlsVerbindung::Tls13(s) => s.ocsp_urteil(),
            TlsVerbindung::Tls12(s) => s.ocsp_urteil(),
        }
    }

    /// Wurde das **Extended Master Secret** benutzt (nur bei TLS 1.2)?
    /// Liest die **Nachschrift** und legt die Tickets ab — auf dem Weg,
    /// den `narsil-net` wirklich benutzt.
    ///
    /// added 02.10.2026, R4l. DER GRUND FUER DIESE DUENNUNG: `narsil-net`
    /// baut seine HTTPS-Verbindung ueber
    /// `TlsVerbindung::ueber_sockel` (siehe `lib.rs`, `fn verbinde`) —
    /// **nicht** ueber `TlsStrom`. Ohne diese Methode kann ein Aufrufer von
    /// aussen an den `TlsStrom` im Inneren nicht heran, also kann er die
    /// Nachschrift gar nicht abholen, egal wie sie gebaut ist.
    ///
    /// DIE LECTION, DIE DIESE DUENNUNG KOSTET, und sie ist teuer gewesen:
    /// `TlsStrom` und `TlsVerbindung` tragen **denselben Methodennamen**
    /// (`ueber_sockel_mit_psk`), verhalten sich aber **verschieden** — der
    /// Enum hat das Angebot richtig gemerkt, der Strom nicht, und der Strom
    /// hat `selected_identity` nie gespeichert. Der gruene Test stand auf
    /// dem Enum, und der Name hat die andere verdeckt. Jetzt hat **beide**
    /// Typen den Aufruf, und `tests/nachschrift_ueber_enum.rs` behauptet es
    /// fuer den Enum.
    ///
    /// **TLS 1.2 LIEFERT `Ok(0)`**, und das ist **kein** Fehler: in diesem
    /// Crate gibt es `NewSessionTicket` nur fuer TLS 1.3 (RFC 8446 4.6.1),
    /// und ein 1.2-Server schickt sie nicht. Ein Aufrufer, der es nicht
    /// unterscheidet, wuerde jede 1.2-Seite als Fehler melden.
    /// Die aktuelle Lesefrist, oder `None`. Siehe `TlsStrom::lesefrist`.
    pub fn lesefrist(&self) -> Option<std::time::Duration> {
        match self {
            TlsVerbindung::Tls13(s) => s.lesefrist(),
            TlsVerbindung::Tls12(s) => s.lesefrist(),
        }
    }

    /// Wie `hole_nachschrift`, aber **mit eigener Frist** — und die Frist
    /// wird hinterher **zurueckgesetzt**.
    ///
    /// added 01.10.2026 (Uhr) / 02.10.2026 (Projektlabel), TLS R4n. DER
    /// GRUND FUER DIE ZWEITE METHODE: `hole_nachschrift` braucht eine
    /// **kurze** Lesefrist, und das Zuruecksetzen ist **Pflicht**, nicht
    /// Sorgfalt — sonst gilt die kurze Frist fuer den ganzen Antwortpfad.
    /// Der erste Aufrufer (`narsil-net`) hatte genau das getan, was man nicht
    /// tun soll: `let _ = strom.set_read_timeout(Some(kurz)); …` und danach
    /// **nichts** zurueckgesetzt, weil `set_read_timeout` den alten Wert
    /// **nicht** zurueckgibt. Die Antwort wurde dann mit 250 ms gelesen.
    ///
    /// **DIE LÖSUNG IST, DASS DER AUFRUFER GAR NICHT FRAGEN MUSS:** hier
    /// wird die Frist gesetzt, gelesen und auf `LESEZEIT` zurueckgesetzt —
    /// der Wert, den `ueber_sockel` selbst gesetzt hat. Ein Aufrufer, der
    /// die Frist vergisst, kann sie nicht vergessen, weil er sie nicht
    /// setzt.
    ///
    /// `LESEZEIT` und nicht „der Wert von vorher": es gibt keinen Zugriff
    /// auf den, und der Wert ist an **einer** Stelle festgeschrieben
    /// (`ueber_sockel`, `ueber_sockel_mit_psk`, `ueber_sockel_mit`).
    /// Dieselbe Zahl an drei Stellen zu wiederholen waere die zweite
    /// Wahrheit ueber dieselbe Sache.
    pub fn hole_nachschrift_mit_frist(
        &mut self,
        tickets: &mut crate::ticket::Tickets,
        host: &str,
        jetzt: u64,
        max_records: usize,
        frist: std::time::Duration,
    ) -> Result<usize, StromFehler> {
        match self {
            TlsVerbindung::Tls13(s) => {
                let _ = s.set_read_timeout(Some(frist));
                let raus = s.hole_nachschrift(tickets, host, jetzt, max_records);
                // DAS ZURUECKSETZEN, und es ist nicht im Fehlerpfad
                // versteckt: es laeuft **immer**, weil der Aufrufer sonst bei
                // einem Fehler mit 250 ms weiterlaeuft und der naechste
                // Lesevorgang — die HTTP-Antwort — daran scheitert.
                let _ = s.set_read_timeout(Some(LESEZEIT));
                raus
            }
            TlsVerbindung::Tls12(_) => Ok(0),
        }
    }

    pub fn hole_nachschrift(
        &mut self,
        tickets: &mut crate::ticket::Tickets,
        host: &str,
        jetzt: u64,
        max_records: usize,
    ) -> Result<usize, StromFehler> {
        match self {
            TlsVerbindung::Tls13(s) => s.hole_nachschrift(tickets, host, jetzt, max_records),
            // added 02.10.2026, R4l: `Ok(0)` statt Fehler — siehe Kopf.
            TlsVerbindung::Tls12(_) => Ok(0),
        }
    }

    /// Die vom Server **gewählte** ALPN-Kennung, oder `None`.
    ///
    /// added 01.10.2026 09:2x UTC (Projektlabel 02.10.2026), R4x. DIE
    /// LÜCKE, an der A2 hinge: `TlsStrom` hat das Feld `alpn` seit dem
    /// 30.09. (TLS R1, S7), aber `TlsVerbindung` — der Typ, den `narsil-net`
    /// benutzt — hatte **keinen** Weg darauf.
    ///
    /// GEMESSEN (`grep -n "alpn" strom.rs | grep "pub fn"` = leer, und
    /// `grep -rn "b\"http/1.1\"" narsil-net/src/lib.rs` = zwei Stellen):
    /// `narsil-net` bietet `http/1.1` an und **kann nicht nachsehen**, was
    /// der Server gewählt hat. Nach RFC 7301 3.2 gilt ohne ALPN-Antwort
    /// HTTP/1.1 — das ist die Vorgabe, nicht die Aushandlung.
    ///
    /// WARUM EINE METHODE UND NICHT DAS FELD ÖFFNEN: `TlsVerbindung` ist
    /// ein `enum` mit zwei Varianten (`Tls13`, `Tls12`), und beide tragen
    /// ein **eigenes** `alpn`-Feld. Ein Zugriff von außen müsste beide
    /// Zweige kennen — genau die Drei-Pfade-Falle aus R4k/R4s, nur mit
    /// einer anderen Anzahl Pfade. Eine Methode fasst das an **einer**
    /// Stelle zusammen.
    ///
    /// DIE BEDEUTUNG, und sie ist nicht „irgendein Protokoll": für A2 ist
    /// genau `Some(b"h2")` der Unterschied zwischen „HTTP/2 sprechen" und
    /// „HTTP/1.1 sprechen". Ohne diese Methode müsste `narsil-net` raten.
    pub fn alpn(&self) -> Option<&[u8]> {
        match self {
            TlsVerbindung::Tls13(s) => s.alpn.as_deref(),
            TlsVerbindung::Tls12(s) => s.alpn.as_deref(),
        }
    }

    /// added 04.10.2026, Rolle TLS. **DEN TICKET-SPEICHER AN DEN STROM HENGEN,
    /// und das ist der Schritt, der das Warten ueberfluessig macht.**
    ///
    /// **WAS DAMIT PASSIERT (Auftrag NETZTEMPO_25):** `NewSessionTicket`
    /// kommen in der **Anwendungsphase**, meist zusammen mit der ersten
    /// Antwort. **Ab jetzt** wertet die Leseschleife sie aus, **sobald sie
    /// ankommen** — es wird **nicht mehr auf sie gewartet**. **GEMESSEN dort:
    /// 250 ms Wartezeit je Verbindung, 280–410 ms je Handshake.**
    ///
    /// **WARUM ALS SETZER UND NICHT ALS PARAMETER BEIM BAU:** der Aufrufer
    /// (`narsil-net`) hat den Speicher **nach** dem Dial in der Hand, und
    /// `ueber_sockel` wird **an vielen Stellen** ohne Speicher aufgerufen
    /// (Tests, `narsil-server`). **Ein Pflichtparameter** waere an **jeder** von
    /// ihnen eine Aenderung in **fremden** Dateien.
    pub fn nachschrift_speicher_setzen(
        &mut self,
        speicher: std::sync::Arc<std::sync::Mutex<crate::ticket::Tickets>>,
    ) {
        match self {
            TlsVerbindung::Tls13(s) => s.nachschrift_speicher = Some(speicher),
            TlsVerbindung::Tls12(_) => {}
        }
    }

    /// added 04.10.2026, Rolle TLS. **WIE VIELE POST-HANDSHAKE-TICKETS BIS JETZT
    /// AUSGEWERTET WURDEN.** Reine Lesefrage fuer Tests und Berichte: **ohne**
    /// sie ist „die Auswertung laeuft“ eine Behauptung, und GEMESSEN werden
    /// muss, ob der Weg **beruehrt** wird.
    pub fn nachschrift_ausgewertet(&self) -> usize {
        match self {
            TlsVerbindung::Tls13(s) => s.nachschrift_ausgewertet,
            TlsVerbindung::Tls12(_) => 0,
        }
    }

    /// Hat der Server `h2` gewählt?
    ///
    /// added 01.10.2026 09:2x, R4x. DIE ENTSCHEIDUNG als eigene Methode,
    /// und der Grund ist ein Fehler, den man mit dem Vergleich sonst
    /// dreimal schreibt: `alpn() == Some(b"h2")` scheitert an der
    /// Typprüfung, sobald der Rückgabewert `[u8]` statt `&[u8; 2]` ist.
    /// GEMESSEN: `Some(b"h2".as_slice())` ist die Form, die der Compiler
    /// annimmt — `self.alpn.as_deref() == Some(b"h2".as_slice())`
    /// steht so in `narsil-server/src/tls.rs:307`.
    pub fn ist_h2(&self) -> bool {
        self.alpn() == Some(b"h2".as_slice())
    }

    /// Welche PSK-Identitaet der Server gewaehlt hat, oder `None`.
    ///
    /// added 02.10.2026, R4l. **Eine Methode und kein Feld**, weil der
    /// Aufrufer von aussen nur die `TlsVerbindung` sieht und nicht in die
    /// Variante hinein. Ohne diese Methode kann niemand feststellen, ob
    /// ein Angebot angenommen wurde — und genau diese Behauptung hat
    /// `tests/psk_rundlauf.rs` bisher nur auf dem **anderen** Typ getroffen.
    pub fn selected_identity(&self) -> Option<u16> {
        match self {
            TlsVerbindung::Tls13(s) => s.selected_identity,
            // TLS 1.2 kann keine PSK waehlen (kein `pre_shared_key` in der
            // ServerHello nach RFC 5246), also gibt es hier nichts zu melden.
            TlsVerbindung::Tls12(_) => None,
        }
    }

    pub fn erweitertes_geheimnis(&self) -> Option<bool> {
        match self {
            TlsVerbindung::Tls13(_) => None,
            TlsVerbindung::Tls12(s) => Some(s.erweitertes_geheimnis),
        }
    }

    /// Das Serverzertifikat im DER, fuer Anwendungen, die es zeigen.
    /// added 03.10.2026 16:2x, R60. **DIE KETTE FUER DEN AUFRUFER** — und sie
    /// heisst `kette()`, **nicht** `kette_der()`, obwohl das Feld so heisst.
    /// GEMESSEN: der Uebersetzer meldete „attempted to take value of method
    /// `kette_der` on type `&TlsVerbindung`" — **in Rust darf ein Getter nicht
    /// denselben Namen tragen wie sein Feld**, der Methodenname verdeckt das Feld
    /// und `self.kette_der` findet die Methode. Das ist eine Form, die man nur
    /// einmal lernt: in manchen Sprachen ist `feld` und `feld()` zweierlei, hier
    /// ist es **eins**, und der Compiler entscheidet.
    ///
    /// added 03.10.2026 16:2x: **UND ES IST EIN GETTER STATT EINES `pub`-FELDS**,
    /// damit der Aufrufer die Liste **nicht veraendern** kann — dieselbe
    /// Begruendung wie bei `blatt_der`.
    pub fn kette(&self) -> &[Vec<u8>] {
        // added 03.10.2026 16:3x: **`match` UEBER BEIDE VARIANTEN**, und mein
        // erster Versuch war `self.kette_der` — also ein **Feldzugriff auf das
        // Enum**. GEMESSEN: „no field `kette_der` on type `&TlsVerbindung`“.
        // `TlsVerbindung` ist ein Enum ueber `Tls13` und `Tls12`, und **beide**
        // tragen das Feld — deshalb der `match`, genau wie bei `blatt_der` und
        // `alpn`.
        match self {
            TlsVerbindung::Tls13(s) => &s.kette_der,
            TlsVerbindung::Tls12(s) => &s.kette_der,
        }
    }

    pub fn blatt_der(&self) -> &[u8] {
        match self {
            TlsVerbindung::Tls13(s) => &s.blatt_der,
            TlsVerbindung::Tls12(s) => &s.blatt_der,
        }
    }

    /// Verbindet, macht den Handshake und liefert eine fertige Verbindung.
    ///
    /// `verlange_ems` wird nur im TLS-1.2-Pfad gelesen; siehe
    /// `strom12::Tls12Strom::verbinde_mit`.
    pub fn verbinde(
        host: &str,
        port: u16,
        alpn: &[&[u8]],
        speicher: &Vertrauensspeicher,
        jetzt: i64,
    ) -> Result<TlsVerbindung, StromFehler> {
        TlsVerbindung::verbinde_mit(host, port, alpn, speicher, jetzt, false)
    }

    /// Wie `verbinde`, aber mit der Entscheidung ueber RFC 7627.
    pub fn verbinde_mit(
        host: &str,
        port: u16,
        alpn: &[&[u8]],
        speicher: &Vertrauensspeicher,
        jetzt: i64,
        verlange_ems: bool,
    ) -> Result<TlsVerbindung, StromFehler> {
        // added 04.10.2026, Rolle TLS. **DER ERSTE SCHRITT ÜBERHAUPT, und er
        // fehlte in der ersten Fassung — GEMESSEN.** Die Tabelle in
        // `tests/schrittzeit_live.rs` summierte sich bei `www.rust-lang.org` auf
        // **25 ms** bei **1072 ms** Gesamtdauer. **Die fehlenden 1047 ms waren
        // nicht „irgendwo“**: sie sind **Adressauflösung und TCP-Verbindung**,
        // also **vor** dem TLS-Handshake. Und das ist **kein** TLS-Problem,
        // sondern ein **DNS**-Problem — **und es ist die halbe Differenz** zu
        // der Zahl aus NETZTEMPO_22 (133–543 ms).
        //
        // **DIE LEHRE, und sie ist allgemeiner als TLS:** wer eine Zeitmessung
        // **in der Mitte** des Ablaufs ansetzt, misst den Rest **nicht** — und
        // der Rest ist hier **zehnmal** so gross wie das Messobjekt.
        let s = crate::schritt!("NETZ: DNS aufloesen und TCP verbinden", {
            TlsStrom::verbinde_ohne_handshake(host, port)?
        });
        TlsVerbindung::ueber_sockel_mit(s, host, alpn, speicher, jetzt, verlange_ems)
    }

    /// Nimmt einen BEREITS VERBUNDENEN Sockel.
    ///
    /// added 30.09.2026 fuer `narsil-net`: der Sockel kommt aus der
    /// eigenen Adresssuche dort, mit eigenen Zeitgrenzen.
    pub fn ueber_sockel(
        socket: std::net::TcpStream,
        host: &str,
        alpn: &[&[u8]],
        speicher: &Vertrauensspeicher,
        jetzt: i64,
    ) -> Result<TlsVerbindung, StromFehler> {
        TlsVerbindung::ueber_sockel_mit(
            TlsStrom::aus_sockel(socket, host),
            host,
            alpn,
            speicher,
            jetzt,
            false,
        )
    }

    /// Wie `ueber_sockel`, aber **mit** PSK-Angebot.
    ///
    /// added 02.10.2026, R4d Stufe 2. DER WEG: das Angebot muss in den
    /// **ClientHello**, also **vor** der Versionsentscheidung. Danach ist
    /// der Weg derselbe wie in `ueber_sockel_mit` — bis auf eine
    /// Pruefung, die nur bei einer Wiederaufnahme etwas bedeutet (unten).
    ///
    /// WAS HIER **NICHT** GEPRUEFT WIRD, und das ist der wichtigste Satz
    /// dieser Funktion: ob der Server unser Angebot **angenommen** hat.
    /// Das steht im `ServerHello` als `selected_identity`, und ein Client
    /// MUSS es pruefen (RFC 8446 4.2.11.3: wenn der Server keine
    /// `pre_shared_key`-Erweiterung schickt, hat er die Wiederaufnahme
    /// abgelehnt, und der Client macht einen vollstaendigen Handshake).
    /// Ohne diese Pruefung wuerde ein abgelehntes Angebot als angenommen
    /// gelten, und der Schluesselplan waere mit einem PSK gebaut, den der
    /// Server nie benutzt hat — der erste Record schlaege dann fehl.
    ///
    /// DER AUFBAUF DIESER FUNKTION, und er ist noch nicht der fertige Weg:
    /// sie schickt das Angebot und macht den Hello-Tausch. Die Annahme-
    /// pruefung folgt im naechsten Schritt (R4d Stufe 3); bis dahin steht
    /// hier ausdruecklich, dass ein Server, der ablehnt, zu einem
    /// `finished`-Fehler fuehrt statt zu einem sauberen Rueckfall.
    pub fn ueber_sockel_mit_psk(
        socket: std::net::TcpStream,
        host: &str,
        alpn: &[&[u8]],
        speicher: &Vertrauensspeicher,
        jetzt: i64,
        angebot: Option<&crate::ticket::PskAngebot>,
    ) -> Result<TlsVerbindung, StromFehler> {
        // M6: Zeitgrenzen wie in `ueber_sockel_mit`, nur wenn der
        // Aufrufer keine gesetzt hat.
        let mut s = TlsStrom::aus_sockel(socket, host);
        if s.socket.read_timeout().ok().flatten().is_none() {
            s.socket
                .set_read_timeout(Some(LESEZEIT))
                .map_err(|e| StromFehler::Netz(e.to_string()))?;
        }
        if s.socket.write_timeout().ok().flatten().is_none() {
            s.socket
                .set_write_timeout(Some(LESEZEIT))
                .map_err(|e| StromFehler::Netz(e.to_string()))?;
        }
        let hallo = s.hallo_tauschen_mit_psk(host, alpn, angebot)?;
        match hallo.sh.version {
            crate::nachricht::VERSION_TLS13 => s.handshake_ab_serverhello(
                host,
                speicher,
                jetzt,
                &Hallo {
                    ch: hallo.ch,
                    sh_nachricht: hallo.sh_nachricht,
                    sh: hallo.sh,
                    geheim: hallo.geheim,
                    hrr: None,
                },
            )?,
            crate::nachricht::VERSION_LEGACY => {
                return Err(StromFehler::Unerwartet(
                    "der Server hat TLS 1.2 gewaehlt, obwohl ein PSK angeboten wurde \
                     (eine Wiederaufnahme gibt es nur in TLS 1.3)",
                ));
            }
            _ => return Err(StromFehler::Unerwartet("unbekannte Version im ServerHello")),
        }
        Ok(TlsVerbindung::Tls13(s))
    }

    /// Wie `ueber_sockel`, aber mit der Entscheidung ueber RFC 7627 und
    /// einem bereits gebauten `TlsStrom` (der Sockel steckt darin).
    ///
    /// DER GANZE ABLAUF, in genau dieser Reihenfolge:
    ///
    /// 1. EIN ClientHello, das TLS 1.3 **und** TLS 1.2 anbietet
    ///    (`ClientHello::baue_beide`). Es wird gesendet und **nicht**
    ///    wiederholt: ein zweiter Versuch mit nur TLS 1.2 waere genau
    ///    das, was der Auftrag verbietet, und es waere die Luecke, in
    ///    die ein Angreifer einen einfuehrt.
    /// 2. EIN ServerHello wird gelesen.
    /// 3. `ServerHello.version` entscheidet den Pfad. 0x0304 heisst
    ///    TLS 1.3, 0x0303 heisst TLS 1.2. Alles andere ist
    ///    `VersionUnerwartet`.
    /// 4. **Downgrade-Schutz** (RFC 8446 Abschnitt 4.1.3): Antwortet
    ///    der Server mit TLS 1.2, obwohl wir 0x0304 angeboten haben,
    ///    dann pruefen die letzten acht Byte seines ServerRandom. Steht
    ///    dort die DOWNGRD-Marke, ist es ein Angriff und der Handshake
    ///    bricht ab.
    pub fn ueber_sockel_mit(
        mut s: TlsStrom,
        host: &str,
        alpn: &[&[u8]],
        speicher: &Vertrauensspeicher,
        jetzt: i64,
        verlange_ems: bool,
    ) -> Result<TlsVerbindung, StromFehler> {
        // M6: Zeitgrenzen wie in `TlsStrom::ueber_sockel` — nur setzen,
        // wenn der Aufrufer keine gesetzt hat.
        if s.socket.read_timeout().ok().flatten().is_none() {
            s.socket
                .set_read_timeout(Some(LESEZEIT))
                .map_err(|e| StromFehler::Netz(e.to_string()))?;
        }
        if s.socket.write_timeout().ok().flatten().is_none() {
            s.socket
                .set_write_timeout(Some(LESEZEIT))
                .map_err(|e| StromFehler::Netz(e.to_string()))?;
        }
        let hallo = s.hallo_tauschen(host, alpn)?;
        // ACHTUNG, und das war ein echter Fehler: `VERSION_TLS13` ohne
        // Pfad ist im `match` ein **Bindungsmuster**, keine
        // Konstante. Es bindet JEDEN Wert, der TLS-1.2-Zweig war
        // toter Code, und die Warnung hiess "unreachable pattern".
        //
        // Derselbe Fehler steht in `strom12.rs` an der
        // ServerHello-Auswertung und war dort drei Stunden lang die
        // Ursache: "kein Extended Master Secret" bei einem Server, der
        // es sehr wohl geschickt hatte. Weg mit ihm: ein Muster, das
        // nach einer Konstante aussieht, braucht einen Pfad.
        match hallo.sh.version {
            crate::nachricht::VERSION_TLS13 => {
                s.handshake_ab_serverhello(host, speicher, jetzt, &hallo)?;
                Ok(TlsVerbindung::Tls13(s))
            }
            crate::nachricht::VERSION_LEGACY => {
                // DER DOWNGRADE-SCHUTZ, bevor irgendetwas weitergeht.
                if downgrade_marke(&hallo.sh.random) {
                    return Err(StromFehler::Downgrade);
                }
                // Der Sockel wandert in den TLS-1.2-Strom. EIN Sockel,
                // EIN Versuch: `Tls12Strom` darf keinen zweiten
                // aufbauen, sonst waere es der verbotene zweite
                // Verbindungsversuch.
                let socket = s.socket;
                let mut t = crate::strom12::Tls12Strom::ueber_sockel(socket, host);
                let mut flug: Vec<Vec<u8>> = Vec::with_capacity(5);
                flug.push(hallo.sh_nachricht.clone());
                flug.extend(t.lese_flug().map_err(|e| StromFehler::Tls12(Box::new(e)))?);
                let eingang = crate::strom12::Eingang12 {
                    ch: &hallo.ch,
                    flug: &flug,
                    privat: hallo.geheim,
                };
                t.handshake_ab_serverhello(host, speicher, jetzt, verlange_ems, &eingang)
                    .map_err(|e| StromFehler::Tls12(Box::new(e)))?;
                Ok(TlsVerbindung::Tls12(t))
            }
            ist => Err(StromFehler::VersionUnerwartet { ist }),
        }
    }

    pub fn set_nodelay(&mut self, wert: bool) -> std::io::Result<()> {
        match self {
            TlsVerbindung::Tls13(s) => s.set_nodelay(wert),
            TlsVerbindung::Tls12(s) => s.set_nodelay(wert),
        }
    }

    pub fn set_read_timeout(&mut self, d: Option<std::time::Duration>) -> std::io::Result<()> {
        match self {
            TlsVerbindung::Tls13(s) => s.set_read_timeout(d),
            TlsVerbindung::Tls12(s) => s.set_read_timeout(d),
        }
    }

    pub fn set_write_timeout(&mut self, d: Option<std::time::Duration>) -> std::io::Result<()> {
        match self {
            TlsVerbindung::Tls13(s) => s.set_write_timeout(d),
            TlsVerbindung::Tls12(s) => s.set_write_timeout(d),
        }
    }

    pub fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        match self {
            TlsVerbindung::Tls13(s) => s.local_addr(),
            TlsVerbindung::Tls12(s) => s.local_addr(),
        }
    }

    pub fn peer_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        match self {
            TlsVerbindung::Tls13(s) => s.peer_addr(),
            TlsVerbindung::Tls12(s) => s.peer_addr(),
        }
    }

    /// Schliesst mit `close_notify` (TLS 1.2) und dann mit dem Sockel.
    ///
    /// DER UNTERSCHIED ZWISCHEN DEN BEIDEN PFADEN, und er ist nicht
    /// kosmetisch: TLS 1.3 schickt `close_notify` ueber die
    /// Anwendungs-Schluessel, TLS 1.2 ueber den AEAD mit
    /// `salt || sequenznummer` als Nonce. Der Aufrufer sieht in
    /// beiden Faellen dasselbe `shutdown`.
    pub fn beende(&mut self, wie: std::net::Shutdown) -> std::io::Result<()> {
        match self {
            TlsVerbindung::Tls13(s) => s.beende(wie),
            TlsVerbindung::Tls12(s) => s.beende(wie),
        }
    }
}

impl Read for TlsVerbindung {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            TlsVerbindung::Tls13(s) => s.read(buf),
            TlsVerbindung::Tls12(s) => s.read(buf),
        }
    }
}

impl Write for TlsVerbindung {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            TlsVerbindung::Tls13(s) => s.write(buf),
            TlsVerbindung::Tls12(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            TlsVerbindung::Tls13(s) => s.flush(),
            TlsVerbindung::Tls12(s) => s.flush(),
        }
    }
}

impl Read for TlsStrom {
    /// Liest Anwendungsdaten.
    ///
    /// In der Anwendungsphase kennt TLS keine Nachrichtengrenze mehr
    /// (RFC 8446 Abschnitt 5.1); der Inhalt eines Records ist einfach
    /// ein Stueck `application_data`. Deshalb wird der Klartext
    /// direkt in `self.klattr` gelegt und von dort bedient.
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        while self.klattr.is_empty() {
            let (typ, nutzlast) = match self.lese_naechsten_verschluesselten() {
                Ok(v) => v,
                // EIN sauberes Ende ist KEIN Fehler. `Read` gibt 0
                // zurueck, wenn die Quelle leer ist, und eine
                // geschlossene Verbindung ist eine leere Quelle.
                //
                // EIGENER FEHLER: hier stand nur ein `Err`, also ein
                // `io::Error`. Der Test brach mit "der Server hat die
                // Verbindung geschlossen" ab, obwohl der Handshake da
                // laengst fertig war. Bei `Connection: close` ist das
                // aber das normale Ende — der Server puffert die
                // Antwort und schliesst danach.
                //
                // `Ok(0)` ist ausserdem das, was ein Aufrufer
                // erwartet: `read` liefert 0 am Ende, nicht einen
                // Fehler. Ein `Err` schickt jeden Leser, der auf 0
                // prueft, in die Fehlerbehandlung.
                //
                // Der `AEAD`-Fehler bleibt ein Fehler: eine falsche
                // Authentifizierung ist kein Ende, sondern ein
                // Angriff oder ein Defekt.
                Err(StromFehler::VerbindungZu) => {
                    return Ok(0);
                }
                Err(e) => return Err(std::io::Error::other(e.to_string())),
            };
            match typ {
                typen::APPLICATION_DATA => self.klattr = nutzlast,
                typen::ALERT => {
                    // Ein Alert vom Server beendet die Richtung.
                    return Ok(0);
                }
                typen::HANDSHAKE => {
                    // added 04.10.2026, Rolle TLS. **DIE ANWENDUNGSPHASE
                    // LIEfert JETZT TICKETS — und darauf wird nicht mehr
                    // gewartet.**
                    //
                    // **WAS HIER BISHER STAND:** `continue`, also **verwerfen**.
                    // Und weil nichts wartete,乍 blieb der Auftrag der Leitung
                    // (NETZTEMPO_25) nur ueber `hole_nachschrift_mit_frist`
                    // loesbar: **250 ms Wartezeit je Verbindung**, gebucht als
                    // „tls“. **GEMESSEN dort: 280–410 ms je Handshake.**
                    //
                    // **DER WEG, UND ER IST KURZ:** Nachschrift auswerten,
                    // Ergebnis **in den mitgeteilten Speicher**, Record
                    // **verwerfen**, naechsten Record lesen. **Kein Warten, kein
                    // zweiter Puffer, keine Frist.**
                    //
                    // **UND WARUM DAS RICHTIG IST:** `NewSessionTicket`
                    // kommen nach RFC 8446 4.6.1 **wenn der Server Lust hat** —
                    // haeufig zusammen mit der ersten Antwort, manchmal
                    // **danach**. **Wer sie sofort braucht, muss warten** (das
                    // war der alte Weg); **wer sie irgendwann braucht, muss
                    // lauschen** (das ist der neue). **Frist 0 ist dabei
                    // kaputt** — GEMESSEN: die Ticket-Records bleiben im Sockel
                    // und die HTTP-Antwort liest sie als Daten (41 s bis zum
                    // Fehler).
                    //
                    // **`KeyUpdate` (Typ 24) und alles andere** wird verworfen,
                    // genau wie vorher: RFC 8446 erlaubt das, und ein Client,
                    // der sich hier weigert, bricht ab, sobald ein Server etwas
                    // Neues schickt.
                    //
                    // **UND DER SPEICHER, und warum ein Mutex um ihn:** der
                    // Aufrufer teilt ihn mit `hole_nachschrift` und dem
                    // Angebot fuer die naechste Verbindung. Ein `RefCell` waere
                    // billiger und **ginge nicht durch den Pool**, der `Send`
                    // verlangt.
                    if let Some(speicher) = self.nachschrift_speicher.clone() {
                        // added 04.10.2026: **DAS GESPERR NUR UEBER DEN EINSCHUB.**
                        // **NICHT** ueber `lese_naechsten_verschluesselten`: wer
                        // **hier** sperrt und dann liest, haelt das Geraete
                        // waehrend des **Netzlesens** — und ein zweiter Faden,
                        // der ein Ticket holen will, wartet bis zum Rundenende.
                        let mut tickets = speicher.lock().unwrap_or_else(|e| e.into_inner());
                        // **`host` und `jetzt` VOR dem Aufruf kopieren:** der
                        // Aufruf leiht `self` mutabel, und `&self.host` waere
                        // dann **zwei** geliehene Blicke auf denselben Strom.
                        let host = self.host.clone();
                        let jetzt = self.nachschrift_zeit;
                        match self.nachschrift_auswerten(&nutzlast, &mut tickets, &host, jetzt) {
                            Ok(_) => {}
                            // **EIN KAPUTTER RECORD IST KEIN FEHLER DES LESERS.**
                            // `read` darf nicht an einem fremden Ticket
                            // scheitern — die Antwort steht in den naechsten
                            // Records. Es steht hier **statt** `?`, weil ein `?`
                            // jeden Seitenabruf mit einem fremden Ticket killt.
                            Err(_) => {}
                        }
                    }
                    continue;
                }
                _ => continue,
            }
        }
        let n = buf.len().min(self.klattr.len());
        buf[..n].copy_from_slice(&self.klattr[..n]);
        self.klattr.drain(..n);
        Ok(n)
    }
}

impl Write for TlsStrom {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        // M4: ein `write` darf nicht groesser sein, als ein Record
        // aufnehmen kann. `MAX_INNEN` ist der Klartext-Teil.
        let n = buf.len().min(crate::record::MAX_INNEN);
        self.sende_verschluesselt(typen::APPLICATION_DATA, &buf[..n])
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.socket.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// added 01.10.2026 22:5x UTC (Projektlabel 02.10.2026), Rolle TLS: ECHTER P-384-Vektor, von
    /// `openssl` erzeugt (nicht abgetippt).
    ///
    /// DER WEG (jeder Schritt ausfuehrbar, `openssl` 3.5.7 am 02.10.2026
    /// 13:7x):
    ///
    /// ```text
    /// openssl ecparam -name secp384r1 -genkey -noout -out p384.key
    /// openssl ec -in p384.key -pubout -outform DER -out p384.spki
    /// printf 'TLS 1.3, server CertificateVerify' > msg.txt
    /// openssl dgst -sha384 -sign p384.key -out p384.sig msg.txt
    /// ```
    ///
    /// Der oeffentliche Punkt steht als letztes Byte im SPKI
    /// (`04 || X || Y`, 97 Byte), `r` und `s` stehen in `p384.sig`. Die
    /// Nachricht, die signiert wurde, ist genau
    /// `crate::signatur::VORLAGE_SERVER` — deshalb heisst der Test
    /// `*_ist_gueltig_p384` nicht, dass wir den Zufall wiederholen,
    /// sondern dass `ecdsa_p384_pruefen_sha384` eine **fremde** Signatur
    /// annimmt.
    mod vektor_p384 {
        /// `X` aus dem SPKI.
        pub const X: [u8; 48] = [
            0xd0, 0xe5, 0x3f, 0x99, 0xff, 0xfd, 0x39, 0x48, 0x82, 0xdd, 0x47, 0x2c, 0x8b, 0xbc,
            0x90, 0x80, 0xca, 0xc2, 0xd3, 0x8a, 0x42, 0x74, 0x94, 0x88, 0xc8, 0xf4, 0x7c, 0xb8,
            0x91, 0xa9, 0x52, 0xb9, 0xe2, 0xf2, 0xa6, 0x27, 0x7d, 0xe0, 0x7d, 0xd8, 0x03, 0x03,
            0x28, 0x32, 0xd1, 0x46, 0xf9, 0x06,
        ];
        /// `Y` aus dem SPKI.
        pub const Y: [u8; 48] = [
            0x20, 0x31, 0x59, 0xe7, 0x17, 0xed, 0xc2, 0xfe, 0xa2, 0x99, 0x79, 0x7c, 0xa7, 0x45,
            0x7d, 0xeb, 0xb5, 0x84, 0xd2, 0x87, 0x77, 0x58, 0x9f, 0x8d, 0xb4, 0xb4, 0xe0, 0x1b,
            0x8a, 0x2b, 0x33, 0xac, 0xa0, 0xa0, 0xce, 0x16, 0x22, 0x0e, 0x04, 0x0f, 0xbf, 0xcb,
            0xb0, 0x52, 0xb0, 0x4c, 0x75, 0xf5,
        ];
        /// `SEQUENCE { INTEGER r, INTEGER s }`, unveraendert aus `p384.sig`.
        pub const SIGNATUR: [u8; 104] = [
            0x30, 0x66, 0x02, 0x31, 0x00, 0xf6, 0x6d, 0x25, 0x6f, 0xb6, 0x14, 0x5e, 0x8a, 0x90,
            0xaa, 0xe9, 0x47, 0x1f, 0x45, 0x76, 0x71, 0x6e, 0x85, 0xa1, 0xe2, 0xbb, 0xbe, 0x3c,
            0xa5, 0x29, 0x3b, 0x62, 0x56, 0x5f, 0xbc, 0xc0, 0x0b, 0x0a, 0x8c, 0x5f, 0xaf, 0xa3,
            0x5f, 0xae, 0xdb, 0xef, 0x6e, 0x29, 0x4c, 0x11, 0x57, 0x1c, 0xa3, 0x02, 0x31, 0x00,
            0xdf, 0x48, 0x04, 0x63, 0x17, 0xd4, 0x0f, 0x1b, 0x40, 0x4d, 0x5b, 0x88, 0x21, 0x65,
            0x24, 0xe5, 0xd4, 0x09, 0x01, 0xb8, 0xac, 0x50, 0x9f, 0xcf, 0x9b, 0x25, 0x91, 0x2e,
            0xd0, 0x9d, 0x42, 0xf4, 0xa1, 0x3e, 0xab, 0x88, 0xcd, 0x3b, 0xe4, 0x39, 0x4d, 0xf9,
            0xff, 0x6e, 0x66, 0xa6, 0x22, 0xc8,
        ];
        /// `secp384r1` als OID-Teile.
        pub const KURVE: [u64; 5] = [1, 3, 132, 0, 34];
    }

    /// added 01.10.2026 22:5x UTC (Projektlabel 02.10.2026): der Schluessel aus dem Vektor.
    ///
    /// added 01.10.2026 22:5x UTC (Projektlabel 02.10.2026): hier steht **kein** Zertifikat. Die Fassung
    /// davor baute ein `crate::x509::Zertifikat` mit 14 Feldern und
    /// haette dafuer `Zertifikat::leer()` gebraucht — eine Funktion nur
    /// fuer den Test, und im normalen Bau unbenutzt (also Warnung).
    /// Deshalb nimmt `cv_gueltig` jetzt den Schluessel.
    fn schluessel_p384() -> SchluesselArt {
        SchluesselArt::Ecdsa {
            x: vektor_p384::X.to_vec(),
            y: vektor_p384::Y.to_vec(),
            kurve: vektor_p384::KURVE.to_vec(),
        }
    }

    /// added 01.10.2026 22:5x UTC (Projektlabel 02.10.2026): 0x0503 mit echter P-384-Signatur wird
    /// angenommen. Das ist der Fall von `www.opensuse.org`.
    #[test]
    fn cv_ist_gueltig_p384() {
        let cv = CertificateVerify {
            algorithmus: 0x0503,
            signatur: vektor_p384::SIGNATUR.to_vec(),
        };
        assert!(
            cv_gueltig(&schluessel_p384(), &cv, crate::signatur::VORLAGE_SERVER),
            "eine echte P-384-Signatur muss gelten"
        );
    }

    /// added 01.10.2026 22:5x UTC (Projektlabel 02.10.2026): GEGENPROBEN zu `cv_ist_gueltig_p384`.
    ///
    /// Jede davon hat die Fassung vor dem 01.10.2026 22:5x UTC (Projektlabel 02.10.2026) **nicht**
    /// gefangen, weil der Arm fehlte und alles ablehnte — sie sichern
    /// jetzt die Grenzen des neuen Arms.
    #[test]
    fn cv_p384_gegenproben() {
        use crate::signatur::VORLAGE_SERVER;
        let gut = |algorithmus: u16| CertificateVerify {
            algorithmus,
            signatur: vektor_p384::SIGNATUR.to_vec(),
        };
        // 1) Das richtige Verfahren.
        assert!(cv_gueltig(&schluessel_p384(), &gut(0x0503), VORLAGE_SERVER));
        // 2) 0x0403 behauptet SHA-256 — die Signatur ist aber mit
        //    SHA-384 gerechnet und muss durchfallen.
        assert!(!cv_gueltig(
            &schluessel_p384(),
            &gut(0x0403),
            VORLAGE_SERVER
        ));
        // 3) 0x0805 (RSA) auf einem ECDSA-Schluessel.
        assert!(!cv_gueltig(
            &schluessel_p384(),
            &gut(0x0805),
            VORLAGE_SERVER
        ));
        // 4) Ein falsches Byte in `r`.
        let mut kaputt = vektor_p384::SIGNATUR.to_vec();
        kaputt[7] ^= 0x01;
        let cv = CertificateVerify {
            algorithmus: 0x0503,
            signatur: kaputt,
        };
        assert!(!cv_gueltig(&schluessel_p384(), &cv, VORLAGE_SERVER));
        // 5) Ein falsches Byte im **Nachrichteninhalt** (der Hash ist
        //    danach ein anderer).
        let anderer = b"TLS 1.3, server CertificateVerifX";
        assert!(!cv_gueltig(&schluessel_p384(), &gut(0x0503), anderer));
        // 6) Leere Signatur.
        let cv = CertificateVerify {
            algorithmus: 0x0503,
            signatur: Vec::new(),
        };
        assert!(!cv_gueltig(&schluessel_p384(), &cv, VORLAGE_SERVER));
    }

    /// added 01.10.2026 22:5x UTC (Projektlabel 02.10.2026): 0x0503 mit einem **P-256**-Schluessel
    /// muss abgelehnt werden — die Zahlen passen nicht zusammen.
    #[test]
    fn cv_0503_auf_p256_ist_abgelehnt() {
        let cv = CertificateVerify {
            algorithmus: 0x0503,
            signatur: vektor_p384::SIGNATUR.to_vec(),
        };
        let schluessel = SchluesselArt::Ecdsa {
            x: vektor_p384::X.to_vec(),
            y: vektor_p384::Y.to_vec(),
            kurve: crate::der::oids::SECP256R1.to_vec(),
        };
        assert!(!cv_gueltig(
            &schluessel,
            &cv,
            crate::signatur::VORLAGE_SERVER
        ));
    }

    /// GEGENPROBE an der Naht, die am 02.10.2026 gebrochen war: der
    /// Anwendungs-Lesepfad des Clients.
    ///
    /// DER FEHLER WAR EIN DOPPELTES EXPANDIEREN: `record::
    /// schluessel_unprotect` erwartet das **Verkehrsgeheimnis**, und
    /// `TlsStrom` haelt bereits den daraus expandierten Schluessel. Der Weg
    /// umging `record` und benutzt `e_schluessel`/`e_iv` direkt, genau wie
    /// `sende_verschluesselt` auf der Schreibseite.
    ///
    /// WARUM DIESER TEST DIE KLASSE VON FEHLERN FING, DIE KEINE
    /// GROESSENMESSUNG FING: alle fuenf Groessen stimmten (Suite,
    /// Sequenzzahl, Schluessel, IV, Geheimnis), die Bytes auf der Leitung
    /// auch. Dieser Test vergleicht nicht Groessen, sondern **Richtungen**:
    /// verschluesselt wird mit dem Geheimnis, entschluesselt mit dem daraus
    /// expandierten Schluessel. Der Test wird genau dann rot, wenn jemand den
    /// einen wieder mit dem anderen vertauscht — und dann sind alle
    /// Groessenmessungen weiterhin gruen.
    ///
    /// M6: der Sockel bekommt eine Zeitgrenze, und der Test laeuft ueber
    /// Loopback mit einem angenommenen Sockel, damit `verbinde_ohne_
    /// handshake` nicht blockiert.
    #[test]
    fn anwendungs_lesepfad_nimmt_das_expandierte_geheimnis() {
        use std::net::TcpListener;
        use std::sync::mpsc;

        let hoerer = TcpListener::bind("127.0.0.1:0").expect("Loopback-Listener");
        let port = hoerer.local_addr().expect("Adresse").port();
        let (fertig_tx, fertig_rx) = mpsc::channel::<()>();
        let annahme = std::thread::spawn(move || {
            let (_angenommen, _peer) = hoerer.accept().expect("Annahme");
            // Der Gegen-Sockel wird bewusst **nicht** benutzt: es geht um die
            // Rechnung, nicht um eine Verbindung.
            let _ = fertig_tx.send(());
        });
        let mut strom = TlsStrom::verbinde_ohne_handshake("127.0.0.1", port).expect("Verbindung");
        fertig_rx.recv().expect("Annahme-Signal");
        annahme.join().expect("Annahme-Thread");

        let suite = Suite::Aes128GcmSha256;
        let geheimnis: [u8; 32] = [7u8; 32];
        let (schluessel, iv) = suite
            .keying_material(&geheimnis)
            .expect("Schluessel rechenbar");
        strom.suite = suite;
        strom.e_schluessel = schluessel;
        strom.e_iv = iv;
        strom.ap_lese_seq = 0;

        let inhalt: &[u8] = b"NewSessionTicket-Inhalt";
        let typ = typen::HANDSHAKE;
        // Der Weg, den der **Server** geht (`server.rs`, Schritt 13).
        let chiffrat =
            crate::record::schluessel_protect(suite, &geheimnis, 0, typ, inhalt).expect("schutz");

        let (klartext, innerer) = strom
            .entschluessele_anwendung(0, Some(typ), &chiffrat)
            .expect("der richtige Schluessel muss oeffnen");
        assert_eq!(klartext, inhalt, "Klartext weicht ab");
        assert_eq!(innerer, typ, "innerer Typ weicht ab");

        // DIE GEGENPROBE, und sie ist der eigentliche Test: ein einziges
        // falsches Byte im Schluessel muss **rot** machen. Ohne sie wuerde
        // der Test oben auch dann gruen bleiben, wenn der Lesepfad
        // grundsaetzlich kaputt waere.
        let mut kaputt = schluessel;
        kaputt[0] ^= 0x01;
        strom.e_schluessel = kaputt;
        assert!(
            strom
                .entschluessele_anwendung(0, Some(typ), &chiffrat)
                .is_err(),
            "ein um ein Bit veraenderter Schluessel muss ablehnen"
        );

        // UND DIE ZWEITE GEGENPROBE, die Sequenzzahl: sie ist Teil der Nonce.
        strom.e_schluessel = schluessel;
        assert!(
            strom
                .entschluessele_anwendung(1, Some(typ), &chiffrat)
                .is_err(),
            "die falsche Sequenzzahl muss ablehnen"
        );

        // UND DIE DRITTE, der Typ: ein abweichender Inhaltstyp ist ein
        // Protokollfehler und muss als solcher kommen, nicht als
        // Authentifizierungsfehler.
        let fehler = strom
            .entschluessele_anwendung(0, Some(typen::ALERT), &chiffrat)
            .expect_err("ein falscher Inhaltstyp muss ablehnen");
        assert!(
            !matches!(fehler, StromFehler::AEAD { .. }),
            "der Typfehler kam als Kryptographiefehler: {fehler:?}"
        );
    }

    /// Der Zeitpunkt, an dem der `#[ignore]`-Test prueft, und warum
    /// er feststeht.
    ///
    /// DER GRUND: der Test unten braucht einen Zeitpunkt fuer die
    /// Gueltigkeitspruefung. `SystemTime::now()` waere hier der
    /// falsche Weg — der Test soll an einem festen Datum haengen und
    /// nicht daran, wie spaet es ist. Also steht die Zahl hier fest,
    /// und sie ist GEMESSEN:
    ///
    /// ```text
    /// date -u -d "2026-09-30 00:00:00" +%s   ->  1790726400
    /// date -u -d "2026-09-30 12:00:00" +%s   ->  1790769600
    /// ```
    ///
    /// Und sie wird hier gegen die Zeitrechnung aus `x509.rs`
    /// geprueft, damit sie nicht verrottet: dieselbe Zahl muss aus
    /// `tage_seit_1970` und aus `zeit_seit_1970` mit einem
    /// GeneralizedTime kommen.
    #[test]
    fn der_feste_zeitpunkt_stimmt() {
        let sekunden = 1_790_769_600i64;
        // Ueber den Weg, den auch die Zertifikate gehen.
        let als_generalisiert =
            crate::x509::zeit_seit_1970(0x18, b"20260930120000Z").expect("GeneralizedTime");
        assert_eq!(
            als_generalisiert, sekunden,
            "beide Wege muessen gleich sein"
        );
        // Und die Zwischenschritte, damit ein Fehler nicht nur als
        // "Zahlen verschieden" auffaellt.
        let tage = crate::x509::tage_seit_1970(2026, 9, 30);
        assert_eq!(
            tage * 86400 + 12 * 3600,
            sekunden,
            "30. September 2026, 12:00"
        );
        // Und zur Sicherheit der Anfang des Tages.
        assert_eq!(tage * 86400, 1_790_726_400, "der Tag began um null");
    }

    /// M7: die Nonce, gegen die Klartext aus RFC 8448 gerechnet.
    ///
    /// Die Nonce ist `iv XOR Sequenznummer`, und die Sequenznummer
    /// steht linksbuendig auf 12 Byte (RFC 8446 Abschnitt 5.3). Bei
    /// Sequenznummer 0 ist die Nonce also das `iv` unveraendert, und
    /// das ist ein Sonderfall, den man nicht pruefen sollte — der
    /// interessierende Fall ist die XOR-Kette ueber 8 Byte.
    #[test]
    fn nonce_ist_iv_xor_sequenznummer() {
        let iv = [0u8; 12];
        // (a) Sequenznummer 0: alles null.
        assert_eq!(nonce(&iv, 0), [0u8; 12], "bei seq = 0 bleibt die Nonce");
        // (b) Sequenznummer 1: nur das LETZTE Byte wird eins. Die
        //     Sequenzzahl steht linksbuendig auf 12 Byte, das
        //     niedrigste ihrer 8 Byte also an Index 11.
        //
        // EIGENER FEHLER IM TEST: ich hatte Byte 10 angesetzt und
        // damit 0x01 an der falschen Stelle erwartet. Die
        // Implementierung war richtig, die Behauptung nicht — und ein
        // solcher Test ist schlimmer als keiner, weil er eine
        // richtige Stelle als falsche markiert.
        let n1 = nonce(&iv, 1);
        assert_eq!(n1[11], 0x01, "seq = 1 steht an Byte 11");
        assert_eq!(&n1[..11], &[0u8; 11], "der Rest bleibt null");
        // (c) Ein gefuelltes `iv`: die Nullen des `iv` an der
        //     Sequenznummer-Stelle werden zu Einsen.
        let mut iv2 = [0u8; 12];
        iv2[11] = 0xFF;
        let n2 = nonce(&iv2, 1);
        assert_eq!(n2[11], 0xFE, "0xFF XOR 0x01 = 0xFE");
        // (d) Die Sequenzzahl laeuft ueber die vollen 64 Bit; das
        //     erste Byte wird erst bei 2^56 erreicht.
        let gross = nonce(&iv, 1u64 << 56);
        assert_eq!(gross[4], 0x01, "das oberste Bit steht an Byte 4");
        // (e) Und die Grenze: 2^64-1 ist der groesste Wert, der noch
        //     keinen Uebertrag in Byte 3 erzeugt.
        let max = nonce(&iv, u64::MAX);
        assert_eq!(max[4..12], [0xFFu8; 8], "alle acht Bytes sind eins");
        assert_eq!(max[3], 0x00, "und es laeuft kein Byte ueber");
    }

    /// M7: das Zerlegen von Handshake-Nachrichten, an den vier
    /// Byte-Grenzen, an denen es in der Praxis kaputtgeht.
    #[test]
    fn nachrichten_an_den_grenzen_zerlegen() {
        // (a) zwei vollstaendige Nachrichten
        let mut p = Vec::new();
        p.extend_from_slice(&[8, 0, 0, 2, 0xAA, 0xBB]);
        p.extend_from_slice(&[20, 0, 0, 4, 1, 2, 3, 4]);
        let teile = zerlege_nachrichten(&mut p).expect("zerlegt");
        assert_eq!(teile.len(), 2, "zwei Nachrichten");
        assert_eq!(
            teile[0],
            vec![8, 0, 0, 2, 0xAA, 0xBB],
            "EncryptedExtensions"
        );
        assert_eq!(teile[1], vec![20, 0, 0, 4, 1, 2, 3, 4], "Finished");
        assert!(p.is_empty(), "der Puffer ist leer");

        // (b) eine unvollstaendige Nachricht bleibt liegen
        let mut p = Vec::new();
        p.extend_from_slice(&[11, 0, 0, 8, 1, 2, 3]);
        let teile = zerlege_nachrichten(&mut p).expect("zerlegt");
        assert!(teile.is_empty(), "eine halbe Nachricht ergibt nichts");
        assert_eq!(p.len(), 7, "und bleibt liegen");
        // Jetzt kommt der Rest, und es wird genau eine Nachricht.
        p.extend_from_slice(&[4, 5, 6, 7, 8]);
        let teile = zerlege_nachrichten(&mut p).expect("zerlegt");
        assert_eq!(teile.len(), 1, "jetzt eine vollstaendige");
        assert_eq!(teile[0].len(), 12, "4 Byte Kopf + 8 Byte Inhalt");
        assert!(p.is_empty(), "und der Puffer ist leer");

        // (c) Die groesste 24-Bit-Laenge ist 0xFFFFFF, und die ist
        //     gerade NOCH erlaubt: `MAX_NACHRICHT` ist `(1<<24) - 1`.
        //
        //     EIGENER FEHLER IM TEST: ich hatte 0xFFFFFF als "zu lang"
        //     gesetzt. Es ist genau die Grenze. Die Ablehnung bei
        //     einer zu langen Nachricht kann man mit diesem Feld
        //     ohnehin nicht ausloesen — 24 Bit tragen keinen groesseren
        //     Wert. Der Schutz gegen eine zu grosse Nachricht ist die
        //     Puffergrenze `MAX_ANTWORT`, und der wird hier geprueft.
        let mut p = vec![11, 0x00, 0xFF, 0xFF];
        p.extend(std::iter::repeat_n(0u8, 0xFFFF));
        let teile = zerlege_nachrichten(&mut p).expect("erlaubt");
        assert_eq!(
            teile[0].len(),
            4 + 0xFFFF,
            "65539 Byte sind noch in Ordnung"
        );
        // Und die tatsaechliche Grenze: `MAX_NACHRICHT` selbst.
        assert_eq!(
            crate::record::MAX_NACHRICHT,
            0xFF_FFFF,
            "MAX_NACHRICHT ist die groesste 24-Bit-Laenge, nicht eine kleinere"
        );
    }

    /// M7: die Kettenpruefung ist nicht abschaltbar. Der Test kann
    /// das nicht beweisen — er kann nur festhalten, dass es keinen
    /// `bool` gibt, mit dem man sie umgehen koennte, und dass ein
    /// leerer Vertrauensspeicher scheitert.
    #[test]
    fn ohne_vertrauensspeicher_kein_strom() {
        // Ein echtes, selbstsigniertes Zertifikat aus den Testdaten.
        const KETTE: &str = include_str!("../testdaten/t12_kette.txt");
        let a = KETTE.find("---WURZEL---").expect("Wurzel");
        let b = KETTE[a..].find("---BLATT_KAPUTT---").expect("Ende") + a;
        let pem = KETTE[a + "---WURZEL---".len()..b].trim().to_string();
        // Der `Vec` wird gebunden, bevor daraus gelesen wird: ein
        // `[0]` direkt auf dem Ergebnis von `pem_zerlegen` waere ein
        // Verweis auf eine temporaere Datei, die sofort wegglaeuft.
        let alle = crate::kette::pem_zerlegen(&pem).expect("PEM liest sich");
        assert_eq!(alle.len(), 1, "genau ein Zertifikat im Abschnitt");
        let der = alle[0].clone();
        let z = crate::x509::zertifikat_lesen(&der).expect("X.509");
        let speicher = crate::kette::Vertrauensspeicher::default();
        let jetzt = (1_790_749_916i64 + 1_862_029_916i64) / 2;
        // M7: das Ergebnis ist ein Fehler, nicht `Ok`. WELCHER Fehler,
        // ist hier der Punkt: die Wurzel traegt `basicConstraints
        // cA = TRUE`, und ein Zertifikat, das sich als CA ausgibt,
        // wird gar nicht erst als Endzertifikat zugelassen
        // (RFC 5280 Abschnitt 4.2.1.9). Der Fehler ist also praeziser
        // als "kein Aussteller" — er sagt, WARUM.
        //
        // Ich hatte zuerst `KeinAussteller` erwartet. Das war die
        // Behauptung "wenn ich den Speicher wegnehme, scheitert es
        // dort" — sie ist richtig, aber sie war an der falschen
        // Stelle formuliert, und ein Test, der die falsche Variante
        // verlangt, faellt bei der naechsten Aenderung im Aufrufpfad
        // um. Beide Wege enden in einem Fehler, und genau das wird
        // hier festgemacht.
        assert_eq!(
            crate::kette::kette_pruefen(
                std::slice::from_ref(&z),
                &speicher,
                jetzt,
                "T12 Testwurzel"
            )
            .err(),
            Some(crate::kette::KetteFehler::EndzertifikatNichtErlaubt),
            "eine CA ist kein Endzertifikat"
        );
        // Die Reihenfolge der Pruefungen ist fest und wird hier
        // festgemacht: die CA-Pruefung kommt ZUERST, also meldet auch
        // ein fremder Hostname zuerst die CA. Das ist billiger (kein
        // Zeichenkettenvergleich) und der Fehler trifft die Ursache.
        //
        // EIGENER FEHLER IM TEST: ich hatte fuer einen fremden
        // Hostnamen `HostnamePasstNicht` erwartet. Der Code prueft
        // aber die CA-Bedingung vorher, also kommt die andere
        // Variante. Zweimoeglich falsche Erwartungen an EINER Stelle
        // — die erste habe ich geschrieben, die zweite ist hier
        // passiert, weil ich die Reihenfolge nicht gelesen hatte.
        assert_eq!(
            crate::kette::kette_pruefen(
                std::slice::from_ref(&z),
                &speicher,
                jetzt,
                "www.gibtesnicht.test"
            )
            .err(),
            Some(crate::kette::KetteFehler::EndzertifikatNichtErlaubt),
            "die CA-Pruefung kommt vor der Hostnamen-Pruefung"
        );
    }

    /// M3/M4: die benannten Grenzen sind benannt und nicht aus der
    /// Eingabe. Der Test haelt ihre Werte fest, damit eine
    /// unbeabsichtigte Aenderung auffaellt.
    #[test]
    fn die_grenzen_sind_fest_gehalten() {
        // M4: die Puffergrenze muss kleiner sein als 2^16, sonst
        // koennte ein Record mit 0xFFFF Byte angenommen werden.
        // added 02.10.2026 12:5x (Projektlabel 02.10.2026), Rolle TLS: **IM
        // CONST-BLOCK**, weil clippy `assertions_on_constants` zu Recht sagt, dass
        // ein Vergleich zweier `const` zur **Uebersetzungszeit** feststeht — im
        // Testrumpf waere er eine Behauptung, die der Optimierer wegwirft. Der
        // Block macht die Aussage wieder zu einer, die den Build stoppt.
        const { assert!(MAX_ANTWORT > crate::record::MAX_CIPHERTEXT) };
        const { assert!(crate::record::MAX_CIPHERTEXT < 0x1_0000) };
        // M3: der Flug braucht vier Records, es sind acht erlaubt.
        const { assert!(MAX_FLUG_RECORDS >= 4, "vier Records sind noetig") };
        // Und der Flug braucht Platz fuer vier Zertifikate a 4 KiB.
        const { assert!(MAX_FLUG >= 4 * crate::x509::MAX_ZERTIFIKAT) };
        const { assert!(MAX_ZERTIFIKATE_KETTE >= 2, "Blatt und Zwischenzertifikat") };
    }

    /// DER ECHTE TEST GEGEN example.com.
    ///
    /// `#[ignore]`, weil er vom Netz abhaengt. Er laeuft mit
    ///
    /// ```text
    /// werkzeuge/bau.sh narsil-tls test -- --ignored
    /// ```
    ///
    /// M7: die Erwartung wird festgemacht, nicht auf „irgendwas"
    /// gestellt. Geprueft wird:
    ///
    ///  1. Der Handshake geht durch, also Kette, Hostname, Signatur
    ///     und `finished` stimmen alle.
    ///  2. Das Serverzertifikat wurde gelesen und traegt den Namen.
    ///  3. Der HTTP-Get antwortet mit `HTTP/1.1 200`.
    ///  4. In der Antwort steht `example.com` — der Server hat die
    ///     Anfrage als die gelesen, die wir geschickt haben.
    ///
    /// Die Zeitgrenze ist `VERBINDUNGSZEIT` + `LESEZEIT`, und beide
    /// stehen in `strom.rs`. Der Test bricht darum nicht ewig.
    #[test]
    #[ignore = "braucht das Netz; example.com:443"]
    fn echter_handshake_gegen_example_com() {
        use std::io::Write as _;
        let bundle =
            std::fs::read_to_string("/etc/ssl/certs/ca-certificates.crt").expect("das CA-Bundle");
        let speicher =
            crate::kette::Vertrauensspeicher::aus_pem(&bundle).expect("das Bundle liest sich");

        // 2026-09-30 12:00:00 UTC, von `date -u -d "2026-09-30 12:00:00" +%s`
        // gemessen. Nicht `SystemTime::now()`: der Test soll an einem
        // festen Datum haengen, nicht an der Uhr.
        let jetzt = 1_790_762_400i64;

        let mut strom = TlsStrom::verbinde("example.com", 443, &[b"http/1.1"], &speicher, jetzt)
            .expect("der Handshake mit example.com:443 muss gehen");

        // M7: das Zertifikat wurde gelesen.
        assert!(!strom.blatt_der.is_empty(), "das Serverzertifikat ist da");
        let z = crate::x509::zertifikat_lesen(&strom.blatt_der).expect("X.509 lesbar");
        assert!(
            z.traegt_hostnamen("example.com"),
            "das Zertifikat traegt example.com; SAN: {:?}",
            z.san.as_ref().map(|s| s.dns.clone())
        );

        // Der HTTP-Get.
        let anfrage = "GET / HTTP/1.1\r\nHost: example.com\r\nUser-Agent: narsil-tls/0.1\r\n\
             Connection: close\r\nAccept: */*\r\n\r\n"
            .to_string();
        strom
            .write_all(anfrage.as_bytes())
            .expect("die Anfrage laesst sich schreiben");
        strom.flush().expect("und wegschicken");

        // M4: die Antwort wird begrenzt gelesen.
        let mut antwort = Vec::with_capacity(8192);
        let mut block = [0u8; 4096];
        // M3: die Schleife hat eine Obergrenze an ITERATIONEN, nicht
        // nur an Bytes. Sonst haengt ein Server, der endlos
        // Leerbytes schickt.
        for _ in 0..MAX_ANTWORT / 4096 {
            let n = strom.read(&mut block).expect("lesen");
            if n == 0 {
                break;
            }
            antwort.extend_from_slice(&block[..n]);
            if antwort.len() > MAX_ANTWORT {
                panic!("die Antwort ist laenger als {MAX_ANTWORT} Byte");
            }
            if antwort.windows(4).any(|w| w == b"\r\n\r\n") && antwort.len() > 1024 {
                break;
            }
        }
        assert!(!antwort.is_empty(), "es kam etwas zurueck");
        let text = String::from_utf8_lossy(&antwort);
        // M7: die Antwort beginnt mit `HTTP/1.1 200`.
        assert!(
            text.starts_with("HTTP/1.1 200"),
            "die Antwort beginnt mit {:?}",
            &text[..text.len().min(40)]
        );
        assert!(
            text.contains("Example Domain"),
            "und es ist die Beispielseite von example.com"
        );
        println!("  Antwort: {} Byte, Status {}", antwort.len(), &text[..15]);
    }
    /// M7: die DOWNGRD-Marke nach RFC 8446 Abschnitt 4.1.3.
    ///
    /// ZWEI FAELLE, und beide sind noetig: ein Server, der die Marke
    /// setzt, muss erkannt werden, und ein Server, der sie NICHT
    /// setzt, muss durchgelassen werden. Ein Test, der nur den ersten
    /// Fall prueft, waere mit der Variante "pruefe immer" erfuellt —
    /// und die Variante wuerde jeden TLS-1.2-Server ablehnen.
    #[test]
    fn die_downgrade_marke_wird_erkannt_und_ignoriert() {
        // 1) Die Marke an den LETZTEN acht Byte, wie in der Norm.
        let mut random = [0x11u8; 32];
        random[24..32].copy_from_slice(&DOWNGRD);
        assert!(downgrade_marke(&random), "M7: die Marke wird erkannt");
        // 2) Ein zufaelliges ServerRandom traegt sie nicht.
        let normal = [0x77u8; 32];
        assert!(!downgrade_marke(&normal), "M7: ein normales Random nicht");
        // 3) DIE KONTRAPROBE: die Marke an einer ANDEREN Stelle
        //    erkennt die Funktion nicht. Sonst wuerde sie einen
        //    Server verwerfen, der zufaellig die Buchstaben D,O,W,N,G,R,D
        //    an anderer Stelle hat — und der Server koennte sie
        //    absichtlich dorthin legen.
        let mut verschoben = [0x11u8; 32];
        verschoben[0..8].copy_from_slice(&DOWNGRD);
        assert!(
            !downgrade_marke(&verschoben),
            "M7: nur die LETZTEN acht Byte zaehlen"
        );
        // 4) Und ein einziges abweichendes Byte in den letzten acht
        //    hebt die Marke auf.
        for stelle in 0..8usize {
            let mut fast = [0x11u8; 32];
            fast[24..32].copy_from_slice(&DOWNGRD);
            fast[24 + stelle] ^= 0x01;
            assert!(
                !downgrade_marke(&fast),
                "M7: ein gekipptes Byte in Stelle {stelle} hebt die Marke auf"
            );
        }
    }

    /// M7: `DOWNGRD` ist die Konstante aus der Norm, nicht
    /// "irgendwas mit D".
    #[test]
    fn die_downgrade_konstante_ist_die_aus_der_norm() {
        assert_eq!(DOWNGRD.len(), 8);
        assert_eq!(&DOWNGRD[..6], b"DOWNGR", "M7: die ersten sechs Byte");
        assert_eq!(DOWNGRD[6], b'D', "M7: das siebte Byte");
        assert_eq!(DOWNGRD[7], 0x01, "M7: das achte Byte ist fest 0x01");
        // Und als Text: "DOWNGRD" + 0x01.
        assert_eq!(&DOWNGRD[..7], b"DOWNGRD");
    }

    /// M7: die gemeinsame Verbindungsfunktion nimmt EINEN ClientHello
    /// und macht genau einen Versuch.
    ///
    /// DER WEG ZUR PRUEFUNG: der Quelltext **ohne** das Testmodul. Ein
    /// Test, der sich selbst mitzaehlt, zaehlt seine eigenen
    /// Zeichenketten mit — das hat dieser Test zweimal getan (er fand
    /// erst drei und dann zwei Aufrufe, weil seine eigenen Literale
    /// das Muster enthielten). Deshalb wird ab `#[cfg(test)]` abgeschnitten
    /// und nur der ausfuehrende Teil gezaehlt.
    ///
    /// DER MASSSTAB: `hallo_tauschen` ist die einzige Funktion, die
    /// einen ClientHello schickt. Sie wird genau einmal aufgerufen, aus
    /// `ueber_sockel_mit`. Zwei Aufrufe wuerden zwei Verbindungsversuche
    /// bedeuten — genau das, was der Auftrag verbietet und was der
    /// Angriff aus RFC 8446 Abschnitt 4.1.1 ist: ein Mittelaenger
    /// schickt eine falsche ServerHello, der Client versucht es erneut
    /// mit einer aelteren Version, und der Angreifer hat gewonnen.
    #[test]
    fn es_gibt_keinen_zweiten_verbindungsversuch() {
        let quelle = include_str!("strom.rs");
        let ausfuehrend = quelle
            .split("#[cfg(test)]")
            .next()
            .expect("der Teil vor den Tests");
        let aufrufe = ausfuehrend.matches("hallo_tauschen(host, alpn)").count();
        assert_eq!(
            aufrufe, 1,
            "M7: genau ein Aufruf von `hallo_tauschen`, gefunden: {aufrufe}"
        );
        // Und die gemeinsame Funktion selbst hat keine Schleife: eine
        // Schleife waere der Rahmen fuer einen zweiten Versuch.
        //
        // DER SCHNITT, und er war bis 01.10.2026 (R4 Teil 2) **falsch**:
        //   `.split("pub fn ueber_sockel_mit").nth(1).split("pub fn set_nodelay")`
        // Das nimmt den Text **von** `ueber_sockel_mit_psk` **bis** zur
        // naechsten `pub fn set_nodelay` — und da steht inzwischen der
        // halbe Rest der Datei, samt neun `for `-Schleifen in voellig
        // fremden Funktionen (`lese_zertifikate`, `lese_nachschrift` …).
        // Der Test meldete daraufhin `M7: keine Schleife in der
        // gemeinsamen Funktion` und meinte **fremden** Code. Ein Test, der
        // an der Form des Quelltexts haengt statt an seinem Gegenstand,
        // meldet den falschen Fehler — das ist schlimmer als keiner,
        // weil er auf einen Fehler zeigt, den es nicht gibt.
        //
        // DER WEG: die Klammern mitzaehlen und den **Funktionsrumpf**
        // nehmen, nicht den Text bis zum naechsten `pub fn`. Eine
        // geschweifte Klammer in einem String wuerde das verhebeln; in
        // diesem Rumpf steht keine (die Langform steht in `ticket.rs`),
        // und der Test sagt das hier, damit man es beim Aendern prueft.
        let teil = funktionsrumpf(
            ausfuehrend,
            "pub fn ueber_sockel_mit_psk",
            "M7: die gemeinsame Funktion nicht gefunden",
        );
        assert!(
            !teil.contains("for "),
            "M7: keine Schleife in der gemeinsamen Funktion"
        );
        // Und sie ruft den TLS-1.2-Weg genau einmal auf, mit dem
        // Sockel, den sie selbst geoeffnet hat — kein `verbinde` dort,
        // denn das waere ein zweiter Sockel und damit ein zweiter
        // Versuch.
        assert!(
            !teil.contains("verbinde("),
            "M7: kein eigener Verbindungsaufbau im TLS-1.2-Zweig"
        );
    }

    /// Der Rumpf einer Funktion, anhand der Klammern gefunden.
    ///
    /// added 01.10.2026, R4 Teil 2. DER GRUND: `.split("pub fn …")` bis
    /// zur naechsten `pub fn` ist kein Funktionsrumpf, sondern "dieser
    /// Text plus der halbe Rest der Datei". Als eine Funktion zwischen zwei
    /// anderen eingefuegt wurde (das war `ueber_sockel_mit_psk`), hat ein
    /// Test dadurch neun `for `-Schleifen **fremder** Funktionen gesehen
    /// und `M7: keine Schleife` gemeldet. Der Schnitt muss am Gegenstand
    /// haengen, nicht an der Nachbarschaft.
    ///
    /// M3: die Klammern werden ueber `char_indices` in einem **begrenzten**
    /// Fenster gezaehlt. Das Fenster ist `MAX_RUMPF_BYTES` gross; ohne die
    /// Grenze wuerde ein Fehler (kein Anfang gefunden) den ganzen Rest
    /// durchsuchen und am Ende eine irritierende Meldung geben.
    fn funktionsrumpf<'a>(text: &'a str, kopf: &str, meldung: &str) -> &'a str {
        /// So weit wird gesucht, bevor der Test aufgibt. M3: benannte
        /// Grenze, weil `include_str!` den **ganzen** Quelltext hereingibt
        /// (ueber 2400 Zeilen) und ein Fehler dann sonst still durch die
        /// Datei liefe.
        const MAX_RUMPF_BYTES: usize = 4000;
        let start = text.find(kopf).unwrap_or_else(|| panic!("{meldung}"));
        // Der Rumpf beginnt mit dem ersten `{` nach der Signatur.
        let offen = text[start..]
            .find('{')
            .unwrap_or_else(|| panic!("{meldung}: keine oehnende Klammer"))
            + start;
        let mut tiefe = 0i32;
        for (i, zeichen) in text[offen..].char_indices() {
            if i > MAX_RUMPF_BYTES {
                panic!("{meldung}: der Rumpf ist laenger als {MAX_RUMPF_BYTES} Byte");
            }
            match zeichen {
                '{' => tiefe += 1,
                '}' => {
                    tiefe -= 1;
                    if tiefe == 0 {
                        return &text[offen..offen + i + 1];
                    }
                }
                _ => {}
            }
        }
        panic!("{meldung}: der Rumpf wurde nie geschlossen");
    }

    /// DER VERTRAUENSSPEICHER aus dem System.
    fn sys_speicher() -> Vertrauensspeicher {
        let pems = std::fs::read_to_string("/etc/ssl/certs/ca-certificates.crt")
            .expect("das CA-Bundle des Systems");
        Vertrauensspeicher::aus_pem(&pems).expect("das CA-Bundle lesbar")
    }

    fn jetzt() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("Systemzeit")
            .as_secs() as i64
    }

    /// M3: so viel Antwort wird gelesen.
    const MAX_ANTWORT: usize = 64 * 1024;
    /// M3: so viele Lesevorgaenge.
    const MAX_LESEKOMBBENEN: usize = 16;

    fn hole(s: &mut TlsVerbindung, host: &str) -> String {
        use std::io::Write as _;
        s.write_all(
            format!("GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .expect("Anfrage schreiben");
        let mut antwort: Vec<u8> = Vec::with_capacity(MAX_ANTWORT);
        let mut block = [0u8; 4096];
        for _ in 0..MAX_LESEKOMBBENEN {
            match s.read(&mut block) {
                Ok(0) => break,
                Ok(n) => {
                    antwort.extend_from_slice(&block[..n]);
                    if antwort.len() >= MAX_ANTWORT {
                        break;
                    }
                }
                Err(e) => panic!("lesen: {e}"),
            }
        }
        String::from_utf8_lossy(&antwort).into_owned()
    }

    /// **Die eine Verbindungsfunktion gegen `example.com`**, das TLS 1.3
    /// spricht.
    ///
    /// WAS DER TEST HALTEN SOLL: dass die gemeinsame Funktion den
    /// TLS-1.3-Pfad nimmt und die Seite liefert. Ohne diesen Test koennte
    /// die Funktion auch immer TLS 1.2 waehlen und niemand saehe es.
    #[test]
    #[ignore = "braucht das Netz; example.com"]
    fn die_verbindungsfunktion_nimmt_tls13_gegen_example_com() {
        let speicher = sys_speicher();
        let mut v = TlsVerbindung::verbinde("example.com", 443, &[b"http/1.1"], &speicher, jetzt())
            .expect("der Handshake muss gehen");
        assert_eq!(
            v.version(),
            crate::nachricht::VERSION_TLS13,
            "M7: example.com antwortet mit TLS 1.3"
        );
        let text = hole(&mut v, "example.com");
        assert!(
            text.starts_with("HTTP/1.1 200"),
            "Antwort: {:?}",
            &text[..text.len().min(60)]
        );
        assert!(
            text.contains("Example Domain"),
            "M7: der Body nennt Example Domain; er ist:\n{}",
            &text[..text.len().min(400)]
        );
    }

    /// **Die eine Verbindungsfunktion gegen `badssl.com`**, das TLS 1.2
    /// waehlt.
    ///
    /// DER PUNKT DES GANZEN AUFTRAGS: derselbe Aufruf wie oben, ein
    /// anderes Ergebnis. example.com nimmt 1.3, badssl.com nimmt 1.2 —
    /// und es wird in BEIDEN Faellen nur EIN ClientHello gesendet.
    #[test]
    #[ignore = "braucht das Netz; badssl.com"]
    fn die_verbindungsfunktion_nimmt_tls12_gegen_badssl() {
        let speicher = sys_speicher();
        let mut v = TlsVerbindung::verbinde("badssl.com", 443, &[b"http/1.1"], &speicher, jetzt())
            .expect("der Handshake muss gehen");
        assert_eq!(
            v.version(),
            crate::nachricht::VERSION_LEGACY,
            "M7: badssl.com antwortet mit TLS 1.2"
        );
        // Und die Ausnahme von W5B steht hier fest: kein Extended
        // Master Secret, weil der Server nicht darauf antwortet.
        assert_eq!(
            v.erweitertes_geheimnis(),
            Some(false),
            "M7: der Server bestaetigt kein Extended Master Secret"
        );
        let text = hole(&mut v, "badssl.com");
        assert!(
            text.starts_with("HTTP/1.1 200"),
            "Antwort: {:?}",
            &text[..text.len().min(60)]
        );
        assert!(
            text.contains("<title>badssl.com</title>"),
            "M7: der Body nennt badssl.com; er ist:\n{}",
            &text[..text.len().min(400)]
        );
    }

    /// **Der Pfad zu `tls-v1-2.badssl.com:1012`**, der auf TLS 1.2
    /// festgenagelt ist.
    ///
    /// DER GRUND FUER DEN DRITTEN TEST: `badssl.com` und
    /// `tls-v1-2.badssl.com` koennten aus unterschiedlichen Gruenden
    /// unterschiedlich ausgehen. Gemessen am 30.09.2026 mit
    /// `openssl s_client -tls1_2`: badssl.com waehlt
    /// ECDHE-RSA-AES128-GCM-SHA256, tls-v1-2.badssl.com:1012
    /// ECDHE-RSA-AES256-GCM-SHA384. Zwei verschiedene Suites, also zwei
    /// verschiedene Wege durch den 1.2-Code.
    #[test]
    #[ignore = "braucht das Netz; tls-v1-2.badssl.com:1012"]
    fn die_verbindungsfunktion_nimmt_tls12_gegen_tls_v1_2_badssl() {
        let speicher = sys_speicher();
        let mut v = TlsVerbindung::verbinde(
            "tls-v1-2.badssl.com",
            1012,
            &[b"http/1.1"],
            &speicher,
            jetzt(),
        )
        .expect("der Handshake muss gehen");
        assert_eq!(v.version(), crate::nachricht::VERSION_LEGACY, "M7: TLS 1.2");
        let text = hole(&mut v, "tls-v1-2.badssl.com");
        assert!(
            text.starts_with("HTTP/1.1 200"),
            "Antwort: {:?}",
            &text[..text.len().min(60)]
        );
        assert!(
            text.contains("<title>tls-v1-2.badssl.com</title>"),
            "M7: der Body nennt tls-v1-2.badssl.com; er ist:\n{}",
            &text[..text.len().min(400)]
        );
    }

    /// M7: die Downgrade-Meldeung sagt, worum es geht, und die
    /// Versionsmeldung nennt die Zahl.
    ///
    /// Beides wird von `narsil-net` als Text in die Fehlerspalte des
    /// Browsers geschrieben; ein Benutzer, der "Downgrade-Angriff" liest,
    /// weiss, dass sein Netz etwas tut, und einer, der "Version 0x0399"
    /// liest, weiss, dass der Server kaputt ist.
    #[test]
    fn die_meldungen_der_verzweigung_sagen_was_passiert() {
        let t = format!("{}", StromFehler::Downgrade);
        assert!(
            t.contains("Downgrade"),
            "M7: das Stichwort steht im Text: {t}"
        );
        assert!(t.contains("8446"), "M7: und die Norm: {t}");
        let t = format!("{}", StromFehler::VersionUnerwartet { ist: 0x0399 });
        assert!(t.contains("0399"), "M7: die Zahl steht im Text: {t}");
        let t = format!(
            "{}",
            StromFehler::Tls12(Box::new(
                crate::strom12::Strom12Fehler::KeinExtendedMasterSecret,
            ))
        );
        assert!(
            t.contains("Extended Master Secret"),
            "M7: der 1.2-Fehler bleibt lesbar: {t}"
        );
    }
    /// Der HelloRetryRequest- und P-256-Vektor aus **RFC 8448
    /// Abschnitt 5**.
    ///
    /// Gelesen, nicht abgetippt:
    /// `testdaten/t13_hrr_rfc8448.txt`, erzeugt von
    /// `mutation_md/tls/tls13_hrr_generator.py` aus `rfc8448.txt`.
    /// Der Generator prueft jede Laenge gegen die Angabe, die der RFC
    /// selbst druckt.
    const V: &str = include_str!("../testdaten/t13_hrr_rfc8448.txt");

    fn feld(name: &str) -> Vec<u8> {
        for zeile in V.lines() {
            if zeile.starts_with('#') {
                continue;
            }
            if let Some(rest) = zeile.strip_prefix(&format!("{name}=")) {
                return crate::sha2::von_hex(rest.trim()).expect("Hex in der Vektordatei");
            }
        }
        panic!("Feld {name} fehlt in der Vektordatei");
    }

    /// M7: die ServerHello aus RFC 8448 Abschnitt 5 ist ein
    /// HelloRetryRequest, und zwar erkennbar am **Sonderwert** des
    /// ServerRandom (RFC 8446 Abschnitt 4.1.3) — nicht an einem Cookie.
    #[test]
    fn der_hrr_aus_rfc8448_wird_erkannt() {
        let hrr = feld("HRR");
        let sh = crate::nachricht::ServerHello::dekodiere(&hrr).expect("der HRR dekodiert");
        assert!(sh.hrr, "M7: der Sonderwert des ServerRandom steckt drin");
        assert_eq!(sh.version, crate::nachricht::VERSION_TLS13, "M7: TLS 1.3");
        // Die verlangte Gruppe: P-256 (0x0017).
        assert_eq!(
            sh.gruppe,
            nachricht::GRUPPE_SECP256R1,
            "M7: der Server will P-256"
        );
        // Und der Sonderwert steht byteweise im Vektor.
        assert_eq!(
            &hrr[6..38],
            &crate::nachricht::HRR_SONDERRANDOM[..],
            "M7: die 32 Byte"
        );
    }

    /// M7: das **echte** ServerHello nach dem HRR ist kein HRR.
    ///
    /// DER FALL, DER UNSICHTBAR SCHLECHT GEHT: erkennt man "HelloRetry-
    /// Request" an irgendeinem Merkmal, das auch ein normales
    /// ServerHello haben kann, laeuft der Handshake in eine Schleife
    /// oder verlangt ein zweites ClientHello, das der Server nicht
    /// braucht. Der Unterschied ist genau der Sonderwert.
    #[test]
    fn das_zweite_serverhello_ist_kein_hrr() {
        let sh = crate::nachricht::ServerHello::dekodiere(&feld("SH2")).expect("dekodiert");
        assert!(!sh.hrr, "M7: das zweite ServerHello ist ein echtes");
        assert_eq!(sh.gruppe, nachricht::GRUPPE_SECP256R1, "M7: P-256, 65 Byte");
        assert_eq!(sh.key_share.len(), 65, "M7: uncompressed 0x04 || x || y");
        assert_eq!(sh.key_share[0], 0x04, "M7: das Praefix");
        // Und seine 65 Byte sind der oeffentliche Schluessel des
        // Servers aus derselben Spur.
        assert_eq!(sh.key_share, feld("SERVER_P256_PUB"), "M7: derselbe Punkt");
    }

    /// M7: **P-256 im TLS-1.3-Weg** — das gemeinsame Geheimnis gegen den
    /// Vektor.
    ///
    /// DER WEG ZUM BEWEIS: der Client hat den privaten P-256-Schluessel
    /// `P256_PRIV`, der Server den oeffentlichen Punkt `SERVER_P256_PUB`
    /// (beides aus derselben RFC-Spur). Ihr Produkt ist das Geheimnis,
    /// aus dem die Handshake-Schluessel kommen. Stimmt es nicht, ist
    /// alles Weitere — Transkript, Schluesselplan, AEAD — gegenstandslos.
    ///
    /// DIE GEGENPROBE: `P256_PUB` ist der oeffentliche Punkt zu
    /// `P256_PRIV`. Beides steht im RFC, und die Uebereinstimmung
    /// beweist, dass die private Seite die richtige ist: ein Byte daneben
    /// gaebe einen anderen Punkt.
    #[test]
    fn p256_im_tls13_weg_ergibt_das_geheimnis_des_vektors() {
        let mut privat = [0u8; 32];
        privat.copy_from_slice(&feld("P256_PRIV"));
        // 1) Der oeffentliche Punkt des Clients.
        assert_eq!(
            crate::ecdsa::oeffentlich_p256(&privat)
                .expect("oeffentlich")
                .to_vec(),
            feld("P256_PUB"),
            "M7: g*d stimmt mit dem Vektor ueberein"
        );
        // 2) Das gemeinsame Geheimnis mit dem oeffentlichen Punkt des
        //    Servers.
        let q = feld("SERVER_P256_PUB");
        assert_eq!(q.len(), 65, "M7: der Serverpunkt ist 65 Byte");
        let k = crate::ecdsa::kurve_p256().expect("P-256");
        let z = crate::ecdsa::ecdh(&privat, &q[1..33], &q[33..65], &k).expect("ECDH");
        assert_eq!(
            z.len(),
            32,
            "M7: das Geheimnis ist die x-Koordinate, 32 Byte"
        );
        // Und es ist nicht etwa der x-Wert des Serverpunkts selbst —
        // das waere der Fehler, den ein "ich benutze die Koordinate
        // direkt" macht.
        assert_ne!(z.as_slice(), &q[1..33], "M7: nicht die Server-x-Koordinate");
        // 3) GEGEN DEN SERVER, DER ES SELBST GERECHNET HAT. Der IKM
        //    aus `extract secret "handshake"` (Z264 des Abschnitts 5)
        //    ist genau dieses Geheimnis: der Server hat mit
        //    SERVER_P256_PRIV und P256_PUB gerechnet. Trifft mein
        //    Wert ihn nicht, ist nicht die Testform falsch, sondern
        //    die Rechnung.
        assert_eq!(
            z,
            feld("GEHEIMNIS"),
            "M7: das Geheimnis ist das, was der Server in RFC 8448 Abschnitt 5 gerechnet hat"
        );
    }

    /// M7: die **Gegenprobe** — ein Byte daneben im privaten Schluessel
    /// muss rot machen.
    ///
    /// DER SINN DIESES TESTS: `p256_im_tls13_weg_ergibt_das_geheimnis_des_vektors`
    /// koennte auch dann gruen sein, wenn `ecdh` konstant denselben
    /// Wert zurueckgaebe — etwa, wenn es den zweiten Parameter
    /// ignorierte und stattdessen den privaten Schlüssel quadrierte.
    /// Dann waere auch der Vergleich in diesem Test gruen, weil beide
    /// Seiten denselben Fehler machten.
    ///
    /// DESWEGEN DIESER TEST: er aendert **nur** ein Byte des privaten
    /// Schluessels und verlangt ein anderes Geheimnis. Kommt dasselbe
    /// heraus, rechnet die Funktion nicht mit dem Privatwert.
    #[test]
    fn p256_ignoriert_den_privatschluessel_nicht() {
        let privat = feld("P256_PRIV");
        let q = feld("SERVER_P256_PUB");
        let k = crate::ecdsa::kurve_p256().expect("P-256");
        let richtig = crate::ecdsa::ecdh(&privat, &q[1..33], &q[33..65], &k).expect("ECDH");
        // Das letzte Bit umdrehen.
        let mut daneben = privat.clone();
        daneben[31] ^= 1;
        assert_ne!(
            privat, daneben,
            "M7: die Testsetzung aendert wirklich ein Byte"
        );
        let falsch = crate::ecdsa::ecdh(&daneben, &q[1..33], &q[33..65], &k).expect("ECDH");
        assert_ne!(
            richtig, falsch,
            "M7: ein anderes Byte im Privatwert muss ein anderes Geheimnis geben"
        );
        // Und derselbe Fehler am oeffentlichen Punkt des Servers.
        let mut q2 = q.clone();
        q2[40] ^= 1;
        let falsch2 = crate::ecdsa::ecdh(&privat, &q2[1..33], &q2[33..65], &k);
        match falsch2 {
            // Ein Punkt, der nicht auf der Kurve liegt, wird abgewiesen —
            // das ist richtig und kein Fehler des Tests.
            Err(_) => {}
            Ok(v) => assert_ne!(v, richtig, "M7: ein anderes Byte im Serverpunkt"),
        }
    }

    /// M7: der HRR-Ablauf aendert den Transkript — und zwar so, wie
    /// RFC 8446 Abschnitt 4.1.1 es vorschreibt.
    ///
    /// WAS HIER **NICHT** BEHAUPTET WIRD, und warum: RFC 8448
    /// Abschnitt 5 druckt zwar einen `hash` (Z275/Z290, beide
    /// `8aa8e828…`), aber dieser Hash gehoert zu einem spaeteren Punkt
    /// der Spur — er kommt in `derive secret "tls13 c hs traffic"`
    /// vor, also **nach** dem `EncryptedExtensions`. Eine Suche ueber
    /// alle 2048 Zusammenstellungen der gedruckten Nachrichten dieser
    /// Spur (ClientHello 1, HRR, ClientHello 2, ServerHello,
    /// EncryptedExtensions, Certificate, CertificateVerify, Finished,
    /// jeweils als Klartext, als `message_hash` und weggelassen) hat
    /// keinen Treffer geliefert. Solange nicht geklaert ist, welche
    /// Nachrichten der RFC an dieser Stelle im Transkript hat, wird
    /// hier **kein** Byte-fuer-Byte-Vergleich behauptet — ein
    /// Sollwert, von dem man nicht weiss, was er ist, taugt als Test
    /// nicht, auch nicht als "scheinbar gruener".
    ///
    /// GEPRUEFT WIRD STATTDESSEN DIE REGEL, und zwar in beiden Richtungen:
    /// der HRR-Transkript ist ein anderer als der ohne HRR, und der mit
    /// `message_hash` ist ein anderer als der mit Klartext.
    #[test]
    fn der_hrr_transkript_unterscheidet_sich_vom_ohne() {
        let ch1 = feld("CH1");
        let hrr = feld("HRR");
        let ch2 = feld("CH2");
        let sh2 = feld("SH2");
        // Der Weg, den die Crate geht (RFC 8446 Abschnitt 4.1.1).
        let mut mit = Sha256::neu();
        mit.aufnehmen(&message_hash(&ch1));
        mit.aufnehmen(&hrr);
        mit.aufnehmen(&message_hash(&ch2));
        mit.aufnehmen(&sh2);
        // 1) Ohne HRR, mit message_hash — das waere der Fehler.
        let mut ohne_hrr = Sha256::neu();
        ohne_hrr.aufnehmen(&message_hash(&ch1));
        ohne_hrr.aufnehmen(&message_hash(&ch2));
        ohne_hrr.aufnehmen(&sh2);
        assert_ne!(
            mit.fertig(),
            ohne_hrr.fertig(),
            "M7: der HRR gehoert in den Transkript"
        );
        // 2) Der HRR steht **selbst** drin, nicht sein Hash.
        let mut hr_gehasht = Sha256::neu();
        hr_gehasht.aufnehmen(&message_hash(&ch1));
        hr_gehasht.aufnehmen(&message_hash(&hrr));
        hr_gehasht.aufnehmen(&message_hash(&ch2));
        hr_gehasht.aufnehmen(&sh2);
        assert_ne!(
            mit.fertig(),
            hr_gehasht.fertig(),
            "M7: der HRR steht im Klartext im Transkript"
        );
        // 3) Beide ClientHellos im Klartext — der Fehler, den man ohne
        //    die Norm macht.
        let mut klartext = Sha256::neu();
        klartext.aufnehmen(&ch1);
        klartext.aufnehmen(&hrr);
        klartext.aufnehmen(&ch2);
        klartext.aufnehmen(&sh2);
        assert_ne!(
            mit.fertig(),
            klartext.fertig(),
            "M7: die ClientHellos gehoeren als message_hash hinein"
        );
        // 4) Und die Regel ist stabil: dieselbe Eingabe, gleiches
        //    Ergebnis. Sonst waere "unterscheidet sich" bedeutungslos.
        let mut nochmal = Sha256::neu();
        nochmal.aufnehmen(&message_hash(&ch1));
        nochmal.aufnehmen(&hrr);
        nochmal.aufnehmen(&message_hash(&ch2));
        nochmal.aufnehmen(&sh2);
        assert_eq!(mit.fertig(), nochmal.fertig(), "M7: reproduzierbar");
    }

    /// M7: die Cookie-Erweiterung wird **nicht** gebaut, wenn es keine
    /// gibt, und ein leeres Cookie wird verworfen.
    ///
    /// DER FALL: `HrrOhneCookie` entsteht genau hier. Ein leeres Cookie
    /// waere nach RFC 8446 Abschnitt 4.2.2 unzulaessig (`cookie<1..2^16-1>`)
    /// und wuerde als "Server hat eins geschickt" gezaehlt.
    #[test]
    fn ohne_cookie_keine_cookie_erweiterung() {
        let mut h = crate::nachricht::ClientHello::dekodiere(&feld("CH1")).expect("CH1");
        assert!(h.cookie.is_none(), "M7: der erste ClientHello hat keins");
        let b = h.kodiere();
        assert!(
            !b.windows(2).any(|w| w == [0x00, 0x2c]),
            "M7: keine cookie-Erweiterung ohne Cookie"
        );
        // Und mit einem leeren Cookie wird die Erweiterung verworfen.
        h = h.wiederholung(nachricht::GRUPPE_X25519, vec![0x11; 32], &[]);
        let b = h.kodiere();
        assert!(
            !b.windows(2).any(|w| w == [0x00, 0x2c]),
            "M7: ein leeres Cookie wird nicht gebaut"
        );
    }
}
