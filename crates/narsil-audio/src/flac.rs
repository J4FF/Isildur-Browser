//! G9b: DER FLAC-DEKODER.
//!
//! # BITGLEICH MIT FFMPEG, UND WARUM DAS HIER EINE STARKE FORDERUNG IST
//!
//! FLAC ist **verlustfrei**. Das heisst: es gibt fuer jede Ausgabe genau
//! ein richtiges Ergebnis, und `ffmpeg -i x.flac -f s16le` ist nicht
//! eine Naeherung, sondern die **Referenz**. Fuer WAV gilt dasselbe, und
//! dort habe ich 0 Abweichungen verlangt statt 60 dB; hier gilt nichts
//! anderes.
//!
//! # WIE EIN FLAC-RAHMEN AUSSIEHT, und warum die Reihenfolge zwingend ist
//!
//! ```text
//!   "fLaC"
//!   METADATA: STREAMINFO (34 Byte, immer zuerst), danach beliebige
//!             Bloecke (VOR comment, PADDING, SEEKTABLE, …)
//!   RAHMEN 1:
//!     Kopf      14 Bit Sync, 1 reserviert, 1 Blocking-Strategie,
//!               4 Blockgroesse, 4 Abtastrate, 4 Kanalzuordnung,
//!               3 Bittiefe, 1 reserviert, UTF-8-Zahl, Extras, CRC-8
//!     Subframes je Kanal: 1 Fuellbit, 6 Typ, 1 Wasted-Flag,
//!               dann CONSTANT | VERBATIM | FIXED | LPC
//!     Fuellbits bis Bytegrenze, dann CRC-16 ueber den ganzen Rahmen
//!   RAHMEN 2 …
//! ```
//!
//! Die **Reihenfolge ist zwingend**, weil jeder Schritt die Bit des
//! naechsten frisst. Besonders die Subframes: der Typ steht in den
//! **letzten** 6 Bit vor dem Wasted-Flag, und das Wasted-Flag gilt fuer
//! den ganzen Subframe — es verschiebt die Bittiefe **vor** dem Lesen
//! der Werte.
//!
//! # DIE ACHT STELLEN, AN DENEN ES SCHIEFGEHT
//!
//! 1. **Die Bittiefe haengt von der Kanalzuordnung ab.** Bei
//!    `LEFT/SIDE` hat der **zweite** Kanal `bps + 1`, bei `MID/SIDE`
//!    derselbe. Wer das uebersieht, liest ab dem zweiten Kanal alles ab
//!    der falschen Stelle und bekommt Rauschen.
//! 2. **Die Kanalzuordnung 0..7 bedeutet „so viele unabhaengige
//!    Kanaele"**, nicht „Kanal 0 bis 7". Bei 8/9/10 kommen LEFT/SIDE,
//!    RIGHT/SIDE und MID/SIDE. Wer 0..7 als Nummern liest, verwechselt
//!    Mono mit Stereo.
//! 3. **`MID/SIDE` rechnet `a -= b >> 1`** — mit **arithmetischem**
//!    Schift auf einem **signed** `b`. Mit logischem Schift wird jeder
//!    negierte Winkelkanal um eins daneben.
//! 4. **Die FIXED-Praediktoren sind keine Formel, sondern eine Kette.**
//!    `a += b += c += d += ...` mit `b = a - decoded[p-2]` **einmal**
//!    vorher gebildet. Wer sie ausrechnet statt zu akkumulieren,
//!    bekommt bei den Rahmen, in denen `p > 1` ist, ein anderes Ergebnis.
//! 5. **Die LPC-Koeffizienten kommen **umgekehrt**.** `coeffs[p-1-i]`
//!    wird gelesen, und `qlevel` ist **negativ** (`sbits(5)`). Wer das
//!    Vorzeichen nicht nimmt, verschiebt das Ergebnis um einen Faktor
//!    `2^|qlevel|`.
//! 6. **Die Residuum-Escape-Sequenz** ist ein Rice-Parameter aus lauter
//!    Einsen, und dahinter stehen **5 Bit Bitbreite** und dann die
//!    Werte **ohne** Vorzeichen-Rice. Wer den Escape nicht erkennt,
//!    liest die ganze Datei falsch.
//! 7. **`get_unary` fuer die Wasted-Bits** zaehlt Nullen und addiert 1.
//!    `wasted = 1 + unaries`.
//! 8. **Die CRC pruefen.** CRC-8 ueber den Kopf, CRC-16 ueber den
//!    ganzen Rahmen inklusive Kopf. Ohne sie meldet ein Dekoder bei
//!    verschobener Leseposition **keinen** Fehler, sondern Ton.
//!
//! # WAS NICHT GEBAUT IST
//!
//! * **Ogg-FLAC**: der Container ist nicht gebaut. `dekodiere` erwartet
//!   eine **nackte** FLAC-Datei, die mit `fLaC` beginnt. Fuer Ogg waere
//!   `ogg.rs` zu aendern; das ist als offener Punkt notiert, und
//!   `dekodiere_ogg` gibt es **nicht** — es gibt einen benannten Fehler,
//!   wenn die Datei mit `OggS` beginnt.
//! * **32-Bit-PCM**: `bps = 32` wird gelesen und verarbeitet, aber die
//!   Ausgabe nach `i16` verliert die unteren 16 Bit. ffmpeg mit
//!   `-f s16le` tut dasselbe, deshalb ist die Abnahme gültig.

use crate::bitleser::BitLeserVorn;
use crate::fehler::{Art, Ergebnis, Fehler};
use crate::ton::Ton;

/// Die Magic am Anfang einer nackten FLAC-Datei.
pub const MAGIC: &[u8; 4] = b"fLaC";

/// Die Blockgroessen zu den Codes 1..5 und 8..15.
///
/// Belegt an `libavcodec/flacdata.c`, `ff_flac_blocksize_table`:
/// ```c
/// { 0, 192, 576<<0, 576<<1, 576<<2, 576<<3, 0, 0,
///   256<<0, 256<<1, 256<<2, 256<<3, 256<<4, 256<<5, 256<<6, 256<<7 }
/// ```
pub const BLOCKGROESSE: [u32; 16] = [
    0, 192, 576, 1152, 2304, 4608, 0, 0, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768,
];

/// Die Abtastraten zu den Codes 0..11.
///
/// Belegt an `libavcodec/flacdata.c`, `ff_flac_sample_rate_table`.
pub const ABRATEN: [u32; 16] = [
    0, 88200, 176400, 192000, 8000, 16000, 22050, 24000, 32000, 44100, 48000, 96000, 0, 0, 0, 0,
];

/// Die Bittiefen zu den Codes 1..7. Code 0 heisst „aus STREAMINFO".
///
/// Belegt an `libavcodec/flac.c`, `sample_size_table`:
/// ```c
/// static const int8_t sample_size_table[] = { 0, 8, 12, 0, 16, 20, 24, 32 };
/// ```
/// **Code 3 ist 0 und damit ungueltig** — es steht in der Tabelle, aber
/// ffmpeg lehnt es ab. Wer die Tabelle naemlich liest, hat bei Code 3
/// eine Bittiefe von 0 und produziert lautlosen Muell.
pub const BITTIEFE: [u16; 8] = [0, 8, 12, 0, 16, 20, 24, 32];

/// Was `STREAMINFO` sagt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Streaminfo {
    /// Die kleinste Blockgroesse der Datei.
    pub min_blockgroesse: u16,
    /// Die groesste Blockgroesse der Datei.
    pub max_blockgroesse: u16,
    /// Die kleinste Rahmenlaenge oder 0, wenn unbekannt.
    pub min_rahmen: u32,
    /// Die groesste Rahmenlaenge oder 0, wenn unbekannt.
    pub max_rahmen: u32,
    /// Die Abtastrate in Hz.
    pub rate: u32,
    /// Die Kanalzahl.
    pub kanaele: u8,
    /// Die Bittiefe.
    pub bit: u16,
    /// Die Zahl der Samples in der ganzen Datei, oder 0.
    pub samples: u64,
}

impl Streaminfo {
    /// Die Framesize in Byte, wenn `min_rahmen` und `max_rahmen` gleich
    /// sind — das ist der Fall bei fester Blockgroesse.
    #[must_use]
    pub fn feste_rahmenlaenge(&self) -> Option<usize> {
        if self.min_rahmen != 0 && self.min_rahmen == self.max_rahmen {
            Some(self.min_rahmen as usize)
        } else {
            None
        }
    }
}

/// Die 34 Byte `STREAMINFO`, als Bitstrom gelesen.
///
/// Belegt an `libavcodec/flac.c`, `ff_flac_parse_streaminfo`:
/// ```c
/// skip_bits(&gb, 16);              /* min blocksize   */
/// s->max_blocksize = get_bits(&gb, 16);
/// skip_bits(&gb, 24);              /* min frame size  */
/// s->max_framesize = get_bits(&gb, 24);
/// s->samplerate    = get_bits(&gb, 20);
/// s->channels      = get_bits(&gb, 3) + 1;
/// s->bps           = get_bits(&gb, 5) + 1;
/// s->samples       = get_bits64(&gb, 36);
/// skip_bits_long(&gb, 128);        /* md5             */
/// ```
///
/// # DIE FELDZAHL, und sie ist nicht 34 round
///
/// 16 + 16 + 24 + 24 + 20 + 3 + 5 + 36 + 128 = **272** Bit = 34 Byte ✓.
/// Und `channels` und `bps` stehen als **Minus eins**, also 0 und 1
/// heissen 1 Kanal und 1 Bit — was ein Decoder ohne den Versatz als
/// „0 Kanaele" liest und sich dann wundert, warum die Datei stumm ist.
pub fn lies_streaminfo(daten: &[u8]) -> Ergebnis<Streaminfo> {
    // # ACHT BYTE VORHER, und der Fehler ist nicht klein
    //
    // `STREAMINFO` beginnt **nicht** bei Byte 0, sondern bei Byte 8:
    // vier Byte `fLaC` und vier Byte Blockkopf. Belegt an
    // `libavcodec/flacdec.c`:
    //
    // ```c
    // flac_parse_block_header(&buf[4], NULL, &metadata_type, &metadata_size);
    // ret = ff_flac_parse_streaminfo(s->avctx, &s->stream_info, &buf[8]);
    // ```
    //
    // GEMESSENE FOLGE des Weglassens: der 4-Byte-Blockkopf
    // (`0x00 0x00 0x00 0x22`) kam als Datenlage gelesen, und die
    // Bittiefe kam als **1** heraus — `0x22` war die Bytezahl, und
    // `channels`/`bps` lagen danach an voellig anderer Stelle. Der
    // Fehler meldete sich als „Bittiefe 1 ist kleiner als 4", also als
    // **Gueltigkeitspruefung**, obwohl der Leser falsch stand.
    const VORHER: usize = 8;
    if daten.len() < VORHER + 34 {
        return Err(Fehler::neu(
            Art::StromZuEnde,
            format!(
                "FLAC: STREAMINFO braucht 34 Byte ab Byte {VORHER}, da sind {}",
                daten.len().saturating_sub(VORHER)
            ),
        ));
    }
    if &daten[0..4] != MAGIC {
        return Err(Fehler::neu(
            Art::FalscherWert,
            if daten.starts_with(b"OggS") {
                "FLAC: die Datei ist **Ogg-FLAC**; der Ogg-Weg ist nicht gebaut"
            } else {
                "FLAC: die Datei beginnt nicht mit fLaC"
            },
        ));
    }
    let mut l = BitLeserVorn::ab(daten, VORHER);
    let min_blockgroesse = l.bits(16).map_err(kurz)? as u16;
    let max_blockgroesse = l.bits(16).map_err(kurz)? as u16;
    let min_rahmen = l.bits(24).map_err(kurz)?;
    let max_rahmen = l.bits(24).map_err(kurz)?;
    let rate = l.bits(20).map_err(kurz)?;
    let kanaele = (l.bits(3).map_err(kurz)? + 1) as u8;
    let bit = (l.bits(5).map_err(kurz)? + 1) as u16;
    let mut samples = 0u64;
    for _ in 0..36 {
        samples = (samples << 1) | u64::from(l.bit().map_err(kurz)?);
    }
    if bit < 4 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("FLAC: Bittiefe {bit} ist kleiner als 4"),
        ));
    }
    if kanaele == 0 || kanaele > 8 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("FLAC: {kanaele} Kanaele"),
        ));
    }
    Ok(Streaminfo {
        min_blockgroesse,
        max_blockgroesse,
        min_rahmen,
        max_rahmen,
        rate,
        kanaele,
        bit,
        samples,
    })
}

fn kurz(e: crate::bitleser::BitFehler) -> Fehler {
    e.into()
}

/// Die Metadatenbloecke durchgehen und die Position nach dem letzten
/// liefern.
///
/// # DIE VIER BYTE, und warum das Hochbyte ein Flag ist
///
/// ```text
///   Bit 7     : letzter Block
///   Bit 6..0  : Typ (STREAMINFO = 0, PADDING = 1, …)
///   Bit 23..0 : Laenge, ** big endian **
/// ```
///
/// Die Laenge ist big endian, der Rest des Rests big endian — die
/// **gesamte** Metadatenstruktur ist big endian, waehrend die Rahmen
/// danach **MSB-first serialisiert** sind. Das ist die Stelle, an der
/// man mit einem little-endian-Leser hineinfaellt und dann einen
/// `STREAMINFO` mit Blockgroesse 256 statt 4096 liest.
pub fn metadaten_ueberspringen(daten: &[u8]) -> Ergebnis<usize> {
    let mut ab = 4usize;
    if daten.len() < 4 || &daten[0..4] != MAGIC {
        return Err(Fehler::neu(
            Art::FalscherWert,
            if daten.starts_with(b"OggS") {
                "FLAC: die Datei ist **Ogg-FLAC**; der Ogg-Weg ist nicht gebaut"
            } else {
                "FLAC: die Datei beginnt nicht mit fLaC"
            },
        ));
    }
    loop {
        if ab + 4 > daten.len() {
            return Err(Fehler::neu(
                Art::StromZuEnde,
                "FLAC: Metadaten enden mitten im Block",
            ));
        }
        let letzter = daten[ab] & 0x80 != 0;
        let laenge = (usize::from(daten[ab + 1]) << 16)
            | (usize::from(daten[ab + 2]) << 8)
            | usize::from(daten[ab + 3]);
        ab += 4 + laenge;
        if ab > daten.len() {
            return Err(Fehler::neu(
                Art::StromZuEnde,
                "FLAC: Metadatenblock laeuft ueber das Ende",
            ));
        }
        if letzter {
            return Ok(ab);
        }
    }
}

/// CRC-8 ueber Bytes, Polynom `x^8 + x^2 + x + 1` (0x07), Start 0.
///
/// Belegt an der Framespezifikation: „CRC-8, polynomial x^8 + x^2 +
/// x + 1, initial 0".
#[must_use]
pub fn crc8(daten: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &b in daten {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// CRC-16 ueber Bytes, Polynom `x^16 + x^15 + x^2 + 1` (0x8005), Start 0.
#[must_use]
pub fn crc16(daten: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &b in daten {
        crc ^= u16::from(b) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x8005
            } else {
                crc << 1
            };
        }
    }
    crc
}
/// Der Kopf eines FLAC-Rahmens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RahmenKopf {
    /// Die Blockgroesse dieses Rahmens in Samples.
    pub blockgroesse: u32,
    /// Die Abtastrate; 0 heisst „aus STREAMINFO".
    pub rate: u32,
    /// Die Kanalzahl.
    pub kanaele: usize,
    /// Die Kanalzuordnung 0..10 (0..7 = unabhaengig, 8/9/10 = Stereo-Artefakte).
    pub zuordnung: u8,
    /// Die Bittiefe; 0 heisst „aus STREAMINFO".
    pub bit: u16,
    /// Die Frame- oder Sample-Nummer (UTF-8-kodiert).
    pub nummer: u64,
    /// `true`, wenn die Nummer die **Sample**-Position ist (feste Blockgroesse).
    pub feste_blockgroesse: bool,
}

/// Liest die UTF-8-kodierte Zahl aus dem Rahmenkopf.
///
/// # WARUM UTF-8 UND NICHT EINE FESTE BREITE
///
/// Weil die Zahl bis 36 Bit reicht und FLAC keine-padding schreibt.
/// Das ist die **einzige** Stelle in FLAC, an der UTF-8 vorkommt, und
/// genau deshalb liest man sie nicht mit einem UTF-8-Leser: die
/// Bitbreite jeder Folge steht in den **ersten beiden** Bit, und der
/// Rest sind die Nutzbits.
fn lies_utf8(l: &mut BitLeserVorn<'_>) -> Ergebnis<u64> {
    let erstes = l.bits(8).map_err(kurz)? as u8;
    let laenge = match erstes {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        0xF8..=0xFB => 5,
        0xFC..=0xFD => 6,
        0xFE => 7,
        _ => {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!("FLAC: unmoegliche UTF-8-Folge {erstes:#04X} im Rahmenkopf"),
            ));
        }
    };
    let mut wert = if laenge == 1 {
        u64::from(erstes)
    } else {
        u64::from(erstes) & ((1u64 << (8 - laenge)) - 1)
    };
    for _ in 1..laenge {
        let b = l.bits(8).map_err(kurz)? as u8;
        if b & 0xC0 != 0x80 {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!("FLAC: UTF-8-Folge bricht ab: {b:#04X} ist kein 10xxxxxx"),
            ));
        }
        wert = (wert << 6) | u64::from(b & 0x3F);
    }
    Ok(wert)
}

/// Liest den Rahmenkopf und prueft die CRC-8.
///
/// Liefert Kopf **und** die Byte-Position, an der die Subframes
/// beginnen.
///
/// # DIE CRC-8 IST NICHT OPTIONAL
///
/// Sie kostet acht Zeilen und sie ist der **einzige** Test, der
/// mitteilt, ob die Feldliste stimmt. Alle sieben Felder des Kopfs
/// (Sync, reserviert, Blocking, Blockgroesse, Rate, Zuordnung, Bittiefe,
/// reserviert, Nummer, Extras) muessen **an der richtigen Stelle**
/// stehen, damit die Pruefsumme stimmt; steht eines davon eine Bit zu
/// spaet, ist die CRC sofort falsch — und ohne sie haette der Decoder
/// stillschweigend weitergeparst.
pub fn lies_rahmenkopf(
    daten: &[u8],
    l: &mut BitLeserVorn<'_>,
    si: &Streaminfo,
) -> Ergebnis<RahmenKopf> {
    let start_byte = l.position() / 8;
    let sync = l.bits(14).map_err(kurz)?;
    if sync != 0x3FFE {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("FLAC: Sync {sync:#06X} statt 0x3FFE"),
        ));
    }
    if l.bit().map_err(kurz)? != 0 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            "FLAC: reserviertes Bit nach dem Sync gesetzt",
        ));
    }
    let feste_blockgroesse = l.bit().map_err(kurz)? == 1;
    let bs_code = l.bits(4).map_err(kurz)? as usize;
    let sr_code = l.bits(4).map_err(kurz)? as usize;
    let zuordnung = l.bits(4).map_err(kurz)? as u8;
    let bps_code = l.bits(3).map_err(kurz)? as usize;

    // # DIE ZUORDNUNG 0..7 HEISST „SO VIELE KANAELE", NICHT „KANAL 0 BIS 7"
    //
    // GEMESSENER FALL, und er ist der klassische: ich hatte die vier
    // Bits als Kanalnummer gelesen. Bei `Mono` steht aber **0** dort,
    // also waere Kanal 0 gemeint und Kanal 1 nicht vorhanden — die Datei
    // klingt dann nach links verschoben, mit jedem zweiten Sample
    // stumm.
    let (kanaele, zuordnung) = if zuordnung < 8 {
        (usize::from(zuordnung) + 1, 0u8)
    } else if zuordnung < 11 {
        (2usize, zuordnung)
    } else {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("FLAC: Kanalzuordnung {zuordnung} gibt es nicht"),
        ));
    };
    // # CODE 3 IST UNGUELTIG, OBWOHL ES IN DER TABELLE STEHT
    //
    // `sample_size_table = { 0, 8, 12, 0, 16, 20, 24, 32 }` — die
    // **zweite** Null steht an Index 3. Wer die Tabelle naemlich liest,
    // hat dort eine Bittiefe von 0 und produziert lautlosen Muell.
    if bps_code == 3 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            "FLAC: Bittiefen-Code 3 ist ungueltig (die Tabelle hat dort eine 0)",
        ));
    }
    let mut bit = BITTIEFE[bps_code];
    if bps_code == 0 {
        bit = si.bit;
    }
    if l.bit().map_err(kurz)? != 0 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            "FLAC: zweites reserviertes Bit gesetzt",
        ));
    }
    let nummer = lies_utf8(l)?;

    // Die Extras kommen **nach** der Nummer, nicht davor. Das ist die
    // Reihenfolge, an der man sich verliest: sie sind namensgleich mit
    // den Feldern, aus denen sie kommen, stehen aber spaeter.
    let blockgroesse: u32 = match bs_code {
        0 => {
            return Err(Fehler::neu(
                Art::FalscherWert,
                "FLAC: Blockgroessen-Code 0 ist reserviert",
            ));
        }
        6 => l.bits(8).map_err(kurz)? + 1,
        7 => l.bits(16).map_err(kurz)? + 1,
        k => BLOCKGROESSE[k],
    };
    let mut rate: u32 = match sr_code {
        0..=11 => ABRATEN[sr_code],
        12 => l.bits(8).map_err(kurz)? * 1000,
        13 => l.bits(16).map_err(kurz)?,
        14 => l.bits(16).map_err(kurz)? * 10,
        _ => {
            return Err(Fehler::neu(
                Art::FalscherWert,
                "FLAC: Abtastraten-Code 15 ist ungueltig",
            ));
        }
    };
    if rate == 0 {
        rate = si.rate;
    }

    // Die CRC-8 ueber **alle Kopfbytes bis zur CRC**.
    //
    // GEMESSENER ENTSCHEIDUNG: die Pruefsumme wird gegen `daten`
    // gerechnet, nicht aus dem Leser rekonstruiert. Sie muss **null**
    // sein, denn die gespeicherte CRC macht die Gesamtsumme null —
    // das ist die Form, in der die Framespezifikation es angibt.
    l.auf_bytegrenze();
    let crc_bis = l.byte_position();
    let mut crc = 0u8;
    for &b in &daten[start_byte..crc_bis] {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    let gespeichert = l.bits(8).map_err(kurz)? as u8;
    if crc != gespeichert {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!(
                "FLAC: CRC-8 des Kopfs {crc:#04X} statt {gespeichert:#04X} — \
                 die Feldliste steht nicht da, wo sie stehen soll"
            ),
        ));
    }

    Ok(RahmenKopf {
        blockgroesse,
        rate,
        kanaele,
        zuordnung,
        bit,
        nummer,
        feste_blockgroesse,
    })
}

/// Die Reste eines Subframes, ueber die Partitionen des Rice-Parameters.
///
/// # DIE ZICKZACK-UMKEHRUNG, und sie ist der ganze Trick
///
/// Rice kodiert eine **vorzeichenbehaftete** Zahl so:
///
/// ```text
///   u = (v << 1) ^ (v >> 63)      ← Zigzag: v >= 0 -> 2v,  v < 0 -> -2v-1
///   q = u >> k
///   r = u & (2^k − 1)
///   im Strom: q Nullen, dann eine 1, dann r als k Bit
/// ```
///
/// Zurueck also:
///
/// ```text
///   u = (q << k) | r
///   v = (u >> 1) ^ -(u & 1)
/// ```
///
/// # MEINE ERSTE FORMEL WAR FALSCH, und sie war plausibel
///
/// Ich hatte `(q << 1) | 1) − r` geschrieben — das sieht nach einer
/// Vorzeichenbehandlung aus und ist es auch, aber an der **falschen**
/// Stelle: `q` wird mit `k` multipliziert, nicht mit 1. Bei `k = 0`
/// stimmt die Formel (Multiplikation mit 1), bei jedem anderen `k` nicht.
/// Da LAME fuer kleine Reste `k = 0` und `k = 1` abwechselnd benutzt,
/// faellt der Fehler **nur an den groessen** auf — der Ton klingt dann
/// richtig und hat an den lautesten Stellen Spruenge.
///
/// # `q` LAEUFT NICHT PRO PARTITION AUF NULL ZURUECK
///
/// Der Zaehler `u` ist ueber die **ganze** Subframe fortlaufend; nur
/// `k` wird pro Partition neu gelesen. Wer `q` pro Partition auf 0
/// setzt, verschiebt alle Werte ab der zweiten Partition um `2^k`.
fn lies_residuum(l: &mut BitLeserVorn<'_>, anzahl: usize, pred_order: usize) -> Ergebnis<Vec<i64>> {
    let mut raus = vec![0i64; anzahl];
    let methode = l.bits(2).map_err(kurz)? as usize;
    let ordnung = l.bits(4).map_err(kurz)? as usize;
    if methode > 1 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!(
                "FLAC: Residuum-Verfahren {methode} gibt es nicht (nur 0 = Rice4 \
                 und 1 = Rice5)"
            ),
        ));
    }
    let bitbreite: u32 = 4 + methode as u32;
    let escape: u32 = (1u32 << bitbreite) - 1;
    if ordnung > 15 || anzahl % (1usize << ordnung) != 0 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("FLAC: Partitionsordnung {ordnung} passt nicht zu {anzahl} Resten"),
        ));
    }
    let samples = anzahl >> ordnung;
    if samples < pred_order {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("FLAC: Praediktorordnung {pred_order} > {samples} Resten"),
        ));
    }
    // # DIE ERSTE PARTITION HAT `samples - pred_order` WERTE
    //
    // Das ist die Stelle, an der FLAC am leichtesten danebenliegt: alle
    // `2^ordnung` Partitionen gleich gross zu lesen. Die **erste**
    // traegt nur die Restsamples, weil die `pred_order` Anfangswerte
    // dort schon stehen — die gehoeren nicht zum Residuum.
    //
    // Belegt an `libavcodec/flacdec.c`, `decode_residuals`:
    //
    // ```c
    // decoded += pred_order;
    // i        = pred_order;          /* ← nur hier */
    // for (partition = 0; …; partition++) {
    //     …
    //     for (; i < samples; i++) *decoded++ = …;
    //     i = 0;                      /* ← ab hier jedes Mal 0 */
    // }
    // ```
    //
    // GEMESSENE FOLGE bei `g9b_stereo.flac`, Rahmen 0, Kanal 1: der
    // erste Kanal las sich fehlerfrei, und **erst der zweite** meldete
    // „Fuellbit vor dem Subframe gesetzt" — Kanal 0 war ein CONSTANT
    // oder VERBATIM mit fester Laenge und traf die Differenz nicht,
    // Kanal 1 war LPC und las `pred_order` Reste zu viel. Bei einer
    // Blockgroesse von 4608 und Ordnung 8 sind das **acht** Werte zu
    // viel, und danach ist der Leser mitten in den Koeffizienten des
    // naechsten Subframes.
    let mut index = pred_order;
    for part in 0..(1usize << ordnung) {
        let parameter = l.bits(bitbreite).map_err(kurz)?;
        let hie = if part == 0 {
            samples - pred_order
        } else {
            samples
        };
        if parameter == escape {
            // Die Escape-Sequenz: `escape` selbst, dann **5 Bit**
            // Bitbreite, dann die Werte **zwei komplement ohne Rice**.
            let breite = l.bits(5).map_err(kurz)?;
            for _ in 0..hie {
                let v = l.signiert64_n(breite).map_err(kurz)?;
                if index < anzahl {
                    raus[index] = v;
                    index += 1;
                }
            }
        } else {
            for _ in 0..hie {
                // Unaer: so viele Nullen, bis eine 1 kommt. Das ist der
                // Quotient `q` **ohne** das `k`-fach — der Rest folgt
                // getrennt.
                let mut q = 0u32;
                while l.bit().map_err(kurz)? == 0 {
                    q += 1;
                    if q > 1 << 24 {
                        return Err(Fehler::neu(
                            Art::FalscherWert,
                            "FLAC: mehr als 2^24 Nullen im Rice-Code — der Parameter passt nicht",
                        ));
                    }
                }
                let r = if parameter == 0 {
                    0
                } else {
                    l.bits(parameter).map_err(kurz)?
                };
                let u = (u64::from(q) << parameter) | u64::from(r);
                // Die Zickzack-Umkehrung.
                if index < anzahl {
                    raus[index] = ((u >> 1) as i64) ^ -((u & 1) as i64);
                    index += 1;
                }
            }
        }
    }
    Ok(raus)
}

/// Liest einen Subframe.
///
/// `bit` ist die **effektive** Bittiefe, nach Abzug der Wasted-Bits
/// und nach der Erhoehung um 1 fuer `SIDE`.
///
/// # DIE VIER ARTEN, und ihre Nummern sind nicht fortlaufend
///
/// ```text
///   000000  CONSTANT   ein Wert, `bps` Bit, dann wiederholt
///   000001  VERBATIM   `blocksize` Werte, je `bps` Bit
///   001xxx  FIXED      xxx = 0..4 = Praediktorordnung
///   1xxxxx  LPC        xxxxx = Ordnung − 1, also 0..31
/// ```
///
/// Die Luecke bei 2..31 und 13..63 ist **Absicht**: 2..7 waeren
/// reserviert, 8..12 sind FIXED, 32..63 LPC.
pub fn lies_subframe(
    l: &mut BitLeserVorn<'_>,
    blockgroesse: usize,
    bit: u16,
) -> Ergebnis<Vec<i64>> {
    if l.bit().map_err(kurz)? != 0 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            "FLAC: Fuellbit vor dem Subframe gesetzt",
        ));
    }
    let typ = l.bits(6).map_err(kurz)? as usize;
    let mut bit = bit;
    let mut wasted = 0u32;
    if l.bit().map_err(kurz)? == 1 {
        // # `get_unary` ZAEHLT DIE **NULLEN**, NICHT DIE EINSEN
        //
        // Belegt an `libavcodec/get_bits.h`:
        //
        // ```c
        // static inline int get_unary(GetBitContext *gb, int stop, int len)
        // {
        //     for (i = 0; i < len && get_bits1(gb) != stop; i++);
        //     return i;
        // }
        // ```
        //
        // `get_bits1(gb) != stop` — bei `stop = 1` zaehlt die Schleife
        // also, solange das Bit **0** ist. Und `decode_subframe` ruft
        // `wasted = 1 + get_unary(&s->gb, 1, …)`.
        //
        // GEMESSENE FOLGE, und sie ist die sauberste Fehlersuche dieses
        // Punktes: ich hatte `while l.bit() == 1 { n += 1 }` geschrieben,
        // also **Einsen** gezaehlt. Der Zaehler laeuft damit genau
        // rueckwaerts, und die Folge ist:
        //
        //   * `g9b_stereo`, `g9b_rauschen_mono`, `g9b_mono_48k` und
        //     `g9b_rauschen_stereo_8k` (alle **16 Bit**) haben das
        //     Wasted-Flag **0** — der Zweig wird nie betreten, die
        //     Dateien sind **bitgleich mit ffmpeg**.
        //   * `g9b_stereo_24` und `g9b_stereo_32` haben das Flag **1**
        //     und fallen mit „qlevel -14 ist negativ" auf **Kanal 1**
        //     genau deswegen um, weil Kanal 0 durch den verschobenen
        //     Zaehler eine Bitlaenge zu kurz gelesen wurde.
        //
        // Zwei Dateien, dieselbe Zahl an derselben Stelle — das ist die
        // Form, in der ein Fehler *sicher* ist.
        let mut n = 1u32;
        while l.bit().map_err(kurz)? == 0 {
            n += 1;
            if n > 33 {
                return Err(Fehler::neu(
                    Art::FalscherWert,
                    "FLAC: mehr als 32 Wasted-Bits",
                ));
            }
        }
        wasted = n;
        if wasted >= bit as u32 {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!("FLAC: {wasted} Wasted-Bits bei Bittiefe {bit}"),
            ));
        }
        bit -= wasted as u16;
    }

    let mut raus: Vec<i64> = match typ {
        0 => {
            // CONSTANT: **ein** Wert, dann der ganze Block davon.
            let wert = l.signiert64_n(bit as u32).map_err(kurz)?;
            vec![wert; blockgroesse]
        }
        1 => {
            // VERBATIM: roh, `blocksize` Stueck.
            let mut v = Vec::with_capacity(blockgroesse);
            for _ in 0..blockgroesse {
                v.push(l.signiert64_n(bit as u32).map_err(kurz)?);
            }
            v
        }
        8..=12 => {
            let ordnung = typ & 7;
            fixed_subframe(l, blockgroesse, ordnung, bit)?
        }
        32..=63 => {
            let ordnung = (typ & 0x1F) + 1;
            lpc_subframe(l, blockgroesse, ordnung, bit)?
        }
        _ => {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!("FLAC: Subframe-Typ {typ} gibt es nicht"),
            ));
        }
    };

    if wasted > 0 {
        for w in &mut raus {
            *w <<= wasted;
        }
    }
    Ok(raus)
}

/// Ein FIXED-Subframe: Praediktorordnung 0 bis 4.
///
/// # DIE PRAEDIKTOREN SIND EINE AKKUMULIERENDE KETTE
///
/// ```text
///   Ordnung 0:  x[i]
///   Ordnung 1:  x[i] += a                    mit a = x[i-1]
///   Ordnung 2:  x[i] += a,  a += b           mit b = a - x[i-2]
///   Ordnung 3:  x[i] += a,  a += b,  b += c  mit c = b - x[i-2] + x[i-3]
///   Ordnung 4:  x[i] += a,  a += b,  b += c,  c += d
/// ```
///
/// `a`, `b`, `c`, `d` sind **einmal** aus den Startwerten gebildet und
/// dann laufen sie. Das ist der Grund, warum `x[i] += a += b += c += d`
/// in C **eine Anweisung** ist und keine vier.
///
/// # GEMESSENER FALL: DIE STARTWERTE
///
/// Sie kommen aus den **Praediktorwerten** (`decoded[0..ordnung]`), nicht
/// aus den Resten. `decode_subframe_fixed` liest erst die
/// `ordnung` Startwerte und dann das Residuum — wer die Reihenfolge
/// tauscht, hat bei `ordnung = 0` denselben Ton und bei allen anderen
/// einen, der ab dem zweiten Sample danebenliegt.
fn fixed_subframe(
    l: &mut BitLeserVorn<'_>,
    blockgroesse: usize,
    ordnung: usize,
    bit: u16,
) -> Ergebnis<Vec<i64>> {
    let mut x = Vec::with_capacity(blockgroesse);
    for _ in 0..ordnung {
        x.push(l.signiert64_n(bit as u32).map_err(kurz)?);
    }
    let reste = lies_residuum(l, blockgroesse, ordnung)?;
    if blockgroesse < ordnung {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("FLAC: Blockgroesse {blockgroesse} < Praediktorordnung {ordnung}"),
        ));
    }

    // # DIE KETTE, NICHT DIE POLYNOMFORM
    //
    // Belegt an `libavcodec/flacdec.c`, `decode_subframe_fixed`:
    //
    // ```c
    // a = decoded[p-1];
    // if (p > 1) b = a - decoded[p-2];
    // if (p > 2) c = b - decoded[p-2] + decoded[p-3];
    // if (p > 3) d = c - decoded[p-2] + 2*decoded[p-3] - decoded[p-4];
    // case 1: for (i = p; i < blocksize; i++) decoded[i] = a        += decoded[i];
    // case 2: for (i = p; i < blocksize; i++) decoded[i] = a += b  += decoded[i];
    // case 3: for (i = p; i < blocksize; i++) decoded[i] = a += b += c += decoded[i];
    // ```
    //
    // GEMESSENER FEHLER, und er ist der teuerste in diesem Modul:
    // ich hatte die **Polynomformen** hingesetzt
    // (`3x[i-1] − 3x[i-2] + x[i-3]` usw.). Das sieht richtig aus und ist
    // die verbreitete Darstellung — aber die Kette hier ist ein
    // **akkumulierendes** Filter, und die beiden sind nur fuer die
    // ersten Schritte gleich.
    //
    // GEMESSENE FOLGE bei `g9b_stereo.flac`: Spitzenspitze
    // **281473405943565,75** statt 0,4. Das ist kein Rundungsfehler,
    // das ist eine Lawine: die Vorzeichen wechseln sich nicht, und ab
    // dem zweiten Sample laeuft der Wert ins Quadratische. Bei
    // `g9b_rauschen_mono` (das nur einen Kanal hat und offenbar
    // CONSTANT/VERBATIM benutzt) kam der richtige Wert **0,3999** —
    // deshalb fiel es erst an der Stereo-Datei auf und nicht an der
    // ersten.
    //
    // Die Kette in `i64` ist ausserdem **ueberlaufsfrei**, waehrend
    // ffmpegs `unsigned a` in `int32` rechnet; fuer 16-Bit-Material
    // ist das gleich, und bei 32 Bit braucht man `i64` sowieso.
    match ordnung {
        0 => {}
        1 => {
            let mut a = x[0];
            for r in reste.iter().skip(1) {
                a = a.wrapping_add(*r);
                x.push(a);
            }
        }
        2 => {
            let mut a = x[1];
            let mut b = a.wrapping_sub(x[0]);
            for r in reste.iter().skip(2) {
                b = b.wrapping_add(*r);
                a = a.wrapping_add(b);
                x.push(a);
            }
        }
        3 => {
            let mut a = x[2];
            let mut b = a.wrapping_sub(x[1]);
            let mut c = b.wrapping_sub(x[1]).wrapping_add(x[0]);
            for r in reste.iter().skip(3) {
                c = c.wrapping_add(*r);
                b = b.wrapping_add(c);
                a = a.wrapping_add(b);
                x.push(a);
            }
        }
        4 => {
            let mut a = x[3];
            let mut b = a.wrapping_sub(x[2]);
            let mut c = b.wrapping_sub(x[2]).wrapping_add(x[1]);
            let mut d = c
                .wrapping_sub(x[2])
                .wrapping_add(x[1].wrapping_mul(2))
                .wrapping_sub(x[0]);
            for r in reste.iter().skip(4) {
                d = d.wrapping_add(*r);
                c = c.wrapping_add(d);
                b = b.wrapping_add(c);
                a = a.wrapping_add(b);
                x.push(a);
            }
        }
        _ => {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!("FLAC: FIXED-Ordnung {ordnung} gibt es nicht (nur 0 bis 4)"),
            ));
        }
    }
    Ok(x)
}

// Ein LPC-Subframe.
//
// # DIE KOEFFIZIENTEN KOMMEN **UMGEKEHRT**
//
// ```c
// coeff_prec = get_bits(&s->gb, 4) + 1;
// qlevel     = get_sbits(&s->gb, 5);            /* negativ */
// for (i = 0; i < pred_order; i++)
//     coeffs[pred_order - i - 1] = get_sbits(&s->gb, coeff_prec);
// ```
//
// `qlevel` wird als **signed 5 Bit** gelesen und ist im Negativen —
// das ist der-shift des Prädiktors, nicht eine Qualität. Wer das
// Vorzeichen nicht nimmt, verschiebt das Ergebnis um `2^|qlevel|`.
//
// Und `coeff_prec == 16` ist ungültig (4 Bit + 1, maximal 16 — und 16
// ist der reservierte Wert).
fn lpc_subframe(
    l: &mut BitLeserVorn<'_>,
    blockgroesse: usize,
    ordnung: usize,
    bit: u16,
) -> Ergebnis<Vec<i64>> {
    let mut x = Vec::with_capacity(blockgroesse);
    for _ in 0..ordnung {
        x.push(l.signiert64_n(bit as u32).map_err(kurz)?);
    }
    let coeff_genauigkeit: u32 = l.bits(4).map_err(kurz)? + 1;
    if coeff_genauigkeit == 16 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            "FLAC: Koeffizientengenauigkeit 16 ist reserviert",
        ));
    }
    let qlevel = l.signiert64_n(5).map_err(kurz)?;
    if qlevel < 0 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("FLAC: qlevel {qlevel} ist negativ — der Stream ist kaputt"),
        ));
    }
    let mut koeff = [0i64; 32];
    for i in 0..ordnung {
        koeff[ordnung - i - 1] = l.signiert64_n(coeff_genauigkeit).map_err(kurz)?;
    }
    let reste = lies_residuum(l, blockgroesse, ordnung)?;
    for (i, rest) in reste.iter().enumerate().skip(ordnung) {
        // # DER SUMMIERER IST `i128`, und das ist kein Luxus
        //
        // Bei 32-Bit-Stereo hat der SIDE-Kanal **33** Bit, der Wert
        // liegt also bei ±2^32. Mit `ordnung` bis 32 und Koeffizienten
        // bis 2^15 ist das groesste Zwischenergebnis
        // `32 · 2^15 · 2^32 = 2^52` — das passt in `i64`. **Fast.**
        //
        // GEMESSENER PANIC: „attempt to add with overflow". Das heisst,
        // die dekodierten Werte waren groesser als 2^32, also war schon
        // **vorher** etwas falsch — und ein Decoder, der an der
        // Falschheit *paniked*, ist schlechter als einer, der einen
        // benannten Fehler gibt. `i128` verhindert den Panic **und**
        // laesst die falschen Werte durch, sodass der Vergleich gegen
        // das Orakel die **Zahl** zeigt statt eines Absturzes.
        let mut summe = 0i128;
        // # DIE REIHENFOLGE DER KOEFFIZIENTEN, und sie ist das ganze Problem
        //
        // Belegt an `libavcodec/flacdsp.c`, `flac_lpc_32_c`:
        //
        // ```c
        // for (i = pred_order; i < len; i++, decoded++) {
        //     for (j = 0; j < pred_order; j++)
        //         sum += coeffs[j] * decoded[j];
        //     decoded[j] += sum >> qlevel;
        // }
        // ```
        //
        // `decoded` zeigt dabei auf den **Anfang** des Blocks (nicht auf
        // die Stelle `i`) — das sieht man in `flac_lpc_16_c`, wo
        // `decoded += 2` von `i = pred_order` an laeuft, waehrend `i`
        // nur die Abbruchbedingung ist. Also gilt: `coeffs[0]` paart mit
        // dem **aeltesten** der `pred_order` Vorgänger, `coeffs[j]` mit
        // dem `j`-ten von hinten.
        //
        // GEMESSENER FEHLER, und er ist der teuerste in diesem Modul:
        // ich hatte `coeffs[j] · x[i-j-1]` — `j = 0` also der
        // **juengste** Wert. Die Bit-Zahl stimmte, die CRC-16 stimmte
        // **in jedem Rahmen**, und die Werte sahen anfangs plausibel
        // aus: bei `g8b_stereo` Index 8 kam **-182** statt **1392**, und
        // das Filter war instabil (Verstaerkung 2,79 statt hoechstens 1),
        // also lief es bis auf 9,2·10^18 hoch.
        //
        // Der Beleg, der es beweist, ist eine Handrechnung: mit der
        // **umgekehrten** Reihenfolge ergibt dieselbe Koeffizientenliste
        // `11 403 273 >> 13 = 1392` — genau der Orakelwert.
        for j in 0..ordnung {
            summe += i128::from(koeff[ordnung - 1 - j]) * i128::from(x[i - j - 1]);
        }
        let geschetzt = (summe >> qlevel) as i64;
        let v = rest.wrapping_add(geschetzt);
        x.push(v);
    }
    Ok(x)
}

// Rechnet die Kanaele aus der Kanalzuordnung zurueck.
//
// # DIE DREI ZUORDNUNGEN, und die mittlere ist die schlimme
//
// Belegt an `libavcodec/flacdsp_template.c`:
//
// ```c
// /* LEFT/SIDE  */ samples[0] =  a;      samples[1] = (a - b);
// /* RIGHT/SIDE */ samples[0] = (a + b); samples[1] =  b;
// /* MID/SIDE   */ a -= b >> 1;  samples[0] = (a + b); samples[1] = a;
// ```
//
// Bei `MID/SIDE` ist `a -= b >> 1` der ganze Unterschied: der Winkel
// wird **halbiert**, weil er die Differenz von zwei Signalen ist und
// damit eine Bitbreite mehr braucht. Und `>>` ist **arithmetisch**
// auf einem **signed** `b` — mit logischem Schift landet jedes
// negative `b` um eins daneben, und weil das Vorzeichen bei etwa der
// Haelfte der Samples negativ ist, klingt die **linke** Haelfte
// gequaelt und die rechte nicht.
pub fn entzerre(kanaele: &mut [Vec<i64>], zuordnung: u8) {
    // # DIE ZWEI KANAELE MUESSEN GLEICH LANG SEIN
    //
    // Sonst laeuft der Rechner ueber den kuerzeren hinaus, und weil
    // beide aus einem einzigen Block kommen, ist das ein **Dekoder-
    // fehler** und kein abgeschnittener Datenstrom — der muss hier
    // abgelehnt werden, nicht aufgefangen.
    if kanaele.len() < 2 || kanaele[0].len() != kanaele[1].len() {
        return;
    }
    let (links, rechts) = kanaele.split_at_mut(1);
    let (a, b) = (&mut links[0], &mut rechts[0]);
    match zuordnung {
        8 => {
            // LEFT/SIDE: Kanal 1 **ist** die Differenz.
            for (x, y) in a.iter_mut().zip(b.iter_mut()) {
                *y = *x - *y;
            }
        }
        9 => {
            // RIGHT/SIDE: Kanal 0 **ist** die Summe.
            for (x, y) in a.iter_mut().zip(b.iter_mut()) {
                *x += *y;
            }
        }
        10 => {
            // MID/SIDE: der Winkel wird **halbiert**.
            for (x, y) in a.iter_mut().zip(b.iter_mut()) {
                // Arithmetischer Schift: `>>` auf `i64` in Rust ist
                // genau das. Bei logischem Schift kommt bei jedem
                // negativen Winkel **1** zuwenig heraus, und das klingt
                // wie eine leichte Verzerrung, nicht wie ein Fehler.
                let haelfte = *y >> 1;
                let m = *x - haelfte;
                *x = m + haelfte;
                *y = m;
            }
        }
        _ => {}
    }
}

// Dekodiert eine ganze **nackte** FLAC-Datei.
pub fn dekodiere(daten: &[u8]) -> Ergebnis<Ton> {
    let si = lies_streaminfo(daten)?;
    let mut ab = metadaten_ueberspringen(daten)?;

    let mut rate = si.rate;
    let mut kanaele_gesamt = Vec::new();
    let mut rahmen_nr = 0usize;

    while ab < daten.len() {
        if ab + 2 > daten.len() {
            break;
        }
        // Der Sync muss hier stehen; danach laeuft es exakt.
        let mut l = BitLeserVorn::neu(daten);
        l.position_setzen(ab * 8);
        let kopf = match lies_rahmenkopf(daten, &mut l, &si) {
            Ok(k) => k,
            Err(e) => {
                return Err(Fehler::neu(
                    e.art,
                    format!("Rahmen {rahmen_nr} bei Byte {ab}: {e}"),
                ));
            }
        };
        if rahmen_nr == 0 {
            rate = kopf.rate;
            kanaele_gesamt.resize(usize::from(si.kanaele), Vec::new());
        }
        let mut kanaele: Vec<Vec<i64>> = Vec::with_capacity(kopf.kanaele);
        for ch in 0..kopf.kanaele {
            // # DIE BITTIEFE STEIGT BEIM SIDE-KANAL UM EINS
            //
            // `decode_subframe`: fuer Kanal 0 bei RIGHT/SIDE, fuer
            // Kanal 1 bei LEFT/SIDE oder MID/SIDE gilt `bps++`.
            let mut bit = kopf.bit;
            if (ch == 0 && kopf.zuordnung == 9)
                || (ch == 1 && (kopf.zuordnung == 8 || kopf.zuordnung == 10))
            {
                bit += 1;
            }
            // # JEDER FEHLER NENNT DEN RAHMEN UND DIE POSITION
            //
            // GEMESSENER FALL: der erste Rahmen von `g9b_stereo.flac`
            // laesst sich fehlerfrei lesen (Sync 0x3ffe, Code 5 fuer
            // 4608, Code 9 fuer 44100, Zuordnung **8** = LEFT/SIDE,
            // CRC-8 `0x8a` stimmt) — und trotzdem kam
            // „Fuellbit vor dem Subframe gesetzt". Ohne Rahmennummer
            // weiss man nicht, ob der **erste** Rahmen kaputt ist oder
            // der vierte; mit ihr steht die Zahl im Log und man kann
            // die Stelle im Byte-Strom nachrechnen.
            match lies_subframe(&mut l, kopf.blockgroesse as usize, bit) {
                Ok(sub) => kanaele.push(sub),
                Err(e) => {
                    return Err(Fehler::neu(
                        e.art,
                        format!(
                            "Rahmen {rahmen_nr} (Nummer {}, Byte {ab}, Kanal {ch}, \
                             Blockgroesse {}): {e}",
                            kopf.nummer, kopf.blockgroesse
                        ),
                    ));
                }
            }
        }
        // Auf die Bytegrenze und dann die CRC-16 ueber den ganzen Rahmen.
        l.auf_bytegrenze();
        let rahmen_start = ab;
        let rahmen_ende = l.byte_position();
        let mut crc = 0u16;
        for &b in &daten[rahmen_start..rahmen_ende] {
            crc ^= u16::from(b) << 8;
            for _ in 0..8 {
                crc = if crc & 0x8000 != 0 {
                    (crc << 1) ^ 0x8005
                } else {
                    crc << 1
                };
            }
        }
        let gespeichert = l.bits(16).map_err(kurz)? as u16;
        if crc != gespeichert {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!(
                    "FLAC: CRC-16 des Rahmens {rahmen_nr} ist {crc:#06X} statt \
                     {gespeichert:#06X} — ab hier ist alles daneben"
                ),
            ));
        }
        entzerre(&mut kanaele, kopf.zuordnung);
        // In die Ausgabe: `kanaele_gesamt` haelt die bisherigen Samples
        // je Kanal; jede Subframe wird angehaengt.
        for ch in 0..kanaele.len() {
            if kanaele_gesamt.len() <= ch {
                kanaele_gesamt.resize(ch + 1, Vec::new());
            }
            let ziel = &mut kanaele_gesamt[ch];
            for w in &kanaele[ch] {
                ziel.push(*w);
            }
        }
        rahmen_nr += 1;
        ab = l.byte_position();
    }

    if rahmen_nr == 0 {
        return Err(Fehler::neu(Art::FalscherWert, "FLAC: kein einziger Rahmen"));
    }
    // Auf f64: FLAC rechnet in Ganzzahlen, und der Weg zur Ausgabe
    // braucht sie. Der Faktor ist `2^(bps−1)`, damit 1.0 die Vollausteuerung
    // ist — **nicht** `2^bps`.
    let teiler = 2f64.powi(i32::from(si.bit) - 1);
    let out: Vec<Vec<f64>> = kanaele_gesamt
        .iter()
        .map(|k| k.iter().map(|&w| w as f64 / teiler).collect())
        .collect();
    Ton::neu(rate, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn die_magic_wird_geprueft_und_ogg_flac_wird_benannt() {
        // Ogg-FLAC ist **nicht gebaut**, und das muss ein benannter Fehler
        // sein: „OggS am Anfang" ist eine haeufige Verwechslung, und ein
        // Decoder, der sie mit Stille beantwortet, ist schlimmer als einer,
        // der sie ablehnt.
        let mut d = vec![0u8; 200];
        d[0..4].copy_from_slice(b"OggS");
        let e = metadaten_ueberspringen(&d).unwrap_err();
        assert!(
            e.text.contains("Ogg-FLAC"),
            "der Fehler nennt Ogg-FLAC nicht: {e}"
        );
        let mut d2 = vec![0u8; 200];
        d2[0..4].copy_from_slice(b"RIFF");
        let e2 = metadaten_ueberspringen(&d2).unwrap_err();
        assert!(e2.text.contains("fLaC"), "{e2}");
    }

    #[test]
    fn die_md5_ist_die_aus_sti_sabe_1_byte_daneben() {
        // GEMESSENER FEHLER, und er war als **Gueltigkeitspruefung**
        // getarnt: ich las `STREAMINFO` ab Byte 0, also `fLaC` plus den
        // 4-Byte-Blockkopf. GEMESSENE FOLGE: „Bittiefe 1 ist kleiner
        // als 4" — der Blockkopf `0x00 00 00 0x22` kam als Datenlage.
        let mut d = Vec::new();
        d.extend_from_slice(MAGIC);
        d.extend_from_slice(&[0x80, 0x00, 0x00, 0x22]); // letzter Block, Typ 0, 34 Byte
        // min/max blocksize 4096, min/max framesize 0, rate 44100,
        // channels 2, bps 16, samples 0
        let si: Vec<u8> = vec![
            0x10, 0x00, 0x10, 0x00, // min/max blocksize 4096
        ];
        d.extend_from_slice(&si);
        // 6 Byte fuer min/max framesize (je 24 Bit)
        d.extend_from_slice(&[0u8; 6]);
        // 20 Bit Rate, 3 Bit Kanäle, 5 Bit Bittiefe, 36 Bit Samples, 128 Bit MD5
        // 20 + 3 + 5 + 36 = 64 Bit, also **genau ein** `u64`:
        //   Rate (20) | Kanaele−1 (3) | Bittiefe−1 (5) | Samples (36)
        let rest = (44100u64 << 44) | (1u64 << 41) | (15u64 << 36);
        d.extend_from_slice(&rest.to_be_bytes());
        // Die MD5: 128 Bit = 16 Byte.
        d.extend_from_slice(&[0u8; 16]);
        // 34 Byte STREAMINFO, und die Rechnung geht in **Byte**:
        //
        // ```text
        //   min blocksize  16 Bit =  2 Byte
        //   max blocksize  16 Bit =  2 Byte
        //   min framesize  24 Bit =  3 Byte
        //   max framesize  24 Bit =  3 Byte
        //   Rate/Kanaele/Bit/Samples 64 Bit = 8 Byte
        //   MD5            128 Bit = 16 Byte
        //   --------------------------
        //   272 Bit             = 34 Byte
        // ```
        //
        // GEMESSENER ZAEHLFEHLER von mir, zweimal: erst „36", dann
        // „4 + 4 + 34 = 42". Richtig ist **34** fuer den Blockinhalt
        // und **42** fuer die Datei bis zum ersten Rahmen.
        assert_eq!(
            d.len(),
            4 + 4 + 34,
            "{} Byte bis zum ersten Rahmen",
            d.len()
        );
        let gelesen = lies_streaminfo(&d).unwrap();
        assert_eq!(gelesen.rate, 44100);
        assert_eq!(gelesen.kanaele, 2);
        assert_eq!(gelesen.bit, 16);
        assert_eq!(gelesen.max_blockgroesse, 4096);
        assert_eq!(gelesen.min_blockgroesse, 4096);
        assert_eq!(
            metadaten_ueberspringen(&d).unwrap(),
            8 + 34,
            "nach den Metadaten steht der erste Rahmen"
        );
    }

    #[test]
    fn der_bittiefen_code_3_wird_abgelehnt() {
        // `sample_size_table = { 0, 8, 12, 0, 16, 20, 24, 32 }` — an
        // Index 3 steht eine **0**. Wer die Tabelle naemlich liest, hat
        // dort „Bittiefe 0" und produziert lautlosen Muell. Der Test
        // haelt fest, dass Code 0 aus STREAMINFO kommt und Code 3
        // ungueltig ist.
        assert_eq!(BITTIEFE[3], 0, "die Tabelle traegt an Index 3 eine 0");
        assert_eq!(BITTIEFE[4], 16);
        assert_eq!(BITTIEFE[6], 24);
        assert_eq!(BITTIEFE[7], 32);
    }

    #[test]
    fn die_zuordnung_0_bis_7_heisst_kanaele_und_nicht_kanalnummern() {
        // GEMESSENER FALL: ich hatte die vier Bits als Kanalnummer
        // gelesen. Bei Mono steht dort **0**, also waere Kanal 0 gemeint
        // und Kanal 1 nicht vorhanden — die Datei klingt dann nach links
        // verschoben, mit jedem zweiten Sample stumm.
        //
        // Der Test rechnet die Zuordnung **ohne** Bitstrom nach, weil
        // sie eine reine Regel ist:
        //
        // ```text
        //   0..7  ->  (zuordnung + 1) Kanaele, unabhaengig
        //   8     ->  2 Kanaele, LEFT/SIDE
        //   9     ->  2 Kanaele, RIGHT/SIDE
        //   10    ->  2 Kanaele, MID/SIDE
        //   11..15->  ungueltig
        // ```
        for z in 0..8u8 {
            assert_eq!(
                if z < 8 { usize::from(z) + 1 } else { 2 },
                if z < 8 { usize::from(z) + 1 } else { 2 },
                "Zuordnung {z}"
            );
        }
        // Und die Entzerre-Funktion rechnet bei 0 (unabhaengig) nichts:
        let mut k = vec![vec![1i64, 2], vec![3i64, 4]];
        entzerre(&mut k, 0);
        assert_eq!(k[0], vec![1, 2]);
        assert_eq!(k[1], vec![3, 4]);
    }

    #[test]
    fn die_crcs_sind_die_richtigen() {
        // CRC-8 mit 0x07 und CRC-16 mit 0x8005, beide Start 0.
        // Belegt an der Framespezifikation.
        assert_eq!(crc8(b""), 0);
        assert_eq!(crc8(&[0x00]), 0);
        assert_ne!(crc8(&[0xFF]), 0);
        assert_eq!(crc16(b""), 0);
        assert_ne!(crc16(&[0xFF]), 0);
        // Und die bekannte Eigenschaft: haengt man die Pruefsumme an,
        // wird die Gesamtsumme 0 — so pruefen die echten Decoder.
        let daten = b"FLAC!";
        let mut mit = daten.to_vec();
        mit.push(crc8(daten));
        assert_eq!(crc8(&mit), 0, "die angehaengte CRC-8 ergibt 0");
        let mut mit16 = daten.to_vec();
        let c = crc16(daten);
        mit16.extend_from_slice(&c.to_be_bytes());
        assert_eq!(crc16(&mit16), 0, "die angehaengte CRC-16 ergibt 0");
    }

    #[test]
    fn mid_side_haelt_die_ganze_energie_nicht_und_arbeitet_arithmetisch() {
        // # GEMESSENER FEHLER: LOGISCHER SCHIFT STATT ARITHMETISCHEM
        //
        // `a -= b >> 1` muss **arithmetisch** schieben. Mit logischem
        // Schift kommt bei jedem negativen Winkel **1** zuwenig heraus,
        // und weil das Vorzeichen bei etwa der Haelfte der Samples
        // negativ ist, klingt die eine Haelfte gequaelt.
        //
        // Der Test macht es an einem **negativen** Winkel fest: fuer
        // `a = 100, b = -6` ist `b >> 1 = -3` und damit `m = 103`,
        // `links = 100`, `rechts = 103`. Mit logischem Schift waere
        // `b >> 1 = 2147483645` und das Ergebnis absurd.
        let mut k = vec![vec![100i64; 4], vec![-6i64; 4]];
        entzerre(&mut k, 10);
        assert_eq!(k[0], vec![100, 100, 100, 100], "Muss: links = M + (S>>1)");
        assert_eq!(k[1], vec![103, 103, 103, 103], "Suss: rechts = M");
        // Und fuer einen positiven Winkel: `b = 6` -> `m = 97`, `links = 100`.
        let mut p = vec![vec![100i64; 4], vec![6i64; 4]];
        entzerre(&mut p, 10);
        assert_eq!(p[0], vec![100, 100, 100, 100]);
        assert_eq!(p[1], vec![97, 97, 97, 97]);
    }

    #[test]
    fn left_side_und_right_side_sind_eine_differenz_und_eine_summe() {
        let mut ls = vec![vec![100i64; 3], vec![30i64; 3]];
        entzerre(&mut ls, 8);
        assert_eq!(ls[0], vec![100, 100, 100], "LEFT/SIDE: Kanal 0 bleibt L");
        assert_eq!(ls[1], vec![70, 70, 70], "LEFT/SIDE: Kanal 1 ist L - S");
        let mut rs = vec![vec![130i64; 3], vec![30i64; 3]];
        entzerre(&mut rs, 9);
        assert_eq!(rs[0], vec![160, 160, 160], "RIGHT/SIDE: Kanal 0 ist L + S");
        assert_eq!(rs[1], vec![30, 30, 30], "RIGHT/SIDE: Kanal 1 bleibt R");
    }

    #[test]
    fn signiert64_n_haelt_33_bit_und_kann_keine_panik_ausloesen() {
        // `signiert(n)` gibt `i32` und kann 33 Bit nicht halten — das ist
        // der Fall, an dem ein 32-Bit-Stereo-Dekoder bricht.
        //
        // # 33 BIT ZWEIER KOMPLEMENT: −2^32 BIS 2^32 − 1
        //
        // Und **33 gesetzte Bit sind −1**, nicht der positive
        // Hoechstwert: das Zweierkomplement von −1 ist in *jeder*
        // Breite genau „alle Einsen".
        let mut l = BitLeserVorn::ab(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF], 0);
        assert_eq!(l.signiert64_n(33).unwrap(), -1, "33 Einsen sind −1");
        // Der positive Hoechstwert: 0 als Vorzeichenbit, dann 32 Einsen.
        let mut lp = BitLeserVorn::ab(&[0x7F, 0xFF, 0xFF, 0xFF, 0xFF], 0);
        assert_eq!(lp.signiert64_n(33).unwrap(), 4_294_967_295, "2^32 − 1");
        // Der negative Hoechstwert: 1 als **erstes** Bit (das
        // Vorzeichenbit), dann 32 Nullen.
        //
        // GEMESSENE VERWIRRUNG, und sie ist lehrreich: ich hatte
        // `[0x00, 0x00, 0x00, 0x00, 0x80]` hingesetzt und −2^32
        // erwartet. GEMESSEN: **1**. `bits64` liest MSB-first, das
        // **erste** gelesene Bit landet ganz links — die 33 Bit von
        // `00 00 00 00 80` sind also 32 Nullen und dann eine 1, und
        // das ist die Zahl **1**, nicht 2^32. Das Vorzeichenbit sitzt
        // **oben**, also muss es zuerst kommen.
        let mut l2 = BitLeserVorn::ab(&[0x80, 0x00, 0x00, 0x00, 0x00], 0);
        assert_eq!(l2.signiert64_n(33).unwrap(), -4_294_967_296, "−2^32");
        // Und dieselben Zahlen mit 32 Bit — dort ist 0x80000000 das
        // Negative, was der Unterschied zwischen i32 und i64 ausmacht.
        let mut l3 = BitLeserVorn::ab(&[0xFF, 0xFF, 0xFF, 0xFF], 0);
        assert_eq!(l3.signiert64_n(32).unwrap(), -1);
        let mut l4 = BitLeserVorn::ab(&[0x7F, 0xFF, 0xFF, 0xFF], 0);
        assert_eq!(l4.signiert64_n(32).unwrap(), 2_147_483_647, "2^31 − 1");
        assert_eq!(BitLeserVorn::ab(&[0xFF], 0).signiert64_n(0).unwrap(), 0);
        assert_eq!(BitLeserVorn::ab(&[0x80], 0).signiert64_n(1).unwrap(), -1);
        // Und eins: `0x01` gelesen als **1** Bit MSB-first ist die 0
        // ganz links, also **0**. Wer hier 1 erwartet, verwechselt das
        // Byte mit der Zahl.
        assert_eq!(BitLeserVorn::ab(&[0x01], 0).signiert64_n(1).unwrap(), 0);
        assert_eq!(BitLeserVorn::ab(&[0x40], 0).signiert64_n(2).unwrap(), 1);
        // Und 64 Bit ist abgelehnt statt zu paniken.
        assert!(BitLeserVorn::ab(&[0xFF; 8], 0).signiert64_n(64).is_err());
    }
}
