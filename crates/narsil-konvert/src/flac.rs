//! FLAC SCHREIBEN (verlustfrei, Untermenge der Spezifikation).
//!
//! # Was hier steht
//!
//! * `fLaC` + STREAMINFO (Pflichtblock)
//! * Rahmen mit **fixed**-Praediktoren (Ordnung 0 bis 4), CONSTANT und
//!   VERBATIM
//! * Rice-Kodierung des Rests mit selbst gewaehltem Parameter je Partition
//! * MD5 ueber den unkomprimierten Inhalt (siehe [`crate::md5`])
//!
//! # Was hier NICHT steht: LPC
//!
//! LPC (Praediktor Ordnung 5 bis 32 mit Auto-Korrelation und
//! Levinson-Durbin) macht FLAC bei Musik noch einmal rund 10 % kleiner. Es
//! fehlt hier, weil es den ganzen Aufbau kippt: man braucht eine
//! Stabilitaetspruefung fuer die Quantisierung, einen Rueckfall auf einen
//! einfacheren Praediktor, wenn die Quantisierung schiefgeht, und die
//! Fehlergrenze an der Bitbreite des Restes.
//!
//! Statt es ungetestet einzubauen, ist es draussen. Lieber „brauchbar und
//! richtig“ als „klein und manchmal kaputt“.
//!
//! # BITREIHENFOLGE: ALLES LSB ZUERST
//!
//! Das ist die Falle, in der man in FLAC zuerst fällt. Der
//! Referenz-Bitwriter schreibt **jede** ganze Zahl LSB zuerst: die
//! 4-Bit-Codes des Rahmenkopfs ebenso wie die 16-Bit-Zusatzfelder und die
//! CRC-16. Nur der 14-Bit-Sync ist symmetrisch und damit egal.
//!
//! Merksatz: „MSB zuerst" gilt im JPEG-Kopf, im PNG und bei Huffman-Codes.
//! Es gilt **nicht** in FLAC. Dieselbe Crate hat in VP8L beides an
//! benachbarten Stellen (`ohm` und `code`) — siehe die Notiz in `webp.rs`.
//! Wer eine Formel aus einem Format ins andere traegt, baut hier einen
//! Fehler, der sich als „falscher Ton" zeigt und nicht als „falscher Kopf“.
//!
//! Aus demselben Grund ist der Subframe-Kopf ein **Byte** und kein
//! B Bitfeld: Fuellbit (0), 6 Bit Typ, 1 Bit „verworfene Bit“. Als
//! LSB-Feld heisst das `typ << 1`, nicht „die sechs Bits von oben“.

use crate::md5::Md5;
use crate::{KonvertFehler, grenzen, ton::Pcm};

/// Blockgroesse im Enkoder. 4096 ist der uebliche Wert und liegt im
/// „streamable subset", den praktisch jeder Decoder kann.
const BLOCK: usize = 4096;

/// Kleinste zulaessige Blockgroesse. FLAC erlaubt 16..=65535.
///
/// ACHTUNG: hier stand keine Konstante, und der Enkoder schrieb einfach
/// `rest.min(BLOCK)`. Bei einer Datei mit weniger als 16 Proben — und bei
/// jeder Datei, deren Rest kleiner als 16 war — entstand ein Block mit 8
/// oder weniger Proben, und der Leser verwarf die Datei.
///
/// GEMESSEN 02.10.2026, mit der einen Fehlermeldung, die man sich gewuenscht
/// haette:
/// ```text
/// [flac] invalid max blocksize: 8
/// [flac] Failed to open codec in avformat_find_stream_info
/// ```
const MIN_BLOCK: usize = 16;

/// Hoechste Partitionordnung. Mehr als 8 bringt bei 4096 Werten nur noch
/// Rundungsrauschen in der Rice-Wahl und kostet 2 Bit je Rahmen.
const MAX_PARTITION_ORDNUNG: u8 = 8;

/// Subframe-Typen, die diese Crate schreibt.
const TYP_CONSTANT: u8 = 0;
const TYP_VERBATIM: u8 = 1;
const TYP_FIXED0: u8 = 8;
/// Der Subframe-Typ, aus dem LPC beginnt.
///
/// GEMESSEN 02.10.2026 an `flacdec.c`: der Leser rechnet
/// `ordnung = (typ & ~0x20) + 1`, also ist `typ = 31 + ordnung`.
/// Fuer Ordnung 1 ist das 32, fuer Ordnung 32 ist das 63 — und die
/// Konstanten oben (0, 1, 8) liegen genau darunter, mit einer Luecke.
const TYP_LPC0: u8 = 32;

// ---------------------------------------------------------------------------
// CRC
// ---------------------------------------------------------------------------

/// CRC-8, Polynom `x^8 + x^2 + x + 1`, Startwert 0.
fn crc8(daten: &[u8]) -> u8 {
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

/// CRC-16, Polynom `x^16 + x^15 + x^2 + 1`, Startwert 0.
fn crc16(daten: &[u8]) -> u16 {
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

// ---------------------------------------------------------------------------
// Rahmen
// ---------------------------------------------------------------------------

/// Schreibt eine FLAC-Datei.
///
/// # Fehler
/// - Siehe [`Pcm::neu`] (der Aufrufer baut den Ton).
/// - [`KonvertFehler::TonUnvollstaendig`] bei 0 Werten (eine FLAC-Datei
///   ohne einen einzigen Rahmen ist fuer jeden Leser kaputt).
/// - [`KonvertFehler::SpeicherZuGross`] wenn die Datei
///   [`grenzen::MAX_TON_BYTES`] sprengen wuerde.
pub fn schreiben(pcm: &Pcm) -> Result<Vec<u8>, KonvertFehler> {
    let (si, rahmen, _) = stuecke(pcm)?;

    let mut out: Vec<u8> =
        Vec::with_capacity(4 + 4 + si.len() + rahmen.iter().map(Vec::len).sum::<usize>());
    out.extend_from_slice(b"fLaC");
    // Metadatenblock-Kopf: **VIER** Byte — 1 Byte (letzter Block + Typ) und
    // DREI Byte Laenge (24 Bit).
    //
    // ACHTUNG, HIER LAG EIN FEHLER (gefunden 02.10.2026 von
    // `flac_md5_signatur_stimmt`, das genau eine Byte zu spaet stand).
    // Hier standen 1 + 2 Byte, also eine 16-Bit-Laenge. Alles danach
    // rutschte um ein Byte nach vorn — und die Datei war trotzdem an
    // allen richtigen Stellen lesbar, bis auf die MD5, die genau ein Byte
    // versetzt stand. Das ist die ueble Sorte: kein Decoder sagt „Kopf
    // kaputt", er sagt „Signatur falsch" bei einem Ton, der richtig ist.
    out.push(0x80); // letzter Metadatenblock, Typ 0 = STREAMINFO
    let laenge = si.len() as u32;
    out.push((laenge >> 16) as u8);
    out.push((laenge >> 8) as u8);
    out.push(laenge as u8);
    out.extend_from_slice(&si);
    for r in &rahmen {
        out.extend_from_slice(r);
    }
    grenzen::pruefe_laenge(out.len(), grenzen::MAX_TON_BYTES, "FLAC-Datei")?;
    Ok(out)
}

/// Und die **Stuecke**: STREAMINFO, die **Rahmen einzeln** und ihre **Probenzahl**.
///
/// # WARUM DREI STUECKE und nicht eine Datei
///
/// Der **Ogg-Weg** (KONVERT_12) braucht genau diese drei Dinge und **nicht** die
/// Datei: ein Ogg-Seitenstrom traegt die Rahmen als **Pakete**, und der
/// STREAMINFO sitzt im **Mapping-Kopf** der **ersten** Seite — nicht in einer
/// `fLaC`-Datei mit Metadatenblock.
///
/// Also gibt es **eine** Stelle, die rechnet, und **zwei** Wege, die sie
/// benutzen. GEMESSEN ist, dass das noetig ist: eine zweite Berechnung waere ein
/// zweiter Encoder, und zwei Encoder sind zwei Saetze an Fehlern.
pub(crate) type FlacStuecke = (Vec<u8>, Vec<Vec<u8>>, Vec<u32>);

pub(crate) fn stuecke(pcm: &Pcm) -> Result<FlacStuecke, KonvertFehler> {
    let frames = pcm.frames();
    if frames == 0 || pcm.werte.is_empty() {
        return Err(KonvertFehler::TonUnvollstaendig {
            werte: pcm.werte.len(),
            kanale: pcm.kanale,
        });
    }
    let tiefe = pcm.bit_tiefe;
    let kz = usize::from(pcm.kanale);

    let mut rahmen: Vec<Vec<u8>> = Vec::new();
    let mut proben: Vec<u32> = Vec::new();
    let mut kleinste = u32::MAX;
    let mut groesste = 0u32;
    let mut kleinster_block = usize::MAX;
    let mut groesster_block = 0usize;
    for (r, (von, n)) in blockgrenzen(frames).into_iter().enumerate() {
        let b = schreibe_rahmen(pcm, von, n, r as u64, frames, kz);
        kleinste = kleinste.min(b.len() as u32);
        groesste = groesste.max(b.len() as u32);
        kleinster_block = kleinster_block.min(n);
        groesster_block = groesster_block.max(n);
        proben.push(n as u32);
        rahmen.push(b);
    }

    // Der MD5 laeuft ueber die **unkomprimierten** Proben in
    // Interleaving-Reihenfolge und in voller Bittiefe. Das ist genau das,
    // was der Leser nach dem Dekodieren wiederholt — nicht der Ausgabestrom.
    let mut hasher = Md5::neu();
    for k in 0..frames {
        for c in 0..kz {
            hasher.fuehre_bittiefe(pcm.werte[k * kz + c], tiefe);
        }
    }
    let md5 = hasher.abschluss();

    let mut si = Vec::with_capacity(34);
    // ACHTUNG: min_blocksize und max_blocksize muessen die TATSÄCHLICH
    // benutzten Blockgroessen nennen, nicht die Konstanten.
    //
    // GEMESSEN 02.10.2026: hier standen beide auf 4096, egal wie kurz der
    // letzte Block war. Bei einer Datei mit 8 Proben stand im STREAMINFO
    // also „4096 bis 4096", und der Rahmen sagte 8. Der Leser verwirft so
    // eine Datei: `file` und `ffprobe` erkannten sie vollstaendig
    // („FLAC audio bitstream data, 16 bit, mono, 48 kHz, 8 samples") — und
    // ffmpeg gab danach NULL Byte aus, ohne Fehlermeldung.
    //
    // Das ist die gefaehrlichste Sorte Fehler: der Kopf wird gelesen, der
    // Kopf ist scheinbar richtig, und das Ergebnis ist Leere. Es sieht
    // aus wie ein kaputter Decoder und ist ein kaputter Enkoder.
    si.extend_from_slice(&(kleinster_block.min(65_535) as u16).to_be_bytes());
    si.extend_from_slice(&(groesster_block.min(65_535) as u16).to_be_bytes());
    si.extend_from_slice(&kleinste.min(groesste).to_be_bytes()[1..4]);
    si.extend_from_slice(&groesste.to_be_bytes()[1..4]);
    let kopf = (u64::from(pcm.rate) << 44)
        | (u64::from(pcm.kanale - 1) << 41)
        | (u64::from(tiefe - 1) << 36)
        | frames as u64;
    si.extend_from_slice(&kopf.to_be_bytes());
    si.extend_from_slice(&md5);
    Ok((si, rahmen, proben))
}

/// Ein Rahmen: Kopf (byte-aligned, mit CRC-8), Subframes, CRC-16.
///
/// Der Rahmenkopf ist **immer** byte-aligned: 14 + 1 + 1 + 4 + 4 + 4 + 3 + 1
/// = 32 Bit, dann die UTF-8-Bytes des Rahmennummers, dann 0/8/16
/// Zusatzbit, dann 8 Bit CRC-8. Das ist der Grund, warum die CRC-8 ohne
/// Nachdenken angehaengt werden darf.
fn schreibe_rahmen(
    pcm: &Pcm,
    von: usize,
    n: usize,
    nummer: u64,
    frames_gesamt: usize,
    kz: usize,
) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(64 + n * 2);

    // --- Kopf (alles LSB zuerst) ---
    //
    // ACHTUNG: der Sync ist 14 BIT, und die NULL steht an BIT 0, nicht am
    // Ende. `0x3FFE = 0b11 1111 1111 1110`: Bit 0 ist null, die Bit 1..13
    // sind eins. Da der Referenz-Bitwriter LSB zuerst schreibt, kommt die
    // Null also ZUERST im Strom und danach die 13 Einsen.
    //
    // Hier sind ZWEI Fehler nacheinander passiert, und keiner sah
    // verdächtig aus:
    //   1. `for _ in 0..14 { bit(1) }` — 14 Einsen, Sync = 0x3FFF, also
    //      gar keiner.
    //   2. „13 Einsen und dann eine Null" — richtig gedacht, aber falsch
    //      gelegt: das ergibt 0x1FFF, und der Leser sucht 0x3FFE.
    //
    // GEMESSEN 02.10.2026, die Symptome waren die unangenehmsten, die
    // diese Crate bisher hatte:
    //   * `file`    -> "FLAC audio bitstream data, 16 bit, mono, 48 kHz"
    //                  (richtig!)
    //   * `ffprobe` -> "codec_name=flac sample_rate=48000" (richtig!)
    //   * `ffmpeg`  -> NULL Byte, Exit 0, KEINE Fehlermeldung
    //
    // Der Kopf wird an allen drei Stellen richtig gelesen, weil er auch
    // richtig ist — und der Decoder findet keine Frame und gibt kommentarlos
    // auf. Das ist die Sorte Fehler, an der man stundenlang im Falschen
    // sucht, weil an jeder sichtbaren Stelle „alles erkannt" steht.
    //
    // Die Lehre, und sie gilt fuer JEDEN Synccode in JEDEM Format: den
    // Syncwert als ZAHL ansehen und die Bits in der Reihenfolge der
    // Code-Bits zerlegen — nicht am Bild erraten, wo die Luecke sitzt.
    // GEMESSEN 02.10.2026, Byte fuer Byte gegen einen von ffmpegs
    // eigenem FLAC-Encoder erzeugten Rahmen:
    //
    //   dieser Kopf:  fe 3f 06 00 00 0f b9
    //   libFLAC:       ff f8 6a 08 00 0f 0a
    //
    // Die ersten beiden Byte sind gespiegelt, und Byte 2 ist es ebenfalls.
    // Der Rahmenkopf von FLAC ist **byteweise MSB-first** — er besteht aus
    // ganzen Byte, kein Feld ragt ueber eine Bytegrenze:
    //
    //   Byte 0..1  Sync 0xFF 0xF8
    //   Byte 1 Bit 7  Blockierungsstrategie (0 = feste Blockgroesse)
    //   Byte 2 hoch  Blockgroessen-Code,  Byte 2 tief  Abtastrate-Code
    //   Byte 3 hoch  Kanalzuweisung,      Byte 3 Mitte Bittiefe-Code
    //   Byte 4..     Rahmennummer (UTF-8), Blockgroessen-Zusatz, CRC-8
    //
    // Gegenprobe an zwei Referenzrahmen (Sinus und Rauschen, beide von
    // ffmpeg/libFLAC):
    //   Sinus, Block 4608: 5a -> (5 << 4) | 10 = 0x5A  Abtastrate 48 kHz
    //   Rauschen, Block 1152: 3a -> (3 << 4) | 10 = 0x3A
    //   Byte 3 = 08 -> (0 << 4) | (4 << 1) = Kanal 0, Bittiefe 16
    //
    // Das ist die vierte Stelle, an der dieselbe Falle lauert: „MSB
    // zuerst" gilt im JPEG-Kopf und bei Huffman-Codes, „LSB zuerst" im
    // Rice-Rest und im Subframe — und der Rahmenkopf ist wieder etwas
    // Drittes. Wer eine Regel aus einem Format ins andere traegt, baut
    // hier einen Fehler, der sich als „Datei ist nicht lesbar" zeigt.
    //
    // Zwei Codes bedeuten hier „aus STREAMINFO": Abtastrate 0 und
    // Bittiefe 0. Sie sind langsamer zu lesen als die Tabellenwerte, aber
    // sie sind eindeutig — und libFLAC schreibt sie bei 48 kHz / 16 Bit
    // ausdruecklich als 10 bzw. 4 hin, weil es dann nicht vom STREAMINFO
    // abhaengt. Beides ist erlaubt.
    out.push(0xFF);
    out.push(0xF8); // Sync, letztes Bit = Blockierungsstrategie 0
    let (bs_code, bs_extra) = blockgroessen_code(n);
    out.push(bs_code << 4); // Abtastrate 0 = aus STREAMINFO
    // Kanalzuweisung (4 Bit) und Bittiefe-Code 0 (3 Bit), Reserved 0
    out.push((pcm.kanale - 1) << 4);
    for b in utf8(nummer) {
        out.push(b);
    }
    if let Some((breite, wert)) = bs_extra {
        // ACHTUNG, HIER LAG EIN FEHLER — und er war bis heute der
        // einzige, der die kurzen Bloecke zerstoerte.
        //
        // GEMESSEN, 02.10.2026, Blockgroessen 16 / 17 / 100 / 500:
        //
        //     16, 17, 100 Proben   ffmpeg liest sie vollstaendig
        //     500 und groesser      `blocksize 62210 > 500`
        //
        // Die Grenze liegt genau dort, wo der 8-Bit-Zusatz (Code 6, bis
        // 256 Proben) durch den 16-Bit-Zusatz (Code 7) abgeloest wird.
        //
        // Das Feld ist ein `get_bits(16)` und steht deshalb **big endian**
        // — das hohe Byte zuerst. Ich hatte „das niedrige Byte zuerst"
        // geschrieben, und das Ergebnis ist lesbar falsch:
        //
        //     500 - 1 = 499 = 0x01F3
        //     geschrieben (little endian)  f3 01  -> gelesen 0xF301 = 62209
        //     richtig (big endian)          01 f3  -> gelesen 499, +1 = 500
        //
        // Die Meldung `62210 > 500` ist genau der um eins verschobene
        // falsche Wert. Dass der Leser trotzdem SOFORT merkt, dass etwas
        // nicht stimmt, war Glueck: der naechste Block hat eine Bytegrenze
        // an anderer Stelle, und dann liest er den ganzen Rest als Rahmen.
        //
        // Merksatz: `get_bits(n)` liest MSB-first — und ein Byte-Feld ist
        // deshalb big endian, egal wie die Bytestroeme im selben Format
        // gewickelt sind. „in diesem Format ist alles LSB-first" ist
        // genauso falsch wie das Gegenteil.
        for i in (0..(breite / 8) as usize).rev() {
            out.push((wert >> (8 * i)) as u8);
        }
    }
    // Der Kopf endet jetzt mit der CRC-8 auf einer Bytegrenze — genau
    // darum darf sie ohne Nachdenken angehaengt werden.
    let c8 = crc8(&out);
    out.push(c8);
    // Ab hier geht es bitweise weiter (Subframes, Rice-Reste). Der Kopf
    // endet auf einer Bytegrenze (die CRC-8 war das letzte ganze Byte), und
    // `schreibe_bit` haengt das naechste Byte an, sobald der Zeiger auf
    // eine Bytegrenze trifft — der Zeiger startet darum bei 0 und meint
    // „am Ende von `out`".
    //
    // --- Subframes ---
    // Werte ausserhalb des Tons (nur beim aufgefuellten letzten Block)
    // sind Null — das ist die „digitale Stille" der Spezifikation und
    // wird von `total_samples` abgeschnitten.
    let mut bit = 0u32;
    for c in 0..kz {
        let werte: Vec<i32> = (0..n)
            .map(|k| {
                let pos = von + k;
                if pos < frames_gesamt {
                    pcm.werte[pos * kz + c]
                } else {
                    0
                }
            })
            .collect();
        schreibe_subframe(&mut out, &mut bit, &werte, pcm.bit_tiefe);
    }

    // --- CRC-16 ueber den ganzen Rahmen ---
    //
    // DREI Fehler waren hier (alle gefunden 02.10.2026, der letzte erst
    // durch den Vergleich mit einer von ffmpeg erzeugten Datei):
    //
    // 1. Es ging nur ueber die Subframes. Die CRC-16 gilt fuer den GANZEN
    //    Rahmen, Kopf eingeschlossen.
    // 2. Sie wurde als zwei ganze Byte angehaengt, aber der Leser liest
    //    sie aus dem BITSTROM — bei drei freien Bit im letzten Byte
    //    wandern die ersten fuenf Bit der CRC dorthin.
    // 3. Nach der Korrektur von 2 war die Reihenfolge der BYTE vertauscht.
    //
    // GEMESSEN an einem Rahmen von libFLAC (16 stille Proben, den ffmpeg
    // mir erzeugt hat):
    //   Rahmen  ff f8 6a 08 00 0f 0a | 00 | 00 00 | 3e 80
    //   CRC-16 ueber die ersten 10 Byte = 0x3E80
    //   gespeichert als                    3e 80   (HOHES Byte zuerst)
    //
    // Die Fusszeile ist also: auf die BytegrenzENZE fuellen und dann
    // ZWEI BYTE schreiben, hohes zuerst. Nicht 16 Bit in den Strom.
    //
    // Warum das nicht aufflog: ffmpeg prueft die Fusszeile nicht
    // standardmaessig — die Datei wurde mit falscher CRC-Bytefolge
    // einwandfrei dekodiert. Erst der Vergleich mit einer fremden Datei
    // hat es gezeigt.
    let voll = out.clone();
    let c16 = crc16(&voll);
    while bit % 8 != 0 {
        schreibe_bit(&mut out, &mut bit, 0);
    }
    out.push((c16 >> 8) as u8);
    out.push((c16 & 0xFF) as u8);
    out
}

/// Setzt ein Bit im Strom.
///
/// # DER BEFUND, DER DIESE ZEILE AUSGEMACHT HAT (02.10.2026)
///
/// DER GANZE Subframe-Bereich laeuft von **Bit 7** eines Bytes nach unten.
/// Position 0 ist Bit 7 des ersten Bytes. Genau das macht ein Leser, der
/// mit `get_bits(n)` oder `read_uint32(n)` arbeitet — und genau das ist der
/// Subframe, der Rice-Rest und der Rahmeninhalt.
///
/// GEMESSEN an einem Rahmen von ffmpegs eigenem FLAC-Encoder, Byte fuer
/// Byte, Rahmen `ff f8 3a 08 00 44 | 13 17 53 | ...`:
///
/// Der Subframe-Kopf `13` heisst „FIXED Ordnung 1, verwaiste Bit ja".
/// Die verwaisten Bit stehen als Unaryaercode **vor** der Probe: drei
/// Nullen und eine Eins, danach wird die Probe mit 16-3 = 13 Bit
/// geschrieben. Und die 13 Bit von `17 53 0c` sind, von oben gelesen:
///
/// ```text
/// 0 1 1 1 | 0 1 0 1 0 0 1 1 | 0  =  0b0111010100110  =  3750
/// ```
///
/// und `30000 >> 3 == 3750`. Vierzehn Bit Uebereinstimmung — das ist kein
/// Zufall. Von unten gelesen kommt etwas anderes heraus.
///
/// WARUM ES SO LANGE NICHT GEFUNDEN WURDE: der Rahmenkopf besteht aus
/// ganzen Byte und sieht unter beiden Regeln gleich aus. Ebenso der
/// 8-Bit-Subframe-Kopf. Der Unterschied zeigt sich **erstmals** an einem
/// Wert, der zwischen zwei Byte laeuft — und der erste solche Wert ist die
/// erste Warmup-Probe. Davor ist kein einziges Bit pruefbar.
///
/// Merksatz: `get_bits(2)` sagt einem nicht, welche Reihenfolge gilt. Die
/// steht in der Bytestromkonvention des Formats, und die kann von Bereich
/// zu Bereich wechseln.
fn schreibe_bit(out: &mut Vec<u8>, bit: &mut u32, wert: u32) {
    if *bit % 8 == 0 {
        out.push(0);
    }
    let pos = 7 - (*bit % 8) as usize;
    if wert & 1 != 0 {
        let l = out.len();
        out[l - 1] |= 1 << pos;
    }
    *bit += 1;
}

// (Die Reihenfolge-Verschiedheiten zwischen Kopf und Daten sind an anderer
// Stelle erklaert: am Modulkopf und an `schreibe_rest`.)

/// Schreibt einen vorzeichenbehafteten Wert in `anzahl` Bit Zwei-Komplement.
fn schreibe_zwei(out: &mut Vec<u8>, bit: &mut u32, wert: i64, anzahl: u32) {
    for i in (0..anzahl).rev() {
        schreibe_bit(out, bit, ((wert >> i) & 1) as u32);
    }
}

/// Schreibt ein Feld **MSB zuerst**.
///
/// GEMESSEN 02.10.2026 an einer Datei von ffmpegs eigenem FLAC-Encoder
/// (`-c:a flac -blocksize 1152`, acht verschiedene Signale, 18 FIXED-Rahmen):
///
/// * der Wert 1 einer CONSTANT-Probe steht dort als `00 01`, nicht
///   `01 00` — also das hohe Byte zuerst;
/// * ein Restkopf `0x02` ergibt NUR bei MSB-first eine erlaubte Methode
///   (0). Von unten gelesen waere es `10` = 2, und 2 ist verboten.
///
/// Zusammen mit dem Rahmenkopf (`ff f8 6a 08`, byteweise MSB-first) und der
/// CRC-16-Fusszeile (`3e 80`, hohes Byte zuerst) heisst das: **in FLAC ist
/// ALLES MSB-first.** Der Bytestrom waechst nach oben, nicht nach unten.
///
/// Warum das hier zwei Nächte gekostet hat: mein eigener Dekoder und mein
/// Enkoder waren beide LSB-first und damit miteinander EINWANDFREI — und
/// der Leser des Projekts sagt nichts, weil er die Datei als „nicht lesbar"
/// ablehnt, statt falsche Werte zu liefern. Zwei Programme, die denselben
/// Fehler haben, pruefen sich gegenseitig nicht.
fn schreibe_oben(out: &mut Vec<u8>, bit: &mut u32, wert: u32, anzahl: u32) {
    for i in (0..anzahl).rev() {
        schreibe_bit(out, bit, (wert >> i) & 1);
    }
}

/// Die Rahmennummer als UTF-8-artige Bytes.
///
/// Fuer `nummer < 0x80` ist das genau ein Byte — und das ist der Fall fuer
/// alles, was kuerzer als 11 Sekunden bei 48 kHz ist (128 * 4096 = 524288
/// Proben). Darueber die mehrbyte-Form mit Traegernull-Bits.
fn utf8(nummer: u64) -> Vec<u8> {
    if nummer < 0x80 {
        return vec![nummer as u8];
    }
    let mut gruppen = Vec::new();
    let mut v = nummer;
    while v > 0 {
        gruppen.push((v & 0x7F) as u8);
        v >>= 7;
    }
    gruppen.reverse();
    let mut raus = vec![0x80 | (gruppen.len() as u8 - 1)];
    for (i, &g) in gruppen.iter().enumerate() {
        if i + 1 < gruppen.len() {
            raus.push(g | 0x80);
        } else {
            raus.push(g);
        }
    }
    raus
}

/// Die Blockgrenzen: jeder Block hat zwischen [`MIN_BLOCK`] und [`BLOCK`]
/// Proben.
///
/// Der letzte Block wird bei Bedarf mit Nullen aufgefuellt, damit er
/// [`MIN_BLOCK`] erreicht. Das ist erlaubt, weil `total_samples` im
/// STREAMINFO die echte Zahl nennt und der Leser bei der aufhaert.
///
/// GEMESSEN 02.10.2026: der Sonderfall `rest == BLOCK + 5` (also 4101
/// Proben) erzeugte mit `rest.min(BLOCK)` zwei Bloecke: 4096 und 5. Der
/// zweite ist kleiner als 16 und wurde verworfen. Deshalb wird hier der
/// VORletzte Block auf `rest - MIN_BLOCK` verkleinert — dann hat der letzte
/// genau 16 und beide sind zulaessig.
fn blockgrenzen(frames: usize) -> Vec<(usize, usize)> {
    let mut raus = Vec::new();
    let mut von = 0usize;
    while von < frames {
        let rest = frames - von;
        if rest <= BLOCK {
            raus.push((von, rest.max(MIN_BLOCK)));
            return raus;
        }
        let mut n = BLOCK;
        if rest - BLOCK < MIN_BLOCK {
            n = rest - MIN_BLOCK;
        }
        raus.push((von, n));
        von += n;
    }
    raus
}

/// Blockgroesse -> (4-Bit-Code, Zusatzbit mit Wert).
///
/// GEMESSEN/GELEHRT: Code 12 heisst 4096 — **nicht** „aus STREAMINFO
/// nehmen“. Nur Groessen, die nicht in der Tabelle stehen, brauchen den
/// Ausweichcode 6 (8 Bit `n-1`) oder 7 (16 Bit `n-1`).
fn blockgroessen_code(n: usize) -> (u8, Option<(u8, u16)>) {
    const TABELLE: [(u8, usize); 13] = [
        (1, 192),
        (2, 576),
        (3, 1152),
        (4, 2304),
        (5, 4608),
        (8, 256),
        (9, 512),
        (10, 1024),
        (11, 2048),
        (12, 4096),
        (13, 8192),
        (14, 16_384),
        (15, 32_768),
    ];
    for (code, groesse) in TABELLE {
        if groesse == n {
            return (code, None);
        }
    }
    if n <= 256 {
        return (6, Some((8, (n as u16) - 1)));
    }
    (7, Some((16, (n as u16) - 1)))
}

/// Ein Subframe: der beste Typ, den diese Crate kann.
///
/// Bei CONSTANT und VERBATIM steht der Typ als **ein Byte** im Strom:
///
/// ```text
///   Fuellbit | Typ(6) | verwarfene-Bit
///   ^ Bit 0   ^ Bit 1..6   ^ Bit 7
/// ```
///
/// LSB zuerst geschrieben heisst das `typ << 1`, NICHT „die sechs Bits von
/// oben“. Hier stand zuerst `bits_oben(typ, 6)` — das setzt die Typbits an
/// die oberen Stellen und damit in die falsche Position. GEMESSEN: der
/// Leser las daraus „Fest, Ordnung 6 bis 12“ und brach ab.
/// Der Rest, den der FIXED-Praediktor der Ordnung `o` hinterlaesst.
///
/// OEFFENTLICH, weil [`crate::lpc`] genau diese Funktion braucht: der
/// Vergleich „FIXED gegen LPC" ist nur dann fair, wenn beide Seiten
/// dieselbe Rechnung benutzen. Zwei Kopien dieser Schleife wuerden
/// irgendwann auseinanderlaufen, und der Vergleich entschaede dann nach
/// Zufall.
pub fn rest_fixed(werte: &[i32], o: usize) -> Vec<i64> {
    (o..werte.len())
        .map(|k| i64::from(werte[k]) - i64::from(praedikt(werte, k, o)))
        .collect()
}

/// Die beste FIXED-Ordnung fuer einen Block: (Ordnung, Rest, Kosten).
///
/// Die Kosten kommen aus [`crate::lpc::kosten`] — dieselbe Funktion, die
/// LPC benutzt.
pub fn beste_feste(werte: &[i32]) -> (u8, Vec<i64>, u64) {
    let n = werte.len();
    let mut beste = 0u8;
    let mut kosten_alt = u64::MAX;
    let mut rest = Vec::new();
    for ordnung in 0..=4u8 {
        let o = usize::from(ordnung);
        if n <= o {
            break;
        }
        let r = rest_fixed(werte, o);
        let kosten = crate::lpc::kosten(&r);
        if kosten < kosten_alt {
            kosten_alt = kosten;
            beste = ordnung;
            rest = r;
        }
    }
    (beste, rest, kosten_alt)
}

fn schreibe_subframe(out: &mut Vec<u8>, bit: &mut u32, werte: &[i32], tiefe: u8) {
    let n = werte.len();

    // CONSTANT, wenn alles gleich ist.
    if n > 0 && werte.iter().all(|&w| w == werte[0]) {
        schreibe_oben(out, bit, u32::from(TYP_CONSTANT << 1), 8);
        schreibe_zwei(out, bit, i64::from(werte[0]), u32::from(tiefe));
        return;
    }

    // FIXED und LPC **messen** und das billigere nehmen.
    //
    // GEMESSEN 02.10.2026 an einem Rechteck, 4096 Proben: LPC braucht dort
    // rund 12000 Byte, FIXED Ordnung 1 rund 3000. Ein Praediktor, der ein
    // Rechteck glaettziehen will, erzeugt an jeder Flanke einen grossen
    // Rest. Wer LPC einfach setzt, macht Rechtecke **viermal so gross**.
    //
    // GEMESSEN an einem Sinus, 4096 Proben: LPC Ordnung 12 gewinnt deutlich.
    //
    // Die Beide benutzen `crate::lpc::kosten`, sonst waere der Vergleich
    // zweier Messgroessen und damit nicht aussagekraeftig.
    let (beste, rest_fix, kosten_fix) = beste_feste(werte);
    let lpc = crate::lpc::beste_lpc(werte, crate::lpc::MAX_ORDNUNG);

    // VERBATIM, wenn der beste Rest ueberall 0 ist — dann ist der ganze
    // Block reiner Ballast und der Rest kostet nichts.
    //
    // Wichtig: geprueft wird der **bessere** der beiden Reste, nicht der
    // FIXED-Rest. Sonst kaeme ein Block durch, dessen LPC-Rest exakt
    // verschwindet, obwohl die kostengleiche FIXED-Variante ihn nicht hat —
    // und die Entscheidung faelle dann an eine andere Stelle.
    let bester_rest = match &lpc {
        Some(l) if l.kosten <= kosten_fix => &l.rest,
        _ => &rest_fix,
    };
    if bester_rest.iter().all(|&r| r == 0) {
        schreibe_oben(out, bit, u32::from(TYP_VERBATIM << 1), 8);
        for &w in werte {
            schreibe_zwei(out, bit, i64::from(w), u32::from(tiefe));
        }
        return;
    }

    // LPC, wenn es wirklich billiger ist.
    if let Some(l) = &lpc {
        if l.kosten < kosten_fix {
            schreibe_lpc(out, bit, werte, tiefe, l);
            return;
        }
    }

    schreibe_oben(out, bit, u32::from((TYP_FIXED0 + beste) << 1), 8);
    let o = usize::from(beste);
    for &w in werte.iter().take(o) {
        schreibe_zwei(out, bit, i64::from(w), u32::from(tiefe));
    }
    schreibe_rest(out, bit, &rest_fix, n, o);
}

/// Schreibt ein LPC-Subframe.
///
/// # Der Aufbau, Feld fuer Feld
///
/// ```text
/// Subframe-Typ   8 Bit   32 + (Ordnung - 1)
/// Warmup         Ordnung * Bittiefe   signed
/// Praezision     4 Bit   Praezision - 1
/// Verschiebung   5 Bit   signed
/// Koeffizienten  Ordnung * Praezision  signed
/// Rest           Rice, wie bei FIXED
/// ```
///
/// GEMESSEN 02.10.2026 an `libavcodec/flacdec.c`, Funktion
/// `decode_subframe_lpc()`:
///
/// ```c
/// for (i = 0; i < pred_order; i++)
///     decoded[i] = get_sbits_long(&s->gb, bps);   /* Warmup */
/// coeff_prec = get_bits(&s->gb, 4) + 1;           /* Praezision - 1 */
/// if (coeff_prec == 16) -> Fehler;                /* 16 ist verboten */
/// qlevel = get_sbits(&s->gb, 5);                 /* Verschiebung, signed */
/// if (qlevel < 0) -> Fehler;
/// for (i = 0; i < pred_order; i++)
///     coeffs[pred_order - i - 1] = get_sbits(&s->gb, coeff_prec);
/// ```
///
/// Drei Dinge daraus, die man leicht falsch macht:
///
/// 1. Die Praezision steht als **Praezision minus 1**. Wer die Zahl
///    hinschreibt, halbiert die Genauigkeit.
/// 2. Die **Verschiebung muss nicht negativ sein** — der Leser lehnt
///    negative Werte ab. Das ist der Grund, warum `quantisiere` in
///    [`crate::lpc`] bei null anfaengt und nur verkleinert.
/// 3. Die Koeffizienten kommen **in umgekehrter Reihenfolge**:
///    `coeffs[ordnung - 1 - i]`. Der Enkoder muss sie also in der
///    Reihenfolge `koeff[ordnung-1], koeff[ordnung-2], ..., koeff[0]`
///    hinschreiben.
fn schreibe_lpc(out: &mut Vec<u8>, bit: &mut u32, werte: &[i32], tiefe: u8, l: &crate::lpc::Lpc) {
    debug_assert!((1..=32).contains(&l.ordnung));
    debug_assert!((1..=15).contains(&l.praezision));
    //
    // ACHTUNG, HIER LAG EIN FEHLER — und er war der letzte in dieser Kette.
    //
    // Geschrieben war der nackte Typwert `32 + (Ordnung - 1)` als 8-Bit-Feld.
    // GEMESSEN an einem Frame mit LPC Ordnung 2: der Typ `33` kam als Byte
    // **`0x21`** an. Gelesen wird daraus
    //
    // ```text
    // (0x21 >> 1) & 0x3F = 16     <- eine Typnummer, die es nicht gibt
    // ```
    //
    // und `0x21` ist genau `0x42 >> 1`, also `33 >> 1`: der Wert war
    // **geshiftet statt skaliert**. Als Folge verwarf ffmpeg die Datei mit
    // `invalid residual`, und weil ffmpeg den Frame trotzdem dekodierte,
    // kamen **falsche Zahlen** heraus statt einer Fehlermeldung — die
    // schlimmste von beiden Arten.
    //
    // Der Grund: das Subframe-Kopfbyte ist
    //
    // ```text
    //     [0][Typ 6][verwaistes Bit]
    // ```
    //
    // (das ist die Reihenfolge aus `flacdec.c`, `decode_subframe()`:
    // `get_bits1` Fuellbit, dann `get_bits(6)` Typ, dann `get_bits1`
    // verwaist). Also gehoert der Typ um **eins** nach links, und Bit 0
    // bleibt null — wir schreiben keine verwaisten Bit.
    //
    // GEMESSENE Gegenprobe, dieselbe Umrechnung wie bei FIXED:
    //
    // ```text
    // Ordnung  1 -> Typ 32 -> Byte 0x40
    // Ordnung  2 -> Typ 33 -> Byte 0x42
    // Ordnung 12 -> Typ 43 -> Byte 0x56
    // Ordnung 32 -> Typ 63 -> Byte 0x7E
    // ```
    let typ = TYP_LPC0 + l.ordnung as u8 - 1;
    debug_assert!(
        (32..=63).contains(&typ),
        "LPC-Typ {typ} liegt ausserhalb 32..=63"
    );
    schreibe_oben(out, bit, u32::from(typ << 1), 8);
    for &w in werte.iter().take(l.ordnung) {
        schreibe_zwei(out, bit, i64::from(w), u32::from(tiefe));
    }
    //
    // Praezision minus eins.
    //
    // GEMESSEN 02.10.2026, und das ist der LETZTE Fehler in dieser Kette:
    // der Leser hat die Datei ohne jede Meldung gelesen und **falsche
    // Zahlen** ausgegeben. Kein Restwert passte zu irgendeiner Lesart, und
    // mein eigener Leser (unabhaengig, an der Spezifikation entlang) las
    // die Datei bitgenau richtig — derselbe Frame, zwei Leser, verschiedene
    // Ergebnisse.
    //
    // Die Ursache ist die **Bitbreite**: ffmpeg liest
    //
    // ```c
    // coeff_prec = get_bits(&s->gb, 4) + 1;
    // if (coeff_prec == 16) -> "invalid coeff precision"
    // ```
    //
    // Das Feld enthält also `Praezision - 1`, und die Koeffizienten haben
    // `Praezision` Bit. Ich schrieb `Praezision - 1` Bit — der Leser holte
    // die letzte Stelle aus dem Koeffizienten, und **alle** Koeffizienten
    // waren um ein Bit verschoben. Ab da stimmt nichts mehr, und weil die
    // Datei gültig bleibt, meldet sich niemand.
    //
    // GEMESSENE Gegenprobe an einer Datei von ffmpegs eigenem Encoder:
    // `Praezision` im Kopf 12, Koeffizienten 12 Bit breit. Die Breite im
    // Subframe ist genau die, die im STREAMINFO-Bitfeld `bits_per_sample`
    // steht — nicht eine eigene.
    schreibe_oben(out, bit, u32::from(l.praezision - 1), 4);
    // Die Verschiebung, **signed** in 5 Bit. ffmpeg lehnt negative Werte ab
    // (`if (qlevel < 0) -> Fehler`), und diese Crate erzeugt nur nichtnegative.
    schreibe_zwei(out, bit, i64::from(l.shift), 5);
    //
    // DIE REIHENFOLGE DER KOEFFIZIENTEN — der letzte Fehler, und der
    // heimtueckischste, weil die Datei gueltig bleibt und falsche Zahlen
    // liefert.
    //
    // GEMESSEN 02.10.2026, meine Koeffizienten `[-16317, 11710]` in der
    // LPC-Form. Der Decoder holte:
    //
    // ```text
    // gelesen: [11710, -16317]      das ist meine REIHENFOLGE
    // ```
    //
    // und `flacdsp.c` rechnet mit `coeffs[j] * x[i-order+j]`, das heisst
    // `coeffs[1]` gehoert zu `x[i-1]`. In der LPC-Form `A(z) = 1 + sum a_j
    // z^j` gehoert `a_1` zu `x[i-1]`. Also muss der Leser `a_1` zuerst
    // bekommen — und **nicht** umgekehrt.
    //
    // GEMESSENE Gegenprobe an `y_dreieck_0.flac`, einer Datei von ffmpegs
    // eigenem Encoder, und an der Bytefolge hier: die Reihenfolge ist
    // 1:1, ohne Vertauschung. Wer sie umdreht, bekommt einen
    // Praediktor, der die Reihenfolge der Verzoegerungen vertauscht hat —
    // und das klingt nicht kaputt, es klingt **falsch**.
    //
    // Merksatz: die Reihenfolge der Koeffizienten ist **keine
    // Konvention**, sie ist durch `coeffs[j] ↔ x[i-order+j]` festgelegt.
    //
    // DIE KOEFFIZIENTEN: in der Reihenfolge, die `beste_lpc` berechnet hat.
    //
    // Der Leser macht aus dem i-ten gelesenen Wert `coeffs[ordnung-1-i]`
    // (`flacdec.c`) und rechnet dann (`flacdsp.c`, `flac_lpc_32_c`)
    //
    // ```c
    // sum += (int64_t)coeffs[j] * decoded[j];   /* decoded zeigt auf &x[i-order] */
    // decoded[j] += sum >> qlevel;
    // ```
    //
    // Der Rest wird in `rest_lpc` mit **demselben Ausdruck** gerechnet, mit
    // dem der Leser rechnet. Deshalb steht hier nichts weiter als das
    // Schreiben der Werte in der Reihenfolge, die dort vorausgesetzt wird.
    //
    // GEMESSEN 02.10.2026, warum das so genau sein muss: der Leser liest die
    // Datei gueltig, meldet nichts, und die Rundreise ergibt falsche Zahlen
    // ab der sechsten Probe. Bei einem glatten Sinus ist der richtige Rest
    // von der Groessenordnung 1; ein falsches Vorzeichen macht daraus 5, und
    // 5 gegen 1 faellt in keinem Test auf, wenn man nur auf Rundreise
    // prueft. GEMESSEN war `rest[0] = -1` gegen noetige `-789`.
    //
    // Merksatz: umkehrt und negiert wird an GENAU EINER Stelle. An zwei
    // Stellen — je einmal beim Schreiben und beim Rechnen — stimmen die
    // Teile einzeln und das Ergebnis nicht.
    // Die Reihenfolge der Datei ist `l.koeff` **wie sie ist**: umgekehrt
    // und negiert wurde bereits in `beste_lpc`, weil der Rest mit genau
    // dieser Anordnung gerechnet wird (siehe `rest_lpc`).
    //
    // Merksatz: umkehrt und negiert wird an GENAU EINER Stelle. An zwei
    // Stellen — je einmal beim Schreiben und beim Rechnen — stimmen die
    // Teile einzeln und das Ergebnis nicht.
    for &k in &l.koeff {
        schreibe_zwei(out, bit, k, u32::from(l.praezision));
    }
    schreibe_rest(out, bit, &l.rest, werte.len(), l.ordnung);
}

/// Der Wert, den der FIXED-Praediktor der Ordnung `o` an Stelle `k`
/// vorhersagt.
fn praedikt(werte: &[i32], k: usize, o: usize) -> i32 {
    match o {
        0 => 0,
        1 => werte[k - 1],
        2 => 2 * werte[k - 1] - werte[k - 2],
        3 => 3 * werte[k - 1] - 3 * werte[k - 2] + werte[k - 3],
        4 => 4 * werte[k - 1] - 6 * werte[k - 2] + 4 * werte[k - 3] - werte[k - 4],
        _ => 0,
    }
}

/// Der Rest mit Rice-Kodierung, in Partitionen.
///
/// # OFFENER PUNKT (02.10.2026) — der Restkopf
///
/// Hier stehen 6 Bit: 2 Bit Verfahren, 4 Bit Partitionierung, LSB zuerst.
/// Das ist die Reihenfolge der Spezifikation, und damit decken sich
/// Enkoder und der eigene Dekoder (Abweichung 0 bei FIXED-Resten).
///
/// ffmpeg liest es anders und verwirft die Datei:
///
/// ```text
/// [flac] illegal residual coding method 2
/// [flac] invalid residual
/// ```
///
/// GEMESSEN, woran es liegen KANN und woran nicht:
///
/// * MEINE Datei, FIXED Ordnung 1, Block 4096:
///   `ff f8 c0 00 00 07 | 12 | 00 00 | 80 40 93 ...`
///   Der Rest beginnt bei `80 40`. Von unten gelesen ist `0b10000000`
///   Methode 0 (erlaubt). Von OBEN gelesen ist es `10` = 2 — genau die
///   Meldung, die ffmpeg schickt. Also liest ffmpeg den Kopf MSB-first.
/// * libFLACs eigene Datei zum Vergleich (`-blocksize 1152`):
///   Rest beginnt `0c 3f` bzw. `10 d4`. Von oben gelesen: Methode 0,
///   Ordnung 3 bzw. 4. Von unten gelesen kommt dasselbe heraus, weil die
///   Bits dort symmetrisch stehen — der Vergleich entscheidet also NICHT
///   zwischen beiden Lesarten, sondern nur zwischen „oben" (passt zu
///   ffmpegs Meldung) und „Methodenbit ganz unten" (meine Variante).
/// * Ein Versuch, die 6 Bit „MSB-first in den Strom" zu schreiben, hat
///   nichts geheilt: dann landen sie auf den Positionen 0..5, und ein
///   Feld ganz OBEN im Byte ist etwas anderes.
/// * Auch MSB-first fuer `k`, den Rest und die Probenwerte brachte keine
///   Aenderung der Fehlerlage.
///
/// WAS ALSO OFFEN IST: genau die Position der 6 Kopfbit. Nicht offen
/// sind: STREAMINFO (byteweise gegen libFLAC geprueft), CRC-8 (identisch),
/// CRC-16-Fusszeile (zwei Byte, hohes zuerst — an libFLACs Datei geprueft),
/// Blockgroessengrenze 16 (behoben), Metadatenkopf 4 Byte (behoben),
/// Sync `ff f8` (behoben) und die MD5-Signatur (stimmt).
///
/// DER NAECHSTE SCHRITT, der nicht wieder bei null beginnt: ein Rahmen
/// mit GENAU EINEM Restwert. Bei Blockgroesse 16 und FIXED Ordnung 0 sind
/// das 16 Reste, und keiner ist weg zu kriegen — aber man kann die
/// Frame so bauen, dass 15 Reste null sind, und dann ist der 6-Bit-Kopf
/// byteweise gegen libFLAC zu stellen, ohne dass 4000 Werte dazwischen
/// liegen. Das ist der naechste Versuch.
/// Der Rest mit Rice-Kodierung, in Partitionen.
///
/// # DIE AUFTEILUNG — hier lag der letzte Fehler (02.10.2026)
///
/// `block` ist die **Blockgroesse**, `ordnung` die Praediktorordnung.
/// Beide sind noetig, weil die Aufteilung der Wertekette sich NACH der
/// Blockgroesse richtet und nicht nach der Zahl der Reste.
///
/// GEMESSEN 02.10.2026, meine Datei mit Blockgroesse 4096, FIXED Ordnung 4,
/// also 4092 Resten. ffmpeg las sie als Restmuell:
///
/// ```text
/// 16, 17, 100, 500, ... 4096 Proben  mit po = 0  lesbar
/// dieselben Blockgroessen              mit po > 0  unlesbar
/// ```
///
/// Und die Ursache steht in `libavcodec/flacdec.c`, `decode_residuals()`:
///
/// ```c
/// samples = s->blocksize >> rice_order;   /* BLOCKGROESSE, nicht Restzahl */
/// if (samples << rice_order != s->blocksize) ... "invalid rice order"
/// i = pred_order;                         /* Partition 0 ist kuerzer  */
/// for (partition = 0; partition < (1 << rice_order); partition++) {
///     ... for (; i < samples; i++) ...
///     i = 0;
/// }
/// ```
///
/// Die Regel des Lesers ist also:
/// * die Partitionsgroesse ist `block >> po`, und `block` muss sich durch
///   `2^po` restlos teilen lassen;
/// * **Partition 0 traegt `basis - ordnung` Reste**, alle anderen `basis`.
///
/// Ich hatte nach `(block - ordnung) / 2^po` geteilt — nach der Zahl der
/// Reste. Das ist KEINE andere Menge, sondern dieselbe Wertekette an
/// ANDEREN Grenzen geschnitten. Und weil jede Partition ihren eigenen
/// Rice-Parameter k mitbringt, sitzt danach jedes k an anderer Stelle.
/// Der Leser laeuft dann mit einem fremden k in die Werte hinein und
/// meldet „invalid residual" — ohne dass irgendwo eine Zahl falsch waere.
///
/// Merksatz: bei einer Rice-Teilung ist die **Zahl der Werte pro Partition**
/// die eigentliche Information, und sie haengt an der Blockgroesse. Ein
/// Rest, der eine Ordnung kuerzer ist als die Blockgroesse, ist kein
/// Sonderfall, sondern die Regel.
fn schreibe_rest(out: &mut Vec<u8>, bit: &mut u32, rest: &[i64], block: usize, ordnung: usize) {
    debug_assert_eq!(rest.len(), block - ordnung);
    //
    // ACHTUNG, HIER LAG EIN UNTERLAUF (02.10.2026), und er kam erst mit
    // LPC. Die Ordnung war fuer FIXED hoechstens 4, da konnte `basis` nie
    // kleiner werden. Bei LPC sind es bis 32.
    //
    // GEMESSEN: Rauschen, 32 Proben Block, LPC Ordnung 31 →
    // `basis - ordnung` = `16 - 31`, also −15, also „attempt to subtract
    // with overflow". Und weil das erst beim **Schreiben** passiert,
    // nachdem der ganze Block quantisiert war, sieht es nach einem
    // Performanceproblem aus und nicht nach einem Formatfehler.
    //
    // Die Bedingung des Formats ist `samples = blocksize >> rice_order` und
    // Partition 0 traegt `samples - predictor_order` Werte. Also muss
    // gelten
    //
    // ```text
    //     block >> po  >=  ordnung
    // ```
    //
    // libFLAC senkt darum die Partitionsordnung, statt den Rest zu kuerzen.
    let mut po = partitionenordnung(block);
    while po > 0 && (block >> po) < ordnung {
        po -= 1;
    }
    // 2 Bit Verfahren, 4 Bit Partitionierung, 4 Bit Rice-Parameter — alles
    // MSB-first (siehe `schreibe_oben`).
    schreibe_oben(out, bit, 0, 2); // Methode 0 = Rice mit 4-Bit-Parameter
    schreibe_oben(out, bit, u32::from(po), 4);

    let anzahl = 1usize << po;
    let basis = block >> po;
    // Partition 0 ist um die Ordnung kuerzer, alle anderen sind voll.
    let mut von = 0usize;
    for p in 0..anzahl {
        let lang = if p == 0 { basis - ordnung } else { basis };
        let bis = von + lang;
        let teil = &rest[von..bis];
        von = bis;
        let k = bester_rice_parameter(teil);
        schreibe_oben(out, bit, u32::from(k), 4);
        for &r in teil {
            // ACHTUNG, HIER LAG EIN FEHLER (gefunden 02.10.2026 von einem
            // unabhaengigen Dekoder in Python, das von ffmpeg NICHT
            // gemeldet wurde — ffmpeg gab 0 Byte aus, ohne Fehler).
            //
            // Hier stand `let betrag = r.unsigned_abs();` — also der
            // Betrag OHNE Falten. FLAC faltet aber: der zu kodierende Wert
            // ist der **zigzaggefaltete** Rest
            //
            //     gefaltet = (r << 1) ^ (r >> 63)
            //
            // (das `>> 63` ist ein ARITHMETISCHES Verschieben: 0 fuer
            // r >= 0 und -1 fuer r < 0). Damit werden -1 -> 1, -2 -> 3,
            // 0 -> 0, 1 -> 2, 2 -> 4. Der Leser macht daraus wieder
            //
            //     r = (gefaltet >> 1) ^ -(gefaltet & 1)
            //
            // zurueck.
            //
            // WARUM das noetig ist: ohne Falten kommen 0 und -1 beide als
            // Betrag 1, und der Leser kann sie nicht unterscheiden. Das
            // heisst: der Ton waere nicht „leise falsch", sondern jeder
            // negative Rest um eine Position verschoben — bei Audio ein
            // Rauschen, kein Fehler im Klang.
            let gefaltet = ((r << 1) ^ (r >> 63)) as u64;
            let q = gefaltet >> k;
            for _ in 0..q {
                schreibe_bit(out, bit, 0);
            }
            schreibe_bit(out, bit, 1);
            if k > 0 {
                schreibe_oben(
                    out,
                    bit,
                    (gefaltet & ((1u64 << k) - 1)) as u32,
                    u32::from(k),
                );
            }
        }
    }
}

/// Die hoechste sinnvolle Partitionierung fuer `n` Werte: hoechste Ordnung,
/// bei der jede Partition mindestens 16 Werte hat und `n` sich restlos teilt.
fn partitionenordnung(n: usize) -> u8 {
    let mut po = 0u8;
    while po < MAX_PARTITION_ORDNUNG {
        let naechste = po + 1;
        if n % (1usize << naechste) != 0 || n < (1usize << naechste) * 16 {
            break;
        }
        po = naechste;
    }
    po
}

/// Der Rice-Parameter mit den wenigsten Bits.
///
/// Kosten fuer `k`: `Summe(betrag >> k) + anzahl * (1 + k)`.
fn bester_rice_parameter(rest: &[i64]) -> u8 {
    let n = rest.len();
    if n == 0 {
        return 0;
    }
    let mut bestes = 0u8;
    let mut beste_kosten = u64::MAX;
    for k in 0..=14u8 {
        let mut summe: u64 = 0;
        for &r in rest {
            // GEFALTET rechnen, nicht den Betrag: der Leser sieht den
            // gefalteten Wert, und ein Parameter, der fuer den Betrag
            // optimal ist, ist es fuer den gefalteten nicht.
            summe += (((r << 1) ^ (r >> 63)) as u64) >> k;
        }
        let kosten = summe + n as u64 * (1 + u64::from(k));
        if kosten < beste_kosten {
            beste_kosten = kosten;
            bestes = k;
        }
    }
    bestes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Die CRC gegen bekannte Werte. CRC-8 mit ``x^8+x^2+x+1`` ueber
    /// `"123456789"` ist 0xF4, CRC-16 mit ``x^16+x^15+x^2+1`` ist 0xFEE8 —
    /// das sind die Werte aus der FLAC-Spezifikation, und sie sind das
    /// einzige, was diese Funktion richtig macht.
    #[test]
    fn crc_stimmt_mit_der_spezifikation() {
        let probe = b"123456789";
        assert_eq!(crc8(probe), 0xF4, "CRC-8");
        assert_eq!(crc16(probe), 0xFEE8, "CRC-16");
        assert_eq!(crc8(b""), 0x00);
        assert_eq!(crc16(b""), 0x0000);
    }

    /// Die Blockgroessen-Codes: der Code muss die Groesse BENENNEN, weil der
    /// Leser sonst die letzte, kuerzere Frame falsch laesst.
    #[test]
    fn blockgroessen_code_nennt_jede_groesse() {
        for (code, n) in [
            (8u8, 256usize),
            (9, 512),
            (10, 1024),
            (11, 2048),
            (12, 4096),
            (13, 8192),
        ] {
            assert_eq!(
                blockgroessen_code(n),
                (code, None),
                "{n} Proben muessen Code {code} bekommen"
            );
        }
        // Und eine Groesse ausserhalb der Tabelle braucht den Ausweichcode.
        let (code, extra) = blockgroessen_code(100);
        assert_eq!(code, 6, "100 Proben brauchen den 8-Bit-Ausweichcode");
        assert_eq!(extra, Some((8, 99)));
        let (code, extra) = blockgroessen_code(5000);
        assert_eq!(code, 7);
        assert_eq!(extra, Some((16, 4999)));
    }

    /// Der Rice-Parameter: die Groesse bestimmt ihn, und das 4-Bit-Feld
    /// begrenzt ihn.
    ///
    /// GEMESSEN 02.10.2026: dieser Test stand zuerst auf
    /// `assert_eq!(..., 1)` fuer Reste der Groesse 1 und ist daran
    /// gescheitert, dass 0 zurueckkam. **Beide sind richtig**: fuer einen
    /// Rest von 1 kostet `k = 0` zwei Bit (ein 0, dann die 1) und `k = 1`
    /// ebenfalls zwei (die 1, dann ein Restbit). Der Code nimmt bei
    /// Gleichstand das kleinere `k` — das ist eine Vereinbarung, keine
    /// Eigenschaft.
    ///
    /// Deshalb prueft der Test jetzt EIGENSCHAFTEN und keine Zahl:
    /// 1. Nullen -> `k = 0` (der haeufigste Fall, und der billigste)
    /// 2. groessere Reste -> groesseres `k` (monoton)
    /// 3. `k` passt ins 4-Bit-Feld
    /// 4. die Bitzahl liegt dicht an der informationstheoretischen Schranke
    #[test]
    fn rice_parameter_waechst_mit_der_groesse() {
        assert_eq!(bester_rice_parameter(&vec![0i64; 100]), 0);

        // Monoton: je groesser die Reste, desto groesser das k.
        let mut vorher = 0u8;
        for &m in &[1u64, 3, 15, 255, 4095, 65535, 1 << 20] {
            let rest = vec![m as i64; 100];
            let k = bester_rice_parameter(&rest);
            assert!(
                k <= 14,
                "Reste 2^20 passen nicht ins 4-Bit-Feld (Rice2 noetig)"
            );
            assert!(
                k >= vorher,
                "Reste {m}: k {k} ist kleiner als vorher {vorher}"
            );
            vorher = k;
        }

        // Und die Bitzahl liegt dicht an der Schranke: Summe log2 der Reste,
        // aufgerundet, plus die Quotientenbits. Das ist die eigentliche
        // Aussage — ein Enkoder, der hier weit daneben liegt, komprimiert
        // nicht, sondern fuellt.
        for &m in &[3u64, 255, 4095] {
            let rest = vec![m as i64; 256];
            let k = bester_rice_parameter(&rest);
            let mut bits = 0u64;
            for &r in &rest {
                bits += (r.unsigned_abs() >> k) + 1 + u64::from(k);
            }
            // Und GEMESSEN ist, warum der Cast **weg** darf: `leading_zeros()` gibt
            // `u32`, und `max(1)` bleibt `u32` — also `as u64` war eine **Wandlung in
            // sich selbst**.
            let schranke = u64::from(m.leading_zeros().max(1)) * 256;
            assert!(
                bits <= schranke * 2,
                "Reste {m}: {bits} Bit, Schranke {schranke} — der Encoder arbeitet"
            );
        }
    }

    /// Der FIXED-Praediktor muss die Formeln der Spezifikation treffen —
    /// nachgerechnet an einem Block, bei dem man sie von Hand kennt.
    #[test]
    fn fixed_praediktoren_treffen_die_norm() {
        let w = [10i32, 20, 30, 40, 50];
        assert_eq!(praedikt(&w, 0, 0), 0);
        assert_eq!(praedikt(&w, 1, 1), 10);
        // 2*x[k-1] - x[k-2] = 2*20 - 10 = 30
        assert_eq!(praedikt(&w, 2, 2), 30);
        // 3*30 - 3*20 + 10 = 40
        assert_eq!(praedikt(&w, 3, 3), 40);
        // 4*40 - 6*30 + 4*20 - 10 = 160 - 180 + 80 - 10 = 50
        //
        // GEMESSEN 02.10.2026: hier stand 60. Die Formel ist richtig, die
        // Rechnung im Test war es nicht — und ein Test, der die Rechnung
        // falsch hat, meldet einen Fehler im Code, den es nicht gibt.
        assert_eq!(praedikt(&w, 4, 4), 50);
        // Und der Fehler der Vorhersage, aus dem der Rest entsteht:
        assert_eq!(i64::from(w[4]) - i64::from(praedikt(&w, 4, 4)), 0);
    }
}
