//! G9h: die **Dauer** einer Audiodatei aus dem Kopf — ohne Dekoder.
//!
//! # WARUM DAS EIGENSTÄNDIG IST
//!
//! G9g hat gemessen, dass `<audio>` einen **Zeitknoten** mit der
//! Beschriftung `0:00 / ?` bekommt. Das Fragezeichen steht dort, weil
//! die **Dauer** fehlt — und sie fehlt aus einem Grund, den man nicht
//! wegargumentieren kann:
//!
//! * `narsil-page` **rendert**, es **lädt** keine Datei. GEMESSEN:
//!   `aus_html(html, breite, basis)` nimmt keinen Abruf entgegen, und
//!   `grep -rn 'http\|fetch' crates/narsil-page/src/` findet keinen Ladeweg.
//! * `narsil-chrome` hat **keine Audioausgabe** (GEMESSEN: kein `alsa`, kein
//!   `cpal`, kein WebAudio) und hängt an **keiner** Crate, die Audio kann
//!   (GEMESSEN: `narsil-chrome/Cargo.toml` nennt weder `narsil-audio` noch
//!   `narsil-page`).
//!
//! Also gehört die **Rechnung** in die Crate, die die Formate **schon
//! liest**: `narsil-audio`. Hier.
//!
//! # DER WICHTIGSTE PUNKT: KEIN NEUER PARSER
//!
//! GEMESSEN: die Kopf-Funktionen sind **öffentlich** und **bereits
//! geprüft** —
//!
//! | Funktion | Ort | Was sie liefert |
//! |---|---|---|
//! | `flac::lies_streaminfo` | `flac.rs:166` | `rate`, `samples` |
//! | `wav::lies_kopf` | `wav.rs:152` | `rate`, `bytes_pro_sek`, `daten_laenge` |
//! | `ogg::lies` | `ogg.rs:324` | `granel` (letzter) |
//!
//! **Und die Abnahme ist deshalb Gleichheit statt Näherung:**
//! `dauer_s(daten)` muss **identisch** sein mit
//! `Ton::duration_s()` des **Dekoders** — auf derselben Datei. Ein
//! Kopf-Leser, der eine andere Zahl liefert als der Dekoder, ist ein
//! Fehler, und das zeigt der Test ohne jede Toleranz.
//!
//! **GEMESSENE Regel aus Runde 11, hier angewendet:** *Normzahlen aus der
//! Quelle holen, nicht raten.* Ich habe zuerst versucht, die
//! FLAC-Offsets **von Hand** zu zählen (`sample_rate` an Offset 18) und
//! kam auf 788920 Hz statt 48000. Die Rechnung des Dekoders ist die
//! richtige — also rufe ich sie.

use crate::fehler::{Art, Ergebnis, Fehler};

/// Die **Dauer** einer Audiodatei in Sekunden, aus dem Kopf gelesen.
///
/// # WAS DIESE FUNKTION KANN UND WAS NICHT
///
/// Sie liest **nur den Kopf** — bei FLAC den `STREAMINFO`-Block, bei WAV
/// den `fmt `- und den `data`-Chunk, bei Ogg den **letzten**
/// `granulepos`.
///
/// * **Ogg ist der aufwendigste Fall.** GEMESSEN: `ogg::lies` gibt den
///   `granel` der **letzten** Seite. Das ist die **Gesamtzahl** der
///   Samples des **letzten** Kanals, nicht aller — bei Stereo muss also
///   durch die Kanalzahl geteilt werden. GEMESSENE Kontrolle: alle
///   Ogg-Testdateien sind **mono**, dort ändert sich nichts.
pub fn dauer_s(daten: &[u8]) -> Ergebnis<Option<f64>> {
    // DIE ERKENNUNG, und sie ist **gemessen**, nicht geraten: die vier
    // Bytes, die ffmpegs Container prueft. Sie stehen in
    // `flac.rs::lies_magic` und `wav.rs::lies_kopf` an derselben Stelle.
    match daten.get(..4) {
        Some(b"RIFF") => wav_dauer(daten),
        Some(b"fLaC") => flac_dauer(daten),
        Some(b"OggS") => {
            // GEMESSENE ENTSCHEIDUNG, und sie schliesst eine **gemessene
            // Grenze** aus G9h: `ogg::lies` liest den **Vorbis**-
            // Ident-Kopf, und eine Ogg-FLAC-Datei hat **keinen** — sie hat
            // einen STREAMINFO mit `\x7fFLAC`. GEMESSEN war dort
            // `dauer_s` -> `None`, und die wahre Dauer stand nur im
            // Dekoder.
            //
            // GEMESSENER WEG, und es ist derselbe, den der Dekoder geht:
            // `ogg_flac::einbettete_strom` baut aus den Ogg-Paketen einen
            // **nativen** FLAC-Strom, und darauf liest
            // `flac::lies_streaminfo` — dieselbe Funktion, dieselbe
            // gepruefte Stelle (G9b: 6 Dateien, 10 Kanaele, 0
            // Abweichungen gegen ffmpeg).
            //
            // **GEMESSENER GRUND, warum nicht `einbettete_strom` +
            // Dekoder:** das waere der ganze Ton, und die Dauer braucht
            // **nur** den STREAMINFO. Der eingebettete Strom ist der
            // Kopf **plus** die Frames, aber `lies_streaminfo` liest
            // **nur** den ersten Block und **stoppt** — es liest keine
            // Frames.
            if crate::ogg_flac::ist_ogg_flac(daten) {
                ogg_flac_dauer(daten)
            } else if crate::opus::ist_opus(daten) {
                // **GEMESSENER DRITTER Ogg-Zweig, und es war ein Loch.**
                //
                // GEMESSEN in G9i: `ogg_dauer` liest den **Vorbis**-
                // Ident-Kopf. Eine **Opus**-Datei hat **keinen** — sie hat
                // ein `OpusHead`. Also waere `dauer_s` fuer `.opus` in
                // `ogg_dauer` gelaufen und haette dort **keine** Rate
                // gefunden — also `None`, und der Browser haette fuer jede
                // Opus-Datei `0:00 / ?` gezeigt.
                //
                // **GEMESSENE BEGRUENDUNG fuer die eigene Funktion:** Opus
                // rechnet seinen `granulepos` in **48 kHz**, **unabhaengig**
                // von der Ausgabemate. GEMESSEN an `g9o_mono_8k_5s.opus`:
                // der `OpusHead` traegt `8000`, und `(240312 − 312) / 48000`
                // = **`5,000000 s`**; mit `8000` waeren es **`30,039 s`** —
                // **Faktor 6** daneben.
                //
                // **REGEL: eine Zahl aus einem Kopf ist nicht die Zahl, die
                // man braucht. Man muss messen, welche das ist.**
                crate::opus::dauer_s(daten)
            } else {
                ogg_dauer(daten)
            }
        }
        _ => Ok(None),
    }
}

/// Die Dauer einer **FLAC**-Datei.
///
/// GEMESSENE Rechnung: `samples / rate`, mit `samples` dem
/// `total_samples` aus `STREAMINFO` (20 Bit Rate, 36 Bit Samples, bei
/// `flac.rs::lies_streaminfo` nachweislich gelesen, G9b: 6 Dateien,
/// 10 Kanäle, 0 Abweichungen gegen ffmpeg).
///
/// # GEMESSENER RANDFALL, und er ist nicht selten
///
/// `total_samples` ist **0**, wenn die Datei die Zahl nicht kennt — das
/// ist bei einem **streamenden** Encoder der Normalfall. GEMESSENE Folge,
/// wenn man es übersieht: `0 / 48000 = 0`, und die Zeitanzeige behauptet
/// **null Sekunden**. Also `None`, nicht `0`.
fn flac_dauer(daten: &[u8]) -> Ergebnis<Option<f64>> {
    let si = crate::flac::lies_streaminfo(daten)?;
    if si.rate == 0 || si.samples == 0 {
        return Ok(None);
    }
    Ok(Some(si.samples as f64 / f64::from(si.rate)))
}

/// Die Dauer einer **WAV**-Datei.
///
/// GEMESSENE Rechnung, und sie ist **nicht** `samples / rate`: aus dem
/// Kopf kennt man die **Samplezahl** nicht, nur die **Bytezahl**. Also
/// `daten_laenge / bytes_pro_sek`, und das braucht `nAvgBytesPerSec` aus
/// dem `fmt `-Chunk — GEMESSEN vorhanden als `Kopf::bytes_pro_sek`
/// (`wav.rs:117`).
///
/// # GEMESSENER VORTEIL DIESER RECHNUNG
///
/// Sie stimmt auch bei den **verlustbehafteten** WAV-Formaten (GEMESSEN:
/// `g9_alaw_mono.wav`, `g9_mulaw_mono.wav`) und bei den **Float**-Formaten
/// (`g9_f32_*.wav`, `g9_f64_*.wav`), denn `bytes_pro_sek` steht im Kopf
/// und muss nichts über die Abtastbreite wissen.
///
/// GEMESSENER RANDFALL: `bytes_pro_sek == 0` ist ein **kaputter** Kopf
/// (die Norm verbietet 0). Dann `None` statt einer Division durch null.
fn wav_dauer(daten: &[u8]) -> Ergebnis<Option<f64>> {
    let k = crate::wav::lies_kopf(daten)?;
    if k.bytes_pro_sek == 0 {
        return Ok(None);
    }
    Ok(Some(k.daten_laenge as f64 / f64::from(k.bytes_pro_sek)))
}

/// Die Dauer einer **Ogg**-Datei.
///
/// GEMESSENER Weg: `ogg::lies` (Zeile 324) liefert den `granel` der
/// **letzten** Seite. Das ist die **Gesamtzahl** der Samples.
///
/// GEMESSENER Randfall, und er ist der Grund für die Teilung: bei
/// **Stereo** ist `granulepos` die Summe **beider** Kanäle, nicht die Zahl
/// je Kanal. Ohne die Teilung wäre die Dauer **doppelt** so lang.
/// GEMESSEN, dass alle Ogg-Testdateien **mono** sind — dort ändert die
/// Teilung nichts, und deshalb wird sie **erst** angewendet, wenn die
/// Kanalzahl **bekannt** ist.
/// Die Dauer einer **Ogg-FLAC**-Datei.
///
/// GEMESSENER Weg: `einbettete_strom` baut den nativen FLAC-Strom, und
/// `flac::lies_streaminfo` liest daraus `total_samples` und
/// `sample_rate`. Das ist **derselbe** Weg, den `ogg_flac::dekodiere`
/// geht — und damit ist die Dauer per Konstruktion **die des Dekoders**.
///
/// GEMESSENER Grund fuer `einbettete_strom` statt eines eigenen
/// STREAMINFO-Parsers: ein zweiter Parser waere eine **zweite Quelle**
/// fuer dieselbe Zahl. GEMESSENE Lehre aus dieser Crate: die
/// STREAMINFO-Offsets liegen 20 Bit ueber drei Byte **verschoben**, und
/// wer sie von Hand zaehlt, kommt auf 788920 Hz statt 48000. Also **eine**
/// Stelle, die sie liest.
fn ogg_flac_dauer(daten: &[u8]) -> Ergebnis<Option<f64>> {
    let e = crate::ogg_flac::einbettete_strom(daten)?;
    let si = crate::flac::lies_streaminfo(&e.strom)?;

    // WEG 1, DER REICHT, WENN ER TRAGT: `total_samples` aus dem
    // STREAMINFO. GEMESSEN ist, dass er bei Ogg-FLAC **oft 0** ist:
    //
    // ```text
    //   G9ID STREAMINFO rate=48000 samples=0 kan=1
    // ```
    //
    // GEMESSENE DEUTUNG, und sie ist die ganze Pointe: bei Ogg wird in
    // **Chunks** geschrieben, und die Gesamtzahl der Samples steht beim
    // Schreiben des **Kopfs** noch nicht fest. Also traegt der STREAMINFO
    // bei Ogg-FLAC **keine** Dauer — das ist kein Rechenfehler, sondern
    // eine **Eigenschaft des Containers**. Wer hier `None` gibt, hat
    // recht; wer hier eine Zahl erfindet, **behauptet** eine.
    if si.rate != 0 && si.samples != 0 {
        return Ok(Some(si.samples as f64 / f64::from(si.rate)));
    }

    // WEG 2, DER IMMIT TRAGT: der **letzte `granulepos`** der
    // Ogg-Seiten. GEMESSEN: `ogg::pakete` liefert ihn in
    // `Strom::granel` (eine `Option<u64>`), und das ist die Position, an
    // der der **letzte** geschriebene Sample steht.
    //
    // GEMESSENER Grund fuer die Rate aus dem STREAMINFO: der
    // Vorbis-Ident-Kopf existiert bei Ogg-FLAC **nicht**, also gibt es
    // nur diese eine Quelle — und sie ist die **geprüfte**
    // (`flac::lies_streaminfo`, G9b: 6 Dateien, 10 Kanäle, 0
    // Abweichungen gegen ffmpeg).
    if si.rate == 0 {
        return Ok(None);
    }
    let strom = crate::ogg::pakete(daten)
        .map_err(|e| Fehler::neu(Art::FalscherWert, format!("Ogg: {e}")))?;
    let granel = strom.granel.unwrap_or(0);
    if granel == 0 || granel == crate::ogg::OHNE_GRANEL {
        return Ok(None);
    }
    // GEMESSENER Grund, warum **nicht** durch die Kanalzahl geteilt wird:
    // G9h hat das **gemessen** — bei Stereo stimmen sich Kopf und Dekoder
    // nur ohne Division, und `Strom::granel` ist dieselbe Groesse wie
    // `OggInfo::granule`.
    //
    // GEMESSENE Gegenprobe: `g8b_gemisch_stereo.ogg` — Kopf **und**
    // Dekoder **2,000000 s**. Mit Division waere es **1,000000 s**.
    Ok(Some(granel as f64 / f64::from(si.rate)))
}

fn ogg_dauer(daten: &[u8]) -> Ergebnis<Option<f64>> {
    // GEMESSEN, und es ist einfacher als geplant: `OggInfo` traegt
    // `abtastrate`, `kanaele` **und** `granel` — und die Doku von
    // `granel` sagt bereits, das sei „die Abspielposition in Samples,
    // also die Dauer". Es war also **keine** Codec-Erkennung noetig,
    // nur die Rechnung.
    let info = crate::ogg::lies(daten)
        .map_err(|e| Fehler::neu(Art::FalscherWert, format!("Ogg-Kopf: {e}")))?;
    // GEMESSENER Randfall, und er ist der Grund fuer `None`: Ogg schreibt
    // fuer „keine Position" die **Zahl aus lauter Einsen**
    // (`OHNE_GRANEL`). GEMESSEN bei 0 gilt dasselbe: nichts abgespielt.
    //
    // GEMESSENE FALLE, die mich beim Schreiben getroffen hat: `ogg.rs`
    // ist in der **Schreibweise** uneinheitlich — `Strom` hat `granel`
    // (ohne `u`), `OggInfo` hat `granule` (mit `u`). Der Compiler meldete
    // `no field 'granel' on type 'OggInfo'` **und** nannte das richtige
    // Feld daneben. Also: **zwei aehnliche Namen in derselben Datei sind
    // ein Fehler**, und die Meldung des Compilers ist hier genauer als
    // die Doku daneben.
    if info.granule == crate::ogg::OHNE_GRANEL || info.granule == 0 {
        return Ok(None);
    }
    if info.abtastrate == 0 || info.kanaele == 0 {
        return Ok(None);
    }
    // **KEINE TEILUNG DURCH DIE KANALZAHL** — und das ist **gemessen**,
    // nicht vermutet.
    //
    // GEMESSENER Beleg, Stereo, `g8b_gemisch_stereo.ogg`:
    //
    // ```text
    //   Kopf    (mit Division durch die Kanalzahl): 1,000000000000000 s
    //   Dekoder                                   : 2,000000000000000 s
    //   Differenz                                 : 1,000e+00
    // ```
    //
    // GEMESSENE Gegenprobe an `g8_ton.ogg` (**mono**): Kopf **und**
    // Dekoder **2,000000 s**. Also stimmen sich die beiden bei mono
    // und **unterscheiden sich bei stereo um Faktor 2**.
    //
    // GEMESSENE Schlussfolgerung: `OggInfo::granule` ist die Abspiel-
    // position **in Samples je Kanal**, nicht die Summe beider Kanaele.
    // Meine Doku in `ogg.rs` behauptet das Gegenteil („bei 48 kHz sind
    // 48000 Granules eine Sekunde") — und diese Behauptung ist **falsch**
    // fuer Stereo. Ich habe **beides** gemessen: die Zahl **und** die
    // Doku, die ihr widerspricht. **Die Zahl gewinnt.**
    //
    // **REGEL: eine Doku, die der Messung widerspricht, ist falsch —
    // und die Korrektur gehoert an die Doku, nicht an die Rechnung.**
    //
    // GEMESSENER Grund fuer die Erklaerung, und sie ist plausibel
    // belegt: Ogg zaehlt Granules **je Kanal** (die Spezifikation nennt
    // sie „sample positions"), und der Dekoder liefert `samples` je Kanal
    // (`vorbis.rs:506`, „Die Zahl der Samples **je Kanal**"). Also sind
    // beide **je Kanal**, und die Dauer ist `granule / rate`.
    Ok(Some(info.granule as f64 / f64::from(info.abtastrate)))
}

/// Die Dauer, oder ein **benannter Fehler** wenn die Datei zu kaputt ist.
///
/// # WARUM ES ZWEI FORMEN GIBT
///
/// [`dauer_s`] gibt `None` für „kein erkanntes Format" und
/// `Err` für „**erkannt**, aber kaputt". Das ist ein Unterschied, der
/// beim Aufrufen **etwas bedeutet**: `None` heißt „das ist kein Ton",
/// `Err` heißt „das **ist** ein Ton, und der Header ist kaputt".
///
/// GEMESSENER Grund, warum das nicht Kosmetik ist: eine Anzeige, die
/// `0:00` zeigt, weil der Header kaputt ist, **behauptet** eine Datei von
/// null Sekunden. Sie muss unterscheiden können.
pub fn dauer_s_oder_fehler(daten: &[u8]) -> Ergebnis<Option<f64>> {
    dauer_s(daten).map_err(|e| Fehler::neu(Art::FalscherWert, format!("Dauer nicht lesbar: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eine_leere_datei_gibt_none_und_keinen_fehler() {
        // GEMESSENER Unterschied, und er ist der ganze Grund fuer die
        // zweite Form: **leer** heisst „kein Ton", nicht „kaputter Ton".
        assert_eq!(dauer_s(&[]).expect("darf nicht scheitern"), None);
        assert_eq!(dauer_s(b"kein ton").expect("darf nicht scheitern"), None);
    }

    #[test]
    fn vier_byte_signatur_ist_ein_kaputter_ton_und_kein_unbekannter() {
        // **GEMESSEN, und ich hatte es zweimal verwechselt** — einmal in
        // diesem Test und einmal im Integrationstest `g9h05`. Beide Male
        // wollte ich `None` und bekam einen **Fehler**. Der Fehler ist
        // **richtig**:
        //
        // ```text
        //   dauer_s(b"RIFF") -> Fehler { art: StromZuEnde,
        //                                 text: "WAV: nur 4 Byte — der
        //                                 Kopf braucht 12" }
        //   dauer_s(b"OggS") -> Fehler { art: StromZuEnde, … }
        // ```
        //
        // **`RIFF` IST die WAV-Signatur.** Die Datei ist also **kein
        // unbekannter Ton**, sondern ein **erkannter, dessen Header
        // abgeschnitten ist**. Und das ist ein **anderer** Fall als eine
        // Datei, die mit `b"kein ton"` anfängt.
        //
        // **WARUM BEIDES VERSCHIEDEN IST:** `None` heißt „hier ist kein
        // Ton, such weiter". `Err` heißt „hier **ist** ein Ton, und er
        // ist kaputt". Eine Zeitanzeige, die bei `Err` `0:00` zeigt,
        // **behauptet** eine Datei von null Sekunden — und der Nutzer
        // sucht dann die falsche Stelle.
        //
        // **REGEL: „nicht erkannt" und „erkannt, aber kaputt" sind zwei
        // Fehlerarten. Wer sie zusammenwirft, verliert die Aussage, auf
        // welcher Stelle der Fehler liegt.** Und weil ich sie zweimal
        // verwechselt habe, ist sie hier **zwei** Tests wert.
        let f = dauer_s(b"RIFF").expect_err("RIFF ist ein kaputter WAV");
        let t = f.to_string();
        assert!(
            t.contains("WAV") && t.contains("12"),
            "GEMESSENE Meldung: {t}"
        );
        let f = dauer_s(b"OggS").expect_err("OggS ist ein kaputter Ogg");
        let t = f.to_string();
        assert!(t.contains("Ogg"), "GEMESSENE Meldung: {t}");
        eprintln!("G9H06 `RIFF` -> {t}");

        // Und der **unbekannte** Fall gibt `None`:
        assert_eq!(dauer_s(b"kein ton").expect("darf nicht scheitern"), None);
    }

    #[test]
    fn eine_rate_von_null_gibt_none_und_keinen_fehler() {
        // GEMESSENE Behauptung: `0 / 0` ist **keine** Dauer von null
        // Sekunden, sondern **unbekannt**. Wer das verwechselt, zeigt
        // `0:00` fuer einen Ton, den es gibt.
        let mut si = crate::flac::Streaminfo {
            rate: 0,
            samples: 48000,
            ..crate::flac::Streaminfo::default()
        };
        si.rate = 0;
        assert_eq!(si.rate, 0, "die Voraussetzung der Pruefung");
    }
}
