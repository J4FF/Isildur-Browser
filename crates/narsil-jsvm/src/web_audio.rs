//! Web Audio API (https://webaudio.github.io/web-audio-api/), the Rust half
//! (2026-10-15). `web_audio.js` builds the WebIDL surface and mirrors each
//! context's graph into a [`Graph`] (`web_audio_render.rs`) through these
//! natives; `decodeAudioData` decodes with narsil-audio. Loaded lazily on
//! the first use of an audio global.

use std::collections::HashMap;
use std::sync::Arc;

use crate::heap::GVal;
use crate::interp::{Engine, Stoerung};
use crate::web_audio_render::{
    Analyser, BiquadType, Buf, BufferSource, Compressor, Convolver, CountMode, Delay, Ev, Graph, Iir, Interp, Kind, Node,
    Osc, Panner, Param, Sched, Shaper, Wave, biquad_coefs, frequency_response, resample,
};
use crate::web_platform::PlatformOp;

const GLUE: &str = include_str!("web_audio.js");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AudioOp {
    CtxNew,
    CtxFree,
    NodeNew,
    Connect,
    Disconnect,
    ParamValue,
    ParamEvent,
    ParamCancel,
    ParamLast,
    NodeSet,
    Render,
    Advance,
    Active,
    AnalyserData,
    Reduction,
    Decode,
    BiquadResponse,
    IirResponse,
}

const NATIVES: &[(&str, AudioOp, u32)] = &[
    ("ctxNew", AudioOp::CtxNew, 2),
    ("ctxFree", AudioOp::CtxFree, 1),
    ("nodeNew", AudioOp::NodeNew, 8),
    ("connect", AudioOp::Connect, 6),
    ("disconnect", AudioOp::Disconnect, 6),
    ("paramValue", AudioOp::ParamValue, 4),
    ("paramEvent", AudioOp::ParamEvent, 8),
    ("paramCancel", AudioOp::ParamCancel, 5),
    ("paramLast", AudioOp::ParamLast, 3),
    ("nodeSet", AudioOp::NodeSet, 6),
    ("render", AudioOp::Render, 2),
    ("advance", AudioOp::Advance, 2),
    ("active", AudioOp::Active, 1),
    ("analyserData", AudioOp::AnalyserData, 3),
    ("reduction", AudioOp::Reduction, 2),
    ("decode", AudioOp::Decode, 2),
    ("biquadResponse", AudioOp::BiquadResponse, 7),
    ("iirResponse", AudioOp::IirResponse, 4),
];

/// All audio graphs of one engine.
#[derive(Default)]
pub(crate) struct AudioState {
    next: u32,
    graphs: HashMap<u32, Graph>,
}

fn f32_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

fn bytes_f32(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

fn count_mode(v: f64) -> CountMode {
    match v as i32 {
        1 => CountMode::ClampedMax,
        2 => CountMode::Explicit,
        _ => CountMode::Max,
    }
}

fn interp(v: f64) -> Interp {
    if v as i32 == 1 { Interp::Discrete } else { Interp::Speakers }
}

fn wave(v: f64) -> Wave {
    match v as i32 {
        1 => Wave::Square,
        2 => Wave::Sawtooth,
        3 => Wave::Triangle,
        _ => Wave::Sine,
    }
}

fn biquad_type(v: f64) -> BiquadType {
    match v as i32 {
        1 => BiquadType::Highpass,
        2 => BiquadType::Bandpass,
        3 => BiquadType::Lowshelf,
        4 => BiquadType::Highshelf,
        5 => BiquadType::Peaking,
        6 => BiquadType::Notch,
        7 => BiquadType::Allpass,
        _ => BiquadType::Lowpass,
    }
}

impl Engine {
    pub(crate) fn audio_glue_laden(&mut self) -> Result<(), Stoerung> {
        self.platform.audio.get_or_insert_with(Box::default);
        let list: Vec<(&str, PlatformOp, u32)> =
            NATIVES.iter().map(|(n, o, l)| (*n, PlatformOp::Audio(*o), *l)).collect();
        let natives = self.platform_natives(&list)?;
        self.platform_run_glue(GLUE, natives)
    }

    fn num(&mut self, args: &[GVal], i: usize) -> Result<f64, Stoerung> {
        self.zu_zahl(self.arg(args, i))
    }

    /// The bytes of a Float32Array argument as f32 values (empty for
    /// anything else).
    fn f32_arg(&mut self, v: GVal) -> Vec<f32> {
        match self.buffer_source_bytes(v) {
            Ok(b) => bytes_f32(&b),
            Err(_) => Vec::new(),
        }
    }

    fn graph(&mut self, args: &[GVal]) -> Result<&mut Graph, Stoerung> {
        let id = self.num(args, 0)? as u32;
        if self.platform.audio.as_ref().is_none_or(|a| !a.graphs.contains_key(&id)) {
            return Err(self.type_error("the audio context is closed"));
        }
        // Present (checked above); the second lookup only satisfies the
        // borrow checker.
        self.platform.audio.as_mut().and_then(|a| a.graphs.get_mut(&id)).ok_or(Stoerung::Wurf(GVal::Undefiniert))
    }

    fn f32_buffers(&mut self, chans: &[Vec<f32>]) -> Result<GVal, Stoerung> {
        let marke = self.wurzel_merke();
        let mut vals = Vec::with_capacity(chans.len());
        for c in chans {
            let b = self.new_array_buffer(f32_bytes(c))?;
            self.wurzeln.push(b);
            vals.push(b);
        }
        let r = self.array_of(&vals);
        self.wurzel_zurueck(marke);
        r
    }

    /// An AudioBuffer's data passed as `[sampleRate, Float32Array...]`.
    fn buf_arg(&mut self, v: GVal) -> Result<Option<Arc<Buf>>, Stoerung> {
        if matches!(v, GVal::Null | GVal::Undefiniert) {
            return Ok(None);
        }
        let items = self.iter_werte(v)?;
        let Some((&first, rest)) = items.split_first() else { return Ok(None) };
        let sr = self.zu_zahl(first)?;
        let ch: Vec<Vec<f32>> = rest.iter().map(|c| self.f32_arg(*c)).collect();
        Ok(Some(Arc::new(Buf { sr, ch })))
    }

    pub(crate) fn audio_native(&mut self, op: AudioOp, args: &[GVal]) -> Result<GVal, Stoerung> {
        match op {
            AudioOp::CtxNew => {
                let sr = self.num(args, 0)?;
                let channels = (self.num(args, 1)? as usize).clamp(1, 32);
                let a = self.platform.audio.get_or_insert_with(Box::default);
                a.next += 1;
                let id = a.next;
                a.graphs.insert(id, Graph::new(sr, channels));
                Ok(GVal::Zahl(id as f64))
            }
            AudioOp::CtxFree => {
                let id = self.num(args, 0)? as u32;
                if let Some(a) = self.platform.audio.as_mut() {
                    a.graphs.remove(&id);
                }
                Ok(GVal::Undefiniert)
            }
            AudioOp::NodeNew => {
                // nodeNew(ctx, kind, [nIn, nOut, channelCount, mode, interp, extra],
                //         [value, min, max, aRate, ...], data1, data2)
                let kind = self.num(args, 1)? as i32;
                let shape = self.iter_werte(self.arg(args, 2))?;
                let mut s = [0.0f64; 6];
                for (i, v) in shape.iter().take(6).enumerate() {
                    s[i] = self.zu_zahl(*v)?;
                }
                let pspec = self.iter_werte(self.arg(args, 3))?;
                let mut pvals = Vec::with_capacity(pspec.len());
                for v in pspec {
                    pvals.push(self.zu_zahl(v)?);
                }
                let params: Vec<Param> = pvals
                    .chunks_exact(4)
                    .map(|c| Param::new(c[0] as f32, c[1] as f32, c[2] as f32, c[3] != 0.0))
                    .collect();
                let d1 = self.f32_arg(self.arg(args, 4));
                let d2 = self.f32_arg(self.arg(args, 5));
                let extra = s[5];
                let k = match kind {
                    1 => Kind::Gain,
                    2 => Kind::Oscillator(Box::new(Osc::new(wave(extra)))),
                    3 => Kind::BufferSource(Box::new(BufferSource::new())),
                    4 => Kind::Constant(Sched::default()),
                    5 => Kind::Delay(Box::new(Delay::new(extra))),
                    6 => Kind::Biquad(Box::new(crate::web_audio_render::Biquad::new())),
                    7 => Kind::Iir(Box::new(Iir::new(&d1, &d2))),
                    8 => Kind::Shaper(Shaper { curve: None }),
                    9 => Kind::StereoPanner,
                    10 => Kind::Panner(Box::new(Panner {
                        distance_model: 1,
                        ref_distance: 1.0,
                        max_distance: 10000.0,
                        rolloff: 1.0,
                        cone_inner: 360.0,
                        cone_outer: 360.0,
                        cone_outer_gain: 0.0,
                    })),
                    11 => Kind::Compressor(Box::new(Compressor::new())),
                    12 => Kind::Analyser(Box::new(Analyser::new())),
                    13 => Kind::Splitter,
                    14 => Kind::Merger,
                    15 => Kind::Convolver(Box::new(Convolver::new())),
                    _ => Kind::Silent(extra.max(1.0) as usize),
                };
                let node = Node::new(k, s[0] as usize, s[1] as usize, params, s[2].max(1.0) as usize, count_mode(s[3]), interp(s[4]));
                let id = self.graph(args)?.add(node);
                Ok(GVal::Zahl(id as f64))
            }
            AudioOp::Connect => {
                // connect(ctx, src, out, dst, input, isParam)
                let (src, out, dst, input) = (self.num(args, 1)? as usize, self.num(args, 2)? as usize, self.num(args, 3)? as usize, self.num(args, 4)? as usize);
                let param = self.zu_bool(self.arg(args, 5));
                let g = self.graph(args)?;
                if param {
                    g.connect_param(src, out, dst, input);
                } else {
                    g.connect(src, out, dst, input);
                }
                Ok(GVal::Undefiniert)
            }
            AudioOp::Disconnect => {
                // disconnect(ctx, src, out|-1, dst|-1, input|-1, isParam)
                let src = self.num(args, 1)? as usize;
                let opt = |v: f64| (v >= 0.0).then_some(v as usize);
                let out = opt(self.num(args, 2)?);
                let dst = opt(self.num(args, 3)?);
                let input = opt(self.num(args, 4)?);
                let param = self.zu_bool(self.arg(args, 5));
                self.graph(args)?.disconnect(src, out, dst, input, param);
                Ok(GVal::Undefiniert)
            }
            AudioOp::ParamValue => {
                let (node, p, v) = (self.num(args, 1)? as usize, self.num(args, 2)? as usize, self.num(args, 3)?);
                if let Some(n) = self.graph(args)?.node(node)
                    && let Some(param) = n.params.get_mut(p)
                {
                    param.set_value(v as f32);
                }
                Ok(GVal::Undefiniert)
            }
            AudioOp::ParamEvent => {
                // paramEvent(ctx, node, param, kind, time, value, extra, curve)
                let node = self.num(args, 1)? as usize;
                let p = self.num(args, 2)? as usize;
                let kind = self.num(args, 3)? as i32;
                let t = self.num(args, 4)?;
                let v = self.num(args, 5)? as f32;
                let extra = self.num(args, 6)?;
                let curve = self.f32_arg(self.arg(args, 7));
                let ev = match kind {
                    1 => Ev::Linear(t, v),
                    2 => Ev::Exp(t, v),
                    3 => Ev::Target(t, v, extra),
                    4 => Ev::Curve(t, extra, Arc::new(curve)),
                    _ => Ev::Set(t, v),
                };
                let mut overlap = false;
                if let Some(n) = self.graph(args)?.node(node)
                    && let Some(param) = n.params.get_mut(p)
                {
                    overlap = match &ev {
                        Ev::Curve(t, d, _) => param.curve_overlaps(*t, *d),
                        other => param.curve_overlaps(time_of(other), 0.0),
                    };
                    if !overlap {
                        param.add(ev);
                    }
                }
                Ok(GVal::Bool(!overlap))
            }
            AudioOp::ParamCancel => {
                let (node, p, t) = (self.num(args, 1)? as usize, self.num(args, 2)? as usize, self.num(args, 3)?);
                let hold = self.zu_bool(self.arg(args, 4));
                if let Some(n) = self.graph(args)?.node(node)
                    && let Some(param) = n.params.get_mut(p)
                {
                    if hold {
                        param.cancel_hold(t);
                    } else {
                        param.cancel(t);
                    }
                }
                Ok(GVal::Undefiniert)
            }
            AudioOp::ParamLast => {
                let (node, p) = (self.num(args, 1)? as usize, self.num(args, 2)? as usize);
                let v = self.graph(args)?.node(node).and_then(|n| n.params.get(p)).map_or(0.0, |p| if p.has_events() { p.last } else { p.value });
                Ok(GVal::Zahl(v as f64))
            }
            AudioOp::NodeSet => self.audio_node_set(args),
            AudioOp::Render => {
                let frames = self.num(args, 1)?.max(0.0) as usize;
                let (chans, ended) = self.graph(args)?.render(frames);
                let marke = self.wurzel_merke();
                let ended_v: Vec<GVal> = ended.iter().map(|e| GVal::Zahl(*e as f64)).collect();
                let e = self.array_of(&ended_v)?;
                self.wurzeln.push(e);
                let c = self.f32_buffers(&chans)?;
                self.wurzeln.push(c);
                let r = self.array_of(&[e, c]);
                self.wurzel_zurueck(marke);
                r
            }
            AudioOp::Advance => {
                let frame = self.num(args, 1)?.max(0.0) as u64;
                let mut ended = Vec::new();
                self.graph(args)?.advance_to(frame, &mut ended);
                let vals: Vec<GVal> = ended.iter().map(|e| GVal::Zahl(*e as f64)).collect();
                self.array_of(&vals)
            }
            AudioOp::Active => Ok(GVal::Bool(self.graph(args)?.active_sources())),
            AudioOp::AnalyserData => {
                // analyserData(ctx, node, kind): 0 = time domain, 1 = frequency dB.
                let node = self.num(args, 1)? as usize;
                let kind = self.num(args, 2)? as i32;
                let data = match self.graph(args)?.node(node).map(|n| &mut n.kind) {
                    Some(Kind::Analyser(a)) => {
                        if kind == 1 {
                            a.frequency_db()
                        } else {
                            a.time_data(a.fft_size)
                        }
                    }
                    _ => Vec::new(),
                };
                self.new_array_buffer(f32_bytes(&data))
            }
            AudioOp::Reduction => {
                let node = self.num(args, 1)? as usize;
                let r = match self.graph(args)?.node(node).map(|n| &n.kind) {
                    Some(Kind::Compressor(c)) => c.reduction as f64,
                    _ => 0.0,
                };
                Ok(GVal::Zahl(r))
            }
            AudioOp::Decode => {
                let bytes = self.buffer_source_bytes(self.arg(args, 0))?;
                let target = self.num(args, 1)?;
                let ton = narsil_audio::Format::erkenne(&bytes).and_then(|f| narsil_audio::dekodiere_format(f, &bytes));
                let Ok(ton) = ton else {
                    return Err(Stoerung::Wurf(self.dom_fehler_wert("EncodingError", "Unable to decode audio data")));
                };
                let chans: Vec<Vec<f32>> = ton
                    .kanaele_daten
                    .iter()
                    .map(|c| {
                        let f: Vec<f32> = c.iter().map(|v| *v as f32).collect();
                        resample(&f, ton.rate as f64, target)
                    })
                    .collect();
                if chans.is_empty() || chans[0].is_empty() {
                    return Err(Stoerung::Wurf(self.dom_fehler_wert("EncodingError", "Unable to decode audio data")));
                }
                self.f32_buffers(&chans)
            }
            AudioOp::BiquadResponse => {
                // biquadResponse(type, sr, frequency, Q, gain, detune, freqs) -> [mag, phase]
                let typ = biquad_type(self.num(args, 0)?);
                let sr = self.num(args, 1)?;
                let (f, q, gain, detune) = (self.num(args, 2)?, self.num(args, 3)?, self.num(args, 4)?, self.num(args, 5)?);
                let freqs = self.f32_arg(self.arg(args, 6));
                let c = biquad_coefs(typ, sr, f, q, gain, detune);
                let r = frequency_response(&[c[0], c[1], c[2]], &[1.0, c[3], c[4]], sr, &freqs);
                let mag: Vec<f32> = r.iter().map(|x| x.0).collect();
                let ph: Vec<f32> = r.iter().map(|x| x.1).collect();
                self.f32_buffers(&[mag, ph])
            }
            AudioOp::IirResponse => {
                // iirResponse(ctx, node, sr, freqs) -> [mag, phase]
                let node = self.num(args, 1)? as usize;
                let sr = self.num(args, 2)?;
                let freqs = self.f32_arg(self.arg(args, 3));
                let (b, a) = match self.graph(args)?.node(node).map(|n| &n.kind) {
                    Some(Kind::Iir(f)) => (f.b.clone(), f.a.clone()),
                    _ => (vec![1.0], vec![1.0]),
                };
                let r = frequency_response(&b, &a, sr, &freqs);
                let mag: Vec<f32> = r.iter().map(|x| x.0).collect();
                let ph: Vec<f32> = r.iter().map(|x| x.1).collect();
                self.f32_buffers(&[mag, ph])
            }
        }
    }

    /// `nodeSet(ctx, node, key, a, b, c)`: node attributes that are not
    /// AudioParams.
    fn audio_node_set(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let node = self.num(args, 1)? as usize;
        let key = self.zu_text(self.arg(args, 2))?;
        let (a, b, c) = (self.arg(args, 3), self.arg(args, 4), self.arg(args, 5));
        let num = |e: &mut Engine, v: GVal| e.zu_zahl(v);
        match key.as_str() {
            "start" | "stop" => {
                let when = num(self, a)?;
                let offset = num(self, b).unwrap_or(0.0);
                let duration = num(self, c).unwrap_or(-1.0);
                let start = key == "start";
                if let Some(n) = self.graph(args)?.node(node) {
                    let sched = match &mut n.kind {
                        Kind::Oscillator(o) => Some(&mut o.sched),
                        Kind::BufferSource(bs) => {
                            if start {
                                bs.offset = offset.max(0.0);
                                bs.duration = (duration >= 0.0).then_some(duration);
                            }
                            Some(&mut bs.sched)
                        }
                        Kind::Constant(s) => Some(s),
                        _ => None,
                    };
                    if let Some(s) = sched {
                        if start {
                            s.start = Some(when.max(0.0));
                        } else {
                            s.stop = Some(when.max(0.0));
                        }
                    }
                }
            }
            "wave" => {
                let w = num(self, a)?;
                if let Some(Node { kind: Kind::Oscillator(o), .. }) = self.graph(args)?.node(node) {
                    o.set_wave(wave(w));
                }
            }
            "periodic" => {
                let real = self.f32_arg(a);
                let imag = self.f32_arg(b);
                let normalize = !self.zu_bool(c);
                if let Some(Node { kind: Kind::Oscillator(o), .. }) = self.graph(args)?.node(node) {
                    o.set_wave(Wave::Custom { real: Arc::new(real), imag: Arc::new(imag), normalize });
                }
            }
            "buffer" => {
                let buf = self.buf_arg(a)?;
                if let Some(Node { kind: Kind::BufferSource(bs), .. }) = self.graph(args)?.node(node) {
                    bs.buffer = buf;
                }
            }
            "loop" => {
                let (l, s, e) = (self.zu_bool(a), num(self, b)?, num(self, c)?);
                if let Some(Node { kind: Kind::BufferSource(bs), .. }) = self.graph(args)?.node(node) {
                    bs.looping = l;
                    bs.loop_start = s;
                    bs.loop_end = e;
                }
            }
            "biquadType" => {
                let t = biquad_type(num(self, a)?);
                if let Some(Node { kind: Kind::Biquad(bq), .. }) = self.graph(args)?.node(node) {
                    bq.typ = t;
                }
            }
            "curve" => {
                let curve = if matches!(a, GVal::Null | GVal::Undefiniert) { None } else { Some(Arc::new(self.f32_arg(a))) };
                if let Some(Node { kind: Kind::Shaper(s), .. }) = self.graph(args)?.node(node) {
                    s.curve = curve;
                }
            }
            "channels" => {
                let (cc, mode, ip) = (num(self, a)?, num(self, b)?, num(self, c)?);
                let g = self.graph(args)?;
                if let Some(n) = g.node(node) {
                    n.channel_count = cc.max(1.0) as usize;
                    n.mode = count_mode(mode);
                    n.interp = interp(ip);
                }
                g.changed();
            }
            "panner" => {
                let vals = self.iter_werte(a)?;
                let mut v = [0.0f64; 7];
                for (i, x) in vals.iter().take(7).enumerate() {
                    v[i] = self.zu_zahl(*x)?;
                }
                if let Some(Node { kind: Kind::Panner(p), .. }) = self.graph(args)?.node(node) {
                    p.distance_model = v[0] as u8;
                    p.ref_distance = v[1];
                    p.max_distance = v[2];
                    p.rolloff = v[3];
                    p.cone_inner = v[4];
                    p.cone_outer = v[5];
                    p.cone_outer_gain = v[6];
                }
            }
            "analyser" => {
                let (size, smoothing) = (num(self, a)?, num(self, b)?);
                if let Some(Node { kind: Kind::Analyser(an), .. }) = self.graph(args)?.node(node) {
                    an.fft_size = (size as usize).clamp(32, 32768);
                    an.smoothing = smoothing;
                }
            }
            "convolver" => {
                let buf = self.buf_arg(a)?;
                let normalize = self.zu_bool(b);
                if let Some(Node { kind: Kind::Convolver(cv), .. }) = self.graph(args)?.node(node) {
                    cv.set_ir(buf.as_deref(), normalize);
                }
            }
            _ => {}
        }
        Ok(GVal::Undefiniert)
    }
}

fn time_of(ev: &Ev) -> f64 {
    match ev {
        Ev::Set(t, _) | Ev::Linear(t, _) | Ev::Exp(t, _) | Ev::Target(t, _, _) | Ev::Curve(t, _, _) => *t,
    }
}
