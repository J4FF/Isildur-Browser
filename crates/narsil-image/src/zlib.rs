//! zlib-Rahmen nach RFC 1950: zwei Kopfbytes, DEFLATE-Daten, vier
//! Adler-Bytes.
//!
//! Aufbau, woertlich aus der Spezifikation:
//!
//! ```text
//! +--------+------------------------+----------+
//! | CMF FLG | DEFLATE (variable len)  | Adler32 |
//! +--------+------------------------+----------+
//! ```
//!
//! * `CMF`: `CM = 8` (Methode DEFLATE) in den unteren vier Bits,
//!   `CINFO` (Fenster minus 32) in den oberen vier.
//! * `FLG`: `FDICT`, `FLEVEL` und fuenf Fuellbits, so dass
//!   `(CMF * 256 + FLG) % 31 == 0` gilt.
//! * Die letzten vier Byte sind der Adler32 der ENTPAckTEN Daten — nicht
//!   der der komprimierten. Das ist der eine Fehler, den jeder macht,
//!   der „Adler ueber alles drueber" schreibt.

use crate::FehlerZlib;
use crate::pruefsummen::adler32;

/// Baut den zlib-Rahmen um bereits DEFLATE-komprimierte Bytes.
///
/// `cmf`/`flg` muessen zusammen durch 31 teilbar sein; wer sie falsch
/// waehlt, erzeugt einen Strom, den jedes `zlib` ablehnt.
pub fn rahme(deflate_bytes: &[u8], rohdaten: &[u8]) -> Vec<u8> {
    // CMF = 0x78 heisst: Methode 8, CINFO 7 (Fenster 32 KiB = Maximum).
    // FLG = 0x01 heisst: kein Woerterbuch, kleinste Kompressionsstufe, und
    // 0x78 * 256 + 0x01 = 30721 = 31 * 991. Stimmt.
    const CMF: u8 = 0x78;
    const FLG: u8 = 0x01;
    debug_assert_eq!(((CMF as u32) << 8 | FLG as u32) % 31, 0);

    let mut out = Vec::with_capacity(deflate_bytes.len() + 6);
    out.push(CMF);
    out.push(FLG);
    out.extend_from_slice(deflate_bytes);
    let a = adler32(rohdaten).to_be_bytes();
    out.extend_from_slice(&a);
    out
}

/// Prueft, ob zwei Bytes ein gueltiger zlib-Kopf sind.
///
/// Steht hier, damit Enkoder und Dekoder dieselbe Definition benutzen —
/// zwei Kopien der Regel sind irgendwann eine Kopie zu viel.
pub fn kopf_gueltig(cmf: u8, flg: u8) -> bool {
    cmf & 0x0F == 8 && (cmf & 0xF0) >> 4 <= 7 && (((cmf as u32) << 8 | flg as u32) % 31 == 0)
}

/// Entpackt einen zlib-Strom in einen vorhandenen Puffer, mit harter Grenze.
pub fn entpacke_in(daten: &[u8], out: &mut Vec<u8>, grenze: usize) -> Result<(), FehlerZlib> {
    crate::inflate::entpacke_in(daten, out, grenze)
}

/// Der Vier-Byte-Adler am Ende weicht vom gerechneten ab.
///
/// Wird von `png_lesen` benutzt: eine Datei, deren entpackte Daten
/// stimmen, deren Adler aber nicht, ist beschaedigt. Ohne diese Pruefung
/// wuerde ein Bitfehler im Datenstrom still durchgehen.
pub fn adler_stimmt(daten: &[u8], entpackt: &[u8]) -> bool {
    if daten.len() < 6 {
        return false;
    }
    let n = daten.len();
    let erwartet = u32::from_be_bytes([daten[n - 4], daten[n - 3], daten[n - 2], daten[n - 1]]);
    erwartet == adler32(entpackt)
}
