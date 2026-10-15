// Release 15.10.2026 (Agent C): Web Animations (Level 1), CSS Animations and
// CSS Transitions for Narsil. Loaded on first use (`vorspann.js`,
// `__narsilAnimQuelle`). The animated values go to the host per element
// (`__narsilAnimSetzen`), where they form their own cascade layer; values are
// interpolated by the host (`__narsilInterpolieren`).
// The loader passes `h`: the host functions (`set`, `base`, `lerp`, `value`,
// `raf`); the module puts its entry points on it (`flush`, `frame`, `intern`).
(function (h) {
  'use strict';
  var g = globalThis;
  if (h.loaded) return;
  h.loaded = true;
  var HOST_SET = h.set, HOST_BASE = h.base, HOST_LERP = h.lerp;
  var HOST_VALUE = h.value;
  var D = g.Document && g.Document.prototype;
  var E = g.Element && g.Element.prototype;
  // Diagnosis: time (ms) and calls of the expensive steps.
  var stats = { cssUpdate: 0, cssUpdateMs: 0, scans: 0, scanMs: 0, keyframes: 0, keyframesMs: 0, flush: 0, flushMs: 0, bases: 0, basesMs: 0, frames: 0, sets: 0 };
  try { Object.defineProperty(g, '__narsilAnimStats', { value: stats, configurable: true, writable: true, enumerable: false }); } catch (e) {}

  function domError(msg, name) {
    try { return new g.DOMException(msg, name); } catch (e) { var x = new Error(msg); x.name = name; return x; }
  }
  function now() { return g.performance && g.performance.now ? g.performance.now() : Date.now(); }
  function kebab(n) {
    if (n === 'cssFloat') return 'float';
    if (n === 'cssOffset') return 'offset';
    if (n.indexOf('--') === 0) return n;
    return n.replace(/[A-Z]/g, function (c) { return '-' + c.toLowerCase(); }).replace(/^(webkit|moz|ms)-/, '-$1-');
  }
  function camel(n) {
    if (n === 'float') return 'cssFloat';
    if (n === 'offset') return 'cssOffset';
    if (n.indexOf('--') === 0) return n;
    return n.replace(/^-(webkit|moz|ms)-/, '$1-').replace(/-([a-z])/g, function (m, c) { return c.toUpperCase(); });
  }

  // ------------------------------------------------------------------
  // Timing functions (CSS Easing 1)
  // ------------------------------------------------------------------
  function bezier(x1, y1, x2, y2) {
    function a(p1, p2) { return 1 - 3 * p2 + 3 * p1; }
    function b(p1, p2) { return 3 * p2 - 6 * p1; }
    function c(p1) { return 3 * p1; }
    function calc(t, p1, p2) { return ((a(p1, p2) * t + b(p1, p2)) * t + c(p1)) * t; }
    function slope(t, p1, p2) { return 3 * a(p1, p2) * t * t + 2 * b(p1, p2) * t + c(p1); }
    return function (x) {
      if (x <= 0) {
        // Linear extrapolation before 0 (CSS Easing 1 §3.1).
        var s0 = x1 > 0 ? y1 / x1 : (y1 === 0 && x2 > 0 ? y2 / x2 : 0);
        return s0 * x;
      }
      if (x >= 1) {
        var s1 = x2 < 1 ? (y2 - 1) / (x2 - 1) : (y2 === 1 && x1 < 1 ? (y1 - 1) / (x1 - 1) : 0);
        return 1 + s1 * (x - 1);
      }
      var t = x;
      for (var i = 0; i < 8; i++) {
        var e = calc(t, x1, x2) - x;
        if (Math.abs(e) < 1e-7) break;
        var d = slope(t, x1, x2);
        if (Math.abs(d) < 1e-6) break;
        t -= e / d;
      }
      var lo = 0, hi = 1;
      if (Math.abs(calc(t, x1, x2) - x) > 1e-7 || t < 0 || t > 1) {
        t = x;
        for (var j = 0; j < 40; j++) {
          var v = calc(t, x1, x2);
          if (Math.abs(v - x) < 1e-7) break;
          if (v < x) lo = t; else hi = t;
          t = (lo + hi) / 2;
        }
      }
      return calc(t, y1, y2);
    };
  }
  function steps(n, pos) {
    return function (x, before) {
      var jumps = n;
      if (pos === 'jump-none') jumps = n - 1;
      else if (pos === 'jump-both') jumps = n + 1;
      var step = Math.floor(x * n);
      if (pos === 'jump-start' || pos === 'start' || pos === 'jump-both') step += 1;
      if (before && Math.floor(x * n) === x * n) step -= 1;
      if (x >= 0 && step < 0) step = 0;
      if (x <= 1 && step > jumps) step = jumps;
      return step / jumps;
    };
  }
  function linearPoints(args) {
    // `linear(0, 0.25 75%, 1)`
    var parts = args.split(',').map(function (s) { return s.trim(); });
    var pts = [];
    for (var i = 0; i < parts.length; i++) {
      var m = parts[i].split(/\s+/);
      var out = parseFloat(m[0]);
      if (!isFinite(out)) return null;
      var ins = [];
      for (var k = 1; k < m.length; k++) {
        if (!/%$/.test(m[k])) return null;
        ins.push(parseFloat(m[k]) / 100);
      }
      if (ins.length > 2) return null;
      if (!ins.length) pts.push([out, null]);
      else ins.forEach(function (x) { pts.push([out, x]); });
    }
    if (pts.length < 2) return null;
    if (pts[0][1] === null) pts[0][1] = 0;
    if (pts[pts.length - 1][1] === null) pts[pts.length - 1][1] = 1;
    var maxIn = 0;
    for (var p = 0; p < pts.length; p++) {
      if (pts[p][1] !== null) { if (pts[p][1] < maxIn) pts[p][1] = maxIn; maxIn = pts[p][1]; }
    }
    for (var q = 1; q < pts.length; q++) {
      if (pts[q][1] === null) {
        var r = q;
        while (pts[r][1] === null) r++;
        var a = pts[q - 1][1], b = pts[r][1], cnt = r - q + 1;
        for (var z = q; z < r; z++) pts[z][1] = a + (b - a) * (z - q + 1) / cnt;
      }
    }
    return function (x) {
      if (x <= pts[0][1]) {
        var a0 = pts[0], a1 = pts[1];
        return a1[1] === a0[1] ? a0[0] : a0[0] + (a1[0] - a0[0]) * (x - a0[1]) / (a1[1] - a0[1]);
      }
      for (var i2 = 1; i2 < pts.length; i2++) {
        if (x <= pts[i2][1] || i2 === pts.length - 1) {
          var u = pts[i2 - 1], v = pts[i2];
          if (v[1] === u[1]) return v[0];
          return u[0] + (v[0] - u[0]) * (x - u[1]) / (v[1] - u[1]);
        }
      }
      return pts[pts.length - 1][0];
    };
  }
  var NAMED = {
    'linear': [function (x) { return x; }, 'linear'],
    'ease': [bezier(0.25, 0.1, 0.25, 1), 'ease'],
    'ease-in': [bezier(0.42, 0, 1, 1), 'ease-in'],
    'ease-out': [bezier(0, 0, 0.58, 1), 'ease-out'],
    'ease-in-out': [bezier(0.42, 0, 0.58, 1), 'ease-in-out'],
    'step-start': [steps(1, 'jump-start'), 'steps(1, start)'],
    'step-end': [steps(1, 'jump-end'), 'steps(1)']
  };
  function zahlText(v) {
    var s = String(Math.round(v * 1e6) / 1e6);
    return s;
  }
  // Parse an easing; returns [fn, canonical text] or throws TypeError.
  function cssUnescape(s) {
    return s.replace(/\\([0-9a-fA-F]{1,6})[ \t\n]?|\\(.)/g, function (m, hex, ch) {
      return hex ? String.fromCodePoint(parseInt(hex, 16)) : ch;
    });
  }
  function parseEasing(text) {
    var t = cssUnescape(String(text).replace(/\/\*[\s\S]*?\*\//g, ' ').trim());
    var low = t.toLowerCase();
    if (NAMED[low]) return NAMED[low];
    var m = /^cubic-bezier\(\s*([^,]+),([^,]+),([^,]+),([^,)]+)\)$/i.exec(t);
    if (m) {
      var n = [m[1], m[2], m[3], m[4]].map(function (s) { return Number(s.trim()); });
      if (n.every(isFinite) && n[0] >= 0 && n[0] <= 1 && n[2] >= 0 && n[2] <= 1 && [m[1], m[2], m[3], m[4]].every(function (s) { return /^\s*[-+]?(\d+\.?\d*|\.\d+)(e[-+]?\d+)?\s*$/i.test(s); })) {
        return [bezier(n[0], n[1], n[2], n[3]), 'cubic-bezier(' + n.map(zahlText).join(', ') + ')'];
      }
    }
    m = /^steps\(\s*(\d+)\s*(?:,\s*(start|end|jump-start|jump-end|jump-none|jump-both)\s*)?\)$/i.exec(t);
    if (m) {
      var cnt = parseInt(m[1], 10), pos = (m[2] || 'end').toLowerCase();
      if (cnt >= 1 && !(pos === 'jump-none' && cnt < 2)) {
        var canonPos = pos === 'jump-end' || pos === 'end' ? '' : (pos === 'jump-start' ? 'start' : pos);
        return [steps(cnt, pos), 'steps(' + cnt + (canonPos ? ', ' + canonPos : '') + ')'];
      }
    }
    m = /^linear\((.*)\)$/i.exec(t);
    if (m) {
      var f = linearPoints(m[1]);
      if (f) return [f, 'linear(' + m[1].split(',').map(function (s) { return s.trim().replace(/\s+/g, ' '); }).join(', ') + ')'];
    }
    throw new TypeError("Failed to parse easing '" + t + "'.");
  }

  // ------------------------------------------------------------------
  // Timelines
  // ------------------------------------------------------------------
  var frameTime = null;
  var timelines = [];
  function AnimationTimeline() { throw new TypeError('Illegal constructor'); }
  Object.defineProperty(AnimationTimeline.prototype, 'currentTime', {
    configurable: true, enumerable: true,
    get: function () { return timelineTime(this); }
  });
  Object.defineProperty(AnimationTimeline.prototype, 'duration', { configurable: true, enumerable: true, get: function () { return null; } });
  function timelineTime(tl) {
    if (!tl || tl.__inactive) return null;
    // Without a running frame loop the time of this moment starts a frame:
    // it stays the same until the next frame (HTML "update the rendering").
    if (!looping) { frameTime = now(); startLoop(); }
    var t = frameTime;
    var v = t - (tl.__origin || 0);
    return v < 0 && !tl.__erlaubtNegativ ? null : v;
  }
  function DocumentTimeline(opts) {
    if (!(this instanceof DocumentTimeline)) throw new TypeError("Failed to construct 'DocumentTimeline': Please use the 'new' operator");
    var origin = opts && opts.originTime !== undefined ? Number(opts.originTime) : 0;
    Object.defineProperty(this, '__origin', { value: isFinite(origin) ? origin : 0, writable: true });
    Object.defineProperty(this, '__erlaubtNegativ', { value: true, writable: true });
    timelines.push(this);
  }
  DocumentTimeline.prototype = Object.create(AnimationTimeline.prototype);
  Object.defineProperty(DocumentTimeline.prototype, 'constructor', { value: DocumentTimeline, writable: true, configurable: true, enumerable: false });
  var docTimeline = Object.create(DocumentTimeline.prototype);
  Object.defineProperty(docTimeline, '__origin', { value: 0, writable: true });
  g.AnimationTimeline = AnimationTimeline;
  g.DocumentTimeline = DocumentTimeline;
  if (D) Object.defineProperty(D, 'timeline', { configurable: true, enumerable: true, get: function () { return this === g.document ? docTimeline : (this.__timeline || (this.__timeline = Object.create(DocumentTimeline.prototype))); } });

  // ------------------------------------------------------------------
  // Effects and timing
  // ------------------------------------------------------------------
  var FILLS = ['none', 'forwards', 'backwards', 'both', 'auto'];
  var DIRS = ['normal', 'reverse', 'alternate', 'alternate-reverse'];
  function normTiming(t, input, ctor) {
    // Validate first (all or nothing), then assign.
    var n = {};
    if (input === undefined || input === null) return;
    if (typeof input === 'number') input = { duration: input };
    if (typeof input !== 'object') {
      if (ctor) { input = { duration: Number(input) }; } else return;
    }
    if (input.delay !== undefined) { var d = Number(input.delay); if (!isFinite(d)) throw new TypeError('delay must be finite'); n.delay = d; }
    if (input.endDelay !== undefined) { var ed = Number(input.endDelay); if (!isFinite(ed)) throw new TypeError('endDelay must be finite'); n.endDelay = ed; }
    if (input.fill !== undefined) { var f = String(input.fill); if (FILLS.indexOf(f) < 0) throw new TypeError("The provided value '" + f + "' is not a valid enum value of type FillMode."); n.fill = f; }
    if (input.iterationStart !== undefined) { var is = Number(input.iterationStart); if (!isFinite(is) || is < 0) throw new TypeError('iterationStart must be non-negative'); n.iterationStart = is; }
    if (input.iterations !== undefined) { var it = Number(input.iterations); if (isNaN(it) || it < 0) throw new TypeError('iterations must be non-negative'); n.iterations = it; }
    if (input.duration !== undefined) {
      var du = input.duration;
      if (typeof du === 'string') {
        if (du !== 'auto') throw new TypeError("duration must be a number or 'auto'");
        n.duration = 'auto';
      } else {
        du = Number(du);
        if (isNaN(du) || du < 0) throw new TypeError('duration must be non-negative');
        n.duration = du;
      }
    }
    if (input.direction !== undefined) { var di = String(input.direction); if (DIRS.indexOf(di) < 0) throw new TypeError("The provided value '" + di + "' is not a valid enum value of type PlaybackDirection."); n.direction = di; }
    if (input.easing !== undefined) { var ez = parseEasing(input.easing); n.easing = ez[1]; n.__easingFn = ez[0]; }
    for (var k in n) t[k] = n[k];
  }
  function defaultTiming() {
    return { delay: 0, endDelay: 0, fill: 'auto', iterationStart: 0, iterations: 1, duration: 'auto', direction: 'normal', easing: 'linear', __easingFn: NAMED.linear[0] };
  }
  function AnimationEffect() { throw new TypeError('Illegal constructor'); }
  function iterDuration(t) { return t.duration === 'auto' ? 0 : t.duration; }
  function activeDuration(t) {
    var d = iterDuration(t);
    if (d === 0 || t.iterations === 0) return 0;
    return d * t.iterations;
  }
  function endTime(t) { return Math.max(t.delay + activeDuration(t) + t.endDelay, 0); }
  function effectAnimation(eff) { return eff.__animation || null; }
  function localTimeOf(eff) {
    var a = effectAnimation(eff);
    if (!a) return null;
    return currentTime(a);
  }
  // CSS Animations/Transitions and keyframe effects: `auto` fill is `none`.
  function fillMode(t) { return t.fill === 'auto' ? 'none' : t.fill; }
  function computed(eff) {
    var t = eff.__timing;
    var lt = localTimeOf(eff);
    var ad = activeDuration(t), et = endTime(t);
    var out = {
      delay: t.delay, endDelay: t.endDelay, fill: fillMode(t), iterationStart: t.iterationStart, iterations: t.iterations,
      duration: iterDuration(t), direction: t.direction, easing: t.easing,
      endTime: et, activeDuration: ad, localTime: lt, progress: null, currentIteration: null, startTime: 0
    };
    if (lt === null) { out.__phase = 'none'; return out; }
    var a = effectAnimation(eff);
    var rate = a ? effectiveRate(a) : 1;
    var before = Math.max(Math.min(t.delay, et), 0), after = Math.max(Math.min(t.delay + ad, et), 0);
    var phase;
    if (lt < before || (rate < 0 && lt === before)) phase = 'before';
    else if (lt > after || (rate >= 0 && lt === after)) phase = 'after';
    else phase = 'active';
    out.__phase = phase;
    var fill = fillMode(t);
    var activeTime = null;
    if (phase === 'before') { if (fill === 'backwards' || fill === 'both') activeTime = Math.max(lt - t.delay, 0); }
    else if (phase === 'active') activeTime = lt - t.delay;
    else if (fill === 'forwards' || fill === 'both') activeTime = Math.max(Math.min(lt - t.delay, ad), 0);
    if (activeTime === null) return out;
    var dur = iterDuration(t);
    var overall;
    if (dur === 0) overall = phase === 'before' ? 0 : t.iterations;
    else overall = activeTime / dur;
    overall += t.iterationStart;
    var simple = overall === Infinity ? t.iterationStart % 1 : overall % 1;
    if (simple === 0 && (phase === 'active' || phase === 'after') && activeTime === ad && t.iterations !== 0 && overall !== 0) simple = 1;
    var iter;
    if (phase === 'after' && t.iterations === Infinity) iter = Infinity;
    else if (simple === 1) iter = Math.floor(overall) - 1;
    else iter = Math.floor(overall);
    // The current direction (Web Animations 1 §4.10.2).
    var dir = t.direction, forwards;
    if (dir === 'normal') forwards = true;
    else if (dir === 'reverse') forwards = false;
    else {
      var d = dir === 'alternate-reverse' ? iter + 1 : iter;
      forwards = d === Infinity || d % 2 === 0;
    }
    var directed = forwards ? simple : 1 - simple;
    // The before flag of a step easing (§4.11.1).
    var beforeFlag = (phase === 'before' && forwards) || (phase === 'after' && !forwards);
    out.progress = t.__easingFn(directed, beforeFlag);
    out.__before = beforeFlag;
    out.currentIteration = iter;
    out.__directed = directed;
    return out;
  }
  AnimationEffect.prototype.getTiming = function getTiming() {
    if (this.__animation && this.__animation.__css && h.cssUpdate) { try { h.cssUpdate(); } catch (e) {} }
    var t = this.__timing;
    return { delay: t.delay, endDelay: t.endDelay, fill: t.fill, iterationStart: t.iterationStart, iterations: t.iterations, duration: t.duration, direction: t.direction, easing: t.easing };
  };
  AnimationEffect.prototype.getComputedTiming = function getComputedTiming() {
    if (this.__animation && this.__animation.__css && h.cssUpdate) { try { h.cssUpdate(); } catch (e) {} }
    var c = computed(this);
    var o = {};
    for (var k in c) if (k.charAt(0) !== '_') o[k] = c[k];
    return o;
  };
  AnimationEffect.prototype.updateTiming = function updateTiming(opt) {
    if (opt !== undefined && opt !== null && typeof opt !== 'object') throw new TypeError("Failed to execute 'updateTiming': parameter 1 is not an object.");
    normTiming(this.__timing, opt || {}, false);
    // A CSS animation no longer takes these from its style (CSS Animations
    // 2 §4.4).
    if (opt) { var o = this.__overrides || (this.__overrides = {}); for (var k in opt) o[k] = true; }
    var a = effectAnimation(this);
    if (a) { updateFinished(a, false, false); dirty(); }
  };
  g.AnimationEffect = AnimationEffect;

  // Keyframes ---------------------------------------------------------

  function valText(v) {
    if (v === null || v === undefined) return null;
    return String(v);
  }
  var COMPOSITES = ['replace', 'add', 'accumulate', 'auto'];
  var HOST_NORMAL = h.normal, HOST_LONGHANDS_EARLY = h.longhands;
  // A keyframe value, serialized; `null` drops it (unknown property or
  // invalid value).
  function normalizeValue(name, v) {
    if (v === null || v === undefined) return null;
    if (!HOST_NORMAL) return v;
    var r = HOST_NORMAL(name, String(v));
    return typeof r === 'string' ? r : null;
  }
  // Web Animations 1 §6.6.2 "process a keyframes argument": the dictionary
  // members are read first (composite, easing, offset), then the animatable
  // properties among the object's own enumerable keys in code point order,
  // each exactly once.
  var NOT_ANIMATABLE = { 'contain': 1, 'direction': 1, 'text-combine-upright': 1, 'text-orientation': 1, 'unicode-bidi': 1, 'will-change': 1, 'writing-mode': 1, 'display': 0 };
  function animatableName(key) {
    if (typeof key !== 'string') return null;
    if (key.indexOf('--') === 0) return key;
    if (key === 'cssFloat') return 'float';
    if (key === 'cssOffset') return 'offset';
    if (key === 'float' || key === 'offset' || key === 'composite' || key === 'easing') return null;
    if (!/^[a-z][a-zA-Z]*$/.test(key)) return null;
    var css = key.replace(/[A-Z]/g, function (c) { return '-' + c.toLowerCase(); }).replace(/^(webkit|moz|ms)-/, '-$1-');
    if (/^(animation|transition)(-|$)/.test(css) || NOT_ANIMATABLE[css]) return null;
    if (HOST_LONGHANDS_EARLY && !longhandsOfName(css).length) return null;
    return css;
  }
  var knownNames = {};
  function longhandsOfName(css) {
    if (css in knownNames) return knownNames[css];
    var r = HOST_LONGHANDS_EARLY(css);
    return (knownNames[css] = Array.isArray(r) ? r : []);
  }
  function compositeValue(v) {
    var s = String(v);
    if (COMPOSITES.indexOf(s) < 0) throw new TypeError("The provided value '" + s + "' is not a valid enum value of type CompositeOperationOrAuto.");
    return s;
  }
  // An offset: a number, or a string with a CSS <number> (`calc(0.5)`).
  function offsetValue(v) {
    if (v === null) return null;
    var o;
    if (typeof v === 'string') {
      var m = /^\s*calc\(\s*([-+]?(?:\d+\.?\d*|\.\d+)(?:e[-+]?\d+)?)\s*\)\s*$/i.exec(v);
      o = m ? Number(m[1]) : Number(v);
    } else o = Number(v);
    if (!isFinite(o)) throw new TypeError('Offsets must be finite numbers.');
    return o;
  }
  function readKeyframeLike(obj, lists) {
    var out = { composite: obj.composite, easing: obj.easing, offset: obj.offset, props: [] };
    var keys = Object.keys(obj).filter(function (k) { return animatableName(k) !== null; }).sort();
    for (var i = 0; i < keys.length; i++) {
      var v = obj[keys[i]];
      if (lists) {
        if (v !== null && typeof v === 'object' && v[g.Symbol.iterator]) v = Array.from(v);
        else v = [v];
        v = v.map(valText);
      } else {
        if (Array.isArray(v)) v = v.join(',');
        v = valText(v);
      }
      out.props.push([animatableName(keys[i]), v]);
    }
    return out;
  }
  function processKeyframes(input) {
    if (input === null || input === undefined) return [];
    if (typeof input !== 'object') throw new TypeError("Failed to execute: parameter is not an object.");
    var frames = [];
    var iter = input[g.Symbol.iterator];
    if (iter !== undefined && iter !== null) {
      if (typeof iter !== 'function') throw new TypeError('Keyframes are not iterable.');
      var it = iter.call(input);
      if (it === null || (typeof it !== 'object' && typeof it !== 'function')) throw new TypeError('The keyframes iterator is not an object.');
      var raw = [];
      for (var guard = 0; guard < 100000; guard++) {
        var step = it.next();
        if (step === null || typeof step !== 'object') throw new TypeError('The iterator result is not an object.');
        if (step.done) break;
        var kf = step.value;
        if (kf !== null && kf !== undefined && typeof kf !== 'object') throw new TypeError('Keyframe is not an object.');
        raw.push(readKeyframeLike(kf || {}, false));
      }
      raw.forEach(function (r) {
        var f = { offset: null, easing: 'linear', composite: 'auto', props: {} };
        if (r.offset !== undefined) f.offset = offsetValue(r.offset);
        if (r.easing !== undefined) f.easing = String(r.easing);
        if (r.composite !== undefined) f.composite = compositeValue(r.composite);
        r.props.forEach(function (pv) {
          var nv = normalizeValue(pv[0], pv[1]);
          if (nv !== null) f.props[pv[0]] = nv;
        });
        frames.push(f);
      });
      var prev = -Infinity;
      for (var j = 0; j < frames.length; j++) {
        var off = frames[j].offset;
        if (off === null) continue;
        if (off < 0 || off > 1) throw new TypeError('Offsets must be null or in the range [0,1].');
        if (off < prev) throw new TypeError('Offsets must be monotonically non-decreasing.');
        prev = off;
      }
    } else {
      // Property-indexed keyframes.
      var r = readKeyframeLike(input, true);
      var byOffset = [];
      r.props.forEach(function (pv) {
        var vals = pv[1];
        for (var q = 0; q < vals.length; q++) {
          var computedOffset = vals.length === 1 ? 1 : q / (vals.length - 1);
          byOffset.push({ off: computedOffset, name: pv[0], v: normalizeValue(pv[0], vals[q]) });
        }
      });
      byOffset.sort(function (a, b) { return a.off - b.off; });
      var current = null;
      byOffset.forEach(function (e) {
        if (!current || current.__off !== e.off) {
          current = { offset: null, __off: e.off, easing: 'linear', composite: 'auto', props: {} };
          frames.push(current);
        }
        if (e.v !== null) current.props[e.name] = e.v;
      });
      var asList = function (v) { return v === undefined ? [] : (Array.isArray(v) ? v : [v]); };
      var offList = asList(r.offset).map(offsetValue);
      offList.forEach(function (o2, idx) { if (frames[idx]) frames[idx].offset = o2; });
      var easList = asList(r.easing).map(String);
      easList.forEach(function (e2) { parseEasing(e2); });
      if (easList.length) frames.forEach(function (fr, idx) { fr.easing = easList[idx % easList.length]; });
      var compList = asList(r.composite).map(compositeValue);
      if (compList.length) frames.forEach(function (fr, idx) { fr.composite = compList[idx % compList.length]; });
      frames.forEach(function (fr) { delete fr.__off; });
      if (offList.length) {
        var pr = -Infinity;
        for (var z = 0; z < frames.length; z++) {
          var oo = frames[z].offset;
          if (oo === null) continue;
          if (oo < 0 || oo > 1) throw new TypeError('Offsets must be null or in the range [0,1].');
          if (oo < pr) throw new TypeError('Offsets must be monotonically non-decreasing.');
          pr = oo;
        }
      }
    }
    // Easings are validated after the offsets (spec order).
    frames.forEach(function (fr) { var e = parseEasing(fr.easing); fr.easing = e[1]; fr.__fn = e[0]; });
    computeOffsets(frames);
    return frames;
  }
  function computeOffsets(frames) {
    if (!frames.length) return;
    for (var i = 0; i < frames.length; i++) frames[i].computedOffset = frames[i].offset;
    if (frames.length > 1 && frames[0].computedOffset === null) frames[0].computedOffset = 0;
    if (frames[frames.length - 1].computedOffset === null) frames[frames.length - 1].computedOffset = frames.length > 1 ? 1 : 1;
    var last = 0;
    for (var j = 1; j < frames.length; j++) {
      if (frames[j].computedOffset === null) continue;
      var gap = j - last;
      for (var k = last + 1; k < j; k++) {
        frames[k].computedOffset = frames[last].computedOffset + (frames[j].computedOffset - frames[last].computedOffset) * (k - last) / gap;
      }
      last = j;
    }
  }

  function KeyframeEffect(target, keyframes, options) {
    if (!(this instanceof KeyframeEffect)) throw new TypeError("Failed to construct 'KeyframeEffect': Please use the 'new' operator");
    if (arguments.length === 1 && target instanceof KeyframeEffect) {
      // Copy constructor.
      var src = target;
      def(this, '__target', src.__target);
      def(this, '__pseudo', src.__pseudo);
      def(this, '__frames', src.__frames.map(cloneFrame));
      def(this, '__timing', Object.assign({}, src.__timing));
      def(this, '__composite', src.__composite);
      def(this, '__iterationComposite', src.__iterationComposite);
      def(this, '__animation', null);
      return;
    }
    if (arguments.length < 2) throw new TypeError("Failed to construct 'KeyframeEffect': 2 arguments required.");
    if (target !== null && (typeof target !== 'object' || typeof target.nodeType !== 'number')) throw new TypeError("Failed to construct 'KeyframeEffect': parameter 1 is not of type 'Element'.");
    var timing = defaultTiming();
    var composite = 'replace', iterComp = 'replace', pseudo = null;
    if (options !== undefined && options !== null && typeof options === 'object') {
      normTiming(timing, options, true);
      if (options.composite !== undefined) { var c = String(options.composite); if (['replace', 'add', 'accumulate'].indexOf(c) < 0) throw new TypeError("The provided value '" + c + "' is not a valid enum value of type CompositeOperation."); composite = c; }
      if (options.iterationComposite !== undefined) { var ic = String(options.iterationComposite); if (['replace', 'accumulate'].indexOf(ic) < 0) throw new TypeError('invalid iterationComposite'); iterComp = ic; }
      if (options.pseudoElement !== undefined && options.pseudoElement !== null) pseudo = checkPseudo(options.pseudoElement);
    } else if (options !== undefined && options !== null) {
      normTiming(timing, Number(options), true);
    }
    def(this, '__target', target);
    def(this, '__pseudo', pseudo);
    def(this, '__frames', processKeyframes(keyframes));
    def(this, '__timing', timing);
    def(this, '__composite', composite);
    def(this, '__iterationComposite', iterComp);
    def(this, '__animation', null);
  }
  function checkPseudo(p) {
    var s = String(p);
    // The pseudo-element name is ASCII case-insensitive (its argument not).
    var m = /^(::?)([a-zA-Z-]+)(\(.*)?$/.exec(s);
    if (m) s = m[1] + m[2].toLowerCase() + (m[3] || '');
    if (/^::?(before|after|marker|first-line|first-letter|placeholder|selection|backdrop)$/.test(s) || /^::(view-transition|part\(|highlight\()/.test(s)) {
      return s.replace(/^:(before|after|marker|first-line|first-letter)$/, '::$1');
    }
    throw domError("Failed to execute: '" + s + "' is not a valid pseudo-element selector.", 'SyntaxError');
  }
  function def(o, k, v) { Object.defineProperty(o, k, { value: v, writable: true, configurable: true }); }
  function cloneFrame(f) { var c = Object.assign({}, f); c.props = Object.assign({}, f.props); return c; }
  KeyframeEffect.prototype = Object.create(AnimationEffect.prototype);
  Object.defineProperty(KeyframeEffect.prototype, 'constructor', { value: KeyframeEffect, writable: true, configurable: true, enumerable: false });
  Object.defineProperty(KeyframeEffect.prototype, 'target', {
    configurable: true, enumerable: true,
    get: function () { return this.__target; },
    set: function (v) {
      var old = this.__target;
      this.__target = v;
      if (old && old !== v) forget(old);
      dirty();
    }
  });
  Object.defineProperty(KeyframeEffect.prototype, 'pseudoElement', {
    configurable: true, enumerable: true,
    get: function () { return this.__pseudo; },
    set: function (v) { this.__pseudo = v === null ? null : checkPseudo(v); dirty(); }
  });
  Object.defineProperty(KeyframeEffect.prototype, 'composite', {
    configurable: true, enumerable: true,
    get: function () { return this.__composite; },
    set: function (v) { var s = String(v); if (['replace', 'add', 'accumulate'].indexOf(s) >= 0) { this.__composite = s; dirty(); } }
  });
  Object.defineProperty(KeyframeEffect.prototype, 'iterationComposite', {
    configurable: true, enumerable: true,
    get: function () { return this.__iterationComposite; },
    set: function (v) { var s = String(v); if (['replace', 'accumulate'].indexOf(s) >= 0) { this.__iterationComposite = s; dirty(); } }
  });
  KeyframeEffect.prototype.getKeyframes = function getKeyframes() {
    if (this.__animation && this.__animation.__css && h.cssUpdate) { try { h.cssUpdate(); } catch (e) {} }
    return this.__frames.map(function (f) {
      var o = { offset: f.offset, computedOffset: f.computedOffset, easing: f.easing, composite: f.composite };
      for (var p in f.props) o[camel(p)] = f.props[p] === null ? '' : f.props[p];
      return o;
    });
  };
  KeyframeEffect.prototype.setKeyframes = function setKeyframes(kf) {
    this.__frames = processKeyframes(kf);
    (this.__overrides || (this.__overrides = {})).keyframes = true;
    dirty();
  };
  g.KeyframeEffect = KeyframeEffect;

  // ------------------------------------------------------------------
  // Animation
  // ------------------------------------------------------------------
  var animations = []; // all animations with a target, in creation order
  var seq = 0;
  function newPromise() {
    var res, rej;
    var p = new Promise(function (a, b) { res = a; rej = b; });
    p.catch(function () {});
    return { p: p, resolve: res, reject: rej, done: false };
  }
  var AnimBase = g.EventTarget || function () {};
  // A class, so that it is an EventTarget (`super()`), and so that pages can
  // subclass it.
  var Animation = class Animation extends AnimBase {
    constructor(effect, timeline) {
      super();
      initAnimation(this, effect, timeline, timeline !== undefined);
    }
  };
  function initAnimation(self, effect, timeline, hasTimeline) {
    def(self, '__seq', ++seq);
    def(self, '__effect', null);
    def(self, '__timeline', hasTimeline ? timeline : docTimeline);
    def(self, '__startTime', null);
    def(self, '__holdTime', null);
    def(self, '__rate', 1);
    def(self, '__pendingRate', null);
    def(self, '__playTask', false);
    def(self, '__pauseTask', false);
    def(self, '__ready', newPromise());
    self.__ready.resolve(self); self.__ready.done = true;
    def(self, '__finished', newPromise());
    def(self, '__prevTime', null);
    def(self, '__finishNotified', false);
    def(self, '__id', '');
    def(self, '__replaceState', 'active');
    def(self, '__events', []);
    def(self, '__handlers', {});
    if (effect !== undefined && effect !== null) setEffect(self, effect);
  }
  g.Animation = Animation;

  function setEffect(a, eff) {
    if (eff === a.__effect) return;
    if (eff && eff.__animation && eff.__animation !== a) eff.__animation.effect = null;
    var old = a.__effect;
    if (old) { old.__animation = null; if (old.__target) forget(old.__target); }
    a.__effect = eff || null;
    if (eff) eff.__animation = a;
    if (animations.indexOf(a) < 0) animations.push(a);
    updateFinished(a, false, false);
    dirty();
  }
  function effectiveRate(a) { return a.__pendingRate !== null ? a.__pendingRate : a.__rate; }
  function tlActive(a) { return a.__timeline && timelineTime(a.__timeline) !== null; }
  function currentTime(a) {
    if (a.__holdTime !== null) return a.__holdTime === 0 ? 0 : a.__holdTime;
    if (!a.__timeline || a.__startTime === null) return null;
    var tt = timelineTime(a.__timeline);
    if (tt === null) return null;
    var v = (tt - a.__startTime) * a.__rate;
    return v === 0 ? 0 : v;
  }
  function effectEnd(a) { return a.__effect ? endTime(a.__effect.__timing) : 0; }
  function applyPendingRate(a) { if (a.__pendingRate !== null) { a.__rate = a.__pendingRate; a.__pendingRate = null; } }
  function playState(a) {
    var ct = currentTime(a);
    if (ct === null && a.__startTime === null && !a.__playTask && !a.__pauseTask) return 'idle';
    if (a.__pauseTask || (a.__startTime === null && !a.__playTask)) return 'paused';
    var rate = effectiveRate(a);
    if (ct !== null && ((rate > 0 && ct >= effectEnd(a)) || (rate < 0 && ct <= 0))) return 'finished';
    return 'running';
  }
  function resetReady(a) {
    if (a.__ready.done) a.__ready = newPromise();
  }
  function silentSetCurrent(a, seek) {
    if (seek === null) {
      if (currentTime(a) !== null) throw new TypeError("Failed to set the 'currentTime' property on 'Animation': currentTime may not be changed from resolved to unresolved");
      return;
    }
    if (a.__holdTime !== null || a.__startTime === null || !tlActive(a) || a.__rate === 0) a.__holdTime = seek;
    else a.__startTime = timelineTime(a.__timeline) - seek / a.__rate;
    if (!tlActive(a)) a.__startTime = null;
    a.__prevTime = null;
  }
  function setCurrent(a, seek) {
    silentSetCurrent(a, seek);
    if (a.__pauseTask) {
      a.__holdTime = seek;
      applyPendingRate(a);
      a.__startTime = null;
      a.__pauseTask = false;
      resolveReady(a);
    }
    updateFinished(a, true, false);
    dirty();
  }
  function resolveReady(a) {
    if (!a.__ready.done) { a.__ready.done = true; a.__ready.resolve(a); }
  }
  function setStart(a, t) {
    var tt = timelineTime(a.__timeline);
    if (tt === null && t !== null) a.__holdTime = null;
    applyPendingRate(a);
    var prev = currentTime(a);
    a.__startTime = t;
    if (t !== null) { if (a.__rate !== 0) a.__holdTime = null; else a.__holdTime = prev; }
    else a.__holdTime = prev;
    if (a.__playTask || a.__pauseTask) { a.__playTask = false; a.__pauseTask = false; resolveReady(a); }
    updateFinished(a, true, false);
    dirty();
  }
  function play(a, autoRewind) {
    var abortedPause = a.__pauseTask;
    var hasPendingReady = false;
    var seek = null;
    var rate = effectiveRate(a);
    var ct = currentTime(a), end = effectEnd(a);
    if (rate > 0 && autoRewind && (ct === null || ct < 0 || ct >= end)) seek = 0;
    else if (rate < 0 && autoRewind && (ct === null || ct <= 0 || ct > end)) {
      if (end === Infinity) throw domError("Failed to execute 'play' on 'Animation': Cannot play reversed Animation with infinite target effect end.", 'InvalidStateError');
      seek = end;
    } else if (rate === 0 && ct === null) seek = 0;
    if (seek !== null) { a.__startTime = null; a.__holdTime = seek; }
    if (a.__holdTime !== null) a.__startTime = null;
    if (a.__playTask || a.__pauseTask) { a.__pauseTask = false; a.__playTask = false; hasPendingReady = true; }
    if (a.__holdTime === null && seek === null && !abortedPause && a.__pendingRate === null) return;
    if (!hasPendingReady) resetReady(a);
    a.__playTask = true;
    if (animations.indexOf(a) < 0) animations.push(a);
    a.__replaceState = 'active';
    updateFinished(a, false, false);
    schedule();
    dirty();
  }
  function pause(a) {
    if (a.__pauseTask) return;
    var seek = null;
    if (currentTime(a) === null) {
      if (a.__rate >= 0) seek = 0;
      else {
        if (effectEnd(a) === Infinity) throw domError("Failed to execute 'pause' on 'Animation': Cannot pause, Animation has infinite target effect end.", 'InvalidStateError');
        seek = effectEnd(a);
      }
    }
    if (seek !== null) a.__holdTime = seek;
    var hasPendingReady = false;
    if (a.__playTask) { a.__playTask = false; hasPendingReady = true; }
    if (!hasPendingReady) resetReady(a);
    a.__pauseTask = true;
    updateFinished(a, false, false);
    schedule();
    dirty();
  }
  function runPending(a, readyTime) {
    if (a.__playTask) {
      a.__playTask = false;
      if (a.__holdTime !== null) {
        applyPendingRate(a);
        a.__startTime = a.__rate === 0 ? readyTime : readyTime - a.__holdTime / a.__rate;
        if (a.__rate !== 0) a.__holdTime = null;
      } else if (a.__startTime !== null && a.__pendingRate !== null) {
        var match = (readyTime - a.__startTime) * a.__rate;
        applyPendingRate(a);
        if (a.__rate === 0) a.__holdTime = match;
        else a.__startTime = readyTime - match / a.__rate;
      }
      resolveReady(a);
      updateFinished(a, false, false);
    }
    if (a.__pauseTask) {
      a.__pauseTask = false;
      if (a.__startTime !== null && a.__holdTime === null) a.__holdTime = (readyTime - a.__startTime) * a.__rate;
      applyPendingRate(a);
      a.__startTime = null;
      resolveReady(a);
      updateFinished(a, false, false);
    }
  }
  function updateFinished(a, didSeek, sync) {
    var unconstrained = null;
    if (a.__startTime !== null && a.__timeline) {
      var tt = timelineTime(a.__timeline);
      unconstrained = didSeek || tt === null ? currentTime(a) : (tt - a.__startTime) * a.__rate;
    } else unconstrained = currentTime(a);
    if (unconstrained !== null && a.__startTime !== null && !a.__playTask) {
      var end = effectEnd(a);
      if (a.__rate > 0 && unconstrained >= end) {
        a.__holdTime = didSeek ? unconstrained : (a.__prevTime === null ? end : Math.max(a.__prevTime, end));
      } else if (a.__rate < 0 && unconstrained <= 0) {
        a.__holdTime = didSeek ? unconstrained : (a.__prevTime === null ? 0 : Math.min(a.__prevTime, 0));
      } else if (a.__rate !== 0 && tlActive(a)) {
        if (didSeek && a.__holdTime !== null) a.__startTime = timelineTime(a.__timeline) - a.__holdTime / a.__rate;
        a.__holdTime = null;
      }
    }
    a.__prevTime = currentTime(a);
    var finished = playState(a) === 'finished';
    if (finished && !a.__finishNotified) {
      a.__finishNotified = true;
      var notify = function () {
        if (playState(a) !== 'finished') { a.__finishNotified = false; return; }
        var fp = a.__finished;
        if (!fp.done) { fp.done = true; fp.resolve(a); }
        queueEvent(a, 'finish', currentTime(a), timelineTime(a.__timeline));
      };
      if (sync) notify(); else Promise.resolve().then(notify);
    }
    if (!finished) {
      a.__finishNotified = false;
      if (a.__finished.done) a.__finished = newPromise();
    }
  }
  function cancel(a) {
    if (playState(a) !== 'idle') {
      if (a.__playTask || a.__pauseTask) { a.__playTask = false; a.__pauseTask = false; }
      if (!a.__ready.done) { a.__ready.done = true; a.__ready.reject(domError('The user aborted a request.', 'AbortError')); }
      a.__ready = newPromise(); a.__ready.done = true; a.__ready.resolve(a);
      if (!a.__finished.done) { a.__finished.done = true; a.__finished.reject(domError('The user aborted a request.', 'AbortError')); }
      a.__finished = newPromise();
      queueEvent(a, 'cancel', null, timelineTime(a.__timeline));
    }
    a.__holdTime = null;
    a.__startTime = null;
    a.__finishNotified = false;
    dirty();
  }
  function finish(a) {
    var rate = effectiveRate(a);
    var end = effectEnd(a);
    if (rate === 0 || (rate > 0 && end === Infinity)) throw domError("Failed to execute 'finish' on 'Animation': Cannot finish Animation with a playbackRate of 0 or an infinite target effect end.", 'InvalidStateError');
    applyPendingRate(a);
    var limit = a.__rate > 0 ? end : 0;
    silentSetCurrent(a, limit);
    if (a.__startTime === null && tlActive(a)) a.__startTime = timelineTime(a.__timeline) - limit / a.__rate;
    if (a.__pauseTask && a.__startTime !== null) { a.__holdTime = null; a.__pauseTask = false; resolveReady(a); }
    if (a.__playTask && a.__startTime !== null) { a.__playTask = false; resolveReady(a); }
    updateFinished(a, true, true);
    dirty();
  }
  function queueEvent(a, type, ct, tt) {
    a.__events.push([type, ct, tt]);
    schedule();
  }
  // Animation events of this frame, sorted by their scheduled time, then by
  // composite order (Web Animations §4.4.18.2 "update animations and send
  // events").
  var playbackSeq = 0;
  function dispatchEvents() {
    var list = [];
    animations.forEach(function (a) {
      while (a.__events.length) {
        var e = a.__events.shift();
        list.push({ t: e[2] === null || e[2] === undefined ? -Infinity : e[2], a: a, n: ++playbackSeq, e: e });
      }
    });
    if (h.cssEvents) list = list.concat(h.cssEvents());
    list.sort(function (x, y) {
      if (x.t !== y.t) return x.t < y.t ? -1 : 1;
      var c = compositeOrder(x.a, y.a);
      return c !== 0 ? c : x.n - y.n;
    });
    list.forEach(function (it) {
      try {
        if (it.run) { it.run(); return; }
        var ev = playbackEvent(it.e[0], { currentTime: it.e[1], timelineTime: it.e[2] });
        it.a.dispatchEvent(ev);
      } catch (x) { if (g.reportError) g.reportError(x); }
    });
  }
  function AnimationPlaybackEvent(type, init) {
    if (arguments.length < 1) throw new TypeError("Failed to construct 'AnimationPlaybackEvent': 1 argument required, but only 0 present.");
    var e = new g.Event(type, init);
    init = init || {};
    Object.defineProperty(e, 'currentTime', { value: init.currentTime === undefined ? null : init.currentTime, enumerable: true, configurable: true });
    Object.defineProperty(e, 'timelineTime', { value: init.timelineTime === undefined ? null : init.timelineTime, enumerable: true, configurable: true });
    try { Object.setPrototypeOf(e, AnimationPlaybackEvent.prototype); } catch (x) {}
    return e;
  }
  AnimationPlaybackEvent.prototype = Object.create(g.Event.prototype);
  Object.defineProperty(AnimationPlaybackEvent.prototype, 'constructor', { value: AnimationPlaybackEvent, writable: true, configurable: true, enumerable: false });
  g.AnimationPlaybackEvent = AnimationPlaybackEvent;
  function playbackEvent(type, init) { return new AnimationPlaybackEvent(type, init); }

  function prop(name, get, set) {
    Object.defineProperty(Animation.prototype, name, { configurable: true, enumerable: true, get: get, set: set });
  }
  prop('id', function () { return this.__id; }, function (v) { this.__id = String(v); });
  prop('effect', function () { return this.__effect; }, function (v) {
    if (this.__css && v !== this.__effect && v) (v.__overrides || (v.__overrides = {})).effect = true;
    setEffect(this, v);
  });
  prop('timeline', function () { return this.__timeline; }, function (v) {
    var old = this.__timeline;
    if (v === undefined) v = null;
    if (v === old) return;
    if (this.__css) this.__cssCancelTime = activeTimeNow(this);
    this.__timeline = v;
    // A resolved start time keeps the animation on the (new) timeline.
    if (this.__startTime !== null) this.__holdTime = null;
    updateFinished(this, false, false);
    dirty();
  });
  prop('startTime', function () { return this.__startTime; }, function (v) { setStart(this, v === null ? null : Number(v)); });
  prop('currentTime', function () { return currentTime(this); }, function (v) { setCurrent(this, v === null ? null : Number(v)); });
  prop('playbackRate', function () { return this.__rate; }, function (v) {
    this.__pendingRate = null;
    var prev = currentTime(this);
    this.__rate = Number(v);
    if (prev !== null) setCurrent(this, prev);
    dirty();
  });
  prop('playState', function () { return playState(this); });
  // Web Animations 2 §4.4.18: current time over the effect end, in [0, 1].
  prop('overallProgress', function () {
    var ct = currentTime(this);
    if (!this.__effect || ct === null) return null;
    var end = effectEnd(this);
    if (end === 0) return ct < 0 ? 0 : 1;
    if (end === Infinity) return 0;
    return Math.min(Math.max(ct / end, 0), 1);
  });
  prop('pending', function () { return this.__playTask || this.__pauseTask; });
  prop('replaceState', function () { return this.__replaceState; });
  prop('ready', function () { return this.__ready.p; });
  prop('finished', function () { return this.__finished.p; });
  ['onfinish', 'oncancel', 'onremove'].forEach(function (h) {
    prop(h, function () { return this.__handlers[h] || null; }, function (f) {
      var t = h.slice(2), old = this.__handlers[h];
      if (old) this.removeEventListener(t, old);
      this.__handlers[h] = typeof f === 'function' ? f : null;
      if (typeof f === 'function') this.addEventListener(t, f);
    });
  });
  Animation.prototype.play = function () { play(this, true); };
  Animation.prototype.pause = function () { pause(this); };
  Animation.prototype.cancel = function () { cancel(this); };
  Animation.prototype.finish = function () { finish(this); };
  Animation.prototype.reverse = function () {
    var orig = this.__pendingRate;
    if (!tlActive(this)) throw domError("Failed to execute 'reverse' on 'Animation': Cannot reverse an animation with no active timeline", 'InvalidStateError');
    this.__pendingRate = -effectiveRate(this) || 0;
    try { play(this, true); } catch (e) { this.__pendingRate = orig; throw e; }
  };
  Animation.prototype.updatePlaybackRate = function (rate) {
    var prevState = playState(this);
    this.__pendingRate = Number(rate);
    if (this.__playTask || this.__pauseTask) return;
    if (prevState === 'idle' || prevState === 'paused' || currentTime(this) === null) { applyPendingRate(this); }
    else if (prevState === 'finished') {
      var tt = timelineTime(this.__timeline);
      var unconstrained = tt === null || this.__startTime === null ? null : (tt - this.__startTime) * this.__rate;
      if (this.__pendingRate === 0) this.__startTime = tt;
      else if (unconstrained !== null) this.__startTime = tt - unconstrained / this.__pendingRate;
      applyPendingRate(this);
      updateFinished(this, false, false);
    } else play(this, false);
    dirty();
  };
  Animation.prototype.persist = function () { this.__replaceState = 'persisted'; };
  // Web Animations 1 §4.5.21: the effect stack up to and including this
  // animation, with this effect evaluated as if it filled both ways.
  Animation.prototype.commitStyles = function () {
    var eff = this.__effect;
    if (!eff || !eff.__target) return;
    var el = eff.__target;
    if (eff.__pseudo) throw domError("Failed to execute 'commitStyles' on 'Animation': Animation target is a pseudo-element.", 'NoModificationAllowedError');
    var style = null;
    try { style = el.style; } catch (e) {}
    if (!style || typeof style.setProperty !== 'function' || !(el.namespaceURI === 'http://www.w3.org/1999/xhtml' || el.namespaceURI === 'http://www.w3.org/2000/svg' || el.namespaceURI === 'http://www.w3.org/1998/Math/MathML')) {
      throw domError("Failed to execute 'commitStyles' on 'Animation': Target element does not have a style attribute.", 'NoModificationAllowedError');
    }
    if (!isRendered(el)) throw domError("Failed to execute 'commitStyles' on 'Animation': Target element is not rendered.", 'InvalidStateError');
    if (h.cssUpdate) { try { h.cssUpdate(); } catch (e) {} }
    isDirty = true;
    flush();
    var vals = sampleTarget(el, this, true);
    for (var p in vals) { try { style.setProperty(p, vals[p]); } catch (e) {} }
  };
  function isRendered(el) {
    if (!el.isConnected) return false;
    for (var n = el; n && n.nodeType === 1; n = n.parentElement) {
      var d = '';
      try { d = g.getComputedStyle(n).display; } catch (e) {}
      if (d === 'none') return false;
    }
    return true;
  }
  if (typeof Symbol === 'function' && Symbol.toStringTag) {
    try { Object.defineProperty(Animation.prototype, Symbol.toStringTag, { value: 'Animation', configurable: true }); } catch (e) {}
    try { Object.defineProperty(KeyframeEffect.prototype, Symbol.toStringTag, { value: 'KeyframeEffect', configurable: true }); } catch (e) {}
  }

  // ------------------------------------------------------------------
  // Frames, sampling and the host
  // ------------------------------------------------------------------
  var isDirty = false, scheduled = false;
  var lastFrame = -1;
  var applied = []; // elements that have animated values in the host
  function dirty() { isDirty = true; schedule(); }
  function hasWork() {
    for (var i = 0; i < animations.length; i++) {
      var a = animations[i];
      var s = playState(a);
      if (a.__playTask || a.__pauseTask || s === 'running' || a.__events.length) return true;
    }
    return !!(h.cssPending && h.cssPending());
  }
  var rawRaf = h.raf || g.requestAnimationFrame;
  var rawCancel = g.cancelAnimationFrame;
  // One timestamp per frame: callbacks requested before a frame began and
  // run right after its start share its time (HTML §8.10: all callbacks of a
  // frame get the same timestamp); later ones start the next frame.
  var frameStart = null;
  function stamp(t, requested) {
    if (frameStart !== null && requested < frameStart && t - frameStart < 8 && t >= frameStart) return frameStart;
    frameStart = t;
    return t;
  }
  // The frame loop runs while animations need frames and a few frames
  // longer, so the timeline time keeps moving as in a rendered document.
  var looping = false, idleFrames = 0;
  var IDLE_FRAMES = 10;
  function startLoop() {
    idleFrames = 0;
    if (!looping) schedule();
  }
  function schedule() {
    idleFrames = 0;
    looping = true;
    nextFrame();
  }
  function nextFrame() {
    if (scheduled || typeof rawRaf !== 'function') return;
    scheduled = true;
    var requested = now();
    rawRaf.call(g, function (t) {
      scheduled = false;
      frame(stamp(t, requested));
      if (hasWork()) idleFrames = 0;
      else idleFrames++;
      if (idleFrames >= IDLE_FRAMES) looping = false;
      else nextFrame();
    });
  }
  // The page's own callbacks see the frame's time, after the animations of
  // that frame were updated.
  if (typeof rawRaf === 'function') {
    var pageRaf = function requestAnimationFrame(cb) {
      if (typeof cb !== 'function') return rawRaf.apply(g, arguments);
      var requested = now();
      startLoop();
      return rawRaf.call(g, function (t) {
        var ft = stamp(t, requested);
        frame(ft);
        try { return cb.call(this, ft); } finally {
          // Style changes of the callback count in this frame (HTML "update
          // the rendering": style after the animation frame callbacks).
          if (h.cssUpdate) { try { h.cssUpdate(); } catch (e) {} }
          flush();
        }
      });
    };
    try { Object.defineProperty(g, 'requestAnimationFrame', { value: pageRaf, writable: true, configurable: true, enumerable: true }); } catch (e) { g.requestAnimationFrame = pageRaf; }
    if (typeof rawCancel === 'function') g.cancelAnimationFrame = rawCancel;
  }
  // The start of a frame: the timeline moves, pending tasks resolve, values
  // and events are updated ("update animations and send events").
  function frame(t) {
    if (t === lastFrame) return;
    stats.frames++;
    lastFrame = t;
    frameTime = t;
    looping = true;
    if (h.cssUpdate) { try { h.cssUpdate(); } catch (e) {} }
    var tt = timelineTime(docTimeline);
    animations.slice().forEach(function (a) {
      if ((a.__playTask || a.__pauseTask) && tt !== null && a.__timeline) {
        var rt = timelineTime(a.__timeline);
        if (rt !== null) runPending(a, rt);
      }
      updateFinished(a, false, false);
    });
    removeReplaced();
    isDirty = true;
    flush();
    dispatchEvents();
    if (h.cssPurge) h.cssPurge();
  }
  h.frame = frame;
  function removeReplaced() {
    // Finished, filling, replaceable animations that are fully replaced are
    // removed (Web Animations §5.5).
    var byTarget = new Map();
    animations.forEach(function (a) {
      if (a.__replaceState !== 'active' || !a.__effect || !a.__effect.__target || a.__css) return;
      var s = playState(a);
      if (s !== 'finished' || a.__playTask || a.__pauseTask) return;
      if (!inEffect(a)) return;
      // One effect stack per target and pseudo-element.
      var tgt = a.__effect.__target, pk = a.__effect.__pseudo || '';
      if (!byTarget.has(tgt)) byTarget.set(tgt, {});
      var per = byTarget.get(tgt);
      (per[pk] || (per[pk] = [])).push(a);
    });
    var stacks = [];
    byTarget.forEach(function (per) { for (var k in per) stacks.push(per[k]); });
    stacks.forEach(function (list) {
      if (list.length < 2) return;
      var covered = {};
      for (var i = list.length - 1; i >= 0; i--) {
        var a = list[i];
        var props = effectProps(a.__effect);
        var all = props.length > 0 && props.every(function (p) { return covered[p]; });
        if (all) {
          a.__replaceState = 'removed';
          queueEvent(a, 'remove', currentTime(a), timelineTime(a.__timeline));
        }
        props.forEach(function (p) { covered[p] = true; });
      }
    });
  }
  function effectProps(eff) {
    var seen = {}, out = [];
    eff.__frames.forEach(function (f) { for (var p in f.props) if (!seen[p]) { seen[p] = true; out.push(p); } });
    return out;
  }
  function inEffect(a) {
    var eff = a.__effect;
    if (!eff) return false;
    var c = computed(eff);
    return c.progress !== null;
  }
  function relevant(a) {
    if (!a.__effect || a.__replaceState === 'removed') return false;
    var s = playState(a);
    if (s === 'idle') return false;
    var c = computed(a.__effect);
    if (c.progress !== null) return true;
    // Current: before or active phase.
    return c.__phase === 'before' || c.__phase === 'active' || a.__playTask || a.__pauseTask;
  }
  // The values of one target, in composite order.
  // The values of the effect stack of `el` (up to and including `upTo`,
  // which fills both ways when `fillBoth`).
  function sampleTarget(el, upTo, fillBoth, kinds, skipTransitions, pseudo) {
    var out = {};
    if (pseudo === undefined && upTo && upTo.__effect) pseudo = upTo.__effect.__pseudo;
    pseudo = pseudo || null;
    var list = animations.filter(function (a) {
      if (skipTransitions && a.__cssClass === 0 && a.__owner) return false;
      return a.__effect && a.__effect.__target === el && (a.__effect.__pseudo || null) === pseudo && (a.__replaceState !== 'removed' || a === upTo);
    });
    list.sort(compositeOrder);
    for (var i = 0; i < list.length; i++) {
      var a = list[i], eff = a.__effect, c;
      if (a === upTo && fillBoth) {
        var fill = eff.__timing.fill;
        eff.__timing.fill = 'both';
        try { c = computed(eff); } finally { eff.__timing.fill = fill; }
      } else c = computed(eff);
      if (c.progress !== null) {
        var props = effectProps(eff);
        for (var j = 0; j < props.length; j++) {
          var v = sampleProp(el, eff, props[j], c.progress, out[props[j]], c);
          if (v !== null && v !== undefined) {
            out[props[j]] = v;
            if (kinds) kinds[props[j]] = a.__cssClass === 0 && a.__owner ? 't' : 'a';
          }
        }
      }
      if (a === upTo) break;
    }
    return out;
  }
  // CSS transitions, then CSS animations (each by the tree order of their
  // owning element), then the other animations in creation order (CSS
  // Animations 2 §4.5, CSS Transitions 2 §4.4).
  var PSEUDO_RANK = { '::marker': 1, '::before': 2, '::after': 3 };
  function pseudoRank(a) { var ps = a.__effect && a.__effect.__pseudo; return ps ? (PSEUDO_RANK[ps] || 4) : 0; }
  function ownedClass(a) { return a.__owner && a.__cssClass !== undefined ? a.__cssClass : 2; }
  function compositeOrder(x, y) {
    var cx = ownedClass(x), cy = ownedClass(y);
    if (cx !== cy) return cx - cy;
    if (cx < 2) {
      if (x.__owner !== y.__owner && x.__owner.compareDocumentPosition) {
        var pos = x.__owner.compareDocumentPosition(y.__owner);
        if (pos & 4) return -1;
        if (pos & 2) return 1;
      }
      var px = pseudoRank(x), py = pseudoRank(y);
      if (px !== py) return px - py;
      if (x.__cssOrder !== y.__cssOrder) return x.__cssOrder - y.__cssOrder;
      if (cx === 0 && x.__cssProp !== y.__cssProp) return x.__cssProp < y.__cssProp ? -1 : 1;
    }
    return x.__seq - y.__seq;
  }
  var baseCache = new Map();
  function baseValue(el, p, pseudo) {
    var m = baseCache.get(el);
    if (!m) { m = {}; baseCache.set(el, m); }
    var key = pseudo ? pseudo + ' ' + p : p;
    if (!(key in m)) m[key] = HOST_BASE ? (HOST_BASE(el, p, pseudo || null) || '') : '';
    return m[key];
  }
  var valueCache = new Map();
  var UNCLAMPED = { 'opacity': 1, 'fill-opacity': 1, 'stroke-opacity': 1, 'flood-opacity': 1, 'stop-opacity': 1 };
  function computedValue(el, p, v, pseudo) {
    if (v === null || v === undefined || v === '') return baseValue(el, p, pseudo);
    if (UNCLAMPED[p] && /^\s*[-+]?(\d+\.?\d*|\.\d+)(e[-+]?\d+)?\s*$/i.test(String(v))) return String(Number(v));
    if (!HOST_VALUE) return v;
    var m = valueCache.get(el);
    if (!m) { m = {}; valueCache.set(el, m); }
    var key = (pseudo || '') + ' ' + p + '\u0000' + v;
    if (key in m) return m[key];
    var r = HOST_VALUE(el, p, v, pseudo || null);
    return (m[key] = r === null || r === undefined ? v : r);
  }
  // The effect value of one property (Web Animations 1 §5.4.4): the
  // interval endpoints around the progress, iteration composite and
  // composite operation applied to them, then interpolated.
  function sampleProp(el, eff, p, progress, underlying, c) {
    var frames = eff.__frames.filter(function (f) { return p in f.props; });
    if (!frames.length) return null;
    var under = underlying !== undefined ? underlying : baseValue(el, p, eff.__pseudo);
    var pts = frames.map(function (f) {
      return { off: f.computedOffset, v: computedValue(el, p, f.props[p], eff.__pseudo), fn: f.__fn || NAMED.linear[0], comp: !f.composite || f.composite === 'auto' ? eff.__composite : f.composite };
    });
    // Implicit keyframes hold the neutral value: the underlying value.
    if (pts[0].off !== 0) pts.unshift({ off: 0, v: under, fn: NAMED.linear[0], comp: 'replace', neutral: true });
    if (pts[pts.length - 1].off !== 1) pts.push({ off: 1, v: under, fn: NAMED.linear[0], comp: 'replace', neutral: true });
    var zeros = 0, ones = 0, i;
    for (i = 0; i < pts.length; i++) { if (pts[i].off === 0) zeros++; if (pts[i].off === 1) ones++; }
    var ends;
    if (progress < 0 && zeros > 1) ends = [pts[0]];
    else if (progress >= 1 && ones > 1) ends = [pts[pts.length - 1]];
    else {
      var s = null, e = null;
      for (i = 0; i < pts.length; i++) if (pts[i].off <= progress && pts[i].off < 1) s = pts[i];
      if (!s) for (i = 0; i < pts.length; i++) if (pts[i].off === 0) s = pts[i];
      for (i = 0; i < pts.length; i++) if (pts[i].off >= progress && pts[i].off > 0) { e = pts[i]; break; }
      if (!e) for (i = 0; i < pts.length; i++) if (pts[i].off === 1) { e = pts[i]; break; }
      ends = [s || pts[0], e || pts[pts.length - 1]];
    }
    var iter = c.currentIteration;
    var accumulate = eff.__iterationComposite === 'accumulate' && iter > 0 && isFinite(iter);
    var finalV = pts[pts.length - 1].v;
    var vals = ends.map(function (k) {
      var v = k.v;
      if (accumulate) {
        var total = finalV;
        for (var n = 1; n < Math.min(iter, 64); n++) total = combine(p, total, finalV, 'accumulate');
        v = combine(p, total, v, 'accumulate');
      }
      if (!k.neutral && k.comp !== 'replace') v = combine(p, under, v, k.comp);
      return v;
    });
    if (ends.length === 1) return vals[0];
    var span = ends[1].off - ends[0].off;
    var dist = span === 0 ? 0 : (progress - ends[0].off) / span;
    if (span === 0 && ends[0] === ends[1]) return vals[0];
    var eased = ends[0].fn(dist, !!c.__before);
    return lerp(p, vals[0], vals[1], eased);
  }
  var HOST_COMBINE = h.combine;
  function combine(p, a, b, op) {
    if (!HOST_COMBINE) return b;
    var r = HOST_COMBINE(p, String(a), String(b), op === 'accumulate');
    return r === null || r === undefined ? b : r;
  }
  function lerp(p, a, b, t) {
    if (!HOST_LERP) return t < 0.5 ? a : b;
    var r = HOST_LERP(p, String(a), String(b), t);
    return r === null || r === undefined ? (t < 0.5 ? a : b) : r;
  }
  // All targets: send changed values to the host.
  function flush() {
    if (!isDirty) return;
    stats.flush++;
    var tf = now();
    try { flushInner(); } finally { stats.flushMs += now() - tf; }
  }
  function flushInner() {
    isDirty = false;
    baseCache = new Map();
    valueCache = new Map();
    // One effect stack per element and pseudo-element.
    var targets = [];
    function sameTarget(x, y) { return x.el === y.el && x.ps === y.ps; }
    function indexOfTarget(list, x) { for (var i = 0; i < list.length; i++) if (sameTarget(list[i], x)) return i; return -1; }
    animations.forEach(function (a) {
      if (!a.__effect || !a.__effect.__target) return;
      var key = { el: a.__effect.__target, ps: a.__effect.__pseudo || null };
      if (indexOfTarget(targets, key) < 0) targets.push(key);
    });
    var now2 = [];
    targets.forEach(function (tg) {
      var kinds = {};
      var vals = sampleTarget(tg.el, null, false, kinds, false, tg.ps);
      var arr = [];
      // A transition's value is marked: it wins over `!important`
      // declarations (the transitions origin, CSS Cascade 5 §6.1).
      for (var p in vals) { arr.push(p, String(vals[p]) + (kinds[p] === 't' ? ' !important' : '')); }
      if (HOST_SET) { HOST_SET(tg.el, arr, tg.ps); stats.sets++; }
      if (arr.length) now2.push(tg);
    });
    applied.forEach(function (tg) {
      if (indexOfTarget(now2, tg) < 0 && indexOfTarget(targets, tg) < 0 && HOST_SET) HOST_SET(tg.el, [], tg.ps);
    });
    applied = now2;
  }
  function forget(el) {
    if (HOST_SET) HOST_SET(el, []);
    for (var i = applied.length - 1; i >= 0; i--) {
      if (applied[i].el !== el) continue;
      if (applied[i].ps && HOST_SET) HOST_SET(el, [], applied[i].ps);
      applied.splice(i, 1);
    }
    dirty();
  }
  h.flush = function () {
    stats.hook = (stats.hook || 0) + 1;
    if (h.cssUpdate) { try { h.cssUpdate(); } catch (e) {} }
    flush();
  };
  h.intern = {
    animations: animations, play: play, cancel: cancel, pause: pause, dirty: dirty, computed: computed, currentTime: currentTime,
    playState: playState, timelineTime: timelineTime, docTimeline: docTimeline, parseEasing: parseEasing, Animation: Animation,
    KeyframeEffect: KeyframeEffect, defaultTiming: defaultTiming, normTiming: normTiming, setEffect: setEffect, relevant: relevant,
    schedule: schedule, queueEvent: queueEvent, kebab: kebab, camel: camel, effectEnd: effectEnd, frameTimeSet: function (t) { frameTime = t; }
  };

  // ------------------------------------------------------------------
  // Element.animate, getAnimations
  // ------------------------------------------------------------------
  if (E) {
    Object.defineProperty(E, 'animate', {
      configurable: true, writable: true,
      value: function animate(keyframes, options) {
        var eff = new KeyframeEffect(this, keyframes === undefined ? null : keyframes, options);
        var a = new Animation(eff, docTimeline);
        if (options && typeof options === 'object' && options.id !== undefined) a.id = options.id;
        if (options && typeof options === 'object' && options.timeline !== undefined) a.__timeline = options.timeline;
        a.play();
        return a;
      }
    });
  }
  // One function for elements, documents and shadow roots: the engine finds
  // a method of `Document.prototype` before the one of `Element.prototype`.
  function getAnimations(opts) {
    if (h.cssUpdate) { try { h.cssUpdate(); } catch (e) {} }
    var self = this;
    var type = self && self.nodeType;
    var sub = type === 1 && opts && opts.subtree;
    return animations.filter(function (a) {
      var t = a.__effect && a.__effect.__target;
      if (!t || !relevant(a)) return false;
      if (type === 1) return sub ? (t === self || !!(self.contains && self.contains(t))) : (t === self && !a.__effect.__pseudo);
      if (type === 9) return t.isConnected && t.ownerDocument === self && (!t.getRootNode || t.getRootNode() === self);
      if (type === 11) return !!(t.getRootNode && t.getRootNode() === self);
      return false;
    }).sort(compositeOrder);
  }
  [E, D, g.ShadowRoot && g.ShadowRoot.prototype].forEach(function (P) {
    if (P) Object.defineProperty(P, 'getAnimations', { configurable: true, writable: true, enumerable: true, value: getAnimations });
  });
  try { if (g.document) delete g.document.getAnimations; } catch (e) {}
  // ------------------------------------------------------------------
  // CSS Animations (CSS Animations 2 §4) and CSS Transitions (CSS
  // Transitions 2 §4)
  // ------------------------------------------------------------------
  var HOST_CSS = h.css, HOST_BASES = h.bases, HOST_CAN_LERP = h.canLerp, HOST_LONGHANDS = h.longhands, HOST_PREV = h.prev, HOST_START = h.start;
  var TOKEN = {};
  var cssGen = -1;
  var removedBoxes = new WeakSet();
  var cssStates = new Map(); // element -> { anims: [CSSAnimation], trans: {prop: CSSTransition}, snap: {prop: value} }
  var treeIndex = new WeakMap();
  var cssEventQueue = [];
  var transitionGeneration = 0;
  var eventSeq = 0;
  // The properties `transition-property: all` compares (the common
  // animatable longhands; a full list would cost a cascade pass per name).
  var ALL_PROPS = ['color', 'background-color', 'opacity', 'visibility', 'transform', 'translate', 'rotate', 'scale',
    'transform-origin', 'width', 'height', 'min-width', 'min-height', 'max-width', 'max-height',
    'margin-top', 'margin-right', 'margin-bottom', 'margin-left', 'padding-top', 'padding-right', 'padding-bottom', 'padding-left',
    'top', 'right', 'bottom', 'left', 'border-top-width', 'border-right-width', 'border-bottom-width', 'border-left-width',
    'border-top-color', 'border-right-color', 'border-bottom-color', 'border-left-color',
    'border-top-left-radius', 'border-top-right-radius', 'border-bottom-right-radius', 'border-bottom-left-radius',
    'outline-color', 'outline-width', 'outline-offset', 'box-shadow', 'text-shadow', 'font-size', 'font-weight', 'line-height',
    'letter-spacing', 'word-spacing', 'text-indent', 'vertical-align', 'z-index', 'flex-grow', 'flex-shrink', 'flex-basis',
    'row-gap', 'column-gap', 'background-position-x', 'background-position-y', 'background-size', 'clip-path', 'filter',
    'backdrop-filter', 'fill', 'stroke', 'fill-opacity', 'stroke-opacity', 'stroke-width', 'text-decoration-color',
    'caret-color', 'column-width', 'column-count', 'order', 'perspective', 'perspective-origin', 'object-position'];
  var DISCRETE_PROPS = ['display', 'float', 'clear', 'position', 'overflow-x', 'overflow-y', 'text-align', 'white-space-collapse',
    'text-wrap-mode', 'font-style', 'text-transform', 'content-visibility', 'pointer-events', 'cursor', 'list-style-type', 'flex-direction',
    'flex-wrap', 'justify-content', 'align-items', 'align-self', 'box-sizing', 'overlay'];
  var longhandCache = {};
  function longhandsOf(p) {
    if (p in longhandCache) return longhandCache[p];
    var r = HOST_LONGHANDS ? HOST_LONGHANDS(p) : [p];
    return (longhandCache[p] = Array.isArray(r) ? r : []);
  }
  function splitList(s) {
    var out = [], depth = 0, cur = '', q = null;
    s = String(s === null || s === undefined ? '' : s);
    for (var i = 0; i < s.length; i++) {
      var c = s.charAt(i);
      if (q) {
        cur += c;
        if (c === '\\' && i + 1 < s.length) cur += s.charAt(++i);
        else if (c === q) q = null;
        continue;
      }
      if (c === '"' || c === "'") { q = c; cur += c; continue; }
      if (c === '(') depth++;
      else if (c === ')') depth = Math.max(0, depth - 1);
      if (c === ',' && depth === 0) { out.push(cur.trim()); cur = ''; continue; }
      cur += c;
    }
    out.push(cur.trim());
    return out;
  }
  function timeMs(s) {
    var m = /^([-+]?(?:\d+\.?\d*|\.\d+)(?:e[-+]?\d+)?)(ms|s)$/i.exec(String(s).trim());
    if (!m) return 0;
    var v = parseFloat(m[1]);
    return m[2].toLowerCase() === 's' ? v * 1000 : v;
  }
  function pick(list, i) { return list.length ? list[i % list.length] : ''; }
  function unquote(n) {
    n = String(n).trim();
    if (/^(["']).*\1$/.test(n)) return n.slice(1, -1);
    return n;
  }

  // @keyframes rules by name, read through the CSSOM (the last one wins).
  var keyframesMap = null, keyframesGen = -2;
  function keyframesRules() {
    if (keyframesMap && keyframesGen === cssGen) return keyframesMap;
    stats.keyframes++;
    var tk = now();
    try { return keyframesRulesInner(); } finally { stats.keyframesMs += now() - tk; }
  }
  function keyframesRulesInner() {
    var map = {};
    function walk(rules) {
      if (!rules) return;
      for (var i = 0; i < rules.length; i++) {
        var r = rules[i];
        if (!r) continue;
        if (r.type === 7) { map[r.name] = r; continue; }
        if (r.type === 4) {
          var ok = true;
          try { ok = !r.media || !r.media.mediaText || g.matchMedia(r.media.mediaText).matches; } catch (e) {}
          if (!ok) continue;
        } else if (r.type === 12) {
          var ok2 = true;
          try { ok2 = g.CSS.supports(r.conditionText); } catch (e) {}
          if (!ok2) continue;
        }
        var inner = null;
        try { inner = r.cssRules; } catch (e) {}
        if (inner && r.type !== 7) walk(inner);
      }
    }
    try {
      var sheets = g.document.styleSheets;
      for (var s = 0; s < sheets.length; s++) {
        var sh = sheets[s];
        if (!sh || sh.disabled) continue;
        var rules = null;
        try { rules = sh.cssRules; } catch (e) {}
        walk(rules);
      }
    } catch (e) {}
    keyframesMap = map;
    keyframesGen = cssGen;
    return map;
  }
  // The keyframes of a CSS animation (CSS Animations 1 §3): sorted by
  // offset; keyframes with the same offset, timing function and composite
  // merge (later declarations win); `!important` ones are ignored.
  function cssKeyframes(rule, easing, composite) {
    var list = [];
    var kfs = rule.cssRules || [];
    for (var i = 0; i < kfs.length; i++) {
      var kf = kfs[i];
      var offsets = String(kf.keyText).split(',').map(function (x) {
        x = x.trim().toLowerCase();
        if (x === 'from') return 0;
        if (x === 'to') return 1;
        return /%$/.test(x) ? parseFloat(x) / 100 : NaN;
      });
      var st = kf.style, props = {}, fe = easing, fc = composite;
      for (var j = 0; st && j < st.length; j++) {
        var n = st.item(j);
        if (!n || st.getPropertyPriority(n) === 'important') continue;
        var v = st.getPropertyValue(n);
        if (n === 'animation-timing-function') { fe = v; continue; }
        if (n === 'animation-composition') { fc = v; continue; }
        if (/^(animation|transition)/.test(n) || n === 'display' || n === 'will-change') continue;
        props[n] = v;
      }
      offsets.forEach(function (o) {
        if (isFinite(o) && o >= 0 && o <= 1) list.push({ o: o, e: fe, c: fc, props: props, n: list.length });
      });
    }
    list.sort(function (a, b) { return a.o - b.o || a.n - b.n; });
    var frames = [];
    list.forEach(function (k) {
      var same = null;
      for (var x = frames.length - 1; x >= 0 && frames[x].offset === k.o; x--) {
        if (frames[x].easing === k.e && frames[x].composite === k.c) { same = frames[x]; break; }
      }
      if (same) { for (var p in k.props) same.props[p] = k.props[p]; }
      else frames.push({ offset: k.o, computedOffset: k.o, easing: k.e, composite: k.c, props: Object.assign({}, k.props) });
    });
    frames.forEach(function (f) {
      var e;
      try { e = parseEasing(f.easing || 'ease'); } catch (x) { e = NAMED.ease; }
      f.easing = e[1];
      f.__fn = e[0];
      if (f.composite === composite) f.composite = 'auto';
    });
    return frames;
  }

  var CSSAnimation = class CSSAnimation extends Animation {
    constructor(token) {
      if (token !== TOKEN) throw new TypeError('Illegal constructor');
      super(null, docTimeline);
    }
  };
  Object.defineProperty(CSSAnimation.prototype, 'animationName', { configurable: true, enumerable: true, get: function () { return this.__cssName; } });
  var CSSTransition = class CSSTransition extends Animation {
    constructor(token) {
      if (token !== TOKEN) throw new TypeError('Illegal constructor');
      super(null, docTimeline);
    }
  };
  Object.defineProperty(CSSTransition.prototype, 'transitionProperty', { configurable: true, enumerable: true, get: function () { return this.__cssProp; } });
  // Calls through the API override the declarative state (CSS Animations 2
  // §4.4 "Animation API overrides"): play()/pause() fix the play state,
  // cancel() releases the owning element.
  ['play', 'pause'].forEach(function (m) {
    var base = Animation.prototype[m];
    // Only a successful call overrides.
    CSSAnimation.prototype[m] = function () { var r = base.apply(this, arguments); this.__playOverride = true; return r; };
  });
  // reverse() and setting the start time override when they change whether
  // the animation runs.
  function runs(a) { var s = playState(a); return !(s === 'paused' || a.__pauseTask); }
  CSSAnimation.prototype.reverse = function () {
    var before = runs(this);
    var r = Animation.prototype.reverse.apply(this, arguments);
    if (runs(this) !== before) this.__playOverride = true;
    return r;
  };
  [CSSAnimation, CSSTransition].forEach(function (C) {
    C.prototype.cancel = function () { var r = Animation.prototype.cancel.apply(this, arguments); this.__owner = null; return r; };
    // These getters see pending style changes (CSS Animations 2 §4.1).
    ['playState', 'pending', 'ready', 'finished', 'currentTime', 'startTime'].forEach(function (n) {
      var d = Object.getOwnPropertyDescriptor(Animation.prototype, n);
      if (!d || !d.get) return;
      Object.defineProperty(C.prototype, n, {
        configurable: true, enumerable: true,
        get: function () { if (h.cssUpdate) { try { h.cssUpdate(); } catch (e) {} } return d.get.call(this); },
        set: d.set ? function (v) {
          var before = n === 'startTime' && C === CSSAnimation ? runs(this) : null;
          d.set.call(this, v);
          if (before !== null && runs(this) !== before) this.__playOverride = true;
        } : undefined
      });
    });
    if (typeof Symbol === 'function' && Symbol.toStringTag) {
      try { Object.defineProperty(C.prototype, Symbol.toStringTag, { value: C.name, configurable: true }); } catch (e) {}
    }
  });
  g.CSSAnimation = CSSAnimation;
  g.CSSTransition = CSSTransition;

  function parseIterations(s) {
    s = String(s).trim().toLowerCase();
    if (s === 'infinite') return Infinity;
    var n = parseFloat(s);
    return isFinite(n) && n >= 0 ? n : 1;
  }
  function cssTimingFrom(lists, i) {
    var dir = pick(lists.dirs, i), fill = pick(lists.fills, i);
    return {
      duration: Math.max(timeMs(pick(lists.durs, i)), 0),
      delay: timeMs(pick(lists.delays, i)),
      iterations: parseIterations(pick(lists.iters, i) || '1'),
      direction: DIRS.indexOf(dir) >= 0 ? dir : 'normal',
      fill: ['none', 'forwards', 'backwards', 'both'].indexOf(fill) >= 0 ? fill : 'none'
    };
  }
  function applyTiming(eff, timing) {
    var t = eff.__timing, o = eff.__overrides || {};
    for (var k in timing) if (!o[k]) t[k] = timing[k];
  }
  function createCssAnimation(el, name, rule, timing, easing, composite, paused, ps) {
    var a = new CSSAnimation(TOKEN);
    def(a, '__css', true);
    def(a, '__cssClass', 1);
    def(a, '__owner', el);
    def(a, '__cssName', name);
    def(a, '__cssPrevPhase', 'idle');
    def(a, '__cssPrevIter', null);
    var eff = new KeyframeEffect(el, null, ps ? { pseudoElement: ps } : undefined);
    applyTiming(eff, timing);
    eff.__composite = ['replace', 'add', 'accumulate'].indexOf(composite) >= 0 ? composite : 'replace';
    eff.__frames = cssKeyframes(rule, easing, eff.__composite);
    setEffect(a, eff);
    if (paused) pause(a); else play(a, true);
    return a;
  }
  function updateCssAnimation(a, rule, timing, easing, composite, paused) {
    var eff = a.__effect;
    if (eff && a.__owner && eff.__target === a.__owner && !(eff.__overrides && eff.__overrides.effect)) {
      applyTiming(eff, timing);
      if (!(eff.__overrides && eff.__overrides.keyframes)) {
        var c = ['replace', 'add', 'accumulate'].indexOf(composite) >= 0 ? composite : 'replace';
        if (!(eff.__overrides && eff.__overrides.composite)) eff.__composite = c;
        eff.__frames = cssKeyframes(rule, easing, eff.__composite);
      }
    }
    if (!a.__playOverride && playState(a) !== 'idle') {
      var s = playState(a);
      if (paused && s !== 'paused' && !a.__pauseTask) pause(a);
      else if (!paused && (s === 'paused' || a.__pauseTask)) play(a, false);
    }
    updateFinished(a, false, false);
    dirty();
  }
  function cancelByStyle(a) {
    if (a.__cancelledByStyle) return;
    a.__cancelledByStyle = true;
    cancel(a);
    a.__purge = true;
  }
  function updateCssAnimations(el, st, vals) {
    var names = splitList(vals[0]);
    var lists = { durs: splitList(vals[1]), iters: splitList(vals[3]), dirs: splitList(vals[4]), fills: splitList(vals[5]), delays: splitList(vals[7]) };
    var eases = splitList(vals[2]), states = splitList(vals[6]), comps = splitList(vals[8]);
    var old = st.anims, used = [], next = [];
    var rules = names.length === 1 && names[0] === 'none' ? {} : keyframesRules();
    for (var i = names.length - 1; i >= 0; i--) {
      var name = unquote(names[i]);
      if (!name || name === 'none') continue;
      var rule = rules[name];
      if (!rule) continue;
      var a = null;
      for (var j = old.length - 1; j >= 0; j--) {
        if (!used[j] && old[j].__cssName === name) { used[j] = true; a = old[j]; break; }
      }
      var timing = cssTimingFrom(lists, i);
      var easing = pick(eases, i) || 'ease', composite = pick(comps, i) || 'replace', paused = pick(states, i) === 'paused';
      if (!a) a = createCssAnimation(el, name, rule, timing, easing, composite, paused, st.ps);
      else if (a.__owner) updateCssAnimation(a, rule, timing, easing, composite, paused);
      a.__cssOrder = i;
      next.unshift(a);
    }
    old.forEach(function (a, j) { if (!used[j] && a.__owner) cancelByStyle(a); });
    st.anims = next;
  }

  // Transitions ------------------------------------------------------
  function transformedProgress(a) {
    var eff = a.__effect;
    if (!eff) return 0;
    var c = computed(eff);
    if (c.progress === null) return c.__phase === 'after' ? 1 : 0;
    return c.progress;
  }
  function canInterpolate(p, a, b) {
    if (!HOST_CAN_LERP) return true;
    return !!HOST_CAN_LERP(p, String(a), String(b));
  }
  function startTransition(el, st, p, from, to, params, adjustedStart, factor) {
    var a = new CSSTransition(TOKEN);
    def(a, '__css', true);
    def(a, '__cssClass', 0);
    def(a, '__owner', el);
    def(a, '__cssProp', p);
    def(a, '__cssFrom', from);
    def(a, '__cssTo', to);
    def(a, '__cssAdjustedStart', adjustedStart);
    def(a, '__cssFactor', factor);
    def(a, '__cssPrevPhase', 'idle');
    def(a, '__cssOrder', transitionGeneration);
    var eff = new KeyframeEffect(el, null, st.ps ? { pseudoElement: st.ps } : undefined);
    var dur = Math.max(params.dur, 0) * factor, delay = params.delay < 0 ? params.delay * factor : params.delay;
    applyTiming(eff, { duration: dur, delay: delay, fill: 'backwards', iterations: 1, direction: 'normal' });
    // The timing function is the effect's easing (CSS Transitions 2 §4.1).
    var e;
    try { e = parseEasing(params.easing || 'ease'); } catch (x) { e = NAMED.ease; }
    eff.__timing.easing = e[1];
    eff.__timing.__easingFn = e[0];
    var p0 = {}, p1 = {};
    p0[p] = from;
    p1[p] = to;
    eff.__frames = [
      { offset: 0, computedOffset: 0, easing: 'linear', __fn: NAMED.linear[0], composite: 'auto', props: p0 },
      { offset: 1, computedOffset: 1, easing: 'linear', __fn: NAMED.linear[0], composite: 'auto', props: p1 }
    ];
    setEffect(a, eff);
    play(a, true);
    st.trans[p] = a;
    return a;
  }
  function cancelTransition(st, p) {
    var a = st.trans[p];
    if (!a) return;
    delete st.trans[p];
    if (playState(a) !== 'idle' && playState(a) !== 'finished') cancelByStyle(a);
    else a.__purge = true;
  }
  function updateCssTransitions(el, st, vals) {
    var props = splitList(vals[9]), durs = splitList(vals[10]), eases = splitList(vals[11]), delays = splitList(vals[12]), behaviors = splitList(vals[13]);
    var params = {};
    for (var i = 0; i < props.length; i++) {
      var raw = props[i];
      var low = raw.toLowerCase();
      if (!raw || low === 'none') continue;
      var q = { dur: timeMs(pick(durs, i)), delay: timeMs(pick(delays, i)), easing: pick(eases, i) || 'ease', discrete: pick(behaviors, i) === 'allow-discrete' };
      // `all` with `allow-discrete` covers the common discrete properties too.
      var list = low === 'all' ? (q.discrete ? ALL_PROPS.concat(DISCRETE_PROPS) : ALL_PROPS) : longhandsOf(raw.indexOf('--') === 0 ? raw : low);
      for (var k = 0; k < list.length; k++) params[list[k]] = q;
    }
    for (var rp in st.trans) {
      if (!params[rp] || Math.max(params[rp].dur, 0) + params[rp].delay <= 0) cancelTransition(st, rp);
    }
    var names = Object.keys(params).filter(function (n) {
      var q = params[n];
      return Math.max(q.dur, 0) + q.delay > 0 || st.snap[n] !== undefined;
    });
    if (!names.length || !HOST_BASES) return;
    var tb = now();
    var bases = HOST_BASES(el, names, st.ps || null) || [];
    stats.bases++;
    stats.basesMs += now() - tb;
    // The before-change value of a property: what the effect stack gives it
    // now (another transition may control it), else the old value.
    var animatedNow = null, otherAnimated = null;
    function nowValue(n, fallback) {
      if (!animatedNow) animatedNow = sampleTarget(el, null, false, null, false, st.ps);
      return animatedNow[n] !== undefined ? animatedNow[n] : fallback;
    }
    // Properties that just got a transition: the styles from before the
    // change are their before-change style.
    // A box taken out of the document has no before-change style when it
    // comes back.
    var wasRemoved = st.fresh && removedBoxes.has(el);
    if (st.fresh) removedBoxes.delete(el);
    st.fresh = false;
    var missing = names.filter(function (n) { return st.snap[n] === undefined; });
    if (missing.length && HOST_PREV && !wasRemoved && !st.ps) {
      var prev = HOST_PREV(el, missing) || [];
      missing.forEach(function (n, idx) { if (typeof prev[idx] === 'string') st.snap[n] = prev[idx]; });
    }
    // A box without any before-change style starts from its starting style
    // (`@starting-style`, CSS Transitions 2 §3.1).
    var unstarted = names.filter(function (n) { return st.snap[n] === undefined; });
    if (unstarted.length && HOST_START && !st.ps) {
      var start = HOST_START(el, unstarted) || [];
      unstarted.forEach(function (n, idx) { if (typeof start[idx] === 'string') st.snap[n] = start[idx]; });
    }
    names.forEach(function (n, idx) {
      var after = bases[idx];
      if (after === null || after === undefined) return;
      var before = st.snap[n];
      st.snap[n] = after;
      if (before === undefined) return;
      // Other animations of the property count in the before- and the
      // after-change style alike: then nothing transitions (CSS Transitions
      // 1 §3; Web Animations 1 §4.4).
      if (!otherAnimated) otherAnimated = sampleTarget(el, null, false, null, true, st.ps);
      if (otherAnimated[n] !== undefined && !st.trans[n]) return;
      var q = params[n];
      var combined = Math.max(q.dur, 0) + q.delay;
      var run = st.trans[n];
      if (run && (playState(run) === 'idle' || playState(run) === 'finished')) {
        // A completed transition (CSS Transitions 1 §3 rule 2).
        if (run.__cssTo !== after) { delete st.trans[n]; run.__purge = true; }
        run = null;
        if (before === after) return;
      }
      if (!run) {
        if (before === after || combined <= 0) return;
        if (!q.discrete && !canInterpolate(n, before, after)) return;
        startTransition(el, st, n, before, after, q, before, 1);
        return;
      }
      if (run.__cssTo === after) return;
      if (combined <= 0) { cancelTransition(st, n); return; }
      var current = nowValue(n, before);
      if (current === after || (!q.discrete && !canInterpolate(n, current, after))) { cancelTransition(st, n); return; }
      if (after === run.__cssAdjustedStart) {
        // Reversing a transition shortens it (§3 rule 4b).
        var factor = Math.abs(transformedProgress(run) * run.__cssFactor + 1 - run.__cssFactor);
        factor = Math.min(Math.max(factor, 0), 1);
        var adjusted = run.__cssTo;
        cancelTransition(st, n);
        startTransition(el, st, n, current, after, q, adjusted, factor);
        return;
      }
      cancelTransition(st, n);
      startTransition(el, st, n, current, after, q, current, 1);
    });
  }

  // Style changes: the elements with CSS animations or transitions.
  function cssUpdate() {
    if (!HOST_CSS) return;
    var t0 = now();
    stats.cssUpdate++;
    try { cssUpdateInner(); } finally { stats.cssUpdateMs += now() - t0; }
  }
  function cssUpdateInner() {
    // Pending CSSOM changes reach the style elements first.
    try { if (typeof g.__narsilCssomSpiegeln === 'function') g.__narsilCssomSpiegeln(); } catch (e) {}
    // Only the elements the DOM changed at (and their subtrees): the
    // observer's records since the last scan.
    var roots = null;
    if (domObserver && !needFull) {
      collectRoots(domObserver.takeRecords());
      roots = pendingRoots;
    } else if (domObserver) domObserver.takeRecords();
    pendingRoots = [];
    needFull = false;
    var ts = now();
    // The elements as single arguments (`true` marks the list): an element
    // goes over the host bridge as its handle that way.
    var r = roots ? HOST_CSS.apply(null, [cssGen, true].concat(roots)) : HOST_CSS(cssGen, null);
    stats.scanMs += now() - ts;
    if (!r || !r.length) return;
    stats.scans++;
    if (r[1] !== false) stats.full = (stats.full || 0) + 1;
    stats.found = (r.length - 2) / 2;
    cssGen = r[0];
    transitionGeneration++;
    // Styles changed: keyframe values in element context (`em`, `inherit`,
    // `currentcolor`, underlying values) are computed again.
    isDirty = true;
    var full = r[1] !== false;
    // One state per element and per pseudo-element (the 15th value names it).
    var seen = new Map();
    for (var i = 2; i + 1 < r.length; i += 2) {
      var ps = r[i + 1][14] || null;
      seen.set(stateKey(r[i], ps), { el: r[i], ps: ps, vals: r[i + 1] });
    }
    cssStates.forEach(function (st, key) {
      if (seen.has(key)) return;
      var el = st.el;
      if (!full && el.isConnected && !roots.some(function (rt) { return rt === el || (rt.contains && rt.contains(el)); })) return;
      if (!el.isConnected) removedBoxes.add(el);
      st.anims.forEach(function (a) { if (a.__owner) cancelByStyle(a); });
      for (var p in st.trans) cancelTransition(st, p);
      cssStates.delete(key);
    });
    seen.forEach(function (e, key) {
      var st = cssStates.get(key);
      if (!st) { st = { el: e.el, ps: e.ps, anims: [], trans: {}, snap: {}, fresh: true }; cssStates.set(key, st); }
      try { updateCssAnimations(e.el, st, e.vals); } catch (x) { if (g.console) g.console.error('css animations: ' + x); }
      try { updateCssTransitions(e.el, st, e.vals); } catch (x) { if (g.console) g.console.error('css transitions: ' + x); }
    });
  }
  var pseudoKeys = new WeakMap();
  function stateKey(el, ps) {
    if (!ps) return el;
    var m = pseudoKeys.get(el);
    if (!m) { m = {}; pseudoKeys.set(el, m); }
    return m[ps] || (m[ps] = { el: el, ps: ps });
  }
  h.cssUpdate = cssUpdate;
  h.cssPending = function () { return cssStates.size > 0 && animations.some(function (a) { return a.__css && a.__owner && playState(a) === 'running'; }); };

  // Events (CSS Animations 2 §4.2, CSS Transitions 2 §5) ---------------
  function activeTimeNow(a) {
    var eff = a.__effect;
    if (!eff) return 0;
    var t = eff.__timing, lt = currentTime(a);
    if (lt === null) return 0;
    return Math.max(Math.min(lt - t.delay, activeDuration(t)), 0);
  }
  var baseCancel = cancel;
  // Canceling dispatches animationcancel/transitioncancel at the next
  // frame even when the animation restarts before it.
  cancel = function (a) {
    if (a.__css) {
      a.__cssCancelTime = activeTimeNow(a);
      var prev = a.__cssPrevPhase || 'idle';
      if (prev !== 'idle' && prev !== 'after' && playState(a) !== 'idle') {
        queueCss(a, a instanceof CSSAnimation ? 'animationcancel' : 'transitioncancel', a.__cssCancelTime);
        a.__cssPrevPhase = 'idle';
        a.__cssPrevIter = null;
      }
    }
    return baseCancel(a);
  };
  function cssPhaseOf(a) {
    if (playState(a) === 'idle') return 'idle';
    if (!a.__effect) { var ct = currentTime(a); return ct !== null && ct < 0 ? 'before' : 'after'; }
    var c = computed(a.__effect);
    return c.__phase === 'none' ? 'idle' : c.__phase;
  }
  function queueCss(a, type, elapsedMs) {
    var el = a.__owner || a.__cssLastOwner;
    if (!el) return;
    var isAnim = a instanceof CSSAnimation;
    // The scheduled event time: the timeline time of the moment the event
    // happened (start time + local time / playback rate).
    var tt = timelineTime(docTimeline);
    if (a.__startTime !== null && a.__rate && a.__effect && !/cancel$/.test(type)) {
      var at = a.__startTime + (a.__effect.__timing.delay + elapsedMs) / a.__rate;
      if (isFinite(at)) tt = at;
    }
    var init = isAnim
      ? { animationName: a.__cssName, elapsedTime: elapsedMs / 1000, pseudoElement: (a.__effect && a.__effect.__pseudo) || '', bubbles: true }
      : { propertyName: a.__cssProp, elapsedTime: elapsedMs / 1000, pseudoElement: (a.__effect && a.__effect.__pseudo) || '', bubbles: true };
    var Ctor = isAnim ? g.AnimationEvent : g.TransitionEvent;
    eventAnimationAttribute(Ctor);
    cssEventQueue.push({
      t: tt === null ? 0 : tt, a: a, n: ++eventSeq,
      run: function () {
        var ev = g.__narsilUA ? g.__narsilUA(Ctor, type, init) : new Ctor(type, init);
        try { Object.defineProperty(ev, '__animation', { value: a, configurable: true }); } catch (e) {}
        el.dispatchEvent(ev);
      }
    });
  }
  // AnimationEvent and TransitionEvent with their attributes on the
  // prototype (CSS Animations 1 §4.1, CSS Transitions 1 §6.1).
  function eventFields(C, fields) {
    fields.forEach(function (f) {
      Object.defineProperty(C.prototype, f[0], {
        configurable: true, enumerable: true,
        get: function () { var s = this.__eventFields; return s && f[0] in s ? s[f[0]] : f[1]; }
      });
    });
  }
  function eventInit(self, init, fields) {
    var vals = {};
    init = init || {};
    fields.forEach(function (f) {
      var v = init[f[0]];
      vals[f[0]] = v === undefined ? f[1] : (typeof f[1] === 'number' ? Number(v) : String(v));
    });
    Object.defineProperty(self, '__eventFields', { value: vals, configurable: true });
  }
  if (typeof g.Event === 'function') {
    try {
      var ANIM_FIELDS = [['animationName', ''], ['elapsedTime', 0], ['pseudoElement', '']];
      var TRANS_FIELDS = [['propertyName', ''], ['elapsedTime', 0], ['pseudoElement', '']];
      var AnimationEventC = class AnimationEvent extends g.Event {
        constructor(type, init) {
          if (arguments.length < 1) throw new TypeError("Failed to construct 'AnimationEvent': 1 argument required, but only 0 present.");
          super(type, init);
          eventInit(this, init, ANIM_FIELDS);
        }
      };
      var TransitionEventC = class TransitionEvent extends g.Event {
        constructor(type, init) {
          if (arguments.length < 1) throw new TypeError("Failed to construct 'TransitionEvent': 1 argument required, but only 0 present.");
          super(type, init);
          eventInit(this, init, TRANS_FIELDS);
        }
      };
      eventFields(AnimationEventC, ANIM_FIELDS);
      eventFields(TransitionEventC, TRANS_FIELDS);
      var probeEvent = new AnimationEventC('x', { animationName: 'a' });
      if (probeEvent.animationName === 'a' && probeEvent.type === 'x' && probeEvent instanceof g.Event) {
        Object.defineProperty(g, 'AnimationEvent', { value: AnimationEventC, writable: true, configurable: true, enumerable: false });
        Object.defineProperty(g, 'TransitionEvent', { value: TransitionEventC, writable: true, configurable: true, enumerable: false });
      }
    } catch (e) {}
  }
  // `event.animation`: the CSSAnimation/CSSTransition of a UA event (CSS
  // Animations 2 §5.1, tentative).
  function eventAnimationAttribute(C) {
    if (!C || !C.prototype || C.prototype.hasOwnProperty('animation')) return;
    try {
      Object.defineProperty(C.prototype, 'animation', {
        configurable: true, enumerable: true,
        get: function () { return this.__animation || null; }
      });
    } catch (e) {}
  }
  function cssEventsFor(a) {
    var eff = a.__effect;
    var prev = a.__cssPrevPhase || 'idle';
    var phase = cssPhaseOf(a);
    if (a.__owner) a.__cssLastOwner = a.__owner;
    var t = eff ? eff.__timing : null;
    var ad = t ? activeDuration(t) : 0;
    var start = t ? Math.max(Math.min(-t.delay, ad), 0) : 0;
    var end = t ? Math.max(Math.min(endTime(t) - t.delay, ad), 0) : 0;
    var isAnim = a instanceof CSSAnimation;
    var c = eff ? computed(eff) : null;
    var iter = c ? c.currentIteration : null;
    if (phase !== 'idle') a.__cssLastActive = activeTimeNow(a);
    if (phase === prev) {
      if (isAnim && phase === 'active' && iter !== null && a.__cssPrevIter !== null && iter !== a.__cssPrevIter) {
        // The iteration boundary crossed (either way).
        var it = Math.max(iter, a.__cssPrevIter);
        queueCss(a, 'animationiteration', (it - t.iterationStart) * iterDuration(t));
      }
      a.__cssPrevIter = iter;
      return;
    }
    var names = isAnim ? ['animationstart', 'animationend', 'animationcancel'] : ['transitionstart', 'transitionend', 'transitioncancel'];
    var S = names[0], EN = names[1], CA = names[2];
    if (phase === 'idle') {
      if (prev !== 'after') queueCss(a, CA, a.__cssCancelTime !== undefined ? a.__cssCancelTime : (a.__cssLastActive || 0));
      a.__cssCancelTime = undefined;
    } else if (prev === 'idle') {
      if (!isAnim) queueCss(a, 'transitionrun', start);
      if (phase === 'active') queueCss(a, S, start);
      else if (phase === 'after') { queueCss(a, S, start); queueCss(a, EN, end); }
    } else if (prev === 'before') {
      if (phase === 'active') queueCss(a, S, start);
      else if (phase === 'after') { queueCss(a, S, start); queueCss(a, EN, end); }
    } else if (prev === 'active') {
      if (phase === 'after') queueCss(a, EN, end);
      else if (phase === 'before') queueCss(a, EN, start);
    } else if (prev === 'after') {
      if (phase === 'active') queueCss(a, S, end);
      else if (phase === 'before') { queueCss(a, S, end); queueCss(a, EN, start); }
    }
    a.__cssPrevPhase = phase;
    a.__cssPrevIter = iter;
  }
  h.cssEvents = function () {
    animations.forEach(function (a) { if (a.__css) cssEventsFor(a); });
    return cssEventQueue.splice(0);
  };
  function purgeCss() {
    for (var i = animations.length - 1; i >= 0; i--) {
      var a = animations[i];
      if (a.__purge && !a.__events.length && (playState(a) === 'idle' || playState(a) === 'finished') && a.__cssPrevPhase === cssPhaseOf(a)) animations.splice(i, 1);
    }
  }
  h.cssPurge = purgeCss;
  // DOM changes between frames start the frame loop again: transitions and
  // animations they cause begin at the next frame even when no script asks.
  // The records also tell `cssUpdate` which elements to look at.
  var domObserver = null, pendingRoots = [], needFull = true;
  function collectRoots(recs) {
    if (needFull || !recs) return;
    for (var q = 0; q < recs.length; q++) {
      var rec = recs[q], tg = rec.target, el = null;
      if (rec.type === 'childList') {
        for (var z = 0; z < rec.addedNodes.length; z++) {
          var an = rec.addedNodes[z];
          if (an && an.nodeType === 1 && pendingRoots.indexOf(an) < 0) pendingRoots.push(an);
        }
        continue;
      }
      if (rec.type === 'attributes') el = tg;
      else if (rec.type === 'characterData') el = tg && tg.parentNode;
      if (el && el.nodeType === 1 && pendingRoots.indexOf(el) < 0) pendingRoots.push(el);
      if (pendingRoots.length > 256) { needFull = true; pendingRoots = []; return; }
    }
  }
  if (typeof g.MutationObserver === 'function' && g.document) {
    try {
      domObserver = new g.MutationObserver(function (recs) {
        var tm = now();
        stats.mo = (stats.mo || 0) + 1;
        stats.moRecs = (stats.moRecs || 0) + (recs ? recs.length : 0);
        collectRoots(recs);
        stats.moMs = (stats.moMs || 0) + now() - tm;
        startLoop();
      });
      domObserver.observe(g.document, { attributes: true, childList: true, subtree: true, characterData: true });
    } catch (e) { domObserver = null; }
  }
  h.treeIndex = function (el) { return treeIndex.get(el) || 0; };
  // The styles of this moment are the first style change event: later
  // changes transition from them.
  try { cssUpdate(); } catch (e) {}
  schedule();
})(typeof h === 'object' && h ? h : {});
