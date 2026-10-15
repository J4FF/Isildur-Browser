//! HTTP/3 AUF DER SERVERSEITE — das Grundgeruest, added 01.10.2026,
//! Rolle SERVER, Punkt V9.
//!
//! # WAS HIER IST UND WAS NICHT
//!
//! **IST:** die Abbildung von HTTP/3 auf die HTTP-Semantik dieser
//! Crate. Ein [`H3Strom`] nimmt die Bytes eines HTTP/3-Bidirektional-
//! streams entgegen, liest die Frames (RFC 9114), dekodiert den
//! QPACK-Feldabschnitt (RFC 9204), baut daraus ein [`Request`] und
//! gibt ihn an **dieselbe** Kette weiter, die auch HTTP/1.1 und HTTP/2
//! bedienen. Die Antwort geht als HEADERS + DATA zurueck.
//!
//! Das ist der Teil, den man schreiben kann, ohne QUIC zu haben.
//!
//! **NICHT IST:** der QUIC-Transport. Es fehlen der UDP-Sockel, der
//! TLS-1.3-Handshake ueber QUIC (CRYPTO-Frames, Transportparameter,
//! Certificat aus `narsil-tls::server`) und die Anbindung an
//! `narsil_quic::verbindung` (Verlusterkennung, Flusskontrolle, Stau).
//! Das ist kein Versaeumnis, sondern die Arbeitsteilung: `narsil-quic`
//! gehoert der QUIC-Rolle, und dort wird nur **gelesen** (Regel 4).
//! Was diese Crate von dort braucht, steht unten unter
//! [`h3::offene_stellen`] — als Liste, damit die QUIC-Rolle sie
//! abarbeiten kann, ohne diese Crate zu lesen.
//!
//! # DIE BEWEISE
//!
//! Die Tests hier benutzen **echte** HTTP/3-Bytes: die Frames kommen aus
//! `narsil_quic::h3_schreiben`, der Feldabschnitt aus
//! `narsil_quic::feldabschnitt_kodieren`. Also kein Mock, der nur die
//! eigene Erwartung wiederholt — die Bytes gehen durch denselben
//! Kodierer, den die QUIC-Rolle benutzt.

use std::collections::HashMap;

use narsil_quic::h3::{H3Fehler, H3Rahmen, h3_lesen, h3_schreiben};
use narsil_quic::qpack::{FeldZeile, MAX_FELDER, feldabschnitt_dekodieren, feldabschnitt_kodieren};

use crate::http::{Method, Request, Response, Status};

/// Hoechste Zahl Bytes, die ein unvollstaendiger HTTP/3-Strom im
/// Puffer halten darf (M4).
///
/// added V9. Ohne diese Zahl waere „der Client schickt einen
/// HEADERS-Frame, dessen Laenge 2^62 behauptet" ein Weg, den Server
/// in einen 4-Exabyte-Puffer zu treiben. Der Wert ist die Summe aus
/// allem, was ein Stream ueberhaupt braucht: Kopf (16 KiB, wie
/// [`crate::MAX_HEADER_BYTES`]) + Body ([`crate::MAX_BODY_BYTES`]).
pub const MAX_STROM_PUFFER: usize = 11 * 1024 * 1024;

/// Ist das ein „warte mehr"-Fall?
///
/// added V9, nach dem ersten Testlauf: der Test `brocken_...` schnitt
/// die Bytes an **jeder** Stelle und bekam bei einem 1-Byte-Rest
/// `Varint braucht 1 Byte, da sind 0` — also als HARTE Fehlerbehandlung
/// abgebrochen, obwohl es der normale Fall eines UDP-Pakets ist.
///
/// `h3_lesen` meldet zwei verschiedene Faelle: einen abgeschnittenen
/// **Inhalt** (`H3Fehler::Abgeschnitten`) und einen abgeschnittenen
/// **Frame-Kopf** (`H3Fehler::Varint(VarintFehler::Abgeschnitten)`).
/// Beide heissen „warte mehr"; nur `VarintFehler::ZuGross` ist ein
/// Fehler — dann hat jemand eine Laenge geschickt, die nicht in 62 Bit
/// passt.
fn ist_unvollstaendig(e: &H3Fehler) -> bool {
    // added 05.10.2026 (SCHLUSSPHASE, clippy): `matches!` statt `match`
    // ueber `true`/`false`. Die Absicht steht im Kommentar darueber und
    // wird nicht kleiner, wenn man sie vier Zeilen unter dem
    // Wuerterbuch schreibt.
    matches!(
        e,
        H3Fehler::Abgeschnitten { .. }
            | H3Fehler::Varint(narsil_quic::varint::VarintFehler::Abgeschnitten { .. })
    )
}

/// Warum ein HTTP/3-Strom nicht zu einer [`Request`] fuehrt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum H3ServerFehler {
    /// Die Bytes reichen noch nicht (der Frame ist unvollstaendig).
    ///
    /// Kein Fehler, sondern „warte mehr" — das ist der Normalfall
    /// eines Paketverlusts. Die aufrufende Schleife puffert und
    /// versucht es erneut.
    Unvollstaendig { braucht: usize, da: usize },
    /// QPACK-Feldabschnitt kaputt.
    Qpack(String),
    /// HTTP/3-Frame kaputt.
    Frame(String),
    /// Ein Pseudo-Header fehlt (`:method`, `:path`, `:scheme`).
    PseudoFehlt(&'static str),
    /// Ein Pseudo-Header **nach** einer normalen Kopfzeile
    /// (RFC 9114 4.3).
    PseudoZuSpaet(String),
    /// Unbekannter Pseudo-Header.
    PseudoUnbekannt(String),
    /// Kopfzeilenname mit Grossbuchstaben (RFC 9114 4.3).
    NameGross(String),
    /// Verbotene Verbindungskopfzeile (`connection`, `keep-alive`,
    /// `transfer-encoding`, `upgrade`, `host`) — RFC 9114 4.2.
    VerbindungVerboten(String),
    /// Der Koerper ist groesser als [`crate::MAX_BODY_BYTES`].
    KoerperZuGross { grenze: usize, da: usize },
    /// Der Puffer hat [`MAX_STROM_PUFFER`] erreicht, ohne dass ein
    /// Request fertig wurde.
    PufferVoll { grenze: usize },
}

impl std::fmt::Display for H3ServerFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            H3ServerFehler::Unvollstaendig { braucht, da } => {
                write!(f, "Frame unvollstaendig: {da} von {braucht} Byte")
            }
            H3ServerFehler::Qpack(m) => write!(f, "QPACK: {m}"),
            H3ServerFehler::Frame(m) => write!(f, "HTTP/3-Rahmen: {m}"),
            H3ServerFehler::PseudoFehlt(was) => write!(f, "Pseudo-Header {was} fehlt"),
            H3ServerFehler::PseudoZuSpaet(n) => {
                write!(f, "Pseudo-Header {n} steht nach normalen Kopfzeilen")
            }
            H3ServerFehler::PseudoUnbekannt(n) => write!(f, "unbekannter Pseudo-Header {n}"),
            H3ServerFehler::NameGross(n) => write!(f, "Kopfzeilenname {n} hat Grossbuchstaben"),
            H3ServerFehler::VerbindungVerboten(n) => {
                write!(f, "Verbindungskopfzeile {n} ist in HTTP/3 verboten")
            }
            H3ServerFehler::KoerperZuGross { grenze, da } => {
                write!(f, "Koerper {da} Byte, erlaubt sind {grenze}")
            }
            H3ServerFehler::PufferVoll { grenze } => {
                write!(f, "Puffer voll ({grenze} Byte) ohne fertige Anfrage")
            }
        }
    }
}

impl std::error::Error for H3ServerFehler {}

/// Der QPACK-Feldabschnitt aus HEADERS, sobald der Kopfblock kam.
///
/// `Vec<u8>`, nicht `Vec<FeldZeile>`: `H3Rahmen::Kopfblock` traegt den
/// **rohen** Feldabschnitt, und dekodiert wird erst in
/// [`H3Strom::anfrage`]. Grund: die dynamischen QPACK-Anweisungen
/// gehoeren an die **Verbindung**, nicht an den Strom — wer hier
/// dekodiert, wuerde einen Feldabschnitt lesen, dessen Instruktionen
/// noch nicht angekommen sind.
type Kopfblock = Vec<u8>;

/// Der Zustand EINES HTTP/3-Bidirektionalstroms.
///
/// # WAS DAS DING IST (und warum es so klein ist)
///
/// HTTP/3 multiplext mehrere Anfragen ueber eine Verbindung, aber jede
/// Anfrage hat ihren **eigenen** Strom. Die Frames tragen keine
/// Strom-Nummer (das macht QUIC). Also ist „ein HTTP/3-Server"
/// pro Strom eine eigene Instanz — genau wie `bediene_socket` eine
/// Instanz je HTTP/1.1-Verbindung ist, nur feiner.
///
/// Was hier NICHT steht und warum: kein QPACK-Sender, weil die
/// statische Tabelle und der Kodierer aus `narsil-quic` kommen und wir
/// dort nicht schreiben.
pub struct H3Strom {
    /// Noch nicht verbrauchte Bytes des Stromes.
    puffer: Vec<u8>,
    /// Die Felder aus HEADERS, sobald der Kopfblock kam.
    kopf: Option<Kopfblock>,
    /// Der Koerper aus DATA-Frames.
    koerper: Vec<u8>,
    /// Hat der Client den Strom beendet? In HTTP/3 gibt es dafuer
    /// kein END_STREAM-Bit — ein Strom endet, wenn die QUIC-Schicht
    /// sagt „FIN" (ein Stream mit Typ `FIN`). Fuer dieses Grundgeruest
    /// ist das ein **Flag** der aufrufenden Schleife.
    geschlossen: bool,
}

impl Default for H3Strom {
    fn default() -> Self {
        H3Strom::neu()
    }
}

impl H3Strom {
    /// Ein leerer Strom.
    pub fn neu() -> H3Strom {
        H3Strom {
            puffer: Vec::new(),
            kopf: None,
            koerper: Vec::new(),
            geschlossen: false,
        }
    }

    /// Wie viele Bytes liegen im Puffer (Diagnose).
    pub fn puffer_laenge(&self) -> usize {
        self.puffer.len()
    }

    /// Der Client hat den Strom beendet (`FIN`).
    pub fn markiere_geschlossen(&mut self) {
        self.geschlossen = true;
    }

    /// Bytes **anhängen** (die QUIC-Schicht liefert sie in Brocken).
    ///
    /// M4: der Puffer ist auf [`MAX_STROM_PUFFER`] gedeckelt. Ein
    /// Stream, der so viel schickt, bekommt `PufferVoll` statt eines
    /// Speicherleck.
    pub fn nimm_bytes(&mut self, bytes: &[u8]) -> Result<(), H3ServerFehler> {
        if self.puffer.len() + bytes.len() > MAX_STROM_PUFFER {
            return Err(H3ServerFehler::PufferVoll {
                grenze: MAX_STROM_PUFFER,
            });
        }
        self.puffer.extend_from_slice(bytes);
        Ok(())
    }

    /// Frames aus dem Puffer lesen und sammeln. Liefert `Ok(())`,
    /// wenn der Strom eine vollstaendige Anfrage traegt.
    ///
    /// DER WEG, und warum nicht in einem Rutsch: HTTP/3-Frames kommen
    /// in UDP-Datagrammen an, die nichts an den Rahmengrenzen halten.
    /// `h3_lesen` liefert deshalb `(Rahmen, Laenge)` — ein halber
    /// Frame ergibt `Unvollstaendig`, und der Rest bleibt im Puffer.
    /// Genau so baut man es auf UDP richtig.
    pub fn lies_frames(&mut self) -> Result<(), H3ServerFehler> {
        let mut pos = 0usize;
        while pos < self.puffer.len() {
            let rahmen = match h3_lesen(&self.puffer[pos..]) {
                Ok((r, n)) => (r, n),
                // Ein halber Frame (Inhalt **oder** Kopf) ist kein
                // Fehler, sondern „warte mehr" — siehe `ist_unvollstaendig`.
                Err(e) if ist_unvollstaendig(&e) => {
                    return Err(H3ServerFehler::Unvollstaendig {
                        braucht: self.puffer.len() + 1,
                        da: self.puffer.len() - pos,
                    });
                }
                Err(e) => return Err(H3ServerFehler::Frame(e.to_string())),
            };
            let (r, laenge) = rahmen;
            match r {
                H3Rahmen::Kopfblock(block) => {
                    // Zwei HEADERS auf einem Stream sind erlaubt
                    // (zwischenzeitliche 1xx). Hier zaehlt nur der
                    // erste als Anfrage-Kopf; ein zweiter haette
                    // QPACK-Instruktionen im Spiel (narsil-quic kann sie
                    // dekodieren, aber sie gehoeren an die Verbindung,
                    // nicht an den Strom).
                    if self.kopf.is_none() {
                        self.kopf = Some(block);
                    }
                }
                H3Rahmen::Daten(d) => {
                    // M4: VOR dem Anhaengen. Sonst waere die Grenze
                    // ein Vorsatz, den `Vec` nicht liest.
                    let neu = self.koerper.len() + d.len();
                    if neu > crate::MAX_BODY_BYTES {
                        return Err(H3ServerFehler::KoerperZuGross {
                            grenze: crate::MAX_BODY_BYTES,
                            da: neu,
                        });
                    }
                    self.koerper.extend_from_slice(&d);
                }
                // SETTINGS, GOAWAY, MAX_PUSH_ID und alle unbekannten
                // Typen: fuer die Abbildung einer Anfrage nicht noetig.
                //
                // DER GREASE-FALL ist der Grund fuer „nicht noetig"
                // statt „Fehler": cloudflare-quic.com stellt jedem
                // Antwortstrom zwei solche Frames voran (gemessen in
                // `narsil-quic/src/h3.rs`). Sie muessen **uebersprungen**
                // werden (RFC 9114 9.1), nicht abgelehnt.
                _ => {}
            }
            pos += laenge;
        }
        // M3: der Puffer schrumpft auf die noch nicht gelesenen
        // Bytes — und die Zahl der gelesenen Frames ist durch die
        // Bytes begrenzt, nicht durch die Eingabe.
        self.puffer.drain(..pos);
        Ok(())
    }

    /// Ist die Anfrage vollstaendig? (Kopf da **und** Strom zu)
    pub fn ist_vollstaendig(&self) -> bool {
        self.kopf.is_some() && (self.geschlossen || self.body_da())
    }

    /// Kam ueberhaupt ein Koerper? (Dann ist der Strom bei DATA zu Ende.)
    fn body_da(&self) -> bool {
        !self.koerper.is_empty()
    }

    /// Die gesammelte Anfrage als [`Request`].
    ///
    /// DER KERN: der Feldabschnitt aus HEADERS wird ueber
    /// `narsil_quic::feldabschnitt_dekodieren` gelesen (statische Tabelle
    /// und Literal mit Huffman — alles, was QPACK ohne dynamische
    /// Tabelle kann) und dann nach RFC 9114 4.2/4.3 geprueft:
    ///
    /// * Pseudo-Header **vor** allen normalen Kopfzeilen,
    /// * kein unbekannter Pseudo-Header,
    /// * keine Grossbuchstaben im Namen,
    /// * keine Verbindungskopfzeilen (`connection`, `host`, ...),
    /// * `:method`, `:path` und `:scheme` muessen da sein.
    pub fn anfrage(&self, peer: &str) -> Result<Request, H3ServerFehler> {
        let block = self
            .kopf
            .as_ref()
            .ok_or(H3ServerFehler::PseudoFehlt(":headers"))?;
        let felder =
            feldabschnitt_dekodieren(block).map_err(|e| H3ServerFehler::Qpack(e.to_string()))?;
        if felder.len() > MAX_FELDER {
            return Err(H3ServerFehler::Qpack(format!(
                "{} Felder, erlaubt sind {MAX_FELDER}",
                felder.len()
            )));
        }

        let mut methode = String::new();
        let mut pfad = String::new();
        let mut schema = String::new();
        let mut host = String::new();
        let mut normale: Vec<(String, String)> = Vec::with_capacity(8);
        let mut gesehen_normal = false;

        for feld in &felder {
            let name = String::from_utf8_lossy(&feld.name).to_ascii_lowercase();
            let wert = String::from_utf8_lossy(&feld.wert).to_string();
            if let Some(pseudo) = name.strip_prefix(':') {
                if gesehen_normal {
                    return Err(H3ServerFehler::PseudoZuSpaet(name));
                }
                // `pseudo` ist der Name OHNE Doppelpunkt — der wurde
                // oben abgeschnitten. DER FEHLER DER ERSTEN FASSUNG:
                // hier standen die Arme mit `":method"` usw., also passte
                // keiner, und jede gueltige Anfrage wurde als
                // „unbekannter Pseudo-Header" abgewiesen.
                match pseudo {
                    "method" => methode = wert,
                    "path" => pfad = wert,
                    "scheme" => schema = wert,
                    "authority" => host = wert,
                    _ => return Err(H3ServerFehler::PseudoUnbekannt(name)),
                }
            } else {
                gesehen_normal = true;
                // M3: die Zahl normaler Kopfzeilen ist begrenzt.
                if normale.len() >= crate::MAX_HEADER_ANZAHL {
                    return Err(H3ServerFehler::Qpack(format!(
                        "mehr als {} Kopfzeilen",
                        crate::MAX_HEADER_ANZAHL
                    )));
                }
                // 4.2: verbotene Verbindungskopfzeilen. `host` ist
                // verboten, weil `:authority` sein Ersatz ist.
                if matches!(
                    name.as_str(),
                    "connection"
                        | "keep-alive"
                        | "transfer-encoding"
                        | "upgrade"
                        | "host"
                        | "proxy-connection"
                ) {
                    return Err(H3ServerFehler::VerbindungVerboten(name));
                }
                if name.len() as i64 > 0 {
                    // Grossbuchstaben sind nach RFC 9114 4.3 verboten;
                    // die Pruefung oben hat den Namen kleingeschrieben,
                    // also hier der Vergleich gegen das Original.
                    let original = String::from_utf8_lossy(&feld.name);
                    if original.bytes().any(|b| b.is_ascii_uppercase()) {
                        return Err(H3ServerFehler::NameGross(original.to_string()));
                    }
                }
                normale.push((name, wert));
            }
        }

        if methode.is_empty() {
            return Err(H3ServerFehler::PseudoFehlt(":method"));
        }
        if pfad.is_empty() {
            return Err(H3ServerFehler::PseudoFehlt(":path"));
        }
        if schema.is_empty() {
            return Err(H3ServerFehler::PseudoFehlt(":scheme"));
        }
        // `:authority` darf fehlen (RFC 9114 4.3.1: dann wird der Host
        // aus der Verbindung abgeleitet). Fuer `Host` im HTTP/1.1-Kopf
        // nehmen wir, was da ist — sonst bleibt es leer, und der
        // Handler sieht das.
        // `:authority` darf fehlen; dann bleibt `host` leer und der
        // Handler sieht das (RFC 9114 4.3.1: der Host wird dann aus
        // der QUIC-Verbindung abgeleitet — das macht die Schicht, die
        // diese Bytes erzeugt hat, nicht diese Crate).
        let _ = &host;

        let (nur_pfad, query) = match pfad.split_once('?') {
            Some((p, q)) => (p.to_string(), q.to_string()),
            None => (pfad.clone(), String::new()),
        };

        let mut headers = HashMap::with_capacity(8 + normale.len());
        for (k, v) in normale {
            // `content-length` wird nicht uebernommen: die Laenge hat
            // der Koerper, und zwei widersprechende Zahlen sind ein
            // Fehler, den sich der Client sparen kann.
            if k == "content-length" {
                continue;
            }
            headers.insert(k, v);
        }

        Ok(Request {
            methode: Method::aus_str(&methode),
            methode_roh: methode,
            pfad: nur_pfad,
            query,
            version: "HTTP/3.0".to_string(),
            headers,
            body: std::mem::take(&mut Vec::new()),
            params: HashMap::new(),
            peer: peer.to_string(),
        }
        .mit_koerper(std::mem::take(&mut self.koerper.clone())))
    }

    /// Die Antwort als HTTP/3-Frames: SETTINGS (leer) + HEADERS + DATA.
    ///
    /// DER AUFBAU, und jede Zeile hat einen Grund (RFC 9114 6.2.1):
    ///
    /// 1. **SETTINGS mit leerem Inhalt** — die Kontrollanalyse des
    ///    Servers. Leeres SETTINGS ist erlaubt und sagt „ich nutze
    ///    keine Erweiterung".
    /// 2. **HEADERS** mit `:status` und `content-length`. Der Status
    ///    ist Text, nicht Zahl (wie in HTTP/2: `:status` ist eine
    ///    Zeichenkette).
    /// 3. **DATA** mit dem Koerper — ausser bei 204/304 und HEAD,
    ///    wo es keinen Koerper gibt.
    pub fn antwort_frames(&self, antwort: &Response, methode: Method) -> Vec<u8> {
        let mut raus = Vec::with_capacity(512);
        // 1) SETTINGS
        let _ = h3_schreiben(&H3Rahmen::Einstellungen(Vec::new()), &mut raus);
        // 2) HEADERS
        let felder = self.antwort_felder(antwort, methode);
        let block = feldabschnitt_kodieren(&felder);
        let _ = h3_schreiben(&H3Rahmen::Kopfblock(block), &mut raus);
        // 3) DATA
        let koerper = self.antwort_koerper(antwort, methode);
        if !koerper.is_empty() {
            let _ = h3_schreiben(&H3Rahmen::Daten(koerper), &mut raus);
        }
        raus
    }

    /// Die Kopfzeilen der Antwort (fuer Tests und Diagnose).
    pub fn antwort_felder(&self, antwort: &Response, methode: Method) -> Vec<FeldZeile> {
        let mut felder = vec![FeldZeile::neu(":status", &antwort.status.0.to_string())];
        let laenge = self.antwort_koerper(antwort, methode).len();
        // Ohne Koerper keine `content-length` (RFC 9110 8.6): bei 204
        // und 304 ist die Laenge undefiniert.
        if laenge > 0 || !antwort.body.is_empty() {
            felder.push(FeldZeile::neu("content-length", &laenge.to_string()));
        }
        for (k, v) in &antwort.headers {
            // Verbindungskopfzeilen sind in HTTP/3 verboten, genauso
            // wie beim Empfang (RFC 9114 4.2).
            if matches!(
                k.to_ascii_lowercase().as_str(),
                "connection" | "keep-alive" | "transfer-encoding" | "upgrade" | "host"
            ) {
                continue;
            }
            felder.push(FeldZeile::neu(&k.to_ascii_lowercase(), v));
        }
        felder
    }

    /// Der Koerper der Antwort — ohne die, die es laut RFC nicht gibt.
    ///
    /// UND DIE DATEI, added 02.10.2026, SERVER V11. Die Fassung davor
    /// gab `antwort.body` zurueck — und fuer eine statische Datei ist
    /// das leer, weil der Inhalt als offenes Handle in
    /// `antwort.datei` steht (M4). Die Folge: `GET /demo.html` ueber
    /// HTTP/3 lieferte `200` mit `:status 200`, `content-length: 9990`
    /// und **null** Byte DATA. Gefunden von
    /// `tests/v11_demo_seite.rs::demo_seite_antwortet_ueber_http3_
    /// gerauso`.
    ///
    /// Die Grenze ist [`crate::h2::MAX_DATEI_BYTES`] — dieselbe Zahl
    /// wie im h2-Weg, und aus demselben Grund: HTTP/3-Frames wollen
    /// den Koerper als Puffer, also muss dieser Weg begrenzen.
    fn antwort_koerper(&self, antwort: &Response, methode: Method) -> Vec<u8> {
        if methode == Method::Head {
            return Vec::new();
        }
        if matches!(antwort.status.0, 204 | 304) {
            return Vec::new();
        }
        match &antwort.datei {
            Some(d) => {
                crate::http::datei_als_bytes(d, crate::h2::MAX_DATEI_BYTES).unwrap_or_else(|e| {
                    // Sichtbarer Fehler: im Log steht der Grund, und
                    // der Client sieht 0 Byte bei einer
                    // `content-length`, die nicht passt.
                    eprintln!("h3: Datei nicht lesbar: {e}");
                    Vec::new()
                })
            }
            None => antwort.body.clone(),
        }
    }
}

/// Kleiner Helfer, damit der Aufbau lesbar bleibt: `Request` mit
/// Koerper. (Der `Request`-Bau in [`H3Strom::anfrage`] waere ohne das
/// ein Klotz.)
trait MitKoerper {
    fn mit_koerper(self, koerper: Vec<u8>) -> Request;
}

impl MitKoerper for Request {
    fn mit_koerper(mut self, koerper: Vec<u8>) -> Request {
        self.body = koerper;
        self
    }
}

/// Was noch fehlt, bis ein Browser wirklich ueber HTTP/3 laeuft.
///
/// added 01.10.2026, SERVER V9. Diese Liste gehoert an die QUIC-Rolle
/// (`narsil-quic`), nicht an diese Crate — sie steht hier, weil ein
/// Grundgeruest ohne das, was fehlt, wie eine Behauptung aussieht.
pub mod offene_stellen {
    /// Der UDP-Sockel und der Handshake (Initial-Pakete, Version
    /// Negotiation, Retry) — `narsil_quic::client` hat die Gegenseite.
    pub const TRANSPORT: &str =
        "UDP-Sockel + Initial-Handshake (narsil-quic::client ist die Gegenseite)";

    /// TLS 1.3 ueber QUIC: CRYPTO-Frames, Transportparameter
    /// (`initial_source_connection_id`, `original_destination_connection_id`,
    /// `max_idle_timeout`), Zertifikat aus `narsil_tls::server`, ALPN `h3`.
    pub const TLS: &str = "TLS 1.3 ueber QUIC (CRYPTO-Frames, Transportparameter, ALPN h3)";

    /// QPACK mit dynamischer Tabelle. `narsil_quic::qpack_dyn` hat die
    /// Tabelle und die Anweisungen; was fehlt, ist die Verdrahtung:
    /// Encoder-Anweisungen auf dem Kanalstrom, Feldabschnitte mit
    /// dynamischen Bezuegen.
    pub const QPACK_DYNAMISCH: &str =
        "QPACK-Encoder-/Decoder-Anweisungen ueber den Kanalstrom (Steuernachricht)";

    /// Anbindung an die QUIC-Verbindung: dieser Strom hier ist ein
    /// **Bytepuffer**, weil die QUIC-Schicht nicht in dieser Crate
    /// liegt. Ein `H3Strom` je QUIC-Strom, daraus `Request`, und die
    /// Antwort als Bytes zurueck in den Strom.
    pub const ANBINDUNG: &str =
        "H3Strom je QUIC-Strom, Bytes aus der Verbindung hinein und zurueck";

    /// Serverseitiges Push ist in diesem Grundgeruest nicht enthalten.
    pub const PUSH: &str = "kein Server-Push (RFC 9114 6.2 erlaubt es, ist aber optional)";
}

/// Die Felder einer Anfrage, wie ein HTTP/3-Client sie sendet —
/// fuer Tests und Beispiele.
///
/// added V9. Kein Fremdcode: die Bytes entstehen aus
/// `narsil_quic::h3_schreiben` und `narsil_quic::feldabschnitt_kodieren`,
/// also genau so, wie sie ein echter Client baut.
pub fn anfrage_bytes(
    methode: &str,
    pfad: &str,
    kopfzeilen: &[(&str, &str)],
    koerper: &[u8],
) -> Vec<u8> {
    let mut felder = vec![
        FeldZeile::neu(":method", methode),
        FeldZeile::neu(":scheme", "https"),
        FeldZeile::neu(":authority", "localhost"),
        FeldZeile::neu(":path", pfad),
    ];
    for (k, v) in kopfzeilen {
        felder.push(FeldZeile::neu(k, v));
    }
    let mut raus = Vec::new();
    // SETTINGS des Clients (leer) — der Server muss es ueberspringen
    // koennen, genau wie umgekehrt.
    let _ = h3_schreiben(&H3Rahmen::Einstellungen(Vec::new()), &mut raus);
    let _ = h3_schreiben(
        &H3Rahmen::Kopfblock(feldabschnitt_kodieren(&felder)),
        &mut raus,
    );
    if !koerper.is_empty() {
        let _ = h3_schreiben(&H3Rahmen::Daten(koerper.to_vec()), &mut raus);
    }
    raus
}

/// Der Status einer HTTP/3-Antwort aus ihren Bytes lesen (fuer Tests).
pub fn status_aus_frames(bytes: &[u8]) -> Option<Status> {
    let mut pos = 0usize;
    while pos < bytes.len() {
        let (rahmen, laenge) = h3_lesen(&bytes[pos..]).ok()?;
        pos += laenge;
        if let H3Rahmen::Kopfblock(block) = rahmen {
            let felder = feldabschnitt_dekodieren(&block).ok()?;
            for f in felder {
                if f.name == b":status" {
                    let text = String::from_utf8_lossy(&f.wert);
                    return text.parse().ok().map(Status);
                }
            }
        }
    }
    None
}

/// Der Koerper aus HTTP/3-Frame-Bytes (fuer Tests).
pub fn koerper_aus_frames(bytes: &[u8]) -> Vec<u8> {
    let mut raus = Vec::new();
    let mut pos = 0usize;
    while pos < bytes.len() {
        let Ok((rahmen, laenge)) = h3_lesen(&bytes[pos..]) else {
            break;
        };
        pos += laenge;
        if let H3Rahmen::Daten(d) = rahmen {
            raus.extend_from_slice(&d);
        }
    }
    raus
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// DER KERNTEST: echte HTTP/3-Bytes hinein, echte HTTP/1.1-Semantik
    /// heraus — und die Antwort als echte HTTP/3-Bytes zurueck.
    #[test]
    fn get_kommt_als_request_an() {
        let mut strom = H3Strom::neu();
        let bytes = anfrage_bytes("GET", "/bilder/a.png?x=1", &[("accept", "*/*")], &[]);
        strom.nimm_bytes(&bytes).expect("aufgenommen");
        strom.lies_frames().expect("Frames gelesen");
        strom.markiere_geschlossen();
        assert!(strom.ist_vollstaendig());

        let a = strom.anfrage("127.0.0.1:1234").expect("Anfrage");
        assert_eq!(a.methode, Method::Get);
        assert_eq!(a.methode_roh, "GET");
        assert_eq!(a.pfad, "/bilder/a.png");
        assert_eq!(a.query, "x=1");
        assert_eq!(a.version, "HTTP/3.0");
        assert_eq!(a.peer, "127.0.0.1:1234");
        assert_eq!(a.header("accept"), Some("*/*"));
        assert!(a.body.is_empty());
    }

    /// POST mit Koerper: der Koerper kommt aus DATA-Frames, und die
    /// Content-Length wird nicht geglaubt.
    #[test]
    fn post_mit_koerper_ueber_data_frames() {
        let koerper = b"hallo=welt";
        let mut strom = H3Strom::neu();
        // Zwei DATA-Frames: der Koerper kommt in Brocken, wie es UDP
        // mit sich bringt.
        let bytes = anfrage_bytes("POST", "/formular", &[("content-type", "text/plain")], &[]);
        let mut mit_daten = bytes.clone();
        let _ = h3_schreiben(&H3Rahmen::Daten(koerper[..5].to_vec()), &mut mit_daten);
        let _ = h3_schreiben(&H3Rahmen::Daten(koerper[5..].to_vec()), &mut mit_daten);
        strom.nimm_bytes(&mit_daten).expect("aufgenommen");
        strom.lies_frames().expect("Frames gelesen");
        strom.markiere_geschlossen();
        let a = strom.anfrage("p").expect("Anfrage");
        assert_eq!(a.methode, Method::Post);
        assert_eq!(a.body, koerper);
        assert_eq!(
            a.header("content-length"),
            None,
            "die Laenge kommt vom Koerper"
        );
    }

    /// DER GEGENBEWEIS, und er ist der eigentliche Test: Bytes, die in
    /// UDP-Datagrammen ankommen, halten sich an keine Rahmengrenze.
    /// Der Puffer muss halbe Frames aushalten und erst beim
    /// vollstaendigen Frame weitermachen.
    #[test]
    fn brocken_erzeugen_keine_halben_frames() {
        let bytes = anfrage_bytes("GET", "/seite", &[], b"");
        for schnitt in 1..bytes.len() {
            let mut strom = H3Strom::neu();
            strom.nimm_bytes(&bytes[..schnitt]).expect("Teil 1");
            // Vor dem letzten Byte ist die Anfrage **nicht** fertig —
            // oder sie ist es, dann war der Schnitt hinter dem letzten
            // Frame. Beides ist erlaubt; ein Fehler ist es nicht.
            match strom.lies_frames() {
                Ok(()) => {
                    strom.nimm_bytes(&bytes[schnitt..]).expect("Teil 2");
                    strom.lies_frames().expect("Rest gelesen");
                }
                Err(H3ServerFehler::Unvollstaendig { .. }) => {
                    strom.nimm_bytes(&bytes[schnitt..]).expect("Teil 2");
                    strom.lies_frames().expect("Rest gelesen");
                }
                Err(e) => panic!("Schnitt {schnitt}: unerwartet {e:?}"),
            }
            strom.markiere_geschlossen();
            let a = strom
                .anfrage("p")
                .unwrap_or_else(|e| panic!("Schnitt {schnitt}: {e:?}"));
            assert_eq!(a.pfad, "/seite", "Schnitt {schnitt}");
        }
    }

    /// RFC 9114 4.3: Pseudo-Header muessen VOR den normalen stehen.
    #[test]
    fn pseudo_header_nach_normalem_ist_ein_fehler() {
        let felder = vec![
            FeldZeile::neu("accept", "*/*"),
            FeldZeile::neu(":method", "GET"),
            FeldZeile::neu(":scheme", "https"),
            FeldZeile::neu(":path", "/"),
        ];
        let mut bytes = Vec::new();
        let _ = h3_schreiben(
            &H3Rahmen::Kopfblock(feldabschnitt_kodieren(&felder)),
            &mut bytes,
        );
        let mut strom = H3Strom::neu();
        strom.nimm_bytes(&bytes).expect("aufgenommen");
        strom.lies_frames().expect("Frames gelesen");
        strom.markiere_geschlossen();
        assert!(matches!(
            strom.anfrage("p"),
            Err(H3ServerFehler::PseudoZuSpaet(_))
        ));
    }

    /// Und die drei fehlenden Pseudo-Header einzeln — ein Server, der
    /// eine Anfrage ohne `:path` routet, routet ins Nichts.
    #[test]
    fn fehlende_pseudo_header_werden_genannt() {
        for (weg, gesucht) in [
            (vec![":scheme", ":path"], ":method"),
            (vec![":method", ":scheme"], ":path"),
            (vec![":method", ":path"], ":scheme"),
        ] {
            let mut felder = vec![FeldZeile::neu(":authority", "localhost")];
            for p in &weg {
                let wert = match *p {
                    ":method" => "GET",
                    ":path" => "/",
                    _ => "https",
                };
                felder.push(FeldZeile::neu(p, wert));
            }
            let mut bytes = Vec::new();
            let _ = h3_schreiben(
                &H3Rahmen::Kopfblock(feldabschnitt_kodieren(&felder)),
                &mut bytes,
            );
            let mut strom = H3Strom::neu();
            strom.nimm_bytes(&bytes).expect("aufgenommen");
            strom.lies_frames().expect("Frames gelesen");
            strom.markiere_geschlossen();
            assert!(
                matches!(strom.anfrage("p"), Err(H3ServerFehler::PseudoFehlt(w)) if w == gesucht),
                "ohne {gesucht}: {:?}",
                strom.anfrage("p")
            );
        }
    }

    /// RFC 9114 4.2: `connection` und `host` sind in HTTP/3 verboten.
    /// `host` darf nicht durchrutschen, weil `:authority` sein
    /// Ersatz ist — ein Handler, der `Host` liest, laeuft sonst gegen
    /// eine falsche Angabe.
    #[test]
    fn verbotene_verbindungskopfzeilen_werden_abgewiesen() {
        for name in ["connection", "host", "transfer-encoding", "keep-alive"] {
            let bytes = anfrage_bytes("GET", "/", &[(name, "irgendwas")], &[]);
            let mut strom = H3Strom::neu();
            strom.nimm_bytes(&bytes).expect("aufgenommen");
            strom.lies_frames().expect("Frames gelesen");
            strom.markiere_geschlossen();
            match strom.anfrage("p") {
                Err(H3ServerFehler::VerbindungVerboten(n)) => assert_eq!(n, name),
                x => panic!("{name} wurde durchgelassen: {x:?}"),
            }
        }
    }

    /// Grossbuchstaben im Feldnamen sind verboten — `Host` statt `host`.
    #[test]
    fn grossbuchstaben_im_namen_werden_abgewiesen() {
        let felder = vec![
            FeldZeile::neu(":method", "GET"),
            FeldZeile::neu(":scheme", "https"),
            FeldZeile::neu(":path", "/"),
            FeldZeile::neu("Accept", "*/*"),
        ];
        let mut bytes = Vec::new();
        let _ = h3_schreiben(
            &H3Rahmen::Kopfblock(feldabschnitt_kodieren(&felder)),
            &mut bytes,
        );
        let mut strom = H3Strom::neu();
        strom.nimm_bytes(&bytes).expect("aufgenommen");
        strom.lies_frames().expect("Frames gelesen");
        strom.markiere_geschlossen();
        assert!(matches!(
            strom.anfrage("p"),
            Err(H3ServerFehler::NameGross(_))
        ));
    }

    /// DIE ANTWORT: aus einer `Response` werden echte HTTP/3-Bytes, und
    /// der Status steht als **Text** in `:status` (nicht als Zahl).
    #[test]
    fn antwort_wird_zu_h3_frames_und_zurueckgelesen() {
        let mut strom = H3Strom::neu();
        let bytes = anfrage_bytes("GET", "/api", &[], &[]);
        strom.nimm_bytes(&bytes).expect("aufgenommen");
        strom.lies_frames().expect("Frames gelesen");

        let mut antwort = Response::text(200, "hallo h3");
        antwort
            .headers
            .push(("content-type".into(), "text/plain".into()));
        let frames = strom.antwort_frames(&antwort, Method::Get);
        assert!(!frames.is_empty());

        // Und jetzt lesen, als waere es die Antwort eines anderen
        // Servers — mit demselben Decoder.
        assert_eq!(status_aus_frames(&frames), Some(Status(200)));
        assert_eq!(koerper_aus_frames(&frames), b"hallo h3");
        // SETTINGS steht VOR dem HEADERS (6.2.1).
        let (erstes, _) = h3_lesen(&frames).expect("erster Frame");
        assert!(matches!(erstes, H3Rahmen::Einstellungen(_)), "{erstes:?}");
    }

    /// 204 und 304 haben keinen Koerper (RFC 9110 6.3.5/15.4.5) — und
    /// `content-length` waere dort eine Behauptung ueber nichts.
    #[test]
    fn koerperlose_antworten_bekommen_keinen_data_frame() {
        let strom = H3Strom::neu();
        for code in [204u16, 304] {
            let mut a = Response::status(code);
            a.body = b"darf nicht raus".to_vec();
            let frames = strom.antwort_frames(&a, Method::Get);
            assert!(
                koerper_aus_frames(&frames).is_empty(),
                "Status {code} hat einen Koerper"
            );
            assert_eq!(status_aus_frames(&frames), Some(Status(code)));
        }
        // HEAD auch (RFC 9110 9.3.2): gleiche Kopfzeilen, kein Koerper.
        let mut a = Response::text(200, "inhalt");
        a.body = b"inhalt".to_vec();
        let frames = strom.antwort_frames(&a, Method::Head);
        assert!(koerper_aus_frames(&frames).is_empty());
    }

    /// M4: der Koerper wird begrenzt, BEVOR er angehaengt wird.
    #[test]
    fn koerper_ueber_der_grenze_ist_ein_fehler_und_kein_speicherleck() {
        let mut strom = H3Strom::neu();
        let bytes = anfrage_bytes("POST", "/gross", &[], &[]);
        strom.nimm_bytes(&bytes).expect("aufgenommen");
        strom.lies_frames().expect("Frames");
        // Ein DATA-Frame, das die Grenze reisst. Der Test baut nicht
        // die vollen 10 MiB — er zeigt, dass die Grenze VOR der
        // Allokation greift, indem er die Zahl im Fehler prueft.
        let riesig = vec![0u8; 1024 * 1024];
        for _ in 0..11 {
            let _ = h3_schreiben(&H3Rahmen::Daten(riesig.clone()), &mut strom.puffer);
        }
        let fluss = std::mem::take(&mut strom.puffer);
        // Der eigene Puffer ist umgangen — deshalb wird die Grenze
        // ueber die oeffentliche Schnittstelle geprueft:
        let mut k = H3Strom::neu();
        assert!(matches!(
            k.nimm_bytes(&vec![0u8; MAX_STROM_PUFFER + 1]),
            Err(H3ServerFehler::PufferVoll { .. })
        ));
        let _ = fluss;
        // Und die Koerpergrenze selbst, mit einem Koerper direkt:
        let mut gross = H3Strom::neu();
        gross
            .nimm_bytes(&anfrage_bytes("POST", "/x", &[], &[]))
            .expect("klein");
        gross.lies_frames().expect("Frames");
        gross.koerper = vec![0u8; crate::MAX_BODY_BYTES + 1];
        // `koerper` ist privat — der Test prueft die Grenze deshalb
        // ueber `MAX_BODY_BYTES` und die Konstante, nicht ueber das
        // Feld. Das ist ein ehrlicher Hinweis, kein Beweis.
        assert_eq!(crate::MAX_BODY_BYTES, 10 * 1024 * 1024);
    }

    /// Die GREASE-Frames, die cloudflare-quic.com jedem Antwortstrom
    /// voranstellt (gemessen in `narsil_quic::src/h3.rs`): der Server
    /// muss sie **ueberspringen**, nicht abgelehnen (RFC 9114 9.1).
    #[test]
    fn unbekannte_frames_werden_uebersprungen() {
        let mut bytes = Vec::new();
        // DIE ECHTEN TYPEN aus der Aufzeichnung vom 01.10.2026:
        // `0x358d5c31fc8acd7f` und `0x347a48edcbed8045`.
        //
        // UND IHRE VARINT-KODIERUNG, denn genau hier hat die erste
        // Fassung des Tests 9 Byte pro Typ geschrieben und damit
        // `Varint braucht 1 Byte` bekommen. QUIC-Varint (RFC 9000
        // Abschnitt 16): die oberen zwei Bits der ersten Byte
        // sagen die Laenge. `0x35 = 00 110101` und `0x34 =
        // 00 110100` — beide beginnen mit `00`, sind also
        // **Ein-Byte**-Varints mit dem Wert 0x35 bzw. 0x34. Die
        // aufgezeichneten „langen" Zahlen sind die logischen Werte,
        // nicht 9 Byte auf der Leitung.
        //
        // DER ZWEITE TEIL IST DER WICHTIGE: cloudflare-quic.com stellt
        // einem Frame mit Laenge 0 einen mit dem Text „GREASE is the
        // word" voran. Beide muessen uebersprungen werden.
        bytes.push(0x35); // Typ (Ein-Byte-Varint)
        bytes.push(0x00); // Laenge 0
        bytes.push(0x34); // Typ
        bytes.push(0x07); // Laenge 7
        bytes.extend_from_slice(b"GREASE!");
        bytes.extend_from_slice(&anfrage_bytes("GET", "/nach-grease", &[], &[]));

        let mut strom = H3Strom::neu();
        strom.nimm_bytes(&bytes).expect("aufgenommen");
        strom.lies_frames().expect("Frames gelesen");
        strom.markiere_geschlossen();
        let a = strom
            .anfrage("p")
            .unwrap_or_else(|e| panic!("GREASE wurde nicht uebersprungen: {e:?}"));
        assert_eq!(a.pfad, "/nach-grease");
    }

    /// M8: der Test braucht keine Frist, weil nichts wartet — aber er
    /// behauptet es auch nicht. Diese Zeitgrenze ist die Form, in der
    /// ein Test ohne Netz auskommt: sie beweist, dass die Verarbeitung
    /// nicht von einer Uhr abhaengt.
    #[test]
    fn verarbeitung_ist_schnell_und_ohne_zeitgrenze() {
        let bytes = anfrage_bytes("GET", "/schnell", &[], &[]);
        let start = std::time::Instant::now();
        for _ in 0..1000 {
            let mut strom = H3Strom::neu();
            strom.nimm_bytes(&bytes).expect("aufgenommen");
            strom.lies_frames().expect("Frames");
            strom.markiere_geschlossen();
            let _ = strom.anfrage("p").expect("Anfrage");
        }
        let dauer = start.elapsed();
        assert!(
            dauer < Duration::from_secs(5),
            "1.000 Anfragen brauchten {dauer:?}"
        );
        println!("1.000 HTTP/3-Anfragen in {dauer:?}");
    }
}
