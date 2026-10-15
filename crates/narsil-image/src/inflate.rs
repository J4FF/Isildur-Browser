//! zlib (RFC 1950) und DEFLATE (RFC 1951) — der Leser.
//!
//! Drei Blocktypen, und alle drei muessen sitzen, weil echte PNG-Dateien alle
//! drei benutzen:
//!   * `00` gespeichert  — unkomprimiert, von `png_schreiben` benutzt, wenn
//!     die Kompression nichts bringt, und von manchen Werkzeugen.
//!   * `01` feste Huffman-Tabelle — die Kurzform.
//!   * `10` DYNAMISCHE Huffman-Tabelle — das ist der Normalfall bei Bildern.
//!     Ohne sie waere der Leser bei praktisch jeder echten PNG tot.
//!   * `11` ist reserviert und ergibt einen benannten Fehler.
//!
//! Das ganze ist ohne `unsafe` gebaut: die Bit-Engine arbeitet auf einem
//! Slice mit einem Index, und die Huffman-Tabellen sind zwei kleine Arrays
//! (`anzahlen` je Code-Laenge, `symbole` in kanonischer Reihenfolge). Das
//! ist langsamer als die Bit-Schiebetabelle von zlib, aber es kann per
//! Konstruktion nicht aus dem Speicher lesen — und das ist bei fremden
//! Bytes genau der Punkt.

use crate::FehlerZlib;

/// Entpackt einen zlib-Strom (Kopf + DEFLATE-Daten + Adler32).
///
/// `grenze` ist die HARTE Obergrenze der Ausgabe in Byte. Sie wird nicht
/// geraten, sondern vom Aufrufer aus der IHDR gerechnet. Ein zlib-Strom
/// kann beliebig viele Null-Bytes erzeugen; ohne diese Grenze wuerde
/// „1x1-Bild, das 40 GB entpackt" den Tab beenden.
pub fn entpacke(daten: &[u8], grenze: usize) -> Result<Vec<u8>, FehlerZlib> {
    // Nicht `with_capacity(grenze)`: ein 16384x16384-RGB-Bild braucht
    // 805 MiB, und der Vektor waechst auch ohne Vorabreservierung. Der
    // Startwert ist nur ein Vorschlag.
    let mut out = Vec::with_capacity(grenze.min(1 << 16));
    entpacke_in(daten, &mut out, grenze)?;
    Ok(out)
}

/// Wie [`entpacke`], schreibt aber in einen vorhandenen Puffer.
///
/// Spart dem Aufrufer die Zwischenallokation: der Bildpuffer wird genau
/// einmal angelegt und dann befuellt.
pub fn entpacke_in(daten: &[u8], out: &mut Vec<u8>, grenze: usize) -> Result<(), FehlerZlib> {
    out.clear();

    // --- zlib-Kopf (RFC 1950 Abschnitt 2.2) ---
    if daten.len() < 2 {
        return Err(FehlerZlib::DatenZuEnde { byte: daten.len() });
    }
    let cmf = daten[0];
    let flg = daten[1];
    if cmf & 0x0F != 8 {
        // CM = 8 bedeutet "DEFLATE". Alles andere gibt es nicht.
        return Err(FehlerZlib::UnbekannteMethode { cmf, flg });
    }
    if (cmf & 0xF0) >> 4 > 7 {
        // CINFO (Fenstergroesse) groesser als 32 KiB. Unzulaessig.
        return Err(FehlerZlib::UnbekannteMethode { cmf, flg });
    }
    if ((cmf as u32) << 8 | flg as u32) % 31 != 0 {
        // FCHECK: die beiden Kopfbytes sind durch 31 teilbar.
        return Err(FehlerZlib::FalschePruefsumme { cmf, flg });
    }
    if flg & 0x20 != 0 {
        // FDICT: ein Praediktionswoerterbuch waere noetig. In PNG gibt es
        // dafuer keine Verwendung — also ein benannter Fehler, kein Versuch.
        return Err(FehlerZlib::Praediktionswoerterbuch);
    }

    let mut b = Bitleser::neu(&daten[2..]);

    // Der DEFLATE-Teil endet, sobald ein Block mit BFINAL=1 fertig ist.
    // Wir lesen die vier Bytes am Ende NICHT — sie sind der Adler32, und ein
    // Block, der sie frisst, waere ein Fehler.
    loop {
        let final_block = b.leses_bit()? != 0;
        let typ = b.leses_bits(2)?;
        match typ {
            0 => block_gespeichert(&mut b, out, grenze)?,
            1 => {
                let (l, d) = huffman_fest();
                block_huffman(&mut b, &l, &d, out, grenze)?;
            }
            2 => {
                let (l, d) = huffman_dynamisch(&mut b)?;
                block_huffman(&mut b, &l, &d, out, grenze)?;
            }
            _ => return Err(FehlerZlib::UnbekannterBlocktyp { typ: typ as u8 }),
        }
        if final_block {
            break;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Bit-Leser
// ---------------------------------------------------------------------------

/// Liest DEFLATE-Bits: erstes Bit im Byte ist das NIEDRIGSTE.
///
/// Das ist die eine Stelle, an der die Spezifikation „unnormal" ist. Der
/// Grund: die Bitreihenfolge in DEFLATE ist LSB-first, damit die
/// Huffman-Codes mit ihrer haeufigsten Laenge nicht am Byte-Anfang
/// gespiegelt werden muessen.
struct Bitleser<'a> {
    daten: &'a [u8],
    /// Index des Bytes, aus dem gerade gelesen wird.
    pos: usize,
    /// Wie viele Bits dieses Bytes schon verbraucht sind (0..8).
    bit: u32,
}

impl<'a> Bitleser<'a> {
    fn neu(daten: &'a [u8]) -> Bitleser<'a> {
        Bitleser {
            daten,
            pos: 0,
            bit: 0,
        }
    }

    /// Liest ein Bit. `0` oder `1`. Fehler, wenn die Daten ausgehen.
    fn leses_bit(&mut self) -> Result<u32, FehlerZlib> {
        if self.pos >= self.daten.len() {
            return Err(FehlerZlib::DatenZuEnde { byte: self.pos });
        }
        let v = (self.daten[self.pos] >> self.bit) & 1;
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.pos += 1;
        }
        Ok(v as u32)
    }

    /// Liest `n` Bits (maximal 16) als Zahl, LSB zuerst gelesen.
    fn leses_bits(&mut self, n: u32) -> Result<u32, FehlerZlib> {
        let mut v = 0u32;
        for i in 0..n {
            v |= self.leses_bit()? << i;
        }
        Ok(v)
    }

    /// Verwirft die restlichen Bits des aktuellen Bytes.
    ///
    /// NUR fuer gespeicherte Bloecke: die Laenge steht ab dem naechsten
    /// Byte-Anfang. An anderer Stelle waere das ein Datenfehler.
    fn richtung_bytegrenze(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.pos += 1;
        }
    }

    /// Liest `n` ganze Byte am Stueck (nur nach [`Self::richtung_bytegrenze`]).
    fn leses_bytes(&mut self, n: usize) -> Result<&'a [u8], FehlerZlib> {
        if self.pos + n > self.daten.len() {
            return Err(FehlerZlib::DatenZuEnde { byte: self.pos });
        }
        let s = &self.daten[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
}

// ---------------------------------------------------------------------------
// Huffman
// ---------------------------------------------------------------------------

/// Kanonische Huffman-Tabelle: `anzahlen[l]` = wie viele Symbole die
/// Laenge `l` haben, `symbole` in aufsteigender Laenge und dann
/// aufsteigendem Symbolwert.
///
/// Diese Darstellung ist 60 Zeilen lang und braucht keinen Index, der
/// out of range sein kann — der Grund fuer `unsafe_code = "forbid"` in
/// diesem Workspace.
struct Huffman {
    anzahlen: [u16; 16],
    symbole: Vec<u16>,
}

impl Huffman {
    /// Baut die Tabelle. `links` ist die Zahl der noch nicht belegten
    ///PLAetze (puff nennt es `left`): 0 = vollstaendig, > 0 = unvollstaendig,
    /// < 0 = ueberzaehlt.
    fn baue(lengen: &[u8]) -> Result<(Huffman, i32), FehlerZlib> {
        let mut anzahlen = [0u16; 16];
        for &l in lengen {
            if l as usize > 15 {
                return Err(FehlerZlib::HuffmanFalsch {
                    grund: "Codelaenge groesser als 15",
                });
            }
            anzahlen[l as usize] += 1;
        }
        if anzahlen[0] as i32 == lengen.len() as i32 {
            // Alle Laengen null: das ist ein LEERER Baum. zlib erlaubt das
            // fuer die Distanztabelle, wenn das Bild keine Matches hat.
            return Ok((
                Huffman {
                    anzahlen,
                    symbole: Vec::new(),
                },
                1,
            ));
        }
        // Ueberzaehlt? links darf nie negativ werden.
        let mut links: i32 = 1;
        for len in 1..16 {
            links <<= 1;
            links -= anzahlen[len] as i32;
            if links < 0 {
                return Err(FehlerZlib::HuffmanFalsch {
                    grund: "Code ist ueberzaehlt (mehr Codes als Platz)",
                });
            }
        }
        // Symbole in kanonischer Reihenfolge ablegen.
        //
        // ACHTUNG, HIER LAG EIN FEHLER (gefunden vom Test `rundlauf_rgb`
        // am 30.09.2026, Panik in Zeile 226: `symbole[24] = ...` auf einer
        // `Vec` mit `with_capacity`, aber `len == 0`).
        //
        // `Vec`s IndexTraet prueft gegen `len`, NICHT gegen `capacity`.
        // Nach `with_capacity(288)` ist die Kapazitaet 288, die Laenge aber
        // 0 — und `symbole[24] = x` paniked mit "index out of bounds: the
        // len is 0 but the index is 24".
        //
        // `resize` statt `with_capacity`: es geht hier nicht um Tempo,
        // sondern um Korrektheit. Man kann auf eine leere, aber
        // ausreichend grosse `Vec` nicht ueber Index schreiben.
        let mut symbole = vec![0u16; lengen.len()];
        let mut naechste = [0u16; 16];
        for len in 1..15 {
            naechste[len + 1] = naechste[len] + anzahlen[len];
        }
        for (sym, &l) in lengen.iter().enumerate() {
            if l != 0 {
                let i = naechste[l as usize] as usize;
                // `i < symbole.len()` ist jetzt per Konstruktion sicher:
                // `naechste` zaehlt die Symbole kumulativ, und die Summe
                // aller `anzahlen[1..=15]` ist hoechstens `lengen.len()`.
                if let Some(s) = symbole.get_mut(i) {
                    *s = sym as u16;
                }
                naechste[l as usize] += 1;
            }
        }
        Ok((Huffman { anzahlen, symbole }, links))
    }

    /// Holt ein Symbol. `code` waechst bitweise; die Formel ist die
    /// kanonische Zaehlung, sie braucht keine Tabelle ueber 2^15 Eintraege.
    fn symbol(&self, b: &mut Bitleser<'_>) -> Result<u16, FehlerZlib> {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for len in 1..16 {
            code |= b.leses_bit()? as i32;
            let anzahl = self.anzahlen[len] as i32;
            if code - first < anzahl {
                let i = (index + (code - first)) as usize;
                return self
                    .symbole
                    .get(i)
                    .copied()
                    .ok_or(FehlerZlib::HuffmanFalsch {
                        grund: "Symbolindex ausserhalb der Tabelle",
                    });
            }
            index += anzahl;
            first = (first + anzahl) << 1;
            code <<= 1;
        }
        Err(FehlerZlib::HuffmanFalsch {
            grund: "Code laenger als 15 Bit",
        })
    }
}

/// Die feste Huffman-Tabelle nach RFC 1951 Abschnitt 3.2.6.
///
/// Literale/Längen: 0-143 -> 8 Bit, 144-255 -> 9 Bit, 256-279 -> 7 Bit,
/// 280-287 -> 8 Bit. Distanzen: 32 Symbole zu je 5 Bit.
fn huffman_fest() -> (Huffman, Huffman) {
    let mut ll = [0u8; 288];
    for (i, v) in ll.iter_mut().enumerate() {
        *v = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    let mut dist = [0u8; 32];
    dist[..30].fill(5); // 30 und 31 sind nicht belegt
    let l = Huffman::baue(&ll)
        .expect("feste Tabelle ist per Konstruktion vollstaendig")
        .0;
    let d = Huffman::baue(&dist)
        .expect("feste Distanztabelle ist vollstaendig")
        .0;
    (l, d)
}

/// Laengen- und Distanztabellen (RFC 1951 Abschnitt 3.2.5).
///
/// `pub`, weil der Enkoder genau dieselben Zahlen braucht: zwei Kopien
/// dieser Tabelle wuerden irgendwann auseinanderlaufen, und dann wuerde der
/// Enkoder Dateien schreiben, die der eigene Leser nicht lesen kann.
pub const LAENGEN_BASIS: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
pub const LAENGEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
pub const DISTANZ_BASIS: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
pub const DISTANZ_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Die Reihenfolge, in der die 19 Code-Laengen selbst kodiert sind
/// (RFC 1951 Abschnitt 3.2.7). Steht so in der Spezifikation, nicht in einer
/// schoeneren Reihenfolge — wer sie "sortiert", bekommt kaputte Bilder.
const CODE_LAENGEN_ORDNUNG: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

// ---------------------------------------------------------------------------
// Bloecke
// ---------------------------------------------------------------------------

fn block_gespeichert(
    b: &mut Bitleser<'_>,
    out: &mut Vec<u8>,
    grenze: usize,
) -> Result<(), FehlerZlib> {
    b.richtung_bytegrenze();
    let kopf = b.leses_bytes(4)?;
    let len = u16::from_le_bytes([kopf[0], kopf[1]]);
    let nlen = u16::from_le_bytes([kopf[2], kopf[3]]);
    if len != !nlen {
        return Err(FehlerZlib::GespeichertFalsch { len, nlen });
    }
    if out.len() + len as usize > grenze {
        return Err(FehlerZlib::ZuGross { grenze });
    }
    let stueck = b.leses_bytes(len as usize)?;
    out.extend_from_slice(stueck);
    Ok(())
}

/// Liest einen Huffman-kodierten Block (Typ 1 oder 2) und fuehrt ihn aus.
fn block_huffman(
    b: &mut Bitleser<'_>,
    l: &Huffman,
    d: &Huffman,
    out: &mut Vec<u8>,
    grenze: usize,
) -> Result<(), FehlerZlib> {
    loop {
        let sym = l.symbol(b)?;
        if sym < 256 {
            if out.len() + 1 > grenze {
                return Err(FehlerZlib::ZuGross { grenze });
            }
            out.push(sym as u8);
        } else if sym == 256 {
            return Ok(());
        } else {
            // 257..285: Laenge/Distanz-Paar.
            let idx = sym as usize - 257;
            if idx >= LAENGEN_BASIS.len() {
                return Err(FehlerZlib::SymbolNichtBelegt { wert: sym });
            }
            let laenge =
                LAENGEN_BASIS[idx] as usize + b.leses_bits(LAENGEN_EXTRA[idx] as u32)? as usize;

            let dsym = d.symbol(b)? as usize;
            if dsym >= DISTANZ_BASIS.len() {
                // 30 und 31 sind nicht belegt.
                return Err(FehlerZlib::DistanzFalsch { wert: dsym as u16 });
            }
            let distanz =
                DISTANZ_BASIS[dsym] as usize + b.leses_bits(DISTANZ_EXTRA[dsym] as u32)? as usize;
            if distanz == 0 || distanz > 32768 {
                return Err(FehlerZlib::DistanzFalsch {
                    wert: distanz as u16,
                });
            }
            if distanz > out.len() {
                // Ein Match darf nicht in Daten zeigen, die es noch nicht
                // gibt. Genau hier entstehen sonst "falsche" Bilder aus
                // kaputten Dateien.
                return Err(FehlerZlib::MatchUeberEnde {
                    laenge: laenge as u16,
                    position: out.len(),
                });
            }
            if out.len() + laenge > grenze {
                return Err(FehlerZlib::ZuGross { grenze });
            }
            let start = out.len() - distanz;
            for k in 0..laenge {
                // Ueberschlagend kopiert wird absichtlich NICHT ueber
                // `extend_from_slice`, weil sich Laenge und Distanz
                // ueberlappen duerfen ("aaaa..." aus distanz=1).
                let byte = out[start + k];
                out.push(byte);
            }
        }
    }
}

/// Baut die beiden Tabellen eines dynamischen Blocks.
fn huffman_dynamisch(b: &mut Bitleser<'_>) -> Result<(Huffman, Huffman), FehlerZlib> {
    let hlit = b.leses_bits(5)? as usize + 257;
    let hdist = b.leses_bits(5)? as usize + 1;
    let hclen = b.leses_bits(4)? as usize + 4;

    if hlit > 286 || hdist > 30 {
        return Err(FehlerZlib::HuffmanFalsch {
            grund: "HLIT/HDIST ausserhalb der zulaessigen Werte",
        });
    }

    // Die 19 Code-Laengen-Längen stehen in der Reihenfolge der Spezifikation.
    let mut code_langen = [0u8; 19];
    for i in 0..hclen {
        code_langen[CODE_LAENGEN_ORDNUNG[i]] = b.leses_bits(3)? as u8;
    }
    let (code_tree, links) = Huffman::baue(&code_langen)?;
    if links != 0 {
        // Fuer die Code-Laengen selbst verlangt RFC 1951 einen vollstaendigen
        // Code. Ein halber waere nicht eindeutig entpackbar.
        return Err(FehlerZlib::HuffmanFalsch {
            grund: "Code-Laengen-Code ist unvollstaendig",
        });
    }

    // Die tatsaechlichen Laengen mit dem RLE 16/17/18 zusammenbauen.
    let gesamt = hlit + hdist;
    let mut laengen = vec![0u8; gesamt];
    let mut i = 0usize;
    while i < gesamt {
        let sym = code_tree.symbol(b)?;
        match sym {
            0..=15 => {
                laengen[i] = sym as u8;
                i += 1;
            }
            16 => {
                if i == 0 {
                    return Err(FehlerZlib::HuffmanFalsch {
                        grund: "RLE 16 ohne vorherige Laenge",
                    });
                }
                let vor = laengen[i - 1];
                let n = 3 + b.leses_bits(2)? as usize;
                if i + n > gesamt {
                    return Err(FehlerZlib::HuffmanFalsch {
                        grund: "RLE 16 laeuft ueber die Laengenliste",
                    });
                }
                for _ in 0..n {
                    laengen[i] = vor;
                    i += 1;
                }
            }
            17 => {
                let n = 3 + b.leses_bits(3)? as usize;
                if i + n > gesamt {
                    return Err(FehlerZlib::HuffmanFalsch {
                        grund: "RLE 17 laeuft ueber die Laengenliste",
                    });
                }
                i += n; // Nullen stehen schon drin
            }
            18 => {
                let n = 11 + b.leses_bits(7)? as usize;
                if i + n > gesamt {
                    return Err(FehlerZlib::HuffmanFalsch {
                        grund: "RLE 18 laeuft ueber die Laengenliste",
                    });
                }
                i += n; // Nullen stehen schon drin
            }
            _ => {
                return Err(FehlerZlib::SymbolNichtBelegt { wert: sym });
            }
        }
    }

    // Literale/Laengen. Unvollstaendig ist nur erlaubt, wenn es genau EIN
    // Symbol mit Laenge 1 gibt (dann ist der "Code" ein einzelnes Bit).
    let (l, links) = Huffman::baue(&laengen[..hlit])?;
    if links > 0 && laengen[..hlit].iter().filter(|&&x| x != 0).count() != 1 {
        return Err(FehlerZlib::HuffmanFalsch {
            grund: "Literal-/Laengen-Code unvollstaendig",
        });
    }

    // Distanzen. Hier ist ein LEERER Baum erlaubt: zlib schreibt so, wenn
    // das Bild keine Matches enthaelt (nur Farbverlaeufe). puff wuerde das
    // ablehnen — wir nehmen die Datei trotzdem, und ein Match laeuft dann in
    // den benannten Fehler `DistanzFalsch`.
    let (d, _links_d) = Huffman::baue(&laengen[hlit..])?;

    Ok((l, d))
}
