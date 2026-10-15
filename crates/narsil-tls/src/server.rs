//! TLS-1.3-**Server**: ClientHello lesen, Server-Flug bauen, `finished`
//! des Clients pruefen.
//!
//! added 30.09.2026, Runde 2, TLS R1. Der Auftrag steht in
//! `team/WUNSCH_CODEX.txt` (S2: Server-Schnittstelle mit
//! Testzertifikat; S7: ALPN anbieten und die ausgehandelte Kennung
//! melden). Vorher konnte `narsil-tls` nur Verbindungen **von** einem
//! Server aufbauen, und `narsil-server` musste den Handshake selbst
//! bauen — oder gar keinen echten HTTPS-Endpunkt anbieten.
//!
//! DER UMFANG, und er ist genau der, der beweisbar ist:
//!  * TLS 1.3, `x25519` (0x001d) als Gruppe, `AES-128-GCM-SHA256`
//!    (0x1301) **und** `ChaCha20-Poly1305-SHA256` (0x1303) als Suite —
//!    beide stehen in `record::schluessel_protect`, deshalb kostet die
//!    zweite nichts.
//!  * Zertifikat: **ein** Blatt, keine Kette, kein OCSP-Stapling. Der
//!    Aufrufer gibt ein fertiges DER-Blob und den privaten Schluessel.
//!  * `CertificateVerify` mit ECDSA P-256/SHA-256 (0x0403) ueber
//!    `signatur::certificate_verify_inhalt` — dieselbe Funktion, die
//!    der Client zum Pruefen benutzt, also kein zweiter Bau.
//!  * ALPN: der Server waehlt in **seiner** Reihenfolge (RFC 7301 3.2
//!    erlaubt beides) und meldet das Ergebnis in `Ausgehandelt::alpn`.
//!
//! WAS NOCH FEHLT, und es steht hier, damit niemand es erraten muss:
//!  * **Kein serverseitiges `hello_retry_request`.** Fehlt der
//!    Schluesselteil fuer `x25519`, gibt es `KeinPassenderSchluesselteil`
//!    zurueck, statt ein HelloRetryRequest zu schicken. Clientseitig
//!    gibt es `nachricht.rs::hrr`; der Server braucht es, sobald er
//!    `secp256r1` anbieten soll.
//!  * **Keine Sitzungswiederaufnahme** (`NewSessionTicket`, A1).
//!  * **Kein Alert-Paket** bei `KeinAlpnPasst` oder
//!    `KeinPassenderSchluesselteil`. RFC 8446 6.2 verlangt
//!    `no_application_protocol` (120) bzw. `handshake_failure` (40).
//!    Ohne Alert beendet sich die Verbindung an einer Stelle, an der
//!    der Client mit einem "unerwartetes Ende" statt mit der Ursache
//!    aufwartet. Das ist als `TODO` markiert und keine Absicht.

use std::io::{Read, Write};

use crate::nachricht::{ClientHello, LEGACY_VERSION, VERSION_TLS13, typen};
use crate::record::{self, Schluesselplan, Suite};
use crate::signatur::{VORLAGE_SERVER, certificate_verify_inhalt};
use crate::vektor::Schreiber;
use crate::x25519;
use crate::zufall;

/// Wie viele Bytes der Server bei einem Handshake hoechstens liest.
///
/// added 30.09.2026, Runde 2, M3. DER WERT, und er ist **keine
/// gerundete Zahl**: `4 * record::MAX_EINLESEN`. Ein Handshake
/// braucht genau zwei Records vom Client (der ClientHello, der bei
/// grossen Zertifikatsketten und zwei `key_share` ueber mehrere
/// Records verteilt sein kann, und das `finished`), und die
/// Faktorzahl 4 laesst einem Client, der alles in Ein-Byte-Records
/// schickt, den Raum. `record::MAX_EINLESEN` ist selbst die Grenze
/// eines einzelnen Records — eine eigene Zahl hier waere eine zweite
/// Wahrheit, die auseinanderlaufen kann, wenn man eines der beiden
/// aendert.
///
/// OHNE DIESE ZAHL waere ein `read` in einer Schleife die einzige
/// Begrenzung, und die ist der Inhalt der Schleife: eine Leitung, die
/// endlos schickt, wuerde den Server Speicher und Zeit fressen, ohne
/// dass irgendwo ein Fehler entsteht.
pub const MAX_GELESEN: usize = 4 * record::MAX_EINLESEN;

/// Wie viele ALPN-Protokolle der Server anbietet.
///
/// added 30.09.2026. `2^8`-Eintraege sind laut RFC 7301 3.1 moeglich,
/// aber ein Server, der 256 anbietet, ist entweder ein Test oder
/// schlecht konfiguriert. Fuer HTTP/2 + http/1.1 (S7) reichen zwei.
pub const MAX_ALPN_ANGEBOTE: usize = 8;

/// Warum der Server-Handshake nicht durchgelaufen ist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerFehler {
    /// Die Leitung wurde zu Ende gelesen, bevor eine Nachricht kam.
    Zu,
    /// Es kam etwas anderes als ein ClientHello.
    KeinClientHello {
        war: u8,
    },
    /// Der Client bietet keine der beiden Suites an, die wir sprechen.
    KeineSuite,
    /// Der Client schickt keinen Schluesselteil fuer `x25519`.
    ///
    /// added 30.09.2026. DER BENENNTE FEHLER STATT EINES
    /// HELLORETRYREQUESTS: die Norm verlangt in diesem Fall ein HRR
    /// (RFC 8446 4.1.4), und wir haben es serverseitig nicht. Ein
    /// Fehler mit Namen ist ehrlicher als ein HRR mit falschem
    /// Transkript, und der Client sieht an der Fehlermeldung, woran es
    /// liegt.
    KeinPassenderSchluesselteil {
        gruppe: u16,
    },
    /// Der Client nennt ALPN-Protokolle, von denen wir keins anbieten.
    ///
    /// TODO(Alert): RFC 8446 6.2 verlangt `no_application_protocol`
    /// (120). Noch nicht gebaut; siehe Modul-Kopf.
    KeinAlpnPasst,
    /// Das Zertifikat oder der private Schluessel passt nicht zusammen.
    AngebotUnbrauchbar(&'static str),
    /// Kein Zufall vom Betriebssystem.
    Zufall,
    /// Der Schluesseltausch ergab nichts Brauchbares.
    Schluesseltausch,
    /// Eine Nachricht liess sich nicht dekodieren, mit dem Grund.
    ///
    /// added 30.09.2026, Runde 2, TLS R1c (Nachrueckung aus dem
    /// openssl-Lauf). DER TYP TRAGT DEN TEXT, und das ist keine
    /// Kosmetik: die Variante war zuerst `Nachricht` ohne Text, und
    /// `openssl s_client` liess sich damit nicht weiterhelfen — der
    /// Server meldete `LESEN-FEHLER: ein Record liess sich nicht lesen
    /// oder schreiben`, und das gilt fuer sechs verschiedene Ursachen
    /// (`record::RecordFehler` hat sieben). Es ist derselbe Fehler wie
    /// bei `AnteileAusserhalb`: ein Fehlername, der mehrere Ursachen
    /// zusammenfasst, kostet die Fehlersuche.
    /// added 02.10.2026 03:3x UTC (Projektlabel 02.10.2026), Rolle TLS.
    /// added 02.10.2026 03:3x: DIE NACHRICHT PASSTE NICHT IN DEN RECORD, und
    /// sie ist **nicht** in `Nachricht(String)` — die sagt nichts darüber, wie
    /// gross etwas war. Vorher stand hier an vier Stellen ein
    /// `.expect("... passt in einen Record")`, also ein **Panic** auf einem
    /// Codepfad, den ein Peer nicht kippen kann (alle vier Groessen kommen aus
    /// unserer eigenen Konfiguration: Zertifikat, Server-Random, PSK-Index,
    /// ALPN). GEMESSEN, wie weit der Weg wirklich ist: `MAX_NACHRICHT` ist
    /// `(1 << 24) - 1`, also 16 777 215 Byte (`record.rs:33`) — fuer eine
    /// `Certificate`-Nachricht muesste die Kette also ueber 16 MiB gross sein.
    /// Trotzdem ist `expect` im Bibliothekscode verboten, und der Befund ist
    /// billig zu beheben: der Fehler nennt Typ, Ist und Erlaubt, und ein
    /// Test baut die Nachricht wirklich zu gross (`nachricht_zu_gross_ist_ein
    /// _benannter_fehler` in `tests/server_nachricht_grenze.rs`).
    NachrichtZuGross {
        typ: u8,
        war: usize,
        erlaubt: usize,
    },
    /// added 02.10.2026 03:5x UTC (Projektlabel 02.10.2026), Rolle TLS.
    /// added 02.10.2026 03:5x: Der Schluesselplan liess sich nicht rechnen.
    /// Das ist die Form, in der der Aufruf `plan.handshakephase()` seit heute
    /// endet — vorher stand dort ein `expect` **in** `record.rs`, und dieses
    /// `expect` war ein Panic mitten im Handshake.
    Schluesselplan(crate::record::RecordFehler),
    Nachricht(String),
    /// Ein Record liess sich nicht lesen oder schreiben, mit dem Grund.
    Record(String),
    /// Das `finished` des Clients stimmt nicht.
    FinishedFalsch,
    /// Es kam eine Nachricht, die hier nicht erwartet wird.
    Unerwartet {
        war: u8,
    },
    /// Die eingebaute Grenze wurde erreicht.
    ZuViel(&'static str),
    /// Fehler der Leitung darunter.
    Netz(String),
    /// Die Zeitgrenze der Leitung ist abgelaufen, ohne dass ein Byte kam.
    ///
    /// added 30.09.2026, Runde 2, TLS R1c. DER EIGENE FALL, und er ist
    /// eingefuehrt, weil `EAGAIN` unter Linux kein Netzfehler ist: ein
    /// abgelaufenes `SO_RCVTIMEO` kommt als `os error 11` zurueck. Ohne
    /// diesen Fall meldete das Beispielprogramm "Netzfehler: Resource
    /// temporarily unavailable" fuer eine Sache, die keine ist — wer
    /// das Beispiel als Skript benutzt, haette die eigenen 15 Sekunden
    /// fuer einen Fehler des Gegners gehalten.
    LesezeitUeberschritten,
}

impl std::fmt::Display for ServerFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerFehler::Zu => write!(f, "die Leitung war zu Ende, bevor eine Nachricht kam"),
            ServerFehler::KeinClientHello { war } => {
                write!(f, "die erste Nachricht war {war}, kein ClientHello (1)")
            }
            ServerFehler::KeineSuite => write!(f, "keine gemeinsame Cipher Suite (0x1301/0x1303)"),
            ServerFehler::KeinPassenderSchluesselteil { gruppe } => {
                write!(
                    f,
                    "kein x25519-Schluesselteil im ClientHello, Gruppe war {gruppe:#06x}"
                )
            }
            ServerFehler::KeinAlpnPasst => {
                write!(f, "keines der ALPN-Protokolle des Clients wird angeboten")
            }
            ServerFehler::AngebotUnbrauchbar(was) => {
                write!(f, "das Serverangebot ist unbrauchbar: {was}")
            }
            ServerFehler::Zufall => write!(f, "das Betriebssystem lieferte keine Zufallsbytes"),
            ServerFehler::Schluesseltausch => write!(f, "der Schluesseltausch ist unbrauchbar"),
            ServerFehler::NachrichtZuGross { typ, war, erlaubt } => write!(
                f,
                "die Nachricht {typ} waere {war} Byte gross, erlaubt sind {erlaubt}"
            ),
            ServerFehler::Schluesselplan(e) => {
                write!(f, "der Schluesselplan liess sich nicht rechnen: {e}")
            }
            ServerFehler::Nachricht(was) => {
                write!(f, "eine Handshake-Nachricht liess sich nicht lesen: {was}")
            }
            ServerFehler::Record(was) => {
                write!(f, "ein Record liess sich nicht lesen oder schreiben: {was}")
            }
            ServerFehler::FinishedFalsch => write!(f, "das finished des Clients stimmt nicht"),
            ServerFehler::Unerwartet { war } => write!(f, "unerwartete Handshake-Nachricht {war}"),
            ServerFehler::ZuViel(was) => write!(f, "zu viel: {was}"),
            ServerFehler::Netz(was) => write!(f, "Netzfehler: {was}"),
            ServerFehler::LesezeitUeberschritten => {
                write!(
                    f,
                    "Lesezeit ueberschritten, der Gegner hat nichts geschickt"
                )
            }
        }
    }
}

impl From<record::RecordFehler> for ServerFehler {
    fn from(e: record::RecordFehler) -> ServerFehler {
        ServerFehler::Record(e.to_string())
    }
}

impl From<record::NachrichtFehler> for ServerFehler {
    // `record::NachrichtFehler` hat kein `Display`, nur `Debug` — es ist
    // eine eigene kleine Fehlerart in `record.rs` (KopfZuKurz,
    // Abgeschnitten, LaengeZuGross). `{:?}` zu nehmen ist hier richtig
    // und wird auch so benannt: die Fehlermeldung nennt die **Variante**,
    // nicht einen Text, den es nicht gibt.
    fn from(e: record::NachrichtFehler) -> ServerFehler {
        ServerFehler::Nachricht(format!("{e:?}"))
    }
}

/// `ClientHello::dekodiere` liefert `nachricht::NachrichtFehler` — nicht
/// `record::NachrichtFehler`. Zwei gleichnamige Fehler in zwei Modulen,
/// und der Compiler brauchte zwei Versuche, bis ich den Unterschied
/// gesehen habe; diese Zeile ist die Erinnerung daran.
impl From<crate::nachricht::NachrichtFehler> for ServerFehler {
    fn from(e: crate::nachricht::NachrichtFehler) -> ServerFehler {
        ServerFehler::Nachricht(e.to_string())
    }
}

/// Eine Leitung, die lesen und schreiben kann.
///
/// added 30.09.2026. DER GRUND FUER DAS EIGENE MERKMAL: `&mut dyn
/// Read + Write` gibt es in Rust nicht — es gibt `&mut dyn Read` **oder**
/// `&mut dyn Write`. Der Trick ist ein Sammer-Merkmal mit einer
/// leeren Implementierung fuer alles, was beides kann; dadurch ist der
/// Server ueber `TcpStream`, `&mut [u8]` (im Test) und
/// `BufReader<TcpStream>` benutzbar, ohne Generizitaet im Rest.
pub trait LeseSchreiben: Read + Write {}
impl<T: Read + Write> LeseSchreiben for T {}

/// Was der Server anbietet.
///
/// added 30.09.2026. DER GRUND FUER EINE EIGENE STRUKTUR STATT
/// VIELER PARAMETER: `narsil-server` baut daraus einmal beim Start etwas
/// und reicht es an jede angenommene Verbindung weiter. Und die drei
/// Felder sind genau die drei Dinge, die ein TLS-Server wirklich
/// braucht — ein viertes (ein Sitzungsspeicher) kommt mit A1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerAngebot {
    /// Das Zertifikat in DER-Form (ein Blatt).
    pub zertifikat: Vec<u8>,
    /// Der private Schluessel zu `zertifikat`, 32 Byte (P-256).
    pub privat: Vec<u8>,
    /// Die ALPN-Protokolle in der Reihenfolge der Vorliebe des Servers.
    pub alpn: Vec<Vec<u8>>,
}

impl ServerAngebot {
    /// added 03.10.2026 02:6x, R48. **TESTBEQUEMLICHKEIT**, und sie ist
    /// `#[cfg(test)]`, damit die oeffentliche Struct unveraendert bleibt: nimmt
    /// das Zertifikat aus `testdaten/` und den Schluessel aus
    /// `testschluessel_p256.hex`.
    #[cfg(test)]
    pub fn neu_test(zertifikat: Vec<u8>, alpn: Vec<Vec<u8>>) -> Self {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/testdaten/testschluessel_p256.hex"
        ))
        .unwrap_or_default();
        let sauber: String = text.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        let privat = (0..sauber.len() / 2)
            .map(|i| u8::from_str_radix(&sauber[i * 2..i * 2 + 2], 16).unwrap_or(0))
            .collect();
        ServerAngebot {
            zertifikat,
            privat,
            alpn,
        }
    }

    /// Prueft das Angebot, **bevor** eine Verbindung angenommen wird.
    ///
    /// added 30.09.2026. DER WEG ZUR PRUEFUNG: `zertifikat_lesen` aus
    /// `x509` liest das Zertifikat und gibt den SPKI zurueck. Damit ist
    /// nicht nur geprueft, dass das DER **parsebar** ist, sondern dass
    /// der oeffentliche Schluessel darin steht. Der private Schluessel
    /// wird gegen diesen SPKI geprueft, indem er einmal skalar
    //  multipliziert wird — ein Server, dessen Schluessel nicht zum
    // Zertifikat passt, scheitert dann hier und nicht erst im
    /// `CertificateVerify`, wo der Client die Signatur nicht pruefen
    /// kann und nur „handshake failed" sieht.
    pub fn pruefen(&self) -> Result<(), ServerFehler> {
        if self.zertifikat.is_empty() {
            return Err(ServerFehler::AngebotUnbrauchbar("leeres Zertifikat"));
        }
        if self.privat.len() != 32 {
            return Err(ServerFehler::AngebotUnbrauchbar(
                "der private Schluessel hat nicht 32 Byte",
            ));
        }
        if self.alpn.len() > MAX_ALPN_ANGEBOTE {
            return Err(ServerFehler::AngebotUnbrauchbar("zu viele ALPN-Protokolle"));
        }
        for protokoll in &self.alpn {
            if protokoll.is_empty() || protokoll.len() > crate::nachricht::MAX_ALPN_EINTRAG {
                return Err(ServerFehler::AngebotUnbrauchbar(
                    "ALPN-Eintrag hat 0 oder zu viele Byte",
                ));
            }
        }
        Ok(())
    }
}

/// Was der Server nach dem Handshake weiss.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ausgehandelt {
    /// Die QUIC-Transportparameter des **Clients**, unveraendert, oder
    /// `None`, wenn er keine geschickt hat.
    ///
    /// added 30.09.2026, Runde 2, TLS R2 (Wunsch DEEPSEEK). DER GRUND,
    /// DASS DER SERVER SIE DEM AUFRUFER GIBT: ein QUIC-Server braucht
    /// daraus `initial_source_connection_id` (RFC 9000 18.2, ID 0x0f),
    /// um seine **eigenen** Parameter zu bauen, und der Aufrufer ist
    /// die einzige Stelle, die den QUIC-Zustand hat. Der Server parst
    /// den Block nicht — das gehoert in `narsil-quic`.
    pub quic_parameter_vom_client: Option<Vec<u8>>,
    /// Die ALPN-Kennung, die **der Server** gewaehlt hat, oder `None`,
    /// wenn der Client keine angeboten hat.
    ///
    /// added 30.09.2026 fuer S7 (`team/WUNSCH_CODEX.txt`): `narsil-server`
    /// darf HTTP/2 auf einem HTTPS-Port erst einschalten, wenn es
    /// **diesen** Wert sieht. `None` heisst: der Client hat kein ALPN
    /// geschickt, also HTTP/1.1 nach RFC 7301 3.2.
    pub alpn: Option<Vec<u8>>,
    /// Die benutzte Cipher Suite.
    pub suite: Suite,
    /// Die benutzte Gruppe.
    pub gruppe: u16,
    /// Das `client application traffic secret` — damit werden die
    /// Daten des Clients entschluesselt.
    pub lesen: [u8; 32],
    /// Wie viele Anwendungs-Records der Server **schon** gesendet hat.
    ///
    /// added 01.10.2026 13:9x, R4z6. **DER SEQUENZZAHL-FEHLER, und er ist
    /// ein echter Fehler, kein Testproblem.** `handshake_mit_tickets`
    /// schickt die `NewSessionTicket` mit
    /// `record::schluessel_protect(…, seq = 0, …)` (Zeile 1134 ff.). Wer
    /// danach `AnwendungsStrom::neu` aufruft, bekommt `seq: 0` — und
    /// verschluesselt den naechsten Record mit **derselben** Nonce wie
    /// das Ticket.
    ///
    /// GEMESSEN am 01.10.2026, bevor dieses Feld existierte: der Browser
    /// meldete `Verschluesselung (TLS_AES_128_GCM_SHA256, lesen)` beim
    /// Lesen einer Antwort, die der Server **richtig** verschluesselt
    /// hatte. Bei AES-GCM ist eine wiederholte Nonce kein kosmetischer
    /// Fehler, sondern ein Verlust der Authentizitaetsgarantie — und
    /// genau deshalb wird das hier behoben und nicht „der Client soll
    /// das pruefen".
    ///
    /// DER WERT, und er ist `1`, sobald ein Ticket gesendet wurde: der
    /// Ticket-Record hat Sequenz **0** benutzt, der naechste also 1.
    pub gesendete_records: u64,
    /// Wurde die Sitzung **wiederaufgenommen**?
    ///
    /// added 01.10.2026 13:2x, R4z4. **WARUM DAS FELD FEHLTE:** Ohne es
    /// kann niemand unterscheiden, ob eine zweite Verbindung neu
    /// verhandelt hat oder den Wiederaufnahmepfad genommen hat. Der
    /// Aufrufer sieht beides gleich: eine vollstaendige Kette und einen
    /// Handshake. Genau daraus entstand eine Testluecke — der Browser-Test
    /// in `narsil-net` konnte nur messen, dass der Client ein Ticket
    /// **anbietet**, nicht dass der Server es **annimmt**.
    ///
    /// DIE WAHRHEIT stand in `handshake_mit_tickets` als `let
    /// wiederaufgenommen = wieder_psk.is_some();` — die Variable gab es,
    /// sie wurde nur nicht herausgegeben.
    pub wiederaufgenommen: bool,
    /// Das `server application traffic secret` — damit werden die
    /// eigenen Daten verschluesselt.
    pub schreiben: [u8; 32],
    /// Was der Handshake-Leser **ueber** gelesen hat: der Rest des
    /// Puffers nach dem `finished` des Clients.
    ///
    /// added 30.09.2026, Runde 2, TLS R1c. DER ANLASS, und es ist ein
    /// echter Fehler gewesen, gefunden am 30.09.2026 ~23:59 am
    /// Rundlauf gegen den echten Client (`tests/rundlauf.rs`): der Leser
    /// liest bis 4096 Byte auf einmal, und ein Client, der sein
    /// `finished` und seinen ersten Anwendungsrecord **zusammen**
    /// schickt, landet damit in einem TCP-Segment. Das `finished`
    /// wurde dekodiert, der Anwendungsrecord blieb im Puffer — und der
    /// Puffer gehoert zu `LeseKette`, einem lokalen Objekt in
    /// `handshake`, das beim Zurueckgeben **weggeworfen** wurde. Danach
    /// wartete der Server auf Daten, die schon gelesen waren.
    ///
    /// DER GEGENWEG, und er ist nicht "ein kleinerer Lesevorgang": ein
    /// `read` mit einem Ein-Byte-Puffer wuerde das Problem verschieben,
    /// nicht loesen (und den Handshake auf einen Syscall je Byte
    /// zurueckbringen). Der Rest gehoert dem Aufrufer, und
    /// `AnwendungsStrom` legt ihn an den Anfang seines Rohpuffers.
    pub rest_bytes: Vec<u8>,
}

/// Ein TLS-1.3-Server.
#[derive(Debug, Clone)]
pub struct TlsServer {
    angebot: ServerAngebot,
    /// added 03.10.2026, 02:0x, R48. **DIE WEITERE KETTE**, nach dem Blatt.
    ///
    /// WARUM DAS EIN FELD IM **Server** ist und kein Feld in `ServerAngebot`:
    /// GEMESSEN (03.10.2026 02:0x) wird `ServerAngebot` ueberall per
    /// Struct-Literal gebaut — auch in **drei fremden Dateien**
    /// (`narsil-server/src/tls.rs`, `narsil-server/tests/tls_records_gegenbeweis.rs`
    /// und zwei `narsil-net/tests/`-Dateien). Ein neues Feld in `ServerAngebot`
    /// wuerde deren Bau brechen, und fremde Dateien aendere ich nicht. Darum
    /// bleibt `ServerAngebot` unangetastet und die Kette kommt ueber einen
    /// **zweiten Konstruktor** herein.
    ///
    /// DER NUTZEN, und er ist der Grund fuer Nr. 48: die OCSP-Auswertung und
    /// die must-staple-Pruefung verlangen beide `ganze_kette.len() >= 2`
    /// (`strom.rs`). Mit einem Blatt konnten diese Strecken gegen eigenen Server
    /// **nie** laufen; mit einem Aussteller koennen sie es.
    kette: Vec<Vec<u8>>,
    /// added 03.10.2026 03:3x, R49. **DIE OCSP-STAPELUNG** (DER), oder `None`.
    ///
    /// added 03.10.2026 03:3x: **DAS FORMAT, und es ist eine Verwechslungsgefahr.**
    /// RFC 8446 4.4.2.1 woertlich: „the body of the `status_request` extension
    /// from the server **MUST** be a `CertificateStatus` structure as defined in
    /// [RFC6066]“. Und `CertificateStatus` ist **ein Statusbyte zuerst** — `ocsp`
    /// heisst `1` — und **danach** die Nutzlast als `opaque<1..2^24-1>`, also
    /// drei Byte Laenge. Ich hatte zuerst vermutet, unser **Client** lese das
    /// falsch; nach dem Nachsehen in der Norm war es **richtig** (`nachricht.rs`
    /// verlangt `inhalt[0] == 1`). Der Verdacht ist also in PROBLEME nichts
    /// geworden — so ist das Nachsehen belohnt.
    ocsp: Option<Vec<u8>>,
}

impl TlsServer {
    /// Die `CertificateVerify`-Nachricht, signiert ueber den Transkript
    /// **bis einschliesslich** `Certificate` (RFC 8446 4.4.3).
    ///
    /// added 01.10.2026, R4q. DER GRUND FUER EINE METHODE: der Block stand
    /// mit 20 Zeilen **inline** im Handshake, und die Wiederaufnahme braucht
    /// ihn **nicht**. Ein bedingter Zweig um 20 Zeilen mit eingebetteten
    /// Kommentaren ist unlesbar; eine Methode daneben ist es nicht, und der
    /// Handshake liest sich als das, was er ist: Kette, Signatur, Fertig.
    fn certificate_verify(
        &self,
        transkript_bis_zertifikat: &[u8; 32],
    ) -> Result<Vec<u8>, ServerFehler> {
        let inhalt = certificate_verify_inhalt(VORLAGE_SERVER, transkript_bis_zertifikat)
            .map_err(|_| ServerFehler::AngebotUnbrauchbar("CertificateVerify-Inhalt zu lang"))?;
        // DER HASH UEBER DEN INHALT, und nicht ueber den Transkript:
        // `certificate_verify_inhalt` liefert die 64 Byte Kontext plus Hash,
        // und signiert wird der **SHA-256** davon. Das ist dieselbe
        // Reihenfolge, mit der `cv_gueltig` auf der Client-Seite prueft.
        let (r, s) = crate::ecdsa::ecdsa_p256_signieren_sha256(
            &self.angebot.privat,
            &crate::sha2::sha256(&inhalt),
        )
        .map_err(|_| ServerFehler::AngebotUnbrauchbar("Signieren nicht moeglich"))?;
        // DER FORM, und nicht `r || s`: `cv_gueltig` auf der Client-Seite
        // liest ueber `kette::ecdsa_teile_oeffentlich` ein
        // `SEQUENCE { INTEGER r, INTEGER s }`. Mit den nackten 64 Byte
        // bekommt der Client eine unlesbare Signatur und meldet
        // `handshake failed` — ohne dass jemand den Grund sieht. Die
        // Begruendung steht bei `signatur::ecdsa_der_signatur`.
        let signatur = crate::signatur::ecdsa_der_signatur(&r, &s)
            .map_err(|_| ServerFehler::AngebotUnbrauchbar("Signatur nicht kodierbar"))?;
        baue_certificate_verify(0x0403, &signatur)
    }

    /// Neuer Server aus einem Angebot. Prueft das Angebot sofort.
    pub fn neu(angebot: ServerAngebot) -> Result<TlsServer, ServerFehler> {
        // added 03.10.2026 02:1x, R48: der alte Weg ist **unveraendert** der
        // Normalfall (ein Blatt) — sonst gaebe es eine Aenderung im Verhalten
        // ohne Ursache.
        Self::neu_mit_kette_und_ocsp(angebot, Vec::new(), None)
    }

    // added 04.10.2026 01:2x, R67. **`neu_mit_kette` GELOESCHT** — GEMESSEN: `grep`
    // ueber alle Crates, **ein** Treffer, die Definition. Es war ein Zweizeiler ohne
    // Faehigkeit: `Self::neu_mit_kette_und_ocsp(angebot, kette, None)`, und genau das
    // ist auch `Self::neu_mit_kette_und_ocsp(angebot, kette, None)` — dieselbe
    // Rechnung, zweimal im Namen. Die Hülle mit demOCSP-Feld bleibt, weil R49 sie
    // mit einer Messung gebaut hat (must-staple: Zertifikat verlangt, Server
    // liefert).

    /// Wie [`TlsServer::neu_mit_kette`], **plus** eine OCSP-Antwort, die im
    /// Blatt als `status_request`-Erweiterung mitgeschickt wird.
    ///
    /// added 03.10.2026 03:3x, R49. **WOFUER DAS DA IST**, und es ist der
    /// dritte Fall von must-staple: das Zertifikat verlangt die Stapelung **und
    /// der Server liefert sie** — dann muss die Verbindung **stehen**. Bisher
    /// waren nur zwei Faelle messbar (R48: kein Stapel → abgelehnt; kein
    /// Merkmal → steht).
    pub fn neu_mit_kette_und_ocsp(
        angebot: ServerAngebot,
        kette: Vec<Vec<u8>>,
        ocsp: Option<Vec<u8>>,
    ) -> Result<TlsServer, ServerFehler> {
        angebot.pruefen()?;
        Ok(TlsServer {
            angebot,
            kette,
            ocsp,
        })
    }

    /// Fuehrt den Server-Handshake ueber eine Leitung aus.
    ///
    /// added 30.09.2026, Runde 2, TLS R1 (S2). Der Ablauf ist
    /// RFC 8446 Abschnitt 4.1 bis 4.4.4:
    ///  1. `ClientHello` lesen und dekodieren.
    ///  2. Suite und Gruppe waehlen, `x25519`-Geheimnis bauen.
    ///  3. `ServerHello` **im Klartext** schicken (das ist die einzige
    ///     unverschluesselte Nachricht nach dem ClientHello, RFC 8446
    ///     5.1).
    ///  4. Schluesselplan aus ClientHello..ServerHello, dann
    ///     `EncryptedExtensions` (mit ALPN), `Certificate`,
    ///     `CertificateVerify` und `Finished` — alle vier **in einem**
    ///     verschluesselten Record, wie es die meisten Server tun.
    ///  5. Das `finished` des Clients lesen und pruefen.
    ///
    /// DER TRANSKRIPT-HASH, und er ist die Stelle, an der ein
    /// Handshake am billigsten zu ruinieren ist: `CertificateVerify`
    /// steht ueber `ClientHello..Certificate`, das `finished` des
    /// Servers ueber `ClientHello..CertificateVerify`, und das
    /// `finished` des Clients ueber `ClientHello..finished des
    /// Servers`. Deshalb wird hier **nicht** ein Hash nebenher
    /// mitgeschleppt, sondern `plan.transkript_hash()` **an der
    /// jeweiligen Stelle** abgefragt — ein zweiter Transkript waere
    /// die wahrscheinlichste Quelle eines Fehlers, der sich als
    /// „Verschluesselung kaputt" zeigt.
    pub fn handshake(&self, leitung: &mut dyn LeseSchreiben) -> Result<Ausgehandelt, ServerFehler> {
        // DER WEG OHNE WIEDERAUFNAHME, und er ist derselbe wie vor
        // 02.10.2026: ein frischer, leerer Speicher, kein Angebot, also
        // ein vollstaendiger Handshake.
        //
        // DER SPEICHER, und warum er hier **nicht** am Server haengt: ein
        // `TlsServer` entsteht je Verbindung, die Tickets muessen aber
        // **verbindungenuebergreifend** leben — sonst kaeme kein Client
        // je wieder. Der Speicher gehoert dem Prozess, und der Prozess
        // reicht ihn herein. Das ist dasselbe Muster wie beim Client, wo
        // `speicher: &Vertrauensspeicher` uebergeben und nicht gehalten
        // wird.
        let mut keine = crate::ticket::Tickets::neu();
        self.handshake_mit_tickets(leitung, &mut keine, false)
    }

    /// Nimmt ein `pre_shared_key`-Angebot an — oder lehnt es **ohne**
    /// Fehler ab.
    ///
    /// added 02.10.2026, R4d Stufe 2. DER RÜCKGABEWERT IST `Result<PSK,
    /// &'static str>`, und `Err` bedeutet hier **nicht** „Handshake
    /// abweisen", sondern „wir nehmen die Wiederaufnahme nicht, mach einen
    /// vollstaendigen". Das ist ein Unterschied, der sich beim Aufrufer
    /// zeigt: `Err` fuehrt zu `selected_identity: None` und einem
    /// normalen Handshake, nicht zu einer Alert.
    ///
    /// DER WEG, und er ist die Norm in ihrer Reihenfolge (RFC 8446
    /// 4.2.11):
    ///
    /// (1) `psk_dhe_ke` muss angeboten sein (4.2.9). Ohne den Modus gibt
    ///     es keine Wiederaufnahme **mit** Schluesselaustausch, und
    ///     `psk_ke` implementieren wir nicht — es waere eine zweite
    ///     Schluesselberechnung, die niemand braucht.
    /// (2) Das Angebot ueberhaupt lesen (`psk_lese`). Ein kaputtes Angebot
    ///     ist kein Angriff, sondern ein Fehler des Clients: vollstaendiger
    ///     Handshake.
    /// (3) Die erste Identitaet im Speicher **suchen** (`psk_fuer`).
    /// (4) Den Binder pruefen: erwarteten rechnen, in konstanter Zeit
    ///     vergleichen (`konstant_gleich`).
    ///
    /// WARUM NUR DIE ERSTE IDENTITAET, und es ist eine Vereinfachung mit
    /// Namen: RFC 8446 4.2.11 erlaubt mehrere, und `selected_identity`
    /// nennt gerade, welche gewaehlt wurde. Wir nehmen Index 0 oder
    /// nichts. Fuer einen Browser mit einem Ticket je Host ist das genau
    /// der Fall; wer mehrere anbieten will, braucht die Schleife hier.
    ///
    /// DER BINDER WIRD **UEBER DEN GEKUERZTEN CLIENTHELLO** RECHNET, und
    /// das ist der einzige Ort, an dem das auf der Serverseite passiert.
    /// `transkript_fuer_binder` schneidet nach 4.2.11.2 ab — vor der
    /// Binderliste **und** vor ihrer Laengenangabe. Die 477 Byte aus
    /// RFC 8448 sind der Beleg fuer genau diesen Schnitt.
    ///
    /// DER GRUND FUER DEN RUECKGABETYP `&'static str`: der Aufrufer
    /// braucht keinen Grund, um ihn weiterzureichen (es gibt keine
    /// Alert), aber ein Test schon. Die Strings stehen darum an der
    /// Fundstelle und nicht in einer Fehleraufzaehlung.
    ///
    /// DER RUECKGABETYP, drei Faelle, und jeder hat eine eigene Form
    /// (added 01.10.2026 22:5x UTC, Projektlabel 02.10.2026, Rolle TLS):
    ///
    /// * `Ok(None)` — **kein Angebot**. Der Client hat kein
    ///   `pre_shared_key` geschickt (RFC 8446 4.2.11: „If the ClientHello
    ///   does not contain a pre_shared_key extension, the server proceeds
    ///   with the full handshake"). Das ist der normale erste Kontakt.
    /// * `Ok(Some(binder))` — Angebot **angenommen**, der Binder stimmt.
    /// * `Err(grund)` — Angebot **da**, aber nicht brauchbar (unbekannte
    ///   Identitaet, falscher Binder, kaputter Block). Auch das fuehrt zum
    ///   vollen Handshake, nur mit einer Beobachtung auf stderr.
    ///
    /// WARUM DAS EIN EIGENER TYP UND NICHT `Ok(Vec::new())` ist: `Ok`
    /// bedeutet an dieser Stelle „angenommen", und der Aufrufer macht daraus
    /// `selected_identity = Some(0)` und `wieder_psk = Some(paar)`. Ein
    /// leeres `Ok` haette dem Server mitgeteilt, es gaebe eine
    /// Wiederaufnahme — mit **leerem** PSK. Das ist kein Fehler, sondern ein
    /// Schluessel aus dem Nichts. Die erste Fassung des Fixes (22:5x) hatte
    /// genau diesen Zustand gebaut; `grep -n "wieder_psk.is_some()"
    /// server.rs` (Zeile 816) zeigt, worauf es ankommt.
    fn angebot_annehmen(
        &self,
        client: &ClientHello,
        clienthello: &[u8],
        tickets: &crate::ticket::Tickets,
    ) -> Result<Option<Vec<u8>>, &'static str> {
        // added 01.10.2026 22:5x UTC (Projektlabel 02.10.2026), Rolle TLS.
        // DIE REIHENFOLGE WAR FALSCH — und der Kommentar darueber sagte es
        // sogar. Er schrieb: „Ohne `pre_shared_key` gibt es nichts zu
        // pruefen, und das kommt vor der Modusfrage". Im Code kam die
        // Modusfrage zuerst.
        //
        // GEMESSEN 01.10.2026 22:4x, `narsil-net`-Test
        // `alpn_client::der_browser_weg_bietet_h2_nur_wenn_er_es_sprechen_kann`
        // in einem von drei Laeufen, mit dieser Meldung auf dem Server:
        //
        //   ANGEBOT ABGELEHNT: das Angebot ist kaputt (Identitaeten: 0,
        //   psk_modi: [1])
        //
        // `psk_modi: [1]` heisst: der Client **hat** `psk_dhe_ke`
        // angeboten. `Identitaeten: 0` heisst: er hat **kein**
        // `pre_shared_key` geschickt — es gibt fuer diesen Host noch kein
        // Ticket. Das ist der normale erste Kontakt und nach RFC 8446 4.2.11
        // **kein Fehler**: „If the ClientHello does not contain a
        // pre_shared_key extension, the server proceeds with the full
        // handshake." Unsere Abfrage las stattdessen `psk_lese` auf leere
        // Bytes und meldete das Angebot als kaputt — und der Server brach
        // den Handshake ab, den `narsil-server/examples/serve_https.rs`
        // (Zeile 431) seit dem 02.10. 10:05 als nicht lauffaehig meldet und
        // `team/log/RUHT_TLS` Punkt 3 festhält.
        //
        // DER WEG NUN, in der Reihenfolge des Kommentars:
        //   (1) Ist `pre_shared_key` da? Nein -> `Ok(Vec::new())`, das
        //       heisst fuer den Aufrufer: kein PSK, voller Handshake.
        //   (2) Ist `psk_dhe_ke` (1) angeboten? Nein -> Abbruch, denn ohne
        //       diesen Modus duerfen wir keine PSK-Suite waehlen
        //       (RFC 8446 4.2.9).
        //   (3) Angebot lesen, Identitaet, Binder.
        // added 01.10.2026 22:5x: `psk` ist der **ganze** Block
        // (Typ + Laenge + Inhalt), den der Decoder bei `pre_shared_key`
        // (41) ablegt (`nachricht.rs` Zeile 1049–1073) — `Some` heisst also
        // „die Erweiterung war da". Es ist kein neues Feld noetig, und die
        // Reihenfolge braucht keine zweite Layout-Wahrheit.
        if client.psk.is_none() {
            // added 01.10.2026 22:5x: **kein Angebot**, nicht ein
            // angenommenes. GEMESSEN: mit diesem Zweig laedt
            // `alpn_client::der_browser_weg_bietet_h2_nur_wenn_er_es_
            // sprechen_kann` wieder, und `narsil-server` kann
            // Mehrverbindungen bedienen.
            return Ok(None);
        }
        // DER MODUS STEHT IM DEKODIERTEN ClientHello und nicht in den
        // Bytes: `psk_key_exchange_modes` (45) ist eine eigene
        // Erweiterung, und sie noch einmal aus den Rohbytes zu ziehen
        // waere eine zweite Layout-Wahrheit fuer dieselbe Nachricht.
        if !client.psk_modi.contains(&1) {
            return Err("psk_dhe_ke (1) wurde nicht angeboten");
        }
        // (2) Das Angebot lesen.
        let empfang = crate::ticket::psk_lese(clienthello).map_err(|_| "das Angebot ist kaputt")?;
        // (3) Die erste Identitaet suchen.
        let (identitaet, _alter) = empfang.eintraege.first().ok_or("leeres Angebot")?;
        let psk = tickets
            .psk_fuer(identitaet)
            .ok_or("unbekannte Identitaet")?;
        // (4) Den Binder pruefen.
        let transkript =
            crate::ticket::transkript_fuer_binder(clienthello, 1).map_err(|_| "Schnitt kaputt")?;
        let erwartet = crate::ticket::binder(&psk, &transkript).map_err(|_| "Binder-Ableitung")?;
        if !crate::ticket::konstant_gleich(&erwartet, &empfang.binder) {
            // added 02.10.2026, R4f. DIE ZAHLEN, OHNE DIE MAN NICHTS
            // ENTSCHEIDEN KANN. `schchnitt` ist die Laenge des
            // **gekuerzten** ClientHello (RFC 8446 4.2.11.2: ohne
            // `binders_list` und ohne deren Laengenfeld), `ch` die volle
            // Laenge. Die Differenz muss 35 sein (2 + 1 + 32) — ist sie das,
            // ist der Schnitt bewiesen und der Fehler liegt **nicht** dort.
            //
            // WARUM DAS DAUERHAFT STEHEN BLEIBT: `openssl s_client -sess_in`
            // bekommt `New` statt `Reused`, und der Server hat einen Monat
            // lang `Err(_)` verschluckt. Eine Zahl, die man beim Lesen
            // wegklicken kann, findet man beim Lesen nicht.
            eprintln!(
                "BINDER STIMMT NICHT: ch={} schnitt={} (Differenz {}) id_len={}                  binder_len={} erwartet={:02x?} bekommen={:02x?}",
                clienthello.len(),
                transkript.len(),
                clienthello.len() - transkript.len(),
                identitaet.len(),
                empfang.binder.len(),
                &erwartet[..8],
                &empfang.binder[..8]
            );
            return Err("der Binder stimmt nicht");
        }

        // added 01.10.2026 22:5x: `Some(psk)` = Angebot angenommen. Das
        // `Some` ist hier die einzige Stelle, die eine Wiederaufnahme
        // einleitet (`wieder_psk.is_some()`, Zeile 816 ff.).
        Ok(Some(psk))
    }

    /// Wie `handshake`, aber mit **Wiederaufnahme**.
    ///
    /// added 02.10.2026, R4d Stufe 2. WAS DER SERVER DAMIT MACHT, und die
    /// Reihenfolge ist durch die Norm festgelegt (RFC 8446 4.2.11):
    ///
    /// 1. `pre_shared_key` aus dem ClientHello **lesen** (`psk_lese`).
    /// 2. Fuer die angebotenen Identitaeten den PSK im Speicher suchen.
    /// 3. Den **Binder pruefen**: den erwarteten rechnen und mit dem
    ///    empfangenen in konstanter Zeit vergleichen. Ohne diesen Schritt
    ///    waere jede Wiederaufnahme mit einem beliebigen PSK moeglich —
    ///    es waere dann keine Wiederaufnahme, sondern eine Behauptung.
    /// 4. `selected_identity` = 0 im ServerHello (4.2.11.3).
    /// 5. Den Schluesselplan **mit** dem PSK bauen (7.1) — sonst waeren
    ///    die Schluessel die aus einem vollstaendigen Handshake, und der
    ///    erste entschluesselte Record schlaege fehl.
    ///
    /// WAS NOCH FEHLT, hier offen benannt (Stand 02.10.2026, 21:4x):
    /// `NewSessionTicket` (4.6.1) wird **noch nicht** geschickt. Ohne
    /// Ticket gibt es nichts, was ein Client beim naechsten Mal anbieten
    /// koennte — die Wiederaufnahme ist damit einseitig. Der
    /// Rundlaufbeweis (Handshake 1 → Ticket → Handshake 2) steht
    /// deshalb auch noch aus, und im Bericht ist das als offen vermerkt.
    pub fn handshake_mit_tickets(
        &self,
        leitung: &mut dyn LeseSchreiben,
        tickets: &mut crate::ticket::Tickets,
        ticket_neu: bool,
    ) -> Result<Ausgehandelt, ServerFehler> {
        // --- 1) ClientHello ---------------------------------------------
        let mut kette = LeseKette::neu(leitung);
        let (typ, ch) = kette.naechste_nachricht()?;
        if typ != typen::CLIENT_HELLO {
            return Err(ServerFehler::KeinClientHello { war: typ });
        }
        let client = ClientHello::dekodiere(&ch)?;

        // --- 2) Suite und Gruppe -----------------------------------------
        //
        // DER WEG DER AUSWAHL, und er ist eine bewusste Entscheidung: in
        // der Reihenfolge, in der **wir** koennen, nicht in der, in der
        // der Client anbietet. RFC 8446 4.1.3 laesst beides zu. Fuer
        // `h2` ist AES-128-GCM ausreichend (RFC 9113 9.2 verlangt
        // keine 256-Bit-Suite), und ChaCha ist auf Servern ohne
        // AES-NI die schnellere Wahl.
        let suite = if client.cipher_suites.contains(&0x1301) {
            Suite::Aes128GcmSha256
        } else if client.cipher_suites.contains(&0x1303) {
            Suite::ChaCha20Poly1305Sha256
        } else {
            return Err(ServerFehler::KeineSuite);
        };
        // `x25519` (0x001d). `secp256r1` waere der zweite Kandidat;
        // dafuer fehlt serverseitig die Schluessel-erzeugung fuer
        // P-256 (siehe Modul-Kopf).
        if client.key_share_gruppe != crate::nachricht::GRUPPE_X25519
            || client.key_share.len() != 32
        {
            // DIE GEMELDETE GRUPPE, und sie ist nicht immer
            // `key_share_gruppe`: `ClientHello::dekodiere` uebernimmt
            // **nur** x25519 und laesst `key_share_gruppe` bei 0 stehen,
            // wenn der Client eine andere Gruppe schickt (nachricht.rs,
            // der Zweig `ext::KEY_SHARE`). Mit `0` in der Fehlermeldung
            // kaeme niemand auf die Idee, dass der Client z. B.
            // secp256r1 angeboten hat — also wird dann die erste Gruppe
            // aus `supported_groups` gemeldet, denn die sagt, was der
            // Client tatsaechlich kann.
            let gruppe = if client.key_share_gruppe == 0 {
                client.gruppen.first().copied().unwrap_or(0)
            } else {
                client.key_share_gruppe
            };
            return Err(ServerFehler::KeinPassenderSchluesselteil { gruppe });
        }

        // --- 3) Das gemeinsame Geheimnis ---------------------------------
        let mut geheim = [0u8; 32];
        geheim.copy_from_slice(&zufall::bytes(32).map_err(|_| ServerFehler::Zufall)?);
        let eigener_public = x25519::oeffentlich(&geheim);
        let mut gegen_public = [0u8; 32];
        gegen_public.copy_from_slice(&client.key_share);
        // `x25519::gemeinsam` lehnt das Ergebnis ab, wenn es **alle
        // Nullen** sind (kleine Ordnung, RFC 8446 4.6.1 / die
        // „low order point"-Angriffe). Die Laenge hat der Client
        // oben schon geprueft, aber M4 verlangt die Nachpruefung, damit
        // eine Aenderung an `ClientHello` nicht unmerklich einen
        // Puffer schlichten kann.
        let shared = x25519::gemeinsam(&geheim, &gegen_public)
            .map_err(|_| ServerFehler::Schluesseltausch)?;

        // --- 3b) Die Wiederaufnahme annehmen, oder nicht ------------------
        //
        // added 02.10.2026, R4d Stufe 2. DER GANZE ABSCHNITT IST EINE
        // ENTSCHEIDUNG, und die Reihenfolge ist die der Norm (RFC 8446
        // 4.2.11): erst **was** angeboten wurde, dann ob wir es kennen,
        // dann ob der Binder stimmt, dann erst `selected_identity`.
        //
        // DER WEG ZUR ENTSCHEIDUNG, und er ist absichtlich conservativ:
        // `Some(psk)` heisst **bekannt und geprueft**. Jeder andere Fall
        // (kein Angebot, unbekannte Identitaet, falscher Binder, falscher
        // Modus, falscher Laengenvergleich) heisst `None`, und `None`
        // heisst vollstaendiger Handshake. Das ist nicht die Regel
        // "Bei Fehler abweisen": ein Client, der ein Ticket schickt, das
        // wir nicht kennen, soll trotzdem eine Seite bekommen — er hat
        // uns nur nichts angeboten, das wir annehmen koennen. Das
        // Abweisen waere eine DoS-Moeglichkeit ueber den eigenen
        // Sitzungsspeicher.
        let angenommen = self.angebot_annehmen(&client, &ch, tickets);
        let (selected_identity, wieder_psk) = match angenommen {
            // added 01.10.2026 22:5x: `Ok(None)` = kein Angebot. Ohne
            // diesen Arm wuerde ein leerer Binder als Wiederaufnahme
            // durchgehen (siehe die Beschreibung von `angebot_annehmen`).
            Ok(None) => (None, None),
            Ok(Some(paar)) => (Some(0u16), Some(paar)),
            // added 02.10.2026, R4f. DIE URSACHE WIRD NICHT MEHR VERWORFEN.
            // `Err(_) => (None, None)` hat den Grund fuer einen Monat
            // unsichtbar gemacht: `openssl s_client -sess_in` bekam `New`
            // statt `Reused`, und der Server sagte nichts. Es gibt keinen
            // Alert dafuer (RFC 8446 4.1.2: der Server *darf* ein Angebot
            // ablehnen, ohne es zu begruenden — aber "darf" heisst nicht
            // "muss", und ein Betreiber, der seine eigenen Tickets nicht
            // wiederbekommt, braucht die Zeile).
            //
            // WARUM `eprintln!` UND NICHT EIN FEHLERTYP: ein Ablehnungsgrund
            // ist kein Protokollfehler. Der Handshake laeuft weiter — ohne
            // Wiederaufnahme —, und der Client sieht davon nichts. Es gibt
            // also keinen Fehler, den man zurueckgeben koennte, nur eine
            // Beobachtung.
            //
            // DIE URSACHE STEHT IN EINEM STATISCHEN STRING, weil
            // `angebot_annehmen` sie schon als `&'static str` zurueckgibt
            // und sie an der Fundstelle stehen soll — eine Aufzaehlung an
            // einer zweiten Stelle waere eine zweite Wahrheit ueber dieselbe
            // Liste.
            Err(grund) => {
                // DIE ZAHL DER IDENTITAETEN kommt aus `psk_lese` und nicht aus
                // `ClientHello`: `ClientHello` fuehrt kein Feld fuer
                // `pre_shared_key`, weil das Angebot im **Rohblock** steht
                // (RFC 8446 4.2.11) und der Dekodierer es dort laesst. Fuer
                // die Diagnose wird es hier also einmal **zusaetzlich**
                // gelesen — auf einem Fehlerpfad, dessen Kosten nichts
                // ausmachen.
                let angebot = crate::ticket::psk_lese(&ch)
                    .map(|e| e.eintraege.len())
                    .unwrap_or(0);
                eprintln!(
                    "ANGEBOT ABGELEHNT: {grund} \
                     (Identitaeten: {angebot}, psk_modi: {:?})",
                    client.psk_modi
                );
                (None, None)
            }
        };
        //
        // DER MODUS, und er wird **nicht** verschwiegen: RFC 8446 4.2.9
        // verlangt `psk_dhe_ke` (1) fuer eine Wiederaufnahme mit
        // Schluesselaustausch. Ein Client, der nur `psk_ke` (0) anbietet,
        // bekommt von einem Server, der `psk_dhe_ke` verlangt, keinen
        // vollen Handshake mit vorbereitetem Schluessel — und genau das
        // wollen wir auch nicht, weil wir `psk_ke` nicht implementieren.
        // DER TEST DAFUER STEHT IN `tests/psk_rundlauf.rs`: ein Client ohne
        // `psk_key_exchange_modes` bekommt keine Wiederaufnahme.
        //
        // --- 4) ServerHello ----------------------------------------------
        let mut server_random = [0u8; 32];
        server_random.copy_from_slice(&zufall::bytes(32).map_err(|_| ServerFehler::Zufall)?);
        let sh = baue_serverhello(
            &client.session_id,
            suite,
            &server_random,
            crate::nachricht::GRUPPE_X25519,
            &eigener_public,
            selected_identity,
        )?;
        // Der Schluesselplan braucht ClientHello..ServerHello.
        //
        // DER PLAN MIT PSK, und das ist der ganze Unterschied: ohne
        // `mit_psk` waeren die Schluessel die eines vollstaendigen
        // Handshakes, und der erste entschluesselte Record des Clients
        // (sein `finished`) schlaege fehl — mit einer Meldung, die nach
        // Verschluesselung klingt und in Wahrheit ein falscher Early
        // Secret ist.
        let mut plan = match &wieder_psk {
            None => Schluesselplan::neu(shared),
            Some(psk) => Schluesselplan::mit_psk(shared, psk.clone()).map_err(|e| {
                ServerFehler::AngebotUnbrauchbar(match e {
                    crate::record::PlanFehler::PskLeer => "PSK der Laenge 0",
                    crate::record::PlanFehler::PskZuLang { .. } => "PSK laenger als ein Ticket",
                })
            })?,
        };
        plan.aufnehmen(&ch);
        plan.aufnehmen(&sh);
        plan.handshakephase()
            .map_err(ServerFehler::Schluesselplan)?;
        // DER EINZIGE KLARTEXT-RECORD. Alles danach ist
        // `application_data` von aussen (RFC 8446 5.1).
        kette.schreiben_record(&record::baue_record(record::typen::HANDSHAKE, &sh)?)?;

        // --- 5) ALPN -----------------------------------------------------
        // RFC 7301 3.2: der Server waehlt. Leeres Angebot des Clients
        // heisst „kein ALPN", und dann wird auch keines geschickt.
        let alpn = if client.alpn.is_empty() {
            None
        } else {
            match self
                .angebot
                .alpn
                .iter()
                .find(|angebot| client.alpn.contains(angebot))
            {
                Some(gewaehlt) => Some(gewaehlt.clone()),
                // TODO(Alert): `no_application_protocol` (120) senden.
                None => return Err(ServerFehler::KeinAlpnPasst),
            }
        };
        let ee = baue_encrypted_extensions(alpn.as_deref())?;

        // --- 6) Certificate ----------------------------------------------
        //
        // added 01.10.2026, R4q. DER KURZE HANDSHAKE, und das ist die
        // Aenderung, ohne die eine Wiederaufnahme **nichts** spart.
        // RFC 8446 Abschnitt 4.2.11: bei einer Wiederaufnahme schickt der
        // Server `EncryptedExtensions` und `Finished` — **kein**
        // `Certificate` und **kein** `CertificateVerify`. GEMESSEN vorher
        // (R4p, Serverzaehler): unser Server schickte **beides**, 774 Byte
        // gegen 778 Byte — die Wiederaufnahme schickte sogar mehr.
        //
        // `wieder_psk.is_some()` ist die **einzige** Bedingung, und sie ist
        // richtig: `wieder_psk` ist nur dann `Some`, wenn
        // `angebot_annehmen` das Angebot **angenommen** hat (die
        // Identitaet stand in unserem Speicher und der Binder stimmte). In
        // jedem anderen Fall ist es `None` und der Handshake bleibt
        // vollstaendig.
        //
        // WARUM DAS KEIN VERTRAUENSVERLUST IST: der Server hat den
        // Sitzungsschluessel aus einem Ticket, das **er selbst** frueher
        // ausgestellt hat. Die Kette zu pruefen, obwohl der Client sie
        // schon bei der ersten Sitzung geprueft hat, waere Arbeit ohne
        // informationellen Gewinn.
        let wiederaufgenommen = wieder_psk.is_some();
        let cert = if wiederaufgenommen {
            Vec::new()
        } else {
            // added 03.10.2026, 02:2x, R48: **DIE KETTE MITSCHICKEN.** Der Blatt
            // kommt zuerst, danach die Aussteller aus `self.kette` — genau die
            // Reihenfolge, die `strom.rs` erwartet (`ganze_kette[0]` ist das
            // Blatt, `[1]` der Aussteller fuer CertID und OCSP).
            let mut liste = Vec::with_capacity(1 + self.kette.len());
            liste.push(self.angebot.zertifikat.clone());
            for z in &self.kette {
                liste.push(z.clone());
            }
            baue_certificate_liste_mit_ocsp(&liste, self.ocsp.as_deref())?
        };

        // --- 7) CertificateVerify ----------------------------------------
        // DER HASH-PUNKT, und er ist die Reihenfolge:
        // `EncryptedExtensions` und `Certificate` muessen **vor** dem
        // Signieren in den Transkript, denn die Signatur steht ueber
        // ClientHello..Certificate (RFC 8446 4.4.3: "the transcript
        // hash ... up to and including the Certificate message").
        //
        // DER FEHLER, DEN DIESER BLOCK ZUERST HATTE: EE und Certificate
        // wurden erst **nach** dem Signieren aufgenommen. Der Server
        // signierte also ueber ClientHello..ServerHello, und die
        // `CertificateVerify`-Signatur war falsch — was der eigene
        // Client als "handshake failed" meldet und niemand zuordnen
        // kann. Gefunden hat es der Test
        // `der_server_handshake_stimmt_mit_einem_unabhaengigen_client`,
        // der den Transkript **selbst** baut; der Test im selben Modul
        // haette es nicht gefunden, weil beide denselben Fehler gemacht
        // haetten.
        plan.aufnehmen(&ee);
        // DER TRANSKRIPT OHNE KETTE, und das ist der Punkt, an dem ein
        // Fehler **nicht** sichtbar waere: nimmt man die Kette nicht in den
        // Transkript auf, schlaegt das `finished` des Servers fehl, und die
        // Meldung lautet `FinishedFalsch` — ohne Hinweis auf die Ursache.
        // Beide Seiten muessen **dieselbe** Nachrichtenfolge in den
        // Transkript legen; das ist der ganze Inhalt dieses Blocks.
        if !wiederaufgenommen {
            plan.aufnehmen(&cert);
        }
        let th_bis_zertifikat = plan.transkript_hash();
        // DIE SIGNATUR, und bei einer Wiederaufnahme **keine**.
        //
        // added 01.10.2026, R4q: RFC 8446 Abschnitt 4.2.11 nennt fuer den
        // wiederaufgenommenen Handshake nur `EncryptedExtensions` und
        // `Finished`. Wer die Signatur trotzdem schickt, sendet 80 Byte, die
        // niemand prueft, und der Client wuerde sie pruefen und die Kette
        // verlangen, die es nicht gibt.
        let cv = if wiederaufgenommen {
            Vec::new()
        } else {
            self.certificate_verify(&th_bis_zertifikat)?
        };
        // DER TRANSKRIPT, und er ist die stille Stelle: nimmt man die
        // `CertificateVerify` **nicht** auf, schlaegt das `finished` des
        // Servers fehl und die Meldet lautet `FinishedFalsch` — ohne jeden
        // Hinweis auf die Ursache. Beide Seiten muessen **dieselbe**
        // Nachrichtenfolge in den Transkript legen.
        if !wiederaufgenommen {
            plan.aufnehmen(&cv);
        }

        // --- 8) Finished des Servers -------------------------------------
        // `CertificateVerify` kommt als **letzte** in den Transkript (das
        // ist der Block oben); danach beginnt die `finished`-Rechnung.
        let th_bis_cv = plan.transkript_hash();
        let finished_server =
            crate::hmac::HmacSha256::berechne(&plan.finished_key_server, &th_bis_cv);
        let fin = record::baue_nachricht(typen::FINISHED, &finished_server)?;

        // --- 9) Der Flug, verschluesselt ---------------------------------
        // EIN Record fuer alle vier Nachrichten (RFC 8446 5.1 erlaubt
        // das; die meisten Server tun es). M4: die Laenge ist durch
        // `schluessel_protect` (`MAX_INNEN`) begrenzt, also hier keine
        // eigene.
        //
        // added 01.10.2026, R4q: `cert` und `cv` sind bei einer
        // Wiederaufnahme **leer**, und `extend_from_slice` eines leeren
        // Slice aendert nichts — es sind also keine `if`s noetig, und der
        // Handshake liest sich unveraendert. Die Alternative (`if
        // wiederaufgenommen { … }`) waere eine zweite Liste, die man
        // pflegen muss, und an der die Reihenfolge der Nachrichten
        // auseinanderlaufen koennte.
        let mut flug = Vec::with_capacity(ee.len() + cert.len() + cv.len() + fin.len());
        flug.extend_from_slice(&ee);
        flug.extend_from_slice(&cert);
        flug.extend_from_slice(&cv);
        flug.extend_from_slice(&fin);
        let verborgen = record::schluessel_protect(
            suite,
            &plan.server_hs_traffic,
            0,
            record::typen::HANDSHAKE,
            &flug,
        )?;
        kette.schreiben_record(&record::baue_record(
            record::typen::AEAD_CHIFFERTEXT,
            &verborgen,
        )?)?;

        // --- 10) Das finished des Clients --------------------------------
        // DER TRANSKRIPT FUER DIE PRUEFUNG: er reicht ueber das
        // `finished` des Servers, also ueber `ClientHello..finished
        // (Server)`. Genau das ist der Unterschied zum Schritt 8, und
        // genau dieser Unterschied hat in `strom.rs` einmal den
        // Live-Handshake scheitern lassen (dort steht er auch).
        // UND JETZT das `finished` des Servers in den Transkript. Ohne
        // das prueft der Server das `finished` des Clients ueber
        // ClientHello..CertificateVerify — und der Client hat es ueber
        // ClientHello..finished (Server) gebaut. Die beiden unterscheiden
        // sich um **ein** 4-Byte-Kopf plus 32 Byte, also sieht der
        // Fehler wie ein Schluesselproblem aus und ist keins.
        //
        // DER FEHLER, DEN DIESER BLOCK ZUERST HATTE: `plan.aufnehmen(&fin)`
        // fehlte. Derselbe Fehler stand in `strom.rs` und hat dort den
        // Live-Handshake scheitern lassen; die Begruendung steht dort.
        plan.aufnehmen(&fin);
        let th_bis_server_fin = plan.transkript_hash();

        // --- 11) Die Anwendungsgeheimnisse -------------------------------
        //
        // DIE STELLE, und sie ist die ganze Fehlerklasse: RFC 8446
        // Abschnitt 7.1 leitet **beide** Anwendungsgeheimnisse aus
        // `ClientHello...server Finished` ab. Das `finished` des
        // **Clients** steht dort nicht. Beleg aus dem Text
        // (`/home/jaff/mutation_md/tls/rfc/rfc8446.txt`, Zeile
        // 5191-5197):
        //
        //   Derive-Secret(., "c ap traffic", ClientHello...server Finished)
        //   Derive-Secret(., "s ap traffic", ClientHello...server Finished)
        //
        // DER FEHLER, DEN DIESER BLOCK HATTE (30.09.2026, ~00:05, gefunden
        // am echten Client **und** bei `openssl s_client`, also nicht an
        // der Gegenseite): der Aufruf stand **nach**
        // `plan.aufnehmen(&client_finished_nachricht)`. Damit hatten
        // beide Seiten verschiedene Anwendungsgeheimnisse, und der
        // erste Anwendungsrecord liess sich mit "die Authentifizierung
        // schlug fehl" nicht oeffnen.
        //
        // (`anwendungsphase_client` in `strom.rs` macht es richtig: der
        // Client nimmt ClientHello..server Finished, und er spricht mit
        // echten Servern — example.com und badssl.com, Live-Tests.)
        //
        plan.anwendungsphase_server()
            .map_err(ServerFehler::Schluesselplan)?;
        let (typ, client_finished_nachricht) =
            kette.naechste_verschluesselte(suite, &plan.client_hs_traffic, 0)?;
        if typ != typen::FINISHED {
            return Err(ServerFehler::Unerwartet { war: typ });
        }
        let client_finished = crate::nachricht::Finished::dekodiere(&client_finished_nachricht)
            .map_err(|e| ServerFehler::Nachricht(e.to_string()))?;
        let erwartet =
            crate::hmac::HmacSha256::berechne(&plan.finished_key_client, &th_bis_server_fin);
        if !crate::hmac::vergleich_gleich(&erwartet, &client_finished.verify_data) {
            return Err(ServerFehler::FinishedFalsch);
        }
        //
        // added 01.10.2026 12:5x, R4z2 Teil C. DIE CLIENT-`finished`
        // GEHOERT INS TRANSKRIPT — und deshalb steht `aufnehmen` hier und
        // nicht weiter oben, wo es einmal stand (der Kommentar in
        // Zeile 868 erzaehlt, warum es dort falsch war).
        //
        // GEMESSEN: der Server nahm sie **nicht** auf, der Client jetzt
        // doch. Damit stimmten die beiden Transkripte nicht mehr
        // ueberein und der Binder blieb falsch.
        //
        // DER ORT, und er ist sicher: die Anwendungsgeheimnisse sind
        // `plan.server_ap_traffic` / `plan.client_ap_traffic`, und die
        // werden in Zeile 1071 und 1134/1135 **entnommen**. Nimmt man
        // die Nachricht vorher auf, aendert sich ihr Wert, und der erste
        // Anwendungsrecord schlaegt fehl — genau der Fehler aus
        // `strom.rs`, wo er die Anwendungsphase zerstoerte.
        //
        // M3: `client_finished_nachricht` ist eine **eingehende** Nachricht,
        // und die unterliegt `MAX_HANDSHAKE_NACHRICHTEN`. Es wird hier
        // genau **eine** aufgenommen — die Grenze ist also nicht
        // verletzt; `aufnehmen` prueft sie selbst.
        plan.aufnehmen(&client_finished_nachricht);
        let th_bis_client_finished = plan.transkript_hash();

        // --- 13) NewSessionTicket ------------------------------------------
        //
        // added 02.10.2026, R4d Stufe 2. **DIES** ist der Schritt, ohne den
        // eine Wiederaufnahme einseitig waere: der Client bekommt hier das
        // Ticket, das er beim naechsten Mal anbieten kann.
        //
        // DER ORT IST FESTGELEGT, und drei Dinge stehen dagegen:
        //  * **Nach** dem Server-`finished`: RFC 8446 5.1 — alles nach dem
        //    Server-`finished` laeuft unter den Anwendungs-Schluesseln.
        //  * **Nach** dem Client-`finished`: der Server ist erst mit dem
        //    Client-`finished` sicher, dass der Client die
        //    Anwendungsgeheimnisse kennt (sie haengen am Transkript bis
        //    Server-`finished`, nicht bis Client-`finished` — aber der
        //    Server schickt erst danach etwas, weil der Client sonst
        //    noch mit dem vollstaendigen Handshake beschaeftigt ist).
        //  * **Als `handshake`-Inhalt im `application_data`-Record**: der
        //    Inhaltstyp ist `handshake` (22), der Aeussere `23`. Wer das
        //    umdreht, schickt das Ticket im Klartext.
        //
        // DER TICKETINHALT, und das ist die in `PskAngebot` benannte
        // Vereinfachung: das Ticket **ist** sein eigener Inhalt, und der
        // PSK daraus ist ebenfalls der Ticketinhalt. Ein echter Server
        // wuerde hier den `resumption_master_secret` ableiten
        // (`Derive-Secret(master, "resumption", Hash(nonce))`, RFC 8446
        // 7.2) und ein Ticket schicken, aus dem der PSK **nicht**
        // folgt — das ist der Grund, warum ein fremder Client unser Ticket
        // nicht benutzen kann und umgekehrt.
        //
        // M3: `MAX_TICKET_BYTES` (65535) ist die Grenze, und sie steht
        // schon in `baue_new_session_ticket`. Der Zufallswert fuer
        // `ticket_age_add` kommt aus demselben Zufallsgenerator wie das
        // ServerRandom — derselbe, aus dem schon das ECDHE-Geheimnis
        // stammt, also kein zweiter Weg zum Zufall.
        // DIE ZEIT, einmal geholt: `empfangen` und der Zeitstempel im
        // Ticketinhalt muessen **dieselbe** Sekunde sein, sonst denkt der
        // Server, das Ticket waere aelter als es ist. Zwei Aufrufen der
        // Uhr an zwei Stellen koennten die Sekunde dazwischen
        // ueberschreiten — an der Jahresgrenze sichtbar, sonst nicht.
        //
        // added 02.10.2026, R4d Stufe 2. GEMESSEN: es stand hier erst
        // `jetzt_serverseitig()` an zwei Stellen und der Compiler
        // meldete `cannot find function` — die Hilfsfunktion gab es noch
        // nicht. Jetzt gibt es sie, und sie ist eine.
        let jetzt_serverseitig = || -> u64 {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        };
        let alter_add = {
            let mut v = [0u8; 4];
            v.copy_from_slice(&zufall::bytes(4).map_err(|_| ServerFehler::Zufall)?);
            u32::from_be_bytes(v)
        };
        // DER INHALT DES TICKETS, und er ist ein echter Block:
        // `u64 Zeitstempel || PSK` (RFC 8446 4.6.1). `MAX_TICKET_BYTES`
        // ist die Grenze fuer das fertige Ticket.
        //
        // added 02.10.2026, R4i. **WAS HIER VORHER STAND:** der
        // `master_secret` der Sitzung, und die PSK war der **gesamte
        // Ticketinhalt**. Beides ist ersetzt, und an **beiden** Stellen
        // aus einem Grund:
        //
        // 1. Das `master_secret` gehoert **nicht** in ein Ticket. Es ist
        //    der Schluessel der laufenden Sitzung; wer ihn hat, kann sie
        //    entschluesseln. RFC 8446 4.6.1 nennt dafuer ein eigenes
        //    `resumption_master_secret`. Ein Ticket, das den Sitzungs-
        //    schluessel mit herausgaibt, ist ein Ticket, das man nicht
        //    weiterreichen darf.
        // 2. Die PSK war der Ticketinhalt — und das ist die
        //    **Entwurfsentscheidung**, an der `openssl` scheitert
        //    (Bericht R4h, `openssl sess_id -text`: 32-Byte-PSK gegen
        //    40-Byte-Ticket, und `SHA-256(Ticket)` ist die Session-ID,
        //    nicht die PSK). Unsere Runde lief, weil Client und Server
        //    dasselbe falsch machten.
        //
        // ZURUECKGENOMMEN (01.10.2026 12:5x, R4z2 Teil C). Hier stand:
        //
        //     DER WEG: ein **eigener Zufallswert** an fester Stelle
        //     (`PSK_VERSATZ` = 8, PSK_LAENGE = 32, also 40 Byte Ticket),
        //     und der Client holt ihn mit `ticket::psk_aus_ticket`. Dass
        //     die PSK im Ticket **steht**, ist genau das, was die Norm
        //     verlangt.
        //
        // DARAUS WAR ZWEI FALSCHES: erstens stimmt der Satz nicht — die
        // Norm verlangt das **Gegenteil** (RFC 8446 4.6.1, Zeile 4176:
        // die PSK wird aus `resumption_master_secret` und `ticket_nonce`
        // abgeleitet, sie steht in keinem Ticket). Zweitens ergab die
        // Zeile darunter eine Kapazitaet von 40 Byte fuer einen Inhalt,
        // der jetzt **16** Byte hat. Beides steht hier noch, weil ein
        // Kommentar, der die falsche Behauptung traegt, schlimmer ist
        // als gar keiner: wer ihn liest, baut die falsche Sache nach.
        //
        // DER INHALT, und er sind genau zwei Felder:
        //     u64 Zeitstempel (fuer unser eigenes Ticket-Alter)
        //   || ticket_nonce     (8 Byte, fuer die PSK-Ableitung)
        // Der Zeitstempel ist **unsere** Buchhaltung, kein Normfeld; die
        // Norm kennt ihn nicht. Er steht deshalb zuerst und wird
        // getrennt begruendet.
        const INHALT_FEST: usize = 8; // u64 Zeitstempel
        let mut inhalt = Vec::with_capacity(INHALT_FEST + TICKET_NONCE_BYTES);
        inhalt.extend_from_slice(&jetzt_serverseitig().to_be_bytes());
        //
        //
        // added 01.10.2026 12:5x, R4z2 Teil C. **DIE PSK WIRD ABGELEITET,
        // SIE STEHT NICHT IM TICKET.** Vorher stand hier
        // `zufall::bytes(PSK_LAENGE)` und die Zufalls-PSK ging in den
        // Ticketinhalt. GEMESSEN falsch: gegen dasselbe Ticket meldete
        // `openssl` eine **andere** PSK (`a332f48f…` bei uns,
        // `7A04648D…` bei openssl), und deshalb stimmte der Binder nicht —
        // `openssl s_client -sess_in` sagte `New` statt `Reused`.
        // Belege: `PROBLEME.txt` Nr. 10, `BERICHT_TLS_R4z2.txt` Teil C.
        //
        // DIE KETTE, woertlich RFC 8446 Abschnitt 4.6.1:
        //     resumption_master_secret = Derive-Secret(master_secret,
        //                     "resumption", Hash(""))
        //     PSK = HKDF-Expand-Label(resumption_master_secret,
        //                     "resumption", ticket_nonce, Hash.length)
        //
        // DER NONCE ist der **leere**. RFC 8446 4.6.1 erlaubt
        // `ticket_nonce<0..255>`, und ein leerer Nonce ist der Normalfall.
        // Er steht trotzdem als benannte Konstante (M1): waere er ein
        // Literal an zwei Stellen und waere eine Stelle spaeter ein
        // Zufallswert, gaebe es zwei Tickets, die sich nicht entschlüsseln
        // lassen — ohne dass man es beim Lesen sieht.
        //
        // DER NONCE, und er ist **nicht leer** — das war mein zweiter
        // Fehler in derselben Runde. RFC 8446 4.6.1, woertlich
        // (`rfc8446.txt` Zeile 4179-4180):
        //   "Because the ticket_nonce value is distinct for each
        //    NewSessionTicket message, a different PSK will be derived
        //    for each ticket."
        // Ein leerer Nonce hiesse: **alle** Tickets einer Sitzung haetten
        // dieselbe PSK. Das faellt nicht auf, solange ein Ticket
        // existiert, und wird sichtbar, sobald der Server ein zweites
        // ausgibt.
        //
        // EIGENE ZIEHUNG, und nicht `alter_add`: `alter_add` ist bereits
        // vier Zufallsbyte (Zeile 947-951). Teilt es die Ziehung, waeren
        // Nonce und `alter_add` gleich, und dann waere der Nonce nicht
        // distinkt. Zwei Ziehungen sind hier billiger als die Erklaerung,
        // warum dieselbe Zufallszahl zweimal etwas anderes bedeutet.
        //
        // added 01.10.2026 13:8x, R4z5: DIE LAENGE IST **EINE**, und sie
        // stand hier als `const TICKET_NONCE_BYTES: usize = 8;` **und**
        // in `ticket.rs` als `TICKET_NONCE_BYTES` — zwei Namen fuer
        // dieselbe Zahl an zwei Stellen, in zwei Dateien. Genau das ist
        // die Fehlerform, die diese Runde abstellen wollte: der Server
        // zieht 8 Byte, schreibt aber 4 ins Ticket, und der Client
        // rechnet mit 8 weiter.
        const TICKET_NONCE_BYTES: usize = crate::ticket::TICKET_NONCE_BYTES;
        let mut nonce_feld = [0u8; TICKET_NONCE_BYTES];
        nonce_feld
            .copy_from_slice(&zufall::bytes(TICKET_NONCE_BYTES).map_err(|_| ServerFehler::Zufall)?);
        let master = plan.master_secret().map_err(ServerFehler::Schluesselplan)?;
        let resumption =
            crate::record::derive_secret(&master, b"res master", &th_bis_client_finished)?;
        let psk = crate::ticket::psk_aus_nonce(&resumption, &nonce_feld)
            .map_err(|_| ServerFehler::AngebotUnbrauchbar("die PSK liess sich nicht ableiten"))?;
        // DIE KONTROLLE, und sie ist billig: die Laenge muss stimmen, sonst
        // waere der Binder aus einer falschen Laenge gebaut und der Fehler
        // faellt erst beim Wiederaufnehmen auf.
        if psk.len() != crate::ticket::PSK_LAENGE {
            return Err(ServerFehler::AngebotUnbrauchbar(
                "die abgeleitete PSK hat nicht die erwartete Laenge",
            ));
        }
        //
        // added 01.10.2026 12:5x, R4z2 Teil C. DER NONCE GEHOERT INS
        // TICKET, und das ist nicht Kosmetik: `Tickets::psk_fuer`
        // nachschlaegt nach der **Ticketkennung**, nicht nach dem Nonce.
        // Ohne den Nonce im Inhalt waere der Inhalt fuer **alle** Tickets
        // gleich (8 Byte Zeitstempel und sonst nichts), und der Server
        // koennte sein eigenes zweites Ticket nicht unterscheiden.
        inhalt.extend_from_slice(&nonce_feld);
        //
        // added 01.10.2026 13:8x, R4z5: DIE LAENGENPRUEFUNG, und sie ist
        // der Grund, warum `ticket::MIN_TICKET` ueberhaupt existiert.
        // Ohne sie waere die Konstante eine Zahl **im Kommentar** — und
        // eine Zahl im Kommentar prueft nichts.
        //
        // WAS SIE FAENGT: eine Aenderung an `INHALT_FEST` oder
        // `TICKET_NONCE_BYTES`, die den Inhalt kuerzer machen wuerde,
        // faellt hier und **nicht** erst beim Wiederaufnehmen. Genau
        // dieser Fehler ist in R4z passiert: das Ticket war 40 Byte
        // lang und enthielt eine PSK, die der Client nicht bilden
        // konnte — sichtbar wurde es erst an einem ganz anderen Ort.
        //
        // M3: `inhalt` ist aus dem **Code** aufgebaut, nicht aus dem
        // Netz. Eine obere Grenze waere hier Theater.
        if inhalt.len() != crate::ticket::MIN_TICKET {
            return Err(ServerFehler::AngebotUnbrauchbar(
                "der Ticketinhalt hat nicht die erwartete Laenge",
            ));
        }
        let nachricht = baue_new_session_ticket(
            crate::ticket::Ticket::VORGABE_LEBENSDAUER,
            alter_add,
            &nonce_feld,
            &inhalt,
            &[],
        )?;
        //
        // added 01.10.2026 13:9x, R4z6: DIE SEQUENZZAHL DES TICKETS.
        // Sie steht jetzt in `gesendete_records` weiter unten, damit
        // `AnwendungsStrom` nicht bei null wieder anfaengt. Vorher stand
        // hier nur die `0` im Aufruf, und der Verbraucher wusste nichts
        // davon — siehe das Feld.
        const TICKET_SEQ: u64 = 0;
        //
        // DER VERSAND, und die Sequenzzahl ist **0**: die
        // Anwendungs-Sequenzzahl des Servers beginnt hier, und ein Ticket
        // ist das erste, was er unter den Anwendungs-Schluesseln schickt.
        let chiffrat = record::schluessel_protect(
            suite,
            &plan.server_ap_traffic,
            TICKET_SEQ,
            // DER INNERE TYP, und es ist `record::typen::HANDSHAKE` (20):
            // der **Inhalt** ist eine `handshake`-Nachricht, der
            // **Aeussere** des Records wird von `schluessel_protect`
            // selbst auf `application_data` (23) gesetzt. DER
            // DOPPELPUNKT-NAMENSFALL, und er hat mich eine Zeile gekostet:
            // `nachricht::typen` hat **kein** `HANDSHAKE` (dort stehen
            // die Nachrichten-Eigenarten 1..20, nicht der Content-Typ),
            // und `record::typen::HANDSHAKE` ist die richtige Fundstelle.
            record::typen::HANDSHAKE,
            &nachricht,
        )?;
        //
        if ticket_neu {
            // DER AEUSSERE RECORD, und der fehlte in der ersten Fassung — der
            // Client meldete `zu viel vom Server: Record zu lang`, weil er
            // die **Chiffrat-Nutzlast** als Record-Kopf gelesen hat.
            // `schluessel_protect` liefert nur den AEAD-Block
            // (`content || explicit-nonce || tag`), nicht den Record; der
            // aeussere Typ ist `AEAD_CHIFFERTEXT` (23), weil die
            // Inhaltslaenge im Klartext nicht stehen darf (RFC 8446 5.2).
            //
            // DER NAME IST IRREFUEHREND, und er hat mich hier zum Nachsehen
            // gebracht (02.10.2026, R4f): `AEAD_CHIFFERTEXT` steht im Crate
            // fuer `application_data` (23). Ein Entwurf aus der
            // Entstehungszeit hat eigene Typen (25, siehe
            // `AEAD_CHIFFERTEXT_ENTWURF`), und der Name ist geblieben.
            // Es ist derselbe Weg wie in `AnwendungsStrom::schreiben`, und es
            // muss derselbe sein — zwei Wege fuer denselben Record waeren
            // zwei Orte, an denen einer falsch sein kann.
            let chiffrat = record::baue_record(record::typen::AEAD_CHIFFERTEXT, &chiffrat)?;
            kette.schreiben_record(&chiffrat)?;
        }
        //
        // UND DAS TICKET ABLEGEN, mit dem Host des Zertifikats. Das ist die
        // eine Stelle, an der ein Ticket entsteht; `Tickets::psk_fuer`
        // findet es spaeter ueber die Identitaet.
        //
        // DER HOST, und er ist nicht der SNI: das Zertifikat traegt
        // `SAN = DNS:localhost`, und der Client, der sich mit ihm
        // verbindet, sagt `localhost`. Der SNI waere hier der richtige
        // Name, aber ein Client darf SNI weglassen — dann gaebe es keinen.
        // Deshalb: `localhost`, wie es im Zertifikat steht, und der
        // Kommentar sagt, dass das eine Vereinfachung ist.
        const HOST_IM_TICKET: &str = "localhost";
        tickets.ablegen(
            HOST_IM_TICKET,
            crate::ticket::Ticket {
                psk: psk.clone(),
                lebensdauer: crate::ticket::Ticket::VORGABE_LEBENSDAUER,
                alter_add,
                nonce: Vec::new(),
                ticket: inhalt.clone(),
                erweiterungen: Vec::new(),
                empfangen: jetzt_serverseitig(),
            },
        );
        let _ = HOST_IM_TICKET;

        // Und jetzt die Anwendungsgeheimnisse in den Ertrag geben. Sie
        // sind oben (Schritt 11) **vor** dem Client-`finished` berechnet
        // worden; hier werden nur noch die Felder gefuellt.
        let les_geheimnis = plan.client_ap_traffic;
        let schreib_geheimnis = plan.server_ap_traffic;

        Ok(Ausgehandelt {
            quic_parameter_vom_client: client.quic_transport_parameters.clone(),
            // added 01.10.2026 13:2x, R4z4: die Auskunft aus der einen
            // Stelle, an der der Server es weiss (`wieder_psk`).
            wiederaufgenommen,
            // added 01.10.2026 13:9x, R4z6: der Ticket-Record hat
            // Sequenz 0 benutzt; der naechste braucht 1. Ohne das startet
            // `AnwendungsStrom` bei 0 und wiederholt die Nonce.
            gesendete_records: if ticket_neu {
                TICKET_SEQ + 1
            } else {
                TICKET_SEQ
            },
            alpn,
            suite,
            gruppe: crate::nachricht::GRUPPE_X25519,
            lesen: les_geheimnis,
            schreiben: schreib_geheimnis,
            // Und der Rest, der nach dem `finished` im Puffer lag.
            // `drain` leert die Quelle, damit die Bytes nicht doppelt
            // im `Ausgehandelt` und im `LeseKette` stehen.
            rest_bytes: kette.puffer.drain(..).collect(),
        })
    }
}

// ---------------------------------------------------------------------------
// Die Nachrichten des Serverflugs
// ---------------------------------------------------------------------------

/// Baut das `ServerHello` (RFC 8446 4.1.3).
///
/// added 30.09.2026, Runde 2. `session_id` wird **echoed**, und das ist
/// nicht Kosmetik: RFC 8446 4.1.2 sagt, der Server wiederholt die
/// `legacy_session_id` des ClientHello unveraendert. Ein Client, der
/// eine 32-Byte-ID fuer Middleboxen schickt (das ist die
/// Empfehlung der Norm), weist einen Server ab, der eine leere
/// zurueckschickt — und zwar *ohne* Fehlermeldung, mit einem
/// `illegal_parameter`.
/// added 02.10.2026 03:3x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// added 02.10.2026 03:3x: DER HELFER, und er ist an **einer** Stelle, weil
/// vier Bauteile denselben Fehler brauchen. `record::baue_nachricht` kennt
/// genau zwei Fehlerbilder: die Nachricht ist zu gross (der Fall, um den es
/// hier geht) — alles andere ist ein Programmfehler und wird **nicht**
/// verschluckt, sondern mit dem Typ durchgereicht, damit man ihn beim Testen
/// sieht. `Display` von `ServerFehler` zeigt den Typ als Zahl, das ist die
/// Form, die `typen::SERVER_HELLO` und so weiter bereits haben.
fn nachricht_zu_gross_oder_melden(typ: u8, e: crate::record::NachrichtFehler) -> ServerFehler {
    match e {
        crate::record::NachrichtFehler::LaengeZuGross { angegeben, erlaubt } => {
            ServerFehler::NachrichtZuGross {
                typ,
                war: angegeben,
                erlaubt,
            }
        }
        // added 02.10.2026 03:3x: `{:?}` und **nicht** `{}` — GEMESSEN: der
        // Uebersetzer meldete „`record::NachrichtFehler` doesn't implement
        // `std::fmt::Display`". Der Debug-Text nennt die Variante, und die
        // ist hier die Information, die man im Protokoll braucht.
        anderes => ServerFehler::Nachricht(format!("Nachricht {typ}: {anderes:?}")),
    }
}

pub fn baue_serverhello(
    session_id: &[u8],
    suite: Suite,
    random: &[u8; 32],
    gruppe: u16,
    key_share: &[u8],
    selected_identity: Option<u16>,
    // added 02.10.2026 03:3x: `Result`, weil `record::baue_nachricht` einen
    // Fehler liefern kann (16-MiB-Grenze) und ein `expect` im Serverflug ein
    // Panic waere.
) -> Result<Vec<u8>, ServerFehler> {
    let mut koerper = Schreiber::neu();
    koerper.u16(LEGACY_VERSION);
    koerper.bytes(random);
    koerper.vektor8(session_id);
    koerper.u16(suite.code());
    // `legacy_compression_method`: EIN Byte, und es muss 0 sein.
    //
    // DER FEHLER, DEN DIESER BLOCK HATTE, und er ist der dritte, den ein
    // **fremdes** Programm gefunden hat (`openssl s_client`, 30.09.2026
    // 23:5x): es stand hier `vektor8(&[0])`, also `01 00`. Das ist die
    // Form des **ClientHello** (`legacy_compression_methods<1..2^8-1>`,
    // RFC 8446 4.1.2). Der **ServerHello** hat laut RFC 8446 4.1.3
    //     uint8 legacy_compression_method = 0;
    // also genau **ein** Byte, ohne Laengenfeld.
    //
    // WARUM ES UNSERE EIGENEN TESTS NICHT GEFUNDEN HABEN, und das ist
    // der eigentliche Grund, es hier zu notieren: unser Dekodierer las
    // das Feld ebenfalls als Vektor. Bei einem echten Server steht dort
    // `00`, der Dekodierer liest daraus "Laenge 0", verbraucht aber
    // trotzdem **genau ein** Byte — dieselbe Zahl, als haette er den
    // Wert gelesen. Die Nachricht blieb also lang richtig gerutscht, und
    // die leere Liste passiert die Pruefung
    // (`!kompression.is_empty() && kompression != [0]`). Ein Fehler, der
    // sich als Kompatibilitaet tarnt.
    //
    // OpenSSL hat ihn trotzdem gesehen, weil es `legacy_compression_method`
    // als `PACKET_get_1` liest (ein Byte): unser `01` wurde als
    // Kompressionsmethode gelesen, das `00` danach als Laengenfeld der
    // Erweiterungen (0), und der Rest der Nachricht blieb uebrig ->
    // `SSL_R_BAD_LENGTH` in `tls_process_server_hello`.
    koerper.u8(0);

    let mut erweiterungen = Schreiber::neu();
    // supported_versions (43): die **gewaehlte** Version.
    let mut versions_inhalt = Schreiber::neu();
    versions_inhalt.u16(VERSION_TLS13);
    erweiterungen.u16(crate::nachricht::ext::SUPPORTED_VERSIONS);
    erweiterungen.vektor16(versions_inhalt.fertig());
    // key_share (51): Gruppe und Schluessel. In einem HRR stuende hier
    // **nur** die Gruppe; das ist der Server-Flug, also mit Schluessel.
    let mut key_inhalt = Schreiber::neu();
    key_inhalt.u16(gruppe);
    key_inhalt.vektor16(key_share);
    erweiterungen.u16(crate::nachricht::ext::KEY_SHARE);
    erweiterungen.vektor16(key_inhalt.fertig());
    // `pre_shared_key` (41) in der Antwort, RFC 8446 4.2.11.3. DER ORT
    // IST RICHTIG, und die Norm sagt es ausdruecklich: "The server MUST
    // send a pre_shared_key extension in its ServerHello" — und die
    // Erweiterung steht hier, **nicht** in den `EncryptedExtensions`.
    // Wer sie dorthin schreibt, macht aus einer Wiederaufnahme einen
    // vollstaendigen Handshake, bei dem beide Seiten den PSK benutzen und
    // sich einig sind, dass sie es nicht sind.
    //
    // added 02.10.2026, R4d Stufe 2. `None` heisst vollstaendiger
    // Handshake, und dann wird **keine** Erweiterung geschrieben — das ist
    // der Unterschied zwischen "kein PSK" und "PSK Nummer 0".
    if let Some(index) = selected_identity {
        let mut psk_inhalt = Schreiber::neu();
        psk_inhalt.u16(index);
        erweiterungen.u16(crate::nachricht::ext::PRE_SHARED_KEY);
        erweiterungen.vektor16(psk_inhalt.fertig());
    }
    koerper.vektor16(erweiterungen.fertig());

    record::baue_nachricht(typen::SERVER_HELLO, koerper.fertig())
        .map_err(|e| nachricht_zu_gross_oder_melden(typen::SERVER_HELLO, e))
}

/// Baut eine `NewSessionTicket`-Nachricht (RFC 8446 4.6.1).
///
/// added 02.10.2026, R4d Stufe 2. DIE FUENF FELDER, in der Reihenfolge
/// der Grammatik:
/// ```text
///     struct {
///         uint32 ticket_lifetime;
///         uint32 ticket_age_add;
///         opaque ticket_nonce<0..255>;
///         opaque ticket<1..2^16-1>;
///         Extension extensions<0..2^16-1>;
///     } NewSessionTicket;
/// ```
/// DER WEG ZUM ABSCHICKEN, und er ist der Punkt, der leicht falsch geht:
/// die Nachricht ist vom **Inhalt** eine `handshake`-Nachricht, sie
/// reist aber in einem `application_data`-Record mit den
/// Anwendungs-Schluesseln (RFC 8446 5.1: alles nach dem Server-`finished`
/// ist `application_data` von aussen). Wer sie als `handshake` schickt,
/// schickt sie im Klartext — und ein Client, der darauf wartet, findet
/// nichts.
///
/// DER NULLBINDERS-GEGENBEWEIS, und er ist in `tests/psk_lese.rs`
/// festgehalten: ein Client, der `NewSessionTicket` **im Klartext**
/// bekaeme, wuerde es als `Handshake`-Nachricht mitten im Flug finden und
/// entweder die Nachricht falsch lesen oder die Verbindung abbrechen.
///
/// `ticket_age_add` ist ein **Zufallswert** des Servers (RFC 8446 4.6.1:
/// "a securely-generated, random 32-bit value"), nicht eine Zeitangabe.
/// Der Client rechnet daraus sein `obfuscated_ticket_age`; der Server
/// vergleicht es spaeter, um Wiederverwendung zu erkennen.
pub fn baue_new_session_ticket(
    lebensdauer: u32,
    alter_add: u32,
    nonce: &[u8],
    ticket: &[u8],
    erweiterungen: &[u8],
) -> Result<Vec<u8>, ServerFehler> {
    // M3: `nonce` und `erweiterungen` kommen vom Aufrufer. Ihre Grenzen
    // stehen in der Grammatik (`<0..255>` bzw. `<0..2^16-1>`) und werden
    // hier geprueft — ein `u8`-Ueberlauf beim Schreiben wuerde eine
    // Nachricht mit falscher Laenge erzeugen, die kein Parser liest.
    if nonce.len() > 255 {
        return Err(ServerFehler::AngebotUnbrauchbar(
            "ticket_nonce laenger als 255 Byte",
        ));
    }
    if ticket.is_empty() {
        return Err(ServerFehler::AngebotUnbrauchbar("Ticket ohne Inhalt"));
    }
    if ticket.len() > crate::ticket::MAX_TICKET_BYTES {
        return Err(ServerFehler::AngebotUnbrauchbar(
            "Ticket laenger als 65535 Byte",
        ));
    }
    if erweiterungen.len() > crate::ticket::MAX_TICKET_BYTES {
        return Err(ServerFehler::AngebotUnbrauchbar("Erweiterungen zu lang"));
    }
    let mut koerper = Schreiber::neu();
    koerper.u32(lebensdauer);
    koerper.u32(alter_add);
    koerper.vektor8(nonce);
    koerper.vektor16(ticket);
    koerper.vektor16(erweiterungen);
    // DER FEHLER: `record::baue_nachricht` gibt `NachrichtFehler`
    // zurueck, und das hat **kein** `Display` (es ist ein reines
    // Fehler-Enum mit `Debug`). Die naechstliegende Form war `{:?}` — und
    // `Debug` in einer Fehlermeldung ist eine Behauptung, keine Meldung:
    // der Aufrufer bekommt `Vektor(ZuKurz { … })` statt „die Nachricht
    // passt nicht in einen Record". Es gibt hier nur **eine** moegliche
    // Ursache (die fertige Nachricht ist zu gross fuer einen Record, also
    // > 16 KiB), und die steht im Text.
    record::baue_nachricht(
        crate::nachricht::typen::NEW_SESSION_TICKET,
        koerper.fertig(),
    )
    .map_err(|_| {
        ServerFehler::AngebotUnbrauchbar("die NewSessionTicket passt nicht in einen Record")
    })
}

/// Baut die `EncryptedExtensions` (RFC 8446 4.3.1), mit ALPN.
///
/// added 30.09.2026, Runde 2, S7. `alpn = None` heisst: **keine**
/// ALPN-Erweiterung. Das ist ein Unterschied zu „leerer Liste" — ein
/// Client, der keine ALPN-Erweiterung findet, faellt auf HTTP/1.1
/// zurueck; eine leere Liste ist nach RFC 7301 3.2 verboten.
///
/// WAS HIER NICHT STEHT, und es ist eine Liste, weil sie sich jemand
/// fragt: `supported_groups`, `key_share` und `server_name` gehoeren
/// **nicht** in die `EncryptedExtensions`. Wer sie dort hineinschreibt,
/// schickt eine Erweiterung, die die Norm an dieser Stelle verbietet;
/// der Client darf sie ignorieren, und der Server glaubt, er habe
/// etwas ausgehandelt.
pub fn baue_encrypted_extensions(alpn: Option<&[u8]>) -> Result<Vec<u8>, ServerFehler> {
    baue_encrypted_extensions_mit(alpn, None)
}

/// Baut die `EncryptedExtensions` mit ALPN **und** QUIC-Transportparametern.
///
/// added 30.09.2026, Runde 2, TLS R2 (Wunsch DEEPSEEK Position 1). Die
/// einfache Form bleibt als Huelle, damit die drei vorhandenen Aufrufer
/// und Tests nicht brechen; wer QUIC spricht, nimmt diese.
///
/// `quic_parameter` ist der **fertige** Block aus RFC 9000 Abschnitt 18
/// (Dreiergruppen ID/Laenge/Wert). Der Server baut ihn nicht selbst: die
/// IDs gehoeren in `narsil-quic`, und hier wuerde eine zweite Stelle
/// entstehen, an der die Bedeutung der Zahlen steht.
pub fn baue_encrypted_extensions_mit(
    alpn: Option<&[u8]>,
    quic_parameter: Option<&[u8]>,
) -> Result<Vec<u8>, ServerFehler> {
    let mut koerper = Schreiber::neu();
    if let Some(parameter) = quic_parameter {
        // Ohne weiteres Laengenfeld: der Erweiterungsinhalt IST der
        // Block (siehe die Begruendung am Dekodierer im ClientHello).
        koerper.u16(crate::nachricht::ext::QUIC_TRANSPORT_PARAMETERS);
        koerper.vektor16(parameter);
    }
    if let Some(protokoll) = alpn {
        let mut liste = Schreiber::neu();
        liste.vektor8(protokoll);
        let mut inhalt = Schreiber::neu();
        inhalt.vektor16(liste.fertig());
        koerper.u16(crate::nachricht::ext::ALPN);
        koerper.vektor16(inhalt.fertig());
    }
    // Ohne ALPN ist die Erweiterungsliste **leer** — ein
    // `vektor16(&[])`, nicht etwa `vektor16` mit Nullbytes.
    record::baue_nachricht(typen::ENCRYPTED_EXTENSIONS, &{
        let mut mit_laenge = Schreiber::neu();
        mit_laenge.vektor16(koerper.fertig());
        mit_laenge.uebernehmen()
    })
    .map_err(|e| nachricht_zu_gross_oder_melden(typen::ENCRYPTED_EXTENSIONS, e))
}

/// Baut die `Certificate`-Nachricht aus **einem** Blatt (RFC 8446 4.4.2).
///
/// added 30.09.2026, Runde 2. DER AUFBAU, wortwoertlich nach der
/// Norm:
/// ```text
///   opaque certificate_request_context<0..2^8-1>;   // leer ohne Client-Zertifikat
///   CertificateEntry certificate_list<0..2^24-1>;
///   struct { opaque cert_data<1..2^24-1>;
///            Extension extensions<0..2^16-1>; }      // leer: kein OCSP-Stapling
/// ```
/// Die zweimal 3-Byte-Laenge sind der häufigste Fehler: die **innere**
/// ist die Laenge eines Zertifikats, die **aeussere** die der ganzen
/// Liste. Bei genau einem Blatt ist die aeussere `3 + 2 + laenge`.
pub fn baue_certificate(zertifikat: &[u8]) -> Result<Vec<u8>, ServerFehler> {
    // added 03.10.2026, 02:2x, R48: der Ein-Zertifikat-Weg bleibt und wird an
    // die Listenform delegiert — ein Aufrufer soll nicht sehen, dass es jetzt
    // zwei Funktionen gibt.
    let liste = vec![zertifikat.to_vec()];
    baue_certificate_liste(&liste)
}

/// added 03.10.2026, 02:2x, R48. **DIE `Certificate`-NACHMIT MIT N EINTRÄGEN.**
/// Das Format ist unveraendert (RFC 8446 4.4.2): `certificate_list` ist ein
/// 24-Bit-Laengenvektor von `CertificateEntry`, und jeder Eintrag ist
/// `cert_data` (24-Bit-Vektor) gefolgt von `extensions` (16-Bit-Vektor). Wir
/// schicken pro Zertifikat **leere** `extensions` — kein OCSP-Stapling, keine
/// SCTs; das ist die dokumentierte Grenze.
pub fn baue_certificate_liste(zertifikate: &[Vec<u8>]) -> Result<Vec<u8>, ServerFehler> {
    baue_certificate_liste_mit_ocsp(zertifikate, None)
}

/// added 03.10.2026 03:4x, R49. **DIE STAPELUNG IM BLATT**, und sie sitzt in den
/// `extensions` des **ersten** Eintrags (das Blatt), nicht in allen.
/// `CertificateStatus` laut RFC 6066: **ein** Byte Statusart (`1` = ocsp), dann
/// die Antwort als `opaque<1..2^24-1>` — drei Byte Laenge, dann die DER.
pub fn baue_certificate_liste_mit_ocsp(
    zertifikate: &[Vec<u8>],
    ocsp: Option<&[u8]>,
) -> Result<Vec<u8>, ServerFehler> {
    // added 03.10.2026 03:5x: **DIE LEERE LISTE BLEIBT EIN FEHLER** — die alte
    // Funktion hat das geprueft, und beim Umschreiben waere die Pruefung sonst
    // still verschwunden (ein `Certificate` ohne Eintraege ist laut RFC 8446
    // 4.4.2 unmoeglich).
    if zertifikate.is_empty() {
        return Err(ServerFehler::AngebotUnbrauchbar(
            "die Zertifikatsliste ist leer",
        ));
    }
    let mut eintraege = Schreiber::neu();
    for (i, der) in zertifikate.iter().enumerate() {
        eintraege.vektor24(der);
        if i == 0 {
            let mut ext = Schreiber::neu();
            if let Some(antwort) = ocsp {
                let mut koerper = vec![0x01u8];
                let mut laenge = [0u8; 3];
                laenge[0] = (antwort.len() >> 16) as u8;
                laenge[1] = (antwort.len() >> 8) as u8;
                laenge[2] = antwort.len() as u8;
                koerper.extend_from_slice(&laenge);
                koerper.extend_from_slice(antwort);
                // added 03.10.2026 04:2x, R49: **MEIN ERSTER BAU WAR FALSCH**,
                // und die Fehlerform ist die schlimmste der drei: ich habe
                // `vektor16(&typ.to_be_bytes())` geschrieben. GEMESSEN (04:1x) am
                // Hexbild des empfangenen Blocks: `00 02 | 00 05 | 00 01 10 | 01 |
                // 00 01 0c | 30 82 01 08 …` — **die `00 02` ist ein 16-Bit-
                // Laengenfeld**, das `vektor16` geschrieben hat. Eine TLS-
                // Erweiterung ist aber `extension_type` **ohne** Laengenfeld,
                // gefolgt von `extension_data<0..2^16-1>`. Die `00 02` hat den
                // Typ nach `0x0002` verschoben, und der Decoder hat daraufhin an
                // `00 05`-Stelle **gelesen**, mit Laenge 1, und die echte
                // Antwort nie gesehen.
                //
                // added 03.10.2026 04:2x: **DIE LEHRE, und sie ist allgemein.**
                // Ein falsches **Feldformat** erzeugt kein Fehlen, sondern
                // **Verschiebung**: die Nachricht bleibt lang genug, laesst sich
                // lesen und scheitert an einer **anderen** Stelle. Nur der
                // Hexdump des empfangenen Blocks hat es aufgedeckt — die
                // Fehlermeldung (``sagt 12422 Byte``) zeigt in die
                // Laengenpruefung und damit ins **Leere**, weil der Fehler
                // drei Felder weiter vorn war.
                ext.u16(STATUS_REQUEST_TYP);
                ext.vektor16(&koerper);
            }
            eintraege.vektor16(ext.fertig());
        } else {
            eintraege.vektor16(&[]);
        }
    }
    certificate_huelle(eintraege)
}

/// added 03.10.2026 03:4x, R49: **DIE `CertificateStatus`-EXTENSION**, und die
/// Nummer steht hier, weil sie sonst in drei Dateien verteilt waere: 5 ist
/// `status_request` (RFC 6066 3; RFC 8446 listet sie in beiden
/// Extension-Beschreibungen mit dem Vermerk „RFC 6066“).
const STATUS_REQUEST_TYP: u16 = 5;

fn certificate_huelle(eintraege: Schreiber) -> Result<Vec<u8>, ServerFehler> {
    let mut koerper = Schreiber::neu();
    koerper.vektor8(&[]); // certificate_request_context
    koerper.vektor24(eintraege.fertig());
    record::baue_nachricht(typen::CERTIFICATE, koerper.fertig())
        .map_err(|e| nachricht_zu_gross_oder_melden(typen::CERTIFICATE, e))
}

// // added 04.10.2026 01:3x, R67. **`baue_certificate_liste_ohne` GELOESCHT** —
// // GEMESSEN: **ein** Treffer in allen Crates, die Definition. Und sie war
// // **verhaltensgleich** mit `baue_certificate_liste`, denn das ist beides
// // `baue_certificate_liste_mit_ocsp(zertifikate, None)` (Z. 1748/1829).
// //
// // added 04.10.2026 01:3x: **DER NAMEN WAR DAS EIGENTLICHE PROBLEM.** „ohne"
// // verspricht eine Variante **mit** etwas — aber `baue_certificate_liste` ist
// // bereits die ohne-Stapling-Fassung. Wer `baue_certificate_liste_ohne` las,
// // bekam die Antwort „gegen OCSP geschuetzt, aber hier ohne" und gedanklich
// // das Gegenteil. Ein toter Doppelgaenger mit irrefuehrendem Namen ist
// // schlimmer als keiner, weil er eine **Entscheidung** vortaeuscht.

/// Baut die `CertificateVerify`-Nachricht (RFC 8446 4.4.3).
///
/// added 30.09.2026, Runde 2. `signatur` ist der ECDSA-Sig-Wert
/// `r || s`, je 32 Byte. Das Schema-Feld ist **nicht** optional und
/// nicht `None`: bei einer Wiederaufnahme schickt der Server eine
/// leere `signature`, und das ist ein eigener Pfad (A1).
pub fn baue_certificate_verify(schema: u16, signatur: &[u8]) -> Result<Vec<u8>, ServerFehler> {
    let mut koerper = Schreiber::neu();
    koerper.u16(schema);
    koerper.vektor16(signatur);
    record::baue_nachricht(typen::CERTIFICATE_VERIFY, koerper.fertig())
        .map_err(|e| nachricht_zu_gross_oder_melden(typen::CERTIFICATE_VERIFY, e))
}

// ---------------------------------------------------------------------------
// Das Lesen und Schreiben ueber die Leitung
// ---------------------------------------------------------------------------

/// Typ, Inhalt und die vollstaendige Nachricht fuer den Transkript.
///
/// added 30.09.2026. DER NAME, und clippy hat ihn verlangt: ein
/// dreifacher Tupel aus drei `Vec<u8>`-artigen Dingen ist an
/// `Some((22, inhalt, voll))` nicht mehr lesbar, und an der
/// Aufrufstelle kann ein Tippfehler nicht mehr auffallen.
type NachrichtTeil = (u8, Vec<u8>, Vec<u8>);

/// Liest Handshake-Nachrichten aus einer Leitung.
///
/// added 30.09.2026, Runde 2. DER WEG ZUM LIMIT: `MAX_GELESEN` wird
/// **gegen die tatsaechlich gelesenen Bytes** geprueft, nicht gegen die
/// Laenge eines einzelnen `read`. Sonst koennte ein `read` 64 KiB
/// liefern und der naechste Aufruf waere wieder bei null — die
/// Obergrenze waere dann eine Zahl ohne Wirkung.
struct LeseKette<'a> {
    leitung: &'a mut dyn LeseSchreiben,
    puffer: Vec<u8>,
    gelesen: usize,
}

impl<'a> LeseKette<'a> {
    fn neu(leitung: &'a mut dyn LeseSchreiben) -> LeseKette<'a> {
        LeseKette {
            leitung,
            puffer: Vec::new(),
            gelesen: 0,
        }
    }

    /// Die naechste vollstaendige Handshake-Nachricht: Typ und die
    /// **vollstaendige** Nachricht (mit dem 4-Byte-Kopf), denn genau
    /// so gehoeren die Bytes in den Transkript.
    fn naechste_nachricht(&mut self) -> Result<(u8, Vec<u8>), ServerFehler> {
        let (typ, inhalt, _voll) = self.naechste_aus_klartext()?;
        Ok((typ, record::baue_nachricht(typ, &inhalt)?))
    }

    fn naechste_aus_klartext(&mut self) -> Result<NachrichtTeil, ServerFehler> {
        loop {
            if let Some(treffer) = self.aus_puffer()? {
                return Ok(treffer);
            }
            self.fuellen()?;
        }
    }

    /// Schluckt einen kompletten Record, entschluesselt ihn und gibt
    /// die **erste** Handshake-Nachricht daraus zurueck — als
    /// vollstaendige Nachricht mit Kopf, denn `Finished::dekodiere`
    /// erwartet genau die.
    ///
    /// added 30.09.2026, Runde 2. `seq` ist die Sequenzzahl des
    /// empfangenden Stroms, beim Client-`finished` also 0. Nach
    /// `handshakephase()` sind die Nummern je Richtung wieder bei null
    /// (RFC 8446 5.1).
    ///
    /// DER GLEICHE PUFFER WIRD WEITERVERWENDET, und das ist richtig so:
    /// ClientHello und Client-`finished` kommen ueber dieselbe
    /// Leitung, und der Puffer enthaelt nach Schritt 9 die Reste des
    /// ClientHello-Records, falls der Client mehrer geschickt hat. Ein
    /// zweites `LeseKette` wuerde diese Reste **verwerfen** und
    /// Blocken — der Fehler waere ein `Zu` an einer Stelle, an der
    /// nichts kaputt ist.
    fn naechste_verschluesselte(
        &mut self,
        suite: Suite,
        geheim: &[u8; 32],
        seq: u64,
    ) -> Result<(u8, Vec<u8>), ServerFehler> {
        loop {
            if let Some(voll) = self.ganzer_record()? {
                let r = record::records(&voll)?
                    .into_iter()
                    .next()
                    .ok_or(ServerFehler::Unerwartet { war: 0 })?;
                // DER CHANGE_CIPHER_SPEC-KOEDER, und er wird
                // **verworfen**, nicht abgelehnt.
                //
                // added 30.09.2026, Runde 2. GEFUNDEN VON OPENSSL:
                // `openssl s_client` sendet im Kompatibilitaetsmodus nach
                // dem `finished` einen `ChangeCipherSpec`-Record mit
                // Nutzlast `01` (ausserhalb von TLS 1.3), und der
                // Server meldete `unerwartete Handshake-Nachricht 20` —
                // der Handshake war zu diesem Zeitpunkt schon
                // erfolgreich. RFC 8446 Abschnitt 5 sagt genau das:
                // "Implementations MAY send a dummy change_cipher_spec
                // record ... to be ignored by implementations"; die
                // Gegenseite MUSS ihn einfach wegwerfen. Wer ihn
                // ablehnt, verliert die Halfte aller Clients im
                // Middlebox-Kompatibilitaetsmodus.
                if r.typ == record::typen::CHANGE_CIPHER_SPEC {
                    continue;
                }
                if r.typ != record::typen::AEAD_CHIFFERTEXT {
                    // Alles nach dem ServerHello ist verschluesselt
                    // (RFC 8446 5.1). Ein Klartext-Handshake-Record
                    // an dieser Stelle ist ein Angriff oder ein Fehler
                    // im Client — beides darf nicht als gueltige
                    // Nachricht durchgehen.
                    return Err(ServerFehler::Unerwartet { war: r.typ });
                }
                // `schluessel_unprotect` gibt den Klartext **ohne** das
                // letzte Typ-Byte zurueck und dieses getrennt als `u8`.
                let (innen, innerer_typ) = record::schluessel_unprotect(
                    suite,
                    geheim,
                    seq,
                    record::typen::HANDSHAKE,
                    &r.inhalt,
                )?;
                if innerer_typ != record::typen::HANDSHAKE {
                    return Err(ServerFehler::Unerwartet { war: innerer_typ });
                }
                let (typ, inhalt) = record::nachrichten(&innen)?
                    .into_iter()
                    .next()
                    .ok_or(ServerFehler::Unerwartet { war: 0 })?;
                return Ok((typ, record::baue_nachricht(typ, &inhalt)?));
            }
            self.fuellen()?;
        }
    }

    /// Nimmt einen vollstaendigen Record aus dem Puffer, falls einer
    /// drinliegt.
    fn ganzer_record(&mut self) -> Result<Option<Vec<u8>>, ServerFehler> {
        if self.puffer.len() < 5 {
            return Ok(None);
        }
        let laenge = u16::from_be_bytes([self.puffer[3], self.puffer[4]]) as usize;
        let gesamt = 5 + laenge;
        if self.puffer.len() < gesamt {
            return Ok(None);
        }
        let voll: Vec<u8> = self.puffer.drain(..gesamt).collect();
        Ok(Some(voll))
    }

    /// Die erste Handshake-Nachricht aus dem Puffer, falls eine
    /// vollstaendige drinliegt.
    ///
    /// added 30.09.2026. DER WEG: erst auf **5 Bytes** warten, dann die
    /// Laenge aus dem Kopf nehmen und **erst dann** auf den ganzen
    /// Record warten. Wer stattdessen `record::nachrichten` aufruft und
    /// dessen `Err` als „noch nicht da" liest, kann einen halben
    /// Record nicht von einem kaputten unterscheiden.
    fn aus_puffer(&mut self) -> Result<Option<NachrichtTeil>, ServerFehler> {
        let voll = match self.ganzer_record()? {
            Some(v) => v,
            None => return Ok(None),
        };
        let records = record::records(&voll)?;
        let r = records.first().ok_or(ServerFehler::Unerwartet { war: 0 })?;
        if r.typ != record::typen::HANDSHAKE {
            return Err(ServerFehler::KeinClientHello { war: r.typ });
        }
        let nachrichten = record::nachrichten(&r.inhalt)?;
        let (typ, inhalt) = nachrichten
            .first()
            .ok_or(ServerFehler::Unerwartet { war: 0 })?;
        Ok(Some((*typ, inhalt.clone(), voll)))
    }

    /// Fuellt den Puffer nach, mit Grenze.
    fn fuellen(&mut self) -> Result<(), ServerFehler> {
        // M3: die Grenze wird **vor** dem Lesen geprueft, damit kein
        // weiteres `read` in einen Puffer laeuft, der schon zu gross ist.
        if self.gelesen >= MAX_GELESEN {
            return Err(ServerFehler::ZuViel(
                "mehr als MAX_GELESEN Byte im Handshake",
            ));
        }
        let mut stueck = [0u8; 4096];
        let n = self.leitung.read(&mut stueck).map_err(|e| {
            // DIE EINORDNUNG, und sie gehoert hierher und nicht im
            // Aufrufer: nur hier ist der `io::Error` noch da und
            // laesst sich nach seiner Art ansehen. `WouldBlock` und
            // `TimedOut` bedeuten unter Linux dasselbe (abgelaufenes
            // `SO_RCVTIMEO` -> `EAGAIN`).
            if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut
            {
                return ServerFehler::LesezeitUeberschritten;
            }
            ServerFehler::Netz(e.to_string())
        })?;
        if n == 0 {
            return Err(ServerFehler::Zu);
        }
        self.gelesen += n;
        self.puffer.extend_from_slice(&stueck[..n]);
        Ok(())
    }

    fn schreiben_record(&mut self, record: &[u8]) -> Result<(), ServerFehler> {
        self.leitung
            .write_all(record)
            .map_err(|e| ServerFehler::Netz(e.to_string()))?;
        self.leitung
            .flush()
            .map_err(|e| ServerFehler::Netz(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Anwendungsdaten
// ---------------------------------------------------------------------------

/// Der Strom **nach** dem Handshake: Lesen und Schreiben von
/// Anwendungsdaten ueber dieselbe Leitung.
///
/// added 30.09.2026, Runde 2, TLS R1. OHNE DIESEN TYP gibt es keinen
/// Weg, nach `handshake` auch nur ein Byte zu bewegen — `Ausgehandelt`
/// liefert nur die Geheimnisse. Das ist der Punkt, an dem `narsil-server`
/// sonst selbst AEAD rechnen muesste, und zwar an einer Stelle, an der
/// die Sequenznummern beider Richtungen stimmen muessen.
///
/// DIE ZWEI GEHEIMNISSE, und sie sind nicht austauschbar: `schreiben`
/// ist das **Server**-Anwendungsgeheimnis, `lesen` das **Client**-
/// Anwendungsgeheimnis. Vertauscht sieht man einen authentifizierten
/// Datensatz, der sich nicht lesen laesst — und die Meldung ist ein
/// AEAD-Fehler, nicht „die Schluessel sind vertauscht".
///
/// DIE ZEITGRENZEN, und sie kommen **nicht** von hier: die Leitung
/// gehoert dem Aufrufer (ein `TcpStream` mit gesetzten
/// `read_timeout`), und dieses Typ setzt nichts daran. Wer eine Frist
/// braucht, setzt sie am Sockel.
pub struct AnwendungsStrom<'a> {
    leitung: &'a mut dyn LeseSchreiben,
    suite: Suite,
    /// Das **Server**-Anwendungsgeheimnis, zum Senden.
    ///
    /// added 30.09.2026. DER WERT UND SEIN TYP, und das ist eine
    /// bewusste Entscheidung: hier steht das *Geheimnis*, nicht der
    /// daraus expandierte Schluessel. `record::schluessel_protect`
    /// erwartet das Geheimnis und macht `keying_material` **selbst**
    /// (`record.rs`: `let (roher_schluessel, iv) = suite.keying_material(secret)`).
    /// Ein Client-Sockel in `strom.rs` haelt den expandierten
    /// Schluessel, weil er den einmal berechnet — fuer den Server
    /// waere dasselbe moeglich, aber dann muessten `iv` und
    /// `lese_iv` hier stehen bleiben und der Compiler wuerde fragen,
    /// warum sie nicht benutzt werden (genau das war die Warnung, die
    /// 30.09.2026 kam: "fields iv and lese_iv are never read").
    geheimnis: [u8; 32],
    seq: u64,
    /// Das **Client**-Anwendungsgeheimnis, zum Empfang.
    lese_geheimnis: [u8; 32],
    lese_seq: u64,
    /// Klartext, der schon entschluesselt, aber noch nicht ausgegeben wurde.
    klattr: Vec<u8>,
    lesepos: usize,
    /// **Chiffrat**, das schon von der Leitung gelesen, aber noch nicht
    /// entpackt ist: der Rest aus dem Handshake
    /// (`Ausgehandelt::rest_bytes`).
    ///
    /// added 30.09.2026, Runde 2, TLS R1c. Ein eigener Puffer und nicht
    /// `klattr`, weil hier noch **verschluesselte** Bytes liegen:
    /// `klattr` ist der Klartext **nach** `schluessel_unprotect`, und
    /// ein Anwendungsrecord, der hier hinein ginge, waere mit dem
    /// falschen Schluessel geoeffnet.
    roh: Vec<u8>,
}

/// M3: so viele `ChangeCipherSpec`-Koeder werden verworfen, bevor es
/// ein Fehler wird.
///
/// added 30.09.2026. DER WERT: 4. Ein Client im Kompatibilitaetsmodus
/// schickt **zwei** (einer vor dem `finished`, einer danach, RFC 8446
/// Anhang D.4). Vier laesst Platz fuer einen Client, der es twice
/// macht, und schuetzt vor der Schleife ohne Ende, in der ein Client
/// nichts ausser Koedern schickt.
pub const MAX_KOEDER: usize = 4;

/// M3: so viele Byte werden bei einem `schreiben` in einen Record gepackt.
///
/// DER WERT: `record::MAX_INNEN` minus 1, weil der innere Typ als
/// letztes Byte dazukommt. Es ist die Record-Grenze selbst, nicht eine
/// gerundete Zahl — eine eigene Zahl waere eine zweite Wahrheit, die
/// auseinanderlaufen kann, wenn man `record.rs` aendert.
pub const MAX_SCHREIB: usize = record::MAX_INNEN - 1;

impl<'a> AnwendungsStrom<'a> {
    /// Macht aus dem Ergebnis eines Handshakes einen Lese-/Schreibstrom.
    ///
    /// added 30.09.2026. Es passiert hier **nichts** als zu kopieren:
    /// Schluessel und Nonce werden pro Record aus dem Geheimnis
    /// expandiert, weil `record::schluessel_protect` und
    /// `schluessel_unprotect` das so erwarten (siehe die Begruendung am
    /// Feld `geheimnis`). Wer die Expandierung einmal machen will, muss
    /// sie in `record.rs` verschieben — nicht hier, denn dort liegen die
    /// beiden Stellen, die sie benutzen.
    pub fn neu(
        leitung: &'a mut dyn LeseSchreiben,
        ausgehandelt: &Ausgehandelt,
    ) -> AnwendungsStrom<'a> {
        AnwendungsStrom {
            leitung,
            suite: ausgehandelt.suite,
            geheimnis: ausgehandelt.schreiben,
            // added 01.10.2026 13:9x, R4z6: **`gesendete_records` statt
            // `0`.** Vorher stand hier `seq: 0`, obwohl
            // `handshake_mit_tickets` das Ticket mit Sequenz 0 geschickt
            // hatte — der erste Anwendungsrecord wiederholte die Nonce.
            // GEMESSEN: `Verschluesselung (TLS_AES_128_GCM_SHA256, lesen)`.
            //
            // WARUM DAS HIER EINE NEUE QUELLE IST und kein `&mut self`:
            // `AnwendungsStrom` bekommt den Sockel **von aussen** und
            // weiss nichts von dem, was der Handshake geschickt hat. Die
            // Zahl muss der Schreibende mitgeben — wie `rest_bytes`, das
            // aus demselben Grund dort steht.
            seq: ausgehandelt.gesendete_records,
            lese_geheimnis: ausgehandelt.lesen,
            lese_seq: 0,
            klattr: Vec::new(),
            lesepos: 0,
            roh: ausgehandelt.rest_bytes.clone(),
        }
    }

    /// Liest bis `ziel.len()` Byte Klartext. `Ok(0)` heisst Ende der
    /// Verbindung (der Peer hat `close_notify` geschickt).
    ///
    /// added 30.09.2026. DER WEG: erst der Rest aus dem letzten
    /// Record, dann ein neuer Record. Das ist wichtig, weil ein `read`
    /// **weniger** Bytes liefern darf als verlangt — ein Aufrufer, der
    /// daraus eine Zeile baut, wuerde sonst mitten in einer Zeile
    /// aufgetehen und der Peer's Antwort als Echo des Absenders lesen.
    pub fn lesen(&mut self, ziel: &mut [u8]) -> Result<usize, ServerFehler> {
        if ziel.is_empty() {
            return Ok(0);
        }
        while self.lesepos >= self.klattr.len() {
            self.klattr.clear();
            self.lesepos = 0;
            let (daten, typ) = self.lesen_record()?;
            // DER INNERE TYP, und er wird unterschieden: `close_notify`
            // (0x01) ist das Ende, alles andere sind Anwendungsdaten.
            // Wer beides in einen Topf wirft, liefert dem Aufrufer
            // Steuerbytes als Nutzlast.
            if typ == CLOSE_NOTIFY {
                return Ok(0);
            }
            self.klattr.extend_from_slice(&daten);
        }
        let n = ziel.len().min(self.klattr.len() - self.lesepos);
        ziel[..n].copy_from_slice(&self.klattr[self.lesepos..self.lesepos + n]);
        self.lesepos += n;
        Ok(n)
    }

    /// Schreibt Klartext; kuerzt auf `MAX_SCHREIB` ab, wenn es mehr ist.
    ///
    /// added 30.09.2026. WARUM **KUERZEN** UND NICHT ABLEHNEN: ein
    /// `Write`-Aufruf darf weniger Bytes annehmen, als angeboten wurden
    /// (`write` liefert die Anzahl). Ein Aufrufer, der das nicht
    /// beachtet, verliert sonst Daten ohne Fehlermeldung. Fuer
    /// Zeilen-Echos ist die Grenze (16 KiB) weit entfernt.
    pub fn schreiben(&mut self, daten: &[u8]) -> Result<usize, ServerFehler> {
        let n = daten.len().min(MAX_SCHREIB);
        // M3: `u64::MAX` heisst „Sequenzzahl erschoepft" (RFC 8446
        // 5.5.1). Danach waere ein Wrapping-Zaehler eine
        // Wiederverwendung von Nonce und Schluessel.
        if self.seq == u64::MAX || self.lese_seq == u64::MAX {
            return Err(ServerFehler::ZuViel("Sequenzzahl erschoepft"));
        }
        let chiffrat = record::schluessel_protect(
            self.suite,
            &self.geheimnis,
            self.seq,
            record::typen::APPLICATION_DATA,
            &daten[..n],
        )?;
        self.seq += 1;
        self.leitung
            .write_all(&record::baue_record(
                record::typen::AEAD_CHIFFERTEXT,
                &chiffrat,
            )?)
            .map_err(|e| ServerFehler::Netz(e.to_string()))?;
        self.leitung
            .flush()
            .map_err(|e| ServerFehler::Netz(e.to_string()))?;
        Ok(n)
    }

    /// Liest einen Record und entpackt ihn.
    ///
    /// added 30.09.2026. DIE SCHLEIFE STATT EINER REKURSION, und das ist
    /// die Regel M3 in ihrer einfachsten Form: ein Client kann beliebig
    /// viele `ChangeCipherSpec`-Koeder schicken, und eine Rekursion
    /// darueber waere ein Stapelueberlauf mit einem Text, den niemand
    /// dem `ChangeCipherSpec` zuordnet. `MAX_KOEDER` begrenzt die
    /// Schleife; ein echter Client schickt hoechstens zwei (vor und nach
    /// dem `finished`).
    fn lesen_record(&mut self) -> Result<(Vec<u8>, u8), ServerFehler> {
        let mut koeder_seen = 0usize;
        loop {
            // DER EIGENE LESEWEG, und nicht `LeseKette`: die hat die
            // Handshake-Nachrichten im Blick und lehnt alles ab, was kein
            // Handshake-Record ist. Hier ist alles ausser
            // `application_data` von aussen verdächtig.
            let mut kette = LeseKette::neu(self.leitung);
            // DER REST AUS DEM HANDSHAKE, und er ist der Grund fuer
            // `roh`: diese Bytes sind von der Leitung **schon gelesen**,
            // aber noch nicht entpackt. Ohne sie wuerde der erste
            // Anwendungsrecord des Clients ignoriert, wenn er im selben
            // TCP-Segment kam wie sein `finished`.
            kette.puffer.append(&mut self.roh);
            let voll = loop {
                if let Some(v) = kette.ganzer_record()? {
                    break v;
                }
                kette.fuellen()?;
            };
            let r = record::records(&voll)?
                .into_iter()
                .next()
                .ok_or(ServerFehler::Unerwartet { war: 0 })?;
            // Auch hier gilt die Kompatibilitaetsregel: ein
            // `ChangeCipherSpec` zwischen zwei Anwendungsrecords wird
            // verworfen (RFC 8446 5, Begruendung siehe im Handshake).
            if r.typ == record::typen::CHANGE_CIPHER_SPEC {
                koeder_seen += 1;
                if koeder_seen > MAX_KOEDER {
                    return Err(ServerFehler::ZuViel("zu viele ChangeCipherSpec-Koeder"));
                }
                continue;
            }
            if r.typ != record::typen::AEAD_CHIFFERTEXT {
                return Err(ServerFehler::Unerwartet { war: r.typ });
            }
            return self.entpacken(&r.inhalt);
        }
    }

    /// Entschluesselt den Inhalt eines verschluesselten Records.
    fn entpacken(&mut self, inhalt: &[u8]) -> Result<(Vec<u8>, u8), ServerFehler> {
        // DER INNERE TYP, und das Argument ist nur Dokumentation:
        // `schluessel_unprotect` benutzt den Typ aus dem Klartext
        // (`record.rs`, `let letzte = innen.pop()`), nicht den Parameter.
        // DIE FEHLERMELDUNG, und sie nennt die drei Groessen, an denen
        // ein Authentifizierungsfehler liegt: die Sequenzzahl, die
        // Laenge des Chiffrats und die Suite. added 30.09.2026 nach dem
        // Lauf gegen `openssl s_client`: die Meldung war vorher
        // `die Authentifizierung schlug fehl` und damit bei vier
        // moeglichen Ursachen (Schluessel, Nonce, AAD, Sequenzzahl)
        // gleich gut und gleich schlecht.
        let (daten, typ) = record::schluessel_unprotect(
            self.suite,
            &self.lese_geheimnis,
            self.lese_seq,
            record::typen::APPLICATION_DATA,
            inhalt,
        )
        .map_err(|e| {
            ServerFehler::Record(format!(
                "{e} (Suite {:#06x}, Sequenzzahl {}, Chiffrat {} Byte, Richtung Client->Server)",
                self.suite.code(),
                self.lese_seq,
                inhalt.len()
            ))
        })?;
        self.lese_seq += 1;
        Ok((daten, typ))
    }
}

// ---------------------------------------------------------------------------
// Anschluss an std::io
// ---------------------------------------------------------------------------

/// `AnwendungsStrom` ist ein `Read`.
///
/// added 01.10.2026, R3. DER GRUND: `narsil_server::Server::bediene`
/// nimmt `R: BufRead` und `W: Write`, und der HTTPS-Weg soll
/// **denselben** Router und Handler bedienen wie der Klartext-Weg —
/// nicht eine zweite HTTP-Implementierung. `Ok(0)` heisst Ende
/// (`close_notify`), Fehler werden zu `io::Error` mit unveraendertem
/// Text.
impl std::io::Read for AnwendungsStrom<'_> {
    fn read(&mut self, ziel: &mut [u8]) -> std::io::Result<usize> {
        AnwendungsStrom::lesen(self, ziel).map_err(|e| std::io::Error::other(e.to_string()))
    }
}

/// `AnwendungsStrom` ist ein `Write`.
///
/// added 01.10.2026, R3. DER VERTRAG, und er ist der ganze Grund, warum
/// hier `Ok(n)` und nicht `Err`: `Write::write` darf eine **Teilmenge**
/// annehmen. `schreiben` kuerzt auf `MAX_SCHREIB` (16 KiB minus 1) und
/// liefert die Zahl; `write_all` fragt solange nach. Ein Fehler bei
/// Teilschreibung wuerde eine 20-KiB-Antwort als Fehler melden und den
/// Antworttext abschneiden — ein haengender Server statt einer
/// vollstaendigen Antwort.
impl std::io::Write for AnwendungsStrom<'_> {
    fn write(&mut self, daten: &[u8]) -> std::io::Result<usize> {
        AnwendungsStrom::schreiben(self, daten).map_err(|e| std::io::Error::other(e.to_string()))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// `close_notify` als **innerer** Typ (RFC 8446 Abschnitt 5.1, Alert 0).
///
/// added 30.09.2026. Eigene Konstante, weil `record::typen` nur die
/// aeusseren Record-Typen kennt; der innere Typ steht im letzten Byte
/// des Klartexts und ist ein Alert-Code.
pub const CLOSE_NOTIFY: u8 = 0x01;

#[cfg(test)]
mod tests {
    use super::*;

    /// added 01.10.2026 22:6x UTC (Projektlabel 02.10.2026), Rolle TLS.
    /// DER FALL, UM DEN ES GEHT: der Client bietet `psk_dhe_ke` an
    /// (`psk_modi: [1]`), schickt aber **kein** `pre_shared_key` — der
    /// normale erste Kontakt zu einem unbekannten Server.
    ///
    /// WARUM DIESER TEST BIS HIER FEHLTE: `ClientHello::baue` setzt
    /// `psk_modi: vec![1]` immer, und die Integrationstests
    /// (`psk_rundlauf.rs`) fahren den vollen Handshake darueber — sie
    /// bemerken aber nichts, denn ein `Err` fuehrt dort auch zum vollen
    /// Handshake, nur mit einer Zeile auf stderr. Der **Zustand** war also
    /// richtig und die **Diagnose** falsch: `ANGEBOT ABGELEHNT: das Angebot
    /// ist kaputt (Identitaeten: 0, psk_modi: [1])` steht bei jedem ersten
    /// Kontakt im Log, und das sieht nach einem Angriff aus. GEMESSEN
    /// 01.10.2026 22:4x in `narsil-net/tests/alpn_client.rs` (dort mitten in
    /// einem Fehlschlag) und in `narsil-server/examples/serve_https.rs`
    /// Zeile 442.
    fn server() -> TlsServer {
        TlsServer::neu(ServerAngebot {
            zertifikat: include_bytes!("../testdaten/testzertifikat.der").to_vec(),
            // added 01.10.2026 22:6x: 32 Byte, die kein gueltiger Schluessel
            // sind. Sie werden hier **nicht** gebraucht: `angebot_annehmen`
            // liest weder Zertifikat noch Schluessel, es geht um die
            // Angebotsentscheidung. `ServerAngebot::pruefen` verlangt aber
            // eine Laenge, und 32 Byte sind die kleinste zulaessige.
            privat: vec![7u8; 32],
            alpn: vec![b"h2".to_vec()],
        })
        .expect("ServerAngebot")
    }

    /// added 01.10.2026 22:6x: `psk_key_exchange_modes` ja, `pre_shared_key`
    /// nein -> **kein Angebot** (`Ok(None)`), kein Ablehnungsgrund.
    #[test]
    fn modus_ohne_angebot_ist_kein_angebot() {
        let ch = crate::nachricht::ClientHello::baue(
            [3u8; 32],
            Some(b"localhost"),
            &[b"h2"],
            &[1u8; 32],
            &[2u8; 32],
        )
        .expect("ClientHello");
        assert_eq!(ch.psk_modi, vec![1], "der Test braucht psk_dhe_ke");
        assert!(ch.psk.is_none(), "und ausdruecklich kein Angebot");
        let bytes = ch.kodiere();
        let s = server();
        let tickets = crate::ticket::Tickets::neu();
        let r = s.angebot_annehmen(&ch, &bytes, &tickets);
        assert_eq!(
            r,
            Ok(None),
            "ohne pre_shared_key gibt es nichts anzunehmen — RFC 8446 4.2.11"
        );
    }

    /// added 01.10.2026 22:6x: GEGENPROBE. Ein Client **mit** Angebot, das
    /// wir nicht kennen, muss weiterhin **abgelehnt** werden — sonst haette
    /// der neue Zweig die Sitzungspflicht abgeschafft.
    ///
    /// added 01.10.2026 22:6x: DER BLOCK WIRD NICHT VON HAND GEBAUT. Die
    /// erste Fassung dieses Tests hat den `pre_shared_key`-Block selbst
    /// zusammengestellt und GEMESSEN `Err("das Angebot ist kaputt")`
    /// bekommen statt `Err("unbekannte Identitaet")` — mein Aufbau war
    /// falsch. Die Crate hat mit `ticket::psk_anbieten` genau den
    /// Kodierer dafuer, und ein Test, der den Layout selbst nachbaut,
    /// prueft den Layout statt der Entscheidung.
    #[test]
    fn angebot_mit_unbekannter_identitaet_wird_abgelehnt() {
        let mut ch = crate::nachricht::ClientHello::baue(
            [4u8; 32],
            Some(b"localhost"),
            &[b"h2"],
            &[1u8; 32],
            &[2u8; 32],
        )
        .expect("ClientHello");
        let vorschlaege = [crate::ticket::PskVorschlag {
            identitaet: b"unbekannt".to_vec(),
            verschleiertes_alter: 0,
        }];
        let binder = crate::ticket::psk_anbieten(&mut ch, &vorschlaege, &[0x5au8; 32])
            .expect("Angebot bauen");
        assert!(ch.psk.is_some(), "der Test braucht ein Angebot");
        let bytes = ch.kodiere();
        let s = server();
        // Der Speicher ist **leer**: es gibt kein Ticket zu dieser
        // Identitaet, und genau das ist der Fall, der hier geprueft wird.
        let tickets = crate::ticket::Tickets::neu();
        let r = s.angebot_annehmen(&ch, &bytes, &tickets);
        assert_eq!(
            r,
            Err("unbekannte Identitaet"),
            "eine unbekannte Identitaet muss mit benanntem Grund abgelehnt \
             werden; der Binder ({} Byte) spielt dabei keine Rolle",
            binder.len()
        );
    }
}

#[cfg(test)]
mod kette_handshake_tests {
    use super::{ServerAngebot, TlsServer};
    use std::io::Write as _;
    use std::net::TcpListener;
    use std::time::Duration;

    /// added 03.10.2026 02:4x, R48. **DER ENDE-ZU-ENDE-BEWEIS**, und er ist das,
    /// was `PROBLEME.txt` Nr. 46 seit zwei Runden fehlte: ein Server mit einem
    /// Zertifikat, das `must-staple` verlangt, muss vom Client **abgewiesen**
    /// werden — und zwar mit dem benannten Grund, nicht mit „handshake failed“.
    ///
    /// added 03.10.2026 02:4x: **WARUM DAS BISHER UNMOEGLICH WAR**, und es ist
    /// gemessen: OCSP-Auswertung (`strom.rs:1424`) und must-staple-Pruefung
    /// (`strom.rs:1461`) verlangen beide `ganze_kette.len() >= 2`, und unser
    /// Server sendete **ein** Blatt. Deshalb sendet er jetzt ueber
    /// `TlsServer::neu_mit_kette` die Kette `[Blatt, CA]`.
    ///
    /// added 03.10.2026 02:4x: **DER GEGENFALL GEHOERT DIZU**, und ohne ihn
    /// waere der Test untauglich: dasselbe Paar **ohne** Merkmal muss
    /// durchkommen. Sonst prueft der Test nur, dass jede Verbindung zu
    /// `localhost` scheitert.
    ///
    /// added 03.10.2026 02:5x: **PEM**, weil `Vertrauensspeicher::aus_pem` PEM
    /// nimmt. 12 Zeilen Base64; es gibt in `src/` keinen Helfer, nur in
    /// `tests/` (GEMESSEN) — und ich darf `src/` von fremden Tests nicht
    /// abhaengig machen, also steht er hier.
    fn base64(e: &[u8]) -> String {
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut r = String::new();
        for block in e.chunks(3) {
            let b = [
                block[0],
                *block.get(1).unwrap_or(&0),
                *block.get(2).unwrap_or(&0),
            ];
            let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
            r.push(T[(n >> 18) as usize & 63] as char);
            r.push(T[(n >> 12) as usize & 63] as char);
            r.push(if block.len() > 1 {
                T[(n >> 6) as usize & 63] as char
            } else {
                '='
            });
            r.push(if block.len() > 2 {
                T[n as usize & 63] as char
            } else {
                '='
            });
        }
        r
    }

    fn pem_von(der: &[u8]) -> String {
        let mut s = String::from("-----BEGIN CERTIFICATE-----\n");
        for zeile in base64(der).as_bytes().chunks(64) {
            s.push_str(std::str::from_utf8(zeile).unwrap_or(""));
            s.push('\n');
        }
        s.push_str("-----END CERTIFICATE-----\n");
        s
    }

    /// added 03.10.2026 02:5x: **DER AUFBAU**, und er macht die Aussage
    /// sichtbar: Server und Client benutzen **denselben** Testschluessel, aber
    /// der Client kennt nur die **CA**. Das Blatt muss also ueber die Kette
    /// kommen — genau das, was vorher niemand testen konnte.
    fn setup() -> Result<(Vec<u8>, Vec<u8>), &'static str> {
        let ca = crate::x509::bauen::zertifikat_ca()?;
        let blatt = crate::x509::bauen::zertifikat(true)?;
        Ok((ca, blatt))
    }

    fn starte_server(blatt: Vec<u8>, ca: Vec<u8>) -> Result<u16, &'static str> {
        starte_server_mit_ocsp(blatt, ca, None)
    }

    /// added 03.10.2026 03:6x, R49. **DER SERVER MIT STAPELUNG** — die dritte
    /// Variante, und sie ist kein Extra-Pfad, sondern derselbe Aufbau mit einem
    /// zusaetzlichen Argument.
    fn starte_server_mit_ocsp(
        blatt: Vec<u8>,
        ca: Vec<u8>,
        ocsp: Option<Vec<u8>>,
    ) -> Result<u16, &'static str> {
        let lauscher = TcpListener::bind("localhost:0").map_err(|_| "Bind")?;
        let port = lauscher.local_addr().map_err(|_| "Addr")?.port();
        std::thread::spawn(move || {
            for _ in 0..400 {
                let _ = lauscher.set_nonblocking(true);
                let sock = match lauscher.accept() {
                    Ok((s, _)) => s,
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                };
                let _ = sock.set_nonblocking(false);
                let _ = sock.set_read_timeout(Some(Duration::from_secs(15)));
                let _ = sock.set_write_timeout(Some(Duration::from_secs(15)));
                let mut leitung: Box<dyn super::LeseSchreiben> = Box::new(sock);
                if let Ok(server) = TlsServer::neu_mit_kette_und_ocsp(
                    ServerAngebot::neu_test(blatt, vec![b"h2".to_vec()]),
                    vec![ca],
                    ocsp,
                ) {
                    let mut tickets = crate::ticket::Tickets::neu();
                    let _ = server.handshake_mit_tickets(&mut leitung, &mut tickets, false);
                }
                return;
            }
        });
        Ok(port)
    }

    /// added 03.10.2026 02:6x: **DER CLIENT**, mit Zeitgrenzen (Netz im Test:
    /// 15 s Lesefrist, 20 s auf der Client-Seite) — die Regel aus M1..M9 gilt
    /// auch fuer Netztests, und dieser Test **ist** einer.
    // added 03.10.2026 02:7x: DER TYP ist `TlsVerbindung`, nicht `TlsStrom` —
    // mein erster Versuch schrieb `TlsStrom` und der Uebersetzer meinte
    // „expected `Result<TlsStrom, _>`, found `Result<TlsVerbindung…>`“.
    fn verbinde(port: u16, ca_pem: &str) -> Result<crate::strom::TlsVerbindung, String> {
        let socket =
            std::net::TcpStream::connect(("localhost", port)).map_err(|e| e.to_string())?;
        let _ = socket.set_read_timeout(Some(Duration::from_secs(20)));
        let _ = socket.set_write_timeout(Some(Duration::from_secs(20)));
        let speicher =
            crate::kette::Vertrauensspeicher::aus_pem(ca_pem).map_err(|e| e.to_string())?;
        let jetzt = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        crate::strom::TlsVerbindung::ueber_sockel(socket, "localhost", &[b"h2"], &speicher, jetzt)
            .map_err(|e| format!("{:?}", e))
    }

    #[test]
    fn must_staple_bricht_den_handshake_ab() {
        let (ca, blatt) = setup().unwrap_or_else(|_| panic!("Setup"));
        let port = starte_server(blatt, ca.clone()).unwrap_or_else(|_| panic!("Server"));
        let pem = pem_von(&ca);
        let e = match verbinde(port, &pem) {
            Ok(_) => panic!("must-staple wurde angenommen — der Test ist damit falsch"),
            Err(text) => text,
        };
        assert!(
            e.contains(crate::x509::MUST_STAPLE_FEHLER),
            "die Meldung nennt den Grund nicht: {e}"
        );
    }

    /// added 03.10.2026 03:7x, R49. **DER DRITTE MUST-STAPLE-FALL**, und er ist
    /// der einzige, der **positiv** endet: das Blatt verlangt die Stapelung, der
    /// Server **liefert** sie, und die Verbindung muss **stehen**.
    /// Bisher waren nur zwei Faelle gemessen — kein Stapel → abgelehnt (R48),
    /// kein Merkmal → steht. Ohne diesen Test waere unentschieden, ob unsere
    /// Stapelung im Client ueberhaupt **ausgewertet** wird oder ob jedes
    /// Merkmal 5 unabhaengig vom Stapel zu einem Abbruch fuehrt.
    #[test]
    fn must_staple_mit_gelieferter_stapelung_steht_die_verbindung() {
        let ca = crate::x509::bauen::zertifikat_ca().unwrap_or_else(|_| panic!("CA"));
        let blatt = crate::x509::bauen::zertifikat(true).unwrap_or_else(|_| panic!("Blatt"));
        let ocsp =
            crate::x509::bauen::ocsp_antwort_gut(&ca, &crate::x509::bauen::blatt_seriennummer())
                .unwrap_or_else(|_| panic!("OCSP-Antwort"));
        let port = starte_server_mit_ocsp(blatt, ca.clone(), Some(ocsp))
            .unwrap_or_else(|_| panic!("Server"));
        let pem = pem_von(&ca);
        let r = verbinde(port, &pem);
        assert!(
            r.is_ok(),
            "mit gelieferter Stapelung muss die Verbindung stehen: {:?}",
            r.err()
        );
        let mut strom = r.unwrap_or_else(|_| unreachable!());
        let _ = strom.flush();
    }

    /// added 03.10.2026 04:5x, R49. **DIE SICHERHEITSWIRKUNG DES DECODER-FIXES.**
    /// Der Server stapelt einen **WIDERRUF** fuer dieses Zertifikat, und die
    /// Verbindung muss abgelehnt werden.
    ///
    /// added 03.10.2026 04:5x: **WARUM DIESER TEST DER WICHTIGSTE DER RUNDE
    /// IST.** Vor dem Fix (04:1x) hat `Certificate::dekodiere` die Erweiterungen
    /// **verworfen**. Ein stapelnder Widerruf kam damit **nie** bei `ocsp::beweise`
    /// an — fuer unseren Client war ein widerrufenes Zertifikat gleichwertig mit
    /// einem guten. Dieser Test faellt ohne den Fix **durch** (die Verbindung
    /// kaeme zustande), mit ihm faellt er **durch die Ablehnung**. Er ist der
    /// messbare Unterschied zwischen „wir pruefen Widerrufe“ und „wir haben es
    /// behauptet“.
    #[test]
    fn stapelt_der_server_einen_widerruf_wird_abgelehnt() {
        let ca = crate::x509::bauen::zertifikat_ca().unwrap_or_else(|_| panic!("CA"));
        let blatt = crate::x509::bauen::zertifikat(false).unwrap_or_else(|_| panic!("Blatt"));
        let ocsp = crate::x509::bauen::ocsp_antwort_widerrufen(
            &ca,
            &crate::x509::bauen::blatt_seriennummer(),
        )
        .unwrap_or_else(|_| panic!("OCSP-Antwort"));
        let port = starte_server_mit_ocsp(blatt, ca.clone(), Some(ocsp))
            .unwrap_or_else(|_| panic!("Server"));
        let pem = pem_von(&ca);
        match verbinde(port, &pem) {
            Ok(_) => panic!(
                "ein widerrufener Stapel wurde angenommen — der Decoder verliert Erweiterungen?"
            ),
            Err(e) => assert!(
                e.contains("WIDERRUF"),
                "die Meldung nennt den Widerruf nicht: {e}"
            ),
        }
    }

    #[test]
    fn ohne_merkmal_kommt_die_verbindung_zustande() {
        let ca = crate::x509::bauen::zertifikat_ca().unwrap_or_else(|_| panic!("CA"));
        let blatt = crate::x509::bauen::zertifikat(false).unwrap_or_else(|_| panic!("Blatt"));
        let port = starte_server(blatt, ca.clone()).unwrap_or_else(|_| panic!("Server"));
        let pem = pem_von(&ca);
        let r = verbinde(port, &pem);
        assert!(
            r.is_ok(),
            "ohne Merkmal muss die Verbindung klappen: {:?}",
            r.err()
        );
        let mut strom = r.unwrap_or_else(|_| unreachable!());
        let _ = strom.flush();
    }
}
