//! Media Source Extensions, the byte streams (2026-10-15).
//!
//! Incremental parsers for the MSE byte stream formats (WebM, ISO BMFF
//! "fragmented MP4", MPEG audio, ADTS) and the track buffers with the
//! coded frame processing and removal algorithms of
//! https://w3c.github.io/media-source/. Coded frames are kept as timing
//! records only (no payload): nothing decodes MSE media yet, and the records
//! are what `buffered`, `remove()`, the duration and the media element
//! clock need. A record is 32 bytes, so an hour of 30 fps video with Opus
//! audio costs about 9 MB less than one second of its payload would.

use std::collections::HashMap;

// ---------------------------------------------------------------------
// Types shared by the parsers and the track buffers
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Audio,
    Video,
    Text,
}

#[derive(Debug, Clone)]
pub(crate) struct TrackInfo {
    pub id: u32,
    pub kind: Kind,
    pub codec: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct InitSegment {
    /// The duration the init segment declares (seconds).
    pub duration: Option<f64>,
    pub tracks: Vec<TrackInfo>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CodedFrame {
    pub track: u32,
    pub pts: f64,
    pub dts: f64,
    pub dur: f64,
    pub key: bool,
    pub size: u32,
}

#[derive(Debug)]
pub(crate) enum Event {
    Init(InitSegment),
    Frame(CodedFrame),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    WebM,
    Mp4,
    Mpeg,
    Adts,
}

/// A parsed MIME type that MSE supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContentType {
    pub format: Format,
    /// `audio/*`: no video track may appear.
    pub audio_only: bool,
}

impl ContentType {
    /// Formats without timestamps get them generated (MSE "generate
    /// timestamps flag"): MPEG audio and ADTS.
    pub(crate) fn generates_timestamps(self) -> bool {
        matches!(self.format, Format::Mpeg | Format::Adts)
    }
}

/// `MediaSource.isTypeSupported()` / `addSourceBuffer()`: the containers
/// and codecs the engine accepts. The answers follow the media element's
/// `canPlayType` table (vorspann.js) plus VP8/VP9 profile 0, the codecs
/// the project decodes or is building decoders for. Not claimed: AV1,
/// HEVC, VP9 profiles 1-3, AC-3/E-AC-3 and HDR transfer functions.
pub(crate) fn parse_type(t: &str) -> Option<ContentType> {
    let mut parts = split_params(t);
    if parts.is_empty() {
        return None;
    }
    let essence = parts.remove(0).trim().to_ascii_lowercase();
    let mut codecs: Option<String> = None;
    for p in &parts {
        let Some((k, v)) = p.split_once('=') else { continue };
        let k = k.trim().to_ascii_lowercase();
        let v = v.trim().trim_matches('"').trim();
        match k.as_str() {
            "codecs" => codecs = Some(v.to_string()),
            // HDR transfer functions (YouTube asks with `eotf=`): only SDR.
            "eotf" if !v.eq_ignore_ascii_case("bt709") => return None,
            _ => {}
        }
    }
    let (format, audio_only) = match essence.as_str() {
        "video/webm" => (Format::WebM, false),
        "audio/webm" => (Format::WebM, true),
        "video/mp4" => (Format::Mp4, false),
        "audio/mp4" => (Format::Mp4, true),
        "audio/mpeg" => (Format::Mpeg, true),
        "audio/aac" => (Format::Adts, true),
        _ => return None,
    };
    let ct = ContentType { format, audio_only };
    let Some(list) = codecs else { return Some(ct) };
    let list: Vec<&str> = list.split(',').map(str::trim).collect();
    if list.iter().any(|c| c.is_empty()) {
        return None;
    }
    for c in list {
        let (video, ok) = codec_supported(format, c);
        if !ok || (video && audio_only) {
            return None;
        }
    }
    Some(ct)
}

/// Splits a MIME type at `;` outside quotes.
fn split_params(t: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for ch in t.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                cur.push(ch);
            }
            ';' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    out.push(cur);
    out
}

/// `(is_video, supported)` for one codec string in one container.
fn codec_supported(format: Format, c: &str) -> (bool, bool) {
    let l = c.to_ascii_lowercase();
    let hex6 = |s: &str| s.len() == 6 && s.bytes().all(|b| b.is_ascii_hexdigit());
    let vp09 = |s: &str| {
        let f: Vec<&str> = s.split('.').collect();
        f.len() >= 4
            && f[0] == "vp09"
            && f[1] == "00"
            && f[2].len() == 2
            && f[2].bytes().all(|b| b.is_ascii_digit())
            && f[3] == "08"
    };
    let aac = |s: &str| matches!(s, "mp4a.40.2" | "mp4a.40.02" | "mp4a.40.5" | "mp4a.40.05" | "mp4a.40.29" | "mp4a.67");
    match format {
        Format::WebM => match l.as_str() {
            "vp8" | "vp8.0" | "vp9" | "vp9.0" => (true, true),
            "opus" | "vorbis" => (false, true),
            s if vp09(s) => (true, true),
            _ => (false, false),
        },
        Format::Mp4 => match l.as_str() {
            s if (s.starts_with("avc1.") || s.starts_with("avc3.")) && hex6(&s[5..]) => (true, true),
            s if vp09(s) => (true, true),
            s if aac(s) => (false, true),
            "mp4a.69" | "mp4a.6b" | "mp3" | "opus" | "flac" => (false, true),
            _ => (false, false),
        },
        Format::Mpeg => (false, l == "mp3"),
        Format::Adts => (false, aac(&l)),
    }
}

// ---------------------------------------------------------------------
// Byte reading helpers
// ---------------------------------------------------------------------

fn be16(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 2).map_or(0, |s| u16::from_be_bytes([s[0], s[1]]) as u32)
}
fn be24(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 3).map_or(0, |s| (s[0] as u32) << 16 | (s[1] as u32) << 8 | s[2] as u32)
}
fn be32(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4).map_or(0, |s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}
fn be64(b: &[u8], o: usize) -> u64 {
    b.get(o..o + 8).map_or(0, |s| u64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
}

/// The unconsumed input of a parser: bytes plus their absolute stream
/// offset, compacted from time to time.
#[derive(Default)]
struct Input {
    buf: Vec<u8>,
    start: usize,
    /// Absolute stream offset of `buf[start]`.
    abs: u64,
    /// Bytes still to drop from the stream (a skipped element).
    skip: u64,
}

impl Input {
    fn push(&mut self, data: &[u8]) {
        if self.start > 0 && self.start * 2 >= self.buf.len() {
            self.buf.drain(..self.start);
            self.start = 0;
        }
        self.buf.extend_from_slice(data);
    }
    fn rest(&self) -> &[u8] {
        &self.buf[self.start..]
    }
    fn consume(&mut self, n: usize) {
        let n = n.min(self.buf.len() - self.start);
        self.start += n;
        self.abs += n as u64;
    }
    /// Drops pending skip bytes; `false` while some remain.
    fn do_skip(&mut self) -> bool {
        if self.skip > 0 {
            let n = (self.rest().len() as u64).min(self.skip);
            self.consume(n as usize);
            self.skip -= n;
        }
        self.skip == 0
    }
    fn clear(&mut self) {
        self.abs += (self.buf.len() - self.start) as u64;
        self.buf.clear();
        self.start = 0;
        self.skip = 0;
    }
}

// ---------------------------------------------------------------------
// WebM (https://w3c.github.io/mse-byte-stream-format-webm/)
// ---------------------------------------------------------------------

const EBML: u32 = 0x1A45_DFA3;
const SEGMENT: u32 = 0x1853_8067;
const SEEKHEAD: u32 = 0x114D_9B74;
const INFO: u32 = 0x1549_A966;
const TRACKS: u32 = 0x1654_AE6B;
const CLUSTER: u32 = 0x1F43_B675;
const CUES: u32 = 0x1C53_BB6B;
const TAGS: u32 = 0x1254_C367;
const CHAPTERS: u32 = 0x1043_A770;
const ATTACHMENTS: u32 = 0x1941_A469;
const VOID: u32 = 0xEC;
const CRC32: u32 = 0xBF;
const TIMECODE: u32 = 0xE7;
const SIMPLEBLOCK: u32 = 0xA3;
const BLOCKGROUP: u32 = 0xA0;
const BLOCK: u32 = 0xA1;
const BLOCKDURATION: u32 = 0x9B;
const REFERENCEBLOCK: u32 = 0xFB;
const TIMECODESCALE: u32 = 0x2A_D7B1;
const DURATION: u32 = 0x4489;
const TRACKENTRY: u32 = 0xAE;
const TRACKNUMBER: u32 = 0xD7;
const TRACKTYPE: u32 = 0x83;
const CODECID: u32 = 0x86;
const DEFAULTDURATION: u32 = 0x23_E383;
const VIDEO: u32 = 0xE0;
const PIXELWIDTH: u32 = 0xB0;
const PIXELHEIGHT: u32 = 0xBA;
const DOCTYPE: u32 = 0x4282;

fn segment_level(id: u32) -> bool {
    matches!(id, SEEKHEAD | INFO | TRACKS | CLUSTER | CUES | TAGS | CHAPTERS | ATTACHMENTS)
}

/// An EBML element header: `Ok(None)` when more bytes are needed.
/// Returns `(id, size (None = unknown), header length)`.
fn ebml_header(b: &[u8]) -> Result<Option<(u32, Option<u64>, usize)>, String> {
    let Some(&first) = b.first() else { return Ok(None) };
    let idlen = first.leading_zeros() as usize + 1;
    if idlen > 4 {
        return Err("WebM: invalid element ID".into());
    }
    if b.len() < idlen + 1 {
        return Ok(None);
    }
    let mut id = 0u32;
    for &x in &b[..idlen] {
        id = id << 8 | x as u32;
    }
    let s0 = b[idlen];
    let slen = s0.leading_zeros() as usize + 1;
    if slen > 8 {
        return Err("WebM: invalid element size".into());
    }
    if b.len() < idlen + slen {
        return Ok(None);
    }
    let mut v = (s0 as u64) & (0xFF >> slen);
    let mut all_ones = v == (0xFF >> slen) as u64;
    for &x in &b[idlen + 1..idlen + slen] {
        v = v << 8 | x as u64;
        all_ones &= x == 0xFF;
    }
    Ok(Some((id, if all_ones { None } else { Some(v) }, idlen + slen)))
}

/// The children of a master element whose body is complete.
fn ebml_children(b: &[u8]) -> Vec<(u32, &[u8])> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < b.len() {
        let Ok(Some((id, size, hl))) = ebml_header(&b[p..]) else { break };
        let len = size.map_or(b.len() - p - hl, |s| (s as usize).min(b.len() - p - hl));
        out.push((id, &b[p + hl..p + hl + len]));
        p += hl + len;
    }
    out
}

fn ebml_uint(b: &[u8]) -> u64 {
    b.iter().take(8).fold(0u64, |v, &x| v << 8 | x as u64)
}

fn ebml_float(b: &[u8]) -> Option<f64> {
    match b.len() {
        4 => Some(f32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64),
        8 => Some(f64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])),
        _ => None,
    }
}

/// An unsigned EBML varint with its marker removed: `(value, length)`.
fn ebml_vint(b: &[u8]) -> Option<(u64, usize)> {
    let first = *b.first()?;
    let len = first.leading_zeros() as usize + 1;
    if len > 8 || b.len() < len {
        return None;
    }
    let mut v = (first as u64) & (0xFF >> len);
    for &x in &b[1..len] {
        v = v << 8 | x as u64;
    }
    Some((v, len))
}

/// The duration of an Opus packet from its TOC byte (RFC 6716 §3.1).
fn opus_duration(p: &[u8]) -> Option<f64> {
    let toc = *p.first()?;
    let config = toc >> 3;
    let frame = match config {
        0..=11 => [0.010, 0.020, 0.040, 0.060][(config % 4) as usize],
        12..=15 => [0.010, 0.020][(config % 2) as usize],
        _ => [0.0025, 0.005, 0.010, 0.020][(config % 4) as usize],
    };
    let count = match toc & 3 {
        0 => 1,
        1 | 2 => 2,
        _ => (*p.get(1)? & 0x3F) as u32,
    };
    Some(frame * count as f64)
}

/// The sizes of the frames laced into one block (Matroska lacing) and the
/// length of the lacing header.
fn lace_sizes(lacing: u8, data: &[u8]) -> Result<(Vec<usize>, usize), String> {
    if lacing == 0 {
        return Ok((vec![data.len()], 0));
    }
    let err = || "WebM: invalid lacing".to_string();
    let count = *data.first().ok_or_else(err)? as usize + 1;
    let mut p = 1;
    let mut sizes = Vec::with_capacity(count);
    match lacing {
        1 => {
            for _ in 0..count - 1 {
                let mut s = 0usize;
                loop {
                    let b = *data.get(p).ok_or_else(err)?;
                    p += 1;
                    s += b as usize;
                    if b != 255 {
                        break;
                    }
                }
                sizes.push(s);
            }
        }
        2 => {
            let total = data.len() - 1;
            if total % count != 0 {
                return Err(err());
            }
            return Ok((vec![total / count; count], 1));
        }
        _ => {
            let (first, n) = ebml_vint(&data[p..]).ok_or_else(err)?;
            p += n;
            let mut prev = first as i64;
            sizes.push(first as usize);
            for _ in 1..count - 1 {
                let (raw, n) = ebml_vint(&data[p..]).ok_or_else(err)?;
                p += n;
                let bias = (1i64 << (7 * n - 1)) - 1;
                prev += raw as i64 - bias;
                if prev < 0 {
                    return Err(err());
                }
                sizes.push(prev as usize);
            }
        }
    }
    let used: usize = sizes.iter().sum();
    if p + used > data.len() {
        return Err(err());
    }
    sizes.push(data.len() - p - used);
    Ok((sizes, p))
}

struct WebmTrack {
    num: u32,
    kind: Kind,
    default_dur: Option<f64>,
    opus: bool,
}

#[derive(Default)]
pub(crate) struct WebmParser {
    input: Input,
    /// Inside a Segment: its absolute end (None = unknown size).
    segment: Option<Option<u64>>,
    /// Inside a Cluster: its absolute end (None = unknown size).
    cluster: Option<Option<u64>>,
    /// Seconds per timecode tick.
    scale: f64,
    duration_ticks: Option<f64>,
    cluster_tc: Option<i64>,
    tracks: Vec<WebmTrack>,
    init_done: bool,
    /// Frames whose duration is only known from the next frame of their
    /// track (no BlockDuration, no DefaultDuration).
    pending: HashMap<u32, Vec<CodedFrame>>,
    max_dur: HashMap<u32, f64>,
}

impl WebmParser {
    fn new() -> Self {
        WebmParser { scale: 0.001, ..Default::default() }
    }

    fn reset(&mut self) {
        self.input.clear();
        self.segment = None;
        self.cluster = None;
        self.cluster_tc = None;
        self.pending.clear();
    }

    fn feed(&mut self, data: &[u8], out: &mut Vec<Event>) -> Result<(), String> {
        self.input.push(data);
        loop {
            if !self.input.do_skip() {
                break;
            }
            let abs = self.input.abs;
            if let Some(Some(end)) = self.cluster
                && abs >= end
            {
                self.end_cluster(out);
                continue;
            }
            if let Some(Some(end)) = self.segment
                && abs >= end
            {
                self.segment = None;
            }
            let Some((id, size, hl)) = ebml_header(self.input.rest())? else { break };
            let avail = self.input.rest().len();
            let full = |size: Option<u64>| -> Result<Option<usize>, String> {
                let s = size.ok_or("WebM: unknown-size element")? as usize;
                Ok((avail >= hl + s).then_some(s))
            };
            if self.cluster.is_some() {
                if segment_level(id) || id == EBML || id == SEGMENT {
                    self.end_cluster(out);
                    continue;
                }
                match id {
                    TIMECODE | SIMPLEBLOCK | BLOCKGROUP => {
                        let Some(s) = full(size)? else { break };
                        let body = self.input.rest()[hl..hl + s].to_vec();
                        self.input.consume(hl + s);
                        match id {
                            TIMECODE => self.cluster_tc = Some(ebml_uint(&body) as i64),
                            SIMPLEBLOCK => self.block(&body, true, None, false, out)?,
                            _ => {
                                let mut block = None;
                                let mut dur = None;
                                let mut has_ref = false;
                                for (cid, cb) in ebml_children(&body) {
                                    match cid {
                                        BLOCK => block = Some(cb.to_vec()),
                                        BLOCKDURATION => dur = Some(ebml_uint(cb)),
                                        REFERENCEBLOCK => has_ref = true,
                                        _ => {}
                                    }
                                }
                                if let Some(b) = block {
                                    self.block(&b, false, dur, has_ref, out)?;
                                }
                            }
                        }
                    }
                    _ => {
                        self.input.consume(hl);
                        self.input.skip = size.ok_or("WebM: unknown-size element in a cluster")?;
                    }
                }
                continue;
            }
            if self.segment.is_some() {
                match id {
                    EBML => {
                        self.segment = None;
                    }
                    SEGMENT => {
                        self.input.consume(hl);
                        self.segment = Some(size.map(|s| self.input.abs + s));
                    }
                    INFO | TRACKS => {
                        let Some(s) = full(size)? else { break };
                        let body = self.input.rest()[hl..hl + s].to_vec();
                        self.input.consume(hl + s);
                        if id == INFO {
                            self.info(&body);
                        } else {
                            self.tracks(&body, out)?;
                        }
                    }
                    CLUSTER => self.start_cluster(size, hl)?,
                    _ => {
                        self.input.consume(hl);
                        self.input.skip = size.ok_or("WebM: unknown-size element in a segment")?;
                    }
                }
                continue;
            }
            match id {
                EBML => {
                    let Some(s) = full(size)? else { break };
                    let body = self.input.rest()[hl..hl + s].to_vec();
                    self.input.consume(hl + s);
                    for (cid, cb) in ebml_children(&body) {
                        if cid == DOCTYPE {
                            let t = String::from_utf8_lossy(cb);
                            let t = t.trim_end_matches('\0');
                            if t != "webm" && t != "matroska" {
                                return Err(format!("WebM: unsupported DocType '{t}'"));
                            }
                        }
                    }
                }
                SEGMENT => {
                    self.input.consume(hl);
                    self.segment = Some(size.map(|s| self.input.abs + s));
                }
                // A media segment appended on its own (after abort()).
                CLUSTER if self.init_done => self.start_cluster(size, hl)?,
                VOID | CRC32 => {
                    self.input.consume(hl);
                    self.input.skip = size.ok_or("WebM: unknown-size element")?;
                }
                _ => return Err(format!("WebM: unexpected element 0x{id:X} at the top level")),
            }
        }
        Ok(())
    }

    fn start_cluster(&mut self, size: Option<u64>, hl: usize) -> Result<(), String> {
        if !self.init_done {
            return Err("WebM: a cluster before the initialization segment".into());
        }
        self.input.consume(hl);
        self.cluster = Some(size.map(|s| self.input.abs + s));
        self.cluster_tc = None;
        Ok(())
    }

    fn end_cluster(&mut self, out: &mut Vec<Event>) {
        self.cluster = None;
        self.flush(out);
    }

    fn info(&mut self, body: &[u8]) {
        let mut scale = 1_000_000u64;
        let mut dur = None;
        for (id, b) in ebml_children(body) {
            match id {
                TIMECODESCALE => scale = ebml_uint(b).max(1),
                DURATION => dur = ebml_float(b),
                _ => {}
            }
        }
        self.scale = scale as f64 / 1e9;
        self.duration_ticks = dur.filter(|d| d.is_finite() && *d > 0.0);
    }

    fn tracks(&mut self, body: &[u8], out: &mut Vec<Event>) -> Result<(), String> {
        self.flush(out);
        let mut infos = Vec::new();
        let mut tracks = Vec::new();
        for (id, entry) in ebml_children(body) {
            if id != TRACKENTRY {
                continue;
            }
            let (mut num, mut typ, mut codec, mut dd, mut w, mut h) = (0u32, 0u64, String::new(), None, 0u32, 0u32);
            for (cid, cb) in ebml_children(entry) {
                match cid {
                    TRACKNUMBER => num = ebml_uint(cb) as u32,
                    TRACKTYPE => typ = ebml_uint(cb),
                    CODECID => codec = String::from_utf8_lossy(cb).trim_end_matches('\0').to_string(),
                    DEFAULTDURATION => dd = Some(ebml_uint(cb) as f64 / 1e9).filter(|d| *d > 0.0),
                    VIDEO => {
                        for (vid, vb) in ebml_children(cb) {
                            match vid {
                                PIXELWIDTH => w = ebml_uint(vb) as u32,
                                PIXELHEIGHT => h = ebml_uint(vb) as u32,
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            let kind = match typ {
                1 => Kind::Video,
                2 => Kind::Audio,
                0x11 => Kind::Text,
                _ => continue,
            };
            let name = match codec.as_str() {
                "V_VP8" => "vp8".to_string(),
                "V_VP9" => "vp9".to_string(),
                "V_AV1" => "av01".to_string(),
                "A_OPUS" => "opus".to_string(),
                "A_VORBIS" => "vorbis".to_string(),
                "V_MPEG4/ISO/AVC" => "avc1".to_string(),
                "A_AAC" => "mp4a.40.2".to_string(),
                other => other.to_ascii_lowercase(),
            };
            tracks.push(WebmTrack { num, kind, default_dur: dd, opus: codec == "A_OPUS" });
            infos.push(TrackInfo { id: num, kind, codec: name, width: w, height: h });
        }
        self.tracks = tracks;
        self.init_done = true;
        let duration = self.duration_ticks.map(|d| d * self.scale);
        out.push(Event::Init(InitSegment { duration, tracks: infos }));
        Ok(())
    }

    fn block(&mut self, b: &[u8], simple: bool, dur: Option<u64>, has_ref: bool, out: &mut Vec<Event>) -> Result<(), String> {
        let err = || "WebM: invalid block".to_string();
        let (num, n) = ebml_vint(b).ok_or_else(err)?;
        if b.len() < n + 3 {
            return Err(err());
        }
        let rel = i16::from_be_bytes([b[n], b[n + 1]]) as i64;
        let flags = b[n + 2];
        let num = num as u32;
        let Some(track) = self.tracks.iter().find(|t| t.num == num) else { return Ok(()) };
        let (kind, default_dur, opus) = (track.kind, track.default_dur, track.opus);
        let tc = self.cluster_tc.ok_or("WebM: a block before the cluster timecode")?;
        let ts = (tc + rel) as f64 * self.scale;
        let key = kind != Kind::Video || if simple { flags & 0x80 != 0 } else { !has_ref };
        let data = &b[n + 3..];
        let (sizes, lh) = lace_sizes((flags >> 1) & 3, data)?;
        let count = sizes.len();
        let mut offset = lh;
        let mut frames = Vec::with_capacity(count);
        for size in &sizes {
            let payload = &data[offset..offset + size];
            offset += size;
            let d = if let Some(d) = dur {
                Some(d as f64 * self.scale / count as f64)
            } else if let Some(d) = default_dur {
                Some(d)
            } else if opus {
                opus_duration(payload)
            } else {
                None
            };
            frames.push((d, *size as u32));
        }
        // The previous frames of this track end where this block starts.
        self.resolve_pending(num, ts, out);
        let mut t = ts;
        for (d, size) in frames {
            let f = CodedFrame { track: num, pts: t, dts: t, dur: d.unwrap_or(0.0), key, size };
            match d {
                Some(d) => {
                    self.note_duration(num, d);
                    out.push(Event::Frame(f));
                    t += d;
                }
                None => self.pending.entry(num).or_default().push(f),
            }
        }
        Ok(())
    }

    fn note_duration(&mut self, track: u32, d: f64) {
        let m = self.max_dur.entry(track).or_insert(0.0);
        if d > *m {
            *m = d;
        }
    }

    fn estimate(&self, track: u32) -> f64 {
        match self.max_dur.get(&track) {
            Some(&d) if d > 0.0 => d,
            _ => match self.tracks.iter().find(|t| t.num == track).map(|t| t.kind) {
                Some(Kind::Video) => 1.0 / 30.0,
                _ => 0.023,
            },
        }
    }

    fn resolve_pending(&mut self, track: u32, next: f64, out: &mut Vec<Event>) {
        let Some(list) = self.pending.remove(&track) else { return };
        if list.is_empty() {
            return;
        }
        let first = list[0].pts;
        let total = next - first;
        let each = if total > 0.0 { total / list.len() as f64 } else { self.estimate(track) };
        for (i, mut f) in list.into_iter().enumerate() {
            f.pts = first + each * i as f64;
            f.dts = f.pts;
            f.dur = each;
            out.push(Event::Frame(f));
        }
        self.note_duration(track, each);
    }

    /// Emits the frames still waiting for a duration with an estimate (the
    /// largest duration seen on the track), like the end of a cluster does
    /// in other engines.
    fn flush(&mut self, out: &mut Vec<Event>) {
        let mut keys: Vec<u32> = self.pending.keys().copied().collect();
        keys.sort_unstable();
        for track in keys {
            let est = self.estimate(track);
            if let Some(list) = self.pending.remove(&track) {
                let mut t = list.first().map_or(0.0, |f| f.pts);
                for mut f in list {
                    f.pts = t;
                    f.dts = t;
                    f.dur = est;
                    t += est;
                    out.push(Event::Frame(f));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------
// ISO BMFF (https://w3c.github.io/mse-byte-stream-format-isobmff/)
// ---------------------------------------------------------------------

/// The child boxes of a complete box body: `(type, body)`.
fn mp4_boxes(b: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let mut p = 0;
    while p + 8 <= b.len() {
        let size = be32(b, p) as u64;
        let typ = [b[p + 4], b[p + 5], b[p + 6], b[p + 7]];
        let (hl, size) = match size {
            0 => (8, (b.len() - p) as u64),
            1 => (16, be64(b, p + 8)),
            s => (8, s),
        };
        if size < hl as u64 || p as u64 + size > b.len() as u64 {
            break;
        }
        out.push((typ, &b[p + hl..p + size as usize]));
        p += size as usize;
    }
    out
}

fn mp4_child<'a>(b: &'a [u8], typ: &[u8; 4]) -> Option<&'a [u8]> {
    mp4_boxes(b).into_iter().find(|(t, _)| t == typ).map(|(_, c)| c)
}

struct Mp4Track {
    id: u32,
    kind: Kind,
    timescale: u32,
    /// Edit list media time (track ticks) shifted out of the timeline.
    edit: i64,
    def_dur: u32,
    def_size: u32,
    def_flags: u32,
    next_dts: Option<i64>,
}

#[derive(Default)]
pub(crate) struct Mp4Parser {
    input: Input,
    tracks: Vec<Mp4Track>,
    init_done: bool,
    /// Inside an mdat: its absolute end.
    mdat_end: Option<u64>,
    /// The samples of the last moof with the absolute end of their data.
    samples: Vec<(u64, CodedFrame)>,
    next_sample: usize,
}

impl Mp4Parser {
    fn reset(&mut self) {
        self.input.clear();
        self.mdat_end = None;
        self.samples.clear();
        self.next_sample = 0;
    }

    fn in_media_segment(&self) -> bool {
        self.mdat_end.is_some() || self.next_sample < self.samples.len()
    }

    fn emit_until(&mut self, abs: u64, out: &mut Vec<Event>) {
        while let Some(&(end, f)) = self.samples.get(self.next_sample) {
            if end > abs {
                break;
            }
            out.push(Event::Frame(f));
            self.next_sample += 1;
        }
    }

    fn emit_all(&mut self, out: &mut Vec<Event>) {
        self.emit_until(u64::MAX, out);
        self.samples.clear();
        self.next_sample = 0;
    }

    fn feed(&mut self, data: &[u8], out: &mut Vec<Event>) -> Result<(), String> {
        self.input.push(data);
        loop {
            if !self.input.do_skip() {
                break;
            }
            if let Some(end) = self.mdat_end {
                let n = (self.input.rest().len() as u64).min(end - self.input.abs);
                self.input.consume(n as usize);
                self.emit_until(self.input.abs, out);
                if self.input.abs >= end {
                    self.mdat_end = None;
                    self.emit_all(out);
                    continue;
                }
                break;
            }
            let b = self.input.rest();
            if b.len() < 8 {
                break;
            }
            let size32 = be32(b, 0) as u64;
            let typ = [b[4], b[5], b[6], b[7]];
            let (hl, size) = match size32 {
                1 => {
                    if b.len() < 16 {
                        break;
                    }
                    (16usize, Some(be64(b, 8)))
                }
                0 => (8, None),
                s => (8, Some(s)),
            };
            if let Some(s) = size
                && s < hl as u64
            {
                return Err("MP4: invalid box size".into());
            }
            match &typ {
                b"moov" | b"moof" => {
                    let s = size.ok_or("MP4: unbounded box")? as usize;
                    if b.len() < s {
                        break;
                    }
                    let body = b[hl..s].to_vec();
                    let start = self.input.abs;
                    self.input.consume(s);
                    if &typ == b"moov" {
                        self.moov(&body, out)?;
                    } else {
                        if !self.init_done {
                            return Err("MP4: a media segment before the initialization segment".into());
                        }
                        self.emit_all(out);
                        self.moof(&body, start)?;
                    }
                }
                b"mdat" => {
                    let start = self.input.abs;
                    self.input.consume(hl);
                    self.mdat_end = Some(size.map_or(u64::MAX, |s| start + s));
                }
                _ => {
                    let s = size.ok_or("MP4: unbounded box")?;
                    self.input.consume(hl);
                    self.input.skip = s - hl as u64;
                }
            }
        }
        Ok(())
    }

    fn moov(&mut self, b: &[u8], out: &mut Vec<Event>) -> Result<(), String> {
        let mut timescale = 1000u32;
        let mut duration = 0u64;
        let mut fragment_duration = 0u64;
        let mut tracks = Vec::new();
        let mut infos = Vec::new();
        let mut trex: HashMap<u32, (u32, u32, u32)> = HashMap::new();
        for (typ, c) in mp4_boxes(b) {
            match &typ {
                b"mvhd" => {
                    if c.first() == Some(&1) {
                        timescale = be32(c, 20);
                        duration = be64(c, 24);
                    } else {
                        timescale = be32(c, 12);
                        duration = be32(c, 16) as u64;
                        if duration == 0xFFFF_FFFF {
                            duration = 0;
                        }
                    }
                }
                b"trak" => {
                    if let Some((t, info)) = Self::trak(c) {
                        tracks.push(t);
                        infos.push(info);
                    }
                }
                b"mvex" => {
                    for (t2, c2) in mp4_boxes(c) {
                        match &t2 {
                            b"mehd" => {
                                fragment_duration = if c2.first() == Some(&1) { be64(c2, 4) } else { be32(c2, 4) as u64 };
                            }
                            b"trex" => {
                                trex.insert(be32(c2, 4), (be32(c2, 12), be32(c2, 16), be32(c2, 20)));
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        for t in &mut tracks {
            if let Some(&(d, s, f)) = trex.get(&t.id) {
                t.def_dur = d;
                t.def_size = s;
                t.def_flags = f;
            }
        }
        let ts = timescale.max(1) as f64;
        let duration = if fragment_duration > 0 {
            Some(fragment_duration as f64 / ts)
        } else if duration > 0 && duration != u64::MAX {
            Some(duration as f64 / ts)
        } else {
            None
        };
        self.tracks = tracks;
        self.init_done = true;
        out.push(Event::Init(InitSegment { duration, tracks: infos }));
        Ok(())
    }

    fn trak(b: &[u8]) -> Option<(Mp4Track, TrackInfo)> {
        let tkhd = mp4_child(b, b"tkhd")?;
        let v1 = tkhd.first() == Some(&1);
        let id = if v1 { be32(tkhd, 20) } else { be32(tkhd, 12) };
        let wo = if v1 { 88 } else { 76 };
        let (mut w, mut h) = (be32(tkhd, wo) >> 16, be32(tkhd, wo + 4) >> 16);
        let mdia = mp4_child(b, b"mdia")?;
        let mdhd = mp4_child(mdia, b"mdhd")?;
        let timescale = if mdhd.first() == Some(&1) { be32(mdhd, 20) } else { be32(mdhd, 12) };
        let hdlr = mp4_child(mdia, b"hdlr")?;
        let kind = match hdlr.get(8..12)? {
            b"vide" => Kind::Video,
            b"soun" => Kind::Audio,
            b"text" | b"subt" | b"sbtl" => Kind::Text,
            _ => return None,
        };
        let stsd = mp4_child(mdia, b"minf").and_then(|m| mp4_child(m, b"stbl")).and_then(|s| mp4_child(s, b"stsd"));
        let mut codec = String::new();
        if let Some(stsd) = stsd
            && let Some((typ, entry)) = stsd.get(8..).and_then(|e| mp4_boxes(e).into_iter().next())
        {
            let (c, ew, eh) = Self::sample_entry(typ, entry, kind);
            codec = c;
            if w == 0 && h == 0 {
                w = ew;
                h = eh;
            }
        }
        let mut edit = 0i64;
        if let Some(elst) = mp4_child(b, b"edts").and_then(|e| mp4_child(e, b"elst")) {
            let v1 = elst.first() == Some(&1);
            let n = be32(elst, 4) as usize;
            let step = if v1 { 20 } else { 12 };
            for i in 0..n.min(16) {
                let o = 8 + i * step;
                let mt = if v1 { be64(elst, o + 8) as i64 } else { be32(elst, o + 4) as i32 as i64 };
                if mt >= 0 {
                    edit = mt;
                    break;
                }
            }
        }
        let track = Mp4Track { id, kind, timescale: timescale.max(1), edit, def_dur: 0, def_size: 0, def_flags: 0, next_dts: None };
        Some((track, TrackInfo { id, kind, codec, width: w, height: h }))
    }

    /// Codec string and picture size of a sample entry.
    fn sample_entry(typ: [u8; 4], e: &[u8], kind: Kind) -> (String, u32, u32) {
        let mut fourcc = typ;
        let children_at = if kind == Kind::Video { 78 } else { 28 };
        let children = e.get(children_at..).unwrap_or(&[]);
        if (&fourcc == b"encv" || &fourcc == b"enca")
            && let Some(frma) = mp4_child(children, b"sinf").and_then(|s| mp4_child(s, b"frma"))
            && frma.len() >= 4
        {
            fourcc = [frma[0], frma[1], frma[2], frma[3]];
        }
        let (w, h) = if kind == Kind::Video { (be16(e, 24), be16(e, 26)) } else { (0, 0) };
        let codec = match &fourcc {
            b"avc1" | b"avc3" => {
                let name = String::from_utf8_lossy(&fourcc).into_owned();
                match mp4_child(children, b"avcC") {
                    Some(c) if c.len() >= 4 => format!("{name}.{:02X}{:02X}{:02X}", c[1], c[2], c[3]),
                    _ => name,
                }
            }
            b"vp09" => match mp4_child(children, b"vpcC") {
                Some(c) if c.len() >= 7 => format!("vp09.{:02}.{:02}.{:02}", c[4], c[5], c[6] >> 4),
                _ => "vp09".to_string(),
            },
            b"av01" => "av01".to_string(),
            b"hvc1" | b"hev1" => "hvc1".to_string(),
            b"mp4a" => Self::esds_codec(mp4_child(children, b"esds")),
            b"Opus" => "opus".to_string(),
            b"fLaC" => "flac".to_string(),
            b".mp3" => "mp3".to_string(),
            other => String::from_utf8_lossy(other).trim().to_ascii_lowercase(),
        };
        (codec, w, h)
    }

    /// `mp4a.OO[.A]` from an esds box (ISO 14496-1 descriptors).
    fn esds_codec(esds: Option<&[u8]>) -> String {
        let Some(b) = esds else { return "mp4a.40.2".to_string() };
        let mut p = 4;
        let mut oti = 0x40u8;
        let mut aot = 2u8;
        // Walk the descriptors: ES (3) -> DecoderConfig (4) -> DecSpecificInfo (5).
        while p < b.len() {
            let tag = b[p];
            p += 1;
            let mut len = 0usize;
            for _ in 0..4 {
                let Some(&x) = b.get(p) else { break };
                p += 1;
                len = len << 7 | (x & 0x7F) as usize;
                if x & 0x80 == 0 {
                    break;
                }
            }
            match tag {
                3 => {
                    let flags = *b.get(p + 2).unwrap_or(&0);
                    p += 3;
                    if flags & 0x80 != 0 {
                        p += 2;
                    }
                    if flags & 0x40 != 0 {
                        p += 1 + *b.get(p).unwrap_or(&0) as usize;
                    }
                    if flags & 0x20 != 0 {
                        p += 2;
                    }
                }
                4 => {
                    oti = *b.get(p).unwrap_or(&0x40);
                    p += 13;
                }
                5 => {
                    let x = *b.get(p).unwrap_or(&0x10);
                    aot = x >> 3;
                    if aot == 31 {
                        aot = 32 + ((x & 7) << 3 | b.get(p + 1).map_or(0, |y| y >> 5));
                    }
                    break;
                }
                _ => p += len,
            }
        }
        if oti == 0x40 { format!("mp4a.40.{aot}") } else { format!("mp4a.{oti:x}") }
    }

    fn moof(&mut self, b: &[u8], moof_start: u64) -> Result<(), String> {
        self.samples.clear();
        self.next_sample = 0;
        for (typ, traf) in mp4_boxes(b) {
            if &typ != b"traf" {
                continue;
            }
            let Some(tfhd) = mp4_child(traf, b"tfhd") else { continue };
            let flags = be24(tfhd, 1);
            let id = be32(tfhd, 4);
            let Some(ti) = self.tracks.iter().position(|t| t.id == id) else { continue };
            let mut p = 8;
            let mut base = moof_start;
            if flags & 0x01 != 0 {
                base = be64(tfhd, p);
                p += 8;
            }
            if flags & 0x02 != 0 {
                p += 4;
            }
            let t = &self.tracks[ti];
            let (kind, ts, edit) = (t.kind, t.timescale as f64, t.edit);
            let mut def_dur = t.def_dur;
            let mut def_size = t.def_size;
            let mut def_flags = t.def_flags;
            if flags & 0x08 != 0 {
                def_dur = be32(tfhd, p);
                p += 4;
            }
            if flags & 0x10 != 0 {
                def_size = be32(tfhd, p);
                p += 4;
            }
            if flags & 0x20 != 0 {
                def_flags = be32(tfhd, p);
            }
            let mut dts: i64 = match mp4_child(traf, b"tfdt") {
                Some(t) if t.first() == Some(&1) => be64(t, 4) as i64,
                Some(t) => be32(t, 4) as i64,
                None => self.tracks[ti].next_dts.unwrap_or(0),
            };
            let mut data_pos = base;
            for (rt, trun) in mp4_boxes(traf) {
                if &rt != b"trun" {
                    continue;
                }
                let rflags = be24(trun, 1);
                let count = be32(trun, 4) as usize;
                let mut q = 8;
                if rflags & 0x01 != 0 {
                    data_pos = (base as i64 + be32(trun, q) as i32 as i64).max(0) as u64;
                    q += 4;
                }
                let mut first_flags = None;
                if rflags & 0x04 != 0 {
                    first_flags = Some(be32(trun, q));
                    q += 4;
                }
                let per = [0x100, 0x200, 0x400, 0x800].iter().filter(|&&f| rflags & f != 0).count() * 4;
                if per > 0 && trun.len() < q + count * per {
                    return Err("MP4: truncated trun".into());
                }
                for i in 0..count.min(1 << 20) {
                    let mut dur = def_dur;
                    let mut size = def_size;
                    let mut sflags = if i == 0 { first_flags.unwrap_or(def_flags) } else { def_flags };
                    let mut cts = 0i64;
                    if rflags & 0x100 != 0 {
                        dur = be32(trun, q);
                        q += 4;
                    }
                    if rflags & 0x200 != 0 {
                        size = be32(trun, q);
                        q += 4;
                    }
                    if rflags & 0x400 != 0 {
                        sflags = be32(trun, q);
                        q += 4;
                    }
                    if rflags & 0x800 != 0 {
                        cts = be32(trun, q) as i32 as i64;
                        q += 4;
                    }
                    let key = kind != Kind::Video || sflags & 0x0001_0000 == 0;
                    data_pos += size as u64;
                    let f = CodedFrame {
                        track: id,
                        pts: (dts + cts - edit) as f64 / ts,
                        dts: (dts - edit) as f64 / ts,
                        dur: dur as f64 / ts,
                        key,
                        size,
                    };
                    self.samples.push((data_pos, f));
                    dts += dur as i64;
                }
            }
            self.tracks[ti].next_dts = Some(dts);
        }
        // Samples arrive in decode order per track; emit them by data offset.
        self.samples.sort_by_key(|s| s.0);
        Ok(())
    }
}

// ---------------------------------------------------------------------
// MPEG audio and ADTS (https://w3c.github.io/mse-byte-stream-format-mpeg-audio/)
// ---------------------------------------------------------------------

#[derive(Default)]
pub(crate) struct AudioParser {
    adts: bool,
    input: Input,
    init_done: bool,
}

/// kbit/s by index: MPEG-1 layers I, II, III, then MPEG-2/2.5 layer I and
/// layers II/III.
const MP3_BITRATES: [[u32; 15]; 5] = [
    [0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448],
    [0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384],
    [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320],
    [0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256],
    [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
];
const MP3_RATES: [u32; 3] = [44100, 48000, 32000];
const ADTS_RATES: [u32; 13] = [96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350];

impl AudioParser {
    /// `(frame length, samples, sample rate, codec)` of the frame at `b`.
    fn frame(&self, b: &[u8]) -> Option<(usize, u32, u32, String)> {
        if self.adts {
            if b.len() < 7 || b[0] != 0xFF || b[1] & 0xF6 != 0xF0 {
                return None;
            }
            let profile = b[2] >> 6;
            let rate = *ADTS_RATES.get(((b[2] >> 2) & 0x0F) as usize)?;
            let len = ((b[3] as usize & 3) << 11) | (b[4] as usize) << 3 | (b[5] as usize) >> 5;
            let blocks = (b[6] & 3) as u32 + 1;
            if len < 7 {
                return None;
            }
            return Some((len, 1024 * blocks, rate, format!("mp4a.40.{}", profile + 1)));
        }
        if b.len() < 4 || b[0] != 0xFF || b[1] & 0xE0 != 0xE0 {
            return None;
        }
        let version = (b[1] >> 3) & 3; // 3 = MPEG-1, 2 = MPEG-2, 0 = MPEG-2.5
        let layer = (b[1] >> 1) & 3; // 1 = III, 2 = II, 3 = I
        if version == 1 || layer == 0 {
            return None;
        }
        let bi = (b[2] >> 4) as usize;
        let ri = ((b[2] >> 2) & 3) as usize;
        if bi == 0 || bi == 15 || ri == 3 {
            return None;
        }
        let mpeg1 = version == 3;
        let row = match (mpeg1, layer) {
            (true, 3) => 0,
            (true, 2) => 1,
            (true, _) => 2,
            (false, 3) => 3,
            (false, _) => 4,
        };
        let bitrate = MP3_BITRATES[row][bi] * 1000;
        let rate = MP3_RATES[ri] >> if mpeg1 { 0 } else if version == 2 { 1 } else { 2 };
        let pad = ((b[2] >> 1) & 1) as usize;
        let (len, samples) = match layer {
            3 => ((12 * bitrate / rate) as usize * 4 + pad * 4, 384),
            2 => ((144 * bitrate / rate) as usize + pad, 1152),
            _ => {
                let s = if mpeg1 { 1152 } else { 576 };
                ((s / 8 * bitrate / rate) as usize + pad, s)
            }
        };
        if len < 4 {
            return None;
        }
        Some((len, samples, rate, "mp3".to_string()))
    }

    fn feed(&mut self, data: &[u8], out: &mut Vec<Event>) -> Result<(), String> {
        self.input.push(data);
        loop {
            if !self.input.do_skip() {
                break;
            }
            let b = self.input.rest();
            if b.len() < 10 {
                break;
            }
            if !self.adts && &b[..3] == b"ID3" {
                let size = (b[6] as u64 & 0x7F) << 21 | (b[7] as u64 & 0x7F) << 14 | (b[8] as u64 & 0x7F) << 7 | (b[9] as u64 & 0x7F);
                let footer = if b[5] & 0x10 != 0 { 10 } else { 0 };
                self.input.skip = 10 + size + footer;
                continue;
            }
            let Some((len, samples, rate, codec)) = self.frame(b) else {
                // Not a frame header: resynchronize on the next 0xFF.
                let n = b[1..].iter().position(|&x| x == 0xFF).map_or(b.len(), |i| i + 1);
                self.input.consume(n);
                continue;
            };
            if b.len() < len {
                break;
            }
            self.input.consume(len);
            if !self.init_done {
                self.init_done = true;
                out.push(Event::Init(InitSegment {
                    duration: None,
                    tracks: vec![TrackInfo { id: 1, kind: Kind::Audio, codec, width: 0, height: 0 }],
                }));
            }
            out.push(Event::Frame(CodedFrame {
                track: 1,
                pts: 0.0,
                dts: 0.0,
                dur: samples as f64 / rate as f64,
                key: true,
                size: len as u32,
            }));
        }
        Ok(())
    }
}

/// One of the byte stream parsers.
pub(crate) enum Parser {
    WebM(Box<WebmParser>),
    Mp4(Box<Mp4Parser>),
    Audio(Box<AudioParser>),
}

impl Parser {
    pub(crate) fn new(format: Format) -> Parser {
        match format {
            Format::WebM => Parser::WebM(Box::new(WebmParser::new())),
            Format::Mp4 => Parser::Mp4(Box::default()),
            Format::Mpeg => Parser::Audio(Box::default()),
            Format::Adts => Parser::Audio(Box::new(AudioParser { adts: true, ..Default::default() })),
        }
    }

    fn feed(&mut self, data: &[u8], out: &mut Vec<Event>) -> Result<(), String> {
        match self {
            Parser::WebM(p) => p.feed(data, out),
            Parser::Mp4(p) => p.feed(data, out),
            Parser::Audio(p) => p.feed(data, out),
        }
    }

    /// The end of one append: WebM frames that wait for their duration get
    /// an estimate, so `buffered` covers everything appended.
    fn end_of_append(&mut self, out: &mut Vec<Event>) {
        if let Parser::WebM(p) = self {
            p.flush(out);
        }
    }

    fn reset(&mut self) {
        match self {
            Parser::WebM(p) => p.reset(),
            Parser::Mp4(p) => p.reset(),
            Parser::Audio(p) => p.input.clear(),
        }
    }

    fn in_media_segment(&self) -> bool {
        match self {
            Parser::WebM(p) => p.cluster.is_some(),
            Parser::Mp4(p) => p.in_media_segment(),
            Parser::Audio(_) => false,
        }
    }
}

// ---------------------------------------------------------------------
// Track buffers and the coded frame processing algorithm
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Frame {
    pts: f64,
    end: f64,
    key: bool,
    size: u32,
}

/// The codec family of a codec string (`avc1.4D401E` and `avc3.64001F`
/// are both H.264): a later init segment may change the profile, but not
/// the codec, unless `changeType()` came first.
fn codec_family(c: &str) -> &str {
    match c.split('.').next().unwrap_or(c) {
        "avc3" => "avc1",
        "vp09" => "vp9",
        "hev1" => "hvc1",
        f => f,
    }
}

struct TrackBuffer {
    id: u32,
    kind: Kind,
    codec: String,
    /// Sorted by presentation timestamp.
    frames: Vec<Frame>,
    last_dts: Option<f64>,
    last_dur: f64,
    highest_end: Option<f64>,
    need_rap: bool,
    max_dur: f64,
    bytes: usize,
    ranges: Option<Vec<(f64, f64)>>,
}

impl TrackBuffer {
    fn new(id: u32, kind: Kind, codec: &str) -> Self {
        TrackBuffer {
            id,
            kind,
            codec: codec.to_string(),
            frames: Vec::new(),
            last_dts: None,
            last_dur: 0.0,
            highest_end: None,
            need_rap: true,
            max_dur: 0.0,
            bytes: 0,
            ranges: None,
        }
    }

    /// Index of the first frame with `pts >= t`.
    fn lower(&self, t: f64) -> usize {
        self.frames.partition_point(|f| f.pts < t)
    }

    /// Removes the frames with `lo <= pts < hi`, then the frames that
    /// depend on them (up to the next random access point).
    fn remove_span(&mut self, lo: f64, hi: f64) {
        let a = self.lower(lo);
        let mut b = self.lower(hi);
        if a >= b {
            return;
        }
        while b < self.frames.len() && !self.frames[b].key {
            b += 1;
        }
        let freed: usize = self.frames[a..b].iter().map(|f| f.size as usize).sum();
        self.bytes -= freed.min(self.bytes);
        self.frames.drain(a..b);
        self.ranges = None;
    }

    fn insert(&mut self, f: Frame) {
        match self.frames.last() {
            Some(l) if l.pts > f.pts => {
                let i = self.frames.partition_point(|x| x.pts <= f.pts);
                self.frames.insert(i, f);
            }
            _ => self.frames.push(f),
        }
        self.bytes += f.size as usize;
        let d = f.end - f.pts;
        if d > self.max_dur {
            self.max_dur = d;
        }
        self.ranges = None;
    }

    fn ranges(&mut self) -> &[(f64, f64)] {
        if self.ranges.is_none() {
            // Frames closer than twice the longest frame belong to one
            // range (the "fudge room" of other engines).
            let fudge = (2.0 * self.max_dur).clamp(0.001, 0.5);
            let mut out: Vec<(f64, f64)> = Vec::new();
            for f in &self.frames {
                match out.last_mut() {
                    Some(r) if f.pts <= r.1 + fudge => r.1 = r.1.max(f.end),
                    _ => out.push((f.pts, f.end)),
                }
            }
            self.ranges = Some(out);
        }
        self.ranges.as_deref().unwrap_or(&[])
    }
}

/// Times in whole microseconds, the resolution of other engines' media
/// clocks (`buffered` then reads 1.4, not 1.4000000000000001).
fn us(t: f64) -> f64 {
    (t * 1e6).round() / 1e6
}

/// The ranges in both lists.
fn intersect(a: &[(f64, f64)], b: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < a.len() && j < b.len() {
        let s = a[i].0.max(b[j].0);
        let e = a[i].1.min(b[j].1);
        if s < e {
            out.push((s, e));
        }
        if a[i].1 < b[j].1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// Ranges of several source buffers (the media element's `buffered`).
pub(crate) fn intersect_all(lists: &[Vec<(f64, f64)>]) -> Vec<(f64, f64)> {
    let mut it = lists.iter();
    let Some(first) = it.next() else { return Vec::new() };
    it.fold(first.clone(), |acc, l| intersect(&acc, l))
}

/// The Rust side of one SourceBuffer.
pub(crate) struct SourceBufferCore {
    format: Format,
    parser: Parser,
    pub(crate) generate_timestamps: bool,
    audio_only: bool,
    /// Bytes added by appendBuffer() and not yet run through the parser.
    input: Vec<u8>,
    tracks: Vec<TrackBuffer>,
    first_init: bool,
    /// `changeType()` ran: the next init segment may change codecs.
    type_changed: bool,
    group_start: Option<f64>,
    group_end: f64,
}

/// What one run of the segment parser loop produced.
pub(crate) struct AppendOutcome {
    pub(crate) init: Option<InitSegment>,
    pub(crate) timestamp_offset: f64,
}

/// Memory limits of one SourceBuffer (the payload sizes of its frames),
/// as other engines use them: 150 MB with video, 12 MB audio only.
const VIDEO_LIMIT: usize = 150 << 20;
const AUDIO_LIMIT: usize = 12 << 20;

impl SourceBufferCore {
    pub(crate) fn new(ct: ContentType) -> Self {
        SourceBufferCore {
            format: ct.format,
            parser: Parser::new(ct.format),
            generate_timestamps: ct.generates_timestamps(),
            audio_only: ct.audio_only,
            input: Vec::new(),
            tracks: Vec::new(),
            first_init: false,
            type_changed: false,
            group_start: None,
            group_end: 0.0,
        }
    }

    pub(crate) fn stage(&mut self, data: &[u8]) {
        self.input.extend_from_slice(data);
    }

    pub(crate) fn in_media_segment(&self) -> bool {
        self.parser.in_media_segment()
    }

    /// Sets the group start timestamp: to the group end when `mode`
    /// becomes "sequence", to the new offset when `timestampOffset` is set
    /// in sequence mode.
    pub(crate) fn set_group_start(&mut self, t: Option<f64>) {
        self.group_start = Some(t.unwrap_or(self.group_end));
    }

    /// The "reset parser state" algorithm.
    pub(crate) fn reset_parser(&mut self) {
        self.parser.reset();
        self.input.clear();
        for t in &mut self.tracks {
            t.last_dts = None;
            t.last_dur = 0.0;
            t.highest_end = None;
            t.need_rap = true;
        }
    }

    /// `changeType()`: a new parser for the new type; the track layout
    /// stays (the next init segment may change codecs).
    pub(crate) fn change_type(&mut self, ct: ContentType) {
        self.format = ct.format;
        self.parser = Parser::new(ct.format);
        self.generate_timestamps = ct.generates_timestamps();
        self.audio_only = ct.audio_only;
        self.input.clear();
        self.type_changed = true;
    }

    /// The segment parser loop over the staged bytes.
    pub(crate) fn append(&mut self, tso: f64, aw_start: f64, aw_end: f64, sequence: bool) -> Result<AppendOutcome, String> {
        let data = std::mem::take(&mut self.input);
        let mut events = Vec::new();
        let r = self.parser.feed(&data, &mut events);
        if r.is_ok() {
            self.parser.end_of_append(&mut events);
        }
        let mut tso = tso;
        let mut init = None;
        for ev in events {
            match ev {
                Event::Init(seg) => {
                    self.init_received(&seg)?;
                    init = Some(seg);
                }
                Event::Frame(f) => self.process_frame(f, &mut tso, aw_start, aw_end, sequence),
            }
        }
        r?;
        Ok(AppendOutcome { init, timestamp_offset: tso })
    }

    /// "Initialization segment received" (track checks and buffers).
    fn init_received(&mut self, seg: &InitSegment) -> Result<(), String> {
        if seg.tracks.is_empty() {
            return Err("the initialization segment has no tracks".into());
        }
        if self.audio_only && seg.tracks.iter().any(|t| t.kind == Kind::Video) {
            return Err("a video track in an audio-only SourceBuffer".into());
        }
        if !self.first_init {
            self.first_init = true;
            self.tracks = seg.tracks.iter().map(|t| TrackBuffer::new(t.id, t.kind, &t.codec)).collect();
            return Ok(());
        }
        let changed = std::mem::take(&mut self.type_changed);
        for kind in [Kind::Audio, Kind::Video, Kind::Text] {
            let old: Vec<usize> = (0..self.tracks.len()).filter(|&i| self.tracks[i].kind == kind).collect();
            let new: Vec<&TrackInfo> = seg.tracks.iter().filter(|t| t.kind == kind).collect();
            if old.len() != new.len() {
                return Err("the initialization segment does not match the first one".into());
            }
            for (i, t) in old.into_iter().zip(new) {
                if !changed && codec_family(&self.tracks[i].codec) != codec_family(&t.codec) {
                    return Err("the initialization segment changes a codec without changeType()".into());
                }
                self.tracks[i].id = t.id;
                self.tracks[i].codec = t.codec.clone();
            }
        }
        for t in &mut self.tracks {
            t.need_rap = true;
        }
        Ok(())
    }

    /// The coded frame processing algorithm for one frame.
    fn process_frame(&mut self, f: CodedFrame, tso: &mut f64, aw_start: f64, aw_end: f64, sequence: bool) {
        let Some(ti) = self.tracks.iter().position(|t| t.id == f.track) else { return };
        let dur = if f.dur.is_finite() && f.dur > 0.0 { us(f.dur) } else { 0.0 };
        for _ in 0..2 {
            if sequence && let Some(gs) = self.group_start.take() {
                *tso = gs - f.pts;
                self.group_end = gs;
                for t in &mut self.tracks {
                    t.need_rap = true;
                }
            }
            let pts = us(f.pts + *tso);
            let dts = us(f.dts + *tso);
            if let Some(ld) = self.tracks[ti].last_dts
                && (dts < ld || dts - ld > 2.0 * self.tracks[ti].last_dur)
            {
                if sequence {
                    self.group_start = Some(self.group_end);
                } else {
                    self.group_end = pts;
                }
                for t in &mut self.tracks {
                    t.last_dts = None;
                    t.last_dur = 0.0;
                    t.highest_end = None;
                    t.need_rap = true;
                }
                continue;
            }
            let end = us(pts + dur);
            let t = &mut self.tracks[ti];
            if pts < aw_start - 1e-9 || end > aw_end + 1e-9 {
                t.need_rap = true;
                return;
            }
            if t.need_rap {
                if !f.key {
                    return;
                }
                t.need_rap = false;
            }
            match t.highest_end {
                None => t.remove_span(pts, end),
                Some(h) if h <= pts => t.remove_span(h, end),
                Some(_) => {}
            }
            t.insert(Frame { pts, end, key: f.key, size: f.size });
            t.last_dts = Some(dts);
            t.last_dur = dur;
            t.highest_end = Some(t.highest_end.map_or(end, |h| h.max(end)));
            if end > self.group_end {
                self.group_end = end;
            }
            if self.generate_timestamps {
                *tso = end;
            }
            return;
        }
    }

    /// The "coded frame removal" algorithm: `[start, end)`, extended to
    /// the next random access point at or after `end` (or to `duration`).
    pub(crate) fn remove(&mut self, start: f64, end: f64, duration: f64) {
        for t in &mut self.tracks {
            let i = t.lower(end);
            let stop = t.frames[i..].iter().find(|f| f.key).map_or(duration.max(end), |f| f.pts);
            let a = t.lower(start);
            let b = t.lower(stop).max(a);
            let freed: usize = t.frames[a..b].iter().map(|f| f.size as usize).sum();
            t.bytes -= freed.min(t.bytes);
            t.frames.drain(a..b);
            t.ranges = None;
        }
    }

    /// `buffered`: the intersection of the audio and video track ranges.
    /// `ended_end`: the MediaSource has ended; the last range of every
    /// track then reaches that time.
    pub(crate) fn buffered(&mut self, ended_end: Option<f64>) -> Vec<(f64, f64)> {
        let mut lists = Vec::new();
        for t in &mut self.tracks {
            if t.kind == Kind::Text {
                continue;
            }
            let mut r = t.ranges().to_vec();
            if let (Some(e), Some(last)) = (ended_end, r.last_mut())
                && e > last.1
            {
                last.1 = e;
            }
            lists.push(r);
        }
        intersect_all(&lists)
    }

    /// `(highest presentation timestamp, highest end time)` of all frames.
    pub(crate) fn highest(&self) -> (f64, f64) {
        let mut pts = f64::NAN;
        let mut end = f64::NAN;
        for t in &self.tracks {
            for f in t.frames.iter().rev().take(64) {
                if !(f.pts <= pts) {
                    pts = f.pts;
                }
                if !(f.end <= end) {
                    end = f.end;
                }
            }
        }
        (pts, end)
    }

    /// The "coded frame eviction" algorithm before an append of `incoming`
    /// bytes: drops whole groups of pictures before `current` while over
    /// the limit. `false` when the data does not fit (QuotaExceededError).
    pub(crate) fn evict(&mut self, current: f64, incoming: usize) -> bool {
        let limit = if self.tracks.iter().any(|t| t.kind == Kind::Video) || !self.audio_only { VIDEO_LIMIT } else { AUDIO_LIMIT };
        let total = |s: &Self| s.tracks.iter().map(|t| t.bytes).sum::<usize>() + s.input.len();
        if total(self) + incoming <= limit {
            return true;
        }
        for t in &mut self.tracks {
            // Keep the group of pictures that contains `current`.
            let keep = t.frames.iter().rposition(|f| f.key && f.pts <= current).unwrap_or(0);
            let freed: usize = t.frames[..keep].iter().map(|f| f.size as usize).sum();
            t.bytes -= freed.min(t.bytes);
            t.frames.drain(..keep);
            t.ranges = None;
        }
        total(self) + incoming <= limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types() {
        assert!(parse_type("video/webm; codecs=\"vp9, opus\"").is_some());
        assert!(parse_type("video/webm; codecs=\"vp09.00.51.08.01.01.01.01.00\"").is_some());
        assert!(parse_type("video/webm; codecs=\"vp09.02.51.10\"").is_none());
        assert!(parse_type("video/mp4; codecs=\"avc1.4d401e, mp4a.40.2\"").is_some());
        assert!(parse_type("video/mp4; codecs=\"av01.0.05M.08\"").is_none());
        assert!(parse_type("audio/webm; codecs=\"vp8\"").is_none());
        assert!(parse_type("video/mp4; codecs=\"vp8\"").is_none());
        assert!(parse_type("video/webm; codecs=\"vp9\"; eotf=smpte2084").is_none());
        assert!(parse_type("video/mp4").is_some());
        assert!(parse_type("video/mp4; codecs=\"\"").is_none());
        assert!(parse_type("audio/mpeg").is_some_and(|c| c.generates_timestamps()));
        assert!(parse_type("video/x-flv").is_none());
        assert!(parse_type("").is_none());
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn opus_toc() {
        // Config 31 (CELT 20 ms), one frame.
        assert!(opus_duration(&[0xFC]).is_some_and(|d| near(d, 0.02)));
        // Config 15 (hybrid 20 ms), code 1: two frames.
        assert!(opus_duration(&[0x79]).is_some_and(|d| near(d, 0.04)));
        // Config 1 (SILK 20 ms), code 3 with a count of 3.
        assert!(opus_duration(&[0x0B, 0x03]).is_some_and(|d| near(d, 0.06)));
    }

    #[test]
    fn lacing() {
        // Xiph lacing, 3 frames: 255+10, 5, rest.
        let mut d = vec![2, 255, 10, 5];
        d.extend(std::iter::repeat_n(0u8, 265 + 5 + 7));
        let (s, h) = lace_sizes(1, &d).expect("xiph");
        assert_eq!((s, h), (vec![265, 5, 7], 4));
        let (s, _) = lace_sizes(2, &[1, 0, 0, 0, 0]).expect("fixed");
        assert_eq!(s, vec![2, 2]);
        // EBML lacing: first 4 (0x84), diff +1 (0xC0 = 64 - 63 = 1), rest.
        let mut d = vec![2, 0x84, 0xC0];
        d.extend(std::iter::repeat_n(0u8, 4 + 5 + 3));
        let (s, _) = lace_sizes(3, &d).expect("ebml");
        assert_eq!(s, vec![4, 5, 3]);
    }

    #[test]
    fn ranges_and_removal() {
        let mut t = TrackBuffer::new(1, Kind::Video, "vp9");
        for i in 0..30 {
            let pts = i as f64 / 10.0;
            t.insert(Frame { pts, end: pts + 0.1, key: i % 10 == 0, size: 10 });
        }
        assert_eq!(t.ranges().len(), 1);
        // Removing [1.0, 1.1) takes the dependent frames up to 2.0.
        t.remove_span(1.0, 1.1);
        let r = t.ranges().to_vec();
        assert_eq!(r.len(), 2);
        assert!(near(r[0].0, 0.0) && near(r[0].1, 1.0) && near(r[1].0, 2.0) && near(r[1].1, 3.0), "{r:?}");
        assert_eq!(t.bytes, 200);
    }
}
