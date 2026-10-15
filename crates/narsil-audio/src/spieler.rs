//! Der Spieler: aus einer Audiodatei ein **abspielbares** Stück Ton.
//!
//! # WAS DIESES MODUL IST und warum es ueberhaupt existiert
//!
//! GEMESSENER Stand vor diesem Modul (02.10.2026): `narsil-audio` hatte
//! drei **Dekoder** (WAV, FLAC, Vorbis), eine **Ausgabe** ueber
//! `aplay`/`pw-play` als Kindprozess — und **keine Verbindung dazwischen**.
//! Es gab keinen Weg von einer Datei zu Samples zu Ton, und erst recht
//! keine Zeitangabe. Genau die verlangt die Abnahme:
//!
//! > Seite mit `<audio src=*.flac>` spielt, **Zeitanzeige laeuft**.
//!
//! Der Browser kann das nicht aus der Datei allein wissen: die Zeit
//! laeuft **waehrend der Wiedergabe**, nicht aus dem Dateikopf. Deshalb
//! gehoert sie hierher, in die Crate, die den Ton auch erzeugt — und
//! nicht in den Browser, der sie nur durchreichen muss.
//!
//! # DIE ZEIT IST DER **VERBRAUCHTE** TON, NICHT DER GEWANDELTE
//!
//! GEMESSENE Entscheidung, und sie ist nicht selbstverstaendlich: Die
//! Position wird aus der **tatsaechlich abgegebenen** Sample-Menge
//! gebildet, nicht aus einer internen Zaehlinvariante. Der Grund ist der
//! Puffer:
//!
//! ```text
//!   Kindprozess-Ausgabe: MAX_PUFFER = 512 * 1024 Byte  (pcm.rs)
//! ```
//!
//! Bei 48000 Hz und 16 Bit sind das rund 5,3 s **Warteschlange**. Eine
//! Zeitanzeige aus einem internen Zaehler laege am Anfang um diese
//! fuenf Sekunden voraus und wuerde danach klemmen. Aus dem Verbrauch
//! kann sie das nicht.
//!
//! # WAS NOCH FEHLT, mit Namen
//!
//! * **Der Browser** muss `<audio>` von einem Platzhalter zu einem
//!   Steuerelement machen und `duration`/`currentTime` durchreichen.
//!   GEMESSEN heute (`narsil-page/src/lib.rs:5405`): `<audio>` schreibt
//!   `[Audio: {src}]` in den Platzhalter, und `currentTime`, `timeupdate`
//!   und `canplay` kommen im ganzen Baum **nicht vor**. Das ist eine
//!   fremde Crate, der Wunsch steht in `team/WUNSCH_KOMPRESS.txt`.
//! * **Ogg-FLAC ist nicht gebaut.** GEMESSEN: `OggS` gibt einen benannten
//!   Fehler; gebaut sind nackte FLAC-Dateien. Fuer die Abnahme also
//!   `<audio src=*.flac>`; Ogg-Vorbis ueber `vorbis::dekodiere` geht,
//!   Ogg-FLAC nicht.

use crate::fehler::{Ergebnis, Fehler};
use crate::pcm::Ausgabe;
use crate::ton::Ton;

/// Wie viele Samples je Schritt in die Ausgabe gehen.
///
/// GEMESSENE Wahl aus `MAX_PUFFER`: bei 48000 Hz und 16 Bit passen
/// 512 KiB in den Puffer, das sind rund 5,3 s. Ein Schritt von 4096
/// Samples ist 85 ms — klein genug, dass `pause` und
/// `position_setzen` schnell genug wirken, und gross genug, dass die
/// Ausgabe nicht bei jedem Sample einen Schreibvorgang macht.
pub const SCHRITT: usize = 4096;

/// Wie das Format erkannt wurde.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// RIFF/WAVE.
    Wav,
    /// Ein nackter FLAC-Stream.
    Flac,
    /// Ogg mit Vorbis.
    OggVorbis,
    /// Ogg mit FLAC (`\x7fFLAC`).
    ///
    /// GEMESSENER Grund fuer einen **eigenen** Fall und nicht
    /// `OggVorbis` fuer beides: Ogg-FLAC und Ogg-Vorbis unterscheiden sich
    /// **erst in Paket 0**, nicht im Seitenkopf. Beide beginnen mit
    /// `OggS`. Wer sie unter einem Fall zusammenfasst, laesst den
    /// Vorbis-Dekoder an einem FLAC-Strom scheitern — mit einer
    /// Fehlermeldung ueber Ogg, nicht ueber FLAC.
    OggFlac,
    /// Ogg mit **Opus** (`OpusHead`).
    ///
    /// GEMESSENER Grund fuer einen **eigenen** Fall, und es ist derselbe
    /// wie bei Ogg-FLAC: Ogg-Opus, Ogg-FLAC und Ogg-Vorbis unterscheiden
    /// sich **erst in Paket 0**, nicht im Seitenkopf. Alle drei beginnen mit
    /// `OggS`.
    ///
    /// **GEMESSENER FEHLER, den das Enum behebt:** vorher gab
    /// [`Format::erkenne`] fuer **jede** unbekannte Ogg-Datei
    /// `OggVorbis` zurueck — also auch fuer eine **Opus**-Datei. Danach
    /// scheiterte `vorbis::dekodiere` an einem `OpusHead`-Paket, weil es
    /// den **Vorbis**-Ident-Kopf in Paket 0 verlangt, und die Meldung
    /// nannte **Vorbis**. Das war eine **Falschangabe** ueber die Datei.
    ///
    /// **GEMESSENER Ort der Entscheidung, und er ist derselbe wie bei den
    /// beiden anderen:** **Byte 28** der Datei (27 Byte Seitenkopf plus 1
    /// Byte Segmenttabelle). GEMESSEN an `g9o_ton_48k.opus`: dort steht
    /// woertlich `OpusHead`.
    OggOpus,
    /// **MP3 / MPEG-1 Layer III** (KOMPRESS 04.10., Anordnung der Leitung:
    /// „Klick auf Play in `<audio controls>` spielt hoerbar ab
    /// (**WAV/FLAC/OGG/MP3**)").
    ///
    /// **GEMESSENER Grund, warum die fehlte:** `erkenne` nahm **vor** diesem
    /// Zweig nur `RIFF`, `fLaC` und `OggS` an, und die Fehlermeldung sagte
    /// woertlich **„kein erkanntes Audioformat: erwartet RIFF, fLaC oder
    /// OggS"**. **Also** war eine **MP3-Datei** fuer den Spieler **kein
    /// Audio**, und der Dekoder `mp3::dekodiere` (`mp3.rs:514`) war
    /// **unerreichbar** — obwohl er **da** ist und **333 gruene Tests**
    /// hat.
    ///
    /// **GEMESSEN ist auch, dass der Weg funktioniert:** `mp3::dekodiere`
    /// liefert `Ergebnis<Ton>`, und G9ap hat damit drei echte **Mono**-MP3
    /// ueber 13 bis 15 s dekodiert (Länge auf **1e-4 s** genau gegen
    /// `ffprobe`).
    Mp3,
    /// **KOMPRESS 03.10.: Matroska/WebM — die `<video>`-Tonspur.**
    ///
    /// **GEMESSENER Grund, warum das ein **eigener** Fall ist:** `erkenne`
    /// sieht nur die **ersten vier** Byte, und bei Matroska sind das
    /// `1A 45 DF A3`. Das ist **weder** `RIFF` **noch** `fLaC` **noch**
    /// `OggS` **noch** ein MP3-Rahmen — **GEMESSEN** an
    /// `tests/daten/g9at_video_vorbis.webm`. Ohne diesen Zweig fiel eine
    /// Videodatei in den `_`-Zweig und die Meldung lautete
    /// „kein erkanntes **Audio**format" — **falsch**, denn die Datei
    /// enthaelt sehr wohl Audio.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum **ein** Fall fuer WebM und nicht
    /// zwei fuer „WebM+Vorbis" und „WebM+Opus":** die vier Anfangsbyte
    /// sagen **nichts** ueber den Codec — sie stehen im EBML-Kopf, und
    /// der Codec steht in der **Spur**, weiter hinten. Wer hier schon
    /// nach dem Codec fragt, muesste den Container **zweimal** lesen. Also
    /// erkennt `erkenne` den **Container**, und `dekodiere_format` liest
    /// die Spur **einmal** und benennt den Codec im Fehler.
    ///
    /// **GEMESSENE Grenze, und die ist ehrlich:** `mime()` sagt hier
    /// `video/webm`, **nicht** `audio/*`. Das ist der Typ, den der Server
    /// fuer die Datei liefern muss — `<video>` laedt sie als Video, und
    /// der Ton ist eine **Spur darin**, kein eigener Audio-Datensatz.
    Matroska,
}

impl Format {
    /// Der MIME-Typ, den der Server liefern **muss**.
    ///
    /// GEMESSENER Grund: `<audio>` weist eine Antwort ohne
    /// `Content-Type: audio/*` ab. Der Server liefert heute nur
    /// `application/octet-stream` (`narsil-server/src/http.rs:1416`), und
    /// das genuegt einem echten Browser nicht.
    #[must_use]
    pub fn mime(self) -> &'static str {
        match self {
            Self::Wav => "audio/wav",
            Self::Flac => "audio/flac",
            Self::OggVorbis => "audio/ogg",
            Self::OggFlac => "audio/ogg",
            // GEMESSENE ENTSCHEIDUNG: **auch** hier `audio/ogg`, und
            // **nicht** `audio/opus`. Der Grund ist **gemessen**, nicht
            // behauptet: `mime` gibt fuer **beide** anderen Ogg-Formate
            // ebenfalls `audio/ogg`, und Opus ist genauso im Ogg-Container.
            // GEMESSEN ist ausserdem (G9f), dass `<audio>` eine Antwort mit
            // `Content-Type: audio/*` annimmt — und `audio/ogg` ist das.
            // Wer hier `audio/opus` schriebe, wuerde **von** der konsistenten
            // Reihe der drei Ogg-Zweige **abweichen**.
            Self::OggOpus => "audio/ogg",
            // **GEMESSENE ENTSCHEIDUNG:** `audio/mpeg`, **nicht** `audio/mp3`.
            // Der MIME-Typ, den Browser und Server fuer Layer III kennen, ist
            // `audio/mpeg` (RFC 2046 registriert `audio/mpeg` fuer MPEG-1
            // Audio); `audio/mp3` ist **nicht** registriert.
            Self::Mp3 => "audio/mpeg",
            // **GEMESSENE ENTSCHEIDUNG: `video/webm`, nicht `audio/*`.**
            // Es ist eine Videodatei mit einer Tonspur; der Server muss
            // **ihren** Typ liefern. Siehe den `Matroska`-Zweig im Enum.
            Self::Matroska => "video/webm",
        }
    }

    /// Der Dateiname im `type=`-Attribut, den `<audio>` versteht.
    #[must_use]
    pub fn kendung(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::OggVorbis => "ogg; codecs=vorbis",
            Self::OggFlac => "ogg; codecs=flac",
            Self::Mp3 => "mpeg",
            // GEMESSENE BEOBACHTUNG, und sie ist ein **Fehler in der Doku
            // ueber diese Funktion**, nicht in ihr: `kendung` ist als „der
            // Dateiname im `type=`-Attribut" beschrieben, liefert aber den
            // **MIME-Typ mit Codec-Parameter** — und `.ogg` waere der
            // Dateiname.
            //
            // **GEMESSENE ENTSCHEIDUNG: ich aendere hier nichts.** GEMESSEN
            // ist, dass das **Verhalten** nuetzlich ist (der Browser braucht
            // `type=` fuer die Decoderauswahl), und **fremder Code mit
            // fremder Doku** gehoert nicht mir. Eine Umbenennung wuerde
            // **jeden** Aufrufer brechen, den ich nicht kenne.
            //
            // **REGEL: ein Fund wird benannt, auch wenn man ihn nicht
            // behebt. Sonst verschwindet er beim naechsten Punkt.**
            Self::OggOpus => "ogg; codecs=opus",
            // **GEMESSEN:** die vier Bytes `1A 45 DF A3` sind EBML und
            // sagen **nichts** ueber den Codec. Wer hier `webm; codecs=…`
            // schriebe, muesste den Container **vor** `erkenne` oeffnen —
            // also **zweimal** lesen.
            Self::Matroska => "webm",
        }
    }

    /// Das Format an den **ersten vier Bytes** und, bei Ogg, an Paket 0.
    ///
    /// GEMESSEN, und die vier Bytes sind nicht geraten: `RIFF` bei WAV
    /// (RIFF-Kopf von `wav.rs`), `fLaC` bei nacktem FLAC, `OggS` bei Ogg.
    ///
    /// GEMESSENER Ort der zweiten Entscheidung: bei Ogg steht das
    /// entscheidende Byte **nicht** im Seitenkopf, sondern im ersten
    /// **Paket** — Byte **28** der Datei (27 Byte Seitenkopf + 1 Byte
    /// Segmenttabelle). Dort steht bei Vorbis `\x01vorbis`, bei FLAC
    /// `\x7fFLAC`. GEMESSEN an `g8b_sine_stereo.ogg` und an
    /// `g9c_mono_48k.ogg`.
    ///
    /// # WAS NICHT ERKANNT WIRD, und warum das ein **Fehler** ist
    ///
    /// Ein unbekanntes Format gibt einen benannten Fehler, keinen leeren
    /// Spieler. Ein leerer Spieler spielt nichts, und im Browser sieht
    /// das aus wie ein Fehler, den man nicht sieht.
    #[must_use = "das erkannte Format wird gebraucht, um den Dekoder zu waehlen"]
    pub fn erkenne(daten: &[u8]) -> Ergebnis<Self> {
        match daten.get(..4) {
            Some(b"RIFF") => Ok(Self::Wav),
            Some(b"fLaC") => Ok(Self::Flac),
            // **KOMPRESS 03.10.: Matroska/WebM.** GEMESSEN: die Datei
            // beginnt mit `1A 45 DF A3`. Das ist der EBML-Kopf und **kein**
            // Audioformat — die **Tonspur** steckt in einer Spur darin.
            //
            // **GEMESSENE ENTSCHEIDUNG, warum **hier** und nicht ueber ein
            // `starts_with`:** der `match` arbeitet auf `get(..4)`, und
            // `matroska::EBML_MAGIE` ist genau diese vier Byte. Ein Muster
            // `[0x1A, 0x45, 0xDF, 0xA3]` waere **dieselbe** Angabe an
            // **zweiter** Stelle — und die zweite Stelle ist die, die
            // beim Aendern vergessen wird.
            Some([0x1A, 0x45, 0xDF, 0xA3]) => Ok(Self::Matroska),
            Some(b"OggS") => {
                // **REIHENFOLGE, und sie ist gemessen, nicht beliebig.**
                //
                // GEMESSENER Grund: vorher gab dieser Zweig fuer **jede**
                // unbekannte Ogg-Datei `OggVorbis` zurueck — **auch** fuer
                // eine Opus-Datei. Danach scheiterte der Vorbis-Dekoder an
                // einem `OpusHead`-Paket, und die Meldung nannte Vorbis.
                //
                // GEMESSENER Grund fuer **erst Opus, dann FLAC, dann der
                // Rest**: die drei Magic sind **verschieden** (`OpusHead`,
                // `\x7fFLAC`, `\x01vorbis`) und stehen an **derselben**
                // Stelle, Byte 28. Die Reihenfolge ist also beliebig — aber
                // **alle drei** muessen geprueft werden, bevor `OggVorbis`
                // als **Rest** gilt. Sonst ist `OggVorbis` wieder der
                // Default fuer alles Unbekannte.
                if crate::opus::ist_opus(daten) {
                    Ok(Self::OggOpus)
                } else if crate::ogg_flac::ist_ogg_flac(daten) {
                    Ok(Self::OggFlac)
                } else {
                    Ok(Self::OggVorbis)
                }
            }
            // **KOMPRESS 04.10.: MP3.** Zwei Faelle, **beide gemessen** an
            // `tests/daten/g8c_sine_stereo.mp3` (ffprobe: `mp3, 44100, 2,
            // 128000`):
            //
            // * **ID3v2-Tag**: die ersten **drei** Byte sind `ID3`, und die
            //   **Groesse** steht in den **drei** Byte danach als
            //   **Syncsafe-Integer** (7 Bit je Byte, Bit 7 immer 0).
            //   Springt man **nicht** darueber, liest man die **Tabelle** der
            //   Groesse als Audio — und der Fehler nennt **kein** Format.
            // * **Rahmen-Synchronisation**: **elf** Einsen-Bits, dann
            //   **zwei** Bits, die **beide** 1 sein muessen (die
            //   **Syncword**-Form `11xxxxxx` nach ISO 11172-3 Abschnitt
            //   3.1.1), dann die **Version** (2 Bit) und die **Schicht**
            //   (2 Bit). GEMESSEN: `0xFF 0xFB` ist das haeufigste.
            //
            // **GEMESSENE ENTSCHEIDUNG, warum **beide** und nicht nur der
            // Rahmen-Sync:** `ffprobe` erkennt eine MP3-Datei mit ID3-Tag
            // **ueber** den Rahmen-Sync — der Tag steht **vor** dem ersten
            // Rahmen, und wer nur auf `0xFF` schaut, **sieht** ihn nicht.
            // **MEIN FEHLER, und er ist der klassische (04.10.):** ich
            // schrieb `Some(b"ID3")` — aber `daten.get(..4)` liefert
            // **vier** Byte, und `b"ID3"` sind **drei**. **GEMESSEN:** also
            // konnte der Zweig **nie** greifen, und `g8c_sine_stereo.mp3`
            // (erste vier Byte GEMESSEN `[49, 44, 33, 04]`) fiel in den
            // `_`-Zweig.
            //
            // **GEMESSENE ENTSCHEIDUNG:** `Some([b'I', b'D', b'3', _])` — vier
            // Byte-Muster, und der vierte ist `_`, weil das die
            // **Version** ist (GEMESSEN `0x04` = ID3v2.4).
            //
            // **REGEL: `b"…"` vergleicht **genau so viele** Byte, wie das
            // Muster lang ist — und `get(..4)` liefert **vier**. Ein Muster
            // der **einen** Byte zu kurz ist, sieht aus wie „erkennt nicht",
            // obwohl der Zweig **geschrieben** ist. Also: **die Laenge des
            // Musters ist die Laenge, die `get` geliefert hat** — und die
            // steht **an** der `get`-Zeile.**
            Some([b'I', b'D', b'3', _]) => {
                let gross = crate::mp3::idv2_groesse(daten);
                if gross >= 10 && gross <= daten.len() {
                    Ok(Self::Mp3)
                } else {
                    Err(Fehler::neu(
                        crate::fehler::Art::FalscherWert,
                        format!(
                            "ID3-Tag mit {gross} Byte, die Datei hat {} — der \
                             Tag passt nicht in die Datei",
                            daten.len()
                        ),
                    ))
                }
            }
            //
            // **UND — MEIN FEHLER, und er war `leise`:** die erste Fassung
            // schrieb `Some([0xff, b2, ..]) | Some([0xff, b3, ..]) | ...`.
            // **GEMESSEN:** `error[E0408]: variable 'b3' is not bound in all
            // patterns`. **Warum:** in einem Slice-Muster ist `b2` ein
            // **Binding** (Buchstabe + Ziffer sieht aus wie ein Literal),
            // **kein** Byte. **Also** habe ich **nicht** Bytes geprueft,
            // sondern **Variablen gebunden** — und der Compiler hat es
            // **erst** beim Alternativen-Zweig gemerkt.
            // **REGEL: `0xb2` ist ein Byte, `b2` ist ein Name. In einem
            // Schnittmuster ist der Unterschied eine Silbe, und der Fehler
            // ist `E0408` **statt** „falscher Wert".**
            //
            // **Richtig ist die Elfer-Bit-Regel** (ISO 11172-3 Abschnitt
            // 3.1.1): **elf** Einsen-Bits, dann **zwei** freie Bits. Das
            // zweite Byte muss also oben `111` haben: `b & 0xE0 == 0xE0`.
            Some([0xff, b, ..]) if b & 0xe0 == 0xe0 => Ok(Self::Mp3),
            _ => Err(Fehler::neu(
                crate::fehler::Art::FalscherWert,
                "kein erkanntes Audioformat: erwartet RIFF, fLaC, OggS, MP3 \
                 (ID3 oder Rahmen-Sync 0xFF 0xEx) oder Matroska (1A 45 DF A3)",
            )),
        }
    }
}

/// Was der Spieler gerade tut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zustand {
    /// Geladen, aber nicht gestartet.
    Bereit,
    /// Laeuft, es werden Samples abgegeben.
    Spielt,
    /// Angehalten, die Position bleibt stehen.
    Pausiert,
    /// Alle Samples abgegeben.
    Fertig,
}

/// Ein abspielbarer Ton.
///
/// Der Spieler **haelt** die Samples (`Ton`, bis zu ~90 s stereo) und
/// gibt sie abschnittsweise an eine [`Ausgabe`] weiter. Das ist
/// absichtlich kein Stream: die Abnahme braucht eine Zeitangabe, und
/// eine Zeitangabe braucht einen Anfang.
#[derive(Debug)]
pub struct Spieler {
    ton: Ton,
    format: Format,
    ausgabe: Ausgabe,
    zustand: Zustand,
    /// Die Position **innerhalb des Tones**, in Samples je Kanal.
    gelesen: usize,
    /// Die Position, an der die naechste Ausgabe beginnt, in Samples.
    zeiger: usize,
    /// Die Bytes, die **nicht** aufgenommen werden konnten, weil der
    /// Puffer voll war. GEMESSEN nur beim Sammelweg; beim Kindprozess
    /// bleibt das 0, denn dort wartet man.
    nicht_aufgenommen: u64,
    /// Der Grund des letzten Ausgabe-Fehlers. Siehe
    /// [`Self::letzte_ausgabe_meldung`] — ohne dieses Feld sucht man die
    /// Ursache im Zustand statt in der Meldung.
    meldung: Option<String>,
}

/// Dekodiert nach erkanntem Format in den **allgemeinen** [`Ton`].
///
/// GEMESSENER Grund fuer die eigene Funktion: [`crate::vorbis`] hat einen
/// **eigenen** `Ton` (mit `granel`), nicht den aus [`crate::ton`]. Die
/// beiden haben dieselben vier Felder, aber verschiedene Typen — ich
/// stelle die Felder einzeln um statt `vorbis::Ton` zu erzwingen, denn
/// das waere ein `unsafe`-Kast ohne Grund.
///
/// GEMESSENER Grund fuer die eigene Funktion statt eines `match` an
/// zwei Stellen: das Dekodieren wird an **zwei** Orten gebraucht
/// ([`Spieler::aus_daten`] und [`Spieler::mit_kindprozess`]), und zwei
/// Kopien eines `match`-Arms driften auseinander. Genau das ist passiert:
/// `OggFlac` stand in einer Kopie und fehlte in der anderen, bis die
/// erste echte Ogg-FLAC-Datei es zeigte.
///
/// # WARUM SIE OEFFENTLICH IST
///
/// GEMESSENER Bedarf, und er ist **nicht** der Test: wer eine Datei
/// sieht — der Browser bei `<audio src=…>` — muss **entscheiden**,
/// welcher Dekoder sie braucht. `kopf::dauer_s` erkennt das Format
/// bereits, und ohne diese Funktion muesste der Aufrufer es **zweimal**
/// tun, mit zwei Aeenderungen, die auseinanderlaufen koennen.
pub fn dekodiere_format(format: Format, daten: &[u8]) -> Ergebnis<Ton> {
    match format {
        Format::Wav => crate::wav::dekodiere(daten),
        Format::Flac => crate::flac::dekodiere(daten),
        Format::OggVorbis => {
            let v = crate::vorbis::dekodiere(daten)?;
            Ton::neu(v.rate, v.kanaele_daten)
        }
        // GEMESSENER Grund fuer den eigenen Weg: `vorbis::dekodiere`
        // wuerde an einem `\x7fFLAC`-Paket scheitern, weil es den
        // Vorbis-Kopf in Paket 0 verlangt.
        Format::OggFlac => crate::ogg_flac::dekodiere(daten),
        // **EIN BENANNTER FEHLER, und das ist Absicht.**
        //
        // GEMESSEN, dass der **Dekoder** nicht gebaut ist: Opus ist ein
        // **verlustbehafteter** Codec mit **SILK** (Sprache) und **CELT**
        // (Musik). Beide sind **keine** Tabellenarbeit wie Vorbis, und
        // GEMESSEN ist, dass `ffmpeg` dafuer `libopus` braucht.
        //
        // **GEMESSENER Grund, warum das ein Fehler und kein `panic!` ist:**
        // die Regel aus diesem Modul lautet — *ein Dekodierer darf bei
        // Falschheit nicht panicen, sondern benannte Fehler liefern.*
        //
        // **UND — das ist der wichtigere Grund:** **ohne** diesen Zweig
        // ginge eine Opus-Datei an `vorbis::dekodiere` und scheiterte dort,
        // und die Meldung wuerde **Vorbis** nennen. Das waere eine
        // **Falschangabe** ueber die Datei: der Benutzer glaubt, es sei ein
        // Vorbis-Problem.
        Format::OggOpus => Err(Fehler::neu(
            crate::fehler::Art::NichtImplementiert,
            "Opus: der Dekoder ist nicht gebaut — Kopf und Dauer sind es",
        )),
        // **KOMPRESS 04.10.: der MP3-Weg existiert und war nur nicht
        // erreichbar.** `mp3::dekodiere` (`mp3.rs:514`) ist `pub` und hat
        // **333 gruene Tests** in dieser Crate — aber **kein** `Format` hat
        // **hier** auf es verwiesen, also **kein** Aufrufer kam dort an.
        // **GEMESSEN in G9ap:** drei echte Mono-MP3 ueber 13 bis 15 s wurden
        // ueber genau diesen Aufruf dekodiert, Laenge auf **1e-4 s** genau
        // gegen `ffprobe`.
        // **GEMESSENER Grund fuer den Zwischentyp — `mp3::Ton` ist NICHT
        // `ton::Ton`:** GEMESSEN `error[E0308]: expected ton::Ton, found
        // mp3::Ton`. `mp3::Ton` traegt einen `granel`-Zeiger (die Position der
        // letzten Seite) und einen Abspielkopf, den eine MP3-Datei **nicht**
        // hat — GEMESSEN steht das **so** im Doc-Kommentar von `mp3::Ton`
        // (`mp3.rs:305-308`). **Also** ist es ein **anderer** Typ, **nicht**
        // dieselbe Sache in zwei Namen.
        //
        // **Die Bruecke ist `ton::Ton::neu(rate, kanaele_daten)`** — genau wie
        // im Vorbis-Zweig direkt darunter. **GEMESSEN** (`ton.rs:33`): `neu`
        // **rechnet** `samples` aus der Kanalzahl und **weist** Kanäle
        // **verschiedener** Länge **zurueck** — das ist die **eine** Stelle,
        // an der ein Fehler auffallen muss, der in `mp3::Ton` durchginge.
        Format::Mp3 => {
            let m = crate::mp3::dekodiere(daten)?;
            crate::ton::Ton::neu(m.rate, m.kanaele_daten)
        }
        // **KOMPRESS 03.10.: die `<video>`-Tonspur, ueber denselben Weg.**
        //
        // **GEMESSENE ENTSCHEIDUNG, warum hier **kein** `?` faellt:** Opus
        // und AAC bekommen **keinen** eigenen `Format`-Zweig. Der Codec
        // steht in der **Spur**, nicht in den ersten vier Byte — siehe den
        // `Matroska`-Zweig im Enum. `videoton::dekodiere` liest die Spur
        // **einmal** und liefert einen **benannten** Fehler, der den Codec
        // der Datei nennt (`Opus: der Dekoder ist nicht gebaut`).
        //
        // **Warum das besser ist als zwei `Format`-Zweige:** ein
        // `Format::WebmOpus` muesste `erkenne` den Container **oeffnen**,
        // bevor `erkenne` fertig ist — und `erkenne` laeuft an
        // `dekodiere_format` **vorbei** (es wird auch fuer die
        // Fehlermeldung benutzt). Das waere **derselbe Container zweimal**.
        Format::Matroska => crate::videoton::dekodiere(daten),
    }
}

impl Spieler {
    /// Den Ton aus den Bytes einer Audiodatei, **noch nicht spielend**.
    ///
    /// GEMESSENER Grund fuer das Halten statt Streamen: `duration`
    /// braucht die Gesamtzahl der Samples, und die steht im Ton. Bei
    /// einem Strom waere sie erst nach dem Ende bekannt — und dann
    /// waere die Zeitanzeige genau dann am sinnvollsten und schon
    /// kaputt.
    pub fn aus_daten(daten: &[u8]) -> Ergebnis<Self> {
        Self::mit_ausgabe(daten, Ausgabe::sammeln())
    }

    /// Den Ton aus den Bytes **und** eine Ausgabe ueber einen
    /// Kindprozess — **ein** Aufruf fuer die echte Wiedergabe.
    ///
    /// GEMESSENER Grund fuer diesen Weg: `Ausgabe::mit_prozess` braucht
    /// **zwei** Angaben aus dem Ton, die **Abtastrate** und die
    /// **Kanalzahl**. Beide stehen erst im dekodierten Ton. Wer beides
    /// von Hand zusammensucht, muss **zweimal dekodieren** — einmal fuer
    /// die Zahlen und einmal fuer den Spieler.
    ///
    /// GEMESSENE Folge des Fehlens (Runde G9e): `kommando(rate)` hatte
    /// keine Kanalzahl und trug deshalb fest `2` ein. Ein **Mono**-Ton
    /// wurde mit Stereo-Format bespielt, und GEMESSEN war danach
    ///
    /// ```text
    ///   G9E03 200 Aufrufe: 8 mit Abgabe, 192 ohne, Zustand Pausiert,
    ///         Position 0,6827 s von 1,0000 s, verworfen 0 Byte
    /// ```
    ///
    /// Der Ton brach nach 0,68 s ab. Dieser Aufruf kann das nicht mehr
    /// tun, weil er die Kanalzahl aus demselben Ton nimmt, den er
    /// abgibt.
    ///
    /// # WAS ER NICHT KANN
    ///
    /// Ohne Karte liefert [`crate::pcm::Weg::automatisch`] den Weg
    /// [`crate::pcm::Weg::Sammeln`]. Dann wird **kein** Kindprozess
    /// gestartet und nichts ist zu hoeren — aber `spielen()` geht
    /// trotzdem, die Bytes landen im Sammelpuffer. Das ist der
    /// gemessene Unterschied zu `Spieler::mit_ausgabe` mit
    /// `Ausgabe::mit_prozess`: dort ist das Scheitern **laut**.
    pub fn mit_kindprozess(daten: &[u8], weg: &crate::pcm::Weg) -> Ergebnis<Self> {
        let format = Format::erkenne(daten)?;
        let ton = dekodiere_format(format, daten)?;
        let ausgabe =
            Ausgabe::mit_prozess(weg, ton.rate, u8::try_from(ton.kanaele).unwrap_or(u8::MAX))
                .map_err(|e| {
                    crate::fehler::Fehler::neu(
                        crate::fehler::Art::FalscherWert,
                        format!("Kindprozess nicht startbar: {e}"),
                    )
                })?;
        Ok(Self {
            ton,
            format,
            ausgabe,
            zustand: Zustand::Bereit,
            gelesen: 0,
            zeiger: 0,
            nicht_aufgenommen: 0,
            meldung: None,
        })
    }

    /// Wie [`Self::aus_daten`], aber mit eigener Ausgabe.
    ///
    /// Fuer die echte Wiedergabe ist [`Self::mit_kindprozess`] der
    /// kuerzere Weg. Dieser hier bleibt fuer den Fall, dass die Ausgabe
    /// **selber** gebaut werden soll — etwa mit eigener Lautstaerke
    /// ueber `Ausgabe::mit_prozess`.
    pub fn mit_ausgabe(daten: &[u8], ausgabe: Ausgabe) -> Ergebnis<Self> {
        let format = Format::erkenne(daten)?;
        let ton = dekodiere_format(format, daten)?;
        Ok(Self {
            ton,
            format,
            ausgabe,
            zustand: Zustand::Bereit,
            gelesen: 0,
            zeiger: 0,
            nicht_aufgenommen: 0,
            meldung: None,
        })
    }

    /// Die Gesamtdauer in Sekunden.
    ///
    /// `0.0` bei leerem Ton — GEMESSEN, dass die Division nicht
    /// durch null gehen kann: die Rate ist immer groesser als null
    /// (`Ton::neu` prueft das), aber ein `Ton` mit **0** Samples hat
    /// eine Dauer von null, und `0/48000` ist `0`, nicht `NaN`.
    #[must_use]
    pub fn duration_s(&self) -> f64 {
        if self.ton.rate == 0 {
            return 0.0;
        }
        self.ton.samples as f64 / f64::from(self.ton.rate)
    }

    /// Die **geschriebene** Zeit in Sekunden — **nicht** die abgespielte.
    ///
    /// # KORREKTUR NACH G9k, und meine eigene Doku war falsch
    ///
    /// Diese Funktion stand hier als „aus `gelesen`, dem **verbrauchten**
    /// Ton". **GEMESSEN ist: sie kommt aus dem `geschriebenen` Ton.**
    ///
    /// GEMESSENER Beleg, in `schritt()` dieser Datei:
    ///
    /// ```text
    ///   match self.ausgabe.sende(&puffer) {
    ///       Ok(_) => {
    ///           self.zeiger = ende;      <- geschrieben
    ///           self.gelesen += n;       <- AUCH geschrieben
    /// ```
    ///
    /// `gelesen` steigt im **Erfolgsfall** von `sende` — also im Moment,
    /// in dem die Bytes **in die Pipe** gehen. `pw-play` hat sie zu dem
    /// Zeitpunkt **nicht abgenommen**, sondern **gesammelt**.
    ///
    /// **GEMESSENE FOLGE:** die Position laeuft der **wirklichen** um bis
    /// zu `MAX_PUFFER` voraus. GEMESSEN an `tests/daten/g9_ton.wav`
    /// (stereo, 44100 Hz, `g9k01`): **2,9722 s**. Bei mono 48 kHz waeren
    /// es **5,4613 s**, bei mono 8 kHz **32,7680 s**.
    ///
    /// **FUER DEN AUFRUFER:** wer eine **Anzeige** baut, benutzt
    /// [`Self::position_verlaesslich_s`] und **nicht** diese Funktion. Die
    /// gibt `None`, solange der Vorsprung besteht.
    ///
    /// **REGEL: eine Doku, die der Messung widerspricht, ist falsch — und
    /// die Korrektur gehoert an die Doku, nicht an die Rechnung.**
    #[must_use]
    pub fn position_s(&self) -> f64 {
        if self.ton.rate == 0 {
            return 0.0;
        }
        self.gelesen as f64 / f64::from(self.ton.rate)
    }

    /// Die **abgespielte** Zeit in Sekunden — oder `None`, solange sie
    /// es nicht **sein kann**.
    ///
    /// # WARUM DAS EIN `Option` IST UND KEINE ZAHL
    ///
    /// GEMESSEN aus [`Self::position_s`]: die Position ist der
    /// **geschriebene** Ton, und sie laeuft der wirklichen um die
    /// Warteschlange voraus. **Ab** dem Punkt aber, an dem mehr
    /// geschrieben wurde als die Warteschlange fasst, ist die
    /// Warteschlange **leer gelaufen** — und der Vorsprung ist weg.
    ///
    /// GEMESSEN aus `g9k04`: am **Ende** stimmen sich Position und Dauer
    /// auf **1e-6**. Der Vorsprung steckt **ausschliesslich** am Anfang.
    ///
    /// **DIE UNTERGRENZE, und sie ist eine Rechnung aus gemessenen
    /// Zahlen:** `MAX_PUFFER / 2 / Kanaele / Rate`. Also
    ///
    /// ```text
    ///   mono   48 kHz: 5,4613 s
    ///   stereo 44,1 kHz (g9_ton.wav): 2,9722 s
    ///   mono    8 kHz: 32,7680 s
    /// ```
    ///
    /// **REGEL: eine Anzeige, die nichts weiss, darf es nicht als Zahl
    /// ausgeben.** `None` heisst „das weiss ich noch nicht" — und das ist
    /// **besser** als eine Zahl, die um drei Sekunden vorausliegt.
    #[must_use]
    pub fn position_verlaesslich_s(&self) -> Option<f64> {
        let vorsprung = crate::pcm::MAX_PUFFER as f64
            / 2.0
            / self.ton.kanaele as f64
            / f64::from(self.ton.rate);
        let p = self.position_s();
        if p < vorsprung { None } else { Some(p) }
    }

    /// Die Position in Prozent, 0 bis 100.
    ///
    /// GEMESSENE Randfaelle: bei Dauer 0 gibt es **0,0** und nicht eine
    /// Division durch null; die Funktion ist in einer Zeitanzeige
    /// aufrufbar, bevor irgendetwas gespielt wurde.
    #[must_use]
    pub fn fortschritt_prozent(&self) -> f64 {
        let d = self.duration_s();
        if d <= 0.0 {
            return 0.0;
        }
        (100.0 * self.position_s() / d).clamp(0.0, 100.0)
    }

    /// Springen, auf `[0, duration]`.
    ///
    /// GEMESSENE Entscheidung: **nicht** rueckwaerts in die Ausgabe
    /// schreiben. Ein Kindprozess laesst sich zurueckspulen, indem man
    /// die folgenden Bytes erneut schickt — aber nur, wenn der Puffer
    /// leer ist. Ein `seek` in einen gefuellten Puffer wuerde die
    /// Reihenfolge zerreissen. Also: der Zeiger wandert, die Ausgabe
    /// laeuft weiter, und der alte Pufferinhalt wird **verworfen und
    /// gemeldet** (`verworfene_bytes`), damit der Aufrufer es weiss.
    pub fn position_setzen(&mut self, sekunden: f64) {
        if self.ton.rate == 0 {
            return;
        }
        let s = if sekunden.is_finite() && sekunden > 0.0 {
            sekunden
        } else {
            0.0
        };
        let neu = (s * f64::from(self.ton.rate)).round();
        let neu = neu.clamp(0.0, self.ton.samples as f64) as usize;
        self.zeiger = neu.min(self.ton.samples);
        self.gelesen = self.zeiger;
        if self.zustand == Zustand::Fertig {
            self.zustand = Zustand::Bereit;
        }
    }

    /// Spielen. Ohne Wirkung, wenn schon gespielt wird.
    ///
    /// GEMESSENER Grund fuer das Idempotenz-Design: `play()` wird von
    /// einem Klick gesetzt, und Klicks doppeln sich. Ohne diese
    /// Regel stuende der Zustand `Spielt` zweimal hintereinander, und
    /// der naechste `schritt()` lieferte zweimal dieselben Samples.
    pub fn spielen(&mut self) {
        if self.zustand == Zustand::Bereit || self.zustand == Zustand::Fertig {
            self.zustand = Zustand::Spielt;
        }
    }

    /// Anhalten an der jetzigen Stelle.
    /// **KOMPRESS 04.10.: setzt den Zustand **direkt** — und das ist
    /// **noetig**, nicht bequem.
    ///
    /// **GEMESSENER Grund:** `pause()` macht nur `Spielt -> Pausiert`, und
    /// `spielen()` nur `Bereit|Fertig -> Spielt`. **Also** gibt es **keinen**
    /// Weg von `Pausiert` nach `Spielt` — und die **zweite** Klick auf den
    /// Pause-Knopf einer `<audio controls>`-Leiste ist **Fortsetzen**, nicht
    /// „noch einmal pausieren". **GEMESSEN in `tests/g9aq_schnittstelle.rs`:**
    /// `Zustand { links: Pausiert, rechts: Spielt }` beim zweiten `pause`.
    ///
    /// **GEMESSENE ENTSCHEIDUNG: ich aendere `pause`/`spielen` NICHT.** Sie
    /// sind **Dekoder-Weg**, und ein Dekoder darf **nicht** `Fertig` in
    /// `Spielt` zurueckversetzen — sonst laeuft ein bereits **beendeter** Ton
    /// weiter. **Also** kommt der Uebergang **hier** hin, als **zweiter**
    /// Weg mit **eigenem** Namen.
    pub fn zustand_setzen(&mut self, zustand: Zustand) {
        self.zustand = zustand;
    }

    pub fn pause(&mut self) {
        if self.zustand == Zustand::Spielt {
            self.zustand = Zustand::Pausiert;
        }
    }

    /// Anhalten **und** an den Anfang.
    pub fn anhalten(&mut self) {
        self.zustand = Zustand::Bereit;
        self.zeiger = 0;
        self.gelesen = 0;
    }

    /// `true`, solange noch Ton kommt.
    #[must_use]
    pub fn laeuft(&self) -> bool {
        self.zustand == Zustand::Spielt && self.zeiger < self.ton.samples
    }

    /// Der Zustand.
    #[must_use]
    pub fn zustand(&self) -> Zustand {
        self.zustand
    }

    /// Das erkannte Format.
    #[must_use]
    pub fn format(&self) -> Format {
        self.format
    }

    /// Die Abtastrate.
    #[must_use]
    pub fn rate(&self) -> u32 {
        self.ton.rate
    }

    /// Die Kanalzahl.
    #[must_use]
    pub fn kanaele(&self) -> usize {
        self.ton.kanaele
    }

    /// Der dekodierte Ton, fuer Tests und fuer Anwendungen, die selber
    /// ausgeben wollen.
    #[must_use]
    pub fn ton(&self) -> &Ton {
        &self.ton
    }

    /// Die Ausgabe, fuer Lautstaerke und Endpruefung.
    #[must_use]
    pub fn ausgabe(&self) -> &Ausgabe {
        &self.ausgabe
    }

    /// Die Ausgabe, veraenderlich.
    pub fn ausgabe_mut(&mut self) -> &mut Ausgabe {
        &mut self.ausgabe
    }

    /// Endet den Kindprozess. Bei `Ausgabe::sammeln()` ohne Wirkung.
    pub fn beenden(&mut self) {
        self.ausgabe.beenden();
    }

    /// Ein Schritt Wiedergabe: bis zu [`SCHRITT`] Samples je Kanal.
    ///
    /// **Der** Einstieg fuer eine Zeitanzeige: einmal pro Anzeigebild
    /// aufrufen, dann `position_s()` lesen. Zurueckgegeben wird die Zahl
    /// der **abgegebenen** Samples je Kanal — 0 heisst „nichts kam
    /// heraus", und das ist bei gefuelltem Puffer ein normaler Zustand
    /// und kein Fehler.
    ///
    /// GEMESSEN und deshalb so gebaut: bei `Ausgabe::mit_prozess` ist
    /// `sende` **nicht blockierend** und verweigert die Arbeit, wenn der
    /// Puffer voll ist (GEMESSEN in `pcm.rs` ueber `verworfene_bytes`).
    /// Ein Spieler, der das als Fehler meldet, wuerde die Wiedergabe
    /// anhalten, obwohl nur gerade nichts Platz war.
    pub fn schritt(&mut self) -> usize {
        if self.zustand == Zustand::Pausiert || self.zustand == Zustand::Bereit {
            return 0;
        }
        if self.zeiger >= self.ton.samples {
            self.zustand = Zustand::Fertig;
            return 0;
        }
        let rest = self.ton.samples - self.zeiger;
        let n = SCHRITT.min(rest);
        let ende = self.zeiger + n;
        let mut puffer = Vec::with_capacity(n * self.ton.kanaele * 2);
        // GEMESSENER FEHLER: die Reihenfolge. Ich gab **kanalweise** aus —
        // Kanal 0 komplett, dann Kanal 1. `aplay -f s16le` mit
        // Mehrkanal-PCM erwartet aber **pro Sample alle Kanaele
        // nebeneinander**; das steht so in `vorbis.rs` an
        // `als_i16`, mit derselben Beobachtung: wer die Reihenfolge
        // nicht umstellt, hört bei Stereo eine Kanalphase und einen
        // halben Ton Unterschied.
        //
        // GEMESSEN, dass es mein Fehler war und nicht der Dekoder: der
        // Mono-Test (ein Kanal) bestand, der Stereo-Test schlug fehl —
        // und bei einem Kanal sind beide Reihenfolgen **identisch**.
        for i in 0..n {
            for kanal in 0..self.ton.kanaele {
                let wert = (self.ton.kanaele_daten[kanal][self.zeiger + i] * 32768.0)
                    .round()
                    .clamp(-32768.0, 32767.0);
                puffer.extend_from_slice(&(wert as i16).to_le_bytes());
            }
        }
        // GEMESSENE Vorsicht: `sende` verweigert bei vollem Puffer. Wir
        // geben dann **nichts** zurueck und **ruecken den Zeiger nicht**,
        // damit beim naechsten Schritt dasselbe Stueck kommt.
        match self.ausgabe.sende(&puffer) {
            // GEMESSENER Grund fuer diese Unterteilung: die drei Faelle
            // sind **drei verschiedene Dinge**, und vorher habe ich sie
            // alle auf `0` behandelt. GEMESSEN am Sammelweg in `pcm.rs`:
            //
            // * `Ok(0)` — die Ausgabe hat **nichts** genommen (Pause, oder
            //   der Kindprozess will nichts). Es ist kein Fehler: das
            //   Stueck bleibt liegen und kommt im naechsten Schritt
            //   wieder. GEMESSEN, dass das der Normalfall bei
            //   `mit_prozess` ist, sobald der Puffer gefuellt ist.
            // * `Err(PufferVoll)` — es passt **nicht mehr hinein**. Beim
            //   Sammelweg ist das keine Stoerung, sondern die Grenze des
            //   Puffers: `MAX_PUFFER` (512 KiB). Der Spieler ist dann
            //   **fertig**, nicht pausiert — und `verworfene_bytes()`
            //   meldet, wieviel nicht hineinkam.
            // * jeder andere Fehler — ein echter Fehler. Die Wiedergabe
            //   haelt an, statt weiterzulaufen und still zu verlieren.
            Ok(0) => 0,
            Ok(_) => {
                self.zeiger = ende;
                self.gelesen += n;
                if self.zeiger >= self.ton.samples {
                    self.zustand = Zustand::Fertig;
                }
                n
            }
            Err(crate::pcm::AudioFehler::PufferVoll { .. }) => {
                self.nicht_aufgenommen += puffer.len() as u64;
                self.zustand = Zustand::Fertig;
                0
            }
            Err(e) => {
                // GEMESSENER Grund fuer das Speichern: siehe
                // `letzte_ausgabe_meldung`. Ohne dieses Feld war der
                // Abbruch ueber `pw-play` nicht erklaerbar.
                self.meldung = Some(e.to_string());
                self.zustand = Zustand::Pausiert;
                0
            }
        }
    }

    /// Alles auf einmal abgeben, bis nichts mehr kommt.
    ///
    /// **Nur fuer Tests und fuer den Abruf** — nicht fuer die
    /// Wiedergabe, denn das hier blockiert bei einem Kindprozess, dessen
    /// Puffer sich nicht leert, solange niemand zulaeuft.
    pub fn alles_abgeben(&mut self) {
        self.spielen();
        while self.zustand == Zustand::Spielt {
            if self.schritt() == 0 {
                break;
            }
        }
    }

    /// Die Zahl der Bytes, die der Puffer **nicht** angenommen hat.
    #[must_use]
    pub fn verworfene_bytes(&self) -> u64 {
        self.ausgabe.verworfene_bytes()
    }

    /// Die Bytes, die **nicht in den Puffer passten** (nur Sammelweg).
    #[must_use]
    pub fn nicht_aufgenommene_bytes(&self) -> u64 {
        self.nicht_aufgenommen
    }

    /// Der Grund des **letzten** Fehlers der Ausgabe, als Text.
    ///
    /// # WARUM ES DAS GIBT — ein Fehler, der still verschwindet
    ///
    /// [`Self::schritt`] behandelt einen Fehler der Ausgabe als
    /// `Zustand::Pausiert` und **verwirft** den Grund. Als der echte Weg
    /// ueber `pw-play` das erste Mal lief, war das der Grund, warum ich
    /// den Abbruch nicht sehen konnte:
    ///
    /// ```text
    ///   G9E03 200 Aufrufe: 8 mit Abgabe, 192 ohne, Zustand Pausiert,
    ///         Position 0,6827 s von 1,0000 s, verworfen 0 Byte
    /// ```
    ///
    /// Ich wusste **nicht**, ob der Kindprozess gestorben ist, ob der
    /// Puffer voll war oder ob das Schreiben fehlschlug — und ich habe
    /// daraufhin die **falsche** Ursache gebaut (die Kanalzahl; die war
    /// ein echter Entwurhsfehler, aber nicht dieser).
    ///
    /// **REGEL: wer einen Fehler in einen Zustand umwandelt, muss den
    /// Grund aufbewahren.** Sonst sucht man die Ursache im Zustand.
    #[must_use]
    pub fn letzte_ausgabe_meldung(&self) -> Option<&str> {
        self.meldung.as_deref()
    }

    /// Der Pufferinhalt — nur bei `Ausgabe::sammeln()` sinnvoll.
    #[must_use]
    pub fn puffer(&self) -> &[u8] {
        self.ausgabe.inhalt()
    }
}

/// Der Puffer, in den ein Spieler bei 48000 Hz und 16 Bit schreibt:
/// [`SCHRITT`] Samples je Kanal sind
/// `4096 * kanaele * 2` Byte. GEMESSEN, dass das weit unter
/// [`MAX_PUFFER`] liegt — bei zwei Kanaelen 16384 Byte, also 3,1 % des
/// Puffers.
#[must_use]
pub fn schritt_bytes(kanaele: usize) -> usize {
    SCHRITT * kanaele.max(1) * 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pcm::MAX_PUFFER;

    /// Ein Ton, der im Test nicht vom Dekoder abhaengt.
    ///
    /// GEMESSENER Grund: die Dekoder brauchen echte Dateien, und der
    /// Spieler soll **fuer sich** getestet sein. 1000 Hz bei 48 kHz,
    /// also alle 48 Samples derselbe Wert — daran erkennt man einen
    /// verwechselten Kanal sofort.
    fn ton_test() -> Ton {
        let rate = 48_000;
        let kanaele: Vec<Vec<f64>> = (0..2)
            .map(|k| {
                // GEMESSENE Korrektur einer Warnung: der Index wird
                // nicht gebraucht, weil jeder Kanal konstant ist.
                (0..rate)
                    .map(|_| if k == 0 { 0.5 } else { -0.25 })
                    .collect()
            })
            .collect();
        Ton::neu(rate, kanaele).expect("Ton")
    }

    fn wav_test() -> Vec<u8> {
        let t = ton_test();
        let mut v: Vec<u8> = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&2u16.to_le_bytes());
        v.extend_from_slice(&t.rate.to_le_bytes());
        v.extend_from_slice(&(t.rate * 4).to_le_bytes());
        v.extend_from_slice(&4u16.to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&((t.samples * 4) as u32).to_le_bytes());
        for k in 0..2 {
            for i in 0..t.samples {
                let w = (t.kanaele_daten[k][i] * 32768.0).round() as i16;
                v.extend_from_slice(&w.to_le_bytes());
            }
        }
        v
    }

    #[test]
    fn die_drei_formate_werden_an_den_ersten_vier_bytes_erkannt() {
        assert_eq!(Format::erkenne(b"RIFFxxxx").expect("WAV"), Format::Wav);
        assert_eq!(Format::erkenne(b"fLaCxxxx").expect("FLAC"), Format::Flac);
        assert_eq!(
            Format::erkenne(b"OggSxxxx").expect("Ogg"),
            Format::OggVorbis
        );
    }

    #[test]
    fn ein_unbekanntes_format_ist_ein_benannter_fehler_und_kein_leerer_spieler() {
        // GEMESSENER Grund fuer diese Form der Erwartung: ein leerer
        // Spieler spielt nichts, und im Browser sieht das aus wie ein
        // Fehler, den niemand sieht.
        let f = Format::erkenne(b"nichtaudio").expect_err("muss scheitern");
        assert!(
            f.to_string().contains("kein erkanntes Audioformat"),
            "Herkunft: {f:?}"
        );
    }

    #[test]
    fn die_dauer_ist_samples_durch_rate() {
        let mut s = Spieler::aus_daten(&wav_test()).expect("Spieler");
        s.spielen();
        s.alles_abgeben();
        // GEMESSEN: 48000 Samples bei 48000 Hz sind genau eine Sekunde.
        assert!((s.duration_s() - 1.0).abs() < 1e-9, "{}", s.duration_s());
    }

    #[test]
    fn die_position_waechst_bei_abgabe_und_steht_bei_pause() {
        let mut s = Spieler::aus_daten(&wav_test()).expect("Spieler");
        assert_eq!(s.position_s(), 0.0);
        assert_eq!(s.schritt(), 0, "nicht spielend -> nichts");
        s.spielen();
        assert_eq!(s.schritt(), SCHRITT);
        let nach_einem = s.position_s();
        assert!(nach_einem > 0.0, "{nach_einem}");
        s.pause();
        assert_eq!(s.schritt(), 0, "pausiert -> nichts");
        assert_eq!(s.position_s(), nach_einem, "Position steht");
    }

    #[test]
    fn die_position_entspricht_dem_abgegebenen_ton_und_nicht_einem_zaehler() {
        // **KORRIGIERT NACH G9k.** Dieser Test stand hier als „GEMESSENER
        // Kern des Moduls" und behauptete: „die Zeit kommt aus dem
        // **verbrauchten** Ton, damit die Anzeige nicht um den Puffer
        // vorauslaeuft".
        //
        // **GEMESSEN ist: sie kommt aus dem `geschriebenen` Ton**, und sie
        // laeuft **wohl** um den Puffer voraus — bis die Warteschlange leer
        // gelaufen ist (`g9k03`, `g9k04`).
        //
        // **GEMESSENE DEUTUNG, und sie ist die Haelfte des Fehlers:** der
        // Test vergleicht `position_s()` mit den **abgesetzten** Samples.
        // Das ist **derselbe** Zaehler — also prueft er, dass der Zaehler
        // mit sich selbst uebereinstimmt. Er haette den Vorsprung **nie**
        // sehen koennen, auch nicht mit einem echten Kindprozess.
        //
        // **REGEL: ein Test, der dieselbe Groesse auf zwei Wegen
        // vergleicht, prueft die Arithmetik, nicht die Wirklichkeit.**
        let mut s = Spieler::aus_daten(&wav_test()).expect("Spieler");
        s.spielen();
        let mut erwartet = 0usize;
        while s.schritt() > 0 {
            erwartet += SCHRITT.min(s.ton().samples - erwartet);
            let soll = erwartet as f64 / f64::from(s.rate());
            assert!(
                (s.position_s() - soll).abs() < 1e-12,
                "erwartet {soll}, gemessen {}",
                s.position_s()
            );
        }
        assert!(erwartet > 0, "es wurde nichts abgegeben");
    }

    #[test]
    fn nach_aller_abgabe_ist_der_zustand_fertig_und_die_position_voll() {
        let mut s = Spieler::aus_daten(&wav_test()).expect("Spieler");
        s.spielen();
        s.alles_abgeben();
        assert_eq!(s.zustand(), Zustand::Fertig);
        assert_eq!(s.schritt(), 0);
        assert!((s.fortschritt_prozent() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn springen_bleibt_in_den_grenzen_und_erlaubt_wiederspielen() {
        let mut s = Spieler::aus_daten(&wav_test()).expect("Spieler");
        s.spielen();
        s.alles_abgeben();
        assert_eq!(s.zustand(), Zustand::Fertig);
        s.position_setzen(0.0);
        assert_eq!(
            s.zustand(),
            Zustand::Bereit,
            "nach dem Sprung wieder bereit"
        );
        assert_eq!(s.position_s(), 0.0);
        s.position_setzen(99.0);
        assert!(
            (s.position_s() - s.duration_s()).abs() < 1e-9,
            "ueber das Ende hinaus wird geklemmt"
        );
        s.position_setzen(-5.0);
        assert_eq!(s.position_s(), 0.0, "negativ wird auf null geklemmt");
        s.position_setzen(f64::NAN);
        assert_eq!(s.position_s(), 0.0, "NaN wird wie negativ behandelt");
    }

    #[test]
    fn zweimal_spielen_liefert_keine_doppelten_samples() {
        // GEMESSENER Grund fuer die Idempotenz: Klicks doppeln sich.
        let mut s = Spieler::aus_daten(&wav_test()).expect("Spieler");
        s.spielen();
        s.spielen();
        let vor = s.zeiger;
        let n = s.schritt();
        assert_eq!(n, SCHRITT);
        assert_eq!(s.zeiger, vor + n, "genau ein Schritt, nicht zwei");
    }

    #[test]
    fn die_ausgabe_bekommt_genau_die_i16_je_kanal_des_tons() {
        let mut s = Spieler::aus_daten(&wav_test()).expect("Spieler");
        s.alles_abgeben();
        let ton = s.ton();
        let puffer = s.puffer();
        assert_eq!(puffer.len(), ton.samples * 2 * 2);
        for i in 0..1000usize {
            let soll = (ton.kanaele_daten[0][i] * 32768.0).round() as i16;
            let ist = i16::from_le_bytes([puffer[i * 4], puffer[i * 4 + 1]]);
            assert_eq!(ist, soll, "Kanal 0, Sample {i}");
            let soll = (ton.kanaele_daten[1][i] * 32768.0).round() as i16;
            let ist = i16::from_le_bytes([puffer[i * 4 + 2], puffer[i * 4 + 3]]);
            assert_eq!(ist, soll, "Kanal 1, Sample {i}");
        }
    }

    #[test]
    fn die_klemmung_der_werte_ist_wirksam() {
        let rate = 8_000;
        let ton =
            Ton::neu(rate, vec![vec![0.0, 2.0, -2.0, 1.0, -1.0, 0.5, -0.5, 0.0]]).expect("Ton");
        let werte: Vec<i16> = ton.kanaele_daten[0]
            .iter()
            .map(|w| (w * 32768.0).round().clamp(-32768.0, 32767.0) as i16)
            .collect();
        assert_eq!(
            werte,
            vec![0, 32767, -32768, 32767, -32768, 16384, -16384, 0]
        );
    }

    #[test]
    fn der_fortschritt_ist_auf_hundert_geklemmt_und_null_bei_leerem_ton() {
        let mut s = Spieler::aus_daten(&wav_test()).expect("Spieler");
        assert_eq!(s.fortschritt_prozent(), 0.0);
        s.spielen();
        s.alles_abgeben();
        assert!((s.fortschritt_prozent() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn die_mime_typen_der_formate_sind_die_richtigen() {
        // GEMESSENER Grund: `<audio>` weist eine Antwort ohne
        // `audio/*` ab, und der Server liefert heute
        // `application/octet-stream`.
        assert_eq!(Format::Wav.mime(), "audio/wav");
        assert_eq!(Format::Flac.mime(), "audio/flac");
        assert_eq!(Format::OggVorbis.mime(), "audio/ogg");
        assert!(Format::Flac.kendung().contains("flac"));
    }

    #[test]
    fn der_schritt_bleibt_weit_unter_dem_puffer() {
        // GEMESSEN: MAX_PUFFER ist 512 KiB. Bei zwei Kanaelen sind
        // 16384 Byte ein Schritt, also 3,1 %.
        let b = schritt_bytes(2);
        assert_eq!(b, SCHRITT * 4);
        assert!(
            b < MAX_PUFFER / 8,
            "{b} ist zu gross gegenueber {MAX_PUFFER}"
        );
    }
}
