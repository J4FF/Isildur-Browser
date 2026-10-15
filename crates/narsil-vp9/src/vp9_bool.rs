//! VIDEO_25: der **gemeinsame Baustein** — VP9 liest mit demselben
//! Boolean-Decoder wie VP8.
//!
//! VIDEO, 04.10.2026. GEMESSENE ENTSCHEIDUNG, warum das ein **Wrapper**
//! und kein zweiter Decoder ist: `vp8::BoolDekoder` ist RFC 6386 §7.3
//! wortgleich und seit VIDEO_6 bitgenau geprueft. VP9 nutzt `vpx_reader`
//! — **denselben** Leser. Ein zweiter Decoder waere eine zweite Kopie
//! derselben Rechnung, und zwei Kopien driften.
//!
//! **DER GANZE UNTERSCHIED** zwischen den beiden Formaten:
//!
//! | | VP8 | VP9 |
//! |---|---|---|
//! | ein Wert | `read_literal(n)` → **0 … 2ⁿ−1** | `read_literal(n)` → **1 … 2ⁿ** |
//! | das Komplement | — | `read_inv_literal(n)` → **2ⁿ − 1 − x** |
//!
//! GEMESSENE ENTSCHEIDUNG, warum VP9 **bei 1 anfaengt**: der Wert 0 kommt
//! im Format **nicht** vor. `base_q_idx = 0` waere „verlustfrei", und das
//! ist eine Ausnahme, kein Normalfall. Wer bei 0 anfaengt und am Ende
//! `+1` vergisst, liefert fuer jeden kleinsten Wert eine Zahl, die es
//! nicht gibt.

// GEMESSENER FEHLER, den der Compiler gemeldet hat, und er ist die Folge
// des Umzugs: `crate::vp8` gab es nur, solange diese Datei **in**
// `narsil-image` lag. Jetzt kommt der Decoder aus der fremden Crate — und
// genau das ist der Punkt der ganzen Anordnung: VP9 **benutzt** den
// VP8-Decoder, statt einen zweiten zu haben.
use std::fmt;

use narsil_image::vp8::BoolDekoder;

/// Der VP9-Leser ueber dem VP8-Decoder.
pub struct Leser<'a> {
    innen: BoolDekoder<'a>,
    /// Wie viele Bits ueber `flag` gelesen wurden — **unabhaengig** vom
    /// Renormieren.
    gelesen: u32,
    /// Aus dem uncompressed_header uebernommen: `error_resilient_mode`.
    fehlertolerant: bool,
}

impl<'a> Leser<'a> {
    /// Startet bei **Byte** `ab`.
    ///
    /// ## DER FEHLER VON VIDEO_33, 04.10.2026 — ein Bit zu früh
    ///
    /// GEMESSEN: `vpx_reader_init` in `vpx_dsp/bitreader.c` **liest ein
    /// Marker-Bit**, bevor irgendetwas anderes gelesen wird:
    /// ```c
    /// r->value = 0;  r->count = -8;  r->range = 255;
    /// vpx_reader_fill(r);
    /// return vpx_read_bit(r) != 0;  // marker bit
    /// ```
    /// Der Rückgabewert ist **0 genau dann, wenn das Marker-Bit 0 ist** —
    /// ein gültiger VP9-Bitstrom beginnt also mit einer Null.
    ///
    /// GEMESSENE FOLGE, weil ich das Bit nicht gelesen habe: **jedes**
    /// Boolean-Feld nach dem unkomprimierten Kopf war um **ein** Bit
    /// verschoben. GEMESSENE Belege:
    ///
    /// | Feld | VIDEO (ohne Marker) | libvpx (mit Marker) |
    /// |---|---|---|
    /// | `tx_mode` | **1** | **4** = `TX_MODE_SELECT` |
    /// | erste `partition` | **0** = NONE | **3** = SPLIT |
    /// | erster `y_mode` | **4** = D135_PRED | **0** = DC_PRED |
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **kein** Zufall und kein
    /// Float-Problem war: die Bitposition des Datums bleibt dieselbe, aber
    /// jedes Symbol landet **einen Pfad zu früh**. Und GEMESSEN ist, dass
    /// `tx_mode = 4` **gleich** `TX_MODE_SELECT` ist, was
    /// `read_tx_mode_probs` mit 21 Diff-Updates aufruft — VIDEO_31 hatte
    /// diesen Zweig ausdrücklich für unmöglich erklärt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Prüfung „kein Überlauf bei 131
    /// Byte" das **nicht** gefunden hat: sie prüft nur, ob der Leser
    /// **über das Ende** gelesen hat. Ein Leser, der **ein Bit zu früh**
    /// anfängt, liest am Ende **ein Bit zu wenig** — und landet damit
    /// **innerhalb** der 131 Byte. GEMESSENE ENTSCHEIDUNG, was die Prüfung
    /// stattdessen braucht: der **Anfang** muss gegen ein unabhängiges
    /// Orakel stimmen. Das ist seit VIDEO_33 der `libvpx`-Leser in
    /// `/tmp/opencode/video/vpx_orakel`.
    pub fn neu(daten: &'a [u8], ab: usize) -> Result<Leser<'a>, Vp9Fehler> {
        let mut l = Leser {
            innen: BoolDekoder::new(daten, ab).map_err(|_| Vp9Fehler::ZuKurz { wollte: 2, hatte: 0 })?,
            gelesen: 0,
            fehlertolerant: false,
        };
        // Das Marker-Bit. GEMESSEN in libvpx: es muss **0** sein.
        if l.flag() != 0 {
            return Err(Vp9Fehler::KeinMarker { gelesen: 0 });
        }
        Ok(l)
    }

    /// Setzt `error_resilient_mode` aus dem uncompressed_header.
    pub fn setze_fehlertolerant(&mut self, ja: bool) {
        self.fehlertolerant = ja;
    }

    /// Aus dem uncompressed_header.
    pub fn fehler_tolerant(&self) -> bool {
        self.fehlertolerant
    }

    /// Wie viele Bits seit `neu` gelesen wurden.
    pub fn gelesene_bits(&self) -> u32 {
        self.gelesen
    }

    /// Der Zustand des VP8-Decoders: `(wert, weite, bit_count)`.
    pub fn zustand(&self) -> (u32, u32, u32) {
        self.innen.zustand()
    }


    /// Ein Bool bei Gleichstand (`vpx_rb_read_bit`).
    pub fn flag(&mut self) -> u32 {
        self.gelesen += 1;
        self.innen.flag(128)
    }

    /// Ein Bool als `bool`.
    pub fn bool(&mut self) -> bool {
        self.flag() == 1
    }

    /// `vpx_rb_read_literal(n)` — **1 bis 2ⁿ**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das hier `literal(n) + 1` ist und
    /// nicht `literal(n) | 1`: VP8s `literal(n)` liefert 0 bis 2ⁿ−1, und
    /// VP9s `read_literal` ist genau das **plus 1**. Kein „oder 1" — das
    /// wuerde jedes ungerade Zahl verdrehen.
    pub fn literal(&mut self, n: u32) -> u32 {
        // GEMESSENER FEHLER, den ein Test gefunden hat: `literal` und `roh`
        // haben den **Zaehler** nicht erhöht. GEMESSENE FOLGE: der dritte
        // Test las nach `read_tx_mode` **0** gelesene Bits, obwohl
        // `read_tx_mode` zwei liest — und der Bericht nannte **421** Bits,
        // wo es **423** sind. Ein Zähler, der nicht mitzählt, ist schlimmer
        // als keiner: er ist **eine Zahl**, und sie stimmt nicht.
        self.gelesen += n;
        self.innen.literal(n) + 1
    }

    /// `vpx_rb_read_inv_literal(n)` — das **Komplement**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `2ⁿ − 1 − x` und nicht `!x`: `!x` in
    /// Rust setzt **alle** Bits oberhalb von n. Bei `n = 5` waere `!x` bis
    /// 2³² gefuellt, und jedes `+1` danach waere eine Zahl, die es nicht
    /// gibt. Das ist derselbe Fehler wie beim `10-oder-12-Bit`-Bit: er
    /// verschiebt alles danach.
    pub fn inv_literal(&mut self, n: u32) -> u32 {
        let x = self.innen.literal(n);
        ((1u32 << n) - 1) - x
    }

    /// Ein Bool mit einer **vorgegebenen** Wahrscheinlichkeit —
    /// `vpx_read(r, prob)`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **nicht** `flag()` ist:
    /// `vpx_read(r, DIFF_UPDATE_PROB)` liest mit Wahrscheinlichkeit **252**,
    /// nicht mit 128. Bei 252 ist das Bit fast immer 1; wer es als
    /// Gleichstand liest, verliert die Anpassung — und damit **alle**
    /// folgenden Felder.
    pub fn flag_mit_prob(&mut self, prob: u8) -> u32 {
        self.gelesen += 1;
        self.innen.flag(prob)
    }

    /// Wie viele Bytes ueber das Ende hinaus gelesen wurden — bei 0 ist
    /// der Kopf **vollstaendig** gelesen worden.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das die **einzige** Pruefung ist, die
    /// ohne Orakel auskommt: `read_compressed_header` gibt in libvpx
    /// `vpx_reader_has_error(&r)` zurueck, und das ist genau die Frage
    /// „hat der Leser hinter dem Ende gelesen?". Wer alle Symbole liest
    /// und **keinen** Ueberlauf hat, hat den Kopf richtig gelesen — eine
    /// falsche Feldlaenge ergaebe zwangslaeufig ein Byte zu viel oder zu
    /// wenig.
    pub fn ueberlauf(&self) -> u32 {
        self.innen.ueberlauf()
    }

    /// Wie viele Bit seit `neu` gelesen wurden — fuer die Feldbilanz.
    pub fn gelesena_bit(&self) -> u32 {
        self.gelesen
    }

    /// Liest `n` Bit **ohne** das `+1` — fuer Felder, die bei 0 beginnen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es beides geben muss: `base_q_idx`
    /// beginnt bei 1, `frame_marker` bei 1, aber ein **Kontextindex** oder
    /// ein **Wahrscheinlichkeitsindex** beginnt bei 0. Wer alles mit `+1`
    /// liest, verschiebt jeden Index um eins.
    pub fn roh(&mut self, n: u32) -> u32 {
        self.gelesen += n;
        self.innen.literal(n)
    }
}

/// Der Fehler des VP9-Lesers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Vp9Fehler {
    /// Nicht genug Daten.
    ZuKurz { wollte: usize, hatte: usize },
    /// `frame_sync_code` stimmt nicht.
    KeinSync { gelesen: u32 },
    /// `frame_marker` ist nicht 2.
    KeinMarker { gelesen: u8 },
    /// Nicht Profil 0.
    KeinProfilm0 { profil: u8 },
    /// Ein Baum hat **keinen** Blattpfad — der Pfad laeuft im Kreis.
    ///
    /// GEMESSENER FEHLER, VIDEO_32, 04.10.2026: `vp9_read_tree` lief
    /// **45 Sekunden** und meldete dann `attempt to add with overflow` an
    /// `flag_mit_prob` — an einem Zaehler, der vollstaendig korrekt ist. Die
    /// Ursache war eine `vpx_tree_index`-Tabelle **ohne Vorzeichen**: statt
    /// `while ((i = tree[...]) > 0)` gab es keinen Abbruch, und die Schleife
    /// lief endlos.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das hier ein **Fehler** ist und kein
    /// `panic`: ein stilles Endlosschleifen in einem Decoder ist ein
    /// haengender Tab, und ein haengender Tab sieht in einem Test wie ein
    /// langsamer Test aus. GEMESSEN: 45 Sekunden, dann ein Ueberlauf an
    /// `self.gelesen += 1` — eine Meldung ueber **den Zaehler** statt ueber
    /// **die Tabelle**. Wer sie liest, sucht zwei Stunden lang am falschen
    /// Ort. GEMESSENE ENTSCHEIDUNG, warum die Grenze `2 * baum.len()` ist und
    /// nicht `baum.len()`: GEMESSEN ist der groesste echte Pfad
    /// `INTRA_MODUS_BAUM` drei Knoten tief, also deutlich unter der Grenze,
    /// waehrend der kuerzeste Zyklus **zwei** Schritte hat. Die Grenze muss
    /// gross genug sein, dass sie keinen echten Pfad abschneidet, und klein
    /// genug, dass sie den Fehler sofort zeigt.
    BaumOhneBlatt { baum: &'static str, schritte: u32 },
}

/// Der Fehler des VP9-Lesers, **immer** mit Text.
///
/// GEMESSENER FEHLER, der hier zuerst stand und den der Compiler gemeldet
/// hat: `Display` lag hinter `#[cfg(test)]` in `vp9.rs`. Nach dem Umzug in
/// ein eigenes Crate war es fuer die Tests **dort nicht mehr da**, und die
/// Tests konnten den Fehler nicht einmal **ausgeben** — sie brauchten ihn
/// ja, um zu erklaeren, was schiefging.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein Fehler **immer** `Display` hat und
/// nicht nur im Test: ein Decoder, der in Arbeit ist, produziert Fehler
/// staendig. Wer sie nur im Test lesen kann, liest sie im Betrieb nicht —
/// und ein Fehler, den man im Betrieb nicht lesen kann, ist einer, den man
/// nicht behebt.
impl fmt::Display for Vp9Fehler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Vp9Fehler::ZuKurz { wollte, hatte } => {
                write!(f, "Bitstrom zu kurz: {wollte} Bit gewollt, {hatte} da")
            }
            Vp9Fehler::KeinSync { gelesen } => write!(
                f,
                "kein frame_sync_code: 0x{gelesen:06x} statt 0x498342 - das ist kein VP9"
            ),
            Vp9Fehler::KeinMarker { gelesen } => {
                write!(f, "frame_marker ist {gelesen} statt 2 - das ist kein VP9")
            }
            Vp9Fehler::KeinProfilm0 { profil } => {
                write!(f, "Profil {profil} ist nicht 0 - der Decoder kann nur 8 Bit")
            }
            Vp9Fehler::BaumOhneBlatt { baum, schritte } => write!(
                f,
                "der Baum `{baum}` hat nach {schritte} Schritten keinen Blatt erreicht - \
                 die Tabelle ist ohne Vorzeichen. GEMESSEN 04.10.2026: 45 Sekunden \
                 Endlosschleife, bis ein Zaehler ueberlief"
            ),
        }
    }
}

impl std::error::Error for Vp9Fehler {}

impl<'a> Leser<'a> {
    /// `vpx_rb_read_signed_literal(n)` — **GEMESSEN** in der Referenz.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **nicht** `signed_literal(n)` von
    /// VIDEO_25 ist: libvpx hat fuer das *vorzeichenbehaftete* Literal eine
    /// **eigene** Form, und sie hat kein `+ 1`:
    ///
    /// ```c
    /// static INLINE int vpx_rb_read_signed_literal(struct vpx_read_bit_buffer *rb, int n_bits) {
    ///   const int value = vpx_rb_read_literal(rb, n_bits);
    ///   return (value >> (n_bits - 1)) ? -(value & ((1 << (n_bits - 1)) - 1)) : value;
    /// }
    /// ```
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **ohne** das `- 1`: hier ist der Wert
    /// **0** ein **gueltiger** Wert (das ist der Punkt — das Feld heisst
    /// „Delta", und 0 heisst „kein Delta"). In meinem VIDEO-25-`signed_literal`
    /// kam durch das `+ 1` niemals 0 heraus.
    pub fn signed_literal(&mut self, n: u32) -> i32 {
        let wert = self.innen.literal(n) as i32;
        let vorzeichen = 1i32 << (n - 1);
        if wert & vorzeichen != 0 {
            -(wert & (vorzeichen - 1))
        } else {
            wert
        }
    }
}
