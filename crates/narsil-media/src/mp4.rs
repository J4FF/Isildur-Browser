use crate::{Codec, MediaInfo, Track, TrackType};

pub fn parse_mp4(data: &[u8]) -> Result<MediaInfo, String> {
    let mut pos = 0;
    let mut duration_ms = 0.0;
    let mut tracks = Vec::new();
    let mut timescale = 1u32;
    
    while pos + 8 <= data.len() {
        let size = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
        let box_type = &data[pos+4..pos+8];
        
        if size == 0 { break; } // End of file
        if pos + size > data.len() { break; } // Truncated
        
        match box_type {
            b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl" => {
                // Container boxes, dive into them
                let mut inner_data = &data[pos+8..pos+size];
                if let Ok(mut info) = parse_mp4(inner_data) {
                    if info.duration_ms > 0.0 && duration_ms == 0.0 {
                        duration_ms = info.duration_ms;
                    }
                    tracks.append(&mut info.tracks);
                }
            }
            b"mvhd" => {
                let version = data[pos+8];
                if version == 0 {
                    timescale = u32::from_be_bytes([data[pos+20], data[pos+21], data[pos+22], data[pos+23]]);
                    let duration = u32::from_be_bytes([data[pos+24], data[pos+25], data[pos+26], data[pos+27]]);
                    if timescale > 0 {
                        duration_ms = (duration as f64 / timescale as f64) * 1000.0;
                    }
                } else if version == 1 {
                    timescale = u32::from_be_bytes([data[pos+28], data[pos+29], data[pos+30], data[pos+31]]);
                    let duration = u64::from_be_bytes([data[pos+32], data[pos+33], data[pos+34], data[pos+35], data[pos+36], data[pos+37], data[pos+38], data[pos+39]]);
                    if timescale > 0 {
                        duration_ms = (duration as f64 / timescale as f64) * 1000.0;
                    }
                }
            }
            b"stsd" => {
                // Sample Description Box
                let count = u32::from_be_bytes([data[pos+12], data[pos+13], data[pos+14], data[pos+15]]);
                if count > 0 && pos + 24 <= pos + size {
                    let entry_size = u32::from_be_bytes([data[pos+16], data[pos+17], data[pos+18], data[pos+19]]) as usize;
                    let format = &data[pos+20..pos+24];
                    let codec = match format {
                        b"avc1" => Codec::H264,
                        b"mp4a" => Codec::Aac,
                        b"vp08" => Codec::Vp8,
                        b"vp09" => Codec::Vp9,
                        b"av01" => Codec::Av1,
                        b"Opus" => Codec::Opus,
                        other => Codec::Unknown(String::from_utf8_lossy(other).to_string()),
                    };
                    let track_type = match format {
                        b"avc1" | b"vp08" | b"vp09" | b"av01" => TrackType::Video,
                        b"mp4a" | b"Opus" => TrackType::Audio,
                        _ => TrackType::Other,
                    };
                    tracks.push(Track { id: tracks.len() as u32 + 1, track_type, codec });
                }
            }
            _ => {}
        }
        pos += size;
    }
    Ok(MediaInfo { duration_ms, tracks })
}
