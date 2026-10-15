//! gzip (RFC 1952) — DEFLATE mit gzip-Huelle und CRC32.
// W5F, 30.09.2026: die unbenutzten Importe am Dateikopf entfernt — der
// `FehlerZlib` und `STANDARD_GRENZE` wurden entfernt: der Code
// schreibt `crate::FehlerZlib` ausgeschrieben. `adler32` und
// `komprimiere` wurde auch entfernt: der Code ruft es ueber
// `crate::deflate::komprimiere(...)` auf (zlib.rs:29), nicht ueber den
// Import — ein Import, den niemand liest, ist ein Kommentar, der etwas
// behauptet, was nicht stattfindet.

use crate::pruefsummen::crc32;

/// Gzip-Komprimieren mit CRC32 und isize/Modtime=0.
pub fn gzip(daten: &[u8]) -> Vec<u8> {
    let deflate = crate::deflate::komprimiere(daten);
    let mut out = Vec::with_capacity(deflate.len() + 18);
    // Magic: 0x1f 0x8b
    out.push(0x1f);
    out.push(0x8b);
    // CM=8 (DEFLATE), FLG=0
    out.push(8);
    out.push(0);
    // MTIME=0, XFL=0, OS=255 (unknown)
    out.extend_from_slice(&0u32.to_le_bytes());
    out.push(0);
    out.push(255);
    out.extend_from_slice(&deflate);
    let crc = crc32(daten);
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(daten.len() as u32).to_le_bytes());
    out
}

/// Entpackt einen gzip-Strom in einen Puffer (mit Grenze).
pub fn gunzip(daten: &[u8], out: &mut Vec<u8>, grenze: usize) -> Result<(), crate::FehlerZlib> {
    if daten.len() < 10 || daten[0] != 0x1f || daten[1] != 0x8b {
        return Err(crate::FehlerZlib::GzipKopfFalsch {
            grund: "Magic Bytes fehlen".to_string(),
        });
    }
    let flags = daten[3];
    if flags & 0xE0 != 0 {
        return Err(crate::FehlerZlib::GzipFlagFalsch { flags });
    }
    let mut pos = 10;
    // FEXTRA
    if flags & 0x04 != 0 {
        if pos + 2 > daten.len() {
            return Err(crate::FehlerZlib::GzipKopfFalsch {
                grund: "FEXTRA zu kurz".to_string(),
            });
        }
        let xlen = u16::from_le_bytes([daten[pos], daten[pos + 1]]) as usize;
        pos += 2 + xlen;
    }
    // FNAME
    if flags & 0x08 != 0 {
        while pos < daten.len() && daten[pos] != 0 {
            pos += 1;
        }
        pos += 1;
    }
    // FCOMMENT
    if flags & 0x10 != 0 {
        while pos < daten.len() && daten[pos] != 0 {
            pos += 1;
        }
        pos += 1;
    }
    // FHCRC
    if flags & 0x02 != 0 {
        pos += 2;
    }
    if pos >= daten.len() {
        return Err(crate::FehlerZlib::GzipKopfFalsch {
            grund: "keine Daten".to_string(),
        });
    }
    // DEFLATE-Daten (alles bis letzte 8 Bytes)
    if daten.len() < pos + 8 {
        return Err(crate::FehlerZlib::GzipKopfFalsch {
            grund: "zu kurz".to_string(),
        });
    }
    let deflate_bytes = &daten[pos..daten.len() - 8];
    let crc_bytes = &daten[daten.len() - 8..daten.len() - 4];
    let isize_bytes = &daten[daten.len() - 4..];
    crate::inflate::deflate_entpacke(deflate_bytes, out, grenze)?;
    // CRC32 pruefen
    let crc = crc32(out);
    let erhalten = u32::from_le_bytes([crc_bytes[0], crc_bytes[1], crc_bytes[2], crc_bytes[3]]);
    if crc != erhalten {
        return Err(crate::FehlerZlib::Crc32Falsch {
            erwartet: crc,
            erhalten,
        });
    }
    // ISIZE pruefen (mod 2^32)
    let isize = u32::from_le_bytes([
        isize_bytes[0],
        isize_bytes[1],
        isize_bytes[2],
        isize_bytes[3],
    ]);
    if isize != out.len() as u32 {
        return Err(crate::FehlerZlib::GzipKopfFalsch {
            grund: "ISIZE stimmt nicht".to_string(),
        });
    }
    Ok(())
}

/// Komprimiert als gzip.
///
/// VORUEBERGEHEND: stillgelegt, added 30.09.2026 bei der Uebernahme von
/// `narsil-compress`. Das ist ein Duplikat von `gzip` mit einem
/// sprechenderen Namen — gedacht als Fassung fuer Aufrufer, die lesen
/// wollen, dass sie gzip bekommen. `lib.rs` exportiert inzwischen
/// `gzip` unter dem Namen `gzip_komprimiere`, und der zweite Weg wird
/// nicht gebraucht.
///
/// Der Code ist ein Einzeiler und bleibt lesbar stehen; `#[allow]`
/// statt Loeschen, weil die Entscheidung "zwei Namen fuer eine
/// Sache" eine bewusste sein sollte und nicht beim naechsten
/// Refactoring versehentlich.
#[allow(dead_code)]
pub fn komprimiere_gzip(daten: &[u8]) -> Vec<u8> {
    gzip(daten)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn gunzip_t1_gzip() {
        let gzip_data = fs::read("/home/jaff/mutation_md/koord/t1.gz").expect("t1.gz lesen");
        let expected = fs::read("/home/jaff/mutation_md/koord/t1.raw").expect("t1.raw lesen");
        let mut out = Vec::new();
        gunzip(&gzip_data, &mut out, 64 * 1024 * 1024).expect("gunzip t1.gz");
        assert_eq!(out, expected, "t1.gz entpackt nicht zu t1.raw");
    }

    #[test]
    fn gunzip_t2_gzip() {
        let gzip_data = fs::read("/home/jaff/mutation_md/koord/t2.gz").expect("t2.gz lesen");
        let expected = fs::read("/home/jaff/mutation_md/koord/t2.raw").expect("t2.raw lesen");
        let mut out = Vec::new();
        gunzip(&gzip_data, &mut out, 64 * 1024 * 1024).expect("gunzip t2.gz");
        assert_eq!(out, expected, "t2.gz entpackt nicht zu t2.raw");
    }

    #[test]
    fn gunzip_bombe_gzip() {
        let gzip_data = fs::read("/home/jaff/mutation_md/koord/bombe.gz").expect("bombe.gz lesen");
        let mut out = Vec::new();
        gunzip(&gzip_data, &mut out, 256 * 1024 * 1024).expect("gunzip bombe.gz");
        // bombe.gz enthaelt 200 MiB Nullen
        assert_eq!(
            out.len(),
            200 * 1024 * 1024,
            "bombe.gz Groesse stimmt nicht"
        );
        assert!(
            out.iter().all(|&b| b == 0),
            "bombe.gz enthaelt nicht nur Nullen"
        );
    }

    #[test]
    fn gzip_gunzip_rundlauf() {
        let original = b"Hallo Welt! ".repeat(1000);
        let komprimiert = gzip(&original);
        let mut out = Vec::new();
        gunzip(&komprimiert, &mut out, 64 * 1024 * 1024).expect("gunzip rundlauf");
        assert_eq!(out, original, "gzip/gunzip Rundlauf fehlgeschlagen");
    }
}
