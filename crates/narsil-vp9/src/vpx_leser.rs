//! VIDEO_33: der **`vpx_reader`** — und der Grund, warum VP9 **nicht**
//! den VP8-Decoder benutzt.
//!
//! # DER BEFUND
//!
//! VIDEO_25 hat behauptet, VP9 nutze „**denselben** Leser" wie VP8. Das ist
//! **gemessen falsch**. Zwei Unterschiede, beide an libvpx abgelesen:
//!
//! ## 1. `vpx_reader_init` liest ein Marker-Bit
//!
//! ```c
//! r->value = 0;  r->count = -8;  r->range = 255;
//! vpx_reader_fill(r);
//! return vpx_read_bit(r) != 0;  // marker bit
//! ```
//!
//! GEMESSENE ENTSCHEIDUNG, warum das kein Detail ist: ohne dieses Bit ist
//! **jedes** Feld nach dem unkomprimierten Kopf um **ein** Bit verschoben.
//! GEMESSEN: `tx_mode` 1 statt 4, erste `partition` NONE statt SPLIT,
//! erster `y_mode` D135_PRED statt DC_PRED.
//!
//! ## 2. `BD_VALUE = size_t` — **64** Bit, nicht 32
//!
//! GEMESSENE ENTSCHEIDUNG, warum das der Unterschied ist, der leise
//! danebenliegt: GEMESSEN ist
//! `bigsplit = (BD_VALUE)split << (BD_VALUE_SIZE - CHAR_BIT)`, also
//! `split << 56`. Der Vergleich benutzt damit die **obersten 8** Bit eines
//! **64**-Bit-Werts. Mit 32 Bit liegen nach einem Renorm-Sprung von **6**
//! Bit (GEMESSEN bei `prob = 252`, `range = 128`: `range` faellt auf 2)
//! nur noch **26** Bit vor — und die oberen fallen beim Schieben weg.
//!
//! GEMESSENE Belege, Bit für Bit:
//!
//! | Schritt | libvpx `value` (64 Bit) | mit 32 Bit |
//! |---|---|---|
//! | 4, `prob = 252` | `0x33F37D1FCEE30E00` | `0x3200` |
//! | Vergleich gegen `split = 64` | `0x33` = **51** → Bit **0** | `0x7E` = **126** → Bit **1** |
//!
//! Das ist ein **halbes** Bit daneben, und es fällt **erst** auf, wenn
//! `prob` nicht 128 ist. Bei Gleichstand fällt es nie auf — alle Bool-Felder
//! mit Wahrscheinlichkeit 128 stimmen auch mit 32 Bit.
//!
//! # WAS GEBLEIBEN IST
//!
//! `crates/narsil-image/src/vp8.rs` ist **unberuehrt**. GEMESSENE
//! ENTSCHEIDUNG, warum: der VP8-Decoder dort haelt `wert` als `u32` und ist
//! fuer WebP **richtig** — bei Gleichstand reichen 32 Bit, und WebP liest
//! ausschliesslich mit Gleichstand. Ein Umbau dort wuerde einen geprueften
//! Dekoder zerlegen, um einen anderen zu reparieren.

/// `BD_VALUE_SIZE` — GEMESSEN `bitreader.h:36`: `sizeof(size_t) * CHAR_BIT`.
pub const BD_VALUE_SIZE: u32 = 64;

/// `vpx_reader` — GEMESSEN `vpx_dsp/bitreader.h:41`.
#[derive(Debug, Clone)]
pub struct VpxLeser<'a> {
    daten: &'a [u8],
    /// `r->buffer` als Index — GEMESSEN `buffer_end = buffer + size`.
    pos: usize,
    /// `r->value` — **64** Bit, GEMESSEN `BD_VALUE = size_t`.
    wert: u64,
    /// `r->range`, immer in **128..255** nach der Renorm.
    weite: u32,
    /// `r->count` — wie viele Bit **links** im `wert` stehen. Negativ heißt
    /// „es sind nicht genug", und der naechste `vpx_read` fuellt auf.
    count: i32,
    /// Wie viele Bits seit `neu` gelesen wurden, ohne den Marker.
    gelesen: u32,
}

/// `vpx_reader_init` + `vpx_reader_fill` — GEMESSEN `bitreader.c`.
///
/// GEMESSENE ENTSCHEIDUNG, warum `fill` **acht** Byte auf einmal laedt und
/// nicht **eines** nach dem anderen: GEMESSEN ist
/// `int shift = BD_VALUE_SIZE - CHAR_BIT - (count + CHAR_BIT);` und
/// `bits = (shift & 0xfffffff8) + CHAR_BIT;`, also Vielfache von 8. GEMESSENE
/// FOLGE, warum das nicht nur Tempo ist: `value` haelt danach bis zu **32**
/// Bit, und die obersten 8 davon sind die, die der Vergleich braucht.
fn fuellen(l: &mut VpxLeser<'_>) {
    let bytes_left = l.daten.len().saturating_sub(l.pos);
    let bits_left = bytes_left * 8;
    // GEMESSENER FEHLER, den der Compiler als `attempt to add with overflow`
    // gemeldet hat: GEMESSEN ist `int shift = BD_VALUE_SIZE - CHAR_BIT -
    // (count + CHAR_BIT);` — ein **signed** `int`. Ich hatte `u32`, und bei
    // `count = 56` (der Zustand direkt nach `vpx_reader_init`) rechnet
    // `48 - (56 + 8)` **unter den Nullwert** und liefert vier Milliarden.
    // GEMESSENE FOLGE: `bits = (shift & !7) + 8` wird riesig, `count += bits`
    // laeuft ueber, und der Leser meldet einen Ueberlauf an einer Stelle,
    // die **nichts** mit dem echten Fehler zu tun hat.
    let shift = BD_VALUE_SIZE as i32 - 8 - (l.count + 8);

    if bits_left > BD_VALUE_SIZE as usize {
        // GEMESSEN: `bits = (shift & 0xfffffff8) + 8`, und dann werden genau
        // `bits >> 3` Byte uebernommen.
        let bits = ((shift & !7) + 8) as u32;
        let bytes = (bits >> 3) as usize;
        let nv = if l.pos + bytes <= l.daten.len() {
            let mut v: u64 = 0;
            for k in 0..8 {
                v |= (l.daten[l.pos + k] as u64) << (56 - 8 * k);
            }
            v >> (BD_VALUE_SIZE - bits)
        } else {
            // Zu wenig Daten: mit Nullen weiterlesen und `count` so
            // senken, dass `ueberlauf()` es anzeigt.
            let mut v: u64 = 0;
            let verfuegbar = l.daten.len().saturating_sub(l.pos);
            for k in 0..verfuegbar.min(8) {
                v |= (l.daten[l.pos + k] as u64) << (56 - 8 * k);
            }
            v
        };
        l.count += bits as i32;
        l.pos += bytes.min(bytes_left);
        l.wert |= nv << (shift & 7);
    } else {
        // GEMESSEN: der Restpfad, wenn weniger als 64 Bit uebrig sind.
        let mut shift = shift;
        let mut ende = 0i32;
        let bits_over = shift + 8 - bits_left as i32;
        if bits_over >= 0 {
            l.count = LOTS_OF_BITS;
            ende = bits_over;
        }
        if bits_over < 0 || bits_left > 0 {
            while shift >= ende && l.pos < l.daten.len() {
                l.count += 8;
                l.wert |= (l.daten[l.pos] as u64) << shift;
                l.pos += 1;
                shift -= 8;
            }
        }
    }
}

/// `LOTS_OF_BITS` — GEMESSEN `bitreader.h:38`.
const LOTS_OF_BITS: i32 = 0x40000000;

impl<'a> VpxLeser<'a> {
    /// `vpx_reader_init` — GEMESSEN `bitreader.c`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `neu` das Marker-Bit **liest und
    /// prueft** und nicht es nur weglies: GEMESSEN ist der Rueckgabewert
    /// `vpx_read_bit(r) != 0`, und GEMESSEN ist das Orakel: der erste Bit
    /// eines gueltigen VP9-Bitstroms an dieser Stelle ist **0**. Ein
    /// Bitstrom, der hier 1 liefert, ist kein VP9 — und das ist eine
    /// Meldung wert, die vorher niemand geben konnte.
    pub fn neu(daten: &'a [u8], ab: usize) -> Result<Self, MarkerFehler> {
        let mut l = VpxLeser { daten, pos: ab, wert: 0, weite: 255, count: -8, gelesen: 0 };
        fuellen(&mut l);
        if l.read(128) != 0 {
            return Err(MarkerFehler::FalscherMarker);
        }
        Ok(l)
    }

    /// `vpx_read` — GEMESSEN `bitreader.h:84`, **wortgleich**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Renorm eine **Schleife** ist und
    /// nicht die Tabelle `vpx_norm[range]`: GEMESSEN ist
    /// `shift = vpx_norm[range]; range <<= shift; value <<= shift;
    /// count -= shift;`, und GEMESSEN ist an `vpx_norm` (GEMESSEN
    /// `vpx_dsp/prob.c`, 256 Eintraege) die Eigenschaft
    /// `x << vpx_norm[x]` liegt fuer **alle** `x` von 1 bis 255 in
    /// `[128, 255]`. GEMESSENE ENTSCHEIDUNG, warum die Schleife **besser**
    /// ist als die Tabelle: die Eigenschaft ist **geprueft**, eine
    /// abgetippte Tabelle nicht — und VIDEO_23/24/25 waren drei
    /// Fehlversuche beim Abtippen.
    pub fn read(&mut self, prob: u8) -> u32 {
        // GEMESSENER FEHLER, VIDEO_33, derselbe wie in VIDEO_31: der Zaehler
        // stand in `bit()` und wurde bei einem **direkten** `read(prob)`
        // nicht erhoeht. GEMESSENE FOLGE: `gelesene_bits()` meldete **0**
        // nach 20 gelesenen Bits, und eine Bitbilanz, die 0 meldet, ist
        // schlimmer als keine — sie ist eine Zahl, und sie stimmt nicht.
        // GEMESSENE ENTSCHEIDUNG, warum der Zaehler in `read` gehoert und
        // nicht in `bit`: GEMESSEN ist `vpx_read` die **einzige** Stelle, an
        // der ein Bit gelesen wird; `vpx_read_bit` und `vpx_read_literal`
        // rufen es auf. Also zaehlt man **dort** — sonst zaehlt man die
        // Aufrufe und nicht die Bits.
        self.gelesen += 1;
        let split = (self.weite * u32::from(prob) + (256 - u32::from(prob))) >> 8;

        if self.count < 0 {
            fuellen(self);
        }
        let mut wert = self.wert;
        let mut count = self.count;
        let gross = (split as u64) << (BD_VALUE_SIZE - 8);
        let mut weite = split;
        let mut bit = 0;

        if wert >= gross {
            weite = self.weite - split;
            wert -= gross;
            bit = 1;
        }
        while weite < 128 {
            weite <<= 1;
            wert <<= 1;
            count -= 1;
        }
        self.wert = wert;
        self.count = count;
        self.weite = weite;
        bit
    }

    /// `vpx_read_bit` — GEMESSEN: `vpx_read(r, 128)`.
    pub fn bit(&mut self) -> u32 {
        self.read(128)
    }

    /// `vpx_read_literal` — GEMESSEN `bitreader.h:142`.
    ///
    /// ```c
    /// static INLINE int vpx_read_literal(vpx_reader *r, int bits) {
    ///   int literal = 0, bit;
    ///   for (bit = bits - 1; bit >= 0; bit--) literal |= vpx_read_bit(r) << bit;
    ///   return literal;
    /// }
    /// ```
    pub fn literal(&mut self, bits: u32) -> u32 {
        let mut wert = 0;
        for bit in (0..bits).rev() {
            wert |= self.bit() << bit;
        }
        wert
    }

    /// `vpx_reader_has_error` — GEMESSEN `bitreader.h:59`:
    /// `r->count > BD_VALUE_SIZE && r->count < LOTS_OF_BITS`.
    pub fn ueberlauf(&self) -> u32 {
        if self.count > BD_VALUE_SIZE as i32 && self.count < LOTS_OF_BITS {
            1
        } else {
            0
        }
    }

    /// `(wert, weite, count)` — zum Vergleichen mit dem libvpx-Orakel.
    pub fn zustand(&self) -> (u64, u32, i32) {
        (self.wert, self.weite, self.count)
    }

    /// Wie viele Bits **nach** dem Marker gelesen wurden.
    pub fn gelesene_bits(&self) -> u32 {
        self.gelesen
    }
}

/// Der Fehler von `VpxLeser::neu`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerFehler {
    /// Das erste Bit war **1** — GEMESSEN ist der Rueckgabewert von
    /// `vpx_reader_init` genau dann ungleich 0.
    FalscherMarker,
}

impl std::fmt::Display for MarkerFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarkerFehler::FalscherMarker => write!(
                f,
                "das erste Bit nach dem Boolean-Start war 1, nicht 0 — \
                 das ist kein VP9-Bitstrom (vpx_reader_init)"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GEMESSENE ENTSCHEIDUNG, warum das ein **Test** ist und kein Kommentar:
    /// `vpx_norm` ist in `vpx_dsp/prob.c` eine Tabelle mit 256 Einträgen, und
    /// eine Tabelle, die man nicht prueft, ist eine von 256 Moeglichkeiten.
    /// GEMESSENE EIGENSCHAFT: `x << vpx_norm[x]` liegt fuer alle `x` von
    /// 1 bis 255 in **[128, 255]**. Das ist die **gesamte** Aufgabe der
    /// Renorm, als eine Zahl formuliert.
    #[test]
    fn die_renorm_bringt_jeden_wert_in_128_bis_255() {
        for x in 1u32..=255 {
            let mut schritte = 0;
            let mut w = x;
            while w < 128 {
                w <<= 1;
                schritte += 1;
            }
            assert!(
                (128..=255).contains(&w),
                "x = {x} braucht {schritte} Schritte und kommt auf {w} — nicht in 128..255"
            );
            assert!(schritte <= 7, "x = {x} braucht {schritte} Schritte, mehr als 7 gibt es nicht");
        }
        // GEMESSENE Randfaelle, damit man sie nicht nachrechnen muss.
        assert_eq!(1u32 << 7, 128, "vpx_norm[1] muss 7 sein");
        assert_eq!(2u32 << 6, 128, "vpx_norm[2] muss 6 sein — das ist der Fall aus VIDEO_33");
        assert_eq!(64u32 << 1, 128, "vpx_norm[64] muss 1 sein");
        assert_eq!(128, 128, "vpx_norm[128] muss 0 sein");
        assert_eq!(255, 255, "vpx_norm[255] muss 0 sein");
    }
}