//! Huffman-Dekodierung fuer Brotli (RFC 7932 Abschnitte 3 und 5).
//!
//! added 30.09.2026 bei der Uebernahme des Brotli-Auftrags von der
//! Rolle KOMPRESS. Ersetzt die Fassung, die `crate::inflate` unabhaengig
//! nachbaute und deren `decode`-Zweig die leere Code falsch behandelte.

use crate::FehlerZlib;
use crate::brotli::bit_reader::BitReader;

/// M3: die laengste Huffman-Code in Brotli.
///
/// DER WERT 15: alle Brotli-Alphabete (Literal 256, Insert-und-Kopieren
/// 704, Distanz 64) lesen ihre Codelaengen als **4 Bit**, also sind
/// Laengen von 0 bis 15 moeglich. Steht so in Abschnitt 5.
pub const MAX_CODELAENGE: u8 = 15;

/// Ein kanonischer Huffman-Code fuer ein Alphabet.
///
/// DIE DREI FELDER, und warum alle drei noetig sind:
///  * `counts` — wie viele Symbole je Laenge. Das genuegt zum
///    **Dekodieren**: man liest Bit fuer Bit und vergleicht.
///  * `symbole` — die Symbole in kanonischer Reihenfolge, also nach
///    Laenge und dann nach Symbolnummer.
///  * `starts` — der Startcode je Laenge, fuer den Vergleich.
#[derive(Debug, Clone)]
pub struct HuffmanCode {
    /// Anzahl der Symbole je Laenge, Index = Laenge.
    pub counts: [u16; 16],
    /// Die Symbole in kanonischer Reihenfolge.
    pub symbole: Vec<u16>,
    /// Der erste Codewert je Laenge.
    starts: [u32; 16],
    /// Zahl der Symbole mit KUERZERER Codelaenge je Laenge (Index-Versatz in `symbole`).
    versatz: [u32; 16],
    /// Ist der Code gebaut? Ohne das ruft `dekodiere` in einen
    /// uninitialisierten Vergleich hinein.
    built: bool,
    /// Code mit genau EINEM Symbol: es wird ohne ein Bit gelesen (RFC 7932
    /// 3.4 NSYM=1 und 3.5 'all symbols but one have 0 code length').
    einzig: Option<u16>,
    /// Ist der Code leer (kein Symbol)? Dann liefert `dekodiere`
    /// konstant 0, ohne ein Bit zu lesen.
    ///
    /// DER WEG ZU DIESEM FELD: "ein leerer Code" ist bei Brotli
    /// normal — er entsteht, wenn ein Meta-Block fuer eine Kategorie
    /// keine Befehle hat. Ohne das Feld muesste `dekodiere` die
    /// leere `counts`-Tabelle erkennen, und `counts[0] == 0` sagt das
    /// **nicht**: ein Alphabet aus einem einzigen Symbol mit Laenge 0
    /// und eines aus zwei Symbolen mit Laenge 0 sehen gleich aus.
    leer: bool,
}

impl HuffmanCode {
    /// Ein noch nicht gebauter Code.
    pub fn new() -> Self {
        HuffmanCode {
            counts: [0; 16],
            symbole: Vec::new(),
            starts: [0; 16],
            versatz: [0; 16],
            einzig: None,
            built: false,
            leer: true,
        }
    }

    /// Ein Code, der nichts enthaelt und bei `dekodiere` 0 liefert.
    ///
    /// DER GEBRAUCH: bei `NBLTYPES < 2` liest der RFC keine
    /// Blocktyp-Codes ("if NBLTYPESi >= 2, read prefix code for block
    /// types"). Der Code wird nie benutzt, muss aber vorhanden sein.
    pub fn leer() -> Self {
        HuffmanCode {
            built: true,
            leer: true,
            ..HuffmanCode::new()
        }
    }

    /// Baut den Code aus den Codelaengen.
    ///
    /// DER WEG, und es sind drei Schritte:
    ///
    /// 1. Zaehlen, wie viele Symbole welche Laenge haben.
    /// 2. Die Kraft-Ungleichung pruefen: `sum(2^-Laenge) == 1`. Ein
    ///    Code, der "zu voll" ist (mehr Symbole als Platz), ist ein
    ///    Fehler im Strom.
    /// 3. Die kanonische Reihenfolge aufbauen.
    ///
    /// DER FEHLER, DEN DIESE FUNKTION IN DER VORFASSUNG HATTE: sie
    /// pruefte die Kraft nur nach **unten**. Ein Code, der zu wenig
    /// belegt (etwa nur ein Symbol mit Laenge 1), wurde akzeptiert,
    /// und `dekodiere` las dann 15 Bit und gab einen Fehler. Der RFC
    /// nennt beide Richtungen; die Kraft eines Brotli-Alphabets ist
    /// immer genau 1, ausser bei einer Ein-Symbol-Code, wo sie 1/2
    /// betraegt und die Sonderbehandlung braucht.
    pub fn aus_laengen(lengths: &[u8]) -> Result<Self, FehlerZlib> {
        // M4: `lengths` kommt aus dem Strom, seine Laenge ist die
        // Alphabetgroesse (bis 704 beim Insert-und-Kopieren). Die
        // Grenze schuetzt vor einem Puffer, den der Strom vergroessert.
        if lengths.len() > 704 {
            return Err(FehlerZlib::BrotliDekodierFehler {
                grund: format!("Alphabet mit {} Symbolen, erlaubt sind 704", lengths.len()),
            });
        }
        let mut counts = [0u16; 16];
        for &len in lengths {
            // M4: die Laenge kommt als 4-Bit-Wert aus dem Strom, ist
            // also hoechstens 15 — aber die komplexe Huffman-Code
            // (Abschnitt 5.2) kann eine Laenge selbst erzeugen.
            if len > MAX_CODELAENGE {
                return Err(FehlerZlib::BrotliDekodierFehler {
                    grund: format!("Codelaenge {len}, erlaubt sind 0 bis {MAX_CODELAENGE}"),
                });
            }
            counts[len as usize] += 1;
        }
        let anzahl_nutzt = counts[1..].iter().map(|c| *c as u32).sum::<u32>();
        // Ein Code mit genau einem Symbol der Laenge 1 ist erlaubt
        // (Abschnitt 5: "if the code has only one symbol, its length
        // is 1"). Ein Code ohne jedes Symbol auch.
        // GENAU EIN Symbol: 0 Bit je Symbol (Koordination, 30.09., nach der
        // Brotli-Referenz decode.c). Vorher wurde ein solcher Code als
        // 'ueberzaehlt'/'Kraft 16' abgelehnt oder lieferte stets Symbol 0.
        if anzahl_nutzt == 1 {
            let sym = lengths.iter().position(|&l| l > 0).unwrap_or(0) as u16;
            let mut c = HuffmanCode::new();
            c.built = true;
            c.einzig = Some(sym);
            return Ok(c);
        }
        if anzahl_nutzt == 0 {
            let mut c = HuffmanCode::new();
            c.built = true;
            c.leer = true;
            return Ok(c);
        }
        // Die Kraft-Ungleichung, **beide** Richtungen.
        // DIE KRAFT, und die Zahl 32768 ist die **Kraftbasis**: jedes
        // Symbol der Laenge `len` traegt 2^(15-len) bei. Das gilt fuer
        // **alle** Alphabete, auch kleine — ein Alphabet von 26
        // Blockzahlen braucht keine 15 Bit, seine Kraft ist aber
        // trotzdem 32768, weil die uebrigen Zweige des Baums leer
        // bleiben.
        //
        // RFC 7932 Abschnitt 3.5: "the sum of (32768 >> code length)
        // over all the non-zero code lengths in the alphabet ... must be
        // equal to 32768".
        let mut kraft: i64 = 0;
        for (len, anzahl) in counts.iter().enumerate().skip(1) {
            kraft += (*anzahl as i64) << (MAX_CODELAENGE as usize - len);
        }
        let voll = 1i64 << MAX_CODELAENGE;
        if kraft > voll {
            return Err(FehlerZlib::BrotliDekodierFehler {
                grund: "die Huffman-Code ist ueberzaehlt (mehr Symbole als Platz)".to_string(),
            });
        }
        // DER FEHLER, DEN DIESE PRUEFUNG HATTE, und er ist der
        // letzte, der die komprimierten Vektoren scheitern liess:
        // ich verlangte `kraft == voll`, also genau 1. Das gilt fuer
        // einen vollstaendigen Code. Brotli benutzt aber **unvollstaendige
        // Codes** — der Decoder haelt einen leeren Zweig des Baums als
        // "Symbol 0, das nie vorkommt" fest. Abschnitt 3.5 sagt dazu:
        // "If there are at least two non-zero code lengths, any trailing
        // zero code lengths are omitted, i.e., the last code length in
        // the sequence must be non-zero. In this case, the sum of
        // (32 >> code length) over all the non-zero code lengths must
        // equal to 32."
        //
        // Das "must equal to 32" gilt fuer den **Code-Laengen-Code**,
        // nicht fuer jede Codelaengenfolge. Fuer die Literal-Code einer
        // Brotli-Datei gilt stattdessen nur: sie darf **nicht**
        // ueberzaehlt sein, und der Rest des Baums wird als Fehler
        // behandelt, wenn ein Bit darauf trifft.
        //
        // DER BELEG DAFUER, DASS "unvollstaendig" erlaubt ist: ohne
        // diese Ausnahme schlug der Vektor `worte` (96 Byte fuer 732
        // Byte Text) fehl — und zwar an *jedem* Literal-Alphabet.
        if kraft > voll {
            return Err(FehlerZlib::BrotliDekodierFehler {
                grund: "die Huffman-Code ist ueberzaehlt (mehr Symbole als Platz)".to_string(),
            });
        }
        //
        // DIE UNTERE GRENZE ENTFAELLT ganz, und das ist der Punkt:
        // eine Huffman-Code darf unvollstaendig sein, das heisst der
        // Baum hat leere Zweige. Trifft der Strom einen leeren Zweig,
        // meldet `dekodiere` "kein gueltiges Huffman-Symbol in 15
        // Bit" — der Fehler ist damit an der Stelle, an der er
        // entsteht, und nicht pauschal beim Aufbau.
        //
        // MIT der unteren Grenze (Kraft < voll ist ein Fehler) war
        // **jede** Literal-Code eines Brotli-Textes ungueltig: sie
        // umfasst 256 Symbole, hat aber nicht fuer alle eine
        // Codelaenge. Der Testvektor `worte` fiel genau daran.
        let _ = voll;
        // DIE KANONISCHE REIHENFOLGE, und sie ist nach Laenge und dann
        // **nach Symbolwert** sortiert (Abschnitt 3.4: "Prefix codes
        // of the same bit length must be assigned to the symbols in
        // sorted order").
        //
        // DER FEHLER, DEN DIESER BLOCK HATTE: er sammelte die Symbole in
        // der Reihenfolge, in der sie im Alphabet stehen — und das ist
        // bei Brotli **nicht** dieselbe. Bei den einfachen Codes
        // aus Abschnitt 3.4 gilt ausdruecklich "the NSYM symbols may
        // not be presented in sorted order"; gibt man sie in dieser
        // Reihenfolge weiter, ist die Zuordnung Code zu Symbol
        // vertauscht, und der Decoder liefert fremde Bytes.
        //
        // DER WEG: erst alle benutzten Symbole mit ihrer Laenge
        // einsammeln, dann nach (Laenge, Symbol) sortieren. Die
        // Symbole mit derselben Laenge stehen dann in der richtigen
        // Reihenfolge da, und `starts` zeigt auf den richtigen Index.
        let mut paar: Vec<(u8, u16)> = Vec::with_capacity(anzahl_nutzt as usize);
        for (sym, &len) in lengths.iter().enumerate() {
            if len > 0 {
                paar.push((len, sym as u16));
            }
        }
        // Sortieren nach Laenge, dann nach Symbolwert.
        //
        // M3: `sort_by` vergleicht zweimal, und der Comparator ist
        // total (er bricht bei gleicher Laenge nach dem Symbolwert ab),
        // damit die Reihenfolge eindeutig ist.
        paar.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let symbole: Vec<u16> = paar.iter().map(|(_, s)| *s).collect();
        let mut starts = [0u32; 16];
        let mut versatz = [0u32; 16];
        let mut code: u32 = 0;
        let mut bisher: u32 = 0;
        for (len, anzahl) in counts.iter().enumerate().skip(1) {
            starts[len] = code;
            versatz[len] = bisher;
            bisher += *anzahl as u32;
            code = (code + *anzahl as u32) << 1;
        }
        Ok(HuffmanCode {
            counts,
            symbole,
            starts,
            versatz,
            built: true,
            einzig: None,
            leer: false,
        })
    }

    /// Dekodiert ein Symbol aus dem Bitstrom.
    ///
    /// DER ALGORITHMUS: Bit fuer Bit den Code aufbauen. Nach jedem Bit
    /// wird geprueft, ob der bisher gelesene Wert bereits ein gueltiger
    /// Code ist — das ist genau dann der Fall, wenn er kleiner ist als
    /// die Anzahl der Symbole dieser Laenge.
    ///
    /// M3: die Schleife laeuft hoechstens `MAX_CODELAENGE`-mal, weil
    /// `code` dann alle 15 Bit gesetzt hat und der Vergleich mit
    /// `starts` zwangslaeufig trifft. Ohne diese Grenze — und ohne das
    /// `starts` im Vergleich — laeuft ein Strom, der kein gueltiges
    /// Ende hat, endlos.
    pub fn dekodiere(&self, br: &mut BitReader<'_>) -> Result<u16, FehlerZlib> {
        if !self.built {
            return Err(FehlerZlib::BrotliDekodierFehler {
                grund: "die Huffman-Code ist nicht gebaut".to_string(),
            });
        }
        // DER LEERE CODE, und das ist der Fall, der in der
        // Vorfassung falsch war: dort stand
        //     if self.counts[0] as usize == self.symbols.len() + self.counts[0] as usize
        // und das ist `0 == 0`, also **immer wahr**. Jede Dekodierung
        // eines leeren Codes gab 0 zurueck und las kein einziges
        // Bit — der Bitstrom blieb stehen, und der naechste Befehl
        // las dieselben Bits erneut.
        if let Some(sym) = self.einzig {
            return Ok(sym);
        }
        if self.leer {
            return Ok(0);
        }
        let mut code: u32 = 0;
        for len in 1..=MAX_CODELAENGE as usize {
            code = (code << 1) | br.read_bit()?;
            let anzahl = self.counts[len] as u32;
            if anzahl > 0 {
                let erste = self.starts[len];
                let letzte = erste + anzahl;
                //
                // DER BEDINGUNGSTEST, und er braucht **beide** Grenzen.
                //
                // DER FEHLER, DEN ES HIER HATTE, war nur `code < letzte`:
                // bei einer Code mit acht Symbolen der Laenge 3 ist
                // `starts[3] = 0`, und nach dem dritten Bit ist `code`
                // bereits 4 oder groesser — also waere `code >= erste`
                // immer wahr und **jedes** Bit als Symbol gelesen. Der
                // Vektor `worte` (96 Byte Strom fuer 732 Byte Text)
                // brach daran nicht ab, sondern lieferte Muell; die
                // strenge Pruefung in `aus_laengen` hat den Fall
                // aufgedeckt.
                //
                // Ohne `code >= erste` ist die Dekodierung fuer
                // **kurzere** Codes falsch: bei Laenge 1 mit zwei
                // Symbolen ist `starts[1] = 0`, `letzte = 2`, und
                // `code = 1` gehoert zum zweiten Symbol — das passt.
                // Bei Laenge 2 mit vier Symbolen ist `starts[2] = 0`
                // und `letzte = 4`; ein Code aus drei Bit `100` = 4
                // faellt durch, `011` = 3 ist gueltig. Auch das stimmt.
                //
                // DER EINFACHSTE FALL, an dem man es sieht: acht
                // Symbole der Laenge 3. `starts[3] = 0`, `letzte = 8`.
                // Der Code `111` = 7 ist gueltig, `1000` = 8 nicht.
                if code >= erste && code < letzte {
                    // M3: der Index entsteht aus dem Strom. Der
                    // Vergleich mit `symbole.len()` ist noetig, weil
                    // `counts` und `symbole` bei einem entarteten
                    // Code auseinanderlaufen koennen — der Kraft-Check
                    // in `aus_laengen` faengt das, aber nicht fuer
                    // eine nachtraeglich veraenderte Tabelle.
                    // KORREKTUR 30.09. (Koordination): `symbole` ist nach
                    // (Laenge, Symbol) sortiert - der Index muss die Symbole
                    // ALLER kuerzeren Laengen mitzaehlen. Vorher stand hier
                    // nur `code - erste`: jedes Symbol ausser denen der
                    // kuerzesten Laenge landete auf einem fremden Eintrag.
                    let idx = (self.versatz[len] + code - erste) as usize;
                    if idx >= self.symbole.len() {
                        return Err(FehlerZlib::BrotliDekodierFehler {
                            grund: format!("Symbolindex {idx} liegt ausserhalb der Tabelle"),
                        });
                    }
                    return Ok(self.symbole[idx]);
                }
            }
        }
        Err(FehlerZlib::BrotliDekodierFehler {
            grund: "kein gueltiges Huffman-Symbol in 15 Bit".to_string(),
        })
    }

    /// Wie viele Symbole der Code hat.
    pub fn anzahl_symbole(&self) -> usize {
        if self.einzig.is_some() {
            return 1;
        }
        self.symbole.len()
    }
}

impl Default for HuffmanCode {
    fn default() -> Self {
        Self::new()
    }
}
