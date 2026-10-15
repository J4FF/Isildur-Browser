//! QUIC-Frames (RFC 9000, Abschnitt 19): lesen und schreiben.
//!
//! Ein QUIC-Paket traegt nach dem Kopf eine Folge von Frames. Jeder Frame
//! beginnt mit einem Varint, dem Frame-Typ; danach folgen die typspezifischen
//! Felder. Hier werden ALLE Typen aus RFC 9000 Abschnitt 19 unterstuetzt:
//! PADDING, PING, ACK (mit Bereichen und ECN), RESET_STREAM, STOP_SENDING,
//! CRYPTO, NEW_TOKEN, STREAM (OFF/LEN/FIN), MAX_DATA, MAX_STREAM_DATA,
//! MAX_STREAMS, DATA_BLOCKED, STREAM_DATA_BLOCKED, STREAMS_BLOCKED,
//! NEW_CONNECTION_ID, RETIRE_CONNECTION_ID, PATH_CHALLENGE, PATH_RESPONSE,
//! CONNECTION_CLOSE (Transport und Anwendung) und HANDSHAKE_DONE.
//!
//! M3/M4: jede von der Eingabe abhaengige Groesse hat eine benannte Grenze,
//! die VOR der Allokation geprueft wird. Ein Laengenfeld, das mehr ankündigt
//! als der Puffer hergibt, ist ein benannter Fehler, kein Panic.

use crate::varint::{self, VarintFehler};

/// Frame-Typen (RFC 9000, Abschnitt 19).
pub const TYP_PADDING: u64 = 0x00;
pub const TYP_PING: u64 = 0x01;
pub const TYP_ACK: u64 = 0x02;
pub const TYP_ACK_ECN: u64 = 0x03;
pub const TYP_RESET_STREAM: u64 = 0x04;
pub const TYP_STOP_SENDING: u64 = 0x05;
pub const TYP_CRYPTO: u64 = 0x06;
pub const TYP_NEW_TOKEN: u64 = 0x07;
pub const TYP_STREAM_MIN: u64 = 0x08;
pub const TYP_STREAM_MAX: u64 = 0x0f;
pub const TYP_MAX_DATA: u64 = 0x10;
pub const TYP_MAX_STREAM_DATA: u64 = 0x11;
pub const TYP_MAX_STREAMS_BIDI: u64 = 0x12;
pub const TYP_MAX_STREAMS_UNI: u64 = 0x13;
pub const TYP_DATA_BLOCKED: u64 = 0x14;
pub const TYP_STREAM_DATA_BLOCKED: u64 = 0x15;
pub const TYP_STREAMS_BLOCKED_BIDI: u64 = 0x16;
pub const TYP_STREAMS_BLOCKED_UNI: u64 = 0x17;
pub const TYP_NEW_CONNECTION_ID: u64 = 0x18;
pub const TYP_RETIRE_CONNECTION_ID: u64 = 0x19;
pub const TYP_PATH_CHALLENGE: u64 = 0x1a;
pub const TYP_PATH_RESPONSE: u64 = 0x1b;
pub const TYP_CONNECTION_CLOSE: u64 = 0x1c;
pub const TYP_CONNECTION_CLOSE_APP: u64 = 0x1d;
pub const TYP_HANDSHAKE_DONE: u64 = 0x1e;

/// STREAM-Flaggen (RFC 9000, Abschnitt 19.8).
const STREAM_FIN: u64 = 0x01;
const STREAM_LEN: u64 = 0x02;
const STREAM_OFF: u64 = 0x04;

/// Hoechste Anzahl von ACK-Bereichen (RFC 9000, Abschnitt 19.3). Ein ACK
/// darf beliebig viele Bereiche ankündigen; diese harte Grenze (M3) macht die
/// Zahl der Schleifendurchlaeufe unabhaengig von boesartiger Eingabe.
pub const MAX_ACK_BEREICHE: usize = 256;
/// Groesster Verbindungsabbruch-Grund in Byte (M4). Begrenzt den Speicher
/// fuer die Reason-Phrase aus boesartiger Eingabe.
pub const MAX_GRUND_LAENGE: usize = 4096;
/// Groesste Connection-ID in einem NEW_CONNECTION_ID (RFC 9000, Abschnitt 5.1:
/// 0..20 Byte).
pub const MAX_CID_LAENGE: usize = 20;

/// Ein ACK-Bereich: eine Luecke und die darauf folgende bestaetigte Strecke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AckBereich {
    /// Zahl der nicht bestaetigten Pakete unterhalb des vorigen Bereichs.
    pub luecke: u64,
    /// Laenge der bestaetigten Strecke.
    pub laenge: u64,
}

/// Ein gelesener QUIC-Frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rahmen {
    Padding,
    Ping,
    Ack {
        groesste: u64,
        verzoegerung: u64,
        erste_range: u64,
        bereiche: Vec<AckBereich>,
        /// ECT0, ECT1, ECN-CE — nur bei ACK_ECN (Typ 0x03).
        ecn: Option<[u64; 3]>,
    },
    ResetStream {
        stream_id: u64,
        fehlercode: u64,
        endgroesse: u64,
    },
    StopSending {
        stream_id: u64,
        fehlercode: u64,
    },
    Crypto {
        offset: u64,
        daten: Vec<u8>,
    },
    NewToken {
        token: Vec<u8>,
    },
    Stream {
        stream_id: u64,
        offset: u64,
        daten: Vec<u8>,
        fin: bool,
    },
    MaxData {
        maximum: u64,
    },
    MaxStreamData {
        stream_id: u64,
        maximum: u64,
    },
    MaxStreams {
        bidirektional: bool,
        maximum: u64,
    },
    DataBlocked {
        maximum: u64,
    },
    StreamDataBlocked {
        stream_id: u64,
        maximum: u64,
    },
    StreamsBlocked {
        bidirektional: bool,
        maximum: u64,
    },
    NewConnectionId {
        sequenz: u64,
        zurueckziehen_vor: u64,
        cid: Vec<u8>,
        reset_token: [u8; 16],
    },
    RetireConnectionId {
        sequenz: u64,
    },
    PathChallenge {
        daten: [u8; 8],
    },
    PathResponse {
        daten: [u8; 8],
    },
    ConnectionClose {
        /// `true` = Transport (Typ 0x1c, mit Frame-Type-Feld), `false` =
        /// Anwendung (Typ 0x1d).
        transport: bool,
        fehlercode: u64,
        frame_typ: u64,
        grund: Vec<u8>,
    },
    HandshakeDone,
}

/// Warum ein Frame nicht gelesen oder geschrieben werden kann.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RahmenFehler {
    /// Der Puffer endet mitten im Frame.
    Abgeschnitten { braucht: usize, da: usize },
    /// Ein Laengenfeld kündigt mehr an als der Puffer hergibt.
    LaengeUeberPuffer { angegeben: u64, da: usize },
    /// Zu viele ACK-Bereiche.
    ZuVieleBereiche { ist: u64, maximum: usize },
    /// Eine Connection-ID war zu lang.
    CidZuLang { ist: usize, maximum: usize },
    /// Ein Token oder Grund war zu lang.
    ZuLang { ist: usize, maximum: usize },
    /// Ein Varint war kaputt.
    Varint(VarintFehler),
    /// Der Frame-Typ ist unbekannt (RFC 9000 Abschnitt 12.4).
    UnbekannterTyp { typ: u64 },
}

impl std::fmt::Display for RahmenFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RahmenFehler::Abgeschnitten { braucht, da } => {
                write!(f, "Frame braucht {braucht} Byte, da sind {da}")
            }
            RahmenFehler::LaengeUeberPuffer { angegeben, da } => {
                write!(f, "Frame kündigt {angegeben} Byte an, da sind {da}")
            }
            RahmenFehler::ZuVieleBereiche { ist, maximum } => {
                write!(f, "ACK hat {ist} Bereiche, erlaubt sind {maximum}")
            }
            RahmenFehler::CidZuLang { ist, maximum } => {
                write!(f, "Connection-ID hat {ist} Byte, erlaubt sind {maximum}")
            }
            RahmenFehler::ZuLang { ist, maximum } => {
                write!(f, "{ist} Byte, erlaubt sind {maximum}")
            }
            RahmenFehler::Varint(e) => write!(f, "Varint im Frame: {e}"),
            RahmenFehler::UnbekannterTyp { typ } => write!(f, "unbekannter Frame-Typ {typ}"),
        }
    }
}

impl std::error::Error for RahmenFehler {}

impl From<VarintFehler> for RahmenFehler {
    fn from(e: VarintFehler) -> Self {
        RahmenFehler::Varint(e)
    }
}

/// Liest einen Varint und gibt `(wert, verbrauchte_bytes)` zurueck; der
/// uebrige Puffer wird weitergereicht.
fn nimm_varint(daten: &[u8]) -> Result<(u64, &[u8]), RahmenFehler> {
    let (wert, len) = varint::lese(daten)?;
    Ok((wert, &daten[len..]))
}

/// Nimmt `laenge` Byte aus dem Puffer; Fehler, wenn nicht genug da sind.
fn nimm(daten: &[u8], laenge: usize) -> Result<(&[u8], &[u8]), RahmenFehler> {
    if daten.len() < laenge {
        return Err(RahmenFehler::Abgeschnitten {
            braucht: laenge,
            da: daten.len(),
        });
    }
    Ok((&daten[..laenge], &daten[laenge..]))
}

/// Liest ein Laengenfeld und prueft es gegen den Restpuffer, BEVOR etwas
/// genommen wird (M4).
fn nimm_laenge(daten: &[u8], maximum: usize) -> Result<(usize, &[u8]), RahmenFehler> {
    let (laenge, rest) = nimm_varint(daten)?;
    if laenge as usize > maximum {
        return Err(RahmenFehler::ZuLang {
            ist: laenge as usize,
            maximum,
        });
    }
    if rest.len() < laenge as usize {
        return Err(RahmenFehler::LaengeUeberPuffer {
            angegeben: laenge,
            da: rest.len(),
        });
    }
    Ok((laenge as usize, rest))
}

/// Liest einen einzelnen Frame. Liefert `(frame, verbrauchte_bytes)`.
pub fn rahmen_lesen(daten: &[u8]) -> Result<(Rahmen, usize), RahmenFehler> {
    let start = daten.len();
    let (typ, mut rest) = nimm_varint(daten)?;
    let rahmen = match typ {
        TYP_PADDING => Rahmen::Padding,
        TYP_PING => Rahmen::Ping,
        TYP_ACK | TYP_ACK_ECN => {
            let (groesste, r) = nimm_varint(rest)?;
            rest = r;
            let (verzoegerung, r) = nimm_varint(rest)?;
            rest = r;
            let (anzahl, r) = nimm_varint(rest)?;
            rest = r;
            if anzahl as usize > MAX_ACK_BEREICHE {
                return Err(RahmenFehler::ZuVieleBereiche {
                    ist: anzahl,
                    maximum: MAX_ACK_BEREICHE,
                });
            }
            let (erste_range, r) = nimm_varint(rest)?;
            rest = r;
            let mut bereiche = Vec::with_capacity(anzahl as usize);
            for _ in 0..anzahl {
                let (luecke, r) = nimm_varint(rest)?;
                rest = r;
                let (laenge, r) = nimm_varint(rest)?;
                rest = r;
                bereiche.push(AckBereich { luecke, laenge });
            }
            let ecn = if typ == TYP_ACK_ECN {
                let (ect0, r) = nimm_varint(rest)?;
                rest = r;
                let (ect1, r) = nimm_varint(rest)?;
                rest = r;
                let (ce, r) = nimm_varint(rest)?;
                rest = r;
                Some([ect0, ect1, ce])
            } else {
                None
            };
            Rahmen::Ack {
                groesste,
                verzoegerung,
                erste_range,
                bereiche,
                ecn,
            }
        }
        TYP_RESET_STREAM => {
            let (stream_id, r) = nimm_varint(rest)?;
            rest = r;
            let (fehlercode, r) = nimm_varint(rest)?;
            rest = r;
            let (endgroesse, r) = nimm_varint(rest)?;
            rest = r;
            Rahmen::ResetStream {
                stream_id,
                fehlercode,
                endgroesse,
            }
        }
        TYP_STOP_SENDING => {
            let (stream_id, r) = nimm_varint(rest)?;
            rest = r;
            let (fehlercode, r) = nimm_varint(rest)?;
            rest = r;
            Rahmen::StopSending {
                stream_id,
                fehlercode,
            }
        }
        TYP_CRYPTO => {
            let (offset, r) = nimm_varint(rest)?;
            rest = r;
            let (laenge, r) = nimm_laenge(rest, usize::MAX)?;
            rest = r;
            let (daten, r) = nimm(rest, laenge)?;
            rest = r;
            Rahmen::Crypto {
                offset,
                daten: daten.to_vec(),
            }
        }
        TYP_NEW_TOKEN => {
            let (laenge, r) = nimm_laenge(rest, crate::kopf::MAX_TOKEN_LAENGE)?;
            rest = r;
            let (token, r) = nimm(rest, laenge)?;
            rest = r;
            Rahmen::NewToken {
                token: token.to_vec(),
            }
        }
        TYP_STREAM_MIN..=TYP_STREAM_MAX => {
            let fin = typ & STREAM_FIN != 0;
            let hat_len = typ & STREAM_LEN != 0;
            let hat_off = typ & STREAM_OFF != 0;
            let (stream_id, r) = nimm_varint(rest)?;
            rest = r;
            let offset = if hat_off {
                let (o, r) = nimm_varint(rest)?;
                rest = r;
                o
            } else {
                0
            };
            let (daten, r) = if hat_len {
                let (laenge, r) = nimm_laenge(rest, usize::MAX)?;
                rest = r;
                let (d, r) = nimm(rest, laenge)?;
                (d.to_vec(), r)
            } else {
                // Ohne LEN reicht der Frame bis zum Ende des Pakets.
                (rest.to_vec(), &[][..])
            };
            rest = r;
            Rahmen::Stream {
                stream_id,
                offset,
                daten,
                fin,
            }
        }
        TYP_MAX_DATA => {
            let (maximum, r) = nimm_varint(rest)?;
            rest = r;
            Rahmen::MaxData { maximum }
        }
        TYP_MAX_STREAM_DATA => {
            let (stream_id, r) = nimm_varint(rest)?;
            rest = r;
            let (maximum, r) = nimm_varint(rest)?;
            rest = r;
            Rahmen::MaxStreamData { stream_id, maximum }
        }
        TYP_MAX_STREAMS_BIDI | TYP_MAX_STREAMS_UNI => {
            let (maximum, r) = nimm_varint(rest)?;
            rest = r;
            Rahmen::MaxStreams {
                bidirektional: typ == TYP_MAX_STREAMS_BIDI,
                maximum,
            }
        }
        TYP_DATA_BLOCKED => {
            let (maximum, r) = nimm_varint(rest)?;
            rest = r;
            Rahmen::DataBlocked { maximum }
        }
        TYP_STREAM_DATA_BLOCKED => {
            let (stream_id, r) = nimm_varint(rest)?;
            rest = r;
            let (maximum, r) = nimm_varint(rest)?;
            rest = r;
            Rahmen::StreamDataBlocked { stream_id, maximum }
        }
        TYP_STREAMS_BLOCKED_BIDI | TYP_STREAMS_BLOCKED_UNI => {
            let (maximum, r) = nimm_varint(rest)?;
            rest = r;
            Rahmen::StreamsBlocked {
                bidirektional: typ == TYP_STREAMS_BLOCKED_BIDI,
                maximum,
            }
        }
        TYP_NEW_CONNECTION_ID => {
            let (sequenz, r) = nimm_varint(rest)?;
            rest = r;
            let (zurueckziehen_vor, r) = nimm_varint(rest)?;
            rest = r;
            let (cid_len, r) = nimm(rest, 1)?;
            rest = r;
            let cid_len = cid_len[0] as usize;
            if cid_len > MAX_CID_LAENGE {
                return Err(RahmenFehler::CidZuLang {
                    ist: cid_len,
                    maximum: MAX_CID_LAENGE,
                });
            }
            let (cid, r) = nimm(rest, cid_len)?;
            rest = r;
            let (token, r) = nimm(rest, 16)?;
            rest = r;
            let mut reset_token = [0u8; 16];
            reset_token.copy_from_slice(token);
            Rahmen::NewConnectionId {
                sequenz,
                zurueckziehen_vor,
                cid: cid.to_vec(),
                reset_token,
            }
        }
        TYP_RETIRE_CONNECTION_ID => {
            let (sequenz, r) = nimm_varint(rest)?;
            rest = r;
            Rahmen::RetireConnectionId { sequenz }
        }
        TYP_PATH_CHALLENGE | TYP_PATH_RESPONSE => {
            let (d, r) = nimm(rest, 8)?;
            rest = r;
            let mut daten = [0u8; 8];
            daten.copy_from_slice(d);
            if typ == TYP_PATH_CHALLENGE {
                Rahmen::PathChallenge { daten }
            } else {
                Rahmen::PathResponse { daten }
            }
        }
        TYP_CONNECTION_CLOSE | TYP_CONNECTION_CLOSE_APP => {
            let transport = typ == TYP_CONNECTION_CLOSE;
            let (fehlercode, r) = nimm_varint(rest)?;
            rest = r;
            let frame_typ = if transport {
                let (f, r) = nimm_varint(rest)?;
                rest = r;
                f
            } else {
                0
            };
            let (laenge, r) = nimm_laenge(rest, MAX_GRUND_LAENGE)?;
            rest = r;
            let (grund, r) = nimm(rest, laenge)?;
            rest = r;
            Rahmen::ConnectionClose {
                transport,
                fehlercode,
                frame_typ,
                grund: grund.to_vec(),
            }
        }
        TYP_HANDSHAKE_DONE => Rahmen::HandshakeDone,
        // Unbekannter Frame-Typ: RFC 9000 Abschnitt 12.4 verlangt einen
        // Verbindungsfehler FRAME_ENCODING_ERROR. Hier ein benannter Fehler,
        // damit der Aufrufer entscheidet (kein Panic bei fremder Eingabe).
        //
        // DIE NORMSTELLE, WOERTLICH (RFC 9000, Tabelle 3, Anmerkung):
        //
        //     An endpoint MUST treat the receipt of a frame of unknown type
        //     as a connection error of type FRAME_ENCODING_ERROR.
        //
        // UND — das war mein offener Punkt vom 01.10.2026 — **QUIC kennt
        // gar kein reserviertes Frame-Muster.** Ich hatte im Verdacht, dass
        // RFC 9000 ein GREASE-Muster `0x1f * N` reserviert, und deshalb
        // stehen gelassen, ob hier statt des Fehlers uebersprungen werden
        // muesse.
        //
        // GEMESSEN (geprueft an der Norm, nicht aus dem Gedaechtnis):
        //     RFC 9000, 403 425 Byte Volltext:
        //         "GREASE"  -> 0 Treffer
        //         "0x1f"    -> 0 Treffer
        //     RFC 9000 Abschnitt 9 (Extensions): Erweiterungen werden ueber
        //     einen **Transport-Parameter** angekuendigt, nicht ueber
        //     reservierte Typen.
        //
        // Das reservierte Muster `0x1f * N + 0x21` gehoert zu **HTTP/3**
        // (RFC 9114 Abschnitt 7.2.8), nicht zu QUIC. Ich hatte die Regel aus
        // dem HTTP/3-Teil in den QUIC-Teil getragen — die Haelfte von Fehler 2
        // aus BERICHT_QUIC_U2.txt, nur auf der anderen Ebene. `h3_reserviert()`
        // in `h3.rs` traegt jetzt die Zahl, damit sie nicht noch einmal
        // heruebergetragen wird.
        _ => return Err(RahmenFehler::UnbekannterTyp { typ }),
    };
    Ok((rahmen, start - rest.len()))
}

/// Liest alle Frames aus einer Nutzlast, bis der Puffer leer ist.
///
/// M3: jeder Frame verbraucht mindestens ein Byte (den Typ-Varint), die
/// Schleife laeuft also hoechstens so oft wie die Eingabe Bytes hat.
pub fn rahmen_alle(daten: &[u8]) -> Result<Vec<Rahmen>, RahmenFehler> {
    let mut raus = Vec::new();
    let mut rest = daten;
    while !rest.is_empty() {
        let (rahmen, verbraucht) = rahmen_lesen(rest)?;
        // Ein Frame ohne Fortschritt waere eine Endlosschleife — nie erlaubt.
        debug_assert!(verbraucht > 0);
        raus.push(rahmen);
        rest = &rest[verbraucht..];
    }
    Ok(raus)
}

/// Schreibt einen Frame in den Puffer.
pub fn rahmen_schreiben(rahmen: &Rahmen, aus: &mut Vec<u8>) -> Result<(), RahmenFehler> {
    match rahmen {
        Rahmen::Padding => varint::schreibe(TYP_PADDING, aus)?,
        Rahmen::Ping => varint::schreibe(TYP_PING, aus)?,
        Rahmen::Ack {
            groesste,
            verzoegerung,
            erste_range,
            bereiche,
            ecn,
        } => {
            if bereiche.len() > MAX_ACK_BEREICHE {
                return Err(RahmenFehler::ZuVieleBereiche {
                    ist: bereiche.len() as u64,
                    maximum: MAX_ACK_BEREICHE,
                });
            }
            let typ = if ecn.is_some() { TYP_ACK_ECN } else { TYP_ACK };
            varint::schreibe(typ, aus)?;
            varint::schreibe(*groesste, aus)?;
            varint::schreibe(*verzoegerung, aus)?;
            varint::schreibe(bereiche.len() as u64, aus)?;
            varint::schreibe(*erste_range, aus)?;
            for b in bereiche {
                varint::schreibe(b.luecke, aus)?;
                varint::schreibe(b.laenge, aus)?;
            }
            if let Some([ect0, ect1, ce]) = ecn {
                varint::schreibe(*ect0, aus)?;
                varint::schreibe(*ect1, aus)?;
                varint::schreibe(*ce, aus)?;
            }
        }
        Rahmen::ResetStream {
            stream_id,
            fehlercode,
            endgroesse,
        } => {
            varint::schreibe(TYP_RESET_STREAM, aus)?;
            varint::schreibe(*stream_id, aus)?;
            varint::schreibe(*fehlercode, aus)?;
            varint::schreibe(*endgroesse, aus)?;
        }
        Rahmen::StopSending {
            stream_id,
            fehlercode,
        } => {
            varint::schreibe(TYP_STOP_SENDING, aus)?;
            varint::schreibe(*stream_id, aus)?;
            varint::schreibe(*fehlercode, aus)?;
        }
        Rahmen::Crypto { offset, daten } => {
            varint::schreibe(TYP_CRYPTO, aus)?;
            varint::schreibe(*offset, aus)?;
            varint::schreibe(daten.len() as u64, aus)?;
            aus.extend_from_slice(daten);
        }
        Rahmen::NewToken { token } => {
            varint::schreibe(TYP_NEW_TOKEN, aus)?;
            varint::schreibe(token.len() as u64, aus)?;
            aus.extend_from_slice(token);
        }
        Rahmen::Stream {
            stream_id,
            offset,
            daten,
            fin,
        } => {
            let mut typ = TYP_STREAM_MIN;
            if *fin {
                typ |= STREAM_FIN;
            }
            typ |= STREAM_LEN;
            if *offset != 0 {
                typ |= STREAM_OFF;
            }
            varint::schreibe(typ, aus)?;
            varint::schreibe(*stream_id, aus)?;
            if *offset != 0 {
                varint::schreibe(*offset, aus)?;
            }
            varint::schreibe(daten.len() as u64, aus)?;
            aus.extend_from_slice(daten);
        }
        Rahmen::MaxData { maximum } => {
            varint::schreibe(TYP_MAX_DATA, aus)?;
            varint::schreibe(*maximum, aus)?;
        }
        Rahmen::MaxStreamData { stream_id, maximum } => {
            varint::schreibe(TYP_MAX_STREAM_DATA, aus)?;
            varint::schreibe(*stream_id, aus)?;
            varint::schreibe(*maximum, aus)?;
        }
        Rahmen::MaxStreams {
            bidirektional,
            maximum,
        } => {
            let typ = if *bidirektional {
                TYP_MAX_STREAMS_BIDI
            } else {
                TYP_MAX_STREAMS_UNI
            };
            varint::schreibe(typ, aus)?;
            varint::schreibe(*maximum, aus)?;
        }
        Rahmen::DataBlocked { maximum } => {
            varint::schreibe(TYP_DATA_BLOCKED, aus)?;
            varint::schreibe(*maximum, aus)?;
        }
        Rahmen::StreamDataBlocked { stream_id, maximum } => {
            varint::schreibe(TYP_STREAM_DATA_BLOCKED, aus)?;
            varint::schreibe(*stream_id, aus)?;
            varint::schreibe(*maximum, aus)?;
        }
        Rahmen::StreamsBlocked {
            bidirektional,
            maximum,
        } => {
            let typ = if *bidirektional {
                TYP_STREAMS_BLOCKED_BIDI
            } else {
                TYP_STREAMS_BLOCKED_UNI
            };
            varint::schreibe(typ, aus)?;
            varint::schreibe(*maximum, aus)?;
        }
        Rahmen::NewConnectionId {
            sequenz,
            zurueckziehen_vor,
            cid,
            reset_token,
        } => {
            if cid.len() > MAX_CID_LAENGE {
                return Err(RahmenFehler::CidZuLang {
                    ist: cid.len(),
                    maximum: MAX_CID_LAENGE,
                });
            }
            varint::schreibe(TYP_NEW_CONNECTION_ID, aus)?;
            varint::schreibe(*sequenz, aus)?;
            varint::schreibe(*zurueckziehen_vor, aus)?;
            aus.push(cid.len() as u8);
            aus.extend_from_slice(cid);
            aus.extend_from_slice(reset_token);
        }
        Rahmen::RetireConnectionId { sequenz } => {
            varint::schreibe(TYP_RETIRE_CONNECTION_ID, aus)?;
            varint::schreibe(*sequenz, aus)?;
        }
        Rahmen::PathChallenge { daten } => {
            varint::schreibe(TYP_PATH_CHALLENGE, aus)?;
            aus.extend_from_slice(daten);
        }
        Rahmen::PathResponse { daten } => {
            varint::schreibe(TYP_PATH_RESPONSE, aus)?;
            aus.extend_from_slice(daten);
        }
        Rahmen::ConnectionClose {
            transport,
            fehlercode,
            frame_typ,
            grund,
        } => {
            if grund.len() > MAX_GRUND_LAENGE {
                return Err(RahmenFehler::ZuLang {
                    ist: grund.len(),
                    maximum: MAX_GRUND_LAENGE,
                });
            }
            let typ = if *transport {
                TYP_CONNECTION_CLOSE
            } else {
                TYP_CONNECTION_CLOSE_APP
            };
            varint::schreibe(typ, aus)?;
            varint::schreibe(*fehlercode, aus)?;
            if *transport {
                varint::schreibe(*frame_typ, aus)?;
            }
            varint::schreibe(grund.len() as u64, aus)?;
            aus.extend_from_slice(grund);
        }
        Rahmen::HandshakeDone => varint::schreibe(TYP_HANDSHAKE_DONE, aus)?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ein Beispiel aus RFC 9000 Abschnitt 19.3: ein ACK mit einem Bereich.
    #[test]
    fn ack_mit_einem_bereich() {
        let mut p = Vec::new();
        // ACK, groesste=0, verzoegerung=0, 0 Bereiche, erste_range=0.
        rahmen_schreiben(
            &Rahmen::Ack {
                groesste: 0,
                verzoegerung: 0,
                erste_range: 0,
                bereiche: vec![],
                ecn: None,
            },
            &mut p,
        )
        .expect("schreiben");
        assert_eq!(narsil_tls::sha2::hexen(&p), "0200000000", "ACK 0x02, 0,0,0,0");
        let (rahmen, n) = rahmen_lesen(&p).expect("lesen");
        assert_eq!(n, p.len(), "M7: alles verbraucht");
        assert!(matches!(rahmen, Rahmen::Ack { groesste: 0, .. }));
    }

    /// Ein ACK mit einem zusaetzlichen Bereich (Gap + Range Length).
    #[test]
    fn ack_mit_bereich_rundlauf() {
        let rahmen = Rahmen::Ack {
            groesste: 100,
            verzoegerung: 3,
            erste_range: 10,
            bereiche: vec![
                AckBereich {
                    luecke: 2,
                    laenge: 5,
                },
                AckBereich {
                    luecke: 1,
                    laenge: 7,
                },
            ],
            ecn: None,
        };
        let mut p = Vec::new();
        rahmen_schreiben(&rahmen, &mut p).expect("schreiben");
        let (gelesen, n) = rahmen_lesen(&p).expect("lesen");
        assert_eq!(n, p.len());
        assert_eq!(gelesen, rahmen, "M7: ACK-Rundlauf");
    }

    /// Ein ACK mit ECN-Zaehlern (Typ 0x03).
    #[test]
    fn ack_ecn_rundlauf() {
        let rahmen = Rahmen::Ack {
            groesste: 9,
            verzoegerung: 1,
            erste_range: 9,
            bereiche: vec![],
            ecn: Some([1, 2, 3]),
        };
        let mut p = Vec::new();
        rahmen_schreiben(&rahmen, &mut p).expect("schreiben");
        let (gelesen, _) = rahmen_lesen(&p).expect("lesen");
        assert_eq!(gelesen, rahmen, "M7: ACK-ECN-Rundlauf");
    }

    /// Ein STREAM-Frame mit OFF, LEN und FIN (Typ 0x0f).
    #[test]
    fn stream_mit_off_len_fin() {
        let rahmen = Rahmen::Stream {
            stream_id: 4,
            offset: 8,
            daten: b"hallo".to_vec(),
            fin: true,
        };
        let mut p = Vec::new();
        rahmen_schreiben(&rahmen, &mut p).expect("schreiben");
        // Typ 0x0f, Stream-ID 4, Offset 8, Laenge 5, "hallo".
        assert_eq!(narsil_tls::sha2::hexen(&p), "0f04080568616c6c6f");
        let (gelesen, n) = rahmen_lesen(&p).expect("lesen");
        assert_eq!(n, p.len());
        assert_eq!(gelesen, rahmen, "M7: STREAM-Rundlauf");
    }

    /// Ein STREAM-Frame OHNE Laenge reicht bis zum Ende des Puffers.
    #[test]
    fn stream_ohne_laenge_reicht_bis_ende() {
        // Typ 0x08 (keine Flags), Stream-ID 0, dann 3 Byte Daten.
        let p = narsil_tls::sha2::von_hex("0800010203").expect("hex");
        let (rahmen, n) = rahmen_lesen(&p).expect("lesen");
        assert_eq!(n, p.len());
        assert_eq!(
            rahmen,
            Rahmen::Stream {
                stream_id: 0,
                offset: 0,
                daten: vec![1, 2, 3],
                fin: false
            }
        );
    }

    /// Alle Frame-Typen einmal schreiben und zuruecklesen.
    #[test]
    fn alle_frame_typen_rundlauf() {
        let alle = vec![
            Rahmen::Padding,
            Rahmen::Ping,
            Rahmen::ResetStream {
                stream_id: 1,
                fehlercode: 2,
                endgroesse: 3,
            },
            Rahmen::StopSending {
                stream_id: 1,
                fehlercode: 2,
            },
            Rahmen::Crypto {
                offset: 0,
                daten: vec![1, 2, 3],
            },
            Rahmen::NewToken {
                token: b"t".to_vec(),
            },
            Rahmen::MaxData { maximum: 100 },
            Rahmen::MaxStreamData {
                stream_id: 2,
                maximum: 200,
            },
            Rahmen::MaxStreams {
                bidirektional: true,
                maximum: 3,
            },
            Rahmen::MaxStreams {
                bidirektional: false,
                maximum: 4,
            },
            Rahmen::DataBlocked { maximum: 100 },
            Rahmen::StreamDataBlocked {
                stream_id: 2,
                maximum: 200,
            },
            Rahmen::StreamsBlocked {
                bidirektional: true,
                maximum: 3,
            },
            Rahmen::StreamsBlocked {
                bidirektional: false,
                maximum: 4,
            },
            Rahmen::NewConnectionId {
                sequenz: 1,
                zurueckziehen_vor: 0,
                cid: vec![1, 2, 3, 4, 5, 6, 7, 8],
                reset_token: [9; 16],
            },
            Rahmen::RetireConnectionId { sequenz: 1 },
            Rahmen::PathChallenge { daten: [1; 8] },
            Rahmen::PathResponse { daten: [2; 8] },
            Rahmen::ConnectionClose {
                transport: true,
                fehlercode: 0x10,
                frame_typ: 0x06,
                grund: b"fehler".to_vec(),
            },
            Rahmen::ConnectionClose {
                transport: false,
                fehlercode: 0x10,
                frame_typ: 0,
                grund: vec![],
            },
            Rahmen::HandshakeDone,
        ];
        for rahmen in &alle {
            let mut p = Vec::new();
            rahmen_schreiben(rahmen, &mut p).expect("schreiben");
            let (gelesen, n) = rahmen_lesen(&p).expect("lesen");
            assert_eq!(n, p.len(), "M7: {rahmen:?} vollstaendig verbraucht");
            assert_eq!(&gelesen, rahmen, "M7: Rundlauf {rahmen:?}");
        }
    }

    /// M4: ein abgeschnittenes Laengenfeld ist ein benannter Fehler.
    #[test]
    fn abgeschnittenes_laengenfeld() {
        // CRYPTO: Typ 0x06, Offset 0, Laenge 10 — aber nur 2 Byte Daten.
        let p = narsil_tls::sha2::von_hex("06000a0102").expect("hex");
        assert!(matches!(
            rahmen_lesen(&p),
            Err(RahmenFehler::LaengeUeberPuffer {
                angegeben: 10,
                da: 2
            })
        ));
    }

    /// M4: zu viele ACK-Bereiche werden abgelehnt, bevor allokiert wird.
    #[test]
    fn zu_viele_ack_bereiche() {
        // ACK, groesste=0, verzoegerung=0, Bereichanzahl = MAX+1.
        let mut p = vec![0x02, 0x00, 0x00];
        varint::schreibe((MAX_ACK_BEREICHE + 1) as u64, &mut p).expect("varint");
        p.push(0x00); // erste_range
        assert!(matches!(
            rahmen_lesen(&p),
            Err(RahmenFehler::ZuVieleBereiche { maximum: 256, .. })
        ));
    }

    /// M7: mehrere Frames hintereinander werden alle gelesen.
    #[test]
    fn rahmen_folge() {
        let mut p = Vec::new();
        rahmen_schreiben(&Rahmen::Ping, &mut p).expect("ping");
        rahmen_schreiben(&Rahmen::HandshakeDone, &mut p).expect("done");
        let alle = rahmen_alle(&p).expect("alle");
        assert_eq!(alle.len(), 2, "M7: zwei Frames");
        assert!(matches!(alle[0], Rahmen::Ping));
        assert!(matches!(alle[1], Rahmen::HandshakeDone));
    }
}
