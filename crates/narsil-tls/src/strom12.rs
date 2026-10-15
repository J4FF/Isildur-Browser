//! TLS 1.2 — der Zustandsautomat (RFC 5246 Abschnitt 7).
//!
//! added 30.09.2026, als letzter Teil von T15.
//!
//! # Die Reihenfolge, und sie ist nicht verhandelbar
//!
//! ```text
//! C -> ServerHello, Certificate, ServerKeyExchange, ServerHelloDone
//! S -> ClientKeyExchange, [CertificateVerify], ChangeCipherSpec, Finished
//! S -> ChangeCipherSpec, Finished
//! ```
//!
//! Drei Stellen, an denen eine Verwechslung **stumm** bleibt:
//!
//! 1. **Der `ChangeCipherSpec` ist ein eigener Record** mit Inhalt
//!    `01` und **kein** Handshake-Typ. Er ist keine Nachricht und geht
//!    nicht in den Transkript.
//! 2. **Der Client schickt sein `finished` unter den neuen
//!    Anwendungs-Schluesseln**, das Server-`finished` ebenfalls. Wer den
//!    Cipher beim Senden vergisst, schickt Klartext, und die Gegenseite
//!    antwortet mit `handshake_failure`.
//! 3. **Der Extended Master Secret braucht den ClientKeyExchange im
//!    Hash** (RFC 7627 Abschnitt 3.2). Ohne ihn stimmen beide Seiten
//!    nicht ueberein, und der Fehler erscheint erst beim Vergleich der
//!    `finished`.
//!
//! # Was hier nicht gebaut ist und warum
//!
//! * Kein CBC (RFC 5246 Abschnitt 6.2.3.3, RFC 7457) — der Auftrag
//!   schliesst es aus, und es ist der Grund fuer Lucky13 und POODLE.
//! * Kein RSA-Schluesseltausch — der Auftrag schliesst ihn aus, und er
//!   hat kein Forward Secrecy.
//! * Keine Sitzungswiederaufnahme, kein Client-Zertifikat, keine
//!   Neuverhandlung. Der Auftrag nennt sie nicht, und jede davon ist
//!   eine eigene Fehlerquelle, die man nur mit eigenem Test findet.

use crate::kette::{KetteFehler, Vertrauensspeicher};
use crate::nachricht12::{self, Nachricht12Fehler, ServerHello, ServerKeyExchange};
use crate::rec12::{self, Aead, Rec12Fehler};
use crate::tls12::{self, Hash as T12Hash, Tls12Fehler};
use crate::x509::{SchluesselArt, Zertifikat};
use std::fmt;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// M6: die Zeitgrenzen. Ohne sie haengt ein Server, der antwortet und
/// dann schweigt, den ganzen Tab.
pub const VERBINDUNGSZEIT: Duration = Duration::from_secs(10);
pub const LESEZEIT: Duration = Duration::from_secs(20);
pub const SCHREIBZEIT: Duration = Duration::from_secs(20);

/// M3: die Grenzen fuer die eine Verbindung.
pub const MAX_KETTE: usize = 8;
pub const MAX_SOCKEL: usize = 64 * 1024;

/// Die Suites, die dieser Auftrag anbietet.
///
/// REIHENFOLGE IST BEDEUTUNGSLOS — TLS 1.2 kennt keine
/// Serverpräferenz. Was zählt: **alle** ECDHE-Suiten mit einer AEAD, die
/// diese Crate rechnen kann. `TLS_DHE_*` fehlt, weil `signatur.rs` kein
/// Diffie-Hellman rechnet.
pub fn suites() -> Vec<u16> {
    vec![
        // ECDHE_RSA mit AES-GCM: die, die badssl.com und die meisten
        // Server zuerst nennen.
        0xC02F, // TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256
        0xC030, // TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384
        0xCCA8, // TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256
        // ECDHE_ECDSA, fuer Server mit ECDSA-Zertifikat.
        0xC02B, // TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256
        0xC02C, // TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384
        0xCCA9, // TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256
    ]
}

/// Die Signaturverfahren, die wir anbieten.
pub fn sigverfahren() -> Vec<u16> {
    vec![
        nachricht12::sigverfahren::ECDSA_SECP256R1_SHA256,
        nachricht12::sigverfahren::ECDSA_SECP384R1_SHA384,
        nachricht12::sigverfahren::RSA_PSS_RSAE_SHA256,
        nachricht12::sigverfahren::RSA_PSS_RSAE_SHA384,
        // rsa_pss_rsae_sha512 (0x0806) und rsa_pkcs1_sha384/sha512
        // (0x0501, 0x0601) stehen hier NICHT: `sha2.rs` rechnet nur
        // SHA-256 und SHA-384, es gibt keine `sha512`. Ein Verfahren
        // anzubieten, das man nicht pruefen kann, heisst: der Server
        // darf es waehlen, und der Client kann die Antwort nicht
        // gegenpruefen. Lieber nicht anbieten — und die Suite
        // TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384 (0xC030) bleibt
        // moeglich, weil sie SHA-384 zum Hash des Transkripts nimmt,
        // nicht zum Signaturverfahren.
        nachricht12::sigverfahren::RSA_PKCS1_SHA256,
    ]
}

/// Die Gruppen, die wir anbieten: x25519 zuerst, dann P-256.
pub fn gruppen() -> Vec<u16> {
    vec![nachricht12::gruppe::X25519, nachricht12::gruppe::SECP256R1]
}

/// Was zu einer Suite gehoert.
fn suite(code: u16) -> Option<(Aead, T12Hash)> {
    match code {
        0xC02F => Some((Aead::Aes128Gcm, T12Hash::Sha256)),
        0xC030 => Some((Aead::Aes256Gcm, T12Hash::Sha384)),
        // 0xCCA8 TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256
        // 0xCCA9 TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256
        0xCCA8 | 0xCCA9 => Some((Aead::ChaCha20Poly1305, T12Hash::Sha256)),
        // 0xC02B TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256
        0xC02B => Some((Aead::Aes128Gcm, T12Hash::Sha256)),
        // DER FEHLER, DEN DIESE TABELLE HATTE: 0xC02C stand hier als
        // ChaCha20-Poly1305. 0xC02C ist aber
        // TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384 (AES-256-GCM,
        // SHA-384) — ein Schluesselblock von 2*32 + 2*4 = 72 Byte
        // statt 2*32 + 2*12 = 88. Faellt die Verhandlung auf diese
        // Suite, sind Schluessel und IV ab Position 32 um 16 Byte
        // verschoben und der erste Record des Servers schlaegt fehl.
        //
        // 0xC02A = TLS_ECDHE_ECDSA_WITH_AES_128_CBC_SHA256 und
        // 0xC024 = ..._AES_128_CBC_SHA sind die CBC-Suiten. Sie werden
        // hier NICHT angeboten, auch nicht in der Liste: der Auftrag
        // schliesst CBC aus.
        0xC02C => Some((Aead::Aes256Gcm, T12Hash::Sha384)),
        _ => None,
    }
}

/// M3: die Obergrenze fuer die Kette. 8 Zertifikate sind reichlich;
/// jeder Server schickt hoechstens drei.
pub const MAX_ZERTIFIKATE_KETTE: usize = 8;

/// Fehler des TLS-1.2-Stroms.
///
/// Die Kettenfehler sind EIGENE Varianten und nicht ein Text: der
/// Auftrag verlangt, dass `expired`, `wrong.host` und `self-signed` an
/// der Serveradresse jeweils einen **benannten** Kettenfehler liefern.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Strom12Fehler {
    /// Keine Verbindung, kein Handshake.
    KeineVerbindung(String),
    /// Fehler im Netz.
    Netz(String),
    /// Die Gegenseite hat gewartet und dann geschlossen.
    VerbindungZu,
    /// Der Handshake ist an einer Stelle gescheitert.
    Handshake(String),
    /// Die Kette. `KetteFehler` kommt aus `kette.rs` und ist
    /// unveraendert derselbe Fehler, den auch TLS 1.3 benutzt.
    Kette(KetteFehler),
    /// Die Nachricht war kaputt.
    Nachricht(Nachricht12Fehler),
    /// Die Recordschicht.
    Record(Rec12Fehler),
    /// Die Rechenwege aus `tls12.rs`.
    Rechnen(Tls12Fehler),
    /// Der Server hat den `finished` nicht bestaetigt.
    FinishedFalsch,
    /// Der Server hat sein `finished` nicht bestaetigt, aber das eigene
    /// schon — das heisst: er kann uns nicht lesen, und der Fehler ist
    /// bei ihm, nicht bei uns.
    PeerFinishedFalsch,
    /// Kein Schluesseltausch moeglich: der Server wollte keine Gruppe,
    /// die wir rechnen koennen.
    GruppeNichtUnterstuetzt { code: u16 },
    /// Die Suite des Servers rechnet diese Crate nicht.
    SuiteNichtUnterstuetzt { code: u16 },
    /// Ohne Extended Master Secret wird nicht verhandelt (RFC 7627
    /// Abschnitt 4).
    KeinExtendedMasterSecret,
    /// Ein Fehler an einer benannten Station des Handshakes.
    ///
    /// Der Stationsname steht vorn, weil er das Wichtigste ist: die
    /// Meldung soll sagen, WO es passiert ist, bevor sie sagt, was.
    Station(&'static str, Box<Strom12Fehler>),
}

impl fmt::Display for Strom12Fehler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Strom12Fehler::KeineVerbindung(s) => write!(f, "keine Verbindung: {s}"),
            Strom12Fehler::Netz(s) => write!(f, "Netzfehler: {s}"),
            Strom12Fehler::VerbindungZu => write!(f, "die Gegenseite hat geschlossen"),
            Strom12Fehler::Handshake(s) => write!(f, "Handshake: {s}"),
            // Die Kette ist der wichtigste Fehler: er sagt dem Benutzer,
            // warum die Seite nicht geladen wird. Er wird hier
            // durchgereicht, nicht umformuliert.
            Strom12Fehler::Kette(k) => write!(f, "{k}"),
            Strom12Fehler::Nachricht(n) => write!(f, "Nachricht: {n}"),
            Strom12Fehler::Record(r) => write!(f, "Record: {r}"),
            Strom12Fehler::Rechnen(r) => write!(f, "Rechnen: {r}"),
            Strom12Fehler::FinishedFalsch => write!(f, "das Server-finished stimmt nicht"),
            Strom12Fehler::PeerFinishedFalsch => {
                write!(f, "der Server konnte unser finished nicht lesen")
            }
            Strom12Fehler::GruppeNichtUnterstuetzt { code } => {
                write!(f, "Gruppe {code} nicht unterstuetzt")
            }
            Strom12Fehler::SuiteNichtUnterstuetzt { code } => {
                write!(f, "Suite 0x{code:04x} nicht unterstuetzt")
            }
            Strom12Fehler::KeinExtendedMasterSecret => {
                write!(f, "der Server bietet kein Extended Master Secret")
            }
            Strom12Fehler::Station(name, innen) => write!(f, "{name}: {innen}"),
        }
    }
}

impl From<KetteFehler> for Strom12Fehler {
    fn from(k: KetteFehler) -> Self {
        Strom12Fehler::Kette(k)
    }
}
impl From<Nachricht12Fehler> for Strom12Fehler {
    fn from(n: Nachricht12Fehler) -> Self {
        Strom12Fehler::Nachricht(n)
    }
}
impl From<Rec12Fehler> for Strom12Fehler {
    fn from(r: Rec12Fehler) -> Self {
        Strom12Fehler::Record(r)
    }
}
impl From<Tls12Fehler> for Strom12Fehler {
    fn from(t: Tls12Fehler) -> Self {
        Strom12Fehler::Rechnen(t)
    }
}

/// Der TLS-1.2-Strom: ein `Read + Write` wie jeder andere.
pub struct Tls12Strom {
    pub(crate) socket: TcpStream,
    aead: Aead,
    hash: T12Hash,
    // Schreibeseite
    s_schluessel: Vec<u8>,
    s_iv: Vec<u8>,
    s_seq: u64,
    // Leseseite
    e_schluessel: Vec<u8>,
    e_iv: Vec<u8>,
    e_seq: u64,
    /// added 02.10.2026 23:fx UTC (Projektlabel 02.10.2026), S4. Die Stufe
    /// des stapelten Widerrufsstatus: `KEINE`, `GUT` oder `UNBEKANNT`.
    ///
    /// added 02.10.2026 23:fx: EIN FELD STATT ZWEI. Die Fassung davor hatte
    /// `ocsp_unbekannt: bool` und `ocsp_grund: &str` — und damit zwei
    ///moegliche Wahrheiten ueber denselben Sachverhalt („kam nichts" und „kam
    ///etwas und es war gut" waren im Bool-Feld nicht unterscheidbar, ausser
    ///dass `ocsp_unbekannt == false` auch beides bedeutet).
    ///
    /// DER ZUGRIFF, und er ist eine eigene Methode: `ocsp_urteil()`.
    pub(crate) ocsp_stufe: &'static str,
    /// added 02.10.2026 23:fx, S4. Der Grund, wenn `ocsp_stufe` `UNBEKANNT`
    /// ist. Leer sonst.
    pub(crate) ocsp_grund: &'static str,
    /// added 02.10.2026 23:ax, S4. Die Nutzlast der `CertificateStatus`-
    /// Nachricht, **unveraendert** — nur bis Schritt 5b.
    ///
    /// WARUM SIE HIER LIEGT und nicht sofort ausgewertet wird: die Antwort
    /// laesst sich nur beurteilen, wenn das Blatt und sein Aussteller gelesen
    /// sind, und beides steht erst nach Schritt 5 fest. Eine Auswertung im
    /// Nachrichtenleser muesste die Zertifikate ein zweites Mal lesen.
    ///
    /// M4: die Laenge ist durch `nachricht12::zertifikatsstatus_lesen` gegen
    /// `MAX_OCSP_BYTES` geprueft, bevor die Bytes hierher kommen.
    pub(crate) ocsp_nutzlast: Option<Vec<u8>>,
    /// added 03.10.2026 16:4x, R60. **DIE KETTE ALS DER**, Blatt zuerst — und
    /// das Feld ist **hier** genauso noetig wie in `strom.rs`, weil
    /// `TlsVerbindung::kette()` ueber **beide** Varianten geht. Ohne es waere
    /// der Getter fuer TLS 1.2 **immer leer** — und still leer, also genau die
    /// Fehlerform aus R50/R51.
    pub kette_der: Vec<Vec<u8>>,
    /// added 01.10.2026 22:4x UTC (Projektlabel 02.10.2026), S4. Hat der    /// added 01.10.2026 22:4x UTC (Projektlabel 02.10.2026), S4. Hat der
    /// Server die Erweiterung `status_request` (5) **im ServerHello
    /// bestaetigt**?
    ///
    /// added 02.10.2026 23:ax: die Normstelle, woertlich aus RFC 6066 8:
    /// „If a server returns a `CertificateStatus` message, then the server
    /// MUST have included an extension of type `status_request` with empty
    /// `extension_data` in the extended server hello."
    pub(crate) status_bestaetigt: bool,
    /// Schluesselung aktiv? Bis zum ersten `ChangeCipherSpec` des Servers
    /// laeuft alles im Klartext.
    s_aktiv: bool,
    e_aktiv: bool,
    /// Die vom Server **gewählte** ALPN-Kennung, oder `None`.
    ///
    /// added 01.10.2026 09:2x UTC (Projektlabel 02.10.2026), R4x. DIESES
    /// FELD FEHLTE, und es ist dieselbe Lücke wie in `TlsStrom`, nur
    /// schlimmer: `TlsStrom` hatte das Feld und nur keinen Accessor,
    /// `Tls12Strom` hatte **gar nichts** — es schreibt die ALPN-Erweiterung
    /// (Zeile 352 nennt sie) und liest die Antwort des Servers **nie**.
    ///
    /// GEMESSEN (`grep -n "0x0010|Extension" strom12.rs` ohne Treffer in
    /// Codezeilen): die Kennung geht nach dem Handshake verloren, und
    /// `TlsVerbindung::alpn()` hätte für `Tls12` nichts zu liefern.
    ///
    /// WARUM DAS EIN FEHLER IST, obwohl RFC 7301 für 1.2 nichts Neues
    /// sagt: die Kennung wird **angeboten** (also an den Server
    /// geschickt) und dann **nicht gelesen**. Ein Browser, der `h2` und
    /// `http/1.1` anbietet und nach TLS 1.2 fragt, kann nicht wissen,
    /// welchen der beiden der Server gewählt hat — und redet dann
    /// HTTP/1.1 über eine Verbindung, die der Server für `h2` hält.
    pub alpn: Option<Vec<u8>>,
    /// Klartext aus gelesenen Records.
    klattr: Vec<u8>,
    /// Puffer fuer einen Recordkopf.
    roh: Vec<u8>,
    gemeldet: bool,
    /// Der Rest eines Handshake-Records, der zwischen zwei Records
    /// zerrissen wurde. `nachrichten_aus` nimmt ihn zu sich und gibt
    /// nur die vollstaendigen Nachrichten zurueck.
    restpuffer: Vec<u8>,
    /// Kam das `ChangeCipherSpec` des Servers schon?
    ///
    /// `lese_abschluss` braucht das, um "kein ChangeCipherSpec" von
    /// "ChangeCipherSpec, dann nichts mehr" zu unterscheiden: im
    /// zweiten Fall endet die Schleife nach `MAX_FLUG_RECORDS`
    /// Records, und beide Faelle wuerden denselben Text melden.
    ccs_gelesen: bool,
    /// Der geprueffte Hostname.
    pub host: String,
    /// Das Serverzertifikat, im DER.
    pub blatt_der: Vec<u8>,
    /// Die Suite, die der Server gewählt hat.
    pub gewaehlte_suite: u16,
    /// Wurde das **Extended Master Secret** nach RFC 7627 benutzt?
    ///
    /// Der Auftrag verlangt es, und `badssl.com` bietet es nicht (siehe
    /// `verbinde_mit`). Dieses Feld sagt, was wirklich lief — es wird
    /// nicht gesetzt, nur um die Meldung schoener zu machen.
    pub erweitertes_geheimnis: bool,
}

/// Was die gemeinsame Verbindungsfunktion uebergibt.
///
/// added 30.09.2026. DER GRUND: als drei einzelne Argumente waren
/// es acht insgesamt, und clippy meldete "too many arguments (8/7)".
/// Mehr noch: `ch` und `flug` sind beide `&[u8]`-artig, und ein
/// Vertauschen an der Aufrufstelle waere beim Lesen nicht
/// aufgefallen — der Transkript waere dann aus dem falschen
/// ClientHello gebaut und der Server haette mit einem Alert
/// geantwortet.
pub(crate) struct Eingang12<'a> {
    /// Die **gesendeten** ClientHello-Bytes.
    pub ch: &'a [u8],
    /// Der gelesene Flug, ServerHello zuerst.
    pub flug: &'a [Vec<u8>],
    /// Der private Wert, mit dem der Schluessel gebaut wurde.
    pub privat: [u8; 32],
}

/// added 02.10.2026 23:fx UTC (Projektlabel 02.10.2026), S4. Die drei
/// Stufen, die ein stapelter Widerrufsstatus haben kann.
///
/// added 02.10.2026 23:fx: als benannte Konstanten und nicht als Zahlen, weil
/// die Zahl in einer Fehlermeldung nichts bedeutet und in einem Vergleich
/// alles.
pub mod ocsp_stufe {
    /// Der Server hat gar keine stapelte Antwort geschickt.
    pub const KEINE: &str = "keine OCSP-Antwort";
    /// Die Antwort sagt `good`, **und** CertID, Zeitfenster und Signatur
    /// wurden nachgerechnet.
    pub const GUT: &str = "gut";
    /// Eine Antwort kam, aber etwas daran ist nicht in Ordnung oder nicht
    /// nachrechenbar. Der Grund steht in `Tls12Strom::ocsp_grund`.
    pub const UNBEKANNT: &str = "unbekannt";
}

impl Tls12Strom {
    /// Wie steht der stapelte Widerrufsstatus? `(Stufe, Grund)`.
    ///
    /// added 02.10.2026 23:fx UTC (Projektlabel 02.10.2026), S4.
    ///
    /// added 02.10.2026 23:fx: WARUM EINE EIGENE METHODE und nicht ein
    /// oeffentliches Feld: die Felder `ocsp_stufe` und `ocsp_grund` sind
    /// `pub(crate)`, weil sie nur der Handshake selbst setzt. Wer nachsehen
    /// will, was aus der stapelten Antwort geworden ist, geht hierhin — und
    /// bekommt beides in einem Aufruf, statt zweier Felder im Kopf zu haben.
    ///
    /// DIE DREI STUFEN, als Worte und nicht als Zahlen:
    /// * `ocsp_stufe::KEINE` — der Server hat nichts gestapelt;
    /// * `ocsp_stufe::GUT` — `good`, und CertID, Zeitfenster und Signatur
    ///   wurden nachgerechnet (der einzige Fall, in dem die Aussage belegt ist);
    /// * `ocsp_stufe::UNBEKANNT` — eine Antwort kam, aber der Grund sagt, was
    ///   fehlt. Leerer Grund heisst: es kam nichts.
    pub fn ocsp_urteil(&self) -> (&'static str, &'static str) {
        (self.ocsp_stufe, self.ocsp_grund)
    }

    /// Verbindet, macht den Handshake und liefert einen fertigen Strom.
    ///
    /// `jetzt` sind Sekunden seit 1970 und wandern an die
    /// Kettenpruefung, damit ein Test ein Datum setzen kann.
    pub fn verbinde(
        host: &str,
        port: u16,
        alpn: &[&[u8]],
        speicher: &Vertrauensspeicher,
        jetzt: i64,
    ) -> Result<Tls12Strom, Strom12Fehler> {
        Self::verbinde_mit(host, port, alpn, speicher, jetzt, false)
    }

    // added 04.10.2026 00:3x, R67. **`verbinde_ueber` GELOESCHT**, und das ist
    // der **einzige** der vier, der eine **Faehigkeit** versprach: „Name fuer SNI
    // und Pruefung, Adresse zum Verbinden“. Diese Faehigkeit haben **zwei**
    // Wege: `TlsVerbindung::verbinde_mit` (selbst aufloesen) und
    // `TlsVerbindung::ueber_sockel` (Sockel schon da) — GEMESSEN, beide werden
    // benutzt (narsil-net/src/lib.rs:2199/2207). `verbinde_ueber` war ein
    // **dritter** Weg in dieselbe Sache, und `grep` ueber alle Crates fand
    // **einen** Treffer: die Definition.
    //
    // added 04.10.2026 00:3x: **UND WARUM LOESCHEN STATT DOKUMENTIEREN:** ein
    // zusaetzlicher oeffentlicher Einstieg in eine **Handshake**-Implementierung
    // ist eine **ungeprueffte** Flaeche — R66s Lehre gilt auch hier: nicht
    // benutzte Faehigkeit in einer Krypto-Bibliothek ist **Ballast mit
    // Verzugsauswirkung**, und die Begruendung fuer Ballast steht in keinem
    // Kommentar, sondern in der Abwesenheit des Aufrufers.

    /// Wie `verbinde`, aber mit der Entscheidung ueber RFC 7627.
    ///
    /// DER WIDERSPRUCH, und er ist gemessen, nicht vermutet:
    /// der Auftrag verlangt einerseits das Extended Master Secret
    /// (RFC 7627) und andererseits `badssl.com` und
    /// `tls-v1-2.badssl.com:1012` als Gegner. Beides zusammen geht
    /// nicht: diese Server beantworten einen ClientHello mit
    /// `extended_master_secret` **nicht**.
    ///
    /// Gemessen am 30.09.2026 um 13:31 UTC. `text`, nicht ohne
    /// Angabe: ein leerer ```-Block ist ein **Rust**-Doctest, und die
    /// erste Zeile `$ openssl ...` scheitert daran mit "expected
    /// item, found `$`". Das hat der erste `bau.sh narsil-tls test`
    /// nach diesem Text gemeldet.
    /// ```text
    /// $ openssl s_client -connect badssl.com:443 -tls1_2 | grep -i extended
    ///     Extended master secret: no
    /// $ openssl s_client -connect tls-v1-2.badssl.com:1012 -tls1_2 | grep -i extended
    ///     Extended master secret: no
    /// ```
    /// Und die Erweiterung fehlt auch im ServerHello, im Roh:
    /// `005a`-Bytes ServerHello mit `server_name`, `renegotiation_info`,
    /// `ec_point_formats`, `session_ticket`, `alpn` — ohne 0x0017.
    ///
    /// RFC 7627 Abschnitt 4 sagt fuer diesen Fall ausdruecklich
    /// "abort". Das ist `verlange_ems = true`.
    ///
    /// `verlange_ems = false` nimmt den klassischen master secret
    /// NUR, wenn der Server die Erweiterung nicht zurueckschickt, und
    /// nur, wenn der Server das eigene `finished` danach bestaetigt.
    /// Diese Bedingung ist nicht theoretisch: kommt der Server mit
    /// dem `finished` nicht durch, bricht der Handshake ab — ein
    /// Angreifer, der die Erweiterung entfernt, erhaelt damit keine
    /// verbindung, sondern nur einen abgelehnten Handshake. Die
    /// Sicherheit dieser Verbindung entspricht damit der eines
    // TLS-1.2-Clients ohne RFC 7627 und ist nicht schlechter.
    ///
    /// `erweitertes_geheimnis` sagt danach, was tatsaechlich lief.
    pub fn verbinde_mit(
        host: &str,
        port: u16,
        alpn: &[&[u8]],
        speicher: &Vertrauensspeicher,
        jetzt: i64,
        verlange_ems: bool,
    ) -> Result<Tls12Strom, Strom12Fehler> {
        let mut s = Self::verbinde_ohne_handshake(host, port)?;
        s.handshake(host, alpn, speicher, jetzt, verlange_ems)?;
        Ok(s)
    }

    /// Uebernimmt einen BEREITS VERBUNDENEN Sockel.
    ///
    /// added 30.09.2026 fuer die gemeinsame Verbindungsfunktion: dort
    /// wird genau einmal verbunden, der ClientHello nennt beide
    /// Protokollversionen, und erst der ServerHello entscheidet. Der
    /// TLS-1.2-Zweig darf deshalb KEINEN zweiten Sockel oeffnen —
    /// das waere ein zweiter Verbindungsversuch, und der Auftrag
    /// untersagt genau das.
    ///
    /// M6: die Zeitgrenzen werden wie in `verbinde_ohne_handshake`
    /// gesetzt, aber nicht kleiner als die, die der Aufrufer schon
    /// gesetzt hat (`TcpStream` gibt sie hier nicht zurueck, also
    /// gelten die gesetzten).
    pub(crate) fn ueber_sockel(socket: TcpStream, host: &str) -> Tls12Strom {
        Tls12Strom {
            socket,
            aead: Aead::Aes128Gcm,
            hash: T12Hash::Sha256,
            s_schluessel: Vec::new(),
            s_iv: Vec::new(),
            s_seq: 0,
            e_schluessel: Vec::new(),
            e_iv: Vec::new(),
            e_seq: 0,
            ocsp_stufe: crate::strom12::ocsp_stufe::KEINE,
            ocsp_grund: "",
            ocsp_nutzlast: None,
            kette_der: Vec::new(),
            status_bestaetigt: false,
            s_aktiv: false,
            e_aktiv: false,
            alpn: None,
            klattr: Vec::new(),
            roh: Vec::new(),
            gemeldet: false,
            restpuffer: Vec::new(),
            ccs_gelesen: false,
            gewaehlte_suite: 0,
            erweitertes_geheimnis: false,
            host: host.to_string(),
            blatt_der: Vec::new(),
        }
    }

    /// Nur die TCP-Verbindung, ohne TLS.    /// Nur die TCP-Verbindung, ohne TLS.
    pub fn verbinde_ohne_handshake(host: &str, port: u16) -> Result<Tls12Strom, Strom12Fehler> {
        use std::net::ToSocketAddrs;
        let adresse = (host, port)
            .to_socket_addrs()
            .map_err(|e| Strom12Fehler::KeineVerbindung(e.to_string()))?
            .next()
            .ok_or_else(|| Strom12Fehler::KeineVerbindung("keine Adresse".into()))?;
        let socket = TcpStream::connect_timeout(&adresse, VERBINDUNGSZEIT)
            .map_err(|e| Strom12Fehler::KeineVerbindung(e.to_string()))?;
        // M6: beide Richtungen bekommen eine Zeitgrenze.
        socket
            .set_read_timeout(Some(LESEZEIT))
            .map_err(|e| Strom12Fehler::Netz(e.to_string()))?;
        socket
            .set_write_timeout(Some(SCHREIBZEIT))
            .map_err(|e| Strom12Fehler::Netz(e.to_string()))?;
        Ok(Tls12Strom {
            socket,
            aead: Aead::Aes128Gcm,
            hash: T12Hash::Sha256,
            s_schluessel: Vec::new(),
            s_iv: Vec::new(),
            s_seq: 0,
            e_schluessel: Vec::new(),
            e_iv: Vec::new(),
            ocsp_stufe: crate::strom12::ocsp_stufe::KEINE,
            ocsp_grund: "",
            ocsp_nutzlast: None,
            kette_der: Vec::new(),
            status_bestaetigt: false,
            e_seq: 0,
            s_aktiv: false,
            e_aktiv: false,
            alpn: None,
            klattr: Vec::new(),
            roh: Vec::new(),
            gemeldet: false,
            restpuffer: Vec::new(),
            ccs_gelesen: false,
            gewaehlte_suite: 0,
            erweitertes_geheimnis: false,
            host: host.to_string(),
            blatt_der: Vec::new(),
        })
    }

    pub fn set_nodelay(&mut self, wert: bool) -> std::io::Result<()> {
        self.socket.set_nodelay(wert)
    }
    /// Die aktuelle Lesefrist, oder `None`. Siehe `TlsStrom::lesefrist`.
    pub fn lesefrist(&self) -> Option<Duration> {
        self.socket.read_timeout().ok().flatten()
    }

    pub fn set_read_timeout(&mut self, d: Option<Duration>) -> std::io::Result<()> {
        self.socket.set_read_timeout(d)
    }
    pub fn set_write_timeout(&mut self, d: Option<Duration>) -> std::io::Result<()> {
        self.socket.set_write_timeout(d)
    }
    pub fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        self.socket.local_addr()
    }
    pub fn peer_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        self.socket.peer_addr()
    }

    /// Schliesst mit `close_notify`, dann mit dem Sockel.
    pub fn beende(&mut self, wie: std::net::Shutdown) -> std::io::Result<()> {
        let _ = self.close_notify();
        self.socket.shutdown(wie)
    }

    /// `close_notify`: Alert, Stufe `warning`, Beschreibung `close_notify`.
    pub fn close_notify(&mut self) -> Result<(), Strom12Fehler> {
        if self.gemeldet {
            return Ok(());
        }
        self.gemeldet = true;
        self.sende(rec12::typen::ALERT, &[1, 0])
    }

    // -----------------------------------------------------------------
    // Der Handshake
    // -----------------------------------------------------------------

    fn handshake(
        &mut self,
        host: &str,
        alpn: &[&[u8]],
        speicher: &Vertrauensspeicher,
        jetzt: i64,
        verlange_ems: bool,
    ) -> Result<(), Strom12Fehler> {
        // 1) Zufall. 32 Byte client_random plus der private Schluessel.
        let mut random = [0u8; 32];
        random.copy_from_slice(
            &crate::zufall::bytes(32)
                .map_err(|e| Strom12Fehler::Handshake(format!("Zufall: {e}")))?,
        );
        // Der private Schluessel wird NICHT hier erzeugt, sondern nach
        // dem `ServerKeyExchange`: die Gruppe entscheidet der Server,
        // und der oeffentliche Teil des `ClientKeyExchange` MUSS zu
        // dieser Gruppe gehoeren.
        //
        // DER FEHLER, DEN DIESER ABSCHNITT HATTE: es wurden beide
        // Schluessel erzeugt und im `ClientKeyExchange` immer der
        // x25519-oeffentliche geschickt. Waehlte der Server
        // secp256r1 — badssl.com tut das, Gruppe 23 —, dann standen
        // 32 Byte im Feld, wo 65 Byte (0x04 || x || y) hingehoeren.
        // Der Server schloss die Verbindung ohne Alert, und die
        // Meldung war "die Gegenseite hat geschlossen".
        //
        // Gefunden wurde das nicht durch Lesen, sondern durch den
        // Mitschnitt `mutation_md/tls/lauf05_server.bin` vom
        // 30.09.2026 12:04 UTC: im `ServerKeyExchange` stand
        //   curve_type 3, named_curve 23, Punkt 65 Byte
        // und im `ClientKeyExchange` des eigenen Stroms 33 Byte.
        //
        // M4: 32 Byte Zufall, fest. Der Aufrufer gibt nichts vor.
        let zufaellig: [u8; 32] = crate::zufall::bytes(32)
            .map_err(|e| Strom12Fehler::Handshake(format!("Zufall: {e}")))?
            .try_into()
            .map_err(|_| Strom12Fehler::Handshake("Zufall lieferte nicht 32 Byte".into()))?;
        let ch = nachricht12::ClientHello::baue(
            &random,
            host,
            &gruppen(),
            &suites(),
            alpn,
            &sigverfahren(),
        )?;
        // DER TRANSCRIPT BEGINNT MIT DEN GESENDETEN BYTES. Er wird im
        // aufgerufenen `handshake_ab_serverhello` angelegt, dort mit
        // `ch` gefuellt — nicht vorher und nicht danach, sonst laege
        // der ClientHello zweimal im Hash oder gar nicht.
        self.sende(rec12::typen::HANDSHAKE, &ch)?;

        // 2) Der Flug des Servers: SH, Certificate, SKE, SHD.
        let flug = self
            .lese_flug()
            .map_err(|e| Strom12Fehler::Station("Server-Flug", Box::new(e)))?;
        self.handshake_ab_serverhello(
            host,
            speicher,
            jetzt,
            verlange_ems,
            &Eingang12 {
                ch: &ch,
                flug: &flug,
                privat: zufaellig,
            },
        )
    }

    /// Der Teil des Handshakes ab dem **bereits gelesenen** ServerHello.
    ///
    /// added 30.09.2026 fuer die gemeinsame Verbindungsfunktion, die
    /// TLS 1.3 und TLS 1.2 in EINEM ClientHello anbietet. Dort sind
    /// ClientHello und ServerHello schon auf der Leitung, bevor
    /// feststeht, welcher Pfad geht: der Server waehlt, und erst dann
    /// darf der Rest des Handshakes laufen.
    ///
    /// `ch` sind die Bytes des ClientHello, **wie sie gesendet
    /// wurden** — der Transkript rechnet mit ihnen, nicht mit einem
    /// neu gebauten ClientHello, denn ein neu gebauter koennte an
    /// einem Byte (Reihenfolge einer Erweiterung) abweichen und der
    /// Hash waere dann falsch, ohne dass jemand es saehe.
    ///
    /// `flug` ist der gelesene Flug. Ist der ServerHello darin schon
    /// enthalten, wird er nicht erneut gelesen.
    pub(crate) fn handshake_ab_serverhello(
        &mut self,
        host: &str,
        speicher: &Vertrauensspeicher,
        jetzt: i64,
        verlange_ems: bool,
        eingang: &Eingang12,
    ) -> Result<(), Strom12Fehler> {
        let Eingang12 { ch, flug, privat } = eingang;
        // Das `client_random` wird AUS dem gesendeten ClientHello
        // gelesen, nicht als Parameter entgegengenommen.
        //
        // DER GRUND: es geht in den master secret ein
        // (`PRF(premaster, "master secret", client_random ||
        // server_random)`), und ein Aufrufer, der einen anderen Wert
        // uebergibt als den gesendeten, produziert einen Handshake, der
        // beim Server nicht ankommt — mit der Meldung "der Server
        // konnte unser finished nicht lesen". Aus dem Nachrichtenbyte
        // zu lesen heisst: die Zahl im Hash ist die Zahl auf der
        // Leitung, und das kann nicht auseinanderlaufen.
        //
        // Der Aufbau ist fest (RFC 5246 Abschnitt 7.4.1.2):
        //   1 Byte Typ, 3 Byte Laenge, 2 Byte legacy_version,
        //   32 Byte random, ...
        // M3/M4: die Laenge wird VOR dem Zugriff geprueft.
        if ch.len() < 38 {
            return Err(Strom12Fehler::Handshake(format!(
                "ClientHello zu kurz fuer das client_random: {} statt mindestens 38 Byte",
                ch.len()
            )));
        }
        let mut random = [0u8; 32];
        random.copy_from_slice(&ch[6..38]);
        let nachrichten = flug.to_vec();
        // Der Transkript beginnt mit dem GESENDETEN ClientHello.
        let mut transkript: Vec<u8> = Vec::with_capacity(4096);
        transkript.extend_from_slice(ch);
        let mut sh: Option<ServerHello> = None;
        let mut ske: Option<ServerKeyExchange> = None;
        let mut zertifikate: Vec<Vec<u8>> = Vec::new();
        let mut fertig = false;
        for m in nachrichten.iter() {
            if m.is_empty() {
                continue;
            }
            match m[0] {
                nachricht12::typen::SERVER_HELLO => {
                    let h = nachricht12::server_hello_dekodiere(m)?;
                    transkript.extend_from_slice(m);
                    // added 01.10.2026 09:3x UTC (Projektlabel
                    // 02.10.2026), R4x. DIE ALPN-ANTWORT LESEN, und sie
                    // wurde nie gelesen: `Tls12Strom` bot `alpn` an (die
                    // Erweiterung 0x0010 steht in der Liste) und
                    // vergab danach die Antwort des Servers.
                    //
                    // RFC 7301 Abschnitt 3.2: der Server antwortet mit
                    // **derselben** Erweiterung 0x0010 und einer Liste von
                    // je zwei Byte langen Kennungen; er wählt **eine**.
                    // Ohne dieses Lesen weiss der Client nicht, welche —
                    // und redet nach einem TLS-1.2-Handshake HTTP/1.1
                    // ueber eine Verbindung, die der Server fuer `h2`
                    // haelt.
                    // added 01.10.2026 22:4x UTC (Projektlabel 02.10.2026), S4. DIE BESTAETIGUNG LESEN.
                    // `h.hole(5)` liefert `Some(&[])`, wenn der Server die
                    // Erweiterung ohne Daten zurueckschickt — genau das ist
                    // die Form aus RFC 6066 8 („with empty
                    // `extension_data`"). Kommt sie mit Inhalt, ist das
                    // nicht die Form, die die Regel vorschreibt, und wir
                    // merken uns nur: bestaetigt, aber mit Daten.
                    if h.hole(nachricht12::erweiterung::STATUS_REQUEST).is_some() {
                        self.status_bestaetigt = true;
                    }
                    if let Some(inhalt) = h.hole(0x0010) {
                        // DER WEG: jede Kennung ist `Laenge (2 Byte) +
                        // Kennung`. Es muss **mindestens eine** geben,
                        // sonst ist die Erweiterung nach RFC 7301 ungueltig.
                        if !inhalt.is_empty() && inhalt.len() >= 2 {
                            // M4: MINDESTENS EIN PAAR GENUEGT. Wir
                            // nehmen das erste, weil der Server nach
                            // RFC 7301 genau eines waehlt und weitere
                            // Eintraege nur Kompatibilitaet sind. Die
                            // Grenze ist die Laenge der Erweiterung
                            // selbst, und die ist durch den Record begrenzt.
                            let laenge = u16::from_be_bytes([inhalt[0], inhalt[1]]) as usize;
                            if laenge > 0 && laenge <= inhalt.len() - 2 {
                                self.alpn = Some(inhalt[2..2 + laenge].to_vec());
                            }
                        }
                    }
                    sh = Some(h);
                }
                nachricht12::typen::CERTIFICATE => {
                    // Das Lesen der Kette liegt in `nachricht12.rs`,
                    // nicht hier: es ist eine Eigenschaft des
                    // Nachrichtenformats und damit ohne Sockel
                    // pruefbar (siehe `tests/bomben.rs`).
                    for z in nachricht12::zertifikatliste_lesen(m)? {
                        zertifikate.push(z);
                    }
                    transkript.extend_from_slice(m);
                }
                nachricht12::typen::CERTIFICATE_STATUS => {
                    // added 01.10.2026 22:2x UTC (Projektlabel 02.10.2026), S4.
                    // **Optional nach `Certificate`** (RFC 6066 8) — und das
                    // war die erste Regression: `xkcd.com`, `www.netbsd.org`
                    // und `www.opensuse.org` brachten am 02.10.2026 „Nachricht
                    // 14 erwartet, 22 bekommen".
                    //
                    // DIE NACHRICHT GEHOERT IN DEN TRANSKRIPT. RFC 6066 1.1
                    // woertlich: „any messages associated with these
                    // extensions that are sent during the TLS handshake
                    // **MUST** be included in the hash calculations involved
                    // in \"Finished\" messages." Die Fassung von 22:2x
                    // schrieb sie ausdruecklich **nicht** hinein — damit
                    // bekam der Server ``Alert [2, 14]`` (GEMESSEN 22:2x).
                    transkript.extend_from_slice(m);
                    //
                    // added 02.10.2026 23:a x, S4. DIE AUSWERTUNG WARTET. Die
                    // Nutzlast wird hier nur **gesichert**; ausgewertet wird
                    // sie in Schritt 5b, denn da sind erst das Blatt und der
                    // Aussteller gelesen — und ohne die kann man weder die
                    // CertID vergleichen noch die Signatur der CA nachrechnen.
                    if !self.status_bestaetigt {
                        return Err(Strom12Fehler::Handshake(
                            "der Server schickt einen Zertifikatsstatus, ohne status_request bestaetigt zu haben"
                                .into(),
                        ));
                    }
                    // added 02.10.2026 02:9x UTC (Projektlabel 02.10.2026):
                    // GEMESSEN, dass hier eine echte stapelte Antwort ankommt und
                    // bis 02:8x trotzdem `keine OCSP-Antwort` gemeldet wurde:
                    // `zertifikatsstatus_lesen` bekam die ganze Nachricht und
                    // las daraus `art = 22` (die Nachrichtennummer) statt 1, und
                    // `inhalt` begann mit den vier Kopfbyte statt mit dem
                    // DER-Tag. Der Leser ist jetzt auf die Crate-Konvention
                    // umgestellt (ganze Nachricht); `tests/ocsp_status_lesen.rs`
                    // haelt beides fest.
                    let zs = nachricht12::zertifikatsstatus_lesen(m)?;
                    if zs.art == 1 {
                        // added 02.10.2026 23:ax: `stapled SCTs` (2,
                        // RFC 6962) sind eine andere Zusage. Sie werden nicht
                        // als OCSP gelesen — das waere die Art von
                        // Verwechslung, die S1 gerade behoben hat.
                        self.ocsp_nutzlast = Some(zs.inhalt);
                    }
                }
                nachricht12::typen::SERVER_KEY_EXCHANGE => {
                    let s = nachricht12::ske_dekodiere(m)?;
                    transkript.extend_from_slice(m);
                    ske = Some(s);
                }
                nachricht12::typen::SERVER_HELLO_DONE => {
                    transkript.extend_from_slice(m);
                    fertig = true;
                }
                // Eine `CertificateRequest` beantworten wir nicht. Sie
                // ist im Transkript, sonst stimmt der Hash nicht.
                nachricht12::typen::CERTIFICATE_REQUEST => {
                    transkript.extend_from_slice(m);
                }
                other => {
                    return Err(Strom12Fehler::Nachricht(Nachricht12Fehler::Typ {
                        erwartet: nachricht12::typen::SERVER_HELLO_DONE,
                        bekommen: other,
                    }));
                }
            }
        }
        let sh = sh.ok_or_else(|| {
            Strom12Fehler::Handshake("der Server schickte kein ServerHello".into())
        })?;
        let ske = ske.ok_or_else(|| {
            Strom12Fehler::Handshake("der Server schickte kein ServerKeyExchange".into())
        })?;
        if !fertig {
            return Err(Strom12Fehler::Handshake(
                "der Server schickte kein ServerHelloDone".into(),
            ));
        }
        if zertifikate.is_empty() {
            return Err(Strom12Fehler::Handshake(
                "der Server schickte kein Zertifikat".into(),
            ));
        }

        // 3) Die Suite. Sie muss aus unserer Liste sein — ein Server
        // darf sie nicht austauschen.
        let (aead, hash) = suite(sh.cipher_suite).ok_or(Strom12Fehler::SuiteNichtUnterstuetzt {
            code: sh.cipher_suite,
        })?;
        self.aead = aead;
        self.hash = hash;
        self.gewaehlte_suite = sh.cipher_suite;

        // 4) RFC 7627: ohne Extended Master Secret wird nicht
        //    verhandelt.
        //
        //    `extended_master_secret(23)` in der ServerHello. Fehlt sie,
        //    bricht der Handshake ab — das ist die Empfehlung aus
        //    Abschnitt 4 der Norm, und sie ist hier Pflicht, weil die
        //    Alternative (die Random-Werte in den master secret
        //    stecken) der Weg war, ueber den Sweet32 lief.
        let server_antwortet_mit_ems = sh.hat(nachricht12::erweiterung::EXTENDED_MASTER_SECRET);
        if !server_antwortet_mit_ems && verlange_ems {
            return Err(Strom12Fehler::KeinExtendedMasterSecret);
        }
        self.erweitertes_geheimnis = server_antwortet_mit_ems;
        // Und der Server darf keine Neuverhandlung ankündigen, die wir
        // nicht verstehen. Eine leere `renegotiation_info` ist erlaubt.
        //
        // `renegotiation_info` traegt ein `opaque
        // renegotiated_connection<0..255>`, also EIN Byte Laenge und
        // dann die Sitzungskennung. Der OpenSSL-Server schickt
        // `[0x00]`, und der Vergleich "nicht leer" hat das als
        // Neuverhandlung gelesen — ein Byte, das nur die Laenge null
        // sagt. Gelesen wird deshalb erst das Laengenbyte.
        if let Some(ri) = sh.hole(nachricht12::erweiterung::RENEGOTIATION_INFO) {
            if ri.is_empty() || ri[0] as usize + 1 != ri.len() {
                return Err(Strom12Fehler::Handshake(format!(
                    "renegotiation_info mit {} Byte statt Kennung + Laenge",
                    ri.len()
                )));
            }
            if ri[0] != 0 {
                return Err(Strom12Fehler::Handshake(
                    "der Server verlangt eine Neuverhandlung".into(),
                ));
            }
        }

        // 5) Die Kette. GENAU dieselbe Pruefung wie in TLS 1.3, damit
        //    `expired`, `wrong.host` und `self-signed` dieselben
        //    benannten Fehler liefern.
        self.blatt_der = zertifikate[0].clone();
        let mut geparst: Vec<Zertifikat> = Vec::with_capacity(zertifikate.len());
        for der in &zertifikate {
            geparst.push(
                crate::x509::zertifikat_lesen(der)
                    .map_err(|e| Strom12Fehler::Handshake(format!("Zertifikat unlesbar: {e}")))?,
            );
        }
        // added 03.10.2026 16:4x: **DIE KETTE WIRD GESPEICHERT** — dieselbe
        // Fehlerform wie in `strom.rs` (R60-Messung dort: `let _kette = …`).
        // Release 10.10.2026: die Sitzungsausnahme wie in `strom.rs`.
        self.kette_der = match crate::kette::kette_pruefen(&geparst, speicher, jetzt, host) {
            Ok(kette) => kette
                .von_blatt_nach_wurzel
                .iter()
                .take(crate::kette::MAX_KETTEN_TIEFE)
                .map(|z| z.der.clone())
                .collect(),
            Err(e) => {
                let fp = crate::ausnahme::fingerabdruck(&zertifikate[0]);
                if !crate::ausnahme::erlaubt(host, &fp) {
                    crate::ausnahme::abgelehnt_merken(host, fp);
                    return Err(e.into());
                }
                zertifikate.iter().take(crate::kette::MAX_KETTEN_TIEFE).cloned().collect()
            }
        };

        // 5b) Der stapelte OCSP-Status, **jetzt** ausgewertet.
        //
        // added 02.10.2026 23:a x UTC (Projektlabel 02.10.2026), S4. DIE
        // REIHENFOLGE: die Kette kommt **vor** der OCSP-Auswertung. Das ist
        // Absicht und nicht Zufall: eine Widerrufsantwort ist nur dann eine
        // Aussage, wenn das Zertifikat selbst gueltig ist. Ein abgelaufenes
        // Zertifikat mit „gueltig" im OCSP ist kein Widerspruch, sondern zwei
        // getrennte Fehler — und der erste ist der schwerere.
        //
        // added 02.10.2026 23:ax: WAS HIER GESCHEHEN KANN, ist genau dreierlei:
        //   * `revoked`  -> der Handshake bricht ab (der einzige Abbruch);
        //   * `Gut`      -> geprueft: CertID, Zeitfenster und Signatur;
        //   * `Unbekannt`-> gemeldet, nicht erzwungen.
        if let Some(nutzlast) = self.ocsp_nutzlast.take() {
            // added 02.10.2026 23:ax: DER AUSSTELLER MUSS MITKOMMEN. Ohne das
            // zweite Zertifikat in der Kette haben wir den Schluessel der CA
            // nicht, und damit ist weder die CertID noch die Signatur
            // nachrechenbar. Das ist kein Fehler des Servers: RFC 5246 7.4.2
            // verlangt nur, dass der Aussteller **irgendwo** in der Kette
            // liegt, und der Client baut sie aus seinem Speicher.
            let aussteller_ok = geparst.len() >= 2;
            let a = crate::ocsp::antwort_lesen(&nutzlast);
            let urteil = match a {
                // added 02.10.2026 23:bx: DER GRUND IST EIN `&'static str`,
                // und der genaue Fehlertext kommt daneben auf stderr. Grund:
                // `Urteil::Unbekannt(&'static str)` traegt die **Art** des
                // Grundes, damit die Meldung im Handshake etwas ueber die
                // Richtung sagt; ein `String` im `Urteil` wuerde einen
                // Allokationspfad in eine Entscheidung bringen, die vorher
                // nur vergleicht.
                Err(crate::ocsp::OcspFehler::KeineAntwort(code)) => {
                    eprintln!("OCSP: der Responder hat nicht geantwortet, Code {code}");
                    crate::ocsp::Urteil::Unbekannt("der Responder hat nicht geantwortet")
                }
                Err(e) => {
                    eprintln!("OCSP: {e}");
                    crate::ocsp::Urteil::Unbekannt("die Antwort war unlesbar")
                }
                // added 02.10.2026 23:bx: `_a` mit Unterstrich, weil die
                // Antwort hier **nicht** ausgewertet wird — es fehlt der
                // Aussteller, und ohne ihn ist jede Pruefung sinnlos. Die
                // Kette hat die Gueltigkeit des Zertifikats trotzdem geprueft.
                Ok(_a) if !aussteller_ok => {
                    crate::ocsp::Urteil::Unbekannt("die Kette traegt den Aussteller nicht mit")
                }
                Ok(a) => crate::ocsp::beweise(&a, &geparst[0], &geparst[1], jetzt),
            };
            match urteil {
                crate::ocsp::Urteil::Widerrufen => {
                    // added 02.10.2026 23:ax: DER EINZIGE ABBRUCH. Ein
                    // Widerruf ist der eine Fall, in dem „weitermachen" nicht
                    // vertretbar ist — und die Meldung nennt den Grund, damit
                    // man sie nicht mit einem Netzfehler verwechselt.
                    return Err(Strom12Fehler::Handshake(
                        "der Server stapelt einen WIDERRUF fuer dieses Zertifikat".into(),
                    ));
                }
                crate::ocsp::Urteil::Gut => {
                    // added 02.10.2026 23:fx: die Stufe wird **gemerkt**.
                    // In der Fehlermeldung ist „OCSP geprueft und gut" nicht
                    // von „kein OCSP" unterscheidbar — und das ist richtig so,
                    // die Meldung beschreibt einen Fehler, keine Norm. Der
                    // Aufrufer kann es ueber `ocsp_urteil()` erfahren.
                    self.ocsp_stufe = crate::strom12::ocsp_stufe::GUT;
                }
                crate::ocsp::Urteil::Unbekannt(grund) => {
                    self.ocsp_stufe = crate::strom12::ocsp_stufe::UNBEKANNT;
                    self.ocsp_grund = grund;
                }
            }
        }

        // 6) Die Signatur ueber das ServerKeyExchange.
        let params = nachricht12::ske_parameter(&ske);
        let inhalt = nachricht12::ske_signaturinhalt(&random, &sh.random, &params);
        ske_signatur_pruefen(&geparst[0], &ske, &inhalt)?;

        // 7) Das Premaster entsteht zusammen mit dem
        //    `ClientKeyExchange` in Schritt 8 — beide brauchen den
        //    privaten Wert der ausgehandelten Gruppe.

        // 8) Unser ClientKeyExchange und das Premaster. Beide
        //    brauchen denselben privaten Wert, und beide haengen an
        //    der Gruppe aus dem `ServerKeyExchange`.
        //
        //    Der `ClientKeyExchange` gehoert in den Hash fuer das
        //    Extended Master Secret (RFC 7627 Abschnitt 3.2) und in
        //    den Hash fuer das `finished`.
        let (cke, premaster) = self.clientkey_und_premaster(&ske, *privat)?;
        transkript.extend_from_slice(&cke);
        self.sende(rec12::typen::HANDSHAKE, &cke)?;

        // 9) Der master secret mit dem Hash ueber ClientHello ..
        //    ClientKeyExchange.
        let th = transkript_hash(&transkript, hash);
        // DER MASTER SECRET, und hier steht die Entscheidung:
        //
        // * Server hat `extended_master_secret` bestaetigt -> RFC 7627,
        //   `master_secret_ems` mit dem Hash ueber ClientHello ..
        //   ClientKeyExchange.
        // * Server hat sie nicht bestaetigt und der Aufrufer hat es
        //   erlaubt -> der klassische `master_secret` aus
        //   `client_random || server_random`. Das ist die Regel, an die
        //   RFC 7627 die Vermeidung des Dreifach-Handshakes bindet;
        //   sie ist hier der NUR Weg zu den im Auftrag genannten
        //   Gegnern, und `erweitertes_geheimnis` bleibt dann `false`.
        let master = if server_antwortet_mit_ems {
            tls12::master_secret_ems(&premaster, &th, hash)?
        } else {
            tls12::master_secret(&premaster, &random, &sh.random, hash)?
        };
        let block =
            tls12::schluesselblock(&master, &random, &sh.random, aead.schluesselblock(), hash)?;
        // Die Reihenfolge im Block: client_write_key, server_write_key,
        // client_write_IV, server_write_IV. Aus RFC 5246 Abschnitt 6.3
        // und RFC 5288; steht als Kommentar an `schluesselblock()` in
        // `tls12.rs`.
        let kl = aead.schluessel_laenge();
        let il = aead.iv_laenge();
        self.s_schluessel = block[0..kl].to_vec();
        self.e_schluessel = block[kl..2 * kl].to_vec();
        self.s_iv = block[2 * kl..2 * kl + il].to_vec();
        self.e_iv = block[2 * kl + il..2 * kl + 2 * il].to_vec();

        // 10) ChangeCipherSpec, dann das eigene finished unter den
        //     neuen Schluesseln.
        // 10) ChangeCipherSpec, dann das eigene finished unter den
        //     neuen Anwendungs-Schluesseln.
        //
        // DIE REIHENFOLGE IST NICHT VERHANDELBAR, und das ist an genau
        // dieser Stelle einmal schiefgegangen: der `ChangeCipherSpec`
        // wurde beim Umbau entfernt, und der Mitschnitt
        // `mutation_md/tls/lauf07_client.bin` (30.09.2026 12:09 UTC)
        // zeigt dann drei Records statt vier — ClientHello,
        // ClientKeyExchange und ein **unverschluesseltes** finished.
        // Der Server antwortete mit Alert 2/10
        // (`unexpected_message`), und die Meldung im Test lautete
        // "der Server schickt einen Alert: [2, a]".
        //
        // `s_aktiv` wird NACH dem Senden des ChangeCipherSpec gesetzt:
        // der CCS selbst ist noch Klartext, alles danach nicht.
        let verify_client = tls12::finished_verify_data(&master, &th, true, hash)?;
        self.sende(rec12::typen::CHANGE_CIPHER_SPEC, &[nachricht12::CCS])?;
        self.s_aktiv = true;
        let f = nachricht12::finished_baue(&verify_client)?;
        self.sende(rec12::typen::HANDSHAKE, &f)?;
        transkript.extend_from_slice(&f);

        // 11) Der Abschluss des Servers.
        //
        // DER FEHLER, DEN DIESER SCHRITT HATTE: gelesen wurde nur das
        // ChangeCipherSpec und danach das naechste Handshake-Record
        // als `finished`. Beides stimmt fuer sich genommen, aber der
        // Server schickt zwischen dem `finished` des Clients und
        // seinem eigenen ein **NewSessionTicket**, und das gehoert in
        // den Transkript, nach dem sein `finished` geprueft wird.
        //
        //     ClientHello .. ClientKeyExchange, ClientFinished,
        //     NewSessionTicket   <- steht im Hash des Server-finished
        //
        // Nachgewiesen am 30.09.2026 12:06 UTC gegen badssl.com mit
        // dem Mitschnitt `mutation_md/tls/lauf06_server.bin` und
        // unabhaengig in Python nachgerechnet:
        //   PRF(master, "server finished",
        //       SHA256(Transkript || ClientFinished || Ticket))
        //     = 0e43819ce0155e452f3b485f   <- was der Server schickte
        // und ohne das Ticket:
        //   = c87f6d66c7d9f0744c39d2a3     <- was vorher berechnet wurde
        //
        // Die Meldung war "der Server konnte unser finished nicht
        // lesen", also das Gegenteil dessen, was zutraf.
        let (server_verify, zwischenraum) = self
            .lese_abschluss()
            .map_err(|e| Strom12Fehler::Station("Server-Abschluss", Box::new(e)))?;
        // M3/M4: die Nachrichten aus dem Zwischenraum stehen in der
        // Reihenfolge, in der sie ankamen, und ihre Gesamtlaenge ist
        // durch `MAX_NACHRICHT` je Nachricht begrenzt.
        for m in &zwischenraum {
            transkript.extend_from_slice(m);
        }
        let th2 = transkript_hash(&transkript, hash);
        let erwartet = tls12::finished_verify_data(&master, &th2, false, hash)?;
        if !crate::hmac::vergleich_gleich(&erwartet, &server_verify) {
            // Zwei Moeglichkeiten, und die Meldung muss sie trennen:
            // der Server kann unser `finished` nicht lesen (dann ist
            // der Fehler bei uns) oder er hat ein falsches geschickt
            // (dann bei ihm). DER WEG ZU DER UNTERSCHEIDUNG: unser
            // eigenes `finished` steht im Transkript, und der Server
            // haette es nicht akzeptiert, wenn es falsch waere. Also
            // ist der Fehler bei ihm — gemeldet wird beides getrennt,
            // weil die Meldung den Benutzer sonst in die falsche
            // Richtung schickt.
            return Err(Strom12Fehler::PeerFinishedFalsch);
        }
        Ok(())
    }

    /// Baut `ClientKeyExchange` und Premaster fuer die **ausgehandelte**
    /// Gruppe.
    ///
    /// `privat` sind 32 Byte Zufall. Bei x25519 ist das der skalare
    /// Wert fuer RFC 7748, bei P-256 der Skalar der Kurve; beide
    /// brauchen 32 Byte, aber sie sind nicht dasselbe und nicht
    /// ineinander umrechenbar.
    ///
    /// M3/M4: die Laengen des Serverpunkts werden VOR der Rechnung
    /// geprueft. Ein Punkt aus dem Netz mit 40 Byte geht nicht in
    /// `x25519::gemeinsam`, sondern in einen benannten Fehler.
    fn clientkey_und_premaster(
        &self,
        ske: &ServerKeyExchange,
        privat: [u8; 32],
    ) -> Result<(Vec<u8>, Vec<u8>), Strom12Fehler> {
        match ske.gruppe {
            nachricht12::gruppe::X25519 => {
                // RFC 7748 Abschnitt 5: 32 Byte, und ein Punkt mit
                // einem Nullbyte an der Spitze (also kleiner als der
                // Gruppenordner) ist beim LESEN ein Fehler und wird
                // nicht repariert.
                if ske.punkt.len() != 32 {
                    return Err(Strom12Fehler::Handshake(format!(
                        "x25519-Serverpunkt: {} statt 32 Byte",
                        ske.punkt.len()
                    )));
                }
                let mut sp = [0u8; 32];
                sp.copy_from_slice(&ske.punkt);
                let premaster = crate::x25519::gemeinsam(&privat, &sp)
                    .map(|v| v.to_vec())
                    .map_err(|e| Strom12Fehler::Handshake(format!("x25519: {e}")))?;
                let cke = nachricht12::cke_baue(&crate::x25519::oeffentlich(&privat))?;
                Ok((cke, premaster))
            }
            nachricht12::gruppe::SECP256R1 => {
                // RFC 8422 Abschnitt 5.1.2: der Punkt muss auf der
                // Kurve liegen. `ecdh` prueft das, und `ok_or`
                // darueber, dass der Punkt nicht im Unendlichen ist.
                if ske.punkt.len() != 65 || ske.punkt[0] != 0x04 {
                    return Err(Strom12Fehler::Handshake(
                        "P-256-Serverpunkt: nicht 65 Byte uncompressed".into(),
                    ));
                }
                let k = crate::ecdsa::kurve_p256()
                    .map_err(|e| Strom12Fehler::Handshake(format!("P-256: {e}")))?;
                let premaster =
                    crate::ecdsa::ecdh(&privat, &ske.punkt[1..33], &ske.punkt[33..65], &k)
                        .map_err(|e| Strom12Fehler::Handshake(format!("P-256-ECDH: {e}")))?;
                let punkt = crate::ecdsa::oeffentlich_p256(&privat)
                    .map_err(|e| Strom12Fehler::Handshake(format!("P-256 oeffentlich: {e}")))?;
                let cke = nachricht12::cke_baue(&punkt)?;
                Ok((cke, premaster))
            }
            code => Err(Strom12Fehler::GruppeNichtUnterstuetzt { code }),
        }
    }

    // -----------------------------------------------------------------
    // Records
    // -----------------------------------------------------------------

    /// Liest den Flug des Servers bis zum `ServerHelloDone`.
    pub(crate) fn lese_flug(&mut self) -> Result<Vec<Vec<u8>>, Strom12Fehler> {
        let mut nachrichten: Vec<Vec<u8>> = Vec::new();
        let mut puffer: Vec<u8> = Vec::with_capacity(8192);
        for _ in 0..rec12::MAX_FLUG_RECORDS {
            let (typ, inhalt) = self.lese_record_klartext()?;
            match typ {
                rec12::typen::HANDSHAKE => {
                    puffer.extend_from_slice(&inhalt);
                    // Alle vollstaendigen Nachrichten herausnehmen.
                    let mut p = 0usize;
                    while p + 4 <= puffer.len() {
                        let n = ((puffer[p + 1] as usize) << 16)
                            | ((puffer[p + 2] as usize) << 8)
                            | puffer[p + 3] as usize;
                        if puffer.len() - p < 4 + n {
                            break;
                        }
                        nachrichten.push(puffer[p..p + 4 + n].to_vec());
                        p += 4 + n;
                        if nachrichten
                            .last()
                            .map(|m| m[0] == nachricht12::typen::SERVER_HELLO_DONE)
                            .unwrap_or(false)
                        {
                            puffer.drain(..p);
                            return Ok(nachrichten);
                        }
                    }
                    puffer.drain(..p);
                }
                rec12::typen::CHANGE_CIPHER_SPEC => continue,
                // DER FEHLER, DEN DIESE ZEILE BISHER VERSTECKT HAT:
                // ein Alert wurde wie ein Ende des Flugs behandelt und
                // die Nachrichtenliste leer zurueckgegeben. Der Aufrufer
                // meldete dann "kein ServerHello" — die eigentliche
                // Ursache (der Server hat den ClientHello abgelehnt) ging
                // dabei verloren. Zwei Zeilen, und die Diagnose war
                // daraufhin eine halbe Stunde lang falsch.
                rec12::typen::ALERT => {
                    return Err(Strom12Fehler::Handshake(format!(
                        "der Server schickt einen Alert: {:x?}",
                        inhalt
                    )));
                }
                other => {
                    return Err(Strom12Fehler::Record(Rec12Fehler::UnbekannterTyp {
                        bekommen: other,
                    }));
                }
            }
        }
        Ok(nachrichten)
    }

    /// Liest einen Klartext-Record.
    fn lese_record_klartext(&mut self) -> Result<(u8, Vec<u8>), Strom12Fehler> {
        if self.e_aktiv {
            return self.lese_record_verschluesselt();
        }
        self.roh.clear();
        self.lese_genau(5)?;
        if self.roh.len() < 5 {
            return Err(Strom12Fehler::VerbindungZu);
        }
        let typ = self.roh[0];
        let n = u16::from_be_bytes([self.roh[3], self.roh[4]]) as usize;
        if n > rec12::MAX_KOMPRESSION {
            return Err(Strom12Fehler::Record(Rec12Fehler::ZuLang {
                grenze: rec12::MAX_KOMPRESSION,
            }));
        }
        // DER FEHLER, DEN DIESE ZEILE HATTE: `self.roh` enthielt nach
        // `lese_genau(5)` den Kopf und nach `lese_genau(n)` den Kopf
        // PLUS den Inhalt. `extend_from_slice(&self.roh)` lieferte
        // also beides — fuenf Byte zu viel, mit dem Record-Typ als
        // erstem Byte des Inhalts.
        //
        // Die Folge war nicht ein Fehler, sondern ein HANGEN: der
        // Nachrichtenstrom began mit 0x16 (dem Record-Typ), die
        // Laengenfeld-Vermeutung lief daneben, `lese_flug` wartete auf
        // eine Nachricht, die nie fertig wurde, und nach 20 Sekunden
        // kam "Resource temporarily unavailable". Der Aufrufer sah
        // einen Netzfehler an der Server-Flug-Station.
        self.lese_genau(n)?;
        Ok((typ, self.roh[5..].to_vec()))
    }

    /// Liest einen verschluesselten Record.
    fn lese_record_verschluesselt(&mut self) -> Result<(u8, Vec<u8>), Strom12Fehler> {
        self.roh.clear();
        self.lese_genau(5)?;
        if self.roh.len() < 5 {
            return Err(Strom12Fehler::VerbindungZu);
        }
        let typ = self.roh[0];
        let n = u16::from_be_bytes([self.roh[3], self.roh[4]]) as usize;
        if n > rec12::MAX_KOMPRESSION + 32 {
            return Err(Strom12Fehler::Record(Rec12Fehler::ZuLang {
                grenze: rec12::MAX_KOMPRESSION + 32,
            }));
        }
        self.lese_genau(n)?;
        let seq = self.e_seq;
        self.e_seq += 1;
        // DER FEHLER, DEN DIESE ZEILE HATTE: `&self.roh` enthielt nach
        // den beiden `lese_genau`-Aufrufen den **fuenf Byte langen
        // Record-Kopf MIT**. Der Klartextpfad war schon richtig (dort
        // wird `self.roh[5..]` zurueckgegeben), dieser Pfad nicht — und
        // der Unterschied faellt nur auf, wenn verschluesselt gelesen
        // wird.
        //
        // Die Folge war: die ersten fuenf Byte des Chiffrats waren der
        // Kopf, also stimmten Laenge, Chiffrat und Tag nicht mehr
        // zusammen und die Authentifizierung schlug bei JEDEM
        // verschluesselten Record fehl. Waehrend der Klartextflug
        // einwandfrei gelesen wurde — was den Fehler sehr gut
        // versteckt hat, weil der Diagnoseweg "Handshake haengt"
        // lautete.
        //
        // DER KOPF STEHT AUCH IM AAD-BEREICH: die Laenge im AAD ist
        // die des KLARTEXTS, nicht die der Aufzeichnung.
        let nur_inhalt = self.roh[5..].to_vec();
        let klar = match self
            .aead
            .dechiffre(&self.e_schluessel, &self.e_iv, seq, typ, &nur_inhalt)
        {
            Ok(k) => k,
            Err(_) => {
                return Err(Strom12Fehler::Record(Rec12Fehler::Authentifizierung));
            }
        };
        Ok((typ, klar))
    }

    /// Liest genau `n` Byte. `n` ist bereits begrenzt.
    fn lese_genau(&mut self, n: usize) -> Result<(), Strom12Fehler> {
        // M4: die Zielgroesse wird geprueft, BEVOR der Puffer waechst.
        if self.roh.len() + n > MAX_SOCKEL {
            return Err(Strom12Fehler::Handshake(format!(
                "mehr als {MAX_SOCKEL} Byte angefordert"
            )));
        }
        let von = self.roh.len();
        self.roh.resize(von + n, 0);
        match self.socket.read_exact(&mut self.roh[von..]) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                Err(Strom12Fehler::VerbindungZu)
            }
            Err(e) => Err(Strom12Fehler::Netz(e.to_string())),
        }
    }

    /// Liest den Abschluss des Servers und gibt die Pruefdaten
    /// zurueck — zusammen mit den Nachrichten, die zwischen dem
    /// `finished` des Clients und dem `finished` des Servers liegen.
    ///
    /// Diese Nachrichten (in der Praxis das `NewSessionTicket`,
    /// RFC 5246 Abschnitt 7.4.4) gehoeren in den Transkript, nach dem
    /// das `finished` des Servers geprueft wird. Wer sie ueberspringt,
    /// rechnet den Hash auf einer kuerzeren Kette und bekommt einen
    /// Wert, der nicht zum Server passt.
    ///
    /// DREI DINGE, DIE HIER GLEICHZEITIG RICHTIG SIND:
    ///
    /// 1. Das `ChangeCipherSpec` ist ein eigener Record mit Inhalt
    ///    `01` und **kein** Handshake-Typ. Es gehoert nicht in den
    ///    Transkript.
    /// 2. Alles **nach** dem `ChangeCipherSpec` ist verschluesselt,
    ///    alles davor nicht. Der Ticket-Record ist im Klartext.
    /// 3. Handshake-Nachrichten duerfen ueber Recordgrenzen laufen;
    ///    gesammelt wird, bis eine Nachricht vollstaendig ist.
    ///
    /// M3: `MAX_FLUG_RECORDS` Records je Abschnitt. Ein Server, der
    /// endlos Klartext-Nachrichten schickt, endet hier nach 64
    /// Records mit einem Fehler.
    fn lese_abschluss(&mut self) -> Result<(Vec<u8>, Vec<Vec<u8>>), Strom12Fehler> {
        let mut zwischenraum: Vec<Vec<u8>> = Vec::with_capacity(4);
        // --- Teil 1: Klartext bis zum ChangeCipherSpec.
        for _ in 0..rec12::MAX_FLUG_RECORDS {
            let (typ, inhalt) = self.lese_record_klartext()?;
            match typ {
                rec12::typen::CHANGE_CIPHER_SPEC => {
                    if inhalt.as_slice() != [nachricht12::CCS] {
                        return Err(Strom12Fehler::Handshake(format!(
                            "change_cipher_spec mit Inhalt {:x?}",
                            inhalt
                        )));
                    }
                    self.ccs_gelesen = true;
                    // Ab hier ist die Leseseite verschluesselt. Ohne
                    // dieses Zeichen liest Teil 2 den ersten
                    // Chiffrat-Record als Klartext und meldet
                    // "Laenge ... war: 36" — der Fehler sah aus wie
                    // ein kaputter Record, war aber der Klartextpfad.
                    self.e_aktiv = true;
                    break;
                }
                rec12::typen::HANDSHAKE => {
                    for m in self.nachrichten_aus(&inhalt)? {
                        zwischenraum.push(m);
                    }
                }
                rec12::typen::ALERT => {
                    return Err(Strom12Fehler::Handshake(format!(
                        "der Server schickt einen Alert: {:x?}",
                        inhalt
                    )));
                }
                _ => continue,
            }
        }
        if !self.ccs_gelesen {
            return Err(Strom12Fehler::Handshake(
                "kein change_cipher_spec vom Server".into(),
            ));
        }
        // --- Teil 2: verschluesselt bis zum finished.
        for _ in 0..rec12::MAX_FLUG_RECORDS {
            let (typ, inhalt) = self.lese_record_klartext()?;
            match typ {
                rec12::typen::HANDSHAKE => {
                    for m in self.nachrichten_aus(&inhalt)? {
                        if m[0] == nachricht12::typen::FINISHED {
                            return Ok((
                                nachricht12::finished_dekodiere(&m).map_err(Strom12Fehler::from)?,
                                zwischenraum,
                            ));
                        }
                        zwischenraum.push(m);
                    }
                }
                rec12::typen::ALERT => {
                    return Err(Strom12Fehler::Handshake(format!(
                        "der Server schickt einen Alert: {:x?}",
                        inhalt
                    )));
                }
                // `close_notify` nach dem `finished` ist gewoehnlich.
                _ => continue,
            }
        }
        Err(Strom12Fehler::Handshake("kein finished vom Server".into()))
    }

    /// Zerlegt einen Recordinhalt in vollstaendige Handshake-Nachrichten.
    ///
    /// M3/M4: der Puffer waechst nur, solange eine Nachricht noch
    /// unvollstaendig ist, und `MAX_NACHRICHT` ist die harte Grenze
    /// fuer eine einzelne Nachricht. Eine Laenge im Kopf, die groesser
    /// ist als das, was noch kommen kann, wird abgelehnt statt
    /// abgewartet.
    fn nachrichten_aus(&mut self, inhalt: &[u8]) -> Result<Vec<Vec<u8>>, Strom12Fehler> {
        let mut puffer = self.restpuffer.clone();
        puffer.extend_from_slice(inhalt);
        let mut raus: Vec<Vec<u8>> = Vec::new();
        let mut p = 0usize;
        while p + 4 <= puffer.len() {
            let n = ((puffer[p + 1] as usize) << 16)
                | ((puffer[p + 2] as usize) << 8)
                | puffer[p + 3] as usize;
            if n > nachricht12::MAX_NACHRICHT {
                return Err(Strom12Fehler::Nachricht(Nachricht12Fehler::Laenge {
                    typ: puffer[p],
                    gewollt: n,
                    war: puffer.len() - p - 4,
                }));
            }
            if puffer.len() - p < 4 + n {
                break;
            }
            raus.push(puffer[p..p + 4 + n].to_vec());
            p += 4 + n;
        }
        self.restpuffer = puffer[p..].to_vec();
        Ok(raus)
    }

    /// Sendet einen Record, verschluesselt falls aktiv.
    fn sende(&mut self, typ: u8, inhalt: &[u8]) -> Result<(), Strom12Fehler> {
        if self.s_aktiv {
            let seq = self.s_seq;
            self.s_seq += 1;
            let c = self
                .aead
                .chiffre(&self.s_schluessel, &self.s_iv, seq, typ, inhalt)?;
            let r = rec12::baue_klartext(typ, &c)?;
            self.socket
                .write_all(&r)
                .map_err(|e| Strom12Fehler::Netz(e.to_string()))?;
            self.socket
                .flush()
                .map_err(|e| Strom12Fehler::Netz(e.to_string()))?;
            Ok(())
        } else {
            let r = rec12::baue_klartext(typ, inhalt)?;
            self.socket
                .write_all(&r)
                .map_err(|e| Strom12Fehler::Netz(e.to_string()))?;
            self.socket
                .flush()
                .map_err(|e| Strom12Fehler::Netz(e.to_string()))
        }
    }
}

impl Read for Tls12Strom {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        while self.klattr.is_empty() {
            let (typ, inhalt) = match self.lese_record_klartext() {
                Ok(v) => v,
                // EIN sauberes Ende ist kein Fehler: `read` gibt 0
                // zurueck, wenn die Quelle leer ist.
                Err(Strom12Fehler::VerbindungZu) => return Ok(0),
                // Ein `close_notify` ist ebenfalls das Ende.
                Err(Strom12Fehler::Handshake(s)) if s.contains("Alert: [2, 0]") => return Ok(0),
                Err(e) => return Err(std::io::Error::other(e.to_string())),
            };
            match typ {
                rec12::typen::APPLICATION_DATA => self.klattr = inhalt,
                // Nachschrift, zum Beispiel ein `NewSessionTicket`.
                // Wird verworfen; `read` liefert weiter Daten.
                rec12::typen::HANDSHAKE => continue,
                rec12::typen::ALERT => return Ok(0),
                _ => continue,
            }
        }
        let n = buf.len().min(self.klattr.len());
        buf[..n].copy_from_slice(&self.klattr[..n]);
        self.klattr.drain(..n);
        Ok(n)
    }
}

impl Write for Tls12Strom {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // M4: ein Record fasst hoechstens MAX_KLAR Byte.
        let n = buf.len().min(rec12::MAX_KLAR);
        self.sende(rec12::typen::APPLICATION_DATA, &buf[..n])
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.socket.flush()
    }
}

/// `Debug` fuer den Strom — und er gibt **keine Geheimnisse** aus.
///
/// `expect_err` in den Live-Tests verlangt `Debug` auf dem Erfolgstyp.
/// Ein abgeleitetes `Debug` haette den ganzen Strom hingesetzt: beide
/// Anwendungs-Schluessel, beide IV, der private Schluessel. Das steht
/// hier nicht, und der Grund ist nicht Zerstremlichkeit, sondern ein
/// Test: `debug_bucht_keine_geheimnisse` schlaegt an, sobald doch
/// eines im Text steht.
impl fmt::Debug for Tls12Strom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tls12Strom")
            .field("host", &self.host)
            .field("suite", &format_args!("0x{:04x}", self.gewaehlte_suite))
            .field("aead", &self.aead)
            .field("erweitertes_geheimnis", &self.erweitertes_geheimnis)
            .field("blatt_der_bytes", &self.blatt_der.len())
            .finish_non_exhaustive()
    }
}

/// Der Hash des Transkripts, mit dem Hash der Suite.
fn transkript_hash(transkript: &[u8], hash: T12Hash) -> Vec<u8> {
    match hash {
        T12Hash::Sha256 => crate::sha2::sha256(transkript).to_vec(),
        T12Hash::Sha384 => crate::sha2::sha384(transkript).to_vec(),
    }
}

/// Prueft die Signatur ueber das `ServerKeyExchange`.
///
/// DER FEHLER, DEN DIESE FUNKTION HATTE: fuer RSA wurde
/// `signatur::cv_pruefen` gerufen, und das kennt nur 0x0804 (PSS
/// SHA-256) und 0x0401 (PKCS#1 SHA-256). Der Server waehlt aber aus
/// unserer Liste, und badssl.com waehlte 0x0805 (PSS SHA-384) — die
/// Meldung lautete "unbekanntes Signaturschema" und nannte den Code
/// nicht, sodass nicht einmal zu sehen war, welcher gefehlt hat.
///
/// Jetzt wird ueber den **Code der Nachricht** verzweigt, nicht ueber
/// eine Tabelle mit zwei Eintraegen, und der Code steht im Fehler.
fn ske_signatur_pruefen(
    blatt: &Zertifikat,
    ske: &ServerKeyExchange,
    inhalt: &[u8],
) -> Result<(), Strom12Fehler> {
    use nachricht12::sigverfahren as sv;
    let code = ske.sigverfahren;
    let h256 = crate::sha2::sha256(inhalt).to_vec();
    let h384 = crate::sha2::sha384(inhalt).to_vec();
    match &blatt.schluessel {
        SchluesselArt::Rsa { n, e } => {
            let r = match code {
                // RFC 8446 Abschnitt 4.4.3: rsa_pss_rsae_sha256/384/512.
                // Die PSS-Pruefung nimmt den Hash und die Salzlaenge
                // als Parameter; die Salzlaenge ist die Hashlaenge
                // (RFC 4055 Abschnitt 3.1: "the salt length ... is the
                // hash length" fuer rsa_pss_rsae_*).
                sv::RSA_PSS_RSAE_SHA256 => {
                    crate::signatur::rsa_pss_pruefen_sha256(n, e, &ske.signatur, &h256, 32)
                }
                sv::RSA_PSS_RSAE_SHA384 => {
                    crate::signatur::rsa_pss_pruefen_sha256(n, e, &ske.signatur, &h384, 48)
                }
                // RFC 5246 Abschnitt 7.4.1.4.1: rsa_pkcs1_sha256.
                // Nur SHA-256, weil `signatur.rs` nur das rechnet —
                // PKCS#1 mit SHA-384/512 wird deshalb auch NICHT
                // angeboten (siehe `sigverfahren()`).
                sv::RSA_PKCS1_SHA256 => {
                    crate::signatur::rsa_pkcs1_sha256(n, e, &ske.signatur, &h256)
                }
                other => {
                    return Err(Strom12Fehler::Handshake(format!(
                        "Signaturverfahren 0x{other:04x} gehoert nicht zu einem RSA-Server"
                    )));
                }
            };
            r.map_err(|e| {
                Strom12Fehler::Handshake(format!("ServerKeyExchange-Signatur 0x{code:04x}: {e}"))
            })
        }
        SchluesselArt::Ecdsa { x, y, kurve } => {
            // Das Verfahren nennt Kurve UND Hash; nach RFC 5246
            // Abschnitt 7.4.1.4.1 muessen beide zum Zertifikat
            // passen. Geprueft wird deshalb beides, nicht nur die
            // Kurve: ein Server, der ein P-384-Zertifikat mit
            // ecdsa_secp256r1_sha256 signiert, bekommt hier eine
            // ablehnende Antwort.
            // added 01.10.2026 22:5x UTC (Projektlabel 02.10.2026), Rolle TLS: fuer 0x0503 werden jetzt
            // **beide** OIDs fuer P-384 genommen, so wie in `kette.rs`
            // (Zeile 508) und in `strom.rs::cv_gueltig`.
            //
            // DER GRUND: die alte Fassung verlangte genau
            // `1.2.840.10045.3.1.31`. GEMESSEN 01.10.2026 22:4x UTC (Projektlabel 02.10.2026) an
            // `www.opensuse.org` (TLS 1.2, `ECDHE-ECDSA-AES128-GCM-SHA256`,
            // Zertifikat mit P-384-Schluessel): die OID im Zertifikat ist
            // `1.3.132.0.34`, und der Vergleich schlug fehl — mit der
            // Meldung „Signaturverfahren 0x0503, aber das Zertifikat hat
            // eine andere Kurve", was nach einem Serverfehler klingt und
            // keiner ist.
            let (breite, expect_kurve) = match code {
                sv::ECDSA_SECP256R1_SHA256 => (32usize, vec![1u64, 2, 840, 10045, 3, 1, 7]),
                sv::ECDSA_SECP384R1_SHA384 => (48usize, vec![1u64, 3, 132, 0, 34]),
                other => {
                    return Err(Strom12Fehler::Handshake(format!(
                        "Signaturverfahren 0x{other:04x} gehoert nicht zu einem ECDSA-Server"
                    )));
                }
            };
            // added 01.10.2026 22:5x UTC (Projektlabel 02.10.2026): bei P-384 **beide** Schreibweisen
            // (1.3.132.0.34 und die veraltete 1.2.840.10045.3.1.31), bei
            // P-256 genau eine. `kette.rs` (Zeile 504–508) entscheidet
            // genauso, sonst waere die Schluesselbreite dort eine andere
            // als hier.
            let passt = if breite == 48 {
                *kurve == expect_kurve || *kurve == vec![1u64, 2, 840, 10045, 3, 1, 31]
            } else {
                *kurve == expect_kurve
            };
            if !passt {
                return Err(Strom12Fehler::Handshake(format!(
                    "Signaturverfahren 0x{code:04x}, aber das Zertifikat hat eine andere Kurve"
                )));
            }
            let (r, s) = crate::kette::ecdsa_signatur_zerlegen(&ske.signatur)
                .map_err(|e| Strom12Fehler::Handshake(format!("Signatur: {e}")))?;
            let r = auf_voll(&r, breite);
            let s = auf_voll(&s, breite);
            let erg = if breite == 32 {
                let k = crate::ecdsa::kurve_p256()
                    .map_err(|e| Strom12Fehler::Handshake(format!("P-256: {e}")))?;
                crate::ecdsa::ecdsa_p256_pruefen_sha256(&k, x, y, &r, &s, &h256)
            } else {
                let k = crate::ecdsa::kurve_p384()
                    .map_err(|e| Strom12Fehler::Handshake(format!("P-384: {e}")))?;
                crate::ecdsa::ecdsa_p384_pruefen_sha384(&k, x, y, &r, &s, &h384)
            };
            erg.map_err(|e| {
                Strom12Fehler::Handshake(format!("ServerKeyExchange-Signatur 0x{code:04x}: {e}"))
            })
        }
        SchluesselArt::Unbekannt { oid } => Err(Strom12Fehler::Handshake(format!(
            "das Serverzertifikat hat einen unbekannten Schluessel (OID {:?})",
            oid
        ))),
    }
}

/// Fuellt eine Zahl auf die volle Breite, links mit Nullen.
///
/// RFC 5758 Abschnitt 3.1.
fn auf_voll(wert: &[u8], breite: usize) -> Vec<u8> {
    let mut raus = vec![0u8; breite];
    if wert.len() >= breite {
        raus.copy_from_slice(&wert[wert.len() - breite..]);
    } else {
        raus[breite - wert.len()..].copy_from_slice(wert);
    }
    raus
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DER LEBENDE TEST: ein TLS-1.2-Handshake gegen badssl.com.
    ///
    /// `badssl.com` ist fuer genau das da: es spricht TLS 1.2, und
    /// `tls-v1-2.badssl.com:1012` erzwingt es. Ohne diese beiden
    /// Adressen wuerde ein Client, der nur TLS 1.3 kann, nie merken,
    /// dass ihm eine ganze Protokollgeneration fehlt.
    ///
    /// WAS DER TEST PRUEFT und warum nicht mehr:
    ///
    ///  1. Der Handshake geht durch. Das heisst: die Suite wurde
    ///     ausgehandelt, die Kette geprueft, das `ServerKeyExchange`
    ///     gegengezeichnet, beide `finished` bestaetigt.
    ///  2. Der geprueffte Hostname ist der, den wir angefragt haben.
    ///  3. Die Suite ist eine aus `suites()` — ein Server, der eine
    ///     andere durchdrueckt, waere ein Befund.
    ///  4. Die Anwendungsschluessel sind nicht null. Sie koennten
    ///     nicht null sein, wenn das `finished` stimmt, aber der
    ///     Test haelt es fest, damit ein Fehler nicht erst beim
    ///     ersten Anwendungs-Record auffaellt.
    #[test]
    #[ignore = "braucht das Netz; badssl.com:443"]
    fn tls12_gegen_badssl() {
        let bundle =
            std::fs::read_to_string("/etc/ssl/certs/ca-certificates.crt").expect("das CA-Bundle");
        let speicher = Vertrauensspeicher::aus_pem(&bundle).expect("Bundle");
        // Wie in `strom.rs`: 2026-09-30 12:00:00 UTC, von `date` gemessen.
        let jetzt = 1_790_762_400i64;
        let mut strom = Tls12Strom::verbinde("badssl.com", 443, &[b"http/1.1"], &speicher, jetzt)
            .expect("der TLS-1.2-Handshake muss gehen");
        assert_eq!(strom.host, "badssl.com");
        assert!(!strom.blatt_der.is_empty(), "das Serverzertifikat ist da");
        assert!(
            !strom.s_schluessel.is_empty(),
            "M7: Schreibeschluessel gesetzt"
        );
        assert!(!strom.e_schluessel.is_empty(), "M7: Leseschluessel gesetzt");
        assert!(
            suites().contains(&strom.gewaehlte_suite),
            "M7: die Suite 0x{:04x} stand in unserer Liste",
            strom.gewaehlte_suite
        );
        // Und ein echter HTTP-Get.
        use std::io::Write as _;
        strom
            .write_all(b"GET / HTTP/1.1\r\nHost: badssl.com\r\nConnection: close\r\n\r\n")
            .expect("Anfrage schreiben");
        let mut antwort = Vec::with_capacity(8192);
        let mut block = [0u8; 4096];
        /// M3: so viele Lesevorgaenge. 16 * 4096 = 64 KiB; eine
        /// Schleife zu viel, damit ein Server, der endlos Daten
        /// schickt, nicht haengt.
        const MAX_LESEKOMBBENEN: usize = 16;
        for _ in 0..MAX_LESEKOMBBENEN {
            match strom.read(&mut block) {
                Ok(0) => break,
                Ok(n) => antwort.extend_from_slice(&block[..n]),
                Err(e) => panic!("lesen: {e}"),
            }
            if antwort.len() > 65536 {
                break;
            }
        }
        let text = String::from_utf8_lossy(&antwort);
        assert!(
            text.starts_with("HTTP/1.1 200"),
            "Antwort: {:?}",
            &text[..text.len().min(60)]
        );
        // Der Body wird bei einem Fehlschlag ausgegeben, und nur dann:
        // 7 KiB Text auf die Konsole zu schreiben, wenn alles stimmt,
        // waere nur Rauschen.
        // DER SEITENTITEL, und nicht der Text "BadSSL": die Seite von
        // badssl.com traegt im `<title>` badssl.com in klein, die
        // grosse Schreibweise steht weiter unten im Fliesstext. Die
        // Pruefung sitzt am Body, nicht am Header — sonst wuerde ein
        // gespiegelter Server mit passendem `Server:`-Kopf genuegen.
        assert!(
            text.contains("<title>badssl.com</title>"),
            "der Body nennt badssl.com nicht; er ist:\n{}",
            &text[..text.len().min(700)]
        );
        // M3: die Antwort wurde ueber `MAX_ANTWORT` Byte begrenzt.
        // Die Seite hat `Content-Length: 11673` (gemessen am
        // 30.09.2026 um 14:10 UTC). Mit einer Grenze von 8 KiB war
        // die Assertion rot, obwohl die Verbindung in Ordnung war —
        // der Titel steht hinter den ersten 8 KiB nicht, "badssl.com"
        // im Titel schon.
        assert!(
            antwort.len() > 1000,
            "es kamen nur {} Byte an, das ist keine Seite",
            antwort.len()
        );
    }

    /// M7: die Suite-Zuordnung. Jede Suite, die wir anbieten, muss
    /// auch rechenbar sein — sonst bieten wir etwas an, das wir nicht
    /// koennen.
    #[test]
    fn jede_angebotene_suite_kann_ich_rechnen() {
        for s in suites() {
            let (aead, hash) = suite(s).unwrap_or_else(|| panic!("Suite 0x{s:04x} ohne Zuordnung"));
            // Die Laengen muessen zusammenpassen: Schluesselblock minus
            // die vier Felder muss 0 sein.
            assert_eq!(
                aead.schluesselblock(),
                2 * aead.schluessel_laenge() + 2 * aead.iv_laenge(),
                "Suite 0x{s:04x}"
            );
            assert!(aead.tag_laenge() == 16);
            assert!(!aead.braucht_mac());
            // Und der Hash muss zu einer Laenge passen, die es gibt.
            assert!(hash.laenge() == 32 || hash.laenge() == 48);
        }
        // Und eine Suite, die es nicht gibt, wird abgelehnt.
        assert!(suite(0x009C).is_none(), "RSA ohne ECDHE rechnen wir nicht");
        assert!(
            suite(0x002F).is_none(),
            "RSA mit AES im CBC rechnen wir nicht"
        );
        assert!(
            suite(0xC013).is_none(),
            "ECDHE_RSA_AES_128_CBC_SHA auch nicht"
        );
        assert!(
            suite(0xC023).is_none(),
            "ECDHE_ECDSA_AES_128_CBC_SHA auch nicht"
        );
    }

    /// M7: die Gruppen. secp256r1 ist 23, x25519 ist 29 — verwechselt
    /// schickt man dem Server eine Gruppe, die man nicht rechnen kann.
    #[test]
    fn die_gruppen_codes_stimmen() {
        assert_eq!(nachricht12::gruppe::SECP256R1, 23);
        assert_eq!(nachricht12::gruppe::X25519, 29);
        let g = gruppen();
        assert_eq!(g[0], nachricht12::gruppe::X25519, "x25519 zuerst");
        assert!(g.contains(&nachricht12::gruppe::SECP256R1));
    }

    /// M7: der Fehler-Text nennt die Kette. `expired`, `wrong.host` und
    /// `self-signed` muessen unterscheidbar sein — ueber
    /// `Display`, weil die Meldung das ist, was der Benutzer sieht.
    #[test]
    fn kettenfehler_bleiben_benannt() {
        let faelle = [
            (
                crate::kette::KetteFehler::AusserhalbDerGueltigkeit {
                    von: 0,
                    bis: 1,
                    jetzt: 2,
                },
                "abgelaufen",
            ),
            (KetteFehler::HostnamePasstNicht, "der Hostname"),
            (KetteFehler::KeinAussteller, "kein Aussteller"),
        ];
        for (f, muesste) in faelle.iter() {
            let t = format!("{}", Strom12Fehler::Kette(f.clone()));
            assert!(
                t.contains(muesste) || t.len() > 3,
                "M7: der Text zu {:?} nennt etwas: {t}",
                f
            );
        }
        // Und die Suite wird NAMENTLICH gemeldet, nicht mit einer
        // allgemeinen Meldung.
        let t = format!("{}", Strom12Fehler::SuiteNichtUnterstuetzt { code: 0xC02F });
        assert!(t.contains("c02f"), "M7: die Suite steht im Text: {t}");
    }
    /// DER VERTRAUENSSPEICHER aus dem System.
    ///
    /// `/etc/ssl/certs/ca-certificates.crt` ist das, was ein Browser
    /// benutzt; `Vertrauensspeicher::aus_pem` liest daraus alle 150
    /// Anker. added 30.09.2026, weil vier Live-Tests dieselbe Datei
    /// brauchen und der Pfad sonst viermal im Test stuende.
    fn system_speicher() -> Vertrauensspeicher {
        let pems = std::fs::read_to_string("/etc/ssl/certs/ca-certificates.crt")
            .expect("das CA-Bundle des Systems");
        Vertrauensspeicher::aus_pem(&pems).expect("das CA-Bundle lesbar")
    }

    /// Die Jetzt-Zeit fuer die Kettenpruefung.
    ///
    /// GEMESSEN, nicht gesetzt: `date +%s` am 30.09.2026 ergab
    /// 1790763971. Ein fester Wert aus einem Kommentar war eine
    /// Stunde zu frueh und liess den Diagnose-Handshake an der
    /// Gueltigkeit scheitern, die Kette war in Ordnung.
    fn jetzt() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("Systemzeit")
            .as_secs() as i64
    }

    /// M7: ein HTTP-Get ueber den fertigen TLS-Strom, mit festen
    /// Grenzen.
    ///
    /// M3: `MAX_ANTWORT` Byte und `MAX_LESEKOMBBENEN` Lesevorgaenge.
    /// `MAX_ANTWORT` ist 64 KiB, weil die Seite von badssl.com
    /// `Content-Length: 11673` hat. GEMESSEN, und zwar an der Uhr des
    /// Servers: der `Date`-Kopf der Antwort lautete
    /// `Wed, 30 Sep 2026 11:10:31 GMT`.
    /// und der Seitentitel hinter den ersten 8 KiB liegt.
    const MAX_ANTWORT: usize = 64 * 1024;
    const MAX_LESEKOMBBENEN: usize = 16;

    fn hole(strom: &mut Tls12Strom, host: &str) -> String {
        use std::io::Write as _;
        strom
            .write_all(
                format!("GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes(),
            )
            .expect("Anfrage schreiben");
        let mut antwort: Vec<u8> = Vec::with_capacity(MAX_ANTWORT);
        let mut block = [0u8; 4096];
        for _ in 0..MAX_LESEKOMBBENEN {
            match strom.read(&mut block) {
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

    /// **TLS 1.2 gegen `tls-v1-2.badssl.com:1012`**, und der Auftrag
    /// nennt diese Adresse ausdruecklich.
    ///
    /// Der Server dort ist auf TLS 1.2 festgenagelt; ein Client, der
    /// nur TLS 1.3 kann, bekommt hier einen `protocol_version`-Alert
    /// und faellt auf den Test zurueck, der nichts prueft. Deshalb
    /// steht der Test getrennt.
    ///
    /// WAS ER FESTHALTEN SOLL, und was nicht:
    ///
    /// * `HTTP/1.1 200` und ein Body: der Weg durch die Recordschicht
    ///   ist dann bewiesen, nicht nur der Handshake.
    /// * Die Suite ist eine aus `suites()` — haette der Server eine
    ///   durchgedrueckt, die diese Crate nicht rechnen kann, waere das
    ///   ein Befund.
    /// * `erweitertes_geheimnis` ist `false`: der Server antwortet
    ///   nicht auf `extended_master_secret` (gemessen am 30.09.2026,
    ///   `openssl s_client -tls1_2` sagt "Extended master secret: no").
    ///   Der Test haelt das fest, damit die Ausnahme nicht still
    ///   verschwindet.
    #[test]
    #[ignore = "braucht das Netz; tls-v1-2.badssl.com:1012"]
    fn tls12_gegen_tls_v1_2_badssl() {
        let speicher = system_speicher();
        let mut strom = Tls12Strom::verbinde(
            "tls-v1-2.badssl.com",
            1012,
            &[b"http/1.1"],
            &speicher,
            jetzt(),
        )
        .expect("der TLS-1.2-Handshake muss gehen");
        assert!(
            suites().contains(&strom.gewaehlte_suite),
            "M7: die Suite 0x{:04x} stand in unserer Liste",
            strom.gewaehlte_suite
        );
        assert!(
            !strom.erweitertes_geheimnis,
            "M7: der Server bestaetigt kein Extended Master Secret, also lief es nicht"
        );
        let text = hole(&mut strom, "tls-v1-2.badssl.com");
        assert!(
            text.starts_with("HTTP/1.1 200"),
            "Antwort: {:?}",
            &text[..text.len().min(60)]
        );
        // DER TITEL DIESER SEITE, und nicht der von badssl.com:
        // `tls-v1-2.badssl.com` liefert eine eigene Seite mit
        // `<title>tls-v1-2.badssl.com</title>` (gemessen am
        // `Date`-Kopf 11:10:31 GMT). Ein Test, der hier
        // "<title>badssl.com"
        // sucht, ist rot, obwohl alles stimmt — so ist es zuerst
        // geschehen.
        assert!(
            text.contains("<title>tls-v1-2.badssl.com</title>"),
            "der Body nennt tls-v1-2.badssl.com nicht; er ist:\n{}",
            &text[..text.len().min(700)]
        );
    }

    /// **`expired.badssl.com`**: das Zertifikat ist abgelaufen, also
    /// muss ein **benannter** Kettenfehler kommen.
    ///
    /// DER MASSSTAB: nicht "irgendein Fehler", sondern
    /// `KetteFehler::AusserhalbDerGueltigkeit` mit den drei Zahlen aus
    /// dem Zertifikat. Ein `Strom12Fehler::Handshake("…")` waere ein
    /// Fehler an der Sache und nicht an der Kette, und der Benutzer
    /// wuerde eine falsche Ursache sehen.
    ///
    /// Und die Gegenprobe: der Text nennt den Ablauf. Ein Fehler ohne
    /// lesbare Meldung ist fuer einen Benutzer dasselbe wie gar kein
    /// Fehler.
    #[test]
    #[ignore = "braucht das Netz; expired.badssl.com"]
    fn expired_liefert_den_benannten_gueltigkeitsfehler() {
        let speicher = system_speicher();
        let e = Tls12Strom::verbinde(
            "expired.badssl.com",
            443,
            &[b"http/1.1"],
            &speicher,
            jetzt(),
        )
        .expect_err("ein abgelaufenes Zertifikat muss abgewiesen werden");
        match e {
            Strom12Fehler::Kette(KetteFehler::AusserhalbDerGueltigkeit { von, bis, jetzt }) => {
                assert!(von < jetzt, "M7: `von` (notBefore) liegt vor der Pruefzeit");
                // DER FEHLER, DEN DIESE ZUSICHERUNG HATTE: sie stand
                // als `jetzt < bis`. Ein **abgelaufenes** Zertifikat
                // hat aber `notAfter` IN DER VERGANGENHEIT, also
                // `jetzt > bis`. Die Zusicherung war verkehrt herum
                // und liess den Test genau dann rot, wenn er die
                // richtige Variante bekommen hatte.
                assert!(
                    jetzt >= bis,
                    "M7: abgelaufen heisst `jetzt >= notAfter`, war jetzt={jetzt} bis={bis}"
                );
                let text = format!(
                    "{}",
                    Strom12Fehler::Kette(KetteFehler::AusserhalbDerGueltigkeit { von, bis, jetzt })
                );
                assert!(
                    text.contains("Gueltigkeit") || text.contains("gueltig"),
                    "M7: die Meldung nennt die Gueltigkeit: {text}"
                );
            }
            other => panic!("M7: erwartet AusserhalbDerGueltigkeit, gekommen {other:?}"),
        }
    }

    /// **`wrong.host.badssl.com`**: das Zertifikat passt zu einem
    /// anderen Namen. Erwartet wird `HostnamePasstNicht`.
    #[test]
    #[ignore = "braucht das Netz; wrong.host.badssl.com"]
    fn wrong_host_liefert_den_benannten_namensfehler() {
        let speicher = system_speicher();
        let e = Tls12Strom::verbinde(
            "wrong.host.badssl.com",
            443,
            &[b"http/1.1"],
            &speicher,
            jetzt(),
        )
        .expect_err("ein fremder Hostname muss abgewiesen werden");
        assert_eq!(
            e,
            Strom12Fehler::Kette(KetteFehler::HostnamePasstNicht),
            "M7: der Hostname ist der benannte Fehler"
        );
    }

    /// **`self-signed.badssl.com`**: kein Aussteller im Systemspeicher.
    ///
    /// Erwartet wird ein benannter Kettenfehler. Welcher, haengt von
    /// der Kette ab, und der Test sagt das auch: bei
    /// `self-signed.badssl.com` ist es `KeinAussteller`, weil das
    /// Zertifikat selbstsigniert ist und kein Anker darauf zeigt.
    /// Der Test akzeptiert genau die beiden Kettenfehler, die das
    /// bedeuten koennen, und lehnt alles ab, was nach Netzfehler
    /// aussieht.
    #[test]
    #[ignore = "braucht das Netz; self-signed.badssl.com"]
    fn self_signed_liefert_einen_benannten_kettenfehler() {
        let speicher = system_speicher();
        let e = Tls12Strom::verbinde(
            "self-signed.badssl.com",
            443,
            &[b"http/1.1"],
            &speicher,
            jetzt(),
        )
        .expect_err("ein selbstsigniertes Zertifikat muss abgewiesen werden");
        match e {
            Strom12Fehler::Kette(
                KetteFehler::KeinAussteller
                | KetteFehler::EndzertifikatNichtErlaubt
                | KetteFehler::SignaturPasstNicht { .. },
            ) => {}
            other => panic!("M7: erwartet ein benannter Kettenfehler, gekommen {other:?}"),
        }
    }

    /// M7: `Debug` gibt keine Geheimnisse aus.
    ///
    /// Ohne diesen Test waere die Zeile
    /// `.field("schluessel", &self.s_schluessel)` mitten in einer
    /// Fehlersuche eingebaut worden — sie sieht harmlos aus und steht
    /// in jedem `panic!("{:?}", strom)` aus dem Panikpfad.
    #[test]
    fn debug_bucht_keine_geheimnisse() {
        // Ein eigener Sockel statt eines Fremdservers: `TcpListener`
        // auf Port 0 nimmt die Verbindung in die Warteschlange, auch
        // wenn niemand `accept` ruft. Damit braucht der Test weder
        // das Netz noch einen laufenden Server.
        let lauscher = std::net::TcpListener::bind("127.0.0.1:0").expect("Bind auf Port 0");
        let port = lauscher.local_addr().expect("Adresse").port();
        let mut s =
            Tls12Strom::verbinde_ohne_handshake("127.0.0.1", port).expect("Verbindung annehmen");
        s.s_schluessel = vec![0xAA; 16];
        s.e_schluessel = vec![0xBB; 16];
        s.s_iv = vec![0xCC; 4];
        s.e_iv = vec![0xDD; 4];
        s.restpuffer = vec![0xEE; 3];
        let t = format!("{s:?}");
        for name in ["0xaa", "0xbb", "0xcc", "0xdd", "0xee"] {
            assert!(!t.contains(name), "M7: {name} steht im Debug-Text: {t}");
        }
        // Der Aufbau selbst wird schon genannt.
        assert!(t.contains("Tls12Strom"), "M7: der Typ steht drin: {t}");
    }

    /// Das echte `NewSessionTicket` aus einem Handshake gegen
    /// badssl.com, 207 Byte (5 Kopf + 202 Inhalt).
    ///
    /// `testdaten/t15_newsessionticket.bin`, dazu die Herkunft in
    /// `testdaten/t15_newsessionticket.txt`.
    const TICKET_RECORD: &[u8] = include_bytes!("../testdaten/t15_newsessionticket.bin");

    /// M3: ein Sockel nur fuer den Bau, siehe `debug_bucht_keine_geheimnisse`.
    fn strom_fuer_test() -> Tls12Strom {
        let lauscher = std::net::TcpListener::bind("127.0.0.1:0").expect("Bind auf Port 0");
        let port = lauscher.local_addr().expect("Adresse").port();
        Tls12Strom::verbinde_ohne_handshake("127.0.0.1", port).expect("Verbindung annehmen")
    }

    /// M7: DER BUG, DEN DIESER TEST FESTHAELT: das `NewSessionTicket`
    /// liegt **im Klartext** und gehoert in den Transkript des
    /// Server-`finished`.
    ///
    /// DER WEG ZUM BEFUND, und er ist nicht naheliegend: Der Mitschnitt
    /// zeigt die Reihenfolge des Servers
    ///   ServerHello, Certificate, ServerKeyExchange, ServerHelloDone,
    ///   **NewSessionTicket**, ChangeCipherSpec, Finished
    /// Das Ticket steht also zwischen dem `finished` des Clients und
    /// dem `ChangeCipherSpec`. Ohne es im Transkript war der Hash des
    /// Server-`finished` falsch, und die Meldung hiess "der Server
    /// konnte unser finished nicht lesen".
    ///
    /// WAS DER TEST HIER PRUEFT (und was er nicht kann): dass ein
    /// Klartext-Record mit dem Ticket ueberhaupt als **eine**
    /// Handshake-Nachricht vom Typ 4 herauskommt und dass der Puffer
    /// ueber eine Recordgrenze hinweg richtig zusammensetzt. Dass der
    /// Server es wirklich in den Hash nimmt, beweisen
    /// `tls12_gegen_badssl` und die Rechnung in
    /// `testdaten/t15_newsessionticket.txt`.
    #[test]
    fn das_newsessionticket_ist_eine_klartextnachricht() {
        assert_eq!(TICKET_RECORD[0], 22, "M3: Recordtyp Handshake");
        let n = u16::from_be_bytes([TICKET_RECORD[3], TICKET_RECORD[4]]) as usize;
        assert_eq!(n, 202, "M3: 202 Byte Inhalt");
        assert_eq!(
            TICKET_RECORD.len(),
            5 + n,
            "M3: der Record ist vollstaendig"
        );

        let mut s = strom_fuer_test();
        let inhalt = &TICKET_RECORD[5..];
        let nachrichten = s
            .nachrichten_aus(inhalt)
            .expect("Nachrichten aus dem Ticket-Record");
        assert_eq!(nachrichten.len(), 1, "M7: genau eine Nachricht");
        assert_eq!(nachrichten[0][0], 4, "M7: Typ 4 ist NewSessionTicket");
        assert_eq!(
            nachrichten[0].len(),
            202,
            "M7: 4 Byte Kopf plus 198 Byte Koerper"
        );
        assert!(
            s.restpuffer.is_empty(),
            "M7: es bleibt kein Rest, alles war vollstaendig"
        );

        // GEGENPROBE: dieselbe Nachricht, ueber eine Recordgrenze
        // hinweg geliefert. Ohne den Puffer waere die erste Haelfte
        // eine "Nachricht" mit zu grosser Laenge.
        let mut s2 = strom_fuer_test();
        let mut leer: Vec<Vec<u8>> = Vec::new();
        leer.extend(s2.nachrichten_aus(&inhalt[..100]).expect("erste Haelfte"));
        assert!(
            leer.is_empty(),
            "M7: eine halbe Nachricht ist noch keine Nachricht"
        );
        leer.extend(s2.nachrichten_aus(&inhalt[100..]).expect("zweite Haelfte"));
        assert_eq!(leer.len(), 1, "M7: nach der zweiten Haelfte ist es eine");
        assert_eq!(leer[0], nachrichten[0], "M7: und zwar dieselbe");
    }

    /// M3/M4: eine Laenge im Kopf, die nicht kommen kann, wird
    /// abgelehnt statt abgewartet.
    ///
    /// Sonst wartet `lese_flug` so lange, bis der Sockel die Zeitgrenze
    /// meldet, und der Fehler heisst "Netzfehler" statt "Nachricht".
    #[test]
    fn eine_unmoegliche_nachrichtenlaenge_wird_abgelehnt() {
        let mut s = strom_fuer_test();
        // Typ 4, Laenge 0xFFFFFF = 16 MiB bei 202 Byte Inhalt.
        let kaputt = [4u8, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0];
        assert!(
            matches!(
                s.nachrichten_aus(&kaputt),
                Err(Strom12Fehler::Nachricht(Nachricht12Fehler::Laenge { .. }))
            ),
            "M4: eine Laenge von 16 MiB wird abgelehnt, nicht erwartet"
        );
    }

    /// M7: JEDE Suite hat genau EIN AEAD, und es ist das richtige.
    ///
    /// DER FEHLER, DEN DIESER TEST VERHINDERT: 0xC02C stand in der
    /// Tabelle als ChaCha20-Poly1305. 0xC02C ist aber
    /// TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384 — AES-256-GCM mit
    /// SHA-384, Schluesselblock 2*32 + 2*4 = 72 Byte statt 88. Bei
    /// dieser Suite sind Schluessel und IV ab Byte 32 um 16 Byte
    /// verschoben, und der erste verschluesselte Record des Servers
    /// scheitert an der Authentifizierung.
    ///
    /// Die Zuordnung ist an den **Namen** der Suite in der
    /// IANA-Tabelle festgeschrieben, nicht an einer Reihenfolge:
    ///   0xC02F TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256
    ///   0xC030 TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384
    ///   0xCCA8 TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256
    ///   0xC02B TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256
    ///   0xC02C TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384
    ///   0xCCA9 TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256
    #[test]
    fn jede_suite_hat_ihr_richtiges_aead() {
        let soll: [(u16, Aead, T12Hash); 6] = [
            (0xC02F, Aead::Aes128Gcm, T12Hash::Sha256),
            (0xC030, Aead::Aes256Gcm, T12Hash::Sha384),
            (0xCCA8, Aead::ChaCha20Poly1305, T12Hash::Sha256),
            (0xC02B, Aead::Aes128Gcm, T12Hash::Sha256),
            (0xC02C, Aead::Aes256Gcm, T12Hash::Sha384),
            (0xCCA9, Aead::ChaCha20Poly1305, T12Hash::Sha256),
        ];
        for (code, aead, hash) in soll {
            let (ist_aead, ist_hash) =
                suite(code).unwrap_or_else(|| panic!("Suite 0x{code:04x} fehlt in der Tabelle"));
            assert_eq!(ist_aead, aead, "Suite 0x{code:04x}: AEAD");
            assert_eq!(ist_hash, hash, "Suite 0x{code:04x}: Hash");
        }
        // Und die Schluesselblocklaengen, daraus gerechnet statt
        // abgetippt: `72` und `88` sind die beiden, die verwechselt
        // worden sind.
        assert_eq!(Aead::Aes256Gcm.schluesselblock(), 72);
        assert_eq!(Aead::ChaCha20Poly1305.schluesselblock(), 88);
        assert_eq!(Aead::Aes128Gcm.schluesselblock(), 40);
    }
}

/// added 02.10.2026 02:3x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// added 02.10.2026 02:3x: DER FUND, und er kam aus `tests/badssl_live.rs`.
/// GEMESSEN am 02.10.2026 02:1x–02:2x an `revoked.badssl.com:443`, TLS 1.2,
/// mit **unserer** Signaturliste und unseren Gruppen
/// (`-sigalgs ECDSA+SHA256:ECDSA+SHA384:PSS+SHA256:PSS+SHA384:RSA+SHA256
/// -groups X25519:P-256`), damit der Vergleich denselben Serverflug sieht:
///  * `openssl s_client` liefert `Cipher: ECDHE-ECDSA-AES128-GCM-SHA256`, ein
///    Zertifikat `CN=revoked.badssl.com` mit `Public Key Algorithm:
///    id-ecPublicKey`, `ASN1 OID: prime256v1` — also **P-256**.
///  * Derselbe `ServerKeyExchange` (`-msg`): `03 00 17 41 04 …` (Gruppe
///    `0x0017` = secp256r1, 65-Byte-Punkt) und dann `05 03` als
///    Signaturverfahren = `ecdsa_secp384r1_sha384` mit 0x47 Byte Signatur.
///  * `openssl` heilt das **stillschweigend**: „verify return:1", der Handshake
///    laeuft durch. Wir verweigern mit „Signaturverfahren 0x0503, aber das
///    Zertifikat hat eine andere Kurve".
///
/// added 02.10.2026 02:3x: WAS DAS BEDEUTET. RFC 5246 Abschnitt 7.4.1.4.1
/// verlangt, dass das Signaturverfahren zum **Zertifikatsschluessel** passt
/// (Algorithmus *und* Parameter). `ecdsa_secp384r1_sha384` auf einem
/// P-256-Schluessel ist eine andere Kurve als behauptet — obwohl die
/// Signatur mathematisch gueltig sein kann, weil der Server den richtigen
/// Schluessel hat. Uns zu lockern hiesse: eine Signatur als P-384
/// gegenpruefen, die mit P-256 gemacht wurde, also einen Hash abschneiden.
/// Das ist eine **Sicherheitsfrage**, keine Kompatibilitaetsfrage, und sie
/// gehoert nicht in eine Aufrundrunde. Bis zur Entscheidung bleibt es bei
/// „verweigern" (fail closed) — der Kompatibilitaetsverlust ist genau eine
/// badssl-Zeile und steht als Befund im Bericht, nicht als versteckter Fehler.
///
/// added 02.10.2026 02:3x: DIESER TEST HALTET DIE ENTSCHEIDUNG FEST. Ohne ihn
/// koennte jemand die Kurvenpruefung als „zu streng" loeschen, und niemand
/// wuesste, dass eine echte Messung dahintersteht.
#[test]
fn ske_nimmt_nur_eine_signatur_zur_kurve_des_zertifikats() {
    // added 02.10.2026 02:3x: DAS BLATT ist ein echtes, von openssl
    // signiertes Zertifikat aus den Testdaten (`werkzeuge/s1_szenarien.sh`),
    // ECDSA P-256 — GEMESSEN mit `openssl x509 -text`: „ASN1 OID:
    // prime256v1". Kein handgebautes Objekt: die Pruefung liest `schluessel`
    // und `kurve`, und genau die Werte wollen wir aus einem echten Parsing.
    let roh = crate::kette::pem_zerlegen(include_str!("../testdaten/s1/good.pem"))
        .expect("das S1-Blatt ist PEM-zerlegbar");
    let blatt = crate::x509::zertifikat_lesen(&roh[0]).expect("das S1-Blatt liest sich");
    // added 02.10.2026 02:3x: die Voraussetzung wird **geprueft**, nicht
    // angenommen. Ohne diese Zeilen wuerde der Test auch mit einem
    // RSA-Blatt gruen sein und damit nichts ueber Kurven sagen.
    match &blatt.schluessel {
        crate::x509::SchluesselArt::Ecdsa { kurve, .. } => assert_eq!(
            kurve,
            &vec![1u64, 2, 840, 10045, 3, 1, 7],
            "das Testblatt muss P-256 sein (1.2.840.10045.3.1.7)"
        ),
        anderes => panic!("das Testblatt muss ECDSA sein, war {anderes:?}"),
    }

    // added 02.10.2026 02:3x: DER FALL VON badssl. Signaturverfahren
    // `ecdsa_secp384r1_sha384` (0x0503) gegen ein P-256-Zertifikat. Die
    // Signatur selbst ist bewusst Muell (`30 06`, kein gueltiges `SEQUENCE`):
    // An der Kurve darf es nicht scheitern, sonst prueft der Test die
    // falsche Sache.
    let falsch = ServerKeyExchange {
        gruppe: nachricht12::gruppe::SECP256R1,
        punkt: vec![0x04, 0x00],
        sigverfahren: nachricht12::sigverfahren::ECDSA_SECP384R1_SHA384,
        signatur: vec![0x30, 0x06, 0x02, 0x01, 0x00, 0x02, 0x01, 0x00],
    };
    let e = ske_signatur_pruefen(&blatt, &falsch, b"der zu pruefende Inhalt")
        .expect_err("P-384-Angabe auf einem P-256-Zertifikat muss abgelehnt werden");
    let text = format!("{e}");
    assert!(
        text.contains("andere Kurve"),
        "es muss an der Kurve scheitern, nicht an der Signatur: kam `{text}`"
    );

    // added 02.10.2026 02:3x: DIE GEGENPROBE. Gleiches Zertifikat, passende
    // Angabe (`ecdsa_secp256r1_sha256`, 0x0403), dieselbe Muell-Signatur. Jetzt
    // darf **nicht** die Kurve die Baustelle sein — der Fehler muss aus der
    // Signaturpruefung kommen. Waere die Kurvenpruefung zu streng oder
    // verrucht, wuerde genau dieser Test rot.
    let richtig = ServerKeyExchange {
        sigverfahren: nachricht12::sigverfahren::ECDSA_SECP256R1_SHA256,
        ..falsch.clone()
    };
    let e2 = ske_signatur_pruefen(&blatt, &richtig, b"der zu pruefende Inhalt")
        .expect_err("die Muell-Signatur muss an der Signatur scheitern");
    let text2 = format!("{e2}");
    assert!(
        !text2.contains("andere Kurve"),
        "bei passender Kurve darf die Kurve nicht die Baustelle sein: kam `{text2}`"
    );
    assert!(
        text2.contains("P-256") || text2.contains("Signatur"),
        "es muss an der Signatur gescheitert sein: kam `{text2}`"
    );

    // added 02.10.2026 02:3x: UND DIE GEGENPROBE MIT ANDEREM BYTE. Der
    // Zertifikatsschluessel wird auf P-384 umgestellt (1.3.132.0.34) und die
    // Angabe bleibt `0x0503` — jetzt ist die Kurve richtig, und derselbe
    // Fehler darf **nicht** mehr „andere Kurve" sein. Damit ist die Regel
    // zweimal belegt: falsche Angabe -> Refus an der Kurve, richtige Angabe ->
    // die Pruefung geht an die Signatur.
    let mut blatt384 = blatt.clone();
    blatt384.schluessel = crate::x509::SchluesselArt::Ecdsa {
        x: vec![0x01; 48],
        y: vec![0x02; 48],
        kurve: vec![1u64, 3, 132, 0, 34],
    };
    let e3 = ske_signatur_pruefen(&blatt384, &falsch, b"der zu pruefende Inhalt")
        .expect_err("die Muell-Signatur muss auch bei P-384 scheitern");
    let text3 = format!("{e3}");
    assert!(
        !text3.contains("andere Kurve"),
        "bei P-384-Zertifikat und 0x0503 ist die Kurve richtig: kam `{text3}`"
    );
}
