// WebAssembly JS API (https://webassembly.github.io/spec/js-api/) and the
// streaming Web API (https://webassembly.github.io/spec/web-api/).
//
// Loaded lazily on the first use of `WebAssembly` (web_platform.rs). `N` are
// the natives of web_wasm.rs; every store object (module, memory, table,
// global, tag, function) is a number there. This file is the WebIDL surface:
// argument conversion, brand checks, descriptors, identity caches, promises.
(function (N, g) {
  'use strict';
  var defineProperty = Object.defineProperty;
  var create = Object.create;
  var freeze = Object.freeze;
  var PromiseCtor = Promise;
  var TypeErrorCtor = TypeError;
  var RangeErrorCtor = RangeError;

  function op(o, name, fn, length) {
    // WebIDL operation: writable, enumerable, configurable.
    defineProperty(o, name, { value: fn, writable: true, enumerable: true, configurable: true });
    if (length !== undefined) defineProperty(fn, 'length', { value: length, configurable: true });
    defineProperty(fn, 'name', { value: name, configurable: true });
  }
  function attr(o, name, get, set) {
    // WebIDL attribute: accessor, enumerable, configurable.
    var d = { get: get, enumerable: true, configurable: true };
    defineProperty(get, 'name', { value: 'get ' + name, configurable: true });
    defineProperty(get, 'length', { value: 0, configurable: true });
    if (set) {
      defineProperty(set, 'name', { value: 'set ' + name, configurable: true });
      defineProperty(set, 'length', { value: 1, configurable: true });
      d.set = set;
    }
    // Delete first: redefining an existing accessor keeps its old flags.
    delete o[name];
    defineProperty(o, name, d);
  }
  function tag(o, text) {
    defineProperty(o, Symbol.toStringTag, { value: text, writable: false, enumerable: false, configurable: true });
  }
  function iface(ctor, protoTag) {
    // Interface object: `prototype` read-only, `constructor` hidden.
    var p = ctor.prototype;
    defineProperty(ctor, 'prototype', { writable: false, enumerable: false, configurable: false });
    defineProperty(p, 'constructor', { value: ctor, writable: true, enumerable: false, configurable: true });
    if (protoTag) tag(p, protoTag);
  }
  function isObject(v) {
    return (typeof v === 'object' && v !== null) || typeof v === 'function';
  }
  // The `address` member of memory and table descriptors (memory64):
  // `true` for "i64", whose sizes are BigInts.
  function addressType(v, what) {
    if (v === undefined) return false;
    var s = String(v);
    if (s === 'i32') return false;
    if (s === 'i64') return true;
    throw new TypeErrorCtor(what + ': unknown address type ' + s);
  }
  function addressValue(v, is64, what) {
    if (!is64) return enforceU32(v, what);
    if (typeof v !== 'bigint') throw new TypeErrorCtor(what + ' must be a BigInt for i64 addresses');
    var b = v;
    if (b < 0n || b > 18446744073709551615n) throw new TypeErrorCtor(what + ' is out of range');
    return Number(b);
  }
  // WebIDL [EnforceRange] unsigned long.
  function enforceU32(v, what) {
    var n = +v;
    if (n !== n || n === Infinity || n === -Infinity) throw new TypeErrorCtor(what + ' must be a finite number');
    n = n < 0 ? Math.ceil(n) : Math.floor(n);
    if (n < 0 || n > 4294967295) throw new TypeErrorCtor(what + ' is out of range');
    return n;
  }

  // ---- Error classes ---------------------------------------------------
  class CompileError extends Error {
    constructor(message) { super(message, arguments[1]); }
  }
  class LinkError extends Error {
    constructor(message) { super(message, arguments[1]); }
  }
  class RuntimeError extends Error {
    constructor(message) { super(message, arguments[1]); }
  }
  [CompileError, LinkError, RuntimeError].forEach(function (E) {
    defineProperty(E.prototype, 'name', { value: E.name, writable: true, enumerable: false, configurable: true });
    defineProperty(E.prototype, 'message', { value: '', writable: true, enumerable: false, configurable: true });
    iface(E);
  });
  N.init(CompileError.prototype, LinkError.prototype, RuntimeError.prototype);

  // ---- Brand slots -----------------------------------------------------
  var moduleSlot = new WeakMap();
  var instanceSlot = new WeakMap();
  var memorySlot = new WeakMap();
  var tableSlot = new WeakMap();
  var globalSlot = new WeakMap();
  var tagSlot = new WeakMap();
  // Identity caches: one JS object per store address.
  var memoryObjects = new Map();
  var tableObjects = new Map();
  var globalObjects = new Map();
  var tagObjects = new Map();

  function needNew(nt, what) {
    if (nt === undefined) throw new TypeErrorCtor("Failed to construct 'WebAssembly." + what + "': Please use the 'new' operator");
  }
  function brand(slot, o, what) {
    var v = isObject(o) ? slot.get(o) : undefined;
    if (v === undefined) throw new TypeErrorCtor('Illegal invocation: not a WebAssembly.' + what);
    return v;
  }

  var KINDS = ['function', 'table', 'memory', 'global', 'tag'];
  var VALTYPES = { i32: 0, i64: 1, f32: 2, f64: 3, anyfunc: 4, funcref: 4, externref: 5, v128: 6, exnref: 7 };
  var VALNAMES = ['i32', 'i64', 'f32', 'f64', 'funcref', 'externref', 'v128', 'exnref'];
  function valtype(v, what) {
    var s = String(v);
    var c = VALTYPES[s];
    if (c === undefined || !Object.prototype.hasOwnProperty.call(VALTYPES, s)) {
      throw new TypeErrorCtor(what + ': unknown value type ' + s);
    }
    return c;
  }

  // ---- Module ----------------------------------------------------------
  class Module {
    constructor(bytes) {
      needNew(new.target, 'Module');
      if (arguments.length < 1) throw new TypeErrorCtor('WebAssembly.Module requires 1 argument');
      var id = N.compile(bytes);
      moduleSlot.set(this, id);
    }
    static exports(moduleObject) {
      var id = brand(moduleSlot, moduleObject, 'Module');
      var flat = N.moduleExports(id);
      var out = [];
      for (var i = 0; i < flat.length; i += 2) out.push({ name: flat[i], kind: KINDS[flat[i + 1]] });
      return out;
    }
    static imports(moduleObject) {
      var id = brand(moduleSlot, moduleObject, 'Module');
      var flat = N.moduleImports(id);
      var out = [];
      for (var i = 0; i < flat.length; i += 3) out.push({ module: flat[i], name: flat[i + 1], kind: KINDS[flat[i + 2]] });
      return out;
    }
    static customSections(moduleObject, sectionName) {
      var id = brand(moduleSlot, moduleObject, 'Module');
      if (arguments.length < 2) throw new TypeErrorCtor('customSections requires 2 arguments');
      return N.customSections(id, String(sectionName));
    }
  }
  iface(Module, 'WebAssembly.Module');
  op(Module, 'exports', Module.exports, 1);
  op(Module, 'imports', Module.imports, 1);
  op(Module, 'customSections', Module.customSections, 2);

  // ---- Instance --------------------------------------------------------
  function wrapMemory(idx) {
    var o = memoryObjects.get(idx);
    if (!o) {
      o = create(Memory.prototype);
      memorySlot.set(o, idx);
      memoryObjects.set(idx, o);
    }
    return o;
  }
  function wrapTable(idx) {
    var o = tableObjects.get(idx);
    if (!o) {
      o = create(Table.prototype);
      tableSlot.set(o, idx);
      tableObjects.set(idx, o);
    }
    return o;
  }
  function wrapGlobal(idx) {
    var o = globalObjects.get(idx);
    if (!o) {
      o = create(Global.prototype);
      globalSlot.set(o, idx);
      globalObjects.set(idx, o);
    }
    return o;
  }
  function wrapTag(idx) {
    var o = tagObjects.get(idx);
    if (!o) {
      o = create(Tag.prototype);
      tagSlot.set(o, idx);
      tagObjects.set(idx, o);
    }
    return o;
  }

  // "read the imports": the import values in module order, then the
  // natives link them. `vals[i]` is the JS value (functions, plain global
  // values) or the store index of a WebAssembly object; `objs[i]` says which.
  function readImports(id, importObject) {
    var flat = N.moduleImports(id);
    if (flat.length > 0 && importObject === undefined) {
      throw new TypeErrorCtor('WebAssembly.Instance: imports need an import object');
    }
    var vals = [];
    var objs = [];
    for (var i = 0; i < flat.length; i += 3) {
      var moduleName = flat[i];
      var name = flat[i + 1];
      var kind = flat[i + 2];
      var o = importObject[moduleName];
      if (!isObject(o)) throw new TypeErrorCtor('WebAssembly.Instance: import module "' + moduleName + '" is not an object');
      var v = o[name];
      var label = 'import ' + moduleName + '.' + name;
      if (kind === 0) {
        if (typeof v !== 'function') throw new LinkError(label + ' must be callable');
        vals.push(v); objs.push(false);
      } else if (kind === 1) {
        var t = isObject(v) ? tableSlot.get(v) : undefined;
        if (t === undefined) throw new LinkError(label + ' must be a WebAssembly.Table');
        vals.push(t); objs.push(true);
      } else if (kind === 2) {
        var m = isObject(v) ? memorySlot.get(v) : undefined;
        if (m === undefined) throw new LinkError(label + ' must be a WebAssembly.Memory');
        vals.push(m); objs.push(true);
      } else if (kind === 3) {
        var gl = isObject(v) ? globalSlot.get(v) : undefined;
        if (gl !== undefined) { vals.push(gl); objs.push(true); }
        else { vals.push(v); objs.push(false); }
      } else {
        var tg = isObject(v) ? tagSlot.get(v) : undefined;
        if (tg === undefined) throw new LinkError(label + ' must be a WebAssembly.Tag');
        vals.push(tg); objs.push(true);
      }
    }
    return [vals, objs];
  }

  function buildExports(flat) {
    var exports = create(null);
    for (var i = 0; i < flat.length; i += 3) {
      var name = flat[i];
      var kind = flat[i + 1];
      var idx = flat[i + 2];
      var value;
      if (kind === 0) value = N.func(idx);
      else if (kind === 1) value = wrapTable(idx);
      else if (kind === 2) value = wrapMemory(idx);
      else if (kind === 3) value = wrapGlobal(idx);
      else value = wrapTag(idx);
      defineProperty(exports, name, { value: value, writable: false, enumerable: true, configurable: false });
    }
    return Object.preventExtensions(exports);
  }

  function instantiateCore(moduleObject, importObject) {
    var id = brand(moduleSlot, moduleObject, 'Module');
    if (importObject !== undefined && !isObject(importObject)) {
      throw new TypeErrorCtor('WebAssembly.Instance: the import object must be an object');
    }
    var r = readImports(id, importObject);
    return N.instantiate(id, r[0], r[1]);
  }

  class Instance {
    constructor(module) {
      needNew(new.target, 'Instance');
      var flat = instantiateCore(module, arguments[1]);
      instanceSlot.set(this, buildExports(flat));
    }
    get exports() {
      return brand(instanceSlot, this, 'Instance');
    }
  }
  iface(Instance, 'WebAssembly.Instance');
  attr(Instance.prototype, 'exports', Object.getOwnPropertyDescriptor(Instance.prototype, 'exports').get);
  function newInstance(flat) {
    var o = create(Instance.prototype);
    instanceSlot.set(o, buildExports(flat));
    return o;
  }

  // ---- Memory ----------------------------------------------------------
  class Memory {
    constructor(descriptor) {
      needNew(new.target, 'Memory');
      if (!isObject(descriptor)) throw new TypeErrorCtor('WebAssembly.Memory: the descriptor must be an object');
      // Dictionary members in lexicographic order (WebIDL).
      var is64 = addressType(descriptor.address, 'WebAssembly.Memory');
      var initial = descriptor.initial;
      if (initial === undefined) throw new TypeErrorCtor('WebAssembly.Memory: initial is required');
      initial = addressValue(initial, is64, 'initial');
      var maximum = descriptor.maximum;
      if (maximum !== undefined) maximum = addressValue(maximum, is64, 'maximum');
      var shared = !!descriptor.shared;
      if (initial > 65536) throw new RangeErrorCtor('WebAssembly.Memory: initial is above 65536 pages');
      if (maximum !== undefined) {
        if (maximum > 65536) throw new RangeErrorCtor('WebAssembly.Memory: maximum is above 65536 pages');
        if (maximum < initial) throw new RangeErrorCtor('WebAssembly.Memory: maximum is below initial');
      }
      if (shared && maximum === undefined) throw new TypeErrorCtor('WebAssembly.Memory: a shared memory needs a maximum');
      var idx = N.memNew(initial, maximum === undefined ? -1 : maximum, shared);
      memorySlot.set(this, idx);
      memoryObjects.set(idx, this);
    }
    grow(delta) {
      var idx = brand(memorySlot, this, 'Memory');
      delta = enforceU32(delta, 'delta');
      return N.memGrow(idx, delta);
    }
    get buffer() {
      return N.memBuffer(brand(memorySlot, this, 'Memory'));
    }
  }
  iface(Memory, 'WebAssembly.Memory');
  op(Memory.prototype, 'grow', Memory.prototype.grow, 1);
  attr(Memory.prototype, 'buffer', Object.getOwnPropertyDescriptor(Memory.prototype, 'buffer').get);

  // ---- Table -----------------------------------------------------------
  var TABLE_KINDS = { anyfunc: 4, funcref: 4, externref: 5 };
  class Table {
    constructor(descriptor) {
      needNew(new.target, 'Table');
      if (!isObject(descriptor)) throw new TypeErrorCtor('WebAssembly.Table: the descriptor must be an object');
      var is64 = addressType(descriptor.address, 'WebAssembly.Table');
      var element = descriptor.element;
      var es = String(element);
      if (element === undefined || !Object.prototype.hasOwnProperty.call(TABLE_KINDS, es)) {
        throw new TypeErrorCtor('WebAssembly.Table: unknown element type ' + es);
      }
      var initial = descriptor.initial;
      if (initial === undefined) throw new TypeErrorCtor('WebAssembly.Table: initial is required');
      initial = addressValue(initial, is64, 'initial');
      var maximum = descriptor.maximum;
      if (maximum !== undefined) maximum = addressValue(maximum, is64, 'maximum');
      if (maximum !== undefined && maximum < initial) throw new RangeErrorCtor('WebAssembly.Table: maximum is below initial');
      if (initial > 10000000) throw new RangeErrorCtor('WebAssembly.Table: initial is above 10000000');
      var hasValue = arguments.length > 1;
      var idx = N.tableNew(TABLE_KINDS[es], initial, maximum === undefined ? -1 : maximum, arguments[1], hasValue);
      tableSlot.set(this, idx);
      tableObjects.set(idx, this);
    }
    grow(delta) {
      var idx = brand(tableSlot, this, 'Table');
      delta = enforceU32(delta, 'delta');
      return N.tableGrow(idx, delta, arguments[1], arguments.length > 1);
    }
    get(index) {
      var idx = brand(tableSlot, this, 'Table');
      index = enforceU32(index, 'index');
      return N.tableGet(idx, index);
    }
    set(index) {
      var idx = brand(tableSlot, this, 'Table');
      if (arguments.length < 1) throw new TypeErrorCtor('WebAssembly.Table.set requires 1 argument');
      index = enforceU32(index, 'index');
      N.tableSet(idx, index, arguments[1], arguments.length > 1);
    }
    get length() {
      return N.tableLength(brand(tableSlot, this, 'Table'));
    }
  }
  iface(Table, 'WebAssembly.Table');
  op(Table.prototype, 'grow', Table.prototype.grow, 1);
  op(Table.prototype, 'get', Table.prototype.get, 1);
  op(Table.prototype, 'set', Table.prototype.set, 1);
  attr(Table.prototype, 'length', Object.getOwnPropertyDescriptor(Table.prototype, 'length').get);

  // ---- Global ----------------------------------------------------------
  class Global {
    constructor(descriptor) {
      needNew(new.target, 'Global');
      if (!isObject(descriptor)) throw new TypeErrorCtor('WebAssembly.Global: the descriptor must be an object');
      var mutable = !!descriptor.mutable;
      var value = descriptor.value;
      var t = valtype(value, 'WebAssembly.Global');
      if (t === 6) throw new TypeErrorCtor('WebAssembly.Global: v128 is not allowed');
      var idx = N.globalNew(t, mutable, arguments[1], arguments.length > 1 && arguments[1] !== undefined);
      globalSlot.set(this, idx);
      globalObjects.set(idx, this);
    }
    valueOf() {
      return N.globalGet(brand(globalSlot, this, 'Global'));
    }
    get value() {
      return N.globalGet(brand(globalSlot, this, 'Global'));
    }
    set value(v) {
      N.globalSet(brand(globalSlot, this, 'Global'), v);
    }
  }
  iface(Global, 'WebAssembly.Global');
  op(Global.prototype, 'valueOf', Global.prototype.valueOf, 0);
  (function () {
    var d = Object.getOwnPropertyDescriptor(Global.prototype, 'value');
    attr(Global.prototype, 'value', d.get, d.set);
  })();

  // ---- Tag -------------------------------------------------------------
  class Tag {
    constructor(type) {
      needNew(new.target, 'Tag');
      if (!isObject(type)) throw new TypeErrorCtor('WebAssembly.Tag: the type must be an object');
      var params = type.parameters;
      if (!isObject(params)) throw new TypeErrorCtor('WebAssembly.Tag: parameters must be a sequence');
      var codes = [];
      for (var p of params) codes.push(valtype(p, 'WebAssembly.Tag'));
      var idx = N.tagNew(codes);
      tagSlot.set(this, idx);
      tagObjects.set(idx, this);
    }
  }
  iface(Tag, 'WebAssembly.Tag');

  // ---- Namespace -------------------------------------------------------
  function validate(bytes) {
    if (arguments.length < 1) throw new TypeErrorCtor('WebAssembly.validate requires 1 argument');
    return N.validate(bytes);
  }
  function asPromise(fn) {
    try {
      return PromiseCtor.resolve(fn());
    } catch (e) {
      return PromiseCtor.reject(e);
    }
  }
  function compile(bytes) {
    var copy;
    try {
      if (arguments.length < 1) throw new TypeErrorCtor('WebAssembly.compile requires 1 argument');
      copy = N.copyBytes(bytes);
    } catch (e) {
      return PromiseCtor.reject(e);
    }
    return new PromiseCtor(function (resolve) { resolve(); }).then(function () {
      return new Module(copy);
    });
  }
  function instantiate(source) {
    var importObject = arguments[1];
    if (isObject(source) && moduleSlot.get(source) !== undefined) {
      var moduleObject = source;
      return new PromiseCtor(function (resolve) { resolve(); }).then(function () {
        return newInstance(instantiateCore(moduleObject, importObject));
      });
    }
    var copy;
    try {
      copy = N.copyBytes(source);
      if (importObject !== undefined && !isObject(importObject)) {
        throw new TypeErrorCtor('WebAssembly.instantiate: the import object must be an object');
      }
    } catch (e) {
      return PromiseCtor.reject(e);
    }
    return new PromiseCtor(function (resolve) { resolve(); }).then(function () {
      var m = new Module(copy);
      var inst = newInstance(instantiateCore(m, importObject));
      return { module: m, instance: inst };
    });
  }
  function responseBytes(source) {
    return PromiseCtor.resolve(source).then(function (response) {
      // Duck-typed: the engine has more than one Response implementation
      // (fetch results are not `instanceof Response`).
      if (!isObject(response) || typeof response.arrayBuffer !== 'function') {
        throw new TypeErrorCtor('WebAssembly streaming needs a Response');
      }
      var headers = response.headers;
      var type = headers && typeof headers.get === 'function' ? headers.get('Content-Type') : null;
      // The whole value must be application/wasm (no parameters).
      var mime = type ? String(type).trim().toLowerCase() : '';
      if (mime !== 'application/wasm') {
        throw new TypeErrorCtor("WebAssembly streaming: MIME type is not application/wasm (got '" + type + "')");
      }
      if (!response.ok) throw new TypeErrorCtor('WebAssembly streaming: the response status is not ok');
      if (response.bodyUsed) throw new TypeErrorCtor('WebAssembly streaming: the body is already used');
      return response.arrayBuffer();
    });
  }
  function compileStreaming(source) {
    if (arguments.length < 1) return PromiseCtor.reject(new TypeErrorCtor('compileStreaming requires 1 argument'));
    return responseBytes(source).then(function (buf) { return new Module(buf); });
  }
  function instantiateStreaming(source) {
    var importObject = arguments[1];
    if (arguments.length < 1) return PromiseCtor.reject(new TypeErrorCtor('instantiateStreaming requires 1 argument'));
    return responseBytes(source).then(function (buf) {
      var m = new Module(buf);
      return { module: m, instance: newInstance(instantiateCore(m, importObject)) };
    });
  }

  var WA = {};
  op(WA, 'validate', validate, 1);
  op(WA, 'compile', compile, 1);
  op(WA, 'instantiate', instantiate, 1);
  op(WA, 'compileStreaming', compileStreaming, 1);
  op(WA, 'instantiateStreaming', instantiateStreaming, 1);
  [['Module', Module], ['Instance', Instance], ['Memory', Memory], ['Table', Table], ['Global', Global],
   ['Tag', Tag], ['CompileError', CompileError], ['LinkError', LinkError], ['RuntimeError', RuntimeError]]
    .forEach(function (e) {
      defineProperty(WA, e[0], { value: e[1], writable: true, enumerable: false, configurable: true });
    });
  tag(WA, 'WebAssembly');
  defineProperty(g, 'WebAssembly', { value: WA, writable: true, enumerable: false, configurable: true });
})
