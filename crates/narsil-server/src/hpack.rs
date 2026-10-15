//! HPACK — der Kopfdaten-Decoder des HTTP/2-Servers (RFC 7541).
//!
//! added 01.10.2026, SERVER V2 (S7: "HPACK-Dekoder mit Tabellengrenze
//! (narsil-net/src/hpack.rs nur LESEN)"). Der Decoder hier ist **neu**
//! geschrieben und nicht von dort importiert: der Server brauch die
//! Gegenrichtung (er dekodiert die Anfrage des Clients und kodiert die
//! eigene Antwort), und `narsil-net` ist keine Abhaengigkeit von
//! `narsil-server` — das soll auch so bleiben.
//!
//! DIE STATISCHE TABELLE ist eine Kopie der Tabelle in
//! `narsil-net/src/hpack.rs` (gelesen, nicht veraendert). Die
//! Huffman-Tabelle wird zur Compile-Zeit aus
//! [`HUFFMAN_QUELLE`] gebaut und dabei **zweifach** geprueft: 257
//! Symbole mit Bitlaenge und die Kraft-Summe 2^30. Ein fehlendes Symbol
//! faellt damit beim **Bauen** auf, nicht beim Dekodieren.
//!
//! SICHERHEIT (M3/M4): jedes Byte kommt vom Client. Es gibt eine
//! Obergrenze fuer die Groesse eines Kopfdatenblocks, fuer die
//! dynamische Tabelle, fuer die Zahl ihrer Eintraege und fuer die
//! Laenge eines Namens oder Wertes. Jede Grenze ist eine benannte
//! Konstante mit Begruendung.

use std::collections::VecDeque;
use std::fmt;

/// M4: hoechstens so viele Oktette fuer eine als Zahl kodierte Laenge.
///
/// DER WERT 8: RFC 7541 Abschnitt 5.1 erlaubt beliebig lange Zahlen, und
/// `ff ff ff ...` ohne Abschlussbit waere eine Endlosschleife. 2^62 ist
/// die groesste Zahl, die noch sinnvoll ist (ein Index in eine Tabelle
/// mit ein paar Millionen Eintraegen).
pub const MAX_ZAHL_BYTES: usize = 8;

/// M4: hoechstens so viele Byte fuer einen Namen oder einen Wert.
///
/// DER WERT 32767: das ist die Groesse, ab der ein Kopfdatenblock im
/// Speicher unangenehm wird, und mehr als 32 KiB hat in keiner
/// brauchbaren Anfrage Platz (RFC 9113 Abschnitt 8.2.1 verlangt ohnehin,
/// dass die Implementierung die Groesse begrenzt).
pub const MAX_STRING_BYTES: usize = 32767;

/// M4: hoechstens so viele Byte in der dynamischen Tabelle.
///
/// DER WERT 4096: der Vorgabewert aus RFC 9113 Abschnitt 6.5.2
/// (`SETTINGS_HEADER_TABLE_SIZE`), und zugleich die Groesse, die die
/// geprueften Server melden. Der Client darf per SETTINGS **weniger**
/// verlangen; mehr als das hier anzubieten, waere eine Einladung, den
/// Speicher des Servers mit 16-Byte-Zeilen zu fuellen.
pub const MAX_DYNAMISCHE_TABELLE: usize = 4096;

/// M4: hoechstens so viele Eintraege in der dynamischen Tabelle.
///
/// DER WERT 128: bei `MAX_DYNAMISCHE_TABELLE / 32` Byte je Eintrag
/// (32 = laengster Name plus Wert plus 32 Byte Overhead nach
/// RFC 7541 4.1) sind das 128 Eintraege. Die Grenze ist **zusaetzlich**
/// noetig, weil 128 Eintraege mit je 1 Byte Namen in der Bytegrenze
/// passen, aber 128 `String`-Paare kosten.
pub const MAX_DYNAMISCHE_EINTRAEGE: usize = 128;

/// M4: hoechstens so viele Byte fuer einen ganzen Kopfdatenblock.
///
/// DER WERT 16384: das ist exakt `SETTINGS_MAX_FRAME_SIZE` beim Vorgabe-
/// wert (2^14) und damit die Groesse, die ein Client ohne
/// `SETTINGS_MAX_FRAME_SIZE`-Sonderwunsch in **einem** HEADERS-Rahmen
/// schicken kann. Wer mehr braucht, muss CONTINUATION schicken — und
/// das ist getrennt begrenzt ([`crate::h2::MAX_HEADER_BYTES`]), weil
/// ein einzelner Block zu gross und eine Kette von Continuation zu
/// langsam beides eigene Angriffe sind.
pub const MAX_HEADER_BYTES: usize = 16 * 1024;

/// M3: hoechstens so viele `Dynamic Table Size Update` am Blockanfang.
///
/// DER WERT 2: RFC 7541 6.3 nennt "a maximum of two" fuer einen
/// geaenderten Wert in einem Block. Mehr davon ist kein Client, sondern
/// eine Schleife, die den Dekoder arbeiten laesst.
pub const MAX_GROESSENAENDERUNGEN: usize = 2;

/// M3: hoechstens so viele Kopfdatenzeilen je Block.
///
/// DER WERT 128: `MAX_HEADER_ANZAHL` des HTTP/1.1-Pfades ist 200; hier
/// sind 128 genug (Browser schicken selten mehr als 20) und die
/// Bytegrenz�� schuetzt schon. Die Zeilenzahl schuetzt davor, dass
/// 8.000 Ein-Byte-Zeilen in einem Block stehen und die `HashMap` des
/// Handlers zerlegen.
pub const MAX_HEADER_ANZAHL: usize = 128;

/// Fehler aus dem HPACK-Decoder.
///
/// Jeder Fehler nennt, **was** schiefging — ein Mitschnitt gibt damit
/// dieselbe Auskunft wie die Meldung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HpackFehler {
    /// Die Zahl am Feldanfang hatte mehr als [`MAX_ZAHL_BYTES`]
    /// Oktette oder lief ohne Abschlussbit ins Leere.
    ZahlZuLang { kam: usize },
    /// Ein als String kodiertes Feld ist laenger als
    /// [`MAX_STRING_BYTES`].
    StringZuLang { war: usize },
    /// Der Huffman-Bitstrom endete mitten in einem Code.
    HuffmanUnvollstaendig,
    /// Der Huffman-Code enthaelt ein Symbol, das es nicht gibt.
    HuffmanUnbekannt { code: u32 },
    /// Der Huffman-Code enthaelt das EOS-Symbol (256). RFC 7541 5.2:
    /// "MUST be treated as a decoding error".
    HuffmanEos,
    /// Der Index einer Kopfdatenzeile steht in keiner Tabelle.
    IndexUngueltig { war: usize },
    /// Die dynamische Tabelle waechst ueber ihre Grenze hinaus.
    TabelleZuGross { war: usize },
    /// Der Block ist laenger als [`MAX_HEADER_BYTES`].
    BlockZuLang { war: usize },
    /// Der Block hat mehr als [`MAX_HEADER_ANZAHL`] Zeilen.
    ZuVieleZeilen { war: usize },
    /// Eine Aenderung der Tabellengroesse steht nicht am Anfang des
    /// Blocks (RFC 7541 6.3).
    GroessenaenderungFalscherOrt,
    /// Ein Name oder Wert ist kein gueltiges UTF-8.
    KeinUtf8 { war: usize },
}

impl fmt::Display for HpackFehler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HpackFehler::ZahlZuLang { kam } => write!(
                f,
                "die Zahl am Feldanfang hat {kam} Oktett, erlaubt sind {MAX_ZAHL_BYTES}"
            ),
            HpackFehler::StringZuLang { war } => write!(
                f,
                "ein String ist {war} Byte lang, erlaubt sind {MAX_STRING_BYTES}"
            ),
            HpackFehler::HuffmanUnvollstaendig => {
                write!(f, "der Huffman-Bitstrom endet mitten in einem Code")
            }
            HpackFehler::HuffmanUnbekannt { code } => {
                write!(f, "im Huffman-Code steht das unbekannte Symbol {code:#x}")
            }
            HpackFehler::HuffmanEos => write!(
                f,
                "der Huffman-Code enthaelt EOS (256), das nie vorkommen darf"
            ),
            HpackFehler::IndexUngueltig { war } => {
                write!(f, "der Kopfindex {war} steht in keiner Tabelle")
            }
            HpackFehler::TabelleZuGross { war } => write!(
                f,
                "die dynamische Tabelle waechst auf {war} Byte, erlaubt sind {MAX_DYNAMISCHE_TABELLE}"
            ),
            HpackFehler::BlockZuLang { war } => write!(
                f,
                "der Kopfdatenblock hat {war} Byte, erlaubt sind {MAX_HEADER_BYTES}"
            ),
            HpackFehler::ZuVieleZeilen { war } => write!(
                f,
                "der Block hat {war} Kopfzeilen, erlaubt sind {MAX_HEADER_ANZAHL}"
            ),
            HpackFehler::GroessenaenderungFalscherOrt => write!(
                f,
                "die Aenderung der Tabellengroesse steht nicht am Anfang des Blocks"
            ),
            HpackFehler::KeinUtf8 { war } => {
                write!(f, "die {war} Byte sind kein gueltiges UTF-8")
            }
        }
    }
}

/// Die statische Tabelle aus RFC 7541 Anhang A, 61 Eintraege.
///
/// Index 1 bis 61; Index 0 ist nach RFC 7541 6.1 ein **Fehler**, kein
/// leerer Eintrag. Deshalb ist die Tabelle hier `[Eintrag; 61]` und der
/// Index wird beim Nachsehen um eins erhoeht.
pub const STATISCHE_TABELLE: [(&str, &str); 61] = [
    (":authority", ""),
    (":method", "GET"),
    (":method", "POST"),
    (":path", "/"),
    (":path", "/index.html"),
    (":scheme", "http"),
    (":scheme", "https"),
    (":status", "200"),
    (":status", "204"),
    (":status", "206"),
    (":status", "304"),
    (":status", "400"),
    (":status", "404"),
    (":status", "500"),
    ("accept-charset", ""),
    ("accept-encoding", "gzip, deflate"),
    ("accept-language", ""),
    ("accept-ranges", ""),
    ("accept", ""),
    ("access-control-allow-origin", ""),
    ("age", ""),
    ("allow", ""),
    ("authorization", ""),
    ("cache-control", ""),
    ("content-disposition", ""),
    ("content-encoding", ""),
    ("content-language", ""),
    ("content-length", ""),
    ("content-location", ""),
    ("content-range", ""),
    ("content-type", ""),
    ("cookie", ""),
    ("date", ""),
    ("etag", ""),
    ("expect", ""),
    ("expires", ""),
    ("from", ""),
    ("host", ""),
    ("if-match", ""),
    ("if-modified-since", ""),
    ("if-none-match", ""),
    ("if-range", ""),
    ("if-unmodified-since", ""),
    ("last-modified", ""),
    ("link", ""),
    ("location", ""),
    ("max-forwards", ""),
    ("proxy-authenticate", ""),
    ("proxy-authorization", ""),
    ("range", ""),
    ("referer", ""),
    ("refresh", ""),
    ("retry-after", ""),
    ("server", ""),
    ("set-cookie", ""),
    ("strict-transport-security", ""),
    ("transfer-encoding", ""),
    ("user-agent", ""),
    ("vary", ""),
    ("via", ""),
    ("www-authenticate", ""),
];

/// Der Eintrag `index` der statischen Tabelle (1..=61), sonst `None`.
pub fn statischer_eintrag(index: usize) -> Option<(&'static str, &'static str)> {
    if index == 0 {
        return None;
    }
    STATISCHE_TABELLE.get(index - 1).copied()
}

/// Liest den Index einer **Indexed Header Field Representation**
/// (RFC 7541 6.1).
///
/// DER GRUND FUER EIGENE FUNKTION: dort ist der Index ein
/// 7-Bit-Praefix **ohne Fortsetzungsbit** — 0x82 heisst Index 2 und
/// nicht "eine Zahl, die noch weitere Oktett hat". `lies_zahl` zu
/// benutzen war der erste Fehler dieses Decoders: 0x82 wurde als
/// `2 + 0x02 << 7` gelesen und der Index war 258.
pub fn lies_index7(b: &[u8]) -> Result<(usize, usize), HpackFehler> {
    if b.is_empty() {
        return Err(HpackFehler::ZahlZuLang { kam: 0 });
    }
    let index = (b[0] & 0x7f) as usize;
    if index >= 127 {
        // 127 ist der Maximalwert des 7-Bit-Praefixes und damit nach
        // 5.1 der Wert "es kommen weitere Oktette". Fuer die
        // **indexed** Form (6.1) gibt es die aber nicht: dort ist der
        // Index ein reiner 7-Bit-Wert. 127 heisst darum: kein Index.
        return Err(HpackFehler::IndexUngueltig { war: index });
    }
    Ok((index, 1))
}

/// Liest eine `Dynamic Table Size Update` (RFC 7541 6.3).
///
/// DER WEG: 3-Bit-Praefix (`001xxxxx`) **mit** Fortsetzung, wie bei
/// `lies_zahl`, nur mit drei statt sieben Startbits.
pub fn lies_zahl5(b: &[u8]) -> Result<(usize, usize), HpackFehler> {
    lies_int(b, 5)
}

/// Der Huffman-Code aus RFC 7541 Anhang B: je Symbol `(wert, bitlaenge)`.
///
/// DER WEG ZU DIESER TABELLE: sie entsteht zur Compile-Zeit mit einem
/// `const fn` aus [`HUFFMAN_QUELLE`], und `build_huffman` bricht ab,
/// wenn die Quelle nicht lesbar ist (falsche Zeilenzahl, fehlende
/// Bitlaenge) oder wenn die Kraft-Summe nicht 2^30 ergibt. Der letzte
/// Punkt ist der eigentliche Beweis: ein vollstaendiger Code ueber 30 Bit
/// deckt genau den ganzen Baum ab — fehlt ein Symbol, stimmt die Summe
/// nicht mehr.
pub const HUFFMAN: [(u32, u8); 257] = build_huffman();

const fn build_huffman() -> [(u32, u8); 257] {
    let bytes = HUFFMAN_QUELLE.as_bytes();
    let mut tabelle = [(0u32, 0u8); 257];
    let mut gelesen = 0usize;
    let mut pos = 0usize;
    while pos < bytes.len() {
        let zeilen_start = pos;
        let mut ende = pos;
        while ende < bytes.len() && bytes[ende] != b'\n' {
            ende += 1;
        }
        pos = ende + 1;
        if ende == zeilen_start || bytes[zeilen_start] == b'#' {
            continue;
        }
        // Zahl (Symbol) lesen
        let mut wert = 0usize;
        let mut i = zeilen_start;
        while i < ende && bytes[i].is_ascii_digit() {
            wert = wert * 10 + (bytes[i] - b'0') as usize;
            i += 1;
        }
        while i < ende && bytes[i] == b' ' {
            i += 1;
        }
        // Hexwert (Code)
        let mut h = 0u32;
        let mut ziffern = 0;
        while i < ende {
            let c = bytes[i];
            let d = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                _ => break,
            };
            h = h * 16 + d as u32;
            ziffern += 1;
            i += 1;
        }
        while i < ende && bytes[i] == b' ' {
            i += 1;
        }
        // Bitlaenge
        let mut laenge = 0u8;
        while i < ende && bytes[i].is_ascii_digit() {
            laenge = laenge * 10 + (bytes[i] - b'0');
            i += 1;
        }
        if ziffern == 0 || wert > 256 {
            continue;
        }
        tabelle[wert] = (h, laenge);
        gelesen += 1;
    }
    // M3: wenn die eingebettete Quelle nicht lesbar waere, faellt es
    // hier beim Bauen auf, nicht spaeter beim Dekodieren. 257 Symbole
    // mit Laenge sind der Nachweis.
    assert!(gelesen == 257, "die Huffman-Quelle muss 257 Zeilen haben");
    let mut n = 0;
    while n < 257 {
        assert!(tabelle[n].1 > 0, "Symbol ohne Bitlaenge");
        assert!(tabelle[n].1 <= 30, "Bitlaenge groesser als 30");
        n += 1;
    }
    // Kraft-Kontrolle: ein vollstaendiger Code deckt 2^30 Bit ab.
    let mut kraft = 0u64;
    let mut k = 0;
    while k < 257 {
        kraft += 1u64 << (30 - tabelle[k].1 as u32);
        k += 1;
    }
    // `assert_eq!` ist in einer `const fn` nicht erlaubt (die Meldung
    // waere eine Formatierung). `panic!` mit einem festen Text ist es
    // seit Rust 1.57 — und genau die braucht man hier.
    if kraft != 1u64 << 30 {
        panic!("der Huffman-Code deckt nicht den ganzen Baum ab");
    }
    tabelle
}

/// Der Huffman-Code als Text, eine Zeile je Symbol: `symbol hex-code
/// bitlaenge`. Erzeugt von `mutation_md/server/huffman_erzeugen.py`.
const HUFFMAN_QUELLE: &str = include_str!("../testdaten/huffman_code.txt");

/// Liest eine Zahl im HPACK-Format (RFC 7541 5.1) und sagt, wie viele
/// Oktette sie kostete.
///
/// M4/M3: hoechstens [`MAX_ZAHL_BYTES`] Oktette. Ohne diese Grenze ist
/// `ff ff ff ...` eine Endlosschleife mit Speicherverbrauch.
pub fn lies_zahl(b: &[u8]) -> Result<(usize, usize), HpackFehler> {
    lies_int(b, 7)
}

/// Liest eine Zahl mit `bits` Bit Praefix (RFC 7541 5.1).
///
/// added 01.10.2026, SERVER V2. **DIE REGEL, und sie stand vorher an
/// drei Stellen falsch im File:** es gibt kein eigenes
/// Fortsetzungsbit. Fortgesetzt wird genau dann, wenn der Praefix den
/// **Maximalwert** `2^bits - 1` traegt (7 Bit -> 127, 5 Bit -> 31,
/// 4 Bit -> 15). Das war der Grund fuer drei Fehler am Stueck:
///
/// * `0x20` (Groessenaenderung auf 0) sah als "Fortsetzung" aus und
///   las das naechste Byte als Zahlenteil.
/// * `0xe4` (Stringlaenge 100 mit Huffman-Marke) sah als "Fortsetzung"
///   aus (`0xe4 & 0x40 != 0`) und las die Laenge als 100 + 7*next.
/// * `0x0f` (Index 15) sah als "Fortsetzung" aus und suchte ein Byte,
///   das es nicht gab.
///
/// added 01.10.2026, SERVER V2: `pub`, weil die Tests aus Anhang C der
/// Norm die drei Praefix-Formen **ueber diese eine** Funktion pruefen
/// wollen. Drei fast gleiche Leser im Test (einer je Form) waeren drei
/// Kopien, die auseinanderlaufen koennen — der Fehler, den dieser
/// Decoder dreimal gemacht hat.
pub fn lies_int(b: &[u8], bits: u32) -> Result<(usize, usize), HpackFehler> {
    if b.is_empty() {
        return Err(HpackFehler::ZahlZuLang { kam: 0 });
    }
    let max = (1u64 << bits) - 1;
    let start = (b[0] as u64) & max;
    if start < max {
        return Ok((start as usize, 1));
    }
    lese_fortsetzung(b, max)
}

/// Liest ein als String kodiertes Feld (RFC 7541 5.2) und sagt, wie
/// viele Oktette es kostete.
///
/// M4: die Laenge wird **vor** der Allokation geprueft, und die
/// Huffman-Ausgabe ist zusaetzlich auf [`MAX_STRING_BYTES`] begrenzt.
pub fn lies_string(b: &[u8]) -> Result<(String, usize), HpackFehler> {
    if b.is_empty() {
        return Ok((String::new(), 0));
    }
    let huffman_bit = b[0] & 0x80 != 0;
    // DIE LAENGE, und sie hat ein eigenes Praefix: nach RFC 7541 5.2 ist
    // das oberste Bit die Huffman-Marke und das **zweitoberste** (0x40)
    // das Fortsetzungsbit; der Startwert steht in den unteren sieben
    // Bit. `lies_zahl` zu nehmen (Fortsetzung bei 0x80) liest jedes
    // Huffman-Feld als Zahl `2^7 + Laenge` — bei `8c` (Huffman, 12 Byte)
    // waren das 140 Byte statt 12.
    let (laenge, verbraucht) = lies_laenge7(b)?;
    if laenge > MAX_STRING_BYTES {
        return Err(HpackFehler::StringZuLang { war: laenge });
    }
    if b.len() < verbraucht.saturating_add(laenge) {
        return Err(HpackFehler::StringZuLang { war: laenge });
    }
    let daten = &b[verbraucht..verbraucht + laenge];
    let out = if huffman_bit {
        huffman_dekodiere(daten)?
    } else {
        // RFC 7541 8.1.2: Oktette, die kein UTF-8 sind, sind ein Fehler.
        String::from_utf8(daten.to_vec()).map_err(|_| HpackFehler::KeinUtf8 { war: laenge })?
    };
    Ok((out, verbraucht + laenge))
}

/// Liest die Laenge eines als String kodierten Feldes (RFC 7541 5.2).
///
/// DER WEG: 7 Startbit im Byte, Fortsetzungsbit ist `0x40` (nicht
/// `0x80` — das ist die Huffman-Marke). Eigene Funktion, weil die
/// "7 Bit, aber Fortsetzung bei 0x40" sonst niemand auf der Hand hat und
/// der Fehler beim Lesen **nicht** auffaellt: 0x8c wird zu 140.
pub fn lies_laenge7(b: &[u8]) -> Result<(usize, usize), HpackFehler> {
    // 7 Startbit, Fortsetzung bei 127 (5.1). Das oberste Bit (0x80) ist
    // die Huffman-Marke und gehoert **nicht** zur Zahl.
    if b.is_empty() {
        return Err(HpackFehler::ZahlZuLang { kam: 0 });
    }
    let start = (b[0] & 0x7f) as u64;
    if start < 127 {
        return Ok((start as usize, 1));
    }
    lese_fortsetzung(b, 127)
}

/// Dekodiert einen Huffman-Bitstrom (RFC 7541 5.2) ueber einen Baum.
///
/// DER BAUM, und warum er bei jedem Aufruf neu entsteht: es gibt keinen
/// Sprung, der bitweise sagt, wohin es weitergeht — der Code ist
/// praefixfrei, aber nicht bitweise zu lesen. Der Baum wird aus
/// [`HUFFMAN`] gebaut: 2*257 Knoten reichen, weil ein praefixfreier Code
/// mit 257 Blaettern hoechstens 256 innere Knoten hat. Ein dauerhaft
/// gehaltener Baum waere eine zweite Wahrheit neben der Tabelle, die
/// auseinanderlaufen koennte.
///
/// M3/M4: der Baum hat hoechstens 30 Ebenen (das EOS-Symbol ist 30 Bit
/// lang) und die Ausgabe ist auf [`MAX_STRING_BYTES`] begrenzt.
pub fn huffman_dekodiere(daten: &[u8]) -> Result<String, HpackFehler> {
    // M3: Knoten-Speicher, `None` = Blatt. Feste Groesse, keine
    // Allokation aus der Eingabe.
    // DIE ZWEI ARRAYS, und der Grund fuer zwei statt einem Paar:
    // Knoten `n` besitzt die Kanten 2n (Bit 0) und 2n+1 (Bit 1). Der
    // erste Versuch hatte **einen** Paar-Speicher und legte den
    // Kindzeiger auf `kinder[knoten]`, las aber ueber `kinder[2n+bit]`
    // — der Faktor 2 fehlte, und der Baum lief bei Symbol 200 aus dem
    // Speicher (`index out of bounds: the len is 514 but the index is
    // 515`). Jetzt ist es ein Array, und die Rechnung steht genau einmal.
    const KANTEN: usize = 2 * 257;
    let mut kind: Vec<i16> = vec![-1; KANTEN];
    let mut blatt: Vec<i16> = vec![-1; KANTEN];
    let mut knoten = 1usize;
    // added 05.10.2026 (SCHLUSSPHASE, clippy `needless_range_loop`):
    // statt ueber den Index zu laufen, wird der **Wert** aus
    // `HUFFMAN` geholt und mitgezählt. Verhalten unveraendert — der
    // Index wird weiter unten fuer `kind`/`blatt` gebraucht, aber
    // nicht als Schleifenvariable.
    for (sym, &(code, laenge)) in HUFFMAN.iter().enumerate().take(257) {
        if laenge == 0 {
            continue;
        }
        let mut hier = 0usize;
        let mut bit = laenge as usize;
        while bit > 0 {
            bit -= 1;
            let wert = ((code >> bit) & 1) as usize;
            let pos = hier * 2 + wert;
            if bit == 0 {
                blatt[pos] = sym as i16;
            } else {
                if kind[pos] < 0 {
                    // M3: die Knotenzahl ist durch die Tabelle begrenzt
                    // (ein praefixfreier Code mit 257 Blaettern hat
                    // hoechstens 256 innere Knoten). Die Pruefung ist da,
                    // damit ein Fehler hier ein **Fehler** ist und kein
                    // Speicherzugriff.
                    if knoten * 2 + 1 >= KANTEN {
                        return Err(HpackFehler::HuffmanUnbekannt { code });
                    }
                    kind[pos] = knoten as i16;
                    knoten += 1;
                }
                hier = kind[pos] as usize;
            }
        }
    }
    let mut aus: Vec<u8> = Vec::new();
    let mut hier = 0usize;
    // DIE FUELLBITS, und warum man sie zaehlen muss: RFC 7541 5.2 sagt,
    // das letzte Byte wird mit den **hoechstwertigen Bits von EOS**
    // (also lauter Einsen) aufgefuellt, und es duerfen hoechstens 7
    // sein. Ein Dekoder, der am Ende nur `hier != 0` prueft, weist damit
    // jede **korrekte** Nachricht ab — genau das hat der erste
    // Testlauf gezeigt (`f1e3 .. ff` aus Anhang C.2.1 ist gueltig und
    // wurde abgewiesen).
    let mut fuell_bits = 0u32;
    let mut fuell_alle_eins = true;
    // ACHTUNG, 8 BIT JE BYTE: die erste Fassung lief
    // `for wert in [byte >> 4, byte & 0x0f]` und hat damit **Halbbytes**
    // als Bit gewertet. Ein Halbbyte ist 0..15, der Index im Baum aber
    // nur 0 oder 1 — jedes Byte mit einem gesetzten oberen Halbbyte ging
    // deshalb in einen Ast, den es nicht gibt, und am Ende blieb ein
    // "unvollstaendiger Code" stehen. Der Testlauf hat es an
    // `0x1f` gezeigt: das erste Bit war 1 statt 0, weil `0x1f >> 4`
    // gleich 1 ist.
    for byte in daten {
        for stelle in 0..8usize {
            let wert = ((byte >> (7 - stelle)) & 1) as usize;
            let pos = hier * 2 + wert;
            if blatt[pos] >= 0 {
                let sym = blatt[pos] as usize;
                // RFC 7541 5.2: EOS im Strom ist ein Fehler.
                if sym == 256 {
                    return Err(HpackFehler::HuffmanEos);
                }
                aus.push(sym as u8);
                if aus.len() > MAX_STRING_BYTES {
                    return Err(HpackFehler::StringZuLang { war: aus.len() });
                }
                hier = 0;
                fuell_bits = 0;
                fuell_alle_eins = true;
            } else {
                fuell_bits += 1;
                if wert != 1 {
                    fuell_alle_eins = false;
                }
                let naechste = kind[pos];
                if naechste < 0 {
                    return Err(HpackFehler::HuffmanUnbekannt {
                        code: ((hier as u32) << 1) | wert as u32,
                    });
                }
                hier = naechste as usize;
            }
        }
    }
    // NACH DEM LETZTEN BYTE, und das ist die Regel (5.2): hoechstens 7
    // Fuellbits, und sie muessen alle Eins sein. Alles andere ist ein
    // abgebrochener Code.
    if fuell_bits > 7 {
        return Err(HpackFehler::HuffmanUnvollstaendig);
    }
    if !fuell_alle_eins {
        return Err(HpackFehler::HuffmanUnvollstaendig);
    }
    String::from_utf8(aus).map_err(|_| HpackFehler::HuffmanUnbekannt { code: 0 })
}

/// Der Decoder-Zustand: die dynamische Tabelle (RFC 7541 2.3.2).
///
/// Neuere Eintraege stehen **vorne** (Index 62, 63, ...). Beim
/// Einfuegen wird der Name kleingeschrieben — so steht es in der Norm
/// und so muss der Server nachsehen koennen.
#[derive(Debug, Clone)]
pub struct Dekoder {
    eintraege: VecDeque<(String, String)>,
    groesse_bytes: usize,
    /// Die Groesse, die der Client per SETTINGS verlangt hat.
    erlaubt: usize,
    /// Hat seit dem letzten Block eine Groessenaenderung stattgefunden?
    groesse_geaendert: bool,
}

impl Dekoder {
    /// Eine leere Tabelle mit der Vorgabe von 4096 Byte.
    pub fn neu() -> Self {
        Dekoder {
            eintraege: VecDeque::new(),
            groesse_bytes: 0,
            erlaubt: MAX_DYNAMISCHE_TABELLE,
            groesse_geaendert: false,
        }
    }

    /// Wie viele Byte die Tabelle belegt (RFC 7541 4.1:
    /// `Eintrag = Laenge(Name) + Laenge(Wert) + 32`).
    pub fn belegt(&self) -> usize {
        self.groesse_bytes
    }

    /// Wie viele Eintraege drin sind.
    pub fn anzahl(&self) -> usize {
        self.eintraege.len()
    }

    /// Der Eintrag an Position `index` (62 ist der neueste).
    pub fn hole(&self, index: usize) -> Option<(&str, &str)> {
        if index < 62 {
            return None;
        }
        self.eintraege
            .get(index - 62)
            .map(|(n, w)| (n.as_str(), w.as_str()))
    }

    /// Setzt die erlaubte Groesse (`SETTINGS_HEADER_TABLE_SIZE`).
    ///
    /// M4: Werte ueber [`MAX_DYNAMISCHE_TABELLE`] werden **abgewiesen**,
    /// nicht gekappt. Ein gekappter Wert waere eine stille
    /// Fehlinterpretation — der Client glaubte, 1 MiB seien erlaubt.
    pub fn setze_erlaubt(&mut self, bytes: usize) -> Result<(), HpackFehler> {
        if bytes > MAX_DYNAMISCHE_TABELLE {
            return Err(HpackFehler::TabelleZuGross { war: bytes });
        }
        self.erlaubt = bytes;
        self.groesse_geaendert = true;
        self.kuerze();
        Ok(())
    }

    /// Fuellt einen Wert an (Literal with incremental indexing).
    ///
    /// M3/M4: der Name ist auf [`MAX_STRING_BYTES`] begrenzt, die Zahl
    /// der Eintraege auf [`MAX_DYNAMISCHE_EINTRAEGE`], und die Tabelle
    /// wird nach jedem Einfuegen so gekuerzt, dass sie in `erlaubt`
    /// passt. Das ist **nicht** optional: 16 Zeilen mit je 20 Byte
    /// kosten weniger Bytes, als sie im Speicher brauchen.
    pub fn einfuegen(&mut self, name: &str, wert: &str) -> Result<(), HpackFehler> {
        if name.len() > MAX_STRING_BYTES || wert.len() > MAX_STRING_BYTES {
            return Err(HpackFehler::StringZuLang {
                war: name.len().max(wert.len()),
            });
        }
        // M3: ohne diese Grenze schickt ein Client 100.000 Eintraege mit
        // je 1 Byte Namen — das passt in die Bytegrenz, kostet aber
        // 100.000 Allokationen. Der aelteste fliegt raus, der Platz
        // bleibt fuer die Groesse der Tabelle frei.
        if self.eintraege.len() >= MAX_DYNAMISCHE_EINTRAEGE {
            if let Some((n, w)) = self.eintraege.pop_back() {
                self.groesse_bytes = self.groesse_bytes.saturating_sub(n.len() + w.len() + 32);
            }
        }
        // OHNE Addition vorher pruefen: `name.len() + wert.len() + 32`
        // koennte theoretisch ueberlaufen, und im Debug-Modus heisst
        // Ueberlauf PANIK (M8: nie eine).
        let kosten = name.len().saturating_add(wert.len()).saturating_add(32);
        if kosten > self.erlaubt {
            // Der Eintrag passt nicht einmal allein: nach RFC 7541 4.4
            // wird die Tabelle **geleert** und der Eintrag nicht
            // eingetragen. Das ist erlaubt und bindend.
            self.eintraege.clear();
            self.groesse_bytes = 0;
            return Ok(());
        }
        self.groesse_bytes = self.groesse_bytes.saturating_add(kosten);
        self.eintraege
            .push_front((name.to_string(), wert.to_string()));
        self.kuerze();
        Ok(())
    }

    /// Kuerzt am **Ende**, bis die Tabelle in ihr Limit passt
    /// (RFC 7541 4.2: "the entry at the end MUST be evicted").
    fn kuerze(&mut self) {
        while self.groesse_bytes > self.erlaubt {
            match self.eintraege.pop_back() {
                Some((n, w)) => {
                    self.groesse_bytes = self
                        .groesse_bytes
                        .saturating_sub(n.len().saturating_add(w.len()).saturating_add(32));
                }
                None => {
                    self.groesse_bytes = 0;
                    break;
                }
            }
        }
    }

    /// Merkt sich, dass eine Groessenaenderung noch am Anfang eines
    /// Blocks unterzubringen ist.
    pub fn groesse_geaendert(&self) -> bool {
        self.groesse_geaendert
    }

    /// Nach der Verarbeitung des Blocks: die Merkung loeschen.
    pub fn groesse_abgearbeitet(&mut self) {
        self.groesse_geaendert = false;
    }
}

/// Dekodiert einen ganzen Kopfdatenblock.
///
/// added 01.10.2026, SERVER V2. DIE GRUENZEN, alle benannt:
/// * [`MAX_HEADER_BYTES`] fuer den Block (M4),
/// * [`MAX_HEADER_ANZAHL`] fuer die Zeilenzahl (M3),
/// * der Dekoder selbst fuer Tabelle, Namen und Werte.
///
/// DIE REIHENFOLGE, und sie ist bindend (RFC 7541 6.3): eine
/// `Dynamic Table Size Update` steht **am Anfang** des Blocks, und
/// danach folgt die `Header Field Representation`. Wer sie vertauscht,
/// haelt die Tabelle des Clients und die des Servers auseinander — und
/// jede weitere Zeile waere dann ein Index in die falsche Tabelle.
pub fn dekodiere(
    block: &[u8],
    dekodierer: &mut Dekoder,
) -> Result<Vec<(String, String)>, HpackFehler> {
    if block.len() > MAX_HEADER_BYTES {
        return Err(HpackFehler::BlockZuLang { war: block.len() });
    }
    let mut zeilen: Vec<(String, String)> = Vec::new();
    let mut pos = 0usize;
    // 1) Groessenaenderungen, nur am Anfang.
    //
    // M3: hoechstens [`MAX_GROESSENAENDERUNGEN`] am Stueck. RFC 7541 6.3
    // erlaubt eine (bzw. zwei, fuer eine Reihenfolge von Aenderungen);
    // eine Kette von 20 ist ein Client, der den Dekoder arbeiten laesst,
    // und kostet sonst nur Rechenzeit.
    let mut groessenaenderungen = 0usize;
    while pos < block.len() && block[pos] & 0xe0 == 0x20 {
        groessenaenderungen += 1;
        if groessenaenderungen > MAX_GROESSENAENDERUNGEN {
            return Err(HpackFehler::ZuVieleZeilen { war: zeilen.len() });
        }
        let (wert, verbraucht) = lies_zahl5(&block[pos..])?;
        dekodierer.setze_erlaubt(wert)?;
        pos += verbraucht;
        dekodierer.groesse_abgearbeitet();
    }
    if dekodierer.groesse_geaendert() {
        // Der Dekoder traegt noch eine Aenderung aus einem **frueheren**
        // Block: sie kam nicht am Anfang dieses Blocks und ist damit
        // nach RFC 7541 6.3 ein Fehler. Ohne diese Pruefung wuerde der
        // Server die Groesse eines Blockes anwenden, den der Client
        // schon vergessen hat.
        return Err(HpackFehler::GroessenaenderungFalscherOrt);
    }
    // 2) Kopfzeilen.
    while pos < block.len() {
        if zeilen.len() >= MAX_HEADER_ANZAHL {
            return Err(HpackFehler::ZuVieleZeilen {
                war: zeilen.len() + 1,
            });
        }
        let b = block[pos];
        if b & 0x80 != 0 {
            // 6.1 Indexed Header Field: 7 Bit, **ohne** Fortsetzung.
            let (index, verbraucht) = lies_index7(&block[pos..])?;
            pos += verbraucht;
            let (name, wert) = hole(index, dekodierer)?;
            zeilen.push((name.to_string(), wert.to_string()));
        } else if b & 0xc0 == 0x40 {
            // 6.2.1 Literal with incremental indexing
            // DER VERSATZ, und er war der Fehler: `lies_name` sagt, wie
            // viele Byte es im **Slice** `&block[pos..]` gebraucht hat —
            // nicht, wo das Feld endet. Mit `pos = verbraucht` las der
            // Decoder ab Byte 1 weiter und produzierte Muell.
            let (name, verbraucht) = lies_name(&block[pos..], dekodierer)?;
            pos += verbraucht;
            let (wert, verbraucht_wert) = lies_string(&block[pos..])?;
            pos += verbraucht_wert;
            dekodierer.einfuegen(&name, &wert)?;
            zeilen.push((name, wert));
        } else if b & 0xe0 == 0x20 {
            // 6.3 Dynamic Table Size Update — nicht am Anfang gesehen.
            return Err(HpackFehler::GroessenaenderungFalscherOrt);
        } else {
            // 6.2.2 / 6.2.3 Literal without indexing / never indexed.
            // Beide sehen fuer den Server gleich aus: sie aendern die
            // Tabelle nicht.
            let (name, verbraucht) = lies_name(&block[pos..], dekodierer)?;
            pos += verbraucht;
            let (wert, verbraucht_wert) = lies_string(&block[pos..])?;
            pos += verbraucht_wert;
            zeilen.push((name, wert));
        }
    }
    Ok(zeilen)
}

/// Ein Name: entweder Index (4 Bit Praefix) oder Literal (4/6 Bit
/// Praefix mit Index 0).
///
/// DER WEG: der Aufrufer gibt den Byte-Start; hier wird der Rest
/// gelesen. Fuer einen Index wird der Tabelleneintrag **geleihen**
/// (`&'a str`) und kopiert — die Tabelle bleibt unveraendert, das ist
/// der Unterschied zu 6.2.1.
fn lies_name(b: &[u8], dekodierer: &Dekoder) -> Result<(String, usize), HpackFehler> {
    if b.is_empty() {
        return Err(HpackFehler::ZahlZuLang { kam: 0 });
    }
    if b[0] & 0x80 != 0 {
        // 6.1 ist die indexed Form, kein Literal — der Aufrufer hat
        // sie schon abgefangen.
        return Err(HpackFehler::IndexUngueltig { war: 0x80 });
    }
    // DIE ZWEI PRAEFIXE, und sie stehen in RFC 7541 6.2.1 gegen 6.2.2:
    //
    // | Form                      | Muster        | Bits |
    // | 6.2.1 mit Indexierung     | 01xxxxxx      | **6** |
    // | 6.2.2 ohne / 6.2.3 nie    | 0000xxxx (0001xxxx) | **4** |
    //
    // DER FEHLER, DEN DER TESTLAUF GEFUNDEN HAT: beide Formen wurden mit
    // 4 Bit gelesen. Bei 6.2.1 mit Index >= 15 (statische Tabelle,
    // Index 24 = `cache-control`) liest man damit ein Byte zu viel und
    // den Wert als Namen — der Block "ergab" eine Kopfzeile, die es so
    // nicht gab. Der Beleg dafuer ist `0x58` aus C.3.2 der Norm.
    let sechs_bit = b[0] & 0xc0 == 0x40;
    let (index, verbraucht) = if sechs_bit {
        lies_int(b, 6)?
    } else {
        lies_int(b, 4)?
    };
    if index != 0 {
        let (name, _wert) = hole(index, dekodierer)?;
        return Ok((name.to_string(), verbraucht));
    }
    // Index 0: der Name steht als String im Block (5.2).
    let (name, verbraucht2) = lies_string(&b[verbraucht..])?;
    Ok((name, verbraucht + verbraucht2))
}

/// Liest eine Zahl mit **4-Bit**-Praefix (die Literal-Formen).
///
/// DER WEG, und der Grund fuer die eigene Funktion: bei 4 Bit
/// Praefix liegt der Startwert in den **unteren** vier Bit und das
/// Fortsetzungsbit ist Bit 4 (0x10) — nicht wie bei [`lies_zahl`] das
/// Hochbit. Ein Tippfehler an einer Stelle hiesse: jedes
/// Huffman-Literal mit Laenge >= 15 waerfalsch gelesen.
pub fn lies_zahl4(b: &[u8]) -> Result<(usize, usize), HpackFehler> {
    lies_int(b, 4)
}

/// Der gemeinsame Teil aller drei Praefix-Leser.
///
/// added 01.10.2026, SERVER V2. DIE FORMEL IST DER GANZE PUNKT:
/// RFC 7541 Abschnitt 5.1 sagt
///
/// ```text
/// I = Praefix
/// fuer jedes Folgeoktett O:  I = I + (O & 0x7f) * 2^M;  M = M + 7
/// ```
///
/// Also **Addition mit Versatz**, nicht `wert * 256 + zeichen`. Der
/// Unterschied ist riesig, und der Fehler sieht harmlos aus: das
/// Beispiel der Norm (1337 als `1f 9a 0a` in der 5-Bit-Form) liefert
/// mit der `* 256`-Form 2038282 — eine Zahl, die kein Index mehr ist,
/// sondern nur noch so aussieht.
///
/// M4/M3: hoechstens `MAX_ZAHL_BYTES` Oktette, und das Ergebnis muss in
/// `usize` passen (eine Zahl, die groesser ist, ist auf einer
/// 32-Bit-Maschine ein abgeschnittener Index).
fn lese_fortsetzung(b: &[u8], start: u64) -> Result<(usize, usize), HpackFehler> {
    let mut wert = start;
    // M3: der Versatz beginnt bei **Null**, nicht bei der Breite des
    // Praefixes. Das ist der Fehler, den der erste Testlauf gemacht
    // hat: die Norm rechnet `I + (O & 0x7f) * 2^(7*j)` mit j = 0 fuer
    // das erste Folgeoktett, und der Beleg dafuer ist ihr eigenes
    // Beispiel (5-Bit-Form von 1337: 31 + 26 + 10*128 = 1337).
    let mut m = 0u32;
    let mut i = 1usize;
    while i < b.len() {
        if i >= MAX_ZAHL_BYTES {
            return Err(HpackFehler::ZahlZuLang { kam: i + 1 });
        }
        let o = b[i];
        // M3: bei 8 Oktett sind 1 + 7*7 = 50 Bit gesetzt, das passt in
        // u64 — deshalb ist die Verschiebung hier unkritisch.
        wert = wert
            .checked_add(((o & 0x7f) as u64) << m)
            .ok_or(HpackFehler::ZahlZuLang { kam: i + 1 })?;
        i += 1;
        m += 7;
        if o & 0x80 == 0 {
            if wert > usize::MAX as u64 {
                return Err(HpackFehler::ZahlZuLang { kam: i });
            }
            return Ok((wert as usize, i));
        }
    }
    // Die Zahl laeuft ins Leere, ohne Abschlussbit.
    Err(HpackFehler::ZahlZuLang { kam: b.len() })
}

/// Der Eintrag `index`: statische Tabelle (1..=61) oder dynamische
/// (62..), sonst [`HpackFehler::IndexUngueltig`].
fn hole(index: usize, dekodierer: &Dekoder) -> Result<(&str, &str), HpackFehler> {
    if index == 0 {
        // RFC 7541 6.1: Index 0 ist ein Fehler, kein leerer Eintrag.
        return Err(HpackFehler::IndexUngueltig { war: index });
    }
    if let Some(e) = statischer_eintrag(index) {
        return Ok(e);
    }
    dekodierer
        .hole(index)
        .ok_or(HpackFehler::IndexUngueltig { war: index })
}
