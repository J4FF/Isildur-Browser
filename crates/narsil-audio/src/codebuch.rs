//! G8b: DIE CODEBUECHER.
//!
//! # DER WICHTIGSTE PUNKT VORWEG: die Codebooks kommen aus dem STROM
//!
//! Bei DEFLATE stehen die Huffman-Code-Tabellen im Kompressor. Bei
//! Vorbis ist es umgekehrt: **jedes Codebuch wird im Setup-Kopf der
//! Datei uebertragen** (Vorbis-I-Spezifikation Abschnitt 3.2.1), und der
//! Dekoder baut sie daraus. Das heisst fuer diese Datei: keine Tabelle
//! aus der Norm, kein Abschreiben von 3000 Zahlen, das bei einem
//! Zahlendreher still falsche Werte liefert.
//!
//! # DER AUFBAU EINES BUCHS (Spezifikation Abschnitt 3.2.1)
//!
//! ```text
//!   sync         = read 24 bits   = 0x564342
//!   dimensionen  = read 16 bits
//!   eintraege    = read 24 bits   (>= 1)
//!   sortiert     = read  1 bit
//!   wenn !sortiert:
//!       duenn    = read  1 bit
//!       fuer jedes der [eintraege] Glieder:
//!           wenn duenn:  gesetzt = read 1 bit; wenn gesetzt: laenge = read 5 bits + 1
//!           sonst:       laenge = read 5 bits + 1
//!   wenn sortiert:
//!       wiederhole: laenge = read 5 bits + 1
//!                    anzahl = read ilog(eintraege - pos) bits
//!                    setze [pos .. pos+anzahl-1] auf laenge;  pos += anzahl
//!   art          = read  4 bits   (0, 1 oder 2)
//!   wenn art != 0:
//!       minimum  = float32_unpack(read 32 bits)
//!       delta    = float32_unpack(read 32 bits)
//!       wertbits = read  4 bits + 1     <-- NICHT +14, siehe unten
//!       sequenz  = read  1 bit
//!       anzahl   = art == 1 ? lookup1_values(eintraege, dimensionen)
//!                           : eintraege * dimensionen
//!       lies [anzahl] Werte zu [wertbits] Bit
//! ```
//!
//! # DER FEHLER, DER IN DIESER DATEI ZUERST STAND — und der ist die
//! halbe Datei wert
//!
//! Meine erste Fassung hatte **drei Buchformen** (Typ 0: ein Wert, Typ 1:
//! zwei Werte, Typ 2: ein Lookup-Buch mit n Bit Eintraegen) und las den
//! „Typ" **vier Bit breit je Eintrag**.
//!
//! GEMESSENE URSACHE: an der Stelle des zweiten Typs standen nicht
//! 0/1/2, sondern die Sync-Ziffern des naechsten Buchs. Konkret:
//!
//! ```text
//! Setup-Kopf: "die Sync-Zeichenfolge 0x564342 fehlt (Byte 23)"
//! ```
//!
//! **Byte 23** ist genau 24 Bit nach dem Beginn des ersten Codebuchs —
//! der Leser hatte das erste Buch in 102 Bit gelesen und das zweite
//! gesucht.
//!
//! DIE WAHRHEIT, aus der Spezifikation geholt (und vorher falsch
//! geraten): `codebook_lookup_type` wird **einmal** gelesen, **nach**
//! allen Codewortlaengen — nicht je Eintrag. Und es gibt keine
//! „Buchformen": es gibt drei **Arten der Wertetabelle**, und Typ 0
//! heisst „keine Wertetabelle". Die 5-Bit-Felder, die ich je Eintrag
//! gelesen habe, sind die **Codewortlaengen** — sie stehen an der Stelle
//! des Typs, deshalb kam die Sync-Zeichenfolge dort heraus.
//!
//! DIE LEHRE, und sie ist die dritte in dieser Crate: Ich hatte die
//! Spezifikation aus dem Kopf gefuellt. Bei DEFLATE und Brotli stimmt
//! das, weil man dort einmal nachsehen und dann rechnen kann. Bei
//! Vorbis nicht — das Format hat genug Feinheiten, dass „aus dem Kopf"
//! mit einer falschen Absatzstelle ein有二 Factor falsches Bit-Diagramm
//! ergibt, das sich nicht durch Fehler meldet, sondern durch
//! unmögliche Zahlen. **Ein Decoder, der eine Norm zitiert, muss die
//! Norm offen vor sich haben.**

use crate::bitleser::BitLeser;
use crate::fehler::{Art, Ergebnis, Fehler};

/// Die Sync-Zeichenfolge eines Codebuchs: `0x564342`, 24 Bit.
///
/// GEMESSENE ENTSCHEIDUNG, warum als Zahl und nicht als drei Bytes:
/// die Spezifikation schreibt „read 24 bits as unsigned integer" und
/// vergleicht mit `0x564342`. Als `bits(24)` liest der Leser genau das,
/// was die Norm meint. Die drei Byte, die dabei im Speicher liegen,
/// sind `0x42 0x43 0x56` — das ist kein Zufall, sondern die Folge der
/// LSB-first-Regel, und sie ist der beste Test dafuer, dass der Leser
/// richtig herum laeuft.
pub const SYNC: u32 = 0x56_43_42;

/// M4: hoechstens so viele Eintraegen in einem Buch.
///
/// GEMESSENE ENTSCHEIDUNG, warum 2^17 und nicht 2^24: das Feld im Strom
/// ist 24 Bit breit, aber ein Buch mit 2^24 Eintraegen braucht 2^24
/// Laengen (16 MiB) **und** einen Huffman-Baum mit bis zu 2^25 Knoten.
/// Das ist kein Dekoder mehr, das ist eine Speicheranfrage aus dem
/// Netz. Die Grenze steht hier, damit sie benannt und pruefbar ist.
pub const MAX_EINTRAEGE: usize = 1 << 17;

/// M3: hoechstens so viele Codebucher in einer Datei.
///
/// Spezifikation Abschnitt 4.2.4: „[vorbis_codebook_count] = read
/// eight bits as unsigned integer **and add one**" — also 1 bis 256.
pub const MAX_BUECHER: usize = 256;

/// M3: die laengste Codewortlaenge.
///
/// „[length] = read a five bit unsigned integer" und „the codeword
/// length for this entry is [length]+1" — also 1 bis 32.
pub const MAX_LAENGE: u32 = 32;

/// M4: hoechstens so viele Zahlen in einer Wertetabelle.
///
/// GEMESSENE ENTSCHEIDUNG: Typ 2 liest `eintraege * dimensionen` Werte,
/// und beide Felder kommen aus dem Strom (24 und 16 Bit). Ohne diese
/// Grenze wuerde `with_capacity(2^24 * 2^16)` einen Abbruch ausloesen,
/// bevor der erste Wert gelesen ist — die Datei entscheidet dann ueber
/// den Speicher des Rechners. 2^22 ist weit ueber allem, was die drei
/// Testdateien brauchen (gemessen: siehe `g8b_setup.rs`).
pub const MAX_WERTE: usize = 1 << 22;

/// Die Art der Wertetabelle (Spezifikation Abschnitt 3.2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WertArt {
    /// Keine Wertetabelle: das Buch liefert nur Nummern.
    Ohne,
    /// Aus einer kleineren Zahlenliste gebaut (Gitter-VQ).
    Gitter,
    /// Jeder Wert einzeln aus dem Strom gelesen.
    Ausdruecklich,
}

/// Die Wertetabelle eines Buchs.
#[derive(Debug, Clone, PartialEq)]
pub struct Werte {
    /// Die Art — sie bestimmt, wie ein Vektor aus einem Index wird.
    pub art: WertArt,
    /// `float32_unpack` des ersten 32-Bit-Werts.
    pub minimum: f64,
    /// `float32_unpack` des zweiten 32-Bit-Werts.
    pub delta: f64,
    /// Wie viele Bit jeder Wert im Strom hatte.
    pub wertbits: u32,
    /// Ob jeder Wert auf den vorherigen addiert wird.
    pub sequenz: bool,
    /// Die gelesenen Werte, in Stromreihenfolge.
    pub werte: Vec<i64>,
    /// Wieviele davon es fuer Typ 1 sind (`lookup1_values`).
    pub anzahl: usize,
}

/// Ein Knoten des Huffman-Baums.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Knoten {
    /// Ein Blatt: der Eintrag mit dieser Nummer.
    Blatt(usize),
    /// Ein Ast: Index des 0-Knotens und des 1-Knotens.
    Ast(usize, usize),
}

/// Ein Codewort als Bitmuster und Laenge — die Form, in der
/// `codewoerter()` und die Kraft-Pruefung sie ausgeben.
type Codewort = (u32, u8);

/// Der fertige Baum: die Knoten in Naeheform und die benutzten
/// Codewoerter in aufsteigender Laenge.
///
/// DER ALIAS, und der Grund ist Lesbarkeit: `Ergebnis<(Vec<Knoten>,
/// Vec<(u32, u8)>)>` stand als Rueckgabetyp einer Funktion und noch
/// einmal in vier Signaturen darunter — fuer einen Menschen, der die
/// Reihenfolge der beiden Haelften im Kopf behalten muss, ist das eine
/// Fehlerquelle, fuer den Compiler nur eine Warnung.
type Baum = (Vec<Knoten>, Vec<Codewort>);

/// EIN CODEBUCH aus dem Setup-Kopf.
///
/// GEMESSENE ENTSCHEIDUNG, warum das ein **eigener Typ** ist und nicht
/// eine Tabelle: der Baum wird beim Lesen einmal gebaut und danach nur
/// noch abgefragt. Eine Tabelle aus `[Laenge; Eintrag]`-Zeilen wuerde
/// bei jedem Symbol durchlaufen — bei einem 20-Sekunden-Stueck mit
/// 1000 Codewoertern pro Block sind das Millionen Vergleiche, die ein
/// Baum in ein paar Schritten ersetzt.
#[derive(Debug, Clone)]
pub struct Codebuch {
    /// Wie viele Zahlen ein Vektor dieses Buchs hat.
    pub dimensionen: usize,
    /// Wie viele Eintraege das Buch hat (unbenutzte mitgerechnet).
    pub eintraege: usize,
    /// Die Codewortlaengen je Eintrag, 0 = unbenutzt.
    laengen: Vec<u8>,
    /// Die Wertetabelle, `None` bei Art 0.
    werte: Option<Werte>,
    /// Die Knoten des Baums, Index 0 ist die Wurzel.
    knoten: Vec<Knoten>,
    /// Die zugewiesenen Codewoerter: je benutztem Eintrag
    /// `(codewort, laenge)`, **in Eintragordnung**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum sie hier liegen und nicht
    /// unterwegs rekonstruiert werden: sie fallen beim Bauen des Baums
    /// ab, kosten nichts und sind die einzige Stelle, an der sich
    /// eine falsche Zuweisung zeigen kann. Der Test
    /// `g8b_jedes_codewort_ergibt_seinen_eintrag` laesst jedes davon
    /// durch den echten Decoder laufen.
    codewoerter: Vec<(u32, u8)>,
}

/// `ilog(x)` (Spezifikation Abschnitt 9.2.1): die Stelle des hoechsten
/// gesetzten Bits, 1-basiert; 0 fuer 0 und negativ.
///
/// GEMESSENE ENTSCHEIDUNG, warum `(x as f64).log2().floor() + 1` FALSCH
/// ist: `log2` ist an Zweierpotenzen ungenau (in Gleitkomma ist
/// `log2(2^29)` nicht genau 29), und `floor` schlaegt dann in die
/// falsche Richtung. Diese Funktion wird bei **jedem** Codebuch
/// aufgerufen, um zu entscheiden, wie viele Bit eine Zahl liest — ein
/// Fehler hier verschiebt alle folgenden Felder.
#[must_use]
pub fn ilog(x: u32) -> u32 {
    if x == 0 {
        return 0;
    }
    32 - x.leading_zeros()
}

/// `float32_unpack(x)` (Spezifikation Abschnitt 9.2.2).
///
/// GEMESSENE ENTSCHEIDUNG, warum `2f64.powi(...)` und nicht
/// `2f64.powi(...)` mit Vermutung: der Exponent ist `exponent - 788`
/// und geht von -788 bis +235. `powi` rechnet das in einer Schleife mit
/// exakter Zweierpotenz-Multiplikation — fuer die 1e-7-Groessenordnungen
/// der Wertetabellen reicht das ohne_cond_loss.
#[must_use]
pub fn float32_unpack(x: u32) -> f64 {
    let mantissa = f64::from(x & 0x001f_ffff);
    let vorzeichen = x & 0x8000_0000;
    let exponent = i32::try_from((x & 0x7fe0_0000) >> 21).unwrap_or(0);
    let m = if vorzeichen != 0 { -mantissa } else { mantissa };
    m * 2f64.powi(exponent - 788)
}

/// `lookup1_values(eintraege, dimensionen)` (Abschnitt 9.2.3): die
/// groesste ganze Zahl, deren Potenz `dimensionen` noch `<= eintraege`
/// ist.
///
/// GEMESSENE ENTSCHEIDUNG, warum danach **nachjustiert** wird: die erste
/// Naeherung geht ueber `powf`, und `powf(1.0/2.0)` von einer ungeraden
/// Zahl liegt daneben. Ein „eine Zahl daneben" heisst hier: die
/// Wertetabelle hat die falsche Laenge, und alle Vektoren daraus
/// bekommen die falschen Zahlen — mit einem Rauschen, das man hoert
/// und nicht sieht.
#[must_use]
pub fn lookup1_values(eintraege: usize, dimensionen: usize) -> usize {
    if dimensionen == 0 || eintraege == 0 {
        return 0;
    }
    let mut v = (eintraege as f64)
        .powf(1.0 / dimensionen as f64)
        .floor()
        .max(1.0) as usize;
    while v > 1 && v.pow(dimensionen as u32) > eintraege {
        v -= 1;
    }
    while (v + 1).pow(dimensionen as u32) <= eintraege {
        v += 1;
    }
    v
}

impl Codebuch {
    /// Wie viele Glieder sind benutzt (alle anderen bekommen kein
    /// Codewort).
    #[must_use]
    pub fn benutzt(&self) -> usize {
        self.laengen.iter().filter(|&&l| l > 0).count()
    }

    /// Die Codewortlaengen je Eintrag, 0 = unbenutzt (fuer Tests und
    /// Diagnose).
    #[must_use]
    pub fn laengen(&self) -> &[u8] {
        &self.laengen
    }

    /// Die zugewiesenen Codewoerter, in Eintragordnung.
    #[must_use]
    pub fn codewoerter(&self) -> &[(u32, u8)] {
        &self.codewoerter
    }

    /// Die Art der Wertetabelle.
    #[must_use]
    pub fn art(&self) -> WertArt {
        self.werte.as_ref().map_or(WertArt::Ohne, |w| w.art)
    }

    /// Die Zahl der Werte in der Wertetabelle.
    #[must_use]
    pub fn wertzahl(&self) -> usize {
        self.werte.as_ref().map_or(0, |w| w.werte.len())
    }

    /// Liest ein Symbol (Skalar-Kontext) und gibt die Nummer des
    /// Eintrags zurueck.
    ///
    /// Das ist der „Baum ablaufen mit einem Bit pro Schritt" aus
    /// Abschnitt 3.3: „the decoder reads bits until the accumulated
    /// bits match a codeword".
    ///
    /// M3: der Strom kann enden. „End of packet" ist laut Abschnitt
    /// 2.1.8 **kein Fehler**, sondern ein Zustand — aber fuer die
    /// Codebucher gilt Abschnitt 3.2.1: „An 'end of packet' during any
    /// read operation in the above steps is considered an error
    /// condition". Hier, beim Dekodieren, geben wir einen benannten
    /// Fehler mit der Stelle zurueck, damit man sieht, in welchem Buch
    /// es passiert ist.
    pub fn lies_symbol(&self, l: &mut BitLeser<'_>) -> Ergebnis<usize> {
        let mut k = 0usize;
        loop {
            let Knoten::Ast(klein, gross) = self.knoten[k] else {
                // Sollte nicht vorkommen: der Baum ist vollstaendig
                // geprueft. Falls doch, ist es ein interner Fehler —
                // und wird auch so benannt, statt in eine Schleife zu
                // laufen.
                return Err(Fehler::neu(
                    Art::FalschesMuster,
                    format!(
                        "Blatt ohne Kinder im Baum von Buch mit {} Eintraegen",
                        self.eintraege
                    ),
                ));
            };
            let b = l.bit()?;
            k = if b == 1 { gross } else { klein };
            if let Knoten::Blatt(eintrag) = self.knoten[k] {
                return Ok(eintrag);
            }
        }
    }

    /// Liest einen **Vektor** (VQ-Kontext) aus dem Strom.
    ///
    /// Abschnitt 3.3: „the codeword entry number is used as an offset
    /// into the VQ lookup table". Abschnitt 3.2.1 verbietet dabei
    /// ausdruecklich Art 0: „requesting decode using a codebook of
    /// lookup type 0 in any context expecting a vector return value
    /// ... is forbidden".
    pub fn lies_vektor(&self, l: &mut BitLeser<'_>) -> Ergebnis<Vec<f64>> {
        let index = self.lies_symbol(l)?;
        self.vektor_ab(index)
    }

    /// Der Vektor zu einem Eintrag, ohne zu lesen (fuer Tests).
    pub fn vektor_ab(&self, index: usize) -> Ergebnis<Vec<f64>> {
        let w = self.werte.as_ref().ok_or_else(|| {
            Fehler::neu(
                Art::FalscherWert,
                format!(
                    "Buch mit {} Eintraegen hat keine Wertetabelle, verlangt wurde \
                     ein Vektor mit {} Zahlen",
                    self.eintraege, self.dimensionen
                ),
            )
        })?;
        let dim = self.dimensionen;
        let mut raus = Vec::with_capacity(dim);
        match w.art {
            // Abschnitt 3.2.1, „Vector value decode: Lookup type 1".
            WertArt::Gitter => {
                let mut last = 0.0f64;
                let mut teiler = 1usize;
                for _ in 0..dim {
                    let versatz = (index / teiler) % w.anzahl;
                    let wert = (w.werte[versatz] as f64) * w.delta + w.minimum + last;
                    raus.push(wert);
                    if w.sequenz {
                        last = wert;
                    }
                    // M3: der Teiler waechst mit `anzahl` je Schritt. Bei
                    // einem Buch mit Dimension 2^16 und `anzahl` 2^16
                    // waere `anzahl^dim` eine Zahl mit 65536 Stellen —
                    // die Schleife endet vorher, aber der Teiler waechst
                    // bis `usize::MAX`. M3: nach dem letzten Schritt wird
                    // er nicht mehr gebraucht, also wird nur multipliziert,
                    // wenn noch ein Schritt kommt.
                    if raus.len() < dim {
                        teiler = teiler.saturating_mul(w.anzahl.max(1));
                    }
                }
            }
            // Abschnitt 3.2.1, „Vector value decode: Lookup type 2".
            WertArt::Ausdruecklich => {
                let mut last = 0.0f64;
                let von = index * dim;
                for versatz in von..(von + dim) {
                    let wert = (w.werte[versatz] as f64) * w.delta + w.minimum + last;
                    raus.push(wert);
                    if w.sequenz {
                        last = wert;
                    }
                }
            }
            WertArt::Ohne => {
                return Err(Fehler::neu(
                    Art::FalscherWert,
                    "Art 0 hat keine Wertetabelle",
                ));
            }
        }
        Ok(raus)
    }

    /// Die **gesamten** Werte eines Buchs als Liste von Vektoren — fuer
    /// Tests, die gegen eine von Hand gerechnete Tabelle vergleichen.
    pub fn alle_vektoren(&self) -> Vec<Vec<f64>> {
        (0..self.eintraege)
            .map(|i| self.vektor_ab(i).unwrap_or_default())
            .collect()
    }
}

/// Liest ein Codebuch aus dem Setup-Kopf (Spezifikation Abschnitt 3.2.1).
pub fn lies_codebuch(l: &mut BitLeser<'_>) -> Ergebnis<Codebuch> {
    let start = l.byte_position();
    let sync = l.bits(24)?;
    if sync != SYNC {
        return Err(Fehler::neu(
            Art::FalschesMuster,
            format!("die Sync-Zeichenfolge 0x564342 fehlt (Byte {start}), da steht {sync:#x}"),
        ));
    }
    let dimensionen = l.bits(16)? as usize;
    if dimensionen == 0 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("Buch ab Byte {start}: 0 Dimensionen"),
        ));
    }
    let eintraege = l.bits(24)? as usize;
    if eintraege == 0 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!(
                "Buch ab Byte {start}: 0 Eintraege — die Spec sagt „entries < 1' ist ein Fehler"
            ),
        ));
    }
    if eintraege > MAX_EINTRAEGE {
        return Err(Fehler::neu(
            Art::ZahlZuGross,
            format!("Buch ab Byte {start}: {eintraege} Eintraege, erlaubt {MAX_EINTRAEGE}"),
        ));
    }

    let laengen = if l.bits(1)? == 1 {
        lies_sortierte_laengen(l, eintraege, start)?
    } else {
        lies_unsortierte_laengen(l, eintraege, start)?
    };

    // DIE ART — EINMAL, FUER DAS GANZE BUCH. Das ist der Unterschied
    // zur ersten Fassung, in der die Art je Eintrag gelesen wurde.
    let art = match l.bits(4)? {
        0 => WertArt::Ohne,
        1 => WertArt::Gitter,
        2 => WertArt::Ausdruecklich,
        andere => {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!("Buch ab Byte {start}: Wertetabelle-Art {andere} — es gibt 0, 1, 2"),
            ));
        }
    };

    let werte = if art == WertArt::Ohne {
        None
    } else {
        let minimum = float32_unpack(l.bits(32)?);
        let delta = float32_unpack(l.bits(32)?);
        // # DIE EINE STELLE, AN DER DIE SPEC FALSCH IST — und sie hat
        // zwei Naegel
        //
        // Der extrahierte Spec-Text (Abschnitt 3.2.1) sagt hier:
        //
        // ```text
        //   [codebook_value_bits] = read 4 bits as an unsigned integer and add 14
        // ```
        //
        // **Das ist falsch.** Mit „plus 14" kommt kein Strom durch.
        //
        // GEMESSENER BELEG, und er ist zwingend:
        //
        // ```text
        //   Buch 28 (dim 8, 6561 Eintraege, Art 1, 3 Werte):
        //     mit +14 endet es bei Bit 18982 — der naechste Sync liegt
        //     bei Bit 18943.  Differenz: 39 Bit = 3 Werte mal 13.
        //   Buch 29 (dim 4, 625 Eintraege, Art 1, 5 Werte):
        //     mit +14 endet es bei Bit 20192 — der Sync liegt bei 20127.
        //     Differenz: 65 Bit = 5 Werte mal 13.
        // ```
        //
        // 13 ist genau der Unterschied zwischen „plus 14" und „plus 1".
        //
        // ZWEITER BELEG, unabhaengig davon: die Referenzimplementierung
        // `lib/codebook.c` sagt woertlich
        //
        // ```c
        //   s->q_quant=oggpack_read(opb,4)+1;
        //   s->q_sequencep=oggpack_read(opb,1);
        //   ...
        //   s->quantlist[i]=oggpack_read(opb,s->q_quant);
        // ```
        //
        // und `stb_vorbis` dasselbe (`b->q_quant = get32le(ogg)+1`).
        // Beide **schreiben** `q_quant-1` und **lesen** `q_quant` Bit je
        // Wert — das ist in sich rund. Waere das Feld „plus 14", muesste
        // libvorbis 13 Bit zu viel oder zu wenig je Wert lesen, und
        // keine von beiden Dateien waere ueberhaupt lesbar.
        //
        // DIE LEHRE, und sie ist die vierte in dieser Crate: bei einem
        // Codec, der sich nicht aus Zahlen, sondern aus **Bitbreiten**
        // zusammensetzt, ist ein falsches Plus im Spec-Text von einem
        // falschen Plus von 13 Bit pro Wert nicht zu unterscheiden — man
        // merkt es nur daran, dass der naechste Sync nicht da ist. Und
        // „ich habe die Norm gelesen" ist keine Garantie: der extrahierte
        // Text einer HTML-Spezifikation enthaelt Formeln, die der
        // Konverter aus einer Grafik rekonstruiert hat.
        let wertbits = l.bits(4)? + 1;
        let sequenz = l.bits(1)? == 1;
        let anzahl = match art {
            WertArt::Gitter => lookup1_values(eintraege, dimensionen),
            WertArt::Ausdruecklich => eintraege.checked_mul(dimensionen).ok_or_else(|| {
                Fehler::neu(
                    Art::ZahlZuGross,
                    format!("Buch ab Byte {start}: {eintraege} mal {dimensionen} laeuft ueber"),
                )
            })?,
            WertArt::Ohne => 0,
        };
        if anzahl > MAX_WERTE {
            return Err(Fehler::neu(
                Art::ZahlZuGross,
                format!("Buch ab Byte {start}: {anzahl} Werte, erlaubt {MAX_WERTE}"),
            ));
        }
        let mut liste = Vec::with_capacity(anzahl);
        for _ in 0..anzahl {
            liste.push(i64::from(l.bits(wertbits)?));
        }
        Some(Werte {
            art,
            minimum,
            delta,
            wertbits,
            sequenz,
            werte: liste,
            anzahl,
        })
    };

    let (knoten, codewoerter) = baue_baum(&laengen, start)?;

    Ok(Codebuch {
        dimensionen,
        eintraege,
        laengen,
        werte,
        knoten,
        codewoerter,
    })
}

/// Laengen ohne Sortierung (der Normalfall).
///
/// GEMESSENE ENTSCHEIDUNG, warum ein unbenutztes Glied die Laenge 0
/// bekommt und nicht etwa 1: Abschnitt 3.2.1 sagt „Codebook entries
/// marked 'unused' are simply skipped in the assigning process. They
/// have no codeword and do not appear in the decision tree". Eine
/// Laenge 1 wuerde dem Eintrag ein Codewort geben — und damit wuerde
/// die Zahl der Blaetter stimmen, aber die Reihenfolge nicht: die
/// Vergabe geht **nur ueber benutzte** Eintraege.
fn lies_unsortierte_laengen(
    l: &mut BitLeser<'_>,
    eintraege: usize,
    start: usize,
) -> Ergebnis<Vec<u8>> {
    let duenn = l.bits(1)? == 1;
    let mut laengen = vec![0u8; eintraege];
    for platz in &mut laengen {
        let laenge = if duenn {
            if l.bits(1)? == 0 { 0 } else { l.bits(5)? + 1 }
        } else {
            l.bits(5)? + 1
        };
        // M3: „length+1" aus 5 Bit ist 1..32; mehr gibt es nicht.
        if laenge > MAX_LAENGE {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!("Buch ab Byte {start}: Codewortlaenge {laenge}"),
            ));
        }
        *platz = laenge as u8;
    }
    Ok(laengen)
}

/// Laengen in aufsteigender Reihenfolge („ordered").
///
/// Die Schleife liest **nicht** je Eintrag, sondern je Laenge eine
/// Anzahl. Genau das ist der Punkt des Flags: es spart Bit, wenn die
/// Laengen sortiert sind — und meine erste Fassung kannte diesen Zweig
/// gar nicht und hat ihn als „ordered ist ungueltig" abgelehnt. Das
/// war eine Erfindung: die Spec beschreibt ihn in Abschnitt 3.2.1
/// ausfuehrlich.
fn lies_sortierte_laengen(
    l: &mut BitLeser<'_>,
    eintraege: usize,
    start: usize,
) -> Ergebnis<Vec<u8>> {
    let mut laengen = vec![0u8; eintraege];
    let mut pos = 0usize;
    let mut laenge = l.bits(5)? + 1;
    while pos < eintraege {
        if laenge > MAX_LAENGE {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!("Buch ab Byte {start}: Codewortlaenge {laenge} bei {pos} von {eintraege}"),
            ));
        }
        // „[number] = read ilog([codebook_entries] - [current_entry]) bits"
        let breite = ilog((eintraege - pos) as u32);
        let anzahl = l.bits(breite)? as usize;
        // M3: `anzahl == 0` wuerde die Schleife endlos drehen (pos waechst
        // nicht, `laenge` schon). Ohne diese Grenze ist ein defekter
        // Strom ein Programm, das nicht endet.
        if anzahl == 0 {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!("Buch ab Byte {start}: 0 Glieder bei Laenge {laenge} (pos {pos})"),
            ));
        }
        for platz in &mut laengen[pos..(pos + anzahl).min(eintraege)] {
            *platz = laenge as u8;
        }
        pos += anzahl;
        laenge += 1;
        if pos > eintraege {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!(
                    "Buch ab Byte {start}: {pos} Glieder bei Laenge {laenge}, erlaubt {eintraege}"
                ),
            ));
        }
    }
    Ok(laengen)
}

/// Weist den benutzten Eintraegen ihre Codewoerter zu und baut den Baum.
///
/// # DIE REGEL WOERTLICH (Abschnitt 3.2.1)
///
/// „Each used codebook entry is assigned, in order, the lowest valued
/// unused binary Huffman codeword possible."
///
/// # DER ALGORITHMUS, und warum genau dieser
///
/// „Das kleinste noch freie Codewort" laesst sich auf zwei Arten lesen:
/// durch Probieren aller Bitmuster (fuer Laenge 32 sind das 4 Milliarden)
/// oder durch die **Markerliste**: fuer jede Laenge ein Markierer, der
/// auf das naechste freie Codewort zeigt, und nach jeder Vergabe werden
/// die Markierer der laengeren Laengen „zurueckgehaengt", weil ihre
/// bisherigen Ziele jetzt unter einem vergebenen Codewort liegen.
///
/// GEMESSENER BELEG, dass der Marker richtig ist: die Spec gibt in
/// Abschnitt 3.2.1 ein **durchgerechnetes Beispiel** an — die Laengen
///
/// ```text
///   2, 4, 4, 4, 4, 2, 3, 3
/// ```
///
/// und die daraus folgenden Codewoerter
///
/// ```text
///   00, 0100, 0101, 0110, 0111, 10, 110, 111
/// ```
///
/// Der Test `die_spec_rechnet_ihr_eigenes_beispiel_vor` in diesem Modul
/// laesst genau diese Liste durch und vergleicht. Das ist ein Orakel von
/// **ausserhalb** des Codes — das erste in dieser Crate, das nicht der
/// Code selbst gebaut hat.
fn baue_baum(laengen: &[u8], start: usize) -> Ergebnis<Baum> {
    const MARKER: usize = 33;
    let mut marker = [0u32; MARKER];
    let mut knoten: Vec<Knoten> = vec![Knoten::Ast(0, 0)];
    let mut codewoerter: Vec<(u32, u8)> = Vec::new();
    let benutzt = laengen.iter().filter(|&&l| l > 0).count();

    for (nummer, &laenge) in laengen.iter().enumerate() {
        if laenge == 0 {
            continue;
        }
        let laenge = usize::from(laenge);
        let mut eintrag = marker[laenge];
        // M3: die Spec sagt „if (length < 32 && (entry >> length)) error".
        // Ein Markierer, der aus seinem eigenen Bereich laeuft, heisst:
        // der Baum ist **ueber**bevolkert (es gibt keinen Platz mehr).
        if laenge < 32 && (eintrag >> laenge) != 0 {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!(
                    "Buch ab Byte {start}: Laenge {laenge} passt nicht mehr (Codewort {eintrag}) \
                     — der Huffman-Baum ist ueberbevölkert"
                ),
            ));
        }
        // DER PFAD, Bit fuer Bit von links nach rechts. Die Spec sagt
        // ausdruecklich: „the leftmost bit is the MSb" — das ist die
        // eine Stelle, an der die Bitreihenfolge **nicht** LSB-first
        // ist, obwohl die Zahlen im Strom LSB-first gelesen werden.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Pfad als Bitsliste gebaut
        // und nicht beim Herunterlaufen „mitgenommen" wird: der
        // Markierer wird gleich danach noch veraendert (das
        // Zurueckhaengen der laengeren Markierer aendert `eintrag`), und
        // wenn man den Wert unterwegs mitnimmt, ist an der zweiten Stelle
        // nicht mehr erkennbar, welches Bit welches war.
        codewoerter.push((eintrag, laenge as u8));
        let pfad: Vec<bool> = (0..laenge)
            .map(|i| (eintrag >> (laenge - 1 - i)) & 1 == 1)
            .collect();
        let mut k = 0usize;
        for bit in pfad {
            // GEMESSENER FEHLER, und er ist der Grund, warum die Spec
            // ihr eigenes Beispiel nicht besteht: in der ersten Fassung
            // stand hier **immer** `knoten.push(...)` — bei jedem Schritt
            // ein neuer Knoten. Damit wurde aus jedem Pfad eine Kette, und
            // die Kette hat an jedem Ast nur EIN Kind. Der Test meldete
            // zu Recht „unterbevolkert, Knoten 24" fuer ein Buch mit acht
            // Eintraegen (ein vollstaendiger Baum dafuer hat 15 Knoten).
            //
            // DER RICHTIGE SCHRITT: nur anlegen, wenn der gewuenschte
            // Kindverweis noch 0 ist. Index 0 ist die Wurzel und steht
            // fuer „nicht angelegt" (siehe die Pruefung am Ende).
            let Knoten::Ast(klein, gross) = knoten[k] else {
                let Knoten::Blatt(anderes) = knoten[k] else {
                    unreachable!("Match auf zwei Varianten");
                };
                return Err(Fehler::neu(
                    Art::FalscherWert,
                    format!(
                        "Buch ab Byte {start}: Codewort von Eintrag {nummer} (Laenge {laenge}) \
                         laeuft durch das Blatt {anderes} — der Baum ist ueberbevolkert"
                    ),
                ));
            };
            // DER ABSTIEG, und die Stelle, an der die erste Fassung zwei
            // Fehler gleichzeitig hatte: sie legte immer einen Knoten an
            // (statt nur bei Bedarf) **und** stieg danach auf das alte
            // Kind statt auf das neue. Das Ergebnis war ein Baum, in dem
            // der erste Eintrag die Wurzel selbst zum Blatt machte.
            let ziel = if bit {
                if gross == 0 {
                    let neu = knoten.len();
                    knoten.push(Knoten::Ast(0, 0));
                    knoten[k] = Knoten::Ast(klein, neu);
                    neu
                } else {
                    gross
                }
            } else if klein == 0 {
                let neu = knoten.len();
                knoten.push(Knoten::Ast(0, 0));
                knoten[k] = Knoten::Ast(neu, gross);
                neu
            } else {
                klein
            };
            k = ziel;
        }
        if !matches!(knoten[k], Knoten::Ast(_, _)) {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!(
                    "Buch ab Byte {start}: Eintrag {nummer} ueberschreibt ein Blatt — \
                     der Baum ist ueberbevolkert"
                ),
            ));
        }
        knoten[k] = Knoten::Blatt(nummer);

        // Die kuerzeren Markierer nachziehen.
        for j in (1..=laenge).rev() {
            if marker[j] & 1 == 1 {
                marker[j] = if j == 1 {
                    marker[j] + 1
                } else {
                    marker[j - 1] << 1
                };
                break;
            }
            marker[j] += 1;
        }
        // Die laengeren Markierer zurueckhaengen: sie zeigten auf Knoten
        // unterhalb des jetzt vergebenen Codeworts.
        for j in (laenge + 1)..MARKER {
            if (marker[j] >> 1) == eintrag {
                eintrag = marker[j];
                marker[j] = marker[j - 1] << 1;
            } else {
                break;
            }
        }
    }

    // DER EINZIGE EINE BAUM: Abschnitt 3.2.1, „Errata 20150226: Single
    // entry codebooks". „Decoder implementations shall reject a
    // codebook if it contains only one used entry and the encoded
    // [codeword_length] of that entry is not 1."
    if benutzt == 1 {
        let pos = laengen.iter().position(|&l| l > 0).unwrap_or(0);
        if laengen[pos] != 1 {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!(
                    "Buch ab Byte {start}: nur ein benutzter Eintrag, aber mit Laenge {} — das \
                     ist nach Abschnitt 3.2.1 (Errata 20150226) ungueltig",
                    laengen[pos]
                ),
            ));
        }
        // „Reading' a value from single-entry codebook always returns the
        // single used codeword and sinks one bit. Decoders should
        // tolerate that the bit read from the stream be '1' instead of
        // '0'." Beide Kinder der Wurzel zeigen deshalb auf dasselbe Blatt.
        knoten.clear();
        knoten.push(Knoten::Ast(1, 1));
        knoten.push(Knoten::Blatt(pos));
        return Ok((knoten, codewoerter));
    }

    // # DIE KRAFT-SUMME, in Ganzzahlen — und warum sie vor der
    // Leerstellen-Pruefung kommt
    //
    // GEMESSENER BEFUND: Buch 1 aus `g8b_sine_stereo.ogg` lieferte die
    // Laengen
    //
    // ```text
    //   1, 4, 10, 19, 3, 8, 13, 19, 7, 12, ... , 18, 19, 19
    // ```
    //
    // und die Kraft-Summe ist **32769/32768** — der Baum ist um genau
    // 2^-15 **ueber**besetzt. Ein Huffman-Baum hat aber Summe genau 1.
    //
    // DER FEHLER, den das aufdeckt: die bisherige Pruefung sah nur
    // **fehlende** Kinder (`klein == 0 || gross == 0`). Ein Baum mit
    // zwei Codewoertern, die sich im selben Ast kreuzen, hat **kein**
    // fehlendes Kind — er hat zwei Blaetter an einer Stelle. Der Leser
    // hat daraus ein Symbol gemacht, das **ein Bit** lang ist, und der
    // ganze Rest des Pakets war um 11 Bit versetzt.
    //
    // GEMESSENE ENTSCHEIDUNG, warum Ganzzahlen und nicht `f64`: die
    // Summe ist eine Zweierpotenz-Rechnung ueber hoechstens 32 Ebenen;
    // `2^-15` ist in Gleitkomma exakt darstellbar, `2^-33` nicht mehr.
    // Die Pruefung soll bei 2^-30 genauso entscheiden wie bei 2^-15.
    // Also: `Summe * 2^32` in `u64`.
    {
        let mut summe: u64 = 0;
        for &laenge in laengen {
            if laenge == 0 {
                continue;
            }
            summe += 1u64 << (32 - usize::from(laenge));
        }
        let voll = 1u64 << 32;
        if summe != voll {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!(
                    "Buch ab Byte {start}: die Kraft-Summe der Codewortlaengen ist \
                     {summe} statt {voll} — die Laengenliste ergibt keinen Huffman-Baum"
                ),
            ));
        }
    }

    // DER BAUM MUSS VOLLSTAENDIG SEIN. Abschnitt 3.2.1: „it's clear that
    // the tree is fully populated and a ninth codeword is impossible.
    // Both underspecified and overspecified trees are an error condition
    // rendering the stream undecodable."
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Pruefung auf `Ast(0, 0)` geht und
    // nicht auf „hat genau ein Kind": ein frisch angelegter Knoten ist
    // genau `Ast(0, 0)` — beide Kinder auf 0 hiesse einen Zyklus ueber
    // die Wurzel. Ein echter Knoten hat zwei verschiedene Indizes, die
    // Bedingung ist also genau „unvollstaendig".
    let mut offen = vec![0usize];
    let mut gesehen = vec![false; knoten.len()];
    while let Some(k) = offen.pop() {
        gesehen[k] = true;
        if let Knoten::Ast(klein, gross) = knoten[k] {
            // GEMESSENER FEHLER: die erste Fassung pruefte `klein == gross
            // && klein == 0`. Das erkennt einen **leeren** Ast (Ast(0,0)),
            // aber nicht einen Ast mit nur EINEM Kind — und genau der ist
            // der Fall „zwei Eintraege zu je 2 Bit": beide Codewoerter
            // („00" und „01") liegen unter demselben Ast, dessen 1-Kind
            // noch auf 0 zeigt. Index 0 ist die Wurzel, und ein echter
            // Kindverweis zeigt nie auf die Wurzel — also heisst 0
            // zuverlaessig „nicht angelegt".
            if klein == 0 || gross == 0 {
                return Err(Fehler::neu(
                    Art::FalscherWert,
                    format!(
                        "Buch ab Byte {start}: der Huffman-Baum ist unterbevolkert — Knoten {k} \
                         hat kein zweites Kind"
                    ),
                ));
            }
            offen.push(klein);
            offen.push(gross);
        }
    }
    // M3: nach der Leerstellen-Pruefung kann kein Kind mehr auf die
    // Wurzel zeigen; ein zweiter Besuch desselben Knotens hiesse, dass
    // sich zwei Aeste einen Knoten teilen — das waere ein geteilter Baum.
    Ok((knoten, codewoerter))
}

// GEMESSENE Korrektur: `ziffern(1 - 1, n)` sieht nach einem Versehen aus
// und ist es **nicht**. Es ist die Schreibweise des Vorbis-Entropie-
// Decoders: die erste Eins wird **mitgezaehlt**, und `1 - 1` sagt das.
// Wer daraus `ziffern(0, n)` macht, bekommt dieselben Bits — und
// verliert die Information, dass hier eine fuehrende Eins stand.
//
// clippy `eq_op` weiss das nicht und meldet „equal expressions as
// operands to `-`". Also wird die Warnung hier **an einer Stelle**
// abgestellt, statt sie an sieben Fundstellen einzeln zu ueberschreiben.
#[allow(clippy::eq_op)]
#[cfg(test)]
mod tests {
    use super::*;

    /// Packt eine Liste von Bit-Ziffern in Bytes, LSB-first.
    ///
    /// GEMESSENER FEHLER, der hier stand: der Helfer schob JEDES Bit als
    /// eigenes Byte. `BitLeser` liest aber Bit `i` aus Byte `i / 8` an
    /// Position `i % 8` — also sieht der Leser bei „ein Byte pro Bit" in
    /// jedem Byte nur Bit 0 und liest Nullen.
    fn packiere(bits: &[u8]) -> Vec<u8> {
        let mut raus = vec![0u8; bits.len().div_ceil(8)];
        for (i, &b) in bits.iter().enumerate() {
            if b & 1 == 1 {
                raus[i / 8] |= 1 << (i % 8);
            }
        }
        raus
    }

    /// Die unteren `n` Bit einer Zahl als Ziffernliste (LSB zuerst).
    fn ziffern(wert: u32, n: u32) -> Vec<u8> {
        (0..n).map(|i| ((wert >> i) & 1) as u8).collect()
    }

    /// Der Kopf, den jedes Codebuch hat (Abschnitt 3.2.1).
    fn kopf(dimensionen: u32, eintraege: u32) -> Vec<u8> {
        let mut bits = Vec::new();
        bits.extend(ziffern(SYNC, 24));
        bits.extend(ziffern(dimensionen, 16));
        bits.extend(ziffern(eintraege, 24));
        bits
    }

    /// Die Bits eines kleinen Gitterbuchs: 2 Dimensionen, 2 Eintraege
    /// (die beiden Laengen sind 1, also zwei Codewoerter zu je 2 Bit),
    /// Art 1, minimum = 0, delta = 1, wertbits = 15, sequenz = 0, ein
    /// Wert = 7 (`lookup1_values(2,2) = 1`).
    fn gitter_bits() -> Vec<u8> {
        let mut bits = kopf(2, 2);
        bits.push(0); // ordered = 0
        bits.push(0); // sparse = 0
        // GEMESSENE Korrektur einer clippy-Warnung: `1 - 1` ist `0`, und
        // die Absicht ist hier `0` (Laenge 1, 5 Bit einer Zahl mit einem
        // gesetzten Bit -> Wert 0 mit einer führenden 1).
        bits.extend(ziffern(1 - 1, 5)); // Laenge 1
        bits.extend(ziffern(1 - 1, 5)); // Laenge 1
        bits.extend(ziffern(1, 4)); // Art 1
        bits.extend(ziffern(788u32 << 21, 32)); // minimum = 0
        bits.extend(ziffern((788u32 << 21) | 1, 32)); // delta = 1
        bits.extend(ziffern(15 - 1, 4)); // wertbits = 15
        bits.push(0); // sequenz = 0
        bits.extend(ziffern(7, 15));
        bits
    }

    #[test]
    fn ein_zweites_buch_steht_genau_dahinter() {
        // Das ist der Test, an dem die erste Fassung scheiterte: sie las
        // die Wertetabelle-Art je Eintrag und stand danach mitten im
        // zweiten Buch. Zwei Buecher liegen im echten Setup-Kopf als EIN
        // Bitstrom hintereinander — nicht als zwei Bytefolgen.
        //
        // GEMESSENE KORREKTUR AN DER DATEI: hier standen zwei gepackte
        // Bytefolgen hintereinander. Das erste Buch ist 164 Bit lang, das
        // gepackte Byte ist aber 21 Byte (168 Bit), also begann das
        // zweite Buch vier Bit zu spaet und der Leser lief aus dem Takt.
        // Genau das ist der Unterschied zwischen „Paket" (ein Byteblock)
        // und „Codebuch" (Bitstrom, nicht byte-aligned).
        let mut bits = gitter_bits();
        bits.extend(gitter_bits());
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        let a = lies_codebuch(&mut l).expect("buch 1");
        let b = lies_codebuch(&mut l).expect("buch 2");
        assert_eq!(a.dimensionen, 2);
        assert_eq!(b.dimensionen, 2);
        assert_eq!(a.eintraege, 2);
        assert_eq!(b.eintraege, 2);
        assert_eq!(
            l.position(),
            2 * 164,
            "nach zwei Buechern muss der Leser bei Bit 2 mal 164 stehen"
        );
        assert_eq!(a.vektor_ab(0).expect("v0"), vec![7.0, 7.0]);
        assert_eq!(b.vektor_ab(0).expect("v0"), vec![7.0, 7.0]);
    }

    #[test]
    fn ein_buch_der_art_0_hat_keine_werte() {
        let mut bits = kopf(1, 2);
        bits.push(0);
        bits.push(0);
        bits.extend(ziffern(0, 5)); // Laenge 1
        bits.extend(ziffern(0, 5)); // Laenge 1
        bits.extend(ziffern(0, 4)); // Art 0
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        let b = lies_codebuch(&mut l).expect("codebuch");
        assert_eq!(b.art(), WertArt::Ohne);
        assert_eq!(b.wertzahl(), 0);
        // Und ein Vektor daraus ist nach Abschnitt 3.3 verboten.
        assert!(b.lies_vektor(&mut l).is_err());
    }

    #[test]
    fn ein_gitterbuch_baut_seine_vektoren() {
        // Art 1, 2 Dimensionen, 4 Eintraege, Laengen alle 2 (die vier
        // Codewoerter 00, 01, 10, 11). minimum = 0, delta = 1,
        // sequenz = 0. lookup1_values(4, 2) = 2 -> zwei Werte.
        //
        // GEMESSENE KORREKTUR AN DEN LAENGEN: hier standen vier
        // Eintraege zu je **1** Bit. Ein 1-Bit-Baum hat genau zwei
        // Plaetze — der Leser meldete zu Recht „der Huffman-Baum ist
        // ueberbevolkert (Codewort 2)". Vier Eintraege brauchen Laenge 2.
        let mut bits = kopf(2, 4);
        bits.push(0);
        bits.push(0);
        for _ in 0..4 {
            bits.extend(ziffern(2 - 1, 5)); // Laenge 2 -> Codewoerter 00,01,10,11
        }
        bits.extend(ziffern(1, 4)); // Art 1
        bits.extend(ziffern(788u32 << 21, 32)); // minimum 0
        bits.extend(ziffern((788u32 << 21) | 1, 32)); // delta 1
        bits.extend(ziffern(15 - 1, 4));
        bits.push(0); // sequenz 0
        bits.extend(ziffern(3, 15)); // wert[0] = 3
        bits.extend(ziffern(5, 15)); // wert[1] = 5
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        let b = lies_codebuch(&mut l).expect("codebuch");
        assert_eq!(b.art(), WertArt::Gitter);
        assert_eq!(b.wertzahl(), 2);
        // Abschnitt 3.2.1, „Vector value decode: Lookup type 1":
        //   vektor[i] = wert[(index / divisor) % lookup_values] * delta
        //               + minimum + last
        assert_eq!(b.vektor_ab(0).expect("vektor"), vec![3.0, 3.0]);
        assert_eq!(b.vektor_ab(1).expect("vektor"), vec![5.0, 3.0]);
        assert_eq!(b.vektor_ab(2).expect("vektor"), vec![3.0, 5.0]);
        assert_eq!(b.vektor_ab(3).expect("vektor"), vec![5.0, 5.0]);
    }

    #[test]
    fn ein_ausdrueckliches_buch_liest_je_wert_seinen_eigenen() {
        // Art 2: eintraege * dimensionen Werte, in dieser Reihenfolge.
        // Drei Eintraege brauchen die Laengen 1, 2, 2 — ein 1-Bit-Baum hat
        // nur zwei Plaetze (GEMESSEN: der Leser hat die drei Laengen-1-
        // Eintraege als ueberbevolkert gemeldet, und das war richtig).
        let mut bits = kopf(2, 3);
        bits.push(0);
        bits.push(0);
        bits.extend(ziffern(1 - 1, 5)); // Laenge 1
        bits.extend(ziffern(2 - 1, 5)); // Laenge 2
        bits.extend(ziffern(2 - 1, 5)); // Laenge 2
        bits.extend(ziffern(2, 4)); // Art 2
        bits.extend(ziffern(788u32 << 21, 32));
        bits.extend(ziffern((788u32 << 21) | 1, 32));
        bits.extend(ziffern(15 - 1, 4));
        bits.push(0);
        for w in [1u32, 2, 3, 4, 5, 6] {
            bits.extend(ziffern(w, 15));
        }
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        let b = lies_codebuch(&mut l).expect("codebuch");
        assert_eq!(b.wertzahl(), 6);
        assert_eq!(b.vektor_ab(0).expect("v0"), vec![1.0, 2.0]);
        assert_eq!(b.vektor_ab(1).expect("v1"), vec![3.0, 4.0]);
        assert_eq!(b.vektor_ab(2).expect("v2"), vec![5.0, 6.0]);
    }

    #[test]
    fn die_sequenz_addiert_auf() {
        // sequenz = 1: jeder Wert wird auf den vorherigen addiert.
        let mut bits = kopf(2, 2);
        bits.push(0);
        bits.push(0);
        for _ in 0..2 {
            bits.extend(ziffern(0, 5));
        }
        bits.extend(ziffern(2, 4));
        bits.extend(ziffern(788u32 << 21, 32));
        bits.extend(ziffern((788u32 << 21) | 1, 32));
        bits.extend(ziffern(15 - 1, 4));
        bits.push(1); // sequenz = 1
        for w in [10u32, 20, 30, 40] {
            bits.extend(ziffern(w, 15));
        }
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        let b = lies_codebuch(&mut l).expect("codebuch");
        // Abschnitt 3.2.1, „Lookup type 2": versatz = index * dimensionen.
        // index 0 -> [10, 20+10] = [10, 30]; index 1 -> [30, 40+30] = [30, 70].
        //
        // GEMESSENE KORREKTUR AN DER ERWARTUNG: hier stand [20, 50] und
        // [10, 30]. Bei sequenz = 1 addiert **jeder** Wert auf den
        // vorherigen des GLEICHEN Vektors — index 1 beginnt also bei
        // wert[2] = 30, nicht bei wert[1] = 20.
        assert_eq!(b.vektor_ab(0).expect("v0"), vec![10.0, 30.0]);
        assert_eq!(b.vektor_ab(1).expect("v1"), vec![30.0, 70.0]);
    }

    #[test]
    fn ein_duennes_buch_ueberspringt_eintraege() {
        // sparse = 1: die Eintraege, deren Bit 0 ist, haben keine
        // Laenge und bekommen kein Codewort.
        let mut bits = kopf(1, 3);
        bits.push(0); // ordered
        bits.push(1); // sparse
        bits.push(1); // Eintrag 0 da
        bits.extend(ziffern(0, 5)); // Laenge 1
        bits.push(0); // Eintrag 1 unbenutzt
        bits.push(1); // Eintrag 2 da
        bits.extend(ziffern(0, 5)); // Laenge 1
        bits.extend(ziffern(0, 4)); // Art 0
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        let b = lies_codebuch(&mut l).expect("codebuch");
        assert_eq!(b.benutzt(), 2);
        // Die beiden benutzten Eintraege bekommen die kuerzesten
        // Codewoerter: „0" und „10" — Eintrag 1 kann nicht dazwischen.
        for (ziffern_liste, erwartet) in [(vec![0u8], 0usize), (vec![1u8, 0], 2)] {
            let bytes2 = packiere(&ziffern_liste);
            let mut l2 = BitLeser::neu(&bytes2);
            assert_eq!(b.lies_symbol(&mut l2).expect("symbol"), erwartet);
        }
    }

    #[test]
    fn ein_sortiertes_buch_liest_die_laenge_nur_einmal() {
        // ordered = 1 (Abschnitt 3.2.1):
        //
        //   2) [current_length] = read a five bit unsigned integer and add 1;
        //   3) [number] = read ilog(entries - current_entry) bits
        //   4) setze [current_entry .. current_entry+number-1] auf die Laenge
        //   6) current_length += 1
        //   8) if (current_entry < entries) repeat starting at **3**
        //
        // DER PUNKT, an dem mein Test zweimal danebenlag:
        //
        // 1) Die Schleife beginnt bei Schritt **3**, nicht bei Schritt 2.
        //    Die Laenge wird also **einmal** gelesen und danach nur
        //    hochgezaehlt. Mein Test schrieb zweimal ein 5-Bit-Laengenfeld.
        // 2) Vier Eintraege mit den Laengen [1,1,2,2] sind **ueber**-
        //    bevölkert: zwei 1-Bit-Cewoerter („0" und „1") fuellen den
        //    ganzen 1-Bit-Baum, fuer ein laengeres Codewort bleibt nichts.
        //    Der Leser meldete zu Recht „Laenge 2 passt nicht mehr
        //    (Codewort 4)".
        //
        // Sechs Eintraege mit [2,2,3,3,3,3] ergeben dagegen einen
        // vollstaendigen Baum mit sechs Blaettern:
        //
        // ```text
        //   00, 01, 100, 101, 110, 111
        // ```
        let mut bits = kopf(1, 6);
        bits.push(1); // ordered = 1
        bits.extend(ziffern(2 - 1, 5)); // Laenge 2, EINMAL gelesen
        bits.extend(ziffern(2, ilog(6))); // zwei Glieder
        // Jetzt zaehlt die Spec nur noch die Laenge hoch (Schritt 6) und
        // beginnt wieder bei Schritt 3 — die Breite ist ilog(6 - 2).
        // GEMESSENE KORREKTUR AN DER ZAHL: hier stand `ziffern(2, ilog(4))`
        // — zwei Glieder statt vier. Der Leser las 2, kam nach zwei
        // Runden auf pos 4 und suchte eine dritte Runde („0 Glieder bei
        // Laenge 4 (pos 4)").
        bits.extend(ziffern(4, ilog(4))); // noch vier mit Laenge 3
        bits.extend(ziffern(0, 4)); // Art 0
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        let b = lies_codebuch(&mut l).expect("codebuch");
        assert_eq!(b.benutzt(), 6);
        // GEMESSENE ENTSCHEIDUNG, warum die **Position** geprueft wird und
        // nicht „kein Rest": ein Codebuch endet mitten im Byte — die
        // restlichen Bit gehoeren im echten Setup-Kopf dem naechsten.
        // 24+16+24 Kopf, 1 Bit ordered, 5 Bit Laenge, 3+3 Bit Anzahlen,
        // 4 Bit Art = 80.
        assert_eq!(
            l.position(),
            64 + 1 + 5 + 3 + 3 + 4,
            "das Buch muss bei Bit 80 enden, sonst ist eine Feldbreite falsch"
        );
        // Und die sechs Codewoerter der Spec-Regel „niedrigstes noch
        // freies Codewort".
        for (ziffern_liste, erwartet) in [
            (vec![0u8, 0], 0usize),
            (vec![0u8, 1], 1),
            (vec![1u8, 0, 0], 2),
            (vec![1u8, 0, 1], 3),
            (vec![1u8, 1, 0], 4),
            (vec![1u8, 1, 1], 5),
        ] {
            let bytes2 = packiere(&ziffern_liste);
            let mut l2 = BitLeser::neu(&bytes2);
            assert_eq!(b.lies_symbol(&mut l2).expect("symbol"), erwartet);
        }
    }

    #[test]
    fn null_eintraege_werden_abgelehnt() {
        // Abschnitt 3.2.1: „if ([entries] < 1) return error".
        let bytes = packiere(&kopf(1, 0));
        let mut l = BitLeser::neu(&bytes);
        assert!(lies_codebuch(&mut l).is_err());
    }

    #[test]
    fn null_dimensionen_werden_abgelehnt() {
        // GEMESSENE ENTSCHEIDUNG: `dimensions` wird gelesen und sonst nur
        // fuer die Wertetabelle benutzt. Fuer Art 0 (keine Wertetabelle)
        // koennte man 0 zulassen — dann waere der Vektor „0 Zahlen lang",
        // und ein Codebuch, das keinen Vektor liefern kann, ist keins.
        let bytes = packiere(&kopf(0, 4));
        let mut l = BitLeser::neu(&bytes);
        assert!(lies_codebuch(&mut l).is_err());
    }

    #[test]
    fn eine_fehlende_sync_ist_ein_benannter_fehler_mit_stelle() {
        let bytes = vec![0u8; 64];
        let mut l = BitLeser::neu(&bytes);
        match lies_codebuch(&mut l) {
            Err(f) => {
                assert_eq!(f.art, Art::FalschesMuster);
                assert!(f.text.contains("Byte 0"), "die Stelle steht im Text: {f}");
            }
            Ok(_) => panic!("erwartet einen Fehler"),
        }
    }

    #[test]
    fn eine_unbekannte_art_ist_ein_benannter_fehler() {
        // Art 3 gibt es nicht („a codebook_lookup_type of greater than
        // two is reserved").
        let mut bits = kopf(1, 2);
        bits.push(0);
        bits.push(0);
        bits.extend(ziffern(0, 5));
        bits.extend(ziffern(0, 5));
        bits.extend(ziffern(3, 4));
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        match lies_codebuch(&mut l) {
            Err(f) => {
                assert_eq!(f.art, Art::FalscherWert);
                assert!(f.text.contains('3'), "{f}");
            }
            Ok(_) => panic!("erwartet einen Fehler"),
        }
    }

    #[test]
    fn ein_ueberbevölkerter_baum_ist_ein_benannter_fehler() {
        // Neun Eintraege zu je 3 Bit: ein 3-Bit-Baum hat genau acht
        // Plaetze, der neunte passt nicht mehr.
        //
        // GEMESSENE KORREKTUR AN DER ZAHL: hier standen SIEBEN. Die sind
        // nicht ueber-, sondern **unter**bevolkert (7 von 8 Blaettern
        // belegt) — und die Diagnose, die der Leser dafuer gab, war
        // „unterbevolkert". Richtig war die Meldung, falsch der Test.
        let mut bits = kopf(1, 9);
        bits.push(0);
        bits.push(0);
        for _ in 0..9 {
            bits.extend(ziffern(2, 5)); // Laenge 3
        }
        bits.extend(ziffern(0, 4));
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        match lies_codebuch(&mut l) {
            Err(f) => {
                assert_eq!(f.art, Art::FalscherWert);
                assert!(
                    f.text.contains("überbev") || f.text.contains("ueberbev"),
                    "der Text muss die Ursache nennen: {f}"
                );
            }
            Ok(_) => panic!("erwartet einen Fehler"),
        }
    }

    #[test]
    fn ein_unterbevölkerter_baum_ist_ein_benannter_fehler() {
        // Zwei Eintraege zu je 2 Bit: die beiden kuerzesten Codewoerter
        // sind „00" und „01" — beide unter demselben Ast. Der Baum hat
        // also einen Ast mit nur einem Kind, und das ist nach Abschnitt
        // 3.2.1 „an underspecified tree".
        let mut bits = kopf(1, 2);
        bits.push(0);
        bits.push(0);
        bits.extend(ziffern(1, 5)); // Laenge 2
        bits.extend(ziffern(1, 5)); // Laenge 2
        bits.extend(ziffern(0, 4));
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        match lies_codebuch(&mut l) {
            Err(f) => {
                assert_eq!(f.art, Art::FalscherWert);
                // GEMESSENE ENTSCHEIDUNG: hier stand `contains("unterbev")`.
                // Der Fehler kommt jetzt **frueher** und mit einer anderen
                // Begruendung: die Kraft-Summe der Laengen 2 und 2 ist
                // 2*2^-2 = 0,5 und damit nicht 1. Beides ist „kein
                // Huffman-Baum", aber die Kraft-Summe ist die
                // vollstaendige Aussage — sie sieht auch die Faelle, in
                // denen sich zwei Codewoerter kreuzen und kein Kind fehlt.
                assert!(f.text.contains("Kraft-Summe"), "{f}");
            }
            Ok(_) => panic!("erwartet einen Fehler"),
        }
    }

    #[test]
    fn ein_einzelnes_codebuch_senkt_genau_ein_bit() {
        // Errata 20150226: „Reading' a value from single-entry codebook
        // always returns the single used codeword and sinks one bit.
        // Decoders should tolerate that the bit read from the stream be
        // '1' instead of '0'".
        let mut bits = kopf(1, 1);
        bits.push(0);
        bits.push(0);
        bits.extend(ziffern(0, 5)); // Laenge 1
        bits.extend(ziffern(0, 4)); // Art 0
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        let b = lies_codebuch(&mut l).expect("codebuch");
        assert_eq!(b.benutzt(), 1);
        for bit_ziffer in [0u8, 1] {
            let bytes2 = packiere(&[bit_ziffer]);
            let mut l2 = BitLeser::neu(&bytes2);
            assert_eq!(
                b.lies_symbol(&mut l2).expect("symbol"),
                0,
                "Bit {bit_ziffer} muss denselben Eintrag liefern"
            );
            assert_eq!(l2.position(), 1, "genau ein Bit gelesen");
        }
    }

    #[test]
    fn ein_einzelnes_codebuch_mit_laenge_2_wird_abgelehnt() {
        // Errata 20150226: „Decoder implementations shall reject a
        // codebook if it contains only one used entry and the encoded
        // [codeword_length] of that entry is not 1."
        let mut bits = kopf(1, 1);
        bits.push(0);
        bits.push(0);
        bits.extend(ziffern(1, 5)); // Laenge 2
        bits.extend(ziffern(0, 4));
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        match lies_codebuch(&mut l) {
            Err(f) => assert!(f.text.contains("Errata"), "{f}"),
            Ok(_) => panic!("erwartet einen Fehler"),
        }
    }

    #[test]
    fn zu_viele_eintraege_werden_abgelehnt() {
        // M4: das Feld ist 24 Bit breit, aber ein Buch mit 2^24
        // Eintraegen braucht einen Baum mit 2^25 Knoten. Die Grenze muss
        // benannt sein, sonst entscheidet die Datei ueber den Speicher.
        let mut bits = kopf(1, (1u32 << 24) - 1);
        bits.push(0);
        bits.push(0);
        bits.extend(ziffern(0, 5));
        bits.extend(ziffern(0, 4));
        let bytes = packiere(&bits);
        let mut l = BitLeser::neu(&bytes);
        match lies_codebuch(&mut l) {
            Err(f) => assert_eq!(f.art, Art::ZahlZuGross),
            Ok(_) => panic!("erwartet einen Fehler"),
        }
    }

    #[test]
    fn die_grenzen_stehen_in_der_norm() {
        assert_eq!(MAX_LAENGE, 32);
        assert_eq!(MAX_BUECHER, 256);
        assert!(MAX_EINTRAEGE <= (1 << 24));
        assert!(MAX_WERTE <= (1 << 24));
        assert!((1usize << 24) > MAX_EINTRAEGE);
        assert!((1usize << 24) * (1usize << 16) > MAX_WERTE);
    }
}
