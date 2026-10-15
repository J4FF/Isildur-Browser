// Compression Streams (https://compression.spec.whatwg.org/):
// CompressionStream and DecompressionStream on top of TransformStream.
// Loaded lazily on the first use of either global. `N.compress` and
// `N.decompress` (web_compression.rs) work on whole byte arrays: the glue
// collects the written chunks. Decompression output appears as soon as the
// collected input holds a complete stream (tried each time the input has
// doubled, so the work stays linear), compression output at the end.
(function (N, g) {
  'use strict';
  var defineProperty = Object.defineProperty;
  var TypeErrorCtor = TypeError;
  var U8 = Uint8Array;
  var COMPRESS = ['deflate', 'deflate-raw', 'gzip', 'brotli'];
  var DECOMPRESS = ['deflate', 'deflate-raw', 'gzip', 'brotli'];

  function chunkBytes(chunk) {
    // BufferSource, not shared.
    if (chunk instanceof ArrayBuffer) return new U8(chunk.slice(0));
    if (ArrayBuffer.isView(chunk) && chunk.buffer instanceof ArrayBuffer) {
      return new U8(chunk.buffer.slice(chunk.byteOffset, chunk.byteOffset + chunk.byteLength));
    }
    throw new TypeErrorCtor('The chunk is not a BufferSource');
  }
  function join(list, total) {
    var out = new U8(total), o = 0;
    for (var i = 0; i < list.length; i++) { out.set(list[i], o); o += list[i].length; }
    return out;
  }
  function formatOf(format, allowed, what) {
    var f = String(format);
    if (allowed.indexOf(f) < 0) throw new TypeErrorCtor("Failed to construct '" + what + "': unsupported format '" + f + "'");
    return f;
  }

  var slot = new WeakMap();
  function streamOf(o, what) {
    var s = (typeof o === 'object' && o !== null) ? slot.get(o) : undefined;
    if (!s) throw new TypeErrorCtor('Illegal invocation: not a ' + what);
    return s;
  }

  class CompressionStream {
    constructor(format) {
      if (new.target === undefined) throw new TypeErrorCtor("Failed to construct 'CompressionStream': Please use the 'new' operator");
      if (arguments.length < 1) throw new TypeErrorCtor("Failed to construct 'CompressionStream': 1 argument required");
      var f = formatOf(format, COMPRESS, 'CompressionStream');
      var parts = [], total = 0;
      var ts = new g.TransformStream({
        transform: function (chunk) {
          var b = chunkBytes(chunk);
          parts.push(b);
          total += b.length;
        },
        flush: function (controller) {
          var out = N.compress(f, join(parts, total));
          parts = [];
          controller.enqueue(new U8(out));
        }
      });
      slot.set(this, ts);
    }
    get readable() { return streamOf(this, 'CompressionStream').readable; }
    get writable() { return streamOf(this, 'CompressionStream').writable; }
  }

  class DecompressionStream {
    constructor(format) {
      if (new.target === undefined) throw new TypeErrorCtor("Failed to construct 'DecompressionStream': Please use the 'new' operator");
      if (arguments.length < 1) throw new TypeErrorCtor("Failed to construct 'DecompressionStream': 1 argument required");
      var f = formatOf(format, DECOMPRESS, 'DecompressionStream');
      var parts = [], total = 0, tried = 0, done = false;
      function attempt(controller, last) {
        var r = N.decompress(f, join(parts, total), last);
        tried = total;
        if (r[0] === 1) {
          if (last) throw new TypeErrorCtor('The compressed data was truncated');
          return;
        }
        done = true;
        var out = new U8(r[1]);
        if (out.length) controller.enqueue(out);
        if (r[0] === 2) {
          // The output stays readable; the error follows it.
          var err = new TypeErrorCtor('Trailing data after the end of the compressed stream');
          if (last) throw err;
          Promise.resolve().then(function () { controller.error(err); });
        }
      }
      var ts = new g.TransformStream({
        transform: function (chunk, controller) {
          var b = chunkBytes(chunk);
          if (b.length === 0) return;
          if (done) throw new TypeErrorCtor('Trailing data after the end of the compressed stream');
          parts.push(b);
          total += b.length;
          if (total >= 2 * tried) attempt(controller, false);
        },
        flush: function (controller) {
          if (!done) attempt(controller, true);
        }
      });
      slot.set(this, ts);
    }
    get readable() { return streamOf(this, 'DecompressionStream').readable; }
    get writable() { return streamOf(this, 'DecompressionStream').writable; }
  }

  [CompressionStream, DecompressionStream].forEach(function (C) {
    var p = C.prototype;
    defineProperty(C, 'prototype', { writable: false, enumerable: false, configurable: false });
    defineProperty(p, Symbol.toStringTag, { value: C.name, configurable: true });
    ['readable', 'writable'].forEach(function (name) {
      var get = Object.getOwnPropertyDescriptor(p, name).get;
      delete p[name];
      defineProperty(p, name, { get: get, enumerable: true, configurable: true });
    });
    defineProperty(g, C.name, { value: C, writable: true, enumerable: false, configurable: true });
  });
})
