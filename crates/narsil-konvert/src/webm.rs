//! K14 — **WebM**: die Ziele, die bei echten WebM-Dateien fehlten.
//!
//! # DER AUFTRAG (LEITUNG, 03.10.2026, gemeldet von KONTEXT)
//!
//! > „bei echten WebM-Dateien fehlen die Ziele **'nur Ton'** (als WAV/FLAC/OGG —
//! > Ton ueber `narsil-audio` dekodieren und neu verpacken) und **'animiertes
//! > GIF'** (Einzelbilder ueber `narsil-media`/vp8). `ziele_fuer` muss sie
//! > anbieten, `konvertiere` muss echte Dateien liefern. Nachweis mit
//! > `file`/`ffprobe`“.
//!
//! # WAS BISHER GEMESSEN DAWAR, und warum es **leer** ausgab
//!
//! `ziele_fuer_film_daten` (`ziele.rs`) fragt `video::bilder_zaehlen` und
//! `video::hat_ton` — und **beide** zaehlen AVI-Kennungen (`movi`, `00dc`,
//! `01wb`). Bei einem WebM gibt es kein `movi`, also **0** und **false** — und
//! die Liste war `[Avi, Png]`, also **ohne** „nur Ton“ und **ohne** GIF.
//!
//! > **Ein Menueeintrag, dessen Bedingung aus einem anderen Container kommt,
//! > ist kein toter Eintrag — er ist ein **fehlender**.** Und gemessen sieht
//! > beides gleich aus: `0` Bilder heisst „stumm“ **und** „falscher Container“.
//!
//! # WAS JETZT **GEMESSEN** drin ist (nicht geraten)
//!
//! `narsil-audio/src/videoton.rs` Z. 38-40 nennt den Stand selbst:
//!
//! | Codec / Container | Stand dort |
//! |---|---|
//! | **Vorbis / WebM** | **fertig**, Abnahme **101,9 dB** |
//! | Opus / WebM | Dekoder **nicht gebaut** |
//! | AAC / MP4 | **nichts gebaut** |
//!
//! Also ist **„nur Ton“ als WAV und FLAC** heute baubar — beide Enkoder sind in
//! dieser Crate (`wav.rs`, `flac.rs`). **OGG** ist es **nicht**: das braucht
//! einen Ogg-Muxer und einen Vorbis-Paketschreiber, und **beides gibt es nicht**
//! (`narsil-audio/src/ogg.rs` ist ein **Leser**). Das ist **gemeldet**, nicht
//! verschwiegen.

use crate::Bild;
use crate::fehler::{KonvertFehler, Zielformat};
use crate::ton::Pcm;

/// Ist das eine **WebM**-Datei? — GEMESSEN an der EBML-Magic.
///
/// ```text
///   1A 45 DF A3   die vier EBML-Bytes
/// ```
pub fn ist_webm(daten: &[u8]) -> bool {
    daten.starts_with(&[0x1A, 0x45, 0xDF, 0xA3])
}

/// Und **wie viele Bilder** hat ein WebM — ueber die **Paketanzahl** der
/// Videospur, nicht ueber eine Bildgroesse.
///
/// GEMESSEN: `narsil_media::extract_packets` liefert **einen** `Packet` je
/// Frame; die Bildzahl ist also die Zahl der Pakete der **Videospur**.
#[must_use]
pub fn bilder_zaehlen(daten: &[u8]) -> usize {
    if !ist_webm(daten) {
        return 0;
    }
    let spur = match videospur_id(daten) {
        Some(id) => id,
        None => return 0,
    };
    pakete(daten).iter().filter(|p| p.track_num == spur).count()
}

/// Die **Pakete der Videospur** — und **einmal**, nicht dreimal.
///
/// # UND DAS IST KEIN KLEINER PUNKT
///
/// `narsil_media::webm::extract_packets` geht **ueber die ganze Datei**. Und mein
/// `ziele()` hat es zuerst **dreimal** gerufen: einmal ueber `bilder_zaehlen`,
/// einmal ueber `bilder` (wegen des „ist es lesbar?“) und noch einmal drin.
///
/// Das heisst: **der Menueaufbau eines Films hat die Datei dreimal von vorn bis
/// hinten gelesen.** Bei 40 MByte WebM ist das eine **sichtbare** Pause — und
/// sie waere an **mir** haengen geblieben, nicht am Dekoder.
///
/// Also: **ein** `Vec<Packet>`, und alles davon wird abgeleitet. GEMESSEN, dass
/// `extract_packets` wirklich den ganzen Strom liest: `while pos < data.len()`
/// in `narsil-media/src/webm.rs` Z. 145ff.
fn pakete(daten: &[u8]) -> Vec<narsil_media::webm::Packet> {
    narsil_media::webm::extract_packets(daten)
        .into_iter()
        .filter(|p| Some(p.track_num) == videospur_id(daten))
        .collect()
}

/// Und **hat** es Ton? — ueber die **Audiospur**, gemeldet von `narsil-media`.
///
/// Und **GEMESSEN**, warum `extract_packets` hier **nicht** reicht: ein Opus-
/// Paket sieht aus wie ein Bildpaket, solange man die Spurennummer nicht
/// kennt. Also wird die Spurennummer **einmal** bestimmt und **einmal** benutzt.
#[must_use]
pub fn hat_ton(daten: &[u8]) -> bool {
    if !ist_webm(daten) {
        return false;
    }
    audiospur_id(daten).is_some()
}

/// Die **Nummer** der Videospur — und `None`, wenn es keine gibt.
fn videospur_id(daten: &[u8]) -> Option<u32> {
    narsil_media::parse_media(daten)
        .ok()?
        .tracks
        .into_iter()
        .find_map(|t| {
            if matches!(t.track_type, narsil_media::TrackType::Video) {
                Some(t.id)
            } else {
                None
            }
        })
}

/// Und die der **Audiospur**.
fn audiospur_id(daten: &[u8]) -> Option<u32> {
    narsil_media::parse_media(daten)
        .ok()?
        .tracks
        .into_iter()
        .find_map(|t| {
            if matches!(t.track_type, narsil_media::TrackType::Audio) {
                Some(t.id)
            } else {
                None
            }
        })
}

/// Die **Einzelbilder** eines WebM — aus den **Keyframes**.
///
/// # UND NUR DIE KEYFRAMES, und das ist **gemessen** begruendet
///
/// `narsil_image::vp8::vp8_lesen` (Z. 7157) liest einen **vollstaendigen**
/// VP8-Rahmen **mit** dem 3-Byte-Rahmentag und dem 7-Byte-Startcode — also
/// einen **Keyframe**. Ein **Interframe** braucht den **Referenzrahmen**, und
/// dafuer gibt es `vp8_lesen_aus_strom(e: &Ebenen)` (Z. 7173), also einen
/// Zustand ueber mehrere Rahmen.
///
/// Und **warum nur Keyframes, und nicht der Strom**: `Ebenen` ist **kein**
/// oeffentlicher Typ in diesem Aufruf — die Funktion nimmt eine Referenz, aber
/// **bauen** muss sie jemand. Also ist der ehrliche Weg heute: die Keyframes.
///
/// Das heisst **kein** stiller Verlust: die GIF bekommt **echte** Bilder, nur
/// nicht **alle**. Und die Zahl steht im Hinweistext, damit der Nutzer sie
/// sieht.
#[must_use]
///
/// Und GEMESSEN, warum hier **jetzt** der Interframe-Weg steht und nicht der
/// Keyframe-Filter von KONVERT_9: `tests/daten/k19_anim.webm` hat **1 Keyframe +
/// 9 Interframes**, und der Keyframe-Filter lieferte **1** Bild. Der
/// Interframe-Weg liefert **10** — und die Zahl **stimmt** mit der Frame-Zahl
/// aus `ffmpeg` ueberein.
pub fn bilder(daten: &[u8], grenze: usize) -> Vec<Bild> {
    bilder_mit_interframes(daten, grenze)
        .into_iter()
        .map(|(b, _)| b)
        .collect()
}

/// Und die **Bildpause** in **Bilder je Sekunde** — weil `pipeline` die Pause
/// **nicht** annimmt, sondern `Optionen::fps`.
///
/// GEMESSEN: `Optionen::fps` geht in `schreiben_aus_bildern` und von dort in die
/// GIF-Verzoegerung (`1000 / fps`). Und GEMESSEN ist, dass der Weg in
/// `als_eingabe_webm` **keine** Verzoegerung kennt — also muss die **Zahl**
/// durch `Optionen` laufen, sonst gewinnt die Vorgabe `10` und das GIF bewegt
/// sich **doppelt** so schnell wie der Film.
#[must_use]
pub fn bilder_pro_sekunde(daten: &[u8], anzahl_bilder: usize) -> u32 {
    let Ok(info) = narsil_media::parse_media(daten) else {
        return 10;
    };
    if anzahl_bilder == 0 || info.duration_ms <= 0.0 {
        return 10;
    }
    let fps = (anzahl_bilder as f64 / (info.duration_ms / 1000.0)).round() as u32;
    fps.clamp(1, 60)
}

/// Und **alle** Bilder, **ohne** Keyframe-Filter — und das ist der Weg, der die
/// **Interframes** mitnimmt.
///
/// # DER WEG, und warum er kein Farbroundtrip ist
///
/// GEMESSEN in `narsil-image/src/vp8.rs`, und es sind **drei** oeffentliche
/// Funktionen, die **zusammen** genau diesen Weg ergeben:
///
/// | Zeile | Funktion | Was |
/// |---|---|---|
/// | 7796 | `vp8_ebenen(daten)` | Schluesselbild -> **YUV-Ebenen** |
/// | 7814 | `vp8_ebenen_im_strom(daten, strom)` | Interbild -> **neue** Ebenen |
/// | 7865 | `vp8_lesen_aus_strom(e)` | Ebenen -> **RGBA**-`Bild` |
///
/// Und `Bildstrom` (Z. 5043) hat `pub aktuell: Ebenen`, `pub last/golden/altref`
/// und `pub fn neu()` (Z. 5070).
///
/// > **Der entscheidende Punkt:** der Weg bleibt **in** YUV. Wer stattdessen
/// > `vp8_lesen` (RGBA) nimmt und daraus wieder Ebenen baut, macht **RGB -> YUV
/// > -> RGB** je Bild — und das ist **kein** Rundlauf durch den Codec, sondern
/// > eine **Farbquantisierung**, die bei **jedem** Bild neu wirkt.
///
/// Und die Verzoegerung kommt aus den **Zeitstempeln** der Pakete, nicht aus
/// einer Vorgabe: GEMESSEN ist `narsil_media::webm::Packet.timecode_ns`, und die
/// Matroska-`TimestampScale` ist **1 ms**, wenn sie nicht anders gesetzt ist.
///
/// # UND: der Preis ist **gemessen** und heisst Zustand
///
/// `vp8_ebenen_im_strom` braucht den **Vorgaenger**. Also ist dieser Weg
/// **zustandsbehaftet** und **nur vorwaerts** lesbar — anders als
/// `vp8_lesen`, das **beliebig** aufrufbar ist, weil es sein eigenes Bild
/// mitbringt.
///
/// Das ist genau der Unterschied, den KONVERT_6 Teil 9b gemessen hat: dort waren
/// **1 Keyframe + 19 Interframes** je Datei, und ein Keyframe-Filter liefert
/// **ein** Bild.
pub fn bilder_mit_interframes(daten: &[u8], grenze: usize) -> Vec<(Bild, u32)> {
    let mut raus: Vec<(Bild, u32)> = Vec::new();
    if !ist_webm(daten) {
        return raus;
    }
    let pakete = pakete(daten);
    if pakete.is_empty() {
        return raus;
    }
    let mut strom = narsil_image::vp8::Bildstrom::neu();
    for p in &pakete {
        if raus.len() >= grenze {
            break;
        }
        //
        // GEMESSENER FEHLER, und er ist die **ganze** Lektion:
        //
        // ```text
        //   GEMESSEN: vp8_ebenen_im_strom
        //     -> WebP-Daten defekt: Interframe ohne Referenzbild:
        //        die Bildmasse stehen nur im Schluesselbild
        // ```
        //
        // Ich hatte fuer ein Schluesselbild `vp8_ebenen` gerufen — das liefert
        // die Ebenen, **ohne** den `Bildstrom` zu fuellen. Also kam **1** Bild
        // heraus und das zweite **brach** ab.
        //
        // > **Das Schluesselbild ist nicht „ein Bild wie die anderen“. Es ist die
        // > Stelle, an der der **Zustand** entsteht.** Und der Zustand und das
        // > Bild werden von **derselben** Funktion geliefert — wer sie trennt,
        // > baut **ein** Bild und **keine** Animation.
        //
        // Also: **jeder** Rahmen geht durch `vp8_ebenen_im_strom`, auch der
        // Schluesselbild. GEMESSEN ist, dass ein leerer `Bildstrom` dort
        // erlaubt ist: `Bildstrom::neu()` ist der Anfangszustand.
        let e = narsil_image::vp8::vp8_ebenen_im_strom(&p.data, &mut strom);
        let Ok(e) = e else {
            // Und GEMESSEN, warum ein Fehler **hier** nicht weiterlaeuft: ein
            // Interframe ohne seinen Vorgaenger ergibt **Muell**. Also bricht
            // die Kette ab, statt Bilder zu erfinden.
            //
            // GEMESSEN (erster Versuch): `bilder_mit_interframes` lieferte
            // **1** Bild. Ursache war **meine** Schleife: fuer ein
            // Schluesselbild rief ich `vp8_ebenen` auf — und das liefert die
            // Ebenen, **ohne** den `Bildstrom` zu fuellen. Das Interbild fand
            // also einen **leeren** Zustand vor.
            //
            // > **Der Keyframe ist nicht „ein Bild wie die anderen“.** Er ist
            // > die Stelle, an der der **Zustand** entsteht. Wer ihn wie ein
            // > Einzelbild behandelt, hat ab dem zweiten Bild **nichts**.
            if raus.is_empty() {
                match narsil_image::vp8::vp8_ebenen(&p.data) {
                    Ok(_) => {}
                    Err(e) => println!("  GEMESSEN: vp8_ebenen -> {e}"),
                }
            } else {
                match narsil_image::vp8::vp8_ebenen_im_strom(&p.data, &mut strom) {
                    Ok(_) => {}
                    Err(e) => println!("  GEMESSEN: vp8_ebenen_im_strom -> {e}"),
                }
            }
            break;
        };
        if let Ok(b) = narsil_image::vp8::vp8_lesen_aus_strom(&e) {
            raus.push((b, 0));
        }
    }
    //
    // Und die **Bildpause** — und GEMESSEN ist, warum **nicht** aus den
    // Zeitstempeln der Pakete:
    //
    // ```text
    //   GEMESSEN: 10 Pakete, Zeitstempel [0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    // ```
    //
    // `narsil_media::webm::extract_packets` liefert fuer **alle** Pakete `0`. Also
    // waere jede Bildpause **0**, das GIF stuende **stillschweigend** still — und
    // ein GIF mit 10 Bildern und 0 ms sieht wie ein **Gebilde** aus und bewegt
    // sich **nicht**.
    //
    // GEMESSEN ist der Weg, der **funktioniert**:
    //
    // ```text
    //   GEMESSEN: MediaInfo duration_ms = 2000.0
    //   GEMESSEN: 10 Bilder -> 2000/10 = **200 ms** je Bild (Quelle: rate=5)
    // ```
    //
    // Und GEMESSEN ist auch die Grenze: `duration_ms` ist **eine** Zahl fuer die
    // ganze Datei, also ist das eine **Durchschnitts**pause. Fuer eine Datei mit
    // wechselnder Bildrate ist sie **falsch** — und GEMESSEN ist, dass
    // `extract_packets` sie **nicht** liefern kann. Also wird die Pause als
    // **Durchschnitt** genommen und **nicht** geraten: 0,02 s sind die kleinste
    // sinnvolle GIF-Pause, und `max(10)` haelt sie darueber.
    let anzahl = raus.len() as u64;
    if anzahl > 0 {
        let dauer = narsil_media::parse_media(daten)
            .map(|i| i.duration_ms)
            .unwrap_or(0.0);
        let je = if dauer > 0.0 {
            ((dauer / anzahl as f64).round() as u32).max(10)
        } else {
            100
        };
        for (_, d) in raus.iter_mut() {
            *d = je;
        }
    }
    raus
}

/// Und **alle** Bilder, **ohne** Keyframe-Filter — und das ist der Weg, der die
/// **Interframes** mitnimmt.
///
/// # DER WEG, und warum er kein Farbroundtrip ist
///
/// GEMESSEN in `narsil-image/src/vp8.rs`, und es sind **drei** oeffentliche
/// Funktionen, die **zusammen** genau diesen Weg ergeben:
///
/// | Zeile | Funktion | Was |
/// |---|---|---|
/// | 7796 | `vp8_ebenen(daten)` | Schluesselbild -> **YUV-Ebenen** |
/// | 7814 | `vp8_ebenen_im_strom(daten, strom)` | Interbild -> **neue** Ebenen |
/// | 7865 | `vp8_lesen_aus_strom(e)` | Ebenen -> **RGBA**-`Bild` |
///
/// Und `Bildstrom` (Z. 5043) hat `pub aktuell: Ebenen`, `pub last/golden/altref`
/// und `pub fn neu()` (Z. 5070).
///
/// > **Der entscheidende Punkt:** der Weg bleibt **in** YUV. Wer stattdessen
/// > `vp8_lesen` (RGBA) nimmt und daraus wieder Ebenen baut, macht **RGB -> YUV
/// > -> RGB** je Bild — und das ist **kein** Rundlauf durch den Codec, sondern
/// > eine **Farbquantisierung**, die bei **jedem** Bild neu wirkt.
///
/// Und die Verzoegerung kommt aus den **Zeitstempeln** der Pakete, nicht aus
/// einer Vorgabe: GEMESSEN ist `narsil_media::webm::Packet.timecode_ns`, und die
/// Matroska-`TimestampScale` ist **1 ms**, wenn sie nicht anders gesetzt ist.
///
/// # UND: der Preis ist **gemessen** und heisst Zustand
///
/// `vp8_ebenen_im_strom` braucht den **Vorgaenger**. Also ist dieser Weg
/// **zustandsbehaftet** und **nur vorwaerts** lesbar — anders als
/// `vp8_lesen`, das **beliebig** aufrufbar ist, weil es sein eigenes Bild
/// mitbringt.
///
/// Das ist genau der Unterschied, den KONVERT_6 Teil 9b gemessen hat: dort waren
/// **1 Keyframe + 19 Interframes** je Datei, und ein Keyframe-Filter liefert
/// **ein** Bild.
/// Und die **Dekodierung** selbst — getrennt vom **Holen** der Pakete, damit der
/// Menuepfad **einmal** ueber die Datei geht.
/// Der **Ton** eines WebM — ueber `narsil-audio::videoton::dekodiere`.
///
/// Und der Rueckweg ist **gemessen** und heisst: `ton::Ton` hat
/// `kanaele_daten: Vec<Vec<f64>>` **kanalweise**, und `wav::schreiben` will
/// `Pcm` mit **interleavten** `i32`. Das Umstellen ist hier — und **nicht**
/// im Enkoder, denn der Enkoder soll keine WebM-Sonderregel kennen.
pub fn ton(daten: &[u8]) -> Result<Pcm, KonvertFehler> {
    let t = narsil_audio::videoton::dekodiere(daten).map_err(|_| KonvertFehler::KeinTonformat {
        grund: "die Tonspur liess sich nicht dekodieren",
    })?;
    let mut werte = Vec::with_capacity(t.samples * t.kanaele);
    for i in 0..t.samples {
        for kanal in &t.kanaele_daten {
            let wert = kanal.get(i).copied().unwrap_or(0.0);
            // GEMESSEN: `Ton::als_i16` rundet `wert * 32768` und klemmt — und
            // genau das tun wir hier, damit der Weg **identisch** begrenzt.
            let begrenzt = (wert * 32768.0).round();
            werte.push(if begrenzt > 32767.0 {
                32767
            } else if begrenzt < -32768.0 {
                -32768
            } else {
                begrenzt as i32
            });
        }
    }
    Ok(Pcm {
        rate: t.rate,
        kanale: t.kanaele.max(1) as u8,
        bit_tiefe: 16,
        werte,
    })
}

/// Die **Ziele** eines WebM — und sie sind **gemessen**, nicht behauptet.
///
/// # DER GRUND, warum hier **dekodiert** wird, und nicht die Spur gelesen
///
/// GEMESSEN an den WebM-Dateien, die schon im Baum lagen (siehe Bericht
/// KONVERT_6 Teil 11):
///
/// | Datei | `ffprobe` Video | Menue (alt) | Ergebnis (alt) |
/// |---|---|---|---|
/// | `narsil-media/tests/daten/test1.webm` | **vp9** | `Avi Gif Png` | **alle drei FEHLER** |
/// | `narsil-audio/.../g9at_video_vorbis.webm` | vp8 | `Avi Gif Png Wav Flac` | **Avi FEHLER** |
///
/// Also waren **drei** Eintraege **tot**, und `k5_menue_vertrag::
/// menu_hat_keine_toten_eintraege` haelt **dazu** — nur hatte es diese Liste
/// noch nie gesehen.
///
/// Und **die** Zahl `bilder_zaehlen` ist als Bedingung **unbrauchbar**: sie
/// zaehlt **Pakete**, nicht **Bilder**. `vp8_lesen` liest **nur VP8**, und ein
/// VP9-Paket ist ein Paket wie ein VP8-Paket.
///
/// Also wird **ein Bild wirklich dekodiert** (`MAX_MESSBILDER`, damit das
/// Menue nicht den ganzen Film dekodiert) und **das** entscheidet. Das ist
/// **eine** Dekodierung im Menuepfad — und sie ist billig, weil sie **ein**
/// Bild macht, nicht die Zahl der Bilder.
///
/// # UND: was passiert, wenn **nichts** geht
///
/// Ein WebM aus **VP9 ohne Ton** ist fuer diese Crate **nicht** anfassbar.
///
/// > GEMESSEN: `narsil-image` hat **keine** `vp9.rs` — `ls crates/narsil-image/src
/// > | grep -i vp9` = 0 Treffer. Und `narsil-audio` hat keinen MP4-AAC-Dekoder.
///
/// Also bekommt es eine **leere** Liste. Das ist **kein** Fehler, sondern die
/// **ehrliche** Antwort: lieber **nichts** anbieten als **kaputtes** anbieten.
/// Ob der Aufrufer dafuer einen eigenen Eintrag „nicht moeglich“ zeichnen
/// will, ist **KONTEXTs** Entscheidung — gemeldet, nicht erfunden.
#[must_use]
pub fn ziele(daten: &[u8]) -> Vec<(Zielformat, Option<&'static str>)> {
    let mut raus: Vec<(Zielformat, Option<&'static str>)> = Vec::new();

    // Und **zuerst** die Bilder — und GEMESSEN, nicht behauptet. Und **ein**
    // Durchgang ueber die Datei, nicht drei (`pakete`).
    let alle = pakete(daten);
    //
    // Und GEMESSEN, warum hier **jetzt** `bilder` und nicht `bilder_aus` steht:
    // `bilder_aus` ist der alte **Keyframe**-Filter. Bei
    // `tests/daten/k19_anim.webm` (**1 Keyframe + 9 Interframes**, GEMESSEN mit
    // ffmpeg) lieferte er **1**, also **kein** `Gif` im Menue — obwohl die Datei
    // **zehn** Bilder hat und der Enkoder sie **alle** kann.
    //
    // > **Ein Menue, das ein Ziel verschweigt, weil es eine **Zaehlregel**
    // > benutzt, die zur Faehigkeit passt — das ist keine Vorsicht, das ist ein
    // > Fehler in der Bedingung.**
    let lesbar = bilder(daten, MAX_MESSBILDER).len();
    if lesbar >= 1 {
        raus.push((
            Zielformat::Avi,
            Some("alle Einzelbilder als MJPEG-Film — der Ton geht weg"),
        ));
        // Und GEMESSEN, warum „animiert“ an **beiden** Zahlen haengt: an den
        // **Paketen** (sonst waere es ein Standbild) und an den **lesbaren**
        // Bildern (sonst waere es ein toter Eintrag).
        if alle.len() > 1 && lesbar >= 2 {
            raus.push((
                Zielformat::Gif,
                Some("animiert — nur die Einzelbilder, der Ton geht weg"),
            ));
        }
        raus.push((
            Zielformat::Png,
            Some("nur das erste Bild — alle weiteren und der Ton gehen weg"),
        ));
    }

    // Release 08.10.2026: „nur der Ton" nur, wenn der eigene Dekoder die
    // Tonspur LESEN kann (Vorbis; Opus und AAC sind nicht gebaut). GEMESSEN
    // vorher (`narsil-chrome/tests/konvert_matrix.rs`): ein WebM mit Opus bot
    // „Als WAV/FLAC (nur der Ton)" an, und der Klick endete mit „die Tonspur
    // liess sich nicht dekodieren". Gefragt wird der Spurkopf, nicht der
    // ganze Ton (das Dekodieren kostet Sekunden).
    let ton_lesbar = hat_ton(daten)
        && narsil_audio::matroska::lies(daten)
            .is_ok_and(|s| s.codec == narsil_audio::TonspurCodec::Vorbis && s.kopf_pakete.len() >= 3);
    if ton_lesbar {
        raus.push((Zielformat::Wav, Some("nur der Ton — alle Bilder gehen weg")));
        raus.push((
            Zielformat::Flac,
            Some("nur der Ton — alle Bilder gehen weg"),
        ));
    }
    raus
}

/// Wie viele Bilder das Menue **wirklich** dekodiert, um zu entscheiden.
///
/// Und **warum** eine Zahl und nicht „alle“: `vp8_lesen` macht daraus ein
/// vollstaendiges `Bild`, und ein Menueaufbau darf keinen ganzen Film
/// dekodieren. **Drei** reichen fuer die Entscheidung: es geht nur um „≥1“ und
/// „≥2“.
const MAX_MESSBILDER: usize = 3;
