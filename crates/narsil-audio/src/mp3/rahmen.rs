//! Der Rahmkopf und die Nebeninformation (2.4.1 und 2.4.2).
//!
//! # WARUM DIESE STELLE ZUERST
//!
//! Ein Layer-III-Rahmen ist 4 Byte Kopf plus Nebeninformation plus
//! Hauptdaten, und **die Hauptdaten liegen nicht im Rahmen, in dem sie
//! gelesen werden** — sie liegen bis zu 511 Byte davor, im „Bit-Tank".
//! Solange Kopf und Nebeninformation nicht stimmen, ist jede weitere
//! Ausgabe geraten. Deshalb steht diese Stufe vor allem anderen, und
//! `tests/g8c_rahmen.rs` misst genau sie: **jeder** Rahmen **jeder**
//! Testdatei, Feld fuer Feld.

use crate::bitleser::{BitFehler, BitLeserVorn};
use crate::fehler::{Art, Ergebnis, Fehler};

/// Die Bitraten in kbit/s fuer MPEG-1 Layer III, nach Bitratenindex.
///
/// Abschnitt 2.4.1.3, Tabelle 3-B.1. Index 0 („frei") und 15
/// („verboten") sind mit 0 belegt — ein Decoder, der sie annimmt, liest
/// Mist.
pub static BITRATE_MPEG1_III: [u32; 16] = [
    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
];

/// Die Bitraten fuer MPEG-2 und MPEG-2.5 Layer III.
///
/// Mit abgelegt, weil die Tabelle dieselbe Form hat — und weil sie an
/// genau dieser Stelle schon einmal einen Unterschied gemacht hat: die
/// Werte sind **halb** so gross, nicht dieselben.
pub static BITRATE_MPEG2_III: [u32; 16] = [
    0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
];

/// Die Kanalbetriebsarten (2.4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kanalart {
    /// Zwei unabhaengige Kanaele.
    Stereo,
    /// Gemeinsame Kanaele, `mode_extension` sagt mehr.
    GemeinsamStereo,
    /// Zwei getrennte Kanaele, die nichts miteinander zu tun haben.
    Getrennt,
    /// Ein Kanal.
    Mono,
}

/// Der Kopf eines Layer-III-Rahmens.
#[derive(Debug, Clone, Copy)]
pub struct Kopf {
    /// Die MPEG-Version: 1, 2 oder 2.5.
    pub mpeg: u8,
    /// Die Bitrate in bit/s.
    pub bitrate: u32,
    /// Die Abtastrate in Hz.
    pub rate: u32,
    /// Der Index der Abtastrate in [`crate::mp3::tabellen::ABRATEN`].
    pub rate_index: usize,
    /// Das Fuellbit: die Rahmenlaenge ist einen Byte groesser.
    pub fuellbit: u8,
    /// Die Kanalbetriebsart.
    pub art: Kanalart,
    /// Bei gemeinsamem Stereo: die Unterteilung (0..3).
    pub unterteilung: u8,
    /// Die Laenge des ganzen Rahmens in Byte, inklusive Kopf.
    pub laenge: usize,
    /// Die Zahl der PCM-Werte je Kanal in diesem Rahmen.
    pub werte_je_kanal: usize,
    /// Wahr, wenn eine 16-Bit-Pruefsumme hinter dem Kopf steht.
    pub pruefsumme: bool,
    /// Der Byte-Index, an dem die Nebeninformation beginnt.
    pub neben_start: usize,
    /// Die Laenge der Nebeninformation in Byte.
    pub neben_laenge: usize,
    /// Die Laenge der Hauptdaten **in diesem Rahmen** (nach Fuellbit).
    pub haupt_laenge: usize,
}

impl Kopf {
    /// Liest den Kopf aus vier Bytes.
    ///
    /// # DER PFAD, UND WARUM SO VIELE PRUEFUNGEN
    ///
    /// Der Sync ist **elf** Bit: `0xFF` und die drei oberen Bit des
    /// zweiten Bytes. Wer auf `0xFF 0xFB` filtert, verliert die beiden
    /// MPEG-2-Varianten — und findet in einer echten Datei mit einer
    /// anderen Bitrate **keinen** einzigen Rahmen, ohne jede Meldung.
    ///
    /// GEPRUEFT an `g8c_sine_stereo.mp3`: alle 79 Rahmen beginnen mit
    /// `FF FB`, alle mit Version 3 (MPEG-1) und Layer 1 (Layer III).
    pub fn lies(kopf: &[u8]) -> Ergebnis<Kopf> {
        if kopf.len() < 4 {
            return Err(Fehler::neu(
                Art::FalscheGroesse,
                format!("ein Rahmkopf ist 4 Byte, da liegen nur {}", kopf.len()),
            ));
        }
        let b = |i: usize| u32::from(kopf[i]);
        // Elf Sync-Bit: die ersten acht plus die drei oberen des zweiten.
        if b(0) != 0xFF || (b(1) & 0xE0) != 0xE0 {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!(
                    "kein Rahmkopf: {:02X} {:02X} — es muessen elf Sync-Bit sein",
                    kopf[0], kopf[1]
                ),
            ));
        }
        let version = (b(1) >> 3) & 0x03;
        let layer = (b(1) >> 1) & 0x03;
        if version == 1 {
            return Err(Fehler::neu(
                Art::FalscherWert,
                "Versions-ID 01 ist reserviert (2.4.1.1)",
            ));
        }
        if layer != 1 {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!("Layer {} — hier ist nur Layer III gebaut", layer),
            ));
        }
        // GEMESSEN, und es ist eine Falle: Layer-Bit 1 heisst Layer III,
        // nicht Layer II. Der Wert im Byte ist „3 minus Layer".
        let mpeg = match version {
            3 => 1,
            2 => 2,
            _ => 0,
        };
        if mpeg == 0 {
            return Err(Fehler::neu(
                Art::FalscherWert,
                "MPEG-2.5 ist nicht gebaut (2.4.1.1)",
            ));
        }
        let pruefsumme = b(1) & 1 == 0;
        let bitrate_index = ((b(2) >> 4) & 0x0F) as usize;
        let tab = if mpeg == 1 {
            &BITRATE_MPEG1_III
        } else {
            &BITRATE_MPEG2_III
        };
        let kbit = tab[bitrate_index];
        if kbit == 0 {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!("Bitratenindex {bitrate_index} ist ungueltig (0 = frei, 15 = verboten)"),
            ));
        }
        let rate_index = ((b(2) >> 2) & 0x03) as usize;
        let rate = crate::mp3::tabellen::ABRATEN[rate_index];
        if rate == 0 {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!("Abtastratenindex {rate_index} ist reserviert (2.4.1.2)"),
            ));
        }
        if mpeg == 1 && rate_index > 2 {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!("MPEG-1 hat nur 44100, 48000 und 32000 — Index {rate_index} nicht"),
            ));
        }
        let fuellbit = (b(2) >> 1) & 1;
        let art = match (b(3) >> 6) & 0x03 {
            0 => Kanalart::Stereo,
            1 => Kanalart::GemeinsamStereo,
            2 => Kanalart::Getrennt,
            _ => Kanalart::Mono,
        };
        let unterteilung = (b(3) >> 4) & 0x03;
        // Die Rahmenlaenge (2.4.1.3): Layer III rechnet 144 Bit je
        // Kanalframe. Fuer MPEG-1 sind das 1152 PCM-Werte, fuer MPEG-2
        // nur 576 — und die halbe Rahmenlaenge.
        let werte_je_kanal = if mpeg == 1 { 1152 } else { 576 };
        let laenge = (werte_je_kanal / 8 * kbit * 1000 / rate) as usize + fuellbit as usize;
        // Die Nebeninformation: 32 Byte bei MPEG-1 Stereo, 17 bei MPEG-1
        // Mono, 17 bei MPEG-2 Stereo, 9 bei MPEG-2 Mono (2.4.2).
        let neben_laenge = match (mpeg, art == Kanalart::Mono) {
            (1, false) => 32,
            (1, true) => 17,
            (_, false) => 17,
            (_, true) => 9,
        };
        let neben_start = 4 + if pruefsumme { 2 } else { 0 };
        Ok(Kopf {
            mpeg,
            bitrate: kbit * 1000,
            rate,
            rate_index,
            fuellbit: fuellbit as u8,
            art,
            unterteilung: unterteilung as u8,
            laenge,
            werte_je_kanal: werte_je_kanal as usize,
            pruefsumme,
            neben_start,
            neben_laenge,
            haupt_laenge: laenge - neben_start - neben_laenge,
        })
    }

    /// Ist es gemeinsames Stereo mit MS-Intensitaets-Anteil?
    #[must_use]
    pub fn ms_stereo(&self) -> bool {
        self.art == Kanalart::GemeinsamStereo && self.unterteilung & 2 != 0
    }

    /// Ist es gemeinsames Stereo mit Intensitaets-Anteil?
    #[must_use]
    pub fn intensitaet(&self) -> bool {
        self.art == Kanalart::GemeinsamStereo && self.unterteilung & 1 != 0
    }

    /// Die Zahl der Kanaele.
    #[must_use]
    pub fn kanaele(&self) -> usize {
        if self.art == Kanalart::Mono { 1 } else { 2 }
    }
}

/// Die Zahl der Granulen je Kanal: **2** fuer MPEG-1, **1** fuer MPEG-2
/// und MPEG-2.5.
///
/// # GEMESSEN, und es ist eine stille Falle
///
/// `libavcodec/mpegaudiodec_template.c`, `mp_decode_layer3`:
/// ```c
///   if (s->hdr.lsf) {
///       ...
///       nb_granules = 1;
///   ...
///   } else {
///       nb_granules = 2;
///   }
/// ```
///
/// Ich hatte vier Granulen gelesen — „vier" steht in vielen
/// Beschreibungstexten und war mein erster Gedanke. GEMESSENE FOLGE: die
/// Nebeninformation von `g8c_rauschen_mono.mp3` ergab **261** statt 130
/// Bit, also das Doppelte, und der Leser lief ueber das Ende der 17 Byte
/// hinaus in die Hauptdaten.
///
/// Der Grund steht in der Norm: ein MPEG-1-Rahmen hat 1152 Werte je
/// Kanal, ein Granul 576. Also **zwei** Granulen.
pub const GRANULEN_MPEG1: usize = 2;
pub const GRANULEN_MPEG2: usize = 1;

/// Die Nebeninformation **eines** Granules und Kanalpaares (2.4.2.3).
#[derive(Debug, Clone, Copy)]
pub struct Granule {
    /// Die Laenge von Skalenfaktoren und Huffman-Daten in Bit.
    pub teil23: u32,
    /// Die Zahl der „grossen" Wertepaare.
    pub grosse_werte: u32,
    /// Der globale Verstaerkungsfaktor.
    pub global_gain: u32,
    /// Der Index in `SLEN1`/`SLEN2`.
    pub skalefaktor_komprimiert: u32,
    /// Wurde der Blocktyp gewechselt?
    pub umgeschaltet: bool,
    /// 0 normal, 1 Anfang, 2 kurz, 3 Ende.
    pub blocktyp: u32,
    /// Nur bei gemischten Bloecken: die ersten Baender bleiben lang.
    pub gemischt: bool,
    /// Die drei Codebuchwahlen (zwei bei „normal").
    pub tabelle: [u32; 3],
    /// Die drei Fensterverstaerkungen der Kurzbloecke.
    pub unterverstaerkung: [u32; 3],
    /// Die Grenzen der drei Regionen, in Linien.
    pub region0: u32,
    pub region1: u32,
    pub region2: u32,
    /// Die Praeemphasis (nur MPEG-1).
    pub praeemphasis: bool,
    /// Die Skalierung der Skalenfaktoren (0 oder 1).
    pub skalierung: u32,
    /// Die Quartett-Tabelle (32 oder 33).
    pub quartett: u32,
}

/// Die Nebeninformation eines ganzen Rahmens.
#[derive(Debug, Clone)]
pub struct Neben {
    /// Um wie viele Byte die Hauptdaten im Tank zurueckliegen.
    pub haupt_beginn: u32,
    /// `scfsi`: **4 Bit je Kanal**, keine Bit je Granule.
    ///
    /// GEMESSEN an `libavcodec/mpegaudiodec_template.c`:
    /// ```c
    /// s->granules[ch][0].scfsi = 0;/* all scale factors are transmitted */
    /// s->granules[ch][1].scfsi = get_bits(&s->gb, 4);
    /// ```
    ///
    /// Das ist ein **Bit je Bandgruppe**, nicht je Granule: Bit 0 steht
    /// fuer die Baender 0 bis 5, Bit 1 fuer 6 bis 10, Bit 2 fuer 11 bis
    /// 15 und Bit 3 fuer 16 bis 20 (2.4.3.4.9.1). Ich hatte 4 Bit je
    /// Kanal **und Granule** gelesen — 8 statt 4 bei Stereo.
    pub skfsi: [u8; 2],
    /// Die Granuelagen, Kanal zuerst: `gr[kanal][granule]`.
    pub granel: [[Granule; GRANULEN_MPEG1]; 2],
    /// Wie viele Byte der Nebeninformation gelesen wurden.
    pub gelesen: usize,
}

impl Neben {
    /// Liest die Nebeninformation eines Rahmens.
    ///
    /// # DIE REIHENFOLGE, und sie ist die ganze Fehlerquelle
    ///
    /// Nach 2.4.2.2 steht zuerst `main_data_begin` (9 Bit), dann
    /// `private_bits` (3 Bit bei Stereo, 5 bei Mono), dann **viermal**
    /// `scfsi` (4 Bit je Kanal), und **erst dann** die vier Granulen je
    /// Kanal. Wer die Granulen vorher liest, liest an der falschen Stelle
    /// — und bekommt Zahlen, die aussehen wie Zahlen.
    pub fn lies(kopf: &Kopf, daten: &[u8]) -> Ergebnis<Neben> {
        if daten.len() < kopf.neben_start + kopf.neben_laenge {
            return Err(Fehler::neu(
                Art::FalscheGroesse,
                format!(
                    "der Rahmen hat {} Byte, die Nebeninformation braucht {}",
                    daten.len(),
                    kopf.neben_start + kopf.neben_laenge
                ),
            ));
        }
        let mut l = BitLeserVorn::ab(daten, kopf.neben_start);
        let haupt_beginn = l.bits(9).map_err(bitfehler)?;
        let privat = if kopf.kanaele() == 1 { 5 } else { 3 };
        let _privat = l.bits(privat).map_err(bitfehler)?;
        let granulen = if kopf.mpeg == 1 {
            GRANULEN_MPEG1
        } else {
            GRANULEN_MPEG2
        };
        let mut skfsi = [0u8; 2];
        if kopf.mpeg == 1 {
            for platz in &mut skfsi[..kopf.kanaele()] {
                *platz = l.bits(4).map_err(bitfehler)? as u8;
            }
        }
        let mut granel = [[Granule {
            teil23: 0,
            grosse_werte: 0,
            global_gain: 0,
            skalefaktor_komprimiert: 0,
            umgeschaltet: false,
            blocktyp: 0,
            gemischt: false,
            tabelle: [0; 3],
            unterverstaerkung: [0; 3],
            region0: 0,
            region1: 0,
            region2: 0,
            praeemphasis: false,
            skalierung: 0,
            quartett: 0,
        }; GRANULEN_MPEG1]; 2];
        // DIE REIHENFOLGE: erst der Granul, dann der Kanal.
        //
        // GEMESSEN an `libavcodec/mpegaudiodec_template.c`:
        // ```c
        // for (gr = 0; gr < nb_granules; gr++) {
        //     for (ch = 0; ch < s->hdr.nb_channels; ch++) {
        //         g = &s->granules[ch][gr];
        // ```
        // Ich hatte „erst der Kanal" gelesen — bei Stereo vertauscht das
        // die beiden Kanaele, und beide Kanaele werden zu Rauschen.
        for g in 0..granulen {
            for zeile in granel.iter_mut().take(kopf.kanaele()) {
                zeile[g] = Self::granule(&mut l, kopf)?;
            }
        }
        let gelesen = l.position() - kopf.neben_start * 8;
        Ok(Neben {
            haupt_beginn,
            skfsi,
            granel,
            gelesen,
        })
    }

    /// Liest ein Granul.
    #[allow(clippy::too_many_lines)]
    fn granule(l: &mut BitLeserVorn<'_>, kopf: &Kopf) -> Ergebnis<Granule> {
        let mut b = |n: u32| -> Ergebnis<u32> { l.bits(n).map_err(bitfehler) };
        let teil23 = b(12)?;
        let grosse_werte = b(9)?;
        let global_gain = b(8)?;
        let skalefaktor_komprimiert = b(4)?;
        let umgeschaltet = b(1)? == 1;
        let mut g = Granule {
            teil23,
            grosse_werte,
            global_gain,
            skalefaktor_komprimiert,
            umgeschaltet,
            blocktyp: 0,
            gemischt: false,
            tabelle: [0; 3],
            unterverstaerkung: [0; 3],
            region0: 0,
            region1: 0,
            region2: 0,
            praeemphasis: false,
            skalierung: 0,
            quartett: 0,
        };
        if umgeschaltet {
            g.blocktyp = b(2)?;
            g.gemischt = b(1)? == 1;
            g.tabelle[0] = b(5)?;
            g.tabelle[1] = b(5)?;
            for i in 0..3 {
                g.unterverstaerkung[i] = b(3)?;
            }
            // GEMESSENER FEHLER: hier stand auch im umgeschalteten Zweig
            // `b(4)` und `b(3)` fuer region0_count und region1_count.
            // Die gibt es dort **nicht** — `init_short_region` setzt die
            // Grenzen aus festen Werten:
            // ```c
            // if (g->block_type == 2) {
            //     g->region_size[0] = (36 / 2);
            // } else {
            //     g->region_size[0] = (36 / 2);
            // }
            // g->region_size[1] = (576 / 2);
            // ```
            // GEMESSENE FOLGE: das Granul las 66 statt 59 Bit, und die
            // Nebeninformation von `g8b_sine_stereo.mp3` Rahmen 2 ergab
            // **266** Bit in einer Fassung von 256 Byte-Bit.
            //
            // Und die Kontrollrechnung stimmt jetzt exakt:
            //
            // ```text
            //   Stereo MPEG-1:  9 + 3 + 2*4 + 4*59 = 256 Bit = 32 Byte
            //   Mono   MPEG-1:  9 + 5 + 1*4 + 2*59 = 136 Bit = 17 Byte
            // ```
            //
            // Das ist der Grund, warum diese beiden Zahlen so ungerade
            // aussehen — und es ist die staerkste Gegenprobe, die es fuer
            // die Feldliste gibt: sie passt nur bei genau 59 Bit je
            // Granule auf die Byte, die die Norm vorschreibt.
            g.region0 = 18;
            g.region1 = 288;
        } else {
            for i in 0..3 {
                g.tabelle[i] = b(5)?;
            }
            g.region0 = b(4)?;
            g.region1 = b(3)?;
            g.region2 = 576;
        }
        if kopf.mpeg == 1 {
            g.praeemphasis = b(1)? == 1;
        }
        g.skalierung = b(1)?;
        g.quartett = b(1)?;
        Ok(g)
    }
}

/// Macht aus einem Bit-Leser-Fehler einen Fehler der Crate.
///
/// Die Umrechnung an **einer** Stelle: `BitLeser` (Vorbis) und
/// `BitLeserVorn` (MP3) benutzen denselben Fehlertyp, aber die Crate
/// weiter oben kennen nur `Fehler`. Wer die Umrechnung an jeder Stelle
/// wiederholt, vergisst sie einmal.
pub(crate) fn bitfehler(e: BitFehler) -> Fehler {
    Fehler::neu(Art::StromZuEnde, format!("MP3-Bitstrom: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Die ersten vier Bytes jedes Rahmens von `g8c_sine_stereo.mp3`,
    /// von Hand abgelesen. GEMESSEN mit einem Python-Lauf ueber die Datei.
    #[test]
    fn der_kopf_von_g8c_sine_stereo_ist_gemessen() {
        let daten = std::fs::read(format!(
            "{}/tests/daten/g8c_sine_stereo.mp3",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("mp3");
        // ID3v2 am Anfang: 10 Byte Kopf + Groesse, synchron-sicher.
        let id3 = 10
            + ((usize::from(daten[6] & 0x7F) << 21)
                | (usize::from(daten[7] & 0x7F) << 14)
                | (usize::from(daten[8] & 0x7F) << 7)
                | usize::from(daten[9] & 0x7F));
        let k = Kopf::lies(&daten[id3..]).expect("Kopf");
        assert_eq!(k.mpeg, 1, "MPEG-1");
        assert_eq!(k.bitrate, 128_000);
        assert_eq!(k.rate, 44100);
        assert_eq!(k.rate_index, 0);
        assert_eq!(k.kanaele(), 2);
        assert_eq!(k.laenge, 417, "144*128000/44100 = 417, noetig 417");
        assert_eq!(k.werte_je_kanal, 1152);
        assert_eq!(k.neben_laenge, 32, "MPEG-1 Stereo");
        assert_eq!(k.haupt_laenge, 417 - 4 - 32);
        assert!(!k.pruefsumme, "LAME schreibt keine Pruefsumme in den Kopf");
    }

    #[test]
    fn alle_drei_dateien_haben_gemessene_kopfwerte() {
        // GEMESSEN mit `ffprobe` und einem eigenen Lauf ueber die Datei:
        //   sine_stereo    128 kbit/s  44100 Hz  Stereo  417 Byte/Rahmen
        //   gemischt_stereo 192 kbit/s 48000 Hz  Stereo  576 Byte/Rahmen
        //   rauschen_mono  320 kbit/s  32000 Hz  Mono    1440 Byte/Rahmen
        let erwartet = [
            ("g8c_sine_stereo", 128_000u32, 44100u32, 2usize, 417usize),
            ("g8c_gemisch_stereo", 192_000, 48000, 2, 576),
            ("g8c_rauschen_mono", 320_000, 32000, 1, 1440),
        ];
        for (teil, bitrate, rate, kan, laenge) in erwartet {
            let daten = std::fs::read(format!(
                "{}/tests/daten/{teil}.mp3",
                env!("CARGO_MANIFEST_DIR")
            ))
            .expect("mp3");
            let id3 = 10
                + ((usize::from(daten[6] & 0x7F) << 21)
                    | (usize::from(daten[7] & 0x7F) << 14)
                    | (usize::from(daten[8] & 0x7F) << 7)
                    | usize::from(daten[9] & 0x7F));
            let k = Kopf::lies(&daten[id3..]).expect("Kopf");
            assert_eq!(k.bitrate, bitrate, "{teil}: Bitrate");
            assert_eq!(k.rate, rate, "{teil}: Abtastrate");
            assert_eq!(k.kanaele(), kan, "{teil}: Kanaele");
            assert_eq!(k.laenge, laenge, "{teil}: Rahmenlaenge");
        }
    }

    #[test]
    fn kein_sync_ist_ein_benannter_fehler() {
        let e = Kopf::lies(&[0x00, 0x00, 0x00, 0x00]).unwrap_err();
        assert!(e.to_string().contains("Sync"), "{e}");
        // Und ein Byte zu wenig ist ein eigener Fehler.
        let e = Kopf::lies(&[0xFF, 0xFB]).unwrap_err();
        assert!(e.to_string().contains("Rahmkopf"), "{e}");
    }

    #[test]
    fn die_reservierten_felder_sagen_nein() {
        // Versions-ID 1 ist reserviert.
        assert!(Kopf::lies(&[0xFF, 0xF9, 0x90, 0x00]).is_err());
        // Layer 0 ist reserviert (Bit 1..2 = 00).
        assert!(Kopf::lies(&[0xFF, 0xFD, 0x90, 0x00]).is_err());
        // Bitratenindex 15 ist verboten.
        assert!(Kopf::lies(&[0xFF, 0xFB, 0xF0, 0x00]).is_err());
        // Bitratenindex 0 ist frei.
        assert!(Kopf::lies(&[0xFF, 0xFB, 0x00, 0x00]).is_err());
        // Abtastratenindex 3 ist reserviert.
        assert!(Kopf::lies(&[0xFF, 0xFB, 0x0C, 0x00]).is_err());
    }

    #[test]
    fn die_nebeninformation_von_g8c_rauschen_mono_ist_17_byte() {
        // GEMESSEN: die Monodatei hat 17 Byte Nebeninformation (2.4.2),
        // die beiden Stereodateien 32. Und 9 + 17 = 26 Byte zusammen mit
        // dem 4-Byte-Kopf — das ist die kleinste denkbare Nebeninformation
        // und liegt weit unter der Rahmenlaenge von 1440 Byte: bei Mono
        // in 320 kbit/s ist der Rest Hauptdaten.
        let daten = std::fs::read(format!(
            "{}/tests/daten/g8c_rauschen_mono.mp3",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("mp3");
        let id3 = 10
            + ((usize::from(daten[6] & 0x7F) << 21)
                | (usize::from(daten[7] & 0x7F) << 14)
                | (usize::from(daten[8] & 0x7F) << 7)
                | usize::from(daten[9] & 0x7F));
        let k = Kopf::lies(&daten[id3..]).expect("Kopf");
        assert_eq!(k.kanaele(), 1);
        assert_eq!(k.neben_laenge, 17, "MPEG-1 Mono");
        let n = Neben::lies(&k, &daten[id3..]).expect("Nebeninformation");
        // GEMESSENE RECHNUNG: 9 (main_data_begin) + 5 (private) +
        // 2 * 4 (scfsi) + 2 * 54 (zwei Granulen ohne Umschaltung) =
        // 9 + 5 + 8 + 108 = 130 Bit. Die 17 Byte der Norm bieten 136 —
        // es bleiben 6 Fuellbit. Bei vier Granulen waeren es 238 Bit, und
        // das laege ueber den 17 Byte.
        eprintln!(
            "G8CFRAME Mono: haupt_beginn {}, Nebeninformation {} Bit",
            n.haupt_beginn, n.gelesen
        );
        assert!(
            (130..=136).contains(&n.gelesen),
            "die Nebeninformation ist {} Bit gelesen — die Rechnung sagt 130",
            n.gelesen
        );
        assert!(n.haupt_beginn < 512, "der Tank ist hoechstens 511 Byte");
    }

    #[test]
    fn die_kanalart_steht_im_oberen_nibble_des_vierten_bytes() {
        // GEMESSEN: bei Stereo ist Bit 7..6 = 00, bei Mono = 11. Die
        // Dateien sagen 00, 00 und 11.
        for (teil, art) in [
            ("g8c_sine_stereo", Kanalart::Stereo),
            ("g8c_gemisch_stereo", Kanalart::Stereo),
            ("g8c_rauschen_mono", Kanalart::Mono),
        ] {
            let daten = std::fs::read(format!(
                "{}/tests/daten/{teil}.mp3",
                env!("CARGO_MANIFEST_DIR")
            ))
            .expect("mp3");
            let id3 = 10
                + ((usize::from(daten[6] & 0x7F) << 21)
                    | (usize::from(daten[7] & 0x7F) << 14)
                    | (usize::from(daten[8] & 0x7F) << 7)
                    | usize::from(daten[9] & 0x7F));
            let k = Kopf::lies(&daten[id3..]).expect("Kopf");
            assert_eq!(k.art, art, "{teil}");
        }
    }

    #[test]
    fn ms_und_intensitaet_kommen_aus_der_unterteilung() {
        // 2.4.1: mode_extension Bit 1 ist MS, Bit 0 die Intensitaet.
        let mut k = Kopf::lies(&[0xFF, 0xFB, 0x90, 0x40]).expect("Kopf");
        assert_eq!(k.art, Kanalart::GemeinsamStereo);
        assert!(!k.ms_stereo() && !k.intensitaet());
        k.unterteilung = 1;
        assert!(!k.ms_stereo() && k.intensitaet());
        k.unterteilung = 2;
        assert!(k.ms_stereo() && !k.intensitaet());
        k.unterteilung = 3;
        assert!(k.ms_stereo() && k.intensitaet());
    }
}

/// Ein Granul mit Nullwerten — fuer die Tests der Nachbarmodule.
///
/// # WARUM DAS HIER STEHT UND NICHT IM TEST
///
/// `mp3::granel::tests` braucht eine `Granule`, ohne eine Datei zu
/// oeffnen. Ein `#[cfg(test)]`-Helfer in einem anderen Modul ist nicht
/// sichtbar; in der Crate-Wurzel schon, aber dann muesste er an zwei
/// Stellen benannt werden. Der Ort neben der struct ist der mit dem
/// wenigsten Aufwand fuer den naechsten, der etwas an `Granule` aendert.
#[cfg(test)]
pub(crate) fn test_granule() -> Granule {
    Granule {
        teil23: 0,
        grosse_werte: 0,
        global_gain: 0,
        skalefaktor_komprimiert: 0,
        umgeschaltet: false,
        blocktyp: 0,
        gemischt: false,
        tabelle: [0; 3],
        unterverstaerkung: [0; 3],
        region0: 0,
        region1: 0,
        region2: 576,
        praeemphasis: false,
        skalierung: 0,
        quartett: 0,
    }
}
