// The Cache API (Service Workers §5.4 Cache, §5.5 CacheStorage): `caches`
// in pages and workers. The storage is `cache_storage.rs`
// (`__narsilCache`), shared by all contexts of an origin; bodies travel as
// Blobs (`__narsilSerialize`). Installed on first use (`lazy_apis.js`).
(function () {
  var g = globalThis;
  var host = g.__narsilCache;
  try { delete g.__narsilCache; } catch (e) {}
  var serialize = g.__narsilSerialize, deserialize = g.__narsilDeserialize;
  if (typeof host !== 'function' || typeof serialize !== 'function') return;
  var DE = g.DOMException;

  function call() {
    try { return host.apply(null, arguments); } catch (e) {
      if (e instanceof DE) throw e;
      var m = String(e && e.message !== undefined ? e.message : e).replace(/^(Laufzeitfehler: )?(Error: )?/, '');
      var i = m.indexOf(': ');
      var name = i > 0 ? m.slice(0, i) : 'UnknownError';
      throw new DE(i > 0 ? m.slice(i + 2) : m, /^[A-Z][A-Za-z]+Error$/.test(name) ? name : 'UnknownError');
    }
  }
  function iface(name, ctor) {
    var C = ctor || function () { throw new TypeError('Illegal constructor'); };
    try { Object.defineProperty(C, 'name', { value: name, configurable: true }); } catch (e) {}
    if (typeof Symbol === 'function' && Symbol.toStringTag) {
      try { Object.defineProperty(C.prototype, Symbol.toStringTag, { value: name, configurable: true }); } catch (e) {}
    }
    try { Object.defineProperty(g, name, { value: C, configurable: true, writable: true, enumerable: false }); } catch (e) { g[name] = C; }
    return C;
  }
  function method(proto, name, fn) {
    try { Object.defineProperty(proto, name, { value: fn, configurable: true, writable: true, enumerable: true }); } catch (e) { proto[name] = fn; }
  }
  function headerList(h) {
    var out = [];
    if (!h) return out;
    try {
      if (typeof h.forEach === 'function') { h.forEach(function (v, k) { out.push([String(k), String(v)]); }); return out; }
    } catch (e) {}
    try {
      if (typeof h.entries === 'function') { for (var p of h.entries()) out.push([String(p[0]), String(p[1])]); return out; }
    } catch (e) {}
    if (typeof h === 'object') for (var k in h) out.push([k, String(h[k])]);
    return out;
  }
  function toRequest(input) {
    if (g.Request && input instanceof g.Request) return input;
    var url;
    try { url = new g.URL(String(input), String(g.location.href)).href; } catch (e) {
      throw new TypeError("Failed to execute on 'Cache': Invalid URL '" + String(input) + "'.");
    }
    return { url: url, method: 'GET', headers: null };
  }
  function reqParts(r) {
    return { url: String(r.url), method: String(r.method || 'GET').toUpperCase(), headers: headerList(r.headers) };
  }
  function opts(o) {
    o = o || {};
    return { ignoreSearch: !!o.ignoreSearch, ignoreMethod: !!o.ignoreMethod, ignoreVary: !!o.ignoreVary, cacheName: o.cacheName };
  }
  // A Response for a stored pair; the methods read its Blob.
  function responseFrom(row) {
    var blob = deserialize(row[3], []);
    var R = g.Response;
    var r = Object.create(R && R.prototype ? R.prototype : Object.prototype);
    var headers = typeof g.Headers === 'function' ? new g.Headers() : {};
    try { row[2].forEach(function (p) { headers.append(p[0], p[1]); }); } catch (e) {}
    var used = false;
    function once(f) {
      return function () {
        if (used) return Promise.reject(new TypeError('Body has already been consumed.'));
        used = true;
        return f();
      };
    }
    var status = Number(row[0]);
    var fields = {
      ok: status >= 200 && status < 300, status: status, statusText: String(row[1]), url: String(row[4]),
      type: String(row[5] || 'basic'), redirected: false, headers: headers,
      text: once(function () { return blob.text(); }),
      arrayBuffer: once(function () { return blob.arrayBuffer(); }),
      blob: once(function () { return Promise.resolve(blob); }),
      json: once(function () { return blob.text().then(function (s) { return JSON.parse(s); }); }),
      clone: function () { return responseFrom(row); }
    };
    Object.keys(fields).forEach(function (k) { try { Object.defineProperty(r, k, { value: fields[k], configurable: true, enumerable: true }); } catch (e) {} });
    try { Object.defineProperty(r, 'bodyUsed', { get: function () { return used; }, configurable: true }); } catch (e) {}
    return r;
  }
  function bodyHandle(response) {
    var get = typeof response.arrayBuffer === 'function' ? response.arrayBuffer() : (typeof response.text === 'function' ? response.text() : Promise.resolve(''));
    return Promise.resolve(get).then(function (body) {
      var blob = new g.Blob([body === undefined || body === null ? '' : body]);
      return serialize(blob, [], [], []);
    });
  }

  var CS = new WeakMap();
  var Cache = iface('Cache');
  // A Cache object keeps working after CacheStorage.delete() of its name
  // (the cache is doomed, not gone): objects of an older generation of a
  // name use the key the host moved that cache to.
  var GEN = Object.create(null), MOVED = Object.create(null);
  function cacheName(c) {
    var st = CS.get(c);
    if (st === undefined) throw new TypeError('Illegal invocation');
    if ((GEN[st.name] | 0) === st.gen) return st.name;
    return MOVED[st.name + '\u0000' + st.gen] || '\u0000gone';
  }
  function cacheObject(name) { var c = Object.create(Cache.prototype); CS.set(c, { name: name, gen: GEN[name] | 0 }); return c; }
  function need(args, n, op, iface) {
    if (args.length < n) throw new TypeError("Failed to execute '" + op + "' on '" + iface + "': " + n + ' argument' + (n > 1 ? 's' : '') + ' required, but only ' + args.length + ' present.');
  }
  method(Cache.prototype, 'match', function match(request, options) {
    var name = cacheName(this), args = arguments;
    return new Promise(function (resolve) {
      need(args, 1, 'match', 'Cache');
      var r = reqParts(toRequest(request)), o = opts(options);
      var rows = call('match', name, r.url, r.method, r.headers, o.ignoreSearch, o.ignoreMethod, o.ignoreVary, false);
      resolve(rows.length ? responseFrom(rows[0]) : undefined);
    });
  });
  method(Cache.prototype, 'matchAll', function matchAll(request, options) {
    var name = cacheName(this);
    return new Promise(function (resolve) {
      var o = opts(options), r = request === undefined ? { url: '', method: 'GET', headers: [] } : reqParts(toRequest(request));
      var rows = call('match', name, r.url, r.method, r.headers, o.ignoreSearch, o.ignoreMethod, o.ignoreVary, true);
      resolve(rows.map(responseFrom));
    });
  });
  method(Cache.prototype, 'put', function put(request, response) {
    var name = cacheName(this), args = arguments;
    return new Promise(function (resolve, reject) {
      need(args, 2, 'put', 'Cache');
      var req = toRequest(request), r = reqParts(req);
      if (!/^https?:/i.test(r.url)) throw new TypeError("Failed to execute 'put' on 'Cache': Request scheme '" + r.url.split(':')[0] + "' is unsupported");
      if (r.method !== 'GET') throw new TypeError("Failed to execute 'put' on 'Cache': Request method '" + r.method + "' is unsupported");
      if (!response || typeof response.status !== 'number') throw new TypeError("Failed to execute 'put' on 'Cache': parameter 2 is not of type 'Response'.");
      if (response.status === 206) throw new TypeError("Failed to execute 'put' on 'Cache': Partial response (status code 206) is unsupported");
      var headers = headerList(response.headers);
      for (var i = 0; i < headers.length; i++) {
        if (headers[i][0].toLowerCase() === 'vary' && headers[i][1].split(',').some(function (f) { return f.trim() === '*'; })) {
          throw new TypeError("Failed to execute 'put' on 'Cache': Vary header contains *");
        }
      }
      if (response.bodyUsed) throw new TypeError("Failed to execute 'put' on 'Cache': Response body is already used");
      bodyHandle(response).then(function (h) {
        call('put', name, r.url, r.method, r.headers, response.status, String(response.statusText || ''), headers, h, String(response.url || r.url), String(response.type || 'basic'));
        resolve(undefined);
      }).catch(reject);
    });
  });
  method(Cache.prototype, 'add', function add(request) {
    var self = this;
    return this.addAll([request]);
  });
  method(Cache.prototype, 'addAll', function addAll(requests) {
    var self = this;
    var list = [];
    for (var x of requests) list.push(toRequest(x));
    return Promise.all(list.map(function (req) {
      var r = reqParts(req);
      if (!/^https?:/i.test(r.url) || r.method !== 'GET') return Promise.reject(new TypeError("Failed to execute 'addAll' on 'Cache': Request is not a GET request with an http(s) URL."));
      return g.fetch(r.url).then(function (resp) {
        if (!resp.ok) throw new TypeError("Failed to execute 'addAll' on 'Cache': Request failed");
        return [req, resp];
      });
    })).then(function (pairs) {
      return Promise.all(pairs.map(function (p) { return self.put(p[0], p[1]); }));
    }).then(function () { return undefined; });
  });
  method(Cache.prototype, 'delete', function del(request, options) {
    var name = cacheName(this), args = arguments;
    return new Promise(function (resolve) {
      need(args, 1, 'delete', 'Cache');
      var r = reqParts(toRequest(request)), o = opts(options);
      resolve(call('remove', name, r.url, r.method, r.headers, o.ignoreSearch, o.ignoreMethod, o.ignoreVary));
    });
  });
  method(Cache.prototype, 'keys', function keys(request, options) {
    var name = cacheName(this);
    return new Promise(function (resolve) {
      var o = opts(options), r = request === undefined ? { url: '', method: 'GET', headers: [] } : reqParts(toRequest(request));
      var rows = call('requests', name, r.url, r.method, r.headers, o.ignoreSearch, o.ignoreMethod, o.ignoreVary);
      resolve(rows.map(function (row) {
        try { return new g.Request(row[0], { method: row[1] }); } catch (e) { return { url: row[0], method: row[1] }; }
      }));
    });
  });

  var CacheStorage = iface('CacheStorage');
  method(CacheStorage.prototype, 'open', function open(name) {
    var args = arguments;
    return new Promise(function (resolve) { need(args, 1, 'open', 'CacheStorage'); var n = String(name); call('open', n); resolve(cacheObject(n)); });
  });
  method(CacheStorage.prototype, 'has', function has(name) {
    var args = arguments;
    return new Promise(function (resolve) { need(args, 1, 'has', 'CacheStorage'); resolve(call('has', String(name))); });
  });
  method(CacheStorage.prototype, 'delete', function del(name) {
    var args = arguments;
    return new Promise(function (resolve) {
      need(args, 1, 'delete', 'CacheStorage');
      var n = String(name);
      var key = call('delete', n);
      if (typeof key !== 'string') { resolve(false); return; }
      MOVED[n + '\u0000' + (GEN[n] | 0)] = key;
      GEN[n] = (GEN[n] | 0) + 1;
      resolve(true);
    });
  });
  method(CacheStorage.prototype, 'keys', function keys() {
    return new Promise(function (resolve) { resolve(call('keys')); });
  });
  method(CacheStorage.prototype, 'match', function match(request, options) {
    var args = arguments;
    return new Promise(function (resolve) {
      need(args, 1, 'match', 'CacheStorage');
      var r = reqParts(toRequest(request)), o = opts(options);
      var name = o.cacheName === undefined ? null : String(o.cacheName);
      if (name !== null && !call('has', name)) { resolve(undefined); return; }
      var rows = call('match', name, r.url, r.method, r.headers, o.ignoreSearch, o.ignoreMethod, o.ignoreVary, false);
      resolve(rows.length ? responseFrom(rows[0]) : undefined);
    });
  });
  var storage = Object.create(CacheStorage.prototype);
  Object.defineProperty(g, 'caches', { get: function () { return storage; }, configurable: true, enumerable: true });
})();
