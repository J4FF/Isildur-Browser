// Web Crypto API (https://w3c.github.io/webcrypto/): SubtleCrypto and
// CryptoKey. Loaded lazily on the first read of `crypto.subtle` (or of the
// `SubtleCrypto`/`CryptoKey` globals). `N` are the natives of
// web_crypto.rs: pure functions over bytes.
(function (N, g) {
  'use strict';
  var defineProperty = Object.defineProperty;
  var create = Object.create;
  var PromiseCtor = Promise;
  var TypeErrorCtor = TypeError;
  var U8 = Uint8Array;

  function domError(name, msg) {
    if (typeof g.DOMException === 'function') return new g.DOMException(msg || ('The operation failed: ' + name), name);
    var e = new Error(msg || name);
    e.name = name;
    return e;
  }
  function fail(name, msg) { throw domError(name, msg); }

  function op(o, name, fn, length) {
    defineProperty(o, name, { value: fn, writable: true, enumerable: true, configurable: true });
    defineProperty(fn, 'length', { value: length, configurable: true });
    defineProperty(fn, 'name', { value: name, configurable: true });
  }
  function attr(o, name, get) {
    defineProperty(get, 'name', { value: 'get ' + name, configurable: true });
    delete o[name];
    defineProperty(o, name, { get: get, enumerable: true, configurable: true });
  }
  function isObject(v) {
    return (typeof v === 'object' && v !== null) || typeof v === 'function';
  }

  // ---- WebIDL conversions --------------------------------------------
  function isBufferSource(v) {
    return v instanceof ArrayBuffer || ArrayBuffer.isView(v) ||
      (typeof SharedArrayBuffer === 'function' && v instanceof SharedArrayBuffer);
  }
  // A copy of the bytes of a BufferSource.
  function bytesOf(v, what) {
    if (typeof SharedArrayBuffer === 'function' && v instanceof SharedArrayBuffer) {
      throw new TypeErrorCtor((what || 'data') + ' must not be a SharedArrayBuffer');
    }
    if (v instanceof ArrayBuffer) return new U8(v.slice(0));
    if (ArrayBuffer.isView(v) && v.buffer instanceof ArrayBuffer) {
      return new U8(v.buffer.slice(v.byteOffset, v.byteOffset + v.byteLength));
    }
    throw new TypeErrorCtor((what || 'data') + ' is not a BufferSource');
  }
  function enforceRange(v, max, what) {
    var n = +v;
    if (n !== n || n === Infinity || n === -Infinity) throw new TypeErrorCtor(what + ' is not a finite number');
    n = n < 0 ? Math.ceil(n) : Math.floor(n);
    if (n < 0 || n > max) throw new TypeErrorCtor(what + ' is out of range');
    return n;
  }
  var U32 = 4294967295, U16 = 65535, U8MAX = 255;
  function ab(u8) { return u8.buffer.slice(u8.byteOffset, u8.byteOffset + u8.byteLength); }
  function u8(buf) { return new U8(buf); }
  function concat(a, b) { var r = new U8(a.length + b.length); r.set(a, 0); r.set(b, a.length); return r; }
  function stripZeros(b) { var i = 0; while (i < b.length - 1 && b[i] === 0) i++; return b.subarray(i); }
  function padTo(b, n) {
    b = stripZeros(b);
    if (b.length > n) return null;
    var r = new U8(n); r.set(b, n - b.length); return r;
  }

  // base64url
  var B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';
  function b64uEncode(b) {
    var s = '', i;
    for (i = 0; i + 2 < b.length; i += 3) {
      var n = (b[i] << 16) | (b[i + 1] << 8) | b[i + 2];
      s += B64[n >> 18] + B64[(n >> 12) & 63] + B64[(n >> 6) & 63] + B64[n & 63];
    }
    if (b.length - i === 1) {
      var n1 = b[i] << 16;
      s += B64[n1 >> 18] + B64[(n1 >> 12) & 63];
    } else if (b.length - i === 2) {
      var n2 = (b[i] << 16) | (b[i + 1] << 8);
      s += B64[n2 >> 18] + B64[(n2 >> 12) & 63] + B64[(n2 >> 6) & 63];
    }
    return s;
  }
  function b64uDecode(s) {
    if (typeof s !== 'string') fail('DataError', 'JWK member is not a string');
    s = s.replace(/=+$/, '');
    if (s.length % 4 === 1 || /[^A-Za-z0-9_-]/.test(s)) fail('DataError', 'invalid base64url');
    var out = [], buf = 0, bits = 0;
    for (var i = 0; i < s.length; i++) {
      buf = (buf << 6) | B64.indexOf(s[i]);
      bits += 6;
      if (bits >= 8) { bits -= 8; out.push((buf >> bits) & 255); }
    }
    return new U8(out);
  }

  // ---- DER -------------------------------------------------------------
  function derLen(n) {
    if (n < 128) return [n];
    var b = [];
    while (n > 0) { b.unshift(n & 255); n >>= 8; }
    return [0x80 | b.length].concat(b);
  }
  function der(tag, content) {
    var h = [tag].concat(derLen(content.length));
    var r = new U8(h.length + content.length);
    r.set(h, 0); r.set(content, h.length);
    return r;
  }
  function derSeq() {
    var parts = Array.prototype.slice.call(arguments), n = 0, i;
    for (i = 0; i < parts.length; i++) n += parts[i].length;
    var c = new U8(n), o = 0;
    for (i = 0; i < parts.length; i++) { c.set(parts[i], o); o += parts[i].length; }
    return der(0x30, c);
  }
  function derInt(b) {
    b = stripZeros(b);
    if (b[0] & 0x80) b = concat(new U8([0]), b);
    return der(0x02, b);
  }
  function derOid(s) {
    var p = s.split('.').map(Number), out = [40 * p[0] + p[1]];
    for (var i = 2; i < p.length; i++) {
      var v = p[i], t = [v & 127];
      v = Math.floor(v / 128);
      while (v > 0) { t.unshift(0x80 | (v & 127)); v = Math.floor(v / 128); }
      out = out.concat(t);
    }
    return der(0x06, new U8(out));
  }
  var DER_NULL = new U8([5, 0]);
  function derBits(b) { return der(0x03, concat(new U8([0]), b)); }
  function derOctets(b) { return der(0x04, b); }
  // Parser: returns {tag, content, end}.
  function derRead(b, pos) {
    if (pos + 2 > b.length) fail('DataError', 'truncated DER');
    var tag = b[pos], len = b[pos + 1], p = pos + 2;
    if (len & 0x80) {
      var k = len & 127;
      if (k === 0 || k > 4 || p + k > b.length) fail('DataError', 'bad DER length');
      len = 0;
      for (var i = 0; i < k; i++) len = len * 256 + b[p + i];
      p += k;
    }
    if (p + len > b.length) fail('DataError', 'truncated DER');
    return { tag: tag, content: b.subarray(p, p + len), end: p + len };
  }
  function derChildren(content) {
    var out = [], p = 0;
    while (p < content.length) { var e = derRead(content, p); out.push(e); p = e.end; }
    return out;
  }
  function derTop(b, tag) {
    var e = derRead(b, 0);
    if (e.tag !== tag || e.end !== b.length) fail('DataError', 'bad DER structure');
    return e;
  }
  function oidText(content) {
    var parts = [Math.floor(content[0] / 40), content[0] % 40], v = 0;
    for (var i = 1; i < content.length; i++) {
      v = v * 128 + (content[i] & 127);
      if (!(content[i] & 0x80)) { parts.push(v); v = 0; }
    }
    return parts.join('.');
  }
  var OID_RSA = '1.2.840.113549.1.1.1', OID_EC = '1.2.840.10045.2.1';
  var OID_ED25519 = '1.3.101.112', OID_X25519 = '1.3.101.110';
  var CURVE_OID = { 'P-256': '1.2.840.10045.3.1.7', 'P-384': '1.3.132.0.34', 'P-521': '1.3.132.0.35' };
  var CURVE_LEN = { 'P-256': 32, 'P-384': 48, 'P-521': 66 };
  function curveOfOid(o) {
    for (var k in CURVE_OID) if (CURVE_OID[k] === o) return k;
    return null;
  }
  // SubjectPublicKeyInfo -> {oid, params(entry|null), key(bytes)}
  function parseSpki(b) {
    var top = derTop(b, 0x30), kids = derChildren(top.content);
    if (kids.length !== 2 || kids[0].tag !== 0x30 || kids[1].tag !== 0x03) fail('DataError', 'bad SubjectPublicKeyInfo');
    var alg = derChildren(kids[0].content);
    if (!alg.length || alg[0].tag !== 0x06) fail('DataError', 'bad AlgorithmIdentifier');
    if (kids[1].content[0] !== 0) fail('DataError', 'bad BIT STRING');
    return { oid: oidText(alg[0].content), params: alg[1] || null, key: kids[1].content.subarray(1) };
  }
  // PrivateKeyInfo -> {oid, params, key}
  function parsePkcs8(b) {
    var top = derTop(b, 0x30), kids = derChildren(top.content);
    if (kids.length < 3 || kids[0].tag !== 0x02 || kids[1].tag !== 0x30 || kids[2].tag !== 0x04) fail('DataError', 'bad PrivateKeyInfo');
    var alg = derChildren(kids[1].content);
    if (!alg.length || alg[0].tag !== 0x06) fail('DataError', 'bad AlgorithmIdentifier');
    return { oid: oidText(alg[0].content), params: alg[1] || null, key: kids[2].content };
  }
  function intsOf(content, n) {
    var kids = derChildren(content);
    if (kids.length < n) fail('DataError', 'bad key structure');
    return kids.slice(0, n).map(function (k) {
      if (k.tag !== 0x02) fail('DataError', 'bad INTEGER');
      return stripZeros(k.content);
    });
  }

  // ---- Algorithm registry and normalization ---------------------------
  var USAGES = ['encrypt', 'decrypt', 'sign', 'verify', 'deriveKey', 'deriveBits', 'wrapKey', 'unwrapKey',
    'encapsulateKey', 'encapsulateBits', 'decapsulateKey', 'decapsulateBits'];
  var NAMES = ['RSASSA-PKCS1-v1_5', 'RSA-PSS', 'RSA-OAEP', 'ECDSA', 'ECDH', 'AES-CTR', 'AES-CBC', 'AES-GCM', 'AES-KW',
    'HMAC', 'SHA-1', 'SHA-256', 'SHA-384', 'SHA-512', 'HKDF', 'PBKDF2', 'Ed25519', 'X25519'];
  var BY_UPPER = {};
  NAMES.forEach(function (n) { BY_UPPER[n.toUpperCase()] = n; });

  function member(dict, key) { return dict === undefined || dict === null ? undefined : dict[key]; }
  function reqBytes(dict, key) {
    var v = member(dict, key);
    if (v === undefined) throw new TypeErrorCtor(key + ' is required');
    return bytesOf(v, key);
  }
  function optBytes(dict, key) {
    var v = member(dict, key);
    return v === undefined ? undefined : bytesOf(v, key);
  }
  function reqHash(dict) {
    var v = member(dict, 'hash');
    if (v === undefined) throw new TypeErrorCtor('hash is required');
    return normalize('digest', v);
  }
  function reqNum(dict, key, max) {
    var v = member(dict, key);
    if (v === undefined) throw new TypeErrorCtor(key + ' is required');
    return enforceRange(v, max, key);
  }
  function optNum(dict, key, max) {
    var v = member(dict, key);
    return v === undefined ? undefined : enforceRange(v, max, key);
  }
  function reqStr(dict, key) {
    var v = member(dict, key);
    if (v === undefined) throw new TypeErrorCtor(key + ' is required');
    return String(v);
  }

  // Parameter dictionaries (members read in lexicographic order).
  var P = {
    none: function () { return {}; },
    aesCbc: function (a) { return { iv: reqBytes(a, 'iv') }; },
    aesCtr: function (a) { return { counter: reqBytes(a, 'counter'), length: reqNum(a, 'length', U8MAX) }; },
    aesGcm: function (a) {
      var r = { additionalData: optBytes(a, 'additionalData'), iv: reqBytes(a, 'iv') };
      r.tagLength = optNum(a, 'tagLength', U8MAX);
      return r;
    },
    aesKeyGen: function (a) { return { length: reqNum(a, 'length', U16) }; },
    hmacImport: function (a) { var h = reqHash(a); return { hash: h, length: optNum(a, 'length', U32) }; },
    rsaKeyGen: function (a) {
      var h = reqHash(a);
      var m = reqNum(a, 'modulusLength', U32);
      var e = member(a, 'publicExponent');
      if (e === undefined) throw new TypeErrorCtor('publicExponent is required');
      if (!(e instanceof U8)) throw new TypeErrorCtor('publicExponent must be a Uint8Array');
      return { hash: h, modulusLength: m, publicExponent: new U8(e) };
    },
    rsaImport: function (a) { return { hash: reqHash(a) }; },
    rsaPss: function (a) { return { saltLength: reqNum(a, 'saltLength', U32) }; },
    rsaOaep: function (a) { return { label: optBytes(a, 'label') }; },
    ecKey: function (a) { return { namedCurve: reqStr(a, 'namedCurve') }; },
    ecdsa: function (a) { return { hash: reqHash(a) }; },
    ecdh: function (a) {
      var k = member(a, 'public');
      if (k === undefined) throw new TypeErrorCtor('public is required');
      if (!keySlot.has(k)) throw new TypeErrorCtor('public is not a CryptoKey');
      return { public: k };
    },
    hkdf: function (a) { var h = reqHash(a); return { hash: h, info: reqBytes(a, 'info'), salt: reqBytes(a, 'salt') }; },
    pbkdf2: function (a) {
      var h = reqHash(a);
      return { hash: h, iterations: reqNum(a, 'iterations', U32), salt: reqBytes(a, 'salt') };
    }
  };

  var REG = {
    encrypt: { 'RSA-OAEP': P.rsaOaep, 'AES-CTR': P.aesCtr, 'AES-CBC': P.aesCbc, 'AES-GCM': P.aesGcm },
    decrypt: { 'RSA-OAEP': P.rsaOaep, 'AES-CTR': P.aesCtr, 'AES-CBC': P.aesCbc, 'AES-GCM': P.aesGcm },
    sign: { 'RSASSA-PKCS1-v1_5': P.none, 'RSA-PSS': P.rsaPss, 'ECDSA': P.ecdsa, 'HMAC': P.none, 'Ed25519': P.none },
    verify: { 'RSASSA-PKCS1-v1_5': P.none, 'RSA-PSS': P.rsaPss, 'ECDSA': P.ecdsa, 'HMAC': P.none, 'Ed25519': P.none },
    digest: { 'SHA-1': P.none, 'SHA-256': P.none, 'SHA-384': P.none, 'SHA-512': P.none },
    generateKey: {
      'RSASSA-PKCS1-v1_5': P.rsaKeyGen, 'RSA-PSS': P.rsaKeyGen, 'RSA-OAEP': P.rsaKeyGen,
      'ECDSA': P.ecKey, 'ECDH': P.ecKey, 'AES-CTR': P.aesKeyGen, 'AES-CBC': P.aesKeyGen,
      'AES-GCM': P.aesKeyGen, 'AES-KW': P.aesKeyGen, 'HMAC': P.hmacImport, 'Ed25519': P.none, 'X25519': P.none
    },
    importKey: {
      'RSASSA-PKCS1-v1_5': P.rsaImport, 'RSA-PSS': P.rsaImport, 'RSA-OAEP': P.rsaImport,
      'ECDSA': P.ecKey, 'ECDH': P.ecKey, 'AES-CTR': P.none, 'AES-CBC': P.none, 'AES-GCM': P.none,
      'AES-KW': P.none, 'HMAC': P.hmacImport, 'HKDF': P.none, 'PBKDF2': P.none, 'Ed25519': P.none, 'X25519': P.none
    },
    deriveBits: { 'ECDH': P.ecdh, 'HKDF': P.hkdf, 'PBKDF2': P.pbkdf2, 'X25519': P.ecdh },
    getKeyLength: {
      'AES-CTR': P.aesKeyGen, 'AES-CBC': P.aesKeyGen, 'AES-GCM': P.aesKeyGen, 'AES-KW': P.aesKeyGen,
      'HMAC': P.hmacImport, 'HKDF': P.none, 'PBKDF2': P.none
    },
    wrapKey: { 'AES-KW': P.none },
    unwrapKey: { 'AES-KW': P.none }
  };

  function normalize(opName, alg) {
    if (typeof alg === 'string') return normalize(opName, { name: alg });
    if (!isObject(alg)) return normalize(opName, { name: String(alg) });
    var nameV = alg.name;
    if (nameV === undefined) throw new TypeErrorCtor('Algorithm: name is required');
    var name = String(nameV);
    var canon = BY_UPPER[name.toUpperCase()];
    var table = REG[opName];
    if (!canon || !Object.prototype.hasOwnProperty.call(table, canon)) {
      fail('NotSupportedError', 'Algorithm not supported: ' + name);
    }
    var r = table[canon](alg);
    r.name = canon;
    return r;
  }

  function usagesOf(v) {
    if (!isObject(v) || typeof v[Symbol.iterator] !== 'function') throw new TypeErrorCtor('usages must be a sequence');
    var out = [];
    for (var u of v) {
      var s = String(u);
      if (USAGES.indexOf(s) < 0) throw new TypeErrorCtor('unknown key usage ' + s);
      out.push(s);
    }
    return out;
  }
  // Usages in the canonical order, deduplicated.
  function canonUsages(list, allowed) {
    return USAGES.filter(function (u) { return list.indexOf(u) >= 0 && (!allowed || allowed.indexOf(u) >= 0); });
  }
  function checkUsages(list, allowed) {
    for (var i = 0; i < list.length; i++) {
      if (allowed.indexOf(list[i]) < 0) fail('SyntaxError', 'usage ' + list[i] + ' is not allowed for this key');
    }
  }

  // ---- CryptoKey ---------------------------------------------------------
  var keySlot = new WeakMap();
  function CryptoKey() { throw new TypeErrorCtor('Illegal constructor'); }
  defineProperty(CryptoKey, 'prototype', { writable: false, enumerable: false, configurable: false });
  defineProperty(CryptoKey.prototype, 'constructor', { value: CryptoKey, writable: true, enumerable: false, configurable: true });
  defineProperty(CryptoKey.prototype, Symbol.toStringTag, { value: 'CryptoKey', configurable: true });
  function slotOf(k, what) {
    var s = isObject(k) ? keySlot.get(k) : undefined;
    if (!s) throw new TypeErrorCtor((what || 'key') + ' is not a CryptoKey');
    return s;
  }
  attr(CryptoKey.prototype, 'type', function () { return slotOf(this).type; });
  attr(CryptoKey.prototype, 'extractable', function () { return slotOf(this).extractable; });
  attr(CryptoKey.prototype, 'algorithm', function () { return slotOf(this).algorithmObject; });
  attr(CryptoKey.prototype, 'usages', function () { return slotOf(this).usagesObject; });

  // algorithm: the KeyAlgorithm dictionary (exposed as a cached object);
  // data: the key material.
  function makeKey(type, extractable, algorithm, usages, data) {
    var k = create(CryptoKey.prototype);
    var exposed = {};
    for (var name in algorithm) {
      var v = algorithm[name];
      if (v instanceof U8) v = new U8(v);
      else if (name === 'hash') v = { name: v.name };
      exposed[name] = v;
    }
    keySlot.set(k, {
      type: type, extractable: !!extractable, algorithm: algorithm, usages: usages,
      algorithmObject: exposed, usagesObject: usages.slice(), data: data
    });
    return k;
  }

  function hashName(h) { return h.name; }

  // ---- Operations ----------------------------------------------------------
  function checkKey(slot, alg, usage) {
    if (slot.algorithm.name !== alg.name) fail('InvalidAccessError', 'key algorithm does not match');
    if (slot.usages.indexOf(usage) < 0) fail('InvalidAccessError', 'key usage ' + usage + ' not allowed');
  }

  function cryptOp(encrypt, alg, slot, data) {
    var d = slot.data;
    try {
      switch (alg.name) {
        case 'AES-CBC':
          if (alg.iv.length !== 16) fail('OperationError', 'AES-CBC iv must be 16 bytes');
          return N.aesCbc(encrypt, d.raw, alg.iv, data);
        case 'AES-CTR':
          if (alg.counter.length !== 16) fail('OperationError', 'AES-CTR counter must be 16 bytes');
          if (alg.length === 0 || alg.length > 128) fail('OperationError', 'AES-CTR length must be 1..128');
          return N.aesCtr(d.raw, alg.counter, alg.length, data);
        case 'AES-GCM':
          var t = alg.tagLength === undefined ? 128 : alg.tagLength;
          if ([32, 64, 96, 104, 112, 120, 128].indexOf(t) < 0) fail('OperationError', 'AES-GCM tag length is not allowed');
          return N.aesGcm(encrypt, d.raw, alg.iv, alg.additionalData || new U8(0), data, t);
        case 'RSA-OAEP':
          if (encrypt) {
            if (slot.type !== 'public') fail('InvalidAccessError', 'RSA-OAEP encrypt needs a public key');
            return N.rsaEncrypt(slot.algorithm.hash.name, d.n, d.e, data, alg.label || new U8(0));
          }
          if (slot.type !== 'private') fail('InvalidAccessError', 'RSA-OAEP decrypt needs a private key');
          return N.rsaDecrypt(slot.algorithm.hash.name, rsaParts(d), data, alg.label || new U8(0));
      }
    } catch (e) {
      if (e && (e instanceof TypeErrorCtor || (typeof g.DOMException === 'function' && e instanceof g.DOMException))) throw e;
      fail('OperationError', e && e.message);
    }
    fail('NotSupportedError');
  }
  function rsaParts(d) { return [d.n, d.e, d.d, d.p, d.q, d.dp, d.dq, d.qi]; }

  function signOp(alg, slot, data) {
    var d = slot.data;
    try {
      switch (alg.name) {
        case 'HMAC':
          return N.hmac(slot.algorithm.hash.name, d.raw, data);
        case 'RSASSA-PKCS1-v1_5':
        case 'RSA-PSS':
          if (slot.type !== 'private') fail('InvalidAccessError', 'signing needs a private key');
          return N.rsaSign(alg.name === 'RSA-PSS', slot.algorithm.hash.name, rsaParts(d), data, alg.saltLength || 0);
        case 'ECDSA':
          if (slot.type !== 'private') fail('InvalidAccessError', 'signing needs a private key');
          return N.ecSign(slot.algorithm.namedCurve, alg.hash.name, d.d, data);
        case 'Ed25519':
          if (slot.type !== 'private') fail('InvalidAccessError', 'signing needs a private key');
          return N.edSign(d.d, data);
      }
    } catch (e) {
      if (e && (e instanceof TypeErrorCtor || (typeof g.DOMException === 'function' && e instanceof g.DOMException))) throw e;
      fail('OperationError', e && e.message);
    }
    fail('NotSupportedError');
  }

  function verifyOp(alg, slot, sig, data) {
    var d = slot.data;
    switch (alg.name) {
      case 'HMAC':
        var mac = u8(N.hmac(slot.algorithm.hash.name, d.raw, data));
        if (mac.length !== sig.length) return false;
        var diff = 0;
        for (var i = 0; i < mac.length; i++) diff |= mac[i] ^ sig[i];
        return diff === 0;
      case 'RSASSA-PKCS1-v1_5':
      case 'RSA-PSS':
        if (slot.type !== 'public') fail('InvalidAccessError', 'verifying needs a public key');
        return N.rsaVerify(alg.name === 'RSA-PSS', slot.algorithm.hash.name, d.n, d.e, data, sig, alg.saltLength || 0);
      case 'ECDSA':
        if (slot.type !== 'public') fail('InvalidAccessError', 'verifying needs a public key');
        return N.ecVerify(slot.algorithm.namedCurve, alg.hash.name, d.x, d.y, data, sig);
      case 'Ed25519':
        if (slot.type !== 'public') fail('InvalidAccessError', 'verifying needs a public key');
        return N.edVerify(d.x, data, sig);
    }
    fail('NotSupportedError');
  }

  var AES_USAGES = ['encrypt', 'decrypt', 'wrapKey', 'unwrapKey'];
  var KW_USAGES = ['wrapKey', 'unwrapKey'];
  function secretUsagesFor(name) {
    if (name === 'AES-KW') return KW_USAGES;
    if (name.slice(0, 4) === 'AES-') return AES_USAGES;
    if (name === 'HMAC') return ['sign', 'verify'];
    if (name === 'HKDF' || name === 'PBKDF2') return ['deriveKey', 'deriveBits'];
    return [];
  }
  // [public usages, private usages] of asymmetric algorithms.
  function pairUsagesFor(name) {
    switch (name) {
      case 'RSASSA-PKCS1-v1_5': case 'RSA-PSS': case 'ECDSA': case 'Ed25519': return [['verify'], ['sign']];
      case 'RSA-OAEP': return [['encrypt', 'wrapKey'], ['decrypt', 'unwrapKey']];
      case 'ECDH': case 'X25519': return [[], ['deriveKey', 'deriveBits']];
    }
    return [[], []];
  }

  function hmacLengthFor(alg) {
    var hl = { 'SHA-1': 512, 'SHA-256': 512, 'SHA-384': 1024, 'SHA-512': 1024 }[alg.hash.name];
    if (alg.length === undefined) return hl;
    if (alg.length === 0) fail('OperationError', 'HMAC key length must not be 0');
    return alg.length;
  }

  function generateKeyOp(alg, extractable, usages) {
    var name = alg.name;
    if (name.slice(0, 4) === 'AES-') {
      checkUsages(usages, secretUsagesFor(name));
      if ([128, 192, 256].indexOf(alg.length) < 0) fail('OperationError', 'AES key length must be 128, 192 or 256');
      var raw = u8(N.random(alg.length / 8));
      return makeKey('secret', extractable, { name: name, length: alg.length }, canonUsages(usages), { raw: raw });
    }
    if (name === 'HMAC') {
      checkUsages(usages, ['sign', 'verify']);
      var len = hmacLengthFor(alg);
      var key = u8(N.random(Math.ceil(len / 8)));
      return makeKey('secret', extractable, { name: name, hash: { name: alg.hash.name }, length: len }, canonUsages(usages), { raw: key });
    }
    var pu = pairUsagesFor(name);
    checkUsages(usages, pu[0].concat(pu[1]));
    var pubAlg, pubData, privData;
    if (name.slice(0, 3) === 'RSA') {
      var e = stripZeros(alg.publicExponent);
      var eh = e.length === 1 && e[0] === 3 ? 3 : (e.length === 3 && e[0] === 1 && e[1] === 0 && e[2] === 1 ? 65537 : 0);
      if (!eh) fail('OperationError', 'RSA public exponent must be 3 or 65537');
      if (alg.modulusLength < 256 || alg.modulusLength > 16384 || alg.modulusLength % 8) fail('OperationError', 'RSA modulus length is not supported');
      var parts;
      try { parts = N.rsaGenerate(alg.modulusLength, e).map(u8); } catch (err) { fail('OperationError', err.message); }
      pubAlg = { name: name, modulusLength: alg.modulusLength, publicExponent: new U8(e), hash: { name: alg.hash.name } };
      pubData = { n: parts[0], e: parts[1] };
      privData = { n: parts[0], e: parts[1], d: parts[2], p: parts[3], q: parts[4], dp: parts[5], dq: parts[6], qi: parts[7] };
    } else if (name === 'ECDSA' || name === 'ECDH') {
      if (!CURVE_LEN[alg.namedCurve]) fail('NotSupportedError', 'unsupported curve ' + alg.namedCurve);
      var ec = N.ecGenerate(alg.namedCurve).map(u8);
      pubAlg = { name: name, namedCurve: alg.namedCurve };
      pubData = { x: ec[1], y: ec[2] };
      privData = { d: ec[0], x: ec[1], y: ec[2] };
    } else if (name === 'Ed25519') {
      var ed = N.edGenerate().map(u8);
      pubAlg = { name: name };
      pubData = { x: ed[1] };
      privData = { d: ed[0], x: ed[1] };
    } else if (name === 'X25519') {
      var xk = N.xGenerate().map(u8);
      pubAlg = { name: name };
      pubData = { x: xk[1] };
      privData = { d: xk[0], x: xk[1] };
    } else {
      fail('NotSupportedError');
    }
    var publicKey = makeKey('public', true, pubAlg, canonUsages(usages, pu[0]), pubData);
    var privateKey = makeKey('private', extractable, pubAlg, canonUsages(usages, pu[1]), privData);
    return { publicKey: publicKey, privateKey: privateKey };
  }

  // ---- import -----------------------------------------------------------------
  var JWK_HASH_SUFFIX = { 'SHA-1': '1', 'SHA-256': '256', 'SHA-384': '384', 'SHA-512': '512' };
  function jwkCheckCommon(jwk, extractable, usages, use) {
    if (use !== undefined && jwk.use !== undefined && jwk.use !== use) fail('DataError', 'JWK use does not match');
    if (jwk.key_ops !== undefined) {
      if (!Array.isArray(jwk.key_ops)) fail('DataError', 'JWK key_ops is not an array');
      var seen = {};
      jwk.key_ops.forEach(function (o) {
        if (seen[o]) fail('DataError', 'JWK key_ops has duplicates');
        seen[o] = true;
      });
      usages.forEach(function (u) { if (jwk.key_ops.indexOf(u) < 0) fail('DataError', 'usage not in JWK key_ops'); });
    }
    if (jwk.ext === false && extractable) fail('DataError', 'JWK is not extractable');
  }
  function jwkOf(keyData) {
    if (!isObject(keyData) || isBufferSource(keyData)) throw new TypeErrorCtor('JWK must be an object');
    var r = {};
    ['kty', 'use', 'alg', 'crv', 'x', 'y', 'd', 'n', 'e', 'p', 'q', 'dp', 'dq', 'qi', 'k'].forEach(function (key) {
      var v = keyData[key];
      if (v !== undefined) r[key] = String(v);
    });
    if (keyData.key_ops !== undefined) {
      var ops = [];
      for (var o of keyData.key_ops) ops.push(String(o));
      r.key_ops = ops;
    }
    if (keyData.ext !== undefined) r.ext = !!keyData.ext;
    if (keyData.oth !== undefined) r.oth = keyData.oth;
    return r;
  }

  function importSecret(format, keyData, alg, extractable, usages) {
    var name = alg.name, raw;
    checkUsages(usages, secretUsagesFor(name));
    if (name === 'HKDF' || name === 'PBKDF2') {
      if (format !== 'raw' && format !== 'raw-secret') fail('NotSupportedError', name + ' keys can only be imported as raw');
      if (extractable) fail('SyntaxError', name + ' keys are not extractable');
      return makeKey('secret', false, { name: name }, canonUsages(usages), { raw: keyData });
    }
    if (format === 'raw' || format === 'raw-secret') {
      raw = keyData;
    } else if (format === 'jwk') {
      var jwk = keyData;
      if (jwk.kty !== 'oct') fail('DataError', 'JWK kty must be oct');
      if (jwk.k === undefined) fail('DataError', 'JWK k is missing');
      raw = b64uDecode(jwk.k);
      if (name === 'HMAC') {
        var want = 'HS' + JWK_HASH_SUFFIX[alg.hash.name];
        if (jwk.alg !== undefined && jwk.alg !== want) fail('DataError', 'JWK alg does not match');
        jwkCheckCommon(jwk, extractable, usages, 'sig');
      } else {
        var mode = { 'AES-CBC': 'CBC', 'AES-CTR': 'CTR', 'AES-GCM': 'GCM', 'AES-KW': 'KW' }[name];
        var wantA = 'A' + (raw.length * 8) + mode;
        if ([16, 24, 32].indexOf(raw.length) >= 0 && jwk.alg !== undefined && jwk.alg !== wantA) fail('DataError', 'JWK alg does not match');
        jwkCheckCommon(jwk, extractable, usages, 'enc');
      }
    } else {
      fail('NotSupportedError', 'format ' + format + ' is not supported for ' + name);
    }
    if (name === 'HMAC') {
      if (raw.length === 0) fail('DataError', 'HMAC key must not be empty');
      var bits = raw.length * 8;
      if (alg.length !== undefined) {
        if (alg.length > bits || alg.length <= bits - 8) fail('DataError', 'HMAC length does not match the key');
        bits = alg.length;
      }
      return makeKey('secret', extractable, { name: name, hash: { name: alg.hash.name }, length: bits }, canonUsages(usages), { raw: raw });
    }
    if ([16, 24, 32].indexOf(raw.length) < 0) fail('DataError', 'AES key must be 128, 192 or 256 bits');
    return makeKey('secret', extractable, { name: name, length: raw.length * 8 }, canonUsages(usages), { raw: raw });
  }

  function rsaJwkAlg(name, hash) {
    var s = JWK_HASH_SUFFIX[hash];
    if (name === 'RSASSA-PKCS1-v1_5') return 'RS' + s;
    if (name === 'RSA-PSS') return 'PS' + s;
    return hash === 'SHA-1' ? 'RSA-OAEP' : 'RSA-OAEP-' + s;
  }
  function rsaKeyAlg(name, hash, n, e) {
    return { name: name, modulusLength: stripZeros(n).length * 8 - leadingZeroBits(stripZeros(n)[0]), publicExponent: new U8(stripZeros(e)), hash: { name: hash } };
  }
  function leadingZeroBits(b) { var n = 0; while (n < 8 && !(b & (0x80 >> n))) n++; return n; }

  function importRsa(format, keyData, alg, extractable, usages) {
    var name = alg.name, pu = pairUsagesFor(name), data, type;
    if (format === 'spki') {
      checkUsages(usages, pu[0]);
      var s = parseSpki(keyData);
      if (s.oid !== OID_RSA) fail('DataError', 'not an RSA key');
      var pk = intsOf(derTop(s.key, 0x30).content, 2);
      data = { n: pk[0], e: pk[1] }; type = 'public';
    } else if (format === 'pkcs8') {
      checkUsages(usages, pu[1]);
      var p8 = parsePkcs8(keyData);
      if (p8.oid !== OID_RSA) fail('DataError', 'not an RSA key');
      var v = intsOf(derTop(p8.key, 0x30).content, 9);
      data = { n: v[1], e: v[2], d: v[3], p: v[4], q: v[5], dp: v[6], dq: v[7], qi: v[8] }; type = 'private';
    } else if (format === 'jwk') {
      var jwk = keyData;
      if (jwk.d !== undefined) checkUsages(usages, pu[1]); else checkUsages(usages, pu[0]);
      if (jwk.kty !== 'RSA') fail('DataError', 'JWK kty must be RSA');
      if (jwk.alg !== undefined && jwk.alg !== rsaJwkAlg(name, alg.hash.name)) fail('DataError', 'JWK alg does not match');
      jwkCheckCommon(jwk, extractable, usages, name === 'RSA-OAEP' ? 'enc' : 'sig');
      if (jwk.n === undefined || jwk.e === undefined) fail('DataError', 'JWK n/e missing');
      data = { n: stripZeros(b64uDecode(jwk.n)), e: stripZeros(b64uDecode(jwk.e)) };
      type = 'public';
      if (jwk.d !== undefined) {
        ['p', 'q', 'dp', 'dq', 'qi'].forEach(function (k) { if (jwk[k] === undefined) fail('DataError', 'JWK ' + k + ' missing'); });
        if (jwk.oth !== undefined) fail('NotSupportedError', 'multi-prime RSA keys are not supported');
        data.d = stripZeros(b64uDecode(jwk.d));
        ['p', 'q', 'dp', 'dq', 'qi'].forEach(function (k) { data[k] = stripZeros(b64uDecode(jwk[k])); });
        type = 'private';
      }
    } else {
      fail('NotSupportedError', 'format ' + format + ' is not supported for ' + name);
    }
    if (type === 'private' && !N.rsaCheck(rsaParts(data))) fail('DataError', 'RSA key is inconsistent');
    if (data.n.length === 0 || data.e.length === 0) fail('DataError', 'RSA key is empty');
    var keyAlg = rsaKeyAlg(name, alg.hash.name, data.n, data.e);
    return makeKey(type, extractable, keyAlg, canonUsages(usages), data);
  }

  function ecPointFromBytes(curve, b) {
    try { return N.ecPoint(curve, b).map(u8); } catch (e) { fail('DataError', 'invalid EC point'); }
  }
  function importEc(format, keyData, alg, extractable, usages) {
    var name = alg.name, pu = pairUsagesFor(name), curve = alg.namedCurve, data, type;
    if (!CURVE_LEN[curve]) fail('NotSupportedError', 'unsupported curve ' + curve);
    var L = CURVE_LEN[curve];
    if (format === 'raw' || format === 'raw-public') {
      checkUsages(usages, pu[0]);
      var pt = ecPointFromBytes(curve, keyData);
      data = { x: pt[0], y: pt[1] }; type = 'public';
    } else if (format === 'spki') {
      checkUsages(usages, pu[0]);
      var s = parseSpki(keyData);
      if (s.oid !== OID_EC) fail('DataError', 'not an EC key');
      if (!s.params || s.params.tag !== 0x06) fail('DataError', 'missing named curve');
      var c = curveOfOid(oidText(s.params.content));
      if (!c) fail('NotSupportedError', 'unsupported curve');
      if (c !== curve) fail('DataError', 'curve does not match');
      var p = ecPointFromBytes(curve, s.key);
      data = { x: p[0], y: p[1] }; type = 'public';
    } else if (format === 'pkcs8') {
      checkUsages(usages, pu[1]);
      var p8 = parsePkcs8(keyData);
      if (p8.oid !== OID_EC) fail('DataError', 'not an EC key');
      if (!p8.params || p8.params.tag !== 0x06) fail('DataError', 'missing named curve');
      var c8 = curveOfOid(oidText(p8.params.content));
      if (!c8) fail('NotSupportedError', 'unsupported curve');
      if (c8 !== curve) fail('DataError', 'curve does not match');
      var ecpk = derChildren(derTop(p8.key, 0x30).content);
      if (ecpk.length < 2 || ecpk[1].tag !== 0x04) fail('DataError', 'bad ECPrivateKey');
      var d = padTo(ecpk[1].content, L);
      if (!d) fail('DataError', 'bad EC private key');
      var xy;
      try { xy = N.ecPublic(curve, d).map(u8); } catch (e) { fail('DataError', 'bad EC private key'); }
      for (var i = 2; i < ecpk.length; i++) {
        if (ecpk[i].tag === 0xa1) {
          var bits = derRead(ecpk[i].content, 0);
          var given = ecPointFromBytes(curve, bits.content.subarray(1));
          if (given[0].join() !== xy[0].join() || given[1].join() !== xy[1].join()) fail('DataError', 'EC public key does not match');
        } else if (ecpk[i].tag === 0xa0) {
          var po = derRead(ecpk[i].content, 0);
          if (curveOfOid(oidText(po.content)) !== curve) fail('DataError', 'curve does not match');
        }
      }
      data = { d: d, x: xy[0], y: xy[1] }; type = 'private';
    } else if (format === 'jwk') {
      var jwk = keyData;
      if (jwk.d !== undefined) checkUsages(usages, pu[1]); else checkUsages(usages, pu[0]);
      if (jwk.kty !== 'EC') fail('DataError', 'JWK kty must be EC');
      if (jwk.crv !== curve) fail('DataError', 'JWK crv does not match');
      if (name === 'ECDSA' && jwk.alg !== undefined) {
        var wantAlg = { 'P-256': 'ES256', 'P-384': 'ES384', 'P-521': 'ES512' }[curve];
        if (jwk.alg !== wantAlg) fail('DataError', 'JWK alg does not match');
      }
      jwkCheckCommon(jwk, extractable, usages, name === 'ECDSA' ? 'sig' : 'enc');
      if (jwk.x === undefined || jwk.y === undefined) fail('DataError', 'JWK x/y missing');
      var x = b64uDecode(jwk.x), y = b64uDecode(jwk.y);
      if (x.length !== L || y.length !== L) fail('DataError', 'JWK coordinate length');
      var pj = ecPointFromBytes(curve, concat(new U8([4]), concat(x, y)));
      data = { x: pj[0], y: pj[1] }; type = 'public';
      if (jwk.d !== undefined) {
        var dj = b64uDecode(jwk.d);
        if (dj.length !== L) fail('DataError', 'JWK d length');
        var q;
        try { q = N.ecPublic(curve, dj).map(u8); } catch (e) { fail('DataError', 'bad EC private key'); }
        if (q[0].join() !== x.join() || q[1].join() !== y.join()) fail('DataError', 'EC public key does not match');
        data.d = dj; type = 'private';
      }
    } else {
      fail('NotSupportedError', 'format ' + format + ' is not supported for ' + name);
    }
    return makeKey(type, extractable, { name: name, namedCurve: curve }, canonUsages(usages), data);
  }

  function importOkp(format, keyData, alg, extractable, usages) {
    var name = alg.name, pu = pairUsagesFor(name), data, type;
    var oid = name === 'Ed25519' ? OID_ED25519 : OID_X25519;
    function checkPub(x) {
      // Any 32 bytes import; an invalid or small-order Ed25519 point just
      // never verifies.
      if (x.length !== 32) fail('DataError', name + ' public key must be 32 bytes');
    }
    function pubOf(d) { return u8(name === 'Ed25519' ? N.edPublic(d) : N.xPublic(d)); }
    if (format === 'raw' || format === 'raw-public') {
      checkUsages(usages, pu[0]);
      checkPub(keyData);
      data = { x: keyData }; type = 'public';
    } else if (format === 'spki') {
      checkUsages(usages, pu[0]);
      var s = parseSpki(keyData);
      if (s.oid !== oid) fail('DataError', 'not an ' + name + ' key');
      if (s.params) fail('DataError', 'unexpected parameters');
      checkPub(s.key);
      data = { x: new U8(s.key) }; type = 'public';
    } else if (format === 'pkcs8') {
      checkUsages(usages, pu[1]);
      var p8 = parsePkcs8(keyData);
      if (p8.oid !== oid) fail('DataError', 'not an ' + name + ' key');
      if (p8.params) fail('DataError', 'unexpected parameters');
      var inner = derTop(p8.key, 0x04).content;
      if (inner.length !== 32) fail('DataError', name + ' private key must be 32 bytes');
      var d = new U8(inner);
      data = { d: d, x: pubOf(d) }; type = 'private';
    } else if (format === 'jwk') {
      var jwk = keyData;
      if (jwk.d !== undefined) checkUsages(usages, pu[1]); else checkUsages(usages, pu[0]);
      if (jwk.kty !== 'OKP') fail('DataError', 'JWK kty must be OKP');
      if (jwk.crv !== name) fail('DataError', 'JWK crv does not match');
      if (name === 'Ed25519' && jwk.alg !== undefined && jwk.alg !== 'Ed25519' && jwk.alg !== 'EdDSA') fail('DataError', 'JWK alg does not match');
      jwkCheckCommon(jwk, extractable, usages, name === 'Ed25519' ? 'sig' : 'enc');
      if (jwk.x === undefined) fail('DataError', 'JWK x missing');
      var x = b64uDecode(jwk.x);
      checkPub(x);
      data = { x: x }; type = 'public';
      if (jwk.d !== undefined) {
        var dj = b64uDecode(jwk.d);
        if (dj.length !== 32) fail('DataError', 'JWK d length');
        if (pubOf(dj).join() !== x.join()) fail('DataError', 'public key does not match');
        data.d = dj; type = 'private';
      }
    } else {
      fail('NotSupportedError', 'format ' + format + ' is not supported for ' + name);
    }
    return makeKey(type, extractable, { name: name }, canonUsages(usages), data);
  }

  var FORMATS = ['raw', 'spki', 'pkcs8', 'jwk', 'raw-public', 'raw-private', 'raw-secret', 'raw-seed'];
  function formatOf(f) {
    var s = String(f);
    if (FORMATS.indexOf(s) < 0) throw new TypeErrorCtor('unknown key format ' + s);
    return s;
  }

  function importKeyOp(format, keyData, alg, extractable, usages) {
    var name = alg.name, key;
    if (name.slice(0, 4) === 'AES-' || name === 'HMAC' || name === 'HKDF' || name === 'PBKDF2') {
      key = importSecret(format, keyData, alg, extractable, usages);
    } else if (name.slice(0, 3) === 'RSA') {
      key = importRsa(format, keyData, alg, extractable, usages);
    } else if (name === 'ECDSA' || name === 'ECDH') {
      key = importEc(format, keyData, alg, extractable, usages);
    } else if (name === 'Ed25519' || name === 'X25519') {
      key = importOkp(format, keyData, alg, extractable, usages);
    } else {
      fail('NotSupportedError');
    }
    var s = keySlot.get(key);
    if ((s.type === 'secret' || s.type === 'private') && s.usages.length === 0) fail('SyntaxError', 'usages must not be empty');
    return key;
  }

  // ---- export -------------------------------------------------------------------
  function exportKeyOp(format, key) {
    var s = slotOf(key);
    var a = s.algorithm, d = s.data, name = a.name;
    if (name === 'HKDF' || name === 'PBKDF2') fail('NotSupportedError', name + ' keys cannot be exported');
    if (!s.extractable) fail('InvalidAccessError', 'key is not extractable');
    if (format === 'jwk') {
      var jwk = { ext: s.extractable, key_ops: s.usages.slice() };
      if (s.type === 'secret') {
        jwk.kty = 'oct';
        jwk.k = b64uEncode(d.raw);
        if (name === 'HMAC') jwk.alg = 'HS' + JWK_HASH_SUFFIX[a.hash.name];
        else if (name.slice(0, 4) === 'AES-') jwk.alg = 'A' + a.length + { 'AES-CBC': 'CBC', 'AES-CTR': 'CTR', 'AES-GCM': 'GCM', 'AES-KW': 'KW' }[name];
        else fail('NotSupportedError', name + ' keys cannot be exported');
      } else if (name.slice(0, 3) === 'RSA') {
        jwk.kty = 'RSA';
        jwk.alg = rsaJwkAlg(name, a.hash.name);
        jwk.n = b64uEncode(stripZeros(d.n)); jwk.e = b64uEncode(stripZeros(d.e));
        if (s.type === 'private') {
          ['d', 'p', 'q', 'dp', 'dq', 'qi'].forEach(function (k) { jwk[k] = b64uEncode(stripZeros(d[k])); });
        }
      } else if (name === 'ECDSA' || name === 'ECDH') {
        jwk.kty = 'EC'; jwk.crv = a.namedCurve;
        jwk.x = b64uEncode(d.x); jwk.y = b64uEncode(d.y);
        if (s.type === 'private') jwk.d = b64uEncode(d.d);
      } else {
        jwk.kty = 'OKP'; jwk.crv = name;
        if (name === 'Ed25519') jwk.alg = 'Ed25519';
        jwk.x = b64uEncode(d.x);
        if (s.type === 'private') jwk.d = b64uEncode(d.d);
      }
      return jwk;
    }
    if (format === 'raw' || format === 'raw-secret' || format === 'raw-public') {
      if (s.type === 'secret' && format !== 'raw-public') {
        if (name === 'HKDF' || name === 'PBKDF2') fail('NotSupportedError', name + ' keys cannot be exported');
        return ab(d.raw);
      }
      if (s.type === 'public' && format !== 'raw-secret') {
        if (name === 'ECDSA' || name === 'ECDH') return ab(concat(new U8([4]), concat(d.x, d.y)));
        if (name === 'Ed25519' || name === 'X25519') return ab(d.x);
      }
      fail('InvalidAccessError', 'raw export is not possible for this key');
    }
    if (format === 'spki') {
      if (s.type !== 'public') fail('InvalidAccessError', 'spki export needs a public key');
      if (name.slice(0, 3) === 'RSA') {
        return ab(derSeq(derSeq(derOid(OID_RSA), DER_NULL), derBits(derSeq(derInt(d.n), derInt(d.e)))));
      }
      if (name === 'ECDSA' || name === 'ECDH') {
        return ab(derSeq(derSeq(derOid(OID_EC), derOid(CURVE_OID[a.namedCurve])), derBits(concat(new U8([4]), concat(d.x, d.y)))));
      }
      return ab(derSeq(derSeq(derOid(name === 'Ed25519' ? OID_ED25519 : OID_X25519)), derBits(d.x)));
    }
    if (format === 'pkcs8') {
      if (s.type !== 'private') fail('InvalidAccessError', 'pkcs8 export needs a private key');
      var zero = derInt(new U8([0]));
      if (name.slice(0, 3) === 'RSA') {
        var rsaKey = derSeq(zero, derInt(d.n), derInt(d.e), derInt(d.d), derInt(d.p), derInt(d.q), derInt(d.dp), derInt(d.dq), derInt(d.qi));
        return ab(derSeq(zero, derSeq(derOid(OID_RSA), DER_NULL), derOctets(rsaKey)));
      }
      if (name === 'ECDSA' || name === 'ECDH') {
        var ecKey = derSeq(derInt(new U8([1])), derOctets(d.d), der(0xa1, derBits(concat(new U8([4]), concat(d.x, d.y)))));
        return ab(derSeq(zero, derSeq(derOid(OID_EC), derOid(CURVE_OID[a.namedCurve])), derOctets(ecKey)));
      }
      return ab(derSeq(zero, derSeq(derOid(name === 'Ed25519' ? OID_ED25519 : OID_X25519)), derOctets(derOctets(d.d))));
    }
    fail('NotSupportedError', 'format ' + format + ' is not supported');
  }

  // ---- derive ----------------------------------------------------------------------
  function deriveBitsOp(alg, slot, length) {
    var name = alg.name;
    if (name === 'ECDH' || name === 'X25519') {
      var pub = keySlot.get(alg.public);
      if (pub.type !== 'public') fail('InvalidAccessError', 'public must be a public key');
      if (pub.algorithm.name !== name) fail('InvalidAccessError', 'public key algorithm does not match');
      if (slot.type !== 'private') fail('InvalidAccessError', 'base key must be a private key');
      var secret;
      if (name === 'ECDH') {
        if (pub.algorithm.namedCurve !== slot.algorithm.namedCurve) fail('InvalidAccessError', 'curves do not match');
        try { secret = u8(N.ecdh(slot.algorithm.namedCurve, slot.data.d, pub.data.x, pub.data.y)); } catch (e) { fail('OperationError', e.message); }
      } else {
        try { secret = u8(N.x25519(slot.data.d, pub.data.x)); } catch (e) { fail('OperationError', e.message); }
      }
      if (length === null) return ab(secret);
      if (length > secret.length * 8) fail('OperationError', 'length is longer than the shared secret');
      var out = secret.slice(0, Math.ceil(length / 8));
      if (length % 8) out[out.length - 1] &= (0xff << (8 - length % 8)) & 0xff;
      return ab(out);
    }
    if (length === null) fail('OperationError', 'length is required');
    if (length % 8 !== 0) fail('OperationError', 'length must be a multiple of 8');
    try {
      if (name === 'HKDF') return N.hkdf(alg.hash.name, slot.data.raw, alg.salt, alg.info, length / 8);
      if (name === 'PBKDF2') {
        if (alg.iterations === 0) fail('OperationError', 'iterations must not be 0');
        return N.pbkdf2(alg.hash.name, slot.data.raw, alg.salt, alg.iterations, length / 8);
      }
    } catch (e) {
      if (e && typeof g.DOMException === 'function' && e instanceof g.DOMException) throw e;
      fail('OperationError', e && e.message);
    }
    fail('NotSupportedError');
  }

  function keyLengthOf(alg) {
    var name = alg.name;
    if (name.slice(0, 4) === 'AES-') {
      if ([128, 192, 256].indexOf(alg.length) < 0) fail('OperationError', 'AES key length must be 128, 192 or 256');
      return alg.length;
    }
    if (name === 'HMAC') return hmacLengthFor(alg);
    return null;
  }

  // ---- SubtleCrypto ------------------------------------------------------------
  function run(fn) {
    return new PromiseCtor(function (resolve, reject) {
      try { resolve(fn()); } catch (e) { reject(e); }
    });
  }
  function needArgs(args, n, what) {
    if (args.length < n) throw new TypeErrorCtor("Failed to execute '" + what + "' on 'SubtleCrypto': " + n + ' arguments required, but only ' + args.length + ' present.');
  }

  function SubtleCrypto() { throw new TypeErrorCtor('Illegal constructor'); }
  defineProperty(SubtleCrypto, 'prototype', { writable: false, enumerable: false, configurable: false });
  var SP = SubtleCrypto.prototype;
  defineProperty(SP, 'constructor', { value: SubtleCrypto, writable: true, enumerable: false, configurable: true });
  defineProperty(SP, Symbol.toStringTag, { value: 'SubtleCrypto', configurable: true });
  var subtleSlot = new WeakSet();
  function thisCheck(o) {
    if (!subtleSlot.has(o)) throw new TypeErrorCtor('Illegal invocation');
  }

  op(SP, 'encrypt', function encrypt(algorithm, key, data) {
    var args = arguments, self = this;
    return run(function () {
      thisCheck(self); needArgs(args, 3, 'encrypt');
      var alg = normalize('encrypt', algorithm);
      var bytes = bytesOf(data);
      var s = slotOf(key);
      checkKey(s, alg, 'encrypt');
      return cryptOp(true, alg, s, bytes);
    });
  }, 3);
  op(SP, 'decrypt', function decrypt(algorithm, key, data) {
    var args = arguments, self = this;
    return run(function () {
      thisCheck(self); needArgs(args, 3, 'decrypt');
      var alg = normalize('decrypt', algorithm);
      var bytes = bytesOf(data);
      var s = slotOf(key);
      checkKey(s, alg, 'decrypt');
      return cryptOp(false, alg, s, bytes);
    });
  }, 3);
  op(SP, 'sign', function sign(algorithm, key, data) {
    var args = arguments, self = this;
    return run(function () {
      thisCheck(self); needArgs(args, 3, 'sign');
      var alg = normalize('sign', algorithm);
      var bytes = bytesOf(data);
      var s = slotOf(key);
      checkKey(s, alg, 'sign');
      return signOp(alg, s, bytes);
    });
  }, 3);
  op(SP, 'verify', function verify(algorithm, key, signature, data) {
    var args = arguments, self = this;
    return run(function () {
      thisCheck(self); needArgs(args, 4, 'verify');
      var alg = normalize('verify', algorithm);
      var sig = bytesOf(signature, 'signature');
      var bytes = bytesOf(data);
      var s = slotOf(key);
      checkKey(s, alg, 'verify');
      return verifyOp(alg, s, sig, bytes);
    });
  }, 4);
  op(SP, 'digest', function digest(algorithm, data) {
    var args = arguments, self = this;
    return run(function () {
      thisCheck(self); needArgs(args, 2, 'digest');
      var alg = normalize('digest', algorithm);
      var bytes = bytesOf(data);
      return N.digest(alg.name, bytes);
    });
  }, 2);
  op(SP, 'generateKey', function generateKey(algorithm, extractable, keyUsages) {
    var args = arguments, self = this;
    return run(function () {
      thisCheck(self); needArgs(args, 3, 'generateKey');
      var ext = !!extractable;
      var usages = usagesOf(keyUsages);
      var alg = normalize('generateKey', algorithm);
      var r = generateKeyOp(alg, ext, usages);
      if (keySlot.has(r)) {
        var s = keySlot.get(r);
        if (s.usages.length === 0) fail('SyntaxError', 'usages must not be empty');
      } else if (keySlot.get(r.privateKey).usages.length === 0) {
        fail('SyntaxError', 'usages must not be empty');
      }
      return r;
    });
  }, 3);
  function importCore(format, keyData, algorithm, extractable, keyUsages) {
    // WebIDL conversion of the arguments first (the JWK dictionary is read
    // here), then normalization, then the copy of the key bytes.
    var fmt = formatOf(format);
    var data;
    var isJwk = !isBufferSource(keyData);
    if (isJwk) {
      if (!isObject(keyData)) throw new TypeErrorCtor('keyData must be a BufferSource or a JsonWebKey');
      data = jwkOf(keyData);
    }
    var ext = !!extractable;
    var usages = usagesOf(keyUsages);
    var alg = normalize('importKey', algorithm);
    if (fmt === 'jwk') {
      if (!isJwk) throw new TypeErrorCtor('jwk import needs a JsonWebKey');
    } else {
      if (isJwk) throw new TypeErrorCtor(fmt + ' import needs a BufferSource');
      data = bytesOf(keyData, 'keyData');
    }
    return importKeyOp(fmt, data, alg, ext, usages);
  }
  op(SP, 'importKey', function importKey(format, keyData, algorithm, extractable, keyUsages) {
    var args = arguments, self = this;
    return run(function () {
      thisCheck(self); needArgs(args, 5, 'importKey');
      return importCore(format, keyData, algorithm, extractable, keyUsages);
    });
  }, 5);
  op(SP, 'exportKey', function exportKey(format, key) {
    var args = arguments, self = this;
    return run(function () {
      thisCheck(self); needArgs(args, 2, 'exportKey');
      var fmt = formatOf(format);
      slotOf(key);
      return exportKeyOp(fmt, key);
    });
  }, 2);
  op(SP, 'deriveBits', function deriveBits(algorithm, baseKey, length) {
    var args = arguments, self = this;
    return run(function () {
      thisCheck(self); needArgs(args, 2, 'deriveBits');
      var alg = normalize('deriveBits', algorithm);
      var len = length === undefined || length === null ? null : enforceRange(length, U32, 'length');
      var s = slotOf(baseKey, 'baseKey');
      checkKey(s, alg, 'deriveBits');
      return deriveBitsOp(alg, s, len);
    });
  }, 2);
  op(SP, 'deriveKey', function deriveKey(algorithm, baseKey, derivedKeyType, extractable, keyUsages) {
    var args = arguments, self = this;
    return run(function () {
      thisCheck(self); needArgs(args, 5, 'deriveKey');
      var alg = normalize('deriveBits', algorithm);
      var importAlg = normalize('importKey', derivedKeyType);
      var lenAlg = normalize('getKeyLength', derivedKeyType);
      var ext = !!extractable;
      var usages = usagesOf(keyUsages);
      var s = slotOf(baseKey, 'baseKey');
      checkKey(s, alg, 'deriveKey');
      var length = keyLengthOf(lenAlg);
      var bits = u8(deriveBitsOp(alg, s, length));
      return importKeyOp('raw', bits, importAlg, ext, usages);
    });
  }, 5);
  function wrapAlgOf(opName, fallback, algorithm) {
    try {
      return normalize(opName, algorithm);
    } catch (e) {
      if (e && e.name === 'NotSupportedError') return normalize(fallback, algorithm);
      throw e;
    }
  }
  op(SP, 'wrapKey', function wrapKey(format, key, wrappingKey, wrapAlgorithm) {
    var args = arguments, self = this;
    return run(function () {
      thisCheck(self); needArgs(args, 4, 'wrapKey');
      var fmt = formatOf(format);
      var alg = wrapAlgOf('wrapKey', 'encrypt', wrapAlgorithm);
      var ws = slotOf(wrappingKey, 'wrappingKey');
      if (ws.algorithm.name !== alg.name) fail('InvalidAccessError', 'wrapping key algorithm does not match');
      if (ws.usages.indexOf('wrapKey') < 0) fail('InvalidAccessError', 'wrapping key cannot wrap');
      var exported = exportKeyOp(fmt, key);
      var bytes;
      if (fmt === 'jwk') bytes = new TextEncoder().encode(JSON.stringify(exported));
      else bytes = u8(exported);
      if (alg.name === 'AES-KW') {
        try { return N.aesKw(true, ws.data.raw, bytes); } catch (e) { fail('OperationError', e.message); }
      }
      return cryptOp(true, alg, ws, bytes);
    });
  }, 4);
  op(SP, 'unwrapKey', function unwrapKey(format, wrappedKey, unwrappingKey, unwrapAlgorithm, unwrappedKeyAlgorithm, extractable, keyUsages) {
    var args = arguments, self = this;
    return run(function () {
      thisCheck(self); needArgs(args, 7, 'unwrapKey');
      var fmt = formatOf(format);
      var wrapped = bytesOf(wrappedKey, 'wrappedKey');
      var alg = wrapAlgOf('unwrapKey', 'decrypt', unwrapAlgorithm);
      var keyAlg = normalize('importKey', unwrappedKeyAlgorithm);
      var ext = !!extractable;
      var usages = usagesOf(keyUsages);
      var us = slotOf(unwrappingKey, 'unwrappingKey');
      if (us.algorithm.name !== alg.name) fail('InvalidAccessError', 'unwrapping key algorithm does not match');
      if (us.usages.indexOf('unwrapKey') < 0) fail('InvalidAccessError', 'unwrapping key cannot unwrap');
      var bytes;
      if (alg.name === 'AES-KW') {
        try { bytes = u8(N.aesKw(false, us.data.raw, wrapped)); } catch (e) { fail('OperationError', e.message); }
      } else {
        bytes = u8(cryptOp(false, alg, us, wrapped));
      }
      var data;
      if (fmt === 'jwk') {
        var parsed;
        try { parsed = JSON.parse(new TextDecoder().decode(bytes)); } catch (e) { fail('DataError', 'wrapped JWK is not JSON'); }
        data = jwkOf(parsed);
      } else {
        data = bytes;
      }
      return importKeyOp(fmt, data, keyAlg, ext, usages);
    });
  }, 7);

  var subtle = create(SP);
  subtleSlot.add(subtle);
  N.setSubtle(subtle);
  [['SubtleCrypto', SubtleCrypto], ['CryptoKey', CryptoKey]].forEach(function (e) {
    defineProperty(g, e[0], { value: e[1], writable: true, enumerable: false, configurable: true });
  });
})
