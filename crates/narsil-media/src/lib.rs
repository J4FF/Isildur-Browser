pub mod untertitel;
pub mod fenster;
pub mod frame_decoder;
pub mod medien;
pub mod bedienung;
pub mod leiste;
pub mod player;
pub mod mp4;
pub mod video;
pub mod webm;

#[derive(Debug, Clone, PartialEq)]
pub enum Codec {
    Vp8,
    Vp9,
    Av1,
    H264,
    Aac,
    Opus,
    Vorbis,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrackType {
    Video,
    Audio,
    Subtitle,
    Other,
}

#[derive(Debug, Clone)]
pub struct Track {
    pub id: u32,
    pub track_type: TrackType,
    pub codec: Codec,
}

#[derive(Debug, Clone)]
pub struct MediaInfo {
    pub duration_ms: f64,
    pub tracks: Vec<Track>,
}

pub fn parse_media(data: &[u8]) -> Result<MediaInfo, String> {
    if data.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        webm::parse_webm(data)
    } else if data.len() > 8 && &data[4..8] == b"ftyp" {
        mp4::parse_mp4(data)
    } else {
        Err("Unknown media format".to_string())
    }
}
