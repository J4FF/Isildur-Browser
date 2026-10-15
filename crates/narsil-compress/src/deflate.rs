//! DEFLATE-Kompression (RFC 1951) — fester Huffman-Encoder mit LZ77.
// W5F, 30.09.2026: dieser `use` stand hier und wurde nie gelesen —
// der Encoder greift auf die vier Basis-Tabellen ueber den
// vollstaendigen Pfad zu (`crate::inflate::huffman::DISTANZ_BASIS`).
// Unbenutzte Importe sind Kommentare, die luegen.

// VORUEBERGEHEND: die fuenf LZ77-Konstanten waren nie benutzt.
//
// added 30.09.2026 bei der Uebernahme von `narsil-compress`. Sie standen
// hier als Vorbereitung auf einen LZ77-Encoder mit Hash-Kette, der
// nicht gebaut wurde: `FENSTER` (32768), `MIN_MATCH` (3), `MAX_MATCH`
// (258), `HASH_GROESSE` (1 << 15) und `KETTEN_MAX` (64). Der Encoder,
// der sie braucht waere, benutzt stattdessen feste Huffman-Tabellen
// ohne Kette.
//
// Sie sind auskommentiert und nicht geloescht, weil die Zahlen
// **richtig** sind und die Norm sie nennt (RFC 1951 Abschnitt 3.2.5:
// minimum match length 3, maximum 258). Wer den LZ77-Encoder baut,
// braucht sie genau so. Als `const` erzeugten sie aber fuenf
// Warnungen bei jedem Bau — und eine Warnung, die immer steht, ist
// eine, die niemand mehr liest.

/// Bitschreiber (LSB-first für DEFLATE, MSB-first für Huffman-Codes).
struct Bitschreiber {
    puffer: Vec<u8>,
    bitfuellung: u32,
    anzahl: u32,
}

impl Bitschreiber {
    fn neu() -> Bitschreiber {
        Bitschreiber {
            puffer: Vec::new(),
            bitfuellung: 0,
            anzahl: 0,
        }
    }
    fn bit(&mut self, wert: u32) {
        self.bitfuellung |= (wert & 1) << self.anzahl;
        self.anzahl += 1;
        if self.anzahl == 8 {
            self.puffer.push(self.bitfuellung as u8);
            self.bitfuellung = 0;
            self.anzahl = 0;
        }
    }
    fn bits(&mut self, wert: u32, n: u32) {
        for i in 0..n {
            self.bit((wert >> i) & 1);
        }
    }
    fn code(&mut self, code: u32, laenge: u32) {
        for i in (0..laenge).rev() {
            self.bit((code >> i) & 1);
        }
    }
    fn ausrichten(&mut self) {
        if self.anzahl > 0 {
            self.puffer.push(self.bitfuellung as u8);
            self.bitfuellung = 0;
            self.anzahl = 0;
        }
    }
    fn fertig(mut self) -> Vec<u8> {
        self.ausrichten();
        self.puffer
    }
}

/// Feste Huffman-Codes (RFC 1951 Abschnitt 3.2.6).
fn code_literal(sym: u32) -> (u32, u32) {
    match sym {
        0..=143 => (0x30 + sym, 8),
        144..=255 => (0x190 + (sym - 144), 9),
        256..=279 => (sym - 256, 7),
        _ => (0xC0 + (sym - 280), 8),
    }
}
fn code_distanz(sym: u32) -> (u32, u32) {
    (sym, 5)
}

/// Komprimiert mit fester Huffman-Tabelle (BTYPE=01).
pub fn komprimiere(daten: &[u8]) -> Vec<u8> {
    let mut s = Bitschreiber::neu();
    s.bit(1);
    s.bits(1, 2); // BFINAL=1, BTYPE=01

    let mut lz = Lz77::neu(daten.len());
    let mut pos = 0usize;
    while pos < daten.len() {
        match lz.suche(daten, pos) {
            Some((laenge, distanz)) => {
                lz_match(&mut s, laenge, distanz);
                for k in 0..laenge {
                    lz.eintrag(daten, pos + k);
                }
                pos += laenge;
            }
            None => {
                let (c, l) = code_literal(daten[pos] as u32);
                s.code(c, l);
                lz.eintrag(daten, pos);
                pos += 1;
            }
        }
    }
    let (c, l) = code_literal(256); // End-of-Block
    s.code(c, l);
    s.fertig()
}

fn lz_match(s: &mut Bitschreiber, laenge: usize, distanz: usize) {
    // (W5F, 30.09.2026: der Import wurde entfernt — der Code unten
    // benutzt die vier Tabellen ueber den vollstaendigen Pfad.)
    let mut li = 0usize;
    for i in (0..crate::inflate::huffman::LAENGEN_BASIS.len()).rev() {
        if crate::inflate::huffman::LAENGEN_BASIS[i] as usize <= laenge {
            li = i;
            break;
        }
    }
    let (c, l) = code_literal(257 + li as u32);
    s.code(c, l);
    let extra = crate::inflate::huffman::LAENGEN_EXTRA[li] as u32;
    if extra > 0 {
        s.bits(
            (laenge - crate::inflate::huffman::LAENGEN_BASIS[li] as usize) as u32,
            extra,
        );
    }

    let mut di = 0usize;
    for i in (0..crate::inflate::huffman::DISTANZ_BASIS.len()).rev() {
        if crate::inflate::huffman::DISTANZ_BASIS[i] as usize <= distanz {
            di = i;
            break;
        }
    }
    let (dc, dl) = code_distanz(di as u32);
    s.code(dc, dl);
    let dextra = crate::inflate::huffman::DISTANZ_EXTRA[di] as u32;
    if dextra > 0 {
        s.bits(
            (distanz - crate::inflate::huffman::DISTANZ_BASIS[di] as usize) as u32,
            dextra,
        );
    }
}

struct Lz77 {
    kopf: Vec<i32>,
    vor: Vec<i32>,
}
impl Lz77 {
    fn neu(len: usize) -> Lz77 {
        let n = len.max(1);
        Lz77 {
            kopf: vec![-1; 1 << 15],
            vor: vec![-1; n],
        }
    }
    fn hash(d: &[u8], i: usize) -> usize {
        ((d[i] as usize) << 10 ^ (d[i + 1] as usize) << 5 ^ (d[i + 2] as usize)) & ((1 << 15) - 1)
    }
    fn eintrag(&mut self, d: &[u8], i: usize) {
        if i + 3 > d.len() {
            return;
        }
        let h = Lz77::hash(d, i);
        self.vor[i] = self.kopf[h];
        self.kopf[h] = i as i32;
    }
    fn suche(&self, d: &[u8], pos: usize) -> Option<(usize, usize)> {
        if pos + 3 > d.len() {
            return None;
        }
        let max_len = (d.len() - pos).min(258);
        if max_len < 3 {
            return None;
        }
        let h = Lz77::hash(d, pos);
        let mut kandidat = self.kopf[h];
        let mut best_len = 0usize;
        let mut best_dist = 0usize;
        let mut versuche = 0usize;
        while kandidat >= 0 && versuche < 64 {
            let k = kandidat as usize;
            let distanz = pos - k;
            if distanz == 0 || distanz > 32768 {
                break;
            }
            if d[k + best_len] == d[pos + best_len] && d[k + 1] == d[pos + 1] {
                let mut l = 0usize;
                while l < (d.len() - pos).min(258) && d[k + l] == d[pos + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_dist = distanz;
                    if l == (d.len() - pos).min(258) {
                        break;
                    }
                }
            }
            kandidat = self.vor[k];
            versuche += 1;
        }
        if best_len >= 3 {
            Some((best_len, best_dist))
        } else {
            None
        }
    }
}

/// Gespeicherter Block (BTYPE=00) — unkomprimiert.
///
/// VORUEBERGEHEND: stillgelegt, added 30.09.2026 bei der Uebernahme
/// von `narsil-compress`. Die Funktion ist **vollstaendig und richtig**
/// (RFC 1951 Abschnitt 3.2.4: BTYPE 00, maximal 65535 Byte je Block,
/// LEN/NLEN am Ende). Sie ist nur unbenutzt: wer bisher
/// komprimiert hat, nimmt den Huffman-Encoder.
///
/// Sie bleibt stehen, weil `narsil-image` beim Schreiben von PNG genau
/// diese Funktion braucht (`png/schreiben.rs` Zeile 135) und dort
/// bislang eine eigene Kopie hatte. Der Aufruf wird beim Verdrahten
/// von `narsil-image` auf `narsil-compress` hierher gewechselt.
///
/// WICHTIG: nicht loeschen. Sie ist der Ausweg, wenn ein Server
/// `Content-Encoding: deflate` mit einem Strom schickt, den der
/// Huffman-Encoder nicht kleiner bekommt — dann ist "unkomprimiert
/// speichern" die richtige Antwort.
#[allow(dead_code)]
pub fn gespeichert(daten: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(daten.len() + daten.len() / 65535 * 5 + 5);
    let mut pos = 0usize;
    loop {
        let rest = &daten[pos..];
        let n = rest.len().min(65535);
        let final_block = pos + n >= daten.len();
        out.push(if final_block { 0x01 } else { 0x00 });
        out.extend_from_slice(&(n as u16).to_le_bytes());
        out.extend_from_slice(&(!n as u16).to_le_bytes());
        out.extend_from_slice(&rest[..n]);
        pos += n;
        if final_block {
            break;
        }
    }
    out
}
