//! DEFLATE-Kompression nach RFC 1951 — der Enkoder.
//!
//! WARUM NUR DIE FESTE HUFFMAN-TABELLE:
//! Eine dynamische Tabelle braucht einen_packages-Optimierer (eigene
//! Huffman-Berechnung, eigene Klassenlage). Die feste Tabelle braucht nur
//! LZ77. Das Ergebnis ist schlechter komprimiert, aber:
//!   * es ist 30 Zeilen Huffman-Code statt 200,
//!   * es ist garantiert RFC-konform (keine Optimierung, die man falsch
//!     machen kann),
//!   * und es ist schnell genug, dass ein Screenshot von 200x200 Pixeln
//!     deutlich kleiner wird als unkomprimiert (gemessen, siehe BERICHT_BILD).
//! Der Auftrag sagt genau das: „feste Huffman-Tabelle genuegt".
//!
//! ZWEI SACHEN, DIE ANSCHLAGEN, WENN MAN DIE SPEC VERKAUFT:
//!   1. Bitreihenfolge: der DEFLATE-Bitstrom ist LSB-first, die
//!      Huffman-CODES werden aber MSB-first geschrieben. Beides gleichzeitig.
//!   2. „Extra-Bits" (Laengenzusatz, Distanzzusatz) werden LSB-first
//!      geschrieben — das ist der Grund, warum sich Code und Zusatz
//!      unterscheiden.

use crate::inflate::{DISTANZ_BASIS, DISTANZ_EXTRA, LAENGEN_BASIS, LAENGEN_EXTRA};

/// Fenstergroesse: 32 KiB, das Maximum von DEFLATE.
const FENSTER: usize = 32768;
/// Kleinster Match, den LZ77 zaehlt. Kuerzere Sachen sind nie billiger
/// als drei Literale, das steht in RFC 1951.
const MIN_MATCH: usize = 3;
/// Groesster Match, den ein Block kodieren kann (Laengencode 285).
const MAX_MATCH: usize = 258;
/// Hashgroesse fuer die Kette. Potenz von zwei, damit `%` ein `&` wird.
const HASH_GROESSE: usize = 1 << 15;
/// Wie viele Kettenglieder wir je Position verfolgen. Mehr bringt bei
/// Bilddaten fast nichts und kostet linear Zeit.
const KETTEN_MAX: usize = 64;

/// Schreibt DEFLATE-Bits in einen Bytepuffer.
///
/// Der Puffer traegt UND-Zustand: `bitfuellung` enthaelt die Bits, die
/// noch nicht voll sind, und `anzahl` zaehlt, wie viele davon schon
/// geschrieben wurden.
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

    /// EIN Bit, LSB-first (so schreibt DEFLATE).
    fn bit(&mut self, wert: u32) {
        self.bitfuellung |= (wert & 1) << self.anzahl;
        self.anzahl += 1;
        if self.anzahl == 8 {
            self.puffer.push(self.bitfuellung as u8);
            self.bitfuellung = 0;
            self.anzahl = 0;
        }
    }

    /// `n` Bits einer Zahl, LSB-first. Fuer „Extra-Bits" und Bit-Laengen.
    fn bits(&mut self, wert: u32, n: u32) {
        for i in 0..n {
            self.bit((wert >> i) & 1);
        }
    }

    /// Einen Huffman-Code MSB-first. Der Grund fuer den Unterschied: die
    /// haeufigsten Symbole bekommen die kuerzesten Codes, und wenn man die
    /// Codes spiegeln wuerde, muesste man sie mit den seltenen beginnen.
    fn code(&mut self, code: u32, laenge: u32) {
        for i in (0..laenge).rev() {
            self.bit((code >> i) & 1);
        }
    }

    /// Fuellt das laufende Byte auf — noetig vor gespeicherten Bloecken
    /// und am Ende des Streams.
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

/// Der Huffman-Code eines Symbols aus der FESTEN Tabelle.
/// `(code_bits, anzahl_bits)` — der Code ist linksbuendig zu schreiben,
/// siehe [`Bitschreiber::code`].
fn code_literal(sym: u32) -> (u32, u32) {
    match sym {
        0..=143 => (0x30 + sym, 8),
        144..=255 => (0x190 + (sym - 144), 9),
        256..=279 => (sym - 256, 7),
        _ => (0xC0 + (sym - 280), 8),
    }
}

/// Der Distanzcode der festen Tabelle: 5 Bit, gerade der Codewert.
fn code_distanz(sym: u32) -> (u32, u32) {
    (sym, 5)
}

/// Komprimiert `daten` mit der festen Huffman-Tabelle.
///
/// Der Aufrufer muss daraus noch den zlib-Rahmen machen; das ist eine
/// andere Schicht (siehe [`crate::zlib`]).
pub fn komprimiere(daten: &[u8]) -> Vec<u8> {
    let mut s = Bitschreiber::neu();

    // Ein einziger Block, BFINAL = 1, BTYPE = 01 (feste Huffman-Tabelle).
    s.bit(1);
    s.bits(1, 2);

    let mut lz = Lz77::neu(daten.len());
    let mut pos = 0usize;
    while pos < daten.len() {
        match lz.suche(daten, pos) {
            Some((laenge, distanz)) => {
                lz_match(&mut s, laenge, distanz);
                // Alle Positionen des Matches in die Kette haengen, sonst
                // findet der naechste Match die Mitte nicht.
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

/// Schreibt ein Match als Laengen-/Distanzpaar.
fn lz_match(s: &mut Bitschreiber, laenge: usize, distanz: usize) {
    // Laenge -> Symbol 257..285
    let mut li = 0usize;
    for i in (0..LAENGEN_BASIS.len()).rev() {
        if LAENGEN_BASIS[i] as usize <= laenge {
            li = i;
            break;
        }
    }
    let (c, l) = code_literal(257 + li as u32);
    s.code(c, l);
    let extra = LAENGEN_EXTRA[li] as u32;
    if extra > 0 {
        s.bits((laenge - LAENGEN_BASIS[li] as usize) as u32, extra);
    }

    // Distanz -> Symbol 0..29
    let mut di = 0usize;
    for i in (0..DISTANZ_BASIS.len()).rev() {
        if DISTANZ_BASIS[i] as usize <= distanz {
            di = i;
            break;
        }
    }
    let (dc, dl) = code_distanz(di as u32);
    s.code(dc, dl);
    let dextra = DISTANZ_EXTRA[di] as u32;
    if dextra > 0 {
        s.bits((distanz - DISTANZ_BASIS[di] as usize) as u32, dextra);
    }
}

/// Der LZ77-Suchraum: eine Hashkette „letzte Position fuer diese 3 Bytes".
struct Lz77 {
    kopf: Vec<i32>,
    vor: Vec<i32>,
}

impl Lz77 {
    fn neu(len: usize) -> Lz77 {
        let n = len.max(1);
        Lz77 {
            kopf: vec![-1; HASH_GROESSE],
            vor: vec![-1; n],
        }
    }

    /// Hash ueber drei Bytes. 15 Bit sind genug: bei Bildern sind viele
    /// Dreierfolgen gleich, und eine gute Verteilung reicht fuer die Kette.
    fn hash(d: &[u8], i: usize) -> usize {
        ((d[i] as usize) << 10 ^ (d[i + 1] as usize) << 5 ^ (d[i + 2] as usize))
            & (HASH_GROESSE - 1)
    }

    fn eintrag(&mut self, d: &[u8], i: usize) {
        if i + MIN_MATCH > d.len() {
            return;
        }
        let h = Lz77::hash(d, i);
        self.vor[i] = self.kopf[h];
        self.kopf[h] = i as i32;
    }

    /// Laengster Match ab `pos`, oder `None`.
    ///
    /// Der Kettenlauf ist begrenzt auf [`KETTEN_MAX`] Glieder. Ohne diese
    /// Grenze ist ein Bild aus 160 000 gleichen Bytes quadratisch langsam —
    /// und der Browser wartet dann auf ein Vorschaubild.
    fn suche(&self, d: &[u8], pos: usize) -> Option<(usize, usize)> {
        if pos + MIN_MATCH > d.len() {
            return None;
        }
        let max_len = (d.len() - pos).min(MAX_MATCH);
        if max_len < MIN_MATCH {
            return None;
        }
        let h = Lz77::hash(d, pos);
        let mut kandidat = self.kopf[h];
        let mut best_len = 0usize;
        let mut best_dist = 0usize;
        let mut versuche = 0usize;

        while kandidat >= 0 && versuche < KETTEN_MAX {
            let k = kandidat as usize;
            let distanz = pos - k;
            if distanz == 0 || distanz > FENSTER {
                break;
            }
            // Schneller Vortest: zwei Byte muessen passen, bevor ueberhaupt
            // die ganze Laenge verglichen wird.
            if d[k + best_len] == d[pos + best_len] && d[k + 1] == d[pos + 1] {
                let mut l = 0usize;
                while l < max_len && d[k + l] == d[pos + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_dist = distanz;
                    if l == max_len {
                        break; // nichts Besseres mehr moeglich
                    }
                }
            }
            kandidat = self.vor[k];
            versuche += 1;
        }
        if best_len >= MIN_MATCH {
            Some((best_len, best_dist))
        } else {
            None
        }
    }
}

/// Komprimiert gar nichts: DEFLATE-Bloecke vom Typ 00.
///
/// Wofuer das da ist, obwohl der Auftrag Kompression verlangt:
///   * der Leser MUSS gespeicherte Bloecke koennen, weil echte Dateien sie
///     benutzen, und
///   * bei komplett unkomprimierbaren Daten ist „gespeichert" oft kleiner
///     als „fixed Huffman" (die Literale kosten dann 8-9 Bit statt 8/8),
///     also entscheidet das der Aufrufer zurueck, wenn es sich nicht lohnt.
pub fn gespeichert(daten: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(daten.len() + daten.len() / 65535 * 5 + 5);
    let mut pos = 0usize;
    // Leere Eingabe: ein einziger finaler Block ueber 0 Byte.
    loop {
        let rest = &daten[pos..];
        let n = rest.len().min(65535);
        let final_block = pos + n >= daten.len();
        out.push(if final_block { 0x01 } else { 0x00 });
        let len = n as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(&rest[..n]);
        pos += n;
        if final_block {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gespeichert_ist_ein_gueltiger_blockstrom() {
        let b = gespeichert(b"Hallo");
        assert_eq!(
            b,
            vec![0x01, 0x05, 0x00, 0xFA, 0xFF, b'H', b'a', b'l', b'l', b'o']
        );
    }

    #[test]
    fn gespeichert_splitet_bei_65535() {
        let d = vec![7u8; 65535 + 10];
        let b = gespeichert(&d);
        // 2 Bloecke: 5 Byte Kopf + 65535, dann 5 + 10
        assert_eq!(b.len(), 5 + 65535 + 5 + 10);
        assert_eq!(b[0], 0x00); // erster Block ist nicht final
        assert_eq!(b[5 + 65535], 0x01); // zweiter ist final
    }

    /// Der Kettenlauf muss begrenzt sein, sonst ist ein Bild aus 160 000
    /// gleichen Bytes unbrauchbar langsam.
    #[test]
    fn kettenlauf_ist_begrenzt() {
        let d = vec![0u8; 100_000];
        let t0 = std::time::Instant::now();
        let _ = komprimiere(&d);
        assert!(
            t0.elapsed().as_secs() < 10,
            "Kompression von 100000 gleichen Byte war langsam: {:?}",
            t0.elapsed()
        );
    }

    #[test]
    fn leere_eingabe_liefert_nur_den_endblock() {
        let b = komprimiere(b"");
        // BFINAL=1, BTYPE=01 -> 0b011 = 3, danach der End-of-Block-Code (7 Bit 0)
        assert_eq!(b, vec![0x03, 0x00]);
    }

    /// DER FILTER-ENKODER gegen den FILTER-DEKODER.
    ///
    /// DEFLATE allein ist rund (`komprimieren_und_entpacken…`). Wenn
    /// `png_lesen(png_schreiben(b)) != b` gilt, liegt es an den Filtern —
    /// und die sitzen in ZWEI Dateien (`png/schreiben.rs` und
    /// `png/lesen.rs`). Dieser Test redet sie direkt an, ohne ein Bild
    /// drumherum zu bauen.
    ///
    /// WICHTIG: `vor` ist hier die ROHE Zeile darueber, genau wie in der
    /// Spezifikation (RFC 2083 Abschnitt 6.2 sagt `Raw(x, y - bpp)`). Der
    /// Decoder liest an dieser Stelle die REKONSTRUIERTE Zeile — und die
    /// ist nach dem Rueckrechnen wieder die rohe. Identisch, aber man muss
    /// es sagen, sonst baut jemand die Zeile zweimal auf.
    ///
    /// ER HAT HIER ZWEI ECHTE FEHLER GEFUNDEN (30.09.2026):
    ///   1. der Enkoder filterte in-place und las den linken Nachbarn aus
    ///      dem eigenen Ausgabepuffer,
    ///   2. und er bekam die gefilterte statt der rohen Zeile darueber.
    #[test]
    fn filter_enkoder_und_dekoder_sind_invers() {
        use crate::png::{lesen::filter_testbar, schreiben::filter_testbar_anwenden};
        for bpp in 1usize..=4 {
            for typ in 0u8..=4 {
                for breite in [1usize, 2, 3, 5, 8, 16] {
                    let roh: Vec<u8> = (0..breite * bpp)
                        .map(|i| ((i * 37 + 11) % 251) as u8)
                        .collect();
                    // Die ROHE Zeile darueber. Bei Filter 2, 3 und 4 geht
                    // sie ein; mit Struktur, damit `b` und `c` nicht 0 sind.
                    let vor: Vec<u8> = (0..breite * bpp)
                        .map(|i| ((i * 53 + 7) % 241) as u8)
                        .collect();
                    let mut gefiltert = vec![0u8; breite * bpp];
                    filter_testbar_anwenden(&roh, &vor, bpp, typ, &mut gefiltert);
                    filter_testbar(&mut gefiltert, &vor, bpp, typ);
                    assert_eq!(
                        gefiltert, roh,
                        "Filter {typ} mit bpp {bpp} und {breite} Byte Breite: \
                         roh={roh:?} vor={vor:?} zurueck={gefiltert:?}"
                    );
                }
            }
        }
    }

    /// DERSELBE Test, aber mit ZWEI Zeilen nacheinander — so wie es im
    /// Bild wirklich zugeht. Die erste Zeile wird zur zweiten Zeile DARUEBER.
    ///
    /// Genau dieser Test ist der, der den Fehler 2 gefangen haette: bei einer
    /// einzigen Zeile ist `vor` noch gar nicht benutzt worden.
    #[test]
    fn zwei_zeilen_uebereinander_sind_konsistent() {
        use crate::png::{lesen::filter_testbar, schreiben::filter_testbar_anwenden};
        for bpp in 1usize..=4 {
            for typen in [[0u8, 1], [2, 3], [4, 4], [1, 2], [3, 4]] {
                let n = 6 * bpp;
                let oben: Vec<u8> = (0..n).map(|i| ((i * 37 + 11) % 251) as u8).collect();
                let unten: Vec<u8> = (0..n).map(|i| ((i * 91 + 5) % 239) as u8).collect();
                let mut f0 = vec![0u8; n];
                let mut f1 = vec![0u8; n];
                // Erste Zeile hat keine Zeile darueber.
                filter_testbar_anwenden(&oben, &[], bpp, typen[0], &mut f0);
                // Zweite Zeile: Nachbar ist die ROHE obere Zeile.
                filter_testbar_anwenden(&unten, &oben, bpp, typen[1], &mut f1);

                // Und jetzt lesen: Zeile 0, dann Zeile 1 mit der
                // REKONSTRUIERTEN oberen Zeile.
                filter_testbar(&mut f0, &[], bpp, typen[0]);
                filter_testbar(&mut f1, &f0, bpp, typen[1]);
                assert_eq!(f0, oben, "obere Zeile, Filter {:?}", typen[0]);
                assert_eq!(f1, unten, "untere Zeile, Filter {:?}", typen);
            }
        }
    }

    #[test]
    fn komprimieren_und_entpacken_sind_invers_zu_einander() {
        fn pruefe(name: &str, daten: &[u8]) {
            let kompr = komprimiere(daten);
            let z = crate::zlib::rahme(&kompr, daten);
            match crate::inflate::entpacke(&z, daten.len() + 64) {
                Ok(raus) => {
                    if raus != daten {
                        let pos = raus
                            .iter()
                            .zip(daten.iter())
                            .position(|(a, b)| a != b)
                            .unwrap_or(daten.len());
                        panic!(
                            "{name}: laeuft bei Byte {pos} auseinander\n\
                             erwartet umgebung: {:?}\n\
                             bekommen        umgebung: {:?}\n\
                             original        umgebung: {:?}",
                            &daten[pos.saturating_sub(4)..(pos + 8).min(daten.len())],
                            &raus[pos.saturating_sub(4)..(pos + 8).min(raus.len())],
                            pos
                        );
                    }
                }
                Err(e) => panic!("{name}: entpacken ergab {e:?}"),
            }
        }

        pruefe("leer", b"");
        pruefe("1 byte", b"x");
        pruefe("alle gleich", &vec![42u8; 5000]);
        pruefe("zwei werte", &{
            let mut v = vec![0u8; 4000];
            for (i, b) in v.iter_mut().enumerate() {
                *b = (i % 2) as u8 * 255;
            }
            v
        });
        pruefe(
            "verlauf",
            &(0..1000u32).map(|i| (i % 256) as u8).collect::<Vec<u8>>(),
        );
        pruefe(
            "text",
            b"der schnelle braune Fuchs springt ueber den faulen Hund",
        );
    }
}
