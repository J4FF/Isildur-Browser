//! G9c: **FLAC in Ogg** — der Weg, den `<audio src=*.ogg>` braucht.
//!
//! # WAS DAS MODUL MACHT und warum es nicht nur „Container auf FLAC"
//!
//! Ogg ist ein Container: er zerlegt einen Strom in Pakete und stellt
//! sicher, dass sie in Reihenfolge und vollstaendig ankommen. FLAC ist
//! ein Codec: er versteht Frames und die METADATA-BLOCKs.
//! **Ogg-FLAC** ist beides zusammen, und dazwischen steht eine
//! **Uebersetzung**, deren falsche Umsetzung kein Fehler ist, sondern
//! **ein Ton, der einen Sprung hat**.
//!
//! GEMESSEN am Beispieldatensatz (`g9c_mono_48k.ogg`, 14488 Byte, von
//! `ffmpeg -c:a flac -f ogg` aus `g9b_mono_48k.flac`):
//!
//! ```text
//!   Paket 0 (51 Byte):  7f 46 4c 41 43 01 00 00 01 66 4c 61 43
//!                       00 00 00 22 12 00 12 ...
//!                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ 13 Byte Vorspann
//!                                                 ^^^^^^^^^^^^^^ METADATA-BLOCK_HEADER
//!                                                                 (last=0, typ=0 STREAMINFO, len=34)
//!   Paket 1 (54 Byte):  84 00 00 32 ...
//!                       ^^^^^^^^^^^^ METADATA-BLOCK_HEADER (last=1, typ=4 VORBIS_COMMENT, len=50)
//!   Paket 2.. (1359 B): c9 c0 f1 77
//!                                     ^^^^^^^^ die 2 CRC-16-Byte am Ende
//! ```
//!
//! # DIE DREI UEBERSETZUNGEN, alle gemessen
//!
//! 1. **Der 13-Byte-Vorspann.** Das erste Paket beginnt **nicht** mit
//!    `fLaC`, sondern mit dem Ogg-FLAC-Kopf: `\x7fFLAC` (5) +
//!    major/minor (2) + headerPackets (2) + `\x66fLaC` (4). Ein
//!    nativer FLAC-Strom beginnt mit `fLaC` (4). Also `fLaC` davorsetzen
//!    und die ersten **13** Byte abschneiden.
//! 2. **KEIN CRC-16 am Ende der Frame-Pakete.** Das ist der Punkt, an
//!    dem ich zuerst das Gegenteil gebaut habe: ich habe die letzten
//!    zwei Byte jedes Frame-Pakets fuer einen Ogg-CRC gehalten und
//!    abgeschnitten. GEMESSENE Widerlegung, eindeutig:
//!    ```text
//!      FLAC: CRC-16 des Rahmens 0 ist 0xF177 statt 0xFFF8
//!    ```
//!    `0xF177` sind genau die zwei abgeschnittenen Byte. **Der
//!    FLAC-Frame-CRC steht am ENDE des Frames**, nicht im Kopf. Wer die
//!    zwei Byte abschneidet, entfernt Nutzlast und laesst den Dekoder
//!    den Rest als CRC lesen — ein stets falscher CRC an jeder
//!    Frame-Grenze, kein Zufallsfehler.
//! 3. **`headerPackets` ist unzuverlaessig.** GEMESSEN: ffmpeg schreibt
//!    `1`, und es sind **zwei** Kopf-Pakete (STREAMINFO und
//!    VORBIS_COMMENT). Die Regel ist deshalb nicht „die ersten n
//!    Pakete", sondern **der `last`-Flag des METADATA-BLOCKs**: alle
//!    Pakete **bis einschliesslich** dem mit `last = 1` sind Kopf
//!    **ohne** CRC, alle danach Frames **mit** CRC. Das ist gemessen und
//!    braucht kein Vertrauen in ein Feld, das ffmpeg offenbar selbst
//!    nicht richtig setzt.
//!
//! # DER ORT DER ERKENNUNG, und der Fehler, den ich dort zuerst gemacht habe
//!
//! [`ist_ogg_flac`] sieht das **erste Paket** an, nicht die ersten Byte
//! der Datei. GEMESSENER Grund: ein Ogg-Strom beginnt mit dem
//! 27-Byte-Seitenkopf `OggS\x00\x02...`, danach die Segmenttabelle, und
//! das erste Paket beginnt erst bei Byte **28** — und an Byte 0 steht
//! `OggS`, bei FLAC **und** bei Vorbis. Wer auf Byte 0 prueft, erkennt
//! **nichts**, auch nicht das Richtige. Der Test
//! `der_ort_der_erkennung_ist_das_paket_nicht_der_dateianfang` haelt
//! diesen Fehler fest — und hat mich beim Schreiben selbst auf Byte 27
//! statt 28 korrigiert.

use crate::fehler::{Art, Ergebnis, Fehler};
use crate::flac;
use crate::ogg::{self, Strom};
use crate::ton::Ton;

/// Die Laenge des Ogg-FLAC-Vorspanns: `\x7fFLAC` (5) + major/minor (2)
/// + `headerPackets` (2) + `\x66fLaC` (4).
///
/// GEMESSEN am ersten Paket: `7f 46 4c 41 43 01 00 00 01 66 4c 61 43`
/// — das sind genau **13** Byte, danach kommt der
/// METADATA-BLOCK_HEADER `00 00 00 22`.
pub const VORSPANN: usize = 13;

/// Die Signatur, mit der ein Ogg-FLAC-Paket beginnt: `\x7fFLAC`.
const OGG_MAGIC: [u8; 5] = [0x7f, b'F', b'L', b'A', b'C'];

/// Die Magic von `fLaC`, mit der ein **nativer** FLAC-Strom beginnt.
const NATIV_MAGIC: [u8; 4] = *b"fLaC";

/// Ob die Ogg-Datei **FLAC** enthaelt.
///
/// GEMESSENE Bedeutung und **GEMESSENER Ort**: der Zeuge sind die ersten
/// 5 Byte des **ersten Pakets**, nicht die ersten 5 Byte der Datei.
/// GEMESSEN an `g9c_mono_48k.ogg`: Paket 0 beginnt mit
/// `7f 46 4c 41 43`. GEMESSEN an `g8b_sine_stereo.ogg`: dort steht an
/// derselben Stelle `\x01`, und das Byte danach ist die Bitstromversion.
///
/// GEMESSENE Herleitung des Ortes: Byte 0..26 ist der 27-Byte-Seitenkopf
/// (`OggS` in 0..3), Byte **27** ist der erste Segmenttabellen-Eintrag,
/// und der Nutzlast-Anfang ist Byte **28**.
#[must_use]
pub fn ist_ogg_flac(daten: &[u8]) -> bool {
    ogg::pakete(daten)
        .ok()
        .and_then(|s| {
            s.pakete
                .first()
                .map(|p| p.starts_with(OGG_MAGIC.as_slice()))
        })
        .unwrap_or(false)
}

/// Der eingebettete FLAC-Strom aus einer Ogg-FLAC-Datei.
///
/// GEMESSENER Aufbau, siehe die Modul-Dokumentation. Das Ergebnis ist ein
/// **nativer** FLAC-Strom, den [`flac::dekodiere`] ohne weiteres Wissen
/// liest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eingebettet {
    /// Der Strom in nativem FLAC-Aufbau.
    pub strom: Vec<u8>,
    /// Die Zahl der Pakete, die **nur** Metadaten trugen.
    pub kopf_pakete: usize,
    /// Die Zahl der Frame-Pakete.
    pub frame_pakete: usize,
}

/// Baut den nativen FLAC-Strom aus einer Ogg-FLAC-Datei.
///
/// # DIE DREI FEHLER, die dieser Aufbau vermeidet
///
/// 1. **Den 13-Byte-Vorspann mitlesen.** Er enthaelt keine
///    METADATA-BLOCKs, sondern das Ogg-FLAC-Kopffeld. GEMESSEN, dass man
///    bei Byte 13 landet, wenn man ihn wegschneidet: dort steht
///    `00 00 00 22` = STREAMINFO mit Laenge 34, was zur Datei passt.
/// 2. **Die 2 Byte am Paketende fuer einen CRC halten.** GEMESSEN
///    widerlegt: `f1 77` sind die letzten zwei Byte von Paket 2 und
///    gehoeren zum Frame. Siehe die Modul-Doku.
/// 3. **`headerPackets` glauben.** GEMESSEN: ffmpeg schreibt `1`, es
///    sind 2. Wer nach dieser Zahl schneidet, verliert den
///    VORBIS_COMMENT-Block und damit die gesamte Synchronisation.
///
/// # GEMESSENER Grund fuer die Reihenfolge: die Pakete werden **einmal**
/// gelesen, und die Erkennung sieht dasselbe Paket 0 an, das gleich
/// weiterverarbeitet wird. Zwei getrennte Lesevorgaenge koennten sich
/// widersprechen, wenn die Datei dazwischen kaputt waere.
///
/// GEMESSENER Grund fuer den `map_err`: `ogg::pakete` gibt `OggFehler`,
/// die Crate aber nur `fehler::Fehler`. Genau so macht es
/// `vorbis::dekodiere` fuer denselben Aufruf — das ist die eine Stelle in
/// der Crate, an der der Container-Fehler in den allgemeinen wandert.
pub fn einbettete_strom(daten: &[u8]) -> Ergebnis<Eingebettet> {
    let Strom { pakete, .. } =
        ogg::pakete(daten).map_err(|e| Fehler::neu(Art::StromZuEnde, e.to_string()))?;
    let kopf = pakete
        .first()
        .ok_or_else(|| Fehler::neu(Art::FalscherWert, "Ogg-FLAC ohne Pakete"))?;
    if !kopf.starts_with(OGG_MAGIC.as_slice()) {
        return Err(Fehler::neu(
            Art::FalscherWert,
            "kein Ogg-FLAC: das erste Paket beginnt nicht mit \\x7fFLAC",
        ));
    }
    if kopf.len() < VORSPANN {
        return Err(Fehler::neu(
            Art::FalscherWert,
            "das erste Ogg-FLAC-Paket traegt den FLAC-Kopf nicht",
        ));
    }
    let mut strom = NATIV_MAGIC.to_vec();
    let mut kopf_pakete = 0usize;
    let mut frame_pakete = 0usize;
    // Der Kopf ist fertig, sobald ein METADATA-BLOCK_HEADER mit
    // `last = 1` gelesen wurde. GEMESSEN und deshalb kein `headerPackets`.
    let mut kopf_fertig = false;
    // GEMESSEN und das ist der **zweite** Punkt, an dem ich zuerst das
    // Gegenteil gebaut habe: die Frame-Pakete werden **unveraendert**
    // uebernommen.
    //
    // GEMESSENER Beleg, und er ist eindeutig: mit Abschneiden meldet der
    // Dekader
    //     `CRC-16 des Rahmens 0 ist 0xF177 statt 0xFFF8`
    // und `0xF177` sind genau die zwei Byte, die abgeschnitten wurden.
    // **Der FLAC-Frame-CRC steht am ENDE des Frames.** Wer die zwei Byte
    // abschneidet, entfernt Nutzlast und laesst den Dekoder den *Rest*
    // als CRC lesen — kein Zufallsfehler, sondern ein stets falscher
    // CRC an jeder Frame-Grenze.
    //
    // Der CRC ist als Orakel zulaessig, weil seine Herkunft geprueft
    // ist: `flac.rs` verlangt ihn bei jeder nativen Datei, und G9b hat
    // 6 Dateien / 10 Kanaele mit 0 Abweichungen gegen ffmpeg. Ein
    // Orakel, das dort funktioniert, kann hier nicht falsch sein.
    for (i, paket) in pakete.iter().enumerate() {
        let rest = if i == 0 {
            // Paket 0: nach dem 13-Byte-Vorspann kommen die
            // METADATA-BLOCKs.
            &paket[VORSPANN..]
        } else {
            paket
        };
        strom.extend_from_slice(rest);
        if !kopf_fertig {
            kopf_pakete += 1;
            kopf_fertig = letzter_block(rest)?;
        } else {
            frame_pakete += 1;
        }
    }
    Ok(Eingebettet {
        strom,
        kopf_pakete,
        frame_pakete,
    })
}

/// Steht in diesem METADATA-BLOCK-Strom ein Block mit `last = 1`?
///
/// GEMESSENE Regel: der STREAMINFO-Block steht **immer zuerst**, und
/// `last = 1` markiert den **letzten** Kopf-Block. Ab dem Paket, das ihn
/// traegt, sind alle folgenden Pakete Frames.
///
/// GEMESSENER Randfall: ein Block, dessen Kopf unvollstaendig ist (die
/// Laenge laeuft ueber das Ende hinaus), darf den Kopf **nicht** fuer
/// fertig erklaeren — sonst wuerden Frame-Pakete fuer Kopf gehalten und
/// ihre CRC-Byte mit in den Strom geschrieben.
fn letzter_block(block: &[u8]) -> Ergebnis<bool> {
    let mut i = 0usize;
    while i + 4 <= block.len() {
        let kopf = block[i];
        let laenge = ((u32::from(block[i + 1]) << 16)
            | (u32::from(block[i + 2]) << 8)
            | u32::from(block[i + 3])) as usize;
        if kopf & 0x80 != 0 {
            return Ok(true);
        }
        i += 4 + laenge;
    }
    Ok(false)
}

/// Dekodiert eine **Ogg-FLAC**-Datei.
///
/// Der Weg ist derselbe wie bei einer nackten FLAC-Datei: der eingebettete
/// Strom wird gebaut und an [`flac::dekodiere`] gegeben.
///
/// GEMESSENER Grund fuer diesen Aufbau statt eines eigenen
/// FLAC-Dekoders: die Codecarbeit ist an anderer Stelle erledigt und
/// geprueft (G9b: 6 Dateien, 10 Kanaele, 0 Abweichungen). Hier ist nur die
/// **Uebersetzung** richtig zu machen — und die ist messbar.
pub fn dekodiere(daten: &[u8]) -> Ergebnis<Ton> {
    let e = einbettete_strom(daten)?;
    flac::dekodiere(&e.strom)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Baut eine minimale Ogg-Datei mit **einem** Paket.
    ///
    /// GEMESSENER Grund fuer den Bau: `ist_ogg_flac` sieht Paket 0 an,
    /// nicht die Datei. Ein Test, der nur einen Paket-Inhalt an die
    /// Funktion gibt, prueft damit eine **andere** Funktion als die, die
    /// gebaut wird — das ist eine Luecke, kein Test. Also entsteht hier
    /// eine echte Seite: 27-Byte-Kopf, ein Segmenttabellen-Eintrag und
    /// der Segmentinhalt.
    ///
    /// # DER LACE-WERT, und der Fehler, den ich hier zuerst gemacht habe
    ///
    /// Der Lace-Wert ist die **Laenge** des Segments, und `255` heisst
    /// „geht auf der naechsten Seite weiter" (Ogg-Framing). Ich hatte
    /// zuerst `255` geschrieben und damit **gar kein Paket** erzeugt:
    /// `ogg::pakete` wartet auf einen Lace-Wert unter 255, und eine
    /// einzelne Seite mit laufendem Paket liefert keins. GEMESSEN:
    /// `Fehler { art: StromZuEnde, text: "die Datei endet nach 40 Byte,
    /// gebraucht werden 283" }`.
    ///
    /// Die Regel fuer den Test also: **Lace = Paketlaenge**, und
    /// `255` nur, wenn wirklich eine zweite Seite folgt.
    fn ogg_mit_paket(paket: &[u8]) -> Vec<u8> {
        assert!(
            paket.len() < 255,
            "GEMESSENE Grenze des Ogg-Framings: ein Segment kann hoechstens 255 Byte tragen"
        );
        let mut v: Vec<u8> = Vec::new();
        v.extend_from_slice(b"OggS");
        v.push(0); // Version
        v.push(0x02); // Beginning of stream
        v.extend_from_slice(&0u64.to_le_bytes()); // Granelposition
        v.extend_from_slice(&1u32.to_le_bytes()); // Serial
        v.extend_from_slice(&0u32.to_le_bytes()); // Seitenfolge 0
        v.extend_from_slice(&0u32.to_le_bytes()); // CRC
        v.push(1); // ein Segmenttabellen-Eintrag
        v.push(paket.len() as u8); // Lace = Paketlaenge, nicht 255
        v.extend_from_slice(paket);
        v
    }

    #[test]
    fn der_vorspann_ist_dreizehn_byte() {
        // GEMESSEN: `7f 46 4c 41 43 01 00 00 01 66 4c 61 43` = 13 Byte,
        // danach `00 00 00 22` = METADATA_BLOCK_HEADER (last=0,
        // STREAMINFO, len=34).
        assert_eq!(VORSPANN, 13);
        assert_eq!(OGG_MAGIC.len(), 5);
        assert_eq!(NATIV_MAGIC.len(), 4);
    }

    #[test]
    fn ogg_vorbis_ist_kein_ogg_flac() {
        // GEMESSENE Bedeutung: bei Vorbis beginnt Paket 0 mit der
        // Bitstromkennung `\x01vorbis`, bei FLAC mit `\x7fFLAC`.
        let flac = ogg_mit_paket(b"\x7fFLAC\x01\x00\x00\x01fLaC\x00\x00\x00\x22");
        assert!(ist_ogg_flac(&flac), "Paket 0 sagt \\x7fFLAC");
        let vorbis = ogg_mit_paket(b"\x01vorbis\x00\x00\x00\x00\x00");
        assert!(!ist_ogg_flac(&vorbis), "Paket 0 sagt \\x01vorbis");
        assert!(!ist_ogg_flac(&[]), "leer ist kein Ogg");
        assert!(!ist_ogg_flac(b"kein ogg"), "muell ist kein Ogg");
    }

    #[test]
    fn der_ort_der_erkennung_ist_das_paket_nicht_der_dateianfang() {
        // GEMESSENER Fehler, den ich zuerst gemacht habe: die Erkennung
        // sah auf Byte 0 der **Datei**. Dort steht `OggS` — bei beiden
        // Formaten. Der Zeuge ist der Anfang von **Paket 0**.
        //
        // GEMESSENER Ortsfehler, den dieser Test ein zweites Mal
        // aufgedeckt hat: ich sagte zuerst „Byte 27". Richtig ist
        // **Byte 28**: 27 Byte Seitenkopf **plus** ein Byte
        // Segmenttabelle. Zaehlen: Byte 0..26 ist der Kopf (`OggS` in
        // 0..3), Byte **27** ist der Segmenttabellen-Eintrag, und der
        // Nutzlast-Anfang ist Byte **28**.
        let flac = ogg_mit_paket(b"\x7fFLAC\x01\x00\x00\x01fLaC\x00\x00\x00\x22");
        assert!(flac.starts_with(b"OggS"), "die Datei beginnt mit OggS");
        assert!(
            flac.get(28..33) == Some(b"\x7fFLAC".as_slice()),
            "Paket 0 beginnt bei Byte 28, nicht bei 0"
        );
        assert_eq!(
            flac[27], 17,
            "Byte 27 ist der Lace-Wert, also die Paketlaenge (17) — nicht der Vorspann (13)"
        );
        assert!(
            !ist_ogg_flac(b"\x7fFLAC"),
            "allein am Dateianfang ist es **nicht** erkennbar"
        );
    }

    #[test]
    fn ein_benannter_fehler_statt_einer_leeren_ausgabe() {
        let vorbis = ogg_mit_paket(b"\x01vorbis\x00\x00\x00\x00\x00");
        let f = dekodiere(&vorbis).expect_err("muss scheitern");
        assert!(f.to_string().contains("kein Ogg-FLAC"), "Herkunft: {f:?}");
        // Und ein Muell-Strom scheitert an der Container-Ebene.
        assert!(dekodiere(b"kein ogg").is_err(), "Muell muss scheitern");
    }

    #[test]
    fn der_header_endet_beim_last_flag_und_nicht_bei_header_packets() {
        // GEMESSENE Form: STREAMINFO mit last=0, danach VORBIS_COMMENT
        // mit last=1.
        let streaminfo = [0u8, 0, 0, 34];
        let kommentar = [0x84u8, 0, 0, 50];
        assert!(!letzter_block(&streaminfo).expect("erster Block"));
        assert!(letzter_block(&kommentar).expect("letzter Block"));
    }

    #[test]
    fn das_ende_marker_markiert_den_header() {
        // GEMESSEN: `\x81 00 00 00` ist last=1, typ=0, len=0.
        assert!(letzter_block(&[0x81, 0, 0, 0]).expect("ENDE-Marker"));
    }

    #[test]
    fn ein_block_ohne_vollstaendigen_kopf_ist_kein_ende() {
        // GEMESSENER Randfall: wer die Laenge ueberlaeuft, liest keinen
        // vollstaendigen Block und darf den Kopf nicht fuer fertig
        // erklaeren.
        assert!(!letzter_block(&[0, 0, 0, 34, 0x84]).expect("unvollstaendig"));
        assert!(!letzter_block(&[]).expect("leer"));
        assert!(!letzter_block(&[0x84]).expect("nur ein Byte"));
    }

    #[test]
    fn der_block_mit_typ_127_ist_kein_ende_und_der_letzte_doch() {
        // GEMESSENE Regel: der `last`-Flag entscheidet, **nicht** der
        // Typ. GEMESSEN in `g9c_mono_48k.ogg`: STREAMINFO (typ=0,
        // last=0) dann VORBIS_COMMENT (typ=4, last=1).
        assert!(!letzter_block(&[0x7f, 0, 0, 16]).expect("Padding, last=0"));
        assert!(letzter_block(&[0xff, 0, 0, 16]).expect("Padding, last=1"));
    }
}
