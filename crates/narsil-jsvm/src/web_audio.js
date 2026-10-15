// Web Audio API (https://webaudio.github.io/web-audio-api/): contexts, nodes,
// AudioParam automation, AudioBuffer, PeriodicWave. Loaded lazily on the
// first use of an audio global. Every graph change is mirrored into the
// context's Rust graph (web_audio.rs / web_audio_render.rs), which renders
// OfflineAudioContexts and, for a running AudioContext, catches up while
// sources play (for `ended` events and AnalyserNode data). There is no
// audio device output yet.
(function (N, g) {
  'use strict';
  var defineProperty = Object.defineProperty;
  var TypeErrorCtor = TypeError;
  var RangeErrorCtor = RangeError;
  var PromiseCtor = Promise;
  var ET = g.EventTarget;
  var F32 = Float32Array;
  var KEY = {};
  var FLT_MAX = 3.4028234663852886e38;
  var DETUNE_MAX = 153600;
  var DEST = 0, LISTENER = 1;

  // ---- Helpers -------------------------------------------------------
  function domError(name, msg) {
    if (typeof g.DOMException === 'function') return new g.DOMException(msg, name);
    var e = new Error(msg);
    e.name = name;
    return e;
  }
  function report(e) { try { if (typeof g.reportError === 'function') g.reportError(e); } catch (x) {} }
  function event(type) {
    try { return typeof g.__narsilUA === 'function' ? g.__narsilUA(g.Event, type, {}) : new g.Event(type); } catch (e) { return new g.Event(type); }
  }
  function fire(target, ev) { try { target.dispatchEvent(ev); } catch (e) { report(e); } }
  function task(f) { g.setTimeout(f, 0); }
  function now() { return g.performance && typeof g.performance.now === 'function' ? g.performance.now() : Date.now(); }
  function fail(iface, member, name, msg) { return domError(name, "Failed to execute '" + member + "' on '" + iface + "': " + msg); }
  function failSet(iface, member, name, msg) { return domError(name, "Failed to set the '" + member + "' property on '" + iface + "': " + msg); }
  function failCtor(iface, name, msg) { return domError(name, "Failed to construct '" + iface + "': " + msg); }
  function typeErr(msg) { return new TypeErrorCtor(msg); }
  function rangeErr(msg) { return new RangeErrorCtor(msg); }
  function rejected(e) { return PromiseCtor.reject(e); }
  function finite(v, what) {
    var n = Number(v);
    if (!isFinite(n)) throw typeErr(what + ': The provided value is non-finite.');
    return n;
  }
  function unsigned(v) {
    var n = Number(v);
    if (!isFinite(n)) return 0;
    n = n < 0 ? Math.ceil(n) : Math.floor(n);
    n = n % 4294967296;
    return n < 0 ? n + 4294967296 : n;
  }
  function f32array(v, what) {
    if (v instanceof F32) return v;
    if (v !== null && typeof v === 'object' && typeof v[Symbol.iterator] === 'function') {
      var a = Array.from(v, Number);
      for (var i = 0; i < a.length; i++) if (!isFinite(a[i])) throw typeErr(what + ': The provided float value is non-finite.');
      return new F32(a);
    }
    throw typeErr(what + ': The provided value cannot be converted to a sequence.');
  }
  function handlers(proto, names, map) {
    names.forEach(function (n) {
      var key = 'on' + n;
      var get = function () { var s = map.get(this); return s && s.handlers[key] || null; };
      var set = function (fn) {
        var s = map.get(this);
        if (!s) throw typeErr('Illegal invocation');
        if (!s.wrapped[key]) {
          s.wrapped[key] = true;
          var self = this;
          this.addEventListener(n, function (e) {
            var h = s.handlers[key];
            if (typeof h === 'function') return h.call(self, e);
            if (h && typeof h.handleEvent === 'function') return h.handleEvent(e);
          });
        }
        s.handlers[key] = (typeof fn === 'function' || (typeof fn === 'object' && fn !== null)) ? fn : null;
      };
      defineProperty(get, 'name', { value: 'get ' + key });
      defineProperty(set, 'name', { value: 'set ' + key });
      defineProperty(proto, key, { get: get, set: set, enumerable: true, configurable: true });
    });
  }
  function brand(map, o) {
    var s = map.get(o);
    if (!s) throw typeErr('Illegal invocation');
    return s;
  }
  function getter(proto, name, f) { defineProperty(proto, name, { get: f, enumerable: true, configurable: true }); }
  function accessor(proto, name, get, set) { defineProperty(proto, name, { get: get, set: set, enumerable: true, configurable: true }); }

  var CTX = new WeakMap();
  var LISTENER_STATE = new WeakMap();
  var WORKLET_STATE = new WeakMap();
  var NODE = new WeakMap();
  var PARAM = new WeakMap();
  var BUF = new WeakMap();
  var WAVE = new WeakMap();
  var MEDIA_SOURCES = new WeakSet();

  // ---- Event classes -------------------------------------------------
  function uaEvent(type, proto, fields) {
    var e = event(type);
    Object.setPrototypeOf(e, proto);
    Object.keys(fields).forEach(function (k) { defineProperty(e, k, { value: fields[k], enumerable: true, configurable: true }); });
    return e;
  }
  function eventClass(name, required, fields) {
    var C = function (type, init) {
      if (new.target === undefined) throw typeErr("Failed to construct '" + name + "': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
      if (arguments.length < (required ? 2 : 1)) throw typeErr("Failed to construct '" + name + "': " + (required ? 2 : 1) + ' arguments required, but only ' + arguments.length + ' present.');
      var e = new g.Event(type, init);
      init = init || {};
      fields.forEach(function (f) {
        if (f[1] && init[f[0]] === undefined) throw typeErr("Failed to construct '" + name + "': Failed to read the '" + f[0] + "' property from '" + name + "Init': Required member is undefined.");
        defineProperty(e, f[0], { value: init[f[0]] === undefined ? f[2] : init[f[0]], enumerable: true, configurable: true });
      });
      Object.setPrototypeOf(e, C.prototype);
      return e;
    };
    C.prototype = Object.create(g.Event.prototype);
    defineProperty(C.prototype, 'constructor', { value: C, writable: true, configurable: true });
    defineProperty(C, 'name', { value: name, configurable: true });
    return C;
  }
  var OfflineAudioCompletionEvent = eventClass('OfflineAudioCompletionEvent', true, [['renderedBuffer', true, null]]);
  var AudioProcessingEvent = eventClass('AudioProcessingEvent', true, [['playbackTime', true, 0], ['inputBuffer', true, null], ['outputBuffer', true, null]]);

  // ---- AudioBuffer ---------------------------------------------------
  function makeBuffer(b, n, len, sr, iface, member) {
    var err = function (msg) { return member ? fail(iface, member, 'NotSupportedError', msg) : failCtor(iface, 'NotSupportedError', msg); };
    if (n < 1 || n > 32) throw err('The number of channels provided (' + n + ') is outside the range [1, 32].');
    if (len < 1) throw err('The number of frames provided (' + len + ') is less than or equal to the minimum bound (0).');
    if (!(sr >= 3000 && sr <= 768000)) throw err('The sample rate provided (' + sr + ') is outside the range [3000, 768000].');
    var ch = [];
    for (var i = 0; i < n; i++) ch.push(new F32(len));
    BUF.set(b, { sr: sr, len: len, ch: ch });
    return b;
  }
  class AudioBuffer {
    constructor(options) {
      if (arguments.length < 1 || options === null || typeof options !== 'object') throw typeErr("Failed to construct 'AudioBuffer': 1 argument required, but only 0 present.");
      if (options.length === undefined) throw typeErr("Failed to construct 'AudioBuffer': Failed to read the 'length' property from 'AudioBufferOptions': Required member is undefined.");
      if (options.sampleRate === undefined) throw typeErr("Failed to construct 'AudioBuffer': Failed to read the 'sampleRate' property from 'AudioBufferOptions': Required member is undefined.");
      var n = options.numberOfChannels === undefined ? 1 : unsigned(options.numberOfChannels);
      makeBuffer(this, n, unsigned(options.length), finite(options.sampleRate, "Failed to construct 'AudioBuffer'"), 'AudioBuffer');
    }
    get sampleRate() { return brand(BUF, this).sr; }
    get length() { return brand(BUF, this).len; }
    get duration() { var b = brand(BUF, this); return b.len / b.sr; }
    get numberOfChannels() { return brand(BUF, this).ch.length; }
    getChannelData(channel) {
      var b = brand(BUF, this);
      if (arguments.length < 1) throw typeErr("Failed to execute 'getChannelData' on 'AudioBuffer': 1 argument required, but only 0 present.");
      var c = unsigned(channel);
      if (c >= b.ch.length) throw fail('AudioBuffer', 'getChannelData', 'IndexSizeError', 'channel index (' + c + ') exceeds number of channels (' + b.ch.length + ')');
      return b.ch[c];
    }
    copyFromChannel(destination, channelNumber, bufferOffset) {
      var b = brand(BUF, this);
      if (!(destination instanceof F32)) throw typeErr("Failed to execute 'copyFromChannel' on 'AudioBuffer': parameter 1 is not of type 'Float32Array'.");
      var c = unsigned(channelNumber), off = unsigned(bufferOffset);
      if (c >= b.ch.length) throw fail('AudioBuffer', 'copyFromChannel', 'IndexSizeError', 'The channelNumber provided (' + c + ') is outside the range [0, ' + b.ch.length + ').');
      if (off >= b.len) return;
      var n = Math.min(b.len - off, destination.length);
      destination.set(b.ch[c].subarray(off, off + n));
    }
    copyToChannel(source, channelNumber, bufferOffset) {
      var b = brand(BUF, this);
      if (!(source instanceof F32)) throw typeErr("Failed to execute 'copyToChannel' on 'AudioBuffer': parameter 1 is not of type 'Float32Array'.");
      var c = unsigned(channelNumber), off = unsigned(bufferOffset);
      if (c >= b.ch.length) throw fail('AudioBuffer', 'copyToChannel', 'IndexSizeError', 'The channelNumber provided (' + c + ') is outside the range [0, ' + b.ch.length + ').');
      if (off >= b.len) return;
      var n = Math.min(b.len - off, source.length);
      b.ch[c].set(source.subarray(0, n), off);
    }
  }
  // [sampleRate, channel data...] for the natives (they copy).
  function bufData(buffer) {
    if (!buffer) return null;
    var b = BUF.get(buffer);
    return [b.sr].concat(b.ch);
  }
  function bufferFrom(sr, list) {
    var b = Object.create(AudioBuffer.prototype);
    var ch = list.map(function (ab) { return new F32(ab); });
    BUF.set(b, { sr: sr, len: ch.length ? ch[0].length : 0, ch: ch });
    return b;
  }

  // ---- PeriodicWave --------------------------------------------------
  function makeWave(w, real, imag, disable, iface, member) {
    var err = function (msg) { return member ? fail(iface, member, 'IndexSizeError', msg) : failCtor(iface, 'IndexSizeError', msg); };
    if (real === undefined && imag === undefined) { real = new F32([0, 0]); imag = new F32([0, 1]); }
    else if (real === undefined) { imag = f32array(imag, 'imag'); real = new F32(imag.length); }
    else if (imag === undefined) { real = f32array(real, 'real'); imag = new F32(real.length); }
    else {
      real = f32array(real, 'real');
      imag = f32array(imag, 'imag');
      if (real.length !== imag.length) throw err('length of real array (' + real.length + ') and length of imaginary array (' + imag.length + ') must match.');
    }
    if (real.length < 2) throw err('length of the real part array (' + real.length + ') must be at least 2.');
    WAVE.set(w, { real: new F32(real), imag: new F32(imag), disable: !!disable });
    return w;
  }
  class PeriodicWave {
    constructor(context, options) {
      if (!CTX.has(context)) throw typeErr("Failed to construct 'PeriodicWave': parameter 1 is not of type 'BaseAudioContext'.");
      var o = options || {};
      makeWave(this, o.real, o.imag, o.disableNormalization, 'PeriodicWave');
    }
  }

  // ---- AudioParam ----------------------------------------------------
  function ctxTime(cs) { return frames(cs) / cs.sr; }
  class AudioParam {
    constructor(key) { if (key !== KEY) throw typeErr('Illegal constructor'); }
    get value() {
      var p = brand(PARAM, this);
      if (p.auto && !p.cs.dead && p.cs.started) return N.paramLast(p.cs.id, p.node, p.index);
      return p.value;
    }
    set value(v) {
      var p = brand(PARAM, this), n = finite(v, "Failed to set the 'value' property on 'AudioParam'");
      p.value = Math.fround(n);
      if (p.cs.dead) return;
      if (p.auto) N.paramEvent(p.cs.id, p.node, p.index, 0, ctxTime(p.cs), n, 0, null);
      else N.paramValue(p.cs.id, p.node, p.index, n);
    }
    get defaultValue() { return brand(PARAM, this).def; }
    get minValue() { return brand(PARAM, this).min; }
    get maxValue() { return brand(PARAM, this).max; }
    get automationRate() { return brand(PARAM, this).rate; }
    set automationRate(v) {
      var p = brand(PARAM, this), r = String(v);
      if (r !== 'a-rate' && r !== 'k-rate') return;
      if (p.fixed && r !== p.rate) throw failSet('AudioParam', 'automationRate', 'InvalidStateError', 'automationRate cannot be changed from "' + p.rate + '" to "' + r + '"');
      p.rate = r;
    }
    setValueAtTime(value, startTime) {
      var p = brand(PARAM, this);
      var v = finite(value, "Failed to execute 'setValueAtTime' on 'AudioParam'"), t = finite(startTime, "Failed to execute 'setValueAtTime' on 'AudioParam'");
      if (t < 0) throw rangeErr("Failed to execute 'setValueAtTime' on 'AudioParam': Time must be a finite non-negative number: " + t);
      schedule(p, 0, t, v, 0, null, 'setValueAtTime');
      return this;
    }
    linearRampToValueAtTime(value, endTime) {
      var p = brand(PARAM, this);
      var v = finite(value, "Failed to execute 'linearRampToValueAtTime' on 'AudioParam'"), t = finite(endTime, "Failed to execute 'linearRampToValueAtTime' on 'AudioParam'");
      if (t < 0) throw rangeErr("Failed to execute 'linearRampToValueAtTime' on 'AudioParam': Time must be a finite non-negative number: " + t);
      startPoint(p);
      schedule(p, 1, t, v, 0, null, 'linearRampToValueAtTime');
      return this;
    }
    exponentialRampToValueAtTime(value, endTime) {
      var p = brand(PARAM, this);
      var v = finite(value, "Failed to execute 'exponentialRampToValueAtTime' on 'AudioParam'"), t = finite(endTime, "Failed to execute 'exponentialRampToValueAtTime' on 'AudioParam'");
      if (Math.fround(v) === 0) throw rangeErr("Failed to execute 'exponentialRampToValueAtTime' on 'AudioParam': The float target value provided (" + v + ') should not be in the range (' + -1.40130e-45 + ', ' + 1.40130e-45 + ').');
      if (t < 0) throw rangeErr("Failed to execute 'exponentialRampToValueAtTime' on 'AudioParam': Time must be a finite non-negative number: " + t);
      startPoint(p);
      schedule(p, 2, t, v, 0, null, 'exponentialRampToValueAtTime');
      return this;
    }
    setTargetAtTime(target, startTime, timeConstant) {
      var p = brand(PARAM, this), w = "Failed to execute 'setTargetAtTime' on 'AudioParam'";
      var v = finite(target, w), t = finite(startTime, w), tau = finite(timeConstant, w);
      if (t < 0) throw rangeErr(w + ': Time must be a finite non-negative number: ' + t);
      if (tau < 0) throw rangeErr(w + ': Time constant must be a finite non-negative number: ' + tau);
      schedule(p, 3, t, v, tau, null, 'setTargetAtTime');
      return this;
    }
    setValueCurveAtTime(values, startTime, duration) {
      var p = brand(PARAM, this), w = "Failed to execute 'setValueCurveAtTime' on 'AudioParam'";
      var curve = f32array(values, w), t = finite(startTime, w), d = finite(duration, w);
      if (t < 0) throw rangeErr(w + ': Time must be a finite non-negative number: ' + t);
      if (!(d > 0)) throw rangeErr(w + ': The duration provided (' + d + ') is less than or equal to the minimum bound (0).');
      if (curve.length < 2) throw domError('InvalidStateError', w + ': Curve length must be at least 2: ' + curve.length);
      schedule(p, 4, t, curve[curve.length - 1], d, curve, 'setValueCurveAtTime');
      return this;
    }
    cancelScheduledValues(cancelTime) {
      var p = brand(PARAM, this), w = "Failed to execute 'cancelScheduledValues' on 'AudioParam'";
      var t = finite(cancelTime, w);
      if (t < 0) throw rangeErr(w + ': Time must be a finite non-negative number: ' + t);
      if (!p.cs.dead) N.paramCancel(p.cs.id, p.node, p.index, t, false);
      return this;
    }
    cancelAndHoldAtTime(cancelTime) {
      var p = brand(PARAM, this), w = "Failed to execute 'cancelAndHoldAtTime' on 'AudioParam'";
      var t = finite(cancelTime, w);
      if (t < 0) throw rangeErr(w + ': Time must be a finite non-negative number: ' + t);
      if (!p.cs.dead) N.paramCancel(p.cs.id, p.node, p.index, t, true);
      return this;
    }
  }
  // A ramp without an earlier event starts at the current time and value.
  function startPoint(p) {
    if (!p.auto && !p.cs.dead) {
      p.auto = true;
      N.paramEvent(p.cs.id, p.node, p.index, 0, ctxTime(p.cs), p.value, 0, null);
    }
  }
  function schedule(p, kind, t, v, extra, curve, member) {
    p.auto = true;
    if (p.cs.dead) return;
    if (!N.paramEvent(p.cs.id, p.node, p.index, kind, t, v, extra, curve)) {
      throw fail('AudioParam', member, 'NotSupportedError', 'The event overlaps a setValueCurveAtTime automation.');
    }
  }
  function makeParam(cs, nodeId, index, spec) {
    var a = new AudioParam(KEY);
    PARAM.set(a, {
      cs: cs, node: nodeId, index: index, def: Math.fround(spec[1]), min: Math.fround(spec[2]), max: Math.fround(spec[3]),
      rate: spec[4] ? 'a-rate' : 'k-rate', fixed: !!spec[5], value: Math.fround(spec[1]), auto: false
    });
    return a;
  }

  // ---- Contexts ------------------------------------------------------
  // The current render frame (a multiple of the 128-frame quantum).
  function frames(cs) {
    if (cs.offline) return cs.rendered;
    var t = cs.acc + (cs.since !== null ? (now() - cs.since) / 1000 : 0);
    return Math.floor(t * cs.sr / 128) * 128;
  }
  function setState(ctx, cs, st) {
    if (cs.state === st) return;
    cs.state = st;
    task(function () { fire(ctx, event('statechange')); });
  }
  function endedSources(cs, ids) {
    ids.forEach(function (id) {
      var n = cs.playing[id];
      if (!n) return;
      delete cs.playing[id];
      task(function () { fire(n, event('ended')); });
    });
  }
  // A running AudioContext renders up to its clock while sources play.
  function catchUp(cs) {
    if (cs.dead || cs.offline) return;
    endedSources(cs, N.advance(cs.id, frames(cs)));
  }
  function pump(ctx, cs) {
    if (cs.offline || cs.timer !== null || cs.state !== 'running' || cs.dead) return;
    cs.timer = g.setInterval(function () {
      catchUp(cs);
      if (cs.state !== 'running' || cs.dead || Object.keys(cs.playing).length === 0) {
        g.clearInterval(cs.timer);
        cs.timer = null;
      }
    }, 40);
  }
  function run(ctx, cs) {
    if (cs.since === null) cs.since = now();
    setState(ctx, cs, 'running');
    if (Object.keys(cs.playing).length) pump(ctx, cs);
  }
  function halt(ctx, cs, st) {
    catchUp(cs);
    if (cs.since !== null) { cs.acc += (now() - cs.since) / 1000; cs.since = null; }
    if (cs.timer !== null) { g.clearInterval(cs.timer); cs.timer = null; }
    setState(ctx, cs, st);
  }

  class BaseAudioContext extends ET {
    constructor(key) {
      if (key !== KEY) throw typeErr('Illegal constructor');
      super();
    }
    get destination() { return brand(CTX, this).dest; }
    get sampleRate() { return brand(CTX, this).sr; }
    get currentTime() { return frames(brand(CTX, this)) / CTX.get(this).sr; }
    get listener() { return brand(CTX, this).listener; }
    get state() { return brand(CTX, this).state; }
    get audioWorklet() { return brand(CTX, this).worklet; }
    createBuffer(numberOfChannels, length, sampleRate) {
      brand(CTX, this);
      if (arguments.length < 3) throw typeErr("Failed to execute 'createBuffer' on 'BaseAudioContext': 3 arguments required, but only " + arguments.length + ' present.');
      return makeBuffer(Object.create(AudioBuffer.prototype), unsigned(numberOfChannels), unsigned(length),
        finite(sampleRate, "Failed to execute 'createBuffer' on 'BaseAudioContext'"), 'BaseAudioContext', 'createBuffer');
    }
    decodeAudioData(audioData, successCallback, errorCallback) {
      var cs = CTX.get(this);
      if (!cs) return rejected(typeErr('Illegal invocation'));
      if (!(audioData instanceof ArrayBuffer)) {
        return rejected(typeErr("Failed to execute 'decodeAudioData' on 'BaseAudioContext': parameter 1 is not of type 'ArrayBuffer'."));
      }
      if (audioData.detached) {
        var de = domError('DataCloneError', "Failed to execute 'decodeAudioData' on 'BaseAudioContext': Cannot decode detached ArrayBuffer");
        if (typeof errorCallback === 'function') task(function () { errorCallback(de); });
        return rejected(de);
      }
      var data;
      try { data = typeof audioData.transfer === 'function' ? audioData.transfer() : audioData.slice(0); } catch (e) { data = audioData.slice(0); }
      return new PromiseCtor(function (resolve, reject) {
        task(function () {
          var buffer;
          try {
            buffer = bufferFrom(cs.sr, N.decode(data, cs.sr));
          } catch (e) {
            var err = e && e.name === 'EncodingError' ? e : domError('EncodingError', 'Unable to decode audio data');
            if (typeof errorCallback === 'function') { try { errorCallback(err); } catch (x) { report(x); } }
            reject(err);
            return;
          }
          if (typeof successCallback === 'function') { try { successCallback(buffer); } catch (x) { report(x); } }
          resolve(buffer);
        });
      });
    }
    createBufferSource() { return new AudioBufferSourceNode(this); }
    createConstantSource() { return new ConstantSourceNode(this); }
    createGain() { return new GainNode(this); }
    createDelay(maxDelayTime) { return new DelayNode(this, maxDelayTime === undefined ? {} : { maxDelayTime: maxDelayTime }); }
    createBiquadFilter() { return new BiquadFilterNode(this); }
    createIIRFilter(feedforward, feedback) {
      if (arguments.length < 2) throw typeErr("Failed to execute 'createIIRFilter' on 'BaseAudioContext': 2 arguments required, but only " + arguments.length + ' present.');
      return new IIRFilterNode(this, { feedforward: feedforward, feedback: feedback });
    }
    createWaveShaper() { return new WaveShaperNode(this); }
    createPanner() { return new PannerNode(this); }
    createStereoPanner() { return new StereoPannerNode(this); }
    createDynamicsCompressor() { return new DynamicsCompressorNode(this); }
    createAnalyser() { return new AnalyserNode(this); }
    createOscillator() { return new OscillatorNode(this); }
    createPeriodicWave(real, imag, constraints) {
      brand(CTX, this);
      if (arguments.length < 2) throw typeErr("Failed to execute 'createPeriodicWave' on 'BaseAudioContext': 2 arguments required, but only " + arguments.length + ' present.');
      return makeWave(Object.create(PeriodicWave.prototype), real, imag, constraints && constraints.disableNormalization, 'BaseAudioContext', 'createPeriodicWave');
    }
    createChannelSplitter(numberOfOutputs) { return new ChannelSplitterNode(this, numberOfOutputs === undefined ? {} : { numberOfOutputs: numberOfOutputs }); }
    createChannelMerger(numberOfInputs) { return new ChannelMergerNode(this, numberOfInputs === undefined ? {} : { numberOfInputs: numberOfInputs }); }
    createConvolver() { return new ConvolverNode(this); }
    createScriptProcessor(bufferSize, numberOfInputChannels, numberOfOutputChannels) {
      var cs = brand(CTX, this);
      var size = bufferSize === undefined ? 0 : unsigned(bufferSize);
      var ins = numberOfInputChannels === undefined ? 2 : unsigned(numberOfInputChannels);
      var outs = numberOfOutputChannels === undefined ? 2 : unsigned(numberOfOutputChannels);
      if ([0, 256, 512, 1024, 2048, 4096, 8192, 16384].indexOf(size) < 0) throw fail('BaseAudioContext', 'createScriptProcessor', 'IndexSizeError', 'buffer size (' + size + ') must be 0 or a power of two between 256 and 16384.');
      if (ins === 0 && outs === 0) throw fail('BaseAudioContext', 'createScriptProcessor', 'IndexSizeError', 'number of input channels and output channels cannot both be zero.');
      if (ins > 32) throw fail('BaseAudioContext', 'createScriptProcessor', 'IndexSizeError', 'number of input channels (' + ins + ') exceeds maximum (32).');
      if (outs > 32) throw fail('BaseAudioContext', 'createScriptProcessor', 'IndexSizeError', 'number of output channels (' + outs + ') exceeds maximum (32).');
      var n = new ScriptProcessorNode(KEY);
      initNode(n, this, { kind: 16, nIn: 1, nOut: 1, cc: Math.max(ins, 1), mode: 'explicit', interp: 'speakers', extra: Math.max(outs, 1), params: [],
        cc_check: function (v) { if (v !== ins) throw domError('NotSupportedError', 'channelCount cannot be changed from ' + ins + ' to ' + v); },
        mode_check: function (m) { if (m !== 'explicit') throw domError('NotSupportedError', "channelCountMode cannot be changed from 'explicit' to '" + m + "'"); } }, {});
      NODE.get(n).bufferSize = size || 1024;
      return cs && n;
    }
  }
  handlers(BaseAudioContext.prototype, ['statechange'], CTX);

  function initContext(ctx, sr, channels, offline, length) {
    var id = N.ctxNew(sr, channels);
    var cs = {
      id: id, sr: sr, channels: channels, offline: offline, length: length || 0, state: 'suspended', dead: false,
      acc: 0, since: null, timer: null, playing: {}, rendered: 0, started: false, renderStarted: false,
      suspends: [], resumeWait: null, chunks: null, handlers: {}, wrapped: {}, worklet: null, dest: null, listener: null
    };
    CTX.set(ctx, cs);
    var dest = new AudioDestinationNode(KEY);
    initNode(dest, ctx, {
      kind: -1, nIn: 1, nOut: 1, cc: channels, mode: 'explicit', interp: 'speakers', params: [], fixedId: DEST,
      cc_check: function (v) {
        if (offline && v !== channels) throw domError('InvalidStateError', 'channelCount cannot be changed from ' + channels + ' to ' + v);
        if (v > (offline ? channels : 2)) throw domError('IndexSizeError', 'The channel count provided (' + v + ') is outside the range [1, ' + (offline ? channels : 2) + '].');
      },
      mode_check: function (m) { if (offline && m !== 'explicit') throw domError('InvalidStateError', "channelCountMode cannot be changed from 'explicit' to '" + m + "'"); }
    }, {});
    NODE.get(dest).maxChannels = offline ? channels : 2;
    cs.dest = dest;
    var l = Object.create(AudioListener.prototype);
    var lp = {};
    [['positionX', 0], ['positionY', 0], ['positionZ', 0], ['forwardX', 0], ['forwardY', 0], ['forwardZ', -1], ['upX', 0], ['upY', 1], ['upZ', 0]].forEach(function (s, i) {
      lp[s[0]] = makeParam(cs, LISTENER, i, [s[0], s[1], -FLT_MAX, FLT_MAX, true, false]);
    });
    LISTENER_STATE.set(l, { params: lp, cs: cs });
    cs.listener = l;
    var w = Object.create(AudioWorklet.prototype);
    WORKLET_STATE.set(w, { cs: cs });
    cs.worklet = w;
    return cs;
  }

  class AudioContext extends BaseAudioContext {
    constructor(contextOptions) {
      if (new.target === undefined) throw typeErr("Failed to construct 'AudioContext': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
      super(KEY);
      var o = contextOptions || {};
      var sr = 48000;
      if (o.sampleRate !== undefined) {
        sr = finite(o.sampleRate, "Failed to construct 'AudioContext'");
        if (!(sr >= 3000 && sr <= 768000)) throw failCtor('AudioContext', 'NotSupportedError', 'The hardware sample rate provided (' + sr + ') is outside the range [3000, 768000].');
      }
      if (o.latencyHint !== undefined && typeof o.latencyHint !== 'number' && ['balanced', 'interactive', 'playback'].indexOf(String(o.latencyHint)) < 0) {
        throw typeErr("Failed to construct 'AudioContext': Failed to read the 'latencyHint' property from 'AudioContextOptions': The provided value '" + o.latencyHint + "' is not a valid enum value of type AudioContextLatencyCategory.");
      }
      var cs = initContext(this, sr, 2, false, 0);
      cs.started = true;
      cs.sinkId = '';
      var self = this;
      // Starting is allowed (no autoplay block in this engine): the
      // context starts running after construction, like an allowed one.
      task(function () { if (cs.state === 'suspended' && !cs.userSuspended && !cs.dead) run(self, cs); });
    }
    get baseLatency() { brand(CTX, this); return 0.01; }
    get outputLatency() { brand(CTX, this); return 0; }
    get sinkId() { return brand(CTX, this).sinkId; }
    getOutputTimestamp() {
      var cs = brand(CTX, this);
      return { contextTime: frames(cs) / cs.sr, performanceTime: now() };
    }
    resume() {
      var cs = CTX.get(this), self = this;
      if (!cs) return rejected(typeErr('Illegal invocation'));
      if (cs.state === 'closed') return rejected(fail('AudioContext', 'resume', 'InvalidStateError', 'Cannot resume a closed AudioContext.'));
      cs.userSuspended = false;
      run(self, cs);
      return new PromiseCtor(function (resolve) { task(function () { resolve(); }); });
    }
    suspend() {
      var cs = CTX.get(this), self = this;
      if (!cs) return rejected(typeErr('Illegal invocation'));
      if (cs.state === 'closed') return rejected(fail('AudioContext', 'suspend', 'InvalidStateError', 'Cannot suspend a closed AudioContext.'));
      cs.userSuspended = true;
      halt(self, cs, 'suspended');
      return new PromiseCtor(function (resolve) { task(function () { resolve(); }); });
    }
    close() {
      var cs = CTX.get(this), self = this;
      if (!cs) return rejected(typeErr('Illegal invocation'));
      if (cs.state === 'closed') return rejected(fail('AudioContext', 'close', 'InvalidStateError', 'Cannot close a closed AudioContext.'));
      halt(self, cs, 'closed');
      cs.dead = true;
      cs.playing = {};
      N.ctxFree(cs.id);
      return new PromiseCtor(function (resolve) { task(function () { resolve(); }); });
    }
    setSinkId(sinkId) {
      var cs = CTX.get(this);
      if (!cs) return rejected(typeErr('Illegal invocation'));
      var id = typeof sinkId === 'string' ? sinkId : '';
      if (id !== '' && id !== 'default') return rejected(fail('AudioContext', 'setSinkId', 'NotFoundError', 'There is no audio output device with the identifier ' + id + '.'));
      cs.sinkId = id;
      return PromiseCtor.resolve();
    }
    createMediaElementSource(mediaElement) { return new MediaElementAudioSourceNode(this, { mediaElement: mediaElement }); }
    createMediaStreamSource(mediaStream) { return new MediaStreamAudioSourceNode(this, { mediaStream: mediaStream }); }
    createMediaStreamDestination() { return new MediaStreamAudioDestinationNode(this); }
  }
  handlers(AudioContext.prototype, ['sinkchange', 'error'], CTX);

  class OfflineAudioContext extends BaseAudioContext {
    constructor(contextOptions, length, sampleRate) {
      if (new.target === undefined) throw typeErr("Failed to construct 'OfflineAudioContext': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
      var n, len, sr;
      if (arguments.length === 1 && contextOptions !== null && typeof contextOptions === 'object') {
        if (contextOptions.length === undefined) throw typeErr("Failed to construct 'OfflineAudioContext': Failed to read the 'length' property from 'OfflineAudioContextOptions': Required member is undefined.");
        if (contextOptions.sampleRate === undefined) throw typeErr("Failed to construct 'OfflineAudioContext': Failed to read the 'sampleRate' property from 'OfflineAudioContextOptions': Required member is undefined.");
        n = contextOptions.numberOfChannels === undefined ? 1 : unsigned(contextOptions.numberOfChannels);
        len = unsigned(contextOptions.length);
        sr = finite(contextOptions.sampleRate, "Failed to construct 'OfflineAudioContext'");
      } else {
        if (arguments.length < 3) throw typeErr("Failed to construct 'OfflineAudioContext': 3 arguments required, but only " + arguments.length + ' present.');
        n = unsigned(contextOptions);
        len = unsigned(length);
        sr = finite(sampleRate, "Failed to construct 'OfflineAudioContext'");
      }
      if (n < 1 || n > 32) throw failCtor('OfflineAudioContext', 'NotSupportedError', 'The number of channels provided (' + n + ') is outside the range [1, 32].');
      if (len < 1) throw failCtor('OfflineAudioContext', 'NotSupportedError', 'The number of frames provided (' + len + ') is less than the minimum bound (1).');
      if (!(sr >= 3000 && sr <= 768000)) throw failCtor('OfflineAudioContext', 'NotSupportedError', 'The sampleRate provided (' + sr + ') is outside the range [3000, 768000].');
      super(KEY);
      initContext(this, sr, n, true, len);
    }
    get length() { return brand(CTX, this).length; }
    startRendering() {
      var cs = CTX.get(this), self = this;
      if (!cs) return rejected(typeErr('Illegal invocation'));
      if (cs.renderStarted) return rejected(fail('OfflineAudioContext', 'startRendering', 'InvalidStateError', 'cannot call startRendering more than once'));
      cs.renderStarted = true;
      cs.started = true;
      cs.chunks = [];
      return new PromiseCtor(function (resolve, reject) {
        cs.finish = resolve;
        cs.fail = reject;
        setState(self, cs, 'running');
        task(function () { renderOffline(self, cs); });
      });
    }
    suspend(suspendTime) {
      var cs = CTX.get(this);
      if (!cs) return rejected(typeErr('Illegal invocation'));
      if (arguments.length < 1) return rejected(typeErr("Failed to execute 'suspend' on 'OfflineAudioContext': 1 argument required, but only 0 present."));
      var t = Number(suspendTime);
      if (!isFinite(t)) return rejected(typeErr("Failed to execute 'suspend' on 'OfflineAudioContext': The provided double value is non-finite."));
      if (t < 0) return rejected(fail('OfflineAudioContext', 'suspend', 'InvalidStateError', 'negative suspend time (' + t + ') is not allowed'));
      var frame = Math.floor(t * cs.sr / 128) * 128;
      if (frame >= cs.length) return rejected(fail('OfflineAudioContext', 'suspend', 'InvalidStateError', 'cannot schedule a suspend at frame ' + frame + ' (' + t + ' seconds) because it is greater than or equal to the total render duration of ' + cs.length + ' frames'));
      if (cs.renderStarted && frame < cs.rendered) {
        return rejected(fail('OfflineAudioContext', 'suspend', 'InvalidStateError', 'cannot schedule a suspend at frame ' + frame + ' because the rendering has already passed it'));
      }
      if (cs.suspends.some(function (x) { return x.frame === frame; })) return rejected(fail('OfflineAudioContext', 'suspend', 'InvalidStateError', 'cannot schedule more than one suspend at frame ' + frame));
      return new PromiseCtor(function (resolve) {
        cs.suspends.push({ frame: frame, resolve: resolve });
        cs.suspends.sort(function (a, b) { return a.frame - b.frame; });
      });
    }
    resume() {
      var cs = CTX.get(this), self = this;
      if (!cs) return rejected(typeErr('Illegal invocation'));
      if (cs.state === 'closed') return rejected(fail('OfflineAudioContext', 'resume', 'InvalidStateError', 'cannot resume a closed offline context'));
      if (!cs.renderStarted) return rejected(fail('OfflineAudioContext', 'resume', 'InvalidStateError', 'cannot resume an offline context that has not started'));
      if (cs.state === 'running') return PromiseCtor.resolve();
      return new PromiseCtor(function (resolve) {
        setState(self, cs, 'running');
        task(function () { resolve(); renderOffline(self, cs); });
      });
    }
  }
  handlers(OfflineAudioContext.prototype, ['complete'], CTX);

  function renderOffline(ctx, cs) {
    if (cs.state !== 'running' || cs.dead) return;
    while (cs.rendered < cs.length) {
      var next = cs.length;
      var stop = null;
      for (var i = 0; i < cs.suspends.length; i++) {
        if (cs.suspends[i].frame >= cs.rendered) { stop = cs.suspends[i]; next = Math.min(next, stop.frame); break; }
      }
      if (stop && stop.frame === cs.rendered) {
        cs.suspends.splice(cs.suspends.indexOf(stop), 1);
        cs.state = 'suspended';
        task(function () { fire(ctx, event('statechange')); });
        var res = stop.resolve;
        task(function () { res(); });
        return;
      }
      var r = N.render(cs.id, next - cs.rendered);
      cs.chunks.push(r[1]);
      cs.rendered = next;
      endedSources(cs, r[0]);
    }
    // Done: one AudioBuffer with all chunks, then `complete`.
    var chans = [];
    for (var c = 0; c < cs.channels; c++) {
      var out = new F32(cs.length), off = 0;
      cs.chunks.forEach(function (chunk) {
        var part = new F32(chunk[c]);
        out.set(part.subarray(0, Math.min(part.length, cs.length - off)), off);
        off += part.length;
      });
      chans.push(out);
    }
    cs.chunks = null;
    var buffer = Object.create(AudioBuffer.prototype);
    BUF.set(buffer, { sr: cs.sr, len: cs.length, ch: chans });
    cs.state = 'closed';
    var finish = cs.finish;
    // The promise settles first; `statechange` and `complete` follow as a
    // task, after its reactions.
    task(function () {
      finish(buffer);
      task(function () {
        fire(ctx, event('statechange'));
        fire(ctx, uaEvent('complete', OfflineAudioCompletionEvent.prototype, { renderedBuffer: buffer }));
      });
    });
  }

  // ---- AudioNode -----------------------------------------------------
  var MODES = ['max', 'clamped-max', 'explicit'];
  var INTERPS = ['speakers', 'discrete'];
  function nodeState(n) { return brand(NODE, n); }
  // initNode(node, context, spec, options): spec = { kind, nIn, nOut, cc,
  // mode, interp, params: [[name, default, min, max, aRate, fixedRate]],
  // extra, d1, d2, cc_check, mode_check, interp_check }.
  function initNode(node, context, spec, options) {
    var cs = CTX.get(context);
    if (!cs) throw typeErr("Failed to construct '" + (spec.iface || 'AudioNode') + "': parameter 1 is not of type 'BaseAudioContext'.");
    var st = {
      ctx: context, cs: cs, id: -1, nIn: spec.nIn, nOut: spec.nOut, cc: spec.cc, mode: spec.mode, interp: spec.interp,
      params: {}, conns: [], spec: spec, handlers: {}, wrapped: {}, started: false, stopped: false
    };
    NODE.set(node, st);
    var o = options || {};
    if (o.channelCount !== undefined) checkCount(st, unsigned(o.channelCount), true);
    if (o.channelCountMode !== undefined) checkMode(st, String(o.channelCountMode), true);
    if (o.channelInterpretation !== undefined) checkInterp(st, String(o.channelInterpretation), true);
    var flat = [];
    (spec.params || []).forEach(function (p) { flat.push(p[1], p[2], p[3], p[4] ? 1 : 0); });
    if (spec.fixedId !== undefined) st.id = spec.fixedId;
    else if (!cs.dead) {
      st.id = N.nodeNew(cs.id, spec.kind, [spec.nIn, spec.nOut, st.cc, MODES.indexOf(st.mode), INTERPS.indexOf(st.interp), spec.extra || 0],
        flat, spec.d1 || null, spec.d2 || null);
    }
    if (spec.fixedId !== undefined && !cs.dead) N.nodeSet(cs.id, st.id, 'channels', st.cc, MODES.indexOf(st.mode), INTERPS.indexOf(st.interp));
    (spec.params || []).forEach(function (p, i) {
      st.params[p[0]] = makeParam(cs, st.id, i, p);
      var v = o[p[0]];
      if (v !== undefined) st.params[p[0]].value = v;
    });
    return st;
  }
  function syncChannels(st) {
    if (!st.cs.dead && st.id >= 0) N.nodeSet(st.cs.id, st.id, 'channels', st.cc, MODES.indexOf(st.mode), INTERPS.indexOf(st.interp));
  }
  function checkCount(st, v, ctor) {
    if (v === 0 || v > 32) throw domError('NotSupportedError', (ctor ? "Failed to construct node" : "Failed to set the 'channelCount' property on 'AudioNode'") + ': The channel count provided (' + v + ') is outside the range [1, 32].');
    if (st.spec.cc_check) st.spec.cc_check(v);
    st.cc = v;
  }
  function checkMode(st, m, ctor) {
    if (MODES.indexOf(m) < 0) {
      if (ctor) throw typeErr("The provided value '" + m + "' is not a valid enum value of type ChannelCountMode.");
      return false;
    }
    if (st.spec.mode_check) st.spec.mode_check(m);
    st.mode = m;
    return true;
  }
  function checkInterp(st, m, ctor) {
    if (INTERPS.indexOf(m) < 0) {
      if (ctor) throw typeErr("The provided value '" + m + "' is not a valid enum value of type ChannelInterpretation.");
      return false;
    }
    if (st.spec.interp_check) st.spec.interp_check(m);
    st.interp = m;
    return true;
  }

  class AudioNode extends ET {
    constructor(key) {
      if (key !== KEY) throw typeErr('Illegal constructor');
      super();
    }
    get context() { return nodeState(this).ctx; }
    get numberOfInputs() { return nodeState(this).nIn; }
    get numberOfOutputs() { return nodeState(this).nOut; }
    get channelCount() { return nodeState(this).cc; }
    set channelCount(v) {
      var st = nodeState(this);
      checkCount(st, unsigned(v), false);
      syncChannels(st);
    }
    get channelCountMode() { return nodeState(this).mode; }
    set channelCountMode(v) {
      var st = nodeState(this);
      if (checkMode(st, String(v), false)) syncChannels(st);
    }
    get channelInterpretation() { return nodeState(this).interp; }
    set channelInterpretation(v) {
      var st = nodeState(this);
      if (checkInterp(st, String(v), false)) syncChannels(st);
    }
    connect(destination, output, input) {
      var st = nodeState(this);
      if (arguments.length < 1) throw typeErr("Failed to execute 'connect' on 'AudioNode': 1 argument required, but only 0 present.");
      var out = output === undefined ? 0 : unsigned(output);
      var ds = NODE.get(destination), ps = PARAM.get(destination);
      if (ds) {
        var inp = input === undefined ? 0 : unsigned(input);
        if (ds.cs !== st.cs) throw fail('AudioNode', 'connect', 'InvalidAccessError', 'cannot connect to an AudioNode belonging to a different audio context.');
        if (out >= st.nOut) throw fail('AudioNode', 'connect', 'IndexSizeError', 'output index (' + out + ') exceeds number of outputs (' + st.nOut + ').');
        if (inp >= ds.nIn) throw fail('AudioNode', 'connect', 'IndexSizeError', 'input index (' + inp + ') exceeds number of inputs (' + ds.nIn + ').');
        if (!st.conns.some(function (c) { return c.dst === destination && c.out === out && c.input === inp; })) {
          st.conns.push({ dst: destination, out: out, input: inp, param: false });
          if (!st.cs.dead && st.id >= 0 && ds.id >= 0) N.connect(st.cs.id, st.id, out, ds.id, inp, false);
        }
        return destination;
      }
      if (ps) {
        if (ps.cs !== st.cs) throw fail('AudioNode', 'connect', 'InvalidAccessError', 'cannot connect to an AudioParam belonging to a different audio context.');
        if (out >= st.nOut) throw fail('AudioNode', 'connect', 'IndexSizeError', 'output index (' + out + ') exceeds number of outputs (' + st.nOut + ').');
        if (!st.conns.some(function (c) { return c.dst === destination && c.out === out; })) {
          st.conns.push({ dst: destination, out: out, input: ps.index, param: true, pnode: ps.node });
          if (!st.cs.dead && st.id >= 0 && ps.node >= 0) N.connect(st.cs.id, st.id, out, ps.node, ps.index, true);
        }
        return undefined;
      }
      throw typeErr("Failed to execute 'connect' on 'AudioNode': No function was found that matched the signature provided.");
    }
    disconnect(destinationOrOutput, output, input) {
      var st = nodeState(this);
      var live = !st.cs.dead && st.id >= 0;
      var drop = function (pred) { st.conns = st.conns.filter(function (c) { return !pred(c); }); };
      if (arguments.length === 0) {
        st.conns = [];
        if (live) N.disconnect(st.cs.id, st.id, -1, -1, -1, false);
        return;
      }
      if (typeof destinationOrOutput === 'number' || (!NODE.has(destinationOrOutput) && !PARAM.has(destinationOrOutput))) {
        if (NODE.has(destinationOrOutput) || PARAM.has(destinationOrOutput) || (destinationOrOutput !== null && typeof destinationOrOutput === 'object')) {
          throw typeErr("Failed to execute 'disconnect' on 'AudioNode': No function was found that matched the signature provided.");
        }
        var o0 = unsigned(destinationOrOutput);
        if (o0 >= st.nOut) throw fail('AudioNode', 'disconnect', 'IndexSizeError', 'The output index provided (' + o0 + ') is outside the range [0, ' + st.nOut + ').');
        drop(function (c) { return c.out === o0; });
        if (live) N.disconnect(st.cs.id, st.id, o0, -1, -1, false);
        return;
      }
      var dest = destinationOrOutput;
      var ds = NODE.get(dest), ps = PARAM.get(dest);
      var out = output === undefined ? -1 : unsigned(output);
      if (out >= 0 && out >= st.nOut) throw fail('AudioNode', 'disconnect', 'IndexSizeError', 'The output index provided (' + out + ') is outside the range [0, ' + st.nOut + ').');
      if (ds) {
        var inp = input === undefined ? -1 : unsigned(input);
        if (inp >= 0 && inp >= ds.nIn) throw fail('AudioNode', 'disconnect', 'IndexSizeError', 'The input index provided (' + inp + ') is outside the range [0, ' + ds.nIn + ').');
        var match = function (c) { return !c.param && c.dst === dest && (out < 0 || c.out === out) && (inp < 0 || c.input === inp); };
        if (!st.conns.some(match)) throw fail('AudioNode', 'disconnect', 'InvalidAccessError', 'the given destination is not connected.');
        drop(match);
        if (live && ds.id >= 0) N.disconnect(st.cs.id, st.id, out, ds.id, inp, false);
        return;
      }
      var pmatch = function (c) { return c.param && c.dst === dest && (out < 0 || c.out === out); };
      if (!st.conns.some(pmatch)) throw fail('AudioNode', 'disconnect', 'InvalidAccessError', 'the given AudioParam is not connected.');
      drop(pmatch);
      if (live) N.disconnect(st.cs.id, st.id, out, ps.node, ps.index, true);
    }
  }

  function paramGetter(proto, name) { getter(proto, name, function () { return nodeState(this).params[name]; }); }
  function nodeCtorCheck(name, args, n) {
    if (args.length < n) throw typeErr("Failed to construct '" + name + "': " + n + ' argument' + (n > 1 ? 's' : '') + ' required, but only ' + args.length + ' present.');
  }
  function stereoOnly(iface) {
    return {
      cc_check: function (v) { if (v > 2) throw domError('NotSupportedError', iface + ': channelCount cannot be greater than 2'); },
      mode_check: function (m) { if (m === 'max') throw domError('NotSupportedError', iface + ": channelCountMode cannot be 'max'"); }
    };
  }
  function merge(a, b) { Object.keys(b).forEach(function (k) { a[k] = b[k]; }); return a; }

  class AudioDestinationNode extends AudioNode {
    constructor(key) {
      if (key !== KEY) throw typeErr('Illegal constructor');
      super(KEY);
    }
    get maxChannelCount() { return nodeState(this).maxChannels; }
  }

  class AudioListener {
    constructor() { throw typeErr('Illegal constructor'); }
    setPosition(x, y, z) {
      var p = brand(LISTENER_STATE, this).params;
      p.positionX.value = x; p.positionY.value = y; p.positionZ.value = z;
    }
    setOrientation(x, y, z, xUp, yUp, zUp) {
      var p = brand(LISTENER_STATE, this).params;
      p.forwardX.value = x; p.forwardY.value = y; p.forwardZ.value = z;
      p.upX.value = xUp; p.upY.value = yUp; p.upZ.value = zUp;
    }
  }
  ['positionX', 'positionY', 'positionZ', 'forwardX', 'forwardY', 'forwardZ', 'upX', 'upY', 'upZ'].forEach(function (n) {
    getter(AudioListener.prototype, n, function () { return brand(LISTENER_STATE, this).params[n]; });
  });

  // ---- Scheduled sources ---------------------------------------------
  class AudioScheduledSourceNode extends AudioNode {
    constructor(key) {
      if (key !== KEY) throw typeErr('Illegal constructor');
      super(KEY);
    }
    start(when) {
      var st = nodeState(this);
      var t = when === undefined ? 0 : finite(when, "Failed to execute 'start' on 'AudioScheduledSourceNode'");
      if (t < 0) throw rangeErr("Failed to execute 'start' on 'AudioScheduledSourceNode': The start time provided (" + t + ') is less than the minimum bound (0).');
      if (st.started) throw fail('AudioScheduledSourceNode', 'start', 'InvalidStateError', 'cannot call start more than once.');
      startSource(this, st, t, 0, -1);
    }
    stop(when) {
      var st = nodeState(this);
      var t = when === undefined ? 0 : finite(when, "Failed to execute 'stop' on 'AudioScheduledSourceNode'");
      if (t < 0) throw rangeErr("Failed to execute 'stop' on 'AudioScheduledSourceNode': The stop time provided (" + t + ') is less than the minimum bound (0).');
      if (!st.started) throw fail('AudioScheduledSourceNode', 'stop', 'InvalidStateError', 'cannot call stop without calling start first.');
      if (!st.cs.dead && st.id >= 0) N.nodeSet(st.cs.id, st.id, 'stop', t, 0, 0);
    }
  }
  handlers(AudioScheduledSourceNode.prototype, ['ended'], NODE);
  function startSource(node, st, when, offset, duration) {
    st.started = true;
    if (st.cs.dead || st.id < 0) return;
    if (st.beforeStart) st.beforeStart();
    N.nodeSet(st.cs.id, st.id, 'start', when, offset, duration);
    st.cs.playing[st.id] = node;
    pump(st.ctx, st.cs);
  }

  var WAVES = ['sine', 'square', 'sawtooth', 'triangle'];
  class OscillatorNode extends AudioScheduledSourceNode {
    constructor(context, options) {
      nodeCtorCheck('OscillatorNode', arguments, 1);
      super(KEY);
      var o = options || {};
      var type = o.type === undefined ? 'sine' : String(o.type);
      if (WAVES.indexOf(type) < 0 && type !== 'custom') throw typeErr("Failed to construct 'OscillatorNode': The provided value '" + type + "' is not a valid enum value of type OscillatorType.");
      if (type === 'custom' && !o.periodicWave) throw failCtor('OscillatorNode', 'InvalidStateError', "A PeriodicWave must be specified if the type is set to 'custom'.");
      var cs = CTX.get(context);
      var nyq = cs ? cs.sr / 2 : 24000;
      var st = initNode(this, context, {
        iface: 'OscillatorNode', kind: 2, nIn: 0, nOut: 1, cc: 2, mode: 'max', interp: 'speakers', extra: Math.max(0, WAVES.indexOf(type)),
        params: [['frequency', 440, -nyq, nyq, true], ['detune', 0, -DETUNE_MAX, DETUNE_MAX, true]]
      }, o);
      st.type = type === 'custom' ? 'sine' : type;
      if (o.periodicWave) this.setPeriodicWave(o.periodicWave);
    }
    get type() { return nodeState(this).type; }
    set type(v) {
      var st = nodeState(this), t = String(v);
      if (t === 'custom') throw failSet('OscillatorNode', 'type', 'InvalidStateError', "'type' cannot be set directly to 'custom'.  Use setPeriodicWave() to create a custom Oscillator type.");
      if (WAVES.indexOf(t) < 0) return;
      st.type = t;
      if (!st.cs.dead) N.nodeSet(st.cs.id, st.id, 'wave', WAVES.indexOf(t), 0, 0);
    }
    setPeriodicWave(periodicWave) {
      var st = nodeState(this), w = WAVE.get(periodicWave);
      if (!w) throw typeErr("Failed to execute 'setPeriodicWave' on 'OscillatorNode': parameter 1 is not of type 'PeriodicWave'.");
      st.type = 'custom';
      if (!st.cs.dead) N.nodeSet(st.cs.id, st.id, 'periodic', w.real, w.imag, w.disable);
    }
  }
  paramGetter(OscillatorNode.prototype, 'frequency');
  paramGetter(OscillatorNode.prototype, 'detune');

  class AudioBufferSourceNode extends AudioScheduledSourceNode {
    constructor(context, options) {
      nodeCtorCheck('AudioBufferSourceNode', arguments, 1);
      super(KEY);
      var o = options || {};
      var st = initNode(this, context, {
        iface: 'AudioBufferSourceNode', kind: 3, nIn: 0, nOut: 1, cc: 2, mode: 'max', interp: 'speakers',
        params: [['playbackRate', 1, -FLT_MAX, FLT_MAX, false, true], ['detune', 0, -FLT_MAX, FLT_MAX, false, true]]
      }, o);
      st.buffer = null;
      st.loop = !!o.loop;
      st.loopStart = o.loopStart === undefined ? 0 : finite(o.loopStart, "Failed to construct 'AudioBufferSourceNode'");
      st.loopEnd = o.loopEnd === undefined ? 0 : finite(o.loopEnd, "Failed to construct 'AudioBufferSourceNode'");
      var self = this;
      st.beforeStart = function () { if (st.buffer) N.nodeSet(st.cs.id, st.id, 'buffer', bufData(st.buffer), 0, 0); };
      sendLoop(st);
      if (o.buffer !== undefined && o.buffer !== null) self.buffer = o.buffer;
    }
    get buffer() { return nodeState(this).buffer; }
    set buffer(v) {
      var st = nodeState(this);
      if (v !== null && !BUF.has(v)) throw typeErr("Failed to set the 'buffer' property on 'AudioBufferSourceNode': The provided value is not of type 'AudioBuffer'.");
      if (v !== null && st.bufferSet) throw failSet('AudioBufferSourceNode', 'buffer', 'InvalidStateError', 'Cannot set buffer to non-null after it has been already been set to a non-null buffer');
      if (v !== null) st.bufferSet = true;
      st.buffer = v;
      if (!st.cs.dead && st.id >= 0) N.nodeSet(st.cs.id, st.id, 'buffer', bufData(v), 0, 0);
    }
    get loop() { return nodeState(this).loop; }
    set loop(v) { var st = nodeState(this); st.loop = !!v; sendLoop(st); }
    get loopStart() { return nodeState(this).loopStart; }
    set loopStart(v) { var st = nodeState(this); st.loopStart = finite(v, "Failed to set the 'loopStart' property on 'AudioBufferSourceNode'"); sendLoop(st); }
    get loopEnd() { return nodeState(this).loopEnd; }
    set loopEnd(v) { var st = nodeState(this); st.loopEnd = finite(v, "Failed to set the 'loopEnd' property on 'AudioBufferSourceNode'"); sendLoop(st); }
    start(when, offset, duration) {
      var st = nodeState(this), w = "Failed to execute 'start' on 'AudioBufferSourceNode'";
      var t = when === undefined ? 0 : finite(when, w);
      var off = offset === undefined ? 0 : finite(offset, w);
      var dur = duration === undefined ? -1 : finite(duration, w);
      if (t < 0) throw rangeErr(w + ': The start time provided (' + t + ') is less than the minimum bound (0).');
      if (off < 0) throw rangeErr(w + ': The offset provided (' + off + ') is less than the minimum bound (0).');
      if (duration !== undefined && dur < 0) throw rangeErr(w + ': The duration provided (' + dur + ') is less than the minimum bound (0).');
      if (st.started) throw fail('AudioBufferSourceNode', 'start', 'InvalidStateError', 'cannot call start more than once.');
      startSource(this, st, t, off, dur);
    }
  }
  function sendLoop(st) { if (!st.cs.dead && st.id >= 0) N.nodeSet(st.cs.id, st.id, 'loop', st.loop, st.loopStart, st.loopEnd); }
  paramGetter(AudioBufferSourceNode.prototype, 'playbackRate');
  paramGetter(AudioBufferSourceNode.prototype, 'detune');

  class ConstantSourceNode extends AudioScheduledSourceNode {
    constructor(context, options) {
      nodeCtorCheck('ConstantSourceNode', arguments, 1);
      super(KEY);
      initNode(this, context, { iface: 'ConstantSourceNode', kind: 4, nIn: 0, nOut: 1, cc: 2, mode: 'max', interp: 'speakers',
        params: [['offset', 1, -FLT_MAX, FLT_MAX, true]] }, options);
    }
  }
  paramGetter(ConstantSourceNode.prototype, 'offset');

  // ---- Processing nodes ----------------------------------------------
  class GainNode extends AudioNode {
    constructor(context, options) {
      nodeCtorCheck('GainNode', arguments, 1);
      super(KEY);
      initNode(this, context, { iface: 'GainNode', kind: 1, nIn: 1, nOut: 1, cc: 2, mode: 'max', interp: 'speakers',
        params: [['gain', 1, -FLT_MAX, FLT_MAX, true]] }, options);
    }
  }
  paramGetter(GainNode.prototype, 'gain');

  class DelayNode extends AudioNode {
    constructor(context, options) {
      nodeCtorCheck('DelayNode', arguments, 1);
      super(KEY);
      var o = options || {};
      var max = o.maxDelayTime === undefined ? 1 : finite(o.maxDelayTime, "Failed to construct 'DelayNode'");
      if (!(max > 0 && max < 180)) throw failCtor('DelayNode', 'NotSupportedError', 'The max delay time provided (' + max + ') is outside the range (0, 180).');
      initNode(this, context, { iface: 'DelayNode', kind: 5, nIn: 1, nOut: 1, cc: 2, mode: 'max', interp: 'speakers', extra: max,
        params: [['delayTime', 0, 0, max, true]] }, o);
    }
  }
  paramGetter(DelayNode.prototype, 'delayTime');

  var BIQUADS = ['lowpass', 'highpass', 'bandpass', 'lowshelf', 'highshelf', 'peaking', 'notch', 'allpass'];
  class BiquadFilterNode extends AudioNode {
    constructor(context, options) {
      nodeCtorCheck('BiquadFilterNode', arguments, 1);
      super(KEY);
      var o = options || {};
      var type = o.type === undefined ? 'lowpass' : String(o.type);
      if (BIQUADS.indexOf(type) < 0) throw typeErr("Failed to construct 'BiquadFilterNode': The provided value '" + type + "' is not a valid enum value of type BiquadFilterType.");
      var cs = CTX.get(context);
      var nyq = cs ? cs.sr / 2 : 24000;
      var st = initNode(this, context, { iface: 'BiquadFilterNode', kind: 6, nIn: 1, nOut: 1, cc: 2, mode: 'max', interp: 'speakers',
        params: [['frequency', 350, 0, nyq, true], ['detune', 0, -DETUNE_MAX, DETUNE_MAX, true], ['Q', 1, -FLT_MAX, FLT_MAX, true],
          ['gain', 0, -FLT_MAX, 1541.273681640625, true]] }, o);
      st.type = 'lowpass';
      this.type = type;
    }
    get type() { return nodeState(this).type; }
    set type(v) {
      var st = nodeState(this), t = String(v);
      if (BIQUADS.indexOf(t) < 0) return;
      st.type = t;
      if (!st.cs.dead) N.nodeSet(st.cs.id, st.id, 'biquadType', BIQUADS.indexOf(t), 0, 0);
    }
    getFrequencyResponse(frequencyHz, magResponse, phaseResponse) {
      var st = nodeState(this), w = "Failed to execute 'getFrequencyResponse' on 'BiquadFilterNode'";
      [frequencyHz, magResponse, phaseResponse].forEach(function (a, i) { if (!(a instanceof F32)) throw typeErr(w + ': parameter ' + (i + 1) + " is not of type 'Float32Array'."); });
      if (magResponse.length !== frequencyHz.length || phaseResponse.length !== frequencyHz.length) {
        throw fail('BiquadFilterNode', 'getFrequencyResponse', 'InvalidAccessError', 'The lengths of the arrays must match.');
      }
      var p = st.params;
      var r = N.biquadResponse(BIQUADS.indexOf(st.type), st.cs.sr, p.frequency.value, p.Q.value, p.gain.value, p.detune.value, frequencyHz);
      magResponse.set(new F32(r[0]));
      phaseResponse.set(new F32(r[1]));
    }
  }
  ['frequency', 'detune', 'Q', 'gain'].forEach(function (n) { paramGetter(BiquadFilterNode.prototype, n); });

  class IIRFilterNode extends AudioNode {
    constructor(context, options) {
      nodeCtorCheck('IIRFilterNode', arguments, 2);
      super(KEY);
      var o = options || {};
      if (o.feedforward === undefined) throw typeErr("Failed to construct 'IIRFilterNode': Failed to read the 'feedforward' property from 'IIRFilterOptions': Required member is undefined.");
      if (o.feedback === undefined) throw typeErr("Failed to construct 'IIRFilterNode': Failed to read the 'feedback' property from 'IIRFilterOptions': Required member is undefined.");
      var ff = f32array(o.feedforward, "Failed to construct 'IIRFilterNode'"), fb = f32array(o.feedback, "Failed to construct 'IIRFilterNode'");
      if (ff.length < 1 || ff.length > 20) throw failCtor('IIRFilterNode', 'NotSupportedError', 'The number of feedforward coefficients provided (' + ff.length + ') is outside the range [1, 20].');
      if (fb.length < 1 || fb.length > 20) throw failCtor('IIRFilterNode', 'NotSupportedError', 'The number of feedback coefficients provided (' + fb.length + ') is outside the range [1, 20].');
      if (Array.prototype.every.call(ff, function (x) { return x === 0; })) throw failCtor('IIRFilterNode', 'InvalidStateError', 'At least one feedforward coefficient must be non-zero.');
      if (fb[0] === 0) throw failCtor('IIRFilterNode', 'InvalidStateError', 'First feedback coefficient cannot be zero.');
      initNode(this, context, { iface: 'IIRFilterNode', kind: 7, nIn: 1, nOut: 1, cc: 2, mode: 'max', interp: 'speakers', params: [], d1: ff, d2: fb }, o);
    }
    getFrequencyResponse(frequencyHz, magResponse, phaseResponse) {
      var st = nodeState(this), w = "Failed to execute 'getFrequencyResponse' on 'IIRFilterNode'";
      [frequencyHz, magResponse, phaseResponse].forEach(function (a, i) { if (!(a instanceof F32)) throw typeErr(w + ': parameter ' + (i + 1) + " is not of type 'Float32Array'."); });
      if (magResponse.length !== frequencyHz.length || phaseResponse.length !== frequencyHz.length) {
        throw fail('IIRFilterNode', 'getFrequencyResponse', 'InvalidAccessError', 'The lengths of the arrays must match.');
      }
      if (st.cs.dead) return;
      var r = N.iirResponse(st.cs.id, st.id, st.cs.sr, frequencyHz);
      magResponse.set(new F32(r[0]));
      phaseResponse.set(new F32(r[1]));
    }
  }

  class WaveShaperNode extends AudioNode {
    constructor(context, options) {
      nodeCtorCheck('WaveShaperNode', arguments, 1);
      super(KEY);
      var o = options || {};
      var st = initNode(this, context, { iface: 'WaveShaperNode', kind: 8, nIn: 1, nOut: 1, cc: 2, mode: 'max', interp: 'speakers', params: [] }, o);
      st.curve = null;
      st.oversample = 'none';
      if (o.curve !== undefined && o.curve !== null) this.curve = f32array(o.curve, "Failed to construct 'WaveShaperNode'");
      if (o.oversample !== undefined) {
        if (['none', '2x', '4x'].indexOf(String(o.oversample)) < 0) throw typeErr("Failed to construct 'WaveShaperNode': The provided value '" + o.oversample + "' is not a valid enum value of type OverSampleType.");
        st.oversample = String(o.oversample);
      }
    }
    get curve() { return nodeState(this).curve; }
    set curve(v) {
      var st = nodeState(this);
      if (v !== null && !(v instanceof F32)) throw typeErr("Failed to set the 'curve' property on 'WaveShaperNode': The provided value is not of type 'Float32Array'.");
      if (v !== null && v.length < 2) throw failSet('WaveShaperNode', 'curve', 'InvalidStateError', 'The curve length provided (' + v.length + ') is less than the minimum bound (2).');
      if (v !== null && st.curve !== null) throw failSet('WaveShaperNode', 'curve', 'InvalidStateError', 'The curve can only be set once.');
      st.curve = v;
      if (!st.cs.dead) N.nodeSet(st.cs.id, st.id, 'curve', v, 0, 0);
    }
    get oversample() { return nodeState(this).oversample; }
    set oversample(v) { var st = nodeState(this), s = String(v); if (['none', '2x', '4x'].indexOf(s) >= 0) st.oversample = s; }
  }

  class StereoPannerNode extends AudioNode {
    constructor(context, options) {
      nodeCtorCheck('StereoPannerNode', arguments, 1);
      super(KEY);
      initNode(this, context, merge({ iface: 'StereoPannerNode', kind: 9, nIn: 1, nOut: 1, cc: 2, mode: 'clamped-max', interp: 'speakers',
        params: [['pan', 0, -1, 1, true]] }, stereoOnly('StereoPannerNode')), options);
    }
  }
  paramGetter(StereoPannerNode.prototype, 'pan');

  var DISTANCES = ['linear', 'inverse', 'exponential'];
  class PannerNode extends AudioNode {
    constructor(context, options) {
      nodeCtorCheck('PannerNode', arguments, 1);
      super(KEY);
      var o = options || {};
      var st = initNode(this, context, merge({ iface: 'PannerNode', kind: 10, nIn: 1, nOut: 1, cc: 2, mode: 'clamped-max', interp: 'speakers',
        params: [['positionX', 0, -FLT_MAX, FLT_MAX, true], ['positionY', 0, -FLT_MAX, FLT_MAX, true], ['positionZ', 0, -FLT_MAX, FLT_MAX, true],
          ['orientationX', 1, -FLT_MAX, FLT_MAX, true], ['orientationY', 0, -FLT_MAX, FLT_MAX, true], ['orientationZ', 0, -FLT_MAX, FLT_MAX, true]] },
        stereoOnly('PannerNode')), o);
      st.pm = 'equalpower'; st.dm = 'inverse'; st.ref = 1; st.maxd = 10000; st.rolloff = 1; st.ci = 360; st.co = 360; st.cog = 0;
      var self = this;
      ['panningModel', 'distanceModel', 'refDistance', 'maxDistance', 'rolloffFactor', 'coneInnerAngle', 'coneOuterAngle', 'coneOuterGain'].forEach(function (k) {
        if (o[k] !== undefined) self[k] = o[k];
      });
      sendPanner(st);
    }
    get panningModel() { return nodeState(this).pm; }
    set panningModel(v) { var s = String(v); if (s === 'equalpower' || s === 'HRTF') nodeState(this).pm = s; }
    get distanceModel() { return nodeState(this).dm; }
    set distanceModel(v) { var st = nodeState(this), s = String(v); if (DISTANCES.indexOf(s) >= 0) { st.dm = s; sendPanner(st); } }
    get refDistance() { return nodeState(this).ref; }
    set refDistance(v) {
      var st = nodeState(this), n = finite(v, "Failed to set the 'refDistance' property on 'PannerNode'");
      if (n < 0) throw rangeErr("Failed to set the 'refDistance' property on 'PannerNode': The refDistance provided (" + n + ') is less than the minimum bound (0).');
      st.ref = n; sendPanner(st);
    }
    get maxDistance() { return nodeState(this).maxd; }
    set maxDistance(v) {
      var st = nodeState(this), n = finite(v, "Failed to set the 'maxDistance' property on 'PannerNode'");
      if (n <= 0) throw rangeErr("Failed to set the 'maxDistance' property on 'PannerNode': The maxDistance provided (" + n + ') is less than or equal to the minimum bound (0).');
      st.maxd = n; sendPanner(st);
    }
    get rolloffFactor() { return nodeState(this).rolloff; }
    set rolloffFactor(v) {
      var st = nodeState(this), n = finite(v, "Failed to set the 'rolloffFactor' property on 'PannerNode'");
      if (n < 0) throw rangeErr("Failed to set the 'rolloffFactor' property on 'PannerNode': The rolloffFactor provided (" + n + ') is less than the minimum bound (0).');
      st.rolloff = n; sendPanner(st);
    }
    get coneInnerAngle() { return nodeState(this).ci; }
    set coneInnerAngle(v) { var st = nodeState(this); st.ci = finite(v, "Failed to set the 'coneInnerAngle' property on 'PannerNode'"); sendPanner(st); }
    get coneOuterAngle() { return nodeState(this).co; }
    set coneOuterAngle(v) { var st = nodeState(this); st.co = finite(v, "Failed to set the 'coneOuterAngle' property on 'PannerNode'"); sendPanner(st); }
    get coneOuterGain() { return nodeState(this).cog; }
    set coneOuterGain(v) {
      var st = nodeState(this), n = finite(v, "Failed to set the 'coneOuterGain' property on 'PannerNode'");
      if (n < 0 || n > 1) throw failSet('PannerNode', 'coneOuterGain', 'InvalidStateError', 'The coneOuterGain provided (' + n + ') is outside the range [0, 1].');
      st.cog = n; sendPanner(st);
    }
    setPosition(x, y, z) {
      var p = nodeState(this).params;
      p.positionX.value = x; p.positionY.value = y; p.positionZ.value = z;
    }
    setOrientation(x, y, z) {
      var p = nodeState(this).params;
      p.orientationX.value = x; p.orientationY.value = y; p.orientationZ.value = z;
    }
  }
  function sendPanner(st) {
    if (!st.cs.dead && st.id >= 0) N.nodeSet(st.cs.id, st.id, 'panner', [DISTANCES.indexOf(st.dm), st.ref, st.maxd, st.rolloff, st.ci, st.co, st.cog], 0, 0);
  }
  ['positionX', 'positionY', 'positionZ', 'orientationX', 'orientationY', 'orientationZ'].forEach(function (n) { paramGetter(PannerNode.prototype, n); });

  class DynamicsCompressorNode extends AudioNode {
    constructor(context, options) {
      nodeCtorCheck('DynamicsCompressorNode', arguments, 1);
      super(KEY);
      initNode(this, context, merge({ iface: 'DynamicsCompressorNode', kind: 11, nIn: 1, nOut: 1, cc: 2, mode: 'clamped-max', interp: 'speakers',
        params: [['threshold', -24, -100, 0, false, true], ['knee', 30, 0, 40, false, true], ['ratio', 12, 1, 20, false, true],
          ['attack', 0.003, 0, 1, false, true], ['release', 0.25, 0, 1, false, true]] }, stereoOnly('DynamicsCompressorNode')), options);
    }
    get reduction() {
      var st = nodeState(this);
      if (st.cs.dead || st.id < 0) return 0;
      catchUp(st.cs);
      return N.reduction(st.cs.id, st.id);
    }
  }
  ['threshold', 'knee', 'ratio', 'attack', 'release'].forEach(function (n) { paramGetter(DynamicsCompressorNode.prototype, n); });

  class AnalyserNode extends AudioNode {
    constructor(context, options) {
      nodeCtorCheck('AnalyserNode', arguments, 1);
      super(KEY);
      var o = options || {};
      var st = initNode(this, context, { iface: 'AnalyserNode', kind: 12, nIn: 1, nOut: 1, cc: 2, mode: 'max', interp: 'speakers', params: [] }, o);
      st.fft = 2048; st.minDb = -100; st.maxDb = -30; st.smooth = 0.8; st.freqFrame = -1; st.freq = null;
      if (o.fftSize !== undefined) this.fftSize = o.fftSize;
      var mn = o.minDecibels === undefined ? -100 : finite(o.minDecibels, "Failed to construct 'AnalyserNode'");
      var mx = o.maxDecibels === undefined ? -30 : finite(o.maxDecibels, "Failed to construct 'AnalyserNode'");
      if (mn >= mx) throw failCtor('AnalyserNode', 'IndexSizeError', 'minDecibels (' + mn + ') must be less than maxDecibels (' + mx + ').');
      st.minDb = mn; st.maxDb = mx;
      if (o.smoothingTimeConstant !== undefined) this.smoothingTimeConstant = o.smoothingTimeConstant;
      sendAnalyser(st);
    }
    get fftSize() { return nodeState(this).fft; }
    set fftSize(v) {
      var st = nodeState(this), n = unsigned(v);
      if (n < 32 || n > 32768 || (n & (n - 1)) !== 0) throw failSet('AnalyserNode', 'fftSize', 'IndexSizeError', 'The value provided (' + n + ') is not a power of two between 32 and 32768.');
      st.fft = n; st.freqFrame = -1; sendAnalyser(st);
    }
    get frequencyBinCount() { return nodeState(this).fft / 2; }
    get minDecibels() { return nodeState(this).minDb; }
    set minDecibels(v) {
      var st = nodeState(this), n = finite(v, "Failed to set the 'minDecibels' property on 'AnalyserNode'");
      if (n >= st.maxDb) throw failSet('AnalyserNode', 'minDecibels', 'IndexSizeError', 'The minDecibels provided (' + n + ') is greater than or equal to the maximum bound (' + st.maxDb + ').');
      st.minDb = n;
    }
    get maxDecibels() { return nodeState(this).maxDb; }
    set maxDecibels(v) {
      var st = nodeState(this), n = finite(v, "Failed to set the 'maxDecibels' property on 'AnalyserNode'");
      if (n <= st.minDb) throw failSet('AnalyserNode', 'maxDecibels', 'IndexSizeError', 'The maxDecibels provided (' + n + ') is less than or equal to the minimum bound (' + st.minDb + ').');
      st.maxDb = n;
    }
    get smoothingTimeConstant() { return nodeState(this).smooth; }
    set smoothingTimeConstant(v) {
      var st = nodeState(this), n = finite(v, "Failed to set the 'smoothingTimeConstant' property on 'AnalyserNode'");
      if (n < 0 || n > 1) throw failSet('AnalyserNode', 'smoothingTimeConstant', 'IndexSizeError', 'The smoothing value provided (' + n + ') is outside the range [0, 1].');
      st.smooth = n; sendAnalyser(st);
    }
    getFloatFrequencyData(array) {
      if (!(array instanceof F32)) throw typeErr("Failed to execute 'getFloatFrequencyData' on 'AnalyserNode': parameter 1 is not of type 'Float32Array'.");
      var d = frequency(nodeState(this));
      array.set(d.subarray(0, Math.min(array.length, d.length)));
    }
    getByteFrequencyData(array) {
      if (!(array instanceof Uint8Array)) throw typeErr("Failed to execute 'getByteFrequencyData' on 'AnalyserNode': parameter 1 is not of type 'Uint8Array'.");
      var st = nodeState(this), d = frequency(st), n = Math.min(array.length, d.length), k = 255 / (st.maxDb - st.minDb);
      for (var i = 0; i < n; i++) {
        var b = Math.floor(k * (d[i] - st.minDb));
        array[i] = b < 0 || b !== b ? 0 : (b > 255 ? 255 : b);
      }
    }
    getFloatTimeDomainData(array) {
      if (!(array instanceof F32)) throw typeErr("Failed to execute 'getFloatTimeDomainData' on 'AnalyserNode': parameter 1 is not of type 'Float32Array'.");
      var d = timeData(nodeState(this));
      array.set(d.subarray(0, Math.min(array.length, d.length)));
    }
    getByteTimeDomainData(array) {
      if (!(array instanceof Uint8Array)) throw typeErr("Failed to execute 'getByteTimeDomainData' on 'AnalyserNode': parameter 1 is not of type 'Uint8Array'.");
      var d = timeData(nodeState(this)), n = Math.min(array.length, d.length);
      for (var i = 0; i < n; i++) {
        var b = Math.floor(128 * (1 + d[i]));
        array[i] = b < 0 ? 0 : (b > 255 ? 255 : b);
      }
    }
  }
  function sendAnalyser(st) { if (!st.cs.dead && st.id >= 0) N.nodeSet(st.cs.id, st.id, 'analyser', st.fft, st.smooth, 0); }
  function timeData(st) {
    if (st.cs.dead || st.id < 0) return new F32(st.fft);
    catchUp(st.cs);
    return new F32(N.analyserData(st.cs.id, st.id, 0));
  }
  // Frequency data is computed at most once per render quantum.
  function frequency(st) {
    if (st.cs.dead || st.id < 0) { var z = new F32(st.fft / 2); z.fill(-Infinity); return z; }
    catchUp(st.cs);
    var f = frames(st.cs);
    if (st.freqFrame !== f || !st.freq) {
      st.freq = new F32(N.analyserData(st.cs.id, st.id, 1));
      st.freqFrame = f;
    }
    return st.freq;
  }

  class ChannelSplitterNode extends AudioNode {
    constructor(context, options) {
      nodeCtorCheck('ChannelSplitterNode', arguments, 1);
      super(KEY);
      var o = options || {};
      var n = o.numberOfOutputs === undefined ? 6 : unsigned(o.numberOfOutputs);
      if (n < 1 || n > 32) throw failCtor('ChannelSplitterNode', 'IndexSizeError', 'The number of outputs provided (' + n + ') is outside the range [1, 32].');
      initNode(this, context, { iface: 'ChannelSplitterNode', kind: 13, nIn: 1, nOut: n, cc: n, mode: 'explicit', interp: 'discrete', params: [],
        cc_check: function (v) { if (v !== n) throw domError('InvalidStateError', 'channelCount cannot be changed from ' + n + ' to ' + v); },
        mode_check: function (m) { if (m !== 'explicit') throw domError('InvalidStateError', "channelCountMode cannot be changed from 'explicit' to '" + m + "'"); },
        interp_check: function (m) { if (m !== 'discrete') throw domError('InvalidStateError', "channelInterpretation cannot be changed from 'discrete' to '" + m + "'"); }
      }, o);
    }
  }

  class ChannelMergerNode extends AudioNode {
    constructor(context, options) {
      nodeCtorCheck('ChannelMergerNode', arguments, 1);
      super(KEY);
      var o = options || {};
      var n = o.numberOfInputs === undefined ? 6 : unsigned(o.numberOfInputs);
      if (n < 1 || n > 32) throw failCtor('ChannelMergerNode', 'IndexSizeError', 'The number of inputs provided (' + n + ') is outside the range [1, 32].');
      initNode(this, context, { iface: 'ChannelMergerNode', kind: 14, nIn: n, nOut: 1, cc: 1, mode: 'explicit', interp: 'speakers', params: [],
        cc_check: function (v) { if (v !== 1) throw domError('InvalidStateError', 'channelCount cannot be changed from 1 to ' + v); },
        mode_check: function (m) { if (m !== 'explicit') throw domError('InvalidStateError', "channelCountMode cannot be changed from 'explicit' to '" + m + "'"); }
      }, o);
    }
  }

  class ConvolverNode extends AudioNode {
    constructor(context, options) {
      nodeCtorCheck('ConvolverNode', arguments, 1);
      super(KEY);
      var o = options || {};
      var st = initNode(this, context, merge({ iface: 'ConvolverNode', kind: 15, nIn: 1, nOut: 1, cc: 2, mode: 'clamped-max', interp: 'speakers', params: [] },
        stereoOnly('ConvolverNode')), o);
      st.buffer = null;
      st.normalize = !o.disableNormalization;
      if (o.buffer !== undefined && o.buffer !== null) this.buffer = o.buffer;
    }
    get buffer() { return nodeState(this).buffer; }
    set buffer(v) {
      var st = nodeState(this);
      if (v !== null && !BUF.has(v)) throw typeErr("Failed to set the 'buffer' property on 'ConvolverNode': The provided value is not of type 'AudioBuffer'.");
      if (v !== null) {
        var b = BUF.get(v);
        if ([1, 2, 4].indexOf(b.ch.length) < 0) throw failSet('ConvolverNode', 'buffer', 'NotSupportedError', 'The buffer must have 1, 2, or 4 channels, not ' + b.ch.length);
        if (b.sr !== st.cs.sr) throw failSet('ConvolverNode', 'buffer', 'NotSupportedError', 'The buffer sample rate (' + b.sr + ') does not match the context rate (' + st.cs.sr + ').');
      }
      st.buffer = v;
      if (!st.cs.dead) N.nodeSet(st.cs.id, st.id, 'convolver', bufData(v), st.normalize, 0);
    }
    get normalize() { return nodeState(this).normalize; }
    set normalize(v) { nodeState(this).normalize = !!v; }
  }

  class ScriptProcessorNode extends AudioNode {
    constructor(key) {
      if (key !== KEY) throw typeErr('Illegal constructor');
      super(KEY);
    }
    get bufferSize() { return nodeState(this).bufferSize; }
  }
  handlers(ScriptProcessorNode.prototype, ['audioprocess'], NODE);

  class MediaElementAudioSourceNode extends AudioNode {
    constructor(context, options) {
      nodeCtorCheck('MediaElementAudioSourceNode', arguments, 2);
      super(KEY);
      var o = options || {};
      var cs = CTX.get(context);
      if (!cs || cs.offline) throw typeErr("Failed to construct 'MediaElementAudioSourceNode': parameter 1 is not of type 'AudioContext'.");
      var el = o.mediaElement;
      if (!(typeof g.HTMLMediaElement === 'function' && el instanceof g.HTMLMediaElement)) {
        throw typeErr("Failed to construct 'MediaElementAudioSourceNode': Failed to read the 'mediaElement' property from 'MediaElementAudioSourceOptions': Required member is undefined.");
      }
      if (MEDIA_SOURCES.has(el)) throw failCtor('MediaElementAudioSourceNode', 'InvalidStateError', 'HTMLMediaElement already connected previously to a different MediaElementSourceNode.');
      MEDIA_SOURCES.add(el);
      var st = initNode(this, context, { iface: 'MediaElementAudioSourceNode', kind: 16, nIn: 0, nOut: 1, cc: 2, mode: 'max', interp: 'speakers', extra: 2, params: [] }, {});
      st.element = el;
    }
    get mediaElement() { return nodeState(this).element; }
  }

  class MediaStreamAudioSourceNode extends AudioNode {
    constructor(context, options) {
      nodeCtorCheck('MediaStreamAudioSourceNode', arguments, 2);
      super(KEY);
      var o = options || {};
      var cs = CTX.get(context);
      if (!cs || cs.offline) throw typeErr("Failed to construct 'MediaStreamAudioSourceNode': parameter 1 is not of type 'AudioContext'.");
      var stream = o.mediaStream;
      if (stream === null || typeof stream !== 'object') throw typeErr("Failed to construct 'MediaStreamAudioSourceNode': Failed to read the 'mediaStream' property from 'MediaStreamAudioSourceOptions': Required member is undefined.");
      if (typeof stream.getAudioTracks === 'function' && stream.getAudioTracks().length === 0) {
        throw failCtor('MediaStreamAudioSourceNode', 'InvalidStateError', 'MediaStream has no audio track');
      }
      var st = initNode(this, context, { iface: 'MediaStreamAudioSourceNode', kind: 16, nIn: 0, nOut: 1, cc: 2, mode: 'max', interp: 'speakers', extra: 2, params: [] }, {});
      st.stream = stream;
    }
    get mediaStream() { return nodeState(this).stream; }
  }

  class MediaStreamAudioDestinationNode extends AudioNode {
    constructor(context, options) {
      nodeCtorCheck('MediaStreamAudioDestinationNode', arguments, 1);
      super(KEY);
      var cs = CTX.get(context);
      if (!cs || cs.offline) throw typeErr("Failed to construct 'MediaStreamAudioDestinationNode': parameter 1 is not of type 'AudioContext'.");
      var st = initNode(this, context, { iface: 'MediaStreamAudioDestinationNode', kind: 16, nIn: 1, nOut: 0, cc: 2, mode: 'explicit', interp: 'speakers', extra: 2, params: [] }, options);
      var stream = null;
      try { if (typeof g.MediaStream === 'function') stream = new g.MediaStream(); } catch (e) { stream = null; }
      st.stream = stream;
    }
    get stream() { return nodeState(this).stream; }
  }

  // ---- AudioWorklet --------------------------------------------------
  // Worklet processors cannot run yet (no AudioWorkletGlobalScope): modules
  // load (the promise resolves) but no processor name gets defined.
  class AudioWorklet {
    constructor() { throw typeErr('Illegal constructor'); }
    addModule(moduleURL) {
      if (!WORKLET_STATE.has(this)) return rejected(typeErr('Illegal invocation'));
      if (arguments.length < 1) return rejected(typeErr("Failed to execute 'addModule' on 'Worklet': 1 argument required, but only 0 present."));
      var url;
      try { url = new g.URL(String(moduleURL), g.document && g.document.baseURI ? g.document.baseURI : undefined).href; } catch (e) {
        return rejected(domError('SyntaxError', "Failed to execute 'addModule' on 'Worklet': Invalid URL"));
      }
      if (typeof g.fetch !== 'function') return PromiseCtor.resolve();
      return g.fetch(url).then(function (r) {
        if (!r.ok) throw domError('AbortError', "Failed to execute 'addModule' on 'Worklet': Unable to load a worklet's module.");
      });
    }
  }
  class AudioWorkletNode extends AudioNode {
    constructor(context, name) {
      nodeCtorCheck('AudioWorkletNode', arguments, 2);
      super(KEY);
      if (!CTX.has(context)) throw typeErr("Failed to construct 'AudioWorkletNode': parameter 1 is not of type 'BaseAudioContext'.");
      throw failCtor('AudioWorkletNode', 'InvalidStateError', "AudioWorkletNode cannot be created: The node name '" + String(name) + "' is not defined in AudioWorkletGlobalScope.");
    }
  }

  [
    ['BaseAudioContext', BaseAudioContext], ['AudioContext', AudioContext], ['OfflineAudioContext', OfflineAudioContext],
    ['AudioNode', AudioNode], ['AudioParam', AudioParam], ['AudioBuffer', AudioBuffer], ['PeriodicWave', PeriodicWave],
    ['AudioDestinationNode', AudioDestinationNode], ['AudioListener', AudioListener], ['AudioScheduledSourceNode', AudioScheduledSourceNode],
    ['OscillatorNode', OscillatorNode], ['AudioBufferSourceNode', AudioBufferSourceNode], ['ConstantSourceNode', ConstantSourceNode],
    ['GainNode', GainNode], ['DelayNode', DelayNode], ['BiquadFilterNode', BiquadFilterNode], ['IIRFilterNode', IIRFilterNode],
    ['WaveShaperNode', WaveShaperNode], ['StereoPannerNode', StereoPannerNode], ['PannerNode', PannerNode],
    ['DynamicsCompressorNode', DynamicsCompressorNode], ['AnalyserNode', AnalyserNode], ['ChannelSplitterNode', ChannelSplitterNode],
    ['ChannelMergerNode', ChannelMergerNode], ['ConvolverNode', ConvolverNode], ['ScriptProcessorNode', ScriptProcessorNode],
    ['MediaElementAudioSourceNode', MediaElementAudioSourceNode], ['MediaStreamAudioSourceNode', MediaStreamAudioSourceNode],
    ['MediaStreamAudioDestinationNode', MediaStreamAudioDestinationNode], ['AudioWorklet', AudioWorklet], ['AudioWorkletNode', AudioWorkletNode],
    ['OfflineAudioCompletionEvent', OfflineAudioCompletionEvent], ['AudioProcessingEvent', AudioProcessingEvent]
  ].forEach(function (p) {
    defineProperty(g, p[0], { value: p[1], writable: true, enumerable: false, configurable: true });
  });
})
