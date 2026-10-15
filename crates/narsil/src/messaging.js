// Cross-context messaging, the script side of `worker.rs`: MessageEvent,
// MessageChannel/MessagePort (HTML §9.4), BroadcastChannel (HTML §9.5),
// Worker and SharedWorker (HTML §10.2.6), the dedicated worker global's
// postMessage/close, and the task pump the host calls (`__narsilWPump`).
// Runs in pages (after the preamble) and in workers (after
// `worker_scope.js`). Messages are structured-serialized by the engine
// (`__narsilSerialize`), the host routes the serialized data.
(function () {
  var g = globalThis;
  var H = {};
  ['WNext', 'WPortPair', 'WPortPost', 'WPortStart', 'WPortClose', 'WPortRebind', 'WWorkerNew', 'WWorkerPost',
    'WWorkerTerminate', 'WBcOpen', 'WBcPost', 'WBcClose', 'WSharedNew', 'WSelfPost', 'WClose',
    'WReportError', 'WBlobPut', 'WBlobDrop', 'WBlobGet'].forEach(function (n) {
    var f = g['__narsil' + n];
    if (typeof f === 'function') {
      H[n] = f;
      try { delete g['__narsil' + n]; } catch (e) {}
    }
  });
  var serialize = g.__narsilSerialize, deserialize = g.__narsilDeserialize;
  var takeUncaught = g.__narsilTakeUncaught;
  var checkpoint = typeof g.__narsilMicrotaskCheckpoint === 'function' ? g.__narsilMicrotaskCheckpoint : function () {};
  // Set by the worker's close().
  var closing = false;
  ['__narsilSerialize', '__narsilDeserialize', '__narsilSerializeCopy', '__narsilTakeUncaught'].forEach(function (n) {
    try { delete g[n]; } catch (e) {}
  });
  // The platform data objects name their class (`Object.prototype.toString`
  // gives "[object Blob]", as `assert_class_string` expects).
  if (typeof Symbol === 'function' && Symbol.toStringTag) {
    ['Blob', 'File', 'FormData', 'Headers', 'Request', 'Response', 'AbortController', 'AbortSignal'].forEach(function (n) {
      var C = g[n];
      if (typeof C !== 'function' || !C.prototype || Object.prototype.hasOwnProperty.call(C.prototype, Symbol.toStringTag)) return;
      try { Object.defineProperty(C.prototype, Symbol.toStringTag, { value: n, configurable: true }); } catch (e) {}
    });
  }
  if (typeof H.WNext !== 'function' || typeof serialize !== 'function') return;
  var isWorker = typeof H.WSelfPost === 'function' || typeof H.WClose === 'function';
  var isShared = isWorker && typeof g.SharedWorkerGlobalScope === 'function' && g instanceof g.SharedWorkerGlobalScope;
  var DE = g.DOMException;
  var BaseEvent = g.Event;
  var ET = g.EventTarget;

  function domError(message, name) { return new DE(message, name); }
  function typeError(op, iface, text) { return new TypeError("Failed to execute '" + op + "' on '" + iface + "': " + text); }
  function hide(o, name, value) {
    try { Object.defineProperty(o, name, { value: value, configurable: true, writable: true, enumerable: false }); } catch (e) { o[name] = value; }
  }
  function method(proto, name, fn) {
    try { Object.defineProperty(proto, name, { value: fn, configurable: true, writable: true, enumerable: true }); } catch (e) { proto[name] = fn; }
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
    return C;
  }

  // ---------------------------------------------------------------------
  // Event targets (DOM §2.7) for the objects of this file and the worker
  // global. Own registry, so that an exception in a listener is reported
  // to the global's error handlers (HTML §8.1.4.6 "report an exception"),
  // which for a worker is how it reaches the Worker object.
  // ---------------------------------------------------------------------
  var EVT = new WeakMap();
  var DISPATCHING = new WeakSet();
  // Operations of the global object called without a receiver
  // (`addEventListener(...)` in strict code) act on the global (WebIDL §3.7).
  function recv(t) {
    if (t === undefined || t === null) return g;
    if (typeof t !== 'object' && typeof t !== 'function') throw new TypeError('Illegal invocation');
    return t;
  }
  function slots(t) {
    t = recv(t);
    var s = EVT.get(t);
    if (!s) { s = { l: Object.create(null), h: Object.create(null) }; EVT.set(t, s); }
    return s;
  }
  function flatten(opts) {
    if (typeof opts === 'boolean') return { capture: opts, once: false, passive: false, signal: null };
    if (opts && typeof opts === 'object') {
      return { capture: !!opts.capture, once: !!opts.once, passive: !!opts.passive, signal: opts.signal === undefined ? null : opts.signal };
    }
    return { capture: false, once: false, passive: false, signal: null };
  }
  function addListener(t, type, cb, opts) {
    var o = flatten(opts);
    if (cb === null || cb === undefined) return;
    if (o.signal && o.signal.aborted) return;
    type = String(type);
    var s = slots(t), list = s.l[type] || (s.l[type] = []);
    for (var i = 0; i < list.length; i++) {
      if (list[i].cb === cb && list[i].capture === o.capture) return;
    }
    var rec = { cb: cb, capture: o.capture, once: o.once, removed: false };
    list.push(rec);
    if (o.signal && typeof o.signal.addEventListener === 'function') {
      o.signal.addEventListener('abort', function () { removeRec(t, type, rec); });
    }
  }
  function removeRec(t, type, rec) {
    var list = slots(t).l[type];
    if (!list) return;
    var i = list.indexOf(rec);
    if (i >= 0) { list.splice(i, 1); rec.removed = true; }
  }
  function removeListener(t, type, cb, opts) {
    var o = flatten(opts);
    type = String(type);
    var list = slots(t).l[type];
    if (!list) return;
    for (var i = 0; i < list.length; i++) {
      if (list[i].cb === cb && list[i].capture === o.capture) { list[i].removed = true; list.splice(i, 1); return; }
    }
  }
  function setField(ev, k, v) {
    try { Object.defineProperty(ev, k, { value: v, configurable: true, enumerable: true, writable: true }); } catch (e) { try { ev[k] = v; } catch (x) {} }
  }
  function trusted(ev) { setField(ev, 'isTrusted', true); return ev; }
  // Dispatches `ev` at `t`; true if not canceled.
  function dispatch(t, ev) {
    if (DISPATCHING.has(ev)) throw domError("Failed to execute 'dispatchEvent' on 'EventTarget': The event is already being dispatched.", 'InvalidStateError');
    DISPATCHING.add(ev);
    var stopped = false;
    var origStop = ev.stopImmediatePropagation;
    hide(ev, 'stopImmediatePropagation', function stopImmediatePropagation() {
      stopped = true;
      if (typeof origStop === 'function') { try { origStop.call(ev); } catch (e) {} }
    });
    setField(ev, 'target', t);
    setField(ev, 'currentTarget', t);
    setField(ev, 'eventPhase', 2);
    try { setField(ev, 'srcElement', t); } catch (e) {}
    var list = (slots(t).l[String(ev.type)] || []).slice();
    for (var i = 0; i < list.length && !stopped; i++) {
      var rec = list[i];
      if (rec.removed) continue;
      if (rec.once) removeRec(t, String(ev.type), rec);
      try {
        if (typeof rec.cb === 'function') rec.cb.call(t, ev);
        else {
          var he = rec.cb.handleEvent;
          if (typeof he !== 'function') throw new TypeError("The provided callback's 'handleEvent' property is not callable.");
          he.call(rec.cb, ev);
        }
      } catch (e) {
        reportException(e);
      }
    }
    setField(ev, 'currentTarget', null);
    setField(ev, 'eventPhase', 0);
    try { delete ev.stopImmediatePropagation; } catch (e) {}
    DISPATCHING.delete(ev);
    return !ev.defaultPrevented;
  }
  function targetMethods(proto) {
    method(proto, 'addEventListener', function addEventListener(type, cb, opts) {
      if (arguments.length < 2) throw typeError('addEventListener', 'EventTarget', '2 arguments required, but only ' + arguments.length + ' present.');
      addListener(recv(this), type, cb, opts);
    });
    method(proto, 'removeEventListener', function removeEventListener(type, cb, opts) {
      if (arguments.length < 2) throw typeError('removeEventListener', 'EventTarget', '2 arguments required, but only ' + arguments.length + ' present.');
      removeListener(recv(this), type, cb, opts);
    });
    method(proto, 'dispatchEvent', function dispatchEvent(ev) {
      if (!(ev instanceof BaseEvent)) throw typeError('dispatchEvent', 'EventTarget', "parameter 1 is not of type 'Event'.");
      setField(ev, 'isTrusted', false);
      return dispatch(recv(this), ev);
    });
  }
  // `on<type>` event handler attributes (HTML §8.1.8.1). `onSet` runs when
  // a handler is set (a port's onmessage starts the port).
  function handler(proto, type, onSet) {
    Object.defineProperty(proto, 'on' + type, {
      configurable: true, enumerable: true,
      get: function () { var h = slots(this).h[type]; return h ? h.fn : null; },
      set: function (v) {
        var target = recv(this);
        var s = slots(target);
        var fn = (typeof v === 'function' || (v !== null && typeof v === 'object')) ? v : null;
        var old = s.h[type];
        if (fn === null) {
          if (old) { removeRec(target, type, old.rec); delete s.h[type]; }
          return;
        }
        if (old) { old.fn = fn; }
        else {
          var holder = { fn: fn, rec: null };
          var listener = function (ev) { callHandler(target, holder, ev, type); };
          addListener(target, type, listener, false);
          var list = s.l[type];
          holder.rec = list[list.length - 1];
          s.h[type] = holder;
        }
        if (onSet) onSet(target);
      }
    });
  }
  function callHandler(target, holder, ev, type) {
    var fn = holder.fn;
    if (typeof fn !== 'function') return;
    if (type === 'error' && target === g && g.ErrorEvent && ev instanceof g.ErrorEvent) {
      var r = fn.call(target, ev.message, ev.filename, ev.lineno, ev.colno, ev.error);
      if (r === true) ev.preventDefault();
      return;
    }
    var res = fn.call(target, ev);
    if (res === false) ev.preventDefault();
  }

  // ---------------------------------------------------------------------
  // ErrorEvent (HTML §8.1.4.7). The engine's native one ignores its init
  // dictionary and is no instance of itself; then this one replaces it.
  // ---------------------------------------------------------------------
  (function () {
    var ok = false;
    try { var probe = new g.ErrorEvent('error', { message: 'm' }); ok = probe instanceof g.ErrorEvent && probe.message === 'm'; } catch (e) {}
    if (ok) return;
    var EES = new WeakMap();
    var ErrorEvent = iface('ErrorEvent', BaseEvent, function ErrorEvent(type, init) {
      if (!new.target) throw new TypeError("Failed to construct 'ErrorEvent': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
      if (arguments.length < 1) throw new TypeError("Failed to construct 'ErrorEvent': 1 argument required, but only 0 present.");
      if (init !== undefined && init !== null && typeof init !== 'object' && typeof init !== 'function') {
        throw new TypeError("Failed to construct 'ErrorEvent': The provided value is not of type 'ErrorEventInit'.");
      }
      var o = init || {};
      var e = new BaseEvent(type, o);
      try { Object.setPrototypeOf(e, new.target.prototype); } catch (x) {}
      EES.set(e, {
        message: o.message === undefined ? '' : String(o.message),
        filename: o.filename === undefined ? '' : String(o.filename),
        lineno: o.lineno === undefined ? 0 : (Number(o.lineno) >>> 0),
        colno: o.colno === undefined ? 0 : (Number(o.colno) >>> 0),
        error: o.error === undefined ? undefined : o.error
      });
      return e;
    });
    ['message', 'filename', 'lineno', 'colno', 'error'].forEach(function (k) {
      Object.defineProperty(ErrorEvent.prototype, k, {
        configurable: true, enumerable: true,
        get: function () { var s = EES.get(this); if (!s) throw new TypeError('Illegal invocation'); return s[k]; }
      });
    });
    hide(g, 'ErrorEvent', ErrorEvent);
  })();

  // ---------------------------------------------------------------------
  // Reporting exceptions (HTML §8.1.4.6)
  // ---------------------------------------------------------------------
  var selfHref = '';
  try { selfHref = String(g.location.href); } catch (e) {}
  function errorInfo(e) {
    var message = 'Uncaught';
    try { message = 'Uncaught ' + String(e); } catch (x) {}
    var line = 0, col = 0, file = '';
    try {
      var st = String(e && typeof e === 'object' && e.stack || '');
      var m = /\(?([^\s()]*):(\d+):(\d+)\)?/.exec(st);
      if (m) { line = Number(m[2]); col = Number(m[3]); file = m[1] && m[1] !== '<anonymous>' ? m[1] : ''; }
    } catch (x) {}
    try { if (e && typeof e === 'object' && typeof e.__narsilFile === 'string') file = e.__narsilFile; } catch (x) {}
    if (!file) file = selfHref;
    return { message: message, filename: file, lineno: line, colno: col };
  }
  function errorEvent(info, error) {
    var EE = g.ErrorEvent;
    var ev;
    try { ev = new EE('error', { message: info.message, filename: info.filename, lineno: info.lineno, colno: info.colno, error: error, cancelable: true }); } catch (x) { ev = new BaseEvent('error', { cancelable: true }); }
    var fields = { message: info.message, filename: info.filename, lineno: info.lineno, colno: info.colno, error: error };
    var fieldNames = ['message', 'filename', 'lineno', 'colno', 'error'];
    for (var fi = 0; fi < fieldNames.length; fi++) {
      var k = fieldNames[fi];
      var have;
      try { have = ev[k]; } catch (x) { have = undefined; }
      if (have !== fields[k]) setField(ev, k, fields[k]);
    }
    return trusted(ev);
  }
  // Fires `error` at this global; true if not canceled.
  function errorAtGlobal(info, error) {
    var ev = errorEvent(info, error);
    if (isWorker) return dispatch(g, ev);
    // The page: the handler lives on `window` (special onerror arguments).
    var W = g.window && g.window !== g ? g.window : g;
    var h = null;
    try { h = W.onerror; } catch (x) {}
    var notCanceled = true;
    if (typeof h === 'function') { try { W.onerror = null; } catch (x) {} }
    try { notCanceled = g.dispatchEvent(ev) !== false; } catch (x) {} finally {
      if (typeof h === 'function') { try { W.onerror = h; } catch (x) {} }
    }
    if (typeof h === 'function') {
      var r;
      try { r = h.call(W, info.message, info.filename, info.lineno, info.colno, error); } catch (x) {}
      if (r === true) notCanceled = false;
    }
    return notCanceled && !ev.defaultPrevented;
  }
  var reporting = 0;
  function reportException(e) {
    if (reporting > 4) return;
    reporting++;
    try {
      var info = errorInfo(e);
      // An exception while an error is being reported (a throwing onerror)
      // is not fired at the global again (HTML §8.1.4.6 "error reporting
      // mode"); it goes straight on.
      var notCanceled = reporting > 1 ? true : errorAtGlobal(info, e);
      if (notCanceled) {
        try { g.console.error(info.message + (e && typeof e === 'object' && e.stack ? ' ' + e.stack : '')); } catch (x) {}
        if (isWorker && !isShared && typeof H.WReportError === 'function') {
          H.WReportError(info.message, info.filename, info.lineno, info.colno);
        }
      }
    } finally { reporting--; }
  }
  // Called by the host after a script or timer threw (`report_uncaught`).
  hide(g, '__narsilWUncaught', function (text) {
    var e = typeof takeUncaught === 'function' ? takeUncaught() : undefined;
    if (e === undefined) {
      var t = String(text || '');
      e = t.replace(/^Laufzeitfehler: /, '').replace(/^(Unbehandelt|Ausnahme im (Timer|Handler)): /, '');
    }
    reportException(e);
  });
  // An exception in the body of a module script (the engine calls this,
  // `module.rs`): reported like one in a classic script. Workers only —
  // the page reports module errors on its console as before.
  if (isWorker) hide(g, '__narsilModuleError', function (url, e) { reportException(e); });

  // ---------------------------------------------------------------------
  // MessageEvent (HTML §9.3)
  // ---------------------------------------------------------------------
  var ME = new WeakMap();
  var MessagePort;
  function isWindowLike(o) {
    try { return o === g || (g.window && o === g.window) || (o && typeof o === 'object' && o.window === o); } catch (e) { return false; }
  }
  function portList(v, what) {
    if (v === undefined) return [];
    if (v === null || typeof v !== 'object' || typeof v[Symbol.iterator] !== 'function') {
      throw new TypeError("Failed to construct 'MessageEvent': " + what + " The provided value cannot be converted to a sequence.");
    }
    var out = [];
    for (var x of v) {
      if (!MessagePort || !(x instanceof MessagePort)) throw new TypeError("Failed to construct 'MessageEvent': Failed to read the 'ports' property from 'MessageEventInit': Failed to convert value to 'MessagePort'.");
      out.push(x);
    }
    return out;
  }
  var MessageEvent = iface('MessageEvent', BaseEvent, function MessageEvent(type, init) {
    if (!new.target) throw new TypeError("Failed to construct 'MessageEvent': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
    if (arguments.length < 1) throw new TypeError("Failed to construct 'MessageEvent': 1 argument required, but only 0 present.");
    if (init !== undefined && init !== null && typeof init !== 'object' && typeof init !== 'function') {
      throw new TypeError("Failed to construct 'MessageEvent': The provided value is not of type 'MessageEventInit'.");
    }
    var o = init || {};
    var ports = portList(o.ports, "Failed to read the 'ports' property from 'MessageEventInit':");
    var source = o.source === undefined ? null : o.source;
    if (source !== null && !(MessagePort && source instanceof MessagePort) && !isWindowLike(source) && !(g.ServiceWorker && source instanceof g.ServiceWorker)) {
      throw new TypeError("Failed to construct 'MessageEvent': Failed to read the 'source' property from 'MessageEventInit': The provided value is not of type '(MessagePort or ServiceWorker or WindowProxy)'.");
    }
    var e = new BaseEvent(type, o);
    try { Object.setPrototypeOf(e, new.target.prototype); } catch (x) {}
    ME.set(e, {
      data: o.data === undefined ? null : o.data,
      origin: o.origin === undefined ? '' : String(o.origin),
      lastEventId: o.lastEventId === undefined ? '' : String(o.lastEventId),
      source: source,
      ports: Object.freeze(ports)
    });
    return e;
  });
  ['data', 'origin', 'lastEventId', 'source', 'ports'].forEach(function (k) {
    Object.defineProperty(MessageEvent.prototype, k, {
      configurable: true, enumerable: true,
      get: function () {
        var s = ME.get(this);
        if (!s) throw new TypeError('Illegal invocation');
        return s[k];
      }
    });
  });
  method(MessageEvent.prototype, 'initMessageEvent', function initMessageEvent(type, bubbles, cancelable, data, origin, lastEventId, source, ports) {
    if (arguments.length < 1) throw typeError('initMessageEvent', 'MessageEvent', '1 argument required, but only 0 present.');
    var s = ME.get(this);
    if (!s) throw new TypeError('Illegal invocation');
    if (DISPATCHING.has(this)) return;
    if (typeof this.initEvent === 'function') this.initEvent(type, !!bubbles, !!cancelable);
    s.data = data === undefined ? null : data;
    s.origin = origin === undefined ? '' : String(origin);
    s.lastEventId = lastEventId === undefined ? '' : String(lastEventId);
    s.source = source === undefined ? null : source;
    s.ports = Object.freeze(ports === undefined ? [] : portList(ports, ''));
  });
  hide(g, 'MessageEvent', MessageEvent);
  function messageEvent(type, data, ports, origin, source) {
    var e = new MessageEvent(type, { origin: origin || '', source: source || null });
    var s = ME.get(e);
    s.data = data;
    s.ports = Object.freeze(ports || []);
    return trusted(e);
  }

  // ---------------------------------------------------------------------
  // Transfer lists (HTML §9.4.3 "message port post message steps")
  // ---------------------------------------------------------------------
  var PORT = new WeakMap();
  var PORTS = new Map();
  function transferArg(arg, op, ifaceName) {
    if (arg === undefined || arg === null) return [];
    if (typeof arg !== 'object' && typeof arg !== 'function') {
      throw typeError(op, ifaceName, 'The provided value is not of type \'(sequence<object> or StructuredSerializeOptions)\'.');
    }
    var seq = arg;
    if (typeof arg[Symbol.iterator] !== 'function') {
      seq = arg.transfer;
      if (seq === undefined) return [];
      if (seq === null || typeof seq !== 'object' || typeof seq[Symbol.iterator] !== 'function') {
        throw typeError(op, ifaceName, "Failed to read the 'transfer' property from 'StructuredSerializeOptions': The provided value cannot be converted to a sequence.");
      }
    }
    var out = [];
    for (var x of seq) {
      if (x === null || (typeof x !== 'object' && typeof x !== 'function')) {
        throw typeError(op, ifaceName, 'Value at index ' + out.length + ' is not of type \'object\'.');
      }
      out.push(x);
    }
    return out;
  }
  function splitTransfer(list, op, ifaceName) {
    var buffers = [], ports = [];
    for (var i = 0; i < list.length; i++) {
      var x = list[i];
      if (buffers.indexOf(x) >= 0 || ports.indexOf(x) >= 0) {
        throw domError("Failed to execute '" + op + "' on '" + ifaceName + "': Value at index " + i + ' is a duplicate of an earlier value.', 'DataCloneError');
      }
      if (x instanceof ArrayBuffer) buffers.push(x);
      else if (PORT.has(x)) {
        var st = PORT.get(x);
        if (st.detached) throw domError("Failed to execute '" + op + "' on '" + ifaceName + "': Port at index " + i + ' is already neutered.', 'DataCloneError');
        ports.push(x);
      } else {
        throw domError("Failed to execute '" + op + "' on '" + ifaceName + "': Value at index " + i + ' does not have a transferable type.', 'DataCloneError');
      }
    }
    return { buffers: buffers, ports: ports };
  }
  var FORBIDDEN = [];
  // Platform objects that are not [Serializable] but are plain script
  // objects here (DataCloneError when posted).
  ['WorkerNavigator', 'WorkerLocation', 'WorkerGlobalScope', 'EventTarget', 'Event'].forEach(function (n) {
    var C = g[n];
    if (typeof C === 'function' && C.prototype) FORBIDDEN.push(C.prototype);
  });
  function pack(message, parts) {
    return serialize(message, parts.buffers, parts.ports, FORBIDDEN);
  }
  // The transferred ports leave this context: local objects are neutered.
  function portIds(ports) {
    var ids = [];
    for (var i = 0; i < ports.length; i++) {
      var st = PORT.get(ports[i]);
      st.detached = true;
      PORTS.delete(st.id);
      ids.push(st.id);
    }
    return ids;
  }
  function makePort(id) {
    var p = Object.create(MessagePort.prototype);
    PORT.set(p, { id: id, started: false, closed: false, detached: false });
    PORTS.set(id, p);
    return p;
  }
  function portsFor(ids) {
    var out = [];
    for (var i = 0; i < ids.length; i++) out.push(makePort(ids[i]));
    return out;
  }
  function unpack(handle, ports) {
    try { return { ok: true, data: deserialize(handle, ports) }; } catch (e) { return { ok: false, data: null }; }
  }

  // ---------------------------------------------------------------------
  // MessagePort, MessageChannel (HTML §9.4)
  // ---------------------------------------------------------------------
  MessagePort = iface('MessagePort', ET);
  FORBIDDEN.push(MessagePort.prototype);
  targetMethods(MessagePort.prototype);
  function portState(p, op) {
    var st = PORT.get(p);
    if (!st) throw new TypeError("Failed to execute '" + op + "' on 'MessagePort': Illegal invocation");
    return st;
  }
  function startPort(p) {
    var st = PORT.get(p);
    if (!st || st.started || st.closed || st.detached) return;
    st.started = true;
    H.WPortStart(st.id);
  }
  method(MessagePort.prototype, 'postMessage', function postMessage(message, transfer) {
    var st = portState(this, 'postMessage');
    if (arguments.length < 1) throw typeError('postMessage', 'MessagePort', '1 argument required, but only 0 present.');
    var parts = splitTransfer(transferArg(transfer, 'postMessage', 'MessagePort'), 'postMessage', 'MessagePort');
    if (parts.ports.indexOf(this) >= 0) {
      throw domError("Failed to execute 'postMessage' on 'MessagePort': Port at index " + parts.ports.indexOf(this) + ' contains the source port.', 'DataCloneError');
    }
    var h = pack(message, parts);
    var ids = portIds(parts.ports);
    if (st.detached || st.closed) {
      for (var i = 0; i < ids.length; i++) H.WPortClose(ids[i]);
      return;
    }
    H.WPortPost(st.id, h, ids);
  });
  method(MessagePort.prototype, 'start', function start() { portState(this, 'start'); startPort(this); });
  method(MessagePort.prototype, 'close', function close() {
    var st = portState(this, 'close');
    if (st.closed) return;
    st.closed = true;
    // close() detaches the port (HTML §9.4.4): it can no longer be
    // transferred (DataCloneError), its entangled port stays usable.
    if (!st.detached) { st.detached = true; PORTS.delete(st.id); H.WPortClose(st.id); }
  });
  handler(MessagePort.prototype, 'message', startPort);
  handler(MessagePort.prototype, 'messageerror');
  hide(g, 'MessagePort', MessagePort);

  var MessageChannel = iface('MessageChannel', null, function MessageChannel() {
    if (!new.target) throw new TypeError("Failed to construct 'MessageChannel': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
    var ids = H.WPortPair();
    var p1 = makePort(ids[0]), p2 = makePort(ids[1]);
    Object.defineProperty(this, 'port1', { value: p1, enumerable: true, configurable: true });
    Object.defineProperty(this, 'port2', { value: p2, enumerable: true, configurable: true });
  });
  // `port1`/`port2` are attributes of the interface.
  ['port1', 'port2'].forEach(function (k) {
    Object.defineProperty(MessageChannel.prototype, k, { configurable: true, enumerable: true, get: function () { var d = Object.getOwnPropertyDescriptor(this, k); return d ? d.value : undefined; } });
  });
  hide(g, 'MessageChannel', MessageChannel);

  function deliverPort(id, handle, ids) {
    var p = PORTS.get(id);
    var ports = portsFor(ids);
    var st = p && PORT.get(p);
    if (!p || st.closed || st.detached) {
      unpack(handle, ports);
      for (var i = 0; i < ids.length; i++) { PORTS.delete(ids[i]); H.WPortClose(ids[i]); }
      return;
    }
    var r = unpack(handle, ports);
    dispatch(p, r.ok ? messageEvent('message', r.data, ports) : messageEvent('messageerror', null, []));
  }

  // ---------------------------------------------------------------------
  // BroadcastChannel (HTML §9.5)
  // ---------------------------------------------------------------------
  var BC = new WeakMap();
  var CHANNELS = new Map();
  var BroadcastChannel = iface('BroadcastChannel', ET, function BroadcastChannel(name) {
    if (!new.target) throw new TypeError("Failed to construct 'BroadcastChannel': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
    if (arguments.length < 1) throw new TypeError("Failed to construct 'BroadcastChannel': 1 argument required, but only 0 present.");
    var n = String(name);
    var id = H.WBcOpen(n);
    BC.set(this, { id: id, name: n, closed: false });
    CHANNELS.set(id, this);
  });
  FORBIDDEN.push(BroadcastChannel.prototype);
  targetMethods(BroadcastChannel.prototype);
  function bcState(c, op) {
    var st = BC.get(c);
    if (!st) throw new TypeError("Failed to execute '" + op + "' on 'BroadcastChannel': Illegal invocation");
    return st;
  }
  Object.defineProperty(BroadcastChannel.prototype, 'name', { configurable: true, enumerable: true, get: function () { return bcState(this, 'name').name; } });
  method(BroadcastChannel.prototype, 'postMessage', function postMessage(message) {
    var st = bcState(this, 'postMessage');
    if (arguments.length < 1) throw typeError('postMessage', 'BroadcastChannel', '1 argument required, but only 0 present.');
    if (st.closed) throw domError("Failed to execute 'postMessage' on 'BroadcastChannel': Channel is closed", 'InvalidStateError');
    // A worker that called close() is not eligible for messaging (HTML §9.6).
    if (closing) return;
    var h = serialize(message, [], [], FORBIDDEN);
    H.WBcPost(st.id, h, []);
  });
  method(BroadcastChannel.prototype, 'close', function close() {
    var st = bcState(this, 'close');
    if (st.closed) return;
    st.closed = true;
    CHANNELS.delete(st.id);
    H.WBcClose(st.id);
  });
  handler(BroadcastChannel.prototype, 'message');
  handler(BroadcastChannel.prototype, 'messageerror');
  hide(g, 'BroadcastChannel', BroadcastChannel);
  function deliverBroadcast(id, handle, origin) {
    var c = CHANNELS.get(id);
    if (!c || BC.get(c).closed) { unpack(handle, []); return; }
    var r = unpack(handle, []);
    dispatch(c, r.ok ? messageEvent('message', r.data, [], origin) : messageEvent('messageerror', null, [], origin));
  }

  // ---------------------------------------------------------------------
  // blob: URLs shared between the page, its frames and its workers (File
  // API §8): the host keeps a copy of each Blob under its URL.
  // ---------------------------------------------------------------------
  (function () {
    var U = g.URL, B = g.Blob;
    if (typeof U !== 'function' || typeof B !== 'function' || typeof H.WBlobPut !== 'function') return;
    var origCreate = U.createObjectURL, origRevoke = U.revokeObjectURL;
    var origLookup = g.__narsilBlobFuer;
    var local = Object.create(null);
    function origin() { try { var o = String(g.location.origin); return o && o !== 'null' ? o : 'null'; } catch (e) { return 'null'; } }
    function uuid() {
      try { if (g.crypto && typeof g.crypto.randomUUID === 'function') return g.crypto.randomUUID(); } catch (e) {}
      return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, function (c) { var r = Math.random() * 16 | 0; return (c === 'x' ? r : (r & 3 | 8)).toString(16); });
    }
    function def(o, n, f) { try { Object.defineProperty(o, n, { value: f, configurable: true, writable: true, enumerable: true }); } catch (e) { o[n] = f; } }
    def(U, 'createObjectURL', function createObjectURL(obj) {
      if (arguments.length < 1) throw new TypeError("Failed to execute 'createObjectURL' on 'URL': 1 argument required, but only 0 present.");
      var u;
      if (typeof origCreate === 'function') u = origCreate.apply(this, arguments);
      else {
        if (!(obj instanceof B)) throw new TypeError("Failed to execute 'createObjectURL' on 'URL': Overload resolution failed.");
        u = 'blob:' + origin() + '/' + uuid();
        local[u] = obj;
      }
      if (obj instanceof B) { try { H.WBlobPut(u, serialize(obj, [], [], [])); } catch (e) {} }
      return u;
    });
    def(U, 'revokeObjectURL', function revokeObjectURL(u) {
      if (arguments.length < 1) throw new TypeError("Failed to execute 'revokeObjectURL' on 'URL': 1 argument required, but only 0 present.");
      if (typeof origRevoke === 'function') origRevoke.apply(this, arguments);
      var s = String(u);
      delete local[s];
      try { H.WBlobDrop(s); } catch (e) {}
    });
    // The Blob behind a blob: URL: this context's own, else the shared copy.
    var lookup = function (u) {
      var s = String(u).split('#')[0];
      var b = typeof origLookup === 'function' ? origLookup(s) : null;
      if (b) return b;
      if (local[s]) return local[s];
      var h = 0;
      try { h = H.WBlobGet(s); } catch (e) { h = 0; }
      if (!h) return null;
      try { return deserialize(h, []); } catch (e) { return null; }
    };
    hide(g, '__narsilBlobFuer', lookup);
    // Blob URLs the page made before this script loaded (`lazy_apis.js`).
    var early = g.__narsilEarlyBlobs;
    try { delete g.__narsilEarlyBlobs; } catch (e) {}
    if (typeof early === 'function') {
      early().forEach(function (e) {
        if (e[1] instanceof B) { try { H.WBlobPut(e[0], serialize(e[1], [], [], [])); } catch (x) {} }
      });
    }
    // A Response for a Blob (Fetch §4.2 "scheme fetch", blob:); the engine's
    // Response keeps no binary body, so the methods read the Blob.
    function blobResponse(b, url) {
      var R = g.Response;
      var r = Object.create(R && R.prototype ? R.prototype : Object.prototype);
      var headers = typeof g.Headers === 'function' ? new g.Headers({ 'Content-Type': b.type || '', 'Content-Length': String(b.size) }) : {};
      var used = false;
      function once(f) {
        return function () {
          if (used) return Promise.reject(new TypeError('Body has already been consumed.'));
          used = true;
          return f();
        };
      }
      var fields = {
        ok: true, status: 200, statusText: 'OK', url: url, type: 'basic', redirected: false, headers: headers,
        text: once(function () { return b.text(); }),
        arrayBuffer: once(function () { return b.arrayBuffer(); }),
        blob: once(function () { return Promise.resolve(b); }),
        json: once(function () { return b.text().then(function (s) { return JSON.parse(s); }); }),
        clone: function () { return blobResponse(b, url); }
      };
      Object.keys(fields).forEach(function (k) { try { Object.defineProperty(r, k, { value: fields[k], configurable: true, enumerable: true }); } catch (e) {} });
      try { Object.defineProperty(r, 'bodyUsed', { get: function () { return used; }, configurable: true }); } catch (e) {}
      return r;
    }
    // fetch(blob:) for URLs the page's own registry does not know.
    var innerFetch = g.fetch;
    if (typeof innerFetch === 'function') {
      hide(g, 'fetch', function fetch(input, init) {
        var u = typeof input === 'string' ? input : (input && typeof input.url === 'string' ? input.url : (input && input.href) || '');
        if (/^blob:/i.test(String(u)) && !(typeof origLookup === 'function' && origLookup(String(u)))) {
          var b = lookup(u);
          var m = String((init && init.method) || (input && input.method) || 'GET').toUpperCase();
          if (!b || m !== 'GET') return Promise.reject(new TypeError('Failed to fetch'));
          return Promise.resolve(blobResponse(b, String(u).split('#')[0]));
        }
        return innerFetch.apply(this, arguments);
      });
    }
  })();

  // ---------------------------------------------------------------------
  // Worker, SharedWorker (HTML §10.2.6)
  // ---------------------------------------------------------------------
  function ownOrigin() {
    try { return String(g.location.origin); } catch (e) { return 'null'; }
  }
  function baseUrl() {
    try { if (!isWorker && g.document && g.document.baseURI) return String(g.document.baseURI); } catch (e) {}
    try { return String(g.location.href); } catch (e) { return ''; }
  }
  function originOf(u) {
    try {
      var x = new g.URL(u);
      if (x.protocol === 'blob:') { try { return new g.URL(x.pathname).origin; } catch (e) { return 'null'; } }
      return x.origin;
    } catch (e) { return 'null'; }
  }
  // URL parsing, the same-origin rule and blob: bodies for a new worker.
  function workerUrl(url, ctorName) {
    var abs;
    try { abs = new g.URL(url, baseUrl()).href; } catch (e) {
      throw domError("Failed to construct '" + ctorName + "': The URL '" + url + "' is invalid.", 'SyntaxError');
    }
    var scheme = abs.slice(0, abs.indexOf(':') + 1).toLowerCase();
    var pageScheme = '';
    try { pageScheme = String(g.location.protocol).toLowerCase(); } catch (e) {}
    // A file: page may start file: workers (as Firefox does); a blob: URL
    // of this page's own Blob is same-origin by construction.
    var ownBlob = scheme === 'blob:' && typeof g.__narsilBlobFuer === 'function' && !!g.__narsilBlobFuer(abs);
    var sameFile = scheme === 'file:' && pageScheme === 'file:';
    // A cross-origin script is not an exception any more: its fetch fails
    // (mode "same-origin"), and the worker fires `error` (HTML §10.2.6.3).
    var blocked = false;
    if (scheme !== 'data:' && !sameFile && !ownBlob) {
      var mine = ownOrigin();
      if (mine === 'null' || originOf(abs) !== mine) blocked = true;
    }
    var body = 0;
    if (scheme === 'blob:' && typeof g.__narsilBlobFuer === 'function') {
      var b = g.__narsilBlobFuer(abs);
      if (b) { try { body = serialize(b, [], [], []); } catch (e) { body = 0; } }
    }
    return { url: abs, body: body, blocked: blocked };
  }
  function enumOption(v, allowed, dflt, ctorName, typeName) {
    if (v === undefined) return dflt;
    var s = String(v);
    if (allowed.indexOf(s) < 0) throw new TypeError("Failed to construct '" + ctorName + "': Failed to read the '" + typeName[0] + "' property from 'WorkerOptions': The provided value '" + s + "' is not a valid enum value of type " + typeName[1] + '.');
    return s;
  }

  var WK = new WeakMap();
  var WORKERS = new Map();
  var Worker = iface('Worker', ET, function Worker(url, options) {
    if (!new.target) throw new TypeError("Failed to construct 'Worker': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
    if (arguments.length < 1) throw new TypeError("Failed to construct 'Worker': 1 argument required, but only 0 present.");
    var u = String(url);
    if (options !== undefined && options !== null && typeof options !== 'object' && typeof options !== 'function') {
      throw new TypeError("Failed to construct 'Worker': The provided value is not of type 'WorkerOptions'.");
    }
    var o = options || {};
    var type = enumOption(o.type, ['classic', 'module'], 'classic', 'Worker', ['type', 'WorkerType']);
    enumOption(o.credentials, ['omit', 'same-origin', 'include'], 'same-origin', 'Worker', ['credentials', 'RequestCredentials']);
    var name = o.name === undefined ? '' : String(o.name);
    var w = workerUrl(u, 'Worker');
    var id = H.WWorkerNew(w.url, type, name, '', w.body, w.blocked);
    WK.set(this, { id: id, terminated: false });
    WORKERS.set(id, this);
  });
  FORBIDDEN.push(Worker.prototype);
  targetMethods(Worker.prototype);
  function workerState(w, op) {
    var st = WK.get(w);
    if (!st) throw new TypeError("Failed to execute '" + op + "' on 'Worker': Illegal invocation");
    return st;
  }
  method(Worker.prototype, 'postMessage', function postMessage(message, transfer) {
    var st = workerState(this, 'postMessage');
    if (arguments.length < 1) throw typeError('postMessage', 'Worker', '1 argument required, but only 0 present.');
    var parts = splitTransfer(transferArg(transfer, 'postMessage', 'Worker'), 'postMessage', 'Worker');
    var h = pack(message, parts);
    var ids = portIds(parts.ports);
    if (st.terminated) { for (var i = 0; i < ids.length; i++) H.WPortClose(ids[i]); return; }
    H.WWorkerPost(st.id, h, ids);
  });
  method(Worker.prototype, 'terminate', function terminate() {
    var st = workerState(this, 'terminate');
    if (st.terminated) return;
    st.terminated = true;
    WORKERS.delete(st.id);
    H.WWorkerTerminate(st.id);
  });
  handler(Worker.prototype, 'message');
  handler(Worker.prototype, 'messageerror');
  handler(Worker.prototype, 'error');
  hide(g, 'Worker', Worker);

  function liveWorker(id) {
    var w = WORKERS.get(id);
    return w && !WK.get(w).terminated ? w : null;
  }
  function deliverFromWorker(id, handle, ids) {
    var ports = portsFor(ids);
    var r = unpack(handle, ports);
    var w = liveWorker(id);
    if (!w) return;
    dispatch(w, r.ok ? messageEvent('message', r.data, ports) : messageEvent('messageerror', null, []));
  }
  function workerError(id, message, filename, lineno, colno) {
    var w = liveWorker(id);
    if (!w) return;
    var info = { message: message, filename: filename, lineno: lineno, colno: colno };
    var notCanceled = dispatch(w, errorEvent(info, null));
    // Not canceled: as if the error happened in this global (HTML §10.2.5).
    if (notCanceled) {
      if (isWorker) {
        if (errorAtGlobal(info, null) && !isShared && typeof H.WReportError === 'function') H.WReportError(message, filename, lineno, colno);
      } else {
        errorAtGlobal(info, null);
      }
    }
  }
  function workerFailed(id) {
    var w = liveWorker(id);
    if (!w) return;
    dispatch(w, trusted(new BaseEvent('error', { cancelable: true })));
  }

  var SW = new WeakMap();
  var SHARED = new Map();
  if (!isShared) {
    var SharedWorker = iface('SharedWorker', ET, function SharedWorker(url, options) {
      if (!new.target) throw new TypeError("Failed to construct 'SharedWorker': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
      if (arguments.length < 1) throw new TypeError("Failed to construct 'SharedWorker': 1 argument required, but only 0 present.");
      var u = String(url);
      var name = '', type = 'classic', credentials = 'same-origin', extended = false;
      if (typeof options === 'string' || (options !== undefined && options !== null && typeof options !== 'object' && typeof options !== 'function')) {
        name = String(options);
      } else if (options) {
        if (options.name !== undefined) name = String(options.name);
        type = enumOption(options.type, ['classic', 'module'], 'classic', 'SharedWorker', ['type', 'WorkerType']);
        credentials = enumOption(options.credentials, ['omit', 'same-origin', 'include'], 'same-origin', 'SharedWorker', ['credentials', 'RequestCredentials']);
        extended = !!options.extendedLifetime;
      }
      var w = workerUrl(u, 'SharedWorker');
      // A running shared worker with other options refuses the connection
      // (HTML §10.2.6.4 step 11.3: type and credentials must match).
      var r = H.WSharedNew(w.url, name, type, w.body, w.blocked, type + '|' + credentials + '|' + extended);
      var port = makePort(r[1]);
      SW.set(this, { id: r[0], port: port });
      SHARED.set(r[0], this);
    });
    FORBIDDEN.push(SharedWorker.prototype);
    targetMethods(SharedWorker.prototype);
    Object.defineProperty(SharedWorker.prototype, 'port', {
      configurable: true, enumerable: true,
      get: function () { var st = SW.get(this); if (!st) throw new TypeError('Illegal invocation'); return st.port; }
    });
    handler(SharedWorker.prototype, 'error');
    hide(g, 'SharedWorker', SharedWorker);
  }
  function sharedFailed(id) {
    var w = SHARED.get(id);
    if (w) dispatch(w, trusted(new BaseEvent('error', { cancelable: true })));
  }

  // ---------------------------------------------------------------------
  // The worker global (HTML §10.2.1)
  // ---------------------------------------------------------------------
  if (isWorker) {
    // The global object is an event target with its own registry.
    targetMethods(g);
    ['error', 'languagechange', 'offline', 'online', 'rejectionhandled', 'unhandledrejection'].forEach(function (t) { handler(g, t); });
    if (isShared) {
      handler(g, 'connect');
    } else {
      handler(g, 'message');
      handler(g, 'messageerror');
      hide(g, 'postMessage', function postMessage(message, transfer) {
        if (arguments.length < 1) throw typeError('postMessage', 'DedicatedWorkerGlobalScope', '1 argument required, but only 0 present.');
        var parts = splitTransfer(transferArg(transfer, 'postMessage', 'DedicatedWorkerGlobalScope'), 'postMessage', 'DedicatedWorkerGlobalScope');
        var h = pack(message, parts);
        H.WSelfPost(h, portIds(parts.ports));
      });
    }
    hide(g, 'close', function close() { closing = true; H.WClose(); });
    // `onmessage = f` at the top level of the script assigns the global
    // property; an engine that made a data property instead is still heard.
    var fallback = function (name) {
      var d = Object.getOwnPropertyDescriptor(g, 'on' + name);
      return d && 'value' in d && typeof d.value === 'function' ? d.value : null;
    };
    var deliverGlobal = function (handle, ids) {
      if (closing) return;
      var ports = portsFor(ids);
      var r = unpack(handle, ports);
      var ev = r.ok ? messageEvent('message', r.data, ports) : messageEvent('messageerror', null, []);
      var f = fallback(r.ok ? 'message' : 'messageerror');
      if (f) { try { f.call(g, ev); } catch (e) { reportException(e); } }
      dispatch(g, ev);
    };
    var connect = function (id) {
      var port = makePort(id);
      var ev = messageEvent('connect', '', [port], '', port);
      var f = fallback('connect');
      if (f) { try { f.call(g, ev); } catch (e) { reportException(e); } }
      dispatch(g, ev);
    };
  }

  // ---------------------------------------------------------------------
  // window.postMessage (HTML §9.3.3) — same engine for the page and its
  // frames: arguments, target origin, structured clone with transfer.
  // `__narsilPostPrep(args, targetWindowOrigin, sourceOrigin)` returns null
  // (dropped: the target origin does not match) or `make(source)`, which
  // builds the MessageEvent in the delivering task. The preamble's frame
  // windows use it too.
  // ---------------------------------------------------------------------
  if (!isWorker) {
    // The origin of the calling script: a frame document's own (a data:
    // frame is opaque, "null"); about:blank/srcdoc frames share the page's.
    var callerOrigin = function (fallback) {
      var f = g.__narsilRahmenFenster;
      if (!f || f === g) return fallback;
      try {
        var href = String(f.location.href);
        if (/^about:/i.test(href)) return fallback;
        var o = String(f.location.origin);
        return o && o !== 'undefined' ? o : 'null';
      } catch (e) { return fallback; }
    };
    var postPrep = function (args, targetWindowOrigin, sourceOrigin) {
      sourceOrigin = callerOrigin(sourceOrigin);
      var ifn = 'Window';
      if (args.length < 1) throw typeError('postMessage', ifn, '1 argument required, but only 0 present.');
      var message = args[0], targetOrigin = '/', list = [];
      var second = args[1];
      if (args.length >= 2 && (second === null || second === undefined || (typeof second === 'object' && !(second instanceof g.String)))) {
        var o = second || {};
        if (o.targetOrigin !== undefined) targetOrigin = String(o.targetOrigin);
        if (o.transfer !== undefined) {
          if (o.transfer === null || typeof o.transfer !== 'object' || typeof o.transfer[Symbol.iterator] !== 'function') {
            throw typeError('postMessage', ifn, "Failed to read the 'transfer' property from 'WindowPostMessageOptions': The provided value cannot be converted to a sequence.");
          }
          list = transferArg(o.transfer, 'postMessage', ifn);
        }
      } else {
        if (args.length >= 2) targetOrigin = String(second);
        if (args.length >= 3 && args[2] !== undefined) {
          var t3 = args[2];
          if (t3 === null || typeof t3 !== 'object' || typeof t3[Symbol.iterator] !== 'function') {
            throw typeError('postMessage', ifn, 'The provided value cannot be converted to a sequence.');
          }
          list = transferArg(t3, 'postMessage', ifn);
        }
      }
      var wanted = null;
      if (targetOrigin === '/') wanted = sourceOrigin;
      else if (targetOrigin !== '*') {
        try { wanted = new g.URL(targetOrigin).origin; } catch (e) {
          throw domError("Failed to execute 'postMessage' on 'Window': Invalid target origin '" + targetOrigin + "' in a call to 'postMessage'.", 'SyntaxError');
        }
      }
      var parts = splitTransfer(list, 'postMessage', ifn);
      var h = pack(message, parts);
      var ids = portIds(parts.ports);
      // The ports stay in this context, as new objects; they wait for start().
      if (ids.length) H.WPortRebind(ids);
      if (wanted !== null && wanted !== targetWindowOrigin) {
        unpack(h, []);
        for (var i = 0; i < ids.length; i++) H.WPortClose(ids[i]);
        return null;
      }
      return function make(source) {
        var ports = portsFor(ids);
        var r = unpack(h, ports);
        return r.ok ? messageEvent('message', r.data, ports, sourceOrigin, source) : messageEvent('messageerror', null, [], sourceOrigin, source);
      };
    };
    hide(g, '__narsilPostPrep', postPrep);
    var pageOrigin = function () { try { return String(g.location.origin); } catch (e) { return 'null'; } };
    hide(g, 'postMessage', function postMessage(message) {
      var origin = pageOrigin();
      var make = postPrep(arguments, origin, origin);
      var source = g.__narsilRahmenFenster || g.window || g;
      if (!make) return;
      g.setTimeout(function () {
        var ev = make(source);
        try { if (typeof g.onmessage === 'function') g.onmessage(ev); } catch (e) { reportException(e); }
        try { if (g.window && typeof g.window.dispatchEvent === 'function') g.window.dispatchEvent(ev); } catch (e) {}
      }, 0);
    });
  }

  // ---------------------------------------------------------------------
  // The pump (`worker.rs`): runs queued tasks, up to `max` of them or until
  // `budget` ms have passed, with a microtask checkpoint after each task
  // (HTML §8.1.7.3). One call for many tasks: a call from the host costs
  // far more than a task (MEASURED: 1000 port messages took 1.2-5.7 s with
  // one call per task, Worker-messageport.html).
  // ---------------------------------------------------------------------
  function runTask(t) {
    var kind = t[0];
    try {
      if (kind === 'p') deliverPort(t[1], t[2], t[3]);
      else if (kind === 'w') deliverFromWorker(t[1], t[2], t[3]);
      else if (kind === 'g') { if (isWorker) deliverGlobal(t[2], t[3]); }
      else if (kind === 'b') deliverBroadcast(t[1], t[2], t[3]);
      else if (kind === 'we') workerError(t[1], t[2], t[3], t[4], t[5]);
      else if (kind === 'wf') workerFailed(t[1]);
      else if (kind === 'c') { if (isWorker) connect(t[1]); }
      else if (kind === 'sf') sharedFailed(t[1]);
    } catch (e) {
      reportException(e);
    }
  }
  hide(g, '__narsilWPump', function (budget, max) {
    var limit = typeof max === 'number' && max > 1 ? max : 1;
    var end = Date.now() + (typeof budget === 'number' && budget > 0 ? budget : 0);
    var n = 0;
    for (;;) {
      // A worker that called close() runs no further task (HTML §10.2.6).
      if (closing) break;
      var t = H.WNext();
      if (t === null || t === undefined) break;
      runTask(t);
      n++;
      if (n >= limit || Date.now() >= end) break;
      try { checkpoint(); } catch (e) { reportException(e); }
    }
    return n;
  });
})();
