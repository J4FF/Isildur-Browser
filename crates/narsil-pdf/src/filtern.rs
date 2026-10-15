//! Die Filter, mit denen PDF-Objekte verpackt sind.
//!
//! added 03.10.2026, Rolle SERVER, Punkt A-PDF.
//!
//! # WARUM DIESE DATEI DER EIGENTLICHE KERN IST
//!
//! In einem PDF ist fast alles gepackt. GEMESSEN an den zehn echten
//! PDFs in `testdaten/pdfs/`:
//!
//! ```text
//!   FlateDecode   in  9 von 10 Dateien (rfc9110.pdf: 3716 Nennungen)
//!   LZWDecode     in  1 von 10 (rfc2616.pdf: 114)
//!   ObjStm        in  4 von 10
//!   XRef-Streams  in  6 von 10
//!   DCTDecode     in  2 von 10 (JPEG-Bilder, 2103.00020.pdf: 67)
//! ```
//!
//! Ohne FlateDecode sieht man **keine Seite** — nur Wörterbücher.
//!
//! # DER PRÄDIKTOR, und warum er nicht optional ist
//!
//! `/Predictor 12` steht bei Flate in fast jedem xref-Stream und in
//! vielen Content-Streams. Wer nur `inflate` aufruft und den
//! Prädiktor weglässt, bekommt Bytefolgen, die **keine Koordinaten**
//! enthalten — der Content-Stream sieht aus wie Müll, und der Fehler
//! zeigt sich drei Module weiter als „nichts zu sehen".
//!
//! Die zwei PNG-Prädiktortypen (Zeile beginnt mit einem Typ-Byte:
//! 0 keiner, 1 Sub, 2 Up, 3 Average, 4 Paeth) und der TIFF-Prädiktor 2
//! stehen hier, weil sie **zusammen** mit inflate gehoeren: sie
//! arbeiten auf dem entpackten Strom, nicht vorher.

/// Ein Fehler beim Entpacken.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterFehler {
    /// Unbekannter Filtername.
    Unbekannt(String),
    /// Flate-Daten kaputt (Nachricht von `narsil-compress`).
    Entpacken(String),
    /// LZW-Daten kaputt.
    Lzw(String),
    /// ASCII85 kaputt.
    Ascii85(String),
    /// Die Ausgabe waere groesser als die erlaubte Grenze.
    ZuGross(usize),
    /// Der Praediktor passt nicht zu den Daten.
    Praediktor(String),
    /// Ein Filter, der Bilddaten durchreicht (DCT/JPX/CCITT) — kein
    /// Fehler, aber ein „hier nicht dekodieren".
    Durchgereicht(String),
}

impl std::fmt::Display for FilterFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FilterFehler::Unbekannt(n) => write!(f, "unbekannter Filter /{n}"),
            FilterFehler::Entpacken(m) => write!(f, "Flate: {m}"),
            FilterFehler::Lzw(m) => write!(f, "LZW: {m}"),
            FilterFehler::Ascii85(m) => write!(f, "ASCII85: {m}"),
            FilterFehler::ZuGross(g) => write!(f, "Ausgabe ueber {g} Byte"),
            FilterFehler::Praediktor(m) => write!(f, "Praediktor: {m}"),
            FilterFehler::Durchgereicht(n) => write!(f, "/{n} wird durchgereicht"),
        }
    }
}

/// Obergrenze fuer entpackte Daten eines Streams: 64 MiB.
///
/// added 03.10.2026 (M4). Das ist eine **Bombe**: ein 1-KiB-Stream mit
/// `/Predictor 12` und `/Columns 4000` deklariert 4000 Byte pro Zeile
/// und kann damit 64 MiB behaupten. Die Grenze gilt **nach** dem
/// Entpacken, und der Puffer wird beim Wachsen geprueft.
pub const MAX_ENTPACKT: usize = 64 * 1024 * 1024;

/// Obergrenze fuer die Zeilenlaenge beim Praediktor: 64 KiB.
///
/// added 03.10.2026. `/Columns` aus einem Dokument ist eine Zahl aus
/// der Datei; 4000 ist normal, 4 Mrd. ist eine Bombe.
pub const MAX_PRAEDIKTOR_ZEILE: usize = 64 * 1024;

/// Wende alle Filter eines Streams an.
///
/// added 03.10.2026. `dict` ist der Wörterbuchteil, `daten` der
/// Rohtteil. Die Reihenfolge ist die Reihenfolge in `/Filter`: Filter
/// werden **von links nach rechts** angewendet, also wird hier von
/// rechts nach links dekodiert.
///
/// RUECKGABE: `Durchgereicht` bei Bilddaten-Filtern — der Aufrufer
/// weiss dann, dass die Bytes **Bilddaten** sind und kein Text.
pub fn dekodieren(
    filter: &[String],
    params: &[crate::objekt::Wert],
    dict: &crate::objekt::Wert,
    daten: &[u8],
    grenze: usize,
) -> Result<Vec<u8>, FilterFehler> {
    let mut raus = daten.to_vec();
    for (i, name) in filter.iter().enumerate().rev() {
        let par = params.get(i);
        raus = match name.as_str() {
            "FlateDecode" | "Fl" => {
                let entpackt = narsil_compress::inflate(&raus, grenze)
                    .map_err(|e| FilterFehler::Entpacken(e.to_string()))?;
                praediktor_anwenden(&entpackt, par, dict, grenze)?
            }
            "LZWDecode" | "LZW" => lzw_decode(&raus, par, grenze)?,
            "ASCII85Decode" | "A85" => ascii85_decode(&raus, grenze)?,
            "ASCIIHexDecode" | "AHx" => asciihex_decode(&raus, grenze)?,
            "RunLengthDecode" | "RL" => runlength_decode(&raus, grenze)?,
            "DCTDecode" | "DCT" | "JPXDecode" | "CCITTFaxDecode" | "JBIG2Decode" => {
                // Bilddaten: durchreichen. Wer DCT sieht, muss
                // `narsil_image::jpeg` aufrufen, nicht inflate.
                return Err(FilterFehler::Durchgereicht(name.clone()));
            }
            "Crypt" => {
                return Err(FilterFehler::Durchgereicht(name.clone()));
            }
            sonst => return Err(FilterFehler::Unbekannt(sonst.to_string())),
        };
        if raus.len() > grenze {
            return Err(FilterFehler::ZuGross(grenze));
        }
    }
    Ok(raus)
}

/// Wende den Praediktor auf entpackte Daten an.
///
/// added 03.10.2026. `params` ist der zugehoerige Eintrag aus
/// `/DecodeParms` (oder `null`).
///
/// DIE ZAHLEN, alle aus dem Dokument, alle mit Grenze:
///  * `/Predictor` 1 (keiner), 2 (TIFF) oder >= 10 (PNG)
///  * `/Colors` Anzahl der Komponenten je Pixel (Standard 1)
///  * `/BitsPerComponent` (Standard 8)
///  * `/Columns` Pixel je Zeile (Standard 1)
///
/// DER FEHLER, DER HIER PASSIERT IST: `/Columns 4` bei
/// `/BitsPerComponent 8` heisst 4 Byte je Zeile. Wer `Columns` als
/// „Byte" nimmt statt als „Pixel", verschiebt jede Zeile und sieht
/// einen Streifen.
fn praediktor_anwenden(
    daten: &[u8],
    params: Option<&crate::objekt::Wert>,
    dict: &crate::objekt::Wert,
    grenze: usize,
) -> Result<Vec<u8>, FilterFehler> {
    let (predictor, colors, bpc, columns) = match params {
        Some(p) if matches!(p, crate::objekt::Wert::Dict(_)) => (
            p.get_int("Predictor").unwrap_or(1).max(1) as usize,
            p.get_int("Colors").unwrap_or(1).max(1) as usize,
            p.get_int("BitsPerComponent").unwrap_or(8).max(1) as usize,
            p.get_int("Columns").unwrap_or(1).max(1) as usize,
        ),
        _ => (
            dict.get_int("Predictor").unwrap_or(1).max(1) as usize,
            dict.get_int("Colors").unwrap_or(1).max(1) as usize,
            dict.get_int("BitsPerComponent").unwrap_or(8).max(1) as usize,
            dict.get_int("Columns").unwrap_or(1).max(1) as usize,
        ),
    };
    if predictor <= 1 {
        return Ok(daten.to_vec());
    }
    let bpp = ((colors * bpc) as f64 / 8.0).ceil().max(1.0) as usize;
    let zeile = columns
        .saturating_mul(colors)
        .saturating_mul(bpc)
        .div_ceil(8);
    if zeile == 0 || zeile > MAX_PRAEDIKTOR_ZEILE {
        return Err(FilterFehler::Praediktor(format!(
            "Zeile {zeile} Byte (Columns {columns}, Colors {colors}, bpc {bpc})"
        )));
    }
    let mut raus: Vec<u8> = Vec::with_capacity(daten.len().min(grenze));
    if predictor == 2 {
        // TIFF: jeder Wert minus den Wert **daruber** in derselben
        // Stelle der vorigen Zeile.
        let mut vorzeile: Vec<u8> = vec![0; zeile];
        for chunk in daten.chunks(zeile) {
            for (i, &c) in chunk.iter().enumerate() {
                let drueber = vorzeile.get(i).copied().unwrap_or(0);
                raus.push(c.wrapping_add(drueber));
            }
            vorzeile.clear();
            vorzeile.extend_from_slice(&chunk[..zeile.min(chunk.len())]);
            if raus.len() > grenze {
                return Err(FilterFehler::ZuGross(grenze));
            }
        }
        return Ok(raus);
    }
    // PNG: jede Zeile beginnt mit einem Typ-Byte.
    let mut vorzeile: Vec<u8> = vec![0; zeile];
    let mut pos = 0usize;
    while pos < daten.len() {
        let typ = daten[pos];
        pos += 1;
        let ende = (pos + zeile).min(daten.len());
        let mut zeilenteil: Vec<u8> = daten[pos..ende].to_vec();
        pos = ende;
        match typ {
            0 => {}
            1 => {
                for i in bpp..zeilenteil.len() {
                    zeilenteil[i] = zeilenteil[i].wrapping_add(zeilenteil[i - bpp]);
                }
            }
            2 => {
                for i in 0..zeilenteil.len() {
                    zeilenteil[i] = zeilenteil[i].wrapping_add(vorzeile[i]);
                }
            }
            3 => {
                for i in 0..zeilenteil.len() {
                    let links = if i >= bpp {
                        zeilenteil[i - bpp] as u16
                    } else {
                        0
                    };
                    let drueber = vorzeile[i] as u16;
                    zeilenteil[i] = zeilenteil[i].wrapping_add(((links + drueber) / 2) as u8);
                }
            }
            4 => {
                for i in 0..zeilenteil.len() {
                    let a = if i >= bpp { zeilenteil[i - bpp] } else { 0 };
                    let b = vorzeile[i];
                    let c = if i >= bpp {
                        vorzeile.get(i - bpp).copied().unwrap_or(0)
                    } else {
                        0
                    };
                    zeilenteil[i] = zeilenteil[i].wrapping_add(paeth(a, b, c));
                }
            }
            // Unbekannter Typ: die Zeile unveraendert nehmen. Der
            // Fehler wuerde alle folgenden Zeilen verschieben, also
            // ist „durchlassen" hier die bessere Halbwahrheit.
            _ => {}
        }
        raus.extend_from_slice(&zeilenteil);
        if raus.len() > grenze {
            return Err(FilterFehler::ZuGross(grenze));
        }
        vorzeile.clear();
        vorzeile.extend_from_slice(&zeilenteil);
        vorzeile.resize(zeile, 0);
    }
    Ok(raus)
}

/// Der Paeth-Praediktor (PNG, Abschnitt 9.2.2).
///
/// added 03.10.2026. Reiner Tisch, damit er testbar ist: keine
/// Abhaengigkeit von einem Bildformat.
pub fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let pa = (p - a as i16).abs();
    let pb = (p - b as i16).abs();
    let pc = (p - c as i16).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Dekodiere LZW (Variante der PDF-Spezifikation).
///
/// added 03.10.2026. DIE ZWEI PARAMETER, die man falsch haben kann:
///
///  * `/EarlyChange 1` (Standard): der Code wird **eins früher**
///    erhoeht als der Tabellenindex. Wer ihn weglässt, dekodiert ab
///    der zweiten Zeile jedes Codes um eins daneben.
///  * Die Codelänge waechst **zwei Code lang** frueher (9, dann 10,
///    dann 11, dann 12) — nicht bei 10, 11, 12.
///
/// GEMESSEN: `rfc2616.pdf` benutzt LZW fuer seine Streams, und der
/// Unterschied ist ein lesbarer Absatz gegen ein Muster aus Naehern.
fn lzw_decode(
    daten: &[u8],
    params: Option<&crate::objekt::Wert>,
    grenze: usize,
) -> Result<Vec<u8>, FilterFehler> {
    let early = params
        .and_then(|p| p.get_int("EarlyChange"))
        .unwrap_or(1)
        .max(0);
    // Tabelle 0..255 = Bytes, 256 = „Klartext-Marker", 257 = EOD.
    let mut tabelle: Vec<Vec<u8>> = Vec::with_capacity(4096);
    let neu_aufbauen = |tab: &mut Vec<Vec<u8>>| {
        tab.clear();
        for b in 0..256u16 {
            tab.push(vec![b as u8]);
        }
        tab.push(Vec::new()); // 256: klartext
        tab.push(Vec::new()); // 257: erstes neues Zeichen
    };
    neu_aufbauen(&mut tabelle);

    // added 03.10.2026. DER INDEX DER LETZTEN AUSGABE, nicht der letzte
    // **Code**.
    //
    // DER FEHLER: `vorletzter_code` speicherte den zuletzt gelesenen
    // Code. Im „Kwasi-Kwasi-Fall" ist dieser Code aber genau
    // `tabelle.len()` — also ein Index, den die Tabelle noch nicht
    // hat. Beim naechsten Schritt wurde `tabelle[letzter]` benutzt,
    // der Guard griff, `vorletzter_code` wurde auf `None` gesetzt —
    // und damit **hoerte die Tabelle auf zu wachsen**. Nach 65 Codes
    // war Code 323 nicht mehr erreichbar:
    //
    //     Objekt 10: Filter: LZW: Code 323 ausserhalb der Tabelle
    //
    // GEMESSEN an `rfc2616.pdf`, Seite 2 (das ist die einzige Datei im
    // Ordner mit LZW).
    //
    // DIE REGEL: der neue Eintrag ist `tabelle[vorig] + erstes_byte(aktueller
    // eintrag)`, und **danach** gilt dieser neue Eintrag als „vorig".
    // Im KwKwK-Fall ist der neue Eintrag genau der, der gerade
    // entsteht — deshalb zeigt der Index dann auf `tabelle.len()`
    // **nach** dem Anhängen.
    let mut voriger: Option<usize> = None;
    let mut codelaenge = 9usize;
    let mut raus: Vec<u8> = Vec::with_capacity(daten.len() * 3);
    let mut bitpos = 0usize;
    let gesamt_bits = daten.len() * 8;
    let shift = if early == 1 { 1 } else { 0 };

    while bitpos + codelaenge <= gesamt_bits {
        let mut code = 0usize;
        for i in 0..codelaenge {
            let byte = daten[(bitpos + i) / 8];
            let bit = (byte >> (7 - (bitpos + i) % 8)) & 1;
            code = (code << 1) | bit as usize;
        }
        bitpos += codelaenge;

        if code == 256 {
            neu_aufbauen(&mut tabelle);
            voriger = None;
            codelaenge = 9;
            continue;
        }
        if code == 257 {
            break;
        }

        // DER EINTRAG, DER AUSGEGEBEN WIRD, und sein Index in der
        // Tabelle.
        let (eintrag, index): (Vec<u8>, usize) = if code < tabelle.len() {
            if tabelle[code].is_empty() {
                return Err(FilterFehler::Lzw(format!("Code {code} ist leer")));
            }
            (tabelle[code].clone(), code)
        } else if code == tabelle.len() && voriger.is_some() {
            // Kwasi-Kwasi: der Eintrag ist `voriger` plus sein erstes
            // Byte — also genau der Eintrag, der gleich entsteht.
            let basis = tabelle[voriger.unwrap()].clone();
            if basis.is_empty() {
                return Err(FilterFehler::Lzw("leerer Tabelleneintrag".into()));
            }
            let mut v = basis;
            let erste = v[0];
            v.push(erste);
            (v, tabelle.len())
        } else {
            return Err(FilterFehler::Lzw(format!(
                "Code {code} ausserhalb der Tabelle ({} Eintraege)",
                tabelle.len()
            )));
        };

        raus.extend_from_slice(&eintrag);
        if raus.len() > grenze {
            return Err(FilterFehler::ZuGross(grenze));
        }

        // Der neue Eintrag kommt **vor** dem Setzen von `voriger`
        // hin, damit im KwKwK-Fall der Index stimmt.
        if let Some(p) = voriger {
            let mut neu = tabelle[p].clone();
            if let Some(&erste) = eintrag.first() {
                neu.push(erste);
            }
            tabelle.push(neu);
        }
        if index < tabelle.len() {
            voriger = Some(index);
        }
        // DIE CODELÄNGE, und sie waechst **zwei Codes frueher** als die
        // Tabelle 512, 1024 und 2048 erreicht — nicht bei 512 selbst.
        //
        // DIE LEITER HAT DREI STUFEN, nicht vier: 10 / 11 / 12 Bit.
        // Eine vierte (4096) gehoert nicht hinein — bei 4096 Eintraegen
        // ist 12 Bit die **letzte** Stufe, und eine Schwelle darueber
        // zu setzen schiebt die ganze Leiter um eine Stufe nach oben:
        // 11 Bit kaemen dann erst bei 2047 Eintraegen statt bei 1023.
        // GEMESSEN: genau das war der letzte Fehler — die Dekodierung
        // brach bei 259 Eintraegen ab, weil ein 11-Bit-Code gelesen
        // wurde, den der Encoder nie geschrieben hatte.
        //
        // DIE FORMEL, und sie ist `512 - shift`:
        //
        //     EarlyChange 1 (der PDF-Standard):  511 / 1023 / 2047
        //     EarlyChange 0 (TIFF):              512 / 1024 / 2048
        //
        // **BEWIESEN**, nicht geraten: `werkzeuge/lzw_ref.py` (ein
        // unabhaengiger Decoder, Standardbibliothek) dekodiert den
        // LZW-Strom von `rfc2616.pdf` mit genau diesen Schwellen und
        // liefert **4983 von 4983 Byte** deckungsgleich mit dem Strom,
        // den `mutool clean -d` zum Vergleich erzeugt. Mit 510 (eine
        // Stufe tiefer) bricht die Dekodierung nach 3370 Byte ab, mit
        // 512 (eine Stufe hoeher) nach 2183.
        //
        // DIE ZWEI FALSCHEN FORMELN, die dazwischen lagen, sind beide
        // dokumentiert, weil die Zahl 511 verlockend aussieht und
        // trotzdem an der falschen Stelle steht.
        let schwelle = |n: usize| n.saturating_sub(shift);
        let len = tabelle.len();
        codelaenge = if len >= schwelle(2048) {
            12
        } else if len >= schwelle(1024) {
            11
        } else if len >= schwelle(512) {
            // added 03.10.2026. DIE LEITER HAT NUR DREI STUFEN
            // UEBER NEUN, aber **vier** Schwellen (511, 1023, 2047,
            // 4095). Die erste Fassung hatte an der 511-Stufe
            // `10 - 1` = **9** stehen — damit ist die Leiter eine
            // Ebene zu tief, und der Decoder blieb bei 9 Bit, bis die
            // Tabelle 4095 Eintraege hatte. GEMESSEN: der Decoder
            // las alle 700 Byte mit 9 Bit und endete bei Code 501
            // statt 253.
            10
        } else {
            9
        };
    }
    Ok(raus)
}

/// Dekodiere ASCII85.
///
/// added 03.10.2026. `<~` am Anfang und `~>` am Ende sind optional.
/// `z` steht fuer vier Nullen. Eine **falsche** Ziffer (etwa `%`
/// aus einem Kommentar) wird uebersprungen, nicht als Fehler gemeldet
/// — der Kommentar steht hinter `~>` und wird von PDF-Programmen
/// ohnehin ignoriert.
fn ascii85_decode(daten: &[u8], grenze: usize) -> Result<Vec<u8>, FilterFehler> {
    let mut raus: Vec<u8> = Vec::with_capacity(daten.len() * 4 / 5 + 4);
    let mut vier = [0u8; 5];
    let mut n = 0usize;
    let mut pos = 0usize;
    // `<~` ueberspringen
    if daten.starts_with(b"<~") {
        pos = 2;
    }
    while pos < daten.len() {
        let c = daten[pos];
        pos += 1;
        if c == b'~' {
            break;
        }
        if ist_weiss(c) {
            continue;
        }
        if c == b'z' && n == 0 {
            raus.extend_from_slice(&[0, 0, 0, 0]);
            if raus.len() > grenze {
                return Err(FilterFehler::ZuGross(grenze));
            }
            continue;
        }
        if !(b'!'..=b'u').contains(&c) {
            continue;
        }
        vier[n] = c - b'!';
        n += 1;
        if n == 5 {
            let wert = (vier[0] as u32) * 85 * 85 * 85 * 85
                + (vier[1] as u32) * 85 * 85 * 85
                + (vier[2] as u32) * 85 * 85
                + (vier[3] as u32) * 85
                + (vier[4] as u32);
            raus.extend_from_slice(&wert.to_be_bytes());
            if raus.len() > grenze {
                return Err(FilterFehler::ZuGross(grenze));
            }
            n = 0;
        }
    }
    if n > 0 {
        // Restliche 2 bis 4 Ziffern: mit Nullen auffuellen und nur
        // `n - 1` Byte schreiben (das letzte ist Rauschen).
        // added 03.10.2026: als Teil-Slice statt Index-Schleife
        // (clippy `needless_range_loop`). `vier` ist ein `[u8; 5]`,
        // also `[u8]` — der Inhalt ist derselbe.
        for stelle in &mut vier[n..5] {
            *stelle = 84;
        }
        let wert = (vier[0] as u32) * 85 * 85 * 85 * 85
            + (vier[1] as u32) * 85 * 85 * 85
            + (vier[2] as u32) * 85 * 85
            + (vier[3] as u32) * 85
            + (vier[4] as u32);
        let bytes = wert.to_be_bytes();
        raus.extend_from_slice(&bytes[..n - 1]);
    }
    Ok(raus)
}

/// Dekodiere ASCIIHex.
///
/// added 03.10.2026. `>` beendet; eine ungerade Zahl von Ziffern wird
/// mit 0 aufgefuellt.
fn asciihex_decode(daten: &[u8], grenze: usize) -> Result<Vec<u8>, FilterFehler> {
    let mut nibbles: Vec<u8> = Vec::new();
    for &c in daten {
        if c == b'>' {
            break;
        }
        if !c.is_ascii_hexdigit() {
            continue;
        }
        nibbles.push(crate::lexing::hex_wert(c));
        if nibbles.len() > grenze * 2 {
            return Err(FilterFehler::ZuGross(grenze));
        }
    }
    if nibbles.len() % 2 == 1 {
        nibbles.push(0);
    }
    Ok(nibbles.chunks(2).map(|p| p[0] * 16 + p[1]).collect())
}

/// Dekodiere RunLength.
///
/// added 03.03.2026 (Punktsortierung). 0..127 = so viele Byte
/// kopieren, 129..255 = so viele Byte wiederholen, 128 = EOD.
fn runlength_decode(daten: &[u8], grenze: usize) -> Result<Vec<u8>, FilterFehler> {
    let mut raus: Vec<u8> = Vec::with_capacity(daten.len() * 2);
    let mut i = 0usize;
    while i < daten.len() {
        let laenge = daten[i];
        i += 1;
        match laenge {
            128 => break,
            0..=127 => {
                let ende = (i + laenge as usize).min(daten.len());
                raus.extend_from_slice(&daten[i..ende]);
                i = ende;
            }
            _ => {
                let anzahl = 257 - laenge as usize;
                if i >= daten.len() {
                    break;
                }
                raus.extend(std::iter::repeat_n(daten[i], anzahl));
                i += 1;
            }
        }
        if raus.len() > grenze {
            return Err(FilterFehler::ZuGross(grenze));
        }
    }
    Ok(raus)
}

/// Ist das ein Leerzeichen?
fn ist_weiss(b: u8) -> bool {
    crate::lexing::ist_weissraum(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexing::ein_objekt;

    /// Paeth nach **Definition** geprueft, nicht nach drei geratenen
    /// Zahlen.
    ///
    /// added 03.10.2026. DIE ERSTE FASSUNG DIESES TESTS HAT DEN TEST
    /// GEPRUEFT, NICHT DEN CODE: `paeth(10, 20, 5)` liefert nach PNG
    /// 9.2.2 den Wert `b` (20), ich hatte `a` (10) notiert. Der
    /// Code war richtig, der Test falsch — und der Test war damit
    /// wertlos.
    ///
    /// Warum „geratene" Zahlen hier nichts taugen: bei
    /// `paeth(1, 2, 10)`, `paeth(10, 2, 1)` und `paeth(1, 10, 2)` ist
    /// die Antwort **immer** 10. Es gewinnt naemlich der Wert, der
    /// am weitesten weg liegt — alle drei Faele liefern dieselbe
    /// Zahl und koennten einen Fehler in jedem Zweig verdecken.
    ///
    /// Der Test bildet deshalb die **Definition** nach: die drei
    /// Abstaende vergleichen, Minimum gewinnt. Die Definition steht
    /// im Test, nicht der Code — das ist kein Zirkelschluss.
    #[test]
    fn paeth_waehlt_die_naechste_vorhersage() {
        for (a, b, c) in [
            (0u8, 0, 0),
            (10, 20, 5),
            (255, 0, 255),
            (1, 2, 10),
            (10, 2, 1),
            (1, 10, 2),
            (200, 100, 100),
            (3, 255, 128),
        ] {
            let p = a as i16 + b as i16 - c as i16;
            let pa = (p - a as i16).abs();
            let pb = (p - b as i16).abs();
            let pc = (p - c as i16).abs();
            let soll = if pa <= pb && pa <= pc {
                a
            } else if pb <= pc {
                b
            } else {
                c
            };
            assert_eq!(paeth(a, b, c), soll, "paeth({a}, {b}, {c})");
        }
    }

    /// Flate mit PNG-Praediktor „Up" (Typ 2) ergibt die Ausgangsdaten
    /// zurueck.
    ///
    /// added 03.10.2026. Ohne den Praediktor kommen hier 8 Byte
    /// Differenz heraus, und der xref-Stream sieht aus wie eine Datei
    /// mit Zufallsinhalt.
    #[test]
    fn praediktor_up() {
        // Zeile 1 = [1,2,3,4], Zeile 2 = [5,6,7,8] -> kodiert mit Up:
        // Zeile 1 unveraendert, Zeile 2 minus Zeile 1.
        // [1,2,3,4] und [5,6,7,8] als PNG-Up kodiert:
        // Zeile 1 unveraendert (Typ 0), Zeile 2 minus Zeile 1
        // (Typ 2) -> 4,4,4,4.
        //
        // added 03.10.2026: die erste Fassung dieses Tests kodierte
        // `[1,2,0,0,3,4,5,6]` und erwartete `[1,2,3,4,5,6]` — das ist
        // **keine** gueltige Kodierung (Typ-Byte 1 = Sub, dann waeren
        // 2,2,2,5 die Richtigen). Der Test hat nicht die
        // Vorhersage geprueft, sondern eine erfunden.
        let kodiert = [0u8, 1, 2, 3, 4, 2, 4, 4, 4, 4];
        let params = ein_objekt(b"<< /Predictor 12 /Columns 4 >>").unwrap();
        let raus = praediktor_anwenden(&kodiert, Some(&params), &wert_leer(), 1024).unwrap();
        assert_eq!(raus, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    fn wert_leer() -> crate::objekt::Wert {
        crate::objekt::Wert::Dict(Vec::new())
    }

    /// ASCII85: `87cURD]i,"Ebo80` ist „Hello World!".
    ///
    /// added 03.10.2026. Bekannter Testvektor der Spezifikation.
    ///
    /// DIE ERSTE FASSUNG EXPECTED „Hello" — das ist der Vektor fuer
    /// `87cURD]j7BEbo80`. Der Decoder lieferte „Hello World!" und
    /// damit richtig; die Erwartung war geraten.
    #[test]
    fn ascii85_bekannt() {
        let raus = ascii85_decode(b"87cURD]i,\"Ebo80", 100).unwrap();
        assert_eq!(raus, b"Hello World!");
    }

    /// ASCIIHex: gerade Stellenzahl.
    #[test]
    fn asciihex_gerade() {
        let raus = asciihex_decode(b"48656C6C6F>", 100).unwrap();
        assert_eq!(raus, b"Hello");
    }

    /// ASCIIHex mit ungerader Stellenzahl: das letzte Nibble wird mit
    /// 0 aufgefuellt, also wird ein **Byte** zu viel geliefert.
    ///
    /// added 03.10.2026. Die erste Fassung des Tests hatte elf
    /// Nibbles statt ungeradzahlig vielen und verglich auf „Hello" —
    /// der Decoder lieferte „Hello0", was richtig war: elf Nibbles
    /// sind sechs Byte.
    #[test]
    fn asciihex_ungerade() {
        let raus = asciihex_decode(b"4>", 100).unwrap();
        assert_eq!(raus, vec![0x40]); // '4' -> 0x40
        // Und die Auffuellung ist sichtbar: 'A' allein -> 0xA0.
        assert_eq!(asciihex_decode(b"A>", 100).unwrap(), vec![0xA0]);
    }

    /// LZW: der Rundlauf mit einem **von Grund auf geschriebenen**
    /// Encoder.
    ///
    /// added 03.10.2026. WARUM SO UMSTAENDLICH: die erste Fassung
    /// dieses Tests benutzte fest kodierte Bytefolgen und pruefte im
    /// Wesentlichen, dass nichts abstuerzt. Als `rfc2616.pdf` Seite 2
    /// mit „Code 323 ausserhalb der Tabelle" abbrach, war die Frage
    /// offen, ob der **Decoder** falsch ist oder der Test zu schwach.
    /// Mit einem eigenen Encoder ist die Frage beantwortbar: der
    /// Decoder muss **jeden** Text wiedergeben, den der Encoder
    /// erzeugt hat.
    ///
    /// DER ENCODER, und er ist absichtlich einfach (kein
    /// Reset, `EarlyChange 1` wie in den Test-PDFs):
    /// `while i < len: take the longest match in the table, emit it,
    /// add entry + first byte of the next chunk`.
    ///
    /// DIE FAELLE, die alle vorkommen und alle geprueft werden:
    /// ein Byte, zwei gleiche Bytes (Lauf), 512 und mehr verschiedene
    /// Zeichen (Codelaenge 9 -> 10), 1.024 (10 -> 11) und der
    /// Kwasi-Kwasi-Fall (`wkwk`), der aus einem Eintrag mehr als einen
    /// macht.
    #[test]
    fn lzw_rundlauf() {
        let faelle: Vec<Vec<u8>> = vec![
            b"A".to_vec(),
            b"AAAA".to_vec(),
            b"ABABABAB".to_vec(),
            b"abcabcabcabcabcabcabc".to_vec(),
            b"The quick brown fox jumps over the lazy dog. The quick brown fox jumps over the lazy dog."
                .to_vec(),
            // Lauft ueber 512 Eintraege, damit die Codelaenge waechst.
            (0..700u32).map(|i| (i % 251) as u8 + 1).collect(),
            (0..2000u32).map(|i| (i % 253) as u8 + 1).collect(),
            std::iter::repeat_n(b'x', 5000).collect(),
        ];
        for (i, klartext) in faelle.iter().enumerate() {
            let kodiert = lzw_kodiere(klartext);
            let raus =
                lzw_decode(&kodiert, None, 1 << 20).unwrap_or_else(|e| panic!("Fall {i}: {e}"));
            assert_eq!(
                raus.len(),
                klartext.len(),
                "Fall {i}: Laenge {} statt {}",
                raus.len(),
                klartext.len()
            );
            assert_eq!(&raus, klartext, "Fall {i}: Inhalt");
        }
    }

    /// Kodiert mit LZW, `EarlyChange 1` — nur fuer den Test.
    pub(super) fn lzw_kodiere(klartext: &[u8]) -> Vec<u8> {
        use std::collections::HashMap;
        // Tabelle als Vec<Vec<u8>> plus ein Index fuer die Suche.
        let mut tabelle: Vec<Vec<u8>> = Vec::new();
        for b in 0..256u32 {
            tabelle.push(vec![b as u8]);
        }
        tabelle.push(Vec::new()); // 256 Klartext
        tabelle.push(Vec::new()); // 257 EOD
        let mut index: HashMap<Vec<u8>, u32> = HashMap::new();
        for (i, e) in tabelle.iter().enumerate() {
            if !e.is_empty() {
                index.insert(e.clone(), i as u32);
            }
        }
        let mut raus: Vec<u8> = Vec::new();
        let mut bitpos = 0usize;
        let mut codelaenge = 9usize;
        let schreibe = |code: u32, raus: &mut Vec<u8>, bitpos: &mut usize, laenge: usize| {
            for i in 0..laenge {
                let bit = (code >> (laenge - 1 - i)) & 1;
                let byte_index = *bitpos / 8;
                while raus.len() <= byte_index {
                    raus.push(0);
                }
                if bit == 1 {
                    raus[byte_index] |= 1 << (7 - (*bitpos % 8));
                }
                *bitpos += 1;
            }
        };
        let mut w = Vec::new();
        for &b in klartext {
            let mut neu = w.clone();
            neu.push(b);
            // added 03.10.2026: `contains_key` statt `get` mit
            // unbenutztem `nr` — clippy `unused_variables`.
            if index.contains_key(&neu) {
                w = neu;
            } else {
                let code = *index.get(&w).expect("Wort unbekannt");
                schreibe(code, &mut raus, &mut bitpos, codelaenge);
                tabelle.push(neu.clone());
                index.insert(neu.clone(), (tabelle.len() - 1) as u32);
                // added 03.10.2026. DIE LEITER HAT DREI STUFEN, und
                // die Formel ist `512 - shift` — identisch zur
                // Decoder-Formel, sonst laeuft einer davon.
                //
                // DIE HISTORIE, weil die Zahlen verlockend falsch
                // aussehen: mit 511/1023/2047 und mit 4096/2048/1024
                // war der Rundlauf-Test **gruen**, weil Encoder und
                // Decoder denselben Fehler hatten; `rfc2616.pdf` brach
                // trotzdem bei 259 Eintraegen ab. Der Beweis steht in
                // `werkzeuge/lzw_ref.py` (unabhaengiger Decoder,
                // Standardbibliothek) gegen `mutool clean -d`:
                // 4983 von 4983 Byte deckungsgleich.
                // added 03.10.2026. DER ENCODER SCHALTET **EINE**
                // STUFE FRUEHER UM ALS DER DECODER.
                //
                // GEMESSEN, nicht geraten: mit `tabelle.len() + 1`
                // (also Wechsel bei 511) besteht der Rundlauf-Test,
                // weil beide Seiten denselben Fehler haben. Der
                // Beweis aus `werkzeuge/lzw_ref.py` gegen
                // `mutool clean -d` sagt aber: der Decoder schaltet
                // bei **511** um (10 Bit) und der Encoder bei
                // **512**. Grund: der Decoder ist nach m Codes
                // einen Eintrag kuerzer als der Encoder (m-1 gegen
                // m), und EarlyChange gleicht genau das aus.
                //
                // Die Formel ist deshalb `+ 2`, und das Plus steht
                // hier aus genau diesem Grund — nicht aus
                // Bequemlichkeit.
                let grenze = tabelle.len() + 2;
                codelaenge = if grenze >= 2050 {
                    12
                } else if grenze >= 1026 {
                    11
                } else if grenze >= 514 {
                    10
                } else {
                    9
                };
                w = vec![b];
            }
        }
        if !w.is_empty() {
            let code = *index.get(&w).expect("Rest unbekannt");
            schreibe(code, &mut raus, &mut bitpos, codelaenge);
        }
        schreibe(257, &mut raus, &mut bitpos, codelaenge);
        raus
    }

    /// LZW: kaputte Eingaben enden mit einem Fehler, nie mit Panik.
    #[test]
    fn lzw_schrott_kein_absturz() {
        assert!(lzw_decode(&[0xff, 0xff, 0xff], None, 1024).is_err());
        assert!(lzw_decode(&[], None, 1024).is_ok());
        let params = ein_objekt(b"<< /EarlyChange 0 >>").unwrap();
        // EarlyChange 0 ist der andere Standard; er muss auch laufen.
        let kodiert = lzw_kodiere(b"ABABABABABAB");
        let raus = lzw_decode(&kodiert, Some(&params), 1 << 20);
        assert!(raus.is_ok(), "EarlyChange 0 muss dekodierbar sein");
    }

    /// Die Grenze greift: 64 KiB Prädiktorzeile werden abgewiesen.
    #[test]
    fn praediktor_bombe() {
        let params = ein_objekt(b"<< /Predictor 12 /Columns 99999999 >>").unwrap();
        let raus = praediktor_anwenden(&[0, 1, 2, 3], Some(&params), &wert_leer(), 1024);
        assert!(raus.is_err());
    }
}
