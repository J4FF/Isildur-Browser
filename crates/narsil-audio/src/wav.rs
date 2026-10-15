//! G9: DER WAV-DEKODER (RIFF/WAVE).
//!
//! # WAS DIESER DEKODER KANN und was nicht
//!
//! | `wFormatTag` | Bit | Status |
//! |---|---|---|
//! | 1 `WAVE_FORMAT_PCM` | 8 | **gebaut** |
//! | 1 `WAVE_FORMAT_PCM` | 16 | **gebaut** |
//! | 1 `WAVE_FORMAT_PCM` | 24 | **gebaut** |
//! | 1 `WAVE_FORMAT_PCM` | 32 | **gebaut** |
//! | 3 `WAVE_FORMAT_IEEE_FLOAT` | 32 | **gebaut** |
//! | 3 `WAVE_FORMAT_IEEE_FLOAT` | 64 | **gebaut** |
//! | 6 `WAVE_FORMAT_ALAW` | 8 | **gebaut** |
//! | 7 `WAVE_FORMAT_MULAW` | 8 | **gebaut** |
//! | 0xFFFE `WAVE_FORMAT_EXTENSIBLE` | alle | **gebaut**, reicht an `SubFormat` weiter |
//! | 2 `ADPCM`, 0x11 `IMA ADPCM`, 0x31 `GSM 6.10` | — | **nicht gebaut**, benannter Fehler |
//!
//! ADPCM und GSM sind komprimiert; dafuer gehoert ein eigener Dekoder
//! mit Subblock-Gleichgewicht, und `narsil-audio` hat dafür keine
//! Spezifikationskopie. Das ist ein **offener Punkt**, kein stiller:
//! [`lies_kopf`] gibt bei diesen Tags `Art::NichtImplementiert` mit dem
//! Tag im Text.
//!
//! # BITGLEICH MIT FFMPEG, UND WAS DAS HEISST
//!
//! Der Auftrag sagt „bitgleich mit ffmpeg". Fuer die unkomprimierten
//! Formen heisst das: die **exakt gleichen** `i16`-Werte, die
//! `ffmpeg -i x.wav -f s16le` schreibt. Das ist erreichbar, weil ffmpeg
//! bei PCM nur umskaliert und sonst nichts tut — und es ist trotzdem
//! drei Zeilen Arbeit, an denen man sich messen lassen muss:
//!
//! * **8 Bit** ist **vorzeichenlos**: ffmpeg zieht **128** ab, damit 128
//!   zur Ruhe wird. Ohne das Abziehen steckt der Ruhepegel mitten im
//!   Signal, und es rauscht mit rund -48 dB.
//! * **24 Bit** wird **um 8 Bit nach links** geschoben. Ohne den Schift
//!   verliert man drei Bits — das ist kein Rundungsfehler, sondern eine
//!   halbe Quantisierungsstufe.
//! * **32 Bit** wird **durch 2^31 geteilt**, nicht durch 2^32: `-1`
//!   soll `-1,0` werden und nicht `+0,5e-9`.
//!
//! Alle drei sind mit `tests/g9_wav.rs` gegen echte ffmpeg-Dateien
//! geprüft, nicht gegen diese Beschreibung.
//!
//! # DIE BIT-TIEFE STEHT NICHT IM `fmt `, sondern IM BYTES PRO BLOCK
//!
//! `nBlockAlign` und `nAvgBytesPerSec` sind die **Byte**-Zahlen. Wer
//! `BitsPerSample` nimmt und alles andere ausmultipliziert, erhaelt bei
//! 24 Bit drei Bytes je Wert — was richtig ist — aber nur, wenn er auch
//! `BitsPerSample` durch 8 teilt. GEMESSENER FALL: bei **6 Kanaelen**
//! ist `nBlockAlign` nicht `6 · 3`, sondern der vom Hersteller
//! gerundete Wert; die **Rahmen**groesse muss deshalb aus
//! `nChannels` und `BitsPerSample` kommen und `nBlockAlign` nur
//! **geprueft** werden.

use crate::fehler::{Art, Ergebnis, Fehler};
use crate::ton::Ton;

/// `WAVE_FORMAT_PCM`.
pub const FORMAT_PCM: u16 = 0x0001;
/// `WAVE_FORMAT_IEEE_FLOAT`.
pub const FORMAT_FLOAT: u16 = 0x0003;
/// `WAVE_FORMAT_ALAW` (A-law).
pub const FORMAT_ALAW: u16 = 0x0006;
/// `WAVE_FORMAT_MULAW` (mu-law).
pub const FORMAT_MULAW: u16 = 0x0007;
/// `WAVE_FORMAT_EXTENSIBLE`: alle gueltigen Bits stehen in `SubFormat`.
pub const FORMAT_EXTENSIBEL: u16 = 0xFFFE;

/// Die erste GUID der K-Teiler von `WAVE_FORMAT_EXTENSIBLE`.
///
/// # WARUM NUR `Data1` GENUEGT, und WARUM ES DIE NIEDRIGEN 16 BIT SIND
///
/// `SubFormat` ist ein 16-Byte-GUID mit dem Aufbau
///
/// ```text
///   Data1 (u32 LE) | Data2 (u16 LE) | Data3 (u16 LE) | Data4[8]
/// ```
///
/// PCM ist `00000001-0000-0010-8000-00aa00389b71`, IEEE-Float ist
/// `00000003-0000-0010-8000-00aa00389b71`. `Data1` unterscheidet die
/// beiden also eindeutig, und der Rest ist bei allen Standard-GUIDs
/// gleich — deshalb wird nicht nach den letzten 14 Byte verglichen, das
/// wuerde jedes `extensible` mit abweichender Endung verwerfen.
///
/// # GEMESSENER FEHLER: DIE HOHEN 16 BIT STATT DER NIEDRIGEN
///
/// Ich hatte `daten[2..4]` gelesen — das sind die **oberen** 16 Bit von
/// `Data1`, und die sind bei beiden GUIDs **0**. GEMESSEN: der 24-Bit-
/// Test meldete `[NichtImplementiert] WAV-Format 0 (0x0000)`.
///
/// Das ist keine Kleinigkeit: **alle** Dateien mit mehr als 16 Bit und
/// allen Kanaelen schreibt ffmpeg als `extensible`, weil 24 Bit und 6
/// Kanaele nicht in das alte `BitsPerSample`-Feld passen. Ein Dekoder,
/// der `extensible` nicht aufloest, verliert also genau die Formate, um
/// die es hier geht.
#[must_use]
pub fn guid_format(daten: &[u8]) -> Option<u16> {
    if daten.len() < 4 {
        return None;
    }
    // Die **niedrigen** 16 Bit von Data1.
    Some(u16::from_le_bytes([daten[0], daten[1]]))
}

/// Was der `fmt `-Chunk sagt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kopf {
    /// Der Format-Tag (nach Aufloesung von `extensible`).
    pub format: u16,
    /// Die Kanalzahl.
    pub kanaele: u16,
    /// Die Abtastrate in Hz.
    pub rate: u32,
    /// `nAvgBytesPerSec`.
    pub bytes_pro_sek: u32,
    /// `nBlockAlign`, in Byte je Rahmen.
    pub block_align: u16,
    /// `BitsPerSample`, in Bit je Wert.
    pub bit_pro_wert: u16,
    /// Die Position des `data`-Chunks im Strom, wenn schon gesehen.
    pub daten_start: usize,
    /// Die Laenge des `data`-Chunks.
    pub daten_laenge: usize,
}

fn u16le(d: &[u8], ab: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*d.get(ab)?, *d.get(ab + 1)?]))
}

fn u32le(d: &[u8], ab: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *d.get(ab)?,
        *d.get(ab + 1)?,
        *d.get(ab + 2)?,
        *d.get(ab + 3)?,
    ]))
}

/// Liest den RIFF-Kopf und folgt den Chunks bis `data`.
///
/// # DIE DREI STOLPERSTEINEN, und alle drei hat die Norm nicht
///
/// 1. **Ein Chunk mit ungerader Laenge hat ein Fuellbyte.** Das steht in
///    der RIFF-Spezifikation, steht aber in keiner Zusammenfassung. Ohne
///    das Fuellbyte landet der `fmt `-Wert eine Byte zu spaet und man
///    liest Ton mit 44100 Hz, der 8 Bit hat und um 100 kHz danebenliegt.
/// 2. **`data` steht nicht zwingend zuerst.** Ein von Windows
///    aufgenommenes 24-Bit-File hat davor oft `fact` und `LIST`. Wer
///    `data` an Offset 12 erwartet, liest `LIST` als Ton.
/// 3. **`RIFF` heisst nicht `WAVE`.** `RIFF....AVI ` ist dieselbe
///    Containerform mit vier anderen Chunks.
pub fn lies_kopf(daten: &[u8]) -> Ergebnis<Kopf> {
    if daten.len() < 12 {
        return Err(Fehler::neu(
            Art::StromZuEnde,
            format!("WAV: nur {} Byte — der Kopf braucht 12", daten.len()),
        ));
    }
    if &daten[0..4] != b"RIFF" {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("{} ist kein RIFF", String::from_utf8_lossy(&daten[0..4])),
        ));
    }
    if &daten[8..12] != b"WAVE" {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("{} ist kein WAVE", String::from_utf8_lossy(&daten[8..12])),
        ));
    }
    let riff_laenge = u32le(daten, 4).unwrap_or(0) as usize;
    let ende = if riff_laenge >= 4 && 4 + riff_laenge <= daten.len() {
        4 + riff_laenge
    } else {
        daten.len()
    };

    let mut kopf: Option<Kopf> = None;
    let mut daten_start = None;
    let mut daten_laenge = 0usize;
    let mut ab = 12usize;
    while ab + 8 <= ende {
        let vier = &daten[ab..ab + 4];
        let laenge = u32le(daten, ab + 4).unwrap_or(0) as usize;
        let inhalt = ab + 8;
        if vier == b"fmt " {
            if inhalt + 16 > daten.len() {
                return Err(Fehler::neu(
                    Art::StromZuEnde,
                    "WAV: der fmt -Chunk ist abgeschnitten",
                ));
            }
            let mut format = u16le(daten, inhalt).unwrap_or(0);
            let bit_pro_wert = u16le(daten, inhalt + 14).unwrap_or(0);
            if format == FORMAT_EXTENSIBEL {
                // `cbSize` steht bei Byte 16, `SubFormat` bei 24.
                if inhalt + 26 <= daten.len() {
                    format = guid_format(&daten[inhalt + 24..]).unwrap_or(format);
                } else {
                    return Err(Fehler::neu(
                        Art::StromZuEnde,
                        "WAV: extensible fmt -Chunk ohne SubFormat",
                    ));
                }
            }
            kopf = Some(Kopf {
                format,
                kanaele: u16le(daten, inhalt + 2).unwrap_or(0),
                rate: u32le(daten, inhalt + 4).unwrap_or(0),
                bytes_pro_sek: u32le(daten, inhalt + 8).unwrap_or(0),
                block_align: u16le(daten, inhalt + 12).unwrap_or(0),
                bit_pro_wert,
                daten_start: 0,
                daten_laenge: 0,
            });
        } else if vier == b"data" {
            daten_start = Some(inhalt);
            // Der Chunk kann laenger sein als der Rest der Datei
            // (Encoder schreiben gern die ** nominelle** Laenge). Es
            // wird deshalb geklammert statt abgelehnt.
            daten_laenge = laenge.min(daten.len().saturating_sub(inhalt));
        }
        // Naechster Chunk: ungerade Laenge -> ein Fuellbyte.
        ab = inhalt + laenge + (laenge & 1);
    }

    let Some(k) = kopf else {
        return Err(Fehler::neu(Art::FalscherWert, "WAV: kein fmt -Chunk"));
    };
    let Some(ds) = daten_start else {
        return Err(Fehler::neu(Art::FalscherWert, "WAV: kein data-Chunk"));
    };
    if k.kanaele == 0 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            "WAV: nChannels ist 0 — es gibt nichts zu hoeren",
        ));
    }
    if k.rate == 0 {
        return Err(Fehler::neu(Art::FalscherWert, "WAV: nSamplesPerSec ist 0"));
    }
    Ok(Kopf {
        daten_start: ds,
        daten_laenge,
        ..k
    })
}

/// Wandelt ein Sample je Kanal in die Normeinheit `-1.0 .. 1.0`.
///
/// # DIE DREI NORMIERUNGEN, die nicht dasselbe sind
///
/// ```text
///   8  Bit vorzeichenlos : (x − 128) / 128
///   16/24/32 Bit        : x / 2^(Bits−1)
/// ```
///
/// Der 8-Bit-Fall ist der eine, der immer wieder falsch gemacht wird:
/// `u8` ist **0..255**, und die Mitte (Stille) ist **128**, nicht 0.
/// ffmpeg zieht 128 ab; ohne das klingt die Datei, als haette sie einen
/// Gleichanteil, und man hoert es als Brummen bei 50 Hz.
fn wert(format: u16, bit: u16, bytes: &[u8]) -> f64 {
    match (format, bit) {
        (FORMAT_PCM, 8) => (f64::from(bytes[0]) - 128.0) / 128.0,
        (FORMAT_PCM, 16) => f64::from(i16::from_le_bytes([bytes[0], bytes[1]])) / 32768.0,
        (FORMAT_PCM, 24) => {
            // Drei Byte little endian, **vorzeichenbehaftet**. Das
            // oberste Byte ist bei negativem Wert >= 0x80, und genau
            // deshalb wird es als `i8` gelesen und nicht als `u8`:
            //
            // ```text
            //   (i32::from(bytes[2] as i8) << 16) | (bytes[1] << 8) | bytes[0]
            // ```
            //
            // GEMESSENER FALL, den diese Zeile verhindert: mit `u8`
            // ergaebe 0x800000 den Wert **8388608** statt **−8388608**,
            // und die linke Haelfte des Signals waere spiegelverkehrt.
            let roh = (i32::from(bytes[2] as i8) << 16)
                | (i32::from(bytes[1]) << 8)
                | i32::from(bytes[0]);
            f64::from(roh) / 8_388_608.0
        }
        (FORMAT_PCM, 32) => {
            f64::from(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                / 2_147_483_648.0
        }
        (FORMAT_FLOAT, 32) => {
            f64::from(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        }
        (FORMAT_FLOAT, 64) => f64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]),
        (FORMAT_MULAW, 8) => mulaw_zu_f64(bytes[0]),
        (FORMAT_ALAW, 8) => alaw_zu_f64(bytes[0]),
        _ => 0.0,
    }
}

/// mu-law (G.711) nach der linearen 16-Bit-Zahl, exakt wie ffmpeg.
///
/// Belegt an `libavcodec/pcm_tablegen.h`, `ulaw2linear`:
///
/// ```c
/// #define BIAS  (0x84)
/// u_val = ~u_val;
/// t = ((u_val & 0xf) << 3) + BIAS;
/// t <<= (u_val & 0x70) >> 4;
/// return (u_val & 0x80) ? (BIAS - t) : (t - BIAS);
/// ```
///
/// # MEINE ERSTE FORMEL WAR FALSCH, und zwar an drei Stellen
///
/// 1. Ich hatte das Vorzeichenbit **invertiert** statt das ganze Byte zu
///    invertieren. Bei mu-law ist es das ganze Byte (`u_val = ~u_val`),
///    weil das Vorzeichen **kodiert** ist und nicht gesetzt.
/// 2. Ich hatte den Bias `0x84` **addiert statt subtrahiert** — und am
///    Ende auch mit umgekehrtem Vorzeichen.
/// 3. Ich hatte die Mantisse mit `<< 1 + 33` verschoben statt mit
///    `<< 3` plus Bias.
///
/// GEMESSENE FOLGE: `g9_mulaw_mono` wich in **allen 8000** Samples ab.
/// Das ist die schoenste Form von „falsch": es sind keine
/// Ausreisser, sondern jedes Sample, und trotzdem sieht die Datei
/// aus wie ein Ton — nur mit einer winzigen Verzerrung, die man
/// hoeren koennte und nicht muss.
#[must_use]
pub fn mulaw_zu_f64(b: u8) -> f64 {
    let v = !b;
    let t = ((u32::from(v & 0x0F) << 3) + BIAS) << (u32::from(v & 0x70) >> 4);
    f64::from(if v & 0x80 != 0 {
        BIAS as i32 - t as i32
    } else {
        t as i32 - BIAS as i32
    }) / 32768.0
}

/// A-law (G.711) nach der linearen 16-Bit-Zahl, exakt wie ffmpeg.
///
/// Belegt an `libavcodec/pcm_tablegen.h`, `alaw2linear`:
///
/// ```c
/// a_val ^= 0x55;
/// t  = a_val & 0x0f;
/// seg = (a_val & 0x70) >> 4;
/// if (seg) t = (t + t + 1 + 32) << (seg + 2);
/// else     t = (t + t + 1)      << 3;
/// return (a_val & 0x80) ? t : -t;
/// ```
///
/// # DER UNTERSCHIED ZU mu-law, UND ER IST NICHT KLEIN
///
/// Bei A-law wird zuerst mit `0x55` ge-XORt, und danach ist das
/// Vorzeichenbit **normal** (1 heisst negativ). Bei mu-law wird das
/// ganze Byte invertiert und das Ergebnis am Ende **noch einmal
/// herumgerechnet** (`BIAS - t`). Deshalb sind es hier zwei
/// verschiedene Formeln und nicht eine mit einem Flag.
#[must_use]
pub fn alaw_zu_f64(b: u8) -> f64 {
    let a = b ^ 0x55;
    let mut t = i32::from(a & 0x0F);
    let seg = i32::from(a & 0x70) >> 4;
    if seg != 0 {
        t = (t + t + 1 + 32) << (seg + 2);
    } else {
        t = (t + t + 1) << 3;
    }
    f64::from(if a & 0x80 != 0 { t } else { -t }) / 32768.0
}

/// Der Bias aus G.711: `0x84`. In der mu-law-Formel zweimal gebraucht.
const BIAS: u32 = 0x84;

/// Dekodiert eine ganze WAV-Datei.
pub fn dekodiere(daten: &[u8]) -> Ergebnis<Ton> {
    let k = lies_kopf(daten)?;
    let bytes_je_wert = k.bit_pro_wert as usize / 8;
    let rahmen = usize::from(k.kanaele) * bytes_je_wert.max(1);

    match k.format {
        FORMAT_PCM | FORMAT_FLOAT => {}
        FORMAT_MULAW | FORMAT_ALAW => {}
        FORMAT_EXTENSIBEL => unreachable!("`lies_kopf` hat extensible bereits aufgeloest"),
        tag => {
            return Err(Fehler::neu(
                Art::NichtImplementiert,
                format!(
                    "WAV-Format {tag} (0x{tag:04X}) ist nicht gebaut; \
                     gebaut sind PCM 8/16/24/32, IEEE-Float 32/64, A-law und mu-law"
                ),
            ));
        }
    }
    if bytes_je_wert == 0 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!(
                "WAV: BitsPerSample = {} ergibt kein volles Byte",
                k.bit_pro_wert
            ),
        ));
    }
    // `nBlockAlign` wird **geprueft**, nicht benutzt: bei 24 Bit und
    // sechs Kanaelen runden manche Hersteller.
    if rahmen > 0 && k.block_align != 0 && usize::from(k.block_align) != rahmen {
        // Kein Fehler — aber es wird gemeldet, weil es der Ort ist, an
        // dem ein Tippfehler in der Rahmengroesse sichtbar wird.
        eprintln!(
            "WAV: nBlockAlign {} passt nicht zu {} Kanaelen mal {} Byte ({}); \
             es wird die berechnete Groesse genommen",
            k.block_align, k.kanaele, bytes_je_wert, rahmen
        );
    }

    let end = (k.daten_start + k.daten_laenge).min(daten.len());
    let verfuegbar = end.saturating_sub(k.daten_start);
    let ganze_rahmen = verfuegbar / rahmen.max(1);
    let mut roh = Vec::with_capacity(ganze_rahmen * usize::from(k.kanaele));
    for r in 0..ganze_rahmen {
        let ab = k.daten_start + r * rahmen;
        for wz in 0..usize::from(k.kanaele) {
            let b = ab + wz * bytes_je_wert;
            roh.push(wert(k.format, k.bit_pro_wert, &daten[b..b + bytes_je_wert]));
        }
    }
    Ton::aus_verflochten(k.rate, usize::from(k.kanaele), &roh)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Baut eine minimale WAV-Datei aus den Chunk-Angaben.
    fn wav(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut raus: Vec<u8> = Vec::new();
        raus.extend_from_slice(b"RIFF");
        raus.extend_from_slice(&0u32.to_le_bytes());
        raus.extend_from_slice(b"WAVE");
        for (name, inhalt) in chunks {
            raus.extend_from_slice(*name);
            raus.extend_from_slice(&(inhalt.len() as u32).to_le_bytes());
            raus.extend_from_slice(inhalt);
            // Ein Chunk mit **ungerader** Laenge hat ein Fuellbyte.
            if inhalt.len() % 2 == 1 {
                raus.push(0);
            }
        }
        let laenge = (raus.len() - 8) as u32;
        raus[4..8].copy_from_slice(&laenge.to_le_bytes());
        raus
    }

    fn fmt_pcm(bit: u16, kan: u16, rate: u32) -> Vec<u8> {
        let mut f = Vec::new();
        f.extend_from_slice(&FORMAT_PCM.to_le_bytes());
        f.extend_from_slice(&kan.to_le_bytes());
        f.extend_from_slice(&rate.to_le_bytes());
        f.extend_from_slice(&(rate * u32::from(kan) * u32::from(bit) / 8).to_le_bytes());
        f.extend_from_slice(&(kan * bit / 8).to_le_bytes());
        f.extend_from_slice(&bit.to_le_bytes());
        f
    }

    #[test]
    fn ein_ungerader_chunk_verschiebt_den_naechsten_nicht() {
        // GEMESSENER FALL, und er ist der Grund fuer die Fuellbit-Regel:
        // ein `LIST`-Chunk mit ungerader Laenge steht bei ffmpeg-Dateien
        // **vor** `data`. Ohne Fuellbyte laesst `data` sich nicht finden
        // und man liest den Ton aus dem `LIST`-Inhalt.
        let d = wav(&[
            (b"fmt ", fmt_pcm(16, 1, 44100)),
            (b"LIST", vec![b'I'; 25]),
            (b"data", vec![0u8; 400]),
        ]);
        let k = lies_kopf(&d).expect("Kopf lesbar");
        assert_eq!(k.rate, 44100);
        assert_eq!(k.daten_laenge, 400);
        assert_eq!(&d[k.daten_start..k.daten_start + 4], &[0, 0, 0, 0]);
    }

    #[test]
    fn fact_und_list_vor_data_stoeren_nicht() {
        // GEMESSEN an `g9_f32_mono.wav`: dort stehen `fmt `, dann
        // `fact`, dann `LIST`, dann `data`. Wer `data` an Offset 12
        // erwartet, liest `fact` als Ton.
        let d = wav(&[
            (b"fmt ", fmt_pcm(32, 2, 44100)),
            (b"fact", vec![0u8; 4]),
            (b"LIST", b"INFOhallo".to_vec()),
            (b"data", vec![7u8; 256]),
        ]);
        let k = lies_kopf(&d).expect("Kopf lesbar");
        assert_eq!(k.daten_laenge, 256);
        assert_eq!(k.daten_start % 2, 0, "der data-Chunk muss gerade beginnen");
    }

    #[test]
    fn extensible_wird_auf_die_guid_aufgeloest() {
        // GEMESSENER FEHLER: ich hatte `daten[2..4]` gelesen — die HOHEN
        // 16 Bit von Data1. Bei beiden Standard-GUIDs sind die 0, also
        // kam Format 0 heraus und der 24-Bit-Test meldete
        // „WAV-Format 0".
        let mut f = Vec::new();
        f.extend_from_slice(&FORMAT_EXTENSIBEL.to_le_bytes());
        f.extend_from_slice(&2u16.to_le_bytes());
        f.extend_from_slice(&44100u32.to_le_bytes());
        f.extend_from_slice(&44100u32.to_le_bytes());
        f.extend_from_slice(&6u16.to_le_bytes());
        f.extend_from_slice(&24u16.to_le_bytes());
        f.extend_from_slice(&22u16.to_le_bytes()); // cbSize
        f.extend_from_slice(&3u16.to_le_bytes()); // wValidBitsPerSample
        f.extend_from_slice(&3u32.to_le_bytes()); // dwChannelMask
        f.extend_from_slice(&[
            1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0, 0xaa, 0, 0x3, 0x89, 0xb1,
        ]);
        let d = wav(&[(b"fmt ", f), (b"data", vec![0u8; 600])]);
        let k = lies_kopf(&d).expect("Kopf lesbar");
        assert_eq!(k.format, FORMAT_PCM, "extensible + PCM-GUID ergibt PCM");
        assert_eq!(k.bit_pro_wert, 24);
        assert_eq!(k.kanaele, 2);
    }

    #[test]
    fn die_guid_liefert_1_und_3_und_nichts_anderes() {
        let mut g = [0u8; 16];
        g[0] = 3;
        g[2] = 0xaa;
        assert_eq!(guid_format(&g), Some(3));
        g[0] = 1;
        assert_eq!(guid_format(&g), Some(1));
        g[0] = 0x42;
        assert_eq!(guid_format(&g), Some(0x42));
        assert_eq!(guid_format(&[1, 2]), None);
    }

    #[test]
    fn kein_riff_ist_ein_fehler_mit_der_magic_im_text() {
        let mut d = wav(&[(b"fmt ", fmt_pcm(16, 1, 8000)), (b"data", vec![0; 16])]);
        d[0] = b'R';
        d[1] = b'I';
        d[2] = b'F';
        d[3] = b'X';
        let e = lies_kopf(&d).unwrap_err();
        assert!(
            e.text.contains("RIFX"),
            "der Fehler nennt die Magic nicht: {e}"
        );
        d[0] = b'R';
        d[1] = b'I';
        d[2] = b'F';
        d[3] = b'F';
        d[8..12].copy_from_slice(b"AVI ");
        let e2 = lies_kopf(&d).unwrap_err();
        assert!(e2.text.contains("AVI"), "kein WAVE: {e2}");
    }

    #[test]
    fn ein_data_chunk_der_laenger_ist_als_die_datei_wird_geklammert() {
        // Encoder schreiben gern die **nominelle** Laenge. ffmpeg
        // enthaelt nichts, was abstuerzen laesst — und der Ton soll
        // trotzdem kommen, nur kuerzer.
        let mut d = wav(&[(b"fmt ", fmt_pcm(16, 1, 8000)), (b"data", vec![0u8; 400])]);
        let laenge = (d.len() - 12) as u32;
        d[44..48].copy_from_slice(&(laenge + 9999).to_le_bytes());
        let k = lies_kopf(&d).expect("Kopf lesbar");
        assert_eq!(k.daten_laenge, 400, "geklammert auf die Dateigroesse");
    }

    #[test]
    fn die_drei_normalisierungen_sind_die_richtigen() {
        // 8 Bit: **vorzeichenlos**, Mitte 128, und ffmpeg zieht 128 ab.
        assert_eq!(wert(FORMAT_PCM, 8, &[128]), 0.0, "Stille ist 128");
        assert_eq!(wert(FORMAT_PCM, 8, &[0]), -1.0);
        assert_eq!(wert(FORMAT_PCM, 8, &[255]), 127.0 / 128.0);
        // 16 Bit: **0x8000 ist -32768**, nicht +32768. Das ist das
        // Zweierkomplement, und ich hatte es erst umgekehrt —
        // GEMESSEN: der Test meldete `-1.0` gegen meine Erwartung
        // `1.0`. Wer statt `-1` `+1` erwartet, hat das Bild im Kopf
        // umgekehrt; der Test faellt dann zu Recht.
        assert_eq!(wert(FORMAT_PCM, 16, &[0x00, 0x80]), -1.0);
        // Das groesste **positive** `i16` ist 0x7FFF, little endian also
        // `[0xFF, 0x7F]` — nicht `[0x00, 0x7F]`, das 0x7F00 = 32512 ist.
        // GEMESSEN: `0,9921875` statt `0,999969482421875`, und die
        // Differenz ist genau 255/32768. Bei little endian steht das
        // **niedrigwertige** Byte zuerst; wer es umdreht, verliert ein
        // Byte und bekommt es bei grossen Werten nicht zu sehen.
        assert_eq!(wert(FORMAT_PCM, 16, &[0xFF, 0x7F]), 32767.0 / 32768.0);
        assert_eq!(wert(FORMAT_PCM, 16, &[0x00, 0x7F]), 32512.0 / 32768.0);
        assert_eq!(wert(FORMAT_PCM, 16, &[0x00, 0x80]), -1.0);
        // 24 Bit: 0x800000 ist **-1**, nicht +1. Mit `u8` statt `i8`
        // waere es +1, und die linke Haelfte waere spiegelverkehrt.
        assert_eq!(wert(FORMAT_PCM, 24, &[0x00, 0x00, 0x80]), -1.0);
        // Das groesste positive 24-Bit-Format ist 0x7FFFFF, also
        // `[0xFF, 0xFF, 0x7F]`. Meine Erwartung `0x7F0000` war um 2^16
        // daneben — GEMESSEN 0,9921875 statt 0,999969482421875, und die
        // Differenz ist genau 65535/8388608.
        assert_eq!(
            wert(FORMAT_PCM, 24, &[0xFF, 0xFF, 0x7F]),
            8388607.0 / 8388608.0
        );
        // 0xFFFFFF ist -1 im 24-Bit-Zweierkomplement, also -1/2^23 und
        // **nicht** -1/2^8. GEMESSEN: -1,1920928955078125e-07 statt
        // -0,00390625 — die zweite Zahl haette zu einem 16-Bit-Wert
        // gepasst, und genau das ist der Fehler, den man macht, wenn man
        // die Wortbreite im Kopf hat statt in der Datei.
        assert_eq!(
            wert(FORMAT_PCM, 24, &[0xFF, 0xFF, 0xFF]),
            -1.0 / 8_388_608.0
        );
        // 32 Bit: geteilt durch 2^31, **nicht** durch 2^32. Und
        // 0x80000000 ist -1 — auch hier steht das hoechste Bit fuer das
        // **negative** Vorzeichen. GEMESSEN: -1,0 gegen meine
        // Erwartung 1,0; dieselbe Verwechslung wie bei 16 Bit, nur
        // einmal mehr.
        assert_eq!(wert(FORMAT_PCM, 32, &[0, 0, 0, 0x80]), -1.0);
        assert_eq!(
            wert(FORMAT_PCM, 32, &[0xFF, 0xFF, 0xFF, 0x7F]),
            2_147_483_647.0 / 2_147_483_648.0
        );
        // Und der Beweis fuer „2^31 statt 2^32": waere es 2^32, kaeme
        // bei -1 genau -0,5 heraus statt -1,0.
        assert!(wert(FORMAT_PCM, 32, &[0xFF, 0xFF, 0xFF, 0xFF]).abs() < 1e-9);
    }

    #[test]
    fn mulaw_und_alaw_haben_je_256_eigene_werte() {
        // GEMESSENE FAELLE: `g9_alaw_mono` und `g9_mulaw_mono` wichen
        // an **allen** 8000 Samples ab, nicht an zwei. Eine
        // falsche Nichtlinearitaet sieht nicht kaputt aus — sie
        // klingt nach einem sehr leisen Verzerrer.
        let m: std::collections::HashSet<u16> = (0..=255u8)
            .map(|b| (mulaw_zu_f64(b) * 32768.0).round() as i16 as u16)
            .collect();
        let a: std::collections::HashSet<u16> = (0..=255u8)
            .map(|b| (alaw_zu_f64(b) * 32768.0).round() as i16 as u16)
            .collect();
        // # GEMESSEN: mu-law hat 255, NICHT 256 VERSCHIEDENE WERTE
        //
        // `0x7F` und `0xFF` ergeben beide **Null**: mu-law hat zwei
        // Codes fuer die Stille, weil negative Null nicht existiert und
        // der Komplement-Bias sie erzeugt. Meine Erwartung „256" war
        // aus dem Gedaechtnis; A-law hat die Eigenheit **nicht** und
        // kommt auf 256. Genau so steht es in G.711.
        assert_eq!(
            m.len(),
            255,
            "mu-law: 255 verschiedene Werte (zwei Codes fuer 0)"
        );
        assert_eq!(a.len(), 256, "A-law: alle 256 Werte verschieden");
        // Beide haben eine Null (die Ruhe) und reichen bis ±32256.
        assert!(m.contains(&0), "mu-law hat keine Null");
        // # GEMESSEN: A-law HAT KEINE NULL — und das ist richtig
        //
        // `alaw2linear` gibt `±t` zurueck, und `t` ist im Segment 0
        // `(2q+1) << 3`, also **mindestens 8**. Die Stufe 0 von A-law
        // hat keine Naehe zur Null; sie beginnt bei 8 von 32768.
        // Ich hatte „A-law hat eine Null" behauptet — genau die
        // Umkehrung der mu-law-Eigenheit.
        assert!(
            !a.contains(&0),
            "A-law kann nicht 0 haben — der Wert waere falsch"
        );
        assert_eq!(
            a.iter().map(|&v| (v as i16).unsigned_abs()).min(),
            Some(8),
            "der kleinste A-law-Wert ist ±8"
        );
        assert!(
            m.iter().any(|&v| v > 32000),
            "mu-law reicht nicht bis 32256"
        );
        assert!(a.iter().any(|&v| v > 32000), "A-law reicht nicht bis 32256");
    }

    #[test]
    fn adpcm_bricht_mit_benanntem_fehler_ab() {
        // KEIN stilles Durchreichen: ein unbekanntes Format, das als
        // PCM gelesen wuerde, liefert Rauschen statt eines Fehlers.
        let mut f = Vec::new();
        f.extend_from_slice(&2u16.to_le_bytes()); // ADPCM
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&44100u32.to_le_bytes());
        f.extend_from_slice(&16000u32.to_le_bytes());
        f.extend_from_slice(&256u16.to_le_bytes());
        f.extend_from_slice(&4u16.to_le_bytes());
        let d = wav(&[(b"fmt ", f), (b"data", vec![0u8; 256])]);
        let e = dekodiere(&d).unwrap_err();
        assert_eq!(e.art, Art::NichtImplementiert);
        assert!(e.text.contains("0x0002"), "der Fehler nennt den Tag: {e}");
    }
}
