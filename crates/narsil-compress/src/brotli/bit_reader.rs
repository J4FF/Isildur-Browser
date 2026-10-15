//! Bit-Reader fuer Brotli (LSB-first).
//!
//! DIE BITREIHENFOLGE, und sie hat mich eine Stunde gekostet, weil der
//! RFC an zwei Stellen das Gegenteil zu sagen scheint.
//!
//! Abschnitt 2 schreibt: "we also write bytes with the most significant
//! bit (msb) on the left". Das stimmt fuer die **Darstellung** eines
//! Byte als Zahl — aber gelesen wird nicht in dieser Reihenfolge.
//! Abschnitt 1.5.1 ("Packing into Bytes") legt es fest:
//!
//! > we would be able to parse the result from **right to left**, with
//! > fixed-width elements in the correct msb-to-lsb order and prefix
//! > codes in bit-reversed order (i.e., with the first bit of the code
//! > in the relative lsb position)
//!
//! und das Beispiel dort zeigt `10010110` ganz rechts im Strom, mit dem
//! 3-Bit-Wert 6 in den drei **niedrigstwertigen** Bits.
//!
//! ALSO: LSB-first, genau wie bei DEFLATE. Integer kommen mit dem
//! ersten Bit im niedrigstwertigen Bit an, Prefix-Codes ebenfalls.
//!
//! DER FEHLER, DEN ICH HIER GEMACHT HABE, und weshalb er nicht auffiel:
//! Ich hatte den Leser auf MSB-first umgestellt, weil der Satz aus
//! Abschnitt 2 naheliegt. Danach las der Dekader **jeden** Strom als
//! leer (das erste Byte `0x8b` ergibt MSB-first `ISLAST=1,
//! ISLASTEMPTY=0` statt `1, 1`) und lieferte 0 Byte — ohne Fehler, an
//! allen fuenf Testvektoren.
// added 30.09.2026: der Import `use crate::FehlerZlib;` stand hier
// und wurde nie gelesen — die Datei schreibt den Typ ueberall
// ausgeschrieben (`crate::FehlerZlib::DatenZuEnde`). Ein Import, den
// niemand liest, ist ein Kommentar, der nichts sagt.

/// Bit-Reader fuer Brotli-Stroeme (LSB-first).
pub struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u8,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        BitReader {
            data,
            pos: 0,
            bit: 0,
        }
    }

    /// Gibt die Anzahl der verfügbaren Bytes zurück.
    pub fn available(&self) -> usize {
        self.data.len() - self.pos
    }

    /// Wie viele Bit des aktuellen Byte sind schon gelesen.
    ///
    /// added 30.09.2026: fuer die Fehlersuche im Brotli-Decoder.
    pub fn bitzahl(&self) -> u8 {
        self.bit
    }

    /// Aktuelle Byte-Position.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Liest ein Bit, **LSB-first** (RFC 7932 Abschnitt 1.5.1).
    ///
    /// DER WEG ZUR BITREIHENFOLGE — und sie hat mich hier eine Stunde
    /// gekostet, weil der RFC an zwei Stellen das Gegenteil zu sagen
    /// scheint:
    ///
    /// Abschnitt 2 schreibt "we also write bytes with the most
    /// significant bit (msb) on the left". Das stimmt fuer die
    /// **Darstellung** eines Byte als Zahl. Gelesen wird aber nicht in
    /// dieser Reihenfolge: Abschnitt 1.5.1 legt es fest.
    ///
    /// Abschnitt 1.5.1 sagt: "Packing into Bytes ... we would be able
    /// to parse the result from **right to left**, with fixed-width
    /// elements in the correct msb-to-lsb order and prefix codes in
    /// bit-reversed order (i.e., with the first bit of the code in the
    /// relative lsb position)." Und das Beispiel dazu zeigt
    /// `10010110` ganz rechts im Strom, mit dem 3-Bit-Wert 6 in den
    /// drei **niedrigstwertigen** Bits.
    ///
    /// Also: Integer kommen mit dem **ersten** Bit im niedrigstwertigen
    /// Bit des Byte an, und Prefix-Codes werden bit-reversiert
    /// abgelegt — das erste Bit des Codes steht auch im niedrigsten
    /// Bit. Beides heisst: **von rechts nach links lesen**, also
    /// LSB-first, genau wie bei DEFLATE.
    ///
    /// Die erste Fassung dieses Lesers war richtig. Ich habe ihn
    /// zwischenzeitlich auf MSB-first umgestellt, weil mich der Satz
    /// aus Abschnitt 2 verleitete — und damit las der Dekoder jeden
    /// Strom als leer und lieferte 0 Byte.
    pub fn read_bit(&mut self) -> Result<u32, crate::FehlerZlib> {
        if self.pos >= self.data.len() {
            return Err(crate::FehlerZlib::DatenZuEnde { byte: self.pos });
        }
        let v = (self.data[self.pos] >> self.bit) & 1;
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.pos += 1;
        }
        Ok(v as u32)
    }

    /// Liest n Bits, **MSB-first** (RFC 7932 Abschnitt 2).
    pub fn read_bits(&mut self, n: u32) -> Result<u32, crate::FehlerZlib> {
        let mut v = 0u32;
        for i in 0..n {
            v |= self.read_bit()? << i;
        }
        Ok(v)
    }

    /// Aligns to next byte boundary.
    pub fn align_to_byte(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.pos += 1;
        }
    }

    /// Ueberspringt `n` Byte (fuer MSKIPLEN, Abschnitt 9.3).
    ///
    /// M4: `n` kommt aus dem Header und ist ein 2-Bit-Wert, also
    /// hoechstens 3. Diese Grenze steht trotzdem hier, weil die
    /// Funktion oeffentlich ist und `n` dann aus einem Aufrufer
    /// kommen kann.
    pub fn ueberspringe(&mut self, n: usize) -> Result<(), crate::FehlerZlib> {
        if n > 3 {
            return Err(crate::FehlerZlib::BrotliDekodierFehler {
                grund: format!("{n} Bytes ueberspringen, erlaubt sind 0 bis 3"),
            });
        }
        self.align_to_byte();
        if self.pos + n > self.data.len() {
            return Err(crate::FehlerZlib::DatenZuEnde { byte: self.pos });
        }
        self.pos += n;
        Ok(())
    }

    /// Wie viele Byte noch ungelesen sind.
    pub fn uebrige(&self) -> usize {
        self.data.len() - self.pos
    }

    /// Nimmt `n` Byte und rueckt weiter (ohne Ausrichtung).
    ///
    /// DER AUFRUFER muss den Strom vorher ausgerichtet haben. Das ist
    /// Absicht: `nimm` wird nur an Bytegrenzen benutzt (unkomprimierte
    /// Meta-Bloecke), und eine automatische Ausrichtung hier wuerde
    /// bei einem Fehler ein Byte zu viel springen.
    pub fn nimm(&mut self, n: usize) -> Result<&'a [u8], crate::FehlerZlib> {
        if self.pos + n > self.data.len() {
            return Err(crate::FehlerZlib::DatenZuEnde { byte: self.pos });
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    /// Reads n whole bytes.
    pub fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], crate::FehlerZlib> {
        self.align_to_byte();
        if self.pos + n > self.data.len() {
            return Err(crate::FehlerZlib::DatenZuEnde { byte: self.pos });
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bit_reader_basic() {
        let data = &[0b10101010, 0b11110000];
        let mut br = BitReader::new(data);

        // LSB-first (RFC 7932 Abschnitt 1.5.1): 0b10101010 liefert
        // 0, 1, 0, 1.
        assert_eq!(br.read_bit().unwrap(), 0, "LSB first: Bit 0");
        assert_eq!(br.read_bit().unwrap(), 1, "Bit 1");
        assert_eq!(br.read_bit().unwrap(), 0, "Bit 2");
        assert_eq!(br.read_bit().unwrap(), 1, "Bit 3");
    }

    #[test]
    fn test_read_bits() {
        let mut br = BitReader::new(&[0b00001111]);
        assert_eq!(
            br.read_bits(4).unwrap(),
            0b1111,
            "die vier niedrigstwertigen Bit"
        );
        let mut br2 = BitReader::new(&[0b11110000]);
        assert_eq!(br2.read_bits(4).unwrap(), 0, "und hier Nullen");
    }

    #[test]
    fn test_align_to_byte() {
        let data = &[0xFF, 0x00];
        let mut br = BitReader::new(data);
        br.read_bits(3).unwrap(); // read 3 bits
        br.align_to_byte(); // should move to next byte
        assert_eq!(br.pos(), 1);
    }
}
