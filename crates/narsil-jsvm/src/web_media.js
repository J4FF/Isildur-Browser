// Media Source Extensions (https://w3c.github.io/media-source/): MediaSource,
// SourceBuffer and SourceBufferList. Loaded lazily on the first use of one
// of these globals. The byte stream parsers and track buffers live in
// web_media.rs. A media element whose src is the blob URL of a MediaSource
// attaches it through `__narsilMseAttach(mediaSource, element, api)`; `api`
// is the element side (vorspann.js): ranges(list) -> TimeRanges,
// currentTime(), duration(d), metadata(width, height), update(), error(kind),
// hasError().
(function (N, g) {
  'use strict';
  var defineProperty = Object.defineProperty;
  var TypeErrorCtor = TypeError;
  var ET = g.EventTarget;
  var KEY = {};

  function domError(name, msg) {
    if (typeof g.DOMException === 'function') return new g.DOMException(msg, name);
    var e = new Error(msg);
    e.name = name;
    return e;
  }
  function report(e) {
    try { if (typeof g.reportError === 'function') g.reportError(e); } catch (x) {}
  }
  function fire(target, type) {
    var ev;
    try {
      ev = typeof g.__narsilUA === 'function' ? g.__narsilUA(g.Event, type, { bubbles: false, cancelable: false }) : new g.Event(type);
    } catch (e) { ev = new g.Event(type); }
    try { target.dispatchEvent(ev); } catch (e) { report(e); }
  }
  function task(f) { g.setTimeout(f, 0); }
  function fail(iface, member, name, msg) {
    return domError(name, "Failed to execute '" + member + "' on '" + iface + "': " + msg);
  }
  function failSet(iface, member, name, msg) {
    return domError(name, "Failed to set the '" + member + "' property on '" + iface + "': " + msg);
  }
  function finite(v, iface, member) {
    var n = Number(v);
    if (!isFinite(n)) throw new TypeErrorCtor("Failed to set the '" + member + "' property on '" + iface + "': The provided double value is non-finite.");
    return n;
  }
  function isBufferSource(v) {
    return v instanceof ArrayBuffer || ArrayBuffer.isView(v) ||
      (typeof g.SharedArrayBuffer === 'function' && v instanceof g.SharedArrayBuffer);
  }

  // ---- Event handler attributes --------------------------------------
  function handlers(proto, names, map) {
    names.forEach(function (n) {
      var key = 'on' + n;
      var get = function () { var s = map.get(this); return s && s.handlers[key] || null; };
      var set = function (fn) {
        var s = map.get(this);
        if (!s) throw new TypeErrorCtor('Illegal invocation');
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
    if (!s) throw new TypeErrorCtor('Illegal invocation');
    return s;
  }

  var MS = new WeakMap();
  var SB = new WeakMap();
  var LIST = new WeakMap();

  // ---- SourceBufferList ----------------------------------------------
  class SourceBufferList extends ET {
    constructor(key) {
      if (key !== KEY) throw new TypeErrorCtor('Illegal constructor');
      super();
      LIST.set(this, { items: [], shown: 0, handlers: {}, wrapped: {} });
    }
    get length() { return brand(LIST, this).items.length; }
  }
  handlers(SourceBufferList.prototype, ['addsourcebuffer', 'removesourcebuffer'], LIST);
  defineProperty(SourceBufferList.prototype, Symbol.iterator, { value: Array.prototype.values, writable: true, configurable: true });
  function listSync(l) {
    var s = LIST.get(l), n = s.items.length;
    for (var i = n; i < s.shown; i++) delete l[i];
    for (var j = 0; j < n; j++) defineProperty(l, j, { value: s.items[j], enumerable: true, configurable: true, writable: false });
    s.shown = n;
  }
  function listHas(l, sb) { return LIST.get(l).items.indexOf(sb) >= 0; }
  function listAdd(l, sb) {
    LIST.get(l).items.push(sb);
    listSync(l);
    task(function () { fire(l, 'addsourcebuffer'); });
  }
  function listRemove(l, sb) {
    var s = LIST.get(l), i = s.items.indexOf(sb);
    if (i < 0) return;
    s.items.splice(i, 1);
    listSync(l);
    task(function () { fire(l, 'removesourcebuffer'); });
  }

  // ---- Helpers over a MediaSource state ------------------------------
  function sbStates(s) { return LIST.get(s.list).items.map(function (sb) { return SB.get(sb); }); }
  function anyUpdating(s) { return sbStates(s).some(function (b) { return b.updating; }); }
  function highest(s, which) {
    var h = NaN;
    sbStates(s).forEach(function (b) {
      var v = N.sbHighest(b.id)[which];
      if (!(v <= h)) h = v;
    });
    return h;
  }
  function openIfEnded(ms) {
    var s = MS.get(ms);
    if (s.readyState === 'ended') {
      s.readyState = 'open';
      task(function () { fire(ms, 'sourceopen'); });
    }
  }
  // The duration change algorithm (MSE §"Duration change").
  function durationChange(ms, d, fromSetter) {
    var s = MS.get(ms);
    if (s.duration === d) return;
    if (fromSetter && d < highest(s, 0)) {
      throw failSet('MediaSource', 'duration', 'InvalidStateError', 'The new duration is lower than the highest buffered presentation timestamp.');
    }
    var he = highest(s, 1);
    if (d < he) d = he;
    s.duration = d;
    if (s.api) s.api.duration(d);
  }
  // The end of stream algorithm.
  function endOfStream(ms, error) {
    var s = MS.get(ms);
    s.readyState = 'ended';
    task(function () { fire(ms, 'sourceended'); });
    if (error === undefined) {
      var he = highest(s, 1);
      if (he === he) durationChange(ms, he, false);
      if (s.api) s.api.update();
    } else if (s.api) {
      s.api.error(error);
    }
  }
  function elementBuffered(s) {
    var ids = LIST.get(s.active).items.map(function (sb) { return SB.get(sb).id; });
    return pairs(N.msBuffered(ids, s.readyState === 'ended'));
  }
  function pairs(flat) {
    var out = [];
    for (var i = 0; i + 1 < flat.length; i += 2) out.push([flat[i], flat[i + 1]]);
    return out;
  }
  function ranges(s, list) {
    if (s.api && typeof s.api.ranges === 'function') return s.api.ranges(list);
    return new TimeRangesLite(list);
  }
  // TimeRanges without the preamble's media element (engine tests).
  function TimeRangesLite(list) { this._l = list; }
  TimeRangesLite.prototype = {
    get length() { return this._l.length; },
    start: function (i) {
      if (!(i >= 0 && i < this._l.length)) throw domError('IndexSizeError', 'The index provided (' + i + ') is out of range.');
      return this._l[i][0];
    },
    end: function (i) {
      if (!(i >= 0 && i < this._l.length)) throw domError('IndexSizeError', 'The index provided (' + i + ') is out of range.');
      return this._l[i][1];
    }
  };

  // ---- SourceBuffer --------------------------------------------------
  class SourceBuffer extends ET {
    constructor(key, ms, id, generate) {
      if (key !== KEY) throw new TypeErrorCtor('Illegal constructor');
      super();
      SB.set(this, {
        ms: ms, id: id, gen: !!generate, mode: generate ? 'sequence' : 'segments', updating: false, removing: false,
        removed: false, active: false, firstInit: false, width: 0, height: 0, tso: 0, aws: 0, awe: Infinity, job: 0,
        handlers: {}, wrapped: {}
      });
    }
    get mode() { return brand(SB, this).mode; }
    set mode(v) {
      var b = brand(SB, this), m = String(v);
      if (m !== 'segments' && m !== 'sequence') return;
      if (b.removed || b.updating) throw failSet('SourceBuffer', 'mode', 'InvalidStateError', 'This SourceBuffer has been removed from the parent media source, or is still processing an operation.');
      if (b.gen && m === 'segments') throw new TypeErrorCtor("Failed to set the 'mode' property on 'SourceBuffer': The mode value provided (segments) is invalid for a byte stream format that uses generated timestamps.");
      openIfEnded(b.ms);
      if (N.sbParsing(b.id)) throw failSet('SourceBuffer', 'mode', 'InvalidStateError', 'The SourceBuffer is in the middle of a media segment.');
      if (m === 'sequence') N.sbGroupStart(b.id);
      b.mode = m;
    }
    get updating() { return brand(SB, this).updating; }
    get buffered() {
      var b = brand(SB, this);
      if (b.removed) throw domError('InvalidStateError', "Failed to read the 'buffered' property from 'SourceBuffer': This SourceBuffer has been removed from the parent media source.");
      var s = MS.get(b.ms);
      return ranges(s, pairs(N.sbBuffered(b.id, s.readyState === 'ended')));
    }
    get timestampOffset() { return brand(SB, this).tso; }
    set timestampOffset(v) {
      var b = brand(SB, this), n = finite(v, 'SourceBuffer', 'timestampOffset');
      if (b.removed || b.updating) throw failSet('SourceBuffer', 'timestampOffset', 'InvalidStateError', 'This SourceBuffer has been removed from the parent media source, or is still processing an operation.');
      openIfEnded(b.ms);
      if (N.sbParsing(b.id)) throw failSet('SourceBuffer', 'timestampOffset', 'InvalidStateError', 'The SourceBuffer is in the middle of a media segment.');
      if (b.mode === 'sequence') N.sbGroupStart(b.id, n);
      b.tso = n;
    }
    get appendWindowStart() { return brand(SB, this).aws; }
    set appendWindowStart(v) {
      var b = brand(SB, this), n = finite(v, 'SourceBuffer', 'appendWindowStart');
      if (b.removed || b.updating) throw failSet('SourceBuffer', 'appendWindowStart', 'InvalidStateError', 'This SourceBuffer has been removed from the parent media source, or is still processing an operation.');
      if (n < 0 || n >= b.awe) throw new TypeErrorCtor("Failed to set the 'appendWindowStart' property on 'SourceBuffer': The value provided (" + n + ') is outside the range [0, ' + b.awe + ').');
      b.aws = n;
    }
    get appendWindowEnd() { return brand(SB, this).awe; }
    set appendWindowEnd(v) {
      var b = brand(SB, this), n = Number(v);
      if (b.removed || b.updating) throw failSet('SourceBuffer', 'appendWindowEnd', 'InvalidStateError', 'This SourceBuffer has been removed from the parent media source, or is still processing an operation.');
      if (n !== n) throw new TypeErrorCtor("Failed to set the 'appendWindowEnd' property on 'SourceBuffer': The value provided is NaN.");
      if (n <= b.aws) throw new TypeErrorCtor("Failed to set the 'appendWindowEnd' property on 'SourceBuffer': The value provided (" + n + ') is less than or equal to the minimum value (' + b.aws + ').');
      b.awe = n;
    }
    appendBuffer(data) {
      var b = brand(SB, this), self = this;
      if (arguments.length < 1) throw new TypeErrorCtor("Failed to execute 'appendBuffer' on 'SourceBuffer': 1 argument required, but only 0 present.");
      if (!isBufferSource(data)) throw new TypeErrorCtor("Failed to execute 'appendBuffer' on 'SourceBuffer': The provided value is not of type '(ArrayBuffer or ArrayBufferView)'.");
      prepareAppend(b, data.byteLength);
      N.sbPush(b.id, data);
      b.updating = true;
      var job = ++b.job;
      task(function () { if (b.job === job) fire(self, 'updatestart'); });
      task(function () { if (b.job === job) runAppend(self, b); });
    }
    abort() {
      var b = brand(SB, this), self = this;
      if (b.removed) throw fail('SourceBuffer', 'abort', 'InvalidStateError', 'This SourceBuffer has been removed from the parent media source.');
      if (MS.get(b.ms).readyState !== 'open') throw fail('SourceBuffer', 'abort', 'InvalidStateError', "The parent media source's readyState is not 'open'.");
      if (b.removing) throw fail('SourceBuffer', 'abort', 'InvalidStateError', 'The SourceBuffer is removing data.');
      if (b.updating) {
        b.job++;
        b.updating = false;
        task(function () { fire(self, 'abort'); fire(self, 'updateend'); });
      }
      N.sbAbort(b.id);
      b.aws = 0;
      b.awe = Infinity;
    }
    changeType(type) {
      var b = brand(SB, this);
      if (arguments.length < 1) throw new TypeErrorCtor("Failed to execute 'changeType' on 'SourceBuffer': 1 argument required, but only 0 present.");
      type = String(type);
      if (type === '') throw new TypeErrorCtor("Failed to execute 'changeType' on 'SourceBuffer': The type provided is empty.");
      if (b.removed || b.updating) throw fail('SourceBuffer', 'changeType', 'InvalidStateError', 'This SourceBuffer has been removed from the parent media source, or is still processing an operation.');
      if (!N.isTypeSupported(type)) throw fail('SourceBuffer', 'changeType', 'NotSupportedError', "Changing to the type provided ('" + type + "') is not supported.");
      openIfEnded(b.ms);
      var gen = N.sbChangeType(b.id, type);
      b.gen = gen === 1;
      if (b.gen && b.mode !== 'sequence') {
        b.mode = 'sequence';
        N.sbGroupStart(b.id);
      }
    }
    remove(start, end) {
      var b = brand(SB, this), self = this;
      if (arguments.length < 2) throw new TypeErrorCtor("Failed to execute 'remove' on 'SourceBuffer': 2 arguments required, but only " + arguments.length + ' present.');
      start = Number(start);
      end = Number(end);
      if (!isFinite(start)) throw new TypeErrorCtor("Failed to execute 'remove' on 'SourceBuffer': The provided double value is non-finite.");
      if (b.removed || b.updating) throw fail('SourceBuffer', 'remove', 'InvalidStateError', 'This SourceBuffer has been removed from the parent media source, or is still processing an operation.');
      var s = MS.get(b.ms);
      if (s.duration !== s.duration) throw new TypeErrorCtor("Failed to execute 'remove' on 'SourceBuffer': The MediaSource's duration is NaN.");
      if (start < 0 || start > s.duration) throw new TypeErrorCtor("Failed to execute 'remove' on 'SourceBuffer': The start provided (" + start + ') is outside the range (0, ' + s.duration + ').');
      if (!(end > start)) throw new TypeErrorCtor("Failed to execute 'remove' on 'SourceBuffer': The end value provided (" + end + ') must be greater than the start value provided (' + start + ').');
      openIfEnded(b.ms);
      b.updating = true;
      b.removing = true;
      var job = ++b.job;
      task(function () { if (b.job === job) fire(self, 'updatestart'); });
      task(function () {
        if (b.job !== job) return;
        N.sbRemove(b.id, start, end, s.duration);
        b.updating = false;
        b.removing = false;
        if (s.api) s.api.update();
        fire(self, 'update');
        fire(self, 'updateend');
      });
    }
  }
  handlers(SourceBuffer.prototype, ['updatestart', 'update', 'updateend', 'error', 'abort'], SB);

  // "Prepare append" (MSE §3.5.4).
  function prepareAppend(b, length) {
    if (b.removed || b.updating) throw fail('SourceBuffer', 'appendBuffer', 'InvalidStateError', 'This SourceBuffer has been removed from the parent media source, or is still processing an operation.');
    var s = MS.get(b.ms);
    if (s.api && s.api.hasError()) throw fail('SourceBuffer', 'appendBuffer', 'InvalidStateError', 'The HTMLMediaElement.error attribute is not null.');
    openIfEnded(b.ms);
    var now = s.api ? s.api.currentTime() : 0;
    if (!N.sbEvict(b.id, now, length)) {
      throw fail('SourceBuffer', 'appendBuffer', 'QuotaExceededError', 'The SourceBuffer is full, and cannot free space to append additional buffers.');
    }
  }
  // The segment parser loop and its outcome (MSE §3.5.1, §3.5.6-§3.5.8).
  function runAppend(sb, b) {
    var ms = b.ms, s = MS.get(ms);
    var r = N.sbAppend(b.id, b.tso, b.aws, b.awe, b.mode === 'sequence');
    b.tso = r[1];
    if (r[3]) initReceived(sb, b, r[3]);
    if (!r[0]) {
      appendError(sb, b);
      return;
    }
    // Data beyond the duration extends it.
    var he = N.sbHighest(b.id)[1];
    if (he > s.duration) durationChange(ms, he, false);
    b.updating = false;
    if (s.api) s.api.update();
    fire(sb, 'update');
    fire(sb, 'updateend');
  }
  function initReceived(sb, b, info) {
    var s = MS.get(b.ms);
    if (s.duration !== s.duration) durationChange(b.ms, info[0] === info[0] ? info[0] : Infinity, false);
    if (info[4]) { b.width = info[1]; b.height = info[2]; }
    if (!b.active) {
      b.active = true;
      listAdd(s.active, sb);
    }
    b.firstInit = true;
    if (s.api && sbStates(s).every(function (x) { return x.firstInit; })) {
      var w = 0, h = 0;
      sbStates(s).forEach(function (x) { if (x.width || x.height) { w = x.width; h = x.height; } });
      s.api.metadata(w, h);
    }
  }
  // The append error algorithm (its events are tasks: the end of stream
  // algorithm has run when they fire).
  function appendError(sb, b) {
    N.sbAbort(b.id);
    b.updating = false;
    if (MS.get(b.ms).readyState === 'open') endOfStream(b.ms, 'decode');
    fire(sb, 'error');
    fire(sb, 'updateend');
  }

  // ---- MediaSource ---------------------------------------------------
  class MediaSource extends ET {
    constructor() {
      if (new.target === undefined) throw new TypeErrorCtor("Failed to construct 'MediaSource': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
      super();
      MS.set(this, {
        readyState: 'closed', duration: NaN, list: new SourceBufferList(KEY), active: new SourceBufferList(KEY),
        api: null, live: null, handlers: {}, wrapped: {}
      });
    }
    get sourceBuffers() { return brand(MS, this).list; }
    get activeSourceBuffers() { return brand(MS, this).active; }
    get readyState() { return brand(MS, this).readyState; }
    get duration() {
      var s = brand(MS, this);
      return s.readyState === 'closed' ? NaN : s.duration;
    }
    set duration(v) {
      var s = brand(MS, this), d = Number(v);
      if (d < 0 || d !== d) throw new TypeErrorCtor("Failed to set the 'duration' property on 'MediaSource': The value provided (" + d + ') is negative or NaN.');
      if (s.readyState !== 'open') throw failSet('MediaSource', 'duration', 'InvalidStateError', "The MediaSource's readyState is not 'open'.");
      if (anyUpdating(s)) throw failSet('MediaSource', 'duration', 'InvalidStateError', "The 'updating' attribute is true on one or more of this MediaSource's SourceBuffers.");
      durationChange(this, d, true);
    }
    addSourceBuffer(type) {
      var s = brand(MS, this);
      if (arguments.length < 1) throw new TypeErrorCtor("Failed to execute 'addSourceBuffer' on 'MediaSource': 1 argument required, but only 0 present.");
      type = String(type);
      if (type === '') throw new TypeErrorCtor("Failed to execute 'addSourceBuffer' on 'MediaSource': The type provided is empty.");
      if (!N.isTypeSupported(type)) throw fail('MediaSource', 'addSourceBuffer', 'NotSupportedError', "The type provided ('" + type + "') is unsupported.");
      if (s.readyState !== 'open') throw fail('MediaSource', 'addSourceBuffer', 'InvalidStateError', "The MediaSource's readyState is not 'open'.");
      var r = N.sbNew(type);
      var sb = new SourceBuffer(KEY, this, r[0], r[1] === 1);
      listAdd(s.list, sb);
      return sb;
    }
    removeSourceBuffer(sb) {
      var s = brand(MS, this);
      if (arguments.length < 1) throw new TypeErrorCtor("Failed to execute 'removeSourceBuffer' on 'MediaSource': 1 argument required, but only 0 present.");
      var b = SB.get(sb);
      if (!b) throw new TypeErrorCtor("Failed to execute 'removeSourceBuffer' on 'MediaSource': parameter 1 is not of type 'SourceBuffer'.");
      if (b.ms !== this || !listHas(s.list, sb)) throw fail('MediaSource', 'removeSourceBuffer', 'NotFoundError', 'The SourceBuffer provided is not contained in this MediaSource.');
      dropSourceBuffer(s, sb, b);
      if (s.api) s.api.update();
    }
    endOfStream(error) {
      var s = brand(MS, this);
      if (error !== undefined && error !== 'network' && error !== 'decode') {
        throw new TypeErrorCtor("Failed to execute 'endOfStream' on 'MediaSource': The provided value '" + error + "' is not a valid enum value of type EndOfStreamError.");
      }
      if (s.readyState !== 'open') throw fail('MediaSource', 'endOfStream', 'InvalidStateError', "The MediaSource's readyState is not 'open'.");
      if (anyUpdating(s)) throw fail('MediaSource', 'endOfStream', 'InvalidStateError', "The 'updating' attribute is true on one or more of this MediaSource's SourceBuffers.");
      endOfStream(this, error);
    }
    setLiveSeekableRange(start, end) {
      var s = brand(MS, this);
      if (arguments.length < 2) throw new TypeErrorCtor("Failed to execute 'setLiveSeekableRange' on 'MediaSource': 2 arguments required, but only " + arguments.length + ' present.');
      start = Number(start);
      end = Number(end);
      if (!isFinite(start) || !isFinite(end)) throw new TypeErrorCtor("Failed to execute 'setLiveSeekableRange' on 'MediaSource': The provided double value is non-finite.");
      if (s.readyState !== 'open') throw fail('MediaSource', 'setLiveSeekableRange', 'InvalidStateError', "The MediaSource's readyState is not 'open'.");
      if (start < 0 || start > end) throw new TypeErrorCtor("Failed to execute 'setLiveSeekableRange' on 'MediaSource': The start provided (" + start + ') is outside the range (0, ' + end + ').');
      s.live = [start, end];
    }
    clearLiveSeekableRange() {
      var s = brand(MS, this);
      if (s.readyState !== 'open') throw fail('MediaSource', 'clearLiveSeekableRange', 'InvalidStateError', "The MediaSource's readyState is not 'open'.");
      s.live = null;
    }
    static isTypeSupported(type) {
      if (arguments.length < 1) throw new TypeErrorCtor("Failed to execute 'isTypeSupported' on 'MediaSource': 1 argument required, but only 0 present.");
      return N.isTypeSupported(String(type));
    }
  }
  handlers(MediaSource.prototype, ['sourceopen', 'sourceended', 'sourceclose'], MS);

  function dropSourceBuffer(s, sb, b) {
    if (b.updating) {
      b.job++;
      b.updating = false;
      b.removing = false;
      task(function () { fire(sb, 'abort'); fire(sb, 'updateend'); });
    }
    if (listHas(s.active, sb)) listRemove(s.active, sb);
    listRemove(s.list, sb);
    b.removed = true;
    N.sbFree(b.id);
  }
  // Detaching from a media element (MSE §2.4.2).
  function detach(ms) {
    var s = MS.get(ms);
    s.readyState = 'closed';
    s.duration = NaN;
    s.api = null;
    LIST.get(s.list).items.slice().forEach(function (sb) { dropSourceBuffer(s, sb, SB.get(sb)); });
    task(function () { fire(ms, 'sourceclose'); });
  }

  // The element side attaches a MediaSource here (MSE §2.4.1). Returns null
  // for objects that are not a MediaSource, false when it cannot attach.
  defineProperty(g, '__narsilMseAttach', {
    value: function (obj, el, api) {
      var s = MS.get(obj);
      if (!s) return null;
      if (s.readyState !== 'closed') return false;
      s.readyState = 'open';
      s.api = api;
      task(function () { fire(obj, 'sourceopen'); });
      var attached = true;
      return {
        buffered: function () { return attached ? elementBuffered(s) : []; },
        ended: function () { return attached && s.readyState === 'ended'; },
        seekable: function () {
          if (!attached || s.duration !== s.duration) return [];
          if (s.duration !== Infinity) return [[0, s.duration]];
          var b = elementBuffered(s);
          if (s.live) {
            var lo = s.live[0], hi = s.live[1];
            if (b.length) { lo = Math.min(lo, b[0][0]); hi = Math.max(hi, b[b.length - 1][1]); }
            return [[lo, hi]];
          }
          return b.length ? [[0, b[b.length - 1][1]]] : [];
        },
        detach: function () {
          if (!attached) return;
          attached = false;
          detach(obj);
        }
      };
    },
    configurable: true, writable: true, enumerable: false
  });

  [['MediaSource', MediaSource], ['SourceBuffer', SourceBuffer], ['SourceBufferList', SourceBufferList]].forEach(function (p) {
    defineProperty(g, p[0], { value: p[1], writable: true, enumerable: false, configurable: true });
  });
})
