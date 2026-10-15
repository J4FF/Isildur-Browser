// W5F, 30.09.2026: der `use` stand hier am Dateikopf, wurde aber nur von
// `mod tests` gebraucht — der Code selbst schreibt `crate::FehlerZlib`
// ueberall ausgeschrieben. Also gehoert der Import in die Tests.
//! Block-Verarbeitung (gespeichert, feste Huffman, dynamische Huffman).

// VORUEBERGEHEND: `FENSTER` (32768) war hier unbenutzt.
//
// added 30.09.2026 bei der Uebernahme von `narsil-compress`. Der Wert
// ist die maximale Fensterdistanz aus RFC 1951 Abschnitt 3.2.5 und
// stimmt; benutzt wird er erst, wenn der Entpacker Sliding-Window-
//Ausgaben ueber Blockgrenzen hinweg pflegt (das `output_buffer` aus
// RFC 1951 Abschnitt 3.2.5). Der jetzige Entpacker schreibt direkt
// in `out` und braucht es nicht.
//
// Auskommentiert statt geloescht, damit der Wert mit seiner
// Fundstelle im RFC dort steht, wo er gebraucht wird.
const _FENSTER: usize = 32768;

/// Liest einen gespeicherten Block (Typ 0).
pub fn block_gespeichert(
    b: &mut crate::inflate::bitleser::Bitleser<'_>,
    out: &mut Vec<u8>,
    grenze: usize,
) -> Result<(), crate::FehlerZlib> {
    b.richtung_bytegrenze();
    let kopf = b.leses_bytes(4)?;
    let len = u16::from_le_bytes([kopf[0], kopf[1]]);
    let nlen = u16::from_le_bytes([kopf[2], kopf[3]]);
    if len != !nlen {
        return Err(crate::FehlerZlib::GespeichertFalsch { len, nlen });
    }
    if out.len() + len as usize > grenze {
        return Err(crate::FehlerZlib::ZuGross { grenze });
    }
    let stueck = b.leses_bytes(len as usize)?;
    out.extend_from_slice(stueck);
    Ok(())
}

/// Liest einen Huffman-kodierten Block (Typ 1 oder 2) und fuehrt ihn aus.
pub fn block_huffman(
    b: &mut crate::inflate::bitleser::Bitleser<'_>,
    l: &crate::inflate::huffman::Huffman,
    d: &crate::inflate::huffman::Huffman,
    out: &mut Vec<u8>,
    grenze: usize,
) -> Result<(), crate::FehlerZlib> {
    //
    // DIE VIER BASIS-TABELLEN werden ueber den vollstaendigen Pfad
    // benutzt (`crate::inflate::huffman::LAENGEN_BASIS` usw., siehe
    // weiter unten). Der Import stand hier und wurde nie gelesen —
    // ein `use`, den der naechste, der hier etwas aendert, als
    // "geprueft" missversteht. W5F, 30.09.2026: entfernt, weil
    // `cargo clippy` (den diese Crate jetzt von mir beantwortet) die
    // unbenutzten Importe in drei Dateien meldete.
    loop {
        let sym = l.symbol(b)?;
        if sym < 256 {
            if out.len() + 1 > grenze {
                return Err(crate::FehlerZlib::ZuGross { grenze });
            }
            out.push(sym as u8);
        } else if sym == 256 {
            return Ok(());
        } else {
            // 257..285: Laenge/Distanz-Paar.
            let idx = sym as usize - 257;
            if idx >= crate::inflate::huffman::LAENGEN_BASIS.len() {
                return Err(crate::FehlerZlib::SymbolNichtBelegt { wert: sym });
            }
            let laenge = crate::inflate::huffman::LAENGEN_BASIS[idx] as usize
                + b.leses_bits(crate::inflate::huffman::LAENGEN_EXTRA[idx] as u32)? as usize;

            let dsym = d.symbol(b)? as usize;
            if dsym >= crate::inflate::huffman::DISTANZ_BASIS.len() {
                return Err(crate::FehlerZlib::DistanzFalsch { wert: dsym as u16 });
            }
            let distanz = crate::inflate::huffman::DISTANZ_BASIS[dsym] as usize
                + b.leses_bits(crate::inflate::huffman::DISTANZ_EXTRA[dsym] as u32)? as usize;
            if distanz == 0 || distanz > 32768 {
                return Err(crate::FehlerZlib::DistanzFalsch {
                    wert: distanz as u16,
                });
            }
            if distanz > out.len() {
                return Err(crate::FehlerZlib::MatchUeberEnde {
                    laenge: laenge as u16,
                    position: out.len(),
                });
            }
            if out.len() + laenge > grenze {
                return Err(crate::FehlerZlib::ZuGross { grenze });
            }
            let start = out.len() - distanz;
            for k in 0..laenge {
                let byte = out[start + k];
                out.push(byte);
            }
        }
    }
}
