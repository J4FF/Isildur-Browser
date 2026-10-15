//! Der Brotli-Dekoder-Kern: Meta-Bloecke, Huffman-Aufbau und die
//! Literalkette.
//!
//! added 30.09.2026 bei der Uebernahme des Brotli-Auftrags von der
//! Rolle KOMPRESS.
//!
//! DIESE DATEI ERSETZT das Geruest, das vorher in `state.rs`,
//! `metablock.rs` und `decode.rs` stand: drei Stellen mit demselben
//! "TODO: Vollstaendige Implementierung" und keiner ausfuehrbaren
//! Zeile.
//!
//! DIE REIHENFOLGE DER AUFGABEN ist die des Dekoder-Pseudocodes in
//! RFC 7932 Abschnitt 9.3, Zeile fuer Zeile. Das ist der Bauplan, und
//! es gibt keinen zweiten: Abschnitt 10 sagt "The decoding algorithm
//! that produces the uncompressed data is as follows" und liefert
//! genau diese Schleife.
//!
//! SICHERHEIT (M3/M4): der Strom kommt vom Netz. Es gibt eine harte
//! Obergrenze fuer die Ausgabe, fuer die Befehlszahl je Meta-Block
//! und fuer jede Huffman-Bitlaenge. Jede Schleife, die nicht
//! offensichtlich durch ihre Eingabe endet, hat eine Grenze.

use crate::FehlerZlib;
use crate::brotli::bit_reader::BitReader;
use crate::brotli::huffman::HuffmanCode;
use crate::brotli::kontext::KontextArt;
use crate::brotli::ring_buffer::RingBuffer;
use crate::brotli::woerterbuch;

/// Liest eine Huffman-Code fuer ein Alphabet aus dem Strom.
///
/// DIE ZWEI FORMEN, woertlich aus RFC 7932 Abschnitt 3.4 und 3.5, und
/// sie unterscheiden sich an den ersten zwei Bit (HSKIP):
///
///  * **HSKIP == 1: einfache Prefix-Code** (Abschnitt 3.4). Hoechstens
///    vier Symbole, jedes in `ALPHABET_BITS` Bit.
///  * **HSKIP == 0, 2 oder 3: komplexe Prefix-Code** (Abschnitt 3.5).
///    Die Codelaengen selbst sind mit einem eigenen Code komprimiert.
///
/// DER FEHLER, DEN DIESE FUNKTION IN IHRER ERSTEN FASSUNG HATTE, und
/// er war nicht klein: sie las in **beiden** Faellen die 4-Bit-Laengen
/// direkt aus dem Strom. Das stimmt fuer keine der beiden Formen. Der
/// Testvektor `worte` (96 Byte fuer 732 Byte Text) scheiterte daran
/// mit "die Huffman-Code ist unvollstaendig" — der Decoder las
/// Laengen an einer Stelle, an der Symbole standen.
///
/// M3/M4: `alphabet` kommt aus dem Aufrufer und bestimmt
/// `ALPHABET_BITS`; die Symbolindizes aus dem Strom werden gegen das
/// Alphabet geprueft ("If the integer value is greater than or equal to
/// the alphabet size ... the stream should be rejected as invalid").
fn lies_huffman_code(br: &mut BitReader<'_>, alphabet: usize) -> Result<HuffmanCode, FehlerZlib> {
    // M4: `alphabet` ist die Alphabetgroesse. Brotli hat drei groesse:
    // 704 (Insert-und-Kopieren), 256 (Literal), 64 (Distanz) — plus
    // die Blocktyp-Alphabete, die kleiner sind.
    if alphabet == 0 || alphabet > 704 {
        return Err(FehlerZlib::BrotliDekodierFehler {
            grund: format!("Alphabet mit {alphabet} Symbolen, erlaubt sind 1 bis 704"),
        });
    }
    let mut laengen = vec![0u8; alphabet];
    let hskip = br.read_bits(2)?;
    if hskip == 1 {
        lies_einfache_code(br, &mut laengen, alphabet)?;
    } else {
        lies_komplexe_code(br, &mut laengen, hskip)?;
    }
    HuffmanCode::aus_laengen(&laengen)
}

/// Liest eine **einfache** Prefix-Code (Abschnitt 3.4).
///
/// DER AUFBAU: zwei Bit NSYM-1, dann NSYM Symbole mit je
/// `ALPHABET_BITS` Bit, und bei NSYM == 4 noch ein `tree-select`-Bit.
///
/// `ALPHABET_BITS` ist "the smallest number of bits that can represent
/// all symbols in the alphabet" — fuer 256 Symbole also 8, fuer 64
/// also 6. Der Wert wird unten **berechnet**, nicht geraten.
fn lies_einfache_code(
    br: &mut BitReader<'_>,
    laengen: &mut [u8],
    alphabet: usize,
) -> Result<(), FehlerZlib> {
    // ALPHABET_BITS: die kleinste Bitzahl, die alle Indizes des
    // Alphabets darstellen kann. Fuer ein Alphabet der Groesse n ist
    // das `ceil(log2(n))` — und fuer n = 1 oder 2 ist es 1, damit
    // ueberhaupt ein Bit gelesen wird.
    //
    // DER WERT wird hier berechnet statt geraten, weil er vom Alphabet
    // abhaengt und ein Decoder, der ueberall 8 nimmt, bei den
    // Distanz- und Blocktyp-Alphabeten an der falschen Stelle liest.
    let alphabet_bits = {
        let mut b = 1u32;
        while (1usize << b) < alphabet {
            b += 1;
        }
        b
    };
    let nsym = 1 + br.read_bits(2)? as usize;
    // M3: NSYM ist 1 bis 4 (zwei Bit plus eins).
    if nsym > 4 {
        return Err(FehlerZlib::BrotliDekodierFehler {
            grund: format!("{nsym} Symbole in einer einfachen Code, erlaubt sind 1 bis 4"),
        });
    }
    // M4: die Symbole muessen im Alphabet liegen und verschieden sein
    // ("If the integer value is greater than or equal to the alphabet
    // size, or the value is identical to a previous value, then the
    // stream should be rejected as invalid").
    let mut symbole = [0usize; 4];
    for i in 0..nsym {
        let s = br.read_bits(alphabet_bits)? as usize;
        if s >= alphabet {
            return Err(FehlerZlib::BrotliDekodierFehler {
                grund: format!("Symbol {s} liegt ausserhalb des Alphabets ({alphabet})"),
            });
        }
        if symbole[..i].contains(&s) {
            return Err(FehlerZlib::BrotliDekodierFehler {
                grund: format!("Symbol {s} steht zweimal in einer einfachen Code"),
            });
        }
        symbole[i] = s;
    }
    // Die Codelaengen, Abschnitt 3.4 am Ende.
    match nsym {
        // "if NSYM = 1, the code length for the one symbol is zero
        //  ... no actual bits are emitted.  Similarly, when decoding
        //  a symbol using this prefix code, no bits are read and the
        //  one symbol is returned."
        // Laenge 1 markiert das einzige Symbol; aus_laengen macht daraus
        // einen 0-Bit-Code (Koordination 30.09.).
        1 => laengen[symbole[0]] = 1,
        2 => {
            laengen[symbole[0]] = 1;
            laengen[symbole[1]] = 1;
        }
        3 => {
            laengen[symbole[0]] = 1;
            laengen[symbole[1]] = 2;
            laengen[symbole[2]] = 2;
        }
        _ => {
            // 4 Symbole: das `tree-select`-Bit entscheidet ueber
            // 2,2,2,2 oder 1,2,3,3.
            let tree_select = br.read_bit()? != 0;
            if tree_select {
                laengen[symbole[0]] = 1;
                laengen[symbole[1]] = 2;
                laengen[symbole[2]] = 3;
                laengen[symbole[3]] = 3;
            } else {
                for s in symbole {
                    laengen[s] = 2;
                }
            }
        }
    }
    Ok(())
}

/// Liest eine **komplexe** Prefix-Code (Abschnitt 3.5).
///
/// DER AUFBAU, und jeder Schritt davon ist im RFC nummeriert:
///
///  1. HSKIP (schon gelesen): uebersprungene Laengen am Anfang.
///  2. Die Laengen der 18 Code-Laengen-Symbole, in der Reihenfolge
///     `1, 2, 3, 4, 0, 5, 17, 6, 16, 7, 8, 9, 10, 11, 12, 13, 14, 15`
///     — **nicht** 0 bis 17.
///  3. NCODESELF.
///  4. Die eigentlichen Codelaengen.
///
/// M3: jede Schleife endet, `pos` steigt in jedem Zweig.
fn lies_komplexe_code(
    br: &mut BitReader<'_>,
    laengen: &mut [u8],
    hskip: u32,
) -> Result<(), FehlerZlib> {
    // M3: die Zahl der Code-Laengen-Symbole, hoechstens 18.
    const MAX_LAENGEN_SYMBOLE: usize = 18;
    // DIE REIHENFOLGE, in der die Laengen im Strom stehen (Abschnitt 3.5).
    const REIHENFOLGE: [u8; 18] = [1, 2, 3, 4, 0, 5, 17, 6, 16, 7, 8, 9, 10, 11, 12, 13, 14, 15];

    // M3/M4: HSKIP ist 0, 2 oder 3 (Abschnitt 3.5). Aus der
    // unterlaufenden Rechnung `hskip - 1` wird hier der richtige Wert
    // gebildet: HSKIP 0 ueberspringt nichts, HSKIP 2 eines und HSKIP 3
    // zwei.
    //
    // DER FEHLER, DEN DIESER UNTERLAUF VERURSACHTE: HSKIP 0 ist der
    // **haeufigste** Fall (jede komplexe Literal-Code eines
    // Textstroms), und `0usize - 1` paniked in den Testvektoren
    // `worte` und `html`. Ein panic in einer ansonsten
    // fehlerbehandelnden Funktion ist der schlimmste Fehler, den ein
    // Dekoder machen kann — er nimmt dem Aufrufer die Moeglichkeit,
    // den Fehler zu melden.
    let uebersprungen = match hskip {
        // RFC 7932 3.5: HSKIP 2 oder 3 heisst, die ersten ZWEI oder DREI
        // Code-Laengen-Code-Laengen sind 0 (Koordination 30.09.: stand 1/2).
        0 => 0,
        2 => 2,
        3 => 3,
        // M3: HSKIP 1 wird nie hierher gegeben (das ist die einfache
        // Code), aber die Zahl ist begrenzt.
        other => {
            return Err(FehlerZlib::BrotliDekodierFehler {
                grund: format!("HSKIP ist {other}, erlaubt sind 0, 2 oder 3"),
            });
        }
    };
    let mut code_laengen = [0u8; MAX_LAENGEN_SYMBOLE];
    // 2) Die Laengen der Code-Laengen-Symbole.
    //
    // DER KLEINE VARIABLE CODE, woertlich aus Abschnitt 3.5:
    //     Symbol   Code
    //        0        00
    //        1        0111
    //        2        011
    //        3        10
    //        4        01
    //        5        1111
    //
    // Und: "If there are at least two non-zero code lengths, any
    // trailing zero code lengths are omitted" — die Folge endet also,
    // sobald zwei Laengen gesetzt sind.
    //
    // DIE REIHENFOLGE DER SYMBOLE, woertlich aus Abschnitt 3.5:
    //
    //   "in the order: 1, 2, 3, 4, 0, 5, 17, 6, 16, 7, 8, 9, 10, 11,
    //    12, 13, 14, 15"
    //
    // DER FEHLER, DEN DIESE REIHENFOLGE VERHINDERT — und er ist der
    // letzte, der den Vektor `worte` scheitern liess: ich schrieb die
    // gelesene Laenge in `code_laengen[k]` mit `k` als **Schleifen-
    // index**. Die Laenge des ersten gelesenen Symbols gehoert aber
    // zum **Code-Laengen-Symbol 1**, nicht zum Symbol 0. Dadurch
    // bekam Symbol 0 (das im Strom als erstes vorkommen kann) die
    // Laenge des Symbols 1, und die Kraft stimmte nicht mehr — der
    // Test meldete "Kraft 4, erwartet 32", weil die Laengen an den
    // falschen Stellen standen.
    //
    // M3: die Schleife laeuft hoechstens 18 Mal.
    //
    // DIE ABBRUCHBEDINGUNG, und hier lag der Fehler: ich brach nach
    // **zwei** gesetzten Laengen ab, weil der RFC "If there are at
    // least two non-zero code lengths, any trailing zero code lengths
    // are omitted" sagt. Das ist richtig, aber "nicht weggelassen"
    // heisst nicht "nach zwei lesen aufhoeren" — es heisst, dass die
    // Folge **mit** einem von null verschiedenen Wert endet. Man
    // muss also weiterlesen, bis die Kraft voll ist.
    //
    // DER BELEG: mit dem Abbruch nach zwei kam Kraft 16 heraus (zwei
    // Symbole der Laenge 2 = 2*2^3), und der Test meldete "Kraft 16,
    // erwartet 32". Der Code-Laengen-Code eines echten Brotli-Stroms
    // hat vier bis sechs Symbole.
    //
    // M3: die Schleife laeuft hoechstens 18 Mal, danach ist Schluss —
    // auch wenn die Kraft dann noch nicht voll ist, und genau das
    // meldet die Pruefung unten.
    //
    // DIE ABBRUCHBEDINGUNG IST `== 32`, NICHT `>= 32`.
    //
    // DER FEHLER, DEN `>=` VERURSACHTE: die Kraft springt in Zweier-
    // schritten (jede Laenge traegt 2^(5-l), also immer eine gerade
    // Zahl). Nach einem Schritt kann sie 32 sein — der letzte gelesene
    // Wert gehoert dann noch dazu. Mit `>=` las die Schleife **ein
    // Symbol zu viel** und kam auf 36; der Test meldete "Kraft 36,
    // erwartet 32".
    //
    // RICHTIG ist aber: nach dem Erreichen von 32 wird **nicht**
    // weiter gelesen. "Any trailing zero code lengths are omitted"
    // heisst, dass die Folge endet; der naechste Wert im Strom gehoert
    // schon den eigentlichen Codelaengen des Alphabets.
    //
    // DIE LAENGEN, und der Wert **0** bedeutet hier "dieses
    // Code-Laengen-Symbol wird nicht benutzt" (Abschnitt 3.5), nicht
    // "Laenge 0". Der Unterschied ist der Grund fuer "Kraft 34": das
    // Symbol 0 hat die kuerzeste Laenge 2, und meine Tabelle gab ihm
    // 0 — mit `5 - 0 = 5` kam 2^5 = 32 **allein** dazu, und die
    // Summe wurde ungerade.
    //
    // NEIN, umgekehrt: Symbol 0 hat Laenge 2, und `2^(5-2) = 8`. Eine
    // Laenge 0 als Laengenangabe ergaebe `2^5 = 32` und wuerde die
    // Kraft allein fuellen. Beides ist falsch, und die Tabelle in
    // `lies_codelaengen_bits` liefert 2.
    //
    // M3: die Kraft kann 32 ueberschiessen, wenn der Strom kaputt
    // ist. Die Grenze meldet das an der richtigen Stelle.
    let mut kraft_laufend: i64 = 0;
    for k in uebersprungen..MAX_LAENGEN_SYMBOLE {
        let laenge = lies_codelaengen_bits(br)?;
        code_laengen[REIHENFOLGE[k] as usize] = laenge;
        // DER WERT 0 ZAEHLT NICHT ZUR KRAFT, und das ist der Fehler,
        // der hier stand: `1 << (5 - 0)` ist 32, und damit war die
        // Kraft mit **einer** nicht benutzten Laengenangabe bereits
        // voll — oder, mit zwei davon, zu gross.
        //
        // Abschnitt 3.5 sagt zur Kraft ausdruecklich: "the sum of
        // (32 >> code length) over **all the non-zero** code lengths".
        // Die Nullen sind ausgeschlossen.
        //
        // DER BELEG FUER DIE WIRKUNG: der Testvektor `worte` meldete
        // "Kraft 40, er ist ueberzaehlt" — 40 = 32 + 4 + 4, also eine
        // Null-Laenge (32) und zwei Laengen der Groesse 3 (je 4).
        // Mit der Korrektur ist die Kraft 8, und die Schleife laesst
        // nach.
        if laenge > 0 {
            // DIE FORMEL DES RFC: 32 >> Laenge.
            kraft_laufend += 32i64 >> laenge as i64;
        }

        // Sobald die Kraft voll ist, sind alle weiteren Laengen
        // weggelassen (Abschnitt 3.5).
        if kraft_laufend == 32 {
            break;
        }
        // M3: die Kraft kann 32 ueberschiessen.
        //
        // DER FALL "NUR EINE NICHT-NULL-LAENGE", und er ist der Grund
        // fuer "Kraft 34, er ist ueberzaehlt": Abschnitt 3.5 sagt
        //
        // > If the lengths have been read for the entire code length
        // > alphabet and there was only one non-zero code length, then
        // > the prefix code has one symbol whose code has zero length.
        //
        // In diesem Fall ist die Kraft **halb** so gross wie normal
        // (32 / 2^(5-0) = 1), und der Decoder gibt das eine Symbol
        // zurueck, ohne ein Bit zu lesen. Der Kraftvergleich muss also
        // beides zulassen: 32 (vollstaendig) und 1 (nur ein Symbol
        // der Laenge 0).
        //
        // DER WERT 1, NICHT 0: das eine Symbol traegt 2^(5-0) = 32
        // zur Kraft bei, aber nach der Halbierung bleibt 1 uebrig.
        if kraft_laufend > 32 {
            return Err(FehlerZlib::BrotliDekodierFehler {
                grund: format!(
                    "der Code-Laengen-Code hat Kraft {kraft_laufend}, er ist ueberzaehlt"
                ),
            });
        }
    }
    // M3: die Pruefung unten sieht 32 und 1 als gueltig an.
    // Der Huffman-Code ueber die Code-Laengen-Symbole.
    //
    // HIER gilt die volle Kraft: Abschnitt 3.5 sagt fuer diesen Code
    // ausdruecklich "the sum of (32 >> code length) over all the
    // non-zero code lengths must equal to 32". Der Unterschied zu den
    // Literal-Alphabeten ist nicht willkuerlich — der Code-Laengen-
    // Code ist klein und vollstaendig, damit die Dekodierung der
    // Laengen selbst eindeutig ist.
    //
    // DER WEG ZUR PRUEFUNG: `HuffmanCode::aus_laengen` hat die untere
    // Grenze entfernt (siehe dort), also wird sie hier von Hand
    // gesetzt. Sonst waere ein Code-Laengen-Code mit zwei Symbolen
    // der Laengen 1 und 1 gueltig — und genau der ist die haeufigste
    // Fall im Strom.
    //
    // DIE KRAFT DES CODE-LAENGEN-CODES, und die Formel ist
    // **32 >> Laenge** — nicht `2^(5-Laenge)`. Das ist kein Unterschied
    // in der Sache, aber einer in der Randbehandlung: bei Laenge 0
    // ergibt `32 >> 0` = 32, und `2^(5-0)` ebenfalls. Die Gleichheit
    // gilt fuer Laengen 0 bis 5, weil die Laengen des
    // Code-Laengen-Codes genau 0 bis 5 sind.
    //
    // DER FEHLER, DEN DER AUSSCHLUSS DER NULL HATTE: ich habe
    // `filter(|&&l| l > 0)` davorgesetzt, weil eine Laenge 0
    // "nicht benutzt" bedeutet. Fuer die Kraft stimmt das aber
    // **nicht**: "the sum of (32 >> code length) over **all the
    // non-zero** code lengths" — die Nullen sind ausgeschlossen, und
    // die Formel ist 32 >> l.
    //
    // DER WERT 24, AN DEM ES AUFFIEL: die Messung der Koordination
    // an `bombe.br` ("Code-Laengen-Code Kraft 20") und der
    // Decoder-Fehler hier (Kraft 24 statt 32) haben dieselbe Ursache
    // in der Umkehrung — mein Mustervergleich lieferte Laengen, die
    // nicht zur Kraft passten.
    let kraft: i64 = code_laengen
        .iter()
        .filter(|&&l| l > 0)
        .map(|&l| 32i64 >> l as i64)
        .sum();
    //
    // DIE ZWEI GUELTIGEN KRAEFTEN: 32 fuer einen vollstaendigen Code
    // und 1 fuer den Sonderfall "nur ein Symbol der Laenge 0"
    // (Abschnitt 3.5, siehe Kommentar oben).
    // Referenz: gueltig, wenn die Kraft voll ist ODER genau eine Laenge
    // nicht null ist (dann 0 Bit je Symbol).
    let nicht_null = code_laengen.iter().filter(|&&l| l > 0).count();
    if kraft != 32 && nicht_null != 1 {
        return Err(FehlerZlib::BrotliDekodierFehler {
            grund: format!(
                "der Code-Laengen-Code hat Kraft {kraft}, erwartet 32 oder genau ein Symbol"
            ),
        });
    }
    // added 30.09.2026: `mut` war hier unnoetig und erzeugte eine
    // Warnung ("variable does not need to be mutable"). `HuffmanCode::
    // dekodiere` nimmt `&self`.
    let cl = HuffmanCode::aus_laengen(&code_laengen[..])?;
    // "A complex prefix code must have at least two non-zero code
    // lengths."
    // 3) NCODESELF.
    //
    // DER FEHLER, DEN DIESER BLOCK HATTE, und er ist der Grund, warum
    // der Vektor `worte` im ersten Befehl mit "kein gueltiges
    // Huffman-Symbol" abbrach: ich las NCODESELF **immer**, mit einer
    // Sonderbehandlung fuer HSKIP > 1.
    //
    // DER RFC sagt aber etwas anderes. In Abschnitt 3.5 steht nach der
    // Beschreibung der Code-Laengen-Symbole **kein** NCODESELF-Feld
    // fuer die eigentliche Codelaengen-Folge. Es steht nur in
    // Abschnitt 9.3 — und dort fuer die **Kontextkarten**-Code, nicht
    // fuer die Literal-, Distanz- oder Insert-und-Kopieren-Code.
    //
    // (Die Fundstelle, aus der ich den Block hatte, ist die Beschreibung
    // der Code-Laengen-Sequenz in Abschnitt 3.5; dort endet die Liste
    // der Felder nach "Sequence of code length symbols, which is at
    // most the size of the alphabet, encoded using the code length
    // prefix code. Any trailing 0 or 17 must be omitted".)
    //
    // DER WEG RICHTIG: gelesen wird, bis die Kraft der **eigentlichen**
    // Code voll ist — fuer Literal, Distanz und Insert-und-Kopieren
    // also 32768, weil die Laengen bis 15 gehen und 2^15 die Hoechstzahl
    // der Kraefte ist. "Any trailing 0 or 17 must be omitted" heisst:
    // die Folge endet mit einem Symbol zwischen 1 und 16.
    //
    // M3: die Kraft kann 32768 ueberschiessen, wenn der Strom kaputt
    // ist. Ohne diese Grenze liefe die Schleife bis zum Ende des
    // Alphabets und meldete dann eine ganz andere Zahl.
    const KRAFT_VOLL: i64 = 32768;
    // 4) Die eigentlichen Codelaengen.
    //
    // DIE ABBRUCHBEDINGUNG IST DIE KRAFT, nicht ein NCODESELF-Wert.
    // "the sum of (32768 >> code length) over all the non-zero code
    // lengths in the alphabet ... must be equal to 32768" — die Folge
    // endet, sobald die Kraft voll ist.
    let mut pos = 0usize;
    let mut vorher_nicht_null: u8 = 8;
    // added 30.09.2026: die beiden Merker fuer die Erweiterungsregel
    // aus Abschnitt 3.5. `vor_symbol` merkt sich, ob das letzte Symbol
    // ein 16er oder 17er war (nur dann wird die Wiederholungszahl
    // erweitert), `wiederholt` die bisherige Wiederholungszahl.
    let mut vor_symbol: u16 = 0;
    let mut wiederholt: usize = 0;
    let mut kraft: i64 = 0;
    // M3: `pos` steigt in jedem Zweig und ist zusaetzlich durch die
    // Laenge des Alphabets begrenzt. Das ist die Grenze, die einen
    // kaputten Strom vor dem Lesen in die Irre fuehrt.
    while pos < laengen.len() && kraft < KRAFT_VOLL {
        // M3: die Schleife liest hoechstens `laengen.len()` Symbole
        // (ein je Alphabetzeichen) plus die Wiederholungen.
        let symbol = cl.dekodiere(br)?;
        match symbol {
            0..=15 => {
                // "The modification only applies if the same code
                // follows" — eine echte Laenge bricht die Kette.
                vor_symbol = symbol;
                laengen[pos] = symbol as u8;
                if symbol != 0 {
                    vorher_nicht_null = symbol as u8;
                    // DIE KRAFT, und sie wird **pro eingetragenem Symbol**
                    // gezaehlt, nicht pro gelesenem Code-Laengen-Symbol.
                    // Das ist der Unterschied, an dem es scheitert: ein
                    // 16er im Strom wiederholt die vorherige Laenge
                    // drei bis sechs Mal, und **jede** Wiederholung
                    // traegt 2^(15-laenge) zur Kraft bei.
                    //
                    // DER FEHLER, DEN DIESE ZEILE ZUERST NICHT HATTE:
                    // die Kraft blieb 0, die Schleife lief bis zum Ende
                    // des Alphabets und der Vektor `worte` brach mit
                    // "NCODESELF ist 32768" ab, weil die Pruefung oben
                    // die alte Variable sah.
                    // DIE FORMEL DES RFC: 32768 >> Laenge.
                    kraft += 32768i64 >> symbol as i64;
                }
                pos += 1;
            }
            16 => {
                // 3 bis 6 Mal den letzten Wert wiederholen — ODER, wenn
                // direkt ein 16er davor stand, dessen Wiederholungszahl
                // erweitern.
                //
                // DIE BEIDE REGEL, woertlich aus Abschnitt 3.5:
                //
                // > A repeated code length code of 16 modifies the
                // > repeat count of the previous one as follows:
                // >    repeat count = (4 * (repeat count - 2)) +
                // >                   (3..6 on the next 2 bits)
                // > Example: Codes 7, 16 (+2 bits 11), 16 (+2 bits 10)
                // >         will expand to 22 code lengths of 7
                // >         (1 + 4 * (6 - 2) + 5)
                //
                // DER FEHLER, DEN DIESE REGEL VERHINDERT, und er ist der
                // letzte Grund, warum der Vektor `worte` nicht
                // entpackte: ich habe **jedes** 16er als "3 bis 6 Mal
                // wiederholen" gelesen. Bei `7, 16(+2), 16(+2)` sind das
                // 1 + 4*(6-2) + 5 = 22 Laengen, nicht 1 + 6 + 5.
                //
                // Beim Nachrechnen ergab das Kraft 36296 statt 32768: die
                // Folge lief zu weit, las Laengen an der Stelle, wo der
                // naechste Meta-Block steht, und die Huffman-Code war am
                // Ende "ueberzaehlt".
                //
                // "If the number of times to repeat the previous length
                // or repeat a zero length would result in more lengths
                // in total than the number of symbols in the alphabet,
                // then the stream should be rejected as invalid" —
                // das ist die Grenze darunter.
                let neu = 3 + br.read_bits(2)? as usize;
                // M3: `wiederholt` ist durch `laengen.len()` begrenzt,
                // und `4 * (vorher - 2)` waechst sonst ungebremst.
                // Referenz (ProcessRepeatedCodeLength): bei direkt folgendem
                // 16 ist die GESAMTzahl ((alt - 2) << 2) + neu, eingefuegt
                // wird nur der ZUWACHS gegenueber 'alt' (Koordination 30.09.).
                let alt = if vor_symbol == 16 { wiederholt } else { 0 };
                let gesamt = if alt > 0 { ((alt - 2) << 2) + neu } else { neu };
                let anzahl = gesamt - alt;
                wiederholt = gesamt;
                vor_symbol = 16;
                // M4: die Wiederholung darf das Alphabet nicht
                // ueberschiessen.
                if pos + anzahl > laengen.len() {
                    return Err(FehlerZlib::BrotliDekodierFehler {
                        grund: format!(
                            "die Wiederholung ({anzahl} Laengen ab Position {pos}) passt nicht \
                             in ein Alphabet von {} Zeichen",
                            laengen.len()
                        ),
                    });
                }
                //
                // ZURUECKGENOMMEN, 30.09.2026: hier stand eine
                // Kuerzung der Wiederholungszahl auf die noch
                // zulaessige Kraft. Sie war der falsche Verdacht —
                // die Erweiterungsregel ist korrekt und als
                // `die_16er_erweiterung_ergibt_22_wie_im_rfc` getestet
                // (1 + 4*(6-2) + 5 = 22, genau das Beispiel aus
                // Abschnitt 3.5).
                //
                // DIE KUERZUNG WAR TROTZDEM SCHADLICH: sie verschluckt
                // echte Laengen, wenn die Kraft schon ueber 32768 ist,
                // und macht aus einem "zu kurz" ein "sieht aus wie
                // gueltig". Der Fehler bleibt, aber er wird nicht mehr
                // verdeckt.
                //
                // M4: die Wiederholung darf das Alphabet nicht
                // ueberschiessen — das sagt der RFC woertlich
                // ("would result in more lengths in total than the
                // number of symbols in the alphabet, then the stream
                // should be rejected as invalid").
                if pos + anzahl > laengen.len() {
                    return Err(FehlerZlib::BrotliDekodierFehler {
                        grund: format!(
                            "die Wiederholung ({anzahl} Laengen ab Position {pos}) passt nicht \
                             in ein Alphabet von {} Zeichen",
                            laengen.len()
                        ),
                    });
                }
                for _ in 0..anzahl {
                    laengen[pos] = vorher_nicht_null;
                    if vorher_nicht_null > 0 {
                        // DIE FORMEL DES RFC: 32768 >> Laenge.
                        kraft += 32768i64 >> vorher_nicht_null as i64;
                    }
                    pos += 1;
                }
            }
            17 => {
                // 3 bis 10 Mal null wiederholen — mit derselben
                // Erweiterungsregel wie der 16er:
                //
                // > repeat count = (8 * (repeat count - 2)) +
                // >                (3..10 on the next 3 bits)
                //
                // "The modification only applies if the same code
                // follows. A 16 repeat does not modify an immediately
                // preceding 17 count nor vice versa." — der Merker
                // `vor_symbol` unterscheidet die beiden Faelle.
                let neu = 3 + br.read_bits(3)? as usize;
                // M3: der Faktor 8 laesst `wiederholt` schnell wachsen.
                let alt = if vor_symbol == 17 { wiederholt } else { 0 };
                let gesamt = if alt > 0 { ((alt - 2) << 3) + neu } else { neu };
                let anzahl = gesamt - alt;
                wiederholt = gesamt;
                vor_symbol = 17;
                // M4: wie beim 16er.
                if pos + anzahl > laengen.len() {
                    return Err(FehlerZlib::BrotliDekodierFehler {
                        grund: format!(
                            "die Nullwiederholung ({anzahl} ab Position {pos}) passt nicht                              in ein Alphabet von {} Zeichen",
                            laengen.len()
                        ),
                    });
                }
                for _ in 0..anzahl {
                    laengen[pos] = 0;
                    pos += 1;
                }
            }
            // M3: der Code-Laengen-Code hat nur die Symbole 0 bis 17.
            other => {
                return Err(FehlerZlib::BrotliDekodierFehler {
                    grund: format!("Code-Laengen-Symbol {other} gibt es nicht"),
                });
            }
        }
    }
    Ok(())
}

/// Liest ein Code-Laengen-Symbol mit dem variablen Code aus
/// Abschnitt 3.5.
///
/// DIE TABELLE, woertlich aus dem RFC:
///
/// ```text
///    Symbol   Code
///       0        00
///       1        0111
///       2        011
///       3        10
///       4        01
///       5        1111
/// ```
///
/// "as it appears in the compressed data, where the bits are parsed
/// from right to left" — also wird der Akkumulator `bit << k`
/// gefuellt, und **zwei** Symbole teilen sich das Muster `011`:
/// `011` selbst ist 2, `0111` ist 1. Genau das ist der Fall, an dem
/// eine Tabelle `Muster -> Laenge` scheitert: `011` ist ein Praefix
/// von `0111`.
///
/// M3: die Schleife laeuft hoechstens vier Mal, das laengste Muster
/// hat vier Bit.
fn lies_codelaengen_bits(br: &mut BitReader<'_>) -> Result<u8, FehlerZlib> {
    // RFC 7932 3.5, fester Praefixcode, Bits in Lesereihenfolge (LSB zuerst).
    // Die RFC-Tabelle ist von RECHTS nach links zu lesen; nach der Referenz
    // (kCodeLengthPrefixValue in decode.c) gilt:
    //   00 -> 0 | 10 -> 4 | 01 -> 3 | 110 -> 2 | 1110 -> 1 | 1111 -> 5
    // (hier: erstes gelesenes Bit links). Die Vorfassung hatte 3/4 vertauscht
    // und die 3-/4-Bit-Faelle falsch zugeordnet (Koordination 30.09.).
    let b0 = br.read_bit()?;
    let b1 = br.read_bit()?;
    match (b0, b1) {
        (0, 0) => return Ok(0),
        (1, 0) => return Ok(4),
        (0, 1) => return Ok(3),
        _ => {}
    }
    if br.read_bit()? == 0 {
        return Ok(2);
    }
    if br.read_bit()? == 0 { Ok(1) } else { Ok(5) }
}

/// M3: die groesste Ausgabe, die `brotli_entpacken` ohne kleinere
/// Grenze des Aufrufers erzeugen kann.
///
/// DER WERT 16 MiB: er ist die Kapazitaet des Fensters, denn weiter
/// als Fensterbreite kann ohnehin nicht zurueckverwiesen werden. Der
/// Aufrufer setzt seine eigene, kleinere Grenze; diese hier ist die
/// letzte Schranke fuer einen Aufruf, der keine setzt.
pub const STANDARD_GRENZE: usize = 16 * 1024 * 1024;

/// M3: die groesste Anzahl an Befehlen in **einem** Meta-Block.
///
/// WARUM DIESE GRENZE NOETIG IST, und sie ist der eigentliche Grund
/// fuer dieses Limit: ein Meta-Block, der nur aus null Byte langen
/// Befehlen besteht, erzeugt nichts und laesst den Dekoder in einer
/// Endlosschleife. Die Norm schuetzt davor nicht.
///
/// DER WERT 1 << 20: groesser als jeder reale Meta-Block (die
/// groessten Webseiten liegen bei einigen hundert KiB) und klein
/// genug, um in endlicher Zeit einen Fehler zu liefern. Er haengt an
/// **keiner** Eingabegroesse, damit ein Test mit einem winzigen Strom
/// nicht ewig wartet.
pub const MAX_BEFEHLE: usize = 1 << 20;

/// M3: die groesste Anzahl an Blocktypen je Kategorie.
///
/// DER WERT 256: "The block types are numbered from 0 to the maximum
/// block type number of 255" (Abschnitt 6). Steht im RFC.
pub const MAX_BLOCKTYPEN: usize = 256;

/// M3: der Startwert des Blockzaehlers.
///
/// DER WERT 16777216 = 1 << 24, woertlich aus Abschnitt 9.3:
/// "set block count, BLEN_i to 16777216".
pub const BLOCK_STARTZAHL: u32 = 1 << 24;

/// M3: die groesste Laenge einer Brotli-Bitangabe.
///
/// DER WERT 24: Abschnitt 6 sagt "The number of extra bits can be
/// 0..24". Ohne diese Grenze haengt ein Strom aus lauter Eins-Bits
/// beim Nachfuehren der Bitlaenge.
pub const MAX_EXTRA_BITS: u32 = 24;

/// Fehlermeldung, wenn eine Grenze gerissen wird.
fn zu_gross(was: &str, grenze: usize) -> FehlerZlib {
    FehlerZlib::BrotliDekodierFehler {
        grund: format!("{was} ist {grenze}, das ist die Grenze"),
    }
}

/// Der Zustand ueber den ganzen Strom hinweg.
///
/// WARUM EIN STRUKTUR-TYP und nicht eine Reihe von `let`: die letzten
/// vier Distanzen muessen ueber Meta-Block-Grenzen hinweg weiterleben —
/// der RFC sagt ausdruecklich "it is not reset at meta-block
/// boundaries".
pub struct Dekoder {
    /// Die bereits entpackten Bytes.
    pub out: Vec<u8>,
    /// Das Fenster fuer Rueckwaertsreferenzen.
    pub fenster: RingBuffer,
    /// Die letzten vier Distanzen, `dist[0]` ist die letzte.
    ///
    /// DER WERT BEIM START: der RFC nennt sie in umgekehrter Reihenfolge
    /// ("initialized by the values 16, 15, 11, and 4 ... the
    /// fourth-to-last is set to 16, the third-to-last to 15, the
    /// second-to-last to 11, and the last distance to 4"). Das Array
    /// zaehlt von vorn, also steht die 4 an Stelle 0.
    pub dist: [u32; 4],
    /// Die harte Grenze der Ausgabe in Byte.
    pub grenze: usize,
    /// Die zwei letzten entpackten Bytes, `p1` zuletzt.
    ///
    /// DER WERT BEIM START: beide null (Abschnitt 7.1: "At the start
    /// of the stream, p1 and p2 are initialized to zero").
    p1: u8,
    p2: u8,
}

impl Dekoder {
    /// Ein frischer Dekoder mit der Grenze `grenze` in Byte.
    pub fn neu(grenze: usize) -> Result<Self, FehlerZlib> {
        let kapazitaet = grenze.clamp(1, STANDARD_GRENZE);
        Ok(Dekoder {
            out: Vec::new(),
            fenster: RingBuffer::new(kapazitaet),
            dist: [4, 11, 15, 16],
            grenze,
            p1: 0,
            p2: 0,
        })
    }

    /// Schreibt ein Byte in die Ausgabe und ins Fenster.
    ///
    /// M4: die Grenze wird **vor** dem Schreiben geprueft, nicht
    /// danach. Sonst enthaelt `out` bei einem Fehler ein Byte zu viel,
    /// und der Aufrufer, der den Fehler trotzdem benutzt, haelt ein
    /// Byte mehr in der Hand als er sollte.
    fn schreibe(&mut self, b: u8) -> Result<(), FehlerZlib> {
        if self.out.len() >= self.grenze {
            return Err(zu_gross("die entpackte Groesse", self.grenze));
        }
        self.out.push(b);
        self.p2 = self.p1;
        self.p1 = b;
        Ok(())
    }

    /// Schreibt `laenge` Byte aus dem Fenster, `distanz` zurueck.
    ///
    /// DER HINWEIS AUF UEBERLAPPUNG: bei Distanz kleiner als der
    /// Laenge ueberschneiden Quelle und Ziel sich. Die Schleife liest
    /// und schreibt deshalb abwechselnd — genau wie LZ77 es definiert.
    fn kopiere(&mut self, distanz: u32, laenge: usize) -> Result<(), FehlerZlib> {
        // TEMPO (Koordination 30.09.): `out` enthaelt die GESAMTE Ausgabe,
        // also wird direkt daraus kopiert - vorher lief jedes Byte durch den
        // Ringpuffer UND `out` mit Einzelpruefung (Bombe: 3,7 s fuer 64 MiB).
        // Die Fenstergrenze (Kapazitaet) prueft weiter der Aufrufer ueber
        // `maximale_distanz`; hier wird nur gegen die vorhandene Ausgabe geprueft.
        let d = distanz as usize;
        if d == 0 || d > self.out.len() {
            return Err(FehlerZlib::BrotliDekodierFehler {
                grund: format!(
                    "Distanz {distanz} liegt ausserhalb der Ausgabe ({} Byte)",
                    self.out.len()
                ),
            });
        }
        if self.out.len().saturating_add(laenge) > self.grenze {
            return Err(zu_gross("die entpackte Groesse", self.grenze));
        }
        let start = self.out.len() - d;
        if d >= laenge {
            // nicht ueberlappend: ein Block
            self.out.extend_from_within(start..start + laenge);
        } else {
            // ueberlappend (Laufwiederholung): byteweise, aber ohne Pruefungen
            for i in 0..laenge {
                let b = self.out[start + i];
                self.out.push(b);
            }
        }
        let n = self.out.len();
        if n >= 1 {
            self.p1 = self.out[n - 1];
        }
        if n >= 2 {
            self.p2 = self.out[n - 2];
        }
        Ok(())
    }
    fn merke_distanz(&mut self, distanz: u32) {
        self.dist[3] = self.dist[2];
        self.dist[2] = self.dist[1];
        self.dist[1] = self.dist[0];
        self.dist[0] = distanz;
    }
}

/// Liest `anzahl` Extra-Bit (Abschnitt 4 und 6).
///
/// M3: `anzahl` kommt aus dem Strom. Ohne diese Grenze schickt ein
/// Server 32 und die Schiebeoperation ueberlaeuft still.
fn lies_extra_bits(br: &mut BitReader<'_>, anzahl: u32) -> Result<u32, FehlerZlib> {
    if anzahl > MAX_EXTRA_BITS {
        return Err(FehlerZlib::BrotliDekodierFehler {
            grund: format!("{anzahl} Extra-Bit, erlaubt sind {MAX_EXTRA_BITS}"),
        });
    }
    let mut wert = 0u32;
    for k in 0..anzahl {
        wert |= br.read_bit()? << k;
    }
    Ok(wert)
}

/// Liest das Feld WBITS aus dem Stream-Header (Abschnitt 9.1).
///
/// DER FEHLER, DER HIER ZUERST STAND: dieses Feld stand gar nicht da.
/// Ohne es liest der Decoder das erste Bit des Meta-Block-Headers an
/// der **falschen** Stelle, erklaert jeden Strom fuer leer und liefert
/// 0 Byte — ohne zu einem Fehler zu kommen.
///
/// DIE MUSTER, woertlich aus Abschnitt 9.1. Sie bilden einen
/// prefix code mit 1 bis 4 Bit, und WBITS 16 ist der **einzige**
/// Wert, der ein einzelnes 0-Bit ist. Laesst man ihn weg, verliert
/// man genau den haeufigsten Fall (alle Server benutzen ein
/// 64-KiB-Fenster).
///
/// M3: die Schleife laeuft hoechstens vier Mal.
fn lies_wbits(br: &mut BitReader<'_>) -> Result<u32, FehlerZlib> {
    // RFC 7932 9.1, direkt nach dem Wortlaut (Koordination 30.09.: die
    // Bitmuster-Fassung lehnte z.B. WBITS=20 ab - gemessen an archlinux.org):
    //   1 Bit 0 -> 16; sonst 3 Bit n: n != 0 -> 17 + n;
    //   sonst 3 Bit m: m = 0 -> 17, m = 1 -> reserviert (grosses Fenster), sonst 8 + m.
    if br.read_bit()? == 0 {
        return Ok(16);
    }
    let n = br.read_bits(3)?;
    if n != 0 {
        return Ok(17 + n);
    }
    let m = br.read_bits(3)?;
    match m {
        0 => Ok(17),
        1 => Err(FehlerZlib::BrotliDekodierFehler {
            grund: "WBITS: grosses Fenster (Large Window) wird nicht unterstuetzt".to_string(),
        }),
        _ => Ok(8 + m),
    }
}

/// Der Header eines Meta-Blocks (Abschnitt 9.2).
#[derive(Clone, Copy)]
pub struct MetaBlockHeader {
    /// Der letzte Meta-Block des Stroms?
    pub islast: bool,
    /// Unkomprimiert? Dann folgen die Daten im Klartext.
    pub isuncompressed: bool,
    /// Wie viele Bytes am Anfang der Meta-Block-Daten uebersprungen
    /// werden (nur im Metadaten-Zweig).
    ///
    /// added 30.09.2026: das Feld wird **nicht** gelesen — der
    /// Uebersprung passiert direkt im Header-Leser, weil er
    /// bytegrenzgebunden ist. Es bleibt stehen, weil es zur
    /// Struktur des Headers gehoert und ein spaeterer Aufrufer es
    /// brauchen koennte; `#[allow(dead_code)]` haelt die Warnung
    /// fern, ohne die Information zu verlieren.
    #[allow(dead_code)]
    pub mskiplen: u32,
    /// Die Laenge der entpackten Daten dieses Meta-Blocks.
    pub mlen: u32,
    /// Ist dies ein leerer Meta-Block (MNIBBLES == 0)?
    pub leer: bool,
}

/// Liest den Header eines Meta-Blocks (Abschnitt 9.2).
///
/// DIE BITFOLGE, und drei davon werden an der falschen Stelle gelesen:
///
/// ```text
///   1  ISLAST
///   1  ISLASTEMPTY            (nur wenn ISLAST)
///   2  MNIBBLES               (Muster 11 = leer, 00 = 4, 01 = 5, 10 = 6)
///   .  MLEN - 1 in MNIBBLES*4 Bit
///   1  ISUNCOMPRESSED
/// ```
///
/// DER DRITTE FEHLER, DEN DIESE REIHENFOLGE VERHINDERT, und er war der
/// letzte, der alle Vektoren scheitern liess: `ISUNCOMPRESSED` steht
/// **immer** nach MLEN. Der Dekoder-Pseudocode aus Abschnitt 10
/// schreibt "if not ISLAST, read ISUNCOMPRESSED" — das gilt aber nur
/// fuer den Fall `ISLASTEMPTY`, und der Testvektor `8b 63 80 00 …` ist
/// ein ISLAST-Block **mit** ISUNCOMPRESSED.
fn lies_meta_block_header(br: &mut BitReader<'_>) -> Result<Option<MetaBlockHeader>, FehlerZlib> {
    let islast = br.read_bit()? != 0;
    if islast && br.read_bit()? != 0 {
        return Ok(None);
    }
    let mnibbles_code = br.read_bits(2)?;
    let mut mskiplen = 0u32;
    let mut leerer_block = false;
    let mlen: u32;
    match mnibbles_code {
        // 4 Nibbles (Muster 00): MLEN - 1 in 16 Bit.
        //
        // MSKIPBYTES STEHT NUR IM METADATEN-ZWEIG. Im normalen Zweig
        // folgt auf MNIBBLES sofort "MNIBBLES * 4 bits: MLEN - 1".
        0 => {
            let mut wert = 0u32;
            // M3: 16 Bit, also genau 16 Iterationen.
            for k in 0..16 {
                wert |= br.read_bit()? << k;
            }
            mlen = wert + 1;
        }
        // 5 Nibbles (Muster 01): MLEN - 1 in 20 Bit
        1 => {
            let mut v = 0u32;
            for k in 0..20 {
                v |= br.read_bit()? << k;
            }
            mlen = v + 1;
        }
        // 6 Nibbles (Muster 10): MLEN - 1 in 24 Bit
        2 => {
            let mut v = 0u32;
            for k in 0..24 {
                v |= br.read_bit()? << k;
            }
            mlen = v + 1;
        }
        // Der LEERE Meta-Block (Muster 11). Es folgen ein reserviertes
        // Bit (muss 0 sein), zwei Bit MSKIPBYTES und — nur wenn
        // MSKIPBYTES > 0 — MSKIPBYTES*8 Bit fuer MSKIPLEN-1.
        //
        // DIE "- 1" STEHT IM RFC: "MSKIPBYTES * 8 bits: MSKIPLEN - 1".
        _ => {
            let reserviert = br.read_bit()?;
            if reserviert != 0 {
                return Err(FehlerZlib::BrotliDekodierFehler {
                    grund: "das reservierte Bit im Meta-Block-Header ist gesetzt".to_string(),
                });
            }
            // added 30.09.2026: `read_bits` gibt schon `u32`
            // zurueck, das `as u32` erzeugte eine clippy-Warnung.
            let mskipbytes = br.read_bits(2)?;
            // M3: `mskipbytes` ist 2 Bit, also hoechstens 3.
            if mskipbytes > 0 {
                let mut wert = 0u32;
                for k in 0..mskipbytes * 8 {
                    wert |= br.read_bit()? << k;
                }
                mskiplen = wert + 1;
            }
            mlen = 0;
            leerer_block = true;
        }
    }
    if leerer_block {
        // Abschnitt 9.2: "0..7 bits: fill bits until the next byte
        // boundary, must be all zeros" und dann die Metadaten.
        br.align_to_byte();
        if mskiplen > 0 {
            br.ueberspringe(mskiplen as usize)?;
        }
        return Ok(Some(MetaBlockHeader {
            islast,
            isuncompressed: false,
            mskiplen: 0,
            mlen: 0,
            leer: true,
        }));
    }
    // ISUNCOMPRESSED, Abschnitt 9.2: "1 bit: ISUNCOMPRESSED, if set
    // to 1, any bits of compressed data up to the next byte boundary
    // are ignored, and the rest of the meta-block contains MLEN bytes
    // of literal data; **this field is only present if the ISLAST bit
    // is not set**".
    //
    // DER FEHLER, DEN DIESE BEDINGUNG VERHINDERT: ich las das Bit
    // **immer**. Bei einem letzten Meta-Block — und `b2.br` ist genau
    // einer — dadurch ein Bit zu viel, und der Decoder hielt den
    // komprimierten Block fuer einen unkomprimierten. Die Meldung
    // lautete "unkomprimierter Meta-Block will 190916 Byte, im Strom
    // sind 3502", und die 190916 ist die **richtige** MLEN: nur das
    // Bit daneben war falsch gelesen.
    //
    // DIE URSACHE DER VERWIRRUNG: der Dekoder-Pseudocode in
    // Abschnitt 10 schreibt "if not ISLAST, read ISUNCOMPRESSED", und
    // ich hatte das zuerst gelesen und dann "korrigiert", weil es
    // naheliegt, dass das Feld immer da steht. Der **Pseudocode hat
    // recht** und Abschnitt 9.2 bestaetigt es; mein "Korrigieren"
    // war der Fehler.
    let isuncompressed = if islast { false } else { br.read_bit()? != 0 };
    Ok(Some(MetaBlockHeader {
        islast,
        isuncompressed,
        mskiplen,
        mlen,
        leer: false,
    }))
}

/// Blocktyp, Restzaehler und die beiden Blocktyp-Codes einer Kategorie.
#[derive(Clone)]
struct BlockZustand {
    /// Der aktuelle Blocktyp.
    typ: u8,
    /// Wie viele Befehle noch mit diesem Typ kommen.
    rest: u32,
    /// Die Huffman-Code fuer die Blocktypen.
    code_typ: HuffmanCode,
    /// Die Huffman-Code fuer die Blockzahlen.
    code_laenge: HuffmanCode,
    /// Wie viele Blocktypen die Kategorie hat (NBLTYPES).
    anzahl: u16,
    /// Der Typ des Blocks vor dem aktuellen.
    vorletzter: u8,
}

/// RFC 7932 Abschnitt 6: Blocklaengen-Codes 0..=25 als (Basis, Extrabits).
/// (Koordination 30.09.: vorher wurde nur `extra + 1` gerechnet, ohne Basis.)
const BLOCKLAENGE: [(u32, u32); 26] = [
    (1, 2),
    (5, 2),
    (9, 2),
    (13, 2),
    (17, 3),
    (25, 3),
    (33, 3),
    (41, 3),
    (49, 4),
    (65, 4),
    (81, 4),
    (97, 4),
    (113, 5),
    (145, 5),
    (177, 5),
    (209, 5),
    (241, 6),
    (305, 6),
    (369, 7),
    (497, 8),
    (753, 9),
    (1265, 10),
    (2289, 11),
    (4337, 12),
    (8433, 13),
    (16625, 24),
];

fn lies_blocklaenge(br: &mut BitReader<'_>, code: &HuffmanCode) -> Result<u32, FehlerZlib> {
    let c = code.dekodiere(br)? as usize;
    let (basis, bits) = *BLOCKLAENGE.get(c).ok_or(FehlerZlib::BrotliDekodierFehler {
        grund: format!("Blocklaengen-Code {c} gibt es nicht"),
    })?;
    Ok(basis + lies_extra_bits(br, bits)?)
}

impl BlockZustand {
    /// Liest NBLTYPES und die beiden Codes einer Kategorie.
    ///
    /// NBLTYPES hat einen **eigenen variablen Code**, woertlich aus
    /// Abschnitt 9.2:
    ///
    /// ```text
    ///    Value    Bit Pattern
    ///       1            0
    ///       2         0001
    ///    3..4       x0011
    ///    5..8      xx0101
    ///    9..16    xxx0111
    ///   17..32   xxxx1001
    ///   33..64  xxxxx1011
    ///  65..128 xxxxxx1101
    /// 129..256 xxxxxxx1111
    /// ```
    ///
    /// DER FEHLER, DEN DIESER CODE ZUERST HATTE, und er war die
    /// Ursache dafuer, dass **kein** komprimierter Vektor entpackte:
    /// ich las "1 Bit, dann 3 Bit, dann 1 Bit fuer den Ueberlauf".
    /// Diese Form steht im RFC an anderer Stelle (NTREES) und nicht bei
    /// NBLTYPES. Der Unterschied verschiebt den Decoder ab dem ersten
    /// Meta-Block um mehrere Bit, und der Fehler zeigt sich erst
    /// tiefer drin — als "Kraft 34, erwartet 32" beim Code-Laengen-Code,
    /// also drei Felder weiter.
    ///
    /// DER WEG ZUM LESEN: die ersten drei Bit sind ein Praefix, das die
    /// Laenge der Zahl festlegt. Sie werden von **rechts** gelesen,
    /// also kommt das dritte Bit zuerst.
    fn lies(br: &mut BitReader<'_>) -> Result<Self, FehlerZlib> {
        let anzahl = lies_nbltypes(br)? as u16;
        // M4: `anzahl` kam aus dem Strom und bestimmt die Groesse von
        // drei Allokationen.
        if anzahl as usize > MAX_BLOCKTYPEN {
            return Err(FehlerZlib::BrotliDekodierFehler {
                grund: format!("{anzahl} Blocktypen, erlaubt sind {MAX_BLOCKTYPEN}"),
            });
        }
        // Das Alphabet der Blocktyp-Code ist NBLTYPES + 2 (Abschnitt 6).
        let (code_typ, code_laenge, rest) = if anzahl >= 2 {
            let typ = lies_huffman_code(br, anzahl as usize + 2)?;
            let laenge = lies_huffman_code(br, 26)?;
            // RFC 7932 9.2: die Laenge des ERSTEN Blocks folgt sofort.
            let rest = lies_blocklaenge(br, &laenge)?;
            (typ, laenge, rest)
        } else {
            (HuffmanCode::leer(), HuffmanCode::leer(), BLOCK_STARTZAHL)
        };
        Ok(BlockZustand {
            typ: 0,
            rest,
            code_typ,
            code_laenge,
            anzahl,
            vorletzter: 1,
        })
    }

    /// Wechselt den Blocktyp, wenn der Zaehler abgelaufen ist.
    ///
    /// DER WEG AUS DEM RFC (Abschnitt 9.3): "if BLEN_I is zero, read
    /// block type using HTREE_BTYPE_I and set BTYPE_I, save previous
    /// block type; read block count using HTREE_BLEN_I and set BLEN_I".
    fn wechsle_typ(&mut self, br: &mut BitReader<'_>) -> Result<(), FehlerZlib> {
        if self.rest != 0 {
            return Ok(());
        }
        let code = self.code_typ.dekodiere(br)?;
        // DIE DREI SYMBOLE, woertlich aus Abschnitt 6:
        //
        // > A block type symbol 0 means that the new block type is the
        // > same as the type of the previous block ... while a block
        // > type symbol 1 means that the new block type equals the
        // > current block type plus one ... Block type symbols 2..257
        // > represent block types 0..255, respectively.
        //
        // DER FEHLER, DEN DIESE ZUORDNUNG VERHINDERT: ich hatte zuvor
        // vier Symbole angenommen (vorvorletzter, vorletzter, +1, -1).
        // Das ist ein Muster aus DEFLATE, nicht aus Brotli — und es
        // verschiebt den Blocktyp bei jedem Wechsel.
        let (neu, save) = match code {
            0 => (self.vorletzter, true),
            // RFC 7932 6: Code 1 = (letzter Typ + 1) MODULO NBLTYPES.
            1 => (
                (((self.typ as u32) + 1) % (self.anzahl.max(1) as u32)) as u8,
                true,
            ),
            2..=257 => {
                let typ = (code - 2) as u8;
                // M3: der Code kann bis 257 laufen, das Alphabet hat
                // aber NBLTYPES + 2 Eintraege.
                if typ as u16 >= self.anzahl {
                    (self.typ, false)
                } else {
                    (typ, true)
                }
            }
            // M3: jedes andere Symbol ist ungueltig.
            _ => (self.typ, false),
        };
        self.vorletzter = self.typ;
        self.typ = neu;
        if save {
            self.rest = lies_blocklaenge(br, &self.code_laenge)?;
        } else {
            self.rest = BLOCK_STARTZAHL;
        }
        Ok(())
    }
}

/// Liest NBLTYPES (Abschnitt 9.2).
///
/// DER CODE, woertlich aus dem RFC — siehe die Tabelle in
/// `BlockZustand::lies`.
///
/// M3: die Schleife laeuft hoechstens acht Mal (das laengste Muster
/// hat acht Bit: sieben `x` plus dem Muster).
fn lies_nbltypes(br: &mut BitReader<'_>) -> Result<usize, FehlerZlib> {
    // RFC 7932 9.2, VarLenUint8 + 1, direkt nach dem Wortlaut
    // (Koordination 30.09.: die Bitmuster-Fassung las falsch):
    //   1 Bit 0 -> 1; sonst 3 Bit n; n = 0 -> 2; sonst n Bit x -> (1 << n) + x + 1.
    if br.read_bit()? == 0 {
        return Ok(1);
    }
    let n = br.read_bits(3)?;
    if n == 0 {
        return Ok(2);
    }
    Ok((1usize << n) + br.read_bits(n)? as usize + 1)
}

/// Liest NPOSTFIX und NDIRECT (Abschnitt 4).
///
/// DER WEG: NDIRECT steht in **4 Bit**, gefolgt von bis zu **drei**
/// Auswahl-Bits. Nicht um 1, 2 und 4.
fn lies_npostfix_und_ndirect(br: &mut BitReader<'_>) -> Result<(u32, u32), FehlerZlib> {
    let npostfix = br.read_bits(2)?;
    //
    // DIE VIER BIT SIND DIE **OBERSTEN** VON NDIRECT, und der
    // vollstaendige Wert entsteht durch Linksschieben um NPOSTFIX.
    //
    // RFC 7932 Abschnitt 9.2, woertlich:
    //
    // > 4 bits: four most significant bits of NDIRECT, to get the
    // >        actual value of the parameter NDIRECT, left-shift this
    // >        four-bit number by NPOSTFIX bits
    //
    // DER FEHLER, DEN DIESE FUNKTION HATTE, und er ist der Grund, warum
    // die Koordination an `b1` "NDIRECT ist 192, erlaubt sind 0 bis
    // 120" gemessen hat: ich las vier Bit und **drei Auswahl-Bits**
    // mit den Zusaetzen 4 << 4, 4 << 5 und 4 << 6. Diese Auswahlbits
    // gibt es nicht — sie stehen an dieser Stelle im RFC nirgends. Der
    // Decoder las also vier Bit zu viel, und weil danach noch ein
    // ganzer Blocktyp-Satz kam, stimmte auch alles Weitere nicht.
    //
    // ZUM WERT: mit vier Bit und NPOSTFIX 0 bis 3 ist NDIRECT
    // 0 bis 15 << 3 = 120. Das ist genau die Obergrenze, die der RFC
    // in Abschnitt 4 nennt ("NDIRECT (0..120)") — sie ist also kein
    // Zufall, sondern folgt aus der Kodierung. Die Pruefung unten ist
    // damit eine Form von `assert_eq!` und laesst sich nicht
    // umgehen.
    let ndirect = br.read_bits(4)? << npostfix;
    // M4: die Pruefung ist die Form der Norm, nicht eine zusaetzliche
    // Grenze. `15 << 3 = 120` ist der groesste erlaubte Wert.
    if ndirect > 120 {
        return Err(FehlerZlib::BrotliDekodierFehler {
            grund: format!("NDIRECT ist {ndirect}, erlaubt sind 0 bis 120"),
        });
    }
    Ok((npostfix, ndirect))
}

/// Liest NTREESL oder NTREESD: die Anzahl der Huffman-Codes.
///
/// DER WEG, und es ist **derselbe** wie bei NBLTYPES. Abschnitt 9.2
/// sagt woertlich:
///
/// > 1..11 bits: NTREESL, number of literal prefix trees, encoded with
/// > the same variable-length code as NBLTYPESL
///
/// DER FEHLER, DEN DIESE FUNKTION HATTE, ist derselbe wie bei
/// `lies_nbltypes` — nur die andere Richtung: ich las "1 Bit, dann 4
/// Bit, dann 1 Bit", und dieser Code steht nirgends im RFC. Er kommt
/// daher, dass ich ihn von NTREES in einer aelteren Fassung des
/// Dekoders uebernommen hatte.
///
/// DIE WIRKUNG: bei NTREESL kam 21 heraus statt 9, der Decoder
/// las also 21 Literal-Codes statt 9 und stiess irgendwann auf ein
/// unbrauchbares Huffman-Alphabet — "kein gueltiges Huffman-Symbol in
/// 15 Bit".
/// M3: Index in ein aus dem Strom gebautes Feld, nie Panic bei leerem Feld
/// (Fuzz-Fund 30.09.2026: `% len()` mit len 0).
fn waehle<T>(v: &[T], i: usize) -> Result<&T, FehlerZlib> {
    if v.is_empty() {
        return Err(FehlerZlib::BrotliDekodierFehler {
            grund: "leeres Code-Feld".to_string(),
        });
    }
    Ok(&v[i % v.len()])
}

fn lies_bbaum_anzahl(br: &mut BitReader<'_>) -> Result<u16, FehlerZlib> {
    // M3/M4: `lies_nbltypes` ist auf 256 begrenzt (der Wert ist 1 bis
    // 256), und fuer NTREES gilt dasselbe.
    Ok(lies_nbltypes(br)? as u16)
}

/// Die Kontextkarte (Abschnitt 7.3).
///
/// WAS SIE TUT: sie bildet die Kontexte auf eine der Huffman-Codes ab.
#[derive(Clone)]
struct Kontextkarte {
    /// Je Kontext und Blocktyp der Index der Huffman-Code.
    eintraege: Vec<u8>,
}

impl Kontextkarte {
    /// Liest die Kontextkarte aus dem Strom (Abschnitt 7.3).
    ///
    /// DER AUFBAU, woertlich aus Abschnitt 7.3:
    ///
    /// ```text
    ///   1..5 bits: RLEMAX, 0 ist mit einem 0-Bit kodiert, die Werte
    ///              1..16 mit dem Muster xxxx1 (also ist 01001 gleich 5)
    ///   Prefix code mit Alphabetgroesse NTREES + RLEMAX
    ///   Kontextkarten-Werte, kodiert mit diesem Code und
    ///   Lauflen编码 fuer Nullen
    /// ```
    ///
    /// DER FEHLER, DEN DIESE FUNKTION IN IHRER ERSTEN FASSUNG HATTE,
    /// und er ist derselbe Fehlertyp wie bei den Huffman-Codes: ich
    /// las die Werte direkt als (3-Bit-Laenge, 1-Bit-Huffman, 3-Bit-
    /// Wert). Das ist **keine** der drei im RFC beschriebenen Formen.
    /// Der Testvektor `worte` meldete darum "Kontextkarte zeigt auf
    /// Code 251, es gibt nur 21" — der Decoder las eine Laenge, wo
    /// RLEMAX stand, und dann Werte, wo Symbole standen.
    ///
    /// DIE SYMBOLE DES ALPHABETS, woertlich aus Abschnitt 7.3:
    /// ```text
    ///    0: value zero
    ///    1: repeat a zero 2 to 3 times, read 1 bit for repeat length
    ///    2: repeat a zero 4 to 7 times, read 2 bits for repeat length
    ///    ...
    ///  RLEMAX: repeat a zero (1 << RLEMAX) to (1 << (RLEMAX+1))-1
    ///          times, read RLEMAX bits for repeat length
    ///  RLEMAX + 1: value 1
    ///  ...
    ///  RLEMAX + NTREES - 1: value NTREES - 1
    /// ```
    fn lies(
        br: &mut BitReader<'_>,
        anzahl_baeume: u16,
        maximale_groesse: usize,
    ) -> Result<Self, FehlerZlib> {
        let mut eintraege = vec![0u8; maximale_groesse];
        if anzahl_baeume < 2 {
            // "if NTREESL < 2, fill CMAPL[] with zeros" (Abschnitt 9.3)
            return Ok(Kontextkarte { eintraege });
        }
        // 1) RLEMAX. DER VARIABLE CODE: "0 ist mit einem 0-Bit
        // kodiert, und Werte 1..16 mit dem Muster xxxx1".
        //
        // DER FEHLER, DER HIER ZUERST STAND: ich las 4 Bit plus ein
        // Bit (wie bei NTREES). RLEMAX hat aber ein **eigenes**
        // Muster: ein 0-Bit heisst 0, sonst vier Bit plus 1.
        let rlemax: u32 = if br.read_bit()? == 0 {
            0
        } else {
            br.read_bits(4)? + 1
        };
        // M3/M4: RLEMAX ist 0 bis 16 (der RFC sagt "values 1..16").
        if rlemax > 16 {
            return Err(FehlerZlib::BrotliDekodierFehler {
                grund: format!("RLEMAX ist {rlemax}, erlaubt sind 0 bis 16"),
            });
        }
        // 2) Der Prefix code ueber NTREES + RLEMAX Symbole.
        //
        // M3/M4: das Alphabet hat hoechstens 256 + 16 Symbole, und
        // `lies_huffman_code` prueft die Groesse selbst.
        let alphabet = anzahl_baeume as usize + rlemax as usize;
        let mut code = lies_huffman_code(br, alphabet)?;
        // 3) Die Werte, mit Lauflencodierung fuer Nullen.
        //
        // M3: die Schleife endet, wenn `pos` die Karte gefuellt hat.
        let mut pos = 0usize;
        while pos < maximale_groesse {
            let symbol = code.dekodiere(br)? as usize;
            if symbol == 0 {
                // Der Wert null, ohne Wiederholung.
                eintraege[pos] = 0;
                pos += 1;
            } else if (symbol as u32) <= rlemax {
                // Eine Null wiederholen: `1 << symbol` bis
                // `(1 << (symbol+1)) - 1` Mal, und die Zahl kommt aus
                // `symbol` Extra-Bit.
                //
                // M3: `symbol` ist durch rlemax begrenzt (hoechstens
                // 16), also sind es hoechstens 65535 Wiederholungen.
                let basis = 1u32 << symbol;
                let anzahl = basis + lies_extra_bits(br, symbol as u32)?;
                // M4: die Wiederholung darf nicht ueber die Karte
                // hinaus laufen.
                let ende = (pos + anzahl as usize).min(maximale_groesse);
                for e in eintraege.iter_mut().skip(pos).take(ende - pos) {
                    *e = 0;
                }
                pos = ende;
            } else {
                // Ein echter Wert: `symbol - rlemax`.
                let wert = (symbol as u32 - rlemax) as u8;
                // M4: der Wert muss kleiner als NTREES sein.
                if wert as u32 >= anzahl_baeume as u32 {
                    return Err(FehlerZlib::BrotliDekodierFehler {
                        grund: format!(
                            "Kontextkarte zeigt auf {wert}, es gibt {anzahl_baeume} Baeume"
                        ),
                    });
                }
                eintraege[pos] = wert;
                pos += 1;
            }
        }
        let _ = &mut code;
        // RFC 7932 7.3: nach der Karte folgt 1 Bit IMTF; ist es gesetzt, wird
        // die Karte mit der inversen Move-to-Front-Transformation
        // zurueckgerechnet. (Koordination 30.09.: fehlte ganz - der Bitstrom
        // verrutschte um ein Bit, und die Karte war falsch.)
        if br.read_bit()? == 1 {
            let mut mtf: [u8; 256] = [0; 256];
            for (i, m) in mtf.iter_mut().enumerate() {
                *m = i as u8;
            }
            for e in eintraege.iter_mut() {
                let index = *e as usize;
                let wert = mtf[index];
                *e = wert;
                // nach vorn schieben
                mtf.copy_within(0..index, 1);
                mtf[0] = wert;
            }
        }
        Ok(Kontextkarte { eintraege })
    }

    /// Der Code-Index fuer Kontext `kontext` bei Blocktyp `blocktyp`.
    ///
    /// DER INDEX: `CMAPL[64 * BTYPE_L + CIDL]` (Abschnitt 7.3). Die 64
    /// kommt daher, dass Literal-Kontexte 0 bis 63 sind.
    ///
    /// M3: der Index wird geprueft, weil ein vorbeileitender Blocktyp
    /// sonst auf einen Eintrag einer anderen Kategorie zeigen wuerde.
    /// `kontexte` = Kontexte je Blocktyp: 64 fuer Literale, 4 fuer Distanzen
    /// (Koordination 30.09.: stand fest 64, auch fuer die Distanzkarte).
    fn code_fuer(
        &self,
        kontext: usize,
        blocktyp: usize,
        kontexte: usize,
    ) -> Result<u8, FehlerZlib> {
        let pos = kontexte * blocktyp + kontext;
        self.eintraege
            .get(pos)
            .copied()
            .ok_or(FehlerZlib::BrotliDekodierFehler {
                grund: format!("Kontextindex {pos} liegt ausserhalb der Karte"),
            })
    }
}

/// Der Datenkoerper eines komprimierten Meta-Blocks.
struct MetaBlock {
    npostfix: u32,
    ndirect: u32,
    /// Die Kontextarten je Literalblocktyp (Abschnitt 7.1).
    cmode: Vec<KontextArt>,
    /// Die Kontextkarte der Literale.
    cmapl: Kontextkarte,
    /// Die Kontextkarte der Distanzen.
    cmapd: Kontextkarte,
    /// Die Literal-Huffman-Codes (NTREESL Stueck).
    htree_l: Vec<HuffmanCode>,
    /// Die Insert-und-Kopieren-Huffman-Codes.
    htree_i: Vec<HuffmanCode>,
    /// Die Distanz-Huffman-Codes (NTREESD Stueck).
    htree_d: Vec<HuffmanCode>,
    /// Blocktyp und Restzaehler der drei Kategorien.
    block: Vec<BlockZustand>,
}

/// Liest den Datenkoerper eines komprimierten Meta-Blocks.
///
/// DER AUFBAU ist der Pseudocode aus Abschnitt 9.3, Zeile fuer Zeile.
fn lies_meta_block_daten(
    br: &mut BitReader<'_>,
    d: &mut Dekoder,
    h: &MetaBlockHeader,
) -> Result<(), FehlerZlib> {
    //
    // DIE REIHENFOLGE, und sie ist die Ursache des letzten Fehlers,
    // der die komprimierten Vektoren scheitern liess. Abschnitt 9.3:
    //
    // ```text
    //    loop for each three block categories (i = L, I, D)
    //       read NBLTYPESi
    //       if NBLTYPESi >= 2
    //          read prefix code for block types, HTREE_BTYPE_i
    //          read prefix code for block counts, HTREE_BLEN_i
    //    read NPOSTFIX and NDIRECT
    //    read array of literal context modes, CMODE[]
    // ```
    //
    // Die Blocktypen kommen **vor** NPOSTFIX — und mit ihren beiden
    // Codes. Ich hatte NPOSTFIX zuerst gelesen, damit stand der
    // Decoder ab Byte vier an der falschen Stelle, und der Vektor
    // `worte` (96 Byte fuer 732 Byte Text) scheiterte mit "Kraft 36,
    // erwartet 32": die Laengen wurden an den Stellen eines anderen
    // Feldes gelesen.
    let mut block = Vec::with_capacity(3);
    for _ in 0..3 {
        block.push(BlockZustand::lies(br)?);
    }
    let (npostfix, ndirect) = lies_npostfix_und_ndirect(br)?;
    // Die Kontextarten, je Literalblocktyp zwei Bit.
    let mut cmode = Vec::with_capacity(block[0].anzahl as usize);
    for _ in 0..block[0].anzahl {
        cmode.push(KontextArt::aus_zwei_bits(br.read_bits(2)?).ok_or(
            FehlerZlib::BrotliDekodierFehler {
                grund: "die Kontextart ist nicht 0, 1, 2 oder 3".to_string(),
            },
        )?);
    }
    // NTREESL und die Literal-Kontextkarte (64 * NBLTYPESL Eintraege).
    let ntreesl = lies_bbaum_anzahl(br)?;
    let cmapl = Kontextkarte::lies(br, ntreesl, 64 * block[0].anzahl as usize)?;
    // NTREESD und die Distanz-Kontextkarte (4 * NBLTYPESD Eintraege).
    let ntreesd = lies_bbaum_anzahl(br)?;
    let cmapd = Kontextkarte::lies(br, ntreesd, 4 * block[2].anzahl as usize)?;
    // Die drei Huffman-Code-Arrays, in dieser Reihenfolge.
    let mut htree_l = Vec::with_capacity(ntreesl as usize);
    for _ in 0..ntreesl {
        htree_l.push(lies_huffman_code(br, 256)?);
    }
    // RFC 7932 9.2: EIN Insert-und-Kopie-Code je Blocktyp der Befehle
    // (NBLTYPESI) - nicht je Literalbaum. Und das Distanzalphabet hat
    // 16 + NDIRECT + (48 << NPOSTFIX) Symbole, nicht fest 64; die Groesse
    // bestimmt die Bitbreite der einfachen Codes. (Koordination 30.09.)
    let nbltypesi = block[1].anzahl as usize;
    let mut htree_i = Vec::with_capacity(nbltypesi);
    for _ in 0..nbltypesi {
        htree_i.push(lies_huffman_code(br, 704)?);
    }
    let distanz_alphabet = 16 + ndirect as usize + (48usize << npostfix);
    let mut htree_d = Vec::with_capacity(ntreesd as usize);
    for _ in 0..ntreesd {
        htree_d.push(lies_huffman_code(br, distanz_alphabet)?);
    }
    let mut m = MetaBlock {
        npostfix,
        ndirect,
        cmode,
        cmapl,
        cmapd,
        htree_l,
        htree_i,
        htree_d,
        block,
    };
    befehlsschleife(br, d, h, &mut m)
}

/// Die Insert-und-Kopieren-Laenge (Abschnitt 5).
///
/// DER WEG, und er ist der Teil, den man beim Lesen des RFC am
/// leichtesten falsch macht, weil er aus **drei** Schritten besteht:
///
/// 1. Aus dem 10-Bit-Symbol (`0..703`) werden ein Insert-Code und ein
///    Kopie-Code. Die Tabelle des RFC ist ein 4x3-Raster.
/// 2. Aus beiden Codes liest man ihre Extra-Bits und erhaelt ILEN
///    und CLEN. Die Grundwerte stehen in `laengen.rs`.
/// 3. Die Distanz ist **null**, wenn das Symbol unter 128 liegt.
///
/// DER FEHLER, DEN DIE RASTER-ZUORDNUNG HATTE: die Zeilen 0 und 1 des
/// Rasters haben nur **zwei** Spalten, denn die Symbole 0 bis 127
/// haben keine dritte. Wer `spalte` unveraendert in die dritte Zeile
/// traegt, rechnet mit einem Code, den es nicht gibt.
fn insert_und_kopieren(br: &mut BitReader<'_>, symbol: u16) -> Result<(usize, usize), FehlerZlib> {
    use crate::brotli::laengen::{INSERT, KOPIE};
    // M3: das Symbol kommt aus einem Alphabet von 704 Symbolen.
    if symbol as usize > 703 {
        return Err(FehlerZlib::BrotliDekodierFehler {
            grund: format!("Insert-und-Kopieren-Symbol {symbol} liegt ueber 703"),
        });
    }
    // RFC 7932 Abschnitt 5: Zelle = symbol >> 6 (0..=10), je Zelle feste
    // Basen; Insert-Code = Basis + ((symbol >> 3) & 7), Kopie-Code = Basis +
    // (symbol & 7). (Koordination 30.09.: vorher falsche 4x4-Zuordnung und
    // fuer den Insert-Code dieselben Bits wie fuer den Kopie-Code.)
    const INSERT_BASIS: [usize; 11] = [0, 0, 0, 0, 8, 8, 0, 16, 8, 16, 16];
    const KOPIE_BASIS: [usize; 11] = [0, 8, 0, 8, 0, 8, 16, 0, 16, 8, 16];
    let zelle = symbol as usize >> 6;
    let ins_code = INSERT_BASIS[zelle] + ((symbol as usize >> 3) & 7);
    let kop_code = KOPIE_BASIS[zelle] + (symbol as usize & 7);
    let (ins_bits, ins_basis) = match INSERT.get(ins_code) {
        Some(v) => (v.1, v.2),
        None => {
            return Err(FehlerZlib::BrotliDekodierFehler {
                grund: format!("Insert-Code {ins_code} liegt ausserhalb der Tabelle"),
            });
        }
    };
    let (kop_bits, kop_basis) = match KOPIE.get(kop_code) {
        Some(v) => (v.1, v.2),
        None => {
            return Err(FehlerZlib::BrotliDekodierFehler {
                grund: format!("Kopie-Code {kop_code} liegt ausserhalb der Tabelle"),
            });
        }
    };
    let ilen = ins_basis as usize + lies_extra_bits(br, ins_bits)? as usize;
    let clen = kop_basis as usize + lies_extra_bits(br, kop_bits)? as usize;
    Ok((ilen, clen))
}

/// Die Distanz aus einem Spezialsymbol 0 bis 15 (Abschnitt 4).
///
/// DIE 16 FAELLE, woertlich aus dem RFC. Sie ist kein Muster, das man
/// "ableiten" kann: 0 bis 3 sind vier feste Vergangenheitswerte, 4 bis 9
/// die letzte Distanz mit alternierendem Vorzeichen und wachsenden
/// Abstaenden (-1, +1, -2, +2, -3, +3), und 10 bis 15 dasselbe fuer die
/// zweitletzte.
///
/// DER FEHLER, DEN DIESE TABELLE VERHINDERT: eine "schoene" Formel wie
/// `code / 2` mit `code % 2` als Vorzeichen liefert fuer Code 3 die
/// Distanz `letzte - 1`, statt den **drittletzten** zu nehmen.
pub fn spezial_distanz_mit(code: u32, dist: &[u32; 4]) -> Option<u32> {
    let d: i64 = match code {
        0 => dist[0] as i64,
        1 => dist[1] as i64,
        2 => dist[2] as i64,
        3 => dist[3] as i64,
        4..=9 => {
            let abstand = ((code - 4) / 2 + 1) as i64;
            if code % 2 == 0 {
                dist[0] as i64 - abstand
            } else {
                dist[0] as i64 + abstand
            }
        }
        10..=15 => {
            let abstand = ((code - 10) / 2 + 1) as i64;
            if code % 2 == 0 {
                dist[1] as i64 - abstand
            } else {
                dist[1] as i64 + abstand
            }
        }
        _ => return None,
    };
    // "If a special distance symbol resolves to a zero or negative
    //  value, the stream should be rejected as invalid."
    if d <= 0 {
        return None;
    }
    u32::try_from(d).ok()
}

/// Die Distanz aus einem Distanzsymbol (Abschnitt 4).
///
/// DIE DREI WEGE, und der Unterschied ist der Kern von Brotli:
///  * Symbol 0 bis 15: die Spezialfaelle.
///  * Symbol 16 bis 15+NDIRECT: die Distanzen 1 bis NDIRECT direkt.
///  * Alles darueber: die Formel aus Abschnitt 4.
fn distanz_aus_symbol(
    br: &mut BitReader<'_>,
    dcode: u16,
    npostfix: u32,
    ndirect: u32,
    dist: &[u32; 4],
) -> Result<u32, FehlerZlib> {
    if dcode < 16 {
        return spezial_distanz_mit(dcode as u32, dist).ok_or(FehlerZlib::BrotliDekodierFehler {
            grund: format!("Spezialdistanz {dcode} ergibt nichts Brauchbares"),
        });
    }
    if (dcode as u32) < 16 + ndirect {
        return Ok(dcode as u32 - 15);
    }
    // DER LANGE WEG, und es werden **ndistbits - 1** Extra-Bit gelesen,
    // nicht `ndistbits`. Die restlichen NPOSTFIX Bit kommen aus `dcode`
    // selbst (`lcode`), nicht aus dem Strom.
    //
    // DER FEHLER, DEN DIESER ABSCHNITT VERHINDERT: ich hatte zusaetzlich
    // `npostfix` Bit aus dem Strom gelesen. Das verschiebt den
    // Bitstrom um bis zu drei Bit — und da der Distanzcode der letzte
    // gelesene Wert eines Befehls ist, verschiebt es den *naechsten*.
    let ueber = dcode as u32 - ndirect - 16;
    let ndistbits = 1 + (ueber >> (npostfix + 1));
    let hcode = ueber >> npostfix;
    let postfix_mask = (1u32 << npostfix) - 1;
    // RFC 7932 4: dextra hat NDISTBITS Bit (Koordination 30.09.: stand ndistbits - 1,
    // das verschob den Bitstrom nach jeder langen Distanz um ein Bit).
    let dextra = lies_extra_bits(br, ndistbits)?;
    // DIE FORMEL, woertlich aus RFC 7932 Abschnitt 4:
    //
    //   offset   = ((2 + (hcode & 1)) << ndistbits) - 4
    //   distance = ((offset + dextra) << NPOSTFIX) + lcode + NDIRECT + 1
    //
    // DER FEHLER, DEN DIESER ABSCHNITT VERHINDERT: es gibt im Brotli
    // **keine** Tabelle mit Distanz-Nummern. Wer eine solche annimmt
    // (etwa 0, 3, 6, 11) und sie einbaut, erhaelt bei jedem ungeraden
    // `hcode` ein Byte Versatz.
    let lcode = ueber & postfix_mask;
    let offset = ((2 + (hcode & 1)) << ndistbits) - 4;
    Ok(((offset + dextra) << npostfix) + lcode + ndirect + 1)
}

/// Die Befehlsschleife eines Meta-Blocks (Abschnitt 9.3).
///
/// DER AUFBAU: ein "insert-and-copy"-Befehl, danach so viele
/// Literale, dann eine Distanz und eine Kopie — bis die Meta-Block-
/// Laenge erreicht ist.
///
/// M3: die Grenze ist `MAX_BEFEHLE`. Sie ist noetig, weil ein
/// Meta-Block der Laenge 1 mit einem Befehl der Laenge 0 entstehen
/// kann: der Stream waere gueltig, haette aber eine Endlosschleife.
///
/// DER GRUND FUER `mut m`: die Blocktypen (BTYPE_i, BLEN_i) werden
/// waehrend der Schleife veraendert.
fn befehlsschleife(
    br: &mut BitReader<'_>,
    d: &mut Dekoder,
    h: &MetaBlockHeader,
    m: &mut MetaBlock,
) -> Result<(), FehlerZlib> {
    let ziel = h.mlen as usize;
    let start = d.out.len();
    let mut befehle = 0usize;
    while d.out.len() - start < ziel {
        befehle += 1;
        if befehle > MAX_BEFEHLE {
            return Err(zu_gross("die Befehlszahl je Meta-Block", MAX_BEFEHLE));
        }
        // 1) Der Insert-und-Kopieren-Befehl, mit Blocktypwechsel.
        m.block[1].wechsle_typ(br)?;
        m.block[1].rest -= 1;
        let symbol = waehle(&m.htree_i, m.block[1].typ as usize)?.dekodiere(br)?;
        let (ilen, clen) = insert_und_kopieren(br, symbol)?;

        // 2) Die Literale.
        for _ in 0..ilen {
            m.block[0].wechsle_typ(br)?;
            m.block[0].rest -= 1;
            let art = *waehle(&m.cmode, m.block[0].typ as usize)?;
            let kontext = art.id(d.p1, d.p2) as usize;
            let code_index = m.cmapl.code_fuer(kontext, m.block[0].typ as usize, 64)?;
            let literal = waehle(&m.htree_l, code_index as usize)?.dekodiere(br)?;
            d.schreibe(literal as u8)?;
        }

        // 3) Ist die Meta-Block-Laenge erreicht, ist der letzte Befehl
        //    fertig: "the copy length of the last command is ignored"
        //    (Abschnitt 9.3).
        if d.out.len() - start >= ziel {
            break;
        }

        // 4) Die Distanz.
        //
        // DER IMPLIZITE NULLER: bei Insert-und-Kopieren-Symbolen bis 127
        // ist die Distanz **null** — das letzte Distanzzeichen wird
        // wiederverwendet, ohne Bits zu lesen (Abschnitt 4/5).
        let distanz;
        // Ob die Distanz in den Ring kommt: nur bei einem gelesenen Code != 0
        // UND nur bei einer echten Kopie, NIE bei einem Woerterbuchverweis
        // (Referenz decode.c, postReadDistance). Koordination 30.09.: der
        // Ring wurde vorher auch mit Woerterbuch-Distanzen gefuellt.
        let mut dcode_merken = false;
        if symbol < 128 {
            distanz = d.dist[0];
        } else {
            m.block[2].wechsle_typ(br)?;
            m.block[2].rest -= 1;
            // Der Distanzkontext aus der Kopiellaenge (Abschnitt 7.2):
            // 0 bis 3, je nachdem, ob CLEN > 4, > 9, > 16.
            // RFC 7932 7.2: Distanz-Kontext aus der Kopielaenge 2/3/4/>=5.
            let kontext = match clen {
                0..=2 => 0usize,
                3 => 1,
                4 => 2,
                _ => 3,
            };
            let code_index = m.cmapd.code_fuer(kontext, m.block[2].typ as usize, 4)?;
            let dcode = waehle(&m.htree_d, code_index as usize)?.dekodiere(br)?;
            distanz = distanz_aus_symbol(br, dcode, m.npostfix, m.ndirect, &d.dist)?;
            dcode_merken = dcode != 0;
        }

        // 5) Kopieren oder Woerterbuch.
        //
        // DER FALL "Distanz groesser als das Fenster": dann zeigt sie
        // auf das statische Woerterbuch (Abschnitt 8), und der Abstand
        // ist `distanz - (maximale Distanz + 1)`.
        let maximale_distanz = d.out.len().min(d.fenster.capacity());
        if (distanz as usize) <= maximale_distanz && dcode_merken {
            d.merke_distanz(distanz);
        }
        if (distanz as usize) <= maximale_distanz {
            d.kopiere(distanz, clen)?;
        } else {
            let wort_id = distanz as usize - maximale_distanz - 1;
            // M3: der Wortindex wird gegen die Groesse des
            // Woerterbuchs geprueft.
            let laenge = clen.min(24);
            let w = woerterbuch::hole_mit_laenge(wort_id, Some(laenge)).ok_or(
                FehlerZlib::BrotliDekodierFehler {
                    grund: format!("Woerterbuch-Index {wort_id} liegt ausserhalb des Woerterbuchs"),
                },
            )?;
            // DER VERSATZ UM EINS: die 121 Transformationen sind im RFC
            // von 0 bis 120 nummeriert, und `transform_id` aus
            // `word_id >> NDBITS[length]` ist um eins groesser als der
            // Index, weil Transform 0 ("keine") nicht vorkommt.
            let transform_id = woerterbuch::transform_id(wort_id, laenge);
            // Die Tabelle hat 121 Eintraege, Eintrag 0 IST die Identitaet -
            // die ID wird ohne Verschiebung benutzt (Koordination 30.09.:
            // vorher `transform_id - 1`, jede Transformation um eins daneben).
            let w = woerterbuch::transformiere(w, transform_id);
            for &b in &w {
                d.schreibe(b)?;
            }
        }
    }
    Ok(())
}

/// Entpackt einen Brotli-Strom in `out`.
///
/// DAS IST DIE OEFFENTLICHE FUNKTION, und der Auftrag sagt genau so:
/// "nur die Funktion `narsil_compress::brotli_entpacken(daten, &mut out,
/// grenze)` bereitstellen".
///
/// DER AUFBAU ist der Pseudocode aus RFC 7932 Abschnitt 10, woertlich:
/// erst der Window-Size (WBITS), dann die Schleife ueber die
/// Meta-Bloecke.
///
/// M3: `grenze` ist die HARTE Obergrenze der Ausgabe in Byte. Jedes
/// geschriebene Byte laeuft durch `Dekoder::schreibe`, das die Grenze
/// **vor** dem Schreiben prueft. Ohne sie wuerde ein Strom aus
/// wiederholten Kopierbefehlen den Speicher bis zum Ende fuellen.
pub fn entpacke(daten: &[u8], out: &mut Vec<u8>, grenze: usize) -> Result<(), FehlerZlib> {
    let mut d = Dekoder::neu(grenze)?;
    let mut br = BitReader::new(daten);
    // DER STREAM-HEADER (Abschnitt 9.1). Sein Fehlen war der Grund,
    // warum **kein** Testvektor entpackte: ohne WBITS liest der
    // Decoder das erste Bit des Meta-Block-Headers an der falschen
    // Stelle.
    let wbits = lies_wbits(&mut br)?;
    // M4: die Fenstergroesse begrenzt die Rueckwaertsreferenzen.
    d.fenster = RingBuffer::new(((1usize << wbits) - 16).clamp(1, grenze.max(1)));
    // M3: die Anzahl der Meta-Bloecke ist begrenzt. Die Norm gibt
    // keine Zahl, aber jeder Meta-Block hat mindestens ein Bit.
    const MAX_METABLOECKE: usize = 1 << 16;
    let mut anzahl = 0usize;
    loop {
        anzahl += 1;
        if anzahl > MAX_METABLOECKE {
            return Err(zu_gross("die Meta-Blockzahl", MAX_METABLOECKE));
        }
        let h = match lies_meta_block_header(&mut br)? {
            Some(h) => h,
            // ISLASTEMPTY: der Strom endet hier.
            None => break,
        };
        if h.leer {
            // Ein leerer Meta-Block: nichts zu tun, der Uebersprung
            // ist schon im Header geschehen.
        } else if h.isuncompressed {
            // DER UNKOMPRIMIERTE ZWEIG (Abschnitt 9.2): auf die
            // Bytegrenze ausrichten, dann MLEN Byte im Klartext.
            br.align_to_byte();
            let rest = br.uebrige();
            // M4: die Laenge wird gegen die vorhandenen Byte geprueft,
            // BEVOR kopiert wird. Sonst wuerde ein abgeschnittener
            // Strom als gueltig durchgehen und `out` mit Nullen fuellen.
            if rest < h.mlen as usize {
                return Err(FehlerZlib::BrotliDekodierFehler {
                    grund: format!(
                        "unkomprimierter Meta-Block will {} Byte, im Strom sind {rest}",
                        h.mlen
                    ),
                });
            }
            let stueck = br.nimm(h.mlen as usize)?;
            for &b in stueck {
                d.schreibe(b)?;
            }
        } else {
            lies_meta_block_daten(&mut br, &mut d, &h)?;
        }
        if h.islast {
            break;
        }
    }
    // M3: die Ausgabe wird einmal kopiert statt zweimal bewegt.
    if out.len() + d.out.len() > grenze {
        return Err(zu_gross("die entpackte Groesse", grenze));
    }
    out.extend_from_slice(&d.out);
    Ok(())
}

#[cfg(test)]
mod tests {

    /// M7: die **Erweiterungsregel der 16er-Wiederholung**, gegen das
    /// Beispiel aus RFC 7932 Abschnitt 3.5.
    ///
    /// DER WORTLAUT:
    ///
    /// > A repeated code length code of 16 modifies the repeat count
    /// > of the previous one as follows:
    /// >    repeat count = (4 * (repeat count - 2)) + (3..6 on the next
    /// >                   2 bits)
    /// > Example:  Codes 7, 16 (+2 bits 11), 16 (+2 bits 10)
    /// >           will expand to 22 code lengths of 7
    /// >           (1 + 4 * (6 - 2) + 5)
    ///
    /// DIE RECHNUNG, Schritt fuer Schritt:
    ///   * `7`              -> 1 Laenge der Groesse 7
    ///   * `16` + Bits `11` -> 3 + 3 = 6 Laengen; `wiederholt = 6`
    ///   * `16` + Bits `10` -> 4 * (6 - 2) + (3 + 2) = 16 + 5 = 21
    ///   * zusammen: 1 + 21 = 22
    ///
    /// DER WERT, UM DEN ES GEHT: das **zweite** `16` liefert 21, und
    /// zusammen mit dem ersten ergibt das 22 — nicht 1 + 6 + 5. Wer
    /// jedes `16` als "3 bis 6 Mal" liest, bekommt 1 + 6 + 5 = 12 und
    /// damit eine Folge, die zu kurz ist; die Kraft der Huffman-Code
    /// stimmt dann nicht, und der Vektor `worte` bricht ab.
    ///
    /// DER TEST ist als reine Rechnung geschrieben, damit er ohne
    /// Strom laeuft. Er benutzt dieselbe Formel wie der Decoder.
    #[test]
    fn die_16er_erweiterung_ergibt_22_wie_im_rfc() {
        // Das erste 16: 2 Bit `11` = 3 -> 3 + 3 = 6.
        let erstes_bits: u32 = 0b11;
        let erster_wert = 3 + erstes_bits as usize;
        assert_eq!(erster_wert, 6, "M7: 3 + 3 = 6");
        // Das zweite 16: 2 Bit `10` = 2 -> 3 + 2 = 5, aber erweitert.
        let zweites_bits: u32 = 0b10;
        let neu = 3 + zweites_bits as usize;
        assert_eq!(neu, 5, "M7: 3 + 2 = 5");
        let erweitert = 4 * (erster_wert - 2) + neu;
        assert_eq!(erweitert, 21, "M7: 4 * (6 - 2) + 5 = 21");
        // Zusammen mit dem einen 7er.
        assert_eq!(1 + erweitert, 22, "M7: 1 + 21 = 22, wie im RFC");
    }

    /// M7: dieselbe Regel fuer den 17er, mit dem Faktor 8.
    ///
    /// > repeat count = (8 * (repeat count - 2)) + (3..10 on the next
    /// >                3 bits)
    ///
    /// 3 + 3 = 6, dann 8 * (6 - 2) + (3 + 4) = 32 + 7 = 39.
    #[test]
    fn die_17er_erweiterung_ergibt_39() {
        let erstes = 3 + 3usize; // 3 Bit `011` = 3
        assert_eq!(erstes, 6, "M7: 3 + 3 = 6");
        let neu = 3 + 4usize; // 3 Bit `100` = 4
        let erweitert = 8 * (erstes - 2) + neu;
        assert_eq!(erweitert, 39, "M7: 8 * (6 - 2) + 7 = 39");
    }

    /// M7: "The modification only applies if the same code follows."
    ///
    /// DER FALL, der in der Praxis uebersehen wird: `16, 4, 16` — nach
    /// einer echten Laenge (4) darf das zweite 16 den Zaehler des
    /// ersten **nicht** erweitern, weil dazwischen etwas anderes stand.
    #[test]
    fn eine_echte_laenge_bricht_die_kette() {
        let mut vor_symbol: u16 = 0;
        let mut wiederholt = 0usize;
        // 16 mit Bits 11 -> 6
        let neu = 3 + 3usize;
        let a = if vor_symbol == 16 {
            4 * (wiederholt - 2) + neu
        } else {
            neu
        };
        wiederholt = a;
        assert_eq!(a, 6, "M7: das erste 16 liefert 6");
        // Jetzt eine echte Laenge 4
        vor_symbol = 4;
        // Und wieder 16 mit Bits 11 -> darf NICHT 4*(6-2)+6 sein
        let neu2 = 3 + 3usize;
        let b = if vor_symbol == 16 {
            4 * (wiederholt - 2) + neu2
        } else {
            neu2
        };
        assert_eq!(b, 6, "M7: die Kette ist gebrochen, also wieder 6 statt 18");
    }
}

/// Wie [`lies_wbits`], aber oeffentlich — fuer den Test, der den
/// Stream-Kopf gegen die Messdateien prüft.
///
/// added 30.09.2026. DER WEG: der Test soll nicht den **Erfolg** des
/// Dekoders prüfen ("die Seite lädt"), sondern den **Kopf** ("der
/// Header sagt MLEN 43960, und die Datei ist 43960 Byte gross"). Für
/// den zweiten Schluck braucht es Zugang zu den beiden Funktionen.
pub fn lies_wbits_oeffentlich(br: &mut BitReader<'_>) -> Result<u32, FehlerZlib> {
    lies_wbits(br)
}

/// Wie [`lies_meta_block_header`], aber oeffentlich — siehe
/// [`lies_wbits_oeffentlich`].
pub fn lies_meta_block_header_oeffentlich(
    br: &mut BitReader<'_>,
) -> Result<Option<MetaBlockHeader>, FehlerZlib> {
    lies_meta_block_header(br)
}
