//! WebSocket in reinem Rust (RFC 6455).
//!
//! Der Browser ohne JavaScript braucht trotzdem einen Rueckkanal:
//! Live-Reload beim Entwickeln, Server-Push fuer Foren und Chats,
//! Statusmeldungen ohne Polling. WebSocket ist dafuer das moderne
//! Protokoll (Vision Punkt 1: HTTPS, HTTP/3, WebSockets) — und es ist
//! ohne JavaScript nutzbar, weil der Server Frames schickt und der
//! Browser sie als Seiten-Aktualisierung zeigt, nicht als Skript.
//!
//! Was hier steht:
//!
//! * Handshake: `GET` mit `Upgrade: websocket` -> `101 Switching
//!   Protocols` mit `Sec-WebSocket-Accept` (SHA-1 + Base64, RFC 6455).
//! * Frames: Text (0x1), Binaer (0x2), Close (0x8), Ping (0x9), Pong
//!   (0xA). Maskierung Client->Server wird entfernt, Server->Client
//!   wird nicht maskiert (RFC-Vorgabe). Fragmente werden zusammengesetzt.
//!
//! Was hier NICHT steht: `wss://` (braucht TLS, siehe [`crate::tls`]),
//! Extensions (`permessage-deflate` wird verhandelt-aber-abgelehnt,
//! nicht halb implementiert), JavaScript-API (gibt es nicht, ADR-037).

use crate::http::{Request, Response};
use std::io::{Read, Write};

/// Hoechstens so viele Frames darf der einfache Echo-Dienst bearbeiten.
pub const MAX_ECHO_FRAMES: usize = 1024;
/// Die Echo-Anwendung braucht keine 10-MiB-Nachrichten. Diese kleinere
/// Grenze gilt vor der Payload-Allokation.
pub const MAX_ECHO_PAYLOAD: usize = 1024 * 1024;

/// Die magische GUID aus RFC 6455, Abschnitt 1.3. Keine Konfiguration,
/// kein Parameter — wer sie aendert, bricht das Protokoll.
pub const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// WebSocket-Opcode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opcode {
    Fortsetzung,
    Text,
    Binaer,
    Close,
    Ping,
    Pong,
    /// Reserviert/Extension (3–7, B–F). Wird gelesen und als Fehler
    /// gemeldet, nicht still verworfen.
    Reserviert(u8),
}

impl Opcode {
    fn aus_nibble(n: u8) -> Self {
        match n {
            0x0 => Opcode::Fortsetzung,
            0x1 => Opcode::Text,
            0x2 => Opcode::Binaer,
            0x8 => Opcode::Close,
            0x9 => Opcode::Ping,
            0xA => Opcode::Pong,
            x => Opcode::Reserviert(x),
        }
    }

    fn als_nibble(self) -> u8 {
        match self {
            Opcode::Fortsetzung => 0x0,
            Opcode::Text => 0x1,
            Opcode::Binaer => 0x2,
            Opcode::Close => 0x8,
            Opcode::Ping => 0x9,
            Opcode::Pong => 0xA,
            Opcode::Reserviert(x) => x & 0x0F,
        }
    }
}

/// Eine empfangene Nachricht (Fragmente bereits zusammengesetzt).
#[derive(Debug, Clone, PartialEq)]
pub enum Nachricht {
    Text(String),
    Binaer(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close { code: u16, grund: String },
}

/// Fehler im WebSocket-Protokoll. Jeder hat eine Close-Code-Antwort —
/// ein Protokollfehler schliesst die Verbindung, statt sie offen zu
/// lassen und zu raten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsFehler {
    /// Kein Upgrade-Header (kein WebSocket, normale HTTP-Antwort).
    KeinUpgrade(String),
    /// Protokollverletzung (mit Close-Code).
    Protokoll { code: u16, meldung: String },
    /// Unvollstaendige Bytes (mehr lesen, kein Fehler).
    Unvollstaendig,
    /// Text kein UTF-8.
    TextKeinUtf8,
}

impl std::fmt::Display for WsFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WsFehler::KeinUpgrade(s) => write!(f, "kein WebSocket-Upgrade: {s}"),
            WsFehler::Protokoll { code, meldung } => {
                write!(f, "WebSocket-Fehler {code}: {meldung}")
            }
            WsFehler::Unvollstaendig => write!(f, "unvollstaendiger Frame"),
            WsFehler::TextKeinUtf8 => write!(f, "Text-Frame kein UTF-8"),
        }
    }
}

impl std::error::Error for WsFehler {}

/// Prueft, ob eine Anfrage ein WebSocket-Handshake ist.
///
/// Bedingungen (RFC 6455, Abschnitt 4.1): `GET`, `Upgrade: websocket`
/// (case-insensitiv), `Connection` enthaelt `Upgrade`, `Sec-WebSocket-Key`
/// vorhanden, `Sec-WebSocket-Version: 13`. Fehlt eines, ist es kein
/// Fehler, sondern keine WebSocket-Anfrage — der Aufrufer routet dann
/// normal weiter.
pub fn ist_handshake(req: &Request) -> bool {
    if req.methode != crate::http::Method::Get {
        return false;
    }
    let upgrade = req.header("upgrade").unwrap_or("").to_ascii_lowercase();
    if upgrade != "websocket" {
        return false;
    }
    let conn = req.header("connection").unwrap_or("").to_ascii_lowercase();
    if !conn.split(',').any(|p| p.trim() == "upgrade") {
        return false;
    }
    if req
        .header("sec-websocket-key")
        .unwrap_or("")
        .trim()
        .is_empty()
    {
        return false;
    }
    req.header("sec-websocket-version").unwrap_or("").trim() == "13"
}

/// Antwortet auf einen Handshake mit `101 Switching Protocols`.
///
/// Gibt `Err`, wenn es kein Handshake ist (mit Grund) — der Aufrufer
/// antwortet dann normal (404/400), statt die Verbindung zu kappen.
pub fn handshake_antwort(req: &Request) -> Result<Response, WsFehler> {
    if req.methode != crate::http::Method::Get {
        return Err(WsFehler::KeinUpgrade("kein GET".to_string()));
    }
    let key = req
        .header("sec-websocket-key")
        .unwrap_or("")
        .trim()
        .to_string();
    if key.is_empty() {
        return Err(WsFehler::KeinUpgrade("Sec-WebSocket-Key fehlt".to_string()));
    }
    // Key muss 16 Bytes Base64 sein — sonst ist es kein Schluessel,
    // sondern ein Tippfehler, der als solcher gemeldet wird.
    match crate::base64::dekodieren(&key) {
        Ok(b) if b.len() == 16 => {}
        _ => {
            return Err(WsFehler::KeinUpgrade(
                "Sec-WebSocket-Key kein 16-Byte-Base64".to_string(),
            ));
        }
    }
    let version = req.header("sec-websocket-version").unwrap_or("").trim();
    if version != "13" {
        let mut antwort = Response::status(426);
        antwort
            .headers
            .push(("Sec-WebSocket-Version".into(), "13".into()));
        antwort.body =
            format!("WebSocket-Version {version:?} nicht unterstuetzt, erwartet 13").into_bytes();
        return Ok(antwort);
    }
    let accept = accept_schluessel(&key);
    let mut antwort = Response::status(101);
    antwort.headers.push(("Upgrade".into(), "websocket".into()));
    antwort
        .headers
        .push(("Connection".into(), "Upgrade".into()));
    antwort
        .headers
        .push(("Sec-WebSocket-Accept".into(), accept));
    Ok(antwort)
}

/// `Sec-WebSocket-Accept` aus dem Key (RFC 6455, Abschnitt 1.3).
pub fn accept_schluessel(key: &str) -> String {
    let hash = crate::sha1::sha1(format!("{}{WS_GUID}", key.trim()).as_bytes());
    crate::base64::kodieren(&hash)
}

/// Kodiert EINE Nachricht als Frame(s) fuer Server->Client.
///
/// Server maskiert NICHT (RFC-Vorgabe). Nachrichten ueber 10 MiB werden
/// abgelehnt (statt den Speicher zu sprengen) — das ist [`crate::MAX_BODY_BYTES`],
/// nicht eine neue Zahl.
pub fn kodiere(opcode: Opcode, payload: &[u8]) -> Result<Vec<u8>, WsFehler> {
    if payload.len() > crate::MAX_BODY_BYTES {
        return Err(WsFehler::Protokoll {
            code: 1009,
            meldung: "Nachricht zu gross".to_string(),
        });
    }
    // Kontrollframes (Ping/Pong/Close) duerfen max. 125 Bytes und nie
    // fragmentiert sein (RFC 6455, Abschnitt 5.5).
    if matches!(opcode, Opcode::Ping | Opcode::Pong | Opcode::Close) && payload.len() > 125 {
        return Err(WsFehler::Protokoll {
            code: 1002,
            meldung: "Kontrollframe zu gross".to_string(),
        });
    }
    let mut raus = Vec::with_capacity(payload.len() + 10);
    raus.push(0x80 | opcode.als_nibble());
    if payload.len() < 126 {
        raus.push(payload.len() as u8);
    } else if payload.len() < 65536 {
        raus.push(126);
        raus.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        raus.push(127);
        raus.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    raus.extend_from_slice(payload);
    Ok(raus)
}

/// Bequemlichkeit: Text senden.
pub fn kodiere_text(text: &str) -> Result<Vec<u8>, WsFehler> {
    kodiere(Opcode::Text, text.as_bytes())
}

/// Dekodiert Frames aus einem Puffer. Gibt die Nachrichten UND die
/// verbrauchten Bytes zurueck — der Rest bleibt im Puffer (TCP liefert
/// Stuecke, keine Nachrichten).
///
/// Client-Frames MUESSEN maskiert sein (RFC 6455). Unmaskierte Frames
/// vom Client sind ein Protokollfehler (1002), kein stilles Lesen.
pub fn dekodiere(puffer: &[u8]) -> Result<(Vec<Nachricht>, usize), WsFehler> {
    // Ein Aufrufer darf nicht beliebig viele kleine Kontrollframes in
    // einem Puffer sammeln. Die Laenge wird vor der ersten Kopie geprueft.
    const MAX_WS_RAHMEN: usize = 1024;
    const MAX_WS_KOPF: usize = 14;
    if puffer.len() > crate::MAX_BODY_BYTES + MAX_WS_KOPF {
        return Err(WsFehler::Protokoll {
            code: 1009,
            meldung: "Frame-Puffer zu gross".into(),
        });
    }
    let mut nachrichten = Vec::new();
    let mut pos = 0;
    let mut rahmen = 0usize;
    // Fragment-Zustand: laufender Text/Binaer-Frame.
    let mut frag_opcode: Option<Opcode> = None;
    let mut frag_puffer: Vec<u8> = Vec::new();

    while pos < puffer.len() {
        rahmen += 1;
        if rahmen > MAX_WS_RAHMEN {
            return Err(WsFehler::Protokoll {
                code: 1009,
                meldung: "zu viele Frames".into(),
            });
        }
        if puffer.len() - pos < 2 {
            break; // Unvollstaendiger Header — mehr lesen.
        }
        let b0 = puffer[pos];
        let b1 = puffer[pos + 1];
        let fin = b0 & 0x80 != 0;
        let opcode = Opcode::aus_nibble(b0 & 0x0F);
        let maskiert = b1 & 0x80 != 0;
        let mut laenge = (b1 & 0x7F) as u64;
        let mut kopf = 2;

        // RSV-Bits muessen 0 sein (keine Extensions in v0).
        if b0 & 0x70 != 0 {
            return Err(WsFehler::Protokoll {
                code: 1002,
                meldung: "RSV-Bits gesetzt (keine Extensions)".to_string(),
            });
        }

        if laenge == 126 {
            if puffer.len() - pos < 4 {
                break;
            }
            laenge = u16::from_be_bytes([puffer[pos + 2], puffer[pos + 3]]) as u64;
            kopf = 4;
        } else if laenge == 127 {
            if puffer.len() - pos < 10 {
                break;
            }
            laenge = u64::from_be_bytes([
                puffer[pos + 2],
                puffer[pos + 3],
                puffer[pos + 4],
                puffer[pos + 5],
                puffer[pos + 6],
                puffer[pos + 7],
                puffer[pos + 8],
                puffer[pos + 9],
            ]);
            kopf = 10;
            // Groesste Laenge: 2^63 (MSB muss 0 sein, RFC 6455).
            if laenge >= (1 << 63) {
                return Err(WsFehler::Protokoll {
                    code: 1009,
                    meldung: "Frame zu gross".to_string(),
                });
            }
        }
        if laenge > crate::MAX_BODY_BYTES as u64 {
            return Err(WsFehler::Protokoll {
                code: 1009,
                meldung: "Frame zu gross".to_string(),
            });
        }
        let maske_len = if maskiert { 4 } else { 0 };
        if puffer.len() - pos < kopf + maske_len + laenge as usize {
            break; // Unvollstaendig — Rest bleibt im Puffer.
        }
        // Client muss maskieren.
        if !maskiert {
            return Err(WsFehler::Protokoll {
                code: 1002,
                meldung: "Client-Frame nicht maskiert".to_string(),
            });
        }
        let maske = &puffer[pos + kopf..pos + kopf + 4];
        let start = pos + kopf + 4;
        let mut payload = puffer[start..start + laenge as usize].to_vec();
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= maske[i % 4];
        }
        pos = start + laenge as usize;

        // Kontrollframes nie fragmentiert.
        if matches!(opcode, Opcode::Ping | Opcode::Pong | Opcode::Close) && !fin {
            return Err(WsFehler::Protokoll {
                code: 1002,
                meldung: "Kontrollframe fragmentiert".to_string(),
            });
        }
        if matches!(opcode, Opcode::Ping | Opcode::Pong | Opcode::Close) && laenge > 125 {
            return Err(WsFehler::Protokoll {
                code: 1002,
                meldung: "Kontrollframe zu gross".to_string(),
            });
        }

        match opcode {
            Opcode::Fortsetzung => {
                let Some(op) = frag_opcode else {
                    return Err(WsFehler::Protokoll {
                        code: 1002,
                        meldung: "Fortsetzung ohne Anfang".to_string(),
                    });
                };
                if payload.len() > crate::MAX_BODY_BYTES.saturating_sub(frag_puffer.len()) {
                    return Err(WsFehler::Protokoll {
                        code: 1009,
                        meldung: "Nachricht zu gross".to_string(),
                    });
                }
                frag_puffer.extend_from_slice(&payload);
                if fin {
                    nachrichten.push(fertige_nachricht(op, std::mem::take(&mut frag_puffer))?);
                    frag_opcode = None;
                }
            }
            Opcode::Text | Opcode::Binaer => {
                if frag_opcode.is_some() {
                    return Err(WsFehler::Protokoll {
                        code: 1002,
                        meldung: "Neuer Frame vor Fragment-Ende".to_string(),
                    });
                }
                if fin {
                    nachrichten.push(fertige_nachricht(opcode, payload)?);
                } else {
                    frag_opcode = Some(opcode);
                    frag_puffer = payload;
                }
            }
            Opcode::Ping => nachrichten.push(Nachricht::Ping(payload)),
            Opcode::Pong => nachrichten.push(Nachricht::Pong(payload)),
            Opcode::Close => {
                let (code, grund) = close_payload(&payload)?;
                nachrichten.push(Nachricht::Close { code, grund });
            }
            Opcode::Reserviert(x) => {
                return Err(WsFehler::Protokoll {
                    code: 1003,
                    meldung: format!("Opcode {x:#x} nicht unterstuetzt"),
                });
            }
        }
    }
    Ok((nachrichten, pos))
}

/// Begrenzter Echo-Dienst nach einem erfolgreichen Upgrade.
///
/// Der Aufrufer setzt Lese- und Schreibfristen auf dem Socket. Dieser
/// Dienst verarbeitet ganze, maskierte Client-Frames; Fragmentierung
/// wird mit einem benannten Fehler abgelehnt.
pub fn echo<R: Read, W: Write>(leser: &mut R, schreiber: &mut W) -> Result<usize, String> {
    let mut beantwortet = 0usize;
    for _ in 0..MAX_ECHO_FRAMES {
        let mut kopf = [0u8; 2];
        match leser.read(&mut kopf[..1]) {
            Ok(0) => return Ok(beantwortet),
            Ok(_) => {}
            Err(e) => return Err(format!("WebSocket-Lesen: {e}")),
        }
        leser
            .read_exact(&mut kopf[1..])
            .map_err(|e| format!("WebSocket-Kopf: {e}"))?;
        if kopf[0] & 0x80 == 0 {
            return Err("WebSocket-Fragmentierung im Echo nicht unterstuetzt".into());
        }
        let mut laenge = (kopf[1] & 0x7f) as u64;
        let mut extra = [0u8; 8];
        let zusatz = match laenge {
            126 => 2,
            127 => 8,
            _ => 0,
        };
        if zusatz > 0 {
            leser
                .read_exact(&mut extra[..zusatz])
                .map_err(|e| format!("WebSocket-Laenge: {e}"))?;
            laenge = if zusatz == 2 {
                u16::from_be_bytes([extra[0], extra[1]]) as u64
            } else {
                u64::from_be_bytes(extra)
            };
        }
        if laenge > MAX_ECHO_PAYLOAD as u64 {
            return Err("WebSocket-Echo-Payload zu gross".into());
        }
        let mut raw = Vec::with_capacity(2 + zusatz + 4 + laenge as usize);
        raw.extend_from_slice(&kopf);
        raw.extend_from_slice(&extra[..zusatz]);
        let rest = raw.len();
        raw.resize(rest + 4 + laenge as usize, 0);
        leser
            .read_exact(&mut raw[rest..])
            .map_err(|e| format!("WebSocket-Frame: {e}"))?;
        let (nachrichten, verbraucht) = dekodiere(&raw).map_err(|e| e.to_string())?;
        if verbraucht != raw.len() || nachrichten.len() != 1 {
            return Err("WebSocket-Frame unvollstaendig".into());
        }
        let (opcode, daten) = match &nachrichten[0] {
            Nachricht::Text(s) => (Opcode::Text, s.as_bytes()),
            Nachricht::Binaer(b) => (Opcode::Binaer, b.as_slice()),
            Nachricht::Ping(b) => (Opcode::Pong, b.as_slice()),
            Nachricht::Pong(_) => continue,
            Nachricht::Close { .. } => {
                let close = kodiere(Opcode::Close, &[]).map_err(|e| e.to_string())?;
                schreiber.write_all(&close).map_err(|e| e.to_string())?;
                return Ok(beantwortet);
            }
        };
        let antwort = kodiere(opcode, daten).map_err(|e| e.to_string())?;
        schreiber.write_all(&antwort).map_err(|e| e.to_string())?;
        beantwortet += 1;
    }
    Err("WebSocket-Echo-Framegrenze erreicht".into())
}

fn fertige_nachricht(opcode: Opcode, payload: Vec<u8>) -> Result<Nachricht, WsFehler> {
    match opcode {
        Opcode::Text => String::from_utf8(payload)
            .map(Nachricht::Text)
            .map_err(|_| WsFehler::TextKeinUtf8),
        Opcode::Binaer => Ok(Nachricht::Binaer(payload)),
        _ => Err(WsFehler::Protokoll {
            code: 1002,
            meldung: "Fragment mit falschem Opcode".to_string(),
        }),
    }
}

fn close_payload(payload: &[u8]) -> Result<(u16, String), WsFehler> {
    if payload.is_empty() {
        return Ok((1005, String::new()));
    }
    if payload.len() == 1 {
        return Err(WsFehler::Protokoll {
            code: 1002,
            meldung: "Close-Frame mit 1 Byte".to_string(),
        });
    }
    let code = u16::from_be_bytes([payload[0], payload[1]]);
    let grund = String::from_utf8(payload[2..].to_vec()).map_err(|_| WsFehler::TextKeinUtf8)?;
    Ok((code, grund))
}

/// Baut einen maskierten Client-Frame (fuer Tests und den Browser als
/// WS-Client). Maske fest (kein Zufall noetig — die Maske schuetzt vor
/// Cache-Poisoning, nicht vor Raten).
pub fn kodiere_client(opcode: Opcode, payload: &[u8], maske: [u8; 4]) -> Vec<u8> {
    let mut raus = Vec::with_capacity(payload.len() + 10);
    raus.push(0x80 | opcode.als_nibble());
    if payload.len() < 126 {
        raus.push(0x80 | payload.len() as u8);
    } else if payload.len() < 65536 {
        raus.push(0x80 | 126);
        raus.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        raus.push(0x80 | 127);
        raus.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    raus.extend_from_slice(&maske);
    for (i, b) in payload.iter().enumerate() {
        raus.push(b ^ maske[i % 4]);
    }
    raus
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::anfrage_aus_text;

    #[test]
    fn rfc6455_accept_vektor() {
        // RFC 6455, Abschnitt 1.3.
        assert_eq!(
            accept_schluessel("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn handshake_runde() {
        let req = anfrage_aus_text(
            "GET",
            "/chat",
            &[
                ("Upgrade", "websocket"),
                ("Connection", "Upgrade"),
                ("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ=="),
                ("Sec-WebSocket-Version", "13"),
            ],
            &[],
        );
        assert!(ist_handshake(&req));
        let antwort = handshake_antwort(&req).unwrap();
        assert_eq!(antwort.status.0, 101);
        assert_eq!(
            antwort.header("Sec-WebSocket-Accept"),
            Some("s3pPLMBiTxaQ9kYGzzhZRbK+xOo=")
        );
    }

    #[test]
    fn kein_upgrade_ist_kein_fehler() {
        let req = anfrage_aus_text("GET", "/", &[("Host", "x")], &[]);
        assert!(!ist_handshake(&req));
        assert!(handshake_antwort(&req).is_err());
    }

    #[test]
    fn text_runde_maskiert() {
        let frame = kodiere_client(Opcode::Text, "Hallo".as_bytes(), [1, 2, 3, 4]);
        let (msgs, n) = dekodiere(&frame).unwrap();
        assert_eq!(n, frame.len());
        assert_eq!(msgs, vec![Nachricht::Text("Hallo".to_string())]);
    }

    #[test]
    fn server_frame_ist_nicht_maskiert() {
        let frame = kodiere(Opcode::Text, b"hi").unwrap();
        // Zweites Byte ohne Masken-Bit.
        assert!(frame[1] & 0x80 == 0);
    }

    #[test]
    fn unmaskiert_vom_client_ist_protokollfehler() {
        let frame = kodiere(Opcode::Text, b"hi").unwrap();
        let err = dekodiere(&frame).unwrap_err();
        assert!(matches!(err, WsFehler::Protokoll { code: 1002, .. }));
    }

    #[test]
    fn ping_pong() {
        let ping = kodiere_client(Opcode::Ping, b"p", [0, 0, 0, 0]);
        let (msgs, _) = dekodiere(&ping).unwrap();
        assert_eq!(msgs, vec![Nachricht::Ping(b"p".to_vec())]);
    }

    #[test]
    fn fragment_wird_zusammengesetzt() {
        // "Hel" + "lo" als zwei Frames (FIN=0, dann FIN=1 mit Opcode 0).
        let mut f1 = kodiere_client(Opcode::Text, b"Hel", [1, 1, 1, 1]);
        f1[0] &= !0x80; // FIN loeschen
        let f2 = kodiere_client(Opcode::Fortsetzung, b"lo", [2, 2, 2, 2]);
        let mut beide = f1.clone();
        beide.extend_from_slice(&f2);
        let (msgs, _) = dekodiere(&beide).unwrap();
        assert_eq!(msgs, vec![Nachricht::Text("Hello".to_string())]);
    }
}
