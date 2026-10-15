// W5F, 30.09.2026: der `use` stand hier am Dateikopf, wurde aber nur von
// `mod tests` gebraucht — der Code selbst schreibt `crate::FehlerZlib`
// ueberall ausgeschrieben. Also gehoert der Import in die Tests.
//! Kanonische Huffman-Tabelle (RFC 1951).

/// Kanonische Huffman-Tabelle: `anzahlen[l]` = wie viele Symbole die
/// Laenge `l` haben, `symbole` in aufsteigender Laenge und dann
/// aufsteigendem Symbolwert.
pub struct Huffman {
    anzahlen: [u16; 16],
    symbole: Vec<u16>,
}

impl Huffman {
    /// Baut die Tabelle. `links` ist die Zahl der noch nicht belegten
    /// Plaetze: 0 = vollstaendig, > 0 = unvollstaendig, < 0 = ueberzaehlt.
    pub fn baue(lengen: &[u8]) -> Result<(Huffman, i32), crate::FehlerZlib> {
        let mut anzahlen = [0u16; 16];
        for &l in lengen {
            if l as usize > 15 {
                return Err(crate::FehlerZlib::HuffmanFalsch {
                    grund: "Codelaenge groesser als 15".to_string(),
                });
            }
            anzahlen[l as usize] += 1;
        }
        if anzahlen[0] as i32 == lengen.len() as i32 {
            // Alle Laengen null: leerer Baum (erlaubt fuer Distanzen).
            return Ok((
                Huffman {
                    anzahlen,
                    symbole: Vec::new(),
                },
                1,
            ));
        }
        let mut links: i32 = 1;
        // added 30.09.2026: `for len in 1..16` wurde zu
        // `for &anzahl in &anzahlen[1..]` — clippy hat zu Recht
        // gemeldet, dass `len` nur als Index dient ("needless_range_
        // loop"). Die Kurzform ist zusaetzlich **korrekter**: sie kann
        // nicht aus dem Takt geraten, wenn `anzahlen` einmal 17 Felder
        // haette, denn `&anzahlen[1..]` nimmt alle Felder ausser dem
        // ersten. Die Rechnung ist unveraendert: `links` verdoppelt
        // sich je Code-Laenge und zieht die Anzahl ab.
        for &anzahl in &anzahlen[1..] {
            links <<= 1;
            links -= anzahl as i32;
            if links < 0 {
                return Err(crate::FehlerZlib::HuffmanFalsch {
                    grund: "Code ist ueberzaehlt (mehr Codes als Platz)".to_string(),
                });
            }
        }
        // Symbole in kanonischer Reihenfolge ablegen.
        let mut symbole = vec![0u16; lengen.len()];
        let mut naechste = [0u16; 16];
        for len in 1..15 {
            naechste[len + 1] = naechste[len] + anzahlen[len];
        }
        for (sym, &l) in lengen.iter().enumerate() {
            if l != 0 {
                let i = naechste[l as usize] as usize;
                if let Some(s) = symbole.get_mut(i) {
                    *s = sym as u16;
                }
                naechste[l as usize] += 1;
            }
        }
        Ok((Huffman { anzahlen, symbole }, links))
    }

    /// Holt ein Symbol. `code` waechst bitweise; die Formel ist die
    /// kanonische Zaehlung.
    pub fn symbol(
        &self,
        b: &mut crate::inflate::bitleser::Bitleser<'_>,
    ) -> Result<u16, crate::FehlerZlib> {
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
                    .ok_or(crate::FehlerZlib::HuffmanFalsch {
                        grund: "Symbolindex ausserhalb der Tabelle".to_string(),
                    });
            }
            index += anzahl;
            first = (first + anzahl) << 1;
            code <<= 1;
        }
        Err(crate::FehlerZlib::HuffmanFalsch {
            grund: "Code laenger als 15 Bit".to_string(),
        })
    }
}

/// Die feste Huffman-Tabelle nach RFC 1951 Abschnitt 3.2.6.
pub fn huffman_fest() -> (Huffman, Huffman) {
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
    dist[..30].fill(5);
    let l = Huffman::baue(&ll)
        .expect("feste Tabelle ist per Konstruktion vollstaendig")
        .0;
    let d = Huffman::baue(&dist)
        .expect("feste Distanztabelle ist vollstaendig")
        .0;
    (l, d)
}

/// Laengen- und Distanztabellen (RFC 1951 Abschnitt 3.2.5).
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

/// Reihenfolge der 19 Code-Laengen-Längen (RFC 1951 Abschnitt 3.2.7).
const CODE_LAENGEN_ORDNUNG: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// Baut die beiden Tabellen eines dynamischen Blocks.
// W5F, 30.09.2026: `mut b` war `mut` und wird nur gelesen (alle
// Zugriffe sind `b.leses_*(&mut self)`). Das `mut` an der BINDUNG war
// damit ueberfluessig; der Leser selbst bleibt veraenderlich, weil die
// Lesebefehle den Puffer fuellen.
pub fn huffman_dynamisch(
    b: &mut crate::inflate::bitleser::Bitleser<'_>,
) -> Result<(Huffman, Huffman), crate::FehlerZlib> {
    let hlit = b.leses_bits(5)? as usize + 257;
    let hdist = b.leses_bits(5)? as usize + 1;
    let hclen = b.leses_bits(4)? as usize + 4;

    if hlit > 286 || hdist > 30 {
        return Err(crate::FehlerZlib::HuffmanFalsch {
            grund: "HLIT/HDIST ausserhalb der zulaessigen Werte".to_string(),
        });
    }

    let mut code_langen = [0u8; 19];
    for i in 0..hclen {
        code_langen[CODE_LAENGEN_ORDNUNG[i]] = b.leses_bits(3)? as u8;
    }
    let (code_tree, links) = Huffman::baue(&code_langen)?;
    if links != 0 {
        return Err(crate::FehlerZlib::HuffmanFalsch {
            grund: "Code-Laengen-Code ist unvollstaendig".to_string(),
        });
    }

    let gesamt = hlit + hdist;
    let mut laengen = vec![0u8; gesamt];
    let mut i = 0usize;
    while i < gesamt {
        let sym = code_tree.symbol(&mut *b)?;
        match sym {
            0..=15 => {
                laengen[i] = sym as u8;
                i += 1;
            }
            16 => {
                if i == 0 {
                    return Err(crate::FehlerZlib::HuffmanFalsch {
                        grund: "RLE 16 ohne vorherige Laenge".to_string(),
                    });
                }
                let vor = laengen[i - 1];
                let n = 3 + b.leses_bits(2)? as usize;
                if i + n > gesamt {
                    return Err(crate::FehlerZlib::HuffmanFalsch {
                        grund: "RLE 16 laeuft ueber die Laengenliste".to_string(),
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
                    return Err(crate::FehlerZlib::HuffmanFalsch {
                        grund: "RLE 17 laeuft ueber die Laengenliste".to_string(),
                    });
                }
                i += n;
            }
            18 => {
                let n = 11 + b.leses_bits(7)? as usize;
                if i + n > gesamt {
                    return Err(crate::FehlerZlib::HuffmanFalsch {
                        grund: "RLE 18 laeuft ueber die Laengenliste".to_string(),
                    });
                }
                i += n;
            }
            _ => {
                return Err(crate::FehlerZlib::SymbolNichtBelegt { wert: sym });
            }
        }
    }

    let (l, links) = Huffman::baue(&laengen[..hlit])?;
    if links > 0 && laengen[..hlit].iter().filter(|&&x| x != 0).count() != 1 {
        return Err(crate::FehlerZlib::HuffmanFalsch {
            grund: "Literal-/Laengen-Code unvollstaendig".to_string(),
        });
    }
    let (d, _links_d) = Huffman::baue(&laengen[hlit..])?;
    Ok((l, d))
}
