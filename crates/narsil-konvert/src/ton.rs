//! TON — der Wertspeicher, den alle Audio-Enkoder bekommen.
//!
//! Warum ein eigener Typ statt `Vec<u8>`: ein Enkoder muss drei Dinge
//! wissen, die in einem Byte-Strom nicht stehen — die Abtastrate, die Zahl
//! der Kanaele, und wie tief ein Wert wirklich ist. Ohne die Bit-Tiefe
//! kann FLAC nicht in den STREAMINFO schreiben, und ohne sie kann WAV nicht
//! zwischen PCM 16 und PCM 24 waehlen.
//!
//! # Warum GANZZAHLIG und nicht `f32`
//!
//! FLAC ist verlustfrei, und „verlustfrei" heisst: nach der Rundreise ist
//! jeder Wert BITGLEICH. Das ist mit Gleitkomma nicht erreichbar, weil
//! jede Umrechnung rundet. Also ganzzahlig, mit einer ausdruecklich
//! genannten Tiefe.
//!
//! # Kanaele
//!
//! Werte liegen **interleaved** vor: `werte[k*kanale + c]` ist Kanal `c`
//! des Frames `k`. Das ist die Reihenfolge, die WAV und FLAC im Datenstrom
//! benutzen, also muss der Enkoder nichts umsortieren — und die
//! Kanalanzahl steht in der Datei, nicht in diesem Container.

use crate::{KonvertFehler, grenzen};

/// Abtastrate in Hz.
///
/// 8000 ist die untere sinnvolle Grenze (darunter kann man nichts
/// hoeren), 192000 die obere (Studio-PCM, 6-kanalig). Dazwischen liegen
/// alle praktisch vorkommenden Raten: 8000, 11025, 16000, 22050, 32000,
/// 44100, 48000, 88200, 96000, 176400, 192000.
pub const MIN_RATE: u32 = 8_000;
pub const MAX_RATE: u32 = 192_000;

/// Hoechste Kanalzahl. 8 deckt 7.1 ab; mehr waere kein Ton mehr, sondern
/// eine Datensammlung, und WAV hat fuer die Kanalzahl nur ein 2-Byte-Feld
/// mit den belegten_mask_bits aus 32.
pub const MAX_KANALE: u8 = 8;

/// Kleinste Bittiefe, die FLAC erlaubt (4 Bit ist der kleinste
/// unterstuetzte Subframe-Typ CONSTANT mit 4 Bit Bestaetigung).
pub const MIN_BIT_TIEFE: u8 = 4;

/// Groesste Bittiefe, die FLAC erlaubt (32 Bit ist die hoechste, und
/// zugleich die Grenze fuer den Zufallsverteiler in der Frame-Kopfzeile).
pub const MAX_BIT_TIEFE: u8 = 32;

/// Tonproben, interleaved, mit ausdruecklicher Bit-Tiefe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pcm {
    /// Abtastrate in Hz.
    pub rate: u32,
    /// Kanaele (1 = mono, 2 = stereo).
    pub kanale: u8,
    /// Bittiefe je Wert. FLAC erlaubt 4..=32; WAV nur 16, 24 oder 32.
    pub bit_tiefe: u8,
    /// Die Werte, interleaved: `werte[k * kanale + c]`.
    ///
    /// Zugestellter Wertebereich: `-2^(bit_tiefe-1) ..= 2^(bit_tiefe-1)-1`.
    /// Das wird beim Bauen geprueft, nicht beim Lesen — ein `Pcm`, das
    /// hier steht, ist gueltig.
    pub werte: Vec<i32>,
}

impl Pcm {
    /// Baut PCM aus ganzzahligen Werten und prueft **alles** (M3/M4).
    ///
    /// # Fehler
    /// - [`KonvertFehler::TonRateFalsch`] bei 0 oder ueber [`MAX_RATE`].
    /// - [`KonvertFehler::TonKanaleFalsch`] bei 0 oder ueber [`MAX_KANALE`].
    /// - [`KonvertFehler::TonTiefeFalsch`] unter [`MIN_BIT_TIEFE`] oder
    ///   ueber [`MAX_BIT_TIEFE`].
    /// - [`KonvertFehler::TonWertAusserhalb`] wenn ein Wert nicht in den
    ///   zugestellten Bereich passt. GEMESSEN: das kommt bei
    ///   `aus_bytes_le` mit zu tiefen Bytes vor, und ohne diese Pruefung
    ///   waere die Datei gueltig und der Inhalt Unsinn.
    /// - [`KonvertFehler::ZuVielePunkte`] wenn `kanale` die Zahl der Werte
    ///   nicht teilt.
    pub fn neu(
        rate: u32,
        kanale: u8,
        bit_tiefe: u8,
        werte: Vec<i32>,
    ) -> Result<Self, KonvertFehler> {
        if !(MIN_RATE..=MAX_RATE).contains(&rate) {
            return Err(KonvertFehler::TonRateFalsch { rate });
        }
        if kanale == 0 || kanale > MAX_KANALE {
            return Err(KonvertFehler::TonKanaleFalsch { kanale });
        }
        if !(MIN_BIT_TIEFE..=MAX_BIT_TIEFE).contains(&bit_tiefe) {
            return Err(KonvertFehler::TonTiefeFalsch { bit_tiefe });
        }
        if werte.len() % usize::from(kanale) != 0 {
            return Err(KonvertFehler::ZuVielePunkte {
                punkte: werte.len() as u64,
                grenze: (werte.len() / usize::from(kanale).max(1)) as u64 * u64::from(kanale),
            });
        }
        grenzen::pruefe_laenge(
            werte.len(),
            grenzen::MAX_TON_PUNKTE,
            "PCM-Werte (Kanale mal Frames)",
        )?;
        // ACHTUNG, HIER LAG EIN FEHLER (gefunden 02.10.2026 von
        // `wav_mono_und_pcm32_gehen_durch_ffmpeg`: „attempt to negate
        // with overflow" in `ton.rs:100`).
        //
        // Hier stand `let hoch = 1i32 << (bit_tiefe - 1); let runter =
        // -hoch;`. Bei `bit_tiefe == 32` ist das `1i32 << 31` — das
        // ueberlaeuft schon bei der Schiebung, und `-hoch` laeuft
        // ebenfalls ueber. Im Debug-Bau ein Panic, im Release ein
        // Falschergebnis.
        //
        // Also in i64 rechnen. `1i64 << 31` passt, und der Vergleich
        // `i64::from(w) >= hoch` ist fuer 32-Bit-Werte richtig: der
        // hoechste zulaessige Wert ist 2^31-1, der kleinste -2^31.
        let hoch = 1i64 << (bit_tiefe - 1);
        let runter = -hoch;
        if let Some(&falsch) = werte
            .iter()
            .find(|&&w| i64::from(w) < runter || i64::from(w) >= hoch)
        {
            return Err(KonvertFehler::TonWertAusserhalb {
                wert: falsch,
                bit_tiefe,
            });
        }
        Ok(Pcm {
            rate,
            kanale,
            bit_tiefe,
            werte,
        })
    }

    /// Baut aus 16-Bit-Werten (der haeufigste Fall: das kommt aus jedem
    /// WAV, das der Browser abspielt).
    pub fn aus_i16(rate: u32, kanale: u8, werte: Vec<i16>) -> Result<Self, KonvertFehler> {
        Pcm::neu(rate, kanale, 16, werte.into_iter().map(i32::from).collect())
    }

    /// Baut aus 24-Bit-Werten. Das sind keine `i32` mit 24 Bit Fuellung,
    /// sondern Werten von -8 388 608 bis 8 388 607.
    pub fn aus_i24(rate: u32, kanale: u8, werte: Vec<i32>) -> Result<Self, KonvertFehler> {
        Pcm::neu(rate, kanale, 24, werte)
    }

    /// Zerlegt einen Byte-Strom in Werte der Tiefe `bit_tiefe` (2 oder 3
    /// Byte je Wert, little endian).
    ///
    /// # Wichtig: Signed
    ///
    /// Das oberste Bit ist das Vorzeichen, und es wird zu Recht NICHT
    /// umgeklappt. Ein 16-Bit-PCM-Wert -1 ist `FF FF`, nicht `00 01` —
    /// und eine Datei, die stattdessen `00 01` schreibt, klingt wie ein
    /// winziger Ausschlag nach oben. Wer das umklappt, verliert die
    /// Rundreise genau an den halben Werten.
    pub fn aus_bytes_le(
        rate: u32,
        kanale: u8,
        bit_tiefe: u8,
        daten: &[u8],
    ) -> Result<Self, KonvertFehler> {
        let je_wert = usize::from(bit_tiefe).div_ceil(8);
        grenzen::pruefe_laenge(daten.len(), grenzen::MAX_TON_BYTES, "PCM-Byte")?;
        if daten.len() % (je_wert * usize::from(kanale)) != 0 {
            return Err(KonvertFehler::ZuKurz {
                vorhanden: daten.len(),
                gebraucht: je_wert * usize::from(kanale),
            });
        }
        let mut werte = Vec::with_capacity(daten.len() / je_wert);
        for block in daten.chunks_exact(je_wert) {
            let mut v = 0i32;
            for (i, &b) in block.iter().enumerate() {
                v |= i32::from(b) << (8 * i);
            }
            // Vorzeichen ueber die Nutzbits hinaus ausdehnen.
            let nutz = i32::from(bit_tiefe);
            let schub = 32 - nutz;
            werte.push((v << schub) >> schub);
        }
        Pcm::neu(rate, kanale, bit_tiefe, werte)
    }

    /// Zahl Frames (nicht Werte, nicht Bytes).
    #[must_use]
    pub fn frames(&self) -> usize {
        self.werte.len() / usize::from(self.kanale)
    }

    /// Dauer in Millisekunden, gerundet. 0 bei 0 Werten.
    #[must_use]
    pub fn dauer_ms(&self) -> u64 {
        if self.rate == 0 {
            return 0;
        }
        u64::from(self.frames() as u32).saturating_mul(1000) / u64::from(self.rate)
    }

    /// Alle Werte eines Kanals (`c`), der Reihe nach.
    ///
    /// Das ist die Sicht, aus der die Prädiktoren leben: FLAC rechnet
    /// seine Differenzen **je Kanal** (frameweise nebeneinander), nicht
    /// ueber die Kanalgrenze hinweg. Der Unterschied ist nicht kosmetisch:
    /// ein Prädiktor ueber die Kanalgrenze wuerde bei Stereo den linken
    /// Kanal aus dem rechten vorhersagen — das ist akustisch und statistisch
    /// falsch und kostet Bits.
    #[must_use]
    pub fn kanal(&self, c: u8) -> Vec<i32> {
        let k = usize::from(c);
        if k >= usize::from(self.kanale) {
            return Vec::new();
        }
        self.werte[k..]
            .iter()
            .copied()
            .step_by(usize::from(self.kanale))
            .collect()
    }
}
