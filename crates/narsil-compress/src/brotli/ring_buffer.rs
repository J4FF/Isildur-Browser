//! Ring-Puffer (Sliding Window) fuer Brotli.
//!
//! Verwaltet den Ausgabepuffer mit Fenster-Semantik fuer
// Rueckwaertsreferenzen (Distance-Codes).

// added 30.09.2026: der Import `use crate::FehlerZlib;` stand hier und
// wurde nie gelesen — die Dateien schreiben den Typ ueberall
// ausgeschrieben (`crate::FehlerZlib::DatenZuEnde`). Ein Import, den
// niemand liest, ist ein Kommentar, der nichts sagt.
/// Ring-Puffer (Sliding Window) mit fester maximaler Groesse.
pub struct RingBuffer {
    /// Datenpuffer
    data: Vec<u8>,
    /// Start-Index des gueltigen Fensters
    start: usize,
    /// Aktuelle Laenge des gueltigen Fensters
    len: usize,
    /// Maximale Kapazitaet (Fenstergroesse)
    capacity: usize,
}

impl RingBuffer {
    /// Erstellt einen neuen Ring-Puffer mit gegebener Kapazitaet.
    pub fn new(capacity: usize) -> Self {
        RingBuffer {
            data: vec![0u8; capacity],
            start: 0,
            len: 0,
            capacity,
        }
    }

    /// Gibt die aktuelle Laenge des gueltigen Fensters zurueck.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Ist das Fenster leer?
    ///
    /// added 30.09.2026: clippy merkt zu Recht an, dass eine Crate mit
    /// `len` auch `is_empty` braucht — und hier ist es nicht Formsache:
    /// der Aufrufer in `kopiere` prueft `distanz > self.fenster.len()`,
    /// und `is_empty` macht die Randfaelle (Distanz 1 im leeren Fenster)
    /// lesbar.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Gibt die Kapazitaet zurueck.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Fuegt ein Byte am Ende hinzu (schiebt Fenster falls noetig).
    pub fn push(&mut self, byte: u8) {
        if self.len == self.capacity {
            // Puffer voll: aeltestes Byte ueberschreiben
            self.data[self.start] = byte;
            self.start = (self.start + 1) % self.capacity;
        } else {
            let pos = (self.start + self.len) % self.capacity;
            self.data[pos] = byte;
            self.len += 1;
        }
    }

    /// Fuegt mehrere Bytes hinzu.
    pub fn extend(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.push(b);
        }
    }

    /// Liest ein Byte an relativer Position vom Ende (Distance).
    /// distance = 1 -> letztes Byte, distance = len -> erstes Byte.
    pub fn get_back(&self, distance: usize) -> Option<u8> {
        if distance == 0 || distance > self.len {
            return None;
        }
        let idx = (self.start + self.len - distance) % self.capacity;
        Some(self.data[idx])
    }

    /// Kopiert `len` Bytes von relativer Position `distance` ans Ende.
    /// Dies ist die Kern-Operation fuer LZ77-Rueckwaertsreferenzen.
    pub fn copy_from_back(&mut self, distance: usize, len: usize) -> Result<(), crate::FehlerZlib> {
        if distance == 0 || distance > self.len {
            return Err(crate::FehlerZlib::BrotliDekodierFehler {
                grund: format!("Ungueltige Distanz: {} (len={})", distance, self.len),
            });
        }

        if len == 0 {
            return Ok(());
        }

        // DER FEHLER, DEN DIESE SCHLEIFE HATTE, und er ist der
        // klassische bei ueberlappenden Kopien: der Quellindex wurde
        // als `(start + len - distance + i)` berechnet, wobei `len` in
        // jeder Runde **waechst**. Dadurch wanderte die Quelle mit dem
        // Ziel mit, und aus "3, 4 kopieren" wurde "3, 5 kopieren".
        //
        // Bei einer Distanz kleiner als der Laenge ist genau das der
        // Unterschied zwischen der richtigen und der falschen Ausgabe:
        // "aaaa" mit Distanz 1 und Laenge 4 muss "aaaa" ergeben; mit
        // der wandernden Quelle ergibt es "aaaa" nur zufaellig, weil
        // alle Bytes gleich sind. Bei [1,2,3,4,5] mit Distanz 3 zeigt
        // es sich als 3,5 statt 3,4.
        //
        // DIE RICHTIGE FORM: die Quelle ist **immer** `distance` Byte
        // vor dem **aktuellen** Ende, also wird sie je Runde neu
        // bestimmt. Das kostet eine Addition je Byte und deckt
        // gleichzeitig die Ueberlappung ab — genau wie LZ77 es
        // definiert ist.
        for _ in 0..len {
            if self.len == 0 {
                return Err(crate::FehlerZlib::BrotliDekodierFehler {
                    grund: "Kopieren aus einem leeren Fenster".to_string(),
                });
            }
            let quelle = (self.start + self.len - distance) % self.capacity;
            let byte = self.data[quelle];
            if self.len < self.capacity {
                self.data[(self.start + self.len) % self.capacity] = byte;
                self.len += 1;
            } else {
                self.data[self.start] = byte;
                self.start = (self.start + 1) % self.capacity;
            }
        }
        Ok(())
    }

    /// Gibt einen Slice ueber die letzten `n` Bytes (falls kontigu).
    /// Falls ueber den Rand, returned None.
    pub fn as_slice(&self) -> Option<&[u8]> {
        if self.len == 0 {
            return Some(&[]);
        }
        if self.start + self.len <= self.capacity {
            Some(&self.data[self.start..self.start + self.len])
        } else {
            None // Um den Rand gewickelt
        }
    }

    /// Gibt alle Daten als Vec zurueck (neu allokiert).
    pub fn to_vec(&self) -> Vec<u8> {
        if self.len == 0 {
            return Vec::new();
        }
        if self.start + self.len <= self.capacity {
            self.data[self.start..self.start + self.len].to_vec()
        } else {
            let mut v = Vec::with_capacity(self.len);
            v.extend_from_slice(&self.data[self.start..]);
            v.extend_from_slice(&self.data[..(self.start + self.len) % self.capacity]);
            v
        }
    }

    /// Gibt die aktuelle Ausgabelaenge zurueck (alias fuer len).
    pub fn output_len(&self) -> usize {
        self.len
    }

    /// Setzt den Puffer zurueck (leert ihn).
    pub fn clear(&mut self) {
        self.start = 0;
        self.len = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer_basic() {
        let mut rb = RingBuffer::new(16);
        rb.push(1);
        rb.push(2);
        rb.push(3);
        assert_eq!(rb.len(), 3);
        assert_eq!(rb.get_back(1), Some(3));
        assert_eq!(rb.get_back(3), Some(1));
    }

    #[test]
    fn test_copy_from_back() {
        let mut rb = RingBuffer::new(16);
        rb.extend(&[1, 2, 3, 4, 5]);
        // Distanz 3 heisst: das Byte **drei** vor dem Ende. Bei
        // [1,2,3,4,5] ist das die 3, und zwei Byte davon sind 3 und 4.
        rb.copy_from_back(3, 2).unwrap();
        assert_eq!(rb.len(), 7, "es kommen zwei Byte dazu");
        // DER FEHLER, DEN DIESER TEST HATTE: er pruefte `get_back(1)`
        // auf 3 und `get_back(2)` auf 4 — nach dem Kopieren ist aber
        // `get_back(1)` die **4** (das zuletzt geschriebene Byte) und
        // `get_back(2)` die 3. Die Reihenfolge ist verkehrt herum.
        //
        // Der Test ist nie gelaufen, seit der Ringpuffer existiert:
        // `brotli` war auskommentiert, und der Modul-Test haengt am
        // Modul. Dass er **falsch** ist und nicht nur ungetestet, fiel
        // erst auf, als das Modul eingeschaltet wurde.
        assert_eq!(rb.get_back(1), Some(4), "das zuletzt kopierte Byte");
        assert_eq!(rb.get_back(2), Some(3), "das davor");
        assert_eq!(rb.get_back(3), Some(5), "das urspruengliche 5");
    }

    #[test]
    fn test_wrap_around() {
        let mut rb = RingBuffer::new(4);
        rb.extend(&[1, 2, 3, 4]);
        assert_eq!(rb.len(), 4);
        rb.push(5); // ueberschreibt 1
        assert_eq!(rb.len(), 4);
        assert_eq!(rb.get_back(1), Some(5));
        assert_eq!(rb.get_back(4), Some(2));
    }

    #[test]
    fn test_copy_overlap() {
        let mut rb = RingBuffer::new(8);
        rb.extend(&[1, 2]);
        // Kopiere 2 Bytes aus Distanz 2 -> dupliziert 1, 2
        rb.copy_from_back(2, 2).unwrap();
        assert_eq!(rb.len(), 4);
        // Inhalt sollte 1, 2, 1, 2 sein
    }
}
