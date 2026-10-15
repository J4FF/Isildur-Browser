// The global scope of a dedicated or shared worker (HTML §10.2.1
// WorkerGlobalScope, §10.2.1.2 DedicatedWorkerGlobalScope, §10.2.1.3
// SharedWorkerGlobalScope). Runs first in every worker engine
// (`worker.rs`), before `messaging.js`; the engine's own globals (timers,
// fetch, URL, TextEncoder, Blob, crypto, Event, ...) are already there.
(function () {
  var g = globalThis;
  var info = typeof g.__narsilWSelf === 'function' ? g.__narsilWSelf() : ['about:blank', '', false, 'null'];
  try { delete g.__narsilWSelf; } catch (e) {}
  var href = String(info[0]), workerName = String(info[1]), shared = !!info[2], origin = String(info[3]);
  var nav = info[4] || {};

  function hidden(name, value) {
    try { Object.defineProperty(g, name, { value: value, configurable: true, writable: true, enumerable: false }); } catch (e) { g[name] = value; }
  }
  function own(name, value) {
    try { Object.defineProperty(g, name, { value: value, configurable: true, writable: true, enumerable: true }); } catch (e) { g[name] = value; }
  }
  function illegal(name, parent) {
    var C = function () { throw new TypeError('Illegal constructor'); };
    try { Object.defineProperty(C, 'name', { value: name }); } catch (e) {}
    C.prototype = Object.create(parent ? parent.prototype : Object.prototype);
    try { Object.defineProperty(C.prototype, 'constructor', { value: C, configurable: true, writable: true, enumerable: false }); } catch (e) {}
    if (typeof Symbol === 'function' && Symbol.toStringTag) {
      try { Object.defineProperty(C.prototype, Symbol.toStringTag, { value: name, configurable: true }); } catch (e) {}
    }
    if (parent) { try { Object.setPrototypeOf(C, parent); } catch (e) {} }
    hidden(name, C);
    return C;
  }

  // -- No document in a worker (HTML §10.2.1: the global is not a Window) --
  ['document', 'window', 'Document', 'Element', 'HTMLElement', 'Node', 'Image', '__narsilCe', 'requestIdleCallback', 'cancelIdleCallback'].forEach(function (n) {
    try { delete g[n]; } catch (e) {}
    if (n in g) { try { Object.defineProperty(g, n, { value: undefined, configurable: true, writable: true, enumerable: false }); } catch (e) {} }
  });
  if (shared) {
    ['requestAnimationFrame', 'cancelAnimationFrame'].forEach(function (n) { try { delete g[n]; } catch (e) {} });
  }

  // -- DOMException (WebIDL §3.14.1) --
  if (typeof g.DOMException === 'undefined') {
    var CODES = {
      IndexSizeError: 1, HierarchyRequestError: 3, WrongDocumentError: 4, InvalidCharacterError: 5,
      NoModificationAllowedError: 7, NotFoundError: 8, NotSupportedError: 9, InUseAttributeError: 10,
      InvalidStateError: 11, SyntaxError: 12, InvalidModificationError: 13, NamespaceError: 14,
      InvalidAccessError: 15, TypeMismatchError: 17, SecurityError: 18, NetworkError: 19, AbortError: 20,
      URLMismatchError: 21, QuotaExceededError: 22, TimeoutError: 23, InvalidNodeTypeError: 24, DataCloneError: 25
    };
    var DOMException = function DOMException(message, name) {
      if (!(this instanceof DOMException)) throw new TypeError("Failed to construct 'DOMException': Please use the 'new' operator");
      this.message = message === undefined ? '' : String(message);
      this.name = name === undefined ? 'Error' : String(name);
      this.code = CODES[this.name] || 0;
    };
    DOMException.prototype = Object.create(Error.prototype);
    DOMException.prototype.constructor = DOMException;
    for (var k in CODES) {
      var c = k.replace(/([a-z])([A-Z])/g, '$1_$2').toUpperCase().replace(/_ERROR$/, '_ERR');
      DOMException[c] = CODES[k];
      DOMException.prototype[c] = CODES[k];
    }
    hidden('DOMException', DOMException);
  }

  // -- Event subclasses a worker sees (ErrorEvent, PromiseRejectionEvent) --
  var Ev = g.Event;
  function eventClass(name, fields) {
    if (typeof g[name] !== 'undefined' || typeof Ev !== 'function') return;
    var C = function (type, init) {
      if (arguments.length < 1) throw new TypeError("Failed to construct '" + name + "': 1 argument required, but only 0 present.");
      var e = new Ev(type, init);
      init = init || {};
      for (var i = 0; i < fields.length; i++) {
        var f = fields[i][0], d = fields[i][1];
        var v = init[f] !== undefined ? init[f] : d;
        if (init[f] !== undefined) {
          if (d === '') v = String(v);
          else if (typeof d === 'number') v = Number(v) >>> 0;
          else if (typeof d === 'boolean') v = !!v;
        }
        try { Object.defineProperty(e, f, { value: v, enumerable: true, configurable: true }); } catch (x) { e[f] = v; }
      }
      try { Object.setPrototypeOf(e, C.prototype); } catch (x) {}
      return e;
    };
    C.prototype = Object.create(Ev.prototype);
    C.prototype.constructor = C;
    try { Object.defineProperty(C, 'name', { value: name }); } catch (x) {}
    hidden(name, C);
  }
  eventClass('ErrorEvent', [['message', ''], ['filename', ''], ['lineno', 0], ['colno', 0], ['error', undefined]]);
  eventClass('PromiseRejectionEvent', [['promise', null], ['reason', undefined]]);
  eventClass('CloseEvent', [['wasClean', false], ['code', 0], ['reason', '']]);
  eventClass('ProgressEvent', [['lengthComputable', false], ['loaded', 0], ['total', 0]]);

  // -- The scope interfaces and the prototype chain of the global --
  var ET = g.EventTarget;
  var WGS = illegal('WorkerGlobalScope', ET);
  var Scope = illegal(shared ? 'SharedWorkerGlobalScope' : 'DedicatedWorkerGlobalScope', WGS);
  try { Object.setPrototypeOf(g, Scope.prototype); } catch (e) {}
  own('self', g);

  // -- WorkerLocation (HTML §10.3.3) --
  var WL = illegal('WorkerLocation');
  var parsed = null;
  try { parsed = new g.URL(href); } catch (e) {}
  var loc = Object.create(WL.prototype);
  ['href', 'origin', 'protocol', 'host', 'hostname', 'port', 'pathname', 'search', 'hash'].forEach(function (k) {
    Object.defineProperty(WL.prototype, k, {
      configurable: true, enumerable: true,
      get: function () {
        if (k === 'href') return href;
        if (k === 'origin') return origin;
        return parsed ? String(parsed[k]) : '';
      }
    });
  });
  Object.defineProperty(WL.prototype, 'toString', { value: function toString() { return href; }, configurable: true, writable: true, enumerable: true });
  Object.defineProperty(g, 'location', { get: function () { return loc; }, configurable: true, enumerable: true });

  // -- WorkerNavigator (HTML §10.3.2) --
  var WN = illegal('WorkerNavigator');
  var navigatorObject = Object.create(WN.prototype);
  var navValues = {
    appCodeName: 'Mozilla', appName: 'Netscape', appVersion: String(nav.appVersion || '5.0'),
    platform: String(nav.platform || ''), product: 'Gecko', userAgent: String(nav.userAgent || ''),
    language: String(nav.language || 'en-US'), onLine: true,
    hardwareConcurrency: Number(nav.hardwareConcurrency || 4)
  };
  Object.keys(navValues).forEach(function (k) {
    Object.defineProperty(WN.prototype, k, { configurable: true, enumerable: true, get: function () { return navValues[k]; } });
  });
  var langs = Object.freeze([navValues.language]);
  Object.defineProperty(WN.prototype, 'languages', { configurable: true, enumerable: true, get: function () { return langs; } });
  Object.defineProperty(g, 'navigator', { get: function () { return navigatorObject; }, configurable: true, enumerable: true });

  // -- name (DedicatedWorkerGlobalScope / SharedWorkerGlobalScope) --
  // [Replaceable]: an assignment replaces the accessor by a data property.
  Object.defineProperty(g, 'name', {
    get: function () { return workerName; },
    set: function (v) { Object.defineProperty(g, 'name', { value: v, writable: true, enumerable: true, configurable: true }); },
    configurable: true, enumerable: true
  });
  own('isSecureContext', /^https:|^http:\/\/(127\.0\.0\.1|localhost|\[::1\])(:|\/|$)/.test(href) || origin === 'null');
  own('origin', origin);
  own('crossOriginIsolated', false);

  // -- setTimeout/setInterval with a string compile it when they fire
  //    (HTML §8.6 "timer initialization steps", step 9.2) --
  ['setTimeout', 'setInterval'].forEach(function (n) {
    var orig = g[n];
    if (typeof orig !== 'function') return;
    var wrapped = function (handler) {
      var args = Array.prototype.slice.call(arguments);
      if (typeof handler !== 'function') {
        var code = String(handler);
        args[0] = function () { (0, eval)(code); };
      }
      return orig.apply(g, args);
    };
    try { Object.defineProperty(wrapped, 'name', { value: n }); } catch (e) {}
    try { Object.defineProperty(wrapped, 'length', { value: orig.length }); } catch (e) {}
    own(n, wrapped);
  });

  // -- importScripts (HTML §10.3.4) --
  var importHost = g.__narsilWImport;
  try { delete g.__narsilWImport; } catch (e) {}
  // Each imported script runs as a classic script of this global: a strict
  // script's functions are globals too (an indirect eval would keep them).
  var runScript = typeof g.__narsilRunScript === 'function' ? g.__narsilRunScript : function (src) { return (0, eval)(src); };
  var isModule = !!info[5];
  own('importScripts', function importScripts() {
    if (isModule) throw new TypeError("Failed to execute 'importScripts' on 'WorkerGlobalScope': Module scripts don't support importScripts().");
    var urls = [];
    for (var i = 0; i < arguments.length; i++) {
      var u;
      try { u = new g.URL(String(arguments[i]), href).href; } catch (e) {
        throw new g.DOMException("Failed to execute 'importScripts' on 'WorkerGlobalScope': The URL '" + String(arguments[i]) + "' is invalid.", 'SyntaxError');
      }
      urls.push(u);
    }
    var bodies = [];
    for (var j = 0; j < urls.length; j++) {
      var r;
      try { r = importHost(urls[j]); } catch (e) {
        throw new g.DOMException("Failed to execute 'importScripts' on 'WorkerGlobalScope': The script at '" + urls[j] + "' failed to load.", 'NetworkError');
      }
      bodies.push(r);
    }
    for (var k = 0; k < bodies.length; k++) {
      try {
        runScript(String(bodies[k][0]));
      } catch (e) {
        // The error comes from that script (its `filename` when reported);
        // the innermost importScripts() names it first.
        if (e && typeof e === 'object' && !Object.prototype.hasOwnProperty.call(e, '__narsilFile')) {
          try { Object.defineProperty(e, '__narsilFile', { value: String(bodies[k][1] || urls[k]), configurable: true }); } catch (x) {}
        }
        throw e;
      }
    }
  });
})();

// -- XMLHttpRequest in a worker (XHR §4): the request runs on the worker
//    thread (`__narsilWXhr` blocks it); asynchronous requests deliver their
//    events in a later task. --
(function () {
  var g = globalThis;
  var hostXhr = g.__narsilWXhr;
  try { delete g.__narsilWXhr; } catch (e) {}
  var serialize = g.__narsilSerialize;
  if (typeof hostXhr !== 'function' || typeof g.XMLHttpRequest !== 'undefined') return;
  var ET = g.EventTarget;
  var PE = g.ProgressEvent || g.Event;
  function defineClass(name, parent, ctor) {
    var C = ctor || function () { throw new TypeError('Illegal constructor'); };
    try { Object.defineProperty(C, 'name', { value: name }); } catch (e) {}
    C.prototype = Object.create(parent.prototype);
    try { Object.defineProperty(C.prototype, 'constructor', { value: C, writable: true, configurable: true }); } catch (e) {}
    try { Object.setPrototypeOf(C, parent); } catch (e) {}
    try { Object.defineProperty(g, name, { value: C, writable: true, configurable: true, enumerable: false }); } catch (e) { g[name] = C; }
    return C;
  }
  var XET = defineClass('XMLHttpRequestEventTarget', ET);
  var XU = defineClass('XMLHttpRequestUpload', XET);
  var S = new WeakMap();
  var XHR = defineClass('XMLHttpRequest', XET, function XMLHttpRequest() {
    if (!new.target) throw new TypeError("Failed to construct 'XMLHttpRequest': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
    S.set(this, {
      state: 0, method: 'GET', url: '', async: true, headers: [], sent: false, status: 0, statusText: '',
      respHeaders: [], body: null, type: '', mime: null, responseURL: '', aborted: false, error: false,
      timeout: 0, withCredentials: false, upload: Object.create(XU.prototype)
    });
  });
  ['UNSENT', 'OPENED', 'HEADERS_RECEIVED', 'LOADING', 'DONE'].forEach(function (k, i) {
    Object.defineProperty(XHR, k, { value: i, enumerable: true });
    Object.defineProperty(XHR.prototype, k, { value: i, enumerable: true });
  });
  function st(x) { var s = S.get(x); if (!s) throw new TypeError('Illegal invocation'); return s; }
  function fire(x, type, loaded, total) {
    var ev;
    try { ev = new PE(type, { lengthComputable: total > 0, loaded: loaded || 0, total: total || 0 }); } catch (e) { ev = new g.Event(type); }
    try { Object.defineProperty(ev, 'isTrusted', { value: true, configurable: true, enumerable: true }); } catch (e) {}
    var h = x['on' + type];
    if (typeof h === 'function') { try { h.call(x, ev); } catch (e) { if (typeof g.reportError === 'function') g.reportError(e); } }
    try { x.dispatchEvent(ev); } catch (e) {}
  }
  function setState(x, s, n) { s.state = n; fire(x, 'readystatechange'); }
  ['readystatechange', 'loadstart', 'progress', 'abort', 'error', 'load', 'timeout', 'loadend'].forEach(function (t) {
    var key = '__on' + t;
    Object.defineProperty(XET.prototype, 'on' + t, {
      configurable: true, enumerable: true,
      get: function () { return this[key] || null; },
      set: function (v) { try { Object.defineProperty(this, key, { value: typeof v === 'function' ? v : null, writable: true, configurable: true }); } catch (e) {} }
    });
  });
  function getter(name, fn) { Object.defineProperty(XHR.prototype, name, { configurable: true, enumerable: true, get: fn }); }
  getter('readyState', function () { return st(this).state; });
  getter('status', function () { return st(this).status; });
  getter('statusText', function () { return st(this).statusText; });
  getter('responseURL', function () { return st(this).responseURL; });
  getter('upload', function () { return st(this).upload; });
  getter('responseXML', function () { return null; });
  Object.defineProperty(XHR.prototype, 'responseType', {
    configurable: true, enumerable: true,
    get: function () { return st(this).type; },
    set: function (v) {
      var s = st(this);
      if (s.state >= 3) throw new g.DOMException("Failed to set the 'responseType' property on 'XMLHttpRequest': The response type cannot be set if the object's state is LOADING or DONE.", 'InvalidStateError');
      if (['', 'arraybuffer', 'blob', 'document', 'json', 'text'].indexOf(String(v)) >= 0) s.type = String(v);
    }
  });
  Object.defineProperty(XHR.prototype, 'timeout', { configurable: true, enumerable: true, get: function () { return st(this).timeout; }, set: function (v) { st(this).timeout = Number(v) >>> 0; } });
  Object.defineProperty(XHR.prototype, 'withCredentials', { configurable: true, enumerable: true, get: function () { return st(this).withCredentials; }, set: function (v) { st(this).withCredentials = !!v; } });
  function charsetOf(s) {
    var ct = s.mime || header(s, 'content-type') || '';
    var m = /charset\s*=\s*"?([^";\s]+)/i.exec(ct);
    return m ? m[1] : 'utf-8';
  }
  function text(s) {
    if (!s.body) return '';
    try { return new g.TextDecoder(charsetOf(s)).decode(s.body); } catch (e) {
      try { return new g.TextDecoder('utf-8').decode(s.body); } catch (x) { return ''; }
    }
  }
  getter('responseText', function () {
    var s = st(this);
    if (s.type !== '' && s.type !== 'text') throw new g.DOMException("Failed to read the 'responseText' property from 'XMLHttpRequest': The value is only accessible if the object's 'responseType' is '' or 'text'.", 'InvalidStateError');
    return s.state < 3 ? '' : text(s);
  });
  getter('response', function () {
    var s = st(this);
    if (s.type === '' || s.type === 'text') return s.state < 3 ? '' : text(s);
    if (s.state !== 4 || s.error) return null;
    if (s.type === 'json') { try { return JSON.parse(text(s)); } catch (e) { return null; } }
    if (s.type === 'arraybuffer') { var b = s.body || new Uint8Array(0); return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength); }
    if (s.type === 'blob') { var ct = header(s, 'content-type') || ''; return new g.Blob([s.body || new Uint8Array(0)], { type: ct }); }
    return null;
  });
  function header(s, name) {
    var n = String(name).toLowerCase(), out = null;
    for (var i = 0; i < s.respHeaders.length; i++) {
      if (String(s.respHeaders[i][0]).toLowerCase() === n) out = out === null ? String(s.respHeaders[i][1]) : out + ', ' + s.respHeaders[i][1];
    }
    return out;
  }
  var TOKEN = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
  XHR.prototype.open = function open(method, url) {
    var s = st(this);
    if (arguments.length < 2) throw new TypeError("Failed to execute 'open' on 'XMLHttpRequest': 2 arguments required, but only " + arguments.length + ' present.');
    var m = String(method);
    if (!TOKEN.test(m)) throw new g.DOMException("Failed to execute 'open' on 'XMLHttpRequest': '" + m + "' is not a valid HTTP method.", 'SyntaxError');
    var up = m.toUpperCase();
    if (up === 'CONNECT' || up === 'TRACE' || up === 'TRACK') throw new g.DOMException("Failed to execute 'open' on 'XMLHttpRequest': '" + m + "' HTTP method is unsupported.", 'SecurityError');
    if (['DELETE', 'GET', 'HEAD', 'OPTIONS', 'POST', 'PUT'].indexOf(up) >= 0) m = up;
    var abs;
    try { abs = new g.URL(String(url), String(g.location.href)).href; } catch (e) {
      throw new g.DOMException("Failed to execute 'open' on 'XMLHttpRequest': Invalid URL", 'SyntaxError');
    }
    s.method = m; s.url = abs; s.async = arguments.length < 3 ? true : !!arguments[2];
    s.headers = []; s.sent = false; s.status = 0; s.statusText = ''; s.respHeaders = []; s.body = null;
    s.aborted = false; s.error = false; s.responseURL = '';
    if (s.state !== 1) setState(this, s, 1);
  };
  XHR.prototype.setRequestHeader = function setRequestHeader(name, value) {
    var s = st(this);
    if (s.state !== 1 || s.sent) throw new g.DOMException("Failed to execute 'setRequestHeader' on 'XMLHttpRequest': The object's state must be OPENED.", 'InvalidStateError');
    s.headers.push([String(name), String(value)]);
  };
  XHR.prototype.overrideMimeType = function overrideMimeType(m) { st(this).mime = String(m); };
  XHR.prototype.getResponseHeader = function getResponseHeader(name) { var s = st(this); return s.state < 2 ? null : header(s, name); };
  XHR.prototype.getAllResponseHeaders = function getAllResponseHeaders() {
    var s = st(this);
    if (s.state < 2) return '';
    return s.respHeaders.map(function (h) { return String(h[0]).toLowerCase() + ': ' + h[1]; }).sort().join('\r\n') + (s.respHeaders.length ? '\r\n' : '');
  };
  XHR.prototype.abort = function abort() {
    var s = st(this);
    s.aborted = true;
    if ((s.state === 1 && s.sent) || s.state === 2 || s.state === 3) {
      s.state = 4; s.sent = false; s.error = true;
      fire(this, 'readystatechange'); fire(this, 'abort'); fire(this, 'loadend');
    }
    if (s.state === 4) { s.state = 0; s.status = 0; s.body = null; }
  };
  function perform(s, body) {
    var r;
    try { r = hostXhr(s.method, s.url, s.headers, body); } catch (e) { r = null; }
    if (s.aborted) return false;
    if (!r) {
      s.error = true; s.state = 4; s.status = 0;
      return false;
    }
    s.status = Number(r[0]);
    s.statusText = s.status === 200 ? 'OK' : '';
    s.respHeaders = r[1] || [];
    s.body = r[2] || null;
    s.responseURL = s.url.split('#')[0];
    return true;
  }
  XHR.prototype.send = function send(body) {
    var s = st(this), x = this;
    if (s.state !== 1 || s.sent) throw new g.DOMException("Failed to execute 'send' on 'XMLHttpRequest': The object's state must be OPENED.", 'InvalidStateError');
    var handle = 0;
    if (body !== undefined && body !== null && s.method !== 'GET' && s.method !== 'HEAD') {
      var b = body;
      if (typeof b !== 'object' || (g.URLSearchParams && b instanceof g.URLSearchParams)) b = String(b);
      try { handle = serialize(b, [], [], []); } catch (e) { handle = serialize(String(b), [], [], []); }
    }
    s.sent = true;
    if (!s.async) {
      var ok = perform(s, handle);
      s.sent = false;
      s.state = 4;
      fire(x, 'readystatechange');
      if (!ok) {
        throw new g.DOMException("Failed to execute 'send' on 'XMLHttpRequest': Failed to load '" + s.url + "'.", 'NetworkError');
      }
      var len = s.body ? s.body.length : 0;
      fire(x, 'load', len, len);
      fire(x, 'loadend', len, len);
      return;
    }
    fire(x, 'loadstart');
    g.setTimeout(function () {
      if (s.aborted) return;
      var ok = perform(s, handle);
      s.sent = false;
      if (s.aborted) return;
      if (!ok) {
        s.state = 4;
        fire(x, 'readystatechange'); fire(x, 'error'); fire(x, 'loadend');
        return;
      }
      setState(x, s, 2);
      setState(x, s, 3);
      var n = s.body ? s.body.length : 0;
      fire(x, 'progress', n, n);
      setState(x, s, 4);
      fire(x, 'load', n, n);
      fire(x, 'loadend', n, n);
    }, 0);
  };
})();

// -- FileReader and FileReaderSync in a worker (File API §6, §7.1) --
// MEASURED before: FileReader was undefined in workers (the page has a
// polyfill in its preamble); WPT webmessaging Channel_postMessage_Blob
// .any.worker.html read the Blob with it.
(function () {
  var g = globalThis;
  var blobBytes = g.__narsilWBlobBytes;
  try { delete g.__narsilWBlobBytes; } catch (e) {}
  var serialize = g.__narsilSerialize;
  var ET = g.EventTarget, PE = g.ProgressEvent || g.Event, DE = g.DOMException;
  if (typeof g.Blob !== 'function' || typeof ET !== 'function') return;
  function defineClass(name, parent, ctor) {
    try { Object.defineProperty(ctor, 'name', { value: name }); } catch (e) {}
    ctor.prototype = Object.create(parent ? parent.prototype : Object.prototype);
    try { Object.defineProperty(ctor.prototype, 'constructor', { value: ctor, writable: true, configurable: true }); } catch (e) {}
    if (parent) { try { Object.setPrototypeOf(ctor, parent); } catch (e) {} }
    if (typeof Symbol === 'function' && Symbol.toStringTag) {
      try { Object.defineProperty(ctor.prototype, Symbol.toStringTag, { value: name, configurable: true }); } catch (e) {}
    }
    try { Object.defineProperty(g, name, { value: ctor, writable: true, configurable: true, enumerable: false }); } catch (e) { g[name] = ctor; }
    return ctor;
  }
  function checkBlob(b, op, iface) {
    if (!(b instanceof g.Blob)) throw new TypeError("Failed to execute '" + op + "' on '" + iface + "': parameter 1 is not of type 'Blob'.");
  }
  var B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  function base64(u) {
    var out = [], i;
    for (i = 0; i + 2 < u.length; i += 3) {
      var n = (u[i] << 16) | (u[i + 1] << 8) | u[i + 2];
      out.push(B64[n >> 18 & 63] + B64[n >> 12 & 63] + B64[n >> 6 & 63] + B64[n & 63]);
    }
    if (i < u.length) {
      var m = u[i] << 16 | (i + 1 < u.length ? u[i + 1] << 8 : 0);
      out.push(B64[m >> 18 & 63] + B64[m >> 12 & 63] + (i + 1 < u.length ? B64[m >> 6 & 63] : '=') + '=');
    }
    return out.join('');
  }
  function binary(u) {
    var s = '';
    for (var i = 0; i < u.length; i += 0x2000) s += String.fromCharCode.apply(null, Array.prototype.slice.call(u, i, i + 0x2000));
    return s;
  }
  // The result of a read of `bytes` (a Uint8Array) as `kind`.
  function convert(bytes, kind, blob, encoding) {
    if (kind === 'buffer') return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
    if (kind === 'binary') return binary(bytes);
    if (kind === 'url') return 'data:' + (blob.type || 'application/octet-stream') + ';base64,' + base64(bytes);
    var label = encoding === undefined ? 'utf-8' : String(encoding);
    var dec;
    try { dec = new g.TextDecoder(label); } catch (e) { dec = new g.TextDecoder('utf-8'); }
    return dec.decode(bytes);
  }

  var S = new WeakMap();
  var FileReader = defineClass('FileReader', ET, function FileReader() {
    if (!new.target) throw new TypeError("Failed to construct 'FileReader': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
    S.set(this, { state: 0, result: null, error: null, token: 0 });
  });
  ['EMPTY', 'LOADING', 'DONE'].forEach(function (k, i) {
    Object.defineProperty(FileReader, k, { value: i, enumerable: true });
    Object.defineProperty(FileReader.prototype, k, { value: i, enumerable: true });
  });
  function st(r) { var s = S.get(r); if (!s) throw new TypeError('Illegal invocation'); return s; }
  ['readyState', 'result', 'error'].forEach(function (n) {
    var key = n === 'readyState' ? 'state' : n;
    Object.defineProperty(FileReader.prototype, n, { configurable: true, enumerable: true, get: function () { return st(this)[key]; } });
  });
  ['loadstart', 'progress', 'load', 'abort', 'error', 'loadend'].forEach(function (t) {
    var key = '__on' + t;
    Object.defineProperty(FileReader.prototype, 'on' + t, {
      configurable: true, enumerable: true,
      get: function () { return this[key] || null; },
      set: function (v) { try { Object.defineProperty(this, key, { value: typeof v === 'function' ? v : null, writable: true, configurable: true }); } catch (e) {} }
    });
  });
  function fire(r, type, loaded, total) {
    var ev;
    try { ev = new PE(type, { lengthComputable: true, loaded: loaded, total: total }); } catch (e) { ev = new g.Event(type); }
    var h = r['on' + type];
    if (typeof h === 'function') { try { h.call(r, ev); } catch (e) { if (typeof g.reportError === 'function') g.reportError(e); } }
    try { r.dispatchEvent(ev); } catch (e) {}
  }
  function read(r, blob, kind, encoding, op) {
    var s = st(r);
    checkBlob(blob, op, 'FileReader');
    if (s.state === 1) throw new DE("Failed to execute '" + op + "' on 'FileReader': The object is already busy reading Blobs.", 'InvalidStateError');
    s.state = 1; s.result = null; s.error = null;
    var token = ++s.token, total = blob.size;
    setTimeout(function () { if (s.token === token) fire(r, 'loadstart', 0, total); }, 0);
    blob.arrayBuffer().then(function (buf) {
      setTimeout(function () {
        if (s.token !== token) return;
        var u = new Uint8Array(buf);
        fire(r, 'progress', u.length, total);
        if (s.token !== token) return;
        s.result = convert(u, kind, blob, encoding);
        s.state = 2;
        fire(r, 'load', u.length, total);
        if (s.state !== 1) fire(r, 'loadend', u.length, total);
      }, 0);
    }, function (e) {
      if (s.token !== token) return;
      s.state = 2; s.error = new DE('A requested file or directory could not be found at the time an operation was processed.', 'NotFoundError');
      fire(r, 'error', 0, total);
      if (s.state !== 1) fire(r, 'loadend', 0, total);
    });
  }
  FileReader.prototype.readAsArrayBuffer = function readAsArrayBuffer(blob) { read(this, blob, 'buffer', undefined, 'readAsArrayBuffer'); };
  FileReader.prototype.readAsBinaryString = function readAsBinaryString(blob) { read(this, blob, 'binary', undefined, 'readAsBinaryString'); };
  FileReader.prototype.readAsText = function readAsText(blob, encoding) { read(this, blob, 'text', encoding, 'readAsText'); };
  FileReader.prototype.readAsDataURL = function readAsDataURL(blob) { read(this, blob, 'url', undefined, 'readAsDataURL'); };
  FileReader.prototype.abort = function abort() {
    var s = st(this);
    if (s.state !== 1) return;
    s.token++; s.state = 2; s.result = null;
    fire(this, 'abort', 0, 0);
    if (s.state !== 1) fire(this, 'loadend', 0, 0);
  };

  // FileReaderSync: the bytes come from the host at once (`__narsilWBlobBytes`).
  if (typeof blobBytes === 'function' && typeof serialize === 'function') {
    var FileReaderSync = defineClass('FileReaderSync', null, function FileReaderSync() {
      if (!new.target) throw new TypeError("Failed to construct 'FileReaderSync': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
    });
    var sync = function (kind, op) {
      return function (blob, encoding) {
        checkBlob(blob, op, 'FileReaderSync');
        var u = blobBytes(serialize(blob, [], [], []));
        if (!(u instanceof Uint8Array)) u = new Uint8Array(0);
        return convert(u, kind, blob, encoding);
      };
    };
    FileReaderSync.prototype.readAsArrayBuffer = sync('buffer', 'readAsArrayBuffer');
    FileReaderSync.prototype.readAsBinaryString = sync('binary', 'readAsBinaryString');
    FileReaderSync.prototype.readAsText = sync('text', 'readAsText');
    FileReaderSync.prototype.readAsDataURL = sync('url', 'readAsDataURL');
  }
})();
