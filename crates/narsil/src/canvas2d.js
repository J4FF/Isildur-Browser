// Release 15.10.2026: the Canvas 2D API (HTML §4.12.5), OffscreenCanvas,
// ImageData, Path2D, ImageBitmap and the geometry interfaces DOMMatrix /
// DOMPoint. Loaded on first use (see the loader at the end of vorspann.js).
//
// The script side is a thin WebIDL layer: it converts arguments, raises the
// exceptions the spec requires and keeps object identity. All drawing state
// and all pixels live in Rust (`crate::canvas`), reached through one host
// function `__narsilC2d(op, id, ...)`. The op numbers must match
// `crate::canvas::op`.
(function () {
  'use strict';
  var g = globalThis;
  var N = g.__narsilC2d;
  var GET_CONTEXT = 1, CREATE_OFFSCREEN = 2, RESIZE = 3, TO_DATA_URL = 5, CONTEXT_ATTRS = 6, SNAPSHOT = 7,
      SAVE = 10, RESTORE = 11, RESET = 12, GET = 13, SET = 14, SET_LINE_DASH = 15, GET_LINE_DASH = 16,
      SET_GRADIENT = 17, SET_PATTERN = 18, PARSE_COLOR = 19,
      TRANSLATE = 20, SCALE = 21, ROTATE = 22, TRANSFORM = 23, SET_TRANSFORM = 24, RESET_TRANSFORM = 25, GET_TRANSFORM = 26,
      BEGIN_PATH = 30, MOVE_TO = 31, LINE_TO = 32, QUAD_TO = 33, CUBIC_TO = 34, ARC_TO = 35, ARC = 36, ELLIPSE = 37,
      RECT = 38, ROUND_RECT = 39, CLOSE_PATH = 40, FILL = 41, STROKE = 42, CLIP = 43, IN_PATH = 44, IN_STROKE = 45,
      FILL_RECT = 46, STROKE_RECT = 47, CLEAR_RECT = 48, DRAW_IMAGE = 50, GET_IMAGE_DATA = 51, PUT_IMAGE_DATA = 52,
      SOURCE_INFO = 53, UPLOAD_BITMAP = 54, ENCODE = 8, DECODE = 9, PARSE_SVG_PATH = 60,
      FILL_TEXT = 70, STROKE_TEXT = 71, MEASURE_TEXT = 72, BEGIN_LAYER = 61, END_LAYER = 62, SET_FILTER_SPEC = 63,
      LINK_PLACEHOLDER = 64;
  // Path2D command numbers (narsil_canvas::commands).
  var P_MOVE = 0, P_LINE = 1, P_QUAD = 2, P_CUBIC = 3, P_ARC_TO = 4, P_ELLIPSE = 5, P_RECT = 6, P_ROUND_RECT = 7,
      P_CLOSE = 8, P_BEGIN_ADD = 9, P_END_ADD = 10;

  var tokens = 0;
  var SPACES = ['srgb', 'srgb-linear', 'display-p3', 'display-p3-linear'];
  function hidden(o, k, v) { Object.defineProperty(o, k, { value: v, writable: true, configurable: true }); }
  function domErr(name, msg) { return new g.DOMException(msg, name); }
  function tooFew(iface, method, need, got) {
    return new TypeError("Failed to execute '" + method + "' on '" + iface + "': " + need + ' argument' + (need > 1 ? 's' : '') + ' required, but only ' + got + ' present.');
  }
  function illegal() { return new TypeError('Illegal invocation'); }
  function ctor(name, fn) {
    try { Object.defineProperty(fn, 'name', { value: name }); } catch (e) {}
    return fn;
  }
  function illegalCtor(name) { return ctor(name, function () { throw new TypeError('Illegal constructor'); }); }
  // WebIDL interface objects: `prototype` is neither writable nor
  // configurable (assigning `null` is ignored).
  function lockPrototype(C) {
    try { Object.defineProperty(C, 'prototype', { writable: false, enumerable: false, configurable: false }); } catch (e) {}
  }
  function tag(P, name) {
    try { Object.defineProperty(P, Symbol.toStringTag, { value: name, configurable: true }); } catch (e) {}
  }
  function expose(name, C) {
    Object.defineProperty(g, name, { value: C, writable: true, configurable: true, enumerable: false });
  }
  function method(P, name, f) {
    Object.defineProperty(P, name, { value: f, writable: true, configurable: true, enumerable: true });
  }
  function accessor(P, name, get, set) {
    var d = { get: get, configurable: true, enumerable: true };
    if (set) d.set = set;
    Object.defineProperty(P, name, d);
  }
  // WebIDL conversions.
  function str(v) {
    if (typeof v === 'symbol') throw new TypeError('Cannot convert a Symbol value to a string');
    return String(v);
  }
  function dbl(v, what) {
    v = +v;
    if (v !== v || v === Infinity || v === -Infinity) throw new TypeError("The provided double value" + (what ? ' for ' + what : '') + ' is non-finite.');
    return v;
  }
  function enforce(v, lo, hi) {
    v = +v;
    if (v !== v || v === Infinity || v === -Infinity) throw new TypeError('Value is not of type long (non-finite).');
    v = v < 0 ? Math.ceil(v) : Math.floor(v);
    if (v < lo || v > hi) throw new TypeError('Value is outside the long value range.');
    return v;
  }
  function enforceLong(v) { return enforce(v, -2147483648, 2147483647); }
  function enforceULong(v) { return enforce(v, 0, 4294967295); }
  function enforceULongLong(v) { return enforce(v, 0, 9007199254740991); }
  function fillRule(v, method) {
    if (v === undefined) return 'nonzero';
    v = str(v);
    if (v !== 'nonzero' && v !== 'evenodd') throw new TypeError("Failed to execute '" + method + "': The provided value '" + v + "' is not a valid enum value of type CanvasFillRule.");
    return v;
  }
  // A dictionary member, read once: `def` when undefined, else `conv(v)`.
  function opt(o, k, def, conv) {
    var v = o[k];
    return v === undefined ? def : conv(v);
  }
  function dictionary(v, what) {
    if (v === undefined || v === null) return {};
    if (typeof v !== 'object' && typeof v !== 'function') throw new TypeError("The provided value is not of type '" + what + "'.");
    return v;
  }

  // ---- DOMPoint / DOMMatrix (Geometry Interfaces Module Level 1) ---------
  function DOMPointReadOnly(x, y, z, w) {
    if (!new.target) throw new TypeError("Failed to construct 'DOMPointReadOnly': Please use the 'new' operator.");
    hidden(this, '__pt', [x === undefined ? 0 : +x, y === undefined ? 0 : +y, z === undefined ? 0 : +z, w === undefined ? 1 : +w]);
  }
  ctor('DOMPointReadOnly', DOMPointReadOnly);
  function DOMPoint(x, y, z, w) {
    if (!new.target) throw new TypeError("Failed to construct 'DOMPoint': Please use the 'new' operator.");
    hidden(this, '__pt', [x === undefined ? 0 : +x, y === undefined ? 0 : +y, z === undefined ? 0 : +z, w === undefined ? 1 : +w]);
  }
  ctor('DOMPoint', DOMPoint);
  DOMPoint.prototype = Object.create(DOMPointReadOnly.prototype);
  Object.defineProperty(DOMPoint.prototype, 'constructor', { value: DOMPoint, writable: true, configurable: true });
  ['x', 'y', 'z', 'w'].forEach(function (k, i) {
    accessor(DOMPointReadOnly.prototype, k, function () { if (!this.__pt) throw illegal(); return this.__pt[i]; });
    accessor(DOMPoint.prototype, k, function () { if (!this.__pt) throw illegal(); return this.__pt[i]; }, function (v) { if (!this.__pt) throw illegal(); this.__pt[i] = +v; });
  });
  function pointFrom(C, o) {
    o = dictionary(o, 'DOMPointInit');
    return new C(o.x, o.y, o.z, o.w);
  }
  DOMPointReadOnly.fromPoint = function fromPoint(o) { return pointFrom(DOMPointReadOnly, o); };
  DOMPoint.fromPoint = function fromPoint(o) { return pointFrom(DOMPoint, o); };
  method(DOMPointReadOnly.prototype, 'toJSON', function toJSON() { var p = this.__pt; return { x: p[0], y: p[1], z: p[2], w: p[3] }; });
  method(DOMPointReadOnly.prototype, 'matrixTransform', function matrixTransform(m) {
    var mm = matrixFromInit(m, false);
    var p = this.__pt;
    var r = mulPoint(mm.__m, p[0], p[1], p[2], p[3]);
    return new DOMPoint(r[0], r[1], r[2], r[3]);
  });
  tag(DOMPointReadOnly.prototype, 'DOMPointReadOnly');
  tag(DOMPoint.prototype, 'DOMPoint');

  // Column-major 4x4: m[0]=m11 m[1]=m12 m[2]=m13 m[3]=m14 m[4]=m21 ... m[12]=m41 m[13]=m42 ...
  var NAMES = ['m11', 'm12', 'm13', 'm14', 'm21', 'm22', 'm23', 'm24', 'm31', 'm32', 'm33', 'm34', 'm41', 'm42', 'm43', 'm44'];
  function ident() { return [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]; }
  function mul(a, b) {
    // a * b (apply b first).
    var r = new Array(16);
    for (var i = 0; i < 4; i++) {
      for (var j = 0; j < 4; j++) {
        var s = 0;
        for (var k = 0; k < 4; k++) s += a[k * 4 + j] * b[i * 4 + k];
        r[i * 4 + j] = s;
      }
    }
    return r;
  }
  function mulPoint(m, x, y, z, w) {
    return [m[0] * x + m[4] * y + m[8] * z + m[12] * w, m[1] * x + m[5] * y + m[9] * z + m[13] * w,
      m[2] * x + m[6] * y + m[10] * z + m[14] * w, m[3] * x + m[7] * y + m[11] * z + m[15] * w];
  }
  function makeMatrix(C, m, is2D) {
    var o = Object.create(C.prototype);
    hidden(o, '__m', m);
    hidden(o, '__2d', is2D);
    return o;
  }
  function parseTransformList(s) {
    // CSS <transform-list> for the DOMMatrix(string) constructor.
    s = str(s).trim();
    var m = ident(), two = true;
    if (s === '' || s === 'none') return { m: m, is2D: true };
    var re = /([a-zA-Z0-9]+)\(([^)]*)\)/g, last = 0, hit;
    function num(t, unitKind) {
      t = t.trim();
      var mm = /^([-+]?(?:\d+\.?\d*|\.\d+)(?:e[-+]?\d+)?)([a-z%]*)$/i.exec(t);
      if (!mm) throw new SyntaxError('bad number');
      var v = +mm[1], u = mm[2].toLowerCase();
      if (unitKind === 'angle') {
        if (u === 'deg' || (u === '' && v === 0)) return v;
        if (u === 'rad') return v * 180 / Math.PI;
        if (u === 'grad') return v * 0.9;
        if (u === 'turn') return v * 360;
        throw new SyntaxError('bad angle');
      }
      if (unitKind === 'length') {
        if (u === 'px' || (u === '' && v === 0)) return v;
        throw new SyntaxError('bad length');
      }
      if (u !== '') throw new SyntaxError('bad number');
      return v;
    }
    while ((hit = re.exec(s))) {
      if (s.slice(last, hit.index).trim() !== '') throw domErr('SyntaxError', 'Failed to parse transform list');
      last = re.lastIndex;
      var name = hit[1].toLowerCase(), args = hit[2].split(',');
      var t = ident();
      try {
        if (name === 'matrix' && args.length === 6) { var a = args.map(function (x) { return num(x); }); t = [a[0], a[1], 0, 0, a[2], a[3], 0, 0, 0, 0, 1, 0, a[4], a[5], 0, 1]; }
        else if (name === 'matrix3d' && args.length === 16) { t = args.map(function (x) { return num(x); }); two = false; }
        else if (name === 'translate' && args.length <= 2) { t[12] = num(args[0], 'length'); t[13] = args.length > 1 ? num(args[1], 'length') : 0; }
        else if (name === 'translatex' && args.length === 1) t[12] = num(args[0], 'length');
        else if (name === 'translatey' && args.length === 1) t[13] = num(args[0], 'length');
        else if (name === 'scale' && args.length <= 2) { t[0] = num(args[0]); t[5] = args.length > 1 ? num(args[1]) : t[0]; }
        else if (name === 'scalex' && args.length === 1) t[0] = num(args[0]);
        else if (name === 'scaley' && args.length === 1) t[5] = num(args[0]);
        else if ((name === 'rotate' || name === 'rotatez') && args.length === 1) { var r = num(args[0], 'angle') * Math.PI / 180; t[0] = Math.cos(r); t[1] = Math.sin(r); t[4] = -Math.sin(r); t[5] = Math.cos(r); }
        else if (name === 'skewx' && args.length === 1) t[4] = Math.tan(num(args[0], 'angle') * Math.PI / 180);
        else if (name === 'skewy' && args.length === 1) t[1] = Math.tan(num(args[0], 'angle') * Math.PI / 180);
        else throw new SyntaxError('unknown');
      } catch (e) {
        throw domErr('SyntaxError', "Failed to execute 'DOMMatrix': '" + s + "' is not a valid transform list.");
      }
      m = mul(m, t);
    }
    if (s.slice(last).trim() !== '' || last === 0) throw domErr('SyntaxError', "Failed to parse '" + s + "'.");
    return { m: m, is2D: two };
  }
  function matrixInit(C, init) {
    if (init === undefined) return makeMatrix(C, ident(), true);
    if (typeof init === 'string') {
      if (typeof g.document === 'undefined') throw new TypeError('DOMMatrix strings need a document');
      var p = parseTransformList(init);
      return makeMatrix(C, p.m, p.is2D);
    }
    if (init && typeof init === 'object' && typeof init.length === 'number') {
      var a = Array.prototype.map.call(init, function (x) { return +x; });
      if (a.length === 6) return makeMatrix(C, [a[0], a[1], 0, 0, a[2], a[3], 0, 0, 0, 0, 1, 0, a[4], a[5], 0, 1], true);
      if (a.length === 16) return makeMatrix(C, a, false);
      throw new TypeError("Failed to construct 'DOMMatrix': The sequence must contain 6 elements for a 2D matrix or 16 elements for a 3D matrix.");
    }
    throw new TypeError("Failed to construct 'DOMMatrix': The provided value is not of type '(DOMString or sequence<unrestricted double>)'.");
  }
  function same(a, b) { return a === b || (a !== a && b !== b); }
  // "validate and fixup" for DOMMatrixInit / DOMMatrix2DInit.
  function fixup(o, only2D) {
    o = dictionary(o, only2D ? 'DOMMatrix2DInit' : 'DOMMatrixInit');
    var pairs = [['a', 'm11', 1], ['b', 'm12', 0], ['c', 'm21', 0], ['d', 'm22', 1], ['e', 'm41', 0], ['f', 'm42', 0]];
    var v = {};
    for (var i = 0; i < pairs.length; i++) {
      var p = pairs[i];
      var short = o[p[0]], long = o[p[1]];
      if (short !== undefined) short = +short;
      if (long !== undefined) long = +long;
      if (short !== undefined && long !== undefined && !same(short, long)) throw new TypeError("Failed to execute: The '" + p[0] + "' property should equal '" + p[1] + "'.");
      v[p[1]] = long !== undefined ? long : (short !== undefined ? short : p[2]);
    }
    if (only2D) return { m: [v.m11, v.m12, 0, 0, v.m21, v.m22, 0, 0, 0, 0, 1, 0, v.m41, v.m42, 0, 1], is2D: true };
    var rest = ['m13', 'm14', 'm23', 'm24', 'm31', 'm32', 'm33', 'm34', 'm43', 'm44'];
    var def = { m33: 1, m44: 1 };
    var any3 = false;
    for (var j = 0; j < rest.length; j++) {
      var x = o[rest[j]];
      v[rest[j]] = x === undefined ? (def[rest[j]] || 0) : +x;
      if (x !== undefined && !((rest[j] === 'm33' || rest[j] === 'm44') ? +x === 1 : +x === 0)) any3 = true;
    }
    var is2D = o.is2D;
    if (is2D === true && any3) throw new TypeError("Failed to execute: The 'is2D' property is set to true but the input matrix is a 3D matrix.");
    if (is2D === undefined) is2D = !any3;
    return { m: [v.m11, v.m12, v.m13, v.m14, v.m21, v.m22, v.m23, v.m24, v.m31, v.m32, v.m33, v.m34, v.m41, v.m42, v.m43, v.m44], is2D: !!is2D };
  }
  function matrixFromInit(o, only2D, C) {
    if (o instanceof DOMMatrixReadOnly) return makeMatrix(C || DOMMatrix, o.__m.slice(), o.__2d);
    var f = fixup(o, only2D);
    return makeMatrix(C || DOMMatrix, f.m, f.is2D);
  }
  function DOMMatrixReadOnly(init) {
    if (!new.target) throw new TypeError("Failed to construct 'DOMMatrixReadOnly': Please use the 'new' operator.");
    var m = matrixInit(DOMMatrixReadOnly, init);
    hidden(this, '__m', m.__m);
    hidden(this, '__2d', m.__2d);
  }
  ctor('DOMMatrixReadOnly', DOMMatrixReadOnly);
  function DOMMatrix(init) {
    if (!new.target) throw new TypeError("Failed to construct 'DOMMatrix': Please use the 'new' operator.");
    var m = matrixInit(DOMMatrix, init);
    hidden(this, '__m', m.__m);
    hidden(this, '__2d', m.__2d);
  }
  ctor('DOMMatrix', DOMMatrix);
  DOMMatrix.prototype = Object.create(DOMMatrixReadOnly.prototype);
  Object.defineProperty(DOMMatrix.prototype, 'constructor', { value: DOMMatrix, writable: true, configurable: true });
  var RO = DOMMatrixReadOnly.prototype, RW = DOMMatrix.prototype;
  NAMES.forEach(function (n, i) {
    accessor(RO, n, function () { return this.__m[i]; });
    accessor(RW, n, function () { return this.__m[i]; }, function (v) {
      this.__m[i] = +v;
      if (!(i === 0 || i === 1 || i === 4 || i === 5 || i === 12 || i === 13) && !((i === 10 || i === 15) ? +v === 1 : +v === 0)) this.__2d = false;
    });
  });
  [['a', 0], ['b', 1], ['c', 4], ['d', 5], ['e', 12], ['f', 13]].forEach(function (p) {
    accessor(RO, p[0], function () { return this.__m[p[1]]; });
    accessor(RW, p[0], function () { return this.__m[p[1]]; }, function (v) { this.__m[p[1]] = +v; });
  });
  accessor(RO, 'is2D', function () { return this.__2d; });
  accessor(RO, 'isIdentity', function () {
    var m = this.__m, id = ident();
    for (var i = 0; i < 16; i++) if (m[i] !== id[i]) return false;
    return true;
  });
  function invert(m) {
    var inv = new Array(16), det;
    inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15] + m[9] * m[7] * m[14] + m[13] * m[6] * m[11] - m[13] * m[7] * m[10];
    inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15] - m[8] * m[7] * m[14] - m[12] * m[6] * m[11] + m[12] * m[7] * m[10];
    inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15] + m[8] * m[7] * m[13] + m[12] * m[5] * m[11] - m[12] * m[7] * m[9];
    inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14] - m[8] * m[6] * m[13] - m[12] * m[5] * m[10] + m[12] * m[6] * m[9];
    inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15] - m[9] * m[3] * m[14] - m[13] * m[2] * m[11] + m[13] * m[3] * m[10];
    inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15] + m[8] * m[3] * m[14] + m[12] * m[2] * m[11] - m[12] * m[3] * m[10];
    inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15] - m[8] * m[3] * m[13] - m[12] * m[1] * m[11] + m[12] * m[3] * m[9];
    inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14] + m[8] * m[2] * m[13] + m[12] * m[1] * m[10] - m[12] * m[2] * m[9];
    inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15] + m[5] * m[3] * m[14] + m[13] * m[2] * m[7] - m[13] * m[3] * m[6];
    inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15] - m[4] * m[3] * m[14] - m[12] * m[2] * m[7] + m[12] * m[3] * m[6];
    inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15] + m[4] * m[3] * m[13] + m[12] * m[1] * m[7] - m[12] * m[3] * m[5];
    inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14] - m[4] * m[2] * m[13] - m[12] * m[1] * m[6] + m[12] * m[2] * m[5];
    inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11] - m[5] * m[3] * m[10] - m[9] * m[2] * m[7] + m[9] * m[3] * m[6];
    inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11] + m[4] * m[3] * m[10] + m[8] * m[2] * m[7] - m[8] * m[3] * m[6];
    inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11] - m[4] * m[3] * m[9] - m[8] * m[1] * m[7] + m[8] * m[3] * m[5];
    inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10] + m[4] * m[2] * m[9] + m[8] * m[1] * m[6] - m[8] * m[2] * m[5];
    det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
    if (det === 0 || det !== det) return null;
    for (var i = 0; i < 16; i++) inv[i] /= det;
    return inv;
  }
  function selfOp(name, f) {
    method(RW, name, f);
    method(RO, name.replace('Self', ''), function () {
      var c = makeMatrix(DOMMatrix, this.__m.slice(), this.__2d);
      return f.apply(c, arguments);
    });
  }
  selfOp('multiplySelf', function multiplySelf(other) {
    var o = matrixFromInit(other, false);
    this.__m = mul(this.__m, o.__m);
    if (!o.__2d) this.__2d = false;
    return this;
  });
  method(RW, 'preMultiplySelf', function preMultiplySelf(other) {
    var o = matrixFromInit(other, false);
    this.__m = mul(o.__m, this.__m);
    if (!o.__2d) this.__2d = false;
    return this;
  });
  selfOp('translateSelf', function translateSelf(tx, ty, tz) {
    var t = ident();
    t[12] = tx === undefined ? 0 : +tx; t[13] = ty === undefined ? 0 : +ty; t[14] = tz === undefined ? 0 : +tz;
    this.__m = mul(this.__m, t);
    if (t[14] !== 0) this.__2d = false;
    return this;
  });
  selfOp('scaleSelf', function scaleSelf(sx, sy, sz, ox, oy, oz) {
    sx = sx === undefined ? 1 : +sx; sy = sy === undefined ? sx : +sy; sz = sz === undefined ? 1 : +sz;
    ox = ox === undefined ? 0 : +ox; oy = oy === undefined ? 0 : +oy; oz = oz === undefined ? 0 : +oz;
    this.translateSelf(ox, oy, oz);
    var t = ident(); t[0] = sx; t[5] = sy; t[10] = sz;
    this.__m = mul(this.__m, t);
    this.translateSelf(-ox, -oy, -oz);
    if (sz !== 1 || oz !== 0) this.__2d = false;
    return this;
  });
  method(RO, 'scaleNonUniform', function scaleNonUniform(sx, sy) {
    return this.scale(sx === undefined ? 1 : sx, sy === undefined ? 1 : sy, 1, 0, 0, 0);
  });
  selfOp('scale3dSelf', function scale3dSelf(s, ox, oy, oz) {
    s = s === undefined ? 1 : +s;
    return this.scaleSelf(s, s, s, ox, oy, oz);
  });
  selfOp('rotateSelf', function rotateSelf(rx, ry, rz) {
    if (ry === undefined && rz === undefined) { rz = rx === undefined ? 0 : +rx; rx = 0; ry = 0; }
    rx = rx === undefined ? 0 : +rx; ry = ry === undefined ? 0 : +ry; rz = rz === undefined ? 0 : +rz;
    if (rz) this.rotateAxisAngleSelf(0, 0, 1, rz);
    if (ry) this.rotateAxisAngleSelf(0, 1, 0, ry);
    if (rx) this.rotateAxisAngleSelf(1, 0, 0, rx);
    return this;
  });
  selfOp('rotateFromVectorSelf', function rotateFromVectorSelf(x, y) {
    x = x === undefined ? 0 : +x; y = y === undefined ? 0 : +y;
    var a = Math.atan2(y, x) * 180 / Math.PI;
    return this.rotateAxisAngleSelf(0, 0, 1, a);
  });
  selfOp('rotateAxisAngleSelf', function rotateAxisAngleSelf(x, y, z, angle) {
    x = x === undefined ? 0 : +x; y = y === undefined ? 0 : +y; z = z === undefined ? 0 : +z; angle = angle === undefined ? 0 : +angle;
    var len = Math.sqrt(x * x + y * y + z * z);
    if (len === 0) return this;
    x /= len; y /= len; z /= len;
    var r = angle * Math.PI / 180, s = Math.sin(r / 2), sc = s * Math.cos(r / 2), sq = s * s;
    var t = ident();
    t[0] = 1 - 2 * (y * y + z * z) * sq; t[1] = 2 * (x * y * sq + z * sc); t[2] = 2 * (x * z * sq - y * sc);
    t[4] = 2 * (x * y * sq - z * sc); t[5] = 1 - 2 * (x * x + z * z) * sq; t[6] = 2 * (y * z * sq + x * sc);
    t[8] = 2 * (x * z * sq + y * sc); t[9] = 2 * (y * z * sq - x * sc); t[10] = 1 - 2 * (x * x + y * y) * sq;
    this.__m = mul(this.__m, t);
    if (x !== 0 || y !== 0) this.__2d = false;
    return this;
  });
  selfOp('skewXSelf', function skewXSelf(sx) {
    var t = ident(); t[4] = Math.tan((sx === undefined ? 0 : +sx) * Math.PI / 180);
    this.__m = mul(this.__m, t);
    return this;
  });
  selfOp('skewYSelf', function skewYSelf(sy) {
    var t = ident(); t[1] = Math.tan((sy === undefined ? 0 : +sy) * Math.PI / 180);
    this.__m = mul(this.__m, t);
    return this;
  });
  selfOp('invertSelf', function invertSelf() {
    var inv = invert(this.__m);
    if (!inv) { this.__m = this.__m.map(function () { return NaN; }); this.__2d = false; return this; }
    this.__m = inv;
    return this;
  });
  method(RO, 'inverse', function inverse() { return makeMatrix(DOMMatrix, this.__m.slice(), this.__2d).invertSelf(); });
  method(RO, 'flipX', function flipX() { var t = ident(); t[0] = -1; return makeMatrix(DOMMatrix, mul(this.__m, t), this.__2d); });
  method(RO, 'flipY', function flipY() { var t = ident(); t[5] = -1; return makeMatrix(DOMMatrix, mul(this.__m, t), this.__2d); });
  method(RW, 'setMatrixValue', function setMatrixValue(s) {
    var p = parseTransformList(s);
    this.__m = p.m; this.__2d = p.is2D;
    return this;
  });
  method(RO, 'transformPoint', function transformPoint(p) {
    p = dictionary(p, 'DOMPointInit');
    var r = mulPoint(this.__m, p.x === undefined ? 0 : +p.x, p.y === undefined ? 0 : +p.y, p.z === undefined ? 0 : +p.z, p.w === undefined ? 1 : +p.w);
    return new DOMPoint(r[0], r[1], r[2], r[3]);
  });
  method(RO, 'toFloat32Array', function toFloat32Array() { return new Float32Array(this.__m); });
  method(RO, 'toFloat64Array', function toFloat64Array() { return new Float64Array(this.__m); });
  function numText(v) {
    if (v !== v || v === Infinity || v === -Infinity) throw domErr('InvalidStateError', 'Cannot stringify a non-finite matrix.');
    return String(v);
  }
  method(RO, 'toString', function toString() {
    var m = this.__m;
    if (this.__2d) return 'matrix(' + [m[0], m[1], m[4], m[5], m[12], m[13]].map(numText).join(', ') + ')';
    return 'matrix3d(' + m.map(numText).join(', ') + ')';
  });
  method(RO, 'toJSON', function toJSON() {
    var o = {}, m = this.__m;
    o.a = m[0]; o.b = m[1]; o.c = m[4]; o.d = m[5]; o.e = m[12]; o.f = m[13];
    NAMES.forEach(function (n, i) { o[n] = m[i]; });
    o.is2D = this.__2d; o.isIdentity = this.isIdentity;
    return o;
  });
  [DOMMatrixReadOnly, DOMMatrix].forEach(function (C) {
    C.fromMatrix = function fromMatrix(o) { return matrixFromInit(o, false, C); };
    C.fromFloat32Array = function fromFloat32Array(a) {
      if (!(a instanceof Float32Array)) throw new TypeError('Float32Array expected');
      if (a.length !== 6 && a.length !== 16) throw new TypeError('The sequence must contain 6 or 16 elements.');
      return matrixInit(C, Array.prototype.slice.call(a));
    };
    C.fromFloat64Array = function fromFloat64Array(a) {
      if (!(a instanceof Float64Array)) throw new TypeError('Float64Array expected');
      if (a.length !== 6 && a.length !== 16) throw new TypeError('The sequence must contain 6 or 16 elements.');
      return matrixInit(C, Array.prototype.slice.call(a));
    };
  });
  tag(RO, 'DOMMatrixReadOnly');
  tag(RW, 'DOMMatrix');

  // ---- ImageData (HTML §4.12.5.1.16) ----------------------------------
  function ImageData(a, b, c, d) {
    if (!new.target) throw new TypeError("Failed to construct 'ImageData': Please use the 'new' operator.");
    var n = arguments.length;
    if (n < 2) throw new TypeError("Failed to construct 'ImageData': 2 arguments required, but only " + n + ' present.');
    var u32 = function (v) { return (+v) >>> 0; };
    var data, w, h, settings, fmt;
    var isF16 = typeof g.Float16Array === 'function' && a instanceof g.Float16Array;
    if (a instanceof Uint8ClampedArray || isF16) {
      // (data, sw, optional sh, optional settings)
      w = u32(b);
      var hh = c === undefined ? undefined : u32(c);
      settings = dictionary(d, 'ImageDataSettings');
      var len = a.length;
      if (len === 0 || len % 4 !== 0) throw domErr('InvalidStateError', "Failed to construct 'ImageData': The input data length is not a multiple of 4.");
      if (w === 0) throw domErr('IndexSizeError', "Failed to construct 'ImageData': The source width is zero or not a number.");
      var px = len / 4;
      if (px % w !== 0) throw domErr('IndexSizeError', "Failed to construct 'ImageData': The input data length is not a multiple of (4 * width).");
      h = px / w;
      if (hh !== undefined && hh !== h) throw domErr('IndexSizeError', "Failed to construct 'ImageData': The input data length is not equal to (4 * width * height).");
      fmt = settings.pixelFormat === undefined ? (isF16 ? 'rgba-float16' : 'rgba-unorm8') : str(settings.pixelFormat);
      if (fmt !== 'rgba-unorm8' && fmt !== 'rgba-float16') throw new TypeError("The provided value '" + fmt + "' is not a valid enum value of type ImageDataPixelFormat.");
      if ((fmt === 'rgba-float16') !== isF16) throw domErr('InvalidStateError', "Failed to construct 'ImageData': The pixel format does not match the data type.");
      data = a;
    } else {
      // (sw, sh, optional settings): unsigned long conversions first.
      w = u32(a);
      h = u32(b);
      settings = dictionary(c, 'ImageDataSettings');
      fmt = settings.pixelFormat === undefined ? 'rgba-unorm8' : str(settings.pixelFormat);
      if (fmt !== 'rgba-unorm8' && fmt !== 'rgba-float16') throw new TypeError("The provided value '" + fmt + "' is not a valid enum value of type ImageDataPixelFormat.");
      if (w === 0 || h === 0) throw domErr('IndexSizeError', "Failed to construct 'ImageData': The source " + (w === 0 ? 'width' : 'height') + ' is zero or not a number.');
      if (w * h * 4 > 2147483647) throw domErr('IndexSizeError', "Failed to construct 'ImageData': The requested image size exceeds the supported range.");
      data = fmt === 'rgba-float16' && typeof g.Float16Array === 'function' ? new g.Float16Array(w * h * 4) : new Uint8ClampedArray(w * h * 4);
    }
    var cs = settings.colorSpace === undefined ? 'srgb' : str(settings.colorSpace);
    if (SPACES.indexOf(cs) < 0) throw new TypeError("The provided value '" + cs + "' is not a valid enum value of type PredefinedColorSpace.");
    initImageData(this, data, w, h, cs, fmt);
  }
  ctor('ImageData', ImageData);
  function initImageData(o, data, w, h, cs, fmt) {
    hidden(o, '__id', { data: data, width: w, height: h, cs: cs || 'srgb', fmt: fmt || (data instanceof Uint8ClampedArray ? 'rgba-unorm8' : 'rgba-float16') });
    return o;
  }
  function newImageData(data, w, h, cs, fmt) { return initImageData(Object.create(ImageData.prototype), data, w, h, cs, fmt); }
  accessor(ImageData.prototype, 'width', function () { if (!this.__id) throw illegal(); return this.__id.width; });
  accessor(ImageData.prototype, 'height', function () { if (!this.__id) throw illegal(); return this.__id.height; });
  accessor(ImageData.prototype, 'data', function () { if (!this.__id) throw illegal(); return this.__id.data; });
  accessor(ImageData.prototype, 'colorSpace', function () { if (!this.__id) throw illegal(); return this.__id.cs; });
  accessor(ImageData.prototype, 'pixelFormat', function () {
    if (!this.__id) throw illegal();
    return this.__id.fmt;
  });
  tag(ImageData.prototype, 'ImageData');
  // Unpremultiplied RGBA8 bytes of any ImageData (float16 data is converted).
  function imageDataBytes(id) {
    var d = id.data;
    if (d instanceof Uint8ClampedArray) return d;
    var out = new Uint8ClampedArray(d.length);
    for (var i = 0; i < d.length; i++) out[i] = Math.round(d[i] * 255);
    return out;
  }

  // ---- CanvasGradient / CanvasPattern ----------------------------------
  var CanvasGradient = illegalCtor('CanvasGradient');
  var CanvasPattern = illegalCtor('CanvasPattern');
  tag(CanvasGradient.prototype, 'CanvasGradient');
  tag(CanvasPattern.prototype, 'CanvasPattern');
  function makeGradient(kind, c) {
    var o = Object.create(CanvasGradient.prototype);
    hidden(o, '__g', { tok: ++tokens, kind: kind, c: c, stops: [], users: [] });
    return o;
  }
  method(CanvasGradient.prototype, 'addColorStop', function addColorStop(offset, color) {
    var gr = this.__g;
    if (!gr) throw illegal();
    if (arguments.length < 2) throw tooFew('CanvasGradient', 'addColorStop', 2, arguments.length);
    offset = dbl(offset, 'offset');
    color = str(color);
    if (offset < 0 || offset > 1) throw domErr('IndexSizeError', "Failed to execute 'addColorStop' on 'CanvasGradient': The provided value (" + offset + ') is outside the range (0.0, 1.0).');
    var rgba = N(PARSE_COLOR, -1, color);
    if (rgba === null) throw domErr('SyntaxError', "Failed to execute 'addColorStop' on 'CanvasGradient': The value provided ('" + color + "') could not be parsed as a color.");
    var s = gr.stops, i = s.length;
    while (i > 0 && s[i - 6] > offset) i -= 6;
    s.splice(i, 0, offset, rgba[0], rgba[1], rgba[2], rgba[3], rgba[4]);
    // Gradients are live: contexts using this one get the new stops.
    for (var k = 0; k < gr.users.length; k++) {
      var u = gr.users[k];
      var cur = N(GET, u[0].__c2d, u[1] ? 'strokeStyle' : 'fillStyle');
      if (cur === gr.tok) pushGradient(u[0], u[1], this);
    }
  });
  function pushGradient(ctx, which, obj) {
    var gr = obj.__g, c = gr.c;
    N(SET_GRADIENT, ctx.__c2d, which, gr.tok, gr.kind, c[0], c[1], c[2], c[3], c[4], c[5], gr.stops);
  }
  method(CanvasPattern.prototype, 'setTransform', function setTransform(transform) {
    var p = this.__pat;
    if (!p) throw illegal();
    var f = fixup(transform, true).m;
    if (![f[0], f[1], f[4], f[5], f[12], f[13]].every(isFinite)) return;
    p.m = [f[0], f[1], f[4], f[5], f[12], f[13]];
    p.tok = ++tokens;
    for (var k = 0; k < p.users.length; k++) {
      var u = p.users[k];
      if (N(GET, u[0].__c2d, u[1] ? 'strokeStyle' : 'fillStyle') === p.old) pushPattern(u[0], u[1], this);
    }
  });
  function pushPattern(ctx, which, obj) {
    var p = obj.__pat;
    p.old = p.tok;
    var r = N(SET_PATTERN, ctx.__c2d, which, p.tok, p.rep, p.m);
    if (r === 'need') N(SET_PATTERN, ctx.__c2d, which, p.tok, p.rep, p.m, p.w, p.h, p.data);
  }
  function remember(ctx, obj, slot, which) {
    var users = obj[slot].users;
    for (var i = 0; i < users.length; i++) if (users[i][0] === ctx && users[i][1] === which) return;
    if (users.length < 64) users.push([ctx, which]);
    var m = ctx.__styles;
    if (m.size > 256) m.clear();
    m.set(obj[slot].tok, obj);
  }
  function setStyle(ctx, which, v) {
    var id = ctx.__c2d;
    if (id === undefined) throw illegal();
    if (v instanceof CanvasGradient && v.__g) {
      pushGradient(ctx, which, v);
      remember(ctx, v, '__g', which);
      return;
    }
    if (v instanceof CanvasPattern && v.__pat) {
      pushPattern(ctx, which, v);
      remember(ctx, v, '__pat', which);
      ctx.__styles.set(v.__pat.tok, v);
      return;
    }
    if (v !== null && typeof v === 'object' && typeof v.r === 'number' && typeof v.g === 'number' && typeof v.b === 'number') {
      // Color objects ({r, g, b, a} with channels in 0..1).
      var ch = function (x) { x = +x; return x !== x ? 0 : Math.round(Math.min(1, Math.max(0, x)) * 255); };
      var al = v.a === undefined ? 1 : Math.min(1, Math.max(0, +v.a || 0));
      N(SET, id, which ? 'strokeStyle' : 'fillStyle', 'rgba(' + ch(v.r) + ', ' + ch(v.g) + ', ' + ch(v.b) + ', ' + al + ')');
      return;
    }
    N(SET, id, which ? 'strokeStyle' : 'fillStyle', str(v));
  }
  function getStyle(ctx, which) {
    var r = N(GET, ctx.__c2d, which ? 'strokeStyle' : 'fillStyle');
    if (typeof r === 'number') {
      var o = ctx.__styles.get(r);
      return o === undefined ? null : o;
    }
    return r;
  }

  // ---- Path2D ----------------------------------------------------------
  function Path2D(path) {
    if (!new.target) throw new TypeError("Failed to construct 'Path2D': Please use the 'new' operator.");
    var c = [];
    if (path instanceof Path2D) c = path.__p.slice();
    else if (path !== undefined) c = N(PARSE_SVG_PATH, -1, str(path));
    hidden(this, '__p', c);
  }
  ctor('Path2D', Path2D);
  tag(Path2D.prototype, 'Path2D');
  function cmds(o) {
    var c = o.__p;
    if (!c) throw illegal();
    if (c.length > 4000000) throw new RangeError('Path2D is too large');
    return c;
  }
  method(Path2D.prototype, 'addPath', function addPath(path, transform) {
    var c = cmds(this);
    if (arguments.length < 1) throw tooFew('Path2D', 'addPath', 1, 0);
    if (!(path instanceof Path2D)) throw new TypeError("Failed to execute 'addPath' on 'Path2D': parameter 1 is not of type 'Path2D'.");
    var m = fixup(transform, true).m;
    if (![m[0], m[1], m[4], m[5], m[12], m[13]].every(isFinite)) return;
    var src = path.__p.slice();
    c.push(P_BEGIN_ADD, m[0], m[1], m[4], m[5], m[12], m[13]);
    for (var i = 0; i < src.length; i++) c.push(src[i]);
    c.push(P_END_ADD);
  });
  method(Path2D.prototype, 'closePath', function closePath() { cmds(this).push(P_CLOSE); });
  method(Path2D.prototype, 'moveTo', function moveTo(x, y) {
    if (arguments.length < 2) throw tooFew('Path2D', 'moveTo', 2, arguments.length);
    cmds(this).push(P_MOVE, +x, +y);
  });
  method(Path2D.prototype, 'lineTo', function lineTo(x, y) {
    if (arguments.length < 2) throw tooFew('Path2D', 'lineTo', 2, arguments.length);
    cmds(this).push(P_LINE, +x, +y);
  });
  method(Path2D.prototype, 'quadraticCurveTo', function quadraticCurveTo(cpx, cpy, x, y) {
    if (arguments.length < 4) throw tooFew('Path2D', 'quadraticCurveTo', 4, arguments.length);
    cmds(this).push(P_QUAD, +cpx, +cpy, +x, +y);
  });
  method(Path2D.prototype, 'bezierCurveTo', function bezierCurveTo(a, b, c, d, x, y) {
    if (arguments.length < 6) throw tooFew('Path2D', 'bezierCurveTo', 6, arguments.length);
    cmds(this).push(P_CUBIC, +a, +b, +c, +d, +x, +y);
  });
  method(Path2D.prototype, 'arcTo', function arcTo(x1, y1, x2, y2, r) {
    if (arguments.length < 5) throw tooFew('Path2D', 'arcTo', 5, arguments.length);
    x1 = +x1; y1 = +y1; x2 = +x2; y2 = +y2; r = +r;
    var c = cmds(this);
    if (isFinite(x1) && isFinite(y1) && isFinite(x2) && isFinite(y2) && isFinite(r) && r < 0) {
      if (c.length === 0) c.push(P_MOVE, x1, y1);
      throw domErr('IndexSizeError', "Failed to execute 'arcTo' on 'Path2D': The radius provided (" + r + ') is negative.');
    }
    c.push(P_ARC_TO, x1, y1, x2, y2, r);
  });
  method(Path2D.prototype, 'rect', function rect(x, y, w, h) {
    if (arguments.length < 4) throw tooFew('Path2D', 'rect', 4, arguments.length);
    cmds(this).push(P_RECT, +x, +y, +w, +h);
  });
  method(Path2D.prototype, 'arc', function arc(x, y, r, s, e, ccw) {
    if (arguments.length < 5) throw tooFew('Path2D', 'arc', 5, arguments.length);
    x = +x; y = +y; r = +r; s = +s; e = +e;
    if (isFinite(x) && isFinite(y) && isFinite(r) && isFinite(s) && isFinite(e) && r < 0) throw domErr('IndexSizeError', "Failed to execute 'arc' on 'Path2D': The radius provided (" + r + ') is negative.');
    cmds(this).push(P_ELLIPSE, x, y, r, r, 0, s, e, ccw ? 1 : 0);
  });
  method(Path2D.prototype, 'ellipse', function ellipse(x, y, rx, ry, rot, s, e, ccw) {
    if (arguments.length < 7) throw tooFew('Path2D', 'ellipse', 7, arguments.length);
    x = +x; y = +y; rx = +rx; ry = +ry; rot = +rot; s = +s; e = +e;
    if ([x, y, rx, ry, rot, s, e].every(isFinite) && (rx < 0 || ry < 0)) throw domErr('IndexSizeError', "Failed to execute 'ellipse' on 'Path2D': The " + (rx < 0 ? 'major' : 'minor') + '-axis radius provided is negative.');
    cmds(this).push(P_ELLIPSE, x, y, rx, ry, rot, s, e, ccw ? 1 : 0);
  });
  // roundRect radii (HTML "roundRect" steps 2-8): eight numbers, or null
  // when the call must do nothing.
  function radii(r, iface) {
    var list;
    if (r === undefined) list = [0];
    else if (typeof r === 'object' && r !== null && typeof r[Symbol.iterator] === 'function' && !(r instanceof DOMPointReadOnly)) list = Array.from(r);
    else list = [r];
    if (list.length < 1 || list.length > 4) throw new RangeError("Failed to execute 'roundRect' on '" + iface + "': " + list.length + ' radii provided. Between one and four radii are necessary.');
    var out = [], bad = false;
    for (var i = 0; i < list.length; i++) {
      var v = list[i], x, y;
      if (typeof v === 'object' && v !== null) {
        x = v.x === undefined ? 0 : +v.x; y = v.y === undefined ? 0 : +v.y;
        if (!isFinite(x) || !isFinite(y)) { bad = true; continue; }
        if (x < 0 || y < 0) throw new RangeError("Failed to execute 'roundRect' on '" + iface + "': Y-radius value " + y + ' is negative.');
      } else {
        x = y = +v;
        if (!isFinite(x)) { bad = true; continue; }
        if (x < 0) throw new RangeError("Failed to execute 'roundRect' on '" + iface + "': Radius value " + x + ' is negative.');
      }
      out.push([x, y]);
    }
    if (bad) return null;
    var ul, ur, lr, ll;
    if (out.length === 4) { ul = out[0]; ur = out[1]; lr = out[2]; ll = out[3]; }
    else if (out.length === 3) { ul = out[0]; ur = ll = out[1]; lr = out[2]; }
    else if (out.length === 2) { ul = lr = out[0]; ur = ll = out[1]; }
    else { ul = ur = lr = ll = out[0]; }
    return [ul[0], ul[1], ur[0], ur[1], lr[0], lr[1], ll[0], ll[1]];
  }
  method(Path2D.prototype, 'roundRect', function roundRect(x, y, w, h, r) {
    if (arguments.length < 4) throw tooFew('Path2D', 'roundRect', 4, arguments.length);
    x = +x; y = +y; w = +w; h = +h;
    var rr = radii(r, 'Path2D');
    if (![x, y, w, h].every(isFinite) || rr === null) return;
    var c = cmds(this);
    c.push(P_ROUND_RECT, x, y, w, h);
    for (var i = 0; i < 8; i++) c.push(rr[i]);
  });

  // ---- image sources ---------------------------------------------------
  function isElementSource(img) {
    return (g.HTMLImageElement && img instanceof g.HTMLImageElement) || (g.HTMLCanvasElement && img instanceof g.HTMLCanvasElement)
      || (g.HTMLVideoElement && img instanceof g.HTMLVideoElement) || (g.SVGImageElement && img instanceof g.SVGImageElement);
  }
  // [kind, ref] for the host: 0 element, 1 surface id, 2 bitmap token.
  function sourceOf(img, method) {
    if (img !== null && typeof img === 'object') {
      if (isElementSource(img)) return [0, img];
      if (img instanceof OffscreenCanvas && img.__oc) {
        if (img.__oc.detached) throw domErr('InvalidStateError', "Failed to execute '" + method + "': The image source is detached.");
        return [1, img.__oc.id];
      }
      if (img instanceof ImageBitmap && img.__bm) {
        if (img.__bm.closed) throw domErr('InvalidStateError', "Failed to execute '" + method + "': The image source is detached.");
        return [2, img.__bm.tok, img];
      }
      if (g.VideoFrame && img instanceof g.VideoFrame) return [0, img];
    }
    throw new TypeError("Failed to execute '" + method + "': The provided value is not of type '(CSSImageValue or HTMLCanvasElement or HTMLImageElement or HTMLVideoElement or ImageBitmap or OffscreenCanvas or SVGImageElement or VideoFrame)'.");
  }
  function upload(src) {
    var b = src[2].__bm;
    N(UPLOAD_BITMAP, b.tok, b.w, b.h, b.data);
  }

  // ---- CanvasFilter (filter primitives as dictionaries) ------------------
  // `beginLayer({filter: {...}})` and `ctx.filter = new CanvasFilter(...)`
  // describe SVG filter primitives as objects. They are converted here
  // into a flat number list that `narsil_canvas::filter::Filter::from_spec`
  // reads: [op, params...] per primitive.
  var F_BLUR = 1, F_SHADOW = 2, F_MATRIX = 3, F_TRANSFER = 4, F_CONVOLVE = 5, F_TURBULENCE = 6;
  function cfError(what, msg) { return new TypeError("Failed to execute '" + what + "': " + msg); }
  // WebIDL `double` from a record value: present but undefined is an error.
  function cfDouble(v, key, what) {
    var n = +v;
    if (v === undefined || n !== n || n === Infinity || n === -Infinity) throw cfError(what, "The value of '" + key + "' is not a finite number.");
    return n;
  }
  function cfIterable(v) { return v !== null && typeof v === 'object' && typeof v[Symbol.iterator] === 'function'; }
  function cfList(v, key, what) {
    if (!cfIterable(v)) throw cfError(what, "The value of '" + key + "' is not a sequence.");
    var out = [];
    for (var it = v[Symbol.iterator](), s = it.next(); !s.done; s = it.next()) out.push(s.value);
    return out;
  }
  // `(double or sequence<double>)` with at most two entries.
  function cfPair(v, key, what) {
    if (!cfIterable(v)) { var n = cfDouble(v, key, what); return [n, n]; }
    var a = cfList(v, key, what).map(function (x) { return cfDouble(x, key, what); });
    if (a.length > 2) throw cfError(what, "'" + key + "' takes one or two numbers.");
    if (a.length === 0) return [0, 0];
    return [a[0], a.length > 1 ? a[1] : a[0]];
  }
  function cfEnum(v, list, key, what) {
    if (typeof v === 'symbol') throw cfError(what, "Invalid '" + key + "'.");
    v = String(v);
    if (list.indexOf(v) < 0) throw cfError(what, "'" + v + "' is not a valid value for '" + key + "'.");
    return v;
  }
  function cfGet(o, key, def, conv) {
    return (key in o) ? conv(o[key]) : def;
  }
  function cfColor(v, ctxId, what) {
    if (typeof v !== 'string') throw cfError(what, "The value of 'floodColor' is not a color string.");
    var c = N(PARSE_COLOR, ctxId, v);
    if (c === null) throw cfError(what, "The value provided ('" + v + "') could not be parsed as a color.");
    return c;
  }
  function cfTransferFunction(o, key, out, what) {
    if (!(key in o)) { out.push(0, 0); return; }
    var f = o[key];
    if (f === null || (typeof f !== 'object' && typeof f !== 'function')) throw cfError(what, "'" + key + "' is not a dictionary.");
    var type = cfGet(f, 'type', 'identity', function (v) { return cfEnum(v, ['identity', 'table', 'discrete', 'linear', 'gamma'], 'type', what); });
    var num = function (k, d) { return cfGet(f, k, d, function (v) { return cfDouble(v, k, what); }); };
    if (type === 'table' || type === 'discrete') {
      var vals = cfGet(f, 'tableValues', [], function (v) { return cfList(v, 'tableValues', what).map(function (x) { return cfDouble(x, 'tableValues', what); }); });
      out.push(type === 'table' ? 1 : 2, vals.length);
      for (var i = 0; i < vals.length; i++) out.push(vals[i]);
    } else if (type === 'linear') {
      out.push(3, 2, num('slope', 1), num('intercept', 0));
    } else if (type === 'gamma') {
      out.push(4, 3, num('amplitude', 1), num('exponent', 1), num('offset', 0));
    } else {
      out.push(0, 0);
    }
  }
  function cfPrimitive(o, out, ctxId, what) {
    if (o === null || (typeof o !== 'object' && typeof o !== 'function')) throw cfError(what, 'A filter must be an object.');
    var name = String(o.name);
    var num = function (k, d) { return cfGet(o, k, d, function (v) { return cfDouble(v, k, what); }); };
    if (name === 'gaussianBlur') {
      if (!('stdDeviation' in o)) throw cfError(what, "Failed to construct 'gaussianBlur' filter, 'stdDeviation' required.");
      var s = cfPair(o.stdDeviation, 'stdDeviation', what);
      out.push(F_BLUR, s[0], s[1]);
    } else if (name === 'dropShadow') {
      var dx = num('dx', 2), dy = num('dy', 2);
      var sd = cfGet(o, 'stdDeviation', [2, 2], function (v) { return cfPair(v, 'stdDeviation', what); });
      var color = cfGet(o, 'floodColor', [0, 0, 0, 1, 1], function (v) { return cfColor(v, ctxId, what); });
      var opacity = num('floodOpacity', 1);
      out.push(F_SHADOW, dx, dy, sd[0], sd[1], color[0], color[1], color[2], color[3] * Math.max(0, Math.min(1, opacity)));
    } else if (name === 'colorMatrix') {
      var type = cfGet(o, 'type', 'matrix', function (v) { return cfEnum(v, ['matrix', 'saturate', 'hueRotate', 'luminanceToAlpha'], 'type', what); });
      var m;
      if (type === 'matrix') {
        if (!('values' in o)) throw cfError(what, "Failed to construct 'colorMatrix' filter, 'values' required.");
        m = cfList(o.values, 'values', what).map(function (x) { return cfDouble(x, 'values', what); });
        if (m.length !== 20) throw cfError(what, "'values' must have 20 entries.");
        out.push(F_MATRIX, 0);
        for (var i = 0; i < 20; i++) out.push(m[i]);
      } else if (type === 'luminanceToAlpha') {
        out.push(F_MATRIX, 3);
      } else {
        if (!('values' in o)) throw cfError(what, "Failed to construct 'colorMatrix' filter, 'values' required.");
        out.push(F_MATRIX, type === 'saturate' ? 1 : 2, cfDouble(o.values, 'values', what));
      }
    } else if (name === 'componentTransfer') {
      out.push(F_TRANSFER);
      ['funcR', 'funcG', 'funcB', 'funcA'].forEach(function (k) { cfTransferFunction(o, k, out, what); });
    } else if (name === 'convolveMatrix') {
      if (!('kernelMatrix' in o)) throw cfError(what, "Failed to construct 'convolveMatrix' filter, 'kernelMatrix' required.");
      var rows = cfList(o.kernelMatrix, 'kernelMatrix', what).map(function (r) {
        return cfList(r, 'kernelMatrix', what).map(function (x) { return cfDouble(x, 'kernelMatrix', what); });
      });
      if (rows.length === 0) throw cfError(what, "'kernelMatrix' must not be empty.");
      var w = rows[0].length;
      for (var r = 0; r < rows.length; r++) if (rows[r].length !== w) throw cfError(what, "'kernelMatrix' rows must have the same length.");
      if (w === 0 && rows.length > 1) throw cfError(what, "'kernelMatrix' rows must not be empty.");
      var sum = 0, flat = [];
      rows.forEach(function (row) { row.forEach(function (v) { sum += v; flat.push(v); }); });
      var divisor = num('divisor', sum === 0 ? 1 : sum);
      var tx = num('targetX', Math.floor(w / 2)), ty = num('targetY', Math.floor(rows.length / 2));
      var edge = cfGet(o, 'edgeMode', 'duplicate', function (v) { return cfEnum(v, ['duplicate', 'wrap', 'none'], 'edgeMode', what); });
      var keepAlpha = cfGet(o, 'preserveAlpha', false, function (v) { return !!v; });
      out.push(F_CONVOLVE, w, rows.length, divisor === 0 ? 1 : divisor, num('bias', 0), tx, ty,
        edge === 'duplicate' ? 0 : edge === 'wrap' ? 1 : 2, keepAlpha ? 1 : 0);
      for (var k = 0; k < flat.length; k++) out.push(flat[k]);
    } else if (name === 'turbulence') {
      var bf = cfGet(o, 'baseFrequency', [0, 0], function (v) { return cfPair(v, 'baseFrequency', what); });
      if (bf[0] < 0 || bf[1] < 0) throw cfError(what, "'baseFrequency' must not be negative.");
      var octaves = num('numOctaves', 1);
      if (octaves < 0) throw cfError(what, "'numOctaves' must not be negative.");
      var seed = num('seed', 0);
      var stitch = cfGet(o, 'stitchTiles', 'noStitch', function (v) { return cfEnum(v, ['stitch', 'noStitch'], 'stitchTiles', what); });
      var kind = cfGet(o, 'type', 'turbulence', function (v) { return cfEnum(v, ['fractalNoise', 'turbulence'], 'type', what); });
      out.push(F_TURBULENCE, bf[0], bf[1], Math.floor(octaves), seed, stitch === 'stitch' ? 1 : 0, kind === 'fractalNoise' ? 1 : 0);
    }
    // Unknown primitives are ignored.
  }
  // `CanvasFilterInput or sequence<CanvasFilterInput>` -> spec numbers.
  function resolveFilter(input, ctxId, what) {
    var out = [];
    if (Array.isArray(input)) {
      for (var i = 0; i < input.length; i++) cfPrimitive(input[i], out, ctxId, what);
    } else {
      cfPrimitive(input, out, ctxId, what);
    }
    return out;
  }
  var CanvasFilter = ctor('CanvasFilter', function CanvasFilter(init) {
    if (!new.target) throw new TypeError("Failed to construct 'CanvasFilter': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
    if (init === null || (typeof init !== 'object' && typeof init !== 'function')) {
      throw new TypeError("Failed to construct 'CanvasFilter': The provided value is not of type '(CanvasFilterDictionary or sequence<CanvasFilterDictionary>)'.");
    }
    hidden(this, '__cf', resolveFilter(init, -1, "construct 'CanvasFilter'"));
  });
  tag(CanvasFilter.prototype, 'CanvasFilter');
  // Contexts store a token for the object assigned to `ctx.filter`; the
  // getter returns the same object (bounded registry).
  var filterObjects = new Map(), filterTokens = 0;
  function filterToken(f) {
    if (!f.__tok) hidden(f, '__tok', ++filterTokens);
    filterObjects.delete(f.__tok);
    filterObjects.set(f.__tok, f);
    if (filterObjects.size > 256) filterObjects.delete(filterObjects.keys().next().value);
    return f.__tok;
  }

  // ---- the 2D context ----------------------------------------------------
  function makeContextProto(name) {
    var C = illegalCtor(name);
    var P = C.prototype;
    tag(P, name);
    function id(o) { var v = o.__c2d; if (v === undefined) throw illegal(); return v; }
    accessor(P, 'canvas', function () { id(this); return this.__canvas; });
    // Attributes stored in the host.
    [['globalAlpha', 1], ['lineWidth', 1], ['miterLimit', 1], ['lineDashOffset', 1], ['shadowOffsetX', 1], ['shadowOffsetY', 1], ['shadowBlur', 1],
      ['globalCompositeOperation', 0], ['lineCap', 0], ['lineJoin', 0], ['shadowColor', 0], ['imageSmoothingQuality', 0],
      ['font', 0], ['textAlign', 0], ['textBaseline', 0], ['direction', 0], ['letterSpacing', 0], ['wordSpacing', 0],
      ['fontKerning', 0], ['fontStretch', 0], ['fontVariantCaps', 0], ['textRendering', 0], ['lang', 0]].forEach(function (a) {
      var nm = a[0], numeric = a[1];
      accessor(P, nm, function () { return N(GET, id(this), nm); }, numeric ? function (v) { N(SET, id(this), nm, +v); } : function (v) { N(SET, id(this), nm, str(v)); });
    });
    accessor(P, 'filter', function () {
      var v = N(GET, id(this), 'filter');
      if (typeof v === 'string' && v.charCodeAt(0) === 1) return filterObjects.get(+v.slice(1)) || 'none';
      return v;
    }, function (v) {
      var i = id(this);
      if (v instanceof CanvasFilter && v.__cf) { N(SET_FILTER_SPEC, i, filterToken(v), v.__cf); return; }
      N(SET, i, 'filter', str(v));
    });
    accessor(P, 'imageSmoothingEnabled', function () { return N(GET, id(this), 'imageSmoothingEnabled'); }, function (v) { N(SET, id(this), 'imageSmoothingEnabled', !!v); });
    accessor(P, 'fillStyle', function () { id(this); return getStyle(this, 0); }, function (v) { setStyle(this, 0, v); });
    accessor(P, 'strokeStyle', function () { id(this); return getStyle(this, 1); }, function (v) { setStyle(this, 1, v); });
    method(P, 'save', function save() { N(SAVE, id(this)); });
    method(P, 'restore', function restore() { N(RESTORE, id(this)); });
    method(P, 'reset', function reset() { N(RESET, id(this)); });
    method(P, 'isContextLost', function isContextLost() { id(this); return false; });
    method(P, 'getContextAttributes', function getContextAttributes() {
      var r = N(CONTEXT_ATTRS, id(this));
      r.toneMapping = { mode: r.toneMappingMode || 'standard' };
      delete r.toneMappingMode;
      return r;
    });
    method(P, 'scale', function scale(x, y) {
      if (arguments.length < 2) throw tooFew(name, 'scale', 2, arguments.length);
      N(SCALE, id(this), +x, +y);
    });
    method(P, 'rotate', function rotate(a) {
      if (arguments.length < 1) throw tooFew(name, 'rotate', 1, 0);
      N(ROTATE, id(this), +a);
    });
    method(P, 'translate', function translate(x, y) {
      if (arguments.length < 2) throw tooFew(name, 'translate', 2, arguments.length);
      N(TRANSLATE, id(this), +x, +y);
    });
    method(P, 'transform', function transform(a, b, c, d, e, f) {
      if (arguments.length < 6) throw tooFew(name, 'transform', 6, arguments.length);
      N(TRANSFORM, id(this), +a, +b, +c, +d, +e, +f);
    });
    method(P, 'setTransform', function setTransform(a, b, c, d, e, f) {
      var n = arguments.length, i = id(this);
      if (n === 0 || n === 1) {
        var m = fixup(a, true).m;
        N(SET_TRANSFORM, i, m[0], m[1], m[4], m[5], m[12], m[13]);
        return;
      }
      if (n < 6) throw new TypeError("Failed to execute 'setTransform' on '" + name + "': Valid arities are: [0, 1, 6], but " + n + ' arguments provided.');
      N(SET_TRANSFORM, i, +a, +b, +c, +d, +e, +f);
    });
    method(P, 'resetTransform', function resetTransform() { N(RESET_TRANSFORM, id(this)); });
    method(P, 'getTransform', function getTransform() {
      var t = N(GET_TRANSFORM, id(this));
      return makeMatrix(DOMMatrix, [t[0], t[1], 0, 0, t[2], t[3], 0, 0, 0, 0, 1, 0, t[4], t[5], 0, 1], true);
    });
    method(P, 'createLinearGradient', function createLinearGradient(x0, y0, x1, y1) {
      if (arguments.length < 4) throw tooFew(name, 'createLinearGradient', 4, arguments.length);
      id(this);
      return makeGradient(0, [dbl(x0, 'x0'), dbl(y0, 'y0'), dbl(x1, 'x1'), dbl(y1, 'y1'), 0, 0]);
    });
    method(P, 'createRadialGradient', function createRadialGradient(x0, y0, r0, x1, y1, r1) {
      if (arguments.length < 6) throw tooFew(name, 'createRadialGradient', 6, arguments.length);
      id(this);
      var c = [dbl(x0, 'x0'), dbl(y0, 'y0'), dbl(r0, 'r0'), dbl(x1, 'x1'), dbl(y1, 'y1'), dbl(r1, 'r1')];
      if (c[2] < 0 || c[5] < 0) throw domErr('IndexSizeError', "Failed to execute 'createRadialGradient' on '" + name + "': The " + (c[2] < 0 ? 'r0' : 'r1') + ' provided is less than 0.');
      return makeGradient(1, c);
    });
    method(P, 'createConicGradient', function createConicGradient(angle, x, y) {
      if (arguments.length < 3) throw tooFew(name, 'createConicGradient', 3, arguments.length);
      id(this);
      return makeGradient(2, [dbl(angle, 'startAngle'), dbl(x, 'x'), dbl(y, 'y'), 0, 0, 0]);
    });
    method(P, 'createPattern', function createPattern(image, repetition) {
      if (arguments.length < 2) throw tooFew(name, 'createPattern', 2, arguments.length);
      id(this);
      var src = sourceOf(image, 'createPattern');
      var rep = repetition === null ? '' : str(repetition);
      if (rep === '') rep = 'repeat';
      if (rep !== 'repeat' && rep !== 'repeat-x' && rep !== 'repeat-y' && rep !== 'no-repeat') {
        throw domErr('SyntaxError', "Failed to execute 'createPattern' on '" + name + "': The provided type ('" + rep + "') is not one of 'repeat', 'no-repeat', 'repeat-x', or 'repeat-y'.");
      }
      var shot = N(SNAPSHOT, src[0], src[1], NaN, NaN, NaN, NaN, true);
      if (shot === 'need') { upload(src); shot = N(SNAPSHOT, src[0], src[1], NaN, NaN, NaN, NaN, true); }
      if (shot === null) return null;
      var o = Object.create(CanvasPattern.prototype);
      hidden(o, '__pat', { tok: ++tokens, old: 0, rep: rep, m: [1, 0, 0, 1, 0, 0], w: shot[0], h: shot[1], data: shot[2], users: [] });
      return o;
    });
    method(P, 'clearRect', function clearRect(x, y, w, h) {
      if (arguments.length < 4) throw tooFew(name, 'clearRect', 4, arguments.length);
      N(CLEAR_RECT, id(this), +x, +y, +w, +h);
    });
    method(P, 'fillRect', function fillRect(x, y, w, h) {
      if (arguments.length < 4) throw tooFew(name, 'fillRect', 4, arguments.length);
      N(FILL_RECT, id(this), +x, +y, +w, +h);
    });
    method(P, 'strokeRect', function strokeRect(x, y, w, h) {
      if (arguments.length < 4) throw tooFew(name, 'strokeRect', 4, arguments.length);
      N(STROKE_RECT, id(this), +x, +y, +w, +h);
    });
    method(P, 'beginPath', function beginPath() { N(BEGIN_PATH, id(this)); });
    method(P, 'closePath', function closePath() { N(CLOSE_PATH, id(this)); });
    method(P, 'moveTo', function moveTo(x, y) {
      if (arguments.length < 2) throw tooFew(name, 'moveTo', 2, arguments.length);
      N(MOVE_TO, id(this), +x, +y);
    });
    method(P, 'lineTo', function lineTo(x, y) {
      if (arguments.length < 2) throw tooFew(name, 'lineTo', 2, arguments.length);
      N(LINE_TO, id(this), +x, +y);
    });
    method(P, 'quadraticCurveTo', function quadraticCurveTo(cpx, cpy, x, y) {
      if (arguments.length < 4) throw tooFew(name, 'quadraticCurveTo', 4, arguments.length);
      N(QUAD_TO, id(this), +cpx, +cpy, +x, +y);
    });
    method(P, 'bezierCurveTo', function bezierCurveTo(a, b, c, d, x, y) {
      if (arguments.length < 6) throw tooFew(name, 'bezierCurveTo', 6, arguments.length);
      N(CUBIC_TO, id(this), +a, +b, +c, +d, +x, +y);
    });
    method(P, 'arcTo', function arcTo(x1, y1, x2, y2, r) {
      if (arguments.length < 5) throw tooFew(name, 'arcTo', 5, arguments.length);
      N(ARC_TO, id(this), +x1, +y1, +x2, +y2, +r);
    });
    method(P, 'rect', function rect(x, y, w, h) {
      if (arguments.length < 4) throw tooFew(name, 'rect', 4, arguments.length);
      N(RECT, id(this), +x, +y, +w, +h);
    });
    method(P, 'roundRect', function roundRect(x, y, w, h, r) {
      if (arguments.length < 4) throw tooFew(name, 'roundRect', 4, arguments.length);
      var i = id(this);
      x = +x; y = +y; w = +w; h = +h;
      var rr = radii(r, name);
      if (![x, y, w, h].every(isFinite) || rr === null) return;
      N(ROUND_RECT, i, x, y, w, h, rr);
    });
    method(P, 'arc', function arc(x, y, r, s, e, ccw) {
      if (arguments.length < 5) throw tooFew(name, 'arc', 5, arguments.length);
      N(ARC, id(this), +x, +y, +r, +s, +e, !!ccw);
    });
    method(P, 'ellipse', function ellipse(x, y, rx, ry, rot, s, e, ccw) {
      if (arguments.length < 7) throw tooFew(name, 'ellipse', 7, arguments.length);
      N(ELLIPSE, id(this), +x, +y, +rx, +ry, +rot, +s, +e, !!ccw);
    });
    method(P, 'fill', function fill(a, b) {
      var i = id(this);
      if (a instanceof Path2D) N(FILL, i, fillRule(b, 'fill'), cmds(a));
      else N(FILL, i, fillRule(a, 'fill'));
    });
    method(P, 'stroke', function stroke(path) {
      var i = id(this);
      if (path === undefined) { N(STROKE, i); return; }
      if (!(path instanceof Path2D)) throw new TypeError("Failed to execute 'stroke' on '" + name + "': parameter 1 is not of type 'Path2D'.");
      N(STROKE, i, cmds(path));
    });
    method(P, 'clip', function clip(a, b) {
      var i = id(this);
      if (a instanceof Path2D) N(CLIP, i, fillRule(b, 'clip'), cmds(a));
      else N(CLIP, i, fillRule(a, 'clip'));
    });
    method(P, 'isPointInPath', function isPointInPath(a, b, c, d) {
      var i = id(this), n = arguments.length;
      if (a instanceof Path2D) {
        if (n < 3) throw tooFew(name, 'isPointInPath', 3, n);
        return N(IN_PATH, i, +b, +c, fillRule(d, 'isPointInPath'), cmds(a));
      }
      if (n < 2) throw tooFew(name, 'isPointInPath', 2, n);
      return N(IN_PATH, i, +a, +b, fillRule(c, 'isPointInPath'));
    });
    method(P, 'isPointInStroke', function isPointInStroke(a, b, c) {
      var i = id(this), n = arguments.length;
      if (a instanceof Path2D) {
        if (n < 3) throw tooFew(name, 'isPointInStroke', 3, n);
        return N(IN_STROKE, i, +b, +c, cmds(a));
      }
      if (n < 2) throw tooFew(name, 'isPointInStroke', 2, n);
      return N(IN_STROKE, i, +a, +b);
    });
    method(P, 'setLineDash', function setLineDash(segments) {
      if (arguments.length < 1) throw tooFew(name, 'setLineDash', 1, 0);
      var i = id(this);
      if (segments === null || typeof segments !== 'object' || typeof segments[Symbol.iterator] !== 'function') throw new TypeError("Failed to execute 'setLineDash' on '" + name + "': The provided value cannot be converted to a sequence.");
      N(SET_LINE_DASH, i, Array.from(segments, function (v) { return +v; }));
    });
    method(P, 'getLineDash', function getLineDash() { return N(GET_LINE_DASH, id(this)); });
    method(P, 'drawImage', function drawImage(image, a, b, c, d, e, f, gg, h) {
      var i = id(this), n = arguments.length;
      if (n < 3) throw tooFew(name, 'drawImage', 3, n);
      if (n !== 3 && n !== 5 && n !== 9) throw new TypeError("Failed to execute 'drawImage' on '" + name + "': Valid arities are: [3, 5, 9], but " + n + ' arguments provided.');
      var src = sourceOf(image, 'drawImage');
      a = +a; b = +b; c = +c; d = +d; e = +e; f = +f; gg = +gg; h = +h;
      var r = N(DRAW_IMAGE, i, src[0], src[1], a, b, c, d, e, f, gg, h, n);
      if (r === 'need') { upload(src); N(DRAW_IMAGE, i, src[0], src[1], a, b, c, d, e, f, gg, h, n); }
    });
    method(P, 'createImageData', function createImageData(a, b, settings) {
      var i = id(this), n = arguments.length;
      if (n < 1) throw tooFew(name, 'createImageData', 1, 0);
      if (a instanceof ImageData) return newImageData(new Uint8ClampedArray(a.width * a.height * 4), a.width, a.height, a.colorSpace);
      if (n < 2) throw new TypeError("Failed to execute 'createImageData' on '" + name + "': parameter 1 is not of type 'ImageData'.");
      var w = enforceLong(a), hh = enforceLong(b);
      if (w === 0 || hh === 0) throw domErr('IndexSizeError', "Failed to execute 'createImageData' on '" + name + "': The source " + (w === 0 ? 'width' : 'height') + ' is 0.');
      w = Math.abs(w); hh = Math.abs(hh);
      if (w * hh * 4 > 2147483647) throw new RangeError("Failed to execute 'createImageData' on '" + name + "': Out of memory at ImageData creation");
      var s = dictionary(settings, 'ImageDataSettings');
      var cs = s.colorSpace === undefined ? N(CONTEXT_ATTRS, i).colorSpace : str(s.colorSpace);
      var fmt = s.pixelFormat === undefined ? 'rgba-unorm8' : str(s.pixelFormat);
      var data = fmt === 'rgba-float16' && typeof g.Float16Array === 'function' ? new g.Float16Array(w * hh * 4) : new Uint8ClampedArray(w * hh * 4);
      return newImageData(data, w, hh, cs);
    });
    method(P, 'getImageData', function getImageData(sx, sy, sw, sh, settings) {
      var i = id(this), n = arguments.length;
      if (n < 4) throw tooFew(name, 'getImageData', 4, n);
      sx = enforceLong(sx); sy = enforceLong(sy); sw = enforceLong(sw); sh = enforceLong(sh);
      if (sw === 0 || sh === 0) throw domErr('IndexSizeError', "Failed to execute 'getImageData' on '" + name + "': The source " + (sw === 0 ? 'width' : 'height') + ' is 0.');
      var s = dictionary(settings, 'ImageDataSettings');
      var cs = s.colorSpace === undefined ? N(CONTEXT_ATTRS, i).colorSpace : str(s.colorSpace);
      if (SPACES.indexOf(cs) < 0) throw new TypeError("The provided value '" + cs + "' is not a valid enum value of type PredefinedColorSpace.");
      var data = N(GET_IMAGE_DATA, i, sx, sy, sw, sh, cs);
      var w = Math.abs(sw), hh = Math.abs(sh);
      if (s.pixelFormat === 'rgba-float16' && typeof g.Float16Array === 'function') {
        var f16 = new g.Float16Array(data.length);
        for (var k = 0; k < data.length; k++) f16[k] = data[k] / 255;
        return newImageData(f16, w, hh, cs);
      }
      return newImageData(data, w, hh, cs);
    });
    method(P, 'putImageData', function putImageData(img, dx, dy, x, y, w, h) {
      var i = id(this), n = arguments.length;
      if (n < 3) throw tooFew(name, 'putImageData', 3, n);
      if (n !== 3 && n !== 7) throw new TypeError("Failed to execute 'putImageData' on '" + name + "': Valid arities are: [3, 7], but " + n + ' arguments provided.');
      if (!(img instanceof ImageData) || !img.__id) throw new TypeError("Failed to execute 'putImageData' on '" + name + "': parameter 1 is not of type 'ImageData'.");
      dx = enforceLong(dx); dy = enforceLong(dy);
      var d = img.__id;
      var bytes = imageDataBytes(d);
      if (bytes.buffer && bytes.byteLength === 0 && d.width * d.height > 0) throw domErr('InvalidStateError', "Failed to execute 'putImageData' on '" + name + "': The source data has been detached.");
      if (n === 7) {
        x = enforceLong(x); y = enforceLong(y); w = enforceLong(w); h = enforceLong(h);
        N(PUT_IMAGE_DATA, i, bytes, d.width, d.height, dx, dy, x, y, w, h, d.cs);
      } else {
        N(PUT_IMAGE_DATA, i, bytes, d.width, d.height, dx, dy, undefined, undefined, undefined, undefined, d.cs);
      }
    });
    method(P, 'fillText', function fillText(text, x, y, maxWidth) {
      var i = id(this), n = arguments.length;
      if (n < 3) throw tooFew(name, 'fillText', 3, n);
      N(FILL_TEXT, i, str(text), +x, +y, n > 3 ? +maxWidth : undefined);
    });
    method(P, 'strokeText', function strokeText(text, x, y, maxWidth) {
      var i = id(this), n = arguments.length;
      if (n < 3) throw tooFew(name, 'strokeText', 3, n);
      N(STROKE_TEXT, i, str(text), +x, +y, n > 3 ? +maxWidth : undefined);
    });
    method(P, 'measureText', function measureText(text) {
      var i = id(this);
      if (arguments.length < 1) throw tooFew(name, 'measureText', 1, 0);
      text = str(text);
      var m = N(MEASURE_TEXT, i, text);
      var o = Object.create(TextMetrics.prototype);
      hidden(o, '__tm', m);
      // The drawing state the clusters are drawn with later.
      hidden(o, '__ti', {
        text: text, font: this.font, align: this.textAlign, baseline: this.textBaseline, direction: this.direction,
        letterSpacing: this.letterSpacing, wordSpacing: this.wordSpacing, fontKerning: this.fontKerning
      });
      return o;
    });
    method(P, 'fillTextCluster', function fillTextCluster(cluster, x, y, options) {
      id(this);
      if (arguments.length < 3) throw tooFew(name, 'fillTextCluster', 3, arguments.length);
      drawCluster(this, cluster, x, y, options, false, 'fillTextCluster');
    });
    method(P, 'strokeTextCluster', function strokeTextCluster(cluster, x, y, options) {
      id(this);
      if (arguments.length < 3) throw tooFew(name, 'strokeTextCluster', 3, arguments.length);
      drawCluster(this, cluster, x, y, options, true, 'strokeTextCluster');
    });
    method(P, 'drawFocusIfNeeded', function drawFocusIfNeeded(a, b) {
      id(this);
      var n = arguments.length;
      if (n < 1) throw tooFew(name, 'drawFocusIfNeeded', 1, 0);
      var el = a instanceof Path2D ? b : a;
      if (a instanceof Path2D && n < 2) throw tooFew(name, 'drawFocusIfNeeded', 2, n);
      if (!(g.Element && el instanceof g.Element)) throw new TypeError("Failed to execute 'drawFocusIfNeeded': parameter is not of type 'Element'.");
      // Draw the focus ring when the element is focused and is fallback
      // content of this canvas (HTML "draw focus if needed").
      var canvas = this.__canvas;
      if (!canvas || typeof canvas.contains !== 'function' || !canvas.contains(el) || canvas === el) return;
      if (!g.document || g.document.activeElement !== el) return;
      this.save();
      try {
        this.setLineDash([]);
        this.globalAlpha = 1;
        this.globalCompositeOperation = 'source-over';
        this.shadowColor = 'transparent';
        this.filter = 'none';
        this.lineWidth = 2;
        this.lineJoin = 'round';
        this.strokeStyle = '#101010';
        // The current path is already in device space; a Path2D follows
        // the current transform.
        if (a instanceof Path2D) {
          this.stroke(a);
        } else {
          this.resetTransform();
          this.stroke();
        }
      } finally {
        this.restore();
      }
    });
    method(P, 'beginLayer', function beginLayer(options) {
      var i = id(this);
      if (options !== undefined && options !== null && typeof options !== 'object' && typeof options !== 'function') {
        throw new TypeError("Failed to execute 'beginLayer' on '" + name + "': The provided value is not of type 'BeginLayerOptions'.");
      }
      var f = options === undefined || options === null ? undefined : options.filter;
      var text, spec;
      if (f instanceof CanvasFilter && f.__cf) spec = f.__cf;
      else if (f !== null && (typeof f === 'object' || typeof f === 'function')) spec = resolveFilter(f, i, "beginLayer' on '" + name);
      else if (f !== undefined && f !== null) text = str(f);
      N(BEGIN_LAYER, i, text, spec);
    });
    method(P, 'endLayer', function endLayer() { N(END_LAYER, id(this)); });
    return C;
  }

  var TextMetrics = illegalCtor('TextMetrics');
  tag(TextMetrics.prototype, 'TextMetrics');
  ['width', 'actualBoundingBoxLeft', 'actualBoundingBoxRight', 'fontBoundingBoxAscent', 'fontBoundingBoxDescent',
    'actualBoundingBoxAscent', 'actualBoundingBoxDescent', 'emHeightAscent', 'emHeightDescent', 'hangingBaseline',
    'alphabeticBaseline', 'ideographicBaseline'].forEach(function (k, i) {
    accessor(TextMetrics.prototype, k, function () { if (!this.__tm) throw illegal(); return this.__tm[i]; });
  });

  // ---- Text clusters (TextMetrics.getTextClusters, fillTextCluster) ------
  var TextCluster = illegalCtor('TextCluster');
  tag(TextCluster.prototype, 'TextCluster');
  ['x', 'y', 'start', 'end', 'align', 'baseline'].forEach(function (k) {
    accessor(TextCluster.prototype, k, function () { if (!this.__tc) throw illegal(); return this.__tc[k]; });
  });
  var ALIGNS = ['start', 'end', 'left', 'right', 'center'];
  var BASELINES = ['top', 'hanging', 'middle', 'alphabetic', 'ideographic', 'bottom'];
  function enumValue(v, list, what) {
    v = str(v);
    if (list.indexOf(v) < 0) throw new TypeError("The provided value '" + v + "' is not a valid enum value of type " + what + '.');
    return v;
  }
  // Characters that join the previous one into a grapheme cluster.
  function extends_(cp) {
    return (cp >= 0x300 && cp <= 0x36F) || (cp >= 0x1AB0 && cp <= 0x1AFF) || (cp >= 0x1DC0 && cp <= 0x1DFF)
      || (cp >= 0x20D0 && cp <= 0x20FF) || (cp >= 0xFE00 && cp <= 0xFE0F) || (cp >= 0xFE20 && cp <= 0xFE2F)
      || (cp >= 0x1F3FB && cp <= 0x1F3FF) || (cp >= 0xE0100 && cp <= 0xE01EF) || cp === 0x200D;
  }
  // [start, end, firstCodePointIndex, codePointCount] per cluster.
  function clustersOf(text) {
    var out = [], i = 0, cpi = 0, joinNext = false;
    while (i < text.length) {
      var cp = text.codePointAt(i), len = cp > 0xFFFF ? 2 : 1;
      var last = out[out.length - 1];
      if (last && (extends_(cp) || joinNext)) {
        last[1] = i + len;
        last[3]++;
      } else {
        out.push([i, i + len, cpi, 1]);
      }
      joinNext = cp === 0x200D;
      i += len;
      cpi++;
    }
    return out;
  }
  function baselineHeight(name, b) {
    switch (name) {
      case 'top': return b[0];
      case 'hanging': return b[2];
      case 'middle': return (b[0] - b[1]) / 2;
      case 'ideographic': return b[3];
      case 'bottom': return -b[1];
      default: return 0;
    }
  }
  method(TextMetrics.prototype, 'getTextClusters', function getTextClusters(a, b, c) {
    var m = this.__tm, info = this.__ti;
    if (!m || !info) throw illegal();
    var len = info.text.length, start = 0, end = len, options;
    if (a === undefined || a === null || typeof a === 'object') {
      options = a;
    } else {
      start = enforceULong(a);
      end = enforceULong(b);
      options = c;
      if (start >= len) throw domErr('IndexSizeError', "Failed to execute 'getTextClusters' on 'TextMetrics': The start index is out of range.");
      if (end > len) throw domErr('IndexSizeError', "Failed to execute 'getTextClusters' on 'TextMetrics': The end index is out of range.");
    }
    var o = dictionary(options, 'TextClusterOptions');
    var align = opt(o, 'align', info.align, function (v) { return enumValue(v, ALIGNS, 'CanvasTextAlign'); });
    var baseline = opt(o, 'baseline', info.baseline, function (v) { return enumValue(v, BASELINES, 'CanvasTextBaseline'); });
    var pens = m[14] || [], advances = m[15] || [], startX = m[12] || 0, heights = m[13] || [0, 0, 0, 0];
    var y = baselineHeight(info.baseline, heights) - baselineHeight(baseline, heights);
    var rtl = info.direction === 'rtl';
    var out = [];
    var all = clustersOf(info.text);
    for (var k = 0; k < all.length; k++) {
      var cl = all[k];
      if (cl[1] <= start || cl[0] >= end) continue;
      var x0 = pens[cl[2]] === undefined ? 0 : pens[cl[2]];
      // The cluster's own advance (kerning towards the next cluster is not
      // part of it), so right/center-aligned clusters line up with fillText.
      var w = 0;
      for (var ci = cl[2]; ci < cl[2] + cl[3]; ci++) w += advances[ci] || 0;
      var left = startX + x0;
      var a2 = align === 'start' ? (rtl ? 'right' : 'left') : align === 'end' ? (rtl ? 'left' : 'right') : align;
      var x = a2 === 'left' ? left : a2 === 'right' ? left + w : left + w / 2;
      var t = Object.create(TextCluster.prototype);
      hidden(t, '__tc', { x: x, y: y, start: cl[0], end: cl[1], align: align, baseline: baseline, info: info });
      out.push(t);
    }
    return out;
  });
  function drawCluster(ctx, cluster, x, y, options, stroke, method) {
    if (!(cluster instanceof TextCluster) || !cluster.__tc) throw new TypeError("Failed to execute '" + method + "': parameter 1 is not of type 'TextCluster'.");
    var c = cluster.__tc, info = c.info;
    var o = dictionary(options, 'TextClusterOptions');
    var align = opt(o, 'align', c.align, function (v) { return enumValue(v, ALIGNS, 'CanvasTextAlign'); });
    var baseline = opt(o, 'baseline', c.baseline, function (v) { return enumValue(v, BASELINES, 'CanvasTextBaseline'); });
    var cx = opt(o, 'x', c.x, Number), cy = opt(o, 'y', c.y, Number);
    ctx.save();
    try {
      ctx.font = info.font;
      ctx.direction = info.direction;
      ctx.letterSpacing = info.letterSpacing;
      ctx.wordSpacing = info.wordSpacing;
      ctx.fontKerning = info.fontKerning;
      ctx.textAlign = align;
      ctx.textBaseline = baseline;
      var text = info.text.substring(c.start, c.end);
      if (stroke) ctx.strokeText(text, +x + cx, +y + cy); else ctx.fillText(text, +x + cx, +y + cy);
    } finally {
      ctx.restore();
    }
  }

  var CanvasRenderingContext2D = makeContextProto('CanvasRenderingContext2D');
  var OffscreenCanvasRenderingContext2D = makeContextProto('OffscreenCanvasRenderingContext2D');

  function makeContext(C, surfaceId, canvas) {
    var c = Object.create(C.prototype);
    hidden(c, '__c2d', surfaceId);
    hidden(c, '__canvas', canvas);
    hidden(c, '__styles', new Map());
    return c;
  }
  function contextSettings(options) {
    // getContext(): "If options is not an object, set options to null".
    var o = options !== null && (typeof options === 'object' || typeof options === 'function') ? options : {};
    // Dictionary members are read once each, in lexicographic order.
    var v = o.alpha;
    var alpha = v === undefined ? true : !!v;
    v = o.colorSpace;
    var cs = v === undefined ? 'srgb' : str(v);
    if (SPACES.indexOf(cs) < 0) throw new TypeError("The provided value '" + cs + "' is not a valid enum value of type PredefinedColorSpace.");
    v = o.colorType;
    var ct = v === undefined ? 'unorm8' : str(v);
    if (ct !== 'unorm8' && ct !== 'float16') throw new TypeError("The provided value '" + ct + "' is not a valid enum value of type CanvasColorType.");
    v = o.desynchronized;
    var des = v === undefined ? false : !!v;
    // CanvasToneMapping {mode: 'standard' | 'extended'}.
    v = o.toneMapping;
    var tm = 'standard';
    if (v !== undefined && v !== null) {
      if (typeof v !== 'object' && typeof v !== 'function') throw new TypeError("The provided value is not of type 'CanvasToneMapping'.");
      var mode = v.mode;
      if (mode !== undefined) {
        tm = str(mode);
        if (tm !== 'standard' && tm !== 'extended') throw new TypeError("The provided value '" + tm + "' is not a valid enum value of type CanvasToneMappingMode.");
      }
    }
    v = o.willReadFrequently;
    var wrf = v === undefined ? false : !!v;
    return [alpha, cs, wrf, des, ct, tm];
  }

  // ---- ImageBitmap -------------------------------------------------------
  var ImageBitmap = illegalCtor('ImageBitmap');
  tag(ImageBitmap.prototype, 'ImageBitmap');
  function makeBitmap(w, h, data) {
    var b = Object.create(ImageBitmap.prototype);
    hidden(b, '__bm', { tok: ++tokens, w: w, h: h, data: data, closed: false });
    return b;
  }
  accessor(ImageBitmap.prototype, 'width', function () { var b = this.__bm; if (!b) throw illegal(); return b.closed ? 0 : b.w; });
  accessor(ImageBitmap.prototype, 'height', function () { var b = this.__bm; if (!b) throw illegal(); return b.closed ? 0 : b.h; });
  method(ImageBitmap.prototype, 'close', function close() { var b = this.__bm; if (!b) throw illegal(); b.closed = true; b.data = null; });

  // ---- ImageBitmapRenderingContext ---------------------------------------
  var ImageBitmapRenderingContext = illegalCtor('ImageBitmapRenderingContext');
  tag(ImageBitmapRenderingContext.prototype, 'ImageBitmapRenderingContext');
  accessor(ImageBitmapRenderingContext.prototype, 'canvas', function () { if (!this.__ibr) throw illegal(); return this.__ibr.canvas; });
  method(ImageBitmapRenderingContext.prototype, 'transferFromImageBitmap', function transferFromImageBitmap(bitmap) {
    var s = this.__ibr;
    if (!s) throw illegal();
    if (arguments.length < 1) throw tooFew('ImageBitmapRenderingContext', 'transferFromImageBitmap', 1, 0);
    if (bitmap === null) { if (s.id !== undefined) N(RESIZE, s.id, N(GET, s.id, 'width'), N(GET, s.id, 'height')); return; }
    if (!(bitmap instanceof ImageBitmap)) throw new TypeError("Failed to execute 'transferFromImageBitmap': parameter 1 is not of type 'ImageBitmap'.");
    var b = bitmap.__bm;
    if (b.closed) throw domErr('InvalidStateError', "Failed to execute 'transferFromImageBitmap': The input ImageBitmap has been detached.");
    if (s.id !== undefined) {
      var w = N(GET, s.id, 'width'), h = N(GET, s.id, 'height');
      N(RESIZE, s.id, w, h);
      N(UPLOAD_BITMAP, b.tok, b.w, b.h, b.data);
      N(SET, s.id, 'globalCompositeOperation', 'copy');
      N(DRAW_IMAGE, s.id, 2, b.tok, 0, 0, w, h, NaN, NaN, NaN, NaN, 5);
      N(SET, s.id, 'globalCompositeOperation', 'source-over');
    }
    b.closed = true;
    b.data = null;
  });

  // ---- OffscreenCanvas ---------------------------------------------------
  function OffscreenCanvas(width, height) {
    if (!new.target) throw new TypeError("Failed to construct 'OffscreenCanvas': Please use the 'new' operator.");
    if (arguments.length < 2) throw new TypeError("Failed to construct 'OffscreenCanvas': 2 arguments required, but only " + arguments.length + ' present.');
    width = enforceULongLong(width);
    height = enforceULongLong(height);
    hidden(this, '__oc', { id: N(CREATE_OFFSCREEN, width, height), w: width, h: height, mode: null, ctx: null, detached: false });
  }
  ctor('OffscreenCanvas', OffscreenCanvas);
  if (g.EventTarget && g.EventTarget.prototype) {
    try { Object.setPrototypeOf(OffscreenCanvas.prototype, g.EventTarget.prototype); } catch (e) {}
  }
  tag(OffscreenCanvas.prototype, 'OffscreenCanvas');
  function oc(o) { var s = o.__oc; if (!s) throw illegal(); return s; }
  ['width', 'height'].forEach(function (k) {
    accessor(OffscreenCanvas.prototype, k, function () { var s = oc(this); return k === 'width' ? s.w : s.h; }, function (v) {
      var s = oc(this);
      v = enforceULongLong(v);
      if (s.detached) throw domErr('InvalidStateError', 'The OffscreenCanvas is detached.');
      if (k === 'width') s.w = v; else s.h = v;
      N(RESIZE, s.id, s.w, s.h);
    });
  });
  method(OffscreenCanvas.prototype, 'getContext', function getContext(type, options) {
    var s = oc(this);
    if (arguments.length < 1) throw tooFew('OffscreenCanvas', 'getContext', 1, 0);
    type = str(type);
    if (['2d', 'bitmaprenderer', 'webgl', 'webgl2', 'webgpu'].indexOf(type) < 0) throw new TypeError("Failed to execute 'getContext' on 'OffscreenCanvas': The provided value '" + type + "' is not a valid enum value of type OffscreenRenderingContextId.");
    if (s.detached) throw domErr('InvalidStateError', "Failed to execute 'getContext' on 'OffscreenCanvas': OffscreenCanvas object is detached.");
    var a = type === '2d' ? contextSettings(options) : null;
    if (s.mode !== null) return s.mode === type ? s.ctx : null;
    if (type === '2d') {
      N(GET_CONTEXT, s.id, a[0], a[1], a[2], a[3], a[4], a[5]);
      s.ctx = makeContext(OffscreenCanvasRenderingContext2D, s.id, this);
    } else if (type === 'bitmaprenderer') {
      dictionary(options, 'ImageBitmapRenderingContextSettings');
      s.ctx = Object.create(ImageBitmapRenderingContext.prototype);
      hidden(s.ctx, '__ibr', { canvas: this, id: s.id });
    } else {
      return null;
    }
    s.mode = type;
    return s.ctx;
  });
  method(OffscreenCanvas.prototype, 'transferToImageBitmap', function transferToImageBitmap() {
    var s = oc(this);
    if (s.detached) throw domErr('InvalidStateError', "Failed to execute 'transferToImageBitmap' on 'OffscreenCanvas': Cannot transfer an ImageBitmap from a detached OffscreenCanvas");
    if (s.mode === null) throw domErr('InvalidStateError', "Failed to execute 'transferToImageBitmap' on 'OffscreenCanvas': Cannot transfer an ImageBitmap from an OffscreenCanvas with no context");
    if (s.w === 0 || s.h === 0) throw domErr('InvalidStateError', "Failed to execute 'transferToImageBitmap' on 'OffscreenCanvas': The canvas has a width or height of 0.");
    var shot = N(SNAPSHOT, 1, s.id, NaN, NaN, NaN, NaN, true);
    // The canvas gets a fresh (transparent) bitmap, the context keeps its state.
    N(SAVE, s.id);
    N(SET_TRANSFORM, s.id, 1, 0, 0, 1, 0, 0);
    N(CLEAR_RECT, s.id, 0, 0, s.w, s.h);
    N(RESTORE, s.id);
    return makeBitmap(shot[0], shot[1], shot[2]);
  });
  function encodeToBlob(kind, ref, type, quality) {
    var r = N(ENCODE, kind, ref, type === undefined ? 'image/png' : str(type), quality);
    if (r === null) return null;
    return new g.Blob([r[1]], { type: r[0] });
  }
  method(OffscreenCanvas.prototype, 'convertToBlob', function convertToBlob(options) {
    var self = this;
    return new Promise(function (resolve, reject) {
      var s = oc(self);
      var o = dictionary(options, 'ImageEncodeOptions');
      if (s.detached) throw domErr('InvalidStateError', "Failed to execute 'convertToBlob' on 'OffscreenCanvas': A detached OffscreenCanvas cannot be converted to a Blob.");
      if (s.w === 0 || s.h === 0) throw domErr('IndexSizeError', "Failed to execute 'convertToBlob' on 'OffscreenCanvas': The size of the OffscreenCanvas is zero.");
      resolve(encodeToBlob(1, s.id, o.type, opt(o, 'quality', NaN, Number)));
    });
  });

  // ---- HTMLCanvasElement -------------------------------------------------
  var elementState = new WeakMap();
  function elState(el) {
    var s = elementState.get(el);
    if (!s) { s = { mode: null, ctx: null, placeholder: false }; elementState.set(el, s); }
    return s;
  }
  var HCE = g.HTMLCanvasElement && g.HTMLCanvasElement.prototype;
  if (HCE) {
    method(HCE, 'getContext', function getContext(type, options) {
      if (!(this instanceof g.HTMLCanvasElement)) throw illegal();
      if (arguments.length < 1) throw tooFew('HTMLCanvasElement', 'getContext', 1, 0);
      type = str(type);
      var s = elState(this);
      if (s.placeholder) throw domErr('InvalidStateError', "Failed to execute 'getContext' on 'HTMLCanvasElement': Cannot get context from a canvas that has transferred its control to offscreen.");
      if (type === '2d') {
        // The options are converted on every call (WebIDL dictionary
        // conversion order is observable), also when the context exists.
        var a = contextSettings(options);
        if (s.mode === '2d') return s.ctx;
        if (s.mode !== null) return null;
        var sid = N(GET_CONTEXT, this, a[0], a[1], a[2], a[3], a[4], a[5]);
        s.ctx = makeContext(CanvasRenderingContext2D, sid, this);
        s.mode = '2d';
        return s.ctx;
      }
      if (type === 'bitmaprenderer') {
        if (s.mode === 'bitmaprenderer') return s.ctx;
        if (s.mode !== null) return null;
        dictionary(options, 'ImageBitmapRenderingContextSettings');
        var bid = N(GET_CONTEXT, this, true, 'srgb', false, false, 'unorm8');
        s.ctx = Object.create(ImageBitmapRenderingContext.prototype);
        hidden(s.ctx, '__ibr', { canvas: this, id: bid });
        s.mode = 'bitmaprenderer';
        return s.ctx;
      }
      return null;
    });
    method(HCE, 'toDataURL', function toDataURL(type, quality) {
      if (!(this instanceof g.HTMLCanvasElement)) throw illegal();
      if (elState(this).placeholder) throw domErr('InvalidStateError', "Failed to execute 'toDataURL' on 'HTMLCanvasElement': Cannot read from a canvas that has transferred its control to offscreen.");
      return N(TO_DATA_URL, 0, this, type === undefined ? 'image/png' : str(type), typeof quality === 'number' ? quality : NaN);
    });
    method(HCE, 'toBlob', function toBlob(callback, type, quality) {
      if (!(this instanceof g.HTMLCanvasElement)) throw illegal();
      if (arguments.length < 1) throw tooFew('HTMLCanvasElement', 'toBlob', 1, 0);
      if (typeof callback !== 'function') throw new TypeError("Failed to execute 'toBlob' on 'HTMLCanvasElement': The callback provided as parameter 1 is not a function.");
      if (elState(this).placeholder) throw domErr('InvalidStateError', "Failed to execute 'toBlob' on 'HTMLCanvasElement': Cannot read from a canvas that has transferred its control to offscreen.");
      var blob = encodeToBlob(0, this, type, typeof quality === 'number' ? quality : NaN);
      setTimeout(function () { callback(blob); }, 0);
    });
    method(HCE, 'transferControlToOffscreen', function transferControlToOffscreen() {
      if (!(this instanceof g.HTMLCanvasElement)) throw illegal();
      var s = elState(this);
      if (s.mode !== null) throw domErr('InvalidStateError', "Failed to execute 'transferControlToOffscreen' on 'HTMLCanvasElement': Cannot transfer control from a canvas that has a rendering context.");
      if (s.placeholder) throw domErr('InvalidStateError', "Failed to execute 'transferControlToOffscreen' on 'HTMLCanvasElement': Cannot transfer control from a canvas for more than one time.");
      s.placeholder = true;
      var oc = new OffscreenCanvas(this.width, this.height);
      // The placeholder shows what the OffscreenCanvas draws.
      N(LINK_PLACEHOLDER, oc.__oc.id, this);
      return oc;
    });
  }

  // ---- createImageBitmap ---------------------------------------------------
  function bitmapOptions(o) {
    o = dictionary(o, 'ImageBitmapOptions');
    var r = {};
    // Members in lexicographic order, each read once.
    var csc = opt(o, 'colorSpaceConversion', 'default', function (v) { return str(v); });
    if (['none', 'default'].indexOf(csc) < 0) throw new TypeError("The provided value '" + csc + "' is not a valid enum value of type ColorSpaceConversion.");
    var io = opt(o, 'imageOrientation', 'from-image', function (v) { return str(v); });
    if (['from-image', 'flipY', 'none'].indexOf(io) < 0) throw new TypeError("The provided value '" + io + "' is not a valid enum value of type ImageOrientation.");
    r.flipY = io === 'flipY';
    var pa = opt(o, 'premultiplyAlpha', 'default', function (v) { return str(v); });
    if (['none', 'premultiply', 'default'].indexOf(pa) < 0) throw new TypeError("The provided value '" + pa + "' is not a valid enum value of type PremultiplyAlpha.");
    r.rh = opt(o, 'resizeHeight', undefined, function (v) { return enforceULong(v); });
    var rq = opt(o, 'resizeQuality', 'low', function (v) { return str(v); });
    if (['pixelated', 'low', 'medium', 'high'].indexOf(rq) < 0) throw new TypeError("The provided value '" + rq + "' is not a valid enum value of type ResizeQuality.");
    r.rw = opt(o, 'resizeWidth', undefined, function (v) { return enforceULong(v); });
    r.rq = rq;
    if (r.rw === 0 || r.rh === 0) throw domErr('InvalidStateError', "Failed to execute 'createImageBitmap': The resize width or height is 0.");
    return r;
  }
  // Scales/flips premultiplied bytes through a scratch surface.
  function finishBitmap(w, h, data, opts) {
    var tw = opts.rw !== undefined ? opts.rw : (opts.rh !== undefined ? Math.max(1, Math.round(w * opts.rh / h)) : w);
    var th = opts.rh !== undefined ? opts.rh : (opts.rw !== undefined ? Math.max(1, Math.round(h * opts.rw / w)) : h);
    if (tw * th > 16777216) throw domErr('InvalidStateError', "Failed to execute 'createImageBitmap': The ImageBitmap is too large.");
    if (tw === w && th === h && !opts.flipY) return makeBitmap(w, h, data);
    var tmp = makeBitmap(w, h, data);
    var c = new OffscreenCanvas(tw, th), ctx = c.getContext('2d');
    if (opts.rq === 'pixelated') ctx.imageSmoothingEnabled = false;
    if (opts.flipY) { ctx.translate(0, th); ctx.scale(1, -1); }
    ctx.drawImage(tmp, 0, 0, tw, th);
    var shot = N(SNAPSHOT, 1, c.__oc.id, NaN, NaN, NaN, NaN, true);
    return makeBitmap(shot[0], shot[1], shot[2]);
  }
  function createImageBitmap(image, a, b, c, d, e) {
    var n = arguments.length;
    return new Promise(function (resolve) {
      if (n < 1) throw tooFew('Window', 'createImageBitmap', 1, 0);
      var sx, sy, sw, sh, opts;
      if (n >= 5) {
        // WebIDL `long` without [EnforceRange]: wraps modulo 2^32.
        sx = a | 0; sy = b | 0; sw = c | 0; sh = d | 0;
        if (sw === 0 || sh === 0) throw new RangeError("Failed to execute 'createImageBitmap' on 'Window': The crop rect " + (sw === 0 ? 'width' : 'height') + ' is 0.');
        opts = bitmapOptions(e);
      } else if (n === 2 || n === 3 || n === 4) {
        if (n > 2) throw new TypeError("Failed to execute 'createImageBitmap' on 'Window': Valid arities are: [1, 2, 5, 6], but " + n + ' arguments provided.');
        opts = bitmapOptions(a);
      } else {
        opts = bitmapOptions(undefined);
      }
      var crop = sx === undefined ? [NaN, NaN, NaN, NaN] : [sx, sy, sw, sh];
      if (image instanceof ImageData && image.__id) {
        var id = image.__id;
        var tmp = new OffscreenCanvas(id.width, id.height);
        tmp.getContext('2d').putImageData(image, 0, 0);
        var s1 = N(SNAPSHOT, 1, tmp.__oc.id, crop[0], crop[1], crop[2], crop[3], true);
        resolve(finishBitmap(s1[0], s1[1], s1[2], opts));
        return;
      }
      // Blobs, also those made by other parts of the engine (XHR/fetch
      // responses) that are not instances of this realm's Blob.
      var blobLike = image !== null && typeof image === 'object' && !(image instanceof ImageBitmap)
        && typeof image.arrayBuffer === 'function' && typeof image.size === 'number' && typeof image.type === 'string';
      if ((g.Blob && image instanceof g.Blob) || blobLike) {
        resolve(image.arrayBuffer().then(function (buf) {
          var dec = N(DECODE, new Uint8Array(buf));
          if (dec === null) throw domErr('InvalidStateError', "Failed to execute 'createImageBitmap' on 'Window': The source image could not be decoded.");
          var t = makeBitmap(dec[0], dec[1], dec[2]);
          if (sx === undefined) return finishBitmap(dec[0], dec[1], dec[2], opts);
          N(UPLOAD_BITMAP, t.__bm.tok, dec[0], dec[1], dec[2]);
          var s2 = N(SNAPSHOT, 2, t.__bm.tok, crop[0], crop[1], crop[2], crop[3], true);
          return finishBitmap(s2[0], s2[1], s2[2], opts);
        }));
        return;
      }
      var src = sourceOf(image, 'createImageBitmap');
      if (src[0] === 0 && g.HTMLImageElement && image instanceof g.HTMLImageElement && N(SOURCE_INFO, 0, image) === null) {
        throw domErr('InvalidStateError', "Failed to execute 'createImageBitmap' on 'Window': The image element contains no image data.");
      }
      if (src[0] === 0 && g.HTMLVideoElement && image instanceof g.HTMLVideoElement) {
        throw domErr('InvalidStateError', "Failed to execute 'createImageBitmap' on 'Window': The video element has not loaded data.");
      }
      var shot = N(SNAPSHOT, src[0], src[1], crop[0], crop[1], crop[2], crop[3], true);
      if (shot === 'need') { upload(src); shot = N(SNAPSHOT, src[0], src[1], crop[0], crop[1], crop[2], crop[3], true); }
      if (shot === null) throw domErr('InvalidStateError', "Failed to execute 'createImageBitmap' on 'Window': The source image could not be decoded.");
      resolve(finishBitmap(shot[0], shot[1], shot[2], opts));
    });
  }
  ctor('createImageBitmap', createImageBitmap);

  [DOMPointReadOnly, DOMPoint, DOMMatrixReadOnly, DOMMatrix, ImageData, CanvasGradient, CanvasPattern, Path2D, TextMetrics,
    CanvasRenderingContext2D, OffscreenCanvasRenderingContext2D, OffscreenCanvas, ImageBitmap, ImageBitmapRenderingContext, TextCluster,
    CanvasFilter].forEach(lockPrototype);
  expose('DOMPointReadOnly', DOMPointReadOnly);
  expose('DOMPoint', DOMPoint);
  expose('DOMMatrixReadOnly', DOMMatrixReadOnly);
  expose('DOMMatrix', DOMMatrix);
  expose('WebKitCSSMatrix', DOMMatrix);
  expose('ImageData', ImageData);
  expose('CanvasGradient', CanvasGradient);
  expose('CanvasPattern', CanvasPattern);
  expose('Path2D', Path2D);
  expose('TextMetrics', TextMetrics);
  expose('TextCluster', TextCluster);
  expose('CanvasFilter', CanvasFilter);
  expose('CanvasRenderingContext2D', CanvasRenderingContext2D);
  expose('OffscreenCanvasRenderingContext2D', OffscreenCanvasRenderingContext2D);
  expose('OffscreenCanvas', OffscreenCanvas);
  expose('ImageBitmap', ImageBitmap);
  expose('ImageBitmapRenderingContext', ImageBitmapRenderingContext);
  expose('createImageBitmap', createImageBitmap);
})();
