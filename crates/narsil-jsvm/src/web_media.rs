//! Media Source Extensions (https://w3c.github.io/media-source/), the Rust
//! half (2026-10-15). `web_media.js` builds MediaSource, SourceBuffer and
//! SourceBufferList; these natives keep one [`SourceBufferCore`] per
//! SourceBuffer (byte stream parser + track buffers, `web_media_demux.rs`).
//! Loaded lazily like the other platform APIs: a page that never names
//! `MediaSource` pays nothing.

use std::collections::HashMap;

use crate::heap::GVal;
use crate::interp::{Engine, Stoerung};
use crate::web_media_demux::{SourceBufferCore, intersect_all, parse_type};
use crate::web_platform::PlatformOp;

const GLUE: &str = include_str!("web_media.js");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MediaOp {
    IsTypeSupported,
    SbNew,
    SbPush,
    SbAppend,
    SbParsing,
    SbAbort,
    SbRemove,
    SbBuffered,
    SbHighest,
    SbEvict,
    SbChangeType,
    SbGroupStart,
    SbFree,
    MsBuffered,
}

const NATIVES: &[(&str, MediaOp, u32)] = &[
    ("isTypeSupported", MediaOp::IsTypeSupported, 1),
    ("sbNew", MediaOp::SbNew, 1),
    ("sbPush", MediaOp::SbPush, 2),
    ("sbAppend", MediaOp::SbAppend, 5),
    ("sbParsing", MediaOp::SbParsing, 1),
    ("sbAbort", MediaOp::SbAbort, 1),
    ("sbRemove", MediaOp::SbRemove, 4),
    ("sbBuffered", MediaOp::SbBuffered, 2),
    ("sbHighest", MediaOp::SbHighest, 1),
    ("sbEvict", MediaOp::SbEvict, 3),
    ("sbChangeType", MediaOp::SbChangeType, 2),
    ("sbGroupStart", MediaOp::SbGroupStart, 2),
    ("sbFree", MediaOp::SbFree, 1),
    ("msBuffered", MediaOp::MsBuffered, 2),
];

/// All SourceBuffers of one engine.
#[derive(Default)]
pub(crate) struct MediaState {
    next: u32,
    buffers: HashMap<u32, SourceBufferCore>,
}

impl Engine {
    pub(crate) fn media_glue_laden(&mut self) -> Result<(), Stoerung> {
        self.platform.media.get_or_insert_with(Box::default);
        let list: Vec<(&str, PlatformOp, u32)> =
            NATIVES.iter().map(|(n, o, l)| (*n, PlatformOp::Media(*o), *l)).collect();
        let natives = self.platform_natives(&list)?;
        self.platform_run_glue(GLUE, natives)
    }

    fn media_buffer(&mut self, args: &[GVal]) -> Result<&mut SourceBufferCore, Stoerung> {
        let id = self.zu_zahl(self.arg(args, 0))? as u32;
        if self.platform.media.as_ref().is_none_or(|m| !m.buffers.contains_key(&id)) {
            return Err(self.type_error("the SourceBuffer is gone"));
        }
        // Present (checked above); the second lookup only satisfies the
        // borrow checker.
        self.platform.media.as_mut().and_then(|m| m.buffers.get_mut(&id)).ok_or(Stoerung::Wurf(GVal::Undefiniert))
    }

    fn number_list(&mut self, values: &[f64]) -> Result<GVal, Stoerung> {
        let vals: Vec<GVal> = values.iter().map(|v| GVal::Zahl(*v)).collect();
        self.array_of(&vals)
    }

    fn ranges_value(&mut self, ranges: &[(f64, f64)]) -> Result<GVal, Stoerung> {
        let flat: Vec<f64> = ranges.iter().flat_map(|r| [r.0, r.1]).collect();
        self.number_list(&flat)
    }

    pub(crate) fn media_native(&mut self, op: MediaOp, args: &[GVal]) -> Result<GVal, Stoerung> {
        match op {
            MediaOp::IsTypeSupported => {
                let t = self.zu_text(self.arg(args, 0))?;
                Ok(GVal::Bool(parse_type(&t).is_some()))
            }
            MediaOp::SbNew => {
                let t = self.zu_text(self.arg(args, 0))?;
                let Some(ct) = parse_type(&t) else { return Ok(GVal::Zahl(-1.0)) };
                let m = self.platform.media.get_or_insert_with(Box::default);
                m.next += 1;
                let id = m.next;
                m.buffers.insert(id, SourceBufferCore::new(ct));
                self.number_list(&[id as f64, if ct.generates_timestamps() { 1.0 } else { 0.0 }])
            }
            MediaOp::SbPush => {
                let data = self.buffer_source_bytes(self.arg(args, 1))?;
                self.media_buffer(args)?.stage(&data);
                Ok(GVal::Undefiniert)
            }
            MediaOp::SbAppend => {
                let tso = self.zu_zahl(self.arg(args, 1))?;
                let aws = self.zu_zahl(self.arg(args, 2))?;
                let awe = self.zu_zahl(self.arg(args, 3))?;
                let sequence = self.zu_bool(self.arg(args, 4));
                let (ok, tso, err, init) = match self.media_buffer(args)?.append(tso, aws, awe, sequence) {
                    Ok(o) => (true, o.timestamp_offset, None, o.init),
                    Err(e) => (false, tso, Some(e), None),
                };
                let marke = self.wurzel_merke();
                let err = match err {
                    Some(e) => self.text_wert(&e).map_err(Stoerung::Fehler)?,
                    None => GVal::Null,
                };
                self.wurzeln.push(err);
                let init = match init {
                    Some(seg) => {
                        let video = seg.tracks.iter().find(|t| t.kind == crate::web_media_demux::Kind::Video);
                        let audio = seg.tracks.iter().filter(|t| t.kind == crate::web_media_demux::Kind::Audio).count();
                        let vals = [
                            seg.duration.unwrap_or(f64::NAN),
                            video.map_or(0.0, |v| v.width as f64),
                            video.map_or(0.0, |v| v.height as f64),
                            audio as f64,
                            if video.is_some() { 1.0 } else { 0.0 },
                        ];
                        self.number_list(&vals)?
                    }
                    None => GVal::Null,
                };
                self.wurzeln.push(init);
                let r = self.array_of(&[GVal::Bool(ok), GVal::Zahl(tso), err, init]);
                self.wurzel_zurueck(marke);
                r
            }
            MediaOp::SbParsing => Ok(GVal::Bool(self.media_buffer(args)?.in_media_segment())),
            MediaOp::SbAbort => {
                self.media_buffer(args)?.reset_parser();
                Ok(GVal::Undefiniert)
            }
            MediaOp::SbRemove => {
                let start = self.zu_zahl(self.arg(args, 1))?;
                let end = self.zu_zahl(self.arg(args, 2))?;
                let duration = self.zu_zahl(self.arg(args, 3))?;
                self.media_buffer(args)?.remove(start, end, duration);
                Ok(GVal::Undefiniert)
            }
            MediaOp::SbBuffered => {
                let ended = self.zu_bool(self.arg(args, 1));
                let b = self.media_buffer(args)?;
                let end = if ended { Some(b.highest().1).filter(|e| e.is_finite()) } else { None };
                let r = b.buffered(end);
                self.ranges_value(&r)
            }
            MediaOp::SbHighest => {
                let (p, e) = self.media_buffer(args)?.highest();
                self.number_list(&[p, e])
            }
            MediaOp::SbEvict => {
                let current = self.zu_zahl(self.arg(args, 1))?;
                let incoming = self.zu_zahl(self.arg(args, 2))?.max(0.0) as usize;
                Ok(GVal::Bool(self.media_buffer(args)?.evict(current, incoming)))
            }
            MediaOp::SbChangeType => {
                let t = self.zu_text(self.arg(args, 1))?;
                let Some(ct) = parse_type(&t) else { return Ok(GVal::Zahl(-1.0)) };
                let b = self.media_buffer(args)?;
                b.change_type(ct);
                Ok(GVal::Zahl(if b.generate_timestamps { 1.0 } else { 0.0 }))
            }
            MediaOp::SbGroupStart => {
                let t = match self.arg(args, 1) {
                    GVal::Undefiniert => None,
                    v => Some(self.zu_zahl(v)?),
                };
                self.media_buffer(args)?.set_group_start(t);
                Ok(GVal::Undefiniert)
            }
            MediaOp::SbFree => {
                let id = self.zu_zahl(self.arg(args, 0))? as u32;
                if let Some(m) = self.platform.media.as_mut() {
                    m.buffers.remove(&id);
                }
                Ok(GVal::Undefiniert)
            }
            MediaOp::MsBuffered => {
                // The media element's `buffered`: the intersection of the
                // active SourceBuffers; after endOfStream() the last range of
                // each reaches the highest end time of all of them.
                let ids = self.iter_werte(self.arg(args, 0))?;
                let ended = self.zu_bool(self.arg(args, 1));
                let mut ids_n = Vec::with_capacity(ids.len());
                for v in ids {
                    ids_n.push(self.zu_zahl(v)? as u32);
                }
                let Some(m) = self.platform.media.as_mut() else { return self.ranges_value(&[]) };
                let mut highest = f64::NAN;
                if ended {
                    for id in &ids_n {
                        if let Some(b) = m.buffers.get(id) {
                            let e = b.highest().1;
                            if !(e <= highest) {
                                highest = e;
                            }
                        }
                    }
                }
                let end = Some(highest).filter(|e| e.is_finite());
                let mut lists = Vec::new();
                for id in &ids_n {
                    if let Some(b) = m.buffers.get_mut(id) {
                        lists.push(b.buffered(end));
                    }
                }
                let r = intersect_all(&lists);
                self.ranges_value(&r)
            }
        }
    }
}
