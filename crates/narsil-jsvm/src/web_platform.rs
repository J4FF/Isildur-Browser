//! Lazily loaded web platform APIs.
//!
//! WebAssembly (and the APIs that follow it: `crypto.subtle`, compression
//! streams, WebSocket, EventSource, Media Source Extensions, Web Audio) are
//! built from two halves:
//! Rust natives for the heavy work and a JavaScript glue file for the WebIDL
//! surface (argument conversion, property descriptors, promises).
//!
//! The glue is NOT part of the page preamble. MEASURED (2026-10-15, wpt
//! profile): the preamble (`vorspann.js`, 675 KB) costs 260 ms per page to
//! parse and run, about 0.4 ms per KB. A glue file runs only on the first
//! use of one of its globals: the global lookup miss, a property read on the
//! global object, `in`, `hasOwnProperty`/`Object.hasOwn`,
//! `getOwnPropertyDescriptor` and `Reflect.has` ask
//! [`Engine::platform_lazy_global`] before they report "not there". A page
//! that never touches the APIs pays one comparison on those miss paths.

use crate::heap::{GVal, HeapRef};
use crate::interp::{Engine, Stoerung};

/// One lazily loaded API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Api {
    Wasm,
    Crypto,
    Compression,
    Net,
    Media,
    Audio,
    /// Release 15.10.2026: `Atomics` and `Float16Array` (es_modern.rs).
    Es,
}

impl Api {
    fn bit(self) -> u32 {
        match self {
            Api::Wasm => 1,
            Api::Crypto => 2,
            Api::Compression => 4,
            Api::Net => 8,
            Api::Media => 16,
            Api::Audio => 32,
            Api::Es => 64,
        }
    }
}

/// All APIs (the bit set of a fully loaded engine).
const ALL: u32 = 127;

/// The globals that appear on first use, and the API that defines them.
const LAZY_GLOBALS: &[(&str, Api)] = &[
    ("WebAssembly", Api::Wasm),
    ("SubtleCrypto", Api::Crypto),
    ("CryptoKey", Api::Crypto),
    ("CompressionStream", Api::Compression),
    ("DecompressionStream", Api::Compression),
    ("WebSocket", Api::Net),
    ("EventSource", Api::Net),
    ("CloseEvent", Api::Net),
    ("MediaSource", Api::Media),
    ("SourceBuffer", Api::Media),
    ("SourceBufferList", Api::Media),
    ("AudioContext", Api::Audio),
    ("OfflineAudioContext", Api::Audio),
    ("BaseAudioContext", Api::Audio),
    ("AudioNode", Api::Audio),
    ("AudioParam", Api::Audio),
    ("AudioBuffer", Api::Audio),
    ("PeriodicWave", Api::Audio),
    ("AudioDestinationNode", Api::Audio),
    ("AudioListener", Api::Audio),
    ("AudioScheduledSourceNode", Api::Audio),
    ("OscillatorNode", Api::Audio),
    ("AudioBufferSourceNode", Api::Audio),
    ("ConstantSourceNode", Api::Audio),
    ("GainNode", Api::Audio),
    ("DelayNode", Api::Audio),
    ("BiquadFilterNode", Api::Audio),
    ("IIRFilterNode", Api::Audio),
    ("WaveShaperNode", Api::Audio),
    ("StereoPannerNode", Api::Audio),
    ("PannerNode", Api::Audio),
    ("DynamicsCompressorNode", Api::Audio),
    ("AnalyserNode", Api::Audio),
    ("ChannelSplitterNode", Api::Audio),
    ("ChannelMergerNode", Api::Audio),
    ("ConvolverNode", Api::Audio),
    ("ScriptProcessorNode", Api::Audio),
    ("MediaElementAudioSourceNode", Api::Audio),
    ("MediaStreamAudioSourceNode", Api::Audio),
    ("MediaStreamAudioDestinationNode", Api::Audio),
    ("AudioWorklet", Api::Audio),
    ("AudioWorkletNode", Api::Audio),
    ("OfflineAudioCompletionEvent", Api::Audio),
    ("AudioProcessingEvent", Api::Audio),
    ("Atomics", Api::Es),
    ("Float16Array", Api::Es),
    ("WeakRef", Api::Es),
    ("FinalizationRegistry", Api::Es),
    ("Intl", Api::Es),
];

/// Natives of the platform glue (one `Nativ` variant for all of them keeps
/// the engine's dispatch to a single line).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlatformOp {
    Wasm(crate::web_wasm::WasmOp),
    Crypto(crate::web_crypto::CryptoOp),
    Compression(crate::web_compression::CompressionOp),
    Net(crate::web_net::NetOp),
    Media(crate::web_media::MediaOp),
    Audio(crate::web_audio::AudioOp),
}

/// Platform state of one engine.
#[derive(Default)]
pub(crate) struct Platform {
    /// Bit set of the APIs whose glue has run.
    loaded: u32,
    /// WebAssembly store and bridge (created with the glue).
    pub(crate) wasm: Option<Box<crate::web_wasm::WasmState>>,
    /// The `SubtleCrypto` instance behind `crypto.subtle`.
    pub(crate) crypto_subtle: Option<GVal>,
    /// WebSocket/EventSource connections.
    pub(crate) net: crate::web_net::NetState,
    /// Media Source Extensions: the SourceBuffers' parsers and track
    /// buffers.
    pub(crate) media: Option<Box<crate::web_media::MediaState>>,
    /// Web Audio: the contexts' audio graphs.
    pub(crate) audio: Option<Box<crate::web_audio::AudioState>>,
}

impl Platform {
    /// Every value the platform keeps alive (GC roots).
    pub(crate) fn roots(&self, out: &mut Vec<GVal>) {
        if let Some(w) = &self.wasm {
            w.roots(out);
        }
        out.extend(self.crypto_subtle);
    }
}

impl Engine {
    /// `true` while some API still waits for its first use. The miss paths
    /// test this first, so a fully loaded (or unused) engine pays one load
    /// and compare.
    #[inline]
    pub(crate) fn platform_pending(&self) -> bool {
        self.platform.loaded != ALL
    }

    /// The lookup of `name` on the global object missed: if `name` is a
    /// lazy global whose API has not run yet, run its glue now. `Ok(true)`
    /// means the global exists now (look it up again).
    pub(crate) fn platform_lazy_global(&mut self, name: &str) -> Result<bool, Stoerung> {
        if !self.platform_pending() {
            return Ok(false);
        }
        let Some(api) = LAZY_GLOBALS
            .iter()
            .find(|(n, a)| *n == name && self.platform.loaded & a.bit() == 0)
            .map(|(_, a)| *a)
        else {
            return Ok(false);
        };
        // A page may have defined the name itself (a polyfill): it wins.
        if self.eigene_lese(self.global_objekt, name).is_some() {
            return Ok(false);
        }
        self.platform_load(api)?;
        Ok(self.eigene_lese(self.global_objekt, name).is_some())
    }

    /// Like [`Engine::platform_lazy_global`] for an arbitrary object: only
    /// the global object has lazy properties. Used by `in`,
    /// `hasOwnProperty` & co.
    pub(crate) fn platform_lazy_property(&mut self, obj: GVal, name: GVal) -> Result<(), Stoerung> {
        if !self.platform_pending() {
            return Ok(());
        }
        if obj != GVal::Ref(self.global_objekt) {
            return Ok(());
        }
        let GVal::Ref(r) = name else { return Ok(()) };
        let text = match self.heap.holen(r) {
            crate::heap::HObj::Zeichenkette(s) => s.clone(),
            _ => return Ok(()),
        };
        self.platform_lazy_global(&text).map(|_| ())
    }

    /// Run the glue of `api` unless it ran already.
    pub(crate) fn platform_lazy_api(&mut self, api: Api) -> Result<(), Stoerung> {
        if self.platform.loaded & api.bit() == 0 {
            self.platform_load(api)?;
        }
        Ok(())
    }

    /// Run the glue of `api` (once).
    fn platform_load(&mut self, api: Api) -> Result<(), Stoerung> {
        // Mark first: the glue itself reads the globals it defines.
        self.platform.loaded |= api.bit();
        match api {
            Api::Wasm => self.wasm_glue_laden(),
            Api::Crypto => self.crypto_glue_laden(),
            Api::Compression => self.compression_glue_laden(),
            Api::Net => self.net_glue_laden(),
            Api::Media => self.media_glue_laden(),
            Api::Audio => self.audio_glue_laden(),
            Api::Es => self.es_lazy_laden(),
        }
    }

    /// Evaluate a glue file `(function (N, g) { … })` globally and call it
    /// with its natives object `N` and the global object `g`.
    pub(crate) fn platform_run_glue(&mut self, source: &str, natives: HeapRef) -> Result<(), Stoerung> {
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(natives));
        let vorher = self.eval_global;
        self.eval_global = true;
        let f = self.quelltext_ausfuehren(source);
        self.eval_global = vorher;
        let r = match f {
            Ok(f) => {
                self.wurzeln.push(f);
                let g = GVal::Ref(self.global_objekt);
                self.aufruf_synchron(f, GVal::Undefiniert, &[GVal::Ref(natives), g], false)
                    .map(|_| ())
            }
            Err(e) => Err(e),
        };
        self.wurzel_zurueck(marke);
        r
    }

    /// A natives object for a glue file: `{ name: native, … }`.
    pub(crate) fn platform_natives(
        &mut self,
        list: &[(&str, PlatformOp, u32)],
    ) -> Result<HeapRef, Stoerung> {
        let obj = self.neues_objekt(Some(self.proto_objekt)).map_err(Stoerung::Fehler)?;
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(obj));
        for (name, op, len) in list {
            let f = self.nativ_anlegen(name, crate::builtin::Nativ::Plattform(*op), self.proto_funktion);
            self.funktionsnamen_setzen(f, name, *len);
            self.setze_eigen(obj, name, GVal::Ref(f));
        }
        self.wurzel_zurueck(marke);
        Ok(obj)
    }

    /// Is `r` the `ArrayBuffer` of a `WebAssembly.Memory`?
    pub(crate) fn platform_wasm_buffer(&self, r: HeapRef) -> bool {
        self.platform.wasm.as_ref().is_some_and(|w| w.owns_buffer(r))
    }

    /// Dispatch of all platform natives.
    pub(crate) fn platform_native(
        &mut self,
        op: PlatformOp,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        match op {
            PlatformOp::Wasm(w) => self.wasm_native(w, this, args),
            PlatformOp::Crypto(c) => self.crypto_native(c, this, args),
            PlatformOp::Compression(c) => self.compression_native(c, args),
            PlatformOp::Net(n) => self.net_native(n, args),
            PlatformOp::Media(m) => self.media_native(m, args),
            PlatformOp::Audio(a) => self.audio_native(a, args),
        }
    }
}
