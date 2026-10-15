//! QUIC-Transportparameter (RFC 9000, Abschnitt 18) und die
//! quic_transport_parameters-Erweiterung (RFC 9001, Abschnitt 8.2).
//!
//! Die Transportparameter werden im TLS-Handshake als Erweiterung vom Typ
//! 0x39 (57) uebertragen. Ihr Inhalt ist eine Folge von Dreiergruppen
//! (Parameter-ID, Parameter-Laenge, Parameter-Wert); ID und Laenge sind
//! Varints. Die ganzzahligen Parameter tragen ihren Wert selbst wieder als
//! Varint im Wert-Feld.
//!
//! Diese Crate baut und liest den Parameter-Block. Das Einhängen als
//! Erweiterung macht narsil-tls (siehe team/WUNSCH_DEEPSEEK.txt).
//!
//! M3/M4: die Anzahl der Parameter ist begrenzt, jede Laenge wird vor dem
//! Lesen geprueft, doppelte IDs sind ein benannter Fehler (RFC 9000
//! Abschnitt 18: TRANSPORT_PARAMETER_ERROR).

use crate::kopf::MAX_CID_LAENGE;
use crate::varint::{self, VarintFehler};

/// Der Erweiterungstyp der quic_transport_parameters-Erweiterung
/// (RFC 9001, Abschnitt 8.2).
pub const EXT_QUIC_TRANSPORT_PARAMETERS: u16 = 0x39;

// Parameter-IDs (RFC 9000, Abschnitt 18.2).
pub const PARAM_ORIGINAL_DCID: u64 = 0x00;
pub const PARAM_MAX_IDLE_TIMEOUT: u64 = 0x01;
pub const PARAM_STATELESS_RESET_TOKEN: u64 = 0x02;
pub const PARAM_MAX_UDP_PAYLOAD: u64 = 0x03;
pub const PARAM_INITIAL_MAX_DATA: u64 = 0x04;
pub const PARAM_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL: u64 = 0x05;
pub const PARAM_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE: u64 = 0x06;
pub const PARAM_INITIAL_MAX_STREAM_DATA_UNI: u64 = 0x07;
pub const PARAM_INITIAL_MAX_STREAMS_BIDI: u64 = 0x08;
pub const PARAM_INITIAL_MAX_STREAMS_UNI: u64 = 0x09;
pub const PARAM_ACK_DELAY_EXPONENT: u64 = 0x0a;
pub const PARAM_MAX_ACK_DELAY: u64 = 0x0b;
pub const PARAM_DISABLE_ACTIVE_MIGRATION: u64 = 0x0c;
pub const PARAM_PREFERRED_ADDRESS: u64 = 0x0d;
pub const PARAM_ACTIVE_CONNECTION_ID_LIMIT: u64 = 0x0e;
pub const PARAM_INITIAL_SOURCE_CID: u64 = 0x0f;
pub const PARAM_RETRY_SOURCE_CID: u64 = 0x10;

/// Hoechste Anzahl von Transportparametern in einem Block (M3). Ein
/// Endpunkt schickt realistisch unter 20; 64 ist grosszuegig und begrenzt
/// die Schleife gegen boesartige Eingabe.
pub const MAX_PARAMETER_ANZAHL: usize = 64;

/// Eine bevorzugte Adresse (Parameter 0x0d, RFC 9000 Abschnitt 18.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BevorzugteAdresse {
    pub ipv4: [u8; 4],
    pub port4: u16,
    pub ipv6: [u8; 16],
    pub port6: u16,
    pub cid: Vec<u8>,
    pub reset_token: [u8; 16],
}

/// Ein Transportparameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parameter {
    OriginalDestinationCid(Vec<u8>),
    MaxIdleTimeout(u64),
    StatelessResetToken([u8; 16]),
    MaxUdpPayloadSize(u64),
    InitialMaxData(u64),
    InitialMaxStreamDataBidiLocal(u64),
    InitialMaxStreamDataBidiRemote(u64),
    InitialMaxStreamDataUni(u64),
    InitialMaxStreamsBidi(u64),
    InitialMaxStreamsUni(u64),
    AckDelayExponent(u64),
    MaxAckDelay(u64),
    DisableActiveMigration,
    BevorzugteAdresse(BevorzugteAdresse),
    ActiveConnectionIdLimit(u64),
    InitialSourceCid(Vec<u8>),
    RetrySourceCid(Vec<u8>),
}

impl Parameter {
    /// Die Parameter-ID fuer die Kodierung.
    pub fn id(&self) -> u64 {
        match self {
            Parameter::OriginalDestinationCid(_) => PARAM_ORIGINAL_DCID,
            Parameter::MaxIdleTimeout(_) => PARAM_MAX_IDLE_TIMEOUT,
            Parameter::StatelessResetToken(_) => PARAM_STATELESS_RESET_TOKEN,
            Parameter::MaxUdpPayloadSize(_) => PARAM_MAX_UDP_PAYLOAD,
            Parameter::InitialMaxData(_) => PARAM_INITIAL_MAX_DATA,
            Parameter::InitialMaxStreamDataBidiLocal(_) => PARAM_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL,
            Parameter::InitialMaxStreamDataBidiRemote(_) => {
                PARAM_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE
            }
            Parameter::InitialMaxStreamDataUni(_) => PARAM_INITIAL_MAX_STREAM_DATA_UNI,
            Parameter::InitialMaxStreamsBidi(_) => PARAM_INITIAL_MAX_STREAMS_BIDI,
            Parameter::InitialMaxStreamsUni(_) => PARAM_INITIAL_MAX_STREAMS_UNI,
            Parameter::AckDelayExponent(_) => PARAM_ACK_DELAY_EXPONENT,
            Parameter::MaxAckDelay(_) => PARAM_MAX_ACK_DELAY,
            Parameter::DisableActiveMigration => PARAM_DISABLE_ACTIVE_MIGRATION,
            Parameter::BevorzugteAdresse(_) => PARAM_PREFERRED_ADDRESS,
            Parameter::ActiveConnectionIdLimit(_) => PARAM_ACTIVE_CONNECTION_ID_LIMIT,
            Parameter::InitialSourceCid(_) => PARAM_INITIAL_SOURCE_CID,
            Parameter::RetrySourceCid(_) => PARAM_RETRY_SOURCE_CID,
        }
    }
}

/// Warum ein Parameter-Block nicht gelesen oder geschrieben werden kann.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParameterFehler {
    /// Der Puffer endet mitten in einem Parameter.
    Abgeschnitten { braucht: usize, da: usize },
    /// Eine ID kam zweimal vor.
    DoppelteId { id: u64 },
    /// Zu viele Parameter.
    ZuViele { ist: usize, maximum: usize },
    /// Eine Connection-ID war zu lang.
    CidZuLang { ist: usize, maximum: usize },
    /// Der Wert passte nicht zum Parameter (z. B. falsche Laenge).
    WertFalsch { id: u64 },
    /// Ein Varint war kaputt.
    Varint(VarintFehler),
}

impl std::fmt::Display for ParameterFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParameterFehler::Abgeschnitten { braucht, da } => {
                write!(f, "Parameter braucht {braucht} Byte, da sind {da}")
            }
            ParameterFehler::DoppelteId { id } => write!(f, "Parameter-ID {id} kommt zweimal vor"),
            ParameterFehler::ZuViele { ist, maximum } => {
                write!(f, "{ist} Parameter, erlaubt sind {maximum}")
            }
            ParameterFehler::CidZuLang { ist, maximum } => {
                write!(f, "Connection-ID hat {ist} Byte, erlaubt sind {maximum}")
            }
            ParameterFehler::WertFalsch { id } => write!(f, "Wert zu Parameter {id} ist ungueltig"),
            ParameterFehler::Varint(e) => write!(f, "Varint im Parameter: {e}"),
        }
    }
}

impl std::error::Error for ParameterFehler {}

impl From<VarintFehler> for ParameterFehler {
    fn from(e: VarintFehler) -> Self {
        ParameterFehler::Varint(e)
    }
}

/// Liest einen ganzzahligen Parameterwert (ein einzelner Varint im Wert).
fn wert_varint(id: u64, wert: &[u8]) -> Result<u64, ParameterFehler> {
    let (v, verbraucht) = varint::lese(wert)?;
    if verbraucht != wert.len() {
        return Err(ParameterFehler::WertFalsch { id });
    }
    Ok(v)
}

/// Liest eine Connection-ID als Wert (0..20 Byte).
fn wert_cid(id: u64, wert: &[u8]) -> Result<Vec<u8>, ParameterFehler> {
    if wert.len() > MAX_CID_LAENGE {
        return Err(ParameterFehler::CidZuLang {
            ist: wert.len(),
            maximum: MAX_CID_LAENGE,
        });
    }
    let _ = id;
    Ok(wert.to_vec())
}

/// Liest alle Transportparameter aus einem Block.
pub fn parameter_lesen(daten: &[u8]) -> Result<Vec<Parameter>, ParameterFehler> {
    let mut raus = Vec::new();
    let mut rest = daten;
    while !rest.is_empty() {
        if raus.len() >= MAX_PARAMETER_ANZAHL {
            return Err(ParameterFehler::ZuViele {
                ist: raus.len() + 1,
                maximum: MAX_PARAMETER_ANZAHL,
            });
        }
        let (id, r) = varint::lese(rest)?;
        rest = &rest[r..];
        let (laenge, r) = varint::lese(rest)?;
        rest = &rest[r..];
        if rest.len() < laenge as usize {
            return Err(ParameterFehler::Abgeschnitten {
                braucht: laenge as usize,
                da: rest.len(),
            });
        }
        let wert = &rest[..laenge as usize];
        rest = &rest[laenge as usize..];

        if raus.iter().any(|p: &Parameter| p.id() == id) {
            return Err(ParameterFehler::DoppelteId { id });
        }

        let parameter = match id {
            PARAM_ORIGINAL_DCID => Parameter::OriginalDestinationCid(wert_cid(id, wert)?),
            PARAM_MAX_IDLE_TIMEOUT => Parameter::MaxIdleTimeout(wert_varint(id, wert)?),
            PARAM_STATELESS_RESET_TOKEN => {
                if wert.len() != 16 {
                    return Err(ParameterFehler::WertFalsch { id });
                }
                let mut t = [0u8; 16];
                t.copy_from_slice(wert);
                Parameter::StatelessResetToken(t)
            }
            PARAM_MAX_UDP_PAYLOAD => Parameter::MaxUdpPayloadSize(wert_varint(id, wert)?),
            PARAM_INITIAL_MAX_DATA => Parameter::InitialMaxData(wert_varint(id, wert)?),
            PARAM_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL => {
                Parameter::InitialMaxStreamDataBidiLocal(wert_varint(id, wert)?)
            }
            PARAM_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE => {
                Parameter::InitialMaxStreamDataBidiRemote(wert_varint(id, wert)?)
            }
            PARAM_INITIAL_MAX_STREAM_DATA_UNI => {
                Parameter::InitialMaxStreamDataUni(wert_varint(id, wert)?)
            }
            PARAM_INITIAL_MAX_STREAMS_BIDI => {
                Parameter::InitialMaxStreamsBidi(wert_varint(id, wert)?)
            }
            PARAM_INITIAL_MAX_STREAMS_UNI => {
                Parameter::InitialMaxStreamsUni(wert_varint(id, wert)?)
            }
            PARAM_ACK_DELAY_EXPONENT => Parameter::AckDelayExponent(wert_varint(id, wert)?),
            PARAM_MAX_ACK_DELAY => Parameter::MaxAckDelay(wert_varint(id, wert)?),
            PARAM_DISABLE_ACTIVE_MIGRATION => {
                if !wert.is_empty() {
                    return Err(ParameterFehler::WertFalsch { id });
                }
                Parameter::DisableActiveMigration
            }
            PARAM_PREFERRED_ADDRESS => {
                Parameter::BevorzugteAdresse(bevorzugte_adresse_lesen(wert)?)
            }
            PARAM_ACTIVE_CONNECTION_ID_LIMIT => {
                Parameter::ActiveConnectionIdLimit(wert_varint(id, wert)?)
            }
            PARAM_INITIAL_SOURCE_CID => Parameter::InitialSourceCid(wert_cid(id, wert)?),
            PARAM_RETRY_SOURCE_CID => Parameter::RetrySourceCid(wert_cid(id, wert)?),
            // Unbekannte Parameter werden ignoriert (RFC 9000 Abschnitt 18).
            _ => continue,
        };
        raus.push(parameter);
    }
    Ok(raus)
}

/// Liest eine bevorzugte Adresse (RFC 9000 Abschnitt 18.2, Abbildung 22).
fn bevorzugte_adresse_lesen(wert: &[u8]) -> Result<BevorzugteAdresse, ParameterFehler> {
    // 4 + 2 + 16 + 2 + 1 = 25 Byte fest, dann CID und 16 Byte Token.
    if wert.len() < 25 + 16 {
        return Err(ParameterFehler::WertFalsch {
            id: PARAM_PREFERRED_ADDRESS,
        });
    }
    let mut ipv4 = [0u8; 4];
    ipv4.copy_from_slice(&wert[0..4]);
    let port4 = u16::from_be_bytes([wert[4], wert[5]]);
    let mut ipv6 = [0u8; 16];
    ipv6.copy_from_slice(&wert[6..22]);
    let port6 = u16::from_be_bytes([wert[22], wert[23]]);
    let cid_len = wert[24] as usize;
    if cid_len > MAX_CID_LAENGE {
        return Err(ParameterFehler::CidZuLang {
            ist: cid_len,
            maximum: MAX_CID_LAENGE,
        });
    }
    if wert.len() != 25 + cid_len + 16 {
        return Err(ParameterFehler::WertFalsch {
            id: PARAM_PREFERRED_ADDRESS,
        });
    }
    let cid = wert[25..25 + cid_len].to_vec();
    let mut reset_token = [0u8; 16];
    reset_token.copy_from_slice(&wert[25 + cid_len..25 + cid_len + 16]);
    Ok(BevorzugteAdresse {
        ipv4,
        port4,
        ipv6,
        port6,
        cid,
        reset_token,
    })
}

/// Schreibt den Wert eines Parameters (ohne ID und Laenge) in den Puffer.
fn wert_schreiben(parameter: &Parameter, aus: &mut Vec<u8>) -> Result<(), ParameterFehler> {
    match parameter {
        Parameter::OriginalDestinationCid(cid)
        | Parameter::InitialSourceCid(cid)
        | Parameter::RetrySourceCid(cid) => {
            if cid.len() > MAX_CID_LAENGE {
                return Err(ParameterFehler::CidZuLang {
                    ist: cid.len(),
                    maximum: MAX_CID_LAENGE,
                });
            }
            aus.extend_from_slice(cid);
        }
        Parameter::MaxIdleTimeout(v)
        | Parameter::MaxUdpPayloadSize(v)
        | Parameter::InitialMaxData(v)
        | Parameter::InitialMaxStreamDataBidiLocal(v)
        | Parameter::InitialMaxStreamDataBidiRemote(v)
        | Parameter::InitialMaxStreamDataUni(v)
        | Parameter::InitialMaxStreamsBidi(v)
        | Parameter::InitialMaxStreamsUni(v)
        | Parameter::AckDelayExponent(v)
        | Parameter::MaxAckDelay(v)
        | Parameter::ActiveConnectionIdLimit(v) => {
            varint::schreibe(*v, aus)?;
        }
        Parameter::StatelessResetToken(t) => aus.extend_from_slice(t),
        Parameter::DisableActiveMigration => {}
        Parameter::BevorzugteAdresse(a) => {
            if a.cid.len() > MAX_CID_LAENGE {
                return Err(ParameterFehler::CidZuLang {
                    ist: a.cid.len(),
                    maximum: MAX_CID_LAENGE,
                });
            }
            aus.extend_from_slice(&a.ipv4);
            aus.extend_from_slice(&a.port4.to_be_bytes());
            aus.extend_from_slice(&a.ipv6);
            aus.extend_from_slice(&a.port6.to_be_bytes());
            aus.push(a.cid.len() as u8);
            aus.extend_from_slice(&a.cid);
            aus.extend_from_slice(&a.reset_token);
        }
    }
    Ok(())
}

/// Schreibt alle Transportparameter als Block.
pub fn parameter_schreiben(
    parameter: &[Parameter],
    aus: &mut Vec<u8>,
) -> Result<(), ParameterFehler> {
    if parameter.len() > MAX_PARAMETER_ANZAHL {
        return Err(ParameterFehler::ZuViele {
            ist: parameter.len(),
            maximum: MAX_PARAMETER_ANZAHL,
        });
    }
    // Doppelte IDs sind verboten (RFC 9000 Abschnitt 18).
    for (i, p) in parameter.iter().enumerate() {
        if parameter[..i].iter().any(|q| q.id() == p.id()) {
            return Err(ParameterFehler::DoppelteId { id: p.id() });
        }
    }
    for p in parameter {
        let mut wert = Vec::new();
        wert_schreiben(p, &mut wert)?;
        varint::schreibe(p.id(), aus)?;
        varint::schreibe(wert.len() as u64, aus)?;
        aus.extend_from_slice(&wert);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Der Transportparameter-Block aus RFC 9001, Anhang A.2 (im
    /// ClientHello, Erweiterung 0x0039). Woertlich uebernommen.
    #[test]
    fn rfc9001_a2_transportparameter() {
        let blob = narsil_tls::sha2::von_hex(
            "0408ffffffffffffffff05048000ffff07048000ffff0801100104800075\
             300901100f088394c8f03e51570806048000ffff",
        )
        .expect("Hex");
        let params = parameter_lesen(&blob).expect("lesen");
        assert_eq!(params.len(), 8, "M7: 8 Parameter");

        assert_eq!(params[0], Parameter::InitialMaxData((1 << 62) - 1));
        assert_eq!(params[1], Parameter::InitialMaxStreamDataBidiLocal(0xffff));
        assert_eq!(params[2], Parameter::InitialMaxStreamDataUni(0xffff));
        assert_eq!(params[3], Parameter::InitialMaxStreamsBidi(16));
        assert_eq!(params[4], Parameter::MaxIdleTimeout(0x7530));
        assert_eq!(params[5], Parameter::InitialMaxStreamsUni(16));
        assert_eq!(
            params[6],
            Parameter::InitialSourceCid(narsil_tls::sha2::von_hex("8394c8f03e515708").unwrap())
        );
        assert_eq!(params[7], Parameter::InitialMaxStreamDataBidiRemote(0xffff));

        // Und zurueck: bytegleich mit dem RFC-Block.
        let mut raus = Vec::new();
        parameter_schreiben(&params, &mut raus).expect("schreiben");
        assert_eq!(raus, blob, "M7: Rundlauf bytegleich");
    }

    /// Ein Rundlauf ueber alle Parameterarten.
    #[test]
    fn alle_parameter_rundlauf() {
        let alle = vec![
            Parameter::OriginalDestinationCid(vec![1, 2, 3]),
            Parameter::MaxIdleTimeout(30_000),
            Parameter::StatelessResetToken([7; 16]),
            Parameter::MaxUdpPayloadSize(65527),
            Parameter::InitialMaxData(1 << 20),
            Parameter::InitialMaxStreamDataBidiLocal(1 << 10),
            Parameter::InitialMaxStreamDataBidiRemote(1 << 10),
            Parameter::InitialMaxStreamDataUni(1 << 10),
            Parameter::InitialMaxStreamsBidi(100),
            Parameter::InitialMaxStreamsUni(3),
            Parameter::AckDelayExponent(3),
            Parameter::MaxAckDelay(25),
            Parameter::DisableActiveMigration,
            Parameter::BevorzugteAdresse(BevorzugteAdresse {
                ipv4: [192, 0, 2, 1],
                port4: 443,
                ipv6: [0x20, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
                port6: 443,
                cid: vec![9, 8, 7, 6, 5, 4, 3, 2],
                reset_token: [1; 16],
            }),
            Parameter::ActiveConnectionIdLimit(2),
            Parameter::InitialSourceCid(vec![4, 5, 6]),
            Parameter::RetrySourceCid(vec![7, 8, 9]),
        ];
        let mut p = Vec::new();
        parameter_schreiben(&alle, &mut p).expect("schreiben");
        let gelesen = parameter_lesen(&p).expect("lesen");
        assert_eq!(gelesen, alle, "M7: alle Parameter rundlauf");
    }

    /// M4: eine doppelte ID ist ein benannter Fehler.
    #[test]
    fn doppelte_id_wird_abgelehnt() {
        let mut p = Vec::new();
        parameter_schreiben(
            &[Parameter::MaxIdleTimeout(1), Parameter::MaxIdleTimeout(2)],
            &mut p,
        )
        .expect_err("schreiben muss scheitern");

        // Und beim Lesen: zwei gleiche IDs hintereinander.
        let blob = narsil_tls::sha2::von_hex("010101010102").expect("hex");
        assert!(matches!(
            parameter_lesen(&blob),
            Err(ParameterFehler::DoppelteId { id: 1 })
        ));
    }

    /// M4: ein abgeschnittenes Laengenfeld ist ein benannter Fehler.
    #[test]
    fn abgeschnitten_ist_ein_benannter_fehler() {
        // ID 0x01 (max_idle_timeout), Laenge 64 (Varint "4040"), aber nur
        // 2 Byte Wert.
        let blob = narsil_tls::sha2::von_hex("0140400001").expect("hex");
        assert!(matches!(
            parameter_lesen(&blob),
            Err(ParameterFehler::Abgeschnitten { braucht: 64, da: 2 })
        ));
    }

    /// M3: zu viele Parameter werden beim SCHREIBEN abgelehnt, bevor die
    /// Duplikat-Pruefung laeuft (die Laengen-Grenze steht zuerst).
    #[test]
    fn zu_viele_parameter() {
        let viele: Vec<Parameter> =
            vec![Parameter::DisableActiveMigration; MAX_PARAMETER_ANZAHL + 1];
        let mut p = Vec::new();
        assert!(matches!(
            parameter_schreiben(&viele, &mut p),
            Err(ParameterFehler::ZuViele {
                ist: 65,
                maximum: 64
            })
        ));
    }
}
