// WebSocket (https://websockets.spec.whatwg.org/) and EventSource (HTML
// "server-sent events"). Loaded lazily on the first use of either global.
// The connections run on threads in web_net.rs; while any is alive a poll
// timer drains their events (`N.poll()`) and dispatches them here.
(function (N, g) {
  'use strict';
  var defineProperty = Object.defineProperty;
  var TypeErrorCtor = TypeError;
  var U8 = Uint8Array;

  function domError(name, msg) {
    if (typeof g.DOMException === 'function') return new g.DOMException(msg, name);
    var e = new Error(msg);
    e.name = name;
    return e;
  }

  // ---- Event classes the engine may lack -----------------------------
  function eventClass(name, fields) {
    if (typeof g[name] === 'function') return g[name];
    var C = function (type, init) {
      if (new.target === undefined) throw new TypeErrorCtor("Failed to construct '" + name + "': Please use the 'new' operator");
      if (arguments.length < 1) throw new TypeErrorCtor("Failed to construct '" + name + "': 1 argument required");
      var e = new g.Event(type, init);
      init = init || {};
      for (var i = 0; i < fields.length; i++) {
        var f = fields[i][0], def = fields[i][1];
        var v = init[f] !== undefined ? init[f] : def;
        if (init[f] !== undefined) {
          if (def === '') v = String(v);
          else if (typeof def === 'number') v = Number(v);
          else if (typeof def === 'boolean') v = !!v;
        }
        defineProperty(e, f, { value: v, enumerable: true, configurable: true });
      }
      Object.setPrototypeOf(e, C.prototype);
      return e;
    };
    C.prototype = Object.create(g.Event.prototype);
    defineProperty(C.prototype, 'constructor', { value: C, writable: true, configurable: true });
    defineProperty(C, 'name', { value: name, configurable: true });
    defineProperty(g, name, { value: C, writable: true, enumerable: false, configurable: true });
    return C;
  }
  var MessageEventC = eventClass('MessageEvent', [['data', null], ['origin', ''], ['lastEventId', ''], ['source', null], ['ports', []]]);
  var CloseEventC = eventClass('CloseEvent', [['wasClean', false], ['code', 0], ['reason', '']]);

  // ---- Event handler attributes (onopen, onmessage, ...) -------------
  function handlers(proto, names) {
    names.forEach(function (n) {
      var key = 'on' + n;
      var get = function () { var s = state.get(this); return s && s.handlers[key] || null; };
      var set = function (fn) {
        var s = state.get(this);
        if (!s) throw new TypeErrorCtor('Illegal invocation');
        if (!(key in s.wrapped)) {
          s.wrapped[key] = true;
          var self = this;
          this.addEventListener(n, function (e) {
            var h = s.handlers[key];
            if (typeof h === 'function') return h.call(self, e);
          });
        }
        s.handlers[key] = (typeof fn === 'function' || (typeof fn === 'object' && fn !== null)) ? fn : null;
      };
      defineProperty(get, 'name', { value: 'get ' + key });
      defineProperty(set, 'name', { value: 'set ' + key });
      defineProperty(proto, key, { get: get, set: set, enumerable: true, configurable: true });
    });
  }

  var state = new WeakMap();
  var live = new Map();
  var timer = null;

  function origin() {
    try { return g.location && g.location.origin ? String(g.location.origin) : 'null'; } catch (e) { return 'null'; }
  }
  function baseUrl() {
    try { if (g.document && g.document.baseURI) return String(g.document.baseURI); } catch (e) {}
    try { return g.location ? String(g.location.href) : undefined; } catch (e) { return undefined; }
  }
  function parseUrl(url) {
    var b = baseUrl();
    return b === undefined ? new g.URL(url) : new g.URL(url, b);
  }

  function fire(target, ev) {
    try { target.dispatchEvent(ev); } catch (e) { if (typeof g.reportError === 'function') g.reportError(e); }
  }

  function pump() {
    var list = N.poll();
    for (var i = 0; i < list.length; i += 5) {
      var obj = live.get(list[i]);
      if (!obj) continue;
      var s = state.get(obj);
      s.on(list[i + 1], list[i + 2], list[i + 3], list[i + 4]);
    }
    if (live.size === 0 && timer !== null) {
      g.clearInterval(timer);
      timer = null;
    }
  }
  function watch(id, obj) {
    live.set(id, obj);
    if (timer === null) timer = g.setInterval(pump, 10);
  }

  function utf8Length(s) {
    var n = 0;
    for (var i = 0; i < s.length; i++) {
      var c = s.charCodeAt(i);
      if (c < 0x80) n += 1;
      else if (c < 0x800) n += 2;
      else if (c >= 0xd800 && c < 0xdc00 && i + 1 < s.length && (s.charCodeAt(i + 1) & 0xfc00) === 0xdc00) { n += 4; i++; }
      else n += 3;
    }
    return n;
  }
  // USVString: lone surrogates become U+FFFD.
  function usv(s) {
    s = String(s);
    return s.replace(/[\ud800-\udbff](?![\udc00-\udfff])|(^|[^\ud800-\udbff])[\udc00-\udfff]/g, function (m) {
      return m.length === 2 ? m[0] + '�' : '�';
    });
  }

  // ---- WebSocket -----------------------------------------------------
  var TOKEN = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
  class WebSocket extends g.EventTarget {
    constructor(url) {
      if (new.target === undefined) throw new TypeErrorCtor("Failed to construct 'WebSocket': Please use the 'new' operator");
      if (arguments.length < 1) throw new TypeErrorCtor("Failed to construct 'WebSocket': 1 argument required");
      var protocols = arguments.length > 1 && arguments[1] !== undefined ? arguments[1] : [];
      super();
      var u;
      try { u = parseUrl(usv(url)); } catch (e) { throw domError('SyntaxError', "Failed to construct 'WebSocket': The URL '" + url + "' is invalid."); }
      if (u.protocol === 'http:') u.protocol = 'ws:';
      else if (u.protocol === 'https:') u.protocol = 'wss:';
      if (u.protocol !== 'ws:' && u.protocol !== 'wss:') {
        throw domError('SyntaxError', "Failed to construct 'WebSocket': The URL's scheme must be 'ws' or 'wss'.");
      }
      if (u.hash || /#/.test(String(url))) {
        throw domError('SyntaxError', "Failed to construct 'WebSocket': The URL contains a fragment identifier.");
      }
      var list = typeof protocols === 'object' && protocols !== null && typeof protocols[Symbol.iterator] === 'function'
        ? Array.from(protocols, String) : [String(protocols)];
      var seen = {};
      list.forEach(function (p) {
        var k = p.toLowerCase();
        if (!TOKEN.test(p) || seen[k]) throw domError('SyntaxError', "Failed to construct 'WebSocket': The subprotocol '" + p + "' is invalid.");
        seen[k] = true;
      });
      try {
        if (g.location && g.location.protocol === 'https:' && u.protocol === 'ws:') {
          throw domError('SecurityError', "Failed to construct 'WebSocket': An insecure WebSocket connection may not be initiated from a page loaded over HTTPS.");
        }
      } catch (e) { if (e && e.name === 'SecurityError') throw e; }
      var self = this;
      var s = {
        url: u.href, readyState: 0, bufferedAmount: 0, protocol: '', extensions: '', binaryType: 'blob',
        handlers: {}, wrapped: {}, id: 0,
        on: function (kind, a, b, c) {
          if (kind === 0) {
            if (s.readyState !== 0) return;
            s.readyState = 1; s.protocol = a; s.extensions = b;
            fire(self, new g.Event('open'));
          } else if (kind === 1 || kind === 2) {
            if (s.readyState !== 1) return;
            var data = a;
            if (kind === 2) data = s.binaryType === 'arraybuffer' ? a : new g.Blob([a]);
            fire(self, new MessageEventC('message', { data: data, origin: new g.URL(s.url).origin }));
          } else if (kind === 3) {
            if (s.readyState === 3) return;
            s.readyState = 3;
            live.delete(s.id);
            fire(self, new CloseEventC('close', { wasClean: !!c, code: a, reason: b }));
          } else if (kind === 5) {
            fire(self, new g.Event('error'));
          } else if (kind === 6) {
            s.bufferedAmount = Math.max(0, s.bufferedAmount - a);
          }
        }
      };
      state.set(this, s);
      s.id = N.wsOpen(s.url, list, origin());
      watch(s.id, this);
    }
    get url() { return wsState(this).url; }
    get readyState() { return wsState(this).readyState; }
    get bufferedAmount() { return wsState(this).bufferedAmount; }
    get extensions() { return wsState(this).extensions; }
    get protocol() { return wsState(this).protocol; }
    get binaryType() { return wsState(this).binaryType; }
    set binaryType(v) {
      var s = wsState(this);
      v = String(v);
      if (v === 'blob' || v === 'arraybuffer') s.binaryType = v;
    }
    close() {
      var s = wsState(this);
      var code = arguments[0], reason = arguments[1];
      if (code !== undefined) {
        code = Number(code);
        code = code !== code ? 0 : Math.max(0, Math.min(65535, Math.round(code)));
        if (code !== 1000 && (code < 3000 || code > 4999)) {
          throw domError('InvalidAccessError', "Failed to execute 'close' on 'WebSocket': The close code must be either 1000, or between 3000 and 4999.");
        }
      }
      if (reason !== undefined) {
        reason = usv(reason);
        if (utf8Length(reason) > 123) throw domError('SyntaxError', "Failed to execute 'close' on 'WebSocket': The close reason must not be greater than 123 UTF-8 bytes.");
      }
      if (s.readyState === 2 || s.readyState === 3) return;
      s.readyState = 2;
      N.wsClose(s.id, code, reason === undefined ? '' : reason);
    }
    send(data) {
      var s = wsState(this);
      if (arguments.length < 1) throw new TypeErrorCtor("Failed to execute 'send' on 'WebSocket': 1 argument required");
      if (s.readyState === 0) throw domError('InvalidStateError', "Failed to execute 'send' on 'WebSocket': Still in CONNECTING state.");
      var bytes, binary = false, size;
      if (data instanceof ArrayBuffer) { bytes = data; binary = true; size = data.byteLength; }
      else if (ArrayBuffer.isView(data)) { bytes = data; binary = true; size = data.byteLength; }
      else if (typeof g.Blob === 'function' && data instanceof g.Blob) {
        size = data.size;
        s.bufferedAmount += size;
        if (s.readyState !== 1) return;
        data.arrayBuffer().then(function (buf) { if (s.readyState === 1) N.wsSend(s.id, buf, true); });
        return;
      } else { bytes = usv(data); size = utf8Length(bytes); }
      s.bufferedAmount += size;
      if (s.readyState !== 1) return;
      N.wsSend(s.id, bytes, binary);
    }
  }
  function wsState(o) {
    var s = (typeof o === 'object' && o !== null) ? state.get(o) : undefined;
    if (!s || s.binaryType === undefined) throw new TypeErrorCtor('Illegal invocation');
    return s;
  }
  ['CONNECTING', 'OPEN', 'CLOSING', 'CLOSED'].forEach(function (n, i) {
    defineProperty(WebSocket, n, { value: i, enumerable: true });
    defineProperty(WebSocket.prototype, n, { value: i, enumerable: true });
  });
  handlers(WebSocket.prototype, ['open', 'error', 'close', 'message']);

  // ---- EventSource ---------------------------------------------------
  class EventSource extends g.EventTarget {
    constructor(url) {
      if (new.target === undefined) throw new TypeErrorCtor("Failed to construct 'EventSource': Please use the 'new' operator");
      if (arguments.length < 1) throw new TypeErrorCtor("Failed to construct 'EventSource': 1 argument required");
      var init = arguments[1];
      super();
      var u;
      try { u = parseUrl(usv(url)); } catch (e) { throw domError('SyntaxError', "Failed to construct 'EventSource': Cannot open an EventSource to '" + url + "'. The URL is invalid."); }
      var cred = !!(init && typeof init === 'object' && init.withCredentials);
      var self = this;
      var s = {
        url: u.href, readyState: 0, withCredentials: cred, handlers: {}, wrapped: {}, id: 0,
        on: function (kind, a, b, c) {
          if (s.readyState === 2) return;
          if (kind === 0) {
            s.readyState = 1;
            fire(self, new g.Event('open'));
          } else if (kind === 4) {
            fire(self, new MessageEventC(a, { data: b, origin: u.origin, lastEventId: c }));
          } else if (kind === 5) {
            if (a) {
              s.readyState = 0;
            } else {
              s.readyState = 2;
              live.delete(s.id);
            }
            fire(self, new g.Event('error'));
          }
        }
      };
      state.set(this, s);
      if (u.protocol !== 'http:' && u.protocol !== 'https:') {
        // Not fetchable: fail asynchronously (HTML "fail the connection").
        s.readyState = 0;
        Promise.resolve().then(function () {
          if (s.readyState === 2) return;
          s.readyState = 2;
          fire(self, new g.Event('error'));
        });
        return;
      }
      s.id = N.esOpen(s.url, origin(), cred);
      watch(s.id, this);
    }
    get url() { return esState(this).url; }
    get readyState() { return esState(this).readyState; }
    get withCredentials() { return esState(this).withCredentials; }
    close() {
      var s = esState(this);
      if (s.readyState === 2) return;
      s.readyState = 2;
      if (s.id) {
        N.esClose(s.id);
        live.delete(s.id);
      }
    }
  }
  function esState(o) {
    var s = (typeof o === 'object' && o !== null) ? state.get(o) : undefined;
    if (!s || s.withCredentials === undefined) throw new TypeErrorCtor('Illegal invocation');
    return s;
  }
  ['CONNECTING', 'OPEN', 'CLOSED'].forEach(function (n, i) {
    defineProperty(EventSource, n, { value: i, enumerable: true });
    defineProperty(EventSource.prototype, n, { value: i, enumerable: true });
  });
  handlers(EventSource.prototype, ['open', 'message', 'error']);

  [WebSocket, EventSource].forEach(function (C) {
    var p = C.prototype;
    defineProperty(C, 'prototype', { writable: false, enumerable: false, configurable: false });
    defineProperty(p, Symbol.toStringTag, { value: C.name, configurable: true });
    Object.getOwnPropertyNames(p).forEach(function (k) {
      var d = Object.getOwnPropertyDescriptor(p, k);
      if (k === 'constructor' || /^[A-Z]+$/.test(k) || /^on/.test(k)) return;
      delete p[k];
      d.enumerable = true;
      defineProperty(p, k, d);
    });
    defineProperty(g, C.name, { value: C, writable: true, enumerable: false, configurable: true });
  });
})
