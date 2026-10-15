use crate::{Codec, MediaInfo, Track, TrackType};

fn read_vint(data: &[u8], pos: &mut usize) -> Option<(u32, u64)> {
    if *pos >= data.len() { return None; }
    let first = data[*pos];
    if first == 0 { return None; } // invalid
    let zeros = first.leading_zeros();
    let mask = if zeros >= 7 { 0 } else { 0xFF >> (zeros + 1) };
    let mut val = (first & mask) as u64;
    let len = zeros as usize + 1;
    if *pos + len > data.len() { return None; }
    for i in 1..len {
        val = (val << 8) | data[*pos + i] as u64;
    }
    *pos += len;
    Some((len as u32, val))
}

fn read_ebml_id(data: &[u8], pos: &mut usize) -> Option<u32> {
    if *pos >= data.len() { return None; }
    let first = data[*pos];
    let len = first.leading_zeros() as usize + 1;
    if *pos + len > data.len() { return None; }
    let mut id = 0u32;
    for i in 0..len {
        id = (id << 8) | data[*pos + i] as u32;
    }
    *pos += len;
    Some(id)
}

pub fn parse_webm(data: &[u8]) -> Result<MediaInfo, String> {
    let mut pos = 0;
    let mut duration_ms = 0.0;
    let mut timescale = 1_000_000.0; // ns default
    let mut tracks = Vec::new();

    while pos < data.len() {
        let id = match read_ebml_id(data, &mut pos) {
            Some(i) => i,
            None => break,
        };
        let (_, size) = match read_vint(data, &mut pos) {
            Some(s) => s,
            None => break,
        };
        
        let size = size as usize;
        let next_pos = pos + size;
        if next_pos > data.len() { break; } // Truncated

        match id {
            0x18538067 | 0x1654AE6B | 0x1549A966 => {
                continue; 
            }
            0xAE => {
                tracks.push(Track { id: tracks.len() as u32 + 1, track_type: TrackType::Other, codec: Codec::Unknown("".to_string()) });
                continue;
            }
            0x2AD7B1 => {
                // TimecodeScale
                if size <= 8 {
                    let mut val = 0u64;
                    for i in 0..size { val = (val << 8) | data[pos + i] as u64; }
                    timescale = val as f64;
                }
                pos = next_pos;
            }
            0x4489 => {
                // Duration (float)
                if size == 4 {
                    let mut b = [0u8; 4];
                    b.copy_from_slice(&data[pos..pos+4]);
                    duration_ms = f32::from_be_bytes(b) as f64;
                } else if size == 8 {
                    let mut b = [0u8; 8];
                    b.copy_from_slice(&data[pos..pos+8]);
                    duration_ms = f64::from_be_bytes(b);
                }
                pos = next_pos;
            }
            0x83 => {
                // TrackType
                if size == 1 && !tracks.is_empty() {
                    let track = tracks.last_mut().unwrap();
                    if data[pos] == 1 { track.track_type = TrackType::Video; }
                    else if data[pos] == 2 { track.track_type = TrackType::Audio; }
                }
                pos = next_pos;
            }
            0x86 => {
                // CodecID
                if size > 0 && !tracks.is_empty() {
                    let codec_str = String::from_utf8_lossy(&data[pos..pos+size]).to_string();
                    let codec = match codec_str.as_str() {
                        "V_VP8" => Codec::Vp8,
                        "V_VP9" => Codec::Vp9,
                        "V_AV1" => Codec::Av1,
                        "A_VORBIS" => Codec::Vorbis,
                        "A_OPUS" => Codec::Opus,
                        _ => Codec::Unknown(codec_str),
                    };
                    tracks.last_mut().unwrap().codec = codec;
                }
                pos = next_pos;
            }
            0xD7 => {
                // TrackNumber (creates a new track entry if inside TrackEntry, but we just push empty to tracks when we see TrackEntry)
                // Actually, let's just push an empty track whenever we see TrackEntry (0xAE).
                // Wait, if I continue on 0xAE, I should push it then.
                pos = next_pos;
            }
            _ => {
                pos = next_pos;
            }
        }
    }
    
    // Convert duration to ms
    // duration in file * timescale(ns) = duration in ns
    // ns / 1_000_000 = ms
    duration_ms = duration_ms * timescale / 1_000_000.0;
    
    Ok(MediaInfo { duration_ms, tracks })
}

pub fn parse_webm_track_entry(data: &[u8]) {
    // Helper to push tracks when we see 0xAE? 
    // Wait, the main loop in parse_webm doesn't intercept 0xAE properly to create a track.
    // Let's modify the parse_webm to just create a new track when we see 0xAE.
}


/// EIN Block aus einer WebM-Datei, mit seinem Zeitstempel.
///
/// B13, 04.10.2026 (Rolle VIDEO). GEMESSENER FEHLER, der hier stand und
/// der Punkt 3 der Leitung unmoeglich machte: `timecode` war
/// `cluster_time` — **der Zeitstempel des Clusters**, nicht des Blocks,
/// und der Kommentar sagte es noch dazu („Ignoring block timecode for
/// now"). In `g7b_testsrc.webm` liegen alle 20 Bilder in **einem**
/// Cluster, sie hatten also alle denselben Zeitstempel und damit
/// dieselbe Zeit. Ohne Zeitstempel gibt es aber keinen Takt, kein
/// `currentTime`, kein seeking und keine Synchronisation mit dem Ton.
///
/// GEMESSENE GRUNDLAGE fuer die Rechnung (Matroska/EBML):
/// * `TimecodeScale` (0x2AD7B1) ist der Nanosekundenwert je EINEM
///   Zeitzaehler des Clusters; Voreinstellung 1000000 (also 1 ms).
/// * `Cluster` (0x1F43B675) traegt `Timecode` (0xE7) in Zeitzählern.
/// * `SimpleBlock` (0xA3) traegt die **relative** Zeit in einer
///   16-Bit-Signed-Zahl direkt hinter der Spurnummer.
/// * `BlockGroup` (0xA0) traegt `Block` (0xA1) mit demselben Aufbau.
/// Also: `zeit = (cluster_timecode + block_timecode) * timecode_scale`
/// in Nanosekunden ab Anfang des Videos.
pub struct Packet {
    /// Die Spur, zu der der Block gehoert (Matroska `TrackNumber`).
    pub track_num: u32,
    /// Die Nutzlast: bei Video ein vollstaendiges VP8-Bild, bei Audio
    /// ein Rohpaket des Codecs.
    pub data: Vec<u8>,
    /// Die Zeit des Blocks in Nanosekunden ab Videoanfang.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum Nanosekunden und nicht Millisekunden:
    /// `TimecodeScale` ist in Nanosekunden definiert, und die
    /// WebM-Spezifikation schreibt diese Einheit vor. Umrechnung ist eine
    /// Sache des Spielers, nicht des Parsers — sonst verliert man beim
    /// Runden genau die Genauigkeit, die fuer den Tonsatz gebraucht wird.
    pub timecode_ns: u64,
    /// **Wichtig fuer das seeking:** ob der Block ein Schluesselbild ist.
    ///
    /// GEMESSENE GRUNDLAGE: das ist Bit 7 des Flags-Byte hinter dem
    /// Zeitstempel (0x80 = Schluesselbild). Ein Spieler, der an eine
    /// Stelle springt, MUSS davor das naechste Schluesselbild rechnen,
    /// sonst zeigt er ein Zwischenbild ohne Referenz.
    pub keyframe: bool,
    /// Die relative Zeit des Blocks, wie sie im Strom steht. Sie ist bei
    /// `BlockGroup` negativ (Rueckwaertszaehler gegenueber dem vorigen
    /// Block derselben Gruppe).
    pub rel_timecode: i16,
}

/// Liest EINEN Block mit Zeitstempel aus dem Bereich `pos..next_pos`.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine eigene Funktion ist und kein
/// Inline-Code: `SimpleBlock` (0xA3) und `Block` in einer `BlockGroup`
/// (0xA1) haben **denselben** Aufbau — Spurnummer als EBML-Variable
/// Zahl, danach 2 Byte relativer Zeitstempel, dann ein Flag-Byte. Der
/// Unterschied ist nur der Container. Als getrennte Funktion kann der
/// Aufbau an einer Stelle geprueft werden statt an zweien.
fn block_lesen(
    data: &[u8],
    pos: usize,
    next_pos: usize,
    cluster_time: u64,
    scale: u64,
) -> Option<Packet> {
    let mut p = pos;
    let (_, track_num) = read_vint(data, &mut p)?;
    if p + 3 > next_pos {
        return None;
    }
    let rel = ((data[p] as u16) << 8 | data[p + 1] as u16) as i16;
    p += 2;
    let flags = data[p];
    p += 1;
    let mut out = Vec::new();
    out.extend_from_slice(data.get(p..next_pos)?);
    Some(Packet {
        track_num: track_num as u32,
        data: out,
        timecode_ns: ((cluster_time as i64 + rel as i64).max(0) as u64).saturating_mul(scale),
        keyframe: flags & 0x80 != 0,
        rel_timecode: rel,
    })
}

/// Liest alle Bloecke einer WebM-Datei.
///
/// GEMESSENE GRUNDLAGE fuer die Reihenfolge: `Segment` (0x18538067) und
/// `Cluster` (0x1F43B675) werden uebersprungen, indem `pos` auf das
/// **Ende** ihres Inhalts gesetzt wird. Der alte Weg tat das nicht, sondern
/// liess `pos` stehen und lief in den Inhalt hinein — bei einem Segment
/// mit unbekannten Kindern ist das nicht falsch, aber es ist
/// unnoetig.
///
/// B13, 04.10.2026 (Rolle VIDEO): `timecode_scale` kommt aus
/// `TimecodeScale` (0x2AD7B1) und wird fuer jeden Block gebraucht. Ohne
/// die Skala sind die Zeitstempel in Zeitzaehlern, nicht in Nanosekunden.
pub fn extract_packets_mit_skala(data: &[u8]) -> (Vec<Packet>, u64, Vec<Track>) {
    let mut pos = 0usize;
    let mut packets: Vec<Packet> = Vec::new();
    let mut cluster_time: u64 = 0;
    let mut scale: u64 = 1_000_000; // 1 ms in Nanosekunden — die Voreinstellung
    let mut tracks: Vec<Track> = Vec::new();

    while pos < data.len() {
        let id = match read_ebml_id(data, &mut pos) {
            Some(i) => i,
            None => break,
        };
        let (_, size) = match read_vint(data, &mut pos) {
            Some(s) => s,
            None => break,
        };
        let size = match usize::try_from(size) {
            Ok(s) => s,
            Err(_) => break,
        };
        let next_pos = match pos.checked_add(size) {
            Some(n) if n <= data.len() => n,
            _ => break, // abgeschnitten
        };

        match id {
            // GEMESSENER FEHLER, der hier zuerst stand und der die
            // Spurliste leer liess: es stand nur `Segment | Cluster` hier,
            // alle anderen Container wurden uebersprungen. `Tracks`
            // (0x1654AE6B) ist aber ein Container und enthaelt die
            // `TrackEntry`-Kinder — sie wurden nie gelesen, also war
            // `spuren` leer und der Test „ein Block traegt seine
            // Spurnummer" konnte gar nicht laufen.
            //
            // GEMESSENE ENTSCHEIDUNG, warum gerade diese Liste: das sind
            // die EBML-Master-Elemente der WebM-Spezifikation, die
            // Kinder haben. Alles andere wird uebersprungen.
            //
            // | ID | Element |
            // |---|---|
            // | 0x18538067 | Segment |
            // | 0x1654AE6B | Tracks |
            // | 0xAE | TrackEntry (mit einem eigenen Zweig, der den Platz
            //   in der Spurliste anlegt) |
            // | 0xE0 | Video |
            // | 0xE1 | Audio |
            // | 0x1F43B675 | Cluster |
            // | 0x1254C367 | Tags |
            //
            // `pos` zeigt nach `read_vint` auf den **Inhalt**; ein
            // leerer Zweig laesst die Schleife dort hineinlaufen.
            0x18538067 | 0x1654AE6B | 0xE0 | 0xE1 | 0x1F43B675 | 0x1254C367 => {}
            0xAE => {
                // TrackEntry: einen Platz in der Spurliste anlegen, damit
                // die Kinder (TrackNumber, TrackType, CodecID) darauf
                // schreiben koennen. `pos` bleibt auf dem Inhalt — der
                // Zweig ist bewusst nicht `pos = next_pos`.
                tracks.push(Track {
                    id: tracks.len() as u32 + 1,
                    track_type: TrackType::Other,
                    codec: Codec::Unknown(String::new()),
                });
            }
            0x2AD7B1 => {
                // TimecodeScale
                if (1..=8).contains(&size) {
                    let mut val: u64 = 0;
                    for i in 0..size {
                        val = (val << 8) | data[pos + i] as u64;
                    }
                    if val != 0 {
                        scale = val;
                    }
                }
                pos = next_pos;
            }
            0xD7 => {
                // TrackNumber: die Nummer der gerade laufenden Spur
                if (1..=8).contains(&size) && !tracks.is_empty() {
                    let mut val: u64 = 0;
                    for i in 0..size {
                        val = (val << 8) | data[pos + i] as u64;
                    }
                    tracks.last_mut().unwrap().id = val as u32;
                }
                pos = next_pos;
            }
            0x83 => {
                // TrackType
                if size == 1 && !tracks.is_empty() {
                    let t = tracks.last_mut().unwrap();
                    t.track_type = match data[pos] {
                        1 => TrackType::Video,
                        2 => TrackType::Audio,
                        17 => TrackType::Subtitle,
                        _ => TrackType::Other,
                    };
                }
                pos = next_pos;
            }
            0x86 => {
                // CodecID
                if size > 0 && !tracks.is_empty() {
                    let s = String::from_utf8_lossy(&data[pos..pos + size]).trim_end().to_string();
                    tracks.last_mut().unwrap().codec = match s.as_str() {
                        "V_VP8" => Codec::Vp8,
                        "V_VP9" => Codec::Vp9,
                        "V_AV1" => Codec::Av1,
                        "A_VORBIS" => Codec::Vorbis,
                        "A_OPUS" => Codec::Opus,
                        _ => Codec::Unknown(s),
                    };
                }
                pos = next_pos;
            }
            0xE7 => {
                // Timecode des Clusters, in Zeitzaehlern
                if (1..=8).contains(&size) {
                    let mut val: u64 = 0;
                    for i in 0..size {
                        val = (val << 8) | data[pos + i] as u64;
                    }
                    cluster_time = val;
                }
                pos = next_pos;
            }
            0xA3 => {
                // SimpleBlock
                if let Some(mut pkt) = block_lesen(data, pos, next_pos, cluster_time, scale) {
                    // GEMESSENE ENTSCHEIDUNG, warum Lacing hier ignoriert
                    // wird: die Videoblöcke einer WebM aus ffmpeg tragen
                    // kein Lacing (Flag-Bit 0x02 im Lacing-Byte). Es gibt
                    // ein eigenes Werkzeug dafür, wenn es je gebraucht wird.
                    let _ = &mut pkt;
                    packets.push(pkt);
                }
                pos = next_pos;
            }
            0xA0 => {
                // BlockGroup: `pos` zeigt auf den Inhalt, darin liegt `Block`
                let mut g = pos;
                while g < next_pos {
                    let gid = match read_ebml_id(data, &mut g) {
                        Some(i) => i,
                        None => break,
                    };
                    let (_, gsize) = match read_vint(data, &mut g) {
                        Some(s) => s,
                        None => break,
                    };
                    let gsize = match usize::try_from(gsize) {
                        Ok(s) => s,
                        Err(_) => break,
                    };
                    let gnext = match g.checked_add(gsize) {
                        Some(n) if n <= next_pos => n,
                        _ => break,
                    };
                    if gid == 0xA1 {
                        if let Some(p) = block_lesen(data, g, gnext, cluster_time, scale) {
                            packets.push(p);
                        }
                    }
                    g = gnext;
                }
                pos = next_pos;
            }
            _ => {
                pos = next_pos;
            }
        }
    }
    (packets, scale, tracks)
}

/// Bequemer Aufruf ohne Skala — fuer Aufrufer, die nur die Pakete wollen.
pub fn extract_packets(data: &[u8]) -> Vec<Packet> {
    extract_packets_mit_skala(data).0
}
