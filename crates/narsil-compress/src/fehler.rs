//! Fehlerarten fuer Kompression/Entkompression.
use std::fmt;

/// Fehler beim Entpacken/Entkomprimieren (zlib, DEFLATE, gzip).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FehlerZlib {
    /// Die Eingabe ist zu kurz oder unerwartet zu Ende.
    DatenZuEnde { byte: usize },
    /// CMF/FLG ungueltig: Methode != 8, CINFO > 7, Pruefsumme falsch.
    UnbekannteMethode { cmf: u8, flg: u8 },
    /// FCHECK fehlgeschlagen: (CMF * 256 + FLG) % 31 != 0.
    FalschePruefsumme { cmf: u8, flg: u8 },
    /// FDICT-Bit gesetzt — praediktion nicht unterstuetzt.
    Praediktionswoerterbuch,
    /// Unbekannter Blocktyp (nicht 0, 1, 2).
    UnbekannterBlocktyp { typ: u8 },
    /// Huffman-Code ungueltig (ueberzaehlt, unvollstaendig, etc.).
    HuffmanFalsch { grund: String },
    /// Symbol in der Literal-/Laengen-Tabelle nicht belegt.
    SymbolNichtBelegt { wert: u16 },
    /// Distanz-Symbol nicht belegt oder ausserhalb 1..32768.
    DistanzFalsch { wert: u16 },
    /// Match zeigt hinter den aktuellen Pufferinhalt.
    MatchUeberEnde { laenge: u16, position: usize },
    /// Ausgabe wuerde `grenze` ueberschreiten.
    ZuGross { grenze: usize },
    /// Gespeicherter Block: LEN und NLEN stimmen nicht ueberein.
    GespeichertFalsch { len: u16, nlen: u16 },
    /// Adler32 am Ende weicht vom berechneten ab.
    Adler32Falsch { erwartet: u32, erhalten: u32 },
    /// CRC32 (gzip) weicht ab.
    Crc32Falsch { erwartet: u32, erhalten: u32 },
    /// gzip-Kopf ungueltig (Magic Bytes, Methode, Flags).
    GzipKopfFalsch { grund: String },
    /// gzip-Flags ungueltig/ungestattet.
    GzipFlagFalsch { flags: u8 },
    /// Brotli-Kopf ungueltig (Magic Bytes, Fenster-Groesse, etc.).
    BrotliKopfFalsch { grund: String },
    /// Brotli-Dekodierungsfehler.
    BrotliDekodierFehler { grund: String },
    /// Feature noch nicht implementiert.
    NichtImplementiert { feature: String },
}

impl fmt::Display for FehlerZlib {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FehlerZlib::DatenZuEnde { byte } => {
                write!(f, "Daten enden unerwartet bei Byte {}", byte)
            }
            FehlerZlib::UnbekannteMethode { cmf, flg } => {
                write!(f, "ungueltige Methode: CMF={:#x}, FLG={:#x}", cmf, flg)
            }
            FehlerZlib::FalschePruefsumme { cmf, flg } => {
                write!(f, "Kopf-Pruefsumme falsch: CMF={:#x}, FLG={:#x}", cmf, flg)
            }
            FehlerZlib::Praediktionswoerterbuch => write!(
                f,
                "FDICT gesetzt — praediktionswoerterbuch nicht unterstuetzt"
            ),
            FehlerZlib::UnbekannterBlocktyp { typ } => write!(f, "unbekannter Blocktyp {}", typ),
            FehlerZlib::HuffmanFalsch { grund } => write!(f, "Huffman-Code ungueltig: {}", grund),
            FehlerZlib::SymbolNichtBelegt { wert } => {
                write!(f, "Symbol {} nicht in Huffman-Tabelle", wert)
            }
            FehlerZlib::DistanzFalsch { wert } => write!(f, "Distanz-Symbol {} ungueltig", wert),
            FehlerZlib::MatchUeberEnde { laenge, position } => write!(
                f,
                "Match ({} Bytes) zeigt hinter Pufferende (Pos {})",
                laenge, position
            ),
            FehlerZlib::ZuGross { grenze } => {
                write!(f, "Ausgabe wuerde Grenze {} Bytes ueberschreiten", grenze)
            }
            FehlerZlib::GespeichertFalsch { len, nlen } => {
                write!(f, "Gespeicherter Block: LEN={} != NLEN={}", len, nlen)
            }
            FehlerZlib::Adler32Falsch { erwartet, erhalten } => write!(
                f,
                "Adler32 falsch: erwartet {:#x}, erhalten {:#x}",
                erwartet, erhalten
            ),
            FehlerZlib::Crc32Falsch { erwartet, erhalten } => write!(
                f,
                "CRC32 falsch: erwartet {:#x}, erhalten {:#x}",
                erwartet, erhalten
            ),
            FehlerZlib::GzipKopfFalsch { grund } => write!(f, "gzip-Kopf ungueltig: {}", grund),
            FehlerZlib::GzipFlagFalsch { flags } => write!(f, "gzip-Flags ungueltig: {:#x}", flags),
            FehlerZlib::BrotliKopfFalsch { grund } => write!(f, "Brotli-Kopf ungueltig: {}", grund),
            FehlerZlib::BrotliDekodierFehler { grund } => {
                write!(f, "Brotli-Dekodierungsfehler: {}", grund)
            }
            FehlerZlib::NichtImplementiert { feature } => {
                write!(f, "Nicht implementiert: {}", feature)
            }
        }
    }
}

impl std::error::Error for FehlerZlib {}
