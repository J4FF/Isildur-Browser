//! zlib (RFC 1950) und DEFLATE (RFC 1951) — der Leser.
mod bitleser;
mod bloecke;
pub mod huffman;

use crate::FehlerZlib;
// W5F, 30.09.2026: `STANDARD_GRENZE` und `adler32` standen hier und
// wurden nicht gelesen — der Code schreibt `crate::pruefsummen::adler32`
// ausgeschrieben (Zeile 72). Unbenutzte Importe sind kein Kleingeld:
// sie sagen einem Leser "hier wird etwas geprueft", wenn hier nichts
// geprueft wird.

/// Entpackt einen zlib-Strom (Kopf + DEFLATE-Daten + Adler32).
///
/// `grenze` ist die HARTE Obergrenze der Ausgabe in Byte. Sie wird nicht
/// geraten, sondern vom Aufrufer aus der IHDR gerechnet. Ein zlib-Strom
/// kann beliebig viele Null-Bytes erzeugen; ohne diese Grenze wuerde
/// "1x1-Bild, das 40 GB entpackt" den Tab beenden.
pub fn entpacke(daten: &[u8], grenze: usize) -> Result<Vec<u8>, FehlerZlib> {
    let mut out = Vec::with_capacity(grenze.min(1 << 16));
    entpacke_in(daten, &mut out, grenze)?;
    Ok(out)
}

/// Wie [`entpacke`], schreibt aber in einen vorhandenen Puffer.
pub fn entpacke_in(daten: &[u8], out: &mut Vec<u8>, grenze: usize) -> Result<(), FehlerZlib> {
    out.clear();

    // --- zlib-Kopf (RFC 1950 Abschnitt 2.2) ---
    if daten.len() < 2 {
        return Err(FehlerZlib::DatenZuEnde { byte: daten.len() });
    }
    let cmf = daten[0];
    let flg = daten[1];
    if cmf & 0x0F != 8 {
        return Err(FehlerZlib::UnbekannteMethode { cmf, flg });
    }
    if (cmf & 0xF0) >> 4 > 7 {
        return Err(FehlerZlib::UnbekannteMethode { cmf, flg });
    }
    if ((cmf as u32) << 8 | flg as u32) % 31 != 0 {
        return Err(FehlerZlib::FalschePruefsumme { cmf, flg });
    }
    if flg & 0x20 != 0 {
        return Err(FehlerZlib::Praediktionswoerterbuch);
    }

    let mut b = bitleser::Bitleser::neu(&daten[2..]);

    loop {
        let final_block = b.leses_bit()? != 0;
        let typ = b.leses_bits(2)?;
        match typ {
            0 => bloecke::block_gespeichert(&mut b, out, grenze)?,
            1 => {
                let (l, d) = huffman::huffman_fest();
                bloecke::block_huffman(&mut b, &l, &d, out, grenze)?;
            }
            2 => {
                let (l, d) = huffman::huffman_dynamisch(&mut b)?;
                bloecke::block_huffman(&mut b, &l, &d, out, grenze)?;
            }
            _ => return Err(FehlerZlib::UnbekannterBlocktyp { typ: typ as u8 }),
        }
        if final_block {
            break;
        }
    }

    // Adler32 pruefen (letzte 4 Bytes)
    if b.verfuegbar() < 4 {
        return Err(FehlerZlib::DatenZuEnde { byte: b.position() });
    }
    let adler_bytes = b.leses_bytes(4)?;
    let erhalten = u32::from_be_bytes([
        adler_bytes[0],
        adler_bytes[1],
        adler_bytes[2],
        adler_bytes[3],
    ]);
    let erwartet = crate::pruefsummen::adler32(out);
    if erhalten != erwartet {
        return Err(FehlerZlib::Adler32Falsch { erwartet, erhalten });
    }

    Ok(())
}

/// Like [`deflate_entpacke`], but also returns how many input bytes the
/// DEFLATE stream used (up to the byte boundary after the final block) —
/// the caller checks the trailer and reports trailing data (Compression
/// Streams: extra input after the end of the stream is an error).
/// Added 2026-10-15.
pub fn deflate_entpacke_laenge(
    deflate_daten: &[u8],
    out: &mut Vec<u8>,
    grenze: usize,
) -> Result<usize, FehlerZlib> {
    out.clear();
    let mut b = bitleser::Bitleser::neu(deflate_daten);
    loop {
        let final_block = b.leses_bit()? != 0;
        let typ = b.leses_bits(2)?;
        match typ {
            0 => bloecke::block_gespeichert(&mut b, out, grenze)?,
            1 => {
                let (l, d) = huffman::huffman_fest();
                bloecke::block_huffman(&mut b, &l, &d, out, grenze)?;
            }
            2 => {
                let (l, d) = huffman::huffman_dynamisch(&mut b)?;
                bloecke::block_huffman(&mut b, &l, &d, out, grenze)?;
            }
            _ => return Err(FehlerZlib::UnbekannterBlocktyp { typ: typ as u8 }),
        }
        if final_block {
            break;
        }
    }
    b.richtung_bytegrenze();
    Ok(b.position())
}

/// Entpackt rohe DEFLATE-Daten (ohne zlib-Kopf, mit CRC32 statt Adler32).
/// Wird fuer gzip (RFC 1952) benoetigt.
///
/// `grenze` ist die HARTE Obergrenze der Ausgabe in Byte.
pub fn deflate_entpacke(
    deflate_daten: &[u8],
    out: &mut Vec<u8>,
    grenze: usize,
) -> Result<(), FehlerZlib> {
    out.clear();
    let mut b = bitleser::Bitleser::neu(deflate_daten);

    loop {
        let final_block = b.leses_bit()? != 0;
        let typ = b.leses_bits(2)?;
        match typ {
            0 => bloecke::block_gespeichert(&mut b, out, grenze)?,
            1 => {
                let (l, d) = huffman::huffman_fest();
                bloecke::block_huffman(&mut b, &l, &d, out, grenze)?;
            }
            2 => {
                let (l, d) = huffman::huffman_dynamisch(&mut b)?;
                bloecke::block_huffman(&mut b, &l, &d, out, grenze)?;
            }
            _ => return Err(FehlerZlib::UnbekannterBlocktyp { typ: typ as u8 }),
        }
        if final_block {
            break;
        }
    }

    // Kein Adler32 bei rohem DEFLATE (gzip nutzt CRC32 am Ende des gzip-Stroms,
    // nicht am Ende der DEFLATE-Daten).
    Ok(())
}
