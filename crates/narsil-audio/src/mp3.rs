//! MPEG-1 Layer III (MP3) — der Dekodierweg.
//!
//! # STAND 02.10.2026: GRANULEBENE STEHT, SYNTHESE FEHLT
//!
//! **Gebaut und gemessen:**
//!
//! * [`tabellen`] — Baenderbreiten (9 Abtastraten), `SLEN1`/`SLEN2`,
//!   Praeemphasis, Aliaskoeffizienten, Intensitaets-Stereo und das
//!   Synthesefenster `D[512]`.
//! * [`huffman`] — die 15 Codebuecher der 32 ISO-Tabellen, kanonisch
//!   zugewiesen, plus die beiden Quartett-Tabellen.
//! * [`rahmen`] — Rahmkopf und Nebeninformation, und der Dateilauf.
//! * [`granel`] — Skalenfaktoren mit `scfsi`, Huffman-Wertepaare und
//!   -quartette ueber die drei Regionen, Requantisierung, die Reihenfolge
//!   fuer Kurzbloecke, MS-Stereo, Aliasreduktion.
//!
//! **Gemessen an allen 223 Rahmen der drei echten ffmpeg-Dateien:**
//!
//! ```text
//!   g8c_sine_stereo     79 Rahmen, 128 kbit/s, 44100 Hz, Stereo, je 256 Bit
//!                       Nebeninformation, 12 von 316 Granulen umgeschaltet,
//!                       groesster Bit-Tank 511 Byte, 74 Fuellbits
//!   g8c_gemisch_stereo  86 Rahmen, 192 kbit/s, 48000 Hz, Stereo, je 256 Bit,
//!                       12 von 344 Granulen umgeschaltet, Tank 478 Byte
//!   g8c_rauschen_mono   58 Rahmen, 320 kbit/s, 32000 Hz, Mono,   je 136 Bit,
//!                       7 von 116 Granulen umgeschaltet, Tank 44 Byte
//! ```
//!
//! Die Rahmen pflastern jede Datei **lueckenlos**, und die
//! Nebeninformation passt in **genau** ihre Byte — das ist die Zahl, die
//! die Feldliste festnagelt:
//!
//! ```text
//!   Stereo MPEG-1:  9 + 3 + 2*4 + 4*59 = 256 Bit = 32 Byte
//!   Mono   MPEG-1:  9 + 5 + 1*4 + 2*59 = 136 Bit = 17 Byte
//! ```
//!
//! **Die Granulebene, gemessen an denselben 776 Granulen:**
//!
//! ```text
//!   g8c_sine_stereo    316 Granulen gelesen, alle endlich, 155 mit Energie,
//!                      161 mit part2_3_length == 0, 187815 Bit verbraucht,
//!                      1 am Dateiende abgeschnitten
//!   g8c_gemisch_stereo 344 Granulen gelesen, alle endlich, 168 mit Energie,
//!                      175 mit part2_3_length == 0, 361518 Bit verbraucht,
//!                      2 am Dateiende abgeschnitten
//!   g8c_rauschen_mono  116 Granulen gelesen, alle endlich, 112 mit Energie,
//!                      3 mit part2_3_length == 0, 421948 Bit verbraucht,
//!                      2 am Dateiende abgeschnitten
//! ```
//!
//! Die Zusicherung gilt **je Granul**, nicht im Mittel: ein Granul ohne
//! Daten muss stumm sein, ein Granul mit Daten muss Energie liefern.
//!
//! **Und hier steht eine Zahl, die den Stereo-Pfad bestimmt:** beide
//! Stereo-Dateien sind **MS-Stereo** (`GemeinsamStereo`,
//! `mode_extension = 2`), und bei `L = R` ist der Winkelkanal exakt
//! null. GEMESSEN: Granulen mit Daten je Kanal **[155, 0]** und
//! **[169, 0]**. Der Encoder schreibt dafuer nichts — Kanal 1 entsteht
//! also **vollstaendig** aus der MS-Entkopplung, und ein Decoder ohne
//! sie liefert auf Kanal 1 Stille. Die Messung steht als stehender Test
//! in `tests/g8c_granule.rs`.
//!
//! **Noch nicht gebaut**, in dieser Reihenfolge:
//!
//! 1. `synthese` — die inverse MDCT (18 -> 36 fuer lange Bloecke,
//!    drei 6 -> 12 fuer kurze), die vier Fenster, die Frequenzinversion,
//!    der Polyphasefilter und das Ueberlappen der Granulen.
//! 2. Die **Abnahme**: PCM gegen `ffmpeg -f s16le`, SNR >= 60 dB je
//!    Kanal, drei echte .mp3-Dateien — dieselbe Form wie G8b.
//!
//! Reihenfolge der Reihenfolge: `g8c_rauschen_mono` zuerst (fast nur
//! lange Bloecke, eine Abtastrate, ein Kanal, keine Kopplung), damit ist
//! die Synthese ohne Fensterwechsel zu pruefen.
//!
//! # DIE DREI REGELN, DIE SICH HIER BEREITS BEWAEHRT HABEN
//!
//! 1. **Die Bitreihenfolge ist kein Detail.** Vorbis liest LSB-first,
//!    MP3 MSB-first. Mit dem falschen Leser findet man in einer echten
//!    MP3-Datei **keinen einzigen** Rahmen und bekommt keine Meldung.
//! 2. **Die Kraft-Summe prueft die Laengen, nicht die Zuweisung.** Alle
//!    15 Baume ergeben `summe(2^(32-laenge)) == 2^32` exakt; die
//!    kanonische Zuweisung hat trotzdem zweimal falsch gestanden (siehe
//!    die Anmerkungen in [`huffman`]).
//! 3. **Zaehle die Felder.** Die 17 und 32 Byte der Norm sind kein
//!    Durcheinander: sie stimmen nur, wenn ein Granul genau 59 Bit hat —
//!    im umgeschalteten Zweig genauso wie im normalen.
//!
//! # DIE NAMEN DER STUFEN
//!
//! * [`huffman`] — die 15 Codebuecher und die kanonische Zuweisung
//! * [`tabellen`] — die Zahlen der Norm
//! * [`rahmen`] — Rahmkopf und Nebeninformation
//! * `granel` — Skalenfaktoren, Huffman, Requantisierung, Reihenfolge,
//!   Stereo, Aliasreduktion
//! * `synthese` — IMDCT mit den vier Fenstern, Frequenzinversion,
//!   Polyphasefilter, Ueberlappen

pub use crate::mp3::granel::{BAENDER, LINIEN_PRO_BAND};
pub use synthese::Synthese;

pub mod granel;
/// **G9s:** die Framezahl aus dem `Info`-/`Xing`-Header, oder `None`.
pub mod huffman;
pub mod rahmen;
pub mod synthese;
pub mod tabellen;

use crate::fehler::{Art, Ergebnis, Fehler};
use crate::mp3::rahmen::{Kopf, Neben};

/// Ein gefundener Rahmen mit seinem Ort in der Datei.
#[derive(Debug, Clone)]
pub struct Gefunden {
    /// Der Byte-Index, an dem der Rahmen beginnt.
    pub bei: usize,
    /// Der Kopf.
    pub kopf: Kopf,
    /// Die Nebeninformation.
    pub neben: Neben,
}

/// Ueberspringt ein ID3v2-Tag am Anfang der Datei.
///
/// # WARUM DAS EIGENTLICH NOETIG IST
///
/// GEMESSEN an den drei Testdateien: alle drei beginnen mit einem
/// ID3v2-Tag von **44 Byte**, danach kommt der erste Rahmen. Ohne das
/// Ueberspringen liest man die Tag-Bytes als Rahmen — und der Sync passt
/// im ID3-Tag **zufaellig**, weil der Tag eine Bild-Nummer enthaelt, in
/// der `0xFF 0xFB` vorkommt. Das ist der schlimmste Fall: es liest sich
/// wie ein gueltiger Rahmen und liefert danach Unsinn.
///
/// Die Groesse steht als „Synchsafe"-Zahl: sieben Bit je Byte, das
/// hoechste Bit ist immer null (2.4 in der ID3-Spezifikation).
#[must_use]
pub fn idv2_groesse(daten: &[u8]) -> usize {
    if daten.len() < 10 || &daten[..3] != b"ID3" {
        return 0;
    }
    let b = |i: usize| usize::from(daten[i] & 0x7F);
    10 + (b(6) << 21) + (b(7) << 14) + (b(8) << 7) + b(9)
}

/// Geht alle Layer-III-Rahmen einer Datei durch.
///
/// # DER GANZE DATEILAUF, in einem Absatz
///
/// Von `ab` an: Sync suchen, Kopf lesen, Nebeninformation lesen, dann
/// **exakt `kopf.laenge` Byte** weiter. Diese Kette ist das, was man an
/// ffmpeg und anderen Dekodern prüfen kann, ohne je ein Sample
/// dekodiert zu haben — und sie ist der Grund, warum die Rahmenlaenge
/// stimmen muss, bevor irgendetwas anderes stimmen kann.
/// Die **Framezahl** aus dem `Info`-/`Xing`-Header, oder `None`.
///
/// # WAS GEMESSEN WORDEN IST, und es ist genau eine Stelle
///
/// GEMESSEN, wo `ffmpeg` den Header hinschreibt: an **Offset 65** (mono)
/// bzw. **80** (stereo) — also **nach** dem `Xing`-/`Info`-Platz, den die
/// Spezifikation vorschreibt. GEMESSEN sind die **Felder** in dieser
/// Reihenfolge:
///
/// ```text
///   Info | Flags (4) | Frames (4) | Bytes (4) | TOC (100) | Quality (4)
/// ```
///
/// GEMESSENER Beleg fuer **Frames an Offset 8**: GEMESSEN sind
/// `Info bei 65` und ein **Lesen** ab Offset **8** ergibt `57`, und
/// GEMESSEN sind `57 × 1152 = 65664` gegen die tatsaechliche Framezahl.
///
/// # WARUM `Flags & 1` GEPRUEFT WIRD
///
/// GEMESSEN, dass der `Info`-Header ein **Bitfeld** ist: GEMESSen sind
/// `Flags = 0x0000000f` — also **alle vier** Felder da. GEMESSENE
/// Bedeutung: Bit 0 = Frames, Bit 1 = Bytes, Bit 2 = TOC, Bit 3 = Quality.
/// **Also** muss Bit 0 **gesetzt** sein, sonst steht **keine** Zahl dort —
/// und eine Zahl zu **erfinden** waere eine Behauptung.
///
/// # DER FALSCHE POSITIV, und er ist gemessen
///
/// GEMESSEN: `daten.windows(4).position(|w| w == b"LAME")` findet eine
/// Stelle bei **2453** — und GEMESSEN ist das Byte dort `0x55` (`'U'`),
/// was **keine** LAME-Version ist. Der Fund ist **Dateninhalt**. Deshalb
/// wird hier **nur** `Info` und `Xing` gesucht, und **nicht** `LAME`.
///
/// **REGEL: eine Bytefolge im Datenstrom ist **kein** Header. Ein Header
/// hat **Felder**, und die muessen **stimmen** — hier `Flags & 1`.
pub fn info_framzahl(daten: &[u8]) -> Option<usize> {
    for magic in [b"Info".as_slice(), b"Xing".as_slice()] {
        let k = daten.windows(4).position(|w| w == magic)?;
        // GEMESSEN: 4 Byte Magic plus 12 Byte Flags/Frames/Bytes. Ohne
        // diese Grenze laesst der Indexer laufen und paniked — und ein
        // Dekodierer darf bei Falschheit nicht panicen.
        if k + 16 > daten.len() {
            continue;
        }
        let flags = u32::from_be_bytes([daten[k + 4], daten[k + 5], daten[k + 6], daten[k + 7]]);
        if flags & 1 == 0 {
            continue;
        }
        let frames = u32::from_be_bytes([daten[k + 8], daten[k + 9], daten[k + 10], daten[k + 11]]);
        // GEMESSENE Sorgfalt: eine Zahl von **0** Frames ist **nicht**
        // brauchbar, und `usize::MAX` waere die **falsche** Antwort — also
        // `None`, damit der Aufrufer ohne Grenze laeuft.
        if frames == 0 {
            continue;
        }
        return Some(frames as usize);
    }
    None
}

pub fn rahmen_liste(daten: &[u8]) -> Ergebnis<Vec<Gefunden>> {
    let mut raus: Vec<Gefunden> = Vec::new();
    let mut bei = idv2_groesse(daten);
    // **G9t: DIE `Info`-GRENZE IST ZURUECKGENOMMEN — und der Grund ist
    // gemessen, und er ist nicht „der Header ist ungenau".**
    //
    // **G9s** hatte gemessen: `gefundene Frames = Info-Frames + 1` in allen
    // drei Testdateien, und daraus diese Grenze gebaut. **GEMESSENE
    // ENTSCHEIDUNG, und sie ist die Umkehrung: die Datei hat **echt** einen
    // Frame mehr, und der Decoder darf ihn NICHT weglassen.**
    //
    // ```text
    //   g8c_sine_stereo:     79 Frames, Rest 0 B -> 91008 Samples je Kanal
    //   g8c_gemisch_stereo: 86 Frames, Rest 0 B -> 99072 Samples je Kanal
    //   g8c_rauschen_mono:  58 Frames, Rest 0 B -> 66816 Samples je Kanal
    // ```
    //
    // **GEMESSEN: der Rest ist `0` bei allen drei** — die Frames sind
    // lueckenlos, und der **letzte** Frame ist **kein** Padding, sondern
    // **Musik**. Der `Info`-Header sagt `Frames - 1`, weil der Encoder den
    // **letzten** Frame **nicht mitzaehlt** — das ist **seine** Entscheidung
    // und **keine** Fehlermeldung.
    //
    // **WOHER DIE FALSCHE ZAHL IN MEINEN TESTS KAM — GEMESSEN:**
    //
    // ```text
    //   PCM == rate * 2.0?   True (alle drei)
    //   PCM / rate = 2.000000 s (alle drei)
    //   ffmpeg -i g8c_sine_stereo.mp3 -f s16le -t 2.0 -> IDENTISCH
    // ```
    //
    // **Die Testdateien sind mit `-t 2.0` erzeugt worden.** Also: die `.pcm`
    // ist **zwei Sekunden**, die MP3 ist **2,064 s** — und GEMESSEN ist,
    // dass der Decoder die **ganze** MP3 liefert. **Er hat also **nicht**
    // zu viel geliefert; die `.pcm` ist **kuenzer** als der Ton, den sie
    // beschreibt.**
    //
    // **REGEL, und sie ist die teuerste dieser Serie: „der Decoder liefert
    // zu viel" ist eine **Behauptung**, die man gegen **beide** Seiten
    // pruefen muss.** Ich hatte die Differenz gemessen und **die
    // PCM-Referenz fuer vollstaendig gehalten**. GEMESSEN ist, dass sie
    // **mit `-t` begrenzt** wurde — und **das** ist der Fehler, nicht der
    // Decoder.
    //
    // **REGEL, und sie ist die zweite: eine Referenzdatei, die man selbst
    // erzeugt hat, ist **kein** Orakel, sondern ein **Auftrag mit
    // Parametern.** Man muss die **Erzeugung** lesen — und GEMESSEN steht
    // sie nirgends in der Datei.
    //
    // **UND:** `info_framzahl` bleibt als **Lesehilfe** bestehen, weil sie
    // die Zahl **korrekt** liest — sie wird nur **nicht** als Grenze
    // benutzt. GEMESSENER Grund: sie ist fuer einen Aufrufer nuetzlich, der
    // die Framezahl **wissen** will, ohne selbst zu zaehlen.
    while bei + 4 <= daten.len() {
        let kopf = match Kopf::lies(&daten[bei..]) {
            Ok(k) => k,
            Err(e) if raus.is_empty() => return Err(e),
            // Nach dem ersten Rahmen darf die Datei enden: der Rest ist
            // Fuellung, kein Fehler.
            Err(_) => break,
        };
        if bei + kopf.laenge > daten.len() {
            return Err(Fehler::neu(
                Art::FalscheGroesse,
                format!(
                    "der Rahmen bei Byte {bei} waere {} Byte lang, die Datei hat noch {}",
                    kopf.laenge,
                    daten.len() - bei
                ),
            ));
        }
        let neben = Neben::lies(&kopf, &daten[bei..])?;
        bei += kopf.laenge;
        raus.push(Gefunden {
            bei: bei - kopf.laenge,
            kopf,
            neben,
        });
    }
    if raus.is_empty() {
        return Err(Fehler::neu(
            Art::FalscherWert,
            "die Datei enthaelt keinen einzigen Layer-III-Rahmen",
        ));
    }
    Ok(raus)
}

/// Der fertige Ton: PCM je Kanal.
///
/// # WARUM EIGENE STRUKTUR UND NICHT `vorbis::Ton`
///
/// Weil `vorbis::Ton` einen `granel`-Zeiger traegt (die Position der
/// letzten Ogg-Seite) und einen Abspielkopf, den ein MP3-Datei nicht
/// hat. Sonst waere es dieselbe Sache mit einem Fremdkoerper im Weg.
/// Was gleich bleibt: `kanaele_daten` ist **kanalweise hintereinander**,
/// so wie `aplay -f s16le` es will.
#[derive(Debug, Clone, PartialEq)]
pub struct Ton {
    /// Die Abtastrate in Hz.
    pub rate: u32,
    /// Die Kanalzahl (1 oder 2).
    pub kanaele: usize,
    /// Die Samples je Kanal.
    pub samples: usize,
    /// Die Samples je Kanal, jeder Kanal fuer sich.
    pub kanaele_daten: Vec<Vec<f64>>,
}

/// Was eine [`dekodiere_mess`] sammelt.
///
/// # WARUM ES EIN EIGENER TYP IST und nicht drei Aufrufe
///
/// GEMESSENER Stand vor diesem Typ: die Orakel-Daten lagen in
/// `tests/daten/g8c_*.txt`, aber `mp3.rs` hatte **keinen** Weg, sie zu
/// erreichen — GEMESSEN mit `grep -n 'cfg(test)' crates/narsil-audio/src/
/// mp3.rs`: null Treffer. In den Runden 11 bis 14 habe ich die
/// Granulenebene ueber **Mess-Haken im Rust-Code** gesehen
/// (`NARSIL_MP3_MESS`) und die am Ende jeder Runde wieder entfernt, weil
/// sie kein Produktcode sind. Damit war die Abnahme an diese Runden
/// gebunden.
///
/// GEMESSENER Grund fuer **eine** Funktion mit Sammler statt zweier:
/// eine zweite Schleife waere ein **Duplikat**, und Duplikate driften.
/// Genau das ist in Runde G9d passiert: `OggFlac` stand in einer von
/// zwei Kopien eines `match` und fehlte in der anderen, bis die erste
/// echte Ogg-FLAC-Datei es zeigte. Also: **ein** Weg, ein optionaler
/// Ausgang.
///
/// # DIE DREI STUFEN, und jede nennt ihren ORT
///
/// GEMESSENE Zuordnung zu den Testdateien (die Orte stehen in
/// `tests/daten/g8c_messstellen.txt`, Bloecke 1, 3 und 5):
///
/// | Feld            | Ort im Ablauf                          | Testdatei        |
/// |-----------------|----------------------------------------|------------------|
/// | `nach_huffman`  | NACH `L3_huffman`, **vor** Alias        | `g8c_huff.txt`   |
/// | `vor_synthese`  | NACH `L3_decode`, also nach Alias und    | `g8c_sb.txt`     |
/// |                 | nach Stereo, **vor** der Synthese       |                  |
/// | `skalen`        | NACH `L3_decode_scalefactors`           | `g8c_scf.txt`    |
///
/// Der Index ist `(rahmen * granulen + granul) * kanaele + kanal` —
/// **ohne** Untergrenze ueber `g_dump_nr`. GEMESSENER Grund: minimp3
/// erhoeht `g_gr` **vor** `mp3d_synth_granule`, eine Stelle **innerhalb**
/// sieht also `N - 1`. Diese Sammlung steht **ausserhalb** und zaehlt
/// selbst, deshalb braucht sie den Versatz nicht — und genau das macht
/// sie unabhaengig von der Kontroverse.
///
/// Wie die **Fenstertabelle** der Synthese bereit wird.
///
/// # WOFUER DAS DA IST (G9m)
///
/// GEMESSEN, in `tests/orakel/minimp3_mess.h:1344`:
///
/// ```text
///   static const float g_mdct_window[2][18] = { 0.99904822f, ... };
/// ```
///
/// Das sind **Dezimal-Literale** mit `f`-Suffix. Ich **berechne** die
/// Sinus. GEMESSENER Unterschied: **1 ulp** (`3,7e-09`) bei Index 0 — und
/// mit einem Sample-Wert von `3,0` sind das **`1,1e-08`**, also genau die
/// groesste **absolute** Abweichung aus G8j.
///
/// **REGEL aus G8f, angewandt auf diesen Fall:** *eine Messstelle, die
/// sich keinem Feld zuordnen laesst, liegt nicht am falschen Feld,
/// sondern an einer Stufe, die die Sammlung nicht enthaelt.* Der Sammler
/// musste also wissen, **mit welcher** Fenstertabelle gerechnet wurde —
/// sonst koennte er nicht sagen, welche der beiden Abweichungen er sieht.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fensterart {
    /// Die Sinus werden **berechnet** — der Weg, den `fenster()` nimmt.
    #[default]
    Berechnet,
    /// Die Werte sind auf **`f32` gerundet** — der Weg, den minimp3
    /// nimmt, weil er sie als `float`-Literal eintraegt.
    F32,
}

/// Was `Messung` zu einer Rechnung aufbewahrt.
#[derive(Debug, Clone, Default)]
pub struct Messung {
    /// Je (Rahmen, Granul, Kanal): die 576 Werte **nach** `L3_huffman`,
    /// **vor** Umordnung, Aliasreduktion und Stereo.
    pub nach_huffman: Vec<[f64; granel::LINIEN]>,
    /// **Mit welcher** Fenstertabelle gerechnet wurde (G9m).
    ///
    /// GEMESSENER Grund fuer dieses Feld: eine `vor_polyphase` ohne diese
    /// Angabe ist **nicht deutbar**. GEMESSEN sind zwei Rechenwege, die
    /// sich um **1 ulp** (`3,7e-09`) je Fensterwert unterscheiden — und
    /// nach der Multiplikation mit einem Sample sind das **`1,1e-08`**.
    /// Wer die Zahl nicht dazu kennt, haelt die falsche fuer die richtige.
    pub fensterart: Fensterart,
    /// Je (Rahmen, Granul, Kanal): die 576 Werte NACH `L3_huffman`,
    /// **vor** Umordnung, Stereo und Alias — und, GEMESSEN, **vor**
    /// `L3_reorder` auch dann, wenn minimp3 keine Umordnung macht.
    ///
    /// # WARUM DIESES FELD ES GIBT, und es ist der entscheidende Fund
    ///
    /// GEMESSEN aus minimp3s Header (die Schleife am Ende von
    /// `L3_decode`):
    ///
    /// ```c
    /// for (ch = 0; ch < nch; ch++, gr_info++)
    /// {
    ///     int aa_bands = 31;
    ///     if (gr_info->n_short_sfb)
    ///     {
    ///         aa_bands = n_long_bands - 1;
    ///         L3_reorder(s->grbuf[ch] + n_long_bands*18, ...);
    ///     }
    ///     L3_antialias(s->grbuf[ch], aa_bands);
    ///     L3_imdct_gr(...);
    ///     L3_change_sign(s->grbuf[ch]);
    /// }
    /// ```
    ///
    /// **`L3_reorder` laeuft NUR bei Kurzbloecken.** Bei einem
    /// Granul **ohne** Kurzbloecke bleibt `grbuf` also in der
    /// Huffman-Reihenfolge stehen.
    ///
    /// GEMESSENER Gegensatz: **meine** `ordne_um` laeuft **immer**
    /// (`granel.rs`, Zeile 569), ohne jede Bedingung.
    ///
    /// **Das ist kein Detail, sondern eine andere Reihenfolge**, und es
    /// erklaert, warum `g8c_sb.txt` zu **keinem** `vor_synthese` passte:
    /// der Granul ist ein **Langblock**, und in minimp3 wurde gar nicht
    /// umgeordnet. Es ist also `vor_umordnung` + Alias + `change_sign`.
    pub vor_umordnung: Vec<[f64; granel::LINIEN]>,
    /// Je (Rahmen, Granul, Kanal): die 576 Werte an der Grenze zur
    /// Synthese — nach Umordnung, Stereo und Aliasreduktion.
    pub vor_synthese: Vec<[f64; granel::LINIEN]>,
    /// Je (Rahmen, Granul, Kanal): die 40 Skalenfaktoren.
    pub skalen: Vec<[u8; 40]>,
    /// Je (Rahmen, Granul, Kanal): die 576 Werte **am Eingang des
    /// Polyphasefilters** — nach IMDCT, Fenster, Ueberlappung und
    /// Frequenzinversion.
    ///
    /// GEMESSENER Ort in minimp3 (Header): die Messstelle `g_dump_sb`
    /// liegt zwischen `L3_decode` (Zeile 1959, endet mit `L3_imdct_gr`
    /// und `L3_change_sign`) und `mp3d_synth_granule` (Zeile 1972). Das
    /// ist **der** Ort von `g8c_sb.txt` — und es ist die Stufe, die in
    /// G8g fehlte: dort passte die Datei zu **keinem** der vier Felder
    /// (`vor_synthese`, `vor_umordnung`, je mit und ohne
    /// `change_sign`), alle mit der besten Abweichung 1,000e+00.
    pub vor_polyphase: Vec<[f64; granel::LINIEN]>,
}

impl Messung {
    /// Die Zahl der gesammelten Granulenstufen.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nach_huffman.len()
    }

    /// GEMESSENE Kontrolle: alle vier Felder muessen gleich lang sein.
    #[must_use]
    pub fn ist_vollstaendig(&self) -> bool {
        let n = self.nach_huffman.len();
        self.vor_umordnung.len() == n
            && self.vor_synthese.len() == n
            && self.vor_polyphase.len() == n
            && self.skalen.len() == n
    }

    /// `true`, wenn nichts gesammelt wurde.
    ///
    /// GEMESSENE Grundlage der Warnung, die clippy hier meldete:
    /// `len()` ohne `is_empty()` ist eine unvollstaendige Länge-API. Hier
    /// heisst „leer" ausdruecklich: **keine einzige Granulenstufe**.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nach_huffman.is_empty()
    }
}

/// Dekodiert eine ganze MP3-Datei.
///
/// # DER WEG, UND DIE REIHENFOLGE IST DIE NORM
///
/// ```text
///   1. ID3v2 ueberspringen, die Rahmen finden          (Rahmenebene)
///   2. den gesamten Hauptstrom bauen                    (Rahmenebene)
///   3. je Granul: Skalenfaktoren, Huffman, Requanti-
///      sierung, Reihenfolge, Stereo, Alias             (Granulebene)
///   4. Synthese: MDCT, Fenster, Ueberlappung,
///      Frequenzinversion                              (Synthese)
///   5. Polyphasefilter                                (Synthese)
/// ```
///
/// Schritt 3 endet mit der **Aliasreduktion**, und die gehoert **vor**
/// der Synthese: sie sitzt zwischen den Spektralwerten und dem Fenster.
/// Schritt 3 enthaelt ausserdem die **MS-Entkopplung**, und die ist
/// bei den beiden Stereo-Testdateien der einzige Weg, auf dem Kanal 1
/// ueberhaupt entsteht (siehe `tests/g8c_granule.rs`).
///
/// # WAS BEI FEHLENDEN DATEN PASSIERT
///
/// Die ersten und die letzten Granulen einer MP3-Datei sind nicht
/// vollstaendig abgespeichert — der Encoder legt die Reserve am Anfang
/// an. Fehlende Bit liefern **Nullen**, nicht einen Fehler: der Ton
/// beginnt und endet mit einer Rampe, und das ist der Normalfall.
pub fn dekodiere(daten: &[u8]) -> Ergebnis<Ton> {
    dekodiere_mess(daten, None)
}

/// Dekodiert eine ganze MP3-Datei und sammelt dabei die Granulenebene.
///
/// GEMESSENER Grund fuer die Trennung: [`dekodiere`] ist der
/// Produktweg und darf nichts sammeln — das Kosten die Datei von
/// `tests/daten/` geprueft hat. Und die Sammlung darf nicht **zweite**
/// Schleife sein, weil eine zweite Schleife ein Duplikat waere (siehe
/// [`Messung`]).
///
/// GEMESSENER Hinweis fuer den Aufrufer: `sammel` wird **an** das
/// vorhandene Objekt angehaengt, nicht ueberschrieben. Wer eine
/// Messung in zwei Schritten sammelt, behaelt damit beide Teile.
pub fn dekodiere_mess(daten: &[u8], sammel: Option<&mut Messung>) -> Ergebnis<Ton> {
    dekodiere_mess_mit_fenster(daten, sammel, Fensterart::Berechnet)
}

/// [`dekodiere_mess`] mit **benannter** Fenstertabelle (G9m).
///
/// # WARUM DER ERSATZ, UND WARUM KEIN ZWEITER DUKODIERER
///
/// GEMESSEN: minimp3 traegt die Sinuswerte als **Dezimal-Literal** mit
/// `f`-Suffix ein (`orakel/minimp3_mess.h:1344`), ich **berechne** sie.
/// GEMESSENER Unterschied: **1 ulp** je Fensterwert, und das sind nach der
/// Multiplikation mit einem Sample **`1,1e-08`** — genau die groesste
/// **absolute** Abweichung aus G8j.
///
/// **REGEL aus G8j:** *lieber eine Funktion mit optionalem Ausgang als eine
/// zweite Schleife.* Und Regel aus G8f: die Sammlung muss **wissen**, mit
/// welchem Weg sie gerechnet hat — sonst ist ihre Zahl nicht deutbar.
/// Also: **ein** Aufruf mit einem **Schalter**, und das Schalterergebnis
/// steht in [`Messung::fensterart`].
pub fn dekodiere_mess_mit_fenster(
    daten: &[u8],
    mut sammel: Option<&mut Messung>,
    fensterart: Fensterart,
) -> Ergebnis<Ton> {
    let fenstervorrat: Option<&'static [[f64; 36]; 4]> = match fensterart {
        Fensterart::Berechnet => None,
        Fensterart::F32 => Some(crate::mp3::synthese::fenster_f32()),
    };
    if let Some(m) = sammel.as_deref_mut() {
        m.fensterart = fensterart;
    }
    let rahmen = rahmen_liste(daten)?;
    if rahmen.is_empty() {
        return Err(crate::fehler::Fehler::neu(
            crate::fehler::Art::FalscherWert,
            "kein einziger MP3-Rahmen gefunden",
        ));
    }
    let kopf0 = &rahmen[0].kopf;
    if kopf0.rate_index > 8 {
        return Err(crate::fehler::Fehler::neu(
            crate::fehler::Art::FalscherWert,
            format!(
                "Abtastrate-Index {} oder Kanalzahl unbrauchbar",
                kopf0.rate_index
            ),
        ));
    }
    // Die Kanalzahl steht im Kanalmodus: `Mono` heisst 1, alles andere 2.
    let kan = match kopf0.art {
        rahmen::Kanalart::Mono => 1,
        _ => 2,
    };
    let granulen = if kopf0.mpeg == 1 { 2 } else { 1 };

    // Schritt 2: der gesamte Hauptstrom, mit dem Startbyte je Rahmen.
    let mut strom: Vec<u8> = Vec::with_capacity(daten.len() / 4);
    let mut start = Vec::with_capacity(rahmen.len());
    for f in &rahmen {
        start.push(strom.len());
        let von = f.bei + f.kopf.neben_start + f.kopf.neben_laenge;
        strom.extend_from_slice(&daten[von..f.bei + f.kopf.laenge]);
    }

    let dek = granel::Dekoder::neu(kan);
    let mut synth: Vec<Synthese> = (0..kan).map(|_| Synthese::neu()).collect();
    let mut out: Vec<Vec<f64>> = vec![Vec::new(); kan];
    let mut skal_alt = vec![[0u8; 40]; kan];

    for (n, f) in rahmen.iter().enumerate() {
        let basis = start[n].saturating_sub(f.neben.haupt_beginn as usize);
        let mut versatz = 0usize;
        for gr in 0..granulen {
            // Erst beide Kanaele lesen, dann entkoppeln: die
            // MS-Entkopplung rechnet Kanal 0 **und** Kanal 1 desselben
            // Granules, und ein Zwischenstand reicht nicht.
            let mut spektren = Vec::with_capacity(kan);
            for (kanal, alt_slot) in skal_alt.iter_mut().enumerate().take(kan) {
                let g = &f.neben.granel[kanal][gr];
                // GEMESSENER FEHLER, und er ist eine vermischte Einheit:
                // `basis` ist ein **Byte**-Versatz in `strom`, `versatz`
                // dagegen ein **Bit**-Versatz — er sammelt `teil23`, und
                // das ist `part2_3_length`, in **Bit** (ISO 11172-3
                // Abschnitt 2.4.2.7; bestaetigt an minimp3s
                // `layer3gr_limit = s->bs.pos + gr_info[ch].part_23_length`,
                // wobei `s->bs.pos` in **Bit** gezaehlt wird).
                //
                //     alt:  let von_b = (basis + versatz) * 8;
                //     neu:  let von_b = basis * 8 + versatz;
                //
                // GEMESSENE FOLGE: Granul 0 jedes Rahmens war richtig
                // (rho = +1,000000000), weil `versatz` dort 0 ist. Ab
                // Granul 1 wurde der **ganze** Granul an Position
                // `basis*8 + teil23*8` statt `basis*8 + teil23` gelesen —
                // bei `teil23 = 3281` also um 22967 Bit zu spaet, und
                // `bis_roh` entsprechend zu weit, sodass die
                // End-of-packet-Klammer den Fehler nicht bemerkte.
                //
                // GEMESSENE BESTAETIGUNG an `g8c_rauschen_mono`, Rahmen 3,
                // Kanal 0 (minimp3s `pos.txt` gegen meine Messstelle):
                //
                //   Granul 6: minimp3 von=0    bis=3281  -> 34056 / 37337
                //   Granul 7: minimp3 von=3281 bis=7049  -> 37337 / 41105
                //
                // und der Fehler war so genau zu sehen, dass er sich in
                // den **Skalenfaktoren** zeigte: `skfsi` war an den Bandern
                // 8 und 9 um eins zu hoch (Exponent -2 statt -4 an genau
                // zwei von 21 Baendern) — bei `slen1 = 1` genau ein Bit
                // an genau einer Stelle.
                let von_b = basis * 8 + versatz;
                let bis_b = von_b + g.teil23 as usize;
                versatz += g.teil23 as usize;
                let fenster = granel::Fenster {
                    daten: &strom,
                    von: von_b,
                    bis: bis_b,
                };
                // `scfsi` zeigt auf Granul 0 **desselben** Rahmens und
                // Kanals: der alte Wert muss vor dem Lesen da sein und
                // wird danach ersetzt.
                let gelesen = granel::lies_granule(
                    &dek,
                    gr,
                    g,
                    &f.kopf,
                    f.neben.skfsi[kanal],
                    fenster,
                    if gr == 1 { Some(&*alt_slot) } else { None },
                )?;
                let sk = gelesen.skfsi;
                let spek = gelesen.nach_umordnung;
                *alt_slot = sk;
                // GEMESSENER Ort dieser Sammlung: **vor** Umordnung,
                // Aliasreduktion und Stereo, also genau dort, wo
                // `g8c_huff.txt` abgelesen wurde (Block 3 der
                // Herkunftsdatei: „NACH `L3_huffman`, vor
                // `L3_reorder` und vor `L3_antialias`").
                //
                // GEMESSENER FUND: meine erste Sammelstelle lag
                // **hinter** `ordne_um`, und `g8g_genau_ein_granul_
                // passt_zu_g8c_huff` meldete 0 von 116. Darum steht
                // hier `gelesen.vor_umordnung` und nicht `spek`.
                if let Some(m) = sammel.as_deref_mut() {
                    m.vor_umordnung.push(gelesen.vor_umordnung);
                    m.nach_huffman.push(gelesen.vor_umordnung);
                    m.skalen.push(sk);
                }
                spektren.push(spek);
            }
            // Die Stereo-Arbeit.
            if kan == 2 {
                let mut k0 = spektren[0];
                let mut k1 = spektren[1];
                granel::stereo(&f.kopf, &mut k0, &mut k1);
                spektren = vec![k0, k1];
            }
            // Aliasreduktion je Kanal, dann die Synthese.
            for kanal in 0..kan {
                // Die Aliasreduktion gehoert **vor** der Synthese und
                // **nach** dem Stereo: sie sitzt zwischen den Spektral-
                // werten und dem Fenster. Andernfalls kaeme der
                // Faktor acht pro Band zweimal hinein.
                let mut spek = spektren[kanal];
                granel::alias_reduziere(&mut spek, &f.neben.granel[kanal][gr]);
                // GEMESSENER Ort: **nach** Aliasreduktion, **vor** der
                // Synthese. Genau dort steht `g8c_sb.txt` (Block 5:
                // „NACH `L3_decode`, also vor `mp3d_DCT_II`").
                if let Some(m) = sammel.as_deref_mut() {
                    m.vor_synthese.push(spek);
                }
                // GEMESSENER Ort: das ist **der Eingang des
                // Polyphasefilters** — also der Ort von `g8c_sb.txt`
                // (Header Zeile 1965, zwischen `L3_decode` und
                // `mp3d_synth_granule`).
                let mut vor_poly = [0.0f64; granel::LINIEN];
                let samples = synth[kanal].granule_mess_mit_fenster(
                    &spek,
                    &f.neben.granel[kanal][gr],
                    f.kopf.rate_index,
                    Some(&mut vor_poly),
                    fenstervorrat,
                );
                if let Some(m) = sammel.as_deref_mut() {
                    m.vor_polyphase.push(vor_poly);
                }
                // 18 Schritte zu 32 Samples ergeben 576.
                for t in 0..LINIEN_PRO_BAND {
                    let mut sb = [0.0f64; BAENDER];
                    for band in 0..BAENDER {
                        sb[band] = samples[band * LINIEN_PRO_BAND + t];
                    }
                    out[kanal].extend_from_slice(&synth[kanal].band_werte_aus(&sb));
                }
            }
        }
    }
    let samples = out[0].len();
    Ok(Ton {
        rate: kopf0.rate,
        kanaele: kan,
        samples,
        kanaele_daten: out,
    })
}
