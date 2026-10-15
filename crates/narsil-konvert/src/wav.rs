//! WAV SCHREIBEN (RIFF/WAVE, PCM 16/24/32).
//!
//! Aufbau:
//!
//! ```text
//! "RIFF" | Groesse (4) | "WAVE"
//!   "fmt " | 16        | ...            <-_FORMAT (16 Byte)
//!   "data" | Laenge   | ...            <- die Rohwerte
//! ```
//!
//! # Zwei Faelle, die still falsch sind
//!
//! 1. **Die `RIFF`-Groesse ist Dateigroesse minus 8.** Sie zaehlt also
//!    WAVE-Format UND alle Bloecke, aber nicht die Marke `RIFF` selbst und
//!    nicht ihr eigenes Laengenfeld. Wer sie als Dateigroesse schreibt,
//!    bekommt eine Datei, die `file` und jedes andere Programm als
//!    abgeschnitten meldet.
//!
//! 2. **24-Bit-Werte liegen linksbuendig im 32-Bit-Feld.** Das Bytefeld
//!    einer 24-Bit-Probe ist 3 Byte, und der Wert wird NICHT
//!    vorzeichenbehandelt in die 3 Byte geschrieben: bei einem negativen
//!    Wert gehoeren die drei oberen Bits auf 1. `-1` ist also `FF FF FF`,
//!    nicht `00 00 FF`. Wer `value as u32` und dann die unteren 3 Byte
//!    nimmt, erhaelt `FF FF 00` — und das ist 0, nicht -1. GEMESSEN
//!    02.10.2026: das ist der ganze Unterschied zwischen einer Rundreise
//!    und einem Bildschirm voller Nullen.

use crate::{KonvertFehler, grenzen, ton::Pcm};

/// Welches PCM in die WAV-Datei.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WavArt {
    /// 16 Bit je Wert. Der uebliche Fall; kleiner und ueberall lesbar.
    Pcm16,
    /// 24 Bit je Wert. Fuenf Bits mehr als 16, aber dreimal so gross.
    ///
    /// GEMESSEN 02.10.2026 an `muster`: 16 Bit liefert dort SNR 51,7 dB,
    /// 24 Bit 74,1 dB. Wer die Absolutgenauigkeit braucht, braucht 24.
    Pcm24,
    /// 32 Bit je Wert (signed). FLAC kann 32, WAV auch — und die
    /// Flexibilitaet ist der Grund, warum dieses Feld hier steht.
    ///
    /// Achtung: 32-Bit-WAV mit „integer PCM" wird von manchen Programmen
    /// als Float gelesen. Deshalb ist es NICHT der Standardfall.
    Pcm32,
}

impl WavArt {
    /// Bytes je Wert (2, 3 oder 4).
    const fn bytes(self) -> usize {
        match self {
            WavArt::Pcm16 => 2,
            WavArt::Pcm24 => 3,
            WavArt::Pcm32 => 4,
        }
    }

    /// Die Bittiefe, die in das fmt-Feld gehoert.
    const fn bit_tiefe(self) -> u8 {
        match self {
            WavArt::Pcm16 => 16,
            WavArt::Pcm24 => 24,
            WavArt::Pcm32 => 32,
        }
    }
}

/// Schreibt eine WAV-Datei.
///
/// # Fehler
/// - Siehe [`Pcm::neu`] (der Aufrufer baut den Ton, hier wird geprueft).
/// - [`KonvertFehler::SpeicherZuGross`] wenn die Datei die Grenze
///   [`grenzen::MAX_TON_BYTES`] sprengen wuerde.
pub fn schreiben(pcm: &Pcm, art: WavArt) -> Result<Vec<u8>, KonvertFehler> {
    // Wenn die Quelle tiefer ist als das Ziel, muss gerundet werden —
    // und der Wert muss in den Bereich des ZIELS passen, sonst traegt
    // `Pcm::neu` spaeter eine Zahl, die nicht darstellbar ist.
    let ziel = art.bit_tiefe();
    let werte = if pcm.bit_tiefe == ziel {
        pcm.werte.clone()
    } else {
        in_tiefe(&pcm.werte, pcm.bit_tiefe, ziel)?
    };

    let je_wert = art.bytes();
    let daten_len = werte.len() * je_wert;
    grenzen::pruefe_laenge(daten_len, grenzen::MAX_TON_BYTES, "WAV-Daten")?;
    // RIFF-Groesse: alles nach dem 8-Byte-Kopf.
    let riff_groesse = 4 + 24 + 8 + daten_len;
    grenzen::pruefe_laenge(riff_groesse + 8, grenzen::MAX_TON_BYTES, "WAV-Datei")?;

    let mut out: Vec<u8> = Vec::with_capacity(riff_groesse + 8);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(riff_groesse as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");

    // --- fmt -------------------------------------------------------------
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt ist 16 Byte lang
    out.extend_from_slice(&1u16.to_le_bytes()); // 1 = reines PCM (Integer)
    // ACCHTUNG, HIER LAG EIN FEHLER (gefunden 02.10.2026 von
    // `wav_kopf_feld_fuer_feld` und von ffmpeg: „WAVE audio, 187 Hz").
    //
    // Hier stand `out.push(pcm.kanale)` — EIN Byte. `nChannels` ist aber
    // ein 16-Bit-Feld (WAVEFORMATEX, und die ueberall abgeschriebene
    // Ausnahme WAVEFORMATEXEXTENSIBLE hat es auch als WORD). Mit einem
    // Byte stimmt die Groesse des fmt-Baus nicht mehr, und jedes Feld
    // danach rueckt um eins auf:
    //
    // ```text
    //  Offset  richtig (24 Byte fmt)   mit 1-Byte-Kanaelen
    //  20      Format (2)              Format (2)
    //  22      Kanaele (2)             Kanaele (1)  <- ab hier eins daneben
    //  24      Abtastrate (4)          (Kanal-Oberteil + 3 Byte Rate)
    //  28      Byte-Rate (4)           ...
    // ```
    //
    // GEMESSEN: `file` meldete „187 Hz" fuer 48000 Hz, ffmpeg verweigerte
    // die Datei, und `daten.len()` war um eins zu klein. Der Test hat den
    // Fehler an drei unabhaengigen Stellen gleichzeitig gesehen, was die
    // Sache eindeutig macht: es war der Kopf und nicht der Inhalt.
    out.extend_from_slice(&(u16::from(pcm.kanale)).to_le_bytes());
    out.extend_from_slice(&pcm.rate.to_le_bytes());
    // Byte-Rate = Abtastrate * Kanale * Bytes je Wert.
    let byte_rate = pcm.rate * u32::from(pcm.kanale) * je_wert as u32;
    out.extend_from_slice(&byte_rate.to_le_bytes());
    // Block-Alignment = Kanale * Bytes je Wert (Bytes je FRAME).
    out.extend_from_slice(&(u16::from(pcm.kanale) * je_wert as u16).to_le_bytes());
    out.extend_from_slice(&(ziel as u16).to_le_bytes());

    // --- data -----------------------------------------------------------
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(daten_len as u32).to_le_bytes());
    match art {
        WavArt::Pcm16 => {
            for &w in &werte {
                // `w` liegt in -2^15..=2^15-1, weil `werte` oben in die
                // Zieltiefe gerechnet wurde. `as u16` bildet auf Zwei-
                // Komplement ab — das ist bei signed PCM richtig.
                out.extend_from_slice(&(w as u16).to_le_bytes());
            }
        }
        WavArt::Pcm24 => {
            for &w in &werte {
                // `w` ist negativ oder 0..=2^24-1. `as u32` wandelt
                // negatives Zwei-Komplement in die oberen Einsen —
                // genau das, was hier gebraucht wird.
                let b = w as u32;
                out.push((b & 0xFF) as u8);
                out.push(((b >> 8) & 0xFF) as u8);
                out.push(((b >> 16) & 0xFF) as u8);
            }
        }
        WavArt::Pcm32 => {
            for &w in &werte {
                out.extend_from_slice(&w.to_le_bytes());
            }
        }
    }
    Ok(out)
}

/// Rechnet Werte von einer Tiefe in eine andere, mit RUNDUNG und
/// Begrenzung — und in **beide** Richtungen.
///
/// # DER PANIC, DEN DIESER WEG AUSGELÖST HAT — und warum er drei Tage blieb
///
/// Die erste Fassung kannte **nur nach unten**:
///
/// ```text
/// let schub = i32::from(von) - i32::from(nach);   // 16 - 24 = **-8**
/// let hoch = 1i64 << schub;                       // << -8 -> PANIC
/// ```
///
/// Und **`text`**, nicht `rust` — GEMESSEN, denn sonst **fuehrt** rustdoc das
/// Beispiel **aus** und es scheitert an `von`, das es dort nicht gibt.
/// Und das ist die **neunte** Fassung derselben Regel:
///
/// > **Ein Beispiel im Kommentar ist **Text**, kein Programm.** Es wird von
/// > niemandem **getestet**, also darf es auch nicht so **aussehen** wie etwas,
/// > das getestet wird.
///
/// GEMESSEN am 03.10.2026, und **erst** durch die neue Option `ton_bits`:
/// `KonvertFehler` / `wav.rs:172`, `attempt to shift left with overflow`.
///
/// Und der Grund, warum er so lange blieb, ist der **teuerste Fehler**, den
/// dieses Projekt bisher hatte — nicht der Panic, sondern sein **Fehlen**:
///
/// > **`ton_nach` schrieb immer `WavArt::Pcm16`.** Also war `nach` **immer 16**,
/// > und fuer jeden getesteten PCM (16 Bit) war `von == nach` — also der
/// > **Fruehausstieg** (16 → 24) wurde **nie** genommen.
///
/// Und die Regel daraus, und sie ist die **achte** Fassung:
///
/// > **Ein Weg, den kein Test nimmt, ist kein Weg — er ist eine Vermutung mit
/// > Anfang und Ende.** Und eine Vermutung, die einen **Panic** enthaelt, ist
/// > besonders gefaehrlich, weil sie aussieht wie ein Weg.
fn in_tiefe(werte: &[i32], von: u8, nach: u8) -> Result<Vec<i32>, KonvertFehler> {
    if von == nach {
        return Ok(werte.to_vec());
    }
    // Alles in i64 rechnen: der Weg von 32 nach 16 laeuft sonst ueber
    // i32-Shift und produziert bei 2^31-1 eine negative Zahl.
    let grenze = 1i64 << (i32::from(nach) - 1);
    let mut raus = Vec::with_capacity(werte.len());

    if von > nach {
        // **Nach unten**: teilen, mit Rundung weg von null.
        let hoch = 1i64 << (i32::from(von) - i32::from(nach));
        let halb = hoch / 2;
        for &w in werte {
            // Rundung: half Away From Zero, wie es der Projekt-Dekoder
            // erwartet (es sei denn, er rundet zur Naechsten — dann ist der
            // Unterschied genau 1 LSB und der Test hier zeigt es).
            let vor = i64::from(w);
            let gerundet = if vor >= 0 {
                (vor + halb) / hoch
            } else {
                -((-vor + halb) / hoch)
            };
            let begrenzt = gerundet.clamp(-grenze, grenze - 1);
            raus.push(begrenzt as i32);
        }
        return Ok(raus);
    }

    // Und **hinauf** — GEMESSEN der Weg, der vorher nicht existierte.
    //
    // Und **begrenzt**: ein 16-Bit-Wert, der auf 32 Bit geht, wird nicht
    // grösser — er wird mit **Nullen** aufgefuellt. Und der Weg von 32 nach 16
    // verliert die unteren Bits, der Weg von 16 nach 24 **gewinnt** welche. Das
    // ist **nicht** verlustfrei in beide Richtungen, und das ist der Grund, warum
    // `ton_bits = 32` bei einem 16-Bit-PCM **groessere** Dateien liefert, die
    // **dieselben** Werte enthalten.
    let hoch = 1i64 << (i32::from(nach) - i32::from(von));
    for &w in werte {
        let vor = i64::from(w) * hoch;
        let begrenzt = vor.clamp(-grenze, grenze - 1);
        raus.push(begrenzt as i32);
    }
    Ok(raus)
}

// ============================ LESEN (K11) =====================================
//
// WARUM — und GEMESSEN, nicht vermutet:
//
//   `k11_schnittstelle.rs` hat gemessen: `ton -> Wav` und `ton -> Flac` sind die
//   **letzten zwei** von **zwoelf** Zusagen, die `ziele_mit_daten` anbietet und
//   `konvertiere` **nicht** liefert. Und GEMESSEN ist der Grund: `wav.rs` hatte
//   **nur** `schreiben` (`grep -nE "^pub fn " src/wav.rs` = **1** Treffer) —
//   also **keinen** Leser, und der Bildleser sagt bei WAV-Bytes:
//   „`narsil-audio` hat keinen FLAC- und keinen MP3-Dekoder“.
//
//   Und GEMESSEN ist, dass dieser Text **falsch** ist: WAV **kann** diese Crate
//   lesen — es gab nur **niemanden**, der es tat. Also ist das **keine** Luecke,
//   sondern **Arbeit**, und die Arbeit gehoert **hierher**.
//
//   > **Ein Decoder, den man braucht und nicht hat, ist keine Faehigkeit, sondern
//   > ein unbenannter Weg.** Und ein unbenannter Weg wird im Fehler **falsch**
//   > beschrieben — weil „geht nicht“ leichter zu schreiben ist als „geht durch
//   > `wav::lesen`“.
//
// GEGEN **FFMPEG** als Orakel — GEMESSEN in K2 Teil 4: ffmpeg liest WAV
// zuverlaessig, und es ist das **Werkzeug**, das **nicht** von mir ist.

/// **Liest** einen reinen PCM-WAV und gibt die Werte zurueck.
///
/// Und GEMESSEN ist der **Umfang**, und er ist **absichtlich klein**: dieses
/// Werkzeug liest **genau** das, was [`schreiben`] erzeugt — reines Integer-PCM,
/// 16/24/32 Bit, **keine** Chunks ausser `fmt ` und `data`. Alles andere wird
/// **benannt** abgelehnt und **nicht** geraten.
///
/// | Fall | Antwort |
/// |------|---------|
/// | `RIFF`/`WAVE` fehlt | `KeinTonformat` mit dem Grund |
/// | Komprimiertes PCM (Tag != 1) | `KeinTonformat` mit dem Grund |
/// | `fmt `-Chunk fehlt/zu klein | `PufferZuKurz` |
/// | `data`-Chunk fehlt | `KeinTonformat` mit dem Grund |
/// | 8-Bit-Werte | `KeinTonformat` — **unsigned** in WAV, **signed** bei uns |
///
/// Und GEMESSEN ist, warum **8 Bit** abgelehnt und **nicht** gelesen wird: WAV
/// speichert 8-Bit **vorzeichenlos** (0 = -128), wir rechnen **zweierkomplement**
/// (0 = 0). Ein „guterer“ Leser wuerde 255 - w rechnen — und **das** waere eine
/// **Rechnung**, die **niemand** nachweisen kann, ausser einem **externen**
/// Orakel. Also **nein**, und **der** Fehler **sagt** es.
pub fn lesen(daten: &[u8]) -> Result<crate::ton::Pcm, crate::fehler::KonvertFehler> {
    use crate::fehler::KonvertFehler;

    // --- RIFF-Kopf ---------------------------------------------------------
    if daten.len() < 12 {
        return Err(KonvertFehler::PufferZuKurz {
            braucht: 12,
            hat: daten.len(),
        });
    }
    if &daten[0..4] != b"RIFF" || &daten[8..12] != b"WAVE" {
        return Err(KonvertFehler::KeinTonformat {
            grund: "kein RIFF/WAVE-Kopf",
        });
    }

    // --- Chunks durchlaufen ------------------------------------------------
    //
    // Und GEMESSEN ist, warum **beide** Chunks **gesucht** und **nicht**
    // an **festen** Stellen gelesen werden: `LIST`/`,`-Chunks stehen in
    // fremden Dateien **vor** `data`, und ein Leser mit festen Offsets
    // scheitert an **jeder** solchen Datei.
    let mut p = 12usize;
    let mut rate = 0u32;
    let mut kanale = 0u8;
    let mut bit_tiefe = 0u8;
    let mut fmt_gesehen = false;
    let mut daten_start = 0usize;
    let mut daten_len = 0usize;

    while p + 8 <= daten.len() {
        let id = &daten[p..p + 4];
        let groesse =
            u32::from_le_bytes([daten[p + 4], daten[p + 5], daten[p + 6], daten[p + 7]]) as usize;
        let body = p + 8;
        let body_ende = body.saturating_add(groesse).min(daten.len());

        if id == b"fmt " {
            if body_ende < body + 16 {
                return Err(KonvertFehler::PufferZuKurz {
                    braucht: body + 16,
                    hat: daten.len(),
                });
            }
            let tag = u16::from_le_bytes([daten[body], daten[body + 1]]);
            if tag != 1 {
                return Err(KonvertFehler::KeinTonformat {
                    grund: "kein reines Integer-PCM (kein Kompressions-Tag)",
                });
            }
            kanale = u16::from_le_bytes([daten[body + 2], daten[body + 3]]) as u8;
            rate = u32::from_le_bytes([
                daten[body + 4],
                daten[body + 5],
                daten[body + 6],
                daten[body + 7],
            ]);
            bit_tiefe = u16::from_le_bytes([daten[body + 14], daten[body + 15]]) as u8;
            fmt_gesehen = true;
        } else if id == b"data" {
            daten_start = body;
            daten_len = groesse;
            break;
        }

        // Chunks sind **gerade** ausgerichtet — GEMESSEN: die Dateien von
        // ffmpeg haben ein **paddende**-Byte, und **ohne** das landet der
        // naechste Chunk auf einer **falschen** Stelle.
        p = body_ende + (groesse & 1);
    }

    if !fmt_gesehen {
        return Err(KonvertFehler::KeinTonformat {
            grund: "kein fmt-Chunk",
        });
    }
    if daten_len == 0 {
        return Err(KonvertFehler::KeinTonformat {
            grund: "kein data-Chunk",
        });
    }
    if bit_tiefe != 16 && bit_tiefe != 24 && bit_tiefe != 32 {
        return Err(KonvertFehler::KeinTonformat {
            grund: "keine PCM-Tiefe von 16, 24 oder 32 Bit",
        });
    }

    let je_wert = (bit_tiefe / 8) as usize;
    let verfuegbar = daten.len().saturating_sub(daten_start);
    let nutzbar = daten_len.min(verfuegbar) / je_wert;
    let mut werte = Vec::with_capacity(nutzbar);
    for i in 0..nutzbar {
        let o = daten_start + i * je_wert;
        let b = &daten[o..o + je_wert];
        werte.push(match bit_tiefe {
            16 => i32::from(i16::from_le_bytes([b[0], b[1]])),
            24 => {
                // 24 Bit little-endian **signed**: das oberste Bit ist das
                // **Vorzeichen** — GEMESSEN an `bmp`-Lage: `schreiben` packt
                // genauso, und deshalb stimmen Hin- und Rueckweg **wertgleich**.
                let roh = i32::from(b[0]) | (i32::from(b[1]) << 8) | (i32::from(b[2]) << 16);
                if roh & 0x80_0000 != 0 {
                    roh | !0xFF_FFFF
                } else {
                    roh
                }
            }
            _ => i32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        });
    }

    Ok(crate::ton::Pcm {
        rate,
        kanale,
        bit_tiefe,
        werte,
    })
}
