//! Web Audio rendering (2026-10-15): the audio graph of one
//! BaseAudioContext, rendered in render quanta of 128 frames
//! (https://webaudio.github.io/web-audio-api/). `web_audio.js` mirrors every
//! graph change into a [`Graph`]; an OfflineAudioContext renders its whole
//! length at once, a running AudioContext renders up to its current time
//! while sources play (for `ended` events and AnalyserNode data; there is
//! no audio device output yet).
//!
//! The node algorithms follow the specification: channel up/down-mixing,
//! the AudioParam automation timeline, band-limited oscillators from
//! PeriodicWave coefficients, the biquad formulas of the "Audio EQ
//! Cookbook" as the spec gives them, equal-power panning, partitioned FFT
//! convolution, and a soft-knee compressor.

use std::f64::consts::PI;
use std::sync::Arc;

const Q: usize = 128;
type Block = [f32; Q];
const SILENT: Block = [0.0; Q];

// ---------------------------------------------------------------------
// FFT (radix 2, in place) — analyser, convolver and wave tables
// ---------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
struct C {
    re: f64,
    im: f64,
}

fn fft(a: &mut [C], inverse: bool) {
    let n = a.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            a.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = 2.0 * PI / len as f64 * if inverse { 1.0 } else { -1.0 };
        let (ws, wc) = ang.sin_cos();
        for start in (0..n).step_by(len) {
            let (mut wr, mut wi) = (1.0, 0.0);
            for k in 0..len / 2 {
                let u = a[start + k];
                let v = a[start + k + len / 2];
                let tr = v.re * wr - v.im * wi;
                let ti = v.re * wi + v.im * wr;
                a[start + k] = C { re: u.re + tr, im: u.im + ti };
                a[start + k + len / 2] = C { re: u.re - tr, im: u.im - ti };
                let nwr = wr * wc - wi * ws;
                wi = wr * ws + wi * wc;
                wr = nwr;
            }
        }
        len <<= 1;
    }
    if inverse {
        for x in a.iter_mut() {
            x.re /= n as f64;
            x.im /= n as f64;
        }
    }
}

// ---------------------------------------------------------------------
// Channel mixing (spec §"Channel Up-Mixing and Down-Mixing")
// ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CountMode {
    Max,
    ClampedMax,
    Explicit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Interp {
    Speakers,
    Discrete,
}

/// Adds `src` (with `src.len()` channels) into `dst` (with `dst.len()`
/// channels) with the speaker or discrete mixing rules.
fn mix_into(dst: &mut [Block], src: &[Block], interp: Interp) {
    let (n, m) = (src.len(), dst.len());
    let add = |d: &mut Block, s: &Block, g: f32| {
        for i in 0..Q {
            d[i] += s[i] * g;
        }
    };
    if n == m {
        for c in 0..n {
            add(&mut dst[c], &src[c], 1.0);
        }
        return;
    }
    let h = std::f32::consts::FRAC_1_SQRT_2;
    if interp == Interp::Speakers {
        match (n, m) {
            (1, 2) | (1, 4) => {
                add(&mut dst[0], &src[0], 1.0);
                add(&mut dst[1], &src[0], 1.0);
                return;
            }
            (1, 6) => {
                add(&mut dst[2], &src[0], 1.0);
                return;
            }
            (2, 4) | (2, 6) => {
                add(&mut dst[0], &src[0], 1.0);
                add(&mut dst[1], &src[1], 1.0);
                return;
            }
            (4, 6) => {
                add(&mut dst[0], &src[0], 1.0);
                add(&mut dst[1], &src[1], 1.0);
                add(&mut dst[4], &src[2], 1.0);
                add(&mut dst[5], &src[3], 1.0);
                return;
            }
            (2, 1) => {
                add(&mut dst[0], &src[0], 0.5);
                add(&mut dst[0], &src[1], 0.5);
                return;
            }
            (4, 1) => {
                for c in 0..4 {
                    add(&mut dst[0], &src[c], 0.25);
                }
                return;
            }
            (6, 1) => {
                add(&mut dst[0], &src[0], h);
                add(&mut dst[0], &src[1], h);
                add(&mut dst[0], &src[2], 1.0);
                add(&mut dst[0], &src[4], 0.5);
                add(&mut dst[0], &src[5], 0.5);
                return;
            }
            (4, 2) => {
                add(&mut dst[0], &src[0], 0.5);
                add(&mut dst[0], &src[2], 0.5);
                add(&mut dst[1], &src[1], 0.5);
                add(&mut dst[1], &src[3], 0.5);
                return;
            }
            (6, 2) => {
                add(&mut dst[0], &src[0], 1.0);
                add(&mut dst[0], &src[2], h);
                add(&mut dst[0], &src[4], h);
                add(&mut dst[1], &src[1], 1.0);
                add(&mut dst[1], &src[2], h);
                add(&mut dst[1], &src[5], h);
                return;
            }
            (6, 4) => {
                add(&mut dst[0], &src[0], 1.0);
                add(&mut dst[0], &src[2], h);
                add(&mut dst[1], &src[1], 1.0);
                add(&mut dst[1], &src[2], h);
                add(&mut dst[2], &src[4], 1.0);
                add(&mut dst[3], &src[5], 1.0);
                return;
            }
            _ => {}
        }
    }
    for c in 0..n.min(m) {
        add(&mut dst[c], &src[c], 1.0);
    }
}

// ---------------------------------------------------------------------
// AudioParam
// ---------------------------------------------------------------------

#[derive(Clone, Debug)]
pub(crate) enum Ev {
    Set(f64, f32),
    Linear(f64, f32),
    Exp(f64, f32),
    Target(f64, f32, f64),
    Curve(f64, f64, Arc<Vec<f32>>),
}

impl Ev {
    fn time(&self) -> f64 {
        match self {
            Ev::Set(t, _) | Ev::Linear(t, _) | Ev::Exp(t, _) | Ev::Target(t, _, _) | Ev::Curve(t, _, _) => *t,
        }
    }
    fn is_ramp(&self) -> bool {
        matches!(self, Ev::Linear(..) | Ev::Exp(..))
    }
}

fn curve_at(vals: &[f32], t0: f64, dur: f64, t: f64) -> f64 {
    let n = vals.len();
    if n == 0 {
        return 0.0;
    }
    if t >= t0 + dur || n == 1 {
        return vals[n - 1] as f64;
    }
    let pos = (t - t0) / dur * (n - 1) as f64;
    let k = pos.floor().max(0.0) as usize;
    if k + 1 >= n {
        return vals[n - 1] as f64;
    }
    let f = pos - k as f64;
    vals[k] as f64 * (1.0 - f) + vals[k + 1] as f64 * f
}

pub(crate) struct Param {
    /// The intrinsic value without automation (the `value` setter).
    pub(crate) value: f32,
    min: f32,
    max: f32,
    pub(crate) a_rate: bool,
    events: Vec<Ev>,
    /// starts[k]: the value just before event k begins.
    starts: Vec<f64>,
    pub(crate) inputs: Vec<(usize, usize)>,
    vals: Block,
    pub(crate) last: f32,
}

impl Param {
    pub(crate) fn new(value: f32, min: f32, max: f32, a_rate: bool) -> Self {
        Param { value, min, max, a_rate, events: Vec::new(), starts: Vec::new(), inputs: Vec::new(), vals: [value; Q], last: value }
    }

    pub(crate) fn set_value(&mut self, v: f32) {
        self.value = v;
        self.last = v;
        self.recompute();
    }

    /// Inserts an event after the events at or before its time.
    pub(crate) fn add(&mut self, ev: Ev) {
        let t = ev.time();
        // A setValueCurve may not overlap other events (the glue checks);
        // an event of the same type at the same time replaces it.
        if let Some(i) = self.events.iter().position(|e| e.time() == t && std::mem::discriminant(e) == std::mem::discriminant(&ev)) {
            self.events[i] = ev;
        } else {
            let i = self.events.partition_point(|e| e.time() <= t);
            self.events.insert(i, ev);
        }
        self.recompute();
    }

    pub(crate) fn has_events(&self) -> bool {
        !self.events.is_empty()
    }

    /// The end time of the last event (`setValueCurveAtTime` overlap checks).
    pub(crate) fn curve_overlaps(&self, t: f64, dur: f64) -> bool {
        self.events.iter().any(|e| match e {
            Ev::Curve(t0, d0, _) => t < t0 + d0 && *t0 < t + dur.max(0.0) || (t >= *t0 && t < t0 + d0),
            other => other.time() > t && other.time() < t + dur,
        })
    }

    pub(crate) fn cancel(&mut self, t: f64) {
        self.events.retain(|e| e.time() < t);
        self.recompute();
    }

    pub(crate) fn cancel_hold(&mut self, t: f64) {
        let v = self.eval(self.events.len(), t) as f32;
        let had_ramp = self.events.iter().any(|e| e.time() > t && e.is_ramp());
        let mut kept: Vec<Ev> = Vec::new();
        for e in self.events.drain(..) {
            match e {
                Ev::Curve(t0, d, vals) if t0 <= t && t < t0 + d => {
                    // Truncate the curve at t (its values scaled to the new duration).
                    let n = vals.len();
                    let keep = (((t - t0) / d) * (n.saturating_sub(1)) as f64).floor() as usize + 1;
                    let mut nv: Vec<f32> = vals[..keep.min(n)].to_vec();
                    nv.push(v);
                    let nd = (t - t0).max(1e-9);
                    kept.push(Ev::Curve(t0, nd, Arc::new(nv)));
                }
                e if e.time() <= t => kept.push(e),
                _ => {}
            }
        }
        self.events = kept;
        if had_ramp {
            self.events.push(Ev::Linear(t, v));
        } else {
            self.events.push(Ev::Set(t, v));
        }
        self.recompute();
    }

    fn recompute(&mut self) {
        let mut starts = Vec::with_capacity(self.events.len());
        for k in 0..self.events.len() {
            let t = self.events[k].time();
            let v = if k == 0 { self.value as f64 } else { self.held_with(&starts, k - 1, t) };
            starts.push(v);
        }
        self.starts = starts;
    }

    /// Value of event k for a time after it (no following ramp).
    fn held_with(&self, starts: &[f64], k: usize, t: f64) -> f64 {
        match &self.events[k] {
            Ev::Set(_, v) | Ev::Linear(_, v) | Ev::Exp(_, v) => *v as f64,
            Ev::Target(t0, v1, tau) => {
                let v0 = starts[k];
                let v1 = *v1 as f64;
                if *tau <= 0.0 { v1 } else { v1 + (v0 - v1) * (-(t - t0) / tau).exp() }
            }
            Ev::Curve(t0, d, vals) => curve_at(vals, *t0, *d, t),
        }
    }

    /// (time, value) where event k hands over to a following ramp.
    fn end_of(&self, k: usize) -> (f64, f64) {
        match &self.events[k] {
            Ev::Set(t, v) | Ev::Linear(t, v) | Ev::Exp(t, v) => (*t, *v as f64),
            Ev::Target(t0, _, _) => (*t0, self.starts[k]),
            Ev::Curve(t0, d, vals) => (t0 + d, vals.last().copied().unwrap_or(0.0) as f64),
        }
    }

    /// The automation value at `t` over the first `m` events.
    fn eval(&self, m: usize, t: f64) -> f64 {
        let ev = &self.events[..m];
        let n = ev.partition_point(|e| e.time() <= t);
        if n < m && ev[n].is_ramp() {
            let (t0, v0) = if n == 0 { (0.0, self.value as f64) } else { self.end_of(n - 1) };
            let (t1, v1) = match &ev[n] {
                Ev::Linear(t, v) | Ev::Exp(t, v) => (*t, *v as f64),
                _ => (t, 0.0),
            };
            if t1 <= t0 || t < t0 {
                return v0;
            }
            let f = (t - t0) / (t1 - t0);
            return if matches!(ev[n], Ev::Linear(..)) {
                v0 + (v1 - v0) * f
            } else if v0 == 0.0 || v0 * v1 < 0.0 {
                v0
            } else {
                v0 * (v1 / v0).powf(f)
            };
        }
        if n == 0 {
            return self.value as f64;
        }
        self.held_with(&self.starts, n - 1, t)
    }

    /// The automation values of one quantum starting at `t0` (before the
    /// input connections are added).
    fn automate(&mut self, t0: f64, sr: f64) {
        // Events wholly in the past fold into the intrinsic value (a page
        // that sets `value` every frame adds an event every frame).
        let k = self.events.partition_point(|e| e.time() <= t0);
        if k > 1 {
            let keep = k - 1;
            self.value = self.starts[keep] as f32;
            self.events.drain(..keep);
            self.recompute();
        }
        if self.events.is_empty() {
            self.vals = [self.value; Q];
            return;
        }
        let last = self.events.len() - 1;
        let settled = self.events[last].time() <= t0 && matches!(self.events[last], Ev::Set(..) | Ev::Linear(..) | Ev::Exp(..));
        if settled || !self.a_rate {
            let v = self.eval(self.events.len(), t0) as f32;
            self.vals = [v; Q];
            return;
        }
        for i in 0..Q {
            self.vals[i] = self.eval(self.events.len(), t0 + i as f64 / sr) as f32;
        }
    }

    fn clamp_block(&mut self) {
        let (lo, hi) = (self.min, self.max);
        for v in self.vals.iter_mut() {
            if v.is_nan() {
                *v = self.value;
            }
            *v = v.clamp(lo, hi);
        }
        self.last = if self.a_rate { self.vals[Q - 1] } else { self.vals[0] };
    }
}

// ---------------------------------------------------------------------
// Nodes
// ---------------------------------------------------------------------

/// Start/stop of a scheduled source.
#[derive(Default, Clone)]
pub(crate) struct Sched {
    pub(crate) start: Option<f64>,
    pub(crate) stop: Option<f64>,
    ended: bool,
}

impl Sched {
    /// The frames of this quantum the source plays: `(first, end)`.
    fn window(&self, frame0: u64, sr: f64) -> Option<(usize, usize)> {
        let start = self.start?;
        if self.ended {
            return None;
        }
        let s = ((start * sr).ceil() as i64 - frame0 as i64).clamp(0, Q as i64) as usize;
        let e = match self.stop {
            Some(st) => ((st.max(start) * sr).ceil() as i64 - frame0 as i64).clamp(0, Q as i64) as usize,
            None => Q,
        };
        if s >= Q {
            return Some((Q, Q));
        }
        Some((s, e.max(s)))
    }

    fn stopped_by(&self, frame_end: u64, sr: f64) -> bool {
        self.stop.is_some_and(|st| (st * sr).ceil() as u64 <= frame_end)
    }
}

/// Built-in oscillator shapes and custom PeriodicWaves.
#[derive(Clone)]
pub(crate) enum Wave {
    Sine,
    Square,
    Sawtooth,
    Triangle,
    Custom { real: Arc<Vec<f32>>, imag: Arc<Vec<f32>>, normalize: bool },
}

const TABLE: usize = 4096;

struct WaveTables {
    /// tables[k]: at most 2^k partials.
    tables: Vec<Option<Vec<f32>>>,
    scale: Option<f64>,
}

impl Wave {
    fn coef(&self, n: usize) -> (f64, f64) {
        let nf = n as f64;
        match self {
            Wave::Sine => (0.0, if n == 1 { 1.0 } else { 0.0 }),
            Wave::Square => (0.0, if n % 2 == 1 { 4.0 / (nf * PI) } else { 0.0 }),
            Wave::Sawtooth => (0.0, if n % 2 == 1 { 2.0 / (nf * PI) } else { -2.0 / (nf * PI) }),
            Wave::Triangle => (0.0, 8.0 * (nf * PI / 2.0).sin() / (nf * PI).powi(2)),
            Wave::Custom { real, imag, .. } => (
                real.get(n).copied().unwrap_or(0.0) as f64,
                imag.get(n).copied().unwrap_or(0.0) as f64,
            ),
        }
    }

    fn partials(&self) -> usize {
        match self {
            Wave::Sine => 1,
            Wave::Custom { real, imag, .. } => real.len().max(imag.len()).saturating_sub(1).clamp(1, TABLE / 2 - 1),
            _ => TABLE / 2 - 1,
        }
    }

    /// One period with the first `p` partials (unnormalized).
    fn build(&self, p: usize) -> Vec<f32> {
        let mut a = vec![C::default(); TABLE];
        for n in 1..=p.min(TABLE / 2 - 1) {
            let (re, im) = self.coef(n);
            // x(t) = sum re cos + im sin  ->  X[n] = (re - i im)/2 * N.
            a[n] = C { re: re / 2.0 * TABLE as f64, im: -im / 2.0 * TABLE as f64 };
            a[TABLE - n] = C { re: re / 2.0 * TABLE as f64, im: im / 2.0 * TABLE as f64 };
        }
        fft(&mut a, true);
        a.iter().map(|c| c.re as f32).collect()
    }
}

pub(crate) struct Osc {
    pub(crate) sched: Sched,
    pub(crate) wave: Wave,
    tables: WaveTables,
    phase: f64,
}

impl Osc {
    pub(crate) fn new(wave: Wave) -> Self {
        Osc { sched: Sched::default(), wave, tables: WaveTables { tables: vec![None; 12], scale: None }, phase: 0.0 }
    }

    pub(crate) fn set_wave(&mut self, wave: Wave) {
        self.wave = wave;
        self.tables = WaveTables { tables: vec![None; 12], scale: None };
    }

    /// The wave table with at most `max_partials` partials (rounded down
    /// to a power of two), normalized by the peak of the full-band wave.
    fn table<'a>(tables: &'a mut WaveTables, wave: &Wave, max_partials: usize) -> &'a [f32] {
        let p = max_partials.clamp(1, wave.partials());
        let k = ((usize::BITS - 1 - p.leading_zeros()) as usize).min(11);
        if tables.scale.is_none() {
            let normalize = !matches!(wave, Wave::Custom { normalize: false, .. });
            let full = wave.build(wave.partials());
            let peak = full.iter().fold(0.0f32, |m, v| m.max(v.abs())) as f64;
            tables.scale = Some(if normalize && peak > 0.0 { 1.0 / peak } else { 1.0 });
        }
        if tables.tables[k].is_none() {
            let scale = tables.scale.unwrap_or(1.0) as f32;
            let t: Vec<f32> = wave.build(1 << k).into_iter().map(|v| v * scale).collect();
            tables.tables[k] = Some(t);
        }
        tables.tables[k].as_deref().unwrap_or(&[])
    }
}

/// Decoded/created audio data shared with the glue's AudioBuffer.
pub(crate) struct Buf {
    pub(crate) sr: f64,
    pub(crate) ch: Vec<Vec<f32>>,
}

pub(crate) struct BufferSource {
    pub(crate) sched: Sched,
    pub(crate) buffer: Option<Arc<Buf>>,
    pub(crate) looping: bool,
    pub(crate) loop_start: f64,
    pub(crate) loop_end: f64,
    pub(crate) offset: f64,
    pub(crate) duration: Option<f64>,
    pos: f64,
    played: f64,
    began: bool,
}

impl BufferSource {
    pub(crate) fn new() -> Self {
        BufferSource {
            sched: Sched::default(),
            buffer: None,
            looping: false,
            loop_start: 0.0,
            loop_end: 0.0,
            offset: 0.0,
            duration: None,
            pos: 0.0,
            played: 0.0,
            began: false,
        }
    }
}

pub(crate) struct Delay {
    max: f64,
    buf: Vec<Vec<f32>>,
    write: usize,
    cycle: bool,
}

impl Delay {
    pub(crate) fn new(max: f64) -> Self {
        Delay { max, buf: Vec::new(), write: 0, cycle: false }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BiquadType {
    Lowpass,
    Highpass,
    Bandpass,
    Lowshelf,
    Highshelf,
    Peaking,
    Notch,
    Allpass,
}

pub(crate) struct Biquad {
    pub(crate) typ: BiquadType,
    state: Vec<[f64; 4]>,
}

impl Biquad {
    pub(crate) fn new() -> Self {
        Biquad { typ: BiquadType::Lowpass, state: Vec::new() }
    }
}

/// Normalized biquad coefficients `[b0, b1, b2, a1, a2]` (spec §"BiquadFilterNode").
pub(crate) fn biquad_coefs(typ: BiquadType, sr: f64, freq: f64, q: f64, gain: f64, detune: f64) -> [f64; 5] {
    let nyquist = sr / 2.0;
    let f0 = (freq * 2f64.powf(detune / 1200.0)).clamp(0.0, nyquist);
    let w0 = 2.0 * PI * f0 / sr;
    let (sinw, cosw) = w0.sin_cos();
    let a = 10f64.powf(gain / 40.0);
    let alpha_q = sinw / (2.0 * q);
    let alpha_qdb = sinw / (2.0 * 10f64.powf(q / 20.0));
    let alpha_s = sinw / 2.0 * 2f64.sqrt();
    let sa = a.sqrt();
    let (b0, b1, b2, a0, a1, a2) = match typ {
        BiquadType::Lowpass => ((1.0 - cosw) / 2.0, 1.0 - cosw, (1.0 - cosw) / 2.0, 1.0 + alpha_qdb, -2.0 * cosw, 1.0 - alpha_qdb),
        BiquadType::Highpass => ((1.0 + cosw) / 2.0, -(1.0 + cosw), (1.0 + cosw) / 2.0, 1.0 + alpha_qdb, -2.0 * cosw, 1.0 - alpha_qdb),
        BiquadType::Bandpass => (alpha_q, 0.0, -alpha_q, 1.0 + alpha_q, -2.0 * cosw, 1.0 - alpha_q),
        BiquadType::Notch => (1.0, -2.0 * cosw, 1.0, 1.0 + alpha_q, -2.0 * cosw, 1.0 - alpha_q),
        BiquadType::Allpass => (1.0 - alpha_q, -2.0 * cosw, 1.0 + alpha_q, 1.0 + alpha_q, -2.0 * cosw, 1.0 - alpha_q),
        BiquadType::Peaking => (1.0 + alpha_q * a, -2.0 * cosw, 1.0 - alpha_q * a, 1.0 + alpha_q / a, -2.0 * cosw, 1.0 - alpha_q / a),
        BiquadType::Lowshelf => (
            a * ((a + 1.0) - (a - 1.0) * cosw + 2.0 * alpha_s * sa),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cosw),
            a * ((a + 1.0) - (a - 1.0) * cosw - 2.0 * alpha_s * sa),
            (a + 1.0) + (a - 1.0) * cosw + 2.0 * alpha_s * sa,
            -2.0 * ((a - 1.0) + (a + 1.0) * cosw),
            (a + 1.0) + (a - 1.0) * cosw - 2.0 * alpha_s * sa,
        ),
        BiquadType::Highshelf => (
            a * ((a + 1.0) + (a - 1.0) * cosw + 2.0 * alpha_s * sa),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cosw),
            a * ((a + 1.0) + (a - 1.0) * cosw - 2.0 * alpha_s * sa),
            (a + 1.0) - (a - 1.0) * cosw + 2.0 * alpha_s * sa,
            2.0 * ((a - 1.0) - (a + 1.0) * cosw),
            (a + 1.0) - (a - 1.0) * cosw - 2.0 * alpha_s * sa,
        ),
    };
    if a0 == 0.0 || !a0.is_finite() {
        return [1.0, 0.0, 0.0, 0.0, 0.0];
    }
    [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0]
}

/// `getFrequencyResponse` of a direct-form filter `b / a` (a[0] = 1).
pub(crate) fn frequency_response(b: &[f64], a: &[f64], sr: f64, freqs: &[f32]) -> Vec<(f32, f32)> {
    freqs
        .iter()
        .map(|&f| {
            let f = f as f64;
            if !(0.0..=sr / 2.0).contains(&f) {
                return (f32::NAN, f32::NAN);
            }
            let w = 2.0 * PI * f / sr;
            let eval = |c: &[f64]| {
                let (mut re, mut im) = (0.0, 0.0);
                for (k, v) in c.iter().enumerate() {
                    let (s, co) = (w * k as f64).sin_cos();
                    re += v * co;
                    im -= v * s;
                }
                (re, im)
            };
            let (nr, ni) = eval(b);
            let (dr, di) = eval(a);
            let d = dr * dr + di * di;
            let hr = (nr * dr + ni * di) / d;
            let hi = (ni * dr - nr * di) / d;
            (((hr * hr + hi * hi).sqrt()) as f32, hi.atan2(hr) as f32)
        })
        .collect()
}

pub(crate) struct Iir {
    pub(crate) b: Vec<f64>,
    pub(crate) a: Vec<f64>,
    hist: Vec<(Vec<f64>, Vec<f64>)>,
}

impl Iir {
    pub(crate) fn new(ff: &[f32], fb: &[f32]) -> Self {
        let a0 = fb.first().copied().unwrap_or(1.0) as f64;
        let a0 = if a0 == 0.0 { 1.0 } else { a0 };
        Iir {
            b: ff.iter().map(|v| *v as f64 / a0).collect(),
            a: fb.iter().map(|v| *v as f64 / a0).collect(),
            hist: Vec::new(),
        }
    }
}

pub(crate) struct Shaper {
    pub(crate) curve: Option<Arc<Vec<f32>>>,
}

pub(crate) struct Panner {
    pub(crate) distance_model: u8,
    pub(crate) ref_distance: f64,
    pub(crate) max_distance: f64,
    pub(crate) rolloff: f64,
    pub(crate) cone_inner: f64,
    pub(crate) cone_outer: f64,
    pub(crate) cone_outer_gain: f64,
}

pub(crate) struct Compressor {
    gain_db: f64,
    pub(crate) reduction: f32,
    delay: Vec<Vec<f32>>,
    pos: usize,
}

impl Compressor {
    pub(crate) fn new() -> Self {
        Compressor { gain_db: 0.0, reduction: 0.0, delay: Vec::new(), pos: 0 }
    }
}

pub(crate) struct Analyser {
    pub(crate) fft_size: usize,
    pub(crate) smoothing: f64,
    ring: Vec<f32>,
    pos: usize,
    prev: Vec<f64>,
}

impl Analyser {
    pub(crate) fn new() -> Self {
        Analyser { fft_size: 2048, smoothing: 0.8, ring: vec![0.0; 32768], pos: 0, prev: vec![0.0; 16384] }
    }

    /// The last `n` samples, oldest first.
    pub(crate) fn time_data(&self, n: usize) -> Vec<f32> {
        let len = self.ring.len();
        (0..n).map(|i| self.ring[(self.pos + len - n + i) % len]).collect()
    }

    /// Smoothed magnitudes in dB of the current window (spec
    /// §"FFT Windowing and smoothing over time").
    pub(crate) fn frequency_db(&mut self) -> Vec<f32> {
        let n = self.fft_size;
        let x = self.time_data(n);
        let mut a: Vec<C> = (0..n)
            .map(|i| {
                let w = 0.42 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos() + 0.08 * (4.0 * PI * i as f64 / n as f64).cos();
                C { re: x[i] as f64 * w, im: 0.0 }
            })
            .collect();
        fft(&mut a, false);
        let tau = self.smoothing;
        (0..n / 2)
            .map(|k| {
                let mag = (a[k].re * a[k].re + a[k].im * a[k].im).sqrt() / n as f64;
                let s = tau * self.prev[k] + (1.0 - tau) * mag;
                let s = if s.is_finite() { s } else { 0.0 };
                self.prev[k] = s;
                (20.0 * s.log10()) as f32
            })
            .collect()
    }
}

pub(crate) struct Convolver {
    /// Partition spectra per IR channel.
    parts: Vec<Vec<Vec<C>>>,
    /// Past input spectra per input channel (newest first).
    fdl: Vec<Vec<Vec<C>>>,
    prev_in: Vec<Block>,
    pub(crate) ir_channels: usize,
}

impl Convolver {
    pub(crate) fn new() -> Self {
        Convolver { parts: Vec::new(), fdl: Vec::new(), prev_in: Vec::new(), ir_channels: 0 }
    }

    /// Sets the impulse response (spec normalization when asked).
    pub(crate) fn set_ir(&mut self, buf: Option<&Buf>, normalize: bool) {
        self.parts.clear();
        self.fdl.clear();
        self.prev_in.clear();
        self.ir_channels = 0;
        let Some(b) = buf else { return };
        let len = b.ch.first().map_or(0, Vec::len);
        if len == 0 {
            return;
        }
        let mut scale = 1.0f64;
        if normalize {
            let mut power = 0.0f64;
            for c in &b.ch {
                for v in c {
                    power += (*v as f64) * (*v as f64);
                }
            }
            power = (power / (b.ch.len() * len) as f64).sqrt();
            if !power.is_finite() || power < 0.000125 {
                power = 0.000125;
            }
            scale = 1.0 / power * 0.00125 * 44100.0 / b.sr;
            if b.ch.len() == 4 {
                scale *= 0.5;
            }
        }
        for c in &b.ch {
            let mut parts = Vec::new();
            for chunk in c.chunks(Q) {
                let mut a = vec![C::default(); 2 * Q];
                for (i, v) in chunk.iter().enumerate() {
                    a[i].re = *v as f64 * scale;
                }
                fft(&mut a, false);
                parts.push(a);
            }
            self.parts.push(parts);
        }
        self.ir_channels = b.ch.len();
    }

    /// Convolves one input block of channel `ic` with IR channel `k`
    /// (the input spectra must have been pushed for this quantum).
    fn convolve(&self, ic: usize, k: usize, out: &mut Block) {
        let parts = &self.parts[k];
        let fdl = &self.fdl[ic];
        let mut acc = vec![C::default(); 2 * Q];
        for (p, h) in parts.iter().enumerate() {
            let Some(x) = fdl.get(p) else { break };
            for i in 0..2 * Q {
                acc[i].re += x[i].re * h[i].re - x[i].im * h[i].im;
                acc[i].im += x[i].re * h[i].im + x[i].im * h[i].re;
            }
        }
        fft(&mut acc, true);
        for i in 0..Q {
            out[i] += acc[Q + i].re as f32;
        }
    }

    fn push_input(&mut self, input: &[Block]) {
        while self.fdl.len() < input.len() {
            self.fdl.push(Vec::new());
            self.prev_in.push(SILENT);
        }
        let parts = self.parts.first().map_or(0, Vec::len);
        for (c, block) in input.iter().enumerate() {
            let mut a = vec![C::default(); 2 * Q];
            for i in 0..Q {
                a[i].re = self.prev_in[c][i] as f64;
                a[Q + i].re = block[i] as f64;
            }
            fft(&mut a, false);
            self.fdl[c].insert(0, a);
            self.fdl[c].truncate(parts);
            self.prev_in[c] = *block;
        }
    }
}

pub(crate) enum Kind {
    Destination,
    Gain,
    Oscillator(Box<Osc>),
    BufferSource(Box<BufferSource>),
    Constant(Sched),
    Delay(Box<Delay>),
    Biquad(Box<Biquad>),
    Iir(Box<Iir>),
    Shaper(Shaper),
    StereoPanner,
    Panner(Box<Panner>),
    Compressor(Box<Compressor>),
    Analyser(Box<Analyser>),
    Splitter,
    Merger,
    Convolver(Box<Convolver>),
    /// Nodes whose input the engine cannot produce yet (script processor,
    /// media element/stream sources, worklets): silence with this many
    /// output channels.
    Silent(usize),
    /// The listener's nine AudioParams (position, forward, up).
    Listener,
}

pub(crate) struct Node {
    pub(crate) kind: Kind,
    pub(crate) inputs: Vec<Vec<(usize, usize)>>,
    pub(crate) params: Vec<Param>,
    pub(crate) n_out: usize,
    pub(crate) channel_count: usize,
    pub(crate) mode: CountMode,
    pub(crate) interp: Interp,
    out: Vec<Vec<Block>>,
    muted: bool,
}

impl Node {
    pub(crate) fn new(kind: Kind, n_in: usize, n_out: usize, params: Vec<Param>, channel_count: usize, mode: CountMode, interp: Interp) -> Self {
        Node {
            kind,
            inputs: vec![Vec::new(); n_in],
            params,
            n_out,
            channel_count,
            mode,
            interp,
            out: vec![vec![SILENT]; n_out],
            muted: false,
        }
    }
}

// ---------------------------------------------------------------------
// The graph
// ---------------------------------------------------------------------

pub(crate) struct Graph {
    pub(crate) sr: f64,
    pub(crate) channels: usize,
    pub(crate) nodes: Vec<Option<Node>>,
    pub(crate) frame: u64,
    order: Option<Vec<usize>>,
    pub(crate) listener: usize,
}

impl Graph {
    pub(crate) fn new(sr: f64, channels: usize) -> Self {
        let mut g = Graph { sr, channels, nodes: Vec::new(), frame: 0, order: None, listener: 0 };
        g.add(Node::new(Kind::Destination, 1, 1, Vec::new(), channels, CountMode::Explicit, Interp::Speakers));
        let mut lp = Vec::new();
        for v in [0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0] {
            lp.push(Param::new(v, f32::MIN, f32::MAX, true));
        }
        g.listener = g.add(Node::new(Kind::Listener, 0, 0, lp, 1, CountMode::Max, Interp::Speakers));
        g
    }

    pub(crate) fn add(&mut self, n: Node) -> usize {
        self.nodes.push(Some(n));
        self.order = None;
        self.nodes.len() - 1
    }

    pub(crate) fn node(&mut self, id: usize) -> Option<&mut Node> {
        self.nodes.get_mut(id).and_then(|n| n.as_mut())
    }

    pub(crate) fn changed(&mut self) {
        self.order = None;
    }

    pub(crate) fn connect(&mut self, src: usize, out: usize, dst: usize, input: usize) {
        if let Some(n) = self.node(dst)
            && let Some(list) = n.inputs.get_mut(input)
            && !list.contains(&(src, out))
        {
            list.push((src, out));
        }
        self.order = None;
    }

    pub(crate) fn connect_param(&mut self, src: usize, out: usize, dst: usize, param: usize) {
        if let Some(n) = self.node(dst)
            && let Some(p) = n.params.get_mut(param)
            && !p.inputs.contains(&(src, out))
        {
            p.inputs.push((src, out));
        }
        self.order = None;
    }

    /// Removes connections from `src` (output `out`, or all when None) to
    /// `dst` (all nodes when None), input `input` (all when None).
    pub(crate) fn disconnect(&mut self, src: usize, out: Option<usize>, dst: Option<usize>, input: Option<usize>, params: bool) {
        let keep = |c: &(usize, usize)| !(c.0 == src && out.is_none_or(|o| o == c.1));
        for (id, n) in self.nodes.iter_mut().enumerate() {
            let Some(n) = n else { continue };
            if dst.is_some_and(|d| d != id) {
                continue;
            }
            if !params || dst.is_none() {
                for (i, list) in n.inputs.iter_mut().enumerate() {
                    if input.is_none_or(|x| x == i) {
                        list.retain(keep);
                    }
                }
            }
            if params || dst.is_none() {
                for (i, p) in n.params.iter_mut().enumerate() {
                    if !params || input.is_none_or(|x| x == i) {
                        p.inputs.retain(keep);
                    }
                }
            }
        }
        self.order = None;
    }

    /// Processing order: dependencies first; nodes on a cycle without a
    /// DelayNode are muted, DelayNodes on a cycle read before they write.
    fn compute_order(&mut self) -> Vec<usize> {
        let n = self.nodes.len();
        let deps: Vec<Vec<usize>> = (0..n)
            .map(|i| match &self.nodes[i] {
                Some(node) => {
                    let mut d: Vec<usize> = node.inputs.iter().flatten().map(|c| c.0).collect();
                    d.extend(node.params.iter().flat_map(|p| p.inputs.iter().map(|c| c.0)));
                    d.sort_unstable();
                    d.dedup();
                    d.retain(|&x| x < n && self.nodes[x].is_some());
                    d
                }
                None => Vec::new(),
            })
            .collect();
        // Tarjan's strongly connected components (iterative).
        let mut index = vec![usize::MAX; n];
        let mut low = vec![0usize; n];
        let mut on_stack = vec![false; n];
        let mut stack: Vec<usize> = Vec::new();
        let mut comps: Vec<Vec<usize>> = Vec::new();
        let mut counter = 0;
        for root in 0..n {
            if index[root] != usize::MAX || self.nodes[root].is_none() {
                continue;
            }
            let mut work: Vec<(usize, usize)> = vec![(root, 0)];
            index[root] = counter;
            low[root] = counter;
            counter += 1;
            stack.push(root);
            on_stack[root] = true;
            while let Some(&mut (v, ref mut ei)) = work.last_mut() {
                if *ei < deps[v].len() {
                    let w = deps[v][*ei];
                    *ei += 1;
                    if index[w] == usize::MAX {
                        index[w] = counter;
                        low[w] = counter;
                        counter += 1;
                        stack.push(w);
                        on_stack[w] = true;
                        work.push((w, 0));
                    } else if on_stack[w] {
                        low[v] = low[v].min(index[w]);
                    }
                } else {
                    work.pop();
                    if let Some(&(parent, _)) = work.last() {
                        low[parent] = low[parent].min(low[v]);
                    }
                    if low[v] == index[v] {
                        let mut comp = Vec::new();
                        while let Some(x) = stack.pop() {
                            on_stack[x] = false;
                            comp.push(x);
                            if x == v {
                                break;
                            }
                        }
                        comps.push(comp);
                    }
                }
            }
        }
        let mut order = Vec::with_capacity(n);
        for comp in comps {
            let cyclic = comp.len() > 1 || deps[comp[0]].contains(&comp[0]);
            for &x in &comp {
                if let Some(node) = self.nodes[x].as_mut() {
                    node.muted = false;
                    if let Kind::Delay(d) = &mut node.kind {
                        d.cycle = false;
                    }
                }
            }
            if !cyclic {
                order.push(comp[0]);
                continue;
            }
            let delays: Vec<usize> = comp.iter().copied().filter(|&x| matches!(self.nodes[x].as_ref().map(|n| &n.kind), Some(Kind::Delay(_)))).collect();
            if delays.is_empty() {
                for &x in &comp {
                    if let Some(node) = self.nodes[x].as_mut() {
                        node.muted = true;
                    }
                    order.push(x);
                }
                continue;
            }
            for &d in &delays {
                if let Some(Node { kind: Kind::Delay(dl), .. }) = self.nodes[d].as_mut() {
                    dl.cycle = true;
                }
            }
            // Inside the component: a DFS order that ignores the inputs of
            // the cycle delays (their output comes from their history).
            let in_comp = |x: usize| comp.contains(&x);
            let mut done: Vec<usize> = Vec::new();
            let mut visiting: Vec<usize> = Vec::new();
            fn visit(x: usize, deps: &[Vec<usize>], delays: &[usize], in_comp: &dyn Fn(usize) -> bool, done: &mut Vec<usize>, visiting: &mut Vec<usize>) -> bool {
                if done.contains(&x) {
                    return true;
                }
                if visiting.contains(&x) {
                    return false;
                }
                visiting.push(x);
                if !delays.contains(&x) {
                    for &w in &deps[x] {
                        if in_comp(w) && !visit(w, deps, delays, in_comp, done, visiting) {
                            return false;
                        }
                    }
                }
                visiting.retain(|&y| y != x);
                done.push(x);
                true
            }
            let mut ok = true;
            for &x in &comp {
                ok &= visit(x, &deps, &delays, &in_comp, &mut done, &mut visiting);
            }
            if !ok {
                for &x in &comp {
                    if let Some(node) = self.nodes[x].as_mut() {
                        node.muted = true;
                    }
                }
                order.extend(comp);
            } else {
                order.extend(done);
            }
        }
        order
    }

    /// Renders `frames` frames (a multiple of the quantum is rendered;
    /// the extra frames are dropped). Returns the destination channels and
    /// the ids of sources that ended.
    pub(crate) fn render(&mut self, frames: usize) -> (Vec<Vec<f32>>, Vec<usize>) {
        let mut out = vec![Vec::with_capacity(frames); self.channels];
        let mut ended = Vec::new();
        let mut done = 0;
        while done < frames {
            if self.order.is_none() {
                let o = self.compute_order();
                self.order = Some(o);
            }
            let order = self.order.take().unwrap_or_default();
            for &id in &order {
                self.process(id, &mut ended);
            }
            // Cycle delays take their input after everything ran.
            for &id in &order {
                self.delay_write_cycle(id);
            }
            self.order = Some(order);
            let take = (frames - done).min(Q);
            if let Some(Some(dest)) = self.nodes.first() {
                for c in 0..self.channels {
                    match dest.out[0].get(c) {
                        Some(b) => out[c].extend_from_slice(&b[..take]),
                        None => out[c].extend(std::iter::repeat_n(0.0, take)),
                    }
                }
            }
            done += take;
            self.frame += Q as u64;
        }
        (out, ended)
    }

    /// Renders and discards up to `frame` (a running AudioContext catching up).
    pub(crate) fn advance_to(&mut self, frame: u64, ended: &mut Vec<usize>) {
        while self.frame + Q as u64 <= frame {
            let (_, e) = self.render(Q);
            ended.extend(e);
        }
    }

    /// The output block channels of `(node, output)`.
    fn output_of(&self, src: usize, out: usize) -> Option<&Vec<Block>> {
        self.nodes.get(src)?.as_ref()?.out.get(out)
    }

    /// The mixed input `input` of node `id`: (channels).
    fn mixed_input(&self, id: usize, input: usize) -> Vec<Block> {
        let Some(node) = self.nodes[id].as_ref() else { return vec![SILENT] };
        let conns = &node.inputs[input];
        let max_in = conns.iter().filter_map(|c| self.output_of(c.0, c.1)).map(Vec::len).max().unwrap_or(0);
        let chans = match node.mode {
            CountMode::Max => max_in.max(1),
            CountMode::ClampedMax => max_in.clamp(1, node.channel_count.max(1)),
            CountMode::Explicit => node.channel_count.max(1),
        };
        let mut buf = vec![SILENT; chans];
        for c in conns {
            if let Some(src) = self.output_of(c.0, c.1) {
                mix_into(&mut buf, src, node.interp);
            }
        }
        buf
    }

    /// Computes param `p` of node `id` for this quantum (automation plus
    /// input connections, clamped).
    fn param_block(&mut self, id: usize, p: usize) -> Block {
        let t0 = self.frame as f64 / self.sr;
        let sr = self.sr;
        let inputs = match self.nodes[id].as_mut() {
            Some(n) if p < n.params.len() => {
                n.params[p].automate(t0, sr);
                // Most params have no input connections: no allocation then.
                if n.params[p].inputs.is_empty() { Vec::new() } else { n.params[p].inputs.clone() }
            }
            _ => return SILENT,
        };
        let mut add = SILENT;
        let mut any = false;
        for (s, o) in inputs {
            if let Some(src) = self.output_of(s, o) {
                let mut mono = [SILENT];
                mix_into(&mut mono, src, Interp::Speakers);
                for i in 0..Q {
                    add[i] += mono[0][i];
                }
                any = true;
            }
        }
        let Some(n) = self.nodes[id].as_mut() else { return SILENT };
        let param = &mut n.params[p];
        if any {
            if param.a_rate {
                for i in 0..Q {
                    param.vals[i] += add[i];
                }
            } else {
                let v = param.vals[0] + add[0];
                param.vals = [v; Q];
            }
        }
        param.clamp_block();
        param.vals
    }

    fn set_out(&mut self, id: usize, output: usize, blocks: Vec<Block>) {
        if let Some(n) = self.nodes[id].as_mut()
            && let Some(o) = n.out.get_mut(output)
        {
            *o = blocks;
        }
    }

    fn process(&mut self, id: usize, ended: &mut Vec<usize>) {
        let Some(node) = self.nodes[id].as_ref() else { return };
        if node.muted {
            let n_out = node.n_out;
            for o in 0..n_out {
                self.set_out(id, o, vec![SILENT]);
            }
            return;
        }
        let sr = self.sr;
        let frame0 = self.frame;
        let t0 = frame0 as f64 / sr;
        let kind_tag = match &node.kind {
            Kind::Destination => 0,
            Kind::Gain => 1,
            Kind::Oscillator(_) => 2,
            Kind::BufferSource(_) => 3,
            Kind::Constant(_) => 4,
            Kind::Delay(_) => 5,
            Kind::Biquad(_) => 6,
            Kind::Iir(_) => 7,
            Kind::Shaper(_) => 8,
            Kind::StereoPanner => 9,
            Kind::Panner(_) => 10,
            Kind::Compressor(_) => 11,
            Kind::Analyser(_) => 12,
            Kind::Splitter => 13,
            Kind::Merger => 14,
            Kind::Convolver(_) => 15,
            Kind::Silent(_) => 16,
            Kind::Listener => 17,
        };
        match kind_tag {
            0 => {
                let input = self.mixed_input(id, 0);
                self.set_out(id, 0, input);
            }
            1 => {
                let mut input = self.mixed_input(id, 0);
                let g = self.param_block(id, 0);
                for ch in input.iter_mut() {
                    for i in 0..Q {
                        ch[i] *= g[i];
                    }
                }
                self.set_out(id, 0, input);
            }
            2 => {
                let freq = self.param_block(id, 0);
                let detune = self.param_block(id, 1);
                let nyquist = (sr / 2.0) as f32;
                let mut block = SILENT;
                let mut finished = false;
                if let Some(Node { kind: Kind::Oscillator(osc), .. }) = self.nodes[id].as_mut()
                    && let Some((s, e)) = osc.sched.window(frame0, sr)
                {
                    let mut max_f = 0.0f32;
                    let mut fr = [0.0f32; Q];
                    for i in s..e {
                        let f = freq[i] * 2f32.powf(detune[i] / 1200.0);
                        let f = if f.abs() > nyquist { 0.0 } else { f };
                        fr[i] = f;
                        max_f = max_f.max(f.abs());
                    }
                    let partials = if max_f > 0.0 { (nyquist / max_f).floor() as usize } else { TABLE / 2 - 1 };
                    let sine = matches!(osc.wave, Wave::Sine);
                    let mut phase = osc.phase;
                    if sine {
                        for i in s..e {
                            block[i] = (2.0 * PI * phase).sin() as f32;
                            phase += fr[i] as f64 / sr;
                            phase -= phase.floor();
                        }
                    } else {
                        let table = Osc::table(&mut osc.tables, &osc.wave, partials);
                        for i in s..e {
                            let p = phase * TABLE as f64;
                            let k = p.floor() as usize % TABLE;
                            let f = (p - p.floor()) as f32;
                            block[i] = table[k] * (1.0 - f) + table[(k + 1) % TABLE] * f;
                            phase += fr[i] as f64 / sr;
                            phase -= phase.floor();
                        }
                    }
                    osc.phase = phase;
                    if osc.sched.stopped_by(frame0 + Q as u64, sr) {
                        osc.sched.ended = true;
                        finished = true;
                    }
                }
                if finished {
                    ended.push(id);
                }
                self.set_out(id, 0, vec![block]);
            }
            3 => {
                let rate = self.param_block(id, 0)[0] as f64;
                let detune = self.param_block(id, 1)[0] as f64;
                let mut finished = false;
                let mut outb: Vec<Block> = vec![SILENT];
                if let Some(Node { kind: Kind::BufferSource(bs), .. }) = self.nodes[id].as_mut()
                    && let Some((s, e)) = bs.sched.window(frame0, sr)
                {
                    match bs.buffer.clone() {
                        Some(buf) => {
                            let len = buf.ch.first().map_or(0, Vec::len);
                            outb = vec![SILENT; buf.ch.len().max(1)];
                            if s < e && !bs.began {
                                bs.began = true;
                                bs.pos = (bs.offset * buf.sr).clamp(0.0, len as f64);
                            }
                            let step = rate * 2f64.powf(detune / 1200.0) * buf.sr / sr;
                            let (mut ls, mut le) = (bs.loop_start * buf.sr, bs.loop_end * buf.sr);
                            if le <= 0.0 || le > len as f64 {
                                le = len as f64;
                            }
                            if ls < 0.0 || ls >= le {
                                ls = 0.0;
                            }
                            let limit = bs.duration.map(|d| d * buf.sr);
                            for i in s..e {
                                if bs.looping && le > ls {
                                    if step >= 0.0 && bs.pos >= le {
                                        bs.pos = ls + (bs.pos - le) % (le - ls);
                                    } else if step < 0.0 && bs.pos < ls {
                                        bs.pos = le - (ls - bs.pos) % (le - ls);
                                    }
                                } else if bs.pos >= len as f64 || bs.pos < 0.0 {
                                    bs.sched.ended = true;
                                    finished = true;
                                    break;
                                }
                                if limit.is_some_and(|l| bs.played >= l) {
                                    bs.sched.ended = true;
                                    finished = true;
                                    break;
                                }
                                let k = bs.pos.floor() as usize;
                                let f = (bs.pos - bs.pos.floor()) as f32;
                                // Past the last frame a non-looping buffer is silence.
                                let next = if k + 1 < len { Some(k + 1) } else if bs.looping { Some(ls as usize) } else { None };
                                for (c, ch) in buf.ch.iter().enumerate() {
                                    let a = ch.get(k).copied().unwrap_or(0.0);
                                    let b = next.and_then(|n| ch.get(n).copied()).unwrap_or(0.0);
                                    outb[c][i] = a + (b - a) * f;
                                }
                                bs.pos += step;
                                bs.played += step.abs();
                            }
                        }
                        None => {
                            if !bs.began && s < e {
                                bs.began = true;
                            }
                        }
                    }
                    if !finished && bs.sched.stopped_by(frame0 + Q as u64, sr) {
                        bs.sched.ended = true;
                        finished = true;
                    }
                }
                if finished {
                    ended.push(id);
                }
                self.set_out(id, 0, outb);
            }
            4 => {
                let off = self.param_block(id, 0);
                let mut block = SILENT;
                let mut finished = false;
                if let Some(Node { kind: Kind::Constant(sched), .. }) = self.nodes[id].as_mut()
                    && let Some((s, e)) = sched.window(frame0, sr)
                {
                    block[s..e].copy_from_slice(&off[s..e]);
                    if sched.stopped_by(frame0 + Q as u64, sr) {
                        sched.ended = true;
                        finished = true;
                    }
                }
                if finished {
                    ended.push(id);
                }
                self.set_out(id, 0, vec![block]);
            }
            5 => {
                let input = self.mixed_input(id, 0);
                let dt = self.param_block(id, 0);
                let mut outb = vec![SILENT; input.len()];
                if let Some(Node { kind: Kind::Delay(d), .. }) = self.nodes[id].as_mut() {
                    let size = (d.max * sr).ceil() as usize + 2 * Q + 2;
                    if d.buf.len() != input.len() {
                        d.buf.resize(input.len(), vec![0.0; size]);
                    }
                    for b in d.buf.iter_mut() {
                        if b.len() != size {
                            b.resize(size, 0.0);
                        }
                    }
                    if !d.cycle {
                        // Write this quantum first: delays under one quantum
                        // read the current input.
                        for (c, ch) in input.iter().enumerate() {
                            for i in 0..Q {
                                d.buf[c][(d.write + i) % size] = ch[i];
                            }
                        }
                    }
                    let min = if d.cycle { Q as f64 } else { 0.0 };
                    for i in 0..Q {
                        let delay = (dt[i] as f64 * sr).clamp(min, d.max * sr);
                        let pos = (d.write + i) as f64 - delay;
                        let pos = pos.rem_euclid(size as f64);
                        let k = pos.floor() as usize % size;
                        let f = (pos - pos.floor()) as f32;
                        for c in 0..input.len() {
                            let a = d.buf[c][k];
                            let b = d.buf[c][(k + 1) % size];
                            outb[c][i] = a + (b - a) * f;
                        }
                    }
                    if !d.cycle {
                        d.write = (d.write + Q) % size;
                    }
                }
                self.set_out(id, 0, outb);
            }
            6 => {
                let mut input = self.mixed_input(id, 0);
                let f = self.param_block(id, 0);
                let det = self.param_block(id, 1);
                let qv = self.param_block(id, 2);
                let gn = self.param_block(id, 3);
                let constant = [f, det, qv, gn].iter().all(|b| b.iter().all(|v| *v == b[0]));
                if let Some(Node { kind: Kind::Biquad(bq), .. }) = self.nodes[id].as_mut() {
                    if bq.state.len() != input.len() {
                        bq.state.resize(input.len(), [0.0; 4]);
                    }
                    let mut coefs = biquad_coefs(bq.typ, sr, f[0] as f64, qv[0] as f64, gn[0] as f64, det[0] as f64);
                    for (c, ch) in input.iter_mut().enumerate() {
                        let st = &mut bq.state[c];
                        for i in 0..Q {
                            if !constant {
                                coefs = biquad_coefs(bq.typ, sr, f[i] as f64, qv[i] as f64, gn[i] as f64, det[i] as f64);
                            }
                            let x = ch[i] as f64;
                            let [b0, b1, b2, a1, a2] = coefs;
                            let y = b0 * x + b1 * st[0] + b2 * st[1] - a1 * st[2] - a2 * st[3];
                            st[1] = st[0];
                            st[0] = x;
                            st[3] = st[2];
                            st[2] = if y.is_finite() { y } else { 0.0 };
                            ch[i] = st[2] as f32;
                        }
                    }
                }
                self.set_out(id, 0, input);
            }
            7 => {
                let mut input = self.mixed_input(id, 0);
                if let Some(Node { kind: Kind::Iir(f), .. }) = self.nodes[id].as_mut() {
                    let (nb, na) = (f.b.len(), f.a.len());
                    if f.hist.len() != input.len() {
                        f.hist.resize(input.len(), (vec![0.0; nb], vec![0.0; na]));
                    }
                    for (c, ch) in input.iter_mut().enumerate() {
                        let (xs, ys) = &mut f.hist[c];
                        for i in 0..Q {
                            xs.rotate_right(1);
                            xs[0] = ch[i] as f64;
                            let mut y = 0.0;
                            for k in 0..nb {
                                y += f.b[k] * xs[k];
                            }
                            for k in 1..na {
                                y -= f.a[k] * ys[k - 1];
                            }
                            let y = if y.is_finite() { y } else { 0.0 };
                            if na > 1 {
                                ys.rotate_right(1);
                                ys[0] = y;
                            }
                            ch[i] = y as f32;
                        }
                    }
                }
                self.set_out(id, 0, input);
            }
            8 => {
                let mut input = self.mixed_input(id, 0);
                if let Some(Node { kind: Kind::Shaper(s), .. }) = self.nodes[id].as_ref()
                    && let Some(curve) = &s.curve
                    && !curve.is_empty()
                {
                    let n = curve.len();
                    for ch in input.iter_mut() {
                        for v in ch.iter_mut() {
                            let x = (n - 1) as f32 * 0.5 * (*v + 1.0);
                            *v = if n == 1 {
                                curve[0]
                            } else if x <= 0.0 {
                                curve[0]
                            } else if x >= (n - 1) as f32 {
                                curve[n - 1]
                            } else {
                                let k = x.floor() as usize;
                                let f = x - k as f32;
                                curve[k] * (1.0 - f) + curve[k + 1] * f
                            };
                        }
                    }
                }
                self.set_out(id, 0, input);
            }
            9 => {
                let input = self.mixed_input(id, 0);
                let pan = self.param_block(id, 0);
                let mut l = SILENT;
                let mut r = SILENT;
                for i in 0..Q {
                    let p = pan[i].clamp(-1.0, 1.0) as f64;
                    if input.len() == 1 {
                        let x = (p + 1.0) / 2.0;
                        let (gl, gr) = ((x * PI / 2.0).cos() as f32, (x * PI / 2.0).sin() as f32);
                        l[i] = input[0][i] * gl;
                        r[i] = input[0][i] * gr;
                    } else {
                        let (il, ir) = (input[0][i], input[1][i]);
                        if p <= 0.0 {
                            let x = p + 1.0;
                            let (gl, gr) = ((x * PI / 2.0).cos() as f32, (x * PI / 2.0).sin() as f32);
                            l[i] = il + ir * gl;
                            r[i] = ir * gr;
                        } else {
                            let x = p;
                            let (gl, gr) = ((x * PI / 2.0).cos() as f32, (x * PI / 2.0).sin() as f32);
                            l[i] = il * gl;
                            r[i] = ir + il * gr;
                        }
                    }
                }
                self.set_out(id, 0, vec![l, r]);
            }
            10 => self.process_panner(id),
            11 => {
                let input = self.mixed_input(id, 0);
                let th = self.param_block(id, 0)[0] as f64;
                let knee = self.param_block(id, 1)[0] as f64;
                let ratio = self.param_block(id, 2)[0].max(1.0) as f64;
                let attack = self.param_block(id, 3)[0] as f64;
                let release = self.param_block(id, 4)[0] as f64;
                let mut outb = vec![SILENT; input.len()];
                if let Some(Node { kind: Kind::Compressor(c), .. }) = self.nodes[id].as_mut() {
                    // Static curve with a soft knee, in dB.
                    let curve = |x: f64| -> f64 {
                        if knee > 0.0 && 2.0 * (x - th).abs() <= knee {
                            x + (1.0 / ratio - 1.0) * (x - th + knee / 2.0).powi(2) / (2.0 * knee)
                        } else if 2.0 * (x - th) > knee {
                            th + (x - th) / ratio
                        } else {
                            x
                        }
                    };
                    // Make-up gain: 0.6 power of the inverse gain at 0 dBFS.
                    let makeup = -0.6 * curve(0.0);
                    let pre = (0.006 * sr) as usize;
                    if c.delay.len() != input.len() {
                        c.delay = vec![vec![0.0; pre + 1]; input.len()];
                        c.pos = 0;
                    }
                    let ca = if attack > 0.0 { (-1.0 / (attack * sr)).exp() } else { 0.0 };
                    let cr = if release > 0.0 { (-1.0 / (release * sr)).exp() } else { 0.0 };
                    for i in 0..Q {
                        let peak = input.iter().fold(0.0f64, |m, ch| m.max(ch[i].abs() as f64));
                        let x = if peak > 1e-9 { 20.0 * peak.log10() } else { -180.0 };
                        let target = curve(x) - x;
                        let coef = if target < c.gain_db { ca } else { cr };
                        c.gain_db = coef * c.gain_db + (1.0 - coef) * target;
                        let g = 10f64.powf((c.gain_db + makeup) / 20.0) as f32;
                        let len = pre + 1;
                        for (ch, (dl, o)) in input.iter().zip(c.delay.iter_mut().zip(outb.iter_mut())) {
                            let delayed = dl[c.pos];
                            dl[c.pos] = ch[i];
                            o[i] = delayed * g;
                        }
                        c.pos = (c.pos + 1) % len;
                    }
                    c.reduction = c.gain_db as f32;
                }
                self.set_out(id, 0, outb);
            }
            12 => {
                let input = self.mixed_input(id, 0);
                let mut mono = [SILENT];
                mix_into(&mut mono, &input, Interp::Speakers);
                if let Some(Node { kind: Kind::Analyser(a), .. }) = self.nodes[id].as_mut() {
                    let len = a.ring.len();
                    for i in 0..Q {
                        a.ring[a.pos] = mono[0][i];
                        a.pos = (a.pos + 1) % len;
                    }
                }
                self.set_out(id, 0, input);
            }
            13 => {
                let input = self.mixed_input(id, 0);
                let n_out = self.nodes[id].as_ref().map_or(0, |n| n.n_out);
                for o in 0..n_out {
                    self.set_out(id, o, vec![input.get(o).copied().unwrap_or(SILENT)]);
                }
            }
            14 => {
                let n_in = self.nodes[id].as_ref().map_or(0, |n| n.inputs.len());
                let mut chans = Vec::with_capacity(n_in);
                for k in 0..n_in {
                    let x = self.mixed_input(id, k);
                    chans.push(x[0]);
                }
                self.set_out(id, 0, chans);
            }
            15 => {
                let input = self.mixed_input(id, 0);
                let mut outb = vec![SILENT];
                if let Some(Node { kind: Kind::Convolver(cv), .. }) = self.nodes[id].as_mut()
                    && cv.ir_channels > 0
                {
                    cv.push_input(&input);
                    let stereo_out = cv.ir_channels >= 2 || input.len() >= 2;
                    outb = vec![SILENT; if stereo_out { 2 } else { 1 }];
                    match (cv.ir_channels, input.len()) {
                        (1, 1) => cv.convolve(0, 0, &mut outb[0]),
                        (1, _) => {
                            cv.convolve(0, 0, &mut outb[0]);
                            cv.convolve(1, 0, &mut outb[1]);
                        }
                        (4, n) if n >= 2 => {
                            let (a, b) = outb.split_at_mut(1);
                            cv.convolve(0, 0, &mut a[0]);
                            cv.convolve(1, 2, &mut a[0]);
                            cv.convolve(0, 1, &mut b[0]);
                            cv.convolve(1, 3, &mut b[0]);
                        }
                        (_, 1) => {
                            cv.convolve(0, 0, &mut outb[0]);
                            cv.convolve(0, 1, &mut outb[1]);
                        }
                        _ => {
                            cv.convolve(0, 0, &mut outb[0]);
                            cv.convolve(1, 1, &mut outb[1]);
                        }
                    }
                }
                self.set_out(id, 0, outb);
            }
            16 => {
                let n = match self.nodes[id].as_ref().map(|n| &n.kind) {
                    Some(Kind::Silent(c)) => (*c).max(1),
                    _ => 1,
                };
                let n_out = self.nodes[id].as_ref().map_or(0, |n| n.n_out);
                for o in 0..n_out {
                    self.set_out(id, o, vec![SILENT; n]);
                }
            }
            _ => {
                for p in 0..9 {
                    self.param_block(id, p);
                }
            }
        }
        let _ = t0;
    }

    fn delay_write_cycle(&mut self, id: usize) {
        let is_cycle = matches!(self.nodes.get(id).and_then(|n| n.as_ref()).map(|n| &n.kind), Some(Kind::Delay(d)) if d.cycle);
        if !is_cycle {
            return;
        }
        let input = self.mixed_input(id, 0);
        if let Some(Node { kind: Kind::Delay(d), .. }) = self.nodes[id].as_mut() {
            let size = d.buf.first().map_or(0, Vec::len);
            if size == 0 {
                return;
            }
            if d.buf.len() < input.len() {
                d.buf.resize(input.len(), vec![0.0; size]);
            }
            for (c, ch) in input.iter().enumerate() {
                for i in 0..Q {
                    d.buf[c][(d.write + i) % size] = ch[i];
                }
            }
            d.write = (d.write + Q) % size;
        }
    }

    fn process_panner(&mut self, id: usize) {
        let input = self.mixed_input(id, 0);
        let mut v = [0.0f64; 6];
        for (k, item) in v.iter_mut().enumerate() {
            *item = self.param_block(id, k)[0] as f64;
        }
        let li = self.listener;
        let mut l = [0.0f64; 9];
        for (k, item) in l.iter_mut().enumerate() {
            *item = self.param_block(li, k)[0] as f64;
        }
        let Some(Node { kind: Kind::Panner(p), .. }) = self.nodes[id].as_ref() else { return };
        let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        let norm = |a: [f64; 3]| {
            let n = dot(a, a).sqrt();
            if n > 0.0 { [a[0] / n, a[1] / n, a[2] / n] } else { a }
        };
        let src = [v[0], v[1], v[2]];
        let orient = [v[3], v[4], v[5]];
        let lpos = [l[0], l[1], l[2]];
        let fwd = norm([l[3], l[4], l[5]]);
        let up0 = [l[6], l[7], l[8]];
        // Azimuth (spec §"Azimuth and Elevation").
        let rel = sub(src, lpos);
        let mut azimuth = 0.0;
        if dot(rel, rel) > 0.0 {
            let sl = norm(rel);
            let right = norm(cross(fwd, up0));
            let up = norm(cross(right, fwd));
            let proj = dot(sl, up);
            let ps = norm(sub(sl, [up[0] * proj, up[1] * proj, up[2] * proj]));
            azimuth = dot(ps, right).clamp(-1.0, 1.0).acos().to_degrees();
            if dot(ps, fwd) < 0.0 {
                azimuth = 360.0 - azimuth;
            }
            azimuth = if (0.0..=270.0).contains(&azimuth) { 90.0 - azimuth } else { 450.0 - azimuth };
        }
        if azimuth < -90.0 {
            azimuth = -180.0 - azimuth;
        } else if azimuth > 90.0 {
            azimuth = 180.0 - azimuth;
        }
        // Distance and cone gains.
        let d = dot(rel, rel).sqrt();
        let (rd, md, rf) = (p.ref_distance, p.max_distance, p.rolloff);
        let dg = match p.distance_model {
            0 => {
                let rf = rf.min(1.0);
                if md <= rd { 1.0 } else { 1.0 - rf * (d.clamp(rd, md) - rd) / (md - rd) }
            }
            2 => {
                if rd <= 0.0 { 0.0 } else { (d.max(rd) / rd).powf(-rf) }
            }
            _ => {
                let den = rd + rf * (d.max(rd) - rd);
                if den <= 0.0 { 1.0 } else { rd / den }
            }
        };
        let cg = if dot(orient, orient) == 0.0 || (p.cone_inner == 360.0 && p.cone_outer == 360.0) {
            1.0
        } else {
            let to_listener = norm(sub(lpos, src));
            let angle = dot(to_listener, norm(orient)).clamp(-1.0, 1.0).acos().to_degrees().abs();
            let (hi, ho) = (p.cone_inner / 2.0, p.cone_outer / 2.0);
            if angle <= hi {
                1.0
            } else if angle >= ho {
                p.cone_outer_gain
            } else {
                1.0 + (p.cone_outer_gain - 1.0) * (angle - hi) / (ho - hi)
            }
        };
        let gain = (dg * cg) as f32;
        let mut lo = SILENT;
        let mut ro = SILENT;
        if input.len() == 1 {
            let x = (azimuth + 90.0) / 180.0;
            let (gl, gr) = ((x * PI / 2.0).cos() as f32, (x * PI / 2.0).sin() as f32);
            for i in 0..Q {
                lo[i] = input[0][i] * gl * gain;
                ro[i] = input[0][i] * gr * gain;
            }
        } else {
            let (x, left_side) = if azimuth <= 0.0 { ((azimuth + 90.0) / 90.0, true) } else { (azimuth / 90.0, false) };
            let (gl, gr) = ((x * PI / 2.0).cos() as f32, (x * PI / 2.0).sin() as f32);
            for i in 0..Q {
                let (il, ir) = (input[0][i], input[1][i]);
                if left_side {
                    lo[i] = (il + ir * gl) * gain;
                    ro[i] = ir * gr * gain;
                } else {
                    lo[i] = il * gl * gain;
                    ro[i] = (ir + il * gr) * gain;
                }
            }
        }
        self.set_out(id, 0, vec![lo, ro]);
    }

    /// Is a scheduled source still playing (or waiting to start)?
    pub(crate) fn active_sources(&self) -> bool {
        self.nodes.iter().flatten().any(|n| match &n.kind {
            Kind::Oscillator(o) => o.sched.start.is_some() && !o.sched.ended,
            Kind::BufferSource(b) => b.sched.start.is_some() && !b.sched.ended,
            Kind::Constant(s) => s.start.is_some() && !s.ended,
            _ => false,
        })
    }
}

/// Resamples `ch` from `from` Hz to `to` Hz (cubic interpolation).
pub(crate) fn resample(ch: &[f32], from: f64, to: f64) -> Vec<f32> {
    if (from - to).abs() < 1e-9 || ch.is_empty() {
        return ch.to_vec();
    }
    let n = ((ch.len() as f64) * to / from).round().max(1.0) as usize;
    let at = |i: isize| -> f64 { ch[i.clamp(0, ch.len() as isize - 1) as usize] as f64 };
    (0..n)
        .map(|j| {
            let x = j as f64 * from / to;
            let i = x.floor() as isize;
            let t = x - i as f64;
            let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
            // Catmull-Rom.
            let v = p1 + 0.5 * t * (p2 - p0 + t * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3 + t * (3.0 * (p1 - p2) + p3 - p0)));
            v as f32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automation_timeline() {
        let mut p = Param::new(1.0, -10.0, 10.0, true);
        p.add(Ev::Set(0.0, 0.0));
        p.add(Ev::Linear(1.0, 1.0));
        p.add(Ev::Exp(2.0, 4.0));
        p.add(Ev::Target(3.0, 0.0, 0.5));
        assert!((p.eval(4, 0.5) - 0.5).abs() < 1e-9);
        assert!((p.eval(4, 1.5) - 2.0).abs() < 1e-9);
        assert!((p.eval(4, 2.5) - 4.0).abs() < 1e-9);
        assert!((p.eval(4, 3.5) - 4.0 * (-1.0f64).exp()).abs() < 1e-9);
        p.cancel_hold(3.5);
        assert!((p.eval(p.events.len(), 10.0) - 4.0 * (-1.0f64).exp()).abs() < 1e-6);
    }

    #[test]
    fn fft_round_trip() {
        let mut a: Vec<C> = (0..16).map(|i| C { re: i as f64, im: 0.0 }).collect();
        fft(&mut a, false);
        fft(&mut a, true);
        assert!(a.iter().enumerate().all(|(i, c)| (c.re - i as f64).abs() < 1e-9));
    }

    #[test]
    fn biquad_lowpass_passes_dc() {
        let c = biquad_coefs(BiquadType::Lowpass, 48000.0, 1000.0, 1.0, 0.0, 0.0);
        let r = frequency_response(&[c[0], c[1], c[2]], &[1.0, c[3], c[4]], 48000.0, &[0.0, 20000.0]);
        assert!((r[0].0 - 1.0).abs() < 1e-6 && r[1].0 < 0.01, "{r:?}");
    }
}
