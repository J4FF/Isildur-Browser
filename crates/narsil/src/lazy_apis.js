// Lazy installation of the messaging APIs (`messaging.js`: Worker,
// SharedWorker, MessageChannel, MessagePort, MessageEvent,
// BroadcastChannel, window.postMessage), of IndexedDB (`indexeddb.js`)
// and of the Cache API (`cache_storage.js`). A page that touches none of them never parses those
// scripts: until first use the names are accessors that load the script
// and then give way to the real definitions. In a worker the messaging
// script runs at once (`worker.rs`); only IndexedDB waits here.
// Annex B legacy accessors (ES2023 B.2.2.2-5): `__defineGetter__` & Co.
// MEASURED: the IndexedDB test helpers (support.js, 173 test files), the
// webmessaging tests and older libraries call them; the engine lacks all
// four. Installed at once (pages and workers), not lazily.
(function () {
  var OP = Object.prototype;
  if (typeof OP.__defineGetter__ === 'function') return;
  function def(name, fn) { Object.defineProperty(OP, name, { value: fn, writable: true, configurable: true, enumerable: false }); }
  function toObj(v) { if (v === null || v === undefined) throw new TypeError('Cannot convert undefined or null to object'); return Object(v); }
  def('__defineGetter__', function __defineGetter__(p, getter) {
    var o = toObj(this);
    if (typeof getter !== 'function') throw new TypeError('Object.prototype.__defineGetter__: Expecting function');
    Object.defineProperty(o, p, { get: getter, enumerable: true, configurable: true });
  });
  def('__defineSetter__', function __defineSetter__(p, setter) {
    var o = toObj(this);
    if (typeof setter !== 'function') throw new TypeError('Object.prototype.__defineSetter__: Expecting function');
    Object.defineProperty(o, p, { set: setter, enumerable: true, configurable: true });
  });
  function lookup(o, p, which) {
    o = toObj(o);
    while (o !== null) {
      var d = Object.getOwnPropertyDescriptor(o, p);
      if (d) return d[which];
      o = Object.getPrototypeOf(o);
    }
    return undefined;
  }
  def('__lookupGetter__', function __lookupGetter__(p) { return lookup(this, p, 'get'); });
  def('__lookupSetter__', function __lookupSetter__(p) { return lookup(this, p, 'set'); });
})();

(function () {
  var g = globalThis;
  var source = g.__narsilLazySource;
  try { delete g.__narsilLazySource; } catch (e) {}
  if (typeof source !== 'function') return;
  var inWorker = typeof g.WorkerGlobalScope === 'function';
  // The engine's natives both scripts take (and then hide): kept here so
  // that the second script still finds them after the first one ran.
  var natives = {};
  ['__narsilSerialize', '__narsilDeserialize', '__narsilSerializeCopy', '__narsilTakeUncaught', '__narsilMicrotaskCheckpoint'].forEach(function (n) {
    natives[n] = g[n];
  });
  // The class name of the platform data objects (`[object Blob]`).
  if (typeof Symbol === 'function' && Symbol.toStringTag) {
    ['Blob', 'File', 'FormData', 'Headers', 'Request', 'Response', 'AbortController', 'AbortSignal'].forEach(function (n) {
      var C = g[n];
      if (typeof C !== 'function' || !C.prototype || Object.prototype.hasOwnProperty.call(C.prototype, Symbol.toStringTag)) return;
      try { Object.defineProperty(C.prototype, Symbol.toStringTag, { value: n, configurable: true }); } catch (e) {}
    });
  }
  var groups = {
    messaging: ['Worker', 'SharedWorker', 'MessageChannel', 'MessagePort', 'MessageEvent', 'BroadcastChannel', '__narsilPostPrep'],
    indexeddb: ['indexedDB', 'IDBFactory', 'IDBDatabase', 'IDBTransaction', 'IDBObjectStore', 'IDBIndex', 'IDBCursor',
      'IDBCursorWithValue', 'IDBKeyRange', 'IDBRequest', 'IDBOpenDBRequest', 'IDBVersionChangeEvent', 'IDBRecord', 'DOMStringList'],
    caches: ['caches', 'CacheStorage', 'Cache']
  };
  // Workers have no page preamble: the Streams Standard and the body
  // mixin of Response/Request (`vorspann.js`, `worker.rs` streams_js) load
  // on first use of a stream class, of `Response`, or of a body method of a
  // Response/Request (fetch() answers in a worker read their bodies so).
  if (inWorker) {
    groups.streams = ['ReadableStream', 'ReadableStreamDefaultReader', 'ReadableStreamBYOBReader',
      'ReadableStreamDefaultController', 'ReadableByteStreamController', 'ReadableStreamBYOBRequest',
      'WritableStream', 'WritableStreamDefaultWriter', 'WritableStreamDefaultController',
      'TransformStream', 'TransformStreamDefaultController', 'ByteLengthQueuingStrategy',
      'CountQueuingStrategy', 'TextEncoderStream', 'TextDecoderStream', 'Response'];
  }
  var protoStubs = [];
  var loaded = {};
  var saved = {};
  function load(which) {
    if (loaded[which]) return;
    loaded[which] = true;
    if (which === 'streams') {
      protoStubs.forEach(function (p) {
        try {
          if (p.desc) Object.defineProperty(p.obj, p.name, p.desc); else delete p.obj[p.name];
        } catch (e) {}
      });
      protoStubs = [];
    }
    groups[which].forEach(function (n) {
      try { delete g[n]; } catch (e) {}
      if (Object.prototype.hasOwnProperty.call(saved, n)) {
        try { Object.defineProperty(g, n, { value: saved[n], configurable: true, writable: true, enumerable: false }); } catch (e) {}
      }
    });
    for (var k in natives) {
      if (typeof natives[k] === 'function') {
        try { Object.defineProperty(g, k, { value: natives[k], configurable: true, writable: true, enumerable: false }); } catch (e) {}
      }
    }
    try {
      (0, eval)(String(source(which)));
    } finally {
      for (var k2 in natives) { try { delete g[k2]; } catch (e) {} }
    }
  }
  function stub(name, which) {
    var d = Object.getOwnPropertyDescriptor(g, name);
    if (d && 'value' in d) saved[name] = d.value;
    try {
      Object.defineProperty(g, name, {
        configurable: true, enumerable: false,
        get: function () { load(which); return g[name]; },
        set: function (v) { load(which); g[name] = v; }
      });
    } catch (e) {}
  }
  if (!inWorker) {
    groups.messaging.forEach(function (n) { stub(n, 'messaging'); });
    // window.postMessage clones and transfers: the first call loads it.
    var oldPost = g.postMessage;
    var wrapper = function postMessage(message) {
      load('messaging');
      var p = g.postMessage;
      return (typeof p === 'function' && p !== wrapper ? p : oldPost).apply(this, arguments);
    };
    try { Object.defineProperty(g, 'postMessage', { value: wrapper, configurable: true, writable: true, enumerable: true }); } catch (e) {}
    // blob: URLs made before the messaging script loads are handed to it
    // then (a worker started later may fetch them); revoked ones are gone.
    var U = g.URL;
    if (U && typeof U.createObjectURL === 'function') {
      var early = new Map();
      var create = U.createObjectURL, revoke = U.revokeObjectURL;
      // Registered with the host at once, too: a module import of the URL
      // (`js.rs`) reads it from there without the messaging script.
      var blobPut = g.__narsilWBlobPut, blobDrop = g.__narsilWBlobDrop, ser = natives.__narsilSerialize;
      var def = function (n, f) { try { Object.defineProperty(U, n, { value: f, configurable: true, writable: true, enumerable: true }); } catch (e) {} };
      def('createObjectURL', function createObjectURL(obj) {
        var u = create.apply(this, arguments);
        if (!loaded.messaging) {
          early.set(u, obj);
          if (typeof blobPut === 'function' && typeof ser === 'function' && typeof g.Blob === 'function' && obj instanceof g.Blob) {
            try { blobPut(u, ser(obj, [], [], [])); } catch (e) {}
          }
        }
        return u;
      });
      def('revokeObjectURL', function revokeObjectURL(u) {
        if (early.delete(String(u)) && typeof blobDrop === 'function') { try { blobDrop(String(u)); } catch (e) {} }
        return revoke.apply(this, arguments);
      });
      try {
        Object.defineProperty(g, '__narsilEarlyBlobs', {
          value: function () { var out = []; early.forEach(function (b, u) { out.push([u, b]); }); early.clear(); return out; },
          configurable: true, writable: true, enumerable: false
        });
      } catch (e) {}
    }
  }
  groups.indexeddb.forEach(function (n) { stub(n, 'indexeddb'); });
  groups.caches.forEach(function (n) { stub(n, 'caches'); });
  if (groups.streams && typeof source('streams') === 'string' && String(source('streams')).length) {
    groups.streams.forEach(function (n) { stub(n, 'streams'); });
    var protoStub = function (obj, name, accessor) {
      if (!obj) return;
      var desc = Object.getOwnPropertyDescriptor(obj, name);
      protoStubs.push({ obj: obj, name: name, desc: desc });
      try {
        if (accessor) {
          Object.defineProperty(obj, name, { configurable: true, enumerable: true, get: function () { load('streams'); return this[name]; } });
        } else {
          Object.defineProperty(obj, name, {
            configurable: true, enumerable: true, writable: true,
            value: function () { load('streams'); return this[name].apply(this, arguments); }
          });
        }
      } catch (e) {}
    };
    [saved.Response || g.Response, g.Request].forEach(function (C) {
      if (typeof C !== 'function' || !C.prototype) return;
      ['arrayBuffer', 'blob', 'bytes', 'formData', 'clone'].forEach(function (n) {
        if (typeof C.prototype[n] !== 'function') protoStub(C.prototype, n, false);
      });
      ['body', 'bodyUsed'].forEach(function (n) { if (!(n in C.prototype)) protoStub(C.prototype, n, true); });
    });
    if (typeof g.Blob === 'function' && g.Blob.prototype && typeof g.Blob.prototype.stream !== 'function') protoStub(g.Blob.prototype, 'stream', false);
  }
  // These names are built-ins, not page globals: frame scripts see them
  // (`abschirmen` in the preamble hides what is not in this list).
  var base = g.__narsilGrundNamen;
  if (base && typeof base.push === 'function') {
    for (var gname in groups) groups[gname].forEach(function (n) { if (base.indexOf(n) < 0) base.push(n); });
  }
  // Script that runs later finds the natives here.
  try { Object.defineProperty(g, '__narsilLazyLoad', { value: load, configurable: true, writable: true, enumerable: false }); } catch (e) {}
  for (var n in natives) { try { delete g[n]; } catch (e) {} }
  // Only the worker scope script uses it (importScripts).
  try { delete g.__narsilRunScript; } catch (e) {}
})();
