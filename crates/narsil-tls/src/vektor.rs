//! Bounded Lesen und Schreiben der TLS-1.3-Vektoren.
//!
//! Fast jede Fehlstelle in TLS 1.3 ist eine Laengenangabe. Deshalb gibt
//! es hier nur EINE Stelle, an der ein Laengenfeld gelesen wird, und dort
//! wird immer zuerst geprueft, ob der Puffer die Laenge hergibt, BEVOR
//! irgendetwas alloziert wird (M4).
//!
//! M3: `Leser` und `Schreiber` laufen ueber `pos`, und `pos` kann nie
//! groesser werden als die Laenge, die vorher gegen die Pufferlaenge
//! geprueft wurde. Es gibt keine `while`-Schleife ueber eine Laenge von
//! aussen.

use crate::record::{MAX_CIPHERTEXT, MAX_NACHRICHT, RecordFehler};

/// Warum ein Vektor nicht lesbar oder schreibbar ist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VektorFehler {
    /// Der Puffer endet vor dem Ende des Vektors.
    ZuKurz { erwartet: usize, vorhanden: usize },
    /// Das Laengenfeld ist groesser als das, was ein TLS-1.3-Feld
    /// aufnehmen kann: 2^16 - 1 fuer einen Vektor mit 16-Bit-Laenge,
    /// 2^24 - 1 fuer eine Handshake-Nachricht.
    LaengeUeberGrenze { angegeben: usize, erlaubt: usize },
    /// Die Laenge passt, aber der Inhalt ist zu kurz.
    InhaltZuKurz { angesagt: usize, vorhanden: usize },
}

impl std::fmt::Display for VektorFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VektorFehler::ZuKurz {
                erwartet,
                vorhanden,
            } => {
                write!(f, "erwartet {erwartet} Byte, vorhanden {vorhanden}")
            }
            VektorFehler::LaengeUeberGrenze { angegeben, erlaubt } => {
                write!(f, "Laenge {angegeben} ueber der Grenze {erlaubt}")
            }
            VektorFehler::InhaltZuKurz {
                angesagt,
                vorhanden,
            } => write!(f, "Vektor sagt {angesagt} Byte, da sind nur {vorhanden}"),
        }
    }
}

impl std::error::Error for VektorFehler {}

/// Liest Felder aus einem Byte-Puffer, ohne zu raten und ohne zu
/// allozieren, bevor die Laenge geprueft ist.
pub struct Leser<'a> {
    daten: &'a [u8],
    pos: usize,
}

impl<'a> Leser<'a> {
    pub fn neu(daten: &'a [u8]) -> Self {
        Leser { daten, pos: 0 }
    }

    /// Wie viel noch da ist.
    pub fn uebrig(&self) -> usize {
        self.daten.len() - self.pos
    }

    /// Ist nichts mehr da?
    pub fn ist_leer(&self) -> bool {
        self.uebrig() == 0
    }

    /// Ein Byte, ohne Laengenfeld.
    pub fn u8(&mut self) -> Result<u8, VektorFehler> {
        if self.uebrig() < 1 {
            return Err(VektorFehler::ZuKurz {
                erwartet: 1,
                vorhanden: self.uebrig(),
            });
        }
        let b = self.daten[self.pos];
        self.pos += 1;
        Ok(b)
    }

    /// Zwei Byte, ohne Laengenfeld.
    //
    // DER FEHLER, DEN DIESER BLOCK HATTE: das `u32` stand hier, im
    // **Leser** — der Leser hat aber kein `u32` zum Schreiben, sondern
    // eines zum **Lesen**, und das hiess `pub fn u32(&mut self) ->
    // Result<u32, VektorFehler>`. Zwei gleichnamige Methoden in zwei
    // `impl`-Bloecken sind fuer den Aufrufer nicht zu unterscheiden; der
    // Compiler meldete `no method named u32 found for struct Schreiber`,
    // und die Meldung sah nach einem fehlenden Feature aus statt nach
    // einer Methode am falschen Typ. Das Schreiben gehoert in den
    // `Schreiber`.
    pub fn u16(&mut self) -> Result<u16, VektorFehler> {
        if self.uebrig() < 2 {
            return Err(VektorFehler::ZuKurz {
                erwartet: 2,
                vorhanden: self.uebrig(),
            });
        }
        let w = u16::from_be_bytes([self.daten[self.pos], self.daten[self.pos + 1]]);
        self.pos += 2;
        Ok(w)
    }

    /// `laenge` Byte, ohne Laengenfeld.
    pub fn block(&mut self, laenge: usize) -> Result<&'a [u8], VektorFehler> {
        if self.uebrig() < laenge {
            return Err(VektorFehler::ZuKurz {
                erwartet: laenge,
                vorhanden: self.uebrig(),
            });
        }
        let r = &self.daten[self.pos..self.pos + laenge];
        self.pos += laenge;
        Ok(r)
    }

    /// Ein Vektor mit 16-Bit-Laengenfeld.
    ///
    /// M4: Das Laengenfeld wird gelesen, gegen `MAX_CIPHERTEXT` geprueft
    /// und gegen `uebrig()` geprueft — und erst dann wird der Ausschnitt
    /// genommen. Ohne diese Reihenfolge wuerde ein Bytekumpen im Laengen-
    /// feld einen Puffer der Laenge 65535 allozieren, den es nicht gibt.
    pub fn vektor16(&mut self) -> Result<&'a [u8], VektorFehler> {
        let laenge = self.u16()? as usize;
        if laenge > MAX_CIPHERTEXT {
            return Err(VektorFehler::LaengeUeberGrenze {
                angegeben: laenge,
                erlaubt: MAX_CIPHERTEXT,
            });
        }
        self.block(laenge)
    }

    /// Ein Vektor mit 8-Bit-Laengenfeld. Fuer Cipher-Suite-Listen,
    /// Kompressionsverfahren und Signaturalgorithmen.
    pub fn vektor8(&mut self) -> Result<&'a [u8], VektorFehler> {
        let laenge = self.u8()? as usize;
        // 255 ist die Grenze eines Ein-Byte-Feldes, also nie verletzt.
        // Die Pruefung steht trotzdem da, damit der Grenzgedanke an
        // JEDER Laengenstelle steht (M4).
        if laenge > u8::MAX as usize {
            return Err(VektorFehler::LaengeUeberGrenze {
                angegeben: laenge,
                erlaubt: u8::MAX as usize,
            });
        }
        self.block(laenge)
    }

    /// Ein Vektor mit 24-Bit-Laengenfeld (Handshake-Nachricht).
    pub fn vektor24(&mut self) -> Result<&'a [u8], VektorFehler> {
        if self.uebrig() < 3 {
            return Err(VektorFehler::ZuKurz {
                erwartet: 3,
                vorhanden: self.uebrig(),
            });
        }
        let laenge = ((self.daten[self.pos] as usize) << 16)
            | ((self.daten[self.pos + 1] as usize) << 8)
            | (self.daten[self.pos + 2] as usize);
        self.pos += 3;
        if laenge > MAX_NACHRICHT {
            return Err(VektorFehler::LaengeUeberGrenze {
                angegeben: laenge,
                erlaubt: MAX_NACHRICHT,
            });
        }
        self.block(laenge)
    }

    /// Alle restlichen Byte, ohne Laengenangabe.
    pub fn rest(&mut self) -> &'a [u8] {
        let r = &self.daten[self.pos..];
        self.pos = self.daten.len();
        r
    }
}

/// Schreibt Felder in einen Puffer. Die Laengen werden VOR dem
/// Fuellen geschrieben, damit man sie nicht nachzaehlen muss.
#[derive(Default)]
pub struct Schreiber {
    buf: Vec<u8>,
}

impl Schreiber {
    pub fn neu() -> Self {
        Schreiber { buf: Vec::new() }
    }

    /// Wie viel bisher drinsteht. Wird gebraucht, um Laengenfelder zu
    /// fuellen.
    pub fn laenge(&self) -> usize {
        self.buf.len()
    }

    pub fn u8(&mut self, w: u8) {
        self.buf.push(w);
    }

    /// Ein `uint32` in Netzwerk-Byteordnung.
    ///
    /// added 02.10.2026, R4d Stufe 2: es fehlte, und `NewSessionTicket`
    /// hat zwei davon (`ticket_lifetime`, `ticket_age_add`, RFC 8446
    /// 4.6.1). DER GRUND EINER EIGENEN METHODE und nicht `u16` zweimal:
    /// die Fehlerform ist das **Vertauschen der Haelften**. `(hi << 16) |
    /// lo` an zwei Aufrufstellen ist richtig, an einer dritten mit
    /// vertauschten Argumenten aber nicht mehr zu erkennen.
    ///
    /// DER WEG UEBER ZWEI `u16`-Aufrufe und nicht ueber `extend_from_slice`
    /// mit vier Byte: der Schreiber kennt seine eigene Reihenfolge
    /// (`Schreiber` schreibt immer gross-endian), und ein Aufrufer, der
    /// vier Byte selbst legt, muss sie kennen.
    pub fn u32(&mut self, wert: u32) {
        self.u16((wert >> 16) as u16);
        self.u16((wert & 0xffff) as u16);
    }

    pub fn u16(&mut self, w: u16) {
        self.buf.extend_from_slice(&w.to_be_bytes());
    }

    pub fn bytes(&mut self, w: &[u8]) {
        self.buf.extend_from_slice(w);
    }

    /// Ein Vektor mit 16-Bit-Laengenfeld.
    pub fn vektor16(&mut self, inhalt: &[u8]) {
        self.u16(inhalt.len() as u16);
        self.bytes(inhalt);
    }

    /// Ein Vektor mit 8-Bit-Laengenfeld.
    pub fn vektor8(&mut self, inhalt: &[u8]) {
        self.u8(inhalt.len() as u8);
        self.bytes(inhalt);
    }

    /// Ein Vektor mit 24-Bit-Laengenfeld.
    pub fn vektor24(&mut self, inhalt: &[u8]) {
        self.buf.push((inhalt.len() >> 16) as u8);
        self.buf.push((inhalt.len() >> 8) as u8);
        self.buf.push(inhalt.len() as u8);
        self.bytes(inhalt);
    }

    /// Der fertige Puffer.
    pub fn fertig(&self) -> &[u8] {
        &self.buf
    }

    /// Der Puffer als eigener Byte-Vektor.
    pub fn uebernehmen(self) -> Vec<u8> {
        self.buf
    }
}

/// Der Wandler zwischen den beiden Fehlerarten. Praktisch, damit die
/// Aufrufer nicht zweimal umrechnen muessen.
impl From<VektorFehler> for RecordFehler {
    fn from(e: VektorFehler) -> RecordFehler {
        match e {
            VektorFehler::ZuKurz {
                erwartet,
                vorhanden,
            }
            | VektorFehler::InhaltZuKurz {
                angesagt: erwartet,
                vorhanden,
            } => RecordFehler::Abgeschnitten {
                erwartet,
                vorhanden,
            },
            VektorFehler::LaengeUeberGrenze { angegeben, erlaubt } => {
                RecordFehler::LaengeZuGross { angegeben, erlaubt }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// M7: der Leser liefert genau das, was geschrieben wurde.
    #[test]
    fn rundlauf_kleiner_vektoren() {
        let mut s = Schreiber::neu();
        s.u8(0x16);
        s.u16(0x0303);
        s.vektor8(&[1, 2, 3]);
        s.vektor16(&[4, 5, 6, 7]);
        s.vektor24(&[8, 9]);
        let bytes = s.uebernehmen();

        let mut l = Leser::neu(&bytes);
        assert_eq!(l.u8().expect("u8"), 0x16);
        assert_eq!(l.u16().expect("u16"), 0x0303);
        assert_eq!(l.vektor8().expect("v8"), &[1, 2, 3]);
        assert_eq!(l.vektor16().expect("v16"), &[4, 5, 6, 7]);
        assert_eq!(l.vektor24().expect("v24"), &[8, 9]);
        assert!(l.ist_leer(), "M7: nichts mehr uebrig");
    }

    /// M4: ein Laengenfeld, das mehr sagt, als der Puffer hat. Der
    /// Fehler nennt BEIDE Zahlen.
    #[test]
    fn laenge_groesser_als_der_puffer() {
        // 16-Bit-Feld sagt 1000, es sind aber 3 Byte da.
        let mut l = Leser::neu(&[0x03, 0xe8, 0x00]);
        assert_eq!(
            l.vektor16(),
            Err(VektorFehler::ZuKurz {
                erwartet: 1000,
                vorhanden: 1
            }),
            "M7: Fehler mit beiden Zahlen"
        );
    }

    /// M4: ein Laengenfeld ueber 2^14 + 256 wird abgelehnt, und es wird
    /// NICHTS alloziert.
    #[test]
    fn laenge_ueber_die_grenze() {
        let mut l = Leser::neu(&[0xff, 0xff, 0x00]);
        assert_eq!(
            l.vektor16(),
            Err(VektorFehler::LaengeUeberGrenze {
                angegeben: 65535,
                erlaubt: MAX_CIPHERTEXT
            })
        );
        // Knapp darunter geht es, der Puffer ist aber zu kurz.
        let mut l = Leser::neu(&[0x41, 0x00]);
        assert!(matches!(l.vektor16(), Err(VektorFehler::ZuKurz { .. })));
    }

    /// M4: das 24-Bit-Feld aus einem Bytekumpen.
    #[test]
    fn laenge_24_bit_aus_einem_bytekumpen() {
        let mut l = Leser::neu(&[0xff, 0xff, 0xff]);
        assert_eq!(
            l.vektor24(),
            Err(VektorFehler::ZuKurz {
                erwartet: MAX_NACHRICHT,
                vorhanden: 0
            })
        );
    }

    /// M3: der Leser laeuft nie ueber den Puffer hinaus, egal wie oft
    /// `block` mit einer grossen Laenge aufgerufen wird.
    #[test]
    fn tausendmal_zu_grosses_block() {
        let daten = [1u8, 2, 3, 4, 5];
        let mut l = Leser::neu(&daten);
        for _ in 0..1000 {
            assert!(l.block(1000).is_err());
        }
        // Und die Position ist unveraendert, es wurde nichts verbraucht.
        assert_eq!(l.uebrig(), 5, "M7: Position unveraendert nach Fehlern");
        assert_eq!(l.rest().len(), 5, "M7: alles noch da");
    }
}
