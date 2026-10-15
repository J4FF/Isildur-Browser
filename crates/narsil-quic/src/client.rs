//! `client.rs` — ein minimaler QUIC-/HTTP/3-Client: `hole(url)`.
//!
//! Q6a: koaleszierte Datagramme zerlegen (`datagramm_zerlegen` aus
//!      `verbindung.rs`), jedes Teilpaket mit den Schluesseln SEINER Ebene
//!      entschuetzen, und Pakete ohne Schluessel **puffern** statt verwerfen.
//! Q6b: Cipher Suite des Servers beachten, Transcript = ClientHello+ServerHello
//!      als Handshake-Nachrichten (4-Byte-Kopf, kein Record-Kopf).
//! Q6c: Handshake bis zum `Finished` beider Seiten, 1-RTT, HTTP/3-Steuerstrom
//!      mit SETTINGS, GET auf Stream 0, Antwort lesen.
//! Q12:  Zertifikatskette, Hostname und `CertificateVerify` mit `narsil-tls`
//!      pruefen.
//! Q13:  Stateless Reset erkennen, Idle-Zeitlimit, Grenzen fuer offene Streams
//!      und gepufferte Byte.
//!
//! ## Die zwei Fehler, an denen der echte Server nach dem ServerHello schwieg
//!
//! Beide sind hier dokumentiert, weil beide **wie** ein "der Server antwortet
//! nicht" aussehen und keiner von ihnen eine Fehlermeldung erzeugt:
//!
//! 1. **Die Destination Connection ID.** Nach dem ersten Serverpaket ist die
//!    DCID nicht mehr die eigene Zufalls-ID aus dem ClientHello, sondern die
//!    Source Connection ID des Servers (RFC 9000 Abschnitt 7.2). Ein Client,
//!    der seine eigene ID weiter schickt, wird stillschweigend verworfen. Und
//!    weil der Server sein Anti-Verstaerkungsbudget (drei Byte gesendet je
//!    Byte empfangen, RFC 9000 Abschnitt 8.1) mit Initial plus Zertifikatskette
//!    nicht fuellen kann, bleibt die Handshake-Flug aus.
//!
//! 2. **Die Connection-ID-Laenge im Short Header.** 1-RTT-Pakete tragen die
//!    Laenge ihrer Connection-ID **nicht** im Kopf; sie steht nur im Zustand
//!    beider Seiten. Ein Client, der sie mit Laenge 0 baut, schickt Pakete,
//!    die der Server nicht zuordnen kann. Beim Senden wie beim Empfangen.
//!
//! Beide Stellen sind mit `DER ZWEITE ORT DIESES FEHLERS` markiert.

use std::collections::BTreeMap;
use std::net::{ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

use crate::h3::{H3Rahmen, h3_lesen, h3_schreiben};
use crate::kopf::{KopfFehler, MAX_CID_LAENGE, TYP_HANDSHAKE, TYP_INITIAL, lange_schreiben};
use crate::parameter::{Parameter, parameter_lesen, parameter_schreiben};
use crate::qpack::{FeldZeile, feldabschnitt_dekodieren, feldabschnitt_kodieren};
use crate::rahmen::{AckBereich, MAX_ACK_BEREICHE, Rahmen, rahmen_alle, rahmen_schreiben};
use crate::schutz::{
    PaketSchluessel, SchutzFehler, Suite, entschuetzen, initial_geheimnis, initial_schuetzen,
    kurzes_schuetzen, retry_tag, richtungs_geheimnis, schluessel_ableiten,
};
use crate::varint;
use crate::verbindung::{Ebene, datagramm_zerlegen};
use narsil_tls::nachricht::{
    Certificate, CertificateVerify, ClientHello, EncryptedExtensions, Finished, ServerHello, typen,
};
use narsil_tls::record::Schluesselplan;
use narsil_tls::x25519;

// ---------------------------------------------------------------------------
// Grenzen (M3/M4). Jede Zahl haengt an fremder Eingabe, also ist jede benannt.
// ---------------------------------------------------------------------------

/// Groesster UDP-Puffer (M4). Das groesste UDP-Datagramm hat 65507 Byte; der
/// Wert fasst den IPv4-Kopf mit und ist eine runde Obergrenze.
pub const MAX_DATAGRAMM: usize = 65536;

/// Hoechste Zahl Datagramme je Phase (M3). Ohne diese Schranke haengt die
/// Phase an einem Server, der endlos Pakete schickt.
pub const MAX_HANDSHAKE_DATAGRAMME: usize = 64;

/// Hoechste Zahl PTO je Phase (M3). Zusammen mit `ZEITBLOCK` ist das die
/// harte Obergrenze einer Phase: `MAX_PTO * ZEITBLOCK`.
pub const MAX_PTO: u32 = 32;

/// Dauer eines Lesevorgangs. Kurz genug, dass ein PTO greift, lang genug,
/// dass ein normales Datagramm nicht zweimal gelesen wird.
pub const ZEITBLOCK: Duration = Duration::from_millis(250);

/// Hoechste Groesse der CRYPTO-Reassembly (M4). Ein Server-Handshake mit
/// Kette ist wenige Kilobyte; mehr endet in einem benannten Fehler.
pub const MAX_HANDSHAKE_PUFFER: usize = 1 << 20; // 1 MiB

/// Hoechste Zahl gepufferter Pakete ohne passenden Schluessel (M3, Q6a).
pub const MAX_GEPUFFERTE_PAKETE: usize = 32;

/// Hoechste Groesse eines gepufferten Pakets (M4).
pub const MAX_GEPUFFERTES_PAKET: usize = 2048;

/// Kleinste Nutzlast, mit der ein Datagramm fuer uns **sinnvoll** ist.
///
/// Q3 misst das nicht, Q3 **leitet** es: ein Datagramm, das weniger als ein
/// KiB traegt, ist bei QUIC ungewoehnlich (die Initial-Pakete sind 1200 Byte,
/// weil der Mindestpfad sonst ist), und wir wollen die Datagrammzahl nicht
/// nach oben schrauben, nur weil ein Server uns 40-Byte-Pakete schickt.
pub const MIN_NUTZLAST: u64 = 1024;

/// Hoechste Zahl Datagramme fuer die Antwort (M3, Q3).
///
/// **DIE ZAHL WAR ZU KLEIN, und das ist gemessen.** Q3 verlangt die Abnahme
/// ueber fuenf Seiten; zwei davon lieferten
/// `Zeitueberschreitung: keine vollstaendige Antwort im Datagrammlimit`:
///
/// ```text
/// cloudflare.com   1 342 991 Byte   ~1 120 Datagramme (bei 1200 B)
/// youtube.com        891 249 Byte   ~  743 Datagramme
/// facebook.com        417 893 Byte   ~  349 Datagramme  (schaffte es)
/// wikipedia.org        93 955 Byte   ~   79 Datagramme
/// google.com           84 311 Byte   ~   71 Datagramme  (schaffte es)
/// ```
///
/// Bei 512 war Schluss nach rund 600 KiB — genau zwischen den zwei groessten
/// und den drei kleinen Seiten. Der Grenzwert war also nicht "knapp", er war
/// **falsch**: er hat zwei normale Webseiten abgeschnitten.
///
/// DIE NEUE ZAHL IST ABGELEITET, nicht geraten: `MAX_ANTWORT_BYTE / MIN_NUTZLAST`
/// = 8 MiB / 1 KiB = **8192**. Damit ist die Bytezgrenze die einzige, die
/// wirklich greift, und die Datagrammzahl beschreibt nur den Fall
/// "viele winzige Pakete". Ohne die Herleitung waere 8192 eine weitere
/// geratene Zahl — und das ist der Fehler, den ich bei `erste_range` und
/// beim Laenge-Feld zweimal gemacht habe.
pub const MAX_ANTWORT_DATAGRAMME: usize = MAX_ANTWORT_BYTE / MIN_NUTZLAST as usize;

/// Hoechste Antwortgroesse in Byte (M4, Q13).
pub const MAX_ANTWORT_BYTE: usize = 8 << 20; // 8 MiB

/// Hoechste Zahl gleichzeitig offener Streams (M3, Q13). Unser Client nutzt
/// zwei (Steuerstrom 2, Anfrage 0); die Grenze gilt fuer alles, was der
/// Server oeffnen will.
pub const MAX_OFFENE_STROEME: usize = 64;

/// Hoechste Zahl gemerkter Paketnummern je Ebene (M3). Mehr braucht ein ACK
/// nicht; die Bereiche werden danach gekappt.
pub const MAX_GEMERKTE_PAKETE: usize = 64;

/// Kleinstes Datagramm, das ein Client mit Initial-Paket schicken muss
/// (RFC 9000 Abschnitt 14.1).
pub const MIN_DATAGRAMM: usize = 1200;

/// Unser `max_idle_timeout` in Millisekunden (RFC 9000 Abschnitt 10.1: es
/// gilt das Minimum beider Richtungen).
pub const EIGENER_IDLE_MS: u64 = 30_000;

/// Kleinste Paketlaenge, ab der ein unentschluesselbares Datagramm ein
/// Stateless Reset sein kann: 16 Byte Token plus ein Byte Kopf.
pub const MIN_STATELESS_RESET: usize = 17;

/// Hoechste Zahl Trailer-Felder in einer Antwort (M3, U3-Rest).
///
/// Jede Zahl haengt an fremden Daten, also ist sie benannt und **geprueft**.
/// Der Wert haelt sich an `MAX_ANTWORT_BYTE`: mehr Trailer als ein kleiner
/// Anteil am maximalen Koerper sind Unsinn, und 1024 Felder sind fuer jede
/// reale Antwort weit mehr.
pub const MAX_TRAILER_FELDER: usize = 1024;

/// Hoechste Zahl H3-Frames in einer Antwort (M3).
const MAX_H3_FRAMES: usize = 4096;

/// Hoechste Zahl TLS-Nachrichten aus einem Server-Handshake (M3).
const MAX_TLS_NACHRICHTEN: usize = 16;

/// Gesamtzeitlimit, wenn `Optionen` keins setzt.
const ZEIT_GESAMT: Duration = Duration::from_secs(20);

// ---------------------------------------------------------------------------
// Antwort und Fehler
// ---------------------------------------------------------------------------

/// Die Antwort auf einen HTTP/3-Abruf.
#[derive(Debug, Clone)]
pub struct Antwort {
    pub status: u16,
    pub koepfe: Vec<FeldZeile>,
    pub groesse: usize,
    /// Die Cipher Suite des 1-RTT-Verkehrs.
    pub suite: Suite,
    /// Zahl verarbeiteter TLS-Handshake-Nachrichten des Servers.
    pub handshake_nachrichten: usize,
    /// `true`, wenn Kette, Hostname und `CertificateVerify` geprueft wurden.
    pub zertifikat_geprueft: bool,
    /// Byte der geprueften Zertifikatskette.
    pub kette_byte: usize,
    /// Der Hostname, gegen den das Zertifikat geprueft wurde.
    pub hostname: String,
    /// Der `Alt-Svc`-Eintrag der Antwort, **geparst** (U3).
    ///
    /// DER GRUND FUER EIN FELD STATT NUR ROH: der Aufrufer (narsil-net) soll
    /// nicht selber einen Headerwert zerlegen muessen, und schon gar nicht
    /// richtig — `alt_svc_lesen` hat die Grenzen (M3/M4) und die Form. Bei
    /// cloudflare-quic.com steht dort `h3=":443"; ma=86400`.
    ///
    /// `None`, wenn der Header fehlt **oder** unlesbar ist. Ein kaputter
    /// Alt-Svc darf eine Seite nicht zerstoeren, die sonst in Ordnung waere.
    pub alt_svc: Option<crate::alt_svc::AltSvc>,
    /// Die `Set-Cookie`-Werte der Antwort, **verlustfrei** (U3-Rest).
    ///
    /// DER GRUND: `narsil_net::Response` traegt `set_cookie`, meines `Antwort`
    /// bis hierher nicht. Ein HTTP/3-Abruf, der in `narsil-net` eingesetzt wird,
    /// verliert Cookies also **still**.
    ///
    /// `Vec<Vec<u8>>` und **nicht** `Vec<String>`: ein Cookie-Wert muss kein
    /// UTF-8 sein, und `from_utf8_lossy` macht aus einem fremden Cookie einen
    /// anderen, ohne zu melden, dass er eingerueckt wurde. Die Umwandlung
    /// gehoert an die Stelle, die weiss, was sie mit nicht-UTF-8 macht.
    ///
    /// Leer, wenn die Antwort keine schickt.
    pub set_cookie: Vec<Vec<u8>>,
    /// Die **Trailer** der Antwort (U3-Rest), als Feldzeilen dekodiert.
    ///
    /// HTTP/3 kennt kein eigenes Trailer-Rahmen: Trailer kommen als ein
    /// **zweiter HEADERS-Rahmen nach den DATA-Rahmen** (RFC 9114 4.1). Die
    /// Position entscheidet, und `koepfe` traegt weiterhin die **Antwort**-
    /// Kopfzeilen.
    ///
    /// Leer, wenn der Server keine schickt.
    pub trailers: Vec<crate::qpack::FeldZeile>,
    /// Die SETTINGS des Servers (RFC 9114 7.2.4), soweit gelesen (U3).
    ///
    /// Leer, wenn der Server keine schickte. Nur die **unbekannten** Paare
    /// sind fuer den Browser interessant (`0x06` = maximale Kopfzeilengroesse
    /// etwa); die bekannten sind hiermit bereits ausgewertet.
    pub settings: Vec<(u64, u64)>,
}

// ---------------------------------------------------------------------------
// Fehlertexte fuer die Oberflaeche (U3, 01.10.2026, fuer UI3)
// ---------------------------------------------------------------------------

/// Die grobe Art des Fehlers, ohne den Text.
///
/// DER GRUND FUER EINE EIGENE ENUM: `narsil` muss entscheiden, **welche Seite**
/// es zeigt (kein Netz / Sicherheit / Server / zu gross), ohne den deutschen
/// Text zu zerlegen. Eine Kategorie ist maschinenlesbar, ein Satz nicht.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FehlerArt {
    /// Die Adresse war schon falsch (kein `https://`, Port 0, Host zu lang).
    Adresse,
    /// UDP kam nicht durch, oder DNS ging nicht.
    Netz,
    /// Die TLS-/QUIC-Verbindung kam nicht zustande.
    Verbindung,
    /// Zertifikat, Hostname oder CertificateVerify stimmen nicht.
    Sicherheit,
    /// Der Server hat die Verbindung geordnet geschlossen.
    Server,
    /// Die Antwort war nicht rechtzeitig oder nicht vollstaendig da.
    Zeit,
    /// Die Antwort war groesser, als wir speichern wollen.
    ZuGross,
    /// HTTP/3 selbst (Frames, QPACK, H3).
    Protokoll,
}

/// Was die Oberflaeche anzeigen soll: eine Kategorie, eine kurze Ueberschrift
/// und einen Satz, der **nichts** verschweigt und nichts erfindet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FehlerText {
    pub art: FehlerArt,
    /// Kurz und ohne Fachwort, fuer die Ueberschrift.
    pub titel: &'static str,
    /// Ein Satz. Enthaelt **keine** ungepruefte Behauptung ueber die Ursache.
    pub satz: String,
}

/// Formt aus einem `ClientFehler` eine Anzeige.
///
/// ## DIE REGEL, DIE DARAUS EINEN TEST GEMACHT HAT
///
/// Kein Satz darf etwas behaupten, was der Fehler **nicht** weiss. Der
/// haeufigste Fehler hier ist "die Verbindung wurde zurueckgesetzt" — das
/// weiss der Client nicht, wenn der Fehler `Zeitueberschreitung` heisst. Der
/// alte Text dafuer war "Der Server hat die Verbindung getrennt", und das ist
/// eine **Erfindung**.
///
/// Also: `StatelessReset` darf "zurueckgesetzt" sagen (das steht im
/// RFC 9000 10.3), `Zeitueberschreitung` darf es nicht.
///
/// ## WARUM DAS IN `narsil-quic` STEHT
///
/// Die Varianten sind hier, und die Zuordnung "Variante -> was ein Mensch
/// sehen soll" gehoert zu ihnen. In `narsil` waere sie eine zweite Liste, die
/// auseinanderlaufen kann — und zwar genau dann, wenn jemand eine Variante
/// ergaenzt und die zweite Liste nicht kennt.
pub fn fehler_text(f: &ClientFehler) -> FehlerText {
    let (art, titel) = match f {
        ClientFehler::Url(_) => (FehlerArt::Adresse, "Adresse ungültig"),
        ClientFehler::Netz(_) => (FehlerArt::Netz, "Kein Netz"),
        ClientFehler::Handshake(_) => (FehlerArt::Verbindung, "Verbindung nicht möglich"),
        ClientFehler::Zertifikat(_) => (FehlerArt::Sicherheit, "Sicherheitsproblem"),
        ClientFehler::StatelessReset => (FehlerArt::Verbindung, "Verbindung zurückgesetzt"),
        ClientFehler::AntwortNichtLesbar { .. } => {
            (FehlerArt::Server, "Antwort des Servers unvollständig")
        }
        ClientFehler::Versionsverhandlung { .. } => {
            (FehlerArt::Verbindung, "Server kann keine HTTP/3-Version")
        }
        ClientFehler::Geschlossen { .. } => (FehlerArt::Server, "Server hat geschlossen"),
        ClientFehler::Zeitueberschreitung(_) => (FehlerArt::Zeit, "Keine Antwort"),
        ClientFehler::ZuVieleStroeme { .. } | ClientFehler::PufferVoll { .. } => {
            (FehlerArt::ZuGross, "Seite zu groß")
        }
        ClientFehler::H3(_) => (FehlerArt::Protokoll, "HTTP/3-Fehler"),
    };
    // DIE SAETZE SIND EINE ZEILE JE, und nicht ueber `\` umbrochen.
    //
    // DER GRUND, und er ist gemessen: `\` am Zeilenende entfernt in Rust nur
    // den **Zeilenumbruch**, nicht die einrueckenden Leerzeichen der
    // Folgezeile. `cargo fmt` hat die Umbrueche beim ersten Entwurf zudem
    // aufgeloest und die Leerzeichen in den String **geschrieben**. Das
    // Ergebnis war in der Anzeige "Es kam keine vollstaendige              Antwort."
    // — eine sichtbare Luecke mitten im Satz. Umbrochene Texte in
    // Ausgabestrings sehen in der Quelle ordentlich aus und in der Anzeige
    // kaputt; `concat!` ist hier die Form, die beide verträgt.
    let satz = match f {
        ClientFehler::Url(_) => concat!(
            "Die Adresse kann nicht gelesen werden. ",
            "Erwartet wird https://host/pfad."
        )
        .into(),
        ClientFehler::Netz(_) => concat!(
            "Der Host ist nicht erreichbar. ",
            "Prüfen Sie die Internetverbindung."
        )
        .into(),
        ClientFehler::Handshake(_) => concat!(
            "Die sichere Verbindung ist nicht zustande gekommen. ",
            "Auf diesem Weg spricht niemand HTTP/3."
        )
        .into(),
        // DIE ZEILE, DEREN WORTE IMMER WIEDER GERETTET WERDEN MUSSEN:
        // "nichts geladen" ist der Unterschied zwischen "die Seite kam nicht"
        // und "die Seite kam und ist nicht die, die du wolltest".
        ClientFehler::Zertifikat(_) => concat!(
            "Das Zertifikat passt nicht zu dieser Adresse oder wird nicht ",
            "vertraut. Es wurde nichts geladen."
        )
        .into(),
        // Hier darf "zurückgesetzt" stehen: es steht im RFC 9000 10.3.
        ClientFehler::StatelessReset => {
            // EINZEILIG, deshalb kein `concat!` (clippy sagt es zu Recht).
            "Die Verbindung wurde ohne Vorwarnung zurückgesetzt.".into()
        }
        ClientFehler::AntwortNichtLesbar { grund } => {
            // DER NAME ENTSCHEIDET, ob der Aufrufer zurueckfaellt: eine
            // unvollstaendige Antwort holt man nicht erneut, sondern meldet sie.
            format!(
                "Die Antwort des Servers war unvollständig und liess sich nicht zu Ende lesen. {grund}"
            )
        }
        ClientFehler::Versionsverhandlung { angeboten } => {
            // Die Versionen **nennen**: "geht nicht" ist keine Meldung, mit der
            // jemand etwas anfangen kann. Der Server sagt uns, was er kann —
            // das ist das einzige, was hier nuetzt.
            format!(
                "Der Server kann keine gemeinsame QUIC-Version. Er bietet: {} (wir sprechen                  Version {:x}).",
                liste_versionen(angeboten),
                crate::kopf::QUIC_VERSION_1
            )
        }
        ClientFehler::Geschlossen { code, .. } => {
            format!("Die Verbindung wurde geordnet geschlossen (Code {code}).")
        }
        // Hier darf es **nicht** stehen, und der Test
        // `zeitueberschreitung_behauptet_keine_ursache` verbietet jedes Wort,
        // das eine Partei nennt. Nicht weil die Nennung falsch waere —
        // sondern weil ein Satz mit "Server" darin in einem Screenshot als
        // Schuldzuweisung gelesen wird, auch wenn er nur eine Moeglichkeit
        // nennt. Der erste Entwurf sagte "Ob der Server still ist ..." und
        // der Test hat ihn zu Recht gefangen.
        ClientFehler::Zeitueberschreitung(_) => concat!(
            "Es kam keine vollständige Antwort. ",
            "Die Ursache lässt sich von hier aus nicht unterscheiden: ",
            "es kann geschwiegen worden sein, ",
            "und die Leitung kann zu langsam sein."
        )
        .into(),
        ClientFehler::ZuVieleStroeme { grenze } => {
            format!("Es waren mehr als {grenze} Ströme gleichzeitig offen.")
        }
        ClientFehler::PufferVoll { grenze } => {
            format!("Die Antwort ist größer als {grenze} Byte.")
        }
        ClientFehler::H3(_) => "Die Antwort war kein gültiges HTTP/3.".into(),
    };
    FehlerText { art, titel, satz }
}

/// Wohin der Aufrufer die **naechste** Anfrage schicken soll, wenn er
/// HTTP/3 gelernt hat (U3).
///
/// DAS IST DIE GANZE ANBINDUNG, DIE `narsil-quic` liefern kann. Sie ist eine
/// Funktion und kein Zustand, damit jeder Aufrufer (narsil-net, narsil) seine
/// eigene Speicherung behalten kann — ein gemeinsamer Merker in dieser Kiste
/// waere ein zweiter globaler Zustand neben dem Socket, und den darf es nicht
/// geben.
///
/// `None`, wenn die Antwort kein `alt-svc` mit `h3` brachte: dann bleibt es
/// bei HTTP/1.1 oder HTTP/2, und das ist ein gueltiges Ergebnis.
///
/// DER GRUND FUER DEN RUECKGABEWERT STATT EINES `bool`: der Aufrufer braucht
/// den **Port**. `h3=":443"` bedeutet Port 443, `h3=":8443"` einen anderen,
/// und ein Browser, der 443 hart verdrahtet, kann einen Server mit einem
/// anderen Port nie erreichen.
pub fn h3_ziel(antwort: &Antwort) -> Option<(String, u16)> {
    let a = antwort.alt_svc.as_ref()?;
    // Port 0 heisst "der Header nannte keinen" (siehe `H3_STD_PORT`), und dann
    // gilt der Standardport. HIER wird die Regel angewandt, nicht im Parser —
    // sonst koennte der Aufrufer nicht mehr unterscheiden, was der Server
    // geschrieben hat.
    let port = if a.port == 0 {
        crate::alt_svc::H3_STD_PORT
    } else {
        a.port
    };
    Some(if a.host.is_empty() {
        (antwort.hostname.clone(), port)
    } else {
        (a.host.clone(), port)
    })
}

/// Warum der Abruf scheitert. Jede Variante ist benannt; kein Panic, kein
/// `unwrap` auf fremder Eingabe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientFehler {
    Url(String),
    Netz(String),
    Handshake(String),
    H3(String),
    /// Die Antwort liess sich nicht zu Ende lesen (N48).
    ///
    /// EIGENE FEHLERART, weil sie **kein Netzfehler** ist: das Netz war gut, der
    /// Server hat geliefert, und **wir** kommen mit dem Strom nicht zu Ende.
    /// Als `Zeitueberschreitung` ausgegeben heisst es "keine Antwort" — das ist
    /// falsch, und ein falscher Fehlername kostet Stunden.
    AntwortNichtLesbar {
        grund: String,
    },
    /// Der Gegenuefer kann unsere QUIC-Version nicht (RFC 9000 6.2).
    ///
    /// EIGENE FEHLERART, weil die Alternative ein **Timeout** war: ohne
    /// Erkennung fiel das Paket in `zerlegen`, wurde als Initial eingeordnet,
    /// scheiterte an der Entschluesselung (ein VN-Paket ist nicht
    /// verschluesselt) — und nach 2 bis 3 Sekunden kam `keine Antwort nach dem
    /// Idle-Zeitlimit`. Man haelt den Server damit fuer stumm, dabei hat er
    /// sich **offiziell** fuer die Version entschieden. Die Liste steht im
    /// Fehler, damit man sieht, **was** er kann.
    Versionsverhandlung {
        angeboten: Vec<u32>,
    },
    /// Der Gegenuefer hat die Verbindung ohne Warnung abgerissen
    /// (RFC 9000 Abschnitt 10.3).
    StatelessReset,
    /// In der Frist kam kein Byte (RFC 9000 Abschnitt 10.1).
    Zeitueberschreitung(String),
    /// Zu viele gleichzeitig offene Streams (Q13).
    ZuVieleStroeme {
        grenze: usize,
    },
    /// Zu viele gepufferte Byte in einer Verbindung (Q13).
    PufferVoll {
        grenze: usize,
    },
    /// Zertifikatskette, Hostname oder `CertificateVerify` stimmen nicht.
    Zertifikat(String),
    /// Der Gegenuefer hat die Verbindung geordnet geschlossen.
    Geschlossen {
        code: u64,
        grund: String,
    },
}

/// N50: Der TLS-Alert aus einem QUIC-Fehlercode (RFC 9001 4.8).
///
/// **Ein** Satz Norm: „A TLS alert is converted into a QUIC connection error.
/// The AlertDescription value is added to 0x0100 to produce a QUIC error code
/// from the range reserved for CRYPTO_ERROR."
///
/// Die Namen sind aus RFC 8446 Abschnitt 6.2, **mit Code** gelesen und nicht
/// aus dem Gedaechtnis befuellt — dieselbe Regel, die ich in N49 gebrochen
/// habe (zwei Zahlen verwechselt und drei Messungen lang die Fehlermeldung des
/// Servers fuer eine Ursache gehalten).
///
/// M3: `code` kommt aus fremden Daten (aus einem Paket). Der Bereichscheck
/// ist deshalb **vor** der Subtraktion, und die Liste hat ein Ende — kein
/// `unwrap`, kein Index.
pub fn tls_alert_aus_quic_code(code: u64) -> Option<&'static str> {
    if !(0x100..=0x1ff).contains(&code) {
        return None;
    }
    Some(match code - 0x100 {
        0 => "close_notify",
        10 => "unexpected_message",
        20 => "bad_record_mac",
        22 => "record_overflow",
        40 => "handshake_failure",
        42 => "bad_certificate",
        43 => "unsupported_certificate",
        44 => "certificate_revoked",
        45 => "certificate_expired",
        46 => "certificate_unknown",
        47 => "illegal_parameter",
        48 => "unknown_ca",
        49 => "access_denied",
        50 => "decode_error",
        51 => "decrypt_error",
        70 => "protocol_version",
        71 => "insufficient_security",
        80 => "internal_error",
        86 => "inappropriate_fallback",
        90 => "user_canceled",
        109 => "missing_extension",
        110 => "unsupported_extension",
        112 => "unrecognized_name",
        113 => "bad_certificate_status_error",
        // M3: Ein Alert, den die Norm nicht kennt, wird **nicht** geraten.
        _ => return None,
    })
}

impl std::fmt::Display for ClientFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientFehler::Url(s) => write!(f, "URL: {s}"),
            ClientFehler::Netz(s) => write!(f, "Netz: {s}"),
            ClientFehler::Handshake(s) => write!(f, "Handshake: {s}"),
            ClientFehler::H3(s) => write!(f, "HTTP/3: {s}"),
            ClientFehler::AntwortNichtLesbar { grund } => {
                write!(f, "Antwort nicht lesbar: {grund}")
            }
            ClientFehler::Versionsverhandlung { angeboten } => write!(
                f,
                "Versionsverhandlung (RFC 9000 6.2): angeboten {}",
                liste_versionen(angeboten)
            ),
            ClientFehler::StatelessReset => write!(
                f,
                "der Server hat die Verbindung per Stateless Reset abgerissen (RFC 9000 10.3)"
            ),
            ClientFehler::Zeitueberschreitung(s) => write!(f, "Zeitueberschreitung: {s}"),
            ClientFehler::ZuVieleStroeme { grenze } => {
                write!(f, "mehr als {grenze} offene Streams")
            }
            ClientFehler::PufferVoll { grenze } => {
                write!(f, "mehr als {grenze} gepufferte Byte")
            }
            ClientFehler::Zertifikat(s) => write!(f, "Zertifikat: {s}"),
            ClientFehler::Geschlossen { code, grund } => {
                // N50: DER ALERT-NAME, und er ist der Grund dieser Aenderung.
                //
                // GEMESSEN an `example.org` (02.10.2026 ~20:10): der Server
                // schliesst mit Code **296**, Grund leer. In der alten Ausgabe
                // stand dort genau das: „Server schliesst (Code 296): " und
                // dann nichts.
                //
                // Zwei Dinge fehlten beide:
                //
                //  1. **Die Deutung.** RFC 9001 4.8: „A TLS alert is converted
                //     into a QUIC connection error. The AlertDescription value
                //     is added to 0x0100." Also 296 − 0x100 = **40**, und
                //     Alert 40 ist `handshake_failure` (RFC 8446 6.2). Wer das
                //     nicht weiss, sieht eine Zahl und muss die Subtraktion
                //     selbst machen — **bei jedem** solchen Fehler.
                //  2. **Der Hinweis, dass der Grund fehlt.** Ein leerer Grund
                //     sieht wie ein Parserfehler aus. Ist er aber nicht: der
                //     Server schickt keinen. Das zu unterscheiden ist der
                //     Unterschied zwischen „unser Grundtext-Parser ist kaputt"
                //     und „der Server sagt nichts" — und das war der halbe
                //     Aufwand von N50.
                //
                // DER PREIS: der Text wird laenger. Das ist gewollt; ein
                // Fehlertext, den man nicht lesen kann, spart keine Zeit.
                match tls_alert_aus_quic_code(*code) {
                    Some(alert) => write!(
                        f,
                        "Server schliesst (Code {code} = TLS-Alert {} `{alert}`): {}",
                        code - 0x100,
                        grund.as_str()
                    ),
                    None => write!(f, "Server schliesst (Code {code}): {grund}"),
                }
            }
        }
    }
}

impl std::error::Error for ClientFehler {}

impl From<crate::schutz::SchutzFehler> for ClientFehler {
    fn from(e: crate::schutz::SchutzFehler) -> Self {
        ClientFehler::Handshake(e.to_string())
    }
}

impl From<crate::varint::VarintFehler> for ClientFehler {
    fn from(e: crate::varint::VarintFehler) -> Self {
        ClientFehler::Handshake(e.to_string())
    }
}

impl From<crate::h3::H3Fehler> for ClientFehler {
    fn from(e: crate::h3::H3Fehler) -> Self {
        ClientFehler::H3(e.to_string())
    }
}

impl From<crate::qpack::QpackFehler> for ClientFehler {
    fn from(e: crate::qpack::QpackFehler) -> Self {
        ClientFehler::H3(e.to_string())
    }
}

impl From<crate::kopf::KopfFehler> for ClientFehler {
    fn from(e: crate::kopf::KopfFehler) -> Self {
        ClientFehler::Handshake(e.to_string())
    }
}

impl From<crate::rahmen::RahmenFehler> for ClientFehler {
    fn from(e: crate::rahmen::RahmenFehler) -> Self {
        ClientFehler::Handshake(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Optionen
// ---------------------------------------------------------------------------

/// Wie `hole_mit` arbeiten soll.
#[derive(Debug, Clone, Default)]
pub struct Optionen {
    /// **Ohne** Vertrauensspeicher wird die Kette nicht geprueft — und der
    /// Auftrag sagt ausdruecklich, dass HTTP/3 ohne diese Pruefung nicht
    /// gesichert ist. Deshalb ist es ein Wert und kein Default: `hole()`
    /// benutzt `Optionen::neu()`, und das liefert einen Speicher aus dem
    /// mitgelieferten Wurzelzertifikat-Bundle.
    pub speicher: Option<narsil_tls::kette::Vertrauensspeicher>,
    /// Gesamtes Zeitlimit fuer den Abruf (Netz **und** Protokoll).
    pub gesamt_zeitlimit: Option<Duration>,
}

impl Optionen {
    /// Mit dem mitgelieferten CA-Bundle und 20 s Gesamtzeitlimit.
    ///
    /// Das Bundle liegt als Datei in `testdaten/` und wird per
    /// `include_str!` eingebunden — ein Browser darf seine Wurzelzertifikate
    /// nicht zur Laufzeit von einer Platte lesen muessen.
    pub fn neu() -> Self {
        Optionen {
            speicher: Some(wurzelzertifikate()),
            gesamt_zeitlimit: Some(Duration::from_secs(20)),
        }
    }

    /// Ohne jede Pruefung. Nur fuer Tests, die den Fehlerfall `Zertifikat`
    /// ueberhaupt erst einmal sehen wollen.
    pub fn ohne_pruefung() -> Self {
        Optionen {
            speicher: None,
            gesamt_zeitlimit: Some(Duration::from_secs(20)),
        }
    }
}

/// Die Wurzelzertifikate, mit denen die Kette geprueft wird (Q12).
///
/// DER ORT IST ABSICHTLICH EINE DATEI: die PEM-Datei liegt unter
/// `testdaten/wurzeln.pem` und wird beim Bauen in die Binaerdatei uebernommen.
/// Wer einen anderen Satz will, benutzt `Optionen::speicher`.
pub fn wurzelzertifikate() -> narsil_tls::kette::Vertrauensspeicher {
    // M4: Die Datei ist Teil des Builds und damit keine Eingabe. Der Fehler
    // waere trotzdem ein Programmierfehler, also gibt es hier kein `Result`
    // nach aussen — `aus_pem` verlangt nur nicht-leere PEM-Abschnitte, und
    // leere Abschnitte werden uebersprungen.
    narsil_tls::kette::Vertrauensspeicher::aus_pem(include_str!("../testdaten/wurzeln.pem"))
        .unwrap_or_default()
}

/// Laedt einen Vertrauensspeicher aus PEM-Text (fuer eigene CA-Bundles).
pub fn speicher_aus_pem(pem: &str) -> Result<narsil_tls::kette::Vertrauensspeicher, String> {
    narsil_tls::kette::Vertrauensspeicher::aus_pem(pem).map_err(|e| e.to_string())
}

/// Wie `hole`, aber mit dem mitgelieferten CA-Bundle.
pub fn hole(url: &str) -> Result<Antwort, ClientFehler> {
    hole_mit(url, &Optionen::neu())
}

/// Wie `hole`, aber mit eigenen Optionen.
pub fn hole_mit(url: &str, opt: &Optionen) -> Result<Antwort, ClientFehler> {
    Sitzung::neu(url, opt)?.lauf()
}

/// Wie `hole_mit`, aber **mit dem Inhalt** (U3, 01.10.2026).
///
/// ## WARUM ES DAS GIBT
///
/// `Antwort.groesse` ist die Zahl der DATA-Bytes, nicht der Inhalt. Das war
/// eine bewusste Entscheidung: eine 8-MiB-Seite soll nicht in einem Feld
/// stecken, das man aus Versehen kopiert. Aber damit kann ein Aufrufer, der
/// die Seite **anzeigen** will, nichts anfangen — und `narsil` ist genau so ein
/// Aufrufer. Die Anbindung dafuer steht als Wunsch W7 in
/// `team/WUNSCH_QUIC.txt`.
///
/// ## WAS GENAU ZURUECKKOMMT
///
/// `(Antwort, Vec<u8>)` — die Inhaltsbytes der DATA-Frames **in der
/// Reihenfolge des Antwortstroms**, also genau das, was der Server geschickt
/// hat. Also **nicht** entpackt: `content-encoding` (Brotli, gzip) wird
/// nicht ausgepackt, weil das HTTP/3 nicht ist, sondern HTTP/1.1-Semantik
/// (RFC 9110 8.4). Das Entpacken gehoert in `narsil-net`, wo der 1.1-Dekoder
/// schon liegt.
///
/// ## DIE GRENZE
///
/// `MAX_ANTWORT_BYTE` (8 MiB) gilt fuer die Byte auf dem Draht **und** fuer
/// die hier zurueckgegebenen. Das ist eine Zusage, keine Ueberraschung: ein
/// Body, der nicht passt, ist `ClientFehler::PufferVoll` mit der Zahl, statt
/// dass der Prozess still ein Memory-Problem bekommt.
/// Welcher Weg fuer einen Abruf benutzt wurde (Q2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weg {
    /// HTTP/3 ueber QUIC.
    Http3,
    /// Kein HTTP/3: der Aufrufer soll es ueber TLS/TCP versuchen.
    Tcp,
}

/// Q2: **ein Abruf, der sich am Alt-Svc merkt** — und bei Fehler **sofort**
/// zurueckfaellt.
///
/// DIE DREI TEILE, und alle drei sind Forderungen aus Q2:
///
/// 1. **Merken.** Steht im Antwortkopf ein `alt-svc` mit `h3`, wird es fuer
///    diesen Ursprung gespeichert (`alt_svc_speicher`).
/// 2. **Versuchen.** Steht fuer den Ursprung ein gültiger Eintrag, wird
///    HTTP/3 versucht — mit einer **eigenen** Frist (`frist`, Standard 2 s),
///    damit ein hängendes h3 den Abruf nicht aufhält.
/// 3. **Zurück.** Schlägt h3 fehl, wird der Eintrag **vergessen** und
///    `(Tcp, Err)` geliefert. Der Aufrufer holt dieselbe URL über TLS/TCP und
///    bekommt sie — ohne dass er etwas entscheiden muss.
///
/// WARUM NICHT EINFACH SELBST ZURUECKFALLEN: TLS/TCP ist `narsil-net`s Weg, und
/// `narsil-net` gehoert einer anderen Rolle (Wunsch W7/N34). Diese Funktion
/// liefert deshalb die **Entscheidung** und nicht den zweiten Abruf: sie ist
/// additiv und braucht keine Schreibzugriffe auf fremden Code.
///
/// DER FEHLER WIRD NICHT VERSCHWUNDEN: `result` traegt ihn, damit der Aufrufer
/// ihn im Bericht nennen kann. Wer den Grund versteckt, kann „der Browser ist
/// langsam" nicht von „HTTP/3 ist kaputt" unterscheiden.
pub fn hole_mit_weg(
    url: &str,
    opt: &Optionen,
    speicher: &mut crate::alt_svc_speicher::AltSvcSpeicher,
    frist: std::time::Duration,
) -> (Weg, Result<Antwort, ClientFehler>) {
    let jetzt = Instant::now();
    let ursprung = ursprung_aus_url(url);
    // 2) Versuchen — nur wenn etwas gilt.
    if speicher.holen(&ursprung, jetzt).is_none() {
        return (
            Weg::Tcp,
            Err(ClientFehler::H3("kein alt-svc fuer diesen Ursprung".into())),
        );
    }
    // Die Frist **nur** fuer diesen Versuch. Das Gesamtlimit des Aufrufers
    // bleibt unberuehrt: Q2 will einen schnellen Rueckfall, nicht einen
    // schnellen Browser.
    let mut kurz = opt.clone();
    kurz.gesamt_zeitlimit = Some(frist);
    let ergebnis = hole_mit(url, &kurz);
    match ergebnis {
        Ok(a) => {
            // 1) Merken — was der Server **jetzt** sagt, ist juenger als alles
            // Gemerkte. Auch `ma=0` wird durchgereicht: das ist das Loeschen.
            if let Some(v) = a.alt_svc.clone() {
                let _ = speicher.setzen(&ursprung, v, Instant::now());
            }
            (Weg::Http3, Ok(a))
        }
        Err(e) => {
            // 3) Zurueck: Eintrag vergessen, Fehler mitgeben.
            let geloescht = speicher.vergessen(&ursprung);
            trace(
                "alt-svc",
                &format!(
                    "HTTP/3 gescheitert ({e}), Eintrag {}geloescht — der Aufrufer faellt auf TLS/TCP zurueck",
                    if geloescht { "" } else { "nicht " }
                ),
            );
            (Weg::Tcp, Err(e))
        }
    }
}

/// Der Ursprung aus einer URL: `scheme://host:port`.
///
/// OHNE Pfad, ohne Query, ohne `user:pass` — weil `alt-svc` fuer den
/// **Ursprung** gilt (RFC 7838 2.1) und nicht fuer eine Ressource. Und
/// kleingeschrieben: `HTTPS://Example.COM/` und `https://example.com/` sind
/// derselbe Ursprung, und ein Speicher, der beide kennt, hat zwei Eintraege
/// fuer eine Sache.
pub fn ursprung_aus_url(url: &str) -> String {
    let (scheme, ohne_scheme) = match url.find("://") {
        Some(i) => (&url[..i], &url[i + 3..]),
        None => ("https", url),
    };
    let host_teil = ohne_scheme
        .find(['/', '?', '#'])
        .map_or(ohne_scheme, |i| &ohne_scheme[..i]);
    let ohne_creds = match host_teil.rfind('@') {
        Some(i) => &host_teil[i + 1..],
        None => host_teil,
    };
    // Ein `format!` statt `+`: die erste Fassung mischte `String` und `&str`
    // mit einem Plus, und der Compiler nennt das nicht schoen.
    format!("{}://{}", scheme.to_lowercase(), ohne_creds.to_lowercase())
}

pub fn hole_mit_roh(url: &str, opt: &Optionen) -> Result<(Antwort, Vec<u8>), ClientFehler> {
    let antwort = Sitzung::neu(url, opt)?.lauf_mit_inhalt()?;
    let inhalt = antwort.1;
    Ok((antwort.0, inhalt))
}

// ---------------------------------------------------------------------------
// Die Sitzung
// ---------------------------------------------------------------------------

/// Eine Verbindung bis zum fertigen Abruf. Kein Thread, kein Timer: die
/// Zeit wird an den Aufrufer weitergegeben, die Zeitlimits sind `Instant`.
pub struct Sitzung {
    socket: UdpSocket,
    /// Die Adresse, an die wir **senden** (N43).
    ///
    /// DAS SOCKEL IST **NICHT** `connect`t — und das ist der ganze Punkt.
    /// Ein `connect`ter UDP-Socket nimmt nach RFC 1122 nur Datagramme von der
    /// verbundenen Adresse an; alles andere verwirft der **Kernel**, und unser
    /// Code sieht es nie: kein Trace, kein Fehler, kein Timeout. QUIC dagegen
    /// verlangt genau das Gegenteil — ein Server darf seine Adresse wechseln
    /// (RFC 9000 8.1 und 9.2), und der Client muss solche Pakete **sehen**
    /// koennen, um die Connection-ID-Pruefung zu entscheiden.
    ///
    /// GEMESSEN, warum das hier auffaellt (N42): gegen www.google.com kommen
    /// von 13 erwarteten 1-RTT-Paketen **8 Datagramme** an, ohne eine einzige
    /// Entschlüsselungsfehlermeldung — die fehlenden sind fuer unseren Code
    /// nicht von fehlenden zu unterscheiden. Gegen cloudflare-quic.com
    /// antwortet alles, weil dort die Quelladresse gleich bleibt.
    ziel: std::net::SocketAddr,
    /// Q7: Nanosekunden je Phase. Ohne diese Rechnung ist "h3 ist langsam"
    /// eine Behauptung — die Zahl, die man braucht, ist die Frage, **woran**.
    /// Gezaehlt wird nur, wenn `NARSIL_QUIC_TRACE` gesetzt ist.
    zeit_entschluesseln: u128,
    zeit_rahmen: u128,
    zeit_h3: u128,
    zeit_kopieren: u128,
    /// Q7: Frame-Schleife und Stream-Verarbeitung — der Rest nach der ersten
    /// Messung.
    zeit_stream: u128,
    /// Q7.2, DIE WARTEZEIT — und warum sie ein eigenes Feld ist.
    ///
    /// DER GRUND: „davon Netz 173 ms" heisst **nichts**, solange man nicht
    /// weiss, ob das Warten auf Pakete ist oder das Rechnen. Q7.1 hat genau
    /// diese Zahl nicht beantworten koennen, und die naheliegende Deutung
    /// („unser Code ist langsam") war damit eine Vermutung.
    ///
    /// M3: `u128` statt `Duration`, weil die Summe in Nanosekunden aufgeht und
    /// weil `saturating_add` bei einer fremden Paketmenge nicht ueberlaufen
    /// darf.
    wartezeit_gesamt: u128,
    /// Wie oft **ein** Paket ohne Lesen zurueckkam (Timeout = leerer Sockel).
    /// Getrennt von der Zeit, weil ein Timeout kein Warten auf **Daten** ist,
    /// sondern ein Warten auf **nichts**.
    wartezeit_echt: u32,
    /// Die groesste Einzelwartezeit — die Zahl, die sagt, ob ein Zeitlimit an
    /// **einer** Stelle klemmt oder ueberall gleichmaessig.
    wartezeit_max: u128,
    /// Q7.2: Die **letzte** Wartezeit, und das ist mehr wert als der Durchschnitt.
    ///
    /// DER GRUND: Die Restgroesse „sonstiges" (konstant 55 bis 62 ms) liess sich
    /// mit Summe und Maximum nicht aufteilen. Wer sie nicht benennen kann,
    /// schiebt sie auf „Syscalls" — und das ist eine Vermutung mit Zahl.
    ///
    /// GEMESSEN: die letzte Wartezeit ist **0 bis 1 ms**, wenn das letzte Paket
    /// sofort da war. Die Restgroesse sitzt also **nicht** in den Wartezeiten.
    wartezeit_letzte: u128,
    /// Q7.2: Die **Sendezeit** — und das ist die_phase, die in Q7.1 gar nicht
    /// existierte.
    ///
    /// DER GRUND: Die Verteilung hatte fuenf Phasen (`entschluesseln`, `rahmen`,
    /// `h3`, `kopieren`, `stream`) und **keine** fuer das Senden. Es gibt aber
    /// neun Aufrufstellen von `sende`, und `send_to` ist ein Syscall, der bei
    /// einem gefuellten Sende-Puffer **blockiert**.
    ///
    /// DIE RESTGROSSE, 55 bis 62 ms, konstant ueber beide Bauarten und ueber
    /// die Laufzeit — das ist das Bild von etwas, das **nicht** vom Netz kommt.
    /// `Cell`, weil `sende(&self, …)` keinen `&mut` hat und trotzdem messen
    /// soll. `Cell` ist dafuer gedacht; `RefCell` waere hier ueberzogen.
    sendezeit: std::cell::Cell<u128>,
    /// Wie oft gesendet wurde. Ohne diese Zahl ist `sendezeit` eine Summe ohne
    /// Bezug.
    sendungen: std::cell::Cell<u32>,
    pakete_entschluesselt: u32,
    pakete_gelesen: u32,
    /// Wann der Abruf begann (Q7).
    t_gesamt: Option<std::time::Instant>,
    /// Wie oft ein Datagramm von einer **anderen** Quelle kam (N43, M7).
    /// Gezaehlt wird ueber `recv_from`, weil es im Trace sonst niemand sieht.
    quelle_gewechselt: u32,
    host: String,
    pfad: String,
    opt: Optionen,

    /// Unsere Source Connection ID; steht in `initial_source_connection_id`.
    scid: Vec<u8>,
    /// Die Destination Connection ID. Ab dem ServerHello ist das die SCID
    /// des Servers (RFC 9000 Abschnitt 7.2).
    dcid: Vec<u8>,
    /// Die DCID des allerersten Initial — nur fuer die Retry-Tag-Pruefung.
    urspruengliche_dcid: Vec<u8>,
    /// Das X25519-Geheimnis des Clients.
    geheim: [u8; 32],

    clienthello: Vec<u8>,
    plan: Schluesselplan,
    serverhello: Vec<u8>,

    initial_client: PaketSchluessel,
    initial_server: PaketSchluessel,
    handshake_client: Option<PaketSchluessel>,
    handshake_server: Option<PaketSchluessel>,
    eins_client: Option<PaketSchluessel>,
    eins_server: Option<PaketSchluessel>,

    /// Naechste zu sendende Paketnummer je Ebene (RFC 9000 12.3: nie
    /// wiederverwenden).
    naechste_pn: [u64; 3],

    crypto_initial: CryptoReassembly,
    crypto_handshake: CryptoReassembly,
    server_finished: bool,

    /// Unser eigenes Client-Finished, zum **Wiederholen** (N28).
    ///
    /// WARUM ES HIER LIEGT, und nicht im Moment des Sendens weggeworfen wird:
    /// RFC 9000 Abschnitt 19.3 verlangt Verlustwiederherstellung **je
    /// Verschluesselungsebene**. Bisher gab es sie nur fuer 1-RTT — bei jedem
    /// PTO wurde die Anfrage wiederholt, unser Client-Finished genau **einmal**
    /// geschickt.
    ///
    /// Das ist an einem echten Server aufgefallen: google und nginx wiederholen
    /// ihre Handshake-CRYPTO 7- bzw. 16mal, wir senden in derselben Zeit
    /// 1-RTT-Pakete bis pn 68 und niemals ein zweites Finished. Wenn das
    /// Finished verloren geht oder nicht akzeptiert wird, gibt es auf unserer
    /// Seite **nichts**, was den Server aus diesem Zustand befreit — er
    /// wiederholt, wir wiederholen die Anfrage, und beide warten.
    client_finished: Option<Vec<u8>>,

    /// Pakete ohne passenden Schluessel (Q6a).
    puffer_handshake: PaketPuffer,
    puffer_eins: PaketPuffer,

    /// Gepufferte Paketnummern je Ebene fuer die ACKs (M3).
    initial_seen: Verfolger,
    handshake_seen: Verfolger,
    eins_seen: Verfolger,

    /// Stateless-Reset-Token (RFC 9000 Abschnitt 10.3).
    reset_token: Vec<[u8; 16]>,

    streams: BTreeMap<u64, StreamPuffer>,
    /// Das zuletzt **erteilte** MAX_STREAM_DATA-Limit (U2). Ohne dieses Feld
    /// wuesste der Client nicht, was er schon versprochen hat, und koennte
    /// kein neues Limit berechnen, das groesser ist.
    limit_strom: u64,
    /// Das zuletzt erteilte MAX_DATA-Limit.
    limit_gesamt: u64,
    /// Hat der Server seine SETTINGS geschickt? (U2, RFC 9114 7.2.4.1)
    server_settings: bool,
    /// Die Werte daraus, fuer `Optionen`.
    server_settings_werte: Vec<(u64, u64)>,
    /// DER HOEHSTE vom Server quittierte Wert im 1-RTT-Nummernraum (U2-Rest).
    ///
    /// WAS DAMIT GEMESSEN WIRD, und warum es ein Feld sein muss: die Frage
    /// "hat der Server unser Paket bekommen?" laesst sich von aussen nicht
    /// stellen, und sie laesst sich am laufenden Trace nicht beantworten,
    /// weil dortPakete stehen und keine Quittungen. Das Feld sammelt jede
    /// ACK-Nummer, die der Server uns fuer die 1-RTT-Ebene schickt, und der
    /// Wert wird beim Abbruch **mit ausgegeben**.
    ///
    /// `None` heisst: der Server hat uns im 1-RTT-Raum **nie** etwas
    /// quittiert. Das ist ein Unterschied zu 0, denn 0 hiesse "unser erstes
    /// Paket ist angekommen".
    ///
    /// DER GRUND FUER EIN FELD UND NICHT FUER EINE TRACE-ZEILE: der Wert muss
    /// dann da sein, wenn der Lauf **fehlschlaegt** — und in einem Fehlschlag
    /// ist die letzte Trace-Zeile genau das, was man nicht mehr liest.
    server_acks_groesste_1rtt: Option<u64>,
    /// Dasselbe fuer die **Handshake**-Ebene (U2-Bericht N23).
    ///
    /// WARUM ES GEBRAUCHT WIRD, obwohl es neben 1-RTT redundant aussieht: der
    /// Server **wiederholt** seine Handshake-CRYPTO, und Wiederholung heisst
    /// "ich habe dein Paket nicht" (RFC 9000 19.3). Wenn er also unsere
    /// Handshake-Pakete nicht quittiert, liegt der Fehler **vor** der 1-RTT-
    /// Ebene — und meine gesamte Beweiskette "1-RTT ist einwandfrei" waere
    /// an der falschen Stelle gesucht.
    ///
    /// Das war eine Luecke in N8-N20: ich hatte 1-RTT gemessen und die
    /// Handshake-Ebene nur mit der Gegenprobe (lesbar) geprueft — nie ihre
    /// Quittung. Lesbar und quittiert sind zwei verschiedene Fragen, und nur
    /// die zweite sagt, ob der Server etwas bekommen hat.
    server_acks_groesste_handshake: Option<u64>,
    antwort: Option<Vec<u8>>,
    handshake_done: bool,
    /// Die 1-RTT-Nutzlast der Anfrage (Steuerstrom + GET), ungeschluetzt.
    ///
    /// DER GRUND FUER DAS MERKEN: QUIC ist zuverlaessig ueber Paketnummern,
    /// nicht ueber Pakete. Geht das Paket mit dem GET verloren, muss der
    /// Client es mit einer **neuen** Paketnummer erneut senden (RFC 9000
    /// Abschnitt 12.3: Nummern werden nie wiederverwendet, die STREAM-Offsets
    /// aber auch nicht geaendert). Ohne diese Wiederholung faellt jede
    /// Anfrage weg, deren erstes Paket der Server nicht entziffern kann — und
    /// das ist die Regel, nicht die Ausnahme: trifft ein 1-RTT-Paket ein,
    /// bevor der Server das Client-Finished verarbeitet hat, sind seine
    /// 1-RTT-Schluessel noch nicht bereit, und er verwirft es lautlos
    /// (RFC 9000 Abschnitt 5.5). Genau das ist hier passiert.
    anfrage_nutzlast: Vec<u8>,

    suite: Suite,
    server_idle_ms: Option<u64>,
    zertifikate: Vec<Vec<u8>>,
    zertifikat_geprueft: bool,
    kette_byte: usize,
    nachrichten: usize,

    /// Das zuletzt empfangene Datagramm (die Aufrufer von `empfange`
    /// brauchen es, ohne es selbst kopieren zu muessen).
    letztes: Vec<u8>,
    frist: Instant,
}

/// Trace-Ausgabe. AN/AUS ueber die Umgebungsvariable `NARSIL_QUIC_TRACE=1`.
///
/// WARUM EIGENTLICH IM CODE: ein Netzfehler wie "Paket braucht 18 Byte, da sind
/// 17" sagt nichts darueber, **welches** Paket das war. Bei drei Servern und
/// fuenf Phasen ist das Raten. Der Trace nennt Phase, Ebene und Laenge — und
/// ist per Voreinstellung aus, damit normale Tests still bleiben.
fn trace(phase: &str, text: &str) {
    // N46: Die Spur wird **jetzt immer** behalten (`src/spur.rs`), nicht nur
    // wenn jemand sie ansieht. Grund: der Live-Test `cloudflare_quic_laden`
    // fiel einmal von 15 Laeufen aus, und **fuer diesen Lauf gab es keine
    // Spur** — sie war damit beim naechsten Auftreten wiederlost. Jetzt ruft
    // der Aufrufer `spur_letzte(n)` und hat sie.
    //
    // Die zweite Folge ist ein Gewinn: `std::env::var_os` war der teuerste
    // Teil **jeder** Trace-Zeile, und es gab ein paar Zeilen je Paket.
    crate::spur::spur_zeile(phase, text);
}

/// Der Kurzname eines Frames fuer den Trace.
fn rahmen_name(r: &Rahmen) -> String {
    match r {
        Rahmen::Padding => "PADDING".into(),
        Rahmen::Ping => "PING".into(),
        Rahmen::Ack {
            groesste,
            erste_range,
            bereiche,
            ..
        } => format!(
            "ACK(bis {groesste}, erste {erste_range}, {} Bereiche)",
            bereiche.len()
        ),
        Rahmen::ResetStream { stream_id, .. } => format!("RESET_STREAM({stream_id})"),
        Rahmen::StopSending { stream_id, .. } => format!("STOP_SENDING({stream_id})"),
        Rahmen::Crypto { offset, daten } => format!("CRYPTO({offset},{})", daten.len()),
        Rahmen::NewToken { .. } => "NEW_TOKEN".into(),
        Rahmen::Stream {
            stream_id,
            offset,
            daten,
            fin,
        } => format!("STREAM({stream_id},{offset},{},fin={fin})", daten.len()),
        Rahmen::MaxData { maximum } => format!("MAX_DATA({maximum})"),
        Rahmen::MaxStreamData { stream_id, maximum } => {
            format!("MAX_STREAM_DATA({stream_id},{maximum})")
        }
        Rahmen::MaxStreams {
            bidirektional,
            maximum,
        } => format!("MAX_STREAMS(bidi={bidirektional},{maximum})"),
        Rahmen::DataBlocked { maximum } => format!("DATA_BLOCKED({maximum})"),
        Rahmen::StreamDataBlocked { stream_id, .. } => format!("STREAM_DATA_BLOCKED({stream_id})"),
        Rahmen::StreamsBlocked { .. } => "STREAMS_BLOCKED".into(),
        Rahmen::NewConnectionId { sequenz, .. } => format!("NEW_CONNECTION_ID({sequenz})"),
        Rahmen::RetireConnectionId { sequenz } => format!("RETIRE_CONNECTION_ID({sequenz})"),
        Rahmen::PathChallenge { .. } => "PATH_CHALLENGE".into(),
        Rahmen::PathResponse { .. } => "PATH_RESPONSE".into(),
        Rahmen::ConnectionClose { fehlercode, .. } => format!("CONNECTION_CLOSE({fehlercode})"),
        Rahmen::HandshakeDone => "HANDSHAKE_DONE".into(),
    }
}

/// Was ein Lesevorgang ergeben hat.
enum Empfang {
    /// Das letzte Datagramm liegt in `Sitzung::letztes`.
    Daten,
    Timeout,
    StatelessReset,
}

impl Sitzung {
    /// Baut die Sitzung auf und schickt den ersten Initial.
    pub fn neu(url: &str, opt: &Optionen) -> Result<Sitzung, ClientFehler> {
        let (host, port, pfad) = zerlege_url(url)?;
        let adresse = adresse_von(&host, port)?;
        let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| ClientFehler::Netz(e.to_string()))?;
        // N43: **kein `connect`**. Siehe das Feld `ziel` — ein verbundener
        // Socket laesst den Kernel alle Datagramme einer anderen Quelle
        // verschwinden, ohne dass unser Code etwas davon merkt.
        // Ohne Zeitlimit waere jedes haengende Netz der Grund fuer einen
        // 10-Minuten-Bau. Das wird bei jedem Lesen neu gesetzt.
        socket
            .set_read_timeout(Some(ZEITBLOCK))
            .map_err(|e| ClientFehler::Netz(e.to_string()))?;

        let geheim =
            narsil_tls::zufall::schluessel32().map_err(|e| ClientFehler::Handshake(e.to_string()))?;
        let oeffentlich = x25519::oeffentlich(&geheim);
        let scid =
            narsil_tls::zufall::bytes(8).map_err(|e| ClientFehler::Handshake(e.to_string()))?;
        let dcid =
            narsil_tls::zufall::bytes(8).map_err(|e| ClientFehler::Handshake(e.to_string()))?;

        // Q7.2, DIE UHR GEHT FRUEHER.
        //
        // DER GRUND: `t_gesamt` wurde erst in `lauf_mit_inhalt` gestartet, also
        // **nach** `Sitzung::neu`. Damit zaehlten DNS-Aufloesung, `UdpSocket::
        // bind`, die x25519-Schluesselberechnung und der ClientHello-Aufbau
        // **nicht** mit — und die Phase „sonstiges" (konstant 55 bis 62 ms,
        // gemessen) liegt genau dort.
        //
        // GEMESSEN: Prozess gesamt 257 bis 264 ms. Die Differenz zur gemessenen
        // Phase ist der Aufbau, und ohne diese Aenderung weiss man nicht, dass.
        let t_aufbau = std::time::Instant::now();
        let mut sitzung = Sitzung {
            socket,
            ziel: adresse,
            zeit_entschluesseln: 0,
            zeit_rahmen: 0,
            zeit_h3: 0,
            zeit_kopieren: 0,
            zeit_stream: 0,
            wartezeit_gesamt: 0,
            wartezeit_echt: 0,
            wartezeit_max: 0,
            wartezeit_letzte: 0,
            sendezeit: std::cell::Cell::new(0),
            sendungen: std::cell::Cell::new(0),
            pakete_entschluesselt: 0,
            pakete_gelesen: 0,
            t_gesamt: None,
            quelle_gewechselt: 0,
            host: host.clone(),
            pfad,
            opt: opt.clone(),
            scid,
            dcid: dcid.clone(),
            urspruengliche_dcid: dcid,
            geheim,
            clienthello: Vec::new(),
            plan: Schluesselplan::neu([0u8; 32]),
            serverhello: Vec::new(),
            initial_client: leerschluessel(),
            initial_server: leerschluessel(),
            handshake_client: None,
            handshake_server: None,
            eins_client: None,
            eins_server: None,
            naechste_pn: [0, 0, 0],
            crypto_initial: CryptoReassembly::neu(),
            crypto_handshake: CryptoReassembly::neu(),
            server_finished: false,
            client_finished: None,
            puffer_handshake: PaketPuffer::neu(),
            puffer_eins: PaketPuffer::neu(),
            initial_seen: Verfolger::default(),
            handshake_seen: Verfolger::default(),
            eins_seen: Verfolger::default(),
            reset_token: Vec::new(),
            streams: BTreeMap::new(),
            limit_strom: FENSTER_START,
            limit_gesamt: FENSTER_START,
            server_settings: false,
            server_settings_werte: Vec::new(),
            server_acks_groesste_1rtt: None,
            server_acks_groesste_handshake: None,
            antwort: None,
            handshake_done: false,
            anfrage_nutzlast: Vec::new(),
            suite: Suite::Aes128Gcm,
            server_idle_ms: None,
            zertifikate: Vec::new(),
            zertifikat_geprueft: false,
            kette_byte: 0,
            nachrichten: 0,
            letztes: Vec::new(),
            frist: Instant::now() + opt.gesamt_zeitlimit.unwrap_or(ZEIT_GESAMT),
        };
        // Q7.2: Die Uhr startet **hier**, vor Schluesselberechnung und
        // ClientHello-Aufbau.
        sitzung.t_gesamt = Some(t_aufbau);
        sitzung.berechne_initial_schluessel()?;
        sitzung.clienthello = sitzung.baut_clienthello(&oeffentlich)?;
        sitzung.sende_initial(0)?;
        Ok(sitzung)
    }

    /// Der Handlauf in drei Phasen.
    pub fn lauf(mut self) -> Result<Antwort, ClientFehler> {
        self.phase_serverhello()?;
        self.phase_handshake()?;
        self.phase_antwort().map(|a| a.0)
    }

    /// Wie `lauf`, aber mit den Inhaltsbytes der DATA-Frames (U3).
    ///
    /// DER GRUND FUER EINE EIGENE LAUFVARIANTE: `phase_antwort` baute bisher
    /// nur die `Antwort` und **warf die DATA-Bytes weg**, weil es sie fuer
    /// `Antwort.groesse` nur zaehlen musste. Die Bytes liegen aber schon da
    /// — sie werden im Puffer durchlaufen — und sie noch einmal zu holen
    /// heisst einen zweiten Handshake. Hier werden sie beim Zaehlen
    /// mitgenommen, was **keinen** zusaetzlichen Speicher und **keinen**
    /// zusaetzlichen Netzweg kostet.
    pub fn lauf_mit_inhalt(mut self) -> Result<(Antwort, Vec<u8>), ClientFehler> {
        // Q7: Die Uhr startet hier und wird in `phase_antwort` ausgegeben —
        // dort, weil sie `self` **bewegt** (`mut self`), eine Ausgabe danach
        // also nicht mehr moeglich waere. Der erste Versuch stand hier und
        // scheiterte an `borrow of moved value`.
        // Q7.2: `t_gesamt` kommt aus `Sitzung::neu` — dort steht der
        // Aufbauzeitpunkt. Vorher stand hier ein eigener `Instant::now()`, und
        // damit begann die Uhr **nach** der DNS-Aufloesung.
        debug_assert!(
            self.t_gesamt.is_some(),
            "M3: `Sitzung::neu` muss `t_gesamt` setzen, sonst zaehlt die \
Verteilung ab dem ersten Paket statt ab dem Aufruf"
        );
        // Q7.2: DIE PHASENGRENZEN, und das ist die Antwort auf die Restgroesse
        // „sonstiges".
        //
        // DER GRUND: 57 bis 64 ms sind konstant ueber **beide** Bauarten und
        // ueber die Laufzeit. Sie kommen also nicht vom Netz (das schwankt
        // zwischen 99 und 202 ms) und nicht vom Rechnen (11 bis 15 ms). Sie
        // liegen in einer der Phasen oder zwischen ihnen — und ohne Marke an
        // den Grenzen ist das nicht entscheidbar.
        let t_ph0 = std::time::Instant::now();
        self.phase_serverhello()?;
        let n_sh = t_ph0.elapsed().as_millis();
        let t_ph1 = std::time::Instant::now();
        self.phase_handshake()?;
        let n_hs = t_ph1.elapsed().as_millis();
        trace(
            "phase",
            &format!(
                "ServerHello {n_sh} ms, Handshake {n_hs} ms, bis hier {} ms von {}",
                n_sh + n_hs,
                self.t_gesamt.map(|t| t.elapsed().as_millis()).unwrap_or(0)
            ),
        );
        self.phase_antwort()
    }

    // -----------------------------------------------------------------------
    // Phase 1: bis zum ServerHello
    // -----------------------------------------------------------------------

    fn phase_serverhello(&mut self) -> Result<(), ClientFehler> {
        let mut pakete = 0usize;
        let mut pto = 0u32;
        while self.serverhello.is_empty() {
            if pakete >= MAX_HANDSHAKE_DATAGRAMME {
                return Err(ClientFehler::Zeitueberschreitung(
                    "kein ServerHello im Datagrammlimit".into(),
                ));
            }
            self.pruefe_frist()?;
            match self.empfange()? {
                Empfang::Daten => {
                    pakete += 1;
                    self.bearbeite_initial()?;
                }
                Empfang::Timeout => {
                    pto += 1;
                    if pto > MAX_PTO {
                        return Err(ClientFehler::Zeitueberschreitung(
                            "kein ServerHello nach dem PTO-Limit".into(),
                        ));
                    }
                    // RFC 9002 Abschnitt 6.2.1. Das wiederholte Initial
                    // schickt wieder 1200 Byte und gibt dem Server damit
                    // Anti-Verstaerkungsbudget fuer die Zertifikatskette.
                    self.sende_initial(self.naechste_pn[0])?;
                }
                Empfang::StatelessReset => return Err(ClientFehler::StatelessReset),
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Phase 2: EncryptedExtensions bis zum Server-Finished
    // -----------------------------------------------------------------------

    fn phase_handshake(&mut self) -> Result<(), ClientFehler> {
        // ACK auf die Initial-Pakete. Es ist kein ack-ausloesendes Paket, der
        // Server wartet also nicht darauf — ohne es wiederholt er sein Initial.
        self.sende_initial_ack()?;

        // Die in Phase 1 gepufferten Handshake-Pakete jetzt: fuer sie gab es
        // vor dem ServerHello noch keinen Schluessel (Q6a).
        let gepuffert = self.puffer_handshake.entleere();
        for paket in gepuffert {
            self.entschluessel_handshake(&paket)?;
        }

        let mut pakete = 0usize;
        let mut pto = 0u32;
        while !self.server_finished {
            if pakete >= MAX_HANDSHAKE_DATAGRAMME {
                return Err(ClientFehler::Zeitueberschreitung(
                    "kein Server-Finished im Datagrammlimit".into(),
                ));
            }
            self.pruefe_frist()?;
            match self.empfange()? {
                Empfang::Daten => {
                    pakete += 1;
                    self.bearbeite_handshake_datagramm()?;
                }
                Empfang::Timeout => {
                    pto += 1;
                    if pto > MAX_PTO {
                        return Err(ClientFehler::Zeitueberschreitung(
                            "kein Server-Finished nach dem PTO-Limit".into(),
                        ));
                    }
                    self.sende_handshake_ping()?;
                }
                Empfang::StatelessReset => return Err(ClientFehler::StatelessReset),
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Phase 3: Client-Finished, 1-RTT, HTTP/3
    // -----------------------------------------------------------------------

    fn phase_antwort(mut self) -> Result<(Antwort, Vec<u8>), ClientFehler> {
        self.sende_client_finished()?;
        // DER DRITTE VERSUCH, den Server ueber die 1-RTT-Ebene zu erreichen,
        // und diesmal mit allem, was die Norm verlangt:
        //
        // * das Client-Finished und die 1-RTT-Anfrage in **einem**
        //   Handshake-Paket bzw. direkt danach — erledigt,
        // * die Initial-Paketnummer wird **nicht** zurueckgesetzt — erledigt,
        // * und: der Server muss unsere 1-RTT-Pakete **auch entziffern
        //   koennen**. Dafuer ist seine eigene 1-RTT-Richtung `s ap traffic`
        //   zustaendig, und die leitet er aus demselben Transkript ab wie
        //   `c ap traffic`. Beides steht in `plan.anwendungsphase_client()`,
        //   das **vor** dem `aufnehmen(client_finished)` laeuft (Zeile 684
        //   ff.). Das ist die Reihenfolge aus RFC 8446 Abschnitt 7.1.
        //
        // Bleibt danach Stille, ist der Grund nicht mehr lokal zu finden;
        // dann gehoert der Befund in den Bericht und nicht in eine weitere
        // Aenderung hier.
        trace(
            "puffer",
            &format!(
                "Handshake-Puffer: {} Pakete, {} verworfen; 1-RTT-Puffer: {} Pakete, {} verworfen",
                self.puffer_handshake.anzahl(),
                self.puffer_handshake.verworfen(),
                self.puffer_eins.anzahl(),
                self.puffer_eins.verworfen(),
            ),
        );
        // BIFURKATION (NARSIL_QUIC_BISECT=1): nur das Client-Finished senden und
        // dann WARTEN. Damit faellt die Frage auseinander, ob der Server das
        // Finished annimmt (dann kommt HANDSHAKE_DONE) oder ob er es verwirft
        // (dann bleibt es still). Siehe Bericht Q6c.
        if std::env::var_os("NARSIL_QUIC_BISECT").is_some() {
            // U3: `bisect_nur_finished` liefert nur die `Antwort` (es geht
            // absichtlich keine Anfrage raus, also gibt es keine Nutzlast).
            // Der Inhalt ist darum **leer** — und das wird auch so gemeldet:
            // `groesse` ist dann 0, und `debug_assert_eq` in
            // `parse_h3_antwort` haelt beide gross. Ein Weg, der nur
            // diagnostisch ist, gibt keine erfundenen Bytes zurueck.
            return self.bisect_nur_finished().map(|a| (a, Vec::new()));
        }
        self.sende_http3_anfrage()?;
        self.lese_antwort()?;
        let roh = self.antwort.clone().unwrap_or_default();
        self.parse_h3_antwort(&roh)
    }

    /// Nur das Client-Finished senden, dann eine Weile lauschen. Reine
    /// Diagnose, keine Abruf-Funktion.
    fn bisect_nur_finished(&mut self) -> Result<Antwort, ClientFehler> {
        self.setze_idle_frist();
        for _ in 0..12 {
            self.pruefe_frist()?;
            match self.empfange()? {
                Empfang::Daten => self.bearbeite_eins_datagramm()?,
                Empfang::Timeout => {
                    self.sende_handshake_ack()?;
                    self.sende_handshake_ping()?;
                }
                Empfang::StatelessReset => return Err(ClientFehler::StatelessReset),
            }
        }
        Err(ClientFehler::Zeitueberschreitung(
            "Bifurkation: nach dem Client-Finished kam nichts".into(),
        ))
    }

    /// ACK fuer die zuletzt empfangenen Handshake-Pakete, noch auf der
    /// Handshake-Ebene.
    ///
    /// DER GRUND FUER DEN EIGENEN SCHRITT: nach dem Client-Finished kommen
    /// beim Cloudflare-Server noch Handshake-Pakete an (sein ACK). Wer die
    /// nur liest und nicht quittet, bekommt sie bei jedem PTO erneut — und
    /// der Server haelt die Handshake-Schluessel offen, solange er sie
    /// nicht losgelassen hat. Die 1-RTT-Schluessel sind davon unabhaengig,
    /// aber die Bestellung ist billig und macht den Zustand deterministic.
    fn sende_handshake_ack(&mut self) -> Result<(), ClientFehler> {
        if self.handshake_seen.benoetigt_kein_ack() {
            return Ok(());
        }
        let schluessel = self
            .handshake_client
            .clone()
            .ok_or_else(|| ClientFehler::Handshake("kein Handshake-Schluessel".into()))?;
        let pn = self.naechste_pn[1];
        self.naechste_pn[1] = pn + 1;
        let payload = ack_payload(&self.handshake_seen);
        // Q7: erst danach leeren — geht das Paket nicht raus, ist die
        // Quittung verloren und `nummern` muss sie noch tragen.
        self.handshake_seen.entleere();
        let paket = lang_paket_gefuellt(
            &schluessel,
            TYP_HANDSHAKE,
            &self.dcid,
            // N43: **nicht `&[]`**. Das war hier ueberall so — und es ist eine
            // **Inkonsistenz in unseren eigenen Paketen**: das Initial traegt
            // unsere 8-Byte-SCID, das Handshake-Paket keine.
            //
            // GEMESSEN, und der Unterschied ist nicht theoretisch:
            //   cloudflare-quic.com  Initial 8 Byte SCID, Handshake 0 Byte
            //                        -> antwortet mit STATUS 200
            //   www.google.com       Initial 8 Byte SCID, Handshake 0 Byte
            //   nginx                Initial 8 Byte SCID, Handshake 0 Byte
            //                        -> beide quittieren unser Initial (ACK
            //                        bis 0) und schicken danach **Handshake-
            //                        PINGs** (nginx, pn=4..7): sie warten auf
            //                        unsere Handshake-Pakete und bekommen sie
            //                        nicht.
            //
            // DIE REGEL (RFC 9000 7.2): Die Source Connection ID eines Endpunkts
            // bleibt ueber die Verbindung **gleich**. Der Server benutzt sie als
            // zweiten Schluessel beim Demultiplexen — ein Paket, dessen SCID
            // nicht zum Datensatz passt, gehoert fuer ihn nicht zu dieser
            // Verbindung, und Cloudflare ist offenbar tolerant, die anderen
            // nicht.
            &self.scid,
            &[],
            pn,
            2,
            &payload,
            0,
        )?;
        trace(
            "sendung",
            &format!("Handshake ACK pn={pn}, {} Byte", paket.len()),
        );
        self.sende(&paket)
            .map(|_| ())
            .map_err(|e| ClientFehler::Netz(e.to_string()))
    }

    fn sende_client_finished(&mut self) -> Result<(), ClientFehler> {
        // DER HASH, UM DEN ES HIER GEHT: `ClientHello..ServerFinished`.
        //
        // RFC 8446 Abschnitt 7.1: `c ap traffic` und `s ap traffic` stehen
        // ueber **diesem** Transkript. Das eigene Client-Finished gehoert
        // ausdruecklich NICHT dazu (Abschnitt 4.4.4 nennt fuer `verify_data`
        // den Stand bis einschliesslich `CertificateVerify*`). Die beiden
        // Stellen unterscheiden sich um 4 + 32 Byte, und beide brauchen
        // **denselben** Hash — deshalb wird er einmal geholt und zweimal
        // benutzt.
        let th_bis_server_finished = self.plan.transkript_hash();

        // 1) Anwendungsgeheimnisse, aus genau diesem Hash.
        self.plan.anwendungsphase_client();
        let client_1rtt = schluessel_ableiten(&self.plan.client_ap_traffic, self.suite)?;
        let server_1rtt = schluessel_ableiten(&self.plan.server_ap_traffic, self.suite)?;
        self.eins_client = Some(client_1rtt);
        self.eins_server = Some(server_1rtt);

        // 2) Das eigene Finished, ueber denselben Hash.
        let verify = Finished::erwartet(&self.plan.finished_key_client, &th_bis_server_finished);
        let mut nachricht = Vec::with_capacity(36);
        nachricht.push(typen::FINISHED);
        // DIE LAENGE IST EIN uint24, KEIN VARINT.
        //
        // DER FEHLER, DER DEN GANZEN HANDSHAKE GETOETET HAT, und er ist
        // erst am echten Server aufgefallen: hier stand
        // `varint::schreibe(32, ...)`, und ein Varint fuer 32 ist **ein**
        // Byte. Die Nachricht kam deshalb als `14 20 <32 Byte>` an statt als
        // `14 00 00 20 <32 Byte>` — und die Norm verlangt in
        // RFC 8446 Abschnitt 4: "struct { ProtocolType type; uint24 length; }"
        // in three-byte lengths, **keine** variablen Laengen.
        //
        // WARUM ES SO LANG UNBEMERKT BLIEB: `Finished::dekodiere` in narsil-tls
        // liest die Laenge ueber `Leser::vektor24`, also drei Byte. Unser
        // eigener Dekodierer in `verarbeite_tls_nachrichten` rechnet die
        // Laenge genauso — aus drei Byte. Beide lesen dasselbe Feld gleich
        // falsch und sind deshalb miteinander konsistent. Nur der Server
        // las die Nachricht als `Finished` mit Laenge `0x20020d` und
        // verwarf sie; er schickte deshalb kein `HANDSHAKE_DONE` und keine
        // 1-RTT-Antwort, sondern nur PING auf der Handshake-Ebene.
        //
        // DER BEWEIS: die Hex-Ausgabe des Trace im Bericht
        // (`... 06 00 22 14 20 02 0d ...`) zeigt genau das falsche
        // Ein-Byte-Laengenfeld.
        nachricht.extend_from_slice(&32u32.to_be_bytes()[1..]); // uint24 = 0x000020
        nachricht.extend_from_slice(&verify);
        // Fuer die 1-RTS-Schluessel ist das danach egal, fuer jede spaetere
        // Fortsetzung (Sitzungswiederaufnahme, Wechsel der Suite) nicht.
        self.plan.aufnehmen(&nachricht);

        let schluessel = self
            .handshake_client
            .clone()
            .ok_or_else(|| ClientFehler::Handshake("kein Handshake-Schluessel".into()))?;
        // N28: aufbewahren, nicht wegwerfen. Ohne das gibt es keine
        // Wiederholung auf der Handshake-Ebene, und die Zahlen von google
        // (7 Rueckholungen seiner CRYPTO) und nginx (16) zeigen, dass genau
        // diese eine Nachricht bei beiden nicht ankommt.
        self.client_finished = Some(nachricht.clone());
        let pn = self.naechste_pn[1];
        self.naechste_pn[1] = pn + 1;
        let mut payload = ack_payload(&self.handshake_seen);
        rahmen_schreiben(
            &Rahmen::Crypto {
                offset: 0,
                daten: nachricht,
            },
            &mut payload,
        )?;
        let paket = lang_paket_gefuellt(
            &schluessel,
            TYP_HANDSHAKE,
            &self.dcid,
            // N43: **nicht `&[]`**. Das war hier ueberall so — und es ist eine
            // **Inkonsistenz in unseren eigenen Paketen**: das Initial traegt
            // unsere 8-Byte-SCID, das Handshake-Paket keine.
            //
            // GEMESSEN, und der Unterschied ist nicht theoretisch:
            //   cloudflare-quic.com  Initial 8 Byte SCID, Handshake 0 Byte
            //                        -> antwortet mit STATUS 200
            //   www.google.com       Initial 8 Byte SCID, Handshake 0 Byte
            //   nginx                Initial 8 Byte SCID, Handshake 0 Byte
            //                        -> beide quittieren unser Initial (ACK
            //                        bis 0) und schicken danach **Handshake-
            //                        PINGs** (nginx, pn=4..7): sie warten auf
            //                        unsere Handshake-Pakete und bekommen sie
            //                        nicht.
            //
            // DIE REGEL (RFC 9000 7.2): Die Source Connection ID eines Endpunkts
            // bleibt ueber die Verbindung **gleich**. Der Server benutzt sie als
            // zweiten Schluessel beim Demultiplexen — ein Paket, dessen SCID
            // nicht zum Datensatz passt, gehoert fuer ihn nicht zu dieser
            // Verbindung, und Cloudflare ist offenbar tolerant, die anderen
            // nicht.
            &self.scid,
            &[],
            pn,
            2,
            &payload,
            0,
        )?;
        trace(
            "sendung",
            &format!(
                concat!(
                    // N42: Die Beschriftung stand hier als "ACK+PING" — und
                    // das Paket traegt **ACK plus CRYPTO** (das Client
                    // Finished). Mit `rahmen_alle` nachgewiesen in
                    // `tests/n42_echte_bytes.rs`.
                    //
                    // WARUM DAS EINE EIGENE SACHE IST: eine Beschriftung, die
                    // etwas anderes sagt als die Bytes, hat mich in dieser
                    // Runde zweimal eine **falsche Ursache** gebaut — einmal so
                    // (ich las "PING", nicht "CRYPTO"), einmal mit derselben
                    // Fehlerklasse an einer Hex-Zeile. Was die Spur ueber ein
                    // Paket behauptet, muss aus dem Paket **berechnet** sein.
                    "Handshake ACK+CRYPTO(Finished) pn={}, {} Byte, dcid={} ({} Byte)",
                    " -- N25: darueber findet der Server die Verbindung"
                ),
                pn,
                paket.len(),
                narsil_tls::sha2::hexen(&self.dcid),
                self.dcid.len()
            ),
        );
        // U2-Rest, dieselbe Gegenprobe wie bei 1-RTT, nur auf der
        // Handshake-Ebene. DER GRUND: der Server wiederholt seine
        // Handshake-CRYPTO, und das tut ein Server, wenn er unsere Pakete auf
        // dieser Ebene **nicht** bekommt. Wenn unser ACK hier nicht lesbar
        // waere, waere das eine Ursache, die **lokal** entscheidbar ist —
        // und die habe ich bis eben nicht geprueft, weil ich die 1-RTT-Ebene
        // fuer die einzige gehalten habe.
        //
        // RICHTIG IST `handshake_client` (das Geheimnis der **Senderseite**,
        // RFC 9001 5), und `dcid.len()` als Laenge der
        // **Ziel**-Connection-ID.
        // N42: DIESE ZEILE WAR KEIN BELEG, und sie hat wochenlang als einer
        // gelesen. Sie entschluesselt ein Paket mit **dem Schluessel, mit dem
        // es gerade gebaut wurde** — sie kann nicht scheitern, also kann sie
        // nichts bestaetigen.
        //
        // WAS SIE ZEIGT: nichts ueber die Schluesselberechnung, nichts ueber
        // das, was der Server daraus macht, nichts ueber die Verbindung. Sie
        // zeigt, dass `initial_schuetzen` und `entschuetzen` zusammenpassen.
        //
        // EINE ECHTE GEGENPROBE braucht eine **unabhaengige** Grundlage. Fuer
        // die 1-RTT-Ebene habe ich eine: wir entschluesseln ein **echtes**
        // Serverpaket mit dem 1-RTT-Schluessel, und das gelingt (`[quic
        // entschlüsseln] EinsRtt pn=6`). Das sagt etwas ueber die Schluessel,
        // weil der Server sie unabhaengig abgeleitet hat.
        match crate::schutz::entschuetzen(&schluessel, &paket, self.dcid.len(), pn) {
            Ok(g) => trace(
                "gegenprobe",
                &format!(
                    concat!(
                        "TAUTOLOGIE, KEINE GEGENPROBE: mit dem CLIENT-Schluessel lesbar: ",
                        "pn={} (gesendet als pn {}), {} Byte Klartext; roh: {}"
                    ),
                    g.paketnummer,
                    pn,
                    g.klartext.len(),
                    narsil_tls::sha2::hexen(&g.klartext[..g.klartext.len().min(64)])
                ),
            ),
            Err(e) => trace(
                "gegenprobe",
                &format!("unser Handshake-Paket ist NICHT lesbar: {e} (pn={pn})"),
            ),
        }
        self.sende(&paket)
            .map(|_| ())
            .map_err(|e| ClientFehler::Netz(e.to_string()))
    }

    /// Wie viele 1-RTT-Pakete hat der Server bisher geschickt? Fuer die
    /// Diagnose im Trace.
    fn eins_gekommen(&self) -> usize {
        self.eins_seen.groesste.map_or(0, |g| g as usize + 1)
    }

    /// Das Client-Finished erneut schicken (N28, RFC 9000 Abschnitt 19.3).
    ///
    /// DER MASSSTAB, an dem sich entscheidet, ob ueberhaupt gesendet wird:
    /// `!self.handshake_done`. Solange der Server kein `HANDSHAKE_DONE`
    /// geschickt hat, ist der Handshake nicht bestaetigt, und nach RFC 9000
    /// Abschnitt 7.1 ist genau dann der Punkt, an dem unser Finished noch
    /// unterwegs sein kann.
    ///
    /// **Warum kein Leerlauf-Ping, sondern das Finished:** der Server hat
    /// Application-Keys, unsere 1-RTT-Pakete zu lesen weiss er aber erst nach
    /// unserem Finished. Solange es fehlt, ist jede Wiederholung auf 1-RTT ein
    /// Paket, das er per RFC 9000 Abschnitt 7.1 gar nicht bearbeiten darf.
    /// Nur das Finished kann ihn aus dem Zustand loesen.
    ///
    /// DIE GRENZE: `MAX_PTO` (gilt ueber die PTO-Schleife in `lese_antwort`)
    /// und die Nutzlast sind unsere **eigenen** 36 Byte, nicht fremde — es
    /// kommt also nichts aus dem Netz in diese Schleife hinein.
    fn wiederhole_client_finished(&mut self) -> Result<(), ClientFehler> {
        if self.handshake_done {
            return Ok(());
        }
        let Some(nachricht) = self.client_finished.clone() else {
            return Ok(());
        };
        let Some(schluessel) = self.handshake_client.clone() else {
            return Ok(());
        };
        let pn = self.naechste_pn[1];
        self.naechste_pn[1] = pn + 1;
        let mut payload = ack_payload(&self.handshake_seen);
        rahmen_schreiben(
            &Rahmen::Crypto {
                offset: 0,
                daten: nachricht,
            },
            &mut payload,
        )?;
        rahmen_schreiben(&Rahmen::Ping, &mut payload)?;
        let paket = lang_paket_gefuellt(
            &schluessel,
            TYP_HANDSHAKE,
            &self.dcid,
            // N43: **nicht `&[]`**. Das war hier ueberall so — und es ist eine
            // **Inkonsistenz in unseren eigenen Paketen**: das Initial traegt
            // unsere 8-Byte-SCID, das Handshake-Paket keine.
            //
            // GEMESSEN, und der Unterschied ist nicht theoretisch:
            //   cloudflare-quic.com  Initial 8 Byte SCID, Handshake 0 Byte
            //                        -> antwortet mit STATUS 200
            //   www.google.com       Initial 8 Byte SCID, Handshake 0 Byte
            //   nginx                Initial 8 Byte SCID, Handshake 0 Byte
            //                        -> beide quittieren unser Initial (ACK
            //                        bis 0) und schicken danach **Handshake-
            //                        PINGs** (nginx, pn=4..7): sie warten auf
            //                        unsere Handshake-Pakete und bekommen sie
            //                        nicht.
            //
            // DIE REGEL (RFC 9000 7.2): Die Source Connection ID eines Endpunkts
            // bleibt ueber die Verbindung **gleich**. Der Server benutzt sie als
            // zweiten Schluessel beim Demultiplexen — ein Paket, dessen SCID
            // nicht zum Datensatz passt, gehoert fuer ihn nicht zu dieser
            // Verbindung, und Cloudflare ist offenbar tolerant, die anderen
            // nicht.
            &self.scid,
            &[],
            pn,
            2,
            &payload,
            0,
        )?;
        trace(
            "sendung",
            &format!(
                "Handshake Finished WIEDERHOLT pn={pn}, {} Byte (N28)",
                paket.len()
            ),
        );
        self.sende(&paket)
            .map(|_| ())
            .map_err(|e| ClientFehler::Netz(e.to_string()))
    }

    fn sende_http3_anfrage(&mut self) -> Result<(), ClientFehler> {
        let schluessel = self
            .eins_client
            .clone()
            .ok_or_else(|| ClientFehler::Handshake("kein 1-RTT-Schluessel".into()))?;

        // RFC 9114 Abschnitt 6.2.1: der Client oeffnet als **erstes** einen
        // Steuerstrom (unidirektional, client-initiiert = Stream 2) und
        // schickt dort als erstes Frame SETTINGS.
        let mut steuerstrom = Vec::with_capacity(32);
        // N49: **`0x01`**, und vorher stand hier `0x00`. Nach RFC 9114 6.2.1
        // ist `0x01` der Steuerstrom; `0x00` ist nach 6.2.2 der **Push**-Strom,
        // und den darf nur der Client oeffnen. Wir haben also unseren
        // Steuerstrom als Push-Strom deklariert — und ein echter Server weist
        // so etwas zurueck. Siehe `h3::STREAM_TYP_STEUERSTROM`.
        steuerstrom.push(crate::h3::STREAM_TYP_STEUERSTROM as u8);
        //
        // GENAU EIN SETTINGS, nicht zwei. RFC 9114 Abschnitt 6.2.1: "Each
        // endpoint MUST NOT send more than one SETTINGS frame on its control
        // stream." Zwei sind ein Protokollverstoß; der Server antwortet mit
        // `H3_FRAME_UNEXPECTED` und schliesst (Abschnitt 7.2).
        //
        // DER GRUND FUER DEN KOMMENTAR: hier standen erst ein leeres
        // SETTINGS und danach ein zweites mit `MAX_FIELD_SECTION_SIZE`.
        // Beide einzeln sind gueltig, zusammen nicht.
        //
        // 0x06 = SETTINGS_MAX_FIELD_SECTION_SIZE (RFC 9114 7.2.4.1).
        h3_schreiben(
            &H3Rahmen::Einstellungen(vec![(0x06, 65536)]),
            &mut steuerstrom,
        )
        .map_err(|e| ClientFehler::H3(e.to_string()))?;

        let koepfe = vec![
            FeldZeile::neu(":method", "GET"),
            FeldZeile::neu(":scheme", "https"),
            FeldZeile::neu(":authority", &self.host),
            FeldZeile::neu(":path", &self.pfad),
            FeldZeile::neu("user-agent", "narsil/0.1"),
            FeldZeile::neu("accept", "text/html,*/*"),
        ];
        let mut anfrage = Vec::new();
        h3_schreiben(
            &H3Rahmen::Kopfblock(feldabschnitt_kodieren(&koepfe)),
            &mut anfrage,
        )
        .map_err(|e| ClientFehler::H3(e.to_string()))?;

        let mut payload = Vec::with_capacity(128);
        rahmen_schreiben(
            &Rahmen::Stream {
                stream_id: 2,
                offset: 0,
                daten: steuerstrom,
                fin: false,
            },
            &mut payload,
        )?;
        rahmen_schreiben(
            &Rahmen::Stream {
                stream_id: 0,
                offset: 0,
                daten: anfrage,
                fin: true,
            },
            &mut payload,
        )?;

        self.anfrage_nutzlast = payload.clone();
        // U2: die ungeschluesselte Anfrage auf Platte, damit sie byteweise mit
        // einer fremden Implementierung verglichen werden kann. Sie liegt in
        // `anfrage_nutzlast` als **Frame-Folge** (STREAM 2, dann STREAM 0) und
        // nicht als 1-RTT-Paket, weil die Paketnummer hier nichts zur Sache
        // tut.
        if let Some(ordner) = std::env::var_os("NARSIL_QUIC_DUMP") {
            let pfad = std::path::Path::new(&ordner).join("anfrage.bin");
            let _ = std::fs::create_dir_all(&ordner);
            let _ = std::fs::write(pfad, &payload);
        }
        self.sende_1rtt_nutzlast(&schluessel, &payload, "Steuerstrom+GET")
    }

    /// Schickt eine 1-RTT-Nutzlast in einem Paket mit der naechsten
    /// Paketnummer. Wird sowohl fuer die Anfrage als auch fuer ihre
    /// Wiederholung benutzt, damit beide Wege bytegleich sind.
    ///
    /// DER ZWEITE ORT DIESES FEHLERS (und der Grund fuer diese Funktion als
    /// eigene): der Short Header traegt die Laenge der Connection-ID nicht.
    /// Mit `&[]` waere jedes 1-RTT-Paket um `self.dcid.len()` Byte zu kurz,
    /// und der Server wuerde es verwerfen — ohne jede Fehlermeldung.
    fn sende_1rtt_nutzlast(
        &mut self,
        schluessel: &PaketSchluessel,
        payload: &[u8],
        was: &str,
    ) -> Result<(), ClientFehler> {
        let pn = self.naechste_pn[2];
        self.naechste_pn[2] = pn + 1;
        // EXPERIMENT (NARSIL_QUIC_PAD_1RTT=1): die 1-RTT-Nutzlast auf 1200 Byte
        // auffuellen. RFC 9000 Abschnitt 14.1 verlangt das nur fuer Initial;
        // manche Server reagieren aber auch auf kurze 1-RTT-Pakete nicht.
        // Der Schalter ist per Voreinstellung AUS, damit die Ausgabe nicht von
        // einer Server-Eigenheit abhaengt.
        let mut genutzt = payload.to_vec();
        if std::env::var_os("NARSIL_QUIC_PAD_1RTT").is_some() {
            while genutzt.len() < 1200 {
                genutzt.push(0x00);
            }
        }
        // DER PUNKT FUER DEN PING: eine Nutzlast von einem Byte (PING) oder
        // vier Byte (ACK) ist kuerzer als das Sample, das der Header-Schutz
        // braucht. Aufgefuellt wird deshalb HIER, beim Bau des Pakets, und
        // nicht in `kurzes_schuetzen`.
        let genutzt = crate::schutz::mit_auffuellung(&genutzt, 2);
        let paket = kurzes_schuetzen(schluessel, false, false, &self.dcid, pn, 2, &genutzt)?;
        // DIE ENTSCHEIDENDE GEGENPROBE. Die Frage, die den Abruf tot macht,
        // ist: **kann der Server unser 1-RTT-Paket ueberhaupt lesen?** Ohne
        // Schluessel stimmt es nicht; die Frage ist aber lokal entscheidbar —
        // wir haben beide Schluesselsaetze, und der Server wuerde genau
        // `entschuetzen(server_1rtt, paket, dcid_laenge=20)` aufrufen.
        //
        //
        // DIE GEGENPROBE, DIE ZWEI TAGE LANG GELOGEN HAT, DASS SIE NICHT
        // FUNKTIONIERT (01.10.2026, U2-Rest).
        //
        // Sie stand hier so:
        //
        //     if let Some(server) = self.eins_server.clone() {
        //         entschuetzen(&server, &paket, self.dcid.len(), 0)
        //
        // Das ist der **Server**-Schluessel an einem **Client**-Paket. Der
        // kann es nicht lesen, und die Ausgabe war konsequent und richtig:
        //
        //     [quic gegenprobe] unser 1-RTT-Paket ist NICHT lesbar:
        //                        AES-GCM: der Tag stimmt nicht
        //
        // Ich habe diese Zeile zwei Berichte lang als "gegenprobe" gefuehrt
        // und daraus geschlossen, unser Paket sei in Ordnung. Die Zeile hat
        // das Gegenteil bewiesen — naemlich gar nichts. Sie war so gebaut,
        // dass sie **nie** erfolgreich sein konnte, und das habe ich nicht
        // bemerkt, weil ich auf die Zeile statt auf die Vorbedingung geguckt
        // habe.
        //
        // RICHTIG IST DER **CLIENT**-Schluessel, denn das Paket ist von uns:
        //
        //     c ap traffic  ->  Paket, das der Client **sendet**  (RFC 9001 5)
        //     s ap traffic  ->  Paket, das der Server **sendet**
        //
        // Und die Paketnummer der Rekonstruktion ist unsere eigene `pn`, weil
        // `entschuetzen` sonst eine Nummer sucht, die bis zu `groesste + 1`
        // liegen muss (RFC 9000 Anhang A.3) — bei pn = 0 waere jede groesste
        // Zahl falsch, und die Rekonstruktion lieferte Unsinn.
        let _ = &self.eins_server; // bleibt fuer die Diagnose im Kommentar
        match crate::schutz::entschuetzen(schluessel, &paket, self.dcid.len(), pn) {
            Ok(g) => trace(
                "gegenprobe",
                &format!(
                    "TAUTOLOGIE, KEINE GEGENPROBE: mit dem CLIENT-Schluessel \
                     lesbar: pn={} (gesendet als {pn}), {} Byte Klartext",
                    g.paketnummer,
                    g.klartext.len()
                ),
            ),
            Err(e) => trace(
                "gegenprobe",
                &format!(
                    "unser eigenes 1-RTT-Paket ist NICHT lesbar: {e} \
                     -> der Server kann es auch nicht lesen. pn={pn}"
                ),
            ),
        }

        // N49: DIE RAHMEN, DIE WIR SENDEN — seit Runden nur die empfangenen.
        //
        // DER GRUND: In N49 liess sich nicht beantworten, **was** wir wirklich
        // auf die Leitung legen. Die Spur zeigte `[quic frames]` nur fuer
        // empfangene Pakete, und die Diagnose lief deshalb ueber das
        // Fehlerbild des Servers ("Received push stream") statt ueber unsere
        // eigenen Bytes. Genau die Luecke, die N48 auf der Empfangsseite
        // geschlossen hat — diese Seite war noch offen.
        let mut raus: Vec<String> = Vec::new();
        for r in rahmen_alle(payload).unwrap_or_default() {
            raus.push(rahmen_name(&r));
            // Beim STREAM zusaetzlich die **ersten 8 Byte des Inhalts**: bei
            // einem Steuerstrom steht dort der Stream-Typ, und genau der war
            // N49s streitige Gegenstand (0x00 = Push, 0x01 = Steuerstrom).
            if let Rahmen::Stream { daten, .. } = &r {
                raus.push(format!(
                    "erste {} Byte: {}",
                    daten.len().min(8),
                    narsil_tls::sha2::hexen(&daten[..daten.len().min(8)])
                ));
            }
        }
        trace(
            "sendung",
            &format!(
                "1-RTT {was} pn={pn}, {} Byte (erstes {:02x}), dcid={} ({} Byte)",
                paket.len(),
                paket[0],
                narsil_tls::sha2::hexen(&self.dcid),
                self.dcid.len()
            ),
        );
        trace("sendung", &format!("  wir senden: {}", raus.join(" | ")));
        self.sende(&paket)
            .map(|_| ())
            .map_err(|e| ClientFehler::Netz(e.to_string()))
    }

    /// Q7: Verteilung der Rechenzeit ausgeben. Nur mit Trace.
    fn melde_verteilung(&self, gesamt: std::time::Duration) {
        trace("zeit", &mess_zeile(self, gesamt));
    }

    fn lese_antwort(&mut self) -> Result<(), ClientFehler> {
        if self.eins_server.is_none() {
            return Err(ClientFehler::Handshake(
                "kein 1-RTT-Schluessel vom Server".into(),
            ));
        }
        self.setze_idle_frist();
        let gepuffert = self.puffer_eins.entleere();
        for paket in gepuffert {
            self.entschluessel_eins(&paket)?;
        }
        let mut pakete = 0usize;
        let mut pto = 0u32;
        while self.antwort.is_none() {
            if pakete >= MAX_ANTWORT_DATAGRAMME {
                return Err(ClientFehler::Zeitueberschreitung(
                    "keine vollstaendige Antwort im Datagrammlimit".into(),
                ));
            }
            self.pruefe_frist()?;
            match self.empfange()? {
                Empfang::Daten => {
                    pakete += 1;
                    self.bearbeite_eins_datagramm()?;
                    self.setze_idle_frist();
                }
                Empfang::Timeout => {
                    pto += 1;
                    if pto > MAX_PTO {
                        // U2-Rest: **bevor** der Fehler entsteht, die Zahl
                        // ausgeben, die die Frage beantwortet. Ein Fehler, der
                        // nur "Timeout" sagt, ist fuer die Diagnose wertlos;
                        // einer, der sagt "der Server hat uns im 1-RTT-Raum
                        // nie etwas quittiert", ist eine halbe Antwort.
                        trace(
                            "1rtt",
                            &format!(
                                "ABBRUCH: hoechste 1-RTT-Quittung des Servers = {:?} \
                                 (wir haben 1-RTT pn 0..{} gesendet); hoechste \
                                 Handshake-Quittung = {:?} (wir haben Handshake pn \
                                 0..{} gesendet)",
                                self.server_acks_groesste_1rtt,
                                self.naechste_pn[2].saturating_sub(1),
                                self.server_acks_groesste_handshake,
                                self.naechste_pn[1].saturating_sub(1)
                            ),
                        );
                        // N46/N48: DER ZUSTAND DES ANTWORTSTROMS, und er ist
                        // die Zahl, die die Frage beantwortet.
                        //
                        // DER GRUND: Die Spur zeigt nur die **letzten** 120
                        // Zeilen, und der Fehlerlag war bei 1 119 Zeilen. Damit
                        // sah ich 197 entschluesselte Server-Pakete, lauter
                        // ACKs, und kein einziges STREAM — und keine Ahnung,
                        // ob ueberhaupt eine Antwort **da** ist. Genau das ist
                        // die Luecke, an der der Fehler zum ersten Mal seit
                        // Tagen scheitert: die Spur beantwortet die Frage
                        // "was kam zuletzt", nicht "was ist der Stand".
                        //
                        // DESHALB STEHT HIER EINE ZUSTANDSZEILE: Byte auf
                        // Stream 0, ob das FIN kam, und wie weit der H3-Parser
                        // kam. `h3_pos != daten.len()` heisst: es fehlt ein
                        // **unvollstaendiger Frame** am Ende — und genau das ist
                        // ein Zustand, in dem `antwort_lesbar()` **nie** true
                        // wird.
                        let zustand = match self.streams.get(&0) {
                            Some(p) => format!(
                                "Stream 0: {} Byte, fin={}, h3_pos={} von {},                                  status={}, content-length={:?}, data={} Byte",
                                p.daten.len(),
                                p.fin,
                                p.h3_pos,
                                p.daten.len(),
                                p.h3_status,
                                p.h3_laenge,
                                p.h3_body
                            ),
                            None => "Stream 0: **gar nicht angelegt**".to_string(),
                        };
                        trace("1rtt", &format!("ZUSTAND: {zustand}"));
                        // N48: DIE BYTES AN DER VERKLEMMUNGSSTELLE.
                        //
                        // DER GRUND: Die Zustandszeile sagt, dass der Parser bei
                        // `h3_pos` stehenbleibt und 93 KB nicht liest, aber
                        // nicht, **warum**. Und `h3_lesen` scheitert an genau
                        // drei Stellen: ein unlesbares Varint, ein
                        // Laengenfeld, das ueber den Puffer hinausweist, oder
                        // zu wenig Byte fuer den Inhalt. Diese 48 Byte um
                        // `h3_pos` sagen es einem, statt dass man es rät.
                        //
                        // DIE GRENZE: 16 Byte davor und 64 danach. Genug, um
                        // den Rahmentyp und sein Laengenfeld zu sehen, und
                        // klein genug, um eine Spur lesbar zu halten.
                        if let Some(p) = self.streams.get(&0) {
                            const VOR: usize = 16;
                            const NACH: usize = 64;
                            let von = p.h3_pos.saturating_sub(VOR);
                            let bis = (p.h3_pos + NACH).min(p.daten.len());
                            if von < bis {
                                trace(
                                    "1rtt",
                                    &format!(
                                        "BYTES um h3_pos={}: [{}] ... [{}]",
                                        p.h3_pos,
                                        narsil_tls::sha2::hexen(
                                            &p.daten[von..p.h3_pos.min(p.daten.len())]
                                        ),
                                        narsil_tls::sha2::hexen(
                                            &p.daten[p.h3_pos.min(p.daten.len())..bis]
                                        )
                                    ),
                                );
                            }
                        }
                        return Err(ClientFehler::Zeitueberschreitung(format!(
                            "keine Antwort nach dem Idle-Zeitlimit. {zustand}"
                        )));
                    }
                    self.setze_idle_frist();
                    trace(
                        "1rtt",
                        &format!(
                            "PTO {pto}, {} 1-RTT-Pakete vom Server",
                            self.eins_gekommen()
                        ),
                    );
                    // ZUERST die Anfrage wiederholen, DANN pingen. Die
                    // Reihenfolge ist nicht kosmetisch: ein Ping weckt den
                    // Server auf, gibt ihm aber nichts zu beantworten — und
                    // er beantwortet nur, was er bekommen hat.
                    // N28, ZUERST das Finished: solange der Server es nicht
                    // hat, ist jede 1-RTT-Wiederholung fuer ihn ein Paket, das
                    // er nach RFC 9000 7.1 nicht bearbeiten darf.
                    self.wiederhole_client_finished()?;
                    // Q7: DIE ANFRAGE WIRD NUR WIEDERHOLT, SOLANGE NICHTS DA IST.
                    //
                    // GEMESSEN vor dieser Aenderung: bei 1,3 MB wurden die
                    // Anfrage **etwa 15-mal** wiederholt, waehrend der Server
                    // die Antwort bereits schickte. Der PTO feuert, weil der
                    // Lesevorgang 250 ms kein Byte sieht — das ist bei einem
                    // grossen Body normal und **kein** Verlustzeichen.
                    //
                    // DER BEWEIS, DASS WIR SIE HATten: `antwort.is_some()`
                    // heisst, dass mindestens ein STREAM-Byte auf Stream 0 da
                    // ist. Wer daraufhin noch einmal anfragt, fragt einen
                    // Server, der gerade antwortet — das ist Rauschen auf der
                    // Leitung und Arbeit bei ihm.
                    if self.antwort.is_none() {
                        self.wiederhole_anfrage()?;
                    }
                    self.sende_1rtt_ping()?;
                }
                Empfang::StatelessReset => return Err(ClientFehler::StatelessReset),
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Netz
    // -----------------------------------------------------------------------

    fn pruefe_frist(&self) -> Result<(), ClientFehler> {
        if Instant::now() >= self.frist {
            return Err(ClientFehler::Zeitueberschreitung(
                "Gesamtzeitlimit ueberschritten".into(),
            ));
        }
        Ok(())
    }

    /// Ein Paket an `ziel` schicken (N43).
    ///
    /// Der Umweg ueber einen Helfer statt `socket.send` ist der ganze Fix: ein
    /// **verbundenes** Sockel kann nur an den verbundenen Peer senden, und
    /// `send` haette genau das getan. Mit `send_to` sagen wir die Adresse bei
    /// jedem Paket dazu, und das Sockel bleibt offen fuer Quellenwechsel.
    fn sende(&self, paket: &[u8]) -> Result<(), std::io::Error> {
        // Q7.2: DIE SENDEMESSUNG. `send_to` ist der einzige Syscall, der
        // **blockieren** kann: ist der Sende-Puffer gefuellt, wartet er, bis der
        // Kernel Platz hat, und das ist Warten — aber **nicht** Warten auf
        // Netz.
        //
        // `&self` und trotzdem Schreiben: deshalb `Cell` statt eines Feldes.
        // `Cell` ist fuer genau diesen Fall gemacht (kein `&mut` noetig), und
        // es steht in `std::cell`, nicht in einer Fremd-Crate.
        //
        // M3: `gewartet` kommt aus einer Uhr, nicht aus fremden Daten; es gibt
        // keinen `unwrap`, und `saturating_add` verhindert den Ueberlauf bei
        // einer langen Sitzung.
        let t = std::time::Instant::now();
        let r = self.socket.send_to(paket, self.ziel).map(|_| ());
        let gewartet = t.elapsed().as_nanos();
        self.sendezeit
            .set(self.sendezeit.get().saturating_add(gewartet));
        self.sendungen.set(self.sendungen.get().saturating_add(1));
        r
    }

    /// Liest genau ein Datagramm. `Timeout` ist **kein** Fehler: er ist das
    /// Signal fuer den PTO. `StatelessReset` wird VOR jeder Entschluesselung
    /// erkannt (RFC 9000 Abschnitt 10.3), weil ein Reset per Definition nicht
    /// entschluesselt werden kann.
    fn empfange(&mut self) -> Result<Empfang, ClientFehler> {
        let mut puffer = vec![0u8; MAX_DATAGRAMM];
        // Q7.2: DER ZEITPUNKT VOR DEM WARTEN. Ohne ihn ist die Wartezeit nicht
        // von der Rechenzeit zu trennen, und „davon Netz" bleibt eine Zahl ohne
        // Aussage.
        //
        // DER PREIS: **eine** `Instant::now()` je `recv_from` — im Gegensatz
        // zu den Phasenzeitern, die an `NARSIL_QUIC_TRACE` haengen. Der
        // Unterschied ist begruendet: die Phasenzeiter summieren sich zu
        // Mikrosekunden und wuerden die Messung selbst verschmutzen, wenn sie
        // immer liefen. Der Wartezeit-Messer kostet einmal pro **Paket** (121
        // im gemessenen Lauf), und die Zeit, die er misst, ist um Groessenordnungen
        // groesser als sein eigener Aufwand. Ohne diese Begruendung waere
        // `mess_start` auch hier die richtige Wahl — und dann saehe man nichts.
        let t_warte = std::time::Instant::now();
        // N43: `recv_from` statt `recv`. Auf einem verbundenen Sockel gaebe es
        // kein `von` — und **genau darin** liegt der Fehler, den wir nicht
        // sehen konnten: der Kernel haette die Datagramme einer anderen Quelle
        // verworfen, bevor dieser Aufruf sie erreicht.
        let (n, von) = match self.socket.recv_from(&mut puffer) {
            // `recv_from` liefert bei 0 **kein** Adresspaar, deshalb greift
            // der Zweig ueber `n == 0` weiter unten statt ueber `Ok(0)`.
            Ok(x) => x,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                // Q7.2: Ein Timeout ist **Warten auf nichts** und wird deshalb
                // nicht in `wartezeit_gesamt` gezaehlt. Sonst wuerde ein
                // Zeitlimit, das am Ende greift, als „Netz" dastehen — und
                // genau das hat in N48 den Idle-Zeitlimit als Netzproblem
                // aussehen lassen.
                return Ok(Empfang::Timeout);
            }
            Err(e) => return Err(ClientFehler::Netz(e.to_string())),
        };
        // Q7.2: Die Wartezeit dieses Pakets.
        let gewartet = t_warte.elapsed().as_nanos();
        self.wartezeit_gesamt = self.wartezeit_gesamt.saturating_add(gewartet);
        self.wartezeit_max = self.wartezeit_max.max(gewartet);
        self.wartezeit_echt = self.wartezeit_echt.saturating_add(1);
        self.wartezeit_letzte = gewartet;
        // Q7.2: DIE GROSSTE WARTEZEIT **mit Ort**, und das ist der Unterschied
        // zwischen einer Zahl und einer Diagnose.
        //
        // GEMESSEN (Debug, cloudflare-quic.com, 3 Laeufe): die groesste
        // Einzelwartezeit liegt bei 78, 83 und 34 ms bei je 120 bis 125
        // Paketen. Also kommen die meisten Pakete sofort, und **eine** Stelle
        // wartet lang.
        //
        // DIE SCHRANZE: nur Wartezeiten ab **einer Millisekunde** kommen in die
        // Spur. Ohne sie waeren es 121 Zeilen, davon 120 mit „0 ms" — und das
        // ist genau die Form von Ausgabe, bei der man die eine wichtige Zeile
        // uebersieht.
        if gewartet >= 1_000_000 {
            trace(
                "wartezeit",
                &format!(
                    "Paket {} nach {} ms ({} Byte)",
                    self.pakete_gelesen.saturating_add(1),
                    gewartet / 1_000_000,
                    n
                ),
            );
        }
        // Eine leere UDP-Nutzlast (0 Byte) ist kein Timeout, sondern eine
        // leere Nachricht — `recv` wuerde sie als Timeout melden, `recv_from`
        // nicht. Also wird hier der alte Sinn wiederhergestellt.
        if n == 0 {
            return Ok(Empfang::Timeout);
        }
        // DIE QUELLE ZAEHLEN, und das ist der Punkt von N43: eine andere
        // Quelladresse ist nach RFC 9000 8.1/9.2 ein **erlaubtes** Ereignis
        // (Address Migration), kein Fehler. Sie sichtbar zu machen ist der
        // Unterschied zwischen "das Paket kam nicht" und "das Paket kam aus
        // einer anderen Ecke".
        //
        // M7: eine Zaehlschranke fuer fremde Ereignisse, mit Namen.
        const MAX_QUELLWECHSEL: u32 = 64;
        if von != self.ziel {
            self.quelle_gewechselt = self.quelle_gewechselt.saturating_add(1);
            trace(
                "empfang",
                &format!(
                    "QUELLWECHSEL {}/{}: von {} (erwartet {})",
                    self.quelle_gewechselt, MAX_QUELLWECHSEL, von, self.ziel
                ),
            );
            if self.quelle_gewechselt > MAX_QUELLWECHSEL {
                return Err(ClientFehler::Handshake(format!(
                    "mehr als {MAX_QUELLWECHSEL} Quellwechsel — das ist kein Server mehr"
                )));
            }
            // Und die **Sende**adresse folgt: ein Server, der migriert, will
            // die Antworten an die neue Adresse. Wir senden weiter an `ziel`,
            // weil QUIC die Connection-ID und nicht die IP-Adresse als
            // Adressbezug nutzt — das ist Absicht, keine Nachlaessigkeit, und
            // steht hier, damit sie niemand als Versehen liest.
        }
        let datagramm = &puffer[..n];
        if self.ist_stateless_reset(datagramm) {
            trace("empfang", "Stateless Reset erkannt");
            return Ok(Empfang::StatelessReset);
        }
        // Q1: VERSIONSVERHANDLUNG, und sie muss **vor** allem anderen erkannt
        // werden. Das Paket ist ein Long Header mit Versionsfeld 0, es ist
        // **nicht verschluesselt**, und es beantwortet die Frage "kann der
        // Server ueberhaupt?".
        //
        // OHNE DIESE ZEILE fiel es in `zerlegen` hinein, wurde als Initial
        // eingeordnet, scheiterte an der Entschluesselung und lief bis zum
        // Zeitlimit. Die Meldung war dann `keine Antwort nach dem
        // Idle-Zeitlimit` — und man haelt einen Server, der sich gerade
        // **offiziell** fuer die Version entschieden hat, fuer stumm.
        if crate::kopf::ist_versionsverhandlung(datagramm) {
            let angeboten = crate::kopf::versionen_lesen(datagramm).unwrap_or_default();
            trace(
                "version",
                &format!(
                    "Versionsverhandlung: angeboten {}",
                    liste_versionen(&angeboten)
                ),
            );
            return Err(ClientFehler::Versionsverhandlung { angeboten });
        }
        trace("empfang", &format!("{n} Byte, erstes {:02x}", datagramm[0]));
        self.pakete_gelesen = self.pakete_gelesen.saturating_add(1);
        self.letztes = datagramm.to_vec();
        Ok(Empfang::Daten)
    }

    /// Vergleicht die letzten 16 Byte mit allen bekannten Reset-Token
    /// (RFC 9000 Abschnitt 10.3.1). Die Vergleichslogik steht in
    /// `token_steht_in_daten_mehrere`, damit Client und Test dieselbe nutzen.
    fn ist_stateless_reset(&self, datagramm: &[u8]) -> bool {
        token_steht_in_daten_mehrere(&self.reset_token, datagramm)
    }

    // -----------------------------------------------------------------------
    // Senden
    // -----------------------------------------------------------------------

    fn berechne_initial_schluessel(&mut self) -> Result<(), ClientFehler> {
        let initial = initial_geheimnis(&self.dcid)?;
        self.initial_client = schluessel_ableiten(
            &richtungs_geheimnis(&initial, b"client in")?,
            Suite::Aes128Gcm,
        )?;
        self.initial_server = schluessel_ableiten(
            &richtungs_geheimnis(&initial, b"server in")?,
            Suite::Aes128Gcm,
        )?;
        Ok(())
    }

    fn baut_clienthello(&self, oeffentlich: &[u8; 32]) -> Result<Vec<u8>, ClientFehler> {
        let random =
            narsil_tls::zufall::bytes(32).map_err(|e| ClientFehler::Handshake(e.to_string()))?;
        let mut zufall = [0u8; 32];
        zufall.copy_from_slice(&random);
        let mut ch = ClientHello::baue(
            zufall,
            Some(self.host.as_bytes()),
            &[b"h3"],
            &self.geheim,
            oeffentlich,
        )
        .map_err(|e| ClientFehler::Handshake(e.to_string()))?;
        // `ClientHello::suites_standard()` nennt [0x1301, 0x1303], beides
        // SHA-256-Suites. 0x1302 (TLS_AES_256_GCM_SHA384) wird hier
        // **nicht** angeboten: der Schluesselplan aus narsil-tls rechnet
        // SHA-256, und ein 32-Byte-Schluessel mit SHA-384-Transcript waere
        // eine falsch abgeleitete Verschluesselung statt eines klaren
        // "diese Suite kann ich nicht".
        let mut parameter = Vec::new();
        parameter_schreiben(
            &[
                Parameter::MaxIdleTimeout(EIGENER_IDLE_MS),
                Parameter::MaxUdpPayloadSize(65527),
                // U2: die Zahlen sind benannt und stehen in `FENSTER_START`.
                // Vorher stand hier `1 << 20` und `1 << 16` als Zahlen im
                // Aufruf, ohne dass irgendwo stand, dass sie mit dem Code
                // weiter unten (der Nachfuellung) zusammengehoeren.
                Parameter::InitialMaxData(FENSTER_START),
                Parameter::InitialMaxStreamDataBidiLocal(FENSTER_START),
                Parameter::InitialMaxStreamDataBidiRemote(FENSTER_START),
                Parameter::InitialMaxStreamDataUni(FENSTER_START),
                Parameter::InitialMaxStreamsBidi(MAX_OFFENE_STROEME as u64),
                Parameter::InitialMaxStreamsUni(MAX_OFFENE_STROEME as u64),
                Parameter::ActiveConnectionIdLimit(4),
                Parameter::InitialSourceCid(self.scid.clone()),
            ],
            &mut parameter,
        )
        .map_err(|e| ClientFehler::Handshake(e.to_string()))?;
        // N44: Die Laenge merken, **bevor** der Block verschoben wird — siehe
        // die Fehlermeldung "borrow of moved value", die mich das hier
        // gekostet hat.
        let parameter_block = parameter.len();
        ch.quic_transport_parameters = Some(parameter);
        // N44: WAS WIR ANBIETEN, in die Spur.
        //
        // DER GRUND: `www.wikipedia.org`, `github.com` und `example.org`
        // antworten auf unser Initial **nicht** (Q3), cloudflare.com,
        // youtube.com, facebook.com, google.com, nginx und
        // cloudflare-quic.com antworten. Dieselbe Maschine, dieselbe Stunde,
        // dieselbe IP — also liegt es an **unseren** Bytes, und die Frage
        // "welche" ist mit einem Blick auf diese Zeile beantwortbar und sonst
        // nicht.
        trace(
            "hello",
            &format!(
                concat!(
                    "ClientHello: versionen {:?}, gruppen {:?}, alpn {:?}, suites {:?}, ",
                    "signaturalgorithmen {:?}, psk_modi {:?}, ",
                    "transportparameter {} Byte"
                ),
                ch.versionen,
                ch.gruppen,
                ch.alpn,
                ch.cipher_suites,
                ch.signaturalgorithmen,
                ch.psk_modi,
                parameter_block
            ),
        );
        Ok(ch.kodiere())
    }

    /// Initial mit dem vollstaendigen ClientHello, auf `MIN_DATAGRAMM`
    /// aufgefuellt (RFC 9000 Abschnitt 14.1).
    fn sende_initial(&mut self, pn: u64) -> Result<(), ClientFehler> {
        let mut payload = Vec::with_capacity(self.clienthello.len() + 8);
        rahmen_schreiben(
            &Rahmen::Crypto {
                offset: 0,
                daten: self.clienthello.clone(),
            },
            &mut payload,
        )?;
        let paket = lang_paket_gefuellt(
            &self.initial_client,
            TYP_INITIAL,
            &self.dcid,
            &self.scid,
            &[],
            pn,
            2,
            &payload,
            MIN_DATAGRAMM,
        )?;
        // Q5: UNSER EIGENES INITIAL VOR SEINER VORHELLUNG ANSEHEN.
        //
        // DER GRUND: N44 zeigt, dass `www.wikipedia.org`, `github.com` und
        // `example.org` auf unser Initial **nicht** antworten, waehrend dieselbe
        // IP curl sofort antwortet. Bis eben hatte ich **keine** Zahl und **kein
        // Bild** von dem, was wir hinschicken — nur die Annahme, es sehe "wie
        // die anderen".
        //
        // Q5 nennt vier Verdachtsstellen, und jede steht in dieser Zeile:
        //   * **Padding 1200** -> `{}` Byte gegen `MIN_DATAGRAMM`, und der Long
        //     Header laesst sich lesen: Laenge-Feld, CIDs, Paketnummer.
        //   * **Version**     -> im Kopf; `lange_lesen` gibt sie heraus.
        //   * **Token**       -> steht im Kopf, `lange_lesen` liest es.
        //   * **Krypto-Frames** -> `rahmen_alle(&payload)` auf dem
        //     **Klartext**, also vor der Verschluesselung.
        if let Ok(k) = crate::kopf::lange_lesen(&paket) {
            let mut frames = Vec::new();
            for r in rahmen_alle(&payload).unwrap_or_default() {
                if let Rahmen::Crypto { offset, daten } = r {
                    frames.push(format!("CRYPTO(offset {offset}, {} Byte)", daten.len()));
                }
            }
            trace(
                "initial",
                &format!(
                    concat!(
                        "INITIAL {} Byte (Ziel {}), version {:08x}, dcid {} Byte, scid {} Byte, ",
                        "token {} Byte, laenge_feld {}, pn_offset {}, kopf {} Byte, ",
                        "ClientHello {} Byte, Rahmen: {}"
                    ),
                    paket.len(),
                    MIN_DATAGRAMM,
                    k.version,
                    k.dcid.len(),
                    k.scid.len(),
                    k.token.len(),
                    k.laenge,
                    k.pn_offset,
                    k.header_laenge,
                    self.clienthello.len(),
                    frames.join(", ")
                ),
            );
        }
        self.sende(&paket)
            .map(|_| ())
            .map_err(|e| ClientFehler::Netz(e.to_string()))?;
        self.naechste_pn[0] = self.naechste_pn[0].max(pn + 1);
        Ok(())
    }

    fn sende_initial_ack(&mut self) -> Result<(), ClientFehler> {
        if self.initial_seen.groesste.is_none() {
            return Ok(());
        }
        let pn = self.naechste_pn[0];
        self.naechste_pn[0] = pn + 1;
        let payload = ack_payload(&self.initial_seen);
        // Auch dieses Datagramm wird auf 1200 Byte gebracht: es erhoeht das
        // Anti-Verstaerkungsbudget des Servers, ohne das er die Kette nicht
        // senden darf (RFC 9000 Abschnitt 8.1).
        let paket = lang_paket_gefuellt(
            &self.initial_client,
            TYP_INITIAL,
            &self.dcid,
            &self.scid,
            &[],
            pn,
            2,
            &payload,
            MIN_DATAGRAMM,
        )?;
        self.sende(&paket)
            .map(|_| ())
            .map_err(|e| ClientFehler::Netz(e.to_string()))
    }

    fn sende_handshake_ping(&mut self) -> Result<(), ClientFehler> {
        let schluessel = self
            .handshake_client
            .clone()
            .ok_or_else(|| ClientFehler::Handshake("kein Handshake-Schluessel".into()))?;
        let pn = self.naechste_pn[1];
        self.naechste_pn[1] = pn + 1;
        let mut payload = ack_payload(&self.handshake_seen);
        // ACK allein ist nicht ack-ausloesend; ohne PING gibt es keine
        // Antwort und der PTO laeuft endlos.
        rahmen_schreiben(&Rahmen::Ping, &mut payload)?;
        let paket = lang_paket_gefuellt(
            &schluessel,
            TYP_HANDSHAKE,
            &self.dcid,
            // N43: **nicht `&[]`**. Das war hier ueberall so — und es ist eine
            // **Inkonsistenz in unseren eigenen Paketen**: das Initial traegt
            // unsere 8-Byte-SCID, das Handshake-Paket keine.
            //
            // GEMESSEN, und der Unterschied ist nicht theoretisch:
            //   cloudflare-quic.com  Initial 8 Byte SCID, Handshake 0 Byte
            //                        -> antwortet mit STATUS 200
            //   www.google.com       Initial 8 Byte SCID, Handshake 0 Byte
            //   nginx                Initial 8 Byte SCID, Handshake 0 Byte
            //                        -> beide quittieren unser Initial (ACK
            //                        bis 0) und schicken danach **Handshake-
            //                        PINGs** (nginx, pn=4..7): sie warten auf
            //                        unsere Handshake-Pakete und bekommen sie
            //                        nicht.
            //
            // DIE REGEL (RFC 9000 7.2): Die Source Connection ID eines Endpunkts
            // bleibt ueber die Verbindung **gleich**. Der Server benutzt sie als
            // zweiten Schluessel beim Demultiplexen — ein Paket, dessen SCID
            // nicht zum Datensatz passt, gehoert fuer ihn nicht zu dieser
            // Verbindung, und Cloudflare ist offenbar tolerant, die anderen
            // nicht.
            &self.scid,
            &[],
            pn,
            2,
            &payload,
            0,
        )?;
        trace(
            "sendung",
            &format!("Handshake ACK+PING pn={pn}, {} Byte", paket.len()),
        );
        self.sende(&paket)
            .map(|_| ())
            .map_err(|e| ClientFehler::Netz(e.to_string()))
    }

    fn sende_1rtt_ping(&mut self) -> Result<(), ClientFehler> {
        let schluessel = self
            .eins_client
            .clone()
            .ok_or_else(|| ClientFehler::Handshake("kein 1-RTT-Schluessel".into()))?;
        let pn = self.naechste_pn[2];
        self.naechste_pn[2] = pn + 1;
        let mut payload = Vec::with_capacity(8);
        rahmen_schreiben(&Rahmen::Ping, &mut payload)?;
        // DER PUNKT, AN DEM ES ZWEIMAL GEHATEN HAT: ein PING-Frame ist **ein**
        // Byte. Mit einer 2-Byte-Paketnummer braucht das Header-Schutz-Sample
        // aber 18 Byte Chiffrat, also 2 Byte Klartext. Ohne die Auffuellung
        // kam hier "Paket braucht 18 Byte, da sind 17" heraus — und das war
        // kein Serverfehler, sondern unser eigenes zu kurzes Paket.
        //
        // `mit_auffuellung` steht bewusst NICHT in `kurzes_schuetzen`: eine
        // Protect-Funktion darf den Klartext nicht veraendern. Hier, wo die
        // Nutzlast entsteht, ist der richtige Ort.
        let genutzt = crate::schutz::mit_auffuellung(&payload, 2);
        let paket = kurzes_schuetzen(&schluessel, false, false, &self.dcid, pn, 2, &genutzt)?;
        self.sende(&paket)
            .map(|_| ())
            .map_err(|e| ClientFehler::Netz(e.to_string()))
    }

    fn sende_1rtt_ack(&mut self) -> Result<(), ClientFehler> {
        let schluessel = match self.eins_client.clone() {
            Some(k) => k,
            None => return Ok(()),
        };
        if self.eins_seen.benoetigt_kein_ack() {
            return Ok(());
        }
        let pn = self.naechste_pn[2];
        self.naechste_pn[2] = pn + 1;
        let payload = crate::schutz::mit_auffuellung(&ack_payload(&self.eins_seen), 2);
        // Q7: dasselbe wie beim Handshake-ACK.
        self.eins_seen.entleere();
        let paket = kurzes_schuetzen(&schluessel, false, false, &self.dcid, pn, 2, &payload)?;
        self.sende(&paket)
            .map(|_| ())
            .map_err(|e| ClientFehler::Netz(e.to_string()))
    }

    // -----------------------------------------------------------------------
    // Empfang: Zerlegen und Entschluesseln
    // -----------------------------------------------------------------------

    /// Phase 1: ein Datagramm mit Initial-Paketen.
    fn bearbeite_initial(&mut self) -> Result<(), ClientFehler> {
        // N40: **EXAKT** 0xf0 (RETRY_ERSTES_BYTE), und vorher
        // `& 0xf0 == 0xf0`.
        //
        // RFC 9000 Tabelle 3 nennt fuer Retry `1 1 1 0 0 0 0 0`. Die unteren
        // vier Bit sind **unbenutzt und null** — sie sind nicht "egal". Mit der
        // Maske hat der Client 0xf0 bis 0xff als Retry behandelt.
        //
        // **Kein angreifbarer Fehler:** der Integritaetstag wird in
        // `bearbeite_retry` geprueft, ein falscher Retry scheitert dort. Es ist
        // aber eine **Annahme ueber fremde Bytes, die nicht stimmt** — und
        // `retry_zerlegen` hat den Typ ueberhaupt nicht geprueft. Beides ist in
        // N40 behoben.
        if self.letztes[0] == crate::client::RETRY_ERSTES_BYTE {
            return self.bearbeite_retry();
        }
        // Das Datagramm wird kopiert, BEVOR die Schleife laeuft: die Teile
        // leihen sich aus dem Puffer, und `entschluessel_*` braucht
        // `&mut self`. Ohne die eigene Kopie wuerde der Puffer mitten in der
        // Schleife freigegeben.
        let kopie = self.letztes.clone();
        let teile = zerlegen(&kopie)?;
        trace("initial", &format!("{} Teile", teile.len()));
        for (ebene, teil) in teile {
            trace("initial", &format!("  {ebene:?} {} Byte", teil.len()));
            match ebene {
                Ebene::Initial => {
                    let schluessel = self.initial_server.clone();
                    self.entschluessel_ebene(Ebene::Initial, &schluessel, teil)?;
                }
                Ebene::Handshake => self.puffer_handshake.push(teil.to_vec()),
                Ebene::EinsRtt => self.puffer_eins.push(teil.to_vec()),
            }
        }
        self.uebernehmen_serverhello_wenn_da()?;
        Ok(())
    }

    /// Phase 2: ein Datagramm mit Handshake-Paketen.
    fn bearbeite_handshake_datagramm(&mut self) -> Result<(), ClientFehler> {
        if self.letztes[0] == crate::client::RETRY_ERSTES_BYTE {
            return Ok(());
        }
        // Das Datagramm wird kopiert, BEVOR die Schleife laeuft: die Teile
        // leihen sich aus dem Puffer, und `entschluessel_*` braucht
        // `&mut self`. Ohne die eigene Kopie wuerde der Puffer mitten in der
        // Schleife freigegeben.
        let kopie = self.letztes.clone();
        let teile = zerlegen(&kopie)?;
        trace("handshake", &format!("{} Teile", teile.len()));
        for (ebene, teil) in teile {
            trace("handshake", &format!("  {ebene:?} {} Byte", teil.len()));
            match ebene {
                Ebene::Handshake => self.entschluessel_handshake(teil)?,
                Ebene::EinsRtt => self.puffer_eins.push(teil.to_vec()),
                Ebene::Initial => {}
            }
        }
        Ok(())
    }

    /// Phase 3: ein Datagramm mit 1-RTT-Paketen.
    ///
    /// DER GRUND FUER DEN EIGENEN AUFRUF statt `entschluessel_ebene`: auf
    /// der 1-RTT-Ebene tragen STREAM- und NEW_CONNECTION_ID-Frames echte
    /// Bedeutung, und die gibt es im gemeinsamen Weg nicht.
    fn bearbeite_eins_datagramm(&mut self) -> Result<(), ClientFehler> {
        if self.letztes[0] == crate::client::RETRY_ERSTES_BYTE {
            return Ok(());
        }
        // Das Datagramm wird kopiert, BEVOR die Schleife laeuft: die Teile
        // leihen sich aus dem Puffer, und `entschluessel_*` braucht
        // `&mut self`. Ohne die eigene Kopie wuerde der Puffer mitten in der
        // Schleife freigegeben.
        let kopie = self.letztes.clone();
        let teile = zerlegen(&kopie)?;
        trace("1rtt", &format!("{} Teile", teile.len()));
        // Q7: Merken, ob in diesem Datagramm ueberhaupt ein **Handshake**-Paket
        // lag. Ohne diese Merkung wird der Handshake-ACK nach **jedem**
        // 1-RTT-Datagramm geschickt — Q7 hat das bei 1,3 MB mit **1 209**
        // Handshake-ACKs gemessen, die niemand gebraucht hat. Der ACK gehoert
        // in den Nummernraum des Pakets, das er quittiert (RFC 9000 13.2.1).
        let mut handshake_dabei = false;
        for (ebene, teil) in teile {
            trace("1rtt", &format!("  {ebene:?} {} Byte", teil.len()));
            match ebene {
                Ebene::EinsRtt => self.entschluessel_eins(teil)?,
                Ebene::Handshake => {
                    handshake_dabei = true;
                    self.entschluessel_handshake(teil)?
                }
                Ebene::Initial => {}
            }
        }
        if handshake_dabei {
            self.sende_handshake_ack()?;
        }
        if self.antwort.is_none() {
            self.sende_1rtt_ack()?;
        }
        Ok(())
    }

    /// Ein Paket einer beliebigen Ebene entschluesseln und seine Frames
    /// einsortieren. Jede Ebene nimmt **ihre** Schluessel (Q6a) und **ihren**
    /// Nummernraum.
    fn entschluessel_ebene(
        &mut self,
        ebene: Ebene,
        schluessel: &PaketSchluessel,
        paket: &[u8],
    ) -> Result<(), ClientFehler> {
        if paket.is_empty() {
            return Ok(());
        }
        // Die Connection-ID-Laenge des Short Headers steht nicht im Paket,
        // sondern im Zustand — und zwar ist sie die Laenge der **eigenen**
        // SCID, weil ein 1-RTT-Paket vom Server an uns gerichtet ist
        // (RFC 9000 Abschnitt 7.2). Beim Senden wird die Server-SCID
        // eingesetzt, beim Empfangen die eigene; die beiden Werte unterscheiden
        // sich bei Cloudflare (20 gegen 8 Byte).
        let dcid_laenge = if ebene == Ebene::EinsRtt {
            self.scid.len()
        } else {
            0
        };
        let t_start = mess_start();
        let gelesen = match entschuetzen(
            schluessel,
            paket,
            dcid_laenge,
            self.verfolger(ebene).groesste.unwrap_or(0),
        ) {
            Ok(g) => g,
            // Ein Paket, das nicht aufgeht, beendet die Verbindung nicht:
            // es kann ein Duplikat mit zu kleiner Paketnummer sein (RFC 9000
            // 5.5) oder aus einer Schluesselphase, die schon abgeschlossen
            // ist. Beides ist normal.
            Err(e) => {
                // DER GRUND FUER DEN TRACE: "Paket braucht 18 Byte, da sind 17"
                // allein sagt nicht, OB ES EIN 1-RTT-PAKET WAR und ob die
                // Connection-ID-Laenge stimmt. Genau das war hier der Fehler.
                trace(
                    "entschlüsseln",
                    &format!("{ebene:?} {} Byte: {e}", paket.len()),
                );
                return Ok(());
            }
        };
        // Q7: die Rechnung, die vorher niemand gemacht hat.
        self.zeit_entschluesseln += mess_seit(t_start);
        self.pakete_entschluesselt = self.pakete_entschluesselt.saturating_add(1);
        trace(
            "entschlüsseln",
            &format!(
                "{ebene:?} pn={} {} Byte Klartext",
                gelesen.paketnummer,
                gelesen.klartext.len()
            ),
        );
        self.verfolger_mut(ebene).push(gelesen.paketnummer);
        // U2-DIAGNOSE (01.10.2026): die ersten 96 Byte des Klartexts als Hex.
        // Nur mit NARSIL_QUIC_TRACE=1. Damit ist ohne Umweg sichtbar, welche
        // STREAM-Nummer der Server fuer den **Antwortkopf** benutzt — die
        // Frame-Liste allein laesst offen, ob der Kopf im selben Paket stand
        // und beim Parsen verloren ging.
        //
        // (Wiederhergestellt am 01.10.2026, N23: beim Einfuegen des
        // Handshake-Quittungszaehlers ist der Code unter diesem Kommentar
        // verloren gegangen. Ein Kommentar, der Code beschreibt, den es nicht
        // gibt, ist schlimmer als gar keiner — er sieht nach einer
        // Beschaeftigung aus, die niemand ausgefuehrt hat.)
        trace(
            "klartext",
            &format!(
                "{ebene:?} pn={} roh: {}",
                gelesen.paketnummer,
                narsil_tls::sha2::hexen(&gelesen.klartext[..gelesen.klartext.len().min(96)])
            ),
        );
        // U2-Rest (N23): die Quittungen des Servers auf der **Handshake**-
        // Ebene merken.
        //
        // DER GRUND, und er ist eine Luecke, die ich in N8-N20 uebersehen
        // habe: der Server **wiederholt** seine Handshake-CRYPTO, und
        // Wiederholung heisst nach RFC 9000 19.3 "ich habe dein Paket nicht".
        // Ich hatte diese Ebene nur mit der Gegenprobe geprueft (lesbar) —
        // nie ihre **Quittung**. Lesbar und quittiert sind zwei verschiedene
        // Fragen, und nur die zweite sagt, ob beim Server etwas angekommen ist.
        //
        // Die 1-RTT-Zahl in `server_acks_groesste_1rtt` beantwortet sie nicht:
        // sie ist None auch dann, wenn der Server alles bis 1-RTT bekommen
        // hat und nur auf dieser Ebene schweigt.
        let rahmen_ak = rahmen_alle(&gelesen.klartext)?;
        if ebene == Ebene::Handshake {
            for frame in &rahmen_ak {
                if let Rahmen::Ack { groesste, .. } = frame {
                    self.server_acks_groesste_handshake = Some(
                        self.server_acks_groesste_handshake
                            .map_or(*groesste, |g: u64| g.max(*groesste)),
                    );
                }
            }
        }
        let rahmen = rahmen_alle(&gelesen.klartext)?;
        trace(
            "frames",
            &format!(
                "{ebene:?} pn={}: {}",
                gelesen.paketnummer,
                rahmen
                    .iter()
                    .map(rahmen_name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
        for frame in &rahmen {
            match frame {
                Rahmen::Crypto { offset, daten } => {
                    let ziel = if ebene == Ebene::Initial {
                        &mut self.crypto_initial
                    } else {
                        &mut self.crypto_handshake
                    };
                    ziel.einfuegen(*offset, daten)?;
                }
                Rahmen::ConnectionClose {
                    transport,
                    fehlercode,
                    grund,
                    ..
                } => return Err(geschlossen(*fehlercode, *transport, grund)),
                _ => {}
            }
        }
        if ebene == Ebene::Initial {
            self.uebernehmen_serverhello_wenn_da()?;
        } else {
            self.verarbeite_tls_nachrichten()?;
        }
        Ok(())
    }

    fn entschluessel_handshake(&mut self, paket: &[u8]) -> Result<(), ClientFehler> {
        let schluessel = match self.handshake_server.clone() {
            Some(k) => k,
            // Q6a: ohne Schluessel wird **gepuffert**, nicht verworfen.
            None => {
                self.puffer_handshake.push(paket.to_vec());
                return Ok(());
            }
        };
        self.entschluessel_ebene(Ebene::Handshake, &schluessel, paket)
    }

    fn entschluessel_eins(&mut self, paket: &[u8]) -> Result<(), ClientFehler> {
        // Q7: DIE RICHTIGE STELLE. In `entschluessel_ebene` ist die
        // Handshake-Strecke; der 1-RTT-Verkehr — also **jedes** Datenpaket —
        // laeuft hier. Die erste Messung stand nur in `entschluessel_ebene`
        // und meldete "4 Pakete entschluesselt", waehrend 1 100 durchgegangen
        // waren. Eine Messung an der falschen Stelle ist eine Zahl ueber
        // etwas anderes.
        let t_start = mess_start();
        let schluessel = match self.eins_server.clone() {
            Some(k) => k,
            // Q6a: ohne Schluessel wird gepuffert, nicht verworfen.
            None => {
                self.puffer_eins.push(paket.to_vec());
                return Ok(());
            }
        };
        // DER DRITTE ORT DIESES FEHLERS, und er ist der am leichtesten
        // falsch zu machende: bei einem 1-RTT-Paket **vom Server** ist die
        // Destination Connection ID die **eigene** SCID des Clients, nicht
        // die SCID des Servers (RFC 9000 Abschnitt 7.2). Beim Senden ist es
        // umgekehrt. Wer auf beiden Seiten dieselbe Laenge nimmt, baut beim
        // Senden ein Paket, das der Server zuordnen kann, und scheitert beim
        // Empfang an jedem einzelnen — was sich wie ein Server ohne Antwort
        // liest.
        let gelesen = match entschuetzen(
            &schluessel,
            paket,
            self.scid.len(),
            self.eins_seen.groesste.unwrap_or(0),
        ) {
            // (die Messung steht um den Aufruf herum, siehe naechste Zeilen)
            Ok(g) => g,
            Err(e) => {
                trace(
                    "entschlüsseln",
                    &format!("EinsRtt {} Byte: {e}", paket.len()),
                );
                self.zeit_entschluesseln += mess_seit(t_start);
                self.pakete_entschluesselt = self.pakete_entschluesselt.saturating_add(1);
                return Ok(());
            }
        };
        // Q7: hier ist die Zeit **zu**, an der der AEAD und der Kopfschutz
        // gebraucht haben — der teuerste Teil des Pakets.
        self.zeit_entschluesseln += mess_seit(t_start);
        self.pakete_entschluesselt = self.pakete_entschluesselt.saturating_add(1);
        trace(
            "entschlüsseln",
            &format!(
                "EinsRtt pn={} {} Byte Klartext",
                gelesen.paketnummer,
                gelesen.klartext.len()
            ),
        );
        self.eins_seen.push(gelesen.paketnummer);
        // U2: den vollstaendigen Klartext auf Platte. Nur mit NARSIL_QUIC_DUMP.
        //
        // DER GRUND: ein Hex-Auszug auf stdout hat eine feste Laenge, und bei
        // 1169 Byte Klartext sieht man 120. Genau die Frage "was steht **hinter**
        // dem STREAM-Rahmen im selben Paket" laesst sich mit einem Auszug
        // nicht beantworten, mit einer Datei aber in Sekunden. Die Dateien
        // dienen zugleich als **Saat** fuer das Fuzz-Ziel (U4).
        if let Some(ordner) = std::env::var_os("NARSIL_QUIC_DUMP") {
            let pfad =
                std::path::Path::new(&ordner).join(format!("klartext-{}.bin", gelesen.paketnummer));
            // M3/M4: ein Fehler beim Schreiben darf den Abruf **nicht**
            // abbrechen — es ist ein Messwerkzeug, keine Verbindung.
            let _ = std::fs::create_dir_all(&ordner);
            let _ = std::fs::write(pfad, &gelesen.klartext);
        }
        let rahmen = rahmen_alle(&gelesen.klartext)?;
        trace(
            "frames",
            &format!(
                "EinsRtt pn={}: {}",
                gelesen.paketnummer,
                rahmen
                    .iter()
                    .map(rahmen_name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
        // U2-Rest: jede ACK-Nummer des Servers im 1-RTT-Raum merken.
        for frame in &rahmen {
            if let Rahmen::Ack { groesste, .. } = frame {
                self.server_acks_groesste_1rtt = Some(
                    self.server_acks_groesste_1rtt
                        .map_or(*groesste, |g: u64| g.max(*groesste)),
                );
            }
        }
        for frame in &rahmen {
            match frame {
                Rahmen::Stream {
                    stream_id,
                    offset,
                    daten,
                    fin,
                } => {
                    // Q7: die Stream-Verarbeitung ist die Phase, in der der
                    // Koerper wächst. Sie wird **um** den Aufruf gemessen —
                    // die erste Fassung las die Uhr davor ab und kam auf 0 ms.
                    let t_stream = mess_start();
                    let r = self.nimm_stream_daten(*stream_id, *offset, daten, *fin);
                    self.zeit_stream += mess_seit(t_stream);
                    r?
                }
                Rahmen::HandshakeDone => self.handshake_done = true,
                // N24, DIE MESSUNG. Vorher stand hier nur `..` — die
                // angebotene CID wurde **verworfen**.
                //
                // WARUM DAS EIN VERDACHT IST, und es ist der erste Verdacht,
                // der zu den Symptomen passt: RFC 9000 7.2 sagt, dass nach
                // bestaetigtem Handshake die Connection ID aus dem
                // **juengsten** NEW_CONNECTION_ID zu benutzen ist. Wir benutzen
                // fuer immer die SCID aus dem ServerHello. Zieht der Server die
                // zurueck (RETIRE_CONNECTION_ID), ist unser Paket unzustellbar —
                // der Server verwirft es **stillschweigend**, und genau das
                // sehen wir: keine Quittung, aber auch kein Fehler.
                //
                // ERST MESSEN, DANN AENDERN. Diese Spur sagt, ob ueberhaupt
                // eine neuere CID angeboten wird.
                Rahmen::NewConnectionId {
                    sequenz,
                    zurueckziehen_vor,
                    cid,
                    reset_token,
                } => {
                    let gleich = cid.as_slice() == self.dcid.as_slice();
                    trace(
                        "1rtt",
                        &format!(
                            concat!(
                                "NEW_CONNECTION_ID seq={} zurueck_vor={} cid={} ",
                                "(wir senden weiter mit {} = {:?})"
                            ),
                            sequenz,
                            zurueckziehen_vor,
                            narsil_tls::sha2::hexen(cid),
                            narsil_tls::sha2::hexen(&self.dcid),
                            gleich
                        ),
                    );
                    self.reset_token.push(*reset_token);
                }
                Rahmen::ConnectionClose {
                    transport,
                    fehlercode,
                    grund,
                    ..
                } => return Err(geschlossen(*fehlercode, *transport, grund)),
                _ => {}
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // TLS
    // -----------------------------------------------------------------------

    /// Der ServerHello steht **am Anfang** des Initial-CRYTO-Stroms.
    fn uebernehmen_serverhello_wenn_da(&mut self) -> Result<(), ClientFehler> {
        if !self.serverhello.is_empty() {
            return Ok(());
        }
        let daten = self.crypto_initial.daten.clone();
        if daten.len() < 4 || daten[0] != typen::SERVER_HELLO {
            return Ok(());
        }
        let laenge = laenge_von(&daten[1..4]);
        if daten.len() < 4 + laenge {
            return Ok(());
        }
        self.serverhello = daten[..4 + laenge].to_vec();
        trace(
            "serverhello",
            &format!(
                "{} Byte, roher Paketkopf: {}",
                self.serverhello.len(),
                narsil_tls::sha2::hexen(&self.letztes[..self.letztes.len().min(48)])
            ),
        );

        let sh = ServerHello::dekodiere(&self.serverhello)
            .map_err(|e| ClientFehler::Handshake(format!("ServerHello: {e}")))?;
        let suite = quic_suite(sh.cipher_suite)?;
        let key_share = sh
            .x25519()
            .ok_or_else(|| ClientFehler::Handshake("kein x25519 im ServerHello".into()))?;
        let shared = x25519::gemeinsam(&self.geheim, &key_share)
            .map_err(|e| ClientFehler::Handshake(e.to_string()))?;

        // Q6b: Das Transcript sind die Handshake-Nachrichten **mit** ihrem
        // 4-Byte-Kopf und **ohne** Record-Kopf. Genau das sind
        // `clienthello` und `serverhello`.
        self.plan = Schluesselplan::neu(shared);
        self.plan.aufnehmen(&self.clienthello);
        self.plan.aufnehmen(&self.serverhello);
        self.plan.handshakephase();

        // RFC 9001 Abschnitt 5: Initial und Handshake sind **immer**
        // AES-128-GCM-SHA256, unabhaengig von der Suite des 1-RTT-Verkehrs.
        self.handshake_server = Some(schluessel_ableiten(
            &self.plan.server_hs_traffic,
            Suite::Aes128Gcm,
        )?);
        self.handshake_client = Some(schluessel_ableiten(
            &self.plan.client_hs_traffic,
            Suite::Aes128Gcm,
        )?);
        self.suite = suite;

        // DER ERSTE ORT DIESES FEHLERS: ab hier ist die Destination
        // Connection ID die Source Connection ID des Servers (RFC 9000 7.2).
        let kopf = crate::kopf::lange_lesen(&self.letztes)
            .map_err(|e| ClientFehler::Handshake(e.to_string()))?;
        if kopf.scid.is_empty() || kopf.scid.len() > MAX_CID_LAENGE {
            return Err(ClientFehler::Handshake(format!(
                "Server-SCID mit {} Byte ist unbrauchbar",
                kopf.scid.len()
            )));
        }
        trace(
            "serverhello",
            &format!("neue dcid = {:?} ({} Byte)", kopf.scid, kopf.scid.len()),
        );
        // N42: DIE SUITE, und sie stand vorher nirgends.
        //
        // DER GRUND: Cloudflare antwortet, Google nicht. Wenn die beiden
        // **verschiedene** Suiten aushandeln, ist "Google schweigt" keine
        // Eigenschaft von Google, sondern eine Folge unserer Angebotsliste.
        // Das ist mit einem Blick auf die Suite entschieden und nicht mit
        // einer Vermutung — und es stand nicht in der Spur.
        trace(
            "serverhello",
            &format!(
                "suite={:?} (0x{:04x}), x25519-Key-Share {} Byte",
                suite,
                sh.cipher_suite,
                key_share.len()
            ),
        );
        self.dcid = kopf.scid.clone();
        Ok(())
    }

    /// Die TLS-Nachrichten des Servers aus dem Handshake-CRYTO-Strom ziehen,
    /// pruefen und in das Transcript aufnehmen.
    fn verarbeite_tls_nachrichten(&mut self) -> Result<(), ClientFehler> {
        for _ in 0..MAX_TLS_NACHRICHTEN {
            if self.server_finished {
                break;
            }
            if self.crypto_handshake.daten.len() < 4 {
                break;
            }
            let laenge = laenge_von(&self.crypto_handshake.daten[1..4]);
            if self.crypto_handshake.daten.len() < 4 + laenge {
                break;
            }
            let nachricht: Vec<u8> = self.crypto_handshake.daten[..4 + laenge].to_vec();
            // Die Nachricht wird **vor** der Pruefung aus dem Puffer
            // genommen; sonst wuerde sie beim naechsten Aufruf erneut
            // gelesen und das Transcript doppelt befuellt.
            self.crypto_handshake.daten.drain(..4 + laenge);
            trace(
                "tls",
                &format!("Nachricht Typ {} ({} Byte)", nachricht[0], nachricht.len()),
            );
            match nachricht[0] {
                typen::ENCRYPTED_EXTENSIONS => {
                    let ee = EncryptedExtensions::dekodiere(&nachricht).map_err(|e| {
                        ClientFehler::Handshake(format!("EncryptedExtensions: {e}"))
                    })?;
                    if ee.alpn.as_deref() != Some(&b"h3"[..]) {
                        return Err(ClientFehler::Handshake(format!(
                            "ALPN {:?} statt h3",
                            ee.alpn.as_deref().unwrap_or(b"<keins>")
                        )));
                    }
                    if let Some(block) = &ee.quic_transport_parameters {
                        self.uebernehmen_serverparameter(block);
                    }
                    self.plan.aufnehmen(&nachricht);
                    self.nachrichten += 1;
                }
                typen::CERTIFICATE => {
                    let zert = Certificate::dekodiere(&nachricht)
                        .map_err(|e| ClientFehler::Handshake(format!("Certificate: {e}")))?;
                    self.zertifikate = zert.zertifikate;
                    self.plan.aufnehmen(&nachricht);
                    self.nachrichten += 1;
                }
                typen::CERTIFICATE_VERIFY => {
                    let cv = CertificateVerify::dekodiere(&nachricht)
                        .map_err(|e| ClientFehler::Handshake(format!("CertificateVerify: {e}")))?;
                    self.zertifikat_pruefen(&cv)?;
                    self.plan.aufnehmen(&nachricht);
                    self.nachrichten += 1;
                }
                typen::FINISHED => {
                    // Der Server prueft ueber den Transkript **bis zum
                    // CertificateVerify** (RFC 8446 Abschnitt 4.4.4) — das
                    // eigene Finished gehoert nicht dazu.
                    let erwartet = Finished::erwartet(
                        &self.plan.finished_key_server,
                        &self.plan.transkript_hash(),
                    );
                    if !narsil_tls::hmac::vergleich_gleich(&erwartet, &nachricht[4..]) {
                        return Err(ClientFehler::Handshake(
                            "Server-Finished stimmt nicht (Q6b: Transcript oder Suite falsch?)"
                                .into(),
                        ));
                    }
                    self.plan.aufnehmen(&nachricht);
                    self.server_finished = true;
                    self.nachrichten += 1;
                    trace("tls", "Server-Finished geprueft");
                }
                // Unbekannte Nachricht: der Puffer ist um sie gekuerzt, die
                // Reihenfolge bleibt also in Ordnung (RFC 9000 12.4).
                _ => {}
            }
        }
        Ok(())
    }

    fn uebernehmen_serverparameter(&mut self, block: &[u8]) {
        let parameter = match parameter_lesen(block) {
            Ok(p) => p,
            Err(e) => {
                // Q1 verlangt, die Transportparameter als moegliche Ursache zu
                // **messen**. Ein Block, den wir nicht lesen, ist genau die
                // Stelle, an der ein stilles Versagen passieren wuerde — und
                // vorher war diese Stelle unsichtbar.
                trace(
                    "parameter",
                    &format!("UNLESBAR ({} Byte): {e}", block.len()),
                );
                return;
            }
        };
        // Q1: **alle** Parameter in die Spur, nicht nur die vier, die wir
        // benutzen. Die Frage "welche Parameter schickt der Server, und welche
        // davon lesen wir nie" ist sonst nicht beantwortbar.
        for p in &parameter {
            trace("parameter", &format!("{p:?}"));
        }
        for p in &parameter {
            match p {
                Parameter::MaxIdleTimeout(ms) => self.server_idle_ms = Some(*ms),
                Parameter::StatelessResetToken(t) => self.reset_token.push(*t),
                Parameter::BevorzugteAdresse(a) => self.reset_token.push(a.reset_token),
                _ => {}
            }
        }
    }

    // -----------------------------------------------------------------------
    // Zertifikat (Q12)
    // -----------------------------------------------------------------------

    fn zertifikat_pruefen(&mut self, cv: &CertificateVerify) -> Result<(), ClientFehler> {
        let speicher = self.opt.speicher.as_ref().ok_or_else(|| {
            ClientFehler::Zertifikat(
                "kein Vertrauensspeicher: ohne Kettenpruefung ist die Verbindung nicht \
                 gesichert (team/AUFTRAG_QUIC.txt Q12)"
                    .into(),
            )
        })?;
        // M4: `zertifikat_lesen` prueft `MAX_ZERTIFIKAT` selbst, und die Zahl
        // der Eintraege ist in `Certificate::dekodiere` begrenzt. Die
        // Kapazitaet folgt also aus einer bereits geprueften Zahl.
        let mut kette: Vec<narsil_tls::x509::Zertifikat> = Vec::with_capacity(self.zertifikate.len());
        for der in &self.zertifikate {
            kette.push(
                narsil_tls::x509::zertifikat_lesen(der)
                    .map_err(|e| ClientFehler::Zertifikat(format!("Zertifikat unlesbar: {e}")))?,
            );
        }
        self.kette_byte = self.zertifikate.iter().map(|z| z.len()).sum();
        // Der Hostname wird VOR dem Finished geprueft: ein falscher Name ist
        // ein Fehler, kein Detail.
        narsil_tls::kette::kette_pruefen(&kette, speicher, jetzt_als_sekunden(), &self.host)
            .map_err(|e| ClientFehler::Zertifikat(format!("Kette/Hostname: {e}")))?;
        let th = self.plan.transkript_hash();
        let inhalt =
            narsil_tls::signatur::certificate_verify_inhalt(narsil_tls::signatur::VORLAGE_SERVER, &th)
                .map_err(|e| ClientFehler::Zertifikat(e.to_string()))?;
        if !cv_gueltig(&kette, cv, &inhalt) {
            return Err(ClientFehler::Zertifikat(
                "CertificateVerify-Signatur passt nicht zum Blatt".into(),
            ));
        }
        self.zertifikat_geprueft = true;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Streams (Q13)
    // -----------------------------------------------------------------------

    fn nimm_stream_daten(
        &mut self,
        stream_id: u64,
        offset: u64,
        daten: &[u8],
        fin: bool,
    ) -> Result<(), ClientFehler> {
        if !self.streams.contains_key(&stream_id) && self.streams.len() >= MAX_OFFENE_STROEME {
            return Err(ClientFehler::ZuVieleStroeme {
                grenze: MAX_OFFENE_STROEME,
            });
        }
        // M4: die Endposition wird geprueft, BEVOR alloziert wird.
        let ende = (offset as usize).saturating_add(daten.len());
        if ende > MAX_ANTWORT_BYTE {
            return Err(ClientFehler::PufferVoll {
                grenze: MAX_ANTWORT_BYTE,
            });
        }
        let (gesamt, fin_neu) = {
            let puffer = self.streams.entry(stream_id).or_default();
            if puffer.gesamt + daten.len() > MAX_ANTWORT_BYTE {
                return Err(ClientFehler::PufferVoll {
                    grenze: MAX_ANTWORT_BYTE,
                });
            }
            // M4: `resize` erst, nachdem `ende` gegen die Grenze geprueft ist.
            if puffer.daten.len() < ende {
                puffer.daten.resize(ende, 0);
            }
            puffer.daten[offset as usize..ende].copy_from_slice(daten);
            // N48: merken, **welcher** Bereich jetzt wirklich da ist. Ohne das
            // ist `daten.len()` eine **Behauptung**: es zaehlt auch die
            // Nullen, mit denen `resize` eine Luecke gefuellt hat.
            puffer.merke_bereich(offset as usize, ende);
            puffer.gesamt += daten.len();
            puffer.fin |= fin;
            (puffer.gesamt, puffer.fin)
        };
        let _ = gesamt;
        // Unsere Anfrage liegt auf Stream 0. WANN die Antwort fertig ist, wird
        // nicht am FIN festgemacht: ein HTTP/3-Server, der die Antwort ueber
        // mehrere Datagramme schickt, muss den Stream ja nicht schliessen,
        // bevor der letzte Byte angekommen ist — und ein grosser Body passt
        // nicht in ein Datagramm. Fertig heisst stattdessen: die Kopfzeilen
        // mit `:status` sind da, und entweder ist der Stream geschlossen oder
        // die angekuendigte `content-length` ist erreicht.
        // Q7: DIE MESSUNG VOR DER AENDERUNG. `antwort_lesbar` liest den
        // **gesamten** Puffer von vorn — und wird nach **jedem** STREAM-Frame
        // gerufen. Fuer 1,3 MB in ~1 120 Paketen ist das quadratisch.
        //
        // UND DIE MESSUNG DARUM — die erste Fassung las die Uhr **vor** dem
        // Aufruf ab (`let t = mess_start(); self.zeit_h3 += mess_seit(t); if
        // ... antwort_lesbar()`) und kam auf **0 ms**. Das ist exakt der Fehler,
        // den `kopf.rs` seit N26/N26 und dieser Bericht seit N42 dokumentiert:
        // eine Uhr, die man vor der Arbeit abliest, misst die Uhr selbst.
        if stream_id == 0 {
            let t_lesbar = mess_start();
            // N48: `antwort_lesbar` kann jetzt **fehlschlagen** — mit einem
            // benannten Grund statt mit `false`. Ein `Err` wird **nicht**
            // geschluckt: es ist der Unterschied zwischen "noch nicht" und
            // "wird nicht mehr".
            let lesbar = self.antwort_lesbar()?;
            self.zeit_h3 += mess_seit(t_lesbar);
            if lesbar {
                if let Some(puffer) = self.streams.get(&0) {
                    let t_kopier = mess_start();
                    self.antwort = Some(puffer.daten.clone());
                    self.zeit_kopieren += mess_seit(t_kopier);
                }
            }
        }
        let _ = fin_neu;
        // U2: die SETTINGS des Servers quittieren (RFC 9114 7.2.4.1). Ohne das
        // hat google.com die Anfrage nie bearbeitet.
        if ist_ntc_uni_server(stream_id) {
            if let Some(puffer) = self.streams.get(&stream_id) {
                // N49: **KEINE Quittung von SETTINGS. Es gibt keine.**
                //
                // RFC 9114 7.2.4 im Originaltext: "A SETTINGS frame MUST be
                // sent as the first frame of each control stream and it MUST
                // NOT be sent subsequently." Im ganzen Dokument steht nirgends
                // eine Quittung von SETTINGS — wer sie aus HTTP/2 gewohnt ist,
                // traegt sie hier zu Unrecht hinein. Nachgezaehlt: das Wort
                // "acknowledg" kommt zweimal vor, beide Male ueber den
                // *Daten*transport (RFC 9000 13.2.1), nicht ueber SETTINGS.
                //
                // VORHER schickten wir ein leeres SETTINGS auf **unserem
                // eigenen** Steuerstrom bei **Offset 0**. Das war doppelt
                // verboten (Abschnitt 6.2.1 nennt den Stream-Typ, 7.2.4
                // verbietet ein zweites SETTINGS). Dass die echten Server es
                // ertrugen, war Zufall: Offset 0 ueberschrieb bereits gelieferte
                // Bytes, es kam also als doppelte Zustellung an und nicht als
                // zweites Frame.
                let _ = settings_gesehen(
                    &puffer.daten,
                    &mut self.server_settings,
                    &mut self.server_settings_werte,
                );
            }
        }
        // U2: **vor** der Antwortpruefung. Wer die Nachfuellung erst danach
        // schickt, wartet bei einer grossen Antwort vergeblich: die Antwort
        // ist erst nach dem Rest lesbar, und der Rest kommt erst nach der
        // Nachfuellung. Das ist der Deadlock, gemessen am 01.10.2026.
        self.flusskontrolle_nachfuellen(stream_id)?;
        Ok(())
    }

    /// Erhaelt und sendet MAX_STREAM_DATA / MAX_DATA, wenn der Server an die
    /// Grenze des angebotenen Fensters kommt (RFC 9000 Abschnitt 4.1).
    ///
    /// DER GRUND FUER EINE EIGENE FUNKTION: sie wird aus `nimm_stream_daten`
    /// aufgerufen, das selbst `&mut self` haelt. Die Rechnung selbst steht in
    /// `fenster_ergaenzen` und ist damit ohne Netz testbar.
    fn flusskontrolle_nachfuellen(&mut self, stream_id: u64) -> Result<(), ClientFehler> {
        let empfangen_strom = self
            .streams
            .get(&stream_id)
            .map_or(0u64, |p| p.daten.len() as u64);
        let neu_strom = fenster_ergaenzen(
            self.limit_strom,
            empfangen_strom,
            FENSTER_SCHRITT,
            MAX_ANTWORT_BYTE as u64,
        );
        // M3: `MAX_ANTWORT_BYTE` ist der Puffer, in den die Nachfuellung
        // hineinlaeuft. Eine Zusage, die groesser ist als der Puffer, holt
        // Daten, die wir nicht halten koennen — das ist kein Vorrat, das ist
        // Speicherdruck aus dem Netz.
        let empfangen_gesamt: u64 = self.streams.values().map(|p| p.daten.len() as u64).sum();
        let neu_gesamt = fenster_ergaenzen(
            self.limit_gesamt,
            empfangen_gesamt,
            FENSTER_SCHRITT,
            MAX_ANTWORT_BYTE as u64,
        );
        if neu_strom.is_none() && neu_gesamt.is_none() {
            return Ok(());
        }
        let mut payload = Vec::with_capacity(64);
        if let Some(n) = neu_gesamt {
            self.limit_gesamt = n;
            rahmen_schreiben(&Rahmen::MaxData { maximum: n }, &mut payload)?;
        }
        if let Some(n) = neu_strom {
            self.limit_strom = n;
            rahmen_schreiben(
                &Rahmen::MaxStreamData {
                    stream_id,
                    maximum: n,
                },
                &mut payload,
            )?;
        }
        trace(
            "fluss",
            &format!(
                "nachgefuellt: MAX_DATA={} MAX_STREAM_DATA({stream_id})={}",
                self.limit_gesamt, self.limit_strom
            ),
        );
        // Ohne 1-RTT-Schluessel gibt es keinen Weg, das zu senden; dann
        // wartet der Server eben. Das ist kein Fehler, sondern eine Reihenfolge.
        let schluessel = match self.eins_client.clone() {
            Some(k) => k,
            None => return Ok(()),
        };
        self.sende_1rtt_nutzlast(&schluessel, &payload, "Flusskontrolle")
    }

    /// Sind auf Stream 0 schon genug Bytes, um daraus eine Antwort zu machen?
    ///
    /// DER GRUND FUER DIESE FUNKTION: `fin` allein ist kein brauchbares
    /// Kriterium. Die HEADERS kommen in einem Datagramm, der DATA-Body in
    /// zwei weiteren, und der Server schliesst den Stream erst danach. Wer auf
    /// `fin` wartet, laeuft bei jeder Antwort mit einem Body ins
    /// Idle-Zeitlimit und gibt dann einen Timeout statt der Seite zurueck.
    ///
    /// Geprueft wird stattdessen, was wirklich da ist: mindestens ein
    /// vollstaendiger H3-Frame mit `:status`, und dann entweder `fin` oder
    /// eine `content-length`, deren Wert die gepufferten DATA-Byte deckt.
    ///
    /// Q7: Sie liest ab `h3_pos` **weiter** und merkt sich, wo sie stehen
    /// blieb. Ein halber Frame am Pufferende laesst die Position **unveraendert**
    /// — beim naechsten Paket wird genau er neu gelesen und nichts darunter.
    /// Damit ist die Summe ueber alle Pakete **linear** statt quadratisch.
    fn antwort_lesbar(&mut self) -> Result<bool, ClientFehler> {
        let puffer = match self.streams.get_mut(&0) {
            Some(p) => p,
            None => return Ok(false),
        };
        if puffer.daten.is_empty() {
            return Ok(false);
        }
        // N48: NUR DER LÜCKENLOSE TEIL, und das ist die ganze Korrektur.
        //
        // VORHER las der Parser bis `daten.len()`. `daten.len()` zaehlt aber
        // auch die Nullen, mit denen `resize` eine Luecke gefuellt hat — und
        // `00 00` liest `h3_lesen` als zwei leere DATA-Frames. Gemessen im
        // Fehlerlauf: **8,0 Byte je Frame** im Mittel, der Parser blieb bei
        // 32 927 von 126 194 Byte stehen, und `MAX_H3_FRAMES` (seit Q7
        // kumulativ) war erschoepft.
        let grenze = puffer.bis_lueckenlos();
        if grenze == 0 {
            return Ok(false);
        }
        // M3: die Schleife ist ueber die Framezahl begrenzt, nicht ueber die
        // Bytezahl, und der Puffer selbst ist durch `MAX_ANTWORT_BYTE` begrenzt.
        // Q7: **ueber den ganzen Abruf**, nicht je Aufruf — bei 1 169 Paketen
        // waere die Grenze je Aufruf nie erreicht und waere damit nicht da.
        let budget_erschoepft = puffer.h3_frames >= MAX_H3_FRAMES;
        while puffer.h3_pos < grenze && puffer.h3_frames < MAX_H3_FRAMES {
            let rest = &puffer.daten[puffer.h3_pos..];
            let (rahmen, verbraucht) = match h3_lesen(rest) {
                Ok(r) => r,
                // Ein halber Frame am Pufferende: noch nicht genug Byte. Die
                // Position bleibt stehen, damit genau er beim naechsten Paket
                // neu gelesen wird — **und nichts darunter**.
                Err(_) => break,
            };
            puffer.h3_pos += verbraucht;
            puffer.h3_frames += 1;
            match rahmen {
                H3Rahmen::Kopfblock(k) => {
                    if let Ok(felder) = feldabschnitt_dekodieren(&k) {
                        for f in &felder {
                            if f.name == b":status" {
                                puffer.h3_status = true;
                            }
                            if f.name == b"content-length" {
                                puffer.h3_laenge = std::str::from_utf8(&f.wert)
                                    .ok()
                                    .and_then(|s| s.trim().parse().ok());
                            }
                        }
                    }
                }
                H3Rahmen::Daten(d) => puffer.h3_body += d.len(),
                _ => {}
            }
        }
        // Die alte Fassung pruefte `!rest.is_empty()`, also: **alle**
        // gepufferten Byte muessen zu vollstaendigen Frames gehoeren. Dasselbe,
        // nur jetzt ueber die Position statt ueber einen Schnitt.
        // N48: DIE ZWEI FÄLLE, in denen wir NICHT einfach false sagen.
        //
        // Vorher gab es an dieser Stelle nur `false`, und **das war der
        // Fehler**: ein leeres `false` ist von einem eingefrorenen Parser nicht
        // zu unterscheiden, und der Aufrufer wiederholt die Anfrage bis zum
        // Zeitlimit. Ein Zustand, der **nicht besser wird**, braucht einen
        // **Namen** — sonst sieht ein Timeout aus wie Glueck.
        //
        // (1) Das Budget ist erschoepft: mehr Frames kann dieser Strom nicht
        //     haben. Ohne das zu melden, friert der Parser fuer immer ein.
        if budget_erschoepft && puffer.h3_pos < grenze {
            return Err(ClientFehler::AntwortNichtLesbar {
                grund: format!(
                    "Frame-Budget erschoepft: {} Frames bei {MAX_H3_FRAMES}, \
                     h3_pos={} von {grenze}",
                    puffer.h3_frames, puffer.h3_pos
                ),
            });
        }
        // (2) Das FIN ist da, aber zwischen `h3_pos` und dem lueckenlosen Ende
        //     fehlt noch ein Frame-Ende: dann ist die Antwort **unvollstaendig**
        //     und wird auch nicht vollstaendig.
        if puffer.h3_status && puffer.fin && puffer.h3_pos < grenze {
            return Err(ClientFehler::AntwortNichtLesbar {
                grund: format!(
                    concat!(
                        "FIN gesetzt, aber der letzte H3-Frame ist unvollstaendig: ",
                        "h3_pos={} von {}; der Strom hat {} Byte in {} Stuecken"
                    ),
                    puffer.h3_pos,
                    grenze,
                    puffer.daten.len(),
                    puffer.stuecke()
                ),
            });
        }
        if !puffer.h3_status || puffer.h3_pos != grenze {
            return Ok(false);
        }
        Ok(match puffer.h3_laenge {
            Some(erwartet) => puffer.h3_body >= erwartet,
            // Ohne `content-length` gibt es kein anderes Kriterium als das FIN.
            None => puffer.fin,
        })
    }

    /// Schickt die Anfrage erneut, mit neuer Paketnummer und denselben
    /// Stream-Offsets (RFC 9000 Abschnitt 12.3).
    fn wiederhole_anfrage(&mut self) -> Result<(), ClientFehler> {
        if self.anfrage_nutzlast.is_empty() {
            return Ok(());
        }
        let schluessel = match self.eins_client.clone() {
            Some(k) => k,
            None => return Ok(()),
        };
        let nutzlast = self.anfrage_nutzlast.clone();
        self.sende_1rtt_nutzlast(&schluessel, &nutzlast, "Anfrage wiederholt")
    }

    fn setze_idle_frist(&mut self) {
        let ms = match self.server_idle_ms {
            Some(b) => EIGENER_IDLE_MS.min(b),
            None => EIGENER_IDLE_MS,
        };
        self.frist = Instant::now() + Duration::from_millis(ms.max(1));
    }

    fn parse_h3_antwort(&self, roh: &[u8]) -> Result<(Antwort, Vec<u8>), ClientFehler> {
        let mut koepfe: Vec<FeldZeile> = Vec::new();
        // DER INHALT, nicht nur seine Laenge (U3). `daten` zaehlt weiterhin,
        // weil `Antwort.groesse` das sagen soll — aber `inhalt` nimmt die
        // Bytes mit. M4: die Grenze wird **einmal** gerechnet und fuer beide
        // benutzt, damit `groesse` und `inhalt` nie auseinanderlaufen.
        let mut inhalt: Vec<u8> = Vec::new();
        let mut daten = 0usize;
        // U3-REST, DIE ZWEITE HAElfte der Schnittstelle: **Trailer**.
        //
        // DER GRUND, und er ist eine Luecke, die mir erst `narsil-net` gezeigt
        // hat: `narsil_net::Response` traegt `trailers`, meines `Antwort` nicht.
        // Ein HTTP/3-Abruf, der in `narsil-net` eingesetzt wird, wuerde sie also
        // stillschweigend verlieren — und bei HTTP/3 sind Trailer haeufiger als
        // bei HTTP/2, weil es keinen Chunked-Transfer gibt.
        //
        // **KEINE NEUE FRAME-ART NÖTIG**: HTTP/3 kennt kein eigenes
        // Trailer-Rahmen. Trailer sind ein **zweiter HEADERS-Rahmen** nach den
        // DATA-Rahmen (RFC 9114 4.1). Es entscheidet also die Position, und
        // genau so ist es hier gebaut: der erste Kopfblock ist die Antwort,
        // jeder weitere sind Trailer.
        let mut trailers: Vec<FeldZeile> = Vec::new();
        let mut kopfblock_gesehen = false;
        let mut rest = roh;
        let mut frames = 0usize;
        while !rest.is_empty() {
            frames += 1;
            if frames > MAX_H3_FRAMES {
                return Err(ClientFehler::H3("zu viele Frames in der Antwort".into()));
            }
            let (rahmen, verbraucht) =
                h3_lesen(rest).map_err(|e| ClientFehler::H3(e.to_string()))?;
            rest = &rest[verbraucht..];
            match rahmen {
                H3Rahmen::Kopfblock(k) => {
                    let felder = feldabschnitt_dekodieren(&k)
                        .map_err(|e| ClientFehler::H3(format!("QPACK: {e}")))?;
                    if kopfblock_gesehen {
                        // M3: Jede Zahl hier haengt an fremden Daten, also ist
                        // sie benannt und geprueft. `MAX_TRAILER_FELDER` haengt
                        // an `MAX_ANTWORT_BYTE`: mehr Trailer als ein
                        // Anteil am maximalen Koerper sind Unsinn.
                        if trailers.len() + felder.len() > MAX_TRAILER_FELDER {
                            return Err(ClientFehler::H3(format!(
                                "zu viele Trailer-Felder ({}), hoechstens {}",
                                trailers.len() + felder.len(),
                                MAX_TRAILER_FELDER
                            )));
                        }
                        trailers.extend(felder);
                    } else {
                        kopfblock_gesehen = true;
                        koepfe = felder;
                    }
                }
                H3Rahmen::Daten(d) => {
                    // M3: `daten + d.len()` kann nicht ueberlaufen, weil beide
                    // <= `MAX_ANTWORT_BYTE` sind, und `daten` wird hier immer
                    // gegen diese Grenze geprueft. `checked_add` trotzdem, weil
                    // die Grenze eine Konstante ist und der Puffer eine
                    // Zusage — eine Zusage ist kein `checked_*`.
                    let neu = daten.checked_add(d.len()).ok_or(ClientFehler::PufferVoll {
                        grenze: MAX_ANTWORT_BYTE,
                    })?;
                    if neu > MAX_ANTWORT_BYTE {
                        return Err(ClientFehler::PufferVoll {
                            grenze: MAX_ANTWORT_BYTE,
                        });
                    }
                    daten = neu;
                    inhalt.extend_from_slice(&d);
                }
                _ => {}
            }
        }
        // U3-Rest: `set-cookie`, **ohne** Verlust. `narsil_net::Response` traegt
        // `set_cookie`, meines `Antwort` bis hierher nicht — ein HTTP/3-Abruf
        // haette Cookies also stillschweigend verloren. "Still" ist das
        // Schlimme daran: der Abruf gelingt, die Seite erscheint, die Anmeldung
        // fehlt.
        //
        // DER NAMENVERGLEICH: nach RFC 9114 4.2 sind Feldnamen klein. Trotzdem
        // `eq_ignore_ascii_case` — ein Server, der `Set-Cookie` gross schreibt,
        // ist nicht konform, aber sein Koerper soll daran nicht scheitern.
        let mut set_cookie: Vec<Vec<u8>> = Vec::new();
        for feld in &koepfe {
            if feld.name.eq_ignore_ascii_case(b"set-cookie") {
                set_cookie.push(feld.wert.clone());
            }
        }

        let mut status = 0u16;
        for feld in &koepfe {
            if feld.name == b":status" {
                if let Ok(s) = std::str::from_utf8(&feld.wert) {
                    status = s.parse().unwrap_or(0);
                }
            }
        }
        if status == 0 {
            return Err(ClientFehler::H3("kein :status in der Antwort".into()));
        }
        // U3: **vor** dem Verschieben lesen, `koepfe` wandert gleich in die
        // Antwort.
        let alt_svc = self.alt_svc_aus_kopfzeilen(&koepfe);
        // M7: `daten` und `inhalt.len()` muessen **immer** gleich sein. Wenn
        // das je auseinanderliefe, waere `Antwort.groesse` eine Zahl ohne
        // Inhalt, und der Aufrufer wuerde es erst spaeter merken.
        debug_assert_eq!(
            daten,
            inhalt.len(),
            "M7: groesse und inhalt muessen uebereinstimmen"
        );
        // N23, **DIE KONTROLLE**, und sie fehlte mir zum Schluss selbst.
        //
        // Die beiden Zahlen werden bisher nur im Abbruch-Fall ausgegeben. Damit
        // ist "Handshake-Quittung = None" bei google und nginx kein *Befund*,
        // sondern eine Zahl, die ich mit gar nichts vergleichen kann — ich
        // wusste nicht, ob Cloudflare dasselbe zeigt.
        //
        // WAS DIE KONTROLLE ENTSCHEIDET: Wenn Cloudflare (Handshake vollendet,
        // HTTP/3 200) ebenfalls `None` meldet, dann ist die Quittung hier
        // nicht der Unterschied und ich war mit N8-N20 wieder am falschen Ende.
        // Wenn Cloudflare eine Quittung zeigt, ist der Unterschied real und
        // der Fehler liegt **vor** der 1-RTT-Ebene — bei der Initial-
        // Zustellung, nicht bei der Anfrage.
        trace(
            "1rtt",
            &format!(
                concat!(
                    "ENDE ok: hoechste 1-RTT-Quittung = {:?} (1-RTT pn 0..{} gesendet); ",
                    "hoechste Handshake-Quittung = {:?} (Handshake pn 0..{} gesendet)"
                ),
                self.server_acks_groesste_1rtt,
                self.naechste_pn[2].saturating_sub(1),
                self.server_acks_groesste_handshake,
                self.naechste_pn[1].saturating_sub(1)
            ),
        );
        let antwort = Antwort {
            status,
            koepfe,
            groesse: daten,
            suite: self.suite,
            handshake_nachrichten: self.nachrichten,
            zertifikat_geprueft: self.zertifikat_geprueft,
            kette_byte: self.kette_byte,
            hostname: self.host.clone(),
            alt_svc,
            settings: self.server_settings_werte.clone(),
            set_cookie,
            trailers,
        };
        // Q7: die Verteilung ausgeben — **vor** `Ok(...)`, weil ab hier `self`
        // bewegt wird.
        let gesamt = self
            .t_gesamt
            .map_or(std::time::Duration::ZERO, |t| t.elapsed());
        self.melde_verteilung(gesamt);
        Ok((antwort, inhalt))
    }

    /// Liest den `Alt-Svc`-Eintrag aus den Kopfzeilen (U3).
    ///
    /// DER FALL, DASS MEHRERE `alt-svc` KOPFZEILEN KOMMEN: RFC 9114 4.2.2
    /// erlaubt das. `Vec<FeldZeile>` behaelt alle, und hier wird der **erste
    /// mit `h3`** genommen — nicht der letzte, und nicht der erste
    /// `alt-svc` ueberhaupt. Grund: ein Header mit `h2` zuerst und `h3`
    /// danach ist die uebliche Reihenfolge, und der Aufrufer will HTTP/3.
    fn alt_svc_aus_kopfzeilen(&self, koepfe: &[FeldZeile]) -> Option<crate::alt_svc::AltSvc> {
        for feld in koepfe {
            if feld.name != b"alt-svc" {
                continue;
            }
            // M3: der Wert kann kein UTF-8 sein (er ist es nie, aber ein
            // Angreifer kann es schicken). `from_utf8` schluckt das mit
            // `None`, und dann ueberspringen wir diesen Header.
            let wert = std::str::from_utf8(&feld.wert).ok()?;
            if let Ok(a) = crate::alt_svc::alt_svc_lesen(wert) {
                return Some(a);
            }
        }
        None
    }

    // -----------------------------------------------------------------------
    // Retry
    // -----------------------------------------------------------------------

    /// Retry nach RFC 9000 Abschnitt 17.2.5. Der Integritaetstag wird
    /// **geprueft**: ohne diese Pruefung koennte jeder im Netzpfad ein
    /// Clienthello umleiten (Amplification-Angriff, RFC 9000 17.2.5).
    fn bearbeite_retry(&mut self) -> Result<(), ClientFehler> {
        let datagramm = self.letztes.clone();
        let (erstes_byte, version, dcid, scid, token) = retry_zerlegen(&datagramm)?;
        let erwartet = retry_tag(
            &self.urspruengliche_dcid,
            erstes_byte,
            version,
            dcid,
            scid,
            &token,
        )?;
        if !narsil_tls::hmac::vergleich_gleich(&erwartet, &datagramm[datagramm.len() - 16..]) {
            return Err(ClientFehler::Handshake(
                "Retry-Integritaetstag stimmt nicht".into(),
            ));
        }
        // N25: nach einem Retry ist die DCID die **SCID des Retry-Pakets**.
        trace(
            "retry",
            &format!(
                "neue dcid = {} ({} Byte)",
                narsil_tls::sha2::hexen(scid),
                scid.len()
            ),
        );
        self.dcid = scid.to_vec();
        self.berechne_initial_schluessel()?;
        self.naechste_pn = [0, 0, 0];
        self.sende_initial(0)
    }

    // -----------------------------------------------------------------------
    // Verfolger
    // -----------------------------------------------------------------------

    fn verfolger(&self, ebene: Ebene) -> &Verfolger {
        match ebene {
            Ebene::Initial => &self.initial_seen,
            Ebene::Handshake => &self.handshake_seen,
            Ebene::EinsRtt => &self.eins_seen,
        }
    }

    fn verfolger_mut(&mut self, ebene: Ebene) -> &mut Verfolger {
        match ebene {
            Ebene::Initial => &mut self.initial_seen,
            Ebene::Handshake => &mut self.handshake_seen,
            Ebene::EinsRtt => &mut self.eins_seen,
        }
    }
}

/// Vergleicht die letzten 16 Byte eines Datagramms mit einer Liste von
/// Stateless-Reset-Token (RFC 9000 Abschnitt 10.3.1). Der Vergleich laeuft in
/// konstanter Zeit (`narsil_tls::hmac::vergleich_gleich`) und bricht nicht frueh
/// ab — ein Zeitunterschied waere hier eine Information, die ein Angreifer
/// nutzen kann.
///
/// **Oeffentlich**, weil `tests/q13_schluss.rs` die Erkennung ohne Netz prueft.
/// Das ist kein Testhaken im Client: die Erkennung ist eine reine Funktion
/// ueber zwei Bytestrings, und die gehoert so benannt und benutzt, nicht
/// hinter einem Sichtbarkeitsschloss.
pub fn token_steht_in_daten(token: &[u8; 16], datagramm: &[u8]) -> bool {
    token_steht_in_daten_mehrere(std::slice::from_ref(token), datagramm)
}

/// Wie `token_steht_in_daten`, fuer mehrere Tokens. `tokens` sind die aus
/// `preferred_address` und allen `NEW_CONNECTION_ID`-Frames.
pub fn token_steht_in_daten_mehrere(tokens: &[[u8; 16]], datagramm: &[u8]) -> bool {
    // Kuerzer als das Token: ein Reset **kann** nicht erkannt werden, und das
    // ist besser als ein Fehlurteil. Die Grenze ist benannt und gleich der
    // Laenge des Tokens plus einem Byte Kopf.
    if datagramm.len() < MIN_STATELESS_RESET || tokens.is_empty() {
        return false;
    }
    let kandidat = &datagramm[datagramm.len() - 16..];
    tokens.iter().fold(false, |vorher, t| {
        vorher | narsil_tls::hmac::vergleich_gleich(t.as_slice(), kandidat)
    })
}

/// Wie `retry_zerlegen`, aber ohne Token-Fehler und ohne `ClientFehler`.
///
/// **Fuer den Fuzzer.** DER GRUND: `retry_zerlegen` in `client.rs` ist die
/// Variante, die der Client benutzt, und sie hat drei Fehlerarten, die der
/// Fuzzer braucht (`ClientFehler::Handshake`). Diese hier wirft `SchutzFehler`
/// zurueck, weil `retry_tag` das erwartet — und gibt die Laengenfelder auch
/// fuer kaputte Eingaben zurueck, damit der Fuzzer die *Werte* sieht, die ein
/// Aufrufer benutzen wuerde.
///
/// DER WICHTIGE PUNKT ist die Grenze: `client.rs::retry_zerlegen` prueft jede
/// Laenge gegen `d.len()`, BEVOR daraus ein Slice entsteht. Ein Fuzzer, der
/// diese Pruefung umgeht, wuerde genau den Fehler finden, den die Pruefung
/// verhindern soll — also laesst er sie hier laufen.
pub fn retry_zerlegen_fuer_test(d: &[u8]) -> Result<RetryTeile, SchutzFehler> {
    // 1 + 4 (Version) + 1 (DCID-Laenge) + DCID + 1 (SCID-Laenge) + SCID
    // + Token + 16 (Tag) = 23 Byte Minimum.
    if d.len() < 23 {
        return Err(SchutzFehler::PaketZuKurz {
            braucht: 23,
            da: d.len(),
        });
    }
    let version = u32::from_be_bytes([d[1], d[2], d[3], d[4]]);
    let dcid_len = d[5] as usize;
    if dcid_len > MAX_CID_LAENGE {
        return Err(SchutzFehler::Kopf(KopfFehler::CidZuLang {
            ist: dcid_len,
            maximum: MAX_CID_LAENGE,
        }));
    }
    let mut pos = 6;
    if d.len() < pos + dcid_len + 1 {
        return Err(SchutzFehler::PaketZuKurz {
            braucht: pos + dcid_len + 1,
            da: d.len(),
        });
    }
    let dcid = d[pos..pos + dcid_len].to_vec();
    pos += dcid_len;
    let scid_len = d[pos] as usize;
    pos += 1;
    if scid_len > MAX_CID_LAENGE {
        return Err(SchutzFehler::Kopf(KopfFehler::CidZuLang {
            ist: scid_len,
            maximum: MAX_CID_LAENGE,
        }));
    }
    if d.len() < pos + scid_len + 16 {
        return Err(SchutzFehler::PaketZuKurz {
            braucht: pos + scid_len + 16,
            da: d.len(),
        });
    }
    let scid = d[pos..pos + scid_len].to_vec();
    pos += scid_len;
    // M3: `rest` wird **einmal** per `checked_sub` bestimmt und dann benutzt.
    // Vorher stand hier `d.len() - 16 - pos` zweimal — eine Kette zweier
    // Subtraktionen auf fremden Daten, genau die Form, die die Koordination
    // als "ungepruefte Subtraktionen" gemeldet hat. Die Pruefung oben
    // verhindert den Unterlauf fuer dieses `d`, aber sie ist eine Zusage
    // einer anderen Funktion, und eine Zusage ist kein `checked_*`.
    let rest = d.len().checked_sub(16).ok_or(SchutzFehler::PaketZuKurz {
        braucht: 16,
        da: d.len(),
    })?;
    if pos > rest {
        return Err(SchutzFehler::PaketZuKurz {
            braucht: pos + 16,
            da: d.len(),
        });
    }
    // M4: der Token-Rest wird begrenzt, bevor kopiert wird.
    if rest - pos > crate::kopf::MAX_TOKEN_LAENGE {
        return Err(SchutzFehler::Kopf(KopfFehler::TokenZuLang {
            ist: rest - pos,
            maximum: crate::kopf::MAX_TOKEN_LAENGE,
        }));
    }
    Ok((d[0], version, dcid, scid, d[pos..rest].to_vec()))
}

/// Die Cipher Suite aus dem TLS-cipher_suite-Feld (Q6b).
///
/// **Warum 0x1302 ein benannter Fehler ist und kein stilles AES-128:**
/// `TLS_AES_256_GCM_SHA384` braucht 32 Byte Schluessel und einen
/// SHA-384-Transkript. `Schluesselplan` aus narsil-tls rechnet SHA-256, und
/// `schluessel_ableiten` gibt bei `Suite::Aes128Gcm` 16 Byte aus. Mit 0x1302
/// weiterzurechnen hiesse: Schluessel, Nonce, AAD und Header-Schutz aus dem
/// falschen Hash — der Handshake scheitert dann spaeter an "der Tag stimmt
/// nicht", und niemand weiss warum. Unser ClientHello bietet die Suite gar
/// nicht an, also ist der Fall nur noch ein Testfall.
pub fn quic_suite(cipher_suite: u16) -> Result<Suite, ClientFehler> {
    match cipher_suite {
        // TLS_AES_128_GCM_SHA256
        0x1301 => Ok(Suite::Aes128Gcm),
        // TLS_CHACHA20_POLY1305_SHA256 — anderer Header-Schutz, in
        // `maske_berechnen` aber bereits getrennt vorhanden (RFC 9001 5.4.4).
        0x1303 => Ok(Suite::ChaCha20Poly1305),
        0x1302 => Err(ClientFehler::Handshake(
            "TLS_AES_256_GCM_SHA384 (0x1302): 32-Byte-Schluessel und SHA-384-Transcript \
             fehlen; der Client bietet die Suite deshalb gar nicht an"
                .into(),
        )),
        andere => Err(ClientFehler::Handshake(format!(
            "unbekannte Cipher Suite 0x{andere:04x}"
        ))),
    }
}

/// Baut ein Long-Header-Paket und fuellt es auf `mindestens` Byte auf.
/// (Der Aufruferweg mit PADDING ist `lang_paket_gefuellt`.)
///
/// DER GRUND FUER DEN ZWEITEN DURCHLAUF: das Laenge-Feld ist ein Varint, und
/// seine Breite haengt von der Nutzlast ab. Die Paketlaenge laesst sich also
/// nicht in einem Durchlauf berechnen. Deshalb wird der Kopf einmal **ohne**
/// Nutzlast gebaut, um die tatsaechliche Kopfgroesse zu kennen.
/// Baut ein Long-Header-Paket, fuellt es auf mindestens `MIN_NUTZLAST` Byte
/// auf und bei `mindestens > 0` auf mindestens diese Datagrammgroesse.
///
/// DER GRUND FUER DEN EIGENEN WEG: `initial_schuetzen` und
/// `kurzes_schuetzen` fuellen **nicht** auf, weil sie den Klartext nicht
/// veraendern duerfen (siehe `mit_auffuellung`). Ein PING oder ein
/// einzelnes ACK ist aber kuerzer als das Header-Schutz-Sample
/// (RFC 9001 Abschnitt 5.4.2), also muss der **Aufrufer** auf fuellen — und
/// genau hier, in der eine QUIC-Nutzlast entsteht, ist der richtige Ort fuer
/// ein PADDING-Byte.
// clippy::too_many_arguments ist hier bewusst abgeschaltet: die Liste ist
// genau die Liste von `lange_schreiben` plus die beiden Fuellziele, und
// ein Bündel (`PaketTeil`) würde beim Aufrufen nur die Reihenfrage
// verstecken, die hier schon falsch beantwortet wurde (pn_laenge).
#[allow(clippy::too_many_arguments)]
fn lang_paket_gefuellt(
    schluessel: &PaketSchluessel,
    typ: u8,
    dcid: &[u8],
    scid: &[u8],
    token: &[u8],
    pn: u64,
    pn_laenge: usize,
    payload: &[u8],
    mindestens: usize,
) -> Result<Vec<u8>, ClientFehler> {
    let mut nutzlast = crate::schutz::mit_auffuellung(payload, pn_laenge);
    if mindestens > 0 {
        let (kopf, _) = lange_schreiben(typ, 1, dcid, scid, token, pn, pn_laenge, nutzlast.len())?;
        let mut gesamt = kopf.len() + nutzlast.len() + 16;
        while gesamt < mindestens {
            nutzlast.push(0x00);
            gesamt += 1;
        }
    }
    Ok(initial_schuetzen(
        schluessel, typ, 1, dcid, scid, token, pn, pn_laenge, &nutzlast,
    )?)
}

/// ACK-Payload aus einem Paketnummer-Verfolger. Leer, wenn nichts empfangen
/// wurde — dann gibt es auch kein ACK-Paket.
fn ack_payload(verfolger: &Verfolger) -> Vec<u8> {
    let mut payload = Vec::with_capacity(16);
    if let Some((groesste, erste, bereiche)) = verfolger.rahmen() {
        let _ = rahmen_schreiben(
            &Rahmen::Ack {
                groesste,
                verzoegerung: 0,
                erste_range: erste,
                bereiche,
                ecn: None,
            },
            &mut payload,
        );
    }
    payload
}

/// Die drei Byte Nachrichtenlaenge einer TLS-Handshake-Nachricht
/// (RFC 8446 Abschnitt 4).
fn laenge_von(drei: &[u8]) -> usize {
    ((drei[0] as usize) << 16) | ((drei[1] as usize) << 8) | drei[2] as usize
}

/// Die fuenf Teile eines Retry-Pakets (RFC 9000 Abschnitt 17.2.5).
///
/// EIN NAMTYP STATT EINES TUPELS, und nicht aus Eleganz: `retry_zerlegen`
/// liefert diese fuenf Werte an drei Stellen, und an zweien davon war in
/// drei aufeinanderfolgenden Bearbeitungen die Reihenfolge vertauscht worden
/// (Flag und Version, dann DCID, dann SCID, dann Token) — jeweils ohne
/// Compilerfehler, weil ein fuenf-elementiges Tupel zweier gleicher Typen
/// (_Vec<u8>_) an jeder Position akzeptiert. Der Name macht den Tausch
/// sichtbar und der Typ zwingt die Reihenfolge.
pub type RetryTeile = (u8, u32, Vec<u8>, Vec<u8>, Vec<u8>);

/// Dasselbe mit Leihverweisen, so wie es die Zwischenrechnung braucht: die
/// Connection-IDs werden nur verglichen, nicht kopiert.
type RetryBloecke<'a> = (u8, u32, &'a [u8], &'a [u8], Vec<u8>);

// ---------------------------------------------------------------------------
// Steuerstroeme (U2, 01.10.2026)
// ---------------------------------------------------------------------------

/// Der Steuerstrom des Clients (RFC 9114 Abschnitt 6.2.1): client-initiiert
/// und unidirektional, also ID 2, 6, 10, ... Davon ist 2 der Steuerstrom.
pub const STEUERSTROM_CLIENT: u64 = 2;

/// Der Steuerstrom des Servers (RFC 9114 Abschnitt 6.2.3): server-initiiert
/// und unidirektional, also ID 3, 7, 11, ... Davon ist 3 der Steuerstrom.
pub const STEUERSTROM_SERVER: u64 = 3;

/// Ist `stream_id` ein **server-initiierter unidirektionaler** Strom?
///
/// QUIC nummeriert die Stroeme so (RFC 9000 Abschnitt 2.1): Bit 0 = Initiat,
/// Bit 1 = Richtung, Bit 2 = unidirektional. Ein server-initiierter
/// unidirektionaler Strom hat also `id % 4 == 3`.
pub fn ist_ntc_uni_server(id: u64) -> bool {
    id % 4 == 3
}

/// Liest auf einem Steuerstrom die SETTINGS des Gegenuebers — **einmal**.
///
/// REIN, damit der Test denselben Weg trifft wie der Client. `gesehen` ist der
/// Zustand des Aufrufers (dieselbe Adresse bei mehreren Aufrufen); die
/// Funktion veraendert ihn nur, wenn ein **vollstaendiges** SETTINGS-Frame
/// darin lag.
///
/// RUECKGABE `true` heisst: die SETTINGS sind vollstaendig angekommen und
/// duerfen quittiert werden.
///
/// WARUM DAS FEHLTE (U2, 01.10.2026): RFC 9114 Abschnitt 7.2.4.1 — "Each
/// endpoint MUST acknowledge the other endpoint's SETTINGS frame with a
/// SETTINGS frame of type 0x04 and length 0." Ohne diese Quittung hat
/// google.com die Anfrage nie bearbeitet: der Server schickte seine SETTINGS,
/// dann 40mal nichts, und unsere eigene 1-RTT-Pakete wurden nie bestaetigt.
/// Cloudflare antwortete auch ohne Quittung — was die Diagnose gerade
/// unmoeglich machte, weil zwei Server sich verschieden verhalten.
///
/// DREI SAEULEN, an denen es hier scheitern kann, und alle drei sind noetig,
/// weil der Steuerstrom bei mehreren Paketen ankam:
///  1. der Strom-Typ (1 Byte) muss 0x00 sein,
///  2. das SETTINGS-Frame muss **vollstaendig** sein (sonst ein halbes Paket),
///  3. es darf nur einmal quittiert werden.
pub fn settings_gesehen(daten: &[u8], gesehen: &mut bool, werte: &mut Vec<(u64, u64)>) -> bool {
    if *gesehen || daten.len() < 2 || daten[0] != 0x00 {
        return false;
    }
    let rest = &daten[1..];
    let (typ, n1) = match varint::lese(rest) {
        Ok(v) => v,
        Err(_) => return false,
    };
    // 0x04 = SETTINGS auf dem Steuerstrom.
    if typ != crate::h3::H3_EINSTELLUNGEN {
        return false;
    }
    // Die Laenge wird gelesen, damit ein **halbes** Frame erkannt wird; das
    // vollstaendige Lesen macht `h3_lesen` darunter.
    if varint::lese(&rest[n1..]).is_err() {
        return false;
    }
    match crate::h3::h3_lesen(rest) {
        Ok((crate::h3::H3Rahmen::Einstellungen(paare), _)) => {
            // M4: die Werte werden begrenzt uebernommen, nicht ungeprueft
            // gesammelt. `MAX_EINSTELLUNGEN` ist die Grenze aus `h3.rs`.
            if paare.len() <= crate::h3::MAX_EINSTELLUNGEN {
                *werte = paare;
            }
            *gesehen = true;
            true
        }
        _ => false,
    }
}

/// Zerlegt ein Retry-Paket (RFC 9000 Abschnitt 17.2.5). M4: jede Laenge wird
/// gegen den Puffer geprueft, BEVOR daraus ein Ausschnitt genommen wird.
/// Das erste Byte eines Retry, nach RFC 9000 Tabelle 3: `1 1 1 0 0 0 0 0`.
///
/// N40: **der Wert, nicht eine Maske** — die unteren vier Bit sind unbenutzt und
/// null.
pub const RETRY_ERSTES_BYTE: u8 = 0xf0;

/// Ist dieses Paket ueberhaupt ein Retry?
///
/// N40, und der Grund ist eine **Messung**: In N39 habe ich mit
/// `retry_zerlegen_fuer_test` geprueft, ob die nicht-entschluesselbaren Pakete
/// einer echten Aufzeichnung Retries sind. Die Funktion sagte bei **allen
/// vier** `ja` — auch bei einem, das sich nachweislich zu einem ServerHello
/// entschluesseln laesst.
///
/// **DIE FUNKTION PRUEFTE DEN PAKETTYPIBERHAUPT NICHT.** Sie las Laengen und
/// Felder, und jedes Long-Header-Paket hat diese Felder. Die Frage "ist das ein
/// Retry?" war damit mit einem Werkzeug beantwortet worden, das sie gar nicht
/// stellte.
pub fn ist_retry(d: &[u8]) -> bool {
    d.first() == Some(&RETRY_ERSTES_BYTE)
}

fn retry_zerlegen(d: &[u8]) -> Result<RetryBloecke<'_>, ClientFehler> {
    // 1 (Flag) + 4 (Version) + 1 (DCID-Laenge) + DCID + 1 (SCID-Laenge)
    // + SCID + Token + 16 (Tag) = 23 Byte Minimum.
    if d.len() < 23 {
        return Err(ClientFehler::Handshake("Retry zu kurz".into()));
    }
    let version = u32::from_be_bytes([d[1], d[2], d[3], d[4]]);
    let dcid_len = d[5] as usize;
    if dcid_len > MAX_CID_LAENGE {
        return Err(ClientFehler::Handshake("Retry-DCID zu lang".into()));
    }
    let mut pos = 6;
    if d.len() < pos + dcid_len + 1 {
        return Err(ClientFehler::Handshake("Retry abgeschnitten".into()));
    }
    let dcid = &d[pos..pos + dcid_len];
    pos += dcid_len;
    let scid_len = d[pos] as usize;
    pos += 1;
    if scid_len > MAX_CID_LAENGE {
        return Err(ClientFehler::Handshake("Retry-SCID zu lang".into()));
    }
    if d.len() < pos + scid_len + 16 {
        return Err(ClientFehler::Handshake("Retry abgeschnitten".into()));
    }
    let scid = &d[pos..pos + scid_len];
    pos += scid_len;
    // M4: der Token-Rest wird begrenzt, nicht ungeprueft kopiert.
    let mut token = d[pos..d.len() - 16].to_vec();
    if token.len() > crate::kopf::MAX_TOKEN_LAENGE {
        return Err(ClientFehler::Handshake("Retry-Token zu lang".into()));
    }
    token.shrink_to_fit();
    Ok((d[0], version, dcid, scid, token))
}

// ---------------------------------------------------------------------------
// Flusskontrolle (U2, 01.10.2026)
// ---------------------------------------------------------------------------

/// Das Fenster, das der Client im ClientHello anbietet
/// (`initial_max_stream_data_*` und `initial_max_data`).
///
/// DIESE ZAHL WAR DIE URSACHE VON U2, und sie steht deshalb weit oben und
/// nicht versteckt in `baut_clienthello`. Gemessen am 01.10.2026 gegen
/// cloudflare-quic.com: mit `1 << 16` genau fuer den **Antwortstrom** bot der
/// Server exakt 65534 Byte auf Stream 0 und dann **nichts mehr** — kein FIN,
/// keine weiteren Pakete, 68 Pakete lang Stille. Der Grund ist RFC 9000
/// Abschnitt 4.1: der Server darf nur so viele Byte auf einem Strom senden,
/// wie der Empfaenger erlaubt hat, und ein Client, der sein Limit nie
/// erhoeht, erhaelt eine abgeschnittene Antwort. Ein Browser bricht hier nicht
/// ab, er **fragt nach**.
///
/// DER WERT WAR ZU KLEIN, und Q3 hat es gemessen (01.10.2026, Abnahme ueber
/// fuenf Seiten, Zeiten in `team/BERICHT_QUIC_U3.txt`).
///
/// DIE ERSTE MESSUNG WAR **DANEBEN**, und das gehoert mit in den Kommentar:
/// mit 64 KiB und 1 MiB war im **Debug**-Bau kein Unterschied zu sehen
/// (6 086 ms gegen 6 260 ms — Rauschen). Der Grund war nicht "das Fenster ist
/// egal", sondern der ganze Abruf lief im Debug-Bau, in dem das reine Rust-AES
/// um Zehnerpotenzen langsamer ist. Erst der **Release**-Bau trennt die
/// Wirkung:
///
/// ```text
/// www.cloudflare.com  1 343 809 Byte
///     64 KiB: 826 ms      1 MiB: 782 ms
/// www.youtube.com       932 657 Byte
///     64 KiB: 737 ms      1 MiB: 654 ms
/// www.facebook.com      417 894 Byte
///     64 KiB: 473 ms      1 MiB: 455 ms
/// ```
///
/// Drei von drei Seiten, jede schneller — klein, aber **gleichgerichtet**.
/// Die Rechnung dahinter: ein Fenster von 64 KiB gibt 64 KiB je Rundenzeit
/// frei, eine 1,3-MB-Seite braucht damit 1 342 991 / 65 536 = **21 Runden**;
/// bei 1 MiB sind es **2**.
///
/// DIE ZAHL IST DIE KLEINERE DER BEIDEN, die heutige Browser beim Stromfenster
/// setzen (Chromium: `kDefaultStreamWindowSize` 1 MiB, Verbindung 32 MiB) —
/// ein Vergleichswert, kein Zitat aus einer Norm.
///
/// WAS Dagegen spricht, und es ist ein echter Einwand: der Wert ist eine
/// **Zusage ueber das Netz** an einen fremden Server. Deshalb bleibt
/// `MAX_ANTWORT_BYTE` (8 MiB) die harte Grenze fuer das, was wir **speichern**,
/// und die Nachfuellung (`FENSTER_SCHRITT`) deckelt es genauso.
pub const FENSTER_START: u64 = 1 << 20;

/// Nachgefuellt wird, sobald der Server diesen Bruchteil seines **aktuellen**
/// Limits erreicht hat — in 256tel, damit die Rechnung ganzzahlig bleibt.
/// 128 heisst "ab der Haelfte".
///
/// DER ERSTE VERSUCH HIER WAR EIN FESTER ABSTAND (`FENSTER_START / 2` als
/// Bytezahl) und der hat **nicht funktioniert**: mit einem Fenster von 65536
/// lag die Schwelle damit bei 98304, also jenseits des Fensters selbst. Der
/// Server haette die Schwelle nie erreichen koennen, der Client haette nie
/// nachgefuellt, und das gemessene Ergebnis war wieder exakt 65482 Byte
/// Stille. Ein Abstand in Byte ist hier grundsaetzlich falsch: er muss sich am
/// **aktuellen** Limit orientieren, weil das nach jeder Nachfuellung groesser
/// wird.
pub const FENSTER_ABSTAND: u64 = 128;

/// Wie weit das Limit je Nachfuellung waechst. So gross wie
/// `MAX_ANTWORT_BYTE`, denn mehr zu erlauben als wir speichern koennten waere
/// eine Zusage ohne Gegenwert.
pub const FENSTER_SCHRITT: u64 = 16 * 1024 * 1024;

/// Rechnet das naechste Flusskontroll-Limit fuer einen Strom.
///
/// REIN, ohne Seiteneffekt und ohne `self` — damit der Test genau den Code
/// trifft, den der Client benutzt, und nicht eine Kopie davon.
///
/// `gegeben` ist das zuletzt erteilte Limit, `empfangen` die hoechste
/// Endposition auf diesem Strom, `schritt` der Zuwachs, `grenze` die harte
/// Obergrenze. Antwort `None` heisst "nichts zu tun".
///
/// DREI EIGENTUMLICHKEITEN, jede mit Grund:
///
/// 1. Das neue Limit ist **nie kleiner** als `gegeben`. RFC 9000 4.1 macht
///    ein sinkendes Limit zum Protokollfehler; ein Peer's spaeteres Paket
///    mit altem (kleinerem) Stand wuerde also die Verbindung zerlegen.
/// 2. Das Limit steigt **nur bei Bedarf**, nicht bei jedem Paket. Sonst
///    schickt der Client pro Datagramm ein MAX_STREAM_DATA und der Server
///    antwortet darauf mit einem eigenen — eine Endlosschleife aus zwei
///    hoeflichen Mitteln.
/// 3. `min(schritt, grenze)`: es gibt nichts zu erlauben, was wir nicht
///    speichern koennten.
pub fn fenster_ergaenzen(gegeben: u64, empfangen: u64, schritt: u64, grenze: u64) -> Option<u64> {
    // Die Schwelle haengt am **aktuellen** Limit, nicht an einem festen
    // Abstand: die Haelfte von `gegeben`. `saturating_mul`, weil `gegeben` aus
    // fremdem Zustand kommt (M3) — auch wenn er hier <= `grenze` ist, ist das
    // eine Zusage und kein `checked_*`.
    let schwelle = gegeben.saturating_mul(FENSTER_ABSTAND).saturating_div(256);
    if empfangen < schwelle {
        return None;
    }
    let ziel = empfangen.saturating_add(schritt.min(grenze));
    let neu = ziel.min(grenze);
    if neu <= gegeben {
        // Schon am Anschlag: weiteres Nachfuellen waere eine Endlosschleife
        // ohne Wirkung. Der Puffer laeuft stattdessen voll und meldet
        // `ClientFehler::PufferVoll` — ein benannter Fehler.
        return None;
    }
    Some(neu)
}

/// Die Adresse eines Hosts auf einem Port, IPv4 bevorzugt.
///
/// DER PORT IST EIN PARAMETER, und nicht aus Bequemlichkeit: mit fest 443
/// konnte `tests/ack_luecken.rs` keinen **lokalen** Server benutzen (Port 443
/// gehoert einem fremden Prozess), und ein Test gegen das eigene Netz ist der
/// einzige, der einen Rechenfehler *beweist* statt zu vermuten.
fn adresse_von(host: &str, port: u16) -> Result<std::net::SocketAddr, ClientFehler> {
    let alle: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|e| ClientFehler::Netz(format!("DNS {host}: {e}")))?
        .collect();
    alle.iter()
        .find(|a| a.is_ipv4())
        .or_else(|| alle.first())
        .copied()
        .ok_or_else(|| ClientFehler::Netz(format!("keine Adresse fuer {host}:{port}")))
}

/// Sekunden seit 1970 fuer die Zertifikatspruefung.
fn jetzt_als_sekunden() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Zerlegt eine `https://host[:port]/pfad`-URL. M4: Host und Pfad werden **vor**
/// dem Kopieren geprueft.
fn zerlege_url(url: &str) -> Result<(String, u16, String), ClientFehler> {
    let ohne = url
        .strip_prefix("https://")
        .ok_or_else(|| ClientFehler::Url("nur https:// wird unterstuetzt".into()))?;
    let (host_teil, pfad) = match ohne.find('/') {
        Some(i) => (&ohne[..i], &ohne[i..]),
        None => (ohne, "/"),
    };
    // M4: der Host-Teil wird geprueft, BEVOR er am Doppelpunkt zerlegt wird.
    // 259 = 253 (maximale Hostlaenge laut RFC 3986) + 6 (":65535").
    if host_teil.len() > 259 {
        return Err(ClientFehler::Url("Host-Teil ist zu lang".into()));
    }
    let (host, port) = match host_teil.rsplit_once(':') {
        Some((h, p)) => match p.parse::<u16>() {
            // Port 0 ist ein Tippfehler (es gibt keine Verbindung zu Port 0),
            // und alles Nicht-Zifferige ist ebenso einer.
            Ok(0) | Err(_) => {
                return Err(ClientFehler::Url(format!("Port {p:?} unbrauchbar")));
            }
            Ok(port) => (h, port),
        },
        None => (host_teil, 443),
    };
    if host.is_empty() || host.len() > 253 || host.chars().any(char::is_whitespace) {
        return Err(ClientFehler::Url(format!("Host {host:?} unbrauchbar")));
    }
    if pfad.len() > 2048 {
        return Err(ClientFehler::Url("Pfad ist zu lang".into()));
    }
    Ok((host.to_string(), port, pfad.to_string()))
}

/// `datagramm_zerlegen` mit einer Fehlerumsetzung, die den Grund nennt.
/// Start einer Zeitmessung; **nur** wenn der Trace laeuft.
///
/// DER GRUND FUER DAS `if`: eine `Instant::now()` pro Paket kostet auf manchen
/// Systemen mehr, als die Messung wert ist — und ein Abruf mit 1 100 Paketen
/// zahlt das 1 100-mal. Ohne Trace soll der Client **keine** Zeitmessung
/// machen, die niemand liest.
fn mess_start() -> Option<std::time::Instant> {
    if std::env::var_os("NARSIL_QUIC_TRACE").is_some() {
        Some(std::time::Instant::now())
    } else {
        None
    }
}

/// Nanosekunden seit `start`, oder 0 wenn nicht gemessen wird.
fn mess_seit(start: Option<std::time::Instant>) -> u128 {
    start.map_or(0, |s| s.elapsed().as_nanos())
}

/// Die gemessene Verteilung, fuer die Spur.
fn mess_zeile(s: &Sitzung, gesamt: std::time::Duration) -> String {
    let ms = |n: u128| n / 1_000_000;
    // Q7.2: DIE RECHNUNG, und sie ist jetzt **beweispflichtig**.
    //
    // VORHER stand hier `gesamt - summe(phasen)` als „davon Netz". Das ist eine
    // **Restgroesse**, und sie enthaelt alles, was *nicht* gemessen wurde —
    // Wartezeit auf Pakete, Syscalls, `recv_from` selbst, und alles, was ich
    // nicht in eine Phase gepackt habe. „davon Netz 173 ms" las sich wie eine
    // Messung und war eine Vermutung.
    //
    // JETZT: die Wartezeit wird **direkt** gemessen (`wartezeit_gesamt`), und
    // der Rest heisst entsprechend „sonstiges" statt „Netz". Wer die Zahl
    // falsch liest, hat wenigstens eine falsche Zahl vor sich.
    let rechnen =
        s.zeit_entschluesseln + s.zeit_rahmen + s.zeit_h3 + s.zeit_kopieren + s.zeit_stream;
    let gesamt_ns = gesamt.as_nanos();
    format!(
        concat!(
            "Q7 VERTEILUNG: {} Pakete gelesen, {} entschluesselt | ",
            "rechnen {} ms (entschl {} / rahmen {} / h3 {} / kopieren {} / stream {}) | ",
            "WARTEZEIT {} ms ueber {} Pakete, groesste {} ms | ",
            "SENDE {} ms bei {} sendungen | ",
            "gesamt {} ms, davon sonstiges {} ms"
        ),
        s.pakete_gelesen,
        s.pakete_entschluesselt,
        ms(rechnen),
        ms(s.zeit_entschluesseln),
        ms(s.zeit_rahmen),
        ms(s.zeit_h3),
        ms(s.zeit_kopieren),
        ms(s.zeit_stream),
        ms(s.wartezeit_gesamt),
        s.wartezeit_echt,
        ms(s.wartezeit_max),
        ms(s.sendezeit.get()),
        s.sendungen.get(),
        ms(gesamt_ns),
        // Q7.2: Die Sendezeit tritt neben die Wartezeit. „Sonstiges" ist damit
        // eine **echte** Restgroesse und nicht mehr der Sammelbehaelter fuer
        // alles, was niemand gemessen hat.
        ms(gesamt_ns
            .saturating_sub(rechnen)
            .saturating_sub(s.wartezeit_gesamt)
            .saturating_sub(s.sendezeit.get())),
    )
}

/// Die angebotenen Versionen als lesbare Liste (`00000001, 6b3343cf`, …).
///
/// EINE EIGENE FUNKTION, weil die Liste an zwei Stellen gebraucht wird
/// (`Display` und `fehler_text`) und zwei Abschreibungen derselben Liste
/// irgendwann auseinanderlaufen — das ist bei `erste_range` in N42 genau so
/// passiert.
fn liste_versionen(v: &[u32]) -> String {
    v.iter()
        .map(|x| format!("{x:08x}"))
        .collect::<Vec<String>>()
        .join(", ")
}

fn zerlegen(daten: &[u8]) -> Result<Vec<(Ebene, &[u8])>, ClientFehler> {
    datagramm_zerlegen(daten).map_err(|e| ClientFehler::Handshake(e.to_string()))
}

/// Ein CONNECTION_CLOSE wird zu einem benannten Fehler.
fn geschlossen(code: u64, transport: bool, grund: &[u8]) -> ClientFehler {
    let mut text = String::from_utf8_lossy(grund).to_string();
    // M3: `grund` ist eine fremde Bytfolge. `truncate` begrenzt sie, damit
    // ein Server mit 60 000 Byte Grundtext nicht unseren Fehlertext aufblaeht.
    text.truncate(256);
    // N50: DER HINWEIS AUF DEN LEEREN GRUND wandert **hierher**, nicht in
    // `Display`.
    //
    // DER GRUND: In `Display` konnte ich die Leerheit nicht erkennen — dort
    // steht schon `Transport: `, und das ist **nicht** leer. Der Unterschied
    // zwischen „Server nannte keinen Grund" und „Parser hat welchen verloren"
    // ist nur **hier** sichtbar, weil nur hier der rohe Grundtext steht.
    //
    // DAS WAR DER AUFWAND: Der gemessene Fall (`example.org`, N50) sah in der
    // alten Ausgabe wie „Code 296, Grund fehlt" aus, und die naheliegende
    // Erklaerung war ein Fehler in **unserem** Grundtext-Parser. Der Satz hier
    // nimmt diese Erklaerung aus der Luft: er sagt, dass der Server nichts
    // geschickt hat — und dass der Unterschied trotzdem **nicht** gemessen ist.
    let leer = text.trim().is_empty();
    let marke = if leer {
        " (der Server hat **keinen** Grund genannt; alle Bytes des \
Grundtextfeldes waren leer)"
    } else {
        ""
    };
    ClientFehler::Geschlossen {
        code,
        grund: if transport {
            format!("Transport: {text}{marke}")
        } else {
            format!("{text}{marke}")
        },
    }
}

/// Die `CertificateVerify`-Signatur gegen das Blatt. Gespiegelt aus
/// `narsil_tls::strom`, damit die Formel nicht an zwei Stellen steht.
fn cv_gueltig(kette: &[narsil_tls::x509::Zertifikat], cv: &CertificateVerify, inhalt: &[u8]) -> bool {
    use narsil_tls::x509::SchluesselArt;
    let blatt = match kette.first() {
        Some(b) => b,
        None => return false,
    };
    match (&blatt.schluessel, cv.algorithmus) {
        (SchluesselArt::Rsa { n, e }, 0x0804 | 0x0401) => {
            narsil_tls::signatur::cv_pruefen(n, e, &cv.signatur, inhalt, cv.algorithmus).is_ok()
        }
        (SchluesselArt::Ecdsa { x, y, kurve }, 0x0403) => {
            // Nur P-256. Alles andere durchzulassen waere genau die Luecke,
            // mit der ein Server eine ungeprueffte Signatur durchbringe.
            if !matches!(
                kurve.as_slice(),
                [1, 2, 840, 10045, 3, 1, 7] | [1, 3, 132, 0, 7]
            ) {
                return false;
            }
            let parameter = match narsil_tls::ecdsa::kurve_p256() {
                Ok(p) => p,
                Err(_) => return false,
            };
            let teile = match narsil_tls::kette::ecdsa_teile_oeffentlich(&cv.signatur) {
                Ok(t) => t,
                Err(_) => return false,
            };
            narsil_tls::ecdsa::ecdsa_p256_pruefen_sha256(
                &parameter,
                x,
                y,
                &teile.0,
                &teile.1,
                &narsil_tls::sha2::sha256(inhalt),
            )
            .is_ok()
        }
        _ => false,
    }
}

/// Ein Nullschluessel. Nur als Startwert; er wird vor dem ersten Senden
/// ersetzt, `initial_geheimnis` laeuft vorher.
fn leerschluessel() -> PaketSchluessel {
    PaketSchluessel {
        suite: Suite::Aes128Gcm,
        schluessel: [0u8; 32],
        iv: [0u8; 12],
        hp: [0u8; 32],
    }
}

// ---------------------------------------------------------------------------
// Bauteile mit Grenzen
// ---------------------------------------------------------------------------

/// Die CRYPTO-Reassembly einer Ebene (RFC 9000 Abschnitt 19.6).
///
/// Der Puffer ist **fortlaufend**: eine Luecke wird nicht geschlossen, sondern
/// gewartet, bis die fehlenden Bytes kommen. `MAX_HANDSHAKE_PUFFER` (M4)
/// begrenzt, was eine boesartige Quelle an Speicher erzwingen kann.
#[derive(Debug, Default)]
struct CryptoReassembly {
    daten: Vec<u8>,
    /// Die Bytezahl, die als naechstes erwartet wird.
    naechster: u64,
}

impl CryptoReassembly {
    fn neu() -> Self {
        CryptoReassembly::default()
    }

    /// Setzt Daten ab `offset` ein. Ueberlappungen sind laut RFC erlaubt.
    fn einfuegen(&mut self, offset: u64, daten: &[u8]) -> Result<(), ClientFehler> {
        if offset > self.naechster {
            // Luecke: warten. Alles andere als Warten wuerde die Reihenfolge
            // zerstoeren.
            return Ok(());
        }
        let ueberschneidung = (self.naechster - offset) as usize;
        if ueberschneidung >= daten.len() {
            return Ok(()); // vollstaendiges Duplikat
        }
        let rest = &daten[ueberschneidung..];
        // M4: **vor** der Allokation.
        if self.daten.len() + rest.len() > MAX_HANDSHAKE_PUFFER {
            return Err(ClientFehler::Handshake(format!(
                "CRYPTO-Puffer voll (Grenze {MAX_HANDSHAKE_PUFFER} Byte)"
            )));
        }
        self.daten.extend_from_slice(rest);
        self.naechster += rest.len() as u64;
        Ok(())
    }
}

/// Pakete, fuer die es noch keinen Schluessel gibt (Q6a).
///
/// DER SINN: ein Server schickt Initial- und Handshake-Pakete gern in
/// DERSELBEN Datagramm. Die Handshake-Pakete lassen sich aber erst
/// entschluesseln, wenn das ServerHello da ist — also genau dann, wenn der
/// Schluessel entsteht. Ohne Puffer ginge so ein Paket verloren.
///
/// Die Grenze ist doppelt: hoechstens `MAX_GEPUFFERTE_PAKETE` Pakete und je
/// Paket hoechstens `MAX_GEPUFFERTES_PAKET` Byte (M3/M4). Wer mehr schickt,
/// verliert die Ueberschuesse — das ist eine bewusste Entscheidung: ein
/// Speicherlimit ist besser als unbegrenztes Wachsen.
struct PaketPuffer {
    pakete: Vec<Vec<u8>>,
    byte: usize,
    verworfen: usize,
}

impl PaketPuffer {
    fn neu() -> Self {
        PaketPuffer {
            pakete: Vec::new(),
            byte: 0,
            verworfen: 0,
        }
    }

    /// Die Zahl verworfener Pakete.
    ///
    /// DAS IST EINE MESSZAHL, kein Detail: sie sagt, ob die Grenze je
    /// gegriffen hat. Beim Client steht sie am Ende im Trace, damit ein
    /// auffaelliger Wert (dauerhaft > 0) im Bericht auftaucht und nicht
    /// stillschweigend Daten verliest.
    pub fn verworfen(&self) -> usize {
        self.verworfen
    }

    fn push(&mut self, paket: Vec<u8>) {
        if paket.len() > MAX_GEPUFFERTES_PAKET
            || self.pakete.len() >= MAX_GEPUFFERTE_PAKETE
            || self.byte + paket.len() > MAX_GEPUFFERTES_PAKET * MAX_GEPUFFERTE_PAKETE
        {
            self.verworfen += 1;
            return;
        }
        self.byte += paket.len();
        self.pakete.push(paket);
    }

    /// Wie viele Pakete liegen gerade im Puffer. Eine Messzahl fuer den Trace.
    fn anzahl(&self) -> usize {
        self.pakete.len()
    }

    /// Gibt alle gepufferten Pakete ab und leert den Puffer.
    fn entleere(&mut self) -> Vec<Vec<u8>> {
        let raus = std::mem::take(&mut self.pakete);
        self.byte = 0;
        raus
    }
}

/// Hoechste Zahl **unzusammenhängender** Stuecke in einem Antwortstrom (M3, N48).
///
/// DER WERT: bei `www.cloudflare-quic.com` waren es im Fehlerlauf rund ein
/// Dutzend. QUIC liefert STREAM-Daten in Paketen, und ein Paket kann mehrere
/// Stuecke enthalten; 32 Stuecke lassen 32 Pakete „vermischt" ankommen und
/// decken damit jede realistische Anordnung ab. Danach wird **nicht** mehr
/// gemerkt, sondern der Strom als **lueckenlos ab offen** behandelt — was
/// eine Falschmeldung waere, deshalb wird es in diesem Fall eine
/// benannte Fehlermeldung (N48).
pub const MAX_STROM_BEREICHE: usize = 32;

/// Ein Strompuffer (Q13).
#[derive(Debug, Default)]
struct StreamPuffer {
    daten: Vec<u8>,
    gesamt: usize,
    fin: bool,
    /// Q7: Wie weit der HTTP/3-Parser gekommen ist.
    ///
    /// DIE URSACHE DER HIERSTEHENDEN FELDER, gemessen: `antwort_lesbar` las
    /// den **ganzen** Puffer von vorn und wurde nach **jedem** STREAM-Frame
    /// gerufen. Fuer 1 343 809 Byte in 1 169 Paketen kostete das **456 ms von
    /// 822 ms** — die groesste einzelne Posten der Rechenzeit. Quadratisch,
    /// weil mit jedem Paket ein bereits gelesener Puffer erneut gelesen wurde.
    ///
    /// DER WEG ZUR UEBERPRUEFZAHL: Der Parser merkt sich, **wie weit** er
    /// gekommen ist, und macht dort weiter. Damit ist die Arbeit ueber den
    /// ganzen Abruf **linear** statt quadratisch, und `MAX_H3_FRAMES` bleibt
    /// eine Gesamtgrenze statt einer Grenze je Aufruf.
    h3_pos: usize,
    /// `:status` gesehen?
    h3_status: bool,
    /// `content-length` gesehen?
    h3_laenge: Option<usize>,
    /// DATA-Byte bisher gesehen.
    h3_body: usize,
    /// Bisher gelesene H3-Frames (zaehlt gegen `MAX_H3_FRAMES`).
    h3_frames: usize,
    /// N48: Die **tatsaechlich empfangenen** Byte-Bereiche, aufsteigend und
    /// zusammengefasst.
    ///
    /// DER GRUND, und er ist ein gemessener Fehlschlag: STREAM-Daten kommen
    /// **nicht in Reihenfolge** an. Fehlt ein mittleres Stueck, fuellt
    /// `resize` die Luecke mit **Nullen** — und `00 00` liest `h3_lesen` als
    /// zwei leere DATA-Frames. Bei `www.cloudflare-quic.com` im Fehlerlauf:
    /// der Parser kam auf **8,0 Byte je Frame** im Mittel und blieb bei
    /// `h3_pos = 32 927` von 126 194 Byte stehen, waehrend das FIN längst da
    /// war. Ursache: die Nullen explodierten die Frame-Zahl, das Budget
    /// `MAX_H3_FRAMES` (seit Q7 **kumulativ**) war erschoepft, und der Parser
    /// fror **stumm** ein — der Abruf lief in das Idle-Zeitlimit.
    ///
    /// DIE REGEL: **geparst wird nur, was ohne Luecke da ist.** Alles andere
    /// ist geraten, und geraten wird hier mit einer Fehlermeldung bestraft.
    bereiche: Vec<(usize, usize)>,
}

impl StreamPuffer {
    /// Den empfangenen Bereich `[von, bis)` eintragen (N48).
    ///
    /// Zusammenfassend, damit die Liste nicht mit jedem Paket wachsen muss:
    /// ein Bereich, der an einen vorhandenen angrenzt, wird **verschluckt**.
    fn merke_bereich(&mut self, von: usize, bis: usize) {
        if bis <= von {
            return;
        }
        self.bereiche.push((von, bis));
        self.bereiche.sort_unstable();
        let mut zusammen: Vec<(usize, usize)> = Vec::with_capacity(self.bereiche.len());
        for (a, b) in self.bereiche.drain(..) {
            match zusammen.last_mut() {
                // Angrenzend oder ueberlappend: **ein** Stueck daraus machen.
                Some(letztes) if a <= letztes.1 => letztes.1 = letztes.1.max(b),
                _ => zusammen.push((a, b)),
            }
        }
        self.bereiche = zusammen;
    }

    /// Bis wohin der Strom **ohne Luecke** empfangen wurde (N48).
    ///
    /// Beginningend bei 0, weil die Antwort mit den Kopfzeilen dort beginnt.
    /// Fehlt das **erste** Stueck, ist das 0 — und dann ist gar nichts lesbar.
    fn bis_lueckenlos(&self) -> usize {
        match self.bereiche.first() {
            Some((0, b)) => *b,
            _ => 0,
        }
    }

    /// Wie viele Stuecke der Strom hat (fuer Meldungen).
    fn stuecke(&self) -> usize {
        self.bereiche.len()
    }
}

/// Die gemerkten Paketnummern einer Ebene fuer den ACK (M3).
#[derive(Debug, Default)]
struct Verfolger {
    /// Aufsteigend, hoechstens `MAX_GEMERKTE_PAKETE` Eintraege.
    nummern: Vec<u64>,
    groesste: Option<u64>,
}

impl Verfolger {
    fn push(&mut self, pn: u64) {
        self.groesste = Some(self.groesste.map_or(pn, |g: u64| g.max(pn)));
        if self.nummern.contains(&pn) {
            return;
        }
        self.nummern.push(pn);
        self.nummern.sort_unstable();
        if self.nummern.len() > MAX_GEMERKTE_PAKETE {
            // M3: `checked_sub`. Die Bedingung oben garantiert das Ergebnis,
            // aber dieselbe Regel wie bei den Retry-Laengen: eine Subtraktion
            // auf einer Zahl, die aus fremden Daten stammt (die Paketnummern),
            // gehoert nicht auf eine Zusage an anderer Stelle.
            let weg = self.nummern.len().saturating_sub(MAX_GEMERKTE_PAKETE);
            self.nummern.drain(..weg);
        }
    }

    fn benoetigt_kein_ack(&self) -> bool {
        self.nummern.is_empty()
    }

    /// Q7: Die Liste der **unbestaetigten** Nummern leeren.
    ///
    /// DER GRUND, und er ist die halbe Fehlersuche dieser Runde: `nummern`
    /// bezeichnete "alles, was wir je gesehen haben". Damit war
    /// `benoetigt_kein_ack()` **nie** wahr, sobald einmal ein Paket kam — und
    /// `sende_handshake_ack` schickte bei **jedem** Datagramm ein Handshake-ACK,
    /// auch bei 1-RTT-Daten. Gemessen: **1 209** Handshake-ACKs waehrend eines
    /// Abrufs von 1,3 MB. 1 209 Pakete nach oben, die niemand gebraucht hat.
    ///
    /// NACH DEM ABSENDEN leeren, nicht vorher: geht das Paket nicht raus,
    /// waere die Quittung weg — und wir wuessten es nicht.
    ///
    /// (`groesste` bleibt stehen: es ist die groesste **gesehene** Nummer und
    /// gehoert in den naechsten ACK, auch wenn sich sonst nichts aendert.)
    fn entleere(&mut self) {
        self.nummern.clear();
    }

    /// `(groesste, erste_range, bereiche)` fuer einen ACK-Frame.
    ///
    /// Die Rechnung steht in `ack_bereiche` (frei zugänglich), damit der Test
    /// `tests/ack_luecken.rs` **dieselbe** Funktion trifft und nicht eine
    /// Kopie. Eine zweite Kopie waere genau der Fehler, den jener Test
    /// verhindert.
    fn rahmen(&self) -> Option<(u64, u64, Vec<AckBereich>)> {
        ack_bereiche(&self.nummern, self.groesste)
    }
}

/// Baut die drei Felder eines ACK-Frames aus einer aufsteigenden Liste
/// empfangener Paketnummern.
///
/// ## DER PANIK-BEFUND, DEN DIESE FUNKTION AUSGELOEST HAT
///
/// An echten Servern (google.com, cloudflare-quic.com), 01.10.2026:
///
/// ```text
/// attempt to subtract with overflow
/// --> crates/narsil-quic/src/client.rs:2413
/// ```
///
/// Dort stand `(nr + 1).saturating_sub(ende) - 1`. `saturating_sub` haelt das
/// Ergebnis bei 0, und **`0 - 1` laeuft unter**. Erreicht wird die Stelle
/// durch eine **Luecke in der Folge der empfangenen Paketnummern** — also
/// genau dann, wenn der Server ein Paket verloren hat und das nächste schickt.
/// Das ist im Internet der Normalfall, nicht der Ausnahmefall, und fremde
/// Paketnummern sind damit ein Weg, den Client abzustuerzen (M3).
///
/// ## DIE KONKRETE FORMEL (RFC 9000 Abschnitt 19.3)
///
/// `nummern` aufsteigend, `groesste` = groesste empfangene Nummer. Wir gehen
/// von oben nach unten und bilden zusammenhaengende Bereiche. Fuer den
/// naechsten, tieferen Bereich mit der groessten Nummer `nr` gilt:
///
/// * fehlende Pakete zwischen `obere` und `nr`: `obere - nr - 1`,
/// * das **Gap-Feld** des ACK-Frames ist diese Zahl **minus eins**, weil das
///   Feld laut RFC die Zahl der fehlenden Pakete *zuzueglich* eins nennt.
///
/// ## DIE REGEL, DIE DARAUS FOLGT
///
/// Jede Arithmetik hier ist `checked_*`. Was nicht darstellbar ist, wird
/// **weggelassen** statt gerechnet: ein fehlender ACK-Bereich kostet den Peer
/// eine erneute Sendung, eine Panik kostet den ganzen Prozess. Und die leere
/// Liste wird VOR jeder Index-Arithmetik geprueft — `nummern.len() - 1` waere
/// dort schon der zweite Fund derselben Art.
pub fn ack_bereiche(nummern: &[u64], groesste: Option<u64>) -> Option<(u64, u64, Vec<AckBereich>)> {
    // `None` heisst "nichts empfangen" — dann gibt es kein ACK. Das wird
    // **vor** der Leer-Pruefung entschieden, weil beides `None` ergibt und
    // ein leeres `nummern` bei vorhandenem `groesste` der Fall ist, den der
    // Fuzz-Fund aus U4 betraf.
    //
    // `mut`, weil der Wert unten noch korrigiert werden **muss**: steht er
    // nicht in der Liste, gilt der groesste Eintrag (RFC 9000 19.3.1).
    let mut groesste = groesste?;
    // M3: die Leer-Pruefung VOR jeder Index-Arithmetik. Ohne das waere
    // `nummern.len() - 1` bei einer leeren Liste schon der naechste Unterlauf
    // an derselben Stelle.
    if nummern.is_empty() {
        return None;
    }
    // `nummern` kommt aus `Verfolger` und ist dort aufsteigend gehalten. Der
    // Aufrufer kann aber eine beliebige Reihenfolge geben (der Test tut das
    // absichtlich), deshalb wird hier einmal sortiert und entdoppelt.
    let mut sortiert: Vec<u64> = nummern.to_vec();
    sortiert.sort_unstable();
    sortiert.dedup();
    // M3 (U4): `groesste` muss eine **wirklich empfangene** Nummer sein
    // (RFC 9000 19.3.1, "Largest Acknowledged"). Steht sie nicht in der Liste,
    // gewinnt der groesste Eintrag der Liste — sonst wuerde das ACK eine
    // Nummer bestaetigen, die nie kam. Begruendung und Fuzz-Fund stehen am
    // `debug_assert_eq!` weiter unten.
    //
    // "Largest Acknowledged" heisst: **bis zu** dieser Nummer bestaetige ich.
    // Was ich darueber empfangen habe, bestaetige ich **nicht** — es wartet im
    // Puffer auf ein spaeteres ACK. Deshalb wird die Liste hier auf
    // `<= groesste` gekuerzt; ohne das wuerde `bereiche[0].0` die groesste
    // Nummer der Liste sein und nicht `groesste`, und das ACK waere falsch.
    //
    // Steht `groesste` nicht in der Liste (der Fund aus U4), gilt der groesste
    // Eintrag: eine Nummer zu bestaetigen, die nie ankam, ist nach
    // RFC 9000 19.3.1 nicht moeglich.
    match sortiert.binary_search(&groesste) {
        Ok(stelle) => {
            // `stelle` ist der Index **von** `groesste`; alles dahinter wird
            // abgeschnitten. `truncate` nimmt die Laenge, also `stelle + 1`.
            sortiert.truncate(stelle + 1);
        }
        Err(_) => {
            // `sortiert` ist nach `is_empty()` **nicht leer** (oben geprueft)
            // und nach `dedup()` aufsteigend, also ist `last()` der groesste.
            // Das `unwrap_or(&0)` ist trotzdem da: eine Zusage einer anderen
            // Stelle ist kein `unwrap`.
            groesste = *sortiert.last().unwrap_or(&0);
        }
    }

    // --- 1) Bereiche bilden: eine Liste `(obergrenze, untergrenze)`, von
    // --- oben nach unten. Jede Zahl kommt aus fremden Daten, deshalb nur
    // --- `checked_add` beim Test "haengt die naechste direkt drunter".
    let mut bereiche: Vec<(u64, u64)> = Vec::new();
    let mut i = sortiert.len() - 1;
    let mut obere = sortiert[i];
    let mut untere = obere;
    while i > 0 {
        // `i > 0` ist die Bedingung, also ist `i - 1` sicher.
        let nr = sortiert[i - 1];
        if nr.checked_add(1) == Some(untere) {
            untere = nr;
            i -= 1;
            continue;
        }
        bereiche.push((obere, untere));
        obere = nr;
        untere = nr;
        i -= 1;
    }
    bereiche.push((obere, untere));
    //
    // DIE REIHENFOLGE, und hier stand ein echter Fehler: die Schleife laeuft
    // von der groessten Nummer herunter und haengt bei jeder Luecke den gerade
    // geschlossenen Bereich an — also **von oben nach unten**. Index 0 ist
    // damit bereits der oberste Bereich, genau wie in RFC 9000 19.3.1.
    //
    // Hier stand ein `bereiche.reverse()` mit der Begruendung "die Schleife
    // laeuft aufsteigend". Sie laeuft aber absteigend, und das `reverse()`
    // hat den Bereich mit der groessten Nummer an die **letzte** Stelle
    // verschoben. Damit waere `erste_range` aus dem falschen Bereich
    // berechnet worden: kein Panic, aber ein **falsches ACK**, das den Peer
    // Pakete doppelt schicken laesst und den Server glauben laesst, er habe
    // etwas empfangen, das er nicht empfangen hat. Gefangen von
    // `ack_bereiche_surviven_luecken_und_grenzen` in `tests/ack_luecken.rs`,
    // Fall 1: bei den Nummern 5 und 7 muss `erste_range` 0 sein (der oberste
    // Bereich umfasst nur Paket 7), und mit dem `reverse()` kam 2 heraus.
    //
    // UND JETZT DIE STELLE, DIE DER FUZZER GEFUNDEN HAT (01.10.2026, U4):
    //
    // ```text
    // assertion `left == right` failed: M3: bereiche[0] muss die groesste
    // Nummer tragen
    //   left: 16916374238009499680
    //  right: 0
    // --> crates/narsil-quic/src/client.rs:3088
    // ```
    //
    // DER FEHLER: `groesste` ist ein **Parameter**, und nichts hinderte den
    // Aufrufer daran vorbeizugeben, eine Nummer, die in `nummern` gar nicht
    // vorkommt. Bei `nummern = [X]` und `groesste = Some(0)` ist
    // `bereiche[0].0 = X != 0`, und der `debug_assert` — der als Behauptung
    // gemeint war — paniked. Und weil `bau.sh` **Debug** baut, ist das kein
    // theoretischer Pfad, sondern der normale.
    //
    // DIE RICHTIGE FORMULIERUNG (RFC 9000 19.3.1): "Largest Acknowledged" ist
    // ein Paket, das der Empfaenger **wirklich empfangen** hat. Ein `groesste`
    // ausserhalb der Liste ist damit nicht bloss ungewoehnlich, sondern
    // **unmoeglich** — und die Funktion darf daraus kein ACK bauen, das dem
    // Peer eine Nummer bestaetigt, die nie kam.
    //
    // BEHEBUNG: `groesste` wird gegen die Liste geprueft und durch deren
    // groessten Eintrag ersetzt, wenn es nicht drin steht. Im Client
    // (`Verfolger`) kommt das nie vor, weil dort `groesste` und `nummern`
    // gemeinsam gepflegt werden — deshalb fiel es erst dem Fuzzer auf, und
    // nicht einem Test. `debug_assert_eq!` darf eine **Einladung**
    // dokumentieren, aber sie darf nicht die **Tuer** sein.
    //
    // Die Behauptung, die jetzt wirklich gilt: der oberste Bereich traegt
    // genau `groesste`. Nach dem Kuerzen ist das nicht nur wahr, es ist die
    // **Bedingung** dafuer, dass die Zahlenfelder des ACK ueberhaupt stimmen —
    // ohne sie waere `erste_range` aus dem falschen Bereich gerechnet.
    debug_assert_eq!(
        bereiche[0].0, groesste,
        "M3: bereiche[0] traegt die groesste bestaetigte Nummer"
    );

    // --- 2) Das erste Feld: Zahl der Pakete **unterhalb** der groessten
    // --- Nummer im obersten Bereich (RFC 9000 19.3.1, "First ACK Range").
    let (_, erste_untergrenze) = bereiche[0];
    let erste_range = groesste.saturating_sub(erste_untergrenze);

    // --- 3) Die weiteren Bereiche. Fuer Bereich `j` (j >= 1) gilt nach
    // --- RFC 9000 19.3.1:
    // ---     Gap  = fehlende Pakete zwischen den Bereichen **minus eins**,
    // ---            wobei "fehlend" = `bereiche[j-1].1 - bereiche[j].0 - 1`,
    // ---     Laenge = Zahl der Pakete im Bereich **abzueglich eins**.
    // --- Also: Gap = `bereiche[j-1].1 - bereiche[j].0 - 2`.
    let mut raus: Vec<AckBereich> = Vec::new();
    for j in 1..bereiche.len() {
        if raus.len() >= MAX_ACK_BEREICHE {
            // Die Grenze ist `MAX_ACK_BEREICHE` (M3); was drueber liegt, wird
            // nicht gemeldet. Ein Peer wiederholt dann sein Paket, ein Prozess
            // stirbt nicht.
            break;
        }
        let (_, vorige_untergrenze) = bereiche[j - 1];
        let (obere_j, untere_j) = bereiche[j];
        // Fehlende Pakete: zwischen `untere_j + 1` und `vorige_untergrenze - 1`.
        let fehlende = vorige_untergrenze
            .checked_sub(obere_j)
            .and_then(|d| d.checked_sub(1));
        let laenge = obere_j.checked_sub(untere_j);
        match (fehlende, laenge) {
            // Nur ein **einzelnes** fehlendes Paket ergibt Gap 0; das ist
            // gueltig. Bei zwei oder mehr ist das Gap-Feld `fehlende - 1`.
            // `fehlende == 0` kann nicht vorkommen (dann haette der Bereich
            // direkt angeschlossen und es gaebe keinen zweiten Bereich), und
            // wird deshalb abgewiesen statt gerechnet.
            (Some(f), Some(l)) if f >= 1 && l < (1u64 << 62) => raus.push(AckBereich {
                luecke: f - 1,
                laenge: l,
            }),
            // Alles andere (Ueberlauf, nicht darstellbar) wird **weggelassen**.
            // Ein fehlender Bereich kostet eine erneute Sendung; eine Panik
            // kostet den Prozess.
            _ => continue,
        }
    }
    Some((groesste, erste_range, raus))
}
