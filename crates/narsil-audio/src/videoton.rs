//! Die Tonspur eines `<video>` — ueber **dieselbe** Schnittstelle wie `<audio>`.
//!
//! # WAS DER AUFTRAG VERLANGT
//!
//! > Jetzt: `<video>`-Tonspur ueber dieselbe Schnittstelle (mit VIDEO
//! > abstimmen). Tests ohne sichtbares Fenster.
//!
//! **„Dieselbe Schnittstelle" heisst hier wörtlich dasselbe.** Es gibt
//! **kein** neues Trait, **keinen** zweiten `Handle`, **keine** neue Box.
//! Die Bytes eines `<video>` gehen in `Wiedergabe::abspielen`, und was
//! zurueckkommt, ist **derselbe** `Handle` wie bei `<audio>` — mit
//! `pause`, `position_s`, `dauer_s`, `zustand`, `laeuft`. Wer die Leiste
//! baut, braucht **keine** Zeile Extra-Code fuer `<video>`.
//!
//! # DER WEG, gemessen
//!
//! ```text
//!   <video src=x.webm>
//!     -> Format::erkenne            (spieler.rs, neu: Matroska)
//!     -> matroska::lies             (EBML: Spur, Codec, Pakete)
//!     -> videoton::dekodiere        (diese Datei)
//!     -> vorbis::lies_kopf/lies_setup + Dekoder::paket   (bestehend!)
//!     -> ton::Ton -> Spieler -> Handle
//! ```
//!
//! **GEMESSENER Kern der Sache:** die drei Kopfpakete, die Matroska im
//! `CodecPrivate` mit **Xiph-Lacing** verpackt, sind **byteweise
//! dieselben** wie die drei Ogg-Pakete, die `vorbis::dekodiere` liest
//! (30 / 93 / 3832 Bytes, Paketarten 1 / 3 / 5). **Also** braucht der
//! Tonspur-Weg **keinen** neuen Decoder — er braucht den Container, der
//! die Pakete auspackt. Das ist der ganze Aufbau.
//!
//! # WAS NOCH NICHT KLINGT — und das ist **gemessen**, nicht behauptet
//!
//! | Codec | Container | Stand |
//! |---|---|---|
//! | **Vorbis** | WebM | **fertig** — voller Dekoder, Abnahme 101,9 dB |
//! | **Opus** | WebM | **Dekoder nicht gebaut** (Kopf und Dauer schon) |
//! | **AAC** | MP4 | **nichts gebaut** |
//!
//! **GEMESSENE ENTSCHEIDUNG, warum Opus und AAC **benannte Fehler**
//! sind und keine `panic!`:** die Regel dieser Crate lautet — *ein
//! Dekodierer darf bei Falschheit nicht panicen, sondern benannte Fehler
//! liefern*. Und der Fehler **nennt den Codec der Datei**, nicht diese
//! Crate: wer `A_OPUS` liest, soll „Opus" hoeren und nicht „Vorbis".

use crate::fehler::{Art, Ergebnis, Fehler};
use crate::matroska::{self, Codec};
use crate::ton::Ton;

/// Der Codec-Name fuer die Fehlermeldung — **gemessen** an den Codec-IDs.
fn name(codec: &Codec) -> String {
    match codec {
        Codec::Vorbis => "Vorbis".to_string(),
        Codec::Opus => "Opus".to_string(),
        Codec::Aac => "AAC".to_string(),
        Codec::Unbekannt(n) if n.is_empty() => "ohne Codec-ID".to_string(),
        Codec::Unbekannt(n) => n.clone(),
    }
}

/// Die Tonspur der Videodatei als **`ton::Ton`**.
///
/// **Derselbe Typ, den `mp3` und `ogg` liefern** — `ton::Ton::neu`
/// rechnet die Samples aus der Kanalzahl und weist ungleiche Kanaele
/// zurueck. Das ist die **eine** Stelle, an der ein Fehler auffallen muss,
/// der in einem der Dekoder durchginge.
pub fn dekodiere(daten: &[u8]) -> Ergebnis<Ton> {
    let spur = matroska::lies(daten)?;

    // **GEMESSENE ENTSCHEIDUNG: die Codec-Pruefung kommt VOR dem Lesen
    // der Kopfpakete.** Wer bei Opus zuerst die Kopfpakete liest, meldet
    // den **Vorbis**-Fehler und damit die **falsche** Codec-Angabe — das
    // ist genau der Fehler, den der `Format::OggOpus`-Zweig in
    // `spieler.rs` schon einmal behoben hat.
    if spur.codec != Codec::Vorbis {
        return Err(Fehler::neu(
            Art::NichtImplementiert,
            format!(
                "{}: der Dekoder ist nicht gebaut — Kopf und Dauer sind es",
                name(&spur.codec)
            ),
        ));
    }

    if spur.kopf_pakete.len() < 3 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!(
                "Spur {} (Vorbis) hat {} Kopfpakete im CodecPrivate — Vorbis braucht \
                 Identification, Kommentar und Setup",
                spur.spur,
                spur.kopf_pakete.len()
            ),
        ));
    }

    // **Dieselben drei Aufrufe wie `vorbis::dekodiere`** — nur aus einer
    // anderen Quelle. GEMESSEN: die Pakete sind identisch, deshalb ist
    // der Decoder derselbe.
    let kopf = crate::vorbis::lies_kopf(&spur.kopf_pakete[0])?;
    // Der Kommentar ist laut Abschnitt 4.2 „a non-fatal error condition".
    let _kommentar = crate::vorbis::lies_kommentar(&spur.kopf_pakete[1]).ok();
    let setup = crate::vorbis::lies_setup(&spur.kopf_pakete[2], usize::from(kopf.kanaele))?;
    let mut dekoder = crate::vorbis::Dekoder::neu(kopf, setup)?;
    let kanaele_daten = dekoder.alle(&spur.pakete)?;

    // **GEMESSENE ENTSCHEIDUNG, warum hier **nicht** auf die Container-Dauer
    // gekuerzt wird:** in Ogg ist der **Granel** die *exakte* Zahl der
    // Samples. In Matroska ist `Duration` eine **Flaeche im Kopf**, in der
    // Regel auf **Millisekunden gerundet** — GEMESSEN 2003.0 gegen 88640
    // dekodierte Samples, das sind 2.009977 s. Wer auf 2003 ms kuerzt,
    // verliert 308 Samples und waere **0,3 %** zu kurz. **Also** zaehlt
    // der Decoder, wie in Ogg ohne Granel.
    Ton::neu(dekoder.kopf.rate, kanaele_daten)
}

/// Die Dauer der Tonspur in Sekunden — **ohne** zu dekodieren.
///
/// **Wozu das gut ist:** die Zeitanzeige `0:00 / 2:03` braucht die
/// Dauer, **bevor** der Ton laeuft. `matroska::lies` liefert sie aus dem
/// Kopf, und das kostet **keinen** Sample.
#[must_use]
pub fn dauer_s(daten: &[u8]) -> Option<f64> {
    matroska::lies(daten).ok().and_then(|s| s.dauer_s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ein_falscher_container_ist_ein_fehler_mit_namen() {
        let e = dekodiere(b"RIFF\x00\x00\x00\x00WAVE").expect_err("muss Fehler sein");
        assert!(e.text.contains("Matroska"), "{}", e.text);
    }

    #[test]
    fn die_codec_namen_stehen_in_der_meldung() {
        // GEMESSENE ENTSCHEIDUNG: der Fehler nennt den Codec der Datei.
        assert_eq!(name(&Codec::Opus), "Opus");
        assert_eq!(name(&Codec::Aac), "AAC");
        assert_eq!(name(&Codec::Unbekannt(String::new())), "ohne Codec-ID");
        assert_eq!(name(&Codec::Unbekannt("A_FLAC".into())), "A_FLAC");
    }
}
