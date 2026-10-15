// IndexedDB 3.0 (https://w3c.github.io/IndexedDB/): the API objects, the
// request and event order, transaction scheduling and auto-commit. The
// storage engine is `idb.rs` (`__narsilIdb`), shared by all contexts of an
// origin; values travel structured-serialized (`__narsilSerialize`).
// Installed on first use of `indexedDB` or an IDB interface
// (`lazy_apis.js`), in pages and workers.
(function () {
  var g = globalThis;
  var host = g.__narsilIdb;
  try { delete g.__narsilIdb; } catch (e) {}
  var serialize = g.__narsilSerialize, deserialize = g.__narsilDeserialize;
  var checkpoint = g.__narsilMicrotaskCheckpoint;
  try { delete g.__narsilMicrotaskCheckpoint; } catch (e) {}
  if (typeof checkpoint !== 'function') checkpoint = function () {};
  if (typeof host !== 'function' || typeof serialize !== 'function' || typeof g.indexedDB !== 'undefined') return;
  var DE = g.DOMException;
  var BaseEvent = g.Event;
  var ET = g.EventTarget;

  function domError(name, message) { return new DE(message || name, name); }
  function typeError(op, iface, text) { return new TypeError("Failed to execute '" + op + "' on '" + iface + "': " + text); }
  // Errors of the storage engine come as "Name: text".
  function hostError(e) {
    // The engine turns "ConstraintError: ..." from the host into that
    // DOMException itself; older engines give an Error with the text.
    if (e instanceof DE) return e;
    var m = String(e && e.message !== undefined ? e.message : e);
    m = m.replace(/^(Laufzeitfehler: )?(Error: )?/, '');
    var i = m.indexOf(': ');
    var name = i > 0 ? m.slice(0, i) : 'UnknownError';
    if (!/^[A-Z][A-Za-z]+Error$/.test(name)) name = 'UnknownError';
    return domError(name, i > 0 ? m.slice(i + 2) : m);
  }
  // Names may hold surrogates; the host keeps them as text, so every string
  // argument travels escaped ("\u0001" + hex code units) when it has one.
  function enc(s) {
    if (!/[\uD800-\uDFFF]/.test(s) && s.charCodeAt(0) !== 1) return s;
    var out = '\u0001';
    for (var i = 0; i < s.length; i++) out += ('000' + s.charCodeAt(i).toString(16)).slice(-4);
    return out;
  }
  function dec(s) {
    if (typeof s !== 'string' || s.charCodeAt(0) !== 1) return s;
    var out = '';
    for (var i = 1; i + 4 <= s.length; i += 4) out += String.fromCharCode(parseInt(s.substr(i, 4), 16));
    return out;
  }
  function call() {
    var args = Array.prototype.slice.call(arguments);
    for (var i = 1; i < args.length; i++) if (typeof args[i] === 'string') args[i] = enc(args[i]);
    try { return host.apply(null, args); } catch (e) { throw hostError(e); }
  }
  function hide(o, k, v) {
    try { Object.defineProperty(o, k, { value: v, configurable: true, writable: true, enumerable: false }); } catch (e) { o[k] = v; }
  }
  function method(proto, name, fn) {
    try { Object.defineProperty(proto, name, { value: fn, configurable: true, writable: true, enumerable: true }); } catch (e) { proto[name] = fn; }
  }
  function getter(proto, name, fn, set) {
    Object.defineProperty(proto, name, { configurable: true, enumerable: true, get: fn, set: set });
  }
  function iface(name, parent, ctor) {
    var C = ctor || function () { throw new TypeError('Illegal constructor'); };
    try { Object.defineProperty(C, 'name', { value: name, configurable: true }); } catch (e) {}
    C.prototype = Object.create(parent ? parent.prototype : Object.prototype);
    hide(C.prototype, 'constructor', C);
    if (typeof Symbol === 'function' && Symbol.toStringTag) {
      try { Object.defineProperty(C.prototype, Symbol.toStringTag, { value: name, configurable: true }); } catch (e) {}
    }
    if (parent) { try { Object.setPrototypeOf(C, parent); } catch (e) {} }
    hide(g, name, C);
    return C;
  }
  var later = typeof g.setTimeout === 'function' ? function (f) { g.setTimeout(f, 0); } : function (f) { Promise.resolve().then(f); };
  function report(e) {
    try { if (typeof g.reportError === 'function') { g.reportError(e); return; } } catch (x) {}
    try { g.console.error('Uncaught', e); } catch (x) {}
  }

  // -------------------------------------------------------------------
  // Events with a propagation path (request -> transaction -> database)
  // -------------------------------------------------------------------
  var EVT = new WeakMap();
  function slots(t) {
    var s = EVT.get(t);
    if (!s) { s = { l: Object.create(null), h: Object.create(null) }; EVT.set(t, s); }
    return s;
  }
  function flatten(o) {
    if (typeof o === 'boolean') return { capture: o, once: false, signal: null };
    if (o && typeof o === 'object') return { capture: !!o.capture, once: !!o.once, signal: o.signal || null };
    return { capture: false, once: false, signal: null };
  }
  function addL(t, type, cb, opts) {
    var o = flatten(opts);
    if (cb === null || cb === undefined) return;
    if (o.signal && o.signal.aborted) return;
    var list = slots(t).l[type] || (slots(t).l[type] = []);
    for (var i = 0; i < list.length; i++) if (list[i].cb === cb && list[i].capture === o.capture) return;
    var rec = { cb: cb, capture: o.capture, once: o.once, removed: false };
    list.push(rec);
    if (o.signal && typeof o.signal.addEventListener === 'function') o.signal.addEventListener('abort', function () { removeRec(t, type, rec); });
  }
  function removeRec(t, type, rec) {
    var list = slots(t).l[type];
    if (!list) return;
    var i = list.indexOf(rec);
    if (i >= 0) { list.splice(i, 1); rec.removed = true; }
  }
  function removeL(t, type, cb, opts) {
    var o = flatten(opts);
    var list = slots(t).l[type];
    if (!list) return;
    for (var i = 0; i < list.length; i++) {
      if (list[i].cb === cb && list[i].capture === o.capture) { list[i].removed = true; list.splice(i, 1); return; }
    }
  }
  function setField(ev, k, v) {
    try { Object.defineProperty(ev, k, { value: v, configurable: true, enumerable: true, writable: true }); } catch (e) { try { ev[k] = v; } catch (x) {} }
  }
  var DISPATCHING = new WeakSet();
  // Dispatch along the path; returns { notCanceled, threw }.
  function dispatchPath(path, ev) {
    if (DISPATCHING.has(ev)) throw domError('InvalidStateError', "Failed to execute 'dispatchEvent' on 'EventTarget': The event is already being dispatched.");
    DISPATCHING.add(ev);
    var stop = false, stopNow = false, threw = false;
    hide(ev, 'stopPropagation', function stopPropagation() { stop = true; });
    hide(ev, 'stopImmediatePropagation', function stopImmediatePropagation() { stop = true; stopNow = true; });
    hide(ev, 'cancelBubble', false);
    setField(ev, 'target', path[0]);
    var type = String(ev.type);
    function run(t, phase, wantCapture) {
      setField(ev, 'currentTarget', t);
      setField(ev, 'eventPhase', phase);
      var list = (slots(t).l[type] || []).slice();
      for (var i = 0; i < list.length && !stopNow; i++) {
        var rec = list[i];
        if (rec.removed) continue;
        if (phase !== 2 && rec.capture !== wantCapture) continue;
        if (rec.once) removeRec(t, type, rec);
        try {
          if (typeof rec.cb === 'function') rec.cb.call(t, ev);
          else {
            var he = rec.cb.handleEvent;
            if (typeof he !== 'function') throw new TypeError("The provided callback's 'handleEvent' property is not callable.");
            he.call(rec.cb, ev);
          }
        } catch (e) {
          threw = true;
          report(e);
        }
        // "Clean up after running a callback": the microtasks run now, while
        // the transaction is still active (IndexedDB event-dispatch tests);
        // then transactions made in the callback become inactive.
        try { checkpoint(); } catch (e) {}
        deactivateNew();
      }
    }
    for (var i = path.length - 1; i >= 1 && !stop; i--) run(path[i], 1, true);
    if (!stop) run(path[0], 2, null);
    if (ev.bubbles) for (var j = 1; j < path.length && !stop; j++) run(path[j], 3, false);
    setField(ev, 'currentTarget', null);
    setField(ev, 'eventPhase', 0);
    try { delete ev.stopPropagation; delete ev.stopImmediatePropagation; delete ev.cancelBubble; } catch (e) {}
    DISPATCHING.delete(ev);
    return { notCanceled: !ev.defaultPrevented, threw: threw };
  }
  function parentsOf(t) {
    var path = [t];
    var p = PARENT.get(t);
    while (p) { path.push(p); p = PARENT.get(p); }
    return path;
  }
  var PARENT = new WeakMap();
  function targetMethods(proto) {
    method(proto, 'addEventListener', function addEventListener(type, cb, opts) {
      if (arguments.length < 2) throw typeError('addEventListener', 'EventTarget', '2 arguments required, but only ' + arguments.length + ' present.');
      addL(this, String(type), cb, opts);
    });
    method(proto, 'removeEventListener', function removeEventListener(type, cb, opts) {
      if (arguments.length < 2) throw typeError('removeEventListener', 'EventTarget', '2 arguments required, but only ' + arguments.length + ' present.');
      removeL(this, String(type), cb, opts);
    });
    method(proto, 'dispatchEvent', function dispatchEvent(ev) {
      if (!(ev instanceof BaseEvent)) throw typeError('dispatchEvent', 'EventTarget', "parameter 1 is not of type 'Event'.");
      setField(ev, 'isTrusted', false);
      return dispatchPath(parentsOf(this), ev).notCanceled;
    });
  }
  function handler(proto, type) {
    Object.defineProperty(proto, 'on' + type, {
      configurable: true, enumerable: true,
      get: function () { var h = slots(this).h[type]; return h ? h.fn : null; },
      set: function (v) {
        var s = slots(this);
        var fn = (typeof v === 'function' || (v !== null && typeof v === 'object')) ? v : null;
        var old = s.h[type];
        if (fn === null) { if (old) { removeRec(this, type, old.rec); delete s.h[type]; } return; }
        if (old) { old.fn = fn; return; }
        var holder = { fn: fn, rec: null }, target = this;
        addL(this, type, function (ev) {
          var f = holder.fn;
          if (typeof f !== 'function') return;
          var r = f.call(target, ev);
          if (r === false && ev.cancelable) ev.preventDefault();
        }, false);
        var list = s.l[type];
        holder.rec = list[list.length - 1];
        s.h[type] = holder;
      }
    });
  }
  function makeEvent(type, bubbles, cancelable) {
    var ev = new BaseEvent(type, { bubbles: !!bubbles, cancelable: !!cancelable });
    setField(ev, 'isTrusted', true);
    return ev;
  }

  // IDBVersionChangeEvent (IndexedDB §4.2)
  var VCE = new WeakMap();
  var IDBVersionChangeEvent = iface('IDBVersionChangeEvent', BaseEvent, function IDBVersionChangeEvent(type, init) {
    if (!new.target) throw new TypeError("Failed to construct 'IDBVersionChangeEvent': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
    if (arguments.length < 1) throw new TypeError("Failed to construct 'IDBVersionChangeEvent': 1 argument required, but only 0 present.");
    var o = init || {};
    var e = new BaseEvent(type, o);
    try { Object.setPrototypeOf(e, new.target.prototype); } catch (x) {}
    VCE.set(e, { oldVersion: o.oldVersion === undefined ? 0 : Number(o.oldVersion), newVersion: o.newVersion === undefined || o.newVersion === null ? null : Number(o.newVersion) });
    return e;
  });
  getter(IDBVersionChangeEvent.prototype, 'oldVersion', function () { var s = VCE.get(this); if (!s) throw new TypeError('Illegal invocation'); return s.oldVersion; });
  getter(IDBVersionChangeEvent.prototype, 'newVersion', function () { var s = VCE.get(this); if (!s) throw new TypeError('Illegal invocation'); return s.newVersion; });
  function versionEvent(type, oldV, newV) {
    var e = new IDBVersionChangeEvent(type, { oldVersion: oldV, newVersion: newV });
    setField(e, 'isTrusted', true);
    return e;
  }

  // DOMStringList (HTML §2.7.4)
  var DOMStringList = iface('DOMStringList');
  function stringList(arr) {
    var l = Object.create(DOMStringList.prototype);
    for (var i = 0; i < arr.length; i++) Object.defineProperty(l, i, { value: arr[i], enumerable: true, configurable: false, writable: false });
    hide(l, '__items', arr.slice());
    return l;
  }
  getter(DOMStringList.prototype, 'length', function () { return (this.__items || []).length; });
  method(DOMStringList.prototype, 'item', function item(i) { var a = this.__items || []; i = Number(i) >>> 0; return i < a.length ? a[i] : null; });
  method(DOMStringList.prototype, 'contains', function contains(s) { return (this.__items || []).indexOf(String(s)) >= 0; });
  if (typeof Symbol === 'function' && Symbol.iterator) {
    method(DOMStringList.prototype, Symbol.iterator, function () { return (this.__items || []).slice()[Symbol.iterator](); });
  }

  // -------------------------------------------------------------------
  // Keys (IndexedDB §2.5, §7.3)
  // -------------------------------------------------------------------
  var INVALID = {};
  function isBufferSource(v) {
    return v instanceof ArrayBuffer || (typeof ArrayBuffer.isView === 'function' && ArrayBuffer.isView(v));
  }
  function bytesOf(v) {
    if (v instanceof ArrayBuffer) return new Uint8Array(v);
    return new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
  }
  function detachedView(v) {
    try { return (v instanceof ArrayBuffer ? v : v.buffer).byteLength === 0 && (v.byteLength === 0) && isDetachedBuffer(v instanceof ArrayBuffer ? v : v.buffer); } catch (e) { return true; }
  }
  function isDetachedBuffer(b) {
    try { new Uint8Array(b); return false; } catch (e) { return true; }
  }
  // convert a value to a key; INVALID if it is none (`seen` for cycles).
  // A key is a text of `;`-terminated tokens (`idb.rs` `Key::parse`), so
  // that keys of any size cross the host boundary: n<number>, d<ms>,
  // s<string> (`\` -> `\\`, `;` -> `\:`), u<code units> (strings with
  // surrogates), b<hex>, and a ... e around the items of an array.
  var HEX = [];
  for (var h = 0; h < 256; h++) HEX.push((h < 16 ? '0' : '') + h.toString(16));
  function toKey(v, seen) {
    if (typeof v === 'number') return v !== v ? INVALID : 'n' + v + ';';
    if (typeof v === 'string') {
      if (!/[\uD800-\uDFFF]/.test(v)) {
        return 's' + (/[\\;]/.test(v) ? v.replace(/\\/g, '\\\\').replace(/;/g, '\\:') : v) + ';';
      }
      var units = [];
      for (var u = 0; u < v.length; u++) units.push(v.charCodeAt(u));
      return 'u' + units.join(',') + ';';
    }
    if (v instanceof Date) {
      var t = v.getTime();
      return t !== t ? INVALID : 'd' + t + ';';
    }
    if (isBufferSource(v)) {
      var buf = v instanceof ArrayBuffer ? v : v.buffer;
      // A detached buffer (transferred away) is no key; it alone fails to
      // serialize, and it always has byteLength 0.
      if (buf.byteLength === 0) { try { serialize(buf, [], [], []); } catch (e) { return INVALID; } }
      var b = bytesOf(v), hex = [];
      for (var j = 0; j < b.length; j++) hex.push(HEX[b[j]]);
      return 'b' + hex.join('') + ';';
    }
    if (Array.isArray(v)) {
      seen = seen || [];
      if (seen.indexOf(v) >= 0) return INVALID;
      seen.push(v);
      var len = v.length, out = ['a;'];
      for (var i = 0; i < len; i++) {
        if (!Object.prototype.hasOwnProperty.call(v, i)) return INVALID;
        var k = toKey(v[i], seen);
        if (k === INVALID) return INVALID;
        out.push(k);
      }
      seen.pop();
      out.push('e;');
      return out.join('');
    }
    return INVALID;
  }
  function keyOrThrow(v, what) {
    var k = toKey(v);
    if (k === INVALID) throw domError('DataError', (what || 'The parameter') + ' is not a valid key.');
    return k;
  }
  // encoded key -> script value (one split, then a walk over the tokens)
  function fromKey(k) {
    if (typeof k !== 'string' || !k) return undefined;
    var tokens = k.split(';'), stack = [], cur = null;
    for (var i = 0; i < tokens.length; i++) {
      var t = tokens[i], kind = t.charAt(0), body = t.slice(1), v;
      if (kind === 'a') { stack.push(cur); cur = []; continue; }
      if (kind === 'e') { v = cur; cur = stack.pop(); }
      else if (kind === 'n') v = Number(body);
      else if (kind === 'd') v = new Date(Number(body));
      else if (kind === 's') v = body.indexOf('\\') < 0 ? body : body.replace(/\\([\\:])/g, function (m, c) { return c === ':' ? ';' : '\\'; });
      else if (kind === 'u') { var us = body.split(','); v = ''; for (var j = 0; j < us.length; j++) v += String.fromCharCode(Number(us[j])); }
      else if (kind === 'b') { var pairs = body.match(/[0-9a-f]{2}/g) || [], bytes = new Uint8Array(pairs.length); for (var x = 0; x < pairs.length; x++) bytes[x] = parseInt(pairs[x], 16); v = bytes.buffer; }
      else return undefined;
      if (cur === null) return v;
      cur.push(v);
    }
    return undefined;
  }
  function cmpKeys(a, b) { return call('cmp', a, b); }

  // Key paths (IndexedDB §2.6, §7.4)
  var IDENT = /^[A-Za-z_$ª-￿][A-Za-z0-9_$ª-￿]*$/;
  function validPath(p) {
    if (typeof p !== 'string') return false;
    if (p === '') return true;
    var parts = p.split('.');
    for (var i = 0; i < parts.length; i++) if (!IDENT.test(parts[i])) return false;
    return true;
  }
  function normPath(kp) {
    if (kp === undefined || kp === null) return null;
    if (Array.isArray(kp) || (typeof kp === 'object' && typeof kp[Symbol.iterator] === 'function' && typeof kp !== 'string')) {
      var arr = [];
      for (var x of kp) arr.push(String(x));
      return arr;
    }
    return String(kp);
  }
  function pathOk(kp) {
    if (kp === null) return true;
    if (Array.isArray(kp)) {
      if (kp.length === 0) return false;
      for (var i = 0; i < kp.length; i++) if (!validPath(kp[i])) return false;
      return true;
    }
    return validPath(kp);
  }
  function pathJson(kp) { return JSON.stringify(kp); }
  function pathOf(json) {
    if (json === null || json === undefined) return null;
    try { return JSON.parse(json); } catch (e) { return json; }
  }
  function evalStep(value, id) {
    if (typeof value === 'string' && id === 'length') return { ok: true, v: value.length };
    if (value === null || (typeof value !== 'object' && typeof value !== 'function')) return { ok: false };
    if (Array.isArray(value) && id === 'length') return { ok: true, v: value.length };
    if (typeof g.Blob === 'function' && value instanceof g.Blob) {
      if (id === 'size') return { ok: true, v: value.size };
      if (id === 'type') return { ok: true, v: value.type };
      if (typeof g.File === 'function' && value instanceof g.File) {
        if (id === 'name') return { ok: true, v: value.name };
        if (id === 'lastModified') return { ok: true, v: value.lastModified };
        if (id === 'lastModifiedDate') return { ok: true, v: new Date(value.lastModified) };
      }
    }
    if (!Object.prototype.hasOwnProperty.call(value, id)) return { ok: false };
    return { ok: true, v: value[id] };
  }
  // evaluate a key path on a value -> { ok, v }
  function evalPath(value, kp) {
    if (Array.isArray(kp)) {
      var out = [];
      for (var i = 0; i < kp.length; i++) {
        var r = evalPath(value, kp[i]);
        if (!r.ok) return { ok: false };
        out.push(r.v);
      }
      return { ok: true, v: out };
    }
    if (kp === '') return { ok: true, v: value };
    var parts = kp.split('.'), cur = value;
    for (var j = 0; j < parts.length; j++) {
      var s = evalStep(cur, parts[j]);
      if (!s.ok) return { ok: false };
      cur = s.v;
    }
    return { ok: true, v: cur };
  }
  function keyFromValue(value, kp) {
    var r = evalPath(value, kp);
    if (!r.ok) return null;
    var k = toKey(r.v);
    return k === INVALID ? INVALID : k;
  }
  // "check that a key could be injected into a value"
  function canInject(value, kp) {
    var parts = kp.split('.');
    parts.pop();
    var cur = value;
    for (var i = 0; i < parts.length; i++) {
      if (cur === null || (typeof cur !== 'object' && typeof cur !== 'function')) return false;
      if (!Object.prototype.hasOwnProperty.call(cur, parts[i])) return true;
      cur = cur[parts[i]];
    }
    return cur !== null && (typeof cur === 'object' || typeof cur === 'function');
  }
  function inject(value, kp, key) {
    var parts = kp.split('.'), last = parts.pop(), cur = value;
    for (var i = 0; i < parts.length; i++) {
      if (!Object.prototype.hasOwnProperty.call(cur, parts[i])) {
        Object.defineProperty(cur, parts[i], { value: {}, writable: true, enumerable: true, configurable: true });
      }
      cur = cur[parts[i]];
    }
    Object.defineProperty(cur, last, { value: fromKey(key), writable: true, enumerable: true, configurable: true });
  }
  // the keys of one record for an index (multiEntry: the subkeys)
  function indexKeysFor(value, ix) {
    var r = evalPath(value, ix.keyPath);
    if (!r.ok) return [];
    if (ix.multiEntry && Array.isArray(r.v)) {
      var out = [], seen = [];
      for (var i = 0; i < r.v.length; i++) {
        var k = toKey(r.v[i]);
        if (k !== INVALID) out.push(k);
      }
      return out;
    }
    var key = toKey(r.v);
    return key === INVALID ? [] : [key];
  }

  // -------------------------------------------------------------------
  // IDBKeyRange (IndexedDB §4.7)
  // -------------------------------------------------------------------
  var KR = new WeakMap();
  var IDBKeyRange = iface('IDBKeyRange');
  function makeRange(lower, upper, lo, uo) {
    var r = Object.create(IDBKeyRange.prototype);
    KR.set(r, { lower: lower, upper: upper, lowerOpen: !!lo, upperOpen: !!uo });
    return r;
  }
  function krs(r) { var s = KR.get(r); if (!s) throw new TypeError('Illegal invocation'); return s; }
  getter(IDBKeyRange.prototype, 'lower', function () { return fromKey(krs(this).lower); });
  getter(IDBKeyRange.prototype, 'upper', function () { return fromKey(krs(this).upper); });
  getter(IDBKeyRange.prototype, 'lowerOpen', function () { return krs(this).lowerOpen; });
  getter(IDBKeyRange.prototype, 'upperOpen', function () { return krs(this).upperOpen; });
  method(IDBKeyRange.prototype, 'includes', function includes(key) {
    var s = krs(this);
    if (arguments.length < 1) throw typeError('includes', 'IDBKeyRange', '1 argument required, but only 0 present.');
    var k = keyOrThrow(key);
    if (s.lower) { var c = cmpKeys(k, s.lower); if (c < 0 || (c === 0 && s.lowerOpen)) return false; }
    if (s.upper) { var d = cmpKeys(k, s.upper); if (d > 0 || (d === 0 && s.upperOpen)) return false; }
    return true;
  });
  hide(IDBKeyRange, 'only', function only(v) {
    if (arguments.length < 1) throw typeError('only', 'IDBKeyRange', '1 argument required, but only 0 present.');
    var k = keyOrThrow(v);
    return makeRange(k, k, false, false);
  });
  hide(IDBKeyRange, 'lowerBound', function lowerBound(v, open) {
    if (arguments.length < 1) throw typeError('lowerBound', 'IDBKeyRange', '1 argument required, but only 0 present.');
    return makeRange(keyOrThrow(v), null, !!open, true);
  });
  hide(IDBKeyRange, 'upperBound', function upperBound(v, open) {
    if (arguments.length < 1) throw typeError('upperBound', 'IDBKeyRange', '1 argument required, but only 0 present.');
    return makeRange(null, keyOrThrow(v), true, !!open);
  });
  hide(IDBKeyRange, 'bound', function bound(lower, upper, lo, uo) {
    if (arguments.length < 2) throw typeError('bound', 'IDBKeyRange', '2 arguments required, but only ' + arguments.length + ' present.');
    var l = keyOrThrow(lower), u = keyOrThrow(upper);
    var c = cmpKeys(l, u);
    if (c > 0 || (c === 0 && (lo || uo))) throw domError('DataError', "Failed to execute 'bound' on 'IDBKeyRange': The lower key is greater than the upper key.");
    return makeRange(l, u, !!lo, !!uo);
  });
  // "convert a value to a key range": null/undefined -> all (if allowed)
  function toRange(v, nullOk) {
    if (v instanceof IDBKeyRange) {
      var s = krs(v);
      return [0, s.lower, s.upper, s.lowerOpen, s.upperOpen];
    }
    if (v === undefined || v === null) {
      if (nullOk) return null;
      throw domError('DataError', 'No key or key range specified.');
    }
    var k = keyOrThrow(v, 'The parameter');
    return [0, k, k, false, false];
  }

  // -------------------------------------------------------------------
  // IDBRequest, IDBOpenDBRequest (IndexedDB §4.1)
  // -------------------------------------------------------------------
  var RQ = new WeakMap();
  var IDBRequest = iface('IDBRequest', ET);
  targetMethods(IDBRequest.prototype);
  ['success', 'error'].forEach(function (t) { handler(IDBRequest.prototype, t); });
  var IDBOpenDBRequest = iface('IDBOpenDBRequest', IDBRequest);
  ['blocked', 'upgradeneeded'].forEach(function (t) { handler(IDBOpenDBRequest.prototype, t); });
  function rq(r) { var s = RQ.get(r); if (!s) throw new TypeError('Illegal invocation'); return s; }
  getter(IDBRequest.prototype, 'result', function () {
    var s = rq(this);
    if (!s.done) throw domError('InvalidStateError', "Failed to read the 'result' property from 'IDBRequest': The request has not finished.");
    return s.result;
  });
  getter(IDBRequest.prototype, 'error', function () {
    var s = rq(this);
    if (!s.done) throw domError('InvalidStateError', "Failed to read the 'error' property from 'IDBRequest': The request has not finished.");
    return s.error;
  });
  getter(IDBRequest.prototype, 'source', function () { return rq(this).source; });
  getter(IDBRequest.prototype, 'transaction', function () { return rq(this).transaction; });
  getter(IDBRequest.prototype, 'readyState', function () { return rq(this).done ? 'done' : 'pending'; });
  function newRequest(source, txn, open) {
    var r = Object.create((open ? IDBOpenDBRequest : IDBRequest).prototype);
    RQ.set(r, { done: false, result: undefined, error: null, source: source || null, transaction: txn || null });
    if (txn) PARENT.set(r, txn);
    return r;
  }

  // -------------------------------------------------------------------
  // Connections, the factory (IndexedDB §4.3, §4.4)
  // -------------------------------------------------------------------
  var DB = new WeakMap();
  var CONNECTIONS = [];      // open connections of this context
  var TXNS = [];             // live transactions in creation order
  var QUEUES = Object.create(null); // name -> open/delete requests in order

  function dbs(d) { var s = DB.get(d); if (!s) throw new TypeError('Illegal invocation'); return s; }
  function storesOf(name) {
    var list = call('stores', name);
    var out = Object.create(null);
    for (var i = 0; i < list.length; i++) {
      var e = list[i], ix = Object.create(null);
      for (var j = 0; j < e[3].length; j++) {
        var x = e[3][j];
        ix[dec(x[0])] = { name: dec(x[0]), keyPath: pathOf(dec(x[1])), unique: !!x[2], multiEntry: !!x[3] };
      }
      out[dec(e[0])] = { name: dec(e[0]), keyPath: pathOf(dec(e[1])), autoIncrement: !!e[2], indexes: ix };
    }
    return out;
  }
  // A copy of the store metadata (maps without prototype: a store may be
  // called "__proto__" or "toString").
  function cloneStores(stores) {
    var out = Object.create(null);
    for (var n in stores) {
      var st = stores[n], ix = Object.create(null);
      for (var m in st.indexes) {
        var x = st.indexes[m];
        ix[m] = { name: x.name, keyPath: Array.isArray(x.keyPath) ? x.keyPath.slice() : x.keyPath, unique: x.unique, multiEntry: x.multiEntry };
      }
      out[n] = { name: st.name, keyPath: Array.isArray(st.keyPath) ? st.keyPath.slice() : st.keyPath, autoIncrement: st.autoIncrement, indexes: ix };
    }
    return out;
  }
  // Sorted by code units (DOMStringList order) — what `<` on strings does.
  function sortedNames(obj) {
    return Object.keys(obj).sort();
  }

  var IDBDatabase = iface('IDBDatabase', ET);
  targetMethods(IDBDatabase.prototype);
  ['abort', 'close', 'error', 'versionchange'].forEach(function (t) { handler(IDBDatabase.prototype, t); });
  getter(IDBDatabase.prototype, 'name', function () { return dbs(this).name; });
  getter(IDBDatabase.prototype, 'version', function () { return dbs(this).version; });
  getter(IDBDatabase.prototype, 'objectStoreNames', function () { return stringList(sortedNames(dbs(this).stores)); });
  method(IDBDatabase.prototype, 'close', function close() { closeConnection(this, false); });
  function closeConnection(d, forced) {
    var s = dbs(d);
    if (s.closePending) return;
    s.closePending = true;
    var i = CONNECTIONS.indexOf(d);
    if (i >= 0) CONNECTIONS.splice(i, 1);
    if (forced) {
      TXNS.slice().forEach(function (t) { if (txs(t).db === d && txs(t).state !== 'finished') abortTxn(t, domError('AbortError', 'The connection was closed.')); });
      later(function () { dispatchPath([d], makeEvent('close')); });
    }
    later(function () { pumpQueues(); schedule(); });
  }
  method(IDBDatabase.prototype, 'createObjectStore', function createObjectStore(name, options) {
    var s = dbs(this);
    if (arguments.length < 1) throw typeError('createObjectStore', 'IDBDatabase', '1 argument required, but only 0 present.');
    name = String(name);
    var o = options === undefined || options === null ? {} : options;
    var kp = normPath(o.keyPath);
    var auto = !!o.autoIncrement;
    var t = s.upgrade;
    if (!t || txs(t).done) throw domError('InvalidStateError', "Failed to execute 'createObjectStore' on 'IDBDatabase': The database is not running a version change transaction.");
    if (txs(t).state !== 'active') throw domError('TransactionInactiveError', "Failed to execute 'createObjectStore' on 'IDBDatabase': The transaction is not active.");
    if (!pathOk(kp)) throw domError('SyntaxError', "Failed to execute 'createObjectStore' on 'IDBDatabase': The keyPath option is not a valid key path.");
    if (s.stores[name]) throw domError('ConstraintError', "Failed to execute 'createObjectStore' on 'IDBDatabase': An object store with the specified name already exists.");
    if (auto && (kp === '' || Array.isArray(kp))) throw domError('InvalidAccessError', "Failed to execute 'createObjectStore' on 'IDBDatabase': The autoIncrement option was set but the keyPath option was empty or an array.");
    var ts2 = txs(t), dbn2 = s.name;
    queueInternal(t, function () { call('create_store', ts2.id, dbn2, name, kp === null ? null : pathJson(kp), auto); });
    s.stores[name] = { name: name, keyPath: kp, autoIncrement: auto, indexes: Object.create(null) };
    if (ts2.scope.indexOf(name) < 0) ts2.scope.push(name);
    var so = storeObject(t, name);
    OS.get(so).createdIn = t;
    return so;
  });
  method(IDBDatabase.prototype, 'deleteObjectStore', function deleteObjectStore(name) {
    var s = dbs(this);
    if (arguments.length < 1) throw typeError('deleteObjectStore', 'IDBDatabase', '1 argument required, but only 0 present.');
    name = String(name);
    var t = s.upgrade;
    if (!t || txs(t).done) throw domError('InvalidStateError', "Failed to execute 'deleteObjectStore' on 'IDBDatabase': The database is not running a version change transaction.");
    if (txs(t).state !== 'active') throw domError('TransactionInactiveError', "Failed to execute 'deleteObjectStore' on 'IDBDatabase': The transaction is not active.");
    if (!s.stores[name]) throw domError('NotFoundError', "Failed to execute 'deleteObjectStore' on 'IDBDatabase': The specified object store was not found.");
    var ts = txs(t), dbn = s.name;
    queueInternal(t, function () { call('delete_store', ts.id, dbn, name); });
    delete s.stores[name];
    var cache = ts.storeObjects[name];
    if (cache) {
      OS.get(cache).deleted = true;
      delete ts.storeObjects[name];
      (ts.deletedObjects || (ts.deletedObjects = [])).push(cache);
    }
  });
  method(IDBDatabase.prototype, 'transaction', function transaction(storeNames, mode, options) {
    var s = dbs(this);
    if (arguments.length < 1) throw typeError('transaction', 'IDBDatabase', '1 argument required, but only 0 present.');
    if (s.upgrade && txs(s.upgrade).state !== 'finished') throw domError('InvalidStateError', "Failed to execute 'transaction' on 'IDBDatabase': A version change transaction is running.");
    if (s.closePending) throw domError('InvalidStateError', "Failed to execute 'transaction' on 'IDBDatabase': The database connection is closing.");
    var names;
    if (typeof storeNames === 'string') names = [storeNames];
    else if (storeNames && typeof storeNames[Symbol.iterator] === 'function') { names = []; for (var x of storeNames) names.push(String(x)); }
    else names = [String(storeNames)];
    var uniq = [];
    for (var i = 0; i < names.length; i++) {
      if (!s.stores[names[i]]) throw domError('NotFoundError', "Failed to execute 'transaction' on 'IDBDatabase': One of the specified object stores was not found.");
      if (uniq.indexOf(names[i]) < 0) uniq.push(names[i]);
    }
    if (uniq.length === 0) throw domError('InvalidAccessError', "Failed to execute 'transaction' on 'IDBDatabase': The storeNames parameter was empty.");
    var m = mode === undefined ? 'readonly' : String(mode);
    if (m !== 'readonly' && m !== 'readwrite') throw new TypeError("Failed to execute 'transaction' on 'IDBDatabase': The provided value '" + m + "' is not a valid enum value of type IDBTransactionMode.");
    var durability = options && options.durability !== undefined ? String(options.durability) : 'default';
    if (['default', 'strict', 'relaxed'].indexOf(durability) < 0) throw new TypeError("Failed to execute 'transaction' on 'IDBDatabase': The provided value '" + durability + "' is not a valid enum value of type IDBTransactionDurability.");
    return newTransaction(this, uniq, m, durability);
  });

  // -------------------------------------------------------------------
  // IDBTransaction (IndexedDB §4.9) and the scheduler
  // -------------------------------------------------------------------
  var TX = new WeakMap();
  var IDBTransaction = iface('IDBTransaction', ET);
  targetMethods(IDBTransaction.prototype);
  ['abort', 'complete', 'error'].forEach(function (t) { handler(IDBTransaction.prototype, t); });
  function txs(t) { var s = TX.get(t); if (!s) throw new TypeError('Illegal invocation'); return s; }
  getter(IDBTransaction.prototype, 'objectStoreNames', function () {
    var s = txs(this);
    var names = s.mode === 'versionchange' ? sortedNames(dbs(s.db).stores) : s.scope.slice().sort();
    return stringList(names);
  });
  getter(IDBTransaction.prototype, 'mode', function () { return txs(this).mode; });
  getter(IDBTransaction.prototype, 'durability', function () { return txs(this).durability; });
  getter(IDBTransaction.prototype, 'db', function () { return txs(this).db; });
  getter(IDBTransaction.prototype, 'error', function () { return txs(this).error; });
  method(IDBTransaction.prototype, 'objectStore', function objectStore(name) {
    var s = txs(this);
    if (arguments.length < 1) throw typeError('objectStore', 'IDBTransaction', '1 argument required, but only 0 present.');
    if (s.state === 'finished') throw domError('InvalidStateError', "Failed to execute 'objectStore' on 'IDBTransaction': The transaction has finished.");
    name = String(name);
    var stores = dbs(s.db).stores;
    if ((s.mode !== 'versionchange' && s.scope.indexOf(name) < 0) || !stores[name]) throw domError('NotFoundError', "Failed to execute 'objectStore' on 'IDBTransaction': The specified object store was not found.");
    return storeObject(this, name);
  });
  method(IDBTransaction.prototype, 'abort', function abort() {
    var s = txs(this);
    if (s.state === 'committing' || s.state === 'finished') throw domError('InvalidStateError', "Failed to execute 'abort' on 'IDBTransaction': The transaction has already been committed or aborted.");
    abortTxn(this, null);
  });
  method(IDBTransaction.prototype, 'commit', function commit() {
    var s = txs(this);
    if (s.state !== 'active') throw domError('InvalidStateError', "Failed to execute 'commit' on 'IDBTransaction': The transaction is not active.");
    s.explicitCommit = true;
    s.state = 'committing';
    schedule();
  });
  var FRESH = [];
  function deactivateNew() {
    var list = FRESH.splice(0);
    for (var i = 0; i < list.length; i++) {
      var s = TX.get(list[i]);
      if (s && s.state === 'active' && !s.running) s.state = 'inactive';
    }
    if (list.length) schedule();
  }
  function newTransaction(db, scope, mode, durability) {
    var t = Object.create(IDBTransaction.prototype);
    var s = { db: db, scope: scope, mode: mode, durability: durability, state: 'active', started: false, id: 0, requests: [],
      pending: [], running: false, error: null, storeObjects: Object.create(null), explicitCommit: false, onFinish: null };
    TX.set(t, s);
    PARENT.set(t, db);
    TXNS.push(t);
    FRESH.push(t);
    // inactive when the task that made it ends
    later(function () { if (s.state === 'active' && !s.running) { s.state = 'inactive'; } schedule(); });
    schedule();
    return t;
  }
  function overlaps(a, b) {
    if (a.db !== b.db && dbs(a.db).name !== dbs(b.db).name) return false;
    if (a.mode === 'versionchange' || b.mode === 'versionchange') return true;
    for (var i = 0; i < a.scope.length; i++) if (b.scope.indexOf(a.scope[i]) >= 0) return true;
    return false;
  }
  function canStart(t) {
    var s = txs(t);
    for (var i = 0; i < TXNS.length; i++) {
      var o = TXNS[i];
      if (o === t) return true;
      var os = txs(o);
      if (os.done) continue;
      if (!overlaps(s, os)) continue;
      if (s.mode === 'readonly' && os.mode === 'readonly') continue;
      return false;
    }
    return true;
  }
  var scheduled = false;
  function schedule() {
    if (scheduled) return;
    scheduled = true;
    later(drain);
  }
  // Work in one task for a few milliseconds: each request result is its
  // own step, and the next step waits behind the microtasks the event
  // handlers queued (FIFO), so promise reactions see the transaction
  // inactive as in a task of its own.
  function drain() {
    scheduled = false;
    var start = Date.now();
    var go = function () {
      if (!step()) return;
      if (Date.now() - start > 8) { schedule(); return; }
      Promise.resolve().then(go);
    };
    go();
  }
  // A transaction without outstanding requests that became inactive
  // commits (IndexedDB §2.7.2 "auto-commit").
  function maybeCommit(t) {
    var s = txs(t);
    if (s.state === 'inactive' && s.started && !s.running && !s.pending.length && !s.waiting) commitTxn(t);
  }
  // One step of work: the next request of a startable transaction, or a
  // commit. False if there was nothing to do.
  // Round robin over the transactions: two running transactions take turns
  // (two readonly transactions run in parallel, IndexedDB §2.7.2).
  var turn = 0;
  function step() {
    var n = TXNS.length, i, t, s;
    // Starts and commits go in creation order (complete events keep it).
    for (i = 0; i < n; i++) {
      t = TXNS[i]; s = txs(t);
      if (s.state === 'finished') continue;
      if (!s.started) {
        if (!canStart(t)) continue;
        s.started = true;
        if (!s.id) s.id = call('begin', dbs(s.db).name, 0);
      }
      if (!s.pending.length && (s.state === 'inactive' || s.state === 'committing') && !s.running && !s.waiting) {
        commitTxn(t);
        return true;
      }
    }
    // Requests take turns between the running transactions.
    for (var c = 0; c < n; c++) {
      i = (turn + c) % n;
      t = TXNS[i]; s = txs(t);
      if (s.state === 'finished' || !s.started || !s.pending.length) continue;
      var req = s.pending.shift();
      turn = i + 1;
      if (req.abort) { abortTxn(t, req.abort); return true; }
      if (req.internal) {
        try { req.internal(); } catch (e) { abortTxn(t, e instanceof DE ? e : hostError(e)); }
        maybeCommit(t);
        return true;
      }
      runRequest(t, s, req);
      maybeCommit(t);
      return true;
    }
    // drop finished transactions
    for (var j = TXNS.length - 1; j >= 0; j--) if (txs(TXNS[j]).done) TXNS.splice(j, 1);
    return false;
  }
  function runRequest(t, s, item) {
    var r = item.request, st = RQ.get(r);
    var result, error = null;
    try { result = item.op(); } catch (e) { error = e instanceof DE ? e : hostError(e); }
    if (s.state === 'finished') return;
    st.done = true;
    if (error) {
      st.result = undefined; st.error = error;
      // After commit() the transaction stays inactive in the callbacks.
      var prev = s.state;
      if (prev !== 'committing') s.state = 'active';
      s.running = true;
      var out = dispatchPath(parentsOf(r), makeEvent('error', true, true));
      s.running = false;
      if (s.state === 'active') s.state = prev === 'committing' ? 'committing' : 'inactive';
      // An error nobody handled aborts, also after commit(); an exception in
      // a handler does not stop an explicit commit.
      if (s.state !== 'finished' && (out.notCanceled || (out.threw && !s.explicitCommit))) {
        abortTxn(t, out.threw ? domError('AbortError', 'An exception was thrown in an error handler.') : error);
      }
    } else {
      st.result = result; st.error = null;
      var prev2 = s.state;
      if (prev2 !== 'committing') s.state = 'active';
      s.running = true;
      var out2 = dispatchPath(parentsOf(r), makeEvent('success', false, false));
      s.running = false;
      if (s.state === 'active') s.state = prev2 === 'committing' ? 'committing' : 'inactive';
      if (out2.threw && s.state !== 'finished' && !s.explicitCommit) abortTxn(t, domError('AbortError', 'An exception was thrown in a success handler.'));
    }
    if (item.after) item.after();
  }
  function commitTxn(t) {
    var s = txs(t);
    if (s.state === 'finished') return;
    s.state = 'committing';
    try { if (s.id) call('commit', s.id); } catch (e) {}
    s.state = 'finished';
    later(function () {
      // Finished for script from here on: the complete handler sees no
      // running upgrade any more (IndexedDB upgrade-transaction-lifecycle).
      s.done = true;
      dispatchPath(parentsOf(t), makeEvent('complete', false, false));
      if (s.onFinish) s.onFinish(true);
      schedule();
    });
  }
  function abortTxn(t, error) {
    var s = txs(t);
    if (s.state === 'finished') return;
    if (s.id) { try { call('abort', s.id); } catch (e) {} }
    s.state = 'finished';
    s.error = error;
    if (s.mode === 'versionchange') {
      var d = dbs(s.db);
      d.version = s.oldVersion;
      d.stores = s.oldStores || Object.create(null);
      // Renames of what existed before are undone (newest first); objects
      // made in this transaction keep their last name and are gone.
      var renames = (s.renames || []).slice().reverse();
      for (var ri = 0; ri < renames.length; ri++) if (!renames[ri].created) renames[ri].undo();
      var all = [];
      for (var name in s.storeObjects) all.push(s.storeObjects[name]);
      (s.deletedObjects || []).forEach(function (o) { if (all.indexOf(o) < 0) all.push(o); });
      for (var k = 0; k < all.length; k++) {
        var so = OS.get(all[k]);
        if (!so) continue;
        var alive = !so.createdIn && !!d.stores[so.name];
        so.deleted = !alive;
        if (alive) s.storeObjects[so.name] = all[k];
        var ixList = [];
        for (var iname in so.indexObjects) ixList.push(so.indexObjects[iname]);
        (so.deletedIndexes || []).forEach(function (x) { if (ixList.indexOf(x) < 0) ixList.push(x); });
        for (var q = 0; q < ixList.length; q++) {
          var io = IX.get(ixList[q]);
          if (!io) continue;
          var ixAlive = alive && !io.createdIn && !!d.stores[so.name].indexes[io.name];
          io.deleted = !ixAlive;
          if (ixAlive) so.indexObjects[io.name] = ixList[q];
        }
      }
    }
    var pend = s.pending.splice(0).filter(function (p) { return p.request; });
    later(function () {
      for (var i = 0; i < pend.length; i++) {
        var st = RQ.get(pend[i].request);
        st.done = true; st.result = undefined; st.error = domError('AbortError', 'The transaction was aborted, so the request cannot be fulfilled.');
        dispatchPath(parentsOf(pend[i].request), makeEvent('error', true, true));
      }
      s.done = true;
      dispatchPath(parentsOf(t), makeEvent('abort', true, false));
      if (s.onFinish) s.onFinish(false);
      schedule();
    });
  }
  // Storage work of a schema change (create/delete/rename of stores and
  // indexes) runs in request order, after the requests made before it.
  function queueInternal(t, fn) {
    txs(t).pending.push({ internal: fn });
    schedule();
  }
  // Queue a request on a transaction (IndexedDB §5.6 "asynchronously
  // execute a request").
  function queue(t, source, op, after) {
    var s = txs(t);
    var r = newRequest(source, t, false);
    s.pending.push({ request: r, op: op, after: after });
    schedule();
    return r;
  }
  function checkActive(t, op, ifaceName) {
    var s = txs(t);
    if (s.state !== 'active') throw domError('TransactionInactiveError', "Failed to execute '" + op + "' on '" + ifaceName + "': The transaction is not active.");
  }

  // -------------------------------------------------------------------
  // IDBObjectStore (IndexedDB §4.5)
  // -------------------------------------------------------------------
  var OS = new WeakMap();
  var IDBObjectStore = iface('IDBObjectStore');
  function oss(o) { var s = OS.get(o); if (!s) throw new TypeError('Illegal invocation'); return s; }
  function storeObject(t, name) {
    var s = txs(t);
    if (s.storeObjects[name]) return s.storeObjects[name];
    var o = Object.create(IDBObjectStore.prototype);
    OS.set(o, { txn: t, name: name, deleted: false, indexObjects: Object.create(null) });
    s.storeObjects[name] = o;
    return o;
  }
  function meta(o) {
    var s = oss(o);
    return dbs(txs(s.txn).db).stores[s.name];
  }
  function liveStore(o, op) {
    var s = oss(o);
    if (s.deleted || !meta(o)) throw domError('InvalidStateError', "Failed to execute '" + op + "' on 'IDBObjectStore': The object store has been deleted.");
    return s;
  }
  getter(IDBObjectStore.prototype, 'name', function () { return oss(this).name; }, function (v) {
    var s = oss(this);
    var t = s.txn, ts = txs(t);
    var n = String(v);
    if (ts.mode !== 'versionchange') throw domError('InvalidStateError', "Failed to set the 'name' property on 'IDBObjectStore': The database is not running a version change transaction.");
    liveStore(this, 'name');
    if (ts.state !== 'active') throw domError('TransactionInactiveError', "Failed to set the 'name' property on 'IDBObjectStore': The transaction is not active.");
    if (n === s.name) return;
    var stores = dbs(ts.db).stores;
    if (stores[n]) throw domError('ConstraintError', "Failed to set the 'name' property on 'IDBObjectStore': An object store with the specified name already exists.");
    var dbn = dbs(ts.db).name, from = s.name;
    queueInternal(t, function () { call('rename_store', ts.id, dbn, from, n); });
    stores[n] = stores[s.name]; stores[n].name = n; delete stores[s.name];
    delete ts.storeObjects[s.name]; ts.storeObjects[n] = this;
    var i = ts.scope.indexOf(s.name); if (i >= 0) ts.scope[i] = n;
    var self = this, old = s.name;
    (ts.renames || (ts.renames = [])).push({ created: !!s.createdIn, undo: function () { delete ts.storeObjects[s.name]; s.name = old; ts.storeObjects[old] = self; } });
    s.name = n;
  });
  getter(IDBObjectStore.prototype, 'keyPath', function () {
    var s = oss(this), m = meta(this), kp = m ? m.keyPath : null;
    if (!Array.isArray(kp)) return kp;
    if (!s.keyPathArray) s.keyPathArray = kp.slice();
    return s.keyPathArray;
  });
  getter(IDBObjectStore.prototype, 'indexNames', function () { var m = meta(this); return stringList(m ? sortedNames(m.indexes) : []); });
  getter(IDBObjectStore.prototype, 'transaction', function () { return oss(this).txn; });
  getter(IDBObjectStore.prototype, 'autoIncrement', function () { var m = meta(this); return !!(m && m.autoIncrement); });
  function dbName(t) { return dbs(txs(t).db).name; }
  function writeCheck(o, op) {
    var s = liveStore(o, op);
    checkActive(s.txn, op, 'IDBObjectStore');
    if (txs(s.txn).mode === 'readonly') throw domError('ReadOnlyError', "Failed to execute '" + op + "' on 'IDBObjectStore': The transaction is read-only.");
    return s;
  }
  // put/add (IndexedDB §4.5 "add or put")
  function addOrPut(o, value, key, noOverwrite, op, preClone) {
    var s = writeCheck(o, op);
    var m = meta(o);
    var hasKey = key !== undefined;
    if (m.keyPath !== null && hasKey) throw domError('DataError', "Failed to execute '" + op + "' on 'IDBObjectStore': The object store uses in-line keys and the key parameter was provided.");
    if (m.keyPath === null && !m.autoIncrement && !hasKey) throw domError('DataError', "Failed to execute '" + op + "' on 'IDBObjectStore': The object store uses out-of-line keys and has no key generator and the key parameter was not provided.");
    var k = null;
    if (hasKey) k = keyOrThrow(key, "Failed to execute '" + op + "' on 'IDBObjectStore': The parameter");
    // The clone is made with the transaction inactive (getters cannot
    // make requests) — IndexedDB §4.5 step 10.
    var ts = txs(s.txn);
    var clone, handle;
    if (preClone) clone = preClone.value;
    else {
      ts.state = 'inactive';
      try {
        handle = serialize(value, [], [], []);
        clone = deserialize(handle, []);
      } finally { ts.state = 'active'; }
    }
    var generateInto = false;
    if (m.keyPath !== null) {
      var kk = keyFromValue(clone, m.keyPath);
      if (kk === INVALID) throw domError('DataError', "Failed to execute '" + op + "' on 'IDBObjectStore': Evaluating the object store's key path yielded a value that is not a valid key.");
      if (kk === null) {
        if (!m.autoIncrement) throw domError('DataError', "Failed to execute '" + op + "' on 'IDBObjectStore': Evaluating the object store's key path did not yield a value.");
        if (!canInject(clone, m.keyPath)) throw domError('DataError', "Failed to execute '" + op + "' on 'IDBObjectStore': A generated key could not be inserted into the value.");
        generateInto = true;
      } else k = kk;
    }
    var store = s.name;
    var ixAtCall = [];
    // A copy: a later rename must not change the names this request uses.
    for (var ixn in m.indexes) {
      var mx = m.indexes[ixn];
      ixAtCall.push({ name: mx.name, keyPath: mx.keyPath, unique: mx.unique, multiEntry: mx.multiEntry });
    }
    var request = queue(s.txn, o, function () {
      var key2 = k, generated = false;
      if (key2 === null) {
        key2 = call('gen_key', ts.id, dbName(s.txn), store);
        generated = true;
        if (generateInto) inject(clone, m.keyPath, key2);
      }
      var idx = [];
      for (var n = 0; n < ixAtCall.length; n++) idx.push([ixAtCall[n].name, indexKeysFor(clone, ixAtCall[n])]);
      var h = serialize(clone, [], [], []);
      var stored;
      try { stored = call('put', ts.id, dbName(s.txn), store, key2, h, idx, noOverwrite); } catch (e) {
        if (generated) { try { call('gen_restore', ts.id, dbName(s.txn), store, key2); } catch (x) {} }
        throw e;
      }
      return fromKey(stored);
    });
    return request;
  }
  method(IDBObjectStore.prototype, 'put', function put(value, key) {
    if (arguments.length < 1) throw typeError('put', 'IDBObjectStore', '1 argument required, but only 0 present.');
    return addOrPut(this, value, key, false, 'put');
  });
  method(IDBObjectStore.prototype, 'add', function add(value, key) {
    if (arguments.length < 1) throw typeError('add', 'IDBObjectStore', '1 argument required, but only 0 present.');
    return addOrPut(this, value, key, true, 'add');
  });
  method(IDBObjectStore.prototype, 'delete', function del(query) {
    if (arguments.length < 1) throw typeError('delete', 'IDBObjectStore', '1 argument required, but only 0 present.');
    var s = writeCheck(this, 'delete');
    var range = toRange(query, false);
    var t = s.txn, name = s.name;
    return queue(t, this, function () { call('delete', txs(t).id, dbName(t), name, range); return undefined; });
  });
  method(IDBObjectStore.prototype, 'clear', function clear() {
    var s = writeCheck(this, 'clear');
    var t = s.txn, name = s.name;
    return queue(t, this, function () { call('clear', txs(t).id, dbName(t), name); return undefined; });
  });
  function readCheck(src, op, ifaceName) {
    var s;
    if (ifaceName === 'IDBIndex') { s = liveIndex(src, op); checkActive(s.txn, op, 'IDBIndex'); }
    else { s = liveStore(src, op); checkActive(s.txn, op, 'IDBObjectStore'); }
    return s;
  }
  function sourceArgs(src) {
    var ix = IX.get(src);
    if (ix) return { txn: ix.txn, store: OS.get(ix.store).name, index: ix.name };
    var s = OS.get(src);
    return { txn: s.txn, store: s.name, index: null };
  }
  // get / getKey / getAll / getAllKeys / count for stores and indexes
  function readOps(proto, ifaceName) {
    method(proto, 'get', function get(query) {
      if (arguments.length < 1) throw typeError('get', ifaceName, '1 argument required, but only 0 present.');
      readCheck(this, 'get', ifaceName);
      var range = toRange(query, false);
      var a = sourceArgs(this);
      return queue(a.txn, this, function () {
        var rows = call('get', dbName(a.txn), a.store, a.index, range, false, false, 1, true);
        return rows.length ? deserialize(rows[0][2], []) : undefined;
      });
    });
    method(proto, 'getKey', function getKey(query) {
      if (arguments.length < 1) throw typeError('getKey', ifaceName, '1 argument required, but only 0 present.');
      readCheck(this, 'getKey', ifaceName);
      var range = toRange(query, false);
      var a = sourceArgs(this);
      return queue(a.txn, this, function () {
        var rows = call('get', dbName(a.txn), a.store, a.index, range, false, false, 1, false);
        return rows.length ? fromKey(rows[0][1]) : undefined;
      });
    });
    function getAllArgs(query, count, op) {
      var dir = 'next';
      if (query !== null && query !== undefined && typeof query === 'object' && !(query instanceof IDBKeyRange) && !Array.isArray(query) && !isBufferSource(query) && !(query instanceof Date)) {
        // IDBGetAllOptions { query, count, direction }
        var o = query;
        query = o.query; count = o.count;
        if (o.direction !== undefined) dir = String(o.direction);
        if (['next', 'nextunique', 'prev', 'prevunique'].indexOf(dir) < 0) throw new TypeError("Failed to execute '" + op + "' on '" + ifaceName + "': The provided value '" + dir + "' is not a valid enum value of type IDBCursorDirection.");
      }
      var range = toRange(query, true);
      var n = 0;
      if (count !== undefined) {
        n = Number(count);
        if (!isFinite(n) || n < 0 || n > 4294967295) throw new TypeError("Failed to execute '" + op + "' on '" + ifaceName + "': Value is outside the 'unsigned long' value range.");
        n = Math.floor(n);
      }
      return { range: range, count: n, dir: dir };
    }
    method(proto, 'getAll', function getAll(query, count) {
      readCheck(this, 'getAll', ifaceName);
      var p = getAllArgs(query, count, 'getAll');
      var a = sourceArgs(this);
      return queue(a.txn, this, function () {
        var rows = call('get', dbName(a.txn), a.store, a.index, p.range, p.dir.indexOf('prev') === 0, /unique$/.test(p.dir), p.count, true);
        return rows.map(function (r) { return deserialize(r[2], []); });
      });
    });
    method(proto, 'getAllKeys', function getAllKeys(query, count) {
      readCheck(this, 'getAllKeys', ifaceName);
      var p = getAllArgs(query, count, 'getAllKeys');
      var a = sourceArgs(this);
      return queue(a.txn, this, function () {
        var rows = call('get', dbName(a.txn), a.store, a.index, p.range, p.dir.indexOf('prev') === 0, /unique$/.test(p.dir), p.count, false);
        return rows.map(function (r) { return fromKey(r[1]); });
      });
    });
    method(proto, 'getAllRecords', function getAllRecords(options) {
      readCheck(this, 'getAllRecords', ifaceName);
      var o = options || {};
      var p = getAllArgs({ query: o.query, count: o.count, direction: o.direction }, undefined, 'getAllRecords');
      var a = sourceArgs(this);
      return queue(a.txn, this, function () {
        var rows = call('get', dbName(a.txn), a.store, a.index, p.range, p.dir.indexOf('prev') === 0, /unique$/.test(p.dir), p.count, true);
        return rows.map(function (r) { return recordObject(fromKey(r[0]), fromKey(r[1]), deserialize(r[2], [])); });
      });
    });
    method(proto, 'count', function count(query) {
      readCheck(this, 'count', ifaceName);
      var range = toRange(query, true);
      var a = sourceArgs(this);
      return queue(a.txn, this, function () { return call('count', dbName(a.txn), a.store, a.index, range); });
    });
    method(proto, 'openCursor', function openCursor(query, direction) {
      readCheck(this, 'openCursor', ifaceName);
      return openCursorOn(this, query, direction, true, 'openCursor', ifaceName);
    });
    method(proto, 'openKeyCursor', function openKeyCursor(query, direction) {
      readCheck(this, 'openKeyCursor', ifaceName);
      return openCursorOn(this, query, direction, false, 'openKeyCursor', ifaceName);
    });
  }
  readOps(IDBObjectStore.prototype, 'IDBObjectStore');
  var IDBRecord = iface('IDBRecord');
  function recordObject(key, primaryKey, value) {
    var r = Object.create(IDBRecord.prototype);
    hide(r, '__r', { key: key, primaryKey: primaryKey, value: value });
    return r;
  }
  ['key', 'primaryKey', 'value'].forEach(function (k) { getter(IDBRecord.prototype, k, function () { return this.__r ? this.__r[k] : undefined; }); });
  method(IDBObjectStore.prototype, 'index', function index(name) {
    var s = liveStore(this, 'index');
    if (arguments.length < 1) throw typeError('index', 'IDBObjectStore', '1 argument required, but only 0 present.');
    if (txs(s.txn).state === 'finished') throw domError('InvalidStateError', "Failed to execute 'index' on 'IDBObjectStore': The transaction has finished.");
    name = String(name);
    var m = meta(this);
    if (!m.indexes[name]) throw domError('NotFoundError', "Failed to execute 'index' on 'IDBObjectStore': The specified index was not found.");
    return indexObject(this, name);
  });
  method(IDBObjectStore.prototype, 'createIndex', function createIndex(name, keyPath, options) {
    var s = oss(this);
    if (arguments.length < 2) throw typeError('createIndex', 'IDBObjectStore', '2 arguments required, but only ' + arguments.length + ' present.');
    name = String(name);
    var ts = txs(s.txn);
    if (ts.mode !== 'versionchange') throw domError('InvalidStateError', "Failed to execute 'createIndex' on 'IDBObjectStore': The database is not running a version change transaction.");
    liveStore(this, 'createIndex');
    if (ts.state !== 'active') throw domError('TransactionInactiveError', "Failed to execute 'createIndex' on 'IDBObjectStore': The transaction is not active.");
    var kp = normPath(keyPath);
    var m = meta(this);
    if (m.indexes[name]) throw domError('ConstraintError', "Failed to execute 'createIndex' on 'IDBObjectStore': An index with the specified name already exists.");
    if (kp === null || !pathOk(kp)) throw domError('SyntaxError', "Failed to execute 'createIndex' on 'IDBObjectStore': The keyPath argument contains an invalid key path.");
    var o = options || {};
    var unique = !!o.unique, multi = !!o.multiEntry;
    if (multi && Array.isArray(kp)) throw domError('InvalidAccessError', "Failed to execute 'createIndex' on 'IDBObjectStore': The keyPath argument was an array and the multiEntry option is true.");
    var ix = { name: name, keyPath: kp, unique: unique, multiEntry: multi };
    var t = s.txn, dbn = dbs(ts.db).name, storeName = s.name;
    queueInternal(t, function () {
      var rows = call('get', dbn, storeName, null, null, false, false, 0, true);
      var entries = rows.map(function (r) { return [r[1], indexKeysFor(deserialize(r[2], []), ix)]; });
      var violated = call('create_index', ts.id, dbn, storeName, name, pathJson(kp), unique, multi, entries);
      // A unique index over duplicates aborts the transaction (IndexedDB
      // §4.5 createIndex step 13, in request order).
      if (violated) throw domError('ConstraintError', 'Unable to add key to index: at least one key does not satisfy the uniqueness requirements.');
    });
    m.indexes[name] = ix;
    var io = indexObject(this, name);
    IX.get(io).createdIn = t;
    return io;
  });
  method(IDBObjectStore.prototype, 'deleteIndex', function deleteIndex(name) {
    var s = oss(this);
    if (arguments.length < 1) throw typeError('deleteIndex', 'IDBObjectStore', '1 argument required, but only 0 present.');
    name = String(name);
    var ts = txs(s.txn);
    if (ts.mode !== 'versionchange') throw domError('InvalidStateError', "Failed to execute 'deleteIndex' on 'IDBObjectStore': The database is not running a version change transaction.");
    liveStore(this, 'deleteIndex');
    if (ts.state !== 'active') throw domError('TransactionInactiveError', "Failed to execute 'deleteIndex' on 'IDBObjectStore': The transaction is not active.");
    var m = meta(this);
    if (!m.indexes[name]) throw domError('NotFoundError', "Failed to execute 'deleteIndex' on 'IDBObjectStore': The specified index was not found.");
    var dbn4 = dbs(ts.db).name, storeName4 = s.name;
    queueInternal(s.txn, function () { call('delete_index', ts.id, dbn4, storeName4, name); });
    delete m.indexes[name];
    var io = s.indexObjects[name];
    if (io) {
      IX.get(io).deleted = true;
      delete s.indexObjects[name];
      (s.deletedIndexes || (s.deletedIndexes = [])).push(io);
    }
  });

  // -------------------------------------------------------------------
  // IDBIndex (IndexedDB §4.6)
  // -------------------------------------------------------------------
  var IX = new WeakMap();
  var IDBIndex = iface('IDBIndex');
  function ixs(i) { var s = IX.get(i); if (!s) throw new TypeError('Illegal invocation'); return s; }
  function indexObject(store, name) {
    var ss = OS.get(store);
    if (ss.indexObjects[name]) return ss.indexObjects[name];
    var i = Object.create(IDBIndex.prototype);
    IX.set(i, { store: store, txn: ss.txn, name: name, deleted: false });
    ss.indexObjects[name] = i;
    return i;
  }
  function ixMeta(i) {
    var s = ixs(i);
    var m = meta(s.store);
    return m ? m.indexes[s.name] : null;
  }
  function liveIndex(i, op) {
    var s = ixs(i);
    if (s.deleted || OS.get(s.store).deleted || !ixMeta(i)) throw domError('InvalidStateError', "Failed to execute '" + op + "' on 'IDBIndex': The index or its object store has been deleted.");
    return s;
  }
  getter(IDBIndex.prototype, 'name', function () { return ixs(this).name; }, function (v) {
    var s = ixs(this);
    var ts = txs(s.txn);
    var n = String(v);
    if (ts.mode !== 'versionchange') throw domError('InvalidStateError', "Failed to set the 'name' property on 'IDBIndex': The database is not running a version change transaction.");
    liveIndex(this, 'name');
    if (ts.state !== 'active') throw domError('TransactionInactiveError', "Failed to set the 'name' property on 'IDBIndex': The transaction is not active.");
    if (n === s.name) return;
    var m = meta(s.store);
    if (m.indexes[n]) throw domError('ConstraintError', "Failed to set the 'name' property on 'IDBIndex': An index with the specified name already exists.");
    var dbn3 = dbs(ts.db).name, storeName3 = OS.get(s.store).name, from3 = s.name;
    queueInternal(s.txn, function () { call('rename_index', ts.id, dbn3, storeName3, from3, n); });
    m.indexes[n] = m.indexes[s.name]; m.indexes[n].name = n; delete m.indexes[s.name];
    var so = OS.get(s.store);
    delete so.indexObjects[s.name]; so.indexObjects[n] = this;
    var me = this, oldName = s.name;
    (ts.renames || (ts.renames = [])).push({ created: !!s.createdIn, undo: function () { delete so.indexObjects[s.name]; s.name = oldName; so.indexObjects[oldName] = me; } });
    s.name = n;
  });
  getter(IDBIndex.prototype, 'objectStore', function () { return ixs(this).store; });
  getter(IDBIndex.prototype, 'keyPath', function () {
    var s = ixs(this), m = ixMeta(this), kp = m ? m.keyPath : null;
    if (!Array.isArray(kp)) return kp;
    if (!s.keyPathArray) s.keyPathArray = kp.slice();
    return s.keyPathArray;
  });
  getter(IDBIndex.prototype, 'multiEntry', function () { var m = ixMeta(this); return !!(m && m.multiEntry); });
  getter(IDBIndex.prototype, 'unique', function () { var m = ixMeta(this); return !!(m && m.unique); });
  readOps(IDBIndex.prototype, 'IDBIndex');

  // -------------------------------------------------------------------
  // IDBCursor (IndexedDB §4.8)
  // -------------------------------------------------------------------
  var CU = new WeakMap();
  var IDBCursor = iface('IDBCursor');
  var IDBCursorWithValue = iface('IDBCursorWithValue', IDBCursor);
  function cus(c) { var s = CU.get(c); if (!s) throw new TypeError('Illegal invocation'); return s; }
  function openCursorOn(src, query, direction, withValue, op, ifaceName) {
    var range = toRange(query, true);
    var dir = direction === undefined ? 'next' : String(direction);
    if (['next', 'nextunique', 'prev', 'prevunique'].indexOf(dir) < 0) throw new TypeError("Failed to execute '" + op + "' on '" + ifaceName + "': The provided value '" + dir + "' is not a valid enum value of type IDBCursorDirection.");
    var a = sourceArgs(src);
    var c = Object.create((withValue ? IDBCursorWithValue : IDBCursor).prototype);
    var cs = { source: src, txn: a.txn, store: a.store, index: a.index, range: range, dir: dir, withValue: withValue,
      key: undefined, primaryKey: undefined, value: undefined, keyEnc: null, primaryEnc: null, gotValue: false, request: null };
    CU.set(c, cs);
    var r = queue(a.txn, src, function () { return iterate(c, cs, null, null, false, 1); });
    cs.request = r;
    return r;
  }
  // iterate: count steps; target key/primary (continue(key), continuePrimaryKey)
  function iterate(c, cs, targetKey, targetPrimary, inclusiveTarget, count) {
    var posKey = cs.keyEnc, posPrimary = cs.primaryEnc;
    var row = null;
    var name = dbName(cs.txn);
    if (targetKey) {
      row = call('cursor', name, cs.store, cs.index, cs.range, cs.dir, targetKey, targetPrimary, true, cs.withValue);
      // continue(key) must move strictly past the current position
      if (row && posKey && cs.dir.indexOf('next') === 0 && cmpKeys(row[0], posKey) === 0 && (cs.index === null || !targetPrimary)) {
        row = call('cursor', name, cs.store, cs.index, cs.range, cs.dir, row[0], row[1], false, cs.withValue);
      }
    } else {
      for (var i = 0; i < count; i++) {
        row = call('cursor', name, cs.store, cs.index, cs.range, cs.dir, posKey, cs.index === null ? null : posPrimary, false, cs.withValue && i === count - 1);
        if (!row) break;
        posKey = row[0]; posPrimary = row[1];
      }
    }
    if (!row) {
      cs.key = undefined; cs.primaryKey = undefined; cs.value = undefined; cs.keyEnc = null; cs.primaryEnc = null; cs.gotValue = false;
      return null;
    }
    cs.keyEnc = row[0]; cs.primaryEnc = row[1];
    cs.key = fromKey(row[0]); cs.primaryKey = fromKey(row[1]);
    cs.value = cs.withValue && row[2] !== null ? deserialize(row[2], []) : undefined;
    cs.gotValue = true;
    return c;
  }
  getter(IDBCursor.prototype, 'source', function () { return cus(this).source; });
  getter(IDBCursor.prototype, 'direction', function () { return cus(this).dir; });
  getter(IDBCursor.prototype, 'key', function () { var k = cus(this).key; return k instanceof Date ? new Date(k.getTime()) : k; });
  getter(IDBCursor.prototype, 'primaryKey', function () { return cus(this).primaryKey; });
  getter(IDBCursor.prototype, 'request', function () { return cus(this).request; });
  getter(IDBCursorWithValue.prototype, 'value', function () { return cus(this).value; });
  function cursorCheck(c, op) {
    var cs = cus(c);
    var src = cs.source;
    checkActive(cs.txn, op, 'IDBCursor');
    if (IX.has(src)) liveIndex(src, op); else liveStore(src, op);
    return cs;
  }
  function advanceRequest(c, cs, fn) {
    if (!cs.gotValue) throw domError('InvalidStateError', 'The cursor is being iterated or has iterated past its end.');
    cs.gotValue = false;
    var r = cs.request;
    var st = RQ.get(r);
    st.done = false; st.result = undefined; st.error = null;
    var ts = txs(cs.txn);
    ts.pending.push({ request: r, op: fn });
    schedule();
  }
  method(IDBCursor.prototype, 'advance', function advance(count) {
    var cs = cus(this);
    if (arguments.length < 1) throw typeError('advance', 'IDBCursor', '1 argument required, but only 0 present.');
    var n = Number(count);
    if (!isFinite(n) || n < 0 || n > 4294967295) throw new TypeError("Failed to execute 'advance' on 'IDBCursor': Value is outside the 'unsigned long' value range.");
    n = Math.floor(n);
    if (n === 0) throw new TypeError("Failed to execute 'advance' on 'IDBCursor': A count argument with value 0 (zero) was supplied, must be greater than 0.");
    cursorCheck(this, 'advance');
    if (!cs.gotValue) throw domError('InvalidStateError', "Failed to execute 'advance' on 'IDBCursor': The cursor is being iterated or has iterated past its end.");
    var c = this;
    advanceRequest(c, cs, function () { return iterate(c, cs, null, null, false, n); });
  });
  method(IDBCursor.prototype, 'continue', function continue_(key) {
    var cs = cursorCheck(this, 'continue');
    if (!cs.gotValue) throw domError('InvalidStateError', "Failed to execute 'continue' on 'IDBCursor': The cursor is being iterated or has iterated past its end.");
    var target = null;
    if (key !== undefined) {
      target = keyOrThrow(key, "Failed to execute 'continue' on 'IDBCursor': The parameter");
      var c0 = cmpKeys(target, cs.keyEnc);
      if (cs.dir.indexOf('next') === 0 ? c0 <= 0 : c0 >= 0) throw domError('DataError', "Failed to execute 'continue' on 'IDBCursor': The parameter is less than or equal to this cursor's position.");
    }
    var c = this;
    advanceRequest(c, cs, function () { return target ? iterate(c, cs, target, null, true, 1) : iterate(c, cs, null, null, false, 1); });
  });
  method(IDBCursor.prototype, 'continuePrimaryKey', function continuePrimaryKey(key, primaryKey) {
    var cs = cus(this);
    if (arguments.length < 2) throw typeError('continuePrimaryKey', 'IDBCursor', '2 arguments required, but only ' + arguments.length + ' present.');
    cursorCheck(this, 'continuePrimaryKey');
    if (!IX.has(cs.source)) throw domError('InvalidAccessError', "Failed to execute 'continuePrimaryKey' on 'IDBCursor': The cursor's source is not an index.");
    if (cs.dir !== 'next' && cs.dir !== 'prev') throw domError('InvalidAccessError', "Failed to execute 'continuePrimaryKey' on 'IDBCursor': The cursor's direction is not 'next' or 'prev'.");
    if (!cs.gotValue) throw domError('InvalidStateError', "Failed to execute 'continuePrimaryKey' on 'IDBCursor': The cursor is being iterated or has iterated past its end.");
    var k = keyOrThrow(key, "Failed to execute 'continuePrimaryKey' on 'IDBCursor': The parameter");
    var p = keyOrThrow(primaryKey, "Failed to execute 'continuePrimaryKey' on 'IDBCursor': The parameter");
    var ck = cmpKeys(k, cs.keyEnc);
    var forward = cs.dir === 'next';
    if (forward ? ck < 0 : ck > 0) throw domError('DataError', "Failed to execute 'continuePrimaryKey' on 'IDBCursor': The key is behind the cursor's position.");
    if (ck === 0) {
      var cp = cmpKeys(p, cs.primaryEnc);
      if (forward ? cp <= 0 : cp >= 0) throw domError('DataError', "Failed to execute 'continuePrimaryKey' on 'IDBCursor': The primary key is not past the cursor's position.");
    }
    var c = this;
    advanceRequest(c, cs, function () {
      var row = call('cursor', dbName(cs.txn), cs.store, cs.index, cs.range, cs.dir, k, p, true, cs.withValue);
      if (!row) { cs.key = cs.primaryKey = cs.value = undefined; cs.keyEnc = cs.primaryEnc = null; return null; }
      cs.keyEnc = row[0]; cs.primaryEnc = row[1]; cs.key = fromKey(row[0]); cs.primaryKey = fromKey(row[1]);
      cs.value = cs.withValue && row[2] !== null ? deserialize(row[2], []) : undefined;
      cs.gotValue = true;
      return c;
    });
  });
  method(IDBCursor.prototype, 'update', function update(value) {
    var cs = cus(this);
    if (arguments.length < 1) throw typeError('update', 'IDBCursor', '1 argument required, but only 0 present.');
    cursorCheck(this, 'update');
    if (txs(cs.txn).mode === 'readonly') throw domError('ReadOnlyError', "Failed to execute 'update' on 'IDBCursor': The record may not be updated inside a read-only transaction.");
    if (!cs.gotValue || !cs.withValue) throw domError('InvalidStateError', "Failed to execute 'update' on 'IDBCursor': The cursor is being iterated or has iterated past its end.");
    var store = IX.has(cs.source) ? IX.get(cs.source).store : cs.source;
    var m = meta(store);
    // One clone for the key check and the store (each property read once).
    var ts = txs(cs.txn);
    ts.state = 'inactive';
    var clone;
    try { clone = deserialize(serialize(value, [], [], []), []); } finally { ts.state = 'active'; }
    if (m.keyPath !== null) {
      var k = keyFromValue(clone, m.keyPath);
      if (k === null || k === INVALID || cmpKeys(k, cs.primaryEnc) !== 0) throw domError('DataError', "Failed to execute 'update' on 'IDBCursor': The effective object store of this cursor uses in-line keys and evaluating the key path of the value parameter results in a different value than the cursor's effective key.");
      return addOrPutWithSource(store, value, undefined, this, clone);
    }
    return addOrPutWithSource(store, value, cs.primaryKey, this, clone);
  });
  function addOrPutWithSource(store, value, key, source, clone) {
    var r = addOrPut(store, value, key, false, 'update', { value: clone });
    RQ.get(r).source = source;
    return r;
  }
  method(IDBCursor.prototype, 'delete', function del() {
    var cs = cus(this);
    cursorCheck(this, 'delete');
    if (txs(cs.txn).mode === 'readonly') throw domError('ReadOnlyError', "Failed to execute 'delete' on 'IDBCursor': The record may not be deleted inside a read-only transaction.");
    if (!cs.gotValue || !cs.withValue) throw domError('InvalidStateError', "Failed to execute 'delete' on 'IDBCursor': The cursor is being iterated or has iterated past its end.");
    var t = cs.txn, store = cs.store, pk = cs.primaryEnc;
    return queue(t, this, function () { call('delete', txs(t).id, dbName(t), store, [0, pk, pk, false, false]); return undefined; });
  });

  // -------------------------------------------------------------------
  // IDBFactory (IndexedDB §4.2): open, deleteDatabase, databases, cmp
  // -------------------------------------------------------------------
  var IDBFactory = iface('IDBFactory');
  function enqueue(name, job) {
    var q = QUEUES[name] || (QUEUES[name] = []);
    q.push(job);
    if (q.length === 1) later(function () { runJob(name); });
  }
  function runJob(name) {
    var q = QUEUES[name];
    if (!q || !q.length) return;
    var job = q[0];
    if (job.running) return;
    job.running = true;
    job.run(function done() {
      q.shift();
      if (q.length) later(function () { runJob(name); });
    });
  }
  function pumpQueues() {
    for (var n in QUEUES) {
      var q = QUEUES[n];
      if (q.length && q[0].waiting) q[0].waiting();
    }
  }
  // Connections of this context that block a version change.
  function others(name, except) {
    return CONNECTIONS.filter(function (c) { var s = DB.get(c); return s.name === name && c !== except && !s.closePending; });
  }
  function fireAt(target, ev) { return dispatchPath(parentsOf(target), ev); }
  function finishRequest(r, result, error, ev) {
    var st = RQ.get(r);
    st.done = true; st.result = result; st.error = error || null;
    return fireAt(r, ev);
  }
  method(IDBFactory.prototype, 'open', function open(name, version) {
    if (arguments.length < 1) throw typeError('open', 'IDBFactory', '1 argument required, but only 0 present.');
    name = String(name);
    var v;
    if (version !== undefined) {
      v = Number(version);
      if (!isFinite(v) || v < 0 || v > 9007199254740991) throw new TypeError("Failed to execute 'open' on 'IDBFactory': Value is outside the 'unsigned long long' value range.");
      v = Math.floor(v);
      if (v === 0) throw new TypeError("Failed to execute 'open' on 'IDBFactory': The version provided must not be 0.");
    }
    opaqueCheck('open');
    var r = newRequest(null, null, true);
    enqueue(name, { run: function (done) { openSteps(r, name, v, done); } });
    return r;
  });
  // An opaque origin has no databases (IndexedDB §4.2 open step 1).
  var opaque = null;
  function opaqueCheck(op) {
    if (opaque === null) opaque = call('opaque') === true;
    if (opaque) throw domError('SecurityError', "Failed to execute '" + op + "' on 'IDBFactory': access to the Indexed Database API is denied in this context.");
  }
  function openSteps(r, name, requested, done) {
    var current;
    try { current = call('version', name); } catch (e) { finishRequest(r, undefined, hostError(e), makeEvent('error', true, true)); done(); return; }
    var version = requested === undefined ? (current === 0 ? 1 : current) : requested;
    if (version < current) {
      finishRequest(r, undefined, domError('VersionError', 'The requested version (' + version + ') is less than the existing version (' + current + ').'), makeEvent('error', true, true));
      done();
      return;
    }
    var conn = Object.create(IDBDatabase.prototype);
    DB.set(conn, { name: name, version: current, stores: storesOf(name), upgrade: null, closePending: false });
    if (version === current) {
      CONNECTIONS.push(conn);
      finishRequest(r, conn, null, makeEvent('success', false, false));
      done();
      return;
    }
    // A version change: other connections get `versionchange`, then the
    // request waits for them to close (`blocked`).
    var blockers = others(name, conn);
    blockers.forEach(function (c) {
      if (!DB.get(c).closePending) fireAt(c, versionEvent('versionchange', DB.get(c).version, version));
    });
    var job = QUEUES[name][0];
    var blocked = whenBlocked(r, others(name, conn).length > 0, versionEvent('blocked', current, version));
    function proceed() {
      if (!blocked.fired || others(name, conn).length) {
        job.waiting = proceed;
        return;
      }
      job.waiting = null;
      CONNECTIONS.push(conn);
      upgrade(r, conn, current, version, done);
    }
    blocked.then = proceed;
    // The blocked task goes on; nobody to wait for: go on in this task.
    if (blocked.fired) { if (blockers.length) later(proceed); else proceed(); }
  }
  // IndexedDB §5.1 step 10.4 / §5.3 step 8: when connections are still
  // open after their versionchange events, a task fires `blocked` at the
  // request (even if they close before it runs); the request goes on only
  // after it.
  function whenBlocked(r, stillOpen, ev) {
    var b = { fired: !stillOpen, then: null };
    if (stillOpen) later(function () { fireAt(r, ev); b.fired = true; if (b.then) b.then(); });
    return b;
  }
  function upgrade(r, conn, oldVersion, newVersion, done) {
    var d = DB.get(conn);
    var t = Object.create(IDBTransaction.prototype);
    var s = { db: conn, scope: Object.keys(d.stores), mode: 'versionchange', durability: 'default', state: 'inactive', started: true, id: 0,
      pending: [], running: false, error: null, storeObjects: Object.create(null), explicitCommit: false, onFinish: null,
      oldVersion: oldVersion, oldStores: cloneStores(d.stores), waiting: false };
    TX.set(t, s);
    PARENT.set(t, conn);
    try { s.id = call('begin', d.name, newVersion); } catch (e) {
      finishRequest(r, undefined, hostError(e), makeEvent('error', true, true));
      done();
      return;
    }
    TXNS.push(t);
    d.version = newVersion;
    d.upgrade = t;
    var st = RQ.get(r);
    st.transaction = t;
    st.done = true; st.result = conn; st.error = null;
    s.onFinish = function (committed) {
      d.upgrade = null;
      st.transaction = null;
      // A connection closed during the upgrade: the upgrade completes, but
      // the open request fails (IndexedDB §5.1 step 10.6).
      if (committed && !d.closePending) {
        finishRequest(r, conn, null, makeEvent('success', false, false));
      } else {
        var i = CONNECTIONS.indexOf(conn);
        if (i >= 0) CONNECTIONS.splice(i, 1);
        d.closePending = true;
        if (!committed) d.version = oldVersion > 0 ? oldVersion : 0;
        finishRequest(r, undefined, domError('AbortError', 'The version change transaction was aborted.'), makeEvent('error', true, true));
      }
      done();
    };
    s.state = 'active';
    s.running = true;
    var out = fireAt(r, versionEvent('upgradeneeded', oldVersion, newVersion));
    s.running = false;
    if (s.state === 'active') s.state = 'inactive';
    if (out.threw && s.state !== 'finished') abortTxn(t, domError('AbortError', 'An exception was thrown in an upgradeneeded handler.'));
    maybeCommit(t);
    schedule();
  }
  method(IDBFactory.prototype, 'deleteDatabase', function deleteDatabase(name) {
    if (arguments.length < 1) throw typeError('deleteDatabase', 'IDBFactory', '1 argument required, but only 0 present.');
    name = String(name);
    opaqueCheck('deleteDatabase');
    var r = newRequest(null, null, true);
    enqueue(name, { run: function (done) { deleteSteps(r, name, done); } });
    return r;
  });
  function deleteSteps(r, name, done) {
    var current = 0;
    try { current = call('version', name); } catch (e) {}
    var blockers = others(name, null);
    blockers.forEach(function (c) { fireAt(c, versionEvent('versionchange', DB.get(c).version, null)); });
    var job = QUEUES[name][0];
    var blocked = whenBlocked(r, others(name, null).length > 0, versionEvent('blocked', current, null));
    function proceed() {
      if (!blocked.fired || others(name, null).length) {
        job.waiting = proceed;
        return;
      }
      job.waiting = null;
      var old = 0;
      try { old = call('delete_db', name); } catch (e) {}
      finishRequest(r, undefined, null, versionEvent('success', old, null));
      done();
    }
    blocked.then = proceed;
    // The blocked task goes on; nobody to wait for: go on in this task.
    if (blocked.fired) { if (blockers.length) later(proceed); else proceed(); }
  }
  method(IDBFactory.prototype, 'databases', function databases() {
    return new Promise(function (resolve) {
      opaqueCheck('databases');
      var list = call('databases');
      resolve(list.map(function (e) { return { name: dec(e[0]), version: e[1] }; }));
    });
  });
  method(IDBFactory.prototype, 'cmp', function cmp(a, b) {
    if (arguments.length < 2) throw typeError('cmp', 'IDBFactory', '2 arguments required, but only ' + arguments.length + ' present.');
    return cmpKeys(keyOrThrow(a, "Failed to execute 'cmp' on 'IDBFactory': The parameter 1"), keyOrThrow(b, "Failed to execute 'cmp' on 'IDBFactory': The parameter 2"));
  });
  var factory = Object.create(IDBFactory.prototype);
  Object.defineProperty(g, 'indexedDB', { get: function () { return factory; }, configurable: true, enumerable: true });
})();
