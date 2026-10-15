//! Der Bildstrom eines Videos: Bild fuer Bild, mit Referenz.
//!
//! GEMESSENE ENTSCHEIDUNG, warum ueberhaupt ein Bildstrom noetig ist und
//! nicht ein Aufruf je Paket: RFC 6386, 9.1 — das erste Bit des Bildes
//! sagt, ob es ein Schluesselbild ist oder ein Zwischenbild. Bei
//! `g7b_testsrc.webm` tragen 1 von 20 Bildern eine Null dort (GEMESSEN mit
//! `examples/video_strom_messung`). Ein Zwischenbild hat KEINE Pixel
//! selbst, sondern einen Bewegungsvektor auf ein frueheres Bild; ohne
//! dieses Bild gibt es nichts zu rekonstruieren.
//!
//! GEMESSENER BEFUND, der diesen Weg erzwungen hat (03.10.2026, VIDEO):
//! `VideoPlayer::push_frame` rief `vp8_lesen` je Paket auf. Damit bekam
//! jedes Zwischenbild eine LEERE Referenz und wurde trotzdem als Bild
//! geliefert — ein Bild, das die Form des richtigen hat und an keiner
//! Stelle das richtige ist. Der Auftrag nennt genau das als offenen Punkt
//! („Zwischenbilder in vp8.rs sind der grosse offene Punkt"), und die
//! Reparatur gehoert an beide Enden: in den Decoder (vp8.rs) und hier in
//! den Weg, der den Decoder aufruft.

use narsil_image::vp8::vp8_ebenen_im_strom;
use narsil_image::vp8::vp8_lesen_aus_strom;
use narsil_image::vp8::Bildstrom;
use narsil_image::Bild;

/// Wie viele Bilder der Puffer haelt.
///
/// GEMESSENE ENTSCHEIDUNG, warum drei und nicht mehr: die Crate soll den
/// Speicher begrenzen (M1), und drei Bilder reichen fuer jede Bewegung,
/// die man im Bild sieht. Wer mehr will, soll es sagen und begruenden
/// koennen.
pub const MAX_BILDER: usize = 3;

/// Der Puffer eines Videos: die letzten Bilder und der Zustand, aus dem
/// das naechste Zwischenbild gerechnet wird.
///
/// `#[derive(Debug, Clone, Default)]` ist hier ausdruecklich NICHT
/// noetig und auch nicht moeglich: `Bildstrom` traegt drei Referenzbilder,
/// jedes so gross wie das Bild. Eine Kopie waere eine Kopie des Bildes.
pub struct VideoPlayer {
    /// Die fertigen Bilder, das juengste hinten.
    buffer: std::collections::VecDeque<Bild>,
    /// Der Zustand, aus dem das naechste Zwischenbild gerechnet wird.
    /// Ohne ihn kann ein Zwischenbild nicht gerechnet werden (9.1).
    strom: Bildstrom,
    /// Wie viele Bilder seit dem letzten Schluesselbild abgewiesen wurden,
    /// weil ihre Referenz fehlte. Das ist eine Zahl und kein Fehler:
    /// ein Video darf mit einem Zwischenbild anfangen.
    ohne_referenz: u32,
}

impl VideoPlayer {
    /// Ein neuer Spieler ohne Bilder und ohne Referenzen.
    pub fn new() -> Self {
        Self {
            buffer: std::collections::VecDeque::with_capacity(MAX_BILDER),
            strom: Bildstrom::neu(),
            ohne_referenz: 0,
        }
    }

    /// Nimmt ein VP8-Bild auf und liefert, ob es dekodierbar war.
    ///
    /// `daten` ist die Nutzlast eines VP8-Bildes, so wie
    /// `webm::extract_packets` sie liefert — nicht die ganze WebM-Datei.
    ///
    /// DER WEG, und warum er so lang ist: `vp8_ebenen_im_strom` rechnet
    /// die drei Ebenen und setzt die Referenzen des Bildstroms;
    /// `vp8_lesen_aus_strom` macht daraus ein `Bild`. Beides ist
    /// getrennt, weil RFC 6386, 14 die Ebenen als Ergebnis festlegt und
    /// die Farbumrechnung nicht dazugehoert — eine Seite, die eigene
    /// Anzeigefarben braucht, nimmt sich die Ebenen.
    ///
    /// EIN FEHLER IST KEIN BILD: ein Zwischenbild ohne Referenz, ein
    /// abgeschnittenes Paket, eine kaputte Partition — das gibt `false`
    /// zurueck und laesst den Puffer unberuehrt. Ein Ersatzbild waere
    /// geraten, und geraten heisst hier: das vorherige Bild noch einmal
    /// zeigen und damit behaupten, es sei das naechste.
    pub fn push_frame(&mut self, daten: &[u8]) -> bool {
        match vp8_ebenen_im_strom(daten, &mut self.strom) {
            Ok(e) => match vp8_lesen_aus_strom(&e) {
                Ok(b) => {
                    if self.buffer.len() >= MAX_BILDER {
                        self.buffer.pop_front();
                    }
                    self.buffer.push_back(b);
                    true
                }
                // Die Ebenen sind gerechnet, die Farbe nicht: das ist ein
                // Fehler im Bild, kein Fehler im Video. Das naechste Bild
                // kann trotzdem kommen.
                Err(_) => false,
            },
            Err(_) => {
                self.ohne_referenz = self.ohne_referenz.saturating_add(1);
                false
            }
        }
    }

    /// Das juengste Bild, oder `None`, wenn noch keines da ist.
    pub fn current_frame(&self) -> Option<&Bild> {
        self.buffer.back()
    }

    /// Wie viele Bilder der Puffer haelt. Nach einem VIDEO mit
    /// Schluesselbild ist das `min(gepushte Bilder, MAX_BILDER)`.
    pub fn anzahl(&self) -> usize {
        self.buffer.len()
    }

    /// Wie viele Bilder seit dem letzten Schluesselbild NICHT gerechnet
    /// werden konnten. GEMESSEN an `g7b_testsrc.webm`: ein Spieler, der
    /// mit Paket 1 beginnt (einem Zwischenbild), zaehlt hier 1 und liefert
    /// 0 Bilder — das ist die Antwort auf die Frage „kann das Video
    /// beliebig anlaufen?", und sie lautet: nein.
    pub fn ohne_referenz(&self) -> u32 {
        self.ohne_referenz
    }

    /// Verwirft alle Bilder und alle Referenzen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es das gibt: nach einem
    /// seeking-Sprung kommt ein Schluesselbild an einer ganz anderen Stelle.
    /// Ohne diesen Weg wuerde der Spieler ein Zwischenbild gegen eine
    /// Referenz rechnen, die drei Sekunden alt ist — das Bild ist dann
    /// weder richtig noch gleichbleibend falsch.
    pub fn zuruecksetzen(&mut self) {
        self.buffer.clear();
        self.strom = Bildstrom::neu();
        self.ohne_referenz = 0;
    }
}

impl Default for VideoPlayer {
    fn default() -> Self {
        Self::new()
    }
}