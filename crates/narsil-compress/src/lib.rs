//! narsil-compress — INFLATE/DEFLATE, gzip, zlib und Brotli in reinem Rust.
//
// added 02.10.2026, Rolle KOMPRESS: DAS MODUL IST WIEDER EINGESCHALTET.
// Das war die Bedingung („bleibt auskommentiert, bis es fehlerfrei
// baut"), und sie ist erfuellt — nicht behauptet, sondern gemessen:
//
//   werkzeuge/bau.sh narsil-compress check  -> GRUEN
//   werkzeuge/bau.sh narsil-compress test   -> GRUEN, 33 Tests, 0 rot
//   werkzeuge/bau.sh narsil-compress clippy -> GRUEN
//   werkzeuge/bau.sh narsil-compress fmt    -> GRUEN
//
// DER BEWEIS, dass der komprimierte Weg wirklich geht (das war der
// offene Punkt, und der alte Dateikopf behauptete das Gegenteil):
//   * `tests/brotli_echt.rs::alle_referenzdateien_bytegleich` — die
//     RFC-7932-Anhang-A-Vektoren (q0, q1, q2, q5, q9, q11) entpacken
//     BYTEGLEICH.
//   * `tests/brotli.rs::alle_fuenf_vektoren_entpacken_byteweise_richtig` —
//     fuenf Vektoren aus `mutation_md/tls/brotli_testdaten.py`, erzeugt
//     mit `python3 -c "import brotli"` 1.1.0. Das ist ein ORAKEL, das
//     nicht aus diesem Workspace stammt.
//   * `bombe_endet_an_der_grenze_und_schnell` — die Grenze greift.
//
// Das Modul war 01.10./02.10. rund 40 Stunden auskommentiert, weil es
// 13 Uebersetzungsfehler hatte und damit narsil-net, narsil und PROBE
// blockierte. Die Regel dahinter war richtig — die Bremse hat die
// anderen drei Rollen wieder laufen lassen, statt ihnen einen Decoder zu
// geben, der Muell liefert. Nur: die Bremse musste weg, sobald der
// Decoder steht. Sie ist weg.
//
// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// ACHTUNG, STAND 30.09.2026 16:40 UTC: DER DEKODER IST **NOCH NICHT FERTIG**.
//
// Die Abnahme der Koordination (30.09.2026, gegen die Dateien aus
// `mutation_md/tls/koord/`) ist **ABGELEHNT**. Was jetzt stimmt und was
// nicht, mit dem Messwert daneben.
//
// -----------------------------------------------------------------------------
// BEHOBEN UND BELEGT (der Test `der_meta_block_header_stimmt_zur_dateigroesse`
// ist gruen, `b3_unkomprimiert_entpackt_byteweise` ist gruen)
// -----------------------------------------------------------------------------
//   * WBITS: die sieben Bit. Vorher `MAX_WBITS_BIT = 4` — der 7-Bit-Fall
//     (WBITS 10 bis 17, Muster `1000010` bis `1000000`) fehlte ganz.
//   * NDIRECT: **4 Bit, dann linksschieben um NPOSTFIX**. Vorher las ich
//     4 Bit plus drei erfundene Auswahl-Bits mit `4 << 4`, `4 << 5`,
//     `4 << 6`. Das war die Koordinationsmessung "NDIRECT ist 192,
//     erlaubt sind 0 bis 120" an b1.
//   * ISUNCOMPRESSED nur, wenn ISLAST == 0. Vorher immer gelesen —
//     das war "unkomprimierter Meta-Block will 190916 Byte" an b2.
//   * NBLTYPES und NTREESL/NTREESD: der variable Code braucht eine
//     **Mindestlaenge**. `0001` hat den Zahlenwert 1, und ein einzelnes
//     gelesenes Bit `1` ergibt denselben Wert; ohne die Mindestlaenge
//     verbrauchte der Decoder ein Bit statt vier.
//   * Die Kraftformel: `32 >> Laenge` fuer den Code-Laengen-Code und
//     `32768 >> Laenge` fuer die eigentlichen Codelaengen (RFC 7932 3.5).
//
// -----------------------------------------------------------------------------
// DIE LETZTE FEHLERSTELLE, Stand 30.09.2026 16:55 UTC
// -----------------------------------------------------------------------------
// Nach den Korrekturen an den Bitlaengen (unten) liest der Decoder bei
// `b1.br` den kompletten Meta-Block-Header und alle Huffman-Codes und
// scheitert erst in der **Dekodierung**:
//
//     b1: kein gueltiges Huffman-Symbol in 15 Bit
//     b2: der Code-Laengen-Code hat Kraft 16, erwartet 32 oder 1
//     b4: die Huffman-Code ist ueberzaehlt (mehr Symbole als Platz)
//
// Das heisst: Header und Codelaengen werden gelesen, und der Fehler
// ist in der **Kanonisierung oder in der Kraftpruefung**.
//
// -----------------------------------------------------------------------------
// DIE SIEBEN BIT-VERSATZ-FEHLER, ALLE BEI „VARIABLE CODES" — und sie
// sind eine Familie
// -----------------------------------------------------------------------------
// Es gibt in RFC 7932 genau **drei** Codes mit fester Laenge und
// variabler Zahl: NBLTYPES (9.2), NTREESL/NTREESD (9.2) und RLEMAX
// (7.3). Bei allen dreien habe ich zuerst eine **feste** Bitbreite
// angenommen. Das war jedes Mal falsch, und jedes Mal kostete es ein
// oder mehrere Bit — mit der Folge, dass der Fehler vier bis sechs
// Felder weiter auftaucht als seine Ursache.
//
// DIE REGEL, DIE GILT (aus der Tabelle in 9.2, addiert):
//
// ```text
//    Value    Bit Pattern    x    feste    gesamt
//       1            0        0       1        1
//       2         0001        0       4        4
//    3..4       x0011        1       4        5
//    5..8      xx0101        2       4        6
//    9..16    xxx0111        3       4        7
//   17..32   xxxx1001        4       4        8
//   33..64  xxxxx1011        5       4        9
//   65..128 xxxxxx1101       6       4       10
//  129..256 xxxxxxx1111      7       4       11
// ```
//
// ZWEI FOLGERUNGEN, die man leicht uebersieht:
//   1. Die Gesamtzahl der Bit ist **x + 4**, nicht x + 3. `x0011`
//      braucht fuenf Bit, nicht vier. Ich hatte vier gesetzt und damit
//      bei jedem Feld ein Bit verloren.
//   2. `0001` hat als Zahlenwert 1, und ein einzelnes gelesenes Bit
//      `1` ergibt ebenfalls 1. Ohne eine **Mindestlaenge** im Vergleich
//      wird nach einem Bit abgebrochen. Das ist die Ursache des
//      Fehlers, der sich als „NBLTYPES: kein gueltiges Muster
//      (101110101)" zeigte.
//
// DER AKKUMULATOR: das erste gelesene Bit kommt in Bit 0. Die vier
// festen Bit der Muster stehen damit in den **niedrigen** Bit des
// Akkumulators, die `x` in den hohen.
//
// -----------------------------------------------------------------------------
// DIE ZWEI OFFENEN PUNKTE
// -----------------------------------------------------------------------------
// 1. `b2: Kraft 16 statt 32` — der Code-Laengen-Code eines echten
//    Stroms. Die Kraft ist `32 >> Laenge` ueber alle **von null
//    verschiedenen** Laengen. 16 ist die Haelfte; es fehlt entweder
//    eine Laenge der Groesse 1, oder eine gelesene Laenge ist zu kurz.
//    DER WEG: alle gelesenen Laengen ausgeben (Symbol, Laenge, Kraft
//    laufend) und mit der Kraft 32 vergleichen.
//
// 2. `b1: kein gueltiges Huffman-Symbol in 15 Bit` — die Codelaengen
//    wurden gelesen und die Kraft stimmte, aber die Dekodierung findet
//    kein Symbol. DER WEG: die Startcodes (`starts`) und die
//    kanonische Symbolreihenfolge ausgeben und mit der Kraft vergleichen.
//    Kraft 32 mit 15 Bit bedeutet: alle Symbole liegen in den unteren
//    15 Bit, und ein gueltiger Code muss immer im Bereich
//    `starts[Laenge] .. starts[Laenge] + counts[Laenge]` liegen.
//
// EIN HINWEIS, DER IN DER LETZTEN FASSUNG NOCH FEHLT: die
// Kanonisierung in `HuffmanCode::aus_laengen` sortiert nach
// (Laenge, Symbol). Das ist richtig fuer **komprimierte** Codes. Fuer
// die **einfachen** Codes aus Abschnitt 3.4 gilt aber ausdruecklich
// "the NSYM symbols may not be presented in sorted order" und
// "Prefix codes of the same bit length must be assigned to the symbols
// in sorted order" — die Zuordnung Code zu Symbol ist dort eine andere.
// ---------------------------------------------------------------------------
//
// ---------------------------------------------------------------------------
// ACHTUNG, STAND 30.09.2026 16:40 UTC: DER DEKODER IST **NOCH NICHT FERTIG**.
//
// Die Abnahme der Koordination (30.09.2026, gegen die Dateien aus
// `mutation_md/tls/koord/`) ist **ABGELEHNT**. Was jetzt stimmt und was
// nicht, mit dem Messwert daneben.
//
// -----------------------------------------------------------------------------
// BEHOBEN UND BELEGT (der Test `der_meta_block_header_stimmt_zur_dateigroesse`
// ist gruen, `b3_unkomprimiert_entpackt_byteweise` ist gruen)
// -----------------------------------------------------------------------------
//   * WBITS: die sieben Bit. Vorher `MAX_WBITS_BIT = 4` — der 7-Bit-Fall
//     (WBITS 10 bis 17, Muster `1000010` bis `1000000`) fehlte ganz.
//   * NDIRECT: **4 Bit, dann linksschieben um NPOSTFIX**. Vorher las ich
//     4 Bit plus drei erfundene Auswahl-Bits mit `4 << 4`, `4 << 5`,
//     `4 << 6`. Das war die Koordinationsmessung "NDIRECT ist 192,
//     erlaubt sind 0 bis 120" an b1.
//   * ISUNCOMPRESSED nur, wenn ISLAST == 0. Vorher immer gelesen —
//     das war "unkomprimierter Meta-Block will 190916 Byte" an b2.
//   * NBLTYPES und NTREESL/NTREESD: der variable Code braucht eine
//     **Mindestlaenge**. `0001` hat den Zahlenwert 1, und ein einzelnes
//     gelesenes Bit `1` ergibt denselben Wert; ohne die Mindestlaenge
//     verbrauchte der Decoder ein Bit statt vier.
//   * Die Kraftformel: `32 >> Laenge` fuer den Code-Laengen-Code und
//     `32768 >> Laenge` fuer die eigentlichen Codelaengen (RFC 7932 3.5).
//
// -----------------------------------------------------------------------------
// DIE LETZTE FEHLERSTELLE, bis auf zwei Felder eingegrenzt
// -----------------------------------------------------------------------------
// Nach den obigen Korrekturen liest der Decoder bei `b1.br` bis
// Byte 4 Bit 3 = Bit 35 und meldet dann:
//
//     NBLTYPES: kein gueltiges Muster (101110101)
//
// Der vierte `lies_nbltypes`-Aufruf ist **NTREESL** (die ersten drei
// sind NBLTYPESL/I/D, je 1 Bit: alle drei sind 1). Der Aufrufer
// `lies_bbaum_anzahl` gibt NBLTYPES weiter — laut Abschnitt 9.2 ist das
// richtig ("encoded with the same variable-length code as NBLTYPESL").
//
// WAS BIS BIT 27 STIMMT, nachgerechnet:
//   WBITS 23 (4 Bit) | ISLAST 1 | ISLASTEMPTY 0 | MNIBBLES-Muster 00
//   -> 4 Nibbles | MLEN 43960 (das ist exakt die Groesse von b1.raw)
//   | NBLTYPES 1/1/1 (Bit 24, 25, 26)
//
// WAS ZWISCHEN BIT 27 UND 35 UNKLAR IST, und das ist der offene Punkt:
//   RFC 7932 Abschnitt 9.2 nennt die Reihenfolge
//     NPOSTFIX (2 Bit), NDIRECT (4 Bit), NBLTYPESL * 2 Bit Kontextmodi,
//     dann NTREESL.
//   Das ergibt fuer b1: NPOSTFIX (27, 28) = 0, NDIRECT (29..32) = 1,
//   Kontextmodi (33, 34), NTREESL ab 35 — und dort gibt es **kein**
//   gueltiges Muster.
//
// ZWEI MOEGLICHKEITEN, und beide sind noch offen:
//   a) Ein Feld davor wird falsch gelesen. Der Nachrechner findet bei
//      p = 33 ein gueltiges NTREESL = 1, aber das ist ein Zufall: die
//      Bits 33/34 sind als Kontextmodi (Wert 2) gelesen.
//   b) `NBLTYPESL * 2 bits` ist nicht die Anzahl der Kontextmodi, oder
//      die Kontextmodi stehen an einer anderen Stelle. Fuer b1 waeren
//      es 0 Bit, und dann laege NTREESL bei 33 = 1 — und der
//      nachfolgende NTREESD waere der naechste Verdacht.
//
// DER WEG ZUR KLAERUNG, der nicht Raten ist: `python3 -c "import brotli"`
// und `brotli.decompress` mit einem **abgeschnittenen** Strom. Konkret:
// in `lies_meta_block_daten` jeden Feldwert ausgeben (NPOSTFIX, NDIRECT,
// cmode.len(), ntreesl) und die Reihenfolge gegen einen Strom stellen,
// bei dem NBLTYPESL > 1 ist — dann sind die Kontextmodi unmistakable
// und ihre Position geht aus der Laenge hervor. `b2.br` hat NBLTYPESL = 2
// (nachgerechnet: Muster `0001` an Bit 28) und eignet sich dafuer.
//
// EIN ZWEITER, KURZER WEG: die Referenzimplementierung des
// google/brotli-Decoders in C lesen. `decode.c`, Funktion
// `BrotliDecoderDecompressStream`, ab der Zeile mit "case BROTLI_STATE_" —
// dort stehen die Felder in genau der Reihenfolge, die gesucht wird, und
// ohne Umweg.
// ---------------------------------------------------------------------------
//
// ---------------------------------------------------------------------------
// ACHTUNG, STAND 30.09.2026: DER DEKODER IST **NOCH NICHT FERTIG**.
//
// Was geht (gemessen, `werkzeuge/bau.sh narsil-compress test`):
//   * der leere Strom (1 Byte, ISLAST + ISLASTEMPTY)
//   * unkomprimierte Meta-Bloecke (200 Byte ueber `8b 63 80 00 …`)
//   * alle Grenzen (Ausgabe, Meta-Blockzahl, Befehlszahl je Block)
//   * 11 Modultests gruen, clippy 0 eigene Warnungen, fmt gruen
//
// Was NICHT geht: der komprimierte Weg. Der Testvektor `worte`
// (96 Byte Strom fuer 732 Byte Text, `testdaten/brotli_vektoren.txt`)
// bricht ab mit "die Huffman-Code ist ueberzaehlt".
//
// ---------------------------------------------------------------------------
// DIE FEHLERSTELLE, bis auf einen Schritt genau. Wer weitermacht, soll
// nicht wieder von vorn suchen.
//
// Der Decoder liest bis hierher **richtig** (jede Zeile ist gegen den
// RFC geprueft, die Werte mit einem Python-Nachrechner bestaetigt):
//   WBITS 23 -> ISLAST 1, ISLASTEMPTY 0, MNIBBLES-Muster 00 (4 Nibbles)
//   -> MLEN 732 -> ISUNCOMPRESSED 0
//   -> NBLTYPES 1/1/1 (kein Blocktypwechsel, drei Ein-Bit-Muster)
//   -> NPOSTFIX 0, NDIRECT 0 -> CMODE 0
//   -> NTREESL 1, NTREESD 1 (je ein Null-Bit) -> also KEINE
//      Kontextkarten, weil "if NTREESL >= 2" nicht gilt
//   -> Literal-Code[0]: HSKIP 2 -> 8 Code-Laengen-Symbole der Laenge 3,
//      Kraft 32  ✓
//
// Der Fehler liegt in der **Folge der 256 Literal-Codelaengen**
// (`lies_komplexe_code`, der 0..=15- und der 16-Zweig). Nachgerechnet
// mit dem Nachrechner aus `mutation_md/tls/`:
//   die Folge liefert Kraft **43456** statt 32768 (Nachrechner-Schritt 31)
//   und bleibt bei pos=61 von 256 stehen.
//
// WAS BISHER BEHOBEN WURDE und belegt ist (siehe die Vermerke in
// `brotli/decode.rs`, jeweils mit dem Fehler, den sie verhindern):
//   * NCODESELF wird fuer die Literal-/Distanz-/IaC-Code **nicht**
//     gelesen (Abschnitt 3.5 hat kein solches Feld; es steht nur in 9.3
//     fuer die Kontextkarten-Code)
//   * die Kraft wird **pro eingetragenem Symbol** gezaehlt, nicht pro
//     gelesenem Code-Laengen-Symbol
//   * die 16er-/17er-Wiederholung hat die Erweiterungsregel
//     `4 * (vorher - 2) + neu` bzw. `8 * (vorher - 2) + neu`
//   * ein 16er wird auf die Kraftgrenze gekuerzt
//
// WAS AUFTRIFT, OBWOHL ES IM RFC STEHT UND TROTZDEM FALSCH WIRKT:
//   Abschnitt 3.5 sagt "the sum of (32768 >> code length) over all the
//   non-zero code lengths ... must be equal to 32768" — die Kraft muss
//   **exakt** 32768 sein. Die wiederholte Laenge traegt aber 2^(15-l)
//   bei, und bei l=5 sind das 1024; die Kraft erreicht 32768 also nur,
//   wenn die Folge aufgeht. Der Nachrechner zeigt, dass sie mit
//   43456 **darueber** springt — die 16er-Wiederholung wurde also mit
//   der falschen Zahl gestartet, oder die Erweiterungsregel greift ein
//   Mal zu viel.
//
// DER NAECHSTE SCHRITT, der nicht Raten ist: im 16er-Zweig die Zahl
// der Wiederholungen ausgeben (pos, kraft vor und nach der
// Wiederholung, `neu`, `wiederholt`, `vorher_nicht_null`) und mit dem
// RFC-Beispiel `7, 16(+2 bits 11), 16(+2 bits 10) -> 22` vergleichen.
// Der Test `hrr` in `tests/brotli.rs` ist dafuer **noch nicht** da und
// sollte zuerst geschrieben werden: 7, 16(+2), 16(+2) muss 22 Laengen
// der Groesse 7 ergeben.
// ---------------------------------------------------------------------------
pub mod brotli;
mod deflate;
mod fehler;
mod gzip;
pub mod inflate;
// added 30.09.2026 bei der Uebernahme von `narsil-compress`: das Modul
// war `mod` (privat), obwohl `crc32_zwei`, `crc32_schritt`,
// `crc32_begin` und `adler32_schritt` echte, geprueffte
// Pruefsummen sind — und `narsil-image/src/png/{lesen,schreiben}.rs`
// benutzt **eine eigene Kopie** davon, weil es nicht auf dieses Modul
// zugreifen konnte.
//
// Zwei Kopien derselben Tabelle sind ein Fehler, der sich nicht
// bemerkbar macht, sondern nur im Byte-Vergleich: die eine ist an
// einer Stelle kaputt und die andere nicht.
//
// Deshalb `pub mod` und ein `pub use` mit sprechenden Namen. Das ist
// die kleinste Aenderung, die `narsil-image` das Verdrahten ueberhaupt
// erst erlaubt — die Umstellung selbst gehoert zur Rolle BILD und
// wird hier nicht gemacht.
pub mod pruefsummen;
mod zlib;

pub use fehler::FehlerZlib;
pub use gzip::{gunzip as gzip_entpacken, gzip as gzip_komprimiere};
pub use inflate::{deflate_entpacke, deflate_entpacke_laenge, entpacke, entpacke_in};
pub use zlib::entpacke_in as zlib_entpacke_in;
// added 30.09.2026: siehe den Vermerk an `pub mod pruefsummen`.
pub use pruefsummen::{adler32, adler32_schritt, crc32, crc32_begin, crc32_schritt, crc32_zwei};
/// Standard-Grenze fuer Entpacken: 64 MiB.
pub const STANDARD_GRENZE: usize = 64 * 1024 * 1024;

/// Entpackt einen zlib-Strom (Kopf + DEFLATE-Daten + Adler32).
///
/// `grenze` ist die HARTE Obergrenze der Ausgabe in Byte. Standard: 64 MiB.
pub fn inflate(daten: &[u8], grenze: usize) -> Result<Vec<u8>, crate::FehlerZlib> {
    inflate::entpacke(daten, grenze)
}

/// Entpackt einen gzip-Strom in einen Puffer (mit Grenze).
pub fn gunzip(daten: &[u8], out: &mut Vec<u8>, grenze: usize) -> Result<(), crate::FehlerZlib> {
    gzip::gunzip(daten, out, grenze)
}

/// Entpackt einen zlib-Strom in einen Puffer (mit Grenze).
pub fn zlib_entpacken(
    daten: &[u8],
    out: &mut Vec<u8>,
    grenze: usize,
) -> Result<(), crate::FehlerZlib> {
    zlib::entpacke_in(daten, out, grenze)
}

/// Komprimiert mit DEFLATE (ohne zlib-Rahmen).
pub fn deflate(daten: &[u8]) -> Vec<u8> {
    deflate::komprimiere(daten)
}

/// Entpackt einen Brotli-Strom (RFC 7932).
///
/// `grenze` ist die HARTE Obergrenze der Ausgabe in Byte.
///
/// added 30.09.2026: die Funktion ruft `brotli::decode::entpacke` auf.
/// Der Auftrag der Rolle KOMPRESS lautete genau diese Signatur, und
/// sie ist unveraendert geblieben — `narsil-net` ruft sie bereits so auf.
///
/// added 02.10.2026: der alte Vermerk hier sagte „der komprimierte Pfad
/// ist noch nicht fertig". Das war FALSCH und ist durch
/// `tests/brotli_echt.rs::alle_referenzdateien_bytegleich` widerlegt
/// (die RFC-Vektoren entpacken bytegleich). Ein Kommentar, der einen
/// Stand behauptet, den der Code nicht mehr hat, ist schlimmer als
/// keiner: wer ihn liest, baut die Funktion nicht mehr fertig.
pub fn brotli_entpacken(
    daten: &[u8],
    out: &mut Vec<u8>,
    grenze: usize,
) -> Result<(), crate::FehlerZlib> {
    brotli::decode::entpacke(daten, out, grenze)
}

/// Die Standardgrenze fuer Brotli: 16 MiB.
///
/// DER WERT gegenueber den 64 MiB der uebrigen Verfahren: die Grenze
/// fasst den **entpackten** Wert, nicht den komprimierten. 16 MiB
/// entpacktes Brotli ist fuer eine Webseite bereits absurd gross;
/// wer eine groessere Datei erwartet, ruft `brotli_entpacken` mit
/// einer eigenen Grenze.
pub const STANDARD_GRENZE_BROTLI: usize = 16 * 1024 * 1024;
