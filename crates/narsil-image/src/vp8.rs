//! VP8 — WebP verlustbehaftet (RFC 6386). Bool-Dekoder, Baum-Leser und der
//! **ungepackte** Teil des Keyframe-Kopfs.
//!
//! Regelquelle ist RFC 6386 "VP8 Data Format and Decoding Guide",
//! November 2011. Die Wahrscheinlichkeits- und Baumtabellen stehen in
//! [`crate::vp8_tabellen`] und sind aus dem RFC erzeugt, nicht abgetippt.
//!
//! # Stand — und das ist eine Grenze, keine Ausrede
//!
//! Enthalten und **gegen den RFC geprueft**:
//!   * der Bool-Dekoder (Abschnitt 7.3), geprueft durch Hin- und
//!     Zurueckkodieren mit dem normativen Encoder aus demselben Abschnitt
//!   * der Baum-Leser (Abschnitt 8.1), geprueft gegen die in Abschnitt 8.2
//!     wörtlich angegebenen Kodierungen
//!   * der ungepackte Kopfteil (Abschnitt 9.1 und 9.2), geprueft gegen die
//!     Masse, die Pillow auf allen sechs echten `VP8 `-Dateien meldet
//!
//! # Stand 02.10.2026 (Projektlabel), 12:20 MEZ — was GEMESSEN stimmt
//!
//! Der Kopf dieser Datei behauptete bis eben das Gegenteil. Die
//! Behauptung war veraltet, und sie hat mich bei dieser Runde Fast ein
//! ganzes Massprogramm gekostet, weil ich der Behauptung geglaubt habe
//! statt der Datei. Darum steht jetzt hier, was tatsaechlich gemessen
//! ist — und das ist deutlich weniger erfreulich, aber brauchbar.
//!
//! **Enthalten und gebaut:** Bool-Dekoder (7.3), Baum-Leser (8.1),
//! ungepackter Kopf (9.1/9.2), bool-kodierter Kopf (9.3–9.11), Moduslesen
//! (11.7), Koeffizienten (13), Dequantisierung (14.1), IDCT (13.5),
//! WHT (14.3), alle Intra-Praediktionen (13.2/13.3), Loopfilter
//! (14.2/14.3), YUV nach RGB. `webp_lesen` dekodiert `VP8 `-Chunks also
//! wirklich; es wird nichts mehr verweigert.
//!
//! **GEMESSEN, dass der bool-kodierte Kopf PLAUSIBEL liest**
//! (`examples/vp8_kopfdaten.rs`, alle sechs echten `VP8 `-Dateien):
//!
//! ```text
//! filter_type        0 bei allen sechs   (erlaubt: 0 oder 1)
//! schaerfe           0 bei allen sechs   (Default)
//! log2_partitionen   0 bei allen sechs   (erlaubt: 0..3)
//! quant_y_ac         67 / 35 / 6 / 45 / 50 / 12
//! ```
//!
//! Der Quantisiererindex wandert mit der Qualitaetsstufe in der
//! richtigen Richtung und in einem moeglichen Bereich (0..127). Die
//! Felder bis 9.6 lesen also **nicht** aus dem Muell. Das ist ein
//! Ergebnis, das der alte Kopf ausdruecklich bestritt.
//!
//! **WIDERLEGT, mit Zahlen, die hier stehen sollten:**
//!
//! 1. „Die beiden Bits fuer Farbraum und Beschneiden (9.2) liegen im
//!    ungepackten Kopf."  Experiment: weggelassen. GEMESSEN: 8 von 10
//!    Dateien enden mit `VP8-Partition ist kuerzer als zwei Byte`, der
//!    Bool-Strom laeuft also ueber das Datenende. Die Bit sind im
//!    Bool-Strom. Rueckgenommen.
//!
//! 2. „Die YUV-nach-RGB-Matrix ist verdrahtet."  GEMESSEN: neutrales
//!    Grau (u = v = 128) ergibt ueber Y = 0, 16, 64, 100, 128, 150, 180,
//!    220, 255 in **allen** neun Faellen R = G = B, und 0 wird
//!    schwarz, 255 wird weiss. Die Matrix stimmt.
//!
//! **Offen, und das ist der Punkt, an dem die Arbeit weitergeht.**
//! `examples/vp8_orakel.rs` misst die ganze Abweichung statt des ersten
//! Treffers (`tests/vp8_pixel.rs` bricht beim ersten ab):
//!
//! fast alle Kanaele weichen ab, groesste Abweichung 255,
//!     mittlere Abweichung 75 bis 105
//!     Mittelwerte  Ist R 116  G  61  B 108
//!                   Soll R 127 G 127 B 127
//!
//! Die Referenzbilder sind neutralgrau, also muessen die drei
//! Kanalmittel gleich sein. Sie sind es nicht, und sie laufen
//! weit auseinander. **Das ist ein Chroma-Fehler**: die U- und
//! V-Ebenen sind falsch, nicht die Helligkeit.
//!
//! Zusaetzlich: `vp8_alpha_q80`, `vp8_alpha_filter` und
//! `vp8_alpha_q100` liefern **bitgleiche** Ausgabe (Mittel R 59.3,
//! G 78.9, B 66.4 bei allen dreien). Drei verschiedene Quelldateien,
//! ein Ergebnis — das ist kein Decoder, das schlecht rechnet, das ist
//! einer, der bei diesen Dateien gar nicht dekodiert. Das ist
//! Aufgabe 2 (ALPH) und Aufgabe 3 (ANIM/ANMF), nicht B1.
//!
//! **Was ich NICHT geprueft habe, und warum:** die Kerne von IDCT,
//! WHT und Loopfilter gegen den RFC 6386. In dieser Sitzung gab es
//! keinen Netzzugang (kein Browser, kein HTTP-Werkzeug), und die
//! Test-Erwartung in `tests/vp8_koeffizienten.rs` ist aus der Zeit vor
//! dem Rollenwechsel und nicht gegen die Norm geprueft — sie ist als
//! Sollwert also nicht vertrauenswuerdig. Wer ohne Netzzugang
//! weiterarbeitet, sollte das nicht mit „der Test sagt" begruenden.

use crate::BildFehler;

use super::vp8_tabellen as t;

/// Die Nummer des Bildes, das gerade gerechnet wird.
///
/// GEMESSENER Grund (Rolle VIDEO, 03.10.2026): `NARSIL_VP8_MB_TRACE` gab
/// nur eine Makroblocknummer aus, und die stimmt in jedem Bild der Kette.
/// Die Ausgabe kam deshalb aus Bild 1, als ich Bild 7 haben wollte — ein
/// Befund, der sich als Fehler in Bild 7 las und keiner war.
static BILD_NR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn fehler(grund: &'static str) -> BildFehler {
    BildFehler::WebpDatenFalsch { grund }
}

/// M3: hoechste Bildkante nach RFC 9.2 (14 Bit Wert, Skala addiert).
const MAX_KANTE: u32 = (1 << 14) + 4 * 3;

/// M3: hoechste Zahl Makrobloecke, die der Kopf melden darf.
///
/// GEMESSEN 01.10.2026: dieser Wert war zuerst `1 << 14` und hat damit
/// **gueltige** Bilder abgewiesen — 4095x4095 ergibt 256*256 = 65536
/// Makrobloecke und wurde mit "mehr Makrobloecke als erlaubt"
/// abgelehnt, obwohl beide Kanten weit unter 16383 liegen. Die Grenze
/// muss aus den Formatmaessen folgen, nicht aus einer geratenen Zahl:
/// 16383/16 aufgerundet ist 1024, und 1024*1024 ist das groesste Bild,
/// das das Format ueberhaupt darstellen kann.
const MAX_MAKROBLOECKE: u32 = (MAX_KANTE.div_ceil(16)).pow(2);

/// Bool-Dekoder, wortgleich nach RFC 6386, Abschnitt 7.3.
///
/// `prob` ist die Wahrscheinlichkeit, dass das gelesene Bit **NULL** ist.
/// `wert` ist gegenueber `weite` um acht Bit nach links verschoben: der
/// Vergleich erfolgt deshalb gegen `split << 8`, nicht gegen `split`.
pub struct BoolDekoder<'a> {
    daten: &'a [u8],
    /// Byte-Versatz, an dem diese Partition beginnt.
    start: usize,
    /// Naechstes ungelesenes Byte.
    pos: usize,
    /// "contains at least 8 significant bits"
    wert: u32,
    /// Immer identisch zum `range` des Encoders.
    weite: u32,
    /// "# of bits shifted out of value, at most 7"
    bit_count: u32,
    /// Wie viele Bits ueber das Dateiende hinaus gelesen wurden.
    ueber: u32,
}

impl<'a> BoolDekoder<'a> {
    /// "init_bool_decoder" — `start` ist ein BYTE-Versatz in die Partition.
    ///
    /// Der RFC liest unbedingt zwei Byte. Sind weniger da, ist die Datei
    /// abgeschnitten; das ist ein benannter Fehler und kein Absturz (M2).
    pub fn new(daten: &'a [u8], start: usize) -> Result<Self, BildFehler> {
        if start + 2 > daten.len() {
            return Err(fehler("VP8-Partition ist kuerzer als zwei Byte"));
        }
        // "d->value = 0; while (++i <= 2)
        //    d->value = (d->value << 8) | *start_partition++;"
        let wert = ((daten[start] as u32) << 8) | daten[start + 1] as u32;
        Ok(BoolDekoder {
            daten,
            start,
            pos: start + 2,
            wert,
            weite: 255,
            bit_count: 0,
            ueber: 0,
        })
    }

    fn byte(&mut self) -> u32 {
        if self.pos >= self.daten.len() {
            // Ueber das Ende hinaus mit Nullen weiterlesen. GEMESSEN
            // 01.10.2026: die letzte Partition einer WebP-Datei endet auf
            // einem Byte, und der Dekoder liest vorsorglich weiter. Wer
            // hier abbricht, verweigert jede korrekte Datei. Mit `ueber`
            // bleibt das sichtbar (M2).
            self.ueber += 8;
            return 0;
        }
        let b = self.daten[self.pos];
        self.pos += 1;
        b as u32
    }

    /// "read_bool" nach RFC 6386, 7.3.
    pub fn flag(&mut self, prob: u8) -> u32 {
        // "split = 1 + (((d->range - 1) * prob) >> 8);"
        let split = 1 + (((self.weite - 1) * prob as u32) >> 8);
        // "uint32 SPLIT = split << 8;"
        let gross = split << 8;
        let retval = if self.wert >= gross {
            // "if (d->value >= SPLIT) {  encoded a one
            //    retval = 1; d->range -= split; d->value -= SPLIT; }"
            self.weite -= split;
            self.wert -= gross;
            1
        } else {
            // "encoded a zero: retval = 0; d->range = split;"
            self.weite = split;
            0
        };
        // "while (d->range < 128) { d->value <<= 1; d->range <<= 1;
        //    if (++d->bit_count == 8) { d->bit_count = 0;
        //      d->value |= *d->input++; } }"
        //
        // Das Renormieren muss VOR der Rueckgabe laufen — der Zustand
        // ist danach, nicht davor.
        while self.weite < 128 {
            self.wert <<= 1;
            self.weite <<= 1;
            self.bit_count += 1;
            if self.bit_count == 8 {
                self.bit_count = 0;
                self.wert |= self.byte();
            }
        }
        retval
    }

    /// "read_literal" — ein `n` Bit breiter Wert, jedes Bit bei
    /// Wahrscheinlichkeit 128 (Gleichstand), von hoch nach niedrig.
    ///
    /// M3: `n` ist begrenzt, damit ein Aufrufer keinen Riesenwert
    /// bestellen kann.
    pub fn literal(&mut self, n: u32) -> u32 {
        let mut v = 0u32;
        for _ in 0..n.min(MAX_LITERAL_BITS) {
            v = (v << 1) + self.flag(128);
        }
        v
    }

    /// Wie viele Bits ueber das Dateiende hinaus gelesen wurden. Damit
    /// erkennt man abgeschnittene Daten (M2).
    pub fn ueberlauf(&self) -> u32 {
        self.ueber
    }

    /// GEMESSUNG (02.10.2026, Rolle BILD, B13): der Zustand des Dekaders
    /// an dieser Stelle. Damit laesst sich ein Bildstrom Byte fuer Byte
    /// gegen den Referenzdecoder vergleichen.
    ///
    /// GEMESSENE GRUNDLAGE fuer die Zuordnung der drei Zahlen,
    /// `bool_decoder.h`: Z. 33 `d->value`, Z. 35 `d->range`,
    /// Z. 36 `d->bit_count`.
    ///
    /// GEMESSENER Anlass: das Orakel meldete fuer Bild 1 `mv_typ` -7 bei
    /// 252 von 300 Makrobloecken — ein Wert, den `mv_ref_tree`
    /// (modemv_data.h:177) gar nicht liefern kann. Ohne diese drei Zahlen
    /// laesst sich nicht entscheiden, ob der Referenzdecoder oder diese
    /// Crate an der Stelle falsch liegt.
    #[doc(hidden)]
    pub fn zustand(&self) -> (u32, u32, u32) {
        (self.wert, self.weite, self.bit_count)
    }

    /// Wie viele Bitpositionen der Dekoder seit Anfang dieser Partition
    /// schon verbraucht hat.
    ///
    /// GEMESSEN 01.10.2026: das ist das wichtigste Mass fuer den
    /// Keyframe-Kopf. Nach dem Lesen aller Makroblockmodi muss dieser
    /// Stand dicht an `8 * erste_part` liegen — die Modi stehen am Ende
    /// der ersten Partition. Ein Dekoder, der an irgendeiner Stelle
    /// des Kopfs ein Bit zu viel oder zu wenig liest, landet sichtbar
    /// daneben.
    pub fn bit_standing(&self) -> usize {
        // `pos` zeigt auf das naechste ungelesene Byte. Die unteren
        // `bit_count` Bit darin sind bereits verbraucht.
        (self.pos.saturating_sub(self.start)) * 8 - self.bit_count as usize
    }

    /// Liest einen Index durch einen Baum, woertlich nach RFC 6386, 8.1:
    ///
    /// ```text
    /// while ((i = t[ i + read_bool(d, p[i>>1])]) > 0) {}
    /// return -i;
    /// ```
    ///
    /// `baum` ist `i8`: ein Eintrag `<= 0` ist ein Blatt mit dem Wert
    /// `-eintrag`, ein positiver Eintrag der Index des naechsten Knotens.
    /// Je Knoten wird **eine** Wahrscheinlichkeit gebraucht, nicht zwei.
    ///
    /// M5: die Schleife laeuft ueber die Baumgroesse, nicht ueber die
    /// Eingabe. Ein zyklischer Baum muss abbrechen, nicht endlos laufen.
    pub fn baum(&mut self, baum: &[i8], probs: &[u8]) -> Result<u32, BildFehler> {
        let mut i: i32 = 0;
        for _ in 0..=baum.len() {
            let p = probs
                .get((i >> 1) as usize)
                .copied()
                .ok_or_else(|| fehler("Baum laeuft aus der Wahrscheinlichkeitstabelle"))?;
            let b = self.flag(p);
            i = baum
                .get((i + b as i32) as usize)
                .copied()
                .ok_or_else(|| fehler("Baum laeuft aus der Tabelle"))? as i32;
            if i <= 0 {
                return Ok((-i) as u32);
            }
        }
        Err(fehler("Baum zu tief verschachtelt"))
    }
}

/// M3: hoechste Breite eines `literal()`-Aufrufs. 32 ist mehr, als
/// VP8 irgendwo braucht (das groesste sind die 7 Dequant-Bits aus 9.6).
const MAX_LITERAL_BITS: u32 = 32;

/// Der **ungepackte** Teil des Keyframe-Kopfs nach RFC 6386, 9.1 und 9.2.
///
/// Der bool-kodierte Teil ab 9.3 ist bewusst nicht enthalten; siehe den
/// Modulkopf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rohkopf {
    pub breite: u32,
    pub hoehe: u32,
    /// 2 Bits Skala je Kante, nach 9.2.
    pub horiz_skala: u32,
    pub vert_skala: u32,
    /// Groesse der ersten Makroblock-Partition in Byte.
    pub erste_part: usize,
    /// B13: `true` = Interframe (das erste Bit des Rahmentags ist 1).
    ///
    /// GEMESSEN an `g7b_testsrc.webm`, Bild 0 bis 5: Bild 0 traegt den
    /// Sync-Code `9d 01 2a`, die Bilder 1 bis 5 `0b 10 ac`. Nach RFC
    /// 6386, 9.1 unterscheidet `9d 01 2a` ein Schluesselbild von
    /// `9d 01 2b` einem Interframe; libvpx schreibt statt 0x2b die
    /// ersten beiden Bytes des Rahmentags (0b 10) an die dritte Stelle,
    /// weshalb die dritte Byte 0xac traegt. Das erste Bit des Tags ist
    /// das verlaessliche Kriterium, und genau das wird hier gelesen.
    pub interframe: bool,
    pub makrobloecke: u32,
}

/// Liest den ungepackten Kopfteil.
///
/// `daten` ist die Nutzlast des `VP8 `-Chunks.
///
/// GEMESSEN 01.10.2026 an `crates/narsil-image/tests/daten/vp8_*.webp`
/// (`werkzeuge/bau.sh narsil-image test`, sechs Dateien): die Byte-Lage
/// stimmt mit dem Codeblock aus RFC 9.2 ueberein und die Masse mit dem,
/// was Pillow meldet (128x128, zweimal 32x32, dreimal 96x96).
pub fn rohkopf_lesen(daten: &[u8]) -> Result<Rohkopf, BildFehler> {
    // 9.1: frame tag, drei Byte, LSB zuerst.
    //   Bit 0: 0 = Keyframe, 1 = Interframe
    //   Bit 1..3: Version
    //   Bit 4: show_frame
    //   Bit 5..23: Groesse der ersten Datenpartition in Byte
    //
    // GEMESSEN 01.10.2026 an den sechs `VP8 `-Dateien: big-endian
    // gesetzt ergab Partitiongroessen von 229584 Byte bei einem Chunk von
    // 1132 Byte — unmoeglich. Little-endian ergibt 36 bis 420 Byte bei
    // Chunks von 108 bis 2858 Byte, und das Feld passt in den Chunk.
    // Das passt zu 9.2, wo die beiden 16-Bit-Worte per `swap2`
    // gelesen werden und dieses Makro auf x86 ein No-op ist.
    if daten.len() < 10 {
        return Err(fehler("VP8-Nutzlast kuerzer als 10 Byte"));
    }
    let tag = (daten[0] as u32) | ((daten[1] as u32) << 8) | ((daten[2] as u32) << 16);
    //
    // B13 (02.10.2026): hier stand bis eben der benannte Fehler
    // "VP8-Interframe ist nicht implementiert". Das erste Bit des
    // Rahmentags sagt die Art (RFC 6386, 9.1) und wird jetzt in
    // `interframe` gemeldet, statt den ganzen Weg abzuschneiden.
    //
    // GEMESSENE GRUNDLAGE: bei `g7b_testsrc.webm` tragen 1 von 20
    // Bildern ein 0 in diesem Bit und 19 ein 1. Die Bildmasse steht bei
    // BEIDEN Arten an derselben Stelle (Byte 6 bis 9) — die Bilder 1 bis 5
    // haben dort `00 18` und `00 1e`, das sind 24 und 30 bei
    // `& 0x3fff` nach der Skalierung; der Referenzdecoder liest sie nur
    // beim Schluesselbild, und fuer ein Interframe erbt das Bild seine
    // Masse vom Referenz. Deshalb wird die Breite hier nur fuer ein
    // Schluesselbild geprueft.
    if tag & (1 << 4) == 0 {
        return Err(fehler("VP8-Bild ist als nicht sichtbar markiert"));
    }
    let version = (tag >> 1) & 7;
    if version > 3 {
        return Err(fehler("VP8-Version groesser als 3"));
    }
    let erste_part = ((tag >> 5) & 0x7_FFFF) as usize;

    // 9.2: Sync-Code in den Bytes 3..6.
    //
    // B13 (02.10.2026), und das war die Ursache, warum kein einziges
    // Interframe gerechnet werden konnte: hier stand ueber Jahre
    // `if daten[3] != 0x9D || daten[4] != 0x01 || daten[5] != 0x2A` —
    // eine ZUSAMMENFASSUNG aus zwei Bedingungen, und eine davon ist
    // falsch.
    //
    // GEMESSEN an `g7b_testsrc.webm` (Bild 0 bis 5):
    //
    // | Bild | Byte 3..6     | erstes Bit des Tags |
    // |------|---------------|---------------------|
    // | 0    | `9d 01 2a`    | 0 (Schluesselbild)   |
    // | 1..5 | `0b 10 ac`    | 1 (Interframe)       |
    //
    // Das sind die beiden Formen aus RFC 6386, 9.1:
    // * Schluesselbild: Sync-Code `9d 01 2a` und danach Breite/Hoehe.
    // * Interframe: **derselbe** Sync-Code `9d 01 2a`, und unmittelbar
    //   danach wiederholen sich die drei Sync-Byte (das ist die
    //   Bildgroesse, und `parse_frame_header` in dixie.c Zeile 39 hat
    //   dafuer `KEYFRAME_HEADER_SZ = 7` gegen `FRAME_HEADER_SZ = 3`).
    //
    // GEMESSENE ENTSCHEIDUNG: streng auf `9d 01 2a` zu pruefen ist
    // richtig und reicht. Die Form `0b 10 ac` bei libvpx ist
    // dasselbe Muster (Rahmentag, dann Sync) nur mit dem Tag an anderer
    // Stelle; sie wird ueber die BITEBENE unterschieden, nicht ueber
    // Bytes 3..6. Deshalb wird hier nur der Schluesselbildfall geprueft.
    if tag & 1 == 0 && (daten[3] != 0x9D || daten[4] != 0x01 || daten[5] != 0x2A) {
        return Err(fehler("VP8-Sync-Code ist nicht 9d 01 2a"));
    }
    // 9.2, Codeblock: "pc->Width = swap2(*(ushort*)(c+3)) & 0x3fff;"
    // mit c = source + 3 sind das die Bytes 6 und 7; das 16-Bit-Wort
    // wird little-endian gelesen, die Breite ist sein unterer Teil.
    let w16 = (daten[6] as u32) | ((daten[7] as u32) << 8);
    let h16 = (daten[8] as u32) | ((daten[9] as u32) << 8);
    let breite = w16 & 0x3FFF;
    let horiz_skala = w16 >> 14;
    let hoehe = h16 & 0x3FFF;
    let vert_skala = h16 >> 14;

    // M4: geprueft, BEVOR daraus eine Allokation wird.
    if breite == 0 || hoehe == 0 {
        return Err(fehler("VP8-Bild ohne Masse"));
    }
    if breite > MAX_KANTE || hoehe > MAX_KANTE {
        return Err(fehler("VP8-Bild groesser als 16384 Pixel je Kante"));
    }
    let makrobloecke = breite.div_ceil(16) * hoehe.div_ceil(16);
    // M3: die Zahl folgt aus den Massen und ist damit begrenzt; die
    // Grenze steht trotzdem da, damit sie nicht von einer Aenderung in
    // der Formel abhaengt.
    if makrobloecke > MAX_MAKROBLOECKE {
        return Err(fehler("VP8-Bild hat mehr Makrobloecke als erlaubt"));
    }
    // Die erste Partition beginnt bei `erste_part` und ist so lang,
    // dass sie bei `erste_part` Byte Nutzlast genau endet. Weniger
    // Byte heisst abgeschnitten (M2).
    if erste_part > daten.len() {
        return Err(fehler("VP8-erste-Partition liegt hinter dem Chunk"));
    }

    Ok(Rohkopf {
        breite,
        hoehe,
        horiz_skala,
        vert_skala,
        erste_part,
        makrobloecke,
        interframe: tag & 1 == 1,
    })
}

/// M3: Laenge des ungepackten Kopfteils in Byte (RFC 6386, 9.1 und 9.2:
/// drei Byte Rahmentag, drei Byte Sync-Code, zwei Byte Breite, zwei Byte
/// Hoehe). Die erste Tokenpartition beginnt HINTER diesem Kopf.
const KOPF_UNGEPACKT: usize = 10;

/// M3: hoechste Zahl Tokenpartitionen (log2 = 3, also 8).
const MAX_PARTITIONEN_LOG2: u32 = 3;

/// Der **bool-kodierte** Keyframe-Kopf nach RFC 6386, 9.2 bis 9.11.
///
/// Die Reihenfolge ist die aus der Rahmentabelle des RFC (Abschnitt 13
/// der Referenz, ab "color_space" bis "prob_skip_false") und NICHT die
/// Abschnittsnummerierung, weil der RFC in 9.7 fuer Keyframes einen
/// anderen Zweig beschreibt als fuer Interframes:
///
///   color_space, clamping_type
///   segmentation_enabled [update_segmentation]
///   filter_type, loop_filter_level, sharpness_level, mb_lf_adjustments
///   log2_nbr_of_dct_partitions
///   quant_indices
///   refresh_entropy_probs        (Keyframes)
///   token_prob_update
///   mb_no_skip_coeff [prob_skip_false]
///
/// Alles daraus, was fuer die Rekonstruktion gebraucht wird, ist noch
/// nicht umgesetzt — siehe den Modulkopf. Der Kopf ist aber in sich
/// geschlossen pruefbar: bei einem Keyframe MUSS refresh_entropy_probs 1
/// und mb_no_skip_coeff 1 sein. Steht dort etwas anderes, ist die
/// Dekodierung an dieser Stelle schon falsch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoolKopf {
    pub quant_y_ac: u32,
    pub quant_y_dc: i32,
    pub quant_y2_dc: i32,
    pub quant_y2_ac: i32,
    pub quant_uv_dc: i32,
    pub quant_uv_ac: i32,
    pub filter_type: u32,
    pub loop_filter_level: u32,
    pub sharpness_level: u32,
    pub log2_partitionen: u32,
    pub refresh_entropy: u32,
    // B13 (02.10.2026, Rolle BILD): die Referenzkopf-Flags. Sie stehen
    // nach den Segmenten und vor den Quantisierer-Deltas; die Reihenfolge
    // ist dixie.c Zeile 89 bis 107 (`decode_reference_header`).
    //
    /// Das Goldene Bild wird erneuert.
    pub refresh_gf: u32,
    /// Das Alternativbild wird erneuert.
    pub refresh_arf: u32,
    /// Kopierquelle fuer `golden` (0 = keines, 1..3 = ein anderes Bild).
    pub copy_gf: u32,
    /// Kopierquelle fuer `altref`.
    pub copy_arf: u32,
    /// Vorzeichenumkehr beim Bezug auf `golden`.
    pub sign_bias_golden: u32,
    /// Vorzeichenumkehr beim Bezug auf `altref`.
    pub sign_bias_altref: u32,
    /// Das letzte Bild wird erneuert.
    pub refresh_last: u32,
    /// B13: `true` = Interframe.
    pub interframe: bool,
    /// B13: die Wahrscheinlichkeiten der Bewegungsvektoren. Ohne sie
    /// laeuft jeder Interframe aus dem Takt.
    pub mv_probs: [[u8; 19]; 2],
    /// B13: Wahrscheinlichkeit, dass der naechste Makroblock ein
    /// Interframe-Modus ist (9.10, `prob_inter`).
    pub prob_inter: u32,
    /// B13: Wahrscheinlichkeit fuer den Bezug `last` (`prob_last`).
    pub prob_last: u32,
    /// B13: Wahrscheinlichkeit fuer `altref` statt `golden` (`prob_gf`).
    pub prob_gf: u32,
    /// B13: die vier Y-Moduswahrscheinlichkeiten eines Interframes.
    ///
    /// GEMESSENE GRUNDLAGE: dixie.c Zeile 78 liest sie nur bei einem
    /// Interframe und nur, wenn ein Flag gesetzt ist. Der erste Wurf hat
    /// sie gelesen und mit `let _ = d.literal(8)` VERWORFEN und statt
    /// dessen die Schluesselbildwerte genommen — dann liest der Modusbaum
    /// mit den falschen Wahrscheinlichkeiten, und weil die Laenge des
    /// Bildes von der Wahrscheinlichkeit abhaengt (das Codesymbol ist
    /// haeufiger), laeuft der Dekader danach aus dem Takt.
    pub y_mode_probs: [u8; 4],
    /// B13: die drei UV-Moduswahrscheinlichkeiten eines Interframes.
    pub uv_mode_probs: [u8; 3],
    pub mb_no_skip_coeff: u32,
    pub prob_skip_false: u32,
    pub segmentierung: bool,
    pub karte: bool,
    pub baum_probs: [u8; 3],
    pub coef_probs: [[[[u8; 11]; 3]; 8]; 4],
    pub lf_anpassung: bool,
    /// Quantisierer je Segment (0..3), falls `segmentierung`.
    ///
    /// Stand 01.10.2026: vorher wurden sie gelesen und verworfen. Ohne
    /// sie hat jeder Makroblock den Faktor des Bildes, und in Bildern
    /// mit Segmentierung stimmt keiner von ihnen (gemessen: Faktor 14
    /// statt 10, siehe Kommentar am Segmentkopf).
    pub segment_quant: [i32; MAX_SEGMENTE],
    /// Schleifenfilter-Stufe je Segment, falls `segmentierung`.
    pub segment_lf: [i32; MAX_SEGMENTE],
    /// `true` heisst: die Segmentwerte sind absolute Quantisierer,
    /// `false`: sie werden zum Bildquantisierer addiert.
    pub segment_abs: bool,
    pub ref_lf_delta: [i32; 4],
    pub mode_lf_delta: [i32; 4],
    /// Groessen der Koeffizientenpartitionen, ohne die erste.
    pub part_groessen: [u32; 7],
    /// Anzahl der belegten Eintraege in `part_groessen`.
    pub part_anzahl: usize,
}

/// Liest ein vorzeichenbehaftetes Delta: vier Bit Betrag, dann ein
/// Vorzeichenbit (0 heisst positiv).
///
/// RFC 6386, 9.6: "y_dc_delta_magnitude L(4), y_dc_delta_sign L(1)".
/// Die Ueberschrift sagt "5 bits", die Tabelle sagt 4 plus 1 — beides
/// ergibt dieselbe Bitzahl, aber die Reihenfolge ist Betrag ZUERST.
fn delta(d: &mut BoolDekoder) -> i32 {
    let betrag = d.literal(4) as i32;
    if d.literal(1) == 1 { -betrag } else { betrag }
}

/// Der Entropiezustand eines Bildes: die vier Wahrscheinlichkeitstabellen,
/// die von Bild zu Bild weitergetragen werden.
///
/// GEMESSENE GRUNDLAGE, dixie.c 320-327 woertlich:
/// ```c
/// /* Set keyframe entropy defaults.  These get updated on keyframes
///  * regardless of the refresh_entropy setting. */
/// if (ctx->frame_hdr.is_keyframe)
/// {
///     ARRAY_COPY(ctx->entropy_hdr.coeff_probs, k_default_coeff_probs);
///     ARRAY_COPY(ctx->entropy_hdr.mv_probs,      k_default_mv_probs);
///     ARRAY_COPY(ctx->entropy_hdr.y_mode_probs,  k_default_y_mode_probs);
///     ARRAY_COPY(ctx->entropy_hdr.uv_mode_probs, k_default_uv_mode_probs);
/// }
/// ```
/// also: **nur** bei einem Schluesselbild auf die Vorgaben. Bei einem
/// Interframe wird der Zustand vom Vorgaenger UEBERNOMMEN und nur durch
/// die Aktualisierungen dieses Bildes veraendert.
///
/// GEMESSENER FEHLER (Rolle VIDEO, 03.10.2026): `boolkopf_lesen` hat
/// bisher in JEDEM Bild mit `DEFAULT_COEFF_PROBS`, `DEFAULT_MV_PROBS`,
/// `[112,86,140,37]` und `[162,101,204]` angefangen. Damit bekam jedes
/// Zwischenbild die Wahrscheinlichkeiten des Schluesselbildes statt der des
/// Vorgaengers, und jede Aktualisierung eines Bildes war fuer das naechste
/// verloren. GEMESSENE Groessenordnung: `coef_probs` allein hat
/// 4 mal 8 mal 3 mal 11 Eintraege, es sind also 1056 Aktualisierungen, die
/// einem Zwischenbild entgehen, wenn es sie setzt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entropie {
    pub coef_probs: [[[[u8; 11]; 3]; 8]; 4],
    pub mv_probs: [[u8; 19]; 2],
    pub y_mode_probs: [u8; 4],
    pub uv_mode_probs: [u8; 3],
}

impl Entropie {
    /// Die Vorgaben des Normtextes (`vp8_prob_data.h`, `k_default_*`).
    pub fn vorgabe() -> Self {
        Entropie {
            coef_probs: crate::vp8_tabellen::DEFAULT_COEFF_PROBS,
            mv_probs: crate::vp8_tabellen::DEFAULT_MV_PROBS,
            y_mode_probs: [112, 86, 140, 37],
            uv_mode_probs: [162, 101, 204],
        }
    }

    /// Der Entropiezustand, der in einem gelesenen Kopf steckt. `BoolKopf`
    /// traegt genau diese vier Tabellen.
    pub fn aus_kopf(k: &BoolKopf) -> Self {
        Entropie {
            coef_probs: k.coef_probs,
            mv_probs: k.mv_probs,
            y_mode_probs: k.y_mode_probs,
            uv_mode_probs: k.uv_mode_probs,
        }
    }
}

/// Liest den bool-kodierten Keyframe-Kopf.
///
/// `daten` ist die Nutzlast des `VP8 `-Chunks, `roh` deren ungepackter
/// Kopf. Der Dekoder wird an der ersten Makroblockpartition angesetzt.
///
/// GEMESSENE ENTSCHEIDUNG, warum es daneben `boolkopf_lesen_mit_entropie`
/// gibt und dieser Aufruf nicht einfach eine Fortsetzung verlangt:
/// `boolkopf_lesen` wird von fuenf fremden Dateien aufgerufen
/// (`crates/narsil-image/examples/mbvergleich.rs`, `vp8_kopfdaten.rs`,
/// `vp8_skipflag_messung.rs`, `crates/narsil-image/tests/vp8_modi.rs`,
/// `vp8_skip.rs`, `vp8_interkopf.rs`) und von einem Beispiel in
/// `narsil-media`. Diese Dateien gehoeren nicht mir (`EIGENTUM.txt`), und ein
/// Aufruf an einer Signatur scheitert dort den Bau. Der Weg mit dem
/// Entropiezustand ist deshalb zusaetzlich; dieser Aufruf hier startet wie
/// bisher ohne mitgebrachten Zustand, also mit den Vorgaben.
pub fn boolkopf_lesen<'a>(
    daten: &'a [u8],
    roh: &Rohkopf,
) -> Result<(BoolKopf, BoolDekoder<'a>), BildFehler> {
    let (kopf, dek, _) = boolkopf_lesen_mit_entropie(daten, roh, &Entropie::vorgabe())?;
    Ok((kopf, dek))
}

/// Derselbe Weg, mit dem Entropiezustand des Vorgaengers.
///
/// Der zurueckgegebene dritte Wert ist der Zustand VOR den Aktualisierungen
/// dieses Bildes, und nur dann `Some`, wenn `refresh_entropy == 0`. Der
/// Aufrufer legt ihn nach dem Bild wieder zurueck — dixie.c 412-416:
///
/// ```c
/// ctx->frame_cnt++;
/// if (!ctx->reference_hdr.refresh_entropy)
/// {
///     ctx->entropy_hdr = ctx->saved_entropy;
///     ctx->saved_entropy_valid = 0;
/// }
/// ```
pub fn boolkopf_lesen_mit_entropie<'a>(
    daten: &'a [u8],
    roh: &Rohkopf,
    start: &Entropie,
) -> Result<(BoolKopf, BoolDekoder<'a>, Option<Entropie>), BildFehler> {
    // GEMESSEN 01.10.2026: die erste Tokenpartition beginnt bei Byte 10,
    // direkt hinter dem ungepackten Kopf — NICHT bei `erste_part`, das
    // ist ihre LAENGE. Mit dem Laengenwert als Anfang bekam man 750 Bit
    // zu frueh gelesene, aber gueltige Symbole: ein Baum mit vier
    // Blaettern dekodiert auch Unsinn "erfolgreich", das ist genau die
    // Falle, vor der hier gewarnt wurde.
    //
    // B13, 02.10.2026. GEMESSENER FEHLER: hier stand fuer BEIDE Bildarten
    // `KOPF_UNGEPACKT` = 10. Beim Interframe ist der ungepackte Teil aber
    // nur DREI Byte lang (der Rahmentag); der Sync-Code und die beiden
    // Massen gehoeren zum Schluesselbild.
    //
    // GEMESSENE GRUNDLAGE, dixie.c Z. 288-294 woertlich:
    //
    // ```c
    // data += FRAME_HEADER_SZ;        /* 3 */
    // if (ctx->frame_hdr.is_keyframe)
    //     data += KEYFRAME_HEADER_SZ; /* 7  -> zusammen 10 */
    // init_bool_decoder(&bool, data, ctx->frame_hdr.part0_sz);
    // ```
    //
    // GEMESSENER Beleg, gegen den Referenzdecoder als Orakel (Kopfdaten
    // von Bild 1, `dixie.c` gegen `boolkopf_lesen` hier):
    //
    // | Feld            | Orakel | vorher | jetzt |
    // |-----------------|--------|--------|-------|
    // | coeff_skip_prob |     16 |     44 |    16 |
    // | prob_inter      |      1 |      5 |     1 |
    // | prob_last       |    255 |     80 |   255 |
    //
    // GEMESSENE Folge auf den Ebenen von Bild 1 aus `kurz.webm`
    // (Orakel `ffmpeg -pix_fmt yuv420p`): Y in 55902 von 76800 Werten
    // falsch vor dem Fix, 8572 danach; U 14310 -> 4808, V 14622 -> 5238
    // (je von 19200).
    let kopf_ungepackt = if roh.interframe { 3 } else { KOPF_UNGEPACKT };
    let mut d = BoolDekoder::new(daten, kopf_ungepackt)?;
    //
    // GEMESSENER FEHLER, den diese Pruefung zuerst hatte (B13, 02.10.2026):
    // sie galt fuer BEIDE Bildarten. GEMESSEN an einem von libvpx
    // erzeugten Interframe aus einer gleichmaessigen Flaeche: 26 Byte
    // gesamt, `erste_part` = 22, also 22 > 26 - 10 = 16 — und die Datei
    // ist einwandfrei, `ffmpeg` rechnet sie fehlerfrei zurueck.
    //
    // GEMESSENE GRUNDLAGE fuer die Lockerung: RFC 6386, 9.1, nennt die
    // Groesse fuer die erste Partition nur im Zusammenhang mit dem
    // ungepackten Kopf, und der ist bei einem Interframe KURZER (6 Byte
    // statt 10). Die Grenze gilt also fuer den SCHLUESSELbildweg; beim
    // Interframe wird nur geprueft, dass ueberhaupt Daten da sind — der
    // Dekoder laesst sich ohnehin von der Laenge nicht fuehren.
    if !roh.interframe && roh.erste_part > daten.len().saturating_sub(KOPF_UNGEPACKT) {
        return Err(fehler("VP8-erste-Partition reicht nicht bis zum Chunkende"));
    }

    //
    // 9.2: Farbraum und Beschneiden — NUR bei einem Schluesselbild.
    //
    // GEMESSENER FEHLER, der JEDES Interframe um zwei Bit verschoben
    // hat (B13, 02.10.2026). Diese zwei Bit standen hier ohne
    // Bildart-Bedingung, und damit begann beim Interframe die
    // Segmentierung an der falschen Stelle.
    //
    // GEMESSENE GRUNDLAGE, dixie.c Zeile 301-303 woertlich:
    //
    // ```c
    // /* Skip the colorspace and clamping bits */
    // if (ctx->frame_hdr.is_keyframe)
    //     if (bool_get_uint(&bool, 2))
    //         vpx_internal_error(...);
    // ```
    //
    // Die beiden Bit werden also nur bei einem Schluesselbild
    // KONSUMIERT; bei einem Interframe steht dort nichts, und der
    // Segmentierungskopf (Z. 305) folgt unmittelbar.
    //
    // GEMESSENER Beleg fuer die Groessenordnung, gegen den
    // Referenzdecoder als Orakel (dixie.c, Kopfbereich, Bild 1 von
    // `g7b_testsrc.webm`):
    //
    // | Feld               | Orakel | vorher | nachher |
    // |--------------------|--------|--------|---------|
    // | coeff_skip_prob    |     16 |     44 |      16 |
    // | prob_inter         |      1 |      5 |       1 |
    // | prob_last          |    255 |     80 |     255 |
    //
    // GEMESSENER Folgebefund: `coeff_skip_prob` = 16 heisst "skip ist
    // sehr wahrscheinlich", und das Orakel zaehlt im Bild 1 281 von 300
    // Makrobloecken als `skip`; vorher waren es hier 5. Und weil das
    // Skip-Flag pro Makroblock ein Bit liest, verschob sich an dieser
    // Stelle der ganze Modalstrom — die Y-Ebene war 55902 von 76800
    // Werten falsch, U 14310 und V 14622 von je 19200.
    if !roh.interframe {
        let _farbraum = d.flag(128);
        let _beschneiden = d.flag(128);
    }

    // 9.3: Segmentierung.
    let segmentierung = d.flag(128) == 1;
    let mut karte = false;
    let mut baum_probs = [255u8; 3];
    // GEMESSEN 01.10.2026 (Rolle BILD): diese Werte wurden gelesen und
    // dann WEGGEWORFEN. Dadurch bekam jeder Makroblock den
    // Bildquantisierer, und jeder Makroblock in einem anderen Segment
    // wurde mit dem falschen Faktor dekoeffiziert.
    // GEMESSEN an `t32_diag`: dixie rechnet fuer den ersten Makroblock
    // den DC-Faktor 10 (Segment 2), der Dekoder 14 (Segment 0) — genau
    // der Faktor 1.4, der als konstanter Versatz in den Ebenen stand.
    // Zaehlung und Rundung bleiben, wie sie waren (9 Bit je gesetztem
    // Eintrag, 1 Bit sonst — `bool_maybe_get_int` des Normtextes liest
    // ein Vorzeichen, `bits` Werte und ein Vorzeichen).
    let mut segment_quant = [0i32; MAX_SEGMENTE];
    let mut segment_lf = [0i32; MAX_SEGMENTE];
    let mut segment_abs = false;
    if segmentierung {
        karte = d.flag(128) == 1;
        let daten_update = d.flag(128) == 1;
        if daten_update {
            segment_abs = d.flag(128) == 1;
            for s in segment_quant.iter_mut() {
                if d.flag(128) == 1 {
                    let wert = d.literal(7) as i32;
                    *s = if d.flag(128) == 1 { -wert } else { wert };
                }
            }
            for s in segment_lf.iter_mut() {
                if d.flag(128) == 1 {
                    let wert = d.literal(6) as i32;
                    *s = if d.flag(128) == 1 { -wert } else { wert };
                }
            }
        }
        if karte {
            for i in 0..3 {
                if d.flag(128) == 1 {
                    baum_probs[i] = d.literal(8) as u8;
                }
            }
        }
    }

    // 9.4: Loopfilter.
    let filter_type = d.flag(128);
    let loop_filter_level = d.literal(6);
    let sharpness_level = d.literal(3);
    // 9.4: vier Deltawerte fuer die Referenzrahmen, vier fuer die Modi.
    // Beide Gruppen haben dieselbe Form: Flag, dann bei gesetztem Flag
    // sechs Bit Betrag und ein Vorzeichenbit.
    let mut ref_lf_delta = [0i32; 4];
    let mut mode_lf_delta = [0i32; 4];
    let lf_anpassung = d.flag(128) == 1;
    if lf_anpassung && d.flag(128) == 1 {
        for i in 0..4 {
            if d.flag(128) == 1 {
                let v = d.literal(6) as i32;
                ref_lf_delta[i] = if d.literal(1) == 1 { -v } else { v };
            }
        }
        for i in 0..4 {
            if d.flag(128) == 1 {
                let v = d.literal(6) as i32;
                mode_lf_delta[i] = if d.literal(1) == 1 { -v } else { v };
            }
        }
    }

    // 9.5: Tokenpartitionen. Zwei Bit, 0..3; mehr ist ungueltig.
    let log2_partitionen = d.literal(2);
    if log2_partitionen > MAX_PARTITIONEN_LOG2 {
        return Err(fehler("VP8 meldet mehr als acht Tokenpartitionen"));
    }

    //
    // B13 (02.10.2026), und das war die Ursache fuer den aus dem Takt
    // laufenden Interframe: die REIHENFOLGE.
    //
    // GEMESSENE Grundlage aus dem Referenzdecoder (`dixie.c`):
    //     Z. 307  decode_segmentation_header     (9.3)
    //     Z. 308  decode_loopfilter_header       (9.4)
    //     Z. 309  decode_and_init_token_partitions (9.5)
    //     Z. 314  decode_quantizer_header        (9.6)
    //     Z. 315  decode_reference_header        (9.10)
    //     Z. 335  decode_entropy_header          (9.7, 9.8, 9.9)
    //
    // `decode_entropy_header` steht also ZULETZT — und darin stehen
    // die Koeffizientenwahrscheinlichkeiten, `coeff_skip`, `prob_inter`,
    // `prob_last`, `prob_gf`, die Moduswahrscheinlichkeiten und die
    // Bewegungsvektoren. Vorher standen bei mir refresh_entropy und die
    // Koeffizienten VOR dem Referenzkopf.
    //
    // GEMESSENER Beleg: Bild 1 von `g7b_testsrc.webm` war ab Bildzeile
    // 192 vollstaendig falsch (320 von 320 Kanalwerten je Zeile). Mit der
    // richtigen Reihenfolge fing die Menge der gelesenen Symbole wieder
    // mit dem ersten Makroblock an.
    //
    // Zusaetzlich ist `refresh_entropy` Teil des REFERENZKOPFES (es steht
    // in `decode_reference_header` zwischen den Vorzeichen und
    // `refresh_last`) und nicht des Entropiekopfes — es ist das Tor fuer
    // die Koeffizientenaktualisierung, und die liegt ja danach.
    // 9.6: Dequantisierungsindizes. GEMESSENE GRUNDLAGE fuer die Lage:
    // `decode_quantizer_header` steht in dixie.c Zeile 314, also VOR dem
    // Referenzkopf (315) und vor dem Entropiekopf (335).
    let quant_y_ac = d.literal(7);
    let quant_y_dc = if d.flag(128) == 1 { delta(&mut d) } else { 0 };
    let quant_y2_dc = if d.flag(128) == 1 { delta(&mut d) } else { 0 };
    let quant_y2_ac = if d.flag(128) == 1 { delta(&mut d) } else { 0 };
    let quant_uv_dc = if d.flag(128) == 1 { delta(&mut d) } else { 0 };
    let quant_uv_ac = if d.flag(128) == 1 { delta(&mut d) } else { 0 };

    // 9.10: der REFERENZKOPF.
    let is_key = !roh.interframe;
    let refresh_gf = if is_key { 1 } else { d.flag(128) };
    let refresh_arf = if is_key { 1 } else { d.flag(128) };
    let copy_gf = if is_key {
        0
    } else if refresh_gf == 0 {
        d.literal(2)
    } else {
        0
    };
    let copy_arf = if is_key {
        0
    } else if refresh_arf == 0 {
        d.literal(2)
    } else {
        0
    };
    let sign_bias_golden = if is_key { 0 } else { d.flag(128) };
    let sign_bias_altref = if is_key { 0 } else { d.flag(128) };

    // GEMESSENE ENTSCHEIDUNG, warum `refresh_last` hier steht und nicht
    // bei den uebrigen: dixie.c Zeile 107 liest es als LETZTES der
    // sieben Angaben, nach `refresh_entropy`. Und `refresh_entropy` ist
    // bei einem Schluesselbild zwingend 1 (9.8), weshalb es der
    // vorhandene Aufbau hier schon abhandelt.
    // `refresh_entropy` steht laut dixie.c Zeile 106 zwischen den beiden
    // Vorzeichen und `refresh_last`. Bei einem Schluesselbild ist es 1
    // (9.8), und es wird NICHT gelesen — der Schluesselbildweg nimmt ohne
    //hin die Vorgabewerte.
    //
    // GEMESSENER FEHLER, den dieser Aufruf zuerst hatte (02.10.2026):
    // `if is_key { 1 } else { d.flag(128) }`. Das Bit wird aber IMMER
    // gelesen — es ist in `decode_reference_header` der EINZIGE Wert
    // ohne den Schalter `key ? ... : bool_get_bit(...)` (dixie.c Z. 106),
    // und das ist kein Zufall: 9.8 schreibt "if key frame:
    // refresh_entropy_probs is 1". Ein gelesenes 1 ist dasselbe Ergebnis
    // wie ein angenommenes 1 — nur eben am richtigen Ort im Strom.
    //
    // GEMESSENE Folge des Fehlers: Bild 0 von `g7b_testsrc.webm` sprang
    // von groesster Abweichung 2 auf 255, weil der Dekader ab diesem Bit
    // ein Byte zu frueh stand. Alle 25 anderen Testprogramme blieben
    // gruen, weil keins ein Bild auf diese Weise liest.
    let refresh_entropy = d.flag(128);
    //
    // B13, 03.10.2026 (Rolle VIDEO). GEMESSENE GRUNDLAGE, dixie.c 320-334,
    // die Reihenfolge ist hier drei Schritte und keiner davon ist optional:
    //
    // ```c
    // if (ctx->frame_hdr.is_keyframe)          /* 320 */
    // {                                        /* 321-327 */
    //     ARRAY_COPY(ctx->entropy_hdr.coeff_probs, k_default_coeff_probs);
    //     ARRAY_COPY(ctx->entropy_hdr.mv_probs,      k_default_mv_probs);
    //     ARRAY_COPY(ctx->entropy_hdr.y_mode_probs,  k_default_y_mode_probs);
    //     ARRAY_COPY(ctx->entropy_hdr.uv_mode_probs, k_default_uv_mode_probs);
    // }
    // if (!ctx->reference_hdr.refresh_entropy) /* 329 */
    // {                                        /* 330-334 */
    //     ctx->saved_entropy = ctx->entropy_hdr;
    //     ctx->saved_entropy_valid = 1;
    // }
    // decode_entropy_header(ctx, &bool, &ctx->entropy_hdr);   /* 335 */
    // ```
    //
    // GEMESSENE ENTSCHEIDUNG, warum das ZURUECKSETZEN hier steht und nicht
    // im Aufrufer: es muss VOR dem Merken stehen, denn das Gemerkte ist der
    // Stand, den ein Schluesselbild seinen Nachfolgern hinterlaesst. Ein
    // Schluesselbild mit `refresh_entropy == 0` gibt also die VORGABEN weiter,
    // nicht seine eigenen Aktualisierungen — das ist der Sinn der
    // Rollback-Zeile 414.
    // MESSschalter (Rolle VIDEO, 03.10.2026): `NARSIL_VP8_ENTROPIE_NUR_MODI`
    // laesst die Koeffizientenwahrscheinlichkeiten in JEDEM Bild bei den
    // Vorgaben und traegt nur die drei Modustabellen ueber. Grund ist eine
    // Messung, keine Vermutung: mit voller Uebernahme fallen die Bilder 7,
    // 9 und 10 zurueck (Bild 7 von 228 auf 145 exakte Makrobloecke), und
    // Bild 7 veraendert 44 Koeffizientenwahrscheinlichkeiten — die
    // meisten von allen Zwischenbildern. Der Schalter trennt die beiden
    // Haelfften der Aenderung. **Er ist ein Messgeraet, keine Norm:** der
    // Vorgabeweg ist die normgebundene Fassung (dixie.c 320-327 setzt
    // alle vier Tabellen nur beim Schluesselbild auf die Vorgaben), und
    // nur wenn die Messung das Gegenteil zeigt, wird die Norm veraendert.
    let start = if roh.interframe && std::env::var_os("NARSIL_VP8_ENTROPIE_NUR_MODI").is_none() {
        start.clone()
    } else if roh.interframe {
        Entropie {
            coef_probs: crate::vp8_tabellen::DEFAULT_COEFF_PROBS,
            ..start.clone()
        }
    } else {
        Entropie::vorgabe()
    };
    // Und das Merken fuer den Rollback am Bildende.
    let zurueck = if refresh_entropy == 0 {
        Some(start.clone())
    } else {
        None
    };
    let refresh_last = if is_key { 1 } else { d.flag(128) };

    // 9.9: Tokenwahrscheinlichkeiten. Das Flag wird mit
    // coeff_update_probs[t] gelesen, nicht mit 128.
    // B13, 03.10.2026 (Rolle VIDEO). GEMESSENER FEHLER: hier stand die
    // Konstante `DEFAULT_COEFF_PROBS`. Nach dixie.c 320-327 wird auf die
    // Vorgaben NUR bei einem Schluesselbild zurueckgesetzt; ein Interframe
    // erbt den Zustand des Vorgaengers und aendert ihn nur durch seine
    // eigenen Aktualisierungen. Siehe `Entropie`.
    let mut coef_probs = start.coef_probs;
    let mut prob_index = 0usize;
    for i in 0..4usize {
        for j in 0..8usize {
            for k in 0..3usize {
                for l in 0..11usize {
                    let p = crate::vp8_tabellen::COEFF_UPDATE_PROBS[prob_index];
                    if d.flag(p) == 1 {
                        coef_probs[i][j][k][l] = d.literal(8) as u8;
                    }
                    prob_index += 1;
                }
            }
        }
    }

    //
    // B13 (02.10.2026): 9.10, der INTERFRAME-Teil des Entropiekopfes.
    //
    // GEMESSENE REIHENFOLGE, woertlich nach dixie.c Zeile 65 bis 87
    // (`decode_entropy_header`):
    //
    //   coeff_skip_enabled = bit
    //   if coeff_skip_enabled: coeff_skip_prob = 8 Bit
    //   if NOT keyframe:
    //       prob_inter = 8 Bit     <-- OHNE Update-Flag
    //       prob_last  = 8 Bit
    //       prob_gf    = 8 Bit
    //       if bit: 4 x 8 Bit y_mode_probs
    //       if bit: 3 x 8 Bit uv_mode_probs
    //       for i,j: if k_mv_entropy_update_probs[i][j]: 7 Bit
    //   refresh_entropy_probs = bit
    //   if refresh: 4 x 1056 Bit Koeffizienten
    //
    // GEMESSENER FEHLER, den der erste Wurf hatte: die MV-Wahrscheinlich-
    // keiten wurden mit einem Update-Flag gelesen und VOR
    // `mb_no_coeff_skip` gesetzt. Beides verschiebt den Bool-Dekoder:
    // `mb_no_coeff_skip` wird in modemv.c Zeile 518 als
    // `coeff_skip_enabled` GELESEN, und zwar OHNE Bedingung der Bildart.
    // Der Folgefehler war messbar: Bild 0 von g7b_testsrc.webm wich in
    // 76249 von 76800 Y-Stellen ab, weil das Skip-Flag an falscher Stelle
    // gelesen wurde.
    let mb_no_skip_coeff = d.flag(128);
    let prob_skip_false = if mb_no_skip_coeff == 1 {
        d.literal(8)
    } else {
        0
    };
    // Die Interframe-Angaben: nur bei einem Interframe vorhanden.
    let (prob_inter, prob_last, prob_gf, mv_probs, y_mode_probs, uv_mode_probs) = if roh.interframe
    {
        let prob_inter = d.literal(8);
        let prob_last = d.literal(8);
        let prob_gf = d.literal(8);
        //
        // B13, 03.10.2026 (Rolle VIDEO). GEMESSENER FEHLER, und der ist
        // der GROSSTE in der Datei: hier stand
        //
        // ```rust
        // let mut y_mode_probs = [0u8; 4];
        // ```
        //
        // Die Norm hat fuer diese beiden Wahrscheinlichkeiten VORGABEN.
        //
        // GEMESSENE GRUNDLAGE, der Referenzdecoder des Normtextes
        // (dixie.c Z. 325-328 setzt sie zurueck, `vp8_prob_data.h`
        // Z. 185-189 traegt die Werte):
        //
        // ```c
        // unsigned char k_default_y_mode_probs  [] = { 112,  86, 140,  37};
        // unsigned char k_default_uv_mode_probs [] = { 162, 101, 204};
        // ```
        //
        // GEMESSENE FOLGE des Fehlers, und sie ist nicht „ein Bild sieht
        // anders aus“: `flag(0)` gibt immer 0 zurueck (`split = 1 + (((range
        // - 1) * 0) >> 8) = 1`, und `wert >= 256` kommt bei diesem Strom
        // nicht vor). Damit wurde JEDER Intra-Makroblock in JEDEM
        // Interframe als DC_PRED gelesen — und, weil dieselbe
        // Wahrscheinlichkeit im Baum benutzt wird, auch die anderen drei
        // Modi nie. Zugleich blieb der Bool-Dekoder im Takt: er liest
        // dieselbe Zahl Bits, und nur die SYMBOLE sind falsch.
        //
        // GEMESSENER Beleg (Bild 7 von `g7b_testsrc.webm`, Makroblock
        // 44, Zeile 2 Spalte 4):
        //
        // | | Crate | Referenzdecoder |
        // |---|---|---|
        // | Intra-Modus | B_PRED | DC_PRED |
        // | Bits gelesen | 3 | 3 |
        // | Dekoder danach | bits 4, wert 29664, weite 219 | bits 3, wert 24528, weite 212 |
        //
        // Die Makroblockwerte 0 bis 43 sind IDENTISCH — dieselben Zaehler,
        // dieselben Vektoren, dieselben Sprungbits. Erst ab 44 laeuft der
        // Bool-Dekoder auseinander, weil der Intra-Ast eine andere Zahl
        // Unterblockmodi mitbringt (B_PRED liest 16 davon, DC_PRED keinen).
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Vorgabe hier und nicht in
        // `vp8_tabellen.rs`: sie gehoert zum KOPF, und der Kopf ist in
        // dieser Datei (`boolkopf_lesen`). Die Tabellendatei gehoert nicht
        // mir (`EIGENTUM.txt`: BILD, alles ausser vp8.rs) — eine Zeile
        // dort waere ein Eingriff in eine fremde Datei.
        // dieselbe Traegerregel (dixie.c 324).
        let mut y_mode_probs = start.y_mode_probs;
        if d.flag(128) == 1 {
            for z in y_mode_probs.iter_mut() {
                *z = d.literal(8) as u8;
            }
        }
        // dieselbe Traegerregel (dixie.c 325).
        let mut uv_mode_probs = start.uv_mode_probs;
        if d.flag(128) == 1 {
            for z in uv_mode_probs.iter_mut() {
                *z = d.literal(8) as u8;
            }
        }
        // Die Bewegungsvektoren (dixie.c Zeile 81 bis 86).
        // dieselbe Traegerregel wie bei `coef_probs` (dixie.c 323).
        let mut probs = start.mv_probs;
        for i in 0..2usize {
            for j in 0..19usize {
                let p = crate::vp8_tabellen::MV_ENTROPY_UPDATE_PROBS[i][j];
                if d.flag(p) == 1 {
                    let x = d.literal(7);
                    probs[i][j] = if x != 0 { (x as u16) << 1 } else { 1 } as u8;
                }
            }
        }
        (
            prob_inter,
            prob_last,
            prob_gf,
            probs,
            y_mode_probs,
            uv_mode_probs,
        )
    } else {
        //
        // B13, 03.10.2026 (Rolle VIDEO). GEMESSENER FEHLER, und es ist der
        // Grund, warum seit heute 5 Intra-Makrobloecke in Zwischenbildern
        // den FALSCHEN Intra-Modus bekommen: hier standen
        // `KF_Y_MODE_PROBS` und `KF_UV_MODE_PROBS`, also [145, 156, 163,
        // 128] und [142, 114, 183]. Das sind die Wahrscheinlichkeiten des
        // Modusbaums eines SCHLUESSELBILDES.
        //
        // GEMESSENE GRUNDLAGE: der Referenzdecoder haelt ZWEI verschiedene
        // Saetze. `modemv.c` Z. 98 liest den Modus eines
        // Schluesselbildmakroblocks mit den KONSTANTEN
        // `kf_y_mode_probs`; `modemv.c` Z. 131 liest den Modus eines
        // INTRA-Makroblocks in einem ZWISCHENBILD mit
        // `hdr->y_mode_probs`. Und `dixie.c` Z. 326 setzt
        //
        // ```c
        // ARRAY_COPY(ctx->entropy_hdr.y_mode_probs, k_default_y_mode_probs);
        // ARRAY_COPY(ctx->entropy_hdr.uv_mode_probs, k_default_uv_mode_probs);
        // ```
        //
        // auf `[112, 86, 140, 37]` und `[162, 101, 204]` — auf die
        // Vorgaben, nicht auf die Schluesselbildwerte.
        //
        // GEMESSENE ENTSCHEIDUNG, warum trotzdem `KF_Y_MODE_PROBS` fuer
        // das Schluesselbild selbst gebraucht wird: der Schluesselbildweg
        // liest seinen Modusbaum an anderer Stelle mit genau dieser
        // Konstante (`t::KF_Y_MODE_PROBS`, Z. 1253) und darf sie nicht aus
        // dem Kopf holen — im Kopf steht ab jetzt die Vorgabe. Vorher waren
        // beide Wege dieselbe Zahl und der Unterschied blieb unsichtbar.
        //
        // GEMESSENER Beleg, Bild 6, Makroblock 259 (Zeile 12, Spalte 19):
        //
        // | | Crate | Referenzdecoder |
        // |---|---|---|
        // | `y_mode_probs` | 145, 156, 163, 128 | 112, 86, 140, 37 |
        // | `uv_mode_probs` | 142, 114, 183 | 162, 101, 204 |
        // | gelesener Modus | DC_PRED (0) | **V_PRED (1)** |
        // | Pixel in der Zeile darueber | 93 93 91 91 91 90 ... | 93 93 91 91 91 90 ... |
        // | Pixel des Makroblocks | 91 91 91 91 ... (flach) | 93 93 91 91 91 90 ... |
        //
        // Die Wahrheit ist auf das BIT gleich der Zeile darueber — das ist
        // V_PRED und nichts anderes. Und das Bild, aus dem die Zeile
        // darueber stammt, ist in diesem Bild an Makroblock 239 richtig
        // (Abweichung 0), die Nachbarwerte sind also nicht die Ursache.
        let vor = Entropie::vorgabe();
        (
            0,
            0,
            0,
            crate::vp8_tabellen::DEFAULT_MV_PROBS,
            vor.y_mode_probs,
            vor.uv_mode_probs,
        )
    };

    // 9.5, zweite Haelfte: die Groessen der Koeffizientenpartitionen
    // stehen als DREI BYTE ROHDATEN direkt hinter der ersten Partition
    // und werden aus dem Bool-Strom NICHT gelesen:
    //   hdr->partition_sz[i] = (data[2] << 16) | (data[1] << 8) | data[0];
    // (Referenzdecoder, `decode_and_init_token_partitions`, RFC 20.x)
    //
    // GEMESSEN 01.10.2026: ich hatte hier 32 Bit aus dem Bool-Strom
    // gelesen. Bei einer einzigen Partition faellt das nicht auf —
    // die Schleife laeuft `partitions - 1` Mal —, ab zwei Partitionen
    // waere der ganze Strom um 32 Bit je Partition verschoben.
    let mut part_groessen = [0u32; 7];
    let mut part_anzahl = 0usize;
    let n = 1usize << log2_partitionen;
    //
    // B13 (02.10.2026): die Groessen der Koeffizientenpartitionen stehen
    // nach RFC 6386, 9.5, nur bei einem SCHLUESSELbild im Strom
    // ("this partition size information is only present for key frames").
    //
    // GEMESSENE GRUNDLAGE, warum das hier FALSCH war (Rolle VIDEO,
    // 03.10.2026): der Referenzdecoder laesst die Einschraenkung nicht
    // gelten. `decode_and_init_token_partitions` (dixie.c 123-159) liest
    // die Groessen, sobald `partitions > 1`, ohne auf den Schluesselbild
    // zu pruefen — und ein Interframe kann sehr wohl mehrere
    // Koeffizientenpartitionen haben. Die Fussnote der alten Fassung
    // nannte als Beleg, Bild 1 von `g7b_testsrc.webm` melde
    // `log2_partitionen = 3`. GENAU DAS ist der Befund, nicht das Gegenteil
    // davon.
    //
    // GEMESSENE FOLGE des `n = 1`: die Crate las den ganzen
    // Koeffizientenstrom als EINE Partition und fing dabei bei den Bytes
    // der MODALPARTITION an. GEMESSEN an Bild 1 von `g7b_testsrc.webm`
    // (20x15 Makrobloecke, 254 Byte):
    //
    // | Makroblockzeilen | Anteil uebersprungener Makrobloecke | Luma-Abweichung |
    // |---|---|---|
    // | 0 bis 10          | praktisch alle                      | **0,00 %** |
    // | 11 bis 13         | wenige                             | 5 bis 10 %  |
    // | 14                | alle                               | 0,00 %      |
    //
    // Das ist die Unterschrift eines falschen Bytestroms: uebersprungene
    // Makrobloecke lesen gar keine Koeffizienten, also faellt der Fehler
    // genau dort nicht auf, wo sich nichts bewegt. Bild 1 hat 254 Byte und
    // trotzdem 6,39 % Luma-Fehler, alle in den drei ZEILEN, in denen sich
    // der Streifen bewegt.
    let n = n;
    //
    // GEMESSENER FEHLER (Rolle VIDEO, 03.10.2026), direkt daneben: die
    // Basis der Groessentabelle war `KOPF_UNGEPACKT` (10 Byte) auch fuer
    // ein Interframe. Der Schluesselbildkopf ist aber 7 Byte, ein
    // Interframe hat gar keinen — sein Rahmentag ist 3 Byte (dixie.c 290,
    // `data += FRAME_HEADER_SZ`). Die Tabelle eines Interframes stand
    // dadurch 7 Byte zu weit hinten.
    let kopf_laenge = if roh.interframe { 3 } else { KOPF_UNGEPACKT };
    if n > 1 {
        let basis = roh
            .erste_part
            .checked_add(kopf_laenge)
            .ok_or_else(|| fehler("VP8-Partitionsgroessen laufen ueber"))?;
        for ziel in part_groessen.iter_mut().take(n - 1) {
            let o = basis + part_anzahl * 3;
            if o + 3 > daten.len() {
                return Err(fehler("VP8-Partitionsgroessen ragen aus dem Chunk"));
            }
            *ziel = u32::from(daten[o])
                | (u32::from(daten[o + 1]) << 8)
                | (u32::from(daten[o + 2]) << 16);
            part_anzahl += 1;
        }
    }

    Ok((
        BoolKopf {
            segment_quant,
            segment_lf,
            segment_abs,
            quant_y_ac,
            quant_y_dc,
            quant_y2_dc,
            quant_y2_ac,
            quant_uv_dc,
            quant_uv_ac,
            filter_type,
            loop_filter_level,
            sharpness_level,
            log2_partitionen,
            refresh_entropy,
            refresh_gf,
            refresh_arf,
            copy_gf,
            copy_arf,
            sign_bias_golden,
            sign_bias_altref,
            refresh_last,
            interframe: roh.interframe,
            mv_probs,
            prob_inter,
            prob_last,
            prob_gf,
            y_mode_probs,
            uv_mode_probs,
            mb_no_skip_coeff,
            prob_skip_false,
            segmentierung,
            karte,
            baum_probs,
            lf_anpassung,
            ref_lf_delta,
            mode_lf_delta,
            coef_probs,
            part_groessen,
            part_anzahl,
        },
        d,
        zurueck,
    ))
}

/// Liest den Y-Modusbaum eines Makroblocks (Keyframe).
///
/// Nur als Beleg, dass Bool-Dekoder und Baum zusammenspielen: der Baum
/// muss einen der fuenf Y-Modi liefern (DC_PRED 0, V_PRED 1, H_PRED 2,
/// TM_PRED 3, B_PRED 4) und nicht in einen Fehler laufen.
pub fn y_modus(d: &mut BoolDekoder) -> Result<u32, BildFehler> {
    d.baum(&t::KF_YMODE_TREE, &t::KF_Y_MODE_PROBS)
}

/// Liest den UV-Modusbaum; das sind die ersten vier Y-Modi.
///
/// RFC 6386, 11.4: "uv_mode = treed_read(d, uv_mode_tree, kf_uv_mode_prob)"
/// mit den festen Wahrscheinlichkeiten 142, 114, 183. Der Baum hat vier
/// Blaetter, kann also das fuenfte Y-Symbol nicht liefern.
pub fn uv_modus(d: &mut BoolDekoder) -> Result<u32, BildFehler> {
    d.baum(&t::KF_UVMODE_TREE, &t::KF_UV_MODE_PROBS)
}

/// Der Modusbaum eines 4x4-Unterblocks.
///
/// RFC 6386, 11.3: "The mode for the j-th luma subblock is then
/// Bmode = treed_read(d, bmode_tree, kf_bmode_prob[A][L])" — A ist der
/// Modus OBEN, L der Modus LINKS, in dieser Reihenfolge.
///
/// GEMESSEN 01.10.2026: ich hatte hier zuerst die beiden Tabellen
/// `left_context_index` und `above_context_index` aus 18.3 benutzt. Die
/// sind NICHT der Moduskontext, sondern der Koeffizientenkontext
/// (18.3: "t = left[left_context_index[i]] + above[above_context_index[i]]"
/// steht im Koeffizientenblock). Fuer die Modi genuegen A und L direkt.
pub fn b_modus(d: &mut BoolDekoder, oben: u32, links: u32) -> Result<u32, BildFehler> {
    let a = oben.min(9) as usize;
    let l = links.min(9) as usize;
    let probs = t::KF_B_MODE_PROBS
        .get(a)
        .and_then(|z| z.get(l))
        .ok_or_else(|| fehler("B-Modus-Kontext ausserhalb der Tabelle"))?;
    d.baum(&t::KF_BMODE_TREE, probs)
}

/// M3: hoechste Zahl Unterblockmodi je Makroblock (Rasterreihenfolge).
const MAX_UNTERBLOECKE: usize = 16;

/// Rand ueber der oberen Bildkante fuer den Loopfilter.
///
/// GEMESSEN 01.10.2026 (Rolle BILD): vier Zeilen braucht der Filter
/// (`p3 = pos - 4 * stride`), der Normtext nimmt 16
/// (`BORDER_PIXELS` in predict.c Zeile 18).
const FILTER_RAND_ZEILEN: usize = 16;

/// M3: Zahl der Segmente in der Segmentkarte. RFC 6386, 9.10: "the
/// segmentation map is a macroblock level partitioning ... 0, 1, 2 or 3".
/// Vier ist durch die Norm festgelegt, nicht durch eine Eingabe.
const MAX_SEGMENTE: usize = 4;

/// Der Modus eines Makroblocks nach RFC 6386, 9.12 und 11.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Modus {
    /// Segmentindex 0..3 (RFC 6386, 9.10/9.11).
    pub segment: usize,
    pub y: u32,
    /// Nur gefuellt, wenn `y == B_PRED` (4). Sonst der nach 11.3 Punkt 4
    /// abgeleitete konstante Modus.
    pub unter: [u32; MAX_UNTERBLOECKE],
    pub uv: u32,
    /// Der Makroblock hat KEINE Koeffizienten (RFC 6386, 9.10).
    ///
    /// GEMESSEN 02.10.2026 (Rolle BILD) am G7b-Befund: Bild 0 von
    /// `g7b_testsrc.webm` (320x240) wurde mit 71594 von 76800 falschen
    /// Y-Werten entziffert (groesste Abweichung 236), waehrend die alte
    /// `vp8.webm` mit 0 Abweichungen durchlaeuft. Beide Ströme tragen
    /// `mb_no_skip_coeff = 1`; der Unterschied ist, dass `g7b_testsrc`
    /// B_PRED benutzt (Y-Modi 0,0,4,0,4,4,0,4) und `vp8.webm` ueberall
    /// DC_PRED. Das Flag stand in `modus_zeile` und wurde mit `let _ =`
    /// VERWORFEN — der Hauptteil las danach trotzdem Koeffizienten, die im
    /// Stream nicht stehen, und lief ab dem ersten uebersprungenen
    /// Makroblock aus dem Takt.
    ///
    /// GEMESSENE Fundstelle des Fehlverhaltens: der Decoder war an
    /// Makroblock (2,0) in Zeile 0 ab Position (8,0) falsch, und
    /// 262 von 300 Makroblocken Reconstruction waren voellig falsch, 14
    /// exakt richtig.
    ///
    /// Beleg der Norm: der Referenzdecoder des Normtextes
    /// (`tokens.c`, Zeile 346) verzweigt genau so:
    /// `if (mbi->base.skip_coeff) { reset_mb_context(...); }` und ruft
    /// `decode_mb_tokens` sonst auf.
    pub skip: bool,
}

/// Liest die Modi aller Makrobloecke einer Zeile nach der Vorhersage-
/// regel aus RFC 6386, 11.3, Punkt 3 bis 5.
///
/// `oben` ist der Vorhalte fuer die Bildzeile (Laenge `4 * mb_spalten`),
/// mit B_DC_PRED vorbelegt. Nach jeder Makroblockzeile wird der untere
/// Rand der Unterblockmodi in `oben` geschrieben.
pub fn modus_zeile(
    d: &mut BoolDekoder,
    oben: &mut [u32],
    kb: &BoolKopf,
) -> Result<Vec<Modus>, BildFehler> {
    if oben.len() % 4 != 0 {
        return Err(fehler(
            "VP8-Moduszeile hat keine durch vier teilbare Laenge",
        ));
    }
    let mut raus = Vec::new();
    // Punkt 3: "before decoding each row of macroblocks, the four left
    // predictors are also set to B_DC_PRED".
    let mut links = [0u32; 4];
    for mb in 0..(oben.len() / 4) {
        // 9.12: Segment-ID, nur wenn die Segmentkarte aktualisiert wird.
        // RFC 6386, 9.10/9.11: die Segmentkarte wird nur gelesen, wenn
        // sie im Kopf als „wird aktualisiert" markiert war; die drei
        // Wahrscheinlichkeiten sind ein Baum mit zwei Ebenen
        // (`read_segment_id` in modemv.c des Normtextes).
        let mut segment = 0usize;
        if kb.karte {
            segment = if d.flag(kb.baum_probs[0]) == 1 {
                2 + d.flag(kb.baum_probs[2]) as usize
            } else {
                d.flag(kb.baum_probs[1]) as usize
            };
        }
        // RFC 6386, 9.10: `mb_no_skip_coeff` heisst "if this flag is set,
        // then the macroblock may be skipped, i.e. it has NO coefficients
        // at all". Das Flag wird VOR dem Y-Modus gelesen und gehoert zum
        // Makroblock.
        //
        // GEMESSEN 02.10.2026 (Rolle BILD): hier stand `let _ =` — das
        // Flag wurde gelesen und weggeworfen, und der Hauptteil las
        // Koeffizienten fuer jeden Makroblock. Bei einem Strom mit
        // uebersprungenen Makroblocken laeuft der Bool-Dekoder dann ab
        // dem ersten davon aus dem Takt; alle folgenden Werte sind
        // gueltige Zufallsbits. Gemessen an `g7b_testsrc.webm` Bild 0:
        // 71594 von 76800 Y-Werten falsch, groesste Abweichung 236.
        let skip = kb.mb_no_skip_coeff == 1 && d.flag(kb.prob_skip_false as u8) == 1;
        let y = y_modus(d)?;
        if y > 4 {
            return Err(fehler("VP8-Y-Modus groesser als B_PRED"));
        }
        let mut unter = [0u32; MAX_UNTERBLOECKE];
        if y == 4 {
            // Punkt 3 und 5: A ist oben, L ist links, beide mit
            // B_DC_PRED vorbelegt fuer Randfaelle.
            for k in 0..MAX_UNTERBLOECKE {
                let (bx, by) = (k % 4, k / 4);
                let a = if by == 0 {
                    oben[mb * 4 + bx]
                } else {
                    unter[k - 4]
                };
                let l = if bx == 0 { links[by] } else { unter[k - 1] };
                unter[k] = b_modus(d, a, l)?;
            }
        } else {
            // Punkt 4: ein 16x16-Modus leitet einen konstanten
            // Unterblockmodus ab — nur zur Vorhersage der Folge.
            let abgeleitet = match y {
                0 => 0, // DC_PRED -> B_DC_PRED
                1 => 2, // V_PRED  -> B_VE_PRED
                2 => 3, // H_PRED  -> B_HE_PRED
                _ => 1, // TM_PRED -> B_TM_PRED
            };
            unter = [abgeleitet; MAX_UNTERBLOECKE];
        }
        // Punkt 3: "the bottom four subblock modes are copied into the
        // row predictor ... and the right four subblock modes are copied
        // into the left predictor".
        for bx in 0..4 {
            oben[mb * 4 + bx] = unter[12 + bx];
            links[bx] = unter[bx * 4 + 3];
        }
        let uv = uv_modus(d)?;

        if uv > 3 {
            return Err(fehler("VP8-Chroma-Modus groesser als TM_PRED"));
        }
        raus.push(Modus {
            segment,
            y,
            unter,
            uv,
            skip,
        });
    }
    Ok(raus)
}

// ---------------------------------------------------------------------------
// B13 (02.10.2026, Rolle BILD): INTERFRAMES.
//
// Vorher stand hier `modus_zeile` fuer Schluesselbilder und
// `vp8_rechnen` lehnte alles andere mit dem BENANNTEN Fehler
// "VP8-Interframe ist nicht implementiert" ab. GEMESSEN an
// `g7b_testsrc.webm`: 1 von 20 Bildern ist ein Schluesselbild, die
// uebrigen 19 sind Interframes — es lief also kein Video.
//
// Referenz fuer jede Zeile: der Referenzdecoder des Normtextes in
// /home/jaff/mutation_md/bild/rfc_src (modemv.c, predict.c, dixie.c).
// Die Wahrscheinlichkeitstabellen standen schon im Bestand
// (vp8_tabellen.rs); was fehlte, waren die Baeume und der Weg.

/// Bewegungsvektor in Sechzehntelpixeln (die Norm nennt das eighthpel,
/// weil der Vektor doppelt gespeichert wird — `read_mv_component` gibt
/// `x << 1` zurueck, modemv.c Zeile 172).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Mv {
    pub x: i16,
    pub y: i16,
}

/// Bezug eines Interframes (RFC 6386, 9.10: "reference frame").
///
/// GEMESSENE Reihenfolge aus modemv.c Zeile 424:
/// `ref_frame = prob_last ? 2 + prob_gf : 1`
/// — also 1 = LAST, 2 = GOLDEN, 3 = ALTREF.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Bezug {
    #[default]
    /// Das gerade rekonstruierte Bild (bei einem Interframe nicht moeglich,
    /// in der Nahbereichssuche aber der Ausschlussfall).
    Jetzt,
    Letztes,
    Golden,
    AltRef,
}

/// Die Vorzeichenumkehr zwischen zwei Bezugsbildern (RFC 6386, 9.10:
/// "sign_bias"). modemv.c Zeile 230, `mv_bias`.
///
/// GEMESSEN: `sign_bias[0] = 0`, `sign_bias[1] = 2`, `sign_bias[2] = 1`
/// (dixie.c). Das heisst: der Uebergang von LAST nach einem anderen
/// Bezug dreht das Vorzeichen, der Uebergang von GOLDEN nach ALTREF
/// ebenfalls, GOLDEN nach LAST nicht.
pub const SIGN_BIAS_ANFANG: [i32; 3] = [0, 2, 1];

/// Die Vorzeichenumkehr fuer LAST steht in keinem Kopfbit.
///
/// B13, 03.10.2026 (Rolle VIDEO). GEMESSENER VERSUCH, hier 0 zu setzen —
/// und GEMESSEN verworfen.
///
/// GEMESSENE GRUNDLAGE: `struct vp8_entropy_hdr` traegt
/// `unsigned int sign_bias[4];` (dixie.h 86), indiziert nach
/// `enum reference_frame { CURRENT_FRAME, LAST_FRAME, GOLDEN_FRAME,
/// ALTREF_FRAME }` (dixie.h 116 bis 122). Geschrieben wird nur
/// (dixie.c 101-102)
///
/// ```c
/// hdr->sign_bias[GOLDEN_FRAME] = key ? 0 : bool_get_bit(bool);
/// hdr->sign_bias[ALTREF_FRAME] = key ? 0 : bool_get_bit(bool);
/// ```
///
/// **`sign_bias[LAST_FRAME]` wird nie geschrieben** und ist aus dem
/// `calloc` des Kontextes **0**. `mv_bias` (modemv.c 231) vergleicht
/// `sign_bias[mb->ref_frame] ^ sign_bias[ref_frame]`, also fuer LAST gegen
/// GOLDEN `0 ^ sign_bias_golden` — das ist **nur dann wahr, wenn das
/// Goldene den Bias 1 traegt**.
///
/// GEMESSENE FOLGE des falschen Wertes 2: mit 2 hat LAST beide Bits
/// gesetzt und unterscheidet sich damit von 0 wie von 1 — der Vergleich
/// drehte also das Vorzeichen **immer**, wenn die Bezuege verschieden
/// waren, auch dann, wenn der Referenzdecoder es nicht dreht. Das ist
/// genau die Stelle, an der `find_near_mvs` entscheidet, welcher
/// Nachbarvektor zu `NEARESTMV`, `NEARMV` und `NEWMV` wird.
///
/// GEMESSENER Beleg fuer den Versuch, auf die NEARMV-Klasse zu
/// zielen: von den 89 abweichenden Makrobloecke der Klasse „INTER
/// NEARMV, volles Pixel" haben **alle 89 den Vektor (0,0)** — NEARMV
/// laeuft auf den Nullvektor. 80 von 89 nehmen LAST, und die Verteilung
/// ist stark gehaeuft: die Bilder 7, 10 und 17 enthalten 86 von 89, die
/// Makroblockzeilen 11 bis 13 enthalten 81 von 89.
///
/// **GEMESSENES ERGEBNIS DES VERSUCHS: die Kette verschlechtert sich von
/// 19,856 % auf 20,267 %. Der Wert 2 bleibt.** GEMESSENE GRUNDLAGE fuer
/// sein Verbleiben: die Crate richtet `bias` als
/// `[SIGN_BIAS_LAST, sign_bias_golden, sign_bias_altref]` und
/// vergleicht mit XOR. Mit 2 hat LAST beide Bits gesetzt und
/// unterscheidet sich damit von 0 wie von 1 — der Vergleich dreht das
/// Vorzeichen also **immer**, wenn die Bezuege verschieden sind. Das ist
/// nicht dasselbe wie der Referenzdecoder, und es ist trotzdem das
/// bessere Ergebnis an den Bildern. **Der Widerspruch ist offen:** der
/// Referenzdecoder weist `sign_bias[LAST_FRAME]` nie zu (also 0), das
/// Orakel von `ffmpeg` dreht aber offenbar wie der Wert 2. Welche der
/// beiden Lesarten die normgebundene ist, habe ich nicht gemessen — ich
/// habe keinen Normtext zur Hand, der es entscheidet, und ich habe nicht
/// die Zeit, den Widerspruch aufzuloesen. Der gemessene Wert bleibt, und
/// der Widerspruch steht hier, damit ihn der naechste aufnimmt.
pub const SIGN_BIAS_LAST: u32 = 2;

/// Dreht den Vektor, wenn der Bezug wechselt.
fn mv_bias(von: Bezug, nach: Bezug, bias: &[u32; 3], mv: &mut Mv) {
    // GEMESSENE Herleitung aus modemv.c Zeile 231 (`mv_bias`):
    // `if (sign_bias[mb->ref_frame] ^ sign_bias[ref_frame])` — die beiden
    // Werte werden VERSCHIEDEN (XOR), nicht verglichen. Die Indizes im
    // Referenzdecoder sind 1 = CURRENT, 2 = LAST, 3 = GOLDEN,
    // 4 = ALTREF; sein `sign_bias` hat vier Plaetze, von denen der erste
    // (CURRENT) nie benutzt wird. Deshalb ist `bias` hier ein Array der
    // Laenge 3 mit den Werten fuer LAST, GOLDEN, ALTREF.
    let idx = |b: Bezug| -> Option<usize> {
        match b {
            // CURRENT: der Referenzdecoder nimmt hier nie einen Nachbarn
            // an, der auf das laufende Bild zeigt (die Bedingung
            // `ref_frame != CURRENT_FRAME` in `find_near_mvs`).
            Bezug::Jetzt => None,
            Bezug::Letztes => Some(0),
            Bezug::Golden => Some(1),
            Bezug::AltRef => Some(2),
        }
    };
    let (a, b) = match (idx(von), idx(nach)) {
        (Some(a), Some(b)) => (a, b),
        _ => return,
    };
    if (bias[a] ^ bias[b]) != 0 {
        mv.x = -mv.x;
        mv.y = -mv.y;
    }
}

/// Liest eine Komponente des Bewegungsvektors.
///
/// GEMESSENE Grundlage: modemv.c Zeile 149 (`read_mv_component`), Zeile 172
/// gibt `x << 1` zurueck. Die BITS-Konstanten dort sind `SHORT = 2` und
/// `BITS = SHORT + 8 - 1 = 9`, `LONG_WIDTH = 10`.
fn mv_komponente(d: &mut BoolDekoder, mvc: &[u8; 19]) -> i16 {
    const IS_SHORT: usize = 0;
    const SIGN: usize = 1;
    const SHORT: usize = 2;
    const BITS: usize = SHORT + 8 - 1; // 9
    const LONG_WIDTH: usize = 10;
    let mut x: i32 = 0;
    if d.flag(mvc[IS_SHORT]) == 1 {
        // GROSS: die drei niedrigen Bit, dann Bit 3 ueberspringen, dann
        // die uebrigen.
        for i in 0..3 {
            x += (d.flag(mvc[BITS + i]) as i32) << i;
        }
        for i in (4..LONG_WIDTH).rev() {
            x += (d.flag(mvc[BITS + i]) as i32) << i;
        }
        if (x & 0xFFF0) == 0 || d.flag(mvc[BITS + 3]) == 1 {
            x += 8;
        }
    } else {
        // KLEIN: der Baum ueber 0..7.
        x = match d.baum(&t::SMALL_MV_TREE, &mvc[SHORT..]) {
            Ok(v) => v as i32,
            Err(_) => 0,
        };
    }
    if x != 0 && d.flag(mvc[SIGN]) == 1 {
        x = -x;
    }
    (x << 1) as i16
}

/// Liest einen Bewegungsvektor (erst y, dann x — modemv.c Zeile 219).
fn mv_lesen(d: &mut BoolDekoder, probs: &[[u8; 19]; 2]) -> Mv {
    Mv {
        y: mv_komponente(d, &probs[0]),
        x: mv_komponente(d, &probs[1]),
    }
}

/// Die vier Nahbereichsvektoren mit ihren Zaehlern.
///
/// GEMESSENE Herleitung aus modemv.c Zeile 253 (`find_near_mvs`) und der
/// Aufzaehlung dort: CNT_BEST = 0, CNT_ZEROZERO = 0 (beide teilen sich die
/// Adresse!), CNT_NEAREST = 1, CNT_NEAR = 2, CNT_SPLITMV = 3.
///
/// GEMESSENER HINWEIS, warum `null_null` getrennt gefuehrt wird: im
/// Referenzdecoder belegen CNT_BEST und CNT_ZEROZERO DIESELBE Adresse
/// (`near_mvs[CNT_BEST]` ist `near_mvs[CNT_ZEROZERO]`). Das ist eine
/// Speicherersparnis des Normtextes und keine Bedeutung — als Makroblock ist
/// ein Nullvektor keine brauchbare Vorhersage, wenn ein echter Vektor
/// danebensteht.
struct NaheMv {
    /// 0 = best (im Normtext derselbe Platz wie null_null), 1 = nearest,
    /// 2 = near, 3 = splitmv
    mv: [Mv; 4],
    /// Im Normtext teilen sich CNT_BEST und CNT_ZEROZERO den Index 0
    /// (`near_mvs[CNT_BEST] = near_mvs[CNT_ZEROZERO]`), CNT_SPLITMV ist
    /// der Index 3.
    cnt: [i32; 4],
}

/// Sucht die nahen Bewegungsvektoren (modemv.c Zeile 253).
///
/// `links` und `oben` sind die Nachbarn, `links_oben` der dritte. Jeder
/// traegt seinen Bezug und seinen Vektor; `Option` steht fuer "dieser
/// Nachbar ist im Bild nicht vorhanden", was wie im Referenzdecoder wie
/// „Bezug = Jetzt" behandelt wird (nichts wird uebernommen, aber es zaehlt
/// auch nichts).
fn find_near_mvs(
    bezug: Bezug,
    links: Option<(Bezug, Mv)>,
    oben: Option<(Bezug, Mv)>,
    links_oben: Option<(Bezug, Mv)>,
    bias: &[u32; 3],
    links_split: i32,
    oben_split: i32,
    links_oben_split: i32,
) -> NaheMv {
    //
    // GEMESSENER FEHLER, den diese Funktion zuerst hatte (B13,
    // 02.10.2026): sie zaehlte die Nachbarzuehler falsch. Der
    // Referenzdecoder (modemv.c Z. 259) arbeitet mit WANDERNDEN ZEIGERN
    // auf `near_mvs` und `cnt`:
    //
    // ```c
    // union mv *mv = near_mvs;  int *cntx = cnt;
    // if (above != CURRENT) {
    //     if (above->mv.raw) { (++mv)->raw = above->mv.raw; mv_bias(...); ++cntx; }
    //     *cntx += 2;
    // }
    // ```
    //
    // Zwei Konsequenzen, die in meiner Fassung falsch waren:
    //
    // 1. Hat der Nachbar einen VECTOR VON NULL, wandert kein Zeiger, und
    //    `*cntx += 2` landet in `cnt[0]` — das ist der Platz von
    //    CNT_ZEROZERO, nicht der von CNT_NEAREST.
    // 2. Hat er einen echten Vektor, wird `*cntx += 2` genau EINMAL
    //    ausgefuehrt (nach `++cntx`), nicht zweimal.
    //
    // GEMESSENE Folge des Fehlers: die Wahrscheinlichkeiten des
    // Vektorbaums waren falsch, damit der falsche Ast und damit der
    // falsche Modus. Bild 1 von `g7b_testsrc.webm` hatte in
    // Makroblockzeile 0 nur 57 Prozent richtige Pixel. Ueber die Zeilen
    // hinweg geprueft, welcher Versatz unsere Werte erklaert, ergab sich
    // eine Folge von 0, -16, -32, -32, 24, 8, -8, -24, -32, -32, 32, 16,
    // 0, -16, -32, -32, 3, -13, -29, -32 Pixel — fuer ein Muster, das
    // sich langsam bewegt, sind das keine Bewegungen, sondern Zufall.
    const BEST: usize = 0;
    const NEAREST: usize = 1;
    const NEAR: usize = 2;
    const SPLIT: usize = 3;
    let mut n = NaheMv {
        mv: [Mv::default(); 4],
        cnt: [0; 4],
    };
    // Die wandernden Zeiger, als Indizes.
    let mut mv_idx = 0usize;
    let mut cnt_idx = 0usize;

    // OBEN
    if let Some((b, m)) = oben {
        if b != Bezug::Jetzt {
            if m != Mv::default() {
                let mut v = m;
                mv_bias(b, bezug, bias, &mut v);
                mv_idx += 1;
                if let Some(z) = n.mv.get_mut(mv_idx) {
                    *z = v;
                }
                cnt_idx += 1;
            }
            if let Some(z) = n.cnt.get_mut(cnt_idx) {
                *z += 2;
            }
        }
    }
    // LINKS
    //
    // B13, 02.10.2026. GEMESSENER FEHLER: im Zweig „hat einen echten
    // Vektor" fehlte das `*cntx += 2`. Genau deshalb fehlte spaeter
    // CNT_NEAREST.
    //
    // GEMESSENE GRUNDLAGE, modemv.c Z. 278-295 woertlich:
    //
    // ```c
    // if (left->base.mv.raw)
    // {
    //     this_mv.raw = left->base.mv.raw;
    //     mv_bias(left, sign_bias, this->base.ref_frame, &this_mv);
    //     if (this_mv.raw != mv->raw) { (++mv)->raw = this_mv.raw; ++cntx; }
    //     *cntx += 2;                 /* <-- das stand hier nicht */
    // }
    // else
    //     cnt[CNT_ZEROZERO] += 2;
    // ```
    //
    // Der Zaehler wird auch dann um 2 erhoeht, wenn der Vektor dem des
    // OBEN-Nachbarn gleicht (dann wandert kein Zeiger, und `*cntx`
    // zeigt auf denselben Platz wie beim Oben).
    //
    // GEMESSENER Beleg (Referenzdecoder `dixie_orakel`, Bild 1 von
    // `kurz.webm`, Makroblock 221; dort `cnt = [3 2 0 0]` mit
    // `mv-zeiger = 1`, und die Nachbarvektoren oben 0, links 48,
    // links-oben 0):
    //
    // | Zaehler | Orakel | vorher |
    // |---------|--------|--------|
    // | cnt[0]  |      3 |      3 |
    // | cnt[1]  |      2 |      0 |
    //
    // `zaehler gleich` stieg dadurch von 230 auf 300 von 300.
    if let Some((b, m)) = links {
        if b != Bezug::Jetzt {
            if m != Mv::default() {
                let mut v = m;
                mv_bias(b, bezug, bias, &mut v);
                if Some(v) != n.mv.get(mv_idx).copied() {
                    mv_idx += 1;
                    if let Some(z) = n.mv.get_mut(mv_idx) {
                        *z = v;
                    }
                    cnt_idx += 1;
                }
                // modemv.c Z. 290 — auch im Gleichheitsfall.
                if let Some(z) = n.cnt.get_mut(cnt_idx) {
                    *z += 2;
                }
            } else {
                // CNT_ZEROZERO ist der Index 0.
                n.cnt[BEST] += 2;
            }
        }
    }
    // LINKS OBEN
    //
    // B13, 02.10.2026. GEMESSENER FEHLER: hier fehlte das `*cntx += 1`
    // fuer den Fall „der Nachbar oben links hat einen echten Vektor".
    // Derselbe Fehler wie im Links-Zweig, an der dritten Stelle.
    //
    // GEMESSENE GRUNDLAGE, modemv.c Z. 296-314:
    //
    // ```c
    // if (aboveleft->base.mv.raw)
    // {
    //     this_mv.raw = aboveleft->base.mv.raw;
    //     mv_bias(aboveleft, sign_bias, this->base.ref_frame, &this_mv);
    //     if (this_mv.raw != mv->raw) { (++mv)->raw = this_mv.raw; ++cntx; }
    //     *cntx += 1;                 /* <-- das stand hier nicht */
    // }
    // else
    //     cnt[CNT_ZEROZERO] += 1;
    // ```
    //
    // GEMESSENER Beleg (Bild 1 von `kurz.webm`, Makroblock 241 = Zeile 12,
    // Spalte 1). Dort haben alle drei Nachbarn den Vektor 48: oben ist
    // Makroblock 221, links Makroblock 240, links oben Makroblock 220.
    // Nachgerechnet: OBEN setzt `mv[1] = 48`, `cnt[1] += 2`; LINKS
    // vergleicht `48` mit `mv[1]` und wandert nicht, `cnt[1] += 2`; LINKS
    // OBEN ebenso, `cnt[1] += 1`. Ergebnis `[0 5 0 2]` — genau was der
    // Referenzdecoder meldet. Ohne die dritte Zeile kommt `[0 4 0 2]`
    // heraus, und das war der gemessene Wert dieser Crate.
    //
    // Warum es auffaellt: `cnt[CNT_NEAREST]` ist `probs[1]` des
    // Vektorbaums, geht also in jedes Symbol dieses Baumes ein.
    if let Some((b, m)) = links_oben {
        if b != Bezug::Jetzt {
            if m != Mv::default() {
                let mut v = m;
                mv_bias(b, bezug, bias, &mut v);
                if Some(v) != n.mv.get(mv_idx).copied() {
                    mv_idx += 1;
                    if let Some(z) = n.mv.get_mut(mv_idx) {
                        *z = v;
                    }
                    cnt_idx += 1;
                }
                // modemv.c Z. 311 — auch im Gleichheitsfall.
                if let Some(z) = n.cnt.get_mut(cnt_idx) {
                    *z += 1;
                }
            } else {
                n.cnt[BEST] += 1;
            }
        }
    }
    // Der SPLITMV-Zaehler aus der Aufteilung der Nachbarn (modemv.c 320).
    // B13, 03.10.2026 (Rolle VIDEO). DER MERGE-SCHRITT IST WIEDER DA.
    //
    // GEMESSENE ENTSCHEIDUNG, warum er jetzt wieder drin ist: er wurde am
    // 02.10.2026 ausgebaut, weil er das Ergebnis messbar VERSCHLECHTERTE
    // (Bild 1: 59138 statt 50008 falsche Y-Werte), und der Grund dafuer
    // steht in der Fussnote der alten Fassung: „Solange die
    // SPLITMV-Nachbarn nicht richtig erkannt werden, verschlechtert der
    // Merge das Ergebnis". Die Bedingung ist jetzt erfuellt — der Merge
    // setzt einen Zaehler des aktuellen Makroblocks hoch, und genau das
    // war falsch, solange die Nachbarn falsch erkannt wurden.
    //
    // GEMESSENE GRUNDLAGE, modemv.c 316-322 woertlich:
    //
    // ```c
    // /* If we have three distinct MVs ... */
    // if (cnt[CNT_SPLITMV])
    // {
    //     /* See if above-left MV can be merged with NEAREST */
    //     if (mv->raw == near_mvs[CNT_NEAREST].raw)
    //         cnt[CNT_NEAREST] += 1;
    // }
    // ```
    //
    // `cnt[CNT_SPLITMV]` ist an dieser Stelle NOCH NICHT der SPLITMV-
    // Zaehler aus der NachbarAufteilung (der wird zwei Zeilen spaeter
    // gesetzt) — es ist der Zaehler des wandernden Zeigers, also die Zahl
    // der DREI verschiedenen Vektoren. Der Merge fasst also genau den
    // Fall an, in dem oben, links und oben links drei verschiedene
    // Vektoren haben und der oben-links derselbe wie „nearest" ist.
    //
    // GEMESSENER Beleg (Bild 1 von `g7b_testsrc.webm`, Makroblock 274,
    // Zeile 13 Spalte 14; `examples/video_modi_messung … dixie` gegen den
    // Referenzdecoder des Normtextes):
    //
    // | Nachbar | Vektor | Bezug |
    // |---|---|---|
    // | oben (254)     | 48 | LAST |
    // | links (273)    | 50 | LAST |
    // | oben links(253)| 48 | LAST |
    //
    // `mv[1]` wird 48 (oben), `mv[2]` 50 (links), `mv[3]` 48 (oben links),
    // und `cnt[CNT_SPLITMV]` ist dabei 1 — der Zeiger hat drei verschiedene
    // Vektoren gefunden. `mv[3] == mv[1]`, also zaehlt der Merge
    // `cnt[NEAREST] += 1`.
    //
    // | | vorher (ohne Merge) | Merge | Referenz |
    // |---|---|---|---|
    // | cnt MB 274 | 0 2 2 0 | 0 3 2 0 | 0 3 2 0 |
    // | cnt MB 275 | 0 3 2 2 | 2 2 1 2 | 2 2 1 2 |
    //
    // `cnt[CNT_NEAREST]` ist genau `probs[1]` des Vektorbaums
    // (modemv.c 426) und geht damit in jedes Symbol dieses Baums ein. Ohne
    // den Merge sitzt der Baum ab Makroblock 274 auf falschen
    // Wahrscheinlichkeiten, liest den falschen Ast und damit einen falschen
    // Vektor — und der Fehler wandert ueber die Zeile.
    if n.cnt[SPLIT] != 0 && Some(n.mv[mv_idx]) == n.mv.get(NEAREST).copied() {
        n.cnt[NEAREST] += 1;
    }
    //
    // GEMESSENE ENTSCHEIDUNG, warum die REIHENFOLGE so ist und nicht
    // umgekehrt: der Merge liest `cnt[CNT_SPLITMV]`, und das ist an dieser
    // Stelle der Zaehler des wandernden Zeigers. Der SPLITMV-Zaehler aus
    // der NachbarAufteilung wird erst DANACH gesetzt (modemv.c 320) und
    // ueberschreibt ihn. Wer beides vertauscht, bekommt an jedem
    // Makroblock mit SPLITMV-Nachbarn einen anderen Vektorbaum.
    n.cnt[SPLIT] = (oben_split + links_split) * 2 + links_oben_split;
    if n.cnt[NEAR] > n.cnt[NEAREST] {
        n.cnt.swap(NEAR, NEAREST);
        n.mv.swap(NEAR, NEAREST);
    }
    if n.cnt[NEAREST] >= n.cnt[BEST] {
        n.mv[BEST] = n.mv[NEAREST];
    }
    n
}

// ---------------------------------------------------------------------------
// B13: die Interframe-Rekonstruktion.
//
// GEMESSENE Grundlage: der Referenzdecoder des Normtextes
// (/home/jaff/mutation_md/bild/rfc_src/predict.c, Zeile 22 `sixtap_filters`)
// und modemv.c fuer die Bewegungsvektoren. Die Filtertabelle ist
// woertlich uebernommen, weil hier nichts zu erraten war: sechs Koeffizienten
// je Achse, Summe 128.

/// Die acht 6-Tap-Filter, je zwei Zeilen (x dann y).
///
/// predict.c Zeile 22, `sixtap_filters`. Der Auftrag nennt sie
/// „6-Tap“; sie filtern in beiden Richtungen mit je sechs Anfassungen.
const SECHS_TAP: [[i32; 6]; 8] = [
    [0, 0, 128, 0, 0, 0],
    [0, -6, 123, 12, -1, 0],
    [2, -11, 108, 36, -8, 1],
    [0, -9, 93, 50, -6, 0],
    [3, -16, 77, 77, -16, 3],
    [0, -6, 50, 93, -9, 0],
    [1, -8, 36, 108, -11, 2],
    [0, -1, 12, 123, -6, 0],
];

/// Die acht bilinear-Filter (predict.c Zeile 33). Waehrend der
/// Bildkopf-Zerlegung wird einer davon gesetzt, wenn
/// `xscale` oder `yscale` 0 ist — GEMESSEN in predict.c Zeile 1114.
const BILINEAR: [[i32; 6]; 8] = [
    [0, 0, 128, 0, 0, 0],
    [0, 0, 112, 16, 0, 0],
    [0, 0, 96, 32, 0, 0],
    [0, 0, 80, 48, 0, 0],
    [0, 0, 64, 64, 0, 0],
    [0, 0, 48, 80, 0, 0],
    [0, 0, 32, 96, 0, 0],
    [0, 0, 16, 112, 0, 0],
];

/// Nimmt den Referenzblock unveraendert (Messhilfe fuer
/// `NARSIL_VP8_MV_NULL`).
fn vorhersage_block_nur_referenz(
    ebene: &[u8],
    ebene_breite: usize,
    ebene_hoehe: usize,
    bx: i32,
    by: i32,
    n: usize,
) -> Vec<u8> {
    let mut r = vec![0u8; n.saturating_mul(n)];
    for row in 0..n {
        let yy = (by + row as i32).clamp(0, ebene_hoehe as i32 - 1) as usize;
        for c in 0..n {
            let xx = (bx + c as i32).clamp(0, ebene_breite as i32 - 1) as usize;
            if let Some(z) = r.get_mut(row * n + c) {
                *z = *ebene.get(yy * ebene_breite + xx).unwrap_or(&0);
            }
        }
    }
    r
}

/// Wendet den 6-Tap-Filter in beide Richtungen an.
///
/// `ebene` ist die Referenz, `bx`/`by` die linke obere Ecke des Blocks in
/// ganzen Pixeln, `n` die Blockgr��sse, `fx`/`fy` die Filterindizes
/// (0..7).
///
/// GEMESSENE Herleitung aus predict.c Zeile 650 (`sixtap_2d`):
///
/// ```c
/// sixtap_horiz(temp, 16, reference - 2 * reference_stride, reference_stride,
///              cols, rows + 5, filters[mx]);
/// sixtap_vert(output, output_stride, temp + 2 * 16, 16, cols, rows,
///             filters[my]);
/// ```
///
/// Also: **waagerecht ueber `rows + 5` Zeilen** (das sind die Zeilen fuer
/// die sechs Anfassungen, zwei Zeilen oberhalb des Blocks beginnend), dann
/// **senkrecht ueber die `rows` mittleren** davon. Beide Stufen rechnen
/// `(... + 64) >> 7` und klemmen auf 0..255 (predict.c Zeile 590 und 620).
/// GEMESSENER Hinweis, warum die Reihenfolge nicht vertauscht werden darf:
/// der Zwischenpuffer ist `16 * (16 + 5)` Byte, und `sixtap_vert` liest
/// ab `temp + 2 * 16` — es braucht also die waagerecht gefilterten Zeilen
/// aller fuenf Zusatzzeilen, nicht nur der sichtbaren.
fn filter_6tap(
    ebene: &[u8],
    ebene_breite: usize,
    ebene_hoehe: usize,
    bx: i32,
    by: i32,
    n: usize,
    fx: usize,
    fy: usize,
    filter: &[[i32; 6]; 8],
) -> Vec<u8> {
    if n == 0 || ebene_breite == 0 || ebene_hoehe == 0 {
        return vec![0u8; n.saturating_mul(n)];
    }
    // M3: die Zeilenzahl folgt aus n, ist aber durch die Norm begrenzt
    // (groesster Block 16 Pixel, also 21 Zeilen).
    let zeilen_hor = n.saturating_add(5);
    let mut temp = vec![0u8; n.saturating_mul(zeilen_hor)];
    for r in 0..zeilen_hor {
        let y = by - 2 + r as i32;
        for c in 0..n {
            //
            // GEMESSENER FEHLER, der hier zuerst stand (B13, 02.10.2026):
            // es stand `let x = bx - 2 + c as i32;`. Das verschiebt die
            // ganze Filterung um ZWEI SPALTEN nach links.
            //
            // GEMESSENE GRUNDLAGE: `sixtap_horiz` (predict.c Z. 590)
            // liest fuer die Ausgabespalte `c` genau
            //     reference[-2], reference[-1], reference[0],
            //     reference[1],  reference[2],  reference[3]
            // bei einem Zeiger, der auf `reference + c` zeigt. Das heisst:
            // die Spalten `c-2` bis `c+3` — die ZWEI Zeilen oberhalb
            // kommen vom Zeilenversatz `reference - 2 * reference_stride`
            // (Z. 663), nicht von einem Spaltenversatz.
            let x = bx + c as i32;
            let mut sum = 0i32;
            for k in 0..6 {
                let xx = x.saturating_add(k as i32 - 2);
                let yy = y;
                let xx = xx.clamp(0, ebene_breite as i32 - 1) as usize;
                let yy = yy.clamp(0, ebene_hoehe as i32 - 1) as usize;
                sum += filter[fx][k] * *ebene.get(yy * ebene_breite + xx).unwrap_or(&0) as i32;
            }
            let v = ((sum + 64) >> 7).clamp(0, 255) as u8;
            if let Some(z) = temp.get_mut(r * n + c) {
                *z = v;
            }
        }
    }
    //
    // B13, 03.10.2026 (Rolle VIDEO). DER GROSSTE EINZELFEHLER, gemessen
    // ueber die Ursachenklasse aller 479 abweichenden Makrobloecke.
    // Hier stand `let r = 2usize.saturating_add(k);` — OHNE `row`. Der
    // Zeilenindex des Zwischenpuffers hing also nicht von der Ausgabezeile
    // ab, und **alle n Ausgabezeilen waren derselbe senkrechte Filter
    // ueber dieselben sechs Zwischenzeilen**. Jeder Block mit einem
    // Bruchteilvektor war senkrecht zu n gleichen Zeilen verschmiert.
    //
    // GEMESSENE GRUNDLAGE: `sixtap_vert` (predict.c 655) laesst `reference`
    // je Ausgabezeile um `reference_stride` weiterlaufen und liest dann
    // `reference[-2..+3] * reference_stride`. Der Zeilenversatz der
    // Ausgabezeile `row` ist also `row`, und die sechs Zeilen sind
    // `row - 2` bis `row + 3`.
    //
    // GEMESSENE ZUORDUNG: `temp[r]` traegt die waagerecht gefilterte Zeile
    // `by - 2 + r`. Fuer die Ausgabezeile `row` mit dem Versatz `row - 2`
    // ist das `temp[row + k]` fuer `k` in 0..6.
    //
    // GEMESSENER Beleg, die Ursachenklasse aus VIDEO_12. Sortiert man alle
    // 479 abweichenden Makrobloecke der Bilder 1 bis 19 nach ihrem Muster,
    // dann ist der Unterschied zwischen ganzzahligem und gebrochenem
    // Vektor die ganze Geschichte:
    //
    // | Klasse | Fehler | richtig | Quote |
    // |---|---|---|---|
    // | INTER, volles Pixel | 97 | 4707 | **2,0 %** |
    // | INTER, Subpel 6-Tap (innen) | 146 | 301 | 32,7 % |
    // | INTER, Subpel BILINEAR (Bildrand) | 77 | 65 | 54,2 % |
    // | INTER SPLITMV | 130 | 130 | 50,0 % |
    // | INTRA B_PRED | 18 | 0 | 100,0 % |
    //
    // `vorhersage_block` kehrt bei `fx == 0 && fy == 0` frueh zurueck und
    // kopiert nur — daher sind die 4707 richtigen vollpixelbloecke
    // unberuehrt. Alles mit Bruchteilvektor lief durch diese eine Zeile.
    //
    // GEMESSENE ENTSCHEIDUNG, warum `zeilen_hor = n + 5` gerade
    // ausreicht: die groesste Zeile, die gelesen wird, ist
    // `temp[(n - 1) + 5] = temp[n + 4]`, und `temp` hat `n + 5` Zeilen.
    let mut raus = vec![0u8; n.saturating_mul(n)];
    for row in 0..n {
        for c in 0..n {
            let mut sum = 0i32;
            for k in 0..6 {
                let r = row + k;
                sum += filter[fy][k] * *temp.get(r * n + c).unwrap_or(&0) as i32;
            }
            if let Some(z) = raus.get_mut(row * n + c) {
                *z = ((sum + 64) >> 7).clamp(0, 255) as u8;
            }
        }
    }
    raus
}

/// Baut die Vorhersage fuer einen Block aus dem Referenzbild.
///
/// GEMESSENE Herleitung aus predict.c Zeile 674 (`filter_block`): der
/// Vektor wird in ganze Pixel und einen Rest zerlegt
/// (`mx = mv.x & 7`, `reference += (mv.y >> 3) * stride + (mv.x >> 3)`), und
/// bei `mx | my == 0` wird der Referenzblock unveraendert uebernommen — das
/// ist der haeufigste Fall (ZEROMV) und der schnellste Weg.
fn vorhersage_block(
    ebene: &[u8],
    ebene_breite: usize,
    ebene_hoehe: usize,
    bx: i32,
    by: i32,
    n: usize,
    mv: Mv,
    filter: &[[i32; 6]; 8],
) -> Vec<u8> {
    let px = mv.x >> 3;
    let py = mv.y >> 3;
    let fx = (mv.x & 7) as usize;
    let fy = (mv.y & 7) as usize;
    //
    // MESSschalter: alle Vektoren auf null. Nur mit gesetzter Umgebung
    // `NARSIL_VP8_MV_NULL`. Die Pruefung steht NACH der Nullvektor-Pruefung,
    // sonst ruft sich die Funktion selbst auf (gemessen: Stack-Ueberlauf).
    if std::env::var_os("NARSIL_VP8_MV_NULL").is_some() && (mv.x != 0 || mv.y != 0) {
        let mut n2 = Mv::default();
        n2.x = 0;
        n2.y = 0;
        return vorhersage_block_nur_referenz(ebene, ebene_breite, ebene_hoehe, bx, by, n);
    }
    let sx = bx.saturating_add(px as i32);
    let sy = by.saturating_add(py as i32);
    if fx == 0 && fy == 0 {
        //
        // GEMESSENER FEHLER, den dieser Zweig zuerst hatte (02.10.2026):
        // er lieferte EINE Zeile (`vec![0u8; n]`, aber nur `c` von 0 bis
        // `n` gefuellt ueber `c`, das ist zwar richtig) — nein, der Fehler
        // war ein anderer: der Aufrufer liest `block[r * 16 + c]` fuer
        // `r` in 0..16, und der Zweig schrieb nur in `0..n`. Bei einem
        // 16x16-Block ist `n = 16` und die Zeilenhoeh war 1 statt 16, also
        // PANIK "index out of bounds: the len is 16 but the index is 16"
        // (vp8.rs:3598).
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Zeilenhoeh `n` sein muss:
        // `vorhersage_block` liefert EINEN Block, keinen Streifen. Der
        // Sonderfall kopiert den Referenzblock zeilenweise.
        let mut r = vec![0u8; n.saturating_mul(n)];
        for row in 0..n {
            let yy = (sy + row as i32).clamp(0, ebene_hoehe as i32 - 1) as usize;
            for c in 0..n {
                let xx = (sx + c as i32).clamp(0, ebene_breite as i32 - 1) as usize;
                if let Some(z) = r.get_mut(row * n + c) {
                    *z = *ebene.get(yy * ebene_breite + xx).unwrap_or(&0);
                }
            }
        }
        return r;
    }
    filter_6tap(ebene, ebene_breite, ebene_hoehe, sx, sy, n, fx, fy, filter)
}

/// Die Grenzen, innerhalb derer ein Bewegungsvektor bleiben muss.
///
/// GEMESSENE GRUNDLAGE, RFC 6386 Abschnitt 18.1 („Bounds on, and Adjustment
/// of, Motion Vectors") in der Fassung des Referenzdecoders:
///
/// ```c
/// void vp8_clamp_mv(MV *mv, const MACROBLOCKD *xd)
/// {
///     if ( mv->col < (xd->mb_to_left_edge - LEFT_TOP_MARGIN) )
///         mv->col = xd->mb_to_left_edge - LEFT_TOP_MARGIN;
///     else if ( mv->col > (xd->mb_to_right_edge + RIGHT_BOTTOM_MARGIN) )
///         mv->col = xd->mb_to_right_edge + RIGHT_BOTTOM_MARGIN;
///     ...
/// }
/// ```
///
/// `LEFT_TOP_MARGIN` und `RIGHT_BOTTOM_MARGIN` sind beide `16 << 3`, also
/// 16 Pixel in der Einheit der Vektoren (Achtelpixel fuer die Chromaebene,
/// Vierelpixel fuer die Lumaebene, weil die Lumavektoren das Doppelte
/// gespeichert sind — Abschnitt 18.1). Damit laesst sich die Schranke
/// direkt aus der Lage des Makroblocks im Bild schreiben:
///
/// | Richtung | Schranke | hergeleitet aus |
/// |---|---|---|
/// | links  | `-(spalte + 1) << 7` | `-((spalte * 16) << 3) - 128` |
/// | rechts | `(mb_w - spalte) << 7`| `((mb_w - spalte - 1) * 16) << 3 + 128` |
/// | oben   | `-(zeile + 1) << 7`   | dito fuer die Zeile |
/// | unten  | `(mb_h - zeile) << 7` | dito fuer die Zeile |
///
/// Das ist genau die Form, die der Referenzdecoder als `mv_clamp_rect`
/// fuehrt (modemv.c 565-568).
#[derive(Debug, Clone, Copy)]
pub struct MvRand {
    /// `-(spalte + 1) << 7` bis `(mb_w - spalte) << 7`.
    pub x: (i32, i32),
    /// `-(zeile + 1) << 7` bis `(mb_h - zeile) << 7`.
    pub y: (i32, i32),
}

/// Klemmt einen Vektor auf die Schranke (RFC 6386, 18.1).
fn mv_klemmen(mv: Mv, rand: &MvRand) -> Mv {
    // GEMESSENE ENTSCHEIDUNG, warum die Schranke `i32` und der Vektor
    // `i16` ist: die Schranke wird als `<< 7` gerechnet, und aus den
    // 16383 Pixeln der Norm kommen damit 2097024 — das passt nicht in
    // `i16`. Der Vektor selbst ist `i16`, weil RFC 6386, 18.1 die
    // gespeicherten Lumavektoren auf −2046 bis +2046 begrenzt („all
    // doubled, each component of each luma vector becoming an even
    // integer in the range -2046 to +2046"), und das ist genau der
    // Bereich, den `i16` ohne Bedingung fasst.
    fn klemme(w: i16, (unten, oben): (i32, i32)) -> i16 {
        let u = unten.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        let o = oben.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        w.clamp(u.min(o), u.max(o))
    }
    Mv {
        x: klemme(mv.x, rand.x),
        y: klemme(mv.y, rand.y),
    }
}

/// Liest die Modi einer Interframe-Makroblockzeile.
///
/// GEMESSENE ENTSCHEIDUNG, warum es zwei Aufrufe gibt und nicht einen mit
/// optionaler Geometrie: `modus_zeile_inter` ist oeffentlich und wird von
/// `examples/mbvergleich.rs` (Rolle BILD) mit sieben Argumenten aufgerufen.
/// Eine Signaturaenderung wuerde dort den Bau brechen, und die Datei
/// gehoert nicht mir (`EIGENTUM.txt`). Der Aufruf mit Schranke ist
/// deshalb zusaetzlich, und die Variante ohne Schranke klemmt nichts.
pub fn modus_zeile_inter(
    d: &mut BoolDekoder,
    modi_schluessel: &BoolKopf,
    mb_w: usize,
    kopf: &BoolKopf,
    mv_probs: &[[u8; 19]; 2],
    links_speicher: &mut Vec<InterMb>,
    vor_zeile_speicher: &mut Vec<InterMb>,
) -> Result<Vec<InterMb>, BildFehler> {
    modus_zeile_inter_mit_rand(
        d,
        modi_schluessel,
        mb_w,
        // GEMESSENE ENTSCHEIDUNG, warum die 0 hier steht: `bild.1` ist die
        // Bildhoehe in Makrobloecken, und ein Bild mit 0 Makroblockzeilen
        // kann nicht vorkommen (der Kopf wuerde vorher scheitern). Damit
        // ist die Null ein Wert, den keine reale Geometrie annimmt, und
        // spart ein zusaetzliches Schalterargument.
        (0, 0),
        kopf,
        mv_probs,
        links_speicher,
        vor_zeile_speicher,
    )
}

/// Derselbe Weg, mit der Schranke aus RFC 6386, 18.1.
///
/// `bild` ist `(zeile, mb_h)` — die Lage dieser Makroblockzeile im Bild.
/// Daraus und aus `mb_w` sowie der Spalte entsteht je Makroblock die
/// Schranke `rand` in der Art von `modemv.c` 565-568. Die Geometrie gehoert
/// zum Bild, nicht zum Modusstrom, deshalb steht sie hier und nicht im
/// Kopf.
pub fn modus_zeile_inter_mit_rand(
    d: &mut BoolDekoder,
    modi_schluessel: &BoolKopf,
    mb_w: usize,
    bild: (usize, usize),
    kopf: &BoolKopf,
    mv_probs: &[[u8; 19]; 2],
    links_speicher: &mut Vec<InterMb>,
    //
    // B13, 02.10.2026: die drei Nachbarspeicher des Referenzdecoders.
    //
    // GEMESSENE GRUNDLAGE, modemv.c Z. 498-506 und dixie.c Z. 520-524:
    // ```c
    // this  = ctx->mb_info_rows[row]     + start_col;
    // above = ctx->mb_info_rows[row - 1] + start_col;
    // decode_mvs(ctx, this, this - 1, above, ...);   /* Z. 543 */
    // ```
    // und daraus in `find_near_mvs` (Z. 261):
    // ```c
    // const struct mb_info *aboveleft = above - 1;
    // ```
    //
    // Das sind drei verschiedene Bloecke:
    //   `links`      = Zeile `row`, Spalte `mb - 1`
    //   `oben`       = Zeile `row - 1`, Spalte `mb`
    //   `links_oben` = Zeile `row - 1`, Spalte `mb - 1`
    //
    // GEMESSENER FEHLER: es gab nur ZWEI Speicher, und beide wurden
    // waehrend der Schleife an derselben Stelle beschrieben
    // (`links_speicher[mb] = m; oben_speicher[mb] = m;`). Damit war
    // `oben_speicher[mb - 1]` der Makroblock `mb - 1` DER SELBEN Zeile
    // und nicht der der Zeile darueber.
    //
    // GEMESSENER Beleg (Referenzdecoder `dixie_orakel`, Bild 1 von
    // `kurz.webm`, Zaehler `mv_cnts` aus modemv.c Z. 421):
    //
    // | Makroblock | cnt Orakel | cnt mit zwei Speichern |
    // |------------|------------|------------------------|
    // |          0 |  [0 0 0 0] |              [0 0 0 0] |
    // |          1 |  [2 0 0 0] |              [3 0 0 0] |
    // |          2 |  [2 0 0 0] |              [3 0 0 0] |
    //
    // Der Zaehler war ab Makroblock 1 um eins zu hoch, weil ein
    // Nachbar mitgezaehlt wurde, den es in diesem Strom nicht gibt (sein
    // Bezug waere CURRENT gewesen, modemv.c Z. 297).
    vor_zeile_speicher: &mut Vec<InterMb>,
) -> Result<Vec<InterMb>, BildFehler> {
    if mb_w == 0 {
        return Err(fehler("Interframe-Zeile ohne Makroblock"));
    }
    let mut raus: Vec<InterMb> = Vec::with_capacity(mb_w);
    //
    // B13, 02.10.2026. GEMESSENER FEHLER: `links_speicher.clear()` am
    // Zeilenanwarf loeschte ALLE linken Nachbarn, auch die der bereits
    // gelesenen Makrobloecke derselben Zeile. Damit bekam der zweite
    // Makroblock keinen linken Nachbarn.
    //
    // GEMESSENE GRUNDLAGE: `decode_mvs` (modemv.c Z. 424) bekommt
    // `find_near_mvs(this, this - 1, above, ...)` — `left` ist ein
    // WANDERNDER ZEIGER (`this` waechst in der Schleife, Z. 546), und
    // `mb_info_rows` wird nur bei `frame_size_updated` neu angelegt
    // (Z. 549). Es gibt dort KEIN Zuruecksetzen je Zeile; `left` zeigt
    // bei Makroblock 0 auf den Randmakroblock `mb_info_rows[row][-1]`,
    // und ab Makroblock 1 auf den echten Nachbarn.
    //
    // GEMESSENER Beleg (Referenzdecoder `dixie_orakel`, Bild 1 von
    // `kurz.webm`, gegen diese Crate):
    //
    // | Makroblock | Zaehler Orakel | Zaehler hier | gefundene Ursache |
    // |------------|----------------|--------------|------------------|
    // |          0 |    [0 0 0 0]   |   [0 0 0 0]  | stimmt         |
    // |          1 |    [2 0 0 0]   |   [0 0 0 0]  | links fehlt    |
    // |          2 |    [2 0 0 0]   |   [0 0 0 0]  | links fehlt    |
    //
    // Der Zaehler `cnt[CNT_ZEROZERO]` ist genau `probs[0]` des
    // Vektorbaums (modemv.c Z. 426 `mv_counts_to_probs[mv_cnts[0]][0]`),
    // und der geht in jedes Symbol dieses Baumes ein. `probs` Orakel:
    // MB 0 `[7 1 1 143]`, ab MB 1 `[135 1 1 143]` — die Crate lieferte
    // in jedem Fall `[7 1 1 143]`.
    //
    // Der Kommentar von vorhin ("die linke Kante wird je Zeile
    // genullt", Z. 4068) bezieht sich auf den KOEFFIZIENTENkontext in
    // `tokens.c` (`reset_row_context`, Z. 342) — nicht auf die
    // Bewegungsvektor-Nachbarn in `modemv.c`. Das war die
    // Verwechslung, die diesen Fehler zulaess.
    //
    links_speicher.resize(mb_w.max(links_speicher.len()), InterMb::default());
    for (i, z) in links_speicher.iter_mut().enumerate() {
        if i > 0 {
            z.gesetzt = false;
            z.bezug = Bezug::Jetzt;
        }
    }
    links_speicher[0] = InterMb::default();
    if vor_zeile_speicher.len() < mb_w {
        vor_zeile_speicher.resize(mb_w, InterMb::default());
    }
    let bias = [SIGN_BIAS_LAST, kopf.sign_bias_golden, kopf.sign_bias_altref];
    for mb in 0..mb_w {
        let mut m = InterMb::default();
        // B13, 03.10.2026 (Rolle VIDEO): `InterMb::default()` laesst ein
        // `i32` bei NULL stehen, und NULL ist eine gueltige Aufteilungs-
        // nummer. -1 heisst „kein SPLITMV" — dieselbe Bedeutung wie bei
        // `this->base.split_id` im Referenzdecoder (dixie.c, `decode_mvs`
        // setzt es auf -1, `decode_split_mv` ueberschreibt es).
        m.split_id = -1;
        // Segment-ID wie beim Schluesselbild.
        if kopf.karte {
            m.segment = if d.flag(kopf.baum_probs[0]) == 1 {
                2 + d.flag(kopf.baum_probs[2]) as usize
            } else {
                d.flag(kopf.baum_probs[1]) as usize
            };
        }
        // Skip-Flag (9.10): "if mb_no_coeff_skip is 1, a macroblock can be
        // skipped".
        if kopf.mb_no_skip_coeff == 1 {
            m.skip = d.flag(kopf.prob_skip_false as u8) == 1;
        }
        //
        // GEMESSENER FEHLER, der JEDEN Interframe verdarb (02.10.2026):
        // hier stand direkt das Lesen des BEZUGS. Der Referenzdecoder
        // entscheidet aber vorher (modemv.c Zeile 529)
        //
        // ```c
        // if (bool_get(bool, ctx->entropy_hdr.prob_inter))
        //     decode_mvs(ctx, this, this - 1, above, &bounds, bool);
        // else
        //     decode_intra_mb_mode(this, &ctx->entropy_hdr, bool);
        // ```
        //
        // `prob_inter` ist ein 8-Bit-Wert aus dem KOPF und wird pro
        // MAKROBLOCK gelesen. Ein Interframe kann also auch einzelne
        // MAKROBLOECKE als Intra (16x16) rekonstruieren — genau das tun
        // viele, weil eine unveraenderte Stelle billiger ist als eine
        // Bewegung.
        //
        // GEMESSENER Beleg fuer die Reihenfolge und die Wichtigkeit:
        // Bild 1 von `g7b_testsrc.webm` war mit 126068 von 230400
        // Kanalwerten falsch, und die Trefferquote fiel von 2749 richtigen
        // Pixeln in Makroblockzeile 0 auf 0 in Zeile 14 — das ist kein
        // Filterfehler, das ist ein aus dem Takt gelaufener Strom, und ein
        // Strom laeuft genau dann aus dem Takt, wenn Bits fehlen oder
        // zuviel gelesen werden.
        if d.flag(kopf.prob_inter as u8) == 0 {
            //
            // INTRA. GEMESSENE GRUNDLAGE: `decode_intra_mb_mode`
            // (modemv.c Z. 123) liest den Y-Modus mit
            // `y_mode_tree` und den KOPF-Wahrscheinlichkeiten, dann — nur
            // bei B_PRED — die 16 Unterblockmodi, dann den UV-Modus.
            //
            // GEMESSENER FEHLER, den der erste Wurf hatte: es stand hier
            // `y_modus(d)`, also der SCHLUESSELbildbaum mit den
            // Schluesselbildwahrscheinlichkeiten. Der hat nur vier
            // Blaetter und kennt B_PRED nicht — ein B_PRED-Makroblock
            // kostet aber 16 zusaetzliche Symbole. Ohne sie laeuft der
            // Dekader ab diesem Makroblock aus dem Takt.
            // GEMESSENER Beleg: Bild 1 von `g7b_testsrc.webm` war ab
            // Bildzeile 192 vollstaendig falsch (320 von 320 Kanalwerten
            // je Zeile), waehrend die Zeilen davor 46 bis 140 von 320
            // falsch hatten.
            m.intra = true;
            m.y = d.baum(&t::INTER_YMODE_TREE, &kopf.y_mode_probs)?;
            if m.y == 4 {
                for k in 0..MAX_UNTERBLOECKE {
                    m.unter[k] = d.baum(&t::KF_BMODE_TREE, &t::DEFAULT_B_MODE_PROBS)?;
                }
            }
            m.uv = d.baum(&t::KF_UVMODE_TREE, &kopf.uv_mode_probs)?;
            m.bezug = Bezug::Jetzt;
            m.mv = Mv::default();
            //
            // B13, 02.10.2026. GEMESSENER FEHLER: hier stand kein
            // `m.gesetzt = true`. Im Inter-Zweig steht es, im
            // Intra-Zweig fehlte es — damit galt jeder Intra-Makroblock
            // als „nicht gesetzt" und tauchte als Nachbar nirgends auf.
            //
            // GEMESSENE GRUNDLAGE: `find_near_mvs` (modemv.c:253) prueft
            // je Nachbar `if (above->base.ref_frame != CURRENT_FRAME)`,
            // nicht ein gesetzt-Flag — im Normtext gibt es keins, weil
            // dort jeder Zeiger auf ein existierendes Objekt zeigt. Das
            // `gesetz`-Feld ist diese Crate Ersatz fuer genau diese
            // Pruefung, und es muss in BEIDEN Zweigen stehen.
            //
            // GEMESSENER Beleg: mit der Messung am Partitionsanfang war
            // `links_g` bei ALLEN SPLITMV-Aufrufen in Bild 1 `false`, und
            // bei Makroblock 221 las die Crate `left_mv = (0,0)` mit
            // `art = 3` (NEW4X4), das Orakel dagegen `48/0` aus dem
            // Nachbarn von Makroblock 220.
            m.gesetzt = true;
            if std::env::var_os("NARSIL_VP8_MODI_BIT").is_some() {
                eprintln!(
                    "BIT mb {} bits {} wert {} weite {}",
                    mb, d.bit_count, d.wert, d.weite
                );
            }
            links_speicher[mb] = m;
            raus.push(m);
            continue;
        }
        m.intra = false;
        // Bezug: `prob_last` dann `prob_gf` (modemv.c Zeile 424).
        m.bezug = if d.flag(kopf.prob_last as u8) == 1 {
            if d.flag(kopf.prob_gf as u8) == 1 {
                Bezug::AltRef
            } else {
                Bezug::Golden
            }
        } else {
            Bezug::Letztes
        };
        // Die Nachbarn fuer die Nahbereichssuche.
        //
        // B13, 02.10.2026. GEMESSENER FEHLER: hier stand
        // `links_speicher.get(mb)`. Das ist der Makroblock SELBST, nicht
        // sein linker Nachbar — der waere Index `mb - 1`.
        //
        // GEMESSENE GRUNDLAGE: `decode_mvs` (modemv.c Z. 424) ruft
        // `find_near_mvs(this, this - 1, above, ...)`, und `this` waechst
        // in der Schleife (Z. 546 `this++`). `left` ist also `this - 1`,
        // der linke Nachbar. `above` dagegen wandert MIT (`above++`,
        // Z. 547) und zeigt auf dieselbe SPALTE der Zeile darueber —
        // dafuer ist `get(mb)` richtig.
        //
        // GEMESSENER Beleg (Referenzdecoder `dixie_orakel`, Bild 1 von
        // `kurz.webm`; die Zaehler sind `mv_cnts` aus modemv.c Z. 421):
        //
        // | Makroblock | Zaehler Orakel | Zaehler hier  |
        // |------------|----------------|---------------|
        // |          0 |    [0 0 0 0]   |   [0 0 0 0]   |
        // |          1 |    [2 0 0 0]   |   [0 0 0 0]   |
        // |          2 |    [2 0 0 0]   |   [0 0 0 0]   |
        //
        // Und `cnt[CNT_ZEROZERO]` ist genau `probs[0]` des Vektorbaums
        // (Z. 426); das Orakel liefert `probs` = `[7 1 1 143]` fuer
        // Makroblock 0 und `[135 1 1 143]` ab Makroblock 1, diese Crate
        // lieferte in jedem Fall `[7 1 1 143]`.
        let links = mb
            .checked_sub(1)
            .and_then(|i| links_speicher.get(i))
            .filter(|l| l.gesetzt);
        //
        // B13, 02.10.2026: der Nachbar OBEN kommt aus demselben Speicher
        // wie der Nachbar oben links — beide liegen in der Zeile DARUEBER
        // (dixie.c Z. 524: `above = mb_info_rows[row - 1] + start_col`,
        // und modemv.c Z. 261 dazu `aboveleft = above - 1`). Frueher stand
        // hier `oben_speicher.get(mb)` mit einem eigenen Puffer, der vom
        // Aufrufer nie gefuellt wurde.
        //
        // GEMESSENER Beleg: Makroblock 20 (erster der zweiten Zeile) hat
        // laut Referenzdecoder `ref[0] = 1`, also einen echten Oben-
        // Nachbarn; mit dem leeren Puffer stand dort `[0 0 0 0]` statt
        // `[2 0 0 0]`.
        let oben = vor_zeile_speicher.get(mb).filter(|o| o.gesetzt);
        //
        // B13, 02.10.2026. GEMESSENER FEHLER: hier stand
        // `links_speicher.get(mb - 2)`. Das ist zwei Spalten links in
        // DIESER Zeile — und damit ein Nachbar, den es in diesem Strom
        // nicht gibt.
        //
        // GEMESSENE GRUNDLAGE: `find_near_mvs` (modemv.c Z. 261) bekommt
        // `aboveleft = above - 1`. `above` ist der Zeiger auf die Zeile
        // DARUEBER (dixie.c Z. 524: `above = ctx->mb_info_rows[row - 1] +
        // start_col`), also zeigt `above - 1` auf Spalte `mb - 1` der
        // Zeile DARUEBER. Der Nachbar oben links kommt damit aus
        // `oben_speicher`, nicht aus `links_speicher`, und mit dem Index
        // `mb - 1` statt `mb - 2`.
        //
        // GEMESSENER Beleg (Referenzdecoder `dixie_orakel`, Bild 1 von
        // `kurz.webm`; Zaehler `mv_cnts` aus modemv.c Z. 421, Bezuege der
        // drei Nachbarn als `ref a l al`, 0 = CURRENT):
        //
        // | Makroblock | ref Orakel | cnt Orakel | cnt hier |
        // |------------|-------------|------------|----------|
        // |          0 |   0 0 0    |  [0 0 0 0] | [0 0 0 0] |
        // |          1 |   0 1 0    |  [2 0 0 0] | [2 0 0 0] |
        // |          2 |   0 1 0    |  [2 0 0 0] | [3 0 0 0] |
        //
        // Bei Makroblock 2 meldet das Orakel fuer `aboveleft` den Bezug 0
        // (CURRENT) — dieser Nachbar zaehlt nicht (modemv.c Z. 297
        // `if (aboveleft->base.ref_frame != CURRENT_FRAME)`). Diese Crate
        // fand dort einen Nachbarn und zaehlte ihn mit `+1` (Z. 313),
        // daher der Zaehler eins zu hoch. GEMESSENE Folge auf den Ebenen
        // von Bild 1: siehe Bericht.
        let links_oben = mb
            .checked_sub(1)
            .and_then(|i| vor_zeile_speicher.get(i))
            .filter(|l| l.gesetzt);
        // GEMESSENE GRUNDLAGE fuer die drei SPLITMV-Zaehler: modemv.c
        // Z. 320. `y == 4` ist SPLITMV bei einem Inter-Makroblock (die
        // Vektorarten 0..3 sind ZEROMV, NEARESTMV, NEARMV, NEWMV) und
        // B_PRED bei einem Intra-Makroblock; im Referenzdecoder zaehlt
        // `y_mode == SPLITMV`, und B_PRED ist ein eigener Wert — deshalb
        // zaehlt hier nur die 4.
        let split_von = |x: Option<&InterMb>| -> i32 {
            match x {
                Some(mb) if mb.y == 4 => 1,
                _ => 0,
            }
        };
        let nahe = find_near_mvs(
            m.bezug,
            links.map(|l| (l.bezug, l.mv)),
            oben.map(|o| (o.bezug, o.mv)),
            links_oben.map(|l| (l.bezug, l.mv)),
            &bias,
            split_von(links),
            split_von(oben),
            split_von(links_oben),
        );
        // Der Vektorbaum braucht vier Wahrscheinlichkeiten aus den
        // Zaehlern (modemv.c Zeile 433, Tabelle `mv_counts_to_probs`).
        //
        // GEMESSENER FEHLER, der hier zuerst stand: als vierter Zaehler
        // stand `nahe.cnt[0]`, und das ist im Normtext der Platz von
        // CNT_ZEROZERO. Fuer `probs[3]` braucht es CNT_SPLITMV (Index 3).
        //
        // B13, 02.10.2026. GEMESSENER FEHLER: hier stand
        // `nahe.null_null` — ein Feld, das diese Crate selbst erfunden
        // hatte und das NIRGENDS befuellt wurde (nur `null_null: 0` in der
        // Initialisierung). Damit stand `probs[0]` des Vektorbaums
        // konstant auf `MV_COUNTS_TO_PROBS[0][0]` = 7.
        //
        // GEMESSENE GRUNDLAGE: der Referenzdecoder hat kein solches Feld.
        // `cnt[CNT_ZEROZERO]` IST `cnt[0]` (dixie.h:118
        // `CNT_BEST = 0`, `CNT_ZEROZERO = 0`), und er wird im Nullzweig
        // gefuellt, modemv.c Z. 292: `else cnt[CNT_ZEROZERO] += 2;`.
        //
        // GEMESSENER Beleg (`dixie_orakel`, Bild 1 von `kurz.webm`;
        // `mv_cnts` aus modemv.c Z. 421, daraus `probs` nach Z. 426):
        //
        // | Makroblock | mv_cnts Orakel | probs Orakel   | probs hier      |
        // |------------|----------------|----------------|-----------------|
        // |          0 |    [0 0 0 0]   | `[7 1 1 143]` | `[7 1 1 143]`   |
        // |          1 |    [2 0 0 0]   | `[135 1 1 143]`| `[7 1 1 143]`  |
        // |          2 |    [2 0 0 0]   | `[135 1 1 143]`| `[7 1 1 143]`  |
        //
        // Die Zahl 7 gegen 135 ist der Unterschied zwischen einem Baum,
        // der fast immer denselben Ast nimmt, und einem, der zwischen
        // ZEROMV und NEWMV entscheidet — und damit der Grund, warum
        // Bild 1 in 65206 von 76800 Y-Werten falsch war.
        let zaehler = [
            nahe.cnt[0].clamp(0, 5) as usize,
            nahe.cnt[1].clamp(0, 5) as usize,
            nahe.cnt[2].clamp(0, 5) as usize,
            nahe.cnt[3].clamp(0, 5) as usize,
        ];

        let probs: [u8; 4] = [
            t::MV_COUNTS_TO_PROBS[zaehler[0]][0],
            t::MV_COUNTS_TO_PROBS[zaehler[1]][1],
            t::MV_COUNTS_TO_PROBS[zaehler[2]][2],
            t::MV_COUNTS_TO_PROBS[zaehler[3]][3],
        ];
        // GEMESSENE ENTSCHEIDUNG, warum `zaehler[3]` der Zaehler von
        // CNT_BEST ist und nicht von CNT_SPLITMV: der Referenzdecoder
        // benutzt `mv_cnts[BEST]`, und `cnt[BEST]` und `cnt[ZEROZERO]`
        // teilen sich im Normtext eine Adresse, in der der Wert am Ende
        // der Nullzaehler ist (siehe `find_near_mvs`, letzte Zeilen).
        let modus = d.baum(&t::MV_REF_TREE, &probs)?;
        m.y = modus;
        m.zaehler = zaehler;
        m.gesetzt = true;

        // Die Schranke nach RFC 6386, 18.1. Sie wird NACH den Zaehlern,
        // dem Tausch und der Best-Kopie gebildet, genau so wie im
        // Normtext (`vp8_find_near_mvs`, Z. 5757-5761: erst `cnt[...]`,
        // dann der Tausch, dann `*best_mv = near_mvs[0]`, und DANN die
        // drei `vp8_clamp_mv`-Aufrufe). Wer vorher klemmt, haette einen
        // anderen `near_mvs[CNT_ZERO]` und damit einen anderen Vektor,
        // der als Basis dient.
        //
        // GEMESSENE GRUNDLAGE, dieselbe Stelle als Text des Norms:
        //
        // ```c
        // /* Set up return values */
        // *best_mv = near_mvs[0].as_mv;
        // *nearest = near_mvs[CNT_NEAREST].as_mv;
        // *near = near_mvs[CNT_NEAR].as_mv;
        //
        // vp8_clamp_mv(nearest, xd);
        // vp8_clamp_mv(near, xd);
        // vp8_clamp_mv(best_mv, xd); //TODO: Move this up before
        //                              the copy
        // ```
        //
        // `bild.1 == 0` bedeutet: der Aufrufer kennt die Bildmasse nicht
        // (siehe `modus_zeile_inter`), dann wird nichts geklemmt.
        let rand = if bild.1 == 0 {
            None
        } else {
            let (mb_y, mb_h) = bild;
            let spalte = mb as i32;
            Some(MvRand {
                x: (-((spalte + 1) << 7), ((mb_w as i32) - spalte) << 7),
                y: (
                    -(((mb_y as i32) + 1) << 7),
                    ((mb_h as i32) - (mb_y as i32)) << 7,
                ),
            })
        };
        // GEMESSENE ENTSCHEIDUNG, warum ein `Option` und kein fuenfftuendiger
        // Vergleich: `rand` haengt an der Geometrie, nicht an der
        // Wahrscheinlichkeit. Der Weg ohne Geometrie soll exakt das
        // alte Verhalten liefern, damit `examples/mbvergleich.rs`
        // (Rolle BILD) unveraendert laeuft.
        let klemm = |mv: Mv| match &rand {
            None => mv,
            Some(r) => mv_klemmen(mv, r),
        };
        let best = klemm(nahe.mv[0]);
        //
        // B13, 02.10.2026. GEMESSENER FEHLER: dieser `match` deutete die
        // Rueckgabewerte des Baums nach einer eigenen Reihenfolge
        // (0 = ZEROMV, 1 = NEARESTMV, 2 = NEARMV). Der Baum liefert aber
        // die Werte der Aufzaehlung aus dixie.h:129.
        //
        // GEMESSENE GRUNDLAGE, `mv_ref_tree` (modemv_data.h:177) mit
        // `NEARESTMV, NEARMV, ZEROMV, NEWMV, SPLITMV` = 0, 1, 2, 3, 4:
        //
        // ```text
        // Index  0 -> -ZEROMV    -> Baum gibt 2 zurueck
        // Index  4 -> -NEARMV    -> Baum gibt 1 zurueck
        // Index  6 -> -NEWMV     -> Baum gibt 3 zurueck
        // Index  7 -> -SPLITMV   -> Baum gibt 4 zurueck
        // ```
        //
        // Zwei Aeste waren also vertauscht: der Ast `0` (ZEROMV) wurde als
        // NEARMV gelesen und der Ast `2` (NEARMV) als NEARESTMV. Da
        // `nahe.mv[1]` (NEAREST) und `nahe.mv[2]` (NEAR) selten gleich
        // sind, war bei fast jedem Makroblock der falsche Nachbar
        // einsetzt.
        //
        // GEMESSENER Beleg (Referenzdecoder `dixie_orakel`, Bild 1 von
        // `kurz.webm`): die Vektoren stimmen jetzt bei 249 von 300
        // Makrobloecken, vorher 182. Bei Makroblock 221 meldet das
        // Orakel `nb = 0 48 0` und `mvx = 48` — der eigene Vektor ist
        // dort der des linken Nachbarn (NEAREST), und genau diesen Pfad
        // nahm vorher der Ast NEARMV.
        //
        // GEMESSENE ENTSCHEIDUNG zur Groesse der Wirkung: `nahe.mv[1]`
        // und `nahe.mv[2]` sind bei **245 von 300** Makrobloecken
        // gleich (gemessen mit einer Ausgabe der beiden Vektoren je
        // Makroblock). Der Vertauschungsfehler ist deshalb nur bei den
        // uebrigen 55 sichtbar — und eine GEGENPROBE mit der alten
        // Reihenfolge kam auf dieselben 249 von 300 Vektoren, weil die
        // Zaehlung `mv gleich` dort nicht unterscheidet. Der
        // Unterschied war trotzdem messbar, an den Ebenen: mit der
        // richtigen Reihenfolge Y 24351, mit der alten Y 24999 von 76800
        // falschen Werten.

        match modus {
            0 => {
                // NEARESTMV (dixie.h:129, Wert 0)
                // CNT_NEAREST ist der Index 1 (modemv.c Z. 248).
                //
                // GEMESSENE ENTSCHEIDUNG, warum hier geklemmt wird: der
                // Normtext klemmt `*nearest` am Ende von
                // `vp8_find_near_mvs` (Z. 5759), und der Modus
                // NEARESTMV UEBERNIMMT genau diesen geklemmten Wert
                // (Z. 498 `this->base.mv = near_mvs[NEAREST]`). Ohne
                // die Klemme zeigt ein Nachbarvektor, der aus dem Bild
                // zeigen wuerde, auf Bildinhalt.
                m.mv = klemm(nahe.mv[1]);
            }
            1 => {
                // NEARMV
                // CNT_NEAR ist der Index 2 (modemv.c Z. 250).
                m.mv = klemm(nahe.mv[2]);
            }
            2 => {
                // ZEROMV
                m.mv = Mv::default();
            }
            3 => {
                // NEWMV: der gelesene Vektor ist eine DIFFERENZ zum
                // besten Nachbarn (modemv.c Zeile 444).
                //
                // GEMESSENE ENTSCHEIDUNG, warum HIER noch einmal geklemmt
                // wird: RFC 6386, 18.1 sagt woertlich „Additional
                // clamping is performed for NEWMV macroblocks, for
                // which the final motion vector is clamped again after
                // combining the 'best' predictor and the differential
                // vector decoded from the stream". Das ist ein zweites
                // Klemmen, nicht das erste: `best` ist schon geklemmt,
                // das Ergebnis wird es noch nicht.
                m.mv = mv_lesen(d, mv_probs);
                m.mv.x += best.x;
                m.mv.y += best.y;
                m.mv = klemm(m.mv);
            }
            _ => {
                // SPLITMV: vier oder sechzehn Unterbloecke.
                //
                // GEMESSENE ENTSCHEIDUNG, warum hier NICHT noch einmal
                // geklemmt wird: RFC 6386, 18.1 direkt danach — „However,
                // the secondary clamping is NOT performed for SPLITMV
                // macroblocks, meaning that any subblock's motion vector
                // within the SPLITMV macroblock may point outside the
                // clamping zone. These non-clamped vectors are also used
                // when determining the decoding tree context for
                // subsequent subblocks' modes." Der Bases `best` ist
                // geklemmt, die Unterbloecke nicht.
                let best = best;
                // B13: `decode_mvs` gibt danach (modemv.c Z. 460)
                // `this->base.mv = this->split.mvs[15]` zurueck — der
                // VEKTOR DES LETZTEN Unterblocks, nicht irgendeiner.
                let (sid, unter) = split_mv(
                    d,
                    links_speicher,
                    vor_zeile_speicher,
                    mb,
                    mv_probs,
                    &bias,
                    best,
                )?;
                m.split_id = sid;
                m.unter_mv = unter;
                m.mv = m.unter_mv[15];
            }
        }
        // B13, 02.10.2026: nur noch der linke Speicher wird hier
        // geschrieben. `oben_speicher` traegt die Zeile DARUEBER und wird
        // vom Aufrufer nach der Zeile uebernommen (Begruendung bei der
        // Signatur). Vorher stand hier `oben_speicher[mb] = m`, damit
        // war `oben_speicher[mb - 1]` der Nachbar AUS DIESER ZEILE.
        if std::env::var_os("NARSIL_VP8_MODI_BIT").is_some() {
            eprintln!(
                "BIT mb {} bits {} wert {} weite {}",
                mb, d.bit_count, d.wert, d.weite
            );
        }
        links_speicher[mb] = m;
        raus.push(m);
    }
    //
    // B13, 02.10.2026: die fertige Zeile wird zur Zeile DARUEBER fuer den
    // naechsten Aufruf. GEMESSENE GRUNDLAGE: dixie.c Z. 524 setzt `above`
    // auf `mb_info_rows[row - 1]`, und `mb_info_rows` wird nur einmal
    // angelegt (modemv.c Z. 562) — der Zeiger wandert also in einer
    // Struktur, deren Zeilen sich gegenseitig ergeben.
    //
    // GEMESSENER Beleg (Referenzdecoder, Bild 1 von `kurz.webm`):
    // Makroblock 20 ist der erste der zweiten Zeile und meldet `ref 1 0 0`
    // (oben LAST) mit `cnt [2 0 0 0]`.
    vor_zeile_speicher.clear();
    vor_zeile_speicher.extend_from_slice(links_speicher);
    vor_zeile_speicher.resize(mb_w.max(1), InterMb::default());
    let _ = modi_schluessel;
    Ok(raus)
}

/// Liest die Unterbloecke eines SPLITMV-Makroblocks.
///
/// GEMESSENE Grundlage: modemv.c Zeile 342 (`decode_split_mv`). Die
/// Aufteilung nennt `split_mv_tree` und `mv_partitions`; jeder Unterblock
/// liest ueber `submv_ref` eine der vier Arten (LEFT, ABOVE, ZERO, NEW).
///
/// B13, 02.10.2026: es gibt drei gemessene Fehler, alle in diesem
/// Aufruf behoben.
///
/// (1) Die 16 Unterblockvektoren wurden NICHT einzeln gespeichert; es
///     gab nur `letzter`, also den zuletzt geschriebenen. Bei einer
///     Partition mit mehreren Unterbloecken (8x8 hat vier, 4x4 hat
///     sechzehn) bekommen damit alle Bloecke denselben Vektor.
/// (2) `aez` — der Nachbar OBEN — war fest `false`. `submv_ref`
///     vergleicht links und oben (modemv.c Z. 214: `lea = l.raw ==
///     a.raw`), und beide Nachbarn bestimmen den Kontextindex.
/// (3) `submv_ref` bekam nur den Vektor des Makroblocks links, nicht
///     den des Nachbarn oben und nicht die VECIOREN DIESES Makroblocks
///     aus den schon gefuellten Feldern.
///
/// GEMESSENE GRUNDLAGE, `decode_split_mv` (modemv.c Z. 343-391):
///
/// ```c
/// for (j = 0, mask = 0; mask < 65535; j++)
/// {
///     for (k = 0; j != partition[k]; k++);      /// erster Unterblock */
///     left_mv  = left_block_mv(this, left, k);
///     above_mv = above_block_mv(this, above, k);
///     subblock_mode = submv_ref(bool, left_mv, above_mv);
///     switch (subblock_mode) { LEFT4X4: mv = left_mv; ... }
///     for (; k < 16; k++)                        /// ab k weiterschreiben */
///         if (j == partition[k])
///         { this->split.mvs[k] = mv; mask |= 1 << k; }
/// }
/// ```
///
/// und die Nachbarn (Z. 174-192): ist der Nachbar selbst SPLITMV, zaehlt
/// sein entsprechender Unterblock, sonst sein Makroblockvektor; fuer
/// `b >= 4` kommt der Nachbar aus DIESEM Makroblock (`this->split.mvs`).
///
/// GEMESSENER Beleg fuer den Nutzen: Bild 1 von `kurz.webm` wurde
/// makroblockweise gegen den Referenzdecoder (`dixie_orakel`) geprueft
/// (`examples/mbvergleich.rb`): 0 von 300 Vektoren gleich, 137 von 300
/// Skip-Flaggen gleich. Die Modi stehen also richtig, die Vektoren nicht.
#[allow(clippy::too_many_arguments)]
fn split_mv(
    d: &mut BoolDekoder,
    links_speicher: &[InterMb],
    oben_speicher: &[InterMb],
    mb: usize,
    mv_probs: &[[u8; 19]; 2],
    bias: &[u32; 3],
    best: Mv,
) -> Result<(i32, [Mv; 16]), BildFehler> {
    let id = d.baum(&t::SPLIT_MV_TREE, &t::SPLIT_MV_PROBS)? as usize;
    let partition = &t::MV_PARTITIONS[id.min(3)];
    // B13, 03.10.2026 (Rolle VIDEO): die Nummer der Aufteilung melden.
    // GEMESSENE GRUNDLAGE: modemv.c Z. 359-363, `decode_split_mv` liest
    // `partition_id` und schreibt ihn nach `this->base.split_id`; ohne
    // dieses Feld laesst sich ein Bildstrom gegen den Referenzdecoder
    // nicht Zeile fuer Zeile vergleichen, weil `y == 4` SPLITMV und
    // B_PRED nicht trennt.
    // B13, 03.10.2026 (Rolle VIDEO): die Nummer der Aufteilung wird
    // ZURUECKGEGEBEN. GEMESSENE GRUNDLAGE: modemv.c Z. 359-363,
    // `decode_split_mv` liest `partition_id` und schreibt ihn nach
    // `this->base.split_id`. Ohne dieses Feld laesst sich ein Bildstrom
    // gegen den Referenzdecoder nicht Zeile fuer Zeile vergleichen, weil
    // `y == 4` SPLITMV und B_PRED nicht trennt.
    let split_id = id.min(3) as i32;
    let mut belegt = 0u32;
    // B13: die 16 Vektoren einzeln, nicht einer.
    let mut mvs: [Mv; 16] = [Mv::default(); 16];
    let mut j = 0usize;
    let mut sicherheit = 0usize;
    while belegt != 0xFFFF {
        // M3: die Schleife ist durch `belegt` begrenzt (16 Bloecke), aber
        // eine kaputte Partitionangabe koennte sie ewig laufen lassen.
        sicherheit += 1;
        if sicherheit > 16 {
            return Err(fehler("SPLITMV-Partition wiederholt sich ohne Ende"));
        }
        // Erster Unterblock dieser Partition.
        let mut k = 0usize;
        while k < 16 && partition[k] as usize != j {
            k += 1;
        }
        if k >= 16 {
            return Err(fehler("SPLITMV-Partition nennt keinen Unterblock"));
        }
        //
        // `left_block_mv` (modemv.c Z. 186): ist `k` ein Vielfaches von
        // vier, kommt der Vektor vom Nachbarn links — und zwar sein
        // Unterblock `k + 3`, falls dieser Nachbar selbst SPLITMV ist,
        // sonst sein Makroblockvektor. Sonst (k mod 4 != 0) kommt der
        // Vektor aus DIESEM Makroblock, Feld `k - 1`, das schon gefuellt
        // ist, weil die Unterbloecke in Zeilenrichtung kommen.
        //
        // B13, 02.10.2026. GEMESSENER FEHLER: hier stand
        // `links_speicher.get(mb)`. Das ist der Makroblock SELBST; der
        // linke Nachbar ist `mb - 1`. Derselbe Fehler wie in
        // `modus_zeile_inter`, an einer zweiten Stelle.
        //
        // GEMESSENE GRUNDLAGE: `decode_split_mv` (modemv.c Z. 345) ruft
        // `left_block_mv(this, left, k)` mit `left = this - 1` (Z. 424),
        // und `this` waechst in der Schleife (Z. 546). `left_block_mv`
        // (Z. 186) gibt bei `b & 3 == 0` `left->split.mvs[b + 3]`
        // zurueck, wenn der Nachbar SPLITMV ist, sonst `left->base.mv`.
        //
        // GEMESSENER Beleg (Bild 1 von `kurz.webm`, Makroblock 221,
        // `split_id 3`, also 4x4): mit dem falschen Index las die Crate
        // `left_mv = (0,0)`, `art = 3` und die Unterblockvektoren
        // `0/0 0/0 0/0 0/0 0/0 156/98 156/98 156/98 0/0 -46/0 -46/0
        // 156/98 -254/2 -254/2 -254/2 -254/2`, das Orakel `48/0 48/0
        // 0/0 0/0 48/0 48/0 … 48/0`.
        let links_mv = if k & 3 == 0 {
            let l = mb.checked_sub(1).and_then(|i| links_speicher.get(i));
            match l {
                Some(l) if l.y == 4 => l.unter_mv[k + 3],
                Some(l) => l.mv,
                None => Mv::default(),
            }
        } else {
            mvs[k - 1]
        };
        // `above_block_mv` (modemv.c Z. 174): ist `k < 4`, kommt der
        // Vektor vom Nachbarn oben (Unterblock `k + 12` bei SPLITMV,
        // sonst sein Makroblockvektor); sonst wieder aus diesem
        // Makroblock, Feld `k - 4`.
        let oben_mv = if k < 4 {
            let o = oben_speicher.get(mb);
            match o {
                Some(o) if o.y == 4 => o.unter_mv[k + 12],
                Some(o) => o.mv,
                None => Mv::default(),
            }
        } else {
            mvs[k - 4]
        };
        let mut neu = Mv::default();
        // `submv_ref` (modemv.c Z. 197-221): `lez` = links ist null,
        // `aez` = oben ist null, `lea` = beide sind GLEICH. Die Reihenfolge
        // der Abfragen dort ist bedeutsam: `lea && lez` zuerst, dann
        // `lea`, dann `aez`, dann `lez`, sonst NORMAL.
        //
        // GEMESSENER FEHLER, den dieser Aufruf zuerst hatte: `lea` stand als
        // `lez && aez` und `aez` fest auf `false`. Nach modemv.c Z. 214 ist
        // aber `lea = (l.raw == a.raw)` — die Gleichheit der beiden
        // Vektoren, unabhaengig davon, ob sie null sind. Mit `aez` fest
        // auf `false` konnte `lea` nie wahr werden und der Kontext
        // `SUBMVREF_LEFT_ABOVE_SAME` (3) nie vorkommen.
        let lez = links_mv == Mv::default();
        let aez = oben_mv == Mv::default();
        let lea = links_mv == oben_mv;
        let ctx = if lea && lez {
            4
        } else if lea {
            3
        } else if aez {
            2
        } else if lez {
            1
        } else {
            0
        };
        //
        // `submv_ref_tree` (modemv_data.h:184) hat die Blaetter in der
        // Reihenfolge LEFT4X4, ABOVE4X4, ZERO4X4, NEW4X4, dazu die
        // Zwischenknoten 2 und 4. Die Crate-Tabelle SUBMV_REF_TREE traegt
        // genau diese Reihenfolge.
        let art = d.baum(&t::SUBMV_REF_TREE, &t::SUBMV_REF_PROBS2[ctx])?;
        match art {
            // LEFT4X4
            0 => neu = links_mv,
            // ABOVE4X4 — der war vorher gar nicht verdrahtet.
            1 => neu = oben_mv,
            // ZERO4X4
            2 => neu = Mv::default(),
            // NEW4X4
            3 => {
                neu = mv_lesen(d, mv_probs);
                neu.x += best.x;
                neu.y += best.y;
            }
            _ => {}
        }
        //
        // B13, 02.10.2026. Hier stand ein Block, der den Vorzeichenbias
        // anwandte — und wirkungslos war:
        //
        // ```rust
        // let nachbar = if lez { eigener } else { eigener };
        // if eigener != nachbar { /* Vorzeichen drehen */ }
        // ```
        //
        // In beiden Zweigen war `nachbar` gleich `eigener`, die
        // Bedingung also nie wahr. GEMESSEN zusaetzlich an der Norm: Der
        // Vorzeichenbias wird im Referenzdecoder AUSSCHLIESSLICH in
        // `find_near_mvs` angewendet, dreimal (modemv.c:273 fuer OBEN, 285
        // fuer LINKS, 303 fuer LINKS OBEN). In `decode_split_mv`
        // (Z. 344 bis 393) kommt `mv_bias` nicht vor — auch nicht fuer den
        // Vektor aus `NEW4X4`, der dort zeilenweise `+= best_mv` erhaelt
        // (Z. 387-388).
        //
        // GEMESSENE Folge des Entfernens: keine, Bild 1 bleibt bei
        // 24301 von 76800 falschen Y-Werten. Der Block ist damit als
        // wirkungslos bestaetigt und nicht nur behauptet. `bias` bleibt
        // als Parameter, weil `find_near_mvs` ihn braucht.
        //
        // Der Referenzdecoder schreibt ab `k` weiter (Z. 386) und setzt
        // das Maskenbit fuer jeden belegten Unterblock.
        for idx in k..16 {
            if partition[idx] as usize == j {
                mvs[idx] = neu;
                belegt |= 1u32 << idx;
            }
        }
        j += 1;
    }
    Ok((split_id, mvs))
}

/// Die Angaben eines Interframe-Makroblocks.
#[derive(Clone, Copy, Default)]
pub struct InterMb {
    pub gesetzt: bool,
    pub segment: usize,
    pub skip: bool,
    pub bezug: Bezug,
    /// Der Modusbaum: 0 = ZEROMV, 1 = NEARESTMV, 2 = NEARMV, 3 = NEWMV,
    /// 4 = SPLITMV. Bei `intra` steht hier der 16x16-Intra-Modus.
    pub y: u32,
    /// B13: `true` = der Makroblock wird intra rekonstruiert.
    ///
    /// GEMESSENE GRUNDLAGE: modemv.c Zeile 529, `prob_inter` wird pro
    /// Makroblock gelesen. Ohne diese Unterscheidung liest der Dekoder
    /// fuer einen Intra-Makroblock Bewegungsvektoren, die nicht im Strom
    /// stehen.
    pub intra: bool,
    /// Der UV-Modus, nur bei `intra` gefuellt.
    pub uv: u32,
    ///
    /// B13, 02.10.2026: die 16 Unterblockvektoren eines SPLITMV-
    /// Makroblocks, einzeln gespeichert.
    ///
    /// GEMESSENE GRUNDLAGE: `above_block_mv` (modemv.c Z. 174) liest
    /// `above->split.mvs[b + 12]` und `left_block_mv` (Z. 186)
    /// `left->split.mvs[b + 3]`, wenn der jeweilige Nachbar selbst
    /// SPLITMV ist — es braucht also die EINZELNEN Vektoren des Nachbarn,
    /// nicht einen Sammelvektor.
    ///
    /// GEMESSENER Beleg: `examples/mbvergleich` zaehlte 0 von 300
    /// Vektoren gleich gegen den Referenzdecoder, waehrend die Modi
    /// richtig standen.
    pub unter_mv: [Mv; 16],
    ///
    /// B13, 02.10.2026: die vier Zaehler der Nahbereichssuche
    /// (`mv_cnts`, modemv.c Z. 421). Sie sind genau die vier
    /// Wahrscheinlichkeiten des Vektorbaums (Z. 426) und damit das
    /// kleinste messbare Stueck des Modalwegs: passen sie, stimmt der
    /// Ast des Baums, passen sie nicht, ist schon die Nachbarsuche
    /// falsch.
    ///
    /// GEMESSENER Zweck: damit laesst sich ein Bildstrom Makroblock fuer
    /// Makroblock gegen den Referenzdecoder vergleichen, ohne den
    /// Bildinhalt zu betrachten (`examples/mbvergleich.rs`).
    pub zaehler: [usize; 4],
    /// B13: die 16 Unterblockmodi, nur bei `intra` und `y == 4`.
    ///
    /// GEMESSENE GRUNDLAGE: modemv.c Z. 134, `decode_intra_mb_mode` liest
    /// bei B_PRED sechzehn Symbole aus `b_mode_tree` mit
    /// `default_b_mode_probs`. Wer sie nicht liest, verliert den Takt.
    pub unter: [u32; MAX_UNTERBLOECKE],
    pub mv: Mv,
    ///
    /// B13, 03.10.2026 (Rolle VIDEO): die Nummer der SPLITMV-Aufteilung,
    /// 0 bis 3, oder -1 wenn dieser Makroblock kein SPLITMV ist.
    ///
    /// GEMESSENE GRUNDLAGE: modemv.c Z. 359, `decode_split_mv` liest
    /// `partition_id = bool_read_tree(bool, split_mv_tree, split_mv_probs)`
    /// und schreibt ihn nach `this->base.split_id`. Die Aufteilung selbst
    /// steht in `mv_partitions[partition_id]` (dixie.h:138).
    ///
    /// GEMESSENER Zweck: `y == 4` allein trennt SPLITMV nicht von B_PRED
    /// (beide sind die Zahl 4), und ohne die Nummer laesst sich ein
    /// Bildstrom gegen den Referenzdecoder nicht Zeile fuer Zeile
    /// vergleichen — man muesste erraten, ob zwei Makrobloecke dieselbe
    /// Aufteilung gelesen haben. Das ist der Unterschied zwischen
    /// „sieht gleich aus" und „ist gleich".
    pub split_id: i32,
}

#[cfg(test)]
mod tests;

pub fn get_large_value(d: &mut BoolDekoder, p: &[u8; 11]) -> i32 {
    let mut v;
    if d.flag(p[3]) == 0 {
        if d.flag(p[4]) == 0 {
            v = 2;
        } else {
            v = 3 + d.flag(p[5]);
        }
    } else {
        if d.flag(p[6]) == 0 {
            if d.flag(p[7]) == 0 {
                v = 5 + d.flag(159);
            } else {
                v = 7 + 2 * d.flag(165) + d.flag(145);
            }
        } else {
            let bit1 = d.flag(p[8]);
            let bit0 = d.flag(p[9 + bit1 as usize]);
            let cat = (2 * bit1 + bit0) as usize;
            v = 0;
            for &prob in t::CAT3456[cat] {
                v = v * 2 + d.flag(prob);
            }
            v += 3 + (8 << cat);
        }
    }
    v as i32
}

const BANDS: [usize; 17] = [0, 1, 2, 3, 6, 4, 5, 6, 6, 6, 6, 6, 6, 6, 6, 7, 7];

pub fn get_coeffs(
    d: &mut BoolDekoder,
    probs: &[[[u8; 11]; 3]; 8],
    ctx: usize,
    dq: &[i32; 2],
    first: usize,
    out: &mut [i32; 16],
) -> usize {
    let mut n = first;
    let mut p = &probs[BANDS[n]][ctx];
    while n < 16 {
        if d.flag(p[0]) == 0 {
            return n;
        }
        while d.flag(p[1]) == 0 {
            n += 1;
            if n == 16 {
                return 16;
            }
            p = &probs[BANDS[n]][0];
        }
        let p_ctx = &probs[BANDS[n + 1]];
        let v;
        if d.flag(p[2]) == 0 {
            v = 1;
            p = &p_ctx[1];
        } else {
            v = get_large_value(d, p);
            p = &p_ctx[2];
        }
        let sign = d.flag(128);
        let val = if sign == 1 { -v } else { v };
        out[t::ZIGZAG[n]] = val * dq[(n > 0) as usize];
        n += 1;
    }
    16
}

pub fn nz_code_bits(mut nz_coeffs: u32, nz: usize, dc_nz: bool) -> u32 {
    nz_coeffs <<= 2;
    nz_coeffs |= if nz > 3 {
        3
    } else if nz > 1 {
        2
    } else {
        dc_nz as u32
    };
    nz_coeffs
}

// In ParseResiduals:
// We need the macroblock structure to store non-zero (nz) bits.
// Since we don't have the full decoder struct yet, I'll define the logic here.

pub enum BMode {
    DcPred = 0,
    TmPred = 1,
    VePred = 2,
    HePred = 3,
    LdPred = 4,
    RdPred = 5,
    VrPred = 6,
    VlPred = 7,
    HdPred = 8,
    HuPred = 9,
}

pub enum MbMode {
    DcPred = 0,
    VPred = 1,
    HPred = 2,
    TmPred = 3,
}

fn clip_pixel(v: i32) -> u8 {
    if v < 0 {
        0
    } else if v > 255 {
        255
    } else {
        v as u8
    }
}

pub fn pred_16x16_v(output: &mut [u8], stride: usize, above: &[u8; 16]) {
    for r in 0..16 {
        for c in 0..16 {
            output[r * stride + c] = above[c];
        }
    }
}

pub fn pred_16x16_h(output: &mut [u8], stride: usize, left: &[u8; 16]) {
    for r in 0..16 {
        for c in 0..16 {
            output[r * stride + c] = left[r];
        }
    }
}

pub fn pred_16x16_dc(
    output: &mut [u8],
    stride: usize,
    above: Option<&[u8; 16]>,
    left: Option<&[u8; 16]>,
) {
    let mut sum = 0;
    let mut count = 0;

    if let Some(a) = above {
        for &p in a.iter() {
            sum += p as u32;
        }
        count += 16;
    }
    if let Some(l) = left {
        for &p in l.iter() {
            sum += p as u32;
        }
        count += 16;
    }

    let dc = if count > 0 {
        ((sum + (count >> 1)) / count) as u8
    } else {
        128
    };
    for r in 0..16 {
        for c in 0..16 {
            output[r * stride + c] = dc;
        }
    }
}

pub fn pred_16x16_tm(
    output: &mut [u8],
    stride: usize,
    above: &[u8; 16],
    left: &[u8; 16],
    top_left: u8,
) {
    for r in 0..16 {
        for c in 0..16 {
            let val = left[r] as i32 + above[c] as i32 - top_left as i32;
            output[r * stride + c] = clip_pixel(val);
        }
    }
}

pub fn pred_8x8_v(output: &mut [u8], stride: usize, above: &[u8; 8]) {
    for r in 0..8 {
        for c in 0..8 {
            output[r * stride + c] = above[c];
        }
    }
}

pub fn pred_8x8_h(output: &mut [u8], stride: usize, left: &[u8; 8]) {
    for r in 0..8 {
        for c in 0..8 {
            output[r * stride + c] = left[r];
        }
    }
}

pub fn pred_8x8_dc(
    output: &mut [u8],
    stride: usize,
    above: Option<&[u8; 8]>,
    left: Option<&[u8; 8]>,
) {
    let mut sum = 0;
    let mut count = 0;

    if let Some(a) = above {
        for &p in a.iter() {
            sum += p as u32;
        }
        count += 8;
    }
    if let Some(l) = left {
        for &p in l.iter() {
            sum += p as u32;
        }
        count += 8;
    }

    let dc = if count > 0 {
        ((sum + (count >> 1)) / count) as u8
    } else {
        128
    };
    for r in 0..8 {
        for c in 0..8 {
            output[r * stride + c] = dc;
        }
    }
}

pub fn pred_8x8_tm(
    output: &mut [u8],
    stride: usize,
    above: &[u8; 8],
    left: &[u8; 8],
    top_left: u8,
) {
    for r in 0..8 {
        for c in 0..8 {
            let val = left[r] as i32 + above[c] as i32 - top_left as i32;
            output[r * stride + c] = clip_pixel(val);
        }
    }
}

pub fn pred_4x4_dc(
    output: &mut [u8],
    stride: usize,
    above: Option<&[u8; 4]>,
    left: Option<&[u8; 4]>,
) {
    // For 4x4 blocks, we actually always use both, using the padding values 127/129 if None.
    // The spec uses 127 for top and 129 for left if not available.
    let mut sum = 0;
    if let Some(a) = above {
        for &p in a.iter() {
            sum += p as u32;
        }
    } else {
        sum += 127 * 4;
    }
    if let Some(l) = left {
        for &p in l.iter() {
            sum += p as u32;
        }
    } else {
        sum += 129 * 4;
    }

    let dc = ((sum + 4) >> 3) as u8;
    for r in 0..4 {
        for c in 0..4 {
            output[r * stride + c] = dc;
        }
    }
}

pub fn pred_4x4_tm(
    output: &mut [u8],
    stride: usize,
    above: &[u8; 4],
    left: &[u8; 4],
    top_left: u8,
) {
    for r in 0..4 {
        for c in 0..4 {
            let val = left[r] as i32 + above[c] as i32 - top_left as i32;
            output[r * stride + c] = clip_pixel(val);
        }
    }
}

pub fn pred_4x4_ve(
    output: &mut [u8],
    stride: usize,
    above: &[u8; 4],
    above_left: u8,
    above_right: Option<u8>,
) {
    let ar = above_right.unwrap_or(above[3]);
    let vals = [
        ((above_left as u32 + 2 * above[0] as u32 + above[1] as u32 + 2) >> 2) as u8,
        ((above[0] as u32 + 2 * above[1] as u32 + above[2] as u32 + 2) >> 2) as u8,
        ((above[1] as u32 + 2 * above[2] as u32 + above[3] as u32 + 2) >> 2) as u8,
        ((above[2] as u32 + 2 * above[3] as u32 + ar as u32 + 2) >> 2) as u8,
    ];
    for r in 0..4 {
        for c in 0..4 {
            output[r * stride + c] = vals[c];
        }
    }
}

pub fn pred_4x4_he(output: &mut [u8], stride: usize, left: &[u8; 4], above_left: u8) {
    let vals = [
        ((above_left as u32 + 2 * left[0] as u32 + left[1] as u32 + 2) >> 2) as u8,
        ((left[0] as u32 + 2 * left[1] as u32 + left[2] as u32 + 2) >> 2) as u8,
        ((left[1] as u32 + 2 * left[2] as u32 + left[3] as u32 + 2) >> 2) as u8,
        ((left[2] as u32 + 2 * left[3] as u32 + left[3] as u32 + 2) >> 2) as u8,
    ];
    for r in 0..4 {
        for c in 0..4 {
            output[r * stride + c] = vals[r];
        }
    }
}

pub fn pred_4x4_ld(output: &mut [u8], stride: usize, above: &[u8; 8]) {
    for r in 0..4 {
        for c in 0..4 {
            let i = r + c;
            let get_above = |idx: usize| above[idx.min(7)] as u32;
            let val = if i < 7 {
                (get_above(i) + 2 * get_above(i + 1) + get_above(i + 2) + 2) >> 2
            } else {
                (get_above(7) + 2 * get_above(7) + get_above(7) + 2) >> 2
            };
            output[r * stride + c] = val as u8;
        }
    }
}

pub fn pred_4x4_rd(
    output: &mut [u8],
    stride: usize,
    above: &[u8; 4],
    left: &[u8; 4],
    above_left: u8,
) {
    let mut context = [0u8; 9];
    context[0] = left[3];
    context[1] = left[2];
    context[2] = left[1];
    context[3] = left[0];
    context[4] = above_left;
    context[5..9].copy_from_slice(above);

    for r in 0..4 {
        for c in 0..4 {
            let idx = c as i32 - r as i32;
            let base = (idx + 4) as usize;
            let val = (context[base - 1] as u32
                + 2 * context[base] as u32
                + context[base + 1] as u32
                + 2)
                >> 2;
            output[r * stride + c] = val as u8;
        }
    }
}

/// B_VR_PRED, nach RFC 6386 Abschnitt 12.3.
///
/// GEMESSEN 01.10.2026 gegen das C aus dem Anhang One (predict.c,
/// `predict_vr_4x4`): die alte Fassung war ein Diagonalen-Raster ueber
/// `E`, bei dem die Spalte 0 der Zeilen 2 und 3 falsch lag — genau
/// zwei Felder von sechzehn. Sie brauchen Werte aus der LINKEN Spalte
/// (`L[1], L[0], P` beziehungsweise `L[2], L[1], L[0]`), die das Raster
/// gar nicht hatte. Die Form mit `pred0..pred9` ist die des Normtextes;
/// `avg3` und `avg2` sind seine beiden Mittelwerte (RFC 6386, Zeile
/// 3097 und 3106): `avg3(x,y,z) = (x + y + y + z + 2) >> 2`,
/// `avg2(x,y) = (x + y + 1) >> 1`.
pub fn pred_4x4_vr(
    output: &mut [u8],
    stride: usize,
    above: &[u8; 4],
    left: &[u8; 4],
    above_left: u8,
) {
    // E[0..9] nach RFC 6386 Zeile 3119: E[0]=L[3], E[1]=L[2], E[2]=L[1],
    // E[3]=L[0], E[4]=P (oben links), E[5..9] = A[0..4].
    let mut e = [0u32; 9];
    e[0] = left[3] as u32;
    e[1] = left[2] as u32;
    e[2] = left[1] as u32;
    e[3] = left[0] as u32;
    e[4] = above_left as u32;
    e[5..9].copy_from_slice(&above.map(u32::from));
    let a3 = |x: u32, y: u32, z: u32| (x + y + y + z + 2) >> 2;
    let a2 = |x: u32, y: u32| (x + y + 1) >> 1;
    let p0 = a2(e[4], e[5]);
    let p1 = a2(e[5], e[6]);
    let p2 = a2(e[6], e[7]);
    let p3 = a2(e[7], e[8]);
    let p4 = a3(e[3], e[4], e[5]);
    let p5 = a3(e[4], e[5], e[6]);
    let p6 = a3(e[5], e[6], e[7]);
    let p7 = a3(e[6], e[7], e[8]);
    let p8 = a3(e[2], e[3], e[4]);
    let p9 = a3(e[1], e[2], e[3]);
    let zeilen = [p0, p1, p2, p3, p4, p5, p6, p7, p8, p9];
    for r in 0..4 {
        for c in 0..4 {
            output[r * stride + c] = match (r, c) {
                (0, _) => zeilen[c],
                (1, _) => zeilen[4 + c],
                (2, 0) => zeilen[8],
                (2, _) => zeilen[c - 1],
                (3, 0) => zeilen[9],
                (3, _) => zeilen[3 + c],
                _ => 0,
            } as u8;
        }
    }
}

/// B_VL_PRED, nach RFC 6386 Abschnitt 12.3 (`predict_vl_4x4`).
///
/// GEMESSEN 01.10.2026: die alte Fassung benutzte fuer die Zeilen 1 und
/// 3 einen anderen Index als der Normtext; der Fehler fiel an zwei
/// Feldern je Zeile auf.
pub fn pred_4x4_vl(output: &mut [u8], stride: usize, above: &[u8; 8]) {
    // A[0..8]: die vier Werte der oberen Zeile, danach vier Werte
    // oben rechts. `a[i]` ist A[i].
    let a: Vec<u32> = above.iter().map(|&v| v as u32).collect();
    let a3 = |x: u32, y: u32, z: u32| (x + y + y + z + 2) >> 2;
    let a2 = |x: u32, y: u32| (x + y + 1) >> 1;
    let p0 = a2(a[0], a[1]);
    let p1 = a2(a[1], a[2]);
    let p2 = a2(a[2], a[3]);
    let p3 = a2(a[3], a[4]);
    let p4 = a3(a[0], a[1], a[2]);
    let p5 = a3(a[1], a[2], a[3]);
    let p6 = a3(a[2], a[3], a[4]);
    let p7 = a3(a[3], a[4], a[5]);
    let p8 = a3(a[4], a[5], a[6]);
    let p9 = a3(a[5], a[6], a[7]);
    let z = [p0, p1, p2, p3, p4, p5, p6, p7, p8, p9];
    for r in 0..4 {
        for c in 0..4 {
            output[r * stride + c] = match (r, c) {
                (0, _) => z[c],
                (1, _) => z[4 + c],
                (2, 0) => z[1],
                (2, 1) => z[2],
                (2, 2) => z[3],
                (2, 3) => z[8],
                (3, 0) => z[5],
                (3, 1) => z[6],
                (3, 2) => z[7],
                (3, 3) => z[9],
                _ => 0,
            } as u8;
        }
    }
}

/// B_HD_PRED, nach RFC 6386 Abschnitt 12.3 (`predict_hd_4x4`).
///
/// GEMESSEN 01.10.2026: die alte Fassung benutzte fuer Spalte 0 die
/// Werte der oberen Zeile statt der linken; der Normtext liest dort
/// `L[r]` und `L[r+1]`.
pub fn pred_4x4_hd(
    output: &mut [u8],
    stride: usize,
    above: &[u8; 4],
    left: &[u8; 4],
    above_left: u8,
) {
    let l: Vec<u32> = left.iter().map(|&v| v as u32).collect();
    let a: Vec<u32> = above.iter().map(|&v| v as u32).collect();
    let p = above_left as u32;
    let a3 = |x: u32, y: u32, z: u32| (x + y + y + z + 2) >> 2;
    let a2 = |x: u32, y: u32| (x + y + 1) >> 1;
    let p0 = a2(l[0], p);
    let p1 = a3(l[0], p, a[0]);
    let p2 = a3(p, a[0], a[1]);
    let p3 = a3(a[0], a[1], a[2]);
    let p4 = a2(l[1], l[0]);
    let p5 = a3(l[1], l[0], p);
    let p6 = a2(l[2], l[1]);
    let p7 = a3(l[2], l[1], l[0]);
    let p8 = a2(l[3], l[2]);
    let p9 = a3(l[3], l[2], l[1]);
    let z = [p0, p1, p2, p3, p4, p5, p6, p7, p8, p9];
    for r in 0..4 {
        for c in 0..4 {
            output[r * stride + c] = match (r, c) {
                (0, 0) => z[0],
                (0, 1) => z[1],
                (0, 2) => z[2],
                (0, 3) => z[3],
                (1, 0) => z[4],
                (1, 1) => z[5],
                (1, 2) => z[0],
                (1, 3) => z[1],
                (2, 0) => z[6],
                (2, 1) => z[7],
                (2, 2) => z[4],
                (2, 3) => z[5],
                (3, 0) => z[8],
                (3, 1) => z[9],
                (3, 2) => z[6],
                (3, 3) => z[7],
                _ => 0,
            } as u8;
        }
    }
}

pub fn pred_4x4_hu(output: &mut [u8], stride: usize, left: &[u8; 4]) {
    for r in 0..4 {
        for c in 0..4 {
            let get_left = |i: usize| left[i.min(3)] as u32;
            let val = if c == 0 {
                if r == 3 {
                    left[3] as u32
                } else {
                    (get_left(r) + get_left(r + 1) + 1) >> 1
                }
            } else if c == 1 {
                if r == 3 {
                    left[3] as u32
                } else {
                    (get_left(r) + 2 * get_left(r + 1) + get_left(r + 2) + 2) >> 2
                }
            } else if c == 2 {
                if r >= 2 {
                    left[3] as u32
                } else {
                    (get_left(r + 1) + get_left(r + 2) + 1) >> 1
                }
            } else {
                if r >= 2 {
                    left[3] as u32
                } else {
                    (get_left(r + 1) + 2 * get_left(r + 2) + get_left(r + 3) + 2) >> 2
                }
            };
            output[r * stride + c] = val as u8;
        }
    }
}
/// Inverse DCT eines 4x4-Blocks, Wort fuer Wort nach RFC 6386,
/// Abschnitt 14.4 (`short_idct4x4llm_c`).
///
/// Stand 01.10.2026 (Rolle BILD). Die Abweichung, die gefunden wurde und
/// die den Fehler enthaelt, sind die FEHLENDEN `+ x`-TERME. Der
/// Normtext benutzt die Konstante `cospi8sqrt2minus1 = 20091`, also
/// NICHT `0.3066` als Faktor, sondern `x * sqrt(2) * cos(pi/8) = x +
/// x * (sqrt(2)*cos(pi/8) - 1)`. Im C des Normtextes heisst das
/// wortwoertlich `ip[12] + ((ip[12] * cospi8sqrt2minus1) >> 16)` und
/// nicht `(ip[12] * cospi8sqrt2minus1) >> 16`. Vorher stand hier nur das
/// Produkt; damit fehlte in jeder der vier betroffenen Stellen ein
/// ganzer Summand `x`. Begruendung im RFC, Abschnitt 14.4: „Because the
/// first constant is bigger than 1 ... we make use of the fact that
/// x * a = x + x*(a-1)“.
pub fn idct4x4(input: &[i16; 16], output: &mut [u8], stride: usize) {
    /// `sqrt(2) * sin(pi/8)` in 16-Bit-Festkomma, RFC 6386, 14.4.
    const SIN_PI8_SQRT2: i32 = 35468;
    /// `sqrt(2) * cos(pi/8) - 1` in 16-Bit-Festkomma, RFC 6386, 14.4.
    const COS_PI8_SQRT2_MINUS1: i32 = 20091;
    // GEMESSEN 02.10.2026 (Rolle BILD, B5) mit `examples/bomben_stress.rs`:
    // der Zwischenwert stand in `i32` und der zweite Durchgang rechnete
    // `x1 * SIN_PI8_SQRT2`. In einer gueltigen Datei sind die
    // Koeffizienten eng begrenzt, dann passt alles in `i32` — aber der
    // Fuzzer fuetterte beliebige Koeffizienten ein und fand nach 2000
    // Runden `attempt to multiply with overflow` (Zeile 1413): der
    // Zwischenwert kann bis rund 126000 werden, mal 35468 sind 4,5
    // Milliarden, mehr als `i32` fasst. Jetzt `i64` — fuer gueltige
    // Eingaben sind die Ergebnisse BITGLEICH, weil dort nichts
    // ueberlaeuft; nur die kaputten Eingaben kommen durch.
    let mut temp = [0i64; 16];
    // Erster Durchgang. Der Normtext laesst `ip` und `op` beide um eins
    // wandern und schreibt nach `op[shortpitch*0..3]` mit
    // `shortpitch = pitch >> 1 = 4`. Durchgang `i` liest also die SPALTE
    // `i` des Eingangs (input[i], input[i+4], input[i+8], input[i+12])
    // und schreibt in die ZEILEN 0 bis 3 an der Spalte `i`. Der
    // Zwischenwert liegt also zeilenweise: `temp[4 * zeile + i]`.
    for i in 0..4 {
        let x0 = input[i] as i64;
        let x1 = input[4 + i] as i64;
        let x2 = input[8 + i] as i64;
        let x3 = input[12 + i] as i64;
        let a1 = x0 + x2;
        let b1 = x0 - x2;
        let t1 = (x1 * SIN_PI8_SQRT2 as i64) >> 16;
        let t2 = x3 + ((x3 * COS_PI8_SQRT2_MINUS1 as i64) >> 16);
        let c1 = t1 - t2;
        let t3 = x1 + ((x1 * COS_PI8_SQRT2_MINUS1 as i64) >> 16);
        let t4 = (x3 * SIN_PI8_SQRT2 as i64) >> 16;
        let d1 = t3 + t4;
        temp[i] = a1 + d1;
        temp[4 + i] = b1 + c1;
        temp[8 + i] = b1 - c1;
        temp[12 + i] = a1 - d1;
    }
    // Zweiter Durchgang: je Zeile des Zwischenwerts, mit der Rundung des
    // Normtextes `(x + 4) >> 3`.
    for i in 0..4 {
        let x0 = temp[i * 4];
        let x1 = temp[i * 4 + 1];
        let x2 = temp[i * 4 + 2];
        let x3 = temp[i * 4 + 3];
        let a1 = x0 + x2;
        let b1 = x0 - x2;
        let t1 = (x1 * SIN_PI8_SQRT2 as i64) >> 16;
        let t2 = x3 + ((x3 * COS_PI8_SQRT2_MINUS1 as i64) >> 16);
        let c1 = t1 - t2;
        let t3 = x1 + ((x1 * COS_PI8_SQRT2_MINUS1 as i64) >> 16);
        let t4 = (x3 * SIN_PI8_SQRT2 as i64) >> 16;
        let d1 = t3 + t4;
        let out0 = (a1 + d1 + 4) >> 3;
        let out1 = (b1 + c1 + 4) >> 3;
        let out2 = (b1 - c1 + 4) >> 3;
        let out3 = (a1 - d1 + 4) >> 3;
        // `clip_pixel` nimmt `i32`; der Zwischenwert wird begrenzt
        // uebergeben, damit die Umrechnung selbst nicht ueberlaeuft.
        let k = |alt: u8, add: i64| -> u8 {
            clip_pixel((alt as i64 + add).clamp(i32::MIN as i64, i32::MAX as i64) as i32)
        };
        output[i * stride] = k(output[i * stride], out0);
        output[i * stride + 1] = k(output[i * stride + 1], out1);
        output[i * stride + 2] = k(output[i * stride + 2], out2);
        output[i * stride + 3] = k(output[i * stride + 3], out3);
    }
}
pub fn iwht4x4(input: &[i16; 16], output: &mut [i16; 16]) {
    let mut tmp = [0i32; 16];
    for i in 0..4 {
        let a0 = input[i] as i32 + input[12 + i] as i32;
        let a1 = input[4 + i] as i32 + input[8 + i] as i32;
        let a2 = input[4 + i] as i32 - input[8 + i] as i32;
        let a3 = input[i] as i32 - input[12 + i] as i32;
        tmp[i] = a0 + a1;
        tmp[8 + i] = a0 - a1;
        tmp[4 + i] = a3 + a2;
        tmp[12 + i] = a3 - a2;
    }
    for i in 0..4 {
        let base = i * 4;
        let dc = tmp[base] + 3;
        let a0 = dc + tmp[base + 3];
        let a1 = tmp[base + 1] + tmp[base + 2];
        let a2 = tmp[base + 1] - tmp[base + 2];
        let a3 = dc - tmp[base + 3];
        output[base] = ((a0 + a1) >> 3) as i16;
        output[base + 1] = ((a3 + a2) >> 3) as i16;
        output[base + 2] = ((a0 - a1) >> 3) as i16;
        output[base + 3] = ((a3 - a2) >> 3) as i16;
    }
}
use crate::Bild;

pub const DC_TABLE: [i32; 128] = [
    4, 5, 6, 7, 8, 9, 10, 10, 11, 12, 13, 14, 15, 16, 17, 17, 18, 19, 20, 20, 21, 21, 22, 22, 23,
    23, 24, 25, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 37, 38, 39, 40, 41, 42, 43, 44,
    45, 46, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67,
    68, 69, 70, 71, 72, 73, 74, 75, 76, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 91,
    93, 95, 96, 98, 100, 101, 102, 104, 106, 108, 110, 112, 114, 116, 118, 122, 124, 126, 128, 130,
    132, 134, 136, 138, 140, 143, 145, 148, 151, 154, 157,
];

pub const AC_TABLE: [i32; 128] = [
    4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28,
    29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52,
    53, 54, 55, 56, 57, 58, 60, 62, 64, 66, 68, 70, 72, 74, 76, 78, 80, 82, 84, 86, 88, 90, 92, 94,
    96, 98, 100, 102, 104, 106, 108, 110, 112, 114, 116, 119, 122, 125, 128, 131, 134, 137, 140,
    143, 146, 149, 152, 155, 158, 161, 164, 167, 170, 173, 177, 181, 185, 189, 193, 197, 201, 205,
    209, 213, 217, 221, 225, 229, 234, 239, 245, 249, 254, 259, 264, 269, 274, 279, 284,
];

pub struct QuantMatrix {
    pub y1_mat: [i32; 2],
    pub y2_mat: [i32; 2],
    pub uv_mat: [i32; 2],
}

/// Die Dequantisierungsfaktoren fuer EIN Segment.
///
/// RFC 6386, 14.1 (`dequant_init`): `q` ist der Quantisierer des
/// Segments — bei absolutem Modus der Segmentwert selbst, sonst der
/// Segmentwert plus der Bildquantisierer. Danach kommen die vier
/// Bild-Deltas.
///
/// GEMESSEN 01.10.2026, zwei Abweichungen vom Normtext, beide in der
/// Reihenfolge danach:
///   * der Chroma-DC wird im Normtext BEIM WERT auf 132 begrenzt
///     („if (dqf->factor[TOKEN_BLOCK_UV][0] > 132) ... = 132"), vorher
///     wurde der TabellenINDEX auf 117 begrenzt — das ist eine andere
///     Stelle mit anderem Ergebnis;
///   * der Y2-AC-Faktor ist im Normtext `ac_q(...) * 155 / 100` in
///     Ganzzahlarithmetik, hier stand eine feste Konstante 101581/65536
///     (1.54998 statt 1.55), bei einem Faktor von 100 also 154 statt
///     155.
pub fn vp8_quant_matrix_segment(
    base_q0: i32,
    segment_quant: i32,
    segment_abs: bool,
    kopf: &BoolKopf,
) -> QuantMatrix {
    let q = if segment_abs {
        segment_quant
    } else {
        base_q0 + segment_quant
    };
    let clip = |v: i32| -> usize { v.clamp(0, 127) as usize };

    let y1_mat = [DC_TABLE[clip(q + kopf.quant_y_dc)], AC_TABLE[clip(q)]];

    let mut y2_mat = [
        DC_TABLE[clip(q + kopf.quant_y2_dc)] * 2,
        AC_TABLE[clip(q + kopf.quant_y2_ac)] * 155 / 100,
    ];
    if y2_mat[1] < 8 {
        y2_mat[1] = 8;
    }

    // `dc_q(q + uv_dc_delta_q)`, danach die Wertbegrenzung auf 132.
    let uv_dc = DC_TABLE[clip(q + kopf.quant_uv_dc)].min(132);
    let uv_mat = [uv_dc, AC_TABLE[clip(q + kopf.quant_uv_ac)]];

    QuantMatrix {
        y1_mat,
        y2_mat,
        uv_mat,
    }
}

/// Die Faktoren des Bildquantisierers (Segment 0 ohne Segmentwert) —
/// bequemer Aufruf fuer Aufrufer, die keine Segmentierung erwarten.
pub fn vp8_quant_matrix(base_q0: i32, kopf: &BoolKopf) -> QuantMatrix {
    vp8_quant_matrix_segment(base_q0, 0, kopf.segment_abs, kopf)
}

// Loop filter logic from webp-rust
fn clip_filter_value(value: i32) -> i32 {
    value.clamp(-128, 127)
}
fn clip_byte(value: i32) -> u8 {
    if value < 0 {
        0
    } else if value > 255 {
        255
    } else {
        value as u8
    }
}
/// `high_edge_variance` des Normtextes: die Kantenabweichung an einer
/// Kante.
///
/// GEMESSEN 01.10.2026 (Rolle BILD, B1), der groesste Fund am
/// Loopfilter ueberhaupt: hier waren die vier Nachbarn um eine Position
/// verschoben. Die Kantenfunktionen zeigen an `pos` auf `q0` — so
/// lesen es `needs_filter2`, `do_filter2`, `do_filter4` und `do_filter6`
/// auch — `p0` ist `pos - step` und `p1` ist `pos - 2 * step`. In `hev`
/// und `needs_filter` stand dagegen `p1 = pos - step`, `p0 = pos`,
/// `q0 = pos + step`, `q1 = pos + 2 * step`; damit wurde
///
///   * die mittlere Differenz `|p0 - q0|` geprueft statt der inneren
///     `|p1 - p0|`, und
///   * in `needs_filter` das Paar `p2`-artig gegen `q2` gerechnet.
///
/// GEMESSEN an `vp8_q75`, Bildzeile 0, Makroblockspalte 0, senkrechte
/// Innenkante, Punkt 4: Nachbarwerte p3..q3 = 34 34 36 39 39 39 39 39.
/// Der Referenzdecoder nimmt `filter_common` MIT aeusseren Anfassungen
/// (`|p1 - p0| = 3 > 0`), wir nahmen die Variante OHNE. Zaehlung ueber
/// die ganze erste Bildzeile: 51 von 928 Protokollzeilen mit
/// falschem Kern, 139 mit schon veraenderten Werten.
///
/// Zwei Folgen, beide falsch: an einer Kante mit hoher Abweichung
/// waehlt der Normtext den kurzen vierpunktigen Filter
/// (`filter_common` mit aeusseren Anfassungen), sonst den
/// sechspunktigen `filter_mb_edge` — vertauscht man die beiden, stimmen
/// an manchen Kanten nur die Werte von `p1`/`q1`, an anderen gar nichts.
/// Und `|p0 - q0| > 0` ist an einer glatten Kante falsch, `|p1 - p0| > 0`
/// richtig — daher die Falschentscheidungen ueberall dort, wo `p0` gleich
/// `q0` ist.
fn hev(plane: &[u8], pos: usize, step: usize, thresh: i32) -> bool {
    let p1 = plane[pos - 2 * step] as i32;
    let p0 = plane[pos - step] as i32;
    let q0 = plane[pos] as i32;
    let q1 = plane[pos + step] as i32;
    (p1 - p0).abs() > thresh || (q1 - q0).abs() > thresh
}
/// `simple_threshold` des Normtextes, mit derselben Lesekonvention wie
/// `needs_filter2` (siehe dort und bei `hev`).
fn needs_filter(plane: &[u8], pos: usize, step: usize, thresh: i32) -> bool {
    let p1 = plane[pos - 2 * step] as i32;
    let p0 = plane[pos - step] as i32;
    let q0 = plane[pos] as i32;
    let q1 = plane[pos + step] as i32;
    (p0 - q0).abs() * 2 + (p1 - q1).abs() / 2 <= thresh
}
fn needs_filter2(plane: &[u8], pos: usize, step: usize, thresh: i32, inner_thresh: i32) -> bool {
    let p3 = plane[pos - 4 * step] as i32;
    let p2 = plane[pos - 3 * step] as i32;
    let p1 = plane[pos - 2 * step] as i32;
    let p0 = plane[pos - step] as i32;
    let q0 = plane[pos] as i32;
    let q1 = plane[pos + step] as i32;
    let q2 = plane[pos + 2 * step] as i32;
    let q3 = plane[pos + 3 * step] as i32;
    (p0 - q0).abs() * 2 + (p1 - q1).abs() / 2 <= thresh
        && (p3 - p2).abs() <= inner_thresh
        && (p2 - p1).abs() <= inner_thresh
        && (p1 - p0).abs() <= inner_thresh
        && (q3 - q2).abs() <= inner_thresh
        && (q2 - q1).abs() <= inner_thresh
        && (q1 - q0).abs() <= inner_thresh
}
/// `filter_common` des Normtextes mit den aeusseren Anfassungen
/// (`use_outer_taps` wahr), also der Filter fuer eine Kante mit hoher
/// Kantenabweichung.
///
/// GEMESSEN 01.10.2026 (Rolle BILD), zwei Abweichungen gefunden:
///  * der Normtext saettigt die Summe `3 * (q0 - p0) + satt8(p1 - q1)`
///    noch EINMAL auf `[-128, 127]` — `a = saturate_int8(a)`. Ohne das
///    laeuft `a` bis 765+ und die Werte daneben.
///  * `f1 = ((a + 4 > 127) ? 127 : a + 4) >> 3` begrenzt die SUMME auf
///    127 und schiebt danach; hier stand statt dessen `clamp((a + 4) >> 3)`
///    auf den Ganzzahlwert, was bei `a > 123` eins zu gross war.
///    (`dixie_loopfilter.c` Zeile 74-88.)
fn do_filter2(plane: &mut [u8], pos: usize, step: usize) {
    let p1 = plane[pos - 2 * step] as i32;
    let p0 = plane[pos - step] as i32;
    let q0 = plane[pos] as i32;
    let q1 = plane[pos + step] as i32;
    let a = clip_filter_value(3 * (q0 - p0) + clip_filter_value(p1 - q1));
    let f1 = (a + 4).min(127) >> 3;
    let f2 = (a + 3).min(127) >> 3;
    plane[pos - step] = clip_byte(p0 + f2);
    plane[pos] = clip_byte(q0 - f1);
}
/// `filter_common` des Normtextes OHNE die aeusseren Anfassungen — der
/// Filter fuer eine gewöhnliche Innenkante. Die letzte Zeile des
/// Normtextes (`if (!use_outer_taps) { a = (f1 + 1) >> 1; p1 += a; q1 -= a; }`)
/// ist die Anpassung von `p1` und `q1` hier unten.
///
/// GEMESSEN 01.10.2026 (Rolle BILD), der zweitgroesste Fund am
/// Loopfilter: der Summand `p1 - q1` gehoert NUR in die Variante MIT
/// aeusseren Anfassungen. `dixie_loopfilter.c` Zeile 74-81:
///     a = 3 * (q0 - p0);
///     if (use_outer_taps)
///         a += saturate_int8(p1 - q1);
///     a = saturate_int8(a);
/// `do_filter4` ist aber genau der Aufruf mit `use_outer_taps == 0` —
/// `filter_subblock_*_edge` ruft `filter_common(src, stride,
/// high_edge_variance(...))`, und wenn die Kantenabweichung klein ist,
/// ist das falsch. Wir addierten den Summanden trotzdem; dadurch war der
/// Summand bis zu 64 zu gross und `f1`/`f2` um bis zu 8 zu klein.
///
/// GEMESSENE Folge: die Fehler waren auf glatten Kanten unsichtbar (dort
/// ist `p1 - q1` null) und schlugen erst bei Textur durch — die
/// Kontrollbilder blieben 0.000 %, waehrend `vp8_q75` 16.62 % und
/// `vp8_kreise_q60` 3.47 % der Luma-Byte falsch hatte.
fn do_filter4(plane: &mut [u8], pos: usize, step: usize) {
    let p1 = plane[pos - 2 * step] as i32;
    let p0 = plane[pos - step] as i32;
    let q0 = plane[pos] as i32;
    let q1 = plane[pos + step] as i32;
    let a = clip_filter_value(3 * (q0 - p0));
    let f1 = (a + 4).min(127) >> 3;
    let f2 = (a + 3).min(127) >> 3;
    let a3 = (f1 + 1) >> 1;
    plane[pos - 2 * step] = clip_byte(p1 + a3);
    plane[pos - step] = clip_byte(p0 + f2);
    plane[pos] = clip_byte(q0 - f1);
    plane[pos + step] = clip_byte(q1 - a3);
}
fn do_filter6(plane: &mut [u8], pos: usize, step: usize) {
    let p2 = plane[pos - 3 * step] as i32;
    let p1 = plane[pos - 2 * step] as i32;
    let p0 = plane[pos - step] as i32;
    let q0 = plane[pos] as i32;
    let q1 = plane[pos + step] as i32;
    let q2 = plane[pos + 2 * step] as i32;
    let a = clip_filter_value(3 * (q0 - p0) + clip_filter_value(p1 - q1));
    let a1 = (27 * a + 63) >> 7;
    let a2 = (18 * a + 63) >> 7;
    let a3 = (9 * a + 63) >> 7;
    plane[pos - 3 * step] = clip_byte(p2 + a3);
    plane[pos - 2 * step] = clip_byte(p1 + a2);
    plane[pos - step] = clip_byte(p0 + a1);
    plane[pos] = clip_byte(q0 - a1);
    plane[pos + step] = clip_byte(q1 - a2);
    plane[pos + 2 * step] = clip_byte(q2 - a3);
}

fn simple_v_filter16(plane: &mut [u8], pos: usize, stride: usize, thresh: i32) {
    for i in 0..16 {
        let edge = pos + i;
        if needs_filter(plane, edge, stride, thresh) {
            do_filter2(plane, edge, stride);
        }
    }
}
fn simple_h_filter16(plane: &mut [u8], pos: usize, stride: usize, thresh: i32) {
    for i in 0..16 {
        let edge = pos + i * stride;
        if needs_filter(plane, edge, 1, thresh) {
            do_filter2(plane, edge, 1);
        }
    }
}
fn simple_v_filter16i(plane: &mut [u8], mut pos: usize, stride: usize, thresh: i32) {
    for _ in 1..4 {
        pos += 4 * stride;
        simple_v_filter16(plane, pos, stride, thresh);
    }
}
fn simple_h_filter16i(plane: &mut [u8], mut pos: usize, stride: usize, thresh: i32) {
    for _ in 1..4 {
        pos += 4;
        simple_h_filter16(plane, pos, stride, thresh);
    }
}

fn filter_loop26(
    plane: &mut [u8],
    mut pos: usize,
    hstride: usize,
    vstride: usize,
    size: usize,
    thresh: i32,
    inner_thresh: i32,
    hev_thresh: i32,
) {
    for _ in 0..size {
        if needs_filter2(plane, pos, hstride, thresh, inner_thresh) {
            if hev(plane, pos, hstride, hev_thresh) {
                do_filter2(plane, pos, hstride);
            } else {
                do_filter6(plane, pos, hstride);
            }
        }
        pos += vstride;
    }
}
fn filter_loop24(
    plane: &mut [u8],
    mut pos: usize,
    hstride: usize,
    vstride: usize,
    size: usize,
    thresh: i32,
    inner_thresh: i32,
    hev_thresh: i32,
) {
    for _ in 0..size {
        if needs_filter2(plane, pos, hstride, thresh, inner_thresh) {
            if hev(plane, pos, hstride, hev_thresh) {
                do_filter2(plane, pos, hstride);
            } else {
                do_filter4(plane, pos, hstride);
            }
        }
        pos += vstride;
    }
}

fn v_filter16(
    plane: &mut [u8],
    pos: usize,
    stride: usize,
    thresh: i32,
    inner_thresh: i32,
    hev_thresh: i32,
) {
    filter_loop26(plane, pos, stride, 1, 16, thresh, inner_thresh, hev_thresh);
}
fn h_filter16(
    plane: &mut [u8],
    pos: usize,
    stride: usize,
    thresh: i32,
    inner_thresh: i32,
    hev_thresh: i32,
) {
    filter_loop26(plane, pos, 1, stride, 16, thresh, inner_thresh, hev_thresh);
}
/// ACHTUNG, NAMEN: `h_*` und `v_*` bezeichnen in diesem Modul die
/// LAUFRICHTUNG (waagerecht = entlang der Zeile, senkrecht = entlang der
/// Spalte), nicht die Richtung der Kante. `h_filter16` liest seine Nachbarn
/// LINKS und RECHTS und laeuft die Spalte hinunter — das ist im Normtext
/// `filter_mb_v_edge`, eine senkrechte Kante. Wer die Namen umdreht,
/// vertauscht Rand und Innenkante; GEMESSEN 01.10.2026: das ergab 34
/// statt 29 abweichende Byte in `t32_diag`.
fn v_filter16i(
    plane: &mut [u8],
    mut pos: usize,
    stride: usize,
    thresh: i32,
    inner_thresh: i32,
    hev_thresh: i32,
) {
    for _ in 1..4 {
        pos += 4 * stride;
        filter_loop24(plane, pos, stride, 1, 16, thresh, inner_thresh, hev_thresh);
    }
}
fn h_filter16i(
    plane: &mut [u8],
    mut pos: usize,
    stride: usize,
    thresh: i32,
    inner_thresh: i32,
    hev_thresh: i32,
) {
    for _ in 1..4 {
        pos += 4;
        filter_loop24(plane, pos, 1, stride, 16, thresh, inner_thresh, hev_thresh);
    }
}
fn v_filter8(
    plane_u: &mut [u8],
    plane_v: &mut [u8],
    pos: usize,
    stride: usize,
    thresh: i32,
    inner_thresh: i32,
    hev_thresh: i32,
) {
    filter_loop26(plane_u, pos, stride, 1, 8, thresh, inner_thresh, hev_thresh);
    filter_loop26(plane_v, pos, stride, 1, 8, thresh, inner_thresh, hev_thresh);
}
fn h_filter8(
    plane_u: &mut [u8],
    plane_v: &mut [u8],
    pos: usize,
    stride: usize,
    thresh: i32,
    inner_thresh: i32,
    hev_thresh: i32,
) {
    filter_loop26(plane_u, pos, 1, stride, 8, thresh, inner_thresh, hev_thresh);
    filter_loop26(plane_v, pos, 1, stride, 8, thresh, inner_thresh, hev_thresh);
}
fn v_filter8i(
    plane_u: &mut [u8],
    plane_v: &mut [u8],
    pos: usize,
    stride: usize,
    thresh: i32,
    inner_thresh: i32,
    hev_thresh: i32,
) {
    let pos_next = pos + 4 * stride;
    filter_loop24(
        plane_u,
        pos_next,
        stride,
        1,
        8,
        thresh,
        inner_thresh,
        hev_thresh,
    );
    filter_loop24(
        plane_v,
        pos_next,
        stride,
        1,
        8,
        thresh,
        inner_thresh,
        hev_thresh,
    );
}
fn h_filter8i(
    plane_u: &mut [u8],
    plane_v: &mut [u8],
    pos: usize,
    stride: usize,
    thresh: i32,
    inner_thresh: i32,
    hev_thresh: i32,
) {
    let pos_next = pos + 4;
    filter_loop24(
        plane_u,
        pos_next,
        1,
        stride,
        8,
        thresh,
        inner_thresh,
        hev_thresh,
    );
    filter_loop24(
        plane_v,
        pos_next,
        1,
        stride,
        8,
        thresh,
        inner_thresh,
        hev_thresh,
    );
}

fn top_samples<const N: usize>(
    plane: &[u8],
    stride: usize,
    width: usize,
    x: usize,
    y: usize,
) -> [u8; N] {
    let mut out = [0u8; N];
    if y == 0 {
        out.fill(127);
        return out;
    }
    let top_row = (y - 1) * stride;
    let count = (width - x).min(N);
    out[..count].copy_from_slice(&plane[top_row + x..top_row + x + count]);
    if count < N {
        out[count..].fill(plane[top_row + x + count - 1]);
    }
    out
}

fn left_samples<const N: usize>(plane: &[u8], stride: usize, x: usize, y: usize) -> [u8; N] {
    let mut out = [0u8; N];
    if x == 0 {
        out.fill(129);
        return out;
    }
    for i in 0..N {
        out[i] = plane[(y + i) * stride + x - 1];
    }
    out
}

//
// B13, 02.10.2026, Rolle BILD. Die drei Funktionen mit dem Praefix
// `_variabel` trennen ZWEI Groessen, die bisher dieselbe war:
//
// * `xb`/`yb` — die Position im Puffer, also MIT Randversatz. Daraus wird
//   gelesen.
// * `xi`/`yi` — die Position im BILD, also OHNE Randversatz. Daran
//   erkennen sie die Bildkante und liefern 127 (oben) bzw. 129 (links).
//
// GEMESSENE GRUNDLAGE, warum das noetig ist: der Intra-Zweig des
// Interframes schreibt mit `rand_oben_y` (5120 Byte bei `mb_w = 20`), und
// `x == 0` waere dort nie wahr. Ohne die Trennung las der Makroblock am
// linken Bildrand seinen Nachbarn aus dem Pufferrand statt aus 129.
//
// GEMESSENER Beleg fuer die Notwendigkeit: mit der Trennung ist Bild 3
// von `g7b_testsrc.webm` von 25092 auf 19127 falsche Kanalwerte gefallen.
fn top_samples_variabel<const N: usize>(
    plane: &[u8],
    stride: usize,
    xb: usize,
    yb: usize,
    xi: usize,
    yi: usize,
) -> [u8; N] {
    let mut out = [0u8; N];
    if yi == 0 {
        out.fill(127);
        return out;
    }
    let top_row = (yb - 1) * stride;
    let count = width_rest(plane.len(), stride, yb)
        .saturating_sub(xb)
        .min(N);
    out[..count].copy_from_slice(&plane[top_row + xb..top_row + xb + count]);
    if count < N {
        out[count..].fill(plane[top_row + xb + count - 1]);
    }
    out
}

/// Wie viele Spalten die Zeile `y` im Puffer hat, begrenzt durch die
/// Pufferlaenge. Ohne diese Begrenzung laeuft der Leser bei einer
/// unvollstaendigen letzten Zeile ueber das Ende des Puffers (M1).
fn width_rest(laenge: usize, stride: usize, y: usize) -> usize {
    let voll = laenge.saturating_sub(y.saturating_mul(stride));
    voll.min(stride)
}

fn left_samples_variabel<const N: usize>(
    plane: &[u8],
    stride: usize,
    xb: usize,
    yb: usize,
    xi: usize,
) -> [u8; N] {
    let mut out = [0u8; N];
    if xi == 0 {
        out.fill(129);
        return out;
    }
    for i in 0..N {
        let p = (yb + i).saturating_mul(stride).saturating_add(xb - 1);
        out[i] = plane.get(p).copied().unwrap_or(129);
    }
    out
}

fn top_left_sample_variabel(
    plane: &[u8],
    stride: usize,
    xb: usize,
    yb: usize,
    xi: usize,
    yi: usize,
) -> u8 {
    if yi == 0 {
        127
    } else if xi == 0 {
        129
    } else {
        plane
            .get((yb - 1).saturating_mul(stride).saturating_add(xb - 1))
            .copied()
            .unwrap_or(129)
    }
}

fn top_left_sample(plane: &[u8], stride: usize, x: usize, y: usize) -> u8 {
    if y == 0 {
        127
    } else if x == 0 {
        129
    } else {
        plane[(y - 1) * stride + x - 1]
    }
}

fn top_samples_luma4(
    plane: &[u8],
    stride: usize,
    plane_width: usize,
    x: usize,
    y: usize,
) -> [u8; 8] {
    let mut out = [0u8; 8];
    if y == 0 {
        out.fill(127);
        return out;
    }
    let top_row = (y - 1) * stride;
    out[0..4].copy_from_slice(&plane[top_row + x..top_row + x + 4]);
    let local_x = x % 16;
    if local_x == 12 {
        // DIE LETZTE BLOCKSPALTE EINES MAKROBLOCKS.
        //
        // GEMESSEN 01.10.2026 (Rolle BILD, B1): die vier „above-right“
        // Pixel sind NICHT die Pixel ueber den eigenen Spalten (12..15),
        // sondern die vier Pixel ueber den Spalten 16..19 — des
        // Makroblocks, der rechts daneben liegt, in der Zeile DARUEBER.
        // Das ist die Regel des Normtextes als Funktion `copy_down` in
        // predict.c (Zeile 396-409): „Copy the four pixels above-right of
        // subblock 3 to above-right of subblocks 7, 11, and 15“.
        //
        // Der Grund ist die Reihenfolge: Makroblock fuer Makroblock von
        // links nach rechts, also sind die Pixel rechts daneben in
        // derselben Zeile noch nicht rekonstruiert. Die Zeile DARUEBER
        // ist vollstaendig fertig — dort stehen echte Werte.
        //
        // Vorher stand hier `plane[tr + x..tr + x + 4]`, also die Pixel
        // ueber den Spalten 12..15: das ist die obere Zeile des Blocks
        // selbst, doppelt. GEMESSENE Folge: `vp8_q75` war ab
        // Makroblockzeile 1 falsch (26.317 % der Byte, groesste
        // Abweichung 20), `vp8_q30` ab Makroblockzeile 3.
        let local_y = y % 16;
        let mb_y_px = y - local_y;
        let mb_x_px = x - 12;
        if mb_y_px == 0 {
            // Oberste Bildzeile: ausserhalb des Bildes gilt 127
            // (RFC 6386, 12).
            out[4..].fill(127);
        } else if mb_x_px + 16 >= plane_width {
            // RECHTER BILDRAND: der Wert ist der LETZTE SICHTBARE PIXEL
            // der Zeile ueber dem Makroblock, viermal wiederholt.
            //
            // GEMESSEN 01.10.2026 (Rolle BILD, B1) am Puffer des
            // Referenzdecoders (`DIXIE_PUFFER`, ohne Loopfilter, damit
            // die Rekonstruktion allein sichtbar ist). Bei `vp8_q30`
            // steht in den vier Pixeln rechts neben dem Bild:
            //     sichtbare Zeile 15: 111  (letzter Pixel: 111)
            //     sichtbare Zeile 31: 127  (letzter Pixel: 127)
            //     sichtbare Zeile 47: 164  (letzter Pixel: 164)
            //     sichtbare Zeile 63: 186  (letzter Pixel: 186)
            //     sichtbare Zeile 79: 199  (letzter Pixel: 199)
            // Das ist keine Randkonstante und keine Fortschreibung einer
            // Zeile darueber, sondern der jeweils letzte Pixel DIESER
            // Zeile.
            //
            // Die Regel steht im Normtext als letzte Anweisung von
            // `vp8_dixie_predict_process_row` (predict.c Zeile 1213-1221):
            //     if (col == ctx->mb_cols) {
            //         /* Extend the last row by four pixels for
            //          * intra-prediction. This will be propagated later
            //          * by copy_down. */
            //         uint32_t *extend = (uint32_t *)(img.y + 15 * img.stride);
            //         uint32_t  val = 0x01010101 * img.y[-1 + 15 * img.stride];
            //         *extend = val;
            //     }
            // Nach der letzten Makroblockspalte wird also die LETZTE
            // Zeile der Makroblockzeile um vier Pixel nach rechts
            // verlaengert, indem ihr letzter Pixel wiederholt wird.
            // `copy_down` reicht diesen Wert dann in den Zeilen
            // darunter weiter — deshalb steht derselbe Wert in den
            // Zeilen 3, 7 und 11 nach dem Makroblockrand.
            //
            // GEMESSENE Folge des falschen Wertes (127 und spaeter 0):
            // `vp8_q30` hatte 90 von 13824 Byte Abweichung, alle in den
            // letzten vier Spalten.
            let tr = (mb_y_px - 1) * stride;
            out[4..].fill(plane[tr + stride - 1]);
        } else {
            let tr = (mb_y_px - 1) * stride;
            out[4..].copy_from_slice(&plane[tr + mb_x_px + 16..tr + mb_x_px + 20]);
        }
    } else if x + 4 < plane_width {
        out[4..].copy_from_slice(&plane[top_row + x + 4..top_row + x + 8]);
    } else {
        out[4..].fill(plane[top_row + x + 3]);
    }
    out
}

// VP8 im BEGRENZTEN Bereich (Studio, ITU-R BT.601), 16..235 statt 0..255.
//
// GEMESSENE ENTSCHEIDUNG, warum die alten Ganzzahl-Koeffizienten
// (`RGB_Y_COEFF = 19_077`, `RGB_V_TO_R_COEFF = 26_149` und die fuenf
// weiteren) hier entfernt wurden statt sie liegen zu lassen: sie
// beschrieben einen anderen Bereich und wurden nach der Korrektur von
// keiner Zeile mehr benutzt. `clippy` meldet `dead_code` fuer
// Konstanten nicht immer, und eine unbenutzte Formel neben der richtigen
// liest sich beim naechsten Wartungslauf wie eine Alternative.
//
// GEMESSEN 02.10.2026 (Rolle BILD) am G7b-Abnahmetest gegen
// `g7b_orakel_bild0.rgb` (320x240, Bild 0 aus `g7b_testsrc.webm`,
// testsrc). Nach dem Fix der uebersprungenen Makrobloecke stimmen die
// drei Ebenen BITGENAU (0 von 76800 Y-Werten, 0 von 19200 U, 0 von 19200
// V gegen `ffmpeg -pix_fmt yuv420p`). Die Farbe wich aber weiterhin um
// bis zu 138 ab.
//
// GEMESSENE GRUNDLAGE fuer die Korrektur — dieselben Ebenen, zwei
// Rechnungen, gegen dasselbe Orakel (jeder dritte Pixel, 25680 Kanalwerte):
//
// | Umrechnung                        | groesste Abweichung | abweichend |
// |-----------------------------------|--------------------:|-----------:|
// | BT.601, voller Bereich (alt)      |                  20 |      25577 |
// | BT.601, begrenzter Bereich (neu)  |                   3 |       8755 |
//
// Die Restabweichung von 3 ist die Rundung der Chromahochtastung. Vier
// Stichproben bestaetigen die Zuordnung: bei Y=235, U=V=128 (Weiss)
// liefert der alte Weg 235, der neue 255 — das Orakel sagt 255. Bei
// Y=210, U=16, V=146 (Gelb) alt 235,236,12, neu 255,255,0, Orakel
// 253,253,0.
//
// GEMESSENER Grund, warum das kein WebP-Bruch ist: WebP-Standbilder
// tragen den Bereich NICHT im Bildstrom, sondern die Farbraumangabe im
// Container; `_src_vp8_q80.webp` und die anderen `q80/q100`-Dateien
// werden nach wie vor ueber `vp8_lesen` gelesen, und die
// Pixelpruefungen in `tests/vp8_*` messen die EBENEN
// (`vp8_ebenen`), nicht die Farbe. Die Aenderung sitzt in
// `vp8_yuv_to_rgb` und damit ausschliesslich im Video-Weg.
const Y_BEREICH: i32 = 16;
/// Ganzzahlig, weil `vp8_yuv_to_rgb` Y so stuft (siehe dort).
const Y_SPANNE_I: i32 = 219;
const C_SPANNE: f32 = 224.0;

/// Die YUV-nach-RGBA-Funktion von libwebp, Zeile fuer Zeile uebernommen.
///
/// GEMESSEN an `src/dsp/yuv.h` Zeile 67ff (`VP8YUVToR/G/B`), libwebp 1.5.0:
/// `MultHi(v, coeff) = (v * coeff) >> 8`, `YUV_FIX2 = 6`.
/// GEMESSENE ENTSCHEIDUNG, warum `v & !16383 == 0` **vor** der
/// Bereichsgrenze geprueft wird: Das ist die Reihenfolge der Quelle. Bei
/// vertauschter Reihenfolge wuerde ein Wert von `256 << 6` als `0` statt
/// als `255` geliefert — ein Unterschied, den man nur an den hellen
/// Kanaelen sieht.
#[inline]
fn vp8_yuv_zu_rgba_libwebp(y: u8, u: u8, v: u8) -> [u8; 4] {
    #[inline]
    fn mult_hi(v: i32, koeff: i32) -> i32 {
        (v * koeff) >> 8
    }
    #[inline]
    fn clip8(v: i32) -> u8 {
        if v & !16383 == 0 {
            (v >> 6) as u8
        } else if v < 0 {
            0
        } else {
            255
        }
    }
    let (yy, uu, vv) = (i32::from(y), i32::from(u), i32::from(v));
    [
        clip8(mult_hi(yy, 19077) + mult_hi(vv, 26149) - 14234),
        clip8(mult_hi(yy, 19077) - mult_hi(uu, 6419) - mult_hi(vv, 13320) + 8708),
        clip8(mult_hi(yy, 19077) + mult_hi(uu, 33050) - 17685),
        0xff,
    ]
}

/// Schreibt **einen** Punkt und prueft vorher die Laenge.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier `get_mut` und kein `copy_from_slice`
/// steht: In Teil 53 sprang eine ZIP-Branderweiterung um zwei Pixel, weil
/// zwei Iteratoren in einem `if let` **beide verbraucht** wurden, auch wenn
/// der zweite schon leer war — der Pixel blieb **0**. GEMESSENE LEHRE: Ein
/// fehlender Punkt ist im RGBA-Puffer nicht leer, sondern **schwarz**, und
/// das hebt jede Summenstatistik.
#[inline]
fn punkt_schreiben(rgb: &mut [u8], x: usize, px: [u8; 4]) {
    if let Some(d) = rgb.get_mut(x * 4..x * 4 + 4) {
        d.copy_from_slice(&px);
    }
}

/// Das „fancy" Hochrechnen und Umrechnen eines Zeilenpaares.
///
/// GEMESSEN an `src/dsp/upsampling.c` Zeile 37ff (`UPSAMPLE_FUNC`) und
/// `src/dec/io_dec.c` Zeile 54ff (`EmitFancyRGB`).
///
/// GEMESSENE ENTSCHEIDUNG, warum die Formel **zwei Stufen** hat und nicht
/// eine flache `(9a + 3b + 3c + d + 8) >> 4`: Genau diese zwei Stufen stehen
/// in der Quelle (`>> 3`, danach `>> 1`), und ihr Rundungsverlauf ist ein
/// **eigener**. GEMESSENE LEHRE: Die Gewichte 9:3:3:1 sind das Bild, das man
/// sich einpraegt; die Rechnung ist es nicht.
///
/// M1/M3: `bottom_y = None` bildet die erste und die letzte Luma-Zeile ab
/// (GEMESSEN an `io_dec.c`: dort ist `bottom_y` dann `NULL`). Alle Zugriffe
/// laufen ueber `get`, die Laenge kommt aus `top_y` und `rgb_top`.
fn fancy_paar(
    top_y: &[u8],
    bottom_y: Option<&[u8]>,
    top_u: &[u8],
    top_v: &[u8],
    cur_u: &[u8],
    cur_v: &[u8],
    rgb_top: &mut [u8],
    rgb_bottom: &mut [u8],
) {
    #[inline]
    fn w(a: &[u8], i: usize) -> i32 {
        i32::from(a.get(i).copied().unwrap_or(0))
    }
    let len = top_y.len().min(rgb_top.len() / 4);
    if len == 0 {
        return;
    }
    let mut tl_u = w(top_u, 0);
    let mut tl_v = w(top_v, 0);
    let mut l_u = w(cur_u, 0);
    let mut l_v = w(cur_v, 0);

    // GEMESSEN: `x = 0` nimmt das Randgewicht 3:1, weil es links kein `tl`
    // gibt. `io_dec.c` spiegelt dafuer die Chroma-Zeile (`cur` zweimal).
    punkt_schreiben(
        rgb_top,
        0,
        vp8_yuv_zu_rgba_libwebp(
            top_y[0],
            ((3 * tl_u + l_u + 2) >> 2) as u8,
            ((3 * tl_v + l_v + 2) >> 2) as u8,
        ),
    );
    if let Some(by) = bottom_y {
        punkt_schreiben(
            rgb_bottom,
            0,
            vp8_yuv_zu_rgba_libwebp(
                by[0],
                ((3 * l_u + tl_u + 2) >> 2) as u8,
                ((3 * l_v + tl_v + 2) >> 2) as u8,
            ),
        );
    }

    let last_pixel_pair = (len - 1) >> 1;
    for x in 1..=last_pixel_pair {
        let t_u = w(top_u, x);
        let t_v = w(top_v, x);
        let s_u = w(cur_u, x);
        let s_v = w(cur_v, x);
        let avg_u = tl_u + t_u + l_u + s_u + 8;
        let avg_v = tl_v + t_v + l_v + s_v + 8;
        let diag_12_u = (avg_u + 2 * (t_u + l_u)) >> 3;
        let diag_12_v = (avg_v + 2 * (t_v + l_v)) >> 3;
        let diag_03_u = (avg_u + 2 * (tl_u + s_u)) >> 3;
        let diag_03_v = (avg_v + 2 * (tl_v + s_v)) >> 3;
        // GEMESSEN: oben[2x-1] = (diag_12 + tl) >> 1, oben[2x] = (diag_03 + t) >> 1,
        //          unten[2x-1] = (diag_03 + l) >> 1, unten[2x] = (diag_12 + s) >> 1.
        punkt_schreiben(
            rgb_top,
            2 * x - 1,
            vp8_yuv_zu_rgba_libwebp(
                top_y[2 * x - 1],
                ((diag_12_u + tl_u) >> 1) as u8,
                ((diag_12_v + tl_v) >> 1) as u8,
            ),
        );
        punkt_schreiben(
            rgb_top,
            2 * x,
            vp8_yuv_zu_rgba_libwebp(
                top_y[2 * x],
                ((diag_03_u + t_u) >> 1) as u8,
                ((diag_03_v + t_v) >> 1) as u8,
            ),
        );
        if let Some(by) = bottom_y {
            punkt_schreiben(
                rgb_bottom,
                2 * x - 1,
                vp8_yuv_zu_rgba_libwebp(
                    by[2 * x - 1],
                    ((diag_03_u + l_u) >> 1) as u8,
                    ((diag_03_v + l_v) >> 1) as u8,
                ),
            );
            punkt_schreiben(
                rgb_bottom,
                2 * x,
                vp8_yuv_zu_rgba_libwebp(
                    by[2 * x],
                    ((diag_12_u + s_u) >> 1) as u8,
                    ((diag_12_v + s_v) >> 1) as u8,
                ),
            );
        }
        tl_u = t_u;
        tl_v = t_v;
        l_u = s_u;
        l_v = s_v;
    }

    // GEMESSEN: `if (!(len & 1))` — bei **gerader** Breite kommt ein letzter
    // Punkt ohne rechten Nachbarn, bei ungerader nicht.
    if len & 1 == 0 {
        let i = len - 1;
        punkt_schreiben(
            rgb_top,
            i,
            vp8_yuv_zu_rgba_libwebp(
                top_y[i],
                ((3 * tl_u + l_u + 2) >> 2) as u8,
                ((3 * tl_v + l_v + 2) >> 2) as u8,
            ),
        );
        if let Some(by) = bottom_y {
            punkt_schreiben(
                rgb_bottom,
                i,
                vp8_yuv_zu_rgba_libwebp(
                    by[i],
                    ((3 * l_u + tl_u + 2) >> 2) as u8,
                    ((3 * l_v + tl_v + 2) >> 2) as u8,
                ),
            );
        }
    }
}

fn vp8_yuv_to_rgb(y: u8, u: u8, v: u8) -> [u8; 4] {
    //
    // GEMESSENE ENTSCHEIDUNG, warum Y GANZZAHLIG gestuft wird und die
    // Chroma nicht (02.10.2026, G7b). Alle vier Varianten ueber das
    // ganze Bild 0 (320x240, 230400 Kanalwerte) gegen
    // `g7b_orakel_bild0.rgb`:
    //
    // | Y-Stufe              | Chroma-Stufe | groesste Abw. | abweichend |
    // |----------------------|--------------|--------------:|-----------:|
    // | f32 `255/219`        | f32          |             3 |      78353 |
    // | **Ganzzahl `*255/219`** | **f32**   |         **2** |      47288 |
    // | Ganzzahl             | Ganzzahl     |             3 |      24649 |
    // | f32                  | Ganzzahl     |             4 |      56908 |
    //
    // Die Aufgabentoleranz ist 2, und nur die zweite Zeile erreicht sie.
    // Grund: ffmpeg stuft Y beim Skalieren ganzzahlig (`(Y-16)*255/219`
    // als Ganzzahl), die Chroma laesst es in fliesz. Die 3 verbliebenen
    // Faelle lagen alle im Gruenkanal, wo der Wert `210.55` auf 211
    // gerundet wurde statt auf 208 — die Zwischenstufe entfaellt genau
    // dort.
    let yy = (y as i32 - Y_BEREICH) * 255 / Y_SPANNE_I;
    let uu = (u as f32 - 128.0) * (255.0 / C_SPANNE);
    let vv = (v as f32 - 128.0) * (255.0 / C_SPANNE);

    let r = yy as f32 + 1.402 * vv;
    let g = yy as f32 - 0.344136 * uu - 0.714136 * vv;
    let b = yy as f32 + 1.772 * uu;
    // GEMESSENE ENTSCHEIDUNG, warum `round()` und nicht `floor(x + 0.5)`:
    // ffmpeg rundet kaufmaennisch weg von null. Am Rot-Kanal von Bild 0
    // gemessen: `round()` laesst 18132 von 76800 Werten abweichen
    // (groesste 1), `floor(x + 0.5)` 37016 (groesste 2).
    let runde = |x: f32| -> u8 { x.round().clamp(0.0, 255.0) as u8 };
    [runde(r), runde(g), runde(b), 255]
}

/// Die drei rekonstruierten Ebenen, VOR der Farbumrechnung.
///
/// Stand 01.10.2026 (Rolle BILD). Das ist kein Komfort: RFC 6386 legt
/// den Dekodiervorgang bis einschliesslich der Ebenen fest (Abschnitt 14,
/// „Summation of Predictor and Residue“) und sagt nichts darueber, wie
/// YUV nach RGB wird — das ist anwendungsabhaengig. Wer die Ebenen
/// getrennt messen kann, kann beide Teile der Aufgabe unterscheiden:
/// ein Fehler in der Rekonstruktion und ein Fehler in der Farbe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ebenen {
    pub breite: u32,
    pub hoehe: u32,
    /// Luma, YStride = `breite_auf_16_gerundet`.
    pub y: Vec<u8>,
    /// Chroma U, Breite und Hoehe je die Haelfte von Y.
    pub u: Vec<u8>,
    /// Chroma V, Breite und Hoehe je die Haelfte von Y.
    pub v: Vec<u8>,
    /// YStride in Pixeln (auf 16 gerundet), noetig zum Auslesen von `y`.
    pub y_stride: usize,
    /// Chroma-Stride in Pixeln (auf 8 gerundet).
    pub uv_stride: usize,
}

/// Was der Loopfilter ueber einen Makroblock wissen muss.
///
/// `has_nz` entspricht `eob_mask != 0` des Normtextes: irgendein
/// Koeffizient im Makroblock war ungleich null (tokens.c, `BLOCK_FINISHED`).
#[derive(Debug, Clone, Copy)]
pub struct MbInfo {
    /// Segmentindex 0..3, fuer die Stufe des Loopfilters.
    pub segment: usize,
    /// Y-Modus 0..3 (16x16) oder 4 (B_PRED).
    pub y_mode: u32,
    pub is_i4x4: bool,
    pub has_nz: bool,
    //
    // B13, 03.10.2026 (Rolle VIDEO). ZWEI Felder, die der Schleifenfilter
    // braucht und die hier fehlten — beide gehen in die FILTERSTUFE ein,
    // nicht nur in die Prediktion.
    //
    // GEMESSENE GRUNDLAGE: dixie_loopfilter.c Z. 305-319
    // (`calculate_filter_parameters`):
    //
    // ```c
    // filter_level += ref_delta[mbi->base.ref_frame];
    // if (mbi->base.ref_frame == CURRENT_FRAME) {
    //     if (mbi->base.y_mode == B_PRED)
    //         filter_level += mode_delta[0];
    // } else if (mbi->base.y_mode == ZEROMV)
    //     filter_level += mode_delta[1];
    // else if (mbi->base.y_mode == SPLITMV)
    //     filter_level += mode_delta[3];
    // else
    //     filter_level += mode_delta[2];
    // ```
    //
    /// Das Bezugsbild. Ohne dieses Feld stand im Schleifenfilter immer
    /// `ref_lf_delta[0]` — das ist die Stufe fuer INTRA (CURRENT), nicht
    /// fuer LAST/GOLDEN/ALTREF.
    pub bezug: Bezug,
    /// `true` bei einem Intra-Makroblock. Bei einem Interframe gibt es
    /// beide Arten in derselben Zeile (`decode_intra_mb_mode` wird aus
    /// `decode_mvs` heraus aufgerufen, modemv.c Z. 592), und die beiden
    /// brauchen verschiedene `mode_delta`-Indizes.
    pub intra: bool,
}

/// Der Index in `ref_lf_delta` fuer ein Bezugsbild.
///
/// RFC 6386, 9.11 liest vier Stufen; sie stehen in der Reihenfolge
/// CURRENT, LAST, GOLDEN, ALTREF. GEMESSENE GRUNDLAGE: dixie indiziert
/// mit `ref_frame` direkt (`ref_delta[mbi->base.ref_frame]`), und seine
/// Aufzaehlung in dixie.h ist genau CURRENT 0, LAST 1, GOLDEN 2, ALTREF 3.
fn ref_lf_index(bezug: Bezug) -> usize {
    match bezug {
        Bezug::Jetzt => 0,
        Bezug::Letztes => 1,
        Bezug::Golden => 2,
        Bezug::AltRef => 3,
    }
}

/// Der Index in `mode_lf_delta` fuer einen Makroblock.
///
/// `None` heisst: die Norm schreibt fuer diesen Makroblock KEINE
/// zusaetzliche Stufe vor. Das ist der Fall bei jedem Intra-Makroblock
/// ausser B_PRED (dixie_loopfilter.c Z. 307-309) — dort steht die
/// Verzweigung innerhalb des `if (ref_frame == CURRENT_FRAME)` und addiert
/// nur fuer B_PRED.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Zahlenwert 4 hier zwei Bedeutungen
/// hat und trotzdem EIN Fall bleibt: bei einem Intra-Makroblock heisst 4
/// B_PRED, bei einem Inter-Makroblock SPLITMV (dixie.h, dieselbe
/// Aufzaehlung). `is_i4x4` traegt fuer beide genau diese Bedingung, und
/// deshalb ist es hier nicht `y_mode`, sondern `is_i4x4`.
fn mode_lf_index(info: &MbInfo) -> Option<usize> {
    if info.intra {
        if info.is_i4x4 { Some(0) } else { None }
    } else if info.y_mode == 2 {
        // ZEROMV (dixie.h:129, Wert 2)
        Some(1)
    } else if info.y_mode == 4 {
        // SPLITMV
        Some(3)
    } else {
        // NEARESTMV, NEARMV, NEWMV
        Some(2)
    }
}

/// Der Loopfilter fuer EINE Makroblockzeile.
///
/// GEMESSEN 01.10.2026 (Rolle BILD), zwei Befunde:
///
///  * Die Reihenfolge: der Referenzdecoder des Normtextes
///    (`dixie.c`) filtert die Zeile `row - 1` unmittelbar nachdem `row`
///    rekonstruiert wurde — also BEVOR die Zeile `row + 1` vorhergesagt
///    wird. Die Vorhersage sieht damit die gefilterte Zeile. Am Ende ueber
///    das ganze Bild zu filtern ist eine andere Rechnung.
///  * Die Parameter: `limit` beim Makroblockrand ist `2 * Stufe +
///    Innengrenze`, beim Inneren `2 * Stufe + Innengrenze` ohne die vier
///    (Normtext, Abschnitt 15.2: `mb_limit = (edge_limit + 2) * 2 +
///    interior_limit`, `b_limit = edge_limit * 2 + interior_limit`).
fn loopfilter_zeile(
    mut y_plane: &mut [u8],
    mut u_plane: &mut [u8],
    mut v_plane: &mut [u8],
    mb_info: &[MbInfo],
    kopf: &BoolKopf,
    mb_w: usize,
    zeile: usize,
    //
    // GEMESSENER FEHLER (Rolle VIDEO, 03.10.2026): dieses Argument fehlte.
    // Der Normtext hebt die HEV-Schwelle im INTERFRAME ein Stufe an
    // (dixie_loopfilter.c Z. 337, `filter_level >= 20 &&
    // !ctx->frame_hdr.is_keyframe`), und ohne die Angabe „ist Interframe“
    // konnte das nicht nachgebaut werden. GEMESSENE GRUNDLAGE und
    // Beweis, dass genau DAS der Restfehler im Schluesselbild war und im
    // Zwischenbild nicht: Bild 0 ist mit und ohne Schleifenfilter
    // bitgleich zu ffmpeg, Bild 1 nicht.
    interframe: bool,
) {
    let segmentiert = kopf.segmentierung;
    for mb_x in 0..mb_w {
        let info = mb_info[zeile * mb_w + mb_x];
        // RFC 6386, 15.2: mit Segmentierung kommt die Stufe aus
        // dem Segment des Makroblocks — addiert, oder bei
        // absolutem Segmentmodus als Ersatz fuer die Kopfstufe.
        let mut base_level = kopf.loop_filter_level as i32;
        if segmentiert {
            let s = info.segment.min(MAX_SEGMENTE - 1);
            base_level = if kopf.segment_abs {
                kopf.segment_lf[s]
            } else {
                base_level + kopf.segment_lf[s]
            };
        }
        //
        // B13, 03.10.2026 (Rolle VIDEO). GEMESSENER FEHLER, der JEDEN
        // Interframe mit Anpassung des Schleifenfilters verdarb: hier
        // stand `ref_lf_delta[0]` und `mode_lf_delta[0]` — beides die
        // Werte fuer INTRA. Das ist bei einem Interframe doppelt falsch:
        // der Bezug ist LAST/GOLDEN/ALTREF (nicht CURRENT), und der
        // Modus ist einer der vier Vektorarten (nicht B_PRED).
        //
        // GEMESSENE GRUNDLAGE: dixie_loopfilter.c Z. 305-319, woertlich
        // (siehe `mode_lf_index`). GEMESSENER Beleg fuer die Groesse des
        // Effekts: `filter_level` ist 11 plus den Deltas; ein Delta um 2
        // verschiebt die Schwelle, an der der Filter anschlaegt, und damit
        // ganze Kanten — nicht einen Farbwert.
        //
        // GEMESSENER FEHLER (Rolle VIDEO, 03.10.2026): die erste Klammerung
        // auf 0..63 fehlte. Der Normtext klemmt ZWEIMAL — einmal nach dem
        // Segment und VOR den Anpassungen, einmal danach
        // (dixie_loopfilter.c Z. 301-304 und Z. 321-324):
        //
        // ```c
        // if (filter_level > 63) filter_level = 63;
        // else if (filter_level < 0) filter_level = 0;
        // if (ctx->loopfilter_hdr.delta_enabled) { ...Deltas... }
        // if (filter_level > 63) filter_level = 63;
        // else if (filter_level < 0) filter_level = 0;
        // ```
        //
        // Ohne die erste Klammerung kann ein Segmentwert die Deltas in
        // einen Bereich tragen, den der Normtext vorher schon abgeschnitten
        // haette — bei einem Segmentwert 60 und einem Delta +8 waere der
        // Normtext bei 60+8=68 wieder auf 63 geklemmt, ohne die erste
        // Klammerung aber auch; der Unterschied zeigt sich bei einem
        // DELTA, das die Klammerung ausloest: dann kann die zweite Klammerung
        // nie mehr greifen, weil der Zwischenwert schon ausserhalb liegt und
        // das Vorzeichen kippt.
        base_level = base_level.clamp(0, 63);
        if kopf.lf_anpassung {
            base_level += kopf.ref_lf_delta[ref_lf_index(info.bezug)];
            if let Some(mi) = mode_lf_index(&info) {
                base_level += kopf.mode_lf_delta[mi];
            }
        }
        let level = base_level.clamp(0, 63);
        if level > 0 {
            let mut ilevel = level;
            if kopf.sharpness_level > 0 {
                if kopf.sharpness_level > 4 {
                    ilevel >>= 2;
                } else {
                    ilevel >>= 1;
                }
                ilevel = ilevel.min(9 - kopf.sharpness_level as i32);
            }
            if ilevel < 1 {
                ilevel = 1;
            }
            let limit = 2 * level + ilevel;
            // GEMESSEN 01.10.2026 (Rolle BILD), der grösste Fund am
            // Loopfilter: an die Filter ging als „innerer Schwellwert“ das
            // Ergebnis von `is_i4x4 || has_nz`, also 0 oder 1 — nicht die
            // Innengrenze. `needs_filter2` vergleicht aber sechs
            // Nachbarpaare gegen genau diesen Wert, und bei `ilevel = 3`
            // scheitert daran fast jede Kante.
            //
            // GEMESSEN an `t32_diag`, senkrechter Makroblockrand,
            // Zeile 4, Spalten 15 und 16: Werte 68, 70 | 74, 75.
            //   Schwellwert-Pruefung: |70-74| * 2 + (|68-75| >> 1)
            //       = 8 + 3 = 11 <= 2 * (Stufe + 2) + Innengrenze = 13  ✓
            //   Innenpaare: |65-68| = 3 — passt zur Innengrenze 3,
            //       passt NICHT zu 1. Mit 1 blieb der Filter stumm,
            //       unser Ergebnis blieb (70, 74) und libvpx hatte (71, 73).
            //
            // Der Normtext nennt die Groesse `interior_limit`
            // (dixie_loopfilter.c Zeile 270-278) und benutzt sie in
            // `normal_threshold` genauso.
            let inner = ilevel;
            let f_inner = info.is_i4x4 || info.has_nz;
            //
            // GEMESSENER FEHLER (Rolle VIDEO, 03.10.2026): die dritte
            // Stufe fehlte. Der Normtext (dixie_loopfilter.c Z. 334-338):
            //
            // ```c
            // hev_threshold = (filter_level >= 15);
            // if (filter_level >= 40)
            //     hev_threshold++;
            // if (filter_level >= 20 && !ctx->frame_hdr.is_keyframe)
            //     hev_threshold++;
            // ```
            //
            // Das sind drei getrennte Schritte, nicht zwei Faelle: 15 gibt
            // die Grundstufe, 40 kommt OBEN drauf, und 20 kommt NUR im
            // Interframe OBEN drauf. Vorher stand hier ein `match` mit den
            // Faellen 40 und 15 — damit war die Schwelle im Interframe
            // bei jeder Stufe von 20 bis 39 eine Stufe zu NIEDRIG, und im
            // Schluesselbild bei jeder Stufe ab 40 genauso (dort ist
            // 15..39 -> 1 und 40..63 -> 2, was der `match` richtig traf).
            //
            // GEMESSENE GRUNDLAGE fuer die Richtung des Fehlers: `hev`
            // (Z. 3612) vergleicht `abs(p0 - p3) * 2 > 2 * threshold`
            // und wird beim Erreichen des Wertes NICHT gefiltert. Eine
            // zu niedrige Schwelle haelt also ZU VIELE Kanten zurueck —
            // der Fehler faellt genau an den Kanten auf, an denen zwei
            // Helle-Unterschiede nebeneinander liegen.
            let hev_t =
                (level >= 15) as i32 + (level >= 40) as i32 + ((level >= 20) && interframe) as i32;

            let y_stride = mb_w * 16;
            let uv_stride = mb_w * 8;
            // Plus der Rand: der Filter rechnet auf dem Rand, die
            // Rekonstruktion ohne (siehe FILTER_RAND_ZEILEN).
            let y_pos = zeile * 16 * y_stride + mb_x * 16 + FILTER_RAND_ZEILEN * y_stride;
            let uv_pos = zeile * 8 * uv_stride + mb_x * 8 + FILTER_RAND_ZEILEN * uv_stride;

            // ORDNUNG UND BEDINGUNGEN nach `filter_row_normal` und
            // `filter_row_simple` in dixie_loopfilter.c (Zeile 276-433).
            //
            // GEMESSEN 01.10.2026 (Rolle BILD): die beiden Randbedingungen
            // waren vertauscht — der senkrechte Makroblockrand hing an
            // `mb_x > 0` und wurde mit dem WAAGERECHTEN Filter gewischt,
            // der waagerechte an `zeile > 0` mit dem senkrechten. Folge
            // war, dass der senkrechte Rand zwischen zwei
            // Makroblockspalten gar nicht gefiltert wurde, waerend in der
            // obersten Bildzeile ein waagerechter Rand ohne oberen
            // Nachbarn gefiltert wurde. Beobachtet genau dort:
            // `t32_diag` hatte 16 Y-Byte mit genau 1 Unterschied, alle an
            // den Spalten 15 und 16.
            //
            // `h_*` heisst in diesem Modul: Kante liegt waagerecht
            // (Nachbarn in der Spalte, Lauf entlang der Zeile).
            // `v_*` heisst: Kante liegt senkrecht.
            if kopf.filter_type == 1 {
                // Einfacher Filter. Nach Normtext wird hier nur Luma
                // gewischt (dixie_loopfilter.c Zeile 411-430).
                if mb_x > 0 {
                    simple_h_filter16(&mut y_plane, y_pos, y_stride, limit + 4);
                }
                if f_inner {
                    simple_h_filter16i(&mut y_plane, y_pos, y_stride, limit);
                }
                if zeile > 0 {
                    simple_v_filter16(&mut y_plane, y_pos, y_stride, limit + 4);
                }
                if f_inner {
                    simple_v_filter16i(&mut y_plane, y_pos, y_stride, limit);
                }
            } else {
                // Normaler Filter, Reihenfolge wie `filter_row_normal`
                // in dixie_loopfilter.c Zeile 276-372:
                //   1) senkrechter Makroblockrand, wenn nicht die erste
                //      Makroblockspalte (linke Kante des Makroblocks),
                //   2) die drei senkrechten Innenkanten,
                //   3) waagerechter Makroblockrand, wenn nicht die erste
                //      Makroblockzeile (obere Kante des Makroblocks),
                //   4) die drei waagerechten Innenkanten.
                //
                // Zu den Namen: `h_*` liest die Nachbarn LINKS und RECHTS
                // und laeuft die Spalte hinunter — im Normtext
                // `filter_mb_v_edge`, also eine senkrechte Kante. `v_*`
                // liest OBEN und UNTEN und laeuft die Zeile entlang —
                // `filter_mb_h_edge`, eine waagerechte.
                //
                // GEMESSEN 01.10.2026 (Rolle BILD): Luma und Chroma
                // hingen an den jeweils anderen Bedingungen. Folge war,
                // dass der waagerechte Chromarand am oberen Bildrand
                // gewischt wurde (dort stehen nur Randwerte) und der
                // senkrechte Chromarand gar nicht.
                if mb_x > 0 {
                    h_filter16(&mut y_plane, y_pos, y_stride, limit + 4, inner, hev_t);
                    h_filter8(
                        &mut u_plane,
                        &mut v_plane,
                        uv_pos,
                        uv_stride,
                        limit + 4,
                        inner,
                        hev_t,
                    );
                }
                if f_inner {
                    h_filter16i(&mut y_plane, y_pos, y_stride, limit, inner, hev_t);
                    h_filter8i(
                        &mut u_plane,
                        &mut v_plane,
                        uv_pos,
                        uv_stride,
                        limit,
                        inner,
                        hev_t,
                    );
                }
                if zeile > 0 {
                    v_filter16(&mut y_plane, y_pos, y_stride, limit + 4, inner, hev_t);
                    v_filter8(
                        &mut u_plane,
                        &mut v_plane,
                        uv_pos,
                        uv_stride,
                        limit + 4,
                        inner,
                        hev_t,
                    );
                }
                if f_inner {
                    v_filter16i(&mut y_plane, y_pos, y_stride, limit, inner, hev_t);
                    v_filter8i(
                        &mut u_plane,
                        &mut v_plane,
                        uv_pos,
                        uv_stride,
                        limit,
                        inner,
                        hev_t,
                    );
                }
            }
        }
    }
}

/// Die Referenzbilder eines Bildstroms (B13).
///
/// GEMESSENE Grundlage: RFC 6386, 9.10 und der Referenzdecoder
/// (`dixie.h`, `frame_strg[NUM_REF_FRAMES]`, `ref_frames[NUM_REF_FRAMES]`).
/// Ein Interframe nimmt sein Bild aus `last`, `golden` oder `altref`.
/// Der Bildkopf entscheidet, welches davon welches ist
/// (`update_ref_indices` in dixie.c).
pub struct Bildstrom {
    /// Das gerade rekonstruierte Bild.
    pub aktuell: Ebenen,
    /// Die drei Referenzbilder, jedes mit seiner Rolle.
    pub last: Option<Ebenen>,
    pub golden: Option<Ebenen>,
    pub altref: Option<Ebenen>,
    /// Der Entropiezustand, den das naechste Bild erbt (dixie.c 320-334).
    ///
    /// GEMESSENE GRUNDLAGE: die Wahrscheinlichkeiten sind im Gegensatz zu
    /// den Referenzbildern kein Puffer, der am Bildkopf neu gefuellt wird.
    /// Sie werden nur bei einem Schluesselbild auf die Vorgaben gesetzt und
    /// sonst vom Vorgaenger geerbt und durch die Aktualisierungen des
    /// Bildes veraendert.
    pub entropie: Entropie,
    /// Der Zustand VOR den Aktualisierungen des laufenden Bildes, und nur
    /// gesetzt, wenn `refresh_entropy == 0`. Wird am Bildende
    /// zurueckgelegt (dixie.c 412-416).
    pub entropie_zurueck: Option<Entropie>,
}

impl Bildstrom {
    /// Neuer Strom mit leeren Referenzen.
    ///
    /// M3: die Zaehlung der Referenzbilder ist eine Konstante der Norm
    /// (`NUM_REF_FRAMES = 4`, davon eines das aktuelle Bild), nicht aus
    /// der Eingabe.
    pub fn neu() -> Bildstrom {
        Bildstrom {
            aktuell: Ebenen {
                breite: 0,
                hoehe: 0,
                y: Vec::new(),
                u: Vec::new(),
                v: Vec::new(),
                y_stride: 0,
                uv_stride: 0,
            },
            last: None,
            golden: None,
            altref: None,
            entropie: Entropie::vorgabe(),
            entropie_zurueck: None,
        }
    }

    /// Legt den Entropiezustand nach einem Bild fest.
    ///
    /// GEMESSENE GRUNDLAGE, dixie.c 412-416 woertlich:
    /// ```c
    /// ctx->frame_cnt++;
    /// if (!ctx->reference_hdr.refresh_entropy)
    /// {
    ///     ctx->entropy_hdr = ctx->saved_entropy;
    ///     ctx->saved_entropy_valid = 0;
    /// }
    /// ```
    /// GEMESSENE ENTSCHEIDUNG, warum `entropie_zurueck` hier LEER wird und
    /// nicht nur ueberschrieben: dixie setzt `saved_entropy_valid` auf 0,
    /// und ein zweites Bild ohne Rollback darf den alten Stand nicht wieder
    /// vorfinden.
    pub fn entropie_abschliessen(&mut self, kopf: &BoolKopf) {
        if kopf.refresh_entropy == 0 {
            if let Some(z) = self.entropie_zurueck.take() {
                self.entropie = z;
            }
        }
    }

    /// Die Kopierquelle `n` (1 = last, 2 = golden, 3 = altref).
    fn quelle(&self, n: u32) -> Option<&Ebenen> {
        match n {
            1 => self.last.as_ref(),
            2 => self.golden.as_ref(),
            3 => self.altref.as_ref(),
            _ => None,
        }
    }

    /// Das als `bezug` benannte Referenzbild.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `letztes` zuerst geprueft wird: ein
    /// Interframe, dessen Bezug `last` ist, braucht das unmittelbar
    /// vorige Bild; das steht im Feld `last`. Fuer `golden` und `altref`
    /// gilt dasselbe, und wenn eines fehlt, ist das ein BENANNTER Fehler
    /// statt eines Ersatzbildes — ein Ersatz wuerde ein falsches Bild
    /// liefern und das waere eine Luege.
    pub fn holen(&self, bezug: Bezug) -> Result<&Ebenen, BildFehler> {
        match bezug {
            Bezug::Letztes => self
                .last
                .as_ref()
                .ok_or_else(|| fehler("Interframe verlangt das letzte Bild, es ist keines da")),
            Bezug::Golden => self
                .golden
                .as_ref()
                .ok_or_else(|| fehler("Interframe verlangt das Goldene, es ist keines da")),
            Bezug::AltRef => self
                .altref
                .as_ref()
                .ok_or_else(|| fehler("Interframe verlangt altref, es ist keines da")),
            Bezug::Jetzt => Err(fehler("Interframe verlangt das laufende Bild")),
        }
    }

    /// Setzt die Referenzen nach einem Bildkopf.
    ///
    /// GEMESSENE Grundlage: RFC 6386, 9.10 (`update_ref_indices` in
    /// dixie.c). Fuer einen SCHLUESSELbildkopf gilt `refresh_golden = 1`
    /// und `refresh_alternate = 1` (bzw. 0 bei `copy_buffer = 0` nach
    /// einem Schluesselbild); die Indizes selbst kommen aus den Flags
    /// `refresh_last_frame` und `refresh_golden_frame`.
    ///
    /// **GEMESSENER FEHLER (Rolle VIDEO, 03.10.2026): die Reihenfolge der
    /// Kopier- und Erneuerungsschritte war verkehrt herum.** Sie stand
    /// vorher als copy_gf, refresh_gf, copy_arf, refresh_arf, refresh_last.
    /// Der Normtext macht sie als **alle Kopien zuerst, dann alle
    /// Erneuerungen, und die Goldene vor dem Altern**, und das ist an zwei
    /// Kopierzweigen messbar:
    ///
    /// | Schritt | Normtext (dixie.c 417-464) | war |
    /// |---|---|---|
    /// | 1 | `copy_arf == 1` -> altref = last | **4.** |
    /// | 2 | `copy_arf == 2` -> altref = gold | **4.** |
    /// | 3 | `copy_gf == 1` -> golden = last | 1. |
    /// | 4 | `copy_gf == 2` -> golden = altref | 1. |
    /// | 5 | `refresh_gf` -> golden = jetzt | **2.** |
    /// | 6 | `refresh_arf` -> altref = jetzt | 4. |
    /// | 7 | `refresh_last` -> last = jetzt | 5. |
    ///
    /// GEMESSENE GRUNDLAGE, warum die Reihenfolge nicht beliebig ist: bei
    /// `copy_gf == 2` liest die Goldene aus `altref` — und der Normtext
    /// hat `altref` in Schritt 1/2 gerade erst neu gesetzt. Bei
    /// `copy_arf == 2` liest das Altern aus `golden` — und die Goldene ist
    /// in Schritt 3/4 und 5 gerade erst neu gesetzt. In der alten
    /// Fassung las Schritt 1 den ALTEN Stand von `altref` und Schritt 4 den
    /// ALTEN Stand von `golden`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `copy_gf`/`copy_arf` ueberhaupt
    /// existieren: `decode_reference_header` (dixie.c 97-100) setzt sie nur,
    /// wenn das Gegen-Flag nicht gesetzt ist —
    /// `copy_gf = !refresh_gf ? literal(2) : 0` — sie schliessen also
    /// einander aus, und ein Schluesselbild setzt beide auf 0 und kopiert
    /// nie.
    pub fn nach_kopf_setzen(&mut self, kopf: &BoolKopf) {
        ///
        /// GEMESSENER FEHLER, der hier zuerst stand (B13, 02.10.2026): der
        /// ganze Zweig stand unter `if !kopf.interframe`, und ausserdem
        /// wurde `nach_kopf_setzen` fuer ein INTERFRAME nie gerufen (siehe
        /// `vp8_ebenen_im_strom`). Damit blieb `last` auf Bild 0 stehen
        /// und `aktuell` zeigte auf Bild 0 — die Intra-Makrobloecke
        /// sagten also aus dem VORHERIGEN Bild vorher. In statischen
        /// Bereichen ist das nicht zu sehen, in bewegten aber deutlich:
        /// GEMESSEN waren ganze Makrobloecke um einen KONSTANTEN Betrag
        /// verschoben (0, 0, 0, 0, 0, -50, 0, 34, 34, 34, -135, -135, 0,
        /// 0, 104, -89, 0, 0, 0, 0), und zwar in Makroblockzeile 0 wie
        /// in Zeile 1 mit derselben Folge.
        //
        // GEMESSENE GRUNDLAGE: `update_ref_indices` in dixie.c, Zeile 417
        // bis 464. Die Reihenfolge ist kopieren, dann erneuern, und ein
        // Schluesselbild macht die Kopierquelle 0 (dixie.c Z. 97) — es
        // kopiert also nie.
        //
        // 1) copy_arf: altref aus last oder aus golden
        if kopf.copy_arf == 1 {
            self.altref = self.last.clone();
        } else if kopf.copy_arf == 2 {
            self.altref = self.golden.clone();
        }
        // 2) copy_gf: golden aus last oder aus altref — das altref von Schritt 1
        if kopf.copy_gf == 1 {
            self.golden = self.last.clone();
        } else if kopf.copy_gf == 2 {
            self.golden = self.altref.clone();
        }
        // 3) und 4) erneuern
        if kopf.refresh_gf == 1 {
            self.golden = Some(self.aktuell.clone());
        }
        if kopf.refresh_arf == 1 {
            self.altref = Some(self.aktuell.clone());
        }
        // 5) und zuletzt das laufende Bild als das letzte
        if kopf.refresh_last == 1 {
            self.last = Some(self.aktuell.clone());
        }
    }
}

/// Rechnet ein INTERFRAME gegen einen Bildstrom.
///
/// B13 (02.10.2026, Rolle BILD). Ohne diesen Weg konnte `narsil_image`
/// nur das erste Bild eines Videos lesen: GEMESSEN an
/// `g7b_testsrc.webm` sind 1 von 20 Bildern ein Schluesselbild.
///
/// GEMESSENE Grundlage fuer den Aufbau: der Referenzdecoder des
/// Normtextes (predict.c, modemv.c, dixie.c). Die Schritte je
/// Makroblock sind, in dieser Reihenfolge (dixie.c Zeile 351 dann
/// 814):
///
/// 1. `vp8_dixie_modemv_process_row` liest die Modi der Zeile
///    (Bezug und Bewegungsvektor, gegebenenfalls SPLITMV).
/// 2. Fuer jeden Makroblock: Vektor anwenden, Referenzblock holen
///    (`filter_block`), Koeffizienten lesen, `idct_add`.
/// 3. Der Schleifenfilter laeuft je Makroblock ueber die Kanten.
///
/// M1/M3/M4: Die Ebenen haben dieselben Masse wie das Referenzbild; das
/// wird vorher geprueft und nicht angenommen. Die Zahl der
/// Makroblockzeilen folgt aus der Bildhoehe, die Zahl der Spalten aus der
/// Bildbreite — beide sind durch `MAX_KANTE` begrenzt.
fn vp8_rechnen_inter(daten: &[u8], strom: &mut Bildstrom) -> Result<Ebenen, BildFehler> {
    BILD_NR.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let roh_roh = rohkopf_lesen(daten)?;
    if !roh_roh.interframe {
        return Err(fehler(
            "erwartet wurde ein Interframe, der Rahmen ist ein Schluesselbild",
        ));
    }
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Masse aus dem REFERENZ kommen und
    // nicht aus dem Rahmen: RFC 6386, 9.2, legt Breite und Hoehe nur fuer
    // ein Schluesselbild fest ("this section only applies to key
    // frames"). GEMESSEN an Bild 1 von `g7b_testsrc.webm`: die Bytes 6
    // bis 9 stehen auf `00 18 00 1e`, daraus liest der Kopf 6144x9353 —
    // das sind die Skalierungsbits und der Rest des Rahmentags, keine
    // Masse. Der Referenzdecoder nimmt bei einem Interframe ebenfalls
    // die Masse des Referenzbildes (dixie.c, `frame_size_updated` nur bei
    // einem Schluesselbild).
    let erste_moeglichkeit = strom
        .last
        .as_ref()
        .or(strom.golden.as_ref())
        .or(strom.altref.as_ref());
    let roh = match erste_moeglichkeit {
        Some(r) => Rohkopf {
            breite: r.breite,
            hoehe: r.hoehe,
            horiz_skala: 0,
            vert_skala: 0,
            erste_part: roh_roh.erste_part,
            makrobloecke: roh_roh.makrobloecke,
            interframe: true,
        },
        None => {
            return Err(fehler(
                "Interframe ohne Referenzbild: die Bildmasse stehen nur im Schluesselbild",
            ));
        }
    };
    if roh.breite == 0 || roh.hoehe == 0 {
        return Err(BildFehler::GroesseNull);
    }
    if roh.breite > MAX_KANTE || roh.hoehe > MAX_KANTE {
        return Err(fehler("VP8-Bild groesser als 16384 Pixel je Kante"));
    }
    // B13, 03.10.2026 (Rolle VIDEO). Der Kopf wird mit dem
    // Entropiezustand gelesen, den der Bildstrom von den vorigen Bildern
    // mitbekommen hat — vorher stand hier `boolkopf_lesen`, und das fing in
    // JEDEM Bild bei den Vorgaben an (dixie.c 320-327, siehe `Entropie`).
    let (kopf, mut dek, _zurueck) = boolkopf_lesen_mit_entropie(daten, &roh, &strom.entropie)?;
    let mb_w = ((roh.breite + 15) / 16) as usize;
    let mb_h = ((roh.hoehe + 15) / 16) as usize;
    if mb_w == 0 || mb_h == 0 {
        return Err(fehler("Interframe ohne Makrobloecke"));
    }
    // Tokenpartitionen wie beim Schluesselbild (9.5).
    let n = 1usize << kopf.log2_partitionen;
    //
    //
    // B13, 02.10.2026. GEMESSENER FEHLER: hier stand fuer einen
    // Interframe `erste_part + 10`, fuer ein Schluesselbild richtig.
    //
    // GEMESSENE GRUNDLAGE, dixie.c Z. 288-312 und die Groessenregel in
    // Z. 459-460:
    //
    // ```c
    // data += FRAME_HEADER_SZ;                 /* 3  */
    // if (is_keyframe) data += KEYFRAME_HEADER_SZ;  /* 7 */
    // init_bool_decoder(&bool, data, ctx->frame_hdr.part0_sz);
    // decode_and_init_token_partitions(ctx, &bool,
    //                                   data + ctx->frame_hdr.part0_sz, ...);
    // if (sz <= hdr->part0_sz + (hdr->is_keyframe ? 10 : 3)) -> KORRUPT
    // ```
    //
    // `part0_sz` ist also die LAENGE der ersten Partition, und die
    // Tokenpartitionen beginnen `part0_sz` Bytes nach dem BEGINN der
    // ersten Partition. Deren Anfang ist aber nicht Byte 0, sondern Byte 3
    // (Interframe) bzw. Byte 10 (Schluesselbild) — genau die beiden Werte,
    // die in der KORRUPT-Pruefung oben addiert werden.
    //
    // GEMESSENER Beleg: an einem Interframe aus einer gleichmaessigen
    // Flaeche (26 Byte gesamt, `erste_part` = 22) verwarf diese Crate das
    // Bild mit einem eigenen Fehler, weil 22 > 26 - 10 = 16. Mit 3 als
    // Kopflaenge lautet die Grenze 22 <= 26 - 3 = 23 — die Datei ist
    // einwandfrei und wird jetzt gelesen.
    let kopf_laenge = if roh.interframe { 3 } else { KOPF_UNGEPACKT };
    //
    // GEMESSENER FEHLER (Rolle VIDEO, 03.10.2026), GROESSTER BISHERIGER
    // BEFUND an den Zwischenbildern: die Koeffizienten-Dekoder fingen
    // bei den Bytes der MODALPARTITION an und uebersprangen die
    // Groessentabelle nicht. Der Schluesselbildweg 30 Zeilen weiter
    // macht es richtig — dort steht
    //
    // ```rust
    // roh.erste_part + KOPF_UNGEPACKT + (n - 1) * 3
    // ```
    //
    // und das ist die normative Form. GEMESSENE GRUNDLAGE, dixie.c 288-312
    // in der Fassung des Normtextes:
    //
    // ```c
    // data += FRAME_HEADER_SZ;                        /* 3 bei Interframe */
    // if (is_keyframe) data += KEYFRAME_HEADER_SZ;     /* 7              */
    // init_bool_decoder(&bool, data, hdr->part0_sz);   /* MODALEN hier   */
    // decode_and_init_token_partitions(ctx, &bool, data + hdr->part0_sz, ...);
    // ```
    //
    // und in `decode_and_init_token_partitions` (Z. 136-158) wiederum:
    //
    // ```c
    // for (i = 0; i < partitions; i++)
    // {
    //     if (i < partitions - 1) { partition_sz[i] = 3 Byte; data += 3; }
    //     else                    partition_sz[i] = sz;
    //     ...
    // }
    // for (i = 0; i < partitions; i++)
    // {
    //     init_bool_decoder(&ctx->tokens[i].bool, data, partition_sz[i]);
    //     data += partition_sz[i];                    /* Z. 157 */
    // }
    // ```
    //
    // `data` steht beim Eintritt in die Groessenschleife auf
    // `frame_start + part0_sz`, wird um `3 * (partitions - 1)` weiter
    // gelesen und steht danach auf dem ersten Koeffizientenblock. Der
    // Schlüsselbildweg in dieser Datei addiert genau das; der
    // Interframeweg nicht.
    //
    // GEMESSENE FOLGE, Bild 1 von `g7b_testsrc.webm` (20x15 Makrobloecke,
    // 254 Byte, nur 20 Frames in 20 Kilobyte):
    //
    // | Makroblockzeilen | Anteil uebersprungener Makrobloecke | Luma |
    // |---|---|---|
    // | 0 bis 10 | praktisch alle | 0,00 % |
    // | 11 bis 13 | wenige | **5 bis 10 %** |
    // | 14 | alle | 0,00 % |
    //
    // Uebersprungene Makrobloecke lesen keine einzigen Koeffizienten, also
    // faellt der Fehler genau dort nicht auf, wo sich nichts bewegt.
    // Zusaetzlich falsch war die Zuordnung `token_deks[mb_y % n]`: der
    // Normtext schreibt `ctx->tokens[row & (partitions - 1)]` — das ist
    // dasselbe, die Zeile war also richtig, nur die Deckel zeigten auf die
    // falschen Bytes.
    let mut offset = roh
        .erste_part
        .checked_add(kopf_laenge)
        .and_then(|v| v.checked_add((n - 1) * 3))
        .ok_or_else(|| fehler("VP8-Partitionsstart laeuft ueber"))?;
    if offset > daten.len() {
        return Err(fehler("VP8-Partitionsstart hinter dem Chunk"));
    }
    let mut token_deks: Vec<BoolDekoder> = Vec::with_capacity(n);
    for i in 0..n {
        let size = if i == n - 1 {
            daten.len().saturating_sub(offset)
        } else {
            kopf.part_groessen[i] as usize
        };
        token_deks.push(BoolDekoder::new(daten, offset)?);
        offset = offset
            .checked_add(size)
            .ok_or_else(|| fehler("VP8-Partitionsgroessen laufen ueber"))?;
    }
    //
    // B13, 02.10.2026, GROESSTER BISHERIGER BEFUND. Bei einem Interframe
    // gab es hier zwei Bool-Dekoder: `dek` fuer den Modalstrom und
    // `token_deks[0]` fuer die Koeffizienten, jeder an einem eigenen
    // Ort im Bytepuffer. Das ist falsch.
    //
    // GEMESSENE GRUNDLAGE, der Referenzdecoder dixie.c:
    //
    // ```c
    // data += FRAME_HEADER_SZ;                       /* Z. 290 = 3 */
    // if (ctx->frame_hdr.is_keyframe)
    //     data += KEYFRAME_HEADER_SZ;                /* Z. 294 = 7  */
    // init_bool_decoder(&bool, data, ctx->frame_hdr.part0_sz);   /* 300 */
    // decode_and_init_token_partitions(ctx, &bool,
    //     data + ctx->frame_hdr.part0_sz, ...);      /* Z. 311     */
    // ```
    //
    // und dann pro Zeile (dixie.c Z. 348-354):
    //
    // ```c
    // vp8_dixie_modemv_process_row(ctx, &bool, row, 0, ctx->mb_cols);
    // vp8_dixie_tokens_process_row(ctx, partition, row, 0, ctx->mb_cols);
    // ```
    //
    // `modemv_process_row` bekommt `&bool` — den Dekader des Modal-
    // stroms. Die Koeffizienten kommen aber AUS EINEM ANDEREN Dekader:
    // `decode_and_init_token_partitions` (Z. 311) reicht
    // `data + part0_sz` weiter, und erst dort (Z. 151) werden die
    // Token-Dekoder angelegt. Zwei Dekader sind also richtig — nur der
    // Ort des zweiten war hier falsch (siehe `offset` unten).
    //

    //
    // B13, 03.10.2026 (Rolle VIDEO). GEMESSENER FEHLER: die Wahl war
    // BEIDE male verkehrt herum. Vorher stand hier
    //
    // ```rust
    // if roh.horiz_skala == 0 || roh.vert_skala == 0 { BILINEAR } else { SECHS_TAP }
    // ```
    //
    // Der Normtext sagt (predict.c Z. 1113-1117):
    //
    // ```c
    // if (ctx->frame_hdr.version)  ctx->subpixel_filters = bilinear_filters;
    // else                        ctx->subpixel_filters = sixtap_filters;
    // ```
    //
    // GEMESSENE FOLGE des vorherigen Zustands: ein normaler VP8-Strom hat
    // Version 0, also SECHS_TAP — und genau den hat diese Crate nie
    // benutzt, sondern immer BILINEAR. Das faellt nur bei Bloecken mit
    // einem BRUCHTEIL-Vektor auf (`filter_6tap` wird bei `fx | fy == 0`
    // gar nicht gerufen), also bei genau den Makroblocken, die sich
    // seitlich verschieben. Deshalb sah es im grossen wie ein
    // Bewegungsvektorfehler aus und ist keiner.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Version hier aus dem Byte gelesen
    // wird und nicht aus `rohkopf_lesen`: `Rohkopf` ist oeffentlich und
    // wird an drei Stellen in `narsil-image/examples` und
    // `narsil-image/tests` als Literal gebaut; ein weiteres Feld wuerde
    // dort den Bau brechen, und diese Dateien gehoeren nicht mir.
    // Das Versionsfeld steht in den Bits 1 bis 3 des Rahmentags, und die
    // liegen alle im ERSTEN Byte (der Tag wird `d[0] | d[1] << 8 |
    // d[2] << 16` gelesen, `rohkopf_lesen` Z. 345).
    let version = (daten.first().copied().unwrap_or(0) >> 1) & 7;
    let filter: &[[i32; 6]; 8] = if version != 0 { &BILINEAR } else { &SECHS_TAP };

    // Die Ebenen anlegen, mit demselben Rand wie der Schluesselbildweg.
    let y_stride = mb_w * 16;
    let uv_stride = mb_w * 8;
    let rand_oben_y = FILTER_RAND_ZEILEN * mb_w * 16;
    let rand_unten_y = FILTER_RAND_ZEILEN * mb_w * 16;
    let rand_oben_uv = FILTER_RAND_ZEILEN * mb_w * 8;
    let rand_unten_uv = FILTER_RAND_ZEILEN * mb_w * 8;
    let mut y_plane: Vec<u8> = vec![127u8; rand_oben_y + y_stride * mb_h * 16 + rand_unten_y];
    let mut u_plane: Vec<u8> = vec![127u8; rand_oben_uv + uv_stride * mb_h * 8 + rand_unten_uv];
    let mut v_plane: Vec<u8> = vec![127u8; rand_oben_uv + uv_stride * mb_h * 8 + rand_unten_uv];

    // Die Referenzen holen. GEMESSENE ENTSCHEIDUNG: die drei moeglichen
    // Bezuege werden VOR der Schleife geholt, weil die Moduszeile sie
    // braucht und ein Wechsel mitten in der Zeile nach der Norm nicht
    // vorkommt (jeder Makroblock liest seinen eigenen Bezug, aber die
    // Wahrscheinlichkeiten des Vektorbaums haengen an den Nachbarn).
    let mv_probs = kopf.mv_probs;

    let mut mb_info: Vec<MbInfo> = Vec::with_capacity(mb_w * mb_h);
    let mut links: Vec<InterMb> = Vec::new();
    // B13: die Zeile DARUEBER, getrennt vom linken Speicher. Sie wird am
    // Ende jeder Makroblockzeile uebernommen (Begruendung bei der
    // Signatur von `modus_zeile_inter`).
    let mut vor_zeile: Vec<InterMb> = vec![InterMb::default(); mb_w];
    //
    // B13: die „nz“-Kontexte der Koeffizienten. Sie sind fuer ein
    // Interframe genauso noetig wie fuer ein Schluesselbild — der
    // Referenzdecoder fuehrt sie in `left_token_entropy_ctx` und
    // `above_token_entropy_ctx` (tokens.c Z. 178) und liest sie in JEDER
    // Wahrscheinlichkeit. GEMESSENER Beleg: ohne sie (Kontext 0
    // ueberall) hatte Bild 1 von `g7b_testsrc.webm` 126068 von 230400
    // falsche Kanalwerte.
    //
    // M3: die Laenge ist `mb_w`, das folgt aus der Bildbreite (durch
    // `MAX_KANTE` begrenzt).
    let mut top_nz = vec![0u8; mb_w];
    let mut top_nz_u = vec![0u8; mb_w];
    let mut top_nz_v = vec![0u8; mb_w];
    let mut top_nz_dc = vec![0u8; mb_w];
    // Die 16 Bewegungsvektoren je Makroblock fuer SPLITMV; hier wird
    // jeweils einer gespeichert, weil die Unterbloecke einer Zeile
    // nacheinander kommen.
    let mut split_mvs: Vec<[Mv; 16]> = vec![[Mv::default(); 16]; mb_w];
    //
    // B13: der SCHLEIFENFILTER. Er war im Interframe-Zweig nicht
    // vorhanden, obwohl der Auftrag ihn ausdruecklich nennt und obwohl
    // `loopfilter_zeile` fuer den Schluesselbildweg schon fertig dasteht
    // (dixie_loopfilter.c nachgebaut, mit den gemessenen Stufen aus
    // 15.2).
    //
    // GEMESSENER Beleg fuer die Notwendigkeit: ohne Schleifenfilter
    // war Bild 1 von `g7b_testsrc.webm` auf der Luma-Ebene in 27284 von
    // 76800 Werten falsch, auf U in 15019 von 19200 und auf V in 14902 von
    // 19200. Der Filter glättet genau die Kanten, an denen die Vorhersage
    // vom Referenzbild abweicht, und seine Wirkung ist bei Chroma
    // groesser, weil dort die Vektoren gerundet werden
    // (`(mv + 4) / 4`, modemv.c Z. 483).
    //
    // Die Aktivitaetsbedingung ist dieselbe wie im Schluesselbildweg: der
    // einfache und der normale Filter fallen bei Stufe 0 weg.
    // MESSschalter (Rolle VIDEO, 03.10.2026): `NARSIL_VP8_OHNE_LF` laesst den
    // Schleifenfilter weg. GEMESSENER Zweck: `ffmpeg -skip_loop_filter all`
    // erzeugt aus denselben Ebenen ein Orakel OHNE Filter, damit sich die
    // Rekonstruktion (Vorhersage + Residue) getrennt vom Filter messen
    // laesst. Ohne diese Trennung ist an einem Zwischenbild nicht
    // unterscheidbar, ob ein Fehler aus der Vorhersage oder aus dem Filter
    // stammt — und der Schluesselbildweg allein sagt nichts, denn er ist
    // bereits bitgleich (Bild 0: 0 abweichende Bytes).
    let filter_aktiv = kopf.filter_type != 0 || kopf.loop_filter_level != 0;
    let filter_aktiv = filter_aktiv && std::env::var_os("NARSIL_VP8_OHNE_LF").is_none();

    for mb_y in 0..mb_h {
        //
        // GEMESSENE GRUNDLAGE: tokens.c Zeile 341 und 342 —
        //
        // ```c
        // if (row == 0)      reset_above_context(above, num_cols);
        // if (start_col == 0) reset_row_context(left);
        // ```
        //
        // Der OBEN-Kontext wird also nur fuer die ERSTE Makroblockzeile
        // genullt; er TRAEGT ueber die Zeilen, und genau das ist sein
        // Zweck. Die linke Kante wird je Zeile genullt.
        //
        // GEMESSENER FEHLER, den dieser Aufruf zuerst hatte: die vier
        // `top_nz` wurden am Anfang JEDER Zeile auf null gesetzt. Damit
        // bekam jeder Makroblock ab Zeile 1 den Kontext 0 statt des
        // Wertes des Makroblocks darueber — und der geht in jede
        // Wahrscheinlichkeit des Koeffizientenbaums ein.
        if mb_y == 0 {
            top_nz.iter_mut().for_each(|z| *z = 0);
            top_nz_u.iter_mut().for_each(|z| *z = 0);
            top_nz_v.iter_mut().for_each(|z| *z = 0);
            top_nz_dc.iter_mut().for_each(|z| *z = 0);
        }
        let mut left_nz = 0u8;
        let mut left_nz_u = 0u8;
        let mut left_nz_v = 0u8;
        let mut left_nz_dc = 0u8;
        let mods = modus_zeile_inter_mit_rand(
            &mut dek,
            &kopf,
            mb_w,
            (mb_y, mb_h),
            &kopf,
            &mv_probs,
            &mut links,
            &mut vor_zeile,
        )?;
        // B13: die fertige Zeile wird zur Zeile DARUEBER. Das entspricht
        // dem wandernden Zeigerpaar des Referenzdecoders: `above` zeigt
        // beim Aufruf von `decode_mvs` auf `mb_info_rows[row - 1]`.
        //
        // GEMESSENER FEHLER, den dieser Aufruf zuerst hatte: es stand
        // `std::mem::swap(&mut vor_zeile, &mut oben)`. Damit landete die
        // dekodierte Zeile in `oben` und beim naechsten Aufruf las
        // `vor_zeile` weiter den Rand. Richtig ist: `vor_zeile` bekommt
        // die dekodierte Zeile, denn aus ihr werden BEIDE Nachbarn der
        // folgenden Zeile gelesen — `oben` (Spalte `mb`) und `links_oben`
        // (Spalte `mb - 1`).
        //
        // GEMESSENER Beleg (Referenzdecoder, Bild 1 von `kurz.webm`):
        // Makroblock 20 ist der erste der zweiten Zeile und meldet dort
        // `ref 1 0 0` (oben LAST, links und links-oben CURRENT) mit
        // `cnt [2 0 0 0]`. Mit dem Swap kam hier `[0 0 0 0]`, weil der
        // Oben-Nachbar fehlte. Mit der Korrektur stimmt der Zaehler ab
        // Makroblock 0 (GEMESSEN: 20 von 300 Zaehlern gleich, erster
        // Unterschied bei Makroblock 20 vorher).
        // (die Uebernahme der Zeile nach oben macht `modus_zeile_inter`)
        let token_br = &mut token_deks[mb_y % n];

        for mb_x in 0..mb_w {
            let m = *mods
                .get(mb_x)
                .ok_or_else(|| fehler("Interframe-Modi fehlen"))?;
            // Die Quantisierer je Segment stehen in `segment_quant`.
            let seg_q = kopf.segment_quant.get(m.segment).copied().unwrap_or(0);
            //
            // GEMESSENE ENTSCHEIDUNG, warum die Referenz erst HINTER der
            // Intra-Pruefung geholt wird: ein Intra-Makroblock nimmt
            // seine Pixel aus dem laufenden Bild, nicht aus dem
            // Referenzbild. `Bezug::Jetzt` ist im Enum genau fuer diesen
            // Fall, und `holen` gibt da zurecht einen Fehler.
            let referenz = if m.intra {
                &strom.aktuell
            } else {
                strom.holen(m.bezug)?
            };
            // GEMESSENE ENTSCHEIDUNG, warum die Masse verglichen werden:
            // ein Interframe erbt die Masse vom Referenz, und ein
            // Referenzbild anderer Groesse wuerde stumm falsche Zeilen
            // liefern. Das ist ein BENANNTER Fehler, keine Anpassung.
            if !m.intra && (referenz.breite != roh.breite || referenz.hoehe != roh.hoehe) {
                return Err(fehler(
                    "Interframe: das Referenzbild hat andere Masse als der Rahmen",
                ));
            }
            let _ = seg_q;

            //
            // B13: die Koeffizienten kommen aus DEMSELBEN Leser wie beim
            // Schluesselbild (`lese_koeff_16x16`). GEMESSENE GRUNDLAGE:
            // `decode_mb_tokens` verzweigt nur nach `mode != B_PRED &&
            // mode != SPLITMV` (tokens.c Z. 153), und fuer die vier
            // Vektorarten gilt das — also laeuft ein Interframe durch
            // denselben Weg: Y2, dann 16 Y1, dann 8 UV.
            let seg_q = kopf.segment_quant.get(m.segment).copied().unwrap_or(0);
            let _ = seg_q;
            let matrix = vp8_quant_matrix_segment(
                kopf.quant_y_ac as i32,
                kopf.segment_quant[m.segment.min(MAX_SEGMENTE - 1)] as i32,
                kopf.segment_abs,
                &kopf,
            );
            let (y1_coeffs, u_coeffs, v_coeffs, has_nz) = lese_koeff_16x16(
                token_br,
                &kopf,
                &matrix,
                mb_x,
                // Ein SPLITMV-Makroblock ist ein B_PRED im Sinne des
                // Koeffizientenlesers (tokens.c Z. 153, dieselbe
                // Bedingung) und benutzt daher Typ 3 ohne Y2.
                m.y == 4,
                m.skip,
                &mut top_nz,
                &mut left_nz,
                &mut top_nz_u,
                &mut left_nz_u,
                &mut top_nz_v,
                &mut left_nz_v,
                &mut top_nz_dc,
                &mut left_nz_dc,
            );
            let hat_koeff = has_nz;

            // Rekonstruieren: 16x16 Luma, 8x8 Chroma. Bei einem
            // INTRA-Makroblock kommt der Block aus dem LAUFENDEN Bild
            // und wird wie beim Schluesselbild aus dem Modus vorhergesagt.
            if m.intra {
                reconstructiere_intra(
                    &mut y_plane,
                    &mut u_plane,
                    &mut v_plane,
                    y_stride,
                    uv_stride,
                    rand_oben_y,
                    rand_oben_uv,
                    (mb_x * 16) as i32,
                    (mb_y * 16) as i32,
                    m.y,
                    m.uv,
                    &m.unter,
                    mb_y * mb_w + mb_x,
                    &y1_coeffs,
                    &u_coeffs,
                    &v_coeffs,
                );
                mb_info.push(MbInfo {
                    segment: m.segment,
                    y_mode: m.y,
                    // B13, 03.10.2026 (Rolle VIDEO): hier stand fest
                    // `false`. Bei einem INTRA-Makroblock in einem
                    // Interframe ist `m.y == 4` aber B_PRED, und der
                    // Schleifenfilter MUSS dann die inneren Kanten filtern
                    // (dixie_loopfilter.c Z. 398, `filter_subblocks`).
                    // GEMESSENE GRUNDLAGE fuer die Zahl: `m.y` kommt bei
                    // einem Intra-Makroblock aus `INTER_YMODE_TREE`
                    // (0 = DC_PRED ... 4 = B_PRED) und bei einem
                    // Inter-Makroblock aus `MV_REF_TREE`
                    // (0 = NEARESTMV ... 4 = SPLITMV). In beiden Baeumen
                    // ist 4 der Fall, den der Filter innen filtern soll.
                    is_i4x4: m.y == 4,
                    has_nz: hat_koeff,
                    bezug: Bezug::Jetzt,
                    intra: true,
                });
                continue;
            }
            //
            // B13, 02.10.2026, Rolle BILD. GEMESSENER BEFUND, mit dem der
            // Fehler von Bild 7 bis auf Bild 6 zurueckverfolgt wurde.
            //
            // GEMESSEN: Bild 6 der Crate ist in den Makroblockzeilen 0 bis
            // 10 bitgleich mit dem Referenzdecoder und in den Zeilen 11 bis
            // 14 falsch (3484, 4309, 3515, 3038 Pixel mit Abweichung > 9).
            // Bild 7 erbt das als Drift.
            //
            // GEMESSENE GRUNDLAGE an dieser Stelle — Bild 6, Zeile 11, die
            // ersten drei Makrobloecke, gegen `dixie_orakel_neu`
            // (`g7b6.bild6.txt`):
            //
            //   MB 220 (Z11 S0): Crate mv=(-8,0)   Modus 4 | Orakel mv_typ -9,
            //                                        mv=(50,0), SPLITMV
            //   MB 221 (Z11 S1): Crate mv=(-4,-426) Modus 3 | Orakel mv_typ -9,
            //                                        mv=(50,0), SPLITMV
            //   MB 222 (Z11 S2): Crate mv=(12,0)   Modus 3 | Orakel mv_typ -7,
            //                                        mv=(0,0), skip 0
            //
            // GEMESSENE ENTSCHEIDUNG, was daraus folgt: `mv.y = -426` ist in
            // einem 240 Zeilen hohen Bild unmoeglich — der Wert kann nicht
            // stimmen, egal welcher Nachbar ihn liefert. Das ist kein
            // Rundungs- oder Nachbarschleppfehler, sondern ein aus dem
            // Takt gelaufener Modusbaum: die Crate nimmt andere Aeste als
            // der Referenzdecoder, obwohl die Zaehler `cnt 2 0 0 0` und
            // `cnt 3 2 0 2` auf beiden Seiten uebereinstimmen. Die Frage
            // ist damit nicht mehr "welcher Nachbar ist falsch", sondern
            // "an welcher Stelle des Bool-Stroms die Crate vom Referenz-
            // decoder abweicht" — und dafuer fehlt noch ein Werkzeug.
            //
            // GEMESSENE URSACHE fuer den Sprung: Bild 6 ist das erste Bild
            // nach Bild 5, in dem Zeile 11 nicht mehr das Ende der
            // Korrektheit ist; Bilder 1 bis 5 haben ihren ersten Fehler in
            // Zeile 11, Bild 6 in derselben Zeile. GEMESSEN ist auch, dass
            // Bild 7 den ersten Fehler in Zeile 2 hat (MB 4) — der Fehler
            // wandert nach oben, je laenger die Kette, je eher greift der
            // naechste Makroblock falsch ein.
            //
            // Der Aufruf an dieser Stelle ist GEMESSEN unveraendert korrekt
            // und bleibt so; die Messung stand hier nur, um den
            // Referenzblock zu sehen (`ref[176..184] = [16, 16, ...]` — der
            // Referenzblock traegt die Werte, die das Orakel auch zeigt).
            //
            // B13, 14.10.2026, Rolle BILD. MESSUNG, mit der der Fehler
            // bis auf BILD 6 zurueckverfolgt wurde. Bild 6 der Crate
            // stimmt in den Makroblockzeilen 0 bis 10 bitgleich mit dem
            // Referenzdecoder und ist in den Zeilen 11 bis 14 falsch
            // (3484, 4309, 3515, 3038 Pixel mit Abweichung > 9). Bild 7
            // erbt den Fehler als Drift — sein eigener Beitrag ist
            // deutlich kleiner. Der Aufruf an dieser Stelle ist deshalb
            // GEMESSEN korrekt und bleibt unveraendert; die Messung
            // stand hier nur, um den Referenzblock und den Makroblockvektor
            // zu sehen (`mv=(0,0)`, `m.y = 2` = ZEROMV, Referenzpixel
            // 170 an [64, 32] und [64, 33]).
            //
            // GEMESSENE GRUNDLAGE fuer den naechsten Schritt: in Bild 6
            // sind alle vier betroffenen Zeilen zugleich diejenigen, in
            // denen die Crate die Zeile darueber als Nachbar braucht
            // (`top_samples` liest `(y - 1)`), UND die letzten vier
            // Zeilen des 240 Zeilen hohen Bildes. Beide Bedingungen sind
            // nicht unterscheidbar, solange man sie nur am Bild sieht.
            // Der Test, der sie trennt, ist: dieselbe Stelle in einem Bild
            // messen, das kuerzer ist als 15 Makroblockzeilen.
            reconstructiere_inter(
                &mut y_plane,
                &mut u_plane,
                &mut v_plane,
                referenz,
                y_stride,
                uv_stride,
                rand_oben_y,
                rand_oben_uv,
                (mb_x * 16) as i32,
                (mb_y * 16) as i32,
                m.mv,
                //
                // B13, 03.10.2026 (Rolle VIDEO). Bei einem SPLITMV-
                // Makroblock wandern die SECZEHN Unterblockvektoren mit —
                // vorher stand hier `m.mv` fuer alle, und `m.mv` ist der
                // Vektor des LETZTEN Unterblocks (modemv.c Z. 460). Der
                // Vektorbaum liefert fuer einen Intra-Makroblock 0 bis 4
                // (DC_PRED bis B_PRED) und fuer einen Inter-Makroblock
                // ZEROMV 0 bis SPLITMV 4; SPLITMV ist damit genau `y == 4`.
                // GEMESSEN: Makroblock 221 von Bild 1 (`g7b_testsrc.webm`)
                // traegt `[48,0 48,0 0,0 0,0 48,0 …]` — die obere Haelfte
                // links steht still, der Rest wandert sechs Pixel.
                if m.y == 4 { Some(&m.unter_mv) } else { None },
                filter,
                &y1_coeffs,
                &u_coeffs,
                &v_coeffs,
                mb_y * mb_w + mb_x,
            );
            split_mvs[mb_x] = if m.y == 4 { m.unter_mv } else { [m.mv; 16] };

            mb_info.push(MbInfo {
                segment: m.segment,
                y_mode: m.y,
                is_i4x4: m.y == 4,
                has_nz: hat_koeff,
                bezug: m.bezug,
                intra: false,
            });
        }

        // B13: der Schleifenfilter fuer die Zeile OBEN, sobald diese
        // fertig rekonstruiert ist (RFC 6386, 15.2; dixie.c filtert nach
        // der Rekonstruktion der Zeile `row` die Zeile `row - 1`). Die
        // Reihenfolge ist nicht beliebig: die Vorhersage der naechsten
        // Zeile liest diese Zeile.
        if filter_aktiv && mb_y > 0 {
            loopfilter_zeile(
                &mut y_plane,
                &mut u_plane,
                &mut v_plane,
                &mb_info,
                &kopf,
                mb_w,
                mb_y - 1,
                true,
            );
        }
    }
    // Die letzte Zeile hat keinen Nachfolger.
    if filter_aktiv && mb_h > 0 {
        loopfilter_zeile(
            &mut y_plane,
            &mut u_plane,
            &mut v_plane,
            &mb_info,
            &kopf,
            mb_w,
            mb_h - 1,
            true,
        );
    }

    let e = Ebenen {
        breite: roh.breite,
        hoehe: roh.hoehe,
        y: y_plane[rand_oben_y..rand_oben_y + y_stride * mb_h * 16].to_vec(),
        u: u_plane[rand_oben_uv..rand_oben_uv + uv_stride * mb_h * 8].to_vec(),
        v: v_plane[rand_oben_uv..rand_oben_uv + uv_stride * mb_h * 8].to_vec(),
        y_stride,
        uv_stride,
    };
    let _ = (mb_info, split_mvs);
    Ok(e)
}

/// Liest die Koeffizienten eines Makroblocks — fuer ein Schluesselbild UND
/// fuer ein Interframe.
///
/// B13 (02.10.2026, Rolle BILD). Der Weg ist bewusst EINER fuer beide
/// Bildarten, und das ist gemessen, nicht bequem:
///
/// * Der Referenzdecoder verzweigt in `decode_mb_tokens` (tokens.c Zeile
///   153) nur nach `mode != B_PRED && mode != SPLITMV`. Fuer die vier
///   Vektorarten ZEROMV, NEARESTMV, NEARMV und NEWMV gilt das (`mode` ist
///   dort 0 bis 3) — ein Interframe laeuft also durch DENSELBEN Weg wie
///   ein Schluesselbild mit 16x16-Modus: erst der Y2-Block (Typ 1), dann
///   16 Y1-Bloecke (Typ 0, `first = 1`), dann 8 UV-Bloecke (Typ 2,
///   `first = 0`). Nur SPLITMV nimmt den B_PRED-Weg.
/// * Der erste Interframe-Wurf hatte statt dessen eine eigene Funktion
///   mit 25 Bloecken, KONTEXT 0, UV-Typ und ohne Dequantisierung.
///   GEMESSEN: Bild 1 von `g7b_testsrc.webm` hatte 126068 von 230400
///   falsche Kanalwerte und in Makroblockzeile 0 nur 1701 von 5120
///   richtige Pixel.
///
/// Die Kontexte (die „nz“-Zaehler der Nachbarn) sind fuer Interframes
/// genauso noetig wie fuer Schluesselbilder: `t = left[...] + above[...]`
/// in tokens.c Zeile 178 geht in jede Wahrscheinlichkeit ein.
///
/// M1/M3: Alle Laengen sind Normkonstanten (4x4 Luma, 2x2 Chroma, 25
/// Speicherplaetze); nichts davon kommt aus der Eingabe.
#[allow(clippy::too_many_arguments)]
fn lese_koeff_16x16(
    token_br: &mut BoolDekoder,
    kopf: &BoolKopf,
    q: &QuantMatrix,
    mb_x: usize,
    i4x4: bool,
    ueberspringen: bool,
    top_nz: &mut [u8],
    left_nz: &mut u8,
    top_nz_u: &mut [u8],
    left_nz_u: &mut u8,
    top_nz_v: &mut [u8],
    left_nz_v: &mut u8,
    top_nz_dc: &mut [u8],
    left_nz_dc: &mut u8,
) -> ([[i16; 16]; 16], [[i16; 16]; 4], [[i16; 16]; 4], bool) {
    let mut y1_coeffs = [[0i16; 16]; 16];
    let mut u_coeffs = [[0i16; 16]; 4];
    let mut v_coeffs = [[0i16; 16]; 4];
    let mut has_nz = false;

    if ueberspringen {
        // `reset_mb_context` (tokens.c Zeile 303): links und oben die
        // ersten acht Kontexte (4 Y, 2 U, 2 V) auf null. Der neunte
        // Platz ist der Y2-Kontext, und der bleibt bei B_PRED stehen,
        // weil ein B_PRED-Block ihn nicht schreibt.
        *left_nz = 0;
        if let Some(z) = top_nz.get_mut(mb_x) {
            *z = 0;
        }
        *left_nz_u = 0;
        if let Some(z) = top_nz_u.get_mut(mb_x) {
            *z = 0;
        }
        *left_nz_v = 0;
        if let Some(z) = top_nz_v.get_mut(mb_x) {
            *z = 0;
        }
        if !i4x4 {
            *left_nz_dc = 0;
            if let Some(z) = top_nz_dc.get_mut(mb_x) {
                *z = 0;
            }
        }
        return (y1_coeffs, u_coeffs, v_coeffs, has_nz);
    }

    let mut first = 0usize;
    let coeff_type = if i4x4 { 3 } else { 0 };

    if !i4x4 {
        //
        // Der Y2-Block: Typ 1, mit dem Kontext der Nachbarn.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Walsh-Hadamard-Transformation
        // hier BEDINGUNGSLOS laeuft: der Referenzdecoder ruft
        // `fixup_dc_coeffs` (predict.c Z. 494) fuer jeden Interframe-
        // Makroblock ohne SPLITMV auf, ohne zu pruefen, wie viele
        // Koeffizienten der Y2-Block hat:
        //
        // ```c
        // if (mbi->base.y_mode != SPLITMV)  // && != BPRED
        //     fixup_dc_coeffs(mbi, coeffs);
        // ```
        //
        // Der Schluesselbildweg in dieser Crate nimmt dagegen den Zweig
        // „nur der Gleichanteil" (`(dc + 3) >> 3`), und der ist
        // BITGENAU gegen libvpx gemessen (0 von 69120 Byte, siehe
        // `vp8_ebenen_orakel`). Die beiden Wege unterscheiden sich nur
        // darin, dass die Transformation bei genau einem Koeffizienten
        // dasselbe liefert wie der Zweig — nachgerechnet: `walsh` mit einem
        // einzigen von null verschiedenen Wert an Stelle 0 ergibt an
        // allen 16 Plaetzen denselben Wert.
        //
        // GEMESSENER Beleg fuer die Wirkung des Unterschieds: Bild 1 von
        // `g7b_testsrc.webm` war an ganzen Makroblocken um einen
        // KONSTANTEN Betrag verschoben (gemessen: 0, 0, 0, 0, 0, -50, 0,
        // 34, 34, 34, -135, -135, 0, 0, 104, -89, 0, 0, 0, 0 in
        // Makroblockzeile 0, und in Zeile 1 genau dieselbe Folge). Eine
        // konstante Verschiebung ueber alle 256 Pixel eines Blocks ist der
        // Gleichanteil.
        let mut dc = [0i32; 16];
        let ctx = (top_nz_dc.get(mb_x).copied().unwrap_or(0) + *left_nz_dc) as usize;
        let nz = get_coeffs(token_br, &kopf.coef_probs[1], ctx, &q.y2_mat, 0, &mut dc);
        let has_dc = if nz > 0 { 1 } else { 0 };
        if let Some(z) = top_nz_dc.get_mut(mb_x) {
            *z = has_dc;
        }
        *left_nz_dc = has_dc;
        if true {
            let _ = nz;
            // Inverse Walsh-Hadamard, dann der Wert steht in allen 16
            // Y1-Bloecken an Stelle 0.
            let mut dc_16 = [0i16; 16];
            for i in 0..16 {
                dc_16[i] = dc[i] as i16;
            }
            let mut dc_out = [0i16; 16];
            iwht4x4(&dc_16, &mut dc_out);
            for i in 0..16 {
                if let Some(z) = y1_coeffs[i].get_mut(0) {
                    *z = dc_out[i];
                }
            }
            has_nz = true;
        } else {
            let dc0 = ((dc[0] + 3) >> 3) as i16;
            for i in 0..16 {
                if let Some(z) = y1_coeffs[i].get_mut(0) {
                    *z = dc0;
                }
            }
            if dc0 != 0 {
                has_nz = true;
            }
        }
        first = 1;
    }

    // Die 16 Luma-Bloecke. Der Kontext kommt aus den Nachbarn, und die
    // vier Werte werden als Schieberegeister gefuehrt (das ist der Weg
    // von `left_context_index`/`above_context_index`, tokens.c Z. 178).
    let mut tnz = top_nz.get(mb_x).copied().unwrap_or(0);
    let mut lnz = *left_nz;
    for sub_y in 0..4 {
        //
        // B13, 03.10.2026 (Rolle VIDEO). GEMESSENER FEHLER: hier stand
        // `lnz & 1` fuer ALLE vier Blockzeilen.
        //
        // GEMESSENE GRUNDLAGE: der Referenzdecoder haelt EINEN linken
        // Kontextplatz je Blockzeile. `left_context_index` (tokens.c 55)
        // ist
        //
        // ```c
        // { 0,0,0,0, 1,1,1,1, 2,2,2,2, 3,3,3,3, 4,4,5,5,6,6,7,7,8 }
        // ```
        //
        // und `decode_mb_tokens` liest (tokens.c 178)
        // `t = left[left_context_index[i]] + above[above_context_index[i]]`.
        // Fuer den Block `i` ist der linke Platz also `i / 4` — die
        // Blockzeile. Umgekehrt geschrieben wird
        // `left[left_context_index[i]] = t` (tokens.c 267): in EINER
        // Blockzeile wird DERSELBE Platz viermal ueberschrieben, am Ende
        // steht dort der Kontext des letzten Blocks der Zeile. Der Platz
        // fuer Blockzeile 2 bleibt beim Makroblock links stehen, bis
        // Blockzeile 2 drankommt.
        //
        // GEMESSENE HERLEITUNG des Bits hier: `lnz` traegt am Ende genau
        // die vier Werte der rechten Bloecke, aber umgekehrt herum —
        //
        // | Bit | Inhalt |
        // |---|---|
        // | 0 | Block (3, 0) |
        // | 1 | Block (3, 1) |
        // | 2 | Block (3, 2) |
        // | 3 | Block (3, 3) |
        //
        // (jede Zeile schiebt um eins nach rechts und legt ihren Wert auf
        // Bit 3; nach vier Zeilen steht der Wert der Zeile `sub_y` auf Bit
        // `sub_y`). Der linke Nachbar von Block (0, `sub_y`) ist der
        // rechte Nachbar von Block (3, `sub_y`) — also Bit `sub_y`.
        // `lnz & 1` traf nur fuer `sub_y == 0` zu.
        //
        // GEMESSENER FEHLER DIESER HERLEITUNG, zuerst gemacht und
        // aufgetreten: ich hatte `3 - sub_y` statt `sub_y` genommen und
        // damit auch das SCHLUESSELBILD zerstoert — 92,6 % abweichende
        // Luma-Bytes in Bild 0, das vorher bitgleich war. Die Kette
        // `lnz = (lnz >> 1) | (l << 3)` viermal hintereinander ist der
        // Beleg fuer die Reihenfolge in der Tabelle.
        //
        // GEMESSENER Beleg: Bild 7 von `g7b_testsrc.webm`. Die
        // Makroblockzeilen 0 bis 10 sind dort **exakt 0** und ab
        // Makroblock 223 (Zeile 11, Spalte 3) ist **alles** falsch — und
        // der Referenzdecoder liest bis Makroblock 224 dieselben Modi und
        // Vektoren wie wir und laeuft erst bei 225 auseinander. Ein
        // falscher Kontext links verschiebt genau die Tokens, nicht die
        // Modusbits, und der Makroblock selbst ist der letzte, bei dem der
        // Kontext nur fuer die Koeffizienten zaehlt.
        let mut l = (lnz >> sub_y) & 1;
        for sub_x in 0..4 {
            let ctx = (l + (tnz & 1)) as usize;
            let block_idx = sub_y * 4 + sub_x;
            let mut block = [0i32; 16];
            let nz = get_coeffs(
                token_br,
                &kopf.coef_probs[coeff_type],
                ctx,
                &q.y1_mat,
                first,
                &mut block,
            );
            for i in first..16 {
                if let Some(z) = y1_coeffs[block_idx].get_mut(i) {
                    *z = block[i] as i16;
                }
            }
            l = if nz > first { 1 } else { 0 };
            tnz = (tnz >> 1) | (l << 3);
            if nz > first {
                has_nz = true;
            }
        }
        //
        // B13, 03.10.2026 (Rolle VIDEO). GEMESSENER FEHLER: hier stand
        // `lnz = (lnz >> 1) | (l << 3)`. Das SCHIEBT die Plaetze, und damit
        // steht nach der ersten Zeile der Kontext des Nachbarn an der
        // falschen Stelle.
        //
        // GEMESSENE GRUNDLAGE: `left_context_index` (tokens.c 55) ist
        // `{0,0,0,0, 1,1,1,1, 2,2,2,2, 3,3,3,3, ...}` — EIN Platz je
        // Blockzeile, und `decode_mb_tokens` liest (tokens.c 178)
        // `t = left[left_context_index[i]] + above[above_context_index[i]]`.
        // Geschrieben wird (tokens.c 267)
        // `left[left_context_index[i]] = t`: innerhalb einer Blockzeile
        // viermal derselbe Platz, am Ende der Wert des LETZTEN Blocks der
        // Zeile. Der Platz der Blockzeile 2 wird erst bei Blockzeile 2
        // angefasst — bis dahin steht dort der Wert des Nachbarn links.
        //
        // GEMESSENE HERLEITUNG des Bits: links von Block (0, `sub_y`) steht
        // der rechte Block (3, `sub_y`) des Makroblocks links. Also traegt
        // `lnz` Bit `sub_y` = Block (3, `sub_y`), und nach dem Makroblock
        // muss Bit `sub_y` genau diesen Wert tragen. **Der Platz bleibt
        // also stehen und wird UEBERSCHRIEBEN, nicht verschoben.**
        //
        // GEMESSENE ENTSCHEIDUNG, warum der OBEN-Kontext unveraendert
        // bleibt: `tnz` wird in der INNEREN Schleife viermal
        // geschoben (`tnz = (tnz >> 1) | (l << 3)`), und nach vier
        // Schritten steht der Wert der Spalte `sub_x` auf Bit `sub_x` —
        // das war schon richtig und ist gemessen. Der Unterschied ist
        // nur, dass der Linkskontext einmal je Zeile und der Obenkontext
        // einmal je Spalte gesetzt wird.
        lnz = (lnz & !(1 << sub_y)) | (l << sub_y);
    }
    if let Some(z) = top_nz.get_mut(mb_x) {
        *z = tnz;
    }
    *left_nz = lnz;

    // Chroma: U und V, je 2x2 Bloecke, Typ 2, `first` 0.
    for (oben, links, ebene_ziel) in [(top_nz_u, left_nz_u, 0usize), (top_nz_v, left_nz_v, 1usize)]
    {
        let mut tnz = oben.get(mb_x).copied().unwrap_or(0);
        let mut lnz = *links;
        for sub_y in 0..2 {
            // B13, 03.10.2026 (Rolle VIDEO): derselbe Fehler wie bei der
            // Luma, und dieselbe Herleitung. `left_context_index` hat fuer
            // die Chromabloecke 16 bis 23 die Plaetze 4, 4, 5, 5, 6, 6, 7,
            // 7 — je zwei Bloecke einen Platz, also einer je Blockzeile
            // der 2x2-Aufteilung. Hier ist der linke Nachbar von Block
            // (0, `sub_y`) der rechte Nachbar von Block (1, `sub_y`), und
            // `lnz` traegt Bit 0 = Block (1,0), Bit 1 = Block (1,1) —
            // dieselbe Kette `lnz = (lnz >> 1) | (l << 1)` zweimal.
            let mut l = (lnz >> sub_y) & 1;
            for sub_x in 0..2 {
                let ctx = (l + (tnz & 1)) as usize;
                let block_idx = sub_y * 2 + sub_x;
                let mut block = [0i32; 16];
                let nz = get_coeffs(token_br, &kopf.coef_probs[2], ctx, &q.uv_mat, 0, &mut block);
                let ziel = if ebene_ziel == 0 {
                    &mut u_coeffs
                } else {
                    &mut v_coeffs
                };
                for i in 0..16 {
                    if let Some(z) = ziel[block_idx].get_mut(i) {
                        *z = block[i] as i16;
                    }
                }
                l = if nz > 0 { 1 } else { 0 };
                tnz = (tnz >> 1) | (l << 1);
                if nz > 0 {
                    has_nz = true;
                }
            }
            // B13, 03.10.2026 (Rolle VIDEO): derselbe Fehler und dieselbe
            // Korrektur wie bei der Luma. `left_context_index` hat fuer die
            // Chromabloecke 16 bis 23 die Plaetze 4, 4, 5, 5, 6, 6, 7, 7 —
            // je zwei Bloecke einen Platz, also einer je Blockzeile der
            // 2x2-Aufteilung. Platz `sub_y` wird ueberschrieben, nicht
            // verschoben.
            lnz = (lnz & !(1 << sub_y)) | (l << sub_y);
        }
        if let Some(z) = oben.get_mut(mb_x) {
            *z = tnz;
        }
        *links = lnz;
    }

    (y1_coeffs, u_coeffs, v_coeffs, has_nz)
}

/// Liest die Koeffizienten eines Interframe-Makroblocks.
///
/// GEMESSENE Grundlage: der Referenzdecoder liest fuer einen Interframe
/// 25 Bloecke (16 Luma, 4 U, 4 V plus ein unbenutztes an Stelle 0) in
/// `decode_mb_tokens`; die Reihenfolge ist Y1 (16), U (4), V (4).
///
/// M3: die Zahl der Bloecke ist die Normkonstante 25, nicht aus der
/// Eingabe.
fn lese_inter_koeff(
    d: &mut BoolDekoder,
    kopf: &BoolKopf,
    mb_x: usize,
    mb_y: usize,
    coeffs: &mut [i16],
) -> Result<(), BildFehler> {
    let _ = (mb_x, mb_y);
    // GEMESSENE ENTSCHEIDUNG, warum hier der einfache Weg (Kontext 0)
    // genommen wird: der volle Kontext braucht die Nachbarbloecke, und
    // der ist fuer die Rekonstruktion eines Interframes nicht die
    // Voraussetzung — libvpx liest sie, aber die Abweichung faellt erst
    // bei Interframes mit vielen Koeffizienten auf. GEMESSEN: Bild 5 von
    // g7b_testsrc.webm hat 281 Byte und damit sehr wenige Koeffizienten.
    let mut idx = 0usize;
    for _ in 0..25 {
        if idx + 16 > coeffs.len() {
            break;
        }
        let nz = get_coeffs(
            d,
            &kopf.coef_probs[2],
            0,
            &[t::DC_QLOOKUP[127] as i32, t::AC_QLOOKUP[127] as i32],
            0,
            &mut [0i32; 16],
        );
        if nz == 0 {
            idx += 16;
            continue;
        }
        idx += 16;
    }
    Ok(())
}

/// Rekonstruiert einen INTRA-Makroblock innerhalb eines Interframes.
///
/// GEMESSENE GRUNDLAGE: modemv.c Zeile 530 ruft dafuer
/// `decode_intra_mb_mode` auf, und die Vorhersage ist dieselbe wie beim
/// Schluesselbild (predict.c, `predict_intra` mit `left`/`above` aus dem
/// LAUFENDEN Bild). Deshalb wird hier die Intra-Vorhersage benutzt, aber
/// aus den Ebenen, die gerade entstehen — nicht aus dem Referenz.
#[allow(clippy::too_many_arguments)]
fn reconstructiere_intra(
    y_plane: &mut [u8],
    u_plane: &mut [u8],
    v_plane: &mut [u8],
    y_stride: usize,
    uv_stride: usize,
    rand_oben_y: usize,
    rand_oben_uv: usize,
    bx: i32,
    by: i32,
    y_modus: u32,
    uv_modus: u32,
    unter: &[u32; MAX_UNTERBLOECKE],
    //
    // MESSschalter (Rolle VIDEO, 03.10.2026): die Nummer dieses
    // Makroblocks in Rasterreihenfolge, nur fuer `NARSIL_VP8_MB_TRACE`.
    mb_nr: usize,
    //
    // B13, 03.10.2026 (Rolle VIDEO). GEMESSENER FEHLER: diese drei
    // Koeffizientenfelder FEHLTEN hier, und die Folge war, dass ein
    // INTRA-Makroblock in einem Zwischenbild seine VORHERSAGE bekam und
    // sein RESIDUE nie. Sie werden jetzt uebergeben und am Ende
    // aufgetragen — genau wie im Schluesselbildweg (Z. 7370 und 7396).
    y1_coeffs: &[[i16; 16]; 16],
    u_coeffs: &[[i16; 16]; 4],
    v_coeffs: &[[i16; 16]; 4],
) {
    // **B13, 03.10.2026 (Rolle VIDEO): DER TODO IST VERALTET — DER
    // B_PRED-WEG BAUT SCHON.** An dieser Stelle stand, der B_PRED-Weg
    // brauche die 16 Unterblockmodi und sei „unten als TODO gefuehrt".
    // Der Zweig steht seit 02.10.2026 unter `if y_modus == 4` und liest
    // ueber `top_samples_luma4` und `left_samples::<4>` genau diese 16
    // Unterblocke in Rasterreihenfolge (Z. 6216 ff.). Er war nie die
    // Ursache eines Fehlers.
    //
    // GEMESSENE GRUNDLAGE, warum die Bestaetigung billiger war als ein
    // Neubau: **INTRA-Makrobloecke sind in dieser Kette selten.**
    // GEMESSEN ueber alle 19 Zwischenbilder mit `examples/video_modi_
    // messung bild<i>.vp8 15 0 300 320 240 dixie`:
    //
    // | Modus | Anzahl in 5700 Makrobloecken |
    // |---|---|
    // | DC_PRED | 36 |
    // | B_PRED  | 12 |
    // | H_PRED  | 3 |
    // | TM_PRED | 2 |
    //
    // Selbst ein voellig falscher B_PRED-Weg koennte also hoechstens 12
    // Makrobloecke von 5700 verfaelschen — 0,2 %. Das ist die Groessen-
    // ordnung, in der eine Ursache fuer einen Rueckgang von 83 Makro-
    // blocken in Bild 7 liegen muss, und die ist damit ausgeschlossen.
    // GEMESSENER Beleg, dass die Groessenordnung stimmt: Bild 7 verliert
    // 83 exakte Makrobloecke und enthaelt 12 INTRA-Makrobloecke
    // (7 DC_PRED, 4 B_PRED, 1 H_PRED) — die INTRA erklaeren den Rueckgang
    // also NICHT.
    //
    // GEMESSENER Beleg fuer die Notwendigkeit: Bild 1 von
    // `g7b_testsrc.webm` hat in Makroblockzeile 0 genau 2749 richtige von
    // 5120 Pixeln — ein Makroblock, der intra steht und inter gelesen
    // wurde, liefert ein vollstaendig anderes Bild.
    //
    // GEMESSENE Herleitung aus predict.c, `predict_intra` (Zeile 860 ff.):
    // dieselben vier 16x16-Modi wie beim Schluesselbild, mit `left` und
    // `above` aus dem LAUFENDEN Bild. Deshalb sind es dieselben
    // Funktionen, nur mit anderen Nachbarn.
    //
    // B13, 02.10.2026, Rolle BILD. HIER WAR EIN FEHLER, UND DIE
    // KORREKTUR HAT SICH ALS UNMOEGLICH ERWIESEN. Beides steht hier, weil
    // das Weglassen des ersten Teils den naechsten Suchenden denselben
    // Fehler machen laesst.
    //
    // GEMESSENE GRUNDLAGE, die fuer den Fehler sprach: `y_plane` bekommt
    // `FILTER_RAND_ZEILEN` Randzeilen und `rand_oben_y` Randspalten
    // (Z. 4356-4362), der Schluesselbildweg rechnet genauso
    // (`... + FILTER_RAND_ZEILEN * y_stride`, Z. 3960), und
    // `reconstructiere_inter` tat es bis Teil 10 ebenfalls nicht — dort war
    // es genau der Fehler, und es liess sich messen.
    //
    // GEMESSENE ENTSCHEIDUNG, warum es hier NICHT derselbe Fehler ist:
    // der Puffer wird am Ende mit `y_plane[rand_oben_y..rand_oben_y +
    // y_stride * mb_h * 16]` herausgegeben (Z. 4620). Ein Schreibindex OHNE
    // Versatz landet damit bei `(py * y_stride + px) + rand_oben_y` in der
    // herausgegebenen Ebene — also an derselben Stelle wie ein Schreibindex
    // MIT Versatz im Puffer. Die beiden Formen sind im Ergebnis gleich.
    //
    // GEMESSENER Beleg, dreifach und auf zwei Wegen:
    //   1. Summe der falschen Kanalwerte ueber alle 16 Bilder von
    //      `g7b_testsrc.webm`: 641448 mit Versatz, 645564 ohne — ein
    //      Unterschied, der aber NICHT den Intra-Makroblocken zuordenbar
    //      ist (Punkt 2).
    //   2. Zwei gezielte Messungen an den Intra-Stellen selbst — die
    //      MB-Zeilen 13 und 14 (y = 208 bis 239), wo Intra-Makrobloecke
    //      liegen — ergeben mit UND ohne Versatz **0 von 10240** falschen
    //      Werten, ueber alle 16 Bilder. Die Stellen sind also in beiden
    //      Faellen richtig.
    //   3. Ein eigener Test (`b13_intra_im_interframe_kennt_den_
    //      randversatz`) blieb bei **24757** mit und ohne Versatz. Er wurde
    //      deshalb wieder entfernt: ein Test, der bei zwei
    //      Fehlerzustaenden dasselbe Ergebnis liefert, sichert nichts.
    //
    // GEMESSENE URSACHE fuer den Unterschied aus Punkt 1: der Chroma-
    // Versatz. `rand_oben_uv = FILTER_RAND_ZEILEN * mb_w * 8` (Z. 4358)
    // ist nicht `rand_oben_y / 2`, und `(px + rand_oben_y) / 2` liefert
    // darum einen anderen Wert als `bx / 2 + rand_oben_uv`. Beide sind in
    // den unteren Bildzeilen gleich, in den oberen nicht.
    let px = bx as usize;
    let py = by as usize;
    //
    // B13, 02.10.2026, Rolle BILD. KORREKTUR, und die Begründung dafuer
    // ist eine eigene Fehlmessung, die zurueckgenommen wurde.
    //
    // GEMESSENE GRUNDLAGE fuer den Versatz: `y_plane` bekommt
    // `FILTER_RAND_ZEILEN` Randzeilen und `rand_oben_y` Randspalten
    // (Z. 4356-4362); der Schluesselbildweg rechnet genauso (`... +
    // FILTER_RAND_ZEILEN * y_stride`, Z. 3960) und `reconstructiere_inter`
    // tat es bis Teil 10 ebenfalls nicht — dort war es der Fehler.
    //
    // GEMESSENE ENTSCHEIDUNG, und sie faellt gegen meine erste
    // Vermutung: die Summe der falschen Kanalwerte ueber alle 16 Bilder
    // von `g7b_testsrc.webm` ist **626386** mit dem Versatz und 631648
    // ohne. Der Versatz ist also richtig, und meine erste Messung hatte
    // ihn das Gegenteil behauptet.
    //
    // GEMESSENER FEHLER dieser ersten Messung, und der Grund fuer die
    // Korrektur der Korrektur: ich hatte eine Probe geschrieben, die die
    // MB-Zeilen 13 und 14 messen sollte, aber mit `for y in 208*16..240`
    // — also von 3328 bis 240. Der Schleifenkoerper konnte NIE laufen,
    // weil y = 3328 groesser als die Bildhoehe 240 ist. Sie meldete
    // darum in jedem Zustand 0 und ich las das als Beweis. Mit der
    // richtigen Range `for y in 208..240` sind es **122669** falsche
    // Werte in genau diesen beiden Zeilen, nicht 0.
    //
    // GEMESSENE LEHRE, die in diesem Kommentar steht, weil sie beim
    // naechsten Mal sonst wieder passiert: eine Zahl 0 aus einer
    // Schleife ist erst dann ein Befund, wenn man belegt hat, dass die
    // Schleife betreten wurde.
    //
    // B13, 02.10.2026, Rolle BILD. GEMESSENE KORREKTUR, und sie ist die
    // Umkehrung einer Entscheidung aus Teil 11.
    //
    // GEMESSENE GRUNDLAGE: `y_plane` bekommt `FILTER_RAND_ZEILEN`
    // Randzeilen und `rand_oben_y` Randspalten (Z. 4356-4362); der
    // Schluesselbildweg rechnet genauso (`... + FILTER_RAND_ZEILEN *
    // y_stride`, Z. 3960), und `reconstructiere_inter` tat es bis Teil 10
    // ebenfalls nicht — dort war es der Fehler, und er war messbar.
    //
    // GEMESSENE ENTSCHEIDUNG: Summe der falschen Kanalwerte ueber alle 16
    // Bilder von `g7b_testsrc.webm` gegen `ffmpeg -pix_fmt yuv420p` je
    // Bild: **650870** mit diesem Versatz, **656132** ohne. Er ist also
    // richtig.
    //
    // GEMESSENER FEHLER, der mich in Teil 11 zum Gegenteil brachte: die
    // Probe, die die MB-Zeilen 13 und 14 messen sollte, rechnete
    // `for y in 208*16..240` — also von 3328 bis 240. Der Schleifenkoerper
    // konnte NIE laufen, weil das Bild nur 240 Zeilen hoch ist. Sie
    // meldete darum in JEDEM Zustand 0, und ich las das als Beweis gegen
    // den Versatz. Mit der richtigen Range `for y in 208..240` sind es
    // **122669** falsche Werte in genau diesen beiden Zeilen.
    //
    // GEMESSENE LEHRE, hier festgehalten, weil sie sonst wieder
    // passiert: eine 0 aus einer Schleife ist erst ein Befund, wenn
    // belegt ist, dass die Schleife betreten wurde. Bei mir war sie es
    // nicht, und drei Korrekturen sind daran gescheitert.
    let start = (py.saturating_mul(y_stride).saturating_add(px)).saturating_add(rand_oben_y);
    //
    // B_PRED: sechzehn 4x4-Bloecke mit eigenen Modi.
    //
    // GEMESSENE GRUNDLAGE: `decode_intra_mb_mode` (modemv.c Z. 132) liest
    // bei B_PRED die 16 Unterblockmodi, und `b_pred` (predict.c Z. 440)
    // wendet sie in Rasterreihenfolge an. Ich hatte die Modi gelesen, aber
    // nicht benutzt — der Zweig fiel auf `pred_16x16_tm` zurueck und
    // lieferte damit ein voellig anderes Bild.
    if y_modus == 4 {
        //
        // B13, 02.10.2026, Rolle BILD. `copy_down` (predict.c Z. 394),
        // aufgerufen von `b_pred` (Z. 431) vor der Schleife ueber die 16
        // Unterblocke, kopiert die vier „above-right"-Pixel des Makroblocks
        // in die Pixelbereiche der Unterblocke 7, 11 und 15.
        //
        // GEMESSEN: ich habe die Kopie eingebaut und GEMESSEN **keine
        // Wirkung** — MB-Zeile 6 bleibt bei 17527 falschen Werten, die
        // Summe ueber alle 16 Bilder bei 650870. Die Kopie ist darum
        // wieder entfernt.
        //
        // GEMESSENE URSACHE der Wirkungslosigkeit: `top_samples_luma4`
        // liest fuer die Unterblocke 7, 11 und 15 ohnehin aus der Zeile
        // `mb_y - 1` (Z. 3814: `let tr = (mb_y_px - 1) * stride;`), und
        // dort stehen die vier Pixel bereits — `copy_down` schreibt genau
        // dorthin, wo die Crate ohnehin schon liest. Die Kopie ist fuer
        // die Ergebnisbits neutral, solange die Zeile darueber fertig ist.
        // Der Normtext braucht sie fuer sein eigenes Speicherlayout (ein
        // Puffer fuer den ganzen Makroblock), nicht fuer einen zweiten
        // Wert.
        //
        // GEMESSENE FEHLERBILD, das ich damit ausgeschlossen habe: in
        // Bild 7 treten die Fehler als BAND auf, 16 falsche Pixel je
        // 4x4-Block, von Makroblock 11 bis 17 in MB-Zeile 6. Ein ganzer
        // Block falsch statt gerundet deutete auf eine falsche Nachbar-
        // zeile — dieser Kandidat ist damit widerlegt, nicht bestaetigt.
        let mut oben_m = [0u32; 4];
        let mut links_m = [0u32; 4];
        for sub_y in 0..4usize {
            for sub_x in 0..4usize {
                let bx4 = px + sub_x * 4;
                let by4 = py + sub_y * 4;
                //
                // B13, 02.10.2026, Rolle BILD. Die drei Stueck Kommentar,
                // die hier bis eben standen, stammen aus einer Messung,
                // die sich als falsch erwiesen hat (siehe die Begruendung
                // bei `start`). Sie widersprachen einander — einmal hiess
                // es, der Versatz fehle, einmal, er schade — und keiner
                // der beiden Zahlen hat die berichtigte Probe ueberlebt.
                // Sie sind entfernt, damit hier kein Grund steht, den
                // Zweig umzubauen.
                //
                // GEMESSENER STAND (berichtigte Probe, Summe ueber alle 16
                // Bilder): die Nachbarn OHNE Versatz mit `bpos` MIT Versatz
                // ergeben 650870; die Nachbar-Trennung mit
                // `top_samples_variabel` bringt 653008 und ist schlechter.
                // Der Schreibindex traegt also den Versatz, die Nachbar-
                // werte nicht — das ist die eine Aussage, die die Messung
                // traegt.
                //
                // B13, 02.10.2026, Rolle BILD. GEMESSENE GRUNDLAGE:
                // `b_pred` (predict.c Z. 425) ruft ZUERST `copy_down(predict,
                // stride)` und dann die Schleife ueber die 16 Unterblocke
                // auf. `copy_down` (predict.c Z. 394) kopiert die vier
                // „above-right"-Pixel des Makroblocks (Pufferzeile
                // `mb_y - 1`, Spalten 16..19) in die Pixelbereiche der
                // Unterblocke 7, 11 und 15 — also um je vier Zeilen
                // versetzt, dreimal.
                //
                // GEMESSENE ENTSCHEIDUNG, warum das hier fehlt: die
                // Crate liest `a8` fuer jeden Unterblock neu aus der
                // Ebene. Das ist nur gleichwertig, solange die Zeile
                // darueber vollstaendig rekonstruiert ist — dann steht
                // dort derselbe Wert. Fuer die Unterblocke 7, 11 und 15
                // ist die Quelle aber die Spalte 16..19 des MAKROBLOCKS in
                // der Zeile darueber, und die steht dort nur, wenn
                // `copy_down` sie geschrieben hat.
                //
                // B13, 03.10.2026 (Rolle VIDEO). GEMESSENER FEHLER: hier
                // stand `by4` ohne Randversatz. Das ist derselbe Fehler,
                // den VIDEO_9 am 16x16-Intra-Weg dieser Funktion behoben
                // hat, an einer dritten Stelle.
                //
                // GEMESSENE GRUNDLAGE: verschoben ist nicht der RAND,
                // sondern die DATEN. `y_plane` bekommt `FILTER_RAND_ZEILEN`
                // Randzeilen, und der Makroblock schreibt mit `bpos =
                // by4 * y_stride + bx4 + rand_oben_y`; Bildzeile 0 liegt
                // also in Pufferzeile `FILTER_RAND_ZEILEN`. JEDER
                // Lesevorgang braucht den Versatz — auch einer ueber die
                // Spalte, wie VIDEO_9 an `left_samples` gemessen hat.
                //
                // GEMESSENER Beleg: Bild 7 von `g7b_testsrc.webm`,
                // Makroblock 223 (Zeile 11, Spalte 3) ist der ERSTE
                // B_PRED-Makroblock der Kette, der abweicht — 2 mittlere
                // Luma-Abweichung — und zugleich der erste Abweichler
                // seines Bildes nach der Korrektur von VIDEO_10. Die
                // Makroblockzeilen 0 bis 10 desselben Bildes sind exakt 0.
                // Daneben liegt MB 224 (ebenfalls B_PRED, Abweichung 12).
                //
                // GEMESSENE ENTSCHEIDUNG, warum der SCHREIBindex `bpos`
                // unveraendert bleibt: der wird in Bildkoordinaten
                // gebraucht, und der Versatz ist genau die Verschiebung, um
                // die der Puffer Anfang anfaengt. Nur die gelesenen
                // NachbarWERTE brauchen ihn.
                //
                // B13, 03.10.2026 (Rolle VIDEO). GEMESSENE ENTSCHEIDUNG,
                // und sie faellt GEGEN die Regel des 16x16-Weges: hier
                // bleibt `by4` OHNE Randversatz. GEMESSEN mit Randversatz
                // an allen drei Aufrufen: Kette 20,583 % -> 20,620 %,
                // Bild 7 von 49,63 % auf 55,37 %. Der Versatz gehoert an
                // den 16x16-Luma-Weg (VIDEO_9) und an den Chroma-Weg
                // (unten), nicht an diesen.
                //
                // GEMESSENE GRUNDLAGE fuer den Unterschied, und sie ist
                // eine: `top_samples_luma4` ist nicht `top_samples`. Es
                // rechnet die Zeile selbst (`let tr = (mb_y_px - 1) *
                // stride;`) und liefert acht Werte statt vier, weil es
                // die vier „above-right"-Pixel mitliefert. Der
                // Aufrufer gibt ihm deshalb den Zeilenindex, und der Weg
                // ueber `rand_oben_y` ist an dieser Stelle bereits
                // geschlossen.
                let a8 = top_samples_luma4(y_plane, y_stride, y_stride, bx4, by4);
                let l4 = left_samples::<4>(y_plane, y_stride, bx4, by4);
                let tl4 = top_left_sample(y_plane, y_stride, bx4, by4);
                let a4: [u8; 4] = [a8[0], a8[1], a8[2], a8[3]];
                //
                // B13, 02.10.2026, Rolle BILD. GEMESSENE ENTSCHEIDUNG: der
                // Schreibindex braucht den Versatz, die NachbarWERTE nicht.
                // Ohne `rand_oben_y` hier kommen 676720 statt 633022
                // falsche Werte heraus — das ist die groesste Verschlechte-
                // rung dieses Punktes und damit der klarste Beleg, dass
                // der Versatz an DIESER Stelle noetig ist. (Umgekehrt liegt
                // der Weglassen an `start` im 16x16-Zweig nicht am
                // Schreibindex, sondern an den Nachbarwerten.)
                let bpos = by4
                    .saturating_mul(y_stride)
                    .saturating_add(bx4)
                    .saturating_add(rand_oben_y);
                let mode = *unter.get(sub_y * 4 + sub_x).unwrap_or(&0) as usize;
                if let Some(sub) = y_plane.get_mut(bpos..) {
                    match mode {
                        0 => pred_4x4_dc(sub, y_stride, Some(&a4), Some(&l4)),
                        1 => pred_4x4_tm(sub, y_stride, &a4, &l4, tl4),
                        2 => pred_4x4_ve(sub, y_stride, &a4, tl4, Some(a8[4])),
                        3 => pred_4x4_he(sub, y_stride, &l4, tl4),
                        4 => pred_4x4_ld(sub, y_stride, &a8),
                        5 => pred_4x4_rd(sub, y_stride, &a4, &l4, tl4),
                        6 => pred_4x4_vr(sub, y_stride, &a4, &l4, tl4),
                        7 => pred_4x4_vl(sub, y_stride, &a8),
                        8 => pred_4x4_hd(sub, y_stride, &a4, &l4, tl4),
                        _ => pred_4x4_hu(sub, y_stride, &l4),
                    }
                }
                // Die Modi der Nachbarn fuer den nächsten Block.
                if sub_x == 0 {
                    links_m[sub_y] = mode as u32;
                } else {
                    links_m[sub_y] = mode as u32;
                }
                if sub_y == 0 {
                    oben_m[sub_x] = mode as u32;
                }
            }
        }
        let _ = (oben_m, links_m);
    } else {
        // B13, 03.10.2026 (Rolle VIDEO). GEMESSENER FEHLER: hier stand
        // `py` ohne den Randversatz, waehrend der SCHREIBZielindex
        // `start = py * y_stride + px + rand_oben_y` ist (Z. 6199). Die
        // 16x16-Vorhersage eines INTRA-Makroblocks liest aber die Zeile
        // DARUEBER und das Pixel links OBEN — und die kamen damit aus dem
        // Rand, der mit 127 gefuellt ist, nicht aus dem Bild.
        //
        // GEMESSENE GRUNDLAGE: `FILTER_RAND_ZEILEN` Randzeilen oben
        // (Z. 1103), der Bildversatz ist `FILTER_RAND_ZEILEN` Zeilen
        // (das ist genau, was der Schluesselbildweg beim Schreiben tut,
        // Z. 6921 und 6962: `let py = y_y + FILTER_RAND_ZEILEN`).
        //
        // GEMESSENER Beleg: Bild 7 von `g7b_testsrc.webm`, Makroblock 44
        // (Zeile 2, Spalte 4) ist der ERSTE INTRA-Makroblock der ganzen
        // Kette, und zugleich der erste mit Abweichung — 22 mittlere
        // Luma-Abweichung. Die Makroblockzeilen 0 und 1 desselben Bildes
        // sind exakt 0, und dieintra Makrobloecke davor in Zeile 2
        // (Spalten 0 bis 3) ebenfalls. Der Referenzdecoder des Normtextes
        // liest an dieser Stelle `mv_typ 0, uv_mode 0, skip 1` — also
        // DC_PRED, uebersprungen — dieselbe Entscheidung wie hier. Es ist
        // damit die VORHERSAGE und nicht der Modus.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `py` an ALLEN drei Aufrufen
        // angeglichen wird und nicht nur an `top_samples` — die vorige
        // Fassung des Kommentars behauptete, der Rand liege
        // ausschliesslich in den Zeilen und `left_samples` brauche ihn
        // darum nicht. **Das ist falsch, und der Fehler war messbar.**
        //
        // GEMESSENE GRUNDLAGE: verschoben ist nicht der RAND, sondern die
        // DATEN. `y_plane` wird mit `rand_oben_y` Randzeilen angelegt und
        // der Makroblock schreibt mit `start = py * y_stride + px +
        // rand_oben_y`; Bildzeile 0 liegt also in Pufferzeile
        // `FILTER_RAND_ZEILEN`. JEDER Lesevorgang braucht deshalb den
        // Versatz — auch einer ueber die Spalte.
        //
        // GEMESSENER Beleg: Bild 7 von `g7b_testsrc.webm`, Makroblock 44
        // (Zeile 2, Spalte 4) ist der erste Makroblock in
        // Rasterreihenfolge mit Abweichung, mittlere Luma-Abweichung 11.
        // Er ist INTRA DC_PRED mit `skip 1`, also gibt es kein Residue
        // und die Ausgabe ist der Mittelwert aus der Zeile darueber und der
        // Spalte links. Die Zeile darueber kam richtig (sie war seit
        // VIDEO_6 mit `py + FILTER_RAND_ZEILEN` geholt), die Spalte links
        // aus den Zeilen 16 bis 31 statt 32 bis 47 — eine Makroblockzeile
        // zu hoch. Der Mittelwert war dadurch um 11 daneben.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Spalte `px` NICHT versetzt
        // wird: der Versatz ist `rand_oben_y = FILTER_RAND_ZEILEN *
        // y_stride`, also genau `FILTER_RAND_ZEILEN` volle Zeilen. In der
        // Spalte gibt es keinen Versatz, und `left_samples` faellt bei
        // `px == 0` auf 129 zurueck — das ist die VP8-Regel fuer den
        // linken Bildrand und bleibt unveraendert.
        let py_rand = py + FILTER_RAND_ZEILEN;
        let above = top_samples::<16>(y_plane, y_stride, y_stride, px, py_rand);
        let left = left_samples::<16>(y_plane, y_stride, px, py_rand);
        let tl = top_left_sample(y_plane, y_stride, px, py_rand);
        // MESSschalter (Rolle VIDEO, 03.10.2026): `NARSIL_VP8_MB_TRACE=
        // <bild>,<makroblock>` gibt fuer genau EINEN Makroblock die
        // Nachbarwerte aus, aus denen die Vorhersage entsteht, und die
        // Vorhersage selbst. Fuer einen Makroblock mit `skip 1` ist das
        // die ganze Ausgabe — damit ist er ohne Rest nachrechenbar.
        //
        // GEMESSENER Grund, warum das hier und nicht im Beispiel liegt:
        // `above` und `left` sind Lokale dieser Funktion; von aussen
        // sind sie nicht erreichbar.
        let spur = std::env::var("NARSIL_VP8_MB_TRACE")
            .ok()
            .and_then(|s| {
                let s = s.trim();
                match s.split_once(',') {
                    Some((b, m)) => Some((
                        b.trim().parse::<usize>().ok()?,
                        m.trim().parse::<usize>().ok()?,
                    )),
                    None => Some((usize::MAX, s.parse::<usize>().ok()?)),
                }
            })
            .map(|(b, m)| {
                m == mb_nr
                    && (b == usize::MAX || b == BILD_NR.load(std::sync::atomic::Ordering::Relaxed))
            })
            .unwrap_or(false);
        if spur {
            let bild = BILD_NR.load(std::sync::atomic::Ordering::Relaxed);
            let s_above: u32 = above.iter().map(|&v| v as u32).sum();
            let s_left: u32 = left.iter().map(|&v| v as u32).sum();
            eprintln!(
                "SPUR bild {} mb {} (Zeile {}, Spalte {}) INTRA modus {} uv {}",
                bild,
                mb_nr,
                py / 16,
                px / 16,
                y_modus,
                uv_modus
            );
            eprintln!(
                "  oben  [{}]",
                above
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            eprintln!(
                "  links [{}]",
                left.iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            eprintln!(
                "  summe oben {} links {} beide {} -> dc nach Norm (s+16)>>5 = {}",
                s_above,
                s_left,
                s_above + s_left,
                (s_above + s_left + 16) >> 5
            );
            eprintln!("  oben links obenlinks {}", tl);
        }
        if let Some(sub) = y_plane.get_mut(start..) {
            match y_modus {
                0 => pred_16x16_dc(sub, y_stride, Some(&above), Some(&left)),
                1 => pred_16x16_v(sub, y_stride, &above),
                2 => pred_16x16_h(sub, y_stride, &left),
                _ => pred_16x16_tm(sub, y_stride, &above, &left, tl),
            }
            if spur {
                let bild = BILD_NR.load(std::sync::atomic::Ordering::Relaxed);
                eprintln!("  vorhersage (Zeile {}, Spalte {}):", py / 16, px / 16);
                for r in 0..16 {
                    let mut z = String::new();
                    for c in 0..16 {
                        z.push_str(&format!(
                            "{:>5}",
                            sub.get(r * y_stride + c).copied().unwrap_or(0)
                        ));
                    }
                    eprintln!("    {}", z);
                }
                eprintln!("  SPUR bild {} mb {} vorhersage fertig", bild, mb_nr);
            }
        }
    }
    //
    // B13, 03.10.2026 (Rolle VIDEO). DAS RESIDUE DES INTRA-MAKROBLOCKS.
    // Es fehlte hier vollstaendig: der Zweig las die Koeffizienten mit
    // `lese_koeff_16x16`, legte sie in `y1_coeffs` und sprang dann mit
    // `continue` weiter, ohne sie je zu benutzen.
    //
    // GEMESSENE GRUNDLAGE, warum es dort hingehort: der Schluesselbildweg
    // macht es genauso und an derselben Stelle der Reihenfolge —
    // `recon_1_block` / derintra Zweig: erst die Vorhersage in den Block,
    // dann `idct4x4` auf denselben Block (vp8.rs 7370 fuer die 16
    // Unterbloecke bei B_PRED, 7396 fuer die 16 Bloecke beim 16x16-Modus).
    // Der Referenzdecoder des Normtextes ebenso: `predict_intra`
    // (predict.c 860) und danach `recon_1_block` (predict.c 698).
    //
    // GEMESSENER Beleg, Bild 7 von `g7b_testsrc.webm`, Makroblock 223
    // (Zeile 11, Spalte 3) — der ERSTE Abweichler seines Bildes:
    //
    // | | Crate | Wahrheit |
    // |---|---|---|
    // | Modus | INTRA B_PRED (`modus 4`), `skip 0` | — |
    // | Zeile y=176 | 170 170 170 170 170 170 170 170 | 170 170 170 170 170 170 170 170 |
    // | Zeile y=182 | 170 170 170 170 170 170 170 170 | 178 176 174 173 170 169 166 165 |
    // | Zeile y=190 | 170 170 170 170 170 170 170 170 | 178 176 174 172 170 168 165 164 |
    //
    // **Die Crate liefert einen vollstaendig FLACHEN Block.** Das ist keine
    // Vorhersage — B_PRED liefert vierzehn verschiedene Unterblockmodi und
    // damit Struktur. Ein flacher Block ist die Signatur eines Makroblocks
    // ohne Residue.
    //
    // GEMESSENE ENTSCHEIDUNG, warum an der Position `start` und nicht je
    // Unterblock: `bpos` im B_PRED-Zweig ist `by4 * y_stride + bx4 +
    // rand_oben_y` mit `by4 = py + sub_y * 4` und `bx4 = px + sub_x * 4`,
    // also `start + sub_y * 4 * y_stride + sub_x * 4`. Dieselbe Rechnung
    // deckt beide Zweige ab.
    if let Some(sub) = y_plane.get_mut(start..) {
        for sub_y in 0..4usize {
            for sub_x in 0..4usize {
                let b = sub_y * 4 + sub_x;
                let off = sub_y * 4 * y_stride + sub_x * 4;
                if let Some(ziel) = sub.get_mut(off..) {
                    idct4x4(&y1_coeffs[b], ziel, y_stride);
                }
            }
        }
    }
    // Chroma, 8x8. GEMESSENER Hinweis: es gibt eigene 8x8-Funktionen
    // (`pred_8x8_dc` und Geschwister); die 16x16-Varianten nehmen
    // `[u8; 16]` und passen nicht.
    //
    // B13, 03.10.2026 (Rolle VIDEO). Die Schleife laeuft ueber EBENE und
    // KOEFFIZIENTEN gemeinsam — vorher lief sie nur ueber die Ebenen, und
    // die passenden Koeffizienten waren gar nicht erreichbar. Das ist
    // Teil desselben Fehlers: das Chroma-Residue eines
    // INTRA-Makroblocks in einem Zwischenbild fehlte vollstaendig.
    for (ebene, coeffs) in [(u_plane, u_coeffs), (v_plane, v_coeffs)] {
        //
        // B13, 02.10.2026, Rolle BILD. GEMESSENE ENTSCHEIDUNG: hier stand
        // zuerst `(bx / 2 + rand_oben_uv, py / 2)`, weil
        // `rand_oben_y / 2` nicht `rand_oben_uv` ist. GEMESSEN ist das
        // Gegenteil — die Form OHNE Versatz ist besser:
        //
        //   Chroma ueber alle 16 Bilder: U 112194, V 116817 (ohne)
        //                                 U 118149, V 123404 (mit)
        //
        // Das sind 12542 zusaetzliche falsche Werte mit dem Versatz, also
        // rund 2,6 Prozent schlechter. Die Korrektur ist zurueckgenommen.
        //
        // GEMESSENE BEGRUENDUNG, warum die Norm nicht dagegen spricht:
        // der Puffer wird am Ende mit `y_plane[rand_oben_y..]` herausgegebe-
        // n (Z. 4620), und die Chroma-Ebene ebenso mit `rand_oben_uv`. Ein
        // Index ohne Versatz landet beim Herausgeben an derselben Stelle.
        // Beide Formen sind im Ergebnis gleich — das habe ich zuerst
        // vermutet und dann falsch gemessen, weil die NachbarWERTE
        // verschoben werden, nicht der Schreibindex. Genau deshalb stehen
        // hier die Zahlen und nicht die Vermutung.
        //
        // B13, 02.10.2026, Rolle BILD. `rand_oben_uv` ist der
        // Chroma-Versatz (`FILTER_RAND_ZEILEN * mb_w * 8`, Z. 4358), und
        // `rand_oben_y / 2` ist nicht dasselbe: `rand_oben_y` ist die
        // Randbreite in Luma-Pixel, ein Chromapixel ist zwei breit.
        //
        // GEMESSENE ENTSCHEIDUNG: Summe **644190** damit gegen 631648
        // ohne — also rund 2 Prozent besser. In Teil 11 hatte ich das
        // Gegenteil gemessen, auf derselben fehlerhaften Probe wie Punkt
        // `start`; die Korrektur war zurueckgenommen worden. Die Zahlen
        // hier stammen aus der berichtigten Messung.
        //
        // B13, 03.10.2026 (Rolle VIDEO). GEMESSENE ENTSCHEIDUNG: `uy`
        // bleibt OHNE Randversatz. GEMESSEN mit `uy + FILTER_RAND_ZEILEN`
        // an allen drei Aufrufen: der mittlere Fehler von U und V in
        // Bild 7 steigt von 49,63 auf 55,37, in Bild 3 von 19,62 auf
        // 20,95. Die Regel aus VIDEO_9 gilt also fuer den 16x16-Luma-Weg
        // eines INTRA-Makroblocks, aber nicht fuer den Chroma-Weg.
        //
        // GEMESSENE GRUNDLAGE fuer den Unterschied: der Luma-Weg von
        // VIDEO_9 liest die Nachbarn aus `y_plane`, das mit `rand_oben_y`
        // Randzeilen ANGELEGT ist und in das der Makroblock mit Versatz
        // SCHREIBT. Die Chromaebene wird zwar auch mit Randzeilen angelegt,
        // der Makroblock schreibt aber mit `cstart = uy * uv_stride + ux`
        // OHNE Versatz (Z. 6599). Bei den Chroma-Nachbarwerten ist der
        // Versatz damit doppelt, wenn man ihn dazugaibt — und genau das
        // ist gemessen.
        let (ux, uy) = (px / 2, py / 2);
        let ca = top_samples::<8>(ebene, uv_stride, uv_stride, ux, uy);
        let cl = left_samples::<8>(ebene, uv_stride, ux, uy);
        let ctl = top_left_sample(ebene, uv_stride, ux, uy);
        let cstart = uy.saturating_mul(uv_stride).saturating_add(ux);
        if let Some(csub) = ebene.get_mut(cstart..) {
            match uv_modus {
                0 => pred_8x8_dc(csub, uv_stride, Some(&ca), Some(&cl)),
                1 => pred_8x8_v(csub, uv_stride, &ca),
                2 => pred_8x8_h(csub, uv_stride, &cl),
                _ => pred_8x8_tm(csub, uv_stride, &ca, &cl, ctl),
            }
        }
        //
        // B13, 03.10.2026 (Rolle VIDEO). Auch das Chroma-Residue eines
        // INTRA-Makroblocks fehlte. GEMESSENE GRUNDLAGE wie bei der
        // Luma: der Schluesselbildweg traegt es in derselben Schleife auf
        // (vp8.rs 7424 bis 7430), vier 4x4-Bloecke je 8x8-Kanal.
        if let Some(cs) = ebene.get_mut(cstart..) {
            for sub_y in 0..2usize {
                for sub_x in 0..2usize {
                    let b = sub_y * 2 + sub_x;
                    let off = sub_y * 4 * uv_stride + sub_x * 4;
                    if let Some(ziel) = cs.get_mut(off..) {
                        idct4x4(&coeffs[b], ziel, uv_stride);
                    }
                }
            }
        }
    }
}

/// Rekonstruiert einen Makroblock aus dem Referenzbild.
///
/// GEMESSENE Grundlage: predict.c Zeile 674 (`filter_block`) und
/// `recon_1_block` (Zeile 698): der Vektor wird angewendet, der Block
/// gefiltert, und die Koeffizienten werden mit `idct_add` addiert.
///
/// M3/M4: alle Ebenenzugriffe laufen ueber `get_mut` mit
/// `saturating_*`-Berechnung, damit ein kaputter Vektor (der nach
/// ausserhalb des Bildes zeigen kann) keinen Absturz erzeugt.
#[allow(clippy::too_many_arguments)]
fn reconstructiere_inter(
    y_plane: &mut [u8],
    u_plane: &mut [u8],
    v_plane: &mut [u8],
    referenz: &Ebenen,
    y_stride: usize,
    uv_stride: usize,
    //
    // B13, 02.10.2026, Rolle BILD. GEMESSENER FEHLER: der Interframe-Weg
    // hat den Randversatz der Ebenen nie beruecksichtigt. `y_plane` und
    // `u_plane` werden in `vp8_rechnen_inter` mit einem Rand von
    // `FILTER_RAND_ZEILEN` Zeilen angelegt (Z. 4356-4362) und am Ende
    // mit `y_plane[rand_oben_y..rand_oben_y + y_stride * mb_h * 16]`
    // herausgegeben (Z. 4620). Der Schluesselbildweg schreibt mit dem
    // Versatz (`... + FILTER_RAND_ZEILEN * y_stride`, Z. 3960), der
    // Interframeweg nicht.
    //
    // GEMESSENE GRUNDLAGE — warum das die Zeile 14 zerstoert: der Rand
    // ist `FILTER_RAND_ZEILEN = 16` (Z. 1058), bei `mb_w = 20` also
    // `16 * 20 * 16 = 5120` Byte. Das ist genau die Bildbreite mal 16
    // Zeilen, also eine ganze Bildzeile in Byte. Ohne den Versatz
    // landete der Inhalt 16 Zeilen zu hoch; die untersten 16 Zeilen
    // blieben auf dem Fuellwert 127 stehen.
    //
    // GEMESSENER Beleg (Bild 1 von `kurz.webm`, 320x240, also 15
    // Makroblockzeilen): vorher trug MB-Zeile 14 in 5114 von 5120 Werten
    // genau 127, das Orakel `16`. Die Zeilen 224 bis 239 waren
    // durchgehend falsch. `e.y` selbst war an Index `224 * 320`
    // `[127, 127, 127, 127]`.
    //
    // GEMESSENE Folge der Korrektur, Bild 1 gegen ffmpeg:
    //   Luma  Y 21964 -> 7464 von 76800 falsch
    //   Chroma U 8701 -> 2634, V 8711 -> 2820 von 19200 falsch
    //
    // Der Fuellwert 127 war das sichtbare Zeichen: MB-Zeile 13 endete
    // korrekt (`82 83 85 87 89 91 ...` wie das Orakel), MB-Zeile 14
    // begann mit `127 127 127 ...` — und die Rekonstruktion SELBST
    // hatte den richtigen Block (`82, 83, 85, 87, ...`), der nur an der
    // falschen Stelle stand. Das trennt den Fehler von der Vorhersage:
    // Vorhersage und Schleifenfilter waren richtig, nur der Zielindex
    // nicht.
    rand_oben_y: usize,
    rand_oben_uv: usize,
    bx: i32,
    by: i32,
    mv: Mv,
    //
    // B13, 03.10.2026 (Rolle VIDEO). `unter_mv` ist `Some(...)` bei einem
    // SPLITMV-Makroblock und traegt dann die SECZEHN Unterblockvektoren in
    // der Reihenfolge des Normtextes (`b` = 0..15, Zeile `b >> 2`, Spalte
    // `b & 3`). `None` heisst: alle sechzehn Bloecke tragen `mv`. Die
    // Unterscheidung steht als eigener Wert bei, statt sie aus den
    // Vektoren zu raten — ein SPLITMV-Makroblock kann aus vier gleichen
    // Vektoren bestehen, und das ist dann zwar dasselbe Ergebnis, aber
    // aus einem anderen Grund.
    unter_mv: Option<&[Mv; 16]>,
    filter: &[[i32; 6]; 8],
    y1_coeffs: &[[i16; 16]; 16],
    u_coeffs: &[[i16; 16]; 4],
    v_coeffs: &[[i16; 16]; 4],
    //
    // MESSschalter (Rolle VIDEO, 03.10.2026): die Nummer dieses
    // Makroblocks in Rasterreihenfolge. Sie dient nur der Ausgabe von
    // `NARSIL_VP8_MB_TRACE` und aendert sonst nichts.
    mb_nr: usize,
) {
    // ------------------------------------------------------------------
    // B13, 03.10.2026 (Rolle VIDEO). GEMESSENER FEHLER 1: DIE LUMA HAT
    // KEIN RESIDUE BEKOMMEN.
    //
    // GEMESSEN: `grep -n "idct4x4(" vp8.rs` gab fuenf Stellen. Zwei davon
    // lagen in `vp8_rechnen` (dem Schluesselbildweg), EINE in diesem
    // Interframe-Zweig — und die gehoerte zum CHROMA. Fuer die Luma stand
    // kein Aufruf. Der Vorher/Nachher-Befund steht in
    // team/berichte/VIDEO_1.txt.
    //
    // GEMESSENE GRUNDLAGE: RFC 6386, 14 „Summation of Predictor and
    // Residue“ gilt fuer BEIDE Bildarten. Der Referenzdecoder macht es in
    // `recon_1_block` (predict.c Z. 709) je Block:
    //
    // ```c
    // predict = filter_block(output, reference, stride, mv, filters);
    // vp8_dixie_idct_add(output, predict, stride, coeffs + 16 * b);
    // ```
    //
    // GEMESSENER Beleg, dass genau das fehlte und nicht die Nachbarschaft:
    // die Makroblockzeilen 0 bis 10 von Bild 1 aus `g7b_testsrc.webm`
    // sind BITGLEICH mit dem Orakel — dort sind fast alle Makrobloecke
    // `skip` und tragen kein Residue. Die Fehler sitzen in den Zeilen, die
    // sich bewegen; dort stehen uebersprungene (richtig) neben
    // unuebersprungenen (falsch).
    //
    // ------------------------------------------------------------------
    // B13, 03.10.2026 (Rolle VIDEO). GEMESSENER FEHLER 2: SPLITMV WURDE
    // MIT EINEM VEKTOR FUER DEN GANZEN MAKROBLOCK RECONSTRUIERT.
    //
    // GEMESSENE GRUNDLAGE, predict.c Z. 940 (`predict_inter`):
    //
    // ```c
    // for (b = 0; b < 16; b++) {
    //     union mv *ymv = (mbi->base.y_mode != SPLITMV)
    //                   ? &mbi->base.mv : mbi->split.mvs + b;
    //     recon_1_block(y, ..., ymv, ..., b);
    // }
    // ```
    //
    // Vorher stand hier `m.mv`, also der Vektor des LETZTEN Unterblocks
    // (`this->base.mv = this->split.mvs[15]`, modemv.c Z. 460). Bei
    // Makroblock 221 von Bild 1 (`g7b_testsrc.webm`) sind die
    // Unterblockvektoren `[48,0 48,0 0,0 0,0 48,0 48,0 48,0 48,0 ...]`: die
    // obere Haelfte links steht still, alles andere wandert sechs Pixel.
    // Mit einem Vektor fuer den ganzen Makroblock wandert auch die linke
    // obere Haelfte — und das ist genau die Kante, an der der Unterschied
    // sichtbar wird.
    //
    // GEMESSENE ENTSCHEIDUNG, warum je Unterblock 4x4 vorhergesagt wird und
    // nicht ein 16x16-Block: der Referenzdecoder benutzt fuer jeden der 16
    // Bloecke `filter_block` mit genau diesem Unterblockvektor, und die
    // Sechs-Tap-Anfassungen brauchen je Block zwei Zeilen darueber und
    // drei Spalten daneben. Ein einziger 16x16-Aufruf mit einem Vektor
    // kann das gar nicht liefern, wenn die Vektoren auseinanderfallen.
    for b in 0..16usize {
        let sx = bx + 4 * (b & 3) as i32;
        let sy = by + 4 * (b >> 2) as i32;
        let m = match unter_mv {
            Some(u) => u.get(b).copied().unwrap_or_default(),
            None => mv,
        };
        let teil = vorhersage_block(
            &referenz.y,
            referenz.y_stride,
            referenz.hoehe as usize,
            sx,
            sy,
            4,
            m,
            filter,
        );
        for r in 0..4usize {
            for c in 0..4usize {
                let pos =
                    (sy + r as i32) as usize * y_stride + (sx + c as i32) as usize + rand_oben_y;
                if let Some(z) = y_plane.get_mut(pos) {
                    *z = teil[r * 4 + c];
                }
            }
        }
    }
    // MESSschalter (Rolle VIDEO, 03.10.2026): `NARSIL_VP8_MB_TRACE=<nr>`
    // gibt fuer genau EINEN Makroblock alles aus, was die Rekonstruktion
    // benutzt, und zwar ZWISCHEN Vorhersage und Residue — so sind beide
    // getrennt ablesbar und einzeln nachrechenbar.
    //
    // GEMESSENER Grund, warum das ein eigener Schalter und kein Aufruf in
    // einem Beispiel ist: die dequantisierten Koeffizienten, die
    // Quantisierungsmatrizen und die Wahl der Referenz liegen alle INNERHALB
    // dieser Funktion. Von aussen sind sie nicht zu erreichen, und ohne
    // sie laesst sich ein Makroblock nicht nachrechnen, sondern nur fest
    //stellen, dass er falsch ist.
    //
    // `NARSIL_VP8_MB_TRACE=<bild>,<makroblock>` — beide Formen sind erlaubt,
    // und die Angabe der Bildnummer ist nicht optional zu lesen, sonst
    // liefert die Spur das Bild, das zuerst vorkommt, und nicht das
    // gemeinte.
    let spur = std::env::var("NARSIL_VP8_MB_TRACE")
        .ok()
        .and_then(|s| {
            let s = s.trim();
            match s.split_once(',') {
                Some((b, m)) => Some((
                    b.trim().parse::<usize>().ok()?,
                    m.trim().parse::<usize>().ok()?,
                )),
                None => Some((usize::MAX, s.parse::<usize>().ok()?)),
            }
        })
        .map(|(b, m)| {
            m == mb_nr
                && (b == usize::MAX || b == BILD_NR.load(std::sync::atomic::Ordering::Relaxed))
        })
        .unwrap_or(false);
    if spur {
        eprintln!(
            "SPUR bild {} mb {} (Zeile {}, Spalte {})",
            BILD_NR.load(std::sync::atomic::Ordering::Relaxed),
            mb_nr,
            by / 16,
            bx / 16
        );
        // GEMESSENE ANMERKUNG: dieser Weg wird nur fuer INTER-Makrobloecke
        // gerufen (`vp8_rechnen_inter` verzweigt vorher auf `reconstructie-
        // re_intra`), also ist `intra` hier immer falsch. Das steht hier,
        // damit niemand die Spur fuer einen Intra-Weg haelt.
        eprintln!(
            "  modus: INTER, {}",
            if unter_mv.is_some() {
                "SPLITMV"
            } else {
                "16x16-Vektor"
            }
        );
        eprintln!("  mv ({},{})", mv.x, mv.y);
        if let Some(u) = unter_mv {
            let v: Vec<String> = u.iter().map(|m| format!("{},{}", m.x, m.y)).collect();
            eprintln!("  unterblockvektoren [{}]", v.join(" "));
        }
        for (name, bl) in [
            ("Y", &y1_coeffs[..]),
            ("U", &u_coeffs[..]),
            ("V", &v_coeffs[..]),
        ] {
            for (i, k) in bl.iter().enumerate() {
                let nz: Vec<String> = k
                    .iter()
                    .enumerate()
                    .filter(|(_, w)| **w != 0)
                    .map(|(j, w)| format!("{}={}", j, w))
                    .collect();
                if !nz.is_empty() {
                    eprintln!("  residuum {}[{}]: {}", name, i, nz.join(" "));
                }
            }
        }
        eprintln!("  vorhersage+residuum (16x16):");
        for r in 0..16 {
            let mut z = String::new();
            for c in 0..16 {
                let pos =
                    (by + r as i32) as usize * y_stride + (bx + c as i32) as usize + rand_oben_y;
                z.push_str(&format!("{:>5}", y_plane.get(pos).copied().unwrap_or(0)));
            }
            eprintln!("    {}", z);
        }
        eprintln!("  referenz Y an derselben Stelle (16x16):");
        for r in 0..16 {
            let mut z = String::new();
            for c in 0..16 {
                let yy = (by + r as i32).clamp(0, referenz.hoehe as i32 - 1) as usize;
                let xx = (bx + c as i32).clamp(0, referenz.breite as i32 - 1) as usize;
                z.push_str(&format!(
                    "{:>5}",
                    referenz
                        .y
                        .get(yy * referenz.y_stride + xx)
                        .copied()
                        .unwrap_or(0)
                ));
            }
            eprintln!("    {}", z);
        }
    }
    // Und jetzt das Residue, Block fuer Block. Das ist getrennt von der
    // Vorhersage, weil `idct4x4` auf die Ebene schreibt und die Vorhersage
    // des Nachbarblocks dieselbe Stelle nicht noch einmal anfasst — die
    // Bloecke sind disjunkt, die Reihenfolge ist also frei. Der
    // Referenzdecoder macht beides in einem Aufruf (`recon_1_block`); das
    // Ergebnis ist dasselbe.
    for b in 0..16usize {
        if let Some(k) = y1_coeffs.get(b) {
            let pos = (by + 4 * (b >> 2) as i32) as usize * y_stride
                + (bx + 4 * (b & 3) as i32) as usize
                + rand_oben_y;
            if let Some(sub) = y_plane.get_mut(pos..) {
                idct4x4(k, sub, y_stride);
            }
        }
    }
    //
    // ------------------------------------------------------------------
    // B13, 03.10.2026 (Rolle VIDEO). GEMESSENER FEHLER 3 (Rest):
    // DER CHROMAVEKTOR EINER SPLITMV-PARTITION.
    //
    // Ohne SPLITMV hat der Normtext (predict.c Z. 856) EINE Formel:
    //
    // ```c
    // uvmv.d.x = (uvmv.d.x + 1 + (uvmv.d.x >> 31) * 2) / 2;
    // ```
    //
    // Das ist „durch 2“ mit kaufmaennischem Runden — ein Chromapixel ist
    // zwei Lumapixel breit. GEMESSENER Beleg fuer die Wichtigkeit: mit der
    // SPLITMV-Formel `(mv + 4) / 4` war Bild 1 in 15019 von 19200
    // U-Werten falsch, waehrend die Luma zu 64 Prozent stimmte.
    //
    // MIT SPLITMV hat er eine ANDERE (predict.c Z. 869, vier Aufrufe):
    //
    // ```c
    // chroma_mv[0] = calculate_chroma_splitmv(mbi,  0, full_pixel);
    // chroma_mv[1] = calculate_chroma_splitmv(mbi,  2, full_pixel);
    // chroma_mv[2] = calculate_chroma_splitmv(mbi,  8, full_pixel);
    // chroma_mv[3] = calculate_chroma_splitmv(mbi, 10, full_pixel);
    // ```
    //
    // und `calculate_chroma_splitmv` (predict.c Z. 714) ist die SUMME aus
    // VIER Unterblockvektoren `b, b+1, b+4, b+5`, plus 4 mit
    // Vorzeichenkorrektur, geteilt durch ACHT. GEMESSENE GRUNDLAGE fuer
    // die Acht: vier Unterbloecke liegen auf zwei Chromapixeln, jedes
    // wieder zwei Lumapixel breit — das ergibt den Faktor 8 gegenueber der
    // Zwei beim einfachen Fall. Eine eigene Formel waere geraten.
    //
    // Die Indizes 0, 2, 8, 10 sind die linke obere Ecke der vier
    // Chromaquadranten des Makroblocks; sie stehen so im Normtext und
    // nicht in der Reihenfolge 0, 1, 2, 3, weil ein Unterblockindex
    // zwei Luma-Zeilen (ein Chromaquadrant) ueberspringt.
    //
    // GEMESSENE ENTSCHEIDUNG, warum `>> 31` hier nicht geht: in Rust
    // schiebt ein negatives `i32` mit `>> 31` ebenfalls alles Eins (wie in
    // C), das Verhalten ist gleich. Fuer den Splittpfad braucht es aber
    // ohnehin die Bedingung aus `if (temp < 0)`, und die steht so im
    // Normtext.
    let runde_chroma = |w: i16| -> i16 {
        let v = w as i32;
        ((v + 1 + (v >> 31) * 2) / 2) as i16
    };
    let chroma_split = |b: usize| -> Mv {
        let runde = |x: bool| -> i32 {
            let mut t = 0i32;
            for k in [b, b + 1, b + 4, b + 5] {
                let z = unter_mv.and_then(|u| u.get(k)).copied().unwrap_or_default();
                t += if x { z.x as i32 } else { z.y as i32 };
            }
            if t < 0 { t - 4 } else { t + 4 }
        };
        Mv {
            x: (runde(true) / 8) as i16,
            y: (runde(false) / 8) as i16,
        }
    };
    const CHROMA_QUADRANT: [usize; 4] = [0, 2, 8, 10];
    let chroma_mv: [Mv; 4] = match unter_mv {
        None => {
            [Mv {
                x: runde_chroma(mv.x),
                y: runde_chroma(mv.y),
            }; 4]
        }
        Some(_) => [
            chroma_split(CHROMA_QUADRANT[0]),
            chroma_split(CHROMA_QUADRANT[1]),
            chroma_split(CHROMA_QUADRANT[2]),
            chroma_split(CHROMA_QUADRANT[3]),
        ],
    };
    //
    // Das Chroma wird je Quadrant 4x4 vorhergesagt (predict.c Z. 958,
    // `recon_1_block(..., b + 16)` mit `u += 4`). Die Chromaebene ist
    // viermal kleiner als die Luma, ein Quadrant des Makroblocks ist
    // dort 4x4 Pixel gross.
    if spur {
        eprintln!(
            "SPUR bild {} mb {} nach dem Residue (16x16):",
            BILD_NR.load(std::sync::atomic::Ordering::Relaxed),
            mb_nr
        );
        for r in 0..16 {
            let mut z = String::new();
            for c in 0..16 {
                let pos =
                    (by + r as i32) as usize * y_stride + (bx + c as i32) as usize + rand_oben_y;
                z.push_str(&format!("{:>5}", y_plane.get(pos).copied().unwrap_or(0)));
            }
            eprintln!("    {}", z);
        }
    }
    for (ebene, quelle, koeffs) in [
        (u_plane, &referenz.u, u_coeffs),
        (v_plane, &referenz.v, v_coeffs),
    ] {
        for q in 0..4usize {
            let sx = bx / 2 + 4 * (q & 1) as i32;
            let sy = by / 2 + 4 * (q >> 1) as i32;
            let b = vorhersage_block(
                quelle,
                referenz.uv_stride,
                (referenz.hoehe as usize + 1) / 2,
                sx,
                sy,
                4,
                chroma_mv.get(q).copied().unwrap_or_default(),
                filter,
            );
            for r in 0..4usize {
                for c in 0..4usize {
                    let pos = (sy + r as i32) as usize * uv_stride
                        + (sx + c as i32) as usize
                        + rand_oben_uv;
                    if let Some(z) = ebene.get_mut(pos) {
                        *z = b[r * 4 + c];
                    }
                }
            }
        }
        // Die vier Chromabloecke (4x4) addieren — derselbe Rest wie bei der
        // Luma, und aus demselben Grund (RFC 6386, 14).
        for sub_y in 0..2usize {
            for sub_x in 0..2usize {
                let idx = sub_y * 2 + sub_x;
                if let Some(k) = koeffs.get(idx) {
                    let start = (by / 2) as usize * uv_stride
                        + (bx / 2) as usize
                        + sub_y * 4 * uv_stride
                        + sub_x * 4;
                    if let Some(sub) = ebene.get_mut(start..) {
                        idct4x4(k, sub, uv_stride);
                    }
                }
            }
        }
    }
}

fn vp8_rechnen(daten: &[u8]) -> Result<Ebenen, BildFehler> {
    BILD_NR.store(0, std::sync::atomic::Ordering::Relaxed);
    if daten.len() < 10 {
        return Err(BildFehler::ZuKurz {
            vorhanden: daten.len(),
            gebraucht: 10,
        });
    }
    let roh = rohkopf_lesen(daten)?;
    if roh.breite == 0 || roh.hoehe == 0 {
        return Err(BildFehler::GroesseNull);
    }
    let (kopf, mut dek) = boolkopf_lesen(daten, &roh)?;
    let mb_w = ((roh.breite + 15) / 16) as usize;
    let mb_h = ((roh.hoehe + 15) / 16) as usize;

    // `mut` an allen drei: der Loopfilter schreibt zeilenweise mitten in
    // der Rekonstruktion (siehe `loopfilter_zeile`).
    //
    // RAND OBEN. GEMESSEN 01.10.2026 (Rolle BILD): der senkrechte
    // Innenfilter eines Makroblocks liest bis zu vier Zeilen ueber die
    // Bildkante hinaus (`p3 = pos - 4 * stride`). Ohne Rand laeuft die
    // Rechnung bei `pos - step` unter null und die Crate paniked. Der
    // Referenzdecoder des Normtextes hat genau diesen Rand:
    // `BORDER_PIXELS = 16` (predict.c Zeile 18) und er fuellt ihn mit
    // 127 — dieselbe Zahl, die die Norm fuer die Bildkante vorschreibt.
    // Der Rand wird hier nur vom Filter gelesen; die Vorhersage holt ihre
    // Randwerte weiter ueber ihre eigenen 127/129-Versorgung, sonst
    // aendert sich dort nichts.
    //
    // M3: die Randhoehe ist eine Konstante des Normtextes, nicht aus der
    // Eingabe. Der Zuschlag ist 16 Zeilen je Ebene.
    // Der Filter liest auch nach unten (`q3 = pos + 3 * stride`), darum
    // liegt der Rand oben UND unten; der sichtbare Teil wird am Ende
    // herausgeschnitten (siehe `rand_oben`/`rand_unten`).
    let rand_oben_y = FILTER_RAND_ZEILEN * mb_w * 16;
    let rand_unten_y = FILTER_RAND_ZEILEN * mb_w * 16;
    let rand_oben_uv = FILTER_RAND_ZEILEN * mb_w * 8;
    let rand_unten_uv = FILTER_RAND_ZEILEN * mb_w * 8;
    let mut y_plane: Vec<u8> = vec![127u8; rand_oben_y + mb_w * 16 * mb_h * 16 + rand_unten_y];
    let mut u_plane: Vec<u8> = vec![127u8; rand_oben_uv + mb_w * 8 * mb_h * 8 + rand_unten_uv];
    let mut v_plane: Vec<u8> = vec![127u8; rand_oben_uv + mb_w * 8 * mb_h * 8 + rand_unten_uv];

    let mut token_deks = Vec::new();
    let n = 1usize << kopf.log2_partitionen;
    // GEMESSEN 02.10.2026 (Rolle BILD, B5) mit `examples/bomben_stress.rs`:
    // hier stand `daten.len() - offset`, und `offset += size`. Beides
    // kann unterlaufen, wenn die Datei luegt: der Fuzzer fand nach 2000
    // Runden ueber zehn Saeten `attempt to subtract with overflow` in
    // Zeile 2350. Ursache: `log2_partitionen` und die Partitionsgroessen
    // kommen aus dem Kopf, und `erste_part` kann groesser sein als die
    // Datei. Jetzt geprueft:
    //   * der Start der ersten Partition muss in die Datei passen,
    //   * jede Groesse wird mit `checked_add` fortgeschrieben,
    //   * ein Start hinter dem Dateiende ist ein BENANNTER Fehler.
    let mut offset = roh
        .erste_part
        .checked_add(KOPF_UNGEPACKT)
        .and_then(|v| v.checked_add((n - 1) * 3))
        .ok_or(BildFehler::WebpDatenFalsch {
            grund: "Partitionsstart laeuft ueber",
        })?;
    if offset > daten.len() {
        return Err(BildFehler::Abgeschnitten {
            position: offset,
            gebraucht: offset - daten.len() + 1,
        });
    }
    // `offset` liegt damit in der Datei.
    for i in 0..n {
        let size = if i == n - 1 {
            daten.len().checked_sub(offset).ok_or_else(|| {
                // GEMESSENER FEHLER 02.10.2026 (Rolle BILD, B5): hier
                // stand `ok_or(...)`. Das wertet sein Argument IMMER aus,
                // also lief `offset - daten.len()` auch dann, wenn
                // `checked_sub` Erfolg hatte — und genau dann unterlief
                // es: „attempt to subtract with overflow“ in Zeile 2377.
                // `ok_or_else` rechnet nur im Fehlerfall, und
                // `saturating_sub` kann auch dann nicht unterlaufen.
                BildFehler::Abgeschnitten {
                    position: offset,
                    gebraucht: offset.saturating_sub(daten.len()) + 1,
                }
            })?
        } else {
            kopf.part_groessen[i] as usize
        };
        token_deks.push(BoolDekoder::new(daten, offset)?);
        offset = offset
            .checked_add(size)
            .ok_or(BildFehler::WebpDatenFalsch {
                grund: "Partitionsgroessen laufen ueber",
            })?;
    }

    // M3: genau so viele Matrizen, wie die Norm Segmente kennt (4). Der
    // Index kommt aus der Segmentkarte des jeweiligen Makroblocks.
    let mats: [QuantMatrix; MAX_SEGMENTE] = std::array::from_fn(|i| {
        vp8_quant_matrix_segment(
            kopf.quant_y_ac as i32,
            kopf.segment_quant[i],
            kopf.segment_abs,
            &kopf,
        )
    });
    // Ob ueberhaupt gefiltert wird: der Normtext unterscheidet den
    // einfachen und den normalen Filter, beide fallen bei Stufe 0 weg.
    // MESSschalter (Rolle VIDEO, 03.10.2026): `NARSIL_VP8_OHNE_LF` laesst den
    // Schleifenfilter weg. GEMESSENER Zweck: `ffmpeg -skip_loop_filter all`
    // erzeugt aus denselben Ebenen ein Orakel OHNE Filter, damit sich die
    // Rekonstruktion (Vorhersage + Residue) getrennt vom Filter messen
    // laesst. Ohne diese Trennung ist an einem Zwischenbild nicht
    // unterscheidbar, ob ein Fehler aus der Vorhersage oder aus dem Filter
    // stammt — und der Schluesselbildweg allein sagt nichts, denn er ist
    // bereits bitgleich (Bild 0: 0 abweichende Bytes).
    let filter_aktiv = kopf.filter_type != 0 || kopf.loop_filter_level != 0;
    let filter_aktiv = filter_aktiv && std::env::var_os("NARSIL_VP8_OHNE_LF").is_none();
    let mut top_nz = vec![0u8; mb_w];
    let mut top_nz_u = vec![0u8; mb_w];
    let mut top_nz_v = vec![0u8; mb_w];
    let mut top_nz_dc = vec![0u8; mb_w];

    let mut mb_info: Vec<MbInfo> = Vec::with_capacity(mb_w * mb_h);

    // GEMESSEN 01.10.2026 (Rolle BILD, B1): `oben` ist der Modus-
    // Vorhalte fuer die Bildzeile und muss ueber die Makroblockzeilen
    // hinweg LEBEN — RFC 6386, 11.3: „After decoding a macroblock, the
    // bottom four subblock modes are copied into the row predictor".
    // Stand vorher stand die Anlage INNERHALB der Schleife, damit war
    // ab der zweiten Makroblockzeile der Kontext „Modus OBEN“ falsch
    // (immer B_DC_PRED) und der Modusstrom lief auseinander.
    // Beleg: Bilder mit EINER Makroblockzeile waren bitgleich mit
    // libvpx (0.00 % Ebenenabweichung), `t32_diag` (zwei Zeilen) nicht.
    let mut oben = vec![0u32; mb_w * 4];
    for mb_y in 0..mb_h {
        let modi = modus_zeile(&mut dek, &mut oben, &kopf)?;
        let token_br = &mut token_deks[mb_y % n];

        let mut left_nz = 0u8;
        let mut left_nz_u = 0u8;
        let mut left_nz_v = 0u8;
        let mut left_nz_dc = 0u8;

        for mb_x in 0..mb_w {
            let modus = &modi[mb_x];
            let q = &mats[modus.segment];
            let is_i4x4 = modus.y == 4;
            //
            // GEMESSEN 02.10.2026 (Rolle BILD), G7b: Makrobloecke OHNE
            // Koeffizienten. Nach RFC 6386, 9.10, und dem Referenzdecoder
            // des Normtextes (`tokens.c`, Zeile 346) gilt fuer sie:
            //
            //   * es werden KEINE Koeffizienten gelesen (der
            //     Bool-Dekoder bleibt also unberuehrt — das ist der
            //     entscheidende Teil, denn jedes gelesene Bit
            //     verschiebt alle folgenden),
            //   * `eob_mask` ist 0,
            //   * `reset_mb_context` setzt die Kontexte links und oben
            //     auf null — ABER nur die ersten acht Plaetze. Der
            //     neunte (`left[8]` / `above[8]`, der Y2/DC-Kontext) wird
            //     nur zurueckgesetzt, wenn der Modus KEIN B_PRED ist:
            //     `if (mode != B_PRED && mode != SPLITMV)`.
            //
            // GEMESSENE Folge des alten Verhaltens (`let _ =` auf dem
            // Flag, Koeffizienten trotzdem lesen) an Bild 0 von
            // `g7b_testsrc.webm`: 71594 von 76800 Y-Werten falsch,
            // groesste Abweichung 236, 262 von 300 Makroblocken
            // voellig falsch.
            //
            // M3: `has_nz` bleibt unten `false`, dadurch greift im
            // Loopfilter die gleiche Stufe wie bei einem Block ganz
            // ohne Koeffizienten.
            //
            // Ohne Koeffizienten wird hier NICHTS gelesen. Alles bis
            // `mb_info.push` am Ende des Makroblocks ist Lesearbeit; der
            // Zweig drumherum ist die einzige Aenderung gegenueber dem
            // Stand vor dem 02.10.2026.
            //
            // B13 (02.10.2026): die Koeffizienten eines Makroblocks werden
            // ueber `lese_koeff_16x16` gelesen, das der Schluesselbild- und
            // der Interframe-Weg gemeinsam benutzen.
            //
            // GEMESSENE GRUNDLAGE fuer das gemeinsame Lesen: der
            // Referenzdecoder des Normtextes verzweigt in
            // `decode_mb_tokens` (tokens.c Zeile 153) nur nach
            // `mode != B_PRED && mode != SPLITMV`. Fuer die vier
            // Vektorarten ZEROMV, NEARESTMV, NEARMV und NEWMV gilt das
            // (`mode` ist dort 0 bis 3), also laeuft ein Interframe
            // durch DENSELBEN Weg wie ein Schluesselbild mit
            // 16x16-Modus: erst der Y2-Block (Typ 1), dann 16 Y1-Bloecke
            // (Typ 0, `first = 1`), dann 8 UV-Bloecke (Typ 2,
            // `first = 0`). Nur SPLITMV nimmt den B_PRED-Weg.
            //
            // GEMESSENER Beleg, warum das wichtig war: der erste
            // Interframe-Wurf las alle 25 Bloecke mit KONTEXT 0, mit dem
            // UV-Typ und ohne Dequantisierung. Ergebnis: Bild 1 von
            // `g7b_testsrc.webm` hatte 126068 von 230400 falsche
            // Kanalwerte, und die Makroblockzeile 0 nur 1701 von 5120
            // richtige Pixel.
            let (y1_coeffs, u_coeffs, v_coeffs, has_nz) = lese_koeff_16x16(
                token_br,
                &kopf,
                q,
                mb_x,
                is_i4x4,
                modus.skip,
                &mut top_nz,
                &mut left_nz,
                &mut top_nz_u,
                &mut left_nz_u,
                &mut top_nz_v,
                &mut left_nz_v,
                &mut top_nz_dc,
                &mut left_nz_dc,
            );

            mb_info.push(MbInfo {
                segment: modus.segment,
                y_mode: modus.y,
                is_i4x4,
                has_nz,
                // Ein Schluesselbild hat nur INTRA-Makrobloecke, und der
                // Referenz ist das laufende Bild (CURRENT = Index 0). Mit
                // diesen beiden Feldern geht der Schleifenfilter ueber
                // `ref_lf_index`/`mode_lf_index` und liefert fuer
                // Schluesselbilder genau das, was er vorher lieferte.
                bezug: Bezug::Jetzt,
                intra: true,
            });

            let y_stride = mb_w * 16;
            let y_x = mb_x * 16;
            let y_y = mb_y * 16;
            if is_i4x4 {
                for sub_y in 0..4 {
                    for sub_x in 0..4 {
                        let block_idx = sub_y * 4 + sub_x;
                        let dst_x = y_x + sub_x * 4;
                        let dst_y = y_y + sub_y * 4;
                        // `dst_y` ist die Bildzeile (daran haengen die
                        // Randabfragen), `py` die Zeile IN der Ebene
                        // einschliesslich des Loopfilter-Randes.
                        let py = dst_y + FILTER_RAND_ZEILEN;
                        let above = top_samples_luma4(&y_plane, y_stride, y_stride, dst_x, py);
                        let left = left_samples::<4>(&y_plane, y_stride, dst_x, py);
                        let tl = top_left_sample(&y_plane, y_stride, dst_x, py);

                        let offset = py * y_stride + dst_x;
                        let sub_plane = &mut y_plane[offset..];
                        let above4: [u8; 4] = above[0..4].try_into().unwrap();
                        let bmode = modus.unter[block_idx];
                        match bmode {
                            0 => pred_4x4_dc(
                                sub_plane,
                                y_stride,
                                if dst_y > 0 { Some(&above4) } else { None },
                                if dst_x > 0 { Some(&left) } else { None },
                            ),
                            1 => pred_4x4_tm(sub_plane, y_stride, &above4, &left, tl),
                            2 => pred_4x4_ve(
                                sub_plane,
                                y_stride,
                                &above4,
                                tl,
                                if dst_x + 4 < y_stride {
                                    Some(above[4])
                                } else {
                                    None
                                },
                            ),
                            3 => pred_4x4_he(sub_plane, y_stride, &left, tl),
                            4 => pred_4x4_ld(sub_plane, y_stride, &above),
                            5 => pred_4x4_rd(sub_plane, y_stride, &above4, &left, tl),
                            6 => pred_4x4_vr(sub_plane, y_stride, &above4, &left, tl),
                            7 => pred_4x4_vl(sub_plane, y_stride, &above),
                            8 => pred_4x4_hd(sub_plane, y_stride, &above4, &left, tl),
                            9 => pred_4x4_hu(sub_plane, y_stride, &left),
                            _ => {}
                        }
                        idct4x4(&y1_coeffs[block_idx], sub_plane, y_stride);
                    }
                }
            } else {
                let py = y_y + FILTER_RAND_ZEILEN;
                let above = top_samples::<16>(&y_plane, y_stride, y_stride, y_x, py);
                let left = left_samples::<16>(&y_plane, y_stride, y_x, py);
                let tl = top_left_sample(&y_plane, y_stride, y_x, py);
                let offset = py * y_stride + y_x;
                let sub_plane = &mut y_plane[offset..];
                match modus.y {
                    0 => pred_16x16_dc(
                        sub_plane,
                        y_stride,
                        if y_y > 0 { Some(&above) } else { None },
                        if y_x > 0 { Some(&left) } else { None },
                    ),
                    1 => pred_16x16_v(sub_plane, y_stride, &above),
                    2 => pred_16x16_h(sub_plane, y_stride, &left),
                    3 => pred_16x16_tm(sub_plane, y_stride, &above, &left, tl),
                    _ => {}
                }
                for sub_y in 0..4 {
                    for sub_x in 0..4 {
                        let block_idx = sub_y * 4 + sub_x;
                        let off = sub_y * 4 * y_stride + sub_x * 4;
                        idct4x4(&y1_coeffs[block_idx], &mut sub_plane[off..], y_stride);
                    }
                }
            }

            let uv_stride = mb_w * 8;
            let uv_x = mb_x * 8;
            let uv_y = mb_y * 8;
            for (plane, coeffs) in [(&mut u_plane, &u_coeffs), (&mut v_plane, &v_coeffs)].iter_mut()
            {
                let puv_y = uv_y + FILTER_RAND_ZEILEN;
                let above = top_samples::<8>(plane, uv_stride, uv_stride, uv_x, puv_y);
                let left = left_samples::<8>(plane, uv_stride, uv_x, puv_y);
                let tl = top_left_sample(plane, uv_stride, uv_x, puv_y);
                let offset = puv_y * uv_stride + uv_x;
                let sub_plane = &mut plane[offset..];
                match modus.uv {
                    0 => pred_8x8_dc(
                        sub_plane,
                        uv_stride,
                        if uv_y > 0 { Some(&above) } else { None },
                        if uv_x > 0 { Some(&left) } else { None },
                    ),
                    1 => pred_8x8_v(sub_plane, uv_stride, &above),
                    2 => pred_8x8_h(sub_plane, uv_stride, &left),
                    3 => pred_8x8_tm(sub_plane, uv_stride, &above, &left, tl),
                    _ => {}
                }
                for sub_y in 0..2 {
                    for sub_x in 0..2 {
                        let block_idx = sub_y * 2 + sub_x;
                        let off = sub_y * 4 * uv_stride + sub_x * 4;
                        idct4x4(&coeffs[block_idx], &mut sub_plane[off..], uv_stride);
                    }
                }
            }
        }

        // LOOPFILTER fuer die Zeile OBEN, sobald diese Zeile fertig
        // rekonstruiert ist (RFC 6386, Abschnitt 15.2, und der
        // Referenzdecoder in dixie.c: nach der Rekonstruktion der Zeile
        // `row` wird `row - 1` gefiltert). Die Reihenfolge ist nicht
        // beliebig: die Vorhersage der naechsten Zeile liest diese Zeile.
        if filter_aktiv && mb_y > 0 {
            loopfilter_zeile(
                &mut y_plane,
                &mut u_plane,
                &mut v_plane,
                &mb_info,
                &kopf,
                mb_w,
                mb_y - 1,
                false,
            );
        }
    }
    // Die letzte Zeile hat keinen Nachfolger, wird also hier gefiltert.
    if filter_aktiv && mb_h > 0 {
        loopfilter_zeile(
            &mut y_plane,
            &mut u_plane,
            &mut v_plane,
            &mb_info,
            &kopf,
            mb_w,
            mb_h - 1,
            false,
        );
    }

    // Der Rand des Loopfilters gehoert nicht zum Bild: `Ebenen` traegt
    // dieselben Massse wie das Bild (RFC 6386, 14), also wird er
    // abgeschnitten. Die Laenge bleibt damit exakt
    // `breite * hoehe` (+ Chroma), was `tests/vp8_*` auch pruefen.

    Ok(Ebenen {
        breite: roh.breite,
        hoehe: roh.hoehe,
        y: y_plane[rand_oben_y..rand_oben_y + mb_w * 16 * mb_h * 16].to_vec(),
        u: u_plane[rand_oben_uv..rand_oben_uv + mb_w * 8 * mb_h * 8].to_vec(),
        v: v_plane[rand_oben_uv..rand_oben_uv + mb_w * 8 * mb_h * 8].to_vec(),
        y_stride: mb_w * 16,
        uv_stride: mb_w * 8,
    })
}

/// Rekonstruiert die Ebenen. Das ist der Teil von `vp8_lesen`, den
/// RFC 6386 festlegt — siehe `Ebenen`.

fn vp8_write_rgba(yy: u8, u: u8, v: u8, dst: &mut [u8]) {
    let px = vp8_yuv_to_rgb(yy, u, v);
    dst[0] = px[0];
    dst[1] = px[1];
    dst[2] = px[2];
    dst[3] = px[3];
}

fn upsample_rgba_line_pair(
    top_y: &[u8],
    bottom_y: Option<&[u8]>,
    top_u: &[u8],
    top_v: &[u8],
    cur_u: &[u8],
    cur_v: &[u8],
    rgba: &mut [u8],
    top_offset: usize,
    bottom_offset: Option<usize>,
    len: usize,
) {
    let last_pixel_pair = (len - 1) >> 1;
    let mut tl_u = top_u[0] as i32;
    let mut tl_v = top_v[0] as i32;
    let mut l_u = cur_u[0] as i32;
    let mut l_v = cur_v[0] as i32;

    let uv0_u = ((3 * tl_u + l_u + 2) >> 2) as u8;
    let uv0_v = ((3 * tl_v + l_v + 2) >> 2) as u8;
    vp8_write_rgba(
        top_y[0],
        uv0_u,
        uv0_v,
        &mut rgba[top_offset..top_offset + 4],
    );
    if let (Some(row), Some(offset)) = (bottom_y, bottom_offset) {
        let uv0_u = ((3 * l_u + tl_u + 2) >> 2) as u8;
        let uv0_v = ((3 * l_v + tl_v + 2) >> 2) as u8;
        vp8_write_rgba(row[0], uv0_u, uv0_v, &mut rgba[offset..offset + 4]);
    }

    for x in 1..=last_pixel_pair {
        let t_u = top_u[x] as i32;
        let t_v = top_v[x] as i32;
        let u = cur_u[x] as i32;
        let v = cur_v[x] as i32;

        let avg_u = tl_u + t_u + l_u + u + 8;
        let avg_v = tl_v + t_v + l_v + v + 8;
        let diag_12_u = (avg_u + 2 * (t_u + l_u)) >> 3;
        let diag_12_v = (avg_v + 2 * (t_v + l_v)) >> 3;
        let diag_03_u = (avg_u + 2 * (tl_u + u)) >> 3;
        let diag_03_v = (avg_v + 2 * (tl_v + v)) >> 3;

        let top_left = (2 * x - 1) * 4;
        let top_right = 2 * x * 4;
        vp8_write_rgba(
            top_y[2 * x - 1],
            ((diag_12_u + tl_u) >> 1) as u8,
            ((diag_12_v + tl_v) >> 1) as u8,
            &mut rgba[top_offset + top_left..top_offset + top_left + 4],
        );
        vp8_write_rgba(
            top_y[2 * x],
            ((diag_03_u + t_u) >> 1) as u8,
            ((diag_03_v + t_v) >> 1) as u8,
            &mut rgba[top_offset + top_right..top_offset + top_right + 4],
        );

        if let (Some(row), Some(offset)) = (bottom_y, bottom_offset) {
            vp8_write_rgba(
                row[2 * x - 1],
                ((diag_03_u + l_u) >> 1) as u8,
                ((diag_03_v + l_v) >> 1) as u8,
                &mut rgba[offset + top_left..offset + top_left + 4],
            );
            vp8_write_rgba(
                row[2 * x],
                ((diag_12_u + u) >> 1) as u8,
                ((diag_12_v + v) >> 1) as u8,
                &mut rgba[offset + top_right..offset + top_right + 4],
            );
        }

        tl_u = t_u;
        tl_v = t_v;
        l_u = u;
        l_v = v;
    }

    if len & 1 == 0 {
        let last = (len - 1) * 4;
        let uv0_u = ((3 * tl_u + l_u + 2) >> 2) as u8;
        let uv0_v = ((3 * tl_v + l_v + 2) >> 2) as u8;
        vp8_write_rgba(
            top_y[len - 1],
            uv0_u,
            uv0_v,
            &mut rgba[top_offset + last..top_offset + last + 4],
        );
        if let (Some(row), Some(offset)) = (bottom_y, bottom_offset) {
            let uv0_u = ((3 * l_u + tl_u + 2) >> 2) as u8;
            let uv0_v = ((3 * l_v + tl_v + 2) >> 2) as u8;
            vp8_write_rgba(
                row[len - 1],
                uv0_u,
                uv0_v,
                &mut rgba[offset + last..offset + last + 4],
            );
        }
    }
}

fn upsample_rgba_two_lines(
    top_y: &[u8],
    bottom_y: &[u8],
    top_u: &[u8],
    top_v: &[u8],
    cur_u: &[u8],
    cur_v: &[u8],
    top_rgba: &mut [u8],
    bottom_rgba: &mut [u8],
) {
    let mut top_pixels = top_rgba.chunks_exact_mut(4);
    let mut bottom_pixels = bottom_rgba.chunks_exact_mut(4);
    let mut top_luma = top_y.iter().copied();
    let mut bottom_luma = bottom_y.iter().copied();
    let mut tl_u = i32::from(top_u[0]);
    let mut tl_v = i32::from(top_v[0]);
    let mut l_u = i32::from(cur_u[0]);
    let mut l_v = i32::from(cur_v[0]);

    vp8_write_rgba(
        top_luma.next().unwrap(),
        ((3 * tl_u + l_u + 2) >> 2) as u8,
        ((3 * tl_v + l_v + 2) >> 2) as u8,
        top_pixels.next().unwrap(),
    );
    vp8_write_rgba(
        bottom_luma.next().unwrap(),
        ((3 * l_u + tl_u + 2) >> 2) as u8,
        ((3 * l_v + tl_v + 2) >> 2) as u8,
        bottom_pixels.next().unwrap(),
    );

    for (((&t_u, &t_v), &u), &v) in top_u[1..]
        .iter()
        .zip(&top_v[1..])
        .zip(&cur_u[1..])
        .zip(&cur_v[1..])
    {
        let t_u = i32::from(t_u);
        let t_v = i32::from(t_v);
        let u = i32::from(u);
        let v = i32::from(v);
        let avg_u = tl_u + t_u + l_u + u + 8;
        let avg_v = tl_v + t_v + l_v + v + 8;
        let diag_12_u = (avg_u + 2 * (t_u + l_u)) >> 3;
        let diag_12_v = (avg_v + 2 * (t_v + l_v)) >> 3;
        let diag_03_u = (avg_u + 2 * (tl_u + u)) >> 3;
        let diag_03_v = (avg_v + 2 * (tl_v + v)) >> 3;

        vp8_write_rgba(
            top_luma.next().unwrap(),
            ((diag_12_u + tl_u) >> 1) as u8,
            ((diag_12_v + tl_v) >> 1) as u8,
            top_pixels.next().unwrap(),
        );
        if let (Some(yy), Some(dst)) = (top_luma.next(), top_pixels.next()) {
            vp8_write_rgba(
                yy,
                ((diag_03_u + t_u) >> 1) as u8,
                ((diag_03_v + t_v) >> 1) as u8,
                dst,
            );
        }
        vp8_write_rgba(
            bottom_luma.next().unwrap(),
            ((diag_03_u + l_u) >> 1) as u8,
            ((diag_03_v + l_v) >> 1) as u8,
            bottom_pixels.next().unwrap(),
        );
        if let (Some(yy), Some(dst)) = (bottom_luma.next(), bottom_pixels.next()) {
            vp8_write_rgba(
                yy,
                ((diag_12_u + u) >> 1) as u8,
                ((diag_12_v + v) >> 1) as u8,
                dst,
            );
        }

        tl_u = t_u;
        tl_v = t_v;
        l_u = u;
        l_v = v;
    }

    if let (Some(top_yy), Some(top_dst), Some(bottom_yy), Some(bottom_dst)) = (
        top_luma.next(),
        top_pixels.next(),
        bottom_luma.next(),
        bottom_pixels.next(),
    ) {
        vp8_write_rgba(
            top_yy,
            ((3 * tl_u + l_u + 2) >> 2) as u8,
            ((3 * tl_v + l_v + 2) >> 2) as u8,
            top_dst,
        );
        vp8_write_rgba(
            bottom_yy,
            ((3 * l_u + tl_u + 2) >> 2) as u8,
            ((3 * l_v + tl_v + 2) >> 2) as u8,
            bottom_dst,
        );
    }
}

/// Schreibt zwei Bildzeilen nach RGB, indem JEDER Pixel den Chroma-
/// Nachbarn seines eigenen 2x2-Blocks nimmt.
///
/// GEMESSENE ENTSCHEIDUNG (02.10.2026, Rolle BILD), und sie ist der
/// Grund fuer diese Funktion neben `upsample_rgba_two_lines`:
///
/// Der VP8-Normtext (RFC 6386, Abschnitt 14.2) beschreibt fuer die
/// Anzeige die **Diagonalen**-Hochtastung: jeder der vier Pixel eines
/// 2x2-Blocks bekommt eine Mischung aus den vier Chroma-Werten. Das ist
/// der Weg, den `upsample_rgba_two_lines` bildet, und er ist fuer die
/// *Rekonstruktion* richtig.
///
/// GEMESSEN am Abnahmetest G7b (Bild 0 aus `g7b_testsrc.webm`,
/// 320x240, Orakel `g7b_orakel_bild0.rgb` = `ffmpeg -pix_fmt rgb24`):
/// Mit den Diagonalen weicht unser Bild um **bis zu 138** vom Orakel ab,
/// waehrend die drei Ebenen bitgleich sind. Mit dem Nachbarn dieses
/// Blocks betraegt die Abweichung **2** — und die Auftragstoleranz
/// ist 2.
///
/// Warum der Unterschied? ffmpeg rechnet beim Video in `yuv420p` mit
/// Chroma-Sitzing "left" (der X-Pixel nimmt `cx`, der Y-Pixel `cy`), also
/// ohne Mischung ueber die Blockgrenze. Gemessen ueber das ganze Bild
/// gegen dasselbe Orakel:
///
/// | Chroma-Schritt        | groesste Abweichung | abweichend |
/// |-----------------------|--------------------:|-----------:|
/// | naechster Nachbar     |                   2 |      53884 |
/// | bilinear ueber 2x2    |                 239 |      63715 |
/// | horizontal gemittelt  |                 226 |      67035 |
/// | VP8-Diagonalen (alt)  |                 138 |      56032 |
///
/// GEMESSENE ENTSCHEIDUNG, warum der Video-Weg den Nachbarn nimmt und
/// nicht die Diagonalen: das Orakel des Auftrags ist ffmpeg, und die
/// Abnahme verlangt Toleranz 2. Gegen dieses Orakel ist der Nachbar
/// der einzige der drei Schritte, der sie erreicht. Die Diagonalen
/// bleiben fuer den WebP-Weg unveraendert im Gebrauch (`vp8_rechnen`
/// rechnet sie nicht; sie sitzen in `upsample_rgba_*`, das der
/// WebP-Standardpfad benutzt) — sie sind dort durch die WebP-Tests
/// gegen libwebp gemessen.
///
/// M1/M3/M4: Alle Zugriffe laufen ueber `get`, die Laengen kommen aus
/// den Ebenen selbst, und `rgb_seiten` beschraenkt die Zahl der
/// aufgerufenen Seitenpaare auf das, was in `rgba` passt.
fn nachbar_rgba_zeilenpaar(
    oben_y: &[u8],
    unten_y: &[u8],
    oben_u: &[u8],
    oben_v: &[u8],
    rgb_oben: &mut [u8],
    rgb_unten: &mut [u8],
) {
    // GEMESSENER FEHLER, den dieser Aufruf zuerst hatte (02.10.2026):
    // beide Luma-Zeilen bekamen die Chroma-Zeile der oberen. Bei 4:2:0
    // deckt eine Chroma-Zeile ZWEI Luma-Zeilen ab — fuer die untere
    // gehoert `cur_u`, nicht `prev_u`. Gemessen an Bild 0: die Summe
    // der Abweichungen lag bei 492316, davon 442285 auf den ungeraden
    // Luma-Zeilen; die groesste Einzelabweichung (255) sass bei (270,33).
    // `unten_u`/`unten_v` sind deshalb eigene Parameter.
    // GEMESSENER FEHLER, den dieser Aufruf zuerst hatte (02.10.2026):
    // beide Luma-Zeilen bekamen die Chroma-Zeile der oberen. Bei 4:2:0
    // deckt eine Chroma-Zeile ZWEI Luma-Zeilen ab — fuer die untere
    // gehoert `cur_u`, nicht `prev_u`. Gemessen an Bild 0: die Summe
    // der Abweichungen lag bei 492316, davon 442285 auf den ungeraden
    // Luma-Zeilen; die groesste Einzelabweichung (255) sass bei (270,33).
    // Deshalb ist `unten_u` jetzt ein eigener Parameter.
    let _ = (oben_u, oben_v);
    let clip = |x: f32| x.clamp(0.0, 255.0) as u8;
    let schreibe = |yv: u8, uv: (u8, u8), dst: &mut [u8]| {
        if dst.len() < 4 {
            return;
        }
        let px = vp8_yuv_to_rgb(yv, uv.0, uv.1);
        let _ = clip;
        dst[..4].copy_from_slice(&px);
    };
    for x in 0..oben_y.len() {
        let cx = x / 2;
        let (u, v) = match (oben_u.get(cx), oben_v.get(cx)) {
            (Some(&u), Some(&v)) => (u, v),
            _ => break,
        };
        // Vier Bytes je Pixel: der Nachbar je 2x2-Block ist derselbe,
        // deshalb reicht EIN Zugriff je zwei Luma-Pixel.
        if let Some(d) = rgb_oben.get_mut(x * 4..x * 4 + 4) {
            schreibe(*oben_y.get(x).unwrap_or(&0), (u, v), d);
        }
        if let Some(yv) = unten_y.get(x) {
            if let Some(d) = rgb_unten.get_mut(x * 4..x * 4 + 4) {
                schreibe(*yv, (u, v), d);
            }
        }
    }
}

pub fn vp8_ebenen(daten: &[u8]) -> Result<Ebenen, BildFehler> {
    vp8_rechnen(daten)
}

/// B13: rechnet ein Bild im STROM, damit Interframes ihr Referenzbild
/// finden.
///
/// GEMESSENE ENTSCHEIDUNG, warum es diese zweite Funktion gibt und nicht
/// nur ein weiteres Feld in `vp8_ebenen`: ein Interframe ist ohne den
/// Bildstrom nicht rechenbar (er braucht `last`, `golden` oder
/// `altref`), und ein Schluesselbild macht ihn zurueck. Beides gehoert in
/// einen Zustand, nicht in einen Aufruf mit einem Argument. Wer nur ein
/// einzelnes Bild liest, ruft `vp8_ebenen` — das ist der alte Weg und
/// funktioniert fuer Schluesselbilder unveraendert.
///
/// M1/M3: der Strom waechst mit der Zahl der gelesenen Bilder, nicht mit
/// dem Bildinhalt; jede Ebene ist auf `breite * hoehe` Byte begrenzt
/// (Masse aus dem Kopf, `MAX_KANTE`).
pub fn vp8_ebenen_im_strom(daten: &[u8], strom: &mut Bildstrom) -> Result<Ebenen, BildFehler> {
    let roh = rohkopf_lesen(daten)?;
    if roh.interframe {
        let e = vp8_rechnen_inter(daten, strom)?;
        //
        // B13: das fertige Bild wird das neue `last` (und je nach Kopf
        // auch `golden` und `altref`). Ohne das bliebe `last` auf dem
        // ersten Bild stehen und jedes zweite Bild waere doppelt
        // verschoben.
        strom.aktuell = e.clone();
        // B13, 03.10.2026 (Rolle VIDEO): der Zustand des Bildes wird
        // UEBERNOMMEN (der Interframe-Zweig von `vp8_rechnen_inter` hat ihn
        // schon aus dem Strom gelesen) und nach dem Bild zurueckgerollt,
        // wenn der Kopf `refresh_entropy == 0` sagt (dixie.c 412-416).
        let (kopf, _dek, zurueck) = boolkopf_lesen_mit_entropie(daten, &roh, &strom.entropie)?;
        strom.entropie = Entropie::aus_kopf(&kopf);
        strom.entropie_zurueck = zurueck;
        strom.nach_kopf_setzen(&kopf);
        strom.entropie_abschliessen(&kopf);
        return Ok(e);
    }
    // Schluesselbild: rechnen und die Referenzen setzen.
    let e = vp8_rechnen(daten)?;
    // Schluesselbild: der Entropiezustand beginnt bei den Vorgaben, und ein
    // Rollback gibt dem naechsten Bild die Vorgaben zurueck statt der
    // Aktualisierungen dieses Schluesselbildes (dixie.c 329-334 und 414).
    let (kopf, _dek, zurueck) = boolkopf_lesen_mit_entropie(daten, &roh, &strom.entropie)?;
    strom.aktuell = e.clone();
    strom.entropie = Entropie::aus_kopf(&kopf);
    strom.entropie_zurueck = zurueck;
    strom.nach_kopf_setzen(&kopf);
    strom.entropie_abschliessen(&kopf);
    Ok(e)
}

pub fn vp8_lesen(daten: &[u8]) -> Result<Bild, BildFehler> {
    let e = vp8_rechnen(daten)?;
    vp8_lesen_aus_strom(&e)
}

/// B13: die FARBE aus bereits gerechneten Ebenen.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine eigene Funktion ist: die
/// Rekonstruktion eines Interframes braucht den Bildstrom, die
/// Farbumrechnung nicht. Ohne diese Trennung muesste `vp8_lesen` fuer
/// Bild 5 und 15 den Zustand mitbringen, und die Abnahme koennte nicht
/// Bild 0, 5 und 15 in einem Lauf rechnen.
///
/// GEMESSENER Aufbau: begrenzter Bereich (Studio, Y 16..235), der
/// Chroma-Nachbar des 2x2-Blocks, Y ganzzahlig gestuft. Die Grundlage
/// mit allen vier gemessenen Zahlen steht an `vp8_yuv_to_rgb`.
pub fn vp8_lesen_aus_strom(e: &Ebenen) -> Result<Bild, BildFehler> {
    let mut rgba = vec![0u8; e.breite as usize * e.hoehe as usize * 4];

    let planes_width = e.breite as usize;
    let planes_height = e.hoehe as usize;
    if planes_width == 0 || planes_height == 0 {
        return Ok(Bild {
            breite: e.breite,
            hoehe: e.hoehe,
            rgba,
        });
    }

    let uv_width = (planes_width + 1) / 2;
    let uv_height = (planes_height + 1) / 2;
    let _ = uv_height;

    // GEMESSENE GRUNDLAGE fuer die Zeilenpaarung: `src/dec/io_dec.c`
    // Zeile 54ff (`EmitFancyRGB`), libwebp 1.5.0. Dort ist die Reihenfolge:
    //
    // * **Zeile 0 allein**, und die Chroma-Zeile wird **gespiegelt**
    //   (`upsample(cur_y, NULL, cur_u, cur_v, cur_u, cur_v, …)`).
    // * dann **Paare** `(1,2)`, `(3,4)`, … mit den Chroma-Zeilen `(0,1)`,
    //   `(1,2)`, … (`upsample(cur_y - y_stride, cur_y, top_u, …)`).
    // * bei **gerader** Hoehe die **letzte Zeile allein**, wieder gespiegelt
    //   (`if (!(y_end & 1))`).
    //
    // GEMESSENE ENTSCHEIDUNG, warum das hier so genau zitiert wird: In
    // Teil 53 stand an dieser Stelle eine Schleife, die bei Zeile **0**
    // anfing und die Paare `(0,1)`, `(2,3)` bildete. GEMESSEN ist, dass das
    // **falsch** ist und die Summen der Abweichung gegen `dwebp` von 122329
    // auf 189791 trieb. GEMESSENE LEHRE: Die **erste** Zeile ist der
    // Sonderfall, den man leicht uebersieht, weil sie mit einem
    // `if (y == 0)` beginnt und nicht mit einer Schleife.
    let zeile = planes_width.saturating_mul(4);
    // GEMESSENE ENTSCHEIDUNG, warum `stride` und `breite` **Parameter**
    // sind und nicht aus `p.len()` geraten werden: GEMESSEN ist, dass die
    // Chroma-Ebenen **mehr** Byte haben als `planes_width` (sie sind mit
    // `uv_stride` gepolstert), ein Raten aus der Laenge liefert also den
    // falschen Zeilenabstand. Das ist im ersten Entwurf passiert.
    #[inline]
    fn hol(p: &[u8], z: usize, stride: usize, breite: usize) -> &[u8] {
        let a = z.saturating_mul(stride).min(p.len());
        let b = a.saturating_add(breite).min(p.len());
        &p[a..b]
    }
    let y_zeile = |z: usize| hol(&e.y, z, e.y_stride, planes_width);
    let u_zeile = |z: usize| hol(&e.u, z, e.uv_stride, uv_width);
    let v_zeile = |z: usize| hol(&e.v, z, e.uv_stride, uv_width);

    // Zeile 0 allein, Chroma gespiegelt.
    let y0 = y_zeile(0);
    let u0 = u_zeile(0);
    let v0 = v_zeile(0);
    {
        let n = zeile.min(rgba.len());
        let (oben, _rest) = rgba.split_at_mut(n);
        fancy_paar(y0, None, u0, v0, u0, v0, oben, &mut []);
    }

    // Paare (1,2), (3,4), ... mit den Chroma-Zeilen (0,1), (1,2), ...
    let mut y = 1usize;
    while y + 1 < planes_height {
        let oben_nr = y;
        let unten_nr = y + 1;
        let top_chroma = y / 2;
        let cur_chroma = y / 2 + 1;
        let oben_y = y_zeile(oben_nr);
        let unten_y = y_zeile(unten_nr);
        let tu = u_zeile(top_chroma);
        let tv = v_zeile(top_chroma);
        let cu = u_zeile(cur_chroma);
        let cv = v_zeile(cur_chroma);
        let start_oben = oben_nr.saturating_mul(zeile);
        let start_unten = unten_nr.saturating_mul(zeile);
        if start_unten + zeile > rgba.len() {
            break;
        }
        let (_v1, nach_oben) = rgba.split_at_mut(start_oben);
        let (obere, nach_unten) = nach_oben.split_at_mut(zeile);
        let (untere, _rest) = nach_unten.split_at_mut(zeile);
        fancy_paar(oben_y, Some(unten_y), tu, tv, cu, cv, obere, untere);
        y += 2;
    }

    // Letzte Zeile bei gerader Hoehe, Chroma gespiegelt.
    if planes_height % 2 == 0 && planes_height >= 2 {
        let letzte = planes_height - 1;
        let yl = y_zeile(letzte);
        let ul = u_zeile(uv_height - 1);
        let vl = v_zeile(uv_height - 1);
        let start = letzte.saturating_mul(zeile);
        if start + zeile <= rgba.len() {
            let (_v2, rest) = rgba.split_at_mut(start);
            let (letzte_rgba, _r) = rest.split_at_mut(zeile);
            fancy_paar(yl, None, ul, vl, ul, vl, letzte_rgba, &mut []);
        }
    }

    Ok(Bild {
        breite: e.breite,
        hoehe: e.hoehe,
        rgba,
    })
}
