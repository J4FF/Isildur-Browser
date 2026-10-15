//! G8: TON — PCM-Ausgabe, Lautstaerke, Pause, Dekoder fuer Ogg/Vorbis.
//!
//! # WAS DIESE CRATE IST und was noch nicht
//!
//! Der Auftrag (team/AUFTRAG_GEMINI_6.txt, G8): „Opus- oder
//! Vorbis-Dekoder (Vorbis zuerst, einfacher), PCM-Ausgabe ueber
//! `aplay`/`pw-play` als Kindprozess (kein `unsafe`), Lautstaerke, Pause.
//! Abnahme: Datei dekodieren, PCM gegen `ffmpeg -f s16le` (Abweichung
//! messen), Test."
//!
//! | Teil | Stand 02.10.2026 |
//! |---|---|
//! | PCM-Ausgabe ueber Kindprozess | **fertig**, 12 Tests |
//! | Lautstaerke | **fertig**, gemessen gerundet/geklemmt |
//! | Pause | **fertig**, mit gemessener Entscheidung (Bytes verkauft) |
//! | Ogg-Container + Vorbis-Kopf | **fertig**, 8 Tests |
//! | **Vorbis I-Dekoder** | **gebaut**, 104 Tests; Abnahme offen — siehe unten |
//! | Opus | nicht gebaut |
//!
//! # DER DEKODER, STAND 06.10.2026
//!
//! Gebaut sind: Codebooks (Kopf, Laengen sortiert und duenn,
//! Wertetabelle, Huffman-Baum mit exakter Kraft-Summe), Floor 1 komplett,
//! Residuum 0/1/2, Fenster, inverse MDCT, Ueberlappen, Kanalkopplung,
//! Setup-Kopf, Paketdekodierung, Weg ueber die ganze Datei.
//!
//! **Die Abnahme ist erfuellt.** Auftrag G8b fordert SNR >= 60 dB JE
//! KANAL gegen `ffmpeg -f s16le`, drei echte .ogg-Dateien. Gemessen am
//! 02.10.2026 (`bau.sh narsil-audio test --test g8b_snr`):
//!
//! ```text
//!   g8b_sine_stereo   Kanal 0: 103,9 dB   Faktor 1,00000
//!   g8b_sine_stereo   Kanal 1: 103,9 dB   Faktor 1,00000
//!   g8b_gemisch_stereo Kanal 0: 101,9 dB   Faktor 1,00000
//!   g8b_gemisch_stereo Kanal 1: 101,9 dB   Faktor 1,00000
//!   g8b_rauschen_mono  Kanal 0: 109,3 dB   Faktor 1,00000
//!   schlechtester Kanal: 101,9 dB   (gefordert 60)
//! ```
//!
//! # DIE VIER FEHLER, DIE DAS VERHINDERT HABEN
//!
//! Vier davon, jeder an einer anderen Stelle, und keiner davon fiel bei
//! den Unit-Tests auf — die pruefen Logik gegen die Spezifikation, und
//! drei der vier sind Stellen, an denen **ich die Spezifikation falsch
//! zitiert** hatte. Der vierte ist eine Normierungskonstante.
//!
//! 1. **Der Kaskadenwert wurde vor dem Schiften maskiert.** `lib/floor1.c`
//!    macht `int book=…[cval&(csub-1)]; cval>>=csubbits;` — maskiert wird
//!    nur fuer den Index. Ich hatte `cval &= csub; cval >>= cbits;`, und
//!    damit die hoeheren Bits weggeschmissen: aus `[1,0,1]` wurde
//!    `[0,0,0]`. GEMESSENE FOLGE: der Bodenrahmen brauchte 62 statt 73
//!    Bit.
//! 2. **Die inverse MDCT war falsch normiert.** `lib/mdct.c` setzt
//!    `lookup->scale = 4.f/n` — **am Ende der Vorwaerstransformation**;
//!    `mdct_backward` hat keinen solchen Faktor. Analyse und Synthese
//!    muessen `A*B = 2/N` erfuellen, mit `A = 2/N` folgt `B = 1`.
//!    GEMESSENE FOLGE: das Verhaeltnis zum Orakel war konstant 1/500,
//!    und die Datei benennt lange Bloecke mit n = 2048 — `n/4 = 512`.
//! 3. **Residuum Art 2 las zwei Klassifikationswoerter.** `lib/res0.c`
//!    liest dort eines, nicht eines je Kanal. GEMESSENE FOLGE: der
//!    Leser lief aus dem Paket und liess bis zu 7 Bit Fuellung.
//! 4. **Die Entkopplung liess den Winkelkanal stehen.** Der letzte
//!    Zweig setzt `new_A = M`, nicht `new_A = A` — ich hatte ein
//!    Spec-Zitat aus dem Gedaechtnis daneben gesetzt. GEMESSENE FOLGE:
//!    bei `L = R` ist `A = 0`, und das trifft bei jedem `s <= 0` genau
//!    diesen Zweig: Kanal 1 bei 2,0 dB gegen 103,9 dB, mit Faktor
//!    0,49863 — die Haelfte, gerundet.
//!
//! Vier neue Tests in `tests/g8b_besonderheiten.rs` nageln jede dieser
//! vier Stellen einzeln fest. Ausfuehrlich in
//! `team/BERICHT_KOMPRESS_G8b.txt`.
//!
//! **Floor 0** (Kurvenrechnung, Abschnitt 6.2.3) ist nicht gebaut, mit
//! Grund: die Gleichungen stehen im HTML-Dokument als Bilder, und
//! Abschnitt 1.2.4 sagt, dass kein Encoder seit Beta 4 Floor 0
//! schreibt. Kopf und Paket sind gebaut; `tests/g8b_setup.rs` faellt,
//! sobald eine echte Datei Floor 0 benutzt.
//!
//! # WARUM ZUERST DER WEG und nicht der Dekoder
//!
//! Drei der vier Auftragsgegenstaende sind der WEG, den die Bytes
//! nehmen: Ausgabe, Lautstaerke, Pause. Nur einer ist der Dekoder. Und
//! nur der Weg ist OHNE Dekoder testbar — man kann eine Ausgabe bauen,
//! die einen echten Kindprozess mit Bytes fuettert, ohne einen Ton zu
//! erzeugen; ein Dekoder ohne Weg ist eine Funktion, die Bytefolgen
//! liefert, die nie ein Lautsprecher sieht.
//!
//! # DER BEFUND, der die Reihenfolge bestimmt hat
//!
//! Bevor hier eine Zeile Dekoder stand, habe ich `narsil_image::vp8`
//! (G7b) gegen ein ffmpeg-Orakel geprueft: **es dekodiert nur
//! Schluesselbilder und rekonstruiert die falsch** (`max_abw=255`). Ein
//! zweiter Audio- oder Videodekoder, gebaut nach demselben Verfahren
//! (Code gegen Code, Tests gegen sich selbst), haette dasselbe Schicksal
//! und niemand haette es gemerkt. Deshalb steht hier der Weg, und der
//! Decoder ist als benannte Luecke notiert statt als behauptete
//! Funktion.

pub mod bitleser;
pub mod codebuch;
pub mod fehler;
pub mod flac;
pub mod floor;
pub mod kopf;
/// KOMPRESS 03.10.2026: EBML/Matroska — der **Container** einer Videodatei.
/// Nur der Ton: `matroska` liest Spur, Codec und **Pakete**, nicht das Bild.
pub mod matroska;
pub mod mdct;
pub mod mp3;
pub mod ogg;
pub mod ogg_flac;
pub mod opus;
pub mod pcm;
pub mod residue;
pub mod spieler;
pub mod ton;
/// KOMPRESS 03.10.2026: die Tonspur eines `<video>` — **ueber dieselbe**
/// Schnittstelle wie `<audio>` (siehe `wiedergabe.rs`).
pub mod videoton;
pub mod vorbis;
pub mod wav;
/// KOMPRESS 04.10.2026: die **Schnittstelle fuer `narsil`** — `abspielen`,
/// `pause`, `position`. **GEMESSEN** enthaelt dieses Modul **keinen** X11-Aufruf:
/// `narsil-audio` hat **keine** Abhaengigkeit von `narsil-window`, und es soll auch
/// **keine** bekommen (Anordnung: „keine Audiotests mit sichtbarem Fenster").
/// Die Gegenseite steht wortgleich in `team/WUNSCH_UI.txt`.
pub mod wiedergabe;
pub mod zeit;

pub use vorbis::{Ton, dekodiere};

/// KOMPRESS 03.10.2026: die Tonspur eines `<video>` aus Container-Bytes.
/// `Matroska` ist der Container, `Codec` sein Audio-Codec.
pub use matroska::{Codec as TonspurCodec, Tonspur};
/// KOMPRESS 03.10.2026: `<video>`-Tonspur -> `ton::Ton`. Fuellt den
/// `Format`-Zweig, den `<audio>` und `<video>` **gemeinsam** benutzen.
pub use videoton::{dauer_s as videoton_dauer_s, dekodiere as videoton_dekodiere};

/// Die Dauer einer Audiodatei aus dem Kopf, ohne Dekoder.
pub use kopf::{dauer_s, dauer_s_oder_fehler};
/// Opus im Ogg: **Kopf und Dauer**. Kein Decoder — der ist nicht gebaut.
pub use opus::{dauer_s as opus_dauer_s, ist_opus, lies_kopf as opus_lies_kopf};
pub use pcm::{AudioFehler, Ausgabe, Kindprozess, MAX_PUFFER, RATE_48000, Weg};
/// Der Spieler: Datei -> Ton -> Ausgabe, mit Zeitangabe.
pub use spieler::{Format, SCHRITT, Spieler, Zustand, dekodiere_format};
/// Der Text der Zeit: `0:00 / ?` — die Form, die `<audio>` zeigt.
pub use zeit::{dauer_text, zeit_text, zeit_zeile, zeit_zeile_aus_daten, zeit_zeile_verlaesslich};
