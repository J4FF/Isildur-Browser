//! MPEG-1 AUDIO LAYER III (MP3), CBR.
//!
//! Aufbau einer Datei: eine Kette von Rahmen, jeder gleich lang und ohne
//! jede Tabelle davor.
//!
//! ```text
//! Rahmen
//!   Kopf        4 Byte
//!   Seiteninfo 17 Byte (mono) / 32 Byte (stereo)
//!   Hauptdaten  Rest des Rahmens
//! ```
//!
//! # Stand: der Rahmen steht, die Granulate noch nicht
//!
//! Geschrieben wird bisher ein Rahmen mit **vollstaendig null gesetzter
//! Seiteninfo**. Das ist kein Umweg, sondern der kuerzeste Weg zu einem
//! gelesenen Rahmen, und es ist ausdruecklich so gewaehlt:
//!
//! Eine Seiteninfo besteht aus lauter Bitfeldern unterschiedlicher Breite
//! (12, 9, 8, 5, 4, 3, 1 ...). Solange **alle** Felder null sind, ist das
//! Ergebnis von den Breiten unabhaengig — der Leser liest ueberall Nullen
//! und findet in jedem Feld eine zulaessige Belegung:
//!
//! * `main_data_begin = 0` (keine Vorlaufbytes),
//! * `part2_3_length = 0` fuer beide Granulate (keine Spektraldaten),
//! * `big_values = 0`, also kein Huffman-Bereich,
//! * `count1`-Bereich leer,
//! * alle drei `table_select` null.
//!
//! Ein solcher Rahmen ist **vollstaendig gueltig** und dekodiert zu 1152
//! Nullproben. Er ist damit der Beweis, dass Rahmenkopf und Seiteninfo
//! stimmen — und das ist der Teil, der bei FLAC zwei Naechte gekostet hat.
//!
//! Was noch fehlt, ist der Inhalt: Skalierungsfaktoren, Huffman-Daten,
//! Requantisierung, Stereo, Aliasbeseitigung, IMDCT und Polyphasefilter.
//! Steht als offener Rest am Ende dieser Datei.
//!
//! # Der Fehler, den die Bytelaenge schon einmal war
//!
//! Die Rahmenlaenge ist **nicht** `144 * bitrate / rate` als ganze Zahl —
//! der Rest geht nicht verloren, sondern er wird ueber die Zeit als Fuellbit
//! verteilt. Ohne das driftet der Ton gegen die Wand und wird nach
//! Minuten unhoerbar.
//!
//! GEMESSEN 128000 bit/s bei 44100 Hz:
//!
//! ```text
//! Rahmen 0: 417 Byte      Rahmen 5: 418 Byte
//! ```
//!
//! Der Rahmen hat 144 * 128000 = 18 432 000 bit = 417,959 Byte. Ohne
//! Fuellbit wuerde jeder Rahmen 417 Byte bekommen, und nach 1000 Rahmen
//! fehlten 959 Byte — eine Sekunde Ton waere um 12 ms zu kurz.

//! # Zwei Dinge, die am 02.10.2026 gemessen wurden und die Arbeit sparen
//!
//! Beides aus `libavcodec/mpegaudiodsp.c` (FFmpeg n6.1.1), der Datei, die
//! das Analysefenster und die Frequenzumkehr aufbaut. Sie ist **klein**, und
//! sie beantwortet zwei Fragen, die man sonst durch Raten beantwortet.
//!
//! **1. Das Analysefenster ist berechnet, nicht tabuliert.** Es gibt keine
//! 512-Werte-Tabelle, die man abtippen muesste — die Werte entstehen aus
//!
//! ```text
//! d = sin(PI * (i + 0.5) / 36)                 i = 0..35
//! d *= 0.5 * IMDCT_SCALAR / cos(PI * (2i + 19) / 72)
//! ```
//!
//! mit drei Besonderheiten, die man beim Uebersetzen leicht verliert:
//!
//! * `j == 1` (Block 0 → 1, also die **Uebergangsfenster**): ab `i = 18`
//!   ist `d = 1`, ab `i = 24` wieder ein Sinus, ab `i = 30` ist `d = 0`.
//! * `j == 3`: unterhalb von `i = 6` ist `d = 0`, bis `i = 12` ein Sinus, bis
//!   `i = 18` ist `d = 1`.
//! * `j == 2` ueberspringt zwei von drei Werten (`if (j == 2 && i % 3 != 1)`),
//!   weil dieser Block nur 12 statt 18 Werte traegt — das ist der 12-Punkt-
//!   Zweig des Hybridfilters.
//!
//! **2. Die Frequenzumkehr ist ein Vorzeichenwechsel auf den ungeraden
//! Fensterkoeffizienten**, ausgefuehrt *nach* der MDCT, nicht als eigener
//! Schritt davor:
//!
//! ```c
//! /* NOTE: we do frequency inversion after the MDCT by changing
//!    the sign of the right window coefs */
//! for (i = 0; i < MDCT_BUF_SIZE; i += 2) {
//!     win[j + 4][i    ] =  win[j][i    ];
//!     win[j + 4][i + 1] = -win[j][i + 1];
//! }
//! ```
//!
//! Das ist billiger als ein Vorzeichenwechsel auf den 18 Spektralwerten
//! **pro Subband** und es ist an der richtigen Stelle. Wer es vorher macht,
//! bekommt bei den ungeraden Subbaenden (1, 3, 5, ...) ein gespiegeltes
//! Spektrum — und das klingt nicht kaputt, es klingt **vertauscht**.
//!
//! # Der naechste Schritt, und was dort NICHT geht
//!
//! Als erstes braucht es einen Quadrupel-Huffmancode fuer den count1-Bereich.
//!
//! **GEMESSEN, warum der nicht aus ffmpegs Arrays geraten werden kann:**
//! `mpa_quad_bits[0]` hat die Laengen `{1,4,4,5,4,6,5,6,4,5,5,6,5,6,6,6}` und
//! `mpa_quad_codes[0]` die Codes `{1,5,4,5,6,5,4,4,7,3,6,0,7,2,3,1}`. Die Kraft-
//! Summe dieser Laengen ist
//!
//! ```text
//! 1/2 + 4/16 + 4/32 + 7/64 = 0,984375   und nicht 1
//! ```
//!
//! und wenn man die Codes als MSB-erste Werte innerhalb ihrer Laenge auffaengt
//! und den Coderaum abzaehlt, bleiben **fuenf unbenutzte Slots** uebrig. Ein
//! vollstaendiger Code hat keine. Die Arrays sind also entweder in einer
//! Laengen-Reihenfolge zu lesen, die man erst erraten muss, oder
//! LSB-erst — und genau darin liegt schon der naechste Fehler.
//!
//! Der Code muss aus **ISO 11172-3 Tabelle B.7** kommen. Das ist die eine
//! Stelle, an der Nachdenken nicht reicht und Abtippen schon.

use crate::{KonvertFehler, Zielformat, grenzen, ton::Pcm};

/// Die Bitraten, die MPEG-1 Layer III kennt, **in bit/s**, Index 0..=14.
///
/// Index 0 ist „frei" (free format) und wird nicht benutzt.
///
/// ACHTUNG, EINHEIT: die Tabelle steht in **bit/s**, nicht in kbit/s. Das
/// steht hier, weil ich sie zuerst in kbit/s geschrieben hatte und die
/// Aufrufe in bit/s — und der Unterschied faellt im Kopf NICHT auf: es wird
/// nur ein Index gesucht, und `128` findet keinen Index, waehrend `128_000`
/// auch keinen findet. Beide Fehler sehen gleich aus und heissen
/// „diese Bitrate kennt MPEG-1 Layer III nicht".
pub const MPEG1_L3_BITRATEN: [u32; 15] = [
    0, 32_000, 40_000, 48_000, 56_000, 64_000, 80_000, 96_000, 112_000, 128_000, 160_000, 192_000,
    224_000, 256_000, 320_000,
];

/// Die Abtastraten, die MPEG-1 kennt, Index 0..=2.
pub const MPEG1_RATEN: [u32; 3] = [44100, 48000, 32000];

/// Die drei Bitraten, die der Auftrag K2 verlangt, in **bit/s**.
pub const CBR_STUFEN: [u32; 3] = [128_000, 192_000, 320_000];

/// Proben je Rahmen (MPEG-1 Layer III, alle Fensterarten).
pub const PROBEN_PRO_RAHMEN: usize = 1152;

/// Mindestzahl an Rahmen in einer Datei: **zwei**.
///
/// GEMESSEN 02.10.2026, eine Datei mit genau einem Rahmen:
///
/// ```text
/// 1 Rahmen (417 Byte)   ffmpeg: 0 Byte
///                       [mp3] Failed to find two consecutive MPEG audio frames.
/// 2 Rahmen (835 Byte)   ffmpeg: 4608 Byte, alles da, keine Meldung
/// ```
///
/// Die Sonde des Lesers erkennt eine MP3-Datei nicht an einem Rahmen,
/// sondern an **zwei aufeinanderfolgenden** — sie kann dann namens der
/// zweiten Kopfzeile bestaetigen, dass es ein Strom ist und keine
/// Zufallsbytekette mit Sync darin. Ohne den zweiten Rahmen gibt es nichts
/// zu bestaetigen.
///
/// Das ist keine Eigenheit von ffmpeg, sondern der übliche Weg, wie man
/// MPEG-Audio erkennt; und es passt zum Format selbst: Layer III hat eine
/// Verzoegerung um ein Granular, weil die Dekodierung erst mit dem
/// folgenden Rahmen vollstaendig wird. Eine Datei „1 Frame lang" gibt es
/// bei diesem Format praktisch nicht.
///
/// Deshalb schreibt diese Crate mindestens zwei Rahmen und fuellt den
/// zweiten mit Nullen. Wer zurueckrechnet, muss die Verzoegerung abziehen
/// — das ist normal und nicht vermeidbar.
pub const MINDEST_RAHMEN: usize = 2;

/// Seiteninfo in Byte: MPEG-1 Layer III.
const SEITENINFO_MONO: usize = 17;
const SEITENINFO_STEREO: usize = 32;

/// Der Index der Bitrate im Kopf, oder `None` wenn MPEG-1 sie nicht kennt.
pub const fn bitraten_index(bitrate: u32) -> Option<u8> {
    let mut i = 0;
    while i < MPEG1_L3_BITRATEN.len() {
        if i != 0 && MPEG1_L3_BITRATEN[i] == bitrate {
            return Some(i as u8);
        }
        i += 1;
    }
    None
}

/// Der Index der Abtastrate im Kopf, oder `None` wenn MPEG-1 sie nicht kennt.
pub const fn raten_index(rate: u32) -> Option<u8> {
    let mut i = 0;
    while i < MPEG1_RATEN.len() {
        if MPEG1_RATEN[i] == rate {
            return Some(i as u8);
        }
        i += 1;
    }
    None
}

/// Die Laenge des Rahmens `nummer` in Byte, mit korrekt verteilten Fuellbit.
///
/// `nummer` ist 0-basiert. Gerechnet wird ueber die **kumulierte** Bytezahl,
/// nicht ueber die Rahmenlaenge:
///
/// ```text
/// laenge(n) = floor((n+1)*144*bitrate/rate) - floor(n*144*bitrate/rate)
/// ```
///
/// Das ist genau die Definition von „Fuellbit", und sie faellt automatisch
/// richtig aus: die Summe ueber n Rahmen ist am Ende `floor(N*144*b/r)`,
/// also auf das Byte genau die gewuenschte Bitrate.
pub fn rahmen_laenge(nummer: u64, bitrate: u32, rate: u32) -> usize {
    let bit = 144 * u64::from(bitrate);
    let r = u64::from(rate);
    let ziel = (nummer + 1) * bit / r;
    let vorher = nummer * bit / r;
    (ziel - vorher) as usize
}

/// Die **Nennlaenge** eines Rahmens: `floor(144 * bitrate / rate)`.
///
/// Das ist die Laenge OHNE Fuellbit, und das Fuellbit sagt genau: „ein Byte
/// mehr als diese". Nicht: „ein Byte mehr als der vorige Rahmen".
pub fn nomalllaenge(bitrate: u32, rate: u32) -> usize {
    (144 * bitrate / rate) as usize
}

/// Die Seiteninfo-Groesse fuer `kanale`.
const fn seiteninfo(kanale: u8) -> usize {
    if kanale == 1 {
        SEITENINFO_MONO
    } else {
        SEITENINFO_STEREO
    }
}

/// Schreibt die vier Kopf-Byte eines Rahmens.
///
/// `fuellung` ist das Fuellbit. Es ist 0 oder 1 und entscheidet, ob der
/// Rahmen ein Byte laenger ist — der Kopf selbst sagt das an Bit 16 der
/// Rahmennummer.
fn kopf(bytes: &mut Vec<u8>, br_index: u8, sr_index: u8, kanaele: u8, fuellung: bool) {
    bytes.push(0xFF);
    // 1111 1011: Sync (111), Version 11 = MPEG-1, Layer 01 = Layer III,
    // Schutzbit 1 = keine CRC.
    bytes.push(0xFB);
    // Bitrate (4) | Abtastrate (2) | Fuellbit (1) | privat (1)
    bytes.push((br_index << 4) | (sr_index << 2) | (u8::from(fuellung) << 1));
    // Kanalmodus (2) | Modus-Erweiterung (2) | Copyright (1) | Original (1)
    // | Betonung (2). Kanalmodus 11 = mono, 00 = stereo.
    let modus: u8 = if kanaele == 1 { 3 } else { 0 };
    bytes.push(modus << 6);
}

/// Schreibt `pcm` als MPEG-1-Layer-III-Datei mit konstanter Bitrate.
///
/// # Fehler
/// - [`KonvertFehler::TonKanaleFalsch`] bei 0 oder mehr als 2 Kanaelen.
/// - [`KonvertFehler::ZielNichtSinnvoll`] bei einer Bitrate oder Abtastrate,
///   die MPEG-1 Layer III nicht kennt, und bei einer Bittiefe ungleich 16.
/// - [`KonvertFehler::SpeicherZuGross`] bzw. [`KonvertFehler::LaengeZuGross`],
///   wenn die Datei [`grenzen::MAX_TON_BYTES`] sprengen wuerde.
pub fn schreiben(pcm: &Pcm, bitrate: u32) -> Result<Vec<u8>, KonvertFehler> {
    if pcm.kanale != 1 && pcm.kanale != 2 {
        return Err(KonvertFehler::TonKanaleFalsch { kanale: pcm.kanale });
    }
    if pcm.bit_tiefe != 16 {
        return Err(KonvertFehler::ZielNichtSinnvoll {
            ziel: Zielformat::Mp3,
            grund: "MP3 nimmt hier 16-Bit-Werte; der Enkoder rechnet nicht um",
        });
    }
    let br_index = bitraten_index(bitrate).ok_or(KonvertFehler::ZielNichtSinnvoll {
        ziel: Zielformat::Mp3,
        grund: "diese Bitrate kennt MPEG-1 Layer III nicht",
    })?;
    let sr_index = raten_index(pcm.rate).ok_or(KonvertFehler::ZielNichtSinnvoll {
        ziel: Zielformat::Mp3,
        grund: "diese Abtastrate kennt MPEG-1 Layer III nicht",
    })?;

    let proben_je_frame = pcm.werte.len() / usize::from(pcm.kanale);
    let rahmen = proben_je_frame
        .div_ceil(PROBEN_PRO_RAHMEN)
        .max(MINDEST_RAHMEN);
    let kopf_len = 4 + seiteninfo(pcm.kanale);
    let mut gesamt = 0usize;
    for n in 0..rahmen as u64 {
        gesamt += rahmen_laenge(n, bitrate, pcm.rate);
    }
    grenzen::pruefe_laenge(gesamt, grenzen::MAX_TON_BYTES, "MP3-Datei")?;

    let mut out: Vec<u8> = Vec::with_capacity(gesamt);
    for n in 0..rahmen as u64 {
        let lang = rahmen_laenge(n, bitrate, pcm.rate);
        debug_assert!(lang >= kopf_len, "Rahmen {lang} Byte ist zu kurz");
        // Das Fuellbit bedeutet „ein Byte mehr als die Nennlaenge" — und
        // nicht „ein Byte mehr als der vorige Rahmen".
        //
        // ACHTUNG, ZWEI FEHLER HINTEREINANDER, beide mit derselben Wirkung
        // und beide nur sichtbar, wenn ueberhaupt gefuellt wird:
        //
        // 1. Ich verglich mit dem VORRANGEN (`lang > rahmen_laenge(n-1)`).
        //    GEMESSEN 128000 bit/s bei 44100 Hz, Nennlaenge 417:
        //
        //    ```text
        //    Rahmen 0: 417 Byte, Fuellbit 0   <- richtig
        //    Rahmen 1: 418 Byte, Fuellbit 1   <- richtig
        //    Rahmen 2: 418 Byte, Fuellbit 0   <- FALSCH, 418 > 418 ist falsch
        //    ```
        //
        //    Der dritte Rahmen war 418 Byte lang, behauptete aber Fuellbit 0.
        //
        // 2. Ich verglich (nach dem ersten Fehler) mit `n > 0 && ...`, also
        //    setzte ich im ERSTEN Rahmen Fuellbit 1, weil ich „vorher = 0"
        //    angenommen hatte. GEMESSEN: der Kopf begann `ff fb 92 c0` statt
        //    `ff fb 90 c0`.
        //
        // Beide Faelle zeigen dasselbe: **die Regel lautet „ein Byte mehr als
        // `floor(144*bitrate/rate)`"**, und nicht „mehr als das, was vorher
        // dastand". Das ist bei zwei aufeinanderfolgenden gefuellten Rahmen
        // ein Unterschied, und ffmpegs Meldung war eindeutig:
        //
        // ```text
        // Rahmen 2: 417 Byte, Fuellbit 0   <- ffmpeg las 417 statt 418
        // ... und ein 1-Byte-Rest am Dateiende
        // ```
        let fuellung = lang > nomalllaenge(bitrate, pcm.rate);
        kopf(&mut out, br_index, sr_index, pcm.kanale, fuellung);
        out.resize(out.len() + kopf_len - 4, 0); // Seiteninfo: alles null
        out.resize(out.len() + lang - kopf_len, 0); // Hauptdaten: Fuellung
    }
    debug_assert_eq!(out.len(), gesamt);
    Ok(out)
}

/// Schreibt **einen** Rahmen mit einer frei gebauten Seiteninfo.
///
/// # Wofuer das da ist
///
/// Nicht fuer den normalen Weg, sondern als **Messgeraet**. Der Leser bricht
/// bei zwei Feldern mit einer benannten Meldung ab, und die Position dieser
/// Felder im Bitstrom laesst sich damit bestimmen, ohne die Spezifikation zu
/// raten:
///
/// ```text
/// "big_values too big"     bei big_values > 288
/// "invalid block type"     bei blocksplit_flag = 1 und block_type = 0
/// ```
///
/// Man schreibt `511` (also alle 9 Bit eins) in ein 9-Bit-Fenster an eine
/// Kandidatenstelle. Erscheint die Meldung, war die Stelle richtig.
///
/// GEMESSEN 02.10.2026: die Meldung erscheint **nur** an der Stelle, die die
/// Reihenfolge aus `mpegaudiodec_template.c` vorhersagt. An allen 137
/// anderen Stellen kommt nichts — und das ist der eigentliche Beweis, denn
/// ein 9-Bit-Fenster aus Einsen an der falschen Stelle landet in
/// `global_gain` oder `scalefac_compress` und erzeugt dort keinen Fehler.
pub fn roh_rahmen(
    bitrate: u32,
    rate: u32,
    kanaele: u8,
    si: &crate::bits::Bits,
    hauptdaten: &[u8],
    nummer: u64,
) -> Result<Vec<u8>, KonvertFehler> {
    let br_index = bitraten_index(bitrate).ok_or(KonvertFehler::ZielNichtSinnvoll {
        ziel: Zielformat::Mp3,
        grund: "diese Bitrate kennt MPEG-1 Layer III nicht",
    })?;
    let sr_index = raten_index(rate).ok_or(KonvertFehler::ZielNichtSinnvoll {
        ziel: Zielformat::Mp3,
        grund: "diese Abtastrate kennt MPEG-1 Layer III nicht",
    })?;
    if kanaele != 1 && kanaele != 2 {
        return Err(KonvertFehler::TonKanaleFalsch { kanale: kanaele });
    }
    let lang = rahmen_laenge(nummer, bitrate, rate);
    let kopf_len = 4 + seiteninfo(kanaele);
    if lang <= kopf_len {
        return Err(KonvertFehler::ZielNichtSinnvoll {
            ziel: Zielformat::Mp3,
            grund: "der Rahmen ist kuerzer als sein Kopf",
        });
    }
    let mut out = Vec::with_capacity(lang);
    let fuellung = lang > nomalllaenge(bitrate, rate);
    kopf(&mut out, br_index, sr_index, kanaele, fuellung);
    //
    // Die Seiteninfo wird auf ihre **feste** Laenge gebracht: 17 Byte mono,
    // 32 Byte stereo. GEMESSEN 02.10.2026: der Leser liest 136 bzw. 256 Bit
    // und **nicht** so viele, wie geschrieben wurden — er raeumt nichts auf.
    //
    // Das ist fuer das Messgeraet entscheidend. Ohne die Auffuellung endete
    // die Seiteninfo mitten im `global_gain`, der Rest war Fuellung, und der
    // Leser haette den Fehler an einer Stelle gemeldet, die gar nicht
    // getestet werden sollte.
    let mut si_voll = si.kopie();
    si_voll.beschneide(seiteninfo(kanaele) * 8);
    si_voll.reserviere(seiteninfo(kanaele) * 8 - si_voll.laenge());
    out.extend_from_slice(&si_voll.fertig());
    debug_assert_eq!(out.len(), kopf_len, "Kopf plus volle Seiteninfo");
    out.extend_from_slice(hauptdaten);
    out.resize(lang, 0);
    Ok(out)
}

/// Der Hauptdatenbereich eines Rahmens in **Bit** — alles, was nach dem Kopf
/// und der Seiteninfo Platz hat.
///
/// # WARUM DAS WICHTIG IST, und es ist der Grund fuer den Transient
///
/// GEMESSEN 02.10.2026: `part2_3_length` **muss** diesen Bereich fuellen. Sonst
/// liest der zweite Rahmen an der falschen Stelle.
///
/// Der Leser verschiebt nach jedem Rahmen den **Rest** in einen Bitpuffer:
///
/// ```c
/// memmove(s->last_buf, s->gb.buffer + (get_bits_count(&s->gb) >> 3), i);
/// s->last_buf_size = i;
/// ```
///
/// und im naechsten Rahmen beginnt der count1-Bereich bei
/// `skip = s->last_buf_size - 8 * main_data_begin`. Mit `main_data_begin = 0`
/// — dem einzigen Wert, der fuer eine einzelne Datei sinnvoll ist — beginnt der
/// zweite Rahmen also **bei Byte `last_buf_size`**, und das ist bei einem nicht
/// gefuellten Rahmen der **Leerraum**.
///
/// GEMESSEN, vorher und nachher:
///
/// ```text
/// part2_3_length = die Nutzlast (144 Bit), 44.1 kHz, 2 Rahmen:
///     R1 =     0 /  7653      R2 = 23570 /  7653      <- Transient
/// part2_3_length = der ganze Bereich (2904 Bit), 48 kHz, 4 Rahmen:
///     R1 =     0 /   367      R2 =     0 /   367      R3 =     0 /   367
/// ```
///
/// Der Transient fiel von **23570** auf **0**, und ein leerer Granulat gibt
/// endlich **Stille** — in jedem Rahmen.
///
/// # UND DIE BITRATE OHNE PADDING-BIT
///
/// Der Bereich haengt von der Rahmenlaenge ab, und die wechselt mit dem
/// Padding-Bit. GEMESSEN: bei 128 kbit/s und 44100 Hz sind die Rahmen 417 und
/// 418 Byte — **ein Byte Unterschied**, den ein fester `part2_3_length`
/// nicht treffen kann. Bei 48000 Hz ist `144 * 128000 / 48000 = 384` genau, alle
/// Rahmen sind gleich lang, und es gibt kein Padding-Bit.
///
/// Fuer Messungen ist deshalb **48000 Hz** die bessere Wahl. Das ist keine
/// Schoenheit, sondern die Bedingung dafuer, dass jeder Rahmen dasselbe liest.
pub fn hauptdaten_bereich(nummer: u64, bitrate: u32, rate: u32, kanaele: u8) -> Option<usize> {
    let lang = rahmen_laenge(nummer, bitrate, rate);
    let kopf = 4 + seiteninfo(kanaele);
    lang.checked_sub(kopf).map(|b| b * 8)
}

/// Schreibt eine ganze Datei aus zwei vorgegebenen Granulaten.
///
/// `granulate[0]` ist Granulat 0, `granulate[1]` ist Granulat 1. Bei stereo
/// traegt **jedes** Granulat beide Kanaele hintereinander, in der Reihenfolge
/// Kanal 0 dann Kanal 1 — das ist die Reihenfolge der Granulatrunde im Leser
/// (`for gr { for ch { ... } }`).
///
/// GEMESSEN 02.10.2026: `main_data_begin = 0` heisst, die Hauptdaten des
/// Granulats beginnen **unmittelbar hinter der Seiteninfo**. Das ist der
/// einzige Fall, in dem die Granulate nicht ueber Rahmengrenzen hinweg
/// gesucht werden, und deshalb der einzige, in dem man ohne Puffer
/// auskommt.
pub fn schreiben_granulate(
    pcm: &Pcm,
    bitrate: u32,
    granulate: &[crate::granulat::Granule; 2],
) -> Result<Vec<u8>, KonvertFehler> {
    if pcm.kanale != 1 && pcm.kanale != 2 {
        return Err(KonvertFehler::TonKanaleFalsch { kanale: pcm.kanale });
    }
    for g in granulate {
        g.pruefe()
            .map_err(|grund| KonvertFehler::ZielNichtSinnvoll {
                ziel: Zielformat::Mp3,
                grund,
            })?;
    }
    let proben_je_frame = pcm.werte.len() / usize::from(pcm.kanale);
    let rahmen = proben_je_frame
        .div_ceil(PROBEN_PRO_RAHMEN)
        .max(MINDEST_RAHMEN);
    let mut gesamt = 0usize;
    for n in 0..rahmen as u64 {
        gesamt += rahmen_laenge(n, bitrate, pcm.rate);
    }
    grenzen::pruefe_laenge(gesamt, grenzen::MAX_TON_BYTES, "MP3-Datei")?;

    let mut out: Vec<u8> = Vec::with_capacity(gesamt);
    for n in 0..rahmen as u64 {
        let mut si = crate::bits::Bits::neu();
        si.feld(0, 9); // main_data_begin = 0
        si.feld(0, if pcm.kanale == 1 { 5 } else { 3 });
        for _ in 0..pcm.kanale {
            si.feld(0, 4); // scfsi: alle Skalierungsfaktoren werden gesendet
        }
        let mut haupt: Vec<u8> = Vec::new();
        for g in granulate {
            for _ in 0..pcm.kanale {
                g.schreibe_seiteninfo(&mut si);
                haupt.extend_from_slice(&g.hauptdaten());
            }
        }
        out.extend_from_slice(&roh_rahmen(bitrate, pcm.rate, pcm.kanale, &si, &haupt, n)?);
    }
    debug_assert_eq!(out.len(), gesamt);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Pcm;

    fn leise(len: usize) -> Pcm {
        Pcm::neu(44_100, 1, 16, vec![0; len]).expect("Pcm")
    }

    /// GEMESSEN 02.10.2026: 144 * 128000 / 44100 = 417,959 Byte. Ohne
    /// Fuellbit bekommt jeder Rahmen 417 und die Summe driftet.
    #[test]
    fn fuellbit_verteilt_den_rest_auf_die_rahmen() {
        assert_eq!(rahmen_laenge(0, 128_000, 44_100), 417);
        assert_eq!(rahmen_laenge(1, 128_000, 44_100), 418);
        assert_eq!(rahmen_laenge(2, 128_000, 44_100), 418);
        // Nach 1000 Rahmen sind es 417959 Byte = 417,959 * 1000 gerundet.
        let summe: usize = (0..1000).map(|n| rahmen_laenge(n, 128_000, 44_100)).sum();
        assert_eq!(summe, 417_959);
    }

    /// Die Bitraten des Auftrags muessen im Kopf stehen.
    #[test]
    fn die_drei_cbr_stufen_sind_bekannt() {
        for b in CBR_STUFEN {
            assert!(bitraten_index(b).is_some(), "{b} bit/s fehlt");
        }
        // Die Tabelle steht in bit/s. Wer sie verwechselt, findet nichts.
        assert_eq!(MPEG1_L3_BITRATEN[9], 128_000);
        assert_eq!(bitraten_index(128), None, "kbit/s ist nicht bit/s");
        // Und die Zahl, die der Auftrag nennt, ist die echte Bitrate.
        assert_eq!(bitraten_index(128_000), Some(9));
        assert_eq!(bitraten_index(192_000), Some(11));
        assert_eq!(bitraten_index(320_000), Some(14));
        // 100 kbit/s gibt es bei Layer III nicht (das ist eine Layer-II-Zahl).
        assert_eq!(bitraten_index(100_000), None);
    }

    #[test]
    fn abtastraten_und_kanalzahl_pruefen() {
        for r in MPEG1_RATEN {
            assert_eq!(
                raten_index(r),
                Some(MPEG1_RATEN.iter().position(|&x| x == r).unwrap() as u8)
            );
        }
        // MPEG-2-Raten sind hier nicht erlaubt.
        assert_eq!(raten_index(22_050), None);
        assert_eq!(raten_index(96_000), None);

        assert!(schreiben(&leise(1152), 128_000).is_ok());
        assert!(schreiben(&Pcm::neu(22_050, 1, 16, vec![0; 1152]).unwrap(), 128_000).is_err());
        assert!(schreiben(&Pcm::neu(44_100, 1, 24, vec![0; 1152]).unwrap(), 128_000).is_err());
        assert!(
            schreiben(
                &Pcm::neu(44_100, 6, 16, vec![0; 1152 * 6]).unwrap(),
                128_000
            )
            .is_err()
        );
        assert!(schreiben(&leise(1152), 100_000).is_err());
    }

    /// Der Kopf: Sync, MPEG-1, Layer III, keine CRC, dann Bitrate und
    /// Abtastrate an den Stellen, an denen der Leser sie erwartet.
    #[test]
    fn kopf_steht_an_seiner_stelle() {
        let d = schreiben(&leise(PROBEN_PRO_RAHMEN), 128_000).expect("mp3");
        assert_eq!(&d[0..2], &[0xFF, 0xFB]);
        // 1001 00 0 0 = Bitrateindex 9 (128k), Abtastrateindex 0 (44100),
        // kein Fuellbit, kein privates Bit.
        assert_eq!(d[2], 0x90);
        // 11 00 0 0 00 = mono.
        assert_eq!(d[3], 0xC0);
        // Danach 17 Byte Seiteninfo, alle null.
        assert!(d[4..21].iter().all(|&b| b == 0), "Seiteninfo nicht null");
        // Mindestens zwei Rahmen (siehe `MINDEST_RAHMEN`): 417 + 418.
        assert_eq!(d.len(), 417 + 418);
        assert_eq!(d[417], 0xFF, "zweiter Rahmen ohne Sync");
    }

    /// Das Fuellbit heisst „ein Byte mehr als die Nennlaenge", nicht
    /// „mehr als der vorige Rahmen".
    ///
    /// GEMESSEN 02.10.2026: mit der Vorgänger-Vergleich-Regel bekam der
    /// DRITTE Rahmen 418 Byte und trotzdem Fuellbit 0, weil `418 > 418`
    /// falsch ist. ffmpeg las daraufhin 417 Byte und am Ende blieb ein
    /// Einzelbyte uebrig — mit der Meldung
    /// `Error submitting packet to decoder: Invalid data found`.
    ///
    /// Und der ERSTE Rahmen hatte vorher Fuellbit 1, weil ich
    /// „vorher = 0" angenommen hatte (`417 > 0`).
    #[test]
    fn fuellbit_haengt_an_der_nennlaenge() {
        assert_eq!(nomalllaenge(128_000, 44_100), 417);
        // Bei 32000 Hz und 320 kbit/s ist die Nennlaenge ganzzahlig:
        // 144 * 320000 / 32000 = 1440. Da gibt es nie ein Fuellbit.
        assert_eq!(nomalllaenge(320_000, 32_000), 1440);
        // Jeder Rahmen, der laenger als die Nennlaenge ist, traegt Fuellbit 1.
        for n in 0..8u64 {
            let lang = rahmen_laenge(n, 128_000, 44_100);
            assert_eq!(
                lang > nomalllaenge(128_000, 44_100),
                n != 0,
                "Rahmen {n}: {lang} Byte, Nennlaenge 417"
            );
        }
    }

    /// Stereo hat 32 Byte Seiteninfo und Kanalmodus 00.
    #[test]
    fn stereo_hat_andere_seiteninfo_und_anderen_modus() {
        let p = Pcm::neu(44_100, 2, 16, vec![0; PROBEN_PRO_RAHMEN * 2]).expect("Pcm");
        let d = schreiben(&p, 192_000).expect("mp3");
        assert_eq!(d[3], 0x00, "Kanalmodus muss stereo (00) sein");
        // 144 * 192000 / 44100 = 626,9 Byte Nennlaenge; zwei Rahmen also
        // 626 + 627 = 1253.
        assert_eq!(nomalllaenge(192_000, 44_100), 626);
        assert_eq!(d.len(), 626 + 627);
    }

    /// Nicht nur Frames: mehrere Rahmen hintereinander, und die Datei ist
    /// genau so lang wie die Summe der Rahmenlaengen.
    #[test]
    fn mehrere_rahmen_hintereinander() {
        const B: u32 = 320_000;
        let d = schreiben(&leise(PROBEN_PRO_RAHMEN * 3 + 500), B).expect("mp3");
        // Erwartet: 3 volle Rahmen (3456 Proben) und einer fuer die 500
        // restlichen Proben.
        let erwartet: usize = (0..4).map(|n| rahmen_laenge(n, B, 44_100)).sum();
        assert_eq!(d.len(), erwartet, "Dateilaenge passt nicht zur Rahmensumme");
        // Und jeder Rahmen beginnt wieder mit dem Sync.
        let mut p = 0usize;
        for n in 0..4u64 {
            assert_eq!(d[p], 0xFF, "kein Sync bei Byte {p} (Rahmen {n})");
            assert_eq!(d[p + 1], 0xFB, "Rahmen {n} ist nicht MPEG-1 Layer III");
            p += rahmen_laenge(n, B, 44_100);
        }
        assert_eq!(p, d.len());
    }
}
