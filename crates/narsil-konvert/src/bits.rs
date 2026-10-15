//! Bit-Schreiber, MSB zuerst — die Reihenfolge, in der der MPEG-Audio-Leser
//! liest (`get_bits`).
//!
//! GEMESSEN 02.10.2026 an FLAC: Rahmen- und Subframe-Kopf sehen unter beiden
//! Bitrichtungen gleich aus, und der erste Unterschied zeigt sich erst bei
//! einem Wert, der ueber eine Bytegrenze reicht. Bei MP3 ist es umgekehrt —
//! hier ist schon das Seiteninfo-Layout an Bit 30 messbar, und dieses
//! Werkzeug ist der Grund, warum es messbar ist.

/// Ein MSB-erst Bitpuffer.
///
/// Der Puffer wird am Ende auf eine Bytegrenze aufgefüllt — mit Nullen, und
/// das ist wichtig: der Leser liest `part2_3_length` Bit aus diesem Bereich
/// und danach die naechsten, und fuellende Einsen wuerden als Huffman-Codes
/// gelesen.
#[derive(Debug, Clone, Default)]
pub struct Bits {
    bytes: Vec<u8>,
    bit: usize,
}

impl Bits {
    /// Ein leerer Puffer.
    pub fn neu() -> Self {
        Self::default()
    }

    /// Wie viele Bit stehen drin.
    pub fn laenge(&self) -> usize {
        self.bit
    }

    /// Schreibt `n` Bit von `wert`, vom höchsten zum niedrigsten.
    ///
    /// `n` darf bis 32 sein; `wert` wird auf `n` Bit gekürzt, wie der Leser
    /// es auch tut.
    pub fn feld(&mut self, wert: u32, n: u32) {
        debug_assert!(
            n <= 32,
            "ein Feld hat hoechstens 32 Bit, gefragt wurden {n}"
        );
        for i in (0..n).rev() {
            self.bit((wert >> i) & 1 == 1);
        }
    }

    /// Schreibt `n` vorzeichenbehaftete Bit von `wert`, Zweierkomplement.
    pub fn feld_zwei(&mut self, wert: i64, n: u32) {
        // GEMESSEN: `>>` ist in Rust arithmetisch, also liefert es fuer
        // negative Zahlen genau die Einsen, die das Zweierkomplement
        // verlangt. Logisch verschoben wuerde aus -1 eine riesige Zahl.
        for i in (0..n).rev() {
            self.bit((wert >> i) & 1 == 1);
        }
    }

    /// Schreibt ein einzelnes Bit.
    pub fn bit(&mut self, eins: bool) {
        if self.bit % 8 == 0 {
            self.bytes.push(0);
        }
        if eins {
            let letzte = self.bytes.len() - 1;
            self.bytes[letzte] |= 0x80 >> (self.bit % 8);
        }
        self.bit += 1;
    }

    /// Fuellt auf eine Bytegrenze auf und gibt die Bytes zurueck.
    ///
    /// Der Rest wird **null**. GEMESSEN warum das nicht kosmetisch ist: der
    /// Leser liest aus diesem Bereich `part2_3_length` Bit und danach
    /// weiter, und fuellende Einsen werden als Huffman-Codes gelesen. Ein
    /// falsch aufgefuellter Bereich klingt nicht „abgeschnitten", er klingt
    /// nach Rauschen.
    pub fn fertig(mut self) -> Vec<u8> {
        while self.bit % 8 != 0 {
            self.bit(false);
        }
        self.bytes
    }

    /// Schneidet auf hoechstens `bit` Bit ab.
    ///
    /// GEMESSEN 02.10.2026: noetig, weil der Leser feste 136 Bit (mono) bzw.
    /// 256 Bit (stereo) liest und **nicht** so viele, wie in der Datei stehen.
    /// Laesst man mehr stehen, rutscht der Anfang der Hauptdaten mit, und die
    /// Hauptdatensuche laeuft dann an der falschen Stelle — ohne Meldung.
    pub fn beschneide(&mut self, bit: usize) {
        if self.bit <= bit {
            return;
        }
        self.bit = bit;
        let bytes = bit.div_ceil(8);
        self.bytes.truncate(bytes);
    }

    /// Eine Kopie, an der man weiterbauen kann, ohne das Original zu veraendern.
    ///
    /// GEMESSEN 02.10.2026: `roh_rahmen` in `mp3.rs` braucht das, weil es
    /// denselben Puffer fuer Kopf und Hauptdaten weiterverwenden will.
    pub fn kopie(&self) -> Self {
        self.clone()
    }

    /// Fuellt auf eine Bytegrenge auf, ohne die Bytes zu liefern — praktisch
    /// fuer das Reservieren von Platz.
    pub fn reserviere(&mut self, bit: usize) {
        for _ in 0..bit {
            self.bit(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn felder_stehen_msb_zuerst() {
        let mut b = Bits::neu();
        b.feld(0b101, 3);
        b.feld(0b1, 1);
        assert_eq!(b.laenge(), 4);
        assert_eq!(b.fertig(), vec![0b1011_0000]);
    }

    #[test]
    fn zweierkomplement_ist_verlustfrei() {
        //
        // GEFUNDEN, DASS HIER EIN FEHLER IM TEST STECKTE und nicht im Code:
        // ich habe `-100` bei 4 Bit geprueft. Vier Bit Zweierkomplement
        // kennen `-8..7`, also war die Erwartung falsch und der Code richtig.
        //
        // Das ist die Sorte Test, die einen Fehler sucht, den es nicht gibt —
        // und sie sieht aus wie ein echter Fund, weil die Meldung
        // `wert -100 bei 4 Bit` lautet und damit sehr konkret wirkt.
        //
        // Jetzt werden je Breite nur Werte geprueft, die **hineinpassen**,
        // und zusaetzlich die Grenzen selbst, weil die interessieren.
        for n in [4u32, 5, 8, 12, 16] {
            //
            // `n` Bit Zweierkomplement kennt `-2^(n-1) .. 2^(n-1)-1`.
            //
            // ACHTUNG, `2^(n-1)` selbst gehoert NICHT dazu — bei 4 Bit ist
            // das die 8, und die groesste darstellbare Zahl ist 7. Ich hatte
            // `grenze` in der Liste und bekam `wert 8 bei 4 Bit`, also
            // wieder eine falsche Erwartung statt eines Fehlers. Die
            // Randwerte, die interessieren, sind `-grenze` und
            // `grenze - 1`, und genau die zwei stehen jetzt drin.
            let grenze = 1i64 << (n - 1);
            let werte = [
                0,
                1,
                -1,
                -grenze,
                grenze - 1,
                grenze - 2,
                -grenze + 1,
                grenze / 3,
                -grenze / 3,
            ];
            for wert in werte {
                let mut b = Bits::neu();
                b.feld_zwei(wert, n);
                let daten = b.fertig();
                let mut roh = 0u64;
                //
                // Und GEMESSEN ist, warum hier **nicht** `for i in 0..` steht:
                // clippy sagt „der Index wird nur zum Indizieren benutzt“ — und
                // **das** stimmt. Also `take()` statt Index, und die Rechnung
                // bleibt **dieselbe**: jedes Byte wird um **acht** Stellen
                // **nach links** geschoben und **verodiert**.
                for &b in daten.iter().take(usize::try_from(n).unwrap().div_ceil(8)) {
                    roh = (roh << 8) | u64::from(b);
                }
                let roh = roh >> ((daten.len() * 8) - n as usize);
                let geprueft = if roh >> (n - 1) == 1 {
                    (roh as i64) - (1i64 << n)
                } else {
                    roh as i64
                };
                assert_eq!(geprueft, wert, "wert {wert} bei {n} Bit");
            }
        }
    }

    /// Und der Kern der Sache: **logisch** verschoben wuerde aus `-1` eine
    /// riesige Zahl. Rusts `>>` ist arithmetisch, deshalb stimmt es — und
    /// deshalb ist es einen Test wert.
    #[test]
    fn einsen_der_zweiterkomplement_stehen_oben() {
        let mut b = Bits::neu();
        b.feld_zwei(-1, 8);
        assert_eq!(b.fertig(), vec![0xff]);
        let mut b = Bits::neu();
        b.feld_zwei(-2, 8);
        assert_eq!(b.fertig(), vec![0xfe]);
        let mut b = Bits::neu();
        b.feld_zwei(-128, 8);
        assert_eq!(b.fertig(), vec![0x80]);
    }

    #[test]
    fn fuellung_ist_null() {
        let mut b = Bits::neu();
        b.feld(1, 1);
        assert_eq!(b.fertig(), vec![0b1000_0000], "der Rest muss null sein");
    }
}
