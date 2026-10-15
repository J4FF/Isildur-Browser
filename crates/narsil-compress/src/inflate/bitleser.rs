// W5F, 30.09.2026: der `use` stand hier am Dateikopf, wurde aber nur von
// `mod tests` gebraucht — der Code selbst schreibt `crate::FehlerZlib`
// ueberall ausgeschrieben. Also gehoert der Import in die Tests.
//! Bit-Leser für DEFLATE (LSB-first).

/// Liest DEFLATE-Bits: erstes Bit im Byte ist das NIEDRIGSTE.
pub struct Bitleser<'a> {
    daten: &'a [u8],
    pos: usize,
    bit: u32,
}

impl<'a> Bitleser<'a> {
    pub fn neu(daten: &'a [u8]) -> Bitleser<'a> {
        Bitleser {
            daten,
            pos: 0,
            bit: 0,
        }
    }

    pub fn verfuegbar(&self) -> usize {
        self.daten.len() - self.pos
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    /// Liest ein Bit. `0` oder `1`.
    pub fn leses_bit(&mut self) -> Result<u32, crate::FehlerZlib> {
        if self.pos >= self.daten.len() {
            return Err(crate::FehlerZlib::DatenZuEnde { byte: self.pos });
        }
        let v = (self.daten[self.pos] >> self.bit) & 1;
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.pos += 1;
        }
        Ok(v as u32)
    }

    /// Liest `n` Bits (maximal 16) als Zahl, LSB zuerst gelesen.
    pub fn leses_bits(&mut self, n: u32) -> Result<u32, crate::FehlerZlib> {
        let mut v = 0u32;
        for i in 0..n {
            v |= self.leses_bit()? << i;
        }
        Ok(v)
    }

    /// Verwirft die restlichen Bits des aktuellen Bytes.
    pub fn richtung_bytegrenze(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.pos += 1;
        }
    }

    /// Liest `n` ganze Byte am Stueck.
    pub fn leses_bytes(&mut self, n: usize) -> Result<&'a [u8], crate::FehlerZlib> {
        self.richtung_bytegrenze();
        if self.pos + n > self.daten.len() {
            return Err(crate::FehlerZlib::DatenZuEnde { byte: self.pos });
        }
        let s = &self.daten[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
}
