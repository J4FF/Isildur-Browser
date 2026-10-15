//! Der Ton: PCM je Kanal — das, was **alle** Dekoder liefern.
//!
//! # WARUM EINE EIGENE STRUKTUR UND NICHT `vorbis::Ton`
//!
//! Vorbis hat als Zusatz eine `granel`-Position (die Byte-Zahl der
//! letzten Ogg-Seite), weil das seeking ueber die Seite laeuft. MP3 hat
//! keinen Granel, FLAC einen Frame-Zaehler, WAV gar nichts — und eine
//! Struktur, die drei Dekoder bedienen muessen, darf nicht die
//! Besonderheit eines vierten tragen.
//!
//! Der Weg ist einfach: **Speicher** = Samples je Kanal, **Rate** =
//! Abtastrate in Hz, **kanaele_daten** = ein `Vec<f64>` je Kanal. Das ist
//! genau das, was `aplay -f s16le` braucht, und was die SNR-Messung gegen
//! `ffmpeg -f s16le` vergleicht.

use crate::fehler::{Art, Ergebnis, Fehler};

/// Der dekodierte Ton.
#[derive(Debug, Clone, PartialEq)]
pub struct Ton {
    /// Die Abtastrate in Hz.
    pub rate: u32,
    /// Die Kanalzahl (1 oder 2).
    pub kanaele: usize,
    /// Die Zahl der Samples je Kanal.
    pub samples: usize,
    /// Die Samples je Kanal, jeder Kanal fuer sich.
    pub kanaele_daten: Vec<Vec<f64>>,
}

impl Ton {
    /// Der Ton aus Kanaeldaten, mit的一致en Laengen.
    pub fn neu(rate: u32, kanaele_daten: Vec<Vec<f64>>) -> Ergebnis<Self> {
        let samples = kanaele_daten.first().map_or(0, Vec::len);
        if kanaele_daten.iter().any(|k| k.len() != samples) {
            return Err(Fehler::neu(
                Art::FalscherWert,
                "die Kanaele haben verschiedene Laengen — das kann kein Ton sein",
            ));
        }
        Ok(Self {
            rate,
            kanaele: kanaele_daten.len(),
            samples,
            kanaele_daten,
        })
    }

    /// Der Ton aus einem **verflochten** Puffer (Frame-weise: alle
    /// Kanaele nebeneinander).
    ///
    /// # DIE UMORDNUNG, und sie ist nicht kosmetisch
    ///
    /// WAV liefert „frameweise": Frame 0 Kanal 0, Frame 0 Kanal 1, Frame
    /// 1 Kanal 0, … `aplay -f s16le` erwartet **pro Sample alle Kanaele
    /// nebeneinander**, was dasselbe ist — aber unsere SNR-Messung und
    /// die Kanalweise-Ausgabe wollen „alle Samples von Kanal 0, dann alle
    /// von Kanal 1". Das ist ein echtes Vertauschen, kein Formatieren:
    /// geht man die Schleife in der falschen Reihenfolge, klingt Stereo
    /// wie Mono mit einer Phasenverzerrung, und die Zahl bleibt
    /// unauffaellig.
    pub fn aus_verflochten(rate: u32, kanaele: usize, roh: &[f64]) -> Ergebnis<Self> {
        if kanaele == 0 {
            return Err(Fehler::neu(Art::FalscherWert, "kein Kanal"));
        }
        if roh.len() % kanaele != 0 {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!(
                    "{} Werte passen nicht auf {kanaele} Kanaele — die Datei ist abgeschnitten",
                    roh.len()
                ),
            ));
        }
        let frames = roh.len() / kanaele;
        let mut raus = vec![Vec::with_capacity(frames); kanaele];
        for (i, &w) in roh.iter().enumerate() {
            raus[i % kanaele].push(w);
        }
        Self::neu(rate, raus)
    }

    /// Die Samples als `i16`, **kanalweise hintereinander** — die
    /// Reihenfolge, die `aplay -f s16le` erwartet.
    #[must_use]
    pub fn als_i16_je_kanal(&self) -> Vec<Vec<i16>> {
        self.kanaele_daten
            .iter()
            .map(|k| {
                k.iter()
                    .map(|&w| (w * 32768.0).round().clamp(-32768.0, 32767.0) as i16)
                    .collect()
            })
            .collect()
    }

    /// Die Spitzenspitze je Kanal.
    #[must_use]
    pub fn spitzen(&self) -> Vec<f64> {
        self.kanaele_daten
            .iter()
            .map(|k| k.iter().fold(0.0f64, |m, w| m.max(w.abs())))
            .collect()
    }
}
